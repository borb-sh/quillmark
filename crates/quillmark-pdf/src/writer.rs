//! Low-level PDF byte-serialization shared by the stamp and flatten paths, so
//! the two emit identical bytes for an object, a text string, and the `/Info`
//! `/Producer` stamp.

use pdf_writer::Ref;

use crate::error::PdfError;
use crate::reader::{
    err, find_dict_value, splice_dict_value, InfoSource, ObjectIndex, UpdatedObject,
};

const CODE_PARSE: &str = "pdf::write";

/// The largest object id a PDF reference carries here: `pdf_writer::Ref` holds
/// an `i32` and panics outside `1 ..= i32::MAX`.
const MAX_ID: u32 = i32::MAX as u32;

/// Serialize one indirect object from its inner dict bytes:
/// `<id> 0 obj\n<< <inner> >>\nendobj\n`.
pub fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

/// Hand out the next object id from `next`, bounded at `i32::MAX` so a
/// malformed large `/Size` errors instead of wrapping into a colliding id or
/// handing out one no reference admits.
pub fn alloc_id(next: &mut u32) -> Result<u32, PdfError> {
    let id = *next;
    if id > MAX_ID {
        return Err(err(
            CODE_PARSE,
            "PDF object id space exhausted (/Size too large)",
        ));
    }
    *next = id + 1;
    Ok(id)
}

/// `id` as a reference, refusing what [`Ref::new`] panics on. Base object ids
/// reach this straight from the file, so the bound is the reader's to enforce.
pub(crate) fn to_ref(id: u32) -> Result<Ref, PdfError> {
    if id == 0 || id > MAX_ID {
        return Err(err(
            CODE_PARSE,
            format!("PDF object id {id} is outside the writable id space 1..={MAX_ID}"),
        ));
    }
    Ok(Ref::new(id as i32))
}

/// Escape bytes for a PDF literal string `( … )`: `(`, `)`, `\` → `\x`.
pub fn pdf_escape(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
}

