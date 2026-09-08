// The canonical consumer API, and the package's sole export: `@quillmark/wasm`
// resolves here.
//
// The render-side types are defined HERE as the backend-neutral render contract,
// not sourced from any one private backend build; `runtime.types.test-d.ts`
// asserts they stay mutually assignable with the Typst backend's generated
// declarations.
//
// The `Quill`/`Document` `init` resolves to ARE the core build's classes, never
// wrappers. Two copies of this package are two WASM linear memories and two
// `Quill`/`Document` classes, so every method taking a handle refuses one
// belonging to another copy, with a `QuillmarkError` naming
// `npm ls @quillmark/wasm`. Errors are the exception: `isQuillmarkError` is
// structural.

// The instance types, so an annotation (`let q: Quill`) needs no await. Their
// values are `CoreSurface`'s.
export type { Quill, Document } from '../core/wasm.js';

import type {
	InitInput,
	Quill as CoreQuill,
	Document as CoreDocument,
	importMarkdown,
	exportMarkdown,
	rebase,
	mapPos,
	mapMarks,
	parseDocPath,
	formatDocPath
} from '../core/wasm.js';

/**
 * The core build's surface, and therefore what `init` resolves to. Exported
 * nowhere statically, so awaiting is the only way to hold one.
 *
 * `Quill` and `Document` here are the classes, statics included, not the
 * instance types above.
 */
export interface CoreSurface {
	Quill: typeof CoreQuill;
	Document: typeof CoreDocument;
	importMarkdown: typeof importMarkdown;
	exportMarkdown: typeof exportMarkdown;
	rebase: typeof rebase;
	mapPos: typeof mapPos;
	mapMarks: typeof mapMarks;
	parseDocPath: typeof parseDocPath;
	formatDocPath: typeof formatDocPath;
}

/**
 * Instantiate the core WASM build and resolve to its surface.
 *
 * ```ts
 * import { init } from '@quillmark/wasm';
 * const { Quill, Document } = await init();
 * ```
 *
 * The same line works everywhere: the binary streams from a URL in a browser
 * and is read off disk under Node.
 *
 * The only door to `Quill`, `Document` and the free functions, so the pre-init
 * mistake is not expressible. Destructure at each entry point (route loader,
 * hydration path, worker) rather than threading one result around: the gate is
 * memoized and concurrency-safe, so every await after the first is free, and a
 * failed init clears the memo. Each realm initializes its own copy.
 *
 * Both failure codes reject, so one `catch` covers the gate. Backends are not
 * initialized here: `Engine` instantiates one on first render against it.
 *
 * @param source override the binary's source (bytes, a `Response`, a
 *   `WebAssembly.Module`, a URL) for hosts that route assets themselves or
 *   embed the binary. Pass it on the FIRST call; a later call passing a
 *   different source rejects with `runtime::init_conflict` rather than silently
 *   ignoring it. Passing the same value again is fine.
 */
export declare function init(source?: InitInput): Promise<CoreSurface>;

import type { CardAddr } from '../core/wasm.js';

/**
 * The main card's address: a named, {@link CardAddr}-typed alias for the empty
 * address `{}`, so a main-card write names its target. It *is* `{}` (frozen at
 * runtime), so `{}` and `undefined` stay equally valid. A card selector only,
 * never a field address.
 */
export declare const MAIN_CARD_ADDR: CardAddr;

/**
 * The key carrying the discriminant inside a variant-bearing enum's value. A
 * field declaring `variants:` rests as `{value: <member>, …that member's
 * fields}`, so reading or writing one means naming this key; it crosses the
 * boundary inside untyped container data, with no type to read it off.
 * Reserved: no variant may declare a field under it, and
 * {@link QuillFieldSchema.variants}, keyed by member, never contains it.
 */
export declare const VARIANT_DISCRIMINANT_KEY: 'value';

// Core-build types consumers read off `Quill`/`Document`.
export type {
	Card,
	PayloadItem,
	Diagnostic,
	Location,
	Severity,
	QuillSchema,
	QuillFieldSchema,
	QuillCardSchema,
	QuillCardBody,
	QuillFieldUi,
	QuillCardUi,
	QuillGroupUi,
	QuillMetadata
} from '../core/wasm.js';

// Content edit vocabulary: the op-grained content model `Document`'s methods
// speak (`applyChange(addr, bundle)`, `overwrite(addr, rt)`, `revise(…) => Delta`).
// Declared in the core build; re-exported here so the single public entry point
// names every type its own re-exported surface already references: `Card.body`
// is a `Content`, `PayloadItem.nestedFills` a `PathStep[][]`, `CardInput.body` a
// `Content | string`: rather than forcing consumers to derive them structurally
// off the `Document` handle. The content write path (a ProseMirror↔content codec)
// must name all of them; they are its correctness core, not edge types.
// `ContentLineKind` is the shared half of `ContentLine` and `setKind`, so lifting
// a line's kind whole (destructure off `containers`/`continues`, spread the rest
// into the op) is the version-proof spelling of building a `setKind`. Naming it
// is what makes that spelling type-check without a cast. The alternative, an
// arm-by-arm switch, means guessing at the open arm's shape and re-editing on
// every arm added.
export type {
	Content,
	ContentLine,
	ContentLineKind,
	ContentContainer,
	ContentMark,
	ContentIsland,
	TableProps,
	ImageProps,
	TableCell,
	CardInput,
	PathStep,
	Addr,
	CardAddr,
	Delta,
	Assoc,
	IslandOp,
	LineOp,
	MarkOp,
	ChangeBundle,
	DocPathSeg
} from '../core/wasm.js';

