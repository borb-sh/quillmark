//! Feature-gated (`--features compose`): the compose-first/stamp-last primitive.
//! Build minimal multi-page PDFs, merge them, and confirm the merged base is a
//! field-free PDF that `stamp` can address across source boundaries.
#![cfg(feature = "compose")]

use quillmark_pdf::{compose, page_count, stamp, FieldSpec, FieldType, StampOptions};

/// A minimal `n`-page PDF (blank pages, traditional xref).
fn minimal_pdf(n: usize) -> Vec<u8> {
    // obj 1 = catalog, obj 2 = pages, obj 3.. = one page each.
    let kids = (0..n)
        .map(|i| format!("{} 0 R", 3 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>"),
    ];
    for _ in 0..n {
        bodies.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_string());
    }

    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body.as_bytes());
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n");
    pdf.extend_from_slice(format!("0 {}\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    pdf.extend_from_slice(b"trailer\n");
    pdf.extend_from_slice(format!("<< /Size {} /Root 1 0 R >>\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn concatenates_pages_in_order_without_acroform() {
    let merged = compose(&[&minimal_pdf(1), &minimal_pdf(2)]).expect("compose");
    // 1 + 2 = 3 pages, addressable by the same scanner stamp() uses.
    assert_eq!(page_count(&merged).expect("page_count"), 3);

    // The composed base carries no form yet.
    let doc = lopdf::Document::load_mem(&merged).expect("reparse merged");
    assert!(!doc.catalog().unwrap().has(b"AcroForm"));
}

#[test]
fn stamp_addresses_fields_across_source_boundaries() {
    // 1-page "background" + 2-page "continuation" → stamp a field on the gov
    // page (0) and an overflow field on a continuation page (2).
    let merged = compose(&[&minimal_pdf(1), &minimal_pdf(2)]).expect("compose");

    let gov = FieldSpec::new("FullName", 0, [180.0, 715.0, 520.0, 735.0], FieldType::Text);
    let overflow = FieldSpec::new("Row4", 2, [180.0, 600.0, 520.0, 620.0], FieldType::Text);
    let out = stamp(merged, &[gov, overflow], &StampOptions::default()).expect("stamp merged");

    let doc = lopdf::Document::load_mem(&out.pdf).expect("reparse stamped");
    let cat = doc.catalog().unwrap();
    let af = doc
        .get_object(cat.get(b"AcroForm").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let fields = af.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2);

    let page_ids: Vec<(u32, u16)> = doc
        .get_pages()
        .iter()
        .map(|(_, &id)| (id.0, id.1))
        .collect();
    assert_eq!(page_ids.len(), 3);
    for f in fields {
        let w = doc
            .get_object(f.as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let name = String::from_utf8_lossy(w.get(b"T").unwrap().as_str().unwrap()).into_owned();
        let p = w.get(b"P").unwrap().as_reference().unwrap();
        let idx = page_ids.iter().position(|&x| x == p).unwrap();
        let expected = if name == "FullName" { 0 } else { 2 };
        assert_eq!(idx, expected, "field {name} landed on wrong page");
    }

    // Regions are reported for both fields.
    assert_eq!(out.regions.len(), 2);
}
