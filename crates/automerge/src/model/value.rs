//! A text value in Automerge's rich-text shape.
//!
//! A value is a map: `text` a Text object, `containers` a list of container
//! records, `points` a map of point anchors, and `codec`. The text opens
//! with a block marker; every block marker is a map holding `type`, `attrs`
//! and `parents`, the ids of the containers it sits in, outermost first. An
//! island is a map in the text holding `isEmbed`, `type`, `props` and
//! `slots`, each slot a value of this same shape, so a value offset `n` is
//! Automerge index `n + 1`.
//!
//! Every block, container and island is named by its Automerge object id.
//! Formatting is a mark per kind; a range anchor is a mark `anchor:<id>`
//! that grows at neither edge, and a point anchor is a cursor under its id
//! in `points`.

use super::json::{hydrate, write_json_at};
use automerge::iter::Span;
use automerge::marks::{ExpandMark, Mark as AmMark, UpdateSpansConfig};
use automerge::transaction::Transactable;
use automerge::{
    AutoCommit, Cursor, CursorPosition, ObjId, ObjType, ReadDoc, ScalarValue, Value as AmValue,
};
use crate::content::lines::island::IslandType;
use crate::content::model::{Block, Codec, Content, ISLAND_SLOT, Island, MarkKind};
use crate::content::wire;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

pub(crate) const TEXT: &str = "text";
pub(crate) const CONTAINERS: &str = "containers";
pub(crate) const POINTS: &str = "points";
pub(crate) const CODEC: &str = "codec";
pub(crate) const TYPE: &str = "type";
pub(crate) const ATTRS: &str = "attrs";
pub(crate) const PARENTS: &str = "parents";
pub(crate) const EMBED: &str = "isEmbed";
pub(crate) const PROPS: &str = "props";
pub(crate) const SLOTS: &str = "slots";
pub(crate) const ANCHOR: &str = "anchor:";
const EXT_SEP: char = '\u{1f}';

