//! The content write primitive and its producers.
//!
//! [`Card::write`] lands an [`Edit`] on a body or a richtext field and
//! answers a [`Receipt`]: a [`Change`], or one built from a structural edit, a
//! writer's markdown or a whole value. Each takes the hash its writer read as
//! [`WriteOptions::base`]. Each value a card writes keeps a live Automerge
//! document beside it, and a base the value has moved on from lowers the
//! write at the heads that base was left at and merges it with what changed
//! since. A write that sets a record or island props a change since also
//! set is refused with `edit::rebase_conflict`, and a base no heads reach
//! with `edit::stale_base`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use quillmark_automerge::Held;
use quillmark_automerge::automerge::ChangeHash;
use quillmark_automerge::automerge::sync;
use quillmark_content::change::{self, Change, Dropped, Moved, Step};
use quillmark_content::diff;
use quillmark_content::structural::{StructuralEdit, Wrap};
use quillmark_content::ids::is_actor;
use quillmark_content::mint;
use quillmark_content::lines::import::ImportWarning;
use quillmark_content::lines::island::IslandType;
use quillmark_content::model::Content;
use quillmark_content::units::{self, Units};

use super::base::{Base, BaseError, content_hash};
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

impl<'a> From<&'a str> for Target<'a> {
    fn from(name: &'a str) -> Self {
        Target::Field(name)
    }
}

impl<'a> From<&'a String> for Target<'a> {
    fn from(name: &'a String) -> Self {
        Target::Field(name)
    }
}

impl<'a> From<Option<&'a str>> for Target<'a> {
    /// `None` the body, a name that field.
    fn from(field: Option<&'a str>) -> Self {
        field.map_or(Target::Body, Target::Field)
    }
}

/// What a write reads besides its edit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WriteOptions<'a> {
    /// What the writer read, which a write over a value that has moved on
    /// merges from.
    pub base: Option<Base<'a>>,
    /// Who writes, scoping each id the write mints: `{actor}:{prefix}{n}`
    /// where it is given, `{prefix}{n}` where not. A live client passes one
    /// unique to its session, so two clients writing from one value never
    /// mint one id. One that is not an
    /// [actor](quillmark_content::ids::is_actor) is refused with
    /// `edit::invalid_actor`.
    pub actor: Option<&'a str>,
}

impl<'a> From<Option<&'a str>> for WriteOptions<'a> {
    /// The options of a write with this hash for its base and no actor.
    fn from(base: Option<&'a str>) -> Self {
        WriteOptions { base: base.map(Base::Hash), actor: None }
    }
}

impl<'a> From<Base<'a>> for WriteOptions<'a> {
    /// The options of a write with this base and no actor.
    fn from(base: Base<'a>) -> Self {
        WriteOptions { base: Some(base), actor: None }
    }
}