// The two schema-bound whole-document reads on `quill.reader(doc)`: the values
// form (`reader.values()`, what the document carries, content leaves as their
// codec's text) and the resolved view (`reader.resolve()`, value + source rung
// per declared field, the body a `body` sibling on its card, never a row in
// `fields`); diagnostics stay `quill.validate`, guidance stays `quill.schema`.
// Declared in the core build's generated `.d.ts` via a
// `typescript_custom_section`; re-exported here so the single public entry
// point names them.
export type {
	FieldSource,
	ResolvedField,
	ResolvedMain,
	ResolvedCard,
	Resolved,
	CardValues,
	DocumentValues,
	CardValuesInput,
	DocumentValuesInput
} from '../core/wasm.js';

// ── Error contract ──────────────────────────────────────────────────────────

/**
 * The error every fallible method in this package throws: parse
 * (`Document.fromMarkdown`), document mutation, validation
 * (`Quill.fromTree`, `quill.validate`), and rendering (`engine.render`,
 * `engine.open`, `session.render`).
 *
 * This is a STRUCTURAL interface, not a class: the WASM layer throws a real
 * `Error` and attaches `diagnostics` to it, so there is no constructor to
 * `instanceof` against, narrow with {@link isQuillmarkError}. `diagnostics`
 * is always non-empty; `message` is the first diagnostic's message (or an
 * `"N error(s): …"` aggregate for multi-diagnostic failures), so iterate
 * `diagnostics` for per-error detail. The shape is identical to
 * `RenderResult.warnings` entries.
 */
export interface QuillmarkError extends Error {
	diagnostics: Diagnostic[];
}

/**
 * Narrow an unknown caught value to {@link QuillmarkError}. Structural
 * (`Error` carrying a `diagnostics` array), so it narrows errors from any build
 * or WASM instance in the page — unlike a handle, which is refused when it
 * comes from a second copy of this package.
 */
export declare function isQuillmarkError(e: unknown): e is QuillmarkError;

// `ContentIsland.type`, `ContentMark.type`, `ContentLine.kind`, and
// `ContentContainer.container` are open sets: each union has a residual
// `{ …: string; … }` arm, so a bare discriminant check never narrows the payload
// (TS keeps the residual arm live, since a `string` can equal the literal).
// These guards are the checked narrowing path for the pinned arms; only the
// payload-carrying arms get one, since the rest narrow to nothing.

import type {
	ContentIsland,
	TableProps,
	ImageProps,
	ContentMark,
	ContentLine,
	ContentContainer
} from '../core/wasm.js';

/** Narrow a {@link ContentIsland} to the pinned `table` arm (`props: TableProps`). */
export declare function isTableIsland(
	island: ContentIsland
): island is ContentIsland & { type: 'table'; props: TableProps };

/** Narrow a {@link ContentIsland} to the pinned `image` arm (`props: ImageProps`). */
export declare function isImageIsland(
	island: ContentIsland
): island is ContentIsland & { type: 'image'; props: ImageProps };

/** Narrow a {@link ContentMark} to the `link` arm (carries `attrs.url`). */
export declare function isLinkMark(
	mark: ContentMark
): mark is ContentMark & { type: 'link'; attrs: { url: string } };

/** Narrow a {@link ContentMark} to the `anchor` arm (carries `attrs.id`). */
export declare function isAnchorMark(
	mark: ContentMark
): mark is ContentMark & { type: 'anchor'; attrs: { id: string } };

/** Narrow a {@link ContentLine} to the `heading` arm (carries `attrs.level`). */
export declare function isHeadingLine(
	line: ContentLine
): line is ContentLine & { kind: 'heading'; attrs: { level: number } };

/** Narrow a {@link ContentLine} to the `code` arm (carries `attrs.lang`). */
export declare function isCodeLine(
	line: ContentLine
): line is ContentLine & { kind: 'code'; attrs?: { lang?: string } };

/** Narrow a {@link ContentContainer} to the `list_item` arm (carries its shape). */
export declare function isListItemContainer(
	container: ContentContainer
): container is ContentContainer & {
	container: 'list_item';
	attrs: { ordered: boolean; start: number; ordinal: number };
	instance: number;
};

