//! The stamp spine's byte-level reads over arbitrary and corrupted PDF bytes:
//! `Err`, never a panic. Nothing in the workspace catches unwind, so one panic
//! kills the CLI and the Python extension and poisons the WASM module.
//!
//! `Ok` is not assertable over mutated bytes — the reader's input contract
//! refuses most well-formed PDFs too — so a refusal is an acceptable answer
//! there. A base the testkit builds is in contract, so its stamp is asserted.

use std::sync::LazyLock;

use proptest::prelude::*;
use quillmark_pdf::testkit::{null_spellings, BasePdf};
use quillmark_pdf::{page_canvas_boxes, stamp, FieldSpec, FieldType, StampOptions};

/// A real AcroForm the spine accepts, so a mutant of it exercises parse paths a
/// random buffer never reaches.
static BASE_PDF: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = quillmark_fixtures::quills_path("sample_form").join("form.pdf");
    std::fs::read(&path).expect("the sample_form fixture ships a form.pdf")
});

fn base_pdf() -> Vec<u8> {
    BASE_PDF.clone()
}

/// One field of each `FieldType`, each carrying a value so `stamp` walks every
/// widget writer and the appearance stream beside it.
fn every_field_kind() -> Vec<FieldSpec> {
    let mut text = FieldSpec::new("t".into(), 0, [10.0, 10.0, 90.0, 30.0], FieldType::Text {
        multiline: true,
    });
    text.value = Some("first\nsecond".into());
    let mut check = FieldSpec::new("c".into(), 0, [10.0, 40.0, 30.0, 60.0], FieldType::Checkbox);
    check.value = Some(quillmark_pdf::CHECKBOX_ON_STATE.into());
    let mut choice = FieldSpec::new("h".into(), 0, [10.0, 100.0, 90.0, 120.0], FieldType::Choice {
        options: vec!["a".into(), "b".into()],
    });
    choice.value = Some("a".into());
    vec![
        text,
        check,
        FieldSpec::new("s".into(), 0, [10.0, 70.0, 90.0, 90.0], FieldType::Signature),
        choice,
    ]
}

/// Drive every byte-taking entry point once; completing at all is the property.
/// The field-less stamp is the trailer and `/Info` read on its own; the other
/// adds the page-tree walk, the widget writers and the appearance streams.
fn exercise(pdf: &[u8]) {
    let _ = page_canvas_boxes(pdf);
    let _ = stamp(pdf.to_vec(), &[], &StampOptions::default());
    let _ = stamp(pdf.to_vec(), &every_field_kind(), &StampOptions::default());
}

/// Entries the stamp reads off a base that a `null` can stand in for without
/// leaving the input contract, by the dictionary carrying them.
const NULLABLE: [(&str, &str); 8] = [
    ("trailer", "Encrypt"),
    ("trailer", "Info"),
    ("trailer", "ID"),
    ("catalog", "AcroForm"),
    ("page", "Annots"),
    ("page", "CropBox"),
    ("page", "MediaBox"),
    ("page", "Rotate"),
];

