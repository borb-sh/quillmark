//! The change of each editing gesture a host lowers, so every editor binding
//! splits and joins blocks and grows marks the same way: the text a gesture
//! keeps keeps its block's id, and text typed at a mark's edge takes it as the
//! canon's growth defaults say.

use crate::change::{ApplyError, Change, Splice, Step};
use crate::model::{Block, Content, HARD_BREAK, MarkKind, Usv};

/// Enter at `at`: the change that splits the block holding `at` there. The
/// new block `new_id` takes the split block's kind and path, and the text keeps
/// its id. At the start of a block holding text, other than the first, the new
/// block is the empty one before it, its `\n` inserted ahead of the one that
/// opens the block; anywhere else it is what follows `at`. A split at the start
/// of the first block moves its text to `new_id`, since no `\n` opens it.
/// `Stale` where `at` is past the text.
pub fn split_block(content: &Content, at: Usv, new_id: &str) -> Result<Change, ApplyError> {
    let ranges = content.block_ranges();
    let k = ranges.iter().position(|&(s, e)| s <= at && at <= e).ok_or(ApplyError::Stale { step: 0 })?;
    let (start, end) = ranges[k];
    let at = if k > 0 && at == start && start < end { start - 1 } else { at };
    let record = Block { id: new_id.to_string(), ..content.blocks[k].clone() };
    Ok(Change::new(vec![Step::Splice(Splice { blocks: vec![record], ..Splice::new(at, "", "\n") })]))
}

/// Backspace at the start of block `id`: the change that deletes the `\n`
/// opening it, so its text joins the block before it under that block's
/// record. `UnknownBlock` where no block holds `id`, and `BadStep` for the
/// first block, which no block precedes. `apply` refuses the change where the
/// joined block would be invalid, as text joined into a rule is.
pub fn join_blocks(content: &Content, id: &str) -> Result<Change, ApplyError> {
    let k = content.blocks.iter().position(|b| b.id == id).ok_or_else(|| ApplyError::UnknownBlock { id: id.into() })?;
    if k == 0 {
        return Err(ApplyError::BadStep { step: 0, reason: "the first block has no block before it to join" });
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
pub fn inherited_marks(content: &Content, at: Usv) -> Result<Vec<MarkKind>, ApplyError> {
    if at > content.len_usv() {
        return Err(ApplyError::Stale { step: 0 });
    }
    let mut around = content.text.chars().skip(at.saturating_sub(1));
    let before = if at > 0 { around.next() } else { None };
    let after = around.next();
    let held = |i: Usv, c: Option<char>| -> Vec<&MarkKind> {
        match c {
            Some(c) if c != '\n' && c != HARD_BREAK => {
                content.marks.iter().filter(|m| m.start <= i && i < m.end).map(|m| &m.kind).collect()
            }
            _ => Vec::new(),
        }
    };
    let before = at.checked_sub(1).map_or_else(Vec::new, |i| held(i, before));
    let after = held(at, after);
    let mut out: Vec<MarkKind> = Vec::new();
    for kind in &before {
        let grows = match kind {
            MarkKind::Strong | MarkKind::Emph | MarkKind::Underline | MarkKind::Strike | MarkKind::FontFamily { .. } => {
                true
            }
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
