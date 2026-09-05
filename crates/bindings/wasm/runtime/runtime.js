/* @ts-self-types="./runtime.d.ts" */
//
// @quillmark/wasm/runtime: the canonical consumer API.
//
// The package ships multiple WASM binaries with SEPARATE linear memories: a
// Typst-less `core` build (small, eager) that is the canonical home of
// `Quill`/`Document`, and one private binary per backend that carries an engine.
// A handle from one memory cannot be used by another; this module hides that
// seam behind `await init()` and a static `Engine`.
//
//   - `Quill` and `Document` ARE the core build's classes, handed out by the
//     gate below, never subclasses or wrappers: that identity is what makes
//     `instanceof` the whole membership test, so a handle either belongs to this
//     copy or to another copy, and the second is always a consumer bug.
//     `runtime.test.js` guards it (`Quill === CoreQuill`). No backend is loaded
//     to use them, so the editor path never pays for a multi-MB binary.
//
//   - `Engine` is the render dispatcher. It routes on `quill.backendId`, lazily
//     imports that backend's build, clones the canonical `Quill`/`Document` into
//     the backend's memory as data (`toTree`→`fromTree`, `toStored`→`fromStored`),
//     renders, and never lets the backend handles escape.
//
//     CLONE LIFETIMES: the per-call `Document` clone is transient, since
//     documents are small and mutate freely. The `Quill` clone is CACHED,
//     because re-cloning re-serializes its whole file tree, copies it into
//     backend memory, and re-parses + re-validates the bundle every call. Each
//     `Engine` memoizes it in a `WeakMap` keyed on the canonical `Quill`, so
//     dropping the core quill makes the entry collectable and `--weak-refs`
//     frees the backend handle. The contract this buys: a `Quill` instance's
//     contents never change after construction — mutate by replacing the
//     instance.

// Local bindings, so this module can augment them: `quill.writer(doc)` is
// patched onto the prototype below, and `instanceof` reads them directly. The
// default import is the core build's generated instantiation entry; `init` is
// the only thing that calls it.
import initCore, { Quill, Document } from '../core/wasm.js';
// The wasm byte source, resolved per environment by package.json's `imports`
// map: a pass-through in a browser (the glue fetches and streams the URL
// itself), a `node:fs` read under Node, whose `fetch` rejects `file:` URLs.
// Resolution-time, so `node:fs` never enters a browser graph.
import { toModuleSource } from '#quillmark-env';
import { importMarkdown, exportMarkdown, rebase, mapPos, mapMarks } from '../core/wasm.js';
import { parseDocPath, formatDocPath } from '../core/wasm.js';

// ── Initialization ──────────────────────────────────────────────────────────
// The builds are `--target web`: they export their classes synchronously but
// carry no wasm instance until something instantiates them. This module owns
// that for core, behind one awaited gate; `Engine` owns it for the backends,
// inside their lazy load.
//
// THE GATE IS THE ONLY DOOR: `init` resolves to the core surface and nothing
// here exports it statically, so a handle is unobtainable without having
// awaited, and package.json's `exports` map carries exactly one entry. The
// guarded surface cannot be async (`Quill.fromTree` and `quill.seedDocument`
// return synchronously), so there is nowhere to hide an await except in front.
//
// WHAT STAYS A STATIC EXPORT is what needs no instance: `MAIN_CARD_ADDR`, the
// open-set guards, and `isQuillmarkError` are pure JS over plain objects.
// `Engine`, `LiveSession` and the four writer/reader classes stay static too,
// gated by their ARGUMENTS instead — every verb takes a `Quill` or both
// handles, so a caller who has not awaited cannot produce an argument, and the
// two constructors taking no handle reach no wasm. None carries a static
// method, the one member shape an argument cannot gate. `gate.test.js` drives
// the whole static surface before `init`. Holding them out of the gate also
// keeps them tree-shakable.
//
// FAILURE DELIVERY follows the FUNCTION kind, not the failure kind: a sync verb
// throws, a promise-returning verb rejects, and nothing does both. `init` is the
// one promise-returning export not declared `async`, because the memo is
// returned by identity; its conflict guard rejects explicitly to hold the rule.

/**
 * The gated surface: the core build's values, which are exactly the ones its
 * instance stands behind. Frozen and built at module scope, because the classes
 * and functions themselves resolve synchronously (only the instance behind them
 * is late), so there is nothing to defer and no per-call allocation.
 *
 * Membership is derived, not chosen: it is the core build's exports minus its
 * instantiation machinery (`default`, `initSync`, and the start-section
 * `start`), which `init` owns and no consumer calls. `init.test.js` § "the
 * gated surface" computes that set and pins it, so a new core export that never
 * reaches here fails there rather than going missing.
 * @type {import('./runtime.js').CoreSurface}
 */
const CORE_SURFACE = Object.freeze({
	Quill,
	Document,
	importMarkdown,
	exportMarkdown,
	rebase,
	mapPos,
	mapMarks,
	parseDocPath,
	formatDocPath
});

/** The in-flight or settled core instantiation, resolving to `CORE_SURFACE`.
 * The memo is the PROMISE, not a boolean, so concurrent callers share one
 * instantiation instead of racing.
 * @type {Promise<import('./runtime.js').CoreSurface> | undefined} */
let coreInit;
/** The source `init` was first called with; the conflict check reads it. */
let coreInitSource;

/**
 * Instantiate the core WASM build and resolve to its surface.
 *
 * ```js
 * import { init } from '@quillmark/wasm';
 * const { Quill, Document } = await init();
 * ```
 *
 * The classes and the free functions come from here and nowhere else, so the
 * pre-init mistake is not expressible. Destructure at each entry point (route
 * loader, hydration path, worker) rather than threading one result around: the
 * gate is memoized, so every await after the first is free.
 *
 * Identical in every environment: in a browser the binary is fetched and
 * streamed, under Node it is read off disk, and the call site is the same line.
 *
 * Idempotent and concurrency-safe: every non-conflicting call returns the same
 * promise, so several entry points cost one instantiation. A failed init clears
 * the memo, so a retry is possible.
 *
 * Both failures reject (§ "Initialization", FAILURE DELIVERY): one `catch`
 * around `await init(...)` covers `runtime::init_conflict` and
 * `runtime::init_failed` alike.
 *
 * @param {import('../core/wasm.js').InitInput} [source] override the binary's
 *   source (bytes, a `Response`, a `WebAssembly.Module`, a URL) for hosts that
 *   route assets themselves or embed the binary. Pass it on the FIRST call; a
 *   later call passing a *different* source rejects with
 *   `runtime::init_conflict` rather than silently ignoring it. Passing the same
 *   value again is fine, so several entry points may each `await init(BYTES)`
 *   against one constant.
 * @returns {Promise<import('./runtime.js').CoreSurface>} the core surface, once
 *   its instance is live
 */
