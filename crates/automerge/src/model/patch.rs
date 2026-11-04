//! Automerge's patches as automerge-js spells them: each a JSON object with
//! an `action` and a `path` from the document's root, its text indexes
//! counted in the host's units.
//!
//! A value's text opens with a block marker and holds each block marker and
//! island as one character, so value offset `n` is text index `n + 1`.

use super::json::json_scalar;
use crate::content::change::{self, Change, Splice, Step};
use crate::content::model::{Content, ISLAND_SLOT};
use crate::content::units::Units;
use crate::content::wire;
use automerge::patches::PatchAction;
use automerge::{AutoCommit, ChangeHash, ObjId, ObjType, Prop, ReadDoc, Value as AmValue};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// The patches taking the document from `before` to `after`.
pub fn between(doc: &mut AutoCommit, before: &[ChangeHash], after: &[ChangeHash], u: impl Units) -> Vec<Value> {
    let raw = doc.diff(before, after);
    spell(doc, before, raw, u, Vec::new())
}

/// [`between`], with the value `value`, which read as `was` at `before`,
/// as the patches leave it where they only splice its own text: a
/// keystroke's view without reading the value again.
pub fn landed(doc: &mut AutoCommit, before: &[ChangeHash], after: &[ChangeHash], u: impl Units, value: &ObjId, was: &Content) -> (Vec<Value>, Option<Content>) {
    let raw = doc.diff(before, after);
    let (spelled, mut followed) = follow_all(doc, before, raw, u, vec![(value.clone(), was.clone())]);
    (spelled, followed.pop().flatten())
}

/// `raw`, the patches from `before`, spelled in `u`, each text in `held`
/// as it read at `before`.
pub(crate) fn spell(doc: &AutoCommit, before: &[ChangeHash], raw: Vec<automerge::Patch>, u: impl Units, held: Vec<(ObjId, Vec<char>)>) -> Vec<Value> {
    let mut texts = Texts { held: held.into_iter().collect(), before: before.to_vec() };
    raw.into_iter().map(|p| texts.spell(doc, p, u)).collect()
}

/// `raw`, the patches from `before`, spelled in `u`, and each of `values`,
/// a value and what it read as at `before`, as the patches leave it where
/// they only splice its own text, `None` where they do more.
pub(crate) fn follow_all(doc: &AutoCommit, before: &[ChangeHash], raw: Vec<automerge::Patch>, u: impl Units, values: Vec<(ObjId, Content)>) -> (Vec<Value>, Vec<Option<Content>>) {
    let mut held = Vec::new();
    let mut work: Vec<(ObjId, Option<ObjId>, Option<Content>)> = Vec::new();
    for (value, was) in values {
        let text = super::value::text(doc, &value);
        if let Some(t) = &text {
            held.push((t.clone(), std::iter::once('\u{fffc}').chain(was.text.chars()).collect()));
        }
        let was = text.as_ref().map(|_| was);
        work.push((value, text, was));
    }
    for p in &raw {
        for (value, text, c) in &mut work {
            if let (Some(t), Some(content)) = (text.as_ref(), c.as_mut())
                && !follow(content, value, t, p)
            {
                *c = None;
            }
        }
    }
    let spelled = spell(doc, before, raw, u, held);
    (spelled, work.into_iter().map(|(_, _, c)| c).collect())
}

/// The keys of the path from the document's root to what `p` changes, the
/// key it puts or deletes in a map last, a list index spelled as a number.
pub(crate) fn keys(p: &automerge::Patch) -> Vec<String> {
    let mut out: Vec<String> = p.path.iter().map(|(_, prop)| prop_string(prop)).collect();
    match &p.action {
        PatchAction::PutMap { key, .. } | PatchAction::DeleteMap { key } => out.push(key.clone()),
        PatchAction::Increment { prop, .. } | PatchAction::Conflict { prop } => out.push(prop_string(prop)),
        _ => {}
    }
    out
}

fn prop_string(p: &Prop) -> String {
    match p {
        Prop::Map(k) => k.clone(),
        Prop::Seq(i) => i.to_string(),
    }
}

