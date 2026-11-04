//! Plain JSON held as Automerge maps, lists and scalars, written key by key
//! and over a list's changed run, so two writers changing different parts
//! both land. A field's value holds each content value in it, at any depth,
//! as a text value instead, which merges as text does.

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ReadDoc, ScalarValue, Value as AmValue};
use serde_json::{Map, Number, Value};

use super::value;

pub(crate) fn scalar(v: &Value) -> ScalarValue {
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

/// An Automerge scalar read back as JSON: a float JSON cannot spell, a
/// NaN or an infinity, reads as `null`.
pub fn json_scalar(s: &ScalarValue) -> Value {
    match s {
        ScalarValue::Null => Value::Null,
        ScalarValue::Boolean(b) => Value::Bool(*b),
        ScalarValue::Int(i) => Value::Number((*i).into()),
        ScalarValue::Uint(u) => Value::Number((*u).into()),
        ScalarValue::F64(f) => Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ScalarValue::Str(s) => Value::String(s.to_string()),
        other => Value::String(other.to_string()),
    }
}

/// An Automerge map or list read back as JSON.
pub fn hydrate(doc: &AutoCommit, o: &ObjId) -> Value {
    hydrate_in(doc, o, false)
}

/// A field's value read back as JSON: each text value in it, at any depth,
/// as the content it reads as.
pub fn hydrate_field(doc: &AutoCommit, o: &ObjId) -> Value {
    hydrate_in(doc, o, true)
}

fn hydrate_in(doc: &AutoCommit, o: &ObjId, texts: bool) -> Value {
    if texts && value::text(doc, o).is_some() {
        return value::view(doc, o).to_value();
    }
    let item = |v: AmValue<'_>, id: ObjId| match v {
        AmValue::Object(_) => hydrate_in(doc, &id, texts),
        AmValue::Scalar(s) => json_scalar(&s),
    };
    match doc.object_type(o).unwrap() {
        ObjType::List => Value::Array(doc.values(o).map(|(v, id)| item(v, id)).collect()),
        _ => Value::Object(
            key_order(doc, o)
                .into_iter()
                .filter_map(|k| {
                    let (v, id) = doc.get(o, k.as_str()).ok()??;
                    Some((k, item(v, id)))
                })
                .collect(),
        ),
    }
}

/// The key a map holds its keys' order under, since an Automerge map holds
/// none: a list of keys, each once.
pub const ORDER: &str = "\u{1f}order";

