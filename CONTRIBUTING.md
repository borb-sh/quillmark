# Contributing

Build and test loops, and the crate map: [`CLAUDE.md`](CLAUDE.md). Design docs:
[`prose/canon/INDEX.md`](prose/canon/INDEX.md). Prose style is the `dense-prose`
skill (`.claude/skills/dense-prose/`), canon structure is `maintain-canon`.

The two rules below live here because nothing else states them.

## What earns a test a place

Exercising logic that can break: parsing, assembly, emission, diffing,
resolution, validation, error classification, round-trips, rendering. Restating
a rule the implementation or canon already carries earns nothing, and deleting a
stale test is maintenance.

- Diagnostic codes, wire-format keys, and public API names are contract;
  human-readable error prose, derive output, and constant literals are not.
- One strong test beats three angles on the same behavior.
- Assert the property, not the artifact: byte counts, hashes, and full-string
  snapshots of rendered output rot.

## What the `!` marker obliges

The changelog is seeded from commit subjects, so `!` is where a break is first
recorded and the last place to catch a missing one.

- `!` marks an observable-contract shift, not only a type change. A region
  address, a wire token, a diagnostic code, a rendered value — anything a
  working consumer reads that stops meaning what it meant. A break a type
  checker cannot report is the one that most needs the marker.
- A release carrying a `!` ships `docs/migrations/<prev>-to-<next>.md` and a row
  in `docs/migrations/index.md`; `mkdocs.yml` globs the directory, so the nav
  needs nothing. Lead the guide with the breaks no type checker reports; the
  changelog is organized by feature, which is the wrong index for an upgrade.
