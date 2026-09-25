//! Regions read off the laid-out frames — span-tracked content, scalar
//! reference sites, the `form-field` `field:` binding — and the forward
//! `field_at`/`position_at`/`locate` navigation over them.

use quillmark_core::backend::Backend;
use quillmark_core::region::HitGranularity;
use quillmark_core::session::LiveSession;
use quillmark_typst::TypstBackend;

mod common;
use common::{content, quill_with_plate as quill, yaml};

fn open(schema: &str, plate: &str, data: serde_json::Value) -> LiveSession {
    TypstBackend
        .open(&quill(&yaml(schema), plate), &data, None)
        .expect("open")
}

fn centre(rect: [f32; 4]) -> (f32, f32) {
    ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0)
}

/// One richtext `body` on a letter page.
fn body_session(body: serde_json::Value) -> LiveSession {
    open(
        "main:\n  fields:\n    body: { type: richtext }\n",
        r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.body
"#,
        serde_json::json!({ "body": body }),
    )
}

#[test]
fn content_fields_emit_frame_regions() {
    // Page chrome too: a header and footer must not truncate a placement to
    // its first page.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(
  width: 612pt,
  height: 792pt,
  margin: 72pt,
  header: [Running Header],
  footer: [Running Footer],
)
#set text(size: 11pt)

#data.intro

#data.body
"#;

    // Long enough to overflow page 0 and continue.
    let long = "This is a markdown paragraph that wraps across several lines. ".repeat(200);
    let session = open(
        "main:\n  fields:\n    intro: { type: richtext }\n    body: { type: richtext }\n",
        PLATE,
        serde_json::json!({
            "intro": content("A **short** intro paragraph on the first page."),
            "body": content(&long),
        }),
    );
    let regions = session.regions();

    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(intro.len(), 1, "intro is one region: {regions:?}");
    let [x0, y0, x1, y1] = intro[0].rect;
    assert!(
        x1 > x0 && y1 > y0,
        "intro has positive area: {:?}",
        intro[0].rect
    );

    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "page-spanning body surfaces one fragment per page: {body:?}"
    );
    assert_eq!(body[0].page, 0, "first fragment on the page the body opens");
    let pages: Vec<usize> = body.iter().map(|r| r.page).collect();
    assert!(
        pages.windows(2).all(|w| w[1] == w[0] + 1),
        "fragments cover consecutive pages, one each: {pages:?}"
    );
    for r in &body {
        assert!(
            r.rect[2] - r.rect[0] > 200.0,
            "each body fragment spans most of the text column: {:?}",
            r.rect
        );
    }
}

#[test]
fn field_placed_twice_surfaces_first_region_but_field_at_resolves_every_placement() {
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro

#lorem(40)

#data.intro
"#;
    let session = open(
        "main:\n  fields:\n    intro: { type: richtext }\n",
        PLATE,
        serde_json::json!({ "intro": content("The same intro, placed twice.") }),
    );
    let regions = session.regions();
    let intro: Vec<_> = regions.iter().filter(|r| r.field == "intro").collect();
    assert_eq!(
        intro.len(),
        1,
        "a twice-placed content value surfaces its first placement only: {regions:?}"
    );
    let first = intro[0];
    let [_, y0, _, y1] = first.rect;
    assert!(
        y1 - y0 < 100.0,
        "the region is one placement's extent, not a spanning union: {:?}",
        first.rect
    );
    assert!(
        y1 > 600.0,
        "the region is the top-of-page first placement (bottom-left origin): {:?}",
        first.rect
    );

    let (cx, cy) = centre(first.rect);
    assert_eq!(
        session.field_at(first.page, cx, cy, 0.0).as_deref(),
        Some("intro"),
        "a click inside the first placement resolves"
    );

    // Probe downward past the lorem filler until ink resolves again.
    let mut second_hit = None;
    let mut y = first.rect[1] - 12.0;
    while y > 40.0 {
        if let Some(f) = session.field_at(first.page, cx, y, 0.0) {
            second_hit = Some(f);
        }
        y -= 6.0;
    }
    assert_eq!(
        second_hit.as_deref(),
        Some("intro"),
        "the second, un-surfaced placement still resolves point-wise"
    );

    assert_eq!(
        session.field_at(first.page, 5.0, 5.0, 0.0),
        None,
        "a click off any field's ink resolves to nothing"
    );
}

