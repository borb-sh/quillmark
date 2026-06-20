# Spike: .NET binding symmetrical to Python

**Goal.** De-risk a .NET binding for Quillmark whose public surface mirrors the
[Python binding](../python) method-for-method, and recommend an approach.

**Outcome.** Feasible and low-risk. A complete native ABI plus managed surface
is in this directory; the native crate compiles in the workspace and exports all
68 `qm_*` symbols. The managed layer and test suite are written to mirror Python
but require a .NET SDK (absent from the Rust CI image) to compile and run.

## The interop options considered

| Option | Verdict |
|--------|---------|
| **Hand-written C ABI (`cdylib`) + P/Invoke** | **Chosen.** Zero extra build tooling, mirrors how PyO3 ships a native module + thin language layer, and the `quillmark` crate already produces clean `serde` JSON for every structured type. Fully under our control. |
| `csbindgen` / `Interoptopus` (generate C# from Rust) | Viable later as a *codegen* over the same ABI to cut the hand-written `NativeMethods.cs`, but adds a build-time dependency and indirection not worth it for a spike. |
| UniFFI (multi-language from one Rust IDL) | Most "symmetrical to Python" in spirit (it targets Python too), but it would mean re-expressing the surface in UDL and adopting a third-party .NET backend — a larger commitment than this spike warrants, and it fights the existing per-binding hand-tuned surfaces. |

**Key simplification.** C has no rich object marshaling, so instead of mirroring
PyO3's per-method object conversions, the ABI uses **JSON as the structured-data
currency** and **opaque handles** for stateful objects. Cards, schema, metadata,
diagnostics, and field values all cross as UTF-8 JSON serialized by the *same*
core `serde` types (`CardWire`, `Diagnostic`, `RenderOptions`, …) the Python and
WASM bindings use. The typed C# surface (`Document.SetField`, `Quill.Metadata`,
…) is reassembled on top. This keeps the ABI small (~68 functions for the whole
surface) and guarantees the data shapes can never drift from the other bindings.

## What was built

- **`src/abi.rs`** — marshaling primitives: owned C strings, an owned byte
  buffer (`QmBytes`) for artifact bytes, opaque handle boxing/freeing, and a
  thread-local last-error.
- **`src/lib.rs`** — the full `qm_*` surface over `quillmark`: engine
  (`new`/`render`/`supported_formats`/`registered_backends`), `Quill`
  (`from_path` + all readers + `validate` + seeding), `Document` (constructors,
  statics, readers, every main-card and composable-card mutator, the `$ext`
  family), and `RenderResult`/`Artifact` accessors. Logic mirrors
  `python/src/types.rs` and `enums.rs`/`errors.rs` line-for-line; only the
  marshaling layer differs (serde-JSON strings instead of PyO3 objects).
- **`csharp/Quillmark/`** — the managed surface: `Quillmark`, `Quill`,
  `Document`, `RenderResult`, `Artifact`, `Diagnostic`, `Location`, `Card`, the
  `OutputFormat`/`Severity` enums, and `QuillmarkException`. Handles use the
  standard dispose pattern (`NativeObject`) so each Rust `Box` is freed exactly
  once.
- **`csharp/QuillDemo/`** — a console demo that is a direct port of
  `python/examples/quill_demo.py`.
- **`csharp/Quillmark.Tests/`** — an xUnit suite mirroring the Python tests
  (engine, quill, render-to-PDF, JSON/Markdown round-trips, mutators, validate,
  `try_from_json`, error contract, static text).
- **`scripts/build-dotnet.sh`** — builds the cdylib then the managed
  assembly/tests, mirroring `scripts/build-wasm.sh`.

## Error contract (symmetry preserved)

Python raises one exception type (`QuillmarkError`) always carrying a non-empty
`.diagnostics` list. The ABI reproduces this without a richer-than-C protocol: a
fallible call returns a null pointer (handle/string) or `-1` (status) and parks
a JSON `{ message, diagnostics }` payload in a thread-local that C# drains into a
`QuillmarkException` with a non-empty `.Diagnostics`. Optional *values* are
encoded as JSON `null` inside a valid string, so a null pointer unambiguously
means a real failure (the one benign exception, `try_from_json`, clears the
error slot and returns a null handle by contract).

## Verified vs. remaining

**Verified here**
- The native crate builds in the workspace (`cargo build -p quillmark-dotnet`)
  and exports all 68 `qm_*` symbols (`nm -D`).
- The whole Python surface has a corresponding ABI entry point and managed
  method (see the mapping table in the README).

**Not verified here (needs a .NET SDK on CI)**
- Compiling/running the managed assembly, demo, and tests. The Rust CI image has
  no `dotnet`; `build-dotnet.sh` degrades gracefully (builds the cdylib, skips
  the managed step with a notice).

## Risks / open questions

1. **Packaging.** A real release needs a NuGet package carrying the native
   library for each RID (`linux-x64`, `osx-arm64`, `win-x64`, …) under
   `runtimes/<rid>/native/`. The current `.csproj` copies the local cargo build
   for dev/test only. This is the main remaining work and is well-trodden for
   Rust+.NET.
2. **`UIntPtr` index width.** Indices/counts cross as `usize`/`UIntPtr`. The
   managed surface exposes `int`, which is correct for realistic card counts;
   the `checked` cast on artifact length guards the one place a buffer could
   nominally exceed `int`.
3. **Thread-local error.** Matches PyO3's implicit per-call model and is correct
   as long as each fallible call's result/error is consumed before the next call
   on the same thread — which the C# wrappers do inline.
4. **No `RenderSession`/canvas.** Same scope as Python (one-shot `render` only);
   the iterative preview surface stays WASM-only.

## Recommendation

Promote the spike. The architecture is sound and symmetrical with Python at low
risk. Next steps, in order: (1) add a `dotnet`-capable CI job to compile and run
`Quillmark.Tests`; (2) build the multi-RID NuGet packaging; (3) optionally
replace the hand-written `NativeMethods.cs` with `csbindgen` codegen over the
same ABI once the surface stabilizes.