export function init(source) {
	if (coreInit) {
		if (source !== undefined && source !== coreInitSource) {
			// A rejection, not a throw: this is the one failure on the
			// promise-returning surface that could land on the caller's stack, where
			// `init(BYTES).catch(…)` would not see it.
			return Promise.reject(
				quillmarkError(
					'runtime::init_conflict',
					'init(source): core is already initializing or initialized from a different source.',
					'Pass a source on the first call only, or pass the same value every time.'
				)
			);
		}
		return coreInit;
	}
	coreInitSource = source;
	// Assign before the first await so a synchronous second call sees the memo.
	coreInit = instantiateCore(source).then(
		() => CORE_SURFACE,
		(err) => {
			// Self-heal, as `#resolveBackend` does: one transient failure (a 404, an
			// offline fetch) must not poison every later attempt.
			coreInit = undefined;
			coreInitSource = undefined;
			throw err;
		}
	);
	return coreInit;
}

/**
 * @param {import('../core/wasm.js').InitInput | undefined} source
 * @returns {Promise<void>}
 */
async function instantiateCore(source) {
	// The literal `new URL(..., import.meta.url)` form: every bundler rewrites it
	// into an emitted asset, and unbundled browsers and Node resolve it against
	// the shipped package layout.
	const resolved = await toModuleSource(
		source ?? new URL('../core/wasm_bg.wasm', import.meta.url)
	);
	try {
		await initCore({ module_or_path: resolved });
	} catch (cause) {
		throw Object.assign(
			quillmarkError(
				'runtime::init_failed',
				`init(): could not load or instantiate the core WASM binary: ${
					/** @type {any} */ (cause)?.message ?? cause
				}`,
				"The binary ships beside the package files; check the network tab for a 404 or an HTML response. Under Vite's dev server, dependency pre-bundling moves the package away from it: add optimizeDeps: { exclude: ['@quillmark/wasm'] }."
			),
			{ cause }
		);
	}
}

// ── The main-card address ───────────────────────────────────────────────────
/**
 * The main card's address: the default target of the card-scoped verbs
 * (`storeFields` / `storeExt` / `commitFields` / …). A named, `CardAddr`-typed
 * alias for the empty address `{}`, so a main-card write names its target:
 * `doc.storeFields(MAIN_CARD_ADDR, fields)`. It IS `{}` (frozen), a pure alias:
 * `{}` and `undefined` stay equally valid. Card axis only: a card selector,
 * never a field address.
 * @type {import('../core/wasm.js').CardAddr}
 */
export const MAIN_CARD_ADDR = Object.freeze({});

// ── The variant discriminant key ────────────────────────────────────────────
/**
 * The key carrying the discriminant inside a variant-bearing enum's value.
 *
 * A field declaring `variants:` rests as a container, `{value: <member>, …that
 * member's fields}`, so reading or writing one means naming this key:
 * `doc.storeFields(MAIN_CARD_ADDR, { classification: { [VARIANT_DISCRIMINANT_KEY]: 'CUI' } })`.
 * It crosses the boundary inside untyped container data, with no type to read
 * it off.
 *
 * Reserved: no variant may declare a field under it
 * (`quill::variant_reserved_field_name`), and `QuillFieldSchema.variants`,
 * keyed by member, never contains it.
 * @type {'value'}
 */
export const VARIANT_DISCRIMINANT_KEY = 'value';

/**
 * Narrow an unknown caught value to a `QuillmarkError`, the error every
 * fallible method in this package throws: a real `Error` with a non-empty
 * `diagnostics` array attached (same entry shape as `RenderResult.warnings`).
 *
 * Structural by necessity AND by design: the WASM layer constructs a plain
 * `Error` and attaches the property (there is no error class to `instanceof`),
 * and a structural check narrows errors from any build or WASM instance in the
 * page. The deliberate exception to § "Handles from another copy": an error is
 * data, not a handle, so nothing is gained by refusing one that crossed.
 *
 * @param {unknown} e
 * @returns {e is Error & { diagnostics: import('../core/wasm.js').Diagnostic[] }}
 */
export function isQuillmarkError(e) {
	return e instanceof Error && Array.isArray(/** @type {any} */ (e).diagnostics);
}

/**
 * Build a `QuillmarkError` JS-side: a real `Error` carrying `diagnostics`, the
 * shape `isQuillmarkError` narrows and the shape Rust's `WasmError::to_js_value`
 * produces. Errors raised by this hand-written layer belong to the same contract
 * as the ones raised across the WASM boundary.
 * @param {string} code
 * @param {string} message
 * @param {string} hint
 * @returns {Error & { diagnostics: import('../core/wasm.js').Diagnostic[] }}
 */
function quillmarkError(code, message, hint) {
	const err = /** @type {any} */ (new Error(message));
	err.diagnostics = [{ severity: 'error', code, message, hint }];
	return err;
}

// ── Handles from another copy: always a bug ─────────────────────────────────
// A duplicate install is two `core` builds: two linear memories and two distinct
// `Quill`/`Document` classes. No topology legitimately loads a multi-megabyte
// WASM package twice AND needs handles to cross between the copies, so a
// crossing is a consumer bug, and every seam taking a core handle says so.
//
// Crossing read-only handles as data is mechanically possible (`toStored` and
// `toTree` serialize either way) and is not done: it leaves a package where some
// verbs work and some throw, and it hides a cliff, since a crossed read is a
// whole-document `toStored` + `fromStored` and a form reading fifty fields pays
// fifty round trips.
//
// What the checks deliver is the ERROR, not the rejection. wasm-bindgen's glue
// already rejects a foreign class wherever a method declares a reference
// parameter, but its `_assertClass` throws a bare `Error` reading
// `expected instance of Document` at a value that IS a `Document`, so
// `isQuillmarkError` returns false and the failure leaves this package's error
// contract. The checks front-run it with a `QuillmarkError` naming both cause
// and cure, and cover the seams with no `_assertClass` to front-run: `Engine`
// and `LiveSession.update` cross into backend memory as data, where a foreign
// handle would silently work at the price of that round trip and a per-copy
// split of the quill clone cache.

/** Per class: the code and hint for "not one at all", and the from-another-copy probe. */
const HANDLE_KINDS = {
	Quill: {
		code: 'runtime::not_a_quill',
		probe: 'toTree',
		hint: 'Pass a Quill built by Quill.fromTree.'
	},
	Document: {
		code: 'runtime::not_a_document',
		probe: 'toStored',
		hint: 'Pass a Document built by Document.fromMarkdown / fromStored or quill.seedDocument.'
	}
};

/**
 * The rejection for a value that is not one of this copy's handles. Two cures,
 * so two diagnostics: a value carrying the class's serializer is that class
 * from ANOTHER copy (dedupe the install), anything else is the wrong argument
 * (fix the call).
 * @param {unknown} value
 * @param {string} method
 * @param {'Quill' | 'Document'} className
 * @returns {Error & { diagnostics: import('../core/wasm.js').Diagnostic[] }}
 */
function notLocal(value, method, className) {
	const { code, probe, hint } = HANDLE_KINDS[className];
	if (value && typeof (/** @type {any} */ (value)[probe]) === 'function') {
		return quillmarkError(
			'runtime::foreign_handle',
			`${method}: the ${className} belongs to a different copy of @quillmark/wasm. Handles never cross between copies: each copy is its own WASM linear memory and its own ${className} class.`,
			'Two copies of @quillmark/wasm are installed. Run `npm ls @quillmark/wasm` and dedupe to one.'
		);
	}
	return quillmarkError(
		code,
		`${method}: expected a ${className}, got ${value === null ? 'null' : typeof value}.`,
		hint
	);
}

