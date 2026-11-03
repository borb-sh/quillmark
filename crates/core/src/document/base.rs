//! The precondition on a revise: what its writer read.
//!
//! A revise diffs its markdown against the value stored now, so an edit made
//! between a writer's read and its write reads as text the writer deleted. A
//! [`Base`] names the read, and the write rebases over the change since: the
//! change the card's log holds from a [`Base::Hash`], or the diff from a
//! [`Base::Content`] to the value now. A whole-document revise names its read
//! with a [`DocumentBase`].

use std::collections::BTreeMap;

use quillmark_content::lines::export::Annotated;
use quillmark_content::model::Content;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use super::edit::{EditError, is_valid_field_name};
use super::merge::{MergeConflict, merge_documents};
use super::revise::align_cards;
use super::{Card, Codec, Document, Parsed, Receipt, Revising, Target};
use crate::error::{Diagnostic, diag_args};
use crate::path::{DocPath, DocSeg};
use crate::quill::{BoundParseError, FieldSchema, FieldType, QuillConfig};

/// What a revise's writer read, checked against the value stored at the write.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Base<'a> {
    /// The hash of the value the writer read: [`Card::hash_of`] or
    /// [`content_hash`] on the content write, and the typed writer's
    /// `hash_of`. A value that now hashes otherwise rebases the write over
    /// the change the card's log holds since: refused with
    /// [`BaseError::Conflict`] where the two touch, and with
    /// [`BaseError::Stale`] where the log no longer reaches the base.
    Hash(&'a str),
    /// The value the writer read, in its canonical form (a `text` field's
    /// content spelling `codec: "plain"`). A value that has moved on from it
    /// rebases the write over the diff from it to the value now, so the base
    /// holds however long ago it was read: refused with [`BaseError::Conflict`]
    /// where the two touch, never stale.
    Content(&'a Content),
}

/// What a whole-document revise's writer read.
#[derive(Debug, Clone, Copy)]
pub enum DocumentBase<'a> {
    /// The document's hash, [`Document::hash`] or the typed writer's `hash`.
    /// A document that now hashes otherwise refuses the write with
    /// [`BaseError::Stale`]: a whole document keeps no log.
    Hash(&'a str),
    /// The document the writer read, as stored. The writer's edit of it and
    /// the change made since merge card by card and field by field; a place
    /// both changed keeps its value now and is named in
    /// [`DocumentRevised::conflicts`](super::DocumentRevised::conflicts).
    Stored(&'a Document),
}

/// A revise refused by its [`Base`]. Each carries the hash of the value stored
/// now, which a writer checks its next read against; retrying the same write
/// under it would land the revert the base refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BaseError {
    #[error("the value changed since the writer read it")]
    Stale {
        current: String,
        /// The field written, for the diagnostic's anchor; `None` for a body
        /// or a document.
        field: Option<String>,
    },
    #[error("the writer's edit and a change made since its read touch the same text or record")]
    Conflict {
        current: String,
        field: Option<String>,
        /// The step of the writer's change that conflicts, and why
        /// ([`Reason::name`](quillmark_content::rebase::Reason::name)), for a
        /// content write rebased over the change since; `None` for a merge
        /// by line.
        step: Option<usize>,
        reason: Option<&'static str>,
    },
}

impl BaseError {
    /// The refusal of a content write whose change, rebased over the change
    /// since its base, conflicts with it; `current` is the hash of the value
    /// now.
    pub fn rebase_conflict(
        current: String,
        field: Option<String>,
        conflict: &quillmark_content::rebase::Conflict,
    ) -> Self {
        BaseError::Conflict {
            current,
            field,
            step: Some(conflict.step),
            reason: Some(conflict.reason.name()),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            BaseError::Stale { .. } => "edit::stale_base",
            BaseError::Conflict { .. } => "edit::rebase_conflict",
        }
    }

    pub fn args(&self) -> BTreeMap<String, JsonValue> {
        match self {
            BaseError::Stale { current, .. } => diag_args! { "current" => current },
            BaseError::Conflict { current, step, reason, .. } => {
                let mut args = diag_args! { "current" => current };
                if let Some(step) = step {
                    args.insert("step".into(), (*step).into());
                }
                if let Some(reason) = reason {
                    args.insert("reason".into(), (*reason).into());
                }
                args
            }
        }
    }

    pub(crate) fn field(&self) -> Option<&str> {
        match self {
            BaseError::Stale { field, .. } | BaseError::Conflict { field, .. } => field.as_deref(),
        }
    }

    fn at_field(mut self, name: &str) -> Self {
        match &mut self {
            BaseError::Stale { field, .. } | BaseError::Conflict { field, .. } => {
                *field = Some(name.to_string());
            }
        }
        self
    }

    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::new(crate::error::Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_args(self.args())
    }
}

/// The tag [`value_hash`] prefixes, versioning the hash scheme itself: the
/// bytes hashed, their key order and the digest.
pub const HASH_TAG: &str = "quillmark/hash@1";

/// The hash of a JSON value: SHA-256 over [`HASH_TAG`], a newline, and the
/// value's JSON with every object's keys sorted, as lowercase hex. Sorting
/// makes equal values hash alike however a store reordered their keys.
pub fn value_hash(value: &JsonValue) -> String {
    digest(&sorted(value))
}

/// [`value_hash`] of a value whose keys are sorted already.
pub(super) fn digest(sorted: &JsonValue) -> String {
    digesting(|w| serde_json::to_writer(w, sorted).map_err(std::io::Error::other))
}

/// SHA-256 over [`HASH_TAG`], a newline, and the bytes `write` writes, fed
/// to the hasher as they are written, as lowercase hex.
fn digesting(write: impl FnOnce(&mut Hashing) -> std::io::Result<()>) -> String {
    let mut hashing = Hashing(Sha256::new());
    hashing.0.update(HASH_TAG.as_bytes());
    hashing.0.update(b"\n");
    write(&mut hashing).expect("writing to a hasher does not fail");
    hashing.0.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A hasher taking bytes as a writer.
struct Hashing(Sha256);

impl std::io::Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn sorted(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            JsonValue::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&map[k])))
                    .collect(),
            )
        }
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The hash of a content's canonical form: what [`Card::hash_of`] answers for
/// a body or a richtext field, however the field rests.
pub fn content_hash(content: &Content) -> String {
    digesting(|w| content.write_canonical(w))
}