/// Encode `s` as a PDF text string. ASCII uses a literal `( … )` with `(`, `)`
/// and `\` escaped; anything else uses a UTF-16BE hex string with a BOM.
pub(crate) fn pdf_text_string(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        let mut out = Vec::with_capacity(s.len() + 2);
        out.push(b'(');
        pdf_escape(&mut out, s.as_bytes());
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

/// What an existing value that is not an inline array means for
/// [`append_refs_to_array_key`].
pub enum OnNonArray {
    /// Take it as the array's first element (`/Contents 4 0 R` → `[4 0 R …]`).
    Wrap,
    /// Refuse it, with the error the fn builds from the raw value.
    Reject(fn(&[u8]) -> PdfError),
}

/// Append `refs` as indirect references to `dict`'s inline array `key`, writing
/// a fresh single-element array when the key is absent. `code` carries the
/// caller's error code for an array that never closes.
pub fn append_refs_to_array_key(
    dict: &[u8],
    key: &str,
    refs: &[u32],
    code: &'static str,
    on_non_array: OnNonArray,
) -> Result<Vec<u8>, PdfError> {
    let refs_str = refs
        .iter()
        .map(|r| format!("{r} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");

    let Some(existing) = find_dict_value(dict, key) else {
        let mut out = dict.to_vec();
        out.extend_from_slice(format!(" /{key} [{refs_str}]").as_bytes());
        return Ok(out);
    };

    let trimmed = existing.trim_ascii();
    let inner = if trimmed.starts_with(b"[") {
        let end = trimmed
            .iter()
            .rposition(|&b| b == b']')
            .ok_or_else(|| err(code, format!("/{key} array missing ]")))?;
        &trimmed[1..end]
    } else {
        match on_non_array {
            OnNonArray::Wrap => trimmed,
            OnNonArray::Reject(to_err) => return Err(to_err(existing)),
        }
    };
    let merged = format!("[{} {refs_str}]", String::from_utf8_lossy(inner).trim());
    Ok(splice_dict_value(
        dict,
        format!("/{key}").as_bytes(),
        existing,
        merged.as_bytes(),
    ))
}

/// Replace `/Producer`'s value if present, else append the entry.
pub(crate) fn upsert_producer(info_dict: &[u8], literal: &[u8]) -> Vec<u8> {
    let key = b"/Producer";
    match find_dict_value(info_dict, "Producer") {
        None => {
            let mut out = info_dict.to_vec();
            out.extend_from_slice(b" /Producer ");
            out.extend_from_slice(literal);
            out
        }
        Some(value) => splice_dict_value(info_dict, key, value, literal),
    }
}

/// Stamp `/Info` `/Producer = producer`, pushing the updated or freshly created
/// `/Info` onto `objects`. Returns `Some(info_id)` when a new `/Info` was
/// allocated, which the caller points the new trailer at.
pub(crate) fn apply_producer_stamp(
    idx: &ObjectIndex,
    info: InfoSource<'_>,
    producer: &str,
    next_id: &mut u32,
    objects: &mut Vec<UpdatedObject>,
) -> Result<Option<u32>, PdfError> {
    let literal = pdf_text_string(producer);
    match info {
        InfoSource::Object(info_id) => {
            // Overwritten in place at generation 0; a non-zero-generation
            // `/Info` would be silently corrupted.
            idx.assert_overwrite_gen_zero(info_id, "/Info")?;
            let what = format!("/Info object {info_id}");
            let info_dict = idx.dict(info_id, CODE_PARSE, &what)?;
            objects.push(dict_object(info_id, &upsert_producer(info_dict, &literal)));
            Ok(None)
        }
        InfoSource::Entries(entries) => {
            let info_id = alloc_id(next_id)?;
            let inner = upsert_producer(entries.trim_ascii(), &literal);
            objects.push(dict_object(info_id, inner.trim_ascii()));
            Ok(Some(info_id))
        }
    }
}

/// Map one `char` to its WinAnsi (CP1252) byte, or `None` when WinAnsi cannot
/// represent it. Pairs with a base-14 font declaring `/Encoding
/// /WinAnsiEncoding`, which the flatten path needs because it draws text into a
/// content stream instead of leaving a UTF-16 `/V` for the viewer.
pub(crate) fn winansi_byte(c: char) -> Option<u8> {
    let cp = c as u32;
    match cp {
        // ASCII and the upper Latin-1 range are identity-mapped in WinAnsi.
        0x00..=0x7F | 0xA0..=0xFF => Some(cp as u8),
        // The CP1252 `0x80..=0x9F` block holds typographic punctuation at code
        // points elsewhere in Unicode.
        _ => match c {
            '\u{20AC}' => Some(0x80), // €
            '\u{201A}' => Some(0x82), // ‚
            '\u{0192}' => Some(0x83), // ƒ
            '\u{201E}' => Some(0x84), // „
            '\u{2026}' => Some(0x85), // …
            '\u{2020}' => Some(0x86), // †
            '\u{2021}' => Some(0x87), // ‡
            '\u{02C6}' => Some(0x88), // ˆ
            '\u{2030}' => Some(0x89), // ‰
            '\u{0160}' => Some(0x8A), // Š
            '\u{2039}' => Some(0x8B), // ‹
            '\u{0152}' => Some(0x8C), // Œ
            '\u{017D}' => Some(0x8E), // Ž
            '\u{2018}' => Some(0x91), // ‘
            '\u{2019}' => Some(0x92), // ’
            '\u{201C}' => Some(0x93), // “
            '\u{201D}' => Some(0x94), // ”
            '\u{2022}' => Some(0x95), // •
            '\u{2013}' => Some(0x96), // –
            '\u{2014}' => Some(0x97), // —
            '\u{02DC}' => Some(0x98), // ˜
            '\u{2122}' => Some(0x99), // ™
            '\u{0161}' => Some(0x9A), // š
            '\u{203A}' => Some(0x9B), // ›
            '\u{0153}' => Some(0x9C), // œ
            '\u{017E}' => Some(0x9E), // ž
            '\u{0178}' => Some(0x9F), // Ÿ
            _ => None,
        },
    }
}

/// Transcode `s` to WinAnsi (CP1252) bytes, substituting `?` for any code point
/// WinAnsi cannot represent.
pub fn winansi_encode(s: &str) -> Vec<u8> {
    s.chars().map(|c| winansi_byte(c).unwrap_or(b'?')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_id_stops_at_the_reference_id_space() {
        let mut next = MAX_ID;
        assert_eq!(alloc_id(&mut next).unwrap(), MAX_ID);
        let err = alloc_id(&mut next).unwrap_err();
        assert_eq!(err.code, CODE_PARSE);
        assert!(err.message.contains("id space"), "{}", err.message);
    }

    #[test]
    fn to_ref_rejects_ids_no_reference_admits() {
        // Base object ids skip `alloc_id`, so these reach `to_ref` unbounded.
        assert!(to_ref(0).is_err());
        assert!(to_ref(MAX_ID + 1).is_err());
        assert!(to_ref(u32::MAX).is_err());
        assert_eq!(to_ref(1).unwrap(), Ref::new(1));
        assert_eq!(to_ref(MAX_ID).unwrap(), Ref::new(i32::MAX));
    }

    #[test]
    fn winansi_latin1_and_cp1252_punctuation() {
        assert_eq!(
            winansi_encode("café—it’s"),
            &[b'c', b'a', b'f', 0xE9, 0x97, b'i', b't', 0x92, b's']
        );
    }

    #[test]
    fn winansi_unmappable_becomes_question_mark() {
        assert_eq!(winansi_encode("日本語"), b"???");
        assert_eq!(winansi_encode("a😀b"), b"a?b");
    }

    #[test]
    fn pdf_text_string_escapes_ascii_literals() {
        assert_eq!(pdf_text_string("a(b)c\\d"), b"(a\\(b\\)c\\\\d)");
    }

    #[test]
    fn pdf_text_string_non_ascii_uses_utf16be_hex_with_bom() {
        // One non-ASCII char tips the whole string into the UTF-16BE hex form.
        assert_eq!(pdf_text_string("é"), b"<FEFF00E9>");
        assert_eq!(pdf_text_string("A€"), b"<FEFF004120AC>");
    }

    #[test]
    fn pdf_text_string_non_bmp_uses_surrogate_pair() {
        assert_eq!(pdf_text_string("😀"), b"<FEFFD83DDE00>");
    }

    #[test]
    fn upsert_producer_replaces_existing_value() {
        let info = b"/Title (Hi) /Producer (Old) /Creator (X)";
        let out = upsert_producer(info, b"(New)");
        assert_eq!(&out, b"/Title (Hi) /Producer (New) /Creator (X)");
    }

    #[test]
    fn upsert_producer_appends_when_absent() {
        let info = b"/Title (Hi)";
        let out = upsert_producer(info, b"(New)");
        assert_eq!(&out, b"/Title (Hi) /Producer (New)");
    }

    #[test]
    fn upsert_producer_ignores_producer_name_in_value_position() {
        // A `/Producer` Name in value position is not the key.
        let info = b"/Title (Hi) /Marker /Producer /Creator (X)";
        let out = upsert_producer(info, b"(New)");
        assert_eq!(
            &out,
            b"/Title (Hi) /Marker /Producer /Creator (X) /Producer (New)"
        );
    }
}
