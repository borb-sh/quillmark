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

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use quillmark_content::change::{self, Change, ContainerStep, Dropped, IslandStep, Moved, RebaseError, Step};
use quillmark_content::diff;
use quillmark_content::lines::import::ImportWarning;
use quillmark_content::lines::island::IslandType;
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
        let taken = self.log.ids(target);
        let revision = diff::revise_holding(read.read(), markdown, &read.now, &taken).map_err(EditError::Import)?;
        let change = match &read.since {
            None => revision.change,
            Some(since) => read.rebase(target, since, &revision.change)?,
        };
        let applied = change::apply(&read.now, &change).map_err(EditError::Change)?;
        keeps_ext_islands(&read.now, &applied.content)?;
        self.land(target, read, applied, revision.warnings, Some(revision.whole_text))
    }

    /// Write `content` over the value at `target`, each anchor `content`
    /// names nowhere carried where the diff keeps its text. A `content` that
    /// is no valid value is refused. The diff is from the value `base` names,
    /// as a revise's is.
    pub fn replace(&mut self, target: Target<'_>, content: &Content, base: Option<&str>) -> Result<Receipt, EditError> {
        let read = self.reading(target, base)?;
        let taken = self.log.ids(target);
        let diffed = diff::replace_holding(read.read(), content, &read.now, &taken).map_err(|e| EditError::Change(e.into()))?;
        let change = match &read.since {
            None => diffed.change,
            Some(since) => read.rebase(target, since, &diffed.change)?,
        };
        let applied = change::apply(&read.now, &change).map_err(EditError::Change)?;
        keeps_ext_islands(&read.now, &applied.content)?;
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
            self.log.record(target, before_hash, before, applied.applied.clone(), hash.clone());
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

/// Refuses a revise or replace whose result no longer holds an `x-` island
/// `before` holds, at any depth.
fn keeps_ext_islands(before: &Content, after: &Content) -> Result<(), EditError> {
    fn ext(c: &Content, at: &str, out: &mut Vec<(String, String, String)>) {
        for island in &c.islands {
            let here = format!("{at}/{}", island.id);
            if let IslandType::Ext(name) = &island.island_type {
                out.push((here.clone(), island.id.clone(), name.clone()));
            }
            for (slot, inner) in &island.slots {
                ext(inner, &format!("{here}/{slot}"), out);
            }
        }
    }
    let (mut held, mut kept) = (Vec::new(), Vec::new());
    ext(before, "", &mut held);
    ext(after, "", &mut kept);
    let kept: HashSet<&str> = kept.iter().map(|(at, _, _)| at.as_str()).collect();
    match held.into_iter().find(|(at, _, _)| !kept.contains(at.as_str())) {
        Some((_, island, kind)) => Err(EditError::DropsExtIsland { island, kind }),
        None => Ok(()),
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

/// The steps, nested ones counted, a card's log reaches back for one value.
/// A base rebases while the writes since it hold at most this many steps,
/// and the base the newest write read rebases however many that write
/// holds. The log holds at most twice this many steps, or the newest write
/// alone where it holds more.
pub const LOG_STEPS: usize = 4096;

/// A card's recent writes, per value it holds, in memory only: no store,
/// hash or comparison sees it. Per value it holds one snapshot, the value
/// its oldest write wrote over, and for each write the change in its
/// normalized form and the hash it left, so a write whose base is a hash it
/// holds rebases over the changes since. A write the log did not see breaks
/// the chain, and the log forgets what came before it. A write that takes
/// it past twice [`LOG_STEPS`] folds its oldest writes into the snapshot,
/// never the newest, until it holds at most [`LOG_STEPS`].
#[derive(Clone, Default)]
pub(crate) struct ChangeLog {
    targets: Vec<(Option<String>, Track)>,
}

#[derive(Clone)]
struct Track {
    floor: Arc<Content>,
    floor_hash: String,
    entries: VecDeque<Arc<Entry>>,
    steps: usize,
}

struct Entry {
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

/// `changes` as one change, composed by halves so each step is copied once
/// per level rather than once per change after it.
fn composed(changes: &[&Change]) -> Change {
    match changes {
        [] => Change::default(),
        [one] => (*one).clone(),
        _ => {
            let (head, tail) = changes.split_at(changes.len() / 2);
            change::compose(&composed(head), &composed(tail))
        }
    }
}

impl Track {
    fn new(before_hash: String, before: Content) -> Self {
        Track { floor: Arc::new(before), floor_hash: before_hash, entries: VecDeque::new(), steps: 0 }
    }

    /// The hash write `k` read.
    fn read(&self, k: usize) -> &str {
        match k {
            0 => &self.floor_hash,
            _ => &self.entries[k - 1].after_hash,
        }
    }

    fn changes(&self, range: impl std::ops::RangeBounds<usize>) -> Change {
        composed(&self.entries.range(range).map(|e| &e.applied).collect::<Vec<_>>())
    }

    /// The value write `k` read: the snapshot, with the writes before it
    /// applied as one change.
    fn value_read(&self, k: usize) -> Option<Content> {
        match k {
            0 => Some(Content::clone(&self.floor)),
            _ => change::apply(&self.floor, &self.changes(..k)).ok().map(|a| a.content),
        }
    }

    /// Fold the oldest writes into the snapshot once the track holds more
    /// than twice [`LOG_STEPS`], so a fold, one apply over the value, comes
    /// at most once per [`LOG_STEPS`] steps written, or right after a write
    /// that alone holds more. `false` where the fold does not apply, and the
    /// track is to be forgotten.
    fn fold(&mut self) -> bool {
        if self.steps <= 2 * LOG_STEPS {
            return true;
        }
        let (mut k, mut steps) = (0, self.steps);
        while k + 1 < self.entries.len() && steps > LOG_STEPS {
            steps -= steps_in(&self.entries[k].applied);
            k += 1;
        }
        if k == 0 {
            return true;
        }
        let Ok(folded) = change::apply(&self.floor, &self.changes(..k)) else { return false };
        self.floor = Arc::new(folded.content);
        self.floor_hash = self.read(k).to_string();
        self.entries.drain(..k);
        self.steps = steps;
        true
    }
}

impl ChangeLog {
    fn key(target: Target<'_>) -> Option<&str> {
        match target {
            Target::Body => None,
            Target::Field(name) => Some(name),
        }
    }

    fn track(&self, target: Target<'_>) -> Option<&Track> {
        let key = Self::key(target);
        self.targets.iter().find(|(k, _)| k.as_deref() == key).map(|(_, t)| t)
    }

    /// Log a write that read `before`, which hashes to `before_hash`, applied
    /// `applied` and left `after_hash`.
    fn record(
        &mut self,
        target: Target<'_>,
        before_hash: String,
        before: Content,
        applied: Change,
        after_hash: String,
    ) {
        let key = Self::key(target);
        let at = match self.targets.iter().position(|(k, _)| k.as_deref() == key) {
            Some(at) => {
                let track = &mut self.targets[at].1;
                if track.read(track.entries.len()) != before_hash {
                    *track = Track::new(before_hash, before);
                }
                at
            }
            None => {
                self.targets.push((key.map(str::to_string), Track::new(before_hash, before)));
                self.targets.len() - 1
            }
        };
        let track = &mut self.targets[at].1;
        track.steps += steps_in(&applied);
        track.entries.push_back(Arc::new(Entry { applied, after_hash }));
        if !track.fold() {
            self.targets.remove(at);
        }
    }

    /// Each top-level block, container and island id the log's snapshot at
    /// `target` holds or a write it holds introduced: the ids a base the log
    /// reaches may name, which a write mints past.
    fn ids(&self, target: Target<'_>) -> HashSet<String> {
        let Some(track) = self.track(target) else { return HashSet::new() };
        let floor = &track.floor;
        let mut ids: HashSet<String> = floor.blocks.iter().map(|b| b.id.clone()).collect();
        ids.extend(floor.containers.iter().map(|k| k.id.clone()));
        ids.extend(floor.islands.iter().map(|i| i.id.clone()));
        for step in track.entries.iter().flat_map(|e| &e.applied.steps) {
            match step {
                Step::Splice(s) => {
                    ids.extend(s.blocks.iter().map(|b| b.id.clone()));
                    ids.extend(s.islands.iter().map(|i| i.id.clone()));
                }
                Step::Island(IslandStep::Insert { island, .. }) => {
                    ids.insert(island.id.clone());
                }
                Step::Container(ContainerStep::Set(k)) => {
                    ids.insert(k.id.clone());
                }
                _ => {}
            }
        }
        ids
    }

    /// The value `base` named at `target` and the change from it to the
    /// value now, which hashes to `current`: from the latest write that read
    /// `base` through the newest, when the newest left `current`. A value
    /// rebuilt from the snapshot that does not hash to `base` reaches none.
    fn since(&self, target: Target<'_>, base: &str, current: &str) -> Option<Since> {
        let track = self.track(target)?;
        if track.entries.back()?.after_hash != current {
            return None;
        }
        let from = (0..track.entries.len()).rposition(|k| track.read(k) == base)?;
        let read = track.value_read(from)?;
        if content_hash(&read) != base {
            return None;
        }
        Some(Since { base: read, over: track.changes(from..) })
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
