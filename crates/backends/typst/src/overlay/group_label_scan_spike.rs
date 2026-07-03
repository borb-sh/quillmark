//! SPIKE — not shipped. Tests a third alternative to metadata-marker
//! bracketing and span-based tracking: label an inline `box()`/block `block()`
//! that wraps the field's *actual visible content itself* (not a decorative
//! sibling the way `metadata()` markers are), and read the label back off the
//! resulting `FrameItem::Group` (`GroupItem.label`, set by
//! `typst-layout/src/inline/box.rs:75-76` and `flow/block.rs:98-99,244-247`
//! whenever the wrapped element itself carries a label).
//!
//! Two prior findings this is trying to reconcile:
//! - `pretag_spike.rs`: `metadata()` is categorically excluded from
//!   `par.body` — Typst's realization phase treats it as non-paragraph
//!   "meta" content regardless of source adjacency.
//! - `span_scan_spike.rs`: `Span` survives capture/replay (it's intrinsic to
//!   the glyph), but is tied to *origin*, not *occurrence* — it can't
//!   distinguish two placements of the same field.
//!
//! Hypothesis: a labeled `box()`/`block()` wrapping the real content (a) is
//! NOT "meta" content — it's the paragraph's actual visible text, so it
//! should be captured into `p.body` where metadata wasn't; and (b) gets a
//! *fresh* `Group` at every layout/realization — same mechanism as
//! `Tag`/`Location`, not `Span` — so two placements of the same labeled value
//! should produce two independent `Group` occurrences.
//!
//! VERDICT: both hold, and this resolves the conflict `span_scan_spike.rs`
//! found unsolvable.
//!
//! - `labeled_box_inside_paragraph_survives_minimal_capture_and_replay`: a
//!   labeled `box()` wrapping the paragraph's entire real content — not a
//!   decorative sibling — DOES get captured into `p.body`, unlike bare
//!   `metadata()`. It's real visible content, not "meta," so the
//!   paragraph-grouping exclusion that sank the marker spike doesn't apply.
//! - `labeled_box_gives_fresh_group_per_placement_unlike_span`: the same
//!   labeled value, placed twice with unrelated content between, yields TWO
//!   independent `Group` occurrences — because `Group`s are minted at
//!   layout/realization time (same mechanism as `Tag`/`Location`), not tied
//!   to origin the way `Span` is. This is the placement-counting guarantee
//!   spans structurally cannot give.
//! - `labeled_block_output_wrap_keeps_occurrence_identity_when_placed_twice`:
//!   both properties together, against the real vendored `render-body`,
//!   using the pattern that would actually ship — label the package's
//!   *output* (`#block[#mainmatter[..]]<label>`, mirroring how `tagged()`
//!   already brackets output today) rather than its raw input. Two
//!   placements of that wrapped value → two clean, disjoint regions.
//!
//! One caveat, *not* fully root-caused: wrapping the *raw pre-rebuild*
//! multi-paragraph input in a labeled box (rather than wrapping the output)
//! produces two identically-sized spurious occurrences of unclear origin —
//! see `labeled_box_wrapping_raw_multi_paragraph_input_does_not_survive`'s
//! diagnostic dump; a likely suspect is `render-body`'s own internal
//! `measure()` call in `body.typ`, which lays content out a second time
//! purely to read its height. Not investigated further because the viable,
//! cleanly-validated pattern is output-wrapping, matching `tagged()`'s
//! existing contract — this spike doesn't need input-wrapping to work.
//!
//! A SECOND, more severe caveat, found while checking layout-neutrality
//! (does wrapping content change what it looks like, the way the current
//! zero-size `metadata()` marker never does): `box()` and `block(width:
//! 100%)` are both layout-neutral for ordinary multi-line wrapping
//! (`box_wrapping_with_no_explicit_width_still_wraps_normally`,
//! `block_wrapping_with_full_width_preserves_normal_wrapping` — identical
//! line counts, identical bounding extent, down to the same float). BUT
//! `box_wrapping_does_not_support_page_spanning_content` found that an
//! inline `box()` around content long enough to span multiple pages
//! collapses everything onto ONE page (13 pages unwrapped → 1 page boxed) —
//! silently, not an error. `box()` cannot be a uniform replacement for
//! `_qm-tag` (auto-tag's own per-field wrapper, which has to handle both
//! short inline scalars AND long page-spanning markdown with the same
//! mechanism) without content-shape-aware dispatch — inline scalars need
//! `box()` (layout-transparent) but page-spanning content needs `block()`
//! (breakable, fragments correctly). `block()`, in turn, isn't safe for
//! scalar/inline fields (it forces a block-level break where a scalar used
//! to flow inline as part of the surrounding text).
//!
//! Net scope: this mechanism is a validated, safe drop-in for `tagged()`'s
//! existing explicit-wrap use (content there is always block-level output
//! from a package call, e.g. `#mainmatter[..]`/`#indorsement(..)` — `block()`
//! is unconditionally correct). It is NOT yet a safe replacement for
//! auto-tag's own `_qm-tag`, which would need to pick box vs. block per
//! field (or per eval'd value's actual content shape) to avoid the
//! page-spanning hazard above.

