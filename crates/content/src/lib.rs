//! `Content`: the canonical content model for Quillmark.
//!
//! One [`Content`] per content field: a single text sequence carrying line
//! attributes, anchored marks, and embedded islands, over one coordinate space
//! of Unicode scalar values. Markdown is a *projection*
//! ([`import::from_markdown`], [`export::to_markdown`]), so every edit is a
//! splice and all structure moves with it. [`serial`] is the canonical,
//! byte-deterministic JSON, in a storage form and a seam form differing only in
//! whether a zero `Container::instance` is spelled; [`delta`] and [`ops`] are
//! the per-field edit surface.

pub mod delta;
pub mod export;
pub mod import;
pub mod island;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod serial;
pub mod traverse;
pub mod usv;

pub use delta::{diff_import, Assoc, Delta, Op};
pub use export::{to_markdown, to_plaintext};
pub use import::{from_markdown, from_plaintext};
pub use island::IslandType;
pub use model::{
    Container, Invariant, Island, Line, LineKind, LineKindMismatch, Loss, Mark, MarkKind,
    Content, Usv,
};
pub use ops::{
    change_bundle_from_value, island_op_from_value, line_op_from_value, mark_op_from_value,
    ApplyError, ChangeBundle, IslandOp, LineOp, MarkOp,
};
pub use serial::ParseError;
pub use model::Normalized;
pub use traverse::{items, runs, segment, Span};

/// Maximum container nesting depth the markdown codecs accept before erroring.
/// The import guard ([`import::from_markdown`]) and the typst backend's markup
/// converter share this one limit, so a document that imports also renders.
pub const MAX_NESTING_DEPTH: usize = 100;

/// Maximum nesting depth of an opaque JSON payload (an island's `props`, an
/// unknown line/container/mark's `attrs`), in container levels from the bag.
///
/// The recursive consumers (key sorting, `serde_json::Value`'s own `Drop`) spend
/// a frame per level, so an unbounded bag overflows the stack: on wasm32 an
/// unrecoverable trap, not a catchable error. Matches `serde_json::from_str`'s
/// own limit, so nothing a stored blob can carry is refused.
pub const MAX_JSON_DEPTH: usize = 128;
