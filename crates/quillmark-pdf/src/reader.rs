//! Minimal byte-level PDF reader and incremental-update writer: a deliberately
//! small scanner that parses just enough of a base PDF to splice one incremental
//! update onto it, and hard-errors on shapes a modern PDF can carry but this
//! reader does not handle.
//!
//! ## Input contract
//!
//! The base PDF must be traditional-xref, unencrypted, inline-annots,
//! bounded-tree: a classic `xref` table (not an xref *stream*), no `/Encrypt`,
//! page `/Annots` written inline rather than as an indirect reference, and a
//! `/Pages` tree of any depth that reaches each node once and stays under
//! 100 000 nodes. That is the precise inverse of the scanner's error branches.
//! `hayro-syntax` is read-only and exposes no byte spans, so it cannot drive a
//! byte-splice append; hence this bespoke scanner.

use std::collections::{HashMap, HashSet};

use crate::error::PdfError;

const CODE_PARSE: &str = "pdf::parse";
const CODE_XREF_STREAM: &str = "pdf::xref_stream";

pub(crate) fn err(code: &'static str, msg: impl Into<String>) -> PdfError {
    PdfError::new(code, msg)
}

/// The offset stored after the last `startxref` marker.
pub(crate) fn find_startxref(pdf: &[u8]) -> Result<usize, PdfError> {
    let needle = b"startxref";
    let from = pdf.len().saturating_sub(1024);
    let tail = &pdf[from..];
    let pos = tail
        .windows(needle.len())
        .rposition(|w| w == needle)
        .ok_or_else(|| err(CODE_PARSE, "missing startxref marker near EOF"))?;
    let after = skip_ws(&tail[pos + needle.len()..]);
    let mut end = 0;
    while end < after.len() && after[end].is_ascii_digit() {
        end += 1;
    }
    let offset: usize = std::str::from_utf8(&after[..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| err(CODE_PARSE, "startxref offset is not a valid integer"))?;
    // Bound the offset so every downstream `pdf[offset..]` slice is in range.
    if offset >= pdf.len() {
        return Err(err(CODE_PARSE, "startxref offset is past end of file"));
    }
    Ok(offset)
}

/// Bail if the base PDF stores an xref stream instead of a traditional table.
pub(crate) fn assert_traditional_xref(pdf: &[u8], xref_offset: usize) -> Result<(), PdfError> {
    if pdf.get(xref_offset..xref_offset + 4) != Some(b"xref") {
        return Err(err(
            CODE_XREF_STREAM,
            "PDF declares an xref stream; only traditional xref is supported",
        ));
    }
    Ok(())
}

/// The inner trailer dict (between `<<` and `>>`) for the xref section at
/// `xref_offset`, queryable with [`find_dict_value`].
pub(crate) fn find_trailer_dict(pdf: &[u8], xref_offset: usize) -> Result<&[u8], PdfError> {
    let needle = b"trailer";
    let pos = pdf[xref_offset..]
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| err(CODE_PARSE, "trailer marker not found"))?
        + xref_offset;
    extract_outer_dict(&pdf[pos + needle.len()..])
        .ok_or_else(|| err(CODE_PARSE, "trailer dict not parseable"))
}

/// What the trailer's `/Info` gives the producer stamp to rewrite.
pub(crate) enum InfoSource<'a> {
    /// Object `id`, rewritten in place under the trailer's existing reference.
    Object(u32),
    /// Entries for a fresh object whose reference replaces the trailer's
    /// `/Info`: a direct dict's — ISO 32000-1 Table 15 asks for an indirect
    /// reference, which not every writer honours — or none, when the trailer
    /// carries no `/Info` or one this reader cannot read.
    Entries(&'a [u8]),
}

pub(crate) fn read_info_source(trailer: &[u8]) -> InfoSource<'_> {
    let Some(value) = find_dict_value(trailer, "Info") else {
        return InfoSource::Entries(b"");
    };
    if let Some((id, _)) = parse_indirect_ref(value) {
        return InfoSource::Object(id);
    }
    let trimmed = value.trim_ascii();
    if trimmed.starts_with(b"<<")
        && let Some(entries) = extract_outer_dict(trimmed)
    {
        return InfoSource::Entries(entries);
    }
    InfoSource::Entries(b"")
}

/// Carry the prior trailer's `/ID` and `/Info` forward into the update's
/// trailer: many readers (lopdf included) consult only the last trailer, so
/// dropping them would lose the document `/Info` and file identifier.
/// `new_info_ref` supersedes that `/Info` rather than joining it, so the new
/// trailer holds one `/Info` whatever shape the old value had. Callers append
/// `/Size`, `/Root` and `/Prev` themselves.
fn write_trailer_tail(out: &mut Vec<u8>, prior_trailer: &[u8], new_info_ref: Option<u32>) {
    match new_info_ref {
        Some(id) => out.extend_from_slice(format!(" /Info {id} 0 R").as_bytes()),
        None => {
            if let Some(value) = find_dict_value(prior_trailer, "Info") {
                out.extend_from_slice(b" /Info ");
                out.extend_from_slice(value.trim_ascii());
            }
        }
    }
    if let Some(value) = find_dict_value(prior_trailer, "ID") {
        out.extend_from_slice(b" /ID ");
        out.extend_from_slice(value.trim_ascii());
    }
}

/// One object emitted into an incremental update, in full serialized form
/// (`<id> 0 obj … endobj`).
#[non_exhaustive]
pub struct UpdatedObject {
    pub id: u32,
    pub bytes: Vec<u8>,
}

