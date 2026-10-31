//! `Content`: the canonical content model for Quillmark.
//!
//! One [`model::Content`] per content field: flat text whose `\n` ends a
//! block, blocks carrying their kind and container path, marks, anchors, and
//! islands, all over one coordinate space of Unicode scalar values. [`wire`] is
//! the canonical, byte-deterministic JSON storage and the bindings carry.
//! [`markdown`] is the projection at the edges, read and written through the
//! line form in [`lines`], which the renderers also read; [`migrate`] moves
//! values between the two.

pub mod change;
pub mod diff;
pub mod ids;
pub mod lines;
pub mod markdown;
pub mod migrate;
pub mod model;
pub mod table;
pub mod units;
pub mod usv;
pub mod wire;

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`markdown::from_markdown`]) and the typst backend's markup
/// converter share this one limit, so a document that imports also renders.
/// Typst stops at a show-rule depth of 64, a block quote spends four of it and
/// the marks and font run around a run at most seven, so the bound leaves a
/// plate's own rules the rest.
pub const MAX_NESTING_DEPTH: usize = 12;

/// Maximum slots of a table's grid, rows (the header among them) times columns,
/// and on a markdown import of all its tables' grids together. A grid pads
/// every row to its widest, so a few bytes of markdown or host JSON can ask for
/// millions of cells: on wasm32 an allocation that traps rather than errors. No
/// document's tables near it, and Typst could not lay out one that did.
pub const MAX_TABLE_CELLS: usize = 100_000;

/// Maximum nesting depth of an opaque JSON payload (an island's `props`), in
/// container levels from the bag.
///
/// The recursive consumers (key sorting, `serde_json::Value`'s own `Drop`) spend
/// a frame per level, so an unbounded bag overflows the stack: on wasm32 an
/// unrecoverable trap, not a catchable error. Matches `serde_json::from_str`'s
/// own limit, so nothing a stored blob can carry is refused.
pub const MAX_JSON_DEPTH: usize = 128;
