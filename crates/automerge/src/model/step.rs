//! A change's steps lowered straight to Automerge operations.
//!
//! A step names what it touches as the view the writer read: offsets into
//! its text, and blocks, containers and islands by the object ids the view
//! gives them. A step that cannot be placed in that view refuses; nothing a
//! concurrent change did refuses one, since the merge is Automerge's and
//! the view reads every merged state valid.

use super::json::write_json_at;
use super::value::{self, ATTRS, CONTAINERS, PARENTS, SLOTS, TYPE, id_of, obj};
use automerge::marks::Mark as AmMark;
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ReadDoc};
use quillmark_content::change::{
    AnchorStep, Change, ContainerStep, IslandStep, MarkStep, SetBlock, Splice, Step,
};
use quillmark_content::model::{Block, Content, Island};
use quillmark_content::wire;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// What a lowered change made: each id the change gave a record it inserted,
/// a placeholder or an id it carried over, and the object id the record
/// took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lowered {
    pub minted: BTreeMap<String, String>,
}

/// A step the view it was made against cannot place. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StepError {
    #[error("step {step}: the text at {at} is not the text it deletes")]
    Stale { step: usize, at: usize },
    #[error("step {step}: offset {at} is past the text")]
    Bounds { step: usize, at: usize },
    #[error("step {step}: no {what} `{id}`")]
    Unknown {
        step: usize,
        what: &'static str,
        id: String,
    },
    #[error("step {step}: the anchor `{id}` is held already")]
    AnchorHeld { step: usize, id: String },
    #[error("step {step}: {reason}")]
    Bad { step: usize, reason: &'static str },
}

/// Checks that `step` places in `view`, the value as the steps before it
/// left it.
fn check(view: &Content, step: &Step, n: usize) -> Result<(), StepError> {
    let len = view.text.chars().count();
    let bounds = |at: usize| StepError::Bounds { step: n, at };
    match step {
        Step::Splice(s) => {
            let del = s.delete.chars().count();
            if s.at + del > len {
                return Err(bounds(s.at + del));
            }
            if !view.text.chars().skip(s.at).take(del).eq(s.delete.chars()) {
                return Err(StepError::Stale { step: n, at: s.at });
            }
        }
        Step::Mark(MarkStep::Add { mark: m, .. }) if m.start > m.end || m.end > len => {
            return Err(bounds(m.end));
        }
        Step::Mark(MarkStep::Remove { start, end, .. }) if start > end || *end > len => {
            return Err(bounds(*end));
        }
        Step::Anchor(AnchorStep::Place { anchor: a, .. })
        | Step::Anchor(AnchorStep::Move { anchor: a, .. })
            if a.start > a.end || a.end > len =>
        {
            return Err(bounds(a.end));
        }
        Step::Anchor(AnchorStep::Place { anchor: a, .. }) if view.anchor_ids().contains(&a.id) => {
            return Err(StepError::AnchorHeld {
                step: n,
                id: a.id.clone(),
            });
        }
        Step::Anchor(AnchorStep::Move { anchor: a, .. }) if !view.anchor_ids().contains(&a.id) => {
            return Err(unknown(n, "anchor", &a.id));
        }
        Step::Island(IslandStep::Insert { at, .. }) if *at > len => return Err(bounds(*at)),
        _ => {}
    }
    Ok(())
}

/// What the value `root` reads as inside the open transaction.
fn current(doc: &AutoCommit, root: &ObjId) -> Content {
    let raw = quillmark_content::repair::Raw::from_value(&value::read(doc, root))
        .expect("a value reads as raw content");
    quillmark_content::repair::repair(raw).0
}

fn renumber(e: StepError, n: usize) -> StepError {
    match e {
        StepError::Stale { at, .. } => StepError::Stale { step: n, at },
        StepError::Bounds { at, .. } => StepError::Bounds { step: n, at },
        StepError::Unknown { what, id, .. } => StepError::Unknown { step: n, what, id },
        StepError::AnchorHeld { id, .. } => StepError::AnchorHeld { step: n, id },
        StepError::Bad { reason, .. } => StepError::Bad { step: n, reason },
    }
}

