//! Parsing and typed in-memory model for Quillmark card-yaml documents.
//!
//! A [`Document`] holds a root [`Card`] plus ordered composable cards. The
//! format is specified in
//! [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md).

use serde::{Deserialize, Serialize};

use quillmark_content::import::{from_markdown as import_markdown, ImportError};
use quillmark_content::Normalized;

use crate::error::ParseError;
use crate::version::QuillReference;
use crate::Diagnostic;

pub(crate) fn import_body(md: &str) -> Result<Normalized, ImportError> {
    if md.is_empty() {
        Ok(Normalized::empty())
    } else {
        import_markdown(md)
    }
}

/// Which encoding a [`Codec::decode_value`] failure came from, so a call site
/// can prefix its diagnostic per encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentDecodeError {
    NotContent(String),
    BadMarkdown(String),
}

impl ContentDecodeError {
    pub(crate) fn into_message(self) -> String {
        match self {
            ContentDecodeError::NotContent(m) | ContentDecodeError::BadMarkdown(m) => m,
        }
    }
}

/// A content codec: which authored string a [`Content`] field accepts, and which
/// text a stored content projects back to. Both codecs also accept a canonical
/// content object, so a codec is exactly the string end of the round trip. The
/// declared type names one (`reader::content_codec`), and every schema-bound
/// content read and projection runs the codec it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Codec {
    /// A string is markdown, and a content projects back to markdown — lossy,
    /// content-only marks not surviving.
    Richtext,
    /// A string is literal text (`*hi*` stays four characters), and a content
    /// projects back verbatim.
    Plaintext,
}

impl Codec {
    /// The schema keyword declaring this codec, carried verbatim on
    /// [`EditError::FieldDecode`].
    pub(crate) fn name(self) -> &'static str {
        match self {
            Codec::Richtext => edit::CODEC_RICHTEXT,
            Codec::Plaintext => edit::CODEC_PLAINTEXT,
        }
    }

    /// How this codec reads a stored string, for the shape-mismatch message.
    fn string_form(self) -> &'static str {
        match self {
            Codec::Richtext => "a markdown string",
            Codec::Plaintext => "a string",
        }
    }

    /// Decode a JSON value in either accepted encoding: a canonical content
    /// object, or an authored string read this codec's way. `None` when the value
    /// is neither an object nor a string; the call site handles those shapes and
    /// maps the error into its own type.
    pub(crate) fn decode_value(
        self,
        value: &serde_json::Value,
    ) -> Option<Result<Normalized, ContentDecodeError>> {
        match value {
            serde_json::Value::Object(_) => Some(
                quillmark_content::serial::from_canonical_value(value)
                    .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            ),
            serde_json::Value::String(s) => Some(match self {
                Codec::Richtext => {
                    import_body(s).map_err(|e| ContentDecodeError::BadMarkdown(e.to_string()))
                }
                Codec::Plaintext => Ok(quillmark_content::from_plaintext(s)),
            }),
            _ => None,
        }
    }

    /// The shape-mismatch message for a value in neither accepted encoding,
    /// naming the shape that arrived. Every surface that refuses an unshaped
    /// richtext value spells this one sentence.
    pub(crate) fn unshaped_message(self, value: &serde_json::Value) -> String {
        format!(
            "expected a {} content object or {}, got {}",
            self.name(),
            self.string_form(),
            match value {
                serde_json::Value::Bool(_) => "a boolean",
                serde_json::Value::Number(_) => "a number",
                serde_json::Value::Array(_) => "an array",
                _ => "an unsupported value",
            }
        )
    }

    /// [`decode_value`](Self::decode_value) closed over the shapes a stored field
    /// can hold: a null is the empty content (null ≡ absent), and anything
    /// neither object nor string is a decode failure.
    pub(crate) fn decode_field(
        self,
        value: &serde_json::Value,
    ) -> Result<Normalized, ContentDecodeError> {
        match self.decode_value(value) {
            Some(result) => result,
            None if value.is_null() => Ok(Normalized::empty()),
            None => Err(ContentDecodeError::NotContent(self.unshaped_message(value))),
        }
    }

    /// Project a content back to this codec's text: the inverse of its string
    /// encoding.
    pub(crate) fn project(self, content: &Normalized) -> String {
        match self {
            Codec::Richtext => quillmark_content::export::to_markdown(content),
            Codec::Plaintext => quillmark_content::export::to_plaintext(content),
        }
    }
}