/**
 * Throw unless `doc` is THIS copy's `Document`.
 * @param {unknown} doc
 * @param {string} method
 * @returns {void}
 */
function requireLocalDoc(doc, method) {
	if (doc instanceof Document) return;
	throw notLocal(doc, method, 'Document');
}

/**
 * Throw unless `quill` is THIS copy's `Quill`.
 * @param {unknown} quill
 * @param {string} method
 * @returns {void}
 */
function requireLocalQuill(quill, method) {
	if (quill instanceof Quill) return;
	throw notLocal(quill, method, 'Quill');
}

// Marker for the patches below. `Symbol.for`, not a module-local `Symbol()`:
// re-evaluating THIS module (Vite HMR, a Vitest worker sharing a module graph)
// against an already-patched (because cached) core build must see the existing
// marker, or each pass wraps the previous wrapper.
const HANDLE_CHECKED = Symbol.for('@quillmark/wasm:handle-checked');

/**
 * Replace `proto[name]` with `wrap(original)`, once.
 * @param {object} proto
 * @param {string} name
 * @param {(original: Function) => Function} wrap
 */
function patchHandleChecked(proto, name, wrap) {
	const original = /** @type {any} */ (proto)[name];
	if (typeof original !== 'function' || original[HANDLE_CHECKED]) return;
	const patched = wrap(original);
	/** @type {any} */ (patched)[HANDLE_CHECKED] = true;
	// Keep the method's name so a stack trace still reads `Quill.validate`.
	Object.defineProperty(patched, 'name', { value: name, configurable: true });
	/** @type {any} */ (proto)[name] = patched;
}

// The core methods declaring a `&Document` parameter. Each already refuses a
// foreign handle inside `_assertClass`; the patch makes the refusal legible.
patchHandleChecked(Document.prototype, 'equals', (original) =>
	function equals(/** @type {any} */ other) {
		requireLocalDoc(other, 'Document.equals');
		return original.call(this, other);
	}
);
// `_resolve` is not among them: it is reached only through `quill.reader(doc)`,
// whose constructor checks both handles.
for (const name of /** @type {const} */ (['validate', 'conform'])) {
	// Named once per patch, not per call: `validate` runs per keystroke.
	const method = `Quill.${name}`;
	patchHandleChecked(Quill.prototype, name, (original) =>
		function (/** @type {any} */ doc) {
			requireLocalDoc(doc, method);
			return original.call(this, doc);
		}
	);
}

// The typed writer/reader primitives (`Document._commitField` and friends) take
// the QUILL by reference, so they hit the same `_assertClass` from the other
// direction. Checked at the four writer/reader classes below, not patched onto
// `Document`: a foreign document carries its OWN prototype, so patching this
// copy's would never run.

// ── Open-set discriminant guards ────────────────────────────────────────────
// `ContentIsland.type`, `ContentMark.type`, `ContentLine.kind`, and
// `ContentContainer.container` are OPEN sets: each union carries a residual
// `{ …: string; … }` arm, so a bare `x.type === 'table'` check never narrows the
// payload, TS keeps the residual arm live (a `string` can be `'table'`),
// leaving `props` / the mark payload / `level` opaque at every consumer. These
// are the checked narrowing path: on the true branch the payload's pinned shape
// is asserted. Only the payload-carrying arms get a guard: an island always
// carries `props`, and a `link`/`anchor` mark, a `heading`/`code` line and a
// `list_item` container each carry their payload in `attrs`; the payload-free
// arms (`strong`/`emph`/`underline`/`strike`/`code` marks, `para`/`island`/
// `rule` lines, `quote`) omit `attrs` and narrow to nothing. An unrecognized
// discriminant fails every guard and carries the same `attrs` a known one
// would.

/**
 * @param {import('../core/wasm.js').ContentIsland} island
 * @returns {island is import('../core/wasm.js').ContentIsland & { type: 'table'; props: import('../core/wasm.js').TableProps }}
 */
export function isTableIsland(island) {
	return island.type === 'table';
}

/**
 * @param {import('../core/wasm.js').ContentIsland} island
 * @returns {island is import('../core/wasm.js').ContentIsland & { type: 'image'; props: import('../core/wasm.js').ImageProps }}
 */
export function isImageIsland(island) {
	return island.type === 'image';
}

/**
 * @param {import('../core/wasm.js').ContentMark} mark
 * @returns {mark is import('../core/wasm.js').ContentMark & { type: 'link'; attrs: { url: string } }}
 */
export function isLinkMark(mark) {
	return mark.type === 'link';
}

/**
 * @param {import('../core/wasm.js').ContentMark} mark
 * @returns {mark is import('../core/wasm.js').ContentMark & { type: 'anchor'; attrs: { id: string } }}
 */
export function isAnchorMark(mark) {
	return mark.type === 'anchor';
}

/**
 * @param {import('../core/wasm.js').ContentLine} line
 * @returns {line is import('../core/wasm.js').ContentLine & { kind: 'heading'; attrs: { level: number } }}
 */
export function isHeadingLine(line) {
	return line.kind === 'heading';
}

/**
 * @param {import('../core/wasm.js').ContentLine} line
 * @returns {line is import('../core/wasm.js').ContentLine & { kind: 'code'; attrs?: { lang?: string } }}
 */
export function isCodeLine(line) {
	return line.kind === 'code';
}

/**
 * @param {import('../core/wasm.js').ContentContainer} container
 * @returns {container is import('../core/wasm.js').ContentContainer & { container: 'list_item'; attrs: { ordered: boolean; start: number; ordinal: number }; instance?: number }}
 */
export function isListItemContainer(container) {
	return container.container === 'list_item';
}

// ── Open-set membership guards ──────────────────────────────────────────────
// The guards above each answer "is this arm X". These four answer "is this a
// value this build knows?", the question any read-modify-write consumer must
// ask, since lowering an edit restates every line's kind and containers. A
// predicate rather than an exported name list, because the tables below are
// upstream's business: they are pinned against the Rust source by
// `tests/known_names_drift.rs`, so adding a built-in means editing there, here,
// and the TS unions in `src/engine.rs` in one commit.
//
// These classify unknown *tags*, not unknown *payloads on known tags*. A future
// `kind: "footnote"` with a sibling `ref` loses `ref` at a consumer that predates
// it either way.

const KNOWN_LINE_KINDS = new Set(['para', 'heading', 'code', 'island', 'rule']);
const KNOWN_CONTAINERS = new Set(['list_item', 'quote']);
const KNOWN_MARK_TYPES = new Set(['strong', 'emph', 'underline', 'strike', 'code', 'link', 'anchor']);
const KNOWN_ISLAND_TYPES = new Set(['table', 'image']);

/**
 * @param {import('../core/wasm.js').ContentLine} line
 * @returns {line is import('../core/wasm.js').ContentLine & { kind: string; attrs: unknown }}
 */
export function isUnknownLine(line) {
	return typeof line?.kind === 'string' && !KNOWN_LINE_KINDS.has(line.kind);
}

/**
 * @param {import('../core/wasm.js').ContentContainer} container
 * @returns {container is import('../core/wasm.js').ContentContainer & { container: string; attrs: unknown }}
 */
export function isUnknownContainer(container) {
	return typeof container?.container === 'string' && !KNOWN_CONTAINERS.has(container.container);
}

