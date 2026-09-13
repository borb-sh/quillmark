//! The `Content` content model: one text sequence per field carrying line
//! attributes, anchored marks, and embedded islands, over a single coordinate
//! space of Unicode scalar values (Rust `char`).
//!
//! Editor-specific policy (edge-expand, adjacent-merge-at-insertion) is *not*
//! encoded: the model stores only the resulting range, so the stored form is
//! identical whatever the editor did.

use crate::island::IslandType;
use crate::normalize::{is_bidi_char, is_line_separator};
use serde_json::Value as JsonValue;
use std::borrow::Cow;

/// A position in a [`Content`], counted in Unicode scalar values (USV): never
/// bytes, never UTF-16 units. One astral char is 1 USV / 4 UTF-8 bytes / 2
/// UTF-16 units. [`crate::usv`] converts one to the UTF-8 byte offset Rust
/// slicing needs.
pub type Usv = usize;

/// U+FFFC OBJECT REPLACEMENT CHARACTER: the single-USV slot an island occupies
/// in the content. One slot per island; every slot has a backing island. A stray
/// slot (or a slot with no island) is an invariant violation.
pub const ISLAND_SLOT: char = '\u{FFFC}';

/// One content field as a content: the text plus the structure that rides on it.
///
/// The mint ([`Content::into_normalized`]) establishes the canonical form:
/// marks sorted and unioned, container paths renumbered, a line's kind agreeing
/// with its text, a block island's slot alone on its line, table props on one
/// column count. [`Content::validate`] reports what the mint cannot repair: the
/// text holds no `\r`, no bidi controls, and no line separator (every character
/// Typst reads as a newline but `\n`); the count of [`ISLAND_SLOT`] equals
/// `islands.len()`; `lines.len()` equals the number of `\n`-separated segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    /// The content. `\n` is a line boundary; [`ISLAND_SLOT`] is an island slot.
    pub text: String,
    /// One entry per `\n`-separated segment of `text`, in order. The line tree
    /// is *derived* from this flat list plus each line's `containers` path,
    /// never stored, so a split/join is a single-char edit with no paragraph
    /// identity to reconcile.
    pub lines: Vec<Line>,
    /// Marks over char ranges, kept normalized: sorted by
    /// `(start, end, type, attrs)`, same-kind formatting marks unioned.
    pub marks: Vec<Mark>,
    /// One entry per [`ISLAND_SLOT`], in slot order (ascending char position).
    pub islands: Vec<Island>,
}

/// A line's attributes: its block role plus the container path it sits in.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    /// Ancestor containers, outermost first. A multi-paragraph list item is two
    /// `Para` lines sharing one `[ListItem]` path; a paragraph in a quote in a
    /// list item is `[ListItem, Quote]`.
    pub containers: Vec<Container>,
    /// Whether this line continues the previous line's *block* across a hard
    /// line break rather than starting a new block. `false` = a new block
    /// (paragraph spacing on either side); `true` = a within-block line break (a
    /// markdown hard break; consecutive lines of one code fence). The first line
    /// is always `false`, as is one whose container path differs from the line
    /// above or whose block above renders a single line
    /// ([`LineKind::takes_continuations`]).
    pub continues: bool,
}

impl Line {
    /// A line of `kind` at the top level: no containers, starting a new block,
    /// which is also what the wire reads off the absent keys.
    pub fn new(kind: LineKind) -> Self {
        Line {
            kind,
            containers: Vec::new(),
            continues: false,
        }
    }

    /// Set the ancestor path, outermost first.
    pub fn with_containers(mut self, containers: Vec<Container>) -> Self {
        self.containers = containers;
        self
    }

    /// Set [`continues`](Self::continues): `true` makes this line a within-block
    /// break off the previous one rather than a new block.
    pub fn with_continues(mut self, continues: bool) -> Self {
        self.continues = continues;
        self
    }
}

/// The block role of a line. The tree between lines is inferred: two adjacent
/// lines with equal `kind`+`containers` are two blocks of that role (e.g. two
/// paragraphs), never one.
///
/// **Closed**, on the same terms as [`MarkKind`]: a `kind` outside this set is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName) at every
/// decoder, so adding a role is a storage-version event.
#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Para,
    /// ATX/Setext heading, level 1..=6.
    Heading {
        level: u8,
    },
    /// A line of a code block. `lang` is the (sanitized) info string, shared by
    /// every line of the same block.
    Code {
        lang: Option<String>,
    },
    /// A block-level island: the line's sole content is one [`ISLAND_SLOT`]
    /// backing an island whose markdown *is* a block
    /// ([`IslandType::block_only`](crate::IslandType::block_only)).
    /// An image is inline markup, so a line holding one alone is
    /// [`Para`](LineKind::Para) — the kind re-importing `![alt](url)` yields,
    /// and the kind [`Content::normalize`] writes there.
    Island,
    /// A thematic break (`---`/`***`/`___`). The line carries no text.
    Rule,
}

impl LineKind {
    /// Whether a block of this kind renders the lines that [`Line::continues`]
    /// joins to its first. A paragraph spans its hard-break run and a code
    /// block its fence's interior; a heading, an island and a rule are one
    /// line, and both emitters render that line alone, so a continuation there
    /// is text the projection never reaches.
    pub fn takes_continuations(&self) -> bool {
        matches!(self, LineKind::Para | LineKind::Code { .. })
    }

    /// The wire `kind` name.
    pub fn tag(&self) -> &'static str {
        match self {
            LineKind::Para => "para",
            LineKind::Heading { .. } => "heading",
            LineKind::Code { .. } => "code",
            LineKind::Island => "island",
            LineKind::Rule => "rule",
        }
    }

    /// The payload bag, one spelling for every member of the vocabulary.
    pub fn attrs(&self) -> Cow<'_, JsonValue> {
        match self {
            LineKind::Para | LineKind::Island | LineKind::Rule => Cow::Owned(JsonValue::Null),
            LineKind::Heading { level } => Cow::Owned(bag([("level", (*level).into())])),
            LineKind::Code { lang } => Cow::Owned(match lang {
                Some(l) => bag([("lang", l.as_str().into())]),
                None => JsonValue::Null,
            }),
        }
    }
}

/// A payload bag from its entries, which must be listed in ascending key order:
/// [`Content::normalize`] canonicalizes an opaque bag, and a minted one is
/// canonical by construction.
fn bag<const N: usize>(entries: [(&str, JsonValue); N]) -> JsonValue {
    debug_assert!(entries.windows(2).all(|w| w[0].0 < w[1].0));
    let mut m = serde_json::Map::with_capacity(N);
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    JsonValue::Object(m)
}

/// Whether a payload bag holds nothing: the two spellings of "no payload" a
/// decode can produce (an absent key reads as `Null`, an empty object is one).
/// [`Content::normalize`] collapses them to `Null`, so a bag's presence on the
/// wire stays a pure function of the value.
pub(crate) fn is_empty_bag(v: &JsonValue) -> bool {
    match v {
        JsonValue::Null => true,
        JsonValue::Object(m) => m.is_empty(),
        _ => false,
    }
}

/// A container a line nests inside. The ancestor path is a `Vec<Container>`.
///
/// **Closed**, on [`LineKind`]'s terms: a `container` outside this set is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName).
#[derive(Debug, Clone, PartialEq)]
pub enum Container {
    /// A list item. `ordered` distinguishes `1.` from `-`; `start` is the list's
    /// first number (1 by default); `ordinal` is this item's 0-based index in
    /// its list; `instance` tells this list from an adjacent one of the same
    /// shape (see [`Container::instance`]).
    ///
    /// Two *adjacent* lines belong to the same item iff their whole container
    /// path is equal. Identity is path **plus contiguity**: two sibling inner
    /// lists under one outer item can produce equal first-item paths,
    /// distinguished only by the non-adjacency of their runs.
    ListItem {
        ordered: bool,
        start: u64,
        ordinal: u64,
        instance: u64,
    },
    /// A block quote. Adjacent lines sharing one `Quote` are one
    /// multi-paragraph quote; two adjacent quotes differ in `instance`.
    Quote { instance: u64 },
}

