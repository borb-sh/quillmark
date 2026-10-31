//! The `/AP` `/N` appearance a stamped widget carries: Form XObjects drawing the
//! field's value, so a consumer that never synthesizes `/NeedAppearances`
//! appearances — hayro, pdfium, Ghostscript — shows the same page a form viewer
//! does. A value widget draws one stream; a checkbox draws one per state.
//!
//! Drawing commits to a byte encoding and a concrete size: text is transcoded to
//! WinAnsi and set in the widget's own base-14 face at [`FieldSpec::font_size`],
//! or at the auto-size a viewer would have picked, and a multiline value is
//! broken into lines that fit the box's width. A viewer that honours
//! `/NeedAppearances` rebuilds the stream from `/V` and `/DA` instead, which is
//! where full Unicode and `/Q` justification reach it.

use pdf_writer::{Chunk, Finish, Name, Rect};

use crate::error::PdfError;
use crate::reader::UpdatedObject;
use crate::writer::{pdf_escape, to_ref, winansi_encode};
use crate::{CHECK_FONT_RESOURCE, CHECK_GLYPH, FieldSpec, FieldType, FormFont};

const MIN_SIZE: f32 = 4.0;
const MAX_SIZE: f32 = 12.0;

/// How far an auto-sized multiline value steps down at a time to fit its box.
const SIZE_STEP: f32 = 0.5;

/// Inset, in points, of value text from the field box's left edge.
const TEXT_INSET: f32 = 2.0;

/// Inset, in points, between the box's top edge and the first baseline.
const TEXT_TOP_INSET: f32 = 1.0;

/// How much of a line's size sits above its baseline, as a viewer reckons it
/// when it centres a one-line value in its box.
const ONE_LINE_RISE: f32 = 0.65;

/// Line height = point size × this.
const LINE_SPACING: f32 = 1.2;

/// Approximate advance width of the [`CHECK_GLYPH`] as a fraction of its point
/// size, used to horizontally centre it in the box.
const CHECK_GLYPH_WIDTH_FACTOR: f32 = 0.6;

/// The size a widget's value text is drawn at: its own when it carries one,
/// else the `0 Tf` auto-size a synthesizing viewer would pick for the box.
fn text_size(spec: &FieldSpec, h: f32) -> f32 {
    explicit_size(spec).unwrap_or_else(|| (h * 0.65).clamp(MIN_SIZE, MAX_SIZE))
}

fn explicit_size(spec: &FieldSpec) -> Option<f32> {
    spec.font_size.filter(|s| s.is_finite() && *s > 0.0)
}

/// A one-line value's size: an auto-sized one shrinks until its widest line
/// fits `room`, as a viewer's `0 Tf` does, down to [`MIN_SIZE`].
fn one_line_size(spec: &FieldSpec, lines: &[Vec<u8>], room: f32, h: f32) -> f32 {
    let size = text_size(spec, h);
    if explicit_size(spec).is_some() {
        return size;
    }
    let widest = lines
        .iter()
        .map(|line| spec.font.width(line, 1.0))
        .fold(0.0, f32::max);
    if widest * size <= room {
        size
    } else {
        ((room / widest * 100.0).floor() / 100.0).max(MIN_SIZE)
    }
}

/// A multiline value broken into lines that fit `room`. An auto-sized one
/// steps down from [`text_size`] until its lines fit the box's height too, down
/// to [`MIN_SIZE`], where what still overflows is cut off at the bottom.
fn wrapped(spec: &FieldSpec, value: &str, room: f32, h: f32) -> (f32, Vec<Vec<u8>>) {
    let mut size = text_size(spec, h);
    if explicit_size(spec).is_some() {
        return (size, wrap(spec.font, value, room, size));
    }
    loop {
        let lines = wrap(spec.font, value, room, size);
        if lines_height(size, lines.len()) <= h || size <= MIN_SIZE {
            return (size, lines);
        }
        size = (size - SIZE_STEP).max(MIN_SIZE);
    }
}

fn lines_height(size: f32, lines: usize) -> f32 {
    TEXT_TOP_INSET + size + (lines.max(1) - 1) as f32 * size * LINE_SPACING
}

