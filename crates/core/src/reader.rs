//! Schema-bound typed reader: the read twin of
//! [`TypedWriter`](crate::TypedWriter).
//!
//! The verbatim [`payload().get`](crate::Card::payload) is *transport*: the
//! stored value, schema-free and round-trippable. Projecting a field to markdown
//! is *interpretation*, a question a schema-free `Document` cannot answer
//! without guessing which fields are richtext, so the projection binds the
//! schema.
//!
//! Every read here answers in the **values form**: the stored value with each
//! content leaf decoded to its codec's text (`richtext` markdown, `plaintext`
//! literal), at every depth the field's type tree reaches, and everything else
//! verbatim. [`get`](TypedReader::get) reads one field in it and
//! [`values`](TypedReader::values) the whole document, so
//! `get(name)` is `values().fields[name]` on every field that decodes.
//! Scalars are never coerced by a read: `qty: "3"` reads `"3"` here and `3`
//! in [`resolve`](TypedReader::resolve), the render view.
//!
//! **Absence returns; mismatch raises; an unknown name is a typo**, as
//! [`TypedWriter::set`](crate::TypedWriter::set) does on the write side.
//!
//! [`get_content`](TypedReader::get_content) is the same read at the other end
//! of the codec, and is total over the storage form. It binds the quill too: a
//! `richtext` string is markdown and a `plaintext` string is literal text, so
//! the same bytes decode two ways. The body read stays quill-free — a body's
//! type is a format fact, not a schema fact.
//!
//! Like [`TypedWriter`](crate::TypedWriter), a bound reader holds `&Document`
//! and `&QuillConfig`, so it cannot cross a lifetime-free binding boundary;
//! those surfaces construct one per call from the quill handle.

use indexmap::IndexMap;
use quillmark_content::Normalized;

use crate::document::edit::field_decode;
use crate::document::{Card, Codec, Document, EditError};
use crate::quill::{
    card_fields, card_values, resolve_document, CardSchema, CardValues, DocumentValues,
    FieldSchema, FieldType, QuillConfig, Resolved,
};
use crate::value::{PathSegment, QuillValue};

/// A [`Document`] bound to its [`QuillConfig`] for typed reads. Construct with
/// [`Quill::reader`](crate::Quill::reader). Reads target the main card; use
/// [`card`](Self::card) for a composable card. The read twin of
/// [`TypedWriter`](crate::TypedWriter).
pub struct TypedReader<'a> {
    config: &'a QuillConfig,
    doc: &'a Document,
}

impl<'a> TypedReader<'a> {
    /// Bind `doc` to `config`. Prefer [`Quill::reader`](crate::Quill::reader).
    pub fn new(config: &'a QuillConfig, doc: &'a Document) -> Self {
        Self { config, doc }
    }

    /// Read a main-card field in the values form: every content leaf in its
    /// type tree as its codec's text (`richtext` markdown, `plaintext` literal),
    /// everything else as stored. A present-null field reads `Some(null)`.
    /// `Ok(None)` when the field is absent;
    /// [`EditError::UnknownField`] for a name the schema does not declare (a typo,
    /// as on the write side); [`EditError::FieldDecode`], anchored at the leaf,
    /// when a content leaf holds a value that does not decode (a scalar an
    /// opaque [`store_field`](crate::Card::store_field) wrote).
    pub fn get(&self, name: &str) -> Result<Option<QuillValue>, EditError> {
        read_field(self.doc.main(), Some(&self.config.main.fields), name)
    }

    /// Read a main-card content field as its [`Content`](quillmark_content::Content), decoded through the
    /// codec its declared type names: the [`Content`](quillmark_content::Content) twin of [`get`](Self::get).
    /// Total over the storage form, so a canonical content object and an
    /// authored string both read back as a [`Content`](quillmark_content::Content).
    ///
    /// `Ok(None)` when the field is absent;
    /// [`EditError::UnknownField`] for a name the schema does not declare;
    /// [`EditError::FieldNotContent`] for a declared type that is not a content
    /// leaf (an `integer` has no [`Content`](quillmark_content::Content) even when it holds a string, and an
    /// `array<richtext>` carries content without having one [`Content`](quillmark_content::Content));
    /// [`EditError::FieldDecode`] when the stored value decodes under
    /// neither encoding.
    pub fn get_content(&self, name: &str) -> Result<Option<Normalized>, EditError> {
        self.get_content_at(name, &[])
    }