impl Container {
    /// Which instance of its shape this container is, among a run of adjacent
    /// siblings that would otherwise be indistinguishable.
    ///
    /// Container identity is path plus contiguity, so two adjacent runs of
    /// equal shape read as one: `[Quote], [Quote]` is one quote, and two
    /// one-item lists are one item spanning two paragraphs. `instance` is the
    /// one field that exists to break that tie, and it is the whole reason the
    /// encoding is complete rather than a quotient.
    ///
    /// A producer writes it against [`same_run`](Self::same_run): two adjacent
    /// runs of one shape need distinct values, whatever they are, or they
    /// arrive as one container. [`Content::normalize`] collapses those to the
    /// canonical pair — **0, flipping to 1 only where the projection would
    /// otherwise [weld](Self::same_weld) the two** — so a document needing no
    /// discriminator carries none and one that needs it alternates
    /// `0, 1, 0, 1`. Non-adjacent runs never collide, so two values suffice.
    pub fn instance(&self) -> u64 {
        match self {
            Container::ListItem { instance, .. } | Container::Quote { instance } => *instance,
        }
    }

    /// The wire `container` name.
    pub fn tag(&self) -> &'static str {
        match self {
            Container::ListItem { .. } => "list_item",
            Container::Quote { .. } => "quote",
        }
    }

    /// The payload bag, one spelling for every member of the vocabulary.
    /// `instance` is not in it: it is an envelope key, carried on every arm.
    pub fn attrs(&self) -> Cow<'_, JsonValue> {
        match self {
            Container::ListItem {
                ordered,
                start,
                ordinal,
                ..
            } => Cow::Owned(bag([
                ("ordered", (*ordered).into()),
                ("ordinal", (*ordinal).into()),
                ("start", (*start).into()),
            ])),
            Container::Quote { .. } => Cow::Owned(JsonValue::Null),
        }
    }

    fn set_instance(&mut self, n: u64) {
        match self {
            Container::ListItem { instance, .. } | Container::Quote { instance } => *instance = n,
        }
    }

    /// Whether these two are the same container shape, `ordinal` and `instance`
    /// aside — `start` counts, so a list starting at 1 and one starting at 3
    /// are two shapes.
    ///
    /// The **identity** rule, read and written alike: two adjacent lines sit in
    /// one container instance iff this holds *and* their
    /// [`instance`](Self::instance)s are equal. [`crate::traverse::runs`]
    /// applies it, and a producer separates its runs against it. Whether the
    /// *projection* can then tell two runs apart is
    /// [`same_weld`](Self::same_weld).
    pub fn same_run(&self, other: &Container) -> bool {
        match (self, other) {
            (
                Container::ListItem {
                    ordered: a, start: b, ..
                },
                Container::ListItem {
                    ordered: c, start: d, ..
                },
            ) => a == c && b == d,
            (Container::Quote { .. }, Container::Quote { .. }) => true,
            _ => false,
        }
    }

    /// Whether the Markdown projection would read two adjacent runs of these
    /// shapes as one — that is, whether the canonical form must spend an
    /// [`instance`](Self::instance) to keep them apart.
    /// [`Content::normalize`] mints against this;
    /// [`same_run`](Self::same_run) is what a producer writes against.
    ///
    /// Coarser than `same_run` for lists, because `start` is invisible in
    /// Markdown: CommonMark reads only a list's *first* number, so `1. a`
    /// beside `3. b` re-imports as one list of two items and the second list's
    /// `start` is lost. Comparing `ordered` alone mints the discriminator
    /// there too, and the marker alternation carries it.
    pub fn same_weld(&self, other: &Container) -> bool {
        match (self, other) {
            (Container::ListItem { ordered: a, .. }, Container::ListItem { ordered: b, .. }) => {
                a == b
            }
            _ => self.same_run(other),
        }
    }
}

/// A [`Content`] that [`Content::normalize`] has run on: the precondition both
/// projections carry. Minted only by [`Content::into_normalized`], which the
/// codecs decode through; a mutation that does not re-establish the invariant
/// takes [`into_content`](Self::into_content) and mints again.
///
/// ## Canonical, not valid
///
/// [`validate`](Content::validate) rejects a disjoint set: what normalization
/// cannot repair. Nothing it does brings a container path under
/// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH), so a token can hold a
/// content `validate` refuses, and the mint stays infallible on that split. The
/// codecs call `validate` after minting; a Rust embedder hand-building a
/// [`Content`] may not.
///
/// A projection taking a token may therefore assume only what the mint
/// establishes, and must be **total over any token**:
/// [`to_markdown`](crate::to_markdown) walks containers on an explicit stack
/// rather than a frame per level, and `emit_content` checks the depth and
/// returns an error. An unguarded recursion aborts the process, which no
/// `Result` can catch.
#[derive(Debug, Clone, PartialEq)]
pub struct Normalized(Content);

impl Normalized {
    /// [`Content::empty`], which is already canonical.
    pub fn empty() -> Normalized {
        Normalized(Content::empty())
    }

    pub fn into_content(self) -> Content {
        self.0
    }

    /// Every caller must leave this normalized; the forwarded `apply_*` in
    /// [`crate::ops`] are the ones that do.
    pub(crate) fn as_content_mut(&mut self) -> &mut Content {
        &mut self.0
    }
}

impl From<Content> for Normalized {
    fn from(rt: Content) -> Normalized {
        rt.into_normalized()
    }
}

impl std::ops::Deref for Normalized {
    type Target = Content;

    fn deref(&self) -> &Content {
        &self.0
    }
}

/// A mark over a char range `[start, end)`. `start == end` (zero-width) is legal
/// only for [`MarkKind::Anchor`]; normalization drops zero-width formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub start: Usv,
    pub end: Usv,
    pub kind: MarkKind,
}

impl Mark {
    pub fn new(start: Usv, end: Usv, kind: MarkKind) -> Self {
        Mark { start, end, kind }
    }
}

/// The mark set, **closed**: a `type` outside it is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName). Two
/// algebra classes: formatting is a property of a range (two coincident are
/// redundant); identity is a handle (two over the same range are two things).
#[derive(Debug, Clone, PartialEq)]
pub enum MarkKind {
    // Formatting: round-trippable projection marks. `is_formatting()`.
    Strong,
    Emph,
    Underline,
    Strike,
    Code,
    Link {
        url: String,
    },
    // Identity: a handle, not a property. Never merged, may be zero-width.
    /// A comment thread or stable anchor, carried by id and rebased across
    /// edits like any position. The id is caller-supplied, unique per `Content`,
    /// and invariant while the mark lives; moved-and-rewritten text drops the
    /// mark whole. No markdown projection: it is omitted on export and survives
    /// via diff-rebase.
    Anchor {
        id: String,
    },
}

/// A structured object with no honest text encoding (a table, figure, or future
/// embed) occupying one [`ISLAND_SLOT`] in the content.
#[derive(Debug, Clone, PartialEq)]
pub struct Island {
    /// Deterministically minted, session-stable id: `isl-{n}` by import
    /// position. Part of the canonical form and thus hash input, so it is never
    /// ambient. Edits keep it stable rather than re-deriving it, so
    /// [`Content::validate`] enforces uniqueness, not positional equality.
    pub id: String,
    /// Island type discriminator, closed: see [`IslandType`].
    pub island_type: IslandType,
    /// Typed payload. Recursively key-sorted by normalization so it hashes
    /// deterministically despite `serde_json`'s `preserve_order`.
    pub props: JsonValue,
    /// How faithfully the markdown projection can carry this island.
    pub loss: Loss,
}

impl Island {
    /// An island of `island_type` under `id`, carrying no payload and claiming
    /// no projection loss, which is also what the wire reads off the absent
    /// keys.
    pub fn new(id: String, island_type: IslandType) -> Self {
        Island {
            id,
            island_type,
            props: JsonValue::Null,
            loss: Loss::Lossless,
        }
    }

    pub fn with_props(mut self, props: JsonValue) -> Self {
        self.props = props;
        self
    }

    pub fn with_loss(mut self, loss: Loss) -> Self {
        self.loss = loss;
        self
    }
}

/// How faithfully the markdown projection carries an island: a **description**
/// of what the projection does with it, for a consumer to surface. It is not a
/// switch: [`crate::export::to_markdown`] dispatches on
/// [`Island::island_type`], never on this.
///
/// Closed: a `loss` outside this set is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName), so a
/// consumer laddering on it has no rung it cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    /// Round-trips identically.
    Lossless,
    /// Round-trips visibly, not identically.
    Degraded,
    /// No markdown encoding, and where an uninterpretable class lands.
    Unrepresentable,
}

