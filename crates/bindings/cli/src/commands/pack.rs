use crate::commands::{lies_within, load_quill};
use crate::errors::{CliError, Result};
use crate::output::write_file;
use clap::Parser;
use quillmark::BUNDLE_EXTENSION;
use std::path::PathBuf;

#[derive(Parser)]
pub struct PackArgs {
    /// Path to quill directory or .quill bundle
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Bundle file to write (default: <name>@<version>.quill)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

pub fn execute(args: PackArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;
    let config = quill.config();
    let output = args.output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}@{}.{BUNDLE_EXTENSION}",
            config.name, config.version
        ))
    });
    if args.quill.is_dir() && lies_within(&output, &args.quill)? {
        return Err(CliError::InvalidArgument(format!(
            "{} is inside the quill, which would load it as a quill file",
            output.display()
        )));
    }
    let bundle = quill.to_bundle().map_err(quillmark::RenderError::new)?;
    write_file(&output, &bundle, true)?;
    Ok(())
}
