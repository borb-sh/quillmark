//! Typed mutators for [`Document`] and [`Card`].
//!
//! Every successful mutator leaves user field names matching
//! `[A-Za-z_][A-Za-z0-9_]*`, composable `$kind`s valid, values inside the §8
//! depth bound, and each card within the §8 field count
//! ([`MAX_FIELD_COUNT`](crate::error::MAX_FIELD_COUNT)), so the result is safely
//! serializable to the backend plate and re-readable by the parser. A write
//! that would create a field past the count is [`EditError::InvalidPayload`].
//! `$ext`/`$seed` are opaque mappings that carry no field-name invariant, but
//! do carry the depth bound.

use std::collections::BTreeMap;

use quillmark_content::lines::import::ImportError;
#[cfg(test)]
use quillmark_content::lines::model::Lines;

use crate::document::base::{Base, BaseError};
use crate::document::meta::{CardKindError, validate_composable_kind};
use crate::document::payload::MetaKey;
use crate::document::{Card, Codec, ContentDecodeError, Document, Payload, PayloadItem};
use crate::error::Diagnostic;
use crate::error::diag_args;
use crate::path::DocPath;
use crate::quill::{CoercionError, FieldSchema, FieldType, Leniency, QuillConfig};
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;

/// A field plus its in-field path, rendered through
/// [`DocPath`] (`recipients[0].name`), so a message names
/// the address its anchor does.
fn render_at(field: &str, at: &[PathSegment]) -> String {
    at.iter()
        .fold(crate::path::DocPath::new().field(field), |p, seg| {
            p.segment(seg)
        })
        .to_string()
}

/// `true` if `name` matches `[A-Za-z_][A-Za-z0-9_]*`, tested on `name`'s own
/// characters: the parser reads a key's raw bytes, so a name that merely
/// normalises to ASCII (`U+212A KELVIN SIGN`) is not a top-level key.
///
/// Case is preserved verbatim; only `$`-prefixed keys are reserved, so a user
/// field can never shadow system metadata.
pub fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// The `richtext` codec name carried by [`EditError::FieldDecode`] and
/// [`EditError::FieldNotSingleLine`]. Frozen with the diagnostic arg: it is the
/// schema keyword, not a display string.
pub const CODEC_RICHTEXT: &str = "richtext";

/// The `text` codec. See [`CODEC_RICHTEXT`].
pub const CODEC_TEXT: &str = "text";

/// Errors returned by document and card mutators.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    #[error("invalid field name '{0}': must match [A-Za-z_][A-Za-z0-9_]*")]
    InvalidFieldName(String),

    /// A typed write ([`TypedWriter::set`](crate::writer::TypedWriter::set) /
    /// [`CardWriter::set`](crate::writer::CardWriter::set)) or a schema-bound
    /// read addressed a well-formed name the bound schema does not declare (or
    /// a card whose `$kind` carries no schema). A property an `object` field
    /// does not declare is the same error one level down, `at` naming it. Use
    /// [`Card::store_field`](Card::store_field) for opaque storage.
    #[error("field '{}' is not declared in the schema", render_at(.field, .at))]
    UnknownField {
        field: String,
        /// The steps from the field to the undeclared name, its last segment
        /// being that name: empty when the field itself is the undeclared one,
        /// `[Key("zip")]` for an `address` object that declares no `zip`. Rides
        /// the [`doc_path`](Self::doc_path) anchor as its own segments, so
        /// `field` stays the field's name.
        at: Vec<PathSegment>,
    },

    #[error("invalid card kind '{0}': must match [a-z_][a-z0-9_]*")]
    InvalidKindName(String),

    #[error("card kind 'main' is reserved for the document root")]
    ReservedKind,

    /// A card placed as a composable card carries a `$` entry that binds the
    /// document root (`$quill`, `$seed`).
    #[error("'{key}' is carried by the document root only, not by a composable card")]
    RootOnlyEntry { key: String },

    #[error("index {index} is out of range (len = {len})")]
    IndexOutOfRange { index: usize, len: usize },

    /// A card placed with a `$id` outside the card-id grammar
    /// ([`is_card_id`](super::card_id::is_card_id)).
    #[error(
        "invalid card id '{0}': must be 1 to 64 of [A-Za-z0-9_-], opening with a letter or digit"
    )]
    InvalidCardId(String),

    /// A card placed with a `$id` another card of the document holds.
    #[error("card id '{0}' is already held by another card")]
    DuplicateCardId(String),

    /// A write naming a card by a `$id` no composable card of the document
    /// holds.
    #[error("no card holds the id '{0}'")]
    UnknownCardId(String),

    /// A card placed in a document already holding
    /// [`MAX_CARD_COUNT`](crate::error::MAX_CARD_COUNT) composable cards, past
    /// which its markdown fails `parse::too_many_cards`.
    #[error("the document already holds the maximum of {max} cards")]
    TooManyCards { max: usize },

    #[error("value nests deeper than the maximum of {max} levels")]
    ValueTooDeep { max: usize },

    /// Markdown import failed: the markdown codec rejected the input for a body
    /// *or* a field path (e.g. container nesting past
    /// [`MAX_NESTING_DEPTH`](quillmark_content::MAX_NESTING_DEPTH)). Returned
    /// instead of silently degrading the target to empty on a rejected import.
    #[error("markdown import failed: {0}")]
    Import(ImportError),

    /// A value could not become the field's content through `codec`: a JSON
    /// object that is not a canonical content, a markdown string that failed to
    /// import, a shape that is neither object nor string, or formatting under
    /// the plain-only `text` codec. `message` names which.
    ///
    /// `codec` is the declared type's, not the stored shape's. Absence is not
    /// this error: a missing field is `None`, and reads as the empty content.
    #[error("{codec} field '{}': {message}", render_at(.field, .at))]
    FieldDecode {
        field: String,
        /// The steps from the field to the value that failed, empty when the
        /// field itself is the value. Rides the [`doc_path`](Self::doc_path)
        /// anchor as its own segments, so `field` stays the field's name.
        at: Vec<PathSegment>,
        /// The codec that ran, named by the field's declared type:
        /// [`CODEC_RICHTEXT`] or [`CODEC_TEXT`].
        codec: String,
        message: String,
    },

    /// A `Content` read
    /// ([`TypedReader::get_content`](crate::reader::TypedReader::get_content))
    /// addressed a field whose declared type is not a content *leaf*. The
    /// schema answers before the payload is consulted, and the test is narrower
    /// than the subtree walk: an `array<richtext>` carries content yet has no
    /// single `Content`, so it lands here. Address one of its elements with
    /// [`get_content_at`](crate::reader::TypedReader::get_content_at), which
    /// raises this in turn for a path that resolves to no content leaf.
    #[error("field '{}' is declared '{declared}', which is not a content field", render_at(.field, .at))]
    FieldNotContent {
        field: String,
        /// The steps from the field to the addressed value, empty when the field
        /// itself is addressed. `declared` names the type reached, so a
        /// `string[]` element reports `string` rather than the field's `array`.
        at: Vec<PathSegment>,
        declared: String,
    },

    /// A content field written under a one-line schema (no `multiline: true`)
    /// decoded to a multi-line content. Both text codecs carry the constraint,
    /// so one code covers both and `codec` says which codec raised it.
    #[error(
        "{codec} field '{field}' is not a single line: {codec} requires a single line with \
         no container or island; declare multiline: true to admit more"
    )]
    FieldNotSingleLine {
        field: String,
        /// The codec whose one-line constraint was violated: [`CODEC_RICHTEXT`]
        /// or [`CODEC_TEXT`].
        codec: String,
    },

    /// A typed write could not coerce the value to the field's schema type: the
    /// general failure for scalar/array/object types (a `"x"` for an `integer`,
    /// a non-object for an `object`, …). Content fields report through
    /// [`FieldDecode`](Self::FieldDecode) / [`FieldNotSingleLine`](Self::FieldNotSingleLine)
    /// instead.
    #[error("field '{field}' could not be coerced to its schema type: {message}")]
    FieldCoercionFailed {
        field: String,
        /// The schema type the write was coerced against; `message` alone is
        /// English.
        target: String,
        message: String,
    },

    /// A content [`Change`](quillmark_content::change::Change) the value
    /// refused; the value is as it was.
    #[error("content change refused: {0}")]
    Change(quillmark_content::change::ApplyError),

    /// Markdown a write imports past
    /// [`MAX_MARKDOWN_SIZE`](quillmark_content::MAX_MARKDOWN_SIZE) bytes, or a
    /// content value a binding takes whose text, its slots' included, is.
    #[error("an input of {size} bytes is past the limit of {max}")]
    InputTooLarge { size: usize, max: usize },

    /// A change a binding takes holding more than
    /// [`MAX_CHANGE_STEPS`](crate::document::MAX_CHANGE_STEPS) steps, nested
    /// ones counted.
    #[error("a change of {count} steps is past the limit of {max}")]
    TooManySteps { count: usize, max: usize },

    /// A value whose text holds more marks than
    /// [`max_marks`](quillmark_content::max_marks) of its length, or a change
    /// that would leave one.
    #[error("{count} marks over a text that holds at most {max}")]
    TooManyMarks { count: usize, max: usize },

    /// A value holding more than
    /// [`MAX_ANCHORS`](quillmark_content::MAX_ANCHORS) anchors, or a change
    /// that would leave one.
    #[error("{count} anchors, past the limit of {max}")]
    TooManyAnchors { count: usize, max: usize },

    /// The card's item list violates an invariant of the list as a whole. An
    /// input path taking a whole payload at once ([`Card::try_from`] a
    /// [`CardWire`](crate::document::CardWire)) reaches every variant; a field
    /// write reaches [`PayloadViolation::TooManyFields`], the one invariant a
    /// caller holding a `(name, value)` pair cannot check for itself.
    #[error("{0}")]
    InvalidPayload(PayloadViolation),

    /// A revise or replace whose change removes an `x-` island. Markdown
    /// cannot spell one, and a host's own model may hold none, so the writer
    /// may never have seen it; removing one takes a change of its own through
    /// [`Card::apply`].
    #[error("the write removes the {kind} island '{island}', which its writer may not have seen; remove it with apply")]
    DropsExtIsland { island: String, kind: String },

    /// A write's or a card placement's actor is not an
    /// [actor](quillmark_content::ids::is_actor).
    #[error("invalid actor '{0}': must be 1 to 57 of [A-Za-z0-9_-], opening with a letter or digit")]
    InvalidActor(String),

    /// Bytes or a sync message the document's Automerge document does not
    /// take: `reason` is the refusal's name, `decode` where they do not
    /// decode, `shape` where the document they leave holds no Quillmark
    /// document in the shape this build writes, and `schema` where it names
    /// a schema this build does not read.
    #[error("the document refuses the bytes ({reason}): {detail}")]
    BadLiveBytes { reason: &'static str, detail: String },

    /// A revise's [`Base`] refused the write.
    #[error(transparent)]
    Base(BaseError),
}

