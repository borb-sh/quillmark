//! Compiles a plate plus injected JSON data into a Typst paged document, then
//! renders selected pages to output bytes (PDF, SVG, or PNG).

use std::collections::HashMap;

use typst::diag::Warned;
use typst::layout::{Frame, FrameItem};
use typst::model::Destination;
use typst::utils::Scalar;
use typst::visualize::Paint;
use typst_layout::{Page, PagedDocument};
use typst_pdf::PdfOptions;
use typst_render::RenderOptions;
use typst_svg::SvgOptions;

use crate::error_mapping::map_typst_errors;
use crate::world::QuillWorld;
use quillmark_core::{
    backend::{check_raster_ppi, page_selection_not_supported, selected_pages},
    error::{Diagnostic, RenderError, RenderResult},
    types::{Artifact, OutputFormat},
};
use quillmark_pdf::{FieldSpec, StampOptions, stamp};

pub(crate) fn render_options(pixel_per_pt: f32) -> RenderOptions {
    RenderOptions {
        pixel_per_pt: Scalar::new(pixel_per_pt as f64),
        ..Default::default()
    }
}

/// `comemo`'s cache is process-global and unbounded without eviction; 10 matches
/// typst-cli's watch-loop policy. The age clock is process-global too, so
/// interleaved sessions evict each other's entries early — lost reuse, never a
/// wrong render.
const COMEMO_EVICT_MAX_AGE: usize = 10;

/// On failure the warnings of the failed compile are dropped along with it.
pub(crate) fn compile_document(
    world: &QuillWorld,
) -> Result<(PagedDocument, Vec<Diagnostic>), RenderError> {
    let Warned { output, warnings } = typst::compile::<PagedDocument>(world);
    comemo::evict(COMEMO_EVICT_MAX_AGE);

    match output {
        Ok(doc) => Ok((doc, map_typst_errors(&warnings, world))),
        Err(errors) => Err(RenderError::new(map_typst_errors(&errors, world))),
    }
}

/// typst-svg writes a link URL into `href` and `xlink:href` with only `"`
/// escaped, so a URL holding `&` or `<` leaves the SVG ill-formed XML. Each
/// URL on the page is known, so an attribute value matching one's written form
/// is rewritten in full.
fn escape_link_hrefs(svg: String, page: &Page) -> String {
    let mut urls = Vec::new();
    collect_urls(&page.frame, &mut urls);
    if let Some(fill) = page.fill_or_white() {
        collect_paint_urls(&fill, &mut urls);
    }
    let fixes: HashMap<String, String> = urls
        .into_iter()
        .filter(|url| url.contains(['&', '<']))
        .map(|url| {
            let written = url.replace('"', "&quot;");
            let escaped = url
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('"', "&quot;");
            (written, escaped)
        })
        .collect();
    if fixes.is_empty() {
        return svg;
    }
    const OPEN: &str = "href=\"";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg.as_str();
    while let Some(at) = rest.find(OPEN) {
        let value_start = at + OPEN.len();
        let Some(len) = rest[value_start..].find('"') else {
            break;
        };
        let value = &rest[value_start..value_start + len];
        out.push_str(&rest[..value_start]);
        out.push_str(fixes.get(value).map_or(value, String::as_str));
        rest = &rest[value_start + len..];
    }
    out.push_str(rest);
    out
}

/// A tiling's frame renders into the SVG too, so its links count.
fn collect_urls(frame: &Frame, urls: &mut Vec<String>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => collect_urls(&group.frame, urls),
            FrameItem::Link(Destination::Url(url), _) => urls.push(url.as_str().to_string()),
            FrameItem::Shape(shape, _) => {
                let paints = shape
                    .fill
                    .iter()
                    .chain(shape.stroke.iter().map(|s| &s.paint));
                paints.for_each(|paint| collect_paint_urls(paint, urls));
            }
            FrameItem::Text(text) => {
                collect_paint_urls(&text.fill, urls);
                text.stroke
                    .iter()
                    .for_each(|s| collect_paint_urls(&s.paint, urls));
            }
            _ => {}
        }
    }
}

fn collect_paint_urls(paint: &Paint, urls: &mut Vec<String>) {
    if let Paint::Tiling(tiling) = paint {
        collect_urls(tiling.frame(), urls);
    }
}

/// `document` with its title resolved: `title`, else the plate's own, else
/// `fallback`, each trimmed and a blank one counting as absent. A plate's
/// `set document(title: none)` reads as no title, so the fallback fills it.
fn titled(document: &PagedDocument, title: Option<&str>, fallback: &str) -> PagedDocument {
    let mut document = document.clone();
    let info = document.info_mut();
    let present = |t: &str| Some(t.trim()).filter(|t| !t.is_empty()).map(Into::into);
    info.title = title
        .and_then(present)
        .or_else(|| info.title.as_deref().and_then(present))
        .or_else(|| present(fallback));
    document
}

/// `field_specs` are stamped as AcroForm widgets, and the title written, by the
/// PDF path only.
pub(crate) fn render_document_pages(
    document: &PagedDocument,
    pages: Option<&[usize]>,
    format: OutputFormat,
    ppi: f32,
    field_specs: &[FieldSpec],
    title: Option<&str>,
    fallback_title: &str,
) -> Result<RenderResult, RenderError> {
    if format == OutputFormat::Pdf && pages.is_some() {
        return Err(page_selection_not_supported(format));
    }

    let selected_indices = selected_pages(pages, document.pages().len())?;

    match format {
        OutputFormat::Svg => {
            let artifacts = selected_indices
                .into_iter()
                .map(|idx| {
                    let page = &document.pages()[idx];
                    let svg = typst_svg::svg(page, &SvgOptions::default());
                    Artifact::new(escape_link_hrefs(svg, page).into_bytes(), OutputFormat::Svg)
                })
                .collect();
            Ok(RenderResult::new(artifacts, OutputFormat::Svg))
        }
        OutputFormat::Png => {
            let scale = quillmark_core::backend::raster_scale(ppi)?;
            let opts = render_options(scale);
            // The longest page passing clears every page, and the ppi its
            // refusal names fits them all.
            if let Some(longest) = selected_indices
                .iter()
                .map(|&idx| document.pages()[idx].frame.size())
                .max_by_key(|size| size.max_by_side())
            {
                check_raster_ppi(ppi, longest.x.to_pt() as f32, longest.y.to_pt() as f32)?;
            }
            let mut artifacts = Vec::with_capacity(selected_indices.len());
            for idx in selected_indices {
                let page = &document.pages()[idx];
                let pixmap = typst_render::render(page, &opts);
                let png_data = pixmap.encode_png().map_err(|e| {
                    RenderError::coded("typst::png_encoding", format!("PNG encoding failed: {e}"))
                })?;
                artifacts.push(Artifact::new(png_data, OutputFormat::Png));
            }
            Ok(RenderResult::new(artifacts, OutputFormat::Png))
        }
        OutputFormat::Pdf => {
            let document = &titled(document, title, fallback_title);
            let pdf = typst_pdf::pdf(document, &PdfOptions::default()).map_err(|e| {
                RenderError::coded(
                    "typst::pdf_generation",
                    format!("PDF generation failed: {e:?}"),
                )
            })?;
            let stamped = stamp(pdf, field_specs, &StampOptions::default())?;
            Ok(RenderResult::new(
                vec![Artifact::new(stamped, OutputFormat::Pdf)],
                OutputFormat::Pdf,
            ))
        }
    }
}
