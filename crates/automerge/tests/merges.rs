//! Example merges of two changes made from one value: what each side put
//! lands, an anchor or mark either side put keeps its edges over text the
//! other typed there whichever actor sorts first, and a step by id conflicts
//! only where the other side set or removed what it names.

use quillmark_automerge::{Held, Reason, conflict, deleted, merge3, merge3_checked};
use quillmark_content::change::{AnchorStep, Change, ContainerStep, IslandStep, MarkStep, SetBlock, Splice, Step, apply, apply_traced};
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{Anchor, BlockKind, Container, ContainerKind, Content, ISLAND_SLOT, Mark, MarkKind};
use quillmark_content::table::{Row, Table};
use serde_json::json;

/// `ours` and `theirs` made from `base`, merged as a stale write merges:
/// `theirs` written under actor `b`, `ours` on a fork at `base` under `a`,
/// then held to its edges.
fn merged(base: &Content, ours: &Change, theirs: &Change, a: &str, b: &str) -> Content {
    let (mine, trace) = apply_traced(base, ours).unwrap();
    let (since, since_trace) = apply_traced(base, theirs).unwrap();
    let mut held = Held::new(base, "");
    let heads = held.heads();
    held.set_actor(Some(b));
    held.write(&since_trace, &since.content);
    let mut fork = held.fork_at(&heads, Some(a)).unwrap();
    fork.write(&trace, &mine.content);
    let (at_mine, at_theirs) = (fork.heads(), held.heads());
    held.merge(&mut fork).unwrap();
    held.clip(&at_mine, &at_theirs, &trace);
    held.read().content
}

fn both_orders(base: &Content, ours: &Change, theirs: &Change) -> Content {
    let [first, second] = [("a", "z"), ("z", "a")].map(|(a, b)| merged(base, ours, theirs, a, b));
    assert_eq!(first, second, "the merge depends on which actor sorts first");
    first
}

fn table_change(d: &Content, edit: impl FnOnce(&mut Table), slots: &[(&str, &str)]) -> Change {
    let island = &d.islands[0];
    let mut grid = Table::read(&island.props).unwrap();
    edit(&mut grid);
    let mut props = island.props.as_object().unwrap().clone();
    grid.write_into(&mut props);
    let slots = slots.iter().map(|(k, v)| (k.to_string(), quillmark_content::markdown::from_plaintext(v))).collect();
    Change::new(vec![Step::Island(IslandStep::Props { id: island.id.clone(), props: props.into(), slots })])
}

#[test]
fn a_slow_anchor_lands_on_its_text_over_live_typing() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "XX ")), Step::Splice(Splice::new(8, "", "!"))]);
    let place = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-2", 5, 5), quote: None }),
        Step::Mark(MarkStep::Add { mark: Mark::new(0, 5, MarkKind::Strong), quote: Some("Hello".into()) }),
    ]);
    let done = both_orders(&d, &place, &typed);
    assert_eq!(done.text, "XX Hello! world");
    assert_eq!(done.anchor("c-1"), Some(&Anchor::new("c-1", 10, 15)));
    assert_eq!(done.anchor("c-2"), Some(&Anchor::new("c-2", 8, 8)));
    assert_eq!(done.marks, [Mark::new(3, 8, MarkKind::Strong)]);
}

#[test]
fn a_mark_keeps_its_edges_where_both_sides_typed_beside_it() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(6, "", "XX"))]);
    let place = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Emph), quote: None }),
    ]);
    let done = both_orders(&d, &place, &typed);
    assert_eq!(done.text, "Hello XXworld");
    assert_eq!(done.anchor("c-1"), Some(&Anchor::new("c-1", 8, 13)));
    assert_eq!(done.marks, [Mark::new(8, 13, MarkKind::Emph)]);
}

#[test]
fn an_anchor_put_on_text_deleted_since_is_named() {
    let d = from_markdown("Hello brave world").unwrap().content;
    let cut = Change::new(vec![Step::Splice(Splice::new(5, " brave", ""))]);
    let place = Change::new(vec![
        Step::Splice(Splice::new(0, "", "Oh ")),
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 9, 14), quote: Some("brave".into()) }),
    ]);
    let (mine, _) = apply_traced(&d, &place).unwrap();
    let done = merge3(&d, &place, &cut).unwrap().content;
    assert_eq!((done.text.as_str(), done.anchor("c-1")), ("Oh Hello world", None));
    assert_eq!(deleted(&mine.applied, &mine.content, &done).map(|c| (c.step, c.reason)), Some((1, Reason::Deleted)));
    let kept = merge3(&d, &place, &Change::new(vec![Step::Splice(Splice::new(0, "", "Yes, "))])).unwrap().content;
    assert_eq!(deleted(&mine.applied, &mine.content, &kept), None);
}

