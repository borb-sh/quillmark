use std::collections::BTreeMap;

use crate::document::{Document, Payload};
use crate::error::{Diagnostic, Severity, diag_args};
use crate::path::DocPath;
use crate::quill::formats::{is_valid_date, is_valid_datetime};
use crate::quill::{CardSchema, FieldSchema, FieldType, QuillConfig, VARIANT_DISCRIMINANT_KEY};
use crate::value::QuillValue;

/// Validation error with a structured field path.
///
/// Field-level type and presence errors carry the field path, the
/// schema-declared type, and any verbatim YAML source token / default:
/// enough for the `Display` impl to render the uniform diagnostic message
/// described in `ERROR.md` ("Validation message contract").
///
/// Two concerns are deliberately *not* well-formedness errors and so have no
/// variant here: the `!must_fill` marker (surfaced as a non-fatal warning by
/// `Quill::validate`) and field absence (an absent or present-null field
/// blank-fills at render). Both are handled outside the value-layer checks below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    TypeMismatch {
        path: String,
        /// Schema-declared type (`string`, `integer`, …).
        expected: String,
        /// YAML-parsed type of the source token (`integer`, `number`,
        /// `boolean`, `null`, `string`, `array`, `object`).
        actual: String,
        /// Verbatim YAML scalar that triggered the error, rendered in
        /// its canonical YAML form (`42`, `null`, `"hello"`, `""`).
        source_token: String,
        /// Pre-rendered default token from the schema, when present.
        /// Same canonical YAML form as `source_token`.
        default: Option<String>,
    },

    EnumViolation {
        path: String,
        value: String,
        allowed: Vec<String>,
    },

    FormatViolation {
        path: String,
        format: String,
    },

    UnknownCard {
        path: String,
        card: String,
    },

    BodyDisabled {
        path: String,
        card: String,
    },

    /// An `inline: true` field whose content is not a single line (a block, a
    /// list/quote container, or an island). Same fatality class as
    /// `TypeMismatch`: the value is well-typed content but the wrong *shape*
    /// for an inline field. Both prose codecs declare `inline`, so this is one
    /// condition under one code, the validation twin of
    /// [`EditError::FieldNotInline`](crate::EditError::FieldNotInline).
    NotInline {
        path: String,
    },

    /// A `plaintext` field whose content carries marks, islands, or block
    /// formatting. Same fatality class as `TypeMismatch`: the value is a
    /// well-formed content but the wrong *shape* for a plaintext field, which
    /// takes prose the author navigates but no formatting.
    NotPlain {
        path: String,
    },
}

impl std::error::Error for ValidationError {}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::TypeMismatch {
                path,
                expected,
                actual,
                source_token,
                default,
            } => {
                // Line 1: what we got vs what the schema says.
                write!(
                    f,
                    "Field `{path}` got {actual} `{source_token}`, schema declares `{expected}`"
                )?;
                if let Some(d) = default {
                    write!(f, " with default `{d}`")?;
                }
                write!(
                    f,
                    ". {hint}",
                    hint = type_mismatch_hint(expected, actual, default.as_deref())
                )
            }
            ValidationError::EnumViolation {
                path,
                value,
                allowed,
            } => {
                write!(
                    f,
                    "field `{path}` value `{value}` not in allowed set {allowed:?}"
                )
            }
            ValidationError::FormatViolation { path, format } => {
                write!(
                    f,
                    "field `{path}` does not match expected format `{format}`"
                )
            }
            ValidationError::UnknownCard { path, card } => {
                write!(f, "unknown card kind `{card}` at `{path}`")
            }
            ValidationError::BodyDisabled { path, card } => {
                write!(
                    f,
                    "card `{card}` at `{path}` has body content but the card kind declares `body.enabled: false`: {hint}",
                    hint = body_disabled_hint(),
                )
            }
            ValidationError::NotInline { path } => {
                write!(
                    f,
                    "field `{path}` declares `inline` but its content is not a single \
                     line: {hint}",
                    hint = not_inline_hint(),
                )
            }
            ValidationError::NotPlain { path } => {
                write!(
                    f,
                    "field `{path}` is `plaintext` but its content carries formatting: {hint}",
                    hint = not_plain_hint(),
                )
            }
        }
    }
}

