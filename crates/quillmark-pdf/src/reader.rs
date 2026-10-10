//! Minimal byte-level PDF reader and incremental-update writer: a deliberately
//! small scanner that parses just enough of a base PDF to splice one incremental
//! update onto it, and hard-errors on shapes a modern PDF can carry but this
//! reader does not handle.
//!
//! ## Input contract
//!
//! The base PDF must be traditional-xref, unencrypted, inline-annots,
//! bounded-tree, well-formed: a classic `xref` table in every section (not an
//! xref *stream*, nor a hybrid trailer naming `/XRefStm`), no `/Encrypt`, page
//! `/Annots` written inline rather than as an indirect reference, a `/Pages`
//! tree of any depth that reaches each node once and stays under 100 000 nodes,
//! and every dictionary the reader meets naming each key once, with a value. That is the precise inverse of the scanner's error
//! branches.
//! `hayro-syntax` is read-only and exposes no byte spans, so it cannot drive a
//! byte-splice append; hence this bespoke scanner.

use std::borrow::Cow;
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

/// Bail if a section the trailer chains to through `/Prev` is an xref stream,
/// or the trailer of any names `/XRefStm`: a hybrid file (ISO 32000-1
/// §7.5.8.4) keeps objects in object streams, where they carry no `obj` header
/// and the index would read each as absent. The walk ends quietly at a `/Prev`
/// that reaches no section.
fn assert_no_object_streams(pdf: &[u8], trailer: &[u8]) -> Result<(), PdfError> {
    let mut trailer = trailer;
    let mut seen = HashSet::new();
    loop {
        if find_dict_value(trailer, "XRefStm").is_some() {
            return Err(err(
                CODE_XREF_STREAM,
                "PDF is a hybrid file whose trailer names /XRefStm; objects in its object \
                 streams are unreadable here, so only traditional xref is supported",
            ));
        }
        let Some(prev) = find_dict_value(trailer, "Prev")
            .and_then(|v| std::str::from_utf8(v).ok()?.parse::<usize>().ok())
            .filter(|&prev| prev < pdf.len() && seen.insert(prev))
        else {
            return Ok(());
        };
        if obj_header_id(&pdf[prev..]).is_some() {
            return assert_traditional_xref(pdf, prev);
        }
        match find_trailer_dict(pdf, prev) {
            Ok(prior) if pdf[prev..].starts_with(b"xref") => trailer = prior,
            _ => return Ok(()),
        }
    }
}

/// The inner trailer dict (between `<<` and `>>`) for the xref section at
/// `xref_offset`, [`well_formed`] and queryable with [`find_dict_value`].
pub(crate) fn find_trailer_dict(pdf: &[u8], xref_offset: usize) -> Result<&[u8], PdfError> {
    let needle = b"trailer";
    let pos = pdf[xref_offset..]
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| err(CODE_PARSE, "trailer marker not found"))?
        + xref_offset;
    parse_dict(&pdf[pos + needle.len()..], CODE_PARSE, "trailer")
}

/// What the trailer's `/Info` gives the producer stamp to rewrite.
pub(crate) enum InfoSource<'a> {
    /// Object `id`, rewritten in place under the trailer's existing reference:
    /// the one its chain of references ends at.
    Object(u32),
    /// Entries for a fresh object whose reference replaces the trailer's
    /// `/Info`: a direct dict's, up to the end of its last one so no trailing
    /// comment runs over what follows — ISO 32000-1 Table 15 asks for an
    /// indirect reference, which not every writer honours — or none, when the
    /// trailer
    /// carries no `/Info` or one this reader cannot read: not a dict, or one
    /// [`well_formed`] refuses.
    Entries(&'a [u8]),
}

pub(crate) fn read_info_source<'t>(idx: &ObjectIndex, trailer: &'t [u8]) -> InfoSource<'t> {
    let Some(value) = idx.value(trailer, "Info") else {
        return InfoSource::Entries(b"");
    };
    if parse_indirect_ref(value).is_some() {
        return idx
            .referent(value)
            .map_or(InfoSource::Entries(b""), InfoSource::Object);
    }
    match as_dict(value.trim_ascii(), CODE_PARSE, "/Info") {
        Ok(Some(entries)) => InfoSource::Entries(&entries[..entries_end(entries)]),
        _ => InfoSource::Entries(b""),
    }
}

