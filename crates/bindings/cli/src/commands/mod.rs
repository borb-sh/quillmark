pub mod blueprint;
pub mod check;
pub mod info;
pub mod render;
pub mod schema;
pub mod validate;
pub mod workspace;

use crate::errors::{CliError, Result};
use quillmark::{CalendarDate, Diagnostic, Document, Quill};
use std::path::Path;

pub fn load_quill(path: &Path) -> Result<Quill> {
    Ok(quillmark::quill_from_path(path)?)
}

/// The render date the CLI supplies: `pinned` when given, else the local date,
/// or the UTC date where the local offset cannot be read.
pub fn render_date(pinned: Option<CalendarDate>) -> CalendarDate {
    pinned.unwrap_or_else(|| {
        let now = time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        CalendarDate::new(now.year(), u8::from(now.month()), now.day())
            .expect("the clock reads a calendar date")
    })
}

/// `markdown` parsed against `quill`, beside its parse warnings; the quill's
/// seeded document without one.
pub fn read_document(quill: &Quill, markdown: Option<&Path>) -> Result<(Document, Vec<Diagnostic>)> {
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
