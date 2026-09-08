//! # quillmark-pdf: the AcroForm stamping spine
//!
//! Typst-free infra (not a backend) whose whole job is one pure operation:
//!
//! ```text
//! (base_pdf_bytes, &[FieldSpec]) -> { stamped_pdf, regions }
//! ```
//!
//! via a single incremental-update append. Both backends — Typst (geometry from
//! introspection) and `pdfform` (geometry from `form.json`) — produce a base PDF
//! plus [`FieldSpec`]s, and unify exactly at that seam. The crate owns its own
//! [`PdfError`]; each backend maps it to `quillmark_core::RenderError`.
//!
//! `crate::reader`'s docs carry the input contract the base PDF must satisfy.

mod error;
/// Byte-level reads over an existing PDF.
///
/// **Workspace-internal; not covered by this crate's semver.** `pub` only so
/// `quillmark-pdfform` can reach it.
#[doc(hidden)]
pub mod reader;
mod stamp;
mod update;
/// Byte-level writes: object emission, id allocation, string escaping, WinAnsi.
///
/// **Workspace-internal; not covered by this crate's semver.**
#[doc(hidden)]
pub mod writer;

pub use error::PdfError;
pub use reader::ObjectIndex;
pub use stamp::{regions_of, stamp, StampOptions, CHECKBOX_ON_STATE};
pub use update::PdfUpdate;

const CODE_BAD_RECT: &str = "pdf::bad_rect";

/// The canvas box of every page of `base` — `/CropBox` intersected with
/// `/MediaBox`, normalized to `[x0, y0, x1, y1]` (lower-left, upper-right) — in
/// document order. Both boxes resolve along the page's ancestor chain, and a
/// page declaring no `/CropBox` takes its `/MediaBox`.
///
/// This is the box a viewer displays and a rasterizer draws, so a backend owning
/// top-left page-relative rects flips against it before building a
/// [`FieldSpec`], and page-canvas geometry measures from its lower-left corner.
///
/// # Errors
///
/// `pdf::degenerate_page_box` when a page's canvas box is under a point per
/// side: nothing renders on it.
pub fn page_canvas_boxes(base: &[u8]) -> Result<Vec<[f32; 4]>, PdfError> {
    reader::page_canvas_boxes(base)
}

/// One fully-resolved form field: the backend-agnostic currency of the spine.
/// `rect` is final geometry, so the spine never reasons about page height or
/// reflow.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// Fully-qualified field name, written to `/T`.
    pub name: String,
    /// The quill schema field address this widget maps to. Opaque to the spine,
    /// which carries it only to key the region sidecar: `None` emits no
    /// [`RenderedRegion`](quillmark_core::RenderedRegion).
    pub schema_field: Option<String>,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin.
    pub rect: [f32; 4],
    pub field_type: FieldType,
    /// `None` is blank; for a checkbox, `Some` (the on-state name) is checked.
    pub value: Option<String>,
    /// Optional `/TU` tooltip / accessible name.
    pub tooltip: Option<String>,
    /// Base-14 face for the value text.
    pub font: FormFont,
    /// Value text size in points; `None` writes `0 Tf`, deferring to the
    /// viewer's auto-size (which refits as the user types).
    pub font_size: Option<f32>,
    /// Value text justification, written to `/Q`.
    pub align: TextAlign,
}

impl FieldSpec {
    /// `schema_field`, `value`, and `tooltip` start `None`; the three type
    /// dials start at the house style (Helvetica, auto-size, left).
    pub fn new(name: String, page: usize, rect: [f32; 4], field_type: FieldType) -> Self {
        Self {
            name,
            schema_field: None,
            page,
            rect,
            field_type,
            value: None,
            tooltip: None,
            font: FormFont::default(),
            font_size: None,
            align: TextAlign::default(),
        }
    }

    /// `Err` with code `pdf::bad_rect` unless every [`rect`](Self::rect)
    /// coordinate is finite. `rect` is public and every writer here prints a
    /// float verbatim, so an unchecked one reaches a widget `/Rect` or a drawn
    /// content stream as `NaN`/`inf`, a token no PDF number grammar admits.
    pub fn assert_finite_rect(&self) -> Result<(), PdfError> {
        if self.rect.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        Err(PdfError::new(
            CODE_BAD_RECT,
            format!(
                "field `{}` has a non-finite /Rect: {:?}",
                self.name, self.rect
            ),
        ))
    }
}

/// A field's definition, never a runtime value (that rides in
/// [`FieldSpec::value`]).
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    Text { multiline: bool },
    /// A checkbox with the engine's fixed on-state ([`CHECKBOX_ON_STATE`]).
    Checkbox,
    /// A dropdown over `options`, bare display strings.
    Choice { options: Vec<String> },
    /// An unsigned signature field.
    Signature,
}

/// The base-14 face a widget's value text is set in.
///
/// Restricted to the three text families the PDF viewer is required to have, so
/// a `/DA` never names a font the document does not carry: embedding an
/// arbitrary face would mean shipping font programs the background already
/// carries, which the two-asset model leaves to the background.
///
/// Only the [`FieldType::Text`] and [`FieldType::Choice`] widgets have variable
/// text; this is inert on the other two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormFont {
    #[default]
    Helvetica,
    Times,
    Courier,
}

impl FormFont {
    /// The `/DR` `/Font` key a `/DA` names this face by, and the name a drawn
    /// content stream selects it with.
    pub const fn resource_name(self) -> &'static str {
        match self {
            Self::Helvetica => "Helv",
            Self::Times => "TiRo",
            Self::Courier => "Cour",
        }
    }

    /// The standard-14 `/BaseFont`, never embedded.
    pub(crate) fn base_font(self) -> &'static [u8] {
        match self {
            Self::Helvetica => b"Helvetica",
            Self::Times => b"Times-Roman",
            Self::Courier => b"Courier",
        }
    }
}

/// Justification of a widget's value text, written to `/Q`.
///
/// A fillable widget's box is sized for the longest plausible value, not the
/// value itself, so this is the only thing that pins text to an edge: geometry
/// cannot, the text extent being unknown until someone types it.
///
/// Only the [`FieldType::Text`] and [`FieldType::Choice`] widgets have variable
/// text; this is inert on the other two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}
