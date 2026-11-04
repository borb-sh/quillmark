//! What a stretch of a document's history did to one value, as Automerge's
//! patches say it, lowered to the view: offsets into the value's text, and
//! records named by their ids.
//!
//! Text patches come in Automerge's order, each at offsets into the text as
//! the patches before it left it. The records, anchors and containers a
//! stretch touched follow, each as it reads at its end. A value whose first
//! block marker the stretch moved, or whose shape it left where no text
//! patch reaches, answers one [`Action::Value`] with the whole value.

use super::value::{
    self, ANCHOR, CONTAINERS, POINTS, SLOTS, TEXT, block_json, container_json, id_of, is_embed,
    mark_json, obj, point_at,
};
use automerge::marks::{Mark as AmMark, MarkSet};
use automerge::patches::PatchAction;
use automerge::{
    AutoCommit, ChangeHash, ObjId, ObjType, Prop, ReadDoc, ScalarValue, Value as AmValue, legacy,
};
use quillmark_content::model::{
    Anchor, Block, Container, Content, ISLAND_SLOT, Island, Mark, MarkKind,
};
use quillmark_content::repair::{Raw, repair};
use quillmark_content::units::Units;
use quillmark_content::wire;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The island and slot a nested value sits in.
pub type SlotStep = (String, String);

/// One change to a value: the slot it reaches, outermost first, and what
/// it did there.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub slot: Vec<SlotStep>,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// `text` put at `at`: each `\n` opens a block whose record is the next
    /// of `blocks`, each island character holds the next of `islands`, and
    /// `marks` is the formatting the text carries, offsets into it.
    Insert {
        at: usize,
        text: String,
        blocks: Vec<Block>,
        islands: Vec<Island>,
        marks: Vec<Mark>,
    },
    /// `len` characters deleted at `at`.
    Delete {
        at: usize,
        len: usize,
    },
    /// A block record as it reads now.
    Block(Block),
    /// An island as it reads now, its slots whole.
    Island(Island),
    Container(Container),
    ContainerRemoved {
        id: String,
    },
    /// Formatting put over `[start, end)`.
    Mark(Mark),
    /// Formatting of `tag` taken off `[start, end)`.
    Unmark {
        start: usize,
        end: usize,
        tag: String,
    },
    /// An anchor as it reads now.
    Anchor(Anchor),
    AnchorRemoved {
        id: String,
    },
    /// The whole value as it reads now, read through repair.
    Value(Content),
}

fn island_in(i: &Island, u: impl Units + Copy) -> Value {
    let mut v = wire::island_value(i);
    if let Some(Value::Object(slots)) = v.get_mut("slots") {
        for (name, slot) in &i.slots {
            slots.insert(name.clone(), slot.to_value_in(u));
        }
    }
    v
}

impl Patch {
    pub fn to_value(&self) -> Value {
        self.to_value_in(quillmark_content::units::Scalars)
    }

    /// The patch as JSON, each value it carries whole, an island's slots
    /// included, counted in `u`; its own offsets are written as they are,
    /// as [`in_units`] counts them.
    pub fn to_value_in(&self, u: impl Units + Copy) -> Value {
        let mut v = match &self.action {
            Action::Insert {
                at,
                text,
                blocks,
                islands,
                marks,
            } => json!({
                "insert": {
                    "at": at,
                    "text": text,
                    "blocks": blocks.iter().map(wire::block_value).collect::<Vec<_>>(),
                    "islands": islands.iter().map(|i| island_in(i, u)).collect::<Vec<_>>(),
                    "marks": marks.iter().map(wire::mark_value).collect::<Vec<_>>(),
                }
            }),
            Action::Delete { at, len } => json!({ "delete": { "at": at, "len": len } }),
            Action::Block(b) => json!({ "block": wire::block_value(b) }),
            Action::Island(i) => json!({ "island": island_in(i, u) }),
            Action::Container(c) => json!({ "container": wire::container_value(c) }),
            Action::ContainerRemoved { id } => json!({ "containerRemoved": { "id": id } }),
            Action::Mark(m) => json!({ "mark": wire::mark_value(m) }),
            Action::Unmark { start, end, tag } => {
                json!({ "unmark": { "start": start, "end": end, "tag": tag } })
            }
            Action::Anchor(a) => json!({ "anchor": wire::anchor_value(a) }),
            Action::AnchorRemoved { id } => json!({ "anchorRemoved": { "id": id } }),
            Action::Value(c) => json!({ "value": c.to_value_in(u) }),
        };
        if !self.slot.is_empty() {
            v["slot"] = Value::Array(
                self.slot
                    .iter()
                    .map(|(island, slot)| json!({ "island": island, "slot": slot }))
                    .collect(),
            );
        }
        v
    }
}

