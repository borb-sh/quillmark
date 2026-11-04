//! The document's Automerge history across the JS boundary: its bytes, its
//! heads, and the sync protocol a peer runs against it.

use super::{Document, edit_error_to_js, serialize_or_throw};
use quillmark_automerge::content::units::Utf16;
use quillmark_core::document::{EditError, Heads, LoadError, SyncMessage};
use quillmark_core::path::DocPath;
use wasm_bindgen::prelude::*;

/// What one peer of the Automerge sync protocol is known to hold:
/// `new SyncState()` makes it, and `generateSyncMessage` and
/// `receiveSyncMessage` read and advance it. One serves one peer; `encode`
/// keeps it across a restart, and `free()` releases it when the peer is
/// gone.
#[wasm_bindgen]
pub struct SyncState {
    inner: quillmark_core::document::SyncState,
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState { inner: quillmark_core::document::SyncState::new() }
    }
}

fn refused(e: LoadError) -> JsValue {
    edit_error_to_js(&EditError::bad_live_bytes(e), &DocPath::main())
}

#[wasm_bindgen]
impl SyncState {
    /// A peer of whom nothing is known yet.
    #[wasm_bindgen(constructor)]
    pub fn new() -> SyncState {
        SyncState::default()
    }

    /// The state as bytes `decode` reads back.
    pub fn encode(&self) -> Vec<u8> {
        self.inner.encode()
    }

    /// The state `encode` gave as `bytes`. Bytes that do not decode throw
    /// `edit::bad_live_bytes` with `reason` `decode`.
    pub fn decode(bytes: &[u8]) -> Result<SyncState, JsValue> {
        quillmark_core::document::SyncState::decode(bytes)
            .map(|inner| SyncState { inner })
            .map_err(refused)
    }
}

#[wasm_bindgen]
impl Document {
    /// The document `bytes` hold, as `save` gave them or a merge of several.
    /// Bytes that do not decode, or hold no Quillmark document of a schema
    /// this build reads, throw `edit::bad_live_bytes`.
    pub fn load(bytes: &[u8]) -> Result<Document, JsValue> {
        let inner = quillmark_core::document::Document::load(bytes).map_err(|e| edit_error_to_js(&e, &DocPath::main()))?;
        Ok(Document { inner, parse_warnings: Vec::new() })
    }

    /// The document as Automerge bytes, its whole history: what a peer `load`s
    /// or `merge`s. A host stores the row, `toStored()`.
    pub fn save(&mut self) -> Vec<u8> {
        self.inner.save()
    }

    /// The actor this document writes under, where one was set: each write's
    /// Automerge actor, and the scope of each card id a placement mints,
    /// `{actor}-{token}`. `undefined` writes under a random actor and mints
    /// card ids positionally.
    #[wasm_bindgen(getter)]
    pub fn actor(&self) -> Option<String> {
        self.inner.actor().map(str::to_string)
    }

    /// Writes under `actor` from now on: a live client sets one unique to its
    /// session. One that is not 1 to 57 of `[A-Za-z0-9_-]`, opening with a
    /// letter or digit, throws `edit::invalid_actor`.
    #[wasm_bindgen(js_name = setActor)]
    pub fn set_actor(&mut self, actor: &str) -> Result<(), JsValue> {
        self.inner.set_actor(actor).map_err(|e| edit_error_to_js(&e, &DocPath::main()))
    }

    /// The document's revision: the hashes of its Automerge heads.
    pub fn heads(&mut self) -> Vec<String> {
        self.inner.heads().encode()
    }

    /// The changes since `heads`, as bytes `merge` takes. A head that is no
    /// change hash throws, as a malformed `heads` option does.
    #[wasm_bindgen(js_name = saveSince)]
    pub fn save_since(&mut self, heads: Vec<String>) -> Result<Vec<u8>, JsValue> {
        let heads = Heads::decode(&heads).map_err(|e| crate::error::WasmError::from(format!("saveSince: {e}")).to_js_value())?;
        Ok(self.inner.save_since(&heads))
    }

    /// Takes the changes `bytes` hold, another peer's `save` or `saveSince`,
    /// merges them as Automerge merges, and answers their `Receipt`. Bytes
    /// that do not decode, or leave the document outside its schema, throw
    /// `edit::bad_live_bytes` and change nothing.
    #[wasm_bindgen(unchecked_return_type = "Receipt")]
    pub fn merge(&mut self, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let receipt = self.inner.merge_in(bytes, Utf16).map_err(|e| edit_error_to_js(&e, &DocPath::main()))?;
        serialize_or_throw(&super::content::receipt_js(receipt, &DocPath::main(), None), "merge")
    }

    /// The next Automerge sync message to the peer `state` tracks,
    /// `undefined` where the peer holds everything.
    #[wasm_bindgen(js_name = generateSyncMessage)]
    pub fn generate_sync_message(&mut self, state: &mut SyncState) -> Option<Vec<u8>> {
        self.inner.generate_sync_message(&mut state.inner).map(SyncMessage::encode)
    }

    /// Takes an Automerge sync message from the peer `state` tracks and
    /// answers its `Receipt`. A message that does not decode, or would leave
    /// the document outside its schema, throws `edit::bad_live_bytes` and
    /// changes nothing.
    #[wasm_bindgen(js_name = receiveSyncMessage, unchecked_return_type = "Receipt")]
    pub fn receive_sync_message(&mut self, state: &mut SyncState, message: &[u8]) -> Result<JsValue, JsValue> {
        let message = SyncMessage::decode(message).map_err(refused)?;
        let receipt = self.inner.receive_sync_message_in(&mut state.inner, message, Utf16).map_err(|e| edit_error_to_js(&e, &DocPath::main()))?;
        serialize_or_throw(&super::content::receipt_js(receipt, &DocPath::main(), None), "receiveSyncMessage")
    }
}
