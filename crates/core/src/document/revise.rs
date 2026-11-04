//! The whole-document revise: a markdown write that keeps what markdown cannot
//! spell.

use quillmark_automerge::content::lines::delta::rebase_marks;
use quillmark_automerge::content::change::ApplyError;
use quillmark_automerge::content::model::Content;
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

use super::align::{Pairing, Slot, align};
use super::card_id::{is_card_id, reminted_warning, settle_reporting};
use super::anchors::AnchorSelector;
use super::edit::EditError;
use super::emit::{canonical_content, emit_payload_fields};
use quillmark_automerge::content::anchors::anchor_contexts;
use quillmark_automerge::content::model::Content as ContentModel;
use super::{Card, Document, Heads, Parsed, Revising, dropped_constructs};
use crate::error::{Diagnostic, ParseError, Severity};
use crate::path::DocPath;
use crate::value::QuillValue;

/// The receipt of [`Document::revise`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[must_use = "names the anchors the write dropped; read `.dropped_anchors` or bind it"]
pub struct DocumentRevised {
    /// Every anchor a stored card's body or content value holds that the card
    /// the revise aligned it to no longer holds at the same address, and every
    /// anchor of a card the revise removed, each as the selector
    /// [`Document::reattach`] takes, read off the stored value.
    pub dropped_anchors: Vec<AnchorSelector>,
    /// The parse warnings, then one `parse::dropped_construct` per construct
    /// dropped from a field revising a stored content value, each at its
    /// address in the revised document, and an `edit::content_apply` warning
    /// at each body or content value the revise leaves as stored because what
    /// the markdown spells for it does not validate.
    pub warnings: Vec<Diagnostic>,
    /// For each composable card of the revised document, the index of the
    /// stored card it revises, or `None` for an inserted card. A stored card no
    /// entry names was removed. Only cards of one `$kind` pair, and a card
    /// revising a stored one holds its `$id`.
    pub from: Vec<Option<usize>>,
    /// The document's heads before the revise.
    pub before: Heads,
    /// The document's heads after the revise.
    pub heads: Heads,
}

