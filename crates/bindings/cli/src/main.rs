mod commands;
mod errors;
mod output;

use clap::{Parser, Subcommand};
use std::process;
use std::sync::LazyLock;

static VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{} (Typst {})",
        env!("CARGO_PKG_VERSION"),
        quillmark::typst_version()
    )
});

#[derive(Parser)]
#[command(name = "quillmark")]
#[command(about = "Render and check Quillmark documents; inspect and validate quills", long_about = None)]
#[command(version = VERSION.as_str())]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render a Markdown document to an output format
    Render(commands::render::RenderArgs),

    /// Check documents against a quill's schema, printing every diagnostic
    Check(commands::check::CheckArgs),

    /// Output a quill's YAML schema, or the JSON Schema of its authoring document
    Schema(commands::schema::SchemaArgs),

    /// Check a quill: read its Quill.yaml and render its canonical documents
    Validate(commands::validate::ValidateArgs),

    /// Display a quill's identity and schema counts
    Info(commands::info::InfoArgs),

    /// Write the files Typst's own tooling needs to compile a quill's plate
    Workspace(commands::workspace::WorkspaceArgs),

    /// Write a quill as one .quill bundle file
    Pack(commands::pack::PackArgs),
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Render(args) => commands::render::execute(args),
        Commands::Check(args) => commands::check::execute(args),
        Commands::Schema(args) => commands::schema::execute(args),
        Commands::Validate(args) => commands::validate::execute(args),
        Commands::Info(args) => commands::info::execute(args),
        Commands::Workspace(args) => commands::workspace::execute(args),
        Commands::Pack(args) => commands::pack::execute(args),
    };

    if let Err(e) = result {
        errors::print_cli_error(&e);
        process::exit(1);
    }
}
