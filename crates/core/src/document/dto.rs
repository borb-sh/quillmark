//! Versioned, storage-stable serialization for [`Document`].
//!
//! [`Document`]'s in-memory layout is an internal detail and is deliberately not
//! serialized directly. Persisting one converts it to a [`StoredDocument`]: a
//! versioned envelope whose wire format is frozen per schema version. `Document`
//! serializes through it via `#[serde(into / try_from)]`, so the ordinary serde
//! entry points produce and consume the versioned form transparently.
//!
//! The schema versions, the shape each names, and the procedure for adding one:
//! `prose/canon/DOCUMENT_STORAGE.md`.

#![allow(non_camel_case_types)]

use std::collections::HashSet;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use quillmark_content::import::ImportError;
use quillmark_content::model::{Container, Content, MarkKind, Normalized};

use super::meta::validate_composable_kind;
use super::payload::{MetaKey, Payload, PayloadItem};
use super::prescan::NestedComment;
use crate::value::PathSegment;
use super::{Card, Document};
use crate::value::QuillValue;
use crate::version::QuillReference;

/// Storage version tag newly serialized documents carry.
///
/// The wire key is spelled `schema` though it names a storage version, not a
/// field schema: it is the serde tag [`StoredDocument`] dispatches on, and
/// retagging it would break the versioning it exists to serve.
pub const STORAGE_V0_116_0: &str = "quillmark/document@0.116.0";

/// The tag before [`STORAGE_V0_116_0`], still read.
pub const STORAGE_V0_115_0: &str = "quillmark/document@0.115.0";

/// The tag before [`STORAGE_V0_115_0`], still read.
pub const STORAGE_V0_112_0: &str = "quillmark/document@0.112.0";

/// The tag before [`STORAGE_V0_112_0`], still read.
pub const STORAGE_V0_93_0: &str = "quillmark/document@0.93.0";

/// Read the storage version off a raw DTO payload without deserializing it.
///
/// `None` when `json` is not valid JSON, not an object, or carries no version
/// tag. The returned string is **not** validated against the supported set:
/// callers use it to tell "unknown future version" from "corrupt payload".
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
/// variant, leaving existing ones byte-stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema")]
pub enum StoredDocument {
    #[serde(rename = "quillmark/document@0.116.0")]
    V0_116_0(DocumentV0_116_0),
    #[serde(rename = "quillmark/document@0.115.0")]
    V0_115_0(DocumentV0_115_0),
    #[serde(rename = "quillmark/document@0.112.0")]
    V0_112_0(DocumentV0_112_0),
    #[serde(rename = "quillmark/document@0.93.0")]
    V0_93_0(DocumentV0_93_0),
    #[serde(rename = "quillmark/document@0.92.0")]
    V0_92_0(DocumentV0_92_0),
    #[serde(rename = "quillmark/document@0.82.0")]
    V0_82_0(DocumentV0_82_0),
    #[serde(rename = "quillmark/document@0.81.0")]
    V0_81_0(DocumentV0_81_0),
}

/// Failure while reconstructing a [`Document`] from a [`StoredDocument`].
///
/// Only [`Self::InvalidQuillReference`] is typed: it is the one error a
/// non-malicious caller hits. Every other defect can only arise from a
/// hand-crafted storage DTO and reports through [`Self::Malformed`].
#[derive(Debug, Clone, PartialEq)]
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

/// Frozen `0.116.0` representation of a [`Document`]: the V0_115_0 tree over a
/// payload whose fields carry no placeholder marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_116_0 {
    pub main: CardV0_116_0,
    #[serde(default)]
    pub cards: Vec<CardV0_116_0>,
}

/// Frozen `0.116.0` representation of a [`Card`]. The `body` is the canonical
/// content embedded structurally (see [`CanonicalContent`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_116_0 {
    pub payload: PayloadV0_116_0,
    pub body: CanonicalContent,
}

/// Frozen `0.116.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PayloadV0_116_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_116_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_92_0>,
}

/// Frozen `0.116.0` representation of a unified payload item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemV0_116_0 {
    /// `$quill` system metadata: the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
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

/// Frozen `0.115.0` representation of a [`Document`]: structurally the V0_112_0
/// tree, over the content form that spells a block island's line `para` rather
/// than `island`. Read-only, and its `body` is raw JSON for the reason
/// [`DocumentV0_93_0`] states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_115_0 {
    pub main: CardV0_115_0,
    #[serde(default)]
    pub cards: Vec<CardV0_115_0>,
}

/// Frozen `0.115.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_115_0 {
    pub payload: PayloadV0_115_0,
    pub body: serde_json::Value,
}

/// The V0_115_0 payload shape: identical to V0_92_0.
pub type PayloadV0_115_0 = PayloadV0_92_0;

/// Frozen `0.112.0` representation of a [`Document`]. Mirrors `DocumentV0_92_0`;
/// the only structural change is `Card.body` (see [`CardV0_112_0`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_112_0 {
    pub main: CardV0_112_0,
    #[serde(default)]
    pub cards: Vec<CardV0_112_0>,
}

/// Frozen `0.112.0` representation of a [`Card`]. Read-only, and its `body` is
/// raw JSON for the reason [`DocumentV0_93_0`] states: [`CanonicalContent`]
/// tracks the live crate, so embedding it here would make this tree *write* the
/// current spelling under the `@0.112.0` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_112_0 {
    pub payload: PayloadV0_112_0,
    pub body: serde_json::Value,
}

/// The V0_112_0 payload shape: identical to V0_92_0.
pub type PayloadV0_112_0 = PayloadV0_92_0;

/// A card body embedded as the **canonical content**. Its serde delegates to the
/// frozen canonical serializer (`quillmark_content::serial`) rather than a
/// hand-mirrored DTO tree that could drift from it:
///
/// - `Serialize` validates, then emits the recursively key-sorted structure
///   byte-identical to `to_canonical_json()` as a **nested JSON object**, never
///   an escaped string, independent of `preserve_order`.
/// - `Deserialize` parses, normalizes, and validates, so an invalid content is
///   rejected at load rather than silently round-tripped.
///
/// Both directions check because [`Normalized`] is the canonical-form token and
/// not a validity one: `validate` refuses only what `normalize` cannot repair,
/// and `Card::overwrite_body` takes a caller's content on that token alone. A
/// store that checked only on load would accept bytes it cannot read back.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalContent(pub Normalized);

impl Serialize for CanonicalContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .validate()
            .map_err(|inv| serde::ser::Error::custom(format!("content invariant: {inv:?}")))?;
        quillmark_content::serial::to_canonical_value(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CanonicalContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let rt = quillmark_content::serial::from_canonical_value(&value)
            .map_err(serde::de::Error::custom)?;
        Ok(CanonicalContent(rt))
    }
}

/// Frozen `0.93.0` representation of a [`Document`]: structurally the V0_112_0
/// tree, over the content form that spelled a built-in's payload as named
/// siblings rather than in the `attrs` bag.
///
/// Read-only, and its `body` is raw JSON rather than [`CanonicalContent`]. The
/// live type tracks the live crate, so embedding it here would make this tree
/// *write* the current spelling under the `@0.93.0` tag — the one thing a
/// frozen tree must not do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_93_0 {
    pub main: CardV0_93_0,
    #[serde(default)]
    pub cards: Vec<CardV0_93_0>,
}