// The guards above answer "is this arm X". These four answer "is this a value
// this build knows?", the question a read-modify-write consumer must ask: an
// edit restates every line's kind and containers, so a construct the consumer
// cannot hold is gone on write-back unless carried inertly, and enumerating the
// built-in names by hand re-couples to a closed set.
//
// They classify unknown TAGS, not unknown payloads on known tags: a future
// `kind: "footnote"` carrying an `attrs.ref` loses `ref` at any consumer that
// predates it, with or without these. The spelling needs no classifying: a
// payload rides `attrs` whether or not this build knows the name.

/** True when this build does not know `line.kind`: the open arm, carrying opaque `attrs`. */
export declare function isUnknownLine(
	line: ContentLine
): line is ContentLine & { kind: string; attrs: unknown };

/** True when this build does not know `container.container`. See {@link isUnknownLine}. */
export declare function isUnknownContainer(
	container: ContentContainer
): container is ContentContainer & { container: string; attrs: unknown };

/** True when this build does not know `mark.type`. See {@link isUnknownLine}. */
export declare function isUnknownMark(
	mark: ContentMark
): mark is ContentMark & { type: string; attrs: unknown };

/** True when this build does not know `island.type` (its payload rides `props`, not `attrs`). */
export declare function isUnknownIsland(
	island: ContentIsland
): island is ContentIsland & { type: string; props: unknown };

// `ContentContainer.instance` is required, so a checker reports an omission; it
// cannot report a `0` stamped on every run, which is the same write. Adjacent
// runs of one shape sharing a value arrive welded, and nothing reports that
// either: the flat `containers` form cannot tell it from one container spanning
// two paragraphs. This carries the rule a codec would otherwise re-derive.

/**
 * Stamp `instance` across one parent's blocks at one depth, in document order,
 * returning containers ready to write.
 *
 * One entry per container RUN — a list, not a list item — and `null` for a
 * block carrying no container at this depth. A bare paragraph between two lists
 * is such a block, and separates them on its own. Every line of a run then
 * carries that run's returned container, `ordinal` varying per item and
 * `instance` held.
 *
 * The `instance` it stamps is canonical, so a document reads back the value it
 * was written. `ordinal` stays the caller's, and a write is renumbered to a
 * gapless index within its run.
 *
 * Which fields decide a weld is coarser than equality for a list: CommonMark
 * reads only a list's first number, so `1. a` beside `3. b` welds despite the
 * differing `start`.
 *
 * ```js
 * const [outer, , inner] = assignInstances([listA, null, listB]);
 * // outer.instance === 0, inner.instance === 0 — the paragraph parts them
 * const [a, b] = assignInstances([listA, listB]);
 * // a.instance === 0, b.instance === 1 — adjacent, one shape
 * ```
 */
export declare function assignInstances(
	runs: (ContentContainer | null)[]
): (ContentContainer | null)[];

// The backend-neutral render contract, defined here rather than re-exported from
// one private backend because no single backend owns the canonical API's types.
// Every backend build must satisfy these shapes; `runtime.types.test-d.ts` keeps
// them from diverging from the generated `pkg/backends/typst/wasm.d.ts`.

import type { Quill, Document, Card } from '../core/wasm.js';
import type { Diagnostic } from '../core/wasm.js';

/** One emitted output. */
export interface Artifact {
	format: OutputFormat;
	bytes: Uint8Array;
	mimeType: string;
}

/** Options for one render. */
export interface RenderOptions {
	format?: OutputFormat;
	/**
	 * Pixels per inch for raster formats (PNG); ignored by PDF and SVG.
	 * Defaults to 144. Must be finite, above 0, and small enough to keep every
	 * rendered page under 268435456 pixels — anything else throws
	 * `backend::invalid_raster_scale`.
	 */
	ppi?: number;
	pages?: number[];
	producer?: string;
	/**
	 * Populate {@link RenderResult.regions} with schema-field geometry, for
	 * consumers without a live session. Defaults to `false`.
	 */
	regions?: boolean;
}

/**
 * How precisely a {@link ContentHit.pos} resolved: the marker a caret UI reads
 * to decide whether to trust the offset. Never sub-cluster: `'cluster'` is the
 * finest, `'segment'` the floor it degrades to on origin-less ink.
 *
 * - `'cluster'`: `pos` is the first content char of the cluster under the point.
 *   Place the caret there directly.
 * - `'segment'`: the point hit origin-less ink (list markers, numbering, a code
 *   fence's interior), so `pos` is the containing segment's start, not a caret.
 */
export type HitGranularity = 'cluster' | 'segment';

/** A click resolved to a field and USV offset into its Content. */
export interface ContentHit {
	/**
	 * The field's canonical `DocPath` address (`parseDocPath`-routable): the same
	 * address {@link LiveSession.fieldAt} returns for that point.
	 */
	field: string;
	pos: number;
	/**
	 * Whether {@link pos} is cluster-exact or floored to the segment start
	 * ({@link HitGranularity}). Absent when the backend does not report it.
	 */
	granularity?: HitGranularity;
}