impl Loss {
    /// Every level, faithful first: the one enumeration point.
    pub const ALL: &'static [Loss] = &[Loss::Lossless, Loss::Degraded, Loss::Unrepresentable];

    /// The wire class naming this level: the one place a class is spelled.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::Degraded => "degraded",
            Self::Unrepresentable => "unrepresentable",
        }
    }

    /// Parse a wire class; `parse(l.as_str()) == Some(l)` for every variant.
    pub fn parse(class: &str) -> Option<Loss> {
        Self::ALL.iter().copied().find(|f| f.as_str() == class)
    }
}

impl MarkKind {
    /// Formatting marks are a property of a range and union when coincident;
    /// an identity mark is a handle and never merges.
    ///
    /// Class membership is stored meaning, not presentation: moving a member
    /// *into* this class starts unioning adjacent runs that round-tripped as
    /// two marks, moving the canonical bytes of documents nobody edited.
    pub fn is_formatting(&self) -> bool {
        matches!(
            self,
            MarkKind::Strong
                | MarkKind::Emph
                | MarkKind::Underline
                | MarkKind::Strike
                | MarkKind::Code
                | MarkKind::Link { .. }
        )
    }

    /// The wire `type` name.
    pub fn tag(&self) -> &'static str {
        match self {
            MarkKind::Strong => "strong",
            MarkKind::Emph => "emph",
            MarkKind::Underline => "underline",
            MarkKind::Strike => "strike",
            MarkKind::Code => "code",
            MarkKind::Link { .. } => "link",
            MarkKind::Anchor { .. } => "anchor",
        }
    }

    /// The payload bag, one spelling for every member of the vocabulary.
    pub fn attrs(&self) -> Cow<'_, JsonValue> {
        match self {
            MarkKind::Strong
            | MarkKind::Emph
            | MarkKind::Underline
            | MarkKind::Strike
            | MarkKind::Code => Cow::Owned(JsonValue::Null),
            MarkKind::Link { url } => Cow::Owned(bag([("url", url.as_str().into())])),
            MarkKind::Anchor { id } => Cow::Owned(bag([("id", id.as_str().into())])),
        }
    }

    /// The canonical sort tie-break after `(start, end)`, and the grouping key
    /// for same-kind union (two `link`s union only at one url).
    ///
    /// It is the pair the **wire** carries, read back off the value, so
    /// canonical order is a function of the stored bytes rather than of variant
    /// declaration order: adding a member reorders nothing already stored.
    ///
    /// The attrs half comes from [`attrs`](Self::attrs) rather than a string per
    /// arm, so it cannot disagree with what the encoder writes.
    pub fn sort_key(&self) -> (String, String) {
        let attrs = self.attrs();
        let attrs = if is_empty_bag(&attrs) {
            String::new()
        } else {
            canonical_json_string(&attrs)
        };
        (self.tag().to_string(), attrs)
    }
}

/// A `serde_json::Value` rendered to a string with object keys recursively
/// sorted: order-insensitive, so it is a stable comparison/grouping key.
pub(crate) fn canonical_json_string(v: &JsonValue) -> String {
    if is_value_key_sorted(v) {
        return serde_json::to_string(v).unwrap_or_default();
    }
    serde_json::to_string(&sort_keys_owned(v.clone())).unwrap_or_default()
}

/// Whether every object in `v` already has its keys in ascending order,
/// recursively: the allocation-free check that lets a re-normalize skip
/// rebuilding an already-canonical tree.
pub(crate) fn is_value_key_sorted(v: &JsonValue) -> bool {
    match v {
        JsonValue::Array(items) => items.iter().all(is_value_key_sorted),
        JsonValue::Object(map) => {
            map.keys().zip(map.keys().skip(1)).all(|(a, b)| a <= b)
                && map.values().all(is_value_key_sorted)
        }
        _ => true,
    }
}

/// `true` when `v` nests deeper than `max` container levels: the guard that
/// keeps the recursive walkers here and `Value`'s own `Drop` inside a bounded
/// frame count. The walk is iterative, so the check itself cannot overflow on
/// the adversarially deep input it exists to detect.
///
/// The unit is **container levels**, not nodes: only arrays/objects are charged
/// a level and a scalar leaf is never checked, so an empty container at level
/// `max + 1` is rejected exactly like a full one. `quillmark_core` re-exports
/// this as its own depth guard, so every boundary rejects the identical shape.
pub fn json_depth_exceeds(v: &JsonValue, max: usize) -> bool {
    // (value, depth) pairs; depth counts container levels entered.
    let mut stack: Vec<(&JsonValue, usize)> = vec![(v, 0)];
    while let Some((v, depth)) = stack.pop() {
        match v {
            JsonValue::Array(items) => {
                if depth + 1 > max {
                    return true;
                }
                stack.extend(items.iter().map(|c| (c, depth + 1)));
            }
            JsonValue::Object(map) => {
                if depth + 1 > max {
                    return true;
                }
                stack.extend(map.values().map(|c| (c, depth + 1)));
            }
            _ => {}
        }
    }
    false
}

/// [`json_depth_exceeds`] against [`MAX_JSON_DEPTH`](crate::MAX_JSON_DEPTH) as
/// an [`Invariant`] result, `what` naming the bag.
pub(crate) fn check_json_depth(v: &JsonValue, what: &'static str) -> Result<(), Invariant> {
    if json_depth_exceeds(v, crate::MAX_JSON_DEPTH) {
        return Err(Invariant::JsonTooDeep {
            what,
            max: crate::MAX_JSON_DEPTH,
        });
    }
    Ok(())
}

/// Put `v` in canonical key order, rebuilding it only when a key is out of
/// order, so an untouched tree pays the scan and skips the deep clone.
///
/// Both walks below recurse, where the container walks do not: a container path
/// is flat memory at any depth, so nothing but the walk bounds it, while a
/// [`JsonValue`] deep enough to overflow these overflows its own `Drop` in the
/// frame that built it. The bound belongs where such a value enters —
/// `bag_from_wire` before the decode clone, [`check_json_depth`] in
/// [`Content::validate`].
pub(crate) fn canonicalize_keys(v: &mut JsonValue) {
    if !is_value_key_sorted(v) {
        *v = sort_keys_owned(std::mem::take(v));
    }
}

/// Reorder every object's keys by **moving** each entry into a freshly
/// key-sorted map, recursively. Pins the canonical bytes against
/// `serde_json`'s `preserve_order` leaking insertion order; rebuilding the map
/// (rather than sorting in place) keeps that independent of whether the feature
/// is on in the crate graph.
pub(crate) fn sort_keys_owned(v: JsonValue) -> JsonValue {
    match v {
        JsonValue::Array(items) => {
            JsonValue::Array(items.into_iter().map(sort_keys_owned).collect())
        }
        JsonValue::Object(map) => {
            let mut entries: Vec<(String, JsonValue)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::with_capacity(entries.len());
            for (k, child) in entries {
                out.insert(k, sort_keys_owned(child));
            }
            JsonValue::Object(out)
        }
        other => other,
    }
}

/// What a [`Content`] can hold that [`Content::normalize`] cannot repair.
/// Returned by [`Content::validate`].
///
/// Each names a shape the model has no principled rewrite for: a forbidden
/// character with no substitute, two counts with no rule saying which is right,
/// a range or depth past a bound, an id whose collision only its author can
/// settle. What normalization *does* repair is not here — the mint establishes
/// it, and nothing re-checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invariant {
    /// `\r` in the text (line endings must be normalized to `\n`).
    CarriageReturn,
    /// A bidi formatting control in the text.
    BidiControl(char),
    /// A line separator in the text — VT, FF, NEL, U+2028 or U+2029 — which a
    /// downstream lexer reads as a line break.
    LineSeparator(char),
    /// `island_slot_count != islands.len()`.
    IslandSlotMismatch { slots: usize, islands: usize },
    /// `lines.len() != newline_segment_count`.
    LineCountMismatch { lines: usize, segments: usize },
    /// A mark range runs past the content or is inverted (`start > end`).
    /// Clamping would guess which end the author meant.
    MarkOutOfRange { start: Usv, end: Usv, len: Usv },
    /// A heading level outside 1..=6. No rewrite is principled: level 0 exports
    /// as bare text and level 7 as literal hashes, and a clamp guesses the
    /// other way. Both wires refuse it ahead of the model
    /// ([`ParseError::Shape`](crate::serial::ParseError::Shape)), so only a Rust
    /// caller spelling the level reaches this.
    BadHeadingLevel(u8),
    /// Two islands share an `id`. Uniqueness is the id invariant `validate`
    /// enforces; positional equality is not, since edits keep an island's id
    /// stable across renumbers.
    IslandIdCollision { id: String },
    /// Two prose anchors share an `id`, or one carries the empty id.
    /// `RemoveAnchor { id }` retains-out *every* match, so a shared id makes
    /// removing one destroy both. Scope is prose marks: cell anchors are outside
    /// the op surface.
    AnchorIdCollision { id: String },
    /// A line's container path is nested deeper than
    /// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH). The Typst emitter
    /// recurses one frame per container and refuses a deeper path rather than
    /// overflow the stack; markdown export walks an explicit stack and projects
    /// any depth.
    NestingTooDeep {
        line: usize,
        depth: usize,
        max: usize,
    },
    /// An opaque JSON payload (an island's `props`) nests deeper than
    /// [`MAX_JSON_DEPTH`](crate::MAX_JSON_DEPTH). `what` names the bag; no true
    /// depth is reported, since the check bails at the first over-deep
    /// container.
    JsonTooDeep { what: &'static str, max: usize },
}

