//! Regions read off the laid-out frames — span-tracked content, scalar
//! reference sites, the `form-field` `field:` binding — and the forward
//! `field_at`/`position_at`/`locate` navigation over them.

use quillmark_core::Backend;
use quillmark_typst::TypstBackend;

mod common;
use common::{content, quill_with_plate as quill};

#[test]
fn content_fields_emit_frame_regions() {
    const YAML: &str = r#"
quill:
  name: content_regions
  version: 0.1.0
  backend: typst
  description: content region span-tracking test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: a short intro paragraph
    body:
      type: richtext
      description: a long body that wraps and breaks across pages
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.intro

#data.body
"#;

    // Long enough to overflow page 0 and continue.
    let long = "This is a markdown paragraph that wraps across several lines. ".repeat(200);
    let data = serde_json::json!({
        "intro": content("A **short** intro paragraph on the first page."),
        "body": content(&long),
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    let mut sorted_pages = pages.clone();
    sorted_pages.sort();
    sorted_pages.dedup();
    assert_eq!(pages, sorted_pages, "fragments in page order, one per page");
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
    const YAML: &str = r#"
quill:
  name: two_placements
  version: 0.1.0
  backend: typst
  description: first-placement region + click-to-field test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: a short paragraph placed twice
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro

#lorem(40)

#data.intro
"#;
    let data = serde_json::json!({ "intro": content("The same intro, placed twice.") });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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

    let cx = (first.rect[0] + first.rect[2]) / 2.0;
    let cy = (first.rect[1] + first.rect[3]) / 2.0;
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

#[test]
fn scalar_reference_sites_each_surface_a_region() {
    const YAML: &str = r#"
quill:
  name: scalar_sites
  version: 0.1.0
  backend: typst
  description: scalar reference-site region test
main:
  fields:
    subject:
      type: string
      description: a plain scalar referenced at two sites
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

*#data.subject*

#lorem(20)

#data.at("subject")
"#;
    let data = serde_json::json!({ "subject": "Request for Quarters" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        2,
        "each scalar reference site surfaces independently: {regions:?}"
    );
    for r in &subject {
        assert!(r.rect[2] > r.rect[0], "positive width: {:?}", r.rect);
    }
    // Bottom-left origin: the later site sits lower.
    assert!(
        subject[0].rect[1] > subject[1].rect[3] || subject[1].rect[1] > subject[0].rect[3],
        "sites do not union: {:?} vs {:?}",
        subject[0].rect,
        subject[1].rect
    );
}

#[test]
fn widget_and_tracked_content_both_surface_widget_ordered_first() {
    // The order is `SessionHandle::regions`'s contract.
    const YAML: &str = r#"
quill:
  name: widget_and_content
  version: 0.1.0
  backend: typst
  description: widget-before-content ordering test
main:
  fields:
    signature_block:
      type: string
      description: bound to both a widget and a scalar reference site
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, signature-field
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.signature_block
#signature-field("Signature", field: "signature_block", width: 137pt)
"#;
    let data = serde_json::json!({ "signature_block": "FIRST M. LAST, Rank, USAF" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let matches: Vec<_> = regions
        .iter()
        .filter(|r| r.field == "signature_block")
        .collect();
    assert_eq!(
        matches.len(),
        2,
        "a widget-bound field with tracked content surfaces both: {regions:?}"
    );

    // The widget is a fixed-size box at the test-owned 137pt width; the
    // content region is the ink of the placed string, distinguishing which
    // entry is which without relying on a `source` field the type doesn't
    // carry.
    assert!(
        (matches[0].rect[2] - matches[0].rect[0] - 137.0).abs() < 0.01,
        "the widget region sorts first: {regions:?}"
    );
}

#[test]
fn content_survives_a_rebuilding_show_rule() {
    const YAML: &str = r#"
quill:
  name: rebuild_survival
  version: 0.1.0
  backend: typst
  description: content-rebuild span survival test
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a body piped through a capture-and-replay package shape
"#;
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
    let data = serde_json::json!({ "body": content("A body paragraph the package rebuilds.") });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    assert!(
        regions.iter().any(|r| r.field == "body"),
        "a rebuilt body still surfaces a region, with no explicit tagging: {regions:?}"
    );
}

#[test]
fn markdown_array_elements_surface_indexed_regions() {
    const YAML: &str = r#"
quill:
  name: array_regions
  version: 0.1.0
  backend: typst
  description: richtext[] element region test
main:
  fields:
    refs:
      type: array
      items:
        type: richtext
      description: a richtext[] field
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#for r in data.refs {
  block(r)
}
"#;
    let data =
        serde_json::json!({ "refs": [content("First reference."), content("Second reference.")] });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    const YAML: &str = r#"
quill:
  name: card_regions
  version: 0.1.0
  backend: typst
  description: card region path test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: a top-level intro
card_kinds:
  alpha:
    description: alpha card
    fields:
      note:
        type: richtext
        description: alpha note
  beta:
    description: beta card
    fields:
      note:
        type: richtext
        description: beta note
"#;
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
    let data = serde_json::json!({
        "intro": content("Top-level intro."),
        "$cards": [
            {"$kind": "alpha", "note": content("Alpha one.")},
            {"$kind": "beta",  "note": content("Beta one.")},
            {"$kind": "alpha", "note": content("Alpha two.")},
        ],
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    const YAML: &str = r#"
quill:
  name: date_region
  version: 0.1.0
  backend: typst
  description: date content-projection region test
typst:
  plate_file: plate.typ
main:
  fields:
    issued:
      type: date
      description: the memo date
"#;
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
    let data = serde_json::json!({ "issued": "2026-01-02" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
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
    const YAML: &str = r#"
quill:
  name: card_date_region
  version: 0.1.0
  backend: typst
  description: per-instance card date region test
typst:
  plate_file: plate.typ
card_kinds:
  stamp:
    description: a dated stamp
    fields:
      on:
        type: date
        description: the stamp date
"#;
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
    let data = serde_json::json!({
        "$cards": [
            {"$kind": "stamp", "on": "2026-01-02"},
            {"$kind": "stamp"},                       // blank date → none
            {"$kind": "stamp", "on": "2028-03-04"},
        ],
    });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let fields: std::collections::HashSet<String> =
        session.regions().into_iter().map(|r| r.field).collect();
    assert!(
        fields.contains("$cards.stamp.0.on"),
        "first card's date regions per-instance: {fields:?}"
    );
    assert!(
        fields.contains("$cards.stamp.2.on"),
        "third card's date regions per-instance: {fields:?}"
    );
    // A `none` date draws no ink: absent, not a zero-area box.
    assert!(
        !fields.contains("$cards.stamp.1.on"),
        "a blank card date surfaces no region: {fields:?}"
    );
    let regions = session.regions();
    let first = regions
        .iter()
        .find(|r| r.field == "$cards.stamp.0.on")
        .expect("first card date region present");
    let cx = (first.rect[0] + first.rect[2]) / 2.0;
    let cy = (first.rect[1] + first.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(first.page, cx, cy, 0.0).as_deref(),
        Some("$cards.stamp.0.on"),
        "a click on a laundered card date routes to its per-instance schema path"
    );
}

#[test]
fn form_field_unknown_path_fails_the_compile() {
    // A typo'd path is a loud compile error, not a silent no-region widget.
    const YAML: &str = r#"
quill:
  name: widget_typo
  version: 0.1.0
  backend: typst
  description: form-field path validation test
typst:
  plate_file: plate.typ
main:
  fields:
    subject:
      type: string
      description: the only schema field
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#form-field("S", type: "text", value: "x", field: "subjcet")
"#;
    let err = TypstBackend
        .open(
            &quill(YAML, PLATE),
            &serde_json::json!({ "subject": "typo'd" }),
        )
        .err()
        .expect("a typo'd field binding must fail the compile");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("subjcet"),
        "the compile error names the bad path: {msg}"
    );
}

#[test]
fn failed_update_keeps_serving_last_good_regions() {
    // A failed compile has already written the next injection's helper source
    // into the world, but the served document's spans must keep resolving
    // against the compile they came from.
    const YAML: &str = r#"
quill:
  name: failed_update_regions
  version: 0.1.0
  backend: typst
  description: transactional regions test
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: richtext
      description: a paragraph
    when:
      type: date
      description: a date the template parses at data-assembly time
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#data.intro
"#;
    let good = serde_json::json!({
        "intro": content("A stable paragraph the session keeps serving."),
        "when": "2026-07-03",
    });
    let mut session = TypstBackend.open(&quill(YAML, PLATE), &good).expect("open");
    let before = session.regions();
    assert!(
        before.iter().any(|r| r.field == "intro"),
        "baseline intro region: {before:?}"
    );

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
    let intro = before.iter().find(|r| r.field == "intro").unwrap();
    let cx = (intro.rect[0] + intro.rect[2]) / 2.0;
    let cy = (intro.rect[1] + intro.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(intro.page, cx, cy, 0.0).as_deref(),
        Some("intro"),
        "clicks keep resolving against the served compile"
    );
}

#[test]
fn continuation_fragments_survive_page_marginals() {
    const YAML: &str = r#"
quill:
  name: marginal_fragments
  version: 0.1.0
  backend: typst
  description: continuation fragments under page chrome
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a long body under headers and footers
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(
  width: 612pt,
  height: 300pt,
  margin: 60pt,
  header: [Running Header],
  footer: [Running Footer],
)
#set text(size: 11pt)

#data.body
"#;
    let long = "A paragraph that wraps and flows across pages. ".repeat(120);
    let data = serde_json::json!({ "body": content(&long) });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let body: Vec<_> = regions.iter().filter(|r| r.field == "body").collect();
    assert!(
        body.len() >= 2,
        "page chrome must not truncate the placement to its first page: {body:?}"
    );
    let pages: Vec<usize> = body.iter().map(|r| r.page).collect();
    assert_eq!(pages[0], 0);
    assert!(
        pages.windows(2).all(|w| w[1] == w[0] + 1),
        "fragments cover consecutive pages: {pages:?}"
    );
}

#[test]
fn wrapped_scalar_expression_attributes_to_its_field() {
    const YAML: &str = r#"
quill:
  name: wrapped_scalar
  version: 0.1.0
  backend: typst
  description: wrapped scalar attribution test
main:
  fields:
    subject:
      type: string
      description: a scalar shown through a wrapping call
typst:
  plate_file: plate.typ
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)

#upper(data.subject)
"#;
    let data = serde_json::json!({ "subject": "request for quarters" });

    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let regions = session.regions();
    let subject: Vec<_> = regions.iter().filter(|r| r.field == "subject").collect();
    assert_eq!(
        subject.len(),
        1,
        "a single-reference wrapping expression attributes to the field: {regions:?}"
    );
    let cx = (subject[0].rect[0] + subject[0].rect[2]) / 2.0;
    let cy = (subject[0].rect[1] + subject[0].rect[3]) / 2.0;
    assert_eq!(
        session.field_at(subject[0].page, cx, cy, 0.0).as_deref(),
        Some("subject"),
        "clicks on the wrapped ink route to the field"
    );
}

#[test]
fn form_field_path_rejected_when_address_tables_are_empty() {
    // Empty address tables validate against the empty set: every address
    // rejects. Only absent tables are permissive.
    const YAML: &str = r#"
quill:
  name: empty_tables
  version: 0.1.0
  backend: typst
  description: empty-tables-vs-absent-meta guard test
typst:
  plate_file: plate.typ
main:
  body:
    enabled: false
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#form-field("S", type: "text", value: "x", field: "subject")
"#;
    let err = TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
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
    // value must never produce source that fails to parse. Two edges the string
    // shape of the codegen could hide but a real compile catches:
    //   - an unterminated `<u>` yields unbalanced `#underline[` markup that, in
    //     a `[ .. ]` content block, would break the whole helper file;
    //   - `i64::MIN` cannot be a Typst int literal (its magnitude overflows).
    const YAML: &str = r#"
quill:
  name: adversarial_codegen
  version: 0.1.0
  backend: typst
  description: codegen robustness
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a body that may carry malformed inline markup
"#;
    // The plate's assert proves the int round-tripped.
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#assert(data.at("n") == -9223372036854775807 - 1)
#data.body
"#;
    let data = serde_json::json!({
        "body": content("Please <u>sign here"),
        "n": i64::MIN,
    });
    // Compile success is the assertion.
    TypstBackend
        .open(&quill(YAML, PLATE), &data)
        .expect("adversarial data (unterminated <u>, i64::MIN) must still compile");
}

/// `aaa` is painted first, `zzz` on top of it; a click in the shared box must
/// resolve to `zzz`, not to the alphabetically-first `/T` name.
#[test]
fn overlapping_widgets_resolve_field_at_by_paint_order_not_name() {
    const YAML: &str = r#"
quill:
  name: overlap_widgets
  version: 0.1.0
  backend: typst
  description: overlapping widget hit-test
typst:
  plate_file: plate.typ
main:
  fields:
    aaa_early:
      type: string
      description: alphabetically first, painted first (underneath)
    zzz_late:
      type: string
      description: alphabetically last, painted last (on top)
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": form-field
#set page(width: 300pt, height: 200pt, margin: 0pt)
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("aaa", type: "checkbox", field: "aaa_early", width: 40pt, height: 40pt))
#place(top + left, dx: 60pt, dy: 60pt,
  form-field("zzz", type: "checkbox", field: "zzz_late", width: 40pt, height: 40pt))
"#;
    let session = TypstBackend
        .open(&quill(YAML, PLATE), &serde_json::json!({}))
        .expect("open");
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
    let cx = (z.rect[0] + z.rect[2]) / 2.0;
    let cy = (z.rect[1] + z.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(z.page, cx, cy, 0.0).as_deref(),
        Some("zzz_late"),
        "the later-painted widget wins the click, not the alphabetically-first name"
    );
}

#[test]
fn segment_regions_carry_span_and_field_union_is_striped() {
    const YAML: &str = r#"
quill:
  name: segment_regions
  version: 0.1.0
  backend: typst
  description: per-segment span regions + striped union
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a two-paragraph body
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.body
"#;
    let data = serde_json::json!({
        "body": content("First paragraph, alpha.\n\nSecond paragraph, beta."),
    });
    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    const YAML: &str = r#"
quill:
  name: nav_round_trip
  version: 0.1.0
  backend: typst
  description: position_at / locate round trip
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: one paragraph
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.body
"#;
    let data = serde_json::json!({ "body": content("Alpha beta gamma delta epsilon.") });
    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
    let body: Vec<_> = session
        .regions()
        .into_iter()
        .filter(|r| r.field == "body")
        .collect();
    assert_eq!(body.len(), 1, "one paragraph, one region: {body:?}");
    let region = &body[0];
    let span = region.span.expect("content region carries a span");

    let cx = region.rect[0] + 5.0;
    let cy = region.rect[3] - 3.0;
    let hit = session
        .position_at(region.page, cx, cy, 0.0)
        .expect("a click inside content resolves to a content position");
    assert_eq!(hit.field, "body");
    assert!(
        span[0] <= hit.pos && hit.pos <= span[1],
        "pos {} within span {span:?}",
        hit.pos
    );
    assert_eq!(
        hit.granularity,
        Some(quillmark_core::HitGranularity::Cluster),
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
    let text_len = "Alpha beta gamma delta epsilon.".chars().count();
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
    const YAML: &str = r#"
quill:
  name: nav_hard_break
  version: 0.1.0
  backend: typst
  description: caret past a trailing hard break
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: one paragraph closed by a hard break
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.body
"#;
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
    let data = serde_json::json!({ "body": quillmark_content::serial::to_canonical_value(&rt) });
    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");

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
    const YAML: &str = r#"
quill:
  name: raw_degrade
  version: 0.1.0
  backend: typst
  description: raw block segment-start degrade
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a paragraph plus a multi-line code fence
"#;
    const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 11pt)

#data.body
"#;
    let data = serde_json::json!({
        "body": content("Intro prose here.\n\n```\nfirst code line\nsecond code line\nthird code line\n```"),
    });
    let session = TypstBackend.open(&quill(YAML, PLATE), &data).expect("open");
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
    assert_eq!(
        top.granularity,
        Some(quillmark_core::HitGranularity::Segment),
        "a multi-line fence hit floors to the segment: {top:?}"
    );
    assert_eq!(bottom.granularity, Some(quillmark_core::HitGranularity::Segment));
    let prose_hit = session
        .position_at(prose.page, prose.rect[0] + 5.0, prose.rect[3] - 3.0, 0.0)
        .expect("a click in the prose paragraph resolves");
    assert_ne!(
        prose_hit.pos, top.pos,
        "prose and the code fence are different segments: {prose_hit:?} {top:?}"
    );
}