impl EditError {
    /// A refused change, as [`TooManyMarks`](Self::TooManyMarks) or
    /// [`TooManyAnchors`](Self::TooManyAnchors) where it passed a bound.
    pub fn change(e: quillmark_content::change::ApplyError) -> Self {
        match &e {
            quillmark_content::change::ApplyError::Invalid(inv) => Self::bound(inv).unwrap_or(EditError::Change(e)),
            _ => EditError::Change(e),
        }
    }

    /// The refusal of bytes or a sync message the document does not take.
    pub fn bad_live_bytes(e: quillmark_automerge::model::LoadError) -> Self {
        EditError::BadLiveBytes { reason: e.name(), detail: e.to_string() }
    }

    /// A refused import, as [`InputTooLarge`](Self::InputTooLarge) where its
    /// markdown passed the size bound.
    pub fn import(e: ImportError) -> Self {
        match e {
            ImportError::TooLarge { size, max } => EditError::InputTooLarge { size, max },
            e => EditError::Import(e),
        }
    }

    /// The error a content bound `inv` names, `None` for an invariant that
    /// is not one.
    pub fn bound(inv: &quillmark_content::model::Invariant) -> Option<Self> {
        use quillmark_content::model::Invariant;
        match *inv {
            Invariant::TooManyMarks { marks, max } => Some(EditError::TooManyMarks { count: marks, max }),
            Invariant::TooManyAnchors { anchors, max } => Some(EditError::TooManyAnchors { count: anchors, max }),
            _ => None,
        }
    }

    pub(crate) fn unknown_field(name: impl Into<String>) -> Self {
        EditError::UnknownField {
            field: name.into(),
            at: Vec::new(),
        }
    }

    /// The namespaced diagnostic `code` (e.g. `"edit::invalid_field_name"`),
    /// one per variant and the variant's only stable discriminator. Consumers
    /// route on this, not on message text. Taxonomy: `prose/canon/ERROR.md`.
    pub fn code(&self) -> &'static str {
        match self {
            EditError::InvalidFieldName(_) => "edit::invalid_field_name",
            EditError::UnknownField { .. } => "edit::unknown_field",
            EditError::InvalidKindName(_) => "edit::invalid_kind_name",
            EditError::ReservedKind => "edit::reserved_kind",
            EditError::RootOnlyEntry { .. } => "edit::root_only_entry",
            EditError::IndexOutOfRange { .. } => "edit::index_out_of_range",
            EditError::InvalidCardId(_) => "edit::invalid_card_id",
            EditError::DuplicateCardId(_) => "edit::duplicate_card_id",
            EditError::UnknownCardId(_) => "edit::unknown_card_id",
            EditError::TooManyCards { .. } => "edit::too_many_cards",
            EditError::ValueTooDeep { .. } => "edit::value_too_deep",
            EditError::Import(_) => "edit::import",
            EditError::FieldDecode { .. } => "edit::field_decode",
            EditError::FieldNotContent { .. } => "edit::field_not_content",
            EditError::FieldNotSingleLine { .. } => "edit::field_not_single_line",
            EditError::FieldCoercionFailed { .. } => "edit::field_coercion_failed",
            EditError::Change(_) => "edit::content_apply",
            EditError::InputTooLarge { .. } => "edit::input_too_large",
            EditError::TooManySteps { .. } => "edit::too_many_steps",
            EditError::TooManyMarks { .. } => "edit::too_many_marks",
            EditError::TooManyAnchors { .. } => "edit::too_many_anchors",
            EditError::InvalidPayload(_) => "edit::invalid_payload",
            EditError::DropsExtIsland { .. } => "edit::drops_ext_island",
            EditError::InvalidActor(_) => "edit::invalid_actor",
            EditError::BadLiveBytes { .. } => "edit::bad_live_bytes",
            EditError::Base(e) => e.code(),
        }
    }

    /// The facts this error's message interpolates. See
    /// [`Diagnostic::args`](crate::error::Diagnostic::args).
    ///
    /// `field` and `kind` ride here as well as in the anchor, because
    /// [`DocPath`] renders field segments unescaped and
    /// parses on `.` and `[`: a malformed name cannot be recovered from the
    /// rendered path. An `at` path is structural, so it rides the anchor alone.
    pub fn args(&self) -> BTreeMap<String, serde_json::Value> {
        match self {
            EditError::InvalidFieldName(field) => diag_args! { "field" => field },
            EditError::UnknownField { field, at: _ } => diag_args! { "field" => field },
            EditError::InvalidKindName(kind) => diag_args! { "kind" => kind },
            EditError::ReservedKind => diag_args! {},
            EditError::RootOnlyEntry { key } => diag_args! { "key" => key },
            EditError::IndexOutOfRange { index, len } => diag_args! {
                "index" => index,
                "len" => len,
            },
            EditError::InvalidCardId(id) | EditError::DuplicateCardId(id) | EditError::UnknownCardId(id) => {
                diag_args! { "id" => id }
            }
            EditError::TooManyCards { max } => diag_args! { "max" => max },
            EditError::ValueTooDeep { max } => diag_args! { "max" => max },
            EditError::Import(_) => diag_args! {},
            EditError::FieldDecode {
                field,
                at: _,
                codec,
                message: _,
            } => diag_args! {
                "field" => field,
                "codec" => codec,
            },
            EditError::FieldNotContent {
                field,
                at: _,
                declared,
            } => diag_args! {
                "field" => field,
                "declared" => declared,
            },
            EditError::FieldNotSingleLine { field, codec } => diag_args! {
                "field" => field,
                "codec" => codec,
            },
            EditError::FieldCoercionFailed {
                field,
                target,
                message: _,
            } => diag_args! {
                "field" => field,
                "target" => target,
            },
            EditError::Change(e) => apply_args(e),
            EditError::InputTooLarge { size, max } => diag_args! { "size" => size, "max" => max },
            EditError::TooManySteps { count, max }
            | EditError::TooManyMarks { count, max }
            | EditError::TooManyAnchors { count, max } => diag_args! { "count" => count, "max" => max },
            // One code, four sentences: the variant that spoke is not on the
            // wire, so a consumer's template falls back to `message`.
            EditError::InvalidPayload(_) => diag_args! {},
            EditError::DropsExtIsland { island, kind } => diag_args! {
                "island" => island,
                "type" => kind,
            },
            EditError::InvalidActor(actor) => diag_args! { "actor" => actor },
            EditError::BadLiveBytes { reason, .. } => diag_args! { "reason" => *reason },
            EditError::Base(e) => e.args(),
        }
    }

    /// This error as a [`Diagnostic`] under its [`code`](Self::code) and
    /// [`args`](Self::args), anchored at `path` when given.
    pub fn to_diagnostic(&self, path: Option<DocPath>) -> Diagnostic {
        let diagnostic = Diagnostic::new(crate::error::Severity::Error, self.to_string())
            .with_code(self.code().to_string())
            .with_args(self.args());
        match path {
            Some(path) => diagnostic.with_path(path.to_string()),
            None => diagnostic,
        }
    }

    /// The [`DocPath`] this error anchors to, relative to
    /// `base`: the card root the mutator ran against, empty for a card built
    /// before placement.
    ///
    /// - A field-named variant: its field under `base`, extended by the
    ///   variant's `at` path, so an element read anchors at `main.<field>[<i>]`.
    /// - [`IndexOutOfRange`](Self::IndexOutOfRange): the document-array slot
    ///   `cards[index]`, base-independent — a structural op names a slot.
    /// - The rest: `base` when it names a card, else no anchor.
    pub fn doc_path(&self, base: &crate::path::DocPath) -> Option<crate::path::DocPath> {
        use crate::path::DocPath;
        match self {
            EditError::FieldNotContent { field: f, at, .. }
            | EditError::FieldDecode { field: f, at, .. }
            | EditError::UnknownField { field: f, at } => {
                Some(at.iter().fold(base.field(f), |p, seg| p.segment(seg)))
            }
            EditError::InvalidFieldName(f)
            | EditError::FieldNotSingleLine { field: f, .. }
            | EditError::FieldCoercionFailed { field: f, .. } => Some(base.field(f)),
            EditError::IndexOutOfRange { index, .. } => Some(DocPath::card(None, *index)),
            EditError::Base(e) if e.field().is_some() => e.field().map(|f| base.field(f)),
            _ => (!base.segs().is_empty()).then(|| base.clone()),
        }
    }
}

