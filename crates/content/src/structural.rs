//! The change of each structural edit a host lowers, so every editor binding
//! splits, joins, deletes, nests and wraps blocks and grows marks the same way:
//! the text an edit keeps keeps its block's id, a record an edit opens
//! takes a [placeholder](crate::ids::is_placeholder) the write that lands it
//! fills, `?0` first and on in the order the change puts them, and text typed
//! at a mark's edge takes it as [`inherited_marks`]' growth defaults say.

use crate::change::{ApplyError, Change, ContainerStep, SetBlock, Splice, Step};
use crate::ids::PLACEHOLDER;
use crate::model::{Block, Container, ContainerKind, Content, HARD_BREAK, MarkKind, Scalar};

/// One structural edit, as a write takes it: [`change`](Self::change) builds its
/// change from the value its writer read.
#[derive(Debug, Clone, PartialEq)]
pub enum StructuralEdit {
    /// [`split_block`]: the new block `id`, or a placeholder where absent.
    SplitBlock { at: Scalar, id: Option<String> },
    /// [`join_blocks`].
    JoinBlocks { id: String },
    /// [`delete_range`].
    DeleteRange { start: Scalar, end: Scalar },
    /// [`indent`].
    Indent { first: String, last: String },
    /// [`outdent`].
    Outdent { first: String, last: String },
    /// [`wrap`].
    Wrap {
        first: String,
        last: String,
        with: Wrap,
    },
}

impl StructuralEdit {
    /// The edit's change on `content`.
    pub fn change(&self, content: &Content) -> Result<Change, ApplyError> {
        match self {
            StructuralEdit::SplitBlock { at, id } => split_block(content, *at, id.as_deref()),
            StructuralEdit::JoinBlocks { id } => join_blocks(content, id),
            StructuralEdit::DeleteRange { start, end } => delete_range(content, *start, *end),
            StructuralEdit::Indent { first, last } => indent(content, first, last),
            StructuralEdit::Outdent { first, last } => outdent(content, first, last),
            StructuralEdit::Wrap { first, last, with } => wrap(content, first, last, with),
        }
    }

    /// The edit with its offsets, counted in `u` over `content`'s text,
    /// as scalar offsets; `Stale` where one falls inside a character or past
    /// the text.
    pub fn to_scalars(
        &self,
        content: &Content,
        u: impl crate::units::Units,
    ) -> Result<StructuralEdit, ApplyError> {
        let at = |n: Scalar| {
            crate::units::to_scalars(u, &content.text, n).ok_or(ApplyError::Stale { step: 0 })
        };
        Ok(match self {
            StructuralEdit::SplitBlock { at: n, id } => StructuralEdit::SplitBlock {
                at: at(*n)?,
                id: id.clone(),
            },
            StructuralEdit::DeleteRange { start, end } => StructuralEdit::DeleteRange {
                start: at(*start)?,
                end: at(*end)?,
            },
            g => g.clone(),
        })
    }
}

/// The `n`th placeholder a structural edit puts.
fn placeholder(n: usize) -> String {
    format!("{PLACEHOLDER}{n}")
}

/// Enter at `at`: the change that splits the block holding `at` there. The
/// new block, `new_id` or a placeholder where absent, takes the split block's
/// kind and path, and the text keeps its id. At the start of a block holding text, other than the first, the new
/// block is the empty one before it, its `\n` inserted ahead of the one that
/// opens the block; anywhere else it is what follows `at`. A split at the start
/// of the first block moves its text to `new_id`, since no `\n` opens it.
/// `Stale` where `at` is past the text.
pub fn split_block(
    content: &Content,
    at: Scalar,
    new_id: Option<&str>,
) -> Result<Change, ApplyError> {
    let ranges = content.block_ranges();
    let k = ranges
        .iter()
        .position(|&(s, e)| s <= at && at <= e)
        .ok_or(ApplyError::Stale { step: 0 })?;
    let (start, end) = ranges[k];
    let at = if k > 0 && at == start && start < end {
        start - 1
    } else {
        at
    };
    let id = new_id.map_or_else(|| placeholder(0), str::to_string);
    let record = Block {
        id,
        ..content.blocks[k].clone()
    };
    Ok(Change::new(vec![Step::Splice(Splice {
        blocks: vec![record],
        ..Splice::new(at, "", "\n")
    })]))
}

