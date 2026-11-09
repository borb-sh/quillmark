//! Blocks addressed by id: the read that lists a value's blocks with their
//! ids, ranges and markdown, and the change of each block op a host or a
//! model names by id. A block is one record of [`Content::blocks`]: a
//! paragraph, heading, code block, rule, a list item's paragraph, or the
//! paragraph a table's island sits in. Its id is the stored record's, so it
//! holds across every apply, split and join the value goes through.

use crate::change::{ApplyError, Change, ContainerStep, SetBlock, Splice, Step, apply, compose};
use crate::ids::PLACEHOLDER;
use crate::markdown::to_markdown;
use crate::mint::fill;
use crate::model::{Block, Container, ContainerKind, Content, ISLAND_SLOT, Mark, Scalar};
use std::collections::{BTreeMap, HashMap};

/// One block as [`blocks`] reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockRead {
    pub id: String,
    pub kind: String,
    /// Container ids, outermost first.
    pub path: Vec<String>,
    /// `[start, end)` in the text, its `\n` excluded.
    pub start: Scalar,
    pub end: Scalar,
    /// The block alone as markdown: a list item with its marker, indented to
    /// its depth.
    pub markdown: String,
}

/// Each block of `c`, in order.
pub fn blocks(c: &Content) -> Vec<BlockRead> {
    let ranges = c.block_ranges();
    let chars: Vec<char> = c.text.chars().collect();
    let mut slots_before = 0;
    let mut out = Vec::with_capacity(c.blocks.len());
    for (k, (b, &(s, e))) in c.blocks.iter().zip(&ranges).enumerate() {
        let slots = chars[s..e].iter().filter(|&&ch| ch == ISLAND_SLOT).count();
        out.push(BlockRead {
            id: b.id.clone(),
            kind: b.kind.tag().to_string(),
            path: b.path.clone(),
            start: s,
            end: e,
            markdown: block_markdown(c, k, s, e, &chars, slots_before, slots),
        });
        slots_before += slots;
    }
    out
}

/// The block alone as markdown, inside its innermost list only, indented two
/// spaces per list around that one.
fn block_markdown(
    c: &Content,
    k: usize,
    s: Scalar,
    e: Scalar,
    chars: &[char],
    slots_before: usize,
    slots: usize,
) -> String {
    let mut one = slice(c, k, s, e, chars, slots_before, slots);
    let is_list = |id: &String| {
        c.container(id)
            .is_some_and(|k| matches!(k.kind, ContainerKind::List { .. }))
    };
    let lists = one.blocks[0].path.iter().filter(|id| is_list(id)).count();
    if lists < 2 {
        return to_markdown(&one);
    }
    let inner = one.blocks[0].path.iter().rposition(is_list).unwrap();
    let path = one.blocks[0].path.split_off(inner);
    one.containers.retain(|k| path.contains(&k.id));
    one.blocks[0].path = path;
    let indent = "  ".repeat(lists - 1);
    to_markdown(&one)
        .lines()
        .map(|l| format!("{indent}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn slice(
    c: &Content,
    k: usize,
    s: Scalar,
    e: Scalar,
    chars: &[char],
    slots_before: usize,
    slots: usize,
) -> Content {
    let b = &c.blocks[k];
    let marks = c
        .marks
        .iter()
        .filter(|m| m.start < e && m.end > s)
        .map(|m| Mark::new(m.start.max(s) - s, m.end.min(e) - s, m.kind.clone()))
        .filter(|m| m.start < m.end)
        .collect();
    let containers = b
        .path
        .iter()
        .filter_map(|id| c.container(id).cloned())
        .collect();
    Content {
        text: chars[s..e].iter().collect(),
        blocks: vec![b.clone()],
        containers,
        marks,
        anchors: Vec::new(),
        islands: c.islands[slots_before..slots_before + slots].to_vec(),
        codec: c.codec,
    }
}

/// One edit to one block, named by its id.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockOp {
    /// The block becomes `with`. Its first block keeps the id; each block after
    /// it is new. A list item replaced by a lone list item stays the item.
    Replace { id: String, with: Content },
    /// `with` lands after the block, all of it new. After a list item it lands
    /// as the item's next sibling, after the item's nested blocks.
    InsertAfter { id: String, with: Content },
    /// The block leaves, and a container it leaves empty with it.
    Delete { id: String },
}

impl BlockOp {
    pub fn id(&self) -> &str {
        match self {
            BlockOp::Replace { id, .. }
            | BlockOp::InsertAfter { id, .. }
            | BlockOp::Delete { id } => id,
        }
    }
}

/// The change of `op` on `c`, each record it opens under a placeholder.
/// `UnknownBlock` where no block holds the id.
pub fn change(c: &Content, op: &BlockOp) -> Result<Change, ApplyError> {
    let k = c
        .blocks
        .iter()
        .position(|b| b.id == op.id())
        .ok_or_else(|| ApplyError::UnknownBlock { id: op.id().into() })?;
    let ranges = c.block_ranges();
    match op {
        BlockOp::Replace { with, .. } => replace(c, k, &ranges, with),
        BlockOp::InsertAfter { with, .. } => insert_after(c, k, &ranges, with),
        BlockOp::Delete { .. } => delete(c, k, &ranges),
    }
}

/// What [`batch`] did beyond its change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Batched {
    pub change: Change,
    /// Per op, the ids of the blocks it opened, in order.
    pub opened: Vec<Vec<String>>,
    /// Each block whose text moved under another id, and that id: deleting a
    /// value's first block leaves the block after it under the first id,
    /// since no `\n` opens the first record.
    pub renamed: BTreeMap<String, String>,
}

