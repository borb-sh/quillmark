# crates/bindings/wasm, crates/backends/pdfform

## Low risk

### wasm/src/engine.rs:1501, wasm/src/types.rs:279, wasm/src/types.rs:310 — the u64→u32 revision narrow is written three times

`.try_into().unwrap_or(u32::MAX)` with a duplicated rationale comment in the
`revision` getter, `FieldRegion::from`, and `CorpusHit::from`. Fix: one
`pub(crate) fn revision_u32(v: u64) -> u32` in types.rs carrying the comment
once.

### wasm/src/engine.rs:1557, wasm/src/engine.rs:1587 — two copy-pasted rollback sites in `applyFieldDelta`

Both exits repeat `*doc.inner.main_mut() = pre_edit_main; return Err(...)`; a
third fallible step needs a third rollback and the sites can drift. Fix: run
splice + recompile chain in one fallible block with a single
rollback-and-return on `Err`. Superseded if the transactional-callee fix in
`seams.md` lands.

### wasm/src/engine.rs:1553 — `applyFieldDelta` snapshots the whole main card

`doc.inner.main().clone()` copies the full payload + body corpus per
keystroke-level delta although only the body is mutated by the fallible steps.
Fix: snapshot the body alone and restore via the body setter — after verifying
`apply_body_change` touches nothing else on the card. Also superseded by the
transactional-callee fix.

### wasm/src/engine.rs:1474 vs wasm/src/engine.rs:1568 — the compile preamble exists twice

`applyFieldDelta` repeats `apply`'s `check_quill_reference` → `compile_data`
sequence with the same `WasmError`→JS mapping, once as `?`-statements and once
as an `and_then` chain. Fix: a private `compile_checked` helper used by both
verbs.

### pdfform/src/resolve.rs:160 — `coerce_text` copies `element_text`'s scalar arms

The String/Number/Bool/Object arms are verbatim copies differing only in
`Option` wrapping, so the scalar-to-text rule lives twice (top-level vs array
path). Fix: delegate every non-array value to `element_text`; keep only the
array-join + emptiness handling.

## Needs judgment

### wasm/src/engine.rs:1240 — `js_to_card`'s `ALLOWED` list shadows `CardWire`'s field set

The local string list exists because `serde_wasm_bindgen` ignores
`deny_unknown_fields`, and it has already diverged from serde's accepted set
(serde accepts the `payload_items`/`body_markdown` aliases; `ALLOWED` rejects
them). Adding a `CardWire` field requires remembering this list. Fix:
deserialize through `serde_json::Value` → `CardWire`, which honors
`deny_unknown_fields` and matches the Python binding
(python/src/types.rs:1230) — a small accepted-input change (aliases become
valid), so pin with a test rather than auto-applying.

### pdfform/src/resolve.rs:197 — richtext detected by parse success instead of schema classification

Every JSON `Object` is sniffed with `from_canonical_value(v).ok()?` (also
lines 169, 190), while the typst backend classifies the same seam value
structurally via the transform schema's
`contentMediaType: application/quillmark-richtext+json`
(typst/src/lib.rs:627, `SchemaMeta.content_fields`). A plain `type: object`
field bound to a text widget renders text or blank depending on whether its
shape happens to validate as a corpus, and any change to richtext
identification must be re-implemented here with different logic. Fix:
pdfform's `open` already holds the Quill — derive the richtext-classified
paths from `build_transform_schema` as typst does; keep `from_canonical_value`
as the decode, not the detector.