/// Backspace at the start of block `id`: the change that deletes the `\n`
/// opening it, so its text joins the block before it under that block's
/// record. `UnknownBlock` where no block holds `id`, and `BadStep` for the
/// first block, which no block precedes. `apply` refuses the change where the
/// joined block would be invalid, as text joined into a rule is.
pub fn join_blocks(content: &Content, id: &str) -> Result<Change, ApplyError> {
    let k = content
        .blocks
        .iter()
        .position(|b| b.id == id)
        .ok_or_else(|| ApplyError::UnknownBlock { id: id.into() })?;
    if k == 0 {
        return Err(ApplyError::BadStep {
            step: 0,
            reason: "the first block has no block before it to join",
        });
    }
    let at = content.block_ranges()[k].0 - 1;
    Ok(Change::new(vec![Step::Splice(Splice::new(at, "\n", ""))]))
}

/// Typing at `at`: the formatting the typed text carries, for the splice's
/// `marks`. `strong`, `emph`, `underline` and `strike` grow at their end, so
/// the text takes one the character before `at` holds. `code`, `link` and an
/// `x-` mark grow at neither edge, so it takes one only where the characters
/// on both sides hold it. `font_family` grows at both, so it takes the family
/// before `at`, or else the one after. A `\n` or a hard break holds no mark,
/// so at a block's or a line's start only a family carries over. In canonical
/// order; `Stale` where `at` is past the text.
pub fn inherited_marks(content: &Content, at: Scalar) -> Result<Vec<MarkKind>, ApplyError> {
    if at > content.len_scalars() {
        return Err(ApplyError::Stale { step: 0 });
    }
    let mut around = content.text.chars().skip(at.saturating_sub(1));
    let before = if at > 0 { around.next() } else { None };
    let after = around.next();
    let held = |i: Scalar, c: Option<char>| -> Vec<&MarkKind> {
        match c {
            Some(c) if c != '\n' && c != HARD_BREAK => content
                .marks
                .iter()
                .filter(|m| m.start <= i && i < m.end)
                .map(|m| &m.kind)
                .collect(),
            _ => Vec::new(),
        }
    };
    let before = at.checked_sub(1).map_or_else(Vec::new, |i| held(i, before));
    let after = held(at, after);
    let mut out: Vec<MarkKind> = Vec::new();
    for kind in &before {
        let grows = match kind {
            MarkKind::Strong
            | MarkKind::Emph
            | MarkKind::Underline
            | MarkKind::Strike
            | MarkKind::FontFamily { .. } => true,
            MarkKind::Code | MarkKind::Link { .. } | MarkKind::Ext { .. } => after.contains(kind),
        };
        if grows && !out.contains(kind) {
            out.push((*kind).clone());
        }
    }
    let family = |k: &&MarkKind| matches!(k, MarkKind::FontFamily { .. });
    if !out.iter().any(|k| family(&k))
        && let Some(after) = after.iter().copied().find(family)
    {
        out.push(after.clone());
    }
    out.sort_by_key(MarkKind::sort_key);
    Ok(out)
}

/// Deleting `[start, end)`: the splice, and where the range runs from the
/// start of a block into a later one, a `setBlock` giving the block that keeps
/// the text the kind, attributes and path of the block the range ends in, so
/// the block whose text survives keeps its shape, as ProseMirror's delete
/// does. The joined block keeps the first block's id. `Stale` where the range
/// is past the text or reversed.
pub fn delete_range(content: &Content, start: Scalar, end: Scalar) -> Result<Change, ApplyError> {
    if start > end || end > content.len_scalars() {
        return Err(ApplyError::Stale { step: 0 });
    }
    if start == end {
        return Ok(Change::default());
    }
    let deleted: String = content.text.chars().skip(start).take(end - start).collect();
    let mut steps = vec![Step::Splice(Splice::new(start, deleted, ""))];
    let ranges = content.block_ranges();
    let (k, m) = (content.block_at(start), content.block_at(end));
    if k != m && ranges[k].0 == start {
        let (first, last) = (&content.blocks[k], &content.blocks[m]);
        if first.kind != last.kind || first.path != last.path {
            steps.push(Step::SetBlock(SetBlock {
                id: first.id.clone(),
                kind: Some(last.kind.tag().to_string()),
                attrs: crate::wire::block_attrs(&last.kind).and_then(|v| v.as_object().cloned()),
                path: Some(last.path.clone()),
            }));
        }
    }
    Ok(Change::new(steps))
}

