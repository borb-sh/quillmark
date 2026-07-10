# crates/core

## Needs judgment

### quill/compose.rs:359 — corpus companion caches leak maybe-populated state to callers

`default_corpus`/`example_corpus` are populated only by a loader post-pass
(config.rs:1580), so each consumer carries its own fallback: `resolve_value`
falls through to the raw markdown `default` — which then crosses the seam
un-imported, since `resolve_fields` runs after coercion in `compile_data`, so
the "seam carries corpus" invariant silently breaks for a schema built outside
the loader — and seed.rs:88 implements a three-tier lookup with
`unwrap_or_else(RichText::empty)` swallowing failures. Fix: enforce at
construction (populate the companions in `FieldSchema` construction) or expose
an accessor that computes on cache miss, so the invariant lives in the type.

### session.rs:210 — `record_field_*_at` twins are forward surface without a consumer

`record_field_delta_at`/`record_field_change_at` have no production caller —
the wasm delta path uses `ensure_base_revision` + `apply_for_field_delta`
(which records via `change_log.record` directly). They are kept deliberately:
they are the only recompile-independent record primitive, they are documented
as the write twins of the public `import_body_delta`/`apply_body_change`
document mutators (edit.rs), and five session tests exercise the
map_pos/stamping/invalidation behavior through them on `PlainHandle` (whose
`apply` is unsupported, so `apply_for_field_delta` cannot substitute). Cut only
once a delta-transport consumer settles the shape; the zero-consumer
`change_log()` accessor and `FieldChange` re-export are already gone.
