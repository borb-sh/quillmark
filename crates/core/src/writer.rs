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
//! w.set_values(&values)?;                // the whole document, replace per axis
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
use crate::quill::{CardSchema, CardValues, DocumentValues, FieldSchema, QuillConfig};
use crate::value::QuillValue;
use crate::{Delta, DocPath};

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

    /// Write the document in the values form: the write twin of
    /// [`TypedReader::values`](crate::TypedReader::values), and the typed lane
    /// widened from one field ([`set`](Self::set)) to the document.
    ///
    /// **An absent axis is untouched; a present one is replaced.** Per axis:
    ///
    /// - `fields`: the whole truth for declared names. A named one is written
    ///   (skipped when it equals its projection), an unnamed one removed. An
    ///   undeclared name the card already holds at that value is accepted,
    ///   since the read emits it; changed or new it is
    ///   [`EditError::UnknownField`], and unnamed it is left alone.
    /// - `body`: replaced from markdown, skipped when equal.
    /// - `cards`: *is* the card list. Position `i` whose kind matches (or
    ///   whose `kind` is absent) is patched in place by the same rules; a
    ///   differing kind rebuilds the slot; past the end appends; document
    ///   cards past the list are removed.
    /// - `ext`: `None` removes `$ext`, an empty map records an explicit
    ///   `$ext: {}`, a map replaces; each skipped when equal.
    ///
    /// All-or-nothing: every cell resolves before any is written.
    ///
    /// A changed content cell is a **cold import**, as on [`set`](Self::set):
    /// anchors on it do not survive, and [`revise_field`](Self::revise_field)
    /// per cell is what keeps them. Cards match by position and kind, so
    /// deleting, inserting or reordering an entry rewrites every card after it;
    /// the structural verbs ([`add_card`](Self::add_card),
    /// [`remove_card`](Self::remove_card), `Document::move_card`) do not.
    pub fn set_values(
        &mut self,
        values: &DocumentValues,
    ) -> Result<(), Vec<(DocPath, EditError)>> {
        let mut errors = Vec::new();
        let main_plan = plan_card(
            self.doc.main(),
            Some(&self.config.main),
            &DocPath::main(),
            values.fields.as_ref(),
            values.body.as_deref(),
            values.ext.as_ref(),
            &mut errors,
        );
        let mut slots: Vec<(usize, Slot)> = Vec::new();
        for (index, incoming) in values.cards.iter().flatten().enumerate() {
            if let Some(slot) = plan_slot(self.config, self.doc, index, incoming, &mut errors) {
                slots.push((index, slot));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        main_plan.apply(self.doc.main_mut());
        for (index, slot) in slots {
            apply_slot(self.doc, index, slot);
        }
        if let Some(cards) = &values.cards {
            while self.doc.cards().len() > cards.len() {
                self.doc.remove_card(self.doc.cards().len() - 1);
            }
        }
        Ok(())
    }

    /// A schema-bound writer for the composable card at `index`. The card's
    /// `$kind` resolves its [`CardSchema`]; an unknown kind carries no schema, so
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

/// A single composable card bound to its [`CardSchema`], from
/// [`TypedWriter::card`]. Same `set` / `set_all` / `set_values` verbs as
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
    /// [`CardSchema`]. An undeclared field (or any field when the card kind is
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
    /// card's [`CardSchema`].
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

    /// Write this card in the values form: [`TypedWriter::set_values`]
    /// restricted to one slot, under the same per-axis rule. An absent `kind`
    /// keeps the card's; a differing one rebuilds the slot. Refusals anchor at
    /// `cards.<kind>[<index>]`.
    pub fn set_values(&mut self, values: &CardValues) -> Result<(), Vec<(DocPath, EditError)>> {
        let mut errors = Vec::new();
        let slot = plan_slot(self.config, self.doc, self.index, values, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        if let Some(slot) = slot {
            apply_slot(self.doc, self.index, slot);
        }
        Ok(())
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

/// What one card position of a values write resolves to: a patch of the card
/// there, or a card built whole for the slot.
enum Slot {
    Plan(CardPlan),
    Build(Card),
}

/// One card's resolved writes, held until every cell in the batch has
/// resolved. Applying cannot fail: each entry was produced by the same verb the
/// single-cell writes use.
#[derive(Default)]
struct CardPlan {
    fields: Vec<(String, QuillValue)>,
    removals: Vec<String>,
    body: Option<quillmark_content::Normalized>,
    ext: Option<Option<serde_json::Map<String, serde_json::Value>>>,
}

impl CardPlan {
    fn apply(self, card: &mut Card) {
        for name in self.removals {
            card.payload_mut().remove(&name);
        }
        for (name, stored) in self.fields {
            card.payload_mut().insert_unchecked(name, stored);
        }
        if let Some(content) = self.body {
            card.overwrite_body(content);
        }
        match self.ext {
            None => {}
            Some(None) => {
                card.payload_mut().take_ext();
            }
            Some(Some(map)) => card.payload_mut().set_ext(map),
        }
    }
}

/// Resolve the values for card position `index`. The kind is the entry's, or
/// the card's there when the entry carries none; a position holding no card
/// and naming no kind is a kindless build, refused at `cards[<index>]`.
fn plan_slot(
    config: &QuillConfig,
    doc: &Document,
    index: usize,
    incoming: &CardValues,
    errors: &mut Vec<(DocPath, EditError)>,
) -> Option<Slot> {
    let current = doc.card(index);
    let kind = match &incoming.kind {
        None => current.and_then(|c| c.kind()),
        Some(kind) => kind.as_deref(),
    };
    let base = DocPath::card(kind, index);
    let schema = kind.and_then(|k| config.card_kind(k));
    match current {
        Some(card) if card.kind() == kind => Some(Slot::Plan(plan_card(
            card,
            schema,
            &base,
            incoming.fields.as_ref(),
            incoming.body.as_deref(),
            incoming.ext.as_ref(),
            errors,
        ))),
        _ => build_card(kind, incoming, schema, &base, errors).map(Slot::Build),
    }
}

fn apply_slot(doc: &mut Document, index: usize, slot: Slot) {
    match slot {
        Slot::Plan(plan) => plan.apply(
            doc.card_mut(index)
                .expect("planned against a card at this index"),
        ),
        Slot::Build(card) => match doc.cards_mut().get_mut(index) {
            Some(existing) => *existing = card,
            // Every index past the end arrives in order, so the append lands
            // where the plan addressed it.
            None => doc
                .push_card(card)
                .expect("kind validated by Card::new in the plan"),
        },
    }
}

/// Resolve one card's incoming axes against `schema`, appending every refusal
/// to `errors` under its own [`DocPath`]. An absent axis plans nothing. A
/// `None` schema is a kind carrying no declaration: every name on it is
/// undeclared, as it is for [`CardWriter::set`].
fn plan_card(
    card: &Card,
    schema: Option<&CardSchema>,
    base: &DocPath,
    fields: Option<&IndexMap<String, serde_json::Value>>,
    body: Option<&str>,
    ext: Option<&Option<serde_json::Map<String, serde_json::Value>>>,
    errors: &mut Vec<(DocPath, EditError)>,
) -> CardPlan {
    let mut plan = CardPlan::default();
    if let Some(fields) = fields {
        let declared = schema.map(|s| &s.fields);
        // The read emits an undeclared field verbatim, so one coming back at
        // the value the card holds is an untouched cell, not the typo the typed
        // lane refuses. The guard answers before the vocabulary does.
        for (name, incoming) in fields {
            if declared.is_some_and(|m| m.contains_key(name)) {
                continue;
            }
            if card.payload().get(name).map(|v| v.as_json()) == Some(incoming) {
                continue;
            }
            errors.push((base.field(name), EditError::unknown_field(name)));
        }
        if let Some(declared) = declared {
            for (name, field_schema) in declared {
                let current = card.payload().get(name);
                let Some(incoming) = fields.get(name) else {
                    if current.is_some() {
                        plan.removals.push(name.clone());
                    }
                    continue;
                };
                if current.is_some_and(|cur| {
                    *incoming == crate::quill::project_field(name, cur, field_schema)
                }) {
                    continue;
                }
                match resolve_field_write(
                    name,
                    QuillValue::from_json(incoming.clone()),
                    field_schema,
                ) {
                    Err(e) => errors.push((
                        e.doc_path(base).unwrap_or_else(|| base.field(name)),
                        e,
                    )),
                    Ok(stored) if current.is_some_and(|cur| cur.as_json() == stored.as_json()) => {}
                    Ok(stored) => plan.fields.push((name.clone(), stored)),
                }
            }
        }
    }
    if let Some(body) = body {
        if body != card.body_markdown() {
            match crate::document::import_body(body) {
                Ok(content) => plan.body = Some(content),
                Err(e) => errors.push((base.body(), EditError::Import(e))),
            }
        }
    }
    match ext {
        None => {}
        Some(None) => {
            if card.ext().is_some() {
                plan.ext = Some(None);
            }
        }
        Some(Some(map)) => {
            if card.ext() != Some(map) {
                match crate::value::depth_check_meta_map(map.clone(), |max| {
                    EditError::ValueTooDeep { max }
                }) {
                    Ok(checked) => plan.ext = Some(Some(checked)),
                    Err(e) => errors.push((base.clone(), e)),
                }
            }
        }
    }
    plan
}

/// Build a whole card from incoming values, for a position whose kind does not
/// match the document's. An absent axis is empty, there being nothing to leave
/// untouched. Committed in full before it is placed, so a refusal leaves the
/// document untouched.
fn build_card(
    kind: Option<&str>,
    incoming: &CardValues,
    schema: Option<&CardSchema>,
    base: &DocPath,
    errors: &mut Vec<(DocPath, EditError)>,
) -> Option<Card> {
    let mut card = match Card::new(kind.unwrap_or_default()) {
        Ok(card) => card,
        Err(e) => {
            errors.push((base.clone(), e));
            return None;
        }
    };
    if let Some(fields) = &incoming.fields {
        let entries: Vec<(String, QuillValue)> = fields
            .iter()
            .map(|(k, v)| (k.clone(), QuillValue::from_json(v.clone())))
            .collect();
        if let Err(bundle) = set_all_impl(&mut card, schema.map(|s| &s.fields), entries) {
            errors.extend(bundle.into_iter().map(|(name, e)| {
                (e.doc_path(base).unwrap_or_else(|| base.field(&name)), e)
            }));
            return None;
        }
    }
    if let Some(body) = incoming.body.as_deref().filter(|b| !b.is_empty()) {
        match crate::document::import_body(body) {
            Ok(content) => card.overwrite_body(content),
            Err(e) => {
                errors.push((base.body(), EditError::Import(e)));
                return None;
            }
        }
    }
    if let Some(Some(map)) = &incoming.ext {
        if let Err(e) = card.store_ext(map.clone()) {
            errors.push((base.clone(), e));
            return None;
        }
    }
    Some(card)
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
mod values_tests;

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
