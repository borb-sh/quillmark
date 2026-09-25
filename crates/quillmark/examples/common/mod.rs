use quillmark_fixtures::{example_output_dir, quills_path, write_example_output};
use std::error::Error;

/// Render a quill's seed document to PDF in the demo output directory: one card
/// per declared kind, every field at its `default:` or blank.
pub fn demo(quill_dir: &str, render_output: &str) -> Result<(), Box<dyn Error>> {
    let quill_path = quills_path(quill_dir);
    let engine = quillmark::Quillmark::new();
    let quill = quillmark::quill_from_path(quill_path.clone()).expect("Failed to load quill");

    let parsed = quill.seed_document();

    let rendered = engine.render(
        &quill,
        &parsed,
        quillmark::CalendarDate::new(2026, 3, 14).expect("a calendar day"),
        &quillmark::RenderOptions::default().with_output_format(quillmark::OutputFormat::Pdf),
    )?;
    let output_bytes = rendered.artifacts[0].bytes.clone();

    write_example_output(render_output, &output_bytes)?;

    println!("------------------------------");
    println!(
        "Access render output: {}",
        example_output_dir().join(render_output).display()
    );

    Ok(())
}
