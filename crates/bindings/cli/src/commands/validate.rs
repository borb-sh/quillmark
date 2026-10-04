use crate::errors::{CliError, Result};
use clap::Parser;
use quillmark::{
    CardSchema, Diagnostic, Document, FieldSchema, Quill, Quillmark, RenderOptions, Severity,
    EXAMPLE_FILE,
};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct ValidateArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill_path: PathBuf,

    /// Show verbose output with all validation details
    #[arg(short, long)]
    verbose: bool,

    /// Skip the render check: only read the configuration
    #[arg(long)]
    no_render: bool,
}

#[derive(Debug, Default)]
struct ValidationResult {
    issues: Vec<Diagnostic>,
}

impl ValidationResult {
    fn add(&mut self, severity: Severity, message: impl Into<String>, code: &str) {
        self.issues
            .push(Diagnostic::new(severity, message.into()).with_code(code.to_string()));
    }

    fn count(&self, severity: Severity) -> usize {
        self.issues.iter().filter(|d| d.severity == severity).count()
    }

    fn has_errors(&self) -> bool {
        self.count(Severity::Error) > 0
    }

    /// Every error a render raised, and each warning not already held: every
    /// render of the quill, failed or not, carries the quill's load warnings.
    fn add_render_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        for diag in diagnostics {
            if diag.severity == Severity::Error || !self.issues.contains(&diag) {
                self.issues.push(diag);
            }
        }
    }
}

pub fn execute(args: ValidateArgs) -> Result<()> {
    // Gated on the directory: the loader below owns the missing-path message.
    // What this adds is the path on a real directory, which it cannot name.
    let quill_yaml_path = args.quill_path.join("Quill.yaml");
    if args.quill_path.is_dir() && !quill_yaml_path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Quill.yaml not found in: {}",
            args.quill_path.display()
        )));
    }

    if args.verbose {
        println!("Validating quill at: {}", args.quill_path.display());
    }

    let mut result = ValidationResult::default();

    let quill = quillmark::quill_from_path(&args.quill_path)?;
    let config = quill.config();

    result.issues.extend(quill.warnings().iter().cloned());

    if args.verbose {
        println!("  Quill name: {}", config.name);
        println!("  Backend: {}", config.backend);
        println!("  Fields: {}", config.main.fields.len());
        println!("  Cards: {}", config.card_kinds.len());
        println!("  Schema generated successfully");
        println!("  Defaults extracted: {}", config.main.defaults().len());
    }

    validate_file_references(&quill, &mut result);

    validate_field_schemas(&config.main.fields, &mut result, "field");

    for card_schema in &config.card_kinds {
        validate_card_schema(&card_schema.name, card_schema, &mut result);
    }

    // A config already refused is not compiled: every canonical document would
    // fail for the reason already named, three times over.
    let render = !args.no_render && !result.has_errors();

    let mut example = read_example(&quill);
    if args.verbose {
        match example {
            Some(_) => println!("  Example: {EXAMPLE_FILE}"),
            None => println!("  Example: none (no {EXAMPLE_FILE} at the quill root)"),
        }
    }
    if render {
        validate_renders(&quill, example.as_mut(), &mut result, args.verbose);
    }
    if let Some(example) = example {
        report_example(example, &mut result);
    }

    print_validation_result(&result, args.verbose);

    if result.has_errors() {
        Err(CliError::Reported)
    } else {
        Ok(())
    }
}

fn validate_file_references(quill: &Quill, result: &mut ValidationResult) {
    // The tree lookup answers `None` to a path that escapes the quill and to one
    // that is simply absent, so the component test runs first and names which of
    // the two a `plate_file` from an untrusted Quill.yaml hit.
    if let Some(plate_file) = quill
        .config()
        .backend_config
        .get("plate_file")
        .and_then(|v| v.as_str())
    {
        let rel = Path::new(plate_file);
        if rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            result.add(
                Severity::Error,
                format!(
                    "plate_file '{}' must be a relative path within the quill (no '..' or absolute components)",
                    plate_file
                ),
                "cli::plate_file_escapes_quill",
            );
        } else if quill.files().get_file(rel).is_none() {
            result.add(
                Severity::Error,
                format!("Referenced plate_file '{}' does not exist", plate_file),
                "cli::plate_file_missing",
            );
        }
    }
}

/// The quill's example document, parsed and validated.
struct Example {
    /// Absent when it already carries an error, which a render would repeat.
    document: Option<Document>,
    diagnostics: Vec<Diagnostic>,
}

fn read_example(quill: &Quill) -> Option<Example> {
    Some(match quill.example_document()? {
        Ok(parsed) => {
            let mut diagnostics = parsed.warnings;
            diagnostics.extend(quill.validate(&parsed.document));
            let fails = diagnostics.iter().any(|d| d.severity == Severity::Error);
            Example {
                document: (!fails).then_some(parsed.document),
                diagnostics,
            }
        }
        Err(diagnostics) => Example {
            document: None,
            diagnostics,
        },
    })
}

/// Any diagnostic on the example fails the quill, a warning included: every
/// value in it is the quill author's own, so each one is theirs to fix.
fn report_example(example: Example, result: &mut ValidationResult) {
    if example.diagnostics.is_empty() {
        return;
    }
    result.add(
        Severity::Error,
        format!(
            "the example document '{EXAMPLE_FILE}' carries {} diagnostic(s)",
            example.diagnostics.len()
        ),
        "cli::example_not_clean",
    );
    result
        .issues
        .extend(example.diagnostics.into_iter().map(|mut d| {
            d.severity = Severity::Error;
            d
        }));
}