/// Why a richtext value did not become stored content, classified so each layer
/// words the failure in its own error vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RichtextValueError {
    /// An accepted encoding that failed to decode.
    Decode(ContentDecodeError),
    /// Neither a canonical content object nor a markdown string.
    Unshaped,
    /// `inline` is declared and the content spans more than one paragraph.
    NotInline,
}

/// The contract for a richtext value that must *become* stored content: decode
/// either accepted encoding, enforce `inline`, canonicalize. The strict typed
/// write and the schema-literal companion cache share it and differ only in the
/// diagnostic they render from the error.
pub(crate) fn canonical_richtext_value(
    value: &serde_json::Value,
    inline: bool,
) -> Result<serde_json::Value, RichtextValueError> {
    let content = match Codec::Richtext.decode_value(value) {
        Some(result) => result.map_err(RichtextValueError::Decode)?,
        None => return Err(RichtextValueError::Unshaped),
    };
    if inline && !content.is_inline() {
        return Err(RichtextValueError::NotInline);
    }
    Ok(quillmark_content::serial::to_canonical_value(&content))
}

pub mod assemble;
pub mod dto;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod meta;
pub mod payload;
pub(crate) mod prescan;
pub mod wire;
pub(crate) mod yaml_hints;

pub use dto::{
    peek_storage_version, StorageError, StoredDocument, STORAGE_V0_112_0, STORAGE_V0_93_0,
};
pub use edit::EditError;
pub use meta::{is_valid_kind_name, validate_composable_kind, CardKindError};
pub use payload::{MetaKey, Payload, PayloadItem};
// Reachable through `PayloadItem`'s `nested_comments` fields, so nameable from here.
pub use prescan::NestedComment;
pub use wire::{CardWire, PayloadItemWire, WireError};

/// Authoring-format rules for the `~~~` card-yaml markdown surface, surfaced
/// verbatim to LLM/MCP consumers and to the CLI / Python bindings. The single
/// source of truth: bindings call in rather than re-stating the rules.
pub const FORMAT_RULES: &str = "Document format rules:
\u{2022} Block opener and closer are EXACTLY `~~~` (three tildes, no info string). The `~~~card-yaml` opener is also accepted as a non-canonical alias.
\u{2022} A blank line must precede every `~~~` block opener (unless it is line 1), and the opener must be at column zero (no leading spaces). An indented `~~~` is an ordinary code block, not a card.
\u{2022} The first block is the root and MUST contain `$quill: <name>@<version>`. Its `$kind` is `main` by position \u{2014} an explicit `$kind: main` is accepted but not required. Additional blocks declare composable cards via `$kind: <card_kind>`.
\u{2022} Reserved `$`-keys: `$quill`, `$kind`, `$ext`, `$seed`. User fields use lowercase snake_case.
\u{2022} Prose body is the text after a block's closing `~~~`, up to the next opener or EOF. To include a literal fenced code block in prose, use a backtick fence (```); any column-zero `~~~` block is parsed as card metadata.
\u{2022} A field that already shows a concrete value carries a default and is shippable as-is \u{2014} keep the line, override the value, or delete it to fall back to the default. A blank or null value (`field:`, `field: null`, `field: ~`) is treated the same as omitting the field: it falls back to the default, or to the field's blank. An explicit `field: \"\"` is different \u{2014} it is kept as-is, not folded into the blank/null fallback, so write it on purpose when you want the field empty rather than defaulted.
\u{2022} `field: !must_fill <value>` marks a placeholder awaiting your input \u{2014} the `<value>` shown, when present, is the schema's own example, not real data; replace it with a real value and drop the `!must_fill` tag before shipping. A bare `field: !must_fill` is an empty placeholder. A leftover marker never blocks rendering, but it is reported as a warning until you replace it.
\u{2022} Numbers and booleans MUST be unquoted (`year: 2025`, `pinned: true`); quoting turns them into strings and fails validation.
\u{2022} Plain-scalar values cannot start with `*` or `&` (YAML alias/anchor markers) and cannot contain `: ` (colon-space). For markdown emphasis, embedded colons, or other special prefixes, quote the value: `field: '**bold**'` or `field: \"Name: subtitle\"`. Multi-line values use `|-`, not multi-line quoted scalars.";

/// Header shown above [`FORMAT_RULES`], which covers the field-level semantics;
/// `{quill}` is substituted with the quill name.
const BLUEPRINT_INSTRUCTION_TEMPLATE: &str =
    "Fill in the `{quill}` blueprint below: replace each `!must_fill` placeholder with a real \
value and edit the body prose. Submit the filled markdown as `content` to `create_document`.";