impl UpdatedObject {
    pub fn new(id: u32, bytes: Vec<u8>) -> Self {
        Self { id, bytes }
    }
}

/// Append one incremental update to `pdf`: each object in `objects`, then an
/// xref subsection table and a trailer chaining to the prior xref via `/Prev`.
///
/// `new_info_ref` names an information dictionary written in this update, which
/// the new trailer's `/Info` points at in place of the prior trailer's own.
/// `new_size` is the updated `/Size` (highest object number + 1) and `root_id`
/// the document catalog.
pub(crate) fn append_incremental_update(
    mut pdf: Vec<u8>,
    prev_xref: usize,
    root_id: u32,
    new_size: u32,
    new_info_ref: Option<u32>,
    objects: &[UpdatedObject],
) -> Result<Vec<u8>, PdfError> {
    // Built while the prior trailer at `prev_xref` is still intact.
    let mut trailer_tail = Vec::new();
    write_trailer_tail(
        &mut trailer_tail,
        find_trailer_dict(&pdf, prev_xref)?,
        new_info_ref,
    );

    if !pdf.ends_with(b"\n") {
        pdf.push(b'\n');
    }
    let mut entries: Vec<(u32, usize)> = Vec::with_capacity(objects.len());
    for obj in objects {
        let off = pdf.len();
        entries.push((obj.id, off));
        pdf.extend_from_slice(&obj.bytes);
        // Keep each `N 0 obj` header a distinct token for any parser;
        // pdf_writer chunks do not always end in a newline.
        if !pdf.ends_with(b"\n") {
            pdf.push(b'\n');
        }
    }

    let new_xref_off = pdf.len();
    entries.sort_by_key(|(id, _)| *id);
    pdf.extend_from_slice(b"xref\n");
    // A traditional xref table is subsections headed by `<first-id> <count>`,
    // each followed by one 20-byte `OOOOOOOOOO GGGGG n ` entry. An update lists
    // only changed objects, so coalesce consecutive ids into the fewest
    // subsections.
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        pdf.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            pdf.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }

    pdf.extend_from_slice(format!("trailer\n<< /Size {new_size} /Root {root_id} 0 R").as_bytes());
    pdf.extend_from_slice(&trailer_tail);
    pdf.extend_from_slice(
        format!(" /Prev {prev_xref} >>\nstartxref\n{new_xref_off}\n%%EOF\n").as_bytes(),
    );
    Ok(pdf)
}

/// A base PDF and the offset of every indirect object header in it, collected in
/// one forward pass. Every read of an object goes through this.
///
/// A per-object scan cannot stop early — a base carrying prior incremental
/// updates serializes an id more than once and the live copy is the last — so
/// reading one object without an index walks the whole buffer, and a stamp or
/// flatten pass reads O(pages) objects.
///
/// A header is `<id> <generation> obj` at a token boundary, so `19 0 obj` is not
/// found inside `519 0 obj`, at any generation (re-saved PDFs carry non-zero
/// ones). A later occurrence overwrites an earlier one, so a lookup answers with
/// the live copy. Strings, `%`-comments and stream bodies are skipped, so header
/// bytes inside a string value or inside stream data cannot shadow the real
/// object.
pub struct ObjectIndex<'a> {
    pdf: &'a [u8],
    starts: HashMap<u32, usize>,
}

impl<'a> ObjectIndex<'a> {
    pub fn new(pdf: &'a [u8]) -> Self {
        let mut starts = HashMap::new();
        let mut i = 0;
        while i < pdf.len() {
            if let Some(ni) = skip_stream_body(pdf, i).or_else(|| skip_string_or_comment(pdf, i)) {
                i = ni;
                continue;
            }
            if pdf[i].is_ascii_digit()
                && (i == 0 || matches!(pdf[i - 1], b'\n' | b'\r' | b' '))
                && let Some(id) = obj_header_id(&pdf[i..])
            {
                starts.insert(id, i);
            }
            i += 1;
        }
        Self { pdf, starts }
    }

    /// The indexed bytes, for the scans that read no object.
    pub fn bytes(&self) -> &'a [u8] {
        self.pdf
    }

    /// `(obj_start, endobj_end)` of object `id`.
    pub fn object_bytes(&self, id: u32) -> Option<(usize, usize)> {
        let start = *self.starts.get(&id)?;
        Some((start, find_endobj_end(self.pdf, start)?))
    }

    /// The inner dict bytes of object `id`. `what` names the object in both
    /// failure messages — `"{what} not found"` and `"{what} dict not
    /// parseable"` — under the caller's error `code`; an Option-returning caller
    /// calls `.ok()`.
    pub fn dict(&self, id: u32, code: &'static str, what: &str) -> Result<&'a [u8], PdfError> {
        let (s, e) = self
            .object_bytes(id)
            .ok_or_else(|| err(code, format!("{what} not found")))?;
        extract_outer_dict(&self.pdf[s..e])
            .ok_or_else(|| err(code, format!("{what} dict not parseable")))
    }

    /// The generation in object `id`'s header, or `None` when the object is
    /// absent or its generation malformed.
    fn generation(&self, id: u32) -> Option<u16> {
        let start = *self.starts.get(&id)?;
        let header = &self.pdf[start..];
        let id_digits = header.iter().take_while(|b| b.is_ascii_digit()).count();
        // Past the id and the one space a header writes after it.
        let rest = &header[id_digits + 1..];
        let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        std::str::from_utf8(&rest[..n]).ok()?.parse().ok()
    }

    /// Reject overwriting a base object that lives at a non-zero generation.
    ///
    /// The update writer re-emits overwritten objects at generation 0 and
    /// references them as generation 0, while the reader accepts a header at any
    /// generation. A base whose catalog / page / `/Info` sits at a non-zero
    /// generation therefore parses fine yet would produce a malformed update: the
    /// new `/Root` points at generation 0 while the prior xref resolves the true
    /// generation.
    ///
    /// `None` (object absent) is left for the caller's not-found error path.
    pub(crate) fn assert_overwrite_gen_zero(&self, id: u32, what: &str) -> Result<(), PdfError> {
        match self.generation(id) {
            Some(0) | None => Ok(()),
            Some(generation) => Err(err(
                "pdf::nonzero_generation",
                format!(
                    "{what} object {id} is at generation {generation}; the stamp spine re-emits \
                     overwritten objects at generation 0 and cannot preserve a non-zero generation"
                ),
            )),
        }
    }
}

