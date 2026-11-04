//! A value while a change runs on it. Its text and marks are held block by
//! block, with each block's start offset and each island slot's offset kept,
//! so a step reads and rewrites only the blocks it touches and finds them by
//! binary search.

use crate::content::model::{Block, Content, ISLAND_SLOT, Mark, Scalar};
use std::collections::HashMap;

pub(crate) struct Draft {
    /// The value's records, containers, anchors and islands. Its `text` and
    /// `marks` are empty until [`finish`](Self::finish): `segs` holds them.
    pub(crate) c: Content,
    segs: Vec<Seg>,
    /// The offset each block's text starts at.
    starts: Vec<Scalar>,
    len: Scalar,
    /// The offset of each island's slot, in slot order.
    slots: Vec<Scalar>,
    /// Each block id's index, `None` for an id held twice; unset while the
    /// records have changed since it was built.
    ids: Option<HashMap<String, Option<usize>>>,
    scans: usize,
    /// The marks `segs` hold together.
    marks: usize,
}

/// One block's text and the marks over it, at offsets into it.
struct Seg {
    text: String,
    len: Scalar,
    marks: Vec<Mark>,
}

/// A block id held twice: a step that names it cannot say which it means.
pub(crate) struct HeldTwice;

/// Lookups by scanning the records before an index is built over them.
const SCANS: usize = 8;

impl Draft {
    pub(crate) fn new(mut c: Content) -> Draft {
        if !marks_within_blocks(&c) {
            c.normalize_marks();
        }
        if !c.anchors.is_sorted_by_key(|a| (a.start, a.end, a.id.as_str())) {
            c.sort_anchors();
        }
        let text = std::mem::take(&mut c.text);
        let mut marks = std::mem::take(&mut c.marks).into_iter().peekable();
        let mut segs = Vec::with_capacity(c.blocks.len());
        let mut starts = Vec::with_capacity(c.blocks.len());
        let mut slots = Vec::new();
        let mut at = 0;
        for piece in text.split('\n') {
            let mut len = 0;
            for ch in piece.chars() {
                if ch == ISLAND_SLOT {
                    slots.push(at + len);
                }
                len += 1;
            }
            let mut own = Vec::new();
            while let Some(m) = marks.next_if(|m| m.start < at + len) {
                own.push(Mark::new(m.start - at, m.end - at, m.kind));
            }
            starts.push(at);
            segs.push(Seg { text: piece.to_string(), len, marks: own });
            at += len + 1;
        }
        if let Some(last) = segs.last_mut() {
            let from = at - last.len - 1;
            last.marks.extend(marks.map(|m| Mark::new(m.start - from, m.end - from, m.kind)));
        }
        let marks = segs.iter().map(|s| s.marks.len()).sum();
        Draft { c, segs, starts, len: at - 1, slots, ids: None, scans: 0, marks }
    }

    pub(crate) fn finish(mut self) -> Content {
        let mut text = String::with_capacity(self.segs.iter().map(|s| s.text.len() + 1).sum());
        let mut marks = Vec::with_capacity(self.segs.iter().map(|s| s.marks.len()).sum());
        for (k, seg) in self.segs.into_iter().enumerate() {
            if k > 0 {
                text.push('\n');
            }
            text.push_str(&seg.text);
            let at = self.starts[k];
            marks.extend(seg.marks.into_iter().map(|m| Mark::new(m.start + at, m.end + at, m.kind)));
        }
        self.c.text = text;
        self.c.marks = marks;
        self.c
    }

    pub(crate) fn len(&self) -> Scalar {
        self.len
    }

    /// The marks the value holds.
    pub(crate) fn mark_count(&self) -> usize {
        self.marks
    }

    /// The index of the block whose text holds offset `p`, its end included.
    pub(crate) fn block_at(&self, p: Scalar) -> usize {
        self.starts.partition_point(|&s| s <= p).saturating_sub(1)
    }

    pub(crate) fn block_end(&self, k: usize) -> Scalar {
        self.starts[k] + self.segs[k].len
    }

    /// How many island slots sit before offset `p`.
    pub(crate) fn slots_before(&self, p: Scalar) -> usize {
        self.slots.partition_point(|&s| s < p)
    }

    /// The text over `[start, end)`, which must lie within the text.
    pub(crate) fn slice(&self, start: Scalar, end: Scalar) -> String {
        let (k0, k1) = (self.block_at(start), self.block_at(end));
        let mut out = String::new();
        for k in k0..=k1 {
            if k > k0 {
                out.push('\n');
            }
            let seg = &self.segs[k];
            let lo = if k == k0 { start - self.starts[k] } else { 0 };
            let hi = if k == k1 { end - self.starts[k] } else { seg.len };
            out.push_str(&seg.text[seg.byte(lo)..seg.byte(hi)]);
        }
        out
    }

