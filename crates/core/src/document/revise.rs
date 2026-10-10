//! The whole-document revise: a markdown write that keeps what markdown cannot
//! spell.

use std::collections::HashSet;

use quillmark_content::delta::rebase_marks;
use quillmark_content::export::anchors_where;
use quillmark_content::import::from_markdown;
use quillmark_content::model::{MarkKind, Normalized};
use quillmark_content::serial::{from_canonical_value, to_canonical_value};
use serde_json::Value as JsonValue;

use super::align::{align, Pairing, Slot};
use super::emit::{canonical_content, emit_payload_fields, DocumentAnchor};
use super::{dropped_constructs, Card, Document, Parsed};
use crate::error::{Diagnostic, ParseError};
use crate::path::DocPath;
use crate::value::QuillValue;

/// The receipt of [`Document::revise`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[must_use = "names the anchors the write dropped; read `.dropped_anchors` or bind it"]
pub struct DocumentRevised {
    /// Every anchor a stored card's body or content value holds that the card
    /// the revise aligned it to no longer holds at the same address, and every
    /// anchor of a card the revise removed, each with the line it stood on.
    pub dropped_anchors: Vec<DocumentAnchor>,
    /// The parse warnings, then one `parse::dropped_construct` per construct
    /// dropped from a field revising a stored content value, each at its
    /// address in the revised document.
    pub warnings: Vec<Diagnostic>,
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

        let pairs = align_cards(&self.cards, &cards);
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
        DocumentRevised {
            dropped_anchors,
            warnings,
        }
    }
}

/// For each of `incoming`'s composable cards, the card of `stored` it revises
/// and how they paired, or `None` for an inserted card.
pub(super) fn align_cards(stored: &[Card], incoming: &[Card]) -> Vec<Option<(usize, Pairing)>> {
    let stored_texts: Vec<String> = stored.iter().map(card_text).collect();
    let incoming_texts: Vec<String> = incoming.iter().map(card_text).collect();
    align(&slots(stored, &stored_texts), &slots(incoming, &incoming_texts))
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
    let mut out = String::new();
    emit_payload_fields(&mut out, card.payload());
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
    *incoming.body_mut() = rebase_marks(stored.body(), body);

    for (name, value) in incoming.payload_mut().fields_mut() {
        if let Some(stored) = stored.payload().get(name) {
            revise_value(stored.as_json(), value.as_json_mut(), &at.field(name), warnings);
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
/// content it imports with `stored`'s anchors rebased.
fn revise_value(
    stored: &JsonValue,
    incoming: &mut JsonValue,
    at: &DocPath,
    warnings: &mut Vec<Diagnostic>,
) {
    if let Some(base) = canonical_content(stored) {
        let JsonValue::String(text) = incoming else {
            return;
        };
        let Ok(imported) = from_markdown(text) else {
            return;
        };
        warnings.extend(dropped_constructs(imported.warnings, Some(at)));
        *incoming = to_canonical_value(&rebase_marks(&base, imported.content));
        return;
    }
    match (stored, incoming) {
        (JsonValue::Object(stored), JsonValue::Object(incoming)) => {
            for (key, value) in incoming.iter_mut() {
                if let Some(stored) = stored.get(key) {
                    revise_value(stored, value, &at.field(key), warnings);
                }
            }
        }
        (JsonValue::Array(stored), JsonValue::Array(incoming)) => {
            for (i, (stored, value)) in stored.iter().zip(incoming.iter_mut()).enumerate() {
                revise_value(stored, value, &at.index(i), warnings);
            }
        }
        _ => {}
    }
}

/// Every anchor `stored` holds at `at` that `revised` does not hold there, in
/// the order [`Document::to_markdown_annotated`] lists anchors; all of them
/// when the card was removed.
fn drop_report(
    stored: &Card,
    revised: Option<&Card>,
    at: &DocPath,
    out: &mut Vec<DocumentAnchor>,
) {
    for (name, value) in stored.payload().iter() {
        let revised = revised.and_then(|card| card.payload().get(name)).map(QuillValue::as_json);
        drop_value(value.as_json(), revised, &at.field(name), out);
    }
    drop_content(stored.body(), revised.map(Card::body), &at.body(), out);
}

/// [`drop_report`] over a field's value: each content object in `stored`, at
/// any depth, against what `revised` holds at its place. A content object
/// stored out of canonical order counts too, though a revise lands over it as
/// authored rather than rebasing it.
fn drop_value(
    stored: &JsonValue,
    revised: Option<&JsonValue>,
    at: &DocPath,
    out: &mut Vec<DocumentAnchor>,
) {
    let content_of =
        |value: &JsonValue| value.is_object().then(|| from_canonical_value(value).ok()).flatten();
    if let Some(content) = content_of(stored) {
        let anchored = content.marks.iter().any(|m| matches!(m.kind, MarkKind::Anchor { .. }));
        if anchored {
            let revised = revised.and_then(content_of);
            drop_content(&content, revised.as_ref(), at, out);
        }
        return;
    }
    match stored {
        JsonValue::Object(map) => {
            for (key, value) in map {
                drop_value(value, revised.and_then(|r| r.get(key)), &at.field(key), out);
            }
        }
        JsonValue::Array(items) => {
            for (i, value) in items.iter().enumerate() {
                drop_value(value, revised.and_then(|r| r.get(i)), &at.index(i), out);
            }
        }
        _ => {}
    }
}

fn drop_content(
    stored: &Normalized,
    revised: Option<&Normalized>,
    at: &DocPath,
    out: &mut Vec<DocumentAnchor>,
) {
    let kept: HashSet<&str> = revised
        .into_iter()
        .flat_map(|content| &content.marks)
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    out.extend(anchors_where(stored, |id| !kept.contains(id)).into_iter().map(|a| DocumentAnchor {
        id: a.id,
        path: at.clone(),
        line: a.line,
    }));
}
