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
mod own;

pub use automerge;
pub use merge::{Conflict, Reason, conflict, deleted, merge3, reminted, resolved, revise, revise_keeping};

use automerge::sync::{self, SyncDoc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

/// A value held in an Automerge document, at an address: the name of the
/// place the value sits, which its first change carries, so the documents
/// of two addresses share no history and neither takes the other's changes.
/// A write with no actor of its own writes under the document's own random
/// actor, which a clone keeps: a clone that writes is the same peer, and
/// another peer loads a save.
#[derive(Debug, Clone)]
pub struct Held {
    doc: AutoCommit,
    root: ObjId,
    own: ActorId,
    checkpoint: Arc<Checkpoint>,
}

/// The document as it was at `heads`, which a fork at heads after them
/// starts from rather than replaying the history from its first change.
#[derive(Debug)]
struct Checkpoint {
    doc: AutoCommit,
    heads: Vec<ChangeHash>,
}

impl Checkpoint {
    fn of(doc: &mut AutoCommit) -> Arc<Checkpoint> {
        let heads = doc.get_heads();
        Arc::new(Checkpoint { doc: doc.clone(), heads })
    }
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
    #[error("the document holds the value of another address")]
    Address,
}

impl LoadError {
    /// The refusal's name, as a diagnostic's `reason` arg carries it.
    pub fn name(&self) -> &'static str {
        match self {
            LoadError::Decode(_) => "decode",
            LoadError::Shape => "shape",
            LoadError::Content(_) => "content",
            LoadError::Address => "address",
        }
    }
}

impl Held {
    /// A new document holding `content` at `address`. Its first change is
    /// written under one actor at time 0 with `address` as its message, so
    /// every process builds the first change of one value at one address as
    /// the same bytes: two documents built so share their origin and sync,
    /// and two of different addresses or values share none.
    pub fn new(content: &Content, address: &str) -> Held {
        let mut doc = AutoCommit::new_with_encoding(TextEncoding::UnicodeCodePoint).with_actor(ActorId::from(FIRST));
        let root = map::create(&mut doc, &ROOT, KEY, content);
        doc.commit_with(CommitOptions::default().with_time(0).with_message(address.to_string()));
        let own = ActorId::random();
        doc.set_actor(own.clone());
        let checkpoint = Checkpoint::of(&mut doc);
        Held { doc, root, own, checkpoint }
    }

    /// The value a document's bytes hold at `address`, and what it reads
    /// as. Refused where the bytes do not decode, the document's history
    /// opens with no change [`new`](Held::new) made at `address`, it holds
    /// no value in the shape [`map`](crate) writes, or its raw value does
    /// not read as content.
    pub fn load(bytes: &[u8], address: &str) -> Result<(Held, Read), LoadError> {
        let options = LoadOptions::new().text_encoding(TextEncoding::UnicodeCodePoint);
        let mut doc = AutoCommit::load_with_options(bytes, options).map_err(|e| LoadError::Decode(e.to_string()))?;
        let origins: Vec<Option<String>> =
            doc.get_changes_meta(&[]).into_iter().filter(|c| c.deps.is_empty()).map(|c| c.message.map(|m| m.into_owned())).collect();
        if origins != [Some(address.to_string())] {
            return Err(LoadError::Address);
        }
        let own = ActorId::random();
        let (root, read) = checked(&doc)?;
        let mut doc = doc.with_actor(own.clone());
        let checkpoint = Checkpoint::of(&mut doc);
        Ok((Held { doc, root, own, checkpoint }, read))
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
        let doc = match self.fork_from_checkpoint(heads) {
            Some(doc) => doc,
            None => self.doc.fork_at(heads)?,
        };
        let doc = doc.with_actor(id);
        let root = map::value_root(&doc).expect("a document holds its value at every heads it reached");
        let checkpoint = self.checkpoint.clone();
        Ok(Held { doc, root, own: ActorId::random(), checkpoint })
    }

    /// The document at `heads` made from the checkpoint and the changes
    /// since it that `heads` reach, where `heads` descend from it.
    fn fork_from_checkpoint(&mut self, heads: &[ChangeHash]) -> Option<AutoCommit> {
        let mut want: Vec<ChangeHash> = heads.to_vec();
        want.sort();
        want.dedup();
        let mut fork = self.checkpoint.doc.clone();
        let mut at = self.checkpoint.heads.clone();
        at.sort();
        if at != want {
            let since = self.doc.get_changes(&self.checkpoint.heads);
            let by_hash: HashMap<ChangeHash, &automerge::Change> = since.iter().map(|c| (c.hash(), c)).collect();
            let (mut reach, mut stack) = (HashSet::new(), want.clone());
            while let Some(h) = stack.pop() {
                if let Some(c) = by_hash.get(&h) {
                    if reach.insert(h) {
                        stack.extend(c.deps().iter().copied());
                    }
                }
            }
            fork.apply_changes(since.iter().filter(|c| reach.contains(&c.hash())).cloned().collect::<Vec<_>>()).ok()?;
        }
        let mut got = fork.get_heads();
        got.sort();
        (got == want).then_some(fork)
    }

    /// Takes the document as it is now as the checkpoint a later fork
    /// starts from.
    pub fn checkpoint(&mut self) {
        self.checkpoint = Checkpoint::of(&mut self.doc);
    }

    /// After a merge, takes back each edge of an anchor or mark `trace`
    /// lowered that grew over text only the other side typed, as `own`
    /// states: `mine` is the heads of the side that lowered `trace` and
    /// `theirs` the other side's. Commits what it changed as one change, and
    /// answers whether it changed anything.
    pub fn clip(&mut self, mine: &[ChangeHash], theirs: &[ChangeHash], trace: &[Trace]) -> bool {
        let changed = own::clip(&mut self.doc, &self.root, mine, theirs, trace);
        if changed {
            self.doc.commit();
        }
        changed
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
    /// apply, brings a history of its own (another address's, or one built
    /// from another value), or leaves the document holding no value
    /// [`load`](Held::load) takes, is refused, and the document and `state`
    /// stay as they were.
    pub fn receive_sync_message(&mut self, state: &mut sync::State, message: sync::Message) -> Result<Read, LoadError> {
        let (mut doc, mut next) = (self.doc.clone(), state.clone());
        let heads = doc.get_heads();
        doc.sync().receive_sync_message(&mut next, message).map_err(|e| match e {
            AutomergeError::DuplicateSeqNumber(_, actor) if actor.to_bytes() == FIRST => LoadError::Address,
            e => LoadError::Decode(e.to_string()),
        })?;
        if doc.get_changes_meta(&heads).iter().any(|c| c.deps.is_empty()) {
            return Err(LoadError::Address);
        }
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