/// Tab over the blocks `first` to `last`: the change that nests each item of
/// the innermost list holding `first`, from the item holding `first` to the
/// one holding `last`, under the item before them, with every block those
/// items hold. They join that item's list where it ends in one, and else open
/// a list of their own list's kind. `BadStep` where `first` is in no item, the
/// range leaves its list or its first item is the list's first.
pub fn indent(content: &Content, first: &str, last: &str) -> Result<Change, ApplyError> {
    let (k0, k1) = block_span(content, first, last)?;
    let path = &content.blocks[k0].path;
    let j = innermost(content, path, |k| matches!(k, ContainerKind::Item { .. })).ok_or(
        ApplyError::BadStep {
            step: 0,
            reason: "the block is in no list item",
        },
    )?;
    let base = &path[..j];
    let (start, end) = sibling_run(content, base, k0, k1)?;
    let before = start
        .checked_sub(1)
        .map(|b| &content.blocks[b].path)
        .filter(|p| p.len() > j && p.starts_with(base))
        .ok_or(ApplyError::BadStep {
            step: 0,
            reason: "the first item has no item before it to nest under",
        })?;
    let mut steps = Vec::new();
    let nested = before
        .get(j + 1)
        .filter(|id| matches!(kind_of(content, id), Some(ContainerKind::List { .. })))
        .cloned();
    let list = match nested {
        Some(id) => id,
        None => {
            let id = placeholder(0);
            let kind = match kind_of(content, &path[j - 1]) {
                Some(ContainerKind::List { ordered, .. }) => ContainerKind::List {
                    ordered: *ordered,
                    start: 1,
                },
                _ => ContainerKind::List {
                    ordered: false,
                    start: 1,
                },
            };
            steps.push(Step::Container(ContainerStep::Set(Container::new(
                id.clone(),
                kind,
            ))));
            id
        }
    };
    let head: Vec<String> = base.iter().chain([&before[j], &list]).cloned().collect();
    for b in &content.blocks[start..=end] {
        steps.push(set_path(
            b,
            head.iter().chain(&b.path[j..]).cloned().collect(),
        ));
    }
    Ok(Change::new(steps))
}

