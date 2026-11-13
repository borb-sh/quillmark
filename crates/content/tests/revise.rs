//! A markdown revise's carry: each anchor rides the diff or a verbatim move
//! of its text, a kept `qm-anchor` tag pins it, and a kept `qm-island` token
//! keeps its island as stored.

use proptest::prelude::*;
use quillmark_content::diff::revise;
use quillmark_content::markdown::{ImportWarning, from_markdown, to_markdown_annotated};
use quillmark_content::model::{Anchor, Content, ISLAND_SLOT};
use quillmark_content::table::Table;

fn content(markdown: &str) -> Content {
    from_markdown(markdown).unwrap().content
}

fn with_anchor(markdown: &str, (start, end): (usize, usize), id: &str) -> Content {
    let mut c = content(markdown);
    c.anchors.push(Anchor::new(id, start, end));
    c
}

fn anchored(markdown: &str, start: usize, end: usize) -> Content {
    with_anchor(markdown, (start, end), "c1")
}

fn revised(base: &Content, markdown: &str) -> (Content, Vec<ImportWarning>) {
    let r = revise(base, markdown).unwrap();
    (r.content, r.warnings)
}

fn anchor_of(c: &Content, id: &str) -> Option<(usize, usize)> {
    c.anchor(id).map(|a| (a.start, a.end))
}

fn anchored_text(c: &Content) -> Option<(usize, String)> {
    let (start, end) = anchor_of(c, "c1")?;
    Some((
        start,
        c.text.chars().skip(start).take(end - start).collect(),
    ))
}

fn reported(warnings: &[ImportWarning]) -> Vec<(String, usize)> {
    warnings
        .iter()
        .map(|w| (w.construct.to_string(), w.count))
        .collect()
}

/// Deterministic filler with no long common substring between two offsets.
fn filler(n: usize, offset: u8) -> String {
    (0..n)
        .map(|i| char::from(b'a' + ((i as u8).wrapping_mul(7).wrapping_add(offset)) % 26))
        .collect()
}

#[test]
fn an_anchor_is_rehomed_on_a_block_move() {
    let base = anchored("first para here\n\nsecond para here", 0, 15);
    let (new, _) = revised(&base, "second para here\n\nfirst para here");
    assert_eq!(
        anchored_text(&new).map(|(_, t)| t),
        Some("first para here".into())
    );
}

#[test]
fn an_anchor_drops_when_its_text_is_deleted() {
    let base = anchored("keep this and drop that", 14, 23);
    let (new, _) = revised(&base, "keep this");
    assert_eq!(anchor_of(&new, "c1"), None);
}

/// The first paragraph is deleted and the second, holding the same word,
/// survives as retained text: the anchor drops and does not jump to it.
#[test]
fn an_anchor_is_not_rehomed_onto_an_unrelated_survivor() {
    let base = anchored("target one to drop\n\nkeep the target two", 0, 6);
    let (new, _) = revised(&base, "keep the target two");
    assert_eq!(anchor_of(&new, "c1"), None);
}

#[test]
fn an_anchor_survives_between_disjoint_edits() {
    let base = anchored("aaaMIDDLEbbb", 3, 9);
    let (new, _) = revised(&base, "AAAMIDDLEZZZ");
    assert_eq!(anchored_text(&new).map(|(_, t)| t), Some("MIDDLE".into()));
}

/// An edit to one word of a paragraph in a multi-paragraph body leaves an
/// anchor elsewhere in that paragraph where it was, however short its text
/// and wherever else that text recurs.
#[test]
fn an_edited_paragraph_keeps_its_anchors_in_place() {
    let base_md = "Intro paragraph.\n\nThe tax rate and the tax base rose in Q3.";
    let new_md = "Intro paragraph.\n\nThe tax rate and the tax base fell in Q3.";
    let para = "Intro paragraph.\n".chars().count();
    let second_tax = para + "The tax rate and the ".chars().count();
    let q3 = para + "The tax rate and the tax base rose in ".chars().count();

    let base = anchored(base_md, second_tax, second_tax + 3);
    let (new, _) = revised(&base, new_md);
    assert_eq!(anchored_text(&new), Some((second_tax, "tax".into())));

    let base = anchored(base_md, q3, q3 + 2);
    let (new, _) = revised(&base, new_md);
    assert_eq!(anchored_text(&new), Some((q3, "Q3".into())));
}

