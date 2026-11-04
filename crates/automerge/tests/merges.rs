//! Example merges of two changes made from one value: what each side put
//! lands, an anchor or mark either side put keeps its edges over text the
//! other typed there as Automerge's expand rules place them, and a step by
//! id lands beside what the other side set.

use quillmark_automerge::merge3;
use quillmark_automerge::content::change::{AnchorStep, Change, ContainerStep, IslandStep, MarkStep, SetBlock, Splice, Step};
use quillmark_automerge::content::markdown::from_markdown;
use quillmark_automerge::content::model::{Anchor, BlockKind, Container, ContainerKind, Content, ISLAND_SLOT, Mark, MarkKind};
use quillmark_automerge::content::table::{Row, Table};
use serde_json::json;

fn table_change(d: &Content, edit: impl FnOnce(&mut Table), slots: &[(&str, &str)]) -> Change {
    let island = &d.islands[0];
    let mut grid = Table::read(&island.props).unwrap();
    edit(&mut grid);
    let mut props = island.props.as_object().unwrap().clone();
    grid.write_into(&mut props);
    let slots = slots.iter().map(|(k, v)| (k.to_string(), quillmark_automerge::content::markdown::from_plaintext(v))).collect();
    Change::new(vec![Step::Island(IslandStep::Props { id: island.id.clone(), props: props.into(), slots })])
}

fn slice(c: &Content, start: usize, end: usize) -> String {
    c.text.chars().skip(start).take(end - start).collect()
}

/// The text `c` holds under the anchor `id`.
fn under(c: &Content, id: &str) -> String {
    let a = c.anchor(id).unwrap();
    slice(c, a.start, a.end)
}

/// The text `c` holds under its one mark of `tag`.
fn marked(c: &Content, tag: &str) -> String {
    let m = c.marks.iter().find(|m| m.kind.tag() == tag).unwrap();
    slice(c, m.start, m.end)
}

/// Whether `held`, the text under an anchor or a mark put on `text`, is
/// that text with any of the text typed since at its two edges.
fn on(held: &str, before: &str, text: &str, after: &str) -> bool {
    [before, ""].iter().any(|b| [after, ""].iter().any(|a| held == format!("{b}{text}{a}")))
}

/// A range anchor or a mark one side put stays on its text, and the text
/// the other side typed since at its very edges lands inside or outside it
/// as Automerge orders the two sides' operations at that place; a point
/// anchor sticks to the character it sits before, so text typed at it lands
/// before it.
#[test]
fn a_slow_anchor_lands_on_its_text_over_live_typing() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "XX ")), Step::Splice(Splice::new(8, "", "!"))]);
    let place = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-2", 5, 5), quote: None }),
        Step::Mark(MarkStep::Add { mark: Mark::new(0, 5, MarkKind::Strong), quote: Some("Hello".into()) }),
    ]);
    for (ours, theirs) in [(&place, &typed), (&typed, &place)] {
        let done = merge3(&d, ours, theirs).unwrap().content;
        assert_eq!(done.text, "XX Hello! world");
        assert_eq!(done.anchor("c-1"), Some(&Anchor::new("c-1", 10, 15)));
        assert_eq!(done.anchor("c-2"), Some(&Anchor::new("c-2", 9, 9)));
        assert!(on(&marked(&done, "strong"), "XX ", "Hello", "!"), "{:?}", done.marks);
    }
}

#[test]
fn a_mark_keeps_its_text_where_both_sides_typed_beside_it() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(6, "", "XX"))]);
    let place = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Emph), quote: None }),
    ]);
    for (ours, theirs) in [(&place, &typed), (&typed, &place)] {
        let done = merge3(&d, ours, theirs).unwrap().content;
        assert_eq!(done.text, "Hello XXworld");
        assert!(on(&under(&done, "c-1"), "XX", "world", ""), "{:?}", done.anchor("c-1"));
        assert!(on(&marked(&done, "emph"), "XX", "world", ""), "{:?}", done.marks);
    }
}

#[test]
fn an_anchor_put_on_text_deleted_since_lands_nowhere() {
    let d = from_markdown("Hello brave world").unwrap().content;
    let cut = Change::new(vec![Step::Splice(Splice::new(5, " brave", ""))]);
    let place = Change::new(vec![
        Step::Splice(Splice::new(0, "", "Oh ")),
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 9, 14), quote: Some("brave".into()) }),
    ]);
    let done = merge3(&d, &place, &cut).unwrap().content;
    assert_eq!((done.text.as_str(), done.anchor("c-1")), ("Oh Hello world", None));
    let kept = merge3(&d, &place, &Change::new(vec![Step::Splice(Splice::new(0, "", "Yes, "))])).unwrap().content;
    assert_eq!(kept.anchor("c-1").map(|a| (a.start, a.end)), Some((14, 19)));
}

