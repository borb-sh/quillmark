//! Option-C preview/PNG path: bake field values as page content streams.
//!
//! Unlike [`stamp`], which writes real AcroForm widgets under Technique A
//! (transparent, appearance-synthesised by the viewer), this operation writes
//! resolved values directly as PDF content — text operators positioned at each
//! field's rect. The result is rasterisable by hayro with values visible,
//! without any appearance synthesis.
//!
//! **Spike implementation.** Sufficient for latency benchmarking. A production
//! version would: auto-size text (emulate `0 Tf`), handle multi-line wrapping,
//! and inject font resources per-page when the /Pages root already has a
//! /Resources dict (rather than only when absent).

use crate::error::PdfError;
use crate::reader::{
    append_incremental_update, assert_traditional_xref, err, extract_outer_dict, find_dict_value,
    find_object_bytes, find_startxref, find_trailer_dict, parse_indirect_ref, resolve_page_ids,
    UpdatedObject,
};
use crate::stamp::CHECKBOX_ON_STATE;
use crate::{FieldSpec, FieldType};

const CODE_PARSE: &str = "pdf::flatten_parse";

/// Fixed text size for the spike — production would auto-size via `0 Tf`
/// emulation (text layout, the deferred hard part).
const TEXT_SIZE: f32 = 9.0;
const TEXT_MARGIN: f32 = 2.0;

/// Produce a flattened PDF: field values baked as page content streams.
///
/// No AcroForm structure is written. The result is rasterisable by hayro with
/// values visible, without appearance synthesis — this is the Option-C
/// canvas/PNG path. Both stamp (for the deliverable) and flatten (for
/// preview/PNG) derive independently from the same `base` and `fields`.
///
/// Input contract is identical to [`stamp`]: traditional-xref, unencrypted,
/// inline-annots, flat-tree.
pub fn flatten(base: Vec<u8>, fields: &[FieldSpec]) -> Result<Vec<u8>, PdfError> {
    // Only fields with resolved values contribute rendered content.
    // Signatures are always unbound by design.
    let valued: Vec<&FieldSpec> = fields
        .iter()
        .filter(|f| f.value.is_some() && !matches!(f.field_type, FieldType::Signature))
        .collect();

    if valued.is_empty() {
        return Ok(base);
    }

    let pdf = base;
    let xref_offset = find_startxref(&pdf)?;
    assert_traditional_xref(&pdf, xref_offset)?;

    let trailer = find_trailer_dict(&pdf, xref_offset)?;
    if find_dict_value(trailer, "Encrypt").is_some() {
        return Err(err(
            "pdf::encrypted",
            "PDF is encrypted; flatten does not handle encrypted PDFs",
        ));
    }
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;
    let size = find_dict_value(trailer, "Size")
        .and_then(|v| std::str::from_utf8(v.trim_ascii()).ok())
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(|| err(CODE_PARSE, "/Size missing or malformed in trailer"))?;

    let page_ids = resolve_page_ids(&pdf, catalog_id)?;
    let page_count = page_ids.len();

    for spec in &valued {
        if spec.page >= page_count {
            return Err(err(
                CODE_PARSE,
                format!(
                    "field {:?} targets page {} but the PDF has {page_count} page(s)",
                    spec.name, spec.page
                ),
            ));
        }
    }

    let mut next_id = size;
    let alloc = |next: &mut u32| -> Result<u32, PdfError> {
        let id = *next;
        *next = id
            .checked_add(1)
            .ok_or_else(|| err(CODE_PARSE, "PDF object id space exhausted"))?;
        Ok(id)
    };

    let mut objects: Vec<UpdatedObject> = Vec::new();

    // ── Standard Type1 fonts ──────────────────────────────────────────────
    let helv_id = alloc(&mut next_id)?;
    let zadb_id = alloc(&mut next_id)?;
    objects.push(type1_font_object(helv_id, b"Helvetica"));
    objects.push(type1_font_object(zadb_id, b"ZapfDingbats"));

    // ── /Pages root: inject font resources (inherited by all pages) ───────
    //
    // Spike simplification: we add /Resources to the /Pages root only when it
    // is absent. If the root already has /Resources the font refs in the
    // content stream will be unresolved — a rasteriser will warn, but timing
    // remains valid for the spike.
    let pages_id = {
        let (cs, ce) = find_object_bytes(&pdf, catalog_id)
            .ok_or_else(|| err(CODE_PARSE, "catalog not found"))?;
        let cat = extract_outer_dict(&pdf[cs..ce])
            .ok_or_else(|| err(CODE_PARSE, "catalog dict not parseable"))?;
        find_dict_value(cat, "Pages")
            .and_then(parse_indirect_ref)
            .map(|(id, _)| id)
            .ok_or_else(|| err(CODE_PARSE, "catalog /Pages reference not found"))?
    };
    {
        let (ps, pe) = find_object_bytes(&pdf, pages_id)
            .ok_or_else(|| err(CODE_PARSE, "pages root not found"))?;
        let pages_dict = extract_outer_dict(&pdf[ps..pe])
            .ok_or_else(|| err(CODE_PARSE, "pages root dict not parseable"))?;
        let mut inner = pages_dict.to_vec();
        if find_dict_value(pages_dict, "Resources").is_none() {
            inner.extend_from_slice(
                format!(
                    " /Resources << /ProcSet [/PDF /Text] /Font \
                     << /Helv {helv_id} 0 R /ZaDb {zadb_id} 0 R >> >>"
                )
                .as_bytes(),
            );
        }
        objects.push(dict_object(pages_id, &inner));
    }

    // ── Per-page content streams ──────────────────────────────────────────
    let mut by_page: Vec<Vec<&FieldSpec>> = vec![Vec::new(); page_count];
    for f in &valued {
        by_page[f.page].push(f);
    }

    for (page_idx, page_fields) in by_page.iter().enumerate() {
        if page_fields.is_empty() {
            continue;
        }

        let stream_id = alloc(&mut next_id)?;

        let mut content: Vec<u8> = Vec::new();
        for field in page_fields {
            let value = field.value.as_deref().unwrap();
            let [x0, y0, _x1, y1] = field.rect;
            match &field.field_type {
                FieldType::Text { .. } => {
                    write_text(
                        &mut content,
                        "Helv",
                        TEXT_SIZE,
                        x0 + TEXT_MARGIN,
                        y0 + TEXT_MARGIN,
                        value,
                    );
                }
                FieldType::Checkbox => {
                    if value == CHECKBOX_ON_STATE {
                        let size = (y1 - y0) * 0.8;
                        write_text(&mut content, "ZaDb", size, x0, y0, "4");
                    }
                }
                FieldType::Choice { .. } => {
                    write_text(
                        &mut content,
                        "Helv",
                        TEXT_SIZE,
                        x0 + TEXT_MARGIN,
                        y0 + TEXT_MARGIN,
                        value,
                    );
                }
                FieldType::Signature => {}
            }
        }

        objects.push(raw_stream(stream_id, &content));

        // Extend the page's /Contents to append the new stream.
        let page_obj_id = page_ids[page_idx];
        let page_update = {
            let (ps, pe) = find_object_bytes(&pdf, page_obj_id)
                .ok_or_else(|| err(CODE_PARSE, format!("page {page_obj_id} not found")))?;
            let pg = extract_outer_dict(&pdf[ps..pe])
                .ok_or_else(|| err(CODE_PARSE, format!("page {page_obj_id} not parseable")))?;
            dict_object(page_obj_id, &extend_contents(pg, stream_id)?)
        };
        objects.push(page_update);
    }

    append_incremental_update(pdf, xref_offset, catalog_id, next_id, None, &objects)
}

