# Operations

Quillmark bounds what it ingests and not what it renders. That asymmetry is the thing to plan around; the rest of this page is what a long-lived process holds and what crosses threads.

## Render is not bounded

**There is no render budget.** No deadline, no cancellation, no fuel, no progress callback. `Quillmark::render` returns when the backend is done.

The parse limits ([Markdown Specification §8](../reference/markdown-spec.md#8-limits)) bound the input, and nothing translates them into a bound on the work, so a document inside every ceiling can hold a core indefinitely. Cost is a property of the quill at least as much as of the document: a plate is a Typst program, and layout has superlinear shapes. Typst refuses an infinite loop and unbounded recursion, which bounds iteration rather than work — a loop that terminates after enough expensive iterations is refused by neither.

So the bound comes from the host:

- **Server.** Render on a thread or subprocess you can abandon, under your own timeout. `Quillmark` is `Sync`, so one engine backs the whole pool.
- **Browser.** `Engine.render` and `Engine.open` return promises, but the compile inside them is synchronous: awaiting does not yield while it runs, and `LiveSession.render` does not even return a promise. Either way the thread is blocked for the render's full duration, so a render on the main thread freezes the tab. Run the module in a Web Worker and `worker.terminate()` to abort — there is no in-band cancel, and terminate discards the module, so plan on re-instantiating it.

No surface times a render for you: clock the `render` call. That is how a host finds which quills are the expensive ones — `performance.now()` in JavaScript, `time.perf_counter()` in Python, `Instant::now()` in Rust.

## Concurrency

`Quillmark`, `Quill`, and `Document` are `Send + Sync` (`crates/quillmark/tests/facade_surface.rs` pins this). One engine serves many threads, and a loaded `Quill` is portable declarative data, shared rather than cloned per request. `LiveSession` is a per-document handle: open one per editing session.

## What a long-lived process accumulates

**The Typst memo cache is process-global and self-evicting.** The backend calls `comemo::evict` with a max age of 10 after each compile, which keeps an editing loop (one compile per keystroke) from growing without bound. The cache is shared across every session in the process, so under concurrent renders one session's compiles age out another's and reuse degrades as concurrency rises — costing time, never correctness, since comemo entries are pure functions of their input. Steady-state memory tracks concurrency and document size, not uptime.

The engine caches no quills: `Quillmark::render` takes a `&Quill` the caller owns.

## Isolation

**No network**, so a deployment needs no egress: Quillmark never downloads a Typst package, and `QuillWorld` loads them only from `{quill}/packages/` in the quill's in-memory tree, so a plate's `#import` resolves against what the quill ships or fails. **No ambient filesystem**: a plate reads through that same tree.

## Failure modes

Nothing in the workspace catches unwind, so a panic is terminal on every surface: it aborts the CLI and the Python extension, and leaves the WASM module poisoned rather than erroring. Panics are bugs — report one.

Ordinary failures are not panics. Every documented error path returns a `RenderError` or a parse diagnostic carrying a stable `code`; route on the code, not the message ([Error Handling](error-handling.md)).