/// The ops in order as one change on `c`, every id minted, so the write that
/// lands it mints nothing. An op naming an id an earlier op renamed reaches it
/// under its new id; one naming an id an earlier op deleted is
/// `UnknownBlock`. `Err` carries the index of the op refused.
pub fn batch(c: &Content, ops: &[BlockOp]) -> Result<Batched, (usize, ApplyError)> {
    let mut now = c.clone();
    let mut out = Batched::default();
    for (n, op) in ops.iter().enumerate() {
        let op = match out.renamed.get(op.id()) {
            Some(to) => with_id(op, to),
            None => op.clone(),
        };
        let first_deleted = matches!(op, BlockOp::Delete { .. })
            && now.blocks.first().is_some_and(|b| b.id == op.id())
            && now.blocks.len() > 1;
        let heir = first_deleted.then(|| now.blocks[1].id.clone());
        let ch = change(&now, &op).map_err(|e| (n, e))?;
        let filled = fill(&ch, &[&now]).map_err(|e| (n, e))?;
        let applied = apply(&now, &filled.change).map_err(|e| (n, e))?;
        let before: std::collections::HashSet<&str> =
            now.blocks.iter().map(|b| b.id.as_str()).collect();
        out.opened.push(
            applied
                .content
                .blocks
                .iter()
                .filter(|b| !before.contains(b.id.as_str()))
                .map(|b| b.id.clone())
                .collect(),
        );
        if let Some(heir) = heir {
            for to in out.renamed.values_mut().filter(|to| **to == heir) {
                *to = op.id().to_string();
            }
            out.renamed.insert(heir, op.id().to_string());
        }
        out.change = compose(&out.change, &applied.applied);
        now = applied.content;
    }
    Ok(out)
}

fn with_id(op: &BlockOp, id: &str) -> BlockOp {
    let id = id.to_string();
    match op.clone() {
        BlockOp::Replace { with, .. } => BlockOp::Replace { id, with },
        BlockOp::InsertAfter { with, .. } => BlockOp::InsertAfter { id, with },
        BlockOp::Delete { .. } => BlockOp::Delete { id },
    }
}

/// `with` under placeholders, `?` and a number past `from`, its ids renamed
/// across every level, and how many it used.
fn placeheld(with: &Content, from: usize) -> (Content, usize) {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut n = from;
    let mut out = with.clone();
    rename(&mut out, &mut map, &mut n);
    (out, n)
}

fn fresh(id: &str, map: &mut HashMap<String, String>, n: &mut usize) -> String {
    if let Some(to) = map.get(id) {
        return to.clone();
    }
    let to = format!("{PLACEHOLDER}{n}");
    *n += 1;
    map.insert(id.to_string(), to.clone());
    to
}

fn rename(c: &mut Content, map: &mut HashMap<String, String>, n: &mut usize) {
    for k in &mut c.containers {
        k.id = fresh(&k.id, map, n);
    }
    for b in &mut c.blocks {
        b.id = fresh(&b.id, map, n);
        for p in &mut b.path {
            if let Some(to) = map.get(p) {
                *p = to.clone();
            }
        }
    }
    for isl in &mut c.islands {
        isl.id = fresh(&isl.id, map, n);
        for slot in isl.slots.values_mut() {
            rename(slot, &mut HashMap::new(), n);
        }
    }
}

/// Whether `with` is one list holding one item, every block in it: the shape a
/// model spells a single list item in.
fn lone_item(with: &Content) -> Option<(String, String)> {
    let first = with.blocks.first()?;
    let (list, item) = match first.path.as_slice() {
        [l, i, ..] => (l.clone(), i.clone()),
        _ => return None,
    };
    let is_list = matches!(with.container(&list)?.kind, ContainerKind::List { .. });
    let all_in = with
        .blocks
        .iter()
        .all(|b| b.path.len() >= 2 && b.path[0] == list && b.path[1] == item);
    (is_list && all_in).then_some((list, item))
}

/// Whether `with` is one list, every block in it.
fn lone_list(with: &Content) -> Option<String> {
    let list = with.blocks.first()?.path.first()?.clone();
    let is_list = matches!(with.container(&list)?.kind, ContainerKind::List { .. });
    let all_in = with.blocks.iter().all(|b| b.path.first() == Some(&list));
    (is_list && all_in).then_some(list)
}

fn in_item(c: &Content, b: &Block) -> bool {
    b.path
        .last()
        .and_then(|id| c.container(id))
        .is_some_and(|k| matches!(k.kind, ContainerKind::Item { .. }))
}

