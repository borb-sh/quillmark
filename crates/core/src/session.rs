use crate::quill::QuillConfig;
use crate::{
    ContentHit, Diagnostic, Document, RenderError, RenderOptions, RenderResult, RenderedRegion,
};
pub use quillmark_content::{ApplyError, Assoc, ChangeBundle, Delta, IslandOp, LineOp, MarkOp, Op};
use std::sync::OnceLock;

/// What a committed [`LiveSession::update`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// Page count after the edit.
    pub page_count: usize,
    /// Pages whose rendered content differs from the previous compile,
    /// including pages the edit added. Pages the edit removed are implied by
    /// `page_count`. A preview repaints `dirty ∩ visible` and nothing else.
    pub dirty_pages: Vec<usize>,
}

impl ChangeSet {
    pub fn new(page_count: usize, dirty_pages: Vec<usize>) -> Self {
        Self {
            page_count,
            dirty_pages,
        }
    }
}

/// Backend-specific session implementation. The `'static` bound prevents
/// borrowing source data: own anything the session must keep alive.
#[doc(hidden)]
pub trait SessionHandle: Send + Sync + 'static {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;

    /// Recompile the session against new document data.
    ///
    /// Transactional: on `Err` the previous compile stays live and every read
    /// keeps serving it. The returned [`ChangeSet`] reports the pages the edit
    /// visibly changed. Default: update is unsupported.
    fn update(&mut self, _json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        Err(RenderError::coded(
            "backend::update_unsupported",
            "this backend's session does not support update",
        ))
    }

    /// Page dimensions in points (1 pt = 1/72"), or `None` if `page` is out of
    /// range. The canvas-preview seam: a backend that can rasterize pages
    /// overrides this and [`render_rgba`](Self::render_rgba). Default `None`
    /// marks the session as having no canvas painter.
    ///
    /// One coordinate space serves the three canvas reads: the page's lower-left
    /// corner is the origin of this extent, of [`regions`](Self::regions), and of
    /// the [`render_rgba`](Self::render_rgba) raster, whose size is this extent
    /// times its `scale`. A backend drawing on a page whose own coordinates start
    /// elsewhere (a PDF background with a `/CropBox` or a translated
    /// `/MediaBox`) reports geometry relative to that corner.
    fn page_size_pt(&self, _page: usize) -> Option<(f32, f32)> {
        None
    }

    /// Render `page` to a non-premultiplied RGBA8 buffer at `scale`× the natural
    /// 72-ppi size, returning `(width_px, height_px, rgba)` (row-major, `w*h*4`
    /// bytes), or `Ok(None)` if `page` is out of range or the backend has no
    /// canvas painter. The other half of the seam paired with
    /// [`page_size_pt`](Self::page_size_pt).
    ///
    /// A backend that returns `Some` here guarantees a **complete** raster:
    /// every piece of page content is already in the returned pixels and the
    /// caller composites nothing. [`regions`](Self::regions) is for overlay and
    /// cross-navigation UIs, never required to complete the raster.
    ///
    /// A scale the page cannot be rasterized at is the `Err`: run it through
    /// [`check_raster`](crate::check_raster), which every raster path shares.
    ///
    /// A backend with no painter overrides neither this nor
    /// [`page_size_pt`](Self::page_size_pt), and
    /// [`LiveSession::supports_canvas`] derives the capability from that half
    /// of the seam rather than a separate flag.
    fn render_rgba(
        &self,
        _page: usize,
        _scale: f32,
    ) -> Result<Option<(u32, u32, Vec<u8>)>, RenderError> {
        Ok(None)
    }

    /// Schema-field geometry for the compiled session: [`RenderedRegion`]s
    /// keyed on the quill schema address each field carries.
    ///
    /// A session-level query, not a render output: computed from resolved
    /// field placements with no rasterization and no byte artifact. Default
    /// empty; a backend that places schema fields overrides this.
    ///
    /// Emit each content field's **first placement** (one region per page it
    /// touches) plus one region per widget and per scalar reference site, so
    /// `field` is not unique in the result. Order deterministically: widget
    /// regions first, then content regions in (page, field, site) order.
    fn regions(&self) -> Vec<RenderedRegion> {
        Vec::new()
    }

    /// The schema field whose content is under a point, or within `tol` points
    /// of one: the forward (click → field) direction of the region system.
    /// `x`/`y`/`tol` are PDF points and the origin is **bottom-left** on `page`,
    /// the same convention as [`RenderedRegion::rect`]. Unlike
    /// [`regions`](Self::regions), *every* placement should answer: one concrete
    /// point identifies one drawn item.
    ///
    /// `tol` is what a pointer cannot hit exactly, so a caller derives it from
    /// the scale it drew the page at rather than passing a document constant: a
    /// tolerance fixed in points shrinks on screen as the page does, which is
    /// where a target is already hardest to hit. Zero is exact containment, and
    /// widening it never changes an answer — the nearest placement wins and
    /// containment is distance zero.
    ///
    /// The default hit-tests [`regions`](Self::regions), which is complete only
    /// for a backend whose regions enumerate every placement. A backend that
    /// emits first-placement-only content must override this with a real
    /// document hit-test, or clicks on unenumerated placements dead-end.
    ///
    /// A tie goes to the last of [`regions`](Self::regions), the later-painted
    /// placement. That order lists widgets before content, so a backend placing
    /// both lanes overrides this to hand a widget the tie, as the Typst backend
    /// does.
    fn field_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<String> {
        let regions = self.regions();
        crate::region::nearest_region(&regions, page, x, y, tol).map(|(_, r)| r.field.clone())
    }

    /// A point → **content position** in a content field: the fine-grained
    /// twin of [`field_at`](Self::field_at) (which answers with the field
    /// alone). `x`/`y` are PDF points, bottom-left origin on `page`. Returns
    /// the field plus a USV offset into its `Content`, cluster-exact and
    /// degrading to the containing segment's start on origin-less ink (see
    /// [`ContentHit`]). `tol` reads as it does on [`field_at`](Self::field_at).
    /// `None` past `tol` from all content ink, on a scalar/widget (no content
    /// address), or when the backend maps no content. Default `None`: a backend
    /// that carries a per-segment source map overrides this.
    fn position_at(&self, _page: usize, _x: f32, _y: f32, _tol: f32) -> Option<ContentHit> {
        None
    }

    /// A content position → **caret rect** in a content field: the reverse of
    /// [`position_at`](Self::position_at). `pos` is a USV offset into `field`'s
    /// `Content`; the returned [`RenderedRegion`] is the box of the glyph the
    /// caret sits at, page-indexed, with `span` collapsed to `[pos, pos]`.
    /// `None` when `field` places no tracked content or `pos` maps to no drawn
    /// glyph. Default `None`: overridden by a backend with a source map.
    fn locate(&self, _field: &str, _pos: usize) -> Option<RenderedRegion> {
        None
    }

    /// Non-fatal diagnostics of the **current compile**. They swap with the
    /// compile on each committed [`update`](Self::update), so a failed update
    /// keeps the last-good compile's warnings alongside its document.
    fn warnings(&self) -> &[Diagnostic] {
        &[]
    }
}

