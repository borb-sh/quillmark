//! The change of each editing gesture a host lowers, so every editor binding
//! splits and joins blocks the same way: the text a gesture keeps keeps its
//! block's id.

use crate::change::{ApplyError, Change, Splice, Step};
use crate::model::{Block, Content, Usv};

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