/// `with`'s blocks with `strip` path entries cut from the front and `base`
/// put before what is left, and the containers it keeps.
fn rebase(with: &Content, strip: usize, base: &[String]) -> (Vec<Block>, Vec<Container>) {
    let blocks: Vec<Block> = with
        .blocks
        .iter()
        .map(|b| Block {
            path: base.iter().chain(&b.path[strip..]).cloned().collect(),
            ..b.clone()
        })
        .collect();
    let used: std::collections::HashSet<&String> =
        blocks.iter().flat_map(|b| b.path.iter()).collect();
    let containers = with
        .containers
        .iter()
        .filter(|k| used.contains(&k.id))
        .cloned()
        .collect();
    (blocks, containers)
}

fn replace(
    c: &Content,
    k: usize,
    ranges: &[(Scalar, Scalar)],
    with: &Content,
) -> Result<Change, ApplyError> {
    let (with, _) = placeheld(with, 0);
    let target = &c.blocks[k];
    let (blocks, containers) = match (in_item(c, target), lone_item(&with)) {
        (true, Some(_)) => rebase(&with, 2, &target.path),
        _ => rebase(&with, 0, &target.path),
    };
    let (s, e) = ranges[k];
    let deleted: String = c.text.chars().skip(s).take(e - s).collect();
    let mut steps: Vec<Step> = containers
        .into_iter()
        .map(|k| Step::Container(ContainerStep::Set(k)))
        .collect();
    let first = &blocks[0];
    steps.push(Step::Splice(Splice {
        marks: with.marks.clone(),
        blocks: blocks[1..].to_vec(),
        islands: with.islands.clone(),
        ..Splice::new(s, deleted, with.text.clone())
    }));
    steps.push(set_block(&target.id, first));
    Ok(Change::new(steps))
}

fn insert_after(
    c: &Content,
    k: usize,
    ranges: &[(Scalar, Scalar)],
    with: &Content,
) -> Result<Change, ApplyError> {
    let target = &c.blocks[k];
    let mut last = k;
    let item = in_item(c, target);
    if item {
        while last + 1 < c.blocks.len() && c.blocks[last + 1].path.starts_with(&target.path) {
            last += 1;
        }
    }
    let (with, used) = placeheld(with, 0);
    let mut steps = Vec::new();
    let (blocks, containers) = if item {
        let parent = &target.path[..target.path.len() - 1];
        let checked = match c.container(target.path.last().unwrap()).map(|k| &k.kind) {
            Some(ContainerKind::Item { checked: Some(_) }) => Some(false),
            _ => None,
        };
        if let Some(list) = lone_list(&with) {
            let mut kept = rebase(&with, 1, parent);
            kept.1.retain(|k| k.id != list);
            kept
        } else {
            let fresh = format!("{PLACEHOLDER}{used}");
            steps.push(Step::Container(ContainerStep::Set(Container::new(
                fresh.clone(),
                ContainerKind::Item { checked },
            ))));
            let base: Vec<String> = parent.iter().cloned().chain([fresh]).collect();
            rebase(&with, 0, &base)
        }
    } else {
        rebase(&with, 0, &target.path)
    };
    steps.extend(
        containers
            .into_iter()
            .map(|k| Step::Container(ContainerStep::Set(k))),
    );
    let at = ranges[last].1;
    let marks = with
        .marks
        .iter()
        .map(|m| Mark::new(m.start + 1, m.end + 1, m.kind.clone()))
        .collect();
    steps.push(Step::Splice(Splice {
        marks,
        blocks,
        islands: with.islands.clone(),
        ..Splice::new(at, "", format!("\n{}", with.text))
    }));
    Ok(Change::new(steps))
}

fn delete(c: &Content, k: usize, ranges: &[(Scalar, Scalar)]) -> Result<Change, ApplyError> {
    let text = |s: Scalar, e: Scalar| -> String { c.text.chars().skip(s).take(e - s).collect() };
    let (s, e) = ranges[k];
    if k > 0 {
        return Ok(Change::new(vec![Step::Splice(Splice::new(
            s - 1,
            text(s - 1, e),
            "",
        ))]));
    }
    if c.blocks.len() == 1 {
        let empty = Block::new(c.blocks[0].id.clone(), crate::model::BlockKind::Para);
        return Ok(Change::new(vec![
            Step::Splice(Splice::new(0, text(0, e), "")),
            set_block(&c.blocks[0].id, &empty),
        ]));
    }
    let next = &c.blocks[1];
    let (s1, _) = ranges[1];
    Ok(Change::new(vec![
        Step::Splice(Splice::new(0, text(0, s1), "")),
        set_block(&c.blocks[0].id, next),
    ]))
}

fn set_block(id: &str, shape: &Block) -> Step {
    Step::SetBlock(SetBlock {
        id: id.to_string(),
        kind: Some(shape.kind.tag().to_string()),
        attrs: crate::wire::block_attrs(&shape.kind).and_then(|v| v.as_object().cloned()),
        path: Some(shape.path.clone()),
    })
}