impl Document {
    /// Replace this document with `markdown`, keeping what the markdown cannot
    /// spell where it can be matched.
    ///
    /// An incoming composable card spelling a stored card's `$id` revises it,
    /// when both are of one `$kind`; of several spelling one id, the one whose
    /// text most resembles the stored card's. The rest align to the stored
    /// cards left by `$kind` and text similarity. A card revising a stored one
    /// holds its id whatever it spells. An inserted card keeps a valid `$id`
    /// no stored card holds, and mints one otherwise, with a
    /// `parse::card_id_reminted` warning where it spelled one. An aligned
    /// card's body, and each
    /// markdown string in a field where the stored field holds a content
    /// object, at any depth, by key in an object and by index in an array,
    /// revise as [`CardMut::revise`](crate::document::CardMut::revise) does, so
    /// surviving anchors ride their text. A body or value whose revise does not
    /// validate keeps its stored value, with a warning. Everything else
    /// lands as the markdown spells it: scalars, `$quill`, `$seed`, YAML
    /// comments, inserted cards, and removed cards dropped.
    ///
    /// `$ext` lands as spelled. A card that omits it keeps the stored one when
    /// it is the main card, when it aligned by id or text, or when it aligned by
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
        let revising = Document::parse_revising(markdown)?;
        Ok(self.revise_parsed(revising))
    }

    /// [`revise`](Self::revise) from `revising`, whose cards hold the `$id`
    /// each spells or none, as [`Document::parse_revising`] leaves them.
    pub(crate) fn revise_parsed(&mut self, revising: Revising) -> DocumentRevised {
        let Revising {
            parsed:
                Parsed {
                    document: incoming,
                    mut warnings,
                },
            bodies,
        } = revising;
        let before = self.heads();
        let Document { main, cards, .. } = incoming;
        let mut bodies = bodies.into_iter();

        let pairs = align_cards(&self.cards, &cards);
        let same_kinds = self
            .cards
            .iter()
            .map(Card::kind)
            .eq(cards.iter().map(Card::kind));

        let mut dropped_anchors = Vec::new();
        let mut moves = Moves::new();

        let main = revise_card(
            &self.main,
            main,
            bodies.next().as_deref(),
            (&DocPath::main(), &DocPath::main()),
            true,
            &mut warnings,
            &mut moves,
        );
        drop_report(
            &self.main,
            Some(&main),
            &DocPath::main(),
            &moves,
            &mut dropped_anchors,
        );

        let stored_ids: HashSet<String> = self
            .cards
            .iter()
            .filter_map(Card::id)
            .map(str::to_string)
            .collect();
        let mut revised_cards = Vec::with_capacity(cards.len());
        let mut kept = vec![None; self.cards.len()];
        for (j, mut card) in cards.into_iter().enumerate() {
            let at = DocPath::card(card.kind(), j);
            let body = bodies.next();
            let card = match pairs[j] {
                Some((i, pairing)) => {
                    kept[i] = Some(j);
                    let stored = &self.cards[i];
                    match stored.id() {
                        Some(id) => card.payload_mut().set_id(id),
                        None => card.payload_mut().remove_id(),
                    }
                    let carry_ext = matches!(pairing, Pairing::Id | Pairing::Text) || same_kinds;
                    let from = DocPath::card(stored.kind(), i);
                    let places = (&at, &from);
                    revise_card(
                        stored,
                        card,
                        body.as_deref(),
                        places,
                        carry_ext,
                        &mut warnings,
                        &mut moves,
                    )
                }
                None => {
                    if let Some(body) = body {
                        let _ = revise_body(&Content::empty(), &body, &at.body(), &mut warnings);
                    }
                    if let Some(spelled) = card.id().map(str::to_string) {
                        if !is_card_id(&spelled) || stored_ids.contains(&spelled) {
                            warnings.push(reminted_warning(&spelled).with_path(at.to_string()));
                            card.payload_mut().remove_id();
                        }
                    }
                    card
                }
            };
            revised_cards.push(card);
        }
        settle_reporting(&mut revised_cards, &stored_ids, &mut warnings);
        for (i, stored) in self.cards.iter().enumerate() {
            let at = DocPath::card(stored.kind(), i);
            let revised = kept[i].map(|j| &revised_cards[j]);
            drop_report(stored, revised, &at, &moves, &mut dropped_anchors);
        }
        for anchor in &mut dropped_anchors {
            anchor.card_id = self.card_id_at(&anchor.path).map(str::to_string);
        }

        self.replace_cards(main, revised_cards);
        DocumentRevised {
            dropped_anchors,
            warnings,
            from: pairs.into_iter().map(|pair| pair.map(|(i, _)| i)).collect(),
            before,
            heads: self.heads(),
        }
    }
}

/// For each of `incoming`'s composable cards, the card of `stored` it revises
/// and how they paired, or `None` for an inserted card.
pub(crate) fn align_cards(stored: &[Card], incoming: &[Card]) -> Vec<Option<(usize, Pairing)>> {
    let stored_texts: Vec<String> = stored.iter().map(card_text).collect();
    let incoming_texts: Vec<String> = incoming.iter().map(card_text).collect();
    align(
        &slots(stored, &stored_texts),
        &slots(incoming, &incoming_texts),
    )
}