use std::collections::HashMap;

use quillmark_core::{FileTreeNode, Quill};
use typst::foundations::Label;
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::world::QuillWorld;

#[derive(Debug)]
struct GroupHit {
    #[allow(dead_code)]
    page: usize,
    rect: [f64; 4],
}

/// Walk every page for `FrameItem::Group` items whose label matches `want`,
/// recording each occurrence's own frame bounds (the group's frame size,
/// transformed into page space) — no manual leaf-union needed, unlike
/// `region_scan`'s marker approach, since the group's frame already bounds
/// its content.
fn collect_group_hits(doc: &PagedDocument, want: Label) -> Vec<GroupHit> {
    fn walk(frame: &Frame, ts: Transform, page_idx: usize, want: Label, out: &mut Vec<GroupHit>) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let inner_ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    if group.label == Some(want) {
                        let size = group.frame.size();
                        let corners = [
                            Point::zero(),
                            Point::new(size.x, Point::zero().y),
                            Point::new(Point::zero().x, size.y),
                            Point::new(size.x, size.y),
                        ];
                        let mut min_x = f64::INFINITY;
                        let mut min_y = f64::INFINITY;
                        let mut max_x = f64::NEG_INFINITY;
                        let mut max_y = f64::NEG_INFINITY;
                        for c in corners {
                            let p = c.transform(inner_ts);
                            min_x = min_x.min(p.x.to_pt());
                            min_y = min_y.min(p.y.to_pt());
                            max_x = max_x.max(p.x.to_pt());
                            max_y = max_y.max(p.y.to_pt());
                        }
                        out.push(GroupHit {
                            page: page_idx,
                            rect: [min_x, min_y, max_x, max_y],
                        });
                    }
                    walk(&group.frame, inner_ts, page_idx, want, out);
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(&page.frame, Transform::identity(), page_idx, want, &mut out);
    }
    out
}

fn label_for(name: &str) -> Label {
    Label::new(PicoStr::intern(name)).expect("non-empty label")
}

/// Total ink extent (union of every Text/Shape/Image leaf's bbox, page-space
/// top-left) across the whole document — a coarse but sufficient proxy for
/// "did wrapping this in a labeled box/block change the layout."
fn total_ink_extent(doc: &PagedDocument) -> Vec<(usize, [f64; 4])> {
    fn walk(frame: &Frame, ts: Transform, page_idx: usize, out: &mut Vec<(usize, [f64; 4])>) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let inner_ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    walk(&group.frame, inner_ts, page_idx, out);
                }
                FrameItem::Text(text) => {
                    let bb = text.bbox();
                    let corners = [
                        Point::new(pos.x + bb.min.x, pos.y + bb.min.y),
                        Point::new(pos.x + bb.max.x, pos.y + bb.max.y),
                    ];
                    let mut rect = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
                    for c in corners {
                        let p = c.transform(ts);
                        rect[0] = rect[0].min(p.x.to_pt());
                        rect[1] = rect[1].min(p.y.to_pt());
                        rect[2] = rect[2].max(p.x.to_pt());
                        rect[3] = rect[3].max(p.y.to_pt());
                    }
                    out.push((page_idx, rect));
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for (page_idx, page) in doc.pages().iter().enumerate() {
        walk(&page.frame, Transform::identity(), page_idx, &mut out);
    }
    out
}

