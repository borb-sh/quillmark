//! Versioned, storage-stable serialization for [`Document`].
//!
//! [`Document`]'s in-memory layout is an internal detail and is deliberately not
//! serialized directly. Persisting one converts it to a [`StoredDocument`]: a
//! versioned envelope whose wire format is frozen per schema version. `Document`
//! serializes to it from its cards and deserializes through `#[serde(try_from)]`,
//! so the ordinary serde entry points produce and consume the versioned form
//! transparently.
//!
//! The schema versions, the shape each names, and the procedure for adding one:
//! `prose/canon/DOCUMENT_STORAGE.md`.

#![allow(non_camel_case_types)]

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use quillmark_automerge::content::model::Content as ContentModel;

use super::meta::validate_composable_kind;
use super::payload::{MetaKey, Payload, PayloadItem, trim_comment_text};
use super::prescan::NestedComment;
use super::{Card, Document};
use crate::value::PathSegment;
use crate::value::QuillValue;
use crate::version::QuillReference;

/// The storage version tag every save writes: each body a content-model
/// value, its blocks, containers, anchors and island slots carrying ids.
///
/// The wire key is spelled `schema` though it names a storage version, not a
/// field schema: it is the serde tag [`StoredDocument`] dispatches on, and
/// retagging it would break the versioning it exists to serve.
///
/// It shares [`STORAGE_V0_134_0`]'s tree and adds the `codec` key to a
/// content value, which a `0.134` reader does not decode.
pub const STORAGE_V0_135_0: &str = "quillmark/document@0.135.0";

/// The tag before [`STORAGE_V0_135_0`], read through the same tree. It is the
/// oldest tag read.
pub const STORAGE_V0_134_0: &str = "quillmark/document@0.134.0";

/// Read the storage version off a raw DTO payload without deserializing it.
///
/// `None` when `json` is not valid JSON, not an object, or carries no version
/// tag. The returned string is **not** validated against the supported set:
/// callers use it to tell a version this build does not read, newer or
/// retired, from a corrupt payload.
pub fn peek_storage_version(json: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Peek {
        schema: Option<String>,
    }
    serde_json::from_str::<Peek>(json).ok()?.schema
}

/// Versioned envelope for a persisted [`Document`].
///
/// The `schema` field selects the payload version. Deserialization dispatches
/// on it; unknown values are rejected. Every variant below the newest is
/// read-only and migrates forward on reconstruction, and a new version is a new
/// variant, leaving existing ones byte-stable. Each variant's tree is internal
/// to this module: a host reads and writes the JSON, never the tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema")]
#[non_exhaustive]
pub enum StoredDocument {
    #[serde(rename = "quillmark/document@0.135.0")]
    V0_135_0(DocumentV0_134_0),
    #[serde(rename = "quillmark/document@0.134.0")]
    V0_134_0(DocumentV0_134_0),
}

/// Failure while reconstructing a [`Document`] from a [`StoredDocument`].
///
/// Only [`Self::InvalidQuillReference`] is typed: it is the one error a
/// non-malicious caller hits. Every other defect can only arise from a
/// hand-crafted storage DTO and reports through [`Self::Malformed`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StorageError {
    /// A stored quill reference string could not be parsed.
    InvalidQuillReference {
        /// The offending string.
        value: String,
        /// Parser explanation.
        reason: String,
    },
    /// The stored document is structurally malformed in a way the markdown
    /// parser would reject. The message describes the specific defect.
    Malformed(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::InvalidQuillReference { value, reason } => {
                write!(f, "invalid quill reference {value:?}: {reason}")
            }
            StorageError::Malformed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// Frozen `0.134.0` representation of a [`Document`], each body a
/// content-model value (see [`StoredContent`]). A document, card, payload or
/// payload item carrying a key its type does not name fails the load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentV0_134_0 {
    main: CardV0_134_0,
    #[serde(default)]
    cards: Vec<CardV0_134_0>,
}

/// Frozen `0.134.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardV0_134_0 {
    payload: PayloadV0_134_0,
    body: StoredContent,
}