impl Patch {
    /// A patch as [`to_value`](Self::to_value) writes it.
    pub fn from_value(v: &Value) -> Result<Patch, String> {
        let Value::Object(o) = v else { return Err("a patch is an object".into()) };
        let slot = match o.get("slot") {
            None => Vec::new(),
            Some(Value::Array(steps)) => steps
                .iter()
                .map(|s| match (s.get("island").and_then(Value::as_str), s.get("slot").and_then(Value::as_str)) {
                    (Some(i), Some(n)) => Ok((i.to_string(), n.to_string())),
                    _ => Err("a slot step names its island and slot".to_string()),
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("`slot` is a list".into()),
        };
        let mut kinds = o.iter().filter(|(k, _)| k.as_str() != "slot");
        let (Some((kind, body)), None) = (kinds.next(), kinds.next()) else { return Err("a patch holds one action".into()) };
        let n = |k: &str| body.get(k).and_then(Value::as_u64).map(|x| x as usize).ok_or_else(|| format!("`{kind}.{k}` is a count"));
        let st = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("`{kind}.{k}` is a string"));
        let list = |k: &str| body.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let e = |x: wire::ParseError| format!("`{kind}`: {x}");
        let action = match kind.as_str() {
            "insert" => Action::Insert {
                at: n("at")?,
                text: st("text")?,
                blocks: list("blocks").iter().map(wire::block_from_value).collect::<Result<_, _>>().map_err(e)?,
                islands: list("islands").iter().map(wire::island_from_value).collect::<Result<_, _>>().map_err(e)?,
                marks: list("marks").iter().map(wire::mark_from_value).collect::<Result<_, _>>().map_err(e)?,
            },
            "delete" => Action::Delete { at: n("at")?, len: n("len")? },
            "block" => Action::Block(wire::block_from_value(body).map_err(e)?),
            "island" => Action::Island(wire::island_from_value(body).map_err(e)?),
            "container" => Action::Container(wire::container_from_value(body).map_err(e)?),
            "containerRemoved" => Action::ContainerRemoved { id: st("id")? },
            "mark" => Action::Mark(wire::mark_from_value(body).map_err(e)?),
            "unmark" => Action::Unmark { start: n("start")?, end: n("end")?, tag: st("tag")? },
            "anchor" => Action::Anchor(wire::anchor_from_value(body).map_err(e)?),
            "anchorRemoved" => Action::AnchorRemoved { id: st("id")? },
            "value" => Action::Value(Content::from_value(body).map_err(e)?),
            other => return Err(format!("`{other}` is no patch")),
        };
        Ok(Patch { slot, action })
    }
}

/// Where in a value a patch lands.
enum Site {
    /// The value's text object.
    Text,
    /// A block marker or an island, by its object, or the slots map of one.
    Record(ObjId),
    Containers,
    Points,
    /// The value's own map, or anything no other site names.
    Whole,
}

/// What one value's patches gathered.
#[derive(Default)]
struct Gathered {
    actions: Vec<Action>,
    whole: bool,
    records: Vec<ObjId>,
    put: BTreeSet<String>,
    containers: bool,
    anchors: BTreeSet<String>,
    /// Marks landed on the text. Their patches are not lowered as they come:
    /// Automerge's diff leaves out a mark covering only embedded objects and
    /// can name the wrong text for a mark, so the formatting is settled
    /// against the view.
    formatted: bool,
    /// The value's root and text, once seen.
    root: Option<ObjId>,
}

/// The patches taking the value `root` from `before` to `after`, read in
/// `doc` as it stands at `after`, and the value they leave: `was`, the
/// value as it read at `before`, with the patches applied.
pub(crate) fn lower(doc: &mut AutoCommit, root: &ObjId, before: &[ChangeHash], after: &[ChangeHash], was: &Content) -> (Vec<Patch>, Content) {
    let raw = doc.diff_obj(root, before, after, true).unwrap_or_default();
    let mut values: BTreeMap<Vec<SlotStep>, Gathered> = BTreeMap::new();
    values.entry(Vec::new()).or_default().root = Some(root.clone());
    for p in &raw {
        let Some((slot, v, site)) = locate(root, &p.path, &p.obj) else { continue };
        let g = values.entry(slot).or_default();
        g.root = Some(v);
        match site {
            Site::Text => text(doc, &p.action, g),
            Site::Record(m) => {
                if !g.records.contains(&m) {
                    g.records.push(m);
                }
            }
            Site::Containers => g.containers = true,
            Site::Points => match &p.action {
                PatchAction::PutMap { key, .. } | PatchAction::DeleteMap { key } => {
                    g.anchors.insert(key.clone());
                }
                _ => g.whole = true,
            },
            Site::Whole => g.whole = true,
        }
    }
    for t in marked_texts(doc, before) {
        let Ok(parents) = doc.parents(&t) else { continue };
        let Some(mut path) = parents.map(|p| p.visible.then_some((p.obj, p.prop))).collect::<Option<Vec<_>>>() else { continue };
        path.reverse();
        if let Some((slot, v, Site::Text)) = locate(root, &path, &t) {
            let g = values.entry(slot).or_default();
            g.root = Some(v);
            g.formatted = true;
        }
    }
    let mut out = Vec::new();
    let wholes: Vec<Vec<SlotStep>> = values.iter().filter(|(_, g)| g.whole).map(|(s, _)| s.clone()).collect();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for (slot, g) in values {
        if wholes.iter().any(|w| slot.starts_with(w) && slot.len() > w.len()) || slot.iter().any(|(island, _)| covered.contains(island)) {
            continue;
        }
        let Some(v) = g.root.clone() else { continue };
        let t = value::text(doc, &v);
        let earlier = find(was, &slot);
        let (Some(t), Some(earlier), false) = (t, earlier, g.whole) else {
            if let Some(content) = view_of(doc, &v) {
                out.push(Patch { slot, action: Action::Value(content) });
            }
            continue;
        };
        covered.extend(g.put.iter().cloned());
        let edited = g.formatted || !g.actions.is_empty();
        let mut actions = g.actions;
        let mut unblocked = false;
        let marks = doc.marks(&t).unwrap_or_default();
        if edited {
            let mut cand = earlier.clone();
            for a in &actions {
                if let Action::Delete { at, len } = a {
                    unblocked |= cand.text.chars().skip(*at).take(*len).any(|c| c == '\n');
                }
                act(&mut cand, a);
            }
            let want = formatting(&marks, &cand.text);
            actions.extend(settle(&cand.marks, &want));
        }
        for m in &g.records {
            if is_embed(doc, m) {
                if let Some(i) = island_of(doc, m).filter(|i| !g.put.contains(&i.id)) {
                    covered.insert(i.id.clone());
                    actions.push(Action::Island(i));
                }
            } else if let Some(b) = block_of(doc, m).filter(|b| !g.put.contains(&b.id)) {
                actions.push(Action::Block(b));
            }
        }
        let restructured = unblocked || !g.records.is_empty() || actions.iter().any(|a| matches!(a, Action::Insert { blocks, .. } if !blocks.is_empty()));
        if g.containers || restructured {
            containers(doc, &v, before, &mut actions);
        }
        let mut anchors = g.anchors.clone();
        if edited {
            anchors.extend(earlier.anchors.iter().map(|a| a.id.clone()));
            anchors.extend(anchor_ids(doc, &v, &marks));
        }
        for id in &anchors {
            actions.push(match anchor_at(doc, &v, &t, &marks, id) {
                Some(a) => Action::Anchor(a),
                None => Action::AnchorRemoved { id: id.clone() },
            });
        }
        out.extend(actions.into_iter().map(|action| Patch { slot: slot.clone(), action }));
    }
    let mut after = was.clone();
    for p in &out {
        apply(&mut after, p);
    }
    after.normalize();
    (out, after)
}

/// The slot path of the value a patch lands in, that value's root, and the
/// site in it. `None` for a patch outside `root`.
fn locate(
    root: &ObjId,
    path: &[(ObjId, Prop)],
    obj: &ObjId,
) -> Option<(Vec<SlotStep>, ObjId, Site)> {
    let k = path.iter().position(|(o, _)| o == root);
    let mut slot = Vec::new();
    let mut v = root.clone();
    let Some(mut i) = k else {
        return (obj == root).then(|| (slot, v, Site::Whole));
    };
    let next = |j: usize| -> ObjId {
        path.get(j + 1)
            .map(|(o, _)| o.clone())
            .unwrap_or_else(|| obj.clone())
    };
    loop {
        let (_, prop) = &path[i];
        let key = match prop {
            Prop::Map(k) => k.as_str(),
            Prop::Seq(_) => return Some((slot, v, Site::Whole)),
        };
        match key {
            TEXT => {
                if i + 1 == path.len() {
                    return Some((slot, v, Site::Text));
                }
                let elem = next(i + 1);
                if i + 2 == path.len() {
                    return Some((slot, v, Site::Record(elem)));
                }
                match &path[i + 2].1 {
                    Prop::Map(k) if k == SLOTS && i + 3 < path.len() => {
                        let Prop::Map(name) = &path[i + 3].1 else {
                            return Some((slot, v, Site::Record(elem)));
                        };
                        slot.push((id_of(&elem), name.clone()));
                        v = next(i + 3);
                        i += 4;
                        if i >= path.len() {
                            return Some((slot, v, Site::Whole));
                        }
                    }
                    _ => return Some((slot, v, Site::Record(elem))),
                }
            }
            CONTAINERS => return Some((slot, v, Site::Containers)),
            POINTS => return Some((slot, v, Site::Points)),
            _ => return Some((slot, v, Site::Whole)),
        }
    }
}

/// Gathers a patch on a value's text.
fn text(doc: &AutoCommit, action: &PatchAction, g: &mut Gathered) {
    match action {
        PatchAction::SpliceText {
            index,
            value,
            marks,
        } => {
            let Some(at) = index.checked_sub(1) else {
                return g.whole = true;
            };
            let text = value.make_string();
            let len = text.chars().count();
            g.actions.push(Action::Insert {
                at,
                text,
                blocks: Vec::new(),
                islands: Vec::new(),
                marks: marks_of(marks.as_ref(), len),
            });
        }
        PatchAction::Insert { index, values } => {
            let Some(at) = index.checked_sub(1) else {
                return g.whole = true;
            };
            let (mut text, mut blocks, mut islands) = (String::new(), Vec::new(), Vec::new());
            for (v, id, _) in values.iter() {
                match v {
                    AmValue::Object(ObjType::Map) if is_embed(doc, id) => {
                        match island_of(doc, id) {
                            Some(i) => {
                                text.push(ISLAND_SLOT);
                                g.put.insert(i.id.clone());
                                islands.push(i);
                            }
                            None => return g.whole = true,
                        }
                    }
                    AmValue::Object(ObjType::Map) => match block_of(doc, id) {
                        Some(b) => {
                            text.push('\n');
                            g.put.insert(b.id.clone());
                            blocks.push(b);
                        }
                        None => return g.whole = true,
                    },
                    _ => return g.whole = true,
                }
            }
            g.actions.push(Action::Insert {
                at,
                text,
                blocks,
                islands,
                marks: Vec::new(),
            });
        }
        PatchAction::DeleteSeq { index, length } => {
            let Some(at) = index.checked_sub(1) else {
                return g.whole = true;
            };
            g.actions.push(Action::Delete { at, len: *length });
        }
        PatchAction::Mark { .. } => g.formatted = true,
        _ => g.whole = true,
    }
}

/// The text objects the changes since `before` place marks on.
fn marked_texts(doc: &mut AutoCommit, before: &[ChangeHash]) -> BTreeSet<ObjId> {
    let mut ids = BTreeSet::new();
    for change in doc.get_changes(before) {
        for op in change.decode().operations {
            if let (legacy::OpType::MarkBegin(_), legacy::ObjectId::Id(id)) = (&op.action, &op.obj)
            {
                ids.insert(id.to_string());
            }
        }
    }
    ids.iter()
        .filter_map(|id| doc.import_obj(id).ok())
        .collect()
}

/// The value at `slot` of `c`.
fn find<'a>(c: &'a Content, slot: &[SlotStep]) -> Option<&'a Content> {
    let Some(((island, name), rest)) = slot.split_first() else { return Some(c) };
    find(c.islands.iter().find(|i| i.id == *island)?.slots.get(name)?, rest)
}