/// The object id a view names a part by.
pub(crate) fn id_of(o: &ObjId) -> String {
    o.to_string()
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

pub(crate) fn text(doc: &AutoCommit, root: &ObjId) -> Option<ObjId> {
    obj(doc, root, TEXT).filter(|t| doc.object_type(t).ok() == Some(ObjType::Text))
}

/// Puts `content` at `key` of the map `parent` as a new value, and answers
/// its map.
pub(crate) fn create(doc: &mut AutoCommit, parent: &ObjId, key: &str, content: &Content) -> ObjId {
    let root = doc.put_object(parent, key, ObjType::Map).unwrap();
    fill(doc, &root, content);
    root
}

/// Fills the empty map `root` with `content`.
pub(crate) fn fill(doc: &mut AutoCommit, root: &ObjId, content: &Content) {
    let t = doc.put_object(root, TEXT, ObjType::Text).unwrap();
    let cs = doc.put_object(root, CONTAINERS, ObjType::List).unwrap();
    doc.put_object(root, POINTS, ObjType::Map).unwrap();
    if let Some(c) = content.codec.name() {
        doc.put(root, CODEC, c).unwrap();
    }
    let mut ids = HashMap::new();
    for (i, c) in content.containers.iter().enumerate() {
        let m = doc.insert_object(&cs, i, ObjType::Map).unwrap();
        let v = wire::container_value(c);
        doc.put(&m, TYPE, v["kind"].as_str().unwrap()).unwrap();
        write_json_at(doc, &m, ATTRS, v.get("attrs").unwrap_or(&json!({})));
        ids.insert(c.id.clone(), id_of(&m));
    }
    let first = doc.split_block(&t, 0).unwrap();
    put_block(doc, &first, &content.blocks[0], &ids);
    insert_text(
        doc,
        &t,
        1,
        &content.text,
        &content.blocks[1..],
        &content.islands,
        &ids,
    );
    for m in &content.marks {
        let (name, value) = mark_key(&m.kind);
        doc.mark(
            &t,
            AmMark::new(name, value, m.start + 1, m.end + 1),
            expand(&m.kind),
        )
        .unwrap();
    }
    for a in &content.anchors {
        place(doc, root, &t, &a.id, a.start, a.end);
    }
}

/// The content `v` holds, where it is a content value.
pub(crate) fn content_of(v: &Value) -> Option<Content> {
    let shaped = v.get("text").is_some_and(Value::is_string) && v.get("blocks").is_some_and(Value::is_array);
    shaped.then(|| Content::from_value(v).ok()).flatten()
}

/// What the value `root` reads as: its raw value read through repair.
pub(crate) fn view(doc: &AutoCommit, root: &ObjId) -> Content {
    let raw = crate::content::repair::Raw::from_value(&read(doc, root)).expect("a value reads as raw content");
    crate::content::repair::repair(raw).0
}

/// Writes `want` over the value `root` in place, so an edit concurrent with
/// it merges: by the change between them, made against what the value reads
/// as, and where that does not lower, by Automerge's own diff of the text.
pub(crate) fn rewrite(doc: &mut AutoCommit, root: &ObjId, want: &Content) {
    let now = view(doc, root);
    if now == *want {
        return;
    }
    let lowered = crate::content::diff::diff(&now, want).is_ok_and(|change| super::step::lower(doc, root, &change, Some(&now)).is_ok());
    if !lowered {
        update(doc, root, want);
    }
}

/// Writes `content` over the value `root` in place by Automerge's own diff
/// of its text, so an edit concurrent with it merges as text does: each
/// block, container and island `content` names by its object id stays that
/// object, its record put over where it differs.
pub(crate) fn update(doc: &mut AutoCommit, root: &ObjId, content: &Content) {
    let Some(t) = text(doc, root) else {
        for k in doc.keys(root).collect::<Vec<_>>() {
            doc.delete(root, k.as_str()).unwrap();
        }
        fill(doc, root, content);
        return;
    };
    match content.codec.name() {
        Some(c) => doc.put(root, CODEC, c).unwrap(),
        None if doc.get(root, CODEC).unwrap().is_some() => doc.delete(root, CODEC).unwrap(),
        None => {}
    }
    let cs = match obj(doc, root, CONTAINERS) {
        Some(cs) => cs,
        None => doc.put_object(root, CONTAINERS, ObjType::List).unwrap(),
    };
    let wanted: std::collections::HashSet<&str> = content.containers.iter().map(|c| c.id.as_str()).collect();
    for i in (0..doc.length(&cs)).rev() {
        if let Ok(Some((_, m))) = doc.get(&cs, i)
            && !wanted.contains(id_of(&m).as_str())
        {
            doc.delete(&cs, i).unwrap();
        }
    }
    let held: HashMap<String, ObjId> = doc.values(&cs).map(|(_, m)| (id_of(&m), m)).collect();
    let mut ids = HashMap::new();
    for c in &content.containers {
        let m = match held.get(&c.id) {
            Some(m) => m.clone(),
            None => doc.insert_object(&cs, doc.length(&cs), ObjType::Map).unwrap(),
        };
        let v = wire::container_value(c);
        doc.put(&m, TYPE, v["kind"].as_str().unwrap()).unwrap();
        write_json_at(doc, &m, ATTRS, v.get("attrs").unwrap_or(&json!({})));
        ids.insert(c.id.clone(), id_of(&m));
    }
    let held: HashMap<String, ObjId> = elements(doc, &t)
        .into_iter()
        .filter_map(|e| match e {
            Element::Block(m) | Element::Island(m) => Some((id_of(&m), m)),
            Element::Char(_) => None,
        })
        .collect();
    let marker = |id: &str, fresh: &[(&str, ScalarValue)]| match held.get(id).and_then(|m| doc.hydrate(m, None).ok()) {
        Some(automerge::hydrate::Value::Map(m)) => m,
        _ => fresh.iter().map(|(k, v)| (k.to_string(), automerge::hydrate::Value::Scalar(v.clone()))).collect::<HashMap<String, _>>().into(),
    };
    let mut spans = vec![Span::Block(marker(&content.blocks[0].id, &[(TYPE, "para".into())]))];
    let (mut blocks, mut islands) = (content.blocks[1..].iter(), content.islands.iter());
    let mut run = String::new();
    for ch in content.text.chars() {
        let next = match ch {
            '\n' => blocks.next().map(|b| marker(&b.id, &[(TYPE, "para".into())])),
            ISLAND_SLOT => islands.next().map(|i| marker(&i.id, &[(EMBED, true.into())])),
            _ => {
                run.push(ch);
                continue;
            }
        };
        if !run.is_empty() {
            spans.push(Span::Text { text: std::mem::take(&mut run), marks: None });
        }
        spans.push(Span::Block(next.unwrap_or_else(|| marker("", &[(TYPE, "para".into())]))));
    }
    if !run.is_empty() {
        spans.push(Span::Text { text: run, marks: None });
    }
    doc.update_spans(&t, UpdateSpansConfig::default(), spans).unwrap();
    let (mut blocks, mut islands) = (content.blocks.iter(), content.islands.iter());
    for e in elements(doc, &t) {
        match e {
            Element::Block(m) => {
                if let Some(b) = blocks.next() {
                    put_block(doc, &m, b, &ids);
                }
            }
            Element::Island(m) => {
                if let Some(i) = islands.next() {
                    put_island(doc, &m, i);
                    let slots = obj(doc, &m, SLOTS).expect("an island holds its slots");
                    for (k, c) in &i.slots {
                        update(doc, &obj(doc, &slots, k).expect("each slot is held"), c);
                    }
                }
            }
            Element::Char(_) => {}
        }
    }
    for m in &content.marks {
        let (name, value) = mark_key(&m.kind);
        doc.mark(&t, AmMark::new(name, value, m.start + 1, m.end + 1), expand(&m.kind)).unwrap();
    }
    if let Some(points) = obj(doc, root, POINTS) {
        for id in doc.keys(&points).collect::<Vec<_>>() {
            doc.delete(&points, id.as_str()).unwrap();
        }
    }
    for a in &content.anchors {
        place(doc, root, &t, &a.id, a.start, a.end);
    }
}

/// Inserts `insert` at Automerge index `at`, each `\n` a block marker
/// holding the next of `blocks` and each `U+FFFC` an island holding the next
/// of `islands`. Container ids in a block's path are mapped through `ids`,
/// and one `ids` lacks is taken as an object id already.
pub(crate) fn insert_text(
    doc: &mut AutoCommit,
    t: &ObjId,
    at: usize,
    insert: &str,
    blocks: &[Block],
    islands: &[Island],
    ids: &HashMap<String, String>,
) -> Vec<ObjId> {
    let (mut blocks, mut islands) = (blocks.iter(), islands.iter());
    let mut made = Vec::new();
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
                match blocks.next() {
                    Some(b) => put_block(doc, &m, b, ids),
                    None => {
                        doc.put(&m, TYPE, "para").unwrap();
                    }
                }
            } else {
                doc.put(&m, EMBED, true).unwrap();
                if let Some(i) = islands.next() {
                    put_island(doc, &m, i);
                }
            }
            made.push(m);
            pos += 1;
        } else {
            run.push(ch);
        }
    }
    flush(doc, &mut pos, &mut run);
    made
}