/// A field-level invariant violation, shared by every payload ingestion path.
///
/// Each boundary maps it to its own error type (`ParseError`, `StorageError`,
/// [`EditError`]), so the invariant is enforced once, here. Its
/// [`Display`](std::fmt::Display) is the reason alone, for a boundary carrying the
/// offending key itself; [`message`](Self::message) names the key inline for the
/// boundaries whose error type does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldViolation {
    /// The field name does not match `[A-Za-z_][A-Za-z0-9_]*` (spec §3.4 / §10).
    InvalidName,
    /// The value nests deeper than [`MAX_JSON_DEPTH`](quillmark_content::MAX_JSON_DEPTH)
    /// (spec §8).
    TooDeep,
}

impl FieldViolation {
    /// The reason with `key` named inline, for `ParseError::InvalidStructure`
    /// and `StorageError::Malformed`, whose error types carry no key of their
    /// own.
    pub fn message(&self, key: &str) -> String {
        format!("invalid field {key:?}: {self}")
    }
}

impl std::fmt::Display for FieldViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldViolation::InvalidName => {
                f.write_str("field names must match [A-Za-z_][A-Za-z0-9_]*")
            }
            FieldViolation::TooDeep => write!(
                f,
                "nests deeper than the maximum of {} levels",
                quillmark_content::MAX_JSON_DEPTH
            ),
        }
    }
}

/// A payload-level invariant violation: [`FieldViolation`]'s seam one level up,
/// reading the item list rather than one item's contents. Shared by every
/// payload ingestion path, each mapping it to its own error type.
///
/// Every variant names something YAML cannot express, so a payload carrying one
/// emits markdown the parser rejects — except
/// [`MultiLineComment`](Self::MultiLineComment), which re-reads cleanly as a
/// different document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadViolation {
    /// Two user fields share a key; a YAML mapping admits each key once.
    DuplicateField { key: String },
    /// More user fields than [`MAX_FIELD_COUNT`](crate::error::MAX_FIELD_COUNT)
    /// (spec §8). Comments and `$` entries are not charged, matching the bound
    /// the parser applies after `$`-key extraction.
    TooManyFields { count: usize, max: usize },
    /// A `$` system entry appears more than once; emit writes its line for each.
    DuplicateMeta { key: &'static str },
    /// Comment text spans lines. A `#` opens one line, so every line after the
    /// first emits as bare YAML: the payload re-reads as fields no one wrote.
    MultiLineComment,
}

impl std::fmt::Display for PayloadViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PayloadViolation::DuplicateField { key } => {
                write!(f, "duplicate user-field key {key:?}")
            }
            PayloadViolation::TooManyFields { count, max } => write!(
                f,
                "card has {count} user fields, exceeding the maximum of {max}"
            ),
            PayloadViolation::DuplicateMeta { key } => {
                write!(f, "duplicate `{key}` entry")
            }
            PayloadViolation::MultiLineComment => {
                f.write_str("comment text spans multiple lines; a comment occupies one line")
            }
        }
    }
}

pub(crate) fn edit_error_from_violation(name: &str, v: FieldViolation) -> EditError {
    match v {
        FieldViolation::InvalidName => EditError::InvalidFieldName(name.to_string()),
        FieldViolation::TooDeep => EditError::ValueTooDeep {
            max: quillmark_content::MAX_JSON_DEPTH,
        },
    }
}

fn check_field_name(name: &str) -> Result<(), EditError> {
    if is_valid_field_name(name) {
        Ok(())
    } else {
        Err(EditError::InvalidFieldName(name.to_string()))
    }
}

fn check_field(name: &str, value: &serde_json::Value) -> Result<(), EditError> {
    validate_field(name, value).map_err(|v| edit_error_from_violation(name, v))
}

/// `value` with each content value it holds, at any depth, decoded as a body
/// write decodes one and held in its canonical form: a field's content
/// validates, stores, compares and hashes as a body's does.
/// [`EditError::FieldDecode`] at the first content value that does not
/// decode.
pub(crate) fn canonical_field(name: &str, mut value: serde_json::Value) -> Result<serde_json::Value, EditError> {
    let mut at = Vec::new();
    canonicalize_contents(&mut value, &mut at)
        .map_err(|(codec, e)| field_decode(name, &at, codec, ContentDecodeError::NotContent(e)))?;
    Ok(value)
}

