# Bookmarks

Quick notes and ideas not yet worth a fleshed-out proposal. Terse capture
only — no commitment, no detailed plans. Promote an item to `proposals/` when
it is ready to be designed.

## wasm API gaps

Surfaced from consumer-perspective audits of the wasm surface.

- **Standalone validator.** Core has `QuillConfig::validate_document`, but the
  only wasm path to "is this document valid?" is `quill.form(doc)` read for its
  diagnostics. Expose a direct `quill.validate(doc) -> Diagnostic[]`.
- **Typed field types in the schema payload.** `FieldType` serializes to bare
  strings (`string`, `integer`, `object`, …); the wasm `.d.ts` advertises named
  return types that aren't defined, so they collapse to `any`. Ship a real
  discriminated-union type, or document a hand-written interface.
- **Schema-blind `setField` / `setFill`.** Both accept any JSON value and never
  consult the field's declared type or enum; mismatches surface only at the
  next `form` call. Per-field diagnostics on set would close the loop.
- **Unstable diagnostic codes.** Diagnostic `code` strings are bare literals
  with no exported enum, constants, or stability guarantee. Consumers keying
  off them are one refactor from breaking. Ship a documented code set.
- **Sync render, no cancellation.** `quill.render` blocks the wasm thread with
  no `AbortSignal` or progress callback. A `renderAsync(doc, { signal })` would
  let consumers wrap timeouts cleanly.
- **No capability discovery.** `new Quillmark()` exposes no version, backends,
  or supported formats; availability is inferred by loading a quill. A static
  `Quillmark.capabilities()` would fix it.
