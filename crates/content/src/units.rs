//! A host's offsets. Storage counts Unicode scalar values; a binding speaks
//! its host's units at its boundary (UTF-16 code units in JavaScript) and
//! converts here. A content value converts on its own text; a change converts
//! against the value it applies to, each step on the text the steps before it
//! left.

use crate::change::{AnchorStep, ApplyError, Change, IslandStep, MarkStep, Run, Splice, Step};
use crate::draft::Draft;
use crate::model::{Content, Island, Mark, Scalar};
use crate::wire::{ParseError, shape};
use serde_json::Value;

/// A scalar index into `text` as a UTF-8 byte offset. Saturates to
/// `text.len()` at or past the end, so it is safe as a slice bound.
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

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
pub fn to_scalars(u: impl Units, text: &str, n: usize) -> Option<Scalar> {
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
pub fn from_scalars(u: impl Units, text: &str, n: Scalar) -> usize {
    text.chars().take(n).map(|c| u.char_len(c)).sum()
}

#[derive(Clone, Copy)]
enum Dir {
    In,
    Out,
}

/// Where a text's characters of other than one unit sit, so an offset
/// converts by binary search rather than a walk from the text's start.
struct Index {
    /// Each such character's scalar offset, the unit offset it starts at, and
    /// its units, in order.
    wide: Vec<(Scalar, usize, usize)>,
    len: Scalar,
    units: usize,
}

impl Index {
    fn new(u: impl Units, text: &str) -> Index {
        Index { wide: Index::read(u, text, 0, 0), len: text.chars().count(), units: u.len(text) }
    }

    /// The characters of `text` of other than one unit, as `wide` holds them,
    /// for `text` starting at scalar offset `at` and unit offset `units`.
    fn read(u: impl Units, text: &str, at: Scalar, units: usize) -> Vec<(Scalar, usize, usize)> {
        let mut out = Vec::new();
        let mut units = units;
        for (k, c) in text.chars().enumerate() {
            let n = u.char_len(c);
            if n != 1 {
                out.push((at + k, units, n));
            }
            units += n;
        }
        out
    }

    fn offset(&self, dir: Dir, n: usize) -> Option<usize> {
        match dir {
            Dir::In => self.to_scalars(n),
            Dir::Out => Some(self.to_units(n)),
        }
    }

    /// [`from_scalars`].
    fn to_units(&self, p: Scalar) -> usize {
        if p >= self.len {
            return self.units;
        }
        match self.wide.partition_point(|w| w.0 < p) {
            0 => p,
            k => {
                let (at, start, n) = self.wide[k - 1];
                start + n + (p - at - 1)
            }
        }
    }

    /// The scalar offset unit offset `n` reaches, and the scalar offset of the
    /// character `n` falls inside, if any.
    fn locate(&self, n: usize) -> (Scalar, Option<Scalar>) {
        let k = self.wide.partition_point(|w| w.1 + w.2 <= n);
        let inside = self.wide.get(k).filter(|w| w.1 < n).map(|w| w.0);
        let p = match k {
            0 => n,
            k => {
                let (at, start, len) = self.wide[k - 1];
                at + 1 + (n - start - len)
            }
        };
        (p, inside)
    }

    /// The scalar offset unit offset `n` reaches, one inside a character
    /// moved to its start and one past the text to its end.
    fn clamped(&self, n: usize) -> Scalar {
        match self.locate(n) {
            (_, Some(at)) => at,
            (p, None) => p.min(self.len),
        }
    }

    /// [`to_scalars`].
    fn to_scalars(&self, n: usize) -> Option<Scalar> {
        match self.locate(n) {
            (_, Some(_)) => None,
            (p, None) => (p <= self.len).then_some(p),
        }
    }

    /// The text with `delete`, at scalar offset `at`, replaced by `insert`.
    fn splice(&mut self, u: impl Units, at: Scalar, delete: &str, insert: &str) {
        let (dn, du) = (delete.chars().count(), u.len(delete));
        let (m, mu) = (insert.chars().count(), u.len(insert));
        let from = self.to_units(at);
        let (i0, i1) = (self.wide.partition_point(|w| w.0 < at), self.wide.partition_point(|w| w.0 < at + dn));
        for w in &mut self.wide[i1..] {
            w.0 = w.0 - dn + m;
            w.1 = w.1 - du + mu;
        }
        let fresh = Index::read(u, insert, at, from);
        self.wide.splice(i0..i1, fresh);
        self.len = self.len - dn + m;
        self.units = self.units - du + mu;
    }

    /// The text as `step`, a step in scalar values, leaves it.
    fn follow(&mut self, u: impl Units, step: &Step) {
        match step {
            Step::Splice(s) => self.splice(u, s.at, &s.delete, &s.insert),
            Step::Island(IslandStep::Insert { at, .. }) => self.splice(u, *at, "", "\u{FFFC}"),
            _ => {}
        }
    }
}

fn marks(u: impl Units, dir: Dir, text: &str, marks: &mut [Mark]) -> Option<()> {
    if marks.is_empty() {
        return Some(());
    }
    marks_on(&Index::new(u, text), dir, marks)
}

fn marks_on(index: &Index, dir: Dir, marks: &mut [Mark]) -> Option<()> {
    for m in marks {
        m.start = index.offset(dir, m.start)?;
        m.end = index.offset(dir, m.end)?;
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
    let index = Index::new(u, &c.text);
    marks_on(&index, dir, &mut c.marks)?;
    for a in &mut c.anchors {
        a.start = index.offset(dir, a.start)?;
        a.end = index.offset(dir, a.end)?;
    }
    islands(u, dir, &mut c.islands)
}

const SPLIT: &str = "an offset falls inside a character or past its text";

impl Content {
    /// [`from_value`](Self::from_value) with offsets in `u`.
    pub fn from_value_in(v: &Value, u: impl Units) -> Result<Content, ParseError> {
        if crate::model::json_too_deep(v) {
            return Content::from_value(v);
        }
        let mut v = v.clone();
        if !value_to_scalars(&mut v, u) {
            return Err(shape(SPLIT));
        }
        Content::from_value(&v)
    }

    /// [`from_value_lenient`](Self::from_value_lenient) with offsets in `u`.
    pub fn from_value_lenient_in(v: &Value, u: impl Units) -> Result<crate::wire::Lenient, ParseError> {
        if crate::model::json_too_deep(v) {
            return Content::from_value_lenient(v);
        }
        let mut v = v.clone();
        if !value_to_scalars(&mut v, u) {
            return Err(shape(SPLIT));
        }
        Content::from_value_lenient(&v)
    }

    /// [`to_value`](Self::to_value) with offsets in `u`.
    pub fn to_value_in(&self, u: impl Units) -> Value {
        let mut c = self.clone();
        content(u, Dir::Out, &mut c);
        c.to_value()
    }
}

impl crate::repair::Raw {
    /// [`from_value`](Self::from_value) with offsets in `u`, converted as
    /// [`raw_to_scalars`] does.
    pub fn from_value_in(v: &Value, u: impl Units) -> Result<crate::repair::Raw, ParseError> {
        if crate::model::json_too_deep(v) {
            return crate::repair::Raw::from_value(v);
        }
        let mut v = v.clone();
        raw_to_scalars(&mut v, u);
        crate::repair::Raw::from_value(&v)
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
    value_offsets(v, u, Dir::In, false).is_some()
}

/// Move a content value's offsets from scalar values to `u` in place.
pub fn value_from_scalars(v: &mut Value, u: impl Units) {
    value_offsets(v, u, Dir::Out, false);
}

/// Move a raw value's offsets ([`Raw`](crate::repair::Raw)) from `u` to scalar
/// values in place, its `islands` an array or an object keyed by id. An offset
/// inside a character moves to that character's start and one past the text to
/// its end, since repair clips what the text no longer holds.
pub fn raw_to_scalars(v: &mut Value, u: impl Units) {
    value_offsets(v, u, Dir::In, true);
}

fn value_offsets(v: &mut Value, u: impl Units, dir: Dir, raw: bool) -> Option<()> {
    if !is_content_value(v) {
        return Some(());
    }
    let index = Index::new(u, v["text"].as_str().unwrap_or_default());
    for key in ["marks", "anchors"] {
        let Some(Value::Array(items)) = v.get_mut(key) else { continue };
        for item in items {
            for edge in ["start", "end"] {
                let Some(n) = item.get(edge).and_then(Value::as_u64) else { continue };
                let n = usize::try_from(n).unwrap_or(usize::MAX);
                let n = if raw { index.clamped(n) } else { index.offset(dir, n)? };
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
                value_offsets(slot, u, dir, raw)?;
            }
        }
    }
    Some(())
}

/// `change`, its offsets in `u`, in scalar values, read against `before`.
/// Refused as [`apply`](crate::change::apply) refuses it, and a step whose
/// offset falls inside a character is stale.
pub fn change_to_scalars(change: &Change, before: &Content, u: impl Units) -> Result<Change, ApplyError> {
    convert(change, before, u, Dir::In)
}

/// `change`, read against `before`, its offsets in `u`.
pub fn change_from_scalars(change: &Change, before: &Content, u: impl Units) -> Result<Change, ApplyError> {
    convert(change, before, u, Dir::Out)
}

fn convert(change: &Change, before: &Content, u: impl Units, dir: Dir) -> Result<Change, ApplyError> {
    steps(before.clone(), &change.steps, u, dir)
}

/// `steps` converted, each on the text the steps before it left, as one run
/// applies them.
fn steps(c: Content, steps_: &[Step], u: impl Units, dir: Dir) -> Result<Change, ApplyError> {
    let mut index = Index::new(u, &c.text);
    let mut draft = Draft::new(c);
    let mut run = Run::new(false);
    let mut out = Vec::with_capacity(steps_.len());
    for (n, step) in steps_.iter().enumerate() {
        let conv = step_in(&draft, &index, step, n, u, dir)?;
        let scalar = match dir {
            Dir::In => &conv,
            Dir::Out => step,
        };
        run.step(&mut draft, scalar, n).map_err(|e| renumber(e, n))?;
        index.follow(u, scalar);
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

fn step_in(d: &Draft, index: &Index, step: &Step, n: usize, u: impl Units, dir: Dir) -> Result<Step, ApplyError> {
    let stale = || ApplyError::Stale { step: n };
    let bad = || ApplyError::BadStep { step: n, reason: SPLIT };
    let at = |p: usize| index.offset(dir, p).ok_or_else(stale);
    let mut step = step.clone();
    match &mut step {
        Step::Splice(s) => {
            s.at = at(s.at)?;
            splice(s, u, dir).ok_or_else(bad)?;
        }
        Step::SetBlock(_) | Step::Container(_) | Step::Anchor(AnchorStep::Remove { .. }) => {}
        Step::Mark(MarkStep::Add { mark: m, .. }) => {
            (m.start, m.end) = (at(m.start)?, at(m.end)?);
        }
        Step::Mark(MarkStep::Remove { start, end, .. }) => {
            (*start, *end) = (at(*start)?, at(*end)?);
        }
        Step::Anchor(AnchorStep::Place { anchor: a, .. } | AnchorStep::Move { anchor: a, .. }) => {
            (a.start, a.end) = (at(a.start)?, at(a.end)?);
        }
        Step::Island(IslandStep::Props { slots, .. }) => {
            for slot in slots.values_mut() {
                content(u, dir, slot).ok_or_else(bad)?;
            }
        }
        Step::Island(IslandStep::Insert { at: p, island, .. }) => {
            *p = at(*p)?;
            islands(u, dir, std::slice::from_mut(island)).ok_or_else(bad)?;
        }
        Step::Nested { island, slot, change } => {
            let inner = d
                .c
                .slot(island, slot)
                .cloned()
                .ok_or_else(|| ApplyError::UnknownSlot { island: island.clone(), slot: slot.clone() })?;
            *change = steps(inner, &change.steps, u, dir)?;
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