/// The keys of the map `o` in the order its [`ORDER`] list names them, each
/// once, then any it does not name, sorted.
fn key_order(doc: &AutoCommit, o: &ObjId) -> Vec<String> {
    let held: Vec<String> = doc.keys(o).filter(|k| k != ORDER).collect();
    let mut out: Vec<String> = Vec::with_capacity(held.len());
    if let Ok(Some((AmValue::Object(ObjType::List), l))) = doc.get(o, ORDER) {
        for (v, _) in doc.values(&l) {
            if let AmValue::Scalar(s) = v
                && let Some(k) = s.as_str()
                && held.iter().any(|h| h == k)
                && !out.iter().any(|x| x == k)
            {
                out.push(k.to_string());
            }
        }
    }
    for k in held {
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// Writes `want` at `key` of the map `parent`, touching only what differs: a
/// map key by key and a list over the run between its common prefix and
/// suffix, so two writers changing different keys or inserting at different
/// places both land.
pub fn write_json_at(doc: &mut AutoCommit, parent: &ObjId, key: &str, want: &Value) {
    write_at(doc, parent, key, want, false)
}

/// [`write_json_at`] for a field's value: each content value in `want`, at
/// any depth, is held as a text value, written over the one held there in
/// place.
pub fn write_field_at(doc: &mut AutoCommit, parent: &ObjId, key: &str, want: &Value) {
    write_at(doc, parent, key, want, true)
}

/// The object `doc` holds at `prop` of `parent`, as `texts` sees it: a text
/// value apart from every other map.
fn held(doc: &AutoCommit, parent: &ObjId, prop: impl Into<automerge::Prop>, texts: bool) -> Option<(Held, ObjId)> {
    match doc.get(parent, prop).unwrap()? {
        (AmValue::Object(ObjType::Map), id) if texts && value::text(doc, &id).is_some() => Some((Held::Text, id)),
        (AmValue::Object(ObjType::Map), id) => Some((Held::Map, id)),
        (AmValue::Object(ObjType::List), id) => Some((Held::List, id)),
        (AmValue::Scalar(s), id) => Some((Held::Scalar(json_scalar(&s)), id)),
        (AmValue::Object(_), id) => Some((Held::Other, id)),
    }
}

enum Held {
    Text,
    Map,
    List,
    Scalar(Value),
    Other,
}

fn write_at(doc: &mut AutoCommit, parent: &ObjId, key: &str, want: &Value, texts: bool) {
    let have = held(doc, parent, key, texts);
    if let Some(content) = texts.then(|| value::content_of(want)).flatten() {
        match have {
            Some((Held::Text, id)) => value::rewrite(doc, &id, &content),
            _ => {
                value::create(doc, parent, key, &content);
            }
        }
        return;
    }
    match (want, have) {
        (Value::Object(w), Some((Held::Map, id))) => write_map(doc, &id, w, texts),
        (Value::Array(w), Some((Held::List, id))) => write_list(doc, &id, w, texts),
        (Value::Object(w), _) => {
            let id = doc.put_object(parent, key, ObjType::Map).unwrap();
            write_map(doc, &id, w, texts)
        }
        (Value::Array(w), _) => {
            let id = doc.put_object(parent, key, ObjType::List).unwrap();
            write_list(doc, &id, w, texts)
        }
        (w, Some((Held::Scalar(s), _))) if s == *w => {}
        (w, _) => doc.put(parent, key, scalar(w)).unwrap(),
    }
}

fn write_map(doc: &mut AutoCommit, o: &ObjId, want: &Map<String, Value>, texts: bool) {
    for k in doc.keys(o).collect::<Vec<_>>() {
        if k != ORDER && !want.contains_key(&k) {
            doc.delete(o, k.as_str()).unwrap();
        }
    }
    for (k, v) in want {
        write_at(doc, o, k, v, texts);
    }
    let order: Vec<&String> = want.keys().collect();
    if want.len() > 1 && key_order(doc, o).iter().collect::<Vec<_>>() != order {
        let keys = Value::Array(order.into_iter().map(|k| Value::String(k.clone())).collect());
        write_json_at(doc, o, ORDER, &keys);
    }
}

fn write_list(doc: &mut AutoCommit, o: &ObjId, want: &[Value], texts: bool) {
    let have: Vec<Value> = match hydrate_in(doc, o, texts) {
        Value::Array(a) => a,
        _ => unreachable!("a list hydrates as an array"),
    };
    let pre = have.iter().zip(want).take_while(|(a, b)| a == b).count();
    let suf = have[pre..]
        .iter()
        .rev()
        .zip(want[pre..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (hm, wm) = (have.len() - pre - suf, want.len() - pre - suf);
    let common = hm.min(wm);
    for k in 0..common {
        let i = pre + k;
        let content = texts.then(|| value::content_of(&want[i])).flatten();
        match (&want[i], content, held(doc, o, i, texts)) {
            (_, Some(c), Some((Held::Text, id))) => value::rewrite(doc, &id, &c),
            (Value::Object(w), None, Some((Held::Map, id))) => write_map(doc, &id, w, texts),
            (Value::Array(w), None, Some((Held::List, id))) => write_list(doc, &id, w, texts),
            (w, _, _) => {
                doc.delete(o, i).unwrap();
                insert_at(doc, o, i, w, texts);
            }
        }
    }
    for _ in common..hm {
        doc.delete(o, pre + common).unwrap();
    }
    for k in common..wm {
        insert_at(doc, o, pre + k, &want[pre + k], texts);
    }
}

fn insert_at(doc: &mut AutoCommit, o: &ObjId, i: usize, v: &Value, texts: bool) {
    if let Some(content) = texts.then(|| value::content_of(v)).flatten() {
        let id = doc.insert_object(o, i, ObjType::Map).unwrap();
        value::fill(doc, &id, &content);
        return;
    }
    match v {
        Value::Object(m) => {
            let id = doc.insert_object(o, i, ObjType::Map).unwrap();
            write_map(doc, &id, m, texts);
        }
        Value::Array(a) => {
            let id = doc.insert_object(o, i, ObjType::List).unwrap();
            write_list(doc, &id, a, texts);
        }
        s => doc.insert(o, i, scalar(s)).unwrap(),
    }
}
