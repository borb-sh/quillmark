//! A change lowered into Automerge operations from its [`Trace`]: each step
//! as the splices, markers, records, marks and anchors it changes, so the
//! document reads as the value `apply` gave without reading it back.
//!
//! An inserted run takes the formatting of any mark op spanning both its
//! neighbours, so it sheds what it took before taking its own. Reading what
//! it took walks the text's marks, so a trace of more inserts than
//! [`QUERIED`] sheds none per insert and instead sets each text it inserted
//! into to the formatting of the value it lowers to, once, at its end.

use crate::map::{self, BLOCK, KIND, mark_key, marker_at, obj, place, text, unplace};
use automerge::marks::{ExpandMark, Mark as AmMark};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ReadDoc};
use quillmark_content::change::{AnchorStep, ContainerStep, MarkStep, Trace};
use quillmark_content::model::{Content, ISLAND_SLOT};
use quillmark_content::wire;

/// The most inserting splices a trace sheds formatting for one by one.
const QUERIED: usize = 8;

/// One text a trace writes into: its value root, its text, the islands and
/// slots that reach it from the value's own text, and whether a splice
/// inserted into it.
struct Level {
    root: ObjId,
    t: ObjId,
    path: Vec<(String, String)>,
}

impl Level {
    fn new(doc: &AutoCommit, root: ObjId, path: Vec<(String, String)>) -> Level {
        let t = text(doc, &root);
        Level { root, t, path }
    }
}

/// Writes `trace`, which takes the value at `root` to `after`, into it.
pub(crate) fn lower(doc: &mut AutoCommit, root: &ObjId, trace: &[Trace], after: &Content) {
    let inserts = trace.iter().filter(|s| matches!(s, Trace::Splice { insert, .. } if !insert.is_empty())).count();
    let shed = inserts <= QUERIED;
    let mut touched: Vec<Vec<(String, String)>> = Vec::new();
    let mut levels = vec![Level::new(doc, root.clone(), Vec::new())];
    for step in trace {
        let level = levels.last_mut().expect("a trace leaves no level it did not enter");
        match step {
            Trace::Enter { at, island, slot } => {
                let marker = marker_at(doc, &level.t, at + 1).expect("a nested step names an island the text holds");
                let slots = obj(doc, &marker, "slots").expect("an island marker holds its slots");
                let inner = obj(doc, &slots, slot).expect("a nested step names a slot the island holds");
                let mut path = level.path.clone();
                path.push((island.clone(), slot.clone()));
                levels.push(Level::new(doc, inner, path));
            }
            Trace::Leave => {
                levels.pop();
            }
            Trace::Splice { at, delete, insert, marks, blocks, islands, anchors } => {
                let t = level.t.clone();
                if *delete > 0 {
                    doc.splice_text(&t, at + 1, *delete as isize, "").unwrap();
                }
                map::insert_text(doc, &t, at + 1, insert, blocks, islands);
                let n = insert.chars().count();
                if n > 0 && shed {
                    let (from, to) = (at + 1, at + 1 + n);
                    let mut names: Vec<String> = doc
                        .marks(&t)
                        .unwrap()
                        .into_iter()
                        .filter(|m| m.start < to && from < m.end && !map::is_anchor_mark(m.name()))
                        .map(|m| m.name().to_string())
                        .collect();
                    names.sort();
                    names.dedup();
                    for name in names {
                        doc.unmark(&t, &name, from, to, ExpandMark::None).unwrap();
                    }
                }
                if n > 0 {
                    for m in marks {
                        let (name, value) = mark_key(&m.kind);
                        doc.mark(&t, AmMark::new(name, value, at + 1 + m.start, at + 1 + m.end), ExpandMark::None).unwrap();
                    }
                    if !shed && !touched.contains(&level.path) {
                        touched.push(level.path.clone());
                    }
                }
                for a in anchors {
                    unplace(doc, &t, &a.id, Some(a.start < a.end));
                    place(doc, &t, &a.id, a.start, a.end);
                }
            }
            Trace::Block { opens, block } => {
                let i = opens.map_or(0, |o| o + 1);
                let m = marker_at(doc, &level.t, i).expect("a block record rides a marker");
                map::sync_block(doc, &m, block);
                if map::get_str(doc, &m, KIND).as_deref() != Some(BLOCK) {
                    doc.put(&m, KIND, BLOCK).unwrap();
                }
            }            Trace::Container(step) => {
                let cs = obj(doc, &level.root, "containers").expect("a value holds its containers");
                match step {
                    ContainerStep::Set(k) => doc.put(&cs, k.id.as_str(), wire::container_value(k).to_string()).unwrap(),
                    ContainerStep::Remove { id } => doc.delete(&cs, id.as_str()).unwrap(),
                }
            }
            Trace::Mark { step, cleared } => {
                let t = level.t.clone();
                match step {
                    MarkStep::Add { mark, .. } => {
                        let (name, value) = mark_key(&mark.kind);
                        doc.mark(&t, AmMark::new(name, value, mark.start + 1, mark.end + 1), ExpandMark::None).unwrap();
                    }
                    MarkStep::Remove { start, end, .. } => {
                        let mut names: Vec<String> = cleared.iter().map(|k| mark_key(k).0).collect();
                        names.dedup();
                        for name in names {
                            doc.unmark(&t, &name, start + 1, end + 1, ExpandMark::None).unwrap();
                        }
                    }
                }
            }
            Trace::Anchor(step) => {
                let t = level.t.clone();
                match step {
                    AnchorStep::Place { anchor, .. } => place(doc, &t, &anchor.id, anchor.start, anchor.end),
                    AnchorStep::Move { anchor, .. } => {
                        unplace(doc, &t, &anchor.id, None);
                        place(doc, &t, &anchor.id, anchor.start, anchor.end);
                    }
                    AnchorStep::Remove { id } => unplace(doc, &t, id, None),
                }
            }
            Trace::Props { at, props, gained, slots } => {
                let marker = marker_at(doc, &level.t, at + 1).expect("a props step names an island the text holds");
                map::write_json_at(doc, &marker, "props", props);
                let held = obj(doc, &marker, "slots").expect("an island marker holds its slots");
                for k in doc.keys(&held).collect::<Vec<_>>() {
                    if !slots.contains(&k) {
                        doc.delete(&held, k.as_str()).unwrap();
                    }
                }
                for k in slots {
                    if obj(doc, &held, k).is_none() {
                        map::create(doc, &held, k, gained.get(k).unwrap_or(&Content::empty()));
                    }
                }
            }
        }
    }
    for path in touched {
        if let Some((t, value)) = reach(doc, root, after, &path) {
            map::sync_marks(doc, &t, &value.marks);
        }
    }
}

/// The text `path` reaches from the value at `root`, and the value `after`
/// holds there, where the islands and slots it names are still in `after`.
fn reach<'a>(doc: &AutoCommit, root: &ObjId, after: &'a Content, path: &[(String, String)]) -> Option<(ObjId, &'a Content)> {
    let (mut t, mut value) = (text(doc, root), after);
    for (island, slot) in path {
        let k = value.islands.iter().position(|i| &i.id == island)?;
        let at = value.text.chars().enumerate().filter(|(_, ch)| *ch == ISLAND_SLOT).nth(k)?.0;
        let marker = marker_at(doc, &t, at + 1)?;
        let inner = obj(doc, &obj(doc, &marker, "slots")?, slot)?;
        t = text(doc, &inner);
        value = value.islands[k].slots.get(slot)?;
    }
    Some((t, value))
}
