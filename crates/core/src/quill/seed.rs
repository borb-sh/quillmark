//! Document seeding from a quill schema: one card per kind, carrying what the
//! kind's `seed:` names and nothing else, so the render layer supplies
//! `default`/blank. A card added to a document commits the document's `$seed`
//! overlay for its kind in place of the kind's own `seed:`.

use quillmark_content::model::Normalized;

use super::Quill;
use crate::quill::{CardSchema, QuillConfig};
use crate::document::PayloadItem;
use crate::{
    document::{Card, Document, Payload, SeedOverlay},
    value::QuillValue,
    version::{QuillReference, Version, VersionSelector},
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
    crate::document::edit::resolve_field_write(name, value, field, &crate::path::DocPath::new())
        .map_or_else(|_| value.clone(), |(rest, _)| rest)
}

/// `$quill` reference for a document the quill creates: `name@MAJOR.MINOR`,
/// the series every later patch reads. Versionless if the configured version
/// is unparseable, which a loaded quill's never is.
pub(crate) fn main_reference(config: &QuillConfig) -> QuillReference {
    let selector = config
        .version
        .parse::<Version>()
        .map_or(VersionSelector::Any, |v| VersionSelector::Minor(v.major, v.minor));
    QuillReference::new(config.name.clone(), selector)
}

pub(crate) fn seed_main(quill: &Quill) -> Card {
    // The main card is never seeded: `$seed` keys range over composable
    // `card_kinds`, `main` is not one of them, and it declares no `seed:`.
    main_with(quill, None)
}

/// The main card committing `overlay`, as a kind's card commits its seed.
fn main_with(quill: &Quill, overlay: Option<&SeedOverlay>) -> Card {
    let (mut payload, body) = seed_parts(&quill.config().main, overlay);
    payload.set_quill(main_reference(quill.config()));
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

/// The empty document: [`Document::new`] under [`main_reference`], so no
/// caller spells `name@MAJOR.MINOR` to reach the document the authoring
/// contract binds.
pub(crate) fn empty_document(quill: &Quill) -> Document {
    Document::new(main_reference(quill.config()))
}

pub(crate) fn seed_document(quill: &Quill) -> Document {
    // A fresh document carries no `$seed`, so every kind seeds from its own.
    document_with(quill, |_| None)
}

/// The main card and one card per kind, each committing `overlay(schema)` in
/// place of its kind's `seed:`, else that seed, as [`seed_document`]'s do.
pub(crate) fn document_with(
    quill: &Quill,
    overlay: impl Fn(&CardSchema) -> Option<SeedOverlay>,
) -> Document {
    let main = main_with(quill, overlay(&quill.config().main).as_ref());
    let cards = quill
        .config()
        .card_kinds
        .iter()
        .map(|schema| seed_composable(schema, overlay(schema).as_ref()))
        .collect();
    Document::from_main_and_cards(main, cards)
}

#[cfg(test)]
mod tests;
