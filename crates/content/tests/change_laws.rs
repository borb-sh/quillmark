//! The change laws over generated documents and generated changes: every
//! change inverts exactly, composing agrees with applying in turn, a mapped
//! position lands on the character it named, and a change either yields a
//! valid value or fails.

use proptest::prelude::*;
use quillmark_content::change::{
    AnchorStep, ApplyError, Assoc, Change, ContainerStep, IslandStep, MapResult, Mapping, MarkStep,
    SetBlock, Splice, Step, apply, compose, compose_in, invert, map_pos, map_pos_in,
};
use quillmark_content::lines::island::IslandType;
use quillmark_content::markdown::from_markdown;
use quillmark_content::model::{
    Anchor, Block, BlockKind, Container, ContainerKind, Content, HARD_BREAK, ISLAND_SLOT,
    Invariant, Island, Mark, MarkKind, Pos,
};
use quillmark_content::structural::{
    Wrap, delete_range, indent, inherited_marks, join_blocks, outdent, split_block, wrap,
};
use quillmark_content::table::{Row, Table};
use quillmark_content::units::{Utf16, change_from_scalars, change_to_scalars, from_scalars};
use serde_json::json;
use std::collections::BTreeSet;

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
    (prop::collection::vec(block(), 1..6), any::<u8>()).prop_map(|(bs, ext)| {
        let mut md = bs.join("\n\n");
        if md.contains("[^n]") {
            md.push_str("\n\n[^n]: a note");
        }
        let mut c = with_ext(from_markdown(&md).unwrap().content, ext);
        let len = c.len_scalars();
        if len > 3 {
            c.anchors.push(Anchor::new("c-0", 1, len.min(4)));
            c.anchors.push(Anchor::new("c-1", 2, 2));
        }
        for (n, island) in c.islands.iter_mut().enumerate() {
            if let Some(slot) = island.slots.values_mut().find(|s| s.len_scalars() > 0) {
                slot.anchors.push(Anchor::new(format!("s-{n}"), 0, 1));
            }
        }
        c.normalize();
        c.validate().unwrap();
        c
    })
}

/// On some documents, a reserved `x-` name on each axis: an island holding a
/// slot at the start, a block's kind and a mark.
fn with_ext(mut c: Content, seed: u8) -> Content {
    if seed % 5 == 0 {
        let island = Island::new("isl-x", IslandType::Ext("x-chart".into()))
            .with_props(json!({"series": [1, 2], "style": {"tone": "warm"}}))
            .with_slots(
                [(
                    "cap".to_string(),
                    quillmark_content::markdown::from_plaintext("caption"),
                )]
                .into(),
            );
        let insert = Change::new(vec![Step::Island(IslandStep::Insert {
            at: 0,
            island,
            block: None,
        })]);
        if let Ok(done) = apply(&c, &insert) {
            c = done.content;
        }
    }
    if seed % 4 == 0 {
        let k = seed as usize / 4 % c.blocks.len();
        c.blocks[k].kind = BlockKind::Ext {
            name: "x-callout".into(),
            attrs: json!({"tone": "warm"}).as_object().unwrap().clone(),
        };
    }
    let len = c.len_scalars();
    if seed % 3 == 0 && len > 1 {
        c.marks.push(Mark::new(
            1,
            len.min(5),
            MarkKind::Ext {
                name: "x-glow".into(),
                attrs: Default::default(),
            },
        ));
    }
    c
}

type Op = (u8, usize, usize, u8);

fn ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec((0u8..15, any::<usize>(), any::<usize>(), any::<u8>()), 1..8)
}

const INSERTS: [&str; 7] = ["x", "yz", " ", "\n", "q\nr", "\u{2028}", "😀a"];
const TAGS: [&str; 5] = ["strong", "emph", "code", "link", "x-glow"];

fn kind_of(tag: &str) -> MarkKind {
    match tag {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "code" => MarkKind::Code,
        "x-glow" => MarkKind::Ext {
            name: tag.into(),
            attrs: Default::default(),
        },
        _ => MarkKind::Link {
            url: "https://y.io".into(),
        },
    }
}

fn chars(c: &Content) -> Vec<char> {
    c.text.chars().collect()
}

/// A splice on one level, deleting up to 3 characters or, on every third
/// `y`, up to 15, its new `\n`s shaped like the block it sits in.
fn splice_on(c: &Content, x: usize, y: usize, z: u8, k: usize) -> Splice {
    let cs = chars(c);
    let len = cs.len();
    let at = x % (len + 1);
    let reach = if y / 16 % 3 == 0 { 16 } else { 4 };
    let dn = y % (len - at + 1).min(reach);
    let insert = INSERTS[z as usize % INSERTS.len()];
    let shape = c.blocks[c.block_at(at)].clone();
    let blocks = insert
        .chars()
        .filter(|&ch| ch == '\n')
        .enumerate()
        .map(|(j, _)| Block {
            id: format!("n{k}x{j}"),
            ..shape.clone()
        })
        .collect();
    let mut s = Splice {
        blocks,
        ..Splice::new(at, cs[at..at + dn].iter().collect::<String>(), insert)
    };
    if z % 3 == 0 && !insert.is_empty() {
        s.marks
            .push(Mark::new(0, insert.chars().count(), MarkKind::Strong));
    }
    s
}

/// The text `c` holds over `[start, end)`, the quote a writer that read it
/// gives, on every other `seed`.
fn quote_of(c: &Content, start: usize, end: usize, seed: usize) -> Option<String> {
    (seed / 2 % 2 == 0).then(|| chars(c)[start..end].iter().collect())
}

/// An anchor step to `anchor`: a move of the one `c`'s text holds under its
/// id, else a placement.
fn anchor_step(c: &Content, anchor: Anchor, quote: Option<String>) -> Step {
    match c.anchor(&anchor.id) {
        Some(_) => Step::Anchor(AnchorStep::Move { anchor, quote }),
        None => Step::Anchor(AnchorStep::Place { anchor, quote }),
    }
}

fn slots_of(c: &Content) -> Vec<(&Island, String)> {
    c.islands
        .iter()
        .flat_map(|i| i.slot_order().into_iter().map(move |s| (i, s)))
        .collect()
}

fn step_for(c: &Content, (op, x, y, z): Op, k: usize) -> Option<Step> {
    let len = c.len_scalars();
    Some(match op {
        0 | 1 => Step::Splice(splice_on(c, x, y, z, k)),
        2 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            let (start, end) = (a.min(b), a.max(b));
            let tag = TAGS[z as usize % TAGS.len()];
            let quote = quote_of(c, start, end, y);
            if z % 2 == 0 {
                Step::Mark(MarkStep::Add {
                    mark: Mark::new(start, end, kind_of(tag)),
                    quote,
                })
            } else {
                Step::Mark(MarkStep::Remove {
                    start,
                    end,
                    tag: tag.into(),
                    quote,
                })
            }
        }
        3 => {
            let (a, b) = (x % (len + 1), y % (len + 1));
            match z % 3 {
                0 if !c.anchors.is_empty() => Step::Anchor(AnchorStep::Remove {
                    id: c.anchors[x % c.anchors.len()].id.clone(),
                }),
                _ => anchor_step(
                    c,
                    Anchor::new(format!("c-{}", z % 4), a.min(b), a.max(b)),
                    quote_of(c, a.min(b), a.max(b), x),
                ),
            }
        }
        4 => {
            let b = &c.blocks[x % c.blocks.len()];
            let kind = match z % 5 {
                0 => ("heading", Some(json!({"level": 1 + (y % 6)}))),
                1 => ("para", None),
                2 => ("x-callout", Some(json!({"tone": "cool"}))),
                3 => ("x-callout", None),
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
            Step::SetBlock(SetBlock {
                id: b.id.clone(),
                path: Some(path),
                ..SetBlock::default()
            })
        }
        6 => {
            let with = slots_of(c);
            let (island, slot) = with.get(x % with.len().max(1))?;
            let inner = &island.slots[slot];
            let mut s = splice_on(inner, y, x / 7, z, k);
            s.blocks.iter_mut().for_each(|b| b.path.clear());
            Step::Nested {
                island: island.id.clone(),
                slot: slot.clone(),
                change: Change::new(vec![Step::Splice(s)]),
            }
        }
        7 => {
            let at = x % (len + 1);
            let island = match z % 2 {
                0 => Island::new(format!("isl-n{k}"), IslandType::Math)
                    .with_props(json!({"tex": "y"})),
                _ => {
                    let mut i = Island::new(format!("isl-n{k}"), IslandType::Footnote);
                    i.slots.insert(
                        "note".into(),
                        quillmark_content::markdown::from_plaintext("n"),
                    );
                    i
                }
            };
            let block = (y % 2 == 0).then(|| c.blocks[c.block_at(at)].id.clone());
            Step::Island(IslandStep::Insert { at, island, block })
        }
        8 => {
            let tables: Vec<&Island> = c
                .islands
                .iter()
                .filter(|i| i.island_type == IslandType::Table)
                .collect();
            let island = tables.get(x % tables.len().max(1))?;
            let mut grid = Table::read(&island.props)?;
            let mut slots = std::collections::BTreeMap::new();
            match z % 3 {
                0 if grid.rows.len() > 1 => {
                    grid.rows.remove(y % grid.rows.len());
                }
                1 if grid.columns.len() > 1 => {
                    let col = y % grid.columns.len();
                    grid.columns.remove(col);
                    grid.rows.iter_mut().for_each(|r| {
                        r.cells.remove(col);
                    });
                }
                2 => {
                    let cells: Vec<String> = (0..grid.columns.len())
                        .map(|j| format!("s-n{k}x{j}"))
                        .collect();
                    for (j, cell) in cells.iter().enumerate() {
                        let mut inner = quillmark_content::markdown::from_plaintext("new");
                        if j == 0 {
                            inner.anchors.push(Anchor::new(format!("ra-{k}"), 0, 2));
                        }
                        slots.insert(cell.clone(), inner);
                    }
                    grid.rows.insert(
                        y % (grid.rows.len() + 1),
                        Row {
                            id: format!("r-n{k}"),
                            cells,
                        },
                    );
                }
                _ => return None,
            }
            let mut props = island.props.as_object()?.clone();
            grid.write_into(&mut props);
            Step::Island(IslandStep::Props {
                id: island.id.clone(),
                props: props.into(),
                slots,
            })
        }
        9 => {
            let id = format!("q-n{k}");
            let b = c.blocks[x % c.blocks.len()].clone();
            let mut path = b.path.clone();
            path.insert(0, id.clone());
            return Some(Step::Nested {
                island: String::new(),
                slot: String::new(),
                change: Change::new(vec![
                    Step::Container(ContainerStep::Set(Container::new(id, ContainerKind::Quote))),
                    Step::SetBlock(SetBlock {
                        id: b.id,
                        path: Some(path),
                        ..SetBlock::default()
                    }),
                ]),
            });
        }
        10 | 11 => {
            let with = slots_of(c);
            let (island, slot) = with.get(x % with.len().max(1))?;
            let inner = &island.slots[slot];
            let n = inner.len_scalars();
            let (a, b) = (y % (n + 1), y / 3 % (n + 1));
            let (start, end) = (a.min(b), a.max(b));
            let quote = quote_of(inner, start, end, z as usize);
            let tag = TAGS[z as usize % TAGS.len()];
            let step = match (op, z % 3) {
                (10, 0) => Step::Mark(MarkStep::Remove {
                    start,
                    end,
                    tag: tag.into(),
                    quote,
                }),
                (10, _) if start < end => Step::Mark(MarkStep::Add {
                    mark: Mark::new(start, end, kind_of(tag)),
                    quote,
                }),
                (11, 0) if !inner.anchors.is_empty() => Step::Anchor(AnchorStep::Remove {
                    id: inner.anchors[x % inner.anchors.len()].id.clone(),
                }),
                (11, _) => anchor_step(
                    inner,
                    Anchor::new(format!("sa-{}", z % 4), start, end),
                    quote,
                ),
                _ => return None,
            };
            Step::Nested {
                island: island.id.clone(),
                slot: slot.clone(),
                change: Change::new(vec![step]),
            }
        }
        12 => {
            let mut note = quillmark_content::markdown::from_plaintext("nb");
            note.anchors.push(Anchor::new(format!("fa-{k}"), 0, 1));
            let mut island = Island::new(format!("isl-f{k}"), IslandType::Footnote);
            island.slots.insert("note".into(), note);
            let mut s = splice_on(c, x, y, z, k);
            s.insert.push(ISLAND_SLOT);
            s.islands = vec![island];
            Step::Splice(s)
        }
        13 => {
            let held = c.containers.get(x % c.containers.len().max(1))?;
            let kind = match held.kind {
                ContainerKind::List { ordered, start } => ContainerKind::List {
                    ordered: ordered != (z % 2 == 0),
                    start: start + y as u64 % 3,
                },
                ContainerKind::Item { .. } => ContainerKind::Item {
                    checked: [None, Some(false), Some(true)][y % 3],
                },
                ContainerKind::Quote => return None,
            };
            Step::Container(ContainerStep::Set(Container::new(held.id.clone(), kind)))
        }
        _ => return island_move(c, x, y, z),
    })
}