/// Whether a line's text contradicts its `kind`, which [`Content::normalize`]
/// answers by demoting the line to [`LineKind::Para`].
///
/// `Para` and `Heading` carry arbitrary text including slots (an inline image is
/// a slot in a `Para`), so only the three kinds whose contract *names* their
/// content constrain it: an [`Island`](LineKind::Island) line is exactly one
/// [`ISLAND_SLOT`], a [`Rule`](LineKind::Rule) carries no text, and a
/// [`Code`](LineKind::Code) line carries no slot — a fence emits its text
/// verbatim, so a slot would land raw in the output and re-import as nothing,
/// taking the island with it.
pub(crate) fn line_kind_contradicts_text(kind: &LineKind, seg: &str) -> bool {
    match kind {
        LineKind::Island => {
            let mut chars = seg.chars();
            !matches!((chars.next(), chars.next()), (Some(ISLAND_SLOT), None))
        }
        LineKind::Rule => !seg.is_empty(),
        LineKind::Code { .. } => seg.contains(ISLAND_SLOT),
        _ => false,
    }
}

/// Whether `[start, end)` is a whole line of `chars`: a line boundary on each
/// side and nothing else between them. An empty range asks it of the position a
/// splice would fill.
pub(crate) fn is_whole_line(chars: &[char], start: Usv, end: Usv) -> bool {
    (start == 0 || chars.get(start - 1) == Some(&'\n')) && matches!(chars.get(end), None | Some('\n'))
}

/// Every block-only island's slot that shares its line with other content, in
/// text order: the single reading behind the authored lane's refusal and the
/// break [`Content::normalize`] performs, so the two cannot drift.
pub(crate) fn inline_block_islands<'a>(
    chars: &'a [char],
    islands: &'a [Island],
) -> impl Iterator<Item = Usv> + 'a {
    chars
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == ISLAND_SLOT)
        .zip(islands)
        .filter(|&((at, _), island)| {
            island.island_type.block_only() && !is_whole_line(chars, at, at + 1)
        })
        .map(|((at, _), _)| at)
}

/// One piece of a line [`Content::split_block_islands`] broke, covering `span`:
/// the slot's own piece is the island's line, the prose pieces keep the line's
/// role, and only the first can still continue the block above.
fn fragment_line(line: &Line, span: std::ops::Range<Usv>, breaks: &[Usv], first: bool) -> Line {
    if span.len() == 1 && breaks.binary_search(&span.start).is_ok() {
        return Line {
            kind: LineKind::Island,
            containers: line.containers.clone(),
            continues: false,
        };
    }
    Line {
        kind: line.kind.clone(),
        containers: line.containers.clone(),
        continues: first && line.continues,
    }
}

/// The [`LineKind`] a line whose sole content is one [`ISLAND_SLOT`] carries in
/// canonical form: [`LineKind::Island`] where markdown writes that island as a
/// block ([`IslandType::block_only`](crate::IslandType::block_only)),
/// [`LineKind::Para`] where it writes it inline. Both spell one markdown, so the
/// model keeps the one re-importing it yields, and [`Content::normalize`] writes
/// that one.
///
/// `None` leaves the stored kind standing, on the three counts the projection
/// settles nothing: a line holding more than the slot, a kind whose own contract
/// carries a slot ([`LineKind::Heading`]), and an island whose type projects no
/// kind back.
fn island_line_kind(kind: &LineKind, seg: &str, island: Option<&Island>) -> Option<LineKind> {
    if !matches!(kind, LineKind::Para | LineKind::Island) {
        return None;
    }
    let mut chars = seg.chars();
    if (chars.next(), chars.next()) != (Some(ISLAND_SLOT), None) {
        return None;
    }
    let known = island?.island_type;
    Some(if known.block_only() {
        LineKind::Island
    } else {
        LineKind::Para
    })
}

impl Content {
    /// The text and its per-line attributes; marks and islands start empty.
    ///
    /// Constructing neither normalizes nor checks: the canonical form is the
    /// caller's until [`into_normalized`](Self::into_normalized) runs, and
    /// [`validate`](Self::validate) reports what that cannot repair. The codecs
    /// ([`crate::import`], [`Content::from_canonical_json`]) do both.
    pub fn new(text: String, lines: Vec<Line>) -> Self {
        Content {
            text,
            lines,
            marks: Vec::new(),
            islands: Vec::new(),
        }
    }

    /// Normalize and seal. With [`Normalized::empty`], the only mint for
    /// [`Normalized`]; the codecs decode through here.
    pub fn into_normalized(mut self) -> Normalized {
        self.normalize();
        Normalized(self)
    }

    pub fn with_marks(mut self, marks: Vec<Mark>) -> Self {
        self.marks = marks;
        self
    }

    /// Set the islands, one per [`ISLAND_SLOT`] in slot order.
    pub fn with_islands(mut self, islands: Vec<Island>) -> Self {
        self.islands = islands;
        self
    }

    /// An empty content: one empty `Para` line, no marks, no islands.
    pub fn empty() -> Self {
        Content::new(String::new(), vec![Line::new(LineKind::Para)])
    }

    /// Total length in USV.
    pub fn len_usv(&self) -> Usv {
        self.text.chars().count()
    }

    /// Whether this content satisfies the `richtext(inline)` constraint: exactly
    /// one `Para` line, sitting in no container, with no islands.
    /// [`Content::empty`] is inline, so a blank inline field passes.
    pub fn is_inline(&self) -> bool {
        self.islands.is_empty()
            && self.lines.len() == 1
            && self.lines[0].kind == LineKind::Para
            && self.lines[0].containers.is_empty()
    }

    /// Whether this content satisfies the `plaintext` constraint: no marks, no
    /// islands, and every line a plain `Para` sitting in no container.
    /// `continues` is unconstrained. [`Content::empty`] is plain.
    ///
    /// The distinguishing property of plaintext over `richtext { marks: [] }` is
    /// the *literal* codec ([`crate::import::from_plaintext`]), not this
    /// predicate.
    pub fn is_plain(&self) -> bool {
        self.marks.is_empty()
            && self.islands.is_empty()
            && self
                .lines
                .iter()
                .all(|l| l.kind == LineKind::Para && l.containers.is_empty())
    }

    /// Whether the text is empty or whitespace-only. An [`ISLAND_SLOT`] is not
    /// whitespace, so an island-bearing content is never blank.
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// Number of `\n`-separated segments: the required `lines.len()`.
    pub fn segment_count(&self) -> usize {
        self.text.chars().filter(|c| *c == '\n').count() + 1
    }

