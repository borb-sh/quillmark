/* @ts-self-types="./runtime.d.ts" */
//
// The canonical consumer API, and the package's sole export. Its contract is
// `runtime.d.ts`, which a consumer's TypeScript reads through the package's
// `exports.types` map; the multi-binary design it implements is
// `prose/canon/BINDINGS.md`.
//
// A `Quill` instance's contents never change after construction — mutate by
// replacing the instance — which is what makes the cached backend clone below
// safe to hand out for the instance's whole life.

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

// A `--target web` build exports its classes synchronously but carries no wasm
// instance until something instantiates it. This module owns that for core,
// behind one awaited gate; `Engine` owns it for the backends, inside their lazy
// load. Which values that gate holds and which stay static exports:
// `prose/canon/BINDINGS.md`.

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
 * @param {import('../core/wasm.js').InitInput} [source]
 * @returns {Promise<import('./runtime.js').CoreSurface>}
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

/**
 * @type {import('../core/wasm.js').CardAddr}
 */
export const MAIN_CARD_ADDR = Object.freeze({});

/**
 * @type {'value'}
 */
export const VARIANT_DISCRIMINANT_KEY = 'value';

/**
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

// These checks deliver the ERROR, not the rejection. wasm-bindgen's glue already
// refuses a foreign class wherever a method declares a reference parameter, but
// its `_assertClass` throws a bare `Error` reading `expected instance of
// Document` at a value that IS a `Document`, so `isQuillmarkError` returns false
// and the failure leaves this package's error contract. They also cover the
// seams with no `_assertClass` to front-run: `Engine` and `LiveSession.update`
// cross into backend memory as data, where a foreign handle would silently work
// at the price of a whole-document round trip and a per-copy split of the quill
// clone cache.

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

// A predicate rather than an exported name list, because these tables are
// upstream's business: `tests/known_names_drift.rs` pins them against the Rust
// source, so adding a built-in means editing there, here, and the TS unions in
// `src/engine.rs` in one commit.

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

// WELD_KEYS is the rule `Container::same_weld` owns upstream: which `attrs`
// entries two adjacent runs must share for the markdown projection to read them
// as one. `start` is not among them, since CommonMark reads only a list's first
// number — a subset, which is why a built-in needs an entry rather than the
// unknown branch's whole-bag compare. A table rather than a switch, so
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
// binary into the eager graph and defeat lazy loading. Each `formats` manifest
// mirrors that backend's Rust `SUPPORTED_FORMATS`, pinned by a
// `runtime.test.js` drift guard that renders once and compares.
const DEFAULT_BACKENDS = {
	typst: {
		load: backendLoad(
			'typst',
			() => import('../backends/typst/wasm.js'),
			() => new URL('../backends/typst/wasm_bg.wasm', import.meta.url)
		),
		formats: ['pdf', 'svg', 'png'] // crates/backends/typst/src/lib.rs SUPPORTED_FORMATS
	},
	pdfform: {
		load: backendLoad(
			'pdfform',
			() => import('../backends/pdfform/wasm.js'),
			() => new URL('../backends/pdfform/wasm_bg.wasm', import.meta.url)
		),
		formats: ['pdf'] // crates/backends/pdfform/src/lib.rs SUPPORTED_FORMATS
	}
};

/**
 * Validate a backend registry descriptor, naming the backend id on any
 * malformed entry. Failing at construction rather than deep inside a render is
 * what lets `supportedFormats` answer from the manifest unconditionally.
 * @param {string} id
 * @param {unknown} entry
 * @returns {{ load: () => Promise<unknown>, formats: string[] }}
 */
function validateBackend(id, entry) {
	if (!entry || typeof entry !== 'object') {
		throw new Error(`Engine: backend '${id}' must be a descriptor { load, formats }.`);
	}
	const { load, formats } = /** @type {any} */ (entry);
	if (typeof load !== 'function') {
		throw new Error(`Engine: backend '${id}' descriptor needs a callable 'load'.`);
	}
	if (!Array.isArray(formats)) {
		throw new Error(`Engine: backend '${id}' descriptor needs a 'formats' array.`);
	}
	return { load, formats };
}

