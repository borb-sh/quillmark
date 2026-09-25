//! Backend trait for output backends.

use crate::error::RenderError;
use crate::quill::{CalendarDate, Quill};
use crate::{session::LiveSession, types::OutputFormat};

/// Backend trait for rendering different output formats.
///
/// Implementing it outside the workspace is unsupported: [`Backend::open`]
/// returns a [`LiveSession`], which only a `#[doc(hidden)]` `SessionHandle`
/// implementation can build.
pub trait Backend: Send + Sync + std::fmt::Debug {
    /// The backend identifier, e.g. `"typst"`.
    fn id(&self) -> &'static str;

    fn supported_formats(&self) -> &'static [OutputFormat];

    /// Open a live render session from a quill and compiled JSON data.
    ///
    /// The backend pulls whatever static inputs it needs straight from
    /// `source`. There is no universal "template" input: a plate is one
    /// backend's private notion, read by that backend from its own files.
    ///
    /// `today` is the render date `json_data` was compiled with. A backend
    /// whose plates can ask for the date answers with it, and the session keeps
    /// it for every [`update`](LiveSession::update).
    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
        today: Option<CalendarDate>,
    ) -> Result<LiveSession, RenderError>;
}

/// The refusal every backend owes a format outside its
/// [`Backend::supported_formats`], under `backend::format_not_supported`.
/// `backend` names the backend in the message; `supported` becomes the hint.
pub fn unsupported_format(format: OutputFormat, backend: &str, supported: &[OutputFormat]) -> RenderError {
    RenderError::coded_hint(
        "backend::format_not_supported",
        format!("{format:?} not supported by the {backend} backend"),
        format!("Supported formats: {supported:?}"),
    )
}

/// The diagnostic code a backend's own declined construct rides.
pub const DECLINED_CONSTRUCT: &str = "backend::declined_construct";

/// The warning a backend owes a content field holding a construct it typesets
/// nothing for: `count` of `construct` in the field `path` anchors, from
/// `backend`. One diagnostic per (field, construct), so a producer that sees
/// every occurrence at once collapses them into `count`. Non-fatal: the content
/// stores and round-trips, and it is the page that will not carry it.
pub fn declined_construct(
    backend: &str,
    construct: crate::quill::BlockConstruct,
    count: usize,
    path: &crate::path::DocPath,
) -> crate::error::Diagnostic {
    let mut args = std::collections::BTreeMap::new();
    args.insert("backend".to_string(), backend.into());
    args.insert("construct".to_string(), construct.as_str().into());
    args.insert("count".to_string(), count.into());
    crate::error::Diagnostic::new(
        crate::error::Severity::Warning,
        format!(
            "the {backend} backend does not typeset {}: {count} in this field \
             will not reach the page",
            plural(construct, count)
        ),
    )
    .with_code(DECLINED_CONSTRUCT.to_string())
    .with_path(path.to_string())
    .with_args(args)
}

/// English enough for the engine's own sentence; a consumer wording this
/// itself reads `construct` and `count` off `args` instead.
fn plural(construct: crate::quill::BlockConstruct, count: usize) -> String {
    use crate::quill::BlockConstruct;
    let name = match construct {
        BlockConstruct::Heading => "heading",
        BlockConstruct::Rule => "horizontal rule",
        BlockConstruct::Code => "code block",
        BlockConstruct::List => "list",
        BlockConstruct::Quote => "block quote",
        BlockConstruct::Table => "table",
        BlockConstruct::Image => "image",
    };
    if count == 1 {
        format!("a {name}")
    } else {
        format!("{name}s")
    }
}

/// The pixel ceiling on either side of one rasterized page, shared by every
/// raster path so a caller meets one number.
///
/// It is the floor across browser canvas limits (~32k a side on Chrome and
/// Firefox, 16k on Safari), and it bounds one page's RGBA buffer at 1 GiB: a quarter of wasm32's whole address
/// space, and far under the size at which the rasterizers' own dimension
/// arithmetic wraps.
pub const MAX_RASTER_SIDE: u32 = 16_384;

fn invalid_raster_scale(message: String, hint: &str) -> RenderError {
    RenderError::coded_hint("backend::invalid_raster_scale", message, hint)
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
/// positive and neither side of the `width_pt` × `height_pt` page passes
/// [`MAX_RASTER_SIDE`].
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
    if w.max(h) > f64::from(MAX_RASTER_SIDE) {
        return Err(invalid_raster_scale(
            format!(
                "a {width_pt}x{height_pt} pt page at {scale} device pixels per point is {w}x{h} px, past the {MAX_RASTER_SIDE} px ceiling on a side"
            ),
            "Rasterize fewer pixels: lower the ppi (the default is 144) or the canvas scale.",
        ));
    }
    Ok(())
}

