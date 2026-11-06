//! Changes for writers with no steps. [`diff`] builds the change that takes
//! one value to another over a word-aligned text diff; [`align`] gives a value
//! read fresh from markdown the ids of what it kept; [`revise`] and
//! [`replace`] are the producers a markdown rewrite and a whole-value write
//! use.

use crate::change::{
    AnchorStep, ApplyError, Change, ContainerStep, IslandStep, SetBlock, Splice, Step, apply,
    mark_fixups, run,
};
use crate::ids::{Minter, prefix};
use crate::lines::delta::{Op, carry_pinned, diff_words as text_diff, relocate, without_sentinel};
use crate::lines::import::{Against, ImportError, ImportWarning, from_markdown_against};
use crate::lines::island::IslandType;
use crate::migrate::{from_lines, to_lines};
use crate::model::{
    Block, BlockKind, Codec, Container, ContainerKind, Content, ISLAND_SLOT, Invariant, Island,
    Mark, MarkKind, Scalar,
};
use crate::table::{self, Table};
use std::collections::{HashMap, HashSet};

/// The change that takes `before` to `after`, refused where `after` is not a
/// valid value. What no change can reach comes out under `before`'s ids
/// instead: the first block's record, which no `\n` carries, and a container
/// whose id `before` holds with another kind.
pub fn diff(before: &Content, after: &Content) -> Result<Change, Invariant> {
    after.validate()?;
    Ok(diff_with(before, after, None))
}

fn diff_with(before: &Content, after: &Content, carry: Option<&HashSet<String>>) -> Change {
    Change::new(diff_level(before, after, carry, true))
}

/// The mark and anchor steps that take `now` to `want` on one level, where
/// the two differ in nothing else.
fn fixups(now: &Content, want: &Content) -> Vec<Step> {
    let mut out = mark_fixups(&now.marks, &want.marks);
    let held: HashMap<&str, &crate::model::Anchor> =
        now.anchors.iter().map(|a| (a.id.as_str(), a)).collect();
    let wanted: HashSet<&str> = want.anchors.iter().map(|a| a.id.as_str()).collect();
    for a in &want.anchors {
        match held.get(a.id.as_str()) {
            Some(h) if *h == a => {}
            Some(_) => out.push(Step::Anchor(AnchorStep::Move {
                anchor: a.clone(),
                quote: None,
            })),
            None => out.push(Step::Anchor(AnchorStep::Place {
                anchor: a.clone(),
                quote: None,
            })),
        }
    }
    for a in &now.anchors {
        if !wanted.contains(a.id.as_str()) {
            out.push(Step::Anchor(AnchorStep::Remove { id: a.id.clone() }));
        }
    }
    out
}

/// The fix-ups to `want`, less each anchor removal `carry` leaves to ride
/// its text: one whose id `after` names nowhere.
fn fixups_carrying(cur: &Content, want: &Content, carry: Option<&HashSet<String>>) -> Vec<Step> {
    fixups(cur, want)
        .into_iter()
        .filter(|s| match (s, carry) {
            (Step::Anchor(AnchorStep::Remove { id }), Some(named)) => named.contains(id),
            _ => true,
        })
        .collect()
}

/// The change that takes `before` to `after` where the two differ over one
/// range of the text: `[from, to)` of `before`, which `after` holds as
/// `[from, to + after.len - before.len)`, as a ProseMirror `ReplaceStep`
/// names it. Its text is one splice over the characters of that range that
/// differ, never coarser than the range, and its records, containers, marks,
/// anchors and islands follow as [`diff`]'s do, so applying it reaches what
/// `diff` reaches. Where the texts differ outside the range, or a block or
/// island there holds another id than `before`'s, the range is not the
/// change's, and the change is [`diff`]'s.
pub fn diff_range(
    before: &Content,
    after: &Content,
    from: Scalar,
    to: Scalar,
) -> Result<Change, Invariant> {
    after.validate()?;
    let want = reachable(before, after, true);
    let splices = range_splice(before, &want, from, to).unwrap_or_else(|| splices(before, &want));
    Ok(Change::new(level_steps(before, &want, None, splices)))
}

fn diff_level(
    before: &Content,
    after: &Content,
    carry: Option<&HashSet<String>>,
    top: bool,
) -> Vec<Step> {
    let want = reachable(before, after, top);
    let splices = splices(before, &want);
    level_steps(before, &want, carry, splices)
}

