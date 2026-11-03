//! Two changes made from one value, merged as Automerge merges them, and the
//! markdown revise that merges what its read could not spell back in.

use crate::{Held, Read};
use quillmark_content::change::{ApplyError, Change, apply, apply_traced};
use quillmark_content::diff::{self, Keeping, Revision, ReviseError};
use quillmark_content::model::Content;

/// `ours` and `theirs`, both made from `base`, merged: `ours` written on
/// `base` and `theirs` on a fork of it at `base`, the anchors and marks
/// `ours` put held to their edges ([`Held::clip`]), and the merged state
/// read through repair.
pub fn merge3(base: &Content, ours: &Change, theirs: &Change) -> Result<Read, ApplyError> {
    let (mine, mine_trace) = apply_traced(base, ours)?;
    let (since, since_trace) = apply_traced(base, theirs)?;
    let mut held = Held::new(base, "");
    let heads = held.heads();
    held.write(&mine_trace, &mine.content);
    let mine_heads = held.heads();
    let mut fork = held.fork_at(&heads, None).expect("a document forks at its own heads");
    fork.write(&since_trace, &since.content);
    let theirs_heads = fork.heads();
    held.merge(&mut fork).expect("a fork merges back into its document");
    held.clip(&mine_heads, &theirs_heads, &mine_trace);
    Ok(held.read())
}

/// [`revise_keeping`] of a write that lands on `before`.
pub fn revise(before: &Content, markdown: &str) -> Result<Revision, ReviseError> {
    revise_keeping(before, markdown, before)
}

/// A markdown rewrite of `before` that keeps what its annotated read does not
/// spell ([`diff::revise_keeping`]): where the read loses nothing, the
/// revision itself; otherwise the writer's revision of the read merged with
/// the change from the read to `before`, so what the read lost rides back in
/// wherever the writer's edit leaves room for it. The change answered takes
/// `before` to the merge.
pub fn revise_keeping(before: &Content, markdown: &str, now: &Content) -> Result<Revision, ReviseError> {
    let (read, since, mine) = match diff::revise_keeping(before, markdown, now)? {
        Keeping::Revised(r) => return Ok(r),
        Keeping::Merge { read, since, mine } => (read, since, mine),
    };
    if mine.change.is_empty() {
        return Ok(Revision { change: Change::default(), content: before.clone(), warnings: mine.warnings });
    }
    let merged = merge3(&read, &mine.change, &since).map_err(ReviseError::Refused)?.content;
    let change = diff::diff(before, &merged).map_err(|e| ReviseError::Refused(e.into()))?;
    let content = apply(before, &change).map_err(ReviseError::Refused)?.content;
    Ok(Revision { change, content, warnings: mine.warnings })
}