/**
 * A rendered field region: the canonical `DocPath` field address (`field`) plus
 * its geometry (`rect`) on the page. Only fields with a schema address produce
 * one: a backend-only widget produces none, and the backend widget name never
 * appears.
 *
 * Use it to scroll to or highlight the focused field's rect; for the click
 * direction use {@link LiveSession.fieldAt}, which resolves a point on *any*
 * placement, not just the first one surfaced here.
 *
 * COORDINATE TRANSFORM. `rect` is in PDF points with a **bottom-left** origin.
 *
 * For an **HTML/CSS overlay** on a `width:100%` canvas, position hotspots as
 * percentages of the page, so they track the displayed size across DPI and pane
 * resize for free; only the Y axis flips:
 *
 * ```js
 * const [x0, y0, x1, y1] = region.rect;            // PDF pt, bottom-left origin
 * const left   = (x0 / pageWidthPt) * 100;         // % of page (from PageSize.widthPt)
 * const top    = (1 - y1 / pageHeightPt) * 100;    // %: flip Y (from PageSize.heightPt)
 * const width  = ((x1 - x0) / pageWidthPt) * 100;
 * const height = ((y1 - y0) / pageHeightPt) * 100;
 * ```
 *
 * For painting **into a raster** at `renderScale` (= `layoutScale × densityScale`),
 * use the device-pixel form instead:
 *
 * ```js
 * const left   = x0 * renderScale;
 * const top    = (pageHeightPt - y1) * renderScale;  // flip Y
 * ```
 */
export interface FieldRegion {
	/**
	 * The field's canonical `DocPath` address (`parseDocPath`-routable), not a
	 * backend widget name: `main.signature_block`,
	 * `cards.<kind>[<i>].signature_block` (`cards[<i>].…` when the card's kind is
	 * unknown), an array element bracketed and a key dotted. The same spelling
	 * `Diagnostic.path` uses, so the two join on string equality.
	 */
	field: string;
	/** 0-based page index. */
	page: number;
	/** `[x0, y0, x1, y1]` in PDF points (1/72″), bottom-left origin. */
	rect: [number, number, number, number];
	/**
	 * The content slice this box covers: USV `[start, end)` into the field's
	 * `Content` for content ink (one segment), absent for a scalar reference
	 * site or widget. Consumers key segment highlights on it;
	 * {@link LiveSession.fieldBoxes} unions same-page segments for the
	 * whole-field box.
	 */
	span?: [number, number];
}

/** Result of one render. */
export interface RenderResult {
	artifacts: Artifact[];
	warnings: Diagnostic[];
	outputFormat: OutputFormat;
	renderTimeMs: number;
	/**
	 * Schema-field geometry, populated only when {@link RenderOptions.regions}
	 * asked for it. Page indices are document-space even under a `pages` subset.
	 */
	regions: FieldRegion[];
}

/** The emittable formats. */
export type OutputFormat = 'pdf' | 'svg' | 'png';

/** Page geometry, in points. */
export interface PageSize {
	widthPt: number;
	heightPt: number;
}

/** Inputs to `paint`. */
export interface PaintOptions {
	layoutScale?: number;
	densityScale?: number;
}

/** Output of `paint`. */
export interface PaintResult {
	layoutWidth: number;      // canvas.style.width target; independent of densityScale
	layoutHeight: number;
	pixelWidth: number;       // canvas.width the painter wrote (clamped at 16384)
	pixelHeight: number;
	/**
	 * True when the backing-store clamp forced `densityScale` down: the page
	 * renders soft at the same `canvas.style` size.
	 */
	clamped: boolean;
	/**
	 * The `densityScale` actually applied, reduced proportionally when
	 * `clamped`. `layoutScale × effectiveDensityScale` is the rasterized scale.
	 */
	effectiveDensityScale: number;
}

/**
 * Output of {@link LiveSession.update}: `dirtyPages` lists the pages whose
 * content differs from the previous compile, including added pages; removed
 * pages are implied by `pageCount`. Repaint `dirty ∩ visible`.
 */
export interface ChangeSet {
	pageCount: number;
	dirtyPages: number[];
}

/**
 * A backend registry entry. `load` is the lazy thunk returning the
 * dynamically-imported backend build module; `formats` is the required static
 * capability manifest, which is what makes `Engine.supportedFormats` free: it
 * answers from it without loading a backend binary or cloning a quill. A
 * malformed descriptor throws at `new Engine(...)`.
 */
export interface BackendDescriptor {
	load: () => Promise<unknown>;
	formats: OutputFormat[];
}

export interface EngineOptions {
	/**
	 * Extra or overriding backend descriptors, merged over the built-ins. Keys are
	 * backend ids (as declared by `Quill.yaml`'s `backend:` and reported by
	 * `Quill.backendId`). Each value is a `BackendDescriptor`: `formats` is
	 * required, so the format probe is ALWAYS free (no binary load, no quill
	 * clone). Malformed entries throw at construction. The default registry maps
	 * `"typst"` and `"pdfform"` to the bundled backend builds.
	 */
	backends?: Record<string, BackendDescriptor>;
}

