// Type-level DRIFT GUARD: `runtime/runtime.d.ts` declares the render-side types
// as the backend-neutral contract rather than re-exporting the private Typst
// build's, so this file asserts the two stay mutually assignable. If either
// drifts, one of the assignments below stops compiling.
//
// Run via `npm run typecheck`. Emits no runtime code.

import type {
	RenderResult as CanonicalRenderResult,
	RenderOptions as CanonicalRenderOptions,
	Artifact as CanonicalArtifact,
	OutputFormat as CanonicalOutputFormat,
	PageSize as CanonicalPageSize,
	PaintOptions as CanonicalPaintOptions,
	PaintResult as CanonicalPaintResult,
	FieldRegion as CanonicalFieldRegion,
	ChangeSet as CanonicalChangeSet,
	ContentHit as CanonicalContentHit
  // The BUILT copy (synced from `runtime/runtime.d.ts` by build-wasm.sh / the
  // cp step), because only there does the d.ts's own `../core/wasm.js` import
  // resolve to the generated `pkg/core` build. The two copies are byte-identical.
} from '../../../pkg/runtime/runtime.d.ts';

import type {
	RenderResult as TypstRenderResult,
	RenderOptions as TypstRenderOptions,
	Artifact as TypstArtifact,
	OutputFormat as TypstOutputFormat,
	PageSize as TypstPageSize,
	PaintOptions as TypstPaintOptions,
	PaintResult as TypstPaintResult,
	FieldRegion as TypstFieldRegion,
	ChangeSet as TypstChangeSet,
	ContentHit as TypstContentHit
} from '../../../pkg/render/wasm';

// One mutual-assignability pair per hoisted type: typst → canonical and
// canonical → typst. `void` the bindings so "declared but never read" is not an
// error under noUnusedLocals.
//
// Mutual assignability alone cannot catch a missing OPTIONAL member: for an
// all-optional interface pair (RenderOptions, PaintOptions) both assignments
// compile even when one side lacks a member entirely. The `KeysEqual`
// assertions close that hole: `true` only when both sides declare exactly
// the same property names.

type KeysEqual<A, B> = [Exclude<keyof A, keyof B>, Exclude<keyof B, keyof A>] extends [
	never,
	never
]
	? true
	: false;

const renderResultA: CanonicalRenderResult = {} as TypstRenderResult;
const renderResultB: TypstRenderResult = {} as CanonicalRenderResult;
void renderResultA;
void renderResultB;

const renderOptionsA: CanonicalRenderOptions = {} as TypstRenderOptions;
const renderOptionsB: TypstRenderOptions = {} as CanonicalRenderOptions;
void renderOptionsA;
void renderOptionsB;

const artifactA: CanonicalArtifact = {} as TypstArtifact;
const artifactB: TypstArtifact = {} as CanonicalArtifact;
void artifactA;
void artifactB;

const outputFormatA: CanonicalOutputFormat = {} as TypstOutputFormat;
const outputFormatB: TypstOutputFormat = {} as CanonicalOutputFormat;
void outputFormatA;
void outputFormatB;

const pageSizeA: CanonicalPageSize = {} as TypstPageSize;
const pageSizeB: TypstPageSize = {} as CanonicalPageSize;
void pageSizeA;
void pageSizeB;

const paintOptionsA: CanonicalPaintOptions = {} as TypstPaintOptions;
const paintOptionsB: TypstPaintOptions = {} as CanonicalPaintOptions;
void paintOptionsA;
void paintOptionsB;

const paintResultA: CanonicalPaintResult = {} as TypstPaintResult;
const paintResultB: TypstPaintResult = {} as CanonicalPaintResult;
void paintResultA;
void paintResultB;

const fieldRegionA: CanonicalFieldRegion = {} as TypstFieldRegion;
const fieldRegionB: TypstFieldRegion = {} as CanonicalFieldRegion;
void fieldRegionA;
void fieldRegionB;

const changeSetA: CanonicalChangeSet = {} as TypstChangeSet;
const changeSetB: TypstChangeSet = {} as CanonicalChangeSet;
void changeSetA;
void changeSetB;

const contentHitA: CanonicalContentHit = {} as TypstContentHit;
const contentHitB: TypstContentHit = {} as CanonicalContentHit;
void contentHitA;
void contentHitB;

