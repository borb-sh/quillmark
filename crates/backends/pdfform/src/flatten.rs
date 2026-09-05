//! Draw field values as PDF content stream operators instead of AcroForm
//! widgets, so they are visible in non-interactive rasterizers (pdfium,
//! Ghostscript) that never synthesize `/NeedAppearances` appearances.
//!
//! Drawing directly commits to a byte encoding: text is transcoded to WinAnsi
//! and shown with a `WinAnsiEncoding` Helvetica, clipped to its field box. The
//! stream is appended last to the page `/Contents` and positions text with
//! absolute `Td`, so it assumes the identity CTM of page default user space —
//! true of any background with balanced `q`/`Q` and no dangling `cm`. Colour and
//! text state carry no such guarantee — a `/Contents` array is one stream, and
//! neither `BT` nor an unpaired `0.9 g` or `3 Tr` is undone — so each drawn
//! value opens by asserting the defaults the stamped `/DA` writes. Backs the
//! SVG/PNG raster outputs only; the AcroForm PDF deliverable is stamped.

use quillmark_pdf::{
    reader::{
        extract_outer_dict, find_dict_value, parse_indirect_ref, splice_dict_value, ObjectIndex,
        Page,
    },
    writer::{
        alloc_id, append_refs_to_array_key, content_stream_object, dict_object, pdf_escape,
        type1_font_object, winansi_encode, OnNonArray,
    },
    FieldSpec, FieldType, PdfError, PdfUpdate, CHECKBOX_ON_STATE,
};

use crate::typography;

const CODE_PARSE: &str = "pdfform::flatten_parse";

const STATE_RESET: &[u8] = b"0 g 0 Tr 0 Tc 0 Tw 100 Tz 0 Ts\n";

/// Flatten `fields` onto `base` by drawing values as content stream operators.
/// Backs raster output only, so it stamps no `/Info /Producer`.
pub fn flatten(base: Vec<u8>, fields: &[FieldSpec]) -> Result<Vec<u8>, PdfError> {
    // Nothing to draw is the base itself: an update carrying only the two font
    // objects would be a revision no page references.
    let drawable: Vec<&FieldSpec> = fields.iter().filter(|s| has_drawable_value(s)).collect();
    if drawable.is_empty() {
        return Ok(base);
    }

    for spec in &drawable {
        spec.assert_finite_rect()?;
    }

    let pdf = base;
    let idx = ObjectIndex::new(&pdf);
    let mut up = PdfUpdate::begin(&idx, None)?;

    let pages = up.resolve_pages(&idx, fields)?;
    let page_count = pages.len();

    // Helvetica and ZapfDingbats are among the 14 standard PDF fonts every
    // conforming reader provides, so neither is embedded.
    let helv_id = alloc_id(&mut up.next_id)?;
    let zadb_id = alloc_id(&mut up.next_id)?;
    up.objects.push(type1_font_object(
        helv_id,
        typography::TEXT_FONT,
        Some(b"WinAnsiEncoding"),
    )?);
    up.objects
        .push(type1_font_object(zadb_id, typography::CHECK_FONT, None)?);

    let mut fields_by_page: Vec<Vec<&FieldSpec>> = vec![Vec::new(); page_count];
    for spec in drawable {
        fields_by_page[spec.page].push(spec);
    }

    for (page_idx, page_fields) in fields_by_page.iter().enumerate() {
        if page_fields.is_empty() {
            continue;
        }

        let page = &pages[page_idx];
        let page_obj_id = page.id;
        let what = format!("page object {page_obj_id}");
        let pg_dict = idx.dict(page_obj_id, CODE_PARSE, &what)?;

        let resources = resolve_resources(&idx, page)?;
        let font_inner: &[u8] = match &resources {
            Some(res) => font_dict(res)?.map_or(&[], |f| f.inner),
            None => &[],
        };
        let names = FontNames::free_in(font_inner);

        let stream_id = alloc_id(&mut up.next_id)?;
        up.objects.push(content_stream_object(
            stream_id,
            &build_content_stream(page_fields, &names),
        )?);

        let new_pg = rewrite_page_for_flatten(
            &idx,
            pg_dict,
            resources.as_deref(),
            &[(&names.text, helv_id), (&names.check, zadb_id)],
            stream_id,
        )?;
        up.objects.push(dict_object(page_obj_id, &new_pg));
    }

    up.finish(pdf)
}