/**
 * @param {import('../core/wasm.js').ContentMark} mark
 * @returns {mark is import('../core/wasm.js').ContentMark & { type: string; attrs: unknown }}
 */
export function isUnknownMark(mark) {
	return typeof mark?.type === 'string' && !KNOWN_MARK_TYPES.has(mark.type);
}

/**
 * @param {import('../core/wasm.js').ContentIsland} island
 * @returns {island is import('../core/wasm.js').ContentIsland & { type: string; props: unknown }}
 */
export function isUnknownIsland(island) {
	return typeof island?.type === 'string' && !KNOWN_ISLAND_TYPES.has(island.type);
}

// ── Container run boundaries ────────────────────────────────────────────────
// `ContentContainer.instance` is what keeps two adjacent runs of one shape
// apart, and only a writer knows where a boundary is: the flat `containers`
// form cannot tell a list ending beside another from one list of two items, so
// an omitted discriminator welds them and nothing reports it.
//
// WELD_KEYS is the rule `Container::same_weld` owns upstream: which `attrs`
// entries two adjacent runs must share for the markdown projection to read them
// as one, and therefore for the canonical form to have to spend a
// discriminator. `start` is not among them, since CommonMark reads only a
// list's first number — a subset, which is why a built-in needs an entry rather
// than the unknown branch's whole-bag compare. A table rather than a switch, so
// `tests/known_names_drift.rs` can pin it against the Rust predicate.

const WELD_KEYS = { list_item: ['ordered'], quote: [] };

function sameJson(a, b) {
	if (a === b) return true;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
	if (Array.isArray(a) !== Array.isArray(b)) return false;
	const ka = Object.keys(a);
	return (
		ka.length === Object.keys(b).length &&
		ka.every((k) => Object.hasOwn(b, k) && sameJson(a[k], b[k]))
	);
}

/**
 * @param {import('../core/wasm.js').ContentContainer} a
 * @param {import('../core/wasm.js').ContentContainer} b
 * @returns {boolean}
 */
function weldsWith(a, b) {
	// A malformed value welds with nothing. The membership guards' posture:
	// answer rather than throw.
	if (typeof a?.container !== 'string' || a.container !== b?.container) return false;
	// `hasOwn`, so a tag colliding with an `Object.prototype` member reaches the
	// unknown branch rather than a function.
	if (!Object.hasOwn(WELD_KEYS, a.container)) return sameJson(a.attrs, b.attrs);
	return WELD_KEYS[a.container].every((k) => a.attrs?.[k] === b.attrs?.[k]);
}

/**
 * @param {(import('../core/wasm.js').ContentContainer | null)[]} runs
 * @returns {(import('../core/wasm.js').ContentContainer | null)[]}
 */
export function assignInstances(runs) {
	let prev = null;
	return runs.map((run) => {
		if (run == null) {
			prev = null;
			return null;
		}
		const instance = prev && weldsWith(prev, run) ? 1 - prev.instance : 0;
		prev = { ...run, instance };
		return prev;
	});
}

/**
 * Build a `load` thunk: dynamic-import a backend build, then instantiate it.
 *
 * Under `--target web` a freshly imported build is inert, so instantiation is
 * part of loading. Memoized at MODULE scope, not per `Engine`: two engines
 * issuing their first render concurrently must share one instantiation, and the
 * generated entry's own `wasm !== undefined` guard only catches a call arriving
 * after one finished, not one already in flight.
 *
 * @param {string} id backend id, for the failure message
 * @param {() => Promise<any>} importThunk the dynamic `import()`
 * @param {() => URL} wasmUrl the build's binary, resolved at call time
 * @returns {() => Promise<any>} resolves to a ready-to-use module
 */
function backendLoad(id, importThunk, wasmUrl) {
	/** @type {Promise<any> | undefined} */
	let loaded;
	return () =>
		(loaded ??= (async () => {
			const mod = await importThunk();
			await mod.default({ module_or_path: await toModuleSource(wasmUrl()) });
			return mod;
		})().catch((cause) => {
			// Self-heal, as core's `init` does.
			loaded = undefined;
			throw Object.assign(
				quillmarkError(
					'runtime::backend_load_failed',
					`Engine: could not load the '${id}' backend: ${
						/** @type {any} */ (cause)?.message ?? cause
					}`,
					'The backend binary ships beside the package files; check the network tab for a 404 or an HTML response.'
				),
				{ cause }
			);
		}));
}

// Backend builds are NEVER statically imported here: that would pull a multi-MB
// binary into the eager graph and defeat lazy loading. Each entry is a
// DESCRIPTOR: `load` dynamically imports and instantiates a backend's chunk, so
// the binary is fetched only when something renders against it; `formats` and
// `canvas` are the required static capability manifest, so the probes
// (`supportedFormats` / `supportsCanvas`) answer without loading the binary or
// cloning the quill. The manifest mirrors each backend's Rust `SUPPORTED_FORMATS`
// (and `formats_support_canvas`: true iff the list includes `svg` or `png`),
// pinned by a `runtime.test.js` drift guard that renders once and compares.
const DEFAULT_BACKENDS = {
	typst: {
		load: backendLoad(
			'typst',
			() => import('../backends/typst/wasm.js'),
			() => new URL('../backends/typst/wasm_bg.wasm', import.meta.url)
		),
		formats: ['pdf', 'svg', 'png'], // crates/backends/typst/src/lib.rs SUPPORTED_FORMATS
		canvas: true // has svg/png → formats_support_canvas == true
	},
	pdfform: {
		load: backendLoad(
			'pdfform',
			() => import('../backends/pdfform/wasm.js'),
			() => new URL('../backends/pdfform/wasm_bg.wasm', import.meta.url)
		),
		// crates/backends/pdfform/src/lib.rs SUPPORTED_FORMATS == [Pdf, Svg, Png]
		formats: ['pdf', 'svg', 'png'],
		canvas: true // has svg/png → formats_support_canvas == true
	}
};

/**
 * Validate a backend registry descriptor, naming the backend id on any
 * malformed entry. Failing at construction rather than deep inside a render is
 * what lets the capability probes answer from the manifest unconditionally.
 * @param {string} id
 * @param {unknown} entry
 * @returns {{ load: () => Promise<unknown>, formats: string[], canvas: boolean }}
 */
function validateBackend(id, entry) {
	if (!entry || typeof entry !== 'object') {
		throw new Error(
			`Engine: backend '${id}' must be a descriptor { load, formats, canvas }.`
		);
	}
	const { load, formats, canvas } = /** @type {any} */ (entry);
	if (typeof load !== 'function') {
		throw new Error(`Engine: backend '${id}' descriptor needs a callable 'load'.`);
	}
	if (!Array.isArray(formats)) {
		throw new Error(`Engine: backend '${id}' descriptor needs a 'formats' array.`);
	}
	if (typeof canvas !== 'boolean') {
		throw new Error(`Engine: backend '${id}' descriptor needs a boolean 'canvas'.`);
	}
	return { load, formats, canvas };
}

/**
 * Render dispatcher over the canonical `Quill`/`Document`. One `Engine`
 * instance can drive every backend; it resolves the right backend build from
 * each quill's declared `backendId` and loads it lazily on first use.
 */