/// The steps that take `before` to `want` after `splices`, which take its
/// text to `want`'s: each container, then the splices, then each record,
/// island and slot that still differs, then the marks and anchors.
fn level_steps(
    before: &Content,
    want: &Content,
    carry: Option<&HashSet<String>>,
    splices: Vec<Step>,
) -> Vec<Step> {
    let held = containers_by_id(before);
    let mut steps = Vec::new();
    for k in &want.containers {
        if held.get(k.id.as_str()) != Some(&k) {
            steps.push(Step::Container(ContainerStep::Set(k.clone())));
        }
    }
    steps.extend(splices.into_iter().map(|mut s| {
        if let Step::Splice(sp) = &mut s {
            sp.removed = None;
        }
        s
    }));
    let mut cur = before.clone();
    let ran = run(&mut cur, &steps);
    debug_assert!(
        ran.is_ok(),
        "a diff's splices apply to the value they were made from"
    );
    let mut rest = Vec::new();
    rest.extend(
        cur.blocks
            .iter()
            .zip(&want.blocks)
            .filter_map(|(have, need)| record_step(have, need)),
    );
    for (have, need) in cur.islands.iter().zip(&want.islands) {
        for (slot, inner) in &need.slots {
            if let Some(held) = have.slots.get(slot)
                && held != inner
            {
                let change = Change::new(diff_level(held, inner, carry, false));
                if !change.is_empty() {
                    rest.push(Step::Nested {
                        island: have.id.clone(),
                        slot: slot.clone(),
                        change,
                    });
                }
            }
        }
        if have.props != need.props {
            let slots = need
                .slots
                .iter()
                .filter(|(k, _)| !have.slots.contains_key(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            rest.push(Step::Island(IslandStep::Props {
                id: have.id.clone(),
                props: need.props.clone(),
                slots,
            }));
        }
    }
    let ran = run(&mut cur, &rest);
    debug_assert!(
        ran.is_ok(),
        "a diff's record and island steps apply after its splices"
    );
    steps.extend(rest);
    steps.extend(fixups_carrying(&cur, want, carry));
    steps
}

/// The `setBlock` that takes `have`'s record to `need`'s, naming only the
/// fields that differ.
fn record_step(have: &Block, need: &Block) -> Option<Step> {
    let (kind, path) = (have.kind != need.kind, have.path != need.path);
    (kind || path).then(|| {
        Step::SetBlock(SetBlock {
            id: have.id.clone(),
            kind: kind.then(|| need.kind.tag().to_string()),
            attrs: crate::wire::block_attrs(&need.kind)
                .and_then(|v| v.as_object().cloned())
                .filter(|_| kind),
            path: path.then(|| need.path.clone()),
        })
    })
}

/// `after` under the ids a change from `before` can give it.
fn reachable(before: &Content, after: &Content, top: bool) -> Content {
    let mut want = after.clone();
    let first = before
        .blocks
        .first()
        .map(|b| b.id.clone())
        .unwrap_or_default();
    if want.blocks[1..].iter().any(|b| b.id == first) {
        let mut ids = Minter::default();
        for b in before.blocks.iter().chain(&want.blocks) {
            ids.hold(&b.id);
        }
        for b in &mut want.blocks[1..] {
            if b.id == first {
                b.id = ids.mint(prefix::BLOCK);
            }
        }
    }
    want.blocks[0].id = first;
    if top {
        let held = containers_by_id(before);
        let mut renamed = HashMap::new();
        for k in &mut want.containers {
            if held
                .get(k.id.as_str())
                .is_some_and(|held| !held.kind.same_kind(&k.kind))
            {
                let fresh = fresh_id(&k.id, before, after);
                renamed.insert(k.id.clone(), fresh.clone());
                k.id = fresh;
            }
        }
        if !renamed.is_empty() {
            for b in &mut want.blocks {
                for id in &mut b.path {
                    if let Some(to) = renamed.get(id) {
                        *id = to.clone();
                    }
                }
            }
        }
    }
    want
}

/// The block ids of `c`.
fn block_ids(c: &Content) -> HashSet<&str> {
    c.blocks.iter().map(|b| b.id.as_str()).collect()
}

/// Each island of `c` by its id.
fn islands_by_id(c: &Content) -> HashMap<&str, &Island> {
    c.islands.iter().map(|i| (i.id.as_str(), i)).collect()
}

/// Each container of `c` by its id.
fn containers_by_id(c: &Content) -> HashMap<&str, &Container> {
    c.containers.iter().map(|k| (k.id.as_str(), k)).collect()
}

fn fresh_id(id: &str, before: &Content, after: &Content) -> String {
    let taken: HashSet<&str> = before
        .containers
        .iter()
        .chain(&after.containers)
        .map(|k| k.id.as_str())
        .collect();
    let base: String = id.chars().take(crate::ids::MAX_ID_LEN - 12).collect();
    (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|c| !taken.contains(c.as_str()))
        .expect("a free id")
}

/// The splices of a word-aligned diff of the two texts, a kept `\n` or slot
/// whose record differs deleted and inserted again.
/// The two texts and the record each `\n` and slot carries: a block's index
/// for a `\n`, an island's for a slot.
struct Texts<'a> {
    before: &'a Content,
    want: &'a Content,
    a: Vec<char>,
    b: Vec<char>,
    ra: Vec<Option<usize>>,
    rb: Vec<Option<usize>>,
}

impl<'a> Texts<'a> {
    fn new(before: &'a Content, want: &'a Content) -> Self {
        let a: Vec<char> = before.text.chars().collect();
        let b: Vec<char> = want.text.chars().collect();
        let (ra, rb) = (records(&a), records(&b));
        Texts {
            before,
            want,
            a,
            b,
            ra,
            rb,
        }
    }

    /// Whether `a[i]`, kept as `b[j]`, keeps its record: a `\n` its block's
    /// id, a slot its island's id, type and, for an `x-` island, slots.
    fn same(&self, i: usize, j: usize) -> bool {
        match (self.a[i], self.ra[i], self.rb[j]) {
            ('\n', Some(x), Some(y)) => self.before.blocks[x].id == self.want.blocks[y].id,
            (ISLAND_SLOT, Some(x), Some(y)) => {
                let (p, q) = (&self.before.islands[x], &self.want.islands[y]);
                let slots_follow_props = !matches!(p.island_type, IslandType::Ext(_));
                p.id == q.id
                    && p.island_type == q.island_type
                    && (slots_follow_props || p.slots.keys().eq(q.slots.keys()))
            }
            _ => true,
        }
    }
}

fn records(chars: &[char]) -> Vec<Option<usize>> {
    let (mut nl, mut sl) = (0, 0);
    chars
        .iter()
        .map(|&ch| match ch {
            '\n' => {
                nl += 1;
                Some(nl)
            }
            ISLAND_SLOT => {
                sl += 1;
                Some(sl - 1)
            }
            _ => None,
        })
        .collect()
}

/// The furthest a range splice slides past its range to meet the records on
/// either side, as Enter at a block's start, which the text alone places on
/// either side of the `\n` before it, needs.
const MAX_SLIDE: usize = 64;

/// The one splice of `before` to `want` over `[from, to)` of `before`, its
/// edges moved in past what the two keep and, where the records outside it
/// differ, slid until they agree; `None` where no place within
/// [`MAX_SLIDE`] makes them.
fn range_splice(before: &Content, want: &Content, from: Scalar, to: Scalar) -> Option<Vec<Step>> {
    let t = Texts::new(before, want);
    let (a, b) = (&t.a, &t.b);
    let to_b = (to + b.len()).checked_sub(a.len())?;
    if from > to || to > a.len() || from > to_b || a[..from] != b[..from] || a[to..] != b[to_b..] {
        return None;
    }
    let (mut i0, mut i1, mut j1) = (from, to, to_b);
    while i0 < i1 && i0 < j1 && a[i0] == b[i0] && t.same(i0, i0) {
        i0 += 1;
    }
    while i1 > i0 && j1 > i0 && a[i1 - 1] == b[j1 - 1] && t.same(i1 - 1, j1 - 1) {
        i1 -= 1;
        j1 -= 1;
    }
    let kept = |i0: usize, i1: usize, j1: usize| {
        let lead = (0..i0).all(|k| t.same(k, k));
        lead && (i1..a.len()).all(|k| t.same(k, k + j1 - i1))
    };
    let mut at = Some((i0, i1, j1)).filter(|&(i0, i1, j1)| kept(i0, i1, j1));
    let (mut l0, mut l1, mut lj) = (i0, i1, j1);
    for _ in 0..MAX_SLIDE {
        if at.is_some() || l0 == 0 {
            break;
        }
        let la = if l1 > l0 { a[l1 - 1] } else { a[l0 - 1] };
        let lb = if lj > l0 { b[lj - 1] } else { b[l0 - 1] };
        if la != lb {
            break;
        }
        (l0, l1, lj) = (l0 - 1, l1 - 1, lj - 1);
        at = Some((l0, l1, lj)).filter(|&(i0, i1, j1)| kept(i0, i1, j1));
    }
    let (mut r0, mut r1, mut rj) = (i0, i1, j1);
    for _ in 0..MAX_SLIDE {
        if at.is_some() || r1 >= a.len() || rj >= b.len() {
            break;
        }
        let fa = if r1 > r0 { a[r0] } else { a[r1] };
        let fb = if rj > r0 { b[r0] } else { b[rj] };
        if fa != fb {
            break;
        }
        (r0, r1, rj) = (r0 + 1, r1 + 1, rj + 1);
        at = Some((r0, r1, rj)).filter(|&(i0, i1, j1)| kept(i0, i1, j1));
    }
    let (i0, i1, j1) = at?;
    if i0 == i1 && i0 == j1 {
        return Some(Vec::new());
    }
    let marks = MarksOver::new(&want.marks);
    Some(vec![Step::Splice(splice_of(
        want,
        &marks,
        &a[i0..i1],
        &b[i0..j1],
        i0,
        &t.rb[i0..j1],
        i0,
    ))])
}

fn splices(before: &Content, want: &Content) -> Vec<Step> {
    let t = Texts::new(before, want);
    let (a, b, ra, rb) = (&t.a, &t.b, &t.ra, &t.rb);
    let same = |i: usize, j: usize| t.same(i, j);
    let marks = MarksOver::new(&want.marks);
    let mut steps = Vec::new();
    let (mut i, mut j, mut pos) = (0usize, 0usize, 0usize);
    let (mut i0, mut j0) = (0usize, 0usize);
    let flush = |i: usize,
                 j: usize,
                 i0: &mut usize,
                 j0: &mut usize,
                 pos: &mut usize,
                 steps: &mut Vec<Step>| {
        if i > *i0 || j > *j0 {
            let mut s = splice_of(want, &marks, &a[*i0..i], &b[*j0..j], *j0, &rb[*j0..j], *pos);
            s.removed = Some(crate::change::Removed {
                blocks: (*i0..i)
                    .filter(|&k| a[k] == '\n')
                    .filter_map(|k| ra[k])
                    .map(|x| before.blocks[x].clone())
                    .collect(),
                islands: (*i0..i)
                    .filter(|&k| a[k] == ISLAND_SLOT)
                    .filter_map(|k| ra[k])
                    .map(|x| before.islands[x].clone())
                    .collect(),
                marks: Vec::new(),
            });
            steps.push(Step::Splice(s));
            *pos += j - *j0;
        }
        *i0 = i;
        *j0 = j;
    };
    for op in text_ops(&before.text, &want.text) {
        match op {
            Op::Retain(n) => {
                for _ in 0..n {
                    if same(i, j) {
                        flush(i, j, &mut i0, &mut j0, &mut pos, &mut steps);
                        i += 1;
                        j += 1;
                        i0 = i;
                        j0 = j;
                        pos += 1;
                    } else {
                        i += 1;
                        j += 1;
                    }
                }
            }
            Op::Delete(n) => i += n,
            Op::Insert(s) => j += s.chars().count(),
        }
    }
    flush(i, j, &mut i0, &mut j0, &mut pos, &mut steps);
    steps
}

/// A value's marks by start, to find those over a range.
struct MarksOver<'a> {
    marks: Vec<&'a Mark>,
    longest: usize,
}

