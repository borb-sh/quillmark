//! The precondition on a revise: what its writer read.
//!
//! A revise diffs its markdown against the value stored now, so an edit made
//! between a writer's read and its write reads as text the writer deleted. A
//! [`Base`] names the read: [`Base::Hash`] refuses the write when the value has
//! changed since, and [`Base::Read`] merges the writer's edit onto the change.

use std::collections::BTreeMap;

use quillmark_content::delta::{merge_lines, rebase_marks_pinned};
use quillmark_content::export::{to_markdown, to_markdown_annotated, to_markdown_tokened, Annotated};
use quillmark_content::import::{from_markdown_against, ImportWarning};
use quillmark_content::model::Normalized;
use quillmark_content::serial::to_canonical_value;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

use super::edit::{is_valid_field_name, revise_import, EditError, Revised};
use super::revise::align_cards;
use super::{dropped_constructs, Card, Codec, Document, Parsed, Revising};
use crate::error::{diag_args, Diagnostic};
use crate::path::{DocPath, DocSeg};
use crate::quill::{BoundParseError, FieldSchema, FieldType, QuillConfig};

/// What a revise's writer read, checked against the value stored at the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base<'a> {
    /// The hash of the value the writer read: [`Card::body_hash`],
    /// [`Card::field_hash`], [`Document::hash`] or [`content_hash`] on the
    /// quill-free lane, and the typed writer's `hash` / `field_hash` on the
    /// typed one. The write is refused with [`BaseError::Stale`] when the value
    /// now hashes otherwise.
    Hash(&'a str),
    /// The markdown the writer read, annotated or not (a `text` field's literal
    /// text). When the value has changed since, the writer's edit of this
    /// markdown and the change since merge by line ([`merge_lines`]) and the
    /// merge is what the revise writes, so both land; runs of lines both
    /// changed refuse the write with [`BaseError::Conflict`].
    Read(&'a str),
}

/// A revise refused by its [`Base`]. Each carries the hash of the value stored
/// now, which a writer checks its next read against; retrying the same write
/// under it would land the revert the base refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BaseError {
    #[error("the value changed since the writer read it")]
    Stale {
        current: String,
        /// The field written, for the diagnostic's anchor; `None` for a body
        /// or a document.
        field: Option<String>,
    },
    #[error("the writer's edit and a change made since its read touch the same lines")]
    Conflict {
        current: String,
        field: Option<String>,
    },
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
            BaseError::Stale { current, .. } | BaseError::Conflict { current, .. } => {
                diag_args! { "current" => current }
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
    let mut bytes = format!("{HASH_TAG}\n").into_bytes();
    serde_json::to_writer(&mut bytes, &sorted(value)).expect("a JSON value serializes");
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
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

/// The hash of a content's canonical form: what [`Card::body_hash`] answers for
/// a body, and [`Card::field_hash`] for a richtext field, however the field
/// rests.
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
        Err(BaseError::Stale { current, field: None })
    }
}

/// The [`Write`] for a richtext revise of `stored`. Each markdown is read as a
/// revise of `stored` and exported with its islands that have no markdown as
/// their tokens and no anchor tag, so the three sides of a merge are spelled
/// alike, a token survives the merge, and a read of the stored value,
/// annotated or not, matches it: a read equal to the stored value's plain or
/// annotated markdown writes the writer's own, though its markup lost an
/// island its spelling cannot match.
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
    if read == to_markdown(stored) || read == to_markdown_annotated(stored).markdown {
        return Ok(Write::Own);
    }
    let spelled = |md: &str| {
        from_markdown_against(md, stored)
            .map(|against| {
                let content = rebase_marks_pinned(stored, against.content, &against.pins);
                (to_markdown_tokened(&content), against.warnings)
            })
            .map_err(EditError::Import)
    };
    let now = to_markdown_tokened(stored);
    let (read, _) = spelled(read)?;
    if read == now {
        return Ok(Write::Own);
    }
    let (new, warnings) = spelled(body)?;
    match merge_lines(&read, &now, &new) {
        Some(merged) => Ok(Write::Merged(merged, warnings)),
        None => Err(EditError::Base(BaseError::Conflict {
            current: current(),
            field: None,
        })),
    }
}