pub(crate) fn put_block(doc: &mut AutoCommit, m: &ObjId, b: &Block, ids: &HashMap<String, String>) {
    let v = wire::block_value(b);
    doc.put(m, TYPE, v["kind"].as_str().unwrap()).unwrap();
    write_json_at(doc, m, ATTRS, v.get("attrs").unwrap_or(&json!({})));
    let parents: Vec<Value> = b
        .path
        .iter()
        .map(|p| Value::String(ids.get(p).cloned().unwrap_or_else(|| p.clone())))
        .collect();
    write_json_at(doc, m, PARENTS, &Value::Array(parents));
}

pub(crate) fn put_island(doc: &mut AutoCommit, m: &ObjId, i: &Island) {
    doc.put(m, EMBED, true).unwrap();
    doc.put(m, TYPE, i.island_type.as_str()).unwrap();
    write_json_at(doc, m, PROPS, &i.props);
    let slots = match obj(doc, m, SLOTS) {
        Some(s) => s,
        None => doc.put_object(m, SLOTS, ObjType::Map).unwrap(),
    };
    for k in doc.keys(&slots).collect::<Vec<_>>() {
        if !i.slots.contains_key(&k) {
            doc.delete(&slots, k.as_str()).unwrap();
        }
    }
    for (k, c) in &i.slots {
        if obj(doc, &slots, k).is_none() {
            create(doc, &slots, k, c);
        }
    }
}