/**
 * Render dispatcher over the canonical `Quill`/`Document`. Routes on
 * `quill.backendId`, lazily loads that backend build, clones the quill and
 * document into the backend's WASM memory on demand, renders, and frees the
 * clones. The cross-memory crossing is invisible to callers.
 *
 * A `quill.backendId` outside the registry rejects with
 * `engine::backend_not_found`, the capability probes included. The diagnostic's
 * `hint` names the registered ids.
 */
export declare class Engine {
	constructor(options?: EngineOptions);

	/**
	 * Render `doc` against `quill` in one shot. Both handles are read
	 * synchronously before the first await, so the caller may `free()` them as
	 * soon as this call returns.
	 *
	 * This is the surface that merges the two warning halves:
	 * {@link RenderResult.warnings} carries `doc.warnings` (parse, `conform::*`,
	 * `plate::unsupported_construct`) ahead of the compile's own. A
	 * {@link LiveSession} outlives the document it opened from, so
	 * {@link LiveSession.render} carries the compile half alone.
	 */
	render(quill: Quill, doc: Document, options?: RenderOptions): Promise<RenderResult>;

	/**
	 * Open a live render session (canvas preview / per-page paint / `update`).
	 * The `quill` and `doc` handles are read synchronously before the first
	 * await, so the caller may `free()` them as soon as this call returns; the
	 * caller owns the returned session and must `.free()` it.
	 */
	open(quill: Quill, doc: Document): Promise<LiveSession>;

	/**
	 * Output formats `quill`'s backend can emit. An always-free pre-render probe:
	 * it answers from the descriptor's `formats` manifest without loading the
	 * backend binary or cloning the quill. Async for API stability.
	 */
	supportedFormats(quill: Quill): Promise<OutputFormat[]>;
}

/**
 * Iterative render session over a compiled snapshot. `free()` when done.
 *
 * CANVAS PAINT IS COMPLETE: {@link LiveSession.paint} writes a whole page
 * raster, every piece of page content already visible in the painted pixels,
 * with no compositing required by the caller — pdfform pre-flattens bound field
 * values into the page content to satisfy this. {@link LiveSession.regions}
 * carries schema-field geometry for overlays drawn on top; it is never needed to
 * complete the picture.
 *
 * A backend with no canvas painter, and a compile with no pages, throw from
 * {@link LiveSession.pageSize} and {@link LiveSession.paint} naming the resolved
 * `backendId`. That throw is how a consumer learns; there is no capability flag.
 */