/// Each reference site surfaces on its own, a single-reference wrapping
/// expression included, and a click on any of them routes to the field.
#[test]
fn scalar_reference_sites_each_surface_a_region() {
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

*#data.subject*

#lorem(20)

#data.at("subject")

#lorem(20)

#upper(data.subject)
"#;
    let session = open(
        "main:\n  fields:\n    subject: { type: string }\n",
        PLATE,
        serde_json::json!({ "subject": "Request for Quarters" }),
    );
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        3,
        "each scalar reference site surfaces independently: {regions:?}"
    );
    for pair in subject.windows(2) {
        assert!(
            pair[0].rect[1] > pair[1].rect[3],
            "sites do not union: {:?} above {:?}",
            pair[0].rect,
            pair[1].rect
        );
    }
    for r in &subject {
        let (cx, cy) = centre(r.rect);
        assert_eq!(
            session.field_at(r.page, cx, cy, 0.0).as_deref(),
            Some("subject"),
            "a click on {:?} routes to the field",
            r.rect
        );
    }
}

/// A module the plate imports is scanned like the plate: its `data` read keeps
/// its click target when the plate draws it through the module's function.
#[test]
fn a_scalar_read_in_an_imported_module_surfaces_a_region() {
    let q = common::quill(
        &yaml("main:\n  fields:\n    subject: { type: string }\n"),
        &[
            (
                "plate.typ",
                b"#import \"lib.typ\": heading-line\n\
                  #set page(width: 612pt, height: 792pt, margin: 72pt)\n\
                  #heading-line()\n",
            ),
            (
                "lib.typ",
                b"#import \"@local/quillmark-helper:0.1.0\": data\n\
                  #let heading-line() = [*#data.subject*]\n",
            ),
        ],
    );
    let session = TypstBackend
        .open(&q, &serde_json::json!({ "subject": "Request for Quarters" }), None)
        .expect("open");
    let regions = session.regions();
    let subject = regions
        .iter()
        .find(|r| r.field == "subject")
        .unwrap_or_else(|| panic!("the module's read surfaces a region: {regions:?}"));
    let (cx, cy) = centre(subject.rect);
    assert_eq!(
        session.field_at(subject.page, cx, cy, 0.0).as_deref(),
        Some("subject")
    );
}

#[test]
fn content_survives_a_rebuilding_show_rule() {
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#let BUF = state("BUF", ())
#let capture(it) = {
  show par: p => {
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }
  it
}

#capture(data.body)

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let session = open(
        "main:\n  fields:\n    body: { type: richtext }\n",
        PLATE,
        serde_json::json!({ "body": content("A body paragraph the package rebuilds.") }),
    );
    let regions = session.regions();
    assert!(
        regions.iter().any(|r| r.field == "body"),
        "a rebuilt body still surfaces a region, with no explicit tagging: {regions:?}"
    );
}

#[test]
fn markdown_array_elements_surface_indexed_regions() {
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#for r in data.refs {
  block(r)
}
"#;
    let session = open(
        "main:\n  fields:\n    refs:\n      type: array\n      items: { type: richtext }\n",
        PLATE,
        serde_json::json!({ "refs": [content("First reference."), content("Second reference.")] }),
    );
    let regions = session.regions();
    for expected in ["refs.0", "refs.1"] {
        assert!(
            regions.iter().any(|r| r.field == expected),
            "each richtext[] element gets its own eval site and region {expected:?}: {regions:?}"
        );
    }
    assert!(
        !regions.iter().any(|r| r.field == "refs"),
        "the array itself is not a region key: {regions:?}"
    );
}