impl<'a> MarksOver<'a> {
    fn new(marks: &'a [Mark]) -> Self {
        let mut marks: Vec<&Mark> = marks.iter().collect();
        marks.sort_by_key(|m| m.start);
        let longest = marks
            .iter()
            .map(|m| m.end.saturating_sub(m.start))
            .max()
            .unwrap_or(0);
        MarksOver { marks, longest }
    }

    /// The marks over `[from, to)`, as offsets into it.
    fn over(&self, from: usize, to: usize) -> Vec<Mark> {
        let lo = self
            .marks
            .partition_point(|m| m.start + self.longest <= from);
        let hi = self.marks.partition_point(|m| m.start < to);
        self.marks[lo..hi.max(lo)]
            .iter()
            .filter_map(|m| {
                let (s, e) = (m.start.max(from), m.end.min(to));
                (s < e).then(|| Mark::new(s - from, e - from, m.kind.clone()))
            })
            .collect()
    }
}

fn splice_of(
    want: &Content,
    marks: &MarksOver,
    del: &[char],
    ins: &[char],
    from: usize,
    records: &[Option<usize>],
    at: usize,
) -> Splice {
    let mut blocks = Vec::new();
    let mut islands = Vec::new();
    for (&ch, r) in ins.iter().zip(records) {
        match (ch, r) {
            ('\n', Some(k)) => blocks.push(want.blocks[*k].clone()),
            (ISLAND_SLOT, Some(k)) => islands.push(want.islands[*k].clone()),
            _ => {}
        }
    }
    Splice {
        marks: marks.over(from, from + ins.len()),
        blocks,
        islands,
        ..Splice::new(
            at,
            del.iter().collect::<String>(),
            ins.iter().collect::<String>(),
        )
    }
}

