//! WebAssembly bindings for Quillmark: the FFI under the hand-written canonical
//! layer (`pkg/runtime/`), which re-exports the core build's `Quill` /
//! `Document` and wraps the render build's [`Engine`] in a JS `Engine` that
//! lazily loads that build. No build this crate emits is a public npm export;
//! `scripts/build-wasm.sh` says which variants ship and what each carries.

use wasm_bindgen::prelude::*;

mod engine;
mod error;
mod types;

pub use engine::{Document, Quill};
#[cfg(feature = "render")]
pub use engine::{Engine, LiveSession};
pub use error::WasmError;
pub use types::*;

/// Runs at instantiation, so a Rust panic reaches the console as a stack trace
/// rather than `unreachable`. Not the package's `init` — that name belongs to
/// the hand-written runtime, which owns instantiation itself.
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}