fn at_field(err: EditError, name: &str) -> EditError {
    match err {
        EditError::Base(e) => EditError::Base(e.at_field(name)),
        other => other,
    }
}

impl Card {
    /// The hash of the body's canonical form, the [`Base::Hash`] a body revise
    /// checks.
    pub(crate) fn body_hash(&self) -> String {
        match self.hashes.body() {
            Some(hash) => hash.to_string(),
            None => content_hash(self.body()),
        }
    }

    /// The body's annotated read ([`to_markdown_annotated`]): the scoped read
    /// a body revise reads back    /// takes. Its anchors carry no path; the body is theirs.
    pub(crate) fn body_annotated(&self) -> Annotated {
        quillmark_content::markdown::to_markdown_annotated(self.body())
    }

    /// The field's annotated read, decoded as a schema-blind revise decodes
    /// it, an absent field the
    /// empty content's: the scoped read a field revise reads back. A field
    /// stored under `codec: "plain"` reads as its literal text, its anchors
    /// listed and no tag spelled.
    pub(crate) fn field_annotated(&self, name: &str) -> Result<Annotated, EditError> {
        Ok(quillmark_content::markdown::to_text_annotated(&self.richtext_value(name)?))
    }

    /// The hash of the richtext field's content, decoded as a schema-blind
    /// revise decodes it, so a field resting as a markdown string and the
    /// canonical content it conforms to hash alike; an absent field hashes as
    /// the empty content. **Richtext only**: the typed writer's `hash_of` is
    /// the door for a field of another type.
    pub(crate) fn field_hash(&self, name: &str) -> Result<String, EditError> {
        match self.hashes.field(name) {
            Some(hash) => Ok(hash.to_string()),
            None => Ok(content_hash(&self.richtext_value(name)?)),
        }
    }