/// Takes `p` into `c`, the value `value` with the text `text`: `false`
/// where `p` does more to the value than splice plain text outside every
/// point anchor.
fn follow(c: &mut Content, value: &ObjId, text: &ObjId, p: &automerge::Patch) -> bool {
    if &p.obj != text {
        return &p.obj != value && !p.path.iter().any(|(o, _)| o == value || o == text);
    }
    let near_point = |lo: usize, hi: usize| c.anchors.iter().any(|a| a.is_point() && lo <= a.start && a.start <= hi);
    let splice = match &p.action {
        PatchAction::SpliceText { index, value, marks } if *index > 0 => {
            let at = index - 1;
            let insert = value.make_string();
            let n = insert.chars().count();
            if near_point(at, at) || insert.contains(['\n', ISLAND_SLOT]) {
                return false;
            }
            let mut kinds = Vec::new();
            let mut anchored = std::collections::BTreeSet::new();
            for (name, v) in marks.iter().flat_map(|m| m.iter()) {
                match name.strip_prefix(super::value::ANCHOR) {
                    Some(id) => {
                        anchored.insert(id.to_string());
                    }
                    None => match super::value::mark_json(name, v, 0, n).and_then(|j| wire::mark_from_value(&j).ok()) {
                        Some(m) => kinds.push(m),
                        None => return false,
                    },
                }
            }
            let inside: std::collections::BTreeSet<String> = c.anchors.iter().filter(|a| !a.is_point() && a.start < at && at < a.end).map(|a| a.id.clone()).collect();
            if inside != anchored {
                return false;
            }
            Splice { marks: kinds, ..Splice::new(at, "", insert) }
        }
        PatchAction::DeleteSeq { index, length } if *index > 0 => {
            let at = index - 1;
            let deleted: String = c.text.chars().skip(at).take(*length).collect();
            if deleted.chars().count() != *length || deleted.contains(['\n', ISLAND_SLOT]) || near_point(at, at + length) {
                return false;
            }
            Splice::new(at, deleted, "")
        }
        _ => return false,
    };
    match change::apply(c, &Change::new(vec![Step::Splice(splice)])) {
        Ok(applied) => {
            *c = applied.content;
            true
        }
        Err(_) => false,
    }
}

/// Each text a patch reaches, as the patches before it left it, one
/// character per index.
struct Texts {
    held: HashMap<ObjId, Vec<char>>,
    before: Vec<ChangeHash>,
}

impl Texts {
    fn text(&mut self, doc: &AutoCommit, obj: &ObjId) -> &mut Vec<char> {
        let before = &self.before;
        self.held.entry(obj.clone()).or_insert_with(|| doc.text_at(obj, before).map(|t| t.chars().collect()).unwrap_or_default())
    }

    fn units(&mut self, doc: &AutoCommit, obj: &ObjId, index: usize, u: impl Units) -> usize {
        self.text(doc, obj).iter().take(index).map(|&c| u.char_len(c)).sum()
    }

    fn is_text(doc: &AutoCommit, obj: &ObjId) -> bool {
        doc.object_type(obj).ok() == Some(ObjType::Text)
    }