/// How a mark kind grows at its edges: run formatting takes text typed at
/// its end, as Peritext has it; a link, an `x-` mark and an anchor take none.
pub(crate) fn expand(kind: &MarkKind) -> ExpandMark {
    match kind {
        MarkKind::Link { .. } | MarkKind::Ext { .. } => ExpandMark::None,
        _ => ExpandMark::After,
    }
}

/// The Automerge mark name and value a mark kind is held under: `link` and
/// `font_family` carry their attribute as the value, and an `x-` mark with
/// attributes folds them into its name.
pub(crate) fn mark_key(kind: &MarkKind) -> (String, ScalarValue) {
    match kind {
        MarkKind::Link { url } => ("link".into(), ScalarValue::Str(url.as_str().into())),
        MarkKind::FontFamily { name } => {
            ("font_family".into(), ScalarValue::Str(name.as_str().into()))
        }
        MarkKind::Ext { name, attrs } if !attrs.is_empty() => (
            format!("{name}{EXT_SEP}{}", Value::Object(attrs.clone())),
            ScalarValue::Boolean(true),
        ),
        other => (other.tag().to_string(), ScalarValue::Boolean(true)),
    }
}

/// The mark `name` holds `value` over `[start, end)`, `None` where its type
/// and attrs are not a mark's.
pub(crate) fn mark_json(name: &str, value: &ScalarValue, start: usize, end: usize) -> Option<Value> {
    let (ty, attrs) = match (name, value) {
        ("link", ScalarValue::Str(s)) => ("link".to_string(), Some(json!({ "url": s.as_str() }))),
        ("font_family", ScalarValue::Str(s)) => (
            "font_family".to_string(),
            Some(json!({ "name": s.as_str() })),
        ),
        (n, _) => match n.split_once(EXT_SEP) {
            Some((ty, attrs)) => (ty.to_string(), serde_json::from_str(attrs).ok()),
            None => (n.to_string(), None),
        },
    };
    let attrs = attrs.and_then(|a| match a {
        Value::Object(o) => Some(o),
        _ => None,
    });
    wire::mark_kind_from(&ty, attrs.as_ref().unwrap_or(&Map::new())).ok()?;
    let mut v = json!({ "start": start, "end": end, "type": ty });
    if let Some(a) = attrs {
        v["attrs"] = Value::Object(a);
    }
    Some(v)
}

/// Puts the anchor `id` over value offsets `[start, end)` of the text `t` of
/// the value `root`: a range as a mark, a point as a cursor on the element
/// after it, or on the text's end.
pub(crate) fn place(
    doc: &mut AutoCommit,
    root: &ObjId,
    t: &ObjId,
    id: &str,
    start: usize,
    end: usize,
) {
    if start < end {
        doc.mark(
            t,
            AmMark::new(format!("{ANCHOR}{id}"), true, start + 1, end + 1),
            ExpandMark::None,
        )
        .unwrap();
        return;
    }
    let at = match start + 1 < doc.length(t) {
        true => CursorPosition::Index(start + 1),
        false => CursorPosition::End,
    };
    let cursor = doc.get_cursor(t, at, None).unwrap();
    let points = match obj(doc, root, POINTS) {
        Some(p) => p,
        None => doc.put_object(root, POINTS, ObjType::Map).unwrap(),
    };
    doc.put(&points, id, cursor.to_string()).unwrap();
}

