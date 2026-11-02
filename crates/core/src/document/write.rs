//! The content write primitive and its producers.
//!
//! [`Card::write`] lands an [`Edit`] on a body or a richtext field and
//! answers a [`Receipt`]: a [`Change`], or one built from a gesture, a
//! writer's markdown or a whole value. Each takes the hash its writer read as
//! [`WriteOptions::base`]. A base the value has moved on from rebases the
//! write over what changed since, which the card's change log holds; a write
//! that conflicts with it is refused with `edit::rebase_conflict`, and a base
//! the log no longer reaches with `edit::stale_base`.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::Arc;

use quillmark_content::change::{self, Change, Dropped, Moved, RebaseError, Step};
use quillmark_content::diff;
use quillmark_content::gestures::{Gesture, Wrap};
use quillmark_content::ids::is_actor;
use quillmark_content::mint;
use quillmark_content::lines::import::ImportWarning;
use quillmark_content::lines::island::IslandType;
use quillmark_content::model::Content;
use quillmark_content::units::{self, Units};

use super::base::{BaseError, content_hash};
use super::edit::EditError;
use super::Card;
use crate::value::QuillValue;

/// Where an offset sits: the block holding it, by id and index, and that
/// block's container path, outermost first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub id: String,
    pub index: usize,
    pub path: Vec<String>,
}

/// The value a write reaches on its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    Body,
    /// A richtext field, an absent one the empty content.
    Field(&'a str),
}

impl<'a> Target<'a> {
    /// The field's name, `None` for the body.
    pub fn field(self) -> Option<&'a str> {
        match self {
            Target::Body => None,
            Target::Field(name) => Some(name),
        }
    }
}

impl<'a> From<Option<&'a str>> for Target<'a> {
    /// `None` the body, a name that field.
    fn from(field: Option<&'a str>) -> Self {
        field.map_or(Target::Body, Target::Field)
    }
}

/// What a write reads besides its edit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteOptions<'a> {
    /// The hash the writer read, which a write over a value that has moved on
    /// rebases from.
    pub base: Option<&'a str>,
    /// Who writes, scoping each id the write mints: `{actor}:{prefix}{n}`
    /// where it is given, `{prefix}{n}` where not. A live client passes one
    /// unique to its session, so two clients writing from one value never
    /// mint one id. An actor other than an
    /// [id](quillmark_content::ids::is_actor) of at most 64 scalar values
    /// holding no `:` is refused with `edit::invalid_actor`.
    pub actor: Option<&'a str>,
}

impl<'a> From<Option<&'a str>> for WriteOptions<'a> {
    /// The options of a write with this base and no actor.
    fn from(base: Option<&'a str>) -> Self {
        WriteOptions { base, actor: None }
    }
}

/// How a write builds its change from the value its writer read.
#[derive(Debug, Clone)]
pub enum Edit<'a> {
    /// A change, its offsets into the value the base names. A record it puts
    /// may carry a [placeholder](quillmark_content::ids::is_placeholder),
    /// which the write fills and names in [`Receipt::minted`].
    Apply(Change),
    /// A gesture's change on the value the base names, its offsets into it;
    /// the ids it opens are placeholders the write fills.
    Gesture(Gesture),
    /// The value rewritten as markdown: the anchors and islands whose `qm-*`
    /// tags it keeps stay put, and the rest keeps what a word-aligned diff
    /// keeps.
    Revise(&'a str),
    /// A whole value, each anchor it names nowhere carried where the diff
    /// keeps its text. A value that is not valid is refused.
    Replace(Content),
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
    /// Each surviving anchor the write moved, a point included, and what
    /// moved: its position, its block or its context.
    pub moved_anchors: Vec<Moved>,
    /// The markdown import's warnings, for a revise.
    pub warnings: Vec<ImportWarning>,
    /// The base was a value the field had moved on from, and the write was
    /// rebased over what changed since, or, for a writer's revise under its
    /// markdown read, merged with it.
    pub rebased: bool,
    /// Each placeholder the change gave a record it put, and the id the
    /// write minted for it past every id the value it landed on, the value
    /// its base named and the writes since hold.
    pub minted: BTreeMap<String, String>,
    counted: Counted,
}

/// What a receipt's `applied` counts its offsets over, when it counts them in
/// scalars and holds what [`Receipt::in_units`] converts them on.
#[derive(Debug, Clone, PartialEq)]
enum Counted {
    Host,
    Content(Arc<Content>),
}