/// Carry the prior trailer's `/ID` and `/Info` forward into the update's
/// trailer: many readers (lopdf included) consult only the last trailer, so
/// dropping them would lose the document `/Info` and file identifier.
/// `new_info_ref` supersedes that `/Info` rather than joining it, so the new
/// trailer holds one `/Info` whatever shape the old value had.
pub(crate) fn write_trailer_tail(
    out: &mut Vec<u8>,
    idx: &ObjectIndex,
    prior_trailer: &[u8],
    new_info_ref: Option<u32>,
) {
    match new_info_ref {
        Some(id) => out.extend_from_slice(format!(" /Info {id} 0 R").as_bytes()),
        None => {
            if let Some(value) = idx.value(prior_trailer, "Info") {
                out.extend_from_slice(b" /Info ");
                out.extend_from_slice(value.trim_ascii());
            }
        }
    }
    if let Some(value) = idx.value(prior_trailer, "ID") {
        out.extend_from_slice(b" /ID ");
        out.extend_from_slice(value.trim_ascii());
    }
}

/// One object emitted into an incremental update, in full serialized form
/// (`<id> 0 obj … endobj`).
pub(crate) struct UpdatedObject {
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
/// `trailer_tail` holds the entries [`write_trailer_tail`] carries forward.
/// `new_size` is the updated `/Size` (highest object number + 1) and `root_id`
/// the document catalog.
pub(crate) fn append_incremental_update(
    mut pdf: Vec<u8>,
    prev_xref: usize,
    root_id: u32,
    new_size: u32,
    trailer_tail: &[u8],
    objects: &[UpdatedObject],
) -> Vec<u8> {
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
    pdf.extend_from_slice(trailer_tail);
    pdf.extend_from_slice(
        format!(" /Prev {prev_xref} >>\nstartxref\n{new_xref_off}\n%%EOF\n").as_bytes(),
    );
    pdf
}

/// How many references [`ObjectIndex::value`] and [`ObjectIndex::resolve`]
/// follow: a longer chain, a cycle included, reads as present to `value` and
/// as `None` to `resolve`.
const MAX_REFERENCE_CHAIN: usize = 8;

/// A base PDF and the offset of every indirect object header in it, collected in
/// one forward pass. Every read of an object goes through this.
///
/// A header is `<id> <generation> obj` at a token boundary and at any generation,
/// so `19 0 obj` is not found inside `519 0 obj`. Strings, `%`-comments and
/// stream bodies are skipped, so bytes inside them cannot shadow a real header.
/// A base carrying prior incremental updates serializes an id more than once and
/// the live copy is the last, which is the one a lookup answers with.
pub(crate) struct ObjectIndex<'a> {
    pdf: &'a [u8],
    starts: HashMap<u32, usize>,
    unnamed_from: u32,
}

impl<'a> ObjectIndex<'a> {
    pub fn new(pdf: &'a [u8]) -> Self {
        let mut starts = HashMap::new();
        let mut unnamed_from = 0u32;
        let mut i = 0;
        while i < pdf.len() {
            if let Some(ni) = skip_stream_body(pdf, i).or_else(|| skip_string_or_comment(pdf, i)) {
                i = ni;
                continue;
            }
            if pdf[i].is_ascii_digit() && (i == 0 || is_pdf_delim(pdf[i - 1])) {
                let header = (i == 0 || is_pdf_ws(pdf[i - 1]))
                    .then(|| obj_header_id(&pdf[i..]))
                    .flatten();
                if let Some(id) = header {
                    starts.insert(id, i);
                }
                if let Some(id) = header.or_else(|| parse_indirect_ref(&pdf[i..]).map(|(id, _)| id))
                {
                    unnamed_from = unnamed_from.max(id.saturating_add(1));
                }
            }
            i += 1;
        }
        Self {
            pdf,
            starts,
            unnamed_from,
        }
    }

    /// The least id that no object header or reference in the base names, nor
    /// any id above it. A reference past the trailer's `/Size` names no object,
    /// so an update allocating from here leaves it naming none.
    pub fn unnamed_from(&self) -> u32 {
        self.unnamed_from
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

    /// The inner dict bytes of object `id`, [`well_formed`]. `what` names the
    /// object in every failure message, under the caller's error `code`; an
    /// Option-returning caller calls `.ok()`.
    pub fn dict(&self, id: u32, code: &'static str, what: &str) -> Result<&'a [u8], PdfError> {
        let (s, e) = self
            .object_bytes(id)
            .ok_or_else(|| err(code, format!("{what} not found")))?;
        parse_dict(&self.pdf[s..e], code, what)
    }

    /// [`find_dict_value`], reading a reference that resolves to `null` as
    /// absent too: one naming a `null` object or no object at all, directly or
    /// through a chain of references (ISO 32000-1 §7.3.10). A present value
    /// reads as written, a reference unresolved.
    pub fn value<'d>(&self, dict: &'d [u8], key: &str) -> Option<&'d [u8]> {
        find_dict_value(dict, key).filter(|value| !self.resolves_to_null(value))
    }

