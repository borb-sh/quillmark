//! A Typst-free Quillmark backend that fills existing PDF forms. An `acroform`
//! quill ships two assets: `form.pdf`, the stripped background (the normalized
//! form with its `/AcroForm`, widget annotations, and page `/Annots` removed),
//! and `Form.yaml`, the value-free field spec. The backend binds document
//! values against `compile_data` and writes a fresh AcroForm onto the
//! background via the `quillmark-pdf` stamping spine; it never reads or
//! reconciles a foreign AcroForm.

mod bind;
mod form;
mod resolve;

use bind::BoundWidget;
use form::{FORM_YAML, FormSpec};
use quillmark_core::quill::{FileTreeNode, QuillConfig};
use quillmark_core::session::SessionHandle;
use quillmark_core::{
    backend::Backend,
    error::{Diagnostic, RenderError, RenderResult, Severity},
    quill::{CalendarDate, Quill},
    region::{RenderedRegion, plate_addr_to_doc_path},
    session::{ChangeSet, LiveSession},
    types::{Artifact, OutputFormat, RenderOptions},
};
use quillmark_pdf::{AppearanceStates, FieldSpec, StampOptions, regions_of, stamp};

use hayro::hayro_interpret::{InterpreterSettings, font::FontQuery};
use hayro::hayro_syntax::Pdf as HayroPdf;
use hayro::vello_cpu::Pixmap;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings, render};
use std::sync::Arc;

const FORM_PDF: &str = "form.pdf";
/// The names a field spec once had, each refused under its own code, never read.
const LEGACY_SPECS: [(&str, &str); 2] = [
    ("form.json", "acroform::legacy_form_json"),
    ("form.yaml", "acroform::legacy_form_yaml"),
];

const SUPPORTED_FORMATS: &[OutputFormat] = &[OutputFormat::Pdf];

/// The acroform backend, which fills an existing PDF form.
#[derive(Debug, Default)]
pub struct AcroformBackend;

impl Backend for AcroformBackend {
    fn id(&self) -> &'static str {
        "acroform"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
        today: CalendarDate,
    ) -> Result<LiveSession, RenderError> {
        let Form {
            base_pdf,
            canvas_boxes,
            bound,
        } = read_form(source)?;
        let field_specs = resolve_field_specs(&bound, json_data);
        let raster = stamp_raster(&base_pdf, &field_specs)?;
        let warnings = value_warnings(&field_specs, json_data);

        Ok(LiveSession::new(
            Box::new(AcroformSession {
                base_pdf,
                bound,
                field_specs,
                canvas_boxes,
                raster,
                warnings,
            }),
            source.config().clone(),
            today,
            json_data,
        ))
    }
}

/// The refusal `open` raises for the quill's `form.pdf` or `Form.yaml`, if
/// any: each is present, the spec parses, and every widget binds against the
/// schema and the background's pages. Nothing is stamped or rendered.
pub fn check_form(source: &Quill) -> Result<(), RenderError> {
    read_form(source).map(|_| ())
}

struct Form {
    base_pdf: Vec<u8>,
    canvas_boxes: Vec<[f32; 4]>,
    bound: Vec<BoundWidget>,
}

fn read_form(source: &Quill) -> Result<Form, RenderError> {
    let files = source.files();
    let base_pdf = files
        .get_file(FORM_PDF)
        .ok_or_else(|| {
            RenderError::coded(
                "acroform::missing_form_pdf",
                format!("acroform quill is missing its `{FORM_PDF}` background"),
            )
        })?
        .to_vec();
    let spec = FormSpec::parse(form_spec_bytes(files)?)
        .map_err(|e| RenderError::from_diag(e.to_diagnostic()))?;

    // Canvas boxes drive the top-left → bottom-left flip and the canvas
    // coordinate space, and reading them surfaces a malformed base early.
    let canvas_boxes = quillmark_pdf::page_canvas_boxes(&base_pdf)?;

    let bound = bind::bind_widgets(&spec, source.config(), &canvas_boxes)
        .map_err(|e| RenderError::coded(e.code(), e.to_string()))?;
    Ok(Form {
        base_pdf,
        canvas_boxes,
        bound,
    })
}