    fn spell(&mut self, doc: &AutoCommit, p: automerge::Patch, u: impl Units) -> Value {
        let mut path: Vec<Value> = Vec::with_capacity(p.path.len() + 1);
        for (parent, prop) in &p.path {
            path.push(match prop {
                Prop::Map(k) => json!(k),
                Prop::Seq(i) if Self::is_text(doc, parent) => json!(self.units(doc, parent, *i, u)),
                Prop::Seq(i) => json!(i),
            });
        }
        let obj = &p.obj;
        let text = Self::is_text(doc, obj);
        let mut out = Map::new();
        match p.action {
            PatchAction::PutMap { key, value, conflict } => {
                path.push(json!(key));
                out.insert("action".into(), json!("put"));
                out.insert("value".into(), spell_value(&value.0));
                if conflict {
                    out.insert("conflict".into(), json!(true));
                }
            }
            PatchAction::PutSeq { index, value, conflict } => {
                path.push(json!(index));
                out.insert("action".into(), json!("put"));
                out.insert("value".into(), spell_value(&value.0));
                if conflict {
                    out.insert("conflict".into(), json!(true));
                }
            }
            PatchAction::Insert { index, values } => {
                let at = if text { self.units(doc, obj, index, u) } else { index };
                if text {
                    let held = self.text(doc, obj);
                    let index = index.min(held.len());
                    held.splice(index..index, std::iter::repeat_n('\u{fffc}', values.len()));
                }
                path.push(json!(at));
                out.insert("action".into(), json!("insert"));
                out.insert("values".into(), Value::Array(values.iter().map(|(v, _, _)| spell_value(v)).collect()));
            }
            PatchAction::SpliceText { index, value, marks } => {
                let at = self.units(doc, obj, index, u);
                let s = value.make_string();
                let held = self.text(doc, obj);
                let index = index.min(held.len());
                held.splice(index..index, s.chars());
                path.push(json!(at));
                out.insert("action".into(), json!("splice"));
                out.insert("value".into(), json!(s));
                if let Some(marks) = marks.filter(|m| m.iter().next().is_some()) {
                    out.insert("marks".into(), Value::Object(marks.iter().map(|(k, v)| (k.to_string(), json_scalar(v))).collect()));
                }
            }
            PatchAction::Increment { prop, value } => {
                path.push(prop_json(&prop));
                out.insert("action".into(), json!("inc"));
                out.insert("value".into(), json!(value));
            }
            PatchAction::Conflict { prop } => {
                path.push(prop_json(&prop));
                out.insert("action".into(), json!("conflict"));
            }
            PatchAction::DeleteMap { key } => {
                path.push(json!(key));
                out.insert("action".into(), json!("del"));
            }
            PatchAction::DeleteSeq { index, length } => {
                let (at, len) = if text {
                    let at = self.units(doc, obj, index, u);
                    let held = self.text(doc, obj);
                    let end = (index + length).min(held.len());
                    let gone: Vec<char> = held.drain(index.min(end)..end).collect();
                    (at, gone.iter().map(|&c| u.char_len(c)).sum())
                } else {
                    (index, length)
                };
                path.push(json!(at));
                out.insert("action".into(), json!("del"));
                if len != 1 {
                    out.insert("length".into(), json!(len));
                }
            }
            PatchAction::Mark { marks } => {
                out.insert("action".into(), json!("mark"));
                let marks = marks
                    .iter()
                    .map(|m| {
                        let start = self.units(doc, obj, m.start, u);
                        let end = self.units(doc, obj, m.end, u);
                        json!({ "name": m.name.as_str(), "value": json_scalar(&m.value), "start": start, "end": end })
                    })
                    .collect();
                out.insert("marks".into(), Value::Array(marks));
            }
        }
        out.insert("path".into(), Value::Array(path));
        Value::Object(out)
    }
}

fn prop_json(p: &Prop) -> Value {
    match p {
        Prop::Map(k) => json!(k),
        Prop::Seq(i) => json!(i),
    }
}

fn spell_value(v: &AmValue<'_>) -> Value {
    match v {
        AmValue::Object(ObjType::Map | ObjType::Table) => json!({}),
        AmValue::Object(ObjType::List) => json!([]),
        AmValue::Object(ObjType::Text) => json!(""),
        AmValue::Scalar(s) => json_scalar(s),
    }
}

/// The spans of a value's own text that `patches`, one write's to that
/// value, put: value offsets in `u` on the text they leave. Islands' slots
/// and the value's other parts are passed over.
pub fn inserted_spans(patches: &[Value], u: impl Units, after: &str) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for p in patches {
        let Some(path) = p["path"].as_array() else { continue };
        let [.., key, index] = path.as_slice() else { continue };
        if key != "text" || path.iter().any(|s| s == "slots") {
            continue;
        }
        let Some(index) = index.as_u64().map(|i| i as usize) else { continue };
        let at = index.saturating_sub(1);
        let (deleted, inserted) = match p["action"].as_str() {
            Some("splice") => (0, u.len(p["value"].as_str().unwrap_or_default())),
            Some("insert") => (0, p["values"].as_array().map_or(0, Vec::len)),
            Some("del") => (p["length"].as_u64().unwrap_or(1) as usize, 0),
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
    spans
        .into_iter()
        .filter_map(|(a, b)| Some((crate::content::units::to_scalars(u, after, a)?, crate::content::units::to_scalars(u, after, b)?)))
        .collect()
}
