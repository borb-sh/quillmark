//! The Word export's probes change nothing a reader sees: over every Typst
//! fixture quill's seed and example documents, the probed compile's pages
//! draw as the plain one's, its ink hashes are equal, and it raises no
//! warning the plain compile does not.

use quillmark_typst::testing::probe_check;

mod common;

#[test]
fn the_probes_leave_every_fixture_page_drawn_as_it_was() {
    let mut checked = 0;
    for (name, quill, data) in common::typst_fixture_documents() {
        let check = probe_check(&quill, &data, common::test_date())
            .unwrap_or_else(|e| panic!("{name} compiles: {e:?}"));
        assert_eq!(
            check.plain_hashes, check.probed_hashes,
            "{name}: the probes moved ink"
        );
        assert!(
            check.plain_svg == check.probed_svg,
            "{name}: the probes changed a page"
        );
        let codes = |w: &[quillmark_core::error::Diagnostic]| {
            w.iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            codes(&check.plain_warnings),
            codes(&check.probed_warnings),
            "{name}: the probes raised a warning"
        );
        checked += 1;
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
#outline()
#show outline.entry: it => [#it.body() #box(width: 1fr, repeat[.]) #it.page()]
#outline(title: none)
#pagebreak(to: "odd")
= last
"#;
    let quill = common::quill_with_plate(&common::yaml(""), plate);
    let check = probe_check(&quill, &serde_json::json!({}), common::test_date()).expect("compiles");
    assert!(check.plain_svg == check.probed_svg);
    let hints: Vec<&str> = check.probed_hints.iter().map(String::as_str).collect();
    assert_eq!(
        hints,
        [
            "align", "block", "box", "br", "break", "columns", "page", "par", "pnum", "repeat"
        ]
    );
}

#[test]
fn a_space_before_a_line_break_collapses_as_it_does_without_the_probes() {
    let plate = r#"#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(font: "TeX Gyre Termes")
A #strong[Bold] \ next #upper[up] \ after
#("a b", "c d").join("\n")
"#;
    let quill = common::host_with_plate(plate);
    let check = probe_check(&quill, &serde_json::json!({}), common::test_date()).expect("compiles");
    assert_eq!(
        check.plain_hashes, check.probed_hashes,
        "the probes moved ink"
    );
    assert!(check.plain_svg == check.probed_svg);
    assert!(check.probed_hints.contains("br"));
}

#[test]
fn a_document_that_sets_its_own_page_foreground_or_background_keeps_its_ink() {
    let fg = "foreground: place(top + right, dx: -5pt, dy: 5pt)[DRAFT]";
    let bg = "background: rect(width: 100%, height: 100%, fill: luma(240))";
    for (sets, keeps_probe) in [
        (fg.to_string(), true),
        (bg.to_string(), true),
        (format!("{fg}, {bg}"), false),
    ] {
        let plate = format!(
            "#set page(width: 300pt, height: 300pt, margin: 20pt, numbering: \"1\", {sets})\n\
             Body text.\n"
        );
        let quill = common::quill_with_plate(&common::yaml(""), &plate);
        let check =
            probe_check(&quill, &serde_json::json!({}), common::test_date()).expect("compiles");
        assert_eq!(check.plain_hashes, check.probed_hashes, "{sets}: ink moved");
        assert!(
            check.plain_svg == check.probed_svg,
            "{sets}: a page changed"
        );
        assert_eq!(check.probed_hints.contains("page"), keeps_probe, "{sets}");
    }
}