/// Whether the baked appearance cuts off some of `spec`'s value: a line wider
/// than the box, or wrapped lines taller than it, at the size it draws them,
/// which an auto-sized value reaches only at its smallest, 4 pt.
pub fn overflows(spec: &FieldSpec) -> bool {
    let [x0, y0, x1, y1] = spec.rect;
    let (w, h) = (x1 - x0, y1 - y0);
    let Some(value) = spec.value.as_deref() else {
        return false;
    };
    if !(w > 0.0 && h > 0.0) {
        return false;
    }
    let room = w - 2.0 * TEXT_INSET;
    match &spec.field_type {
        FieldType::Text { multiline: true } => {
            let (size, lines) = wrapped(spec, value, room, h);
            lines_height(size, lines.len()) > h
        }
        FieldType::Text { multiline: false } | FieldType::Choice { .. } => {
            let lines: Vec<Vec<u8>> = value.lines().map(winansi_encode).collect();
            let size = one_line_size(spec, &lines, room, h);
            lines.iter().any(|line| spec.font.width(line, size) > room)
        }
        FieldType::Checkbox | FieldType::Signature => false,
    }
}

/// `value` broken at spaces where a line would pass `room` at `size`, and
/// inside a word that is wider than `room` on its own. A newline always
/// breaks, and a space a break falls on is not drawn.
fn wrap(font: FormFont, value: &str, room: f32, size: f32) -> Vec<Vec<u8>> {
    let space = font.width(b" ", size);
    let mut lines = Vec::new();
    for paragraph in value.lines() {
        let mut line: Vec<u8> = Vec::new();
        let mut line_width = 0.0;
        for (i, word) in paragraph.split(' ').enumerate() {
            let word = winansi_encode(word);
            let word_width = font.width(&word, size);
            if i > 0 && line_width + space + word_width <= room {
                line.push(b' ');
                line.extend_from_slice(&word);
                line_width += space + word_width;
                continue;
            }
            if i > 0 {
                lines.push(std::mem::take(&mut line));
            }
            line = word;
            line_width = word_width;
            while line_width > room && line.len() > 1 {
                let mut cut = 0;
                let mut head = 0.0;
                while cut < line.len() {
                    let next = font.width(&line[cut..=cut], size);
                    if cut > 0 && head + next > room {
                        break;
                    }
                    head += next;
                    cut += 1;
                }
                if cut == line.len() {
                    break;
                }
                let tail = line.split_off(cut);
                lines.push(std::mem::replace(&mut line, tail));
                line_width -= head;
            }
        }
        lines.push(line);
    }
    lines
}

fn one_line_baseline(h: f32, size: f32) -> f32 {
    (h - size * ONE_LINE_RISE) * 0.5
}

/// Larger than [`text_size`]: the check glyph reads a touch small.
fn check_size(h: f32) -> f32 {
    (h * 0.75).clamp(MIN_SIZE, MAX_SIZE)
}

/// What a widget's `/AP` `/N` holds.
pub(crate) enum Normal {
    /// The one stream a text or choice widget draws its value in.
    Value(Appearance),
    /// A checkbox's two states, drawn whatever its value, so a filler renaming
    /// `/AS` has a state to show (ISO 32000-1 §12.7.4.2.3).
    States { on: Appearance, off: Appearance },
}

/// One appearance stream: the `/DR` face it selects, if any, the field box's
/// extent, and the stream itself in appearance space — the field box with its
/// lower-left corner at the origin.
pub(crate) struct Appearance {
    resource: Option<&'static str>,
    bbox: [f32; 2],
    content: Vec<u8>,
}

