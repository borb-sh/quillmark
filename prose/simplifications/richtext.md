# crates/richtext

## Needs judgment

### ops.rs:352 — line removal is quadratic under large deletions

`sync_lines_for_delta` calls `lines.remove(line_idx + 1)` once per deleted
`\n` — O(n) each, quadratic for a select-all-delete on a large body. A
single-pass cursor rewrite is possible but the interleaving of retain/insert
with the template-clone rule has edge cases on malformed corpora; semantics
need pinning by tests before restructuring.

### ops.rs:221 — `apply_field_change` normalizes three times

`apply_text_delta`, `apply_line_ops`, and `apply_mark_ops` each end with
`self.normalize()`, so a committed edit bundle pays 3× (mark sort + full-text
char collection + island props rebuild). Fix: internal non-normalizing apply
steps with one `normalize()` at the bundle end. Needs care: the public
single-channel methods must keep normalizing.

### model.rs:333, serial.rs:388 — every `normalize()` rebuilds all table-cell JSON

`normalize_table_cell_marks` parses and rebuilds each cell (`parse_cell` +
`cell_to_value`) and `sorted_value`-clones every island's `props` tree even
when a pure text splice touched no island — O(total island props) per
keystroke on any corpus containing a table. Fix: skip the island pass unless
islands were touched (dirty flag on island-mutating paths), or verify-sorted
before rebuilding.

### model.rs:334, model.rs:448 — `island_type == "table"` string dispatch in three places

`normalize` and `validate` each string-match `"table"` to reach into props via
`serial`, and export's `emit_island` dispatches on the same tag independently.
Islands are an open set: a new mark-carrying island type requires coordinated
edits in three files, and missing one silently voids the canonical-bytes
guarantee for that type. Fix: one island-type dispatch table in `serial.rs`
exposing per-type normalize/validate/emit hooks.

### ops.rs:111, delta.rs:99 — short-delta leniency lives at the wrong altitude

`extend_to_base` pads abbreviated deltas inside `apply_text_delta` to
accommodate one producer (the WASM `applyFieldDelta` path), leaving `Delta`
with three application semantics: `apply` clamps, `try_apply` is strict,
`apply_text_delta` pads-then-checks — while `map_pos` separately implements
implicit trailing retain. The change log records the *unpadded* delta, so a
future consumer replaying entries via strict `try_apply` (undo, sync) gets
`BaseLengthMismatch` for edits that succeeded. Fix: normalize abbreviated
deltas at the boundary where they enter (the WASM delta deserializer), or make
implicit trailing retain the documented contract of `try_apply` itself.

### import.rs:512 — the `MarkdownFixer` erases the `<u>`/`Strong` distinction it owns

The fixer rewrites `<u>`/`</u>` into `Strong` events; both downstream consumers
(`Tag::Strong` at import.rs:526 and the table-cell path at import.rs:689) then
re-sniff raw source bytes via `strong_or_underline` to recover the distinction.
That peek's 2-byte `<u`-prefix test and the fixer's `is_u_open_tag` (trim +
inner `== "u"`) are two hand-synced encodings of one rule that classify a tag
like `<ul>` oppositely; only the fixer's gate — which never converts `<ul>` to
`Strong` — keeps that divergence from reaching the peek today. Fix: the fixer
emits the distinction explicitly (wrapper event or `MarkKind`), deleting both
peeks. The shared `strong_or_underline` helper already narrows the drift but not
the altitude — the distinction is still recovered from source bytes rather than
carried.

### serial.rs:115 — `to_canonical_value` still clones + normalizes unconditionally

The double **tree** build is gone: `to_canonical_value` now finishes with
`sort_keys_owned(rt.to_value())`, which reorders keys by moving each entry
rather than deep-cloning every leaf (the `text` string, mark attrs, arrays)
like `sorted_value` did. Remaining: it still `clone()`s + `normalize()`s the
corpus even when the caller (live `Document` bodies, invariantly normalized)
is already canonical. Fix: a no-clone path for known-normalized values —
interacts with the seam round-trips (see `seams.md`), so it lands with that
work.
