//! A Quillmark content value held in an Automerge document.
//!
//! [`Held`] keeps one value under the root key [`KEY`] in the shape the
//! `map` module states, takes each [`Change`] as the Automerge operations its
//! [`Trace`] lowers to, and reads back the raw value [`repair`] takes, so
//! every peer holding one merged state reads one valid value. A write made
//! from earlier heads lowers on [`Held::fork_at`] and merges back. [`merge3`]
//! merges two changes made from one value, and [`revise_keeping`] is the
//! markdown revise that keeps what its read does not spell by merging the
//! writer's revision with what the read lost.

mod lower;
mod map;
mod merge;

pub use automerge;
pub use merge::{Conflict, Reason, conflict, merge3, revise, revise_keeping};

use automerge::sync::{self, SyncDoc};
use std::collections::HashSet;

use automerge::{ActorId, AutoCommit, AutomergeError, ChangeHash, LoadOptions, ObjId, ObjType, ROOT, ReadDoc, TextEncoding};
use quillmark_content::change::{Change, Trace};
use quillmark_content::model::Content;
use quillmark_content::repair::{Raw, Repair, repair};

/// The root key a value sits under.
pub const KEY: &str = "value";

/// The actor a write with none of its own writes under: a byte no actor
/// spells.
const NO_ACTOR: &[u8] = b"~";

/// The byte between an actor's id and its fork's level in a fork's id.
const FORK: &[u8] = b"\x1f";

/// The Automerge actor of a write's actor: its bytes, or for a write with no
/// actor one byte no actor spells.
pub fn actor_id(actor: Option<&str>) -> ActorId {
    ActorId::from(actor.map_or(NO_ACTOR, str::as_bytes))
}

/// A value held in an Automerge document.
#[derive(Debug, Clone)]
pub struct Held {
    doc: AutoCommit,
    root: ObjId,
}

/// The value a document reads as: what repair made of its merged state, what
/// it changed, and the change from the merged text to the value's.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub content: Content,
    pub repairs: Vec<Repair>,
    pub change: Change,
}

/// Bytes that do not load as a held value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error("the bytes are not an Automerge document: {0}")]
    Automerge(String),
    #[error("the document holds no content value under `{KEY}`")]
    NoValue,
}

impl Held {
    /// A new document holding `content`, written in one change under no
    /// actor.
    pub fn new(content: &Content) -> Held {
        let mut doc = AutoCommit::new_with_encoding(TextEncoding::UnicodeCodePoint).with_actor(actor_id(None));
        let root = map::create(&mut doc, &ROOT, KEY, content);
        doc.commit();
        Held { doc, root }
    }

    /// The value a document's bytes hold.
    pub fn load(bytes: &[u8]) -> Result<Held, LoadError> {
        let options = LoadOptions::new().text_encoding(TextEncoding::UnicodeCodePoint);
        let doc = AutoCommit::load_with_options(bytes, options).map_err(|e| LoadError::Automerge(e.to_string()))?;
        let mut held = Held { root: ROOT, doc: doc.with_actor(actor_id(None)) };
        held.root = held.value_root().ok_or(LoadError::NoValue)?;
        Ok(held)
    }

    fn value_root(&self) -> Option<ObjId> {
        let root = map::obj(&self.doc, &ROOT, KEY)?;
        (self.doc.object_type(&root).ok()? == ObjType::Map && map::obj(&self.doc, &root, "text").is_some()).then_some(root)
    }

    /// The document's bytes.
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// The heads of the document.
    pub fn heads(&mut self) -> Vec<ChangeHash> {
        self.doc.get_heads()
    }

    /// Writes from here on under `actor`'s [`actor_id`].
    pub fn set_actor(&mut self, actor: Option<&str>) {
        let id = actor_id(actor);
        if *self.doc.get_actor() != id {
            self.doc.set_actor(id);
        }
    }

    /// The value the document reads as.
    pub fn read(&self) -> Read {
        let raw = Raw::from_value(&self.raw()).expect("a held document reads as raw content");
        let (content, repairs, change) = repair(raw);
        Read { content, repairs, change }
    }

    /// The raw value the document holds, before repair.
    pub fn raw(&self) -> serde_json::Value {
        map::read(&self.doc, &self.root)
    }

    /// Lowers the trace of a change that takes the value the document reads
    /// as to `after`, and commits it as one change.
    pub fn write(&mut self, trace: &[Trace], after: &Content) {
        lower::lower(&mut self.doc, &self.root, trace, after);
        self.doc.commit();
    }

    /// Makes the document read as `target` by diffing what it holds, and
    /// commits that as one change. Answers whether the text differed.
    pub fn sync_to(&mut self, target: &Content) -> bool {
        let differed = map::sync(&mut self.doc, &self.root, target);
        self.doc.commit();
        differed
    }

    /// The document as at `heads`, writing under `actor`'s [`actor_id`]
    /// where no change since `heads` was written under it, and otherwise
    /// under the first of its forks' ids none was: a write on the fork,
    /// merged back, is concurrent with every change since.
    pub fn fork_at(&mut self, heads: &[ChangeHash], actor: Option<&str>) -> Result<Held, AutomergeError> {
        let since: HashSet<ActorId> = self.doc.get_changes(heads).iter().map(|c| c.actor_id().clone()).collect();
        let own = actor_id(actor);
        let id = (0u32..)
            .map(|level| match level {
                0 => own.clone(),
                _ => ActorId::from([own.to_bytes(), FORK, &level.to_be_bytes()].concat()),
            })
            .find(|id| !since.contains(id))
            .expect("some fork id no change holds");
        let doc = self.doc.fork_at(heads)?.with_actor(id);
        let mut held = Held { doc, root: ROOT };
        held.root = held.value_root().expect("a document holds its value at every heads it reached");
        Ok(held)
    }

    /// Takes every change `other` holds that this document does not.
    pub fn merge(&mut self, other: &mut Held) -> Result<(), AutomergeError> {
        self.doc.merge(&mut other.doc).map(|_| ())
    }

    /// Takes the changes an Automerge save or incremental save holds.
    pub fn load_incremental(&mut self, bytes: &[u8]) -> Result<(), AutomergeError> {
        self.doc.load_incremental(bytes).map(|_| ())
    }

    /// The next message of the sync protocol to the peer `state` tracks.
    pub fn generate_sync_message(&mut self, state: &mut sync::State) -> Option<sync::Message> {
        self.doc.sync().generate_sync_message(state)
    }

    /// Takes a message of the sync protocol from the peer `state` tracks.
    pub fn receive_sync_message(&mut self, state: &mut sync::State, message: sync::Message) -> Result<(), AutomergeError> {
        self.doc.sync().receive_sync_message(state, message)
    }

    /// The document.
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }
}