    /// Read the [`Content`](quillmark_content::Content) *nested inside* a composite field at `at`: an
    /// `array<richtext>` element, an `object`'s content property, a leaf under
    /// both (`cells[1].notes`), or a variant's cell.
    /// [`get_content`](Self::get_content) is the empty path.
    ///
    /// The codec is the leaf's, resolved by the same schema walk `conform` and
    /// rest enforcement take, so a leaf reads back at the codec it was conformed
    /// at whatever its resting form.
    ///
    /// `Ok(None)` for an absent field **and for a path that names nothing in
    /// the stored value**: an editor's row index goes stale between derive and
    /// read, so absence on the axis a repeater mutates is a read, not a fault.
    /// A cell of a variant world that is not live reads the same way, the schema
    /// walk unioning the worlds. A bad *card* index still raises,
    /// [`card`](Self::card) being guarded by a count the caller holds.
    ///
    /// [`EditError::UnknownField`] for a name at any depth the schema does not
    /// declare, anchored at that name (`main.letterhead.nope`);
    /// [`EditError::FieldNotContent`] when `at` resolves to no content leaf;
    /// [`EditError::FieldDecode`], anchored at the addressed path, when the
    /// value there decodes under neither encoding.
    pub fn get_content_at(
        &self,
        name: &str,
        at: &[PathSegment],
    ) -> Result<Option<Normalized>, EditError> {
        read_content(self.doc.main(), Some(&self.config.main.fields), name, at)
    }

    /// The main body's markdown projection. Consults no schema and never
    /// raises; the body is never absent.
    pub fn body_markdown(&self) -> String {
        self.doc.main().body_markdown()
    }

    /// The whole document in the values form: the main card's fields, body
    /// and `$ext`, and every composable card, every axis filled
    /// ([`DocumentValues`]). Total: never raises, a content leaf that decodes
    /// under neither encoding riding out as stored, because the documents an
    /// ingestion most needs to read are the ones that arrived dirty. The read
    /// [`TypedWriter::set_values`](crate::TypedWriter::set_values) writes back.
    pub fn values(&self) -> DocumentValues {
        DocumentValues {
            fields: Some(card_fields(Some(&self.config.main), self.doc.main())),
            body: Some(self.doc.main().body_markdown()),
            cards: Some(
                self.doc
                    .cards()
                    .iter()
                    .map(|card| card_values(self.config, card))
                    .collect(),
            ),
            ext: Some(self.doc.main().ext().cloned()),
        }
    }

    /// The resolved view: for every declared field, the value the render
    /// projection would use and the rung it came from. The one read that
    /// blank-fills and coerces; see [`Quill::resolve`](crate::Quill::resolve).
    pub fn resolve(&self) -> Resolved {
        resolve_document(self.config, self.doc)
    }

    /// A schema-bound reader for the composable card at `index`. The card's
    /// `$kind` resolves its [`CardSchema`]; an unknown kind carries no schema, so
    /// every field name on it reads with [`EditError::UnknownField`] (read such
    /// a card verbatim through [`Card::payload`]).
    /// [`EditError::IndexOutOfRange`] when `index` is out of range.
    pub fn card(&self, index: usize) -> Result<CardReader<'_>, EditError> {
        let len = self.doc.cards().len();
        let card = self
            .doc
            .card(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        let schema = card.kind().and_then(|k| self.config.card_kind(k));
        Ok(CardReader {
            config: self.config,
            schema,
            card,
        })
    }
}

/// A single composable card bound to its [`CardSchema`], from
/// [`TypedReader::card`]. Same `get` / `values` / `body_markdown` verbs as
/// [`TypedReader`], reading the card at its bound index.
pub struct CardReader<'a> {
    config: &'a QuillConfig,
    schema: Option<&'a CardSchema>,
    card: &'a Card,
}

impl CardReader<'_> {
    /// The card's `$kind`, if any.
    pub fn kind(&self) -> Option<&str> {
        self.card.kind()
    }

    /// Read a field on this card in the values form: the card twin of
    /// [`TypedReader::get`]. Resolves the field against the card's
    /// [`CardSchema`]; a name the schema does not declare (or any name when the
    /// card kind is unknown) reads with [`EditError::UnknownField`].
    pub fn get(&self, name: &str) -> Result<Option<QuillValue>, EditError> {
        read_field(self.card, self.schema.map(|s| &s.fields), name)
    }

    /// Read a content field on this card as its [`Content`](quillmark_content::Content): the card twin
    /// of [`TypedReader::get_content`], carrying the same outcomes.
    pub fn get_content(&self, name: &str) -> Result<Option<Normalized>, EditError> {
        self.get_content_at(name, &[])
    }

    /// The card twin of [`TypedReader::get_content_at`], carrying the same
    /// outcomes.
    pub fn get_content_at(
        &self,
        name: &str,
        at: &[PathSegment],
    ) -> Result<Option<Normalized>, EditError> {
        read_content(self.card, self.schema.map(|s| &s.fields), name, at)
    }

    /// This card's body markdown: the card twin of [`TypedReader::body_markdown`].
    pub fn body_markdown(&self) -> String {
        self.card.body_markdown()
    }

    /// This card in the values form: [`TypedReader::values`] restricted to one
    /// slot, every axis filled.
    pub fn values(&self) -> CardValues {
        card_values(self.config, self.card)
    }
}

