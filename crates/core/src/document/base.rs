//! The precondition on a revise: what its writer read.
//!
//! A revise diffs its markdown against the value stored now, so an edit made
//! between a writer's read and its write reads as text the writer deleted. A
//! [`Base`] names the read: [`Base::Hash`] refuses the write when the value has
//! changed since, and [`Base::Read`] merges the writer's edit onto the change.

use std::collections::BTreeMap;

use quillmark_content::delta::merge_lines;
use quillmark_content::export::to_markdown;
use quillmark_content::import::{from_markdown, ImportWarning};
use quillmark_content::model::Normalized;
use quillmark_content::serial::to_canonical_value;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use super::dto::STORAGE_V0_131_0;
use super::edit::{is_valid_field_name, revise_import, EditError, Revised};
use super::{dropped_constructs, Card, Codec, Document, Parsed};
use crate::error::{diag_args, Diagnostic};
use crate::quill::{BoundParseError, FieldSchema, FieldType};

/// What a revise's writer read, checked against the value stored at the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base<'a> {
    /// The hash of the value the writer read ([`Card::body_hash`],
    /// [`Card::field_hash`], [`Document::hash`], [`content_hash`]). The write
    /// is refused with [`BaseError::Stale`] when the value now hashes
    /// otherwise.
    Hash(&'a str),
    /// The markdown the writer read, annotated or not (a `text` field's literal
    /// text). When the value has changed since, the writer's edit of this
    /// markdown and the change since merge by line ([`merge_lines`]) and the
    /// merge is what the revise writes, so both land; runs of lines both
    /// changed refuse the write with [`BaseError::Conflict`].
    Read(&'a str),
}

/// A revise refused by its [`Base`]. Each carries the hash of the value stored
/// now, for a writer that reads again and retries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BaseError {
    #[error("the value changed since the writer read it")]
    Stale { current: String },
    #[error("the writer's edit and a change made since its read touch the same lines")]
    Conflict { current: String },
}

impl BaseError {
    pub fn code(&self) -> &'static str {
        match self {
            BaseError::Stale { .. } => "edit::stale_base",
            BaseError::Conflict { .. } => "edit::rebase_conflict",
        }
    }

    pub fn args(&self) -> BTreeMap<String, JsonValue> {
        match self {
            BaseError::Stale { current } | BaseError::Conflict { current } => {
                diag_args! { "current" => current }
            }
        }
    }

    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::new(crate::error::Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_args(self.args())
    }
}