/// The object id of the `<id> <generation> obj` header at the start of `rest`,
/// when there is one. An id written with a leading zero is not one: the index
/// matches the exact decimal form a reference to the object is written in.
fn obj_header_id(rest: &[u8]) -> Option<u32> {
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if (digits > 1 && rest[0] == b'0')
        || rest.get(digits) != Some(&b' ')
        || !is_obj_header_tail(&rest[digits + 1..])
    {
        return None;
    }
    std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()
}

/// The index just past the `endobj` closing the body at `from`. Strings,
/// `%`-comments and stream bodies are skipped so those bytes inside a string
/// value, a comment or stream data cannot truncate the object early.
fn find_endobj_end(pdf: &[u8], from: usize) -> Option<usize> {
    let needle = b"endobj";
    let mut i = from;
    while i < pdf.len() {
        if let Some(ni) = skip_stream_body(pdf, i).or_else(|| skip_string_or_comment(pdf, i)) {
            i = ni;
            continue;
        }
        if pdf[i..].starts_with(needle) {
            return Some(i + needle.len());
        }
        i += 1;
    }
    None
}

/// Whether the bytes after an `<id> ` prefix continue as `<generation> obj`.
fn is_obj_header_tail(rest: &[u8]) -> bool {
    let gen_digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if gen_digits == 0 {
        return false;
    }
    let after_gen = &rest[gen_digits..];
    let ws = after_gen
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .count();
    if ws == 0 {
        return false;
    }
    let after_ws = &after_gen[ws..];
    after_ws.starts_with(b"obj") && after_ws.get(3).is_none_or(|b| !b.is_ascii_alphanumeric())
}

/// Locate `/Key` in a dict's *inner* bytes (between its `<<` / `>>`) and return
/// its raw value slice, beginning just after the key token.
///
/// Entries alternate `key value key value …`, so the scan reads a key Name then
/// consumes its value wholesale via `read_value_end` (stepping over nested
/// `<<>>` / `[]` / `()` / `<>` as a unit). Only keys are matched, so a Name in
/// value position (`/Subtype /Producer`) is never mistaken for one.
pub fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    loop {
        i = skip_ws_and_comments(dict_bytes, i);
        // A well-formed flat dict yields a Name key here. Anything else (end of
        // input, or a stray token) means there is no further key to match.
        if dict_bytes.get(i) != Some(&b'/') {
            return None;
        }
        let key_start = i;
        i += 1;
        while i < dict_bytes.len() && !is_pdf_delim(dict_bytes[i]) {
            i += 1;
        }
        let after_key = i;
        let matched = &dict_bytes[key_start..after_key] == km;
        let value_start = skip_ws_and_comments(dict_bytes, after_key);
        let value_end = read_value_end(dict_bytes, value_start)?;
        if matched {
            // Slice from after the key, not `value_start`, so the key span is
            // recoverable by subtraction.
            return Some(&dict_bytes[after_key..value_end]);
        }
        i = value_end;
    }
}

/// Replace `key`'s value in a flat dict. `value` MUST be the subslice
/// [`find_dict_value`] returned for that key: its start locates the key span by
/// pointer subtraction rather than a re-scan, so a `key` token inside another
/// value cannot be matched by accident. `key` is the on-page byte form,
/// including the leading slash (`b"/Producer"`).
pub fn splice_dict_value(dict: &[u8], key: &[u8], value: &[u8], new_value: &[u8]) -> Vec<u8> {
    let value_start = value.as_ptr() as usize - dict.as_ptr() as usize;
    let value_end = value_start + value.len();
    let key_at = value_start - key.len();
    let mut out =
        Vec::with_capacity(key_at + key.len() + 1 + new_value.len() + dict.len() - value_end);
    out.extend_from_slice(&dict[..key_at]);
    out.extend_from_slice(key);
    out.push(b' ');
    out.extend_from_slice(new_value);
    out.extend_from_slice(&dict[value_end..]);
    out
}