fn canonicalize_contents(value: &mut serde_json::Value, at: &mut Vec<PathSegment>) -> Result<(), (Codec, String)> {
    if quillmark_content::units::is_content_value(value) {
        let content = quillmark_content::model::Content::from_value(value)
            .map_err(|e| (Codec::of_object(value), e.to_string()))?;
        *value = content.to_value();
        return Ok(());
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                at.push(PathSegment::Key(key.clone()));
                canonicalize_contents(v, at)?;
                at.pop();
            }
        }
        serde_json::Value::Array(items) => {
            for (index, v) in items.iter_mut().enumerate() {
                at.push(PathSegment::Index(index));
                canonicalize_contents(v, at)?;
                at.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

/// The composable-kind rule mapped onto the mutator error surface.
fn check_kind(kind: &str) -> Result<(), EditError> {
    validate_composable_kind(kind).map_err(|e| match e {
        CardKindError::InvalidName => EditError::InvalidKindName(kind.to_string()),
        CardKindError::Reserved => EditError::ReservedKind,
    })
}

/// A decode failure at an address, under the codec that ran. The one
/// [`EditError::FieldDecode`] constructor for a codec outcome, shared with the
/// schema-bound reads in [`reader`](crate::reader).
pub(crate) fn field_decode(
    name: &str,
    at: &[PathSegment],
    codec: Codec,
    e: ContentDecodeError,
) -> EditError {
    EditError::FieldDecode {
        field: name.to_string(),
        at: at.to_vec(),
        codec: codec.name().to_string(),
        message: e.into_message(),
    }
}

/// Depth-bound the values of an `$ext` / `$seed` map: the map itself is level
/// 1, so its values carry the rest of the budget. The iterator form of
/// [`crate::value::depth_check_meta_map`], for the merge that must clear the
/// bound before it owns a map.
fn check_meta_depth<'v>(
    values: impl IntoIterator<Item = &'v serde_json::Value>,
) -> Result<(), EditError> {
    let max = quillmark_content::MAX_JSON_DEPTH;
    if values
        .into_iter()
        .any(|v| crate::value::json_depth_exceeds(v, max - 1))
    {
        return Err(EditError::ValueTooDeep { max });
    }
    Ok(())
}

/// Validate a user field: name conformance and value-depth bound.
pub fn validate_field(key: &str, value: &serde_json::Value) -> Result<(), FieldViolation> {
    if !is_valid_field_name(key) {
        return Err(FieldViolation::InvalidName);
    }
    if crate::value::json_depth_exceeds(value, quillmark_content::MAX_JSON_DEPTH) {
        return Err(FieldViolation::TooDeep);
    }
    Ok(())
}

/// Validate a payload against every [`PayloadViolation`]. The per-item twin is
/// [`validate_field`].
pub fn validate_payload(payload: &Payload) -> Result<(), PayloadViolation> {
    let max = crate::error::MAX_FIELD_COUNT;
    let count = payload.len();
    if count > max {
        return Err(PayloadViolation::TooManyFields { count, max });
    }

    let mut fields: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut seen_quill = false;
    let mut seen_kind = false;
    let mut seen_id = false;
    let mut metas: std::collections::HashSet<MetaKey> = std::collections::HashSet::new();
    for item in payload.items() {
        let dup = |key| Err(PayloadViolation::DuplicateMeta { key });
        match item {
            PayloadItem::Field { key, .. } => {
                if !fields.insert(key.as_str()) {
                    return Err(PayloadViolation::DuplicateField { key: key.clone() });
                }
            }
            PayloadItem::Quill { .. } => {
                if std::mem::replace(&mut seen_quill, true) {
                    return dup("$quill");
                }
            }
            PayloadItem::Kind { .. } => {
                if std::mem::replace(&mut seen_kind, true) {
                    return dup("$kind");
                }
            }
            PayloadItem::Id { .. } => {
                if std::mem::replace(&mut seen_id, true) {
                    return dup("$id");
                }
            }
            PayloadItem::Meta { key, .. } => {
                if !metas.insert(*key) {
                    return dup(key.as_str());
                }
            }
            PayloadItem::Comment { text, .. } => {
                if text.contains(['\n', '\r']) {
                    return Err(PayloadViolation::MultiLineComment);
                }
            }
        }
    }
    if payload
        .nested_comments()
        .iter()
        .any(|nc| nc.text.contains(['\n', '\r']))
    {
        return Err(PayloadViolation::MultiLineComment);
    }
    Ok(())
}

/// Map a strict-write [`CoercionError`] to the field-write [`EditError`] surface.
///
/// Keys on the coercion `target`, not the field's own type, because the
/// constraint can be nested: an `array` of one-line `richtext` items fails with
/// `target == RICHTEXT_SINGLE_LINE_TARGET` while the field's type is `Array`.
fn conform_error_to_edit(name: &str, err: CoercionError) -> EditError {
    let CoercionError::Uncoercible {
        path: _,
        value: _,
        target,
        reason,
    } = err;
    let field = name.to_string();
    match target.as_str() {
        t if t == crate::quill::RICHTEXT_SINGLE_LINE_TARGET => EditError::FieldNotSingleLine {
            field,
            codec: CODEC_RICHTEXT.to_string(),
        },
        t if t == crate::quill::TEXT_SINGLE_LINE_TARGET => EditError::FieldNotSingleLine {
            field,
            codec: CODEC_TEXT.to_string(),
        },
        CODEC_RICHTEXT | CODEC_TEXT => EditError::FieldDecode {
            field,
            at: Vec::new(),
            codec: target,
            message: reason,
        },
        _ => EditError::FieldCoercionFailed {
            field,
            target,
            message: reason,
        },
    }
}

/// The `(name, error)` pairs for the batch names that would land past the §8
/// field count, `None` when the batch fits. The count belongs to the payload, so
/// a batch asks it once, before it applies any of itself: what comes back is the
/// overflowing tail, one diagnostic per offending field as every other batch
/// refusal reports.
pub(crate) fn overflow_errors<'n>(
    payload: &Payload,
    names: impl IntoIterator<Item = &'n str>,
) -> Option<Vec<(String, EditError)>> {
    let (count, past) = payload.overflow(names)?;
    Some(
        past.into_iter()
            .map(|name| {
                (
                    name.to_string(),
                    EditError::InvalidPayload(PayloadViolation::TooManyFields {
                        count,
                        max: crate::error::MAX_FIELD_COUNT,
                    }),
                )
            })
            .collect(),
    )
}

/// The canonical stored form of a typed field write, **without applying it**:
/// the dry-run that lets a batch collect every violation before mutating. Beside
/// it, one `parse::dropped_construct` per construct a markdown string in the
/// value dropped on its import, at its path under `base`, the card's root.
pub(crate) fn resolve_field_write(
    name: &str,
    value: &QuillValue,
    schema: &FieldSchema,
    base: &crate::path::DocPath,
) -> Result<(QuillValue, Vec<Diagnostic>), EditError> {
    check_field_name(name)?;
    let mut drops = Vec::new();
    let stored = QuillConfig::conform_value(
        value,
        schema,
        &base.field(name),
        Leniency::Write,
        &mut drops,
    )
    .map_err(|e| conform_error_to_edit(name, e))?;
    check_field(name, stored.as_json())?;
    Ok((stored, drops))
}

/// The literal text a `text` field's strict write commits for `body`: the
/// codec's boundary cleanup, so what a revise carries anchors onto is what
/// lands.
pub(crate) fn committed_text(name: &str, body: String, schema: &FieldSchema) -> Result<String, EditError> {
    let (committed, _) = resolve_field_write(name, &QuillValue::from(body), schema, &DocPath::new())?;
    Codec::Text
        .decode_field(committed.as_json())
        .map(|c| Codec::Text.project(&c))
        .map_err(|e| field_decode(name, &[], Codec::Text, e))
}

/// The mutable handle on a card already placed in a [`Document`]: every
/// `&mut self` verb [`Card`] carries, forwarded, and no way to replace the card
/// itself. Reads arrive through [`Deref`](std::ops::Deref).
///
/// Placement is positional — the main card carries `$quill` and `$seed`, a
/// composable card carries neither — and the gates that police it
/// ([`Unchecked::insert_card`],
/// [`Card::store_seed_overlay`]) run when a card is placed or edited, not when
/// one is assigned over another. Handing out a `&mut Card` would make
/// `*doc.card_mut(0).unwrap() = doc.main().clone()` a legal write past all of
/// them, and [`Document::quill_reference`] reads the root's `$quill` without
/// re-checking. A card built before placement is an ordinary `Card`, so the
/// verbs stay there and this only withholds the assignment.
pub struct CardMut<'a> {
    doc: &'a mut Document,
    at: Option<usize>,
}

impl<'a> CardMut<'a> {
    pub(crate) fn new(doc: &'a mut Document, at: Option<usize>) -> Self {
        Self { doc, at }
    }