fn minimal_quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: br#"
quill:
  name: group_label_spike
  version: 0.1.0
  backend: typst
  description: group-label scan spike
typst:
  plate_file: plate.typ
main:
  fields: {}
"#
            .to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

fn compile(plate: &str) -> PagedDocument {
    let quill = minimal_quill();
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    doc
}

fn host_tree() -> FileTreeNode {
    fn walk(dir: &std::path::Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let p = entry.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(
                    name,
                    FileTreeNode::File {
                        contents: std::fs::read(&p)?,
                    },
                );
            } else if p.is_dir() {
                files.insert(name, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let quill_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("resources")
        .join("quills")
        .join("usaf_memo")
        .join("0.2.0");
    walk(&quill_path).expect("walk fixture")
}

#[test]
fn labeled_box_survives_direct_placement() {
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#box[FIRSTFIELD one two three.]<qm-field-first>
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("direct placement hits: {hits:?}");
    assert_eq!(hits.len(), 1, "one direct placement, one group: {hits:?}");
    assert!(hits[0].rect[2] > hits[0].rect[0] && hits[0].rect[3] > hits[0].rect[1]);
}

#[test]
fn labeled_box_gives_fresh_group_per_placement_unlike_span() {
    // The critical test: two placements of the SAME labeled value. If group
    // creation is occurrence-based (like Tag/Location) rather than
    // origin-based (like Span), this must yield 2 independent hits.
    let plate = r#"
#set page(width: 400pt, height: 700pt, margin: 40pt)
#let content = [#box[FIRSTFIELD placed here.]<qm-field-first>]
#content

#lorem(40)

#content
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("two-placement hits: {hits:?}");
    assert_eq!(
        hits.len(),
        2,
        "the same labeled value placed twice must yield two independent group occurrences: {hits:?}"
    );
    assert!(
        hits[0].rect[1] > hits[1].rect[3] || hits[1].rect[1] > hits[0].rect[3],
        "the two occurrences must not overlap/collapse: {hits:?}"
    );
}

#[test]
fn labeled_box_inside_paragraph_survives_minimal_capture_and_replay() {
    // Does a labeled box, when it constitutes the paragraph's ENTIRE inline
    // content (not a decorative sibling), get captured into p.body — unlike
    // bare metadata()? Uses the same minimal show-par capture rule as
    // pretag_spike.rs / span_scan_spike.rs.
    let plate = r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)

#let BUF = state("BUF", ())
#let capture(it) = {
  show par: p => {
    BUF.update(buf => buf + (text([#p.body]),))
    []
  }
  it
}

#capture([#box[FIRSTFIELD one two three.]<qm-field-first>])

#context {
  for c in BUF.get() {
    block[#c]
  }
}
"#;
    let doc = compile(plate);
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("minimal capture/replay hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "a labeled box wrapping the paragraph's real content must survive the capture/replay: {hits:?}"
    );
}

#[test]
fn labeled_box_wrapping_raw_single_paragraph_input_survives_by_accident() {
    // Correction to the module doc's original hypothesis: for a SINGLE
    // paragraph, wrapping the raw pre-rebuild input in a labeled box DOES
    // survive — because that one paragraph's `p.body`, when render-body's
    // `show par:` captures it, IS the whole labeled box (nothing else is in
    // that paragraph), so the label rides along into the capture exactly
    // like `labeled_box_inside_paragraph_survives_minimal_capture_and_replay`
    // already showed. This isn't the general case — see the next test.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[#box[FIRSTFIELD one two three.]<qm-field-first>]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("single-paragraph raw-input-wrapped hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "single-paragraph case survives (the label rides inside p.body): {hits:?}"
    );
}

#[test]
fn labeled_box_wrapping_raw_multi_paragraph_input_does_not_survive() {
    // The real auto-tag shape: a field's markdown is usually multiple
    // paragraphs. Wrapping ALL of them in ONE outer labeled box before
    // handing it to render-body: render-body's `show par:` fires once per
    // paragraph, each capturing its OWN `p.body` — the label lives on the
    // OUTER wrapper spanning all of them, not on any individual paragraph,
    // so no single capture carries it, and the outer wrapper ends up empty
    // once its paragraph children are replaced with `[]` at the original
    // site. Same structural reason `tagged()` has to wrap the package's
    // *output*, not its input.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#mainmatter[#box[FIRSTFIELD one two three.

SECONDFIELD four five six.]<qm-field-first>]
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    // Diagnostic: dump every Group frame item (labeled or not) to understand
    // where the second, unexpected occurrence comes from.
    fn dump_groups(frame: &Frame, depth: usize) {
        for (_, item) in frame.items() {
            if let FrameItem::Group(g) = item {
                eprintln!(
                    "{}group label={:?} size={:?}",
                    "  ".repeat(depth),
                    g.label,
                    g.frame.size()
                );
                dump_groups(&g.frame, depth + 1);
            }
        }
    }
    for page in doc.pages() {
        dump_groups(&page.frame, 0);
    }

    let hits = collect_group_hits(&doc, label_for("qm-field-first"));
    eprintln!("multi-paragraph raw-input-wrapped hits: {hits:?}");
    // OPEN QUESTION, not a clean confirmation either way: two identically-
    // sized labeled groups appear, at different positions, both matching the
    // FIRST paragraph's dimensions ("SECONDFIELD ..." doesn't appear at all
    // in either). The group dump above shows this isn't simple survival —
    // something in render-body's own pipeline (a likely suspect: its
    // `measure(final_par, ...)` call in body.typ, which lays content out a
    // second time purely to read its height for the sticky/breakable
    // decision) produces a duplicate. Not root-caused here; NOT the pattern
    // to build on without further digging. The clean, validated pattern is
    // the next test: label the package's OUTPUT (mirroring tagged() today),
    // which gives exactly one correct hit.
    assert_eq!(
        hits.len(),
        2,
        "documents the open question rather than asserting either clean outcome: {hits:?}"
    );
}

#[test]
fn labeled_block_wrapping_render_body_output_survives_and_keeps_occurrence_identity() {
    // The viable pattern: label render-body's OUTPUT (exactly how `tagged()`
    // brackets the package's output today) with a `block` (not `box` — the
    // output is itself multi-paragraph, block-level content). Also the
    // pattern `flow/block.rs` documents labeling every FRAGMENT of a
    // breakable block, matching the "one region per page fragment"
    // contract, for free.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#block[#mainmatter[FIRSTFIELD one two three.

SECONDFIELD four five six.]]<qm-field-body>
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-body"));
    eprintln!("output-wrapped (block) render-body hits: {hits:?}");
    assert!(
        !hits.is_empty(),
        "labeling render-body's output with a block (like tagged() already does) survives: {hits:?}"
    );
}