    /// [`field_hash`](Self::field_hash) through the field's schema: a content
    /// field hashes its content, a `text` field's authored string read as
    /// literal text and an absent one as the empty plain content, any other
    /// type its stored value, an absent one as `null`. The
    /// [`Base::Hash`] a typed field revise checks.
    #[doc(hidden)]
    pub fn field_hash_checked(
        &self,
        name: &str,
        schema: &FieldSchema,
    ) -> Result<String, EditError> {
        match schema.r#type {
            FieldType::RichText { .. } => self.field_hash(name),
            FieldType::Text { .. } => match self.hashes.field(name) {
                Some(hash) => Ok(hash.to_string()),
                None => {
                    if !is_valid_field_name(name) {
                        return Err(EditError::InvalidFieldName(name.to_string()));
                    }
                    let content = match self.payload().get(name) {
                        None => Content::empty_as(Codec::Text.content()),
                        Some(v) => Codec::Text
                            .decode_content(v.as_json())
                            .map_err(|e| super::edit::field_decode(name, &[], Codec::Text, e))?,
                    };
                    Ok(content_hash(&content))
                }
            },
            _ => {
                if !is_valid_field_name(name) {
                    return Err(EditError::InvalidFieldName(name.to_string()));
                }
                Ok(value_hash(
                    self.payload()
                        .get(name)
                        .map_or(&JsonValue::Null, |v| v.as_json()),
                ))
            }
        }
    }

    /// [`revise_body`](Self::revise_body) under `base`.
    pub(crate) fn revise_body_against(&mut self, base: Base<'_>, body: impl Into<String>) -> Result<Receipt, EditError> {
        self.revise_counted_from(Target::Body, &body.into(), Some(base))
    }

    /// [`revise_field_checked`](Self::revise_field_checked) under `base`, its
    /// hash [`field_hash_checked`](Self::field_hash_checked). A `text` field
    /// merges its literal text; a field of any other type but `richtext`
    /// revises as it does with no base.
    #[doc(hidden)]
    pub fn revise_field_checked_against(
        &mut self,
        name: &str,
        base: Base<'_>,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Receipt, EditError> {
        let body = body.into();
        if !matches!(
            schema.r#type,
            FieldType::Text { .. } | FieldType::RichText { .. }
        ) {
            return self.revise_field_checked(name, body, schema);
        }
        let read = match base {
            Base::Hash(read) => read.to_string(),
            Base::Content(read) => content_hash(read),
        };
        if read == self.field_hash_checked(name, schema)? {
            return self.revise_field_checked(name, body, schema);
        }
        let body = match schema.r#type {
            FieldType::Text { .. } => super::edit::committed_text(name, body, schema)?,
            _ => body,
        };
        self.rebase_checked(name, &body, base, schema)
    }

    /// A typed field revise of `body` under a `base` the field has moved on
    /// from: the content revise rebased over the change since, held to the
    /// field's schema, or nothing written.
    fn rebase_checked(&mut self, name: &str, body: &str, base: Base<'_>, schema: &FieldSchema) -> Result<Receipt, EditError> {
        let mut card = self.clone();
        let receipt = card.revise_counted_from(Target::Field(name), body, Some(base)).map_err(|e| at_field(e, name))?;
        if let Some(value) = card.payload().get(name) {
            super::edit::resolve_field_write(name, value, schema, &DocPath::new())?;
        }
        *self = card;
        Ok(receipt)
    }
}

impl Document {
    /// The hash of the document's stored form, the [`Base::Hash`] a
    /// whole-document revise checks on the quill-free write. The typed writer's
    /// `hash` conforms first, so a document loaded without its quill hashes
    /// as its conformed self.
    pub fn hash(&self) -> String {
        value_hash(&serde_json::to_value(self).expect("a document serializes"))
    }
}