    fn card(&mut self) -> &mut Card {
        self.doc.card_at_mut(self.at)
    }

    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        self.card().payload_mut()
    }

    /// [`Card::remove_field`].
    pub fn remove_field(&mut self, name: &str) -> Result<Option<QuillValue>, EditError> {
        self.card().remove_field(name)
    }

    /// [`Card::store_ext`].
    pub fn store_ext(&mut self, value: serde_json::Map<String, serde_json::Value>) -> Result<(), EditError> {
        self.card().store_ext(value)
    }

    /// [`Card::remove_ext`].
    pub fn remove_ext(&mut self) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.card().remove_ext()
    }

    /// [`Card::store_seed_overlay`].
    pub fn store_seed_overlay(&mut self, card_kind: impl Into<String>, value: serde_json::Value) -> Result<(), EditError> {
        self.card().store_seed_overlay(card_kind, value)
    }

    /// [`Card::remove_seed_overlay`].
    pub fn remove_seed_overlay(&mut self, card_kind: &str) -> Option<serde_json::Value> {
        self.card().remove_seed_overlay(card_kind)
    }

    /// [`Card::overwrite_body`].
    #[cfg(test)]
    pub(crate) fn overwrite_body(&mut self, content: impl Into<Lines>) {
        self.card().overwrite_body(content)
    }

    /// [`write`](Self::write) of [`Edit::Apply`](super::Edit::Apply).
    pub fn apply<'o>(&mut self, target: super::Target<'_>, change: &quillmark_content::change::Change, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.write(target, super::Edit::Apply(change.clone()), opts)
    }

    /// [`write`](Self::write) of [`Edit::Revise`](super::Edit::Revise).
    pub fn revise<'o>(&mut self, target: super::Target<'_>, markdown: &str, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.write(target, super::Edit::Revise(markdown), opts)
    }

    /// The value at `target` as the writer that read `base` read it,
    /// refused with `edit::stale_base` where no write left it.
    pub fn read_at(&mut self, target: super::Target<'_>, base: Option<&str>) -> Result<quillmark_content::model::Content, EditError> {
        self.doc.read_under(self.at, target, base)
    }

    /// [`write`](Self::write) of [`Edit::Replace`](super::Edit::Replace).
    pub fn replace<'o>(&mut self, target: super::Target<'_>, content: &quillmark_content::model::Content, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.write(target, super::Edit::Replace(content.clone()), opts)
    }

    /// [`Card::split_block`], landed in the document.
    pub fn split_block<'o>(&mut self, target: super::Target<'_>, at: usize, id: Option<&str>, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::SplitBlock { at, id: id.map(str::to_string) }, opts)
    }

    /// [`Card::join_blocks`], landed in the document.
    pub fn join_blocks<'o>(&mut self, target: super::Target<'_>, id: &str, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::JoinBlocks { id: id.to_string() }, opts)
    }

    /// [`Card::delete_range`], landed in the document.
    pub fn delete_range<'o>(&mut self, target: super::Target<'_>, start: usize, end: usize, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::DeleteRange { start, end }, opts)
    }

    /// [`Card::indent`], landed in the document.
    pub fn indent<'o>(&mut self, target: super::Target<'_>, first: &str, last: &str, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::Indent { first: first.to_string(), last: last.to_string() }, opts)
    }

    /// [`Card::outdent`], landed in the document.
    pub fn outdent<'o>(&mut self, target: super::Target<'_>, first: &str, last: &str, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::Outdent { first: first.to_string(), last: last.to_string() }, opts)
    }

    /// [`Card::wrap`], landed in the document.
    pub fn wrap<'o>(&mut self, target: super::Target<'_>, first: &str, last: &str, with: quillmark_content::structural::Wrap, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.structural(target, quillmark_content::structural::StructuralEdit::Wrap { first: first.to_string(), last: last.to_string(), with }, opts)
    }

    fn structural<'o>(&mut self, target: super::Target<'_>, edit: quillmark_content::structural::StructuralEdit, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.write(target, super::Edit::Structural(edit), opts)
    }

    /// Lands `edit` on the value at `target` in the document's Automerge
    /// document, built against the value its base names: a base the value
    /// has moved on from builds at the heads a write left it at, and the
    /// write merges with every change since as Automerge merges.
    pub fn write<'o>(&mut self, target: super::Target<'_>, edit: super::Edit<'_>, opts: impl Into<super::WriteOptions<'o>>) -> Result<super::Receipt, EditError> {
        self.doc.write_at(self.at, target, edit, opts.into(), None::<quillmark_content::units::Scalars>)
    }

    /// [`write`](Self::write) for a host counting offsets in `u`.
    pub fn write_in<'o>(&mut self, target: super::Target<'_>, edit: super::Edit<'_>, opts: impl Into<super::WriteOptions<'o>>, u: impl quillmark_content::units::Units) -> Result<super::Receipt, EditError> {
        self.doc.write_at(self.at, target, edit, opts.into(), Some(u))
    }

    /// [`Card::overwrite_field`].
    #[cfg(test)]
    pub(crate) fn overwrite_field(&mut self, name: &str, content: impl Into<Lines>) -> Result<(), EditError> {
        self.card().overwrite_field(name, content)
    }

    #[doc(hidden)]
    pub fn commit_field(&mut self, name: &str, value: impl Into<QuillValue>, schema: &FieldSchema) -> Result<(), EditError> {
        self.card().commit_field(name, value, schema)
    }

    /// A markdown revise of the value at `target` under `base`, its
    /// receipt counted in scalars on the value it landed on.
    pub(crate) fn revise_counted(&mut self, target: super::Target<'_>, markdown: &str, base: Option<Base<'_>>) -> Result<super::Receipt, EditError> {
        let before = self.read(target)?;
        Ok(self.write(target, super::Edit::Revise(markdown), super::WriteOptions { base, actor: None })?.counted_on(before))
    }

    #[doc(hidden)]
    pub fn revise_field_checked(&mut self, name: &str, body: impl Into<String>, schema: &FieldSchema) -> Result<super::Receipt, EditError> {
        self.card().revise_field_checked(name, body, schema)
    }

    #[doc(hidden)]
    pub fn field_hash_checked(&self, name: &str, schema: &FieldSchema) -> Result<String, EditError> {
        std::ops::Deref::deref(self).field_hash_checked(name, schema)
    }

    /// [`revise_field_checked`](Self::revise_field_checked) under `base`. A
    /// content field the base names an earlier value of is revised as its
    /// writer read it, and the change from that read lands at the heads it
    /// was read at, merged with every change since.
    #[doc(hidden)]
    pub fn revise_field_checked_against(&mut self, name: &str, base: Base<'_>, body: impl Into<String>, schema: &FieldSchema) -> Result<super::Receipt, EditError> {
        let read_hash = match base {
            Base::Hash(h) => h.to_string(),
            Base::Content(c) => super::base::content_hash(c),
        };
        let content = matches!(schema.r#type, FieldType::Text { .. } | FieldType::RichText { .. });
        if !content || read_hash == self.field_hash_checked(name, schema)? {
            return self.revise_field_checked(name, body, schema);
        }
        let target = super::Target::Field(name);
        let read = self.doc.read_under(self.at, target, Some(&read_hash)).map_err(|e| match e {
            EditError::Base(e) => EditError::Base(e.at_field(name)),
            other => other,
        })?;
        let mut scratch = Card::from_parts(Payload::new(), quillmark_content::model::Content::empty());
        scratch.payload_mut().insert(name.to_string(), QuillValue::from_json(read.to_value())).map_err(EditError::InvalidPayload)?;
        let revised = scratch.revise_field_checked(name, body, schema)?;
        let wanted = scratch.read(target)?;
        let change = quillmark_content::diff::diff(&read, &wanted).map_err(|e| EditError::change(e.into()))?;
        let before = self.read(target)?;
        let mut receipt = self.write(target, super::Edit::Apply(change), super::WriteOptions { base: Some(base), actor: None })?;
        receipt.warnings = revised.warnings;
        Ok(receipt.counted_on(before))
    }
}

impl Drop for CardMut<'_> {
    fn drop(&mut self) {
        self.doc.settle();
    }
}

impl std::ops::Deref for CardMut<'_> {
    type Target = Card;

    fn deref(&self) -> &Card {
        self.doc.card_at(self.at)
    }
}