fn find_mut<'a>(c: &'a mut Content, slot: &[SlotStep]) -> Option<&'a mut Content> {
    let Some(((island, name), rest)) = slot.split_first() else { return Some(c) };
    find_mut(c.islands.iter_mut().find(|i| i.id == *island)?.slots.get_mut(name)?, rest)
}

/// Applies `p` to `c` as a host holding `c` applies it, and answers whether
/// it reached a value `c` holds. Formatting is held per character: text an
/// insert puts carries only the insert's marks, and a mark puts its kind
/// over its range in place of any of the same tag.
pub fn apply(c: &mut Content, p: &Patch) -> bool {
    match find_mut(c, &p.slot) {
        Some(c) => {
            act(c, &p.action);
            true
        }
        None => false,
    }
}

fn byte_at(text: &str, at: usize) -> usize {
    text.char_indices().nth(at).map_or(text.len(), |(b, _)| b)
}

fn count(text: &str, upto: usize, ch: char) -> usize {
    text.chars().take(upto).filter(|c| *c == ch).count()
}

/// `c`'s marks of `tag` taken off `[start, end)`.
fn unmark(c: &mut Content, start: usize, end: usize, tag: &str) {
    let mut kept = Vec::with_capacity(c.marks.len());
    for m in c.marks.drain(..) {
        if m.kind.tag() != tag || m.end <= start || m.start >= end {
            kept.push(m);
            continue;
        }
        if m.start < start {
            kept.push(Mark::new(m.start, start, m.kind.clone()));
        }
        if m.end > end {
            kept.push(Mark::new(end, m.end, m.kind.clone()));
        }
    }
    c.marks = kept;
}