/// The word-aligned text diff, a last block compared whole with its newline as
/// every other block is, and each inserted or deleted block bringing the `\n`
/// that opens it.
fn text_ops(a: &str, b: &str) -> Vec<Op> {
    if !a.contains('\n') && !b.contains('\n') {
        return text_diff(a, b).ops;
    }
    let sentinel = format!("{b}\n");
    let ops = without_sentinel(text_diff(&format!("{a}\n"), &sentinel).ops, &sentinel);
    opening(ops, &a.chars().collect::<Vec<_>>())
}

/// `ops` with each run of edits that follows a kept `\n` and ends in a `\n` on
/// both sides moved one character back: the same texts, with the kept `\n`
/// now the one after the run, so it still opens the block it opened. A run at
/// the start that ends in a `\n` on both sides keeps that `\n`, since the
/// first block's record rides none.
fn opening(ops: Vec<Op>, a: &[char]) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    let (mut i, mut k) = (0usize, 0usize);
    while k < ops.len() {
        if let Op::Retain(n) = ops[k] {
            push(&mut out, Op::Retain(n));
            i += n;
            k += 1;
            continue;
        }
        let end = (k..ops.len())
            .find(|&e| matches!(ops[e], Op::Retain(_)))
            .unwrap_or(ops.len());
        let run = &ops[k..end];
        let dn: usize = run
            .iter()
            .map(|op| if let Op::Delete(n) = op { *n } else { 0 })
            .sum();
        let ins: String = run
            .iter()
            .filter_map(|op| {
                if let Op::Insert(s) = op {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect();
        let slides = matches!(out.last(), Some(Op::Retain(_)))
            && a[i - 1] == '\n'
            && (dn == 0 || a[i + dn - 1] == '\n')
            && (ins.is_empty() || ins.ends_with('\n'));
        let first = out.is_empty() && dn > 0 && a[dn - 1] == '\n' && ins.ends_with('\n');
        if first {
            if dn > 1 {
                push(&mut out, Op::Delete(dn - 1));
            }
            let kept = &ins[..ins.len() - 1];
            if !kept.is_empty() {
                push(&mut out, Op::Insert(kept.to_string()));
            }
            push(&mut out, Op::Retain(1));
        } else if !slides {
            out.extend(run.iter().cloned());
        } else {
            if let Some(Op::Retain(r)) = out.last_mut() {
                *r -= 1;
                if *r == 0 {
                    out.pop();
                }
            }
            let mut moved = std::iter::once('\n')
                .chain(ins.chars())
                .take(ins.chars().count());
            for op in run {
                match op {
                    Op::Insert(s) => push(
                        &mut out,
                        Op::Insert(moved.by_ref().take(s.chars().count()).collect()),
                    ),
                    op => push(&mut out, op.clone()),
                }
            }
            push(&mut out, Op::Retain(1));
        }
        i += dn;
        k = end;
    }
    out
}

fn push(out: &mut Vec<Op>, op: Op) {
    match (out.last_mut(), op) {
        (Some(Op::Retain(a)), Op::Retain(b)) => *a += b,
        (Some(Op::Delete(a)), Op::Delete(b)) => *a += b,
        (Some(Op::Insert(a)), Op::Insert(b)) => a.push_str(&b),
        (_, op) => out.push(op),
    }
}

/// `after` under the ids of what it kept from `before`: the first block, each
/// block whose opening `\n` the text diff keeps, each island whose slot it
/// keeps with its type or that an island it drops further back held under the
/// same id and type, a kept table's columns, rows and cells by position, and
/// each container a kept block's path names at the same depth with the same
/// kind. Every other id is kept where `before` does not hold it, and minted
/// otherwise. What markdown cannot spell rides over from `before`: a kept
/// block's `x-` kind where it reads back as a paragraph, and an `x-` mark over
/// the first to the last of its characters the diff keeps.
pub fn align(before: &Content, after: Content) -> Content {
    let mut after = after;
    let a: Vec<char> = before.text.chars().collect();
    let b: Vec<char> = after.text.chars().collect();
    let mut blocks: Vec<(usize, usize)> = vec![(0, 0)];
    let mut islands: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut nl_a, mut nl_b, mut sl_a, mut sl_b) = (0usize, 0usize, 0usize, 0usize);
    let (mut dropped_at, mut added_at): (HashMap<usize, usize>, HashMap<usize, usize>) =
        (HashMap::new(), HashMap::new());
    let mut event = 0usize;
    let mut kept: Vec<Option<usize>> = vec![None; a.len()];
    let mut step = |ia: Option<usize>,
                    jb: Option<usize>,
                    blocks: &mut Vec<(usize, usize)>,
                    islands: &mut Vec<(usize, usize)>| {
        let ca = ia.map(|k| a[k]);
        let cb = jb.map(|k| b[k]);
        event += 1;
        if ca == Some('\n') && cb == Some('\n') {
            blocks.push((nl_a + 1, nl_b + 1));
        }
        if ca == Some(ISLAND_SLOT) && cb == Some(ISLAND_SLOT) {
            islands.push((sl_a, sl_b));
        } else if ca == Some(ISLAND_SLOT) {
            dropped_at.insert(sl_a, event);
        } else if cb == Some(ISLAND_SLOT) {
            added_at.insert(sl_b, event);
        }
        if ca == Some('\n') {
            nl_a += 1;
        }
        if cb == Some('\n') {
            nl_b += 1;
        }
        if ca == Some(ISLAND_SLOT) {
            sl_a += 1;
        }
        if cb == Some(ISLAND_SLOT) {
            sl_b += 1;
        }
    };
    for op in text_ops(&before.text, &after.text) {
        match op {
            Op::Retain(n) => {
                for _ in 0..n {
                    step(Some(i), Some(j), &mut blocks, &mut islands);
                    kept[i] = Some(j);
                    i += 1;
                    j += 1;
                }
            }
            Op::Delete(n) => {
                for _ in 0..n {
                    step(Some(i), None, &mut blocks, &mut islands);
                    i += 1;
                }
            }
            Op::Insert(s) => {
                for _ in s.chars() {
                    step(None, Some(j), &mut blocks, &mut islands);
                    j += 1;
                }
            }
        }
    }

    let mut block_to: Vec<Option<usize>> = vec![None; after.blocks.len()];
    let mut containers: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    let kinds_before: HashMap<&str, &ContainerKind> = before
        .containers
        .iter()
        .map(|k| (k.id.as_str(), &k.kind))
        .collect();
    let kinds_after: HashMap<String, ContainerKind> = after
        .containers
        .iter()
        .map(|k| (k.id.clone(), k.kind.clone()))
        .collect();
    for &(x, y) in &blocks {
        block_to[y] = Some(x);
        if matches!(before.blocks[x].kind, BlockKind::Ext { .. })
            && after.blocks[y].kind == BlockKind::Para
        {
            after.blocks[y].kind = before.blocks[x].kind.clone();
        }
        let (pa, pb) = (&before.blocks[x].path, &after.blocks[y].path);
        for (ka, kb) in pa.iter().zip(pb) {
            let same_kind = match (kinds_before.get(ka.as_str()), kinds_after.get(kb)) {
                (Some(p), Some(q)) => p.same_kind(q),
                _ => false,
            };
            if !same_kind {
                break;
            }
            match containers.get(kb) {
                Some(to) if to == ka => continue,
                Some(_) => break,
                None if taken.contains(ka) => break,
                None => {
                    containers.insert(kb.clone(), ka.clone());
                    taken.insert(ka.clone());
                }
            }
        }
    }
    let mut minter: Option<Minter> = None;
    let build = |containers: &HashMap<String, String>| {
        let mut ids = Minter::default();
        for id in before
            .blocks
            .iter()
            .map(|b| &b.id)
            .chain(before.containers.iter().map(|k| &k.id))
            .chain(containers.values())
        {
            ids.hold(id);
        }
        ids
    };
    let mut fresh: Vec<(String, String)> = Vec::new();
    for k in &after.containers {
        if !containers.contains_key(&k.id) {
            let kept = if !kinds_before.contains_key(k.id.as_str()) && !taken.contains(&k.id) {
                minter.get_or_insert_with(|| build(&containers)).hold(&k.id);
                k.id.clone()
            } else {
                minter
                    .get_or_insert_with(|| build(&containers))
                    .mint(container_prefix(k))
            };
            taken.insert(kept.clone());
            fresh.push((k.id.clone(), kept));
        }
    }
    containers.extend(fresh);
    let held: HashSet<&str> = before.blocks.iter().map(|b| b.id.as_str()).collect();
    for (y, b) in after.blocks.iter_mut().enumerate() {
        b.id = match block_to[y] {
            Some(x) => before.blocks[x].id.clone(),
            None if !held.contains(b.id.as_str())
                && !minter
                    .get_or_insert_with(|| build(&containers))
                    .holds(&b.id) =>
            {
                minter.get_or_insert_with(|| build(&containers)).hold(&b.id);
                b.id.clone()
            }
            None => minter
                .get_or_insert_with(|| build(&containers))
                .mint(prefix::BLOCK),
        };
        for id in &mut b.path {
            if let Some(to) = containers.get(id) {
                *id = to.clone();
            }
        }
    }
    for k in &mut after.containers {
        if let Some(to) = containers.get(&k.id) {
            k.id = to.clone();
        }
    }

    let mut island_ids = Minter::default();
    for i in &before.islands {
        island_ids.hold(&i.id);
    }
    let matched: HashMap<usize, usize> = islands
        .into_iter()
        .filter(|&(x, y)| before.islands[x].island_type == after.islands[y].island_type)
        .map(|(x, y)| (y, x))
        .collect();
    let mut unclaimed: HashMap<&str, (usize, &Island)> = before
        .islands
        .iter()
        .enumerate()
        .filter_map(|(x, i)| Some((i.id.as_str(), (*dropped_at.get(&x)?, i))))
        .collect();
    for (y, island) in after.islands.iter_mut().enumerate() {
        match matched.get(&y) {
            Some(&x) => align_island(&before.islands[x], island),
            None => {
                let moved = unclaimed
                    .get(island.id.as_str())
                    .copied()
                    .filter(|(at, held)| {
                        held.island_type == island.island_type
                            && added_at.get(&y).is_some_and(|added| at < added)
                    });
                if let Some((_, held)) = moved {
                    unclaimed.remove(held.id.as_str());
                    align_island(held, island);
                } else if island_ids.holds(&island.id) {
                    island.id = island_ids.mint(prefix::ISLAND);
                } else {
                    island_ids.hold(&island.id);
                }
            }
        }
    }
    for m in before
        .marks
        .iter()
        .filter(|m| matches!(m.kind, MarkKind::Ext { .. }))
    {
        let mut covered = kept[m.start..m.end].iter().flatten();
        if let Some(&lo) = covered.next() {
            let hi = covered.last().copied().unwrap_or(lo);
            after.marks.push(Mark::new(lo, hi + 1, m.kind.clone()));
        }
    }
    after.normalize();
    after
}

fn container_prefix(k: &crate::model::Container) -> &'static str {
    match k.kind {
        crate::model::ContainerKind::List { .. } => prefix::LIST,
        crate::model::ContainerKind::Item { .. } => prefix::ITEM,
        crate::model::ContainerKind::Quote => prefix::QUOTE,
    }
}