impl Revised {
    fn reporting(mut self, warnings: Vec<ImportWarning>) -> Self {
        self.warnings = dropped_constructs(warnings, None);
        self
    }
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
    pub fn body_hash(&self) -> String {
        content_hash(self.body())
    }

    /// The body's annotated read ([`to_markdown_annotated`]): the scoped read
    /// a body revise reads back, and the markdown a [`Base::Read`] merge of one
    /// takes. Its anchors carry no path; the body is theirs.
    pub fn body_annotated(&self) -> Annotated {
        to_markdown_annotated(self.body())
    }

    /// The richtext field's annotated read, decoded as
    /// [`revise_field`](Self::revise_field) decodes it, an absent field the
    /// empty content's: the scoped read a field revise reads back.
    /// **Richtext only**, as [`field_hash`](Self::field_hash) is.
    pub fn field_annotated(&self, name: &str) -> Result<Annotated, EditError> {
        Ok(to_markdown_annotated(&self.richtext_base(name)?))
    }

    /// The hash of the richtext field's content, decoded as
    /// [`revise_field`](Self::revise_field) decodes it, so a field resting as a
    /// markdown string and the canonical content it conforms to hash alike;
    /// an absent field hashes as the empty content. The [`Base::Hash`] a
    /// quill-free field revise checks. **Richtext only**, on
    /// [`revise_field`](Self::revise_field)'s terms: the typed writer's
    /// `field_hash` is the door for a field of another type.
    pub fn field_hash(&self, name: &str) -> Result<String, EditError> {
        Ok(content_hash(&self.richtext_base(name)?))
    }

    /// [`field_hash`](Self::field_hash) through the field's schema: a
    /// `richtext` field hashes its content, a `text` field its literal text,
    /// any other type its stored value, an absent one as `null`. The
    /// [`Base::Hash`] a typed field revise checks.
    #[doc(hidden)]
    pub fn field_hash_checked(&self, name: &str, schema: &FieldSchema) -> Result<String, EditError> {
        match schema.r#type {
            FieldType::RichText { .. } => self.field_hash(name),
            FieldType::Text { .. } => Ok(value_hash(&match self.text_now(name)? {
                Some(text) => JsonValue::String(text),
                None => JsonValue::Null,
            })),
            _ => {
                if !is_valid_field_name(name) {
                    return Err(EditError::InvalidFieldName(name.to_string()));
                }
                Ok(value_hash(
                    self.payload().get(name).map_or(&JsonValue::Null, |v| v.as_json()),
                ))
            }
        }
    }

    /// The `text` field's literal text, `None` for an absent field.
    fn text_now(&self, name: &str) -> Result<Option<String>, EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        match self.field_text(name, Codec::Text) {
            Some(Ok(text)) => Ok(Some(text)),
            Some(Err(e)) => Err(super::edit::field_decode(name, &[], Codec::Text, e)),
            None => Ok(None),
        }
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

    /// [`revise_field`](Self::revise_field) under `base`, its hash
    /// [`field_hash`](Self::field_hash).
    pub fn revise_field_against(
        &mut self,
        name: &str,
        base: Base<'_>,
        body: impl Into<String>,
    ) -> Result<Revised, EditError> {
        let body = body.into();
        let stored = self.richtext_base(name)?;
        let write = richtext_write(&stored, || content_hash(&stored), base, &body)
            .map_err(|e| at_field(e, name))?;
        match write {
            Write::Own => self.revise_field(name, body),
            Write::Merged(md, warnings) => Ok(self.revise_field(name, md)?.reporting(warnings)),
        }
    }

    /// [`revise_field_checked`](Self::revise_field_checked) under `base`, its
    /// hash [`field_hash_checked`](Self::field_hash_checked). A `text` field
    /// merges its literal text.
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
            let write = richtext_write(&stored, || content_hash(&stored), base, &body)
                .map_err(|e| at_field(e, name))?;
            return match write {
                Write::Own => self.revise_field_checked(name, body, schema),
                Write::Merged(md, warnings) => {
                    Ok(self.revise_field_checked(name, md, schema)?.reporting(warnings))
                }
            };
        }
        let current = || self.field_hash_checked(name, schema);
        let body = match base {
            Base::Hash(read) => {
                check(current()?, read).map_err(|e| EditError::Base(e.at_field(name)))?;
                body
            }
            Base::Read(read) => {
                let now = self.text_now(name)?.unwrap_or_default();
                if read == now {
                    body
                } else {
                    match merge_lines(read, &now, &body) {
                        Some(merged) => merged,
                        None => {
                            return Err(EditError::Base(BaseError::Conflict {
                                current: current()?,
                                field: Some(name.to_string()),
                            }))
                        }
                    }
                }
            }
        };
        self.revise_field_checked(name, body, schema)
    }
}

