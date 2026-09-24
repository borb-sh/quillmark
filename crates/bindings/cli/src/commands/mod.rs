pub mod blueprint;
pub mod check;
pub mod info;
pub mod render;
pub mod schema;
pub mod validate;

use crate::errors::Result;
use quillmark::{CalendarDate, Quill};
use std::path::Path;
use std::sync::OnceLock;

pub fn load_quill(path: &Path) -> Result<Quill> {
    Ok(quillmark::quill_from_path(path)?)
}

/// The host's UTC offset, or UTC where it cannot be read. `time` reads it only
/// while the process has one thread, so `main` calls this before any work.
pub fn local_offset() -> time::UtcOffset {
    static OFFSET: OnceLock<time::UtcOffset> = OnceLock::new();
    *OFFSET.get_or_init(|| time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC))
}

/// The render date the CLI supplies: `pinned` when given, else the local date.
pub fn render_date(pinned: Option<CalendarDate>) -> CalendarDate {
    pinned.unwrap_or_else(|| {
        let now = time::OffsetDateTime::now_utc().to_offset(local_offset());
        CalendarDate::new(now.year(), u8::from(now.month()), now.day())
            .expect("the clock reads a calendar date")
    })
}
