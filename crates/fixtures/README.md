# quillmark-fixtures

Sample Quill templates and markdown files backing [Quillmark](https://github.com/borb-sh/quillmark)'s tests and examples, plus the helpers that resolve their paths.

## Usage

```rust
let sample_md = quillmark_fixtures::resource_path("sample.md");

// Resolves to the latest version directory
let usaf_memo = quillmark_fixtures::quills_path("usaf_memo");
```

## Resources

- **Quill templates** under `resources/quills/<name>/<version>/`, each with a `Quill.yaml` naming its backend and either a Typst `plate.typ` or a PDF-form template. `quills_path` resolves the latest version, and `quill_names` walks the directory for the sweeps in `quiver_test.rs`, so a quill added here is rendered without being named anywhere.

- **Sample markdown** under `resources/`
  - `sample.md` - markdown constructs only, no card-yaml block
  - `card_yaml_demo.md` - a card-yaml document
  - `extended_metadata_demo.md` - composable cards under one main card

- **The parity corpus**, `resources/parity/parity.json`: one entry per row of the matrix in [`resources/parity/README.md`](resources/parity/README.md), holding the row's markdown, its canonical stored content, substrings its Typst lowering contains and the signals it raises.
  `crates/quillmark/tests/parity.rs` asserts every entry and holds the matrix to the corpus, and each binding holds its crossing to it: the WASM binding's `parity.test.js` and the Python binding's `tests/test_parity.py`.

## License

Apache-2.0 ([LICENSE](../../LICENSE)), except the third-party files below, each under its own license:

| Path under `resources/quills/` | License |
|---|---|
| `usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/` | MIT ([LICENSE](resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/LICENSE)) |
| `…/tonguetoquill-usaf-memo/fonts/Cinzel/`, `CopperplateCC/`, `LiberationMono/` | SIL Open Font License 1.1, text beside each font |
| `…/tonguetoquill-usaf-memo/fonts/TeXGyreTermes/` | GUST Font License, text beside the fonts |
| `taro/0.1.0/assets/fonts/` (Figtree) | SIL Open Font License 1.1 ([OFL.txt](resources/quills/taro/0.1.0/assets/fonts/OFL.txt)) |
| `usaf_memo/0.2.0/assets/dod_seal.png`, `dow_seal.png` | United States government insignia: not licensed, and their use is restricted by law |

The `usaf_memo` quill reproduces the format of AFH 33-337 correspondence. Quillmark is not affiliated with or endorsed by the Department of Defense, the Department of the Air Force, or any Military Service.
