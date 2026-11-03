//! The precondition on a revise: what its writer read.
//!
//! A revise diffs its markdown against the value stored now, so an edit made
//! between a writer's read and its write reads as text the writer deleted. A
//! [`Base`] names the read by its hash or by the value itself, and the write
//! lands at the heads the document noted that hash at, merged with the change
//! since (`land.rs`). A whole-document revise names its read with a
//! [`DocumentBase`].

use std::collections::BTreeMap;

use quillmark_content::lines::export::Annotated;
use quillmark_content::model::Content;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use super::edit::{EditError, is_valid_field_name};
use super::{Card, Codec, Document};
use crate::error::{Diagnostic, diag_args};
use crate::quill::{BoundParseError, FieldSchema, FieldType, QuillConfig};

/// What a revise's writer read, checked against the value stored at the write.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Base<'a> {
    /// The hash of the value the writer read: [`Card::hash_of`] or
    /// [`content_hash`] on the content write, and the typed writer's
    /// `hash_of`. A value that now hashes otherwise merges the write with
    /// the change since, at the heads a write left the hash at; refused with
    /// [`BaseError::Stale`] where the document's history does not reach it.
    Hash(&'a str),
    /// The value the writer read, in its canonical form (a `text` field's
    /// content spelling `codec: "plain"`), found by its hash among the heads
    /// the document noted, and merged as a [`Base::Hash`] is.
    Content(&'a Content),
}

/// What a whole-document revise's writer read.
#[derive(Debug, Clone, Copy)]
pub enum DocumentBase<'a> {
    /// The document's hash, [`Document::hash`] or the typed writer's `hash`.
    /// A document that now hashes otherwise refuses the write with
    /// [`BaseError::Stale`]: a whole document keeps no history.
    Hash(&'a str),
    /// The document the writer read. The writer's edit of it lands as one
    /// Automerge change at the heads it was read at, merged with every change
    /// since; one this document's history does not reach refuses with
    /// [`BaseError::Stale`] unless it reads as the document now.
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
}

impl BaseError {
    pub fn code(&self) -> &'static str {
        match self {
            BaseError::Stale { .. } => "edit::stale_base",
        }
    }

    pub fn args(&self) -> BTreeMap<String, JsonValue> {
        match self {
            BaseError::Stale { current, .. } => diag_args! { "current" => current },
        }
    }

    pub(crate) fn field(&self) -> Option<&str> {
        match self {
            BaseError::Stale { field, .. } => field.as_deref(),
        }
    }

    pub(crate) fn at_field(mut self, name: &str) -> Self {
        match &mut self {
            BaseError::Stale { field, .. } => *field = Some(name.to_string()),
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

/// The hash of a content's canonical form, each id its document gave a part
/// read as its place: what [`Card::hash_of`] answers for a body or a
/// richtext field, however the field rests.
pub fn content_hash(content: &Content) -> String {
    let placed = content.by_place();
    digesting(|w| placed.write_canonical(w))
}

impl Card {
    /// The hash of the body's canonical form, the [`Base::Hash`] a body revise
    /// checks.
    pub(crate) fn body_hash(&self) -> String {
        content_hash(self.body())
    }

    /// The body's annotated read ([`to_markdown_annotated`]): the scoped read
    /// a body revise reads back. Its anchors carry no path; the body is theirs.
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
        Ok(content_hash(&self.richtext_value(name)?))
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
            FieldType::Text { .. } => {
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