    /// The id of the object a chain of references from `value` ends at, the
    /// first holding no reference. `None` where `value` is no reference, a
    /// link names no object, or the chain runs past [`MAX_REFERENCE_CHAIN`].
    fn referent(&self, value: &[u8]) -> Option<u32> {
        let (mut id, _) = parse_indirect_ref(value)?;
        for _ in 0..MAX_REFERENCE_CHAIN {
            match parse_indirect_ref(self.body(id)?) {
                Some((next, _)) => id = next,
                None => return Some(id),
            }
        }
        None
    }

    fn resolves_to_null(&self, value: &[u8]) -> bool {
        let mut reference = parse_indirect_ref(value);
        for _ in 0..MAX_REFERENCE_CHAIN {
            let Some((id, _)) = reference else {
                return false;
            };
            if !self.starts.contains_key(&id) {
                return true;
            }
            match self.body(id) {
                Some(b"null") => return true,
                Some(body) => reference = parse_indirect_ref(body),
                None => return false,
            }
        }
        false
    }

    /// `value`, or the value its reference names, followed along a chain of up
    /// to [`MAX_REFERENCE_CHAIN`] references (ISO 32000-1 §7.3.10). `None`
    /// where a reference names no object or one that never closes, or the chain
    /// runs past the bound.
    pub fn resolve<'d>(&self, value: &'d [u8]) -> Option<&'d [u8]>
    where
        'a: 'd,
    {
        if parse_indirect_ref(value).is_none() {
            return Some(value);
        }
        self.body(self.referent(value)?)
    }

    /// The first value object `id` holds past its header, or `None` when the
    /// object is absent or never closes.
    fn body(&self, id: u32) -> Option<&'a [u8]> {
        let (s, e) = self.object_bytes(id)?;
        let object = &self.pdf[s..e - b"endobj".len()];
        let header_end = object.windows(3).position(|w| w == b"obj")? + 3;
        let start = skip_ws_and_comments(object, header_end);
        Some(&object[start..read_value_end(object, start)?])
    }

    /// The generation in object `id`'s header, or `None` when the object is
    /// absent or its generation malformed.
    fn generation(&self, id: u32) -> Option<u16> {
        let start = *self.starts.get(&id)?;
        let header = &self.pdf[start..];
        let id_digits = header.iter().take_while(|b| b.is_ascii_digit()).count();
        let rest = skip_ws(&header[id_digits..]);
        let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        std::str::from_utf8(&rest[..n]).ok()?.parse().ok()
    }

    /// Reject overwriting a base object that lives at a non-zero generation: the
    /// update writer re-emits and references overwritten objects at generation 0,
    /// so the new `/Root` would point at generation 0 while the prior xref
    /// resolves the true one. An absent object is left to the caller's not-found
    /// path.
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
    let generation = ws_end(rest, digits);
    if (digits > 1 && rest[0] == b'0')
        || generation == digits
        || !is_obj_header_tail(&rest[generation..])
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
    let ws = after_gen.iter().take_while(|&&b| is_pdf_ws(b)).count();
    if ws == 0 {
        return false;
    }
    let after_ws = &after_gen[ws..];
    after_ws.starts_with(b"obj") && after_ws.get(3).is_none_or(|b| !b.is_ascii_alphanumeric())
}

/// Locate `/Key` in a dict's *inner* bytes (between its `<<` / `>>`) and return
/// its value's bytes. `None` when the dict has no such key or its value is
/// `null`: ISO 32000-1 §7.3.9 makes the two the same entry, so every read goes
/// through here.
pub(crate) fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let (_, entry) = find_dict_entry(dict_bytes, key)?;
    let value = &entry[skip_ws_and_comments(entry, 0)..];
    (value != b"null").then_some(value)
}

/// [`find_dict_value`] without the `null` filter: the key token as written and
/// the entry a rewrite replaces, whatever it holds.
fn find_dict_entry<'a>(dict_bytes: &'a [u8], key: &str) -> Option<(&'a [u8], &'a [u8])> {
    let key_marker = format!("/{key}");
    dict_entries(dict_bytes).find(|&(name, _)| is_name(name, key_marker.as_bytes()))
}

/// Whether the name token `token` is `name` (`/AcroForm`) once each `#xx`
/// escape in it is decoded, so `/Acro#46orm` is `/AcroForm` (ISO 32000-1
/// §7.3.5).
pub(crate) fn is_name(token: &[u8], name: &[u8]) -> bool {
    *decode_name(token) == *name
}