/// An island moved as `diff` moves one: its slot, with the `\n` opening its
/// block where it sits alone in one, deleted and inserted at another block
/// boundary or offset under the same id, the copy first on every other `z`.
fn island_move(c: &Content, x: usize, y: usize, z: u8) -> Option<Step> {
    let island = c.islands.get(x % c.islands.len().max(1))?.clone();
    let cs = chars(c);
    let slot = cs
        .iter()
        .enumerate()
        .filter(|(_, ch)| **ch == ISLAND_SLOT)
        .nth(x % c.islands.len())?
        .0;
    let k = c.block_at(slot);
    let alone = k > 0 && c.block_texts()[k] == ISLAND_SLOT.to_string();
    let (from, chunk, blocks) = match alone {
        true => (
            slot - 1,
            format!("\n{ISLAND_SLOT}"),
            vec![c.blocks[k].clone()],
        ),
        false => (slot, ISLAND_SLOT.to_string(), Vec::new()),
    };
    let n = chunk.chars().count();
    let to = match alone {
        true => {
            let ends: Vec<usize> = (0..=cs.len())
                .filter(|&i| (i == cs.len() || cs[i] == '\n') && (i < from || i > from + n))
                .collect();
            *ends.get(y % ends.len().max(1))?
        }
        false => (0..=cs.len())
            .filter(|&i| i < from || i > from + n)
            .nth(y % (cs.len() - n).max(1))?,
    };
    let insert = |at| {
        Step::Splice(Splice {
            blocks: blocks.clone(),
            islands: vec![island.clone()],
            ..Splice::new(at, "", chunk.clone())
        })
    };
    let delete = |at| Step::Splice(Splice::new(at, chunk.clone(), ""));
    let steps = match (z % 2 == 0, to < from) {
        (true, true) => vec![insert(to), delete(from + n)],
        (true, false) => vec![insert(to), delete(from)],
        (false, true) => vec![delete(from), insert(to)],
        (false, false) => vec![delete(from), insert(to - n)],
    };
    Some(Step::Nested {
        island: String::new(),
        slot: String::new(),
        change: Change::new(steps),
    })
}

/// A change built against `d`, each step kept only where the value it reads
/// takes it.
fn change_for(d: &Content, ops: &[Op]) -> Change {
    change_from(d, ops, 0)
}

