//! Typed mutators for [`Document`] and [`Card`].
//!
//! Every successful mutator leaves user field names matching
//! `[A-Za-z_][A-Za-z0-9_]*`, composable `$kind`s valid, and values inside the
//! §8 depth bound, so the result is safely serializable to the backend plate.
//! `$ext`/`$seed` are opaque mappings that carry no field-name invariant, but
//! do carry the depth bound.

use std::collections::BTreeMap;

use quillmark_content::delta::diff_import;
use quillmark_content::import::ImportError;
use quillmark_content::{ApplyError, ChangeBundle, Delta, Normalized};

use crate::document::meta::{validate_composable_kind, CardKindError};
use crate::error::diag_args;
use crate::document::payload::MetaKey;
use crate::document::{Card, Codec, ContentDecodeError, Document, Payload, PayloadItem};
use crate::quill::{CoercionError, FieldSchema, FieldType, Leniency, QuillConfig};
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;

/// A field plus its in-field path, rendered through
/// [`DocPath`](crate::path::DocPath) (`recipients[0].name`), so a message names
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
/// [`EditError::FieldNotInline`]. Frozen with the diagnostic arg: it is the
/// schema keyword, not a display string.
pub const CODEC_RICHTEXT: &str = "richtext";

/// The `plaintext` codec. See [`CODEC_RICHTEXT`].
pub const CODEC_PLAINTEXT: &str = "plaintext";

/// Errors returned by document and card mutators.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("invalid field name '{0}': must match [A-Za-z_][A-Za-z0-9_]*")]
    InvalidFieldName(String),

    /// A typed write ([`TypedWriter::set`](crate::TypedWriter::set) /
    /// [`CardWriter::set`](crate::CardWriter::set)) or a schema-bound read
    /// addressed a well-formed name the bound schema does not declare (or a card
    /// whose `$kind` carries no schema). A property an `object` field does not
    /// declare is the same error one level down, `at` naming it. Use
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

    #[error("value nests deeper than the maximum of {max} levels")]
    ValueTooDeep { max: usize },

    /// The offending marker may be on a node nested inside `field` rather than on
    /// `field` itself.
    #[error("`!must_fill` on field '{field}' targets a mapping; `!must_fill` is supported on scalars and sequences only")]
    FillOnMapping { field: String },

    /// Markdown import failed: the content codec rejected the input for a body
    /// *or* a field path (e.g. container nesting past
    /// [`MAX_NESTING_DEPTH`](quillmark_content::MAX_NESTING_DEPTH)). Returned
    /// instead of silently degrading the target to empty on a rejected import.
    #[error("markdown import failed: {0}")]
    Import(ImportError),

    /// A value could not become the field's content through `codec`: a JSON
    /// object that is not a canonical content, a markdown string that failed to
    /// import, a shape that is neither object nor string, or formatting under
    /// the plain-only `plaintext` codec. `message` names which.
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
        /// [`CODEC_RICHTEXT`] or [`CODEC_PLAINTEXT`].
        codec: String,
        message: String,
    },

    /// A `Content` read ([`TypedReader::get_content`](crate::TypedReader::get_content))
    /// addressed a field whose declared type is not a content *leaf*. The schema
    /// answers before the payload is consulted, and the test is narrower than
    /// the subtree walk: an `array<richtext>` carries content yet has no single
    /// `Content`, so it lands here. Address one of its elements with
    /// [`get_content_at`](crate::TypedReader::get_content_at), which raises this
    /// in turn for a path that resolves to no content leaf.
    #[error("field '{}' is declared '{declared}', which is not a content field", render_at(.field, .at))]
    FieldNotContent {
        field: String,
        /// The steps from the field to the addressed value, empty when the field
        /// itself is addressed. `declared` names the type reached, so a
        /// `string[]` element reports `string` rather than the field's `array`.
        at: Vec<PathSegment>,
        declared: String,
    },

    /// A content field written under an `inline: true` schema decoded to a
    /// multi-line content. Both prose codecs declare `inline`, so one code
    /// covers both and `codec` says which lane raised it.
    #[error("{codec} field '{field}' is not inline: {codec}(inline) requires a single line with no container or island")]
    FieldNotInline {
        field: String,
        /// The codec whose inline constraint was violated: [`CODEC_RICHTEXT`]
        /// or [`CODEC_PLAINTEXT`].
        codec: String,
    },

    /// A typed write could not coerce the value to the field's schema type: the
    /// general failure for scalar/array/object types (a `"x"` for an `integer`,
    /// a non-object for an `object`, …). Content fields report through
    /// [`FieldDecode`](Self::FieldDecode) / [`FieldNotInline`](Self::FieldNotInline)
    /// instead.
    #[error("field '{field}' could not be coerced to its schema type: {message}")]
    FieldCoercionFailed {
        field: String,
        /// The schema type the write was coerced against; `message` alone is
        /// English.
        target: String,
        message: String,
    },

    /// A content field-change bundle (text delta, line ops, mark ops) applied
    /// out of bounds or broke an invariant normalization could not repair.
    #[error("content apply failed: {0:?}")]
    ContentApply(ApplyError),

    /// The card's item list violates an invariant of the list as a whole. Only a
    /// door taking a whole payload at once reaches it ([`Card::try_from`] a
    /// [`CardWire`](crate::CardWire)); the per-field mutators cannot build one.
    #[error("{0}")]
    InvalidPayload(PayloadViolation),
}