    /// Blocks `k0` through `k1`: where their text starts, the text with the
    /// `\n` between them, and the marks over it at offsets into the value.
    pub(crate) fn window(&self, k0: usize, k1: usize) -> (Scalar, String, Vec<Mark>) {
        let mut text = String::new();
        let mut marks = Vec::new();
        for k in k0..=k1 {
            if k > k0 {
                text.push('\n');
            }
            text.push_str(&self.segs[k].text);
            let at = self.starts[k];
            marks.extend(self.segs[k].marks.iter().map(|m| Mark::new(m.start + at, m.end + at, m.kind.clone())));
        }
        (self.starts[k0], text, marks)
    }

    /// The marks over blocks `k0` through `k1` replaced by `marks`, sorted,
    /// none over a `\n`, at offsets into the value.
    pub(crate) fn set_marks(&mut self, k0: usize, k1: usize, marks: Vec<Mark>) {
        let mut marks = marks.into_iter().peekable();
        for k in k0..=k1 {
            let (at, end) = (self.starts[k], self.block_end(k));
            let own = &mut self.segs[k].marks;
            self.marks -= own.len();
            own.clear();
            while let Some(m) = marks.next_if(|m| m.start < end || k == k1) {
                own.push(Mark::new(m.start - at, m.end - at, m.kind));
            }
            self.marks += own.len();
        }
    }

    /// Blocks `k0` through `k1` replaced by `text`, which a `\n` breaks into
    /// blocks, the marks over it `marks`, as [`set_marks`](Self::set_marks)
    /// takes them, and the records of the blocks after the first `records`,
    /// one per `\n` in `text`. Answers the records it replaced.
    pub(crate) fn rewrite(&mut self, k0: usize, k1: usize, text: &str, marks: Vec<Mark>, records: Vec<Block>) -> Vec<Block> {
        let at = self.starts[k0];
        let old_end = self.block_end(k1);
        let mut segs = Vec::with_capacity(records.len() + 1);
        let mut starts = Vec::with_capacity(records.len() + 1);
        let mut slots = Vec::new();
        let mut p = at;
        for piece in text.split('\n') {
            let mut len = 0;
            for ch in piece.chars() {
                if ch == ISLAND_SLOT {
                    slots.push(p + len);
                }
                len += 1;
            }
            starts.push(p);
            segs.push(Seg { text: piece.to_string(), len, marks: Vec::new() });
            p += len + 1;
        }
        let new_end = p - 1;
        let n = segs.len();
        self.marks -= self.segs[k0..=k1].iter().map(|s| s.marks.len()).sum::<usize>();
        self.segs.splice(k0..=k1, segs);
        for s in &mut self.starts[k1 + 1..] {
            *s = *s - old_end + new_end;
        }
        self.starts.splice(k0..=k1, starts);
        let (s0, s1) = (self.slots_before(at), self.slots_before(old_end));
        for s in &mut self.slots[s1..] {
            *s = *s - old_end + new_end;
        }
        self.slots.splice(s0..s1, slots);
        self.len = self.len - old_end + new_end;
        self.set_marks(k0, k0 + n - 1, marks);
        if k1 == k0 && records.is_empty() {
            return Vec::new();
        }
        self.ids = None;
        self.scans = 0;
        self.c.blocks.splice(k0 + 1..=k1, records).collect()
    }

    /// The index of the block `id` names, `None` where no block holds it.
    pub(crate) fn block_index(&mut self, id: &str) -> Result<Option<usize>, HeldTwice> {
        if self.ids.is_none() && self.scans >= SCANS {
            let mut ids = HashMap::with_capacity(self.c.blocks.len());
            for (k, b) in self.c.blocks.iter().enumerate() {
                ids.entry(b.id.clone()).and_modify(|e| *e = None).or_insert(Some(k));
            }
            self.ids = Some(ids);
        }
        match &self.ids {
            Some(ids) => match ids.get(id) {
                None => Ok(None),
                Some(Some(k)) => Ok(Some(*k)),
                Some(None) => Err(HeldTwice),
            },
            None => {
                self.scans += 1;
                let mut held = self.c.blocks.iter().enumerate().filter(|(_, b)| b.id == id).map(|(k, _)| k);
                match (held.next(), held.next()) {
                    (Some(_), Some(_)) => Err(HeldTwice),
                    (k, _) => Ok(k),
                }
            }
        }
    }
}

impl Seg {
    /// The byte offset of the `n`th character.
    fn byte(&self, n: Scalar) -> usize {
        if self.len == self.text.len() {
            return n;
        }
        self.text.char_indices().nth(n).map_or(self.text.len(), |(b, _)| b)
    }
}

/// Whether each mark lies inside one block and the marks run in order.
fn marks_within_blocks(c: &Content) -> bool {
    let mut ends: Vec<Scalar> = c.text.chars().enumerate().filter(|&(_, ch)| ch == '\n').map(|(i, _)| i).collect();
    ends.push(c.len_scalars());
    let mut last = (0, 0);
    c.marks.iter().all(|m| {
        let k = ends.partition_point(|&e| e < m.start);
        let fits = m.start < m.end && k < ends.len() && m.end <= ends[k] && (m.start, m.end) >= last;
        last = (m.start, m.end);
        fits
    })
}