/// The shared read dispatch behind [`TypedReader::get`] and [`CardReader::get`].
/// A `None` schema is an unknown card kind: every name on it is undeclared.
fn read_field(
    card: &Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
    name: &str,
) -> Result<Option<QuillValue>, EditError> {
    let schema = fields_schema
        .and_then(|m| m.get(name))
        .ok_or_else(|| EditError::unknown_field(name))?;
    let Some(value) = card.payload().get(name) else {
        return Ok(None);
    };
    let projected = project_value(
        name,
        value.as_json(),
        schema,
        &mut Vec::new(),
        ProjectMode::Strict,
    )?;
    Ok(Some(QuillValue::from_json(projected)))
}

/// The shared [`Content`](quillmark_content::Content) dispatch behind every `get_content` /
/// `get_content_at`, the whole-field read being the empty `at`.
/// [`EditError::FieldNotContent`] is answered from the schema before the payload
/// is read: whether an address has a [`Content`](quillmark_content::Content) is a declared-type fact, so a
/// `string` holding markdown-looking text is not content, and an
/// `array<richtext>` has no single [`Content`](quillmark_content::Content) while each of its elements does.
fn read_content(
    card: &Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
    name: &str,
    at: &[PathSegment],
) -> Result<Option<Normalized>, EditError> {
    let field = fields_schema
        .and_then(|m| m.get(name))
        .ok_or_else(|| EditError::unknown_field(name))?;
    let leaf = schema_at(field, name, at)?;
    // The codec rides out of the dispatch: it is the declared type's, not the
    // stored shape's.
    let codec = content_codec(&leaf.r#type).ok_or_else(|| EditError::FieldNotContent {
        field: name.to_string(),
        at: at.to_vec(),
        declared: leaf.r#type.as_str().to_string(),
    })?;
    let Some(value) = card.payload().get(name).and_then(|v| value_at(v.as_json(), at)) else {
        return Ok(None);
    };
    codec
        .decode_field(value)
        .map(Some)
        .map_err(|e| field_decode(name, at, codec, e))
}

/// What a content leaf that decodes under neither encoding does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectMode {
    /// Raise [`EditError::FieldDecode`] anchored at the leaf.
    Strict,
    /// Pass the value verbatim.
    Total,
}

/// Project `value` through `schema`'s type tree into the values form: every
/// content leaf to its codec's text, every other node verbatim, descending
/// `items` / `properties` / `variants`. `at` is the path from the field to
/// `value`, extended on descent and read only to anchor a
/// [`Strict`](ProjectMode::Strict) failure.
///
/// The descent is over the schema, so a node whose shape the schema cannot take
/// stops there and passes verbatim: a value is never reshaped to match a
/// declaration it does not fit. A null rides as null at every type, so the form
/// keeps present-null apart from authored-empty; blank-filling is the render
/// view's.
pub(crate) fn project_value(
    name: &str,
    value: &serde_json::Value,
    schema: &FieldSchema,
    at: &mut Vec<PathSegment>,
    mode: ProjectMode,
) -> Result<serde_json::Value, EditError> {
    if value.is_null() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(codec) = content_codec(&schema.r#type) {
        return match codec.decode_field(value) {
            Ok(content) => Ok(serde_json::Value::String(codec.project(&content))),
            Err(e) => match mode {
                ProjectMode::Strict => Err(field_decode(name, at, codec, e)),
                ProjectMode::Total => Ok(value.clone()),
            },
        };
    }
    match (&schema.r#type, value) {
        (FieldType::Array, serde_json::Value::Array(items)) => {
            let Some(item_schema) = schema.items.as_deref() else {
                return Ok(value.clone());
            };
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                at.push(PathSegment::Index(index));
                let projected = project_value(name, item, item_schema, at, mode);
                at.pop();
                out.push(projected?);
            }
            Ok(serde_json::Value::Array(out))
        }
        (FieldType::Object, serde_json::Value::Object(map)) => {
            let props = schema.properties.as_ref();
            project_map(name, map, at, mode, |key| {
                props.and_then(|p| p.get(key)).map(|s| &**s)
            })
        }
        // Which world is live is a value-time fact, so the walk unions the
        // worlds: a cell of a dormant world projects at its own codec, as the
        // document carries it. The discriminant declares no cell and rides
        // verbatim.
        (FieldType::Enum, serde_json::Value::Object(map)) if schema.is_variant_bearing() => {
            project_map(name, map, at, mode, |key| schema.variant_field(key))
        }
        _ => Ok(value.clone()),
    }
}

