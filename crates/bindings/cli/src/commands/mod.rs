pub mod check;
pub mod info;
pub mod pack;
pub mod render;
pub mod schema;
pub mod validate;
pub mod workspace;

use crate::errors::{CliError, Result};
use quillmark::{CalendarDate, Diagnostic, Document, Quill};
use std::fs;
use std::path::{Path, PathBuf};

pub fn load_quill(path: &Path) -> Result<Quill> {
    Ok(quillmark::quill_from_path(path)?)
}

/// The bytes of each `--font` file, in the order given.
pub fn read_fonts(paths: &[PathBuf]) -> Result<Vec<Vec<u8>>> {
    paths
        .iter()
        .map(|path| {
            std::fs::read(path).map_err(|e| {
                CliError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read font '{}': {e}", path.display()),
                ))
            })
        })
        .collect()
}

/// The render date the CLI supplies: `pinned` when given, else the local date,
/// or the UTC date where the local offset cannot be read.
pub fn render_date(pinned: Option<CalendarDate>) -> CalendarDate {
    pinned.unwrap_or_else(|| {
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        CalendarDate::new(now.year(), u8::from(now.month()), now.day())
            .expect("the clock reads a calendar date")
    })
}

/// `markdown` parsed against `quill`, beside its parse warnings; the quill's
/// seeded document without one.
pub fn read_document(
    quill: &Quill,
    markdown: Option<&Path>,
) -> Result<(Document, Vec<Diagnostic>)> {
    let Some(path) = markdown else {
        return Ok((quill.seed_document(), Vec::new()));
    };
    if !path.exists() {
        return Err(CliError::InvalidArgument(format!(
            "Markdown file not found: {}",
            path.display()
        )));
    }
    let output = quill.parse(&std::fs::read_to_string(path)?)?;
    Ok((output.document, output.warnings))
}

/// Whether `path` is `root` or under it. The part of `path` that does not exist
/// yet resolves lexically against its nearest existing ancestor.
pub fn lies_within(path: &Path, root: &Path) -> Result<bool> {
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
