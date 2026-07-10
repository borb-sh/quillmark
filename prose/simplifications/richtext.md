# crates/richtext

## Needs judgment

### ops.rs:352 — line removal is quadratic under large deletions

`sync_lines_for_delta` calls `lines.remove(line_idx + 1)` once per deleted
`\n` — O(n) each, quadratic for a select-all-delete on a large body. A
single-pass cursor rewrite is possible but the interleaving of retain/insert
with the template-clone rule has edge cases on malformed corpora; semantics
need pinning by tests before restructuring.

### ops.rs:221 — `apply_field_change` normalizes three times (behavior-load-bearing)

`apply_text_delta`, `apply_line_ops`, and `apply_mark_ops` each end with
`self.normalize()`, so a committed edit bundle pays 3× (mark sort + full-text
char collection + island props rebuild). The obvious fix — non-normalizing
inner steps and one `normalize()` at the bundle end — is **not**
behavior-preserving, so it is deferred until the semantics are pinned:

- `normalize` unions adjacent same-kind marks (`[0..3]`+`[3..5]`→`[0..5]`), and
  `apply_mark_ops`'s `Remove` matches by `ranges_overlap` (half-open). With the
  intermediate normalize, `Remove{3..5}` overlaps the whole union and clears
  `[0..3]` too; without it, only `[3..5]` is removed. The normalize between text
  and mark ops changes which marks a subsequent `Remove` matches.
- `split_line`/`join_line` insert/delete `\n` in the text **without** remapping
  marks, and `normalize`'s formatting-edge trim is `\n`-position-sensitive, so
  normalizing on the pre-line-op text vs the post-line-op text can trim
  different edges.

Fix: give `apply_field_change` a real op-level model (remap marks across line
ops; define `Remove`-vs-union order) and pin it with tests, then collapse to one
terminal normalize.

### model.rs:333 — `normalize()` re-canonicalizes table cells even on a pure text splice

Half done: `normalize` now guards the `sorted_value` clone of each island's
`props` (and each unknown mark's `attrs`) behind `is_value_key_sorted`, so an
untouched, already-sorted tree pays a scan rather than a deep clone on the
common per-keystroke path. Remaining: `normalize_island_cell_marks` still
`parse_cell` + `cell_to_value`-rebuilds every table cell each `normalize`, even
when no island was touched — and under `serde_json` `preserve_order` that
re-inserts cell keys unsorted, so the guard above then does rebuild a table's
props. The real fix is a dirty flag on the island-mutating paths
(`sync_islands_for_delta`, cell edits) so a text-only edit skips the island
pass entirely; deferred for the correctness care that flag needs.

### island_type mark dispatch — normalize/validate consolidated; emit sites remain

`normalize` and `validate` no longer string-match `"table"`: both route through
`serial::normalize_island_cell_marks` / `serial::island_cell_marks`, which share
a single `island_is_mark_carrying` predicate — a new mark-carrying island type
is one edit in `serial.rs`, and neither pass can silently skip it (voiding the
canonical-bytes guarantee). Left as-is: richtext export's `emit_island`
(export.rs:318) and the typst backend's `island_markup` (emit.rs) each dispatch
`island_type → renderer` independently. These are HTML/Typst emit sites, not
codec logic; folding rendering into `serial.rs` would cross a layer. A full
per-type hook registry is deferred until a second mark-carrying type exists —
at cardinality one it is machinery without payoff.

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

### serial.rs:115 — `to_canonical_value` still clones + normalizes unconditionally

The double **tree** build is gone: `to_canonical_value` now finishes with
`sort_keys_owned(rt.to_value())`, which reorders keys by moving each entry
rather than deep-cloning every leaf (the `text` string, mark attrs, arrays)
like `sorted_value` did. Remaining: it still `clone()`s + `normalize()`s the
corpus even when the caller (live `Document` bodies, invariantly normalized)
is already canonical. Fix: a no-clone path for known-normalized values —
interacts with the seam round-trips (see `seams.md`), so it lands with that
work.