/// Takes the anchor `id` out of the value `root`, as a range and a point.
pub(crate) fn unplace(doc: &mut AutoCommit, root: &ObjId, t: &ObjId, id: &str) {
    let len = doc.length(t);
    let name = format!("{ANCHOR}{id}");
    if doc.marks(t).unwrap().iter().any(|m| m.name() == name) {
        doc.unmark(t, &name, 0, len, ExpandMark::None).unwrap();
    }
    if let Some(points) = obj(doc, root, POINTS).filter(|p| doc.get(p, id).unwrap().is_some()) {
        doc.delete(&points, id).unwrap();
    }
}

/// One element of a text: a character, a block marker or an island.
#[derive(Debug, Clone)]
pub(crate) enum Element {
    Char(char),
    Block(ObjId),
    Island(ObjId),
}

pub(crate) fn elements(doc: &AutoCommit, t: &ObjId) -> Vec<Element> {
    let text = doc.text(t).unwrap_or_default();
    text.chars()
        .enumerate()
        .map(|(i, c)| match c {
            '\u{fffc}' => match doc.get(t, i) {
                Ok(Some((AmValue::Object(ObjType::Map), m))) => match is_embed(doc, &m) {
                    true => Element::Island(m),
                    false => Element::Block(m),
                },
                _ => Element::Char(c),
            },
            c => Element::Char(c),
        })
        .collect()
}

pub(crate) fn is_embed(doc: &AutoCommit, m: &ObjId) -> bool {
    matches!(doc.get(m, EMBED), Ok(Some((AmValue::Scalar(s), _))) if matches!(s.as_ref(), ScalarValue::Boolean(true)))
}

