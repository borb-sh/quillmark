//! A document's cards as the Automerge document holds them.
//!
//! A card map holds its `layout`, each payload item in source order with a
//! field named by key, its `fields`, each value by key, its `ext` and `seed`
//! maps, its `nestedComments` and its `body`. A field holding a content value
//! holds it as a text value, so it merges as text does; every other value is
//! plain data, a map key by key and a list by element, and a scalar last
//! writer wins; a content value nested in such a value is plain data too,
//! and reads through repair. [`Card`] is that shape as plain data, the one a
//! host reads its cards from and writes each card edit into.

use automerge::transaction::Transactable;
use automerge::{ObjId, ObjType, ROOT, ReadDoc, Value as AmValue};
use crate::content::model::Content;
use crate::content::repair::{Raw, repair};
use crate::content::units::is_content_value;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::json::{hydrate, json_scalar, write_json_at};
use super::{BODY, Doc, EXT, FIELDS, LAYOUT, NESTED, ORDER, SEED, value};

/// One entry of a card's `layout`: a payload item in source order, a field
/// by its key and `$ext` and `$seed` without the maps the card holds apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum LayoutItem {
    Quill { value: String },
    Kind { value: String },
    Id { value: String },
    Ext,
    Seed,
    Field { key: String },
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// A YAML comment inside a field's value, at `position` among the items of
/// the container `container_path` names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedComment {
    pub container_path: Vec<PathStep>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathStep {
    Key(String),
    Index(usize),
}

/// A card as its map holds it. `fields` holds each field's value by key, a
/// content value as its JSON.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Card {
    pub layout: Vec<LayoutItem>,
    pub fields: Vec<(String, Value)>,
    pub ext: Map<String, Value>,
    pub seed: Map<String, Value>,
    pub nested: Vec<NestedComment>,
    pub body: Content,
}

impl Card {
    /// The `$id` its layout holds.
    pub fn id(&self) -> Option<&str> {
        self.layout.iter().find_map(|i| match i {
            LayoutItem::Id { value } => Some(value.as_str()),
            _ => None,
        })
    }

