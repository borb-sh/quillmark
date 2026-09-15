use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct ExampleArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,
}

pub fn execute(args: ExampleArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let example = quill.example().ok_or_else(|| {
        CliError::InvalidArgument(format!(
            "quill '{}' declares no example document; add 'example: <path>' under 'quill:' in \
             Quill.yaml, or run `quillmark blueprint` for the form to fill",
            quill.name()
        ))
    })?;

    print!("{}", example);
    if !example.ends_with('\n') {
        println!();
    }

    Ok(())
}