/// `island` under `held`'s id, a table's grid under `held`'s ids by
/// position, and each slot's content aligned to the slot it now names.
fn align_island(held: &Island, island: &mut Island) {
    island.id = held.id.clone();
    if let (Some(old), Some(new)) = (Table::read(&held.props), Table::read(&island.props)) {
        let mut ids = Minter::default();
        for id in old
            .columns
            .iter()
            .chain(old.rows.iter().map(|r| &r.id))
            .chain(old.slot_order().iter())
        {
            ids.hold(id);
        }
        let by_position =
            |a: &[String], b: &[String], ids: &mut Minter, p: &str| -> HashMap<String, String> {
                b.iter()
                    .enumerate()
                    .map(|(n, id)| {
                        let to = a.get(n).cloned().unwrap_or_else(|| ids.mint(p));
                        (id.clone(), to)
                    })
                    .collect()
            };
        let columns = by_position(&old.columns, &new.columns, &mut ids, prefix::COLUMN);
        let rows = by_position(
            &old.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            &new.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            &mut ids,
            prefix::ROW,
        );
        let mut slots = HashMap::new();
        for (r, row) in new.rows.iter().enumerate() {
            for (c, slot) in row.cells.iter().enumerate() {
                let to = old
                    .slot(r, c)
                    .map(str::to_string)
                    .unwrap_or_else(|| ids.mint(prefix::SLOT));
                slots.insert(slot.clone(), to);
            }
        }
        table::rename(island, &columns, &rows, &slots);
    }
    for (id, slot) in island.slots.iter_mut() {
        if let Some(old) = held.slots.get(id) {
            *slot = align(old, std::mem::take(slot));
        }
    }
}