/// `Form.yaml`'s bytes. A quill holding a spec under a name it once had, alone
/// or beside `Form.yaml`, is refused: no file is ever chosen silently.
fn form_spec_bytes(files: &FileTreeNode) -> Result<&[u8], RenderError> {
    let spec = files.get_file(FORM_YAML);
    if let Some(&(legacy, code)) = LEGACY_SPECS
        .iter()
        .find(|(name, _)| files.get_file(name).is_some())
    {
        return Err(match spec {
            None => RenderError::coded_hint(
                code,
                format!(
                    "acroform quill ships its field spec as `{legacy}`; the backend reads `{FORM_YAML}`"
                ),
                format!("Rename `{legacy}` to `{FORM_YAML}`; the contents are valid as-is."),
            ),
            Some(_) => RenderError::coded_hint(
                code,
                format!(
                    "acroform quill ships both `{FORM_YAML}` and `{legacy}`; the backend reads `{FORM_YAML}` alone"
                ),
                format!("Keep one: delete `{legacy}`, or rename it over `{FORM_YAML}`."),
            ),
        });
    }
    spec.ok_or_else(|| {
        RenderError::coded_hint(
            "acroform::missing_form_spec",
            format!("acroform quill is missing its `{FORM_YAML}` field spec"),
            format!("Add `{FORM_YAML}` at the quill root, beside `Quill.yaml`."),
        )
    })
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

/// The warnings on what the baked appearance draws of each field's value, one
/// of each code per field, anchored at the field: `acroform::unencodable_text`
/// where it draws characters as `?`, and `acroform::text_overflow` where it
/// cuts the value off.
fn value_warnings(field_specs: &[FieldSpec], json_data: &serde_json::Value) -> Vec<Diagnostic> {
    let card_kinds: Vec<Option<&str>> = json_data
        .get("$cards")
        .and_then(serde_json::Value::as_array)
        .map(|cards| {
            cards
                .iter()
                .map(|card| card.get("$kind").and_then(serde_json::Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let mut warned = std::collections::BTreeSet::new();
    let mut warnings = Vec::new();
    for spec in field_specs {
        let (Some(field), Some(value)) = (spec.schema_field.as_deref(), spec.value.as_deref())
        else {
            continue;
        };
        let path = || {
            plate_addr_to_doc_path(field, &card_kinds)
                .map_or_else(|| field.to_string(), |path| path.to_string())
        };
        let count = quillmark_pdf::unencodable_chars(value);
        if count > 0 && warned.insert((field, "acroform::unencodable_text")) {
            warnings.push(
                Diagnostic::new(
                    Severity::Warning,
                    format!(
                        "{count} character(s) in this field are outside the form's font and \
                         draw as `?` in the preview and wherever the form's own appearance shows"
                    ),
                )
                .with_code("acroform::unencodable_text".to_string())
                .with_path(path())
                .with_hint(
                    "The filled field keeps the full text, and a viewer that redraws the field \
                     (Acrobat, Chrome, pdf.js) shows it."
                        .to_string(),
                ),
            );
        }
        if quillmark_pdf::overflows(spec) && warned.insert((field, "acroform::text_overflow")) {
            warnings.push(
                Diagnostic::new(
                    Severity::Warning,
                    "this field's text does not fit its box even at the smallest size, so the \
                     preview cuts it off"
                        .to_string(),
                )
                .with_code("acroform::text_overflow".to_string())
                .with_path(path())
                .with_hint(
                    "The filled field keeps the full text; shorten it to show all of it on the \
                     printed form."
                        .to_string(),
                ),
            );
        }
    }
    warnings
}

fn resolve_field_specs(bound: &[BoundWidget], json_data: &serde_json::Value) -> Vec<FieldSpec> {
    bound
        .iter()
        .map(|widget| resolve::field_spec(widget, json_data))
        .collect()
}

/// Stamp `field_specs` onto the base as the canvas draws it, then parse the
/// result for hayro. hayro reads a widget's `/AP` `/N` only as a stream, so each
/// checkbox carries the one state its `/AS` names; the deliverable `render`
/// stamps carries both, for a filler to toggle.
///
/// A parse failure is this crate stamping something malformed, never a property
/// of the document being rendered.
fn stamp_raster(base_pdf: &[u8], field_specs: &[FieldSpec]) -> Result<HayroPdf, RenderError> {
    let opts = StampOptions {
        states: AppearanceStates::Selected,
        ..StampOptions::default()
    };
    let stamped = stamp(base_pdf.to_vec(), field_specs, &opts)?;
    HayroPdf::new(stamped).map_err(|_| {
        RenderError::coded(
            "acroform::stamped_parse_failed",
            "failed to parse the stamped PDF for rasterisation",
        )
    })
}

struct AcroformSession {
    base_pdf: Vec<u8>,
    bound: Vec<BoundWidget>,
    field_specs: Vec<FieldSpec>,
    /// Per page, `/CropBox` ∩ `/MediaBox`: the box hayro rasterizes, whose
    /// lower-left corner is the canvas origin. Cached so `page_size_pt` need not
    /// reparse.
    canvas_boxes: Vec<[f32; 4]>,
    /// [`stamp_raster`] of `field_specs`: the canvas path holds pages rather
    /// than bytes to reparse per paint.
    raster: HayroPdf,
    /// [`value_warnings`] of `field_specs`.
    warnings: Vec<Diagnostic>,
}

impl SessionHandle for AcroformSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);
        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(quillmark_core::backend::unsupported_format(
                format,
                "acroform",
                SUPPORTED_FORMATS,
            ));
        }

        if opts.pages.is_some() {
            return Err(quillmark_core::backend::page_selection_not_supported(
                format,
            ));
        }

        let pdf = stamp(
            self.base_pdf.clone(),
            &self.field_specs,
            &StampOptions::default(),
        )?;
        Ok(RenderResult::new(
            vec![Artifact::new(pdf, OutputFormat::Pdf)],
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

    fn render_rgba(
        &self,
        page: usize,
        scale: f32,
    ) -> Result<Option<(u32, u32, Vec<u8>)>, RenderError> {
        let Some(pixmap) = self.raster(page, scale)? else {
            return Ok(None);
        };
        let w = pixmap.width() as u32;
        let h = pixmap.height() as u32;
        let bytes = pixmap
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        Ok(Some((w, h, bytes)))
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

    /// Specs and raster swap together only after both succeed. The background
    /// never changes, so field deltas are the only visible delta.
    fn update(
        &mut self,
        _config: &QuillConfig,
        json_data: &serde_json::Value,
    ) -> Result<ChangeSet, RenderError> {
        let field_specs = resolve_field_specs(&self.bound, json_data);
        let raster = stamp_raster(&self.base_pdf, &field_specs)?;
        self.warnings = value_warnings(&field_specs, json_data);

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
        self.raster = raster;

        Ok(ChangeSet::new(self.canvas_boxes.len(), dirty_pages))
    }

    fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }
}

impl AcroformSession {
    fn raster(&self, page: usize, scale: f32) -> Result<Option<Pixmap>, RenderError> {
        let Some(p) = self.raster.pages().get(page) else {
            return Ok(None);
        };
        let (width_pt, height_pt) = p.render_dimensions();
        quillmark_core::backend::check_raster(scale, width_pt, height_pt)?;
        // The size is explicit because hayro's `render` floors the scaled page,
        // to 0 px under one pixel, where `check_raster` counts a rounded side
        // of at least one; `check_raster` bounds each side within `u16`.
        let (width, height) = quillmark_core::backend::raster_size(scale, width_pt, height_pt);
        let (width, height) = (width as u16, height as u16);
        let pixmap = render(
            p,
            &RenderCache::new(),
            &standard_font_settings(),
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                width: Some(width),
                height: Some(height),
                bg_color: WHITE,
            },
        );
        Ok(Some(pixmap))
    }
}

/// Satisfies standard Type1 font queries from hayro's embedded font data:
/// required for the base-14 faces the widgets' appearance streams draw with.
fn standard_font_settings() -> InterpreterSettings {
    InterpreterSettings {
        font_resolver: Arc::new(|query| match query {
            FontQuery::Standard(s) => Some(s.get_font_data()),
            FontQuery::Fallback(f) => Some(f.pick_standard_font().get_font_data()),
        }),
        ..Default::default()
    }
}
