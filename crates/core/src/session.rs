use crate::{
    CorpusHit, Diagnostic, RenderError, RenderOptions, RenderResult, RenderedRegion, Severity,
};
pub use quillmark_richtext::{Assoc, ChangeLog, Delta, FieldChange, StaleRevision};

/// What a committed [`LiveSession::apply`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// Page count after the edit.
    pub page_count: usize,
    /// Pages whose rendered content differs from the previous compile,
    /// including pages the edit added. Pages the edit removed are implied by
    /// `page_count`. A preview repaints `dirty ∩ visible` and nothing else.
    pub dirty_pages: Vec<usize>,
}

/// Backend-specific session implementation.
///
/// Implementors must be `'static`, `Send`, and `Sync`. The `'static` bound
/// prevents borrowing source data — own anything you need to keep alive for
/// the session's lifetime.
#[doc(hidden)]
pub trait SessionHandle: Send + Sync + 'static {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError>;
    fn page_count(&self) -> usize;

    /// Recompile the session against new document data.
    ///
    /// Transactional: on `Err` the previous compile stays live — every read
    /// (`render`, `render_rgba`, `page_size_pt`, `regions`) keeps serving it.
    /// A backend with a persistent compilation environment recompiles
    /// incrementally; one whose compile is cheap recompiles fully. Either way
    /// the returned [`ChangeSet`] reports the pages the edit visibly changed.
    /// Default: apply is unsupported.
    fn apply(&mut self, _json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        Err(RenderError::from_diag(
            Diagnostic::new(
                Severity::Error,
                "this backend's session does not support apply".to_string(),
            )
            .with_code("backend::apply_unsupported".to_string()),
        ))
    }

    /// Page dimensions in points (1 pt = 1/72"), or `None` if `page` is out of
    /// range. The canvas-preview seam: a backend that can rasterize pages
    /// overrides this and [`render_rgba`](Self::render_rgba). Default `None`
    /// marks the session as having no canvas painter — the painter dispatches
    /// generically through these two methods rather than downcasting to a
    /// backend-specific session type.
    fn page_size_pt(&self, _page: usize) -> Option<(f32, f32)> {
        None
    }

    /// Render `page` to a non-premultiplied RGBA8 buffer at `scale`× the natural
    /// 72-ppi size, returning `(width_px, height_px, rgba)` (row-major, `w*h*4`
    /// bytes), or `None` if `page` is out of range or the backend has no canvas
    /// painter. The other half of the seam paired with
    /// [`page_size_pt`](Self::page_size_pt).
    ///
    /// # Per-backend contract
    ///
    /// A backend that returns `Some` here guarantees a **complete** raster of
    /// the page: every piece of page content is already visible in the returned
    /// pixels. The caller paints them straight to a canvas with **no
    /// compositing** of its own. Backends satisfy this differently:
    ///
    /// - **Typst** rasterizes its laid-out page natively.
    /// - **pdfform** pre-flattens the bound field values into the page content
    ///   streams at session-open, then rasterizes that flat PDF — so field
    ///   values appear in the raster without the caller drawing them.
    ///
    /// The [`regions`](Self::regions) accessor carries per-field geometry keyed
    /// on the quill schema field path, for *overlay* / cross-navigation UIs
    /// regardless; it is never required to make the raster complete.
    ///
    /// A backend with no painter overrides neither this nor
    /// [`page_size_pt`](Self::page_size_pt); the defaults mark the session as
    /// non-canvas, which is exactly what [`LiveSession::supports_canvas`]
    /// reports. Capability is derived from the `page_size_pt` half of this seam,
    /// not declared as a separate flag — a canvas backend is contractually
    /// expected to pair this method with `page_size_pt` over the same page set.
    fn render_rgba(&self, _page: usize, _scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }

    /// Schema-field geometry for the compiled session — [`RenderedRegion`]s
    /// keyed on the quill schema address each field carries.
    ///
    /// A session-level query, not a render output: the geometry is a property of
    /// the current compile, computed from already-resolved field placements
    /// with no rasterization and no byte artifact. An interactive preview reads
    /// it to lay out overlays / field cross-navigation over a `paint`-ed canvas;
    /// a one-shot byte render carries it only on request
    /// ([`RenderOptions::regions`](crate::RenderOptions)). Default empty — a
    /// backend that places schema fields overrides this.
    ///
    /// Emit each content field's **first placement** — one region per page
    /// that placement touches — plus one region per widget and per scalar
    /// reference site. `field` is still not unique in the result: page
    /// fragments, several scalar sites, or tracked content plus a bound
    /// widget each surface independently ([`LiveSession::regions`] passes
    /// them through; consumers group by `field`). Order deterministically:
    /// widget regions first, then content regions in (page, field, site)
    /// order.
    fn regions(&self) -> Vec<RenderedRegion> {
        Vec::new()
    }

    /// The schema field whose content is under a point — the forward
    /// (click → field) direction of the region system. `x`/`y` are PDF points
    /// with a **bottom-left** origin on `page`, the same convention as
    /// [`RenderedRegion::rect`]. Unlike [`regions`](Self::regions), the
    /// intent is that *every* placement answers, not just the first: one
    /// concrete point identifies one drawn item, whose origin is unambiguous
    /// however many times its field is placed.
    ///
    /// Default: hit-test [`regions`](Self::regions) — complete only for a
    /// backend whose regions enumerate every placement (widget-only backends
    /// like pdfform), and empty when `regions` is. A backend whose regions
    /// under-enumerate relative to its placements — first-placement-only
    /// content emission, like Typst's — must override this with a real
    /// document hit-test, or clicks on unenumerated placements dead-end.
    fn field_at(&self, page: usize, x: f32, y: f32) -> Option<String> {
        self.regions()
            .into_iter()
            .find(|r| r.contains(page, x, y))
            .map(|r| r.field)
    }

    /// A point → **corpus position** in a content field — the fine-grained
    /// twin of [`field_at`](Self::field_at) (which answers with the field
    /// alone). `x`/`y` are PDF points, bottom-left origin on `page`. Returns
    /// the field plus a USV offset into its `RichText`, cluster-exact and
    /// degrading to the containing segment's start on origin-less ink (see
    /// [`CorpusHit`]). `None` off all content ink, on a scalar/widget (no
    /// corpus address), or when the backend maps no corpus. Default `None` —
    /// a backend that carries a per-segment source map overrides this.
    fn position_at(&self, _page: usize, _x: f32, _y: f32) -> Option<CorpusHit> {
        None
    }

    /// A corpus position → **caret rect** in a content field — the reverse of
    /// [`position_at`](Self::position_at). `pos` is a USV offset into `field`'s
    /// `RichText`; the returned [`RenderedRegion`] is the box of the glyph the
    /// caret sits at, page-indexed, with `span` collapsed to `[pos, pos]`.
    /// `None` when `field` places no tracked content or `pos` maps to no drawn
    /// glyph. Default `None` — overridden by a backend with a source map.
    fn locate(&self, _field: &str, _pos: usize) -> Option<RenderedRegion> {
        None
    }

    /// Non-fatal diagnostics of the **current compile**. A backend whose
    /// compile emits warnings (Typst: font fallback, overfull pages, …)
    /// overrides this to expose them; they swap with the compile on each
    /// committed [`apply`](Self::apply), so a failed apply keeps the last-good
    /// compile's warnings alongside its document. Default empty — a backend
    /// whose compile cannot warn leaves it.
    fn warnings(&self) -> &[Diagnostic] {
        &[]
    }
}

