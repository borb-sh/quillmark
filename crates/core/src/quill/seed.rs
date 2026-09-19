//! Document seeding from a quill schema: commit each field's `example` and
//! leave every other field absent, so the render layer still supplies
//! `default`/blank. A committed `example` on a must-fill field carries the
//! `!must_fill` marker, so seeding and the blueprint stamp the same cells and a
//! fresh seed reads as incomplete exactly where a blank document does.

use quillmark_content::model::Normalized;

use super::Quill;
use crate::quill::CardSchema;
use crate::document::PayloadItem;
use crate::{
    document::{Card, Document, Payload, SeedOverlay},
    value::QuillValue,
    version::QuillReference,
};

/// Build the seeded `(payload, body)` for one card schema, layering an optional
/// [`SeedOverlay`] over the schema-example base. Per field the precedence is
/// `overlay › example › absent`; the overlay may also add a field the base
/// omits. Body: `overlay › body.example › empty`, honored only when the kind
/// enables bodies. The `$quill` / `$kind` system metadata is attached by the
/// caller.
///
/// Every seeded content field commits through [`seeded_rest`], the same strict
/// write the typed writer uses, so a seed is at rest from birth
/// (`SCHEMAS.md` § "Document seeding": seed-commits-rest).
fn seed_parts(schema: &CardSchema, overlay: Option<&SeedOverlay>) -> (Payload, Normalized) {
    // Driven by `schema.fields`, so the result is in declaration order natively
    // and an overlay key naming no schema field is never reached.
    let mut items: Vec<PayloadItem> = Vec::new();
    for (name, field) in &schema.fields {
        let overlaid = overlay.and_then(|o| o.fields.get(name));
        if let Some(item) = seed_item(name, field, overlaid) {
            items.push(item);
        }
        // **The discriminant resolves first.** Which world is live decides
        // which cells seed an `example:`, so the world walked is
        // `overlay › example: › default: › blank`, the render floor's own
        // selection. A cell of another world commits only what the overlay
        // wrote for it: `$seed` is a template author deciding, and the card
        // reports the stranded cell as `validation::out_of_variant`.
        let member = overlaid
            .and_then(|v| v.as_str())
            .or_else(|| field.example.as_ref().and_then(|e| e.as_str()))
            .or_else(|| field.default.as_ref().and_then(|d| d.as_str()))
            .unwrap_or_default();
        let live = field.variant_fields(member);
        for (cell_name, cell) in field.variant_cells() {
            let overlaid = overlay.and_then(|o| o.fields.get(cell_name));
            let seeded = match live.is_some_and(|world| world.contains_key(cell_name)) {
                true => seed_item(cell_name, cell, overlaid),
                false => overlaid.and_then(|_| seed_item(cell_name, cell, overlaid)),
            };
            if let Some(item) = seeded {
                items.push(item);
            }
        }
    }

    // Body region as a content: an overlay body (authored markdown) is imported;
    // otherwise the `body.example` content cache is used; else empty, and only
    // when bodies are enabled for the kind.
    let body = if schema.body_enabled() {
        if let Some(overlay_body) = overlay.and_then(|o| o.body.clone()) {
            crate::document::import_body(&overlay_body).unwrap_or_else(|_| Normalized::empty())
        } else if let Some(content) = schema.body.as_ref().and_then(|b| b.example_content.as_ref()) {
            quillmark_content::serial::from_canonical_value(content.as_json())
                .unwrap_or_else(|_| Normalized::empty())
        } else if let Some(example) = schema.body.as_ref().and_then(|b| b.example.as_ref()) {
            // Fallback for a schema built outside the loader (no cached content).
            crate::document::import_body(example).unwrap_or_else(|_| Normalized::empty())
        } else {
            Normalized::empty()
        }
    } else {
        Normalized::empty()
    };

    debug_assert!(
        items.len() <= crate::error::MAX_FIELD_COUNT,
        "a loaded quill's card schema declares at most MAX_FIELD_COUNT fields"
    );
    (Payload::from_items(items), body)
}

