//! The content write primitive and its producers.
//!
//! [`Card::write`] lands an [`Edit`] on a body or a richtext field and
//! answers a [`Receipt`]: a [`Change`], or one built from a structural edit, a
//! writer's markdown or a whole value. Each takes the hash its writer read as
//! [`WriteOptions::base`]. A base the value has moved on from lowers the
//! write into the document's Automerge document at the heads that base was
//! left at, where it merges with what changed since as Automerge merges; a
//! base the history does not reach is refused with `edit::stale_base`.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

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
use super::{Card, Heads};
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
    /// Who writes: in a document, the Automerge actor the write writes
    /// under, each id it mints an object id under it; on a card outside one,
    /// the scope of each id it mints, `{actor}:{prefix}{n}`. A live client
    /// passes one unique to its session, so two clients writing from one
    /// value never mint one id. One that is not an
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
    /// The document's heads after the write, empty for a card no document
    /// holds.
    pub heads: Heads,
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
    /// The receipt counted in scalars on `before`, which
    /// [`in_units`](Self::in_units) converts.
    pub(crate) fn counted_on(mut self, before: Content) -> Receipt {
        self.counted = Counted::Content(Arc::new(before));
        self
    }

    /// The receipt of a write that left the document at `heads`.
    pub(crate) fn at(mut self, heads: Heads) -> Receipt {
        self.heads = heads;
        self
    }

    pub(crate) fn landed(applied: Change, hash: String, dropped_anchors: Vec<Dropped>, moved_anchors: Vec<Moved>, warnings: Vec<ImportWarning>, rebased: bool, minted: BTreeMap<String, String>) -> Receipt {
        Receipt { applied, hash, heads: Heads::default(), dropped_anchors, moved_anchors, warnings, rebased, minted, counted: Counted::Host }
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

    /// Land `edit` on the value at `target`, all of it or none, the
    /// writer's base the value now: a card not yet placed in a document
    /// keeps no history, so a base it has moved on from refuses with
    /// `edit::stale_base`. [`CardMut::write`](super::CardMut::write) is the
    /// write of a placed card, which merges.
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
        let now = self.read_at(target, opts.base.map(|b| match b {
            Base::Hash(h) => h.to_string(),
            Base::Content(c) => content_hash(c),
        }).as_deref())?;
        let fill = |change: &Change| mint::fill(change, &[&now], opts.actor).map_err(EditError::change);
        let mut minted = BTreeMap::new();
        let (change, warnings, diffed) = match edit {
            Edit::Apply(change) => {
                let filled = fill(&change)?;
                minted = filled.minted;
                match u {
                    Some(u) => (units::change_to_scalars(&filled.change, &now, u).map_err(EditError::change)?, Vec::new(), false),
                    None => (filled.change, Vec::new(), false),
                }
            }
            Edit::Structural(edit) => {
                let edit = match u {
                    Some(u) => edit.to_scalars(&now, u).map_err(EditError::change)?,
                    None => edit,
                };
                let filled = fill(&edit.change(&now).map_err(EditError::change)?)?;
                minted = filled.minted;
                (filled.change, Vec::new(), false)
            }
            Edit::Revise(markdown) => {
                let revision = quillmark_automerge::revise(&now, markdown).map_err(|e| match e {
                    diff::ReviseError::Import(e) => EditError::import(e),
                    diff::ReviseError::Refused(e) => EditError::change(e),
                })?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&revision.change, &now, None, &[&now], actor),
                    None => revision.change,
                };
                (change, revision.warnings, true)
            }
            Edit::Replace(content) => {
                let change = diff::replace(&now, &content).map_err(|e| EditError::change(e.into()))?;
                let change = match opts.actor {
                    Some(actor) => mint::remint(&change, &now, Some(&content), &[&now], actor),
                    None => change,
                };
                (change, Vec::new(), true)
            }
        };
        let mut applied = change::apply(&now, &change).map_err(EditError::change)?;
        if diffed {
            keeps_ext_islands(&now, &applied.content)?;
            applied.dropped = change::lost_anchors(&now, &applied.content, |_| true);
        }
        let shown = match u {
            Some(u) => units::change_from_scalars(&applied.applied, &now, u).map_err(EditError::change)?,
            None => applied.applied.clone(),
        };
        let hash = self.store(target, &now, applied.content)?;
        Ok(Receipt { applied: shown, hash, heads: Heads::default(), dropped_anchors: applied.dropped, moved_anchors: applied.moved, warnings, rebased: false, minted, counted: Counted::Host })
    }

    /// [`revise`](Self::revise) with no base, its receipt counted in scalars
    /// on the value it read, which [`Receipt::in_units`] converts.
    pub(crate) fn revise_counted(&mut self, target: Target<'_>, markdown: &str) -> Result<Receipt, EditError> {
        let before = self.read(target)?;
        Ok(self.write(target, Edit::Revise(markdown), WriteOptions::default())?.counted_on(before))
    }

    /// Run `write`, which stores a new value at `target`, and answer its
    /// receipt: the change from the value before to the value after, in
    /// scalars.
    pub(crate) fn revising(
        &mut self,
        target: Target<'_>,
        warnings: Vec<ImportWarning>,
        write: impl FnOnce(&mut Self) -> Result<(), EditError>,
    ) -> Result<Receipt, EditError> {
        let before = self.read(target)?;
        write(self)?;
        let after = self.read(target)?;
        let change = diff::diff(&before, &after).map_err(|e| EditError::change(e.into()))?;
        let applied = change::apply(&before, &change).map_err(EditError::change)?;
        let dropped_anchors = change::lost_anchors(&before, &after, |_| true);
        Ok(Receipt {
            applied: applied.applied,
            hash: content_hash(&after),
            heads: Heads::default(),
            dropped_anchors,
            moved_anchors: applied.moved,
            warnings,
            rebased: false,
            minted: applied.minted,
            counted: Counted::Content(Arc::new(before)),
        })
    }

    /// The value at `target` as the writer that read `base` read it: the
    /// value now, where `base` is absent or its hash, and refused with
    /// `edit::stale_base` otherwise.
    pub fn read_at(&self, target: Target<'_>, base: Option<&str>) -> Result<Content, EditError> {
        let now = self.read(target)?;
        match base {
            Some(read) if read != content_hash(&now) => Err(EditError::Base(BaseError::Stale { current: content_hash(&now), field: target.field().map(str::to_string) })),
            _ => Ok(now),
        }
    }

    /// Store `content` at `target`, where it differs from `before`, and
    /// answer its hash.
    fn store(&mut self, target: Target<'_>, before: &Content, content: Content) -> Result<String, EditError> {
        let hash = content_hash(&content);
        if content != *before {
            match target {
                Target::Field(name) => self.payload_mut().insert(name.to_string(), QuillValue::from_json(content.to_value())).map(drop).map_err(EditError::InvalidPayload)?,
                Target::Body => self.set_body(content),
            }
        }
        Ok(hash)
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

/// The steps one change a host writes may hold, nested ones counted. Each
/// step is applied and lowered into the document on its own, so a
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

/// The address of the value at `target` on the card `card` names, `None`
/// for the main card, spelled as the path with the card's `$id` in place of
/// its index, `main.body`, `main.summary`, `cards[n1].body`,
/// `cards[n1].summary`: the key a base hash is noted under.
pub(crate) fn address(card: Option<&str>, target: Target<'_>) -> String {
    let card = card.map_or_else(|| "main".to_string(), |id| format!("cards[{id}]"));
    format!("{card}.{}", target.field().unwrap_or("body"))
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