/// Actionable exit clause for a TypeMismatch. Mirrors the (expected, actual,
/// has_default) branching in `Display` so the structured hint and the prose
/// message can never disagree.
fn type_mismatch_hint(expected: &str, actual: &str, default: Option<&str>) -> String {
    if default.is_some() {
        format!(
            "Either omit the line (the default will fill in) or provide a value of type `{expected}`."
        )
    } else {
        format!(
            "Either provide a value of type `{expected}` or change the schema's `type:` to `{actual}`."
        )
    }
}

/// Actionable exit clause for a `BodyDisabled` error. Same text in both the
/// prose message and the structured hint.
fn body_disabled_hint() -> &'static str {
    "remove the body content or set `body.enabled: true` on the card kind"
}

/// Actionable exit clause for a `NotInline` error. Codec-neutral: both prose
/// types declare `inline`, and the way out is the same for either.
fn not_inline_hint() -> &'static str {
    "keep the value to a single line (no blank lines, headings, lists, \
     quotes, or tables), or drop `inline: true` from the schema"
}

/// Actionable exit clause for a `NotPlain` error.
fn not_plain_hint() -> &'static str {
    "remove the formatting (marks, tables, images, headings, lists, quotes), or \
     change the schema's `type:` to `richtext`"
}

impl ValidationError {
    /// Document-model path anchor for this error.
    ///
    /// See [`crate::error`] module docs for the path grammar and conventions.
    pub fn path(&self) -> &str {
        match self {
            ValidationError::TypeMismatch { path, .. }
            | ValidationError::EnumViolation { path, .. }
            | ValidationError::FormatViolation { path, .. }
            | ValidationError::UnknownCard { path, .. }
            | ValidationError::BodyDisabled { path, .. }
            | ValidationError::NotInline { path, .. }
            | ValidationError::NotPlain { path, .. } => path,
        }
    }

    /// Stable diagnostic code for this error variant. Pattern-match on this
    /// instead of the message text.
    pub fn code(&self) -> &'static str {
        match self {
            ValidationError::TypeMismatch { .. } => "validation::type_mismatch",
            ValidationError::EnumViolation { .. } => "validation::enum_violation",
            ValidationError::FormatViolation { .. } => "validation::format_violation",
            ValidationError::UnknownCard { .. } => "validation::unknown_card",
            ValidationError::BodyDisabled { .. } => "validation::body_disabled",
            ValidationError::NotInline { .. } => "validation::not_inline",
            ValidationError::NotPlain { .. } => "validation::not_plain",
        }
    }

    /// The facts this error's message interpolates. See
    /// [`Diagnostic::args`](crate::error::Diagnostic::args).
    ///
    /// `path` stays out: it is the diagnostic's anchor, and an anchor
    /// reachable by two routes acquires two spellings. `NotInline` and
    /// `NotPlain` carry nothing else, so their sentence follows from the code
    /// and the anchor alone.
    ///
    /// `default` is present only when the schema declares one, the same
    /// condition `type_mismatch_hint` branches on, so a consumer picks its
    /// own exit clause from the key's presence instead of re-deriving the
    /// branch. Emitting `null` instead would read as a default spelled `null`.
    pub fn args(&self) -> BTreeMap<String, serde_json::Value> {
        match self {
            ValidationError::TypeMismatch {
                path: _,
                expected,
                actual,
                source_token,
                default,
            } => {
                let mut args = diag_args! {
                    "expected" => expected,
                    "actual" => actual,
                    "sourceToken" => source_token,
                };
                if let Some(default) = default {
                    args.insert("default".to_string(), serde_json::json!(default));
                }
                args
            }
            ValidationError::EnumViolation {
                path: _,
                value,
                allowed,
            } => diag_args! {
                "value" => value,
                "allowed" => allowed,
            },
            ValidationError::FormatViolation { path: _, format } => diag_args! {
                "format" => format,
            },
            ValidationError::UnknownCard { path: _, card } => diag_args! {
                "card" => card,
            },
            ValidationError::BodyDisabled { path: _, card } => diag_args! {
                "card" => card,
            },
            ValidationError::NotInline { path: _ } => diag_args! {},
            ValidationError::NotPlain { path: _ } => diag_args! {},
        }
    }

    /// Actionable hint for this error, when defined for the variant: the same
    /// string the `Display` impl bakes in, exposed so consumers can surface it
    /// without re-parsing prose.
    pub fn hint(&self) -> Option<String> {
        match self {
            ValidationError::TypeMismatch {
                expected,
                actual,
                default,
                ..
            } => Some(type_mismatch_hint(expected, actual, default.as_deref())),
            ValidationError::BodyDisabled { .. } => Some(body_disabled_hint().to_string()),
            ValidationError::NotInline { .. } => Some(not_inline_hint().to_string()),
            ValidationError::NotPlain { .. } => Some(not_plain_hint().to_string()),
            ValidationError::EnumViolation { .. }
            | ValidationError::FormatViolation { .. }
            | ValidationError::UnknownCard { .. } => None,
        }
    }

    /// Convert this error into a structured [`Diagnostic`] carrying the
    /// stable code, the document-model `path`, the canonical message, and
    /// the actionable hint (when the variant defines one).
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diag = Diagnostic::new(Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_path(self.path().to_string())
            .with_args(self.args());
        if let Some(hint) = self.hint() {
            diag = diag.with_hint(hint);
        }
        diag
    }
}

