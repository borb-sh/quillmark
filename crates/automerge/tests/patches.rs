//! A write's patches take the value it read to the value it left: applied
//! to the view before, as a host holding that view applies them, they give
//! the view after in text, block, container and island records, anchors and
//! the formatting on each character, whichever heads the write landed at,
//! and each reads back from its wire form as itself.
//! `CASES` raises the case count.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::automerge::ObjId;
use quillmark_automerge::model::patch::{Action, Patch};
use quillmark_automerge::model::{Doc, ids};
use quillmark_automerge::content::model::{Content, ISLAND_SLOT, MarkKind};
use std::collections::BTreeSet;

/// A value as a host holds it to apply patches: text, the record each block
/// and island character carries, containers, anchors, and the formatting of
/// each character.
#[derive(Debug, Clone, PartialEq)]
struct Held {
    text: Vec<char>,
    blocks: Vec<serde_json::Value>,
    islands: Vec<Held>,
    island_records: Vec<serde_json::Value>,
    containers: BTreeSet<String>,
    anchors: BTreeSet<(String, usize, usize)>,
    marks: Vec<BTreeSet<String>>,
    ids: (Vec<String>, Vec<String>),
}

fn kind_key(k: &MarkKind) -> String {
    quillmark_automerge::content::wire::mark_value(&quillmark_automerge::content::model::Mark::new(0, 1, k.clone()))["type"].to_string()
        + &quillmark_automerge::content::wire::mark_value(&quillmark_automerge::content::model::Mark::new(0, 1, k.clone())).get("attrs").map(|a| a.to_string()).unwrap_or_default()
}

fn held(c: &Content) -> Held {
    let mut marks = vec![BTreeSet::new(); c.text.chars().count()];
    for m in &c.marks {
        for set in &mut marks[m.start..m.end] {
            set.insert(kind_key(&m.kind));
        }
    }
    Held {
        text: c.text.chars().collect(),
        blocks: c.blocks.iter().map(quillmark_automerge::content::wire::block_value).collect(),
        islands: c.islands.iter().map(|i| held_island(i)).collect(),
        island_records: c.islands.iter().map(|i| serde_json::json!({ "id": i.id, "type": i.island_type.as_str(), "props": i.props })).collect(),
        containers: c.containers.iter().map(|k| quillmark_automerge::content::wire::container_value(k).to_string()).collect(),
        anchors: c.anchors.iter().map(|a| (a.id.clone(), a.start, a.end)).collect(),
        marks,
        ids: (c.blocks.iter().map(|b| b.id.clone()).collect(), c.islands.iter().map(|i| i.id.clone()).collect()),
    }
}

/// An island's slots, in key order, as one `Held` each; held in `islands`.
fn held_island(i: &quillmark_automerge::content::model::Island) -> Held {
    let mut h = held(&Content::empty());
    h.islands = i.slots.values().map(held).collect();
    h.island_records = i
        .slots
        .keys()
        .map(|k| serde_json::Value::String(k.clone()))
        .collect();
    h
}

fn slot_mut<'a>(h: &'a mut Held, island: &str, slot: &str) -> &'a mut Held {
    let k = h
        .ids
        .1
        .iter()
        .position(|i| i == island)
        .expect("the slot's island is held");
    let s = &mut h.islands[k];
    let j = s
        .island_records
        .iter()
        .position(|r| r == slot)
        .expect("the island holds the slot");
    &mut s.islands[j]
}

