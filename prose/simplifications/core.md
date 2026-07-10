# crates/core

## Low risk

### quill/types.rs:344 — dead binding in `resolve_richtext_inline`

The `(other, Some(_))` arm binds `other` only to discard it with
`let _ = other;`. Fix: `(_, Some(_)) => Err(...)`.

### quill/config.rs:396 — the RichText coercion branch re-implements the String leniency cascade

Bare string → length-1 array unwrap → `scalar_as_string` appears in the String
branch (config.rs:337) and again in a slightly different shape in the RichText
branch — two copies of the "reduce a lenient value to a string" rule that can
drift. Fix: a shared `lenient_string(&Value) -> Option<String>` used by both,
error construction staying at the call sites.

### quill/config.rs:964 — `field_parse_hint` duplicates the deserializer's migration message

The hint string-matches raw YAML for `type: "richtext(inline)"` and re-emits
verbatim the message `FieldType`'s custom `Deserialize` already produces
(types.rs:676), so the diagnostic carries the text twice and the pattern
invites a third copy for the next retired token. Fix: let the deserializer's
error be the single source, or share one `const`.

## Needs judgment

### quill/types.rs:117, quill/types.rs:228 — the richtext `inline` flag has two carriers

Stored on `FieldType::RichText { inline }` and again as `FieldSchema.inline`,
synced only inside `from_quill_value` (`resolve_richtext_inline`). The derived
`Deserialize` on `FieldSchema` does not sync: `{type: "richtext", inline:
true}` deserializes to `RichText { inline: false }` + `inline: Some(true)`,
silently dropping the constraint for every `FieldType`-matching consumer
(coercion, validation, blueprint, transform schema). The "inline only on
richtext" rule is also enforced twice with different error surfaces
(types.rs:344 parse error vs config.rs:560 `quill::inline_not_supported`).
Fix: one representation — `inline` only on `FieldSchema` with `FieldType` a
plain token, or a custom (de)serialize deriving the struct field from the
enum. Public API shape.

### quill/config.rs:560 — `inline_not_supported` is unreachable through the loader

Every `FieldSchema` reaching `validate_field_schema_shape` comes from
`from_quill_value`, which already rejects `inline` on non-richtext types; no
test or code references the error code. A second, differently-worded
enforcement of one rule. Reachable in principle via derived-`Deserialize`
schemas, so deleting trades away defense-in-depth — pick one enforcement
point deliberately. Collapses to nothing if the two-carrier finding above is
fixed.

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

### session.rs:195, session.rs:210 — public session API with no consumer

`change_log()` and the `FieldChange` re-export (session.rs:4, lib.rs:47) have
zero uses; `record_field_delta_at`/`record_field_change_at` are copy-paste
twins exercised only by one unit test — the wasm delta path uses
`ensure_base_revision` + `apply_for_field_delta`. Three public methods plus a
re-export to keep semver-stable for a client that does not exist. Fix: keep
only the used surface; add record/bundle variants when a consumer appears.
Possibly deliberate forward surface — decide, then either wire a consumer or
cut.

### document/edit.rs:448 — `Card::set_body_corpus` is a public alias of `overwrite_body`

One line, no consumer outside core's own unit tests (wasm mutates bodies via
`apply_body_change`; storage builds cards via `from_parts`); a second public
write path to the body corpus. lib.rs docs reference it and it may be intended
editor API — decide, then cut or document as the canonical setter.
