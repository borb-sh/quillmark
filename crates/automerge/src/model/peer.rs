//! What crosses between peers of a document: its heads, a sync message, and
//! what one peer is known to hold, each with the encoding a host stores or
//! sends.

use automerge::ChangeHash;
use automerge::sync;

use super::LoadError;

/// A document's revision: the hashes of its Automerge heads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Heads(Vec<ChangeHash>);

impl Heads {
    /// The heads as lowercase hex strings, one per head.
    pub fn encode(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }

    /// The heads `hex` spells, refused as [`LoadError::Decode`] where one
    /// is no change hash.
    pub fn decode<S: AsRef<str>>(hex: &[S]) -> Result<Heads, LoadError> {
        hex.iter()
            .map(|h| {
                let h = h.as_ref();
                h.parse().map_err(|e| LoadError::Decode(format!("head `{h}`: {e}")))
            })
            .collect::<Result<_, _>>()
            .map(Heads)
    }

    pub fn hashes(&self) -> &[ChangeHash] {
        &self.0
    }
}

impl From<Vec<ChangeHash>> for Heads {
    fn from(hashes: Vec<ChangeHash>) -> Heads {
        Heads(hashes)
    }
}

/// What one peer of the sync protocol is known to hold. One serves one peer.
#[derive(Debug, Clone, Default)]
pub struct SyncState(pub(crate) sync::State);

impl SyncState {
    /// A peer of whom nothing is known yet.
    pub fn new() -> SyncState {
        SyncState::default()
    }

    /// The state as bytes [`decode`](SyncState::decode) reads back.
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    /// The state `bytes` hold, refused as [`LoadError::Decode`] where they
    /// do not decode.
    pub fn decode(bytes: &[u8]) -> Result<SyncState, LoadError> {
        sync::State::decode(bytes).map(SyncState).map_err(|e| LoadError::Decode(e.to_string()))
    }
}

/// One message of the sync protocol.
#[derive(Debug, Clone)]
pub struct SyncMessage(pub(crate) sync::Message);

impl SyncMessage {
    pub fn encode(self) -> Vec<u8> {
        self.0.encode()
    }

    /// The message `bytes` hold, refused as [`LoadError::Decode`] where
    /// they do not decode.
    pub fn decode(bytes: &[u8]) -> Result<SyncMessage, LoadError> {
        sync::Message::decode(bytes).map(SyncMessage).map_err(|e| LoadError::Decode(e.to_string()))
    }
}
