//! The authoring document: the values form lifted to the whole document, the
//! JSON a model reads and writes (`prose/canon/AUTHORING.md`).

use serde_json::{Map as JsonMap, Value as JsonValue};

use super::schema::{build_transform_schema, SchemaCodec, AUTHORING_KIND_KEY};
use super::seed::main_reference;
use super::{CardSchema, Quill, QuillConfig};
use crate::document::{import_body_at, Card, Document, EditError, Parsed};
use crate::error::{Diagnostic, Severity};
use crate::path::DocPath;
use crate::reader::project_value;

/// The code every structural refusal of [`Quill::from_authoring`] carries: a
/// value whose shape is not the authoring document's.
pub const AUTHORING_INVALID_SHAPE: &str = "authoring::invalid_shape";

impl QuillConfig {
    /// The JSON Schema of the authoring document:
    /// [`build_transform_schema`] under [`SchemaCodec::Text`].
    pub fn authoring_schema(&self) -> JsonValue {
        build_transform_schema(self, SchemaCodec::Text).into_json()
    }
}

impl Quill {
    /// `doc` as an authoring document: `{fields, body, cards: [{kind, fields,
    /// body}]}`, every field in the values form
    /// ([`TypedReader::get`](crate::reader::TypedReader::get)) and every body
    /// as Markdown. Sparse, as the values form is: an absent field stays absent.
    /// A field the schema does not declare, or any field of a card whose kind
    /// it does not declare, rides verbatim. `body` is present where the kind
    /// enables one or the card carries one; `cards` where the document has any.
    /// `$quill`, `$ext` and `$seed` never appear.
    ///
    /// Fails with `edit::field_decode`, anchored at the leaf, when a content
    /// leaf holds a value that decodes under neither encoding.
    pub fn to_authoring(&self, doc: &Document) -> Result<JsonValue, Diagnostic> {
        let main = Some(&self.config.main);
        let mut root =
            authoring_card(doc.main(), main).map_err(|e| edit_error(&e, &DocPath::main()))?;
        if !doc.cards().is_empty() {
            let cards = doc
                .cards()
                .iter()
                .enumerate()
                .map(|(index, card)| {
                    let kind = card.kind().unwrap_or_default();
                    let mut envelope = JsonMap::new();
                    envelope.insert(AUTHORING_KIND_KEY.to_string(), kind.into());
                    let base = DocPath::card(Some(kind), index);
                    envelope.extend(
                        authoring_card(card, self.config.card_kind(kind))
                            .map_err(|e| edit_error(&e, &base))?,
                    );
                    Ok(JsonValue::Object(envelope))
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            root.insert("cards".to_string(), JsonValue::Array(cards));
        }
        Ok(JsonValue::Object(root))
    }

    /// A document from an authoring document, conformed against this quill:
    /// the JSON twin of [`parse`](Self::parse). The document carries this
    /// quill's `$quill`; a `null` anywhere a value goes is absent.
    ///
    /// Field values are stored as written and then conformed, so a value the
    /// schema refuses lands with a `conform::*` warning rather than failing
    /// the import, and [`validate`](Self::validate) reports the rest at its
    /// document-model path. The `warnings` carry a body's or a `richtext`
    /// field's dropped constructs and the conform diagnostics.
    ///
    /// Refused, with every error found, when the value is not the authoring
    /// document's shape ([`AUTHORING_INVALID_SHAPE`]), or when a name or kind
    /// cannot be stored (`edit::*`, as [`Card::store_field`] and
    /// [`Card::new`] refuse it).
    pub fn from_authoring(&self, value: &JsonValue) -> Result<Parsed, Vec<Diagnostic>> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut document = Document::new(main_reference(&self.config));

        let Some(root) = expect_object(value, "", &mut errors) else {
            return Err(errors);
        };
        for key in root.keys() {
            if !matches!(key.as_str(), "fields" | "body" | "cards") {
                errors.push(shape_error(key, "is not an authoring document key; expected `fields`, `body` or `cards`"));
            }
        }
        let main = document.main_card_mut();
        fill_card(main, root, &DocPath::main(), "", &mut errors, &mut warnings);

        let cards = match root.get("cards") {
            None | Some(JsonValue::Null) => &[][..],
            Some(JsonValue::Array(cards)) => cards.as_slice(),
            Some(_) => {
                errors.push(shape_error("cards", "must be an array of cards"));
                &[][..]
            }
        };
        for (index, value) in cards.iter().enumerate() {
            let at = format!("cards[{index}]");
            let Some(envelope) = expect_object(value, &at, &mut errors) else {
                continue;
            };
            for key in envelope.keys() {
                if !matches!(key.as_str(), AUTHORING_KIND_KEY | "fields" | "body") {
                    errors.push(shape_error(
                        &format!("{at}.{key}"),
                        "is not a card key; expected `kind`, `fields` or `body`",
                    ));
                }
            }
            let Some(kind) = envelope.get(AUTHORING_KIND_KEY).and_then(JsonValue::as_str) else {
                errors.push(shape_error(&format!("{at}.{AUTHORING_KIND_KEY}"), "must name the card's kind"));
                continue;
            };
            let mut card = match Card::new(kind) {
                Ok(card) => card,
                Err(e) => {
                    errors.push(edit_error(&e, &DocPath::card(None, index)));
                    continue;
                }
            };
            let base = DocPath::card(Some(kind), index);
            fill_card(&mut card, envelope, &base, &format!("{at}."), &mut errors, &mut warnings);
            if let Err(e) = document.push_card(card) {
                errors.push(edit_error(&e, &base));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        let conformed = self.conform(&mut document).map_err(|e| e.diagnostics().to_vec())?;
        warnings.extend(conformed);
        Ok(Parsed { document, warnings })
    }
}

/// One card's `fields` and `body` in the authoring form.
fn authoring_card(card: &Card, schema: Option<&CardSchema>) -> Result<JsonMap<String, JsonValue>, EditError> {
    let mut fields = JsonMap::new();
    for (name, value) in card.payload().iter() {
        let value = match schema.and_then(|s| s.fields.get(name)) {
            Some(field) => project_value(name, value.as_json(), field, &mut Vec::new())?,
            None => value.as_json().clone(),
        };
        fields.insert(name.clone(), value);
    }
    let mut envelope = JsonMap::new();
    envelope.insert("fields".to_string(), JsonValue::Object(fields));
    let body = card.body_markdown();
    if schema.is_some_and(CardSchema::body_enabled) || !body.is_empty() {
        envelope.insert("body".to_string(), body.into());
    }
    Ok(envelope)
}

/// Store an envelope's `fields` and `body` on `card`. `at` prefixes the
/// envelope's own keys in a shape error.
fn fill_card(
    card: &mut Card,
    envelope: &JsonMap<String, JsonValue>,
    base: &DocPath,
    at: &str,
    errors: &mut Vec<Diagnostic>,
    warnings: &mut Vec<Diagnostic>,
) {
    let fields_at = format!("{at}fields");
    let fields = envelope.get("fields").filter(|v| !v.is_null());
    if let Some(fields) = fields.and_then(|f| expect_object(f, &fields_at, errors)) {
        for (name, value) in fields.iter().filter(|(_, v)| !v.is_null()) {
            if let Err(e) = card.store_field(name, value.clone()) {
                errors.push(edit_error(&e, base));
            }
        }
    }
    match envelope.get("body") {
        None | Some(JsonValue::Null) => {}
        Some(JsonValue::String(markdown)) => {
            match import_body_at(markdown, &base.body(), warnings) {
                Ok(body) => card.overwrite_body(body),
                Err(e) => errors.push(
                    Diagnostic::new(Severity::Error, e.to_string())
                        .with_code("edit::import".to_string())
                        .with_path(base.body().to_string()),
                ),
            }
        }
        Some(_) => errors.push(shape_error(&format!("{at}body"), "must be a Markdown string")),
    }
}

fn expect_object<'v>(
    value: &'v JsonValue,
    at: &str,
    errors: &mut Vec<Diagnostic>,
) -> Option<&'v JsonMap<String, JsonValue>> {
    let object = value.as_object();
    if object.is_none() {
        errors.push(shape_error(at, "must be an object"));
    }
    object
}

/// A shape refusal. Its path is the authoring document's own (`cards[1].kind`),
/// since the value it names never reached the document model.
fn shape_error(at: &str, message: &str) -> Diagnostic {
    if at.is_empty() {
        return Diagnostic::new(Severity::Error, format!("The authoring document {message}"))
            .with_code(AUTHORING_INVALID_SHAPE.to_string());
    }
    Diagnostic::new(Severity::Error, format!("`{at}` {message}"))
        .with_code(AUTHORING_INVALID_SHAPE.to_string())
        .with_path(at.to_string())
}

fn edit_error(e: &EditError, base: &DocPath) -> Diagnostic {
    let diag = Diagnostic::new(Severity::Error, e.to_string())
        .with_code(e.code().to_string())
        .with_args(e.args());
    match e.doc_path(base) {
        Some(path) => diag.with_path(path.to_string()),
        None => diag,
    }
}