fn has_drawable_value(spec: &FieldSpec) -> bool {
    match &spec.field_type {
        FieldType::Signature => false,
        FieldType::Checkbox => spec.value.as_deref() == Some(CHECKBOX_ON_STATE),
        _ => spec.value.is_some(),
    }
}

/// The `/Font` resource names one page's flatten stream selects. Chosen per page
/// because they must be free in that page's own `/Font` dict.
struct FontNames {
    text: String,
    check: String,
}

impl FontNames {
    fn free_in(font_inner: &[u8]) -> Self {
        Self {
            text: free_font_name(font_inner, typography::TEXT_FONT_RESOURCE),
            check: free_font_name(font_inner, typography::CHECK_FONT_RESOURCE),
        }
    }
}

/// `preferred` when it is unbound in `font_inner`, else the first free
/// `<preferred><n>`. Binding a name the background's own stream selects would
/// rebind it there too: a dict carrying the key twice resolves last-wins.
fn free_font_name(font_inner: &[u8], preferred: &str) -> String {
    if find_dict_value(font_inner, preferred).is_none() {
        return preferred.to_string();
    }
    let mut n = 2;
    loop {
        let name = format!("{preferred}{n}");
        if find_dict_value(font_inner, &name).is_none() {
            return name;
        }
        n += 1;
    }
}

fn build_content_stream(fields: &[&FieldSpec], names: &FontNames) -> Vec<u8> {
    let mut out = Vec::new();
    for spec in fields {
        let [x0, y0, x1, y1] = spec.rect;
        let w = x1 - x0;
        let h = y1 - y0;
        match &spec.field_type {
            FieldType::Signature => {}
            FieldType::Checkbox => {
                let size = typography::check_size(h);
                // ZapfDingbats check glyphs are roughly square; centre in the box.
                let x_pos = x0 + (w - size * typography::CHECK_GLYPH_WIDTH_FACTOR) * 0.5;
                let y_pos = y0 + (h - size) * 0.5;
                write_check_char(&mut out, &names.check, x_pos, y_pos, size);
            }
            FieldType::Text { .. } => {
                if let Some(value) = &spec.value {
                    let size = typography::value_size(h);
                    let x_pos = x0 + typography::TEXT_INSET;
                    let y_top = y1 - size - typography::TEXT_TOP_INSET;
                    let lines: Vec<&str> = value.lines().collect();
                    write_text_block(&mut out, &names.text, &lines, x_pos, y_top, size, spec.rect);
                }
            }
            FieldType::Choice { .. } => {
                if let Some(value) = &spec.value {
                    let size = typography::value_size(h);
                    let x_pos = x0 + typography::TEXT_INSET;
                    let y_pos = y0 + (h - size) * 0.5;
                    write_text_block(
                        &mut out,
                        &names.text,
                        &[value.as_str()],
                        x_pos,
                        y_pos,
                        size,
                        spec.rect,
                    );
                }
            }
        }
    }
    out
}

/// Draw `lines` in the page's text font, clipped to `clip` so an over-long value
/// cannot paint over neighbouring content. Bytes are transcoded to WinAnsi to
/// match the font's `/Encoding /WinAnsiEncoding`.
fn write_text_block(
    out: &mut Vec<u8>,
    font: &str,
    lines: &[&str],
    x: f32,
    y: f32,
    size: f32,
    clip: [f32; 4],
) {
    if lines.is_empty() {
        return;
    }
    let line_h = size * typography::LINE_SPACING;
    let [cx0, cy0, cx1, cy1] = clip;
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(STATE_RESET);
    // `x y w h re W n`
    push_f32(out, cx0);
    out.push(b' ');
    push_f32(out, cy0);
    out.push(b' ');
    push_f32(out, cx1 - cx0);
    out.push(b' ');
    push_f32(out, cy1 - cy0);
    out.extend_from_slice(b" re W n\n");
    out.extend_from_slice(format!("BT\n/{font} ").as_bytes());
    push_f32(out, size);
    out.extend_from_slice(b" Tf\n");
    push_f32(out, x);
    out.push(b' ');
    push_f32(out, y);
    out.extend_from_slice(b" Td\n");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"0 ");
            push_f32(out, -line_h);
            out.extend_from_slice(b" Td\n");
        }
        out.push(b'(');
        pdf_escape(out, &winansi_encode(line));
        out.extend_from_slice(b") Tj\n");
    }
    out.extend_from_slice(b"ET\nQ\n");
}