/// `doc` conformed against `config`, for a hash or a spelling that does not
/// depend on whether the stored document has been conformed yet.
pub(crate) fn conformed(config: &QuillConfig, doc: &Document) -> Result<Document, BoundParseError> {
    let mut doc = doc.clone();
    config.conform(&mut doc)?;
    Ok(doc)
}

/// Parse `markdown` as a revise of `doc` under `base`: the writer's markdown,
/// or the merge of its edit onto what changed since its read, carrying the
/// writer's own parse warnings at their cards' places in the merge. The read
/// and the value now are spelled as their conformed annotated reads, and the
/// writer's edit as the read revised by it, so the three sides are spelled
/// alike, a token survives the merge, and a tag the writer moved pins where
/// the merge keeps its line. A merge that does not parse as a document, or
/// names another quill, is a [`BaseError::Conflict`], since the writer's own
/// markdown parsed. A read that conforms to the document now takes the
/// writer's own.
pub(crate) fn parse_against(
    config: &QuillConfig,
    doc: &Document,
    base: DocumentBase<'_>,
    markdown: &str,
) -> Result<(Revising, Vec<MergeConflict>), BoundParseError> {
    let hash = || conformed(config, doc).map(|d| d.hash());
    let stored = match base {
        DocumentBase::Hash(read) => {
            let current = hash()?;
            if current != read {
                return Err(BaseError::Stale { current, field: None }.into());
            }
            return Ok((Document::parse_revising(markdown)?, Vec::new()));
        }
        DocumentBase::Stored(read) => read,
    };
    let revising = Document::parse_revising(markdown)?;
    config.check_quill_reference(&revising.parsed.document)?;
    let read = conformed(config, stored)?;
    let now = conformed(config, doc)?;
    if read.hash() == now.hash() {
        return Ok((revising, Vec::new()));
    }
    let spell = |side: Revising| -> Result<String, BoundParseError> {
        let mut revised = read.clone();
        let _ = revised.revise_parsed(side);
        Ok(conformed(config, &revised)?
            .to_markdown_annotated()
            .markdown)
    };
    let now = now.to_markdown_annotated().markdown;
    let read = read.to_markdown_annotated().markdown;
    let new = spell(Revising {
        parsed: Parsed {
            document: revising.parsed.document.clone(),
            warnings: Vec::new(),
        },
        bodies: revising.bodies.clone(),
    })?;
    let conflict = || BaseError::Conflict {
        current: hash().unwrap_or_default(),
        field: None,
        step: None,
        reason: None,
    };
    let parse = |markdown: &str| Document::parse_revising(markdown).map_err(|_| conflict());
    let (merged, conflicts) =
        merge_documents(config, parse(&read)?, parse(&now)?, parse(&new)?).ok_or_else(conflict)?;
    config
        .check_quill_reference(&merged.parsed.document)
        .map_err(|_| conflict())?;
    let pairs = align_cards(
        &merged.parsed.document.cards,
        &revising.parsed.document.cards,
    );
    let warnings = revising
        .parsed
        .warnings
        .into_iter()
        .map(
            |w| match w.path.as_deref().and_then(|p| p.parse::<DocPath>().ok()) {
                Some(path) => match recard(&path, &pairs) {
                    Some(moved) => w.with_path(moved.to_string()),
                    None => w,
                },
                None => w,
            },
        )
        .collect();
    let merged = Revising {
        parsed: Parsed {
            document: merged.parsed.document,
            warnings,
        },
        bodies: merged.bodies,
    };
    Ok((merged, conflicts))
}

/// `path` with its card index moved through `pairs`, the writer's cards to the
/// merge's; `None` where it names no paired card.
fn recard(path: &DocPath, pairs: &[Option<(usize, super::align::Pairing)>]) -> Option<DocPath> {
    let Some(DocSeg::Card { index, .. }) = path.segs().first() else {
        return None;
    };
    let (to, _) = (*pairs.get(*index)?)?;
    path.with_card_index(to)
}
