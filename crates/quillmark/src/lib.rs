//! # Quillmark
//!
//! Quillmark is a schema-driven document framework that turns Markdown
//! with card-yaml metadata blocks into a fully typeset document (PDF, SVG, PNG).
//!
//! `Quill::parse` reads Markdown and conforms it to the quill's schema.
//!
//! ```no_run
//! use quillmark::{quill_from_path, CalendarDate, Engine, OutputFormat, RenderOptions};
//!
//! let quill = quill_from_path("path/to/quill").unwrap();
//! let engine = Engine::new();
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
    Content, Normalized,
    backend::{Backend, FontFace},
    document::{
        AnchorRead, AnchorSelector, Annotated, AnnotatedMarkdown, Base, BaseError, Card, Document,
        DocumentAnchor, DocumentRevised, EditError, ImportError, Parsed, Reattached, Revised,
    },
    error::{DOCUMENT_FILE, Diagnostic, Location, ParseError, RenderError, RenderResult, Severity},
    path::DocPath,
    quill::{
        BUNDLE_EXTENSION, BoundParseError, CalendarDate, CardSchema, FieldSchema, FieldType,
        FileTreeNode, ParseDateError, Quill, QuillConfig, QuillIgnore, ValidationError,
    },
    reader::{CardReader, TypedReader},
    region::{
        ContentHit, FieldCoverage, HitGranularity, RenderedRegion, coverage, plate_addr_to_doc_path,
    },
    session::{ChangeBundle, ChangeSet, Delta, LiveSession},
    types::{Artifact, OutputFormat, RenderOptions},
    value::{PathSegment, QuillValue},
    version::QuillReference,
    writer::TypedWriter,
};

mod load;
mod orchestration;

pub use load::{quill_from_path, tree_from_path};
pub use orchestration::Engine;

#[cfg(feature = "typst")]
pub use quillmark_typst::{
    FontLibrary, plate_file as typst_plate_file, typst_version, workspace as typst_workspace,
};

#[cfg(feature = "acroform")]
pub use quillmark_acroform::check_form as acroform_check_form;