/// An edited paragraph and a paragraph inserted beside it: the edit still
/// pairs with the paragraph it rewrites.
#[test]
fn an_edited_paragraph_beside_an_inserted_one_keeps_its_anchor() {
    let base = anchored(
        "Lead.\n\nPay the fee by Friday.",
        6,
        6 + "Pay the fee".len(),
    );
    let (new, _) = revised(
        &base,
        "Lead.\n\nNew paragraph here.\n\nPay the fee by Monday.",
    );
    assert_eq!(
        anchored_text(&new).map(|(_, t)| t),
        Some("Pay the fee".into())
    );
}

/// Two different paragraphs swapping places is a move, not a rewrite: the
/// anchor follows its text rather than mapping into the other paragraph.
#[test]
fn swapped_paragraphs_are_moved_not_rewritten() {
    let base = anchored("alpha beta gamma\n\ndelta epsilon zeta", 0, 16);
    let (new, _) = revised(&base, "delta epsilon zeta\n\nalpha beta gamma");
    assert_eq!(
        anchored_text(&new).map(|(_, t)| t),
        Some("alpha beta gamma".into())
    );
}

/// The shape a whole-document rewrite hits: one long line rewritten past a
/// character diff's reach still carries an anchor sitting in shared text.
#[test]
fn a_large_single_line_rewrite_keeps_a_prefix_anchor() {
    let base = anchored(
        &format!("hello target world-{}-end", filler(30_000, 0)),
        6,
        12,
    );
    let new_markdown = format!("hello target world-{}-end", filler(30_000, 11));
    let (new, _) = revised(&base, &new_markdown);
    assert_eq!(anchored_text(&new).map(|(_, t)| t), Some("target".into()));
}

/// A kept tag pins: the anchor starts where the writer moved its tag and
/// ends where its text still ends on that line. A tag dropped falls back to
/// the diff, and a moved tag's anchor drops nothing where its text went.
#[test]
fn a_kept_tag_pins_and_a_dropped_tag_falls_back_to_the_diff() {
    let stored = with_anchor("Hello world, again.", (6, 11), "w");
    let read = to_markdown_annotated(&stored).markdown;
    assert_eq!(read, "Hello <qm-anchor ref=\"w\"></qm-anchor>world, again.");

    let (moved, warnings) = revised(
        &stored,
        "<qm-anchor ref=\"w\"></qm-anchor>Hello world, again.",
    );
    assert_eq!(anchor_of(&moved, "w"), Some((0, 11)));
    assert!(warnings.is_empty());

    let (later, _) = revised(
        &stored,
        "Hello world, <qm-anchor ref=\"w\"></qm-anchor>again.",
    );
    assert_eq!(
        anchor_of(&later, "w"),
        Some((13, 18)),
        "the stored length, cut at the line's end"
    );

    let (dropped, _) = revised(&stored, "Hello brave world, again.");
    assert_eq!(anchor_of(&dropped, "w"), Some((12, 17)));

    let (kept, _) = revised(&stored, &read);
    assert_eq!(kept, stored, "the read revises to itself");
}

/// A tag the read moved off its anchor's start, to a code span's start or
/// the line's end, pins nothing.
#[test]
fn a_tag_marked_at_pins_nothing() {
    let stored = with_anchor("Hello world.", (6, 11), "w");
    for at in ["line", "span"] {
        let md = format!("<qm-anchor at=\"{at}\" ref=\"w\"></qm-anchor>Hello world.");
        let (revised, warnings) = revised(&stored, &md);
        assert_eq!(anchor_of(&revised, "w"), Some((6, 11)), "{at}");
        assert!(warnings.is_empty());
    }
}

/// A tag naming no anchor the stored value holds, and a second tag naming
/// one, report its `ref`; a cold import drops either silently.
#[test]
fn a_ref_the_stored_value_does_not_hold_drops_reported() {
    let stored = with_anchor("Hello world.", (6, 11), "w");
    let md = "Hello <qm-anchor ref=\"w\"></qm-anchor>world<qm-anchor ref=\"w\"></qm-anchor>. \
              <qm-anchor ref=\"gone\"></qm-anchor><qm-anchor></qm-anchor>";
    let (revised, warnings) = revised(&stored, md);
    assert_eq!(anchor_of(&revised, "w"), Some((6, 11)));
    assert_eq!(anchor_of(&revised, "gone"), None);
    assert_eq!(reported(&warnings), [("qm-anchor[ref]".to_string(), 3)]);
    assert!(from_markdown(md).unwrap().warnings.is_empty());
}

