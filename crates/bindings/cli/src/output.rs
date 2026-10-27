use crate::errors::Result;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn write_stdout(bytes: &[u8]) -> Result<()> {
    io::stdout().write_all(bytes)?;
    Ok(())
}

pub fn write_file(path: &Path, bytes: &[u8], announce: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    if announce {
        println!("Output written to: {}", path.display());
    }
    Ok(())
}

pub fn derive_output_path(markdown_path: &Path, format: &str) -> PathBuf {
    let mut output = markdown_path.to_path_buf();
    output.set_extension(format);
    output
}

/// `path` with `-{page}` appended to its file stem: `out.svg` at page 2 is
/// `out-2.svg`. A path with no stem keeps the number as its whole name.
pub fn page_output_path(path: &Path, page: usize) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let mut name = match stem {
        Some(stem) if !stem.is_empty() => format!("{stem}-{page}"),
        _ => page.to_string(),
    };
    if let Some(ext) = path.extension() {
        name.push('.');
        name.push_str(&ext.to_string_lossy());
    }
    path.with_file_name(name)
}
