//! Two changes made from one value, merged as Automerge merges them, and the
//! markdown revise that merges what its read could not spell back in.

use crate::model::{Doc, StepError, View, ids};
use automerge::ActorId;
use quillmark_content::change::{ApplyError, Change, apply};
use quillmark_content::diff::{self, Keeping, Revision, ReviseError};
use quillmark_content::model::Content;
use std::collections::HashMap;

/// The actors `merge3` writes under, `ours` after `theirs` where both put
/// text at one place, so the merge is a function of its inputs.
const OURS: &[u8] = b"ours";
const THEIRS: &[u8] = b"theirs";

/// The key the merged value sits under in the document's main card.
const VALUE: &str = "value";

/// `ours` and `theirs`, both made from `base`, merged as a write at earlier
/// heads merges with the writes since: `base` held in a document, `theirs`
/// lowered at its heads and `ours` at the heads `base` was held at, and the
/// merged state read through repair, under the ids `base` holds and the ids
/// each change put.
pub fn merge3(base: &Content, ours: &Change, theirs: &Change) -> Result<View, ApplyError> {
    let (mine, since) = (apply(base, ours)?.content, apply(base, theirs)?.content);
    let mut doc = Doc::new();
    let main = doc.main();
    let root = doc.put_value(&main, VALUE, base);
    doc.commit();
    let held = doc.view(&root).content;
    let before = doc.heads();
    doc.set_actor(ActorId::from(THEIRS));
    doc.apply_from(&root, &ids::renamed(theirs, base, &held), None, Some(&held)).map_err(refused)?;
    let mut back: HashMap<String, String> = ids::places(&doc.view(&root).content, &since);
    let after_theirs = doc.heads();
    doc.set_actor(ActorId::from(OURS));
    doc.apply_from(&root, &ids::renamed(ours, base, &held), Some(&before), Some(&held)).map_err(refused)?;
    if let Some(at_ours) = doc.heads().into_iter().find(|h| !after_theirs.contains(h)).map(|h| vec![h]) {
        back.extend(ids::places(&doc.view_at(&root, &at_ours).content, &mine));
    }
    let View { content, repairs } = doc.view(&root);
    Ok(View { content: ids::renamed_content(&content, &back), repairs })
}

/// A step the document could not lower, as the error `apply` gives for it.
fn refused(e: StepError) -> ApplyError {
    match e {
        StepError::Stale { step, .. } | StepError::Bounds { step, .. } => ApplyError::Stale { step },
        StepError::Unknown { what: "block", id, .. } => ApplyError::UnknownBlock { id },
        StepError::Unknown { what: "container", id, .. } => ApplyError::UnknownContainer { id },
        StepError::Unknown { what: "anchor", id, .. } => ApplyError::UnknownAnchor { id },
        StepError::Unknown { what: "slot", id, .. } => match id.split_once('/') {
            Some((island, slot)) => ApplyError::UnknownSlot { island: island.to_string(), slot: slot.to_string() },
            None => ApplyError::UnknownSlot { island: id, slot: String::new() },
        },
        StepError::Unknown { id, .. } => ApplyError::UnknownIsland { id },
        StepError::AnchorHeld { id, .. } => ApplyError::AnchorHeld { id },
        StepError::Bad { step, reason } => ApplyError::BadStep { step, reason },
    }
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