/// A card body embedded as a content-model value, through its canonical
/// JSON ([`quillmark_automerge::content::wire`]): `Serialize` validates, then writes the
/// key-sorted object; `Deserialize` reads it as a stored value
/// ([`ContentModel::from_value_lenient`]), so an invalid body fails the load
/// and a non-canonical one loads canonical, `rewritten` saying so. Both
/// directions check: `Card::overwrite_body` takes a caller's value, and a
/// store that checked only on load would accept bytes it cannot read back.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContent {
    content: ContentModel,
    /// Whether the stored bytes spelled the body otherwise than `content`.
    rewritten: bool,
}

impl From<ContentModel> for StoredContent {
    fn from(content: ContentModel) -> Self {
        StoredContent { content, rewritten: false }
    }
}

impl Serialize for StoredContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.content
            .validate()
            .map_err(|inv| serde::ser::Error::custom(format!("content invariant: {inv}")))?;
        self.content.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StoredContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        ContentModel::from_value_lenient(&value)
            .map(|read| StoredContent { content: read.content, rewritten: read.rewritten })
            .map_err(serde::de::Error::custom)
    }
}

/// Frozen `0.134.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadV0_134_0 {
    #[serde(default)]
    items: Vec<PayloadItemV0_134_0>,
    #[serde(default)]
    nested_comments: Vec<NestedCommentV0_134_0>,
}

/// Frozen `0.134.0` representation of a unified payload item. An item carrying
/// a key its variant does not name fails the load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum PayloadItemV0_134_0 {
    /// `$quill` system metadata: the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
    /// `$id` system metadata: a composable card's id.
    Id { value: String },
    /// `$ext` system metadata: an opaque mapping carrying out-of-band
    /// extension data. Never emitted into the plate JSON.
    Ext {
        value: serde_json::Map<String, serde_json::Value>,
    },
    /// `$seed` system metadata: a mapping keyed by card-kind carrying the
    /// per-kind seed overlays. Never emitted into the plate JSON.
    Seed {
        value: serde_json::Map<String, serde_json::Value>,
    },
    /// A user-defined field.
    Field {
        key: String,
        value: serde_json::Value,
    },
    /// A YAML comment.
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.134.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_134_0 {
    container_path: Vec<CommentPathSegmentV0_134_0>,
    position: usize,
    text: String,
    inline: bool,
}

/// Frozen `0.134.0` representation of a [`PathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_134_0 {
    Key(String),
    Index(usize),
}

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_135_0(DocumentV0_134_0::from(&doc))
    }
}

/// A document serializes as the [`StoredDocument`] its cards read as,
/// leaving the Automerge document it is held in untouched.
impl Serialize for Document {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoredDocument::V0_135_0(DocumentV0_134_0::from(self)).serialize(serializer)
    }
}

impl From<&Document> for DocumentV0_134_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_134_0 {
            main: CardV0_134_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_134_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_134_0 {
    fn from(card: &Card) -> Self {
        CardV0_134_0 {
            payload: PayloadV0_134_0::from(card.payload()),
            body: StoredContent::from(card.body().clone()),
        }
    }
}

/// Each content value in a field's value, at any depth, read as a stored
/// value is: one spelled otherwise than its canonical form is rewritten to
/// it, and one that does not read refuses the load, as a body's does. True
/// where one was rewritten.
fn canonicalize_field(value: &mut serde_json::Value) -> Result<bool, quillmark_automerge::content::wire::ParseError> {
    if quillmark_automerge::content::units::is_content_value(value) {
        let read = ContentModel::from_value_lenient(value)?;
        if read.rewritten {
            *value = read.content.to_value();
        }
        return Ok(read.rewritten);
    }
    let mut any = false;
    match value {
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                any |= canonicalize_field(v)?;
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                any |= canonicalize_field(v)?;
            }
        }
        _ => {}
    }
    Ok(any)
}

impl From<&Payload> for PayloadV0_134_0 {
    fn from(payload: &Payload) -> Self {
        PayloadV0_134_0 {
            items: payload
                .items()
                .iter()
                .map(PayloadItemV0_134_0::from)
                .collect(),
            nested_comments: payload
                .nested_comments()
                .iter()
                .map(NestedCommentV0_134_0::from)
                .collect(),
        }
    }
}

