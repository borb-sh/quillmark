pub mod blueprint;
pub mod example;
pub mod info;
pub mod render;
pub mod schema;
pub mod validate;

use crate::errors::Result;
use quillmark::Quill;
use std::path::Path;

pub fn load_quill(path: &Path) -> Result<Quill> {
    Ok(quillmark::quill_from_path(path)?)
}
