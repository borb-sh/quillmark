//! # quillmark-pdf
//!
//! Shared, backend-agnostic AcroForm widget stamping for Quillmark.
//!
//! AcroForm stamping is a cross-cutting capability: both the Typst backend
//! (geometry from introspection) and the `pdfform` backend (geometry from a
//! quill's `form.json`) need it, and they differ *only* in where the geometry
//! comes from. This crate is that shared spine — a pure function
//!
//! ```text
//! (base_pdf_bytes, &[FieldSpec]) -> { pdf_with_widgets, regions }
//! ```
//!
//! via an incremental-update append. It writes the `/AcroForm` **fresh** from
//! the specs (strip-and-rebuild, never reconcile a foreign form), styles the
//! real fields under Technique A (`/NeedAppearances`, no baked `/AP`), and
//! reports each field's geometry as a phase-1 [`RenderedRegion`] for the GUI's
//! interactivity overlay.
//!
//! It knows AcroForm dictionaries and nothing about Typst, `form.json`, or
//! field maps. See issue #744 for the surrounding architecture.

#[cfg(feature = "compose")]
mod compose;
mod scan;
mod spec;
mod stamp;

#[cfg(feature = "compose")]
pub use compose::compose;
pub use spec::{Appearance, ChoiceOption, FieldSpec, FieldType};
pub use stamp::{default_producer, page_count, stamp, StampOptions, StampResult};

// The regions sidecar types live in core (they ride on `RenderResult`); re-export
// for callers that work through this crate's stamping API.
pub use quillmark_core::{RegionKind, RenderedRegion};
