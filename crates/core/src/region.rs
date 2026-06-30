//! Schema-field geometry, queried from a compiled
//! [`RenderSession`](crate::RenderSession) via
//! [`regions`](crate::RenderSession::regions).
//!
//! A region ties a rectangle on the rendered page to the **quill schema field**
//! that produced it — the address the document author already uses to refer to
//! that field (the same address the Typst plate reads as `data.*` and the
//! pdfform binder resolves against `compile_data`). It exists so a consumer can
//! map between a place on the page and a field in the editor: click a rendered
//! field → focus it in the editor, or highlight the page rectangle for the
//! focused field.
//!
//! Two producers feed regions, both keyed on the schema path:
//!
//! - **Content fields** (a markdown body) auto-tag from their content — the
//!   Typst eval site brackets each one with its schema address, so the key is
//!   carried by construction with no plate-author effort. A computed scalar
//!   (`data.from + ", " + rank`) cannot be tagged (a string has no label), so
//!   it produces no region.
//! - **Form-field widgets** carry a schema path explicitly: pdfform binds it
//!   from the form mapping; a Typst `form-field` binds it from its `field:`
//!   argument. **Caveat:** a Typst widget that omits `field:` falls back to its
//!   `/T` widget name, which is the schema address only when the two coincide —
//!   bind `field:` when they differ. A pdfform widget with no schema binding
//!   (`schema_field: null`) is decorative and produces no region.
//!
//! **A field may produce more than one region.** Content that breaks across
//! pages emits one region per page-fragment, all sharing the same `field` (the
//! shape Typst's own PDF link annotations use, for the same reason — a single
//! bbox over a line-broken span would span the whole paragraph). Consumers
//! group by `field`. A form-field widget is one fixed box, so it stays a single
//! entry; both kinds coexist in one `Vec`.
//!
//! Regions are a session-level query, not a render output: the geometry is a
//! property of the compiled snapshot, read once from the session without
//! producing any byte artifact. Only the interactive-preview path wants it (to
//! lay out overlays over a `paint`-ed canvas); a one-shot byte render
//! (PDF/PNG/SVG) never does. They are an overlay sidecar, never a compositing
//! input: both canvas backends hand back a complete page raster, so nothing
//! about the picture depends on reading a region. Empty for backends that place
//! no schema fields.

/// One schema field's placement on one rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin —
/// the same final geometry the stamp spine writes to the widget `/Rect`, so the
/// region and the rendered field describe the identical box.
///
/// `field` is not unique across a `Vec<RenderedRegion>`: a content field that
/// breaks across pages repeats it, one entry per page-fragment. Group by
/// `(field, page)` to recover a field's full footprint.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedRegion {
    /// Quill schema field path, e.g. `"signature_block"` or
    /// `"$cards.indorsement.1.from"` — the author-facing field address (the
    /// card form is kind + 0-based ordinal, `$cards.<kind>.<n>.<field>`), not
    /// any backend widget name.
    pub field: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]`, PDF points, bottom-left origin.
    pub rect: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_round_trips_through_json() {
        let region = RenderedRegion {
            field: "full_name".to_string(),
            page: 0,
            rect: [180.0, 715.0, 520.0, 735.0],
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"field\":\"full_name\""), "{json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }
}
