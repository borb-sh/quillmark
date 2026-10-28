//! The quill's example document, derived from the `example:` its slots declare
//! and the `example` its bodies declare.
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::{seed, CardSchema, FieldSchema, FieldType, Quill, VARIANT_DISCRIMINANT_KEY};
use crate::document::{Document, SeedOverlay};
use crate::value::QuillValue;

impl Quill {
    /// The quill's example document: a filled-in page whose values are made
    /// up, to show what the quill looks like in use. Nobody keeps it; starter
    /// content someone keeps is a template.
    ///
    /// It is [`seed_document`](Self::seed_document) with every declared
    /// `example:` committed as its cell's value and each kind's `body.example`
    /// as its body, at rest as a seeded document is. A slot declaring none
    /// composes one from the slots below it: a typed dictionary from its
    /// properties, an array as one element from its `items`, a typed table
    /// as one row from its columns, a matrix as its roster's first member
    /// held with its columns. A variant-bearing enum's world is the one its
    /// example names, else its default's, and that world's cells commit
    /// theirs. An example outranks a kind's `seed:` where both name a cell.
    ///
    /// `None` where the quill declares no example the document would commit.
    pub fn example_document(&self) -> Option<Document> {
        let committed = std::cell::Cell::new(false);
        let document = seed::document_with(self, |card| {
            let overlay = example_overlay(card)?;
            committed.set(true);
            Some(overlay)
        });
        committed.get().then_some(document)
    }
}

/// A card's example as the overlay it commits: its kind's `seed:` with each
/// [`example_value`] and the body's example over it. `None` where the card
/// commits no example.
fn example_overlay(card: &CardSchema) -> Option<SeedOverlay> {
    let examples: Vec<(&String, JsonValue)> = card
        .fields
        .iter()
        .filter_map(|(name, field)| Some((name, example_value(field)?)))
        .collect();
    let body = card
        .body
        .as_ref()
        .and_then(|b| b.example.as_deref())
        .filter(|text| card.body_enabled() && !text.trim().is_empty());
    if examples.is_empty() && body.is_none() {
        return None;
    }
    let mut overlay = card.seed.clone().unwrap_or(SeedOverlay {
        fields: indexmap::IndexMap::new(),
        body: None,
    });
    for (name, value) in examples {
        overlay.fields.insert(name.clone(), QuillValue::from_json(value));
    }
    if let Some(body) = body {
        overlay.body = Some(body.to_string());
    }
    Some(overlay)
}

/// The value a slot commits in the example document: its `example:`, else
/// the value the slots below it compose, else `None`.
fn example_value(field: &FieldSchema) -> Option<JsonValue> {
    if field.is_variant_bearing() {
        let member = match field.example.as_ref() {
            Some(example) => example.as_str().unwrap_or_default().to_string(),
            None => field.selected_member(None),
        };
        let mut world = JsonMap::new();
        if let Some(example) = &field.example {
            world.insert(VARIANT_DISCRIMINANT_KEY.to_string(), example.as_json().clone());
        }
        for (name, cell) in field.variant_fields(&member).into_iter().flatten() {
            if let Some(value) = example_value(cell) {
                world.insert(name.clone(), value);
            }
        }
        return (!world.is_empty()).then_some(JsonValue::Object(world));
    }
    if let Some(example) = &field.example {
        return Some(example.as_json().clone());
    }
    match &field.r#type {
        FieldType::Object => composed(field.properties.as_ref()?).map(JsonValue::Object),
        FieldType::Array if field.max != Some(0) => {
            let element = example_value(field.items.as_deref()?)?;
            Some(JsonValue::Array(vec![element]))
        }
        FieldType::Matrix { roster, .. } => {
            let columns = composed(field.matrix_columns())?;
            let first = roster.keys().next()?;
            let mut held = JsonMap::new();
            held.insert(first.clone(), JsonValue::Object(columns));
            Some(JsonValue::Object(held))
        }
        _ => None,
    }
}

/// The mapping of each property's [`example_value`], `None` where none has one.
fn composed(props: &indexmap::IndexMap<String, Box<FieldSchema>>) -> Option<JsonMap<String, JsonValue>> {
    let map: JsonMap<String, JsonValue> = props
        .iter()
        .filter_map(|(name, prop)| Some((name.clone(), example_value(prop)?)))
        .collect();
    (!map.is_empty()).then_some(map)
}