impl Receipt {
    /// The receipt with `applied` counted in `u`. A writer's revise answers
    /// its change in scalars, and this converts it on the value it read; a
    /// receipt [`Card::write`] or [`Card::write_in`] answered comes back as
    /// it is, in the units its write was given.
    pub fn in_units(mut self, u: impl Units) -> Receipt {
        self.applied = match std::mem::replace(&mut self.counted, Counted::Host) {
            Counted::Host => return self,
            Counted::Content(before) => units::change_from_scalars(&self.applied, &before, u)
                .expect("a receipt's change applies to the value it read"),
        };
        self
    }

    /// The receipt's warnings as diagnostics at `at`: a
    /// [`dropped_construct`](super::dropped_construct) per construct the
    /// markdown dropped.
    pub fn diagnostics(&self, at: Option<&crate::path::DocPath>) -> Vec<crate::error::Diagnostic> {
        super::dropped_constructs(self.warnings.clone(), at)
    }
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
        match target {
            Target::Body => Ok(self.body_hash()),
            Target::Field(name) => self.field_hash(name),
        }
    }

    /// [`write`](Self::write) of [`Edit::Apply`].
    pub fn apply<'o>(&mut self, target: Target<'_>, change: &Change, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Apply(change.clone()), opts)
    }

    /// [`write`](Self::write) of [`Edit::Revise`].
    pub fn revise<'o>(&mut self, target: Target<'_>, markdown: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Revise(markdown), opts)
    }

    /// [`write`](Self::write) of [`Edit::Replace`].
    pub fn replace<'o>(&mut self, target: Target<'_>, content: &Content, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Replace(content.clone()), opts)
    }

    /// Enter at `at`: [`write`](Self::write) of
    /// [`Gesture::SplitBlock`], the new block `id` or one the write mints.
    pub fn split_block<'o>(&mut self, target: Target<'_>, at: usize, id: Option<&str>, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::SplitBlock { at, id: id.map(str::to_string) }), opts)
    }

    /// Backspace at the start of block `id`: [`write`](Self::write) of
    /// [`Gesture::JoinBlocks`].
    pub fn join_blocks<'o>(&mut self, target: Target<'_>, id: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::JoinBlocks { id: id.to_string() }), opts)
    }

    /// Deleting `[start, end)`: [`write`](Self::write) of
    /// [`Gesture::DeleteRange`].
    pub fn delete_range<'o>(&mut self, target: Target<'_>, start: usize, end: usize, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::DeleteRange { start, end }), opts)
    }

    /// Tab over the blocks `first` to `last`: [`write`](Self::write) of
    /// [`Gesture::Indent`].
    pub fn indent<'o>(&mut self, target: Target<'_>, first: &str, last: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::Indent { first: first.to_string(), last: last.to_string() }), opts)
    }

    /// Shift-Tab over the blocks `first` to `last`: [`write`](Self::write)
    /// of [`Gesture::Outdent`].
    pub fn outdent<'o>(&mut self, target: Target<'_>, first: &str, last: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::Outdent { first: first.to_string(), last: last.to_string() }), opts)
    }

    /// Wrapping the blocks `first` to `last`: [`write`](Self::write) of
    /// [`Gesture::Wrap`].
    pub fn wrap<'o>(&mut self, target: Target<'_>, first: &str, last: &str, with: Wrap, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Gesture(Gesture::Wrap { first: first.to_string(), last: last.to_string(), with }), opts)
    }

    /// The block holding offset `at` of the value at `target`, its index and
    /// its container path; at a block's end, that block. `Stale` where `at`
    /// is past the text.
    pub fn locate(&self, target: Target<'_>, at: usize) -> Result<Located, EditError> {
        let content = self.read(target)?;
        if at > content.len_usv() {
            return Err(EditError::change(change::ApplyError::Stale { step: 0 }));
        }
        let index = content.block_at(at);
        let block = &content.blocks[index];
        Ok(Located { id: block.id.clone(), index, path: block.path.clone() })
    }

    /// Land `edit` on the value at `target`, all of it or none. A base the
    /// value has moved on from rebases the edit's change over what changed
    /// since; a revise or replace diffs from the value the base names, so what
    /// changed since is rebased over rather than undone, and is refused where
    /// it would drop an `x-` island. Each id it mints is minted under the
    /// options' actor where one is given.
    pub fn write<'o>(&mut self, target: Target<'_>, edit: Edit<'_>, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write_with(target, edit, opts.into(), None::<units::Scalars>)
    }

    /// [`write`](Self::write) for a host counting offsets in `u`: an
    /// [`Edit::Apply`]'s change, an [`Edit::Gesture`]'s offsets and the
    /// receipt's `applied` are in `u`.
    pub fn write_in<'o>(&mut self, target: Target<'_>, edit: Edit<'_>, opts: impl Into<WriteOptions<'o>>, u: impl Units) -> Result<Receipt, EditError> {
        self.write_with(target, edit, opts.into(), Some(u))
    }

    fn write_with<U: Units>(&mut self, target: Target<'_>, edit: Edit<'_>, opts: WriteOptions<'_>, u: Option<U>) -> Result<Receipt, EditError> {
        if let Edit::Apply(change) = &edit {
            check_change(change)?;
        }
        if let Some(actor) = opts.actor.filter(|a| !is_actor(a)) {
            return Err(EditError::InvalidActor(actor.to_string()));
        }
        let read = self.reading(target, opts.base)?;
        let over = read.since.as_ref().map(|since| &since.over);
        let fill = |change: &Change| mint::fill(change, &[read.read(), &read.now], over, opts.actor).map_err(EditError::change);
        let mut minted = BTreeMap::new();
        let (change, warnings, diffed) = match edit {
            Edit::Apply(change) => {
                let filled = fill(&change)?;
                minted = filled.minted;
                match u {
                    Some(u) => (units::change_to_scalars(&filled.change, read.read(), u).map_err(EditError::change)?, Vec::new(), false),
                    None => (filled.change, Vec::new(), false),
                }
            }
            Edit::Gesture(gesture) => {
                let gesture = match u {
                    Some(u) => gesture.to_scalars(read.read(), u).map_err(EditError::change)?,
                    None => gesture,
                };
                let filled = fill(&gesture.change(read.read()).map_err(EditError::change)?)?;
                minted = filled.minted;
                (filled.change, Vec::new(), false)
            }
            Edit::Revise(markdown) => {
                let revision = diff::revise_keeping(read.read(), markdown, &read.now).map_err(|e| match e {
                    diff::ReviseError::Import(e) => EditError::import(e),
                    diff::ReviseError::Conflict(c) => {
                        EditError::Base(BaseError::rebase_conflict(
                            read.hash.clone().unwrap_or_else(|| self.hash_now(target, &read.now)),
                            target.field().map(str::to_string),
                            &c,
                        ))
                    }
                })?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&revision.change, read.read(), None, &[read.read(), &read.now], over, actor),
                    None => revision.change,
                };
                (change, revision.warnings, true)
            }
            Edit::Replace(content) => {
                let change = diff::replace_holding(read.read(), &content, &read.now).map_err(|e| EditError::change(e.into()))?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&change, read.read(), Some(&content), &[read.read(), &read.now], over, actor),
                    None => change,
                };
                (change, Vec::new(), true)
            }
        };
        let change = match &read.since {
            None => change,
            Some(since) => read.rebase(target, since, &change)?,
        };
        let mut applied = change::apply(&read.now, &change).map_err(EditError::change)?;
        if diffed {
            keeps_ext_islands(&read.now, &applied.content)?;
            applied.dropped = change::lost_anchors(&read.now, &applied.content, |_| true);
        }
        let shown = match u {
            Some(u) => units::change_from_scalars(&applied.applied, &read.now, u).map_err(EditError::change)?,
            None => applied.applied.clone(),
        };
        let rebased = read.since.is_some();
        let hash = self.land(target, read, applied.content, applied.applied)?;
        Ok(Receipt { applied: shown, hash, dropped_anchors: applied.dropped, moved_anchors: applied.moved, warnings, rebased, minted, counted: Counted::Host })
    }

    /// Run `write`, which stores a new value at `target`, and answer its
    /// receipt: the change from the value before to the value after, in
    /// scalars, logged as a write through [`write`](Self::write) logs its own,
    /// so a later write naming the hash before rebases over it.
    pub(crate) fn revising(
        &mut self,
        target: Target<'_>,
        warnings: Vec<ImportWarning>,
        write: impl FnOnce(&mut Self) -> Result<(), EditError>,
    ) -> Result<Receipt, EditError> {
        let before = self.read(target)?;
        let before_hash = self.hash_now(target, &before);
        write(self)?;
        let after = self.read(target)?;
        let change = diff::diff(&before, &after).map_err(|e| EditError::change(e.into()))?;
        let applied = change::apply(&before, &change).map_err(EditError::change)?;
        let dropped_anchors = change::lost_anchors(&before, &after, |_| true);
        let hash = self.hash_now(target, &after);
        let before = Arc::new(before);
        if hash != before_hash {
            self.log.record(target, before_hash, Content::clone(&before), applied.applied.clone(), hash.clone());
        }
        Ok(Receipt {
            applied: applied.applied,
            hash,
            dropped_anchors,
            moved_anchors: applied.moved,
            warnings,
            rebased: false,
            minted: applied.minted,
            counted: Counted::Content(before),
        })
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
        let Some(base) = base else {
            return Ok(Reading { now, hash: None, since: None });
        };
        let hash = self.hash_now(target, &now);
        let since = match base != hash {
            true => {
                let since = self.log.since(target, base, &hash);
                let stale = || BaseError::Stale { current: hash.clone(), field: target.field().map(str::to_string) };
                Some(since.ok_or_else(|| EditError::Base(stale()))?)
            }
            false => None,
        };
        Ok(Reading { now, hash: Some(hash), since })
    }

    /// The hash of `now`, the value at `target`: the one the last write left
    /// there, or `now` hashed.
    fn hash_now(&self, target: Target<'_>, now: &Content) -> String {
        let held = match target {
            Target::Body => self.hashes.body(),
            Target::Field(name) => self.hashes.field(name),
        };
        held.map_or_else(|| content_hash(now), str::to_string)
    }

    /// Store `content` at `target`, where it differs from the value read,
    /// log `applied` as the change that made it, and answer its hash.
    fn land(&mut self, target: Target<'_>, read: Reading, content: Content, applied: Change) -> Result<String, EditError> {
        let Reading { now: before, hash: before_hash, .. } = read;
        let before_hash = before_hash.unwrap_or_else(|| self.hash_now(target, &before));
        let (hash, value) = match target {
            Target::Body => (content_hash(&content), None),
            Target::Field(_) => {
                let value = content.to_value();
                (super::base::digest(&value), Some(value))
            }
        };
        if hash != before_hash {
            match (target, value) {
                (Target::Field(name), Some(value)) => {
                    self.payload_mut().insert(name.to_string(), QuillValue::from_json(value)).map_err(EditError::InvalidPayload)?;
                }
                _ => self.set_body(content),
            }
            self.log.record(target, before_hash, before, applied, hash.clone());
        }
        self.remember_hash(target.field(), hash.clone());
        Ok(hash)
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

/// The value a write reads now and its hash, and where its base is an earlier
/// hash the log reaches, what changed since.
struct Reading {
    now: Content,
    /// Hashed only where a base asks for it.
    hash: Option<String>,
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
            RebaseError::Apply(e) => EditError::change(e),
            RebaseError::Conflict(c) => EditError::Base(BaseError::rebase_conflict(self.hash.clone().unwrap_or_default(), target.field().map(str::to_string), &c)),
        })
    }
}

