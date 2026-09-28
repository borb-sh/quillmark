//! The quill's example document (`quill.example`).
use crate::document::{Document, Parsed};
use crate::error::{Diagnostic, Severity, DOCUMENT_FILE};
use crate::version::VersionSelector;

use super::Quill;

impl Quill {
    /// The quill's example document, `None` when `Quill.yaml` declares no
    /// `quill.example`: a filled-in page whose values are made up to show what
    /// the quill looks like in use. Nobody keeps it; starter content someone
    /// keeps is a template.
    ///
    /// The file's `$quill` names this quill with no version selector. The
    /// document comes back pinned to this quill's `name@version`, as
    /// [`seed_document`](Self::seed_document)'s is, and conformed as
    /// [`parse`](Self::parse) conforms, with the parse and `conform::*`
    /// warnings on [`Parsed::warnings`]. A diagnostic located in the document
    /// names the example's path as its file.
    ///
    /// # Errors
    ///
    /// The file is not UTF-8 (`quill::invalid_utf8`), does not parse
    /// (`parse::*`), or its `$quill` names another quill or carries a version
    /// selector (`quill::example_reference`).
    pub fn example_document(&self) -> Option<Result<Parsed, Vec<Diagnostic>>> {
        let path = self.config.example.as_deref()?;
        let mut example = self.read_example(path);
        let diagnostics = match &mut example {
            Ok(parsed) => &mut parsed.warnings,
            Err(errors) => errors,
        };
        for location in diagnostics.iter_mut().filter_map(|d| d.location.as_mut()) {
            if location.file == DOCUMENT_FILE {
                location.file = path.to_string();
            }
        }
        Some(example)
    }

    fn read_example(&self, path: &str) -> Result<Parsed, Vec<Diagnostic>> {
        let bytes = self
            .files
            .get_file(path)
            .ok_or_else(|| vec![super::load::example_missing(path)])?;
        let markdown = std::str::from_utf8(bytes).map_err(|e| {
            vec![Diagnostic::new(
                Severity::Error,
                format!("the example document '{path}' is not valid UTF-8: {e}"),
            )
            .with_code("quill::invalid_utf8".to_string())]
        })?;
        let Parsed {
            mut document,
            mut warnings,
        } = Document::parse(markdown).map_err(|e| vec![e.to_diagnostic()])?;

        let reference = document.quill_reference();
        if reference.name != self.config.name || reference.selector != VersionSelector::Any {
            return Err(vec![Diagnostic::new(
                Severity::Error,
                format!(
                    "the example document '{path}' declares `$quill: {reference}`, where it \
                     names this quill with no version: `$quill: {}`",
                    self.config.name
                ),
            )
            .with_code("quill::example_reference".to_string())
            .with_hint(
                "The example is part of the quill version it sits in, and the quill pins it \
                 to that version when it hands it out."
                    .to_string(),
            )]);
        }

        document.set_quill_ref(super::seed::main_reference(self));
        warnings.extend(self.conform(&mut document).map_err(|e| e.into_diagnostics())?);
        Ok(Parsed { document, warnings })
    }
}
