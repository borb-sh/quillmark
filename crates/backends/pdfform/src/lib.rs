//! A Typst-free Quillmark backend that fills existing PDF forms. A `pdfform`
//! quill ships two assets: `form.pdf`, the stripped background (the normalized
//! form with its `/AcroForm`, widget annotations, and page `/Annots` removed),
//! and `form.json`, the value-free field spec. The backend binds document
//! values against `compile_data` and writes a fresh AcroForm onto the
//! background via the `quillmark-pdf` stamping spine; it never reads or
//! reconciles a foreign AcroForm.

mod bind;
mod flatten;
mod form;
mod resolve;
mod typography;

use bind::BoundWidget;
use flatten::flatten as flatten_to_pdf;
use form::FormSpec;
use quillmark_core::quill::QuillConfig;
use quillmark_core::session::SessionHandle;
use quillmark_core::{
    Artifact, Backend, ChangeSet, LiveSession, OutputFormat, Quill, RenderError, RenderOptions,
    RenderResult, RenderedRegion,
};
use quillmark_pdf::regions_of;
use quillmark_pdf::{stamp, FieldSpec, StampOptions};

use {
    hayro::hayro_interpret::{font::FontQuery, InterpreterSettings},
    hayro::hayro_syntax::Pdf as HayroPdf,
    hayro::{render as hayro_render, RenderCache, RenderSettings},
    hayro_svg::{convert as hayro_svg_convert, RenderCache as SvgCache, SvgRenderSettings},
    std::sync::Arc,
};

const FORM_PDF: &str = "form.pdf";
const FORM_JSON: &str = "form.json";

const SUPPORTED_FORMATS: &[OutputFormat] =
    &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

/// The PDF-form backend.
#[derive(Debug, Default)]
pub struct PdfformBackend;

impl quillmark_core::backend::sealed::Sealed for PdfformBackend {}

impl Backend for PdfformBackend {
    fn id(&self) -> &'static str {
        "pdfform"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<LiveSession, RenderError> {
        let files = source.files();
        let base_pdf = files
            .get_file(FORM_PDF)
            .ok_or_else(|| {
                RenderError::coded(
                    "pdfform::missing_form_pdf",
                    format!("pdfform quill is missing its `{FORM_PDF}` background"),
                )
            })?
            .to_vec();
        let form_json = files.get_file(FORM_JSON).ok_or_else(|| {
            RenderError::coded(
                "pdfform::missing_form_json",
                format!("pdfform quill is missing its `{FORM_JSON}` field spec"),
            )
        })?;

        let spec = FormSpec::parse(form_json)
            .map_err(|e| RenderError::coded(e.code(), e.to_string()))?;

        // Canvas boxes drive the top-left → bottom-left flip and the canvas
        // coordinate space, and reading them surfaces a malformed base early.
        let canvas_boxes = quillmark_pdf::page_canvas_boxes(&base_pdf)?;

        let bound = bind::bind_widgets(&spec, source.config(), &canvas_boxes)
            .map_err(|e| RenderError::coded(e.code(), e.to_string()))?;

        let field_specs = resolve_field_specs(&bound, json_data);

        let flat = flatten_and_parse(&base_pdf, &field_specs)?;

        Ok(LiveSession::new(
            Box::new(PdfformSession {
                base_pdf,
                bound,
                field_specs,
                canvas_boxes,
                flat,
            }),
            source.config().clone(),
        ))
    }
}

/// Whether `path` resolves against `config` as a schema address.
///
/// Binding a widget needs more: the resolved field must also project to a widget
/// shape. The grammar alone is what the Typst helper's `_qm-known-path` states a
/// second time, so `quillmark/tests/address_grammar.rs` compares the two here.
#[doc(hidden)]
pub fn resolves_schema_address(config: &QuillConfig, path: &str) -> bool {
    bind::bind(config, "", path).is_ok()
}

fn resolve_field_specs(bound: &[BoundWidget], json_data: &serde_json::Value) -> Vec<FieldSpec> {
    bound
        .iter()
        .map(|widget| resolve::field_spec(widget, json_data))
        .collect()
}

/// Bake `field_specs` into the base as content-stream operators, then parse the
/// result for hayro. Both halves happen here so the two places `field_specs`
/// are set are the two places the derived PDF moves.
///
/// A parse failure is this crate flattening to something malformed, never a
/// property of the output format asking for it.
fn flatten_and_parse(base_pdf: &[u8], field_specs: &[FieldSpec]) -> Result<HayroPdf, RenderError> {
    let flat = flatten_to_pdf(base_pdf.to_vec(), field_specs)?;
    HayroPdf::new(flat).map_err(|_| {
        RenderError::coded(
            "pdfform::flat_parse_failed",
            "failed to parse the flattened PDF for rasterisation",
        )
    })
}

struct PdfformSession {
    base_pdf: Vec<u8>,
    bound: Vec<BoundWidget>,
    field_specs: Vec<FieldSpec>,
    /// Per page, `/CropBox` ∩ `/MediaBox`: the box hayro rasterizes, whose
    /// lower-left corner is the canvas origin. Cached so `page_size_pt` need not
    /// reparse.
    canvas_boxes: Vec<[f32; 4]>,
    /// The base with `field_specs` baked in, parsed: the render paths hold
    /// pages rather than bytes to reparse per paint.
    flat: HayroPdf,
}

