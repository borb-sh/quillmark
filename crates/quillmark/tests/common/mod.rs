//! The `usaf_memo` fixture and an engine, loaded once per test binary rather
//! than re-reading the 1.3 MB quill in every test.

// Each integration test binary compiles this module and uses part of it.
#![allow(dead_code)]

use quillmark::{Document, Quill, Quillmark};
use quillmark_fixtures::quills_path;
use std::sync::LazyLock;

static ENGINE: LazyLock<Quillmark> = LazyLock::new(Quillmark::new);

static MEMO: LazyLock<Quill> = LazyLock::new(|| {
    quillmark::quill_from_path(quills_path("usaf_memo")).expect("usaf_memo should load")
});

/// The engine and the flagship `usaf_memo` quill.
pub fn memo() -> (&'static Quillmark, &'static Quill) {
    (&ENGINE, &MEMO)
}

/// [`memo`] plus the quill's seed document, one card per declared kind with
/// every field blank, and an authored main body: a numbered paragraph and a
/// lettered bullet.
pub fn seeded_memo() -> (&'static Quillmark, &'static Quill, Document) {
    let mut doc = MEMO.seed_document();
    doc.main_mut()
        .revise_body("The first paragraph.\n\n- A nested bullet.")
        .expect("a paragraph and a bullet import");
    (&ENGINE, &MEMO, doc)
}
