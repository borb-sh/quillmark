//! Backend trait for output backends.

use crate::error::RenderError;
use crate::quill::Quill;
use crate::{LiveSession, OutputFormat};

#[doc(hidden)]
pub mod sealed {
    /// The seal on [`Backend`](super::Backend), implemented by the workspace's
    /// own backends. Naming it from elsewhere names a hidden item, which is the
    /// declaration that the seam behind it moves without notice.
    pub trait Sealed {}
}

/// Backend trait for rendering different output formats.
///
/// Implementing it outside the workspace is unsupported: [`Backend::open`]
/// returns a [`LiveSession`], which only a `#[doc(hidden)]` `SessionHandle`
/// implementation can build. The [`sealed::Sealed`] supertrait states that in
/// the type system — a declaration, not a barrier — so adding a method here
/// stays a minor release.
pub trait Backend: sealed::Sealed + Send + Sync + std::fmt::Debug {
    /// The backend identifier, e.g. `"typst"`.
    fn id(&self) -> &'static str;

    fn supported_formats(&self) -> &'static [OutputFormat];

    /// Open a live render session from a quill and compiled JSON data.
    ///
    /// The backend pulls whatever static inputs it needs straight from
    /// `source`. There is no universal "template" input: a plate is one
    /// backend's private notion, read by that backend from its own files.
    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
    ) -> Result<LiveSession, RenderError>;
}

/// The refusal every backend owes a format outside its
/// [`Backend::supported_formats`], under `backend::format_not_supported`.
/// `backend` names the backend in the message; `supported` becomes the hint.
pub fn unsupported_format(format: OutputFormat, backend: &str, supported: &[OutputFormat]) -> RenderError {
    RenderError::from_diag(
        crate::Diagnostic::new(
            crate::Severity::Error,
            format!("{format:?} not supported by the {backend} backend"),
        )
        .with_code("backend::format_not_supported".to_string())
        .with_hint(format!("Supported formats: {supported:?}")),
    )
}

/// The pixel ceiling on one rasterized page, shared by every raster path so a
/// caller meets one number.
///
/// RGBA is 4 bytes a pixel, so one page's buffer stops at 1 GiB: a quarter of
/// wasm32's whole address space, and far under the size at which the
/// rasterizers' own 32-bit dimension arithmetic wraps. It is also the area of
/// the WASM painter's 16384-px-per-side backing-store clamp
/// ([PREVIEW.md](https://github.com/borb-sh/quillmark/blob/main/prose/canon/PREVIEW.md)),
/// so every raster that clamp admits still renders. US Letter at
/// [`RenderOptions::DEFAULT_PPI`](crate::RenderOptions::DEFAULT_PPI) is 1.9 Mpx,
/// 138× under it.
pub const MAX_RASTER_PIXELS: u64 = 16_384 * 16_384;

fn invalid_raster_scale(message: String, hint: &str) -> RenderError {
    RenderError::from_diag(
        crate::Diagnostic::new(crate::Severity::Error, message)
            .with_code("backend::invalid_raster_scale".to_string())
            .with_hint(hint.to_string()),
    )
}

/// Device pixels per point for a raster render at `ppi`, under
/// `backend::invalid_raster_scale` unless `ppi` is finite and positive.
pub fn raster_scale(ppi: f32) -> Result<f32, RenderError> {
    if !ppi.is_finite() || ppi <= 0.0 {
        return Err(invalid_raster_scale(
            format!("ppi {ppi} is not a finite positive number"),
            "Pass a ppi above 0, or none at all for the default 144.",
        ));
    }
    Ok(ppi / 72.0)
}

