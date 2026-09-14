//! The `/AP` `/N` appearance a stamped widget carries: one Form XObject drawing
//! the field's current value, so a consumer that never synthesizes
//! `/NeedAppearances` appearances — hayro, pdfium, Ghostscript — shows the same
//! page a form viewer does.
//!
//! Drawing commits to a byte encoding and a concrete size: text is transcoded to
//! WinAnsi and set in the widget's own base-14 face at [`FieldSpec::font_size`],
//! or at the auto-size a viewer would have picked. A viewer that honours
//! `/NeedAppearances` rebuilds the stream from `/V` and `/DA` instead, which is
//! where full Unicode and `/Q` justification reach it.

use pdf_writer::{Chunk, Finish, Name, Rect};

use crate::error::PdfError;
use crate::reader::UpdatedObject;
use crate::writer::{pdf_escape, to_ref, winansi_encode};
use crate::{FieldSpec, FieldType, CHECK_FONT_RESOURCE, CHECK_GLYPH};

const MIN_SIZE: f32 = 4.0;
const MAX_SIZE: f32 = 12.0;

/// Inset, in points, of value text from the field box's left edge.
const TEXT_INSET: f32 = 2.0;

/// Inset, in points, between the box's top edge and the first baseline.
const TEXT_TOP_INSET: f32 = 1.0;

/// Line height = point size × this.
const LINE_SPACING: f32 = 1.2;

/// Approximate advance width of the [`CHECK_GLYPH`] as a fraction of its point
/// size, used to horizontally centre it in the box.
const CHECK_GLYPH_WIDTH_FACTOR: f32 = 0.6;

/// The size a widget's value text is drawn at: its own when it carries one,
/// else the `0 Tf` auto-size a synthesizing viewer would pick for the box.
fn text_size(spec: &FieldSpec, h: f32) -> f32 {
    spec.font_size
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or_else(|| (h * 0.65).clamp(MIN_SIZE, MAX_SIZE))
}

/// Larger than [`text_size`]: the check glyph reads a touch small.
fn check_size(h: f32) -> f32 {
    (h * 0.75).clamp(MIN_SIZE, MAX_SIZE)
}

/// What a widget draws for its current value: the `/DR` face the stream selects,
/// the field box's extent, and the stream itself in appearance space — the field
/// box with its lower-left corner at the origin.
pub(crate) struct Appearance {
    resource: &'static str,
    bbox: [f32; 2],
    content: Vec<u8>,
}

/// The appearance `spec` draws, or `None` when it draws nothing: a blank field,
/// an unchecked box, a signature, or a box with no area to draw in.
pub(crate) fn of(spec: &FieldSpec) -> Option<Appearance> {
    let [x0, y0, x1, y1] = spec.rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }

    let (resource, size, x, y, lines) = match &spec.field_type {
        FieldType::Signature => return None,
        FieldType::Checkbox => {
            if !spec.is_checked() {
                return None;
            }
            let size = check_size(h);
            (
                CHECK_FONT_RESOURCE,
                size,
                (w - size * CHECK_GLYPH_WIDTH_FACTOR) * 0.5,
                (h - size) * 0.5,
                vec![CHECK_GLYPH.to_vec()],
            )
        }
        FieldType::Text { .. } => {
            let size = text_size(spec, h);
            (
                spec.font.resource_name(),
                size,
                TEXT_INSET,
                h - size - TEXT_TOP_INSET,
                spec.value.as_deref()?.lines().map(winansi_encode).collect(),
            )
        }
        FieldType::Choice { .. } => {
            let size = text_size(spec, h);
            (
                spec.font.resource_name(),
                size,
                TEXT_INSET,
                (h - size) * 0.5,
                vec![winansi_encode(spec.value.as_deref()?)],
            )
        }
    };

    if lines.is_empty() {
        return None;
    }
    Some(Appearance {
        resource,
        bbox: [w, h],
        content: show_lines(resource, size, x, y, &lines),
    })
}

impl Appearance {
    /// This appearance as one indirect Form XObject, over a `/BBox` the size of
    /// the field box and a `/Resources` binding the one face it selects: a form
    /// resolves names in its own dictionary, not the page's, and the `/BBox`
    /// clips an over-long value off the neighbouring content.
    pub(crate) fn object(&self, id: u32, font_id: u32) -> Result<UpdatedObject, PdfError> {
        let [w, h] = self.bbox;
        let mut chunk = Chunk::new();
        {
            let mut form = chunk.form_xobject(to_ref(id)?, &self.content);
            form.bbox(Rect::new(0.0, 0.0, w, h));
            form.resources()
                .fonts()
                .pair(Name(self.resource.as_bytes()), to_ref(font_id)?);
            form.finish();
        }
        Ok(UpdatedObject::new(id, chunk.as_bytes().to_vec()))
    }
}

