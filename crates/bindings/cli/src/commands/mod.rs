pub mod blueprint;
pub mod check;
pub mod info;
pub mod render;
pub mod schema;
pub mod validate;

use crate::errors::Result;
use quillmark::{CalendarDate, Quill};
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
