//! The content write, landed in the document's Automerge document.
//!
//! A write builds its change against the value its writer read, at the
//! heads its base was left at or now, and lowers it as one Automerge change:
//! at earlier heads it is concurrent with every change since and merges with
//! them as Automerge merges, refusing nothing a concurrent change did. The
//! card then takes the value the write left.

use std::collections::BTreeMap;

use quillmark_automerge::automerge::transaction::Transactable;
use quillmark_automerge::automerge::{ActorId, ChangeHash, ObjId, ReadDoc, Value as AmValue};
use quillmark_automerge::model::{self as am, StepError};
use quillmark_content::change::{self, AnchorStep, ApplyError, Change, Step};
use quillmark_content::diff;
use quillmark_content::ids::is_actor;
use quillmark_content::model::Content;
use quillmark_content::units::{self, Units};

use super::base::{Base, BaseError, content_hash};
use super::edit::EditError;
use super::write::{Edit, Receipt, Target, WriteOptions, address, check_change, keeps_ext_islands};
use super::{Card, Document};
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

fn obj(doc: &am::Doc, parent: &ObjId, key: &str) -> Option<ObjId> {
    match doc.am().get(parent, key).ok()?? {
        (AmValue::Object(_), id) => Some(id),
        _ => None,
    }
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

/// `content` with each id `minted` renames renamed.
fn renamed_content(content: &Content, minted: &BTreeMap<String, String>) -> Content {
    if minted.is_empty() {
        return content.clone();
    }
    Content::from_value(&renamed_json(&content.to_value(), minted, &["text", "kind", "type"])).unwrap_or_else(|_| content.clone())
}

/// Whether `content` holds a point anchor, which Automerge keeps as a
/// cursor that a splice can leave elsewhere than the content model does.
fn holds_points(content: &Content) -> bool {
    content.anchors.iter().any(|a| a.start == a.end) || content.islands.iter().any(|i| i.slots.values().any(holds_points))
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
        let doc = self.doc();
        let card = match (at, id) {
            (None, _) => doc.main(),
            (Some(_), Some(id)) => obj(doc, &doc.cards(), &id).expect("a flushed card is held"),
            (Some(i), None) => return Err(EditError::IndexOutOfRange { index: i, len: 0 }),
        };
        let Target::Field(name) = target else {
            return Ok((doc.value(&card, am::BODY).expect("a card holds its body"), put));
        };
        let fields = obj(doc, &card, am::FIELDS).expect("a card holds its fields");
        if let Some(v) = doc.value(&fields, name) {
            return Ok((v, put));
        }
        if doc.am().get(&fields, name).unwrap().is_some() {
            doc.am_mut().delete(&fields, name).unwrap();
        }
        let v = doc.put_value(&fields, name, &content.expect("a field target reads its value"));
        doc.am_mut().commit();
        self.reread();
        Ok((v, put))
    }

    fn address_of(&self, at: Option<usize>, target: Target<'_>) -> String {
        address(self.card_at(at).id(), target)
    }

    /// The heads a write naming `base` builds at: none for the value now,
    /// the heads a write left the hash at otherwise.
    fn heads_for(&mut self, at: Option<usize>, target: Target<'_>, root: &ObjId, base: Option<Base<'_>>) -> Result<Option<Vec<ChangeHash>>, EditError> {
        let Some(base) = base else { return Ok(None) };
        let now = self.doc().view(root).content;
        let hash = match base {
            Base::Hash(h) => h.to_string(),
            Base::Content(c) => content_hash(c),
        };
        let current = content_hash(&now);
        if hash == current {
            return Ok(None);
        }
        let key = (self.address_of(at, target), hash);
        if let Some(heads) = self.seen().get(&key) {
            return Ok(Some(heads.clone()));
        }
        let history = self.history().clone();
        for heads in history.iter().rev() {
            let read = self.doc().try_view_at(root, heads);
            if read.is_some_and(|v| content_hash(&v.content) == key.1) {
                self.seen().insert(key, heads.clone());
                return Ok(Some(heads.clone()));
            }
        }
        Err(EditError::Base(BaseError::Stale { current, field: target.field().map(str::to_string) }))
    }

    /// Notes `content`, the value at `target` of the card at `at` now, as
    /// read at the document's heads, and answers its hash.
    fn note(&mut self, at: Option<usize>, target: Target<'_>, content: &Content) -> String {
        let address = self.address_of(at, target);
        let live = self.held();
        let heads = live.doc.heads();
        let hash = match &live.last {
            Some((a, h, hash)) if *a == address && *h == heads => hash.clone(),
            _ => content_hash(content),
        };
        super::remember(&mut live.history, heads.clone());
        live.seen.entry((address.clone(), hash.clone())).or_insert_with(|| heads.clone());
        live.last = Some((address, heads, hash.clone()));
        hash
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

    /// The value at `target` of the card at `at` as its writer read it under
    /// `base`, refused with `edit::stale_base` where no write left it.
    pub(crate) fn read_under(&mut self, at: Option<usize>, target: Target<'_>, base: Option<&str>) -> Result<Content, EditError> {
        if base.is_none() {
            return self.card_at(at).read(target);
        }
        let (root, _) = self.value_obj(at, target)?;
        let heads = self.heads_for(at, target, &root, base.map(Base::Hash))?;
        let doc = self.doc();
        Ok(match heads {
            Some(h) => doc.view_at(&root, &h).content,
            None => doc.view(&root).content,
        })
    }

    /// Lands `edit` on the value at `target` of the card at `at`, the main
    /// card where `None`, built against the value its base names.
    pub(crate) fn write_at<U: Units>(&mut self, at: Option<usize>, target: Target<'_>, edit: Edit<'_>, opts: WriteOptions<'_>, u: Option<U>) -> Result<Receipt, EditError> {
        if let Edit::Apply(change) = &edit {
            check_change(change)?;
        }
        if let Some(actor) = opts.actor.filter(|a| !is_actor(a)) {
            return Err(EditError::InvalidActor(actor.to_string()));
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
        let heads = self.heads_for(at, target, &root, opts.base)?;
        let now = self.card_at(at).read(target)?;
        debug_assert_eq!(now, self.held().doc.view(&root).content);
        self.note(at, target, &now);
        let read = match &heads {
            Some(h) => self.held().doc.view_at(&root, h).content,
            None => now.clone(),
        };
        let (change, warnings, diffed) = match edit {
            Edit::Apply(change) => match u {
                Some(u) => {
                    let filled = quillmark_content::mint::fill(&change, &[&read], None).map_err(EditError::change)?;
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
                (revision.change, revision.warnings, true)
            }
            Edit::Replace(content) => (diff::replace_holding(&read, &content, &now).map_err(|e| EditError::change(e.into()))?, Vec::new(), true),
        };
        let mine = change::apply(&read, &change).map_err(EditError::change)?;
        if diffed {
            keeps_ext_islands(&read, &mine.content)?;
        }
        let doc = &mut self.held().doc;
        let actor = doc.am().get_actor().clone();
        if let Some(a) = opts.actor {
            doc.set_actor(ActorId::from(a.as_bytes()));
        }
        let lowered = doc.apply_from(&root, &change, heads.as_deref(), Some(&read));
        if opts.actor.is_some() {
            doc.set_actor(actor);
        }
        let lowered = lowered.map_err(step_error)?;
        let merged = heads.is_some();
        let named: BTreeMap<String, String> = mine.minted.iter().filter_map(|(placeholder, id)| Some((id.clone(), lowered.minted.get(placeholder)?.clone()))).collect();
        let after = match merged || diffed || holds_points(&read) || holds_points(&mine.content) {
            true => doc.view(&root).content,
            false => {
                let held: BTreeMap<String, String> = lowered.minted.iter().map(|(given, id)| (mine.minted.get(given).unwrap_or(given).clone(), id.clone())).collect();
                let after = renamed_content(&mine.content, &held);
                debug_assert_eq!(after, doc.view(&root).content);
                after
            }
        };
        let (dropped_anchors, moved_anchors) = match merged || diffed {
            false => (mine.dropped.clone(), mine.moved.clone()),
            true => {
                let mut removed = Vec::new();
                removed_by(&change, &mut removed);
                let mut dropped = change::lost_anchors(&now, &after, |id| diffed || !removed.iter().any(|r| r == id));
                let held: std::collections::HashSet<String> = now.anchor_ids().into_iter().collect();
                dropped.extend(change::lost_anchors(&mine.content, &after, |id| !held.contains(id)));
                (dropped, change::moved_between(&now, &after, &held))
            }
        };
        let applied = match merged || diffed {
            true => diff::diff(&now, &after).map_err(|e| EditError::change(e.into()))?,
            false => renamed(&mine.applied, &named),
        };
        let applied = match u {
            Some(u) => units::change_from_scalars(&applied, &now, u).map_err(EditError::change)?,
            None => applied,
        };
        self.set_read(at, target, &after);
        let hash = self.note(at, target, &after);
        let minted = match diffed {
            true => BTreeMap::new(),
            false => lowered.minted,
        };
        Ok(Receipt::landed(applied, hash, dropped_anchors, moved_anchors, warnings, merged, minted))
    }
}
