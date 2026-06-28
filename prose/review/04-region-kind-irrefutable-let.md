# 04 — Irrefutable `let` on the extensible `RegionKind`

**Severity:** minor **Category:** smelly (future breakage) **Status:** Open

**Location:** `crates/quillmark/examples/pdfform_preview.rs` — the
`let quillmark_core::RegionKind::Field { field_type, value } = &region.kind;`
destructuring (around line 66; verify current line).

## Finding

The example destructures `RegionKind` with an **irrefutable** `let`:

```rust
let RegionKind::Field { field_type, value } = &region.kind;
```

This compiles today only because `RegionKind` has exactly one variant. The moment
a second variant is added, this becomes a hard compile error
(`refutable pattern in local binding`).

## Why it matters

`RegionKind` was deliberately made an **enum from day one** (per the design doc)
precisely so new region kinds can be added later. This `let` is a latent
tripwire that will break the build on exactly the future the enum exists to
support — and it sits in an example, which is the first code a new contributor
reads and copies.

## Fix

Use a refutable form so adding a variant is non-breaking at this site:

```rust
if let RegionKind::Field { field_type, value } = &region.kind {
    // …
}
```

or a `match` with an explicit arm. Prefer `match` if the example should
illustrate handling multiple kinds.

## Notes

- Cheap; bundle with the other low-risk cleanups (#05).
- Worth a quick grep for the same pattern elsewhere
  (`rg "let .*RegionKind::Field"`) in case other call sites copied it.
