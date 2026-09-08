//! `form.json` wire types and parsing (`form@0.2.0`).
//!
//! `form.json` is the durable, value-free placement + binding layer of a
//! `pdfform` quill. It carries only what the quill schema cannot know: where a
//! widget sits (`page`/`rect`) and which logical field it binds. A bound field
//! in `fields` names a `schema_field` and derives its kind, options, multiline
//! and tooltip from the resolved schema, so the two cannot drift; a widget in
//! `widgets` has no schema field (a signer fills it) and declares its own
//! `type`. Unknown keys are ignored, so additive evolution needs no version bump.

use serde::Deserialize;

/// The `schema` tag prefix every `form.json` must carry.
pub const SCHEMA_PREFIX: &str = "quillmark/form@";

/// The `form.json` format version this backend reads.
pub const SCHEMA_VERSION: &str = "0.2.0";

/// The accepted major.minor; a matching patch is tolerated.
const SUPPORTED_MAJOR_MINOR: &str = "0.2";

/// A parsed `form.json`. The `schema` tag is version-gated separately
/// ([`SchemaTag`]) before this is deserialized, and ignored here.
#[derive(Debug, Clone, Deserialize)]
pub struct FormSpec {
    /// Widgets bound to a [`FieldSchema`](quillmark_core::FieldSchema).
    #[serde(default)]
    pub fields: Vec<BoundField>,
    #[serde(default)]
    pub widgets: Vec<UnboundWidget>,
}

/// Kind, options and multiline are derived from the referenced
/// [`FieldSchema`](quillmark_core::FieldSchema) at load; `tooltip` overrides
/// that field's `description`. `rect` is top-left and page-relative, flipped to
/// the spine's bottom-left origin by the loader.
#[derive(Debug, Clone, Deserialize)]
pub struct BoundField {
    pub name: String,
    /// A path that does not resolve against the quill schema is a load error.
    pub schema_field: String,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// A widget bound to no schema field, so its intrinsics are declared here.
#[derive(Debug, Clone, Deserialize)]
pub struct UnboundWidget {
    pub name: String,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(flatten)]
    pub kind: WidgetKind,
}

/// A top-left rectangle in PDF points (1/72").
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Internally tagged by `type` and flattened into the widget, so the JSON stays
/// flat while invalid combinations (a `signature` with `options`) cannot parse.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WidgetKind {
    Text {
        #[serde(default)]
        multiline: bool,
    },
    Checkbox,
    Choice {
        options: Vec<String>,
    },
    Signature,
}

/// Why a `form.json` failed to parse.
#[derive(Debug)]
pub enum FormParseError {
    /// The bytes were not valid JSON, or did not match the schema.
    Json(serde_json::Error),
    /// The `schema` tag is not a recognized `quillmark/form@…` string.
    BadSchema(String),
    /// Two fields/widgets share a `name`. AcroForm top-level field names must be
    /// unique per form; colliding `/T`s render as one malformed field.
    DuplicateField(String),
}

impl std::fmt::Display for FormParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormParseError::Json(e) => write!(f, "form.json is not valid: {e}"),
            FormParseError::BadSchema(s) => write!(
                f,
                "form.json `schema` is {s:?}, expected a \"{SCHEMA_PREFIX}{SCHEMA_VERSION}\" tag"
            ),
            FormParseError::DuplicateField(name) => write!(
                f,
                "form.json declares field name {name:?} more than once; field names must be unique"
            ),
        }
    }
}

impl FormParseError {
    /// The stable diagnostic code for this error.
    pub fn code(&self) -> &'static str {
        "pdfform::invalid_form_json"
    }
}

/// Deserialized ahead of the full spec so the version gate runs before
/// field-shape validation: a `form@0.1.0` file's fields do not deserialize into
/// a [`BoundField`], and it must get the version error, not a JSON one.
#[derive(Debug, Deserialize)]
struct SchemaTag {
    schema: String,
}

impl FormSpec {
    /// Parse and validate a `form.json` byte slice.
    pub fn parse(bytes: &[u8]) -> Result<FormSpec, FormParseError> {
        let tag: SchemaTag = serde_json::from_slice(bytes).map_err(FormParseError::Json)?;
        check_version(&tag.schema)?;
        let spec: FormSpec = serde_json::from_slice(bytes).map_err(FormParseError::Json)?;
        spec.check_unique_names()?;
        Ok(spec)
    }

