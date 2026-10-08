//! The whole-document revise: a markdown write that keeps what markdown cannot
//! spell.

use quillmark_content::delta::{diff_import, rebase_onto, Delta};
use quillmark_content::model::{Content, MarkKind, Normalized};
use quillmark_content::serial::{from_canonical_value, to_canonical_value};
use serde_json::Value as JsonValue;

use super::align::{align, Pairing, Slot};
use super::emit::{emit_payload_items, project_content_field};
use super::{dropped_construct, Card, Document, Parsed, Payload, PayloadItem};
use crate::error::{Diagnostic, ParseError};
use crate::path::DocPath;
use crate::value::QuillValue;

/// The receipt of [`Document::revise`].
#[derive(Debug, Clone, PartialEq)]
#[must_use = "names the anchors the write dropped; read `.dropped_anchors` or bind it"]
pub struct DocumentRevised {
    /// One per revised body or content field, at its address in the revised
    /// document: the text change an editor bridge maps positions through.
    pub deltas: Vec<FieldDelta>,
    /// Every anchor the stored document held that the revised one does not, at
    /// its address in the stored document.
    pub dropped_anchors: Vec<DroppedAnchor>,
    /// For each composable card of the revised document, the index of the
    /// stored card it revised, or `None` for an inserted card. A stored index
    /// no entry names was removed.
    pub alignment: Vec<Option<usize>>,
    /// The parse warnings, then one `parse::dropped_construct` per construct a
    /// content field's import dropped, each at its address in the revised
    /// document.
    pub warnings: Vec<Diagnostic>,
}

/// A revised body or content field's [`Delta`].
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDelta {
    pub path: DocPath,
    pub delta: Delta,
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
    /// ones by `$kind` and text similarity
    /// ([`alignment`](DocumentRevised::alignment)). An aligned card's body, and
    /// each field whose stored value is a content object and whose incoming
    /// value is a markdown string, revise as [`Card::revise_body`] and
    /// [`Card::revise_field`] do, so surviving anchors rebase. Everything else
    /// lands as the markdown spells it: scalars, `$quill`, `$seed`, YAML
    /// comments, inserted cards, and removed cards dropped.
    ///
    /// `$ext` lands as spelled. A card that omits it keeps the stored one when
    /// it is the main card, when it aligned by text, or when it aligned by
    /// position and the two documents' composable `$kind` sequences are
    /// identical, so a card that merely sits where another was never takes
    /// that card's durable key.
    ///
    /// Schema-free: nothing conforms. A field that is not a stored content
    /// object rests as authored, and an over-nested field string lands as given
    /// with its anchors reported dropped. The bound door is
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

        let mut deltas = Vec::new();
        let mut dropped_anchors = Vec::new();

        let main = revise_card(
            &self.main,
            main,
            &DocPath::main(),
            true,
            &mut deltas,
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
                    revise_card(&self.cards[i], card, &at, carry_ext, &mut deltas, &mut warnings)
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
            deltas,
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

/// `incoming` with `stored`'s anchors rebased onto its body and content
/// fields, and `stored`'s `$ext` when `incoming` omits it and `carry_ext`.
fn revise_card(
    stored: &Card,
    mut incoming: Card,
    at: &DocPath,
    carry_ext: bool,
    deltas: &mut Vec<FieldDelta>,
    warnings: &mut Vec<Diagnostic>,
) -> Card {
    let body = std::mem::replace(incoming.body_mut(), Normalized::empty());
    let (body, delta) = rebase_onto(stored.body(), body);
    *incoming.body_mut() = body;
    deltas.push(FieldDelta {
        path: at.body(),
        delta,
    });

    let revisable: Vec<(String, String)> = incoming
        .payload()
        .items()
        .iter()
        .filter_map(|item| match item {
            PayloadItem::Field { key, value } => {
                let text = value.as_json().as_str()?;
                let stored = stored.payload().get(key)?.as_json();
                project_content_field(stored)?;
                Some((key.clone(), text.to_string()))
            }
            _ => None,
        })
        .collect();
    for (name, text) in revisable {
        let base = from_canonical_value(
            stored
                .payload()
                .get(&name)
                .expect("filtered on presence above")
                .as_json(),
        )
        .expect("project_content_field decoded it above");
        let Ok((content, delta, dropped)) = diff_import(&base, &text) else {
            continue;
        };
        let path = at.field(&name);
        warnings.extend(
            dropped
                .into_iter()
                .map(|w| dropped_construct(w).with_path(path.to_string())),
        );
        incoming
            .payload_mut()
            .insert(name, QuillValue::from_json(to_canonical_value(&content)))
            .expect("a replace never grows the card");
        deltas.push(FieldDelta { path, delta });
    }

    if carry_ext && incoming.ext().is_none() {
        if let Some(ext) = stored.ext() {
            incoming.payload_mut().set_ext(ext.clone());
        }
    }
    incoming
}

/// Record every anchor of `stored` that `revised` does not hold at the same
/// body or field; all of them when the card was removed.
fn drop_report(
    stored: &Card,
    revised: Option<&Card>,
    at: &DocPath,
    out: &mut Vec<DroppedAnchor>,
) {
    let survivors = revised.map(card_anchors).unwrap_or_default();
    for (field, id) in card_anchors(stored) {
        if !survivors.iter().any(|(f, i)| f == &field && i == &id) {
            let path = match &field {
                Some(name) => at.field(name),
                None => at.body(),
            };
            out.push(DroppedAnchor { path, id });
        }
    }
}

/// `(field, id)` for each anchor in the card: `None` for the body, the
/// top-level field name for an anchor in any content object inside its value.
fn card_anchors(card: &Card) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = content_anchors(card.body())
        .map(|id| (None, id))
        .collect();
    for (name, value) in card.payload().iter() {
        let mut ids = Vec::new();
        value_anchors(value.as_json(), &mut ids);
        out.extend(ids.into_iter().map(|id| (Some(name.clone()), id)));
    }
    out
}

fn content_anchors(content: &Content) -> impl Iterator<Item = String> + '_ {
    content.marks.iter().filter_map(|m| match &m.kind {
        MarkKind::Anchor { id } => Some(id.clone()),
        _ => None,
    })
}

fn value_anchors(value: &JsonValue, out: &mut Vec<String>) {
    match value {
        JsonValue::Object(map) => match from_canonical_value(value) {
            Ok(content) => out.extend(content_anchors(&content)),
            Err(_) => map.values().for_each(|v| value_anchors(v, out)),
        },
        JsonValue::Array(items) => items.iter().for_each(|v| value_anchors(v, out)),
        _ => {}
    }
}