    fn field(&self, key: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn payload_eq(&self, other: &Card) -> bool {
        (&self.layout, &self.fields, &self.ext, &self.seed, &self.nested)
            == (&other.layout, &other.fields, &other.ext, &other.seed, &other.nested)
    }
}

/// Whether a field's value is a content value.
pub fn is_content(v: &Value) -> bool {
    v.get("text").is_some_and(Value::is_string)
        && v.get("blocks").is_some_and(Value::is_array)
        && Content::from_value(v).is_ok()
}

/// A new document holding `main` and each of `cards` that holds an id,
/// built under the first actor: equal cards build equal bytes in every
/// process.
pub fn build(main: &Card, cards: &[Card]) -> Doc {
    Doc::building(|d| {
        let m = d.main();
        write_card(d, &m, None, main);
        let (all, order) = (d.cards(), order_obj(d));
        for card in cards {
            let Some(id) = card.id() else { continue };
            let c = d.doc.put_object(&all, id, ObjType::Map).unwrap();
            d.card_skeleton(&c);
            write_card(d, &c, None, card);
            let at = d.doc.length(&order);
            d.doc.insert(&order, at, id).unwrap();
        }
    })
}

/// The main card and the cards in document order the document reads as.
pub fn read(d: &Doc) -> Result<(Card, Vec<Card>), String> {
    let main = read_card(d, &d.main())?;
    let all = d.cards();
    let mut cards = Vec::new();
    for id in d.order() {
        let c = value::obj(&d.doc, &all, &id).expect("order names held cards");
        cards.push(read_card(d, &c)?);
    }
    Ok((main, cards))
}

/// What a stretch of patches reached among the cards.
#[derive(Debug, Default)]
pub struct Touched {
    /// A patch outside the cards: the whole document reads again.
    pub whole: bool,
    pub main: bool,
    /// The list of cards changed: it reads again from `order`.
    pub moved: bool,
    /// The `$id` of each card a patch reached.
    pub cards: std::collections::BTreeSet<String>,
}

/// What `patches`, as [`patch::between`](super::patch::between) spells
/// them, reached among the cards.
pub fn touched(patches: &[Value]) -> Touched {
    let mut t = Touched::default();
    for p in patches {
        let path = p["path"].as_array().map(Vec::as_slice).unwrap_or_default();
        match (path.first().and_then(Value::as_str), path.get(1).and_then(Value::as_str)) {
            (Some(super::MAIN), _) => t.main = true,
            (Some(super::CARDS), Some(id)) => {
                t.moved |= path.len() == 2;
                t.cards.insert(id.to_string());
            }
            (Some(ORDER), _) => t.moved = true,
            _ => t.whole = true,
        }
    }
    t
}

/// The card held under `id`.
pub fn read_id(d: &Doc, id: &str) -> Result<Card, String> {
    read_card(d, &value::obj(&d.doc, &d.cards(), id).ok_or_else(|| format!("no card holds `{id}`"))?)
}

/// The card the card map `c` holds: each layout entry a merge left
/// readable, and each field by key.
pub fn read_card(d: &Doc, c: &ObjId) -> Result<Card, String> {
    let list = |key: &str| {
        value::obj(&d.doc, c, key)
            .map(|o| hydrate(&d.doc, &o))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };
    let map = |key: &str| match value::obj(&d.doc, c, key).map(|o| hydrate(&d.doc, &o)) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let mut fields = Vec::new();
    if let Some(f) = value::obj(&d.doc, c, FIELDS) {
        for key in d.doc.keys(&f).collect::<Vec<_>>() {
            let v = match d.value(&f, &key) {
                Some(root) => d.view(&root).content.to_value(),
                None => match d.doc.get(&f, key.as_str()).ok().flatten() {
                    Some((AmValue::Object(_), o)) => repaired(hydrate(&d.doc, &o)),
                    Some((AmValue::Scalar(s), _)) => json_scalar(&s),
                    None => continue,
                },
            };
            fields.push((key, v));
        }
    }
    let nested = serde_json::from_value(Value::Array(list(NESTED))).map_err(|e| e.to_string())?;
    Ok(Card {
        layout: list(LAYOUT).into_iter().filter_map(|e| serde_json::from_value(e).ok()).collect(),
        fields,
        ext: map(EXT),
        seed: map(SEED),
        nested,
        body: d.value(c, BODY).map(|b| d.view(&b).content).unwrap_or_else(Content::empty),
    })
}

/// `v` with each content value it holds, at any depth, read through repair.
fn repaired(v: Value) -> Value {
    match v {
        v if is_content_value(&v) => match Raw::from_value(&v) {
            Ok(raw) => repair(raw).0.to_value(),
            Err(_) => v,
        },
        Value::Array(a) => Value::Array(a.into_iter().map(repaired).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, x)| (k, repaired(x))).collect()),
        v => v,
    }
}

/// Writes what differs between `was` and `now` into the document, each
/// card's payload and body and the cards added, removed and moved, and
/// commits.
pub fn write_all(d: &mut Doc, was: (&Card, &[Card]), now: (&Card, &[Card])) {
    let m = d.main();
    write_card(d, &m, Some(was.0), now.0);
    let all = d.cards();
    let held: std::collections::HashMap<&str, &Card> = was.1.iter().filter_map(|c| Some((c.id()?, c))).collect();
    let wanted: Vec<&str> = now.1.iter().filter_map(Card::id).collect();
    for card in now.1 {
        let Some(id) = card.id() else { continue };
        match (held.get(id), value::obj(&d.doc, &all, id)) {
            (Some(w), Some(c)) => write_card(d, &c, Some(w), card),
            _ => {
                let c = d.doc.put_object(&all, id, ObjType::Map).unwrap();
                d.card_skeleton(&c);
                write_card(d, &c, None, card);
            }
        }
    }
    for id in held.keys() {
        if !wanted.contains(id) {
            d.doc.delete(&all, *id).unwrap();
        }
    }
    let order = order_obj(d);
    let have: Vec<String> = d
        .doc
        .values(&order)
        .filter_map(|(v, _)| match v {
            AmValue::Scalar(s) => s.as_str().map(str::to_string),
            _ => None,
        })
        .collect();
    if have != wanted {
        let want: Vec<Value> = wanted.iter().map(|s| Value::String(s.to_string())).collect();
        write_json_at(&mut d.doc, &ROOT, ORDER, &Value::Array(want));
    }
    d.doc.commit();
}

