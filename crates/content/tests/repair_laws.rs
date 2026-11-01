//! The repair law over generated merge states: repair is total, it repairs
//! the same state to the same value, and a valid value repairs to itself. A
//! write-back reads as the state it repaired, by any number of peers where no
//! rule inserts and by one writer otherwise. Each rule of the merge-repair
//! table has its own case.

use proptest::prelude::*;
use quillmark_content::lines::island::IslandType;
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{
    Anchor, Block, BlockKind, ContainerKind, Content, ISLAND_SLOT, Mark, MarkKind,
};
use quillmark_content::repair::{Raw, RawAnchor, RawIsland, Repair, repair};
use serde_json::json;
use std::collections::BTreeMap;

fn word() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,5}",
        "[a-z]{1,4}".prop_map(|w| format!("**{w}**")),
        Just("$x^2$".to_string()),
        Just("[^n]".to_string()),
        Just("é😀".to_string()),
    ]
}

fn block() -> impl Strategy<Value = String> {
    let line = || prop::collection::vec(word(), 1..4).prop_map(|w| w.join(" "));
    prop_oneof![
        2 => line(),
        1 => line().prop_map(|l| format!("## {l}")),
        1 => prop::collection::vec(line(), 1..4).prop_map(|ls| ls.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")),
        1 => (line(), line()).prop_map(|(a, b)| format!("> {a}\n>\n> - {b}")),
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

type Damage = (u8, usize, usize);

fn pick<T>(v: &[T], x: usize) -> Option<usize> {
    (!v.is_empty()).then(|| x % v.len())
}

fn char_offset(text: &str, x: usize) -> usize {
    x % (text.chars().count() + 1)
}

fn insert_at(text: &mut String, at: usize, s: &str) {
    let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
    text.insert_str(byte, s);
}

/// One merge-shaped break: what two valid concurrent edits can leave, and a
/// few no merge can, to keep the repair total.
fn damage(raw: &mut Raw, (op, x, y): Damage) {
    match op % 18 {
        0 => {
            if let Some(k) = pick(&raw.containers, x) {
                raw.containers.remove(k);
            }
        }
        1 => {
            if let (Some(i), Some(j)) = (pick(&raw.blocks, x), pick(&raw.blocks, y)) {
                raw.blocks[i].path = raw.blocks[j].path.clone();
            }
        }
        2 => {
            if let Some(i) = pick(&raw.blocks, x) {
                raw.blocks[i].path.clear();
            }
        }
        3 => {
            if let Some(k) = pick(&raw.slot_ids, x) {
                raw.slot_ids[k] = None;
            }
        }
        4 => {
            if let Some(id) = raw
                .islands
                .keys()
                .nth(x % raw.islands.len().max(1))
                .cloned()
            {
                raw.islands.remove(&id);
            }
        }
        5 => {
            raw.islands.insert(
                "orphan".into(),
                RawIsland {
                    island_type: IslandType::Math,
                    props: json!({"tex": "x"}),
                    slots: BTreeMap::new(),
                },
            );
        }
        6 => {
            let slots: Vec<usize> = raw
                .text
                .chars()
                .enumerate()
                .filter(|(_, c)| *c == ISLAND_SLOT)
                .map(|(k, _)| k)
                .collect();
            if let Some(k) = pick(&slots, x) {
                insert_at(&mut raw.text, slots[k] + y % 2, "zz");
            }
        }
        7 => {
            if let Some(k) = pick(&raw.anchors, x) {
                let a = &mut raw.anchors[k].anchor;
                a.end = a.start;
            }
        }
        8 => {
            if let (Some(i), Some(j)) = (pick(&raw.blocks, x), pick(&raw.blocks, y)) {
                raw.blocks[i].id = raw.blocks[j].id.clone();
            }
        }
        9 => {
            let at = char_offset(&raw.text, x);
            insert_at(&mut raw.text, at, "\r\u{202E}");
        }
        10 => raw.marks.push(Mark::new(x % 50, y % 60, MarkKind::Strong)),
        11 => {
            if let (Some(i), Some(j)) = (pick(&raw.slot_ids, x), pick(&raw.slot_ids, y)) {
                raw.slot_ids[i] = raw.slot_ids[j].clone();
            }
        }
        12 => {
            if let Some(i) = pick(&raw.blocks, x) {
                raw.blocks[i].kind = if y % 2 == 0 {
                    BlockKind::Rule
                } else {
                    BlockKind::Heading { level: 2 }
                };
            }
        }
        13 => {
            let at = char_offset(&raw.text, x);
            insert_at(
                &mut raw.text,
                at,
                if y % 2 == 0 { "\n" } else { "\u{2028}" },
            );
        }
        14 => {
            for island in raw.islands.values_mut() {
                if let Some(rows) = island.props.get_mut("rows").and_then(|r| r.as_array_mut()) {
                    let n = x % rows.len().max(1);
                    if let Some(cells) = rows.get_mut(n).and_then(|r| r.get_mut("cells")) {
                        if let Some(m) = cells.as_object_mut() {
                            let k = m.keys().nth(y % m.len().max(1)).cloned();
                            k.map(|k| m.remove(&k));
                        }
                    }
                }
            }
        }
        15 => {
            if let (Some(i), Some(j)) = (pick(&raw.blocks, x), pick(&raw.blocks, y)) {
                let path = &raw.blocks[j].path;
                raw.blocks[i].path = path[(y % 3).min(path.len())..].to_vec();
            }
        }
        16 => {
            if let Some(slot) = raw
                .islands
                .values_mut()
                .flat_map(|i| i.slots.values_mut())
                .nth(x % 4)
            {
                damage(slot, ((y % 251) as u8, x / 7, y / 3));
            }
        }
        _ => {
            if let Some(k) = pick(&raw.anchors, x) {
                raw.anchors.push(RawAnchor {
                    anchor: raw.anchors[k].anchor.clone(),
                    range: false,
                });
            }
        }
    }
}

fn damages() -> impl Strategy<Value = Vec<Damage>> {
    prop::collection::vec((any::<u8>(), any::<usize>(), any::<usize>()), 1..6)
}

/// For each character of `b`, the character of `a` a longest common
/// subsequence pairs it with; `None` for one `b` adds.
fn align(a: &str, b: &str) -> Vec<Option<usize>> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut l = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            l[i][j] = if a[i] == b[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut kept = vec![None; b.len()];
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            kept[j] = Some(i);
            (i, j) = (i + 1, j + 1);
        } else if l[i + 1][j] >= l[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    kept
}

/// Whether `c` holds a character or a slot the merged state `m` does not.
fn adds(m: &Raw, c: &Content) -> bool {
    let kept = align(&m.text, &c.text);
    if kept.contains(&None) {
        return true;
    }
    let m_slots: Vec<usize> = m
        .text
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == ISLAND_SLOT)
        .map(|(k, _)| k)
        .collect();
    let c_slots = c
        .text
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == ISLAND_SLOT);
    c.islands.iter().zip(c_slots).any(|(island, (j, _))| {
        let k = m_slots.iter().position(|&p| Some(p) == kept[j]).unwrap();
        let Some(record) = m
            .slot_ids
            .get(k)
            .cloned()
            .flatten()
            .and_then(|id| m.islands.get(&id))
        else {
            return true;
        };
        island
            .slots
            .iter()
            .any(|(key, slot)| record.slots.get(key).is_none_or(|r| adds(r, slot)))
    })
}

/// A peer's write-back of `repair(m)`: the repaired value, and for each of its
/// characters the character of `m` it keeps, `None` where the repair inserted
/// it.
struct WriteBack {
    value: Content,
    kept: Vec<Option<usize>>,
}

fn write_back(m: &Raw) -> (WriteBack, Vec<Repair>) {
    let (value, repairs) = repair(m.clone());
    let kept = align(&m.text, &value.text);
    (WriteBack { value, kept }, repairs)
}

/// The shared state after `writers` peers each wrote `w` back over the merged
/// state it repaired. A character deleted goes once however many delete it, a
/// part keyed by id takes the one value every writer sets, a block's record
/// rides the `\n` that opens it, and each writer's inserted text lands, as
/// concurrent inserts do in a sequence CRDT.
fn merge(w: &WriteBack, writers: usize) -> Raw {
    let c = &w.value;
    let chars: Vec<char> = c.text.chars().collect();
    let n = chars.len();
    let (mut records, mut islands) = (c.blocks[1..].iter(), c.islands.iter());
    let mut text = Vec::new();
    let mut blocks = vec![c.blocks[0].clone()];
    let mut slot_ids = Vec::new();
    let (mut first, mut last) = (vec![0; n + 1], vec![0; n + 1]);
    let mut j = 0;
    while j < n {
        let (k, copies) = match w.kept[j] {
            Some(_) => (j + 1, 1),
            None => ((j..n).find(|&k| w.kept[k].is_some()).unwrap_or(n), writers),
        };
        let (run_records, run_islands) = (records.clone(), islands.clone());
        for copy in 0..copies {
            (records, islands) = (run_records.clone(), run_islands.clone());
            for i in j..k {
                if copy == 0 {
                    first[i] = text.len();
                }
                text.push(chars[i]);
                last[i] = text.len();
                match chars[i] {
                    '\n' => blocks.push(records.next().unwrap().clone()),
                    ISLAND_SLOT => slot_ids.push(Some(islands.next().unwrap().id.clone())),
                    _ => {}
                }
            }
        }
        j = k;
    }
    first[n] = text.len();
    let end = |e: usize| if e == 0 { 0 } else { last[e - 1] };
    Raw {
        text: text.into_iter().collect(),
        blocks,
        containers: c.containers.clone(),
        marks: c
            .marks
            .iter()
            .map(|m| Mark::new(first[m.start], end(m.end), m.kind.clone()))
            .collect(),
        anchors: c
            .anchors
            .iter()
            .map(|a| RawAnchor {
                anchor: if a.is_point() {
                    Anchor::new(a.id.clone(), first[a.start], first[a.start])
                } else {
                    Anchor::new(a.id.clone(), first[a.start], end(a.end))
                },
                range: !a.is_point(),
            })
            .collect(),
        slot_ids,
        islands: Raw::from(c).islands,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_merge_state_repairs_to_a_valid_value(d in document(), ds in damages()) {
        let mut raw = Raw::from(&d);
        for x in &ds {
            damage(&mut raw, *x);
        }
        let (c, _) = repair(raw.clone());
        prop_assert_eq!(c.validate(), Ok(()));
        prop_assert_eq!(Content::from_value(&c.to_value()), Ok(c.clone()));
        prop_assert_eq!(repair(raw).0, c.clone());
        prop_assert_eq!(repair(Raw::from(&c)), (c, Vec::new()));
    }

    /// A rule that inserts nothing reaches its value from the merged state by
    /// deletions and sets alone, which two peers writing it back both write
    /// once.
    #[test]
    fn a_write_back_reads_as_the_state_it_repaired(d in document(), ds in damages()) {
        let mut m = Raw::from(&d);
        for x in &ds {
            damage(&mut m, *x);
        }
        let (w, repairs) = write_back(&m);
        prop_assert_eq!(&repair(merge(&w, 1)).0, &w.value);
        if !repairs.iter().any(Repair::inserts) {
            prop_assert!(!adds(&m, &w.value));
            prop_assert_eq!(&repair(merge(&w, 2)).0, &w.value);
        }
    }

    #[test]
    fn a_valid_value_repairs_to_itself(d in document()) {
        prop_assert_eq!(repair(Raw::from(&d)), (d.clone(), Vec::new()));
        prop_assert_eq!(repair(Raw::from_value(&d.to_value()).unwrap()), (d, Vec::new()));
    }
}

fn raw(v: serde_json::Value) -> Raw {
    Raw::from_value(&v).unwrap()
}

fn paths(c: &Content) -> Vec<Vec<&str>> {
    c.blocks
        .iter()
        .map(|b| b.path.iter().map(String::as_str).collect())
        .collect()
}

#[test]
fn a_path_naming_a_missing_container_loses_it() {
    let (c, repairs) = repair(raw(json!({
        "text": "a\nb",
        "blocks": [{"id": "b0", "kind": "para", "path": ["q0"]}, {"id": "b1", "kind": "para", "path": ["l0", "i5"]}],
        "containers": [{"id": "i5", "kind": "item"}]
    })));
    assert_eq!(paths(&c), [Vec::<&str>::new(), vec![]]);
    assert!(repairs.contains(&Repair::MissingContainer {
        block: "b0".into(),
        container: "q0".into()
    }));
    assert!(repairs.contains(&Repair::MissingContainer {
        block: "b1".into(),
        container: "l0".into()
    }));
}

#[test]
fn a_container_under_two_paths_takes_its_first_blocks() {
    let (c, repairs) = repair(raw(json!({
        "text": "a\nb",
        "blocks": [
            {"id": "b0", "kind": "para", "path": ["q0", "l0", "i0"]},
            {"id": "b1", "kind": "para", "path": ["l0", "i0"]}
        ],
        "containers": [
            {"id": "q0", "kind": "quote"},
            {"id": "l0", "kind": "list", "attrs": {"ordered": false, "start": 1}},
            {"id": "i0", "kind": "item"}
        ]
    })));
    assert_eq!(paths(&c), [vec!["q0", "l0", "i0"], vec!["q0", "l0", "i0"]]);
    assert_eq!(
        repairs,
        [Repair::TwoParents {
            container: "l0".into()
        }]
    );
}

#[test]
fn a_container_over_two_runs_splits_with_derived_ids() {
    let (c, repairs) = repair(raw(json!({
        "text": "a\nb\nc",
        "blocks": [
            {"id": "b0", "kind": "para", "path": ["l0", "i0"]},
            {"id": "b1", "kind": "para"},
            {"id": "b2", "kind": "para", "path": ["l0", "i1"]}
        ],
        "containers": [
            {"id": "l0", "kind": "list", "attrs": {"ordered": true, "start": 3}},
            {"id": "i0", "kind": "item"},
            {"id": "i1", "kind": "item", "attrs": {"checked": true}}
        ]
    })));
    assert_eq!(
        paths(&c),
        [vec!["l0", "i0"], vec![], vec!["l0-b2", "i1-b2"]]
    );
    assert_eq!(
        c.container("l0-b2").unwrap().kind,
        c.container("l0").unwrap().kind
    );
    assert_eq!(
        c.container("i1-b2").unwrap().kind,
        ContainerKind::Item {
            checked: Some(true)
        }
    );
    assert_eq!(
        repairs,
        [Repair::SplitRun {
            container: "l0".into(),
            fresh: "l0-b2".into()
        }]
    );
}

#[test]
fn a_slot_without_a_record_and_a_record_without_a_slot_both_go() {
    let (c, repairs) = repair(raw(json!({
        "text": "a\u{FFFC}b",
        "blocks": [{"id": "b0", "kind": "para"}],
        "marks": [{"start": 0, "end": 3, "type": "strong"}],
        "slot_ids": ["t9"],
        "islands": {"m0": {"type": "math", "props": {"tex": "x"}}}
    })));
    assert_eq!(c.text, "ab");
    assert!(c.islands.is_empty());
    assert_eq!(c.marks, [Mark::new(0, 2, MarkKind::Strong)]);
    assert_eq!(
        repairs,
        [
            Repair::OrphanSlot {
                island: Some("t9".into())
            },
            Repair::OrphanIsland {
                island: "m0".into()
            }
        ]
    );
}

#[test]
fn a_second_slot_naming_one_island_holds_a_copy() {
    let (c, repairs) = repair(raw(json!({
        "text": "\u{FFFC}\u{FFFC}",
        "blocks": [{"id": "b0", "kind": "para"}],
        "slot_ids": ["m0", "m0"],
        "islands": {"m0": {"type": "math", "props": {"tex": "x"}}}
    })));
    let ids: Vec<_> = c.islands.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["m0", "m0-2"]);
    assert_eq!(
        repairs,
        [Repair::SharedIsland {
            island: "m0".into(),
            fresh: "m0-2".into()
        }]
    );
}

#[test]
fn text_beside_a_block_island_splits_into_its_own_blocks() {
    let cell = json!({"text": "", "blocks": [{"id": "b0", "kind": "para"}]});
    let (c, repairs) = repair(raw(json!({
        "text": "ab\u{FFFC}cd",
        "blocks": [{"id": "b0", "kind": "para"}],
        "anchors": [{"id": "c-1", "start": 0, "end": 2}, {"id": "c-2", "start": 2, "end": 5}],
        "slot_ids": ["t0"],
        "islands": {"t0": {
            "type": "table",
            "props": {"columns": ["k0"], "rows": [{"id": "r0", "cells": {"k0": "s0"}}]},
            "slots": {"s0": cell}
        }}
    })));
    assert_eq!(c.text, "ab\n\u{FFFC}\ncd");
    let ids: Vec<_> = c.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b0-t0", "b0-t0-2"]);
    assert_eq!(
        c.anchors,
        [Anchor::new("c-1", 0, 2), Anchor::new("c-2", 3, 7)]
    );
    assert_eq!(
        repairs,
        [
            Repair::SplitBlock {
                island: "t0".into(),
                block: "b0-t0".into()
            },
            Repair::SplitBlock {
                island: "t0".into(),
                block: "b0-t0-2".into()
            }
        ]
    );
}

#[test]
fn an_x_block_holding_a_table_alone_repairs_to_itself() {
    let mut c = from_markdown("intro\n\n| a | b |\n| - | - |\n| c | d |\n\nafter")
        .unwrap()
        .content;
    let k = c.text.split('\n').position(|s| s == "\u{FFFC}").unwrap();
    c.blocks[k].kind = BlockKind::Ext {
        name: "x-callout".into(),
        attrs: Default::default(),
    };
    c.normalize();
    assert_eq!(c.validate(), Ok(()));
    assert_eq!(repair(Raw::from(&c)), (c, Vec::new()));
}

#[test]
fn a_collapsed_range_anchor_is_dropped_and_a_point_kept() {
    let (c, repairs) = repair(raw(json!({
        "text": "abc",
        "blocks": [{"id": "b0", "kind": "para"}],
        "anchors": [
            {"id": "c-1", "start": 1, "end": 1, "range": true},
            {"id": "c-2", "start": 1, "end": 1},
            {"id": "c-3", "start": 2, "end": 1}
        ]
    })));
    assert_eq!(c.anchors, [Anchor::new("c-2", 1, 1)]);
    assert_eq!(
        repairs,
        [
            Repair::CollapsedAnchor { id: "c-1".into() },
            Repair::CollapsedAnchor { id: "c-3".into() }
        ]
    );
}

#[test]
fn a_table_row_missing_a_cell_gets_an_empty_slot() {
    let cell = |t: &str| json!({"text": t, "blocks": [{"id": "b0", "kind": "para"}]});
    let (c, repairs) = repair(raw(json!({
        "text": "\u{FFFC}",
        "blocks": [{"id": "b0", "kind": "para"}],
        "islands": [{
            "id": "t0", "type": "table",
            "props": {"columns": ["k0", "k1"], "rows": [
                {"id": "r0", "cells": {"k0": "s0", "k1": "s1"}},
                {"id": "r1", "cells": {"k0": "s2"}}
            ]},
            "slots": {"s0": cell("a"), "s1": cell("b"), "s2": cell("c")}
        }]
    })));
    let t = &c.islands[0];
    assert_eq!(
        t.props["rows"][1]["cells"],
        json!({"k0": "s2", "k1": "r1-k1"})
    );
    assert_eq!(t.slots["r1-k1"], Content::empty());
    let minted = Repair::EmptyCell {
        island: "t0".into(),
        slot: "r1-k1".into(),
    };
    assert!(repairs.contains(&minted));
    assert_eq!(minted.to_value()["inserts"], json!(true));
}

#[test]
fn a_missing_block_record_is_minted_from_its_neighbour() {
    let (c, _) = repair(Raw {
        text: "a\nb".into(),
        blocks: vec![Block::new("b0", BlockKind::Para)],
        ..Raw::default()
    });
    let ids: Vec<_> = c.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b0", "b0-2"]);
}

#[test]
fn two_peers_writing_back_an_inserting_repair_leave_a_valid_second_copy() {
    let cell = json!({"text": "", "blocks": [{"id": "b0", "kind": "para"}]});
    let beside_a_table = raw(json!({
        "text": "ab\u{FFFC}cd",
        "blocks": [{"id": "b0", "kind": "para"}],
        "slot_ids": ["t0"],
        "islands": {"t0": {
            "type": "table",
            "props": {"columns": ["k0"], "rows": [{"id": "r0", "cells": {"k0": "s0"}}]},
            "slots": {"s0": cell}
        }}
    }));
    let broken_heading = raw(json!({
        "text": "a\u{2028}b",
        "blocks": [{"id": "b0", "kind": "heading", "attrs": {"level": 1}}]
    }));
    for (m, twice) in [
        (beside_a_table, "ab\n\n\u{FFFC}\n\ncd"),
        (broken_heading, "a  b"),
    ] {
        let (w, repairs) = write_back(&m);
        assert!(repairs.iter().all(Repair::inserts));
        assert_eq!(repair(merge(&w, 1)).0, w.value);
        assert_eq!(repair(merge(&w, 2)).0.text, twice);
    }
}