fn act(c: &mut Content, a: &Action) {
    match a {
        Action::Insert { at, text, blocks, islands, marks } => {
            let at = (*at).min(c.text.chars().count());
            let len = text.chars().count();
            let (b, i) = (count(&c.text, at, '\n') + 1, count(&c.text, at, ISLAND_SLOT));
            let byte = byte_at(&c.text, at);
            c.text.insert_str(byte, text);
            let b = b.min(c.blocks.len());
            c.blocks.splice(b..b, blocks.iter().cloned());
            let i = i.min(c.islands.len());
            c.islands.splice(i..i, islands.iter().cloned());
            let mut shifted = Vec::with_capacity(c.marks.len() + marks.len());
            for m in c.marks.drain(..) {
                if m.end <= at {
                    shifted.push(m);
                } else if m.start >= at {
                    shifted.push(Mark::new(m.start + len, m.end + len, m.kind));
                } else {
                    shifted.push(Mark::new(m.start, at, m.kind.clone()));
                    shifted.push(Mark::new(at + len, m.end + len, m.kind));
                }
            }
            shifted.extend(marks.iter().map(|m| Mark::new(m.start + at, m.end + at, m.kind.clone())));
            c.marks = shifted;
            for x in &mut c.anchors {
                if x.start >= at {
                    x.start += len;
                }
                if x.end > at || (x.end == at && x.start > at) {
                    x.end += len;
                }
            }
        }
        Action::Delete { at, len } => {
            let total = c.text.chars().count();
            let at = (*at).min(total);
            let end = (at + len).min(total);
            let gone: String = c.text.chars().skip(at).take(end - at).collect();
            let (b, i) = (count(&c.text, at, '\n') + 1, count(&c.text, at, ISLAND_SLOT));
            let (from, to) = (byte_at(&c.text, at), byte_at(&c.text, end));
            c.text.replace_range(from..to, "");
            let nb = gone.chars().filter(|c| *c == '\n').count();
            let ni = gone.chars().filter(|c| *c == ISLAND_SLOT).count();
            c.blocks.drain(b.min(c.blocks.len())..(b + nb).min(c.blocks.len()));
            c.islands.drain(i.min(c.islands.len())..(i + ni).min(c.islands.len()));
            let clip = |x: usize| if x <= at { x } else if x >= end { x - (end - at) } else { at };
            c.marks = c.marks.drain(..).map(|m| Mark::new(clip(m.start), clip(m.end), m.kind)).filter(|m| m.start < m.end).collect();
            for x in &mut c.anchors {
                (x.start, x.end) = (clip(x.start), clip(x.end));
            }
        }
        Action::Block(b) => {
            if let Some(x) = c.blocks.iter_mut().find(|x| x.id == b.id) {
                *x = b.clone();
            }
        }
        Action::Island(i) => {
            if let Some(x) = c.islands.iter_mut().find(|x| x.id == i.id) {
                *x = i.clone();
            }
        }
        Action::Container(k) => match c.containers.iter_mut().find(|x| x.id == k.id) {
            Some(x) => *x = k.clone(),
            None => c.containers.push(k.clone()),
        },
        Action::ContainerRemoved { id } => c.containers.retain(|x| x.id != *id),
        Action::Mark(m) => {
            unmark(c, m.start, m.end, m.kind.tag());
            c.marks.push(m.clone());
        }
        Action::Unmark { start, end, tag } => unmark(c, *start, *end, tag),
        Action::Anchor(x) => match c.anchors.iter_mut().find(|y| y.id == x.id) {
            Some(y) => *y = x.clone(),
            None => c.anchors.push(x.clone()),
        },
        Action::AnchorRemoved { id } => c.anchors.retain(|x| x.id != *id),
        Action::Value(v) => *c = v.clone(),
    }
}