/// The refusal every raster backend owes a page it cannot rasterize, checked
/// before the rasterizer allocates: under `backend::invalid_raster_scale` unless
/// `scale` (device pixels per point, as [`raster_scale`] returns) is finite and
/// positive and the `width_pt` × `height_pt` page fits [`MAX_RASTER_PIXELS`].
pub fn check_raster(scale: f32, width_pt: f32, height_pt: f32) -> Result<(), RenderError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid_raster_scale(
            format!("raster scale {scale} is not a finite positive number of device pixels per point"),
            "Pass a scale above 0.",
        ));
    }
    // `max` takes the non-NaN side, flooring a degenerate page size at the one
    // pixel the rasterizers floor it at.
    let px = |pt: f32| (f64::from(scale) * f64::from(pt)).round().max(1.0);
    let (w, h) = (px(width_pt), px(height_pt));
    if w * h > MAX_RASTER_PIXELS as f64 {
        return Err(invalid_raster_scale(
            format!(
                "a {width_pt}x{height_pt} pt page at {scale} device pixels per point is {w}x{h} px, past the {MAX_RASTER_PIXELS} px ceiling"
            ),
            "Rasterize fewer pixels: lower the ppi (the default is 144) or the canvas scale.",
        ));
    }
    Ok(())
}

/// Pre-session hint for whether a backend with these `formats` can paint pages
/// to a canvas, used before a session exists (e.g. a GUI deciding whether to
/// mount a canvas preview without first paying to open one).
///
/// Canvas paint needs a per-page visual image of the laid-out page, so the
/// predicate keys off the visual-page formats ([`OutputFormat::Png`],
/// [`OutputFormat::Svg`]) rather than [`OutputFormat::Pdf`].
///
/// Only a hint: the authoritative answer is
/// [`LiveSession::supports_canvas`](crate::LiveSession::supports_canvas),
/// derived from the session's own canvas seam.
pub fn formats_support_canvas(formats: &[OutputFormat]) -> bool {
    formats
        .iter()
        .any(|f| matches!(f, OutputFormat::Png | OutputFormat::Svg))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// US Letter, the shape every raster check below is measured against.
    const LETTER_PT: (f32, f32) = (612.0, 792.0);

    fn code(err: RenderError) -> String {
        err.diagnostics()[0]
            .code
            .clone()
            .expect("a refusal carries its code")
    }

    #[test]
    fn raster_scale_refuses_a_ppi_that_is_not_finite_and_positive() {
        for ppi in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 0.0, -144.0] {
            let err = raster_scale(ppi)
                .err()
                .unwrap_or_else(|| panic!("{ppi} is not a usable ppi"));
            assert_eq!(code(err), "backend::invalid_raster_scale");
        }
        assert_eq!(raster_scale(144.0).expect("144 ppi is usable"), 2.0);
    }

    #[test]
    fn check_raster_refuses_a_page_past_the_pixel_ceiling() {
        let (w, h) = LETTER_PT;
        let ceiling = (MAX_RASTER_PIXELS as f64 / f64::from(w * h)).sqrt() as f32;
        assert!(check_raster(ceiling, w, h).is_ok());
        assert_eq!(
            code(check_raster(ceiling * 2.0, w, h).expect_err("twice the ceiling scale")),
            "backend::invalid_raster_scale"
        );
        assert_eq!(
            code(check_raster(f32::INFINITY, w, h).expect_err("an infinite scale")),
            "backend::invalid_raster_scale"
        );
    }

    #[test]
    fn the_default_ppi_leaves_a_letter_page_far_under_the_ceiling() {
        let (w, h) = LETTER_PT;
        let scale = raster_scale(crate::RenderOptions::DEFAULT_PPI).expect("the default ppi");
        check_raster(scale, w, h).expect("the default render is not near the ceiling");
        assert!(f64::from(w * scale) * f64::from(h * scale) * 100.0 < MAX_RASTER_PIXELS as f64);
    }

    #[test]
    fn formats_support_canvas_keys_off_visual_formats() {
        use OutputFormat::*;
        assert!(formats_support_canvas(&[Pdf, Svg, Png]));
        assert!(formats_support_canvas(&[Pdf, Svg]));
        assert!(formats_support_canvas(&[Png]));
        assert!(!formats_support_canvas(&[Pdf]));
        assert!(!formats_support_canvas(&[]));
    }
}
