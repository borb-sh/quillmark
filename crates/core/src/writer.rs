//! Schema-bound typed writer: the front door for typed field writes.
//!
//! `Card::commit_field` asks the caller to fetch a [`FieldSchema`] per write.
//! [`Quill::writer`](crate::Quill::writer) binds the schema once instead, so
//! callers issue one verb (`set`) and never pass a type token or an `inline`
//! flag. An undeclared name is [`EditError::UnknownField`]: on the typed path
//! it is a typo, not a fallback. Opaque storage stays available through the raw
//! [`Card::store_field`](crate::Card::store_field) verb.
//!
//! ```ignore
//! let mut w = quill.writer(&mut doc);
//! w.set("subject", "Q3 results")?;       // richtext(inline) → strict content commit
//! w.set("qty", "3")?;                    // integer → strict coerce, stores 3
//! w.card(2)?.set("desc", content_json)?;  // card kind → CardSchema → field type
//! w.set_all([("a", "1"), ("b", "2")])?;  // batched, all-or-nothing, a merge
//! ```
//!
//! The writer holds `&mut Document` and `&QuillConfig`, so a bound `TypedWriter`
//! cannot cross a lifetime-free binding boundary; those surfaces construct one
//! per call from the quill handle.
//!
//! [`Quill::conform`](crate::Quill::conform) is this same strict commit driven
//! by the schema rather than by a caller, so what an ingestion lands and what a
//! write lands are the same bytes. Where a write refuses, conform leaves the
//! value authored under a `conform::*` warning.

use indexmap::IndexMap;

use crate::document::edit::resolve_field_write;
use crate::document::{Card, Document, EditError};
use crate::quill::{FieldSchema, QuillConfig};
use crate::value::QuillValue;
use crate::Delta;

/// A [`Document`] bound to its [`QuillConfig`] for typed writes. Construct with
/// [`Quill::writer`](crate::Quill::writer). Writes target the main card; use
/// [`card`](Self::card) for a composable card.
pub struct TypedWriter<'a> {
    config: &'a QuillConfig,
    doc: &'a mut Document,
}

impl<'a> TypedWriter<'a> {
    /// Bind `doc` to `config`. Prefer [`Quill::writer`](crate::Quill::writer).
    pub fn new(config: &'a QuillConfig, doc: &'a mut Document) -> Self {
        Self { config, doc }
    }

    /// Write a field on the main card, strict-committing it against the field's
    /// schema type. An undeclared name fails with [`EditError::UnknownField`]
    /// rather than falling to the opaque
    /// [`Card::store_field`](crate::Card::store_field). Other errors are those
    /// of `Card::commit_field`.
    pub fn set(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let schema = Some(&self.config.main.fields);
        commit_impl(self.doc.main_mut(), schema, name, value)
    }

