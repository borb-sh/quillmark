//! Merge repair. A single step cannot break the model, but two valid
//! concurrent edits can merge into a state no step allows. [`repair`] makes any
//! such state valid by fixed rules: it is total, the same raw value always
//! repairs to the same content, and every id it mints is derived from the ids
//! involved, so two peers repairing one state at once write the same value.

use crate::ids::{MAX_ID_LEN, is_id};
use crate::lines::island::IslandType;
use crate::lines::model::json_depth_exceeds;
use crate::lines::normalize::is_bidi_char;
use crate::model::{
    Anchor, Block, BlockKind, Container, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT,
    Invariant, Island, Mark, NOTE_SLOT, Usv, is_lang, is_text_char,
};
use crate::table::{Row, Table};
use crate::wire::{
    ParseError, anchor_from_value, array, block_from_value, container_from_value, is_v1,
    mark_from_value, shape, str_key,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A content value as a merge left it: shaped as [`Content`], its invariants
/// unchecked, and its islands matched to their slots by id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Raw {
    pub text: String,
    pub blocks: Vec<Block>,
    pub containers: Vec<Container>,
    pub marks: Vec<Mark>,
    pub anchors: Vec<RawAnchor>,
    /// The island id each [`ISLAND_SLOT`] of `text` carries, in slot order;
    /// `None` where a slot carries none.
    pub slot_ids: Vec<Option<String>>,
    pub islands: BTreeMap<String, RawIsland>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawAnchor {
    pub anchor: Anchor,
    /// Placed as a range: at zero width it has collapsed.
    pub range: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawIsland {
    pub island_type: IslandType,
    pub props: Value,
    pub slots: BTreeMap<String, Raw>,
}

/// What [`repair`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repair {
    /// A path named a container with no record; the id left the path.
    MissingContainer { block: String, container: String },
    /// A container sat under two paths; its blocks took its first block's.
    TwoParents { container: String },
    /// A container spanned two runs; the later run's is `fresh`.
    SplitRun { container: String, fresh: String },
    /// A slot named no island record; its character left the text.
    OrphanSlot { island: Option<String> },
    /// An island record no slot named was removed.
    OrphanIsland { island: String },
    /// A second slot named an island the first already held; it holds a copy
    /// under `fresh`.
    SharedIsland { island: String, fresh: String },
    /// Text beside a block island split into its own block, `block`.
    SplitBlock { island: String, block: String },
    /// A range anchor collapsed to zero width and was dropped.
    CollapsedAnchor { id: String },
    /// Anything else that broke the invariant, fixed by the nearest valid value.
    Invalid(Invariant),
    /// A repair inside an island's slot.
    InSlot {
        island: String,
        slot: String,
        repair: Box<Repair>,
    },
}

impl Repair {
    pub fn to_value(&self) -> Value {
        match self {
            Repair::MissingContainer { block, container } => {
                json!({"repair": "missing_container", "block": block, "container": container})
            }
            Repair::TwoParents { container } => {
                json!({"repair": "two_parents", "container": container})
            }
            Repair::SplitRun { container, fresh } => {
                json!({"repair": "split_run", "container": container, "fresh": fresh})
            }
            Repair::OrphanSlot { island } => json!({"repair": "orphan_slot", "island": island}),
            Repair::OrphanIsland { island } => json!({"repair": "orphan_island", "island": island}),
            Repair::SharedIsland { island, fresh } => {
                json!({"repair": "shared_island", "island": island, "fresh": fresh})
            }
            Repair::SplitBlock { island, block } => {
                json!({"repair": "split_block", "island": island, "block": block})
            }
            Repair::CollapsedAnchor { id } => json!({"repair": "collapsed_anchor", "id": id}),
            Repair::Invalid(inv) => json!({"repair": "invalid", "reason": inv.to_string()}),
            Repair::InSlot {
                island,
                slot,
                repair,
            } => {
                json!({"repair": "in_slot", "island": island, "slot": slot, "inner": repair.to_value()})
            }
        }
    }

    /// The anchor this repair dropped, at any depth.
    pub fn dropped_anchor(&self) -> Option<&str> {
        match self {
            Repair::CollapsedAnchor { id } => Some(id),
            Repair::InSlot { repair, .. } => repair.dropped_anchor(),
            _ => None,
        }
    }
}

impl From<&Content> for Raw {
    fn from(c: &Content) -> Self {
        Raw {
            text: c.text.clone(),
            blocks: c.blocks.clone(),
            containers: c.containers.clone(),
            marks: c.marks.clone(),
            anchors: c
                .anchors
                .iter()
                .map(|a| RawAnchor {
                    anchor: a.clone(),
                    range: !a.is_point(),
                })
                .collect(),
            slot_ids: c.islands.iter().map(|i| Some(i.id.clone())).collect(),
            islands: c
                .islands
                .iter()
                .map(|i| {
                    let slots = i
                        .slots
                        .iter()
                        .map(|(k, s)| (k.clone(), Raw::from(s)))
                        .collect();
                    (
                        i.id.clone(),
                        RawIsland {
                            island_type: i.island_type,
                            props: i.props.clone(),
                            slots,
                        },
                    )
                })
                .collect(),
        }
    }
}

impl Raw {
    /// Decode a raw value: a content value whose `islands` may be an object
    /// keyed by island id, with `slot_ids` naming the island each slot carries
    /// (`null` for none), and whose anchors may carry `range: true`. An
    /// `islands` array pairs with the slots in order. Refused only where the
    /// value is not shaped as content at all.
    pub fn from_value(v: &Value) -> Result<Raw, ParseError> {
        if crate::model::json_too_deep(v) {
            return Err(ParseError::Invalid(Invariant::JsonTooDeep {
                what: "content",
                max: crate::MAX_JSON_DEPTH,
            }));
        }
        if is_v1(v) {
            return Ok(Raw::from(&Content::from_value(v)?));
        }
        decode(v)
    }
}

fn decode(v: &Value) -> Result<Raw, ParseError> {
    if !v.is_object() {
        return Err(shape("content is not an object"));
    }
    let anchors = array(v, "anchors")?
        .iter()
        .map(|a| {
            Ok(RawAnchor {
                anchor: anchor_from_value(a)?,
                range: a.get("range") == Some(&Value::Bool(true)),
            })
        })
        .collect::<Result<_, ParseError>>()?;
    let mut raw = Raw {
        text: str_key(v, "text")?,
        blocks: array(v, "blocks")?
            .iter()
            .map(block_from_value)
            .collect::<Result<_, _>>()?,
        containers: array(v, "containers")?
            .iter()
            .map(container_from_value)
            .collect::<Result<_, _>>()?,
        marks: array(v, "marks")?
            .iter()
            .map(mark_from_value)
            .collect::<Result<_, _>>()?,
        anchors,
        slot_ids: Vec::new(),
        islands: BTreeMap::new(),
    };
    match v.get("islands") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for i in list {
                let id = str_key(i, "id")?;
                raw.slot_ids.push(Some(id.clone()));
                raw.islands.entry(id).or_insert(island(i)?);
            }
        }
        Some(Value::Object(map)) => {
            for (id, i) in map {
                raw.islands.insert(id.clone(), island(i)?);
            }
            raw.slot_ids = array(v, "slot_ids")?
                .iter()
                .map(|s| match s {
                    Value::Null => Ok(None),
                    Value::String(s) => Ok(Some(s.clone())),
                    _ => Err(shape("a `slot_ids` entry is not a string or null")),
                })
                .collect::<Result<_, _>>()?;
        }
        Some(_) => return Err(shape("`islands` is not an array or an object")),
    }
    Ok(raw)
}