#[test]
fn a_diff_names_only_the_block_fields_it_changes_so_a_quote_and_a_heading_both_land() {
    let d = from_markdown("first\n\nsecond").unwrap().content;
    let id = d.blocks[1].id.clone();
    let mut heading = d.clone();
    heading.blocks[1].kind = BlockKind::Heading { level: 2 };
    let mine = quillmark_automerge::content::diff::diff(&d, &heading).unwrap();
    let quote = Change::new(vec![
        Step::Container(ContainerStep::Set(Container::new("q-n", ContainerKind::Quote))),
        Step::SetBlock(SetBlock { id, path: Some(vec!["q-n".into()]), ..SetBlock::default() }),
    ]);
    let done = merge3(&d, &mine, &quote).unwrap().content;
    assert_eq!((&done.blocks[1].kind, &done.blocks[1].path), (&BlockKind::Heading { level: 2 }, &vec!["q-n".to_string()]));
}

#[test]
fn rewording_the_word_beside_a_footnote_merges_with_an_edit_inside_the_note() {
    let d = from_markdown("Alpha[^a] beta gamma\n\n[^a]: note text here").unwrap().content;
    let island = d.islands[0].clone();
    let note = island.slot_order()[0].clone();
    let len = island.slots[&note].len_scalars();
    let over = Change::new(vec![Step::Nested { island: island.id.clone(), slot: note.clone(), change: Change::new(vec![Step::Splice(Splice::new(len, "", "!"))]) }]);
    let mine = quillmark_automerge::revise(&d, "Omega[^a] beta gamma\n\n[^a]: note text here").unwrap().change;
    let done = merge3(&d, &mine, &over).unwrap().content;
    assert!(done.text.starts_with("Omega"), "{:?}", done.text);
    assert_eq!(done.slot(&island.id, &note).unwrap().text, "note text here!");
}

#[test]
fn a_nested_step_merges_with_the_edits_inside_its_slot() {
    let d = from_markdown("| aa | bb |\n| - | - |\n| cc | dd |").unwrap().content;
    let island = d.islands[0].clone();
    let slots = island.slot_order();
    let cell = |slot: &str, s: Splice| Step::Nested { island: island.id.clone(), slot: slot.to_string(), change: Change::new(vec![Step::Splice(s)]) };
    let theirs = Change::new(vec![cell(&slots[2], Splice::new(0, "", "XY")), cell(&slots[3], Splice::new(0, "dd", "ee"))]);
    let mine = Change::new(vec![cell(&slots[2], Splice::new(2, "", "!")), cell(&slots[2], Splice::new(0, "c", "C"))]);
    let done = merge3(&d, &mine, &theirs).unwrap().content;
    let cc = &done.slot(&island.id, &slots[2]).unwrap().text;
    assert!(["XYCc!", "CXYc!"].contains(&cc.as_str()), "two inserts at one place land in actor order: {cc}");
    assert_eq!(done.slot(&island.id, &slots[3]).unwrap().text, "ee");
}

#[test]
fn revises_of_different_short_paragraphs_merge() {
    use quillmark_automerge::revise;
    let base = from_markdown("Alpha.\n\nBeta.\n\nGamma.").unwrap().content;
    for (theirs, ours, want) in [
        ("Alpha, edited.\n\nBeta.\n\nGamma.", "Alpha.\n\nBeta, mine.\n\nGamma.", "Alpha, edited.\nBeta, mine.\nGamma."),
        ("Alpha.\n\nBeta, edited.\n\nGamma.", "Alpha.\n\nBeta.\n\nGamma, mine.", "Alpha.\nBeta, edited.\nGamma, mine."),
        ("Something else entirely.\n\nBeta.\n\nGamma.", "Alpha.\n\nBeta, mine.\n\nGamma.", "Something else entirely.\nBeta, mine.\nGamma."),
    ] {
        let (since, mine) = (revise(&base, theirs).unwrap().change, revise(&base, ours).unwrap().change);
        assert_eq!(merge3(&base, &mine, &since).unwrap().content.text, want);
    }
    let since = revise(&base, "Alpha!\n\nBeta.\n\nGamma.").unwrap().change;
    let heading = Change::new(vec![Step::SetBlock(SetBlock { id: "b1".into(), kind: Some("heading".into()), attrs: json!({"level": 2}).as_object().cloned(), path: None })]);
    let done = merge3(&base, &heading, &since).unwrap().content;
    assert_eq!((done.text.as_str(), &done.blocks[1].kind), ("Alpha!\nBeta.\nGamma.", &BlockKind::Heading { level: 2 }));
}