impl Document {
    pub fn set_quill_ref(&mut self, reference: QuillReference) {
        self.main_card_mut().payload_mut().set_quill(reference);
    }

    pub fn card_mut(&mut self, index: usize) -> Option<CardMut<'_>> {
        (index < self.cards.len()).then(|| CardMut::new(self, Some(index)))
    }

    /// The writes no quill schema checks: [`Unchecked`].
    pub fn unchecked(&mut self) -> Unchecked<'_> {
        Unchecked(self)
    }

    /// Place `card` at `at` (`None` appends), its `$id` as
    /// [`Unchecked::insert_card`] takes it, answering that id.
    pub(crate) fn place_card(&mut self, at: Option<usize>, mut card: Card, actor: Option<&str>) -> Result<String, EditError> {
        let len = self.cards().len();
        let index = at.unwrap_or(len);
        if index > len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        Self::check_composable_placement(&card)?;
        self.check_card_room()?;
        if let Some(actor) = actor.filter(|a| !quillmark_content::ids::is_actor(a)) {
            return Err(EditError::InvalidActor(actor.to_string()));
        }
        let held: std::collections::HashSet<String> = self
            .cards()
            .iter()
            .filter_map(Card::id)
            .map(str::to_string)
            .collect();
        let id = match card.id() {
            Some(id) if !super::card_id::is_card_id(id) => {
                return Err(EditError::InvalidCardId(id.to_string()));
            }
            Some(id) if held.contains(id) => {
                return Err(EditError::DuplicateCardId(id.to_string()));
            }
            Some(id) => id.to_string(),
            None => {
                let id = match actor {
                    Some(actor) => self.mints.mint(held, actor),
                    None => super::card_id::Mint::new(held).mint(),
                };
                card.payload_mut().set_id(id.clone());
                id
            }
        };
        self.cards_vec_mut().insert(index, card);
        Ok(id)
    }

    fn check_card_room(&self) -> Result<(), EditError> {
        let max = crate::error::MAX_CARD_COUNT;
        if self.cards().len() >= max {
            return Err(EditError::TooManyCards { max });
        }
        Ok(())
    }

    /// The `$` entries a card may carry once placed as a composable card. A
    /// card with no `$kind` is rejected as an invalid (empty) name.
    ///
    /// Positional, so it lives here rather than in `TryFrom<CardWire>`: a
    /// [`CardWire`](crate::document::CardWire) is equally how the *main* card
    /// is read back and rewritten, and carries no signal of which it is.
    fn check_composable_placement(card: &Card) -> Result<(), EditError> {
        check_kind(card.kind().unwrap_or(""))?;
        if card.quill().is_some() {
            return Err(EditError::RootOnlyEntry {
                key: "$quill".to_string(),
            });
        }
        if card.seed().is_some() {
            return Err(EditError::RootOnlyEntry {
                key: MetaKey::Seed.as_str().to_string(),
            });
        }
        Ok(())
    }

    pub fn remove_card(&mut self, index: usize) -> Option<Card> {
        if index >= self.cards().len() {
            return None;
        }
        Some(self.cards_vec_mut().remove(index))
    }

    /// Replace the `$kind` of the composable card at `index`.
    ///
    /// Only the `$kind` metadata changes; the payload and body are untouched, so
    /// old-schema fields linger and schema migration is the caller's job.
    ///
    /// Returns [`EditError::IndexOutOfRange`], [`EditError::InvalidKindName`],
    /// or [`EditError::ReservedKind`] on constraint violations.
    pub fn set_card_kind(
        &mut self,
        index: usize,
        new_kind: impl Into<String>,
    ) -> Result<(), EditError> {
        let new_kind = new_kind.into();
        check_kind(&new_kind)?;
        let len = self.cards().len();
        let mut card = self
            .card_mut(index)
            .ok_or(EditError::IndexOutOfRange { index, len })?;
        card.payload_mut().set_kind(new_kind);
        Ok(())
    }

    /// Move card at `from` to position `to`. No-op when `from == to`.
    /// Either index out of range → [`EditError::IndexOutOfRange`].
    pub fn move_card(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        let len = self.cards().len();
        if from >= len {
            return Err(EditError::IndexOutOfRange { index: from, len });
        }
        if to >= len {
            return Err(EditError::IndexOutOfRange { index: to, len });
        }
        if from == to {
            return Ok(());
        }
        let card = self.cards_vec_mut().remove(from);
        self.cards_vec_mut().insert(to, card);
        Ok(())
    }
}

/// The writes on a [`Document`] that no quill schema checks, from
/// [`Document::unchecked`]: a field stored with no type coercion, and a card
/// placed whole. Each still holds every invariant the document does: names,
/// the §8 bounds, card ids, and each content value decoded as a body write
/// decodes one and stored in its canonical form, so `==`, the stored bytes
/// and the markdown export agree for a field as they do for a body. The
/// schema-checked writes are [`Quill::writer`](crate::quill::Quill::writer)'s.
pub struct Unchecked<'a>(&'a mut Document);

impl Unchecked<'_> {
    /// [`Card::store_field`] on the main card (`card: None`) or the composable
    /// card at `card`, [`EditError::IndexOutOfRange`] where none is.
    pub fn store_field(
        &mut self,
        card: Option<usize>,
        name: &str,
        value: impl Into<QuillValue>,
    ) -> Result<(), EditError> {
        let len = self.0.cards().len();
        let card = match card {
            None => self.0.main_card_mut(),
            Some(index) => self
                .0
                .cards_vec_mut()
                .get_mut(index)
                .ok_or(EditError::IndexOutOfRange { index, len })?,
        };
        card.store_field(name, value)
    }

    /// Place a composable card at `at` (`None` appends; past the last card
    /// [`EditError::IndexOutOfRange`]), answering its `$id`. Its `$kind` must
    /// be a valid, non-reserved composable kind ([`EditError::InvalidKindName`]
    /// / [`EditError::ReservedKind`]), and the document must hold fewer than
    /// [`MAX_CARD_COUNT`](crate::error::MAX_CARD_COUNT) cards
    /// ([`EditError::TooManyCards`]). A card spelling an id keeps it, refused
    /// where it is no card id ([`EditError::InvalidCardId`]) or another card
    /// holds it ([`EditError::DuplicateCardId`]). A card spelling none mints
    /// one: under `actor`, `{actor}-{token}`, so two writers placing cards
    /// from one document never mint one id, and positionally with none. An
    /// actor that cannot open a card id is [`EditError::InvalidActor`].
    pub fn insert_card(&mut self, at: Option<usize>, card: Card, actor: Option<&str>) -> Result<String, EditError> {
        self.0.place_card(at, card, actor)
    }
}

impl Card {
    /// Create a composable card with the given kind, no fields, and an empty body.
    pub fn new(kind: impl Into<String>) -> Result<Self, EditError> {
        let kind = kind.into();
        check_kind(&kind)?;
        let mut payload = Payload::new();
        payload.set_kind(kind);
        Ok(Card::from_parts(
            payload,
            quillmark_content::lines::model::Lines::empty(),
        ))
    }

    /// Store a payload field with no schema in play: schema coercion is
    /// deferred to render; contrast the typed
    /// [`TypedWriter::set`](crate::writer::TypedWriter::set). Scalars convert
    /// in place (`store_field("qty", 3)`) via the `From` impls on
    /// [`QuillValue`]. Each content value in `value`, at any depth, decodes as
    /// a body write's does and stores in its canonical form.
    ///
    /// Returns [`EditError::InvalidFieldName`] when `name` does not match
    /// `[A-Za-z_][A-Za-z0-9_]*`, [`EditError::FieldDecode`] for a content
    /// value that does not decode, and [`EditError::InvalidPayload`] when
    /// `name` is absent and the card already carries
    /// [`MAX_FIELD_COUNT`](crate::error::MAX_FIELD_COUNT) fields; overwriting a
    /// field a full card already carries is not a growth and lands.
    pub fn store_field(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
    ) -> Result<(), EditError> {
        let value = value.into();
        check_field(name, value.as_json())?;
        let value = QuillValue::from_json(canonical_field(name, value.into_json())?);
        self.payload_mut()
            .insert(name.to_string(), value)
            .map_err(EditError::InvalidPayload)?;
        Ok(())
    }