fn island(v: &Value) -> Result<RawIsland, ParseError> {
    let ty = str_key(v, "type")?;
    let island_type = IslandType::parse(&ty).ok_or(ParseError::UnknownName {
        axis: "island type",
        name: ty,
    })?;
    let props = match v.get("props") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err(shape("`props` is not an object")),
    };
    let slots = match v.get("slots") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, s)| Ok((k.clone(), decode(s)?)))
            .collect::<Result<_, ParseError>>()?,
        Some(_) => return Err(shape("`slots` is not an object")),
    };
    Ok(RawIsland {
        island_type,
        props,
        slots,
    })
}

/// `raw` made valid, and what changed. A valid value comes back as it is,
/// normalized, with nothing reported.
pub fn repair(raw: Raw) -> (Content, Vec<Repair>) {
    let mut out = Vec::new();
    let mut c = field(raw, &mut out);
    dedupe_anchors(&mut c, &mut HashSet::new(), &mut out);
    c.normalize();
    if let Err(inv) = c.validate() {
        out.push(Repair::Invalid(inv));
        c = flatten(c);
        c.normalize();
        if c.validate().is_err() {
            c = Content::empty();
        }
    }
    (c, out)
}

/// Mints ids no part of a scope holds.
struct Ids(HashSet<String>);