/// A render that failed: what went wrong, then the backend's own diagnostics.
struct RenderFailure {
    what: String,
    diagnostics: Vec<Diagnostic>,
}

/// The quill authoring contract, which no config read reaches: each of the
/// three canonical documents — the empty document, the blueprint, the seeded document —
/// compiles through the quill's own plate (`BLUEPRINT.md` §Guarantees). The
/// example, when the quill has one, compiles after them. The backend's
/// first declared format is the one rendered: a plate that compiles carries
/// every format its backend serves.
fn validate_renders(
    quill: &Quill,
    example: Option<&mut Example>,
    result: &mut ValidationResult,
    verbose: bool,
) {
    let engine = Quillmark::new();

    let format = match engine.supported_formats(quill) {
        Ok([format, ..]) => *format,
        Ok([]) => {
            result.add(
                Severity::Error,
                "the quill's backend declares no output format",
                "cli::backend_unresolved",
            );
            return;
        }
        Err(e) => {
            result.add(
                Severity::Error,
                "the quill's backend does not resolve",
                "cli::backend_unresolved",
            );
            result.issues.extend(e.into_diagnostics());
            return;
        }
    };

    if verbose {
        println!("  Rendering the canonical documents to {}", format);
    }

    let blueprint = quill.config().blueprint();
    let documents = [
        ("empty", Ok(quill.empty_document())),
        (
            "blueprint",
            Document::parse(&blueprint).map(|parsed| parsed.document),
        ),
        ("seeded", Ok(quill.seed_document())),
    ];

    let options = RenderOptions::default().with_output_format(format);
    let today = super::render_date(None);
    let render = |document: &Document| -> std::result::Result<Vec<Diagnostic>, RenderFailure> {
        match engine.render(quill, document, today, &options) {
            Ok(rendered) if rendered.artifacts.iter().all(|a| a.bytes.is_empty()) => {
                Err(RenderFailure {
                    what: format!("rendered no {format} bytes"),
                    diagnostics: Vec::new(),
                })
            }
            Ok(rendered) => Ok(rendered.warnings),
            Err(e) => Err(RenderFailure {
                what: "does not render through the quill's plate".to_string(),
                diagnostics: e.into_diagnostics(),
            }),
        }
    };

    for (label, document) in documents {
        let document = match document {
            Ok(document) => document,
            Err(e) => {
                result.add(
                    Severity::Error,
                    format!("the {label} document does not parse: {}", e),
                    "cli::canonical_document_failed",
                );
                continue;
            }
        };

        match render(&document) {
            Ok(warnings) => {
                if verbose {
                    println!("    {label}: ok");
                }
                result.add_render_diagnostics(warnings);
            }
            Err(failure) => {
                result.add(
                    Severity::Error,
                    format!("the {label} document {}", failure.what),
                    "cli::canonical_document_failed",
                );
                result.add_render_diagnostics(failure.diagnostics);
            }
        }
    }

    let Some(example) = example else { return };
    let Some(document) = &example.document else { return };
    let (failed, diagnostics) = match render(document) {
        Ok(warnings) => (None, warnings),
        Err(failure) => (Some(failure.what), failure.diagnostics),
    };
    // A warning a canonical document raised too is the plate's, not the
    // example's; one `validate` already raised is counted once.
    let fresh: Vec<Diagnostic> = diagnostics
        .into_iter()
        .filter(|d| {
            d.severity == Severity::Error
                || (!result.issues.contains(d) && !example.diagnostics.contains(d))
        })
        .collect();
    match failed {
        None if verbose && fresh.is_empty() => println!("    example: ok"),
        None => {}
        Some(what) => example.diagnostics.push(
            Diagnostic::new(Severity::Error, format!("the example document {what}"))
                .with_code("cli::example_render_failed".to_string()),
        ),
    }
    example.diagnostics.extend(fresh);
}

/// The one advisory check config parsing does not already make: `default:`
/// literal errors are caught authoritatively at load time.
fn validate_field_schemas(
    fields: &IndexMap<String, FieldSchema>,
    result: &mut ValidationResult,
    context: &str,
) {
    for (field_name, field_schema) in fields {
        if field_schema
            .description
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            result.add(
                Severity::Warning,
                format!("{context} '{field_name}': missing or empty description"),
                "cli::missing_description",
            );
        }
    }
}

fn validate_card_schema(card_name: &str, card_schema: &CardSchema, result: &mut ValidationResult) {
    if card_schema
        .description
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        result.add(
            Severity::Warning,
            format!("card '{}': missing or empty description", card_name),
            "cli::missing_description",
        );
    }

    let context = format!("card '{}' field", card_name);
    validate_field_schemas(&card_schema.fields, result, &context);
}

fn print_validation_result(result: &ValidationResult, verbose: bool) {
    let error_count = result.count(Severity::Error);
    let warning_count = result.count(Severity::Warning);

    // `-v` adds warnings; errors always print.
    for diag in &result.issues {
        if diag.severity == Severity::Error || verbose {
            eprintln!("{}", diag.fmt_pretty());
        }
    }

    if error_count == 0 && warning_count == 0 {
        println!("Validation passed: quill configuration is valid");
    } else if error_count == 0 {
        println!("Validation passed with {} warning(s)", warning_count);
    } else {
        eprintln!(
            "Validation failed: {} error(s), {} warning(s)",
            error_count, warning_count
        );
    }
}
