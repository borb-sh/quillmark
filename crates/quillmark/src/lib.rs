//! # Quillmark
//!
//! Quillmark is a schema-driven document engine that turns Markdown
//! with card-yaml metadata blocks into a fully typeset document (PDF, SVG, PNG).
//!
//! Markdown enters through the bound door: `Quill::parse` conforms the
//! document against the quill that will render it.
//!
//! ```no_run
//! use quillmark::{quill_from_path, CalendarDate, OutputFormat, Quillmark, RenderOptions};
//!
//! let quill = quill_from_path("path/to/quill").unwrap();
//! let engine = Quillmark::new();
//! let today = CalendarDate::new(2026, 3, 14).unwrap(); // the host's local date
//!
//! let doc = quill.parse("~~~\n$quill: my_quill\n$kind: main\ntitle: Hello\n~~~\n\n# Hello World").unwrap().document;
//! let result = engine.render(&quill, &doc, today, &RenderOptions::default().with_output_format(OutputFormat::Pdf)).unwrap();
//! ```
//!
//! Or no Markdown at all: a blank canvas and the schema-bound writer.
//!
//! ```no_run
//! use quillmark::{quill_from_path, Document};
//!
//! let quill = quill_from_path("path/to/quill").unwrap();
//! let mut doc = Document::new("my_quill".parse().unwrap());
//!
//! let mut writer = quill.writer(&mut doc);
//! writer.set("title", "Hello").unwrap();
//! ```

// A verb's return type belongs here whenever the verb does. `quillmark-cli`
// compiles against this crate alone, so a re-export dropped here breaks its
// build; `tests/facade_surface.rs` covers the lanes the CLI does not reach.
// The python and wasm bindings sit lower, on core's wire and addressing seam,
// and name `quillmark-core` directly.
pub use quillmark_core::{
    backend::Backend,
    document::{Card, Document, EditError, ImportError, Parsed},
    error::{Diagnostic, Location, ParseError, RenderError, RenderResult, Severity},
    quill::{
        BoundParseError, CalendarDate, CardSchema, FieldSchema, FieldType, FileTreeNode,
        ParseDateError, Quill, QuillConfig, QuillIgnore, ValidationError,
    },
    reader::{CardReader, TypedReader},
    region::{ContentHit, HitGranularity, RenderedRegion},
    session::{ChangeBundle, ChangeSet, Delta, LiveSession},
    types::{Artifact, OutputFormat, RenderOptions},
    value::{PathSegment, QuillValue},
    version::QuillReference,
    writer::TypedWriter,
    Content, Normalized,
};

mod load;
mod orchestration;

pub use load::{quill_from_path, tree_from_path};
pub use orchestration::Quillmark;

#[cfg(feature = "typst")]
pub use quillmark_typst::workspace as typst_workspace;
