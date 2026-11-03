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

use automerge::transaction::CommitOptions;
use automerge::{ActorId, AutoCommit, AutomergeError, ChangeHash, LoadOptions, ObjId, ROOT, TextEncoding};
use quillmark_content::change::{Change, Trace};
use quillmark_content::model::Content;
use quillmark_content::repair::{Raw, Repair, repair};

/// The root key a value sits under.
pub const KEY: &str = "value";

/// The actor of a value's first change: a byte no actor spells.
const FIRST: &[u8] = b"~";

/// The byte between an actor's id and its fork's level in a fork's id.
const FORK: &[u8] = b"\x1f";

/// A value held in an Automerge document. A write with no actor of its own
/// writes under the document's own random actor, which a clone keeps: a
/// clone that writes is the same peer, and another peer loads a save.
#[derive(Debug, Clone)]
pub struct Held {
    doc: AutoCommit,
    root: ObjId,
    own: ActorId,
}

/// The value a document reads as: what repair made of its merged state, what
/// it changed, and the change from the merged text to the value's.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub content: Content,
    pub repairs: Vec<Repair>,
    pub change: Change,
}

/// Bytes or a sync message a held value refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error("the bytes do not decode as Automerge: {0}")]
    Decode(String),
    #[error("the document holds no content value under `{KEY}` in the shape a held value takes")]
    Shape,
    #[error("the value the document holds does not read as content: {0}")]
    Content(String),
}

impl LoadError {
    /// The refusal's name, as a diagnostic's `reason` arg carries it.
    pub fn name(&self) -> &'static str {
        match self {
            LoadError::Decode(_) => "decode",
            LoadError::Shape => "shape",
            LoadError::Content(_) => "content",
        }
    }
}

impl Held {
    /// A new document holding `content`. Its first change is written under
    /// one actor at time 0, so every process builds one value's first change
    /// as the same bytes, and two documents built from equal values share
    /// their origin and sync.
    pub fn new(content: &Content) -> Held {
        let mut doc = AutoCommit::new_with_encoding(TextEncoding::UnicodeCodePoint).with_actor(ActorId::from(FIRST));
        let root = map::create(&mut doc, &ROOT, KEY, content);
        doc.commit_with(CommitOptions::default().with_time(0));
        let own = ActorId::random();
        doc.set_actor(own.clone());
        Held { doc, root, own }
    }

    /// The value a document's bytes hold, and what it reads as. Refused
    /// where the bytes do not decode, the document holds no value in the
    /// shape [`map`](crate) writes, or its raw value does not read as
    /// content.
    pub fn load(bytes: &[u8]) -> Result<(Held, Read), LoadError> {
        let options = LoadOptions::new().text_encoding(TextEncoding::UnicodeCodePoint);
        let doc = AutoCommit::load_with_options(bytes, options).map_err(|e| LoadError::Decode(e.to_string()))?;
        let own = ActorId::random();
        let (root, read) = checked(&doc)?;
        Ok((Held { doc: doc.with_actor(own.clone()), root, own }, read))
    }

    /// The document's bytes.
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// The heads of the document.
    pub fn heads(&mut self) -> Vec<ChangeHash> {
        self.doc.get_heads()
    }

    /// The Automerge actor of a write's actor: its bytes, or for a write
    /// with no actor the document's own.
    fn actor_id(&self, actor: Option<&str>) -> ActorId {
        actor.map_or_else(|| self.own.clone(), |a| ActorId::from(a.as_bytes()))
    }

    /// Writes from here on under `actor`'s Automerge actor, the document's
    /// own for none.
    pub fn set_actor(&mut self, actor: Option<&str>) {
        let id = self.actor_id(actor);
        if *self.doc.get_actor() != id {
            self.doc.set_actor(id);
        }
    }

    /// The value the document reads as.
    pub fn read(&self) -> Read {
        let raw = Raw::from_value(&self.raw()).expect("a held document reads as raw content");
        read(raw)
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

    /// The document as at `heads`, writing under `actor`'s Automerge actor
    /// where no change since `heads` was written under it, and otherwise
    /// under the first of its forks' ids none was: a write on the fork,
    /// merged back, is concurrent with every change since.
    pub fn fork_at(&mut self, heads: &[ChangeHash], actor: Option<&str>) -> Result<Held, AutomergeError> {
        let since: HashSet<ActorId> = self.doc.get_changes(heads).iter().map(|c| c.actor_id().clone()).collect();
        let own = self.actor_id(actor);
        let id = (0u32..)
            .map(|level| match level {
                0 => own.clone(),
                _ => ActorId::from([own.to_bytes(), FORK, &level.to_be_bytes()].concat()),
            })
            .find(|id| !since.contains(id))
            .expect("some fork id no change holds");
        let doc = self.doc.fork_at(heads)?.with_actor(id);
        let root = map::value_root(&doc).expect("a document holds its value at every heads it reached");
        Ok(Held { doc, root, own: ActorId::random() })
    }

    /// Takes every change `other` holds that this document does not.
    pub fn merge(&mut self, other: &mut Held) -> Result<(), AutomergeError> {
        self.doc.merge(&mut other.doc).map(|_| ())
    }

    /// The next message of the sync protocol to the peer `state` tracks.
    pub fn generate_sync_message(&mut self, state: &mut sync::State) -> Option<sync::Message> {
        self.doc.sync().generate_sync_message(state)
    }

    /// Takes a message of the sync protocol from the peer `state` tracks,
    /// and answers what the document reads as then. A message that does not
    /// apply, or leaves the document holding no value [`load`](Held::load)
    /// takes, is refused, and the document and `state` stay as they were.
    pub fn receive_sync_message(&mut self, state: &mut sync::State, message: sync::Message) -> Result<Read, LoadError> {
        let (mut doc, mut next) = (self.doc.clone(), state.clone());
        doc.sync().receive_sync_message(&mut next, message).map_err(|e| LoadError::Decode(e.to_string()))?;
        let (root, read) = checked(&doc)?;
        (self.doc, self.root, *state) = (doc, root, next);
        Ok(read)
    }

    /// The document.
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }
}

/// The value root of `doc` and what it reads as, where it holds one in the
/// shape `map` writes and its raw value reads as content.
fn checked(doc: &AutoCommit) -> Result<(ObjId, Read), LoadError> {
    let root = map::value_root(doc).filter(|root| map::shaped(doc, root)).ok_or(LoadError::Shape)?;
    let raw = Raw::from_value(&map::read(doc, &root)).map_err(|e| LoadError::Content(e.to_string()))?;
    Ok((root, read(raw)))
}

fn read(raw: Raw) -> Read {
    let (content, repairs, change) = repair(raw);
    Read { content, repairs, change }
}