impl Document {
    /// The hash of the document's stored form, the [`Base::Hash`] a
    /// whole-document revise checks on the quill-free lane. The typed writer's
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
/// or the line merge of its edit onto what changed since its read, carrying
/// the writer's own parse warnings at their cards' places in the merge. Each
/// side is spelled as `doc` revised by it and conformed, its islands that
/// have no markdown as their tokens and no anchor tag, so a token survives
/// the merge and the three sides are spelled alike. A merge that does not
/// parse as a document, or names another quill, is a [`BaseError::Conflict`],
/// since the writer's own markdown parsed. A read equal to the document's
/// plain or annotated markdown, as stored or conformed, takes the writer's own.
pub(crate) fn parse_against(
    config: &QuillConfig,
    doc: &Document,
    base: Base<'_>,
    markdown: &str,
) -> Result<Revising, BoundParseError> {
    let hash = || conformed(config, doc).map(|d| d.hash());
    let read = match base {
        Base::Hash(read) => {
            check(hash()?, read)?;
            return Ok(Document::parse_revising(markdown)?);
        }
        Base::Read(read) => read,
    };
    let revising = Document::parse_revising(markdown)?;
    config.check_quill_reference(&revising.parsed.document)?;
    let reads_now = |doc: &Document| read == doc.to_markdown() || read == doc.to_markdown_annotated().markdown;
    if reads_now(doc) || reads_now(&conformed(config, doc)?) {
        return Ok(revising);
    }
    let spell = |side: Revising| -> Result<String, BoundParseError> {
        let mut revised = doc.clone();
        let _ = revised.revise_parsed(side);
        Ok(conformed(config, &revised)?.to_markdown_tokened())
    };
    let now = conformed(config, doc)?.to_markdown_tokened();
    let read = spell(Document::parse_revising(read)?)?;
    if read == now {
        return Ok(revising);
    }
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
    };
    let merged = merge_lines(&read, &now, &new).ok_or_else(conflict)?;
    let merged = Document::parse_revising(&merged).map_err(|_| conflict())?;
    config.check_quill_reference(&merged.parsed.document).map_err(|_| conflict())?;
    let pairs = align_cards(&merged.parsed.document.cards, &revising.parsed.document.cards);
    let warnings = revising
        .parsed
        .warnings
        .into_iter()
        .map(|w| match w.path.as_deref().and_then(|p| p.parse::<DocPath>().ok()) {
            Some(path) => match recard(&path, &pairs) {
                Some(moved) => w.with_path(moved.to_string()),
                None => w,
            },
            None => w,
        })
        .collect();
    Ok(Revising {
        parsed: Parsed {
            document: merged.parsed.document,
            warnings,
        },
        bodies: merged.bodies,
    })
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
