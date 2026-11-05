//! The content write primitive and its producers.
//!
//! [`CardMut::write`](super::CardMut::write) lands an [`Edit`] on a body or a
//! richtext field and answers a [`Receipt`]: a [`Change`], or one built from a
//! structural edit, a writer's markdown or a whole value. Each takes the heads its writer read
//! as [`WriteOptions::heads`]. A write at heads the document has moved on
//! from lowers into the document's Automerge document at those heads, where
//! it merges with what changed since as Automerge merges; heads the
//! document's history does not hold refuse with `edit::stale_base`.

use std::collections::HashSet;

use quillmark_automerge::content::change::{self, Change, Dropped, Step};
use quillmark_automerge::content::structural::StructuralEdit;
use quillmark_automerge::content::lines::import::ImportWarning;
use quillmark_automerge::content::lines::island::IslandType;
use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::repair::Repair;

use super::edit::EditError;
use super::{Card, Heads};

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
    /// The document's heads when the writer read, as `heads()` answered
    /// them. A write at heads the document has moved on from is built on
    /// the value at those heads and lowered there, so it merges with every
    /// change since; heads the document's history does not hold refuse with
    /// `edit::stale_base`. None writes on the value now.
    pub heads: Option<&'a Heads>,
}

impl<'a> From<Option<&'a Heads>> for WriteOptions<'a> {
    /// The options of a write at these heads.
    fn from(heads: Option<&'a Heads>) -> Self {
        WriteOptions { heads }
    }
}

impl<'a> From<&'a Heads> for WriteOptions<'a> {
    /// The options of a write at these heads.
    fn from(heads: &'a Heads) -> Self {
        WriteOptions { heads: Some(heads) }
    }
}

/// How a write builds its change from the value its writer read.
#[derive(Debug, Clone)]
pub enum Edit<'a> {
    /// A change, its offsets into the value the heads name. A record it puts
    /// may carry a [placeholder](quillmark_automerge::content::ids::is_placeholder),
    /// which the write fills; the receipt's patches carry the record's id.
    Apply(Change),
    /// A structural edit's change on the value the heads name, its offsets into it;
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

/// What a write, a merge or a sync message did. A merge's or sync message's
/// `dropped_anchors`, `repairs` and `warnings` are empty; its `patches`
/// carry what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    /// The document's heads before the write; the heads
    /// [`CardMut::undo`](super::CardMut::undo) takes the value back to.
    pub before: Heads,
    /// The document's heads after the write, the next write's `heads`.
    pub heads: Heads,
    /// Automerge's patches for the write, as automerge-js spells them: each
    /// a path from the document's root, text indexes in the writer's units.
    pub patches: Vec<serde_json::Value>,
    /// Each anchor the value held and holds nowhere now, as it read before,
    /// with the slot that held it: every one a revise or replace lost, and
    /// for an apply every one but those its `anchor` steps removed.
    pub dropped_anchors: Vec<Dropped>,
    /// What reading the value after the write repaired, where the write
    /// left it invalid.
    pub repairs: Vec<Repair>,
    /// The markdown import's warnings, for a revise.
    pub warnings: Vec<ImportWarning>,
}

impl Receipt {
    /// The receipt's warnings as diagnostics at `at`: a
    /// [`dropped_construct`](super::dropped_construct) per construct the
    /// markdown dropped.
    pub fn diagnostics(&self, at: Option<&crate::path::DocPath>) -> Vec<crate::error::Diagnostic> {
        super::dropped_constructs(self.warnings.clone(), at)
    }
}

impl Card {
    /// The body's annotated read ([`to_markdown_annotated`]): the scoped read
    /// a body revise reads back. Its anchors carry no path; the body is theirs.
    ///
    /// [`to_markdown_annotated`]: quillmark_automerge::content::markdown::to_markdown_annotated
    pub(crate) fn body_annotated(&self) -> quillmark_automerge::content::lines::export::Annotated {
        quillmark_automerge::content::markdown::to_markdown_annotated(self.body())
    }

    /// The field's annotated read, decoded as a schema-blind revise decodes
    /// it, an absent field the empty content's: the scoped read a field
    /// revise reads back. A field stored under `codec: "plain"` reads as its
    /// literal text, its anchors listed and no tag spelled.
    pub(crate) fn field_annotated(&self, name: &str) -> Result<quillmark_automerge::content::lines::export::Annotated, EditError> {
        Ok(quillmark_automerge::content::markdown::to_text_annotated(&self.richtext_value(name)?))
    }

    /// The value at `target`.
    pub fn read<'t>(&self, target: impl Into<Target<'t>>) -> Result<Content, EditError> {
        match target.into() {
            Target::Body => Ok(self.body().clone()),
            Target::Field(name) => self.richtext_value(name),
        }
    }

    /// The annotated read of the value at `target`: the scoped read a revise
    /// of it reads back. A field stored under `codec: "plain"` reads as its literal
    /// text, its anchors listed and no tag spelled.
    pub fn annotated<'t>(&self, target: impl Into<Target<'t>>) -> Result<quillmark_automerge::content::lines::export::Annotated, EditError> {
        match target.into() {
            Target::Body => Ok(self.body_annotated()),
            Target::Field(name) => self.field_annotated(name),
        }
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
/// [`EditError::TooManySteps`]. Every [`CardMut::write`](super::CardMut::write) of a change runs it,
/// and the bindings run it on each change they take.
pub fn check_change(change: &Change) -> Result<(), EditError> {
    match steps_in(change) {
        count if count > MAX_CHANGE_STEPS => Err(EditError::TooManySteps { count, max: MAX_CHANGE_STEPS }),
        _ => Ok(()),
    }
}

/// Refuses a content value, as the JSON a host sent, whose text and its
/// slots' texts together pass
/// [`MAX_MARKDOWN_SIZE`](quillmark_automerge::content::MAX_MARKDOWN_SIZE) bytes, as
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
    let max = quillmark_automerge::content::MAX_MARKDOWN_SIZE;
    match text_bytes(v) {
        size if size > max => Err(EditError::InputTooLarge { size, max }),
        _ => Ok(()),
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