/// Shift-Tab over the blocks `first` to `last`: the change that lifts them
/// out of the innermost item or quote holding `first`. In an item, each item
/// from the one holding `first` to the one holding `last` leaves its list: a
/// nested one becomes a sibling of the item it was nested in, taking the items
/// after it and the rest of that item as its own, and a top-level one leaves
/// the list, its blocks taking the list's place. In a quote, each child of it
/// the range touches leaves it. What follows the lifted run in its list or
/// quote stays in it, or opens one of its kind where the run split it.
/// `BadStep` where `first` is in no item or quote, or the range leaves it.
pub fn outdent(content: &Content, first: &str, last: &str) -> Result<Change, ApplyError> {
    let (k0, k1) = block_span(content, first, last)?;
    let path = &content.blocks[k0].path;
    let j = innermost(content, path, |k| !matches!(k, ContainerKind::List { .. })).ok_or(
        ApplyError::BadStep {
            step: 0,
            reason: "the block is in no list item or quote",
        },
    )?;
    let mut steps = Vec::new();
    let quoted = matches!(kind_of(content, &path[j]), Some(ContainerKind::Quote));
    let (base, depth) = if quoted {
        (&path[..=j], j + 1)
    } else {
        (&path[..j], j)
    };
    let (start, end) = if quoted {
        child_run(content, base, k0, k1)?
    } else {
        sibling_run(content, base, k0, k1)?
    };
    let held = |b: usize| content.blocks[b].path.starts_with(base);
    let mut tail = end + 1;
    while tail < content.blocks.len() && held(tail) {
        tail += 1;
    }
    let split = start > 0 && held(start - 1) && tail > end + 1;
    let rest = if split {
        let id = placeholder(0);
        let kind = match kind_of(content, &base[depth - 1]) {
            Some(ContainerKind::List { ordered, .. }) => ContainerKind::List {
                ordered: *ordered,
                start: 1,
            },
            Some(k) => k.clone(),
            None => ContainerKind::Quote,
        };
        steps.push(Step::Container(ContainerStep::Set(Container::new(
            id.clone(),
            kind,
        ))));
        Some(id)
    } else {
        None
    };
    let parent_item = !quoted
        && j >= 2
        && matches!(
            kind_of(content, &path[j - 2]),
            Some(ContainerKind::Item { .. })
        );
    if parent_item {
        let outer = &path[..j - 2];
        let parent = &path[j - 2];
        let last_item = content.blocks[end].path[j].clone();
        for b in &content.blocks[start..=end] {
            steps.push(set_path(
                b,
                outer.iter().chain(&b.path[j..]).cloned().collect(),
            ));
        }
        let list = rest.unwrap_or_else(|| path[j - 1].clone());
        for b in &content.blocks[end + 1..tail] {
            let to = outer
                .iter()
                .chain([&last_item, &list])
                .chain(&b.path[j..])
                .cloned()
                .collect();
            steps.push(set_path(b, to));
        }
        let mut after = tail;
        while after < content.blocks.len() && content.blocks[after].path.starts_with(&path[..j - 1])
        {
            let b = &content.blocks[after];
            if b.path[j - 2] != *parent {
                break;
            }
            steps.push(set_path(
                b,
                outer
                    .iter()
                    .chain([&last_item])
                    .chain(&b.path[j - 1..])
                    .cloned()
                    .collect(),
            ));
            after += 1;
        }
    } else {
        let outer = &base[..depth - 1];
        let skip = if quoted { depth } else { depth + 1 };
        for b in &content.blocks[start..=end] {
            steps.push(set_path(
                b,
                outer.iter().chain(&b.path[skip..]).cloned().collect(),
            ));
        }
        if let Some(id) = rest {
            for b in &content.blocks[end + 1..tail] {
                steps.push(set_path(
                    b,
                    outer
                        .iter()
                        .chain([&id])
                        .chain(&b.path[depth..])
                        .cloned()
                        .collect(),
                ));
            }
        }
    }
    Ok(Change::new(steps))
}

/// What [`wrap`] opens around the blocks it wraps.
#[derive(Debug, Clone, PartialEq)]
pub enum Wrap {
    Quote,
    /// A list with an item per child it wraps, each `checked` as given.
    List {
        ordered: bool,
        checked: Option<bool>,
    },
}

