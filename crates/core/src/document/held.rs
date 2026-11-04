//! A document's cards held in its Automerge document: each [`Card`] as the
//! plain data the model crate places and reads it in
//! ([`quillmark_automerge::model::cards`]), each card edit written part by
//! part, and the cards a stretch of changes reached read again from its
//! patches.

use quillmark_automerge::model::Doc;
use quillmark_automerge::model::cards::{self, LayoutItem, NestedComment as HeldComment, PathStep};

use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::units::Units;

use super::payload::{MetaKey, PayloadItem, Touched, trim_comment_text};
use super::prescan::NestedComment;
use super::{Card, Payload};
use crate::value::{PathSegment, QuillValue};

fn layout_item(item: &PayloadItem) -> LayoutItem {
    match item {
        PayloadItem::Quill { reference } => LayoutItem::Quill { value: reference.to_string() },
        PayloadItem::Kind { value } => LayoutItem::Kind { value: value.clone() },
        PayloadItem::Id { value } => LayoutItem::Id { value: value.clone() },
        PayloadItem::Meta { key: MetaKey::Ext, .. } => LayoutItem::Ext,
        PayloadItem::Meta { key: MetaKey::Seed, .. } => LayoutItem::Seed,
        PayloadItem::Field { key, .. } => LayoutItem::Field { key: key.clone() },
        PayloadItem::Comment { text, inline } => LayoutItem::Comment { text: text.clone(), inline: *inline },
    }
}

fn layout(payload: &Payload) -> Vec<LayoutItem> {
    payload.items().iter().map(layout_item).collect()
}

fn nested(payload: &Payload) -> Vec<HeldComment> {
    payload
        .nested_comments()
        .iter()
        .map(|n| HeldComment {
            container_path: n
                .container_path
                .iter()
                .map(|s| match s {
                    PathSegment::Key(k) => PathStep::Key(k.clone()),
                    PathSegment::Index(i) => PathStep::Index(*i),
                })
                .collect(),
            position: n.position,
            text: n.text.clone(),
            inline: n.inline,
        })
        .collect()
}

fn to_model(card: &Card) -> cards::Card {
    let payload = card.payload();
    cards::Card {
        layout: layout(payload),
        fields: payload.iter().map(|(k, v)| (k.clone(), v.as_json().clone())).collect(),
        ext: payload.ext().cloned().unwrap_or_default(),
        seed: payload.seed().cloned().unwrap_or_default(),
        nested: nested(payload),
        body: card.body().clone(),
    }
}

/// The card `c` holds: its layout in order, each field once with its value,
/// then each field the layout does not name, as a merge can leave one.
fn card(mut c: cards::Card) -> Card {
    let body = std::mem::replace(&mut c.body, Content::empty());
    Card::from_parts(payload(c), body)
}

fn payload(c: cards::Card) -> Payload {
    let mut seen = std::collections::HashSet::new();
    let value = |key: &str| c.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut items = Vec::new();
    for entry in &c.layout {
        items.push(match entry {
            LayoutItem::Quill { value } => match value.parse() {
                Ok(reference) => PayloadItem::Quill { reference },
                Err(_) => continue,
            },
            LayoutItem::Kind { value } => PayloadItem::Kind { value: value.clone() },
            LayoutItem::Id { value } => PayloadItem::Id { value: value.clone() },
            LayoutItem::Ext => PayloadItem::Meta { key: MetaKey::Ext, value: c.ext.clone() },
            LayoutItem::Seed => PayloadItem::Meta { key: MetaKey::Seed, value: c.seed.clone() },
            LayoutItem::Field { key } => {
                let Some(v) = value(key) else { continue };
                if !seen.insert(key.clone()) {
                    continue;
                }
                PayloadItem::Field { key: key.clone(), value: QuillValue::from_json(v) }
            }
            LayoutItem::Comment { text, inline } => PayloadItem::Comment { text: text.clone(), inline: *inline },
        });
    }
    for (key, v) in &c.fields {
        if seen.insert(key.clone()) {
            items.push(PayloadItem::Field { key: key.clone(), value: QuillValue::from_json(v.clone()) });
        }
    }
    let nested = c
        .nested
        .into_iter()
        .map(|n| NestedComment {
            container_path: n
                .container_path
                .into_iter()
                .map(|s| match s {
                    PathStep::Key(k) => PathSegment::Key(k),
                    PathStep::Index(i) => PathSegment::Index(i),
                })
                .collect(),
            position: n.position,
            text: trim_comment_text(n.text),
            inline: n.inline,
        })
        .collect();
    Payload::from_items_with_nested(items, nested)
}