impl EditError {
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
            EditError::ValueTooDeep { .. } => "edit::value_too_deep",
            EditError::FillOnMapping { .. } => "edit::fill_on_mapping",
            EditError::Import(_) => "edit::import",
            EditError::FieldDecode { .. } => "edit::field_decode",
            EditError::FieldNotContent { .. } => "edit::field_not_content",
            EditError::FieldNotInline { .. } => "edit::field_not_inline",
            EditError::FieldCoercionFailed { .. } => "edit::field_coercion_failed",
            EditError::ContentApply(_) => "edit::content_apply",
            EditError::InvalidPayload(_) => "edit::invalid_payload",
        }
    }

    /// The facts this error's message interpolates. See
    /// [`Diagnostic::args`](crate::error::Diagnostic::args).
    ///
    /// `field` and `kind` ride here as well as in the anchor, because
    /// [`DocPath`](crate::path::DocPath) renders field segments unescaped and
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
            EditError::ValueTooDeep { max } => diag_args! { "max" => max },
            EditError::FillOnMapping { field } => diag_args! { "field" => field },
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
            EditError::FieldNotInline { field, codec } => diag_args! {
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
            EditError::ContentApply(_) => diag_args! {},
            // One code, four sentences: the variant that spoke is not on the
            // wire, so a consumer's template falls back to `message`.
            EditError::InvalidPayload(_) => diag_args! {},
        }
    }

    /// The [`DocPath`](crate::path::DocPath) this error anchors to, relative to
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
            | EditError::FieldNotInline { field: f, .. }
            | EditError::FillOnMapping { field: f }
            | EditError::FieldCoercionFailed { field: f, .. } => Some(base.field(f)),
            EditError::IndexOutOfRange { index, .. } => Some(DocPath::card(None, *index)),
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
    /// A `!must_fill` marker targets a mapping. The marker rides a value's tag,
    /// and a block mapping opens on the next line, with no tag position of its
    /// own (spec §3.4).
    FillOnMapping,
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
            FieldViolation::FillOnMapping => f.write_str(
                "`!must_fill` targets a mapping; `!must_fill` is supported on scalars and sequences only",
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
            PayloadViolation::MultiLineComment => f.write_str(
                "comment text spans multiple lines; a comment occupies one line",
            ),
        }
    }
}

pub(crate) fn edit_error_from_violation(name: &str, v: FieldViolation) -> EditError {
    match v {
        FieldViolation::InvalidName => EditError::InvalidFieldName(name.to_string()),
        FieldViolation::TooDeep => EditError::ValueTooDeep {
            max: quillmark_content::MAX_JSON_DEPTH,
        },
        FieldViolation::FillOnMapping => EditError::FillOnMapping {
            field: name.to_string(),
        },
    }
}

