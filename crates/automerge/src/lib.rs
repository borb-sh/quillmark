//! A Quillmark document held as one Automerge document.
//!
//! [`content`] is the value a body or a rich field holds: its canonical JSON,
//! the changes that edit it, its repair and its markdown and plain codecs.
//!
//! [`model::Doc`] is the document every `Document` is held in: its bytes
//! ([`save`](model::Doc::save), [`load`](model::Doc::load)), its revision
//! ([`heads`](model::Doc::heads)), the changes past it
//! ([`save_since`](model::Doc::save_since), [`merge`](model::Doc::merge)) and
//! Automerge's sync protocol over it, with a change lowered into it at its
//! heads or at earlier ones and each value read back through repair.
//!
//! [`merge3`] merges two changes made from one value as a write at earlier
//! heads merges with the writes since, and [`revise_keeping`] is the markdown
//! revise that keeps what its read does not spell by merging the writer's
//! revision with what the read lost.

pub mod content;
mod merge;
pub mod model;

pub use automerge;
pub use merge::{merge3, revise, revise_keeping};