export declare class LiveSession {
	private constructor();
	readonly pageCount: number;
	readonly backendId: string;
	readonly warnings: Diagnostic[];
	/**
	 * Recompile the session against `doc`: the edit verb of a live preview.
	 * Transactional — on throw every read keeps serving the last-good compile and
	 * the session recovers on the next successful `update`. On success, repaint
	 * `dirtyPages ∩ visible`.
	 */
	update(doc: Document): ChangeSet;
	render(options?: RenderOptions): RenderResult;
	/**
	 * Schema-field geometry for this compiled session, keyed on the canonical
	 * `DocPath` address (`parseDocPath`-routable). A session-level query: no
	 * render, no byte artifact. Read it to scroll to / highlight the focused
	 * field over a `paint`-ed canvas; the click direction is {@link fieldAt}.
	 * Empty for backends that place no schema fields.
	 *
	 * `field` is **not** unique: a content field surfaces its **first placement**
	 * as one {@link FieldRegion} per page that placement touches, a scalar
	 * referenced at several plate sites surfaces each site, and tracked content
	 * plus a `field:`-bound widget yields both, widget first. Group by `field`.
	 * Later placements of one content value are not enumerated; {@link fieldAt}
	 * still resolves clicks on them.
	 */
	regions(): FieldRegion[];
	/**
	 * The whole-field highlight boxes for `field` (a canonical `DocPath` address,
	 * as {@link regions} keys): one union rect per page over the field's
	 * `span`-bearing content segments, the union {@link regions} leaves derived.
	 * **Content only**: a field placed solely as a scalar reference or a bound
	 * widget carries no `span` and returns `[]`, its box being a single
	 * {@link regions} rect. Reflects the current compile.
	 */
	fieldBoxes(field: string): FieldRegion[];
	/**
	 * The schema field whose content is under a point on `page`: the canonical
	 * `DocPath` address to focus in the editor, or `undefined` off any field's
	 * ink. `x`/`y` are PDF points with a **bottom-left** origin, the same space as
	 * {@link FieldRegion.rect}, so from a canvas click invert the overlay
	 * transform documented there: `x = clickPx.x / renderScale`,
	 * `y = pageHeightPt - clickPx.y / renderScale`. Unlike {@link regions},
	 * *every* placement answers, not just the first.
	 *
	 * `tolPt` is how far off the ink a click still counts, in the same points,
	 * and defaults to `0` — exact. It is pointer slack, so derive it from the
	 * scale the page was drawn at (`slackPx / renderScale`) rather than fixing a
	 * value in points, which shrinks under the cursor as the page zooms out. The
	 * nearest placement answers and containment is distance zero, so raising
	 * `tolPt` only ever fills a miss.
	 */
	fieldAt(page: number, x: number, y: number, tolPt?: number): string | undefined;
	/**
	 * Fine-grained click → content position (caret placement). Same PDF-point
	 * space as {@link fieldAt}; `undefined` past `tolPt` from all content ink.
	 *
	 * `tolPt` buys the most here: the leading between two lines lies inside a
	 * paragraph and on no glyph, and under `tolPt` a point there takes the line
	 * it is nearer.
	 */
	positionAt(page: number, x: number, y: number, tolPt?: number): ContentHit | undefined;
	/**
	 * Content position → caret rect: reverse of {@link positionAt}. `field` is a
	 * canonical `DocPath` address (`parseDocPath`-routable), as {@link regions} keys.
	 */
	locate(field: string, pos: number): FieldRegion | undefined;
	/** Page geometry in points (1/72″). Report-only; the painter sizes the canvas. */
	pageSize(page: number): PageSize;
	/**
	 * Paint `page` into a 2D canvas context, sizing the backing store itself (it
	 * owns `canvas.width`/`height`; the caller owns `canvas.style.*`). The
	 * rasterization scale is `layoutScale × densityScale`, clamped so neither
	 * backing dimension exceeds 16384 px; {@link PaintResult.clamped} reports the
	 * clamp and {@link PaintResult.effectiveDensityScale} the density applied.
	 *
	 * The write is a whole-backing-store `putImageData`, which bypasses the 2D
	 * context transform, `globalAlpha`, and clip, so give each visible page its
	 * own canvas: no compositing, sub-rect, or transform reaches through this
	 * call, and the raster is complete precisely so none is needed. Keep the
	 * per-page canvases alive while their pages stay near the viewport: each
	 * `paint` re-rasterizes from scratch, whereas an idle canvas retains its
	 * pixels for free.
	 */
	paint(
		ctx: CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D,
		page: number,
		options?: PaintOptions
	): PaintResult;
	free(): void;
}

// ── Typed writer: the schema-bound front door ───────────────────────────────

// `quill.writer(doc)` is patched onto the re-exported `Quill` prototype (the
// class is re-exported verbatim, so the method is declared by merging into the
// core module's `Quill` rather than redeclaring the class).
declare module '../core/wasm.js' {
	interface Quill {
		/**
		 * Bind this quill's schema to `doc` for typed writes. The returned writer
		 * holds both handles by reference and owns neither (nothing to `free()`);
		 * it is ephemeral by convention: bind, write, discard.
		 */
		writer(doc: Document): DocumentWriter;
		/**
		 * Bind this quill's schema to `doc` for schema-bound reads: the read twin of
		 * {@link Quill.writer}, mirroring core's `quill.reader(&doc)`. Each field is
		 * read in the values form (every content leaf as its codec's text, every
		 * other value as stored) with schema authority, so a name the schema does
		 * not declare throws rather than reading back `undefined`; the whole
		 * document reads through `values()`, and the render view through
		 * `resolve()`. Holds both handles by reference and owns neither (nothing
		 * to `free()`); ephemeral by convention: bind, read, discard.
		 */
		reader(doc: Document): DocumentReader;
	}
}

/**
 * A `Document` bound to its `Quill` for typed writes, from {@link Quill.writer}.
 * Speaks names, values, and markdown, so bare `set` / `setAll` / `reviseBody` /
 * `reviseField` / `addCard` / `card(i).set` replace threading the `quill` handle
 * through the underscored ABI. Holds both handles by reference and owns neither.
 *
 * Typed commit is the default whenever a quill is in hand: it resolves each
 * field's schema type and strict-commits it, throwing `UnknownField` for an
 * undeclared name rather than falling back. The raw `Document.storeField` /
 * `storeFields` verbs remain the deliberate quill-free primitive (standalone
 * data, storage/migration infra, or not-yet-conforming in-progress input).
 */