impl Ids {
    fn derive(&mut self, parts: &[&str]) -> String {
        let base: String = parts
            .join("-")
            .chars()
            .filter(|&c| {
                !c.is_whitespace() && !c.is_control() && !is_bidi_char(c) && c != ISLAND_SLOT
            })
            .take(MAX_ID_LEN - 12)
            .collect();
        let base = if base.is_empty() {
            "x".to_string()
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2u64;
        while self.0.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.0.insert(id.clone());
        id
    }
}

/// Offsets past characters taken out of the text.
struct Removed(Vec<Usv>);

impl Removed {
    fn map(&self, o: Usv) -> Usv {
        o - self.0.partition_point(|&p| p < o)
    }
}

fn remove_chars(
    text: &mut Vec<char>,
    at: &BTreeSet<Usv>,
    marks: &mut [Mark],
    anchors: &mut [&mut Anchor],
) {
    if at.is_empty() {
        return;
    }
    let removed = Removed(at.iter().copied().collect());
    let mut k = 0;
    text.retain(|_| {
        k += 1;
        !at.contains(&(k - 1))
    });
    for m in marks {
        (m.start, m.end) = (removed.map(m.start), removed.map(m.end));
    }
    for a in anchors {
        (a.start, a.end) = (removed.map(a.start), removed.map(a.end));
    }
}

fn field(raw: Raw, out: &mut Vec<Repair>) -> Content {
    let mut ids = Ids(raw
        .blocks
        .iter()
        .map(|b| b.id.clone())
        .chain(raw.containers.iter().map(|c| c.id.clone()))
        .chain(raw.islands.keys().cloned())
        .collect());
    let Raw {
        text,
        mut blocks,
        containers,
        mut marks,
        mut anchors,
        slot_ids,
        islands,
    } = raw;
    let mut text: Vec<char> = text.chars().collect();
    for a in &mut anchors {
        a.range |= a.anchor.start < a.anchor.end;
    }

    let mut forbidden = BTreeSet::new();
    let mut named = BTreeSet::new();
    for (k, &c) in text.iter().enumerate() {
        if !is_text_char(c) {
            forbidden.insert(k);
            if named.insert(c) {
                out.push(Repair::Invalid(Invariant::ForbiddenChar(c)));
            }
        }
    }
    let mut anchor_refs: Vec<&mut Anchor> = anchors.iter_mut().map(|a| &mut a.anchor).collect();
    remove_chars(&mut text, &forbidden, &mut marks, &mut anchor_refs);

    let slots: Vec<Usv> = text
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == ISLAND_SLOT)
        .map(|(k, _)| k)
        .collect();
    let mut kept = Vec::new();
    let mut gone = BTreeSet::new();
    let mut held: HashSet<String> = HashSet::new();
    for (k, &at) in slots.iter().enumerate() {
        let carried = slot_ids.get(k).cloned().flatten();
        let Some((id, record)) = carried
            .as_ref()
            .and_then(|id| Some((id.clone(), islands.get(id)?.clone())))
        else {
            out.push(Repair::OrphanSlot { island: carried });
            gone.insert(at);
            continue;
        };
        let island_id = if is_id(&id) && !held.contains(&id) {
            id.clone()
        } else if is_id(&id) {
            let fresh = ids.derive(&[&id]);
            out.push(Repair::SharedIsland {
                island: id.clone(),
                fresh: fresh.clone(),
            });
            fresh
        } else {
            out.push(Repair::Invalid(Invariant::BadId { id: id.clone() }));
            ids.derive(&["isl"])
        };
        held.insert(island_id.clone());
        match island_of(island_id, record, out) {
            Some(island) => kept.push(island),
            None => {
                gone.insert(at);
            }
        }
    }
    let carried: HashSet<&String> = slot_ids.iter().flatten().collect();
    for id in islands.keys().filter(|id| !carried.contains(id)) {
        out.push(Repair::OrphanIsland { island: id.clone() });
    }
    let mut anchor_refs: Vec<&mut Anchor> = anchors.iter_mut().map(|a| &mut a.anchor).collect();
    remove_chars(&mut text, &gone, &mut marks, &mut anchor_refs);

