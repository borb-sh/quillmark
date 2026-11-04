//! A value's ids as the document gives them. Inside a document a block,
//! container or island id is its record's object id, so a value put into a
//! document reads back under other ids than it was given, each at the same
//! place; a change made against the given value is renamed to the document's
//! ids before it lowers, and a value read out is renamed back.

use quillmark_content::change::Change;
use quillmark_content::model::Content;
use serde_json::Value;
use std::collections::HashMap;

/// The keys whose string values are prose or a kind name, never an id.
const KEPT: [&str; 11] = ["text", "insert", "delete", "quote", "type", "op", "kind", "name", "url", "tag", "codec"];

/// Each id `from` holds, by its place among the blocks, containers and
/// islands, to the id `to` holds there; one value's own records only.
fn by_place(from: &Content, to: &Content) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (a, b) in from.blocks.iter().zip(&to.blocks) {
        out.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in from.containers.iter().zip(&to.containers) {
        out.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in from.islands.iter().zip(&to.islands) {
        out.insert(a.id.clone(), b.id.clone());
    }
    out
}

/// Each id `from` holds at any depth, by its place, to the id `to` holds
/// there, as one map: the ids on one side are unique across the value's
/// slots, as a document's object ids are.
pub fn places(from: &Content, to: &Content) -> HashMap<String, String> {
    let mut out = by_place(from, to);
    for (a, b) in from.islands.iter().zip(&to.islands) {
        for (k, slot) in &a.slots {
            if let Some(other) = b.slots.get(k) {
                out.extend(places(slot, other));
            }
        }
    }
    out
}

fn rename_json(v: &Value, map: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| rename_json(x, map)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| match KEPT.contains(&k.as_str()) && x.is_string() {
                    true => (k.clone(), x.clone()),
                    false => (k.clone(), rename_json(x, map)),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn rename_steps(v: &Value, from: &Content, to: &Content) -> Value {
    let map = by_place(from, to);
    let steps = v["steps"].as_array().map(|s| s.as_slice()).unwrap_or_default().iter().map(|step| match step.get("nested") {
        Some(n) => {
            let (island, slot) = (n["island"].as_str().unwrap_or_default(), n["slot"].as_str().unwrap_or_default());
            let Some(i) = from.islands.iter().position(|i| i.id == island) else {
                return step.clone();
            };
            let mut out = n.clone();
            out["island"] = Value::String(to.islands.get(i).map(|i| i.id.clone()).unwrap_or_else(|| island.to_string()));
            if let (Some(a), Some(b)) = (from.islands[i].slots.get(slot), to.islands.get(i).and_then(|i| i.slots.get(slot))) {
                out["change"] = rename_steps(&n["change"], a, b);
            }
            serde_json::json!({ "nested": out })
        }
        None => rename_json(step, &map),
    });
    serde_json::json!({ "steps": steps.collect::<Vec<_>>() })
}

/// `change`, made against `from`, with each id `from` holds renamed to the
/// one `to` holds at its place, a nested change to its slot's ids; an id
/// `from` does not hold stays.
pub fn renamed(change: &Change, from: &Content, to: &Content) -> Change {
    Change::from_value(&rename_steps(&change.to_value(), from, to)).unwrap_or_else(|_| change.clone())
}

/// `content` with each id `map` names renamed, at any depth.
pub fn renamed_content(content: &Content, map: &HashMap<String, String>) -> Content {
    if map.is_empty() {
        return content.clone();
    }
    Content::from_value(&rename_json(&content.to_value(), map)).unwrap_or_else(|_| content.clone())
}
