use crate::commands::load_quill;
use crate::errors::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct InfoArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill_path: PathBuf,
}

pub fn execute(args: InfoArgs) -> Result<()> {
    let quill = load_quill(&args.quill_path)?;
    let config = quill.config();

    println!("Quill: {}", config.name);

    if !config.description.is_empty() {
        println!("  {:<12} {}", "Description:", config.description);
    }
    println!("  {:<12} {}", "Version:", config.version);
    println!("  {:<12} {}", "Author:", config.author);
    println!("  {:<12} {}", "Backend:", config.backend);
    println!("  {:<12} {}", "Fields:", config.main.fields.len());

    let card_count = config.card_kinds.len();
    if card_count > 0 {
        println!("  {:<12} {}", "Cards:", card_count);
    }

    let defaults_count = config.main.defaults().len();
    if defaults_count > 0 {
        println!("  {:<12} {}", "Defaults:", defaults_count);
    }

    Ok(())
}