/// Frozen `0.93.0` representation of a [`Card`]. See [`DocumentV0_93_0`] for why
/// `body` is raw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_93_0 {
    pub payload: PayloadV0_93_0,
    pub body: serde_json::Value,
}

/// The V0_93_0 payload shape: identical to V0_92_0. The `body` is where the two
/// document versions differ.
pub type PayloadV0_93_0 = PayloadV0_92_0;

/// Frozen `0.92.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_92_0 {
    pub main: CardV0_92_0,
    #[serde(default)]
    pub cards: Vec<CardV0_92_0>,
}

/// Frozen `0.92.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_92_0 {
    pub payload: PayloadV0_92_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.92.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PayloadV0_92_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_92_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_92_0>,
}

/// Frozen `0.92.0` representation of a unified payload item. Carries the `Seed`
/// variant and a per-`Field` `nested_fills` list: the paths of the retired
/// `!must_fill` markers nested inside the field value.
///
/// A shipped schema version never changes, so a new item kind is a new schema
/// version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemV0_92_0 {
    /// `$quill` system metadata: the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
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
        #[serde(default)]
        fill: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        nested_fills: Vec<Vec<CommentPathSegmentV0_92_0>>,
    },
    /// A YAML comment.
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.92.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_92_0 {
    pub container_path: Vec<CommentPathSegmentV0_92_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.92.0` representation of a [`PathSegment`]. Also used for
/// `nested_fills` path segments, and by every later tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_92_0 {
    Key(String),
    Index(usize),
}

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_116_0(DocumentV0_116_0::from(&doc))
    }
}

impl From<&Document> for DocumentV0_116_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_116_0 {
            main: CardV0_116_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_116_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_116_0 {
    fn from(card: &Card) -> Self {
        CardV0_116_0 {
            payload: PayloadV0_116_0::from(card.payload()),
            body: CanonicalContent(card.body().clone()),
        }
    }
}

impl From<&Payload> for PayloadV0_116_0 {
    fn from(payload: &Payload) -> Self {
        PayloadV0_116_0 {
            items: payload
                .items()
                .iter()
                .map(PayloadItemV0_116_0::from)
                .collect(),
            nested_comments: payload
                .nested_comments()
                .iter()
                .map(NestedCommentV0_92_0::from)
                .collect(),
        }
    }
}

impl From<&PayloadItem> for PayloadItemV0_116_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Quill { reference } => PayloadItemV0_116_0::Quill {
                value: reference.to_string(),
            },
            PayloadItem::Kind { value } => PayloadItemV0_116_0::Kind {
                value: value.clone(),
            },
            // The DTO keeps `$ext` / `$seed` as explicit variants, so the live
            // model's unified `Meta` splits back out by key. Neither carries
            // `nested_comments`: those live in the payload-level sidecar.
            PayloadItem::Meta {
                key: MetaKey::Ext,
                value,
            } => PayloadItemV0_116_0::Ext {
                value: value.clone(),
            },
            PayloadItem::Meta {
                key: MetaKey::Seed,
                value,
            } => PayloadItemV0_116_0::Seed {
                value: value.clone(),
            },
            PayloadItem::Field { key, value } => PayloadItemV0_116_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
            },
            PayloadItem::Comment { text, inline } => PayloadItemV0_116_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_92_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_92_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_92_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&PathSegment> for CommentPathSegmentV0_92_0 {
    fn from(seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => CommentPathSegmentV0_92_0::Key(k.clone()),
            PathSegment::Index(i) => CommentPathSegmentV0_92_0::Index(*i),
        }
    }
}

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        // Only the newest DTO converts to the live model; older versions migrate
        // forward (V0_81 → V0_82 → V0_92 → V0_116, with V0_93 → V0_112 →
        // V0_115 → V0_116 beside it). The hops into V0_115 are retags: every
        // tree up to it spells `body` as raw JSON, and the decode they defer is
        // the V0_115 → V0_116 hop's.
        //
        // The V0_92 chain lands on V0_116 rather than passing through V0_115: a
        // cold import yields the live content, and re-spelling it *back* to a
        // raw `body` only to read it forward again would need an encoder for
        // that form, which this crate does not have.
        match stored {
            StoredDocument::V0_116_0(payload) => Document::try_from(payload),
            StoredDocument::V0_115_0(payload) => {
                Document::try_from(DocumentV0_116_0::try_from(payload)?)
            }
            StoredDocument::V0_112_0(payload) => Document::try_from(DocumentV0_116_0::try_from(
                DocumentV0_115_0::from(payload),
            )?),
            StoredDocument::V0_93_0(payload) => Document::try_from(DocumentV0_116_0::try_from(
                DocumentV0_115_0::from(DocumentV0_112_0::from(payload)),
            )?),
            StoredDocument::V0_92_0(payload) => {
                Document::try_from(DocumentV0_116_0::try_from(payload)?)
            }
            StoredDocument::V0_82_0(payload) => Document::try_from(DocumentV0_116_0::try_from(
                DocumentV0_92_0::from(payload),
            )?),
            StoredDocument::V0_81_0(payload) => Document::try_from(DocumentV0_116_0::try_from(
                DocumentV0_92_0::from(DocumentV0_82_0::from(payload)),
            )?),
        }
    }
}

impl TryFrom<DocumentV0_116_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_116_0) -> Result<Self, Self::Error> {
        let mut main = Card::try_from(payload.main)?;
        if main.quill().is_none() {
            return Err(StorageError::Malformed(
                "main card must carry a $quill entry".into(),
            ));
        }
        // The root's `$kind` is `main` by position: any other value emits a root
        // block the parser rejects. An absent one is synthesised so the emit
        // stays parseable, as the parser and the V0_81_0 hop both do.
        match main.kind() {
            Some("main") => {}
            None => main.payload_mut().set_kind("main"),
            Some(other) => {
                return Err(StorageError::Malformed(format!(
                    "main card has $kind {other:?}, but `main` is reserved for \
                     the document root"
                )))
            }
        }
        let cards = payload
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let mut kindless = false;
        for card in &cards {
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
                kindless = true;
                continue;
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
        if kindless {
            return fold_kindless(main, cards);
        }
        Ok(Document::from_main_and_cards(main, cards))
    }
}

/// Fold each card naming no `$kind` into the body above it: its fields and
/// comments as a code block, then its own body. A composable card names its
/// kind, so this is the one reading of such a stored card that keeps its text.
/// Its `$ext` drops, since `$ext` never reaches a render and the body does.
/// Every other card loads untouched.
fn fold_kindless(mut main: Card, cards: Vec<Card>) -> Result<Document, StorageError> {
    let malformed = |e: &dyn std::fmt::Debug| {
        StorageError::Malformed(format!("folding a card with no $kind: {e:?}"))
    };
    let mut kept: Vec<Card> = Vec::with_capacity(cards.len());
    for card in cards {
        if card.kind().is_some() {
            kept.push(card);
            continue;
        }
        let target = match kept.last_mut() {
            Some(card) => card,
            None => &mut main,
        };
        let mut payload = card.payload().clone();
        payload.take_ext();
        let mut yaml = String::new();
        super::emit::emit_payload_items(&mut yaml, &payload);
        let code = code_block(&yaml).map_err(|e| malformed(&e))?;
        let body = append_block(target.body().clone().into_content(), code);
        let body = append_block(body, card.body().clone().into_content()).into_normalized();
        body.validate().map_err(|e| malformed(&e))?;
        *target.body_mut() = body;
    }
    Ok(Document::from_main_and_cards(main, kept))
}

/// `yaml` as one untagged code block, imported as markdown so its text is
/// admitted as any body's is: a field value may hold a bidi control or a line
/// separator that body text refuses.
fn code_block(yaml: &str) -> Result<Content, ImportError> {
    if yaml.trim().is_empty() {
        return Ok(Content::empty());
    }
    let longest = yaml.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let newline = if yaml.ends_with('\n') { "" } else { "\n" };
    super::import_body(&format!("{fence}\n{yaml}{newline}{fence}\n")).map(Normalized::into_content)
}

/// `src` as the blocks following `dst`'s, each its own block: its top-level
/// containers never continue `dst`'s last run. `src` lands as new content, so
/// an island id `dst` already holds is minted anew past the highest `isl-{n}`
/// either holds, and an anchor whose id `dst` already holds drops whole.
fn append_block(mut dst: Content, mut src: Content) -> Content {
    let void = |c: &Content| c.text.is_empty() && c.marks.is_empty() && c.islands.is_empty();
    if void(&src) {
        return dst;
    }
    if void(&dst) {
        return src;
    }
    let offset = dst.len_usv() + 1;

    let held: HashSet<&str> = dst.islands.iter().map(|i| i.id.as_str()).collect();
    let mut next = dst
        .islands
        .iter()
        .chain(&src.islands)
        .filter_map(|i| i.id.strip_prefix("isl-")?.parse::<u64>().ok())
        .max()
        .map_or(0, |n| n.saturating_add(1));
    for island in &mut src.islands {
        if held.contains(island.id.as_str()) {
            island.id = format!("isl-{next}");
            next = next.saturating_add(1);
        }
    }

    let held: HashSet<&str> = dst
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    src.marks
        .retain(|m| !matches!(&m.kind, MarkKind::Anchor { id } if held.contains(id.as_str())));

    // A run opens where the stored `instance` changes, so lifting every
    // top-level instance past `dst`'s keeps `src`'s runs its own.
    let lift = dst
        .lines
        .iter()
        .filter_map(|l| l.containers.first().map(container_instance))
        .max()
        .map_or(0, |max| max + 1);
    for line in &mut src.lines {
        if let Some(top) = line.containers.first_mut() {
            match top {
                Container::ListItem { instance, .. } | Container::Quote { instance } => {
                    *instance += lift;
                }
            }
        }
    }

    if let Some(first) = src.lines.first_mut() {
        first.continues = false;
    }
    dst.text.push('\n');
    dst.text.push_str(&src.text);
    dst.lines.append(&mut src.lines);
    dst.marks.extend(src.marks.into_iter().map(|mut m| {
        m.start += offset;
        m.end += offset;
        m
    }));
    dst.islands.append(&mut src.islands);
    dst
}

fn container_instance(container: &Container) -> u64 {
    match container {
        Container::ListItem { instance, .. } | Container::Quote { instance } => *instance,
    }
}

impl TryFrom<CardV0_116_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_116_0) -> Result<Self, Self::Error> {
        let payload = Payload::try_from(card.payload)?;
        validate_dto_payload(&payload)?;
        // `CanonicalContent`'s Deserialize already normalized and validated it.
        Ok(Card::from_parts(payload, card.body.0))
    }
}

// The hop into V0_116 decodes the raw `body` the older trees defer — which is
// also where an invalid legacy body is caught, `CanonicalContent`'s parse-time
// check not being available to a raw field — and drops the retired
// placeholder markers ([`PayloadV0_116_0::from`]). The content decoder reads
// every retired spelling (an `island` line as `para`, an island's `loss`
// ignored), and the row rewrites under the new tag without them.

impl TryFrom<DocumentV0_115_0> for DocumentV0_116_0 {
    type Error = StorageError;

    fn try_from(d: DocumentV0_115_0) -> Result<Self, Self::Error> {
        Ok(DocumentV0_116_0 {
            main: CardV0_116_0::try_from(d.main)?,
            cards: d
                .cards
                .into_iter()
                .map(CardV0_116_0::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<CardV0_115_0> for CardV0_116_0 {
    type Error = StorageError;

    fn try_from(card: CardV0_115_0) -> Result<Self, Self::Error> {
        let body = quillmark_content::serial::from_canonical_value(&card.body)
            .map_err(|e| StorageError::Malformed(format!("card body: {e}")))?;
        Ok(CardV0_116_0 {
            payload: PayloadV0_116_0::from(card.payload),
            body: CanonicalContent(body),
        })
    }
}

impl From<DocumentV0_112_0> for DocumentV0_115_0 {
    fn from(d: DocumentV0_112_0) -> Self {
        DocumentV0_115_0 {
            main: CardV0_115_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_115_0::from).collect(),
        }
    }
}

impl From<CardV0_112_0> for CardV0_115_0 {
    fn from(card: CardV0_112_0) -> Self {
        CardV0_115_0 {
            payload: card.payload,
            body: card.body,
        }
    }
}

// Both trees spell `body` raw, so the older tag's rows enter the chain by
// retagging alone and the decode above answers for both.

impl From<DocumentV0_93_0> for DocumentV0_112_0 {
    fn from(d: DocumentV0_93_0) -> Self {
        DocumentV0_112_0 {
            main: CardV0_112_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_112_0::from).collect(),
        }
    }
}

impl From<CardV0_93_0> for CardV0_112_0 {
    fn from(card: CardV0_93_0) -> Self {
        CardV0_112_0 {
            payload: card.payload,
            body: card.body,
        }
    }
}

// The stored markdown body cold-imports to a content. An over-nested body never
// rendered, so mapping it to `StorageError::Malformed` loses nothing
// renderable. Byte-stability of a *migrated* row is therefore conditional on
// `pulldown-cmark` (DOCUMENT_STORAGE.md § byte stability).

impl TryFrom<DocumentV0_92_0> for DocumentV0_116_0 {
    type Error = StorageError;

    fn try_from(d: DocumentV0_92_0) -> Result<Self, Self::Error> {
        Ok(DocumentV0_116_0 {
            main: CardV0_116_0::try_from(d.main)?,
            cards: d
                .cards
                .into_iter()
                .map(CardV0_116_0::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<CardV0_92_0> for CardV0_116_0 {
    type Error = StorageError;

    fn try_from(card: CardV0_92_0) -> Result<Self, Self::Error> {
        let body = super::import_body(&card.body)
            .map_err(|e| StorageError::Malformed(format!("card body: {e}")))?;
        Ok(CardV0_116_0 {
            payload: PayloadV0_116_0::from(card.payload),
            body: CanonicalContent(body),
        })
    }
}

/// Drop the retired placeholder markers: a value the `!must_fill` tag held, at
/// the field's root or nested inside it, was a placeholder rather than an
/// answer, so it reads as null — the field unanswered.
impl From<PayloadV0_92_0> for PayloadV0_116_0 {
    fn from(p: PayloadV0_92_0) -> Self {
        PayloadV0_116_0 {
            items: p
                .items
                .into_iter()
                .map(|item| match item {
                    PayloadItemV0_92_0::Quill { value } => PayloadItemV0_116_0::Quill { value },
                    PayloadItemV0_92_0::Kind { value } => PayloadItemV0_116_0::Kind { value },
                    PayloadItemV0_92_0::Ext { value } => PayloadItemV0_116_0::Ext { value },
                    PayloadItemV0_92_0::Seed { value } => PayloadItemV0_116_0::Seed { value },
                    PayloadItemV0_92_0::Field {
                        key,
                        mut value,
                        fill,
                        nested_fills,
                    } => {
                        if fill {
                            value = serde_json::Value::Null;
                        }
                        for path in nested_fills {
                            let path: Vec<PathSegment> =
                                path.into_iter().map(PathSegment::from).collect();
                            crate::value::null_at(&mut value, &path);
                        }
                        PayloadItemV0_116_0::Field { key, value }
                    }
                    PayloadItemV0_92_0::Comment { text, inline } => {
                        PayloadItemV0_116_0::Comment { text, inline }
                    }
                })
                .collect(),
            nested_comments: p.nested_comments,
        }
    }
}

impl TryFrom<PayloadV0_116_0> for Payload {
    type Error = StorageError;

    fn try_from(p: PayloadV0_116_0) -> Result<Self, Self::Error> {
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

impl TryFrom<PayloadItemV0_116_0> for PayloadItem {
    type Error = StorageError;

    fn try_from(item: PayloadItemV0_116_0) -> Result<Self, Self::Error> {
        Ok(match item {
            PayloadItemV0_116_0::Quill { value } => {
                let reference = QuillReference::from_str(&value).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: value.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            PayloadItemV0_116_0::Kind { value } => PayloadItem::Kind { value },
            PayloadItemV0_116_0::Ext { value } => PayloadItem::Meta {
                key: MetaKey::Ext,
                value: depth_check_meta_map(value, "$ext")?,
            },
            PayloadItemV0_116_0::Seed { value } => PayloadItem::Meta {
                key: MetaKey::Seed,
                value: depth_check_meta_map(value, "$seed")?,
            },
            PayloadItemV0_116_0::Field { key, value } => {
                super::edit::validate_field(&key, &value)
                    .map_err(|v| StorageError::Malformed(v.message(&key)))?;
                PayloadItem::Field {
                    key,
                    value: QuillValue::from_json(value),
                }
            }
            PayloadItemV0_116_0::Comment { text, inline } => PayloadItem::Comment { text, inline },
        })
    }
}

/// Depth-bound a `$ext` / `$seed` mapping at the storage boundary (§8).
fn depth_check_meta_map(
    value: serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, StorageError> {
    crate::value::depth_check_meta_map(value, |max| {
        StorageError::Malformed(format!("{key} nests deeper than the maximum of {} levels", max))
    })
}

impl From<NestedCommentV0_92_0> for NestedComment {
    fn from(nc: NestedCommentV0_92_0) -> Self {
        NestedComment {
            container_path: nc
                .container_path
                .into_iter()
                .map(PathSegment::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_92_0> for PathSegment {
    fn from(seg: CommentPathSegmentV0_92_0) -> Self {
        match seg {
            CommentPathSegmentV0_92_0::Key(k) => PathSegment::Key(k),
            CommentPathSegmentV0_92_0::Index(i) => PathSegment::Index(i),
        }
    }
}

/// Frozen `0.81.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_81_0 {
    pub main: CardV0_81_0,
    #[serde(default)]
    pub cards: Vec<CardV0_81_0>,
}

/// Frozen `0.81.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_81_0 {
    pub sentinel: SentinelV0_81_0,
    #[serde(default)]
    pub frontmatter: FrontmatterV0_81_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.81.0` representation of a card discriminator (sentinel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SentinelV0_81_0 {
    Main { quill: String },
    Card { tag: String },
}

/// Frozen `0.81.0` representation of a card payload (user fields only).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FrontmatterV0_81_0 {
    #[serde(default)]
    pub items: Vec<FrontmatterItemV0_81_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_81_0>,
}

/// Frozen `0.81.0` representation of a payload item. The `$` entries live in
/// the sentinel, and neither `$ext` (`0.83.0`) nor `$seed` (`0.92.0`) existed,
/// so `Field` and `Comment` are the whole set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FrontmatterItemV0_81_0 {
    Field {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        fill: bool,
    },
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.81.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_81_0 {
    pub container_path: Vec<CommentPathSegmentV0_81_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.81.0` representation of a [`PathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_81_0 {
    Key(String),
    Index(usize),
}

// Quill-reference, field-name, and depth validation happen once, further down
// the chain.

impl From<DocumentV0_81_0> for DocumentV0_82_0 {
    fn from(d: DocumentV0_81_0) -> Self {
        DocumentV0_82_0 {
            main: CardV0_82_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_82_0::from).collect(),
        }
    }
}

impl From<CardV0_81_0> for CardV0_82_0 {
    fn from(c: CardV0_81_0) -> Self {
        let mut items: Vec<PayloadItemV0_82_0> = Vec::new();

        // The sentinel leaves `$kind: main` implicit; the live model needs it
        // explicit for the markdown emit to produce a parseable document.
        match c.sentinel {
            SentinelV0_81_0::Main { quill } => {
                items.push(PayloadItemV0_82_0::Quill { value: quill });
                items.push(PayloadItemV0_82_0::Kind {
                    value: "main".into(),
                });
            }
            SentinelV0_81_0::Card { tag } => {
                items.push(PayloadItemV0_82_0::Kind { value: tag });
            }
        }

        // Comments migrate as-is, landing after the `$` prelude: V0_81_0
        // tracks no `$`-line comments to interleave them with.
        for item in c.frontmatter.items {
            items.push(match item {
                FrontmatterItemV0_81_0::Field { key, value, fill } => {
                    PayloadItemV0_82_0::Field { key, value, fill }
                }
                FrontmatterItemV0_81_0::Comment { text, inline } => {
                    PayloadItemV0_82_0::Comment { text, inline }
                }
            });
        }

        CardV0_82_0 {
            payload: PayloadV0_82_0 {
                items,
                nested_comments: c
                    .frontmatter
                    .nested_comments
                    .into_iter()
                    .map(NestedCommentV0_82_0::from)
                    .collect(),
            },
            body: c.body,
        }
    }
}

impl From<NestedCommentV0_81_0> for NestedCommentV0_82_0 {
    fn from(nc: NestedCommentV0_81_0) -> Self {
        NestedCommentV0_82_0 {
            container_path: nc
                .container_path
                .into_iter()
                .map(CommentPathSegmentV0_82_0::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_81_0> for CommentPathSegmentV0_82_0 {
    fn from(seg: CommentPathSegmentV0_81_0) -> Self {
        match seg {
            CommentPathSegmentV0_81_0::Key(k) => CommentPathSegmentV0_82_0::Key(k),
            CommentPathSegmentV0_81_0::Index(i) => CommentPathSegmentV0_82_0::Index(i),
        }
    }
}

/// Frozen `0.82.0` representation of a [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentV0_82_0 {
    pub main: CardV0_82_0,
    #[serde(default)]
    pub cards: Vec<CardV0_82_0>,
}

/// Frozen `0.82.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardV0_82_0 {
    pub payload: PayloadV0_82_0,
    #[serde(default)]
    pub body: String,
}

/// Frozen `0.82.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PayloadV0_82_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_82_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_82_0>,
}

/// Frozen `0.82.0` representation of a unified payload item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemV0_82_0 {
    /// `$quill` system metadata: the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
    /// `$id` system metadata. The live model has no counterpart, so this is
    /// the one item the forward migration drops.
    Id { value: String },
    /// `$ext` system metadata: an opaque mapping carrying out-of-band
    /// extension data.
    Ext {
        value: serde_json::Map<String, serde_json::Value>,
    },
    /// A user-defined field.
    Field {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        fill: bool,
    },
    /// A YAML comment.
    Comment {
        text: String,
        #[serde(default)]
        inline: bool,
    },
}