#[test]
fn card_regions_use_canonical_kind_ordinal_path() {
    // Interleaved alpha/beta/alpha: the ordinal is per kind, so the second alpha
    // is `.1` even though it is the third card overall.
    const SCHEMA: &str = "\
main:
  fields:
    intro: { type: richtext }
card_kinds:
  alpha:
    fields:
      note: { type: richtext }
  beta:
    fields:
      note: { type: richtext }
";
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#for card in data.at("$cards", default: ()) {
  card.at("note", default: [])
  parbreak()
}
"#;
    let session = open(
        SCHEMA,
        PLATE,
        serde_json::json!({
            "intro": content("Top-level intro."),
            "$cards": [
                {"$kind": "alpha", "note": content("Alpha one.")},
                {"$kind": "beta",  "note": content("Beta one.")},
                {"$kind": "alpha", "note": content("Alpha two.")},
            ],
        }),
    );
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();

    for expected in [
        "intro",
        "$cards.alpha.0.note",
        "$cards.beta.0.note",
        "$cards.alpha.1.note",
    ] {
        assert!(
            fields.contains(expected),
            "expected a region keyed {expected:?}; got {fields:?}"
        );
    }
    assert!(
        !fields.iter().any(|f| f.starts_with("$cards.0.")
            || f.starts_with("$cards.1.")
            || f.starts_with("$cards.2.")),
        "card regions must use kind+ordinal, not positional index: {fields:?}"
    );
}

#[test]
fn date_field_display_surfaces_a_clickable_region() {
    // `data.issued` is the native `datetime`, so the comparison and the
    // component read are ordinary Typst; `display` places the ink that regions.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, display
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#assert(data.issued.year() == 2026)
#assert(data.issued < datetime(year: 2027, month: 1, day: 1))
#display("issued", "[day padding:none] [month repr:long] [year]")
"#;
    let session = open(
        "main:\n  fields:\n    issued: { type: date }\n",
        PLATE,
        serde_json::json!({ "issued": "2026-01-02" }),
    );
    let regions = session.regions();
    let issued: Vec<_> = regions.iter().filter(|r| r.field == "issued").collect();
    assert_eq!(
        issued.len(),
        1,
        "the rendered date surfaces exactly one whole-placement region: {regions:?}"
    );
    let [x0, y0, x1, y1] = issued[0].rect;
    assert!(
        x1 > x0 && y1 > y0,
        "the date region has positive area: {:?}",
        issued[0].rect
    );
    assert!(
        issued[0].span.is_none(),
        "a date region is whole-placement, carrying no content span: {:?}",
        issued[0]
    );
    let (cx, cy) = centre(issued[0].rect);
    assert_eq!(
        session.field_at(issued[0].page, cx, cy, 0.0).as_deref(),
        Some("issued"),
        "a click on the date ink routes to its schema path"
    );
}

#[test]
fn card_dates_surface_per_instance_regions_through_laundering() {
    // `scalar_windows` does not chase the shared `card.<field>` loop variable,
    // so a card date surfaces only through its own per-instance `text(..)` node,
    // which `display` reaches by address rather than through the value.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, display
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#for card in data.at("$cards", default: ()) {
  let d = card.at("on", default: none)
  if d != none {
    display(card.at("$path") + "on", "[day padding:none] [month repr:long] [year]")
  } else {
    [—]
  }
  parbreak()
}
"#;
    let session = open(
        "card_kinds:\n  stamp:\n    fields:\n      on: { type: date }\n",
        PLATE,
        serde_json::json!({
            "$cards": [
                {"$kind": "stamp", "on": "2026-01-02"},
                {"$kind": "stamp"},
                {"$kind": "stamp", "on": "2028-03-04"},
            ],
        }),
    );
    let regions = session.regions();
    let fields: std::collections::HashSet<&str> =
        regions.iter().map(|r| r.field.as_str()).collect();
    for expected in ["$cards.stamp.0.on", "$cards.stamp.2.on"] {
        assert!(
            fields.contains(expected),
            "a card's date regions per-instance: {fields:?}"
        );
    }
    // A `none` date draws no ink: absent, not a zero-area box.
    assert!(
        !fields.contains("$cards.stamp.1.on"),
        "a blank card date surfaces no region: {fields:?}"
    );
    let first = regions
        .iter()
        .find(|r| r.field == "$cards.stamp.0.on")
        .expect("first card date region present");
    let (cx, cy) = centre(first.rect);
    assert_eq!(
        session.field_at(first.page, cx, cy, 0.0).as_deref(),
        Some("$cards.stamp.0.on"),
        "a click on a laundered card date routes to its per-instance schema path"
    );
}