fn slots<'a>(cards: &'a [Card], texts: &'a [String]) -> Vec<Slot<'a>> {
    cards
        .iter()
        .zip(texts)
        .map(|(card, text)| Slot {
            kind: card.kind().unwrap_or_default(),
            id: card.id(),
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

/// For each array a revise aligned, by its address in the stored document,
/// the index each stored item landed at, or `None` for a removed one.
type Moves = HashMap<DocPath, Vec<Option<usize>>>;

/// `incoming` with `stored`'s anchors rebased onto its body and the content
/// in its fields, and `stored`'s `$ext` when `incoming` omits it and
/// `carry_ext`. The body revises from `source`, its markdown, when given. `at`
/// is the card's address in the revised document, `from` the stored card's.
fn revise_card(
    stored: &Card,
    mut incoming: Card,
    source: Option<&str>,
    (at, from): (&DocPath, &DocPath),
    carry_ext: bool,
    warnings: &mut Vec<Diagnostic>,
    moves: &mut Moves,
) -> Card {
    let revised = match source {
        Some(md) => revise_body(stored.body(), md, &at.body(), warnings),
        None => Ok(None),
    };
    let body = revised.and_then(|revised| match revised {
        Some(revised) => Ok(revised),
        None => {
            let lines = rebase_marks(stored.body_lines(), incoming.body_lines().clone());
            let lifted = quillmark_automerge::content::diff::lift(stored.body(), &lines);
            lifted.validate().map(|()| lifted).map_err(ApplyError::Invalid)
        }
    });
    match body {
        Ok(body) => incoming.set_body(body),
        Err(e) => {
            warnings.push(kept_warning(e, &at.body()));
            incoming.set_body(stored.body().clone());
        }
    }

    for (name, value) in incoming.payload_mut().fields_mut() {
        if let Some(stored) = stored.payload().get(name) {
            let places = (&at.field(name), &from.field(name));
            revise_value(
                stored.as_json(),
                value.as_json_mut(),
                places,
                warnings,
                moves,
            );
        }
    }

    if carry_ext && incoming.ext().is_none() {
        if let Some(ext) = stored.ext() {
            incoming.payload_mut().set_ext(ext.clone());
        }
    }
    incoming
}

/// `markdown` revising `stored` as [`CardMut::revise`](crate::document::CardMut::revise) revises a value with no
/// base. `None` on an over-nested input, and the refusal where the markdown
/// imports to a value that does not validate.
fn revised(stored: &Content, markdown: &str) -> Result<Option<quillmark_automerge::content::diff::Revision>, ApplyError> {
    use quillmark_automerge::content::diff::ReviseError;
    match quillmark_automerge::revise(stored, markdown) {
        Ok(revision) => Ok(Some(revision)),
        Err(ReviseError::Refused(e)) => Err(e),
        Err(ReviseError::Import(_)) => Ok(None),
    }
}

/// The warning for a value a revise leaves as stored because what the
/// markdown spells for it does not validate.
fn kept_warning(e: ApplyError, at: &DocPath) -> Diagnostic {
    let mut warning = EditError::change(e).to_diagnostic(Some(at.clone()));
    warning.severity = Severity::Warning;
    warning
}

/// `source`, a body the parse read with its tokens unresolved, revising
/// `stored`: [`revised`], and a `parse::dropped_construct` at `at` for each
/// tag the parse did not report.
fn revise_body(stored: &Content, source: &str, at: &DocPath, warnings: &mut Vec<Diagnostic>) -> Result<Option<Content>, ApplyError> {
    let Some(revision) = revised(stored, source)? else {
        return Ok(None);
    };
    let tags = revision.warnings.into_iter().filter(|w| w.construct.is_held_tag()).collect();
    warnings.extend(dropped_constructs(tags, Some(at)));
    Ok(Some(revision.content))
}

/// Revise each string in `incoming` that sits where `stored` holds a content
/// object, by key in an object and by [aligned](align_items) item in an array,
/// into the content it imports under that object's codec with `stored`'s
/// anchors rebased.
fn revise_value(
    stored: &JsonValue,
    incoming: &mut JsonValue,
    (at, from): (&DocPath, &DocPath),
    warnings: &mut Vec<Diagnostic>,
    moves: &mut Moves,
) {
    if canonical_content(stored).is_some() {
        let JsonValue::String(text) = incoming else {
            return;
        };
        let Some(base) = Content::from_value_lenient(stored).ok().map(|read| read.content) else {
            return;
        };
        let revision = match revised(&base, text) {
            Ok(Some(revision)) => revision,
            Ok(None) => return,
            Err(e) => {
                warnings.push(kept_warning(e, at));
                *incoming = stored.clone();
                return;
            }
        };
        warnings.extend(dropped_constructs(revision.warnings, Some(at)));
        *incoming = revision.content.to_value();
        return;
    }
    match (stored, incoming) {
        (JsonValue::Object(stored), JsonValue::Object(incoming)) => {
            for (key, value) in incoming.iter_mut() {
                if let Some(stored) = stored.get(key) {
                    revise_value(
                        stored,
                        value,
                        (&at.field(key), &from.field(key)),
                        warnings,
                        moves,
                    );
                }
            }
        }
        (JsonValue::Array(stored), JsonValue::Array(incoming)) => {
            let pairs = align_items(stored, incoming);
            let mut landed = vec![None; stored.len()];
            for (j, value) in incoming.iter_mut().enumerate() {
                if let Some(i) = pairs[j] {
                    landed[i] = Some(j);
                    revise_value(
                        &stored[i],
                        value,
                        (&at.index(j), &from.index(i)),
                        warnings,
                        moves,
                    );
                }
            }
            moves.insert(from.clone(), landed);
        }
        _ => {}
    }
}

/// For each item of `incoming`, the item of `stored` it revises, or `None` for
/// an inserted one: aligned by text as cards are, so an item inserted or
/// removed ahead of an edited one leaves it paired with its own stored item.
pub(crate) fn align_items(stored: &[JsonValue], incoming: &[JsonValue]) -> Vec<Option<usize>> {
    let stored_texts: Vec<String> = stored.iter().map(item_text).collect();
    let incoming_texts: Vec<String> = incoming.iter().map(item_text).collect();
    fn slots(texts: &[String]) -> Vec<Slot<'_>> {
        texts
            .iter()
            .map(|text| Slot {
                kind: "",
                id: None,
                text,
            })
            .collect()
    }
    align(&slots(&stored_texts), &slots(&incoming_texts))
        .into_iter()
        .map(|pair| pair.map(|(i, _)| i))
        .collect()
}

/// The text an item aligns by: a content object's projection, a string as
/// written, and every other value's leaves in order.
fn item_text(value: &JsonValue) -> String {
    fn walk(value: &JsonValue, out: &mut String) {
        if let Some(content) = canonical_content(value) {
            out.push_str(&super::Codec::of_object(value).project(&content));
            out.push('\n');
            return;
        }
        match value {
            JsonValue::String(text) => {
                out.push_str(text);
                out.push('\n');
            }
            JsonValue::Object(map) => {
                for (key, value) in map {
                    out.push_str(key);
                    out.push_str(": ");
                    walk(value, out);
                }
            }
            JsonValue::Array(items) => items.iter().for_each(|item| walk(item, out)),
            other => {
                out.push_str(&other.to_string());
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(value, &mut out);
    out
}

/// Every anchor `stored` holds at `at` that `revised` does not hold there, in
/// the order [`Document::to_markdown_annotated`] lists anchors; all of them
/// when the card was removed.
fn drop_report(
    stored: &Card,
    revised: Option<&Card>,
    at: &DocPath,
    moves: &Moves,
    out: &mut Vec<AnchorSelector>,
) {
    for (name, value) in stored.payload().iter() {
        let revised = revised
            .and_then(|card| card.payload().get(name))
            .map(QuillValue::as_json);
        drop_value(value.as_json(), revised, &at.field(name), moves, out);
    }
    drop_content(stored.body(), revised.map(Card::body), &at.body(), out);
}

/// [`drop_report`] over a field's value: each content object in `stored`, at
/// any depth, against what `revised` holds at its place, an array item's
/// place the one `moves` names for it. A content object
/// stored out of canonical order counts too, though a revise lands over it as
/// authored rather than merging with it.
fn drop_value(
    stored: &JsonValue,
    revised: Option<&JsonValue>,
    at: &DocPath,
    moves: &Moves,
    out: &mut Vec<AnchorSelector>,
) {
    let content_of = |value: &JsonValue| {
        quillmark_automerge::content::units::is_content_value(value)
            .then(|| ContentModel::from_value_lenient(value).ok().map(|read| read.content))
            .flatten()
    };
    if let Some(content) = content_of(stored) {
        if !anchor_contexts(&content).is_empty() {
            let revised = revised.and_then(content_of);
            drop_content(&content, revised.as_ref(), at, out);
        }
        return;
    }
    match stored {
        JsonValue::Object(map) => {
            for (key, value) in map {
                drop_value(
                    value,
                    revised.and_then(|r| r.get(key)),
                    &at.field(key),
                    moves,
                    out,
                );
            }
        }
        JsonValue::Array(items) => {
            let landed = moves.get(at);
            for (i, value) in items.iter().enumerate() {
                let place = landed.map_or(Some(i), |landed| landed[i]);
                let revised = place.and_then(|j| revised.and_then(|r| r.get(j)));
                drop_value(value, revised, &at.index(i), moves, out);
            }
        }
        _ => {}
    }
}

fn drop_content(
    stored: &ContentModel,
    revised: Option<&ContentModel>,
    at: &DocPath,
    out: &mut Vec<AnchorSelector>,
) {
    let held: HashSet<String> = revised.map_or_else(HashSet::new, |r| anchor_contexts(r).into_iter().map(|c| c.id).collect());
    out.extend(
        anchor_contexts(stored)
            .into_iter()
            .filter(|ctx| !held.contains(&ctx.id))
            .map(|ctx| AnchorSelector::new(ctx.id, at.clone(), ctx.quote, ctx.prefix, ctx.suffix)),
    );
}
