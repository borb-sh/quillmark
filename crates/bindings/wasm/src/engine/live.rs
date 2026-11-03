//! The document's Automerge history across the JS boundary: its bytes, its
//! heads, and the sync protocol a peer runs against it.

use super::{Document, edit_error_to_js};
use quillmark_core::document::{ChangeHash, EditError, LoadError, SyncMessage};
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

fn heads_from(heads: Vec<String>) -> Result<Vec<ChangeHash>, JsValue> {
    heads
        .iter()
        .map(|h| h.parse::<ChangeHash>().map_err(|e| refused(LoadError::Decode(format!("head `{h}`: {e}")))))
        .collect()
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
            .map_err(|e| refused(LoadError::Decode(e.to_string())))
    }
}

#[wasm_bindgen]
impl Document {
    /// The document `bytes` hold, as `save` gave them or a merge of several.
    /// Bytes that do not decode, or hold no Quillmark document of a schema
    /// this build reads, throw `edit::bad_live_bytes`.
    pub fn load(bytes: &[u8]) -> Result<Document, JsValue> {
        let inner = quillmark_core::document::Document::load(bytes).map_err(refused)?;
        Ok(Document { inner, parse_warnings: Vec::new() })
    }

    /// The document as Automerge bytes, its whole history: the row a host
    /// stores, which `load` reads back.
    pub fn save(&mut self) -> Vec<u8> {
        self.inner.save()
    }

    /// The document's revision: the hashes of its Automerge heads.
    pub fn heads(&mut self) -> Vec<String> {
        self.inner.heads().iter().map(ToString::to_string).collect()
    }

    /// The changes since `heads`, as bytes `merge` takes.
    #[wasm_bindgen(js_name = saveSince)]
    pub fn save_since(&mut self, heads: Vec<String>) -> Result<Vec<u8>, JsValue> {
        Ok(self.inner.save_since(&heads_from(heads)?))
    }

    /// Takes the changes `bytes` hold, another peer's `save` or `saveSince`,
    /// and merges them as Automerge merges. Bytes that do not decode, or
    /// leave the document outside its schema, throw `edit::bad_live_bytes`
    /// and change nothing.
    pub fn merge(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.merge(bytes).map_err(refused)
    }

    /// The next Automerge sync message to the peer `state` tracks,
    /// `undefined` where the peer holds everything.
    #[wasm_bindgen(js_name = generateSyncMessage)]
    pub fn generate_sync_message(&mut self, state: &mut SyncState) -> Option<Vec<u8>> {
        self.inner.generate_sync_message(&mut state.inner).map(SyncMessage::encode)
    }

    /// Takes an Automerge sync message from the peer `state` tracks. A
    /// message that does not decode, or would leave the document outside its
    /// schema, throws `edit::bad_live_bytes` and changes nothing.
    #[wasm_bindgen(js_name = receiveSyncMessage)]
    pub fn receive_sync_message(&mut self, state: &mut SyncState, message: &[u8]) -> Result<(), JsValue> {
        let message = SyncMessage::decode(message).map_err(|e| refused(LoadError::Decode(e.to_string())))?;
        self.inner.receive_sync_message(&mut state.inner, message).map_err(refused)
    }
}
