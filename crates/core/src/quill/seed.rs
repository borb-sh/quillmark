//! Document seeding from a quill schema: commit each field's `example` and
//! leave every other field absent, so the render layer still supplies
//! `default`/blank. A committed `example` on a must-fill field carries the
//! `!must_fill` marker, so seeding and the blueprint stamp the same cells and a
//! fresh seed reads as incomplete exactly where a blank document does.

use quillmark_content::Normalized;

use super::Quill;
use crate::quill::{CardSchema, FieldType, VARIANT_DISCRIMINANT_KEY};
use crate::document::PayloadItem;
use crate::{Card, Document, Payload, QuillReference, QuillValue, SeedOverlay};

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
        if field.is_variant_bearing() {
            if let Some(item) = seed_variant(name, field, overlaid) {
                items.push(item);
            }
            continue;
        }
        let Some(Seeded { value, fills }) = seed_field(field, overlaid) else {
            continue;
        };
        let mut value = seeded_rest(name, &value, field);
        for path in fills.iter().filter(|p| !p.is_empty()) {
            value.set_fill_at(path);
        }
        items.push(PayloadItem::Field {
            key: name.clone(),
            value,
            // The root marker, where the field itself is the marked cell. A
            // mapping never carries one: its obligation sits on the leaves
            // inside it, which `fills` addresses by path.
            fill: fills.iter().any(Vec::is_empty),
            nested_comments: Vec::new(),
        });
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

    (Payload::from_items(items), body)
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
    if let (FieldType::Object, Some(props)) = (&field.r#type, &field.properties) {
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

/// Seed one variant-bearing enum, or `None` where neither the overlay nor any
/// `example:` in the selected world has anything to commit.
///
/// **The discriminant resolves first.** Which world is live decides which fields
/// are even candidates, so an overlay naming the discriminant must be read
/// before the field set is walked — otherwise the seed commits one world's tag
/// beside another world's answers.
///
/// The world walked is `overlay › example: › default: › blank`, the render
/// floor's own selection, so a cell lands under the member the seeded card
/// renders. Only a member the overlay or an `example:` named is *written*: a
/// `default:` is read-only here as everywhere. Per cell the precedence is the
/// ordinary `overlay › example: › absent`.
fn seed_variant(
    name: &str,
    field: &crate::quill::FieldSchema,
    overlaid: Option<&QuillValue>,
) -> Option<PayloadItem> {
    let overlay_json = overlaid.map(|v| v.as_json());
    let overlay_object = overlay_json.and_then(|j| j.as_object());
    let overlay_member =
        crate::quill::FieldSchema::authored_member(overlay_json).and_then(|v| v.as_str());

    let committed_member =
        overlay_member.or_else(|| field.example.as_ref().and_then(|e| e.as_str()));
    let member = committed_member
        .or_else(|| field.default.as_ref().and_then(|d| d.as_str()))
        .unwrap_or_default();

    let mut map = serde_json::Map::new();
    let mut fills: Vec<Vec<crate::value::PathSegment>> = Vec::new();
    if let Some(committed) = committed_member {
        map.insert(
            VARIANT_DISCRIMINANT_KEY.to_string(),
            serde_json::Value::String(committed.to_string()),
        );
        if overlay_member.is_none() && field.must_fill() {
            fills.push(vec![crate::value::PathSegment::Key(
                VARIANT_DISCRIMINANT_KEY.to_string(),
            )]);
        }
    }

    if let Some(fields) = field.variant_fields(member) {
        for (key, schema) in fields {
            // A cell carries any type a card field may, so it seeds through the
            // same descent.
            let overlaid_cell = overlay_object
                .and_then(|o| o.get(key))
                .map(|j| QuillValue::from_json(j.clone()));
            let Some(seeded) = seed_field(schema, overlaid_cell.as_ref()) else {
                continue;
            };
            for path in seeded.fills {
                let mut rebased = vec![crate::value::PathSegment::Key(key.clone())];
                rebased.extend(path);
                fills.push(rebased);
            }
            map.insert(key.clone(), seeded.value.into_json());
        }
    }

    if map.is_empty() {
        return None;
    }

    let mut value = seeded_rest(
        name,
        &QuillValue::from_json(serde_json::Value::Object(map)),
        field,
    );
    for path in &fills {
        value.set_fill_at(path);
    }
    Some(PayloadItem::Field {
        key: name.to_string(),
        value,
        // A mapping never carries the root marker: the obligation sits on the
        // discriminant cell inside it.
        fill: false,
        nested_comments: Vec::new(),
    })
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
