//! Document seeding from a quill schema: one card per kind, carrying what the
//! kind's `seed:` names and nothing else, so the render layer supplies
//! `default`/blank. A card added to a document commits the document's `$seed`
//! overlay for its kind in place of the kind's own `seed:`.

use quillmark_content::model::Normalized;

use super::Quill;
use crate::quill::CardSchema;
use crate::document::PayloadItem;
use crate::{
    document::{Card, Document, Payload, SeedOverlay},
    value::QuillValue,
    version::{QuillReference, VersionSelector},
};

/// Build the seeded `(payload, body)` for one card schema under an optional
/// document [`SeedOverlay`], which replaces the kind's `seed:` whole. A field
/// commits that seed's value in declaration order, a present-null reading as
/// absent; the body commits its `$body`, honored only when the kind enables
/// bodies. A key naming no schema field is never reached.
/// The `$quill` / `$kind` system metadata is attached by the caller.
///
/// Every seeded content field commits through [`seeded_rest`], the same strict
/// write the typed writer uses, so a seeded card is at rest from birth
/// (`SCHEMAS.md` § "Document seeding": seed-commits-rest).
fn seed_parts(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> (Payload, Normalized) {
    let seed = overlay.or(schema.seed.as_ref());
    let items: Vec<PayloadItem> = schema
        .fields
        .iter()
        .filter_map(|(name, field)| {
            let value = seed?.fields.get(name).filter(|v| !v.as_json().is_null())?;
            Some(PayloadItem::Field {
                key: name.clone(),
                value: seeded_rest(name, value, field),
            })
        })
        .collect();

    let body = match seed.and_then(|seed| seed.body.as_ref()) {
        Some(seeded_body) if schema.body_enabled() => {
            crate::document::import_body(seeded_body).unwrap_or_else(|_| Normalized::empty())
        }
        _ => Normalized::empty(),
    };

    debug_assert!(
        items.len() <= crate::error::MAX_FIELD_COUNT,
        "a loaded quill's card schema declares at most MAX_FIELD_COUNT fields"
    );
    (Payload::from_items(items), body)
}

/// The form a seeded value commits at: the strict write's, for a field whose
/// type tree bears a content leaf; verbatim for every other field (a scalar's
/// authored shorthand is the typed write's to canonicalize, and conform leaves
/// it alone). A value the strict write refuses stays authored, exactly as
/// conform leaves it.
fn seeded_rest(name: &str, value: &QuillValue, field: &crate::quill::FieldSchema) -> QuillValue {
    if !crate::quill::config::field_contains_content(field) {
        return value.clone();
    }
    crate::document::edit::resolve_field_write(name, value.clone(), field)
        .unwrap_or_else(|_| value.clone())
}

/// `$quill` reference for the main card, as `name@version`. Falls back to a
/// versionless reference if the configured version is unparseable (it is
/// validated at quill load, so the fallback is defensive only).
pub(crate) fn main_reference(quill: &Quill) -> QuillReference {
    let config = quill.config();
    format!("{}@{}", config.name, config.version)
        .parse()
        .unwrap_or_else(|_| QuillReference::new(config.name.clone(), VersionSelector::Any))
}

pub(crate) fn seed_main(quill: &Quill) -> Card {
    // The main card is never seeded: `$seed` keys range over composable
    // `card_kinds`, `main` is not one of them, and it declares no `seed:`.
    let (mut payload, body) = seed_parts(&quill.config().main, None);
    payload.set_quill(main_reference(quill));
    // The root block carries `$kind: main` alongside `$quill` (see the
    // markdown spec); set it so a seeded main card round-trips through
    // `to_markdown()` exactly as the parser and blueprint emit it.
    payload.set_kind("main");
    Card::from_parts(payload, body)
}

pub(crate) fn seed_card_for_kind(
    quill: &Quill,
    card_kind: &str,
    overlay: Option<&SeedOverlay>,
) -> Option<Card> {
    let schema = quill.config().card_kind(card_kind)?;
    Some(seed_composable(schema, overlay))
}

/// Seed a single composable card from its schema and an optional overlay (sets
/// `$kind`, never `$quill`).
fn seed_composable(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> Card {
    let (mut payload, body) = seed_parts(schema, overlay);
    payload.set_kind(schema.name.clone());
    Card::from_parts(payload, body)
}

/// The empty document: [`Document::new`] under the quill's own reference, so no
/// caller spells `name@version` to reach the document the authoring contract
/// binds.
pub(crate) fn empty_document(quill: &Quill) -> Document {
    Document::new(main_reference(quill))
}

pub(crate) fn seed_document(quill: &Quill) -> Document {
    // A fresh document carries no `$seed`, so every kind seeds from its own.
    let main = seed_main(quill);
    let cards = quill
        .config()
        .card_kinds
        .iter()
        .map(|schema| seed_composable(schema, None))
        .collect();
    Document::from_main_and_cards(main, cards)
}

#[cfg(test)]
mod tests;
