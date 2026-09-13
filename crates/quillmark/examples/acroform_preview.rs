//! Visual preview harness for the `acroform` backend: renders `sample_form` to
//! the fixtures output directory and prints its field regions, so geometry can
//! be cross-checked against a PDF viewer.
//!
//! `cargo run --example acroform_preview -p quillmark`

use quillmark::{Document, OutputFormat, Quillmark, RenderOptions};
use quillmark_fixtures::{example_output_dir, quills_path, write_example_output};

const SAMPLE_FORM_MD: &str = "\
~~~
$quill: sample_form
$kind: main
full_name: Ada Lovelace
comments:
  - First comment line.
  - Second comment line.
agree: true
favorite_color: green
~~~
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Quillmark::new();
    let out_dir = example_output_dir();

    println!("=== acroform backend: sample_form → PDF ===");
    let gf_quill =
        quillmark::quill_from_path(quills_path("sample_form")).expect("load sample_form quill");
    let gf_doc = Document::parse(SAMPLE_FORM_MD).expect("parse sample_form markdown").document;
    let gf_result = engine
        .render(
            &gf_quill,
            &gf_doc,
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("sample_form render");

    write_example_output("sample_form_filled.pdf", &gf_result.artifacts[0].bytes)?;
    println!(
        "Written: {}",
        out_dir.join("sample_form_filled.pdf").display()
    );

    // Region geometry is a session-level query, not on the render result.
    let gf_session = engine
        .open(&gf_quill, &gf_doc)
        .expect("open sample_form session");
    let regions = gf_session.regions();
    println!("\nField regions ({} fields):", regions.len());
    for region in &regions {
        println!(
            "  {:20}  page={:<2}  rect=[{:.1},{:.1},{:.1},{:.1}]",
            region.field,
            region.page,
            region.rect[0],
            region.rect[1],
            region.rect[2],
            region.rect[3],
        );
    }

    println!("\nDone. Open the file in the output directory to review:");
    println!(
        "  PDF: {}",
        out_dir.join("sample_form_filled.pdf").display()
    );

    Ok(())
}