#[test]
fn a_step_by_id_lands_beside_what_the_other_side_set() {
    let d = from_markdown("lead\n\n| aa | bb |\n| - | - |\n| cc | dd |\n\ntail").unwrap().content;
    let island = d.islands[0].clone();
    let grid = Table::read(&island.props).unwrap();
    let cell = |slot: &str| Change::new(vec![Step::Nested { island: island.id.clone(), slot: slot.into(), change: Change::new(vec![Step::Splice(Splice::new(2, "", "!"))]) }]);
    let (last, first) = (grid.rows[1].cells[0].clone(), grid.rows[0].cells[0].clone());
    let add_row = table_change(&d, |g| g.rows.push(Row { id: "r-n".into(), cells: vec!["s-n0".into(), "s-n1".into()] }), &[("s-n0", "new"), ("s-n1", "row")]);
    let add_column = table_change(
        &d,
        |g| {
            g.columns.push("k-n".into());
            g.rows.iter_mut().for_each(|r| r.cells.push(format!("{}k", r.id)));
        },
        &[],
    );
    let drop_last = table_change(&d, |g| { g.rows.pop(); }, &[]);
    for (mine, since) in [(&add_column, &add_row), (&drop_last, &cell(&first)), (&add_column, &cell(&last)), (&cell(&last), &add_column)] {
        merge3(&d, mine, since).unwrap();
    }
    let done = merge3(&d, &cell(&last), &add_column).unwrap().content;
    assert_eq!(done.slot(&island.id, &last).unwrap().text, "cc!");

    let at = d.text.chars().position(|ch| ch == ISLAND_SLOT).unwrap();
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "x"))]);
    let drop_table = Change::new(vec![Step::Splice(Splice::new(at - 1, "\n\u{FFFC}", ""))]);
    assert_eq!(merge3(&d, &drop_table, &typed).unwrap().content.text, "xlead\ntail");

    let tail = d.blocks.last().unwrap().id.clone();
    let set = |level: u8| Change::new(vec![Step::SetBlock(SetBlock { id: tail.clone(), kind: Some("heading".into()), attrs: json!({"level": level}).as_object().cloned(), path: None })]);
    let quote = Change::new(vec![
        Step::Container(ContainerStep::Set(Container::new("q-n", ContainerKind::Quote))),
        Step::SetBlock(SetBlock { id: tail.clone(), path: Some(vec!["q-n".into()]), ..SetBlock::default() }),
    ]);
    for (mine, since) in [(&set(2), &quote), (&quote, &set(2))] {
        let done = merge3(&d, mine, since).unwrap().content;
        let b = done.blocks.last().unwrap();
        assert_eq!((&b.kind, &b.path), (&BlockKind::Heading { level: 2 }, &vec!["q-n".to_string()]));
    }
}

#[test]
fn text_typed_at_both_edges_of_a_mark_put_since_lands_beside_it() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(6, "", "XX")), Step::Splice(Splice::new(13, "", "YY"))]);
    let put = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Emph), quote: None }),
        Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Link { url: "https://x.io".into() }), quote: None }),
    ]);
    for (ours, theirs) in [(&typed, &put), (&put, &typed)] {
        let done = merge3(&d, ours, theirs).unwrap().content;
        assert_eq!(done.text, "Hello XXworldYY");
        assert!(on(&under(&done, "c-1"), "XX", "world", "YY"), "{:?}", done.anchor("c-1"));
        for tag in ["emph", "link"] {
            assert!(on(&marked(&done, tag), "XX", "world", "YY"), "{tag}: {:?}", done.marks);
        }
    }
}

#[test]
fn a_reword_merges_with_typing_elsewhere() {
    let d = from_markdown("Hello world, again").unwrap().content;
    let mine = quillmark_automerge::revise(&d, "Hello World, again").unwrap().change;
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "Oh, "))]);
    let done = merge3(&d, &mine, &typed).unwrap();
    assert_eq!(done.content.text, "Oh, Hello World, again");
}

/// Two peers rewriting one rich text value nested in a field, an
/// `array<richtext>` element, merge into a value that reads as valid
/// content, in either merge order.
#[test]
fn nested_rich_text_both_sides_rewrote_reads_valid() {
    use quillmark_automerge::model::{Doc, cards};
    let c = |md: &str| from_markdown(md).unwrap().content.to_value();
    let main = cards::Card { fields: vec![("refs".into(), json!([c("*first* ref"), c("second")]))], ..Default::default() };
    let origin = cards::build(&main, &[]).save();
    let wrote = |md: &str| {
        let mut d = Doc::load(&origin).unwrap();
        let (was, _) = cards::read(&d).unwrap();
        let mut now = was.clone();
        now.fields[0].1[0] = c(md);
        cards::write_one(&mut d, true, &was, &now);
        d
    };
    let (mut a, mut b) = (wrote("*a*\n\nb\n\nc\n\nd"), wrote("*first* ref and *more words here*"));
    let (sa, sb) = (a.save(), b.save());
    for (d, other) in [(&mut a, &sb), (&mut b, &sa)] {
        d.merge(other).unwrap();
        let (read, _) = cards::read(d).unwrap();
        let refs = read.fields[0].1.as_array().unwrap();
        for r in refs {
            Content::from_value(r).unwrap();
        }
    }
}
