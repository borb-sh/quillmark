use crate::commands::{load_quill, read_document, render_date};
use crate::errors::{CliError, Result};
use crate::output::{derive_output_path, page_output_path, write_file, write_stdout};
use clap::Parser;
use quillmark::{CalendarDate, OutputFormat, Quillmark, RenderOptions, Severity};
use std::path::{Path, PathBuf};

#[derive(Parser)]
pub struct RenderArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Path to markdown file with card-yaml blocks
    #[arg(value_name = "MARKDOWN_FILE")]
    markdown_file: Option<PathBuf>,

    /// Output file path (default: derived from input filename)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Output format: pdf, svg, png (default: the -o extension when it names one, else pdf)
    #[arg(short, long, value_name = "FORMAT")]
    format: Option<String>,

    /// Write output to stdout instead of file
    #[arg(long)]
    stdout: bool,

    /// Suppress all non-error output
    #[arg(long)]
    quiet: bool,

    /// Output intermediate JSON data to file
    #[arg(long, value_name = "DATA_FILE")]
    output_data: Option<PathBuf>,

    /// The render date a `today` date renders as (default: the local date)
    #[arg(long, value_name = "YYYY-MM-DD")]
    today: Option<CalendarDate>,
}

pub fn execute(args: RenderArgs) -> Result<()> {
    let today = render_date(args.today);
    let quill = load_quill(&args.quill)?;

    let (parsed, parse_warnings) = read_document(&quill, args.markdown_file.as_deref())?;
    let markdown_path_for_output = args.markdown_file.clone();

    let output_format = resolve_format(
        args.format.as_deref(),
        args.output.as_deref().filter(|_| !args.stdout),
    )?;

    if let Some(data_path) = args.output_data {
        let json_data = quill.compile_data(&parsed, today).map_err(CliError::Render)?;
        let f = std::fs::File::create(&data_path).map_err(|e| {
            CliError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to create data output file '{}': {}",
                    data_path.display(),
                    e
                ),
            ))
        })?;
        serde_json::to_writer_pretty(f, &json_data).map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to write JSON data: {}",
                e
            )))
        })?;
    }

    let engine = Quillmark::new();
    let mut result = engine.render(
        &quill,
        &parsed,
        today,
        &RenderOptions::default().with_output_format(output_format),
    )?;

    // `validate`'s warnings name input the page leaves out
    // (`prose/canon/SCHEMAS.md` § "What blocks a render").
    let unclaimed = quill
        .validate(&parsed)
        .into_iter()
        .filter(|d| d.severity == Severity::Warning);
    result
        .warnings
        .splice(0..0, parse_warnings.into_iter().chain(unclaimed));

    if !args.quiet {
        crate::errors::print_warnings(&result.warnings);
    }

    if result.artifacts.is_empty() {
        return Err(CliError::InvalidArgument(
            "No artifacts produced from rendering".to_string(),
        ));
    }

    if args.stdout {
        if result.artifacts.len() > 1 {
            return Err(CliError::InvalidArgument(format!(
                "{} renders {} pages, one artifact each, and --stdout carries one; \
                 drop --stdout to write the pages as files",
                output_format,
                result.artifacts.len()
            )));
        }
        write_stdout(&result.artifacts[0].bytes)?;
    } else {
        let output_path = args.output.unwrap_or_else(|| {
            if let Some(ref path) = markdown_path_for_output {
                derive_output_path(path, output_format.as_str())
            } else {
                PathBuf::from(format!("example.{}", output_format))
            }
        });
        if let [artifact] = result.artifacts.as_slice() {
            write_file(&output_path, &artifact.bytes, !args.quiet)?;
        } else {
            // Every page numbered, page one included: an unnumbered file beside
            // numbered ones reads as the whole document.
            for (i, artifact) in result.artifacts.iter().enumerate() {
                let path = page_output_path(&output_path, i + 1);
                write_file(&path, &artifact.bytes, !args.quiet)?;
            }
        }
    }

    Ok(())
}

/// An `-o` extension that names a format is a second statement of it: it
/// supplies an omitted `-f` and must agree with a given one. Any other
/// extension names no format and is written as given.
fn resolve_format(flag: Option<&str>, output: Option<&Path>) -> Result<OutputFormat> {
    let flag = flag
        .map(str::parse::<OutputFormat>)
        .transpose()
        .map_err(|e| CliError::InvalidArgument(e.to_string()))?;
    let named = output.and_then(|path| {
        let format = path.extension()?.to_str()?.parse::<OutputFormat>().ok()?;
        Some((format, path))
    });
    match (flag, named) {
        (Some(flag), Some((named, path))) if flag != named => {
            Err(CliError::InvalidArgument(format!(
                "-f {flag} disagrees with -o {}, which names {named}; \
                 drop -f to write {named}, or give -o a .{flag} extension",
                path.display()
            )))
        }
        (flag, named) => Ok(flag
            .or(named.map(|(format, _)| format))
            .unwrap_or(OutputFormat::Pdf)),
    }
}