/// Frozen `0.82.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_82_0 {
    pub container_path: Vec<CommentPathSegmentV0_82_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.82.0` representation of a [`PathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_82_0 {
    Key(String),
    Index(usize),
}

// Lossy by decision: `$id` has no live counterpart.

impl From<DocumentV0_82_0> for DocumentV0_92_0 {
    fn from(d: DocumentV0_82_0) -> Self {
        DocumentV0_92_0 {
            main: CardV0_92_0::from(d.main),
            cards: d.cards.into_iter().map(CardV0_92_0::from).collect(),
        }
    }
}

impl From<CardV0_82_0> for CardV0_92_0 {
    fn from(c: CardV0_82_0) -> Self {
        CardV0_92_0 {
            payload: PayloadV0_92_0::from(c.payload),
            body: c.body,
        }
    }
}

impl From<PayloadV0_82_0> for PayloadV0_92_0 {
    fn from(p: PayloadV0_82_0) -> Self {
        PayloadV0_92_0 {
            items: p
                .items
                .into_iter()
                .filter_map(PayloadItemV0_92_0::from_v0_82_0)
                .collect(),
            nested_comments: p
                .nested_comments
                .into_iter()
                .map(NestedCommentV0_92_0::from)
                .collect(),
        }
    }
}

impl PayloadItemV0_92_0 {
    /// `None` for `$id`, which has no live counterpart.
    fn from_v0_82_0(item: PayloadItemV0_82_0) -> Option<Self> {
        Some(match item {
            PayloadItemV0_82_0::Id { .. } => return None,
            PayloadItemV0_82_0::Quill { value } => PayloadItemV0_92_0::Quill { value },
            PayloadItemV0_82_0::Kind { value } => PayloadItemV0_92_0::Kind { value },
            PayloadItemV0_82_0::Ext { value } => PayloadItemV0_92_0::Ext { value },
            PayloadItemV0_82_0::Field { key, value, fill } => PayloadItemV0_92_0::Field {
                key,
                value,
                fill,
                nested_fills: Vec::new(),
            },
            PayloadItemV0_82_0::Comment { text, inline } => {
                PayloadItemV0_92_0::Comment { text, inline }
            }
        })
    }
}

