//! Acceptance tests for the stamp spine: build a tiny traditional-xref base PDF
//! with pdf-writer, stamp it, reparse with lopdf. Technique A bakes no `/AP`, so
//! values land in `/V` and the viewer synthesizes appearances.

use pdf_writer::writers::Form;
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Settings, TextStr};
use quillmark_pdf::{regions_of, stamp, FieldSpec, FieldType, StampOptions};

/// An `n`-page US-Letter base satisfying the spine's input contract.
fn build_base_pdf(n: usize) -> Vec<u8> {
    build_base_pdf_origin(n, [0.0, 0.0, 612.0, 792.0])
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

#[test]
fn stamps_all_four_field_types_into_valid_acroform() {
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &all_four_fields(),
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("lopdf reparse");
    let cat = doc.catalog().expect("catalog");
    let af_ref = cat
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    let af = doc.get_object(af_ref).unwrap().as_dict().unwrap();

    assert!(af.get(b"NeedAppearances").unwrap().as_bool().unwrap());
    assert!(af.get(b"SigFlags").is_err(), "no signature → no SigFlags");

    let dr = af.get(b"DR").unwrap().as_dict().unwrap();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Helv"), "house font Helv registered in /DR");

    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 4);

    let mut by_name = std::collections::HashMap::new();
    for f in fields {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name = String::from_utf8_lossy(w.get(b"T").unwrap().as_str().unwrap()).into_owned();
        by_name.insert(name, w);
    }

    let full = by_name.get("FullName").unwrap();
    assert_eq!(full.get(b"FT").unwrap().as_name().unwrap(), b"Tx");
    assert_eq!(full.get(b"V").unwrap().as_str().unwrap(), b"Ada Lovelace");
    assert!(full.get(b"DA").is_ok(), "text field carries /DA");
    assert_eq!(
        full.get(b"TU").unwrap().as_str().unwrap(),
        b"Full legal name"
    );
    assert_eq!(full.get(b"Subtype").unwrap().as_name().unwrap(), b"Widget");

    let comments = by_name.get("Comments").unwrap();
    let ff = comments.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(ff & (1 << 12), 1 << 12, "multiline flag set");
    assert!(comments.get(b"V").is_err(), "blank field has no /V");

    let agree = by_name.get("Agree").unwrap();
    assert_eq!(agree.get(b"FT").unwrap().as_name().unwrap(), b"Btn");
    assert_eq!(agree.get(b"V").unwrap().as_name().unwrap(), b"Yes");
    assert_eq!(agree.get(b"AS").unwrap().as_name().unwrap(), b"Yes");

    let color = by_name.get("FavoriteColor").unwrap();
    assert_eq!(color.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    let cff = color.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(cff & (1 << 17), 1 << 17, "combo flag set");
    let opts = color.get(b"Opt").unwrap().as_array().unwrap();
    assert_eq!(opts.len(), 3);
    assert_eq!(color.get(b"V").unwrap().as_str().unwrap(), b"green");

    // `into_annotation` writes /Subtype, so a second `.subtype()` would
    // duplicate the key.
    for f in fields {
        let r = f.as_reference().unwrap();
        let header = format!("{} 0 obj", r.0);
        let start = result
            .windows(header.len())
            .position(|w| w == header.as_bytes())
            .expect("widget header");
        let after = &result[start..];
        let endobj = after.windows(6).position(|w| w == b"endobj").unwrap();
        let body = &after[..endobj];
        let count = body.windows(8).filter(|w| *w == b"/Subtype").count();
        assert_eq!(count, 1, "exactly one /Subtype in widget {}", r.0);
    }

    let regions = regions_of(&all_four_fields());
    assert_eq!(regions.len(), 4);
    let agree_region = regions.iter().find(|r| r.field == "agree").unwrap();
    assert_eq!(agree_region.rect, [180.0, 560.0, 194.0, 574.0]);
}

#[test]
fn signature_field_sets_sigflags() {
    let base = build_base_pdf(2);
    let mut sig = FieldSpec::new(
        "Signature".into(),
        1,
        [180.0, 100.0, 520.0, 140.0],
        FieldType::Signature,
    );
    sig.schema_field = Some("signature".into());
    let fields = vec![sig];
    let result = stamp(base, &fields, &StampOptions::default()).expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("reparse");
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(af.get(b"SigFlags").unwrap().as_i64().unwrap(), 1);
    let w = doc
        .get_object(
            af.get(b"Fields").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap(),
        )
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(w.get(b"FT").unwrap().as_name().unwrap(), b"Sig");
    let pages = doc.get_pages();
    let page2 = doc
        .get_object(*pages.get(&2).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(
        page2.has(b"Annots"),
        "signature widget added to page 2 /Annots"
    );
}

#[test]
fn no_producer_no_fields_is_identity() {
    let base = build_base_pdf(1);
    let before = base.clone();
    let result = stamp(base, &[], &StampOptions::default()).expect("stamp ok");
    assert_eq!(result, before, "no-op stamp returns base unchanged");
    assert!(regions_of(&[]).is_empty(), "no fields → no regions");
}

#[test]
fn producer_only_no_fields_stamps_info_producer() {
    // Not the identity short-circuit: a minimal `/Info`-only incremental append.
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
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
        b"Quillmark test"
    );
}

/// A one-page base whose compact `/Info` ends in a hex-encoded `/Title` —
/// what pdf-writer's compact mode emits for a non-ASCII title written last.
fn build_base_with_hex_title_info(title: &str) -> Vec<u8> {
    let mut pdf = Pdf::with_settings(Settings { pretty: false });
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let info_id = Ref::new(5);
    let rect = Rect::new(0.0, 0.0, 612.0, 792.0);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages.kids([page_id]).count(1).media_box(rect);
    }
    pdf.page(page_id)
        .parent(page_tree_id)
        .media_box(rect)
        .contents(content_id);
    let mut content = Content::new();
    content.set_line_width(1.0);
    content.rect(72.0, 700.0, 200.0, 20.0);
    content.stroke();
    pdf.stream(content_id, &content.finish());
    pdf.document_info(info_id)
        .producer(TextStr("Base"))
        .title(TextStr(title));
    pdf.finish()
}

