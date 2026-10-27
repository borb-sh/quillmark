# Operations

Quillmark bounds what it ingests and not what it renders. That asymmetry is the thing to plan around; the rest of this page is what a long-lived process holds and what crosses threads.

## Render is not bounded

**There is no render budget.** No deadline, no cancellation, no fuel, no progress callback. `Engine::render` returns when the backend is done.

The parse limits ([Markdown Specification §8](../reference/markdown-spec.md#8-limits)) bound the input, and the card ceiling bounds a stored document too: a row holding more cards fails to load, and an edit placing one more is refused. Nothing translates them into a bound on the work, so a document inside every ceiling can hold a core indefinitely. Cost is a property of the quill at least as much as of the document: a plate is a Typst program, and layout has superlinear shapes. Typst refuses an infinite loop and unbounded recursion, which bounds iteration rather than work — a loop that terminates after enough expensive iterations is refused by neither.

So the bound comes from the host:

- **Server.** For untrusted documents, render in a subprocess or worker pool under a wall-clock timeout and a memory cap (`RLIMIT_AS`, a cgroup, the container limit), and kill the worker that passes either. A thread is no bound: one abandoned past its timeout keeps its core and its memory until the compile finishes. Threads suit trusted input; `Engine` is `Sync`, so one engine backs the whole pool.
- **Input size.** The parse ceilings are a safety limit, not a sizing recommendation. On one x86_64 core, a 10 MiB document takes on the order of 15–25 s and 2.5–3 GB natively through a plain quill, and about a minute and 3 GB under WASM; a quill whose plate measures or queries per paragraph grows faster than its input. Cap untrusted documents at the size your slowest quill renders within your timeout, then clock it.
- **Browser.** `Engine.render` and `Engine.open` return promises, but the compile inside them is synchronous: awaiting does not yield while it runs, and `LiveSession.render` does not even return a promise. Either way the thread is blocked for the render's full duration, so a render on the main thread freezes the tab. Run the module in a Web Worker and `worker.terminate()` to abort — there is no in-band cancel, and terminate discards the module, so plan on re-instantiating it. Call `engine.load(quill)` early: the render build is about 10 MB gzipped, and the core build behind validation and editing is under 1 MB.

No surface times a render for you: clock the `render` call. That is how a host finds which quills are the expensive ones — `performance.now()` in JavaScript, `time.perf_counter()` in Python, `Instant::now()` in Rust.

## Concurrency

`Engine`, `Quill`, and `Document` are `Send + Sync` (`crates/quillmark/tests/facade_surface.rs` pins this). One engine serves many threads, and a loaded `Quill` is portable declarative data, shared rather than cloned per request. `LiveSession` is a per-document handle: open one per editing session.

Python's `Engine.render` releases the GIL while it compiles, so renders on separate threads run in parallel and an asyncio loop calling it through `asyncio.to_thread` keeps serving.

## What a long-lived process accumulates

**The Typst memo cache is process-global and self-evicting.** The backend calls `comemo::evict` with a max age of 10 after each compile, which keeps an editing loop (one compile per keystroke) from growing without bound. The cache is shared across every session in the process, so under concurrent renders one session's compiles age out another's and reuse degrades as concurrency rises — costing time, never correctness, since comemo entries are pure functions of their input. Steady-state memory tracks concurrency and document size, not uptime.

**WASM memory only grows.** Linear memory never shrinks, so a module stays at the high-water mark of the largest render it has run. A long-lived tab or Node worker that renders one large document holds that memory until the realm is discarded.

`Engine::render` caches no quills: it takes a `&Quill` the caller owns. The WASM `Engine` caches a backend-memory clone of each `Quill` it renders in a `WeakMap` keyed on that `Quill`, so a clone's lifetime follows its `Quill`.

## Isolation

**No network**, so a deployment needs no egress: Quillmark never downloads a Typst package, and `QuillWorld` loads them only from `{quill}/packages/` in the quill's in-memory tree, so a plate's `#import` resolves against what the quill ships or fails. **No ambient filesystem**: a plate reads through that same tree.

## Untrusted input

A document is data. Its text reaches a plate escaped, never as Typst code, and it names no file outside the quill. Quillmark does not bound what it costs to render; the host does, as above.

A quill is code. A plate is a Typst program, and an acroform quill's `form.pdf` is parsed in full; `quill_from_path` skips symlinks in a directory, refuses any file over 50 MiB, and caps neither file count nor total size. A `.quill` bundle carries its own budget, 2048 entries, 32 MiB a file and 64 MiB in all, spent off the sizes it declares before anything inflates. Load a quill from someone you do not trust inside the same bounded worker as its renders.

SVG output keeps links as live `href`s. A Markdown link in a body or rich-text field whose scheme is `javascript:`, `vbscript:`, `data:` or `file:` renders as its text alone. A plate that links a field value itself (`#link(..)`) passes it through as written, so that plate vets the value. An SVG placed inline in a page is the page's markup: show SVG from untrusted documents through `<img>`, or sanitize it first.

## Failure modes

Nothing in the workspace catches unwind, so a panic is terminal on every surface: it aborts the CLI and the Python extension, and leaves the WASM module poisoned. Panics are bugs — report one.

Under WASM, running out of linear memory (at most 4 GiB, less in some browsers) traps the same way. A trap rejects as `runtime::backend_trapped`, carrying the `WebAssembly.RuntimeError` as `cause`; so does exhausting the native stack, which V8 and JavaScriptCore report as a `RangeError` and Firefox as an `InternalError`, carried the same way. It leaves the instance's state undefined, and the instance is module-scoped, so a new `Engine` inherits it: discard the realm before rendering again.

Ordinary failures are not panics. Every documented error path returns a `RenderError` or a parse diagnostic carrying a stable `code`; route on the code, not the message ([Error Handling](error-handling.md)).
