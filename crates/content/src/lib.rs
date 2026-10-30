//! `Content`: the canonical content model for Quillmark.
//!
//! One [`model::Content`] per content field: a single text sequence carrying
//! line attributes, anchored marks, and embedded islands, over one coordinate
//! space of Unicode scalar values. Markdown is a *projection*
//! ([`import::from_markdown`], [`export::to_markdown`]), so every edit is a
//! splice and all structure moves with it. [`serial`] is the canonical,
//! byte-deterministic JSON that storage, the render seam and the binding seam
//! all carry; [`delta`] and [`ops`] are the per-field edit surface.

pub mod anchor;
pub mod carrier;
pub mod delta;
pub mod export;
mod html;
mod html_table;
pub mod import;
pub mod island;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod serial;
pub mod traverse;
pub mod usv;

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`import::from_markdown`]) and the typst backend's markup
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
