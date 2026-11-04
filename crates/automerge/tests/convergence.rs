//! The convergence laws: two changes made from one value merge as Automerge
//! merges them into one valid value, a change merged with nothing is itself
//! in text and structure, with its marks and anchors where Automerge's
//! expand rules put them, changes to text either side of a character neither
//! touches keep the text each wrote, and what one side set by id the other
//! left alone stays set. `CASES` raises the case count.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::merge3;
use quillmark_content::change::{Change, SetBlock, Step, apply};
use quillmark_content::model::Content;

fn block<'a>(c: &'a Content, id: &str) -> Option<&'a quillmark_content::model::Block> {
    c.blocks.iter().find(|b| b.id == id)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases("CASES", 48), ..ProptestConfig::default() })]

    #[test]
    fn two_changes_from_one_value_merge_into_a_valid_value(d in document(), mine in ops(), theirs in ops()) {
        let (c, o) = (change_for(&d, &mine), change_from(&d, &theirs, 500));
        let merged = merge3(&d, &c, &o).unwrap();
        prop_assert_eq!(merged.content.validate(), Ok(()));
    }

    #[test]
    fn a_change_merged_with_nothing_is_itself(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let done = shape(&apply(&d, &c).unwrap().content);
        prop_assert_eq!(shape(&merge3(&d, &c, &Change::default()).unwrap().content), done.clone());
        prop_assert_eq!(shape(&merge3(&d, &Change::default(), &c).unwrap().content), done);
    }

    /// The text left of the character as the one wrote it, the text right of
    /// it as the other did, whichever is written first.
    #[test]
    fn changes_to_disjoint_text_merge_into_both(d in document(), cut in any::<usize>(), mine in ops(), theirs in ops()) {
        let len = d.len_scalars();
        let cut = cut % len;
        let c = windowed(&d, &mine, (0, cut), "l");
        let o = windowed(&d, &theirs, (cut + 1, len), "r");
        let (left, right) = (apply(&d, &c).unwrap().content, apply(&d, &o).unwrap().content);
        let both = merge3(&d, &c, &o).unwrap().content;
        let mine: String = chars(&left)[..left.len_scalars() - (len - cut)].iter().collect();
        let theirs: String = chars(&right)[cut..].iter().collect();
        prop_assert_eq!(&both.text, &format!("{mine}{theirs}"));
        prop_assert_eq!(&merge3(&d, &o, &c).unwrap().content.text, &both.text);
    }

    /// A block kind one change set stays where the other neither sets that
    /// block's record nor removes it nor puts it back, and the merge needed
    /// no repair.
    #[test]
    fn a_kind_one_side_set_the_other_left_alone_stays(d in document(), mine in ops(), theirs in ops()) {
        let (c, o) = (change_for(&d, &mine), change_from(&d, &theirs, 500));
        let merged = merge3(&d, &c, &o).unwrap();
        prop_assume!(merged.repairs.is_empty());
        let (ours, theirs) = (apply(&d, &c).unwrap().content, apply(&d, &o).unwrap().content);
        let set: Vec<&str> = c.steps.iter().flat_map(|s| match s {
            Step::SetBlock(SetBlock { id, kind: Some(_), .. }) | Step::SetBlock(SetBlock { id, attrs: Some(_), .. }) => vec![id.as_str()],
            Step::Splice(sp) => sp.blocks.iter().map(|b| b.id.as_str()).collect(),
            _ => Vec::new(),
        }).collect();
        for b in &d.blocks {
            let (Some(t), Some(m), Some(_)) = (block(&theirs, &b.id), block(&merged.content, &b.id), block(&ours, &b.id)) else { continue };
            if t.kind != b.kind && !set.contains(&b.id.as_str()) {
                prop_assert_eq!(&m.kind, &t.kind, "block {}", b.id);
            }
        }
    }
}

