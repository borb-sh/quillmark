//! A document's cards as the Automerge document holds them.
//!
//! A card map holds its `layout`, each payload item in source order with a
//! field named by key, its `fields`, each value by key, its `ext` and `seed`
//! maps, its `nestedComments` and its `body`. Each content value in a field,
//! the field's value or one at any depth in it, is a text value, so it merges
//! as text does; every other value is plain data, a map key by key and a list
//! by element, and a scalar last writer wins. [`Card`] is that shape as plain
//! data, the one a host reads its cards from and writes each card edit into.

use automerge::transaction::Transactable;
use automerge::{ObjId, ObjType, ROOT, ReadDoc, Value as AmValue};
use crate::content::model::Content;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::json::{hydrate, hydrate_field, json_scalar, write_field_at, write_json_at};
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
/// content value as its JSON: the shape a card is placed in and read as.
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
}

/// Whether a field's value, or a value at any depth in it, is a content
/// value: the document holds it as a text value.
pub fn is_content(v: &Value) -> bool {
    value::content_of(v).is_some()
}

/// A new document holding `main` and each of `cards` that holds an id,
/// built under the first actor: equal cards build equal bytes in every
/// process.
pub fn build(main: &Card, cards: &[Card]) -> Doc {
    Doc::building(|d| {
        let m = d.main();
        fill(d, &m, main);
        let order = order_obj(d);
        for card in cards {
            let Some(id) = card.id() else { continue };
            put(d, id, card);
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

/// The card held under `id`.
pub fn read_id(d: &Doc, id: &str) -> Result<Card, String> {
    read_card(d, &value::obj(&d.doc, &d.cards(), id).ok_or_else(|| format!("no card holds `{id}`"))?)
}

/// The card the card map `c` holds: each layout entry a merge left
/// readable, and each field by key.
pub fn read_card(d: &Doc, c: &ObjId) -> Result<Card, String> {
    read_card_with(d, c, true)
}

/// [`read_card`], its fields and body read where `whole` and left empty
/// otherwise.
fn read_card_with(d: &Doc, c: &ObjId, whole: bool) -> Result<Card, String> {
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
    if let Some(f) = value::obj(&d.doc, c, FIELDS).filter(|_| whole) {
        for key in d.doc.keys(&f).collect::<Vec<_>>() {
            if let Some(v) = read_field(d, c, &key) {
                fields.push((key, v));
            }
        }
    }
    let nested = serde_json::from_value(Value::Array(list(NESTED))).map_err(|e| e.to_string())?;
    Ok(Card {
        layout: list(LAYOUT).into_iter().filter_map(|e| serde_json::from_value(e).ok()).collect(),
        fields,
        ext: map(EXT),
        seed: map(SEED),
        nested,
        body: d.value(c, BODY).filter(|_| whole).map(|b| d.view(&b).content).unwrap_or_else(Content::empty),
    })
}

/// Puts `card` among the cards at `index` of `order` as one change, and
/// answers it as it reads.
pub fn insert(d: &mut Doc, index: usize, card: &Card) -> Card {
    let id = card.id().expect("a placed card has an id");
    let c = put(d, id, card);
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


/// Puts `card` under `id` among the cards as a new card map, with no place
/// in `order`.
pub fn put(d: &mut Doc, id: &str, card: &Card) -> ObjId {
    let all = d.cards();
    let c = d.doc.put_object(&all, id, ObjType::Map).unwrap();
    d.card_skeleton(&c);
    fill(d, &c, card);
    c
}

/// Fills the card map `c`, an empty card, with `card`.
fn fill(d: &mut Doc, c: &ObjId, card: &Card) {
    put_layout(d, c, &card.layout);
    for (key, v) in &card.fields {
        put_field(d, c, key, v);
    }
    put_ext(d, c, &card.ext);
    put_seed(d, c, &card.seed);
    put_nested(d, c, &card.nested);
    put_body(d, c, &card.body);
}

/// The card map of the main card where `id` is none, and otherwise of the
/// card `id` names.
pub fn card_obj(d: &Doc, id: Option<&str>) -> Option<ObjId> {
    match id {
        None => Some(d.main()),
        Some(id) => value::obj(&d.doc, &d.cards(), id),
    }
}

/// Puts `layout` over the layout of the card map `c`.
pub fn put_layout(d: &mut Doc, c: &ObjId, layout: &[LayoutItem]) {
    let layout = serde_json::to_value(layout).expect("a layout serializes");
    write_json_at(&mut d.doc, c, LAYOUT, &layout);
}

/// Puts `want` at field `key` of the card map `c`, each content value in it a
/// text value written in place over the one held there.
pub fn put_field(d: &mut Doc, c: &ObjId, key: &str, want: &Value) {
    let fields = value::obj(&d.doc, c, FIELDS).expect("a card holds its fields");
    write_field_at(&mut d.doc, &fields, key, want);
}

/// Takes field `key` out of the card map `c`.
pub fn remove_field(d: &mut Doc, c: &ObjId, key: &str) {
    let fields = value::obj(&d.doc, c, FIELDS).expect("a card holds its fields");
    if d.doc.get(&fields, key).unwrap().is_some() {
        d.doc.delete(&fields, key).unwrap();
    }
}

pub fn put_ext(d: &mut Doc, c: &ObjId, ext: &Map<String, Value>) {
    write_json_at(&mut d.doc, c, EXT, &Value::Object(ext.clone()));
}

pub fn put_seed(d: &mut Doc, c: &ObjId, seed: &Map<String, Value>) {
    write_json_at(&mut d.doc, c, SEED, &Value::Object(seed.clone()));
}

pub fn put_nested(d: &mut Doc, c: &ObjId, nested: &[NestedComment]) {
    let nested = serde_json::to_value(nested).expect("nested comments serialize");
    write_json_at(&mut d.doc, c, NESTED, &nested);
}

/// Writes `body` over the body of the card map `c` in place.
pub fn put_body(d: &mut Doc, c: &ObjId, body: &Content) {
    match d.value(c, BODY) {
        Some(root) => value::rewrite(&mut d.doc, &root, body),
        None => {
            d.put_value(c, BODY, body);
        }
    }
}

/// Puts the card `id` names under `card`'s `$id` instead, at its place in
/// `order`: Automerge moves no map, so the card is put anew there.
pub fn rename(d: &mut Doc, id: &str, card: &Card) {
    let new = card.id().expect("a held card has an id");
    let all = d.cards();
    d.doc.delete(&all, id).unwrap();
    put(d, new, card);
    let order = order_obj(d);
    if let Some(at) = position(d, &order, id) {
        d.doc.put(&order, at, new).unwrap();
    }
}

/// Puts `ids` over `order`, the cards `cards` holds and no others: each card
/// map an id leaves goes.
pub fn put_order(d: &mut Doc, ids: &[&str]) {
    let all = d.cards();
    for held in d.doc.keys(&all).collect::<Vec<_>>() {
        if !ids.contains(&held.as_str()) {
            d.doc.delete(&all, held.as_str()).unwrap();
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
    if have != ids {
        let want: Vec<Value> = ids.iter().map(|s| Value::String(s.to_string())).collect();
        write_json_at(&mut d.doc, &ROOT, ORDER, &Value::Array(want));
    }
}

/// One card's part a stretch of changes reached: its body, a field by key,
/// or the rest of its payload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Part {
    Body,
    Field(String),
    Payload,
    Card,
}

/// What a stretch of changes left of the cards a host holds: Automerge's
/// patches, spelled, and per card each part they reached as it reads now.
#[derive(Debug, Default)]
pub struct Refreshed {
    pub patches: Vec<Value>,
    /// A patch outside the cards: the whole document reads again.
    pub whole: bool,
    /// The card list changed: the ids in document order now.
    pub order: Option<Vec<String>>,
    /// Each card a patch reached, by `$id` (none for the main card).
    pub cards: Vec<(Option<String>, Reread)>,
}

/// One card's parts as they read now.
#[derive(Debug, Default)]
pub struct Reread {
    /// The whole card, where a patch reached past its parts, or `None`
    /// where the card is gone.
    pub card: Option<Option<Card>>,
    /// The layout, `ext`, `seed` and comments, where a patch reached the
    /// payload past its fields' values; `fields` and `body` are empty.
    pub payload: Option<Card>,
    /// Each field a patch reached, `None` where it is gone.
    pub fields: Vec<(String, Option<Value>)>,
    pub body: Option<Content>,
}

/// The patches taking the document from `before` to now, spelled in `u`,
/// and what they left of each card part they reached. A body, or a field
/// holding a text value, that `held` answers for, as the host holds it, and
/// that the patches only splice, is followed over that; every other part
/// reached reads again.
pub fn refresh(d: &mut Doc, before: &super::Heads, held: impl Fn(Option<&str>, Option<&str>) -> Option<Content>, u: impl crate::content::units::Units) -> Refreshed {
    d.doc.commit();
    let (before, after) = (before.hashes(), d.doc.get_heads());
    let raw = d.doc.diff(before, &after);
    let mut out = Refreshed::default();
    let mut reached: std::collections::BTreeMap<Option<String>, std::collections::BTreeSet<Part>> = Default::default();
    for p in &raw {
        let keys = super::patch::keys(p);
        let key = |i: usize| keys.get(i).map(String::as_str);
        let card = match (key(0), key(1)) {
            (Some(super::MAIN), _) => None,
            (Some(super::CARDS), Some(id)) => Some(id.to_string()),
            (Some(ORDER), _) => {
                out.order = Some(Vec::new());
                continue;
            }
            _ => {
                out.whole = true;
                continue;
            }
        };
        let at = usize::from(card.is_some());
        let part = match (key(at + 1), key(at + 2)) {
            _ if keys.len() <= at + 2 => Part::Card,
            (Some(BODY), _) => Part::Body,
            (Some(FIELDS), Some(k)) => Part::Field(k.to_string()),
            (Some(LAYOUT | EXT | SEED | NESTED), _) => Part::Payload,
            _ => Part::Card,
        };
        if card.is_some() && part == Part::Card {
            out.order = Some(Vec::new());
        }
        reached.entry(card).or_default().insert(part);
    }
    if out.whole {
        out.patches = super::patch::spell(&d.doc, before, raw, u, Vec::new());
        return out;
    }
    let mut followed = Vec::new();
    for (card, parts) in &reached {
        if parts.contains(&Part::Card) {
            continue;
        }
        let Some(c) = card_obj(d, card.as_deref()) else { continue };
        for part in parts {
            let (root, field) = match part {
                Part::Body => (d.value(&c, BODY), None),
                Part::Field(k) => (value::obj(&d.doc, &c, FIELDS).and_then(|f| d.value(&f, k)), Some(k.as_str())),
                _ => continue,
            };
            if let (Some(root), Some(was)) = (root, held(card.as_deref(), field)) {
                followed.push(((card.clone(), part.clone()), root, was));
            }
        }
    }
    let values: Vec<(ObjId, Content)> = followed.iter().map(|(_, root, was)| (root.clone(), was.clone())).collect();
    let (patches, follows) = super::patch::follow_all(&d.doc, before, raw, u, values);
    out.patches = patches;
    let mut done: std::collections::HashMap<(Option<String>, Part), Content> = Default::default();
    for ((place, root, _), f) in followed.into_iter().zip(follows) {
        if let Some(content) = f {
            debug_assert_eq!(content, d.view(&root).content, "the patches leave the value the document reads");
            done.insert(place, content);
        }
    }
    if out.order.is_some() {
        out.order = Some(d.order());
    }
    for (card, parts) in reached {
        let Some(c) = card_obj(d, card.as_deref()) else {
            out.cards.push((card, Reread { card: Some(None), ..Default::default() }));
            continue;
        };
        let mut r = Reread::default();
        if parts.contains(&Part::Card) {
            r.card = Some(Some(read_card(d, &c).expect("a held card reads")));
            out.cards.push((card, r));
            continue;
        }
        if parts.contains(&Part::Payload) {
            r.payload = Some(read_card_with(d, &c, false).expect("a held card reads"));
        }
        for part in parts {
            match part {
                Part::Body => {
                    r.body = Some(match done.remove(&(card.clone(), Part::Body)) {
                        Some(content) => content,
                        None => d.value(&c, BODY).map(|b| d.view(&b).content).unwrap_or_else(Content::empty),
                    });
                }
                Part::Field(k) => {
                    let v = match done.remove(&(card.clone(), Part::Field(k.clone()))) {
                        Some(content) => Some(content.to_value()),
                        None => read_field(d, &c, &k),
                    };
                    r.fields.push((k, v));
                }
                _ => {}
            }
        }
        out.cards.push((card, r));
    }
    out
}

/// Field `key` of the card map `c` as it reads, `None` where it holds none.
fn read_field(d: &Doc, c: &ObjId, key: &str) -> Option<Value> {
    let f = value::obj(&d.doc, c, FIELDS)?;
    match d.doc.get(&f, key).ok().flatten()? {
        (AmValue::Object(_), o) => Some(hydrate_field(&d.doc, &o)),
        (AmValue::Scalar(s), _) => Some(json_scalar(&s)),
    }
}