    /// Normalize in place: canonicalize container `ordinal`/`instance`, break a
    /// line around a block-only island's slot, drop zero-width formatting, union
    /// same-kind formatting that is adjacent or overlapping, recursively
    /// key-sort island props, then sort marks canonically. Idempotent: the
    /// fixed point the canonical serialization commits to.
    pub fn normalize(&mut self) {
        canonicalize_containers(&mut self.lines);
        // A splice writes text, never kinds: typing into a table line leaves it
        // `Island` over prose, joining a fence to an image line leaves it `Code`
        // over a slot, and export reads the kind and not the text, so the
        // un-repaired line projects its content away. Demote to `Para`, which is
        // what re-importing the line's own markdown yields.
        let mut slot = 0usize;
        for (line, seg) in self.lines.iter_mut().zip(self.text.split('\n')) {
            if line_kind_contradicts_text(&line.kind, seg) {
                line.kind = LineKind::Para;
            }
            if let Some(kind) = island_line_kind(&line.kind, seg, self.islands.get(slot)) {
                line.kind = kind;
            }
            slot += seg.chars().filter(|&c| c == ISLAND_SLOT).count();
        }
        self.split_block_islands();
        // A `continues` flag under a block that cannot take one clears: nothing
        // precedes the first line, and below it a differing container path or a
        // one-line kind above, where export would drop the continuation's text.
        // `Join` across two paths, `SetKind` retagging the line above and
        // `SetContinues` itself all reach the shape. Read after the demotion
        // above, which settles what a spliced-over kind is.
        for i in 0..self.lines.len() {
            if self.lines[i].continues
                && (i == 0
                    || self.lines[i].containers != self.lines[i - 1].containers
                    || !self.lines[i - 1].kind.takes_continuations())
            {
                self.lines[i].continues = false;
            }
        }
        // A table island's props are repaired (padded to one column count, cell
        // `\n` rewritten to a space, cell marks canonicalized) before the key
        // sort, so equal cells serialize to equal bytes.
        for island in &mut self.islands {
            island.island_type.normalize_props(&mut island.props);
            canonicalize_keys(&mut island.props);
        }
        // A formatting mark's edges never sit on a line boundary: markdown can't
        // bold a `\n`, so two producers that disagree only about whether the
        // boundary is "inside" the mark must canonicalize to the same bounds.
        // Trim leading/trailing `\n` (interior boundaries are kept: a mark may
        // legitimately span lines). Zero-width results are dropped below.
        // Skip the full-text char collection when nothing needs trimming.
        if self.marks.iter().any(|m| m.kind.is_formatting()) {
            let chars: Vec<char> = self.text.chars().collect();
            for m in &mut self.marks {
                if m.kind.is_formatting() {
                    while m.start < m.end && chars.get(m.start) == Some(&'\n') {
                        m.start += 1;
                    }
                    while m.end > m.start && chars.get(m.end - 1) == Some(&'\n') {
                        m.end -= 1;
                    }
                }
            }
        }
        self.marks = normalize_marks(std::mem::take(&mut self.marks));
    }