export declare class DocumentWriter {
	constructor(quill: Quill, doc: Document);
	/** The bound document: the instance passed in, mutated in place. */
	readonly document: Document;
	/**
	 * Typed-commit one main-card field (strict coerce, mismatch throws now).
	 * Throws `UnknownField` for a name the schema does not declare.
	 */
	set(name: string, value: unknown): void;
	/**
	 * Typed-commit several main-card fields atomically: nothing is applied on
	 * error (throws a {@link QuillmarkError} carrying one diagnostic per
	 * offending field, including an `UnknownField` for each undeclared name).
	 */
	setAll(fields: Record<string, unknown>): void;
	/**
	 * Revise the main body from markdown; anchors rebase. Returns the text
	 * `Delta`: a body carries no field schema to type against, so this is the
	 * content lane's `revise` reached through the writer.
	 */
	reviseBody(markdown: string): Delta;
	/**
	 * Revise the content main-card field `name` from authored text: typed *and*
	 * anchor-preserving. Surviving anchors rebase, then the diffed result is
	 * schema-conformed (`richtext(inline)` rejects a multi-block result). Throws
	 * `UnknownField` for a name the schema does not declare. Returns the `Delta`.
	 *
	 * The codec comes from the declared type: `richtext` diffs markdown, while
	 * `plaintext` diffs the literal text and never imports markdown, so a
	 * byte-identical revise of a value carrying escapes is a byte no-op.
	 */
	reviseField(name: string, text: string): Delta;
	/**
	 * Build a composable card of `kind`, typed-commit `fields` onto it, set its
	 * body from optional markdown, and place it. `at` omitted appends, a number
	 * inserts at that index. Transactional: a rejected field (throwing a per-field
	 * diagnostic bundle) or an invalid kind, body, or position leaves the document
	 * untouched.
	 */
	addCard(kind: string, fields?: Record<string, unknown>, body?: string, at?: number): void;
	/**
	 * Write the document in the values form: the write twin of
	 * {@link DocumentReader.values}. An absent axis is untouched; a present one
	 * is replaced: `fields` is the whole truth for declared names (an unnamed
	 * one is removed; an undeclared one the card holds is accepted unchanged and
	 * refused changed), `cards` is the card list, `body` the body, `ext: null`
	 * removes `$ext` and `{}` records an explicit empty one. All-or-nothing:
	 * nothing is applied on error and every refused cell is one diagnostic
	 * carrying its own `path`.
	 *
	 * A cell whose value equals its projection is not written, so writing back
	 * an unedited read changes no bytes. A changed content cell is a cold
	 * import — {@link DocumentWriter.reviseField} per cell is what keeps its
	 * anchors — and cards match by position and kind, so deleting or reordering
	 * an entry rewrites every card after it. An `undefined` member reads as
	 * absent.
	 */
	setValues(values: DocumentValuesInput): void;
	/** Remove the composable card at `index`, returning it (or `undefined`). */
	removeCard(index: number): Card | undefined;
	/**
	 * A {@link CardWriter} for the composable card at `index`. Index validity is
	 * checked lazily at commit time, so an out-of-range index does not throw here.
	 * The cursor is ephemeral: a `removeCard`/`addCard` between binding and writing
	 * silently retargets it; re-resolve the index at write time when cards may
	 * move.
	 */
	card(index: number): CardWriter;
}

/**
 * A composable card bound to its `Quill` for typed writes, from
 * {@link DocumentWriter.card}. Same verbs as {@link DocumentWriter}, targeting
 * the card at its bound index; each write throws `IndexOutOfRange` if that index
 * is out of range.
 */
export declare class CardWriter {
	constructor(quill: Quill, doc: Document, index: number);
	/** The bound card index. */
	readonly index: number;
	/**
	 * The bound card's `$kind`, empty string when it carries none. Throws
	 * `IndexOutOfRange` for a bad bound index.
	 */
	readonly kind: string;
	set(name: string, value: unknown): void;
	setAll(fields: Record<string, unknown>): void;
	/** Revise this card's body from markdown (edit semantics), returning the text `Delta`. */
	reviseBody(markdown: string): Delta;
	/**
	 * The card twin of {@link DocumentWriter.reviseField}. Throws `UnknownField`
	 * for an undeclared name and `IndexOutOfRange` for a bad bound index.
	 */
	reviseField(name: string, text: string): Delta;
	/**
	 * Write this card in the values form: {@link DocumentWriter.setValues}
	 * restricted to one slot, under the same per-axis rule. An absent `kind`
	 * keeps the card's; a differing one rebuilds the slot. Refusals anchor at
	 * `cards.<kind>[index]`; throws `IndexOutOfRange` for a bad bound index.
	 */
	setValues(values: CardValuesInput): void;
}

/**
 * A `Document` bound to its `Quill` for interpreted reads, from
 * {@link Quill.reader}: the read twin of {@link DocumentWriter}. One `get` reads
 * each field by its declared type — a richtext field to its markdown projection,
 * a plaintext field to its literal text, every other type verbatim.
 *
 * The schema authority is the point: unlike the quill-free `Document.getStored`,
 * an undeclared name throws `UnknownField` rather than reading back `undefined`,
 * and an undecodable content value throws `FieldDecode`. A field's markdown lives
 * here, not on the body-only `Document.bodyMarkdown`; the body read stays
 * quill-free and never throws.
 *
 * `getContent` is the same read at the other end of the codec, returning the
 * `Content` rather than the projection. It binds the quill for the same reason
 * `get` does: the same stored bytes decode two ways, and only the declared type
 * says which. `getContentAt` is that read one axis further in, for a `Content`
 * nested inside a composite field.
 */
