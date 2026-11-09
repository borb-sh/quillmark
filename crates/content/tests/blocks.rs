//! Block ops addressed by id: each keeps every id it does not name, and an id
//! no block holds is refused.

use quillmark_content::blocks::{BlockOp, batch, blocks, change};
use quillmark_content::change::{ApplyError, apply};
use quillmark_content::markdown::{from_markdown, to_markdown};
use quillmark_content::mint::fill;
use quillmark_content::model::Content;

fn md(s: &str) -> Content {
    from_markdown(s).unwrap().content
}

const DOC: &str =
    "# Title\n\nAlpha **one**.\n\n- item a\n- item b\n  - nested c\n- item d\n\nOmega.";

fn ids(c: &Content) -> Vec<String> {
    c.blocks.iter().map(|b| b.id.clone()).collect()
}

fn id_of(c: &Content, text: &str) -> String {
    blocks(c)
        .into_iter()
        .find(|b| {
            c.text
                .chars()
                .skip(b.start)
                .take(b.end - b.start)
                .collect::<String>()
                == text
        })
        .unwrap()
        .id
}

fn run(c: &Content, ops: Vec<BlockOp>) -> Content {
    let b = batch(c, &ops).unwrap();
    let out = apply(c, &b.change).unwrap().content;
    out.validate().unwrap();
    out
}

#[test]
fn the_read_lists_each_block_with_its_markdown() {
    let c = md(DOC);
    let read: Vec<(String, String)> = blocks(&c)
        .into_iter()
        .map(|b| (b.kind, b.markdown))
        .collect();
    assert_eq!(read[0], ("heading".into(), "# Title".into()));
    assert_eq!(read[1].1, "Alpha **one**.");
    assert_eq!(read[2].1, "- item a");
    assert_eq!(read[4].1, "  - nested c");
}

#[test]
fn replace_keeps_the_id_and_every_other() {
    let c = md(DOC);
    let alpha = id_of(&c, "Alpha one.");
    let out = run(
        &c,
        vec![BlockOp::Replace {
            id: alpha.clone(),
            with: md("Alpha _two_."),
        }],
    );
    assert_eq!(ids(&out), ids(&c));
    assert_eq!(
        to_markdown(&out),
        to_markdown(&c).replace("Alpha **one**.", "Alpha *two*.")
    );
}

#[test]
fn a_list_item_replaced_by_a_list_item_stays_the_item() {
    let c = md(DOC);
    let b = id_of(&c, "item b");
    let out = run(
        &c,
        vec![BlockOp::Replace {
            id: b,
            with: md("- item **B**"),
        }],
    );
    assert_eq!(ids(&out), ids(&c));
    assert_eq!(
        to_markdown(&out),
        to_markdown(&c).replace("item b", "item **B**")
    );
}

#[test]
fn a_replace_into_several_blocks_opens_ids_after_the_first() {
    let c = md(DOC);
    let alpha = id_of(&c, "Alpha one.");
    let b = batch(
        &c,
        &[BlockOp::Replace {
            id: alpha.clone(),
            with: md("First.\n\nSecond."),
        }],
    )
    .unwrap();
    let out = apply(&c, &b.change).unwrap().content;
    assert_eq!(id_of(&out, "First."), alpha);
    assert_eq!(b.opened, vec![vec![id_of(&out, "Second.")]]);
}

#[test]
fn insert_after_an_item_lands_a_sibling_past_its_nested_blocks() {
    let c = md(DOC);
    let b = id_of(&c, "item b");
    let out = run(
        &c,
        vec![BlockOp::InsertAfter {
            id: b,
            with: md("item new"),
        }],
    );
    assert_eq!(
        to_markdown(&out),
        to_markdown(&c).replace("  - nested c\n\n", "  - nested c\n\n- item new\n\n")
    );
    let out = run(
        &c,
        vec![BlockOp::InsertAfter {
            id: id_of(&c, "item d"),
            with: md("- x\n- y"),
        }],
    );
    assert!(to_markdown(&out).contains("- item d\n\n- x\n\n- y"));
}

#[test]
fn insert_after_a_paragraph_lands_a_list_as_spelled() {
    let c = md(DOC);
    let out = run(
        &c,
        vec![BlockOp::InsertAfter {
            id: id_of(&c, "Omega."),
            with: md("| a | b |\n|---|---|\n| 1 | 2 |"),
        }],
    );
    assert!(
        to_markdown(&out).ends_with("Omega.\n\n| a | b |\n| --- | --- |\n| 1 | 2 |"),
        "{}",
        to_markdown(&out)
    );
}

#[test]
fn delete_retires_the_id_and_a_later_op_on_it_fails_loudly() {
    let c = md(DOC);
    let alpha = id_of(&c, "Alpha one.");
    let out = run(&c, vec![BlockOp::Delete { id: alpha.clone() }]);
    assert!(!ids(&out).contains(&alpha));
    assert_eq!(
        to_markdown(&out),
        to_markdown(&c).replace("Alpha **one**.\n\n", "")
    );
    let err = batch(
        &c,
        &[
            BlockOp::Delete { id: alpha.clone() },
            BlockOp::Replace {
                id: alpha.clone(),
                with: md("x"),
            },
        ],
    )
    .unwrap_err();
    assert_eq!(err, (1, ApplyError::UnknownBlock { id: alpha }));
}

