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

use quillmark_content::lines::import::ImportError;
use quillmark_content::lines::model::{Container, Content, MarkKind, Normalized};
use quillmark_content::model::Content as ContentModel;

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
pub const STORAGE_V0_135_0: &str = "quillmark/document@0.135.0";

/// The tag before [`STORAGE_V0_135_0`], read: the line-form body, each
/// composable card's payload holding an `id` item.
pub const STORAGE_V0_134_0: &str = "quillmark/document@0.134.0";

/// The tag before [`STORAGE_V0_134_0`], read.
pub const STORAGE_V0_131_0: &str = "quillmark/document@0.131.0";

/// The tag before [`STORAGE_V0_131_0`], read.
pub const STORAGE_V0_129_0: &str = "quillmark/document@0.129.0";

/// The tag before [`STORAGE_V0_129_0`], still read.
pub const STORAGE_V0_124_0: &str = "quillmark/document@0.124.0";

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
/// variant, leaving existing ones byte-stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema")]
pub enum StoredDocument {
    #[serde(rename = "quillmark/document@0.135.0")]
    V0_135_0(DocumentV0_135_0),
    #[serde(rename = "quillmark/document@0.134.0")]
    V0_134_0(DocumentV0_134_0),
    #[serde(rename = "quillmark/document@0.131.0")]
    V0_131_0(DocumentV0_131_0),
    #[serde(rename = "quillmark/document@0.129.0")]
    V0_129_0(DocumentV0_129_0),
    #[serde(rename = "quillmark/document@0.124.0")]
    V0_124_0(DocumentV0_124_0),
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

/// Frozen `0.135.0` representation of a [`Document`]: the V0_129_0 tree, each
/// body a content-model value (see [`StoredContent`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentV0_135_0 {
    pub main: CardV0_135_0,
    #[serde(default)]
    pub cards: Vec<CardV0_135_0>,
}

/// Frozen `0.135.0` representation of a [`Card`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardV0_135_0 {
    pub payload: PayloadV0_129_0,
    pub body: StoredContent,
}

/// A card body embedded as a content-model value, through its canonical
/// JSON ([`quillmark_content::wire`]): `Serialize` validates, then writes the
/// key-sorted object; `Deserialize` decodes, normalizes and validates, so an
/// invalid body fails the load. Both directions check, as [`CanonicalContent`]
/// does.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContent(pub ContentModel);

impl Serialize for StoredContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .validate()
            .map_err(|inv| serde::ser::Error::custom(format!("content invariant: {inv}")))?;
        self.0.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StoredContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        ContentModel::from_value(&value)
            .map(StoredContent)
            .map_err(serde::de::Error::custom)
    }
}

/// Frozen `0.134.0` representation of a [`Document`]: the V0_129_0 tree, each
/// composable card's payload holding an `id` item.
pub type DocumentV0_134_0 = DocumentV0_129_0;

/// Frozen `0.131.0` representation of a [`Document`]: the V0_129_0 tree over
/// the content vocabulary holding the `font_family` mark, a table's `head` and
/// a cell's `colspan` and `rowspan`.
pub type DocumentV0_131_0 = DocumentV0_129_0;

/// Frozen `0.129.0` representation of a [`Document`]: the V0_124_0 tree over
/// the content vocabulary holding the `footnote` and `math` islands, the
/// `term` line kind and the `description` container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentV0_129_0 {
    pub main: CardV0_129_0,
    #[serde(default)]
    pub cards: Vec<CardV0_129_0>,
}

/// Frozen `0.129.0` representation of a [`Card`]. The `body` is the canonical
/// content embedded structurally (see [`CanonicalContent`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardV0_129_0 {
    pub payload: PayloadV0_129_0,
    pub body: CanonicalContent,
}

/// The V0_129_0 payload shape: identical to V0_124_0.
pub type PayloadV0_129_0 = PayloadV0_124_0;

