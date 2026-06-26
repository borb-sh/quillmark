//! Page composition: the "compose first, stamp last" primitive for
//! continuation/overflow pages.
//!
//! [`compose`] concatenates every page of every source PDF into one document
//! whose catalog has only `/Pages` — **no `/AcroForm`**. You then hand the
//! result to [`crate::stamp`], which writes the form fresh over the combined
//! page set. This sidesteps merging a live AcroForm entirely: there's nothing
//! to preserve, because the form is added afterward.
//!
//! Page content is imported via `hayro-write`'s `extract` (objects + their
//! dependency closure, renumbered through a shared ref counter), so a single
//! field can later be stamped onto any page regardless of which source it came
//! from. The output uses `pdf-writer`'s traditional xref table, which
//! [`crate::stamp`]'s incremental appender accepts.
//!
//! Trade-off: unlike single-form stamping (which byte-preserves the background
//! via incremental append), composition re-serializes every page through
//! `pdf-writer`. Content is preserved structurally, never rasterized, but the
//! bytes are re-emitted — so reserve composition for the continuation case.

use std::cell::RefCell;
use std::rc::Rc;

use hayro_write::hayro_syntax::Pdf;
use hayro_write::{extract, ExtractionQuery, ExtractionResult};
use pdf_writer::{Pdf as PdfWriter, Ref};
use quillmark_core::{Diagnostic, RenderError, Severity};

const CODE: &str = "pdf::compose";

fn err(msg: impl Into<String>) -> RenderError {
    RenderError::CompilationFailed {
        diags: vec![Diagnostic::new(Severity::Error, msg.into()).with_code(CODE.to_string())],
    }
}

/// Concatenate every page of every PDF in `sources`, in order, into one
/// field-free PDF (catalog → `/Pages` only). Feed the result to [`crate::stamp`].
///
/// Errors if `sources` is empty, a source won't parse, or a page can't be
/// extracted.
pub fn compose(sources: &[&[u8]]) -> Result<Vec<u8>, RenderError> {
    if sources.is_empty() {
        return Err(err("compose requires at least one source PDF"));
    }

    let pdfs: Vec<Pdf> = sources
        .iter()
        .enumerate()
        .map(|(i, bytes)| {
            Pdf::new(bytes.to_vec())
                .map_err(|e| err(format!("source {i} is not a readable PDF: {e:?}")))
        })
        .collect::<Result<_, _>>()?;

    // One ref allocator shared across the catalog, the page-tree nodes, and
    // every per-source `extract` call, so imported objects never collide.
    let counter = Rc::new(RefCell::new(Ref::new(1)));
    let catalog_id = counter.borrow_mut().bump();
    let root_pages_id = counter.borrow_mut().bump();

    let mut extracted: Vec<(usize, ExtractionResult)> = Vec::with_capacity(pdfs.len());
    let mut total_pages = 0usize;
    for pdf in &pdfs {
        let n = pdf.pages().len();
        total_pages += n;
        let queries: Vec<ExtractionQuery> = (0..n).map(ExtractionQuery::new_page).collect();
        let c = counter.clone();
        let result = extract(
            pdf,
            Box::new(move || c.borrow_mut().bump()),
            Default::default(),
            // Unused for page queries (only XObject extraction calls it).
            |_group: &mut pdf_writer::writers::Group| {},
            &queries,
        )
        .map_err(|e| err(format!("page extraction failed: {e:?}")))?;
        // Surface any per-page extraction failure rather than unwrapping later.
        for r in &result.root_refs {
            r.map_err(|e| err(format!("page extraction failed: {e:?}")))?;
        }
        extracted.push((n, result));
    }

    let mut out = PdfWriter::new();
    out.catalog(catalog_id).pages(root_pages_id);
    // Root /Pages → one intermediate node per source (a clean 2-level tree;
    // the extracted pages already point /Parent at their source's node).
    out.pages(root_pages_id)
        .kids(extracted.iter().map(|(_, r)| r.page_tree_parent_ref))
        .count(total_pages as i32);
    for (n, result) in &extracted {
        out.pages(result.page_tree_parent_ref)
            .kids(result.root_refs.iter().map(|r| r.expect("validated above")))
            .count(*n as i32)
            .parent(root_pages_id);
    }
    for (_, result) in &extracted {
        out.extend(&result.chunk);
    }
    Ok(out.finish())
}