/// `scale` reduced, where it must be, to the largest at which neither side of
/// the `width_pt` × `height_pt` page passes [`MAX_RASTER_SIDE`]: what a preview
/// paints at, where a softer page beats a refused one. A scale that is not
/// finite and positive is returned as given, for [`check_raster`] to refuse.
pub fn fit_raster_scale(scale: f32, width_pt: f32, height_pt: f32) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return scale;
    }
    scale.min(MAX_RASTER_SIDE as f32 / width_pt.max(height_pt))
}

/// The pages a render covers: `pages` as given, or every page of `page_count`
/// when it is `None`. An index at or past `page_count` fails under
/// `backend::page_index_out_of_bounds`, naming every offending index.
///
/// The indices come back as given: order and repeats are the caller's.
pub fn selected_pages(
    pages: Option<&[usize]>,
    page_count: usize,
) -> Result<Vec<usize>, RenderError> {
    let Some(requested) = pages else {
        return Ok((0..page_count).collect());
    };

    let out_of_bounds: Vec<usize> = requested
        .iter()
        .copied()
        .filter(|&i| i >= page_count)
        .collect();
    if !out_of_bounds.is_empty() {
        return Err(RenderError::coded_hint(
            "backend::page_index_out_of_bounds",
            format!(
                "Page index out of bounds (page_count={page_count}); offending indices: {out_of_bounds:?}"
            ),
            "Read the session's page count before requesting pages.",
        ));
    }

    Ok(requested.to_vec())
}

/// The refusal a backend owes a `pages` selection on a format it emits whole,
/// under `backend::page_selection_not_supported`. `format` names it in the
/// message.
pub fn page_selection_not_supported(format: OutputFormat) -> RenderError {
    RenderError::coded_hint(
        "backend::page_selection_not_supported",
        format!("{format:?} output does not support page selection"),
        "Drop the page selection to render the whole document, or ask for a per-page format.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RenderOptions;

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
    fn check_raster_refuses_a_page_past_the_side_ceiling() {
        let (w, h) = LETTER_PT;
        let ceiling = MAX_RASTER_SIDE as f32 / h;
        assert!(check_raster(ceiling, w, h).is_ok());
        assert_eq!(
            code(check_raster(ceiling * 1.01, w, h).expect_err("past the ceiling scale")),
            "backend::invalid_raster_scale"
        );
        assert_eq!(
            code(check_raster(f32::INFINITY, w, h).expect_err("an infinite scale")),
            "backend::invalid_raster_scale"
        );
    }

    #[test]
    fn a_fitted_scale_is_one_check_raster_admits() {
        for (w, h) in [LETTER_PT, (792.0, 612.0), (1.0, 14_400.0), (3.3, 7.7)] {
            for scale in [0.5, 2.0, 21.0, 1e6, f32::MAX] {
                let fitted = fit_raster_scale(scale, w, h);
                assert!(fitted <= scale);
                check_raster(fitted, w, h)
                    .unwrap_or_else(|_| panic!("{w}x{h} pt at {scale} fits at {fitted}"));
            }
        }
        assert_eq!(fit_raster_scale(2.0, LETTER_PT.0, LETTER_PT.1), 2.0);
        assert!(fit_raster_scale(f32::NAN, LETTER_PT.0, LETTER_PT.1).is_nan());
    }

    #[test]
    fn the_default_ppi_leaves_a_letter_page_far_under_the_ceiling() {
        let (w, h) = LETTER_PT;
        let scale = raster_scale(RenderOptions::DEFAULT_PPI).expect("the default ppi");
        check_raster(scale, w, h).expect("the default render is not near the ceiling");
        assert!(h * scale * 10.0 < MAX_RASTER_SIDE as f32);
    }

    #[test]
    fn no_selection_covers_every_page_and_a_selection_is_taken_verbatim() {
        assert_eq!(selected_pages(None, 3).unwrap(), [0, 1, 2]);
        assert_eq!(selected_pages(None, 0).unwrap(), [] as [usize; 0]);
        assert_eq!(selected_pages(Some(&[2, 0, 0]), 3).unwrap(), [2, 0, 0]);
    }

    #[test]
    fn a_page_past_the_document_is_refused() {
        let err = selected_pages(Some(&[0, 3]), 3).unwrap_err();
        assert_eq!(
            err.diagnostics()[0].code.as_deref(),
            Some("backend::page_index_out_of_bounds")
        );
    }
}
