//! A Quillmark document as one Automerge document.
//!
//! The document is the state: its bytes are what a host stores, its heads
//! are its revision, a write is a transaction and a merge is Automerge's.
//! What a document reads as is a view: each text value read through
//! [`repair`], which nothing writes back.
//!
//! The root holds `schema`, `quill`, `main` (a card), `cards` (cards by
//! `$id`) and `order` (the `$id`s in document order). A card holds `kind`,
//! `layout` (the payload's items in source order, each field by key and
//! each comment whole), `fields`, `nestedComments`, `ext`, `seed` and
//! `body`; a body or content field is a value in the shape `value` states.

pub mod cards;
pub mod ids;
pub mod json;
pub mod patch;
mod peer;
mod stale;
mod step;
pub(crate) mod value;

pub use peer::{Heads, SyncMessage, SyncState};

/// A value's place in the document, which the document's reads and writes
/// of that value take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueId(ObjId);

impl std::ops::Deref for ValueId {
    type Target = ObjId;

    fn deref(&self) -> &ObjId {
        &self.0
    }
}
pub use step::{Lowered, StepError};

use automerge::sync::SyncDoc;
use automerge::transaction::{CommitOptions, Transactable};
use automerge::{
    ActorId, AutoCommit, ChangeHash, ObjId, ObjType, ROOT, ReadDoc, TextEncoding,
};
use crate::content::change::Change;
use crate::content::model::Content;
use crate::content::repair::{Raw, Repair, repair};
use serde_json::Value;

/// The document as it was at `heads`, writing under a random actor: the
/// changes `heads` reach, in the order the document holds them. None where
/// one of them names an actor none of them brings, since Automerge 0.12
/// panics applying such a change to a document that lacks the actor.
pub(crate) fn fork(doc: &mut AutoCommit, heads: &[ChangeHash]) -> Option<AutoCommit> {
    let mut reached = std::collections::HashSet::new();
    let mut todo = heads.to_vec();
    while let Some(h) = todo.pop() {
        if reached.insert(h) {
            todo.extend(doc.get_change_by_hash(&h)?.deps().iter().copied());
        }
    }
    let changes: Vec<automerge::Change> = doc.get_changes(&[]).into_iter().filter(|c| reached.contains(&c.hash())).collect();
    let actors: std::collections::HashSet<&ActorId> = changes.iter().map(|c| c.actor_id()).collect();
    if !changes.iter().all(|c| c.other_actor_ids().iter().all(|a| actors.contains(a))) {
        return None;
    }
    let mut f = AutoCommit::new_with_encoding(TextEncoding::UnicodeCodePoint);
    f.apply_changes(changes).ok()?;
    Some(f)
}

/// The most steps a write at earlier heads lowers into the document itself,
/// isolated at those heads; a longer one lowers on a fork at them, where each
/// op costs what it does at the heads now.
const ISOLATED_STEPS: usize = 16;

/// The schema a document's bytes hold under `schema`: the layout version of
/// the live document.
pub const SCHEMA: &str = "quillmark/document@0.135.0";

pub const QUILL: &str = "quill";
pub const MAIN: &str = "main";
pub const CARDS: &str = "cards";
pub const ORDER: &str = "order";
pub const KIND: &str = "kind";
pub const FIELDS: &str = "fields";
pub const LAYOUT: &str = "layout";
pub const NESTED: &str = "nestedComments";
pub const EXT: &str = "ext";
pub const SEED: &str = "seed";
pub const BODY: &str = "body";

/// The bytes every saved Automerge document starts with.
pub const MAGIC: [u8; 4] = [0x85, 0x6f, 0x4a, 0x83];

/// The actor of a document's first change: a byte no random actor spells,
/// so every process builds one skeleton as the same bytes.
const FIRST: &[u8] = b"~";

/// A Quillmark document held as one Automerge document.
#[derive(Debug, Clone)]
pub struct Doc {
    doc: AutoCommit,
}

/// What a value reads as: repair's valid value and what it read through.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub content: Content,
    pub repairs: Vec<Repair>,
}

