//! A host's offsets. Storage counts Unicode scalar values; a binding speaks
//! its host's units at its boundary (UTF-16 code units in JavaScript) and
//! converts here. A content value converts on its own text; a change converts
//! against the value it applies to, each step on the text the steps before it
//! left.

use crate::change::{self, AnchorStep, ApplyError, Change, IslandStep, MarkStep, Splice, Step};
use crate::model::{Content, Island, Mark, Usv};
use crate::wire::{ParseError, shape};
use serde_json::Value;

/// A unit offsets count in.
pub trait Units: Copy {
    /// The units `c` spans.
    fn char_len(self, c: char) -> usize;

    /// The units `s` spans.
    fn len(self, s: &str) -> usize {
        s.chars().map(|c| self.char_len(c)).sum()
    }
}

/// Unicode scalar values, storage's own unit.
#[derive(Debug, Clone, Copy)]
pub struct Scalars;

/// UTF-16 code units, JavaScript's.
#[derive(Debug, Clone, Copy)]
pub struct Utf16;

impl Units for Scalars {
    fn char_len(self, _: char) -> usize {
        1
    }

    fn len(self, s: &str) -> usize {
        s.chars().count()
    }
}

impl Units for Utf16 {
    fn char_len(self, c: char) -> usize {
        c.len_utf16()
    }
}

/// The scalar offset `n` units into `text`; `None` inside a character or past
/// the end.
pub fn to_scalars(u: impl Units, text: &str, n: usize) -> Option<Usv> {
    let mut at = 0;
    for (k, c) in text.chars().enumerate() {
        if at == n {
            return Some(k);
        }
        at += u.char_len(c);
        if at > n {
            return None;
        }
    }
    (at == n).then(|| text.chars().count())
}

/// The unit offset of scalar offset `n` into `text`, saturating at its end.
pub fn from_scalars(u: impl Units, text: &str, n: Usv) -> usize {
    text.chars().take(n).map(|c| u.char_len(c)).sum()
}

/// `s` split `n` units in; `None` inside a character or past the end.
pub(crate) fn split_at(u: impl Units, s: &str, n: usize) -> Option<(&str, &str)> {
    let k = to_scalars(u, s, n)?;
    Some(s.split_at(crate::usv::char_to_byte(s, k)))
}

/// The scalar offset `n` units into `text`, inside a character taken to its
/// start, and past the end kept as far past it.
fn floor_scalars(u: impl Units, text: &str, n: usize) -> Usv {
    let mut at = 0;
    let mut count = 0;
    for (k, c) in text.chars().enumerate() {
        let next = at + u.char_len(c);
        if next > n {
            return k;
        }
        at = next;
        count = k + 1;
    }
    count + (n - at)
}

#[derive(Clone, Copy)]
enum Dir {
    In,
    Out,
    Raw,
}

fn offset(u: impl Units, dir: Dir, text: &str, n: usize) -> Option<usize> {
    match dir {
        Dir::In => to_scalars(u, text, n),
        Dir::Out => Some(from_scalars(u, text, n)),
        Dir::Raw => Some(floor_scalars(u, text, n)),
    }
}

fn marks(u: impl Units, dir: Dir, text: &str, marks: &mut [Mark]) -> Option<()> {
    for m in marks {
        m.start = offset(u, dir, text, m.start)?;
        m.end = offset(u, dir, text, m.end)?;
    }
    Some(())
}

fn islands(u: impl Units, dir: Dir, islands: &mut [Island]) -> Option<()> {
    for i in islands {
        for slot in i.slots.values_mut() {
            content(u, dir, slot)?;
        }
    }
    Some(())
}

fn content(u: impl Units, dir: Dir, c: &mut Content) -> Option<()> {
    marks(u, dir, &c.text, &mut c.marks)?;
    for a in &mut c.anchors {
        a.start = offset(u, dir, &c.text, a.start)?;
        a.end = offset(u, dir, &c.text, a.end)?;
    }
    islands(u, dir, &mut c.islands)
}

const SPLIT: &str = "an offset falls inside a character or past its text";

impl Content {
    /// [`from_value`](Self::from_value) with offsets in `u`. A v1 value
    /// counts scalar values, as v1 did.
    pub fn from_value_in(v: &Value, u: impl Units) -> Result<Content, ParseError> {
        if crate::model::json_too_deep(v) || crate::wire::is_v1(v) {
            return Content::from_value(v);
        }
        let mut v = v.clone();
        if !value_to_scalars(&mut v, u) {
            return Err(shape(SPLIT));
        }
        Content::from_value(&v)
    }

    /// [`to_value`](Self::to_value) with offsets in `u`.
    pub fn to_value_in(&self, u: impl Units) -> Value {
        let mut c = self.clone();
        content(u, Dir::Out, &mut c);
        c.to_value()
    }
}

/// Whether `v` is shaped as a v2 content value: a `text` string and a
/// `blocks` array.
pub fn is_content_value(v: &Value) -> bool {
    v.get("text").is_some_and(Value::is_string) && v.get("blocks").is_some_and(Value::is_array)
}

/// Move a content value's offsets from `u` to scalar values in place, its
/// other keys untouched; false when one falls inside a character or past its
/// text. A value not shaped as v2 content is left as it is.
pub fn value_to_scalars(v: &mut Value, u: impl Units) -> bool {
    value_offsets(v, u, Dir::In).is_some()
}

