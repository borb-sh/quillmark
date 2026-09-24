//! Acceptance tests for the stamp spine: build a tiny traditional-xref base PDF
//! with pdf-writer, stamp it, reparse with lopdf. A value lands in `/V` for a
//! viewer that synthesizes appearances, and in the widget's own `/AP` for one
//! that does not.

use std::collections::HashMap;

use quillmark_pdf::testkit::BasePdf;
use quillmark_pdf::{regions_of, stamp, FieldSpec, FieldType, StampOptions};

/// An `n`-page US-Letter base satisfying the spine's input contract.
fn build_base_pdf(n: usize) -> Vec<u8> {
    BasePdf::letter(n).build()
}

/// A schema-bound single-line text field carrying `value`.
fn text_field(name: &str, schema: &str, page: usize, rect: [f32; 4], value: &str) -> FieldSpec {
    let mut spec = FieldSpec::new(
        name.into(),
        page,
        rect,
        FieldType::Text { multiline: false },
    );
    spec.schema_field = Some(schema.into());
    spec.value = Some(value.into());
    spec
}

fn all_four_fields() -> Vec<FieldSpec> {
    let mut full = text_field(
        "FullName",
        "full_name",
        0,
        [180.0, 700.0, 520.0, 720.0],
        "Ada Lovelace",
    );
    full.tooltip = Some("Full legal name".into());

    let mut comments = FieldSpec::new(
        "Comments".into(),
        0,
        [180.0, 600.0, 520.0, 680.0],
        FieldType::Text { multiline: true },
    );
    comments.schema_field = Some("comments".into());

    let mut agree = FieldSpec::new(
        "Agree".into(),
        0,
        [180.0, 560.0, 194.0, 574.0],
        FieldType::Checkbox,
    );
    agree.schema_field = Some("agree".into());
    agree.value = Some(quillmark_pdf::CHECKBOX_ON_STATE.into());

    let mut color = FieldSpec::new(
        "FavoriteColor".into(),
        0,
        [180.0, 520.0, 520.0, 540.0],
        FieldType::Choice {
            options: vec!["red".into(), "green".into(), "blue".into()],
        },
    );
    color.schema_field = Some("favorite_color".into());
    color.value = Some("green".into());

    vec![full, comments, agree, color]
}

/// The stamped document, its AcroForm dict, and its `/T` → widget map.
fn stamped_on(
    base: Vec<u8>,
    fields: &[FieldSpec],
) -> (lopdf::Document, lopdf::Dictionary, HashMap<String, lopdf::Dictionary>) {
    let out = stamp(base, fields, &StampOptions::default()).expect("stamp ok");
    let doc = lopdf::Document::load_mem(&out).expect("lopdf reparse");
    let af_ref = doc
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap().clone();
    let mut by_name = HashMap::new();
    for f in af.get(b"Fields").unwrap().as_array().unwrap() {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name = String::from_utf8_lossy(w.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, w.clone());
    }
    (doc, af, by_name)
}

/// [`stamped_on`] a one-page base.
fn stamped(fields: &[FieldSpec]) -> (lopdf::Document, lopdf::Dictionary, HashMap<String, lopdf::Dictionary>) {
    stamped_on(build_base_pdf(1), fields)
}