/// Bytes or a message a document refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error("the bytes do not decode as Automerge: {0}")]
    Decode(String),
    #[error("the document holds no Quillmark document schema")]
    Shape,
    #[error("the document's schema `{0}` is not one this build reads")]
    Schema(String),
    #[error("the sync message decodes but does not apply: {0}")]
    Apply(String),
}

impl LoadError {
    pub fn name(&self) -> &'static str {
        match self {
            LoadError::Decode(_) => "decode",
            LoadError::Shape => "shape",
            LoadError::Schema(_) => "schema",
            LoadError::Apply(_) => "apply",
        }
    }
}

impl Default for Doc {
    fn default() -> Doc {
        Doc::new()
    }
}

impl Doc {
    /// A new document holding the schema skeleton, an empty main card and
    /// no cards. The skeleton is written under one actor at time 0, so every
    /// process builds it as the same change and two documents built so
    /// share their origin; the document then writes under a random actor.
    pub fn new() -> Doc {
        Doc::building(|_| {})
    }

    /// A new document whose first change is the skeleton and what `build`
    /// writes, under the first actor at time 0: two processes building one
    /// document from one input make the same history and sync.
    pub fn building(build: impl FnOnce(&mut Doc)) -> Doc {
        let doc = AutoCommit::new_with_encoding(TextEncoding::UnicodeCodePoint)
            .with_actor(ActorId::from(FIRST));
        let mut d = Doc { doc };
        d.doc.put(&ROOT, "schema", SCHEMA).unwrap();
        let main = d.doc.put_object(&ROOT, MAIN, ObjType::Map).unwrap();
        d.card_skeleton(&main);
        d.doc.put_object(&ROOT, CARDS, ObjType::Map).unwrap();
        d.doc.put_object(&ROOT, ORDER, ObjType::List).unwrap();
        build(&mut d);
        d.doc.commit_with(CommitOptions::default().with_time(0));
        d.doc.set_actor(ActorId::random());
        d
    }

    /// Fills the empty card map `card` with an empty card: no fields and an
    /// empty body.
    pub fn card_skeleton(&mut self, card: &ObjId) {
        for key in [FIELDS, EXT, SEED] {
            self.doc.put_object(card, key, ObjType::Map).unwrap();
        }
        for key in [LAYOUT, NESTED] {
            self.doc.put_object(card, key, ObjType::List).unwrap();
        }
        value::create(&mut self.doc, card, BODY, &Content::empty());
    }

    /// The document the bytes hold.
    pub fn load(bytes: &[u8]) -> Result<Doc, LoadError> {
        let doc = AutoCommit::load_with_options(
            bytes,
            automerge::LoadOptions::new().text_encoding(TextEncoding::UnicodeCodePoint),
        )
        .map_err(|e| LoadError::Decode(e.to_string()))?;
        let mut d = Doc { doc };
        d.check()?;
        d.doc.set_actor(ActorId::random());
        Ok(d)
    }

