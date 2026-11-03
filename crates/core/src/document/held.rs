//! A document's cards held in its Automerge document.
//!
//! A card's payload is its `layout`, each item in source order with a
//! field named by key, its `fields`, each value by key, its `ext` and `seed`
//! maps and its `nestedComments`. A field holding a content value holds it
//! as a text value, so it merges as text does; every other value is plain
//! data, a map key by key and a list by element, and a scalar last writer
//! wins. The cards are a view the document is read into: a write lands in
//! the document first, or is flushed into it from the cards.

use quillmark_automerge::automerge::transaction::Transactable;
use quillmark_automerge::automerge::{ObjId, ObjType, ROOT, ReadDoc, Value as AmValue};
use quillmark_automerge::model::json::{hydrate, write_json_at};
use quillmark_automerge::model::{self as am, Doc};
use quillmark_content::model::Content;
use serde_json::{Value, json};

use super::dto::{NestedCommentV0_134_0, PayloadItemV0_134_0};
use super::payload::{MetaKey, PayloadItem};
use super::{Card, Payload};
use crate::value::QuillValue;

/// Whether a field's stored value is a content value.
pub(crate) fn is_content(v: &Value) -> bool {
    v.get("text").is_some_and(Value::is_string) && v.get("blocks").is_some_and(Value::is_array) && Content::from_value(v).is_ok()
}

fn get_obj(doc: &Doc, parent: &ObjId, key: &str) -> Option<ObjId> {
    match doc.am().get(parent, key).ok()?? {
        (AmValue::Object(_), id) => Some(id),
        _ => None,
    }
}

fn map_at(doc: &mut Doc, parent: &ObjId, key: &str, ty: ObjType) -> ObjId {
    match get_obj(doc, parent, key) {
        Some(o) => o,
        None => doc.am_mut().put_object(parent, key, ty).unwrap(),
    }
}

/// A new document holding `main` and each of `cards` that holds an id,
/// built under the first actor: equal cards build equal bytes in every
/// process.
pub(crate) fn build(main: &Card, cards: &[Card]) -> Doc {
    Doc::building(|d| {
        let m = d.main();
        write_card(d, &m, main);
        let (all, order) = (d.cards(), order_obj(d));
        for card in cards {
            let Some(id) = card.id() else { continue };
            let c = d.am_mut().put_object(&all, id, ObjType::Map).unwrap();
            d.card_skeleton(&c);
            write_card(d, &c, card);
            let at = d.am().length(&order);
            d.am_mut().insert(&order, at, id).unwrap();
        }
    })
}

fn order_obj(d: &Doc) -> ObjId {
    get_obj(d, &ROOT, am::ORDER).expect("a document holds its order")
}

fn item_json(item: &PayloadItem) -> Value {
    serde_json::to_value(PayloadItemV0_134_0::from(item)).expect("a payload item serializes")
}

/// The layout entry an item takes: the item without the value a field, an
/// `$ext` or a `$seed` holds apart.
fn layout_entry(item: &PayloadItem) -> Value {
    let mut v = item_json(item);
    if matches!(v["type"].as_str(), Some("field" | "ext" | "seed")) {
        v.as_object_mut().unwrap().remove("value");
    }
    v
}

/// Writes `card`'s payload and body into the card map `c`.
fn write_card(d: &mut Doc, c: &ObjId, card: &Card) {
    write_payload(d, c, &Payload::new(), card.payload());
    if d.am().get(c, am::BODY).unwrap().is_some() {
        d.am_mut().delete(c, am::BODY).unwrap();
    }
    d.put_value(c, am::BODY, card.body());
}

