//! Sets the PDF `/Info` `/Producer` string via an incremental update appended
//! to a typst_pdf-produced PDF. typst_pdf 0.14 writes only `/Creator (Typst …)`
//! into `/Info` and leaves `/Producer` unset, so this pass stamps the producing
//! tool — `Quillmark <version>` by default, or a caller-supplied override.
//!
//! Runs before [`crate::sig_overlay::inject`]: this pass rewrites the existing
//! `/Info` object (object number and `/Size` unchanged), so the signature pass
//! still reads a consistent `/Root` and `/Size` from the latest trailer.

use quillmark_core::RenderError;

use crate::pdf_scan::{
    assert_traditional_xref, err, extract_outer_dict, find_dict_value, find_object_bytes,
    find_startxref, find_trailer_dict, parse_indirect_ref, parse_int, write_preserved_trailer_keys,
};

const CODE_PARSE: &str = "typst::meta_overlay_pdf_parse";

/// Default `/Producer` value: `Quillmark <crate-version>`.
pub(crate) fn default_producer() -> String {
    format!("Quillmark {}", env!("CARGO_PKG_VERSION"))
}

/// Append an incremental update that sets `/Producer` to `producer` in the
/// document's `/Info` dictionary. When `/Info` is absent (not emitted by
/// typst_pdf 0.14, handled defensively) a fresh `/Info` object is created and
/// referenced from the new trailer.
pub(crate) fn inject(pdf: Vec<u8>, producer: &str) -> Result<Vec<u8>, RenderError> {
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;

    let trailer = find_trailer_dict(&pdf, xref_offset)?;
    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "typst::meta_overlay_encrypted",
            "PDF is encrypted; producer inject does not handle encrypted PDFs",
        ));
    }
    let (root_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;
    let size = find_dict_value(trailer, "Size")
        .and_then(parse_int)
        .ok_or_else(|| err(CODE_PARSE, "/Size missing or malformed in trailer"))?
        as u32;

    let literal = pdf_text_string(producer);
    let info_ref = find_dict_value(trailer, "Info").and_then(parse_indirect_ref);

    // (updated-object bytes, object id, new /Size, optional /Info ref for the
    // trailer when we had to create the object).
    let (object_bytes, new_size, info_for_trailer) = match info_ref {
        Some((info_id, _)) => {
            let (s, e) = find_object_bytes(&pdf, info_id)
                .ok_or_else(|| err(CODE_PARSE, format!("/Info object {info_id} not found")))?;
            let info_dict = extract_outer_dict(&pdf[s..e])
                .ok_or_else(|| err(CODE_PARSE, "/Info dict not parseable"))?;
            let inner = upsert_producer(info_dict, &literal);
            let mut buf = Vec::new();
            buf.extend_from_slice(format!("{info_id} 0 obj\n<< ").as_bytes());
            buf.extend_from_slice(&inner);
            buf.extend_from_slice(b" >>\nendobj\n");
            (vec![(info_id, buf)], size, None)
        }
        None => {
            let info_id = size;
            let mut buf = Vec::new();
            buf.extend_from_slice(format!("{info_id} 0 obj\n<< /Producer ").as_bytes());
            buf.extend_from_slice(&literal);
            buf.extend_from_slice(b" >>\nendobj\n");
            (vec![(info_id, buf)], size + 1, Some(info_id))
        }
    };

    // Forward `/Info` and `/ID` from the prior trailer so readers that only
    // read the last trailer keep them. When we created a fresh `/Info` object
    // (no prior one to forward), reference it explicitly. Built while `trailer`
    // still borrows `pdf`, before the move below.
    let mut trailer_tail = Vec::new();
    write_preserved_trailer_keys(&mut trailer_tail, trailer);
    if let Some(id) = info_for_trailer {
        trailer_tail.extend_from_slice(format!(" /Info {id} 0 R").as_bytes());
    }

    let mut out = pdf;
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let mut entries: Vec<(u32, usize)> = Vec::new();
    for (obj_id, buf) in &object_bytes {
        let off = out.len();
        out.extend_from_slice(buf);
        entries.push((*obj_id, off));
    }

    let new_xref_off = out.len();
    entries.sort_by_key(|(id, _)| *id);
    out.extend_from_slice(b"xref\n");
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        out.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            out.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }

    out.extend_from_slice(format!("trailer\n<< /Size {new_size} /Root {root_id} 0 R").as_bytes());
    out.extend_from_slice(&trailer_tail);
    out.extend_from_slice(
        format!(" /Prev {xref_offset} >>\nstartxref\n{new_xref_off}\n%%EOF\n").as_bytes(),
    );
    Ok(out)
}

/// Replace `/Producer`'s value if present, else append the entry. typst_pdf
/// 0.14 never emits `/Producer`, so the append branch is the live path; the
/// replace branch guards future typst versions and idempotent re-runs.
fn upsert_producer(info_dict: &[u8], literal: &[u8]) -> Vec<u8> {
    let key = b"/Producer";
    match find_dict_value(info_dict, "Producer") {
        None => {
            let mut out = info_dict.to_vec();
            out.extend_from_slice(b" /Producer ");
            out.extend_from_slice(literal);
            out
        }
        Some(value) => {
            // `value` is a subslice of `info_dict`; recover its byte range.
            let value_start = value.as_ptr() as usize - info_dict.as_ptr() as usize;
            let value_end = value_start + value.len();
            let key_at = info_dict
                .windows(key.len())
                .position(|w| w == key)
                .unwrap_or(value_start);
            let mut out = Vec::new();
            out.extend_from_slice(&info_dict[..key_at]);
            out.extend_from_slice(b"/Producer ");
            out.extend_from_slice(literal);
            out.extend_from_slice(&info_dict[value_end..]);
            out
        }
    }
}

/// Encode `s` as a PDF text string. ASCII uses a literal `( … )` with `(`, `)`
/// and `\` escaped; anything else uses a UTF-16BE hex string with a BOM, the
/// portable encoding for non-Latin producer overrides.
fn pdf_text_string(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        let mut out = Vec::with_capacity(s.len() + 2);
        out.push(b'(');
        for &b in s.as_bytes() {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b')');
        out
    } else {
        let mut out = Vec::new();
        out.push(b'<');
        out.extend_from_slice(b"FEFF");
        for unit in s.encode_utf16() {
            out.extend_from_slice(format!("{unit:04X}").as_bytes());
        }
        out.push(b'>');
        out
    }
}