/// Opaque, backend-backed live render session: a persistent compiler that
/// serves reads (`render`, `paint` seams, `regions`) from its current compile
/// and takes edits via [`apply`](LiveSession::apply). Reads between edits see
/// a stable document — `apply` is transactional, swapping the compile only on
/// success — so immutability is an invariant between commits, not a type.
///
/// Phase 3 adds a monotonic [`revision`](Self::revision) and a bounded
/// [`change_log`](Self::change_log) of per-field text deltas so stale corpus
/// positions map forward via [`map_field_pos`](Self::map_field_pos) instead of
/// silently reading the current compile.
pub struct LiveSession {
    inner: Box<dyn SessionHandle>,
    change_log: ChangeLog,
}

impl LiveSession {
    #[doc(hidden)]
    pub fn new(inner: Box<dyn SessionHandle>) -> Self {
        Self {
            inner,
            change_log: ChangeLog::with_default_capacity(),
        }
    }

    /// Monotonic edit revision — `0` before the first recorded field delta.
    pub fn revision(&self) -> u64 {
        self.change_log.revision()
    }

    /// Bounded ring of per-field text deltas since session open.
    pub fn change_log(&self) -> &ChangeLog {
        &self.change_log
    }

    /// Record a committed field text splice; returns the new revision. Mark and
    /// line op channels (PR-D) extend the log entry shape later.
    pub fn record_field_delta(
        &mut self,
        path: impl Into<String>,
        text_delta: Delta,
    ) -> u64 {
        self.change_log.record(path, text_delta)
    }

