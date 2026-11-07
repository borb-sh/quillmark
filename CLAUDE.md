# Quillmark

Schema-driven document framework: Markdown + YAML card metadata → rendered PDF/SVG/PNG via a Typst or acroform backend.

- Read [`prose/INVARIANTS.md`](prose/INVARIANTS.md) before changing behavior: it holds the rules the code cannot carry where they are broken, and why. Each crate's `lib.rs` rustdoc describes the crate.
- Commit subjects, the `!` break marker, and what earns a test a place: [`CONTRIBUTING.md`](CONTRIBUTING.md).
- Comments: none by default; keep one only where the code cannot carry the fact. Present tense, no history or changelog narration.
- A change that breaks an invariant updates `prose/INVARIANTS.md` in the same commit, with the new reason. A rule earns a place there only if a capable reader of the code would plausibly break it; [`prose/README.md`](prose/README.md) is normative, and `node scripts/check-prose.mjs` enforces the mechanical half in CI.
- The `Cargo.toml` version is the last *released* one; CI bumps it on release.
- `CHANGELOG.md` is `merge=union` in `.gitattributes`, so merge `main` with local git, not GitHub's "Update branch".

## Tests

`cargo test --workspace` is the working loop. PR CI builds both bindings on every push, and each takes minutes, so build one locally only to reproduce a red CI job or to test binding code no Rust test reaches.

- WASM: `./scripts/build-wasm.sh --ci && cd crates/bindings/wasm && npm test`. `--ci` is the fast-compile profile.
- Python: `cd crates/bindings/python && uv venv && source .venv/bin/activate && uv pip install maturin pytest && maturin develop && pytest`. Avoid `uv run` there: it syncs the project, which compiles the extension in release.