/// Frozen `0.124.0` representation of a [`Document`], over the content
/// vocabulary without the `footnote` and `math` islands, the `term`
/// line kind and the `description` container.
///
/// Read-only, and its `body` is raw JSON rather than [`CanonicalContent`]. The
/// live type tracks the live crate, so embedding it here would make this tree
/// *write* the current spelling under the `@0.124.0` tag — the one thing a
/// frozen tree must not do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentV0_124_0 {
    pub main: CardV0_124_0,
    #[serde(default)]
    pub cards: Vec<CardV0_124_0>,
}

/// Frozen `0.124.0` representation of a [`Card`]. See [`DocumentV0_124_0`] for
/// why `body` is raw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardV0_124_0 {
    pub payload: PayloadV0_124_0,
    pub body: serde_json::Value,
}

/// Frozen `0.124.0` representation of a [`Payload`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadV0_124_0 {
    #[serde(default)]
    pub items: Vec<PayloadItemV0_124_0>,
    #[serde(default)]
    pub nested_comments: Vec<NestedCommentV0_124_0>,
}

/// Frozen `0.124.0` representation of a unified payload item. An item carrying
/// a key its variant does not name fails the load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum PayloadItemV0_124_0 {
    /// `$quill` system metadata: the quill reference string.
    Quill { value: String },
    /// `$kind` system metadata.
    Kind { value: String },
    /// `$id` system metadata: a composable card's id. Written under
    /// [`STORAGE_V0_134_0`].
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

/// A card body embedded as the **canonical content**. Its serde delegates to the
/// frozen canonical serializer (`quillmark_content::lines::serial`) rather than a
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
        quillmark_content::lines::serial::to_canonical_value(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CanonicalContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let rt = quillmark_content::lines::serial::from_canonical_value(&value)
            .map_err(serde::de::Error::custom)?;
        Ok(CanonicalContent(rt))
    }
}

/// Frozen `0.124.0` representation of a [`NestedComment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedCommentV0_124_0 {
    pub container_path: Vec<CommentPathSegmentV0_124_0>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Frozen `0.124.0` representation of a [`PathSegment`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommentPathSegmentV0_124_0 {
    Key(String),
    Index(usize),
}

impl From<Document> for StoredDocument {
    fn from(doc: Document) -> Self {
        StoredDocument::V0_135_0(DocumentV0_135_0::from(&doc))
    }
}

impl From<&Document> for DocumentV0_135_0 {
    fn from(doc: &Document) -> Self {
        DocumentV0_135_0 {
            main: CardV0_135_0::from(doc.main()),
            cards: doc.cards().iter().map(CardV0_135_0::from).collect(),
        }
    }
}

impl From<&Card> for CardV0_135_0 {
    fn from(card: &Card) -> Self {
        CardV0_135_0 {
            payload: PayloadV0_129_0::from(card.payload()),
            body: StoredContent(card.body().clone()),
        }
    }
}

/// The hop from the line form: each body, and each field value that decodes
/// as line-form content at any depth, migrates, minting its ids.
impl From<DocumentV0_129_0> for DocumentV0_135_0 {
    fn from(d: DocumentV0_129_0) -> Self {
        let card = |mut c: CardV0_129_0| {
            for item in &mut c.payload.items {
                if let PayloadItemV0_124_0::Field { value, .. } = item {
                    migrate_field(value);
                }
            }
            CardV0_135_0 {
                payload: c.payload,
                body: StoredContent(quillmark_content::migrate::from_lines(&c.body.0)),
            }
        };
        DocumentV0_135_0 { main: card(d.main), cards: d.cards.into_iter().map(card).collect() }
    }
}

/// A field's line-form content, at any depth of its value, as the model.
fn migrate_field(value: &mut serde_json::Value) {
    if quillmark_content::wire::is_v1(value)
        && let Some(lines) = super::emit::canonical_content(value)
    {
        *value = quillmark_content::migrate::from_lines(&lines).to_value();
        return;
    }
    match value {
        serde_json::Value::Object(map) => map.values_mut().for_each(migrate_field),
        serde_json::Value::Array(items) => items.iter_mut().for_each(migrate_field),
        _ => {}
    }
}

impl From<&Payload> for PayloadV0_124_0 {
    fn from(payload: &Payload) -> Self {
        PayloadV0_124_0 {
            items: payload
                .items()
                .iter()
                .map(PayloadItemV0_124_0::from)
                .collect(),
            nested_comments: payload
                .nested_comments()
                .iter()
                .map(NestedCommentV0_124_0::from)
                .collect(),
        }
    }
}

impl From<&PayloadItem> for PayloadItemV0_124_0 {
    fn from(item: &PayloadItem) -> Self {
        match item {
            PayloadItem::Quill { reference } => PayloadItemV0_124_0::Quill {
                value: reference.to_string(),
            },
            PayloadItem::Kind { value } => PayloadItemV0_124_0::Kind {
                value: value.clone(),
            },
            PayloadItem::Id { value } => PayloadItemV0_124_0::Id {
                value: value.clone(),
            },
            // The DTO keeps `$ext` / `$seed` as explicit variants, so the live
            // model's unified `Meta` splits back out by key. Neither carries
            // `nested_comments`: those live in the payload-level sidecar.
            PayloadItem::Meta {
                key: MetaKey::Ext,
                value,
            } => PayloadItemV0_124_0::Ext {
                value: value.clone(),
            },
            PayloadItem::Meta {
                key: MetaKey::Seed,
                value,
            } => PayloadItemV0_124_0::Seed {
                value: value.clone(),
            },
            PayloadItem::Field { key, value } => PayloadItemV0_124_0::Field {
                key: key.clone(),
                value: value.as_json().clone(),
            },
            PayloadItem::Comment { text, inline } => PayloadItemV0_124_0::Comment {
                text: text.clone(),
                inline: *inline,
            },
        }
    }
}

impl From<&NestedComment> for NestedCommentV0_124_0 {
    fn from(nc: &NestedComment) -> Self {
        NestedCommentV0_124_0 {
            container_path: nc
                .container_path
                .iter()
                .map(CommentPathSegmentV0_124_0::from)
                .collect(),
            position: nc.position,
            text: nc.text.clone(),
            inline: nc.inline,
        }
    }
}

impl From<&PathSegment> for CommentPathSegmentV0_124_0 {
    fn from(seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => CommentPathSegmentV0_124_0::Key(k.clone()),
            PathSegment::Index(i) => CommentPathSegmentV0_124_0::Index(*i),
        }
    }
}

