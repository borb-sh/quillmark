use crate::commands::{load_quill, read_document, render_date};
use crate::errors::{CliError, Result};
use crate::output::write_file;
use clap::Parser;
use quillmark::typst_workspace::{workspace, FONTS_DIR, HELPER_DIR, PACKAGES_DIR};
use quillmark::{CalendarDate, Severity};
use std::fs;
use std::path::{Path, PathBuf};

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
    if lies_within(&args.output, &args.quill)? {
        return Err(CliError::InvalidArgument(format!(
            "Workspace directory {} is inside the quill, which would load it as quill files",
            args.output.display()
        )));
    }
    let (document, parse_warnings) = read_document(&quill, args.markdown_file.as_deref())?;
    let json_data = quill.compile_checked(&document, render_date(args.today))?;
    let workspace = workspace(&quill, &json_data)?;

    clear_previous_export(&args.output)?;
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
        let plate = args.quill.join(&workspace.plate_file);
        let pdf_name = Path::new(&workspace.plate_file).with_extension("pdf");
        let pdf = args.output.join(pdf_name.file_name().unwrap_or_default());
        println!(
            "typst watch --root {quill} --package-path {packages} --font-path {fonts} \
             --ignore-system-fonts --ignore-embedded-fonts {plate} {pdf}",
            quill = shell_word(&args.quill),
            packages = shell_word(&args.output.join(PACKAGES_DIR)),
            fonts = shell_word(&args.output.join(FONTS_DIR)),
            plate = shell_word(&plate),
            pdf = shell_word(&pdf),
        );
    }
    Ok(())
}

/// Empty `packages/` and `fonts/` of an earlier export into `out`, so Typst
/// loads no package or face this quill does not ship. A directory holding
/// either without the helper package is not an export, and is refused.
fn clear_previous_export(out: &Path) -> Result<()> {
    let owned = [out.join(PACKAGES_DIR), out.join(FONTS_DIR)];
    if !owned.iter().any(|dir| dir.exists()) {
        return Ok(());
    }
    if !out.join(PACKAGES_DIR).join(HELPER_DIR).is_dir() {
        return Err(CliError::InvalidArgument(format!(
            "Workspace directory {} holds a {PACKAGES_DIR}/ or {FONTS_DIR}/ directory \
             no earlier export wrote; name an empty or new directory",
            out.display()
        )));
    }
    for dir in owned {
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

/// Whether `path` is `root` or under it. The part of `path` that does not exist
/// yet resolves lexically against its nearest existing ancestor.
fn lies_within(path: &Path, root: &Path) -> Result<bool> {
    let root = fs::canonicalize(root)?;
    let mut existing = std::env::current_dir()?.join(path);
    let mut rest = Vec::new();
    let mut resolved = loop {
        if let Ok(canonical) = fs::canonicalize(&existing) {
            break canonical;
        }
        let Some(last) = existing.components().next_back() else {
            break PathBuf::new();
        };
        rest.push(last.as_os_str().to_owned());
        if !existing.pop() {
            break PathBuf::new();
        }
    };
    for name in rest.into_iter().rev() {
        if name == ".." {
            resolved.pop();
        } else {
            resolved.push(name);
        }
    }
    Ok(resolved.starts_with(root))
}

/// `path` as one POSIX shell word: bare when every character is safe, else
/// single-quoted.
fn shell_word(path: &Path) -> String {
    let text = path.display().to_string();
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+=:@,%".contains(c);
    if !text.is_empty() && text.chars().all(safe) {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}