/// Opaque, backend-backed live render session: a persistent compiler that
/// serves reads (`render`, `paint` seams, `regions`) from its current compile
/// and takes edits via [`update`](LiveSession::update). Reads between edits see
/// a stable document (`update` is transactional, swapping the compile only on
/// success) so immutability is an invariant between commits, not a type.
///
/// Geometry reads (`regions`, `position_at`, `locate`) resolve against the
/// current compile. Anchoring a caret or selection across edits is the editor's
/// job (its own transaction mapping): the session holds no change log and maps
/// no positions forward; a consumer re-reads geometry after each committed
/// [`update`](Self::update).
pub struct LiveSession {
    inner: Box<dyn SessionHandle>,
    /// Held as the config rather than the whole [`Quill`](crate::Quill)
    /// because the compile is a pure config read; the font and package bytes
    /// stay with the backend that needed them.
    config: QuillConfig,
    /// The current compile's geometry, rebuilt by the backend at most once per
    /// compile: invariant between commits, so the commit points clear it.
    regions: OnceLock<Vec<RenderedRegion>>,
}

impl LiveSession {
    /// Born bound: a session cannot exist without the schema it renders. The
    /// backend has the [`Quill`](crate::Quill) in hand inside
    /// [`Backend::open`](crate::Backend::open), so binding costs it a
    /// `source.config().clone()` and buys [`update`](Self::update) a document
    /// verb whose plate is always compiled by *this* config: the pairing is
    /// structural, not an obligation on the caller.
    #[doc(hidden)]
    pub fn new(inner: Box<dyn SessionHandle>, config: QuillConfig) -> Self {
        Self {
            inner,
            config,
            regions: OnceLock::new(),
        }
    }

    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// Whether this session can paint pages to a canvas, derived from the
    /// canvas seam rather than a separate flag. A canvas-capable backend with
    /// zero pages reports `false`.
    pub fn supports_canvas(&self) -> bool {
        self.inner.page_count() > 0 && self.inner.page_size_pt(0).is_some()
    }