const renderResultKeys: KeysEqual<CanonicalRenderResult, TypstRenderResult> = true;
const renderOptionsKeys: KeysEqual<CanonicalRenderOptions, TypstRenderOptions> = true;
const artifactKeys: KeysEqual<CanonicalArtifact, TypstArtifact> = true;
const pageSizeKeys: KeysEqual<CanonicalPageSize, TypstPageSize> = true;
const paintOptionsKeys: KeysEqual<CanonicalPaintOptions, TypstPaintOptions> = true;
const paintResultKeys: KeysEqual<CanonicalPaintResult, TypstPaintResult> = true;
const fieldRegionKeys: KeysEqual<CanonicalFieldRegion, TypstFieldRegion> = true;
const changeSetKeys: KeysEqual<CanonicalChangeSet, TypstChangeSet> = true;
const contentHitKeys: KeysEqual<CanonicalContentHit, TypstContentHit> = true;
void renderResultKeys;
void renderOptionsKeys;
void artifactKeys;
void pageSizeKeys;
void paintOptionsKeys;
void paintResultKeys;
void fieldRegionKeys;
void changeSetKeys;
void contentHitKeys;

// ── Re-export presence guard ────────────────────────────────────────
// The content edit vocabulary is DECLARED in the core build but consumed through
// the single runtime entry point. Importing every name from the runtime root
// here asserts the re-export in `runtime/runtime.d.ts` stays present: drop any
// one and this import stops resolving, failing `npm run typecheck`. Type-only:
// no runtime code, no assignability claim, pure existence.
import type {
	Content,
	ContentLine,
	ContentLineKind,
	ContentContainer,
	ContentMark,
	ContentMarkKind,
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
	LineOp,
	MarkOp,
	ChangeBundle
} from '../../../pkg/runtime/runtime.d.ts';

// Referencing each name in an exported tuple keeps the import "used" without a
// runtime statement; an exported alias is never an unused-local error.
export type ContentExportsPresent = [
	Content,
	ContentLine,
	ContentLineKind,
	ContentContainer,
	ContentMark,
	ContentMarkKind,
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
	LineOp,
	MarkOp,
	ChangeBundle
];

// ── MAIN_CARD_ADDR is a CardAddr ────────────────────────────────────
// The named main-card address must type as a `CardAddr` so it flows into every
// card-scoped verb's address slot. `typeof import(...)` keeps this purely
// type-level (no value import, no runtime code) and the assignment fails
// `npm run typecheck` if the constant's declared type ever drifts off `CardAddr`.
type MainCardAddrType = typeof import('../../../pkg/runtime/runtime.d.ts').MAIN_CARD_ADDR;
const mainCardAddrIsCardAddr: CardAddr = {} as MainCardAddrType;
void mainCardAddrIsCardAddr;

// The narrowing assertions below take these three.
declare const guardIsland: ContentIsland;
declare const guardMark: ContentMark;
declare const guardLine: ContentLine;

// ── The vocabularies are closed ─────────────────────────────────────
// A name outside a union is a type error, which is the compile-time half of the
// decoder's refusal. Each `@ts-expect-error` fails `npm run typecheck` if its
// union ever regains a residual `string` arm.

// @ts-expect-error 'callout' is not a line kind
const closedLine: ContentLineKind = { kind: 'callout' };
void closedLine;
// @ts-expect-error 'indent' is not a container
const closedContainer: ContentContainer = { container: 'indent', instance: 0 };
void closedContainer;
// @ts-expect-error 'highlight' is not a mark type
const closedMark: ContentMark = { start: 0, end: 1, type: 'highlight' };
void closedMark;
// @ts-expect-error 'widget' is not an island type
const closedIsland: ContentIsland = { id: 'i', loss: 'lossless', type: 'widget', props: {} };
void closedIsland;
// @ts-expect-error 'partial' is not a loss class
const closedLoss: ContentIsland['loss'] = 'partial';
void closedLoss;

// A bare discriminant check narrows the payload, with no guard.
if (guardLine.kind === 'heading') {
	const level: number = guardLine.attrs.level;
	void level;
}
if (guardMark.type === 'link') {
	const url: string = guardMark.attrs.url;
	void url;
}
if (guardIsland.type === 'table') {
	const props: TableProps = guardIsland.props;
	void props;
}

// ── ContentLineKind is nameable ─────────────────────────────────────
// `ContentLineKind` is exactly `setKind`'s payload, so building the op is a
// whole-lift: drop a line's envelope, spread the rest. That spelling survives
// every arm added upstream. It only type-checks if the type is nameable from
// the package entry point, which is the point of the re-export.
function kindPart(line: ContentLine): ContentLineKind {
	const { containers, continues, ...kind } = line;
	void containers;
	void continues;
	return kind;
}
declare const liftLine: ContentLine;
const liftedOp: LineOp = { op: 'setKind', line: 0, ...kindPart(liftLine) };
void liftedOp;

