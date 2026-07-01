//! # `LiveSession` — persistent, incremental preview compiler (SPIKE, issue #778)
//!
//! **Experimental.** A learning spike for [#778]: collapse the frozen
//! `RenderSession` snapshot into a persistent compiler that holds the Typst
//! [`QuillWorld`] alive across edits, so an edit pays only for what it changed
//! rather than rebuilding the world + re-laying-out the whole document.
//!
//! This module deliberately lives *beside* the production `open`/compile path
//! (which it leaves untouched) and does not route through
//! `quillmark_core::SessionHandle`. It exists to prove the mechanism and put
//! numbers on the two wins the issue's staging separates:
//!
//! 1. **Persist the `World` (stage 1) — the real, modest win.** Scaffolding —
//!    font parsing (`Font::iter`), package/asset loading, `typst.toml` parsing —
//!    happens once in [`QuillWorld::new`], not per edit. `comemo`'s cache is
//!    process-global and already survives a world rebuild, so the measurable
//!    stage-1 win is exactly the non-`comemo` scaffolding cost — isolated,
//!    compile-free, by [`bench::world_build_ms`]. It is small and quill-shaped:
//!    ~0.2 ms for a bare quill, ~2 ms for `usaf_memo` (real fonts + vendored
//!    packages), growing with font/package weight. Free (pure refactor, no API
//!    change), so worth taking, but not a step-change.
//!
//! 2. **Body as a real `Source` (stage 4) — tested, and *not* the win it looked
//!    like.** The two [`BodyMode`]s materialize the same converted markup
//!    differently:
//!    - [`BodyMode::EvalBlob`] — today's path: the body markup is a string baked
//!      into the helper `lib.typ` and `eval`'d. An edit rewrites `lib.typ`, so
//!      [`Source::replace`] reparses the whole body-bearing string literal
//!      (O(body): ~37 KB for 400 paragraphs).
//!    - [`BodyMode::IncludeSource`] — the body markup is its own package
//!      `Source` file that `lib.typ` `include`s. An edit is a
//!      [`Source::replace`] on that file, reparsing only the touched span
//!      (O(edit): ~200 B).
//!
//!    The `live_incremental` benchmark shows the reparse-size gap does **not**
//!    translate into a layout win: `comemo`'s global memoization reuses the
//!    layout of untouched paragraphs *below the `eval` boundary too*, so both
//!    modes make a body edit incremental (edit ≈ ⅕ of cold at 50 pages). Eval
//!    is in fact marginally faster — the `include` path's per-compile module
//!    overhead outweighs its reparse savings, and the O(body) reparse never
//!    overtakes even at 188 pages / 140 KB. The dominant per-edit cost is
//!    document-wide reflow (O(pages)), which neither mode changes. Kept as a
//!    two-mode switch so the measurement is reproducible.
//!
//! `apply_body` is **transactional**: a failed recompile keeps the last-good
//! document (the swap-on-commit invariant that lets the immutable snapshot fold
//! into this type).
//!
//! [#778]: https://github.com/quillmark-org/quillmark/issues/778

use std::time::Instant;

use typst_layout::PagedDocument;

use crate::compile::compile_document;
use crate::convert::{escape_string, mark_to_typst};
use crate::helper::generate_typst_toml;
use crate::world::QuillWorld;
use quillmark_core::{Quill, RenderError};

/// How the converted body markup is fed to Typst — the stage-4 variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyMode {
    /// Today's path: body markup baked into `lib.typ` as an `eval`'d string.
    /// An edit is a full `comemo` miss for the body.
    EvalBlob,
    /// Body markup as its own `include`d `Source` file. An edit is a
    /// [`Source::replace`] → incremental reparse + `comemo` reuse.
    IncludeSource,
}

/// The field key the spike carries the body under (the real pipeline uses
/// `$body`).
const BODY_FIELD: &str = "$body";
/// Package-relative path of the body source file in `IncludeSource` mode.
const BODY_REL: &str = "content/body.typ";