impl From<NestedCommentV0_82_0> for NestedCommentV0_92_0 {
    fn from(nc: NestedCommentV0_82_0) -> Self {
        NestedCommentV0_92_0 {
            container_path: nc
                .container_path
                .into_iter()
                .map(CommentPathSegmentV0_92_0::from)
                .collect(),
            position: nc.position,
            text: nc.text,
            inline: nc.inline,
        }
    }
}

impl From<CommentPathSegmentV0_82_0> for CommentPathSegmentV0_92_0 {
    fn from(seg: CommentPathSegmentV0_82_0) -> Self {
        match seg {
            CommentPathSegmentV0_82_0::Key(k) => CommentPathSegmentV0_92_0::Key(k),
            CommentPathSegmentV0_82_0::Index(i) => CommentPathSegmentV0_92_0::Index(i),
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
    fn round_trips_through_serde_json() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, restored);
        assert_eq!(doc.to_markdown(), restored.to_markdown());
    }

    /// `overwrite_body` takes a caller's content on the canonical-form token
    /// alone, so an invalid shape reaches the DTO.
    #[test]
    fn an_unreadable_body_is_refused_on_write() {
        use quillmark_content::model::{Container, Content, Line, LineKind};

        let mut doc = sample();
        let mut line = Line::new(LineKind::Para);
        line.containers =
            vec![Container::Quote { instance: 0 }; quillmark_content::MAX_NESTING_DEPTH + 1];
        doc.main_mut()
            .overwrite_body(Content::new("x".to_string(), vec![line]));
        let err = serde_json::to_string(&doc).expect_err("write refuses what load would reject");
        assert!(err.to_string().contains("NestingTooDeep"), "{err}");
    }