#[test]
fn producer_stamp_preserves_a_trailing_hex_title() {
    let title = "Résumé";
    let result = stamp(
        build_base_with_hex_title_info(title),
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect("stamp ok");

    let doc = lopdf::Document::load_mem(&result).expect("lopdf reparse");
    let info_ref = doc
        .trailer
        .get(b"Info")
        .expect("trailer /Info")
        .as_reference()
        .expect("/Info indirect");
    let info = doc.get_object(info_ref).unwrap().as_dict().unwrap();
    assert_eq!(
        info.get(b"Producer").unwrap().as_str().unwrap(),
        b"Quillmark test"
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

#[test]
fn rotated_page_rejected_cleanly() {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids([page_id])
            .count(1)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    }
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0))
            .rotate(90)
            .contents(content_id);
    }
    let mut content = Content::new();
    content.set_line_width(1.0);
    content.rect(72.0, 700.0, 200.0, 20.0);
    content.stroke();
    pdf.stream(content_id, &content.finish());
    let base = pdf.finish();

    let fields = vec![text_field(
        "FullName",
        "full_name",
        0,
        [180.0, 700.0, 520.0, 720.0],
        "Ada",
    )];
    let err = stamp(base, &fields, &StampOptions::default()).expect_err("rotated page rejected");
    assert_eq!(err.code, "pdf::rotated_page");
}