    /// Write several main-card fields atomically, the typed twin of
    /// [`Card::store_fields`](crate::Card::store_fields). A merge: fields the
    /// batch does not name are untouched. Every field resolves before any is
    /// applied; on a violation nothing is written and every offending field
    /// comes back as a `(name, error)` pair, so a caller submitting a whole form
    /// sees every typo in one pass.
    pub fn set_all<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let schema = Some(&self.config.main.fields);
        set_all_impl(self.doc.main_mut(), schema, fields)
    }

    /// Revise the main card's body from markdown: edit semantics, surviving
    /// anchors rebase, text [`Delta`] returned. Untyped, because a body carries
    /// no field schema to type against.
    pub fn revise_body(&mut self, markdown: &str) -> Result<Delta, EditError> {
        self.doc.main_mut().revise_body(markdown)
    }

    /// Revise a content field on the main card from authored text: typed *and*
    /// anchor-preserving. Surviving anchors rebase and the diffed result is
    /// schema-conformed (`richtext(inline)` rejects a multi-block result).
    /// An undeclared name fails with [`EditError::UnknownField`], as
    /// [`set`](Self::set).
    ///
    /// The codec comes from the declared type: `richtext` diffs markdown and
    /// rebases anchors; `plaintext` diffs the literal text and never imports
    /// markdown, so a byte-identical revise of a value carrying escapes is a
    /// byte no-op.
    pub fn revise_field(&mut self, name: &str, text: &str) -> Result<Delta, EditError> {
        let schema = Some(&self.config.main.fields);
        revise_impl(self.doc.main_mut(), schema, name, text)
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it,
    /// optionally set its body from markdown, and place it. `at` picks the
    /// position: `None` appends, `Some(i)` inserts at index `i`. The card is
    /// committed in full *before* it joins the document, so a rejected field
    /// (or an invalid kind, body, or out-of-range `at`) leaves the document
    /// untouched. Field errors use the all-or-nothing bundle of
    /// [`set_all`](Self::set_all); an invalid kind or body, or an out-of-range
    /// position, surfaces as a single-entry bundle keyed `$kind` / `$body`.
    pub fn add_card<K, V, I>(
        &mut self,
        kind: &str,
        fields: I,
        body: Option<&str>,
        at: Option<usize>,
    ) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut card = Card::new(kind).map_err(|e| vec![("$kind".to_string(), e)])?;
        let schema = self.config.card_kind(kind).map(|s| &s.fields);
        set_all_impl(&mut card, schema, fields)?;
        if let Some(md) = body {
            card.revise_body(md)
                .map_err(|e| vec![("$body".to_string(), e)])?;
        }
        match at {
            Some(index) => self.doc.insert_card(index, card),
            None => self.doc.push_card(card),
        }
        .map_err(|e| vec![("$kind".to_string(), e)])?;
        Ok(())
    }

    /// Remove the composable card at `index`, returning it. `None` when
    /// `index` is out of range.
    pub fn remove_card(&mut self, index: usize) -> Option<Card> {
        self.doc.remove_card(index)
    }

    /// A schema-bound writer for the composable card at `index`. The card's
    /// `$kind` resolves its [`CardSchema`](crate::CardSchema); an unknown kind carries no schema, so
    /// every typed write on it fails with [`EditError::UnknownField`] (write
    /// such a card opaquely through
    /// [`Card::store_field`](crate::Card::store_field)).
    /// [`EditError::IndexOutOfRange`] when `index` is out of range.
    pub fn card(&mut self, index: usize) -> Result<CardWriter<'_>, EditError> {
        let len = self.doc.cards().len();
        if index >= len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        Ok(CardWriter {
            config: self.config,
            doc: self.doc,
            index,
        })
    }
}

/// A single composable card bound to its [`CardSchema`](crate::CardSchema), from
/// [`TypedWriter::card`]. Same `set` / `set_all` verbs as
/// [`TypedWriter`], targeting the card at its bound index.
pub struct CardWriter<'a> {
    config: &'a QuillConfig,
    doc: &'a mut Document,
    index: usize,
}

impl<'a> CardWriter<'a> {
    fn card(&self) -> &Card {
        self.doc
            .card(self.index)
            .expect("bound in range, and cards cannot move while this cursor holds the document")
    }

    fn card_mut(&mut self) -> &mut Card {
        self.doc
            .card_mut(self.index)
            .expect("bound in range, and cards cannot move while this cursor holds the document")
    }

    fn fields_schema(&self) -> Option<&'a IndexMap<String, FieldSchema>> {
        self.card()
            .kind()
            .and_then(|k| self.config.card_kind(k))
            .map(|s| &s.fields)
    }

    /// The card's `$kind`, if any.
    pub fn kind(&self) -> Option<&str> {
        self.card().kind()
    }

    /// Write a field on this card, strict-committed against the card's
    /// [`CardSchema`](crate::CardSchema). An undeclared field (or any field when the card kind is
    /// unknown) fails with [`EditError::UnknownField`] rather than storing
    /// opaquely.
    pub fn set(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let schema = self.fields_schema();
        commit_impl(self.card_mut(), schema, name, value)
    }

    /// Revise this card's body from markdown (edit semantics), returning the
    /// text [`Delta`]: the card twin of [`TypedWriter::revise_body`].
    pub fn revise_body(&mut self, markdown: &str) -> Result<Delta, EditError> {
        self.card_mut().revise_body(markdown)
    }

    /// The card twin of [`TypedWriter::revise_field`], resolved against the
    /// card's [`CardSchema`](crate::CardSchema).
    pub fn revise_field(&mut self, name: &str, text: &str) -> Result<Delta, EditError> {
        let schema = self.fields_schema();
        revise_impl(self.card_mut(), schema, name, text)
    }

    /// Write several fields on this card atomically; see
    /// [`TypedWriter::set_all`]; an undeclared name aborts the whole batch with
    /// [`EditError::UnknownField`].
    pub fn set_all<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let schema = self.fields_schema();
        set_all_impl(self.card_mut(), schema, fields)
    }
}