/// Draw ZapfDingbats glyph 0x34 (`'4'`), the filled check mark — the same glyph
/// the AcroForm stamp path declares via `/MK /CA (4)`.
fn write_check_char(out: &mut Vec<u8>, font: &str, x: f32, y: f32, size: f32) {
    out.extend_from_slice(b"q\n");
    out.extend_from_slice(STATE_RESET);
    out.extend_from_slice(format!("BT\n/{font} ").as_bytes());
    push_f32(out, size);
    out.extend_from_slice(b" Tf\n");
    push_f32(out, x);
    out.push(b' ');
    push_f32(out, y);
    out.extend_from_slice(b" Td\n(4) Tj\nET\nQ\n");
}

fn rewrite_page_for_flatten(
    idx: &ObjectIndex,
    pg_dict: &[u8],
    resources: Option<&[u8]>,
    fonts: &[(&str, u32)],
    stream_id: u32,
) -> Result<Vec<u8>, PdfError> {
    let with_stream = append_content_stream(idx, pg_dict, stream_id)?;
    add_font_resources(&with_stream, resources, fonts)
}

/// Append the flatten stream to the page `/Contents`. A `/Contents` naming an
/// *array* object expands to that array's elements: wrapping the reference would
/// leave an array as an element of the `/Contents` array, which is not a content
/// stream. A reference to a stream is legitimate and wraps.
fn append_content_stream(
    idx: &ObjectIndex,
    pg_dict: &[u8],
    stream_id: u32,
) -> Result<Vec<u8>, PdfError> {
    if let Some(value) = find_dict_value(pg_dict, "Contents")
        && let Some(inner) = referenced_array_inner(idx, value)
    {
        let merged = format!("[{} {stream_id} 0 R]", String::from_utf8_lossy(inner).trim());
        return Ok(splice_dict_value(
            pg_dict,
            b"/Contents",
            value,
            merged.as_bytes(),
        ));
    }
    append_refs_to_array_key(
        pg_dict,
        "Contents",
        &[stream_id],
        CODE_PARSE,
        OnNonArray::Wrap,
    )
}

/// The elements of the array object `value` references, or `None` when `value`
/// is not a reference or names an object that is not an array.
fn referenced_array_inner<'a>(idx: &ObjectIndex<'a>, value: &[u8]) -> Option<&'a [u8]> {
    let (id, _) = parse_indirect_ref(value.trim_ascii())?;
    let (start, end) = idx.object_bytes(id)?;
    let body = &idx.bytes()[start..end];
    // The `<id> <gen> obj` header holds only digits, so the first `obj` is it.
    let after_header = body.windows(3).position(|w| w == b"obj")? + 3;
    let inner = body[after_header..].trim_ascii().strip_prefix(b"[")?;
    let close = inner.iter().rposition(|&b| b == b']')?;
    Some(&inner[..close])
}

/// The page's effective `/Resources` inner bytes, with an indirect `/Resources`
/// or `/Font` replaced by an inline copy of the referenced dict. `/Resources` is
/// inheritable, so a page carrying none draws with the nearest ancestor's.
/// `None` when no node on the chain carries one; the first present value is
/// taken, so one that fails to parse is an error rather than a step past.
fn resolve_resources(idx: &ObjectIndex, page: &Page) -> Result<Option<Vec<u8>>, PdfError> {
    page.inherited_attribute(idx, "Resources", |value| {
        Some(inline_dict(idx, value, "page /Resources"))
    })
    .transpose()?
    .map(|inner| inline_font_dict(idx, inner))
    .transpose()
}