/// The index of the first significant byte at or after `start`, skipping
/// whitespace and `%`-comments (which run to end-of-line).
fn skip_ws_and_comments(b: &[u8], start: usize) -> usize {
    let mut i = start;
    loop {
        i = ws_end(b, i);
        if b.get(i) == Some(&b'%') {
            while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        return i;
    }
}

/// If `b[i]` opens a string — literal or hex — or a `%`-comment, the index just
/// past it, so a scanner steps over raw `<<`/`>>`/`[`/`]`/`endobj` bytes without
/// reading them as structure. A hex string's closing `>` is one of those:
/// against the enclosing dict's `>>` it forms a `>>` that is not the dict's. A
/// `<` followed by `<` opens a dict, not a hex string. `None` when `b[i]` opens
/// none of them, and the caller advances one byte.
fn skip_string_or_comment(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'(' => Some(skip_pdf_string(b, i)),
        b'<' if b.get(i + 1) != Some(&b'<') => Some(skip_pdf_hex_string(b, i)),
        b'%' => {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'\n' && b[j] != b'\r' {
                j += 1;
            }
            Some(j)
        }
        _ => None,
    }
}

/// If `b[i]` opens a stream body — the `stream` keyword at a token boundary,
/// followed by CRLF or LF per ISO 32000 §7.3.8 — the index just past the
/// `endstream` closing it, so raw stream data is never read as structure. `None`
/// when `b[i]` opens no stream, and when no `endstream` follows: a truncated
/// stream is then scanned as ordinary bytes rather than swallowing the rest of
/// the file.
fn skip_stream_body(b: &[u8], i: usize) -> Option<usize> {
    const OPEN: &[u8] = b"stream";
    const CLOSE: &[u8] = b"endstream";
    if !b[i..].starts_with(OPEN) || !(i == 0 || is_pdf_delim(b[i - 1])) {
        return None;
    }
    let after_kw = i + OPEN.len();
    let body = match b.get(after_kw)? {
        b'\n' => after_kw + 1,
        b'\r' if b.get(after_kw + 1) == Some(&b'\n') => after_kw + 2,
        _ => return None,
    };
    (body..b.len().saturating_sub(CLOSE.len() - 1))
        .find(|&j| b[j..].starts_with(CLOSE))
        .map(|j| j + CLOSE.len())
}

/// The index after the last byte of the value beginning at `start`, whose
/// leading whitespace is skipped before the value type is classified.
fn read_value_end(b: &[u8], start: usize) -> Option<usize> {
    let mut i = ws_end(b, start);
    if i >= b.len() {
        return Some(i);
    }
    match b[i] {
        b'[' => {
            let mut depth = 1;
            i += 1;
            while i < b.len() {
                if let Some(ni) = skip_string_or_comment(b, i) {
                    i = ni;
                    continue;
                }
                if b[i] == b'[' {
                    depth += 1;
                } else if b[i] == b']' {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                i += 1;
            }
            Some(i)
        }
        b'(' => Some(skip_pdf_string(b, i)),
        b'<' if b[i..].starts_with(b"<<") => Some(match dict_end(b, i) {
            Ok(close) => close + 2,
            Err(stop) => stop,
        }),
        b'<' => Some(skip_pdf_hex_string(b, i)),
        b'/' => {
            i += 1;
            while i < b.len() && !is_pdf_delim(b[i]) {
                i += 1;
            }
            Some(i)
        }
        c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
            // Possibly `N N R`; the standalone-R check rejects `5 0 Rect`.
            let num_end = read_number_end(b, i);
            let mut j = num_end;
            while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            let n2_start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > n2_start {
                while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                if b.get(j).copied() == Some(b'R') && b.get(j + 1).is_none_or(|c| is_pdf_delim(*c))
                {
                    return Some(j + 1);
                }
            }
            Some(num_end)
        }
        _ => {
            while i < b.len() && !is_pdf_delim(b[i]) {
                i += 1;
            }
            Some(i)
        }
    }
}

fn read_number_end(b: &[u8], start: usize) -> usize {
    let mut i = start;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    i
}

/// `start` points at `(`. Returns index AFTER the matching `)`.
fn skip_pdf_string(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1;
    while i < b.len() && depth > 0 {
        match b[i] {
            b'\\' => i = (i + 2).min(b.len()),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// `start` points at `<` (not `<<`). Returns index AFTER the closing `>`.
fn skip_pdf_hex_string(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < b.len() && b[i] != b'>' {
        i += 1;
    }
    (i + 1).min(b.len())
}

fn is_pdf_delim(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'[' | b']' | b'(' | b')' | b'<' | b'>'
    )
}

pub fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
    let s = skip_ws(s);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let id: u32 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let generation: u16 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    if !s.starts_with(b"R") {
        return None;
    }
    // Standalone-R check rejects identifiers like `Roller`.
    if !s.get(1).is_none_or(|c| is_pdf_delim(*c)) {
        return None;
    }
    Some((id, generation))
}

/// Slice between the outermost `<< ... >>` of an indirect object's body.
pub fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
    let open = obj_bytes.windows(2).position(|w| w == b"<<")?;
    let close = dict_end(obj_bytes, open).ok()?;
    Some(&obj_bytes[open + 2..close])
}

/// The index of the `>>` matching the `<<` at `open`. Strings and `%`-comments
/// are skipped: any of them can carry `<<` / `>>` as raw bytes that would
/// otherwise skew the nesting depth. `Err` carries the index the scan ran out
/// at, for a caller that reads an unbalanced dict leniently.
fn dict_end(b: &[u8], open: usize) -> Result<usize, usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i + 1 < b.len() {
        if let Some(ni) = skip_string_or_comment(b, i) {
            i = ni;
            continue;
        }
        if b[i..].starts_with(b"<<") {
            depth += 1;
            i += 2;
        } else if b[i..].starts_with(b">>") {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Err(i)
}

/// The index of the first byte at or after `i` that is not whitespace.
fn ws_end(b: &[u8], i: usize) -> usize {
    let mut i = i;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
        i += 1;
    }
    i
}