/// Writes what differs from `was` to `now` in the payload of the card map
/// `c`, key by key.
fn write_payload(d: &mut Doc, c: &ObjId, was: &Payload, now: &Payload) {
    let layout: Vec<Value> = now.items().iter().map(layout_entry).collect();
    if was.items().iter().map(layout_entry).collect::<Vec<_>>() != layout {
        write_json_at(d.am_mut(), c, am::LAYOUT, &Value::Array(layout));
    }
    let fields = map_at(d, c, am::FIELDS, ObjType::Map);
    let held: Vec<String> = d.am().keys(&fields).collect();
    for key in held {
        if now.get(&key).is_none() {
            d.am_mut().delete(&fields, key.as_str()).unwrap();
        }
    }
    for item in now.items() {
        let PayloadItem::Field { key, value } = item else { continue };
        let want = value.as_json();
        if was.get(key).map(|v| v.as_json()) == Some(want) && d.am().get(&fields, key.as_str()).unwrap().is_some() {
            continue;
        }
        put_field(d, &fields, key, was.get(key).map(|v| v.as_json()), want);
    }
    for (key, want) in [(am::EXT, now.ext()), (am::SEED, now.seed())] {
        let want = want.cloned().map(Value::Object).unwrap_or_else(|| json!({}));
        write_json_at(d.am_mut(), c, key, &want);
    }
    let nested: Vec<Value> = now
        .nested_comments()
        .iter()
        .map(|n| serde_json::to_value(NestedCommentV0_134_0::from(n)).expect("a nested comment serializes"))
        .collect();
    write_json_at(d.am_mut(), c, am::NESTED, &Value::Array(nested));
}

/// Puts field `key` of `fields` to `want`: a content value over a content
/// value by the change between them, so concurrent edits to it merge, and
/// anything else as data.
fn put_field(d: &mut Doc, fields: &ObjId, key: &str, was: Option<&Value>, want: &Value) {
    if is_content(want) {
        let content = Content::from_value(want).expect("a content field reads");
        if let (Some(root), Some(_)) = (d.value(fields, key), was.filter(|w| is_content(w))) {
            let now = d.view(&root).content;
            if now == content {
                return;
            }
            if let Ok(change) = quillmark_content::diff::diff(&now, &content)
                && d.apply(&root, &change, None).is_ok()
            {
                return;
            }
        }
        if d.am().get(fields, key).unwrap().is_some() {
            d.am_mut().delete(fields, key).unwrap();
        }
        d.put_value(fields, key, &content);
        return;
    }
    if d.value(fields, key).is_some() {
        d.am_mut().delete(fields, key).unwrap();
    }
    write_json_at(d.am_mut(), fields, key, want);
}

/// The card the card map `c` holds, and what a payload the merge left
/// outside the model reads through.
pub(crate) fn read_card(d: &Doc, c: &ObjId) -> Result<Card, String> {
    let fields = get_obj(d, c, am::FIELDS);
    let value_of = |key: &str| -> Option<Value> {
        let f = fields.as_ref()?;
        match d.value(f, key) {
            Some(root) => Some(d.view(&root).content.to_value()),
            None => match d.am().get(f, key).ok()?? {
                (AmValue::Object(_), o) => Some(hydrate(d.am(), &o)),
                (AmValue::Scalar(s), _) => Some(scalar_json(&s)),
            },
        }
    };
    let layout = get_obj(d, c, am::LAYOUT).map(|l| hydrate(d.am(), &l)).unwrap_or_else(|| json!([]));
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in layout.as_array().into_iter().flatten() {
        let mut e = entry.clone();
        match e["type"].as_str() {
            Some("field") => {
                let Some(key) = e["key"].as_str().map(str::to_string) else { continue };
                let Some(v) = value_of(&key) else { continue };
                if !seen.insert(key) {
                    continue;
                }
                e["value"] = v;
            }
            Some(k @ ("ext" | "seed")) => {
                let v = get_obj(d, c, k).map(|o| hydrate(d.am(), &o)).unwrap_or_else(|| json!({}));
                e["value"] = v;
            }
            _ => {}
        }
        items.push(e);
    }
    if let Some(f) = fields.as_ref() {
        for key in d.am().keys(f).collect::<Vec<_>>() {
            if seen.insert(key.clone())
                && let Some(v) = value_of(&key)
            {
                items.push(json!({ "type": "field", "key": key, "value": v }));
            }
        }
    }
    let nested = get_obj(d, c, am::NESTED).map(|n| hydrate(d.am(), &n)).unwrap_or_else(|| json!([]));
    let nested: Vec<NestedCommentV0_134_0> = serde_json::from_value(nested).map_err(|e| e.to_string())?;
    let items = items.into_iter().filter_map(item_from).collect();
    let payload = Payload::from_items_with_nested(items, nested.into_iter().map(Into::into).collect());
    let body = d.value(c, am::BODY).map(|b| d.view(&b).content).unwrap_or_else(Content::empty);
    Ok(Card::from_parts(payload, body))
}