/// Lowers `change` into the value `root`, which reads as `view`.
///
/// Each step is checked against the value as the steps before it left it,
/// and a step that does not place refuses the whole change: the caller
/// rolls the transaction back.
pub(crate) fn lower(doc: &mut AutoCommit, root: &ObjId, change: &Change) -> Result<Lowered, StepError> {
    let mut out = Lowered::default();
    let mut scopes = HashMap::new();
    lower_in(doc, root, change, &mut out, &mut scopes)?;
    Ok(out)
}

/// Per value a change made, each id its given content named and the object
/// id that part took, so a later step in it finds them.
type Scopes = HashMap<ObjId, HashMap<String, String>>;

/// Notes the ids of each slot `island` was given, in the island `m` made.
fn note_slots(doc: &AutoCommit, m: &ObjId, island: &Island, scopes: &mut Scopes) {
    let Some(s) = obj(doc, m, SLOTS) else { return };
    for (k, given) in &island.slots {
        if let Some(root) = obj(doc, &s, k) {
            note_scope(doc, &root, given, scopes);
        }
    }
}

fn note_scope(doc: &AutoCommit, root: &ObjId, given: &Content, scopes: &mut Scopes) {
    let made = current(doc, root);
    let mut map = HashMap::new();
    for (a, b) in given.blocks.iter().zip(&made.blocks) {
        map.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in given.containers.iter().zip(&made.containers) {
        map.insert(a.id.clone(), b.id.clone());
    }
    for (a, b) in given.islands.iter().zip(&made.islands) {
        map.insert(a.id.clone(), b.id.clone());
    }
    scopes.insert(root.clone(), map);
}

fn lower_in(doc: &mut AutoCommit, root: &ObjId, change: &Change, out: &mut Lowered, scopes: &mut Scopes) -> Result<(), StepError> {
    let t = value::text(doc, root).ok_or(StepError::Bad { step: 0, reason: "the value holds no text" })?;
    let mut ids: HashMap<String, String> = scopes.get(root).cloned().unwrap_or_default();
    for (n, step) in change.steps.iter().enumerate() {
        let view = current(doc, root);
        check(&view, step, n)?;
        match step {
            Step::Splice(s) => {
                splice(doc, &t, s, &mut ids, out, scopes);
                let one = Change::new(vec![step.clone()]);
                if let Ok(landed) = quillmark_content::change::apply(&view, &one) {
                    for d in landed.dropped {
                        value::unplace(doc, root, &t, &d.context.id);
                    }
                }
            }
            Step::SetBlock(s) => set_block(doc, &t, s, &ids, n)?,
            Step::Container(ContainerStep::Set(c)) => {
                let v = wire::container_value(c);
                let m = match find(doc, &c.id, &ids) {
                    Some(m) => m,
                    None => {
                        let cs = match obj(doc, root, CONTAINERS) {
                            Some(cs) => cs,
                            None => doc.put_object(root, CONTAINERS, ObjType::List).unwrap(),
                        };
                        let m = doc
                            .insert_object(&cs, doc.length(&cs), ObjType::Map)
                            .unwrap();
                        ids.insert(c.id.clone(), id_of(&m));
                        out.minted.insert(c.id.clone(), id_of(&m));
                        m
                    }
                };
                doc.put(&m, TYPE, v["kind"].as_str().unwrap()).unwrap();
                write_json_at(doc, &m, ATTRS, v.get("attrs").unwrap_or(&json!({})));
            }
            Step::Container(ContainerStep::Remove { id }) => {
                let cs = obj(doc, root, CONTAINERS).ok_or_else(|| unknown(n, "container", id))?;
                let target = find(doc, id, &ids).ok_or_else(|| unknown(n, "container", id))?;
                let at = doc
                    .values(&cs)
                    .position(|(_, o)| o == target)
                    .ok_or_else(|| unknown(n, "container", id))?;
                doc.delete(&cs, at).unwrap();
            }
            Step::Mark(MarkStep::Add { mark: m, .. }) => {
                if m.start < m.end {
                    let (name, v) = value::mark_key(&m.kind);
                    doc.mark(
                        &t,
                        AmMark::new(name, v, m.start + 1, m.end + 1),
                        value::expand(&m.kind),
                    )
                    .unwrap();
                }
            }
            Step::Mark(MarkStep::Remove {
                start, end, tag, ..
            }) => unmark_tag(doc, &t, *start + 1, *end + 1, tag),
            Step::Anchor(AnchorStep::Place { anchor: a, .. }) => {
                value::place(doc, root, &t, &a.id, a.start, a.end)
            }
            Step::Anchor(AnchorStep::Move { anchor: a, .. }) => {
                value::unplace(doc, root, &t, &a.id);
                value::place(doc, root, &t, &a.id, a.start, a.end);
            }
            Step::Anchor(AnchorStep::Remove { id }) => value::unplace(doc, root, &t, id),
            Step::Island(IslandStep::Insert { at, island, .. }) => {
                let m = doc.split_block(&t, at + 1).unwrap();
                value::put_island(doc, &m, island);
                note_slots(doc, &m, island, scopes);
                out.minted.insert(island.id.clone(), id_of(&m));
                ids.insert(island.id.clone(), id_of(&m));
            }
            Step::Island(IslandStep::Props { id, props, slots }) => {
                let m = find(doc, id, &ids).ok_or_else(|| unknown(n, "island", id))?;
                if !props.is_object() {
                    return Err(StepError::Bad {
                        step: n,
                        reason: "`props` is not an object",
                    });
                }
                write_json_at(doc, &m, value::PROPS, props);
                let ty = view
                    .islands
                    .iter()
                    .find(|i| i.id == id_of(&m))
                    .map(|i| i.island_type.clone())
                    .ok_or_else(|| unknown(n, "island", id))?;
                let mut probe = Island::new(id.as_str(), ty).with_props(props.clone());
                let s = obj(doc, &m, SLOTS)
                    .unwrap_or_else(|| doc.put_object(&m, SLOTS, ObjType::Map).unwrap());
                probe.slots = doc.keys(&s).map(|k| (k, Content::empty())).collect();
                let order = probe.slot_order();
                for k in doc.keys(&s).collect::<Vec<_>>() {
                    if !order.contains(&k) {
                        doc.delete(&s, k.as_str()).unwrap();
                    }
                }
                for k in order {
                    if obj(doc, &s, &k).is_none() {
                        let given = slots.get(&k).cloned().unwrap_or_else(Content::empty);
                        let r = value::create(doc, &s, &k, &given);
                        note_scope(doc, &r, &given, scopes);
                    }
                }
            }
            Step::Nested {
                island,
                slot,
                change,
            } => {
                let m = find(doc, island, &ids).ok_or_else(|| unknown(n, "island", island))?;
                let slot_root = obj(doc, &m, SLOTS)
                    .and_then(|s| obj(doc, &s, slot))
                    .ok_or_else(|| unknown(n, "slot", slot))?;
                lower_in(doc, &slot_root, change, out, scopes).map_err(|e| renumber(e, n))?;
            }
        }
    }
    Ok(())
}

fn unknown(step: usize, what: &'static str, id: &str) -> StepError {
    StepError::Unknown {
        step,
        what,
        id: id.to_string(),
    }
}

/// The object an id names: one this change minted, or an object id.
fn find(doc: &AutoCommit, id: &str, ids: &HashMap<String, String>) -> Option<ObjId> {
    let id = ids.get(id).map(String::as_str).unwrap_or(id);
    doc.import(id).ok().map(|(o, _)| o)
}

fn splice(
    doc: &mut AutoCommit,
    t: &ObjId,
    s: &Splice,
    ids: &mut HashMap<String, String>,
    out: &mut Lowered,
    scopes: &mut Scopes,
) {
    let (del, ins): (Vec<char>, Vec<char>) = (s.delete.chars().collect(), s.insert.chars().collect());
    let kept = |a: &char, b: &char| a == b && *a == '\n';
    let pre = del.iter().zip(&ins).take_while(|(a, b)| kept(a, b)).count();
    let suf = del[pre..].iter().rev().zip(ins[pre..].iter().rev()).take_while(|(a, b)| kept(a, b)).count();
    let breaks = |cs: &[char]| cs.iter().filter(|c| **c == '\n').count();
    let (pre_blocks, suf_blocks) = (breaks(&ins[..pre]), breaks(&ins[ins.len() - suf..]));
    let mid_blocks = &s.blocks[pre_blocks.min(s.blocks.len())..s.blocks.len().saturating_sub(suf_blocks).max(pre_blocks.min(s.blocks.len()))];
    let gone = del.len() - pre - suf;
    if gone > 0 {
        doc.splice_text(t, s.at + pre + 1, gone as isize, "").unwrap();
    }
    let mid: String = ins[pre..ins.len() - suf].iter().collect();
    let made = value::insert_text(doc, t, s.at + pre + 1, &mid, mid_blocks, &s.islands, ids);
    let mut keep = |doc: &mut AutoCommit, offset: usize, b: Option<&Block>, ids: &mut HashMap<String, String>| {
        let (Some(b), Ok(Some((_, m)))) = (b, doc.get(t, offset + 1)) else { return };
        value::put_block(doc, &m, b, ids);
        ids.insert(b.id.clone(), id_of(&m));
        out.minted.insert(b.id.clone(), id_of(&m));
    };
    let mut n = 0;
    for (i, c) in ins[..pre].iter().enumerate() {
        if *c == '\n' {
            keep(doc, s.at + i, s.blocks.get(n), ids);
            n += 1;
        }
    }
    let base = s.at + ins.len() - suf;
    let mut n = s.blocks.len().saturating_sub(suf_blocks);
    for (i, c) in ins[ins.len() - suf..].iter().enumerate() {
        if *c == '\n' {
            keep(doc, base + i, s.blocks.get(n), ids);
            n += 1;
        }
    }
    let (mut blocks, mut islands) = (mid_blocks.iter(), s.islands.iter());
    for (m, ch) in made.iter().zip(
        mid.chars()
            .filter(|c| *c == '\n' || *c == quillmark_content::model::ISLAND_SLOT),
    ) {
        let given = match ch {
            '\n' => blocks.next().map(|b| b.id.clone()),
            _ => islands.next().map(|i| {
                note_slots(doc, m, i, scopes);
                i.id.clone()
            }),
        };
        if let Some(g) = given {
            ids.insert(g.clone(), id_of(m));
            out.minted.insert(g, id_of(m));
        }
    }
    let len = s.insert.chars().count();
    if len == 0 {
        return;
    }
    let (start, end) = (s.at + 1, s.at + 1 + len);
    let carried: Vec<String> = doc
        .marks(t)
        .unwrap()
        .iter()
        .filter(|m| !m.name().starts_with(value::ANCHOR) && m.start < end && m.end > start)
        .map(|m| m.name().to_string())
        .collect();
    for name in carried {
        doc.unmark(t, &name, start, end, automerge::marks::ExpandMark::None)
            .unwrap();
    }
    for m in &s.marks {
        if m.start < m.end {
            let (name, v) = value::mark_key(&m.kind);
            doc.mark(
                t,
                AmMark::new(name, v, start + m.start, start + m.end),
                value::expand(&m.kind),
            )
            .unwrap();
        }
    }
}

fn set_block(
    doc: &mut AutoCommit,
    t: &ObjId,
    s: &SetBlock,
    ids: &HashMap<String, String>,
    n: usize,
) -> Result<(), StepError> {
    let m = find(doc, &s.id, ids)
        .filter(|m| {
            doc.object_type(m).ok() == Some(ObjType::Map)
                && doc
                    .parents(m)
                    .ok()
                    .and_then(|mut ps| ps.next())
                    .is_some_and(|p| p.obj == *t)
        })
        .ok_or_else(|| unknown(n, "block", &s.id))?;
    if let Some(kind) = &s.kind {
        doc.put(&m, TYPE, kind.as_str()).unwrap();
        if s.attrs.is_none() {
            write_json_at(doc, &m, ATTRS, &json!({}));
        }
    }
    if let Some(attrs) = &s.attrs {
        write_json_at(doc, &m, ATTRS, &Value::Object(attrs.clone()));
    }
    if let Some(path) = &s.path {
        let parents: Vec<Value> = path
            .iter()
            .map(|p| Value::String(ids.get(p).cloned().unwrap_or_else(|| p.clone())))
            .collect();
        write_json_at(doc, &m, PARENTS, &Value::Array(parents));
    }
    Ok(())
}

/// Clears every mark of type `tag` over Automerge indices `[start, end)`.
fn unmark_tag(doc: &mut AutoCommit, t: &ObjId, start: usize, end: usize, tag: &str) {
    if start >= end {
        return;
    }
    let names: Vec<String> = doc
        .marks(t)
        .unwrap()
        .iter()
        .map(|m| m.name().to_string())
        .filter(|name| name == tag || name.split_once('\u{1f}').is_some_and(|(ty, _)| ty == tag))
        .collect();
    for name in names {
        doc.unmark(t, &name, start, end, automerge::marks::ExpandMark::None)
            .unwrap();
    }
}
