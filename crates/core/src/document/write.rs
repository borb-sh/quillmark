//! The content write primitive and its producers.
//!
//! [`Card::apply`] writes a [`Change`] to a body or a richtext field and
//! answers a [`Receipt`]. [`Card::revise`] and [`Card::replace`] build the
//! change from a writer's markdown or a whole value and apply it.
//! [`Document::apply_all`] applies several writes, all or nothing, each naming
//! its card by `$id`. Each takes the hash its writer read as `base`. A base
//! the value has moved on from rebases the write over what changed since,
//! which the card's change log holds; a write that conflicts with it is
//! refused with `edit::rebase_conflict`, and a base the log no longer reaches
//! with `edit::stale_base`.

use std::collections::VecDeque;
use std::sync::Arc;

use quillmark_content::change::{self, Change, Dropped, Moved, RebaseError, Step};
use quillmark_content::diff;
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

/// The card a write reaches: the main card, or a composable card by its
/// `$id`, which a card keeps wherever it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardRef<'a> {
    Main,
    Id(&'a str),
}

/// What a write did.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The change in its normalized form, which [`change::invert`] undoes.
    pub applied: Change,
    /// The value's new hash, the next write's base.
    pub hash: String,
    /// Each anchor the value held and holds nowhere now, as it read before,
    /// with the slot that held it: every one a revise or replace lost, and
    /// for an apply every one but those its `anchor` steps removed.
    pub dropped_anchors: Vec<Dropped>,
    /// Each surviving anchor whose text the write rewrote.
    pub moved_anchors: Vec<Moved>,
    /// The markdown import's warnings, for a revise.
    pub warnings: Vec<ImportWarning>,
    /// A revise or replace whose diff rewrote the whole text of the value or
    /// of a slot as one splice, since no step-wise change reached it
    /// ([`diff::Diff::whole_text`]).
    pub whole_text: bool,
    /// The base was a value the field had moved on from, and the write was
    /// rebased over what changed since.
    pub rebased: bool,
}

impl Receipt {
    /// The receipt's warnings as diagnostics at `at`: a
    /// [`dropped_construct`](super::dropped_construct) per construct the
    /// markdown dropped, and a [`whole_text_diff`](super::whole_text_diff)
    /// where the diff rewrote a whole text.
    pub fn diagnostics(&self, at: Option<&crate::path::DocPath>) -> Vec<crate::error::Diagnostic> {
        let mut out = super::dropped_constructs(self.warnings.clone(), at);
        if self.whole_text {
            let warning = super::whole_text_diff();
            out.push(match at {
                Some(at) => warning.with_path(at.to_string()),
                None => warning,
            });
        }
        out
    }
}

