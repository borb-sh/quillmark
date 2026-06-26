//! hayro-backed preview: render the *stripped background* to RGBA / SVG.
//!
//! Always renders the background, never the stamped PDF — under Technique A
//! (`NeedAppearances`, no `/AP`) a renderer paints blank fields, so field
//! values are composited by the GUI from the regions sidecar instead.

use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::{render, RenderCache, RenderSettings};
use quillmark_core::{Artifact, Diagnostic, OutputFormat, RenderError, Severity};

fn err(msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![
            Diagnostic::new(Severity::Error, msg.into()).with_code("pdfform::preview".to_string())
        ],
    }
}

fn load(bg: &[u8]) -> Result<Pdf, RenderError> {
    Pdf::new(bg.to_vec())
        .map_err(|e| err(format!("hayro could not read the background PDF: {e:?}")))
}

/// Page size in points, or `None` if the background won't parse / `page` is OOB.
pub(crate) fn page_size_pt(bg: &[u8], page: usize) -> Option<(f32, f32)> {
    let pdf = load(bg).ok()?;
    let pages = pdf.pages();
    let p = pages.get(page)?;
    Some(p.render_dimensions())
}

/// Non-premultiplied RGBA8 pixmap of the background page at `scale`× 72 ppi.
pub(crate) fn render_rgba(bg: &[u8], page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
    let pdf = load(bg).ok()?;
    let pages = pdf.pages();
    let p = pages.get(page)?;
    let pixmap = render(
        p,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            ..Default::default()
        },
    );
    let (w, h) = (pixmap.width() as u32, pixmap.height() as u32);
    let rgba = pixmap.take_unpremultiplied();
    let mut bytes = Vec::with_capacity(rgba.len() * 4);
    for px in rgba {
        bytes.extend_from_slice(&[px.r, px.g, px.b, px.a]);
    }
    Some((w, h, bytes))
}

/// One vector SVG artifact per requested page (`None` = all pages).
pub(crate) fn background_svgs(
    bg: &[u8],
    pages: Option<&[usize]>,
) -> Result<Vec<Artifact>, RenderError> {
    let pdf = load(bg)?;
    let all = pdf.pages();
    let indices: Vec<usize> = match pages {
        Some(sel) => sel.to_vec(),
        None => (0..all.len()).collect(),
    };
    let mut artifacts = Vec::with_capacity(indices.len());
    for idx in indices {
        let p = all
            .get(idx)
            .ok_or_else(|| err(format!("page index {idx} out of range")))?;
        let svg = hayro_svg::convert(
            p,
            &hayro_svg::RenderCache::new(),
            &InterpreterSettings::default(),
            &hayro_svg::SvgRenderSettings::default(),
        );
        artifacts.push(Artifact {
            bytes: svg.into_bytes(),
            output_format: OutputFormat::Svg,
        });
    }
    Ok(artifacts)
}
