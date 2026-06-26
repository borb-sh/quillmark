//! Rendered-region geometry: the phase-1 sidecar a render reports alongside its
//! artifact, so a GUI can build an interactivity overlay (field↔region
//! navigation, click-to-field) without parsing the output.
//!
//! Geometry is authoritative from the backend: fixed page-relative rects for the
//! `pdfform` backend (`form.json`), introspection for Typst. Interactivity is
//! GUI-owned; the engine only reports geometry.

/// One region reported back to the GUI. Phase 1: fields only.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRegion {
    /// The field/region name (e.g. an AcroForm `/T`).
    pub name: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin.
    pub rect: [f32; 4],
    pub kind: RegionKind,
}

/// Discriminated region kind. Only [`RegionKind::Field`] exists today; the enum
/// shape is deliberate so later phases (named markup regions) extend it without
/// breaking consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionKind {
    Field {
        /// Lowercase field-type discriminant (`text` / `checkbox` / `choice` /
        /// `signature`).
        field_type: String,
        /// The field's current value, if any (text/choice).
        value: Option<String>,
    },
}