/// The formatting a text object's marks give `text`, as the view reads it.
fn formatting(marks: &[AmMark], text: &str) -> Vec<Mark> {
    let mut c = Content { text: text.to_string(), ..Content::empty() };
    c.marks = marks
        .iter()
        .filter(|m| !m.name().starts_with(ANCHOR) && m.end > 1)
        .filter_map(|m| wire::mark_from_value(&mark_json(m.name(), m.value(), m.start.saturating_sub(1), m.end - 1)?).ok())
        .collect();
    c.normalize();
    c.marks
}

/// Each tag's ranges in `marks`, sorted, each with its kind.
fn by_tag(marks: &[Mark]) -> BTreeMap<String, Vec<(usize, usize, MarkKind)>> {
    let mut out: BTreeMap<String, Vec<(usize, usize, MarkKind)>> = BTreeMap::new();
    for m in marks.iter().filter(|m| m.start < m.end) {
        out.entry(m.kind.tag().to_string()).or_default().push((m.start, m.end, m.kind.clone()));
    }
    for v in out.values_mut() {
        v.sort_by_key(|(s, e, _)| (*s, *e));
    }
    out
}

/// The kind of `ranges` over `at`, past ranges ending by `at` skipped from `k`.
fn kind_over<'a>(ranges: &'a [(usize, usize, MarkKind)], k: &mut usize, at: usize) -> Option<&'a MarkKind> {
    while *k < ranges.len() && ranges[*k].1 <= at {
        *k += 1;
    }
    ranges.get(*k).filter(|(s, _, _)| *s <= at).map(|(_, _, kind)| kind)
}