/// An island markdown cannot spell reads as its token, and a revise keeps
/// it as stored wherever the writer leaves the token; a cold import drops
/// the token, reported.
#[test]
fn a_token_keeps_its_island_as_stored() {
    let mut stored = content("See ![a](u.png) here.");
    stored.islands[0].props["title"] = "kept".into();
    let read = to_markdown_annotated(&stored).markdown;
    assert_eq!(read, "See <qm-island ref=\"isl-0\"></qm-island> here.");

    let (revised, warnings) = revised(&stored, "Now <qm-island ref=\"isl-0\"></qm-island> there.");
    assert!(warnings.is_empty());
    assert_eq!(revised.text, format!("Now {ISLAND_SLOT} there."));
    assert_eq!(revised.islands, stored.islands);

    let cold = from_markdown(&read).unwrap();
    assert!(cold.content.islands.is_empty());
    assert_eq!(reported(&cold.warnings), [("qm-island".to_string(), 1)]);
}

/// A token naming no island the stored value holds reports its `ref`, and
/// one where no island sits reports itself. A table's token on a line of
/// prose takes a line of its own.
#[test]
fn a_token_drops_reported_where_it_cannot_be_kept() {
    let mut stored = content("| h |\n| --- |\n| c |");
    stored.islands[0].props["note"] = "kept".into();
    assert_eq!(
        to_markdown_annotated(&stored).markdown,
        "<qm-island ref=\"isl-0\"></qm-island>"
    );

    let md = "a <qm-island ref=\"isl-0\"></qm-island> b <qm-island ref=\"isl-9\"></qm-island>\n\n\
              | x |\n| --- |\n| <qm-island ref=\"isl-0\"></qm-island> |";
    let (revised, warnings) = revised(&stored, md);
    assert_eq!(revised.text, "a \n\u{FFFC}\n b \n\u{FFFC}");
    assert_eq!(revised.islands[0].props, stored.islands[0].props);
    assert_eq!(revised.islands[0].slots, stored.islands[0].slots);
    assert_eq!(
        reported(&warnings),
        [
            ("qm-island[ref]".to_string(), 1),
            ("qm-island".to_string(), 1)
        ]
    );
}

/// A token written twice copies its table, and its cell's anchor stays on
/// the copy that continues the stored island alone.
#[test]
fn a_token_written_twice_copies_no_cell_anchor() {
    let mut stored = content("| h |\n| --- |\n| cell |");
    stored.islands[0].props["note"] = "kept".into();
    let slot = Table::read(&stored.islands[0].props)
        .unwrap()
        .slot(1, 0)
        .unwrap()
        .to_string();
    let cell = stored.islands[0].slots.get_mut(&slot).unwrap();
    cell.anchors.push(Anchor::new("c1", 0, 4));
    let token = "<qm-island ref=\"isl-0\"></qm-island>";
    assert_eq!(to_markdown_annotated(&stored).markdown, token);

    let (once, _) = revised(&stored, token);
    assert_eq!(once.islands, stored.islands);
    let (twice, _) = revised(&stored, &format!("{token}\n\n{token}"));
    assert_eq!(twice.islands.len(), 2);
    assert_eq!(twice.anchor_ids(), ["c1"]);
}

/// A tag right after a table's token pins the line the split gives the
/// prose after it, not the table's.
#[test]
fn a_tag_after_a_tables_token_pins_the_prose_after_it() {
    let mut stored = with_anchor("Hello world.\n\n| h |\n| --- |\n| c |", (6, 11), "w");
    stored.islands[0].props["note"] = "kept".into();
    let read = to_markdown_annotated(&stored).markdown;
    let token = &read[read.find("<qm-island").unwrap()..];
    let (next, _) = revised(
        &stored,
        &format!("Hello {token}<qm-anchor ref=\"w\"></qm-anchor>world."),
    );
    assert_eq!(next.text, "Hello \n\u{FFFC}\nworld.");
    assert_eq!(anchor_of(&next, "w"), Some((9, 14)));
}

proptest! {
    /// An anchor over text a rewrite keeps is carried onto it.
    #[test]
    fn an_anchor_over_surviving_text_is_carried(a in "[a-z]{3,8}", b in "[a-z]{3,8}") {
        let base = anchored(&format!("keep {a} here"), 5, 5 + a.len());
        let (new, _) = revised(&base, &format!("{b} keep {a} here"));
        prop_assert_eq!(anchored_text(&new).map(|(_, t)| t), Some(a));
    }
}