/// `token` with each `#xx` escape decoded. A `#` not followed by two hex
/// digits stands for itself.
fn decode_name(token: &[u8]) -> Cow<'_, [u8]> {
    if !token.contains(&b'#') {
        return Cow::Borrowed(token);
    }
    let hex = |c: u8| char::from(c).to_digit(16);
    let mut out = Vec::with_capacity(token.len());
    let mut i = 0;
    while i < token.len() {
        let digit = |at: usize| token.get(at).and_then(|&c| hex(c));
        match (token[i], digit(i + 1), digit(i + 2)) {
            (b'#', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// Each entry of a dict's inner bytes, in order, as its key Name (`/Pages`)
/// and its value's bytes from just after the key, so the key span is
/// recoverable by subtraction.
///
/// Entries alternate `key value key value …`, so the scan reads a key Name then
/// consumes its value wholesale via `read_value_end` (stepping over nested
/// `<<>>` / `[]` / `()` / `<>` as a unit). Only keys are matched, so a Name in
/// value position (`/Subtype /Producer`) is never mistaken for one. A
/// well-formed flat dict yields a Name key at each step; anything else (end of
/// input, or a stray token) ends the scan.
fn dict_entries(dict: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut i = 0;
    std::iter::from_fn(move || {
        i = skip_ws_and_comments(dict, i);
        if dict.get(i) != Some(&b'/') {
            return None;
        }
        let key_start = i;
        i += 1;
        while i < dict.len() && !is_pdf_delim(dict[i]) {
            i += 1;
        }
        let after_key = i;
        i = read_value_end(dict, skip_ws_and_comments(dict, after_key))?;
        Some((&dict[key_start..after_key], &dict[after_key..i]))
    })
}

/// `dict` when [`dict_entries`] reads it to its end and it names each key
/// once, with a value, as ISO 32000-1 §7.3.7 requires, else `Err` under
/// `code`. Readers part on which entry a repeated key holds and on what follows
/// a token where a key belongs, and a key with no value takes whatever follows
/// it as its value, an entry a rewrite appends included, so no read or rewrite
/// of such a dict is safe.
fn well_formed<'d>(dict: &'d [u8], code: &'static str, what: &str) -> Result<&'d [u8], PdfError> {
    let mut keys = HashSet::new();
    for (key, entry) in dict_entries(dict) {
        if !keys.insert(decode_name(key)) {
            return Err(err(
                code,
                format!(
                    "{what} dict names {} twice; a dictionary names each key once, so keep one \
                     entry",
                    String::from_utf8_lossy(key)
                ),
            ));
        }
        if skip_ws_and_comments(entry, 0) == entry.len() {
            return Err(err(
                code,
                format!(
                    "{what} dict names {} with no value",
                    String::from_utf8_lossy(key)
                ),
            ));
        }
    }
    if skip_ws_and_comments(dict, entries_end(dict)) < dict.len() {
        return Err(err(
            code,
            format!("{what} dict holds a token where a key belongs"),
        ));
    }
    Ok(dict)
}

/// The index just past the last entry [`dict_entries`] reads in `dict`.
fn entries_end(dict: &[u8]) -> usize {
    dict_entries(dict).last().map_or(0, |(_, entry)| {
        entry.as_ptr() as usize + entry.len() - dict.as_ptr() as usize
    })
}

/// The inner bytes of the dictionary `value` writes inline, [`well_formed`],
/// or `None` for any other value. A dictionary that does not close or that
/// [`well_formed`] refuses is `Err` under `code`, `what` naming it.
pub(crate) fn as_dict<'v>(
    value: &'v [u8],
    code: &'static str,
    what: &str,
) -> Result<Option<&'v [u8]>, PdfError> {
    if !value.starts_with(b"<<") {
        return Ok(None);
    }
    parse_dict(value, code, what).map(Some)
}

/// The inner bytes of the first dictionary in `bytes`, [`well_formed`].
fn parse_dict<'b>(bytes: &'b [u8], code: &'static str, what: &str) -> Result<&'b [u8], PdfError> {
    let dict = extract_outer_dict(bytes)
        .ok_or_else(|| err(code, format!("{what} dict not parseable")))?;
    well_formed(dict, code, what)
}

/// Each element of the array `value` writes inline, in order, from its first
/// significant byte. The read ends at a token no value starts with, and any
/// value but an array holds none.
pub(crate) fn array_elements(value: &[u8]) -> impl Iterator<Item = &[u8]> {
    let inner = value
        .strip_prefix(b"[")
        .and_then(|array| array.strip_suffix(b"]"))
        .unwrap_or_default();
    let mut i = 0;
    std::iter::from_fn(move || {
        let start = skip_ws_and_comments(inner, i);
        i = read_value_end(inner, start)?;
        (i > start).then(|| &inner[start..i])
    })
}

/// A flat dict's inner bytes with `key` (bare, `"Producer"`) holding
/// `new_value`: the entry replaced in place where the dict carries one, a `null`
/// one included, else appended, so a [`well_formed`] dict names `key` exactly
/// once after.
pub(crate) fn set_dict_value(dict: &[u8], key: &str, new_value: &[u8]) -> Vec<u8> {
    let Some((key_token, value)) = find_dict_entry(dict, key) else {
        let mut out = dict.to_vec();
        out.extend_from_slice(format!(" /{key} ").as_bytes());
        out.extend_from_slice(new_value);
        return out;
    };
    // The entry's own subslices locate the key span by pointer subtraction
    // rather than a re-scan, so a `key` token inside another value cannot match.
    let value_start = value.as_ptr() as usize - dict.as_ptr() as usize;
    let key_at = key_token.as_ptr() as usize - dict.as_ptr() as usize;
    let mut out = dict[..key_at].to_vec();
    out.extend_from_slice(format!("/{key} ").as_bytes());
    out.extend_from_slice(new_value);
    out.extend_from_slice(&dict[value_start + value.len()..]);
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
/// reading them as structure; the sharp case is a hex string's closing `>`
/// forming a `>>` against the enclosing dict's own. `None` when `b[i]` opens none
/// of them; either `<` of a `<<` opens none.
fn skip_string_or_comment(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'(' => Some(skip_pdf_string(b, i)),
        b'<' if b.get(i + 1) != Some(&b'<') && (i == 0 || b[i - 1] != b'<') => {
            Some(skip_pdf_hex_string(b, i))
        }
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
/// followed by CRLF or LF per ISO 32000 §7.3.8 — the index just past its
/// `endstream`, so raw stream data is never read as structure. `None` also when
/// no `endstream` follows, so a truncated stream is scanned as ordinary bytes
/// rather than swallowing the rest of the file.
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
            let mut j = skip_ws_and_comments(b, num_end);
            let n2_start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > n2_start {
                j = skip_ws_and_comments(b, j);
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

/// White-space per ISO 32000-1 §7.2.2.
fn is_pdf_ws(c: u8) -> bool {
    matches!(c, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// Whether `c` ends a token: white-space or an ISO 32000-1 §7.2.2 delimiter.
fn is_pdf_delim(c: u8) -> bool {
    is_pdf_ws(c)
        || matches!(
            c,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

pub(crate) fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
    let s = &s[skip_ws_and_comments(s, 0)..];
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let id: u32 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = &s[skip_ws_and_comments(s, i)..];
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let generation: u16 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = &s[skip_ws_and_comments(s, i)..];
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
pub(crate) fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
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
    while i < b.len() && is_pdf_ws(b[i]) {
        i += 1;
    }
    i
}

fn skip_ws(s: &[u8]) -> &[u8] {
    &s[ws_end(s, 0)..]
}

/// Open the base's trailer: the xref offset, the trailer dict, and the catalog
/// (`/Root`) object id, after refusing an xref stream and a hybrid file.
/// `code` carries the caller's error code for a missing or malformed `/Root`.
pub(crate) fn open_trailer<'a>(
    pdf: &'a [u8],
    code: &'static str,
) -> Result<(usize, &'a [u8], u32), PdfError> {
    let xref_offset = find_startxref(pdf)?;
    assert_traditional_xref(pdf, xref_offset)?;
    let trailer = find_trailer_dict(pdf, xref_offset)?;
    assert_no_object_streams(pdf, trailer)?;
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(code, "/Root missing or malformed in trailer"))?;
    Ok((xref_offset, trailer, catalog_id))
}

/// A page object and the `/Pages` nodes it descends from, nearest ancestor
/// first: the chain an inheritable attribute resolves along.
#[derive(Debug)]
pub(crate) struct Page {
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
                parse(idx.value(dict, key)?)
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
        if find_dict_value(dict, "Type").is_some_and(|typ| is_name(typ, b"/Pages")) {
            let kids = idx
                .value(dict, "Kids")
                .and_then(|kids| idx.resolve(kids))
                .ok_or_else(|| err(CODE_PARSE, "/Pages node missing /Kids"))?;
            if !(kids.starts_with(b"[") && kids.ends_with(b"]")) {
                return Err(err(CODE_PARSE, format!("page node {node_id} /Kids is not an array")));
            }
            let mut kid_ancestors = Vec::with_capacity(ancestors.len() + 1);
            kid_ancestors.push(node_id);
            kid_ancestors.extend_from_slice(&ancestors);
            let mut kid_ids = array_elements(kids)
                .map(|kid| {
                    parse_indirect_ref(kid).map(|(id, _)| id).ok_or_else(|| {
                        err(
                            CODE_PARSE,
                            format!("page node {node_id} /Kids holds a non-reference"),
                        )
                    })
                })
                .collect::<Result<Vec<u32>, _>>()?;
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

/// Reject a page whose `/Rotate`, its own or inherited, is non-zero: the stamp
/// writes geometry in unrotated user space and does not compensate, so every
/// widget would display away from its box. The first
/// *present* value binds, and a non-integer one errors rather than falling to the
/// default zero.
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

/// Parse a 4-number array (`[x0 y0 x1 y1]`) such as `/MediaBox`.
fn parse_rect_array(bytes: &[u8]) -> Option<[f32; 4]> {
    let trimmed = bytes.trim_ascii();
    let inner = trimmed.strip_prefix(b"[")?.strip_suffix(b"]")?;
    let mut nums = [0.0f32; 4];
    let mut count = 0;
    let mut i = skip_ws_and_comments(inner, 0);
    while i < inner.len() {
        let end = (i..inner.len())
            .find(|&j| is_pdf_delim(inner[j]))
            .unwrap_or(inner.len());
        if count >= 4 {
            return None;
        }
        nums[count] = std::str::from_utf8(&inner[i..end])
            .ok()?
            .parse()
            .ok()
            .filter(|f: &f32| f.is_finite())?;
        count += 1;
        i = skip_ws_and_comments(inner, end);
    }
    (count == 4).then_some(nums)
}

/// Normalize a page box so `(x0, y0)` is lower-left and `(x1, y1)`
/// upper-right, whichever corners the array listed.
fn normalize_rect(mb: [f32; 4]) -> [f32; 4] {
    [
        mb[0].min(mb[2]),
        mb[1].min(mb[3]),
        mb[0].max(mb[2]),
        mb[1].max(mb[3]),
    ]
}

/// The overlap of two normalized rects. Disjoint inputs give an inverted rect,
/// which [`canvas_box`] rejects on extent.
fn intersect_rect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// The canvas box of every page, in document order.
pub(crate) fn page_canvas_boxes(pdf: &[u8]) -> Result<Vec<[f32; 4]>, PdfError> {
    let (_, _, catalog_id) = open_trailer(pdf, CODE_PARSE)?;
    canvas_boxes_of(&ObjectIndex::new(pdf), catalog_id)
}

fn canvas_boxes_of(idx: &ObjectIndex, catalog_id: u32) -> Result<Vec<[f32; 4]>, PdfError> {
    walk_page_tree(idx, catalog_id)?
        .iter()
        .map(|page| canvas_box(idx, page))
        .collect()
}

/// `/CropBox` ∩ `/MediaBox`, each resolved along the page's ancestor chain and
/// normalized; a page reaching no `/CropBox` takes its `/MediaBox`.
fn canvas_box(idx: &ObjectIndex, page: &Page) -> Result<[f32; 4], PdfError> {
    let media = page_box(idx, page, "MediaBox")?.ok_or_else(|| {
        err(
            CODE_PARSE,
            format!("page {} has no resolvable /MediaBox", page.id),
        )
    })?;
    let canvas = match page_box(idx, page, "CropBox")? {
        Some(crop) => intersect_rect(media, crop),
        None => media,
    };
    let (w, h) = (canvas[2] - canvas[0], canvas[3] - canvas[1]);
    // hayro draws a page under a point per side at a clamped or A4 size
    // (`hayro_syntax::page::Page::base_dimensions`), so its raster would not
    // match the extent reported here.
    if w < 1.0 || h < 1.0 {
        return Err(err(
            "pdf::degenerate_page_box",
            format!(
                "page {} has a {w} × {h} pt canvas box (/CropBox ∩ /MediaBox); \
                 a page under a point per side has no renderable canvas",
                page.id
            ),
        ));
    }
    Ok(canvas)
}

/// The page box `key`, normalized, from the first ancestor-chain node carrying
/// one. A value that is not a direct `[x0 y0 x1 y1]` array errors rather than
/// counting as absent: a renderer resolving it would draw a different box.
fn page_box(idx: &ObjectIndex, page: &Page, key: &str) -> Result<Option<[f32; 4]>, PdfError> {
    let Some(raw) = page.inherited_attribute(idx, key, |raw| Some(raw.trim_ascii().to_vec()))
    else {
        return Ok(None);
    };
    parse_rect_array(&raw)
        .map(normalize_rect)
        .map(Some)
        .ok_or_else(|| {
            err(
                CODE_PARSE,
                format!(
                    "page {} has /{key} {}, which is not an [x0 y0 x1 y1] array of numbers",
                    page.id,
                    String::from_utf8_lossy(&raw)
                ),
            )
        })
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
    fn a_null_value_reads_as_absent() {
        for dict in [
            &b" /AcroForm null /Pages 2 0 R "[..],
            b" /AcroForm\nnull/Pages 2 0 R",
            b" /AcroForm %stripped\n null /Pages 2 0 R ",
        ] {
            assert_eq!(
                find_dict_value(dict, "AcroForm"),
                None,
                "{:?}",
                String::from_utf8_lossy(dict)
            );
            assert_eq!(
                find_dict_value(dict, "Pages").map(<[u8]>::trim_ascii),
                Some(&b"2 0 R"[..])
            );
        }
        assert_eq!(
            find_dict_value(b" /Title (null) /Kind /null ", "Title").map(<[u8]>::trim_ascii),
            Some(&b"(null)"[..]),
            "a string spelling null is a value"
        );
        assert!(
            find_dict_value(b" /Kind /null ", "Kind").is_some(),
            "so is the name /null"
        );
    }

    #[test]
    fn a_reference_resolving_to_null_reads_as_absent() {
        let pdf = b"%PDF\n7 0 obj\nnull\nendobj\n8 0 obj 7 0 R endobj\n9 0 obj 9 0 R endobj\n\
                    10 0 obj\n<< /Fields [] >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        for (value, present) in [
            ("7 0 R", false),
            ("99 0 R", false),
            ("8 0 R", false),
            ("9 0 R", true),
            ("10 0 R", true),
        ] {
            let dict = format!("/AcroForm {value} /Pages 2 0 R");
            assert_eq!(
                idx.value(dict.as_bytes(), "AcroForm"),
                present.then_some(value.as_bytes()),
                "{dict}"
            );
        }
    }

    #[test]
    fn an_array_reads_each_element_through_its_references() {
        let pdf = b"%PDF\n7 0 obj\n<< /Subtype /Widget >>\nendobj\n8 0 obj 7 0 R endobj\n\
                    9 0 obj 9 0 R endobj\n";
        let idx = ObjectIndex::new(pdf);
        let array = b"[7 0 R 8 0 R 9 0 R 99 0 R (a]b) %c]\n [1 2] << /K [3] >> 12 /N) 5]";
        let widget = Some(&b"<< /Subtype /Widget >>"[..]);
        assert_eq!(
            array_elements(array)
                .map(|element| idx.resolve(element))
                .collect::<Vec<_>>(),
            [
                widget,
                widget,
                None,
                None,
                Some(b"(a]b)"),
                Some(b"[1 2]"),
                Some(b"<< /K [3] >>"),
                Some(b"12"),
                Some(b"/N"),
            ]
        );
    }

    #[test]
    fn a_token_ends_at_nul_and_at_every_delimiter() {
        for sep in [" ", "\0", "%c\n"] {
            for (value, want) in [
                ("null", None),
                ("/Pages", Some("/Pages")),
                ("2\x000\x00R", Some("2\x000\x00R")),
                ("612", Some("612")),
            ] {
                for end in ["\0", "%c\n", "{", "}"] {
                    let dict = format!("/A{sep}{value}{end}");
                    assert_eq!(
                        find_dict_value(dict.as_bytes(), "A"),
                        want.map(str::as_bytes),
                        "{dict:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_name_reads_through_its_escapes() {
        let dict = b"/Acro#46orm 7 0 R /Ty#70e /P#61ges /X #23";
        assert_eq!(find_dict_value(dict, "AcroForm"), Some(&b"7 0 R"[..]));
        assert!(is_name(find_dict_value(dict, "Type").unwrap(), b"/Pages"));
        assert!(is_name(b"/A#2", b"/A#2") && !is_name(b"/A#zz", b"/A"));
        assert_eq!(
            set_dict_value(b"/Acro#46orm null /X 1", "AcroForm", b"9 0 R"),
            b"/AcroForm 9 0 R /X 1"
        );
        let pdf = b"%PDF\n1 0 obj\n<< /AcroForm 1 /Acro#46orm 2 >>\nendobj\n";
        let e = ObjectIndex::new(pdf)
            .dict(1, CODE_PARSE, "catalog")
            .expect_err("a key named twice in two spellings");
        assert_eq!(e.code, CODE_PARSE);
    }

    #[test]
    fn a_comment_stands_for_white_space_inside_a_reference() {
        assert_eq!(parse_indirect_ref(b"%a\n7 %b\r0%c\nR"), Some((7, 0)));
        assert_eq!(
            find_dict_value(b"/Pages 2 %c\n0 R /X 1", "Pages"),
            Some(&b"2 %c\n0 R"[..])
        );
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 %c\n0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 %c\n0 R %c\n4 0\n%c\nR] /Count 2 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
                    4 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let pages = walk_page_tree(&ObjectIndex::new(pdf), 1).expect("page tree walks");
        assert_eq!(pages.iter().map(|page| page.id).collect::<Vec<_>>(), [3, 4]);
    }

    #[test]
    fn a_dict_holding_a_stray_token_or_a_key_with_no_value_is_refused() {
        for inner in [
            "/Lang /en{US} /AcroForm 7 0 R",
            "/AcroForm null} /Pages 2 0 R",
            "/A 1 2 /AcroForm 7 0 R",
            "(x) /A 1",
            "/Pages 2 0 R /Lang",
            "/Pages 2 0 R /Lang %c\n",
        ] {
            let pdf = format!("%PDF\n1 0 obj\n<< {inner} >>\nendobj\n");
            let e = ObjectIndex::new(pdf.as_bytes())
                .dict(1, CODE_PARSE, "catalog")
                .expect_err(inner);
            assert_eq!(e.code, CODE_PARSE);
        }
    }

    #[test]
    fn set_dict_value_replaces_the_one_entry_or_appends_it() {
        for (dict, want) in [
            (
                &b"/Title (Hi) /Producer (Old) /Creator (X)"[..],
                &b"/Title (Hi) /Producer (New) /Creator (X)"[..],
            ),
            (
                b"/Title (Hi) /Producer null /Creator (X)",
                b"/Title (Hi) /Producer (New) /Creator (X)",
            ),
            (b"/Title (Hi)", b"/Title (Hi) /Producer (New)"),
            // A `/Producer` Name in value position is not the key.
            (
                b"/Title (Hi) /Marker /Producer",
                b"/Title (Hi) /Marker /Producer /Producer (New)",
            ),
        ] {
            assert_eq!(
                String::from_utf8_lossy(&set_dict_value(dict, "Producer", b"(New)")),
                String::from_utf8_lossy(want)
            );
        }
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
    fn rect_array_reads_its_numbers_through_white_space_and_comments() {
        for bytes in [&b"[0\x000\t612\x0c792]"[..], b"[%c\n0 0%c\n612 792%c\n]"] {
            assert_eq!(parse_rect_array(bytes), Some([0.0, 0.0, 612.0, 792.0]));
        }
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
    fn an_object_header_reads_through_any_white_space() {
        let pdf = b"%PDF\n1\t0\x0cobj << /A 1 >> endobj\x002  3\r\nobj << /B 2 >> endobj\
                    \x0c3\x000\x00obj<</C 3>>endobj\n";
        let idx = ObjectIndex::new(pdf);
        for (id, generation, body) in [(1, 0, &b"/A 1"[..]), (2, 3, b"/B 2"), (3, 0, b"/C 3")] {
            assert_eq!(idx.generation(id), Some(generation));
            assert_eq!(idx.dict(id, CODE_PARSE, "obj").unwrap().trim_ascii(), body);
        }
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
    fn a_dict_opening_hides_no_endobj_or_reference() {
        let pdf = b"%PDF\n3 0 obj\n<< /T (x>endobj) /X 41 0 R >>\nendobj\n";
        let idx = ObjectIndex::new(pdf);
        let (s, e) = idx.object_bytes(3).expect("found object 3");
        assert!(pdf[s..e].ends_with(b">>\nendobj"));
        assert_eq!(idx.unnamed_from(), 42);
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
    fn page_boxes_resolve_to_the_nearest_ancestor_carrying_them() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [5 0 R 4 0 R] /Count 2 \
                    /MediaBox [0 0 612 792] /CropBox [0 0 612 792] >>\nendobj\n\
                    5 0 obj\n<< /Type /Pages /Parent 2 0 R /Kids [3 0 R] /Count 1 \
                    /MediaBox [0 0 200 400] >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 5 0 R /CropBox [10 20 150 300] >>\nendobj\n\
                    4 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let boxes = canvas_boxes_of(&ObjectIndex::new(pdf), 1).expect("canvas boxes resolve");
        assert_eq!(boxes, [[10.0, 20.0, 150.0, 300.0], [0.0, 0.0, 612.0, 792.0]]);
    }

    #[test]
    fn the_canvas_box_is_the_crop_box_clipped_to_the_media_box() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                    /CropBox [-40 100 500 900] >>\nendobj\n";
        let boxes = canvas_boxes_of(&ObjectIndex::new(pdf), 1).expect("canvas boxes resolve");
        assert_eq!(boxes, [[0.0, 100.0, 500.0, 792.0]]);
    }

    #[test]
    fn a_canvas_box_under_a_point_per_side_is_rejected() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                    /CropBox [700 0 900 792] >>\nendobj\n";
        let e = canvas_boxes_of(&ObjectIndex::new(pdf), 1).expect_err("disjoint boxes rejected");
        assert_eq!(e.code, "pdf::degenerate_page_box");
    }

    #[test]
    fn a_page_box_that_is_not_a_number_array_is_rejected() {
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 \
                    /CropBox [0 0 100 100] >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                    /CropBox 9 0 R >>\nendobj\n\
                    9 0 obj\n[0 0 50 50]\nendobj\n";
        let e = canvas_boxes_of(&ObjectIndex::new(pdf), 1).expect_err("indirect /CropBox rejected");
        assert_eq!(e.code, CODE_PARSE);
        assert!(e.message.contains("/CropBox"), "{}", e.message);
    }
}