/// A new document holding `main` and each of `cards` that holds an id.
pub(crate) fn build(main: &Card, all: &[Card]) -> Doc {
    cards::build(&to_model(main), &all.iter().map(to_model).collect::<Vec<_>>())
}

/// The main card and the cards in document order the document reads as.
pub(crate) fn read(d: &Doc) -> Result<(Card, Vec<Card>), String> {
    let (main, all) = cards::read(d)?;
    Ok((card(main), all.into_iter().map(card).collect()))
}

/// Writes the parts `touched` names of `card`, the card held under `id`
/// (the main card where none), into the document.
pub(crate) fn write_parts(d: &mut Doc, id: Option<&str>, card: &Card, touched: &Touched) {
    let c = cards::card_obj(d, id).expect("a held card is in the document");
    let payload = card.payload();
    if touched.layout {
        cards::put_layout(d, &c, &layout(payload));
    }
    for key in &touched.fields {
        match payload.get(key) {
            Some(v) => cards::put_field(d, &c, key, v.as_json()),
            None => cards::remove_field(d, &c, key),
        }
    }
    if touched.meta {
        cards::put_ext(d, &c, &payload.ext().cloned().unwrap_or_default());
        cards::put_seed(d, &c, &payload.seed().cloned().unwrap_or_default());
    }
    if touched.nested {
        cards::put_nested(d, &c, &nested(payload));
    }
    if touched.body {
        cards::put_body(d, &c, card.body());
    }
}

/// The parts of `now` that differ from `then`, value by value.
fn differing(then: &Card, now: &Card) -> Touched {
    let (a, b) = (then.payload(), now.payload());
    let mut t = Touched {
        layout: a.items().len() != b.items().len() || a.items().iter().zip(b.items()).any(|(x, y)| layout_item(x) != layout_item(y)),
        meta: (a.ext(), a.seed()) != (b.ext(), b.seed()),
        nested: a.nested_comments() != b.nested_comments(),
        body: then.body() != now.body(),
        ..Touched::default()
    };
    for (key, v) in b.iter() {
        if a.get(key) != Some(v) {
            t.fields.insert(key.clone());
        }
    }
    t.fields.extend(a.keys().filter(|k| b.get(k).is_none()).cloned());
    t
}

/// Writes `now` over `then`, what the document reads as, value by value:
/// each card's parts that differ, each card `now` adds put, each it drops
/// taken out, and the order.
pub(crate) fn write_over(d: &mut Doc, then: (&Card, &[Card]), now: (&Card, &[Card])) {
    write_parts(d, None, now.0, &differing(then.0, now.0));
    let held: std::collections::HashMap<&str, &Card> = then.1.iter().filter_map(|c| Some((c.id()?, c))).collect();
    for card in now.1 {
        let Some(id) = card.id() else { continue };
        match held.get(id) {
            Some(was) if cards::card_obj(d, Some(id)).is_some() => write_parts(d, Some(id), card, &differing(was, card)),
            _ => {
                cards::put(d, id, &to_model(card));
            }
        }
    }
    cards::put_order(d, &now.1.iter().filter_map(Card::id).collect::<Vec<_>>());
}

/// Puts the card held under `id` under `card`'s `$id`, whole.
pub(crate) fn rename_card(d: &mut Doc, id: &str, card: &Card) {
    cards::rename(d, id, &to_model(card));
}