/// Render the blueprint-instruction header with `quill_name` substituted in.
pub fn blueprint_instruction(quill_name: &str) -> String {
    BLUEPRINT_INSTRUCTION_TEMPLATE.replace("{quill}", quill_name)
}

#[cfg(test)]
mod tests;

/// The record of one load: the [`Document`] and any non-fatal warnings.
/// Returned by both [`Document::parse`] and the bound
/// [`Quill::parse`](crate::Quill::parse), whose `warnings` also carry the
/// `conform::*` ones. Warnings live here and only here: `Document` is the
/// value, `Parsed` the load event.
#[derive(Debug)]
#[must_use = "carries parse warnings; read `.document`/`.warnings` or bind it"]
pub struct Parsed {
    pub document: Document,
    pub warnings: Vec<Diagnostic>,
}

/// A single card-yaml block (root or composable). `body` is the content
/// ([`Content`](quillmark_content::Content)) form of the prose after the closing fence: the empty content
/// when none follows; check `card.body().is_blank()`. Markdown is a projection:
/// [`Card::body_markdown`] re-emits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    payload: Payload,
    body: Normalized,
}

impl Card {
    /// Create a `Card` from its parts without validation.
    pub(crate) fn from_parts(payload: Payload, body: Normalized) -> Self {
        Self { payload, body }
    }

    pub fn quill(&self) -> Option<&QuillReference> {
        self.payload.quill()
    }

    pub fn kind(&self) -> Option<&str> {
        self.payload.kind()
    }

    /// Opaque `$ext` map for out-of-band extension data (UI editor state,
    /// agent annotations, …). Carried through Markdown and storage DTO
    /// round-trips; never emitted into the plate JSON backends consume.
    pub fn ext(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload.ext()
    }

    /// The card's card-yaml storage as a read view. Writes go through the
    /// enforcing verbs ([`store_field`](Card::store_field),
    /// [`TypedWriter`](crate::TypedWriter)).
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        &mut self.payload
    }

    /// The card body as a [`Content`](quillmark_content::Content): the canonical content model. For the
    /// markdown projection use [`Card::body_markdown`].
    pub fn body(&self) -> &Normalized {
        &self.body
    }

    /// The card body rendered back to its markdown projection. This is a
    /// derived view (`export ∘ body`), not stored state; a `Document` round-trip
    /// therefore canonicalizes the body (e.g. `__b__` → `**b**`).
    pub fn body_markdown(&self) -> String {
        quillmark_content::export::to_markdown(&self.body)
    }

    pub(crate) fn body_mut(&mut self) -> &mut Normalized {
        &mut self.body
    }

    /// Read a content-valued user field back through `codec`: the field-level
    /// twin of [`Card::body`].
    ///
    /// - `None`: the field is absent.
    /// - `Some(Ok(content))`: decoded content, from either of the codec's
    ///   encodings — a stored string and an already-canonical content object read
    ///   back the same.
    /// - `Some(Err(_))`: the field is present but decodes under neither (e.g. a
    ///   bare number an opaque `store_field` wrote).
    ///
    /// A `Document` carries no schema, so the caller names the codec the field is
    /// declared at; the schema-bound door is
    /// [`TypedReader::get_content`](crate::TypedReader::get_content).
    pub(crate) fn field_content(
        &self,
        name: &str,
        codec: Codec,
    ) -> Option<Result<Normalized, ContentDecodeError>> {
        Some(codec.decode_field(self.payload.get(name)?.as_json()))
    }

    /// The text projection of a content-valued field (`project ∘ decode`),
    /// carrying [`field_content`](Card::field_content)'s `None`/`Ok`/`Err`.
    pub(crate) fn field_text(
        &self,
        name: &str,
        codec: Codec,
    ) -> Option<Result<String, ContentDecodeError>> {
        Some(self.field_content(name, codec)?.map(|c| codec.project(&c)))
    }
}

/// A parsed, per-kind **seed overlay**: the sparse fields (and optional body)
/// a newly-added card of a given kind starts with. Built from a `$seed[<kind>]`
/// entry of the main card's [`Card::seed`] map via [`SeedOverlay::from_json`],
/// and layered over the quill's schema-example seed by
/// [`crate::Quill::seed_card`] (overlay › example › absent). The reserved inner
/// key `$body` carries the body override; every other user field becomes an
/// entry, while any other `$`-prefixed key is reserved and dropped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeedOverlay {
    /// Field-value overrides, keyed by field name.
    pub fields: indexmap::IndexMap<String, crate::value::QuillValue>,
    /// Body override, when the overlay declares a `$body` string.
    pub body: Option<String>,
}