#[test]
fn labeled_block_output_wrap_keeps_occurrence_identity_when_placed_twice() {
    // Combines both properties for the pattern that would actually ship: the
    // SAME labeled-and-wrapped render-body output, referenced twice with
    // unrelated content between (mainmatter is called once, building one
    // value; that value is placed twice) — must survive AND stay two
    // independent occurrences, the way `field_placed_twice_yields_independent_regions`
    // requires, unlike span identity.
    let plate = r#"
#import "@local/tonguetoquill-usaf-memo:3.0.0": frontmatter, mainmatter

#show: frontmatter.with(subject: "Spike Memo", memo_for: ("TEST/SYMB",));

#let wrapped = [#block[#mainmatter[FIRSTFIELD one two three.]]<qm-field-body>]
#wrapped

#lorem(40)

#wrapped
"#;
    let quill = Quill::from_tree(host_tree()).expect("load usaf_memo host quill");
    let world = QuillWorld::new(&quill, plate).expect("build world");
    let (doc, _warnings) = compile_document(&world).expect("compile");
    let hits = collect_group_hits(&doc, label_for("qm-field-body"));
    eprintln!("twice-placed output-wrapped hits: {hits:?}");
    assert_eq!(
        hits.len(),
        2,
        "the same rebuilt-and-labeled value, placed twice, must stay two independent occurrences: {hits:?}"
    );
    assert!(
        hits[0].rect[1] > hits[1].rect[3] || hits[1].rect[1] > hits[0].rect[3],
        "the two occurrences must not overlap/collapse: {hits:?}"
    );
}