export class Engine {
	/** backendId → Promise<backend module>, memoized so each build loads once. */
	#modules = new Map();
	/** backendId → that backend's engine instance (the WASM backend registry). */
	#engines = new Map();
	/** backendId → descriptor `{ load, formats, canvas }`. */
	#loaders;
	/**
	 * backendId → WeakMap<canonical Quill, backend-memory clone>, caching the
	 * expensive materialization. WeakMap so dropping the canonical quill makes
	 * its clone collectable, and wasm-bindgen weak-refs then free the handle.
	 * @type {Map<string, WeakMap<object, any>>}
	 */
	#quillClones = new Map();

	/**
	 * @param {{ backends?: Record<string, { load: () => Promise<unknown>, formats: string[], canvas: boolean }> }} [options]
	 *   Extra or overriding backend descriptors, merged over the built-ins. Each
	 *   is `{ load, formats, canvas }` with the manifest REQUIRED, since that is
	 *   what makes `supportedFormats` / `supportsCanvas` free; malformed entries
	 *   throw here, at construction.
	 *
	 *   `load` resolves to a READY module: a registrant shipping its own
	 *   `--target web` build instantiates inside the thunk. More than one
	 *   `Engine` may call it, so memoize (the built-ins do, at module scope).
	 */
	constructor(options) {
		const merged = { ...DEFAULT_BACKENDS, ...(options?.backends ?? {}) };
		/** @type {Record<string, { load: () => Promise<unknown>, formats: string[], canvas: boolean }>} */
		const loaders = {};
		for (const [id, entry] of Object.entries(merged)) {
			loaders[id] = validateBackend(id, entry);
		}
		this.#loaders = loaders;
	}

	/**
	 * The registered descriptor for `backendId`, or the "no backend registered"
	 * throw. Touches no binary.
	 * @param {string} backendId
	 * @returns {{ load: () => Promise<unknown>, formats: string[], canvas: boolean }}
	 */
	#descriptorFor(backendId) {
		const descriptor = this.#loaders[backendId];
		if (!descriptor) {
			throw new Error(
				`Engine: no backend registered for '${backendId}'. ` +
					`Known backends: ${Object.keys(this.#loaders).join(', ') || '(none)'}.`
			);
		}
		return descriptor;
	}

	/**
	 * `quill`'s backend id, after checking the handle. The ONE way an `Engine`
	 * verb reaches `backendId`, so "no verb touches a foreign quill" is
	 * structural rather than four remembered calls.
	 * @param {Quill} quill
	 * @param {string} method the caller's name, for the rejection message
	 * @returns {string}
	 */
	#backendOf(quill, method) {
		requireLocalQuill(quill, method);
		return quill.backendId;
	}

	/**
	 * Resolve (and lazily load) the backend module + its engine for `backendId`.
	 * @param {string} backendId
	 * @returns {Promise<{ mod: any, engine: any }>}
	 */
	async #resolveBackend(backendId) {
		const descriptor = this.#descriptorFor(backendId);

		let modPromise = this.#modules.get(backendId);
		if (!modPromise) {
			// Set the promise synchronously (before any await) so concurrent first
			// renders share ONE import. Self-heal on failure so a transient load
			// error doesn't poison every later attempt.
			modPromise = Promise.resolve()
				.then(descriptor.load)
				.catch((err) => {
					this.#modules.delete(backendId);
					throw err;
				});
			this.#modules.set(backendId, modPromise);
		}
		const mod = await modPromise;

		let engine = this.#engines.get(backendId);
		if (!engine) {
			engine = new mod.Quillmark();
			this.#engines.set(backendId, engine);
		}
		return { mod, engine };
	}

	/**
	 * Get (or materialize-and-cache) the backend-memory `Quill` clone for `quill`
	 * under `backendId`. On a miss the clone is built from `tree`, the caller's
	 * pre-await snapshot, since the canonical handle may be freed by now.
	 * @param {any} mod the backend build module
	 * @param {string} backendId
	 * @param {object} quill the canonical instance (cache key only)
	 * @param {Map<string, Uint8Array> | null} tree pre-await snapshot; `null` on a cache hit
	 * @returns {any} the backend-memory quill clone
	 */
	#cachedQuillClone(mod, backendId, quill, tree) {
		let perQuill = this.#quillClones.get(backendId);
		if (!perQuill) {
			perQuill = new WeakMap();
			this.#quillClones.set(backendId, perQuill);
		}
		let backendQuill = perQuill.get(quill);
		if (!backendQuill) {
			backendQuill = mod.Quill.fromTree(tree);
			perQuill.set(quill, backendQuill);
		}
		return backendQuill;
	}

	/**
	 * Materialize the backend-memory clones for `quill` + `doc` in `backendId`'s
	 * memory and run `fn` against the backend engine. Only `render`/`open` call
	 * this, so `doc` is always present.
	 *
	 * OWNERSHIP WINDOW: both caller handles are snapshotted (`doc.toStored()` and
	 * `doc.warnings`, and `quill.toTree()` on a clone-cache miss) BEFORE the first
	 * await. The backend load below is a real suspension point, so reading the
	 * handles after it would race a caller that `free()`s them as soon as this
	 * call returns its promise ("null pointer passed to rust").
	 *
	 * `docWarnings` rides the context because the storage DTO does not carry
	 * them: `fromStored` clears the load's warnings, so the backend clone knows
	 * nothing of them and `render` splices the snapshot back in.
	 *
	 * Clone lifetimes differ by design: the `doc` clone is TRANSIENT, freed in
	 * the `finally` of every call, while the `quill` clone is CACHED and is not
	 * freed here — a `Quill` instance's contents never change after
	 * construction, so it is dropped with the canonical quill when the consumer
	 * replaces the instance.
	 * @param {string} method the caller's name, for the rejection message
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @param {(ctx: { mod: any, engine: any, quill: any, doc: any, docWarnings: any[] }) => any} fn
	 */
	async #withClones(method, quill, doc, fn) {
		const backendId = this.#backendOf(quill, method);
		requireLocalDoc(doc, method);
		const docJson = doc.toStored();
		const docWarnings = doc.warnings;
		const quillTree = this.#quillClones.get(backendId)?.has(quill) ? null : quill.toTree();
		const { mod, engine } = await this.#resolveBackend(backendId);
		// The doc clone and `fn` share one try so the clone is freed even if a
		// later step throws; the cached quill clone is intentionally not freed.
		// `fn` MUST be synchronous: an async one would run against a freed clone.
		const backendQuill = this.#cachedQuillClone(mod, backendId, quill, quillTree);
		let backendDoc = null;
		try {
			backendDoc = mod.Document.fromStored(docJson);
			return fn({ mod, engine, quill: backendQuill, doc: backendDoc, docWarnings });
		} finally {
			backendDoc?.free();
		}
	}

	/**
	 * Render `doc` against `quill` in one shot. Both handles are read
	 * synchronously before the first await.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @param {object} [options] render options (`{ format, ppi, pages, producer }`)
	 * @returns {Promise<import('./runtime.js').RenderResult>}
	 */
	async render(quill, doc, options) {
		return this.#withClones(
			'engine.render(quill, doc)',
			quill,
			doc,
			({ engine, quill: q, doc: d, docWarnings }) => {
				const result = engine.render(q, d, options ?? undefined);
				result.warnings = docWarnings.concat(result.warnings);
				return result;
			}
		);
	}

	/**
	 * Open a live render session. It retains what `update` needs, so the
	 * transient clones are freed before this returns; the caller owns the session
	 * and must `.free()` it.
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @returns {Promise<LiveSession>}
	 */
	async open(quill, doc) {
		return this.#withClones(
			'engine.open(quill, doc)',
			quill,
			doc,
			({ mod, engine, quill: q, doc: d }) => new LiveSession(engine.open(q, d), mod)
		);
	}

	/**
	 * The output formats `quill`'s backend can emit: an always-free probe over
	 * the descriptor's manifest. `async` for API stability; it awaits nothing.
	 * @param {Quill} quill
	 * @returns {Promise<import('./runtime.js').OutputFormat[]>}
	 */
	async supportedFormats(quill) {
		const descriptor = this.#descriptorFor(this.#backendOf(quill, 'engine.supportedFormats(quill)'));
		// Defensive copy so callers can't mutate the shared manifest.
		return descriptor.formats.slice();
	}

	/**
	 * Whether `quill`'s backend can paint to a canvas: a pre-session estimate over
	 * the descriptor's manifest, so it can answer `true` where the resulting
	 * `LiveSession.supportsCanvas` answers `false`.
	 * @param {Quill} quill
	 * @returns {Promise<boolean>}
	 */
	async supportsCanvas(quill) {
		const descriptor = this.#descriptorFor(this.#backendOf(quill, 'engine.supportsCanvas(quill)'));
		return descriptor.canvas;
	}
}