    /// Give a block-only island's slot the line its markup needs: a `\n` on each
    /// side that has other content, so the prose around it becomes its own block
    /// and the slot is alone. Markdown writes a table as a block, and left
    /// inline it emits pipes into the middle of a paragraph, which re-imports as
    /// prose with the island gone.
    ///
    /// The lanes that *author* an island refuse the placement up front
    /// ([`ApplyError::BlockIslandNotAlone`](crate::ApplyError::BlockIslandNotAlone)),
    /// so what reaches here is a stored blob carrying the shape and an accepted
    /// `Join` that ran a slot back into its prose.
    fn split_block_islands(&mut self) {
        use crate::delta::{Delta, Op};

        if !self.islands.iter().any(|i| i.island_type.block_only())
            || self.lines.len() != self.segment_count()
        {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let breaks: Vec<Usv> = inline_block_islands(&chars, &self.islands).collect();
        if breaks.is_empty() {
            return;
        }
        // Where the `\n` goes, in the text's own coordinates, so every cut sits
        // strictly inside a line and the fragments below stay in step with it.
        let mut cuts: Vec<Usv> = Vec::with_capacity(breaks.len() * 2);
        for &at in &breaks {
            if at > 0 && chars[at - 1] != '\n' {
                cuts.push(at);
            }
            if chars.get(at + 1).is_some_and(|&c| c != '\n') {
                cuts.push(at + 1);
            }
        }
        cuts.dedup(); // two adjacent slots name the boundary between them twice

        let mut lines = Vec::with_capacity(self.lines.len() + cuts.len());
        let mut cut = cuts.iter().copied().peekable();
        let mut pos = 0usize;
        for (line, seg) in self.lines.iter().zip(self.text.split('\n')) {
            let end = pos + seg.chars().count();
            let mut start = pos;
            let mut first = true;
            while let Some(p) = cut.next_if(|&p| p < end) {
                lines.push(fragment_line(line, start..p, &breaks, first));
                (start, first) = (p, false);
            }
            lines.push(fragment_line(line, start..end, &breaks, first));
            pos = end + 1;
        }

        let mut text = String::with_capacity(self.text.len() + cuts.len());
        let mut at_cut = cuts.iter().copied().peekable();
        for (i, &c) in chars.iter().enumerate() {
            if at_cut.next_if_eq(&i).is_some() {
                text.push('\n');
            }
            text.push(c);
        }
        let mut ops = Vec::with_capacity(cuts.len() * 2);
        let mut last = 0usize;
        for &p in &cuts {
            ops.push(Op::Retain(p - last));
            ops.push(Op::Insert("\n".to_string()));
            last = p;
        }

        self.text = text;
        self.lines = lines;
        self.rebase_marks(&Delta { ops });
    }

    /// What the mint cannot repair. `Ok(())` on every content a codec or an
    /// accepted op hands out; a hand-built one can fail it.
    pub fn validate(&self) -> Result<(), Invariant> {
        let mut slots = 0usize;
        let mut newlines = 0usize;
        let mut len: Usv = 0;
        for c in self.text.chars() {
            if c == '\r' {
                return Err(Invariant::CarriageReturn);
            }
            if is_bidi_char(c) {
                return Err(Invariant::BidiControl(c));
            }
            if is_line_separator(c) {
                return Err(Invariant::LineSeparator(c));
            }
            if c == ISLAND_SLOT {
                slots += 1;
            }
            if c == '\n' {
                newlines += 1;
            }
            len += 1;
        }
        if slots != self.islands.len() {
            return Err(Invariant::IslandSlotMismatch {
                slots,
                islands: self.islands.len(),
            });
        }
        let segments = newlines + 1;
        if self.lines.len() != segments {
            return Err(Invariant::LineCountMismatch {
                lines: self.lines.len(),
                segments,
            });
        }
        // Anchor-id uniqueness is what `RemoveAnchor` presumes.
        let mut seen_anchor_ids = std::collections::HashSet::new();
        for m in &self.marks {
            if m.start > m.end || m.end > len {
                return Err(Invariant::MarkOutOfRange {
                    start: m.start,
                    end: m.end,
                    len,
                });
            }
            if let MarkKind::Anchor { id } = &m.kind
                && (id.is_empty() || !seen_anchor_ids.insert(id.as_str()))
            {
                return Err(Invariant::AnchorIdCollision { id: id.clone() });
            }
        }
        for (i, line) in self.lines.iter().enumerate() {
            match &line.kind {
                LineKind::Heading { level } if !(1..=6).contains(level) => {
                    return Err(Invariant::BadHeadingLevel(*level));
                }
                _ => {}
            }
            if line.containers.len() > crate::MAX_NESTING_DEPTH {
                return Err(Invariant::NestingTooDeep {
                    line: i,
                    depth: line.containers.len(),
                    max: crate::MAX_NESTING_DEPTH,
                });
            }
        }
        // Table-cell marks: the prose range rule again, but each mark is bounded
        // by its own cell's text length (in USV).
        let mut seen_ids = std::collections::HashSet::with_capacity(self.islands.len());
        for island in &self.islands {
            if !seen_ids.insert(island.id.as_str()) {
                return Err(Invariant::IslandIdCollision {
                    id: island.id.clone(),
                });
            }
            // Depth before any pass that walks `props`; a cell's own `attrs` is
            // a subtree, so this bounds the cell marks read below as well.
            check_json_depth(&island.props, "island props")?;
            for (text, marks) in island.island_type.cell_marks(&island.props) {
                let clen = text.chars().count();
                for m in &marks {
                    if m.start > m.end || m.end > clen {
                        return Err(Invariant::MarkOutOfRange {
                            start: m.start,
                            end: m.end,
                            len: clen,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// One open container run, while [`canonicalize_containers`] walks past it.
struct Run {
    /// The container **as stored** at the line that opened this run, which is
    /// what decides where the input's runs begin: a producer's own `instance`
    /// values separate its runs whatever they are, and only their canonical
    /// spelling is this pass's business. Cloned once per run, not per line.
    raw: Container,
    instance: u64,
    ordinal: u64,
    raw_ordinal: u64,
}

/// Canonicalize every container path: `instance` to the minimal discriminator
/// the adjacency needs, `ordinal` to a gapless 0-based index.
///
/// Both are derived from *run structure*, which the stored path already spells:
/// a run opens where the stored run key or the stored `instance` changes, and
/// within one list item `ordinal` repeating continues that item across its
/// paragraphs while any change opens the next. So `[5, 9]` and `[0, 1]` are the
/// same two items, `[3, 3, 7]` is two items the first of which spans two
/// paragraphs, and a producer's `instance: 7, 9` pair reads as the same two
/// runs as `0, 1`.
///
/// `instance` resets to 0 wherever the preceding sibling run could not weld
/// with this one anyway — a different container kind, an intervening block, a
/// fresh parent — so it stays 0 in every document that needs no discriminator.
fn canonicalize_containers(lines: &mut [Line]) {
    let mut state: Vec<Run> = Vec::new();
    for line in lines.iter_mut() {
        let depth_len = line.containers.len();
        // Once a depth opens a new run, every depth below it is under a fresh
        // parent, so nothing there can be continuing a run and nothing there
        // has an adjacent predecessor to be told apart from.
        let mut opened_above = false;
        for d in 0..depth_len {
            let here = &line.containers[d];
            let raw_ordinal = match here {
                Container::ListItem { ordinal, .. } => *ordinal,
                _ => 0,
            };
            let continues = !opened_above
                && state
                    .get(d)
                    .is_some_and(|r| r.raw.same_run(here) && r.raw.instance() == here.instance());
            if continues {
                let run = &mut state[d];
                if raw_ordinal != run.raw_ordinal {
                    run.ordinal += 1;
                    run.raw_ordinal = raw_ordinal;
                    // The run continues but the *item* changed, and an item is
                    // a parent: everything below is inside a different one, so
                    // it neither continues its predecessor nor has an adjacent
                    // sibling to be told apart from. Two inner lists under two
                    // outer items are two lists however alike they look.
                    state.truncate(d + 1);
                    opened_above = true;
                }
            } else {
                // The run being replaced is this one's adjacent predecessor,
                // and only then: a fresh parent above leaves none.
                let instance = match state.get(d) {
                    Some(prev) if !opened_above && prev.raw.same_weld(here) => 1 - prev.instance,
                    _ => 0,
                };
                let raw = here.clone();
                state.truncate(d);
                state.push(Run {
                    raw,
                    instance,
                    ordinal: 0,
                    raw_ordinal,
                });
                opened_above = true;
            }
            let (ordinal, instance) = (state[d].ordinal, state[d].instance);
            if let Container::ListItem { ordinal: o, .. } = &mut line.containers[d] {
                *o = ordinal;
            }
            line.containers[d].set_instance(instance);
        }
        state.truncate(depth_len);
    }
}

/// Apply the three merge rules and the canonical sort to a flat mark list:
/// same-kind formatting marks union when adjacent *or* overlapping, different
/// kinds overlap freely (never split into runs), and an identity mark never
/// merges. Zero-width formatting is dropped; zero-width anchors survive.
pub(crate) fn normalize_marks(marks: Vec<Mark>) -> Vec<Mark> {
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<(String, String), Vec<(Usv, Usv)>> = BTreeMap::new();
    let mut kind_of: BTreeMap<(String, String), MarkKind> = BTreeMap::new();
    let mut passthrough: Vec<Mark> = Vec::new();

    for m in marks {
        if m.kind.is_formatting() {
            if m.start >= m.end {
                continue; // drop zero-width / inverted formatting
            }
            let key = m.kind.sort_key();
            kind_of.entry(key.clone()).or_insert_with(|| m.kind.clone());
            groups.entry(key).or_default().push((m.start, m.end));
        } else {
            passthrough.push(m);
        }
    }

    let mut out: Vec<Mark> = Vec::new();
    for (key, mut ranges) in groups {
        ranges.sort_unstable();
        let kind = kind_of.remove(&key).expect("kind recorded with group");
        let mut cur = ranges[0];
        for &(s, e) in &ranges[1..] {
            if s <= cur.1 {
                // adjacent (s == cur.1) or overlapping: union
                cur.1 = cur.1.max(e);
            } else {
                out.push(Mark {
                    start: cur.0,
                    end: cur.1,
                    kind: kind.clone(),
                });
                cur = (s, e);
            }
        }
        out.push(Mark {
            start: cur.0,
            end: cur.1,
            kind,
        });
    }
    out.extend(passthrough);

    // Key cached per mark so `sort_key`'s allocation runs once each, not once
    // per comparison.
    out.sort_by_cached_key(|m| (m.start, m.end, m.kind.sort_key()));
    // Two marks equal in range, kind and attrs are one handle recorded twice:
    // redundant bytes, not two handles. The sort makes any such pair adjacent.
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(start: Usv, end: Usv, kind: MarkKind) -> Mark {
        Mark { start, end, kind }
    }


    #[test]
    fn is_blank_tracks_whitespace_and_islands() {
        assert!(Content::empty().is_blank());
        let mut ws = Content::empty();
        ws.text = "  \n\t ".to_string();
        ws.lines = vec![
            Line {
                kind: LineKind::Para,
                containers: Vec::new(),
                continues: false,
            },
            Line {
                kind: LineKind::Para,
                containers: Vec::new(),
                continues: false,
            },
        ];
        assert!(ws.is_blank(), "whitespace-only text is blank");

        let mut has_text = Content::empty();
        has_text.text = "x".to_string();
        assert!(!has_text.is_blank());

        let mut island_only = Content::empty();
        island_only.text = ISLAND_SLOT.to_string();
        assert!(!island_only.is_blank());
    }

    fn tagged(text: &str, kind: LineKind) -> Content {
        Content {
            text: text.to_string(),
            lines: vec![Line {
                kind,
                containers: Vec::new(),
                continues: false,
            }],
            marks: Vec::new(),
            islands: Vec::new(),
        }
    }

    /// Export trusts the kind and never re-reads the segment, so `Island` over
    /// prose would project to the island alone and `Rule` over prose to `---`,
    /// the text silently gone. The mint demotes to `Para`, which is what
    /// re-importing the line's own markdown yields.
    #[test]
    fn normalize_demotes_a_stranded_line_kind() {
        for (text, kind) in [
            ("typed into a table line", LineKind::Island),
            ("", LineKind::Island),
            ("text on a rule line", LineKind::Rule),
        ] {
            let mut rt = tagged(text, kind.clone());
            rt.normalize();
            assert_eq!(rt.lines[0].kind, LineKind::Para, "{text:?} as {kind:?}");
            assert_eq!(rt.validate(), Ok(()));
        }

        // `Para`/`Heading` carry slots, so only a fence, whose text is emitted
        // verbatim, strands one.
        let mut code = tagged(&format!("a{ISLAND_SLOT}b"), LineKind::Code { lang: None });
        code.islands = vec![Island {
            id: "isl-0".into(),
            island_type: IslandType::Image,
            props: serde_json::json!({"alt": "x", "url": "y.png"}),
            loss: Loss::Lossless,
        }];
        for (kind, settles_to) in [
            (LineKind::Code { lang: None }, LineKind::Para),
            (LineKind::Para, LineKind::Para),
            (LineKind::Heading { level: 1 }, LineKind::Heading { level: 1 }),
        ] {
            let mut rt = code.clone();
            rt.lines[0].kind = kind.clone();
            rt.normalize();
            assert_eq!(rt.lines[0].kind, settles_to, "a slot under {kind:?}");
            assert_eq!(rt.validate(), Ok(()));
        }

        // A well-formed island line — a block island's slot alone — is left alone.
        let mut rt = tagged(&ISLAND_SLOT.to_string(), LineKind::Island);
        rt.islands = vec![table_island()];
        rt.normalize();
        assert_eq!(rt.lines[0].kind, LineKind::Island);
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(tagged("", LineKind::Rule).validate(), Ok(()));
    }

    /// A one-cell table: the island type markdown writes as a block.
    fn table_island() -> Island {
        Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
            "aligns": ["none"],
            "header": [{"marks": [], "text": "h"}],
            "rows": [[{"marks": [], "text": "c"}]],
        }))
    }

    /// Markdown spells a slot-alone `Para` line and a slot-alone `Island` line
    /// alike, so which one a document holds is the island type's to settle. Both
    /// spellings converge on the one the round trip yields, so no document holds
    /// a kind its own markdown denies.
    #[test]
    fn an_island_alone_on_a_line_takes_the_kind_its_type_projects() {
        let image = Island::new("isl-0".into(), IslandType::Image)
            .with_props(serde_json::json!({"alt": "a", "url": "u"}));
        for (island, canonical) in [
            (table_island(), LineKind::Island),
            (image, LineKind::Para),
        ] {
            for stored in [LineKind::Para, LineKind::Island] {
                let what = format!("{} as {stored:?}", island.island_type.as_str());
                let rt = tagged(&ISLAND_SLOT.to_string(), stored)
                    .with_islands(vec![island.clone()])
                    .into_normalized();
                assert_eq!(rt.validate(), Ok(()), "{what}");
                assert_eq!(rt.lines[0].kind, canonical, "{what}");
                let md = crate::export::to_markdown(&rt);
                assert_eq!(
                    crate::import::from_markdown(&md).expect("re-imports"),
                    rt,
                    "{what} is not a fixed point: {md:?}"
                );
            }
        }
    }

    #[test]
    fn container_nesting_is_capped() {
        let mut rt = tagged("hi", LineKind::Para);
        rt.lines[0].containers = vec![Container::Quote { instance: 0 }; crate::MAX_NESTING_DEPTH];
        assert_eq!(rt.validate(), Ok(()));
        rt.lines[0].containers.push(Container::Quote { instance: 0 });
        assert_eq!(
            rt.validate(),
            Err(Invariant::NestingTooDeep {
                line: 0,
                depth: crate::MAX_NESTING_DEPTH + 1,
                max: crate::MAX_NESTING_DEPTH,
            })
        );
    }

    #[test]
    fn json_payload_depth_is_capped() {
        let nested = |depth: usize| {
            let mut v = JsonValue::Null;
            for _ in 0..depth {
                v = JsonValue::Array(vec![v]);
            }
            v
        };
        let too_deep = |what: &'static str| {
            Err(Invariant::JsonTooDeep {
                what,
                max: crate::MAX_JSON_DEPTH,
            })
        };

        let mut rt = tagged("\u{fffc}", LineKind::Island);
        rt.islands = vec![Island {
            id: "i1".into(),
            island_type: IslandType::Image,
            props: nested(crate::MAX_JSON_DEPTH + 1),
            loss: Loss::Lossless,
        }];
        assert_eq!(rt.validate(), too_deep("island props"));
    }

    #[test]
    fn same_kind_adjacent_unions() {
        let got = normalize_marks(vec![f(3, 6, MarkKind::Strong), f(0, 3, MarkKind::Strong)]);
        assert_eq!(got, vec![f(0, 6, MarkKind::Strong)]);
    }

    #[test]
    fn same_kind_overlapping_unions() {
        let got = normalize_marks(vec![f(0, 4, MarkKind::Emph), f(2, 7, MarkKind::Emph)]);
        assert_eq!(got, vec![f(0, 7, MarkKind::Emph)]);
    }

    #[test]
    fn different_kinds_overlap_freely() {
        let got = normalize_marks(vec![f(0, 5, MarkKind::Strong), f(2, 7, MarkKind::Emph)]);
        assert_eq!(
            got,
            vec![f(0, 5, MarkKind::Strong), f(2, 7, MarkKind::Emph)]
        );
    }

    #[test]
    fn links_union_only_at_same_url() {
        let a = MarkKind::Link { url: "a".into() };
        let b = MarkKind::Link { url: "b".into() };
        let got = normalize_marks(vec![
            f(0, 2, a.clone()),
            f(2, 4, a.clone()),
            f(4, 6, b.clone()),
        ]);
        assert_eq!(got, vec![f(0, 4, a), f(4, 6, b)]);
    }

    #[test]
    fn identity_never_merges() {
        let a = MarkKind::Anchor { id: "c1".into() };
        let b = MarkKind::Anchor { id: "c2".into() };
        let got = normalize_marks(vec![f(3, 3, a.clone()), f(3, 3, b.clone())]);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&f(3, 3, a)));
        assert!(got.contains(&f(3, 3, b)));
    }

    #[test]
    fn zero_width_formatting_dropped_zero_width_anchor_kept() {
        let got = normalize_marks(vec![
            f(2, 2, MarkKind::Strong),
            f(2, 2, MarkKind::Anchor { id: "x".into() }),
        ]);
        assert_eq!(got, vec![f(2, 2, MarkKind::Anchor { id: "x".into() })]);
    }

    #[test]
    fn is_inline_accepts_empty_and_single_para() {
        assert!(Content::empty().is_inline());
        assert!(crate::import::from_markdown("just one line")
            .unwrap()
            .is_inline());
        assert!(crate::import::from_markdown("a *bold* run")
            .unwrap()
            .is_inline());
    }

    #[test]
    fn is_inline_rejects_blocks_containers_and_islands() {
        assert!(!crate::import::from_markdown("one\n\ntwo")
            .unwrap()
            .is_inline());
        assert!(!crate::import::from_markdown("# heading")
            .unwrap()
            .is_inline());
        assert!(!crate::import::from_markdown("- item").unwrap().is_inline());
    }

    #[test]
    fn validate_catches_slot_mismatch() {
        let mut rt = Content::empty();
        rt.text = "\u{FFFC}".into();
        rt.lines = vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        assert_eq!(
            rt.validate(),
            Err(Invariant::IslandSlotMismatch {
                slots: 1,
                islands: 0
            })
        );
    }

    #[test]
    fn validate_catches_line_count() {
        let mut rt = Content::empty();
        rt.text = "a\nb".into(); // 2 segments, but 1 line
        assert_eq!(
            rt.validate(),
            Err(Invariant::LineCountMismatch {
                lines: 1,
                segments: 2
            })
        );
    }

    /// A within-block break lives inside one container. `Join` mints the
    /// crossing shape by merging two lines of differing paths, which leaves the
    /// *next* line continuing across the seam; `normalize` clears it.
    #[test]
    fn continues_across_a_container_boundary_is_cleared() {
        let mut rt = Content::new(
            "a\nb".to_string(),
            vec![
                Line::new(LineKind::Para),
                Line::new(LineKind::Para)
                    .with_containers(vec![Container::Quote { instance: 0 }])
                    .with_continues(true),
            ],
        );
        rt.normalize();
        assert!(!rt.lines[1].continues, "normalize clears it");
        assert_eq!(rt.validate(), Ok(()));

        // Equal-length but different containers is the same crossing: two list
        // items are two blocks, and a hard break does not span them.
        let li = |ordinal| {
            vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal,
                instance: 0,
            }]
        };
        let mut rt = Content::new(
            "a\nb".to_string(),
            vec![
                Line::new(LineKind::Para).with_containers(li(0)),
                Line::new(LineKind::Para)
                    .with_containers(li(1))
                    .with_continues(true),
            ],
        );
        rt.normalize();
        assert!(!rt.lines[1].continues);

        // The within-container break is untouched: same path, flag kept.
        let mut rt = Content::new(
            "a\nb".to_string(),
            vec![
                Line::new(LineKind::Para).with_containers(li(0)),
                Line::new(LineKind::Para)
                    .with_containers(li(0))
                    .with_continues(true),
            ],
        );
        rt.normalize();
        assert!(rt.lines[1].continues, "a hard break inside one item survives");
        assert_eq!(rt.validate(), Ok(()));
    }

    /// A heading, an island and a rule render as their own line alone, so a
    /// `continues` line after one is text no projection reaches. `SetKind`
    /// mints the shape by retagging the line a continuation already follows;
    /// `normalize` clears the flag.
    #[test]
    fn continues_after_a_single_line_block_is_cleared() {
        let cases = [
            (LineKind::Heading { level: 1 }, "a\nb", "# a\n\nb"),
            (LineKind::Island, "\u{FFFC}\nb", "| h |\n| --- |\n| c |\n\nb"),
            (LineKind::Rule, "\nb", "***\n\nb"),
        ];
        for (kind, text, markdown) in cases {
            let mut rt = Content::new(
                text.to_string(),
                vec![
                    Line::new(kind.clone()),
                    Line::new(LineKind::Para).with_continues(true),
                ],
            )
            .with_islands(match kind {
                LineKind::Island => vec![Island::new("isl-0".into(), IslandType::Table)
                    .with_props(serde_json::json!({
                        "header": [{"text": "h", "marks": []}],
                        "rows": [[{"text": "c", "marks": []}]],
                        "aligns": ["none"],
                    }))],
                _ => vec![],
            });
            rt.normalize();
            assert!(!rt.lines[1].continues, "normalize clears it");
            assert_eq!(rt.validate(), Ok(()));
            assert_eq!(
                crate::export::to_markdown(&rt.into_normalized()),
                markdown,
                "the continuation projects as the paragraph it is"
            );
        }
    }

    #[test]
    fn normalize_is_idempotent() {
        let mut rt = Content::empty();
        rt.text = "hello world".into();
        rt.marks = vec![
            f(6, 11, MarkKind::Strong),
            f(0, 5, MarkKind::Strong),
            f(0, 5, MarkKind::Emph),
        ];
        rt.normalize();
        let once = rt.marks.clone();
        rt.normalize();
        assert_eq!(rt.marks, once);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn table_cell_marks_normalize_and_are_idempotent() {
        fn table(cell_marks: serde_json::Value) -> Content {
            let mut rt = Content::empty();
            rt.text = ISLAND_SLOT.to_string();
            rt.lines = vec![Line {
                kind: LineKind::Island,
                containers: vec![],
                continues: false,
            }];
            rt.islands = vec![Island {
                id: "i".into(),
                island_type: IslandType::Table,
                props: serde_json::json!({
                    "aligns": ["none"],
                    "header": [{"text": "abcd", "marks": cell_marks}],
                    "rows": [],
                }),
                loss: Loss::Lossless,
            }];
            rt
        }
        let mut a = table(serde_json::json!([
            {"start": 2, "end": 4, "type": "strong"},
            {"start": 1, "end": 1, "type": "strong"},
            {"start": 0, "end": 2, "type": "strong"}
        ]));
        a.normalize();
        assert_eq!(a.validate(), Ok(()));
        let cell = &a.islands[0].props["header"][0];
        assert_eq!(cell["marks"].as_array().unwrap().len(), 1);
        assert_eq!(cell["marks"][0]["start"], 0);
        assert_eq!(cell["marks"][0]["end"], 4);
        let mut b = table(serde_json::json!([
            {"start": 0, "end": 2, "type": "strong"},
            {"start": 2, "end": 4, "type": "strong"}
        ]));
        b.normalize();
        let canon = |rt: &Content| rt.clone().into_normalized().to_canonical_json();
        assert_eq!(canon(&a), canon(&b));
        let once = canon(&a);
        a.normalize();
        assert_eq!(canon(&a), once);
    }

    /// A cell is canonicalized in place, so a key this build does not recognize
    /// survives. Two columns with one body cell, so `pad_row` mints the second
    /// and the pass covers both a carried cell and a synthesized one.
    #[test]
    fn unrecognized_cell_key_survives_normalize() {
        let mut rt = table_rt(serde_json::json!({
            "aligns": ["none", "none"],
            "header": [{"text": "h", "marks": [], "colspan": 2}, cell("h2")],
            "rows": [[cell("a")]],
        }));
        rt.normalize();
        assert_eq!(rt.islands[0].props["header"][0]["colspan"], 2);
        assert!(rt.islands[0].props["rows"][0][1].get("colspan").is_none());
        assert!(rt
            .into_normalized()
            .to_canonical_json()
            .contains(r#""colspan":2"#));
    }

    #[test]
    fn validate_catches_cell_mark_out_of_range() {
        let mut rt = Content::empty();
        rt.text = ISLAND_SLOT.to_string();
        rt.lines = vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        rt.islands = vec![Island {
            id: "i".into(),
            island_type: IslandType::Table,
            props: serde_json::json!({
                "aligns": ["none"],
                // "ab" is 2 USV; a mark ending at 5 runs past the cell.
                "header": [{"text": "ab", "marks": [{"start": 0, "end": 5, "type": "strong"}]}],
                "rows": [],
            }),
            loss: Loss::Lossless,
        }];
        assert_eq!(
            rt.validate(),
            Err(Invariant::MarkOutOfRange {
                start: 0,
                end: 5,
                len: 2
            })
        );
    }

    fn table_rt(props: serde_json::Value) -> Content {
        let mut rt = Content::empty();
        rt.text = ISLAND_SLOT.to_string();
        rt.lines = vec![Line {
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        rt.islands = vec![Island {
            id: "i".into(),
            island_type: IslandType::Table,
            props,
            loss: Loss::Lossless,
        }];
        rt
    }

    fn cell(t: &str) -> serde_json::Value {
        serde_json::json!({ "text": t, "marks": [] })
    }

    /// The widest row (3) drives the header width, so the markdown
    /// (header-derived) and Typst (widest-row) projections agree.
    #[test]
    fn normalize_repairs_table_shape() {
        let mut rt = table_rt(serde_json::json!({
            "aligns": ["none"],
            "header": [cell("h")],
            "rows": [
                [cell("a"), cell("b"), cell("c")],
                [cell("d\ne")],
            ],
        }));
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()));

        let props = &rt.islands[0].props;
        assert_eq!(props["header"].as_array().unwrap().len(), 3);
        assert_eq!(props["aligns"].as_array().unwrap().len(), 3);
        for row in props["rows"].as_array().unwrap() {
            assert_eq!(row.as_array().unwrap().len(), 3);
        }
        assert_eq!(props["aligns"][2], serde_json::json!("none"));
        assert_eq!(props["header"][1]["text"], serde_json::json!(""));
        assert_eq!(props["rows"][1][0]["text"], serde_json::json!("d e"));

        let canon = |rt: &Content| rt.clone().into_normalized().to_canonical_json();
        let once = canon(&rt);
        rt.normalize();
        assert_eq!(canon(&rt), once);
    }

    #[test]
    fn empty_table_is_valid() {
        let mut rt = table_rt(serde_json::json!({
            "aligns": [],
            "header": [],
            "rows": [],
        }));
        assert_eq!(rt.validate(), Ok(()));
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn non_array_table_header_is_repaired() {
        let mut rt = table_rt(serde_json::json!({
            "header": "oops",
            "aligns": [],
            "rows": [],
        }));
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(rt.islands[0].props["header"], serde_json::json!([]));
    }

    #[test]
    fn duplicate_island_id_is_rejected() {
        let mut rt = Content::empty();
        rt.text = format!("{ISLAND_SLOT}\n{ISLAND_SLOT}");
        rt.lines = vec![
            Line {
                kind: LineKind::Island,
                containers: vec![],
                continues: false,
            },
            Line {
                kind: LineKind::Island,
                containers: vec![],
                continues: false,
            },
        ];
        let table = |id: &str| Island {
            id: id.into(),
            island_type: IslandType::Table,
            props: serde_json::json!({ "header": [cell("h")], "aligns": ["none"], "rows": [] }),
            loss: Loss::Lossless,
        };
        rt.islands = vec![table("dup"), table("dup")];
        assert_eq!(
            rt.validate(),
            Err(Invariant::IslandIdCollision { id: "dup".into() })
        );
        rt.islands = vec![table("a"), table("b")];
        assert_eq!(rt.validate(), Ok(()));
    }

    /// Byte-identical anchors `normalize` already dedupes; this is the
    /// surviving collision.
    #[test]
    fn duplicate_or_empty_anchor_id_is_rejected() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        let anchor = |start, end, id: &str| Mark {
            start,
            end,
            kind: MarkKind::Anchor { id: id.into() },
        };
        rt.marks = vec![anchor(0, 2, "x"), anchor(2, 4, "x")];
        assert_eq!(
            rt.validate(),
            Err(Invariant::AnchorIdCollision { id: "x".into() })
        );
        rt.marks = vec![anchor(0, 2, "x"), anchor(2, 4, "y")];
        assert_eq!(rt.validate(), Ok(()));
        rt.marks = vec![anchor(0, 2, "")];
        assert_eq!(
            rt.validate(),
            Err(Invariant::AnchorIdCollision { id: String::new() })
        );
    }

    #[test]
    fn normalize_dedupes_identical_identity_marks() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        let anchor = |id: &str| Mark {
            start: 0,
            end: 4,
            kind: MarkKind::Anchor { id: id.into() },
        };
        rt.marks = vec![anchor("x"), anchor("x")];
        rt.normalize();
        assert_eq!(rt.marks, vec![anchor("x")]);
        rt.marks = vec![anchor("x"), anchor("y")];
        rt.normalize();
        assert_eq!(rt.marks.len(), 2);
    }
}