/// One field's payload item, or `None` where it has nothing to commit.
fn seed_item(
    name: &str,
    field: &crate::quill::FieldSchema,
    overlaid: Option<&QuillValue>,
) -> Option<PayloadItem> {
    let Seeded { value, fills } = seed_field(field, overlaid)?;
    let mut value = seeded_rest(name, &value, field);
    for path in fills.iter().filter(|p| !p.is_empty()) {
        value.set_fill_at(path);
    }
    Some(PayloadItem::Field {
        key: name.to_string(),
        value,
        // The root marker, where the field itself is the marked cell. A
        // mapping never carries one: its obligation sits on the leaves
        // inside it, which `fills` addresses by path.
        fill: fills.iter().any(Vec::is_empty),
    })
}

/// What one field contributes to a seed: the value to commit, and the paths
/// inside it that carry a `!must_fill` marker (the empty path being the value's
/// own root).
struct Seeded {
    value: QuillValue,
    fills: Vec<Vec<crate::value::PathSegment>>,
}

/// The `example:` a field commits, descending a typed dictionary to reach the
/// examples its properties declare: a namespace carries no `example:` of its
/// own (`quill::example_on_namespace`), so its seed composes from whatever its
/// cells commit and stays `None` when none do.
///
/// The content companion is read before the raw `example:`, so a content field
/// seeds its resting form. An overlay covers the whole field, cells included,
/// and lifts the marker: `$seed` is a template author deciding, which is the
/// act the marker asks for.
fn seed_field(field: &crate::quill::FieldSchema, overlaid: Option<&QuillValue>) -> Option<Seeded> {
    if let Some(value) = overlaid {
        return Some(Seeded {
            value: value.clone(),
            fills: Vec::new(),
        });
    }
    // A matrix seeds empty. It holds no literal of its own, and a column's
    // `example:` documents one cell's shape rather than which members a fresh
    // document ticks — committing it would seed every member held. The
    // blueprint shows the vocabulary through the roster instead.
    if matches!(field.r#type, crate::quill::FieldType::Matrix { .. }) {
        return None;
    }
    if let Some(props) = field.namespace_props() {
        let mut map = serde_json::Map::new();
        let mut fills = Vec::new();
        for (name, prop) in props {
            let Some(seeded) = seed_field(prop, None) else {
                continue;
            };
            for path in seeded.fills {
                let mut rebased = vec![crate::value::PathSegment::Key(name.clone())];
                rebased.extend(path);
                fills.push(rebased);
            }
            map.insert(name.clone(), seeded.value.into_json());
        }
        if map.is_empty() {
            return None;
        }
        return Some(Seeded {
            value: QuillValue::from_json(serde_json::Value::Object(map)),
            fills,
        });
    }
    let value = field
        .example_content
        .as_ref()
        .or(field.example.as_ref())?
        .clone();
    // An `example` documents shape, not the answer, so it commits *carrying the
    // marker*, landing a seed on the cells the blueprint stamps.
    let fills = if field.must_fill() {
        vec![Vec::new()]
    } else {
        Vec::new()
    };
    Some(Seeded { value, fills })
}

/// The form a seeded value commits at: the strict write's, for a field whose
/// type tree bears a content leaf; verbatim for every other field (a scalar's
/// authored shorthand is the typed write's to canonicalize, and conform leaves
/// it alone). A value the strict write refuses (an `example` the schema's own
/// validation flagged at load) stays authored, exactly as conform leaves it.
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
fn main_reference(quill: &Quill) -> QuillReference {
    let config = quill.config();
    format!("{}@{}", config.name, config.version)
        .parse()
        .unwrap_or_else(|_| QuillReference::latest(config.name.clone()))
}

pub(crate) fn seed_main(quill: &Quill) -> Card {
    // The main card is never seeded from an overlay: `$seed` keys range over
    // composable `card_kinds`, and `main` is not one of them.
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

pub(crate) fn seed_document(quill: &Quill) -> Document {
    // A fresh document carries no `$seed`, so every kind seeds from its schema
    // example base (overlay = `None`).
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