/// A dict-valued entry as inline bytes: an inline dict is copied as is, an
/// indirect reference is dereferenced through `idx`. References nested in the
/// copy stay valid — they resolve against the same file.
fn inline_dict(idx: &ObjectIndex, value: &[u8], what: &str) -> Result<Vec<u8>, PdfError> {
    let trimmed = value.trim_ascii();
    if trimmed.starts_with(b"<<") {
        return extract_outer_dict(trimmed)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| PdfError::new(CODE_PARSE, format!("{what} dict not parseable")));
    }
    let (id, _) = parse_indirect_ref(trimmed).ok_or_else(|| {
        PdfError::new(
            CODE_PARSE,
            format!("{what} is neither a dict nor an indirect reference"),
        )
    })?;
    Ok(idx
        .dict(id, CODE_PARSE, &format!("{what} object {id}"))?
        .to_vec())
}

/// `res_inner` with an indirect `/Font` replaced by an inline copy, so the font
/// dict can be read by name and extended.
fn inline_font_dict(idx: &ObjectIndex, res_inner: Vec<u8>) -> Result<Vec<u8>, PdfError> {
    let inlined = match find_dict_value(&res_inner, "Font") {
        Some(font_val) if !font_val.trim_ascii().starts_with(b"<<") => {
            let mut value = b"<< ".to_vec();
            value.extend_from_slice(&inline_dict(idx, font_val, "page /Resources /Font")?);
            value.extend_from_slice(b" >>");
            Some(splice_dict_value(&res_inner, b"/Font", font_val, &value))
        }
        _ => None,
    };
    Ok(inlined.unwrap_or(res_inner))
}

/// The `/Font` subdictionary of a resolved `/Resources` dict, which
/// [`resolve_resources`] has already inlined.
struct FontDict<'a> {
    /// The value span, locating the entry for a splice.
    value: &'a [u8],
    inner: &'a [u8],
}

fn font_dict(res_inner: &[u8]) -> Result<Option<FontDict<'_>>, PdfError> {
    let Some(value) = find_dict_value(res_inner, "Font") else {
        return Ok(None);
    };
    let inner = extract_outer_dict(value)
        .ok_or_else(|| PdfError::new(CODE_PARSE, "page /Resources /Font dict not parseable"))?;
    Ok(Some(FontDict { value, inner }))
}

