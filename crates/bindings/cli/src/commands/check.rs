use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::{
    Diagnostic, Document, Engine, FieldCoverage, Quill, RenderOptions, Severity, DOCUMENT_FILE,
};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct CheckArgs {
    /// Path to quill directory or .quill bundle
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Markdown documents to check against the quill
    #[arg(value_name = "MARKDOWN_FILE", required = true)]
    markdown_files: Vec<PathBuf>,

    /// Exit 1 on any warning, not only on an error
    #[arg(long)]
    strict: bool,

    /// Render each document and report the filled fields no click on the page
    /// reaches, and those only an enclosing field's region reaches
    #[arg(long)]
    regions: bool,
}

pub fn execute(args: CheckArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let (mut errors, mut warnings) = (0, 0);
    for path in &args.markdown_files {
        let (mut diagnostics, document) = check_document(&quill, path);
        let clean = !diagnostics.iter().any(|d| d.severity == Severity::Error);
        if let (true, true, Some(document)) = (args.regions, clean, document) {
            match coverage(&quill, &document) {
                Ok((coverage, rendered)) => {
                    print_coverage(path, &coverage);
                    let fresh: Vec<Diagnostic> = rendered
                        .into_iter()
                        .filter(|w| {
                            !diagnostics
                                .iter()
                                .any(|d| d.code == w.code && d.message == w.message)
                        })
                        .collect();
                    diagnostics.extend(fresh);
                }
                Err(failure) => diagnostics.extend(failure),
            }
        }
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
/// warnings, then `Quill::validate`'s, beside the parsed document. A document
/// that does not read or parse draws only that failure, there being no
/// document to validate. A location in the document names `path`.
fn check_document(quill: &Quill, path: &Path) -> (Vec<Diagnostic>, Option<Document>) {
    let markdown = match fs::read_to_string(path) {
        Ok(markdown) => markdown,
        Err(e) => {
            return (
                vec![Diagnostic::new(
                    Severity::Error,
                    format!("the document could not be read: {e}"),
                )
                .with_code("cli::unreadable_document".to_string())],
                None,
            )
        }
    };
    let (mut diagnostics, document) = match quill.parse(&markdown) {
        Ok(parsed) => {
            let mut diagnostics = parsed.warnings;
            diagnostics.extend(quill.validate(&parsed.document));
            (diagnostics, Some(parsed.document))
        }
        Err(e) => (e.to_diagnostics(), None),
    };
    let file = path.display().to_string();
    for loc in diagnostics.iter_mut().filter_map(|d| d.location.as_mut()) {
        if loc.file == DOCUMENT_FILE {
            loc.file = file.clone();
        }
    }
    (diagnostics, document)
}

/// `document`'s coverage over the regions a one-shot render reports, with each
/// address spelled as a `DocPath`, beside the render's warnings. The render's
/// failure is the document's.
fn coverage(
    quill: &Quill,
    document: &Document,
) -> std::result::Result<(Vec<FieldCoverage>, Vec<Diagnostic>), Vec<Diagnostic>> {
    let today = super::render_date(None);
    let options = RenderOptions::default().with_regions(true);
    let rendered = Engine::new()
        .render(quill, document, today, &options)
        .map_err(|e| e.into_diagnostics())?;
    let data = quill
        .compile_checked(document, today)
        .map_err(|e| e.into_diagnostics())?;
    let card_kinds: Vec<Option<&str>> = document
        .cards()
        .iter()
        .map(|card| card.kind())
        .collect();
    let doc_path = |addr: String| {
        quillmark::plate_addr_to_doc_path(&addr, &card_kinds).map_or(addr, |p| p.to_string())
    };
    let coverage = quillmark::coverage(quill.config(), &data, &rendered.regions)
        .into_iter()
        .map(|c| FieldCoverage {
            field: doc_path(c.field),
            covered_by: c.covered_by.map(doc_path),
        })
        .collect();
    Ok((coverage, rendered.warnings))
}

/// The filled fields no click reaches, then those an enclosing field's region
/// reaches, under the document's path. A report: neither moves the exit code,
/// since a quill may leave a field unprinted on purpose.
fn print_coverage(path: &Path, coverage: &[FieldCoverage]) {
    let unreachable: Vec<&FieldCoverage> =
        coverage.iter().filter(|c| c.covered_by.is_none()).collect();
    let coarse: Vec<&FieldCoverage> = coverage
        .iter()
        .filter(|c| c.covered_by.as_ref().is_some_and(|by| *by != c.field))
        .collect();
    println!(
        "{}: {} unreachable, {} coarse",
        path.display(),
        unreachable.len(),
        coarse.len()
    );
    for c in unreachable {
        println!("  unreachable  {}", c.field);
    }
    for c in coarse {
        let by = c.covered_by.as_deref().unwrap_or_default();
        println!("  coarse       {} ← {by}", c.field);
    }
}
