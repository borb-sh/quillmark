use crate::commands::{load_quill, read_document, render_date};
use crate::errors::Result;
use crate::output::write_file;
use clap::Parser;
use quillmark::typst_workspace::{workspace, FONTS_DIR, PACKAGES_DIR};
use quillmark::{CalendarDate, Severity};
use std::path::PathBuf;

#[derive(Parser)]
pub struct WorkspaceArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Path to markdown file with card-yaml blocks (default: the quill's seeded document)
    #[arg(value_name = "MARKDOWN_FILE")]
    markdown_file: Option<PathBuf>,

    /// Workspace directory
    #[arg(short, long, value_name = "DIR", default_value = "quillmark-workspace")]
    output: PathBuf,

    /// The render date a `today` date renders as (default: the local date)
    #[arg(long, value_name = "YYYY-MM-DD")]
    today: Option<CalendarDate>,

    /// Suppress warnings and the command line; errors still print
    #[arg(long)]
    quiet: bool,
}

pub fn execute(args: WorkspaceArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;
    let (document, parse_warnings) = read_document(&quill, args.markdown_file.as_deref())?;
    let json_data = quill.compile_checked(&document, Some(render_date(args.today)))?;
    let workspace = workspace(&quill, &json_data)?;

    for (path, contents) in &workspace.files {
        write_file(&args.output.join(path), contents, false)?;
    }

    if !args.quiet {
        let unclaimed = quill
            .validate(&document)
            .into_iter()
            .filter(|d| d.severity == Severity::Warning);
        let warnings: Vec<_> = parse_warnings.into_iter().chain(unclaimed).collect();
        crate::errors::print_warnings(&warnings);
        println!("Workspace written to: {}", args.output.display());
        println!(
            "typst watch --root {quill} --package-path {packages} --font-path {fonts} \
             --ignore-system-fonts --ignore-embedded-fonts {plate}",
            quill = args.quill.display(),
            packages = args.output.join(PACKAGES_DIR).display(),
            fonts = args.output.join(FONTS_DIR).display(),
            plate = args.quill.join(&workspace.plate_file).display(),
        );
    }
    Ok(())
}