    let segments = text.iter().filter(|&&c| c == '\n').count() + 1;
    if blocks.len() != segments {
        out.push(Repair::Invalid(Invariant::BlockCountMismatch {
            blocks: blocks.len(),
            segments,
        }));
        blocks.truncate(segments);
        while blocks.len() < segments {
            let base = blocks.last().map_or("b".to_string(), |b| b.id.clone());
            blocks.push(Block::new(ids.derive(&[&base]), BlockKind::Para));
        }
    }

    let mut taken = HashSet::new();
    for b in &mut blocks {
        if !is_id(&b.id) || taken.contains(&b.id) {
            out.push(Repair::Invalid(if is_id(&b.id) {
                Invariant::DuplicateId { id: b.id.clone() }
            } else {
                Invariant::BadId { id: b.id.clone() }
            }));
            b.id = ids.derive(&[if is_id(&b.id) { &b.id } else { "b" }]);
        }
        taken.insert(b.id.clone());
        match &mut b.kind {
            BlockKind::Heading { level } if !(1..=6).contains(&*level) => {
                out.push(Repair::Invalid(Invariant::BadHeadingLevel(*level)));
                *level = (*level).clamp(1, 6);
            }
            BlockKind::Code { lang } if lang.as_deref().is_some_and(|l| !is_lang(l)) => {
                out.push(Repair::Invalid(Invariant::BadLang(
                    lang.take().unwrap_or_default(),
                )));
            }
            _ => {}
        }
    }

    let mut kinds: HashMap<String, ContainerKind> = HashMap::new();
    let mut records = Vec::new();
    for c in containers {
        if !is_id(&c.id) {
            out.push(Repair::Invalid(Invariant::BadId { id: c.id }));
        } else if kinds.contains_key(&c.id) {
            out.push(Repair::Invalid(Invariant::DuplicateId { id: c.id }));
        } else {
            kinds.insert(c.id.clone(), c.kind.clone());
            records.push(c);
        }
    }
    for b in &mut blocks {
        let block = b.id.clone();
        b.path.retain(|id| {
            let known = kinds.contains_key(id);
            if !known {
                out.push(Repair::MissingContainer {
                    block: block.clone(),
                    container: id.clone(),
                });
            }
            known
        });
    }
    nest_all(&mut blocks, &kinds, out);
    let bound = blocks.len() * (2 * crate::MAX_NESTING_DEPTH + 2) + 8;
    let mut settled = false;
    for _ in 0..bound {
        if !relate(&mut blocks, &mut kinds, &mut records, &mut ids, out) {
            settled = true;
            break;
        }
        nest_all(&mut blocks, &kinds, out);
    }
    if !settled {
        for b in &mut blocks {
            b.path.clear();
        }
    }