/// The marks and unmarks that take the formatting `has` to `want`, per
/// character.
fn settle(has: &[Mark], want: &[Mark]) -> Vec<Action> {
    let (has, want) = (by_tag(has), by_tag(want));
    let tags: BTreeSet<&String> = has.keys().chain(want.keys()).collect();
    let none = Vec::new();
    let mut out = Vec::new();
    for tag in tags {
        let (h, w) = (has.get(tag).unwrap_or(&none), want.get(tag).unwrap_or(&none));
        let mut cuts: Vec<usize> = h.iter().chain(w).flat_map(|(s, e, _)| [*s, *e]).collect();
        cuts.sort_unstable();
        cuts.dedup();
        let (mut kh, mut kw) = (0, 0);
        let mut run: Option<(usize, usize, Option<MarkKind>)> = None;
        let flush = |run: &mut Option<(usize, usize, Option<MarkKind>)>, out: &mut Vec<Action>| {
            if let Some((s, e, kind)) = run.take() {
                out.push(match kind {
                    Some(kind) => Action::Mark(Mark::new(s, e, kind)),
                    None => Action::Unmark { start: s, end: e, tag: tag.clone() },
                });
            }
        };
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (x, y) = (kind_over(h, &mut kh, a), kind_over(w, &mut kw, a));
            if x == y {
                flush(&mut run, &mut out);
                continue;
            }
            match &mut run {
                Some((_, e, kind)) if *e == a && kind.as_ref() == y => *e = b,
                _ => {
                    flush(&mut run, &mut out);
                    run = Some((a, b, y.cloned()));
                }
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

fn kind_of(name: &str, value: &ScalarValue) -> Option<MarkKind> {
    wire::mark_from_value(&mark_json(name, value, 0, 1)?)
        .ok()
        .map(|m| m.kind)
}

fn marks_of(set: Option<&MarkSet>, len: usize) -> Vec<Mark> {
    let Some(set) = set else { return Vec::new() };
    set.iter()
        .filter(|(name, _)| !name.starts_with(ANCHOR))
        .filter_map(|(name, value)| Some(Mark::new(0, len, kind_of(name, value)?)))
        .filter(|m| len > 0 && m.start < m.end)
        .collect()
}

fn block_of(doc: &AutoCommit, m: &ObjId) -> Option<Block> {
    doc.object_type(m).ok()?;
    wire::block_from_value(&block_json(doc, m)).ok()
}

fn island_of(doc: &AutoCommit, m: &ObjId) -> Option<Island> {
    let v = value::island_json(doc, m)?;
    let mut island =
        wire::island_from_value(&json!({ "id": id_of(m), "type": v["type"], "props": v["props"] }))
            .ok()?;
    if let Some(s) = obj(doc, m, SLOTS) {
        for k in doc.keys(&s).collect::<Vec<_>>() {
            if let Some(root) = obj(doc, &s, &k) {
                island.slots.insert(k, view_of(doc, &root)?);
            }
        }
    }
    Some(island)
}

fn view_of(doc: &AutoCommit, root: &ObjId) -> Option<Content> {
    let raw = Raw::from_value(&value::read(doc, root)).ok()?;
    Some(repair(raw).0)
}

fn anchor_at(doc: &AutoCommit, root: &ObjId, t: &ObjId, marks: &[AmMark], id: &str) -> Option<Anchor> {
    let name = format!("{ANCHOR}{id}");
    let (mut start, mut end) = (usize::MAX, 0);
    for m in marks.iter().filter(|m| m.name() == name) {
        start = start.min(m.start.saturating_sub(1));
        end = end.max(m.end.saturating_sub(1));
    }
    if start <= end && start != usize::MAX {
        return Some(Anchor::new(id, start, end));
    }
    let points = obj(doc, root, POINTS)?;
    let at = point_at(doc, &points, t, id)?;
    Some(Anchor::new(id, at, at))
}

/// The id of each anchor the value `root`, its text's marks `marks`, holds.
fn anchor_ids(doc: &AutoCommit, root: &ObjId, marks: &[AmMark]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = marks.iter().filter_map(|m| m.name().strip_prefix(ANCHOR).map(str::to_string)).collect();
    if let Some(points) = obj(doc, root, POINTS) {
        out.extend(doc.keys(&points));
    }
    out
}

/// Each container record of `root` a block sits in now, as it reads, and
/// each one that held at `before` and holds no block now.
fn containers(doc: &AutoCommit, root: &ObjId, before: &[ChangeHash], out: &mut Vec<Action>) {
    let Some(cs) = obj(doc, root, CONTAINERS) else {
        return;
    };
    let was: BTreeSet<String> = (0..doc.length_at(&cs, before))
        .filter_map(|i| match doc.get_at(&cs, i, before).ok()?? {
            (AmValue::Object(ObjType::Map), id) => Some(id_of(&id)),
            _ => None,
        })
        .collect();
    let used: BTreeSet<String> = match value::text(doc, root) {
        Some(t) => value::elements(doc, &t)
            .into_iter()
            .filter_map(|e| match e {
                value::Element::Block(m) => block_of(doc, &m),
                _ => None,
            })
            .flat_map(|b| b.path)
            .collect(),
        None => BTreeSet::new(),
    };
    let mut now = BTreeSet::new();
    for (v, id) in doc.values(&cs) {
        if !matches!(v, AmValue::Object(ObjType::Map)) {
            continue;
        }
        if let Some(c) = container_json(doc, &id)
            .and_then(|c| wire::container_from_value(&c).ok())
            .filter(|c| used.contains(&c.id))
        {
            now.insert(c.id.clone());
            out.push(Action::Container(c));
        }
    }
    out.extend(
        was.difference(&now)
            .map(|id| Action::ContainerRemoved { id: id.clone() }),
    );
}

/// `patches` with their offsets counted in `u`, on the texts `before`
/// names for each slot the patches reach, as they read where the patches
/// start.
pub fn in_units(
    patches: &[Patch],
    before: &BTreeMap<Vec<SlotStep>, String>,
    u: impl Units,
) -> Vec<Patch> {
    let mut texts: BTreeMap<Vec<SlotStep>, Vec<char>> = before
        .iter()
        .map(|(k, v)| (k.clone(), v.chars().collect()))
        .collect();
    let to = |t: &[char], at: usize| -> usize {
        t[..at.min(t.len())].iter().map(|c| u.char_len(*c)).sum()
    };
    let span = |t: &[char], at: usize, len: usize| -> usize {
        t[at.min(t.len())..(at + len).min(t.len())]
            .iter()
            .map(|c| u.char_len(*c))
            .sum()
    };
    let mut out = Vec::with_capacity(patches.len());
    for p in patches {
        let t = texts.entry(p.slot.clone()).or_default();
        let action = match &p.action {
            Action::Insert {
                at,
                text,
                blocks,
                islands,
                marks,
            } => {
                let a = to(t, *at);
                let inserted: Vec<char> = text.chars().collect();
                let marks = marks
                    .iter()
                    .map(|m| {
                        Mark::new(to(&inserted, m.start), to(&inserted, m.end), m.kind.clone())
                    })
                    .collect();
                let tail = t.split_off((*at).min(t.len()));
                t.extend(inserted);
                t.extend(tail);
                Action::Insert {
                    at: a,
                    text: text.clone(),
                    blocks: blocks.clone(),
                    islands: islands.clone(),
                    marks,
                }
            }
            Action::Delete { at, len } => {
                let (a, l) = (to(t, *at), span(t, *at, *len));
                let from = (*at).min(t.len());
                t.drain(from..(at + len).min(t.len()));
                Action::Delete { at: a, len: l }
            }
            Action::Mark(m) => {
                Action::Mark(Mark::new(to(t, m.start), to(t, m.end), m.kind.clone()))
            }
            Action::Unmark { start, end, tag } => Action::Unmark {
                start: to(t, *start),
                end: to(t, *end),
                tag: tag.clone(),
            },
            Action::Anchor(a) => {
                Action::Anchor(Anchor::new(a.id.clone(), to(t, a.start), to(t, a.end)))
            }
            Action::Value(c) => {
                *t = c.text.chars().collect();
                Action::Value(c.clone())
            }
            other => other.clone(),
        };
        out.push(Patch {
            slot: p.slot.clone(),
            action,
        });
    }
    out
}

/// The text of each value `patches` reach in `c`, the value they start on.
pub fn texts(c: &Content, patches: &[Patch]) -> BTreeMap<Vec<SlotStep>, String> {
    let mut out = BTreeMap::new();
    for p in patches {
        if !out.contains_key(&p.slot) {
            out.insert(p.slot.clone(), find(c, &p.slot).map(|v| v.text.clone()).unwrap_or_default());
        }
    }
    out
}

/// The spans of the value's own text the patches put, in scalars on
/// `after`, the text they leave; offsets in the patches are counted in `u`.
/// A patch of the whole value puts all of it.
pub fn inserted_spans(patches: &[Patch], after: &str, u: impl Units) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for p in patches.iter().filter(|p| p.slot.is_empty()) {
        let (at, deleted, inserted) = match &p.action {
            Action::Insert { at, text, .. } => (*at, 0, u.len(text)),
            Action::Delete { at, len } => (*at, *len, 0),
            Action::Value(c) => {
                spans = vec![(0, u.len(&c.text))];
                continue;
            }
            _ => continue,
        };
        let end = at + deleted;
        let shift = |p: usize, inside: usize| match p {
            p if p <= at => p,
            p if p >= end => p - deleted + inserted,
            _ => inside,
        };
        spans = spans.into_iter().map(|(a, b)| (shift(a, at + inserted), shift(b, at))).filter(|(a, b)| a < b).collect();
        if inserted > 0 {
            spans.push((at, at + inserted));
        }
    }
    spans.sort_unstable();
    spans.into_iter().filter_map(|(a, b)| Some((quillmark_content::units::to_scalars(u, after, a)?, quillmark_content::units::to_scalars(u, after, b)?))).collect()
}