#[test]
fn a_diff_names_only_the_block_fields_it_changes_so_a_quote_and_a_heading_both_land() {
    let d = from_markdown("first\n\nsecond").unwrap().content;
    let id = d.blocks[1].id.clone();
    let mut heading = d.clone();
    heading.blocks[1].kind = BlockKind::Heading { level: 2 };
    let mine = quillmark_content::diff::diff(&d, &heading).unwrap();
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
fn a_step_by_id_conflicts_only_where_the_other_set_or_removed_what_it_names() {
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
    let refused = |since: &Change, mine: &Change| conflict(&d, &apply(&d, since).unwrap().content, mine).map(|c| (c.step, c.reason));
    assert_eq!(refused(&add_row, &add_column), Some((0, Reason::Changed)));
    for (mine, since) in [(&drop_last, &cell(&first)), (&add_column, &cell(&last)), (&cell(&last), &add_column)] {
        assert_eq!(refused(since, mine), None);
        merge3(&d, mine, since).unwrap();
    }
    let done = merge3(&d, &cell(&last), &add_column).unwrap().content;
    assert_eq!(done.slot(&island.id, &last).unwrap().text, "cc!");

    let at = d.text.chars().position(|ch| ch == ISLAND_SLOT).unwrap();
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "x"))]);
    let drop_table = Change::new(vec![Step::Splice(Splice::new(at - 1, "\n\u{FFFC}", ""))]);
    assert_eq!(refused(&drop_table, &typed), None);
    assert_eq!(merge3(&d, &drop_table, &typed).unwrap().content.text, "xlead\ntail");

    let tail = d.blocks.last().unwrap().id.clone();
    let set = |level: u8| Change::new(vec![Step::SetBlock(SetBlock { id: tail.clone(), kind: Some("heading".into()), attrs: json!({"level": level}).as_object().cloned(), path: None })]);
    let quote = Change::new(vec![
        Step::Container(ContainerStep::Set(Container::new("q-n", ContainerKind::Quote))),
        Step::SetBlock(SetBlock { id: tail.clone(), path: Some(vec!["q-n".into()]), ..SetBlock::default() }),
    ]);
    assert_eq!(refused(&set(2), &set(3)), Some((0, Reason::Changed)));
    for (mine, since) in [(&set(2), &quote), (&quote, &set(2))] {
        assert_eq!(refused(since, mine), None);
        let done = merge3(&d, mine, since).unwrap().content;
        let b = done.blocks.last().unwrap();
        assert_eq!((&b.kind, &b.path), (&BlockKind::Heading { level: 2 }, &vec!["q-n".to_string()]));
    }
    let unquote = Change::new(vec![Step::SetBlock(SetBlock { id: tail, path: Some(Vec::new()), ..SetBlock::default() })]);
    assert_eq!(refused(&quote, &unquote), Some((0, Reason::Changed)));
}

#[test]
fn text_typed_at_the_edge_of_a_mark_put_since_takes_no_part_in_it() {
    let d = from_markdown("Hello world").unwrap().content;
    let typed = Change::new(vec![Step::Splice(Splice::new(6, "", "XX")), Step::Splice(Splice::new(13, "", "YY"))]);
    let put = Change::new(vec![
        Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-1", 6, 11), quote: Some("world".into()) }),
        Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Emph), quote: None }),
    ]);
    let done = both_orders(&d, &typed, &put);
    assert_eq!(done.text, "Hello XXworldYY");
    assert_eq!(done.anchor("c-1"), Some(&Anchor::new("c-1", 8, 13)));
    assert_eq!(done.marks, [Mark::new(8, 13, MarkKind::Emph)]);
}

#[test]
fn a_change_deleting_text_formatted_since_is_named_and_one_elsewhere_is_not() {
    let d = from_markdown("Hello world, again").unwrap().content;
    let link = Change::new(vec![Step::Mark(MarkStep::Add { mark: Mark::new(6, 11, MarkKind::Link { url: "https://x.io".into() }), quote: None })]);
    let mine = quillmark_automerge::revise(&d, "Hello World, again").unwrap().change;
    let (_, refused) = merge3_checked(&d, &mine, &link).unwrap();
    assert_eq!(refused.map(|c| c.reason), Some(Reason::Changed));
    let typed = Change::new(vec![Step::Splice(Splice::new(0, "", "Oh, "))]);
    let (done, refused) = merge3_checked(&d, &mine, &typed).unwrap();
    assert_eq!((done.content.text.as_str(), refused), ("Oh, Hello World, again", None));
}
