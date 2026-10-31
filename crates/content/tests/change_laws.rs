//! The change laws over generated documents and generated changes: every
//! change inverts exactly, composing agrees with applying in turn, a mapped
//! position lands on the character it named, and a change either yields a
//! valid value or fails.

use proptest::prelude::*;
use quillmark_content::change::{
    AnchorStep, Assoc, Change, ContainerStep, IslandStep, MarkStep, SetBlock, Splice, Step, apply, compose, compose_in,
    invert, map_pos, map_pos_in,
};
use quillmark_content::lines::island::IslandType;
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{
    Anchor, Block, BlockKind, Container, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT, Island, Mark, MarkKind, Pos,
};
use quillmark_content::units::{Utf16, change_from_scalars, change_to_scalars, from_scalars};
use serde_json::json;

fn word() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,5}",
        "[a-z]{1,4}".prop_map(|w| format!("**{w}**")),
        "[a-z]{1,4}".prop_map(|w| format!("_{w}_")),
        "[a-z]{1,4}".prop_map(|w| format!("[{w}](https://x.io/{w})")),
        Just("$x^2$".to_string()),
        Just("[^n]".to_string()),
        Just("é😀".to_string()),
    ]
}

fn line() -> impl Strategy<Value = String> {
    prop::collection::vec(word(), 1..5).prop_map(|w| w.join(" "))
}

fn block() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => line(),
        1 => line().prop_map(|l| format!("## {l}")),
        1 => prop::collection::vec(line(), 1..4).prop_map(|ls| ls.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")),
        1 => prop::collection::vec(line(), 1..3).prop_map(|ls| ls.iter().map(|l| format!("1. {l}")).collect::<Vec<_>>().join("\n")),
        1 => (line(), line()).prop_map(|(a, b)| format!("> {a}\n>\n> {b}")),
        1 => (line(), line()).prop_map(|(a, b)| format!("{a}\\\n{b}")),
        1 => ("[a-z]{1,4}", "[a-z]{1,4}", "[a-z]{1,4}", "[a-z]{1,4}")
            .prop_map(|(a, b, c, d)| format!("| {a} | {b} |\n| - | - |\n| {c} | {d} |")),
        1 => "[a-z]{1,6}".prop_map(|c| format!("```rust\n{c}\n```")),
    ]
}

fn document() -> impl Strategy<Value = Content> {
    prop::collection::vec(block(), 1..6).prop_map(|bs| {
        let mut md = bs.join("\n\n");
        if md.contains("[^n]") {
            md.push_str("\n\n[^n]: a note");
        }
        let mut c = from_markdown(&md).unwrap().content;
        let len = c.len_usv();
        if len > 3 {
            c.anchors.push(Anchor::new("c-0", 1, len.min(4)));
            c.anchors.push(Anchor::new("c-1", 2, 2));
        }
        c.normalize();
        c.validate().unwrap();
        c
    })
}

type Op = (u8, usize, usize, u8);

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec((0u8..9, any::<usize>(), any::<usize>(), any::<u8>()), 1..8)
}

const INSERTS: [&str; 7] = ["x", "yz", " ", "\n", "q\nr", "\u{2028}", "😀a"];
const TAGS: [&str; 4] = ["strong", "emph", "code", "link"];

fn kind_of(tag: &str) -> MarkKind {
    match tag {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "code" => MarkKind::Code,
        _ => MarkKind::Link { url: "https://y.io".into() },
    }
}

fn chars(c: &Content) -> Vec<char> {
    c.text.chars().collect()
}

/// A splice on one level, its new `\n`s shaped like the block it sits in.
fn splice_on(c: &Content, x: usize, y: usize, z: u8, k: usize) -> Splice {
    let cs = chars(c);
    let len = cs.len();
    let at = x % (len + 1);
    let dn = y % (len - at + 1).min(4);
    let insert = INSERTS[z as usize % INSERTS.len()];
    let shape = c.blocks[c.block_at(at)].clone();
    let blocks = insert
        .chars()
        .filter(|&ch| ch == '\n')
        .enumerate()
        .map(|(j, _)| Block { id: format!("n{k}x{j}"), ..shape.clone() })
        .collect();
    let mut s = Splice { blocks, ..Splice::new(at, cs[at..at + dn].iter().collect::<String>(), insert) };
    if z % 3 == 0 && !insert.is_empty() {
        s.marks.push(Mark::new(0, insert.chars().count(), MarkKind::Strong));
    }
    s
}

