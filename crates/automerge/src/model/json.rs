//! Plain JSON held as Automerge maps, lists and scalars, written key by key
//! and over a list's changed run, so two writers changing different parts
//! both land.

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ReadDoc, ScalarValue, Value as AmValue};
use serde_json::{Map, Number, Value};

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
    let item = |v: AmValue<'_>, id: ObjId| match v {
        AmValue::Object(_) => hydrate(doc, &id),
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
    let have = doc
        .get(parent, key)
        .unwrap()
        .map(|(v, id)| (v.into_owned(), id));
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
        if k != ORDER && !want.contains_key(&k) {
            doc.delete(o, k.as_str()).unwrap();
        }
    }
    for (k, v) in want {
        write_json_at(doc, o, k, v);
    }
    let order: Vec<&String> = want.keys().collect();
    if want.len() > 1 && key_order(doc, o).iter().collect::<Vec<_>>() != order {
        let keys = Value::Array(order.into_iter().map(|k| Value::String(k.clone())).collect());
        write_json_at(doc, o, ORDER, &keys);
    }
}

fn write_list(doc: &mut AutoCommit, o: &ObjId, want: &[Value]) {
    let have: Vec<Value> = match hydrate(doc, o) {
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
        let held = doc.get(o, i).unwrap().map(|(v, id)| (v.into_owned(), id));
        match (&want[i], held) {
            (Value::Object(w), Some((AmValue::Object(ObjType::Map), id))) => write_map(doc, &id, w),
            (Value::Array(w), Some((AmValue::Object(ObjType::List), id))) => {
                write_list(doc, &id, w)
            }
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

pub(crate) fn insert_json(doc: &mut AutoCommit, o: &ObjId, i: usize, v: &Value) {
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