/// One write of [`Document::apply_all`].
#[derive(Debug, Clone)]
pub struct Write<'a> {
    pub card: CardRef<'a>,
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

    /// Apply `change` to the value at `target`. A `base` the value has moved
    /// on from rebases `change` over what changed since; a refused change
    /// writes nothing.
    pub fn apply(&mut self, target: Target<'_>, change: &Change, base: Option<&str>) -> Result<Receipt, EditError> {
        let read = self.reading(target, base)?;
        let change = match &read.since {
            None => change.clone(),
            Some(since) => read.rebase(target, since, change)?,
        };
        let applied = change::apply(&read.now, &change).map_err(EditError::Change)?;
        self.land(target, read, applied, Vec::new(), None)
    }

    /// Rewrite the value at `target` as `markdown`: the anchors and islands
    /// whose `qm-*` tags it keeps stay put, and the rest of the value keeps
    /// what a word-aligned diff keeps. The diff is from the value `base`
    /// names, so what changed since is rebased over rather than undone.
    pub fn revise(&mut self, target: Target<'_>, markdown: &str, base: Option<&str>) -> Result<Receipt, EditError> {
        let read = self.reading(target, base)?;
        let revision = diff::revise(read.read(), markdown).map_err(EditError::Import)?;
        let change = match &read.since {
            None => revision.change,
            Some(since) => read.rebase(target, since, &revision.change)?,
        };
        let applied = change::apply(&read.now, &change).map_err(EditError::Change)?;
        self.land(target, read, applied, revision.warnings, Some(revision.whole_text))
    }

    /// Write `content` over the value at `target`, each anchor `content`
    /// names nowhere carried where the diff keeps its text. A `content` that
    /// is no valid value is refused. The diff is from the value `base` names,
    /// as a revise's is.
    pub fn replace(&mut self, target: Target<'_>, content: &Content, base: Option<&str>) -> Result<Receipt, EditError> {
        let read = self.reading(target, base)?;
        let diffed = diff::replace(read.read(), content).map_err(|e| EditError::Change(e.into()))?;
        let change = match &read.since {
            None => diffed.change,
            Some(since) => read.rebase(target, since, &diffed.change)?,
        };
        let applied = change::apply(&read.now, &change).map_err(EditError::Change)?;
        self.land(target, read, applied, Vec::new(), Some(diffed.whole_text))
    }

    /// The value `base` names at `target`: the value now, where `base` is
    /// absent or its hash, or the value the card's log holds for an earlier
    /// hash. Refused with `edit::stale_base` where the log reaches no value
    /// that hashes to `base`.
    pub fn read_at(&self, target: Target<'_>, base: Option<&str>) -> Result<Content, EditError> {
        let read = self.reading(target, base)?;
        Ok(match read.since {
            Some(since) => since.base,
            None => read.now,
        })
    }

    fn reading(&self, target: Target<'_>, base: Option<&str>) -> Result<Reading, EditError> {
        let now = self.read(target)?;
        let hash = content_hash(&now);
        let since = match base {
            Some(base) if base != hash => {
                let since = self.log.since(target, base, &hash);
                let stale = || BaseError::Stale { current: hash.clone(), field: field_of(target) };
                Some(since.ok_or_else(|| EditError::Base(stale()))?)
            }
            _ => None,
        };
        Ok(Reading { now, hash, since })
    }

    /// Store `applied` at `target` and log it. `diffed` is a diff's
    /// `whole_text`, `None` for a caller's change; a diff's receipt names
    /// every anchor it lost.
    fn land(
        &mut self,
        target: Target<'_>,
        read: Reading,
        mut applied: change::Applied,
        warnings: Vec<ImportWarning>,
        diffed: Option<bool>,
    ) -> Result<Receipt, EditError> {
        let Reading { now: before, hash: before_hash, since } = read;
        if diffed.is_some() {
            applied.dropped = change::lost_anchors(&before, &applied.content, |_| true);
        }
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
            self.log.record(target, Entry { before_hash, before, applied: applied.applied.clone(), after_hash: hash.clone() });
        }
        Ok(Receipt {
            applied: applied.applied,
            hash,
            dropped_anchors: applied.dropped,
            moved_anchors: applied.moved,
            warnings,
            whole_text: diffed.unwrap_or(false),
            rebased: since.is_some(),
        })
    }
}

fn field_of(target: Target<'_>) -> Option<String> {
    match target {
        Target::Body => None,
        Target::Field(name) => Some(name.to_string()),
    }
}

/// The value a write reads now and its hash, and where its base is an earlier
/// hash the log reaches, what changed since.
struct Reading {
    now: Content,
    hash: String,
    since: Option<Since>,
}

/// The value a base names, and the change from it to the value now.
struct Since {
    base: Content,
    over: Change,
}

impl Reading {
    /// The value the writer read.
    fn read(&self) -> &Content {
        self.since.as_ref().map_or(&self.now, |since| &since.base)
    }

    /// `change`, made from the value `since` names, rebased over the change
    /// since: a conflict refused with `edit::rebase_conflict`.
    fn rebase(&self, target: Target<'_>, since: &Since, change: &Change) -> Result<Change, EditError> {
        change::rebase(&since.base, change, &since.over).map_err(|e| match e {
            RebaseError::Apply(e) => EditError::Change(e),
            RebaseError::Conflict(c) => EditError::Base(BaseError::rebase_conflict(self.hash.clone(), field_of(target), &c)),
        })
    }
}

/// The most writes a card's log holds for one value.
pub const LOG_WRITES: usize = 32;

/// The most steps, nested ones counted, a card's log holds for one value
/// across its writes. The newest write stays however many it holds.
pub const LOG_STEPS: usize = 4096;

/// A card's recent writes, per value it holds, in memory only: no store,
/// hash or comparison sees it. Each write it holds names the hash it read
/// and the one it left, the value it wrote over, and the change in its
/// normalized form, so a write whose base is one of those hashes rebases over
/// the changes since. A write the log did not see breaks the chain, and the
/// log forgets what came before it. It holds at most [`LOG_WRITES`] writes
/// and [`LOG_STEPS`] steps per value, the oldest leaving first.
#[derive(Clone, Default)]
pub(crate) struct ChangeLog {
    targets: Vec<(Option<String>, VecDeque<Arc<Entry>>)>,
}

