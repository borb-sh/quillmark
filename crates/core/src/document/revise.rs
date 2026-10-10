//! The whole-document revise: a markdown write that keeps what markdown cannot
//! spell.

use quillmark_content::delta::rebase_onto;
use quillmark_content::model::{Content, MarkKind, Normalized};
use quillmark_content::serial::{from_canonical_value, to_canonical_value};
use serde_json::Value as JsonValue;

use super::align::{align, Pairing, Slot};
use super::edit::revise_import;
use super::emit::{canonical_content, emit_payload_items};
use super::{Card, Document, Parsed, Payload, PayloadItem};
use crate::error::{Diagnostic, ParseError};
use crate::path::DocPath;
use crate::value::{PathSegment, QuillValue};

/// The receipt of [`Document::revise`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[must_use = "names the anchors the write dropped; read `.dropped_anchors` or bind it"]
pub struct DocumentRevised {
    /// Every prose anchor the stored document held that the revised one does
    /// not, at its address in the stored document. A table cell's anchors drop
    /// unnamed.
    pub dropped_anchors: Vec<DroppedAnchor>,
    /// For each composable card of the revised document, the index of the
    /// stored card it revised, or `None` for an inserted card.
    pub(crate) alignment: Vec<Option<usize>>,
    /// The parse warnings, then one `parse::dropped_construct` per construct
    /// dropped from a field revising a stored content value, each at its
    /// address in the revised document.
    pub warnings: Vec<Diagnostic>,
}

/// An anchor the revise did not carry.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedAnchor {
    pub path: DocPath,
    pub id: String,
}

impl Document {
    /// Replace this document with `markdown`, keeping what the markdown cannot
    /// spell where it can be matched.
    ///
    /// Cards carry no id, so the incoming composable cards align to the stored
    /// ones by `$kind` and text similarity. An aligned card's body, and each
    /// markdown string in a field where the stored field holds a content
    /// object, at any depth, by key in an object and by index in an array,
    /// revise as [`Card::revise_body`] and [`Card::revise_field`] do, so
    /// surviving anchors rebase. Everything else
    /// lands as the markdown spells it: scalars, `$quill`, `$seed`, YAML
    /// comments, inserted cards, and removed cards dropped.
    ///
    /// `$ext` lands as spelled. A card that omits it keeps the stored one when
    /// it is the main card, when it aligned by text, or when it aligned by
    /// position and the two documents' composable `$kind` sequences are
    /// identical, so a card that merely sits where another was never takes
    /// that card's durable key.
    ///
    /// Schema-free: nothing conforms. A string that meets no stored content
    /// object rests as authored, and an over-nested one lands as given with
    /// its anchors reported dropped. The bound door is
    /// [`TypedWriter::revise_document`](crate::writer::TypedWriter::revise_document).
    ///
    /// Errors as [`Document::parse`] does, and then leaves `self` unchanged.
    pub fn revise(&mut self, markdown: &str) -> Result<DocumentRevised, ParseError> {
        let parsed = Document::parse(markdown)?;
        Ok(self.revise_parsed(parsed))
    }

    pub(crate) fn revise_parsed(&mut self, parsed: Parsed) -> DocumentRevised {
        let Parsed {
            document: incoming,
            mut warnings,
        } = parsed;
        let Document { main, cards } = incoming;

        let stored_texts: Vec<String> = self.cards.iter().map(card_text).collect();
        let incoming_texts: Vec<String> = cards.iter().map(card_text).collect();
        let pairs = align(
            &slots(&self.cards, &stored_texts),
            &slots(&cards, &incoming_texts),
        );
        let same_kinds = self.cards.iter().map(Card::kind).eq(cards.iter().map(Card::kind));

        let mut dropped_anchors = Vec::new();

        let main = revise_card(
            &self.main,
            main,
            &DocPath::main(),
            true,
            &mut warnings,
        );
        drop_report(&self.main, Some(&main), &DocPath::main(), &mut dropped_anchors);

        let mut revised_cards = Vec::with_capacity(cards.len());
        let mut kept = vec![None; self.cards.len()];
        for (j, card) in cards.into_iter().enumerate() {
            let at = DocPath::card(card.kind(), j);
            let card = match pairs[j] {
                Some((i, pairing)) => {
                    kept[i] = Some(j);
                    let carry_ext = pairing == Pairing::Text || same_kinds;
                    revise_card(&self.cards[i], card, &at, carry_ext, &mut warnings)
                }
                None => card,
            };
            revised_cards.push(card);
        }
        for (i, stored) in self.cards.iter().enumerate() {
            let at = DocPath::card(stored.kind(), i);
            let revised = kept[i].map(|j| &revised_cards[j]);
            drop_report(stored, revised, &at, &mut dropped_anchors);
        }

        *self = Document::from_main_and_cards(main, revised_cards);
        let alignment = pairs.into_iter().map(|pair| pair.map(|(i, _)| i)).collect();
        DocumentRevised {
            dropped_anchors,
            alignment,
            warnings,
        }
    }
}

fn slots<'a>(cards: &'a [Card], texts: &'a [String]) -> Vec<Slot<'a>> {
    cards
        .iter()
        .zip(texts)
        .map(|(card, text)| Slot {
            kind: card.kind().unwrap_or_default(),
            text,
        })
        .collect()
}