export class Engine {
	/** backendId → Promise<backend module>, memoized so each build loads once. */
	#modules = new Map();
	/** backendId → that backend's engine instance (the WASM backend registry). */
	#engines = new Map();
	/** backendId → descriptor `{ load, formats }`. */
	#loaders;
	/**
	 * backendId → WeakMap<canonical Quill, backend-memory clone>, caching the
	 * expensive materialization. WeakMap so dropping the canonical quill makes
	 * its clone collectable, and wasm-bindgen weak-refs then free the handle.
	 * @type {Map<string, WeakMap<object, any>>}
	 */
	#quillClones = new Map();

	/**
	 * `load` must resolve to a READY module: a registrant shipping its own
	 * `--target web` build instantiates inside the thunk, and more than one
	 * `Engine` may call it, so memoize (the built-ins do, at module scope).
	 * @param {{ backends?: Record<string, { load: () => Promise<unknown>, formats: string[] }> }} [options]
	 */
	constructor(options) {
		const merged = { ...DEFAULT_BACKENDS, ...(options?.backends ?? {}) };
		/** @type {Record<string, { load: () => Promise<unknown>, formats: string[] }>} */
		const loaders = {};
		for (const [id, entry] of Object.entries(merged)) {
			loaders[id] = validateBackend(id, entry);
		}
		this.#loaders = loaders;
	}