fn skip_ws(s: &[u8]) -> &[u8] {
    &s[ws_end(s, 0)..]
}

/// Open the base's trailer: the xref offset, the trailer dict, and the catalog
/// (`/Root`) object id, after refusing an xref stream. `code` carries the
/// caller's error code for a missing or malformed `/Root`.
pub(crate) fn open_trailer<'a>(
    pdf: &'a [u8],
    code: &'static str,
) -> Result<(usize, &'a [u8], u32), PdfError> {
    let xref_offset = find_startxref(pdf)?;
    assert_traditional_xref(pdf, xref_offset)?;
    let trailer = find_trailer_dict(pdf, xref_offset)?;
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(code, "/Root missing or malformed in trailer"))?;
    Ok((xref_offset, trailer, catalog_id))
}

/// A page object and the `/Pages` nodes it descends from, nearest ancestor
/// first: the chain an inheritable attribute resolves along.
#[derive(Debug)]
pub struct Page {
    pub id: u32,
    ancestors: Vec<u32>,
}

impl Page {
    /// The inheritable attribute `key`, `parse`d from the page dict, else from
    /// the nearest ancestor `/Pages` node carrying a parseable one
    /// (ISO 32000-1 §7.7.3.4). `parse` returning `None` keeps the search
    /// climbing, so a caller that wants the first *present* value wraps its
    /// result in `Some`.
    pub fn inherited_attribute<T>(
        &self,
        idx: &ObjectIndex,
        key: &str,
        parse: impl Fn(&[u8]) -> Option<T>,
    ) -> Option<T> {
        std::iter::once(self.id)
            .chain(self.ancestors.iter().copied())
            .find_map(|id| {
                let dict = idx.dict(id, CODE_PARSE, "page node").ok()?;
                parse(find_dict_value(dict, key)?)
            })
    }
}

/// Flatten the catalog's `/Pages` tree into its page objects in document order,
/// each carrying its ancestor chain. The walk is capped to prevent runaway on a
/// pathological PDF.
pub(crate) fn walk_page_tree(idx: &ObjectIndex, catalog_id: u32) -> Result<Vec<Page>, PdfError> {
    let root_pages_id = root_pages_id(idx, catalog_id)?;

    const MAX_NODES: usize = 100_000;
    let mut out = Vec::new();
    let mut stack = vec![(root_pages_id, Vec::<u32>::new())];
    // A node reached twice is a cyclic or shared-node `/Pages` tree: a `/Kids`
    // self-cycle would otherwise walk until MAX_NODES.
    let mut seen: HashSet<u32> = HashSet::new();
    while let Some((node_id, ancestors)) = stack.pop() {
        if !seen.insert(node_id) {
            return Err(err(
                CODE_PARSE,
                format!("page tree revisits node {node_id} (cycle or shared node)"),
            ));
        }
        if seen.len() > MAX_NODES {
            return Err(err(CODE_PARSE, "page tree exceeds 100 000 nodes"));
        }
        let dict = idx.dict(node_id, CODE_PARSE, &format!("page node {node_id}"))?;
        let typ = find_dict_value(dict, "Type")
            .map(|b| String::from_utf8_lossy(b.trim_ascii()).into_owned())
            .unwrap_or_default();
        if typ.starts_with("/Pages") {
            let kids = find_dict_value(dict, "Kids")
                .ok_or_else(|| err(CODE_PARSE, "/Pages node missing /Kids"))?;
            let mut kid_ancestors = Vec::with_capacity(ancestors.len() + 1);
            kid_ancestors.push(node_id);
            kid_ancestors.extend_from_slice(&ancestors);
            let mut kid_ids: Vec<u32> = parse_ref_array(kids)
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            kid_ids.reverse();
            stack.extend(kid_ids.into_iter().map(|id| (id, kid_ancestors.clone())));
        } else {
            out.push(Page {
                id: node_id,
                ancestors,
            });
        }
    }
    Ok(out)
}

/// The catalog's root `/Pages` node id.
fn root_pages_id(idx: &ObjectIndex, catalog_id: u32) -> Result<u32, PdfError> {
    let cat_dict = idx.dict(catalog_id, CODE_PARSE, "catalog")?;
    find_dict_value(cat_dict, "Pages")
        .and_then(parse_indirect_ref)
        .map(|(id, _)| id)
        .ok_or_else(|| err(CODE_PARSE, "catalog /Pages reference not found"))
}

