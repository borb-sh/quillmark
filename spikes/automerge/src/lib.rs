//! A content value held in an Automerge document.
//!
//! A value is a map: `text` a Text object, `containers` a map of container
//! records by id, `anchors` a map of cursor pairs by id, and `codec`. The text
//! opens with a block marker holding block 0's record; every later `\n` is a
//! block marker and every `U+FFFC` an island marker, so a value offset `n` is
//! Automerge index `n + 1`. An island marker holds `id`, `type`, `props` as
//! Automerge maps and lists, and `slots`, a map of values of this same shape.
//!
//! Reading a document gives the raw value repair takes, so every peer reads
//! one merged state as one valid value.

use automerge::marks::{ExpandMark, Mark as AmMark};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, Cursor, MoveCursor, ObjId, ObjType, ReadDoc, ScalarValue, Value as AmValue};
use quillmark_content::change::{Change, IslandStep, Step};
use quillmark_content::model::{IslandType, Block, Content, ISLAND_SLOT, Island, Mark, MarkKind};
use quillmark_content::repair::{Raw, Repair, repair};
use quillmark_content::wire;
use serde_json::{Map, Number, Value, json};
use std::collections::BTreeMap;

const KIND: &str = "k";
const BLOCK: &str = "b";
const ISLAND: &str = "i";
const ANCHOR: &str = "@";
const EXT_SEP: char = '\u{1f}';

/// How much of a write fell back to diffing a value rather than lowering its
/// steps: text spliced by a prefix and suffix diff because the steps left it
/// unequal to the value the change names.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Fallback {
    pub text_diffs: usize,
}

/// Puts `content` at `key` of `parent` as a new value.
pub fn create(doc: &mut AutoCommit, parent: &ObjId, key: &str, content: &Content) -> ObjId {
    let root = doc.put_object(parent, key, ObjType::Map).unwrap();
    init(doc, &root, content);
    root
}

fn init(doc: &mut AutoCommit, root: &ObjId, content: &Content) {
    let t = doc.put_object(root, "text", ObjType::Text).unwrap();
    let first = doc.split_block(&t, 0).unwrap();
    doc.put(&first, KIND, BLOCK).unwrap();
    doc.put_object(root, "containers", ObjType::Map).unwrap();
    doc.put_object(root, "anchors", ObjType::Map).unwrap();
    insert_text(doc, &t, 1, &content.text, &content.blocks[1..], &content.islands);
    sync(doc, root, content);
}

/// The value `root` holds as one valid value, what repair changed, and the
/// change from the merged text to the value's.
pub fn project(doc: &AutoCommit, root: &ObjId) -> (Content, Vec<Repair>, Change) {
    repair(Raw::from_value(&read(doc, root)).expect("a document reads as raw content"))
}

/// Writes `change`, which takes the value `root` holds to `after`: each
/// step's text as Automerge splices and markers, then every record, mark,
/// anchor and island where it differs from `after`.
pub fn write(doc: &mut AutoCommit, root: &ObjId, change: &Change, after: &Content) -> Fallback {
    lower(doc, root, &change.steps);
    sync(doc, root, after)
}

fn lower(doc: &mut AutoCommit, root: &ObjId, steps: &[Step]) {
    let t = text(doc, root);
    for step in steps {
        match step {
            Step::Splice(s) => {
                let n = s.delete.chars().count();
                if n > 0 {
                    doc.splice_text(&t, s.at + 1, n as isize, "").unwrap();
                }
                insert_text(doc, &t, s.at + 1, &s.insert, &s.blocks, &s.islands);
            }
            Step::Island(IslandStep::Insert { at, island, .. }) => {
                insert_text(doc, &t, at + 1, &ISLAND_SLOT.to_string(), &[], std::slice::from_ref(island))
            }
            Step::Nested { island, slot, change } => {
                let marker = island_marker(doc, &t, island).expect("a nested step names an island the text holds");
                let slots = obj(doc, &marker, "slots").expect("an island marker holds its slots");
                let slot_root = obj(doc, &slots, slot).expect("a nested step names a slot the island holds");
                lower(doc, &slot_root, &change.steps);
            }
            Step::Island(IslandStep::Props { id, props, slots }) => {
                let marker = island_marker(doc, &t, id).expect("a props step names an island the text holds");
                write_json_at(doc, &marker, "props", props);
                let ty = get_str(doc, &marker, "type").and_then(|t| IslandType::parse(&t)).expect("an island marker holds its type");
                let named = Island::new(id.clone(), ty).with_props(props.clone()).slot_order();
                let held = obj(doc, &marker, "slots").expect("an island marker holds its slots");
                for k in doc.keys(&held).collect::<Vec<_>>() {
                    if !named.contains(&k) {
                        doc.delete(&held, k.as_str()).unwrap();
                    }
                }
                for k in named {
                    if obj(doc, &held, &k).is_none() {
                        create(doc, &held, &k, slots.get(&k).unwrap_or(&Content::empty()));
                    }
                }
            }
            Step::SetBlock(_) | Step::Container(_) | Step::Mark(_) | Step::Anchor(_) => {}
        }
    }
}