/**
 * Thin wrapper over a backend's live render session; see `runtime.d.ts` for the
 * contract. The quill/document clones it was opened from have already been
 * freed: the session retains what `update` needs.
 */
export class LiveSession {
	/**
	 * @param {{ pageCount: number, backendId: string, supportsCanvas: boolean, warnings: any[], update: Function, render: Function, regions: Function, pageSize: Function, paint: Function, free: Function }} inner backend-build LiveSession (typst or pdfform)
	 * @param {{ Document: { fromStored(json: string): any } }} mod the session's backend build, used to materialize `update` documents in its linear memory
	 */
	constructor(inner, mod) {
		this.#inner = inner;
		this.#mod = mod;
	}
	#inner;
	#mod;

	/**
	 * @param {Document} doc
	 * @returns {import('./runtime.d.ts').ChangeSet}
	 */
	update(doc) {
		requireLocalDoc(doc, 'session.update(doc)');
		let backendDoc = null;
		try {
			backendDoc = this.#mod.Document.fromStored(doc.toStored());
			return this.#inner.update(backendDoc);
		} finally {
			backendDoc?.free();
		}
	}

	get pageCount() {
		return this.#inner.pageCount;
	}
	get backendId() {
		return this.#inner.backendId;
	}
	/**
	 * `true` iff `paint`/`pageSize` will succeed for THIS compile: the
	 * authoritative answer, which can be `false` where `Engine.supportsCanvas`
	 * answered `true` for the same quill.
	 * @returns {boolean}
	 */
	get supportsCanvas() {
		return this.#inner.supportsCanvas;
	}
	get warnings() {
		return this.#inner.warnings;
	}

	/** @param {object} [options] */
	render(options) {
		return this.#inner.render(options ?? undefined);
	}

	/**
	 * @returns {import('./runtime.d.ts').FieldRegion[]}
	 */
	regions() {
		return this.#inner.regions();
	}

	/**
	 * @param {string} field
	 * @returns {import('./runtime.d.ts').FieldRegion[]}
	 */
	fieldBoxes(field) {
		return this.#inner.fieldBoxes(field);
	}

	/**
	 * @param {number} page
	 * @param {number} x
	 * @param {number} y
	 * @param {number} [tolPt]
	 * @returns {string | undefined}
	 */
	fieldAt(page, x, y, tolPt) {
		return this.#inner.fieldAt(page, x, y, tolPt);
	}

	/**
	 * @param {number} page
	 * @param {number} x
	 * @param {number} y
	 * @param {number} [tolPt]
	 * @returns {import('./runtime.d.ts').ContentHit | undefined}
	 */
	positionAt(page, x, y, tolPt) {
		return this.#inner.positionAt(page, x, y, tolPt);
	}

	/**
	 * @param {string} field
	 * @param {number} pos
	 * @returns {import('./runtime.d.ts').FieldRegion | undefined}
	 */
	locate(field, pos) {
		return this.#inner.locate(field, pos);
	}

	/** @param {number} page */
	pageSize(page) {
		return this.#inner.pageSize(page);
	}

	/**
	 * @param {CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D} ctx
	 * @param {number} page
	 * @param {object} [options]
	 */
	paint(ctx, page, options) {
		return this.#inner.paint(ctx, page, options);
	}

	free() {
		this.#inner.free();
	}
}

// ── Typed-writer sugar: bind the quill once ─────────────────────────────────
// Rust exposes `quill.writer(&mut doc)` so a caller issues bare `set` / `set_all`
// without threading the schema per write. The WASM `commit*` verbs can't borrow
// like that: a `Document` carries only a `$quill` REFERENCE, not the resolved
// schema, so each `commit*` method takes the `quill` handle as its first
// argument. These pure-JS classes restore the Rust ergonomics: bind `quill` +
// `doc` once, then issue `set` / `setAll` / `card(i).set`.
//
// They hold JS references to the caller's EXISTING handles (no WASM object of
// their own, no `free()` burden, no second owner of either handle) and every
// write delegates straight to the underlying `commit*` verb: a schema field is
// typed-committed (coerced to canonical form, mismatch throws now), and a name
// the schema does not declare throws `UnknownField` rather than falling to the
// opaque store, on the typed path an undeclared name is a typo. Opaque storage
// stays available through the raw addressed `Document.storeField` verb.

/**
 * A {@link Document} bound to its {@link Quill} for typed writes: the JS twin
 * of Rust's `quill.writer(&mut doc)`. Writes target the main card; use
 * {@link card} for a composable card. Holds both handles by reference and owns
 * neither, so there is nothing to `free()`.
 */