#[test]
fn failed_update_keeps_serving_last_good_regions() {
    // A failed compile has already written the next injection's helper source
    // into the world, but the served document's spans must keep resolving
    // against the compile they came from.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro
"#;
    let mut session = open(
        "main:\n  fields:\n    intro: { type: richtext }\n    when: { type: date }\n",
        PLATE,
        serde_json::json!({
            "intro": content("A stable paragraph the session keeps serving."),
            "when": "2026-07-03",
        }),
    );
    let before = session.regions();
    let intro = before
        .iter()
        .find(|r| r.field == "intro")
        .expect("baseline intro region");

    // Shorter content shifts every byte offset in the regenerated helper, and
    // the unparseable date fails the compile at data-assembly time.
    let bad = serde_json::json!({ "intro": content("X"), "when": "not-a-date" });
    session
        .update_data(&bad)
        .expect_err("the bad date must fail the compile");

    assert_eq!(
        session.regions(),
        before,
        "a failed update must not move or drop the served compile's regions"
    );
    let (cx, cy) = centre(intro.rect);
    assert_eq!(
        session.field_at(intro.page, cx, cy, 0.0).as_deref(),
        Some("intro"),
        "clicks keep resolving against the served compile"
    );
}

#[test]
fn form_field_path_rejected_when_address_tables_are_empty() {
    // Empty address tables validate against the empty set: every address
    // rejects. Only absent tables are permissive.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#form-field("S", type: "text", value: "x", field: "subject")
"#;
    let err = TypstBackend
        .open(
            &quill(&yaml("main:\n  body:\n    enabled: false\n"), PLATE),
            &serde_json::json!({}),
            None,
        )
        .err()
        .expect("an address must still fail when the tables are empty, not absent");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("subject"),
        "the compile error names the bad path: {msg}"
    );
}

#[test]
fn adversarial_codegen_inputs_still_compile() {
    // The generated helper is Typst source built from document data, so a data
    // value must never produce source that fails to parse. Edges the string
    // shape of the codegen could hide but a real compile catches:
    //   - an unterminated `<u>` yields unbalanced `#underline[` markup that, in
    //     a `[ .. ]` content block, would break the whole helper file;
    //   - `strong[0,4)` partially overlapping `code[2,6)`, a content an editor
    //     can build but markdown import never produces;
    //   - `i64::MIN` cannot be a Typst int literal (its magnitude overflows).
    use quillmark_content::model::{Content, Line, LineKind, Mark, MarkKind};
    // The plate's assert proves the int round-tripped.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#assert(data.at("n") == -9223372036854775807 - 1)
#data.body
#data.overlap
"#;
    let overlap = Content::new("abcdef".to_string(), vec![Line::new(LineKind::Para)]).with_marks(vec![
        Mark::new(0, 4, MarkKind::Strong),
        Mark::new(2, 6, MarkKind::Code),
    ]);
    TypstBackend
        .open(
            &quill(
                &yaml("main:\n  fields:\n    body: { type: richtext }\n    overlap: { type: richtext }\n"),
                PLATE,
            ),
            &serde_json::json!({
                "body": content("Please <u>sign here"),
                "overlap": quillmark_content::serial::to_canonical_value(&overlap.into_normalized()),
                "n": i64::MIN,
            }),
            None,
        )
        .expect("adversarial data must still compile");
}