/// Inserts `insert` at `at`, each `\n` a block marker holding the next of
/// `blocks` and each `U+FFFC` an island marker holding the next of `islands`,
/// so a record lands in the change that brings its character.
fn insert_text(doc: &mut AutoCommit, t: &ObjId, at: usize, insert: &str, blocks: &[Block], islands: &[Island]) {
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
            doc.put(&m, KIND, if ch == '\n' { BLOCK } else { ISLAND }).unwrap();
            match ch == '\n' {
                true => {
                    if let Some(b) = blocks.next() {
                        sync_block(doc, &m, b);
                    }
                }
                _ => {
                    if let Some(i) = islands.next() {
                        sync_island(doc, &m, i);
                    }
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
/// anchors and the codec where they differ.
pub fn sync(doc: &mut AutoCommit, root: &ObjId, target: &Content) -> Fallback {
    let mut fb = Fallback::default();
    let t = text(doc, root);
    let have: Vec<char> = value_text(doc, &t).chars().collect();
    let want: Vec<char> = target.text.chars().collect();
    if have != want {
        fb.text_diffs += 1;
        let pre = have.iter().zip(&want).take_while(|(a, b)| a == b).count();
        let suf = have[pre..].iter().rev().zip(want[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
        let del = have.len() - pre - suf;
        if del > 0 {
            doc.splice_text(&t, pre + 1, del as isize, "").unwrap();
        }
        let ins: String = want[pre..want.len() - suf].iter().collect();
        insert_text(doc, &t, pre + 1, &ins, &[], &[]);
    }

    let markers = markers(doc, &t);
    let mut blocks = target.blocks.iter();
    let mut islands = target.islands.iter();
    for (_, m, kind) in &markers {
        if kind == BLOCK {
            sync_block(doc, m, blocks.next().expect("a block per block marker"));
        } else {
            fb.text_diffs += sync_island(doc, m, islands.next().expect("an island per island marker")).text_diffs;
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
    sync_anchors(doc, root, &t, target);

    let codec = target.codec.name();
    if get_str(doc, root, "codec").as_deref() != codec {
        match codec {
            Some(c) => doc.put(root, "codec", c).unwrap(),
            None => doc.delete(root, "codec").unwrap(),
        }
    }
    fb
}

/// A block marker holds `id`, `path`, and `kind`: the kind and its
/// attributes as one JSON value, so two writers setting them at once leave
/// one writer's pair rather than a kind with the other's attributes.
fn sync_block(doc: &mut AutoCommit, m: &ObjId, b: &Block) {
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

fn sync_island(doc: &mut AutoCommit, m: &ObjId, i: &Island) -> Fallback {
    let mut fb = Fallback::default();
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
            Some(r) => fb.text_diffs += sync(doc, &r, c).text_diffs,
            None => {
                create(doc, &slots, k, c);
            }
        }
    }
    fb
}

fn mark_key(m: &Mark) -> (String, ScalarValue) {
    match &m.kind {
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

fn sync_marks(doc: &mut AutoCommit, t: &ObjId, want: &[Mark]) {
    let len = doc.length(t).saturating_sub(1);
    let have = mark_table(
        len,
        doc.marks(t)
            .unwrap()
            .into_iter()
            .filter(|m| !m.name().starts_with(ANCHOR))
            .map(|m| (m.name().to_string(), m.value().clone(), m.start.saturating_sub(1), m.end.saturating_sub(1))),
    );
    let want = mark_table(
        len,
        want.iter().map(|m| {
            let (n, v) = mark_key(m);
            (n, v, m.start, m.end)
        }),
    );
    let none = vec![None; len];
    let names: std::collections::BTreeSet<&String> = have.keys().chain(want.keys()).collect();
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
                Some(v) => doc
                    .mark(t, AmMark::new(name.clone(), v.clone(), start + 1, k + 1), ExpandMark::None)
                    .unwrap(),
                None => doc.unmark(t, name, start + 1, k + 1, ExpandMark::None).unwrap(),
            }
        }
    }
}

fn cursor(doc: &AutoCommit, t: &ObjId, am: usize, mv: MoveCursor) -> String {
    if am >= doc.length(t) {
        Cursor::End.to_string()
    } else {
        doc.get_cursor_moving(t, am, None, mv).unwrap().to_string()
    }
}

fn position(doc: &AutoCommit, t: &ObjId, c: &str) -> usize {
    let c = Cursor::try_from(c).expect("a stored cursor parses");
    doc.get_cursor_position(t, &c, None).unwrap()
}

/// A range anchor is a mark named `@` and its id, which neither edge grows,
/// so its edges converge as formatting does and a range whose text is all
/// deleted is gone. A point is a cursor on the character after it, in
/// `anchors`.
fn sync_anchors(doc: &mut AutoCommit, root: &ObjId, t: &ObjId, target: &Content) {
    let len = doc.length(t);
    let ranges = anchor_marks(doc, t);
    let points = obj(doc, root, "anchors").unwrap();
    let want: BTreeMap<&str, (usize, usize)> = target.anchors.iter().map(|a| (a.id.as_str(), (a.start, a.end))).collect();
    for k in doc.keys(&points).collect::<Vec<_>>() {
        if want.get(k.as_str()).is_none_or(|(s, e)| s != e) {
            doc.delete(&points, k.as_str()).unwrap();
        }
    }
    for k in ranges.keys() {
        if want.get(k.as_str()).is_none_or(|(s, e)| s == e) {
            doc.unmark(t, &format!("{ANCHOR}{k}"), 0, len, ExpandMark::None).unwrap();
        }
    }
    for (id, (start, end)) in want {
        if start == end {
            let held = obj(doc, &points, id).and_then(|a| point(doc, t, &a));
            if held != Some(start) {
                let a = doc.put_object(&points, id, ObjType::Map).unwrap();
                let s = cursor(doc, t, start + 1, MoveCursor::After);
                doc.put(&a, "s", s).unwrap();
            }
        } else if ranges.get(id) != Some(&(start, end)) {
            let name = format!("{ANCHOR}{id}");
            if ranges.contains_key(id) {
                doc.unmark(t, &name, 0, len, ExpandMark::None).unwrap();
            }
            doc.mark(t, AmMark::new(name, true, start + 1, end + 1), ExpandMark::None).unwrap();
        }
    }
}

/// Each range anchor's extent, from the first character its mark covers to
/// the last.
fn anchor_marks(doc: &AutoCommit, t: &ObjId) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for m in doc.marks(t).unwrap() {
        if let Some(id) = m.name().strip_prefix(ANCHOR) {
            let (s, e) = (m.start.saturating_sub(1), m.end.saturating_sub(1));
            let r = out.entry(id.to_string()).or_insert((s, e));
            *r = (r.0.min(s), r.1.max(e));
        }
    }
    out
}

fn point(doc: &AutoCommit, t: &ObjId, a: &ObjId) -> Option<usize> {
    let s = get_str(doc, a, "s")?;
    Some(position(doc, t, &s).saturating_sub(1))
}

/// The raw value `root` holds: text, records, marks and anchors as the merge
/// left them, islands keyed by id with the id each island character carries.
pub fn read(doc: &AutoCommit, root: &ObjId) -> Value {
    let t = text(doc, root);
    let chars: Vec<char> = doc.text(&t).unwrap().chars().collect();
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut slot_ids = Vec::new();
    let mut islands = Map::new();
    for (i, ch) in chars.iter().enumerate() {
        match (*ch == ISLAND_SLOT).then(|| marker_at(doc, &t, i)).flatten() {
            Some((m, kind)) if kind == BLOCK => {
                if i > 0 {
                    text.push('\n');
                }
                blocks.push(block_json(doc, &m));
            }
            Some((m, _)) => {
                text.push(ISLAND_SLOT);
                let id = get_str(doc, &m, "id").unwrap_or_default();
                slot_ids.push(Value::String(id.clone()));
                islands.entry(id).or_insert_with(|| island_json(doc, &m));
            }
            None => text.push(*ch),
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
        .filter(|m| !m.name().starts_with(ANCHOR))
        .map(|m| mark_json(m.name(), m.value(), m.start.saturating_sub(1), m.end.saturating_sub(1)))
        .collect();
    let mut anchors: Vec<Value> = anchor_marks(doc, &t)
        .into_iter()
        .map(|(id, (start, end))| json!({ "id": id, "start": start, "end": end, "range": true }))
        .collect();
    if let Some(points) = obj(doc, root, "anchors") {
        for k in doc.keys(&points) {
            if let Some(at_) = obj(doc, &points, &k).and_then(|a| point(doc, &t, &a)) {
                anchors.push(json!({ "id": k, "start": at_, "end": at_ }));
            }
        }
    }
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

/// The value text, which the markers spell as `\n` and `U+FFFC`.
fn value_text(doc: &AutoCommit, t: &ObjId) -> String {
    let mut cs: Vec<char> = doc.text(t).unwrap().chars().collect();
    for (i, _, kind) in markers(doc, t) {
        if kind == BLOCK {
            cs[i] = '\n';
        }
    }
    cs.into_iter().skip(1).collect()
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

fn markers(doc: &AutoCommit, t: &ObjId) -> Vec<(usize, ObjId, String)> {
    doc.text(t)
        .unwrap()
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == ISLAND_SLOT)
        .filter_map(|(i, _)| marker_at(doc, t, i).map(|(m, k)| (i, m, k)))
        .collect()
}

fn marker_at(doc: &AutoCommit, t: &ObjId, i: usize) -> Option<(ObjId, String)> {
    match doc.get(t, i).ok()?? {
        (AmValue::Object(ObjType::Map), id) => {
            let kind = get_str(doc, &id, KIND).unwrap_or_else(|| BLOCK.into());
            Some((id, kind))
        }
        _ => None,
    }
}

fn island_marker(doc: &AutoCommit, t: &ObjId, id: &str) -> Option<ObjId> {
    markers(doc, t)
        .into_iter()
        .find(|(_, m, k)| k == ISLAND && get_str(doc, m, "id").as_deref() == Some(id))
        .map(|(_, m, _)| m)
}

fn text(doc: &AutoCommit, root: &ObjId) -> ObjId {
    obj(doc, root, "text").expect("a value holds its text")
}

fn obj(doc: &AutoCommit, parent: &ObjId, key: &str) -> Option<ObjId> {
    match doc.get(parent, key).ok()?? {
        (AmValue::Object(_), id) => Some(id),
        _ => None,
    }
}

fn get_str(doc: &AutoCommit, parent: &ObjId, key: &str) -> Option<String> {
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
            _ => ScalarValue::F64(n.as_f64().unwrap()),
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
    match doc.object_type(o).unwrap() {
        ObjType::List => Value::Array(
            (0..doc.length(o))
                .map(|i| match doc.get(o, i).unwrap().unwrap() {
                    (AmValue::Object(_), id) => hydrate(doc, &id),
                    (AmValue::Scalar(s), _) => json_scalar(&s),
                })
                .collect(),
        ),
        _ => Value::Object(
            doc.keys(o)
                .map(|k| {
                    let v = match doc.get(o, k.as_str()).unwrap().unwrap() {
                        (AmValue::Object(_), id) => hydrate(doc, &id),
                        (AmValue::Scalar(s), _) => json_scalar(&s),
                    };
                    (k, v)
                })
                .collect(),
        ),
    }
}

/// Writes `want` at `key` of the map `parent`, touching only what differs: a
/// map key by key and a list over the run between its common prefix and
/// suffix, so two writers changing different keys or inserting at different
/// places both land.
fn write_json_at(doc: &mut AutoCommit, parent: &ObjId, key: &str, want: &Value) {
    let have = doc.get(parent, key).unwrap();
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
        _ => unreachable!(),
    };
    let pre = have.iter().zip(want).take_while(|(a, b)| a == b).count();
    let suf = have[pre..].iter().rev().zip(want[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (hm, wm) = (have.len() - pre - suf, want.len() - pre - suf);
    let common = hm.min(wm);
    for k in 0..common {
        let i = pre + k;
        match (&want[i], doc.get(o, i).unwrap()) {
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
