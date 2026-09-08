//! WebAssembly bindings for Quillmark: the FFI under the hand-written canonical
//! layer (`pkg/runtime/`), which re-exports the core build's `Quill` /
//! `Document` and wraps [`Quillmark`] in an `Engine` that lazily loads a
//! backend. No build this crate emits is a public npm export; which variants
//! ship and what each carries: `prose/canon/BINDINGS.md`.

use wasm_bindgen::prelude::*;

mod engine;
mod error;
mod types;

pub use engine::{Document, Quill};
#[cfg(any(feature = "typst", feature = "pdfform"))]
pub use engine::{LiveSession, Quillmark};
pub use error::WasmError;
pub use types::*;

/// Runs at instantiation, so a Rust panic reaches the console as a stack trace
/// rather than `unreachable`. Not the package's `init` — that name belongs to
/// the hand-written runtime, which owns instantiation itself.
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}