/// `aaa` is painted first, `zzz` on top of it; a click in the shared box must
/// resolve to `zzz`, not to the alphabetically-first `/T` name.
#[test]
fn overlapping_widgets_resolve_field_at_by_paint_order_not_name() {
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 300pt, height: 200pt, margin: 0pt)
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("aaa", type: "text", field: "aaa_early", width: 40pt, height: 40pt))
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("zzz", type: "text", field: "zzz_late", width: 40pt, height: 40pt))
"#;
    let session = open(
        "main:\n  fields:\n    aaa_early: { type: string }\n    zzz_late: { type: string }\n",
        PLATE,
        serde_json::json!({}),
    );
    let regions = session.regions();
    let a = regions
        .iter()
        .find(|r| r.field == "aaa_early")
        .expect("aaa_early widget region");
    let z = regions
        .iter()
        .find(|r| r.field == "zzz_late")
        .expect("zzz_late widget region");
    assert_eq!(a.page, z.page, "both widgets on the same page");
    let (cx, cy) = centre(z.rect);
    assert_eq!(
        session.field_at(z.page, cx, cy, 0.0).as_deref(),
        Some("zzz_late"),
        "the later-painted widget wins the click, not the alphabetically-first name"
    );
}

#[test]
fn segment_regions_carry_span_and_field_union_is_striped() {
    let session = body_session(content("First paragraph, alpha.\n\nSecond paragraph, beta."));
    let body: Vec<_> = session
        .regions()
        .into_iter()
        .filter(|r| r.field == "body")
        .collect();
    assert_eq!(
        body.len(),
        2,
        "two paragraphs → two segment regions: {body:?}"
    );
    assert!(body.iter().all(|r| r.page == 0), "both on page 0: {body:?}");

    let s0 = body[0].span.expect("segment 0 carries a span");
    let s1 = body[1].span.expect("segment 1 carries a span");
    assert!(
        s0[0] < s0[1] && s1[0] < s1[1],
        "non-empty spans: {s0:?} {s1:?}"
    );
    assert!(s0[1] <= s1[0], "spans disjoint and ordered: {s0:?} {s1:?}");

    let h0 = body[0].rect[3] - body[0].rect[1];
    let h1 = body[1].rect[3] - body[1].rect[1];
    let union_lo = body[0].rect[1].min(body[1].rect[1]);
    let union_hi = body[0].rect[3].max(body[1].rect[3]);
    assert!(
        union_hi - union_lo > h0 + h1 + 1.0,
        "the field union is striped: union {} exceeds segments {h0}+{h1}, so the \
         blank line between paragraphs is uncovered",
        union_hi - union_lo
    );
}

#[test]
fn position_at_and_locate_round_trip_a_content_offset() {
    const TEXT: &str = "Alpha beta gamma delta epsilon.";
    let session = body_session(content(TEXT));
    let body: Vec<_> = session
        .regions()
        .into_iter()
        .filter(|r| r.field == "body")
        .collect();
    assert_eq!(body.len(), 1, "one paragraph, one region: {body:?}");
    let region = &body[0];
    let span = region.span.expect("content region carries a span");

    let hit = session
        .position_at(region.page, region.rect[0] + 5.0, region.rect[3] - 3.0, 0.0)
        .expect("a click inside content resolves to a content position");
    assert_eq!(hit.field, "body");
    assert!(
        span[0] <= hit.pos && hit.pos <= span[1],
        "pos {} within span {span:?}",
        hit.pos
    );
    assert_eq!(
        hit.granularity,
        Some(HitGranularity::Cluster),
        "a prose hit is cluster-exact: {hit:?}"
    );

    let caret = session
        .locate("body", hit.pos)
        .expect("a content position locates a caret rect");
    assert_eq!(caret.page, region.page);
    assert_eq!(caret.span, Some([hit.pos, hit.pos]));
    assert!(
        caret.rect[0] >= region.rect[0] - 1.0
            && caret.rect[2] <= region.rect[2] + 1.0
            && caret.rect[1] >= region.rect[1] - 1.0
            && caret.rect[3] <= region.rect[3] + 1.0,
        "the caret sits inside the field's region: caret {:?} in {:?}",
        caret.rect,
        region.rect
    );

    assert_eq!(session.position_at(region.page, 5.0, 5.0, 0.0), None);

    // One past the last character — the caret position while typing — sits at
    // the last glyph, not back at the paragraph's first.
    let text_len = TEXT.chars().count();
    let end = session.locate("body", text_len).expect("end-of-text caret");
    let last = session.locate("body", text_len - 1).expect("last-glyph caret");
    let first = session.locate("body", 0).expect("first-glyph caret");
    assert!(
        end.rect[0] >= last.rect[0] && end.rect[0] > first.rect[0],
        "end caret {:?} is at the last glyph {:?}, not the first {:?}",
        end.rect,
        last.rect,
        first.rect
    );
}

