//! The content write, landed in the document's Automerge document.
//!
//! A write builds its change against the value its writer read, at the
//! heads it read at or now, and lowers it as one Automerge change:
//! at earlier heads it is concurrent with every change since and merges with
//! them as Automerge merges, refusing nothing a concurrent change did. The
//! card then takes the value the write left, which Automerge's patches since
//! the write began build from the value before it.

use std::collections::BTreeMap;

use quillmark_automerge::automerge::ObjId;
use quillmark_automerge::model::{StepError, cards};
use quillmark_automerge::content::change::{self, AnchorStep, ApplyError, Change, Step};
use quillmark_automerge::content::lines::import::ImportWarning;
use quillmark_automerge::content::diff;
use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::units::{self, Units};

use super::edit::EditError;
use super::write::{Edit, Receipt, Target, WriteOptions, check_change, keeps_ext_islands};
use super::{Card, Document, Heads};
use crate::value::QuillValue;

fn step_error(e: StepError) -> EditError {
    EditError::change(match e {
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
    })
}

/// `v` with each string `minted` renames renamed, the values under a key
/// `keep` names kept as they are.
fn renamed_json(v: &serde_json::Value, minted: &BTreeMap<String, String>, keep: &[&str]) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => Value::String(minted.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Array(a) => Value::Array(a.iter().map(|x| renamed_json(x, minted, keep)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| match keep.contains(&k.as_str()) {
                    true => (k.clone(), x.clone()),
                    false => (k.clone(), renamed_json(x, minted, keep)),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `change` with each id `minted` renames renamed.
fn renamed(change: &Change, minted: &BTreeMap<String, String>) -> Change {
    if minted.is_empty() {
        return change.clone();
    }
    let keep = ["text", "insert", "delete", "quote", "type", "op", "kind"];
    Change::from_value(&renamed_json(&change.to_value(), minted, &keep)).unwrap_or_else(|_| change.clone())
}

fn removed_by(change: &Change, out: &mut Vec<String>) {
    for step in &change.steps {
        match step {
            Step::Anchor(AnchorStep::Remove { id }) => out.push(id.clone()),
            Step::Nested { change, .. } => removed_by(change, out),
            _ => {}
        }
    }
}

impl Document {
    pub(crate) fn card_at(&self, at: Option<usize>) -> &Card {
        match at {
            None => &self.main,
            Some(i) => &self.cards[i],
        }
    }

    pub(crate) fn card_at_mut(&mut self, at: Option<usize>) -> &mut Card {
        self.touch();
        match at {
            None => &mut self.main,
            Some(i) => &mut self.cards[i],
        }
    }

    /// The value at `target` of the card at `at`, a field made a text value
    /// where it holds its content as JSON or holds nothing; `true` with it
    /// where the field was absent and is put now.
    fn value_obj(&mut self, at: Option<usize>, target: Target<'_>) -> Result<(ObjId, bool), EditError> {
        let mut put = false;
        let mut content = None;
        if let Target::Field(name) = target {
            let read = content.insert(self.card_at(at).read(target)?).to_value();
            if self.card_at(at).payload().get(name).is_none() {
                self.card_at_mut(at).payload_mut().insert(name.to_string(), QuillValue::from_json(read)).map_err(EditError::InvalidPayload)?;
                put = true;
            }
        }
        let id = self.card_at(at).id().map(str::to_string);
        if let (Some(i), None) = (at, &id) {
            return Err(EditError::IndexOutOfRange { index: i, len: 0 });
        }
        let field = match target {
            Target::Field(name) => Some(name),
            _ => None,
        };
        let (v, written) = cards::value_at(self.doc(), at.and(id.as_deref()), field, || content.expect("a field target reads its value"))
            .expect("a flushed card is held");
        if written {
            self.reread();
        }
        Ok((v, put))
    }

    /// The heads a write at `heads` builds at: none for the value now, and
    /// refused with `edit::stale_base` where the document's history does not
    /// hold them.
    fn heads_for(&mut self, target: Target<'_>, heads: Option<&Heads>) -> Result<Option<Heads>, EditError> {
        let Some(heads) = heads else { return Ok(None) };
        let doc = self.doc();
        if doc.heads() == heads.hashes() {
            return Ok(None);
        }
        match doc.holds(heads.hashes()) {
            true => Ok(Some(heads.clone())),
            false => Err(EditError::StaleHeads { field: target.field().map(str::to_string) }),
        }
    }

    /// Takes `after`, what the value at `target` of the card at `at` reads as
    /// once a write landed, into the cards: a body in place, a field by
    /// reading the cards again.
    fn set_read(&mut self, at: Option<usize>, target: Target<'_>, after: &Content) {
        let Target::Body = target else { return self.reread() };
        match at {
            None => self.main.set_body(after.clone()),
            Some(i) => self.cards[i].set_body(after.clone()),
        }
        #[cfg(debug_assertions)]
        {
            let (main, cards) = (self.main.clone(), self.cards.clone());
            self.reread();
            debug_assert!(self.main == main && self.cards == cards, "a body write reads as the document does");
        }
    }

    /// The value at `target` of the card at `at` as its writer read it at
    /// `heads`, refused with `edit::stale_base` where the document's history
    /// does not hold them.
    pub(crate) fn read_under(&mut self, at: Option<usize>, target: Target<'_>, heads: Option<&Heads>) -> Result<Content, EditError> {
        if heads.is_none() {
            return self.card_at(at).read(target);
        }
        let (root, _) = self.value_obj(at, target)?;
        let heads = self.heads_for(target, heads)?;
        let doc = self.doc();
        Ok(match heads {
            Some(h) => doc.view_at(&root, h.hashes()).content,
            None => doc.view(&root).content,
        })
    }

    /// Lands `edit` on the value at `target` of the card at `at`, the main
    /// card where `None`, built against the value its heads name.
    pub(crate) fn write_at<U: Units>(&mut self, at: Option<usize>, target: Target<'_>, edit: Edit<'_>, opts: WriteOptions<'_>, u: Option<U>) -> Result<Receipt, EditError> {
        if let Edit::Apply(change) = &edit {
            check_change(change)?;
        }
        let (root, put) = self.value_obj(at, target)?;
        let landed = self.write_on(at, target, &root, edit, opts, u);
        if let (Err(_), true, Target::Field(name)) = (&landed, put, target) {
            self.card_at_mut(at).remove_field(name)?;
            self.flush();
        }
        landed
    }

    fn write_on<U: Units>(&mut self, at: Option<usize>, target: Target<'_>, root: &ObjId, edit: Edit<'_>, opts: WriteOptions<'_>, u: Option<U>) -> Result<Receipt, EditError> {
        let root = root.clone();
        let heads = self.heads_for(target, opts.heads)?;
        let now = self.card_at(at).read(target)?;
        debug_assert_eq!(now, self.held().doc.view(&root).content);
        let read = match &heads {
            Some(h) => self.held().doc.view_at(&root, h.hashes()).content,
            None => now.clone(),
        };
        let (change, warnings, diffed) = match edit {
            Edit::Apply(change) => match u {
                Some(u) => {
                    let filled = quillmark_automerge::content::mint::fill(&change, &[&read], None).map_err(EditError::change)?;
                    let scalars = units::change_to_scalars(&filled.change, &read, u).map_err(EditError::change)?;
                    let back = filled.minted.into_iter().map(|(placeholder, id)| (id, placeholder)).collect();
                    (renamed(&scalars, &back), Vec::new(), false)
                }
                None => (change, Vec::new(), false),
            },
            Edit::Structural(edit) => {
                let edit = match u {
                    Some(u) => edit.to_scalars(&read, u).map_err(EditError::change)?,
                    None => edit,
                };
                (edit.change(&read).map_err(EditError::change)?, Vec::new(), false)
            }
            Edit::Revise(markdown) => {
                let revision = quillmark_automerge::revise_keeping(&read, markdown, &now).map_err(|e| match e {
                    diff::ReviseError::Import(e) => EditError::import(e),
                    diff::ReviseError::Refused(e) => EditError::change(e),
                })?;
                keeps_ext_islands(&read, &revision.content)?;
                (revision.change, revision.warnings, true)
            }
            Edit::Replace(content) => {
                let change = diff::replace_holding(&read, &content, &now).map_err(|e| EditError::change(e.into()))?;
                keeps_ext_islands(&read, &content)?;
                (change, Vec::new(), true)
            }
        };
        let doc = &mut self.held().doc;
        if heads.is_none() {
            doc.track();
        }
        let before = Heads::from(doc.heads());
        let lowered = doc.apply_from(&root, &change, heads.as_ref().map(Heads::hashes), Some(&read));
        lowered.map_err(step_error)?;
        let (patches, view) = doc.patches_on(&root, before.hashes(), &now);
        let landed = Heads::from(doc.heads());
        let mut removed = Vec::new();
        removed_by(&change, &mut removed);
        let mut dropped = change::lost_anchors(&now, &view.content, |id| diffed || !removed.iter().any(|r| r == id));
        if heads.is_some() {
            let mine: Vec<_> = landed.hashes().iter().filter(|h| !before.hashes().contains(h)).copied().collect();
            let wrote = doc.view_at(&root, &mine).content;
            let held: std::collections::HashSet<String> = now.anchor_ids().into_iter().collect();
            dropped.extend(change::lost_anchors(&wrote, &view.content, |id| !held.contains(id)));
        }
        self.set_read(at, target, &view.content);
        let receipt = Receipt::landed(before, landed, patches, &now, dropped, view.repairs, warnings);
        Ok(match u {
            Some(u) => receipt.in_units(u),
            None => receipt,
        })
    }

    /// Runs `write`, which changes the value at `target` of the card at `at`
    /// on the card, lands it in the document, and answers its receipt.
    pub(crate) fn landing(&mut self, at: Option<usize>, target: Target<'_>, write: impl FnOnce(&mut Card) -> Result<Vec<ImportWarning>, EditError>) -> Result<Receipt, EditError> {
        let (root, put) = self.value_obj(at, target)?;
        self.doc().track();
        let before = Heads::from(self.doc().heads());
        let was = self.card_at(at).read(target)?;
        let warnings = match write(self.card_at_mut(at)) {
            Ok(w) => w,
            Err(e) => {
                if let (true, Target::Field(name)) = (put, target) {
                    self.card_at_mut(at).remove_field(name)?;
                }
                self.flush();
                return Err(e);
            }
        };
        self.flush();
        let doc = &mut self.held().doc;
        let (patches, view) = doc.patches_on(&root, before.hashes(), &was);
        let landed = Heads::from(doc.heads());
        let dropped = change::lost_anchors(&was, &view.content, |_| true);
        Ok(Receipt::landed(before, landed, patches, &was, dropped, view.repairs, warnings))
    }
}