impl From<&PayloadItem> for PayloadItemV0_134_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Quill { reference } => PayloadItemV0_134_0::Quill {
                value: reference.to_string(),
            },
            PayloadItem::Kind { value } => PayloadItemV0_134_0::Kind {
                value: value.clone(),
            },
            PayloadItem::Id { value } => PayloadItemV0_134_0::Id {
                value: value.clone(),
            },
            // The DTO keeps `$ext` / `$seed` as explicit variants, so the live
            // model's unified `Meta` splits back out by key. Neither carries
            // `nested_comments`: those live in the payload-level sidecar.
            PayloadItem::Meta {
                key: MetaKey::Ext,
                value,
            } => PayloadItemV0_134_0::Ext {
                value: value.clone(),
            },
            PayloadItem::Meta {
                key: MetaKey::Seed,
                value,
            } => PayloadItemV0_134_0::Seed {
                value: value.clone(),
            },
            PayloadItem::Field { key, value } => PayloadItemV0_134_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
            },
            PayloadItem::Comment { text, inline } => PayloadItemV0_134_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_134_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_134_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_134_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&PathSegment> for CommentPathSegmentV0_134_0 {
    fn from(seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => CommentPathSegmentV0_134_0::Key(k.clone()),
            PathSegment::Index(i) => CommentPathSegmentV0_134_0::Index(*i),
        }
    }
}

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        from_stored(stored).map(|parsed| parsed.document)
    }
}

impl TryFrom<DocumentV0_134_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_134_0) -> Result<Self, Self::Error> {
        document_from_stored(payload).map(|parsed| parsed.document)
    }
}

/// A stored row's [`Document`], with a
/// [`noncanonical_read`](super::noncanonical_read) warning for each content
/// value the read rewrote to its canonical form.
pub fn from_stored(stored: StoredDocument) -> Result<super::Parsed, StorageError> {
    match stored {
        StoredDocument::V0_135_0(payload) | StoredDocument::V0_134_0(payload) => document_from_stored(payload),
    }
}

fn document_from_stored(payload: DocumentV0_134_0) -> Result<super::Parsed, StorageError> {
    let mut warnings = Vec::new();
    let (mut main, rewritten) = card_from_stored(payload.main)?;
    warnings.extend(rewritten.warnings(&crate::path::DocPath::main()));
    if main.quill().is_none() {
        return Err(StorageError::Malformed(
            "main card must carry a $quill entry".into(),
        ));
    }
    if main.id().is_some() {
        return Err(StorageError::Malformed(
            "main card must not carry a $id entry: it is addressed as `main`".into(),
        ));
    }
    // The root's `$kind` is `main` by position: any other value emits a root
    // block the parser rejects. An absent one is synthesised so the emit
    // stays parseable, as the parser does.
    match main.kind() {
        Some("main") => {}
        None => main.payload_mut().set_kind("main"),
        Some(other) => {
            return Err(StorageError::Malformed(format!(
                "main card has $kind {other:?}, but `main` is reserved for \
                 the document root"
            )));
        }
    }
    let mut cards = Vec::with_capacity(payload.cards.len());
    for (index, card) in payload.cards.into_iter().enumerate() {
        let (card, rewritten) = card_from_stored(card)?;
        warnings.extend(rewritten.warnings(&crate::path::DocPath::card(card.kind(), index)));
        cards.push(card);
    }
    let mut ids = std::collections::HashSet::new();
    for card in &cards {
        if let Some(id) = card.id() {
            if !super::card_id::is_card_id(id) {
                return Err(StorageError::Malformed(format!("invalid card id {id:?}")));
            }
            if !ids.insert(id) {
                return Err(StorageError::Malformed(format!(
                    "card id {id:?} is held by two cards"
                )));
            }
        }
        if card.quill().is_some() {
            return Err(StorageError::Malformed(
                "composable cards must not carry a $quill entry".into(),
            ));
        }
        if card.seed().is_some() {
            return Err(StorageError::Malformed(
                "composable cards must not carry a $seed entry".into(),
            ));
        }
        let Some(kind) = card.kind() else {
            return Err(StorageError::Malformed("composable cards must carry a $kind entry".into()));
        };
        match validate_composable_kind(kind) {
            Ok(()) => {}
            Err(super::meta::CardKindError::InvalidName) => {
                return Err(StorageError::Malformed(format!(
                    "invalid composable card kind {kind:?}: must match \
                     [a-z_][a-z0-9_]*"
                )));
            }
            Err(super::meta::CardKindError::Reserved) => {
                return Err(StorageError::Malformed(format!(
                    "composable card kind {kind:?} is reserved (root only)"
                )));
            }
        }
    }
    within_card_ceiling(Document::from_main_and_cards(main, cards)).map(|document| super::Parsed { document, warnings })
}