/// The payload item a layout entry, its value filled in, spells; none for
/// one a merge left unreadable.
fn item_from(e: Value) -> Option<PayloadItem> {
    let text = |k: &str| e.get(k).and_then(Value::as_str).map(str::to_string);
    Some(match e["type"].as_str()? {
        "quill" => PayloadItem::Quill { reference: text("value")?.parse().ok()? },
        "kind" => PayloadItem::Kind { value: text("value")? },
        "id" => PayloadItem::Id { value: text("value")? },
        "ext" => PayloadItem::Meta { key: MetaKey::Ext, value: e["value"].as_object()?.clone() },
        "seed" => PayloadItem::Meta { key: MetaKey::Seed, value: e["value"].as_object()?.clone() },
        "field" => PayloadItem::Field { key: text("key")?, value: QuillValue::from_json(e["value"].clone()) },
        "comment" => PayloadItem::Comment { text: text("text")?, inline: e.get("inline").and_then(Value::as_bool).unwrap_or(false) },
        _ => return None,
    })
}

fn scalar_json(s: &quillmark_automerge::automerge::ScalarValue) -> Value {
    use quillmark_automerge::automerge::ScalarValue as S;
    match s {
        S::Null => Value::Null,
        S::Boolean(b) => Value::Bool(*b),
        S::Int(i) => json!(i),
        S::Uint(u) => json!(u),
        S::F64(f) => json!(f),
        S::Str(s) => Value::String(s.to_string()),
        other => Value::String(other.to_string()),
    }
}

/// The main card and the cards in document order the document reads as.
pub(crate) fn read(d: &Doc) -> Result<(Card, Vec<Card>), String> {
    let main = read_card(d, &d.main())?;
    let all = d.cards();
    let mut cards = Vec::new();
    for id in d.order() {
        let c = get_obj(d, &all, &id).expect("order names held cards");
        cards.push(read_card(d, &c)?);
    }
    Ok((main, cards))
}

/// Writes what differs between `was` and `now` into the document: each
/// card's payload and body, and the cards added, removed and moved.
pub(crate) fn flush(d: &mut Doc, was: (&Card, &[Card]), now: (&Card, &[Card])) {
    let m = d.main();
    flush_card(d, &m, was.0, now.0);
    let all = d.cards();
    let held: std::collections::HashMap<&str, &Card> = was.1.iter().filter_map(|c| Some((c.id()?, c))).collect();
    let wanted: Vec<&str> = now.1.iter().filter_map(|c| c.id()).collect();
    for card in now.1 {
        let Some(id) = card.id() else { continue };
        match (held.get(id), get_obj(d, &all, id)) {
            (Some(w), Some(c)) => flush_card(d, &c, w, card),
            (_, existing) => {
                if existing.is_some() {
                    d.am_mut().delete(&all, id).unwrap();
                }
                let c = d.am_mut().put_object(&all, id, ObjType::Map).unwrap();
                d.card_skeleton(&c);
                write_card(d, &c, card);
            }
        }
    }
    for id in held.keys() {
        if !wanted.contains(id) {
            d.am_mut().delete(&all, *id).unwrap();
        }
    }
    let order = order_obj(d);
    let have: Vec<String> = d
        .am()
        .values(&order)
        .filter_map(|(v, _)| match v {
            AmValue::Scalar(s) => s.as_str().map(str::to_string),
            _ => None,
        })
        .collect();
    if have.iter().map(String::as_str).collect::<Vec<_>>() != wanted {
        let want: Vec<Value> = wanted.iter().map(|s| Value::String(s.to_string())).collect();
        write_json_at(d.am_mut(), &ROOT, am::ORDER, &Value::Array(want));
    }
    d.am_mut().commit();
}

fn flush_card(d: &mut Doc, c: &ObjId, was: &Card, now: &Card) {
    if was.payload() != now.payload() {
        write_payload(d, c, was.payload(), now.payload());
    }
    if was.body() != now.body() {
        let root = d.value(c, am::BODY);
        let landed = root.as_ref().is_some_and(|root| {
            let held = d.view(root).content;
            quillmark_content::diff::diff(&held, now.body()).ok().is_some_and(|change| d.apply(root, &change, None).is_ok())
        });
        if !landed {
            if d.am().get(c, am::BODY).unwrap().is_some() {
                d.am_mut().delete(c, am::BODY).unwrap();
            }
            d.put_value(c, am::BODY, now.body());
        }
    }
}