    /// AcroForm `/T` names must be unique across *both* populations.
    fn check_unique_names(&self) -> Result<(), FormParseError> {
        let mut seen = std::collections::HashSet::new();
        for name in self.field_names() {
            if !seen.insert(name) {
                return Err(FormParseError::DuplicateField(name.to_string()));
            }
        }
        Ok(())
    }

    fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .map(|f| f.name.as_str())
            .chain(self.widgets.iter().map(|w| w.name.as_str()))
    }
}

fn check_version(schema: &str) -> Result<(), FormParseError> {
    let version = schema
        .strip_prefix(SCHEMA_PREFIX)
        .ok_or_else(|| FormParseError::BadSchema(schema.to_string()))?;
    if version_matches(version, SUPPORTED_MAJOR_MINOR) {
        Ok(())
    } else {
        Err(FormParseError::BadSchema(schema.to_string()))
    }
}

/// Matches `<major.minor>` exactly or with a `.patch` suffix, so `0.2` and
/// `0.2.7` match `"0.2"` but `0.20` does not.
fn version_matches(version: &str, major_minor: &str) -> bool {
    version == major_minor
        || version
            .strip_prefix(major_minor)
            .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bound_fields_and_unbound_widgets_ignoring_unknown_keys() {
        let json = br#"{
          "schema": "quillmark/form@0.2.0",
          "fields": [
            { "name": "FullName", "schema_field": "full_name", "page": 0,
              "rect": { "x": 180, "y": 57, "w": 340, "h": 20 },
              "tooltip": "Full legal name", "future_key": 42 },
            { "name": "Comments", "schema_field": "comments", "page": 0,
              "rect": { "x": 180, "y": 120, "w": 340, "h": 80 } }
          ],
          "widgets": [
            { "name": "Signature", "page": 0,
              "rect": { "x": 180, "y": 190, "w": 340, "h": 40 }, "type": "signature" }
          ]
        }"#;
        let spec = FormSpec::parse(json).expect("parse ok");
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].schema_field, "full_name");
        assert_eq!(spec.fields[0].tooltip.as_deref(), Some("Full legal name"));
        assert_eq!(spec.fields[1].tooltip, None);
        assert_eq!(spec.widgets.len(), 1);
        assert_eq!(spec.widgets[0].kind, WidgetKind::Signature);
    }

    #[test]
    fn v1_with_v1_shaped_fields_fails_the_version_gate_not_the_field_shape() {
        let json = br#"{
          "schema": "quillmark/form@0.1.0",
          "fields": [
            { "name": "FullName", "schema_field": "full_name", "page": 0,
              "rect": { "x": 0, "y": 0, "w": 1, "h": 1 }, "type": "text" },
            { "name": "Signature", "page": 0,
              "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "signature" }
          ]
        }"#;
        match FormSpec::parse(json) {
            Err(e @ FormParseError::BadSchema(_)) => {
                assert_eq!(e.code(), "pdfform::invalid_form_json");
            }
            other => panic!("expected BadSchema, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_schema_tag() {
        for json in [
            br#"{ "schema": "something/else@1", "fields": [] }"#.as_slice(),
            br#"{ "schema": "quillmark/form@9.9.9", "fields": [] }"#.as_slice(),
        ] {
            assert!(matches!(
                FormSpec::parse(json),
                Err(FormParseError::BadSchema(_))
            ));
        }
    }

    #[test]
    fn rejects_duplicate_names_across_populations() {
        let json = br#"{
          "schema": "quillmark/form@0.2.0",
          "fields": [
            { "name": "Dup", "schema_field": "a", "page": 0, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 } }
          ],
          "widgets": [
            { "name": "Dup", "page": 0, "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "type": "signature" }
          ]
        }"#;
        match FormSpec::parse(json) {
            Err(FormParseError::DuplicateField(name)) => assert_eq!(name, "Dup"),
            other => panic!("expected DuplicateField, got {other:?}"),
        }
    }

    #[test]
    fn version_matches_guards_adjacent_minors() {
        assert!(version_matches("0.2", "0.2"));
        assert!(version_matches("0.2.0", "0.2"));
        assert!(version_matches("0.2.15", "0.2"));
        assert!(!version_matches("0.20", "0.2"));
        assert!(!version_matches("0.21.0", "0.2"));
    }
}