export declare class DocumentReader {
	constructor(quill: Quill, doc: Document);
	/** The bound document: the instance passed in. */
	readonly document: Document;
	/**
	 * Read the value at `addr`, interpreted by its declared type: a richtext field
	 * to markdown, every other type verbatim. A bare string is `Addr` shorthand for
	 * `{ field }`; an absent `addr.field` reads the body markdown. `undefined` for
	 * an absent field; throws `UnknownField` for a name the schema does not declare,
	 * `FieldDecode` for a richtext field holding an undecodable value, and
	 * `IndexOutOfRange` for a bad `addr.card`.
	 */
	get(addr: Addr | string): unknown;
	/**
	 * Read the content field at `addr` as canonical `Content`: the twin of
	 * {@link get}, which projects. Decodes through the codec the declared type
	 * names, so a committed field and a parsed one read back the same `Content`.
	 * An absent `addr.field` reads the body `Content`. `undefined` for an absent
	 * field; throws `UnknownField`, `FieldNotContent` for a declared type that is
	 * not a content leaf, `FieldDecode` for an undecodable value, and
	 * `IndexOutOfRange`.
	 */
	getContent(addr: Addr | string): Content | undefined;
	/**
	 * Read the `Content` nested inside the composite field at `addr`, at `path`:
	 * `[0]` an element of an `array<richtext>`, `["motto"]` an object's content
	 * property, `[1, "notes"]` a leaf under both, `["controlled_by"]` a variant's
	 * cell. The codec is the leaf's declared type's, resolved through the field
	 * schema's `items` / `properties` / `variants`, so the element's storage
	 * form is not the caller's business. The empty path is {@link getContent}.
	 *
	 * `undefined` for an absent field and for a path that names nothing in the
	 * stored value: a repeater's row index goes stale between derive and read,
	 * so absence there is a read, not a fault. Throws `UnknownField` for an
	 * undeclared name at any depth, `FieldNotContent` when `path` resolves to no
	 * content leaf, `FieldDecode` anchored at the addressed path, and
	 * `IndexOutOfRange` for a bad `addr.card`.
	 */
	getContentAt(addr: Addr | string, path: PathStep[]): Content | undefined;
	/** The main body's markdown: the quill-free body read. Equals `get({})`. */
	bodyMarkdown(): string;
	/**
	 * The whole document in the values form: the main card's fields, body and
	 * `$ext`, and every composable card, every content leaf as its codec's text,
	 * everything else as stored. Every axis is present, so the result is a valid
	 * {@link DocumentWriter.setValues} input and writing it back unedited changes
	 * no bytes. Never throws: a content leaf that decodes under neither encoding
	 * rides out as stored where {@link get} would throw.
	 */
	values(): DocumentValues;
	/**
	 * The resolved-value view: for every declared field, the value the render
	 * projection would use and the rung it came from (`authored` / `default` /
	 * `blank`). The one read that blank-fills and coerces; {@link values}
	 * reports what the document carries. Value and provenance only;
	 * completeness stays `quill.validate`'s.
	 */
	resolve(): Resolved;
	/**
	 * A {@link CardReader} for the composable card at `index`. Index validity is
	 * checked lazily at read time, so an out-of-range index does not throw here.
	 * The cursor is ephemeral: a `removeCard`/`addCard` between binding and reading
	 * silently retargets it.
	 */
	card(index: number): CardReader;
}

/**
 * A composable card bound to its `Quill` for interpreted reads, from
 * {@link DocumentReader.card}. Same verbs as {@link DocumentReader}, reading the card
 * at its bound index; each read throws `IndexOutOfRange` if that index is out of
 * range.
 */
export declare class CardReader {
	constructor(quill: Quill, doc: Document, index: number);
	/** The bound card index. */
	readonly index: number;
	/**
	 * The bound card's `$kind` (empty string when it carries none). Throws
	 * `IndexOutOfRange` if the bound index is out of range.
	 */
	readonly kind: string;
	/**
	 * Read the field `name` on this card, interpreted by its declared type.
	 * `undefined` when absent; throws `UnknownField` for an undeclared name and
	 * `IndexOutOfRange` for a bad index.
	 */
	get(name: string): unknown;
	/**
	 * The card twin of {@link DocumentReader.getContent}.
	 */
	getContent(name: string): Content | undefined;
	/** The card twin of {@link DocumentReader.getContentAt}. */
	getContentAt(name: string, path: PathStep[]): Content | undefined;
	/** This card's body markdown: the card twin of {@link DocumentReader.bodyMarkdown}. */
	bodyMarkdown(): string;
	/**
	 * This card in the values form: {@link DocumentReader.values} restricted to
	 * one slot. Throws `IndexOutOfRange` for a bad bound index.
	 */
	values(): CardValues;
}
