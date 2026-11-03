//! The mapping between a content value and the Automerge objects that hold it.
//!
//! A value is a map: `text` a Text object, `containers` a map of container
//! records by id, and `codec`. The text opens with a block marker holding
//! block 0's record; every later `\n` is a block marker and every `U+FFFC` an
//! island marker, so a value offset `n` is Automerge index `n + 1`. An island
//! marker holds `id`, `type`, `props` as Automerge maps and lists, and
//! `slots`, a map of values of this same shape.
//!
//! Formatting is Automerge marks that grow at neither edge. An anchor is
//! marks too, since a mark's edges converge as formatting does: a range is a
//! mark named `@` and its id over its text, and a point is a mark named `<`
//! and its id on the element before it, the opening block marker for a point
//! at 0, with a mark named `>` and its id on the character after it where one
//! follows. A point reads at the end of its `<` mark, or where that element
//! is gone, at the start of its `>` mark, so it survives a deletion of either
//! side and is gone only when both are. A point is marks rather than an
//! Automerge cursor: a cursor is an id kept beside the text, which a merge
//! does not move and a save does not hold, while a mark is the text's own
//! history and merges as a range's edges do.

use automerge::marks::{ExpandMark, Mark as AmMark};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ReadDoc, ScalarValue, Value as AmValue};
use quillmark_content::model::{Block, Content, ISLAND_SLOT, Island, Mark, MarkKind};
use quillmark_content::wire;
use serde_json::{Map, Number, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(crate) const KIND: &str = "k";
pub(crate) const BLOCK: &str = "b";
pub(crate) const ISLAND: &str = "i";
pub(crate) const RANGE: char = '@';
pub(crate) const BEFORE: char = '<';
pub(crate) const AFTER: char = '>';
const EXT_SEP: char = '\u{1f}';

/// The map a document holds its value in, under [`KEY`](crate::KEY).
pub(crate) fn value_root(doc: &AutoCommit) -> Option<ObjId> {
    obj(doc, &automerge::ROOT, crate::KEY)
}

/// Whether `root` holds a value in the shape this module writes, at every
/// depth: a map holding a map `containers` and a Text `text` that opens with
/// a block marker and holds only map markers and characters, no character a
/// `\n` or `U+FFFC` that a marker spells, and on each island marker a map
/// `slots` of values of this same shape. Every read and write of this module
/// takes such a value without a panic.
pub(crate) fn shaped(doc: &AutoCommit, root: &ObjId) -> bool {
    let is = |o: &ObjId, ty: ObjType| doc.object_type(o).ok() == Some(ty);
    if !is(root, ObjType::Map) || !obj(doc, root, "containers").is_some_and(|cs| is(&cs, ObjType::Map)) {
        return false;
    }
    let Some(t) = obj(doc, root, "text").filter(|t| is(t, ObjType::Text)) else { return false };
    let mut n = 0;
    for (v, id) in doc.values(&t) {
        match v {
            AmValue::Object(ObjType::Map) => {
                let island = get_str(doc, &id, KIND).is_some_and(|k| k != BLOCK);
                if n == 0 && island {
                    return false;
                }
                if island {
                    let Some(slots) = obj(doc, &id, "slots").filter(|s| is(s, ObjType::Map)) else { return false };
                    for k in doc.keys(&slots).collect::<Vec<_>>() {
                        if !obj(doc, &slots, &k).is_some_and(|slot| shaped(doc, &slot)) {
                            return false;
                        }
                    }
                }
            }
            AmValue::Object(_) => return false,
            AmValue::Scalar(s) => match s.as_ref() {
                ScalarValue::Str(s) if s.chars().count() == 1 => {
                    if n == 0 || s.starts_with(['\n', ISLAND_SLOT]) {
                        return false;
                    }
                }
                _ => continue,
            },
        }
        n += 1;
    }
    n > 0 && n == doc.length(&t)
}

/// Puts `content` at `key` of `parent` as a new value.
pub(crate) fn create(doc: &mut AutoCommit, parent: &ObjId, key: &str, content: &Content) -> ObjId {
    let root = doc.put_object(parent, key, ObjType::Map).unwrap();
    let t = doc.put_object(&root, "text", ObjType::Text).unwrap();
    let first = doc.split_block(&t, 0).unwrap();
    doc.put(&first, KIND, BLOCK).unwrap();
    doc.put_object(&root, "containers", ObjType::Map).unwrap();
    insert_text(doc, &t, 1, &content.text, &content.blocks[1..], &content.islands);
    sync(doc, &root, content);
    root
}

/// Inserts `insert` at Automerge index `at`, each `\n` a block marker holding
/// the next of `blocks` and each `U+FFFC` an island marker holding the next
/// of `islands`, so a record lands in the change that brings its character.
pub(crate) fn insert_text(doc: &mut AutoCommit, t: &ObjId, at: usize, insert: &str, blocks: &[Block], islands: &[Island]) {
    let (mut blocks, mut islands) = (blocks.iter(), islands.iter());
    let mut pos = at;
    let mut run = String::new();
    let flush = |doc: &mut AutoCommit, pos: &mut usize, run: &mut String| {
        if !run.is_empty() {
            doc.splice_text(t, *pos, 0, run).unwrap();
            *pos += run.chars().count();
            run.clear();
        }
    };
    for ch in insert.chars() {
        if ch == '\n' || ch == ISLAND_SLOT {
            flush(doc, &mut pos, &mut run);
            let m = doc.split_block(t, pos).unwrap();
            if ch == '\n' {
                doc.put(&m, KIND, BLOCK).unwrap();
                if let Some(b) = blocks.next() {
                    sync_block(doc, &m, b);
                }
            } else {
                doc.put(&m, KIND, ISLAND).unwrap();
                if let Some(i) = islands.next() {
                    sync_island(doc, &m, i);
                }
            }
            pos += 1;
        } else {
            run.push(ch);
        }
    }
    flush(doc, &mut pos, &mut run);
}

/// Makes `root` read as `target`: the text by a prefix and suffix diff where
/// it differs, then each marker's record, the containers, the marks, the
/// anchors and the codec where they differ. Answers whether the text
/// differed.
pub(crate) fn sync(doc: &mut AutoCommit, root: &ObjId, target: &Content) -> bool {
    let t = text(doc, root);
    let mut held = elements(doc, &t);
    let have: Vec<char> = held.iter().skip(1).map(|e| e.ch()).collect();
    let want: Vec<char> = target.text.chars().collect();
    let differed = have != want;
    if differed {
        let pre = have.iter().zip(&want).take_while(|(a, b)| a == b).count();
        let suf = have[pre..].iter().rev().zip(want[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
        let del = have.len() - pre - suf;
        if del > 0 {
            doc.splice_text(&t, pre + 1, del as isize, "").unwrap();
        }
        let ins: String = want[pre..want.len() - suf].iter().collect();
        insert_text(doc, &t, pre + 1, &ins, &[], &[]);
        held = elements(doc, &t);
    }

    let mut blocks = target.blocks.iter();
    let mut islands = target.islands.iter();
    for e in &held {
        match e {
            Element::Marker(m, true) => sync_block(doc, m, blocks.next().expect("a block per block marker")),
            Element::Marker(m, false) => sync_island(doc, m, islands.next().expect("an island per island marker")),
            Element::Char(_) => {}
        }
    }

    let cs = obj(doc, root, "containers").unwrap();
    let want: BTreeMap<String, String> =
        target.containers.iter().map(|c| (c.id.clone(), wire::container_value(c).to_string())).collect();
    for k in doc.keys(&cs).collect::<Vec<_>>() {
        if !want.contains_key(&k) {
            doc.delete(&cs, k.as_str()).unwrap();
        }
    }
    for (k, v) in &want {
        if get_str(doc, &cs, k).as_deref() != Some(v.as_str()) {
            doc.put(&cs, k.as_str(), v.as_str()).unwrap();
        }
    }

    sync_marks(doc, &t, &target.marks);
    sync_anchors(doc, &t, target);

    let codec = target.codec.name();
    if get_str(doc, root, "codec").as_deref() != codec {
        match codec {
            Some(c) => doc.put(root, "codec", c).unwrap(),
            None => doc.delete(root, "codec").unwrap(),
        }
    }
    differed
}

/// A block marker holds `id`, `path`, and `kind`: the kind and its
/// attributes as one JSON value, so two writers setting them at once leave
/// one writer's pair rather than a kind with the other's attributes.
pub(crate) fn sync_block(doc: &mut AutoCommit, m: &ObjId, b: &Block) {
    let v = wire::block_value(b);
    let kind = json!({ "kind": v["kind"], "attrs": v.get("attrs") }).to_string();
    let path = v.get("path").map(Value::to_string);
    for (key, want) in [("id", Some(b.id.clone())), ("kind", Some(kind)), ("path", path)] {
        if get_str(doc, m, key) != want {
            match want {
                Some(w) => doc.put(m, key, w).unwrap(),
                None => doc.delete(m, key).unwrap(),
            }
        }
    }
}

pub(crate) fn sync_island(doc: &mut AutoCommit, m: &ObjId, i: &Island) {
    if get_str(doc, m, "id").as_deref() != Some(i.id.as_str()) {
        doc.put(m, "id", i.id.as_str()).unwrap();
    }
    if get_str(doc, m, "type").as_deref() != Some(i.island_type.as_str()) {
        doc.put(m, "type", i.island_type.as_str()).unwrap();
    }
    write_json_at(doc, m, "props", &i.props);
    let slots = match obj(doc, m, "slots") {
        Some(s) => s,
        None => doc.put_object(m, "slots", ObjType::Map).unwrap(),
    };
    for k in doc.keys(&slots).collect::<Vec<_>>() {
        if !i.slots.contains_key(&k) {
            doc.delete(&slots, k.as_str()).unwrap();
        }
    }
    for (k, c) in &i.slots {
        match obj(doc, &slots, k) {
            Some(r) => {
                sync(doc, &r, c);
            }
            None => {
                create(doc, &slots, k, c);
            }
        }
    }
}

/// The Automerge mark name and value a mark kind is held under: `link` and
/// `font_family` carry their attribute as the value, and an `x-` mark with
/// attributes folds them into its name.
pub(crate) fn mark_key(kind: &MarkKind) -> (String, ScalarValue) {
    match kind {
        MarkKind::Link { url } => ("link".into(), ScalarValue::Str(url.as_str().into())),
        MarkKind::FontFamily { name } => ("font_family".into(), ScalarValue::Str(name.as_str().into())),
        MarkKind::Ext { name, attrs } if !attrs.is_empty() => {
            (format!("{name}{EXT_SEP}{}", Value::Object(attrs.clone())), ScalarValue::Boolean(true))
        }
        other => (other.tag().to_string(), ScalarValue::Boolean(true)),
    }
}

fn mark_json(name: &str, value: &ScalarValue, start: usize, end: usize) -> Value {
    let (ty, attrs) = match (name, value) {
        ("link", ScalarValue::Str(s)) => ("link".to_string(), Some(json!({ "url": s.as_str() }))),
        ("font_family", ScalarValue::Str(s)) => ("font_family".to_string(), Some(json!({ "name": s.as_str() }))),
        (n, _) => match n.split_once(EXT_SEP) {
            Some((ty, attrs)) => (ty.to_string(), serde_json::from_str(attrs).ok()),
            None => (n.to_string(), None),
        },
    };
    let mut v = json!({ "start": start, "end": end, "type": ty });
    if let Some(a) = attrs {
        v["attrs"] = a;
    }
    v
}

/// Whether a mark name holds an anchor rather than formatting.
pub(crate) fn is_anchor_mark(name: &str) -> bool {
    name.starts_with([RANGE, BEFORE, AFTER])
}

/// Per value offset, the mark name and value over that character.
fn mark_table(len: usize, marks: impl Iterator<Item = (String, ScalarValue, usize, usize)>) -> BTreeMap<String, Vec<Option<ScalarValue>>> {
    let mut out: BTreeMap<String, Vec<Option<ScalarValue>>> = BTreeMap::new();
    for (name, value, start, end) in marks {
        let row = out.entry(name).or_insert_with(|| vec![None; len]);
        for slot in row.iter_mut().take(end.min(len)).skip(start) {
            *slot = Some(value.clone());
        }
    }
    out
}

/// Sets the formatting of the text `t` to `want`'s, over the value's
/// characters and the opening block marker alike.
pub(crate) fn sync_marks(doc: &mut AutoCommit, t: &ObjId, want: &[Mark]) {
    let len = doc.length(t);
    let have = mark_table(
        len,
        doc.marks(t).unwrap().into_iter().filter(|m| !is_anchor_mark(m.name())).map(|m| (m.name().to_string(), m.value().clone(), m.start, m.end)),
    );
    let want = mark_table(
        len,
        want.iter().map(|m| {
            let (n, v) = mark_key(&m.kind);
            (n, v, m.start + 1, m.end + 1)
        }),
    );
    let none = vec![None; len];
    let names: BTreeSet<&String> = have.keys().chain(want.keys()).collect();
    for name in names {
        let h = have.get(name).unwrap_or(&none);
        let w = want.get(name).unwrap_or(&none);
        let mut k = 0;
        while k < len {
            if h[k] == w[k] {
                k += 1;
                continue;
            }
            let start = k;
            while k < len && h[k] != w[k] && w[k] == w[start] {
                k += 1;
            }
            match &w[start] {
                Some(v) => doc.mark(t, AmMark::new(name.clone(), v.clone(), start, k), ExpandMark::None).unwrap(),
                None => doc.unmark(t, name, start, k, ExpandMark::None).unwrap(),
            }
        }
    }
}

/// Where an anchor of the text `t` reads: a range's extent, or a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placed {
    Range(usize, usize),
    Point(usize),
}

/// Each anchor the marks of `t` hold, by id, at value offsets: a range from
/// the first character its marks cover to the last, a point as the module
/// reads it.
pub(crate) fn anchors_placed(doc: &AutoCommit, t: &ObjId) -> BTreeMap<String, Placed> {
    let mut ranges: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut before: BTreeMap<String, usize> = BTreeMap::new();
    let mut after: BTreeMap<String, usize> = BTreeMap::new();
    for m in doc.marks(t).unwrap() {
        let name = m.name();
        let Some(kind) = name.chars().next() else { continue };
        let id = &name[kind.len_utf8()..];
        match kind {
            RANGE => {
                let (s, e) = (m.start.saturating_sub(1), m.end.saturating_sub(1));
                let r = ranges.entry(id.to_string()).or_insert((s, e));
                *r = (r.0.min(s), r.1.max(e));
            }
            BEFORE => {
                before.entry(id.to_string()).or_insert(m.end - 1);
            }
            AFTER => {
                after.entry(id.to_string()).or_insert(m.start.saturating_sub(1));
            }
            _ => {}
        }
    }
    let mut out: BTreeMap<String, Placed> = ranges.into_iter().map(|(id, (s, e))| (id, Placed::Range(s, e))).collect();
    for (id, p) in after.into_iter().chain(before) {
        out.entry(id).and_modify(|h| *h = match h {
            Placed::Range(..) => *h,
            Placed::Point(_) => Placed::Point(p),
        }).or_insert(Placed::Point(p));
    }
    out
}

/// Takes the anchor `id` out of the text `t`, its range or point marks as
/// `range` says, or both where it says nothing.
pub(crate) fn unplace(doc: &mut AutoCommit, t: &ObjId, id: &str, range: Option<bool>) {
    let len = doc.length(t);
    let kinds: &[char] = match range {
        Some(true) => &[RANGE],
        Some(false) => &[BEFORE, AFTER],
        None => &[RANGE, BEFORE, AFTER],
    };
    for kind in kinds {
        doc.unmark(t, &format!("{kind}{id}"), 0, len, ExpandMark::None).unwrap();
    }
}

/// Puts the anchor `id` over value offsets `[start, end)` of the text `t`,
/// a point where they are equal.
pub(crate) fn place(doc: &mut AutoCommit, t: &ObjId, id: &str, start: usize, end: usize) {
    if start < end {
        doc.mark(t, AmMark::new(format!("{RANGE}{id}"), true, start + 1, end + 1), ExpandMark::None).unwrap();
        return;
    }
    doc.mark(t, AmMark::new(format!("{BEFORE}{id}"), true, start, start + 1), ExpandMark::None).unwrap();
    if start + 2 <= doc.length(t) {
        doc.mark(t, AmMark::new(format!("{AFTER}{id}"), true, start + 1, start + 2), ExpandMark::None).unwrap();
    }
}

fn sync_anchors(doc: &mut AutoCommit, t: &ObjId, target: &Content) {
    let held = anchors_placed(doc, t);
    let want: BTreeMap<&str, (usize, usize)> = target.anchors.iter().map(|a| (a.id.as_str(), (a.start, a.end))).collect();
    for (id, h) in &held {
        if !want.contains_key(id.as_str()) {
            unplace(doc, t, id, Some(matches!(h, Placed::Range(..))));
        }
    }
    for (id, (start, end)) in want {
        let at = match start == end {
            true => Placed::Point(start),
            false => Placed::Range(start, end),
        };
        match held.get(id) {
            Some(h) if *h == at => {}
            Some(_) => {
                unplace(doc, t, id, None);
                place(doc, t, id, start, end);
            }
            None => place(doc, t, id, start, end),
        }
    }
}

/// One element of a text: a character, or a marker and whether it opens a
/// block.
pub(crate) enum Element {
    Char(char),
    Marker(ObjId, bool),
}

impl Element {
    /// The character a marker spells in the value's text.
    fn ch(&self) -> char {
        match self {
            Element::Char(c) => *c,
            Element::Marker(_, true) => '\n',
            Element::Marker(_, false) => ISLAND_SLOT,
        }
    }
}

/// The elements of the text `t` in order, read in one pass. The pass meets
/// each mark's boundary ops too, which hold a word where a character holds
/// one character.
pub(crate) fn elements(doc: &AutoCommit, t: &ObjId) -> Vec<Element> {
    let items: Vec<Result<char, ObjId>> = doc
        .values(t)
        .filter_map(|(v, id)| match v {
            AmValue::Object(_) => Some(Err(id)),
            AmValue::Scalar(s) => match s.as_ref() {
                ScalarValue::Str(s) => {
                    let mut cs = s.chars();
                    match (cs.next(), cs.next()) {
                        (Some(c), None) => Some(Ok(c)),
                        _ => None,
                    }
                }
                _ => None,
            },
        })
        .collect();
    debug_assert_eq!(items.len(), doc.length(t));
    items
        .into_iter()
        .map(|item| match item {
            Err(m) => {
                let block = get_str(doc, &m, KIND).is_none_or(|k| k == BLOCK);
                Element::Marker(m, block)
            }
            Ok(c) => Element::Char(c),
        })
        .collect()
}

/// The raw value `root` holds: text, records, marks and anchors as the merge
/// left them, islands keyed by id with the id each island character carries.
pub(crate) fn read(doc: &AutoCommit, root: &ObjId) -> Value {
    let t = text(doc, root);
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut slot_ids = Vec::new();
    let mut islands = Map::new();
    for (i, e) in elements(doc, &t).into_iter().enumerate() {
        match e {
            Element::Marker(m, true) => {
                if i > 0 {
                    text.push('\n');
                }
                blocks.push(block_json(doc, &m));
            }
            Element::Marker(m, false) => {
                text.push(ISLAND_SLOT);
                let id = get_str(doc, &m, "id").unwrap_or_default();
                slot_ids.push(Value::String(id.clone()));
                islands.entry(id).or_insert_with(|| island_json(doc, &m));
            }
            Element::Char(c) => {
                if i == 0 {
                    blocks.push(json!({ "id": "", "kind": "para" }));
                }
                text.push(c)
            }
        }
    }
    let containers: Vec<Value> = obj(doc, root, "containers")
        .map(|cs| {
            doc.keys(&cs)
                .filter_map(|k| get_str(doc, &cs, &k))
                .filter_map(|s| serde_json::from_str(&s).ok())
                .collect()
        })
        .unwrap_or_default();
    let marks: Vec<Value> = doc
        .marks(&t)
        .unwrap()
        .iter()
        .filter(|m| !is_anchor_mark(m.name()) && m.end > 1)
        .map(|m| mark_json(m.name(), m.value(), m.start.saturating_sub(1), m.end - 1))
        .collect();
    let anchors: Vec<Value> = anchors_placed(doc, &t)
        .into_iter()
        .map(|(id, h)| match h {
            Placed::Range(start, end) => json!({ "id": id, "start": start, "end": end, "range": true }),
            Placed::Point(at) => json!({ "id": id, "start": at, "end": at }),
        })
        .collect();
    let mut v = json!({
        "text": text,
        "blocks": blocks,
        "containers": containers,
        "marks": marks,
        "anchors": anchors,
        "islands": islands,
        "slot_ids": slot_ids,
    });
    if let Some(c) = get_str(doc, root, "codec") {
        v["codec"] = Value::String(c);
    }
    v
}

fn block_json(doc: &AutoCommit, m: &ObjId) -> Value {
    let mut b = Map::new();
    b.insert("id".into(), Value::String(get_str(doc, m, "id").unwrap_or_default()));
    let kind: Value = get_str(doc, m, "kind").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({ "kind": "para" }));
    b.insert("kind".into(), kind.get("kind").cloned().unwrap_or(json!("para")));
    if let Some(a) = kind.get("attrs").filter(|a| !a.is_null()) {
        b.insert("attrs".into(), a.clone());
    }
    if let Some(v) = get_str(doc, m, "path").and_then(|s| serde_json::from_str(&s).ok()) {
        b.insert("path".into(), v);
    }
    Value::Object(b)
}

fn island_json(doc: &AutoCommit, m: &ObjId) -> Value {
    let props = obj(doc, m, "props").map(|p| hydrate(doc, &p)).unwrap_or_else(|| json!({}));
    let slots: Map<String, Value> = obj(doc, m, "slots")
        .map(|s| doc.keys(&s).filter_map(|k| Some((k.clone(), read(doc, &obj(doc, &s, &k)?)))).collect())
        .unwrap_or_default();
    json!({ "type": get_str(doc, m, "type").unwrap_or_default(), "props": props, "slots": slots })
}

/// The text the islands and slots of `path` reach from the value at
/// `root`, each island found by its marker's id.
pub(crate) fn reach(doc: &AutoCommit, root: &ObjId, path: &[(String, String)]) -> Option<ObjId> {
    let mut t = text(doc, root);
    for (island, slot) in path {
        let marker = markers(doc, &t).remove(island)?;
        t = text(doc, &obj(doc, &obj(doc, &marker, "slots")?, slot)?);
    }
    Some(t)
}

/// Each marker of the text `t` by the id its record holds, the first where
/// two hold one.
pub(crate) fn markers(doc: &AutoCommit, t: &ObjId) -> HashMap<String, ObjId> {
    let mut out = HashMap::new();
    for e in elements(doc, t) {
        if let Element::Marker(m, _) = e
            && let Some(id) = get_str(doc, &m, "id")
        {
            out.entry(id).or_insert(m);
        }
    }
    out
}

/// The marker at Automerge index `i` of the text `t`.
pub(crate) fn marker_at(doc: &AutoCommit, t: &ObjId, i: usize) -> Option<ObjId> {
    match doc.get(t, i).ok()?? {
        (AmValue::Object(ObjType::Map), id) => Some(id),
        _ => None,
    }
}

pub(crate) fn text(doc: &AutoCommit, root: &ObjId) -> ObjId {
    obj(doc, root, "text").expect("a value holds its text")
}

pub(crate) fn obj(doc: &AutoCommit, parent: &ObjId, key: &str) -> Option<ObjId> {
    match doc.get(parent, key).ok()?? {
        (AmValue::Object(_), id) => Some(id),
        _ => None,
    }
}

pub(crate) fn get_str(doc: &AutoCommit, parent: &ObjId, key: &str) -> Option<String> {
    match doc.get(parent, key).ok()?? {
        (AmValue::Scalar(s), _) => match s.as_ref() {
            ScalarValue::Str(s) => Some(s.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn scalar(v: &Value) -> ScalarValue {
    match v {
        Value::Null => ScalarValue::Null,
        Value::Bool(b) => ScalarValue::Boolean(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => ScalarValue::Int(i),
            (_, Some(u)) => ScalarValue::Uint(u),
            _ => ScalarValue::F64(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => ScalarValue::Str(s.as_str().into()),
        _ => unreachable!("only scalars"),
    }
}

fn json_scalar(s: &ScalarValue) -> Value {
    match s {
        ScalarValue::Null => Value::Null,
        ScalarValue::Boolean(b) => Value::Bool(*b),
        ScalarValue::Int(i) => Value::Number((*i).into()),
        ScalarValue::Uint(u) => Value::Number((*u).into()),
        ScalarValue::F64(f) => Number::from_f64(*f).map(Value::Number).unwrap_or(Value::Null),
        ScalarValue::Str(s) => Value::String(s.to_string()),
        other => Value::String(other.to_string()),
    }
}

/// An Automerge map or list read back as JSON.
fn hydrate(doc: &AutoCommit, o: &ObjId) -> Value {
    let item = |v: AmValue<'_>, id: ObjId| match v {
        AmValue::Object(_) => hydrate(doc, &id),
        AmValue::Scalar(s) => json_scalar(&s),
    };
    match doc.object_type(o).unwrap() {
        ObjType::List => Value::Array(doc.values(o).map(|(v, id)| item(v, id)).collect()),
        _ => Value::Object(
            doc.keys(o)
                .filter_map(|k| {
                    let (v, id) = doc.get(o, k.as_str()).ok()??;
                    Some((k, item(v, id)))
                })
                .collect(),
        ),
    }
}

/// Writes `want` at `key` of the map `parent`, touching only what differs: a
/// map key by key and a list over the run between its common prefix and
/// suffix, so two writers changing different keys or inserting at different
/// places both land.
pub(crate) fn write_json_at(doc: &mut AutoCommit, parent: &ObjId, key: &str, want: &Value) {
    let have = doc.get(parent, key).unwrap().map(|(v, id)| (v.into_owned(), id));
    match (want, have) {
        (Value::Object(w), Some((AmValue::Object(ObjType::Map), id))) => write_map(doc, &id, w),
        (Value::Array(w), Some((AmValue::Object(ObjType::List), id))) => write_list(doc, &id, w),
        (Value::Object(w), _) => {
            let id = doc.put_object(parent, key, ObjType::Map).unwrap();
            write_map(doc, &id, w)
        }
        (Value::Array(w), _) => {
            let id = doc.put_object(parent, key, ObjType::List).unwrap();
            write_list(doc, &id, w)
        }
        (w, Some((AmValue::Scalar(s), _))) if json_scalar(&s) == *w => {}
        (w, _) => doc.put(parent, key, scalar(w)).unwrap(),
    }
}

fn write_map(doc: &mut AutoCommit, o: &ObjId, want: &Map<String, Value>) {
    for k in doc.keys(o).collect::<Vec<_>>() {
        if !want.contains_key(&k) {
            doc.delete(o, k.as_str()).unwrap();
        }
    }
    for (k, v) in want {
        write_json_at(doc, o, k, v);
    }
}

fn write_list(doc: &mut AutoCommit, o: &ObjId, want: &[Value]) {
    let have: Vec<Value> = match hydrate(doc, o) {
        Value::Array(a) => a,
        _ => unreachable!("a list hydrates as an array"),
    };
    let pre = have.iter().zip(want).take_while(|(a, b)| a == b).count();
    let suf = have[pre..].iter().rev().zip(want[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (hm, wm) = (have.len() - pre - suf, want.len() - pre - suf);
    let common = hm.min(wm);
    for k in 0..common {
        let i = pre + k;
        let held = doc.get(o, i).unwrap().map(|(v, id)| (v.into_owned(), id));
        match (&want[i], held) {
            (Value::Object(w), Some((AmValue::Object(ObjType::Map), id))) => write_map(doc, &id, w),
            (Value::Array(w), Some((AmValue::Object(ObjType::List), id))) => write_list(doc, &id, w),
            (w, _) => {
                doc.delete(o, i).unwrap();
                insert_json(doc, o, i, w);
            }
        }
    }
    for _ in common..hm {
        doc.delete(o, pre + common).unwrap();
    }
    for k in common..wm {
        insert_json(doc, o, pre + k, &want[pre + k]);
    }
}

fn insert_json(doc: &mut AutoCommit, o: &ObjId, i: usize, v: &Value) {
    match v {
        Value::Object(m) => {
            let id = doc.insert_object(o, i, ObjType::Map).unwrap();
            write_map(doc, &id, m);
        }
        Value::Array(a) => {
            let id = doc.insert_object(o, i, ObjType::List).unwrap();
            write_list(doc, &id, a);
        }
        s => doc.insert(o, i, scalar(s)).unwrap(),
    }
}
