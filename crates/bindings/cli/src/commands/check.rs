use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::{Diagnostic, Quill, Severity, DOCUMENT_FILE};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct CheckArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Markdown documents to check against the quill
    #[arg(value_name = "MARKDOWN_FILE", required = true)]
    markdown_files: Vec<PathBuf>,

    /// Exit 1 on any warning, not only on an error
    #[arg(long)]
    strict: bool,
}

pub fn execute(args: CheckArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let (mut errors, mut warnings) = (0, 0);
    for path in &args.markdown_files {
        let diagnostics = check_document(&quill, path);
        if diagnostics.is_empty() {
            continue;
        }
        eprintln!("{}", path.display());
        for diag in &diagnostics {
            match diag.severity {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
            }
            eprintln!("{}", diag.fmt_pretty());
        }
    }

    let documents = args.markdown_files.len();
    let counts = format!("{errors} error(s), {warnings} warning(s) in {documents} document(s)");
    if errors > 0 || (args.strict && warnings > 0) {
        eprintln!("\nCheck failed: {counts}");
        Err(CliError::Reported)
    } else {
        println!("Check passed: {counts}");
        Ok(())
    }
}

/// Every diagnostic one document draws against `quill`: the bound parse's
/// warnings, then `Quill::validate`'s. A document that does not read or parse
/// draws only that failure, there being no document to validate.
fn check_document(quill: &Quill, path: &Path) -> Vec<Diagnostic> {
    let markdown = match fs::read_to_string(path) {
        Ok(markdown) => markdown,
        Err(e) => {
            return vec![Diagnostic::new(
                Severity::Error,
                format!("the document could not be read: {e}"),
            )
            .with_code("cli::unreadable_document".to_string())]
        }
    };
    let mut diagnostics = match quill.parse(&markdown) {
        Ok(parsed) => {
            let mut diagnostics = parsed.warnings;
            diagnostics.extend(quill.validate(&parsed.document));
            diagnostics
        }
        Err(e) => e.to_diagnostics(),
    };
    for location in diagnostics.iter_mut().filter_map(|d| d.location.as_mut()) {
        if location.file == DOCUMENT_FILE {
            location.file = path.display().to_string();
        }
    }
    diagnostics
}