impl SeedOverlay {
    /// Parse an overlay from a `$seed[<kind>]` JSON value, or `None` when it is
    /// not a mapping. Use this to turn the raw overlay object a consumer reads
    /// from the main card's `$seed` map ([`Card::seed`]) into a typed overlay to
    /// hand to [`crate::Quill::seed_card`]; e.g.
    /// `doc.main().seed().and_then(|m| m.get(kind)).and_then(SeedOverlay::from_json)`.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        value.as_object().map(Self::from_json_map)
    }

    /// Build an overlay from a single `$seed[<kind>]` JSON map: the reserved
    /// `$body` string becomes [`body`](Self::body); every other user-field entry
    /// becomes a field. A non-string `$body` is ignored (no body override). Any
    /// other `$`-prefixed key is reserved and dropped (never stored as a user
    /// field) since an overlay only ever carries user fields plus `$body`.
    fn from_json_map(map: &serde_json::Map<String, serde_json::Value>) -> Self {
        let mut fields = indexmap::IndexMap::new();
        let mut body = None;
        for (key, value) in map {
            if key == "$body" {
                if let Some(s) = value.as_str() {
                    body = Some(s.to_string());
                }
            } else if key.starts_with('$') {
                // Reserved key other than `$body`: not a user field. Drop it
                // rather than smuggle a `$`-key into the field set.
                continue;
            } else {
                fields.insert(
                    key.clone(),
                    crate::value::QuillValue::from_json(value.clone()),
                );
            }
        }
        SeedOverlay { fields, body }
    }
}

/// A fully-parsed Quillmark document. Serde routes through [`StoredDocument`].
///
/// Parse-time warnings are *not* document state: they ride out-of-band on
/// [`Parsed`] from [`Document::parse`], the single owner. Equality and the
/// storage DTO therefore cover only structural content (`main` and `cards`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "StoredDocument", try_from = "StoredDocument")]
pub struct Document {
    main: Card,
    cards: Vec<Card>,
}

impl Document {
    /// Create a blank document: a main card carrying only `$quill`, an empty
    /// body, and no composable cards. The programmatic blank canvas: every
    /// schema field is absent and resolves at render time (`default`, else
    /// the field's blank), so nothing the caller did not set reaches the
    /// output. For an example-filled starter shaped like the blueprint, use
    /// `Quill::seed_document`.
    pub fn new(quill: QuillReference) -> Self {
        let mut payload = Payload::new();
        payload.set_quill(quill);
        // Parsed main cards always carry `$kind: main` (the parser normalizes
        // it in); match that shape so a blank document round-trips equal.
        payload.set_kind("main");
        Self {
            main: Card::from_parts(payload, Normalized::empty()),
            cards: Vec::new(),
        }
    }

    /// Create a `Document` from a pre-built main card and composable cards.
    /// `main` must carry `$quill`; composable cards must not carry `$quill` or
    /// `$seed`.
    ///
    /// The invariants are `debug_assert`s, so a release build accepts a main
    /// card without `$quill` and [`quill_reference`](Self::quill_reference)
    /// panics on it: every caller pre-validates. [`Document::new`] is the
    /// public blank canvas, and `TryFrom<StoredDocument>` the public door for
    /// external data, checking all three and returning
    /// `StorageError::Malformed`.
    pub(crate) fn from_main_and_cards(main: Card, cards: Vec<Card>) -> Self {
        debug_assert!(main.quill().is_some(), "main card must carry `$quill`");
        debug_assert!(
            cards.iter().all(|c| c.quill().is_none()),
            "composable cards must not carry `$quill`"
        );
        debug_assert!(
            cards.iter().all(|c| c.seed().is_none()),
            "composable cards must not carry `$seed`"
        );
        Self { main, cards }
    }

    /// Parse card-yaml Markdown into a [`Parsed`]: the [`Document`] plus any
    /// non-fatal warnings. The single parse entry; a caller that wants only the
    /// document writes `Document::parse(md)?.document`. Errors on malformed
    /// YAML, a missing root `$quill`, an over-size input, and the other
    /// [`ParseError`] variants.
    #[doc(alias = "from_markdown")]
    pub fn parse(markdown: &str) -> Result<Parsed, ParseError> {
        assemble::decompose_with_warnings(markdown)
            .map(|(document, warnings)| Parsed { document, warnings })
    }

