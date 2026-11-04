//! Automerge's patches as automerge-js spells them: each a JSON object with
//! an `action` and a `path` from the document's root, its text indexes
//! counted in the host's units.
//!
//! A value's text opens with a block marker and holds each block marker and
//! island as one character, so value offset `n` is text index `n + 1`.

use super::json::json_scalar;
use crate::content::units::Units;
use automerge::patches::PatchAction;
use automerge::{AutoCommit, ChangeHash, ObjId, ObjType, Prop, ReadDoc, Value as AmValue};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// The patches taking the document from `before` to `after`.
pub fn between(doc: &mut AutoCommit, before: &[ChangeHash], after: &[ChangeHash], u: impl Units) -> Vec<Value> {
    let patches = doc.diff(before, after);
    let mut texts = Texts { held: HashMap::new(), before: before.to_vec() };
    patches.into_iter().map(|p| texts.spell(doc, p, u)).collect()
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