    let mut start = 0;
    for b in &mut blocks {
        let end = text[start..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(text.len(), |n| start + n);
        let seg = &mut text[start..end];
        match b.kind {
            BlockKind::Heading { .. } if seg.contains(&HARD_BREAK) => {
                out.push(Repair::Invalid(Invariant::BreakInHeading {
                    block: b.id.clone(),
                }));
                for c in seg.iter_mut().filter(|c| **c == HARD_BREAK) {
                    *c = ' ';
                }
            }
            BlockKind::Rule if !seg.is_empty() => {
                out.push(Repair::Invalid(Invariant::TextInRule {
                    block: b.id.clone(),
                }));
                b.kind = BlockKind::Para;
            }
            BlockKind::Code { .. } if seg.contains(&ISLAND_SLOT) => {
                out.push(Repair::Invalid(Invariant::SlotInCode {
                    block: b.id.clone(),
                }));
                b.kind = BlockKind::Para;
            }
            _ => {}
        }
        start = end + 1;
    }

    let (text, blocks) =
        split_block_islands(text, blocks, &kept, &mut marks, &mut anchors, &mut ids, out);

    let len = text.len();
    marks.retain_mut(|m| {
        if m.start < m.end && m.end <= len {
            return true;
        }
        out.push(Repair::Invalid(Invariant::BadMark {
            start: m.start,
            end: m.end,
            len,
        }));
        m.end = m.end.min(len);
        m.start < m.end
    });
    let anchors = anchors
        .into_iter()
        .filter_map(|RawAnchor { mut anchor, range }| {
            if anchor.id.is_empty() {
                out.push(Repair::Invalid(Invariant::BadAnchor { id: anchor.id }));
                return None;
            }
            anchor.start = anchor.start.min(len);
            anchor.end = anchor.end.min(len);
            if anchor.start > anchor.end || (range && anchor.start == anchor.end) {
                out.push(Repair::CollapsedAnchor { id: anchor.id });
                return None;
            }
            Some(anchor)
        })
        .collect();

    Content {
        text: text.into_iter().collect(),
        blocks,
        containers: records,
        marks,
        anchors,
        islands: kept,
    }
}

fn island_of(id: String, raw: RawIsland, out: &mut Vec<Repair>) -> Option<Island> {
    let bad = |reason| {
        Repair::Invalid(Invariant::BadIsland {
            id: id.clone(),
            reason,
        })
    };
    if json_depth_exceeds(&raw.props, crate::MAX_JSON_DEPTH) {
        out.push(Repair::Invalid(Invariant::JsonTooDeep {
            what: "island props",
            max: crate::MAX_JSON_DEPTH,
        }));
        return None;
    }
    let mut props = if raw.props.is_object() {
        raw.props
    } else {
        Value::Object(Map::new())
    };
    let mut slots: BTreeMap<String, Content> = BTreeMap::new();
    for (key, slot) in raw.slots {
        let mut inner = Vec::new();
        let mut c = field(slot, &mut inner);
        if !c.is_slot_shape() {
            inner.push(Repair::Invalid(Invariant::BadSlot {
                island: id.clone(),
                slot: key.clone(),
            }));
            c = flatten(c);
        }
        out.extend(inner.into_iter().map(|r| Repair::InSlot {
            island: id.clone(),
            slot: key.clone(),
            repair: Box::new(r),
        }));
        slots.insert(key, c);
    }
    match raw.island_type {
        IslandType::Table => {
            if !table(&id, &mut props, &mut slots, out) {
                return None;
            }
        }
        IslandType::Footnote => {
            if slots.len() != 1 || !slots.contains_key(NOTE_SLOT) {
                out.push(bad("a footnote holds one slot, `note`"));
                let note = slots.remove(NOTE_SLOT).unwrap_or_default();
                slots = BTreeMap::from([(NOTE_SLOT.to_string(), note)]);
            }
        }
        IslandType::Math | IslandType::Image => {
            let key = if raw.island_type == IslandType::Math {
                "tex"
            } else {
                "url"
            };
            if !props.get(key).is_some_and(Value::is_string) {
                out.push(bad(if key == "tex" {
                    "an equation's `tex` is not a string"
                } else {
                    "an image's `url` is not a string"
                }));
                return None;
            }
            if !slots.is_empty() {
                out.push(bad("an equation or an image holds no slot"));
                slots.clear();
            }
        }
    }
    Some(Island {
        id,
        island_type: raw.island_type,
        props,
        slots,
    })
}

/// A table's grid made whole: columns and rows with valid unique ids, every
/// cell naming a slot of its own, an empty slot minted for a cell that names
/// none, and a slot no cell names removed. False where no grid is left.
fn table(
    island: &str,
    props: &mut Value,
    slots: &mut BTreeMap<String, Content>,
    out: &mut Vec<Repair>,
) -> bool {
    let bad = |reason| {
        Repair::Invalid(Invariant::BadIsland {
            id: island.to_string(),
            reason,
        })
    };
    let Some(obj) = props.as_object_mut() else {
        return false;
    };
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let raw_columns = strings(obj.get("columns"));
    let raw_rows: Vec<Value> = obj
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut ids = Ids(raw_columns
        .iter()
        .cloned()
        .chain(
            raw_rows
                .iter()
                .filter_map(|r| r.get("id")?.as_str().map(str::to_string)),
        )
        .chain(
            raw_rows
                .iter()
                .filter_map(|r| r.get("cells")?.as_object())
                .flat_map(|c| {
                    c.values()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                }),
        )
        .chain(slots.keys().cloned())
        .collect());
    let mut seen = HashSet::new();
    let columns: Vec<String> = raw_columns
        .into_iter()
        .filter(|c| is_id(c) && seen.insert(c.clone()))
        .collect();
    let mut row_ids = HashSet::new();
    let mut named = HashSet::new();
    let mut rows = Vec::new();
    for r in &raw_rows {
        let Some(cells) = r.get("cells").and_then(Value::as_object) else {
            continue;
        };
        let id = match r.get("id").and_then(Value::as_str) {
            Some(id) if is_id(id) && !row_ids.contains(id) => id.to_string(),
            Some(id) if is_id(id) => ids.derive(&[id]),
            _ => ids.derive(&["r"]),
        };
        row_ids.insert(id.clone());
        let cells = columns
            .iter()
            .map(|col| match cells.get(col).and_then(Value::as_str) {
                Some(s) if is_id(s) && slots.contains_key(s) && !named.contains(s) => {
                    named.insert(s.to_string());
                    s.to_string()
                }
                _ => {
                    let fresh = ids.derive(&[&id, col]);
                    slots.insert(fresh.clone(), Content::empty());
                    named.insert(fresh.clone());
                    fresh
                }
            })
            .collect();
        rows.push(Row { id, cells });
    }
    if columns.is_empty() || rows.is_empty() {
        out.push(bad("a table holds at least one row and one column"));
        return false;
    }
    let cells = columns.len().saturating_mul(rows.len());
    if cells > crate::MAX_TABLE_CELLS {
        out.push(Repair::Invalid(Invariant::TableTooLarge {
            cells,
            max: crate::MAX_TABLE_CELLS,
        }));
        return false;
    }
    let before = (
        obj.get("columns").cloned(),
        obj.get("rows").cloned(),
        slots.len(),
    );
    slots.retain(|k, _| named.contains(k));
    let table = Table { columns, rows };
    table.write_into(obj);
    if (
        obj.get("columns").cloned(),
        obj.get("rows").cloned(),
        slots.len(),
    ) != before
    {
        out.push(bad("a table's slots are not the ones its rows name"));
    }
    true
}

fn is_list(kinds: &HashMap<String, ContainerKind>, id: &str) -> bool {
    matches!(kinds.get(id), Some(ContainerKind::List { .. }))
}

/// A path whose kinds nest: each id once, an item only right after a list, a
/// list only right before an item, at most [`crate::MAX_NESTING_DEPTH`] deep.
fn nest(path: &[String], kinds: &HashMap<String, ContainerKind>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for id in path {
        if !seen.insert(id) {
            continue;
        }
        let after_list = out.last().is_some_and(|l| is_list(kinds, l));
        let is_item = matches!(kinds.get(id), Some(ContainerKind::Item { .. }));
        if is_item && !after_list {
            continue;
        }
        if !is_item && after_list {
            out.pop();
        }
        out.push(id.clone());
    }
    loop {
        if out.last().is_some_and(|l| is_list(kinds, l)) {
            out.pop();
        }
        if crate::model::depth(&out, |id| kinds.get(id).cloned()) <= crate::MAX_NESTING_DEPTH {
            return out;
        }
        out.pop();
    }
}

fn nest_all(blocks: &mut [Block], kinds: &HashMap<String, ContainerKind>, out: &mut Vec<Repair>) {
    for b in blocks {
        let nested = nest(&b.path, kinds);
        if nested != b.path {
            out.push(Repair::Invalid(Invariant::BadPath {
                block: b.id.clone(),
            }));
            b.path = nested;
        }
    }
}

/// Fixes the first container, in first-use order, that sits under two paths
/// or spans two runs; false when none does.
fn relate(
    blocks: &mut [Block],
    kinds: &mut HashMap<String, ContainerKind>,
    records: &mut Vec<Container>,
    ids: &mut Ids,
    out: &mut Vec<Repair>,
) -> bool {
    let mut order: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for b in blocks.iter() {
        for id in &b.path {
            if seen.insert(id.clone()) {
                order.push(id.clone());
            }
        }
    }
    for x in order {
        let holders: Vec<(usize, usize)> = blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.path.iter().position(|p| *p == x).map(|j| (i, j)))
            .collect();
        let (first, j0) = holders[0];
        let prefix = blocks[first].path[..j0].to_vec();
        let mut moved = false;
        for &(i, j) in &holders[1..] {
            if blocks[i].path[..j] != prefix[..] {
                let rest = blocks[i].path[j..].to_vec();
                blocks[i].path = prefix.iter().cloned().chain(rest).collect();
                moved = true;
            }
        }
        if moved {
            out.push(Repair::TwoParents { container: x });
            return true;
        }
        let Some(k) = (1..holders.len()).find(|&k| holders[k].0 != holders[k - 1].0 + 1) else {
            continue;
        };
        let run_first = blocks[holders[k].0].id.clone();
        let mut renamed: HashMap<String, String> = HashMap::new();
        let mut i = holders[k].0;
        while i < blocks.len() {
            let Some(j) = blocks[i].path.iter().position(|p| *p == x) else {
                break;
            };
            for p in j..blocks[i].path.len() {
                let y = blocks[i].path[p].clone();
                let fresh = renamed.entry(y.clone()).or_insert_with(|| {
                    let fresh = ids.derive(&[&y, &run_first]);
                    let kind = kinds[&y].clone();
                    kinds.insert(fresh.clone(), kind.clone());
                    records.push(Container::new(fresh.clone(), kind));
                    fresh
                });
                blocks[i].path[p] = fresh.clone();
            }
            i += 1;
        }
        let fresh = renamed[&x].clone();
        out.push(Repair::SplitRun {
            container: x,
            fresh,
        });
        return true;
    }
    false
}

