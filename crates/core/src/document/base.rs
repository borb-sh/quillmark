//! The precondition on a write: the hash of what its writer read.
//!
//! Core is single-writer. A write that names a base lands only while the value
//! still hashes to it, and is refused with [`StaleBase`] otherwise,
//! carrying the hash now, so the writer reads again and writes from that.
//! Writers that answer late and must merge with what landed since go through
//! `@quillmark/collab`.

use std::collections::BTreeMap;

use quillmark_content::lines::export::Annotated;
use quillmark_content::model::Content;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use super::edit::{EditError, is_valid_field_name};
use super::{Card, Codec, Document, Receipt, Target};
use crate::error::{Diagnostic, diag_args};
use crate::quill::{BoundParseError, FieldSchema, FieldType, QuillConfig};

/// A write refused by its base: the value no longer hashes to what the writer
/// read. It carries the hash of the value stored now, which a writer checks
/// its next read against; retrying the same write under it would land the
/// revert the base refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the value changed since the writer read it")]
pub struct StaleBase {
    pub current: String,
    /// The field written, for the diagnostic's anchor; `None` for a body or a
    /// document.
    pub field: Option<String>,
}

impl StaleBase {
    pub fn code(&self) -> &'static str {
        "edit::stale_base"
    }

    pub fn args(&self) -> BTreeMap<String, JsonValue> {
        diag_args! { "current" => &self.current }
    }

    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::new(crate::error::Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_args(self.args())
    }
}

/// Refuses a write whose `base` is not `current`, the hash of the value now.
pub(crate) fn check_base(
    base: Option<&str>,
    current: impl FnOnce() -> Result<String, EditError>,
    field: Option<&str>,
) -> Result<(), EditError> {
    let Some(base) = base else { return Ok(()) };
    let current = current()?;
    if base == current {
        return Ok(());
    }
    Err(EditError::StaleBase(StaleBase {
        current,
        field: field.map(str::to_string),
    }))
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
    hashing
        .0
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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

impl Card {
    /// The hash of the body's canonical form, the base a body revise checks.
    pub(crate) fn body_hash(&self) -> String {
        match self.hashes.body() {
            Some(hash) => hash.to_string(),
            None => content_hash(self.body()),
        }
    }

    /// The body's annotated read ([`to_markdown_annotated`]): the scoped read
    /// a body revise takes back. Its anchors carry no path; the body is theirs.
    pub(crate) fn body_annotated(&self) -> Annotated {
        quillmark_content::markdown::to_markdown_annotated(self.body())
    }

    /// The field's annotated read, decoded as a schema-blind revise decodes
    /// it, an absent field the empty content's: the scoped read a field
    /// revise takes back. A field
    /// stored under `codec: "plain"` reads as its literal text, its anchors
    /// listed and no tag spelled.
    pub(crate) fn field_annotated(&self, name: &str) -> Result<Annotated, EditError> {
        Ok(quillmark_content::markdown::to_text_annotated(
            &self.richtext_value(name)?,
        ))
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
    /// type its stored value, an absent one as `null`: the
    /// base a typed field revise checks.
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

    /// [`revise_body`](Self::revise_body) while the body hashes to `base`.
    pub(crate) fn revise_body_against(
        &mut self,
        base: &str,
        body: impl Into<String>,
    ) -> Result<Receipt, EditError> {
        check_base(Some(base), || Ok(self.body_hash()), None)?;
        self.revise_counted(Target::Body, &body.into())
    }

    /// [`revise_field_checked`](Self::revise_field_checked) while the field
    /// hashes to `base`, its hash [`field_hash_checked`](Self::field_hash_checked).
    #[doc(hidden)]
    pub fn revise_field_checked_against(
        &mut self,
        name: &str,
        base: &str,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Receipt, EditError> {
        check_base(
            Some(base),
            || self.field_hash_checked(name, schema),
            Some(name),
        )?;
        self.revise_field_checked(name, body, schema)
    }
}

impl Document {
    /// The hash of the document's stored form, the base a whole-document
    /// revise checks on the quill-free write. The typed writer's
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