fn check_field(name: &str, value: &serde_json::Value) -> Result<(), EditError> {
    validate_field(name, value).map_err(|v| edit_error_from_violation(name, v))
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
    Ok(())
}

/// Refuse a `!must_fill` marker targeting a mapping
/// ([`FieldViolation::FillOnMapping`]), the rule the parser enforces on source.
///
/// The root under `fill` may still be a canonical content object: emit projects
/// that to its markdown scalar before writing the marker
/// (`emit::project_content_field`). A nested node is emitted structurally, with
/// no projection, so a marker there targets a scalar or a sequence.
pub fn validate_fill_targets(
    value: &crate::QuillValue,
    fill: bool,
) -> Result<(), FieldViolation> {
    if fill
        && value.as_json().is_object()
        && super::emit::project_content_field(value.as_json()).is_none()
    {
        return Err(FieldViolation::FillOnMapping);
    }
    for path in value.nonroot_fill_paths() {
        if value.is_object_at(&path) {
            return Err(FieldViolation::FillOnMapping);
        }
    }
    Ok(())
}

/// Map a strict-write [`CoercionError`] to the field-write [`EditError`] surface.
///
/// Keys on the coercion `target`, not the field's own type, because the
/// constraint can be nested: an `array` of `richtext(inline)` items fails with
/// `target == "richtext(inline)"` while the field's type is `Array`.
fn conform_error_to_edit(name: &str, err: CoercionError) -> EditError {
    let CoercionError::Uncoercible {
        path: _,
        value: _,
        target,
        reason,
    } = err;
    let field = name.to_string();
    match target.as_str() {
        "richtext(inline)" => EditError::FieldNotInline {
            field,
            codec: CODEC_RICHTEXT.to_string(),
        },
        "plaintext(inline)" => EditError::FieldNotInline {
            field,
            codec: CODEC_PLAINTEXT.to_string(),
        },
        CODEC_RICHTEXT | CODEC_PLAINTEXT => EditError::FieldDecode {
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

/// The canonical stored form of a typed field write, **without applying it**:
/// the dry-run that lets a batch collect every violation before mutating.
pub(crate) fn resolve_field_write(
    name: &str,
    value: QuillValue,
    schema: &FieldSchema,
) -> Result<QuillValue, EditError> {
    if !is_valid_field_name(name) {
        return Err(EditError::InvalidFieldName(name.to_string()));
    }
    let stored = QuillConfig::conform_value(&value, schema, name, Leniency::Write)
        .map_err(|e| conform_error_to_edit(name, e))?;
    check_field(name, stored.as_json())?;
    Ok(stored)
}

impl Document {
    pub fn set_quill_ref(&mut self, reference: QuillReference) {
        self.main_mut().payload_mut().set_quill(reference);
    }

    pub fn card_mut(&mut self, index: usize) -> Option<&mut Card> {
        self.cards_mut().get_mut(index)
    }

    /// Append a composable card. Its `$kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] /
    /// [`EditError::ReservedKind`] otherwise).
    pub fn push_card(&mut self, card: Card) -> Result<(), EditError> {
        Self::check_composable_placement(&card)?;
        self.cards_vec_mut().push(card);
        Ok(())
    }

    /// Insert a composable card at `index` (`index > len` →
    /// [`EditError::IndexOutOfRange`]; invalid `$kind` →
    /// [`EditError::InvalidKindName`] / [`EditError::ReservedKind`]).
    pub fn insert_card(&mut self, index: usize, card: Card) -> Result<(), EditError> {
        let len = self.cards().len();
        if index > len {
            return Err(EditError::IndexOutOfRange { index, len });
        }
        Self::check_composable_placement(&card)?;
        self.cards_vec_mut().insert(index, card);
        Ok(())
    }

    /// The `$` entries a card may carry once placed as a composable card. A
    /// card with no `$kind` is rejected as an invalid (empty) name.
    ///
    /// Positional, so it lives here rather than in `TryFrom<CardWire>`: a
    /// [`CardWire`](crate::CardWire) is equally how the *main* card is read back
    /// and rewritten, and carries no signal of which it is.
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
        let card = self
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

impl Card {
    /// Create a composable card with the given kind, no fields, and an empty body.
    pub fn new(kind: impl Into<String>) -> Result<Self, EditError> {
        let kind = kind.into();
        check_kind(&kind)?;
        let mut payload = Payload::new();
        payload.set_kind(kind);
        Ok(Card::from_parts(
            payload,
            quillmark_content::Normalized::empty(),
        ))
    }

    /// Store a payload field verbatim, clearing any `!must_fill` marker on that
    /// key. Coercion is deferred to render; contrast the typed
    /// [`TypedWriter::set`](crate::TypedWriter::set). Scalars convert in place
    /// (`store_field("qty", 3)`) via the `From` impls on [`QuillValue`].
    ///
    /// Returns [`EditError::InvalidFieldName`] when `name` does not match
    /// `[A-Za-z_][A-Za-z0-9_]*`.
    pub fn store_field(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let value = value.into();
        check_field(name, value.as_json())?;
        validate_fill_targets(&value, false).map_err(|v| edit_error_from_violation(name, v))?;
        self.payload_mut().insert_unchecked(name.to_string(), value);
        Ok(())
    }

    /// Store a payload field verbatim and mark it as a `!must_fill` placeholder.
    /// `Null` emits as `key: !must_fill`; other values as
    /// `key: !must_fill <value>`. Validation as [`Card::store_field`].
    pub fn store_fill(&mut self, name: &str, value: impl Into<QuillValue>) -> Result<(), EditError> {
        let value = value.into();
        check_field(name, value.as_json())?;
        validate_fill_targets(&value, true).map_err(|v| edit_error_from_violation(name, v))?;
        self.payload_mut()
            .insert_fill_unchecked(name.to_string(), value);
        Ok(())
    }

    /// Store several payload fields verbatim and atomically, clearing any
    /// `!must_fill` marker on each key. The whole batch is validated first: on
    /// any violation nothing is applied and every offending field is reported as
    /// a `(name, error)` pair. Per-field rules are those of
    /// [`Card::store_field`]; insertion order follows the iterator, and a
    /// repeated name behaves like repeated `store_field` calls (last value wins,
    /// first position kept).
    pub fn store_fields<K, V, I>(&mut self, fields: I) -> Result<(), Vec<(String, EditError)>>
    where
        K: Into<String>,
        V: Into<QuillValue>,
        I: IntoIterator<Item = (K, V)>,
    {
        let fields: Vec<(String, QuillValue)> = fields
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let errors: Vec<(String, EditError)> = fields
            .iter()
            .filter_map(|(name, value)| {
                check_field(name, value.as_json())
                    .and_then(|()| {
                        validate_fill_targets(value, false)
                            .map_err(|v| edit_error_from_violation(name, v))
                    })
                    .err()
                    .map(|e| (name.clone(), e))
            })
            .collect();
        if !errors.is_empty() {
            return Err(errors);
        }
        // Validated above; the unchecked insert avoids re-checking per field.
        for (name, value) in fields {
            self.payload_mut().insert_unchecked(name, value);
        }
        Ok(())
    }

    /// Remove a payload field; `Ok(None)` when the name is absent. Same name
    /// validation as [`Card::store_field`].
    pub fn remove_field(&mut self, name: &str) -> Result<Option<QuillValue>, EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
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
        let mut map = self.payload_mut().take_meta(key).unwrap_or_default();
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
        let mut map = self.payload_mut().take_meta(key)?;
        let removed = map.remove(namespace);
        if !map.is_empty() {
            self.payload_mut().set_meta(key, map);
        }
        removed
    }

    /// The raw `$seed` map (keyed by card-kind), or `None`. For a parsed,
    /// per-kind overlay, index this map by kind and pass the entry to
    /// [`crate::SeedOverlay::from_json`]. Only the main card carries `$seed`.
    pub fn seed(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload().seed()
    }

    /// Merge a card-kind's seed overlay `value` into the card's `$seed` map
    /// under `card_kind`, creating the map when absent and replacing any
    /// existing overlay for that kind; sibling kinds are preserved. Unlike the
    /// free-form namespaces of `$ext`, `card_kind` must be a valid, non-reserved
    /// composable kind ([`EditError::InvalidKindName`] /
    /// [`EditError::ReservedKind`] otherwise). Returns
    /// [`EditError::ValueTooDeep`] when the merged map nests past the §8 depth
    /// limit; the card is unchanged on error.
    pub fn store_seed_overlay(
        &mut self,
        card_kind: impl Into<String>,
        value: serde_json::Value,
    ) -> Result<(), EditError> {
        let card_kind = card_kind.into();
        check_kind(&card_kind)?;
        self.merge_meta_namespace(MetaKey::Seed, card_kind, value)
    }

    /// Remove `card_kind` from the card's `$seed` map, returning the overlay
    /// stored there. Emptying the map drops the `$seed` entry entirely.
    pub fn remove_seed_overlay(&mut self, card_kind: &str) -> Option<serde_json::Value> {
        self.remove_meta_namespace(MetaKey::Seed, card_kind)
    }

    /// Overwrite the body with a pre-built [`Content`](quillmark_content::Content): value semantics, no
    /// markdown import, no diff, no schema check, infallible. A raw `Content`
    /// normalizes on the way in, so what lands is canonical. Anchor fate across
    /// the content lane: **overwrite destroys, [`revise_body`](Self::revise_body)
    /// rebases, [`apply_body_change`](Self::apply_body_change) preserves.**
    pub fn overwrite_body(&mut self, content: impl Into<Normalized>) {
        self.body = content.into();
    }

    /// Overwrite a content field's value with a pre-built [`Content`](quillmark_content::Content): the
    /// field-level twin of [`overwrite_body`](Self::overwrite_body). Stores the
    /// canonical content JSON (identity and content-only marks intact), no diff,
    /// no schema check. The previous value's anchors are gone. Returns
    /// [`EditError::InvalidFieldName`] for a malformed name.
    ///
    /// Richtext codec: a `plaintext` field rests as its literal string, so an
    /// object landed here departs that field's resting form until the next bound
    /// load ([`Quill::conform`](crate::Quill::conform)) converges it.
    pub fn overwrite_field(
        &mut self,
        name: &str,
        content: impl Into<Normalized>,
    ) -> Result<(), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        self.store_field_content(name, &content.into());
        Ok(())
    }

    /// Assumes `name` is already validated: every caller checks it or resolves an
    /// existing field first.
    fn store_field_content(&mut self, name: &str, content: &Normalized) {
        let canonical = quillmark_content::serial::to_canonical_value(content);
        self.payload_mut()
            .insert_unchecked(name.to_string(), QuillValue::from_json(canonical));
    }

    /// Write-time commit: validate and normalize `value` per the field's schema
    /// `type` and store the canonical form. The typed sibling of the opaque
    /// [`store_field`](Self::store_field), which defers coercion to render.
    ///
    /// The resting form each `type` commits to is
    /// `prose/canon/SCHEMAS.md` § "Content fields rest per codec". Stricter than
    /// the render floor: a cross-type value the floor would coerce (`1` → `true`)
    /// fails here. `null` passes through unchanged, under the null ≡ absent rule.
    ///
    /// The caller supplies `schema` because a [`Document`] holds only a `$quill`
    /// *reference*; [`crate::TypedWriter`] resolves it per field and calls this.
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldDecode`] / [`EditError::FieldNotInline`]
    /// for a content field, [`EditError::FieldCoercionFailed`] for any other type
    /// mismatch, and [`EditError::ValueTooDeep`] when the stored value nests
    /// past the §8 depth limit.
    ///
    /// **Hidden**: the typed primitive, whose door is
    /// [`Quill::writer`](crate::Quill::writer).
    #[doc(hidden)]
    pub fn commit_field(
        &mut self,
        name: &str,
        value: impl Into<QuillValue>,
        schema: &FieldSchema,
    ) -> Result<(), EditError> {
        let stored = resolve_field_write(name, value.into(), schema)?;
        self.payload_mut().insert_unchecked(name.to_string(), stored);
        Ok(())
    }

    /// Revise the body from an authored markdown string: edit semantics. Imports
    /// the markdown, diffs it against the current body so surviving identity
    /// anchors rebase (formatting marks are re-derived), and returns the text
    /// [`Delta`] an editor bridge maps its own positions through
    /// ([`Delta::map_pos`]). An over-nested input returns [`EditError::Import`]
    /// rather than degrading to the empty content.
    pub fn revise_body(&mut self, body: impl Into<String>) -> Result<Delta, EditError> {
        let (content, delta) =
            diff_import(self.body(), &body.into()).map_err(EditError::Import)?;
        self.overwrite_body(content);
        Ok(delta)
    }

    /// Decode the field's current content (an absent field imports from empty),
    /// diff `body` against it so surviving anchors rebase, and return the new
    /// content with its text [`Delta`]. Stores nothing: the caller lands it.
    fn diff_field(
        &self,
        name: &str,
        body: impl Into<String>,
    ) -> Result<(Normalized, Delta), EditError> {
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        let base = match self.field_content(name, Codec::Richtext) {
            Some(Ok(rt)) => rt,
            Some(Err(e)) => return Err(field_decode(name, &[], Codec::Richtext, e)),
            None => Normalized::empty(),
        };
        diff_import(&base, &body.into()).map_err(EditError::Import)
    }

    /// Revise a richtext field from an authored markdown string: the field-level
    /// twin of [`revise_body`](Self::revise_body). Decodes the field's current
    /// content as the diff base (an **absent** field cold-imports from empty),
    /// rebases surviving anchors, re-stores the canonical content, and returns
    /// the text [`Delta`].
    ///
    /// Schema-blind: a `richtext(inline)` violation surfaces at validate/render,
    /// not here.
    ///
    /// **Richtext only**, and the exclusion bites: decoding a `plaintext` field's
    /// value as markdown eats its escapes (`a \*b\*` commits back as `a *b*`)
    /// and leaves a content object where that field rests as a string. The typed
    /// [`TypedWriter::revise_field`](crate::TypedWriter::revise_field) resolves
    /// the codec from the schema and is the plaintext-safe door.
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldDecode`] when the field is present but is not a
    /// richtext content (a scalar a `store_field` wrote), and
    /// [`EditError::Import`] on an over-nested markdown input.
    pub fn revise_field(&mut self, name: &str, body: impl Into<String>) -> Result<Delta, EditError> {
        let (content, delta) = self.diff_field(name, body)?;
        self.store_field_content(name, &content);
        Ok(delta)
    }

    /// Revise a content field from authored text **with schema enforcement**:
    /// [`revise_field`](Self::revise_field)'s anchor-rebasing diff, then the
    /// conform [`commit_field`](Self::commit_field) runs. Returns the text
    /// [`Delta`], and leaves the field unchanged on any error.
    ///
    /// **Hidden** on the same terms as [`commit_field`](Self::commit_field): its
    /// door is [`TypedWriter::revise_field`](crate::TypedWriter::revise_field).
    #[doc(hidden)]
    pub fn revise_field_checked(
        &mut self,
        name: &str,
        body: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Delta, EditError> {
        // Decoding a `plaintext` value as markdown eats its escapes, so a
        // byte-identical revise of `a \*b\*` would commit `a *b*`. It has no
        // anchors to rebase either, so it takes the literal codec.
        if matches!(schema.r#type, FieldType::PlainText { .. }) {
            return self.revise_field_plaintext(name, body, schema);
        }
        let (content, delta) = self.diff_field(name, body)?;
        // Re-canonicalizing a content object keeps its identity marks, so the
        // schema check fires on the value the anchors survived onto.
        let canonical = quillmark_content::serial::to_canonical_value(&content);
        let stored = resolve_field_write(name, QuillValue::from_json(canonical), schema)?;
        self.payload_mut().insert_unchecked(name.to_string(), stored);
        Ok(delta)
    }

    /// The `plaintext` arm of [`revise_field_checked`](Self::revise_field_checked).
    /// No markdown import in either direction, so escapes, `*`, and `_` are
    /// ordinary characters on both sides of the diff and a byte-identical revise
    /// is a byte no-op. The `Delta` is over the literal text.
    fn revise_field_plaintext(
        &mut self,
        name: &str,
        text: impl Into<String>,
        schema: &FieldSchema,
    ) -> Result<Delta, EditError> {
        // Ahead of the read: a directly-constructed payload can hold an ill-named
        // field, whose decode error would otherwise mask the bad name.
        if !is_valid_field_name(name) {
            return Err(EditError::InvalidFieldName(name.to_string()));
        }
        let base = match self.field_text(name, Codec::Plaintext) {
            Some(Ok(text)) => text,
            Some(Err(e)) => return Err(field_decode(name, &[], Codec::Plaintext, e)),
            None => String::new(),
        };
        // The strict write is the codec: it runs the `from_plaintext` boundary
        // cleanup, so the committed string is what the diff must measure against.
        let stored = resolve_field_write(name, QuillValue::from(text.into()), schema)?;
        let delta = quillmark_content::delta::diff(
            &base,
            stored
                .as_json()
                .as_str()
                .expect("a plaintext field rests as a string"),
        );
        self.payload_mut().insert_unchecked(name.to_string(), stored);
        Ok(delta)
    }

    /// Apply a committed field-change bundle to the body content. Order is text
    /// delta → island ops → line ops → mark ops, then one terminal
    /// normalization; mark ranges are in final-text coordinates. Returns
    /// [`EditError::ContentApply`] when an op is out of bounds; the apply is
    /// all-or-nothing, so the body is unchanged on error.
    pub fn apply_body_change(&mut self, bundle: &ChangeBundle) -> Result<(), EditError> {
        self.body_mut()
            .apply_field_change(bundle)
            .map_err(EditError::ContentApply)
    }

    /// Splice a content field-change bundle into a field's stored content: the
    /// field-path twin of [`apply_body_change`](Self::apply_body_change), and
    /// what lets identity marks persist on field content across incremental
    /// edits. Decodes the field's canonical content, applies the all-or-nothing
    /// bundle, and re-stores the canonical result.
    ///
    /// An **absent** field splices against the empty content. A bundle that
    /// expected content still fails: its text delta declares the base length it
    /// was computed against, so only a zero-base bundle lands.
    ///
    /// Returns [`EditError::InvalidFieldName`] for a malformed name,
    /// [`EditError::FieldDecode`] when the stored value is not a content, and
    /// [`EditError::ContentApply`] when the bundle applies out of bounds.
    ///
    /// **Richtext codec**: schema-blind like [`revise_field`](Self::revise_field),
    /// so a `plaintext` field's stored string decodes here as markdown and a
    /// splice lands it off its resting form until the next bound load.
    pub fn apply_field_change(
        &mut self,
        name: &str,
        bundle: &ChangeBundle,
    ) -> Result<(), EditError> {
        let mut content = match self.field_content(name, Codec::Richtext) {
            Some(Ok(rt)) => rt,
            Some(Err(e)) => return Err(field_decode(name, &[], Codec::Richtext, e)),
            // Only this arm creates; the `Some` arms resolved an existing field.
            None => {
                if !is_valid_field_name(name) {
                    return Err(EditError::InvalidFieldName(name.to_string()));
                }
                Normalized::empty()
            }
        };
        content
            .apply_field_change(bundle)
            .map_err(EditError::ContentApply)?;
        self.store_field_content(name, &content);
        Ok(())
    }
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
    fn the_store_verbs_enforce_the_field_invariant() {
        let qv = QuillValue::from_json;
        let mut card = Card::new("note").unwrap();

        for name in ["bad name", "$id"] {
            assert_eq!(
                card.store_field(name, qv(serde_json::json!("v"))),
                Err(EditError::InvalidFieldName(name.to_string()))
            );
            assert_eq!(
                card.store_fill(name, qv(serde_json::json!("v"))),
                Err(EditError::InvalidFieldName(name.to_string()))
            );
        }

        let mut deep = serde_json::json!(0);
        for _ in 0..(quillmark_content::MAX_JSON_DEPTH + 5) {
            deep = serde_json::json!([deep]);
        }
        assert_eq!(
            card.store_field("field", qv(deep)),
            Err(EditError::ValueTooDeep {
                max: quillmark_content::MAX_JSON_DEPTH
            })
        );

        assert!(card.payload().is_empty());
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