    pub fn main(&self) -> &Card {
        &self.main
    }

    pub fn main_mut(&mut self) -> &mut Card {
        &mut self.main
    }

    /// The `$quill` reference from the root block. Always present on parsed documents.
    pub fn quill_reference(&self) -> QuillReference {
        self.main
            .quill()
            .cloned()
            .expect("root block's $quill is validated at parse time")
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn cards_mut(&mut self) -> &mut [Card] {
        &mut self.cards
    }

    /// A single composable card by index: the immutable twin of
    /// [`card_mut`](Document::card_mut), so reading one card's payload does not
    /// require materializing every card via [`cards`](Document::cards). `None`
    /// when out of range.
    pub fn card(&self, index: usize) -> Option<&Card> {
        self.cards.get(index)
    }

    pub(crate) fn cards_vec_mut(&mut self) -> &mut Vec<Card> {
        &mut self.cards
    }

    /// Serialize to the JSON wire shape consumed by backend plates. This is
    /// the **only** place in `quillmark-core` that produces this shape:
    ///
    /// ```json
    /// {
    ///   "$quill": "<ref>",
    ///   "$body": { "text": "…", "lines": [...], "marks": [...], "islands": [...] },
    ///   "$cards": [{ "$kind": "<tag>", "$body": <content>, "<field>": <value>, ... }],
    ///   "<field>": <value>, ...
    /// }
    /// ```
    ///
    /// `$body` (global and per-card) is canonical Content-JSON: the content as
    /// a nested object, not a markdown string. Richtext payload fields likewise
    /// cross as content objects (committed at coercion time).
    ///
    /// `$`-prefixed keys carry document-level metadata (quill ref, body
    /// text, card list, card kind). User payload fields stay flat at the
    /// root: they cannot collide with `$` keys because user field names are
    /// never `$`-prefixed (they match `[A-Za-z_][A-Za-z0-9_]*`).
    ///
    /// `$kind` is document-defined and omitted for a kindless card (never a
    /// fabricated `""`).
    ///
    /// Body presence is the caller's decision: the root carries `$body` iff
    /// `main_body`, and card *i* iff `card_bodies` is `None` (all present) or
    /// `card_bodies[i]` holds. The schema-gated render plate
    /// (`QuillConfig::compile_data`) passes the body-enabled bit it already
    /// resolved per card, so a card whose kind enables no body carries no
    /// `$body`, and the decision is never re-derived from the serialized plate.
    /// `Document` stays schema-free: it receives the decision, not a schema.
    pub(crate) fn to_plate_json_gated(
        &self,
        main_body: bool,
        card_bodies: Option<&[bool]>,
    ) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        map.insert(
            "$quill".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        // The seam carries the body as canonical Content-JSON (Option A): a
        // nested content object, byte-identical to `to_canonical_json`, never a lossy
        // markdown string. Backends lower the content (typst → markup + source
        // map; pdfform → `.text`); the markdown projection is `body_markdown`.
        if main_body {
            map.insert(
                "$body".to_string(),
                quillmark_content::serial::to_canonical_value(self.main.body()),
            );
        }

        let cards_array: Vec<serde_json::Value> = self
            .cards
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let mut card_map = serde_json::Map::new();
                // A kindless card carries no `$kind`, never a fabricated `""`:
                // matching the resolved view's `kind: None`.
                if let Some(kind) = card.kind() {
                    card_map.insert(
                        "$kind".to_string(),
                        serde_json::Value::String(kind.to_string()),
                    );
                }
                // `$body` iff the caller's schema defines a body for this card.
                if card_bodies.map_or(true, |f| f.get(i).copied().unwrap_or(true)) {
                    card_map.insert(
                        "$body".to_string(),
                        quillmark_content::serial::to_canonical_value(card.body()),
                    );
                }
                for (key, value) in card.payload.iter() {
                    card_map.insert(key.clone(), value.as_json().clone());
                }
                serde_json::Value::Object(card_map)
            })
            .collect();

        map.insert("$cards".to_string(), serde_json::Value::Array(cards_array));

        for (key, value) in self.main.payload.iter() {
            map.insert(key.clone(), value.as_json().clone());
        }

        serde_json::Value::Object(map)
    }
}