/// Render a JSON scalar as the verbatim YAML token it would parse from.
/// Primitives appear bare (`42`, `true`, `null`); strings appear quoted
/// (`"hello"`, `""`); compound values render as a short placeholder.
fn verbatim_yaml_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("\"{s}\""),
        serde_json::Value::Array(_) => "[…]".to_string(),
        serde_json::Value::Object(_) => "{…}".to_string(),
    }
}

/// YAML-parsed type name for a JSON value. Distinguishes `integer` from
/// `number` so diagnostic messages can report the two separately.
fn yaml_scalar_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        // An `integer` is the engine's `i64`. A numeric literal past that range
        // is a `number`, the type that does carry it, which keeps the mismatch
        // hint ("change the schema's `type:` to `{actual}`") true.
        serde_json::Value::Number(n) => {
            if n.is_i64() {
                "integer"
            } else {
                "number"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Validate a typed [`Document`] (typed [`Payload`] + typed `Card` list).
///
/// This is the typed entry point used by `QuillConfig::validate_document`.
pub fn validate_typed_document(
    config: &QuillConfig,
    doc: &Document,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = validate_fields_for_card(&config.main, doc.main().payload(), &DocPath::main());

    // Enforce body.enabled on the main card. Whitespace-only bodies are
    // treated as empty: only meaningful prose triggers the diagnostic.
    if !config.main.body_enabled() && !doc.main().body().is_blank() {
        errors.push(ValidationError::BodyDisabled {
            path: DocPath::main_body().to_string(),
            card: "main".to_string(),
        });
    }

    for (index, card) in doc.cards().iter().enumerate() {
        let card_name = card.kind().unwrap_or("").to_string();

        let Some(card_schema) = config.card_kind(card_name.as_str()) else {
            // An unknown-kind card has no kind to qualify with: `cards[<i>]`,
            // the sole bare-index root. (A document's cards are always a
            // `cards` list; the kind *definitions* live under `card_kinds:`.)
            errors.push(ValidationError::UnknownCard {
                path: DocPath::card(None, index).to_string(),
                card: card_name,
            });
            continue;
        };

        let card_path = DocPath::card(Some(&card_name), index);
        errors.extend(validate_fields_for_card(
            card_schema,
            card.payload(),
            &card_path,
        ));

        if !card_schema.body_enabled() && !card.body().is_blank() {
            errors.push(ValidationError::BodyDisabled {
                path: card_path.body().to_string(),
                card: card_name,
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_fields_for_card(
    card: &CardSchema,
    fields: &Payload,
    base: &DocPath,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut field_names: Vec<&String> = card.fields.keys().collect();
    field_names.sort();

    for field_name in field_names {
        let schema = &card.fields[field_name];
        let path = base.field(field_name);
        // Absence is a completeness concern, not a well-formedness one: an
        // absent field (like a present-null one) is blank-filled at render and
        // raises nothing here.
        if let Some(value) = fields.get(field_name) {
            errors.extend(validate_field(schema, value, &path));
        }
    }

    errors
}

/// Distinguishes the two value sources the conformance core
/// [`validate_value`] serves. The type/enum/format/recursion checks are
/// identical; only the document-authoring concerns differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueContext {
    /// A value parsed from an authored document. Treats present-null as absent
    /// and reports the field's `default:` token alongside a type mismatch.
    Document,
    /// An `example:` or `default:` literal declared in Quill.yaml. Partial
    /// objects are allowed (absent properties are not errors) and the
    /// document-only null/default semantics do not apply.
    SchemaLiteral,
}

/// Shared conformance core: validate a single `value` against `field` at
/// `path`, checking type compatibility, enum membership, datetime format, and
/// recursing into array elements / object properties. `ctx` selects the few
/// document-only behaviors (see [`ValueContext`]).
///
/// A document value is judged in the form the render floor builds from it, not
/// as authored: `conform_value` at `Leniency::Render` runs first. Validity is
/// therefore renderability, in both directions — `Quill::validate` neither
/// refuses a document `compile_data` accepts nor passes one it refuses. A leaf
/// the floor refuses is judged as authored, so the type check names the value
/// the author wrote, and is refused even where that check finds the authored
/// shape well-typed. Conforming runs per node rather than per field, so one
/// refused element does not mistype its siblings.
///
/// Schema literals are judged as written: an `example:`/`default:` is the
/// blueprint's own text, and coercing it would let the blueprint emit a
/// spelling it then teaches authors to write.
fn validate_value(
    field: &FieldSchema,
    value: &QuillValue,
    path: &DocPath,
    ctx: ValueContext,
) -> Vec<ValidationError> {
    // Null ≡ absent: a present-null value in a document is treated as omitted
    // (no type error). The `!must_fill` marker is surfaced separately as a
    // warning by `Quill::validate`, not here.
    if ctx == ValueContext::Document && value.as_json().is_null() {
        return vec![];
    }

    // Peeled before conforming, as `conform_value` peels it: the floor turns the
    // bare scalar (`classification: CUI`) into its container, which would move a
    // domain violation to `<path>.value`, a key the author never wrote.
    // `validate_variant` reads both shapes and conforms each live cell.
    if field.is_variant_bearing() {
        return validate_variant(field, value, path, ctx);
    }

    let conformed = match ctx {
        ValueContext::Document => Some(super::QuillConfig::conform_value(
            value,
            field,
            &path.to_string(),
            super::config::Leniency::Render,
        )),
        ValueContext::SchemaLiteral => None,
    };
    // A container's refusal belongs to the element or property that caused it,
    // which the recursion below reaches at its own path; a leaf's refusal is
    // this path's.
    let floor_refused = matches!(conformed, Some(Err(_)))
        && !matches!(field.r#type, FieldType::Array | FieldType::Object);
    let conformed = conformed.and_then(Result::ok);
    let value = conformed.as_ref().unwrap_or(value);

    let mut errors = Vec::new();

    let type_valid = match field.r#type {
        // Enum is string-valued data (domain membership is checked separately
        // below), so it is type-valid exactly where a string is.
        FieldType::String | FieldType::Enum => value.as_str().is_some(),
        // A conformed richtext/plaintext value is a canonical content object;
        // an authored `default`/`example` is a string (markdown for richtext,
        // literal for plaintext). The plaintext-specific plain constraint is
        // checked in the shape pass below, parallel to the inline check.
        FieldType::RichText { .. } | FieldType::PlainText { .. } => {
            value.as_json().is_object() || value.as_str().is_some()
        }
        FieldType::Integer => value.as_json().is_i64(),
        FieldType::Number => value.as_json().is_number(),
        FieldType::Boolean => value.as_bool().is_some(),
        FieldType::Date | FieldType::DateTime => {
            if value.as_json().is_null() {
                true
            } else {
                match value.as_str() {
                    Some("") => true,
                    Some(text) => {
                        let (ok, format) = match field.r#type {
                            FieldType::Date => (is_valid_date(text), "date"),
                            _ => (is_valid_datetime(text), "datetime"),
                        };
                        if ok {
                            true
                        } else {
                            errors.push(ValidationError::FormatViolation {
                                path: path.to_string(),
                                format: format.to_string(),
                            });
                            false
                        }
                    }
                    None => false,
                }
            }
        }
        FieldType::Array => match value.as_array() {
            Some(items) => {
                // Validate each element against the array's `items` schema.
                // Scalar elements (`string[]`, `integer[]`, `richtext[]`, …)
                // are type-checked element-wise; object elements recurse into
                // their properties via the Object branch.
                if let Some(item_schema) = &field.items {
                    for (idx, item) in items.iter().enumerate() {
                        let row_path = path.index(idx);
                        errors.extend(validate_value(
                            item_schema,
                            &QuillValue::from_json(item.clone()),
                            &row_path,
                            ctx,
                        ));
                    }
                }
                true
            }
            None => false,
        },
        FieldType::Object => match value.as_object() {
            Some(object) => {
                if let Some(properties) = &field.properties {
                    let mut property_names: Vec<&String> = properties.keys().collect();
                    property_names.sort();
                    for property_name in property_names {
                        let property_schema = &properties[property_name];
                        let property_path = path.field(property_name);
                        // Absent object property: completeness, not
                        // well-formedness. Like a top-level absent field, it
                        // blank-fills at render and raises nothing here.
                        if let Some(property_value) = object.get(property_name) {
                            errors.extend(validate_value(
                                property_schema,
                                &QuillValue::from_json(property_value.clone()),
                                &property_path,
                                ctx,
                            ));
                        }
                    }
                }
                true
            }
            None => false,
        },
    };

    // Content shape checks, run only on a type-valid value (a mistyped value
    // already raises TypeMismatch below, and a null/absent field blank-fills to
    // the empty content, which is both inline and plain). Mirror the
    // coercion-layer checks so a content that bypassed coercion (e.g. a direct
    // `validate_document`) is still caught. A decode failure names no shape:
    // for a document it is the floor's refusal, reported below; for a schema
    // literal it is the load-time content import's.
    if type_valid {
        match field.r#type {
            FieldType::RichText { inline: true } => {
                let parsed = crate::document::Codec::Richtext
                    .decode_value(value.as_json())
                    .and_then(Result::ok);
                if let Some(rt) = parsed {
                    if !rt.is_inline() {
                        errors.push(ValidationError::NotInline {
                            path: path.to_string(),
                        });
                    }
                }
            }
            FieldType::PlainText { inline } => {
                // Plaintext strings are literal, not markdown, so a schema
                // literal decodes through the literal codec; a Document value is
                // a canonical content object. The plain constraint is primary;
                // the single-line constraint applies only when `inline`.
                if let Some(rt) = crate::document::Codec::Plaintext
                    .decode_value(value.as_json())
                    .and_then(Result::ok)
                {
                    if !rt.is_plain() {
                        errors.push(ValidationError::NotPlain {
                            path: path.to_string(),
                        });
                    } else if inline && !rt.is_inline() {
                        errors.push(ValidationError::NotInline {
                            path: path.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // A Date/DateTime with a string value already emitted a FormatViolation;
    // skip the redundant TypeMismatch in that case.
    let format_error_already_reported =
        matches!(field.r#type, FieldType::Date | FieldType::DateTime) && value.as_str().is_some();

    // The floor refused a leaf the checks above find well-typed — a content
    // object that is not canonical content, an integer past `i64` — and no
    // shape check named the refusal. The mismatch is the refusal: the render
    // door and this surface give one verdict.
    let unnamed_refusal = floor_refused && errors.is_empty();

    if (!type_valid || unnamed_refusal) && !format_error_already_reported {
        errors.push(ValidationError::TypeMismatch {
            path: path.to_string(),
            expected: field.r#type.as_str().to_string(),
            actual: yaml_scalar_type(value.as_json()).to_string(),
            source_token: verbatim_yaml_scalar(value.as_json()),
            // The `default:` token is a document-authoring aid ("omit the line
            // and the default fills in"): meaningless when validating the
            // schema's own literals.
            default: match ctx {
                ValueContext::Document => field
                    .default
                    .as_ref()
                    .map(|d| verbatim_yaml_scalar(d.as_json())),
                ValueContext::SchemaLiteral => None,
            },
        });
    }

    if type_valid {
        // The accepted domain is `values ∪ blank`. Checked here rather than at
        // the call sites so `validate_value` stays context-free: an enum at
        // element position inside an array accepts the blank on the same line
        // as one at the top level.
        if let (Some(allowed), Some(actual)) = (&field.enum_values, value.as_str()) {
            if !actual.is_empty() && !allowed.contains(&actual.to_string()) {
                errors.push(ValidationError::EnumViolation {
                    path: path.to_string(),
                    value: actual.to_string(),
                    allowed: allowed.clone(),
                });
            }
        }
    }

    errors
}

/// Validate a variant-bearing enum. The discriminant answers the domain check
/// at `<path>.value`, and only the **live** world's fields are checked: a key a
/// non-active variant declares is not this layer's to type-check, since nothing
/// downstream reads it (`Quill::validate` reports it as
/// `validation::out_of_variant`, a warning).
///
/// Both authored shapes reach here: the container, and the bare scalar
/// (`classification: CUI`) a schema literal rests in and a document value the
/// floor refused falls back to. The scalar is checked as the discriminant it
/// stands for, at the container's own path — the path an author who wrote a
/// scalar can act on.
fn validate_variant(
    field: &FieldSchema,
    value: &QuillValue,
    path: &DocPath,
    ctx: ValueContext,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let json = value.as_json();

    let check_member = |member: &str, at: &DocPath, errors: &mut Vec<ValidationError>| {
        if let Some(allowed) = &field.enum_values {
            if !member.is_empty() && !allowed.contains(&member.to_string()) {
                errors.push(ValidationError::EnumViolation {
                    path: at.to_string(),
                    value: member.to_string(),
                    allowed: allowed.clone(),
                });
            }
        }
    };

    let Some(object) = json.as_object() else {
        // The bare scalar spelling. Anything that is not a member-shaped scalar
        // is a type error against the container.
        let scalar = value.as_str().map(str::to_string).or_else(|| match ctx {
            // The floor's leniency, which a document value passes through. A
            // schema literal is judged as written, and every reader of a
            // `default:` takes it through `as_str`.
            ValueContext::Document => super::config::scalar_as_string(json),
            ValueContext::SchemaLiteral => None,
        });
        match scalar {
            Some(member) => check_member(&member, path, &mut errors),
            None => errors.push(ValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "enum".to_string(),
                actual: yaml_scalar_type(json).to_string(),
                source_token: verbatim_yaml_scalar(json),
                default: match ctx {
                    ValueContext::Document => field
                        .default
                        .as_ref()
                        .map(|d| verbatim_yaml_scalar(d.as_json())),
                    ValueContext::SchemaLiteral => None,
                },
            }),
        }
        return errors;
    };

    // The container is a document spelling. A schema literal names the
    // discriminant alone, each world's cells carrying their own; the container
    // caches no content and yields no discriminant, so accepting it would
    // blank-fill the field in silence.
    if ctx == ValueContext::SchemaLiteral {
        errors.push(ValidationError::TypeMismatch {
            path: path.to_string(),
            expected: "enum".to_string(),
            actual: yaml_scalar_type(json).to_string(),
            source_token: verbatim_yaml_scalar(json),
            default: None,
        });
        return errors;
    }

    let discriminant_path = path.field(VARIANT_DISCRIMINANT_KEY);
    let authored = object
        .get(VARIANT_DISCRIMINANT_KEY)
        .filter(|v| !v.is_null());
    let member = match authored {
        Some(v) => match v
            .as_str()
            .map(str::to_string)
            .or_else(|| super::config::scalar_as_string(v))
        {
            Some(member) => {
                check_member(&member, &discriminant_path, &mut errors);
                member
            }
            None => {
                errors.push(ValidationError::TypeMismatch {
                    path: discriminant_path.to_string(),
                    expected: "enum".to_string(),
                    actual: yaml_scalar_type(v).to_string(),
                    source_token: verbatim_yaml_scalar(v),
                    default: None,
                });
                String::new()
            }
        },
        // An absent discriminant blank-fills from the ladder, exactly as an
        // absent scalar enum does; the world it selects is the default's.
        None => field
            .default
            .as_ref()
            .and_then(|d| d.as_str())
            .unwrap_or_default()
            .to_string(),
    };

    if let Some(fields) = field.variant_fields(&member) {
        for (name, schema) in fields {
            if let Some(cell) = object.get(name) {
                errors.extend(validate_value(
                    schema,
                    &QuillValue::from_json(cell.clone()),
                    &path.field(name),
                    ctx,
                ));
            }
        }
    }

    errors
}

/// Validate a single document value against a field schema at the given path.
pub(crate) fn validate_field(
    field: &FieldSchema,
    value: &QuillValue,
    path: &DocPath,
) -> Vec<ValidationError> {
    validate_value(field, value, path, ValueContext::Document)
}

/// Validate a schema literal value (an `example:` or `default:` declared in
/// Quill.yaml) against a field schema.
///
/// Shares the type/enum/format/recursion core with [`validate_field`] (see
/// [`validate_value`]) but omits the document-authoring concerns: it does not
/// apply null≡absent leniency, and never attaches a `default:` token to a type
/// mismatch (partial examples/defaults are intentional and valid).
pub(crate) fn validate_schema_literal(
    schema: &FieldSchema,
    value: &QuillValue,
    path: &DocPath,
) -> Vec<ValidationError> {
    validate_value(schema, value, path, ValueContext::SchemaLiteral)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Card, Document};
    use indexmap::IndexMap;
    use serde_json::json;

    fn config_with(main_fields: &str, cards: &str) -> QuillConfig {
        let yaml = format!(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  fields:
{main_fields}
{cards}
"#
        );
        let (config, warnings) = QuillConfig::from_yaml_with_warnings(&yaml).unwrap();
        assert!(
            warnings.is_empty(),
            "config_with produced warnings (test schema is unsupported): {:?}",
            warnings
        );
        config
    }

    fn doc_from_fm(entries: &[(&str, serde_json::Value)]) -> Document {
        doc_with_typed_cards(entries, vec![])
    }

    fn doc_with_typed_cards(fm: &[(&str, serde_json::Value)], cards: Vec<Card>) -> Document {
        use crate::document::Payload;
        let mut payload = IndexMap::new();
        for (k, v) in fm {
            payload.insert(k.to_string(), QuillValue::from_json(v.clone()));
        }
        let mut p = Payload::from_index_map(payload);
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(p, quillmark_content::Normalized::empty());
        Document::from_main_and_cards(main, cards)
    }

    fn typed_card(tag: &str, fields: &[(&str, serde_json::Value)]) -> Card {
        let mut card = Card::new(tag).unwrap();
        for (k, v) in fields {
            card.store_field(k, QuillValue::from_json(v.clone())).unwrap();
        }
        card
    }

    fn has_error<F>(errors: &[ValidationError], predicate: F) -> bool
    where
        F: Fn(&ValidationError) -> bool,
    {
        errors.iter().any(predicate)
    }

    #[test]
    fn validates_simple_string_field() {
        let config = config_with("    title:\n      type: string", "");
        let doc = doc_from_fm(&[("title", json!("Memo"))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_simple_string_type_mismatch() {
        let config = config_with("    title:\n      type: string\n      default: \"\"", "");
        let doc = doc_from_fm(&[("title", json!([1, 2, 3]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual, source_token, .. }
            if path == "main.title" && expected == "string" && actual == "array" && source_token == "[…]"
        )));
    }

    #[test]
    fn rejects_integer_field_with_decimal_value() {
        let config = config_with("    count:\n      type: integer\n      default: 0", "");
        let doc = doc_from_fm(&[("count", json!(9.5))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::TypeMismatch { path, expected, actual, source_token, .. }
            if path == "main.count" && expected == "integer" && actual == "number" && source_token == "9.5"
        )));
    }

    #[test]
    fn date_type_mismatch_names_date_not_string() {
        let config = config_with("    due:\n      type: date", "");
        let doc = doc_from_fm(&[("due", json!(20260101))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let err = errors
            .iter()
            .find(|e| matches!(e, ValidationError::TypeMismatch { path, .. } if path == "main.due"))
            .expect("expected a type mismatch on main.due");
        assert!(
            err.to_string().contains("schema declares `date`"),
            "message should name the date type, got: {err}"
        );
        assert_eq!(err.args().get("expected"), Some(&json!("date")));
    }

    #[test]
    fn enum_type_mismatch_names_enum_not_string() {
        let config = config_with("    lvl:\n      type: enum\n      values: [a, b]", "");
        let doc = doc_from_fm(&[("lvl", json!(["a", "b"]))]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        let err = errors
            .iter()
            .find(|e| matches!(e, ValidationError::TypeMismatch { path, .. } if path == "main.lvl"))
            .expect("expected a type mismatch on main.lvl");
        assert!(
            err.to_string().contains("schema declares `enum`"),
            "message should name the enum type, got: {err}"
        );
        assert_eq!(err.args().get("expected"), Some(&json!("enum")));
    }

    #[test]
    fn absent_defaultless_field_raises_nothing() {
        let config = config_with("    memo_for:\n      type: string", "");
        let doc = doc_from_fm(&[]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn present_null_is_treated_as_absent() {
        let config = config_with(
            "    memo_for:\n      type: string\n    n:\n      type: integer",
            "",
        );
        let doc = doc_from_fm(&[("memo_for", json!(null)), ("n", json!(null))]);
        assert!(
            validate_typed_document(&config, &doc).is_ok(),
            "present-null must validate like absence"
        );
    }

    #[test]
    fn absent_object_property_raises_nothing() {
        let config = config_with(
            "    recipients:\n      type: array\n      default: []\n      items:\n        type: object\n        properties:\n          name:\n            type: string\n          org:\n            type: string\n            default: \"\"",
            "",
        );
        let doc = doc_from_fm(&[("recipients", json!([{ "org": "HQ" }]))]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn validates_card_with_valid_discriminator() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![typed_card(
                "indorsement",
                &[("signature_block", json!("Signed"))],
            )],
        );
        assert!(validate_typed_document(&config, &doc).is_ok());
    }

    #[test]
    fn rejects_unknown_card_discriminator() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(&[], vec![typed_card("unknown", &[])]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::UnknownCard { path, card } if path == "cards[0]" && card == "unknown")
        }));
    }

    #[test]
    fn reports_card_field_paths_with_card_name_and_index() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  indorsement:\n    fields:\n      signature_block:\n        type: string",
        );
        let doc = doc_with_typed_cards(
            &[],
            vec![typed_card(
                "indorsement",
                &[("signature_block", json!([1, 2, 3]))],
            )],
        );
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| {
            matches!(e, ValidationError::TypeMismatch { path, .. } if path == "cards.indorsement[0].signature_block")
        }));
    }

    #[test]
    fn body_disabled_card_enforces_trim_boundary() {
        let config = config_with(
            "    title:\n      type: string\n      default: \"\"",
            "card_kinds:\n  skills:\n    body:\n      enabled: false\n    fields:\n      items:\n        type: array\n        items:\n          type: string\n        default: []",
        );
        let mut prose_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        prose_card.revise_body("Should not be here.").unwrap();
        let doc = doc_with_typed_cards(&[], vec![prose_card]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "skills" && path == "cards.skills[0].body"
        )));

        let mut ws_card = typed_card("skills", &[("items", json!(["Rust"]))]);
        ws_card.revise_body("\n   \n").unwrap();
        let ok_doc = doc_with_typed_cards(&[], vec![ws_card]);
        assert!(validate_typed_document(&config, &ok_doc).is_ok());
    }

    #[test]
    fn to_diagnostic_carries_path_code_and_hint() {
        let err = ValidationError::TypeMismatch {
            path: "cards.indorsement[0].signature_block".to_string(),
            expected: "string".to_string(),
            actual: "integer".to_string(),
            source_token: "42".to_string(),
            default: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("validation::type_mismatch"));
        assert_eq!(
            diag.path.as_deref(),
            Some("cards.indorsement[0].signature_block")
        );
        assert_eq!(diag.severity, Severity::Error);
        let hint = diag
            .hint
            .as_deref()
            .expect("type_mismatch diagnostic should carry a hint");
        assert!(
            hint.contains("string"),
            "hint missing expected type: {hint}"
        );
    }

    #[test]
    fn bare_scalar_into_string_field_is_valid() {
        for value in [json!(42), json!(true), json!(1.5)] {
            let config = config_with(
                "    build_number:\n      type: string\n      default: \"\"",
                "",
            );
            let doc = doc_from_fm(&[("build_number", value.clone())]);
            assert!(
                validate_typed_document(&config, &doc).is_ok(),
                "bare scalar {value} should validate as a string"
            );
        }
    }

    #[test]
    fn main_body_disabled_with_body_content_is_an_error() {
        let config = QuillConfig::from_yaml(
            r#"
quill:
  name: native_validation
  backend: typst
  description: Native validator tests
  version: 1.0.0
main:
  body:
    enabled: false
  fields:
    title:
      type: string
      default: ""
"#,
        )
        .unwrap();
        use crate::document::Payload;
        let mut p = Payload::from_index_map(IndexMap::new());
        p.set_quill("test_quill".parse().unwrap());
        p.set_kind("main");
        let main = Card::from_parts(
            p,
            crate::document::import_body("Body content that should not be here.").unwrap(),
        );
        let doc = Document::from_main_and_cards(main, vec![]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::BodyDisabled { path, card }
            if card == "main" && path == "main.body"
        )));
    }

    #[test]
    fn rejects_richtext_inline_with_multi_block_content() {
        let config = config_with("    tag:\n      type: richtext\n      inline: true", "");
        let rt = quillmark_content::import::from_markdown("one\n\ntwo").unwrap();
        let content = quillmark_content::serial::to_canonical_value(&rt);
        let doc = doc_from_fm(&[("tag", content)]);
        let errors = validate_typed_document(&config, &doc).unwrap_err();
        assert!(has_error(&errors, |e| matches!(
            e,
            ValidationError::NotInline { path } if path == "main.tag"
        )));
    }

    #[test]
    fn accepts_richtext_inline_single_para_content() {
        let config = config_with("    tag:\n      type: richtext\n      inline: true", "");
        let rt = quillmark_content::import::from_markdown("one line only").unwrap();
        let content = quillmark_content::serial::to_canonical_value(&rt);
        let doc = doc_from_fm(&[("tag", content)]);
        assert!(validate_typed_document(&config, &doc).is_ok());
    }
}
