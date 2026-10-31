//! `Form.yaml` wire types and parsing (`form@0.2.0`).
//!
//! `Form.yaml` is the durable, value-free placement + binding layer of a
//! `acroform` quill. It carries only what the quill schema cannot know: where a
//! widget sits (`page`/`rect`) and which logical field it binds. A bound field
//! in `fields` names a `schema_field` and derives its kind, options, multiline
//! and tooltip from the resolved schema, so the two cannot drift; a widget in
//! `widgets` has no schema field (a signer fills it) and declares its own
//! `type`. Unknown keys are ignored, so a later minor that only adds keys loads
//! on any Quillmark release reading its major.

use quillmark_core::error::{Diagnostic, Severity, YamlError};
use quillmark_core::version::Version;
use serde::Deserialize;

/// The field spec's file name at the quill root.
pub const FORM_YAML: &str = "Form.yaml";

/// The `schema` tag prefix every `Form.yaml` must carry.
pub const SCHEMA_PREFIX: &str = "quillmark/form@";

/// The oldest version read. Any later version under the same major is read too,
/// and a break moves the major.
const OLDEST_READ: Version = Version {
    major: 0,
    minor: 2,
    patch: 0,
};

/// A parsed `Form.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct FormSpec {
    /// Widgets bound to a [`FieldSchema`](quillmark_core::quill::FieldSchema).
    #[serde(default)]
    pub fields: Vec<BoundField>,
    #[serde(default)]
    pub widgets: Vec<UnboundWidget>,
}

/// Kind, options and multiline are derived from the referenced
/// [`FieldSchema`](quillmark_core::quill::FieldSchema) at load; `tooltip`
/// overrides that field's `description`. `rect` is top-left and page-relative,
/// flipped to the spine's bottom-left origin by the loader.
#[derive(Debug, Clone, Deserialize)]
pub struct BoundField {
    pub name: String,
    /// A path that does not resolve against the quill schema is a load error.
    pub schema_field: String,
    pub page: usize,
    pub rect: Rect,
    #[serde(default)]
    pub tooltip: Option<String>,
    /// The `enum` value that checks this widget, a box of its own: one per
    /// value lays out a group of boxes answering one question.
    #[serde(default)]
    pub option: Option<String>,
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

/// Internally tagged by `type` and flattened into the widget, so the mapping stays
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

/// Why a `Form.yaml` failed to parse.
#[derive(Debug)]
pub enum FormParseError {
    NotUtf8,
    /// The text is not YAML, or does not fit the spec's shape.
    Yaml(YamlError),
    /// The `schema` tag is not a recognized `quillmark/form@…` string.
    BadSchema(String),
    /// Two fields/widgets share a `name`. AcroForm top-level field names must be
    /// unique per form; colliding `/T`s render as one malformed field.
    DuplicateField(String),
}

impl std::fmt::Display for FormParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormParseError::NotUtf8 => write!(f, "{FORM_YAML} is not valid UTF-8"),
            FormParseError::Yaml(e) => write!(f, "{FORM_YAML} is not valid: {e}"),
            FormParseError::BadSchema(s) => write!(
                f,
                "{FORM_YAML} `schema` is {s:?}, expected a \"{SCHEMA_PREFIX}0.x\" tag, 0.2 or later"
            ),
            FormParseError::DuplicateField(name) => write!(
                f,
                "{FORM_YAML} declares field name {name:?} more than once; field names must be unique"
            ),
        }
    }
}

impl FormParseError {
    /// The stable diagnostic code for this error.
    pub fn code(&self) -> &'static str {
        "acroform::invalid_form_spec"
    }

    /// A YAML failure keeps its line and column as a `Location` in `Form.yaml`.
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            FormParseError::Yaml(e) => e.to_diagnostic(self.code(), FORM_YAML),
            _ => Diagnostic::new(Severity::Error, self.to_string())
                .with_code(self.code().to_string()),
        }
    }
}

/// Read ahead of the full spec, so an older tag gets the version error even
/// where its fields do not fit this shape.
#[derive(Debug, Deserialize)]
struct SchemaTag {
    schema: String,
}

impl FormSpec {
    /// Parse and validate the bytes of a `Form.yaml`.
    pub fn parse(bytes: &[u8]) -> Result<FormSpec, FormParseError> {
        let text = std::str::from_utf8(bytes).map_err(|_| FormParseError::NotUtf8)?;
        let tag: SchemaTag =
            quillmark_core::value::from_yaml_str(text).map_err(FormParseError::Yaml)?;
        check_version(&tag.schema)?;
        let spec: FormSpec =
            quillmark_core::value::from_yaml_str(text).map_err(FormParseError::Yaml)?;
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
        .and_then(|v| v.parse::<Version>().ok())
        .ok_or_else(|| FormParseError::BadSchema(schema.to_string()))?;
    if version.major == OLDEST_READ.major && version >= OLDEST_READ {
        Ok(())
    } else {
        Err(FormParseError::BadSchema(schema.to_string()))
    }
}

#[cfg(test)]
mod tests {
    mod properties;

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
                assert_eq!(e.code(), "acroform::invalid_form_spec");
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
    fn a_later_minor_is_read_and_a_later_major_is_not() {
        let tagged = |v: &str| format!(r#"{{ "schema": "quillmark/form@{v}", "fields": [] }}"#);
        for v in ["0.2", "0.2.15", "0.3.0", "0.20"] {
            assert!(FormSpec::parse(tagged(v).as_bytes()).is_ok(), "{v}");
        }
        for v in ["0.1.9", "1.0.0", "0.2.x", "0.2-rc"] {
            assert!(
                matches!(FormSpec::parse(tagged(v).as_bytes()), Err(FormParseError::BadSchema(_))),
                "{v}"
            );
        }
    }

    #[test]
    fn a_yaml_1_1_boolean_word_is_refused_as_a_boolean() {
        let multiline_yes = b"schema: quillmark/form@0.2.0
widgets:
  - { name: T, type: text, multiline: yes, page: 0, rect: { x: 0, y: 0, w: 1, h: 1 } }
";
        assert!(matches!(FormSpec::parse(multiline_yes), Err(FormParseError::Yaml(_))));
    }
}