#[test]
fn stamps_all_four_field_types_into_valid_acroform() {
    let (_, af, by_name) = stamped(&all_four_fields());

    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert!(af.get(b"SigFlags").is_err(), "no signature → no SigFlags");

    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Helv"), "house font Helv registered in /DR");
    assert_eq!(by_name.len(), 4);

    let full = &by_name["FullName"];
    assert_eq!(full.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert!(full.get(b"DA").is_ok(), "text field carries /DA");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name"
    );
    assert_eq!(full.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");

    let comments = &by_name["Comments"];
    let ff = comments.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(ff & (1 << 12), 1 << 12, "multiline flag set");
    assert!(comments.get(b"V").is_err(), "blank field has no /V");

    let agree = &by_name["Agree"];
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");

    let color = &by_name["FavoriteColor"];
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    let cff = color.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(cff & (1 << 17), 1 << 17, "combo flag set");
    let opts = color.get(b"Opt").unwrap().as_array().unwrap();
    assert_eq!(opts.len(), 3);
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");

    let regions = regions_of(&all_four_fields());
    assert_eq!(regions.len(), 4);
    let agree_region = regions.iter().find(|r| r.field == "agree").unwrap();
    assert_eq!(agree_region.rect, [180.0, 560.0, 194.0, 574.0]);
}

#[test]
fn signature_field_sets_sigflags() {
    let mut sig = FieldSpec::new(
        "Signature".into(),
        1,
        [180.0, 100.0, 520.0, 140.0],
        FieldType::Signature,
    );
    sig.schema_field = Some("signature".into());
    let (doc, af, w) = stamped_on(build_base_pdf(2), &[sig]);
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    assert_eq!(w["Signature"].get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    let page2 = doc
        .get_object(*doc.get_pages().get(&2).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(
        page2.has(b"Annots"),
        "signature widget added to page 2 /Annots"
    );
}

/// A field-less stamp writes `/Producer` alone: no `/AcroForm`, and a trailing
/// hex `/Title` survives the `/Info` rewrite.
#[test]
fn no_fields_stamps_info_producer_alone() {
    let title = "Résumé";
    let result = stamp(
        BasePdf::letter(1).compact().info_title(title).build(),
        &[],
        &StampOptions::default(),
    )
    .expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("lopdf reparse");
    assert!(
        doc.catalog().unwrap().get(b"AcroForm").is_err(),
        "producer-only stamp must not add an /AcroForm"
    );
    let info_ref = doc
        .trailer
        .get(b"Info")
        .expect("trailer /Info")
        .as_reference()
        .expect("/Info indirect");
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    assert_eq!(
        info.get(b"Producer").unwrap().as_str().unwrap(),
        format!("Quillmark {}", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    let mut utf16be = vec![0xFE, 0xFF];
    for unit in title.encode_utf16() {
        utf16be.extend_from_slice(&unit.to_be_bytes());
    }
    assert_eq!(
        info.get(b"Title").unwrap().as_str().unwrap(),
        utf16be.as_slice(),
        "the hex /Title survives the /Producer rewrite"
    );
}

/// A one-page base whose trailer `/Size` reads `size`. The xref table precedes
/// the trailer, so the length change leaves every stored offset intact.
fn base_with_spliced_size(size: &str) -> Vec<u8> {
    let base = build_base_pdf(1);
    // Byte-level splice: the PDF binary-marker comment is not valid UTF-8.
    let needle = b"/Size 5";
    let at = base
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("trailer /Size");
    let mut tampered = base[..at].to_vec();
    tampered.extend_from_slice(format!("/Size {size}").as_bytes());
    tampered.extend_from_slice(&base[at + needle.len()..]);
    tampered
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("needle {:?} not found", String::from_utf8_lossy(needle)))
}

/// Equal-length in-place replacement of the first `needle`.
fn replace_first(pdf: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(
        needle.len(),
        replacement.len(),
        "in-place replace keeps length"
    );
    let at = find_sub(pdf, needle);
    pdf[at..at + needle.len()].copy_from_slice(replacement);
}

/// Insert `insertion` immediately after the first `needle`.
fn insert_after(pdf: &[u8], needle: &[u8], insertion: &[u8]) -> Vec<u8> {
    let at = find_sub(pdf, needle) + needle.len();
    let mut out = pdf[..at].to_vec();
    out.extend_from_slice(insertion);
    out.extend_from_slice(&pdf[at..]);
    out
}

/// `base` with `insertion` spliced in before the page object's closing `>>`,
/// `startxref` re-pointed past it.
fn with_page_insertion(base: &[u8], insertion: &[u8]) -> Vec<u8> {
    let page_start = find_sub(base, b"3 0 obj");
    let close = page_start + find_sub(&base[page_start..], b">>");
    let mut tampered = base[..close].to_vec();
    tampered.extend_from_slice(insertion);
    tampered.extend_from_slice(&base[close..]);
    let marker = b"startxref\n";
    let pos = tampered
        .windows(marker.len())
        .rposition(|w| w == marker)
        .unwrap()
        + marker.len();
    let mut end = pos;
    while end < tampered.len() && tampered[end].is_ascii_digit() {
        end += 1;
    }
    let off: usize = std::str::from_utf8(&tampered[pos..end])
        .unwrap()
        .parse()
        .unwrap();
    let mut out = tampered[..pos].to_vec();
    out.extend_from_slice((off + insertion.len()).to_string().as_bytes());
    out.extend_from_slice(&tampered[end..]);
    out
}

/// Every base or field the spine's input contract refuses, refused under its
/// code rather than stamped or panicking.
#[test]
fn an_out_of_contract_input_is_refused_under_its_code() {
    let field = || text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi");
    let replaced = |pairs: &[(&[u8], &[u8])]| {
        let mut base = build_base_pdf(1);
        for (needle, replacement) in pairs {
            replace_first(&mut base, needle, replacement);
        }
        base
    };
    let mut off_page = FieldSpec::new("X".into(), 5, [0.0, 0.0, 10.0, 10.0], FieldType::Signature);
    off_page.schema_field = Some("x".into());

    let mut cases: Vec<(&str, Vec<u8>, Vec<FieldSpec>, &str)> = vec![
        ("rotated page", BasePdf::letter(1).rotate(90).build(), vec![field()], "pdf::rotated_page"),
        ("indirect /Rotate", BasePdf::letter(1).indirect_rotate(90).build(), vec![field()], "pdf::parse"),
        (
            "encrypted",
            insert_after(&build_base_pdf(1), b"/Root 1 0 R", b" /Encrypt 1 0 R"),
            vec![],
            "pdf::encrypted",
        ),
        // A non-`xref` byte run at the startxref offset reads as an xref stream.
        ("xref stream", replaced(&[(b"xref\n0", b"1 0 \n0")]), vec![], "pdf::xref_stream"),
        // Two `/AcroForm` keys are undefined per spec, and the old form's
        // widgets stay live in the preserved page `/Annots`.
        ("existing /AcroForm", BasePdf::letter(1).acroform().build(), vec![field()], "pdf::existing_acroform"),
        (
            "non-zero generation catalog",
            replaced(&[(b"1 0 obj", b"1 2 obj"), (b"/Root 1 0 R", b"/Root 1 2 R")]),
            vec![],
            "pdf::nonzero_generation",
        ),
        (
            "non-zero generation page",
            replaced(&[(b"3 0 obj", b"3 4 obj")]),
            vec![field()],
            "pdf::nonzero_generation",
        ),
        (
            "indirect /Annots",
            with_page_insertion(&build_base_pdf(1), b" /Annots 99 0 R"),
            vec![field()],
            "pdf::indirect_annots",
        ),
        ("near-u32::MAX /Size", base_with_spliced_size("4294967295"), vec![], "pdf::write"),
        // Ids seeded from here fit a `u32` but not the `i32` a reference holds.
        ("/Size past i32::MAX", base_with_spliced_size("2147483648"), vec![field()], "pdf::write"),
        ("field past the last page", build_base_pdf(1), vec![off_page], "pdf::update_parse"),
    ];
    // pdf-writer prints a non-finite float verbatim, which parses as no PDF number.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        cases.push((
            "non-finite rect",
            build_base_pdf(1),
            vec![text_field("X", "x", 0, [10.0, bad, 100.0, 30.0], "hi")],
            "pdf::bad_rect",
        ));
    }
    for (what, base, fields, code) in cases {
        let err = stamp(base, &fields, &StampOptions::default())
            .expect_err(&format!("{what} is refused"));
        assert_eq!(err.code, code, "{what}: {}", err.message);
    }
}

#[test]
fn nonzero_mediabox_origin_flows_through() {
    let base = BasePdf::letter(1)
        .media_box([10.0, 20.0, 622.0, 812.0])
        .build();
    let boxes = quillmark_pdf::page_canvas_boxes(&base).expect("canvas boxes");
    assert_eq!(boxes, vec![[10.0, 20.0, 622.0, 812.0]]);
}

#[test]
fn inline_annots_are_merged_not_replaced() {
    let base = BasePdf::letter(1).inline_annot();
    let existing = base.inline_annot_id();
    let fields = vec![text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi")];
    let result = stamp(base.build(), &fields, &StampOptions::default()).expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("reparse");
    let pages = doc.get_pages();
    let page = doc
        .get_object(*pages.get(&1).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let annots = page.get(b"Annots").unwrap().as_array().unwrap();
    let ids: Vec<u32> = annots
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .map(|(id, _)| id)
        .collect();
    assert!(
        ids.contains(&(existing as u32)),
        "existing annot {existing} preserved, got {ids:?}"
    );
    assert!(
        ids.len() >= 2,
        "widget appended alongside existing: {ids:?}"
    );
}

#[test]
fn xref_emits_multiple_subsections_when_ids_have_gaps() {
    // Overwriting low ids while allocating fresh high ones leaves gaps in the
    // changed-id set, so the appended xref needs several subsections.
    let base = build_base_pdf(1);
    let result = stamp(base, &all_four_fields(), &StampOptions::default()).expect("stamp ok");

    // The appended table is the last standalone `\nxref\n`. Header lines carry
    // two numeric tokens; entries carry three.
    let table_marker = b"\nxref\n";
    let pos = result
        .windows(table_marker.len())
        .rposition(|w| w == table_marker)
        .expect("appended xref")
        + table_marker.len();
    let section_end = pos + find_sub(&result[pos..], b"trailer");
    let headers = result[pos..section_end]
        .split(|&b| b == b'\n')
        .filter(|line| {
            let toks: Vec<&[u8]> = line
                .split(|&b| b == b' ')
                .filter(|t| !t.is_empty())
                .collect();
            toks.len() == 2 && toks.iter().all(|t| t.iter().all(u8::is_ascii_digit))
        })
        .count();
    assert!(
        headers >= 2,
        "expected multiple xref subsections, found {headers}"
    );
}

/// `font_size` is public, so the spine cannot assume the Typst helper's asserts
/// ran, and a `/DA` reading `NaN Tf` parses as no PDF number.
#[test]
fn a_nonsense_font_size_falls_back_to_auto_rather_than_forging_a_da() {
    let base = build_base_pdf(1);
    let mut fields = vec![FieldSpec::new(
        "Date".into(),
        0,
        [180.0, 700.0, 520.0, 720.0],
        FieldType::Text { multiline: false },
    )];
    for bad in [f32::NAN, f32::INFINITY, -12.0] {
        fields[0].font_size = Some(bad);
        let out = stamp(base.clone(), &fields, &StampOptions::default())
            .unwrap_or_else(|e| panic!("{bad} should stamp: {}", e.message));
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("/Helv 0 Tf 0 g"),
            "{bad} should write the auto-size /DA"
        );
        for token in ["NaN", "inf", "-12 Tf"] {
            assert!(!text.contains(token), "{bad} leaked {token:?} into the PDF");
        }
    }
}

/// A checkbox's `/DA` is the engine's check font, registered in `/DR` for a
/// viewer synthesizing the `/MK /CA` caption under `/NeedAppearances` (in
/// Helvetica the check glyph is the digit `4`). Its `font` names nothing, so
/// registering it would emit an unreferenced Type1 object; a text widget's is
/// still registered, and a form with no checkbox registers no check font.
#[test]
fn a_checkbox_registers_the_check_font_and_only_it() {
    let mut agree = FieldSpec::new(
        "Agree".into(),
        0,
        [180.0, 560.0, 194.0, 574.0],
        FieldType::Checkbox,
    );
    agree.font = quillmark_pdf::FormFont::Times;
    let out = stamp(build_base_pdf(1), &[agree.clone()], &StampOptions::default()).expect("stamp ok");
    let text = String::from_utf8_lossy(&out);
    assert!(
        !text.contains("Times-Roman") && !text.contains("/TiRo"),
        "a checkbox's inert font reached the output"
    );
    let (doc, af, w) = stamped(&[agree]);
    let fonts = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = fonts.get(b"Font").unwrap().as_dict().unwrap();
    let zadb = fonts
        .get(quillmark_pdf::CHECK_FONT_RESOURCE.as_bytes())
        .expect("/DR registers the check font")
        .as_reference()
        .unwrap();
    let zadb = doc.get_object(zadb).unwrap().as_dict().unwrap();
    assert_eq!(
        zadb.get(b"BaseFont").unwrap().as_name().unwrap(),
        quillmark_pdf::CHECK_FONT
    );
    let da = String::from_utf8_lossy(w["Agree"].get(b"DA").unwrap().as_str().unwrap()).into_owned();
    assert!(
        da.starts_with(&format!("/{} ", quillmark_pdf::CHECK_FONT_RESOURCE)),
        "checkbox /DA selects the check font, got {da:?}"
    );

    let mut typed = text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi");
    typed.font = quillmark_pdf::FormFont::Times;
    let out = stamp(build_base_pdf(1), &[typed], &StampOptions::default()).expect("stamp ok");
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("Times-Roman") && text.contains("/TiRo"),
        "a text widget's font is still registered"
    );
    assert!(
        !text.contains("ZapfDingbats"),
        "a form with no checkbox registers no check font"
    );
}

/// A widget's `/AP` `/N` stream object, or `None` when it bakes no appearance.
fn normal_appearance<'a>(doc: &'a lopdf::Document, w: &lopdf::Dictionary) -> Option<&'a lopdf::Stream> {
    let ap = w.get(b"AP").ok()?.as_dict().expect("/AP is a dict");
    let n = ap.get(b"N").expect("/AP carries /N");
    doc.get_object(n.as_reference().expect("/N is one indirect stream"))
        .unwrap()
        .as_stream()
        .ok()
}