struct Entry {
    before_hash: String,
    before: Content,
    applied: Change,
    after_hash: String,
}

impl PartialEq for ChangeLog {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for ChangeLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChangeLog")
    }
}

fn steps_in(change: &Change) -> usize {
    change
        .steps
        .iter()
        .map(|s| match s {
            Step::Nested { change, .. } => 1 + steps_in(change),
            _ => 1,
        })
        .sum()
}

impl ChangeLog {
    fn key(target: Target<'_>) -> Option<&str> {
        match target {
            Target::Body => None,
            Target::Field(name) => Some(name),
        }
    }

    fn entries(&self, target: Target<'_>) -> Option<&VecDeque<Arc<Entry>>> {
        let key = Self::key(target);
        self.targets.iter().find(|(k, _)| k.as_deref() == key).map(|(_, e)| e)
    }

    fn record(&mut self, target: Target<'_>, entry: Entry) {
        let key = Self::key(target);
        let at = match self.targets.iter().position(|(k, _)| k.as_deref() == key) {
            Some(at) => at,
            None => {
                self.targets.push((key.map(str::to_string), VecDeque::new()));
                self.targets.len() - 1
            }
        };
        let entries = &mut self.targets[at].1;
        if entries.back().is_some_and(|last| last.after_hash != entry.before_hash) {
            entries.clear();
        }
        entries.push_back(Arc::new(entry));
        let mut steps: usize = entries.iter().map(|e| steps_in(&e.applied)).sum();
        while entries.len() > LOG_WRITES || (steps > LOG_STEPS && entries.len() > 1) {
            if let Some(gone) = entries.pop_front() {
                steps -= steps_in(&gone.applied);
            }
        }
    }

    /// The value `base` named at `target` and the change from it to the
    /// value now, which hashes to `current`: from the latest write that read
    /// `base` through the newest, when the newest left `current`.
    fn since(&self, target: Target<'_>, base: &str, current: &str) -> Option<Since> {
        let entries = self.entries(target)?;
        if entries.back()?.after_hash != current {
            return None;
        }
        let from = entries.iter().rposition(|e| e.before_hash == base)?;
        let over = entries.range(from..).fold(Change::default(), |over, e| change::compose(&over, &e.applied));
        Some(Since { base: entries[from].before.clone(), over })
    }
}

impl Document {
    /// The card `card` names, refused with `edit::unknown_card_id` where no
    /// composable card holds its id.
    pub fn card_at(&self, card: CardRef<'_>) -> Result<&Card, EditError> {
        match card {
            CardRef::Main => Ok(&self.main),
            CardRef::Id(id) => self.card_by_id(id).ok_or_else(|| EditError::UnknownCardId(id.to_string())),
        }
    }

    /// [`card_at`](Self::card_at), mutable.
    pub fn card_at_mut(&mut self, card: CardRef<'_>) -> Result<super::CardMut<'_>, EditError> {
        match card {
            CardRef::Main => Ok(self.main_mut()),
            CardRef::Id(id) => self.card_by_id_mut(id).ok_or_else(|| EditError::UnknownCardId(id.to_string())),
        }
    }

    /// Apply each write in order, or none of them: the first refused write's
    /// index and error, and the document as it was. Every card id resolves
    /// before any write applies.
    pub fn apply_all(&mut self, writes: &[Write<'_>]) -> Result<Vec<Receipt>, (usize, EditError)> {
        let cards = writes
            .iter()
            .enumerate()
            .map(|(n, w)| match w.card {
                CardRef::Main => Ok(None),
                CardRef::Id(id) => {
                    self.card_index(id).map(Some).ok_or_else(|| (n, EditError::UnknownCardId(id.to_string())))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut next = self.clone();
        let mut receipts = Vec::with_capacity(writes.len());
        for (n, (w, index)) in writes.iter().zip(cards).enumerate() {
            let card = match index {
                None => &mut next.main,
                Some(index) => &mut next.cards[index],
            };
            receipts.push(card.apply(w.target, &w.change, w.base).map_err(|e| (n, e))?);
        }
        *self = next;
        Ok(receipts)
    }
}
