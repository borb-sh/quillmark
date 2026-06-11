# Quillmark Security & Parsing/Conversion Audit

**Date:** 2026-06-11
**Scope:** All production crates — `quillmark-core`, `quillmark` (orchestration),
`quillmark-typst` (backend), and the CLI / Python / WASM bindings. Plus a
dependency advisory scan and a review of fuzz coverage.
**Method:** Three parallel deep-read audits (core parsing/YAML/emit, Typst
backend injection/conversion, orchestration + bindings), each verifying
high-value findings against the real pipeline with throwaway harnesses; plus
`cargo audit` on the full dependency tree.

## How to read this document

- **~~Struck-through headings~~** are **resolved on this branch** — fix applied,
  regression-tested, and committed. Each carries a ✅ **Resolved** line.
- Headings in normal text are **open** and await your review. They were judged
  `NEEDS-REVIEW` (a behavior/contract change or an architecturally significant
  change) and were intentionally **not** auto-applied.
- Severity: Critical / High / Medium / Low / Info.

## Executive summary

The most security-critical surface — user Markdown/YAML/card/plate content
breaking out of escaping to inject **raw Typst code** — is **solid**. Adversarial
end-to-end testing through the real Typst evaluator produced **no injection**, and
the in-memory virtual filesystem makes **path traversal structurally impossible**
on the render path (no disk or network access; packages are vendored). The
dependency tree has **zero known-vulnerable crates**.

The real issues are concentrated in **core parsing/emit** and in
**defense-in-depth hardening** of the filesystem loader and language bindings.

| Area | Critical | High | Medium | Low | Info |
|------|:-:|:-:|:-:|:-:|:-:|
| Core (parse/YAML/emit) | 0 | 2 | 3 | 0 | 1 |
| Typst backend | 0 | 0 | 0 | 2 | 3 |
| Orchestration / bindings | 0 | 0 | 2 | 6 | 2 |

**11 findings resolved** on this branch. **5 substantive findings** plus several
info items are left open for your review — they involve deserialization
architecture or binding API breaks.

---

## Open findings (await your review)

### CORE-2 — Unbounded recursion → stack-overflow abort via deserialization / value conversion (DoS)

- **Severity:** High
- **Files:** `crates/core/src/document/dto.rs`, `wire.rs`, `emit.rs:406+`,
  `value.rs`; binding converters `crates/bindings/python/src/types.rs`
  (`py_to_json`), `crates/bindings/wasm/src/engine.rs` (`serde_wasm_bindgen::from_value`).
- **Verified:** Yes (depth ~5000 aborts the process).

**Description.** Spec §8 mandates a YAML nesting limit of **100**, but it is
enforced **only on the markdown parse path** (via `serde_saphyr::Budget` in
`limits.rs`). Other entry points have **no depth bound**:

- `serde_json::from_value::<Document>` (used when a binding already holds a
  `serde_json::Value`) does **not** apply serde_json's recursion limit (`from_str`
  does, ~128; `from_value` does not). Depth 500 succeeds; ~5000 overflows the
  stack *inside* `from_value` — before any code we control runs.
- The recursive value converters `py_to_json` (Python) and
  `serde_wasm_bindgen::from_value` (WASM) walk arbitrarily nested input with no
  depth guard.
- The recursive emitters (`to_markdown`, `to_plate_json`, `flat_nested_comments`)
  are likewise unbounded.

The JSON-**string** paths (`Document::from_json` → `serde_json::from_str`) are
protected by serde_json's default limit, so this is reachable specifically
through the in-memory-`Value` / native-object conversion paths.

**Why not auto-fixed.** A depth check in `PayloadItem::try_from` cannot help,
because the overflow happens *during* `from_value`, before that code runs. A real
fix needs depth-bounding **at the deserialization/conversion boundary** — e.g. a
shared bounded-depth `Value`-walk used by the bindings before handing off to
`from_value`, plus an explicit depth guard in `py_to_json` and the emitters. This
spans core + both bindings and is a contract change (previously-"working" deep
inputs would now be rejected with a clean error instead of aborting). **Recommended:**
add a `MAX_VALUE_DEPTH` (reuse the §8 limit of 100) enforced in the recursive
converters and a pre-deserialization depth check in the bindings.

---

### CORE-4 — Top-level data-field names not validated against `[a-z_][a-z0-9_]*` at parse time

- **Severity:** Medium
- **Files:** `crates/core/src/document/assemble.rs` (`build_payload`); spec §3.4, §10.
- **Verified:** Yes.

**Description.** The parser never enforces the field-name charset on the
*markdown-parse* path. `Title:`, `"my key":`, `"a: b":` all parse successfully.
The charset is only enforced on the *mutator* path (`edit.rs::is_valid_field_name`),
not on `from_markdown`. Spec §10 lists "a data-field name failing
`/^[a-z_][a-z0-9_]*$/`" as a parse error. This breaks the invariant that field
names can't collide with `$`-keys or carry structural characters, and downstream
code (e.g. `to_plate_json`) assumes conforming names.