/// Rewrite a page dict's `/Contents` to append `stream_id`.
///
/// Handles three cases: absent (fresh array), inline array (append before `]`),
/// single indirect ref (wrap in array). Indirect-ref /Contents is an error per
/// the input contract.
fn extend_contents(pg: &[u8], stream_id: u32) -> Result<Vec<u8>, PdfError> {
    match find_dict_value(pg, "Contents") {
        None => {
            let mut out = pg.to_vec();
            out.extend_from_slice(format!(" /Contents [{stream_id} 0 R]").as_bytes());
            Ok(out)
        }
        Some(existing) => {
            let trimmed = existing.trim_ascii();
            let new_val = if trimmed.starts_with(b"[") {
                let end = trimmed
                    .iter()
                    .rposition(|&b| b == b']')
                    .ok_or_else(|| err(CODE_PARSE, "/Contents array missing ]"))?;
                let inner = String::from_utf8_lossy(&trimmed[1..end]);
                format!("[{} {stream_id} 0 R]", inner.trim())
            } else if let Some((ref_id, _)) = parse_indirect_ref(existing) {
                format!("[{ref_id} 0 R {stream_id} 0 R]")
            } else {
                return Err(err(
                    CODE_PARSE,
                    "/Contents is neither an array nor an indirect ref",
                ));
            };

            let key = b"/Contents";
            let value_start = existing.as_ptr() as usize - pg.as_ptr() as usize;
            let key_at = value_start - key.len();
            let value_end = value_start + existing.len();
            let mut out = Vec::new();
            out.extend_from_slice(&pg[..key_at]);
            out.extend_from_slice(b"/Contents ");
            out.extend_from_slice(new_val.as_bytes());
            out.extend_from_slice(&pg[value_end..]);
            Ok(out)
        }
    }
}

/// PDF text operators for one value at `(x, y)` in the given font at `size` pt.
fn write_text(out: &mut Vec<u8>, font: &str, size: f32, x: f32, y: f32, text: &str) {
    out.extend_from_slice(b"q\nBT\n");
    out.extend_from_slice(
        format!("/{font} {size:.2} Tf\n0 g\n{x:.2} {y:.2} Td\n").as_bytes(),
    );
    out.extend_from_slice(&pdf_literal(text));
    out.extend_from_slice(b" Tj\nET\nQ\n");
}

/// Encode `s` as a PDF literal string `( … )` escaping `(`, `)`, `\`.
/// ASCII only — sufficient for the spike (gov-form values are typically ASCII).
fn pdf_literal(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for &b in s.as_bytes() {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// An uncompressed stream object: `id 0 obj\n<< /Length N >>\nstream\n…\nendstream\nendobj\n`.
fn raw_stream(id: u32, data: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< /Length {} >>\nstream\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"endstream\nendobj\n");
    UpdatedObject { id, bytes }
}

/// A minimal Type1 font object (standard 14 fonts need no embedding).
fn type1_font_object(id: u32, base_font: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n").into_bytes();
    bytes.extend_from_slice(b"<< /Type /Font /Subtype /Type1 /BaseFont /");
    bytes.extend_from_slice(base_font);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

/// Serialize one dict as an indirect object: `id 0 obj\n<< inner >>\nendobj\n`.
fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}