/// How a write builds its change from the value its writer read.
#[derive(Debug, Clone)]
pub enum Edit<'a> {
    /// A change, its offsets into the value the base names. A record it puts
    /// may carry a [placeholder](quillmark_content::ids::is_placeholder),
    /// which the write fills and names in [`Receipt::minted`].
    Apply(Change),
    /// A structural edit's change on the value the base names, its offsets into it;
    /// the ids it opens are placeholders the write fills.
    Structural(StructuralEdit),
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
    /// merged with what changed since.
    pub rebased: bool,
    /// Each placeholder the change gave a record it put, and the id the
    /// write minted for it past every id the value it landed on and the
    /// value its base named hold.
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
    pub(crate) fn landed(applied: Change, hash: String, dropped_anchors: Vec<Dropped>, moved_anchors: Vec<Moved>, warnings: Vec<ImportWarning>, rebased: bool, minted: BTreeMap<String, String>) -> Receipt {
        Receipt { applied, hash, dropped_anchors, moved_anchors, warnings, rebased, minted, counted: Counted::Host }
    }

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
    pub fn read<'t>(&self, target: impl Into<Target<'t>>) -> Result<Content, EditError> {
        match target.into() {
            Target::Body => Ok(self.body().clone()),
            Target::Field(name) => self.richtext_value(name),
        }
    }

    /// The hash of the value at `target`, the `base` a write to it names.
    pub fn hash_of<'t>(&self, target: impl Into<Target<'t>>) -> Result<String, EditError> {
        match target.into() {
            Target::Body => Ok(self.body_hash()),
            Target::Field(name) => self.field_hash(name),
        }
    }

    /// The annotated read of the value at `target`: the scoped read a revise
    /// of it reads back. A field stored under `codec: "plain"` reads as its literal
    /// text, its anchors listed and no tag spelled.
    pub fn annotated<'t>(&self, target: impl Into<Target<'t>>) -> Result<quillmark_content::lines::export::Annotated, EditError> {
        match target.into() {
            Target::Body => Ok(self.body_annotated()),
            Target::Field(name) => self.field_annotated(name),
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
    /// [`StructuralEdit::SplitBlock`], the new block `id` or one the write mints.
    pub fn split_block<'o>(&mut self, target: Target<'_>, at: usize, id: Option<&str>, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::SplitBlock { at, id: id.map(str::to_string) }), opts)
    }

    /// Backspace at the start of block `id`: [`write`](Self::write) of
    /// [`StructuralEdit::JoinBlocks`].
    pub fn join_blocks<'o>(&mut self, target: Target<'_>, id: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::JoinBlocks { id: id.to_string() }), opts)
    }

    /// Deleting `[start, end)`: [`write`](Self::write) of
    /// [`StructuralEdit::DeleteRange`].
    pub fn delete_range<'o>(&mut self, target: Target<'_>, start: usize, end: usize, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::DeleteRange { start, end }), opts)
    }

    /// Tab over the blocks `first` to `last`: [`write`](Self::write) of
    /// [`StructuralEdit::Indent`].
    pub fn indent<'o>(&mut self, target: Target<'_>, first: &str, last: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::Indent { first: first.to_string(), last: last.to_string() }), opts)
    }

    /// Shift-Tab over the blocks `first` to `last`: [`write`](Self::write)
    /// of [`StructuralEdit::Outdent`].
    pub fn outdent<'o>(&mut self, target: Target<'_>, first: &str, last: &str, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::Outdent { first: first.to_string(), last: last.to_string() }), opts)
    }

    /// Wrapping the blocks `first` to `last`: [`write`](Self::write) of
    /// [`StructuralEdit::Wrap`].
    pub fn wrap<'o>(&mut self, target: Target<'_>, first: &str, last: &str, with: Wrap, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write(target, Edit::Structural(StructuralEdit::Wrap { first: first.to_string(), last: last.to_string(), with }), opts)
    }

    /// The block holding offset `at` of the value at `target`, its index and
    /// its container path; at a block's end, that block. `Stale` where `at`
    /// is past the text.
    pub fn block_at(&self, target: Target<'_>, at: usize) -> Result<Located, EditError> {
        let content = self.read(target)?;
        if at > content.len_scalars() {
            return Err(EditError::change(change::ApplyError::Stale { step: 0 }));
        }
        let index = content.block_at(at);
        let block = &content.blocks[index];
        Ok(Located { id: block.id.clone(), index, path: block.path.clone() })
    }

    /// Land `edit` on the value at `target`, all of it or none. A base the
    /// value has moved on from merges the edit's change, made on the value
    /// the base names, with what changed since; a revise or replace diffs
    /// from the value the base names, so what changed since is merged with
    /// rather than undone, and is refused where it would drop an `x-` island.
    /// Each id it mints is minted under the options' actor where one is
    /// given, and the live document takes the write under that actor.
    pub fn write<'o>(&mut self, target: Target<'_>, edit: Edit<'_>, opts: impl Into<WriteOptions<'o>>) -> Result<Receipt, EditError> {
        self.write_with(target, edit, opts.into(), None::<units::Scalars>)
    }

    /// [`write`](Self::write) for a host counting offsets in `u`: an
    /// [`Edit::Apply`]'s change, an [`Edit::Structural`]'s offsets and the
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
        let read = self.reading(target, opts.base, opts.actor)?;
        let fill = |change: &Change| mint::fill(change, &[read.read(), &read.now], opts.actor).map_err(EditError::change);
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
            Edit::Structural(edit) => {
                let edit = match u {
                    Some(u) => edit.to_scalars(read.read(), u).map_err(EditError::change)?,
                    None => edit,
                };
                let filled = fill(&edit.change(read.read()).map_err(EditError::change)?)?;
                minted = filled.minted;
                (filled.change, Vec::new(), false)
            }
            Edit::Revise(markdown) => {
                let revision = quillmark_automerge::revise_keeping(read.read(), markdown, &read.now).map_err(|e| match e {
                    diff::ReviseError::Import(e) => EditError::import(e),
                    diff::ReviseError::Refused(e) => EditError::change(e),
                })?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&revision.change, read.read(), None, &[read.read(), &read.now], actor),
                    None => revision.change,
                };
                (change, revision.warnings, true)
            }
            Edit::Replace(content) => {
                let change = diff::replace_holding(read.read(), &content, &read.now).map_err(|e| EditError::change(e.into()))?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&change, read.read(), Some(&content), &[read.read(), &read.now], actor),
                    None => change,
                };
                (change, Vec::new(), true)
            }
        };
        let Some(since) = &read.since else {
            let (mut applied, trace) = change::apply_traced(&read.now, &change).map_err(EditError::change)?;
            if diffed {
                keeps_ext_islands(&read.now, &applied.content)?;
                applied.dropped = change::lost_anchors(&read.now, &applied.content, |_| true);
            }
            let shown = match u {
                Some(u) => units::change_from_scalars(&applied.applied, &read.now, u).map_err(EditError::change)?,
                None => applied.applied.clone(),
            };
            let hash = self.land(target, read, applied.content, Lowering::Trace(&trace), opts.actor)?;
            return Ok(Receipt { applied: shown, hash, dropped_anchors: applied.dropped, moved_anchors: applied.moved, warnings, rebased: false, minted, counted: Counted::Host });
        };
        let current = read.hash.clone().unwrap_or_default();
        if let Some(c) = quillmark_automerge::conflict(since.base(), &read.now, &change) {
            return Err(EditError::Base(BaseError::rebase_conflict(current, target.field().map(str::to_string), &c)));
        }
        let merge = self.merged(target, &read, &change, opts.actor)?;
        let landed = (|| {
            let to_merge = diff::diff(&read.now, &merge.content).map_err(|e| EditError::change(e.into()))?;
            let (applied, trace) = change::apply_traced(&read.now, &to_merge).map_err(EditError::change)?;
            if diffed {
                keeps_ext_islands(&read.now, &applied.content)?;
            }
            let shown = match u {
                Some(u) => units::change_from_scalars(&applied.applied, &read.now, u).map_err(EditError::change)?,
                None => applied.applied.clone(),
            };
            Ok((applied, trace, shown))
        })();
        let (applied, trace, shown) = landed?;
        let dropped = change::lost_anchors(&read.now, &applied.content, |id| diffed || !merge.removed.contains(id));
        let mut moved = applied.moved;
        for m in change::moved_between(&merge.mine, &applied.content, &merge.put) {
            if !moved.iter().any(|n| n.id == m.id) {
                moved.push(m);
            }
        }
        moved.sort_by(|a, b| a.id.cmp(&b.id));
        let lowering = match merge.held {
            None => Lowering::Trace(&trace),
            Some((held, repaired)) => Lowering::Merged { held, repaired, sync: applied.content != merge.content },
        };
        let hash = self.land(target, read, applied.content, lowering, opts.actor)?;
        Ok(Receipt { applied: shown, hash, dropped_anchors: dropped, moved_anchors: moved, warnings, rebased: true, minted, counted: Counted::Host })
    }

    /// `change`, made from the value its base names, merged with what
    /// changed since: lowered on the fork of the live document at the heads
    /// the base was left at and merged into a copy of the document, the
    /// anchors and marks it put held to their edges ([`Held::clip`]), a
    /// repair the merge needs written back under `actor` where none of it
    /// inserts; for a base the live document never held, merged with the
    /// diff from it to the value now. Refused where a step names a record by
    /// an id minted again since, or the merge deleted the text an anchor it
    /// put sits on; the live document is untouched until the write lands.
    fn merged(&mut self, target: Target<'_>, read: &Reading, change: &Change, actor: Option<&str>) -> Result<Merged, EditError> {
        let since = read.since.as_ref().expect("a merge reads an earlier value");
        let (mine, trace) = change::apply_traced(since.base(), change).map_err(EditError::change)?;
        let removed: HashSet<String> = change::lost_anchors(since.base(), &mine.content, |_| true)
            .into_iter()
            .map(|d| d.context.id)
            .filter(|id| !mine.dropped.iter().any(|d| &d.context.id == id))
            .collect();
        let hash = read.hash.as_deref().unwrap_or_default();
        let refuse = |c: quillmark_automerge::Conflict| EditError::Base(BaseError::rebase_conflict(hash.to_string(), target.field().map(str::to_string), &c));
        if let Some(c) = quillmark_automerge::resolved(&mine.applied, since.base(), &read.now) {
            return Err(refuse(c));
        }
        let (content, held) = match since {
            Since::Heads { fork, .. } => {
                let since_fork: &Held = fork;
                let live = self.live.at(&address(self.id(), target), target, &read.now, hash);
                if let Some(c) = quillmark_automerge::reminted(fork, &live.held, &mine.applied) {
                    return Err(refuse(c));
                }
                let mut fork = fork.as_ref().clone();
                fork.write(&trace, &mine.content);
                if let Some(c) = Held::restyled(since_fork, &fork, &live.held, &trace, &mine.applied) {
                    return Err(refuse(c));
                }
                let (ours, theirs) = (fork.heads(), live.held.heads());
                let mut next = live.held.clone();
                next.merge(&mut fork).expect("a fork merges back into the document it forked from");
                next.set_actor(actor);
                next.clip(&ours, &theirs, &trace);
                let merged = next.read();
                let repaired = merged.repairs.iter().any(|r| r.inserts());
                if !repaired && !merged.repairs.is_empty() {
                    next.sync_to(&merged.content);
                }
                (merged.content, Some((Box::new(next), repaired)))
            }
            Since::Content { base } => {
                let theirs = diff::diff(base, &read.now).map_err(|e| EditError::change(e.into()))?;
                let (merged, restyled) = quillmark_automerge::merge3_checked(base, change, &theirs).map_err(EditError::change)?;
                if let Some(c) = restyled {
                    return Err(refuse(c));
                }
                (merged.content, None)
            }
        };
        if let Some(c) = quillmark_automerge::deleted(&mine.applied, &mine.content, &content) {
            return Err(refuse(c));
        }
        let put = anchors_put(&mine.applied.steps);
        Ok(Merged { content, removed, held, mine: mine.content, put })
    }

    /// [`revise`](Self::revise) with no base, its receipt counted in scalars
    /// on the value it read, which [`Receipt::in_units`] converts.
    pub(crate) fn revise_counted(&mut self, target: Target<'_>, markdown: &str) -> Result<Receipt, EditError> {
        self.revise_counted_from(target, markdown, None)
    }

    /// [`revise_counted`](Self::revise_counted) under `base`, rebased over
    /// the change since where the value has moved on.
    pub(crate) fn revise_counted_from(&mut self, target: Target<'_>, markdown: &str, base: Option<Base<'_>>) -> Result<Receipt, EditError> {
        let before = self.read(target)?;
        let mut receipt = self.write(target, Edit::Revise(markdown), WriteOptions { base, actor: None })?;
        receipt.counted = Counted::Content(Arc::new(before));
        Ok(receipt)
    }

    /// Run `write`, which stores a new value at `target`, and answer its
    /// receipt: the change from the value before to the value after, in
    /// scalars, lowered into the live document as a write through
    /// [`write`](Self::write) lowers its own, so a later write naming the
    /// hash before merges with it.
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
        let (applied, trace) = change::apply_traced(&before, &change).map_err(EditError::change)?;
        let dropped_anchors = change::lost_anchors(&before, &after, |_| true);
        let hash = self.hash_now(target, &after);
        if hash != before_hash {
            let live = self.live.at(&address(self.id(), target), target, &before, &before_hash);
            live.held.set_actor(None);
            live.held.write(&trace, &after);
            live.wrote(hash.clone());
        }
        let before = Arc::new(before);
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

    /// The value the hash `base` names at `target`: the value now, where
    /// `base` is absent or its hash, or the value the live document reads as
    /// at the heads a write left that hash at. Refused with
    /// `edit::stale_base` where no heads reach a value that hashes to `base`.
    pub fn read_at(&mut self, target: Target<'_>, base: Option<&str>) -> Result<Content, EditError> {
        let read = self.reading(target, base.map(Base::Hash), None)?;
        Ok(match read.since {
            Some(Since::Heads { base, .. } | Since::Content { base }) => base,
            None => read.now,
        })
    }

    /// The value a write under `base` by `actor` reads now, and the earlier
    /// value its base names, with the fork the write lowers on.
    fn reading(&mut self, target: Target<'_>, base: Option<Base<'_>>, actor: Option<&str>) -> Result<Reading, EditError> {
        let now = self.read(target)?;
        let Some(base) = base else {
            return Ok(Reading { now, hash: None, since: None });
        };
        let hash = self.hash_now(target, &now);
        let since = match base {
            Base::Hash(read) if read == hash => None,
            Base::Hash(read) => {
                let since = self.live.read_at(&address(self.id(), target), target, &now, &hash, read, actor);
                let stale = || BaseError::Stale { current: hash.clone(), field: target.field().map(str::to_string) };
                Some(since.ok_or_else(|| EditError::Base(stale()))?)
            }
            Base::Content(read) if *read == now => None,
            Base::Content(read) => Some(
                self.live
                    .read_at(&address(self.id(), target), target, &now, &hash, &content_hash(read), actor)
                    .unwrap_or_else(|| Since::Content { base: read.clone() }),
            ),
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
    /// bring the live document to it as `lowering` says, and answer its hash.
    fn land(&mut self, target: Target<'_>, read: Reading, content: Content, lowering: Lowering<'_>, actor: Option<&str>) -> Result<String, EditError> {
        let Reading { now: before, hash: before_hash, .. } = read;
        let before_hash = before_hash.unwrap_or_else(|| self.hash_now(target, &before));
        let (hash, value) = match target {
            Target::Body => (content_hash(&content), None),
            Target::Field(_) => {
                let value = content.to_value();
                (super::base::digest(&value), Some(value))
            }
        };
        let live = self.live.at(&address(self.id(), target), target, &before, &before_hash);
        live.held.set_actor(actor);
        match lowering {
            Lowering::Trace(trace) if !live.repaired => live.held.write(trace, &content),
            Lowering::Trace(_) | Lowering::Sync => {
                live.held.sync_to(&content);
                live.repaired = false;
            }
            Lowering::Done => {}
            Lowering::Merged { held, repaired, sync } => {
                live.held = *held;
                live.repaired = repaired;
                if sync {
                    live.held.set_actor(actor);
                    live.held.sync_to(&content);
                    live.repaired = false;
                }
            }
        }
        live.wrote(hash.clone());
        if hash != before_hash {
            match (target, value) {
                (Target::Field(name), Some(value)) => {
                    if let Err(e) = self.payload_mut().insert(name.to_string(), QuillValue::from_json(value)) {
                        self.live.forget(target, &before, &before_hash);
                        return Err(EditError::InvalidPayload(e));
                    }
                }
                _ => self.set_body(content),
            }
        }
        self.remember_hash(target.field(), hash.clone());
        Ok(hash)
    }

    /// The live document of the value at `target`, as Automerge bytes: made
    /// from the value where no write has made it yet.
    pub fn save(&mut self, target: Target<'_>) -> Result<Vec<u8>, EditError> {
        let now = self.read(target)?;
        let hash = self.hash_now(target, &now);
        Ok(self.live.at(&address(self.id(), target), target, &now, &hash).held.save())
    }

    /// Replace the live document of the value at `target` with the one
    /// `bytes` hold, and store what it reads as there: the receipt of a
    /// write from the value before. The document keeps the history the bytes
    /// hold, for a sync to read, but notes no heads before the load, so a
    /// base read before it is stale; a repair its merged state needs is
    /// written back.
    pub fn load(&mut self, target: Target<'_>, bytes: &[u8]) -> Result<Receipt, EditError> {
        let (held, read) = Held::load(bytes, &address(self.id(), target)).map_err(EditError::bad_live_bytes)?;
        self.taking(target, |live| {
            live.held = held;
            live.heads.clear();
            live.noted.clear();
            Ok(read)
        })
    }

    /// The next message of the Automerge sync protocol from the live
    /// document at `target` to the peer `state` tracks, `None` where the
    /// peer holds everything.
    pub fn generate_sync_message(&mut self, target: Target<'_>, state: &mut sync::State) -> Result<Option<sync::Message>, EditError> {
        let now = self.read(target)?;
        let hash = self.hash_now(target, &now);
        Ok(self.live.at(&address(self.id(), target), target, &now, &hash).held.generate_sync_message(state))
    }

    /// Take a message of the Automerge sync protocol from the peer `state`
    /// tracks into the live document at `target`, and store what it reads as
    /// then: the receipt of a write from the value before.
    pub fn receive_sync_message(&mut self, target: Target<'_>, state: &mut sync::State, message: sync::Message) -> Result<Receipt, EditError> {
        self.taking(target, |live| live.held.receive_sync_message(state, message).map_err(EditError::bad_live_bytes))
    }

    /// Run `take` on the live document at `target`, which answers what the
    /// document reads as then, and store that, a repair none of which
    /// inserts written back.
    fn taking(&mut self, target: Target<'_>, take: impl FnOnce(&mut Live) -> Result<quillmark_automerge::Read, EditError>) -> Result<Receipt, EditError> {
        let before = self.read(target)?;
        let before_hash = self.hash_now(target, &before);
        let live = self.live.at(&address(self.id(), target), target, &before, &before_hash);
        let read = take(live)?;
        live.write_back(&read, None);
        let reading = Reading { now: before.clone(), hash: Some(before_hash), since: None };
        let change = diff::diff(&before, &read.content).map_err(|e| EditError::change(e.into()))?;
        let applied = change::apply(&before, &change).map_err(EditError::change)?;
        let lowering = if applied.content == read.content { Lowering::Done } else { Lowering::Sync };
        let dropped_anchors = change::lost_anchors(&before, &applied.content, |_| true);
        let hash = self.land(target, reading, applied.content, lowering, None)?;
        Ok(Receipt {
            applied: applied.applied,
            hash,
            dropped_anchors,
            moved_anchors: applied.moved,
            warnings: Vec::new(),
            rebased: false,
            minted: BTreeMap::new(),
            counted: Counted::Content(Arc::new(before)),
        })
    }
}

/// Refuses a revise or replace whose result no longer holds an `x-` island
/// `before` holds, at any depth.
pub(crate) fn keeps_ext_islands(before: &Content, after: &Content) -> Result<(), EditError> {
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

/// The value a write reads now and its hash, and where its base names an
/// earlier value, that value.
struct Reading {
    now: Content,
    /// Hashed only where a base asks for it.
    hash: Option<String>,
    since: Option<Since>,
}

/// The earlier value a base names.
enum Since {
    /// Read on the fork of the live document at the heads a write left its
    /// hash at, which the write lowers on.
    Heads { base: Content, fork: Box<Held> },
    /// A value the live document never held, which a base gave whole.
    Content { base: Content },
}

impl Since {
    fn base(&self) -> &Content {
        match self {
            Since::Heads { base, .. } | Since::Content { base } => base,
        }
    }
}

impl Reading {
    /// The value the writer read.
    fn read(&self) -> &Content {
        self.since.as_ref().map_or(&self.now, Since::base)
    }
}

/// A write's change merged with what changed since its base.
struct Merged {
    content: Content,
    /// Each anchor the write's own `anchor` steps removed.
    removed: HashSet<String>,
    /// The live document holding the merge, and whether it reads through a
    /// repair it does not hold, where the write lowered on a fork.
    held: Option<(Box<Held>, bool)>,
    /// The value the write's own change gave, and each anchor its `anchor`
    /// steps placed or moved.
    mine: Content,
    put: HashSet<String>,
}

/// Each anchor an `anchor` step of `steps` placed or moved, at any depth.
fn anchors_put(steps: &[Step]) -> HashSet<String> {
    let mut out = HashSet::new();
    for step in steps {
        match step {
            Step::Anchor(change::AnchorStep::Place { anchor, .. } | change::AnchorStep::Move { anchor, .. }) => {
                out.insert(anchor.id.clone());
            }
            Step::Nested { change, .. } => out.extend(anchors_put(&change.steps)),
            _ => {}
        }
    }
    out
}

/// How [`Card::land`] brings the live document to the value it stores.
enum Lowering<'a> {
    /// The change's trace, lowered from the value read.
    Trace(&'a [change::Trace]),
    /// A diff from what the document reads as.
    Sync,
    /// The document reads as it already.
    Done,
    /// The document a merge left, which replaces the live one, then a diff
    /// from what it reads as where `sync` says the value differs.
    Merged { held: Box<Held>, repaired: bool, sync: bool },
}

/// The steps one change a host writes may hold, nested ones counted. Each
/// step is applied and lowered into the live document on its own, so a
/// change past this is refused before any of that runs; a writer splits a
/// larger edit into several changes.
pub const MAX_CHANGE_STEPS: usize = 16384;

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

/// Each value's live Automerge document, made on the first write to it, in
/// memory only: no store, hash or comparison sees it. A document loaded from
/// storage has none, so a base it names that is not the value now reaches no
/// heads.
#[derive(Clone, Default)]
pub(crate) struct Values {
    values: Vec<(Option<String>, Live)>,
}

/// The address of the value at `target` on the card `card` names, `None`
/// for the main card: the message of its live document's first change,
/// spelled as the path with the card's `$id` in place of its index,
/// `main.body`, `main.summary`, `cards[n1].body`, `cards[n1].summary`.
pub(crate) fn address(card: Option<&str>, target: Target<'_>) -> String {
    let card = card.map_or_else(|| "main".to_string(), |id| format!("cards[{id}]"));
    format!("{card}.{}", target.field().unwrap_or("body"))
}

/// One value's live document, the address it was made at, the hash of the
/// value it reads as, the heads at which each of the last [`NOTED_HASHES`]
/// hashes a write left reads, the latest write's for a hash two writes left,
/// in the order they were first noted, whether the document reads
/// through a repair it does not hold, one that inserts, so a write lowers
/// into it as a diff rather than at its trace's offsets.
#[derive(Clone)]
pub(crate) struct Live {
    held: Held,
    address: String,
    hash: String,
    heads: HashMap<String, Vec<ChangeHash>>,
    noted: std::collections::VecDeque<String>,
    repaired: bool,
}

/// The hashes a live document notes heads for, the latest kept: a base
/// older than these is stale.
const NOTED_HASHES: usize = 1024;

impl PartialEq for Values {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Values {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Values")
    }
}

impl Live {
    /// Write back what repair made of the merged state `read`, under
    /// `actor`, where it repaired anything and none of it inserts; a repair
    /// that inserts stays unwritten, the document reading through it.
    fn write_back(&mut self, read: &quillmark_automerge::Read, actor: Option<&str>) {
        if read.repairs.iter().any(|r| r.inserts()) {
            self.repaired = true;
        } else if !read.repairs.is_empty() {
            self.held.set_actor(actor);
            self.held.sync_to(&read.content);
        }
    }

    /// Note the heads the document is at as where `hash` reads.
    fn wrote(&mut self, hash: String) {
        if self.heads.insert(hash.clone(), self.held.heads()).is_none() {
            self.noted.push_back(hash.clone());
            if self.noted.len() > NOTED_HASHES {
                let oldest = self.noted.pop_front().expect("a full queue holds a hash");
                self.heads.remove(&oldest);
            }
        }
        self.hash = hash;
    }
}

impl Values {
    fn position(&self, target: Target<'_>) -> Option<usize> {
        let key = target.field();
        self.values.iter().position(|(k, _)| k.as_deref() == key)
    }

    /// The live document of `target`, reading as `now`, which hashes to
    /// `hash`: made from it where none is, and brought to it by a diff where
    /// a write the document did not see moved the value on.
    /// A document made at `address` replaces one made at another, as when a
    /// card takes its `$id`.
    fn at(&mut self, address: &str, target: Target<'_>, now: &Content, hash: &str) -> &mut Live {
        let fresh = || {
            let mut live = Live { held: Held::new(now, address), address: address.to_string(), hash: String::new(), heads: HashMap::new(), noted: Default::default(), repaired: false };
            live.wrote(hash.to_string());
            live
        };
        let k = match self.position(target) {
            Some(k) => {
                if self.values[k].1.address != address {
                    self.values[k].1 = fresh();
                }
                k
            }
            None => {
                self.values.push((target.field().map(str::to_string), fresh()));
                self.values.len() - 1
            }
        };
        let live = &mut self.values[k].1;
        if live.hash != hash {
            live.held.set_actor(None);
            live.held.sync_to(now);
            live.repaired = false;
            live.wrote(hash.to_string());
        }
        live
    }

    /// The value `base` names at `target`, read on a fork for `actor` at
    /// the heads a write left it at, where it hashes to `base` there.
    fn read_at(&mut self, address: &str, target: Target<'_>, now: &Content, hash: &str, base: &str, actor: Option<&str>) -> Option<Since> {
        self.position(target)?;
        let live = self.at(address, target, now, hash);
        let heads = live.heads.get(base)?.clone();
        let mut fork = live.held.fork_at(&heads, actor).ok()?;
        let read = fork.read();
        if read.repairs.iter().any(|r| r.inserts()) {
            fork.sync_to(&read.content);
        }
        (super::base::content_hash(&read.content) == base).then(|| Since::Heads { base: read.content, fork: Box::new(fork) })
    }

    /// Bring the live document of `target` back to `now` after a write that
    /// lowered into it and then failed.
    fn forget(&mut self, target: Target<'_>, now: &Content, hash: &str) {
        if let Some(k) = self.position(target) {
            let live = &mut self.values[k].1;
            live.held.set_actor(None);
            live.held.sync_to(now);
            live.repaired = false;
            live.wrote(hash.to_string());
        }
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