/// Places `c` at `index` of the document's cards, and answers it as it reads.
pub(crate) fn insert_card(d: &mut Doc, index: usize, c: &Card) -> Card {
    card(cards::insert(d, index, &to_model(c)))
}

pub(crate) fn remove_card(d: &mut Doc, id: &str) {
    cards::remove(d, id);
}

pub(crate) fn move_card(d: &mut Doc, id: &str, index: usize) {
    cards::move_to(d, id, index);
}

/// The card at `at` as the document reads it, the main card where `None`.
pub(crate) fn read_at(d: &Doc, id: Option<&str>) -> Card {
    card(match id {
        None => cards::read_card(d, &d.main()),
        Some(id) => cards::read_id(d, id),
    }
    .expect("a held card reads"))
}

/// Takes the changes since `before` into `main` and `all`: each part a
/// patch reached read again, or, where `follow` and `main` and `all` read
/// as the document did at `before`, followed over what the card held.
/// Answers the patches, spelled in `u`.
pub(crate) fn refresh(d: &mut Doc, before: &super::Heads, main: &mut Card, all: &mut Vec<Card>, follow: bool, u: impl Units) -> Vec<serde_json::Value> {
    let r = {
        let (main, all) = (&*main, &*all);
        let held = |id: Option<&str>, field: Option<&str>| -> Option<Content> {
            if !follow {
                return None;
            }
            let card = match id {
                None => main,
                Some(id) => all.iter().find(|c| c.id() == Some(id))?,
            };
            match field {
                None => Some(card.body().clone()),
                Some(key) => card.payload().get(key).filter(|v| quillmark_automerge::content::units::is_content_value(v.as_json())).and_then(|v| Content::from_value(v.as_json()).ok()),
            }
        };
        cards::refresh(d, before, held, u)
    };
    if r.whole {
        (*main, *all) = read(d).expect("a held document reads as cards");
        return r.patches;
    }
    let mut gone = std::collections::HashSet::new();
    for (id, reread) in r.cards {
        let at = match &id {
            None => Some(&mut *main),
            Some(id) => all.iter_mut().find(|c| c.id() == Some(id.as_str())),
        };
        match (at, reread.card) {
            (_, Some(None)) => {
                gone.extend(id);
            }
            (Some(c), Some(Some(read))) => *c = card(read),
            (Some(c), None) => apply(c, reread.payload, reread.fields, reread.body),
            (None, _) => {}
        }
    }
    if let Some(order) = r.order {
        let mut held: std::collections::HashMap<String, Card> = all.drain(..).filter_map(|c| Some((c.id()?.to_string(), c))).collect();
        *all = order
            .into_iter()
            .map(|id| match held.remove(&id).filter(|_| !gone.contains(&id)) {
                Some(c) => c,
                None => read_at(d, Some(&id)),
            })
            .collect();
    }
    r.patches
}

/// Takes what a stretch of changes left of a card's parts into `c`: the
/// rest of its payload where `payload_now` reads it, and each field in
/// `fields`.
fn apply(c: &mut Card, payload_now: Option<cards::Card>, fields: Vec<(String, Option<serde_json::Value>)>, body: Option<Content>) {
    match payload_now {
        Some(mut p) => {
            p.fields = c.payload().iter().map(|(k, v)| (k.clone(), v.as_json().clone())).collect();
            for (key, v) in fields {
                let at = p.fields.iter().position(|(k, _)| *k == key);
                match (at, v) {
                    (Some(at), Some(v)) => p.fields[at].1 = v,
                    (Some(at), None) => {
                        p.fields.remove(at);
                    }
                    (None, Some(v)) => p.fields.push((key, v)),
                    (None, None) => {}
                }
            }
            *c.payload_mut() = payload(p);
        }
        None => {
            for (key, v) in fields {
                c.payload_mut().refresh_field(&key, v.map(QuillValue::from_json));
            }
        }
    }
    if let Some(body) = body {
        c.set_body(body);
    }
    c.take_touched();
}
