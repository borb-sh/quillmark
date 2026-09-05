# Contributing to Quillmark

## Documentation

No comment is the default. Prose is added back only where a reader cannot get the fact from the code:

- Public API — `pub` items of a published crate, TypeScript declarations, `.pyi` stubs, READMEs — earns a tight paragraph: the contract, the meaning the type does not carry, errors, panics. One example, where the call shape is not obvious.
- Internal items earn a line at most, and only for the non-obvious *why*: a workaround, an ordering constraint, a spec citation, an invariant a reader would otherwise violate.
- Delete comments that restate the code; prefer a clearer name over a comment. Never enumerate a module's items or behaviors — the hand-list rots.
- No marketing words (powerful, seamless, robust, first-class, simply…). State the capability plainly.
- Describe what *is*, not how it got here: except `docs/migrations/` (era-stamped) and load-bearing legacy.

Where things live:

- API docs: standard in-line Rust doc comments (`///`).
- Canonical design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md).
- User guide: `docs/` (rendered by mkdocs).
- Full style rubric and review pass: the `dense-prose` skill (`.claude/skills/dense-prose/`); `maintain-canon` covers canon structure.

## Tests

A test earns its place by exercising logic that can break: parsing, assembly, emission, diffing, resolution, validation, error classification, round-trips, rendering. It does not earn a place by restating a rule the implementation or canon already carries.

- No spec-pinning. Diagnostic codes, wire-format keys, and public API names are contract; human-readable error prose, derive output, and constant literals are not.
- No duplicates. One strong test beats three angles on the same behavior.
- Assert the property, not the artifact. Byte counts, hashes, and full-string snapshots of rendered output rot; narrow to what matters.
- The test name is its documentation. A regression test states the invariant guarded, not the bug's history.

Deleting a stale test is maintenance, not loss — coverage can be added back when something real needs guarding.

## Commits and releases

The changelog is seeded from commit subjects, so the `!` marker is where a break is first recorded and the last place to catch a missing one.

- **`!` marks an observable-contract shift, not only a type change.** A region address, a wire token, a diagnostic code, a rendered value — anything a working consumer reads that stops meaning what it meant. A break a type checker cannot report is the one that most needs the marker.
- **A release carrying a `!` ships `docs/migrations/<prev>-to-<next>.md`**, and a row in `docs/migrations/index.md` and `mkdocs.yml`'s `not_in_nav`. Lead the guide with the breaks no type checker reports; the changelog is organized by feature, which is the wrong index for an upgrade.

## Binding tests

**WASM:** repo root → `./scripts/build-wasm.sh` → `cd crates/bindings/wasm` → `npm install` (first time) → `npm run test`

**Python:** `cd crates/bindings/python` → `uv venv` → `source .venv/bin/activate` → `uv pip install maturin pytest` → `maturin develop` → `pytest`

`maturin develop` builds the extension in the debug profile. Stay in the activated venv: `uv sync`, `uv run`, and `pip install -e` each sync the project through maturin's PEP 517 backend, a release build that replaces the debug `.so`.