/// The shared object arm of [`project_value`]: every entry projected through the
/// schema `cell` resolves for its key, and verbatim where it resolves none (the
/// schema is a floor, not an allowlist).
fn project_map<'a>(
    name: &str,
    map: &serde_json::Map<String, serde_json::Value>,
    at: &mut Vec<PathSegment>,
    mode: ProjectMode,
    cell: impl Fn(&str) -> Option<&'a FieldSchema>,
) -> Result<serde_json::Value, EditError> {
    let mut out = serde_json::Map::with_capacity(map.len());
    for (key, child) in map {
        let projected = match cell(key) {
            None => child.clone(),
            Some(child_schema) => {
                at.push(PathSegment::Key(key.clone()));
                let projected = project_value(name, child, child_schema, at, mode);
                at.pop();
                projected?
            }
        };
        out.insert(key.clone(), projected);
    }
    Ok(serde_json::Value::Object(out))
}

/// The one declared-type → codec dispatch: `None` for a type that is no content
/// leaf. Every schema-bound content read routes through this — the whole field,
/// a nested leaf, and [`get`](TypedReader::get)'s text projection alike — so a
/// codec change reaches all three by construction.
fn content_codec(r#type: &FieldType) -> Option<Codec> {
    match r#type {
        FieldType::RichText { .. } => Some(Codec::Richtext),
        FieldType::PlainText { .. } => Some(Codec::Plaintext),
        _ => None,
    }
}

/// Walk `at` through a field's schema to the type declared at that address. A
/// step the schema cannot take is [`EditError::FieldNotContent`] naming the type
/// that blocked it; a property an `object` does not declare is the same
/// [`EditError::UnknownField`] an undeclared field name is, one level down.
fn schema_at<'a>(
    field: &'a FieldSchema,
    name: &str,
    at: &[PathSegment],
) -> Result<&'a FieldSchema, EditError> {
    let mut cursor = field;
    for (depth, seg) in at.iter().enumerate() {
        let blocked = EditError::FieldNotContent {
            field: name.to_string(),
            at: at[..depth].to_vec(),
            declared: cursor.r#type.as_str().to_string(),
        };
        cursor = match (&cursor.r#type, seg) {
            (FieldType::Array, PathSegment::Index(_)) => cursor.items.as_deref().ok_or(blocked)?,
            (FieldType::Object, PathSegment::Key(key)) => match cursor.properties.as_ref() {
                None => return Err(blocked),
                Some(props) => props.get(key).ok_or_else(|| EditError::UnknownField {
                    field: name.to_string(),
                    // Through the failed step, not up to it: the anchor names the
                    // undeclared property, not the object holding it.
                    at: at[..=depth].to_vec(),
                })?,
            },
            // Which world is live is a value-time fact, so the walk unions the
            // worlds: a dormant cell resolves here and reads absent at
            // `value_at`. The guard holds a variantless enum to a scalar, which
            // `variant_field` alone would answer as an unknown cell.
            (FieldType::Enum, PathSegment::Key(key)) if cursor.is_variant_bearing() => cursor
                .variant_field(key)
                .ok_or_else(|| EditError::UnknownField {
                    field: name.to_string(),
                    at: at[..=depth].to_vec(),
                })?,
            _ => return Err(blocked),
        };
    }
    Ok(cursor)
}

/// Walk `at` through a stored value. `None` when the path names nothing there,
/// which the read reports as absence rather than as a fault.
fn value_at<'a>(value: &'a serde_json::Value, at: &[PathSegment]) -> Option<&'a serde_json::Value> {
    let mut cursor = value;
    for seg in at {
        cursor = match seg {
            PathSegment::Key(key) => cursor.get(key)?,
            PathSegment::Index(index) => cursor.get(index)?,
        };
    }
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::edit::CODEC_RICHTEXT;
    use crate::document::Document;
    use crate::version::QuillReference;
    use std::str::FromStr;

    const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Reader test quill