    /// Store several payload fields atomically. The whole batch is validated first: on
    /// any violation nothing is applied and every offending field is reported as
    /// a `(name, error)` pair. Per-field rules are those of
    /// [`Card::store_field`]; insertion order follows the iterator, and a
    /// repeated name behaves like repeated `store_field` calls (last value wins,
    /// first position kept).
    ///
    /// The §8 field count is charged over the whole batch rather than per call,
    /// so a batch that would take the card past it reports every name in the
    /// overflowing tail.
    pub fn store_fields<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut errors: Vec<(String, EditError)> = Vec::new();
        let fields: Vec<(String, QuillValue)> = fields
            .into_iter()
            .filter_map(|(k, v)| {
                let (name, value): (String, QuillValue) = (k.into(), v.into());
                let settled = check_field(&name, value.as_json())
                    .and_then(|()| canonical_field(&name, value.into_json()));
                match settled {
                    Ok(value) => Some((name, QuillValue::from_json(value))),
                    Err(e) => {
                        errors.push((name, e));
                        None
                    }
                }
            })
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        if let Some(errors) =
            overflow_errors(self.payload(), fields.iter().map(|(name, _)| name.as_str()))
        {
            return Err(errors);
        }
        for (name, value) in fields {
            self.payload_mut()
                .insert(name, value)
                .expect("the batch's overflow was refused above");
        }
        Ok(())
    }

    /// Remove a payload field; `Ok(None)` when the name is absent. Same name
    /// validation as [`Card::store_field`].
    pub fn remove_field(&mut self, name: &str) -> Result<Option<QuillValue>, EditError> {
        check_field_name(name)?;
        Ok(self.payload_mut().remove(name))
    }

    /// Replace the card's opaque `$ext` map wholesale, inserting it at the
    /// canonical position (after `$quill`/`$kind`, before user fields)
    /// when none existed. Passing an empty map records an explicit `$ext: {}`.
    ///
    /// The whole map is the write, so a consumer holding one namespace merges
    /// the rest of [`ext`](Card::ext) into what it stores; nested comments
    /// attached to the replaced map are dropped. [`EditError::ValueTooDeep`]
    /// when the map nests past the §8 depth limit.
    pub fn store_ext(
        &mut self,
        value: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), EditError> {
        let value =
            crate::value::depth_check_meta_map(value, |max| EditError::ValueTooDeep { max })?;
        self.payload_mut().set_ext(value);
        Ok(())
    }

    /// Remove the card's `$ext` map *entirely*, returning the previous map.
    /// Discards every namespace at once.
    pub fn remove_ext(&mut self) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.payload_mut().take_ext()
    }

    /// The map is written back only after the depth check passes, so the card is
    /// unchanged on error.
    fn merge_meta_namespace(
        &mut self,
        key: MetaKey,
        namespace: String,
        value: serde_json::Value,
    ) -> Result<(), EditError> {
        let surviving = self
            .payload()
            .meta(key)
            .into_iter()
            .flatten()
            .filter(|(k, _)| **k != namespace)
            .map(|(_, v)| v);
        check_meta_depth(surviving.chain(std::iter::once(&value)))?;
        let mut map = self.payload().meta(key).cloned().unwrap_or_default();
        map.insert(namespace, value);
        self.payload_mut().set_meta(key, map);
        Ok(())
    }

    /// Emptying the map drops the entry rather than leaving `$<key>: {}`.
    fn remove_meta_namespace(
        &mut self,
        key: MetaKey,
        namespace: &str,
    ) -> Option<serde_json::Value> {
        let mut map = self.payload().meta(key)?.clone();
        let removed = map.shift_remove(namespace)?;
        if map.is_empty() {
            self.payload_mut().take_meta(key);
        } else {
            self.payload_mut().set_meta(key, map);
        }
        Some(removed)
    }

    /// The raw `$seed` map (keyed by card-kind), or `None`. For a parsed,
    /// per-kind overlay, index this map by kind and pass the entry to
    /// [`crate::document::SeedOverlay::from_json`]. Only the main card carries
    /// `$seed`.
    pub fn seed(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload().seed()
    }

    /// Merge a card-kind's seed overlay `value` into the card's `$seed` map
    /// under `card_kind`, creating the map when absent and replacing any
    /// existing overlay for that kind; sibling kinds are preserved. Unlike the
    /// free-form namespaces of `$ext`, `card_kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] /
    /// [`EditError::ReservedKind`] otherwise). `$seed` binds the document root,
    /// so a card that is not the root is [`EditError::RootOnlyEntry`]. Returns
    /// [`EditError::ValueTooDeep`] when the merged map nests past the §8 depth
    /// limit; the card is unchanged on error.
    pub fn store_seed_overlay(
        &mut self,
        card_kind: impl Into<String>,
        value: serde_json::Value,
    ) -> Result<(), EditError> {
        let card_kind = card_kind.into();
        check_kind(&card_kind)?;
        self.check_carries_root_only(MetaKey::Seed)?;
        self.merge_meta_namespace(MetaKey::Seed, card_kind, value)
    }

    /// Refuse a root-only write on a card that is not the root. `$kind: main`
    /// is the root's own marker: the parser normalizes it in, the DTO
    /// synthesizes it, and placement refuses it as reserved, so a card answers
    /// this about itself.
    fn check_carries_root_only(&self, key: MetaKey) -> Result<(), EditError> {
        if self.kind() == Some("main") {
            return Ok(());
        }
        Err(EditError::RootOnlyEntry {
            key: key.as_str().to_string(),
        })
    }

    /// Remove `card_kind` from the card's `$seed` map, returning the overlay
    /// stored there. Emptying the map drops the `$seed` entry entirely.
    pub fn remove_seed_overlay(&mut self, card_kind: &str) -> Option<serde_json::Value> {
        self.remove_meta_namespace(MetaKey::Seed, card_kind)
    }

    /// Overwrite the body with a line-form value: no diff, no schema check,
    /// infallible, the stored body's anchors gone. The public whole-value
    /// write is [`replace`](Self::replace).
    #[cfg(test)]
    pub(crate) fn overwrite_body(&mut self, content: impl Into<Lines>) {
        let body = quillmark_content::diff::lift(self.body(), &content.into());
        self.set_body(body);
    }

    /// Overwrite a content field's value with a line-form value: the
    /// field-level twin of [`overwrite_body`](Self::overwrite_body). Stores the
    /// canonical content JSON (identity and content-only marks intact), no diff,
    /// no schema check. The previous value's anchors are gone. Returns
    /// [`EditError::InvalidFieldName`] for a malformed name, and
    /// [`EditError::InvalidPayload`] when an absent `name` would take the card
    /// past the §8 field count.
    ///
    /// A field stored under `codec: "plain"` keeps it and refuses a `content`
    /// that is not plain with [`EditError::FieldDecode`]; one still stored as
    /// an authored string lands under the codec its next bound load
    /// ([`Quill::conform`](crate::quill::Quill::conform)) gives it.
    #[cfg(test)]
    pub(crate) fn overwrite_field(
        &mut self,
        name: &str,
        content: impl Into<Lines>,
    ) -> Result<(), EditError> {
        check_field_name(name)?;
        self.store_field_content(name, &content.into())
    }

    /// Assumes `name` is already validated: every caller checks it or resolves an
    /// existing field first. The §8 field count is the payload's, so an absent
    /// `name` on a full card is [`EditError::InvalidPayload`].
    #[cfg(test)]
    fn store_field_content(&mut self, name: &str, content: &Lines) -> Result<(), EditError> {
        let stored = self.payload.get(name).map(QuillValue::as_json).filter(|v| v.is_object());
        if stored.map_or(Codec::Richtext, Codec::of_object) == Codec::Text && !content.is_plain() {
            return Err(field_decode(
                name,
                &[],
                Codec::Text,
                super::ContentDecodeError::NotContent(
                    quillmark_content::model::Invariant::NotPlain.to_string(),
                ),
            ));
        }
        let canonical = super::stored_over(self.payload.get(name).map(QuillValue::as_json), content);
        self.payload_mut()
            .insert(name.to_string(), QuillValue::from_json(canonical))
            .map(|_| ())
            .map_err(EditError::InvalidPayload)
    }

    /// Write-time commit: validate and normalize `value` per the field's schema
    /// `type` and store the canonical form. The typed sibling of the opaque
    /// [`store_field`](Self::store_field), which defers coercion to render.
    ///
    /// The stored form each `type` commits to is
    /// `prose/canon/SCHEMAS.md` § "Content fields are stored per codec". Stricter than
    /// the render floor: a cross-type value the floor would coerce (`1` → `true`)
    /// fails here. `null` passes through unchanged, under the null ≡ absent rule.
    ///
    /// The caller supplies `schema` because a [`Document`] holds only a `$quill`
    /// *reference*; [`crate::writer::TypedWriter`] resolves it per field and
    /// commits as this does, returning the `parse::dropped_construct` warnings
    /// this discards.
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldDecode`] / [`EditError::FieldNotSingleLine`]
    /// for a content field, [`EditError::FieldCoercionFailed`] for any other type
    /// mismatch, [`EditError::ValueTooDeep`] when the stored value nests
    /// past the §8 depth limit, and [`EditError::InvalidPayload`] when `name` is
    /// absent and the card is at the §8 field count.
    ///
    /// **Hidden**: the typed primitive, whose door is
    /// [`Quill::writer`](crate::quill::Quill::writer).
    #[doc(hidden)]
    pub fn commit_field(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
        schema: &FieldSchema,
    ) -> Result<(), EditError> {
        self.commit_field_at(name, &value.into(), schema, &DocPath::new())?;
        Ok(())
    }

    /// [`commit_field`](Self::commit_field), returning one
    /// `parse::dropped_construct` per construct a markdown string in `value`
    /// dropped, at its path under `base`, the card's root.
    pub(crate) fn commit_field_at(
        &mut self,
        name: &str,
        value: &QuillValue,
        schema: &FieldSchema,
        base: &DocPath,
    ) -> Result<Vec<Diagnostic>, EditError> {
        let (stored, drops) = resolve_field_write(name, value, schema, base)?;
        self.payload_mut()
            .insert(name.to_string(), stored)
            .map_err(EditError::InvalidPayload)?;
        Ok(drops)
    }

    /// [`revise`](Self::revise) of the body with no base: the one revise at
    /// [`Target::Body`](super::Target::Body), its receipt's `applied` counted
    /// in scalars on the body it read, which
    /// [`Receipt::in_units`](super::Receipt::in_units) converts.
    pub(crate) fn revise_body(&mut self, body: impl Into<String>) -> Result<super::Receipt, EditError> {
        self.revise_counted(super::Target::Body, &body.into())
    }

    /// The field's current content value: a markdown string imports, an
    /// absent field is empty, and an object reads under the codec it names.
    pub(crate) fn richtext_value(&self, name: &str) -> Result<quillmark_content::model::Content, EditError> {
        check_field_name(name)?;
        match self.payload.get(name) {
            None => Ok(quillmark_content::model::Content::empty()),
            Some(v) => Codec::Richtext.read_content(v.as_json()).map_err(|e| field_decode(name, &[], Codec::Richtext, e)),
        }
    }

    /// The revise of `before`, the value at `target`, by `markdown`, as
    /// [`revise`](Self::revise) builds it, landed nowhere.
    fn revision(
        &self,
        before: &quillmark_content::model::Content,
        markdown: &str,
    ) -> Result<quillmark_content::diff::Revision, EditError> {
        quillmark_automerge::revise(before, markdown).map_err(|e| match e {
            quillmark_content::diff::ReviseError::Import(e) => EditError::import(e),
            quillmark_content::diff::ReviseError::Refused(e) => EditError::change(e),
        })
    }

    /// Revise a content field from authored text **with schema enforcement**:
    /// [`revise`](Self::revise) of the field, read under the
    /// codec `schema` declares, then the conform
    /// [`commit_field`](Self::commit_field) runs. Returns the
    /// [`Receipt`](super::Receipt), and leaves the field unchanged on any error. A
    /// `text` field imports no markdown, so its receipt holds no warning.
    ///
    /// **Hidden** on the same terms as [`commit_field`](Self::commit_field): its
    /// door is
    /// [`TypedWriter::revise`](crate::writer::TypedWriter::revise).
    #[doc(hidden)]
    pub fn revise_field_checked(
        &mut self,
        name: &str,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<super::Receipt, EditError> {
        check_field_name(name)?;
        let target = super::Target::Field(name);
        if matches!(schema.r#type, FieldType::Text { .. }) {
            let text = committed_text(name, body.into(), schema)?;
            let mut before = match self.payload.get(name) {
                None => quillmark_content::model::Content::empty(),
                Some(v) => Codec::Text.read_content(v.as_json()).map_err(|e| field_decode(name, &[], Codec::Text, e))?,
            };
            before.codec = Codec::Text.content();
            let revised = self.revision(&before, &text)?;
            let stored = QuillValue::from_json(revised.content.to_value());
            return self.revising(target, Vec::new(), |card| {
                card.payload_mut().insert(name.to_string(), stored).map(|_| ()).map_err(EditError::InvalidPayload)
            });
        }
        let before = self.richtext_value(name)?;
        let revised = self.revision(&before, &body.into())?;
        super::write::keeps_ext_islands(&before, &revised.content)?;
        let (stored, _) = resolve_field_write(
            name,
            &QuillValue::from_json(revised.content.to_value()),
            schema,
            &DocPath::new(),
        )?;
        self.revising(target, revised.warnings, |card| {
            card.payload_mut()
                .insert(name.to_string(), stored)
                .map(|_| ())
                .map_err(EditError::InvalidPayload)
        })
    }
}

fn apply_args(e: &quillmark_content::change::ApplyError) -> BTreeMap<String, serde_json::Value> {
    use quillmark_content::change::ApplyError as A;
    let reason = match e {
        A::Stale { .. } => "stale",
        A::UnknownBlock { .. } => "unknown_block",
        A::UnknownContainer { .. } => "unknown_container",
        A::ContainerKind { .. } => "container_kind",
        A::ContainerInUse { .. } => "container_in_use",
        A::UnknownAnchor { .. } => "unknown_anchor",
        A::AnchorHeld { .. } => "anchor_held",
        A::UnknownIsland { .. } => "unknown_island",
        A::UnknownSlot { .. } => "unknown_slot",
        A::BadStep { .. } => "bad_step",
        A::Invalid(_) => "invalid",
        _ => return diag_args! {},
    };
    let mut args = diag_args! { "reason" => reason };
    if let A::Stale { step } | A::BadStep { step, .. } = e {
        args.insert("step".into(), (*step).into());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::DocPath;

    #[test]
    fn field_error_anchors_under_the_card_base() {
        let main = DocPath::main();
        assert_eq!(
            EditError::FieldCoercionFailed {
                field: "font_size".into(),
                target: "integer".into(),
                message: "x".into(),
            }
            .doc_path(&main)
            .unwrap()
            .to_string(),
            "main.font_size"
        );
        let card = DocPath::card(Some("indorsement"), 1);
        assert_eq!(
            EditError::unknown_field("signature_block")
                .doc_path(&card)
                .unwrap()
                .to_string(),
            "cards.indorsement[1].signature_block"
        );
    }

    #[test]
    fn index_out_of_range_anchors_at_the_array_slot() {
        for base in [DocPath::main(), DocPath::card(Some("note"), 0)] {
            assert_eq!(
                EditError::IndexOutOfRange { index: 4, len: 2 }
                    .doc_path(&base)
                    .unwrap()
                    .to_string(),
                "cards[4]"
            );
        }
    }

    #[test]
    fn kind_and_depth_errors_anchor_at_base_or_nowhere() {
        assert_eq!(
            EditError::ReservedKind
                .doc_path(&DocPath::card(None, 2))
                .unwrap()
                .to_string(),
            "cards[2]"
        );
        assert_eq!(
            EditError::ValueTooDeep { max: 8 }.doc_path(&DocPath::new()),
            None
        );
        assert_eq!(
            EditError::ValueTooDeep { max: 8 }
                .doc_path(&DocPath::main())
                .unwrap()
                .to_string(),
            "main"
        );
    }
}
