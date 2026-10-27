use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct SchemaArgs {
    /// Path to quill directory or .quill bundle
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,
}

pub fn execute(args: SchemaArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let config = quill.config();
    let schema_yaml = config
        .schema_yaml()
        .map_err(|e| CliError::InvalidArgument(format!("Failed to serialize schema: {}", e)))?;

    println!("{}", schema_yaml);

    Ok(())
}