export class DocumentWriter {
	#quill;
	#doc;
	/**
	 * @param {Quill} quill the schema source for typed commits
	 * @param {Document} doc the document to mutate, held by reference (not owned)
	 */
	constructor(quill, doc) {
		requireLocalQuill(quill, 'quill.writer(doc)');
		requireLocalDoc(doc, 'quill.writer(doc)');
		this.#quill = quill;
		this.#doc = doc;
	}
	/** The bound document: the same instance passed in, mutated in place. */
	get document() {
		return this.#doc;
	}
	/**
	 * Typed-commit one main-card field (strict coerce, mismatch throws now).
	 * Throws `UnknownField` for a name the schema does not declare.
	 * @param {string} name
	 * @param {unknown} value
	 * @returns {void}
	 */
	set(name, value) {
		return this.#doc._commitField(this.#quill, name, value);
	}
	/**
	 * Typed-commit several main-card fields atomically: nothing is applied on
	 * error (throws a per-field diagnostic bundle, including an `UnknownField`
	 * for each undeclared name).
	 * @param {Record<string, unknown>} fields
	 * @returns {void}
	 */
	setAll(fields) {
		return this.#doc._commitFields(this.#quill, MAIN_CARD_ADDR, fields);
	}
	/**
	 * Revise the main body from markdown; anchors rebase. A body carries no field
	 * schema, so this is the content lane's `revise` reached through the writer.
	 * @param {string} markdown
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseBody(markdown) {
		return this.#doc.revise({}, markdown);
	}
	/**
	 * Revise the content main-card field `name` from authored text: typed *and*
	 * anchor-preserving. Anchors rebase, then the diffed result is
	 * schema-conformed. The codec comes from the declared type: `richtext` diffs
	 * markdown, `plaintext` the literal text.
	 * @param {string} name
	 * @param {string} text
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseField(name, text) {
		return this.#doc._reviseField(this.#quill, name, text);
	}
	/**
	 * Build a composable card of `kind`, typed-commit `fields` onto it, set its
	 * body from optional markdown, and place it. Transactional: the card is
	 * committed in full before it joins the document, so a rejected field, kind,
	 * body, or position leaves the document untouched.
	 * @param {string} kind
	 * @param {Record<string, unknown>} [fields]
	 * @param {string} [body]
	 * @param {number} [at] insertion index; appends when omitted
	 * @returns {void}
	 */
	addCard(kind, fields, body, at) {
		return this.#doc._addCard(this.#quill, kind, fields, body, at);
	}
	/**
	 * @param {number} index
	 * @returns {import('../core/wasm.js').Card | undefined}
	 */
	removeCard(index) {
		return this.#doc.removeCard(index);
	}
	/**
	 * Write the document in the values form: the write twin of
	 * `reader.values()`. An absent axis is untouched; a present one is
	 * replaced: `fields` is the whole truth for declared names (an unnamed one
	 * is removed; an undeclared one the card holds is accepted unchanged and
	 * refused changed), `cards` is the card list, `body` the body, `ext: null`
	 * removes `$ext` and `{}` records an explicit empty one. All-or-nothing:
	 * nothing is applied on error and every refused cell is one diagnostic
	 * carrying its own `path`.
	 *
	 * A cell whose value equals its projection is not written, so writing back
	 * an unedited read changes no bytes. A changed content cell is a cold
	 * import — `reviseField` per cell is what keeps its anchors — and cards
	 * match by position and kind, so deleting or reordering an entry rewrites
	 * every card after it. An `undefined` member reads as absent.
	 * @param {DocumentValuesInput} values
	 * @returns {void}
	 */
	setValues(values) {
		return this.#doc._setValues(this.#quill, MAIN_CARD_ADDR, withoutUndefined(values));
	}
	/**
	 * A {@link CardWriter} bound to the composable card at `index`, checked
	 * lazily at the write. It holds `index`, not the card, so a
	 * `removeCard`/`addCard` between binding and writing silently retargets it.
	 * @param {number} index
	 * @returns {CardWriter}
	 */
	card(index) {
		return new CardWriter(this.#quill, this.#doc, index);
	}
}

/**
 * The values shape with every `undefined` member dropped, at the shape's own
 * members, its `fields` entries, and each card entry's. `undefined` is absent
 * (untouched) where `null` is a value (a removal, a present-null), and the
 * wasm boundary would fold the two together.
 * @param {unknown} values
 * @returns {unknown}
 */
function withoutUndefined(values) {
	if (values === null || typeof values !== 'object' || Array.isArray(values)) return values;
	const out = {};
	for (const [key, value] of Object.entries(values)) {
		if (value === undefined) continue;
		if (key === 'cards' && Array.isArray(value)) {
			out[key] = value.map(withoutUndefined);
		} else if (key === 'fields' && value !== null && typeof value === 'object') {
			out[key] = Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined));
		} else {
			out[key] = value;
		}
	}
	return out;
}

/**
 * A single composable card bound to its {@link Quill} for typed writes, from
 * {@link DocumentWriter.card}. Same `set` / `setAll` verbs as
 * {@link DocumentWriter}, targeting the card at its bound index.
 */
export class CardWriter {
	#quill;
	#doc;
	#index;
	/**
	 * @param {Quill} quill the schema source
	 * @param {Document} doc the document to mutate, held by reference (not owned)
	 * @param {number} index the composable card's index
	 */
	constructor(quill, doc, index) {
		requireLocalQuill(quill, 'writer.card(index)');
		requireLocalDoc(doc, 'writer.card(index)');
		this.#quill = quill;
		this.#doc = doc;
		this.#index = index;
	}
	/** The bound card index. */
	get index() {
		return this.#index;
	}
	/**
	 * The bound card's `$kind`, empty string when it carries none. Throws
	 * `IndexOutOfRange` for a bad bound index.
	 * @returns {string}
	 */
	get kind() {
		return this.#doc.card(this.#index).kind;
	}
	/**
	 * Typed-commit one field on this card, addressed at `{ card, field }`. Throws
	 * `UnknownField` for an undeclared name and `IndexOutOfRange` if the bound
	 * index is out of range.
	 * @param {string} name
	 * @param {unknown} value
	 * @returns {void}
	 */
	set(name, value) {
		return this.#doc._commitField(this.#quill, { card: this.#index, field: name }, value);
	}
	/**
	 * Typed-commit several fields on this card atomically, addressed at
	 * `{ card }`. Throws a per-field diagnostic bundle on error and
	 * `IndexOutOfRange` if the bound index is out of range.
	 * @param {Record<string, unknown>} fields
	 * @returns {void}
	 */
	setAll(fields) {
		return this.#doc._commitFields(this.#quill, { card: this.#index }, fields);
	}
	/**
	 * The card twin of {@link DocumentWriter.reviseBody}.
	 * @param {string} markdown
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseBody(markdown) {
		return this.#doc.revise({ card: this.#index }, markdown);
	}
	/**
	 * The card twin of {@link DocumentWriter.reviseField}. Throws `UnknownField`
	 * for an undeclared name and `IndexOutOfRange` for a bad bound index.
	 * @param {string} name
	 * @param {string} text
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseField(name, text) {
		return this.#doc._reviseField(this.#quill, { card: this.#index, field: name }, text);
	}
	/**
	 * Write this card in the values form: {@link DocumentWriter.setValues}
	 * restricted to one slot, under the same per-axis rule. An absent `kind`
	 * keeps the card's; a differing one rebuilds the slot. Refusals anchor at
	 * `cards.<kind>[index]`; throws `IndexOutOfRange` for a bad bound index.
	 * @param {CardValuesInput} values
	 * @returns {void}
	 */
	setValues(values) {
		return this.#doc._setValues(this.#quill, { card: this.#index }, withoutUndefined(values));
	}
}

// ── `quill.writer(doc)`, the typed front door ──────────────────────────────
// Patched onto the re-exported `Quill` prototype rather than wrapped, so
// `Quill === CoreQuill` stays true: this only adds a method constructing the
// pure-JS writer, which owns no WASM handle.
/**
 * A {@link DocumentWriter} binding this quill's schema to `doc`. It holds both
 * handles by reference and owns neither: bind, write, discard.
 * @this {Quill}
 * @param {Document} doc the document to mutate, held by reference (not owned)
 * @returns {DocumentWriter}
 */
Quill.prototype.writer = function writer(doc) {
	return new DocumentWriter(this, doc);
};