/// Typed single-field commit shared by [`TypedWriter::set`] and
/// [`CardWriter::set`]. A `None` schema is an unknown card kind: every name on
/// it is undeclared.
fn commit_impl(
    card: &mut Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
    name: &str,
    value: impl Into<QuillValue>,
) -> Result<(), EditError> {
    match fields_schema.and_then(|m| m.get(name)) {
        Some(schema) => card.commit_field(name, value, schema),
        None => Err(EditError::unknown_field(name)),
    }
}

/// The anchor-preserving twin of [`commit_impl`], shared by
/// [`TypedWriter::revise_field`] and [`CardWriter::revise_field`].
fn revise_impl(
    card: &mut Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
    name: &str,
    text: &str,
) -> Result<Delta, EditError> {
    match fields_schema.and_then(|m| m.get(name)) {
        Some(schema) => card.revise_field_checked(name, text, schema),
        None => Err(EditError::unknown_field(name)),
    }
}

/// All-or-nothing batched write shared by [`TypedWriter::set_all`] and
/// [`CardWriter::set_all`]: resolve every field first, collecting every error.
/// A `None` schema is an unknown card kind: every name on it is undeclared.
fn set_all_impl<K, V, I>(
    card: &mut Card,
    fields_schema: Option<&IndexMap<String, FieldSchema>>,
    fields: I,
) -> Result<(), Vec<(String, EditError)>>
where
    K: Into<String>,
    V: Into<QuillValue>,
    I: IntoIterator<Item = (K, V)>,
{
    let fields: Vec<(String, QuillValue)> = fields
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();

    let mut resolved: Vec<(String, QuillValue)> = Vec::with_capacity(fields.len());
    let mut errors: Vec<(String, EditError)> = Vec::new();
    for (name, value) in fields {
        match fields_schema.and_then(|m| m.get(&name)) {
            Some(schema) => match resolve_field_write(&name, value, schema) {
                Ok(stored) => resolved.push((name, stored)),
                Err(e) => errors.push((name, e)),
            },
            None => errors.push((name.clone(), EditError::unknown_field(name))),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // Every entry validated by `resolve_field_write` above; apply unchecked.
    for (name, stored) in resolved {
        card.payload_mut().insert_unchecked(name, stored);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Codec;
    use crate::document::{Card, Document};
    use crate::version::QuillReference;
    use std::str::FromStr;

    const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Editor test quill
main:
  fields:
    subject:
      type: richtext
      inline: true
    qty:
      type: integer
card_kinds:
  note:
    fields:
      body:
        type: richtext
";

    fn config() -> QuillConfig {
        QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
    }

    fn blank_doc() -> Document {
        Document::new(QuillReference::from_str("memo@1.0.0").unwrap())
    }

    #[test]
    fn set_resolves_schema_field_as_typed_commit() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);

        ed.set("qty", "3").unwrap();
        ed.set("subject", "Hello").unwrap();
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(3)
        );
        assert_eq!(doc.main().field_text("subject", Codec::Richtext).unwrap().unwrap(), "Hello");
    }

    #[test]
    fn set_rejects_unknown_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        let err = ed.set("notafield", "x").unwrap_err();
        assert_eq!(err.code(), "edit::unknown_field");
        assert!(doc.main().payload().get("notafield").is_none());
    }

    /// A richtext write refuses a value in neither accepted encoding under the
    /// codec's own sentence, the one the wire and the schema-bound read spell.
    #[test]
    fn set_rejects_a_non_content_shape_by_naming_it() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        let err = ed
            .set("subject", QuillValue::from_json(serde_json::json!(true)))
            .unwrap_err();
        assert_eq!(err.code(), "edit::field_decode");
        assert!(
            matches!(&err, EditError::FieldDecode { message, .. }
                if message
                    == "expected a richtext content object or a markdown string, got a boolean"),
            "got {err:?}"
        );
    }

    #[test]
    fn set_all_is_all_or_nothing() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        let errs = ed
            .set_all([("qty", "5"), ("subject", "bad\n\nblock")])
            .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, "subject");
        assert!(doc.main().payload().get("qty").is_none());

        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.set_all([("qty", "5"), ("subject", "ok")]).unwrap();
        assert_eq!(
            doc.main().payload().get("qty").unwrap().as_json(),
            &serde_json::json!(5)
        );
    }

    #[test]
    fn add_card_fuses_new_commit_push() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.add_card("note", [("body", "**hi**")], Some("card body"), None)
            .unwrap();
        assert_eq!(doc.cards().len(), 1);
        assert_eq!(doc.cards()[0].kind(), Some("note"));
        assert_eq!(doc.cards()[0].field_text("body", Codec::Richtext).unwrap().unwrap(), "**hi**");
        assert_eq!(doc.cards()[0].body_markdown(), "card body");
    }

    #[test]
    fn add_card_at_inserts_and_remove_card_returns() {
        let config = config();
        let mut doc = blank_doc();
        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            ed.add_card("note", [("body", "a")], None, None).unwrap();
            ed.add_card("note", [("body", "c")], None, None).unwrap();
            ed.add_card("note", [("body", "b")], None, Some(1)).unwrap();
        }
        let bodies: Vec<String> = doc
            .cards()
            .iter()
            .map(|c| c.field_text("body", Codec::Richtext).unwrap().unwrap())
            .collect();
        assert_eq!(bodies, ["a", "b", "c"]);

        // Out-of-range insert is transactional.
        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            let errs = ed
                .add_card("note", [("body", "x")], None, Some(9))
                .unwrap_err();
            assert_eq!(errs[0].0, "$kind");
        }
        assert_eq!(doc.cards().len(), 3);

        {
            let mut ed = TypedWriter::new(&config, &mut doc);
            let removed = ed.remove_card(1).unwrap();
            assert_eq!(removed.field_text("body", Codec::Richtext).unwrap().unwrap(), "b");
            assert!(ed.remove_card(5).is_none());
        }
        assert_eq!(doc.cards().len(), 2);
    }

    #[test]
    fn add_card_is_transactional_on_bad_field() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        let errs = ed
            .add_card("note", [("stray", "x")], None, None)
            .unwrap_err();
        assert_eq!(errs[0].0, "stray");
        assert_eq!(errs[0].1.code(), "edit::unknown_field");
        assert_eq!(doc.cards().len(), 0);
    }

    #[test]
    fn card_writer_resolves_card_kind_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedWriter::new(&config, &mut doc);
        let mut card_ed = ed.card(0).unwrap();
        card_ed.set("body", "**hi**").unwrap();
        let err = card_ed.set("stray", "v").unwrap_err();
        assert_eq!(err.code(), "edit::unknown_field");

        assert_eq!(doc.cards()[0].field_text("body", Codec::Richtext).unwrap().unwrap(), "**hi**");

        let mut ed = TypedWriter::new(&config, &mut doc);
        assert!(matches!(
            ed.card(9),
            Err(EditError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn revise_field_is_typed_and_rejects_unknown_and_non_inline() {
        let config = config();
        let mut doc = blank_doc();
        let mut ed = TypedWriter::new(&config, &mut doc);
        let _delta = ed.revise_field("subject", "Hello").unwrap();
        assert_eq!(doc.main().field_text("subject", Codec::Richtext).unwrap().unwrap(), "Hello");

        let mut ed = TypedWriter::new(&config, &mut doc);
        assert_eq!(
            ed.revise_field("nope", "x").unwrap_err().code(),
            "edit::unknown_field"
        );
        // `richtext(inline)` rejects a multi-block result.
        let err = ed.revise_field("subject", "a\n\nb").unwrap_err();
        assert_eq!(err.code(), "edit::field_not_inline");
        assert_eq!(doc.main().field_text("subject", Codec::Richtext).unwrap().unwrap(), "Hello");
    }

    #[test]
    fn card_writer_revise_field_resolves_card_schema() {
        let config = config();
        let mut doc = blank_doc();
        doc.push_card(Card::new("note").unwrap()).unwrap();

        let mut ed = TypedWriter::new(&config, &mut doc);
        ed.card(0).unwrap().revise_field("body", "**hi**").unwrap();
        assert_eq!(doc.cards()[0].field_text("body", Codec::Richtext).unwrap().unwrap(), "**hi**");

        let mut ed = TypedWriter::new(&config, &mut doc);
        assert_eq!(
            ed.card(0)
                .unwrap()
                .revise_field("stray", "x")
                .unwrap_err()
                .code(),
            "edit::unknown_field"
        );
    }
}