/// `doc`, when it holds no more cards than markdown parse admits.
fn within_card_ceiling(doc: Document) -> Result<Document, StorageError> {
    let (count, max) = (doc.cards().len(), crate::error::MAX_CARD_COUNT);
    if count > max {
        return Err(StorageError::Malformed(format!(
            "document has {count} cards, exceeding the maximum of {max}"
        )));
    }
    Ok(doc)
}

impl TryFrom<CardV0_134_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_134_0) -> Result<Self, Self::Error> {
        card_from_stored(card).map(|(card, _)| card)
    }
}

/// A stored card, with what of it the read rewrote to its canonical form: its
/// body, and each field holding a content value spelled otherwise.
fn card_from_stored(mut card: CardV0_134_0) -> Result<(Card, Rewritten), StorageError> {
    let mut rewritten = Rewritten { body: card.body.rewritten, fields: Vec::new() };
    for item in &mut card.payload.items {
        if let PayloadItemV0_134_0::Field { key, value } = item {
            let changed = canonicalize_field(value)
                .map_err(|e| StorageError::Malformed(format!("field `{key}` holds content that does not read: {e}")))?;
            if changed {
                rewritten.fields.push(key.clone());
            }
        }
    }
    let payload = Payload::try_from(card.payload)?;
    validate_dto_payload(&payload)?;
    Ok((Card::from_parts(payload, card.body.content), rewritten))
}

/// What a stored card's read rewrote to its canonical form.
struct Rewritten {
    body: bool,
    fields: Vec<String>,
}

impl Rewritten {
    /// One [`noncanonical_read`](super::noncanonical_read) warning per value
    /// rewritten, at the card `at` names.
    fn warnings(self, at: &crate::path::DocPath) -> impl Iterator<Item = crate::error::Diagnostic> {
        let body = self.body.then(|| at.body());
        let fields = self.fields.into_iter().map(|name| at.field(&name));
        body.into_iter().chain(fields).map(|path| super::noncanonical_read().with_path(path.to_string()))
    }
}

impl TryFrom<PayloadV0_134_0> for Payload {
    type Error = StorageError;

    fn try_from(p: PayloadV0_134_0) -> Result<Self, Self::Error> {
        let mut items = Vec::with_capacity(p.items.len());
        for item in p.items {
            items.push(PayloadItem::try_from(item)?);
        }
        let nested = p
            .nested_comments
            .into_iter()
            .map(NestedComment::from)
            .collect();
        Ok(Payload::from_items_with_nested(items, nested))
    }
}

impl TryFrom<PayloadItemV0_134_0> for PayloadItem {
    type Error = StorageError;