/// The text alignment measures a card by: its fields and body as emitted. Its
/// `$` keys stay out, so a writer that omits `$ext` still spells the card the
/// stored one did.
fn card_text(card: &Card) -> String {
    let fields = Payload::from_items(
        card.payload()
            .items()
            .iter()
            .filter(|item| matches!(item, PayloadItem::Field { .. }))
            .cloned()
            .collect(),
    );
    let mut out = String::new();
    emit_payload_items(&mut out, &fields);
    out.push_str(&card.body_markdown());
    out
}

/// `incoming` with `stored`'s anchors rebased onto its body and the content
/// in its fields, and `stored`'s `$ext` when `incoming` omits it and
/// `carry_ext`.
fn revise_card(
    stored: &Card,
    mut incoming: Card,
    at: &DocPath,
    carry_ext: bool,
    warnings: &mut Vec<Diagnostic>,
) -> Card {
    let body = std::mem::replace(incoming.body_mut(), Normalized::empty());
    let (body, _) = rebase_onto(stored.body(), body);
    *incoming.body_mut() = body;

    let fields: Vec<(String, JsonValue)> = incoming
        .payload()
        .items()
        .iter()
        .filter_map(|item| match item {
            PayloadItem::Field { key, value } => Some((key.clone(), value.as_json().clone())),
            _ => None,
        })
        .collect();
    for (name, mut value) in fields {
        let Some(stored) = stored.payload().get(&name) else {
            continue;
        };
        if revise_value(stored.as_json(), &mut value, &at.field(&name), warnings) {
            incoming
                .payload_mut()
                .swap_value(&name, QuillValue::from_json(value));
        }
    }

    if carry_ext && incoming.ext().is_none() {
        if let Some(ext) = stored.ext() {
            incoming.payload_mut().set_ext(ext.clone());
        }
    }
    incoming
}

/// Revise each markdown string in `incoming` that sits where `stored` holds a
/// content object, by key in an object and by index in an array, into the
/// content it imports with `stored`'s anchors rebased. Whether any did.
fn revise_value(
    stored: &JsonValue,
    incoming: &mut JsonValue,
    at: &DocPath,
    warnings: &mut Vec<Diagnostic>,
) -> bool {
    if let Some(base) = canonical_content(stored) {
        let JsonValue::String(text) = incoming else {
            return false;
        };
        let Ok((content, revised)) = revise_import(&base, text.as_str()) else {
            return false;
        };
        warnings.extend(revised.with_path(at).warnings);
        *incoming = to_canonical_value(&content);
        return true;
    }
    let mut any = false;
    match (stored, incoming) {
        (JsonValue::Object(stored), JsonValue::Object(incoming)) => {
            for (key, value) in incoming.iter_mut() {
                if let Some(stored) = stored.get(key) {
                    any |= revise_value(stored, value, &at.field(key), warnings);
                }
            }
        }
        (JsonValue::Array(stored), JsonValue::Array(incoming)) => {
            for (i, (stored, value)) in stored.iter().zip(incoming.iter_mut()).enumerate() {
                any |= revise_value(stored, value, &at.index(i), warnings);
            }
        }
        _ => {}
    }
    any
}

/// Record every anchor of `stored` that `revised` does not hold in the same
/// body or top-level field, at the address the stored card held it; all of
/// them when the card was removed.
fn drop_report(
    stored: &Card,
    revised: Option<&Card>,
    at: &DocPath,
    out: &mut Vec<DroppedAnchor>,
) {
    let top = |f: &Option<Vec<PathSegment>>| f.as_ref().and_then(|segs| segs.first().cloned());
    let survivors = revised.map(card_anchors).unwrap_or_default();
    for (field, id) in card_anchors(stored) {
        if !survivors.iter().any(|(f, i)| top(f) == top(&field) && i == &id) {
            let path = match &field {
                Some(segs) => segs.iter().fold(at.clone(), |p, seg| p.segment(seg)),
                None => at.body(),
            };
            out.push(DroppedAnchor { path, id });
        }
    }
}

/// `(field, id)` for each anchor in the card: `None` for the body, and for a
/// field the value path of the content object holding it, as the annotated
/// read lists it.
fn card_anchors(card: &Card) -> Vec<(Option<Vec<PathSegment>>, String)> {
    let mut out: Vec<(Option<Vec<PathSegment>>, String)> = content_anchors(card.body())
        .map(|id| (None, id))
        .collect();
    for (name, value) in card.payload().iter() {
        let mut found = Vec::new();
        value_anchors(value.as_json(), &mut vec![PathSegment::Key(name.clone())], &mut found);
        out.extend(found.into_iter().map(|(path, id)| (Some(path), id)));
    }
    out
}

fn content_anchors(content: &Content) -> impl Iterator<Item = String> + '_ {
    content.marks.iter().filter_map(|m| match &m.kind {
        MarkKind::Anchor { id } => Some(id.clone()),
        _ => None,
    })
}

fn value_anchors(
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    out: &mut Vec<(Vec<PathSegment>, String)>,
) {
    let children: Vec<(PathSegment, &JsonValue)> = match value {
        JsonValue::Object(map) => match from_canonical_value(value) {
            Ok(content) => {
                out.extend(content_anchors(&content).map(|id| (path.clone(), id)));
                return;
            }
            Err(_) => map.iter().map(|(k, v)| (PathSegment::Key(k.clone()), v)).collect(),
        },
        JsonValue::Array(items) => items.iter().enumerate().map(|(i, v)| (PathSegment::Index(i), v)).collect(),
        _ => return,
    };
    for (seg, child) in children {
        path.push(seg);
        value_anchors(child, path, out);
        path.pop();
    }
}
