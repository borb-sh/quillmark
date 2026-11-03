//! A value's live Automerge document across the JS boundary: its bytes, and
//! the sync protocol a peer runs against it.

use super::content::{ReceiptJs, card_base, card_mut_of, receipt_js, target, value_path};
use super::{Addr, Document, edit_error_to_js, serialize_or_throw};
use quillmark_automerge::automerge::sync;
use quillmark_content::units::Utf16;
use quillmark_core::document::{EditError, Receipt};
use wasm_bindgen::prelude::*;

/// What one peer of the Automerge sync protocol is known to hold, for one
/// value: `Document.syncState()` makes it, and `generateSyncMessage` and
/// `receiveSyncMessage` read and advance it. Opaque, and in memory only.
#[wasm_bindgen]
pub struct SyncState {
    inner: sync::State,
}

/// Run `take` on the card `addr` names, its receipt as it crosses.
fn taking(
    doc: &mut quillmark_core::document::Document,
    addr: &Addr,
    take: impl FnOnce(&mut quillmark_core::document::CardMut<'_>) -> Result<Receipt, EditError>,
) -> Result<ReceiptJs, JsValue> {
    let card_base = card_base(doc, addr);
    let path = value_path(&card_base, addr);
    let mut card = card_mut_of(doc, addr)?;
    let receipt = take(&mut card).map_err(|e| edit_error_to_js(&e, &card_base))?;
    let card_id = card.id().map(str::to_string);
    Ok(receipt_js(receipt.in_units(Utf16), &path, card_id))
}

#[wasm_bindgen]
impl Document {
    /// The live Automerge document of the value at `addr`, as its bytes:
    /// made from the value where no write has made it yet. Throws as `read`.
    #[wasm_bindgen(js_name = save)]
    pub fn save(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let addr = Addr::from_js_or_string(&addr, &self.inner)?;
        let card_base = card_base(&self.inner, &addr);
        card_mut_of(&mut self.inner, &addr)?
            .save(target(&addr))
            .map_err(|e| edit_error_to_js(&e, &card_base))
    }

    /// Replace the live document of the value at `addr` with the one `bytes`
    /// hold (`save`'s), store what it reads as, and answer the `Receipt` of a
    /// write from the value before. The document keeps the history the bytes
    /// hold but notes no hash before the load, so a base read before it
    /// throws `edit::stale_base`. Bytes that hold no value throw
    /// `edit::bad_live_bytes`; the card is named by `$id`, as a content write's
    /// is.
    #[wasm_bindgen(js_name = load, unchecked_return_type = "Receipt")]
    pub fn load(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        bytes: &[u8],
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::for_write(&addr, &self.inner, "load")?;
        let t = target(&addr);
        let receipt = taking(&mut self.inner, &addr, |card| card.load(t, bytes))?;
        serialize_or_throw(&receipt, "load")
    }

    /// A fresh `SyncState`: a peer of whom nothing is known yet. One serves
    /// one value and one peer.
    #[wasm_bindgen(js_name = syncState)]
    pub fn sync_state() -> SyncState {
        SyncState { inner: sync::State::new() }
    }

    /// The next Automerge sync message from the live document of the value
    /// at `addr` to the peer `state` tracks, `undefined` where the peer holds
    /// everything. Throws as `read`.
    #[wasm_bindgen(js_name = generateSyncMessage)]
    pub fn generate_sync_message(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        state: &mut SyncState,
    ) -> Result<Option<Vec<u8>>, JsValue> {
        let addr = Addr::from_js_or_string(&addr, &self.inner)?;
        let card_base = card_base(&self.inner, &addr);
        let message = card_mut_of(&mut self.inner, &addr)?
            .generate_sync_message(target(&addr), &mut state.inner)
            .map_err(|e| edit_error_to_js(&e, &card_base))?;
        Ok(message.map(sync::Message::encode))
    }

    /// Take an Automerge sync message from the peer `state` tracks into the
    /// live document of the value at `addr`, store what it then reads as,
    /// and answer the `Receipt` of a write from the value before: its
    /// `droppedAnchors` each anchor the merge left nowhere, its
    /// `movedAnchors` each one it moved. A message the document refuses
    /// throws `edit::bad_live_bytes`; the card is named by `$id`, as a content
    /// write's is.
    #[wasm_bindgen(js_name = receiveSyncMessage, unchecked_return_type = "Receipt")]
    pub fn receive_sync_message(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        state: &mut SyncState,
        message: &[u8],
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::for_write(&addr, &self.inner, "receiveSyncMessage")?;
        let t = target(&addr);
        let receipt = taking(&mut self.inner, &addr, |card| {
            let message = sync::Message::decode(message).map_err(|e| EditError::bad_live_bytes(quillmark_automerge::LoadError::Decode(e.to_string())))?;
            card.receive_sync_message(t, &mut state.inner, message)
        })?;
        serialize_or_throw(&receipt, "receiveSyncMessage")
    }
}
