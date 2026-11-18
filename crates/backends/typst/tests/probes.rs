//! The Word export's probes change nothing a reader sees: over every Typst
//! fixture quill's seed and example documents, the probed compile's PDF is
//! byte-identical to the plain one's, its ink hashes are equal, and it raises
//! no warning the plain compile does not.

use quillmark_typst::testing::probe_check;

mod common;

#[test]
fn the_probes_leave_every_fixture_pdf_byte_identical() {
    let mut checked = 0;
    for name in quillmark_fixtures::quill_names() {
        let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path(&name))
            .unwrap_or_else(|e| panic!("{name} loads: {e:?}"));
        if quill.backend_id() != "typst" {
            continue;
        }
        for doc in std::iter::once(quill.seed_document()).chain(quill.example_document()) {
            let data = quill
                .compile_checked(&doc, common::test_date())
                .unwrap_or_else(|e| panic!("{name} compiles its data: {e:?}"));
            let check = probe_check(&quill, &data, common::test_date())
                .unwrap_or_else(|e| panic!("{name} compiles: {e:?}"));
            assert_eq!(check.plain_hashes, check.probed_hashes, "{name}: the probes moved ink");
            assert!(check.plain_pdf == check.probed_pdf, "{name}: the probes changed the PDF");
            let codes = |w: &[quillmark_core::error::Diagnostic]| {
                w.iter().map(|d| (d.code.clone(), d.message.clone())).collect::<Vec<_>>()
            };
            assert_eq!(
                codes(&check.plain_warnings),
                codes(&check.probed_warnings),
                "{name}: the probes raised a warning"
            );
            checked += 1;
        }
    }
    assert!(checked >= 4, "the sweep reached {checked} documents");
}

#[test]
fn every_probe_records_where_its_element_is_laid_out() {
    let plate = r#"#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt, numbering: "i")
#align(center)[centred]
#block(sticky: true)[kept]
one #box[two three] \ four
#columns(2)[left #colbreak() right]
#pagebreak(to: "odd")
last
"#;
    let quill = common::quill_with_plate(&common::yaml(""), plate);
    let check = probe_check(&quill, &serde_json::json!({}), common::test_date()).expect("compiles");
    assert!(check.plain_pdf == check.probed_pdf);
    let hints: Vec<&str> = check.probed_hints.iter().map(String::as_str).collect();
    assert_eq!(
        hints,
        ["align", "block", "box", "br", "break", "columns", "page", "par", "pnum"]
    );
}