/// Write `resources`, extended with `/<name> <font_id> 0 R` for each of `fonts`,
/// as the page's own inline `/Resources`, creating intermediate dicts as needed.
/// `resources` is the page's *effective* dict, possibly inherited: a page's own
/// `/Resources` shadows the inherited one outright, so the copy has to carry the
/// ancestor's entries forward or the background loses its names.
fn add_font_resources(
    pg_dict: &[u8],
    resources: Option<&[u8]>,
    fonts: &[(&str, u32)],
) -> Result<Vec<u8>, PdfError> {
    let entries = fonts
        .iter()
        .map(|(name, font_id)| format!("/{name} {font_id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");

    let new_res_inner: Vec<u8> = match resources {
        None => format!("/Font << {entries} >>").into_bytes(),
        Some(res_inner) => match font_dict(res_inner)? {
            None => {
                let mut out = res_inner.to_vec();
                out.extend_from_slice(format!(" /Font << {entries} >>").as_bytes());
                out
            }
            Some(font) => {
                let mut new_font_val = b"<< ".to_vec();
                new_font_val.extend_from_slice(font.inner);
                new_font_val.extend_from_slice(format!(" {entries} >>").as_bytes());
                splice_dict_value(res_inner, b"/Font", font.value, &new_font_val)
            }
        },
    };

    let mut new_res_val = b"<< ".to_vec();
    new_res_val.extend_from_slice(&new_res_inner);
    new_res_val.extend_from_slice(b" >>");

    Ok(match find_dict_value(pg_dict, "Resources") {
        Some(res_val) => splice_dict_value(pg_dict, b"/Resources", res_val, &new_res_val),
        None => {
            let mut out = pg_dict.to_vec();
            out.extend_from_slice(b" /Resources ");
            out.extend_from_slice(&new_res_val);
            out
        }
    })
}

/// Append `v` as a compact `%.2f` float, stripping trailing zeros and dot.
fn push_f32(out: &mut Vec<u8>, v: f32) {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Document as PdfDoc;
    use pdf_writer::{Name, Pdf, Rect, Ref};
    use quillmark_pdf::CHECKBOX_ON_STATE;

    /// Single-page US-Letter background, no AcroForm and no annots.
    const BASE: &[u8] =
        include_bytes!("../../../fixtures/resources/quills/sample_form/0.1.0/form.pdf");

    fn text_field(name: &str, value: &str) -> FieldSpec {
        let mut spec = FieldSpec::new(
            name.to_string(),
            0,
            [72.0, 700.0, 300.0, 720.0],
            FieldType::Text { multiline: false },
        );
        spec.schema_field = Some(name.to_string());
        spec.value = Some(value.to_string());
        spec
    }

    fn checkbox_field(name: &str, checked: bool) -> FieldSpec {
        let mut spec = FieldSpec::new(
            name.to_string(),
            0,
            [72.0, 660.0, 90.0, 678.0],
            FieldType::Checkbox,
        );
        spec.schema_field = Some(name.to_string());
        spec.value = checked.then(|| CHECKBOX_ON_STATE.to_string());
        spec
    }

    fn flatten_ok(fields: &[FieldSpec]) -> Vec<u8> {
        flatten(BASE.to_vec(), fields).expect("flatten succeeds")
    }

    fn window_pos(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn contains_window(haystack: &[u8], needle: &[u8]) -> bool {
        window_pos(haystack, needle).is_some()
    }

    fn default_names() -> FontNames {
        FontNames::free_in(b"")
    }

    /// The `/Resources` dict of the flattened PDF's only page.
    fn page_resources(pdf: &[u8]) -> lopdf::Dictionary {
        let doc = PdfDoc::load_mem(pdf).expect("lopdf reparse: structurally valid");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        doc.get_dictionary(page_id)
            .expect("page dict")
            .get(b"Resources")
            .expect("page carries its own /Resources")
            .as_dict()
            .expect("/Resources is a dict")
            .clone()
    }

    /// One US-Letter page whose object ids are 1 catalog, 2 `/Pages`, 3 page,
    /// 4 a background content stream, 5 the font it selects. `on_pages` and
    /// `on_page` write the entries under test onto the tree node and the page;
    /// `extra` adds further objects.
    fn build_base(
        on_pages: impl FnOnce(&mut pdf_writer::writers::Pages),
        on_page: impl FnOnce(&mut pdf_writer::writers::Page),
        extra: impl FnOnce(&mut Pdf),
    ) -> Vec<u8> {
        let letter = Rect::new(0.0, 0.0, 612.0, 792.0);
        let mut pdf = Pdf::new();
        pdf.catalog(Ref::new(1)).pages(Ref::new(2));
        {
            let mut pages = pdf.pages(Ref::new(2));
            pages.kids([Ref::new(3)]).count(1).media_box(letter);
            on_pages(&mut pages);
        }
        {
            let mut page = pdf.page(Ref::new(3));
            page.parent(Ref::new(2)).media_box(letter);
            on_page(&mut page);
        }
        pdf.stream(Ref::new(4), b"BT /F1 12 Tf 72 700 Td (background) Tj ET");
        pdf.indirect(Ref::new(5))
            .dict()
            .pair(Name(b"Type"), Name(b"Font"))
            .pair(Name(b"Subtype"), Name(b"Type1"))
            .pair(Name(b"BaseFont"), Name(b"Helvetica"));
        extra(&mut pdf);
        pdf.finish()
    }

    #[test]
    fn resources_inherited_from_the_page_tree_reach_the_flattened_page() {
        let base = build_base(
            |pages| {
                let mut res = pages.insert(Name(b"Resources")).dict();
                res.insert(Name(b"ProcSet"))
                    .array()
                    .items([Name(b"PDF"), Name(b"Text")]);
                res.insert(Name(b"Font"))
                    .dict()
                    .pair(Name(b"F1"), Ref::new(5));
            },
            |page| {
                page.contents(Ref::new(4));
            },
            |_| {},
        );

        let pdf = flatten(base, &[text_field("FullName", "Ada Lovelace")]).expect("flatten ok");

        let res = page_resources(&pdf);
        assert!(
            res.has(b"ProcSet"),
            "the inherited /Resources entries survive the page's own dict"
        );
        let fonts = res.get(b"Font").unwrap().as_dict().unwrap();
        assert!(fonts.has(b"F1"), "the background's font binding survives");
        assert!(
            fonts.has(b"Helv") && fonts.has(b"ZaDb"),
            "the drawn fonts are injected beside it"
        );
    }

    #[test]
    fn resources_inherit_along_the_kids_chain_without_a_parent_link() {
        // The same tree as `build_base`, minus the page's `/Parent`: the ancestor
        // chain is the `/Kids` walk's, the one `/Rotate` and `/MediaBox` resolve
        // along, so a missing back-link cannot detach the page from its resources.
        let letter = Rect::new(0.0, 0.0, 612.0, 792.0);
        let mut pdf = Pdf::new();
        pdf.catalog(Ref::new(1)).pages(Ref::new(2));
        {
            let mut pages = pdf.pages(Ref::new(2));
            pages.kids([Ref::new(3)]).count(1).media_box(letter);
            pages
                .insert(Name(b"Resources"))
                .dict()
                .insert(Name(b"Font"))
                .dict()
                .pair(Name(b"F1"), Ref::new(5));
        }
        pdf.page(Ref::new(3)).media_box(letter).contents(Ref::new(4));
        pdf.stream(Ref::new(4), b"BT /F1 12 Tf 72 700 Td (background) Tj ET");
        pdf.indirect(Ref::new(5))
            .dict()
            .pair(Name(b"Type"), Name(b"Font"))
            .pair(Name(b"Subtype"), Name(b"Type1"))
            .pair(Name(b"BaseFont"), Name(b"Helvetica"));
        let base = pdf.finish();

        let pdf = flatten(base, &[text_field("FullName", "Ada Lovelace")]).expect("flatten ok");

        let fonts = page_resources(&pdf)
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        assert!(fonts.has(b"F1"), "the background's font binding survives");
        assert!(fonts.has(b"Helv"), "the drawn font is injected beside it");
    }

    #[test]
    fn a_font_name_the_page_already_binds_keeps_its_own_font() {
        let base = build_base(
            |_| {},
            |page| {
                page.contents(Ref::new(4));
                let mut res = page.insert(Name(b"Resources")).dict();
                res.insert(Name(b"Font"))
                    .dict()
                    .pair(Name(b"Helv"), Ref::new(5));
            },
            |_| {},
        );

        let pdf = flatten(base, &[text_field("FullName", "Ada Lovelace")]).expect("flatten ok");

        let fonts = page_resources(&pdf)
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        assert_eq!(
            fonts.get(b"Helv").unwrap().as_reference().unwrap(),
            (5, 0),
            "/Helv stays bound to the background's own font"
        );
        assert!(
            fonts.has(b"Helv2"),
            "the drawn font takes a free name instead"
        );
        assert!(
            contains_window(&pdf, b"/Helv2 "),
            "the drawn stream selects the name it was registered under"
        );
    }

    #[test]
    fn a_contents_reference_to_an_array_expands_to_its_streams() {
        let base = build_base(
            |_| {},
            |page| {
                page.contents(Ref::new(6));
                let mut res = page.insert(Name(b"Resources")).dict();
                res.insert(Name(b"Font"))
                    .dict()
                    .pair(Name(b"F1"), Ref::new(5));
            },
            // The `/Contents` reference names an array object, not a stream.
            |pdf| {
                pdf.indirect(Ref::new(6))
                    .array()
                    .items([Ref::new(4), Ref::new(7)]);
                pdf.stream(Ref::new(7), b"0.75 w 72 690 200 20 re S");
            },
        );

        let pdf = flatten(base, &[text_field("FullName", "Ada Lovelace")]).expect("flatten ok");

        let doc = PdfDoc::load_mem(&pdf).expect("lopdf reparse: structurally valid");
        let (_, page_id) = doc.get_pages().into_iter().next().expect("one page");
        let contents = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_array()
            .expect("/Contents is an array")
            .clone();
        assert_eq!(
            contents.len(),
            3,
            "the referenced array's elements plus the drawn stream"
        );
        for item in &contents {
            let id = item.as_reference().expect("/Contents element is a reference");
            assert!(
                doc.get_object(id).unwrap().as_stream().is_ok(),
                "every /Contents element resolves to a stream"
            );
        }
    }

    #[test]
    fn flatten_produces_no_acroform() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);

        let doc = PdfDoc::load_mem(&pdf).expect("lopdf reparse: structurally valid");
        let cat = doc.catalog().expect("catalog");
        assert!(
            cat.get(b"AcroForm").is_err(),
            "flat PDF must not contain /AcroForm"
        );
    }

    #[test]
    fn flatten_text_font_declares_winansi_encoding() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);
        assert!(
            String::from_utf8_lossy(&pdf).contains("/Encoding /WinAnsiEncoding"),
            "text font must declare WinAnsiEncoding"
        );
    }

    #[test]
    fn flatten_clips_to_field_box() {
        let pdf = flatten_ok(&[text_field("FullName", "Ada Lovelace")]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains(" re W n"),
            "text must clip to the field box (re W n)"
        );
    }

    #[test]
    fn every_drawn_value_asserts_the_default_colour_and_text_state() {
        for spec in [
            text_field("FullName", "Ada Lovelace"),
            checkbox_field("Agree", true),
        ] {
            let stream = build_content_stream(&[&spec], &default_names());
            let reset = window_pos(&stream, b"0 g 0 Tr 0 Tc 0 Tw 100 Tz 0 Ts")
                .unwrap_or_else(|| panic!("`{}` draws no state reset", spec.name));
            let shown = window_pos(&stream, b") Tj").expect("the value is shown");
            assert!(
                reset < shown,
                "`{}` must reset before it shows, or a background's `0.9 g` or \
                 `3 Tr` paints the value invisible",
                spec.name
            );
        }
    }

    #[test]
    fn build_content_stream_transcodes_non_ascii_to_winansi() {
        let value = "Caf\u{e9} \u{2014} Se\u{f1}or \u{2019}A\u{2019}";
        let spec = text_field("FullName", value);
        let stream = build_content_stream(&[&spec], &default_names());

        // WinAnsi: é→0xE9, —→0x97, ñ→0xF1, ’→0x92.
        let want: &[u8] = &[
            b'C', b'a', b'f', 0xE9, b' ', 0x97, b' ', b'S', b'e', 0xF1, b'o', b'r', b' ', 0x92,
            b'A', 0x92,
        ];
        assert!(
            contains_window(&stream, want),
            "content stream must carry the WinAnsi-encoded value bytes"
        );
        assert!(
            !contains_window(&stream, &[b'f', 0xC3, 0xA9, b' ']),
            "value must not be drawn as raw UTF-8"
        );

        let pdf = flatten_ok(&[spec]);
        assert!(
            contains_window(&pdf, want),
            "flat PDF must carry the WinAnsi-encoded value bytes"
        );
    }

    #[test]
    fn flatten_checked_checkbox_emits_zapfdingbats_glyph() {
        let spec = checkbox_field("Agree", true);
        let stream = build_content_stream(&[&spec], &default_names());
        let text = String::from_utf8_lossy(&stream);

        assert!(
            text.contains("/ZaDb"),
            "checked checkbox must select the ZapfDingbats font (/ZaDb)"
        );
        assert!(
            contains_window(&stream, b"(4) Tj"),
            "checked checkbox must draw the check glyph"
        );

        let pdf = flatten_ok(&[spec]);
        assert!(
            String::from_utf8_lossy(&pdf).contains("/ZapfDingbats"),
            "flat PDF must declare the ZapfDingbats font"
        );

        assert!(has_drawable_value(&checkbox_field("Agree", true)));
        assert!(!has_drawable_value(&checkbox_field("Agree", false)));
        let unchecked = flatten_ok(&[checkbox_field("Agree", false)]);
        assert!(
            !contains_window(&unchecked, b"(4) Tj"),
            "unchecked checkbox must not draw the check glyph"
        );
    }

    #[test]
    fn a_form_with_nothing_to_draw_is_returned_unchanged() {
        assert_eq!(
            flatten_ok(&[checkbox_field("Agree", false)]),
            BASE,
            "no drawable value must append no revision"
        );
    }

    #[test]
    fn a_non_finite_rect_is_refused_rather_than_drawn() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut spec = text_field("FullName", "Ada Lovelace");
            spec.rect[1] = bad;
            let err = flatten(BASE.to_vec(), &[spec]).expect_err("non-finite rect rejected");
            assert_eq!(err.code, "pdf::bad_rect", "{bad}");
        }
    }
}