/// The appearance `spec` draws, or `None` when it draws nothing: a blank field,
/// a signature, or a box with no area to draw in.
pub(crate) fn of(spec: &FieldSpec) -> Option<Normal> {
    let [x0, y0, x1, y1] = spec.rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }

    let room = w - 2.0 * TEXT_INSET;
    let (resource, size, x, y, lines) = match &spec.field_type {
        FieldType::Signature => return None,
        FieldType::Checkbox => return Some(check_states([w, h])),
        FieldType::Text { multiline: true } => {
            let (size, lines) = wrapped(spec, spec.value.as_deref()?, room, h);
            (
                spec.font.resource_name(),
                size,
                TEXT_INSET,
                h - size - TEXT_TOP_INSET,
                lines,
            )
        }
        FieldType::Text { multiline: false } => {
            let lines: Vec<Vec<u8>> = spec.value.as_deref()?.lines().map(winansi_encode).collect();
            let size = one_line_size(spec, &lines, room, h);
            let y = if lines.len() == 1 {
                one_line_baseline(h, size)
            } else {
                h - size - TEXT_TOP_INSET
            };
            (spec.font.resource_name(), size, TEXT_INSET, y, lines)
        }
        FieldType::Choice { .. } => {
            let line = winansi_encode(spec.value.as_deref()?);
            let size = one_line_size(spec, std::slice::from_ref(&line), room, h);
            (
                spec.font.resource_name(),
                size,
                TEXT_INSET,
                one_line_baseline(h, size),
                vec![line],
            )
        }
    };

    if lines.is_empty() {
        return None;
    }
    Some(Normal::Value(Appearance {
        resource: Some(resource),
        bbox: [w, h],
        content: show_lines(resource, size, x, y, &lines),
    }))
}

/// On shows the check glyph centred in the box. Off draws nothing: the
/// background owns the box's border and fill, so the mark is all on draws.
fn check_states(bbox: [f32; 2]) -> Normal {
    let [w, h] = bbox;
    let size = check_size(h);
    let x = (w - size * CHECK_GLYPH_WIDTH_FACTOR) * 0.5;
    let y = (h - size) * 0.5;
    Normal::States {
        on: Appearance {
            resource: Some(CHECK_FONT_RESOURCE),
            bbox,
            content: show_lines(CHECK_FONT_RESOURCE, size, x, y, &[CHECK_GLYPH.to_vec()]),
        },
        off: Appearance {
            resource: None,
            bbox,
            content: Vec::new(),
        },
    }
}