/// Move a content value's offsets from scalar values to `u` in place.
pub fn value_from_scalars(v: &mut Value, u: impl Units) {
    value_offsets(v, u, Dir::Out);
}

/// Move a [raw](crate::repair::Raw) value's offsets from `u` to scalar values
/// in place, an offset inside a character taken to its start, for
/// [`repair`](crate::repair::repair) to settle.
pub fn raw_to_scalars(v: &mut Value, u: impl Units) {
    value_offsets(v, u, Dir::Raw);
}

fn value_offsets(v: &mut Value, u: impl Units, dir: Dir) -> Option<()> {
    let raw = matches!(dir, Dir::Raw) && v.get("text").is_some_and(Value::is_string);
    if !raw && !is_content_value(v) {
        return Some(());
    }
    let text = v["text"].as_str().unwrap_or_default().to_string();
    for key in ["marks", "anchors"] {
        let Some(Value::Array(items)) = v.get_mut(key) else { continue };
        for item in items {
            for edge in ["start", "end"] {
                let Some(n) = item.get(edge).and_then(Value::as_u64) else { continue };
                let n = offset(u, dir, &text, usize::try_from(n).ok()?)?;
                item[edge] = n.into();
            }
        }
    }
    let islands: Vec<&mut Value> = match v.get_mut("islands") {
        Some(Value::Array(list)) => list.iter_mut().collect(),
        Some(Value::Object(map)) if raw => map.values_mut().collect(),
        _ => Vec::new(),
    };
    for island in islands {
        if let Some(Value::Object(slots)) = island.get_mut("slots") {
            for slot in slots.values_mut() {
                value_offsets(slot, u, dir)?;
            }
        }
    }
    Some(())
}

/// `change`, its offsets in `u`, in scalar values, read against `before`.
/// Refused as [`change::apply`] refuses it, and a step whose offset falls
/// inside a character is stale.
pub fn change_to_scalars(change: &Change, before: &Content, u: impl Units) -> Result<Change, ApplyError> {
    convert(change, before, u, Dir::In)
}

/// `change`, read against `before`, its offsets in `u`.
pub fn change_from_scalars(change: &Change, before: &Content, u: impl Units) -> Result<Change, ApplyError> {
    convert(change, before, u, Dir::Out)
}

fn convert(change: &Change, before: &Content, u: impl Units, dir: Dir) -> Result<Change, ApplyError> {
    let mut c = before.clone();
    steps(&mut c, &change.steps, u, dir)
}

fn steps(c: &mut Content, steps_: &[Step], u: impl Units, dir: Dir) -> Result<Change, ApplyError> {
    let mut out = Vec::with_capacity(steps_.len());
    for (n, step) in steps_.iter().enumerate() {
        let conv = step_in(c, step, n, u, dir)?;
        let scalar = match dir {
            Dir::In | Dir::Raw => &conv,
            Dir::Out => step,
        };
        change::run(c, std::slice::from_ref(scalar), &mut Vec::new(), &mut Vec::new())
            .map_err(|e| renumber(e, n))?;
        out.push(conv);
    }
    Ok(Change { steps: out })
}

fn renumber(e: ApplyError, n: usize) -> ApplyError {
    match e {
        ApplyError::Stale { .. } => ApplyError::Stale { step: n },
        ApplyError::BadStep { reason, .. } => ApplyError::BadStep { step: n, reason },
        e => e,
    }
}

fn step_in(c: &Content, step: &Step, n: usize, u: impl Units, dir: Dir) -> Result<Step, ApplyError> {
    let stale = || ApplyError::Stale { step: n };
    let bad = || ApplyError::BadStep { step: n, reason: SPLIT };
    let at = |p: usize| offset(u, dir, &c.text, p).ok_or_else(stale);
    let mut step = step.clone();
    match &mut step {
        Step::Splice(s) => {
            s.at = at(s.at)?;
            splice(s, u, dir).ok_or_else(bad)?;
        }
        Step::SetBlock(_) | Step::Container(_) | Step::Anchor(AnchorStep::Remove { .. }) => {}
        Step::Mark(MarkStep::Add(m)) => {
            (m.start, m.end) = (at(m.start)?, at(m.end)?);
        }
        Step::Mark(MarkStep::Remove { start, end, .. }) => {
            (*start, *end) = (at(*start)?, at(*end)?);
        }
        Step::Anchor(AnchorStep::Set(a)) => {
            (a.start, a.end) = (at(a.start)?, at(a.end)?);
        }
        Step::Island(IslandStep::Props { slots, .. }) => {
            for slot in slots.values_mut() {
                content(u, dir, slot).ok_or_else(bad)?;
            }
        }
        Step::Island(IslandStep::Insert { at: p, island }) => {
            *p = at(*p)?;
            islands(u, dir, std::slice::from_mut(island)).ok_or_else(bad)?;
        }
        Step::Nested { island, slot, change } => {
            let mut inner = c
                .slot(island, slot)
                .cloned()
                .ok_or_else(|| ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() })?;
            *change = steps(&mut inner, &change.steps, u, dir)?;
        }
    }
    Ok(step)
}

fn splice(s: &mut Splice, u: impl Units, dir: Dir) -> Option<()> {
    marks(u, dir, &s.insert, &mut s.marks)?;
    islands(u, dir, &mut s.islands)?;
    if let Some(r) = &mut s.removed {
        marks(u, dir, &s.delete, &mut r.marks)?;
        islands(u, dir, &mut r.islands)?;
    }
    Some(())
}