    /// Map a USV position in `field` forward from `base_revision` through
    /// subsequent recorded text deltas for that field.
    pub fn map_field_pos(
        &self,
        field: &str,
        base_revision: u64,
        pos: usize,
        assoc: Assoc,
    ) -> Result<usize, StaleRevision> {
        self.change_log.map_pos(field, base_revision, pos, assoc)
    }

    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// Whether this session can paint pages to a canvas — the authoritative,
    /// session-level capability. Derived directly from the canvas seam (a
    /// painter exposes [`page_size_pt`](SessionHandle::page_size_pt) for its
    /// pages), so there is no separate capability flag to keep in sync: a
    /// canvas backend pairs [`render_rgba`](Self::render_rgba) with
    /// `page_size_pt`, so this reflects what `paint` will do. A canvas-capable
    /// backend with zero pages reports `false` (nothing to paint).
    ///
    /// For a pre-session estimate (no open session yet), see
    /// [`formats_support_canvas`](crate::formats_support_canvas).
    pub fn supports_canvas(&self) -> bool {
        self.inner.page_count() > 0 && self.inner.page_size_pt(0).is_some()
    }

    /// Page dimensions in points, or `None` if `page` is out of range or the
    /// backend has no canvas painter. Generalized canvas-preview seam — see
    /// [`SessionHandle::page_size_pt`].
    pub fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        self.inner.page_size_pt(page)
    }

    /// Rasterize `page` to non-premultiplied RGBA8 at `scale`× 72 ppi, or `None`
    /// if `page` is out of range or the backend has no canvas painter. A `Some`
    /// result is a **complete** raster of the page — all content visible, no
    /// caller-side compositing — per the per-backend contract on
    /// [`SessionHandle::render_rgba`].
    pub fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        self.inner.render_rgba(page, scale)
    }

    /// Schema-field geometry for the compiled session — each content field's
    /// **first placement** (one [`RenderedRegion`] per page it touches), plus
    /// one region per `field:`-bound widget and per direct scalar reference
    /// site, keyed on the quill schema field path. A session-level query
    /// computed without rendering bytes; an interactive preview reads it to
    /// scroll to / highlight the focused field over a `paint`-ed canvas.
    /// Empty for backends that place no schema fields.
    ///
    /// `field` is still not unique in the result: a placement breaking across
    /// pages surfaces one fragment per page (a highlight covers continuation
    /// pages), a scalar referenced at several plate sites surfaces each site,
    /// and a field arising from both tracked content and a bound widget
    /// surfaces both (overlapping rects that route to the same field). Group
    /// by `field`; every entry routes to that field in the editor. Later
    /// placements of one content value are **not** enumerated — for
    /// point-driven lookup over any placement, use
    /// [`field_at`](Self::field_at).
    pub fn regions(&self) -> Vec<RenderedRegion> {
        self.inner.regions()
    }

    /// The schema field whose content is under a point on `page` — the
    /// forward (click → field) direction: hit-test a click against the
    /// compiled document and get back the field address to focus in the
    /// editor. `x`/`y` are PDF points with a **bottom-left** origin, the same
    /// convention as [`RenderedRegion::rect`] (a canvas consumer applies the
    /// inverse of the overlay transform it already uses for regions). Every
    /// placement answers, not just the first surfaced by
    /// [`regions`](Self::regions). `None` off any field's ink, out of range,
    /// or for backends that place no schema fields.
    pub fn field_at(&self, page: usize, x: f32, y: f32) -> Option<String> {
        self.inner.field_at(page, x, y)
    }

    /// A point → **corpus position** — the fine-grained click direction:
    /// hit-test a point and get back the field *and* a USV offset into its
    /// `RichText`, for placing a caret or mapping a selection into the content
    /// model. `x`/`y` are PDF points, bottom-left origin, the same convention
    /// as [`field_at`](Self::field_at). The offset is cluster-exact and
    /// degrades to the containing segment's start on origin-less ink (list
    /// markers, a code fence's interior). `None` off all content ink, on a
    /// scalar/widget, or for backends with no corpus map. See [`CorpusHit`].
    pub fn position_at(&self, page: usize, x: f32, y: f32) -> Option<CorpusHit> {
        self.inner.position_at(page, x, y)
    }

    /// A corpus position → **caret rect** — the reverse of
    /// [`position_at`](Self::position_at): given a field and a USV offset into
    /// its `RichText`, return the box (page-indexed) to draw a caret at. `None`
    /// when the field places no tracked content or the offset maps to no drawn
    /// glyph.
    pub fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
        self.inner.locate(field, pos)
    }

    /// Non-fatal diagnostics of the session's **current compile** — set at
    /// `Backend::open` and refreshed by each committed [`apply`](Self::apply);
    /// a failed apply keeps the last-good compile *and* its warnings. Also
    /// appended to [`RenderResult::warnings`] on each
    /// [`render`](Self::render) call. Exposed for consumers (e.g. canvas
    /// previews) that never call `render()`.
    pub fn warnings(&self) -> &[Diagnostic] {
        self.inner.warnings()
    }

    pub fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let mut result = self.inner.render(opts)?;
        result
            .warnings
            .extend(self.inner.warnings().iter().cloned());
        // The regions sidecar is attached here, at the wrapper, so every
        // backend's one-shot render carries it without implementing anything
        // beyond the `regions` accessor it already has.
        if opts.regions {
            result.regions = self.inner.regions();
        }
        Ok(result)
    }

    /// Recompile the session against new document data — the edit verb of a
    /// live preview. Transactional: on `Err` the previous compile stays live,
    /// so every read keeps serving the last-good document and its
    /// [`warnings`](Self::warnings); on `Ok` the session serves the new
    /// compile — warnings included — and the [`ChangeSet`] reports what
    /// changed. Pass data compiled by the same schema pipeline as
    /// `Backend::open`'s `json_data` (`Quill::compile_data`) — and from the
    /// *same quill*: the `$quill` reference check lives at the layer that
    /// still holds a `Document` (`Quillmark::open`, the WASM `apply`);
    /// compiled data does not carry the reference, so this seam cannot
    /// re-check it.
    pub fn apply(&mut self, json_data: &serde_json::Value) -> Result<ChangeSet, RenderError> {
        self.inner.apply(json_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canvas-capable session: overrides the seam for `pages` pages.
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

    /// A non-canvas session: leaves the seam at its `None` defaults.
    struct PlainHandle;
    impl SessionHandle for PlainHandle {
        fn render(&self, _: &RenderOptions) -> Result<RenderResult, RenderError> {
            unimplemented!("render is not exercised by capability tests")
        }
        fn page_count(&self) -> usize {
            1
        }
    }

    /// A warning-emitting session: `warnings` reflects the current compile
    /// (one warning per committed apply), and `render` succeeds empty.
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
        fn apply(&mut self, _: &serde_json::Value) -> Result<ChangeSet, RenderError> {
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

    /// `LiveSession::warnings` reflects the handle's current compile —
    /// refreshed by a committed apply — and `render` appends the same set to
    /// `RenderResult::warnings`.
    #[test]
    fn warnings_track_current_compile() {
        let open_warning = vec![Diagnostic::new(Severity::Warning, "open-time".to_string())];
        let mut session = LiveSession::new(Box::new(WarningHandle {
            current: open_warning,
            applies: 0,
        }));
        assert_eq!(session.warnings()[0].message, "open-time");

        session.apply(&serde_json::Value::Null).unwrap();
        assert_eq!(session.warnings()[0].message, "warning of compile 1");

        let result = session.render(&RenderOptions::default()).unwrap();
        assert_eq!(result.warnings[0].message, "warning of compile 1");
    }

    #[test]
    fn field_delta_revision_and_map_pos() {
        use quillmark_richtext::delta::diff;

        let mut session = LiveSession::new(Box::new(PlainHandle));
        assert_eq!(session.revision(), 0);

        let d = diff("abcdef", "abcXYdef");
        session.record_field_delta("subject", d);
        assert_eq!(session.revision(), 1);
        assert_eq!(
            session.map_field_pos("subject", 0, 3, Assoc::Before).unwrap(),
            3
        );
        assert_eq!(
            session.map_field_pos("subject", 0, 3, Assoc::After).unwrap(),
            5
        );
    }

    #[test]
    fn supports_canvas_derives_from_seam() {
        // A session that exposes page geometry is canvas-capable…
        let canvas = LiveSession::new(Box::new(CanvasHandle { pages: 2 }));
        assert!(canvas.supports_canvas());
        // …one that leaves the seam at its defaults is not…
        let plain = LiveSession::new(Box::new(PlainHandle));
        assert!(!plain.supports_canvas());
        // …and a canvas backend with no pages has nothing to paint.
        let empty = LiveSession::new(Box::new(CanvasHandle { pages: 0 }));
        assert!(!empty.supports_canvas());
    }
}