/// Writes what the card changed since it read as `was` into the document as
/// one change, the main card where `main` and otherwise the card `was`'s
/// `$id` names, and answers it as it reads after. A card whose `$id`
/// changed is put under the new one at the old one's place in `order`.
pub fn write_one(d: &mut Doc, main: bool, was: &Card, now: &Card) -> Card {
    let c = if main {
        let m = d.main();
        write_card(d, &m, Some(was), now);
        m
    } else {
        let all = d.cards();
        let (old, new) = (was.id().expect("a held card has an id"), now.id().expect("a held card has an id"));
        let held = value::obj(&d.doc, &all, old).expect("a held card is in the document");
        if old == new {
            write_card(d, &held, Some(was), now);
            held
        } else {
            d.doc.delete(&all, old).unwrap();
            let c = d.doc.put_object(&all, new, ObjType::Map).unwrap();
            d.card_skeleton(&c);
            write_card(d, &c, None, now);
            let order = order_obj(d);
            if let Some(at) = position(d, &order, old) {
                d.doc.put(&order, at, new).unwrap();
            }
            c
        }
    };
    d.doc.commit();
    read_card(d, &c).expect("a written card reads")
}

/// Puts `card` among the cards at `index` of `order` as one change, and
/// answers it as it reads.
pub fn insert(d: &mut Doc, index: usize, card: &Card) -> Card {
    let id = card.id().expect("a placed card has an id");
    let all = d.cards();
    let c = d.doc.put_object(&all, id, ObjType::Map).unwrap();
    d.card_skeleton(&c);
    write_card(d, &c, None, card);
    let order = order_obj(d);
    let at = nth_slot(d, &order, index);
    d.doc.insert(&order, at, id).unwrap();
    d.doc.commit();
    read_card(d, &c).expect("a placed card reads")
}

/// Takes the card `id` out of `cards` and `order` as one change.
pub fn remove(d: &mut Doc, id: &str) {
    let all = d.cards();
    if d.doc.get(&all, id).unwrap().is_some() {
        d.doc.delete(&all, id).unwrap();
    }
    let order = order_obj(d);
    while let Some(at) = position(d, &order, id) {
        d.doc.delete(&order, at).unwrap();
    }
    d.doc.commit();
}

/// Moves the card `id` to `index` of the order as one change: Automerge has
/// no move, so its id leaves `order` and goes in again, and an edit inside
/// the card concurrent with the move keeps.
pub fn move_to(d: &mut Doc, id: &str, index: usize) {
    let order = order_obj(d);
    while let Some(at) = position(d, &order, id) {
        d.doc.delete(&order, at).unwrap();
    }
    let at = nth_slot(d, &order, index);
    d.doc.insert(&order, at, id).unwrap();
    d.doc.commit();
}

/// The index in the `order` list of the first entry spelling `id`.
fn position(d: &Doc, order: &ObjId, id: &str) -> Option<usize> {
    d.doc.values(order).position(|(v, _)| matches!(&v, AmValue::Scalar(s) if s.as_str() == Some(id)))
}

/// The index in the `order` list before which the card the document reads
/// at `index` of its order sits, or the list's end: the list can hold ids
/// the read skips, a card removed concurrently or an id held twice.
fn nth_slot(d: &Doc, order: &ObjId, index: usize) -> usize {
    let read = d.order();
    let Some(id) = read.get(index) else { return d.doc.length(order) };
    position(d, order, id).unwrap_or_else(|| d.doc.length(order))
}