#[test]
fn a_value_is_baked_into_the_widgets_own_appearance_stream() {
    let (doc, af, w) = stamped(&all_four_fields());

    let full = normal_appearance(&doc, &w["FullName"]).expect("a filled text field bakes an /AP");
    assert_eq!(
        full.dict.get(b"Subtype").unwrap().as_name().unwrap(),
        b"Form",
        "the appearance is a Form XObject"
    );
    // `/BBox` is the field box moved to the origin, so a consumer maps it onto
    // `/Rect` without a translation and clips the value to the box.
    let bbox: Vec<f32> = full
        .dict
        .get(b"BBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(bbox, [0.0, 0.0, 340.0, 20.0]);
    let drawn = String::from_utf8_lossy(&full.content).into_owned();
    assert!(drawn.contains("(Ada Lovelace) Tj"), "{drawn}");

    // The face the stream selects resolves in the appearance's own
    // `/Resources`, not the page's, and is the same object `/DR` registers.
    let ap_font = full
        .dict
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Helv")
        .expect("the appearance binds the face it selects")
        .as_reference()
        .unwrap();
    let dr_font = af
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Helv")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(ap_font, dr_font, "one font object, named from both places");

    let color = normal_appearance(&doc, &w["FavoriteColor"]).expect("a chosen option bakes an /AP");
    assert!(String::from_utf8_lossy(&color.content).contains("(green) Tj"));

    // A checked box bakes one stream rather than a per-state subdictionary: the
    // state a stamp writes is the state it renders at.
    let agree = normal_appearance(&doc, &w["Agree"]).expect("a checked box bakes an /AP");
    let drawn = String::from_utf8_lossy(&agree.content).into_owned();
    assert!(drawn.contains("/ZaDb"), "{drawn}");
    assert!(drawn.contains("(4) Tj"), "{drawn}");

    assert!(
        w["Comments"].get(b"AP").is_err(),
        "a blank field bakes nothing to draw"
    );
}

#[test]
fn a_widget_with_nothing_to_show_bakes_no_appearance() {
    let mut unchecked = FieldSpec::new(
        "Agree".into(),
        0,
        [180.0, 560.0, 194.0, 574.0],
        FieldType::Checkbox,
    );
    unchecked.value = Some("Off".into());
    let sig = FieldSpec::new(
        "Signature".into(),
        0,
        [180.0, 100.0, 520.0, 140.0],
        FieldType::Signature,
    );
    let (_, _, w) = stamped(&[unchecked, sig]);

    assert!(w["Agree"].get(b"AP").is_err(), "an unchecked box");
    assert!(w["Signature"].get(b"AP").is_err(), "an unsigned signature");
}

#[test]
fn a_face_an_appearance_draws_with_declares_the_encoding_it_writes() {
    let (doc, _, w) = stamped(&all_four_fields());
    let font = |widget: &lopdf::Dictionary, resource: &[u8]| {
        let id = normal_appearance(&doc, widget)
            .expect("an appearance")
            .dict
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(resource)
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object(id).unwrap().as_dict().unwrap().clone()
    };

    assert_eq!(
        font(&w["FullName"], b"Helv")
            .get(b"Encoding")
            .expect("a text face declares one")
            .as_name()
            .unwrap(),
        b"WinAnsiEncoding",
        "the stream writes WinAnsi bytes, so the face must read them as such"
    );
    assert!(
        font(&w["Agree"], b"ZaDb").get(b"Encoding").is_err(),
        "the symbol face keeps its built-in encoding, where the check glyph lives"
    );
}

#[test]
fn a_non_winansi_value_draws_substituted_while_the_field_keeps_it_whole() {
    let value = "\u{65e5}\u{672c} Caf\u{e9}";
    let (doc, _, w) = stamped(&[text_field(
        "FullName",
        "full_name",
        0,
        [180.0, 700.0, 520.0, 720.0],
        value,
    )]);

    let drawn = normal_appearance(&doc, &w["FullName"]).expect("an appearance");
    let want: &[u8] = &[b'?', b'?', b' ', b'C', b'a', b'f', 0xE9];
    assert!(
        drawn.content.windows(want.len()).any(|c| c == want),
        "{}",
        String::from_utf8_lossy(&drawn.content)
    );

    let mut utf16be = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        utf16be.extend_from_slice(&unit.to_be_bytes());
    }
    assert_eq!(
        w["FullName"].get(b"V").unwrap().as_str().unwrap(),
        utf16be.as_slice(),
        "/V is the source of truth and keeps every code point"
    );
}
