//! A content that `validate` accepts can still project to markdown that
//! re-imports as a different content. The projections take a [`Normalized`],
//! so those shapes cannot reach them.

use quillmark_content::model::{Container, Content, Line, LineKind};
use quillmark_content::{from_markdown, to_markdown};

fn li(ordered: bool, ordinal: u64, instance: u64) -> Container {
    Container::ListItem {
        ordered,
        start: 1,
        ordinal,
        instance,
    }
}

fn built(paths: Vec<Container>) -> Content {
    let lines = paths
        .into_iter()
        .map(|c| {
            let mut l = Line::new(LineKind::Para);
            l.containers = vec![c];
            l
        })
        .collect::<Vec<_>>();
    Content::new("a\nb".to_string(), lines)
}

/// Two one-item lists at a non-canonical `instance` pair: the markers alternate
/// on parity, so `0, 2` spells both `- ` and the runs weld on re-import.
#[test]
fn an_off_parity_instance_pair_projects_as_two_lists() {
    let rt = built(vec![li(false, 0, 0), li(false, 0, 2)]);
    assert_eq!(rt.validate(), Ok(()), "the raw content is valid either way");

    let rt = rt.into_normalized();
    assert_eq!(to_markdown(&rt), "- a\n\n+ b");
    assert_eq!(from_markdown(&to_markdown(&rt)).unwrap(), rt);
}

/// A stored `ordinal` that is not the item's index renumbers, so the bytes a
/// re-encode produces are the bytes the projection rendered from.
#[test]
fn an_off_by_one_ordinal_pair_renumbers_before_it_renders() {
    let rt = built(vec![li(true, 1, 0), li(true, 2, 0)]).into_normalized();
    assert_eq!(to_markdown(&rt), "1. a\n\n2. b");
    assert_eq!(from_markdown(&to_markdown(&rt)).unwrap(), rt);
}

/// Minting is idempotent.
#[test]
fn re_minting_a_token_changes_nothing() {
    let rt = built(vec![li(false, 0, 2), li(false, 1, 2)]).into_normalized();
    assert_eq!(rt.clone().into_content().into_normalized(), rt);
}

/// The empty mint hands out what `normalize` would produce.
#[test]
fn the_empty_mint_is_canonical() {
    assert_eq!(Content::empty().into_normalized(), quillmark_content::Normalized::empty());
}

/// An op list that fails partway leaves its earlier ops applied; the token
/// still holds a canonical value.
#[test]
fn a_failed_op_list_leaves_the_token_canonical() {
    use quillmark_content::model::MarkKind;
    use quillmark_content::MarkOp;

    let mut rt = from_markdown("**a**b").unwrap();
    let ops = [
        MarkOp::Add { start: 0, end: 2, kind: MarkKind::Strong },
        MarkOp::Add { start: 0, end: 99, kind: MarkKind::Strong },
    ];
    assert!(rt.apply_mark_ops(&ops).is_err());
    assert_eq!((*rt).clone().into_normalized(), rt);
}