/// The raw value `root` holds, as [`Raw::from_value`] reads it: text,
/// records, marks and anchors as the merge left them, islands keyed by id
/// with the id each island character carries. A record, mark or island a
/// peer left in a shape no content holds, such as a kind no build names or
/// a path entry that is not a string, reads as the nearest one the decoder
/// takes, so every document reads.
///
/// [`Raw::from_value`]: crate::content::repair::Raw::from_value
pub(crate) fn read(doc: &AutoCommit, root: &ObjId) -> Value {
    let Some(t) = text(doc, root) else {
        return json!({ "text": "", "blocks": [{ "id": id_of(root), "kind": "para" }] });
    };
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut slot_ids = Vec::new();
    let mut islands = Map::new();
    for (i, e) in elements(doc, &t).into_iter().enumerate() {
        match e {
            Element::Block(m) => {
                if i > 0 {
                    text.push('\n');
                }
                blocks.push(block_json(doc, &m));
            }
            Element::Island(m) => {
                if i == 0 {
                    blocks.push(json!({ "id": id_of(root), "kind": "para" }));
                }
                text.push(ISLAND_SLOT);
                match island_json(doc, &m) {
                    Some(island) => {
                        let id = id_of(&m);
                        slot_ids.push(Value::String(id.clone()));
                        islands.insert(id, island);
                    }
                    None => slot_ids.push(Value::Null),
                }
            }
            Element::Char(c) => {
                if i == 0 {
                    blocks.push(json!({ "id": id_of(root), "kind": "para" }));
                }
                text.push(c)
            }
        }
    }
    if blocks.is_empty() {
        blocks.push(json!({ "id": id_of(root), "kind": "para" }));
    }
    let containers: Vec<Value> = obj(doc, root, CONTAINERS)
        .map(|cs| {
            doc.values(&cs)
                .filter_map(|(v, id)| match v {
                    AmValue::Object(ObjType::Map) => container_json(doc, &id),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let all = doc.marks(&t).unwrap();
    let marks: Vec<Value> = all
        .iter()
        .filter(|m| !m.name().starts_with(ANCHOR) && m.end > 1)
        .filter_map(|m| mark_json(m.name(), m.value(), m.start.saturating_sub(1), m.end - 1))
        .collect();
    let mut anchors: BTreeMap<String, Value> = BTreeMap::new();
    for m in all.iter() {
        let Some(id) = m.name().strip_prefix(ANCHOR) else {
            continue;
        };
        let (s, e) = (m.start.saturating_sub(1), m.end.saturating_sub(1));
        let entry = anchors
            .entry(id.to_string())
            .or_insert_with(|| json!({ "id": id, "start": s, "end": e, "range": true }));
        let (os, oe) = (
            entry["start"].as_u64().unwrap() as usize,
            entry["end"].as_u64().unwrap() as usize,
        );
        entry["start"] = json!(os.min(s));
        entry["end"] = json!(oe.max(e));
    }
    if let Some(points) = obj(doc, root, POINTS) {
        for id in doc.keys(&points).collect::<Vec<_>>() {
            let Some(at) = point_at(doc, &points, &t, &id) else {
                continue;
            };
            anchors
                .entry(id.clone())
                .or_insert_with(|| json!({ "id": id, "start": at, "end": at }));
        }
    }
    let mut v = json!({
        "text": text,
        "blocks": blocks,
        "containers": containers,
        "marks": marks,
        "anchors": anchors.into_values().collect::<Vec<_>>(),
        "islands": islands,
        "slot_ids": slot_ids,
    });
    if let Some(c) = get_str(doc, root, CODEC).filter(|c| Codec::parse(c).is_some()) {
        v["codec"] = Value::String(c);
    }
    v
}

/// The object `key` of `m` holds, hydrated, where it is a non-empty object.
fn attrs_json(doc: &AutoCommit, m: &ObjId) -> Map<String, Value> {
    obj(doc, m, ATTRS)
        .map(|a| hydrate(doc, &a))
        .and_then(|a| match a {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default()
}

/// The value offset the point `id` of `points` reads at.
pub(crate) fn point_at(doc: &AutoCommit, points: &ObjId, t: &ObjId, id: &str) -> Option<usize> {
    let cursor = Cursor::try_from(get_str(doc, points, id)?.as_str()).ok()?;
    // Automerge 0.12 panics, rather than refusing, on a cursor naming an op of another object.
    let at = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| doc.get_cursor_position(t, &cursor, None))).ok()?.ok()?;
    Some(at.saturating_sub(1).min(doc.length(t).saturating_sub(1)))
}

/// The block record `m`: a kind and attrs the decoder takes, a paragraph
/// where they are not, and the string entries of its path.
pub(crate) fn block_json(doc: &AutoCommit, m: &ObjId) -> Value {
    let mut b = Map::new();
    b.insert("id".into(), Value::String(id_of(m)));
    let (mut kind, mut attrs) = (get_str(doc, m, TYPE).unwrap_or_else(|| "para".into()), attrs_json(doc, m));
    if wire::block_kind_from(&kind, &attrs).is_err() {
        (kind, attrs) = ("para".into(), Map::new());
    }
    b.insert("kind".into(), Value::String(kind));
    if !attrs.is_empty() {
        b.insert("attrs".into(), Value::Object(attrs));
    }
    let path: Vec<Value> = obj(doc, m, PARENTS)
        .map(|p| hydrate(doc, &p))
        .and_then(|p| p.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.is_string())
        .collect();
    if !path.is_empty() {
        b.insert("path".into(), Value::Array(path));
    }
    Value::Object(b)
}

/// The container record `m`, `None` where its kind and attrs are not a
/// container's.
pub(crate) fn container_json(doc: &AutoCommit, m: &ObjId) -> Option<Value> {
    let (kind, attrs) = (get_str(doc, m, TYPE)?, attrs_json(doc, m));
    wire::container_kind_from(&kind, &attrs).ok()?;
    let mut c = json!({ "id": id_of(m), "kind": kind });
    if !attrs.is_empty() {
        c["attrs"] = Value::Object(attrs);
    }
    Some(c)
}

/// The island `m`, `None` where its type is not one a build reads.
pub(crate) fn island_json(doc: &AutoCommit, m: &ObjId) -> Option<Value> {
    let ty = get_str(doc, m, TYPE).filter(|t| IslandType::parse(t).is_some())?;
    let props = obj(doc, m, PROPS)
        .map(|p| hydrate(doc, &p))
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let slots: Map<String, Value> = obj(doc, m, SLOTS)
        .map(|s| {
            doc.keys(&s)
                .filter_map(|k| Some((k.clone(), read(doc, &obj(doc, &s, &k)?))))
                .collect()
        })
        .unwrap_or_default();
    Some(json!({ "type": ty, "props": props, "slots": slots }))
}