/// Outcome of one [`LiveSession::apply_body`] edit.
#[derive(Debug, Clone)]
pub struct ApplyStat {
    /// Wall-clock of the recompile (convert + source swap + `typst::compile`).
    pub recompile: std::time::Duration,
    /// Byte length of the range `Source::replace` reparsed (`IncludeSource`
    /// only; `EvalBlob` regenerates `lib.typ` wholesale).
    pub reparsed_bytes: usize,
    /// Page count after the edit.
    pub page_count: usize,
    /// `false` if the recompile failed and the last-good document was kept.
    pub committed: bool,
}

/// A persistent, incremental Typst preview compiler (spike).
pub struct LiveSession {
    world: QuillWorld,
    doc: PagedDocument,
    mode: BodyMode,
    /// Non-body document fields as a JSON object string (scalars/dates/cards).
    /// Stable across body edits — only the body changes per keystroke.
    scalars_json: String,
}

impl LiveSession {
    /// Open a session: build the world scaffolding once, materialize the body
    /// per `mode`, and run the first (cold) compile.
    ///
    /// `plate` is the Typst plate (main source). `scalars_json` is a JSON object
    /// of the non-body fields. `body_markdown` is the raw markdown body.
    pub fn open(
        quill: &Quill,
        plate: &str,
        scalars_json: &str,
        body_markdown: &str,
        mode: BodyMode,
    ) -> Result<Self, RenderError> {
        let mut world = QuillWorld::new(quill, plate).map_err(|e| {
            RenderError::EngineCreation {
                diags: vec![quillmark_core::Diagnostic::new(
                    quillmark_core::Severity::Error,
                    format!("world scaffolding failed: {e}"),
                )],
            }
        })?;

        // The helper package always ships its typst.toml.
        world.set_binary(
            QuillWorld::helper_fid("typst.toml"),
            generate_typst_toml().into_bytes(),
        );

        let body_markup = convert(body_markdown);

        match mode {
            BodyMode::IncludeSource => {
                world.set_source(QuillWorld::helper_fid("lib.typ"), &lib_include(scalars_json));
                world.set_source(QuillWorld::helper_fid(BODY_REL), &body_markup);
            }
            BodyMode::EvalBlob => {
                world.set_source(
                    QuillWorld::helper_fid("lib.typ"),
                    &lib_eval(scalars_json, &body_markup),
                );
            }
        }

        let doc = compile_document(&world)?;
        Ok(Self {
            world,
            doc,
            mode,
            scalars_json: scalars_json.to_string(),
        })
    }

    /// Apply a body edit: reconvert the markdown, swap the body into the world
    /// per [`BodyMode`], recompile. Transactional — on compile failure the
    /// last-good document is retained and `committed` is `false`.
    pub fn apply_body(&mut self, body_markdown: &str) -> ApplyStat {
        self.apply_body_markup(&convert(body_markdown))
    }

    /// Like [`apply_body`](Self::apply_body) but takes already-converted Typst
    /// markup, skipping the markdown pass. Lets a caller (or the transactional
    /// test) inject markup that deliberately fails to compile.
    pub fn apply_body_markup(&mut self, body_markup: &str) -> ApplyStat {
        let start = Instant::now();

        let reparsed_bytes = match self.mode {
            BodyMode::IncludeSource => {
                self.world
                    .set_source(QuillWorld::helper_fid(BODY_REL), body_markup)
            }
            BodyMode::EvalBlob => {
                // The whole helper lib.typ is regenerated: the body string is
                // baked into it, so the edit is not localizable.
                self.world.set_source(
                    QuillWorld::helper_fid("lib.typ"),
                    &lib_eval(&self.scalars_json, body_markup),
                )
            }
        };

        let result = compile_document(&self.world);
        let recompile = start.elapsed();

        match result {
            Ok(doc) => {
                self.doc = doc; // swap-on-commit
                ApplyStat {
                    recompile,
                    reparsed_bytes,
                    page_count: self.doc.pages().len(),
                    committed: true,
                }
            }
            Err(_) => ApplyStat {
                recompile,
                reparsed_bytes,
                page_count: self.doc.pages().len(),
                committed: false,
            },
        }
    }

