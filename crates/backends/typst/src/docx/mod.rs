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
use quillmark_docx::notice::Code;
use typst_layout::PagedDocument;

use crate::world::QuillWorld;

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
            Diagnostic::new(Severity::Warning, Code::ProbesDisabled.message("", 1))
                .with_code(Code::ProbesDisabled.as_str().to_string())
        })
}

/// The `.docx` bytes for `live`, whose PDF carries `fields` as widgets, and
/// the warnings the export raises.
pub(crate) fn export(
    live: &PagedDocument,
    world: &QuillWorld,
    title: Option<String>,
    fields: &[quillmark_pdf::FieldSpec],
) -> (Vec<u8>, Vec<Diagnostic>) {
    let (doc, mut warnings) = match probed(live, world) {
        Ok(doc) => (read::read(&doc, world, title), Vec::new()),
        Err(warning) => (read::read(live, world, title), vec![warning]),
    };
    let names: std::collections::BTreeSet<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    if !names.is_empty() {
        warnings.push(
            Diagnostic::new(
                Severity::Warning,
                Code::FormFieldDeclined.message("", names.len()),
            )
            .with_code(Code::FormFieldDeclined.as_str().to_string()),
        );
    }
    let (word, notices) = quillmark_docx::lower::lower(&doc);
    warnings.extend(
        notices
            .into_iter()
            .map(|n| Diagnostic::new(Severity::Warning, n.message).with_code(n.code)),
    );
    (quillmark_docx::write_docx(&word), warnings)
}