/// Every block island alone in a paragraph: the text beside one splits into
/// blocks of its own, each with a fresh id, and the island keeps the record.
fn split_block_islands(
    text: Vec<char>,
    blocks: Vec<Block>,
    islands: &[Island],
    marks: &mut [Mark],
    anchors: &mut [RawAnchor],
    ids: &mut Ids,
    out: &mut Vec<Repair>,
) -> (Vec<char>, Vec<Block>) {
    let mut new_text = Vec::with_capacity(text.len());
    let mut new_blocks = Vec::with_capacity(blocks.len());
    let mut breaks: Vec<Usv> = Vec::new();
    let mut slot = 0usize;
    let mut start = 0usize;
    for b in blocks {
        let end = text[start..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(text.len(), |n| start + n);
        let seg = &text[start..end];
        let mut pieces: Vec<(Usv, Usv, Option<&Island>)> = Vec::new();
        let mut from = 0;
        for (k, &c) in seg.iter().enumerate() {
            if c != ISLAND_SLOT {
                continue;
            }
            let island = &islands[slot];
            slot += 1;
            if island.island_type.block_only() {
                if from < k {
                    pieces.push((from, k, None));
                }
                pieces.push((k, k + 1, Some(island)));
                from = k + 1;
            }
        }
        if pieces.is_empty() {
            new_text.extend_from_slice(seg);
            new_blocks.push(b);
        } else {
            if from < seg.len() {
                pieces.push((from, seg.len(), None));
            }
            let mut record = Some(b.clone());
            if pieces.len() == 1 && b.kind != BlockKind::Para {
                out.push(Repair::Invalid(Invariant::BlockIslandNotAlone {
                    island: pieces[0].2.map_or(String::new(), |i| i.id.clone()),
                }));
            }
            for (n, &(s, e, island)) in pieces.iter().enumerate() {
                if n > 0 {
                    breaks.push(start + s);
                    new_text.push('\n');
                }
                new_text.extend_from_slice(&seg[s..e]);
                let near = island
                    .or_else(|| pieces.get(n + 1).and_then(|p| p.2))
                    .or_else(|| pieces[..n].iter().rev().find_map(|p| p.2));
                let near = near.map_or("isl", |i| i.id.as_str());
                match (island, record.take()) {
                    (Some(_), Some(mut keep)) => {
                        keep.kind = BlockKind::Para;
                        new_blocks.push(keep);
                    }
                    (Some(_), None) => {
                        let fresh = ids.derive(&[&b.id, near]);
                        out.push(Repair::SplitBlock {
                            island: near.to_string(),
                            block: fresh.clone(),
                        });
                        new_blocks
                            .push(Block::new(fresh, BlockKind::Para).with_path(b.path.clone()));
                    }
                    (None, keep) => {
                        record = keep;
                        let fresh = ids.derive(&[&b.id, near]);
                        out.push(Repair::SplitBlock {
                            island: near.to_string(),
                            block: fresh.clone(),
                        });
                        new_blocks.push(Block {
                            id: fresh,
                            kind: b.kind.clone(),
                            path: b.path.clone(),
                        });
                    }
                }
            }
        }
        if end < text.len() {
            new_text.push('\n');
        }
        start = end + 1;
    }
    if !breaks.is_empty() {
        let shift = |o: Usv, edge_moves: bool| {
            o + breaks.partition_point(|&p| if edge_moves { p <= o } else { p < o })
        };
        for m in marks {
            (m.start, m.end) = (shift(m.start, true), shift(m.end, false));
        }
        for a in anchors {
            let a = &mut a.anchor;
            let point = a.start == a.end;
            (a.start, a.end) = (shift(a.start, !point), shift(a.end, false));
        }
    }
    (new_text, new_blocks)
}

/// Each anchor id once over the text and every slot, the first kept.
fn dedupe_anchors(c: &mut Content, seen: &mut HashSet<String>, out: &mut Vec<Repair>) {
    c.anchors.retain(|a| {
        let fresh = seen.insert(a.id.clone());
        if !fresh {
            out.push(Repair::Invalid(Invariant::DuplicateId { id: a.id.clone() }));
        }
        fresh
    });
    for island in &mut c.islands {
        for (key, slot) in &mut island.slots {
            let mut inner = Vec::new();
            dedupe_anchors(slot, seen, &mut inner);
            out.extend(inner.into_iter().map(|r| Repair::InSlot {
                island: island.id.clone(),
                slot: key.clone(),
                repair: Box::new(r),
            }));
        }
    }
}

/// Paragraphs in no container, holding no island: what a slot may hold, and
/// the last resort for a field.
fn flatten(mut c: Content) -> Content {
    let mut text: Vec<char> = c.text.chars().collect();
    let slots: BTreeSet<Usv> = text
        .iter()
        .enumerate()
        .filter(|(_, ch)| **ch == ISLAND_SLOT)
        .map(|(k, _)| k)
        .collect();
    let mut anchors: Vec<&mut Anchor> = c.anchors.iter_mut().collect();
    remove_chars(&mut text, &slots, &mut c.marks, &mut anchors);
    c.anchors.retain(|a| a.start <= a.end);
    c.text = text.into_iter().collect();
    c.islands.clear();
    c.containers.clear();
    for b in &mut c.blocks {
        b.kind = BlockKind::Para;
        b.path.clear();
    }
    c.marks.retain(|m| m.start < m.end);
    c
}