fn step_for(c: &Content, (op, x, y, z): Op, k: usize) -> Option<Step> {
    let len = c.len_usv();
    Some(match op {
        0 | 1 => Step::Splice(splice_on(c, x, y, z, k)),
        2 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            let (start, end) = (a.min(b), a.max(b));
            let tag = TAGS[z as usize % TAGS.len()];
            if z % 2 == 0 {
                Step::Mark(MarkStep::Add(Mark::new(start, end, kind_of(tag))))
            } else {
                Step::Mark(MarkStep::Remove { start, end, tag })
            }
        }
        3 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            match z % 3 {
                0 if !c.anchors.is_empty() => {
                    Step::Anchor(AnchorStep::Remove { id: c.anchors[x % c.anchors.len()].id.clone() })
                }
                _ => Step::Anchor(AnchorStep::Set(Anchor::new(format!("c-{}", z % 4), a.min(b), a.max(b)))),
            }
        }
        4 => {
            let b = &c.blocks[x % c.blocks.len()];
            let kind = match z % 3 {
                0 => ("heading", Some(json!({"level": 1 + (y % 6)}))),
                1 => ("para", None),
                _ => ("code", None),
            };
            Step::SetBlock(SetBlock {
                id: b.id.clone(),
                kind: Some(kind.0.into()),
                attrs: kind.1.and_then(|v| v.as_object().cloned()),
                path: None,
            })
        }
        5 => {
            let b = &c.blocks[x % c.blocks.len()];
            let path = match z % 2 {
                0 => Vec::new(),
                _ => c.blocks[y % c.blocks.len()].path.clone(),
            };
            Step::SetBlock(SetBlock { id: b.id.clone(), path: Some(path), ..SetBlock::default() })
        }
        6 => {
            let with: Vec<(&Island, String)> =
                c.islands.iter().flat_map(|i| i.slot_order().into_iter().map(move |s| (i, s))).collect();
            let (island, slot) = with.get(x % with.len().max(1))?;
            let inner = &island.slots[slot];
            let mut s = splice_on(inner, y, x / 7, z, k);
            s.blocks.iter_mut().for_each(|b| b.path.clear());
            Step::Nested { island: island.id.clone(), slot: slot.clone(), change: Change::new(vec![Step::Splice(s)]) }
        }
        7 => {
            let at = x % (len + 1);
            let island = match z % 2 {
                0 => Island::new(format!("isl-n{k}"), IslandType::Math).with_props(json!({"tex": "y"})),
                _ => {
                    let mut i = Island::new(format!("isl-n{k}"), IslandType::Footnote);
                    i.slots.insert("note".into(), quillmark_content::markdown::from_plaintext("n"));
                    i
                }
            };
            Step::Island(IslandStep::Insert { at, island })
        }
        _ => {
            let id = format!("q-n{k}");
            let b = c.blocks[x % c.blocks.len()].clone();
            let mut path = b.path.clone();
            path.insert(0, id.clone());
            return Some(Step::Nested {
                island: String::new(),
                slot: String::new(),
                change: Change::new(vec![
                    Step::Container(ContainerStep::Set(Container::new(id, ContainerKind::Quote))),
                    Step::SetBlock(SetBlock { id: b.id, path: Some(path), ..SetBlock::default() }),
                ]),
            });
        }
    })
}

/// A change built against `d`, each step kept only where the value it reads
/// takes it.
fn change_for(d: &Content, ops: &[Op]) -> Change {
    let mut sim = d.clone();
    let mut steps = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let Some(step) = step_for(&sim, *op, k) else { continue };
        let group = match step {
            Step::Nested { island, change, .. } if island.is_empty() => change.steps,
            step => vec![step],
        };
        let mut trial = steps.clone();
        trial.extend(group.iter().cloned());
        if let Ok(done) = apply(d, &Change::new(trial.clone())) {
            steps = trial;
            sim = done.content;
        }
    }
    Change::new(steps)
}

fn positions(c: &Content) -> Vec<(Pos, char)> {
    let mut out: Vec<(Pos, char)> = chars(c).into_iter().enumerate().map(|(i, ch)| (Pos::At(i), ch)).collect();
    for island in &c.islands {
        for (slot, content) in &island.slots {
            for (i, ch) in chars(content).into_iter().enumerate() {
                out.push((Pos::In { island: island.id.clone(), slot: slot.clone(), at: Box::new(Pos::At(i)) }, ch));
            }
        }
    }
    out
}

