//! `Content`: the canonical content model for Quillmark.
//!
//! One [`model::Content`] per content field: flat text a `\n` breaks into
//! blocks, blocks carrying their kind and container path, marks, anchors, and
//! islands, all over one coordinate space of Unicode scalar values. [`wire`] is
//! the canonical, byte-deterministic JSON storage and the bindings carry.
//! [`markdown`] is the projection at the edges, one import and one export
//! per codec.
//! [`change`] is the one way a value is edited, [`anchors`] reads each
//! anchor's context and re-attaches a dropped one, [`carry`] carries a
//! value's anchors and island ids onto its markdown rewrite across a
//! [`delta`], [`mapping`] maps positions across changes, [`structural`] is
//! the change of each structural edit, [`mint`] fills the ids a change leaves
//! to its write, and [`repair`] makes valid a state two concurrent edits
//! merged into.
//!
//! The line form, `lines` and the `migrate` between it and the model, is what
//! the markdown import walks and the shape a v1 stored value decodes from; it
//! is hidden from the documented surface and no public API takes or answers
//! it. The markdown export and the renderers read the model through [`view`].

pub mod anchors;
pub mod blocks;
pub mod carry;
pub mod change;
pub mod delta;
pub mod diff;
mod draft;
mod export;
pub mod ids;
#[doc(hidden)]
pub mod lines;
pub mod mapping;
pub mod markdown;
#[doc(hidden)]
pub mod migrate;
pub mod mint;
pub mod model;
pub mod repair;
pub mod structural;
pub mod table;
pub mod units;
pub mod view;
pub mod wire;

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`markdown::from_markdown`]) and the typst backend's markup
/// converter share this one limit, so a document that imports also renders.
/// Typst stops at a show-rule depth of 64, a block quote spends four of it and
/// the marks and font run around a run at most seven, so the bound leaves a
/// plate's own rules the rest.
pub const MAX_NESTING_DEPTH: usize = 12;

/// Maximum slots of a table's grid, rows (the header among them) times columns,
/// and on a markdown import of all its tables' grids together, a revise's past
/// it where its stored tables hold more. A grid pads every row to its widest,
/// so a few bytes of markdown or host JSON can ask for millions of cells: on
/// wasm32 an allocation that traps rather than errors. No document's tables
/// near it, and Typst could not lay out one that did.
pub const MAX_TABLE_CELLS: usize = 100_000;

/// Maximum bytes of markdown one import reads, the bound a document's parse
/// holds its whole input to. Every markdown import, a revise's included, refuses
/// past it, so a field cannot take through a revise what a document could not
/// through a parse.
pub const MAX_MARKDOWN_SIZE: usize = 10 * 1024 * 1024;

/// Characters a markdown import may copy from footnote notes into a reference
/// past the first reading each, the notes of a document sharing one budget. A
/// reference copies its whole note, so a few bytes citing a long note many
/// times ask for the note times the references; past the budget a reference
/// stays as its text. A note read once costs nothing of it.
pub const MAX_NOTE_COPIES: usize = 1_000_000;

/// Marks one text, a field's or a slot's, may hold: [`MARKS_BASE`] and
/// [`MARKS_PER_CHAR`] per character of it ([`max_marks`]). A mark of a type
/// this build does not know unions only with one of its own attributes, and
/// one over several blocks is cut at each, so a few bytes of host JSON can ask
/// for marks times blocks of them: on wasm32 an allocation that traps rather
/// than errors. Tied to the text, the bound keeps what a value holds linear in
/// what it was sent; a real document holds well under one mark per character.
pub const MARKS_PER_CHAR: usize = 4;

/// The marks a text holds beyond [`MARKS_PER_CHAR`] per character, so a short
/// one, a table cell's, may carry every mark at once.
pub const MARKS_BASE: usize = 64;

/// The most marks a text of `len` characters holds.
pub const fn max_marks(len: usize) -> usize {
    MARKS_BASE.saturating_add(MARKS_PER_CHAR.saturating_mul(len))
}

/// Maximum anchors a value holds, its text's and its slots' together. A
/// comment thread is far fewer; an anchor step adds at most one.
pub const MAX_ANCHORS: usize = 100_000;

/// Maximum nesting depth of an opaque JSON payload (an island's `props`), in
/// container levels from the bag.
///
/// The recursive consumers (key sorting, `serde_json::Value`'s own `Drop`) spend
/// a frame per level, so an unbounded bag overflows the stack: on wasm32 an
/// unrecoverable trap, not a catchable error. Matches `serde_json::from_str`'s
/// own limit, so nothing a stored blob can carry is refused.
pub const MAX_JSON_DEPTH: usize = 128;

pub use lines::model::json_depth_exceeds;
