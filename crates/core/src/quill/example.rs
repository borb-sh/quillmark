//! The quill's example document, [`EXAMPLE_FILE`] at its root.
use crate::document::{Document, Parsed};
use crate::error::{Diagnostic, Severity, DOCUMENT_FILE};
use crate::version::VersionSelector;

use super::Quill;

/// The example document's path in a quill, by name as `Quill.yaml` is. A file
/// of this name below the root is an ordinary file.
pub const EXAMPLE_FILE: &str = "example.md";

impl Quill {
    /// The quill's example document, `None` when the quill has no
    /// [`EXAMPLE_FILE`] at its root: a filled-in page whose values are made up
    /// to show what the quill looks like in use. Nobody keeps it; starter
    /// content someone keeps is a template.
    ///
    /// The file's `$quill` names this quill with no version selector. The
    /// document comes back pinned to this quill's `name@version`, as
    /// [`seed_document`](Self::seed_document)'s is, and conformed as
    /// [`parse`](Self::parse) conforms, with the parse and `conform::*`
    /// warnings on [`Parsed::warnings`]. A diagnostic located in the document
    /// names [`EXAMPLE_FILE`] as its file.
    ///
    /// # Errors
    ///
    /// The file is not UTF-8 (`quill::invalid_utf8`), does not parse
    /// (`parse::*`), or its `$quill` names another quill or carries a version
    /// selector (`quill::example_reference`).
    pub fn example_document(&self) -> Option<Result<Parsed, Vec<Diagnostic>>> {
        let bytes = self.files.get_file(EXAMPLE_FILE)?;
        let mut example = self.read_example(bytes);
        let diagnostics = match &mut example {
            Ok(parsed) => &mut parsed.warnings,
            Err(errors) => errors,
        };
        for location in diagnostics.iter_mut().filter_map(|d| d.location.as_mut()) {
            if location.file == DOCUMENT_FILE {
                location.file = EXAMPLE_FILE.to_string();
            }
        }
        Some(example)
    }

    fn read_example(&self, bytes: &[u8]) -> Result<Parsed, Vec<Diagnostic>> {
        let markdown = std::str::from_utf8(bytes).map_err(|e| {
            vec![Diagnostic::new(
                Severity::Error,
                format!("the example document '{EXAMPLE_FILE}' is not valid UTF-8: {e}"),
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
                    "the example document '{EXAMPLE_FILE}' declares `$quill: {reference}`, \
                     where it names this quill with no version: `$quill: {}`",
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
