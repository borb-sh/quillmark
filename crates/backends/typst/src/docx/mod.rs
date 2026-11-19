//! Word export of a compiled document.
//!
//! The export reads a second compile of the session's sources with
//! [`probes`] in the library, which record what Typst lays out without
//! tagging it. Their ink must equal the live compile's; when it does not, the
//! live document is read instead, without probes, under
//! `docx::probes_disabled`.

pub(crate) mod probes;
pub(crate) mod read;

use quillmark_core::error::{Diagnostic, Severity};
use typst_layout::PagedDocument;

use crate::world::QuillWorld;

const CODE_PROBES_DISABLED: &str = "docx::probes_disabled";

/// The probed compile of `world` when its ink equals `live`'s, else `None`
/// and the `docx::probes_disabled` warning.
pub(crate) fn probed(
    live: &PagedDocument,
    world: &QuillWorld,
) -> Result<PagedDocument, Diagnostic> {
    world
        .probed()
        .and_then(|w| crate::compile::compile_document(&w).ok())
        .map(|(doc, _)| doc)
        .filter(|doc| crate::ink_hashes(doc) == crate::ink_hashes(live))
        .ok_or_else(|| {
            Diagnostic::new(
                Severity::Warning,
                "the probed compile did not lay out the document's ink, so the Word file is \
                 read without its probes and carries less structure"
                    .to_string(),
            )
            .with_code(CODE_PROBES_DISABLED.to_string())
        })
}

/// The `.docx` bytes for `live` and the warnings the export raises.
pub(crate) fn export(
    live: &PagedDocument,
    world: &QuillWorld,
    title: Option<String>,
) -> (Vec<u8>, Vec<Diagnostic>) {
    let (doc, mut warnings) = match probed(live, world) {
        Ok(doc) => (read::read(&doc, world, title), Vec::new()),
        Err(warning) => (read::read(live, world, title), vec![warning]),
    };
    let (word, notices) = quillmark_docx::lower::lower(&doc);
    warnings.extend(
        notices
            .into_iter()
            .map(|n| Diagnostic::new(Severity::Warning, n.message).with_code(n.code)),
    );
    (quillmark_docx::write_docx(&word), warnings)
}
