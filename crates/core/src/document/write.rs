//! The content write primitive and its producers.
//!
//! [`Card::apply`] writes a [`Change`] to a body or a richtext field and
//! answers a [`Receipt`]. [`Card::revise`] and [`Card::replace`] build the
//! change from a writer's markdown or a whole value and apply it.
//! [`Document::apply_all`] applies several writes, all or nothing. Each takes
//! the hash its writer read as `base` and refuses the write with
//! `edit::stale_base` when the value has changed since.

use quillmark_content::change::{self, Change, Moved};
use quillmark_content::diff;
use quillmark_content::lines::anchor::AnchorContext;
use quillmark_content::lines::import::ImportWarning;
use quillmark_content::model::Content;

use super::base::{BaseError, content_hash};
use super::edit::EditError;
use super::{Card, Document};
use crate::value::QuillValue;

/// The value a write reaches on its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    Body,
    /// A richtext field, an absent one the empty content.
    Field(&'a str),
}

/// What a write did.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The change in its normalized form, which [`change::invert`] undoes.
    pub applied: Change,
    /// The value's new hash, the next write's base.
    pub hash: String,
    /// Each anchor whose text the write deleted, as it read before.
    pub dropped_anchors: Vec<AnchorContext>,
    /// Each surviving anchor whose text the write rewrote.
    pub moved_anchors: Vec<Moved>,
    /// The markdown import's warnings, for a revise.
    pub warnings: Vec<ImportWarning>,
}

/// One write of [`Document::apply_all`].
#[derive(Debug, Clone)]
pub struct Write<'a> {
    /// The card, `None` for the main card.
    pub card: Option<usize>,
    pub target: Target<'a>,
    pub change: Change,
    pub base: Option<&'a str>,
}

impl Card {
    /// The value at `target`.
    pub fn read(&self, target: Target<'_>) -> Result<Content, EditError> {
        match target {
            Target::Body => Ok(self.body().clone()),
            Target::Field(name) => self.richtext_value(name),
        }
    }

    /// The hash of the value at `target`, the `base` a write to it names.
    pub fn hash_of(&self, target: Target<'_>) -> Result<String, EditError> {
        Ok(content_hash(&self.read(target)?))
    }

    /// Apply `change` to the value at `target`, refused when `base` is not
    /// the value's hash. A refused change writes nothing.
    pub fn apply(&mut self, target: Target<'_>, change: &Change, base: Option<&str>) -> Result<Receipt, EditError> {
        let before = self.read_at(target, base)?;
        let applied = change::apply(&before, change).map_err(EditError::Change)?;
        self.land(target, before, applied, Vec::new())
    }

    /// Rewrite the value at `target` as `markdown`: the anchors and islands
    /// whose `qm-*` tags it keeps stay put, and the rest of the value keeps
    /// what a word-aligned diff keeps.
    pub fn revise(&mut self, target: Target<'_>, markdown: &str, base: Option<&str>) -> Result<Receipt, EditError> {
        let before = self.read_at(target, base)?;
        let revision = diff::revise(&before, markdown).map_err(EditError::Import)?;
        let applied = change::apply(&before, &revision.change).map_err(EditError::Change)?;
        self.land(target, before, applied, revision.warnings)
    }

    /// Write `content` over the value at `target`, each anchor `content` does
    /// not name carried where the diff keeps its text.
    pub fn replace(&mut self, target: Target<'_>, content: &Content, base: Option<&str>) -> Result<Receipt, EditError> {
        let before = self.read_at(target, base)?;
        let applied = change::apply(&before, &diff::replace(&before, content)).map_err(EditError::Change)?;
        self.land(target, before, applied, Vec::new())
    }

    /// The value at `target`, refused with `edit::stale_base` when `base` is
    /// not its hash.
    pub fn read_at(&self, target: Target<'_>, base: Option<&str>) -> Result<Content, EditError> {
        let before = self.read(target)?;
        if let Some(base) = base {
            let current = content_hash(&before);
            if current != base {
                let field = match target {
                    Target::Body => None,
                    Target::Field(name) => Some(name.to_string()),
                };
                return Err(EditError::Base(BaseError::Stale { current, field }));
            }
        }
        Ok(before)
    }

    fn land(
        &mut self,
        target: Target<'_>,
        before: Content,
        applied: change::Applied,
        warnings: Vec<ImportWarning>,
    ) -> Result<Receipt, EditError> {
        let hash = content_hash(&applied.content);
        if applied.content != before {
            match target {
                Target::Body => self.set_body(applied.content),
                Target::Field(name) => {
                    self.payload_mut()
                        .insert(name.to_string(), QuillValue::from_json(applied.content.to_value()))
                        .map_err(EditError::InvalidPayload)?;
                }
            }
        }
        Ok(Receipt {
            applied: applied.applied,
            hash,
            dropped_anchors: applied.dropped,
            moved_anchors: applied.moved,
            warnings,
        })
    }
}

impl Document {
    /// Apply each write in order, or none of them: the first refused write's
    /// index and error, and the document as it was.
    pub fn apply_all(&mut self, writes: &[Write<'_>]) -> Result<Vec<Receipt>, (usize, EditError)> {
        let mut next = self.clone();
        let mut receipts = Vec::with_capacity(writes.len());
        for (n, w) in writes.iter().enumerate() {
            let len = next.cards.len();
            let card = match w.card {
                None => &mut next.main,
                Some(index) => next.cards.get_mut(index).ok_or((n, EditError::IndexOutOfRange { index, len }))?,
            };
            receipts.push(card.apply(w.target, &w.change, w.base).map_err(|e| (n, e))?);
        }
        *self = next;
        Ok(receipts)
    }
}