    pub fn page_count(&self) -> usize {
        self.doc.pages().len()
    }

    /// Per-page content signatures (SVG hash). The robust-but-costly dirty
    /// signal the issue names: diffing two of these across an edit yields the
    /// dirty page set. Flow layout makes that set a contiguous suffix from the
    /// first affected page.
    pub fn page_signatures(&self) -> Vec<u128> {
        self.doc
            .pages()
            .iter()
            .map(|p| {
                let svg = typst_svg::svg(p, &typst_svg::SvgOptions::default());
                fnv1a(svg.as_bytes())
            })
            .collect()
    }
}

/// Convert markdown to Typst markup, tolerating failure (empty on error) so the
/// spike never panics mid-benchmark.
fn convert(markdown: &str) -> String {
    mark_to_typst(markdown).unwrap_or_default()
}

/// `lib.typ` for `IncludeSource`: the body comes from an `include`d package
/// source; scalars come from the JSON blob.
fn lib_include(scalars_json: &str) -> String {
    format!(
        "#let data = {{\n  let d = json(bytes(\"{}\"))\n  d.insert(\"{}\", include \"{}\")\n  d\n}}\n",
        escape_string(scalars_json),
        BODY_FIELD,
        // include path is relative to lib.typ inside the package
        BODY_REL,
    )
}

/// `lib.typ` for `EvalBlob`: the body markup is baked in and `eval`'d — today's
/// mechanism, distilled.
fn lib_eval(scalars_json: &str, body_markup: &str) -> String {
    // Fold the body into the scalar object so a single json() carries both,
    // mirroring the production single-blob shape.
    let mut obj: serde_json::Value =
        serde_json::from_str(scalars_json).unwrap_or_else(|_| serde_json::json!({}));
    obj[BODY_FIELD] = serde_json::Value::String(body_markup.to_string());
    let full = obj.to_string();
    format!(
        "#let data = {{\n  let d = json(bytes(\"{}\"))\n  d.insert(\"{}\", eval(d.at(\"{}\"), mode: \"markup\"))\n  d\n}}\n",
        escape_string(&full),
        BODY_FIELD,
        BODY_FIELD,
    )
}

/// 128-bit FNV-1a, enough to fingerprint a page's SVG for the dirty-set demo.
fn fnv1a(bytes: &[u8]) -> u128 {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    for &b in bytes {
        h ^= b as u128;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Benchmark helpers used by the spike's integration test.
pub mod bench {
    use super::*;

    /// Time to build the [`QuillWorld`] scaffolding *alone* (font parsing,
    /// package/asset loading) with no compile — the unconfounded stage-1 cost
    /// that persisting the world saves on every edit. No `comemo` interaction.
    pub fn world_build_ms(quill: &Quill, plate: &str) -> std::time::Duration {
        let start = Instant::now();
        let _world = QuillWorld::new(quill, plate).expect("scaffold");
        start.elapsed()
    }

    /// Compile once through a *fresh* [`QuillWorld`] built from scratch — the
    /// production per-edit shape. Isolates the stage-1 scaffolding cost when
    /// compared against a persistent-world recompile (both with warm `comemo`).
    pub fn fresh_world_recompile(
        quill: &Quill,
        plate: &str,
        scalars_json: &str,
        body_markdown: &str,
    ) -> std::time::Duration {
        let body_markup = convert(body_markdown);
        let start = Instant::now();
        let mut world = QuillWorld::new(quill, plate).expect("scaffold");
        world.set_binary(
            QuillWorld::helper_fid("typst.toml"),
            generate_typst_toml().into_bytes(),
        );
        world.set_source(
            QuillWorld::helper_fid("lib.typ"),
            &lib_eval(scalars_json, &body_markup),
        );
        let _ = compile_document(&world).expect("compile");
        start.elapsed()
    }
}
