//! The content write, landed in the document's Automerge document.
//!
//! A write builds its change against the value its writer read, at the
//! heads it read at or now, and lowers it as one Automerge change:
//! at earlier heads it is concurrent with every change since and merges with
//! them as Automerge merges, refusing nothing a concurrent change did. The
//! card then takes the value the write left, which Automerge's patches since
//! the write began build from the value before it.

use quillmark_automerge::model::{ValueId, cards, ids};
use quillmark_automerge::content::change::{self, AnchorStep, Change, Step};
use quillmark_automerge::content::lines::import::ImportWarning;
use quillmark_automerge::content::diff;
use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::units::{self, HostUnits, Units};

use super::edit::EditError;
use super::write::{Edit, Receipt, Target, WriteOptions, check_change, keeps_ext_islands};
use super::{Card, Document, Heads};
use crate::value::QuillValue;

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
            None => &self.view.main,
            Some(i) => &self.view.cards[i],
        }
    }

    /// The value at `target` of the card at `at`, a field made a text value
    /// where it holds its content as JSON or holds nothing; `true` with it
    /// where the field was absent and is put now.
    fn value_obj(&mut self, at: Option<usize>, target: Target<'_>) -> Result<(ValueId, bool), EditError> {
        let mut put = false;
        if let Target::Field(name) = target
            && self.card_at(at).payload().get(name).is_none()
        {
            let empty = self.card_at(at).read(target)?.to_value();
            self.edit_card(at, |c| c.payload_mut().insert(name.to_string(), QuillValue::from_json(empty))).map_err(EditError::InvalidPayload)?;
            put = true;
        }
        let id = self.card_at(at).id().map(str::to_string);
        if let (Some(i), None) = (at, &id) {
            return Err(EditError::IndexOutOfRange { index: i, len: 0 });
        }
        let card = match at {
            None => &self.view.main,
            Some(i) => &self.view.cards[i],
        };
        let doc = &mut self.held.0;
        let before = Heads::from(doc.heads());
        let (v, written) = cards::value_at(doc, at.and(id.as_deref()), target.field(), || card.read(target))?
            .expect("each card is held");
        if written {
            super::held::refresh(doc, &before, &mut self.view.main, &mut self.view.cards, false, self.units.0);
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
    /// once a write landed, into the card in place.
    fn set_read(&mut self, at: Option<usize>, target: Target<'_>, after: &Content) {
        let card = self.card_raw_mut(at);
        match target {
            Target::Body => card.set_body(after.clone()),
            Target::Field(name) => {
                card.payload_mut().insert(name.to_string(), after.clone()).expect("a held field takes its value");
            }
        }
        #[cfg(debug_assertions)]
        {
            let card = self.card_at(at).clone();
            self.reread_at(at);
            debug_assert!(self.card_at(at) == &card, "a write reads as the document does");
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
    /// card where `None`, built against the value its heads name, or `read`
    /// where the caller read it already. An `Edit::Apply` or structural edit
    /// counts its offsets in the handle's units where `host`, and in scalars
    /// otherwise; the receipt's patches count theirs in the handle's units.
    pub(crate) fn write_at(&mut self, at: Option<usize>, target: Target<'_>, edit: Edit<'_>, opts: WriteOptions<'_>, host: bool, read: Option<Content>) -> Result<Receipt, EditError> {
        if let Edit::Apply(change) = &edit {
            check_change(change)?;
        }
        let units = self.units();
        let edit_units = (host && units != HostUnits::Scalars).then_some(units);
        let (root, put) = self.value_obj(at, target)?;
        let landed = self.write_on(at, target, &root, edit, opts, edit_units, units, read);
        if let (Err(_), true, Target::Field(name)) = (&landed, put, target) {
            self.edit_card(at, |c| c.remove_field(name))?;
        }
        landed
    }

    #[allow(clippy::too_many_arguments)]
    fn write_on<U: Units>(&mut self, at: Option<usize>, target: Target<'_>, root: &ValueId, edit: Edit<'_>, opts: WriteOptions<'_>, edit_units: Option<U>, units: U, read: Option<Content>) -> Result<Receipt, EditError> {
        let root = root.clone();
        let heads = self.heads_for(target, opts.heads)?;
        let now = self.card_at(at).read(target)?;
        debug_assert_eq!(now, self.doc().view(&root).content);
        let read = match (read, &heads) {
            (Some(read), _) => read,
            (None, Some(h)) => self.doc().view_at(&root, h.hashes()).content,
            (None, None) => now.clone(),
        };
        let (change, warnings, diffed) = match edit {
            Edit::Apply(change) => match edit_units {
                Some(u) => {
                    let filled = quillmark_automerge::content::mint::fill(&change, &[&read]).map_err(EditError::change)?;
                    let scalars = units::change_to_scalars(&filled.change, &read, u).map_err(EditError::change)?;
                    let back = filled.minted.into_iter().map(|(placeholder, id)| (id, placeholder)).collect();
                    (ids::renamed_by(&scalars, &back), Vec::new(), false)
                }
                None => (change, Vec::new(), false),
            },
            Edit::Structural(edit) => {
                let edit = match edit_units {
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
        let doc = self.doc();
        if heads.is_none() {
            doc.track();
        }
        let before = Heads::from(doc.heads());
        let wrote = doc.apply_from(&root, &change, heads.as_ref().map(Heads::hashes), Some(&read)).map_err(|e| EditError::change(e.into()))?;
        let (patches, view) = doc.landed(&root, before.hashes(), &now, units);
        let landed = Heads::from(doc.heads());
        let mut removed = Vec::new();
        removed_by(&change, &mut removed);
        let mut dropped = change::lost_anchors(&now, &view.content, |id| diffed || !removed.iter().any(|r| r == id));
        if heads.is_some() {
            let wrote = wrote.unwrap_or_else(|| {
                let mine: Vec<_> = landed.hashes().iter().filter(|h| !before.hashes().contains(h)).copied().collect();
                doc.view_at(&root, &mine).content
            });
            let held: std::collections::HashSet<String> = now.anchor_ids().into_iter().collect();
            dropped.extend(change::lost_anchors(&wrote, &view.content, |id| !held.contains(id)));
        }
        self.set_read(at, target, &view.content);
        Ok(Receipt { before, heads: landed, patches, dropped_anchors: dropped, repairs: view.repairs, warnings })
    }

    /// Runs `write`, which changes the value at `target` of the card at `at`
    /// on the card, lands it in the document, and answers its receipt.
    pub(crate) fn landing(&mut self, at: Option<usize>, target: Target<'_>, write: impl FnOnce(&mut Card) -> Result<Vec<ImportWarning>, EditError>) -> Result<Receipt, EditError> {
        let (root, put) = self.value_obj(at, target)?;
        self.doc().track();
        let before = Heads::from(self.doc().heads());
        let was = self.card_at(at).read(target)?;
        let warnings = self.edit_card(at, |card| {
            write(card).inspect_err(|_| {
                if let (true, Target::Field(name)) = (put, target) {
                    let _ = card.remove_field(name);
                }
            })
        })?;
        let units = self.units();
        let doc = self.doc();
        let (patches, view) = doc.landed(&root, before.hashes(), &was, units);
        let landed = Heads::from(doc.heads());
        let dropped = change::lost_anchors(&was, &view.content, |_| true);
        Ok(Receipt { before, heads: landed, patches, dropped_anchors: dropped, repairs: view.repairs, warnings })
    }
}
