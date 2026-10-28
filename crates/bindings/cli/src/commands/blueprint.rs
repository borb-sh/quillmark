use crate::commands::load_quill;
use crate::errors::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct BlueprintArgs {
    /// Path to quill directory or .quill bundle
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,
}

pub fn execute(args: BlueprintArgs) -> Result<()> {
    let quill = load_quill(&args.quill)?;

    let blueprint = quill.blueprint();

    println!("{}", blueprint);

    Ok(())
}