fn char_at(c: &Content, p: &Pos) -> Option<char> {
    match p {
        Pos::At(n) => c.text.chars().nth(*n),
        Pos::In { island, slot, at } => char_at(c.slot(island, slot)?, at),
    }
}

fn next(p: &Pos) -> Pos {
    match p {
        Pos::At(n) => Pos::At(n + 1),
        Pos::In { island, slot, at } => Pos::In { island: island.clone(), slot: slot.clone(), at: Box::new(next(at)) },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn every_change_inverts_exactly(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let after = apply(&d, &c).unwrap();
        let undo = invert(&c, &d).unwrap();
        let back = apply(&after.content, &undo);
        prop_assert!(back.is_ok(), "undo refused: {:?}\nchange {:?}\nundo {:?}", back, c, undo);
        prop_assert_eq!(back.unwrap().content, d, "change {:?}", c);
    }

    #[test]
    fn a_change_yields_a_valid_value_or_fails(d in document(), ops in ops()) {
        let mut sim = d.clone();
        let mut steps = Vec::new();
        for (k, op) in ops.iter().enumerate() {
            if let Some(step) = step_for(&sim, *op, k) {
                steps.push(step);
            }
            if let Ok(done) = apply(&d, &Change::new(steps.clone())) {
                sim = done.content;
            }
        }
        if let Ok(done) = apply(&d, &Change::new(steps)) {
            prop_assert_eq!(done.content.validate(), Ok(()));
            prop_assert_eq!(Content::from_json(&done.content.to_json()).unwrap(), done.content);
        }
    }

    #[test]
    fn composing_agrees_with_applying_in_turn(d in document(), ops in ops(), cut in any::<usize>()) {
        let c = change_for(&d, &ops);
        let cut = cut % (c.steps.len() + 1);
        let (a, b) = (Change::new(c.steps[..cut].to_vec()), Change::new(c.steps[cut..].to_vec()));
        let (Ok(mid), Ok(_)) = (apply(&d, &a), Ok::<(), ()>(())) else { return Ok(()) };
        let Ok(end) = apply(&mid.content, &b) else { return Ok(()) };
        let composed = apply(&d, &compose(&a, &b));
        prop_assert!(composed.is_ok(), "{:?}", composed);
        prop_assert_eq!(composed.unwrap().content, end.content);
    }

    #[test]
    fn typing_and_deleting_compose_as_applied(d in document(), keys in prop::collection::vec(0u8..4, 1..10), at in any::<usize>()) {
        let mut doc = d.clone();
        let mut caret = at % (d.len_usv() + 1);
        let mut total = Change::default();
        for k in keys {
            let s = match k {
                0 | 1 => Splice::new(caret, "", if k == 0 { "a" } else { "b" }),
                2 if caret > 0 => {
                    let ch = doc.text.chars().nth(caret - 1).unwrap();
                    if ch == '\n' || ch == ISLAND_SLOT { continue }
                    caret -= 1;
                    Splice::new(caret, ch.to_string(), "")
                }
                _ => continue,
            };
            if k < 2 { caret += 1 }
            let step = Change::new(vec![Step::Splice(s)]);
            let Ok(done) = apply(&doc, &step) else { break };
            doc = done.content;
            total = compose(&total, &done.applied);
        }
        prop_assert_eq!(apply(&d, &total).unwrap().content, doc);
    }

    #[test]
    fn a_mapped_position_lands_on_its_character(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let after = apply(&d, &c).unwrap();
        for (p, ch) in positions(&d) {
            let (Some(q), Some(r)) = (map_pos(&after.applied, &p, Assoc::After), map_pos(&after.applied, &next(&p), Assoc::Before)) else {
                continue;
            };
            if next(&q) == r {
                prop_assert_eq!(char_at(&after.content, &q), Some(ch), "{:?} → {:?} under {:?}", p, q, c);
            }
        }
    }
}

#[test]
fn a_typed_run_composes_into_one_splice() {
    let d = from_markdown("ab").unwrap().content;
    let mut total = Change::default();
    let mut doc = d.clone();
    for (at, ch) in [(1, "x"), (2, "y"), (3, "z")] {
        let done = apply(&doc, &Change::new(vec![Step::Splice(Splice::new(at, "", ch))])).unwrap();
        doc = done.content;
        total = compose(&total, &done.applied);
    }
    let back = apply(&doc, &Change::new(vec![Step::Splice(Splice::new(3, "z", ""))])).unwrap();
    total = compose(&total, &back.applied);
    assert!(matches!(&total.steps[..], [Step::Splice(s)] if s.insert == "xy"), "{total:?}");
}

#[test]
fn a_split_names_the_left_half_and_a_join_keeps_the_right() {
    let d = from_markdown("one two\n\nthree").unwrap().content;
    let (left, right) = (d.blocks[0].id.clone(), d.blocks[1].id.clone());
    let split = Splice { blocks: vec![Block::new("b9", BlockKind::Para)], ..Splice::new(3, " ", "\n") };
    let done = apply(&d, &Change::new(vec![Step::Splice(split)])).unwrap().content;
    assert_eq!(done.blocks.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["b9", left.as_str(), right.as_str()]);
    let join = apply(&d, &Change::new(vec![Step::Splice(Splice::new(7, "\n", ""))])).unwrap().content;
    assert_eq!(join.blocks.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), [right.as_str()]);
}