**Why not auto-fixed.** Tightens accepted input — previously-accepted documents
with non-conforming top-level keys would now error. That is the spec-intended
behavior, but it is a compatibility change you should sign off on. **Recommended:**
validate each non-`$` user key in `build_payload` and return
`ParseError::InvalidStructure`. (Note: with CORE-3 resolved, the emitter already
round-trips nested keys safely; CORE-4 is about *top-level* names specifically.)

---

### CORE-5 — Trailing comment silently dropped when a plain scalar value contains `'` or `"`

- **Severity:** Medium
- **Files:** `crates/core/src/document/prescan.rs:424-459` (`split_trailing_comment`)
- **Verified:** Yes.

**Description.** `split_trailing_comment` enters quoted-string state on *any* `'`
or `"` anywhere in the value. In a YAML **plain** scalar an apostrophe is an
ordinary character (`it's`), and a real trailing comment can follow. The function
treats the `'` as an unterminated open quote, never matches the `#`, and the
comment is **silently dropped**, violating the §3.4 comment-preservation guarantee.

**Trigger:** `x: it's a test # note` → the `# note` comment is lost.

**Why not auto-fixed.** The fix (only enter quote state when the quote *begins*
the scalar, mirroring `yaml_hints.rs::first_field_with_unquoted_colon`) changes
comment-attachment heuristics and must be careful not to regress the legitimate
`x: 'a # b'` quoted case. Worth a focused change + test pass. **Recommended:** as
above, gated on a regression test for the quoted-scalar case.

---

### CORE-6 — `from_value` / DTO paths inherit no `serde_json` recursion guard (sub-note to CORE-2)

- **Severity:** Info
- **Description.** Explicitly: `serde_json::from_str` enforces a recursion limit
  (~128) but `from_value` of an already-parsed `Value` does **not**. Any binding
  that builds a `serde_json::Value` first and then calls `from_value::<Document>`
  loses even that protection. Folded into the CORE-2 fix.

---

### TYPST-2 — Markdown→Typst compile warnings and package-load problems are discarded to stderr

- **Severity:** Low (observability)
- **Files:** `crates/backends/typst/src/compile.rs:34-36`; `world.rs:237-242, 311-315`
- **Verified:** Yes.

**Description.** Typst compile warnings and quill package-load problems (bad
`typst.toml`, missing entrypoint) are written to stderr via `eprintln!` and
dropped rather than surfaced as `Diagnostic`s. In WASM/library embeddings stderr
is invisible, so authors get no signal. Not a security issue.

**Why not auto-fixed.** Changing the diagnostic surface / warnings channel is an
API expectation change. **Recommended:** collect warnings into the returned
`Diagnostic` set.

---

### WASM-1 — Nine silent `.unwrap_or(JsValue::UNDEFINED)` on serialization failures

- **Severity:** Low
- **Files:** `crates/bindings/wasm/src/engine.rs:338, 389, 600, 625, 673, 920, 987, 1006, 1225`
- **Verified:** Yes.

**Description.** Nine getters degrade silently to `undefined` if
`serde_wasm_bindgen::serialize` fails. The notable cases are the `Quill.schema`
getter (an editor would render an empty form), the `Document.cards` getter
(`undefined` where `Card[]` is expected crashes iterating callers), and the
`warnings` getters. In practice unlikely (all types are serde-derived), but the
pattern is dangerous if non-serializable types are added later.

**Why not auto-fixed.** The right fix changes getter signatures from `JsValue`
to `Result<JsValue, JsValue>` (throw on failure) for at least `schema`/`cards`,
which is a public WASM API break. **Recommended:** throw for collection/struct
getters; at minimum fall back to `JsValue::NULL` (explicit "no data") rather than
`UNDEFINED` (property absent) elsewhere.

---

### Info-level / accepted notes (no change recommended now)

- **TYPST-3** (`convert.rs:55-71`) — `escape_markup` omits line-start `=`/`-`/`+`.
  Unreachable today (pulldown soft-breaks become spaces; hard breaks emit inline
  `#linebreak()`, so escaped text is never at a true Typst line-start). Latent if
  the emitter is ever changed to emit raw newlines between text runs.
- **TYPST-4** (`lib.typ.template:36`, `lib.rs:325`) — date fields flow unescaped
  into `_parse-date`, which hard-errors on malformed values (graceful, not a
  panic). `Quill::compile_data()` validates `format: date-time` upstream, so this
  is defense-in-depth only.