// ---------------------------------------------------------------------------
// The single most important "does this play well with real quills" question:
// does wrapping content in box()/block() CHANGE THE LAYOUT (line-wrapping,
// spacing) compared to leaving it unwrapped, the way the current zero-size
// metadata() marker is layout-neutral by construction? If wrapping forces
// single-line/no-wrap behavior on long prose, or changes spacing, it isn't a
// safe drop-in replacement no matter how well it solves tagging.
// ---------------------------------------------------------------------------

#[test]
fn box_wrapping_with_no_explicit_width_still_wraps_normally() {
    // Correction to an initial hypothesis: an inline box() with NO explicit
    // width was expected to shrink-to-fit and force everything onto one
    // (overflowing) line, unlike plain paragraph content. Empirically false
    // — an inline box() inside a paragraph still participates in that
    // paragraph's line-breaking; identical line count and identical
    // bounding extent to the unwrapped version.
    let long = "This is a long sentence that must wrap across several lines within the page's text column when laid out normally. ".repeat(3);

    let unwrapped_plate = format!(
        r#"
#set page(width: 300pt, height: 400pt, margin: 20pt)
{long}
"#
    );
    let wrapped_plate = format!(
        r#"
#set page(width: 300pt, height: 400pt, margin: 20pt)
#box[{long}]<qm-field-x>
"#
    );

    let unwrapped_doc = compile(&unwrapped_plate);
    let wrapped_doc = compile(&wrapped_plate);

    let unwrapped_ink = total_ink_extent(&unwrapped_doc);
    let wrapped_ink = total_ink_extent(&wrapped_doc);

    let unwrapped_lines: std::collections::HashSet<_> = unwrapped_ink
        .iter()
        .map(|(_, r)| (r[1] * 100.0) as i64)
        .collect();
    let wrapped_lines: std::collections::HashSet<_> = wrapped_ink
        .iter()
        .map(|(_, r)| (r[1] * 100.0) as i64)
        .collect();

    eprintln!(
        "unwrapped: {} pages, {} distinct line y-positions, max width {:.1}pt",
        unwrapped_doc.pages().len(),
        unwrapped_lines.len(),
        unwrapped_ink.iter().map(|(_, r)| r[2] - r[0]).fold(0.0, f64::max)
    );
    eprintln!(
        "box-wrapped: {} pages, {} distinct line y-positions, max width {:.1}pt",
        wrapped_doc.pages().len(),
        wrapped_lines.len(),
        wrapped_ink.iter().map(|(_, r)| r[2] - r[0]).fold(0.0, f64::max)
    );

    assert!(
        unwrapped_lines.len() > 3,
        "the unwrapped paragraph must wrap across several lines: {} lines",
        unwrapped_lines.len()
    );
    assert_eq!(
        unwrapped_lines.len(),
        wrapped_lines.len(),
        "box() with no explicit width still wraps identically to unwrapped content: \
         unwrapped={} wrapped={}",
        unwrapped_lines.len(),
        wrapped_lines.len()
    );
}