#[test]
fn a_wholly_deleted_range_anchor_is_dropped_and_a_point_collapses() {
    let mut d = from_markdown("keep this text").unwrap().content;
    d.anchors = vec![Anchor::new("gone", 5, 9), Anchor::new("pt", 7, 7), Anchor::new("cut", 3, 7)];
    let done = apply(&d, &Change::new(vec![Step::Splice(Splice::new(5, "this", "that"))])).unwrap();
    assert_eq!(done.dropped.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["gone"]);
    assert_eq!(done.content.anchor("pt"), Some(&Anchor::new("pt", 5, 5)));
    assert_eq!(done.content.anchor("cut"), Some(&Anchor::new("cut", 3, 5)));
}

#[test]
fn a_break_in_a_heading_fails_the_change() {
    let d = from_markdown("# title").unwrap().content;
    let c = Change::new(vec![Step::Splice(Splice::new(2, "", HARD_BREAK.to_string()))]);
    assert!(apply(&d, &c).is_err());
}

#[test]
fn insertion_never_grows_a_mark() {
    let d = from_markdown("a **bold** b").unwrap().content;
    let c = Change::new(vec![Step::Splice(Splice::new(4, "", "X")), Step::Splice(Splice::new(6, "", "Y"))]);
    let done = apply(&d, &c).unwrap().content;
    assert_eq!(done.text, "a boXlYd b");
    assert_eq!(done.marks, [Mark::new(2, 4, MarkKind::Strong), Mark::new(5, 6, MarkKind::Strong), Mark::new(7, 8, MarkKind::Strong)]);
}


fn memo() -> Content {
    let para = |t: &str| json!({"text": t, "blocks": [{"id": "b0", "kind": "para"}]});
    Content::from_value(&json!({
        "text": "Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\n\u{FFFC}",
        "blocks": [
            {"id": "b0", "kind": "heading", "attrs": {"level": 1}},
            {"id": "b1", "kind": "para"},
            {"id": "b2", "kind": "para", "path": ["l0", "i0"]},
            {"id": "b3", "kind": "para", "path": ["l0", "i1"]},
            {"id": "b4", "kind": "para"}
        ],
        "containers": [
            {"id": "l0", "kind": "list", "attrs": {"ordered": false, "start": 1}},
            {"id": "i0", "kind": "item"},
            {"id": "i1", "kind": "item", "attrs": {"checked": true}}
        ],
        "marks": [{"start": 28, "end": 31, "type": "strong"}],
        "anchors": [{"id": "c-7f3a", "start": 23, "end": 27}],
        "islands": [{
            "id": "t0", "type": "table",
            "props": {
                "columns": ["k1", "k2"],
                "rows": [
                    {"id": "r0", "cells": {"k1": "s0", "k2": "s1"}},
                    {"id": "r1", "cells": {"k1": "s2", "k2": "s3"}}
                ]
            },
            "slots": {"s0": para("Q"), "s1": para("Revenue"), "s2": para("Q3"), "s3": para("4.1M")}
        }]
    }))
    .unwrap()
}