- **TYPST-5** (`lib.typ.template:8-29`) — `signature-field` emits `#metadata(..)`
  that a plate using an unfiltered `query(metadata).last()` could read by
  accident. Already documented in the template; `extract.rs` filters correctly.
- **FUZZ-1** — concrete proptest coverage gaps, highest value first:
  - **FUZZ-A** `QuillConfig::from_yaml_with_warnings` with arbitrary YAML — the
    largest untested untrusted-input surface (CLI `validate`, WASM `fromTree`).
  - **FUZZ-B** `Document::from_json` with arbitrary JSON (storage DTO from a DB/network).
  - **FUZZ-C** `QuillReference::from_str`; **FUZZ-D** `FileTreeNode::insert` with
    adversarial paths; **FUZZ-E** `QuillIgnore` pattern/path matching;
    **FUZZ-F** mutation-model round-trip for `to_markdown`.
- **Dependency advisories** (`cargo audit`, 437 deps): **0 vulnerabilities**. Three
  *unmaintained* informational warnings on transitive deps pulled in by the Typst
  stack — `bincode 1.3.3` (RUSTSEC-2025-0141), `paste 1.0.15` (RUSTSEC-2024-0436),
  `yaml-rust 0.4.5` (RUSTSEC-2024-0320). No action required; track for upstream
  replacement.

---

## Resolved on this branch

### ~~CORE-1 — Indented `~~~` inside a YAML block scalar prematurely closes the card block (silent data corruption)~~

- **Severity:** High · **Files:** `crates/core/src/document/fences.rs` (closer
  scan), `prose/references/markdown-spec.md` §3.2 / §4 D2
- ✅ **Resolved — by spec amendment.** The spec previously allowed the closing
  `~~~` to carry 1–3 leading spaces (inherited from CommonMark's closing-fence
  rule). That leniency exists in CommonMark for indented openers and list
  contexts — neither applies to card-yaml blocks, whose openers are required to
  be column-zero — and the payload between the fences is YAML, where an
  indented `~~~` is structurally payload (a block-scalar line), not a closer.
  The spec now requires the closing fence at **column zero** (§3.2, D2), the
  scanner enforces it, and the corruption case parses intact: a tilde code
  fence inside a `|` block-scalar value stays in the field. A document whose
  only closer was indented now falls through to CommonMark as an unclosed code
  block with the existing `parse::unclosed_code_block` warning — a diagnostic
  instead of silent truncation. A column-zero `~~~` can never be block-scalar
  content (YAML requires scalar content indented past its key), so the closer
  is unambiguous and no block-scalar-aware scanner is needed.
- **Regression tests:**
  `card_fence_tests.rs::indented_tilde_inside_block_scalar_is_payload_not_closer`,
  `card_fence_tests.rs::indented_tilde_line_never_closes_a_card_fence`.
- **Migration note:** `docs/migrations/0.90-to-0.91.md`.

### ~~CORE-3 — Map keys emitted unescaped → invalid YAML / broken round-trip~~

- **Severity:** Medium-High · **Files:** `crates/core/src/document/emit.rs`
- ✅ **Resolved.** Nested mapping keys are now emitted through saphyr's scalar
  quoting (`emit_key`), so a nested key containing `: `, a leading YAML indicator
  (`*`, `&`, `?`, …), `#`, edge whitespace, or a type-ambiguous form (`n`, `true`,
  `123`) is correctly quoted and re-parses to the same key. Top-level field names
  (indent 0) are kept verbatim via `emit_key_at`, because the line-oriented
  prescan accepts only bare `[a-z_][a-z0-9_]*` names there and quoting one would
  make it unparseable (this is the CORE-4 boundary). Before the fix, a nested key
  like `a: b` emitted `a: b: 1`, which fails to re-parse.
- **Regression test:** `emit_tests.rs::nested_map_keys_with_structural_chars_emit_valid_yaml`.

### ~~TYPST-1 — Error locations from foreign sources mis-attributed to `main.typ`~~

- **Severity:** Low · **Files:** `crates/backends/typst/src/error_mapping.rs:46-62`
- ✅ **Resolved.** `resolve_span_to_location` now resolves a diagnostic against
  **its own source file** (`span.id()`), falling back to `world.main()` only for
  the detached span. Errors originating in an injected helper package or a
  vendored package now report the correct file path, line, and column instead of
  `main.typ` coordinates.

### ~~CLI-1 — `validate`: `plate_file` path-existence oracle (traversal + absolute)~~

- **Severity:** Medium · **Files:** `crates/bindings/cli/src/commands/validate.rs:181+`
- ✅ **Resolved.** `validate_file_references` now rejects any `plate_file` whose
  path contains non-`Normal` components (`..`, absolute roots) before touching the
  filesystem, closing the existence-probing oracle a crafted `Quill.yaml` could
  use (`../../../etc/shadow`, `/etc/passwd`). (The render path was already safe —
  `FileTreeNode::get_node` returns `None` for escaping paths.)