/// The hash of a stored JSON value: SHA-256 over the newest storage tag, a
/// newline, and the value's JSON bytes, as lowercase hex. The tag scopes the
/// hash to the canonical form this build writes, so a build that changes it
/// changes every hash and refuses a write across the upgrade rather than
/// accepting one.
pub fn value_hash(value: &JsonValue) -> String {
    let mut bytes = format!("{STORAGE_V0_131_0}\n").into_bytes();
    serde_json::to_writer(&mut bytes, value).expect("a JSON value serializes");
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The hash of a content's canonical form: what [`Card::body_hash`] answers for
/// a body, and [`Card::field_hash`] for a richtext field at rest.
pub fn content_hash(content: &Normalized) -> String {
    value_hash(&to_canonical_value(content))
}

/// [`revise_import`] under `base`: refused when `base` is stale, a merge when
/// `base` is a read the stored value has moved on from.
pub fn revise_import_against(
    stored: &Normalized,
    base: Base<'_>,
    body: &str,
) -> Result<(Normalized, Revised), EditError> {
    let write = richtext_write(stored, || content_hash(stored), base, body)?;
    let revise = |md: &str| revise_import(stored, md).map_err(EditError::Import);
    match write {
        Write::Own => revise(body),
        Write::Merged(md, warnings) => {
            let (content, revised) = revise(&md)?;
            Ok((content, revised.reporting(warnings)))
        }
    }
}

/// What a guarded write lands: the writer's own markdown, or the merge with
/// the import warnings of the writer's markdown, which the merge no longer
/// spells.
enum Write {
    Own,
    Merged(String, Vec<ImportWarning>),
}

fn check(current: String, read: &str) -> Result<(), BaseError> {
    if current == read {
        Ok(())
    } else {
        Err(BaseError::Stale { current })
    }
}

/// The [`Write`] for a richtext revise of `stored`. Each markdown is read through
/// one import and export, so the three sides of a merge are spelled alike and a
/// read of the stored value, annotated or not, matches it.
fn richtext_write(
    stored: &Normalized,
    current: impl FnOnce() -> String,
    base: Base<'_>,
    body: &str,
) -> Result<Write, EditError> {
    let read = match base {
        Base::Hash(read) => return check(current(), read).map(|()| Write::Own).map_err(EditError::Base),
        Base::Read(read) => read,
    };
    let spelled = |md: &str| {
        from_markdown(md)
            .map(|imported| (to_markdown(&imported.content), imported.warnings))
            .map_err(EditError::Import)
    };
    let (now, _) = spelled(&to_markdown(stored))?;
    let (read, _) = spelled(read)?;
    if read == now {
        return Ok(Write::Own);
    }
    let (new, warnings) = spelled(body)?;
    match merge_lines(&read, &now, &new) {
        Some(merged) => Ok(Write::Merged(merged, warnings)),
        None => Err(EditError::Base(BaseError::Conflict { current: current() })),
    }
}

impl Revised {
    fn reporting(mut self, warnings: Vec<ImportWarning>) -> Self {
        self.warnings = dropped_constructs(warnings, None);
        self
    }
}

impl Card {
    /// The hash of the body's canonical form, the [`Base::Hash`] a body revise
    /// checks.
    pub fn body_hash(&self) -> String {
        content_hash(self.body())
    }

    /// The hash of the field's stored value, `null` for an absent field: the
    /// [`Base::Hash`] a field revise checks. Returns
    /// [`EditError::InvalidFieldName`] for a malformed name.
    pub fn field_hash(&self, name: &str) -> Result<String, EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        Ok(self.stored_hash(name))
    }

    fn stored_hash(&self, name: &str) -> String {
        value_hash(self.payload().get(name).map_or(&JsonValue::Null, |v| v.as_json()))
    }

    /// [`revise_body`](Self::revise_body) under `base`.
    pub fn revise_body_against(
        &mut self,
        base: Base<'_>,
        body: impl Into<String>,
    ) -> Result<Revised, EditError> {
        let (content, revised) = revise_import_against(self.body(), base, &body.into())?;
        self.overwrite_body(content);
        Ok(revised)
    }

    /// [`revise_field`](Self::revise_field) under `base`.
    pub fn revise_field_against(
        &mut self,
        name: &str,
        base: Base<'_>,
        body: impl Into<String>,
    ) -> Result<Revised, EditError> {
        let body = body.into();
        let stored = self.richtext_base(name)?;
        match richtext_write(&stored, || self.stored_hash(name), base, &body)? {
            Write::Own => self.revise_field(name, body),
            Write::Merged(md, warnings) => Ok(self.revise_field(name, md)?.reporting(warnings)),
        }
    }

    /// [`revise_field_checked`](Self::revise_field_checked) under `base`. A
    /// `text` field merges its literal text.
    #[doc(hidden)]
    pub fn revise_field_checked_against(
        &mut self,
        name: &str,
        base: Base<'_>,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Revised, EditError> {
        let body = body.into();
        if !matches!(schema.r#type, FieldType::Text { .. }) {
            let stored = self.richtext_base(name)?;
            return match richtext_write(&stored, || self.stored_hash(name), base, &body)? {
                Write::Own => self.revise_field_checked(name, body, schema),
                Write::Merged(md, warnings) => {
                    Ok(self.revise_field_checked(name, md, schema)?.reporting(warnings))
                }
            };
        }
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        let body = match base {
            Base::Hash(read) => {
                check(self.stored_hash(name), read).map_err(EditError::Base)?;
                body
            }
            Base::Read(read) => {
                let now = match self.field_text(name, Codec::Text) {
                    Some(Ok(text)) => text,
                    Some(Err(e)) => return Err(super::edit::field_decode(name, &[], Codec::Text, e)),
                    None => String::new(),
                };
                if read == now {
                    body
                } else {
                    merge_lines(read, &now, &body).ok_or_else(|| {
                        EditError::Base(BaseError::Conflict {
                            current: self.stored_hash(name),
                        })
                    })?
                }
            }
        };
        self.revise_field_checked(name, body, schema)
    }
}

impl Document {
    /// The hash of the document's stored form, the [`Base::Hash`] a
    /// whole-document revise checks.
    pub fn hash(&self) -> String {
        value_hash(&serde_json::to_value(self).expect("a document serializes"))
    }

    /// Parse `markdown` as a revise of this document under `base`: the
    /// writer's markdown, or the line merge of its edit onto what changed
    /// since its read, carrying the warnings of the writer's own parse. Each
    /// side is read through one parse and emit, as a richtext merge is.
    pub(crate) fn parse_against(
        &self,
        base: Base<'_>,
        markdown: &str,
    ) -> Result<Parsed, BoundParseError> {
        let read = match base {
            Base::Hash(read) => {
                check(self.hash(), read)?;
                return Ok(Document::parse(markdown)?);
            }
            Base::Read(read) => read,
        };
        let parsed = Document::parse(markdown)?;
        let now = Document::parse(&self.to_markdown())?.document.to_markdown();
        let read = Document::parse(read)?.document.to_markdown();
        if read == now {
            return Ok(parsed);
        }
        let new = parsed.document.to_markdown();
        let merged = merge_lines(&read, &now, &new)
            .ok_or_else(|| BaseError::Conflict { current: self.hash() })?;
        Ok(Parsed {
            document: Document::parse(&merged)?.document,
            warnings: parsed.warnings,
        })
    }
}