/// A one-page testkit base carrying `/key` and its spelling of `null` at each
/// of `nulls`.
fn nulled_base(nulls: &[((&str, &'static str), Vec<u8>)]) -> Vec<u8> {
    let mut base = BasePdf::letter(1);
    let mut trailer = Vec::new();
    for ((holder, key), spelling) in nulls {
        match *holder {
            "catalog" => base = base.catalog_raw(key, spelling.clone()),
            "page" => base = base.page_raw(key, spelling.clone()),
            _ => {
                trailer.extend_from_slice(format!(" /{key}").as_bytes());
                trailer.extend_from_slice(spelling);
            }
        }
    }
    let pdf = base.build();
    // The trailer follows the xref table, so the splice moves no stored offset.
    let root = b"/Root 1 0 R";
    let at = pdf
        .windows(root.len())
        .position(|w| w == root)
        .expect("trailer /Root")
        + root.len();
    [&pdf[..at], &trailer, &pdf[at..]].concat()
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|w| *w == needle).count()
}

proptest! {
    // Above proptest's default 256: a case is one parse over a few kilobytes,
    // no oracle and no I/O, so the wider net is close to free.
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Nothing checks for a `%PDF-` header (`PdfUpdate::begin` scans backwards
    /// for `startxref`), so both buffer shapes take the same path.
    #[test]
    fn arbitrary_bytes_are_refused(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        exercise(&bytes);
    }

    /// Every length and offset the file declares now points past the end. The
    /// range is the fixture's own length: a fixed bound would mostly sample
    /// past it and re-run the intact file.
    #[test]
    fn a_truncated_form_is_refused(cut in any::<prop::sample::Index>()) {
        let pdf = base_pdf();
        let cut = cut.index(pdf.len() + 1);
        exercise(&pdf[..cut]);
    }

    /// What truncation cannot hit: a corrupted xref offset, a `/Length` that
    /// overshoots, an unbalanced delimiter, a broken object header.
    #[test]
    fn a_single_corrupted_byte_is_refused(at in any::<prop::sample::Index>(), to in any::<u8>()) {
        let mut pdf = base_pdf();
        let i = at.index(pdf.len());
        pdf[i] = to;
        exercise(&pdf);
    }

    /// A run wide enough to take out a whole keyword (`trailer`, `startxref`).
    #[test]
    fn a_spliced_run_is_refused(
        at in any::<prop::sample::Index>(),
        run in proptest::collection::vec(any::<u8>(), 1..64),
    ) {
        let mut pdf = base_pdf();
        let start = at.index(pdf.len());
        let end = (start + run.len()).min(pdf.len());
        pdf[start..end].copy_from_slice(&run[..end - start]);
        exercise(&pdf);
    }

    /// The unit tests pin the individual refusals; this adds the combinations,
    /// and a page index past the fixture's single page, which `stamp` must
    /// refuse rather than index into its `Vec`.
    #[test]
    fn out_of_contract_field_geometry_is_refused(
        page in 0usize..8,
        x0 in prop::num::f32::ANY,
        y0 in prop::num::f32::ANY,
        x1 in prop::num::f32::ANY,
        y1 in prop::num::f32::ANY,
        name in "\\PC{0,32}",
    ) {
        let field = FieldSpec::new(name, page, [x0, y0, x1, y1], FieldType::Checkbox);
        let _ = stamp(base_pdf(), &[field], &StampOptions::default());
    }

    /// ISO 32000-1 §7.3.9: an entry whose value is `null` is an absent one.
    /// The [`NULLABLE`] entries, nulled in any combination and each in any of
    /// the [`null_spellings`], stamp as their absence: the page keeps the page
    /// tree's box, and each dictionary the update rewrites names each key it
    /// writes once.
    #[test]
    fn a_null_entry_stamps_as_its_absence(
        spellings in proptest::collection::vec(
            proptest::option::of(proptest::sample::select(null_spellings())),
            NULLABLE.len(),
        ),
    ) {
        let nulls: Vec<_> = NULLABLE
            .into_iter()
            .zip(spellings)
            .filter_map(|(entry, spelling)| Some((entry, spelling?)))
            .collect();
        let base = nulled_base(&nulls);
        prop_assert_eq!(
            page_canvas_boxes(&base).map_err(|e| e.message),
            Ok(vec![[0.0, 0.0, 612.0, 792.0]])
        );
        let out = stamp(base.clone(), &every_field_kind(), &StampOptions::default())
            .map_err(|e| TestCaseError::fail(format!("{}: {}", e.code, e.message)))?;
        let update = &out[base.len()..];
        for (key, want) in [
            (&b"/AcroForm"[..], 1),
            (b"/Annots", 1),
            (b"/Info", 1),
            (b"/Producer", 1),
            (b"/ID", 0),
        ] {
            prop_assert_eq!(
                count(update, key),
                want,
                "{} in {}",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(update)
            );
        }
    }
}