	/**
	 * The registered descriptor for `backendId`, or the
	 * `engine::backend_not_found` throw core raises for the same condition.
	 * Touches no binary.
	 * @param {string} backendId
	 * @returns {{ load: () => Promise<unknown>, formats: string[] }}
	 */
	#descriptorFor(backendId) {
		const descriptor = this.#loaders[backendId];
		if (!descriptor) {
			throw quillmarkError(
				'engine::backend_not_found',
				`Engine: backend '${backendId}' not registered or not enabled.`,
				`Available backends: ${Object.keys(this.#loaders).join(', ') || '(none)'}`
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
	 * @param {Quill} quill
	 * @param {Document} doc
	 * @param {object} [options] render options (`{ format, ppi, pages, regions }`)
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
	 * @param {Quill} quill
	 * @returns {Promise<import('./runtime.js').OutputFormat[]>}
	 */
	async supportedFormats(quill) {
		const descriptor = this.#descriptorFor(this.#backendOf(quill, 'engine.supportedFormats(quill)'));
		// Defensive copy so callers can't mutate the shared manifest.
		return descriptor.formats.slice();
	}
}

/**
 * Thin wrapper over a backend's live render session. The quill/document clones
 * it was opened from have already been freed: the session retains what `update`
 * needs.
 */
export class LiveSession {
	/**
	 * @param {object} inner backend-build LiveSession (typst or pdfform), whose members the delegations below name
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

// These pure-JS classes bind a `quill` + `doc` pair once and delegate to the
// `_commit*` / `_reader*` verbs, which take the quill per call. They hold JS
// references to the caller's existing handles, so there is no WASM object of
// their own, no `free()` burden, and no second owner of either handle.

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
	get document() {
		return this.#doc;
	}
	/**
	 * @param {string} name
	 * @param {unknown} value
	 * @returns {void}
	 */
	set(name, value) {
		return this.#doc._commitField(this.#quill, name, value);
	}
	/**
	 * @param {Record<string, unknown>} fields
	 * @returns {void}
	 */
	setAll(fields) {
		return this.#doc._commitFields(this.#quill, MAIN_CARD_ADDR, fields);
	}
	/**
	 * @param {string} markdown
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseBody(markdown) {
		return this.#doc.revise({}, markdown);
	}
	/**
	 * @param {string} name
	 * @param {string} text
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseField(name, text) {
		return this.#doc._reviseField(this.#quill, name, text);
	}
	/**
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
	 * @param {DocumentValuesInput} values
	 * @returns {void}
	 */
	setValues(values) {
		return this.#doc._setValues(this.#quill, MAIN_CARD_ADDR, withoutUndefined(values));
	}
	/**
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
	get index() {
		return this.#index;
	}
	/**
	 * @returns {string}
	 */
	get kind() {
		return this.#doc.card(this.#index).kind;
	}
	/**
	 * @param {string} name
	 * @param {unknown} value
	 * @returns {void}
	 */
	set(name, value) {
		return this.#doc._commitField(this.#quill, { card: this.#index, field: name }, value);
	}
	/**
	 * @param {Record<string, unknown>} fields
	 * @returns {void}
	 */
	setAll(fields) {
		return this.#doc._commitFields(this.#quill, { card: this.#index }, fields);
	}
	/**
	 * @param {string} markdown
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseBody(markdown) {
		return this.#doc.revise({ card: this.#index }, markdown);
	}
	/**
	 * @param {string} name
	 * @param {string} text
	 * @returns {import('../core/wasm.js').Delta}
	 */
	reviseField(name, text) {
		return this.#doc._reviseField(this.#quill, { card: this.#index, field: name }, text);
	}
	/**
	 * @param {CardValuesInput} values
	 * @returns {void}
	 */
	setValues(values) {
		return this.#doc._setValues(this.#quill, { card: this.#index }, withoutUndefined(values));
	}
}

// Patched onto the re-exported `Quill` prototype rather than wrapped, so
// `Quill === CoreQuill` stays true: this only adds a method constructing the
// pure-JS writer, which owns no WASM handle.
/**
 * @this {Quill}
 * @param {Document} doc the document to mutate, held by reference (not owned)
 * @returns {DocumentWriter}
 */
Quill.prototype.writer = function writer(doc) {
	return new DocumentWriter(this, doc);
};

// The transport `Document.getStored` is schema-free, so an unknown field name
// reads back `undefined` rather than as the typo it is. Binding the quill's
// schema lets one `get` interpret by declared type and throw `UnknownField`.

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
	get document() {
		return this.#doc;
	}
	/**
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @returns {unknown}
	 */
	get(addr) {
		return this.#doc._readerGet(this.#quill, addr);
	}
	/**
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContent(addr) {
		return this.#doc._readerGetContent(this.#quill, addr);
	}
	/**
	 * @param {import('../core/wasm.js').Addr | string} addr
	 * @param {import('../core/wasm.js').PathStep[]} path
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContentAt(addr, path) {
		return this.#doc._readerGetContentAt(this.#quill, addr, path);
	}
	/**
	 * @returns {string}
	 */
	bodyMarkdown() {
		return this.#doc._readerGet(this.#quill, {});
	}
	/**
	 * @returns {DocumentValues}
	 */
	values() {
		return this.#doc._readerValues(this.#quill, MAIN_CARD_ADDR);
	}
	/**
	 * @returns {Resolved}
	 */
	resolve() {
		return this.#quill._resolve(this.#doc);
	}
	/**
	 * @param {number} index
	 * @returns {CardReader}
	 */
	card(index) {
		return new CardReader(this.#quill, this.#doc, index);
	}
}

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
	get index() {
		return this.#index;
	}
	/**
	 * @returns {string}
	 */
	get kind() {
		return this.#doc.card(this.#index).kind;
	}
	/**
	 * @param {string} name
	 * @returns {unknown}
	 */
	get(name) {
		return this.#doc._readerGet(this.#quill, { card: this.#index, field: name });
	}
	/**
	 * @param {string} name
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContent(name) {
		return this.#doc._readerGetContent(this.#quill, { card: this.#index, field: name });
	}
	/**
	 * @param {string} name
	 * @param {import('../core/wasm.js').PathStep[]} path
	 * @returns {import('../core/wasm.js').Content | undefined}
	 */
	getContentAt(name, path) {
		return this.#doc._readerGetContentAt(this.#quill, { card: this.#index, field: name }, path);
	}
	/**
	 * @returns {string}
	 */
	bodyMarkdown() {
		return this.#doc._readerGet(this.#quill, { card: this.#index });
	}
	/**
	 * @returns {CardValues}
	 */
	values() {
		return this.#doc._readerValues(this.#quill, { card: this.#index });
	}
}

// Patched onto the same re-exported `Quill` prototype as `writer`.
/**
 * @this {Quill}
 * @param {Document} doc the document to read, held by reference (not owned)
 * @returns {DocumentReader}
 */
Quill.prototype.reader = function reader(doc) {
	return new DocumentReader(this, doc);
};