/// The steps, nested ones counted, a card's log reaches back for one value.
/// A base rebases while the writes since it hold at most this many steps,
/// and the base the newest write read rebases however many that write
/// holds. The log holds at most twice this many steps, or the newest write
/// alone where it holds more.
pub const LOG_STEPS: usize = 4096;

/// The steps one change a host writes may hold, nested ones counted. Each
/// step is applied, mapped through a rebase and logged on its own, and a
/// rebase maps each over every step before it, so a change past this is
/// refused before any of that runs; a writer splits a larger edit into
/// several changes.
pub const MAX_CHANGE_STEPS: usize = 4 * LOG_STEPS;

/// Refuses a change of more than [`MAX_CHANGE_STEPS`] steps, as
/// [`EditError::TooManySteps`]. Every [`Card::write`] of a change runs it,
/// and the bindings run it on each change they take.
pub fn check_change(change: &Change) -> Result<(), EditError> {
    match steps_in(change) {
        count if count > MAX_CHANGE_STEPS => Err(EditError::TooManySteps { count, max: MAX_CHANGE_STEPS }),
        _ => Ok(()),
    }
}

/// Refuses a content value, as the JSON a host sent, whose text and its
/// slots' texts together pass
/// [`MAX_MARKDOWN_SIZE`](quillmark_content::MAX_MARKDOWN_SIZE) bytes, as
/// [`EditError::InputTooLarge`]: the bound a markdown door holds, before the
/// value is decoded. The bindings run it on each content value they take.
pub fn check_content_value(v: &serde_json::Value) -> Result<(), EditError> {
    fn text_bytes(v: &serde_json::Value) -> usize {
        let own = v.get("text").and_then(serde_json::Value::as_str).map_or(0, str::len);
        let islands: Box<dyn Iterator<Item = &serde_json::Value>> = match v.get("islands") {
            Some(serde_json::Value::Array(list)) => Box::new(list.iter()),
            Some(serde_json::Value::Object(map)) => Box::new(map.values()),
            _ => Box::new(std::iter::empty()),
        };
        islands
            .filter_map(|i| i.get("slots").and_then(serde_json::Value::as_object))
            .flat_map(|slots| slots.values())
            .map(text_bytes)
            .fold(own, usize::saturating_add)
    }
    let max = quillmark_content::MAX_MARKDOWN_SIZE;
    match text_bytes(v) {
        size if size > max => Err(EditError::InputTooLarge { size, max }),
        _ => Ok(()),
    }
}

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
    fn track(&self, target: Target<'_>) -> Option<&Track> {
        let key = target.field();
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
        let key = target.field();
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