/// A markdown rewrite of `before`, as v1's revise reads it: anchors kept by
/// the tags the markdown kept, otherwise by the word-aligned diff.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    pub change: Change,
    pub content: Content,
    pub warnings: Vec<ImportWarning>,
}

/// Why a revise answered no change.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviseError {
    Import(ImportError),
    /// The markdown imports to a value its change cannot land on `before`,
    /// one that would not validate.
    Refused(ApplyError),
}

impl std::fmt::Display for ReviseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviseError::Import(e) => write!(f, "{e}"),
            ReviseError::Refused(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReviseError {}

impl From<ImportError> for ReviseError {
    fn from(e: ImportError) -> Self {
        ReviseError::Import(e)
    }
}

/// A markdown rewrite of `before`: the anchors and islands whose `qm-*` tags
/// the markdown keeps stay put, and the rest keeps what a word-aligned diff
/// keeps. What `before` holds and markdown does not spell is gone from the
/// result. A [`Codec::Plain`] value reads `markdown` as its literal text.
pub fn revise(before: &Content, markdown: &str) -> Result<Revision, ReviseError> {
    revise_holding(before, markdown, before)
}

/// `markdown` imported and aligned to `before`, so what `before` holds and
/// the markdown does not spell is gone from it, for a write that lands on
/// `now` rather than on `before`: each block, container and island id the
/// revision introduces that `now` holds is minted afresh, past the ids of
/// `before`, `now` and the revision. The ids are a function of the three
/// values alone, so equal values write equal bytes.
pub fn revise_holding(
    before: &Content,
    markdown: &str,
    now: &Content,
) -> Result<Revision, ReviseError> {
    revise_lines(before, &to_lines(before), markdown, now)
}

/// [`revise_holding`] with `before` already lowered to `lines`.
fn revise_lines(
    before: &Content,
    lines: &crate::lines::model::Lines,
    markdown: &str,
    now: &Content,
) -> Result<Revision, ReviseError> {
    let Against {
        content: imported,
        warnings,
        pins,
    } = match before.codec {
        Codec::Rich => from_markdown_against(markdown, lines)?,
        Codec::Plain => Against {
            content: to_lines(&crate::markdown::from_plaintext(markdown)),
            warnings: Vec::new(),
            pins: Vec::new(),
        },
    };
    let (imported, _) = carry_pinned(lines, imported, &pins);
    let mut content = holding(before, &align(before, from_lines(&imported)), now, true);
    content.codec = before.codec;
    // A pinned anchor stands where its tag does; every other rides the diff's
    // splices, as a replace's do.
    let pinned: HashSet<String> = pins.into_iter().map(|p| p.id).collect();
    content.anchors.retain(|a| pinned.contains(&a.id));
    if content == *before {
        return Ok(Revision {
            change: Change::default(),
            content,
            warnings,
        });
    }
    let change = diff_with(before, &content, Some(&pinned));
    let content = apply(before, &change)
        .map_err(ReviseError::Refused)?
        .content;
    Ok(relocating(before, change, content, warnings))
}

/// The revision `change` makes of `before`, landing on `after`, with a
/// placement for each anchor of `before`'s text that `after` holds nowhere
/// and whose text `after` holds again in what `change` inserted: a
/// paragraph moved whole keeps its anchor. A quote shorter than
/// [`MIN_MOVE`](crate::lines::delta::MIN_MOVE) characters is too weak to
/// follow, and drops.
fn relocating(
    before: &Content,
    change: Change,
    after: Content,
    warnings: Vec<ImportWarning>,
) -> Revision {
    let held: HashSet<&str> = after.anchors.iter().map(|a| a.id.as_str()).collect();
    let inserted = crate::change::inserted_spans(&change, &after.text, crate::units::Scalars);
    let places: Vec<Step> = before
        .anchors
        .iter()
        .filter(|a| !held.contains(a.id.as_str()))
        .filter_map(|a| {
            let (start, end) = relocate(&before.text, &after.text, &inserted, a.start, a.end)?;
            Some(Step::Anchor(AnchorStep::Place {
                anchor: crate::model::Anchor {
                    id: a.id.clone(),
                    start,
                    end,
                },
                quote: None,
            }))
        })
        .collect();
    if places.is_empty() {
        return Revision {
            change,
            content: after,
            warnings,
        };
    }
    let mut steps = change.steps.clone();
    steps.extend(places);
    let relocated = Change::new(steps);
    match apply(before, &relocated) {
        Ok(done) => Revision {
            change: relocated,
            content: done.content,
            warnings,
        },
        Err(_) => Revision {
            change,
            content: after,
            warnings,
        },
    }
}

/// The change that writes `content` over `before`: each anchor of `before`
/// that `content` names nowhere carried where the diff keeps its text.
/// Refuses a `content` that is not valid or not canonical
/// ([`Content::check_canonical`]), as the decode a write reads it through does.
pub fn replace(before: &Content, content: &Content) -> Result<Change, Invariant> {
    replace_holding(before, content, before)
}

/// [`replace`] for a write that lands on `now` rather than on `before`, its
/// ids minted as [`revise_holding`] mints them.
pub fn replace_holding(
    before: &Content,
    content: &Content,
    now: &Content,
) -> Result<Change, Invariant> {
    content.validate()?;
    content.check_canonical()?;
    let named: HashSet<String> = content.anchor_ids().into_iter().collect();
    Ok(diff_with(
        before,
        &holding(before, content, now, true),
        Some(&named),
    ))
}

/// `want` under the ids a change from `before` can give it, each block,
/// container and island id it holds and `before` does not that `now` holds
/// minted past the ids of all of them, and so in each slot of an island all
/// three hold.
fn holding(before: &Content, want: &Content, now: &Content, top: bool) -> Content {
    let mut want = reachable(before, want, top);
    let held_ids = |cs: [&Content; 3]| {
        let mut ids = Minter::default();
        for c in cs {
            c.blocks
                .iter()
                .map(|b| &b.id)
                .chain(c.containers.iter().map(|k| &k.id))
                .for_each(|id| ids.hold(id));
        }
        ids
    };
    let mut ids: Option<Minter> = None;
    let containers =
        |c: &Content| -> HashSet<String> { c.containers.iter().map(|k| k.id.clone()).collect() };
    let mut renamed = HashMap::new();
    let (was, held) = (containers(before), containers(now));
    for k in 0..want.containers.len() {
        let id = &want.containers[k].id;
        if !was.contains(id) && held.contains(id) {
            let fresh = ids
                .get_or_insert_with(|| held_ids([before, now, &want]))
                .mint(container_prefix(&want.containers[k]));
            renamed.insert(
                std::mem::replace(&mut want.containers[k].id, fresh.clone()),
                fresh,
            );
        }
    }
    let (was, held) = (block_ids(before), block_ids(now));
    let clash: Vec<usize> = (0..want.blocks.len())
        .filter(|&k| {
            !was.contains(want.blocks[k].id.as_str()) && held.contains(want.blocks[k].id.as_str())
        })
        .collect();
    for k in clash {
        let fresh = ids
            .get_or_insert_with(|| held_ids([before, now, &want]))
            .mint(prefix::BLOCK);
        want.blocks[k].id = fresh;
    }
    for b in &mut want.blocks {
        for id in &mut b.path {
            if let Some(to) = renamed.get(id) {
                *id = to.clone();
            }
        }
    }
    let (was, held) = (islands_by_id(before), islands_by_id(now));
    let mut island_ids: Option<Minter> = None;
    for k in 0..want.islands.len() {
        match (
            was.get(want.islands[k].id.as_str()),
            held.get(want.islands[k].id.as_str()),
        ) {
            (Some(was), Some(held)) => {
                for (slot, inner) in &mut want.islands[k].slots {
                    if let (Some(w), Some(n)) = (was.slots.get(slot), held.slots.get(slot)) {
                        *inner = holding(w, inner, n, false);
                    }
                }
            }
            (None, Some(_)) => {
                let ids = island_ids.get_or_insert_with(|| {
                    let mut ids = Minter::default();
                    [before, now, &want]
                        .iter()
                        .flat_map(|c| &c.islands)
                        .for_each(|i| ids.hold(&i.id));
                    ids
                });
                want.islands[k].id = ids.mint(prefix::ISLAND);
            }
            _ => {}
        }
    }
    want
}

/// A line-form value written over `before`, under the ids of what it kept and
/// its codec.
#[doc(hidden)]
pub fn lift(before: &Content, after: &crate::lines::model::Content) -> Content {
    let mut out = align(before, from_lines(after));
    out.codec = before.codec;
    out
}