#[test]
fn the_worked_edits_apply() {
    let d = memo();
    let edits = [
        json!({"steps": [{"splice": {"at": 31, "delete": "", "insert": "!", "marks": [{"type": "strong"}]}}]}),
        json!({"steps": [{"splice": {"at": 27, "delete": " ", "insert": "\n", "blocks": [{"id": "b5", "kind": "para"}]}}]}),
        json!({"steps": [{"mark": {"start": 15, "end": 22, "type": "strong", "add": true}}]}),
        json!({"steps": [
            {"container": {"id": "l1", "kind": "list", "attrs": {"ordered": false, "start": 1}}},
            {"setBlock": {"id": "b3", "path": ["l0", "i0", "l1", "i1"]}}
        ]}),
        json!({"steps": [{"nested": {"island": "t0", "slot": "s3",
            "change": {"steps": [{"splice": {"at": 2, "delete": "1", "insert": "3"}}]}}}]}),
        json!({"steps": [{"anchor": {"id": "c-91be", "start": 33, "end": 40}}]}),
    ];
    let mut results = Vec::new();
    for e in &edits {
        let c = Change::from_value(e).unwrap();
        let done = apply(&d, &c).unwrap_or_else(|err| panic!("{e}: {err}"));
        assert_eq!(Change::from_json(&done.applied.to_json()).unwrap(), done.applied);
        assert_eq!(apply(&done.content, &invert(&c, &d).unwrap()).unwrap().content, d);
        results.push(done.content);
    }
    assert_eq!(results[0].marks[0], Mark::new(28, 32, MarkKind::Strong));
    assert_eq!(results[1].blocks.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["b0", "b5", "b1", "b2", "b3", "b4"]);
    assert_eq!(results[3].blocks[3].path, ["l0", "i0", "l1", "i1"]);
    assert_eq!(results[4].slot("t0", "s3").unwrap().text, "4.3M");
    assert_eq!(results[5].anchor("c-91be").map(|a| (a.start, a.end)), Some((33, 40)));
}

#[test]
fn deleting_the_last_block_of_an_item_drops_its_containers() {
    let d = memo();
    let c = Change::new(vec![
        Step::Splice(Splice::new(41, "Draft spec\n", "")),
    ]);
    let done = apply(&d, &c).unwrap();
    assert!(done.content.container("i1").is_none());
    assert!(matches!(done.applied.steps.last(), Some(Step::Container(ContainerStep::Remove { id })) if id == "i1"));
    assert_eq!(apply(&done.content, &invert(&c, &d).unwrap()).unwrap().content, d);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn an_applied_change_reads_and_writes_itself(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let applied = apply(&d, &c).unwrap().applied;
        let json = applied.to_json();
        let back = Change::from_json(&json).unwrap();
        prop_assert_eq!(back.to_json(), json);
        prop_assert_eq!(back, applied);
    }
}

