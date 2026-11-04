//! A document's cards held in its Automerge document: each [`Card`] as the
//! plain data the model crate holds it in ([`quillmark_automerge::model::cards`]),
//! and back.

use quillmark_automerge::model::Doc;
use quillmark_automerge::model::cards::{self, LayoutItem, NestedComment as HeldComment, PathStep};

use super::payload::{MetaKey, PayloadItem, trim_comment_text};
use super::prescan::NestedComment;
use super::{Card, Payload};
use crate::value::{PathSegment, QuillValue};

fn held(card: &Card) -> cards::Card {
    let payload = card.payload();
    let mut layout = Vec::new();
    let mut fields = Vec::new();
    for item in payload.items() {
        layout.push(match item {
            PayloadItem::Quill { reference } => LayoutItem::Quill { value: reference.to_string() },
            PayloadItem::Kind { value } => LayoutItem::Kind { value: value.clone() },
            PayloadItem::Id { value } => LayoutItem::Id { value: value.clone() },
            PayloadItem::Meta { key: MetaKey::Ext, .. } => LayoutItem::Ext,
            PayloadItem::Meta { key: MetaKey::Seed, .. } => LayoutItem::Seed,
            PayloadItem::Field { key, value } => {
                fields.push((key.clone(), value.as_json().clone()));
                LayoutItem::Field { key: key.clone() }
            }
            PayloadItem::Comment { text, inline } => LayoutItem::Comment { text: text.clone(), inline: *inline },
        });
    }
    let nested = payload
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
        .collect();
    cards::Card {
        layout,
        fields,
        ext: payload.ext().cloned().unwrap_or_default(),
        seed: payload.seed().cloned().unwrap_or_default(),
        nested,
        body: card.body().clone(),
    }
}

/// The card `c` holds: its layout in order, each field once with its value,
/// then each field the layout does not name, as a merge can leave one.
fn card(c: cards::Card) -> Card {
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
    Card::from_parts(Payload::from_items_with_nested(items, nested), c.body)
}

/// A new document holding `main` and each of `cards` that holds an id.
pub(crate) fn build(main: &Card, all: &[Card]) -> Doc {
    cards::build(&held(main), &all.iter().map(held).collect::<Vec<_>>())
}

/// The main card and the cards in document order the document reads as.
pub(crate) fn read(d: &Doc) -> Result<(Card, Vec<Card>), String> {
    let (main, all) = cards::read(d)?;
    Ok((card(main), all.into_iter().map(card).collect()))
}

/// Writes what differs between `was` and `now` into the document.
pub(crate) fn flush(d: &mut Doc, was: (&Card, &[Card]), now: (&Card, &[Card])) {
    let was_all: Vec<_> = was.1.iter().map(held).collect();
    let now_all: Vec<_> = now.1.iter().map(held).collect();
    cards::flush(d, (&held(was.0), &was_all), (&held(now.0), &now_all));
}

/// Writes what the card changed since it read as `was` into the document,
/// the main card where `main`, and answers it as it reads after.
pub(crate) fn write_card(d: &mut Doc, main: bool, was: &Card, now: &Card) -> Card {
    card(cards::write_one(d, main, &held(was), &held(now)))
}

/// Places `c` at `index` of the document's cards, and answers it as it reads.
pub(crate) fn insert_card(d: &mut Doc, index: usize, c: &Card) -> Card {
    card(cards::insert(d, index, &held(c)))
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

/// Reads again the cards `patches` reached into `main` and `all`.
pub(crate) fn reread(d: &Doc, patches: &[serde_json::Value], main: &mut Card, all: &mut Vec<Card>) {
    let t = cards::touched(patches);
    if t.whole {
        (*main, *all) = read(d).expect("a held document reads as cards");
        return;
    }
    if t.main {
        *main = read_at(d, None);
    }
    let read_id = |id: &str| read_at(d, Some(id));
    if t.moved {
        let mut held: std::collections::HashMap<String, Card> = all.drain(..).filter_map(|c| Some((c.id()?.to_string(), c))).collect();
        *all = d
            .order()
            .into_iter()
            .map(|id| match held.remove(&id).filter(|_| !t.cards.contains(&id)) {
                Some(c) => c,
                None => read_id(&id),
            })
            .collect();
    } else {
        for c in all.iter_mut() {
            if let Some(id) = c.id().filter(|id| t.cards.contains(*id)).map(str::to_string) {
                *c = read_id(&id);
            }
        }
    }
}