// ── ContentMarkKind is nameable ─────────────────────────────────────
// The same lift on the mark axis: `ContentMarkKind` is exactly an `add` /
// `remove`'s payload, so an arm added upstream reaches the op through the name.
// That is what makes the lift the guard — a `MarkOp` re-spelling the payload by
// hand stops accepting it here.
function markPart(mark: ContentMark): ContentMarkKind {
	const { start, end, ...kind } = mark;
	void start;
	void end;
	return kind;
}
declare const liftMark: ContentMark;
const liftedMark: MarkOp = { op: 'add', start: 0, end: 1, ...markPart(liftMark) };
void liftedMark;
// The op's envelope is the mark's own, so a held mark spreads in whole.
const liftedWhole: MarkOp = { op: 'remove', ...liftMark };
void liftedWhole;

// The payload rides `attrs` on the op as on the mark. The named sibling is the
// retired `@0.93.0` spelling, which the authored lane refuses as `legacy mark
// payload`, so the type that admits it is the one that cannot be written.
// @ts-expect-error a link's url rides `attrs`
const siblingUrl: MarkOp = { op: 'add', start: 0, end: 1, type: 'link', url: 'https://x' };
void siblingUrl;
// @ts-expect-error an anchor's id rides `attrs`
const siblingId: MarkOp = { op: 'add', start: 0, end: 0, type: 'anchor', id: 'a1' };
void siblingId;

// ── The gate is the only door ───────────────────────────────────────
// The guarantee `init` exists to hold: a value needing the WASM instance is
// reachable through the gate and nowhere else. A static export of one would
// type-check at every call site that forgot to await, and a floating promise is
// an ESLint rule rather than a `tsc` diagnostic, so a signature alone is no
// guard. The level the guarantee lives on is this one, so it is asserted here.
//
// Both assertions derive their names from `CoreSurface`, so a member added
// there is covered without touching this file.

import type * as runtimeModule from '../../../pkg/runtime/runtime.d.ts';
import type {
	CoreSurface,
	init as initFn,
	Quill as RuntimeQuill,
	Document as RuntimeDocument
} from '../../../pkg/runtime/runtime.d.ts';

// No gated name is among the module's VALUE exports. `Extract` collapses to
// `never` only while every one of them is absent.
type StaticallyReachable = Extract<keyof CoreSurface, keyof typeof runtimeModule>;
const gateIsTheOnlyDoor: StaticallyReachable extends never ? true : false = true;
void gateIsTheOnlyDoor;

// And the gate resolves to exactly that surface, in both directions.
const gateResolvesToSurface: CoreSurface = {} as Awaited<ReturnType<typeof initFn>>;
const surfaceIsWhatTheGateGives: Awaited<ReturnType<typeof initFn>> = {} as CoreSurface;
void gateResolvesToSurface;
void surfaceIsWhatTheGateGives;

// The classes arrive with their statics, so the whole construction surface is
// reachable off the destructure.
declare const gated: CoreSurface;
const fromTree: RuntimeQuill = gated.Quill.fromTree(new Map<string, Uint8Array>());
const fromMarkdown: RuntimeDocument = gated.Document.fromMarkdown('# Hi');
void fromTree;
void fromMarkdown;

// The instance types stay exported as TYPES, so an annotation needs no await:
// only obtaining a value does.
declare const annotated: RuntimeQuill;
void annotated;

// The container write lane. `instance` decides whether two adjacent runs weld,
// and nothing reports an omission at runtime. The seam spells it on every read,
// so the one container type requires it: the op lane and the whole-`Content`
// lane a document-shaped codec writes through both report an omission.
import type { assignInstances } from '../../../pkg/runtime/runtime.d.ts';

// @ts-expect-error the field is the whole point of the type.
const unstamped: ContentContainer = { container: 'quote' };
void unstamped;

const looseOp: Extract<LineOp, { op: 'setContainers' }> = {
	op: 'setContainers',
	line: 0,
	// @ts-expect-error the op carries written containers, which spell `instance` out.
	containers: [{ container: 'quote' }]
};
void looseOp;

// `overwrite` and `CardInput.body` take a whole `Content`, the lane a codec
// flattening a tree most likely writes through. Requiring the field on the read
// shape is what reaches it.
const looseBody: CardInput = {
	kind: 'note',
	body: {
		text: 'a',
		lines: [
			{
				kind: 'para',
				// @ts-expect-error a hand-built container spells `instance` out.
				containers: [{ container: 'quote' }]
			}
		],
		marks: [],
		islands: []
	}
};
void looseBody;

const stampedBody: CardInput = {
	kind: 'note',
	body: {
		text: 'a',
		lines: [{ kind: 'para', containers: [{ container: 'quote', instance: 0 }] }],
		marks: [],
		islands: []
	}
};
void stampedBody;

// What `assignInstances` returns is what the op takes, nulls dropped.
const stampedFeedsTheOp: Extract<LineOp, { op: 'setContainers' }>['containers'] = [] as NonNullable<
	ReturnType<typeof assignInstances>[number]
>[];
void stampedFeedsTheOp;