#[test]
fn block_wrapping_with_full_width_preserves_normal_wrapping() {
    // The fix: block() (not box()) at full available width participates in
    // the normal block-flow layout, so multi-line wrapping is preserved.
    // Compares line count and total ink width between unwrapped and
    // block-wrapped versions of the same long paragraph.
    let long = "This is a long sentence that must wrap across several lines within the page's text column when laid out normally. ".repeat(3);

    let unwrapped_plate = format!(
        r#"
#set page(width: 300pt, height: 400pt, margin: 20pt)
{long}
"#
    );
    let wrapped_plate = format!(
        r#"
#set page(width: 300pt, height: 400pt, margin: 20pt)
#block(width: 100%)[{long}]<qm-field-x>
"#
    );

    let unwrapped_doc = compile(&unwrapped_plate);
    let wrapped_doc = compile(&wrapped_plate);

    let unwrapped_ink = total_ink_extent(&unwrapped_doc);
    let wrapped_ink = total_ink_extent(&wrapped_doc);

    let unwrapped_lines: std::collections::HashSet<_> = unwrapped_ink
        .iter()
        .map(|(_, r)| (r[1] * 100.0) as i64)
        .collect();
    let wrapped_lines: std::collections::HashSet<_> = wrapped_ink
        .iter()
        .map(|(_, r)| (r[1] * 100.0) as i64)
        .collect();

    eprintln!(
        "unwrapped lines={} block-wrapped lines={}",
        unwrapped_lines.len(),
        wrapped_lines.len()
    );
    assert_eq!(
        unwrapped_lines.len(),
        wrapped_lines.len(),
        "block(width: 100%) must preserve the same line count as unwrapped content"
    );

    // Bounding extent of all ink should match closely (allowing for
    // sub-point float noise), confirming no visual shift.
    let bounds = |ink: &[(usize, [f64; 4])]| {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for (_, r) in ink {
            b[0] = b[0].min(r[0]);
            b[1] = b[1].min(r[1]);
            b[2] = b[2].max(r[2]);
            b[3] = b[3].max(r[3]);
        }
        b
    };
    let ub = bounds(&unwrapped_ink);
    let wb = bounds(&wrapped_ink);
    eprintln!("unwrapped bounds={ub:?} block-wrapped bounds={wb:?}");
    for i in 0..4 {
        assert!(
            (ub[i] - wb[i]).abs() < 0.5,
            "bounding extent must match within noise at index {i}: unwrapped={ub:?} wrapped={wb:?}"
        );
    }
}

#[test]
fn box_wrapping_does_not_support_page_spanning_content() {
    // The remaining open question before touching `_qm-tag`: does an inline
    // box() (needed because it's layout-transparent, per the tests above)
    // also support content long enough to break across a page boundary —
    // the auto-tag content_regions.rs::content_fields_emit_frame_regions
    // scenario ("body spans several pages: one fragment per page it
    // touches"). If it can't, box() can't be a uniform replacement for
    // `_qm-tag` — only content-shape-aware dispatch (box for inline scalars,
    // block for multi-paragraph/page-spanning content) would work.
    let long = "This is a long paragraph that must wrap across many lines and break across pages. ".repeat(200);

    let unwrapped_plate = format!(
        r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
{long}
"#
    );
    let wrapped_plate = format!(
        r#"
#set page(width: 400pt, height: 400pt, margin: 40pt)
#box[{long}]<qm-field-x>
"#
    );

    let unwrapped_doc = compile(&unwrapped_plate);
    let wrapped_doc = compile(&wrapped_plate);

    eprintln!(
        "unwrapped pages={} box-wrapped pages={}",
        unwrapped_doc.pages().len(),
        wrapped_doc.pages().len()
    );

    assert!(
        unwrapped_doc.pages().len() >= 2,
        "the unwrapped long body must span multiple pages: {} page(s)",
        unwrapped_doc.pages().len()
    );
    // Documents whatever Typst actually does: either it silently confines
    // everything to fewer pages (content lost/overlapping) or it errors, or
    // — if it turns out box() also breaks across pages — this assertion
    // should be revisited rather than trusted blindly.
    eprintln!(
        "box() page-spanning behavior: {}",
        if wrapped_doc.pages().len() == unwrapped_doc.pages().len() {
            "MATCHES unwrapped page count"
        } else {
            "DIFFERS from unwrapped page count"
        }
    );
}