/// Reject a page with a non-zero `/Rotate`, its own value or the one inherited
/// from its nearest ancestor `/Pages` node. The stamp and flatten paths write
/// geometry in unrotated user space and do not compensate, so a rotated base
/// page would display every widget away from its intended box. The first
/// *present* value binds, and one that is not a direct integer — an indirect
/// reference, say — is an error rather than an absence falling to the default
/// zero.
pub(crate) fn assert_unrotated_pages<'p>(
    idx: &ObjectIndex,
    pages: impl IntoIterator<Item = &'p Page>,
) -> Result<(), PdfError> {
    for page in pages {
        let Some(raw) =
            page.inherited_attribute(idx, "Rotate", |raw| Some(raw.trim_ascii().to_vec()))
        else {
            continue;
        };
        let rotate: i64 = std::str::from_utf8(&raw)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                err(
                    CODE_PARSE,
                    format!(
                        "page object {} has /Rotate {}, which is not an integer",
                        page.id,
                        String::from_utf8_lossy(&raw)
                    ),
                )
            })?;
        if rotate.rem_euclid(360) != 0 {
            return Err(err(
                "pdf::rotated_page",
                format!(
                    "page object {} has /Rotate {rotate}; the stamp spine only \
                     handles unrotated pages",
                    page.id
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn parse_ref_array(bytes: &[u8]) -> Vec<(u32, u16)> {
    let mut s = bytes;
    if let Some(l) = s.iter().position(|&b| b == b'[') {
        s = &s[l + 1..];
    }
    if let Some(r) = s.iter().position(|&b| b == b']') {
        s = &s[..r];
    }
    let mut out = Vec::new();
    let mut cur = s;
    loop {
        cur = skip_ws(cur);
        if cur.is_empty() {
            break;
        }
        match parse_indirect_ref(cur) {
            Some((id, generation)) => {
                out.push((id, generation));
                if let Some(pos) = cur.iter().position(|&b| b == b'R') {
                    cur = &cur[pos + 1..];
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    out
}

/// Parse a 4-number array (`[x0 y0 x1 y1]`) such as `/MediaBox`.
fn parse_rect_array(bytes: &[u8]) -> Option<[f32; 4]> {
    let trimmed = bytes.trim_ascii();
    let inner = trimmed.strip_prefix(b"[")?.strip_suffix(b"]")?;
    let mut nums = [0.0f32; 4];
    let mut count = 0;
    for tok in String::from_utf8_lossy(inner).split_whitespace() {
        if count >= 4 {
            return None;
        }
        nums[count] = tok.parse().ok().filter(|f: &f32| f.is_finite())?;
        count += 1;
    }
    (count == 4).then_some(nums)
}

/// Normalize a `/MediaBox` so `(x0, y0)` is lower-left and `(x1, y1)`
/// upper-right, whichever corners the array listed.
fn normalize_rect(mb: [f32; 4]) -> [f32; 4] {
    [
        mb[0].min(mb[2]),
        mb[1].min(mb[3]),
        mb[0].max(mb[2]),
        mb[1].max(mb[3]),
    ]
}

/// The `/MediaBox` of every page, normalized to `[x0, y0, x1, y1]`, in document
/// order, taken from the nearest ancestor `/Pages` node carrying one when a page
/// declares none. The full rect rather than width/height, so a caller flipping
/// page-relative top-left geometry can honour a non-zero page origin.
pub(crate) fn page_media_boxes(pdf: &[u8]) -> Result<Vec<[f32; 4]>, PdfError> {
    let (_, _, catalog_id) = open_trailer(pdf, CODE_PARSE)?;
    media_boxes_of(&ObjectIndex::new(pdf), catalog_id)
}

fn media_boxes_of(idx: &ObjectIndex, catalog_id: u32) -> Result<Vec<[f32; 4]>, PdfError> {
    let pages = walk_page_tree(idx, catalog_id)?;
    let mut out = Vec::with_capacity(pages.len());
    for page in &pages {
        let mb = page
            .inherited_attribute(idx, "MediaBox", parse_rect_array)
            .ok_or_else(|| {
                err(
                    CODE_PARSE,
                    format!("page {} has no resolvable /MediaBox", page.id),
                )
            })?;
        out.push(normalize_rect(mb));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_value_handles_nested_dict() {
        let dict = b" /Resources << /ColorSpace << /Color /DeviceGray >> >> /Pages 7 0 R ";
        let v = find_dict_value(dict, "Pages").expect("found /Pages");
        let s = std::str::from_utf8(v).unwrap().trim();
        assert_eq!(s, "7 0 R");
    }

    #[test]
    fn dict_value_finds_array_value() {
        let dict = b" /MediaBox [0 0 612 792] /Other 1 ";
        let v = find_dict_value(dict, "MediaBox").expect("found");
        assert_eq!(parse_rect_array(v), Some([0.0, 0.0, 612.0, 792.0]));
    }

    #[test]
    fn dict_value_ignores_name_in_value_position() {
        let dict = b" /Subtype /Producer /Producer (real) /Creator (X) ";
        let v = find_dict_value(dict, "Producer").expect("found the key, not the value");
        assert_eq!(v.trim_ascii(), b"(real)");
    }

    #[test]
    fn dict_value_skips_comments_between_entries() {
        let dict = b" /A 1 %decoy /Producer (decoy)\n /Producer (real) ";
        let v = find_dict_value(dict, "Producer").expect("found");
        assert_eq!(v.trim_ascii(), b"(real)");
    }

    #[test]
    fn endobj_inside_comment_does_not_truncate_object() {
        let pdf = b"%PDF\n3 0 obj\n<< /A 1 >> %endobj in a comment\n/B 2 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(3).expect("found object 3");
        assert_eq!(&pdf[e - 6..e], b"endobj");
        assert!(&pdf[s..e].ends_with(b"/B 2 >>\nendobj"));
    }

    #[test]
    fn outer_dict_skips_comment_bearing_gt_gt() {
        let obj = b"5 0 obj\n<< /A 1 %trailing >> in a comment\n /MediaBox [0 0 1 2] >>\nendobj\n";
        let dict = extract_outer_dict(obj).expect("dict parses");
        let mb = find_dict_value(dict, "MediaBox").expect("/MediaBox survives the comment");
        assert_eq!(parse_rect_array(mb), Some([0.0, 0.0, 1.0, 2.0]));
    }

    #[test]
    fn outer_dict_ends_after_a_trailing_hex_string() {
        for (obj, inner) in [
            (&b"<< /T <41>>>"[..], &b" /T <41>"[..]),
            (b"<< /T <>>>", b" /T <>"),
            (b"<< /T (a)>>", b" /T (a)"),
            (b"<< /D << /T <41>>>>>", b" /D << /T <41>>>"),
        ] {
            assert_eq!(extract_outer_dict(obj), Some(inner));
        }
    }

    #[test]
    fn value_end_nested_dict_skips_string_and_comment_gt_gt() {
        let dict = b" /K << /S (a>>b) %c >> d\n /T 3 >> /After 9 0 R ";
        let after = find_dict_value(dict, "After").expect("/After after the nested dict");
        assert_eq!(after.trim_ascii(), b"9 0 R");
    }

    #[test]
    fn value_end_array_skips_comment_bracket() {
        let dict = b" /Arr [1 2 %x]\n 3] /After (real) ";
        let after = find_dict_value(dict, "After").expect("/After after the array");
        assert_eq!(after.trim_ascii(), b"(real)");
    }

    #[test]
    fn ref_array_parses_basic() {
        let bytes = b"[5 0 R 7 0 R 9 0 R]";
        let v = parse_ref_array(bytes);
        assert_eq!(v, vec![(5u32, 0u16), (7, 0), (9, 0)]);
    }

    #[test]
    fn indirect_ref_rejects_non_ref() {
        assert!(parse_indirect_ref(b"5 0 R").is_some());
        assert!(parse_indirect_ref(b"5 0 G").is_none());
        assert!(parse_indirect_ref(b"abc").is_none());
    }

    #[test]
    fn rect_array_rejects_wrong_arity() {
        assert_eq!(parse_rect_array(b"[0 0 612]"), None);
        assert_eq!(parse_rect_array(b"[0 0 612 792 1]"), None);
        assert_eq!(parse_rect_array(b"0 0 612 792"), None);
    }

    #[test]
    fn normalize_rect_orders_corners() {
        assert_eq!(
            normalize_rect([10.0, 20.0, 622.0, 812.0]),
            [10.0, 20.0, 622.0, 812.0]
        );
        assert_eq!(
            normalize_rect([622.0, 812.0, 10.0, 20.0]),
            [10.0, 20.0, 622.0, 812.0]
        );
    }

    #[test]
    fn rect_array_rejects_non_finite() {
        assert_eq!(parse_rect_array(b"[0 0 inf 792]"), None);
        assert_eq!(parse_rect_array(b"[0 0 612 nan]"), None);
        assert_eq!(parse_rect_array(b"[-inf 0 612 792]"), None);
    }

    #[test]
    fn find_object_at_token_boundary() {
        let pdf = b"%PDF\n519 0 obj\n<< /A 1 >>\nendobj\n19 0 obj\n<< /B 2 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(19).expect("found object 19");
        assert_eq!(&pdf[s..e], b"19 0 obj\n<< /B 2 >>\nendobj");
    }

    #[test]
    fn index_resolves_every_object_from_one_pass() {
        let pdf = b"%PDF\n1 0 obj\n<< /A 1 >>\nendobj\n2 0 obj\n<< /B 2 >>\nendobj\n\
                    3 0 obj\n<< /C 3 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        for (id, body) in [(1u32, &b"/A 1"[..]), (2, b"/B 2"), (3, b"/C 3")] {
            assert_eq!(idx.dict(id, CODE_PARSE, "obj").unwrap().trim_ascii(), body);
        }
        assert!(idx.object_bytes(4).is_none());
    }

    #[test]
    fn index_ignores_a_leading_zero_id_header() {
        // `019` is not how a `19 0 R` reference writes the id, so it is not
        // object 19 and does not supersede the real one.
        let pdf = b"%PDF\n19 0 obj\n<< /V (real) >>\nendobj\n019 0 obj\n<< /V (decoy) >>\nendobj\n";
        let dict = ObjectIndex::new(pdf).dict(19, CODE_PARSE, "obj").unwrap();
        assert_eq!(find_dict_value(dict, "V").unwrap().trim_ascii(), b"(real)");
    }

    #[test]
    fn object_generation_reads_header_gen() {
        let pdf = b"%PDF\n7 2 obj\n<< /C 3 >>\nendobj\n4 0 obj\n<< /D 1 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        assert_eq!(idx.generation(7), Some(2));
        assert_eq!(idx.generation(4), Some(0));
        assert_eq!(idx.generation(99), None);
    }

    #[test]
    fn assert_overwrite_gen_zero_rejects_nonzero() {
        let pdf = b"%PDF\n7 2 obj\n<< /C 3 >>\nendobj\n4 0 obj\n<< /D 1 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        assert!(idx.assert_overwrite_gen_zero(4, "x").is_ok());
        // Absent is accepted: the caller owns the not-found path.
        assert!(idx.assert_overwrite_gen_zero(99, "x").is_ok());
        let e = idx
            .assert_overwrite_gen_zero(7, "catalog")
            .expect_err("generation 2 rejected");
        assert_eq!(e.code, "pdf::nonzero_generation");
        assert!(e.message.contains("generation 2"), "{}", e.message);
    }

    #[test]
    fn find_object_returns_last_revision() {
        // Same id serialized twice, as an incremental update writes it.
        let pdf = b"%PDF\n4 0 obj\n<< /V (old) >>\nendobj\n4 0 obj\n<< /V (new) >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(4).expect("found object 4");
        assert_eq!(&pdf[s..e], b"4 0 obj\n<< /V (new) >>\nendobj");
    }

    #[test]
    fn endobj_inside_string_does_not_truncate_object() {
        let pdf = b"%PDF\n3 0 obj\n<< /Title (My endobj report) /Author (X) >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(3).expect("found object 3");
        assert_eq!(
            &pdf[s..e],
            b"3 0 obj\n<< /Title (My endobj report) /Author (X) >>\nendobj"
);
        let dict = extract_outer_dict(&pdf[s..e]).expect("dict parses");
        let title = find_dict_value(dict, "Title").expect("/Title");
        assert_eq!(title.trim_ascii(), b"(My endobj report)");
    }

    #[test]
    fn obj_header_inside_string_does_not_shadow_object() {
        let pdf = b"%PDF\n4 0 obj\n<< /V (real) >>\nendobj\n\
                    5 0 obj\n<< /Subject (see 4 0 obj for the rest) >>\nendobj\n";
        let dict = ObjectIndex::new(pdf).dict(4, CODE_PARSE, "obj").unwrap();
        assert_eq!(find_dict_value(dict, "V").unwrap().trim_ascii(), b"(real)");
    }

    #[test]
    fn obj_header_inside_stream_body_does_not_shadow_object() {
        let pdf = b"%PDF\n4 0 obj\n<< /V (real) >>\nendobj\n\
                    5 0 obj\n<< /Length 13 >>\nstream\n4 0 obj junk\nendstream\nendobj\n";
        let dict = ObjectIndex::new(pdf).dict(4, CODE_PARSE, "obj").unwrap();
        assert_eq!(find_dict_value(dict, "V").unwrap().trim_ascii(), b"(real)");
    }

    #[test]
    fn object_after_a_stream_body_is_indexed() {
        // The stream's unbalanced `(` would otherwise run the string skipper to EOF.
        let pdf = b"%PDF\n5 0 obj\n<< /Length 12 >>\nstream\n(unbalanced\nendstream\nendobj\n\
                    6 0 obj\n<< /V (after) >>\nendobj\n";
        let dict = ObjectIndex::new(pdf).dict(6, CODE_PARSE, "obj").unwrap();
        assert_eq!(find_dict_value(dict, "V").unwrap().trim_ascii(), b"(after)");
    }

    #[test]
    fn endobj_inside_stream_body_does_not_truncate_object() {
        let pdf = b"%PDF\n5 0 obj\n<< /Length 9 >>\nstream\nendobj x\nendstream\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(5).expect("found object 5");
        assert!(pdf[s..e].ends_with(b"endstream\nendobj"));
    }

    #[test]
    fn page_tree_cycle_is_rejected() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [2 0 R] /Count 1 >>\nendobj\n";
        let e = walk_page_tree(&ObjectIndex::new(pdf), 1).expect_err("cycle rejected");
        assert_eq!(e.code, CODE_PARSE);
        assert!(e.message.contains("revisits"), "{}", e.message);
    }

    #[test]
    fn rotated_page_is_rejected() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /Rotate 90 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let pages = walk_page_tree(&idx, 1).expect("page tree walks");
        let e = assert_unrotated_pages(&idx, &pages).expect_err("rotated page rejected");
        assert_eq!(e.code, "pdf::rotated_page");
        let flat = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                     2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                     3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let flat_idx = ObjectIndex::new(flat);
        let flat_pages = walk_page_tree(&flat_idx, 1).expect("page tree walks");
        assert!(assert_unrotated_pages(&flat_idx, &flat_pages).is_ok());
    }

    #[test]
    fn rotate_resolves_to_the_nearest_ancestor_carrying_it() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [5 0 R] /Count 2 >>\nendobj\n\
                    5 0 obj\n<< /Type /Pages /Parent 2 0 R /Kids [3 0 R 4 0 R] /Count 2 \
                    /Rotate 90 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 5 0 R >>\nendobj\n\
                    4 0 obj\n<< /Type /Page /Parent 5 0 R /Rotate 0 >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let pages = walk_page_tree(&idx, 1).expect("page tree walks");
        let e = assert_unrotated_pages(&idx, &pages[..1])
            .expect_err("an intermediate /Pages node's /Rotate reaches its pages");
        assert_eq!(e.code, "pdf::rotated_page");
        assert!(
            assert_unrotated_pages(&idx, &pages[1..]).is_ok(),
            "a page's own /Rotate 0 outranks its ancestor's 90"
        );
    }

    #[test]
    fn media_box_resolves_to_the_nearest_ancestor_carrying_it() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [5 0 R 4 0 R] /Count 2 \
                    /MediaBox [0 0 612 792] >>\nendobj\n\
                    5 0 obj\n<< /Type /Pages /Parent 2 0 R /Kids [3 0 R] /Count 1 \
                    /MediaBox [0 0 200 400] >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 5 0 R >>\nendobj\n\
                    4 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let boxes = media_boxes_of(&ObjectIndex::new(pdf), 1).expect("media boxes resolve");
        assert_eq!(boxes, [[0.0, 0.0, 200.0, 400.0], [0.0, 0.0, 612.0, 792.0]]);
    }
}