/// [`change_for`], the ids it mints numbered from `k0`.
fn change_from(d: &Content, ops: &[Op], k0: usize) -> Change {
    let mut sim = d.clone();
    let mut steps = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        let Some(step) = step_for(&sim, *op, k0 + k) else {
            continue;
        };
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
    let mut out: Vec<(Pos, char)> = chars(c)
        .into_iter()
        .enumerate()
        .map(|(i, ch)| (Pos::At(i), ch))
        .collect();
    for island in &c.islands {
        for (slot, content) in &island.slots {
            for (i, ch) in chars(content).into_iter().enumerate() {
                out.push((
                    Pos::In {
                        island: island.id.clone(),
                        slot: slot.clone(),
                        at: Box::new(Pos::At(i)),
                    },
                    ch,
                ));
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
        Pos::In { island, slot, at } => Pos::In {
            island: island.clone(),
            slot: slot.clone(),
            at: Box::new(next(at)),
        },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// The canonical bytes written as the value is read, which the content
    /// hash digests, are the bytes of the key-sorted JSON tree, opaque bags
    /// and their numbers and escapes included.
    #[test]
    fn canonical_bytes_stream_as_the_tree_serializes(d in document(), n in any::<i64>(), f in any::<f64>(), s in "\\PC{0,6}") {
        let mut d = d;
        let attrs = json!({"z": f.is_finite().then_some(f), "a": {"y": [n, null, true, -0.0], "b": s.clone()}, "m": [{"q": 1, "c": 2}]});
        let len = d.len_scalars();
        if len > 0 {
            d.marks.push(Mark::new(0, len, MarkKind::Ext { name: "x-z".into(), attrs: attrs.as_object().unwrap().clone() }));
        }
        let mut streamed = Vec::new();
        d.write_canonical(&mut streamed).unwrap();
        prop_assert_eq!(String::from_utf8(streamed).unwrap(), serde_json::to_string(&d.to_value()).unwrap());
    }

    #[test]
    fn every_change_inverts_exactly(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let after = apply(&d, &c).unwrap();
        let undo = invert(&c, &d).unwrap();
        let back = apply(&after.content, &undo);
        prop_assert!(back.is_ok(), "undo refused: {:?}\nchange {:?}\nundo {:?}", back, c, undo);
        prop_assert_eq!(back.unwrap().content, d, "change {:?}", c);
        prop_assert!(Mapping::from_change(&undo).is_ok(), "the inverse does not map: {:?}", undo);
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
        let mut caret = at % (d.len_scalars() + 1);
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

    /// Each character either goes, which the two edges around it both say,
    /// or keeps its place: the edges land around it, and the survivors keep
    /// their order. One in a slot the change neither edits nor leaves
    /// otherwise, its island moved or not, keeps its place.
    #[test]
    fn a_mapped_position_lands_on_its_character(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let after = apply(&d, &c).unwrap();
        let mut last: Option<usize> = None;
        for (p, ch) in positions(&d) {
            let q = map_pos(&after.applied, &p, Assoc::After).unwrap();
            let r = map_pos(&after.applied, &next(&p), Assoc::Before).unwrap();
            prop_assert_eq!(q.deleted, r.deleted, "{:?} under {:?}", p, c);
            if let Pos::In { island, slot, .. } = &p {
                let edited = c.steps.iter().any(|s| matches!(s, Step::Nested { island: i, slot: k, .. } if i == island && k == slot));
                let kept = !edited && after.content.slot(island, slot).is_some_and(|now| Some(now) == d.slot(island, slot));
                prop_assert!(!(kept && q.deleted), "{:?} in a slot the change kept, under {:?}", p, c);
            }
            if q.deleted {
                prop_assert!(q.deleted_after && r.deleted_before);
                continue;
            }
            prop_assert_eq!(next(&q.pos), r.pos.clone());
            prop_assert_eq!(char_at(&after.content, &q.pos), Some(ch), "{:?} → {:?} under {:?}", p, q, c);
            if let Pos::At(n) = q.pos {
                prop_assert!(last.is_none_or(|l| l < n));
                last = Some(n);
            }
        }
    }

    /// A change's maps, then their inverses each linked to its mirror, take
    /// every position home: one the change deleted is put back where it was.
    #[test]
    fn a_change_and_its_mirrored_inverse_map_every_position_home(d in document(), ops in ops()) {
        let after = apply(&d, &change_for(&d, &ops)).unwrap();
        let forward = Mapping::from_change(&after.applied).unwrap();
        let n = forward.len();
        let mut both = forward.clone();
        both.append_mapping_inverted(&forward);
        for i in 0..n {
            both.set_mirror(i, 2 * n - 1 - i);
        }
        let edges = (0..=d.len_scalars()).map(Pos::At);
        for p in positions(&d).into_iter().map(|(p, _)| p).chain(edges) {
            for assoc in [Assoc::Before, Assoc::After] {
                prop_assert_eq!(&both.map(&p, assoc).pos, &p, "{:?} under {:?}", assoc, after.applied);
            }
        }
        prop_assert_eq!(forward.invert().invert(), forward);
    }
}

#[test]
fn a_deleted_position_lands_on_the_deletions_edge_and_says_so() {
    let c = Change::new(vec![Step::Splice(Splice::new(2, "cdef", "XY"))]);
    let at = |p, assoc| {
        let r = map_pos(&c, &Pos::At(p), assoc).unwrap();
        (r.pos.clone(), flags(&r))
    };
    assert_eq!(at(4, Assoc::Before), (Pos::At(2), (true, true, true, true)));
    assert_eq!(at(4, Assoc::After), (Pos::At(4), (true, true, true, true)));
    assert_eq!(
        at(2, Assoc::After),
        (Pos::At(4), (true, false, true, false))
    );
    assert_eq!(
        at(2, Assoc::Before),
        (Pos::At(2), (false, false, true, false))
    );
    assert_eq!(
        at(6, Assoc::Before),
        (Pos::At(2), (true, true, false, false))
    );
    assert_eq!(
        at(7, Assoc::Before),
        (Pos::At(5), (false, false, false, false))
    );

    let d = from_markdown("a\n\n| x | y |\n| - | - |\n| p | q |")
        .unwrap()
        .content;
    let slot = d.text.chars().position(|ch| ch == ISLAND_SLOT).unwrap();
    let island = &d.islands[0];
    let cell = Pos::In {
        island: island.id.clone(),
        slot: island.slot_order()[0].clone(),
        at: Box::new(Pos::At(1)),
    };
    let gone = Change::new(vec![Step::Splice(Splice::new(
        slot,
        ISLAND_SLOT.to_string(),
        "",
    ))]);
    assert!(matches!(
        map_pos(&gone, &cell, Assoc::After),
        Err(ApplyError::BadStep { step: 0, .. })
    ));
    let r = map_pos(&apply(&d, &gone).unwrap().applied, &cell, Assoc::After).unwrap();
    assert_eq!(
        (r.pos.clone(), flags(&r)),
        (Pos::At(slot), (true, true, true, true))
    );
}

#[test]
fn a_step_sets_and_removes_a_reserved_name_and_refuses_another_unknown_one() {
    let d = from_markdown("hello world").unwrap().content;
    let c = Change::from_json(
        r#"{"steps": [{"setBlock": {"id": "b0", "kind": "x-callout", "attrs": {"tone": "warm"}}},
                      {"mark": {"start": 0, "end": 5, "type": "x-glow", "add": true}},
                      {"mark": {"start": 0, "end": 2, "type": "x-glow", "add": false}}]}"#,
    )
    .unwrap();
    let done = apply(&d, &c).unwrap().content;
    assert_eq!(done.blocks[0].kind.tag(), "x-callout");
    assert_eq!(
        done.marks,
        [Mark::new(
            2,
            5,
            MarkKind::Ext {
                name: "x-glow".into(),
                attrs: Default::default()
            }
        )]
    );
    assert_eq!(apply(&done, &invert(&c, &d).unwrap()).unwrap().content, d);

    let unknown = r#"{"steps": [{"mark": {"start": 0, "end": 2, "type": "glow", "add": false}}]}"#;
    assert!(matches!(
        Change::from_json(unknown),
        Err(quillmark_content::wire::ParseError::UnknownName { .. })
    ));
    let unknown = Change::new(vec![Step::SetBlock(SetBlock {
        id: "b0".into(),
        kind: Some("callout".into()),
        ..SetBlock::default()
    })]);
    assert!(matches!(
        apply(&d, &unknown),
        Err(ApplyError::BadStep { step: 0, .. })
    ));
}

/// A step, or a record or change holding one, with a key its kind does not
/// define is refused, so a misspelled precondition never decodes as a step
/// without it; the `props` bag and an `x-` member's `attrs` stay open.
#[test]
fn a_step_holding_a_key_it_does_not_define_is_refused() {
    let steps = [
        json!({"splice": {"at": 0, "delete": "he", "insert": "x", "marks": [{"start": 0, "end": 1, "type": "x-glow", "attrs": {"x-any": 1}}], "removed": {}}}),
        json!({"setBlock": {"id": "b0", "kind": "x-callout", "attrs": {"tone": "warm", "x-any": 1}, "path": []}}),
        json!({"container": {"id": "l9", "kind": "list", "attrs": {"ordered": true}}}),
        json!({"container": {"id": "l9", "remove": true}}),
        json!({"mark": {"start": 0, "end": 2, "type": "strong", "add": true, "quote": "he"}}),
        json!({"anchor": {"id": "c1", "start": 0, "end": 2, "quote": "he"}}),
        json!({"anchor": {"id": "c1", "remove": true}}),
        json!({"island": {"at": 0, "island": {"id": "m0", "type": "math", "props": {"tex": "y", "x-any": 1}}, "block": "b0"}}),
        json!({"island": {"id": "m0", "props": {"tex": "y"}, "slots": {}}}),
        json!({"nested": {"island": "m0", "slot": "s0", "change": {"steps": []}}}),
    ];
    for step in steps {
        assert!(
            Change::from_value(&json!({"steps": [step.clone()]})).is_ok(),
            "{step}"
        );
        let (name, body) = step.as_object().unwrap().iter().next().unwrap();
        let mut wrong = body.clone();
        wrong["qoute"] = json!("he");
        let refused = Change::from_value(&json!({"steps": [{name: wrong}]}));
        assert!(
            matches!(&refused, Err(quillmark_content::wire::ParseError::UnknownKey { key, .. }) if key == "qoute"),
            "{name}: {refused:?}"
        );
    }
    for nested in [
        json!({"splice": {"at": 0, "delet": "he", "insert": "x"}}),
        json!({"splice": {"at": 0, "insert": "x", "marks": [{"type": "strong", "atrs": {}}]}}),
        json!({"splice": {"at": 0, "insert": "x", "removed": {"block": []}}}),
        json!({"splice": {"at": 0, "insert": "\nx", "blocks": [{"id": "b9", "kind": "para", "pth": []}]}}),
        json!({"splice": {"at": 0, "insert": "x", "marks": [{"type": "link", "attrs": {"url": "u", "title": "t"}}]}}),
        json!({"mark": {"start": 0, "end": 2, "type": "link", "attrs": {"url": "u", "rel": "x"}, "add": true}}),
        json!({"container": {"id": "l9", "kind": "list", "attrs": {"style": "roman"}}}),
        json!({"island": {"at": 0, "island": {"id": "m0", "type": "math", "props": {"tex": "y"}, "caption": "x"}}}),
    ] {
        assert!(
            matches!(
                Change::from_value(&json!({"steps": [nested.clone()]})),
                Err(quillmark_content::wire::ParseError::UnknownKey { .. })
            ),
            "{nested}"
        );
    }
    assert!(matches!(
        Change::from_value(&json!({"steps": [], "version": 2})),
        Err(quillmark_content::wire::ParseError::UnknownKey { .. })
    ));
}

#[test]
fn a_typed_run_composes_into_one_splice() {
    let d = from_markdown("ab").unwrap().content;
    let mut total = Change::default();
    let mut doc = d.clone();
    for (at, ch) in [(1, "x"), (2, "y"), (3, "z")] {
        let done = apply(
            &doc,
            &Change::new(vec![Step::Splice(Splice::new(at, "", ch))]),
        )
        .unwrap();
        doc = done.content;
        total = compose(&total, &done.applied);
    }
    let back = apply(
        &doc,
        &Change::new(vec![Step::Splice(Splice::new(3, "z", ""))]),
    )
    .unwrap();
    total = compose(&total, &back.applied);
    assert!(
        matches!(&total.steps[..], [Step::Splice(s)] if s.insert == "xy"),
        "{total:?}"
    );
}

/// Each block's text and id, in order, a heading marked `#`.
fn segments(c: &Content) -> Vec<String> {
    c.text
        .split('\n')
        .zip(&c.blocks)
        .map(|(t, b)| {
            format!(
                "{}{}={t}",
                b.id,
                if matches!(b.kind, BlockKind::Heading { .. }) {
                    "#"
                } else {
                    ""
                }
            )
        })
        .collect()
}

#[test]
fn a_split_keeps_the_text_on_its_id_and_a_join_keeps_the_left_block() {
    let d = from_markdown("# Title\n\nalpha\n\nbody").unwrap().content;
    let split = |at| {
        segments(
            &apply(&d, &split_block(&d, at, Some("n")).unwrap())
                .unwrap()
                .content,
        )
    };
    assert_eq!(split(8), ["b0#=Title", "b1=al", "n=pha", "b2=body"]);
    assert_eq!(split(5), ["b0#=Title", "n#=", "b1=alpha", "b2=body"]);
    assert_eq!(split(11), ["b0#=Title", "b1=alpha", "n=", "b2=body"]);
    assert_eq!(split(16), ["b0#=Title", "b1=alpha", "b2=body", "n="]);
    assert_eq!(split(6), ["b0#=Title", "n=", "b1=alpha", "b2=body"]);
    assert_eq!(split(0), ["b0#=", "n#=Title", "b1=alpha", "b2=body"]);
    let join = |c: &Content, id| segments(&apply(c, &join_blocks(c, id).unwrap()).unwrap().content);
    assert_eq!(join(&d, "b1"), ["b0#=Titlealpha", "b2=body"]);
    assert_eq!(join(&d, "b2"), ["b0#=Title", "b1=alphabody"]);
    assert!(join_blocks(&d, "b0").is_err());
}

/// A block or island id may be held twice between two steps, so a change
/// can insert a record before it deletes the one it moves; a step naming the
/// id then is refused, and an id still held twice at the end fails the change.
#[test]
fn an_id_held_twice_between_steps_is_refused_by_name_and_fails_at_the_end() {
    let d = from_markdown("alpha[^n]\n\nbeta\n\n[^n]: note")
        .unwrap()
        .content;
    let len = d.len_scalars();
    let (block, island) = (d.blocks[1].id.clone(), d.islands[0].clone());
    let copy = Step::Splice(Splice {
        blocks: vec![d.blocks[1].clone()],
        ..Splice::new(len, "", "\ncopy")
    });
    let twin = Step::Island(IslandStep::Insert {
        at: len,
        island: island.clone(),
        block: None,
    });
    let level = json!({"level": 2}).as_object().cloned();
    let heading = Step::SetBlock(SetBlock {
        id: block.clone(),
        kind: Some("heading".into()),
        attrs: level,
        path: None,
    });
    let props = Step::Island(IslandStep::Props {
        id: island.id.clone(),
        props: island.props.clone(),
        slots: Default::default(),
    });
    let into = Step::Nested {
        island: island.id.clone(),
        slot: "note".into(),
        change: Change::default(),
    };
    for (held, named) in [(&copy, &heading), (&twin, &props), (&twin, &into)] {
        assert!(
            apply(&d, &Change::new(vec![named.clone()])).is_ok(),
            "{named:?}"
        );
        let refused = apply(&d, &Change::new(vec![held.clone(), named.clone()]));
        assert!(
            matches!(refused, Err(ApplyError::BadStep { step: 1, .. })),
            "{named:?}: {refused:?}"
        );
    }
    assert_eq!(
        apply(&d, &Change::new(vec![copy.clone()])).map(|_| ()),
        Err(ApplyError::Invalid(Invariant::DuplicateId {
            id: block.clone()
        }))
    );
    let moved = Change::new(vec![
        copy,
        Step::Splice(Splice::new(len - 5, "\nbeta", "")),
        heading,
    ]);
    assert_eq!(
        segments(&apply(&d, &moved).unwrap().content),
        ["b0=alpha\u{FFFC}".to_string(), format!("{block}#=copy")]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn a_split_keeps_every_id_and_its_join_undoes_it(d in document(), at in any::<usize>()) {
        let at = at % (d.len_scalars() + 1);
        let split = apply(&d, &split_block(&d, at, Some("n")).unwrap()).unwrap().content;
        let kept: Vec<&str> = split.blocks.iter().map(|b| b.id.as_str()).filter(|id| *id != "n").collect();
        prop_assert_eq!(kept, d.blocks.iter().map(|b| b.id.as_str()).collect::<Vec<_>>());
        let joined = apply(&split, &join_blocks(&split, "n").unwrap()).unwrap().content;
        prop_assert_eq!(joined, d);
    }
}

#[test]
fn typed_text_grows_each_mark_at_the_edges_the_canon_names() {
    let mut d = from_markdown("a **bold** `code` [link](https://u.io) z\n\nnext")
        .unwrap()
        .content;
    d.marks.push(Mark::new(
        0,
        1,
        MarkKind::FontFamily {
            name: "Inter".into(),
        },
    ));
    d.marks.push(Mark::new(
        19,
        20,
        MarkKind::FontFamily {
            name: "Lora".into(),
        },
    ));
    d.marks.push(Mark::new(
        20,
        23,
        MarkKind::Ext {
            name: "x-glow".into(),
            attrs: Default::default(),
        },
    ));
    d.normalize();
    let tags = |at| {
        inherited_marks(&d, at)
            .unwrap()
            .iter()
            .map(|k| k.tag().to_string())
            .collect::<Vec<_>>()
    };
    let none: [&str; 0] = [];
    assert_eq!(tags(2), none, "strong grows at its end only");
    assert_eq!(tags(4), ["strong"]);
    assert_eq!(tags(6), ["strong"]);
    assert_eq!(tags(7), none, "code grows at neither edge");
    assert_eq!(tags(9), ["code"]);
    assert_eq!(tags(11), none);
    assert_eq!(tags(12), none);
    assert_eq!(tags(14), ["link"]);
    assert_eq!(tags(16), none);
    assert_eq!(
        tags(20),
        ["font_family"],
        "an x- mark grows at neither edge"
    );
    assert_eq!(tags(21), ["x-glow"]);
    assert_eq!(tags(23), none);
    assert_eq!(tags(0), ["font_family"], "a family grows at its start");
    assert_eq!(tags(1), ["font_family"], "and at its end");
    assert_eq!(
        tags(19),
        ["font_family"],
        "past the block's opening break, from the text after"
    );
    assert_eq!(
        inherited_marks(&d, d.len_scalars() + 1),
        Err(ApplyError::Stale { step: 0 })
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Typing carries exactly the marks `inherited_marks` names, and typing
    /// on after it carries the same, so a typed run holds one formatting.
    #[test]
    fn typed_text_carries_its_inherited_marks_and_passes_them_on(
        mut d in document(), at in any::<usize>(), (x, y, z) in (any::<usize>(), any::<usize>(), any::<usize>()),
    ) {
        let len = d.len_scalars() + 1;
        let (x, y, z) = (x % len, y % len, z % len);
        d.marks.push(Mark::new(x.min(y), x.max(y), MarkKind::FontFamily { name: "Inter".into() }));
        d.marks.push(Mark::new(y.min(z), y.max(z), MarkKind::Code));
        d.marks.retain(|m| m.start < m.end);
        d.normalize();
        prop_assume!(d.validate().is_ok());
        let at = at % len;
        let inherited = inherited_marks(&d, at).unwrap();
        let marks = inherited.iter().map(|k| Mark::new(0, 1, k.clone())).collect();
        let typed = Change::new(vec![Step::Splice(Splice { marks, ..Splice::new(at, "", "x") })]);
        let Ok(done) = apply(&d, &typed) else { return Ok(()) };
        let mut held: Vec<MarkKind> =
            done.content.marks.iter().filter(|m| m.start <= at && at < m.end).map(|m| m.kind.clone()).collect();
        held.sort_by_key(MarkKind::sort_key);
        prop_assert_eq!(&held, &inherited);
        prop_assert_eq!(inherited_marks(&done.content, at + 1).unwrap(), inherited);
    }
}

#[test]
fn a_wholly_deleted_range_anchor_is_dropped_and_a_point_collapses() {
    let mut d = from_markdown("keep this text").unwrap().content;
    d.anchors = vec![
        Anchor::new("gone", 5, 9),
        Anchor::new("pt", 7, 7),
        Anchor::new("cut", 3, 7),
    ];
    let done = apply(
        &d,
        &Change::new(vec![Step::Splice(Splice::new(5, "this", "that"))]),
    )
    .unwrap();
    assert_eq!(
        done.dropped
            .iter()
            .map(|a| a.context.id.as_str())
            .collect::<Vec<_>>(),
        ["gone"]
    );
    assert_eq!(done.content.anchor("pt"), Some(&Anchor::new("pt", 5, 5)));
    assert_eq!(done.content.anchor("cut"), Some(&Anchor::new("cut", 3, 5)));
}

#[test]
fn a_quoted_step_refuses_the_text_it_did_not_read() {
    let one = |s: &Step| Change::new(vec![s.clone()]);
    let d = from_markdown("hello\n\nworld").unwrap().content;
    let typed = apply(&d, &one(&Step::Splice(Splice::new(0, "", "XX "))))
        .unwrap()
        .content;
    let joined = apply(&d, &one(&Step::Splice(Splice::new(5, "\n", ""))))
        .unwrap()
        .content;
    let math = Island::new("isl-9", IslandType::Math).with_props(json!({"tex": "y"}));
    let cases = [
        (
            Step::Anchor(AnchorStep::Place {
                anchor: Anchor::new("c-1", 6, 11),
                quote: Some("world".into()),
            }),
            &typed,
        ),
        (
            Step::Mark(MarkStep::Add {
                mark: Mark::new(6, 11, MarkKind::Strong),
                quote: Some("world".into()),
            }),
            &typed,
        ),
        (
            Step::Mark(MarkStep::Remove {
                start: 6,
                end: 11,
                tag: "strong".into(),
                quote: Some("world".into()),
            }),
            &typed,
        ),
        (
            Step::Island(IslandStep::Insert {
                at: 8,
                island: math,
                block: Some(d.blocks[1].id.clone()),
            }),
            &joined,
        ),
    ];
    for (step, moved) in cases {
        assert!(apply(&d, &one(&step)).is_ok(), "{step:?}");
        assert_eq!(
            apply(moved, &one(&step)).map(|_| ()),
            Err(ApplyError::Stale { step: 0 }),
            "{step:?}"
        );
        let unguarded = match step {
            Step::Anchor(AnchorStep::Place { anchor, .. }) => Step::Anchor(AnchorStep::Place {
                anchor,
                quote: None,
            }),
            Step::Mark(MarkStep::Add { mark, .. }) => {
                Step::Mark(MarkStep::Add { mark, quote: None })
            }
            Step::Mark(MarkStep::Remove {
                start, end, tag, ..
            }) => Step::Mark(MarkStep::Remove {
                start,
                end,
                tag,
                quote: None,
            }),
            Step::Island(IslandStep::Insert { at, island, .. }) => {
                Step::Island(IslandStep::Insert {
                    at,
                    island,
                    block: None,
                })
            }
            step => step,
        };
        assert!(apply(moved, &one(&unguarded)).is_ok(), "{unguarded:?}");
    }
}

#[test]
fn the_receipt_and_the_inverse_carry_each_steps_precondition() {
    let d = from_markdown("hello world").unwrap().content;
    let c = Change::new(vec![
        Step::Anchor(AnchorStep::Place {
            anchor: Anchor::new("c-1", 6, 11),
            quote: None,
        }),
        Step::Mark(MarkStep::Add {
            mark: Mark::new(0, 5, MarkKind::Strong),
            quote: None,
        }),
        Step::Island(IslandStep::Insert {
            at: 5,
            island: Island::new("isl-9", IslandType::Math).with_props(json!({"tex": "y"})),
            block: None,
        }),
    ]);
    let done = apply(&d, &c).unwrap();
    assert_eq!(
        done.applied.steps[0],
        Step::Anchor(AnchorStep::Place {
            anchor: Anchor::new("c-1", 6, 11),
            quote: Some("world".into())
        })
    );
    assert_eq!(
        done.applied.steps[1],
        Step::Mark(MarkStep::Add {
            mark: Mark::new(0, 5, MarkKind::Strong),
            quote: Some("hello".into())
        })
    );
    assert!(
        matches!(&done.applied.steps[2], Step::Island(IslandStep::Insert { block: Some(b), .. }) if *b == d.blocks[0].id)
    );
    let undo = invert(&c, &d).unwrap();
    let guarded = |s: &Step| match s {
        Step::Mark(MarkStep::Add { quote, .. } | MarkStep::Remove { quote, .. })
        | Step::Anchor(AnchorStep::Place { quote, .. } | AnchorStep::Move { quote, .. }) => {
            quote.is_some()
        }
        _ => true,
    };
    assert!(undo.steps.iter().all(guarded), "{undo:?}");
    assert!(undo.steps.iter().any(|s| matches!(s, Step::Mark(_))));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// A quoted step lands exactly where its range still covers its quote,
    /// and the same step unquoted lands wherever its range fits.
    #[test]
    fn a_quote_holds_a_step_to_the_text_it_read(d in document(), (x, y, z) in (any::<usize>(), any::<usize>(), any::<u8>()), pre in (any::<usize>(), any::<usize>(), any::<u8>())) {
        let len = d.len_scalars();
        let (a, b) = (x % (len + 1), y % (len + 1));
        let (start, end) = (a.min(b), a.max(b));
        let quote: String = chars(&d)[start..end].iter().collect();
        let step = |quote: Option<String>| match z % 3 {
            0 => Step::Anchor(AnchorStep::Place { anchor: Anchor::new("c-9", start, end), quote }),
            1 => Step::Mark(MarkStep::Add { mark: Mark::new(start, end, MarkKind::Emph), quote }),
            _ => Step::Mark(MarkStep::Remove { start, end, tag: "strong".into(), quote }),
        };
        let Ok(mid) = apply(&d, &Change::new(vec![Step::Splice(splice_on(&d, pre.0, pre.1, pre.2, 0))])) else { return Ok(()) };
        let now: Option<String> = (end <= mid.content.len_scalars()).then(|| chars(&mid.content)[start..end].iter().collect());
        let unquoted = apply(&mid.content, &Change::new(vec![step(None)]));
        match apply(&mid.content, &Change::new(vec![step(Some(quote.clone()))])) {
            Ok(done) => {
                prop_assert_eq!(now, Some(quote));
                prop_assert_eq!(Ok(done.content), unquoted.map(|u| u.content));
            }
            Err(ApplyError::Stale { step: 0 }) => prop_assert_ne!(now, Some(quote)),
            Err(e) => prop_assert!(unquoted.is_err(), "{:?}", e),
        }
    }
}

fn anchor_ids(c: &Content, out: &mut BTreeSet<String>) {
    out.extend(c.anchors.iter().map(|a| a.id.clone()));
    for island in &c.islands {
        island.slots.values().for_each(|slot| anchor_ids(slot, out));
    }
}

fn removed_by_steps(steps: &[Step], out: &mut BTreeSet<String>) {
    for step in steps {
        match step {
            Step::Anchor(AnchorStep::Remove { id }) => {
                out.insert(id.clone());
            }
            Step::Nested { change, .. } => removed_by_steps(&change.steps, out),
            _ => {}
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Every anchor held before a change and nowhere after it, in the text or
    /// a slot, is named in `dropped` once, with the slot that held it, unless
    /// an `anchor` step removed it.
    #[test]
    fn every_anchor_a_change_loses_is_dropped(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let done = apply(&d, &c).unwrap();
        let (mut before, mut after, mut removed) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        anchor_ids(&d, &mut before);
        anchor_ids(&done.content, &mut after);
        removed_by_steps(&c.steps, &mut removed);
        let lost: BTreeSet<String> = before.difference(&after).filter(|id| !removed.contains(*id)).cloned().collect();
        let named: Vec<&str> = done.dropped.iter().map(|a| a.context.id.as_str()).collect();
        prop_assert_eq!(named.iter().copied().collect::<BTreeSet<_>>().len(), named.len());
        prop_assert_eq!(named.iter().map(|id| id.to_string()).collect::<BTreeSet<_>>(), lost, "{:?}", c);
        for a in &done.dropped {
            let home = match &a.slot {
                Some(s) => d.slot(&s.island, &s.slot).unwrap(),
                None => &d,
            };
            prop_assert!(home.anchor(&a.context.id).is_some(), "{:?}", a);
        }
    }
}

/// Each anchor of `c`, in the text or a slot: the slots it sits in, its
/// range, the id of the block its start sits in, and the eight characters
/// before it, its text and the eight after.
type Read = (Vec<(String, String)>, (usize, usize), String, [String; 3]);

fn reads(
    c: &Content,
    path: &[(String, String)],
    out: &mut std::collections::BTreeMap<String, Read>,
) {
    let cs = chars(c);
    let text = |a: usize, b: usize| cs[a..b].iter().collect::<String>();
    for a in &c.anchors {
        let block = c.blocks[cs[..a.start].iter().filter(|&&ch| ch == '\n').count()]
            .id
            .clone();
        let around = [
            text(a.start.saturating_sub(8), a.start),
            text(a.start, a.end),
            text(a.end, (a.end + 8).min(cs.len())),
        ];
        out.insert(
            a.id.clone(),
            (path.to_vec(), (a.start, a.end), block, around),
        );
    }
    for island in &c.islands {
        for (slot, inner) in &island.slots {
            let mut deeper = path.to_vec();
            deeper.push((island.id.clone(), slot.clone()));
            reads(inner, &deeper, out);
        }
    }
}

fn stepped(steps: &[Step], out: &mut BTreeSet<String>) {
    for s in steps {
        match s {
            Step::Anchor(a @ (AnchorStep::Place { .. } | AnchorStep::Move { .. })) => {
                out.insert(a.id().to_string());
            }
            Step::Nested { change, .. } => stepped(&change.steps, out),
            _ => {}
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Every anchor a change keeps is named in `moved` once where it moved,
    /// a point included, with the text it covered before and after and the
    /// first of these that holds: an `anchor` step put it other than where
    /// the change's text edits carry it (`position`), its start sits in
    /// another block (`block`), or its text or the eight characters either
    /// side read otherwise (`context`).
    #[test]
    fn every_anchor_a_change_moves_is_named_with_what_moved(d in document(), ops in ops()) {
        let c = change_for(&d, &ops);
        let done = apply(&d, &c).unwrap();
        let (mut before, mut after) = (Default::default(), Default::default());
        reads(&d, &[], &mut before);
        reads(&done.content, &[], &mut after);
        let mut steps = BTreeSet::new();
        stepped(&c.steps, &mut steps);
        let map = Mapping::from_change(&done.applied).unwrap();
        let local = |mut p: Pos| loop {
            match p {
                Pos::At(p) => return p,
                Pos::In { at, .. } => p = *at,
            }
        };
        let want: Vec<(String, String, String, &str)> = after
            .into_iter()
            .filter_map(|(id, (path, range, block, around))| {
                let (wpath, (s, e), wblock, waround) = before.get(&id)?.clone();
                let at = |p: usize| wpath.iter().rev().fold(Pos::At(p), |inner, (i, k)| Pos::In { island: i.clone(), slot: k.clone(), at: Box::new(inner) });
                let carried = match s == e {
                    true => {
                        let p = local(map.map_point(&at(s)).pos);
                        (p, p)
                    }
                    false => (local(map.map(&at(s), Assoc::After).pos), local(map.map(&at(e), Assoc::Before).pos)),
                };
                let reason = if steps.contains(&id) && (wpath != path || carried != range) {
                    "position"
                } else if wblock != block {
                    "block"
                } else if waround != around {
                    "context"
                } else {
                    return None;
                };
                Some((id, waround[1].clone(), around[1].clone(), reason))
            })
            .collect();
        let got: Vec<(String, String, String, &str)> = done.moved.into_iter().map(|m| (m.id, m.before, m.after, m.reason.name())).collect();
        prop_assert_eq!(got, want, "{:?}", c);
    }
}

/// A receipt names a point a step moved as moved by position, one whose
/// block split off as moved by block, and one whose neighbours were
/// retyped as moved by context; an anchor with nothing near the edit is not
/// named.
#[test]
fn a_receipt_names_what_moved_about_each_anchor_points_included() {
    let mut d = from_markdown("Alpha beta gamma delta epsilon zeta eta theta")
        .unwrap()
        .content;
    d.anchors = vec![
        Anchor::new("p", 6, 6),
        Anchor::new("q", 40, 40),
        Anchor::new("r", 17, 22),
        Anchor::new("far", 0, 5),
    ];
    d.normalize();
    let change = Change::new(vec![
        Step::Anchor(AnchorStep::Move {
            anchor: Anchor::new("p", 11, 11),
            quote: None,
        }),
        Step::Splice(Splice {
            blocks: vec![Block::new("b9", BlockKind::Para)],
            ..Splice::new(35, " ", "\n")
        }),
        Step::Splice(Splice::new(23, "", "!")),
    ]);
    let done = apply(&d, &change).unwrap();
    let got: Vec<(&str, &str, &str, &str)> = done
        .moved
        .iter()
        .map(|m| {
            (
                m.id.as_str(),
                m.before.as_str(),
                m.after.as_str(),
                m.reason.name(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("p", "", "", "position"),
            ("q", "", "", "block"),
            ("r", "delta", "delta", "context")
        ]
    );
}

/// Deleting an island's slot character, or dropping a table row with a props
/// step, names each anchor its slots held with the island and the slot.
#[test]
fn an_island_takes_the_anchors_of_its_slots_into_the_receipt() {
    let mut d = from_markdown("intro[^n]\n\n| a | b |\n| - | - |\n| c | d |\n\n[^n]: a note")
        .unwrap()
        .content;
    let homes: Vec<(String, String)> = d
        .islands
        .iter()
        .map(|i| (i.id.clone(), i.slot_order().last().unwrap().clone()))
        .collect();
    for (n, (island, slot)) in homes.iter().enumerate() {
        d.slot_mut(island, slot)
            .unwrap()
            .anchors
            .push(Anchor::new(format!("a{n}"), 0, 1));
    }
    d.validate().unwrap();
    let slot_of = |done: &quillmark_content::change::Applied| -> Vec<(String, String, String)> {
        done.dropped
            .iter()
            .map(|a| {
                let s = a.slot.clone().unwrap();
                (a.context.id.clone(), s.island, s.slot)
            })
            .collect()
    };
    let at = d.text.chars().position(|ch| ch == ISLAND_SLOT).unwrap();
    let cut = apply(
        &d,
        &Change::new(vec![Step::Splice(Splice::new(
            at,
            ISLAND_SLOT.to_string(),
            "",
        ))]),
    )
    .unwrap();
    assert_eq!(
        slot_of(&cut),
        [("a0".to_string(), homes[0].0.clone(), homes[0].1.clone())]
    );
    assert_eq!(cut.dropped[0].context.quote, "a");

    let table = &d.islands[1];
    let mut grid = Table::read(&table.props).unwrap();
    grid.rows.pop();
    let mut props = table.props.as_object().unwrap().clone();
    grid.write_into(&mut props);
    let props = Step::Island(IslandStep::Props {
        id: table.id.clone(),
        props: props.into(),
        slots: Default::default(),
    });
    let shrunk = apply(&d, &Change::new(vec![props])).unwrap();
    assert_eq!(
        slot_of(&shrunk),
        [("a1".to_string(), homes[1].0.clone(), homes[1].1.clone())]
    );
}

/// A props step's `slots` holds only slots the island gains: naming one it
/// keeps, or one the props do not name, refuses the step.
#[test]
fn a_props_step_gives_content_only_to_a_slot_it_adds() {
    let d = from_markdown("| a | b |\n| - | - |\n| c | d |")
        .unwrap()
        .content;
    let table = &d.islands[0];
    let held = table.slot_order()[0].clone();
    for (slot, reason) in [
        (held, "`slots` names a slot the island already holds"),
        (
            "elsewhere".to_string(),
            "`slots` names a slot the props do not name",
        ),
    ] {
        let step = Step::Island(IslandStep::Props {
            id: table.id.clone(),
            props: table.props.clone(),
            slots: [(slot, quillmark_content::markdown::from_plaintext("x"))].into(),
        });
        assert_eq!(
            apply(&d, &Change::new(vec![step])).unwrap_err(),
            ApplyError::BadStep { step: 0, reason }
        );
    }
}

#[test]
fn a_break_in_a_heading_fails_the_change() {
    let d = from_markdown("# title").unwrap().content;
    let c = Change::new(vec![Step::Splice(Splice::new(
        2,
        "",
        HARD_BREAK.to_string(),
    ))]);
    assert!(apply(&d, &c).is_err());
}

#[test]
fn insertion_never_grows_a_mark() {
    let d = from_markdown("a **bold** b").unwrap().content;
    let c = Change::new(vec![
        Step::Splice(Splice::new(4, "", "X")),
        Step::Splice(Splice::new(6, "", "Y")),
    ]);
    let done = apply(&d, &c).unwrap().content;
    assert_eq!(done.text, "a boXlYd b");
    assert_eq!(
        done.marks,
        [
            Mark::new(2, 4, MarkKind::Strong),
            Mark::new(5, 6, MarkKind::Strong),
            Mark::new(7, 8, MarkKind::Strong)
        ]
    );
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
        assert_eq!(
            Change::from_json(&done.applied.to_json()).unwrap(),
            done.applied
        );
        assert_eq!(
            apply(&done.content, &invert(&c, &d).unwrap())
                .unwrap()
                .content,
            d
        );
        results.push(done.content);
    }
    assert_eq!(results[0].marks[0], Mark::new(28, 32, MarkKind::Strong));
    assert_eq!(
        results[1]
            .blocks
            .iter()
            .map(|b| b.id.as_str())
            .collect::<Vec<_>>(),
        ["b0", "b1", "b5", "b2", "b3", "b4"]
    );
    assert_eq!(results[3].blocks[3].path, ["l0", "i0", "l1", "i1"]);
    assert_eq!(results[4].slot("t0", "s3").unwrap().text, "4.3M");
    assert_eq!(
        results[5].anchor("c-91be").map(|a| (a.start, a.end)),
        Some((33, 40))
    );
}

#[test]
fn deleting_the_last_block_of_an_item_drops_its_containers() {
    let d = memo();
    let c = Change::new(vec![Step::Splice(Splice::new(40, "\nDraft spec", ""))]);
    let done = apply(&d, &c).unwrap();
    assert!(done.content.container("i1").is_none());
    assert!(
        matches!(done.applied.steps.last(), Some(Step::Container(ContainerStep::Remove { id })) if id == "i1")
    );
    assert_eq!(
        apply(&done.content, &invert(&c, &d).unwrap())
            .unwrap()
            .content,
        d
    );
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
    let id = || {
        prop_oneof![
            Just("b0".to_string()),
            Just("l0".to_string()),
            Just("i0".to_string()),
            "[a-z0-9]{1,3}"
        ]
    };
    prop_oneof![
        (
            n(),
            "[a \n\u{2028}\u{FFFC}]{0,3}",
            "[a \n\u{2028}\u{FFFC}]{0,3}",
            prop::collection::vec(id(), 0..3),
            n(),
            n()
        )
            .prop_map(|(at, delete, insert, ids, s, e)| Step::Splice(Splice {
                marks: vec![Mark::new(s, e, MarkKind::Emph)],
                blocks: ids
                    .into_iter()
                    .map(|i| Block::new(i, BlockKind::Para))
                    .collect(),
                ..Splice::new(at, delete, insert)
            })),
        (n(), n(), any::<bool>()).prop_map(|(s, e, add)| Step::Mark(if add {
            MarkStep::Add {
                mark: Mark::new(s, e, MarkKind::Strong),
                quote: None,
            }
        } else {
            MarkStep::Remove {
                start: s,
                end: e,
                tag: "strong".into(),
                quote: Some("a".into()),
            }
        })),
        (id(), n(), n(), any::<bool>()).prop_map(|(i, s, e, place)| Step::Anchor(match place {
            true => AnchorStep::Place {
                anchor: Anchor::new(i, s, e),
                quote: None
            },
            false => AnchorStep::Move {
                anchor: Anchor::new(i, s, e),
                quote: None
            },
        })),
        id().prop_map(|i| Step::Anchor(AnchorStep::Remove { id: i })),
        (id(), prop::collection::vec(id(), 0..4)).prop_map(|(i, path)| Step::SetBlock(SetBlock {
            id: i,
            path: Some(path),
            ..SetBlock::default()
        })),
        id().prop_map(|i| Step::Container(ContainerStep::Remove { id: i })),
        id().prop_map(|i| Step::Container(ContainerStep::Set(Container::new(
            i,
            ContainerKind::Quote
        )))),
        (id(), n()).prop_map(|(i, at)| Step::Island(IslandStep::Insert {
            at,
            island: Island::new(i, IslandType::Table),
            block: None
        })),
        id().prop_map(|i| Step::Island(IslandStep::Props {
            id: i,
            props: json!({"columns": ["k"], "rows": [{"id": "r", "cells": {"k": "s"}}]}),
            slots: Default::default()
        })),
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
    use quillmark_content::diff::{align, diff, lift, replace, revise};
    use quillmark_content::markdown::{
        Loss, to_markdown, to_markdown_annotated, to_markdown_checked,
    };
    use quillmark_content::migrate::to_lines;

    /// The two equal once every block, container and island id is renamed by
    /// where it first appears.
    fn ids_reached(_before: &Content, want: &Content, got: &Content) -> bool {
        fn erased(c: &Content) -> Content {
            let mut c = c.clone();
            let names: std::collections::HashMap<String, String> = c
                .containers
                .iter()
                .enumerate()
                .map(|(n, k)| (k.id.clone(), format!("k{n}")))
                .collect();
            for (n, b) in c.blocks.iter_mut().enumerate() {
                b.id = format!("b{n}");
                b.path.iter_mut().for_each(|id| *id = names[id].clone());
            }
            c.containers
                .iter_mut()
                .for_each(|k| k.id = names[&k.id].clone());
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

    /// `c` with each `x-` block a paragraph, each `x-` mark dropped and each
    /// `x-` island gone.
    fn without_ext(mut c: Content) -> Content {
        let ext_slots: Vec<usize> = c
            .islands
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i.island_type, IslandType::Ext(_)))
            .map(|(n, _)| n)
            .collect();
        for n in ext_slots.into_iter().rev() {
            let at = c
                .text
                .chars()
                .enumerate()
                .filter(|(_, ch)| *ch == ISLAND_SLOT)
                .nth(n)
                .unwrap()
                .0;
            let step = Step::Splice(Splice::new(at, ISLAND_SLOT.to_string(), ""));
            c = apply(&c, &Change::new(vec![step])).unwrap().content;
        }
        for b in &mut c.blocks {
            if matches!(b.kind, BlockKind::Ext { .. }) {
                b.kind = BlockKind::Para;
            }
        }
        c.marks.retain(|m| !matches!(m.kind, MarkKind::Ext { .. }));
        for island in &mut c.islands {
            for slot in island.slots.values_mut() {
                slot.marks
                    .retain(|m| !matches!(m.kind, MarkKind::Ext { .. }));
            }
        }
        c.normalize();
        c
    }

    fn without_anchors(mut c: Content) -> Content {
        c.anchors.clear();
        for island in &mut c.islands {
            for slot in island.slots.values_mut() {
                *slot = without_anchors(std::mem::take(slot));
            }
        }
        c
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn a_diff_reaches_its_target(a in document(), b in document()) {
            let c = diff(&a, &b).unwrap();
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
            let done = apply(&d, &diff(&d, &aligned).unwrap()).unwrap().content;
            prop_assert_eq!(done, aligned);
        }

        /// Over values the model reaches whose checked export names no loss,
        /// markdown-born or not: a revise with the value's own annotated read
        /// writes nothing.
        #[test]
        fn revising_with_the_markdown_read_changes_nothing(d in document(), ops in ops()) {
            for c in [d.clone(), apply(&d, &change_for(&d, &ops)).unwrap().content] {
                if !to_markdown_checked(&c).losses.is_empty() {
                    continue;
                }
                let r = revise(&c, &to_markdown_annotated(&c).markdown).unwrap();
                prop_assert_eq!(apply(&c, &r.change).unwrap().content, c.clone(), "{:?}", r.change);
            }
        }

        /// The line form is a faithful view of every value but for what the
        /// checked export names as its loss: an `x-` member, which it lowers,
        /// and a slot whose breaks it spells otherwise.
        #[test]
        fn the_line_form_keeps_every_value_but_its_named_losses(d in document(), ops in ops()) {
            for c in [d.clone(), apply(&d, &change_for(&d, &ops)).unwrap().content] {
                let c = without_ext(c);
                let mut lifted = lift(&c, &to_lines(&c));
                let mut want = c.clone();
                for loss in to_markdown_checked(&c).losses {
                    let Loss::SlotBreaks { island, slot } = loss else { continue };
                    for v in [&mut lifted, &mut want] {
                        let i = v.islands.iter().position(|i| i.id == island).unwrap();
                        v.islands[i].slots.insert(slot.clone(), Content::empty());
                    }
                }
                prop_assert_eq!(lifted, want);
            }
        }

        #[test]
        fn a_replace_carries_the_anchors_it_does_not_name(d in document(), ops in ops()) {
            let edited = without_anchors(apply(&d, &change_for(&d, &ops)).unwrap().content);
            let done = apply(&d, &replace(&d, &edited).unwrap()).unwrap();
            prop_assert!(ids_reached(&d, &edited, &without_anchors(done.content)));
        }
    }

    #[test]
    fn a_diff_reaches_the_last_record_and_none_the_first() {
        let d = from_markdown("one\n\ntwo").unwrap().content;
        let mut want = from_markdown("one\n\ntwo\n\nthree").unwrap().content;
        want.blocks[0].id = "x0".into();
        want.blocks[1].id = "x1".into();
        want.blocks[2].id = "b1".into();
        let done = apply(&d, &diff(&d, &want).unwrap()).unwrap().content;
        assert_eq!(segments(&done), ["b0=one", "x1=two", "b1=three"]);
    }

    #[test]
    fn a_block_inserted_or_deleted_brings_its_own_opening_break() {
        let d = from_markdown("alpha\n\nbeta").unwrap().content;
        let r = revise(&d, "alpha\n\ngamma\n\nbeta").unwrap();
        assert_eq!(segments(&r.content), ["b0=alpha", "b2=gamma", "b1=beta"]);
        assert!(
            matches!(&r.change.steps[..], [Step::Splice(s)] if s.at == 5 && s.delete.is_empty() && s.insert == "\ngamma"),
            "{:?}",
            r.change
        );
        let back = revise(&r.content, "alpha\n\nbeta").unwrap();
        assert!(
            matches!(&back.change.steps[..], [Step::Splice(s)] if s.at == 5 && s.delete == "\ngamma" && s.insert.is_empty()),
            "{:?}",
            back.change
        );
    }

    /// A revise that moves a paragraph moves its anchor with it, where the
    /// anchor's quote is at least four characters; a shorter one drops.
    #[test]
    fn a_revise_moves_an_anchor_with_its_paragraph() {
        let mut d = from_markdown("First paragraph here.\n\nSecond one there.")
            .unwrap()
            .content;
        d.anchors.push(Anchor::new("c1", 22, 28));
        d.anchors.push(Anchor::new("c2", 29, 31));
        let r = revise(&d, "Second one there.\n\nFirst paragraph here.").unwrap();
        let quote = |id: &str| {
            let a = r.content.anchors.iter().find(|a| a.id == id)?;
            Some(
                r.content
                    .text
                    .chars()
                    .skip(a.start)
                    .take(a.end - a.start)
                    .collect::<String>(),
            )
        };
        assert_eq!(
            (quote("c1").as_deref(), quote("c2")),
            (Some("Second"), None),
            "{:?}",
            r.content.anchors
        );
        assert_eq!(apply(&d, &r.change).unwrap().content, r.content);

        let mut d = from_markdown("Alpha beta gamma.\n\nDelta epsilon zeta.\n\nEta.")
            .unwrap()
            .content;
        d.anchors.push(Anchor::new("c1", 18, 31));
        for md in [
            "Eta.\n\nDelta epsilon zeta.\n\nAlpha beta gamma.",
            "Intro.\n\nAlpha beta gamma.\n\nEta.\n\nNew para holding Delta epsilon zeta. inside.",
        ] {
            let r = revise(&d, md).unwrap();
            let a = r
                .content
                .anchors
                .iter()
                .find(|a| a.id == "c1")
                .unwrap_or_else(|| panic!("{md:?}: {:?}", r.change));
            assert_eq!(
                r.content
                    .text
                    .chars()
                    .skip(a.start)
                    .take(a.end - a.start)
                    .collect::<String>(),
                "Delta epsilon",
                "{md:?}"
            );
        }
    }

    /// An anchor `after` holds in another place, the prose or a cell, moves
    /// there under `replace` as under `diff`, and drops nothing.
    #[test]
    fn an_anchor_moves_between_the_prose_and_a_cell() {
        let mut prose = from_markdown("intro\n\n| a | b |\n| - | - |\n| c | d |")
            .unwrap()
            .content;
        prose.anchors.push(Anchor::new("c1", 0, 3));
        let mut cell = prose.clone();
        cell.anchors.clear();
        let (island, slot) = (
            cell.islands[0].id.clone(),
            cell.islands[0].slot_order()[3].clone(),
        );
        cell.slot_mut(&island, &slot)
            .unwrap()
            .anchors
            .push(Anchor::new("c1", 0, 1));
        for (from, to) in [(&prose, &cell), (&cell, &prose)] {
            for change in [diff(from, to).unwrap(), replace(from, to).unwrap()] {
                let done = apply(from, &change).unwrap();
                assert_eq!(&done.content, to);
                assert!(done.dropped.is_empty(), "{:?}", done.dropped);
            }
        }
    }

    /// A block or island the target holds elsewhere keeps its id, whether it
    /// moves toward the front or the back.
    #[test]
    fn a_block_or_island_a_diff_moves_keeps_its_id_either_way() {
        let d = from_markdown("one\n\ntwo $a$\n\nthree $b$\n\nfour\n\nfive")
            .unwrap()
            .content;
        let moved = |order: [usize; 5]| {
            let texts = d.block_texts();
            let blocks: Vec<Block> = order.iter().map(|&k| d.blocks[k].clone()).collect();
            let text = order
                .iter()
                .map(|&k| texts[k].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let mut islands = d.islands.clone();
            if order[1] == 2 {
                islands.reverse();
            }
            let c = Content {
                text,
                blocks,
                islands,
                ..d.clone()
            };
            c.validate().unwrap();
            c
        };
        for order in [[0, 3, 1, 2, 4], [0, 2, 3, 1, 4], [0, 2, 1, 3, 4]] {
            let after = moved(order);
            let done = apply(&d, &diff(&d, &after).unwrap()).unwrap().content;
            assert_eq!(done, after, "{order:?}");
        }
    }

    /// An `x-` island's slots are its own rather than its props', so a slot
    /// it gains or loses reaches the target, through `diff` and `replace`.
    #[test]
    fn a_diff_reaches_an_x_island_whose_slots_changed() {
        let holding = |slots: &[&str]| {
            let island = Island::new("isl-x", IslandType::Ext("x-chart".into()))
                .with_props(json!({"series": [1, 2]}))
                .with_slots(
                    slots
                        .iter()
                        .map(|s| {
                            (
                                s.to_string(),
                                quillmark_content::markdown::from_plaintext("caption"),
                            )
                        })
                        .collect(),
                );
            let insert = Change::new(vec![Step::Island(IslandStep::Insert {
                at: 5,
                island,
                block: None,
            })]);
            apply(
                &quillmark_content::markdown::from_plaintext("hello world"),
                &insert,
            )
            .unwrap()
            .content
        };
        let (one, two) = (holding(&["cap"]), holding(&["cap", "cap2"]));
        for (from, to) in [(&one, &two), (&two, &one)] {
            for d in [diff(from, to).unwrap(), replace(from, to).unwrap()] {
                assert_eq!(&apply(from, &d).unwrap().content, to);
            }
        }
    }

    /// A target that is no valid value is refused, never diffed to nothing.
    #[test]
    fn a_diff_refuses_an_invalid_target() {
        let before = from_markdown("one").unwrap().content;
        let mut invalid = from_markdown("one\n\ntwo").unwrap().content;
        invalid.blocks[1].id = invalid.blocks[0].id.clone();
        assert!(diff(&before, &invalid).is_err());
        assert!(replace(&before, &invalid).is_err());
    }

    #[test]
    fn a_revise_keeps_the_ids_of_what_it_kept() {
        let d = from_markdown(
            "# Title\n\nKeep this one.\n\n- first\n- second\n\n| a | b |\n| - | - |\n| c | d |",
        )
        .unwrap()
        .content;
        let r = revise(&d, "# Title\n\nKeep this one, now longer.\n\n- first\n- second\n- third\n\n| a | b |\n| - | - |\n| c | e |").unwrap();
        let done = apply(&d, &r.change).unwrap().content;
        assert_eq!(
            &done.blocks[..3].iter().map(|b| &b.id).collect::<Vec<_>>(),
            &d.blocks[..3].iter().map(|b| &b.id).collect::<Vec<_>>()
        );
        assert_eq!(done.blocks[2].path, d.blocks[2].path);
        assert_eq!(done.blocks[4].path[0], d.blocks[3].path[0]);
        assert_eq!(done.islands[0].id, d.islands[0].id);
        assert_eq!(done.islands[0].slot_order(), d.islands[0].slot_order());
        assert!(
            r.change
                .steps
                .iter()
                .any(|s| matches!(s, Step::Nested { .. }))
        );
    }

    /// A held id at the top of `u64` leaves no number past it, and a revise
    /// mints the lowest one no held id takes.
    #[test]
    fn a_revise_past_an_id_at_the_top_of_u64_mints_the_lowest_free_one() {
        let mut d = from_markdown("one\n\ntwo").unwrap().content;
        d.blocks[1].id = format!("b{}", u64::MAX);
        let r = revise(&d, "one\n\nnew\n\ntwo").unwrap();
        assert_eq!(
            segments(&r.content),
            ["b0=one", "b1=new", &format!("b{}=two", u64::MAX)]
        );
    }

    /// Two tables each under the cell limit pass the import's budget for a
    /// document's tables together; a revise with the read keeps every row of
    /// the second, as it does an edit beside them.
    #[test]
    #[ignore = "a hundred thousand cells, minutes in a debug build: run with --release"]
    fn a_revise_keeps_the_rows_its_reads_import_cut() {
        let rows = quillmark_content::MAX_TABLE_CELLS / 4;
        let table = |tag: &str| {
            let body: String = (0..rows).map(|r| format!("| {tag}{r} | x |\n")).collect();
            from_markdown(&format!("| a | b |\n| - | - |\n{body}"))
                .unwrap()
                .content
        };
        let (one, two) = (table("p"), table("q"));
        let mut second = two.islands[0].clone();
        second.id = "isl-1".into();
        let c = Content {
            text: format!("{ISLAND_SLOT}\n{ISLAND_SLOT}\nTail."),
            blocks: vec![
                one.blocks[0].clone(),
                Block::new("b1", BlockKind::Para),
                Block::new("b2", BlockKind::Para),
            ],
            islands: vec![one.islands[0].clone(), second],
            ..Content::empty()
        };
        assert_eq!(c.validate(), Ok(()));
        let read = to_markdown_annotated(&c).markdown;
        let rows_of = |c: &Content| {
            c.islands
                .iter()
                .map(|i| Table::read(&i.props).unwrap().rows.len())
                .collect::<Vec<_>>()
        };
        let cut = from_markdown(&read).unwrap().content;
        assert!(
            rows_of(&cut)[1] < rows + 1,
            "the import cuts the second table"
        );
        let done = apply(
            &c,
            &revise(&c, &read.replace("Tail.", "Tail, edited."))
                .unwrap()
                .change,
        )
        .unwrap()
        .content;
        assert_eq!(rows_of(&done), [rows + 1, rows + 1]);
        assert_eq!(done.islands, c.islands);
    }
}

/// `p` with its offsets in UTF-16 code units, each level on its own text.
fn flags(r: &MapResult) -> (bool, bool, bool, bool) {
    (
        r.deleted,
        r.deleted_before,
        r.deleted_after,
        r.deleted_across,
    )
}

/// Whether `c` holds the slot `p` names.
fn holds(c: &Content, p: &Pos) -> bool {
    match p {
        Pos::At(_) => true,
        Pos::In { island, slot, at } => c.slot(island, slot).is_some_and(|s| holds(s, at)),
    }
}

fn pos_utf16(c: &Content, p: &Pos) -> Pos {
    match p {
        Pos::At(n) => Pos::At(from_scalars(Utf16, &c.text, *n)),
        Pos::In { island, slot, at } => Pos::In {
            island: island.clone(),
            slot: slot.clone(),
            at: Box::new(pos_utf16(
                c.slot(island, slot).expect("the position's slot"),
                at,
            )),
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
                let want = map_pos(&after.applied, &p, assoc).unwrap();
                let got = map_pos_in(&c16, &pos_utf16(&d, &p), assoc, Utf16).unwrap();
                prop_assert_eq!(flags(&got), flags(&want));
                if holds(&after.content, &want.pos) {
                    prop_assert_eq!(got.pos, pos_utf16(&after.content, &want.pos));
                }
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

/// Each block's text and its path as container kinds, outermost first: `L`
/// an unordered list, `O` an ordered one, `I` an item, `Q` a quote.
fn shapes(c: &Content) -> Vec<String> {
    c.text
        .split('\n')
        .zip(&c.blocks)
        .map(|(t, b)| {
            let path: String = b
                .path
                .iter()
                .map(|id| match &c.container(id).unwrap().kind {
                    ContainerKind::List { ordered: false, .. } => 'L',
                    ContainerKind::List { ordered: true, .. } => 'O',
                    ContainerKind::Item { .. } => 'I',
                    ContainerKind::Quote => 'Q',
                })
                .collect();
            format!("{path}:{t}")
        })
        .collect()
}

fn ids(c: &Content) -> Vec<String> {
    c.blocks.iter().map(|b| b.id.clone()).collect()
}

#[test]
fn a_tab_nests_an_item_under_the_one_before_it_and_a_shift_tab_lifts_it() {
    let d = from_markdown("- a\n- b\n  - c\n- d\n\npara")
        .unwrap()
        .content;
    let b = ids(&d);
    let run = |c: Change| shapes(&apply(&d, &c).unwrap().content);
    assert_eq!(
        run(indent(&d, &b[1], &b[1]).unwrap()),
        ["LI:a", "LILI:b", "LILILI:c", "LI:d", ":para"]
    );
    assert_eq!(
        run(indent(&d, &b[3], &b[3]).unwrap()),
        ["LI:a", "LI:b", "LILI:c", "LILI:d", ":para"],
        "d joins b's list"
    );
    assert_eq!(
        run(indent(&d, &b[1], &b[3]).unwrap()),
        ["LI:a", "LILI:b", "LILILI:c", "LILI:d", ":para"]
    );
    assert!(
        indent(&d, &b[0], &b[0]).is_err(),
        "the first item has nothing to nest under"
    );
    assert!(
        indent(&d, &b[4], &b[4]).is_err(),
        "a paragraph is in no item"
    );
    assert!(
        indent(&d, &b[1], &b[4]).is_err(),
        "the range leaves the list"
    );
    assert_eq!(
        run(outdent(&d, &b[2], &b[2]).unwrap()),
        ["LI:a", "LI:b", "LI:c", "LI:d", ":para"]
    );
    assert_eq!(
        run(outdent(&d, &b[1], &b[1]).unwrap()),
        ["LI:a", ":b", "LI:c", "LI:d", ":para"]
    );
    assert_eq!(
        run(outdent(&d, &b[0], &b[0]).unwrap()),
        [":a", "LI:b", "LILI:c", "LI:d", ":para"]
    );
    let split = apply(&d, &outdent(&d, &b[1], &b[1]).unwrap())
        .unwrap()
        .content;
    assert_ne!(
        split.blocks[0].path[0], split.blocks[3].path[0],
        "the items after a lifted one open a list of their own"
    );
}

#[test]
fn a_lifted_nested_item_takes_the_items_after_it_and_the_rest_of_its_parent() {
    let d = from_markdown("- a\n  - b\n  - c\n  - d\n\n  tail")
        .unwrap()
        .content;
    let b = ids(&d);
    let done = apply(&d, &outdent(&d, &b[2], &b[2]).unwrap())
        .unwrap()
        .content;
    assert_eq!(
        shapes(&done),
        ["LI:a", "LILI:b", "LI:c", "LILI:d", "LI:tail"]
    );
    assert_ne!(
        done.blocks[3].path[2], d.blocks[3].path[2],
        "b stays in the list, so the items after open one of their own"
    );
    let first = apply(&d, &outdent(&d, &b[1], &b[1]).unwrap())
        .unwrap()
        .content;
    assert_eq!(
        shapes(&first),
        ["LI:a", "LI:b", "LILI:c", "LILI:d", "LI:tail"]
    );
    assert_eq!(
        first.blocks[2].path[2], d.blocks[2].path[2],
        "with none before, the items after keep their list"
    );
    assert_eq!(
        done.blocks[4].path[1], done.blocks[2].path[1],
        "the parent's rest moves into the lifted item"
    );
}

#[test]
fn a_shift_tab_in_a_quote_lifts_what_it_touches_and_splits_the_rest() {
    let d = from_markdown("> a\n>\n> b\n>\n> c").unwrap().content;
    let b = ids(&d);
    let done = apply(&d, &outdent(&d, &b[1], &b[1]).unwrap())
        .unwrap()
        .content;
    assert_eq!(shapes(&done), ["Q:a", ":b", "Q:c"]);
    assert_ne!(done.blocks[0].path, done.blocks[2].path);
}

#[test]
fn a_wrap_opens_a_container_around_the_children_its_range_touches() {
    let d = from_markdown("- a\n- b\n  - c\n- d\n\npara\n\nmore")
        .unwrap()
        .content;
    let b = ids(&d);
    let run = |c: Change| shapes(&apply(&d, &c).unwrap().content);
    let quote = Wrap::Quote;
    let ordered = Wrap::List {
        ordered: true,
        checked: None,
    };
    assert_eq!(
        run(wrap(&d, &b[4], &b[5], &quote).unwrap()),
        ["LI:a", "LI:b", "LILI:c", "LI:d", "Q:para", "Q:more"]
    );
    assert_eq!(
        run(wrap(&d, &b[4], &b[5], &ordered).unwrap()),
        ["LI:a", "LI:b", "LILI:c", "LI:d", "OI:para", "OI:more"]
    );
    assert_eq!(
        run(wrap(&d, &b[1], &b[3], &quote).unwrap()),
        ["QLI:a", "QLI:b", "QLILI:c", "QLI:d", ":para", ":more"],
        "a list the range sits in is wrapped whole"
    );
    assert_eq!(
        run(wrap(&d, &b[1], &b[1], &quote).unwrap()),
        ["LI:a", "LIQ:b", "LILI:c", "LI:d", ":para", ":more"]
    );
    let listed = apply(&d, &wrap(&d, &b[4], &b[5], &ordered).unwrap())
        .unwrap()
        .content;
    assert_ne!(
        listed.blocks[4].path[1], listed.blocks[5].path[1],
        "each wrapped child is an item of its own"
    );
    let task = apply(
        &d,
        &wrap(
            &d,
            &b[4],
            &b[4],
            &Wrap::List {
                ordered: false,
                checked: Some(false),
            },
        )
        .unwrap(),
    )
    .unwrap()
    .content;
    assert_eq!(
        task.container(&task.blocks[4].path[1]).unwrap().kind,
        ContainerKind::Item {
            checked: Some(false)
        }
    );
}

#[test]
fn a_delete_from_a_blocks_start_keeps_the_shape_of_the_block_it_ends_in() {
    let h = from_markdown("# Title\n\nbody").unwrap().content;
    let done = apply(&h, &delete_range(&h, 0, 6).unwrap()).unwrap().content;
    assert_eq!(segments(&done), ["b0=body"]);
    let joined = apply(&h, &delete_range(&h, 2, 7).unwrap()).unwrap().content;
    assert_eq!(
        segments(&joined),
        ["b0#=Tiody"],
        "a delete from inside a block keeps that block's kind"
    );
    let l = from_markdown("- a\n- b\n\npara").unwrap().content;
    let done = apply(&l, &delete_range(&l, 2, 5).unwrap()).unwrap().content;
    assert_eq!(shapes(&done), ["LI:a", ":ara"]);
    assert_eq!(delete_range(&l, 3, 2), Err(ApplyError::Stale { step: 0 }));
    assert!(delete_range(&l, 1, 1).unwrap().is_empty());
}

fn nested_document() -> impl Strategy<Value = Content> {
    let item = prop_oneof![
        3 => "[a-z]{1,4}".prop_map(|w| format!("- {w}")),
        2 => "[a-z]{1,4}".prop_map(|w| format!("  - {w}")),
        1 => "[a-z]{1,4}".prop_map(|w| format!("    - {w}")),
        1 => "[a-z]{1,4}".prop_map(|w| format!("  {w}")),
    ];
    let list = prop::collection::vec(item, 1..7).prop_map(|ls| format!("- x\n{}", ls.join("\n")));
    let part = prop_oneof![2 => list, 1 => block()];
    prop::collection::vec(part, 1..4)
        .prop_map(|ps| from_markdown(&ps.join("\n\n")).unwrap().content)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Every structural edit's change applies wherever the edit answers one, and
    /// keeps the text and every block id; a Tab and a Shift-Tab over the same
    /// blocks put the value back.
    #[test]
    fn a_structural_edit_keeps_the_text_and_its_tab_undoes_by_shift_tab(
        d in nested_document(), x in any::<usize>(), y in any::<usize>(), w in 0u8..3,
    ) {
        let n = d.blocks.len();
        let (k0, k1) = ((x % n).min(y % n), (x % n).max(y % n));
        let (first, last) = (d.blocks[k0].id.clone(), d.blocks[k1].id.clone());
        let with = match w {
            0 => Wrap::Quote,
            1 => Wrap::List { ordered: true, checked: None },
            _ => Wrap::List { ordered: false, checked: Some(true) },
        };
        let deep = |e: &ApplyError| matches!(e, ApplyError::Invalid(Invariant::NestingTooDeep { .. }));
        for change in [indent(&d, &first, &last), outdent(&d, &first, &last), wrap(&d, &first, &last, &with)].into_iter().flatten() {
            match apply(&d, &change) {
                Ok(done) => {
                    prop_assert_eq!(&done.content.text, &d.text);
                    prop_assert_eq!(ids(&done.content), ids(&d));
                }
                Err(e) => prop_assert!(deep(&e), "{:?} refused {:?}", change, e),
            }
        }
        if let Ok(tab) = indent(&d, &first, &last)
            && let Ok(done) = apply(&d, &tab)
        {
            let tabbed = done.content;
            let back = apply(&tabbed, &outdent(&tabbed, &first, &last).unwrap()).unwrap().content;
            prop_assert_eq!(&back, &d);
        }
        let len = d.len_scalars();
        let (s, e) = ((x % (len + 1)).min(y % (len + 1)), (x % (len + 1)).max(y % (len + 1)));
        if let Ok(done) = apply(&d, &delete_range(&d, s, e).unwrap()) {
            let kept: String = d.text.chars().take(s).chain(d.text.chars().skip(e)).collect();
            prop_assert_eq!(done.content.text, kept);
        }
    }
}

#[test]
fn a_diff_merging_or_splitting_table_cells_lands_the_span() {
    let d = from_markdown("| a | b |\n| - | - |\n| c | d |")
        .unwrap()
        .content;
    let mut after = d.clone();
    let table = &mut after.islands[0];
    let row = table.props["rows"][1]["cells"].clone();
    let cols: Vec<String> = table.props["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    let (left, right) = (
        row[&cols[0]].as_str().unwrap().to_string(),
        row[&cols[1]].as_str().unwrap().to_string(),
    );
    table.props["cell_attrs"] = json!({ left.clone(): { "colspan": 2 } });
    table
        .slots
        .insert(left, quillmark_content::markdown::from_plaintext("c d"));
    table
        .slots
        .insert(right, quillmark_content::markdown::from_plaintext(""));
    let after = Content::from_value(&after.to_value()).unwrap();
    for (from, to) in [(&d, &after), (&after, &d)] {
        let change = quillmark_content::diff::diff(from, to).unwrap();
        assert_eq!(&apply(from, &change).unwrap().content, to);
    }
}

/// Each character of `c`'s text, a `\n` with the id of the block it opens and
/// a slot with its island's.
fn keyed(c: &Content) -> Vec<String> {
    let (mut nl, mut sl) = (0, 0);
    c.text
        .chars()
        .map(|ch| match ch {
            '\n' => {
                nl += 1;
                format!("\n{}", c.blocks[nl].id)
            }
            ISLAND_SLOT => {
                sl += 1;
                format!("{ch}{}", c.islands[sl - 1].id)
            }
            ch => ch.to_string(),
        })
        .collect()
}

/// The narrowest range of `a` outside which `b` holds the same characters
/// and records: `(from, to)`, offsets into `a`.
fn changed_range(a: &Content, b: &Content) -> (usize, usize) {
    let (ka, kb) = (keyed(a), keyed(b));
    let lead = ka.iter().zip(&kb).take_while(|(x, y)| x == y).count();
    let room = ka.len().min(kb.len()) - lead;
    let tail = ka
        .iter()
        .rev()
        .zip(kb.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    (lead, ka.len() - tail)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// A range splice reaches what the whole diff reaches for every range,
    /// and over a range holding every change, is one splice no wider than
    /// it.
    #[test]
    fn a_range_diff_reaches_the_diffs_value_and_is_never_coarser_than_its_range(
        d in document(), ops in ops(), wide in (any::<usize>(), any::<usize>()), wild in (any::<usize>(), any::<usize>()),
    ) {
        use quillmark_content::diff::{diff, diff_range};
        let edited = apply(&d, &change_for(&d, &ops)).unwrap().content;
        let whole = apply(&d, &diff(&d, &edited).unwrap()).unwrap().content;
        let (from, to) = changed_range(&d, &edited);
        let len = d.len_scalars();
        let (from, to) = (from - wide.0 % (from + 1), to + wide.1 % (len - to + 1));
        let change = diff_range(&d, &edited, from, to).unwrap();
        prop_assert_eq!(&apply(&d, &change).unwrap().content, &whole, "{:?}", change);
        let splices: Vec<&Splice> = change.steps.iter().filter_map(|s| if let Step::Splice(s) = s { Some(s) } else { None }).collect();
        prop_assert!(splices.len() <= 1, "{:?}", change);
        let grown = edited.len_scalars() + to - len;
        for s in splices {
            prop_assert!(s.delete.chars().count() <= to - from && s.insert.chars().count() <= grown - from, "{:?} over {}..{}", s, from, to);
        }
        let (x, y) = (wild.0 % (len + 1), wild.1 % (len + 1));
        let any = diff_range(&d, &edited, x.min(y), x.max(y)).unwrap();
        prop_assert_eq!(apply(&d, &any).unwrap().content, whole);
    }
}

#[test]
fn a_range_diff_types_one_letter_as_one_letter_and_keeps_enter_on_the_text() {
    use quillmark_content::diff::diff_range;
    let d = from_markdown("Paragraph one\n\ntwo").unwrap().content;
    let typed = apply(
        &d,
        &Change::new(vec![Step::Splice(Splice::new(4, "", "x"))]),
    )
    .unwrap()
    .content;
    let change = diff_range(&d, &typed, 4, 4).unwrap();
    assert_eq!(change.steps, [Step::Splice(Splice::new(4, "", "x"))]);
    let entered = apply(&d, &split_block(&d, 14, Some("n")).unwrap())
        .unwrap()
        .content;
    assert_eq!(segments(&entered), ["b0=Paragraph one", "n=", "b1=two"]);
    let change = diff_range(&d, &entered, 14, 14).unwrap();
    assert_eq!(apply(&d, &change).unwrap().content, entered);
    assert!(
        matches!(&change.steps[..], [Step::Splice(s)] if s.delete.is_empty() && s.insert == "\n"),
        "{:?}",
        change
    );
}

#[test]
fn a_placement_refuses_a_held_id_and_a_move_a_missing_one() {
    let mut d = from_markdown("Hello world\n\n| aa | bb |\n| - | - |\n| cc | dd |")
        .unwrap()
        .content;
    d.anchors.push(Anchor::new("c-1", 0, 5));
    let island = d.islands[0].clone();
    let cell = island.slot_order()[2].clone();
    d.islands[0]
        .slots
        .get_mut(&cell)
        .unwrap()
        .anchors
        .push(Anchor::new("c-2", 0, 2));
    let one = |s: Step| Change::new(vec![s]);
    let place = |id: &str, s, e| {
        Step::Anchor(AnchorStep::Place {
            anchor: Anchor::new(id, s, e),
            quote: None,
        })
    };
    let shift = |id: &str, s, e| {
        Step::Anchor(AnchorStep::Move {
            anchor: Anchor::new(id, s, e),
            quote: None,
        })
    };
    let in_cell = |s: Step| Step::Nested {
        island: island.id.clone(),
        slot: cell.clone(),
        change: Change::new(vec![s]),
    };
    assert_eq!(
        apply(&d, &one(place("c-1", 6, 11))).map(|_| ()),
        Err(ApplyError::AnchorHeld { id: "c-1".into() })
    );
    assert_eq!(
        apply(&d, &one(place("c-2", 6, 11))).map(|_| ()),
        Err(ApplyError::AnchorHeld { id: "c-2".into() })
    );
    assert_eq!(
        apply(&d, &one(in_cell(place("c-1", 0, 1)))).map(|_| ()),
        Err(ApplyError::Invalid(Invariant::DuplicateId {
            id: "c-1".into()
        }))
    );
    assert_eq!(
        apply(&d, &one(shift("c-9", 6, 11))).map(|_| ()),
        Err(ApplyError::UnknownAnchor { id: "c-9".into() })
    );
    assert_eq!(
        apply(&d, &one(shift("c-2", 6, 11))).map(|_| ()),
        Err(ApplyError::UnknownAnchor { id: "c-2".into() })
    );
    let moved = apply(&d, &one(shift("c-1", 6, 11))).unwrap();
    assert_eq!(
        moved.content.anchor("c-1"),
        Some(&Anchor::new("c-1", 6, 11))
    );
    assert_eq!(
        invert(&one(shift("c-1", 6, 11)), &d).unwrap().steps,
        [Step::Anchor(AnchorStep::Move {
            anchor: Anchor::new("c-1", 0, 5),
            quote: Some("Hello".into())
        })]
    );
    let across = Change::new(vec![
        Step::Anchor(AnchorStep::Remove { id: "c-1".into() }),
        in_cell(place("c-1", 0, 1)),
    ]);
    assert!(apply(&d, &across).is_ok());
    for (step, wire) in [
        (
            place("c-3", 1, 2),
            json!({"anchor": {"id": "c-3", "start": 1, "end": 2}}),
        ),
        (
            shift("c-1", 1, 2),
            json!({"anchor": {"id": "c-1", "start": 1, "end": 2, "move": true}}),
        ),
    ] {
        assert_eq!(one(step.clone()).to_value(), json!({"steps": [wire]}));
        assert_eq!(
            Change::from_value(&json!({"steps": [wire]})).unwrap(),
            one(step)
        );
    }
    assert!(
        Change::from_value(
            &json!({"steps": [{"anchor": {"id": "c-1", "remove": true, "move": true}}]})
        )
        .is_err()
    );
}

/// A container re-kinded under an id of the longest length a value holds
/// diffs to a change that applies: the id it derives for the new kind fits.
#[test]
fn a_rekinded_container_of_the_longest_id_diffs() {
    let long = "l".repeat(quillmark_content::ids::MAX_ID_LEN);
    let before = Content::from_value(&json!({"blocks":[{"id":"b0","kind":"para","path":[long, "i0"]}],"containers":[{"attrs":{"ordered":false,"start":1},"id":long,"kind":"list"},{"id":"i0","kind":"item"}],"text":"a"})).unwrap();
    let after = Content::from_value(&json!({"blocks":[{"id":"b0","kind":"para","path":[long]}],"containers":[{"id":long,"kind":"quote"}],"text":"a"})).unwrap();
    let change = quillmark_content::diff::replace(&before, &after).unwrap();
    let landed = apply(&before, &change).unwrap().content;
    assert_eq!(
        landed
            .containers
            .iter()
            .map(|c| &c.kind)
            .collect::<Vec<_>>(),
        [&ContainerKind::Quote]
    );
}
