//! Marks and anchors held to their edges once two sides of a write merge.
//!
//! A mark's begin op is inserted after the element before it, as text typed
//! there is, and Automerge orders two inserts after one element by op id. So
//! a range anchor, a point's marks or a formatting mark one side put can take
//! in text the other side typed at its edge, and which side of the edge that
//! text lands on depends on which actor sorts first. [`clip`] takes back each
//! edge of a mark that grew so: an element only one side holds, which that
//! side does not format so.

use crate::map::reach;
use automerge::marks::{ExpandMark, Mark};
use automerge::transaction::Transactable;
use automerge::{ActorId, AutoCommit, ChangeHash, ObjId, ReadDoc, ScalarValue, Value};
use quillmark_content::change::{AnchorStep, MarkStep, Trace};
use std::collections::HashMap;

/// Each text a trace inserted into or put a mark or an anchor on, by the
/// islands and slots that reach it from the value's own text.
fn touched(trace: &[Trace]) -> Vec<Vec<(String, String)>> {
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut path = Vec::new();
    for step in trace {
        let puts = match step {
            Trace::Enter { island, slot, .. } => {
                path.push((island.clone(), slot.clone()));
                false
            }
            Trace::Leave => {
                path.pop();
                false
            }
            Trace::Splice { insert, anchors, .. } => !insert.is_empty() || !anchors.is_empty(),
            Trace::Mark { step: MarkStep::Add { .. }, .. } => true,
            Trace::Anchor(AnchorStep::Place { .. } | AnchorStep::Move { .. }) => true,
            _ => false,
        };
        if puts && !out.contains(&path) {
            out.push(path.clone());
        }
    }
    out
}

/// Whether a value a text's iteration yields is one of its elements, a
/// character or a marker, rather than a mark's boundary.
fn element(v: &Value<'_>) -> bool {
    match v {
        Value::Object(_) => true,
        Value::Scalar(s) => matches!(s.as_ref(), ScalarValue::Str(s) if s.chars().count() == 1),
    }
}

/// One side of a merge: the ops only it made, by actor, and its heads, at
/// which it reads each text as `views` holds once read.
struct Side {
    made: HashMap<ActorId, Vec<(u64, u64)>>,
    heads: Vec<ChangeHash>,
    views: HashMap<ObjId, View>,
}

/// A text as one side reads it: each element's index, and the marks.
struct View {
    at: HashMap<ObjId, usize>,
    marks: Vec<Mark>,
}

impl Side {
    /// The side at `heads`, whose own ops are those of the changes the other
    /// side, at `other`, lacks.
    fn new(doc: &mut AutoCommit, heads: &[ChangeHash], other: &[ChangeHash]) -> Side {
        let mut made: HashMap<ActorId, Vec<(u64, u64)>> = HashMap::new();
        for c in doc.get_changes_meta(other) {
            made.entry(c.actor.into_owned()).or_default().push((c.start_op, c.max_op));
        }
        Side { made, heads: heads.to_vec(), views: HashMap::new() }
    }

    fn made(&self, id: &ObjId) -> bool {
        match id {
            ObjId::Id(counter, actor, _) => self.made.get(actor).is_some_and(|ops| ops.iter().any(|(s, m)| (*s..=*m).contains(counter))),
            ObjId::Root => false,
        }
    }

    /// Whether this side formats the element `id` of the text `t` with the
    /// mark `name` of `value`.
    fn formats(&mut self, doc: &AutoCommit, t: &ObjId, id: &ObjId, name: &str, value: &ScalarValue) -> bool {
        let heads = &self.heads;
        let view = self.views.entry(t.clone()).or_insert_with(|| View {
            at: doc.values_at(t, heads).filter(|(v, _)| element(v)).enumerate().map(|(j, (_, id))| (id, j)).collect(),
            marks: doc.marks_at(t, heads).unwrap_or_default(),
        });
        view.at.get(id).is_some_and(|&j| view.marks.iter().any(|m| m.name() == name && m.value() == value && m.start <= j && j < m.end))
    }
}

/// After a merge of the side at `mine`, which lowered `trace`, and the side
/// at `theirs`, takes back each edge of a mark, an anchor's included, in a
/// text `trace` touched, that grew over an element one side alone holds and
/// does not format so. Answers whether it changed anything.
pub(crate) fn clip(doc: &mut AutoCommit, root: &ObjId, mine: &[ChangeHash], theirs: &[ChangeHash], trace: &[Trace]) -> bool {
    let touched = touched(trace);
    if touched.is_empty() {
        return false;
    }
    let mut sides = [Side::new(doc, mine, theirs), Side::new(doc, theirs, mine)];
    if sides.iter().all(|s| s.made.is_empty()) {
        return false;
    }
    let mut changed = false;
    for path in touched {
        if let Some(t) = reach(doc, root, &path) {
            changed |= clip_text(doc, &t, &mut sides);
        }
    }
    changed
}

fn clip_text(doc: &mut AutoCommit, t: &ObjId, sides: &mut [Side; 2]) -> bool {
    let runs = doc.marks(t).unwrap();
    let mut shed = Vec::new();
    for run in &runs {
        let (name, value) = (run.name(), run.value());
        let mut grew = |i: usize| {
            let Ok(Some((_, id))) = doc.get(t, i) else { return false };
            sides.iter_mut().find(|s| s.made(&id)).is_some_and(|s| !s.formats(doc, t, &id, name, value))
        };
        let (mut s, mut e) = (run.start, run.end);
        while s < e && grew(s) {
            s += 1;
        }
        while e > s && grew(e - 1) {
            e -= 1;
        }
        for (from, to) in [(run.start, s), (e, run.end)] {
            if from < to {
                shed.push((name.to_string(), from, to));
            }
        }
    }
    for (name, from, to) in &shed {
        doc.unmark(t, name, *from, *to, ExpandMark::None).unwrap();
    }
    !shed.is_empty()
}
