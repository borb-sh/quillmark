# Quillmark Fuzzing Tests

Property-based fuzz tests over Quillmark's escaping functions, parsers, and JSON decode lanes, built on `proptest` rather than `cargo-fuzz`. Unpublished; internal testing only.

```bash
cargo test --package quillmark-fuzz            # everything
cargo test --package quillmark-fuzz pdf_fuzz   # one module
```

The crate is excluded from `default-members`, so a bare `cargo test` skips it; `cargo test --workspace` includes it.

One module per surface, each stating its own target and properties; `src/lib.rs`
lists them. A new escaping, parsing, or decoding surface gets a fuzz target
here.
