# 03 — WASM region type: `skip_serializing_if` without `serde(default)`

**Severity:** minor **Category:** correctness (latent) **Status:** Open

**Location:** `crates/bindings/wasm/src/types.rs:245` (the `value` field on the
`FieldRegionKind::Field` / `FieldRegion` Tsify type)

## Finding

The wasm `FieldRegion` kind carries
`#[serde(skip_serializing_if = "Option::is_none")]` on its `value: Option<String>`
field, but **no matching `#[serde(default)]`**. The type is annotated
`#[tsify(into_wasm_abi, from_wasm_abi)]`, so Tsify generates a `from_wasm_abi`
(JS→Rust) deserialization path as well as the serialization path.

When `value` is `None`, serialization omits the key entirely. If that same JSON is
ever fed back through `from_wasm_abi`, serde errors with `missing field 'value'`
because there is no default to fall back on.

## Why it matters

- Today regions are **output-only** (Rust→JS), so this never fires in practice.
- But the **bidirectional** Tsify annotation advertises a round-trip that is
  silently broken for the blank-value case. Any future "accept/edit a region"
  API that deserializes a `FieldRegion` back into Rust would hit
  `missing field 'value'` for every unbound field — the common case.

## Fix

Add `#[serde(default)]` alongside the existing `skip_serializing_if` on the
`value` field. One line; makes the declared round-trip actually total.

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub value: Option<String>,
```

## Notes

- Mirrors a deliberate guard already present on the **core** side: the
  `blank_field_value_is_null` test in `crates/core/src/region.rs` exists
  specifically to keep core's `value` serializing as `null` (not omitted). The
  wasm wrapper chose the opposite (omit) without the compensating `default`, so
  the two layers treat blank values differently — worth aligning while here.