impl SessionHandle for PdfformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(quillmark_core::unsupported_format(
                format,
                "pdfform",
                SUPPORTED_FORMATS,
            ));
        }

        if format == OutputFormat::Svg {
            return self.render_svg();
        }
        if format == OutputFormat::Png {
            let scale = opts.ppi_or_default() / 72.0;
            return self.render_png(scale);
        }

        // PDF output is always an interactive AcroForm; value-flattening backs
        // only the raster paths, never a PDF deliverable.
        let producer = opts.producer.clone().unwrap_or_else(default_producer);
        let stamp_opts = StampOptions::default().with_producer(producer);
        let stamped = stamp(self.base_pdf.clone(), &self.field_specs, &stamp_opts)?;

        Ok(RenderResult::new(
            vec![Artifact::new(stamped, OutputFormat::Pdf)],
            OutputFormat::Pdf,
        ))
    }

    fn page_count(&self) -> usize {
        self.canvas_boxes.len()
    }

    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let [x0, y0, x1, y1] = *self.canvas_boxes.get(page)?;
        Some((x1 - x0, y1 - y0))
    }

    fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        let p = self.flat.pages().get(page)?;
        let cache = RenderCache::new();
        let interp = standard_font_settings();
        let render_settings = scaled_render_settings(scale);
        let pixmap = hayro_render(p, &cache, &interp, &render_settings);
        let w = pixmap.width() as u32;
        let h = pixmap.height() as u32;
        let bytes: Vec<u8> = pixmap
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|px| [px.r, px.g, px.b, px.a])
            .collect();
        Some((w, h, bytes))
    }

    /// A widget's `/Rect` is user space, which starts at the canvas box's
    /// lower-left corner rather than at `(0, 0)`; the raster puts that corner at
    /// the origin, so a region measures from it.
    fn regions(&self) -> Vec<RenderedRegion> {
        let mut regions = regions_of(&self.field_specs);
        for region in &mut regions {
            let Some([x0, y0, ..]) = self.canvas_boxes.get(region.page) else {
                continue;
            };
            let [rx0, ry0, rx1, ry1] = region.rect;
            region.rect = [rx0 - x0, ry0 - y0, rx1 - x0, ry1 - y0];
        }
        regions
    }

    /// Specs and flat PDF swap together only after both succeed. The background
    /// never changes, so field deltas are the only visible delta.
    fn update(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        let field_specs = resolve_field_specs(&self.bound, json_data);
        let flat = flatten_and_parse(&self.base_pdf, &field_specs)?;

        let mut dirty_pages: Vec<usize> = self
            .field_specs
            .iter()
            .zip(&field_specs)
            .filter(|(old, new)| old != new)
            .map(|(_, new)| new.page)
            .collect();
        dirty_pages.sort_unstable();
        dirty_pages.dedup();

        self.field_specs = field_specs;
        self.flat = flat;

        Ok(ChangeSet::new(self.canvas_boxes.len(), dirty_pages))
    }
}

impl PdfformSession {
    fn render_svg(&self) -> Result<RenderResult, RenderError> {
        let interp = standard_font_settings();
        let svg_settings = SvgRenderSettings {
            bg_color: [255, 255, 255, 255],
        };
        let artifacts: Vec<Artifact> = self
            .flat
            .pages()
            .iter()
            .map(|page| {
                let cache = SvgCache::new();
                let svg = hayro_svg_convert(page, &cache, &interp, &svg_settings);
                Artifact::new(svg.into_bytes(), OutputFormat::Svg)
            })
            .collect();

        Ok(RenderResult::new(artifacts, OutputFormat::Svg))
    }

    /// `scale` is device pixels per PDF point (`ppi / 72`).
    fn render_png(&self, scale: f32) -> Result<RenderResult, RenderError> {
        let interp = standard_font_settings();
        let render_settings = scaled_render_settings(scale);

        let mut artifacts = Vec::with_capacity(self.flat.pages().len());
        for page in self.flat.pages().iter() {
            let cache = RenderCache::new();
            let pixmap = hayro_render(page, &cache, &interp, &render_settings);
            let png = pixmap.into_png().map_err(|e| {
                RenderError::coded(
                    "pdfform::png_encoding",
                    format!("failed to encode page as PNG: {e}"),
                )
            })?;
            artifacts.push(Artifact::new(png, OutputFormat::Png));
        }

        Ok(RenderResult::new(artifacts, OutputFormat::Png))
    }
}

/// Shared by the RGBA canvas path and the PNG artifact path, which must agree
/// or a preview and its export drift apart.
fn scaled_render_settings(scale: f32) -> RenderSettings {
    use hayro::vello_cpu::color::palette::css::WHITE;
    RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..Default::default()
    }
}

/// Satisfies standard Type1 font queries from hayro's embedded font data:
/// required for the Helvetica and ZapfDingbats the flat PDF's content streams
/// draw with.
fn standard_font_settings() -> InterpreterSettings {
    InterpreterSettings {
        font_resolver: Arc::new(|query| match query {
            FontQuery::Standard(s) => Some(s.get_font_data()),
            FontQuery::Fallback(f) => Some(f.pick_standard_font().get_font_data()),
        }),
        ..Default::default()
    }
}

/// Owned by the backend, never defaulted from the leaf spine's version.
fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}
