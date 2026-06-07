//! # Orchestration
//!
//! Orchestrates the Quillmark engine and the portable [`Quill`] type.
//!
//! ## Usage
//!
//! 1. Load a quill with [`Quill::from_tree`] or [`Quill::from_path`] (no engine
//!    required — a `Quill` is engine-free, validated data)
//! 2. Create an engine with [`Quillmark::new`]
//! 3. Render documents via [`Quillmark::render`] or [`Quillmark::open`]

mod engine;
mod quill;

pub use engine::Quillmark;
pub use quill::Quill;