impl Appearance {
    /// This appearance as one indirect Form XObject, over a `/BBox` the size of
    /// the field box and a `/Resources` binding the one face it selects, as
    /// `font_id`: a form resolves names in its own dictionary, not the page's,
    /// and the `/BBox` clips an over-long value off the neighbouring content.
    pub(crate) fn object(&self, id: u32, font_id: u32) -> Result<UpdatedObject, PdfError> {
        let [w, h] = self.bbox;
        let mut chunk = Chunk::new();
        {
            let mut form = chunk.form_xobject(to_ref(id)?, &self.content);
            form.bbox(Rect::new(0.0, 0.0, w, h));
            if let Some(resource) = self.resource {
                form.resources()
                    .fonts()
                    .pair(Name(resource.as_bytes()), to_ref(font_id)?);
            }
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
    use crate::{CHECKBOX_ON_STATE, FormFont};

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

    fn checkbox() -> FieldSpec {
        let mut spec = FieldSpec::new(
            "Agree".into(),
            0,
            [72.0, 660.0, 90.0, 678.0],
            FieldType::Checkbox,
        );
        spec.value = Some(CHECKBOX_ON_STATE.to_string());
        spec
    }

    fn value(spec: &FieldSpec) -> Appearance {
        match of(spec) {
            Some(Normal::Value(ap)) => ap,
            _ => panic!("{} draws no value stream", spec.name),
        }
    }

    fn content(spec: &FieldSpec) -> String {
        String::from_utf8_lossy(&value(spec).content).into_owned()
    }

    #[test]
    fn nothing_to_draw_is_no_appearance() {
        assert!(of(&text(None)).is_none(), "a blank field");
        assert!(of(&text(Some(""))).is_none(), "an empty value");
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
            for mut spec in [text(Some("Ada Lovelace")), checkbox()] {
                spec.rect = rect;
                assert!(of(&spec).is_none(), "{rect:?}");
            }
        }
    }

    #[test]
    fn the_appearance_is_drawn_in_the_box_moved_to_the_origin() {
        let ap = value(&text(Some("Ada Lovelace")));
        assert_eq!(ap.bbox, [228.0, 20.0], "the field box's own extent");
        let drawn = String::from_utf8_lossy(&ap.content).into_owned();
        assert!(
            drawn.contains("\n2 6.1 Td\n"),
            "a one-line value is inset from the box's left and centred in its height: {drawn}"
        );
    }

    #[test]
    fn a_multiline_value_steps_one_line_height_per_line() {
        let mut spec = text(Some("first\nsecond"));
        spec.field_type = FieldType::Text { multiline: true };
        spec.rect = [72.0, 600.0, 300.0, 720.0];
        let drawn = content(&spec);
        assert!(drawn.contains("(first) Tj"), "{drawn}");
        assert!(drawn.contains("(second) Tj"), "{drawn}");
        assert!(
            drawn.contains("0 -14.4 Td"),
            "12pt → 14.4pt leading: {drawn}"
        );
    }

    /// The `(..) Tj` lines a stream shows, unescaped, and the size it sets.
    fn shown(spec: &FieldSpec) -> (f32, Vec<String>) {
        let drawn = content(spec);
        let size = drawn
            .split_once(" Tf")
            .and_then(|(head, _)| head.rsplit(' ').next())
            .and_then(|s| s.parse().ok())
            .expect("a stream sets its size");
        let lines = drawn
            .lines()
            .filter_map(|l| l.strip_prefix('(')?.strip_suffix(") Tj"))
            .map(|l| l.replace("\\(", "(").replace("\\)", ")"))
            .collect();
        (size, lines)
    }

    fn paragraph(spec: &mut FieldSpec, value: &str, rect: [f32; 4]) {
        spec.field_type = FieldType::Text { multiline: true };
        spec.value = Some(value.into());
        spec.rect = rect;
    }

    #[test]
    fn a_multiline_value_breaks_into_lines_that_fit_its_box() {
        let value = "Led an 8-member team that rebuilt the base network core, restoring \
                     service to 2,100 users in six hours and saving the contract support.\n\
                     Second statement.";
        for font in [FormFont::Helvetica, FormFont::Times, FormFont::Courier] {
            let mut spec = text(None);
            spec.font = font;
            paragraph(&mut spec, value, [20.0, 25.0, 300.0, 640.0]);
            let (size, lines) = shown(&spec);
            assert_eq!(size, 12.0, "{font:?}: a tall box keeps the auto-size");
            assert!(lines.len() > 2, "{font:?}: {lines:?}");
            for line in &lines {
                assert!(
                    font.width(line.as_bytes(), size) <= 280.0 - 2.0 * TEXT_INSET,
                    "{font:?}: {line:?} is wider than the box"
                );
            }
            assert_eq!(
                lines.last().unwrap(),
                "Second statement.",
                "a newline breaks"
            );
            assert_eq!(
                lines[..lines.len() - 1].join(" "),
                value.lines().next().unwrap(),
                "{font:?}: every word is drawn once, in order"
            );
        }
    }

    #[test]
    fn a_word_wider_than_its_box_breaks_inside_itself() {
        let mut spec = text(None);
        paragraph(&mut spec, &"W".repeat(40), [0.0, 0.0, 100.0, 400.0]);
        let (size, lines) = shown(&spec);
        assert!(lines.len() > 1, "{lines:?}");
        assert_eq!(lines.concat(), "W".repeat(40));
        assert!(
            lines
                .iter()
                .all(|l| FormFont::Helvetica.width(l.as_bytes(), size) <= 96.0)
        );
    }

    #[test]
    fn an_auto_sized_multiline_value_shrinks_until_its_lines_fit_the_box() {
        let mut spec = text(None);
        paragraph(
            &mut spec,
            &"one two three four five six ".repeat(6),
            [0.0, 0.0, 200.0, 60.0],
        );
        let (size, lines) = shown(&spec);
        assert!(size < 12.0, "{size}");
        assert!(
            TEXT_TOP_INSET + size + (lines.len() - 1) as f32 * size * LINE_SPACING <= 60.0,
            "{} lines at {size}pt overflow the box",
            lines.len()
        );

        spec.font_size = Some(12.0);
        assert_eq!(shown(&spec).0, 12.0, "an explicit size is never shrunk");
    }

    #[test]
    fn an_auto_sized_line_shrinks_to_its_box_width_and_an_explicit_one_does_not() {
        let value = "Lt Col Casey B. Sample, DSN 555-0100, Comm (555) 555-0100";
        let mut spec = text(Some(value));
        spec.rect = [0.0, 0.0, 150.0, 20.0];
        let (size, lines) = shown(&spec);
        assert_eq!(lines, [value]);
        assert!(size < 12.0, "{size}");
        assert!(FormFont::Helvetica.width(value.as_bytes(), size) <= 146.0);

        spec.font_size = Some(10.0);
        assert_eq!(shown(&spec).0, 10.0);
    }

    #[test]
    fn a_shrunk_line_stays_centred_in_its_box() {
        let mut spec = text(Some(
            "Lt Col Casey B. Sample, DSN 555-0100, Comm (555) 555-0100",
        ));
        spec.rect = [0.0, 0.0, 150.0, 20.0];
        let (size, _) = shown(&spec);
        let drawn = content(&spec);
        let baseline: f32 = drawn
            .lines()
            .find_map(|l| l.strip_prefix("2 ")?.strip_suffix(" Td"))
            .and_then(|y| y.parse().ok())
            .expect("the line is placed");
        let cap_height = size * 0.718;
        let (below, above) = (baseline, 20.0 - baseline - cap_height);
        assert!(size < 12.0, "the line shrinks to {size}pt");
        assert!(
            below > 0.0 && above > 0.0 && (above - below).abs() < size * 0.1,
            "{size}pt capitals sit {below} over the box's floor and {above} under its top"
        );
    }

    #[test]
    fn a_value_overflows_only_where_the_smallest_size_cannot_fit_it() {
        let mut line = text(Some("Ada"));
        line.rect = [0.0, 0.0, 150.0, 20.0];
        assert!(!overflows(&line));
        line.value = Some("Sample ".repeat(6));
        assert!(!overflows(&line), "a long line shrinks to fit");
        line.value = Some("Sample ".repeat(30));
        assert!(overflows(&line), "past 4pt it runs off the box");
        line.value = Some("Sample ".repeat(6));
        line.font_size = Some(12.0);
        assert!(overflows(&line), "a widget's own size does not shrink");

        let mut box_ = text(None);
        paragraph(
            &mut box_,
            &"Sample text. ".repeat(40),
            [0.0, 0.0, 200.0, 60.0],
        );
        assert!(!overflows(&box_), "a paragraph shrinks until its lines fit");
        paragraph(
            &mut box_,
            &"Sample text. ".repeat(400),
            [0.0, 0.0, 200.0, 60.0],
        );
        assert!(overflows(&box_), "past 4pt its last lines are cut off");
    }

    #[test]
    fn the_stream_selects_the_widgets_own_face_and_size() {
        let mut spec = text(Some("Ada"));
        spec.font = FormFont::Times;
        spec.font_size = Some(9.0);
        let ap = value(&spec);
        assert_eq!(ap.resource, Some("TiRo"));
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
        assert!(
            content(&small).contains("/Helv 4 Tf"),
            "clamped at the floor"
        );
    }

    #[test]
    fn a_box_draws_its_mark_centred_at_the_box_size() {
        let Some(Normal::States { on, .. }) = of(&checkbox()) else {
            panic!("a box draws its states");
        };
        assert_eq!(on.resource, Some(CHECK_FONT_RESOURCE));
        let drawn = String::from_utf8_lossy(&on.content).into_owned();
        assert!(drawn.contains("/ZaDb 12 Tf"), "{drawn}");
        assert!(
            drawn.contains("\n5.4 3 Td\n"),
            "centred in the box: {drawn}"
        );
    }

    #[test]
    fn a_value_that_closes_the_literal_string_is_escaped() {
        assert!(content(&text(Some("a(b)c\\d"))).contains("(a\\(b\\)c\\\\d) Tj"));
    }
}