    fn try_from(item: PayloadItemV0_134_0) -> Result<Self, Self::Error> {
        Ok(match item {
            PayloadItemV0_134_0::Quill { value } => {
                let reference = QuillReference::from_str(&value).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: value.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            PayloadItemV0_134_0::Kind { value } => PayloadItem::Kind { value },
            PayloadItemV0_134_0::Id { value } => PayloadItem::Id { value },
            PayloadItemV0_134_0::Ext { value } => PayloadItem::Meta {
                key: MetaKey::Ext,
                value: depth_check_meta_map(value, "$ext")?,
            },
            PayloadItemV0_134_0::Seed { value } => PayloadItem::Meta {
                key: MetaKey::Seed,
                value: depth_check_meta_map(value, "$seed")?,
            },
            PayloadItemV0_134_0::Field { key, value } => {
                super::edit::validate_field(&key, &value)
                    .map_err(|v| StorageError::Malformed(v.message(&key)))?;
                PayloadItem::Field {
                    key,
                    value: QuillValue::from_json(value),
                }
            }
            PayloadItemV0_134_0::Comment { text, inline } => PayloadItem::Comment {
                text: trim_comment_text(text),
                inline,
            },
        })
    }
}

/// Depth-bound a `$ext` / `$seed` mapping at the storage boundary (§8).
fn depth_check_meta_map(
    value: serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, StorageError> {
    crate::value::depth_check_meta_map(value, |max| {
        StorageError::Malformed(format!(
            "{key} nests deeper than the maximum of {} levels",
            max
        ))
    })
}

impl From<NestedCommentV0_134_0> for NestedComment {
    fn from(nc: NestedCommentV0_134_0) -> Self {
        NestedComment {
            container_path: nc
                .container_path
                .into_iter()
                .map(PathSegment::from)
                .collect(),
            position: nc.position,
            text: trim_comment_text(nc.text),
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_134_0> for PathSegment {
    fn from(seg: CommentPathSegmentV0_134_0) -> Self {
        match seg {
            CommentPathSegmentV0_134_0::Key(k) => PathSegment::Key(k),
            CommentPathSegmentV0_134_0::Index(i) => PathSegment::Index(i),
        }
    }
}

/// Reject a payload no markdown-parsed `Document` could produce. The parser
/// rejects each on source; this guards hand-crafted storage DTOs.
fn validate_dto_payload(payload: &Payload) -> Result<(), StorageError> {
    super::edit::validate_payload(payload).map_err(|v| StorageError::Malformed(v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Codec;

    fn sample() -> Document {
        Document::parse(
            "\
~~~card-yaml
$quill: usaf_memo@0.1
$kind: main
# a top-level comment
memo_for:
  - ORG/SYMBOL # inline comment inside a sequence
date: 2504-10-05
subject: Subject of the Memorandum
~~~

The body of the memorandum.

~~~card-yaml
$kind: indorsement
for: ORG/SYMBOL
from: ORG/SYMBOL
~~~

This body and the metadata above are an indorsement card.
",
        )
        .unwrap()
        .document
    }

    #[test]
    fn content_field_survives_storage_round_trip_losslessly() {
        // The storage DTO is the lossless carrier: an `underline` with no
        // markdown form survives here where a markdown save would drop it.
        use quillmark_automerge::content::lines::model::{Mark, MarkKind};

        let mut doc = sample();
        let mut content = quillmark_automerge::content::lines::import::from_markdown("underlined intro")
            .unwrap()
            .content
            .into_content();
        content.marks.push(Mark::new(0, 10, MarkKind::Underline));
        let content = content.into_lines();
        let json = quillmark_automerge::content::migrate::from_lines(&content).to_value();
        let schema = crate::quill::FieldSchema::new(
            "intro".to_string(),
            crate::quill::FieldType::RichText { multiline: true },
            None,
        );
        doc.main_mut()
            .commit_field("intro", crate::value::QuillValue::from_json(json), &schema)
            .unwrap();

        let stored = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&stored).unwrap();
        assert_eq!(
            doc, restored,
            "content field must survive storage round-trip"
        );
        let read = restored
            .main()
            .field_content("intro", Codec::Richtext)
            .unwrap()
            .unwrap();
        assert!(
            read.marks
                .iter()
                .any(|m| matches!(m.kind, MarkKind::Underline)),
            "underline (content-only) must survive the DTO carrier"
        );
    }

    fn row(
        schema: &str,
        item: serde_json::Value,
        nested: serde_json::Value,
    ) -> serde_json::Map<String, serde_json::Value> {
        let serde_json::Value::Object(row) = serde_json::json!({
            "schema": schema,
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                    item,
                ], "nested_comments": nested },
                "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""},
            },
            "cards": [],
        }) else {
            unreachable!()
        };
        row
    }

    #[test]
    fn a_row_with_an_unknown_key_fails_the_load() {
        let field = serde_json::json!({ "type": "field", "key": "a", "value": [1] });
        for schema in [STORAGE_V0_135_0, STORAGE_V0_134_0] {
            let with_fill = row(
                schema,
                serde_json::json!({ "type": "field", "key": "a", "value": 1, "fill": true }),
                serde_json::json!([]),
            );
            let mut at_root = row(schema, field.clone(), serde_json::json!([]));
            at_root.insert("extra".into(), serde_json::json!(1));
            let mut at_card = row(schema, field.clone(), serde_json::json!([]));
            at_card["main"]["extra"] = serde_json::json!(1);
            let mut at_payload = row(schema, field.clone(), serde_json::json!([]));
            at_payload["main"]["payload"]["extra"] = serde_json::json!(1);

            for (row, key) in [
                (with_fill, "fill"),
                (at_root, "extra"),
                (at_card, "extra"),
                (at_payload, "extra"),
            ] {
                let err = serde_json::from_value::<Document>(row.into()).unwrap_err();
                assert!(
                    err.to_string().contains(&format!("unknown field `{key}`")),
                    "{schema}: {err}"
                );
            }
        }
    }

    /// A row loads only where its own markdown parses within the card ceiling.
    #[test]
    fn a_stored_row_past_the_card_ceiling_fails_the_load() {
        use crate::error::MAX_CARD_COUNT;

        let mut doc = Document::parse("~~~card-yaml\n$quill: q@1.0\n$kind: main\n~~~\n")
            .unwrap()
            .document;
        for _ in 0..MAX_CARD_COUNT {
            doc.unchecked().insert_card(None, Card::new("note").unwrap()).unwrap();
        }
        let row = serde_json::to_value(&doc).unwrap();
        let loads = |row: serde_json::Value| -> Result<Document, String> {
            serde_json::from_value::<Document>(row).map_err(|e| e.to_string())
        };

        let at_ceiling = loads(row.clone()).expect("1,000 cards load");
        assert!(Document::parse(&at_ceiling.to_markdown()).is_ok());
        let mut past = row;
        let cards = past["cards"].as_array_mut().unwrap();
        cards.push(cards[0].clone());
        let err = loads(past).expect_err("1,001 cards");
        assert!(err.contains("cards"), "{err}");
    }

    #[test]
    fn a_stored_nested_comment_spanning_lines_fails_the_load() {
        let row = row(
            STORAGE_V0_135_0,
            serde_json::json!({ "type": "field", "key": "a", "value": [1] }),
            serde_json::json!([{
                "container_path": [{"Key": "a"}],
                "position": 0,
                "text": "x\n~~~\n\n~~~\n$kind: evil",
                "inline": false,
            }]),
        );
        assert!(serde_json::from_value::<Document>(row.into()).is_err());
    }

    #[test]
    fn a_task_survives_storage() {
        let doc = Document::parse(
            "~~~card-yaml\n$quill: q@1.0\n$kind: main\n~~~\n\n- [x] done\n- [ ] open\n",
        )
        .unwrap()
        .document;
        let stored = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&stored).as_deref(),
            Some(STORAGE_V0_135_0)
        );
        assert!(stored.contains(r#""checked":true"#), "{stored}");
        assert!(stored.contains(r#""checked":false"#), "{stored}");
        let back: Document = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, doc);
    }

    /// A newer tag and a retired one alike fail as an unknown version.
    #[test]
    fn rejects_unknown_schema_version() {
        for version in ["0.99.0", "0.131.0", "0.129.0", "0.124.0"] {
            let json = format!(r#"{{"schema":"quillmark/document@{version}","main":{{}}}}"#);
            let err = serde_json::from_str::<Document>(&json)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!("unknown variant `quillmark/document@{version}`")),
                "{err}"
            );
        }
    }

    #[test]
    fn peek_storage_version_reads_field_without_full_parse() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&json).as_deref(),
            Some(STORAGE_V0_135_0)
        );

        let future = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert_eq!(
            peek_storage_version(future).as_deref(),
            Some("quillmark/document@0.99.0")
        );
        assert_eq!(peek_storage_version("not json"), None);
        assert_eq!(peek_storage_version(r#"{"foo":"bar"}"#), None);
    }

    #[test]
    fn comment_on_dollar_line_round_trips() {
        let src = "\
~~~card-yaml
$quill: q@1.0
$kind: main # required for root
title: Hi
~~~
";
        let doc = Document::parse(src).unwrap().document;
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, restored);
        assert!(
            restored
                .to_markdown()
                .contains("$kind: main # required for root")
        );
    }

    #[test]
    fn rejects_main_card_without_quill() {
        let json = r#"{
            "schema": "quillmark/document@0.135.0",
            "main": {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}},
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("$quill"), "{err}");
    }

    #[test]
    fn rejects_composable_card_tagged_main() {
        let json = r#"{
            "schema": "quillmark/document@0.135.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}
            },
            "cards": [
                {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("reserved (root only)"), "{err}");
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let json = r#"{
            "schema": "quillmark/document@0.135.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "not a valid ref!!"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}
            },
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid quill reference"), "{err}");
    }

    #[test]
    fn rejects_composable_card_with_seed() {
        let json = r#"{
            "schema": "quillmark/document@0.135.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}
            },
            "cards": [
                {"payload": {"items": [
                    {"type": "kind", "value": "indorsement"},
                    {"type": "seed", "value": {"note": {"from": "X"}}}
                ]}, "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("composable cards must not carry a $seed entry"),
            "{err}"
        );
    }

    #[test]
    fn a_seed_item_round_trips() {
        let json = r#"{
            "schema": "quillmark/document@0.135.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "49 FW/CC"}}}
                ]},
                "body": {"blocks": [{"id": "b0", "kind": "para"}], "text": ""}
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        let overlay = doc
            .main()
            .seed()
            .and_then(|m| m.get("indorsement"))
            .and_then(crate::document::SeedOverlay::from_json)
            .expect("overlay present");
        assert_eq!(
            overlay.fields.get("from").and_then(|v| v.as_str()),
            Some("49 FW/CC")
        );
        let reser: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, reser);
    }

    /// The exact embedded bytes of the first top-level `"body":` object,
    /// balanced-brace and string-aware.
    fn locate_body_subtree(envelope: &str) -> &str {
        const KEY: &str = "\"body\":";
        let start = envelope.find(KEY).expect("body key present") + KEY.len();
        let bytes = envelope.as_bytes();
        assert_eq!(
            bytes[start], b'{',
            "body must embed as a nested object, not an escaped string"
        );
        let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
        for (i, &b) in bytes[start..].iter().enumerate() {
            if in_str {
                match (escaped, b) {
                    (true, _) => escaped = false,
                    (false, b'\\') => escaped = true,
                    (false, b'"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &envelope[start..start + i + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced body object");
    }

    #[test]
    fn body_subtree_is_byte_identical_to_canonical_json() {
        // Two disciplines in one envelope: compact insertion-ordered outer
        // structure, canonical key-sorted `body` subtree.
        let doc = Document::parse(
            "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: Hi\n~~~\n\n\
             A paragraph with **bold**, _emph_, and a [link](https://example.com).\n\n\
             Second paragraph continues the content.\n",
        )
        .unwrap()
        .document;
        let body = doc.main().body();
        assert!(
            !body.marks.is_empty(),
            "test needs a non-trivial content (marks present)"
        );
        let expected = body.to_json();
        let envelope = serde_json::to_string(&doc).unwrap();
        let body = locate_body_subtree(&envelope);
        assert_eq!(
            body, expected,
            "the envelope body subtree must equal to_json byte-for-byte"
        );
        assert!(body.starts_with("{\"blocks\":"));
    }

    #[test]
    fn current_tag_round_trips_as_fixed_point() {
        let doc = sample();
        let first = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&first).unwrap();
        assert_eq!(doc, restored);
        let second = serde_json::to_string(&restored).unwrap();
        assert_eq!(
            first, second,
            "V0_134_0 serialize→deserialize is a byte-fixed point"
        );
        assert_eq!(
            peek_storage_version(&first).as_deref(),
            Some(STORAGE_V0_135_0)
        );
    }

    /// A row whose body and field hold values a write's decode refuses loads
    /// them canonical, warned at each, and re-stores to bytes that load
    /// warning nothing; a row the write stored warns nothing.
    #[test]
    fn a_noncanonical_stored_value_loads_canonical_and_warns_where() {
        let mut row: serde_json::Value = serde_json::to_value(sample()).unwrap();
        let noncanonical = serde_json::json!({
            "text": "ab\ncd", "blocks": [{"id": "b0", "kind": "para"}, {"id": "b1", "kind": "para"}],
            "marks": [{"start": 3, "end": 5, "type": "strong"}, {"start": 0, "end": 2, "type": "strong"}, {"start": 1, "end": 1, "type": "emph"}],
        });
        row["main"]["body"] = noncanonical.clone();
        row["main"]["payload"]["items"].as_array_mut().unwrap().push(serde_json::json!({"type": "field", "key": "note", "value": noncanonical}));
        let json = row.to_string();
        assert!(ContentModel::from_value(&noncanonical).is_err());

        let parsed = Document::from_stored(&json).unwrap();
        let at: Vec<_> = parsed.warnings.iter().map(|w| (w.code.as_deref(), w.path.as_deref())).collect();
        assert_eq!(at, [(Some(super::super::NONCANONICAL_READ), Some("main.body")), (Some(super::super::NONCANONICAL_READ), Some("main.note"))]);
        let doc = parsed.document;
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
        let canonical = ContentModel::from_value_lenient(&noncanonical).unwrap().content;
        assert_eq!(doc.main().body(), &canonical);
        assert_eq!(doc.main().payload().get("note").map(|v| v.as_json().clone()), Some(canonical.to_value()));

        let restored = serde_json::to_string(&doc).unwrap();
        let again = Document::from_stored(&restored).unwrap();
        assert_eq!((again.document, again.warnings), (doc, Vec::new()));
        assert_eq!(Document::from_stored(&serde_json::to_string(&sample()).unwrap()).unwrap().warnings, []);
    }

    /// A document holding a composable card saves under the newest tag, each card's
    /// `$id` an `id` item; a stored card naming none mints one on load, the
    /// same id on every load; a bad, duplicated, or main-card id refuses.
    #[test]
    fn a_card_id_saves_under_the_newest_tag() {
        let doc = sample();
        let row: serde_json::Value = serde_json::to_value(&doc).unwrap();
        assert_eq!(row["schema"], STORAGE_V0_135_0);
        let id = doc.cards()[0].id().unwrap().to_string();
        assert!(
            row["cards"][0]["payload"]["items"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!({"type": "id", "value": id}))
        );

        let mut older = row.clone();
        older["cards"][0]["payload"]["items"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item["type"] != "id");
        let first: Document = serde_json::from_value(older.clone()).unwrap();
        let second: Document = serde_json::from_value(older).unwrap();
        assert_eq!(first.cards()[0].id(), second.cards()[0].id());
        assert!(first.cards()[0].id().is_some());

        let refuse = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut bad = row.clone();
            edit(&mut bad);
            serde_json::from_value::<Document>(bad)
                .unwrap_err()
                .to_string()
        };
        let set_id = |value: &str| {
            let value = value.to_string();
            move |row: &mut serde_json::Value| {
                for item in row["cards"][0]["payload"]["items"].as_array_mut().unwrap() {
                    if item["type"] == "id" {
                        item["value"] = value.clone().into();
                    }
                }
            }
        };
        assert!(refuse(&set_id("-x")).contains("invalid card id"));
        assert!(
            refuse(&|row| {
                let card = row["cards"][0].clone();
                row["cards"].as_array_mut().unwrap().push(card);
            })
            .contains("held by two cards")
        );
        assert!(
            refuse(&|row| {
                row["main"]["payload"]["items"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({"type": "id", "value": "m"}));
            })
            .contains("must not carry a $id")
        );
    }

    /// A row of the tag before the newest loads and saves under the newest. A
    /// body still in the line form fails the load, and a field in it loads as
    /// the plain value it spells, read as no content.
    #[test]
    fn a_0_134_0_row_saves_under_the_newest_tag_and_a_line_form_value_fails() {
        let mut row = serde_json::to_value(sample()).unwrap();
        row["schema"] = STORAGE_V0_134_0.into();
        let doc: Document = serde_json::from_value(row.clone()).unwrap();
        assert_eq!(doc, sample());
        assert_eq!(serde_json::to_value(&doc).unwrap()["schema"], STORAGE_V0_135_0);

        let lines = serde_json::json!({"text": "a", "lines": [{"kind": "para", "containers": []}], "marks": [], "islands": []});
        let mut in_body = row.clone();
        in_body["main"]["body"] = lines.clone();
        assert!(serde_json::from_value::<Document>(in_body).is_err());
        let mut in_field = row;
        in_field["main"]["payload"]["items"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "field", "key": "intro", "value": lines}));
        let loaded = serde_json::from_value::<Document>(in_field).unwrap();
        assert!(loaded.main().payload().get("intro").unwrap().as_json().get("lines").is_some());
    }

    /// A stored card naming no `$kind` fails the load.
    #[test]
    fn rejects_composable_card_without_kind() {
        let mut row = serde_json::to_value(sample()).unwrap();
        row["cards"][0]["payload"]["items"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item["type"] != "kind");
        let err = serde_json::from_value::<Document>(row).unwrap_err().to_string();
        assert!(err.contains("$kind"), "{err}");
    }
}