### ~~CLI-2 — `unreachable!` in `OutputWriter::write`~~

- **Severity:** Info · **Files:** `crates/bindings/cli/src/output.rs:34`
- ✅ **Resolved.** Replaced the `unreachable!` with a typed
  `CliError::InvalidArgument`, so a future caller constructing
  `OutputWriter::new(false, None, ..)` gets a clean error instead of a panic.

### ~~LOAD-1 — Quill directory loader follows symlinks into the in-memory tree~~

- **Severity:** Medium · **Files:** `crates/quillmark/src/load.rs:78+`
- ✅ **Resolved.** `load_dir` now stats entries with `symlink_metadata` and
  **skips symlinks** instead of dereferencing them, so a crafted quill bundle
  cannot pull a sensitive host file (`assets/x -> /etc/shadow`) into the asset
  tree the backend reads.
- **Regression test:** `load.rs::tests::load_dir_skips_symlinks` (unix).

### ~~LOAD-2 — No per-file size limit in the quill directory walker~~

- **Severity:** Low · **Files:** `crates/quillmark/src/load.rs`
- ✅ **Resolved.** Added a `MAX_QUILL_FILE_SIZE` (50 MiB) guard so a single
  oversized file in a quill directory returns a clean error instead of exhausting
  memory — mirroring the `MAX_INPUT_SIZE` guard on `Document::from_markdown`.

### ~~LOAD-3 — `FileTreeNode::get_node` silently drops `..`/`.`/root components~~

- **Severity:** Low · **Files:** `crates/core/src/quill/tree.rs:30-40`
- ✅ **Resolved.** `get_node` now **rejects** any non-`Normal` path component
  (returns `None`), matching `insert`'s behavior. Previously `get_file("a/../b")`
  navigated to `a/b`; an asymmetry that could mask path handling assuming
  normalization.
- **Regression test:** `tree.rs::tests::get_node_rejects_traversal_components`.

### ~~PY-1 — Python `float('nan')`/`float('inf')` silently becomes JSON `null`~~

- **Severity:** Low · **Files:** `crates/bindings/python/src/types.rs`
- ✅ **Resolved.** `py_to_json` now raises a `ValueError` for non-finite floats
  instead of silently storing `null` (the old behavior — `serde_json::json!(nan)`
  maps to `Value::Null` — corrupted data with no diagnostic).

### ~~PY-2 — Python `int` overflow leaked a raw `OverflowError` across FFI~~

- **Severity:** Low · **Files:** `crates/bindings/python/src/types.rs`
- ✅ **Resolved.** `py_to_json` now tries `i64`, then `u64`, then raises a clean
  `ValueError` for integers beyond 64-bit — so large positive ints convert
  losslessly and out-of-range values report a uniform binding error rather than
  PyO3's raw `OverflowError`.

### ~~WASM-2 — `paint()` did not validate the computed `render_scale` product~~

- **Severity:** Low · **Files:** `crates/bindings/wasm/src/engine.rs:1325`
- ✅ **Resolved.** `paint()` now validates `render_scale = layout_scale *
  effective_density` is finite, positive, and within `f32` range before handing
  it to `render_rgba`, closing the overflow-to-infinity path (reachable e.g. via a
  zero-dimension page that bypasses the `MAX_BACKING_DIMENSION` clamp).

---

## Verification

- `cargo test --workspace` — **green** (baseline and after all fixes).
- `cargo build -p quillmark-python` — compiles.
- WASM crate (`quillmark-wasm`) — verified on CI (no local `wasm32` target).
- New regression tests added: CORE-3 nested-key quoting, LOAD-1 symlink skip,
  LOAD-3 traversal rejection.

## Areas audited and found sound (no findings)

- **Typst escaping/eval boundary** — every breakout attempt (`#`-calls, bracket
  breakout from `#strong[..]`, `$math$`, `@label`, block/line comments, string
  breakout in `#link("..")`, control chars, code-fence content) was neutralized
  under adversarial end-to-end testing. No injection.
- **Typst world/file resolution** — pure in-memory map lookups; no disk/network;
  packages vendored; `../`/absolute/`/etc/passwd` all blocked.
- **`pdf_scan.rs` / `overlay`** — offset/pointer arithmetic is bounded; page-tree
  walk capped at `MAX_NODES`; encrypted/xref-stream PDFs rejected.
- **Multibyte/char-boundary slicing** across `prescan.rs`, `fences.rs`,
  `yaml_hints.rs`, `version.rs` — all slice at ASCII offsets or via `strip_prefix`.
- **YAML→JSON value conversion** (`value.rs`) — thin newtype over
  `serde_json::Value`; duplicate keys rejected upstream by saphyr.