impl TryFrom<StoredDocument> for Document {
    type Error = StorageError;

    fn try_from(stored: StoredDocument) -> Result<Self, Self::Error> {
        match stored {
            StoredDocument::V0_135_0(payload) => Document::try_from(payload),
            StoredDocument::V0_134_0(payload)
            | StoredDocument::V0_131_0(payload)
            | StoredDocument::V0_129_0(payload) => Document::try_from(DocumentV0_135_0::from(payload)),
            StoredDocument::V0_124_0(payload) => {
                Document::try_from(DocumentV0_135_0::from(DocumentV0_129_0::try_from(payload)?))
            }
        }
    }
}

impl TryFrom<DocumentV0_135_0> for Document {
    type Error = StorageError;

    fn try_from(payload: DocumentV0_135_0) -> Result<Self, Self::Error> {
        let mut main = Card::try_from(payload.main)?;
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
        let cards = payload
            .cards
            .into_iter()
            .map(Card::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let mut kindless = false;
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
        let doc = if kindless {
            fold_kindless(main, cards)?
        } else {
            Document::from_main_and_cards(main, cards)
        };
        within_card_ceiling(doc)
    }
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
        let body = append_block(target.body_lines().clone().into_content(), code);
        let body = append_block(body, card.body_lines().clone().into_content()).into_normalized();
        body.validate().map_err(|e| malformed(&e))?;
        target.set_body_lines(body);
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
    let taken: HashSet<String> = dst
        .islands
        .iter()
        .chain(&src.islands)
        .map(|i| i.id.clone())
        .collect();
    // `u128` counts past `isl-{u64::MAX}`; `taken` steps over an id past that,
    // which the count does not parse.
    let mut next = dst
        .islands
        .iter()
        .chain(&src.islands)
        .filter_map(|i| i.id.strip_prefix("isl-")?.parse::<u64>().ok())
        .max()
        .map_or(0, |n| u128::from(n) + 1);
    for island in &mut src.islands {
        if held.contains(island.id.as_str()) {
            while taken.contains(&format!("isl-{next}")) {
                next += 1;
            }
            island.id = format!("isl-{next}");
            next += 1;
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
        .filter_map(|l| l.containers.first().map(Container::instance))
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

impl TryFrom<CardV0_135_0> for Card {
    type Error = StorageError;

    fn try_from(card: CardV0_135_0) -> Result<Self, Self::Error> {
        let payload = Payload::try_from(card.payload)?;
        validate_dto_payload(&payload)?;
        // `StoredContent`'s Deserialize already normalized and validated it.
        Ok(Card::from_parts(payload, card.body.0))
    }
}

// The hop into V0_129 decodes the raw `body` the V0_124 tree defers, which is
// also where an invalid legacy body is caught, `CanonicalContent`'s parse-time
// check not being available to a raw field.

impl TryFrom<DocumentV0_124_0> for DocumentV0_129_0 {
    type Error = StorageError;

    fn try_from(d: DocumentV0_124_0) -> Result<Self, Self::Error> {
        Ok(DocumentV0_129_0 {
            main: CardV0_129_0::try_from(d.main)?,
            cards: d
                .cards
                .into_iter()
                .map(CardV0_129_0::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<CardV0_124_0> for CardV0_129_0 {
    type Error = StorageError;

    fn try_from(card: CardV0_124_0) -> Result<Self, Self::Error> {
        let body = quillmark_content::lines::serial::from_canonical_value(&card.body)
            .map_err(|e| StorageError::Malformed(format!("card body: {e}")))?;
        Ok(CardV0_129_0 {
            payload: card.payload,
            body: CanonicalContent(body),
        })
    }
}

impl TryFrom<PayloadV0_124_0> for Payload {
    type Error = StorageError;

    fn try_from(p: PayloadV0_124_0) -> Result<Self, Self::Error> {
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

impl TryFrom<PayloadItemV0_124_0> for PayloadItem {
    type Error = StorageError;

    fn try_from(item: PayloadItemV0_124_0) -> Result<Self, Self::Error> {
        Ok(match item {
            PayloadItemV0_124_0::Quill { value } => {
                let reference = QuillReference::from_str(&value).map_err(|reason| {
                    StorageError::InvalidQuillReference {
                        value: value.clone(),
                        reason,
                    }
                })?;
                PayloadItem::Quill { reference }
            }
            PayloadItemV0_124_0::Kind { value } => PayloadItem::Kind { value },
            PayloadItemV0_124_0::Id { value } => PayloadItem::Id { value },
            PayloadItemV0_124_0::Ext { value } => PayloadItem::Meta {
                key: MetaKey::Ext,
                value: depth_check_meta_map(value, "$ext")?,
            },
            PayloadItemV0_124_0::Seed { value } => PayloadItem::Meta {
                key: MetaKey::Seed,
                value: depth_check_meta_map(value, "$seed")?,
            },
            PayloadItemV0_124_0::Field { key, value } => {
                super::edit::validate_field(&key, &value)
                    .map_err(|v| StorageError::Malformed(v.message(&key)))?;
                PayloadItem::Field {
                    key,
                    value: QuillValue::from_json(value),
                }
            }
            PayloadItemV0_124_0::Comment { text, inline } => PayloadItem::Comment {
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

impl From<NestedCommentV0_124_0> for NestedComment {
    fn from(nc: NestedCommentV0_124_0) -> Self {
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

impl From<CommentPathSegmentV0_124_0> for PathSegment {
    fn from(seg: CommentPathSegmentV0_124_0) -> Self {
        match seg {
            CommentPathSegmentV0_124_0::Key(k) => PathSegment::Key(k),
            CommentPathSegmentV0_124_0::Index(i) => PathSegment::Index(i),
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

    /// `overwrite_body` takes a caller's content on the canonical-form token
    /// alone, so an invalid shape reaches the DTO.
    #[test]
    fn an_unreadable_body_is_refused_on_write() {
        use quillmark_content::lines::model::{Container, Content, Line, LineKind};

        let mut doc = sample();
        let mut line = Line::new(LineKind::Para);
        line.containers =
            vec![Container::Quote { instance: 0 }; quillmark_content::MAX_NESTING_DEPTH + 1];
        doc.main_mut()
            .overwrite_body(Content::new("x".to_string(), vec![line]));
        let err = serde_json::to_string(&doc).expect_err("write refuses what load would reject");
        assert!(err.to_string().contains("nests 13 deep"), "{err}");
    }

    #[test]
    fn content_field_survives_storage_round_trip_losslessly() {
        // The storage DTO is the lossless carrier: an `underline` with no
        // markdown form survives here where a markdown save would drop it.
        use quillmark_content::lines::model::{Mark, MarkKind};

        let mut doc = sample();
        let mut content = quillmark_content::lines::import::from_markdown("underlined intro")
            .unwrap()
            .content
            .into_content();
        content.marks.push(Mark::new(0, 10, MarkKind::Underline));
        let content = content.into_normalized();
        let json = quillmark_content::lines::serial::to_canonical_value(&content);
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

    fn row_0_124_0(
        item: serde_json::Value,
        nested: serde_json::Value,
    ) -> serde_json::Map<String, serde_json::Value> {
        row(STORAGE_V0_124_0, item, nested)
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
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}],
                         "marks": [], "text": ""},
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
        for schema in [STORAGE_V0_129_0, STORAGE_V0_124_0] {
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
            doc.push_card(Card::new("note").unwrap()).unwrap();
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
        let row = row_0_124_0(
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

    /// The `0.124.0` hop decodes its raw body with the live decoder, which
    /// loads an element as the lines it held.
    #[test]
    fn a_0_124_0_row_reads_through_the_body_decode() {
        let mut row = row_0_124_0(
            serde_json::json!({ "type": "field", "key": "a", "value": [1] }),
            serde_json::json!([]),
        );
        row["main"]["body"] = serde_json::json!({
            "islands": [],
            "lines": [{"containers": [{"attrs": {"$name": "keep"}, "container": "element"}], "kind": "para"}],
            "marks": [],
            "text": "a",
        });
        let doc: Document = serde_json::from_value(row.into()).unwrap();
        assert!(doc.main().body_lines().lines[0].containers.is_empty());
        let rewritten = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            peek_storage_version(&rewritten).as_deref(),
            Some(STORAGE_V0_135_0)
        );
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

    #[test]
    fn rejects_unknown_schema_version() {
        let json = r#"{"schema":"quillmark/document@0.99.0","main":{}}"#;
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("unknown variant `quillmark/document@0.99.0`"),
            "{err}"
        );
    }

    /// The half no migration reaches: a `richtext` field rests as a content
    /// object inside an opaque payload value, under no schema tag of its own, so
    /// the decoder's own tolerance is what reads it.
    #[test]
    fn a_legacy_content_field_reads_without_a_migration() {
        let stored = serde_json::json!({
            "schema": "quillmark/document@0.129.0",
            "main": {
                "payload": { "items": [
                    { "type": "quill", "value": "q@1.0" },
                    { "type": "kind", "value": "main" },
                    { "type": "field", "key": "intro", "value": {
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
            quillmark_content::lines::model::LineKind::Code {
                lang: Some("rust".into())
            }
        );
        assert_eq!(
            content.marks[0].kind,
            quillmark_content::lines::model::MarkKind::Anchor { id: "a1".into() }
        );
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

    /// A field's content is untagged, so no hop respells a stored `loss`, an
    /// `island` or `term` line kind, or an `element` container there: the
    /// decoder reads each.
    #[test]
    fn a_retired_content_spelling_in_a_field_loads() {
        let blob = |value: &str| {
            format!(
                r#"{{
                "schema": "quillmark/document@0.129.0",
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

        let content =
            quillmark_content::lines::import::from_markdown("see ![a](u.png)\n\n| h |\n|---|\n| c |")
                .expect("content")
                .content;
        let canonical =
            serde_json::to_string(&quillmark_content::lines::serial::to_canonical_value(&content))
                .expect("canonical content serializes");
        let retired = canonical
            .replace(r#""id":"isl-0","#, r#""id":"isl-0","loss":"lossless","#)
            .replace(r#""id":"isl-1","#, r#""id":"isl-1","loss":"degraded","#)
            .replacen(r#""kind":"para"}]"#, r#""kind":"island"}]"#, 1)
            .replacen(
                r#"{"containers":[],"kind":"para"}"#,
                r#"{"containers":[{"attrs":{"$name":"keep"},"container":"element"}],"kind":"term"}"#,
                1,
            );
        assert_eq!(retired.matches(r#""loss":"#).count(), 2, "{retired}");
        assert!(retired.contains(r#""kind":"island""#), "{retired}");
        assert!(retired.contains(r#""kind":"term""#), "{retired}");
        let current = serde_json::from_str::<Document>(&blob(&canonical))
            .expect("the current spelling loads");
        let respelled =
            serde_json::from_str::<Document>(&blob(&retired)).expect("the retired spelling loads");
        assert_eq!(respelled.to_markdown(), current.to_markdown());
    }

    #[test]
    fn rejects_main_card_without_quill() {
        let json = r#"{
            "schema": "quillmark/document@0.129.0",
            "main": {"payload": {"items": [{"type": "kind", "value": "main"}]}, "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}},
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
            "schema": "quillmark/document@0.129.0",
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
        let err = serde_json::from_str::<Document>(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("reserved (root only)"), "{err}");
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
        ext.insert(
            "editor".into(),
            serde_json::json!({"title": "Secret draft name"}),
        );
        let kindless = Payload::from_items(vec![
            PayloadItem::Meta {
                key: MetaKey::Ext,
                value: ext,
            },
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

        let mut json = v1_row(&stored, STORAGE_V0_134_0);
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
        let main = restored.main().body_lines();
        let islands: Vec<&str> = main.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(islands, ["isl-1", "isl-2"]);
        let anchors: Vec<(String, String)> = main
            .marks
            .iter()
            .map(|m| match &m.kind {
                MarkKind::Anchor { id } => (
                    id.clone(),
                    main.text
                        .chars()
                        .skip(m.start)
                        .take(m.end - m.start)
                        .collect(),
                ),
                other => panic!("{other:?}"),
            })
            .collect();
        let expected = [("c1", "Intro"), ("c2", "Conclusion")];
        assert_eq!(
            anchors,
            expected.map(|(i, t)| (i.to_string(), t.to_string()))
        );
        assert_eq!(restored.cards()[0].body_lines().marks.len(), 1);
    }

    /// The id past `isl-{u64::MAX}` is `isl-{u64::MAX + 1}`, and one either body
    /// holds is stepped over.
    #[test]
    fn a_folded_island_id_past_u64_max_is_fresh() {
        let body = |ids: &[&str]| {
            let md = vec!["![a](a.png)"; ids.len()].join("\n\n");
            let mut content = super::super::import_body(&md).unwrap().into_content();
            for (island, id) in content.islands.iter_mut().zip(ids) {
                island.id = id.to_string();
            }
            content
        };
        let isl = |n: u128| format!("isl-{n}");
        let max = u128::from(u64::MAX);
        let folded = append_block(body(&[&isl(max)]), body(&[&isl(max), &isl(max + 1)]));
        let ids: Vec<&str> = folded.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, [isl(max), isl(max + 2), isl(max + 1)]);
        folded.validate().unwrap();
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
            let json = serde_json::to_value(&Document {
                main,
                cards: vec![card],
            })
            .unwrap();
            let restored: Document = serde_json::from_value(json).unwrap_or_else(|e| panic!("{e}"));
            let again = Document::parse(&restored.to_markdown()).unwrap().document;
            assert_eq!(again.to_markdown(), restored.to_markdown());
            assert_eq!(again.main().body_lines(), restored.main().body_lines());
            restored.main().body_lines().clone()
        };

        let body = fold(
            "Intro.",
            serde_json::json!({ "name": "a\u{202D}b\u{2028}c\u{FFFC}d" }),
            "",
        );
        assert_eq!(
            body.text, "Intro.\nname: \"ab\\Lcd\"",
            "admitted as a body import admits it"
        );

        for (above, below) in [("> a", "> b"), ("1. a", "1. b"), ("- a", "- b")] {
            let body = fold(above, serde_json::json!({}), below);
            let top = |line: &quillmark_content::lines::model::Line| line.containers.first().cloned();
            assert_ne!(
                top(&body.lines[0]),
                top(body.lines.last().unwrap()),
                "{above:?}: {body:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_quill_reference() {
        let json = r#"{
            "schema": "quillmark/document@0.129.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "not a valid ref!!"},
                    {"type": "kind", "value": "main"}
                ]},
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}
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
            "schema": "quillmark/document@0.129.0",
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
            "schema": "quillmark/document@0.129.0",
            "main": {
                "payload": {"items": [
                    {"type": "quill", "value": "q@1.0"},
                    {"type": "kind", "value": "main"},
                    {"type": "seed", "value": {"indorsement": {"from": "49 FW/CC"}}}
                ]},
                "body": {"islands": [], "lines": [{"containers": [], "kind": "para"}], "marks": [], "text": ""}
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
    /// `doc` as a row of the line-form tag `schema`.
    fn v1_row(doc: &Document, schema: &str) -> serde_json::Value {
        let mut row = serde_json::to_value(doc).unwrap();
        row["schema"] = schema.into();
        row["main"]["body"] =
            quillmark_content::lines::serial::to_canonical_value(doc.main().body_lines());
        for (n, card) in doc.cards().iter().enumerate() {
            row["cards"][n]["body"] =
                quillmark_content::lines::serial::to_canonical_value(card.body_lines());
        }
        row
    }

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
            "V0_135_0 serialize→deserialize is a byte-fixed point"
        );
        assert_eq!(
            peek_storage_version(&first).as_deref(),
            Some(STORAGE_V0_135_0)
        );
    }

    /// A document holding a composable card saves under the newest tag, each card's
    /// `$id` an `id` item; an older row's cards mint theirs on load, the same
    /// ids on every load; a bad, duplicated, or main-card id refuses.
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

        let mut older = v1_row(&doc, STORAGE_V0_129_0);
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

    /// Every save writes the newest tag, and a row of any older tag loads
    /// with its body migrated, the same ids on every load.
    #[test]
    fn an_older_row_migrates_its_body_and_saves_under_the_newest_tag() {
        for schema in [STORAGE_V0_134_0, STORAGE_V0_131_0, STORAGE_V0_129_0] {
            let row = serde_json::json!({
                "schema": schema,
                "main": {
                    "payload": {"items": [
                        {"type": "quill", "value": "q@0.1"},
                        {"type": "kind", "value": "main"}
                    ]},
                    "body": {"text": "one\ntwo", "lines": [
                        {"kind": "para", "containers": []},
                        {"kind": "para", "containers": [], "continues": true}
                    ], "marks": [{"start": 0, "end": 3, "type": "anchor", "attrs": {"id": "c1"}}], "islands": []}
                },
                "cards": []
            });
            let first: Document = serde_json::from_value(row.clone()).unwrap();
            let second: Document = serde_json::from_value(row).unwrap();
            assert_eq!(first, second, "{schema}");
            assert_eq!(first.main().body().text, "one\u{2028}two", "{schema}");
            assert_eq!(first.main().body().anchors.len(), 1, "{schema}");
            let saved = serde_json::to_value(&first).unwrap();
            assert_eq!(saved["schema"], STORAGE_V0_135_0);
            assert_eq!(serde_json::from_value::<Document>(saved).unwrap(), first);
        }
    }

    /// Both tags refuse it, by two machineries: `CanonicalContent`'s
    /// parse-time check under the current one, the hop's own decode under
    /// `0.124.0`, whose frozen tree carries the body raw.
    #[test]
    fn deserialize_rejects_invalid_content_body() {
        for schema in [STORAGE_V0_129_0, STORAGE_V0_124_0] {
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