    fn check(&self) -> Result<(), LoadError> {
        match value::get_str(&self.doc, &ROOT, "schema") {
            Some(s) if s == SCHEMA => {}
            Some(s) => return Err(LoadError::Schema(s)),
            None => return Err(LoadError::Shape),
        }
        let is = |k: &str, ty: ObjType| {
            value::obj(&self.doc, &ROOT, k)
                .is_some_and(|o| self.doc.object_type(&o).ok() == Some(ty))
        };
        match is(MAIN, ObjType::Map) && is(CARDS, ObjType::Map) && is(ORDER, ObjType::List) {
            true => Ok(()),
            false => Err(LoadError::Shape),
        }
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// The changes since `heads`, as bytes [`merge`](Doc::merge) takes.
    pub fn save_since(&mut self, heads: &[ChangeHash]) -> Vec<u8> {
        self.doc.save_after(heads)
    }

    /// Takes the changes `bytes` hold, a save or a [`save_since`]'s.
    /// Changes the document holds already change nothing.
    ///
    /// [`save_since`]: Doc::save_since
    pub fn merge(&mut self, bytes: &[u8]) -> Result<(), LoadError> {
        let before = self.doc.clone();
        self.doc
            .load_incremental(bytes)
            .map_err(|e| LoadError::Decode(e.to_string()))?;
        if let Err(e) = self.check() {
            self.doc = before;
            return Err(e);
        }
        Ok(())
    }

    pub fn heads(&mut self) -> Vec<ChangeHash> {
        self.doc.get_heads()
    }

    /// Writes under `actor`, its bytes the Automerge actor, from now on.
    pub fn set_actor(&mut self, actor: &str) {
        self.doc.set_actor(ActorId::from(actor.as_bytes()));
    }

    pub fn generate_sync_message(&mut self, state: &mut SyncState) -> Option<SyncMessage> {
        self.doc.sync().generate_sync_message(&mut state.0).map(SyncMessage)
    }

    /// Takes a peer's sync message. A message that leaves the document
    /// outside the schema is undone.
    pub fn receive_sync_message(
        &mut self,
        state: &mut SyncState,
        message: SyncMessage,
    ) -> Result<(), LoadError> {
        let (before, was) = (self.doc.clone(), state.clone());
        self.doc
            .sync()
            .receive_sync_message(&mut state.0, message.0)
            .map_err(|e| LoadError::Apply(e.to_string()))?;
        if let Err(e) = self.check() {
            self.doc = before;
            *state = was;
            return Err(e);
        }
        Ok(())
    }

    pub fn am(&self) -> &AutoCommit {
        &self.doc
    }

    /// Commits what the document holds uncommitted.
    pub fn commit(&mut self) {
        self.doc.commit();
    }

    pub fn main(&self) -> ObjId {
        value::obj(&self.doc, &ROOT, MAIN).expect("a checked document holds its main card")
    }

    /// The card `id` names, where `cards` holds it and `order` names it.
    pub fn card(&self, id: &str) -> Option<ObjId> {
        self.order()
            .iter()
            .any(|o| o == id)
            .then(|| value::obj(&self.doc, &self.cards(), id))
            .flatten()
    }

    pub fn cards(&self) -> ObjId {
        value::obj(&self.doc, &ROOT, CARDS).expect("a checked document holds its cards")
    }

    /// The card ids in document order, each once, the first place a
    /// concurrent move left it, and only ids `cards` holds.
    pub fn order(&self) -> Vec<String> {
        let Some(order) = value::obj(&self.doc, &ROOT, ORDER) else {
            return Vec::new();
        };
        let cards = self.cards();
        let mut seen = std::collections::HashSet::new();
        self.doc
            .values(&order)
            .filter_map(|(v, _)| match v {
                automerge::Value::Scalar(s) => s.as_str().map(str::to_string),
                _ => None,
            })
            .filter(|id| value::obj(&self.doc, &cards, id).is_some() && seen.insert(id.clone()))
            .collect()
    }

    /// What the value `root` reads as.
    pub fn view(&self, root: &ObjId) -> View {
        let raw = Raw::from_value(&self.raw(root)).expect("a value reads as raw content");
        let (content, repairs, _) = repair(raw);
        View { content, repairs }
    }

    /// Whether the document's history holds every change `heads` names.
    pub fn holds(&self, heads: &[ChangeHash]) -> bool {
        heads.iter().all(|h| ReadDoc::get_change_by_hash(&self.doc, h).is_some())
    }

    /// Runs `f` on the document as it was at `heads`: what it reads is that
    /// state, and what it writes is one change concurrent with every change
    /// since, which merges with them.
    pub fn isolated<T>(&mut self, heads: &[ChangeHash], f: impl FnOnce(&mut Doc) -> T) -> T {
        self.doc.commit();
        self.doc.isolate(heads);
        let out = f(self);
        self.doc.commit();
        self.doc.integrate();
        out
    }

    /// What the value `root` read as at `heads`, `None` where it held no
    /// value then.
    pub fn try_view_at(&mut self, root: &ObjId, heads: &[ChangeHash]) -> Option<View> {
        self.reading_at(heads, |d| value::text(&d.doc, root).is_some().then(|| d.view(root)))
    }

    /// What the value `root` read as at `heads`.
    pub fn view_at(&mut self, root: &ObjId, heads: &[ChangeHash]) -> View {
        self.reading_at(heads, |d| d.view(root))
    }

    /// Reads the document as it was at `heads`. A read isolated at earlier
    /// heads costs the whole text for each element it seeks, so it reads a
    /// fork at them.
    pub fn reading_at<T>(&mut self, heads: &[ChangeHash], read: impl FnOnce(&Doc) -> T) -> T {
        self.doc.commit();
        if heads == self.doc.get_heads().as_slice() {
            return read(self);
        }
        match fork(&mut self.doc, heads) {
            Some(doc) => read(&Doc { doc }),
            None => self.isolated(heads, |d| read(d)),
        }
    }

    /// The raw value `root` holds, before repair.
    pub fn raw(&self, root: &ObjId) -> Value {
        value::read(&self.doc, root)
    }

    /// Lowers `change`, made against what the value `root` reads as at
    /// `heads` (now, where none), as one change. A step that does not place
    /// refuses and nothing changes.
    pub fn apply(
        &mut self,
        root: &ObjId,
        change: &Change,
        heads: Option<&[ChangeHash]>,
    ) -> Result<Lowered, StepError> {
        self.apply_from(root, change, heads, None)
    }

    /// [`apply`](Self::apply), with `read` the value `root` reads as at
    /// `heads`, so the lowering reads no view of its own.
    pub fn apply_from(
        &mut self,
        root: &ObjId,
        change: &Change,
        heads: Option<&[ChangeHash]>,
        read: Option<&Content>,
    ) -> Result<Lowered, StepError> {
        self.doc.commit();
        let before = self.doc.get_heads();
        let isolated = heads.filter(|h| *h != before.as_slice());
        if let Some(h) = isolated
            && change.steps.len() > ISOLATED_STEPS
            && let Some(lowered) = stale::apply(&mut self.doc, root, change, h, read)
        {
            return lowered;
        }
        if let Some(h) = isolated {
            self.doc.isolate(h);
        }
        let lowered = step::lower(&mut self.doc, root, change, read);
        match lowered {
            Ok(_) => {
                self.doc.commit();
            }
            Err(_) => {
                self.doc.rollback();
            }
        }
        if isolated.is_some() {
            self.doc.integrate();
        }
        lowered
    }

    /// Automerge's patches taking the document from `before` to now, text
    /// indexes counted in `u`.
    pub fn patches(&mut self, before: &[ChangeHash], u: impl crate::content::units::Units) -> Vec<Value> {
        self.doc.commit();
        let after = self.doc.get_heads();
        patch::between(&mut self.doc, before, &after, u)
    }

    /// Starts recording the changes past the document's heads now, so
    /// [`landed`](Self::landed) from them reads no history.
    pub fn track(&mut self) {
        self.doc.update_diff_cursor();
    }

    /// [`patches`](Self::patches) from `before`, and the view of `root`, which
    /// read as `was` at `before`, they leave.
    pub fn landed(&mut self, root: &ObjId, before: &[ChangeHash], was: &Content, u: impl crate::content::units::Units) -> (Vec<Value>, View) {
        self.doc.commit();
        let after = self.doc.get_heads();
        let (patches, followed) = patch::landed(&mut self.doc, before, &after, u, root, was);
        let view = match followed {
            Some(content) => {
                debug_assert_eq!(content, self.view(root).content, "the patches leave the value the document reads");
                View { content, repairs: Vec::new() }
            }
            None => self.view(root),
        };
        (patches, view)
    }

    /// Automerge's patches taking the document from `before` to `after`.
    pub fn patches_between(&mut self, before: &[ChangeHash], after: &[ChangeHash], u: impl crate::content::units::Units) -> Vec<Value> {
        self.doc.commit();
        patch::between(&mut self.doc, before, after, u)
    }

    /// Puts `content` under `key` of the map `parent` as a new value.
    pub fn put_value(&mut self, parent: &ObjId, key: &str, content: &Content) -> ObjId {
        value::create(&mut self.doc, parent, key, content)
    }

    /// The value at `key` of the map `parent`.
    pub fn value(&self, parent: &ObjId, key: &str) -> Option<ObjId> {
        value::obj(&self.doc, parent, key).filter(|v| value::text(&self.doc, v).is_some())
    }
}
