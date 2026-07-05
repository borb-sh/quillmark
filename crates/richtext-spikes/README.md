# quillmark-richtext-spikes

Throwaway Phase-0 probes for the richtext content model
([#831](https://github.com/quillmark-org/quillmark/issues/831)). **Not a
product** — a member of the workspace held *outside* `default-members`, so
`cargo test --workspace` runs it but a bare `cargo test` skips it. Delete this
crate when the phases it de-risks land.

Each spike leaves an executable finding: every claim in a finding doc maps to an
assertion in the matching test file.

All three findings live in one place —
[`prose/plans/richtext/phase-0.md`](../../prose/plans/richtext/phase-0.md).

| Spike | Question | Tests |
|-------|----------|-------|
| A | mark semantics + annotation rebase | `tests/spike_a_editor.rs` |
| B | source-map inversion + navigation | `tests/spike_b_sourcemap.rs` |
| C | seam encoding + determinism | `tests/spike_c_seam.rs` |

```
cargo test -p quillmark-richtext-spikes
```

`src/` is the shared throwaway `RichText` prototype: `model` (corpus + lines +
marks + islands), `canonical` (byte-deterministic JSON), `codec` (markdown ⇄
corpus + pdfform `.text`), `sourcemap` (per-run escape inversion), `diff`
(cold-parse diff + rebase), `usv` (USV ↔ UTF-16/UTF-8).
