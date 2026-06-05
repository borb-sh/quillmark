# Quillmark

Format-first Markdown rendering: Markdown + YAML card metadata → PDF/SVG/PNG via a Typst backend. Crates: `core` (parsing/schema/traits), `quillmark` (orchestration), `backends/typst`, `bindings/{python,wasm,cli}`, `fixtures` (test Quills).

Design docs: [`prose/canon/INDEX.md`](prose/canon/INDEX.md)

## Migration docs

The migration guides in [`docs/migrations/`](docs/migrations/) for already-released
version steps are era-accurate and immutable: each captures the state of the world
at its release, so don't edit them to match later changes. The one exception is the
working migration doc for the in-progress (unreleased) version, which is still mutable
until that version ships.

## Tests

```bash
cargo test --workspace
```

WASM: `./scripts/build-wasm.sh` → `cd crates/bindings/wasm && npm test`  
Python: `cd crates/bindings/python && uv run maturin develop && uv run pytest`