/// The value at the body of the card `card` names, or the main card's, or
/// at its field `field`, and whether it is put now: a field holding its
/// content as JSON or holding nothing is put as a text value of `content`,
/// and committed. None where no card holds that id; an error `content`
/// answers, with nothing put.
pub fn value_at<E>(d: &mut Doc, card: Option<&str>, field: Option<&str>, content: impl FnOnce() -> Result<Content, E>) -> Result<Option<(super::ValueId, bool)>, E> {
    let c = match card {
        None => d.main(),
        Some(id) => match value::obj(&d.doc, &d.cards(), id) {
            Some(c) => c,
            None => return Ok(None),
        },
    };
    let Some(name) = field else {
        return Ok(Some((super::ValueId(d.value(&c, BODY).expect("a card holds its body")), false)));
    };
    let fields = value::obj(&d.doc, &c, FIELDS).expect("a card holds its fields");
    if let Some(v) = d.value(&fields, name) {
        return Ok(Some((super::ValueId(v), false)));
    }
    let content = content()?;
    let v = d.put_value(&fields, name, &content);
    d.doc.commit();
    Ok(Some((super::ValueId(v), true)))
}

fn order_obj(d: &Doc) -> ObjId {
    value::obj(&d.doc, &ROOT, ORDER).expect("a document holds its order")
}

/// Writes `now` into the card map `c` over `was`, what it held, or over an
/// empty card where `was` is none.
fn write_card(d: &mut Doc, c: &ObjId, was: Option<&Card>, now: &Card) {
    let empty = Card::default();
    let fresh = was.is_none();
    let was = was.unwrap_or(&empty);
    if fresh || !was.payload_eq(now) {
        write_payload(d, c, was, now);
    }
    if fresh || (was.body != now.body && !d.value(c, BODY).is_some_and(|root| merge_value(d, &root, &now.body))) {
        d.put_value(c, BODY, &now.body);
    }
}

/// Writes `want` over the value `root` by the change between them, so a
/// concurrent edit to it merges; false where the change does not land.
fn merge_value(d: &mut Doc, root: &ObjId, want: &Content) -> bool {
    let now = d.view(root).content;
    now == *want
        || crate::content::diff::diff(&now, want)
            .ok()
            .is_some_and(|change| d.apply(root, &change, None).is_ok())
}

fn write_payload(d: &mut Doc, c: &ObjId, was: &Card, now: &Card) {
    if was.layout != now.layout {
        let layout = serde_json::to_value(&now.layout).expect("a layout serializes");
        write_json_at(&mut d.doc, c, LAYOUT, &layout);
    }
    let fields = match value::obj(&d.doc, c, FIELDS) {
        Some(f) => f,
        None => d.doc.put_object(c, FIELDS, ObjType::Map).unwrap(),
    };
    for key in d.doc.keys(&fields).collect::<Vec<_>>() {
        if now.field(&key).is_none() {
            d.doc.delete(&fields, key.as_str()).unwrap();
        }
    }
    for (key, want) in &now.fields {
        let had = was.field(key);
        if had == Some(want) && d.doc.get(&fields, key.as_str()).unwrap().is_some() {
            continue;
        }
        put_field(d, &fields, key, had, want);
    }
    write_json_at(&mut d.doc, c, EXT, &Value::Object(now.ext.clone()));
    write_json_at(&mut d.doc, c, SEED, &Value::Object(now.seed.clone()));
    let nested = serde_json::to_value(&now.nested).expect("nested comments serialize");
    write_json_at(&mut d.doc, c, NESTED, &nested);
}

/// Puts field `key` of `fields` to `want`: a content value over a content
/// value by the change between them, and anything else as data.
fn put_field(d: &mut Doc, fields: &ObjId, key: &str, was: Option<&Value>, want: &Value) {
    if is_content(want) {
        let content = Content::from_value(want).expect("a content field reads");
        if let (Some(root), Some(_)) = (d.value(fields, key), was.filter(|w| is_content(w)))
            && merge_value(d, &root, &content)
        {
            return;
        }
        d.put_value(fields, key, &content);
        return;
    }
    if d.value(fields, key).is_some() {
        d.doc.delete(fields, key).unwrap();
    }
    write_json_at(&mut d.doc, fields, key, want);
}