// ── Typed-reader sugar: the schema-plane read surface ─────────────────────────
// The transport `Document.getStored` is schema-free, so an unknown field name
// reads back `undefined` rather than as the typo it is. Binding the quill's
// schema lets one `get` interpret by declared type and throw `UnknownField` on a
// name the schema does not declare.

/**
 * A {@link Document} bound to its {@link Quill} for typed reads, the read
 * counterpart of {@link DocumentWriter}. Reads target the main card; use
 * {@link card} for a composable one. Owns neither handle.
 */
export class DocumentReader {
	#quill;
	#doc;
	/**
	 * @param {Quill} quill the schema source for interpreted reads
	 * @param {Document} doc the document to read, held by reference (not owned)
	 */
	constructor(quill, doc) {
		requireLocalQuill(quill, 'quill.reader(doc)');
		requireLocalDoc(doc, 'quill.reader(doc)');
		this.#quill = quill;
		this.#doc = doc;
	}
	/** The bound document: the same instance passed in. */
	get document() {
		return this.#doc;
	}
	/**
	 * Read the value at `addr`, interpreted by its declared type: a richtext field
	 * to markdown, every other type verbatim. A bare string is `Addr` shorthand
	 * for `{ field }`; an absent `addr.field` reads the body markdown. `undefined`
	 * for an absent field; throws `UnknownField` for a name the schema does not
	 * declare, `FieldDecode` for a richtext field holding an undecodable
	 * value, and `IndexOutOfRange` for a bad `addr.card`.
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @returns {unknown}
	 */
	get(addr) {
		return this.#doc._readerGet(this.#quill, addr);
	}
	/**
	 * Read the content field at `addr` as canonical `Content`: the twin of
	 * {@link get}, which projects. An absent `addr.field` reads the body
	 * `Content`. `undefined` for an absent field; throws `UnknownField`,
	 * `FieldNotContent` for a type that is not a content leaf, `FieldDecode` for
	 * an undecodable value, and `IndexOutOfRange` for a bad `addr.card`.
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContent(addr) {
		return this.#doc._readerGetContent(this.#quill, addr);
	}
	/**
	 * Read the `Content` nested inside the composite field at `addr`, at `path`:
	 * `[0]` an `array<richtext>` element, `["motto"]` an object's content property,
	 * `[1, "notes"]` a leaf under both, `["controlled_by"]` a variant's cell. The
	 * codec is the leaf's declared type's.
	 * `undefined` for an absent field and for a path that names nothing stored;
	 * throws `UnknownField`, `FieldNotContent` when `path` resolves to no content
	 * leaf, `FieldDecode` anchored at the addressed path, and `IndexOutOfRange`.
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @param {import('../core/wasm.js').PathStep[]} path
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContentAt(addr, path) {
		return this.#doc._readerGetContentAt(this.#quill, addr, path);
	}
	/**
	 * The main body's markdown: the quill-free body read. Equals `get({})`.
	 * @returns {string}
	 */
	bodyMarkdown() {
		return this.#doc._readerGet(this.#quill, {});
	}
	/**
	 * The whole document in the values form: the main card's fields, body and
	 * `$ext`, and every composable card, every content leaf as its codec's
	 * text, everything else as stored. Every axis is present, so the result is
	 * a valid {@link DocumentWriter.setValues} input and writing it back
	 * unedited changes no bytes. Never throws: a content leaf that decodes
	 * under neither encoding rides out as stored where {@link get} would throw.
	 * @returns {DocumentValues}
	 */
	values() {
		return this.#doc._readerValues(this.#quill, MAIN_CARD_ADDR);
	}
	/**
	 * The resolved-value view: for every declared field, the value the render
	 * projection would use and the rung it came from (`authored` / `default` /
	 * `blank`). The one read that blank-fills and coerces; {@link values} reports
	 * what the document carries. Value and provenance only; completeness stays
	 * `quill.validate`'s.
	 * @returns {Resolved}
	 */
	resolve() {
		return this.#quill._resolve(this.#doc);
	}
	/**
	 * A {@link CardReader} bound to the composable card at `index`, checked lazily
	 * at the read. It holds `index`, not the card, so a `removeCard`/`addCard`
	 * between binding and reading silently retargets it.
	 * @param {number} index
	 * @returns {CardReader}
	 */
	card(index) {
		return new CardReader(this.#quill, this.#doc, index);
	}
}

/**
 * A single composable card bound to its {@link Quill} for typed reads, from
 * {@link DocumentReader.card}. Same `get` / `bodyMarkdown` verbs as
 * {@link DocumentReader}, reading the card at its bound index.
 */
export class CardReader {
	#quill;
	#doc;
	#index;
	/**
	 * @param {Quill} quill the schema source
	 * @param {Document} doc the document to read, held by reference (not owned)
	 * @param {number} index the composable card's index
	 */
	constructor(quill, doc, index) {
		requireLocalQuill(quill, 'reader.card(index)');
		requireLocalDoc(doc, 'reader.card(index)');
		this.#quill = quill;
		this.#doc = doc;
		this.#index = index;
	}
	/** The bound card index. */
	get index() {
		return this.#index;
	}
	/**
	 * The bound card's `$kind` (empty string when it carries none), read through
	 * the document. Throws `IndexOutOfRange` if the bound index is out of range.
	 * @returns {string}
	 */
	get kind() {
		return this.#doc.card(this.#index).kind;
	}
	/**
	 * Read the field `name` on this card, interpreted by its declared type,
	 * addressed at `{ card, field }`. `undefined` when absent; throws
	 * `UnknownField` for an undeclared name and `IndexOutOfRange` for a bad index.
	 * @param {string} name
	 * @returns {unknown}
	 */
	get(name) {
		return this.#doc._readerGet(this.#quill, { card: this.#index, field: name });
	}
	/**
	 * The card twin of {@link DocumentReader.getContent}.
	 * @param {string} name
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContent(name) {
		return this.#doc._readerGetContent(this.#quill, { card: this.#index, field: name });
	}
	/**
	 * The card twin of {@link DocumentReader.getContentAt}.
	 * @param {string} name
	 * @param {import('../core/wasm.js').PathStep[]} path
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContentAt(name, path) {
		return this.#doc._readerGetContentAt(this.#quill, { card: this.#index, field: name }, path);
	}
	/**
	 * The card twin of {@link DocumentReader.bodyMarkdown}.
	 * @returns {string}
	 */
	bodyMarkdown() {
		return this.#doc._readerGet(this.#quill, { card: this.#index });
	}
	/**
	 * This card in the values form: {@link DocumentReader.values} restricted to
	 * one slot. Throws `IndexOutOfRange` for a bad bound index.
	 * @returns {CardValues}
	 */
	values() {
		return this.#doc._readerValues(this.#quill, { card: this.#index });
	}
}

// ── `quill.reader(doc)`: the schema-plane read front door ─────────────────────
// Patched onto the same re-exported `Quill` prototype as `writer`.
/**
 * A {@link DocumentReader} binding this quill's schema to `doc`. It holds both
 * handles by reference and owns neither: bind, read, discard.
 * @this {Quill}
 * @param {Document} doc the document to read, held by reference (not owned)
 * @returns {DocumentReader}
 */
Quill.prototype.reader = function reader(doc) {
	return new DocumentReader(this, doc);
};