/// `lines` shown from `(x, y)` downward in `resource` at `size`, each already
/// encoded in the face's own byte encoding.
fn show_lines(resource: &str, size: f32, x: f32, y: f32, lines: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!("BT\n/{resource} ").into_bytes();
    push_f32(&mut out, size);
    out.extend_from_slice(b" Tf\n0 g\n");
    push_f32(&mut out, x);
    out.push(b' ');
    push_f32(&mut out, y);
    out.extend_from_slice(b" Td\n");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"0 ");
            push_f32(&mut out, -size * LINE_SPACING);
            out.extend_from_slice(b" Td\n");
        }
        out.push(b'(');
        pdf_escape(&mut out, line);
        out.extend_from_slice(b") Tj\n");
    }
    out.extend_from_slice(b"ET\n");
    out
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
    use crate::{FormFont, CHECKBOX_ON_STATE};

    fn text(value: Option<&str>) -> FieldSpec {
        let mut spec = FieldSpec::new(
            "FullName".into(),
            0,
            [72.0, 700.0, 300.0, 720.0],
            FieldType::Text { multiline: false },
        );
        spec.value = value.map(str::to_string);
        spec
    }

    fn checkbox(checked: bool) -> FieldSpec {
        let mut spec = FieldSpec::new(
            "Agree".into(),
            0,
            [72.0, 660.0, 90.0, 678.0],
            FieldType::Checkbox,
        );
        spec.value = checked.then(|| CHECKBOX_ON_STATE.to_string());
        spec
    }

    fn content(spec: &FieldSpec) -> String {
        String::from_utf8_lossy(&of(spec).expect("an appearance").content).into_owned()
    }

    #[test]
    fn nothing_to_draw_is_no_appearance() {
        assert!(of(&text(None)).is_none(), "a blank field");
        assert!(of(&text(Some(""))).is_none(), "an empty value");
        assert!(of(&checkbox(false)).is_none(), "an unchecked box");
        assert!(
            of(&FieldSpec::new(
                "Sig".into(),
                0,
                [72.0, 100.0, 300.0, 140.0],
                FieldType::Signature
            ))
            .is_none(),
            "a signature"
        );
    }

    #[test]
    fn a_box_with_no_area_draws_nothing_rather_than_dividing_by_it() {
        // hayro maps the `/BBox` onto the `/Rect` by their ratio, so a zero side
        // would reach the raster as a non-finite transform.
        for rect in [
            [72.0, 700.0, 72.0, 720.0],
            [72.0, 700.0, 300.0, 700.0],
            [300.0, 700.0, 72.0, 720.0],
        ] {
            let mut spec = text(Some("Ada Lovelace"));
            spec.rect = rect;
            assert!(of(&spec).is_none(), "{rect:?}");
        }
    }

    #[test]
    fn the_appearance_is_drawn_in_the_box_moved_to_the_origin() {
        let ap = of(&text(Some("Ada Lovelace"))).expect("an appearance");
        assert_eq!(ap.bbox, [228.0, 20.0], "the field box's own extent");
        let drawn = String::from_utf8_lossy(&ap.content).into_owned();
        assert!(
            drawn.contains("\n2 7 Td\n"),
            "the first baseline is inset from the box's own top-left: {drawn}"
        );
    }

    #[test]
    fn a_multiline_value_steps_one_line_height_per_line() {
        let mut spec = text(Some("first\nsecond"));
        spec.field_type = FieldType::Text { multiline: true };
        let drawn = content(&spec);
        assert!(drawn.contains("(first) Tj"), "{drawn}");
        assert!(drawn.contains("(second) Tj"), "{drawn}");
        assert!(drawn.contains("0 -14.4 Td"), "12pt → 14.4pt leading: {drawn}");
    }

    #[test]
    fn the_stream_selects_the_widgets_own_face_and_size() {
        let mut spec = text(Some("Ada"));
        spec.font = FormFont::Times;
        spec.font_size = Some(9.0);
        let ap = of(&spec).expect("an appearance");
        assert_eq!(ap.resource, "TiRo");
        assert!(
            String::from_utf8_lossy(&ap.content).contains("/TiRo 9 Tf"),
            "an explicit size is drawn at that size, never the box's auto-size"
        );
    }

    #[test]
    fn an_auto_sized_widget_draws_at_the_size_a_viewer_would_pick() {
        assert!(content(&text(Some("Ada"))).contains("/Helv 12 Tf"));
        let mut small = text(Some("Ada"));
        small.rect = [72.0, 700.0, 300.0, 706.0];
        assert!(content(&small).contains("/Helv 4 Tf"), "clamped at the floor");
    }

    #[test]
    fn a_checked_box_draws_the_check_glyph_centred_in_its_own_face() {
        let ap = of(&checkbox(true)).expect("an appearance");
        assert_eq!(ap.resource, CHECK_FONT_RESOURCE);
        let drawn = String::from_utf8_lossy(&ap.content).into_owned();
        assert!(drawn.contains("/ZaDb 12 Tf"), "{drawn}");
        assert!(drawn.contains("(4) Tj"), "{drawn}");
        assert!(drawn.contains("\n5.4 3 Td\n"), "centred in the box: {drawn}");
    }

    #[test]
    fn value_bytes_are_transcoded_to_the_faces_winansi_encoding() {
        let drawn = of(&text(Some("Caf\u{e9} \u{2014} \u{65e5}")))
            .expect("an appearance")
            .content;
        // WinAnsi: é→0xE9, —→0x97; 日 has no WinAnsi byte.
        let want: &[u8] = &[b'C', b'a', b'f', 0xE9, b' ', 0x97, b' ', b'?'];
        assert!(
            drawn.windows(want.len()).any(|w| w == want),
            "{}",
            String::from_utf8_lossy(&drawn)
        );
    }

    #[test]
    fn a_value_that_closes_the_literal_string_is_escaped() {
        assert!(content(&text(Some("a(b)c\\d"))).contains("(a\\(b\\)c\\\\d) Tj"));
    }
}
