//! # Quillmark Core
//!
//! Foundational types for the Quillmark document engine: the
//! [`document::Document`] model and its `~~~` card-yaml blocks, the
//! [`quill::Quill`] format bundle, the [`backend::Backend`] seam output
//! backends implement, and structured diagnostics carrying source locations.
//!
//! The markdown grammar is specified in
//! [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md).
//!
//! ```no_run
//! use quillmark_core::document::Document;
//!
//! let markdown = "~~~\n$quill: my_quill\n$kind: main\ntitle: Example\n~~~\n\n# Content";
//! let doc = Document::parse(markdown).unwrap().document;
//! let title = doc.main()
//!     .payload()
//!     .get("title")
//!     .and_then(|v| v.as_str())
//!     .unwrap_or("Untitled");
//! assert_eq!(title, "Example");
//! ```

pub mod backend;
pub mod document;
pub mod error;
pub mod normalize;
pub mod path;
pub mod quill;
pub mod reader;
pub mod region;
pub mod session;
pub mod types;
pub mod value;
pub mod version;
pub mod writer;

/// The canonical content model, the pre-built value the document mutators
/// accept, and the canonical-form token every content read answers in.
pub use quillmark_content::model::{Content, Normalized};
