# Quillmark Rust Workspace — CI/CD

> **Implementation**: `.github/workflows/`

Published crates: `quillmark-core`, `quillmark-typst`, `quillmark`, `quillmark-cli`.

Not published: `quillmark-fixtures`, `quillmark-fuzz`, `quillmark-python`, `quillmark-wasm`.

---

## 1) Continuous Integration (CI)

**Trigger**: pull requests and pushes to any branch except version tags.
**Jobs** (all Linux, run in parallel):

| Job | What it does |
|-----|-------------|
| `lint` | `cargo doc --no-deps --locked` with `-Dwarnings` (Clippy commented out, not yet enforced) |
| `test` | `cargo test --workspace --all-features --locked` |
| `wasm` | builds WASM via `./scripts/build-wasm.sh --ci`, then runs `npx vitest run` |

Excluded: multi-OS matrix, MSRV, security scanners, coverage, benchmarks.

---

## 2) Continuous Delivery (CD)

### Release Preparation (`release-prepare.yml`)

**Trigger**: `workflow_dispatch` from GitHub UI with a `bump` input (`patch` or `minor`) and an optional `release_candidate` boolean flag.

1. Installs `cargo-release` and runs `cargo release version <next>` to bump all workspace `Cargo.toml` versions and intra-workspace dependencies.
2. Seeds a changelog entry and opens a PR from a `release/vX.Y.Z` branch into `main`.

Merging the release PR triggers the Release workflow via a GitHub App token (PRs opened with the default `GITHUB_TOKEN` do not fire workflow events).

### Release & Publish (`release.yml`)

**Trigger**: pull request merged into `main` from a branch starting with `release/v`.

**Phase 1 — Release** (runs first):
1. Reads the workspace version and creates a `vX.Y.Z` tag on `main`.
2. Creates a GitHub Release for the tag (marked as a pre-release for `-rc` versions).

**Phase 2 — Publish** (all run in parallel, after release):

| Target | Registry | Auth |
|--------|----------|------|
| Rust crates | crates.io | OIDC Trusted Publishing via `rust-lang/crates-io-auth-action` (`id-token: write`) |
| WASM bindings | npm | OIDC Trusted Publisher (`id-token: write`) |
| Python bindings | PyPI | OIDC Trusted Publishing via `pypa/gh-action-pypi-publish` (`id-token: write`) |

- **Crates**: `cargo publish --locked --no-verify`
- **WASM**: builds via `./scripts/build-wasm.sh`, runs `npx vitest run` (tests), publishes `@quillmark/wasm` with `--provenance`
- **Python**: builds wheels via `maturin-action` for Linux (x86_64, aarch64), Windows (x64), macOS (aarch64) — Python 3.10–3.12 — plus sdist, then uploads to PyPI

---

## 3) Versioning

- SemVer across all workspace crates and bindings.
- Version bumps are initiated via GitHub UI (`workflow_dispatch`) and executed by `cargo-release` in CI, which opens a release PR; merging the PR tags `main` and publishes.
- WASM npm package version is derived from the workspace version at build time (`scripts/build-wasm.sh`).
- Python package version is derived from the workspace Cargo.toml via maturin's `dynamic = ["version"]`.