/// Wrapping the blocks `first` to `last` in a quote or a list: the change that
/// opens one around the children of their innermost shared item or quote
/// the range touches, or around the blocks at the top. A list the range sits
/// in is wrapped whole. Each wrapped child keeps what it holds. `BadStep`
/// where `last` comes before `first`.
pub fn wrap(content: &Content, first: &str, last: &str, with: &Wrap) -> Result<Change, ApplyError> {
    let (k0, k1) = block_span(content, first, last)?;
    let mut shared: &[String] = &content.blocks[k0].path;
    for b in &content.blocks[k0..=k1] {
        let n = shared
            .iter()
            .zip(&b.path)
            .take_while(|(x, y)| x == y)
            .count();
        shared = &shared[..n];
    }
    if let Some(id) = shared.last()
        && matches!(kind_of(content, id), Some(ContainerKind::List { .. }))
    {
        shared = &shared[..shared.len() - 1];
    }
    let (start, end) = child_run(content, shared, k0, k1)?;
    let d = shared.len();
    let mut steps = Vec::new();
    let outer = placeholder(0);
    let mut minted = 1;
    let kind = match with {
        Wrap::Quote => ContainerKind::Quote,
        Wrap::List { ordered, .. } => ContainerKind::List {
            ordered: *ordered,
            start: 1,
        },
    };
    steps.push(Step::Container(ContainerStep::Set(Container::new(
        outer.clone(),
        kind,
    ))));
    let mut item: Option<(Option<&String>, String)> = None;
    let mut sets = Vec::new();
    for (i, b) in content.blocks[start..=end].iter().enumerate() {
        let mut to: Vec<String> = shared.to_vec();
        to.push(outer.clone());
        if let Wrap::List { checked, .. } = with {
            let child = b.path.get(d);
            let id = match &item {
                Some((held, id)) if child.is_some() && *held == child && i > 0 => id.clone(),
                _ => {
                    let id = placeholder(minted);
                    minted += 1;
                    steps.push(Step::Container(ContainerStep::Set(Container::new(
                        id.clone(),
                        ContainerKind::Item { checked: *checked },
                    ))));
                    item = Some((child, id.clone()));
                    id
                }
            };
            to.push(id);
        }
        to.extend(b.path[d..].iter().cloned());
        sets.push(set_path(b, to));
    }
    steps.extend(sets);
    Ok(Change::new(steps))
}

fn block_span(content: &Content, first: &str, last: &str) -> Result<(usize, usize), ApplyError> {
    let at = |id: &str| {
        content
            .blocks
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| ApplyError::UnknownBlock { id: id.into() })
    };
    let (k0, k1) = (at(first)?, at(last)?);
    if k1 < k0 {
        return Err(ApplyError::BadStep {
            step: 0,
            reason: "the last block comes before the first",
        });
    }
    Ok((k0, k1))
}

fn kind_of<'a>(content: &'a Content, id: &str) -> Option<&'a ContainerKind> {
    content.container(id).map(|c| &c.kind)
}

/// The index in `path` of its innermost container `is` admits.
fn innermost(
    content: &Content,
    path: &[String],
    is: impl Fn(&ContainerKind) -> bool,
) -> Option<usize> {
    path.iter()
        .rposition(|id| kind_of(content, id).is_some_and(&is))
}

/// The blocks of the children of `base` from the one holding block `k0` to
/// the one holding `k1`, where every child is an item: `base` ends in a list.
fn sibling_run(
    content: &Content,
    base: &[String],
    k0: usize,
    k1: usize,
) -> Result<(usize, usize), ApplyError> {
    if content.blocks[k1].path.len() <= base.len() || !content.blocks[k1].path.starts_with(base) {
        return Err(ApplyError::BadStep {
            step: 0,
            reason: "the range leaves the list",
        });
    }
    child_run(content, base, k0, k1)
}

/// The blocks of the children of `base` from the one holding block `k0` to
/// the one holding `k1`; a block whose path is `base` is a child of its own.
fn child_run(
    content: &Content,
    base: &[String],
    k0: usize,
    k1: usize,
) -> Result<(usize, usize), ApplyError> {
    let d = base.len();
    let paths: Vec<&Vec<String>> = content.blocks.iter().map(|b| &b.path).collect();
    if !paths[k1].starts_with(base) {
        return Err(ApplyError::BadStep {
            step: 0,
            reason: "the range leaves its container",
        });
    }
    let child = |k: usize| paths[k].get(d);
    let same = |k: usize, of: usize| {
        paths[k].starts_with(base) && child(k).is_some() && child(k) == child(of)
    };
    let mut start = k0;
    while start > 0 && child(k0).is_some() && same(start - 1, k0) {
        start -= 1;
    }
    let mut end = k1;
    while end + 1 < paths.len() && child(k1).is_some() && same(end + 1, k1) {
        end += 1;
    }
    Ok((start, end))
}

fn set_path(b: &Block, path: Vec<String>) -> Step {
    Step::SetBlock(SetBlock {
        id: b.id.clone(),
        kind: None,
        attrs: None,
        path: Some(path),
    })
}