fn apply(h: &mut Held, p: &Patch) {
    let mut at = h;
    for (island, slot) in &p.slot {
        at = slot_mut(at, island, slot);
    }
    let h = at;
    let before = |h: &Held, at: usize, c: char| h.text[..at].iter().filter(|x| **x == c).count();
    match &p.action {
        Action::Insert {
            at,
            text,
            blocks,
            islands,
            marks,
        } => {
            let (b, i) = (before(h, *at, '\n') + 1, before(h, *at, ISLAND_SLOT));
            let chars: Vec<char> = text.chars().collect();
            let mut sets = vec![BTreeSet::new(); chars.len()];
            for m in marks {
                for set in &mut sets[m.start..m.end] {
                    set.insert(kind_key(&m.kind));
                }
            }
            h.text.splice(*at..*at, chars);
            h.marks.splice(*at..*at, sets);
            h.blocks.splice(
                b..b,
                blocks.iter().map(quillmark_automerge::content::wire::block_value),
            );
            h.ids.0.splice(b..b, blocks.iter().map(|x| x.id.clone()));
            h.islands.splice(i..i, islands.iter().map(held_island));
            h.island_records.splice(i..i, islands.iter().map(|x| serde_json::json!({ "id": x.id, "type": x.island_type.as_str(), "props": x.props })));
            h.ids.1.splice(i..i, islands.iter().map(|x| x.id.clone()));
        }
        Action::Delete { at, len } => {
            let gone: Vec<char> = h.text.drain(*at..at + len).collect();
            h.marks.drain(*at..at + len);
            let (b, i) = (before(h, *at, '\n') + 1, before(h, *at, ISLAND_SLOT));
            let (nb, ni) = (
                gone.iter().filter(|c| **c == '\n').count(),
                gone.iter().filter(|c| **c == ISLAND_SLOT).count(),
            );
            h.blocks.drain(b..b + nb);
            h.ids.0.drain(b..b + nb);
            h.islands.drain(i..i + ni);
            h.island_records.drain(i..i + ni);
            h.ids.1.drain(i..i + ni);
        }
        Action::Block(b) => {
            let k = h
                .ids
                .0
                .iter()
                .position(|x| *x == b.id)
                .expect("a block patch names a held block");
            h.blocks[k] = quillmark_automerge::content::wire::block_value(b);
        }
        Action::Island(x) => {
            let k = h
                .ids
                .1
                .iter()
                .position(|i| *i == x.id)
                .expect("an island patch names a held island");
            h.islands[k] = held_island(x);
            h.island_records[k] =
                serde_json::json!({ "id": x.id, "type": x.island_type.as_str(), "props": x.props });
        }
        Action::Container(c) => {
            let v = quillmark_automerge::content::wire::container_value(c);
            h.containers
                .retain(|x| !x.contains(&format!("\"id\":\"{}\"", c.id)));
            h.containers.insert(v.to_string());
        }
        Action::ContainerRemoved { id } => h
            .containers
            .retain(|x| !x.contains(&format!("\"id\":\"{id}\""))),
        Action::Mark(m) => {
            for set in &mut h.marks[m.start..m.end] {
                let tag = m.kind.tag().to_string();
                set.retain(|k| !k.starts_with(&format!("\"{tag}\"")));
                set.insert(kind_key(&m.kind));
            }
        }
        Action::Unmark { start, end, tag } => {
            for set in &mut h.marks[*start..*end] {
                set.retain(|k| !k.starts_with(&format!("\"{tag}\"")));
            }
        }
        Action::Anchor(a) => {
            h.anchors.retain(|(id, _, _)| *id != a.id);
            h.anchors.insert((a.id.clone(), a.start, a.end));
        }
        Action::AnchorRemoved { id } => h.anchors.retain(|(x, _, _)| x != id),
        Action::Value(c) => *h = held(c),
    }
}

fn doc_of(d: &Content) -> (Doc, ObjId) {
    let mut doc = Doc::new();
    let main = doc.main();
    let root = doc.put_value(&main, "v", d);
    doc.commit();
    (doc, root)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases("CASES", 96), ..ProptestConfig::default() })]

    #[test]
    fn a_writes_patches_take_the_view_before_to_the_view_after(d in document(), ops in ops()) {
        let (mut doc, root) = doc_of(&d);
        let before = doc.view(&root).content;
        let base = doc.heads();
        let change = ids::renamed(&change_for(&d, &ops), &d, &before);
        doc.apply(&root, &change, None).unwrap();
        let after = doc.view(&root);
        prop_assume!(after.repairs.is_empty());
        let patches = doc.patches(&root, &base);
        for p in &patches {
            prop_assert_eq!(&Patch::from_value(&p.to_value()).unwrap(), p);
        }
        let mut h = held(&before);
        for p in &patches {
            apply(&mut h, p);
        }
        prop_assert!(h == held(&after.content), "{}\npatches {:?}", differ(&h, &held(&after.content)), patches);
    }

    #[test]
    fn a_merge_s_patches_take_the_view_before_to_the_view_after(d in document(), mine in ops(), theirs in ops()) {
        let (mut doc, root) = doc_of(&d);
        let before = doc.view(&root).content;
        let base = doc.heads();
        let mut peer = Doc::load(&doc.save()).unwrap();
        doc.apply(&root, &ids::renamed(&change_for(&d, &mine), &d, &before), None).unwrap();
        peer.apply(&root, &ids::renamed(&change_from(&d, &theirs, 100), &d, &before), None).unwrap();
        let read = doc.view(&root).content;
        let at = doc.heads();
        doc.merge(&peer.save_since(&base)).unwrap();
        let after = doc.view(&root);
        prop_assume!(after.repairs.is_empty());
        let patches = doc.patches(&root, &at);
        let mut h = held(&read);
        for p in &patches {
            apply(&mut h, p);
        }
        prop_assert!(h == held(&after.content), "{}\npatches {:?}", differ(&h, &held(&after.content)), patches);
    }
}

/// The first lines where two held values part, for a failure message.
fn differ(a: &Held, b: &Held) -> String {
    let (a, b) = (format!("{a:#?}"), format!("{b:#?}"));
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = a
        .iter()
        .zip(&b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    let lo = i.saturating_sub(12);
    format!(
        "got:\n{}\nwant:\n{}",
        a[lo..(i + 4).min(a.len())].join("\n"),
        b[lo..(i + 4).min(b.len())].join("\n")
    )
}