/// A one-page base whose page carries `/Rotate` as the indirect reference
/// `5 0 R`, resolving to `90`, or no `/Rotate` at all.
fn build_base_with_indirect_rotate(rotate: bool) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let rotate_id = Ref::new(5);
    let rect = Rect::new(0.0, 0.0, 612.0, 792.0);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages.kids([page_id]).count(1).media_box(rect);
    }
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id).media_box(rect).contents(content_id);
        if rotate {
            page.pair(Name(b"Rotate"), rotate_id);
        }
    }
    let mut content = Content::new();
    content.set_line_width(1.0);
    content.rect(72.0, 700.0, 200.0, 20.0);
    content.stroke();
    pdf.stream(content_id, &content.finish());
    pdf.indirect(rotate_id).primitive(90);
    pdf.finish()
}

#[test]
fn indirect_rotate_rejected_cleanly() {
    let fields = vec![text_field(
        "FullName",
        "full_name",
        0,
        [180.0, 700.0, 520.0, 720.0],
        "Ada",
    )];
    let err = stamp(
        build_base_with_indirect_rotate(true),
        &fields,
        &StampOptions::default(),
    )
    .expect_err("an indirect /Rotate is not a resolvable rotation");
    assert_eq!(err.code, "pdf::parse");
    assert!(err.message.contains("/Rotate"), "{}", err.message);

    stamp(
        build_base_with_indirect_rotate(false),
        &fields,
        &StampOptions::default(),
    )
    .expect("the same base without the /Rotate stamps");
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

#[test]
fn implausible_size_errors_cleanly_without_panic() {
    let err = stamp(
        base_with_spliced_size("4294967295"),
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect_err("near-u32::MAX /Size should error");
    assert!(err.message.contains("id space"), "{}", err.message);
}

#[test]
fn size_past_i32_max_errors_cleanly_without_panic() {
    // Ids seeded from here fit a `u32` but not the `i32` a reference holds.
    let fields = vec![text_field(
        "FullName",
        "full_name",
        0,
        [180.0, 700.0, 520.0, 720.0],
        "Ada",
    )];
    let err = stamp(
        base_with_spliced_size("2147483648"),
        &fields,
        &StampOptions::default(),
    )
    .expect_err("/Size past i32::MAX should error");
    assert_eq!(err.code, "pdf::write");
    assert!(err.message.contains("id space"), "{}", err.message);
}

#[test]
fn field_targeting_missing_page_errors() {
    let base = build_base_pdf(1);
    let mut sig = FieldSpec::new("X".into(), 5, [0.0, 0.0, 10.0, 10.0], FieldType::Signature);
    sig.schema_field = Some("x".into());
    let err = stamp(base, &[sig], &StampOptions::default()).expect_err("out of range");
    assert!(err.message.contains("page"), "{}", err.message);
}

/// An `n`-page base whose pages carry the given `/MediaBox`.
fn build_base_pdf_origin(n: usize, mb: [f32; 4]) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    pdf.catalog(catalog_id).pages(page_tree_id);

    let mut page_ids = Vec::new();
    let mut content_ids = Vec::new();
    let mut next = 3i32;
    for _ in 0..n {
        page_ids.push(Ref::new(next));
        next += 1;
        content_ids.push(Ref::new(next));
        next += 1;
    }
    let rect = Rect::new(mb[0], mb[1], mb[2], mb[3]);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids(page_ids.iter().copied())
            .count(n as i32)
            .media_box(rect);
    }
    for i in 0..n {
        pdf.page(page_ids[i])
            .parent(page_tree_id)
            .media_box(rect)
            .contents(content_ids[i]);
        let mut content = Content::new();
        content.set_line_width(1.0);
        content.rect(72.0, 700.0, 200.0, 20.0);
        content.stroke();
        pdf.stream(content_ids[i], &content.finish());
    }
    pdf.finish()
}