    /// Page dimensions in points, or `None` if `page` is out of range or the
    /// backend has no canvas painter. Generalized canvas-preview seam; see
    /// [`SessionHandle::page_size_pt`].
    pub fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        self.inner.page_size_pt(page)
    }

    /// Rasterize `page` to non-premultiplied RGBA8 at `scale`× 72 ppi, or
    /// `Ok(None)` if `page` is out of range or the backend has no canvas
    /// painter. A `Some` result is a **complete** raster: all content visible,
    /// no caller-side compositing.
    ///
    /// `scale` is device pixels per point, and must be finite, positive, and
    /// small enough to keep the page under
    /// [`MAX_RASTER_PIXELS`](crate::MAX_RASTER_PIXELS); anything else is a
    /// `backend::invalid_raster_scale` refusal.
    pub fn render_rgba(
        &self,
        page: usize,
        scale: f32,
    ) -> Result<Option<(u32, u32, Vec<u8>)>, RenderError> {
        self.inner.render_rgba(page, scale)
    }

    /// Schema-field geometry for the compiled session: each content field's
    /// **first placement** (one [`RenderedRegion`] per page it touches), plus
    /// one region per `field:`-bound widget and per direct scalar reference
    /// site. Computed without rendering bytes; empty for backends that place no
    /// schema fields.
    ///
    /// `field` is not unique in the result, so group by it. Later placements of
    /// one content value are **not** enumerated; for point-driven lookup over
    /// any placement use [`field_at`](Self::field_at).
    ///
    /// Reflects the current compile; re-read after each committed
    /// [`update`](Self::update).
    pub fn regions(&self) -> Vec<RenderedRegion> {
        self.cached_regions().to_vec()
    }

    fn cached_regions(&self) -> &[RenderedRegion] {
        self.regions.get_or_init(|| self.inner.regions())
    }

    /// The whole-field highlight boxes for `field`: one union rect per page,
    /// over the field's `span`-bearing content segments. Content only: a field
    /// placed solely as a scalar reference or a bound widget yields nothing
    /// here, its box being a single [`regions`](Self::regions) rect.
    pub fn field_boxes(&self, field: &str) -> Vec<RenderedRegion> {
        crate::field_boxes(self.cached_regions(), field)
    }

    /// The schema field whose content is under a point on `page`, or within
    /// `tol` of one. `x`/`y`/`tol` are PDF points with a **bottom-left**
    /// origin, as [`RenderedRegion::rect`]. Every placement answers, not just
    /// the first surfaced by [`regions`](Self::regions). `None` past `tol` from
    /// any field's ink, out of range, or for backends that place no schema
    /// fields.
    ///
    /// The nearest placement answers, so `tol` only ever fills a miss: a point
    /// inside ink is at distance zero and keeps that ink whatever `tol` is.
    /// Pass what a pointer's imprecision is worth at the scale the page was
    /// drawn at — a screen-space slack converted to points, not a constant.
    pub fn field_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<String> {
        self.inner.field_at(page, x, y, tol)
    }

    /// A point → **content position**: the field *and* a USV offset into its
    /// `Content`. The offset is cluster-exact and degrades to the containing
    /// segment's start on origin-less ink. `tol` reads as on
    /// [`field_at`](Self::field_at), and buys the most here: the leading
    /// between two lines is inside a paragraph and on no glyph.
    /// `None` past `tol` from all content ink, on a scalar/widget, or for
    /// backends with no content map.
    ///
    /// Resolves against the current compile; the editor anchors the caret it
    /// places across later edits itself.
    pub fn position_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<ContentHit> {
        self.inner.position_at(page, x, y, tol)
    }

    /// A content position → **caret rect**, the reverse of
    /// [`position_at`](Self::position_at): given a field and a USV offset into
    /// its `Content`, return the box (page-indexed) to draw a caret at. `None`
    /// when the field places no tracked content or the offset maps to no drawn
    /// glyph. Resolves against the current compile.
    pub fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
        self.inner.locate(field, pos)
    }

    /// Non-fatal diagnostics of the session's **current compile**, refreshed by
    /// each committed [`update`](Self::update); a failed update keeps the
    /// last-good compile *and* its warnings. Also appended to
    /// [`RenderResult::warnings`], for consumers that never call `render`.
    pub fn warnings(&self) -> &[Diagnostic] {
        self.inner.warnings()
    }

    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let mut result = self.inner.render(opts)?;
        result
            .warnings
            .extend(self.inner.warnings().iter().cloned());
        // Attached at the wrapper, so a backend needs nothing beyond the
        // `regions` accessor it already has.
        if opts.regions {
            result.regions = self.regions();
        }
        Ok(result)
    }

    /// Recompile the session against new document data. Transactional: on
    /// `Err` the previous compile stays live, so every read keeps serving the
    /// last-good document and its [`warnings`](Self::warnings).
    ///
    /// `doc` is checked against the session's quill and compiled through the
    /// same pipeline as the first compile, so an edit cannot reach the backend
    /// under a schema the session was not opened against.
    pub fn update(&mut self, doc: &Document) -> Result<ChangeSet, RenderError> {
        let json_data = self.config.compile_checked(doc)?;
        self.regions.take();
        self.inner.update(&json_data)
    }

    /// [`update`](Self::update) with the schema layer cut away: plate data
    /// straight to the backend, no `$quill` check and no compile.
    ///
    /// For a backend's own acceptance tests, which drive a session against
    /// synthetic plate data — including data a schema would reject, the only
    /// lever that makes a backend's compile fail on demand.
    ///
    /// Feature-gated rather than merely `#[doc(hidden)]`, so the seam is absent
    /// from a consumer build unless that crate asks for it.
    #[cfg(feature = "internal-test-seam")]
    #[doc(hidden)]
    pub fn update_data(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        self.regions.take();
        self.inner.update(json_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::QuillReference;
    use crate::Severity;
    use std::str::FromStr;

    const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Session test quill
main:
  fields:
    subject:
      type: plaintext
";

    fn config() -> QuillConfig {
        QuillConfig::from_yaml(QUILL_YAML).expect("valid quill")
    }

    fn doc() -> Document {
        Document::new(QuillReference::from_str("memo@1.0.0").unwrap())
    }

    /// Canvas-capable: overrides the seam for `pages` pages.
    struct CanvasHandle {
        pages: usize,
    }
    impl SessionHandle for CanvasHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by capability tests")
        }
        fn page_count(&self) -> usize {
            self.pages
        }
        fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
            (page < self.pages).then_some((612.0, 792.0))
        }
    }

    /// Non-canvas: leaves the seam at its `None` defaults.
    struct PlainHandle;
    impl SessionHandle for PlainHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by capability tests")
        }
        fn page_count(&self) -> usize {
            1
        }
    }

    /// One warning per committed update.
    struct WarningHandle {
        current: Vec<Diagnostic>,
        applies: usize,
    }
    impl SessionHandle for WarningHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            Ok(RenderResult::new(Vec::new(), crate::OutputFormat::Pdf))
        }
        fn page_count(&self) -> usize {
            1
        }
        fn update(&mut self, _: &serde_json::Value) -> Result<ChangeSet, RenderError> {
            self.applies += 1;
            self.current = vec![Diagnostic::new(
                Severity::Warning,
                format!("warning of compile {}", self.applies),
            )];
            Ok(ChangeSet {
                page_count: 1,
                dirty_pages: vec![],
            })
        }
        fn warnings(&self) -> &[Diagnostic] {
            &self.current
        }
    }

    #[test]
    fn warnings_track_current_compile() {
        let open_warning = vec![Diagnostic::new(Severity::Warning, "open-time".to_string())];
        let mut session = LiveSession::new(
            Box::new(WarningHandle {
                current: open_warning,
                applies: 0,
            }),
            config(),
        );
        assert_eq!(session.warnings()[0].message, "open-time");

        session.update(&doc()).unwrap();
        assert_eq!(session.warnings()[0].message, "warning of compile 1");

        let result = session.render(&RenderOptions::default()).unwrap();
        assert_eq!(result.warnings[0].message, "warning of compile 1");
    }

    struct RegionHandle;
    impl SessionHandle for RegionHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by geometry tests")
        }
        fn page_count(&self) -> usize {
            1
        }
        fn regions(&self) -> Vec<RenderedRegion> {
            vec![RenderedRegion {
                field: "subject".to_string(),
                page: 0,
                rect: [1.0, 2.0, 3.0, 4.0],
                span: Some([0, 3]),
            }]
        }
        fn position_at(&self, _: usize, _: f32, _: f32, _: f32) -> Option<ContentHit> {
            Some(ContentHit {
                field: "subject".to_string(),
                pos: 2,
                granularity: Some(crate::HitGranularity::Cluster),
            })
        }
        fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
            Some(RenderedRegion {
                field: field.to_string(),
                page: 0,
                rect: [1.0, 2.0, 1.0, 4.0],
                span: Some([pos, pos]),
            })
        }
    }

    /// Two regions on one rect, so every hit inside it is a tie.
    struct TiedRegionHandle;
    impl SessionHandle for TiedRegionHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by geometry tests")
        }
        fn page_count(&self) -> usize {
            1
        }
        fn regions(&self) -> Vec<RenderedRegion> {
            ["under", "over"]
                .into_iter()
                .map(|field| RenderedRegion::new(field.to_string(), 0, [0.0, 0.0, 10.0, 10.0]))
                .collect()
        }
    }

    #[test]
    fn field_at_tie_takes_the_later_region() {
        let session = LiveSession::new(Box::new(TiedRegionHandle), config());
        assert_eq!(session.field_at(0, 5.0, 5.0, 0.0).as_deref(), Some("over"));
        // Outside both rects by the same gap: the tolerant path ties too.
        assert_eq!(session.field_at(0, 14.0, 5.0, 8.0).as_deref(), Some("over"));
    }

    #[test]
    fn field_boxes_derives_off_regions() {
        let session = LiveSession::new(Box::new(RegionHandle), config());
        let boxes = session.field_boxes("subject");
        assert_eq!(boxes.len(), 1, "one span-bearing region → one box");
        assert_eq!(boxes[0].field, "subject");
        assert!(session.field_boxes("nope").is_empty());
    }

    #[test]
    fn supports_canvas_derives_from_seam() {
        let canvas = LiveSession::new(Box::new(CanvasHandle { pages: 2 }), config());
        assert!(canvas.supports_canvas());
        let plain = LiveSession::new(Box::new(PlainHandle), config());
        assert!(!plain.supports_canvas());
        // A canvas backend with no pages has nothing to paint.
        let empty = LiveSession::new(Box::new(CanvasHandle { pages: 0 }), config());
        assert!(!empty.supports_canvas());
    }
}
