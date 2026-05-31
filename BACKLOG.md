# Backlog — `rework/fill-strategy` PR review

Review of the `rework/fill-strategy` → `main` PR (7 commits, 23 files), the
**zero-filled render / blueprint-example split** rework.

## Verdict

The PR is **clean and well-executed**. Verified on this branch:

- `cargo build --workspace`: clean (0 errors, 0 warnings).
- `cargo test --workspace`: **941 passed, 0 failed**, 0 panics.
- `cargo clippy --workspace --all-targets`: 0 errors; the 16 warnings present
  are **all pre-existing**, in files (or unchanged regions) this PR never
  touched — see item 3.
- The removed public API (`FillBehavior`, `QuillConfig::blueprint_filled`) has
  **no stale references** left in code — only the proposal docs mention the old
  names, and only where they describe what is being replaced.
- The `print_blueprint` example calls `.blueprint()` (not the removed API), so
  it is **not** broken by the rename.
- Core logic is sound: `resolve_fields` resolves `authored › default › zero`
  for the plate-JSON projection only; `coerce_and_validate` correctly demotes
  `validation::must_fill_absent` (verified the exact code string and the
  `ValidationError::code()` method) while keeping `must_fill_sentinel` and
  coercion failures fatal. No dead code left behind (`must_fill_value` /
  `first_enum` were folded into the shared `zero_value` producer).

No correctness bugs, dead logic, or redundant tests were found in the PR's
changes, so no in-scope auto-fixes were applied. One out-of-scope change made
during review (an automatic `cargo clippy --fix` that touched pre-existing
warnings in non-PR files) was **reverted** to keep the diff focused.

The items below are follow-ups, not blockers.

---

## 1. Proposal docs say "not yet implemented" but this PR implements the core (medium)

`prose/proposals/zero-filled-render.md:25` and
`prose/proposals/blueprint-example-split.md:21` both open with:

> Pre-1.0; not yet implemented. When built, … graduates into …

This PR **does** implement the core of both: zero-filled render
(`resolve_fields` in `crates/quillmark/src/orchestration/quill.rs`), the
`FillSource` demotion, and `QuillConfig::example()`. `prose/canon/BLUEPRINT.md`
already partially graduated the split (its new "Two reference documents"
section), but:

- the proposals still declare themselves unimplemented, and
- `prose/canon/SCHEMAS.md` did **not** receive the "Zero-filled render" section
  the proposal names as its graduation target.

**Why backlog (not auto-fixed):** each proposal deliberately scopes deferred
sub-features (warnings surface, standalone completeness-query API), so whether
to graduate now (fold into canon + delete the proposal) or keep the proposal
until the deferred work lands is an author/workflow decision, not a mechanical
edit. Decide and either (a) update the status line to "partially implemented —
core landed, warnings/completeness deferred" or (b) graduate the implemented
concepts into `SCHEMAS.md`/`BLUEPRINT.md` and trim the proposals.

## 2. `must_fill_absent` doc phrasing may mislead post-rework (low)

These describe `validation::must_fill_absent` firing "if the field is absent at
validate time" without noting that the **render path now tolerates absence**
(zero-fills it):

- `crates/core/src/quill/types.rs:166`
- `crates/bindings/wasm/src/engine.rs:41`
- `docs/format-designer/quill-yaml-reference.md` (the `default` row)

The statements are *layer-accurate* — `validate_document` still emits the code
(the form view consumes it for per-field "doneness") — so they are not wrong,
but a reader could infer that an absent Must Fill field fails rendering, which
is exactly what this rework changed. The format-designer reference was edited
in this PR and kept the old phrasing, so a clarifying one-clause addition
("…at validate time; the render path zero-fills an absent field rather than
failing") is left to the author rather than imposed during review.

## 3. Pre-existing clippy debt outside this PR (low)

`cargo clippy --workspace --all-targets` reports 16 warnings, none introduced
by this PR:

- `crates/backends/typst/src/pdf_scan.rs:312,394` — `unnecessary_map_or`
- `crates/backends/typst/src/world.rs:509` — `only_used_in_recursion`
- `crates/bindings/cli/src/commands/render.rs:129,141` — `redundant_closure`,
  `io_other_error` (unchanged regions)
- `crates/bindings/cli/src/commands/validate.rs:187` — `ptr_arg` (`&PathBuf`)
- `crates/core/src/document/tests/ambiguous_strings_tests.rs:78,205` —
  `explicit_auto_deref`
- `crates/core/src/quill/validation.rs:1143` — `to_string_in_format_args`
- `crates/core/src/quill/tests.rs:1896,1964` — `unnecessary_map_or`

Mechanical cleanup pass (`cargo clippy --fix`) in a dedicated, separate commit
— intentionally kept out of this PR's diff.

## 4. Deferred features acknowledged by the PR (tracking)

So they are not lost when the proposals are eventually deleted:

- **Warnings surface** for absent Must Fill fields (today absence is
  zero-filled silently on the render path).
- **Standalone strict-completeness query API** + any finalize/publish gate
  (today the form's `FormFieldSource::Missing` carries the doneness signal).
- Verify the `FillSource` mention picked up by grep in
  `docs/migrations/0.83-to-0.84.md` is historical/accurate (point-in-time
  migration note), not a forward reference to this PR's new internal type.