main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
    qty:
      type: integer
    recipients:
      type: array
      items:
        type: plaintext
    paragraphs:
      type: array
      items:
        type: richtext
    tags:
      type: array
      items:
        type: string
    letterhead:
      type: object
      properties:
        motto:
          type: richtext
        code:
          type: string
    rows:
      type: array
      items:
        type: object
        properties:
          notes:
            type: richtext
    plain_enum:
      type: enum
      values: [a, b]
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      variants:
        CUI:
          controlled_by:
            type: plaintext
          banner:
            type: richtext
          count:
            type: integer
          nest:
            type: object
            properties:
              deep:
                type: richtext
card_kinds:
  note:
    fields:
      body:
        type: richtext
      lines:
        type: array
        items:
          type: plaintext
";

    fn config() -> QuillConfig {
        QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
    }

    fn blank_doc() -> Document {
        Document::new(QuillReference::from_str("memo@1.0.0").unwrap())
    }

    fn seeded_doc(config: &QuillConfig) -> Document {
        let mut doc = blank_doc();
        {
            let mut w = crate::TypedWriter::new(config, &mut doc);
            w.set("subject", "Hello **world**").unwrap();
            w.set("qty", "3").unwrap();
            w.add_card("note", [("body", "a *card*")], None, None).unwrap();
        }
        doc
    }

    fn json(value: serde_json::Value) -> Option<QuillValue> {
        Some(QuillValue::from_json(value))
    }

    #[test]
    fn richtext_field_projects_to_markdown() {
        let config = config();
        let doc = seeded_doc(&config);
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.get("subject").unwrap(),
            json(serde_json::json!("Hello **world**"))
        );
    }

    #[test]
    fn scalar_field_reads_as_stored() {
        let config = config();
        let doc = seeded_doc(&config);
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get("qty").unwrap(), json(serde_json::json!(3)));

        let mut authored = blank_doc();
        authored
            .main_mut()
            .store_field("qty", QuillValue::from_json(serde_json::json!("3")))
            .unwrap();
        assert_eq!(
            TypedReader::new(&config, &authored).get("qty").unwrap(),
            json(serde_json::json!("3")),
            "a read never coerces: the shorthand stays until a write canonicalizes it"
        );
    }

    #[test]
    fn present_null_reads_as_null_at_every_type() {
        let config = config();
        let mut doc = blank_doc();
        for name in ["subject", "note", "qty", "paragraphs"] {
            doc.main_mut()
                .store_field(name, QuillValue::from_json(serde_json::Value::Null))
                .unwrap();
        }
        let view = TypedReader::new(&config, &doc);
        for name in ["subject", "note", "qty", "paragraphs"] {
            assert_eq!(
                view.get(name).unwrap(),
                json(serde_json::Value::Null),
                "`{name}`: present-null is not authored-empty"
            );
        }
    }

    #[test]
    fn absent_field_returns_none() {
        let config = config();
        let doc = blank_doc();
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get("subject").unwrap(), None);
        assert_eq!(view.get("qty").unwrap(), None);
    }

    #[test]
    fn unknown_field_name_raises() {
        let config = config();
        let doc = blank_doc();
        let view = TypedReader::new(&config, &doc);
        assert!(matches!(
            view.get("nope"),
            Err(EditError::UnknownField { field, .. }) if field == "nope"
        ));
    }

    #[test]
    fn richtext_field_holding_scalar_raises_mismatch() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field("subject", QuillValue::from_json(serde_json::json!(3)))
            .unwrap();
        let err = TypedReader::new(&config, &doc).get("subject").unwrap_err();
        assert!(
            matches!(&err, EditError::FieldDecode { field, message, .. }
                if field == "subject"
                    && message
                        == "expected a richtext content object or a markdown string, got a number"),
            "got {err:?}"
        );
    }

    #[test]
    fn parse_lane_plaintext_reads_through_the_literal_codec() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field(
                "note",
                QuillValue::from_json(serde_json::json!("a *literal* line")),
            )
            .unwrap();
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get("note").unwrap(), json(serde_json::json!("a *literal* line")));
    }

    #[test]
    fn content_read_spans_both_storage_forms() {
        let config = config();
        let committed = seeded_doc(&config);
        let mut authored = blank_doc();
        authored
            .main_mut()
            .store_field(
                "subject",
                QuillValue::from_json(serde_json::json!("Hello **world**")),
            )
            .unwrap();

        let from_content = TypedReader::new(&config, &committed)
            .get_content("subject")
            .unwrap()
            .unwrap();
        let from_string = TypedReader::new(&config, &authored)
            .get_content("subject")
            .unwrap()
            .unwrap();
        assert_eq!(from_content.text, "Hello world");
        assert_eq!(from_content.text, from_string.text);
        assert_eq!(from_content.marks, from_string.marks);
    }

    #[test]
    fn content_read_decodes_by_declared_type() {
        let config = config();
        let mut doc = blank_doc();
        let text = serde_json::json!("a *literal* line");
        {
            let card = doc.main_mut();
            card.store_field("subject", QuillValue::from_json(text.clone())).unwrap();
            card.store_field("note", QuillValue::from_json(text)).unwrap();
        }
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get_content("subject").unwrap().unwrap().text, "a literal line");
        assert_eq!(view.get_content("note").unwrap().unwrap().text, "a *literal* line");
    }

    #[test]
    fn content_read_absent_unknown_and_non_content() {
        let config = config();
        let doc = seeded_doc(&config);
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get_content("note").unwrap(), None);
        assert!(matches!(
            view.get_content("nope"),
            Err(EditError::UnknownField { field, .. }) if field == "nope"
        ));
        // Answered from the schema, not the payload: `qty` holds 3.
        assert!(matches!(
            view.get_content("qty"),
            Err(EditError::FieldNotContent { field, declared, .. })
                if field == "qty" && declared == "integer"
        ));
    }

    #[test]
    fn content_read_undecodable_value_raises() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field("subject", QuillValue::from_json(serde_json::json!(3)))
            .unwrap();
        let view = TypedReader::new(&config, &doc);
        assert!(matches!(
            view.get_content("subject"),
            Err(EditError::FieldDecode { field, .. }) if field == "subject"
        ));
    }

    fn idx(i: usize) -> Vec<PathSegment> {
        vec![PathSegment::Index(i)]
    }

    fn key(k: &str) -> Vec<PathSegment> {
        vec![PathSegment::Key(k.to_string())]
    }

    #[test]
    fn element_read_spans_both_storage_forms() {
        let config = config();
        let mut doc = blank_doc();
        {
            let card = doc.main_mut();
            card.store_field(
                "recipients",
                QuillValue::from_json(serde_json::json!(["a *literal* line"])),
            )
            .unwrap();
            card.store_field(
                "paragraphs",
                QuillValue::from_json(serde_json::json!(["Hello **world**"])),
            )
            .unwrap();
        }
        let mut committed = blank_doc();
        {
            let mut w = crate::TypedWriter::new(&config, &mut committed);
            w.set("recipients", serde_json::json!(["a *literal* line"])).unwrap();
            w.set("paragraphs", serde_json::json!(["Hello **world**"])).unwrap();
        }

        for (field, text) in [
            ("recipients", "a *literal* line"),
            ("paragraphs", "Hello world"),
        ] {
            let authored = TypedReader::new(&config, &doc)
                .get_content_at(field, &idx(0))
                .unwrap()
                .unwrap();
            let rested = TypedReader::new(&config, &committed)
                .get_content_at(field, &idx(0))
                .unwrap()
                .unwrap();
            assert_eq!(authored.text, text, "{field} decodes at its declared codec");
            assert_eq!(authored, rested, "{field} reads the same from either rest");
        }
    }

    #[test]
    fn element_read_reaches_object_and_nested_shapes() {
        let config = config();
        let mut doc = blank_doc();
        {
            let card = doc.main_mut();
            card.store_field(
                "letterhead",
                QuillValue::from_json(serde_json::json!({"motto": "Fly **fight**", "code": "9"})),
            )
            .unwrap();
            card.store_field(
                "rows",
                QuillValue::from_json(serde_json::json!([{}, {"notes": "a *note*"}])),
            )
            .unwrap();
        }
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.get_content_at("letterhead", &key("motto")).unwrap().unwrap().text,
            "Fly fight"
        );
        assert_eq!(
            view.get_content_at(
                "rows",
                &[PathSegment::Index(1), PathSegment::Key("notes".to_string())]
            )
            .unwrap()
            .unwrap()
            .text,
            "a note"
        );
        assert_eq!(
            view.get_content_at(
                "rows",
                &[PathSegment::Index(0), PathSegment::Key("notes".to_string())]
            )
            .unwrap(),
            None
        );
    }

    fn cui_doc() -> Document {
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field(
                "classification",
                QuillValue::from_json(serde_json::json!({
                    "value": "CUI",
                    "controlled_by": "a *literal* line",
                    "banner": "Hello **world**",
                    "count": 3,
                    "nest": {"deep": "a *note*"},
                })),
            )
            .unwrap();
        doc
    }

    #[test]
    fn variant_cell_reads_at_its_declared_codec() {
        let config = config();
        let doc = cui_doc();
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.get_content_at("classification", &key("controlled_by")).unwrap().unwrap().text,
            "a *literal* line",
            "a plaintext cell decodes literally, as a card-level one does"
        );
        assert_eq!(
            view.get_content_at("classification", &key("banner")).unwrap().unwrap().text,
            "Hello world"
        );
        assert_eq!(
            view.get_content_at(
                "classification",
                &[PathSegment::Key("nest".into()), PathSegment::Key("deep".into())]
            )
            .unwrap()
            .unwrap()
            .text,
            "a note",
            "the walk continues past the cell into its own subtree"
        );
    }

    #[test]
    fn a_dormant_worlds_cell_reads_absent() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field(
                "classification",
                QuillValue::from_json(serde_json::json!("UNCLASSIFIED")),
            )
            .unwrap();
        assert_eq!(
            TypedReader::new(&config, &doc)
                .get_content_at("classification", &key("controlled_by"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn a_variant_step_the_schema_cannot_take_is_blocked() {
        let config = config();
        let doc = cui_doc();
        let view = TypedReader::new(&config, &doc);
        assert!(matches!(
            view.get_content_at("classification", &key("count")),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "integer"
        ));
        assert!(
            matches!(
                view.get_content_at("plain_enum", &key("a")),
                Err(EditError::FieldNotContent { declared, .. }) if declared == "enum"
            ),
            "a variantless enum is a scalar, not a container"
        );
        assert!(matches!(
            view.get_content_at("classification", &idx(0)),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "enum"
        ));
        assert!(matches!(
            view.get_content_at("classification", &[]),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "enum"
        ));
    }

    #[test]
    fn a_cell_no_world_declares_is_unknown_field() {
        let config = config();
        let doc = cui_doc();
        let view = TypedReader::new(&config, &doc);
        for name in ["nope", "value"] {
            assert!(
                matches!(
                    view.get_content_at("classification", &key(name)),
                    Err(EditError::UnknownField { at, .. }) if at == key(name)
                ),
                "`{name}` anchors through the failed step"
            );
        }
    }

    #[test]
    fn empty_path_is_the_whole_field_read() {
        let config = config();
        let doc = seeded_doc(&config);
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.get_content_at("subject", &[]).unwrap(),
            view.get_content("subject").unwrap()
        );
        assert!(matches!(
            view.get_content_at("qty", &[]),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "integer"
        ));
    }

    #[test]
    fn element_read_out_of_range_and_absent_field_return_none() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field("recipients", QuillValue::from_json(serde_json::json!(["a"])))
            .unwrap();
        let view = TypedReader::new(&config, &doc);
        assert_eq!(view.get_content_at("recipients", &idx(7)).unwrap(), None);
        assert_eq!(view.get_content_at("paragraphs", &idx(0)).unwrap(), None);
        assert_eq!(view.get_content_at("letterhead", &key("motto")).unwrap(), None);
    }

    #[test]
    fn element_read_without_a_content_leaf_raises_not_content() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field("tags", QuillValue::from_json(serde_json::json!(["x"])))
            .unwrap();
        let view = TypedReader::new(&config, &doc);
        assert!(matches!(
            view.get_content_at("tags", &idx(0)),
            Err(EditError::FieldNotContent { field, declared, .. })
                if field == "tags" && declared == "string"
        ));
        assert!(matches!(
            view.get_content_at("qty", &idx(0)),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "integer"
        ));
        assert!(matches!(
            view.get_content_at("recipients", &key("motto")),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "array"
        ));
        assert!(matches!(
            view.get_content_at("recipients", &[]),
            Err(EditError::FieldNotContent { declared, .. }) if declared == "array"
        ));
    }

    #[test]
    fn unknown_property_anchors_at_the_property_it_names() {
        let config = config();
        let doc = blank_doc();
        let view = TypedReader::new(&config, &doc);

        let err = view.get_content_at("letterhead", &key("nope")).unwrap_err();
        assert!(
            matches!(&err, EditError::UnknownField { field, at }
                if field == "letterhead" && at == &key("nope")),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "field 'letterhead.nope' is not declared in the schema",
            "a property is not a claim about a top-level field of the same name"
        );
        assert_eq!(
            err.doc_path(&crate::DocPath::main()).unwrap().to_string(),
            "main.letterhead.nope"
        );

        let deep = view
            .get_content_at("rows", &[PathSegment::Index(0), PathSegment::Key("nope".into())])
            .unwrap_err();
        assert_eq!(
            deep.doc_path(&crate::DocPath::main()).unwrap().to_string(),
            "main.rows[0].nope"
        );
    }

    #[test]
    fn element_decode_failure_anchors_at_the_element() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field(
                "paragraphs",
                QuillValue::from_json(serde_json::json!(["ok", 3])),
            )
            .unwrap();
        let err = TypedReader::new(&config, &doc)
            .get_content_at("paragraphs", &idx(1))
            .unwrap_err();
        assert!(matches!(
            &err,
            EditError::FieldDecode { field, codec, .. }
                if field == "paragraphs" && codec == CODEC_RICHTEXT
        ));
        let path = err.doc_path(&crate::DocPath::main()).unwrap();
        assert_eq!(path.to_string(), "main.paragraphs[1]");
        assert_eq!(
            crate::DocPath::from_str("main.paragraphs[1]").unwrap(),
            path,
            "the anchor round-trips as segments, not as a bracketed name"
        );
    }

    #[test]
    fn card_element_read_reaches_the_kind_schema() {
        let config = config();
        let mut doc = seeded_doc(&config);
        doc.card_mut(0)
            .unwrap()
            .store_field("lines", QuillValue::from_json(serde_json::json!(["a *b*"])))
            .unwrap();
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.card(0).unwrap().get_content_at("lines", &idx(0)).unwrap().unwrap().text,
            "a *b*"
        );
        assert_eq!(view.card(0).unwrap().get_content_at("lines", &idx(4)).unwrap(), None);
    }

    #[test]
    fn card_field_reads_through_kind_schema() {
        let config = config();
        let doc = seeded_doc(&config);
        let view = TypedReader::new(&config, &doc);
        let card = view.card(0).unwrap();
        assert_eq!(card.kind(), Some("note"));
        assert_eq!(card.get("body").unwrap(), json(serde_json::json!("a *card*")));
        assert!(matches!(card.get("nope"), Err(EditError::UnknownField { .. })));
    }

    #[test]
    fn a_composite_reads_its_content_leaves_as_text() {
        let config = config();
        let mut doc = blank_doc();
        {
            let mut w = crate::TypedWriter::new(&config, &mut doc);
            w.set("paragraphs", serde_json::json!(["Para **one**"])).unwrap();
            w.set("recipients", serde_json::json!(["a *literal* line"])).unwrap();
            w.set("letterhead", serde_json::json!({"motto": "Fly **fight**", "code": "9"}))
                .unwrap();
        }
        let view = TypedReader::new(&config, &doc);
        assert_eq!(
            view.get("paragraphs").unwrap(),
            json(serde_json::json!(["Para **one**"])),
        );
        assert_eq!(
            view.get("recipients").unwrap(),
            json(serde_json::json!(["a *literal* line"])),
            "each element decodes at its own declared codec"
        );
        assert_eq!(
            view.get("letterhead").unwrap(),
            json(serde_json::json!({"motto": "Fly **fight**", "code": "9"})),
            "a content property projects; a scalar sibling rides verbatim"
        );
    }

    #[test]
    fn a_content_free_composite_reads_verbatim() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field("tags", QuillValue::from_json(serde_json::json!(["x", "y"])))
            .unwrap();
        assert_eq!(
            TypedReader::new(&config, &doc).get("tags").unwrap(),
            json(serde_json::json!(["x", "y"]))
        );
    }

    #[test]
    fn a_composite_leaf_that_does_not_decode_raises_at_the_leaf() {
        let config = config();
        let mut doc = blank_doc();
        doc.main_mut()
            .store_field(
                "paragraphs",
                QuillValue::from_json(serde_json::json!(["ok", 3])),
            )
            .unwrap();
        let err = TypedReader::new(&config, &doc).get("paragraphs").unwrap_err();
        assert_eq!(
            err.doc_path(&crate::DocPath::main()).unwrap().to_string(),
            "main.paragraphs[1]",
            "the strict read anchors at the element, as get_content_at does"
        );
    }

    #[test]
    fn card_out_of_range_raises() {
        let config = config();
        let doc = blank_doc();
        let view = TypedReader::new(&config, &doc);
        assert!(matches!(
            view.card(9),
            Err(EditError::IndexOutOfRange { index: 9, len: 0 })
        ));
    }

}