/// A one-page base already carrying an inline `/Annots [ref]`. Returns
/// `(pdf, existing_annot_id)`.
fn build_base_with_inline_annot() -> (Vec<u8>, i32) {
    use pdf_writer::types::AnnotationType;
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let annot_id = Ref::new(5);
    pdf.catalog(catalog_id).pages(page_tree_id);
    {
        let mut pages = pdf.pages(page_tree_id);
        pages
            .kids([page_id])
            .count(1)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0));
    }
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(Rect::new(0.0, 0.0, 612.0, 792.0))
            .contents(content_id);
        page.annotations([annot_id]);
    }
    {
        let mut content = Content::new();
        content.set_line_width(1.0);
        content.rect(72.0, 700.0, 200.0, 20.0);
        content.stroke();
        pdf.stream(content_id, &content.finish());
    }
    {
        pdf.annotation(annot_id)
            .subtype(AnnotationType::Text)
            .rect(Rect::new(10.0, 10.0, 30.0, 30.0));
    }
    (pdf.finish(), 5)
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

#[test]
fn nonzero_generation_catalog_rejected_cleanly() {
    let mut base = build_base_pdf(1);
    // Bump the catalog (object 1) header and the trailer /Root to generation 2.
    replace_first(&mut base, b"1 0 obj", b"1 2 obj");
    replace_first(&mut base, b"/Root 1 0 R", b"/Root 1 2 R");
    let err = stamp(
        base,
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect_err("non-zero generation catalog rejected");
    assert_eq!(err.code, "pdf::nonzero_generation");
    assert!(err.message.contains("generation 2"), "{}", err.message);
}

#[test]
fn nonzero_generation_page_rejected_cleanly() {
    // Same guard, reached via a page node a field targets (object 3).
    let mut base = build_base_pdf(1);
    replace_first(&mut base, b"3 0 obj", b"3 4 obj");
    let fields = vec![text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi")];
    let err = stamp(base, &fields, &StampOptions::default())
        .expect_err("non-zero generation page rejected");
    assert_eq!(err.code, "pdf::nonzero_generation");
    assert!(err.message.contains("page"), "{}", err.message);
}

#[test]
fn encrypted_pdf_rejected_cleanly() {
    // After the xref table, so the startxref offset stays valid.
    let base = build_base_pdf(1);
    let tampered = insert_after(&base, b"/Root 1 0 R", b" /Encrypt 1 0 R");
    let err = stamp(
        tampered,
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect_err("encrypted PDF rejected");
    assert_eq!(err.code, "pdf::encrypted");
}

#[test]
fn xref_stream_rejected_cleanly() {
    // A non-`xref` byte run at the startxref offset reads as an xref stream.
    // `xref\n0` heads the table; `startxref\n<n>` never matches it.
    let mut base = build_base_pdf(1);
    replace_first(&mut base, b"xref\n0", b"1 0 \n0");
    let err = stamp(
        base,
        &[],
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect_err("xref stream rejected");
    assert_eq!(err.code, "pdf::xref_stream");
}

#[test]
fn nonzero_mediabox_origin_flows_through() {
    let base = build_base_pdf_origin(1, [10.0, 20.0, 622.0, 812.0]);
    let boxes = quillmark_pdf::page_canvas_boxes(&base).expect("canvas boxes");
    assert_eq!(boxes, vec![[10.0, 20.0, 622.0, 812.0]]);
}

#[test]
fn inline_annots_are_merged_not_replaced() {
    let (base, existing) = build_base_with_inline_annot();
    let fields = vec![text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi")];
    let result = stamp(base, &fields, &StampOptions::default()).expect("stamp ok");

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
fn indirect_annots_rejected_cleanly() {
    let base = build_base_pdf(1);
    // Insert ` /Annots 99 0 R` before the page object's closing `>>`.
    let page_start = find_sub(&base, b"3 0 obj");
    let close = page_start + find_sub(&base[page_start..], b">>");
    let insertion = b" /Annots 99 0 R";
    let mut tampered = base[..close].to_vec();
    tampered.extend_from_slice(insertion);
    tampered.extend_from_slice(&base[close..]);
    // Re-point startxref past the inserted bytes.
    {
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
        let fixed = (off + insertion.len()).to_string();
        let mut out = tampered[..pos].to_vec();
        out.extend_from_slice(fixed.as_bytes());
        out.extend_from_slice(&tampered[end..]);
        tampered = out;
    }
    let fields = vec![text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi")];
    let err =
        stamp(tampered, &fields, &StampOptions::default()).expect_err("indirect /Annots rejected");
    assert_eq!(err.code, "pdf::indirect_annots");
}

#[test]
fn xref_emits_multiple_subsections_when_ids_have_gaps() {
    // Overwriting low ids while allocating fresh high ones leaves gaps in the
    // changed-id set, so the appended xref needs several subsections.
    let base = build_base_pdf(1);
    let result = stamp(
        base,
        &all_four_fields(),
        &StampOptions::default().with_producer("Quillmark test".into()),
    )
    .expect("stamp ok");

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

/// `rect` is public on the same terms as `font_size`, and pdf-writer prints a
/// non-finite float verbatim: `inf`/`NaN` in a `/Rect` parses as no PDF number.
#[test]
fn a_non_finite_rect_is_refused_rather_than_written() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let fields = vec![text_field("X", "x", 0, [10.0, bad, 100.0, 30.0], "hi")];
        let err = stamp(build_base_pdf(1), &fields, &StampOptions::default())
            .expect_err("non-finite rect rejected");
        assert_eq!(err.code, "pdf::bad_rect", "{bad}");
    }
}

/// A catalog with two `/AcroForm` keys is undefined per spec and
/// parser-dependent in practice, and the old form's widgets stay live in the
/// preserved page `/Annots`.
#[test]
fn a_base_that_already_carries_an_acroform_is_refused() {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let content_id = Ref::new(4);
    let acroform_id = Ref::new(5);
    {
        let mut cat = pdf.catalog(catalog_id);
        cat.pages(page_tree_id);
        cat.pair(Name(b"AcroForm"), acroform_id);
    }
    let rect = Rect::new(0.0, 0.0, 612.0, 792.0);
    pdf.pages(page_tree_id)
        .kids([page_id])
        .count(1)
        .media_box(rect);
    pdf.page(page_id)
        .parent(page_tree_id)
        .media_box(rect)
        .contents(content_id);
    pdf.stream(content_id, &Content::new().finish());
    pdf.indirect(acroform_id).start::<Form>().fields([]).finish();

    let fields = vec![text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi")];
    let err = stamp(pdf.finish(), &fields, &StampOptions::default())
        .expect_err("pre-existing /AcroForm rejected");
    assert_eq!(err.code, "pdf::existing_acroform");
}

/// Only `Text` and `Choice` widgets write a `/DA`, so a checkbox's `font` names
/// nothing and registering it would emit an unreferenced Type1 object.
#[test]
fn a_checkbox_font_registers_no_font_object() {
    let mut agree = FieldSpec::new(
        "Agree".into(),
        0,
        [180.0, 560.0, 194.0, 574.0],
        FieldType::Checkbox,
    );
    agree.font = quillmark_pdf::FormFont::Times;
    let out = stamp(build_base_pdf(1), &[agree], &StampOptions::default()).expect("stamp ok");
    let text = String::from_utf8_lossy(&out);
    assert!(
        !text.contains("Times-Roman") && !text.contains("/TiRo"),
        "a checkbox's inert font reached the output"
    );

    let mut typed = text_field("X", "x", 0, [10.0, 10.0, 100.0, 30.0], "hi");
    typed.font = quillmark_pdf::FormFont::Times;
    let out = stamp(build_base_pdf(1), &[typed], &StampOptions::default()).expect("stamp ok");
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("Times-Roman") && text.contains("/TiRo"),
        "a text widget's font is still registered"
    );
}
