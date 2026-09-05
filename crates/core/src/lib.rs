//! # Quillmark Core
//!
//! Foundational types for the Quillmark document engine: the [`Document`] model
//! and its `~~~` card-yaml blocks, the [`Quill`] format bundle, the [`Backend`]
//! seam output backends implement, and structured diagnostics carrying source
//! locations.
//!
//! The markdown grammar is specified in
//! [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md).
//!
//! ```no_run
//! use quillmark_core::Document;
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

pub mod document;
pub use document::{
    Card, CardWire, Document, EditError, Parsed, Payload, PayloadItem, PayloadItemWire,
    SeedOverlay, WireError,
};

pub mod writer;
pub use writer::{CardWriter, TypedWriter};

pub mod reader;
pub use reader::{CardReader, TypedReader};

pub mod backend;
pub use backend::{
    formats_support_canvas, page_selection_not_supported, selected_pages, unsupported_format,
    Backend,
};

pub mod error;
pub use error::{
    Diagnostic, Location, ParseError, RenderError, RenderResult, Severity, YamlError,
};

pub mod types;
pub use types::{Artifact, OutputFormat, RenderOptions};

pub mod region;
pub use region::{
    doc_path_to_plate_addr, field_boxes, plate_addr_to_doc_path, regions_to_doc_path, ContentHit,
    HitGranularity, RenderedRegion,
};

pub mod session;
pub use session::{
    ApplyError, Assoc, ChangeBundle, ChangeSet, Delta, IslandOp, LineOp, LiveSession, MarkOp, Op,
};

/// The canonical content model, re-exported so a consumer of the document
/// mutators need not depend on `quillmark-content` directly.
pub use quillmark_content::Content;

pub mod quill;
pub use quill::{
    blank, BoundParseError, CardValues, DocumentValues, FieldSource, FileTreeNode, Quill,
    Resolved, ResolvedCard, ResolvedField, ResolvedMain, QuillIgnore, STANDARD_METADATA_KEYS,
};
/// The schema model behind [`Quill::config`], and the error
/// [`QuillConfig::validate_document`] returns.
pub use quill::{CardSchema, FieldSchema, FieldType, QuillConfig, ValidationError};

pub mod value;
pub use value::{json_depth_exceeds, PathSegment, QuillValue};

pub mod path;
pub use path::{DocPath, DocSeg};

pub mod normalize;

pub mod version;
pub use version::{quill_ref_hint, QuillReference, Version, VersionSelector};
