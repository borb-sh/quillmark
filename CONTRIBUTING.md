# Contributing

## Questions, bugs, and ideas

- A question, or an idea not yet a proposal: a blank issue.
- A bug: an issue from the bug form, with the smallest document that reproduces it.
- A change larger than a fix: open an issue before the pull request. Before 1.0 most releases change the API, so agree on the shape before building it.

## Setup

```bash
git clone https://github.com/borb-sh/quillmark
cd quillmark
cargo test --workspace
```

`cargo test --workspace` is the working loop. The Python and WASM bindings take minutes to build and PR CI builds both; their local loops are in [`CLAUDE.md` § Tests](CLAUDE.md#tests). CI also gates on `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --workspace` and `node scripts/check-canon.mjs`.

## Pull requests

- Every commit lands on `main` and seeds the changelog, so each subject reads `type(scope): summary`, or `type(scope)!: summary` for a break. `type` is one of `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `ci`, `chore`; the scope is optional.
- Comments and docs follow the rules in [`CLAUDE.md`](CLAUDE.md), which also covers canon upkeep. Design docs start at [`prose/canon/INDEX.md`](prose/canon/INDEX.md).

## AI-assisted work

Most of Quillmark's code is written by AI coding agents working for the maintainer. Each change lands through a pull request, CI, and the maintainer's review. [`CLAUDE.md`](CLAUDE.md) is the instructions those agents follow, and it binds human contributors the same way.

AI-assisted pull requests are welcome on the same terms as any other: you ran the tests, you can explain each change, and you answer review yourself.

## What earns a test a place

Exercising logic that can break: parsing, assembly, emission, diffing,
resolution, validation, error classification, round-trips, rendering. Restating
a rule the implementation or canon already carries earns nothing, and deleting a
stale test is maintenance.

- Diagnostic codes, wire-format keys, and public API names are contract;
  human-readable error prose, derive output, and constant literals are not.
- One strong test beats three angles on the same behavior.
- A boundary that takes bytes or JSON it did not write — a parser, a decoder,
  an escaper — carries a `properties` module beside the unit tests of the seam
  it states: refusal rather than a panic over generated input, and the
  round-trip or fixed point its codec claims. A generator that cannot reach the
  code it names states nothing, so shape the envelope and generate what sits
  inside it.
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
- A Typst minor bump is a `!`, since a plate is Typst source and a Typst minor
  release can change what a plate compiles to. The release's guide names the
  new Typst version, and so does `docs/quills/typst-backend.md` § "Typst
  Version". A Typst patch bump is not a `!`.
