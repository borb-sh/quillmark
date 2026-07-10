# Cross-crate: the richtext value seam and the delta-commit protocol

All entries need judgment — each spans crates or changes a contract.

### ~~Four sites re-implement the dual-shape richtext decode~~ (three unified)

Done for the three strict sites. `document::decode_richtext_value` (next to
`import_body`) is now the one place the "JSON object → `from_canonical_value`,
string → `import_body`" dispatch lives; `body_from_wire`, `literal_corpus`, and
the `validation.rs` backstop call it and keep only their deliberate local
handling — wire's `null` → empty and shape error, literal's per-encoding
message prefixes (via the `RichtextDecodeError` variant), validation's
`.ok()`-swallow. All three now reach markdown through the single `import_body`
boundary rather than three raw `from_markdown` calls.

The coercion branch (`config.rs`) stays open-coded on purpose: its string path
is deliberately lenient (reduce a scalar / length-1 array to text before
import), which the strict decoder must not do — flagged in a comment beside it.
The inline-violation surfaces (`CoercionError`, `ValidationError::NotInline`,
`richtext_inline_error`) are intentionally per-layer error types over the one
shared `rt.is_inline()` condition, so they are left distinct.

### Every apply round-trips each richtext value through the canonical codec three times

Per `apply`/`applyFieldDelta`: `to_data_json` serializes the
already-normalized body via `to_canonical_value` (clone + normalize + build
tree + `sorted_value` rebuild — `core/src/document/mod.rs:346`), coercion
re-parses with `from_canonical_value` and re-serializes
(`core/src/quill/config.rs:382`), and backend codegen parses a third time
before `emit_richtext` (`backends/typst/src/helper.rs:180`). ~3 full
parse/serialize + normalize passes over every corpus per keystroke preview
recompile. Fix: treat a corpus emitted by `to_canonical_value` as already
canonical across the internal seam — skip the coercion re-parse/re-serialize
for object inputs after validation, or carry `RichText` values through the
pipeline and serialize only at the backend boundary. Compounds with
`to_canonical_value`'s double tree build (richtext.md).

### The delta-commit orchestration lives in the WASM binding

The phase gate (`field != "$body"`), quill-ref check, `compile_data`
sequencing, `apply_for_field_delta`, and document rollback — ~60 lines of
transactional protocol — sit in `wasm/src/engine.rs:1531`, while the
`quillmark` orchestration crate (which owns the analogous `open`/`render`
seams and holds both `Quill` and `Document`) has no delta-commit helper. The
Python binding and any future consumer must replicate the protocol, and when
delta targets extend beyond `$body`, every binding is edited in lockstep. Fix:
a core/quillmark-level
`apply_field_delta(session, config, doc, field, base_revision, delta)` owning
field-address routing and the transaction; bindings become thin wrappers.

### ~~Transactionality is the caller's job~~ (done for the corpus apply)

`RichText::apply_field_change` is now all-or-nothing: a multi-op bundle stages
on a scratch copy and swaps on success (`richtext/src/ops.rs`), and
`Card::apply_body_change` documents the same contract, so no consumer needs to
snapshot-and-restore around a *half-applied body*. The WASM binding still holds
one body snapshot (`wasm/src/engine.rs`), but only as the transaction boundary
for the compile and record steps that run *after* the (now atomic) body
mutation and can still fail — which is the separate "delta-commit orchestration
lives in the WASM binding" seam above, not corpus transactionality. Close that
seam (a core/quillmark-level `apply_field_delta` owning apply + compile + record
+ rollback) and the caller-side snapshot goes with it.