    #[test]
    fn content_field_survives_storage_round_trip_losslessly() {
        // The storage DTO is the lossless carrier: an `underline` with no
        // markdown form survives here where a markdown save would drop it.
        use quillmark_content::model::{Mark, MarkKind};

        let mut doc = sample();
        let mut content = quillmark_content::import::from_markdown("underlined intro").unwrap().into_content();
        content.marks.push(Mark::new(0, 10, MarkKind::Underline));
        let content = content.into_normalized();
        let json = quillmark_content::serial::to_canonical_value(&content);
        let schema = crate::quill::FieldSchema::new(
            "intro".to_string(),
            crate::quill::FieldType::RichText { inline: false },
            None,
        );
        doc.main_mut()
            .commit_field("intro", crate::value::QuillValue::from_json(json), &schema)
            .unwrap();

        let stored = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&stored).unwrap();
        assert_eq!(doc, restored, "content field must survive storage round-trip");
        let read = restored.main().field_content("intro", Codec::Richtext).unwrap().unwrap();
        assert!(
            read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)),
            "underline (content-only) must survive the DTO carrier"
        );
    }

    /// The `@0.116.0` hop's payload half: a value the retired `!must_fill`
    /// marker held was a placeholder, so it reads as null wherever it sat.
    #[test]
    fn a_0_115_0_row_drops_its_placeholders() {
        let legacy = serde_json::json!({
            "schema": "quillmark/document@0.115.0",
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                    { "type": "field", "key": "subject", "value": "Example", "fill": true },
                    { "type": "field", "key": "addr", "fill": false,
                      "value": {"street": "1 Main", "city": "Anytown"},
                      "nested_fills": [[{"Key": "street"}]] },
                    { "type": "field", "key": "to", "fill": false,
                      "value": [{"name": "Jane"}, {"name": "Real"}],
                      "nested_fills": [[{"Index": 0}, {"Key": "name"}], [{"Key": "gone"}]] },
                    { "type": "field", "key": "title", "value": "Kept", "fill": false },
                ], "nested_comments": [] },
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}],
                         "marks": [], "text": ""},
            },
            "cards": [],
        })
        .to_string();

        let doc: Document = serde_json::from_str(&legacy).expect("a 0.115.0 blob still loads");
        let get = |k: &str| doc.main().payload().get(k).unwrap().as_json().clone();
        assert_eq!(get("subject"), serde_json::Value::Null);
        assert_eq!(get("addr"), serde_json::json!({"street": null, "city": "Anytown"}));
        assert_eq!(get("to"), serde_json::json!([{"name": null}, {"name": "Real"}]));
        assert_eq!(get("title"), serde_json::json!("Kept"));

        let rewritten = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&rewritten).as_deref(),
            Some(STORAGE_V0_116_0)
        );
        assert!(!rewritten.contains("fill"), "{rewritten}");
    }

    #[test]
    fn root_kind_is_main_through_round_trip() {
        let doc = Document::parse(
            "~~~card-yaml\n$quill: usaf_memo@0.1\n$kind: main\ntitle: \"Hi\"\n~~~\n",
        )
        .unwrap()
        .document;
        assert_eq!(doc.main().kind(), Some("main"));
        let restored: Document =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, restored);
        assert_eq!(restored.main().kind(), Some("main"));
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    /// The `@0.112.0` hop's whole content: a block island's line spelled
    /// `island` reads as `para` and an island's `loss` drops, so the row loads
    /// unchanged in meaning and rewrites under the new tag with its
    /// island-bearing bytes moved.
    #[test]
    fn a_0_112_0_row_migrates_its_island_line_to_para() {
        let legacy = serde_json::json!({
            "schema": "quillmark/document@0.112.0",
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                ], "nested_comments": [] },
                "body": {
                    "islands": [{
                        "id": "isl-0", "loss": "lossless", "type": "table",
                        "props": {
                            "aligns": ["none"],
                            "header": [{"marks": [], "text": "h"}],
                            "rows": [[{"marks": [], "text": "c"}]],
                        },
                    }],
                    "lines": [{"containers": [], "kind": "island"}],
                    "marks": [],
                    "text": "\u{FFFC}",
                },
            },
            "cards": [],
        })
        .to_string();

        let doc: Document = serde_json::from_str(&legacy).expect("a 0.112.0 blob still loads");
        let body = doc.main().body();
        assert_eq!(body.lines[0].kind, quillmark_content::model::LineKind::Para);
        assert_eq!(
            body.islands[0].island_type,
            quillmark_content::island::IslandType::Table
        );
        assert!(
            body.to_canonical_json().contains(r#""kind":"para""#),
            "the line re-encodes under the spelling the writer has"
        );

        let rewritten = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&rewritten).as_deref(),
            Some(STORAGE_V0_116_0)
        );
        assert!(!rewritten.contains(r#""kind":"island""#), "{rewritten}");
    }

    #[test]
    fn a_0_93_0_row_migrates_its_body_forward() {
        let legacy = serde_json::json!({
            "schema": "quillmark/document@0.93.0",
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                ], "nested_comments": [] },
                "body": {
                    "islands": [],
                    "lines": [{
                        "containers": [{"container": "list_item", "ordered": true,
                                        "ordinal": 0, "start": 3}],
                        "kind": "heading", "level": 2,
                    }],
                    "marks": [{"end": 2, "start": 0, "type": "link", "url": "u"}],
                    "text": "hi",
                },
            },
            "cards": [],
        })
        .to_string();

        let doc: Document = serde_json::from_str(&legacy).expect("a 0.93.0 blob still loads");
        let body = doc.main().body();
        assert_eq!(
            body.lines[0].kind,
            quillmark_content::model::LineKind::Heading { level: 2 }
        );
        assert_eq!(
            body.lines[0].containers[0],
            quillmark_content::model::Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 0,
                instance: 0,
            }
        );
        assert_eq!(
            body.marks[0].kind,
            quillmark_content::model::MarkKind::Link { url: "u".into() }
        );

        // Read-repair: the row rests under the current tag once written back.
        let rewritten = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&rewritten).as_deref(),
            Some(STORAGE_V0_116_0)
        );
        assert!(rewritten.contains(r#""attrs":{"level":2}"#), "{rewritten}");
    }

    /// The half no migration reaches: a `richtext` field rests as a content
    /// object inside an opaque payload value, under no schema tag of its own, so
    /// the decoder's own tolerance is what reads it.
    #[test]
    fn a_legacy_content_field_reads_without_a_migration() {
        let stored = serde_json::json!({
            "schema": "quillmark/document@0.112.0",
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                    { "type": "field", "key": "intro", "fill": false, "value": {
                        "islands": [],
                        "lines": [{"containers": [], "kind": "code", "lang": "rust"}],
                        "marks": [{"end": 2, "start": 0, "type": "anchor", "id": "a1"}],
                        "text": "hi",
                    }},
                ], "nested_comments": [] },
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}],
                         "marks": [], "text": ""},
            },
            "cards": [],
        })
        .to_string();

        let doc: Document = serde_json::from_str(&stored).unwrap();
        let value = doc.main().payload().get("intro").unwrap().as_json().clone();
        let content = Codec::Richtext
            .decode_field(&value)
            .expect("the legacy spelling still decodes");
        assert_eq!(
            content.lines[0].kind,
            quillmark_content::model::LineKind::Code {
                lang: Some("rust".into())
            }
        );
        assert_eq!(
            content.marks[0].kind,
            quillmark_content::model::MarkKind::Anchor { id: "a1".into() }
        );
    }

    #[test]
    fn peek_storage_version_reads_field_without_full_parse() {
        let doc = sample();
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_storage_version(&json).as_deref(), Some(STORAGE_V0_116_0));

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
        assert!(restored
            .to_markdown()
            .contains("$kind: main # required for root"));
    }

    #[test]
    fn v0_82_0_payload_loads_via_migration() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "usaf_memo@0.1"},
                    {"type": "kind", "value": "main"},
                    {"type": "field", "key": "title", "value": "Hello"}
                ]},
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
        let reser = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&reser).as_deref(),
            Some(STORAGE_V0_116_0)
        );
    }

    #[test]
    fn v0_82_0_ext_item_loads() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "ext", "value": {"editor": {"pinned": true}}}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert!(doc.main().ext().is_some());
    }

    #[test]
    fn v0_82_0_id_item_is_dropped_not_rejected() {
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "id", "value": "card-7"},
                    {"type": "field", "key": "title", "value": "Hello"}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
        let md = doc.to_markdown();
        assert!(!md.contains("$id"), "{md}");
        assert!(!md.contains("card-7"), "{md}");
        assert_eq!(doc, Document::parse(&md).unwrap().document);
    }

    #[test]
    fn v0_82_0_seed_item_is_rejected() {
        // `$seed` is what the `@0.92.0` bump added: a blob claiming the older
        // tag cannot legitimately carry one.
        let json = r#"{
            "schema": "quillmark/document@0.82.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "X"}}}
                ]},
                "body": ""
            },
            "cards": []
        }"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }

    #[test]
    fn v0_81_0_payload_loads_via_migration() {
        let json = r#"{
            "schema": "quillmark/document@0.81.0",
            "main": {
                "sentinel": {"kind": "main", "quill": "usaf_memo@0.1"},
                "frontmatter": {
                    "items": [{"kind": "field", "key": "title", "value": "Hello"}]
                },
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.main().kind(), Some("main"));
        assert_eq!(doc.quill_reference().to_string(), "usaf_memo@0.1");
        assert_eq!(
            doc.main().payload().get("title").unwrap().as_str(),
            Some("Hello")
        );
        let reser = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&reser).as_deref(),
            Some(STORAGE_V0_116_0)
        );
    }

    #[test]
    fn v0_81_0_with_composable_card_migrates() {
        let json = r#"{
            "schema": "quillmark/document@0.81.0",
            "main": {
                "sentinel": {"kind": "main", "quill": "q@1.0"},
                "frontmatter": {"items": []},
                "body": ""
            },
            "cards": [
                {
                    "sentinel": {"kind": "card", "tag": "indorsement"},
                    "frontmatter": {"items": [{"kind": "field", "key": "for", "value": "X"}]},
                    "body": "C body"
                }
            ]
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.cards().len(), 1);
        assert_eq!(doc.cards()[0].kind(), Some("indorsement"));
        assert_eq!(
            doc.cards()[0].payload().get("for").unwrap().as_str(),
            Some("X")
        );
    }

    #[test]
    fn v0_81_0_comments_survive_and_a_placeholder_drops() {
        // The two the sentinel split could drop: comment order against the `$`
        // prelude, and nested-comment paths, whose absolute payload-level form
        // V0_81_0 and V0_92_0 share.
        let json = r#"{
            "schema": "quillmark/document@0.81.0",
            "main": {
                "sentinel": {"kind": "main", "quill": "q@1.0"},
                "frontmatter": {
                    "items": [
                        {"kind": "comment", "text": "a top-level comment"},
                        {"kind": "field", "key": "subject", "value": "S", "fill": true},
                        {"kind": "field", "key": "memo_for", "value": ["ORG/SYMBOL"]}
                    ],
                    "nested_comments": [
                        {
                            "container_path": [{"Key": "memo_for"}],
                            "position": 0,
                            "text": "inline note",
                            "inline": true
                        }
                    ]
                },
                "body": "Body."
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        let md = doc.to_markdown();
        assert!(md.contains("# a top-level comment"), "{md}");
        assert_eq!(doc.main().payload().get("subject").unwrap().as_json(), &serde_json::Value::Null);
        assert!(md.contains("# inline note"), "{md}");

        // The migration invents nothing the parser would not.
        let reparsed = Document::parse(&md).unwrap().document;
        assert_eq!(doc, reparsed);
    }

    /// A field's content is untagged, so no hop respells a stored `loss` or
    /// `island` line kind there: the decoder reads both.
    #[test]
    fn a_retired_content_spelling_in_a_field_loads() {
        let blob = |value: &str| {
            format!(
                r#"{{
                "schema": "quillmark/document@0.112.0",
                "main": {{
                    "payload": {{"items": [
                        {{"type": "quill", "value": "q@1.0"}},
                        {{"type": "kind", "value": "main"}},
                        {{"type": "field", "key": "x", "value": {value}}}
                    ]}},
                    "body": {{"islands": [], "lines": [{{"containers": [], "kind": "para"}}], "marks": [], "text": ""}}
                }},
                "cards": []
            }}"#
            )
        };

        let content = quillmark_content::import::from_markdown("see ![a](u.png)\n\n| h |\n|---|\n| c |")
            .expect("content");
        let canonical =
            serde_json::to_string(&quillmark_content::serial::to_canonical_value(&content))
                .expect("canonical content serializes");
        let retired = canonical
            .replace(r#""id":"isl-0","#, r#""id":"isl-0","loss":"lossless","#)
            .replace(r#""id":"isl-1","#, r#""id":"isl-1","loss":"degraded","#)
            .replacen(r#""kind":"para"}]"#, r#""kind":"island"}]"#, 1);
        assert_eq!(retired.matches(r#""loss":"#).count(), 2, "{retired}");
        assert!(retired.contains(r#""kind":"island""#), "{retired}");
        let current = serde_json::from_str::<Document>(&blob(&canonical))
            .expect("the current spelling loads");
        let respelled = serde_json::from_str::<Document>(&blob(&retired))
            .expect("the retired spelling loads");
        assert_eq!(respelled.to_markdown(), current.to_markdown());
    }

    #[test]
    fn rejects_main_card_without_quill() {
        let json = r#"{
            "schema": "quillmark/document@0.112.0",
            "main": {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}},
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("$quill"));
    }

    #[test]
    fn rejects_composable_card_tagged_main() {
        let json = r#"{
            "schema": "quillmark/document@0.112.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}
            },
            "cards": [
                {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("reserved (root only)"));
    }

    /// A stored card with no `$kind` folds into the body above it: its fields
    /// and comments as a code block, then its body, whose marks shift with it.
    #[test]
    fn a_stored_kindless_card_folds_into_the_body_above() {
        let parsed = Document::parse(
            "~~~\n$quill: q@1.0\n~~~\n\nIntro. ![a](a.png)\n\n~~~\n$kind: note\n~~~\n\nNote.\n",
        )
        .unwrap()
        .document;
        let mut ext = serde_json::Map::new();
        ext.insert("editor".into(), serde_json::json!({"title": "Secret draft name"}));
        let kindless = Payload::from_items(vec![
            PayloadItem::Meta { key: MetaKey::Ext, value: ext },
            PayloadItem::comment("note"),
            PayloadItem::Field {
                key: "name".to_string(),
                value: QuillValue::from_json(serde_json::json!("server")),
            },
        ]);
        let body = super::super::import_body("Conclusion. ![c](c.png)").unwrap();
        let kindless = Card::from_parts(kindless, body);
        let stored = Document {
            main: parsed.main().clone(),
            cards: vec![kindless, parsed.cards()[0].clone()],
        };

        let mut json: serde_json::Value = serde_json::to_value(&stored).unwrap();
        let anchor = serde_json::json!([{"type": "anchor", "id": "c1", "start": 0, "end": 5}]);
        json["main"]["body"]["marks"] = anchor.clone();
        json["cards"][1]["body"]["marks"] = anchor;
        json["cards"][0]["body"]["marks"] = serde_json::json!([
            {"type": "anchor", "id": "c1", "start": 0, "end": 10},
            {"type": "anchor", "id": "c2", "start": 0, "end": 10}
        ]);
        json["main"]["body"]["islands"][0]["id"] = "isl-1".into();
        json["cards"][0]["body"]["islands"][0]["id"] = "isl-1".into();
        let restored: Document = serde_json::from_value(json).unwrap();
        assert_eq!(restored.cards().len(), 1);
        assert_eq!(restored.cards()[0].kind(), Some("note"));
        assert_eq!(
            restored.main().body_markdown(),
            "Intro. ![a](a.png)\n\n```\n# note\nname: server\n```\n\nConclusion. ![c](c.png)"
        );
        let main = restored.main().body();
        let islands: Vec<&str> = main.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(islands, ["isl-1", "isl-2"]);
        let anchors: Vec<(String, String)> = main
            .marks
            .iter()
            .map(|m| match &m.kind {
                MarkKind::Anchor { id } => (
                    id.clone(),
                    main.text.chars().skip(m.start).take(m.end - m.start).collect(),
                ),
                other => panic!("{other:?}"),
            })
            .collect();
        let expected = [("c1", "Intro"), ("c2", "Conclusion")];
        assert_eq!(anchors, expected.map(|(i, t)| (i.to_string(), t.to_string())));
        assert_eq!(restored.cards()[0].body().marks.len(), 1);
    }

    /// A fold loads whatever the stored card holds, keeps the blocks on either
    /// side of it apart, and emits markdown that parses back to it.
    #[test]
    fn a_stored_kindless_fold_is_valid_and_a_fixed_point() {
        let fold = |main_md: &str, fields: serde_json::Value, body_md: &str| {
            let main = Document::parse(&format!("~~~\n$quill: q@1.0\n~~~\n\n{main_md}\n"))
                .unwrap()
                .document
                .main()
                .clone();
            let items = fields
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| PayloadItem::Field {
                    key: key.clone(),
                    value: QuillValue::from_json(value.clone()),
                })
                .collect();
            let card = Card::from_parts(
                Payload::from_items(items),
                super::super::import_body(body_md).unwrap(),
            );
            let json = serde_json::to_value(&Document { main, cards: vec![card] }).unwrap();
            let restored: Document = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            let again = Document::parse(&restored.to_markdown()).unwrap().document;
            assert_eq!(again, restored);
            restored.main().body().clone()
        };

        let body = fold("Intro.", serde_json::json!({ "name": "a\u{202D}b\u{2028}c\u{FFFC}d" }), "");
        assert_eq!(body.text, "Intro.\nname: \"ab\\Lcd\"", "admitted as a body import admits it");

        for (above, below) in [("> a", "> b"), ("1. a", "1. b"), ("- a", "- b")] {
            let body = fold(above, serde_json::json!({}), below);
            let top = |line: &quillmark_content::model::Line| line.containers.first().cloned();
            assert_ne!(top(&body.lines[0]), top(body.lines.last().unwrap()), "{above:?}: {body:?}");
        }
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let json = r#"{
            "schema": "quillmark/document@0.112.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "not a valid ref!!"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}
            },
            "cards": []
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err.to_string().contains("invalid quill reference"));
    }

    #[test]
    fn rejects_composable_card_with_seed() {
        let json = r#"{
            "schema": "quillmark/document@0.112.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}
            },
            "cards": [
                {"payload": {"items": [
                    {"type": "kind", "value": "indorsement"},
                    {"type": "seed", "value": {"note": {"from": "X"}}}
                ]}, "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}}
            ]
        }"#;
        let err = serde_json::from_str::<Document>(json).unwrap_err();
        assert!(err
            .to_string()
            .contains("composable cards must not carry a $seed entry"));
    }

    #[test]
    fn v0_92_0_seed_item_round_trips() {
        let json = r#"{
            "schema": "quillmark/document@0.92.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "49 FW/CC"}}}
                ]},
                "body": ""
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
        let rt = doc.main().body().clone();
        assert!(
            !rt.marks.is_empty(),
            "test needs a non-trivial content (marks present)"
        );
        let expected = rt.to_canonical_json();
        let envelope = serde_json::to_string(&doc).unwrap();
        let body = locate_body_subtree(&envelope);
        assert_eq!(
            body, expected,
            "the envelope body subtree must equal to_canonical_json byte-for-byte"
        );
        assert!(body.starts_with("{\"islands\":"));
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
            "V0_116_0 serialize→deserialize is a byte-fixed point"
        );
        assert_eq!(peek_storage_version(&first).as_deref(), Some(STORAGE_V0_116_0));
    }

    #[test]
    fn legacy_table_body_migrates_deterministically_with_islands() {
        // Import is a pure function, so the same legacy row migrates to
        // byte-identical storage.
        let blob = r#"{
            "schema": "quillmark/document@0.92.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@0.1"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": "| A | B |\n| - | - |\n| 1 | 2 |\n"
            },
            "cards": []
        }"#;
        let doc: Document = serde_json::from_str(blob).unwrap();
        let body = doc.main().body();
        assert_eq!(body.islands.len(), 1, "table imports as one island");
        assert_eq!(body.islands[0].id, "isl-0", "sequential island id");
        assert_eq!(
            body.islands[0].island_type,
            quillmark_content::island::IslandType::Table
        );
        // Each table cell is inline `{text, marks}`, not a raw markdown slice.
        let key = body.to_canonical_json();
        assert_eq!(
            key,
            "{\"islands\":[{\"id\":\"isl-0\",\"props\":{\
             \"aligns\":[\"none\",\"none\"],\
             \"header\":[{\"marks\":[],\"text\":\"A\"},{\"marks\":[],\"text\":\"B\"}],\
             \"rows\":[[{\"marks\":[],\"text\":\"1\"},{\"marks\":[],\"text\":\"2\"}]]},\
             \"type\":\"table\"}],\
             \"lines\":[{\"containers\":[],\"kind\":\"para\"}],\
             \"marks\":[],\"text\":\"\u{FFFC}\"}",
            "cells are structured text+marks"
        );

        let again: Document = serde_json::from_str(blob).unwrap();
        assert_eq!(
            serde_json::to_string(&doc).unwrap(),
            serde_json::to_string(&again).unwrap(),
            "same legacy input → same migrated bytes"
        );
        let reser = serde_json::to_string(&doc).unwrap();
        assert_eq!(peek_storage_version(&reser).as_deref(), Some(STORAGE_V0_116_0));
    }

    #[test]
    fn over_nested_legacy_body_is_malformed() {
        // An over-nested legacy body never rendered; the 92→116 hop maps
        // `NestingTooDeep` to `Malformed` rather than dropping structure.
        let deep = ">".repeat(crate::error::MAX_NESTING_DEPTH + 5);
        let card = CardV0_92_0 {
            payload: PayloadV0_92_0::default(),
            body: format!("{deep} too deep"),
        };
        let err = CardV0_116_0::try_from(card).unwrap_err();
        assert!(matches!(err, StorageError::Malformed(_)), "got: {err:?}");
        assert!(err.to_string().contains("card body"));
    }

    /// Every tag refuses it, by two machineries: `CanonicalContent`'s
    /// parse-time check under the current one, the hop's own decode under the
    /// older ones, whose frozen trees carry the body raw.
    #[test]
    fn deserialize_rejects_invalid_content_body() {
        for schema in [STORAGE_V0_116_0, STORAGE_V0_115_0, STORAGE_V0_112_0, STORAGE_V0_93_0] {
            let blob = format!(
                r#"{{
                "schema": "{schema}",
                "main": {{
                    "payload": {{"items": [
                        {{"type": "quill", "value": "q@0.1"}},
                        {{"type": "kind", "value": "main"}}
                    ]}},
                    "body": {{"text": "a\nb", "lines": [{{"kind": "para", "containers": []}}], "marks": [], "islands": []}}
                }},
                "cards": []
            }}"#
            );
            assert!(
                serde_json::from_str::<Document>(&blob).is_err(),
                "accepted a one-line `lines` over two lines of text under {schema}"
            );
        }
    }
}
