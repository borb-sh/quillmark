use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

// `Tsify` declares the TypeScript type; values cross through `tsify::Ts<T>`,
// whose JS handle the wasm-bindgen shim owns and frees. The `into_wasm_abi` /
// `from_wasm_abi` ABI impls throw mid-conversion while holding that handle,
// which strands it (tsify#65), so no type here derives them.

/// Output formats supported by backends. Gated behind the engine surface so
/// tsify omits it from the core bundle, which has no rendering surface.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Pdf,
    Svg,
    Png,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<OutputFormat> for quillmark_core::OutputFormat {
    fn from(format: OutputFormat) -> Self {
        match format {
            OutputFormat::Pdf => quillmark_core::OutputFormat::Pdf,
            OutputFormat::Svg => quillmark_core::OutputFormat::Svg,
            OutputFormat::Png => quillmark_core::OutputFormat::Png,
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::OutputFormat> for OutputFormat {
    fn from(format: quillmark_core::OutputFormat) -> Self {
        match format {
            quillmark_core::OutputFormat::Pdf => OutputFormat::Pdf,
            quillmark_core::OutputFormat::Svg => OutputFormat::Svg,
            quillmark_core::OutputFormat::Png => OutputFormat::Png,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl From<quillmark_core::Severity> for Severity {
    fn from(severity: quillmark_core::Severity) -> Self {
        match severity {
            quillmark_core::Severity::Warning => Severity::Warning,
            quillmark_core::Severity::Error => Severity::Error,
        }
    }
}

impl From<Severity> for quillmark_core::Severity {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Error => quillmark_core::Severity::Error,
            Severity::Warning => quillmark_core::Severity::Warning,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl From<quillmark_core::Location> for Location {
    fn from(loc: quillmark_core::Location) -> Self {
        Location {
            file: loc.file,
            line: loc.line as usize,
            column: loc.column as usize,
        }
    }
}

impl From<Location> for quillmark_core::Location {
    fn from(loc: Location) -> Self {
        quillmark_core::Location::new(loc.file, loc.line as u32, loc.column as u32)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(hashmap_as_object)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    /// Document-model path anchor (e.g. `"cards.indorsement[0].signature_block"`),
    /// set on schema validation diagnostics and `undefined` otherwise.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
    /// The facts `message` interpolates, keyed by name. With `code`, enough to
    /// word this diagnostic in another language.
    ///
    /// Declared optional explicitly: `tsify` does not read
    /// `skip_serializing_if`, so an omitted field would be declared required.
    #[tsify(optional, type = "Record<string, unknown>")]
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty", default)]
    pub args: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_chain: Vec<String>,
}

// tsify's default serializer emits a `Map` for a map-typed field, against
// `args`' declared `Record<string, unknown>`. The config that applies is the
// *outermost* crossing type's, so every type that can carry a diagnostic across
// the ABI declares `hashmap_as_object`.
const _: () = assert!(<Diagnostic as tsify::Tsify>::SERIALIZATION_CONFIG.hashmap_as_object);

impl From<quillmark_core::Diagnostic> for Diagnostic {
    fn from(diag: quillmark_core::Diagnostic) -> Self {
        Diagnostic {
            severity: diag.severity.into(),
            code: diag.code,
            message: diag.message,
            location: diag.location.map(Into::into),
            path: diag.path,
            hint: diag.hint,
            args: diag.args,
            source_chain: diag.source_chain,
        }
    }
}

impl From<Diagnostic> for quillmark_core::Diagnostic {
    fn from(diag: Diagnostic) -> Self {
        let mut out = quillmark_core::Diagnostic::new(diag.severity.into(), diag.message);
        out.code = diag.code;
        out.location = diag.location.map(Into::into);
        out.path = diag.path;
        out.hint = diag.hint;
        out.args = diag.args;
        out.source_chain = diag.source_chain;
        out
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub format: OutputFormat,
    /// `serde_bytes` so the boundary emits a real `Uint8Array`, not `number[]`.
    #[serde(with = "serde_bytes")]
    #[tsify(type = "Uint8Array")]
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Artifact {
    fn mime_type_for_format(format: OutputFormat) -> String {
        quillmark_core::OutputFormat::from(format)
            .mime_type()
            .to_string()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::Artifact> for Artifact {
    fn from(artifact: quillmark_core::Artifact) -> Self {
        let format = artifact.output_format.into();
        Artifact {
            format,
            mime_type: Self::mime_type_for_format(format),
            bytes: artifact.bytes,
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(hashmap_as_object)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub artifacts: Vec<Artifact>,
    pub warnings: Vec<Diagnostic>,
    pub output_format: OutputFormat,
    pub render_time_ms: f64,
    /// Schema-field geometry, populated only when `RenderOptions.regions` asked
    /// for it. Page indices are document-space even under a `pages` subset.
    pub regions: Vec<FieldRegion>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
const _: () = assert!(<RenderResult as tsify::Tsify>::SERIALIZATION_CONFIG.hashmap_as_object);

/// What a committed `LiveSession.update` changed. `dirtyPages` lists pages whose
/// content differs from the previous compile, including pages the edit added;
/// removed pages are implied by `pageCount`.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub page_count: usize,
    pub dirty_pages: Vec<usize>,
}

/// A schema field address plus its geometry on the page. `field` is **not**
/// unique, and the whole-field highlight is a union `LiveSession.fieldBoxes`
/// owns: the consumer's copy of this contract is `runtime/runtime.d.ts`.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct FieldRegion {
    /// Canonical `DocPath` field address (e.g. `"cards.indorsement[1].from"`):
    /// the grammar `parseDocPath` reads and `Diagnostic.path` carries. Feed it
    /// back to `fieldBoxes` / `locate`.
    pub field: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin.
    pub rect: [f32; 4],
    /// The slice this box covers: USV `[start, end)` into the field's `Content`
    /// for one content segment, `undefined` for a scalar site or widget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[usize; 2]>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::RenderedRegion> for FieldRegion {
    fn from(r: quillmark_core::RenderedRegion) -> Self {
        FieldRegion {
            field: r.field,
            page: r.page,
            rect: r.rect,
            span: r.span,
        }
    }
}

/// How precisely a `ContentHit.pos` resolved. Never sub-cluster: `cluster` is
/// the finest this API offers, `segment` the floor it degrades to.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub enum HitGranularity {
    /// `pos` is the first content char of the cluster under the point. Place
    /// the caret there directly.
    Cluster,
    /// The point hit origin-less ink (list markers, numbering, a code fence's
    /// interior), so `pos` is the containing segment's start, not a caret.
    Segment,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::HitGranularity> for HitGranularity {
    fn from(g: quillmark_core::HitGranularity) -> Self {
        match g {
            quillmark_core::HitGranularity::Cluster => HitGranularity::Cluster,
            quillmark_core::HitGranularity::Segment => HitGranularity::Segment,
        }
    }
}

/// A resolved point → content position: the field a click landed in and the USV
/// offset into its `Content`. The `LiveSession.positionAt` result, inverse of
/// `locate`.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct ContentHit {
    /// Canonical `DocPath` field address (same grammar as `FieldRegion.field`).
    pub field: String,
    /// USV offset into the field's `Content`.
    pub pos: usize,
    /// `undefined` when the backend does not report granularity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<HitGranularity>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<quillmark_core::ContentHit> for ContentHit {
    fn from(h: quillmark_core::ContentHit) -> Self {
        ContentHit {
            field: h.field,
            pos: h.pos,
            granularity: h.granularity.map(Into::into),
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct RenderOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
    /// Pixels per inch for PNG; ignored for PDF and SVG. Defaults to 144.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppi: Option<f32>,
    /// 0-based page indices to render; `undefined` renders all pages. An index
    /// `>= pageCount` throws `backend::page_index_out_of_bounds`. Not supported
    /// for PDF output: throws `backend::page_selection_not_supported`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<Vec<usize>>,
    /// Populate `RenderResult.regions` with schema-field geometry, for consumers
    /// without a live session. Defaults to `false`. Page indices are
    /// document-space even when `pages` selects a subset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<bool>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            format: Some(OutputFormat::Pdf),
            ppi: None,
            pages: None,
            regions: None,
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl From<RenderOptions> for quillmark_core::RenderOptions {
    fn from(opts: RenderOptions) -> Self {
        let mut core = Self::default();
        core.output_format = opts.format.map(|f| f.into());
        core.ppi = opts.ppi;
        core.pages = opts.pages;
        core.regions = opts.regions.unwrap_or(false);
        core
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(feature = "typst", feature = "pdfform"))]
    fn field_region_serializes_to_expected_shape() {
        use super::FieldRegion;

        let region = FieldRegion {
            field: "full_name".to_string(),
            page: 0,
            rect: [180.0, 672.0, 520.0, 692.0],
            span: None,
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"field\":\"full_name\""));
        assert!(json.contains("\"page\":0"));
        assert!(json.contains("\"rect\":[180.0,672.0,520.0,692.0]"));
        assert!(!json.contains("\"span\""));
        assert!(!json.contains("\"name\""));
        assert!(!json.contains("\"kind\""));
    }

}