fn wild_step() -> impl Strategy<Value = Step> {
    let n = || prop_oneof![0usize..40, Just(usize::MAX)];
    let id = || prop_oneof![Just("b0".to_string()), Just("l0".to_string()), Just("i0".to_string()), "[a-z0-9]{1,3}"];
    prop_oneof![
        (n(), "[a \n\u{2028}\u{FFFC}]{0,3}", "[a \n\u{2028}\u{FFFC}]{0,3}", prop::collection::vec(id(), 0..3), n(), n())
            .prop_map(|(at, delete, insert, ids, s, e)| Step::Splice(Splice {
                marks: vec![Mark::new(s, e, MarkKind::Emph)],
                blocks: ids.into_iter().map(|i| Block::new(i, BlockKind::Para)).collect(),
                ..Splice::new(at, delete, insert)
            })),
        (n(), n(), any::<bool>()).prop_map(|(s, e, add)| Step::Mark(if add {
            MarkStep::Add(Mark::new(s, e, MarkKind::Strong))
        } else {
            MarkStep::Remove { start: s, end: e, tag: "strong" }
        })),
        (id(), n(), n()).prop_map(|(i, s, e)| Step::Anchor(AnchorStep::Set(Anchor::new(i, s, e)))),
        id().prop_map(|i| Step::Anchor(AnchorStep::Remove { id: i })),
        (id(), prop::collection::vec(id(), 0..4)).prop_map(|(i, path)| Step::SetBlock(SetBlock { id: i, path: Some(path), ..SetBlock::default() })),
        id().prop_map(|i| Step::Container(ContainerStep::Remove { id: i })),
        id().prop_map(|i| Step::Container(ContainerStep::Set(Container::new(i, ContainerKind::Quote)))),
        (id(), n()).prop_map(|(i, at)| Step::Island(IslandStep::Insert { at, island: Island::new(i, IslandType::Table) })),
        id().prop_map(|i| Step::Island(IslandStep::Props { id: i, props: json!({"columns": ["k"], "rows": [{"id": "r", "cells": {"k": "s"}}]}), slots: Default::default() })),
        (id(), n(), "[a\n]{0,2}").prop_map(|(i, at, insert)| Step::Nested {
            island: i,
            slot: "s0".into(),
            change: Change::new(vec![Step::Splice(Splice::new(at, "", insert))]),
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(600))]

    #[test]
    fn a_wild_change_is_refused_or_valid(d in document(), steps in prop::collection::vec(wild_step(), 1..4)) {
        let c = Change::new(steps);
        if let Ok(done) = apply(&d, &c) {
            prop_assert_eq!(done.content.validate(), Ok(()));
            let _ = invert(&c, &d).unwrap();
        }
        let _ = map_pos(&c, &Pos::At(3), Assoc::After);
    }
}

mod diffs {
    use super::*;
    use quillmark_content::diff::{align, diff, replace, revise};
    use quillmark_content::markdown::to_markdown;

    /// The two equal once every block, container and island id is renamed by
    /// where it first appears.
    fn ids_reached(_before: &Content, want: &Content, got: &Content) -> bool {
        fn erased(c: &Content) -> Content {
            let mut c = c.clone();
            let names: std::collections::HashMap<String, String> =
                c.containers.iter().enumerate().map(|(n, k)| (k.id.clone(), format!("k{n}"))).collect();
            for (n, b) in c.blocks.iter_mut().enumerate() {
                b.id = format!("b{n}");
                b.path.iter_mut().for_each(|id| *id = names[id].clone());
            }
            c.containers.iter_mut().for_each(|k| k.id = names[&k.id].clone());
            for (n, i) in c.islands.iter_mut().enumerate() {
                i.id = format!("i{n}");
                for slot in i.slots.values_mut() {
                    *slot = erased(slot);
                }
            }
            c
        }
        erased(want) == erased(got)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn a_diff_reaches_its_target(a in document(), b in document()) {
            let c = diff(&a, &b);
            let done = apply(&a, &c);
            prop_assert!(done.is_ok(), "{:?}\n{:?}", done, c);
            let got = done.unwrap().content;
            prop_assert!(ids_reached(&a, &b, &got), "\nwant {:?}\ngot  {:?}", b, got);
        }

        #[test]
        fn a_diff_of_an_aligned_edit_reaches_it_exactly(d in document(), ops in ops()) {
            let edited = apply(&d, &change_for(&d, &ops)).unwrap().content;
            let fresh = from_markdown(&to_markdown(&edited)).unwrap().content;
            let aligned = align(&d, fresh);
            prop_assert_eq!(aligned.validate(), Ok(()));
            let done = apply(&d, &diff(&d, &aligned)).unwrap().content;
            prop_assert_eq!(done, aligned);
        }

        #[test]
        fn revising_with_the_markdown_read_changes_nothing(d in document()) {
            let r = revise(&d, &to_markdown(&d)).unwrap();
            let done = apply(&d, &r.change).unwrap().content;
            prop_assert_eq!(&done.text, &d.text);
            prop_assert_eq!(done.blocks.iter().map(|b| &b.id).collect::<Vec<_>>(), d.blocks.iter().map(|b| &b.id).collect::<Vec<_>>());
            prop_assert_eq!(done.containers, d.containers.clone());
            prop_assert_eq!(done.islands.iter().map(|i| &i.id).collect::<Vec<_>>(), d.islands.iter().map(|i| &i.id).collect::<Vec<_>>());
        }

        #[test]
        fn a_replace_carries_the_anchors_it_does_not_name(d in document(), ops in ops()) {
            let mut edited = apply(&d, &change_for(&d, &ops)).unwrap().content;
            edited.anchors.clear();
            let done = apply(&d, &replace(&d, &edited)).unwrap();
            let mut kept = done.content.clone();
            kept.anchors.clear();
            prop_assert!(ids_reached(&d, &edited, &kept));
        }
    }

    #[test]
    fn a_revise_keeps_the_ids_of_what_it_kept() {
        let d = from_markdown("# Title\n\nKeep this one.\n\n- first\n- second\n\n| a | b |\n| - | - |\n| c | d |").unwrap().content;
        let r = revise(&d, "# Title\n\nKeep this one, now longer.\n\n- first\n- second\n- third\n\n| a | b |\n| - | - |\n| c | e |").unwrap();
        let done = apply(&d, &r.change).unwrap().content;
        assert_eq!(&done.blocks[..3].iter().map(|b| &b.id).collect::<Vec<_>>(), &d.blocks[..3].iter().map(|b| &b.id).collect::<Vec<_>>());
        assert_eq!(done.blocks[2].path, d.blocks[2].path);
        assert_eq!(done.blocks[4].path[0], d.blocks[3].path[0]);
        assert_eq!(done.islands[0].id, d.islands[0].id);
        assert_eq!(done.islands[0].slot_order(), d.islands[0].slot_order());
        assert!(r.change.steps.iter().any(|s| matches!(s, Step::Nested { .. })));
    }
}

/// `p` with its offsets in UTF-16 code units, each level on its own text.
fn pos_utf16(c: &Content, p: &Pos) -> Pos {
    match p {
        Pos::At(n) => Pos::At(from_scalars(Utf16, &c.text, *n)),
        Pos::In { island, slot, at } => Pos::In {
            island: island.clone(),
            slot: slot.clone(),
            at: Box::new(pos_utf16(c.slot(island, slot).expect("the position's slot"), at)),
        },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn a_value_crosses_utf16_and_back(d in document()) {
        prop_assert_eq!(Content::from_value_in(&d.to_value_in(Utf16), Utf16).unwrap(), d);
    }

    #[test]
    fn a_change_crosses_utf16_and_back(d in document(), ops in ops()) {
        let c = apply(&d, &change_for(&d, &ops)).unwrap().applied;
        let wire = Change::from_value_in(&change_from_scalars(&c, &d, Utf16).unwrap().to_value(), Utf16).unwrap();
        prop_assert_eq!(change_to_scalars(&wire, &d, Utf16).unwrap(), c);
    }

    #[test]
    fn composing_in_utf16_agrees_with_applying_in_turn(d in document(), ops in ops(), cut in any::<usize>()) {
        let c = change_for(&d, &ops);
        let cut = cut % (c.steps.len() + 1);
        let (a, b) = (Change::new(c.steps[..cut].to_vec()), Change::new(c.steps[cut..].to_vec()));
        let Ok(mid) = apply(&d, &a).map(|m| m.content) else { return Ok(()) };
        let Ok(end) = apply(&mid, &b).map(|e| e.content) else { return Ok(()) };
        let (a16, b16) = (change_from_scalars(&a, &d, Utf16).unwrap(), change_from_scalars(&b, &mid, Utf16).unwrap());
        let composed = change_to_scalars(&compose_in(&a16, &b16, Utf16), &d, Utf16).unwrap();
        prop_assert_eq!(apply(&d, &composed).unwrap().content, end);
    }

    #[test]
    fn mapping_in_utf16_agrees_with_mapping_in_scalars(d in document(), ops in ops()) {
        let after = apply(&d, &change_for(&d, &ops)).unwrap();
        let c16 = change_from_scalars(&after.applied, &d, Utf16).unwrap();
        for (p, _) in positions(&d) {
            for assoc in [Assoc::Before, Assoc::After] {
                let want = map_pos(&after.applied, &p, assoc).map(|q| pos_utf16(&after.content, &q));
                prop_assert_eq!(map_pos_in(&c16, &pos_utf16(&d, &p), assoc, Utf16), want);
            }
        }
    }
}

#[test]
fn an_offset_inside_a_surrogate_pair_is_refused() {
    let d = from_markdown("a😀b").unwrap().content;
    let wire = serde_json::json!({"steps": [{"splice": {"at": 2, "delete": "", "insert": "x"}}]});
    let c = Change::from_value_in(&wire, Utf16).unwrap();
    assert!(change_to_scalars(&c, &d, Utf16).is_err());
    let at3 = serde_json::json!({"steps": [{"splice": {"at": 3, "delete": "", "insert": "x"}}]});
    let c = change_to_scalars(&Change::from_value_in(&at3, Utf16).unwrap(), &d, Utf16).unwrap();
    assert_eq!(apply(&d, &c).unwrap().content.text, "a😀xb");
}