#[test]
fn locate_past_a_trailing_hard_break_holds_the_preceding_run() {
    // Shift+enter at the end of a paragraph: the segment's last content
    // character is the hard break, which lowers to `#linebreak()` and closes no
    // run, so the caret past it has only a preceding run to hold.
    use quillmark_content::model::{Content, Line, LineKind};
    let rt = Content::new(
        "Alpha beta\n".to_string(),
        vec![
            Line::new(LineKind::Para),
            Line::new(LineKind::Para).with_continues(true),
        ],
    )
    .into_normalized();
    assert_eq!(rt.validate(), Ok(()));
    let session = body_session(quillmark_content::serial::to_canonical_value(&rt));

    let first = session.locate("body", 0).expect("first-glyph caret");
    let last = session.locate("body", 10).expect("last-glyph caret");
    let end = session.locate("body", 11).expect("past-the-break caret");
    assert!(
        end.rect[0] >= last.rect[0] && end.rect[0] > first.rect[0],
        "the caret past the break is at the last glyph {:?}, not the paragraph's \
         first {:?}: {:?}",
        last.rect,
        first.rect,
        end.rect
    );
}

#[test]
fn position_at_on_a_raw_block_degrades_to_the_segment_start() {
    let session = body_session(content(
        "Intro prose here.\n\n```\nfirst code line\nsecond code line\nthird code line\n```",
    ));
    let body: Vec<_> = session
        .regions()
        .into_iter()
        .filter(|r| r.field == "body")
        .collect();
    assert_eq!(
        body.len(),
        2,
        "one prose segment, one code segment: {body:?}"
    );
    let (prose, code) = (&body[0], &body[1]);

    // Probe a few x offsets so a click lands on a glyph.
    let hit_at = |y: f32| {
        [2.0f32, 6.0, 12.0, 24.0, 48.0]
            .iter()
            .find_map(|dx| session.position_at(code.page, code.rect[0] + dx, y, 0.0))
    };
    let top = hit_at(code.rect[3] - 3.0).expect("a click on the first fence line resolves");
    let bottom = hit_at(code.rect[1] + 3.0).expect("a click on the last fence line resolves");
    assert_eq!(top.field, "body");
    assert_eq!(
        top.pos, bottom.pos,
        "different fence lines both degrade to the one code-segment start: {top:?} {bottom:?}"
    );
    for hit in [&top, &bottom] {
        assert_eq!(
            hit.granularity,
            Some(HitGranularity::Segment),
            "a multi-line fence hit floors to the segment: {hit:?}"
        );
    }
    let prose_hit = session
        .position_at(prose.page, prose.rect[0] + 5.0, prose.rect[3] - 3.0, 0.0)
        .expect("a click in the prose paragraph resolves");
    assert_ne!(
        prose_hit.pos, top.pos,
        "prose and the code fence are different segments: {prose_hit:?} {top:?}"
    );
}