#[test]
fn deleting_an_items_only_block_drops_the_item() {
    let c = md(DOC);
    let out = run(
        &c,
        vec![BlockOp::Delete {
            id: id_of(&c, "item d"),
        }],
    );
    assert_eq!(
        to_markdown(&out),
        to_markdown(&c).replace("- item d\n\n", "")
    );
}

#[test]
fn deleting_the_first_block_renames_the_next_and_the_batch_follows() {
    let c = md(DOC);
    let first = c.blocks[0].id.clone();
    let alpha = id_of(&c, "Alpha one.");
    let b = batch(
        &c,
        &[
            BlockOp::Delete { id: first.clone() },
            BlockOp::Replace {
                id: alpha.clone(),
                with: md("Moved."),
            },
        ],
    )
    .unwrap();
    let out = apply(&c, &b.change).unwrap().content;
    assert_eq!(b.renamed.get(&alpha), Some(&first));
    assert_eq!(id_of(&out, "Moved."), first);
    assert!(to_markdown(&out).starts_with("Moved.\n\n- item a"));
}

#[test]
fn a_split_and_a_join_keep_the_left_id_under_block_ops() {
    let c = md(DOC);
    let alpha = id_of(&c, "Alpha one.");
    let split =
        quillmark_content::structural::split_block(&c, c.block_ranges()[1].0 + 5, None).unwrap();
    let split = fill(&split, &[&c]).unwrap();
    let after = apply(&c, &split.change).unwrap().content;
    assert_eq!(id_of(&after, "Alpha"), alpha);
    let right = id_of(&after, " one.");
    let joined = quillmark_content::structural::join_blocks(&after, &right).unwrap();
    let joined = apply(&after, &joined).unwrap().content;
    assert_eq!(id_of(&joined, "Alpha one."), alpha);
    assert_eq!(
        change(&joined, &BlockOp::Delete { id: right.clone() }).unwrap_err(),
        ApplyError::UnknownBlock { id: right }
    );
}

/// Every op on every block of a value with lists, a quote, a table and a code
/// block lands a valid value, and every id the op does not name holds.
#[test]
fn every_op_on_every_block_keeps_the_ids_it_does_not_name() {
    let c = md(
        "# T\n\n> quoted\n> - in a list\n\n1. one\n2. two\n   1. deep\n\n```rs\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n\nEnd.",
    );
    let withs = [
        "Plain **text**.",
        "- an item",
        "A.\n\nB.",
        "## Head",
        "| x |\n|---|\n| y |",
    ];
    for target in ids(&c) {
        let mut ops = vec![BlockOp::Delete { id: target.clone() }];
        for w in withs {
            ops.push(BlockOp::Replace {
                id: target.clone(),
                with: md(w),
            });
            ops.push(BlockOp::InsertAfter {
                id: target.clone(),
                with: md(w),
            });
        }
        for op in ops {
            let b =
                batch(&c, std::slice::from_ref(&op)).unwrap_or_else(|e| panic!("{op:?}: {e:?}"));
            let out = apply(&c, &b.change).unwrap().content;
            out.validate().unwrap_or_else(|e| panic!("{op:?}: {e:?}"));
            let held = ids(&out);
            for id in ids(&c) {
                let named = id == target && matches!(op, BlockOp::Delete { .. });
                if !named && !b.renamed.contains_key(&id) {
                    assert!(held.contains(&id), "{op:?} lost {id}");
                }
            }
        }
    }
}

/// A block id a write mints is a function of the value it lands on, and a
/// retired one does not come back on a later write.
#[test]
fn a_retired_block_id_is_not_minted_again() {
    let c = md("One.\n\nTwo.\n\nThree.");
    let last = c.blocks[2].id.clone();
    let gone = run(&c, vec![BlockOp::Delete { id: last.clone() }]);
    let op = BlockOp::InsertAfter {
        id: gone.blocks[1].id.clone(),
        with: md("Three again."),
    };
    let a = batch(&gone, std::slice::from_ref(&op)).unwrap();
    assert_eq!(a, batch(&gone, std::slice::from_ref(&op)).unwrap());
    let back = apply(&gone, &a.change).unwrap().content;
    assert_ne!(id_of(&back, "Three again."), last);
}

/// A markdown revise mints a block it cannot align the same way.
#[test]
fn a_revise_does_not_mint_a_retired_block_id_again() {
    let c = md("One.\n\nTwo.\n\nThree.");
    let last = c.blocks[2].id.clone();
    let r1 = quillmark_content::diff::revise(&c, "One.\n\nTwo.").unwrap();
    let r2 = quillmark_content::diff::revise(&r1.content, "One.\n\nTwo.\n\nFour.").unwrap();
    assert_eq!(&ids(&r2.content)[..2], &ids(&c)[..2]);
    assert_ne!(id_of(&r2.content, "Four."), last);
}
