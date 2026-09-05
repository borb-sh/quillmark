use crate::error::WasmError;
use crate::types::Diagnostic;
#[cfg(any(feature = "typst", feature = "pdfform"))]
use crate::types::{ChangeSet, ContentHit, FieldRegion, RenderOptions, RenderResult};
use js_sys::{Array, Uint8Array};
#[cfg(any(feature = "typst", feature = "pdfform"))]
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const METADATA_TS: &'static str = r#"
/** UI layout hints for a single field. Display order is not a hint: key order
 * in the schema's `fields`/`properties` objects is the ordering contract. */
export interface QuillFieldUi {
    title?: string;
    group?: string;
    compact?: boolean;
    multiline?: boolean;
    /** Label for an `enum`'s blank option. Absent, the consumer supplies a
     *  conventional label of its own. */
    blank_title?: string;
}

/** One entry in a card's `ui.groups` registry: a display-label override for the
 * group id (the map key). An empty object carries no override, and the consumer
 * derives the label from the id (`memo_for` → "Memo For"). */
export interface QuillGroupUi {
    title?: string;
}

/** UI layout hints for a card (main or named card kind). */
export interface QuillCardUi {
    title?: string;
    /** The groups a field's `ui.group` may reference, keyed by group id. Key
     * order is the display-order contract, as with `fields`. Absent when the
     * card declares no groups. */
    groups?: Record<string, QuillGroupUi>;
}

/** A block construct a body can hold. `paragraph` is the floor and cannot be
 * declined, so it is absent. */
export type QuillBlockConstruct =
    | "heading"
    | "rule"
    | "code"
    | "list"
    | "quote"
    | "table"
    | "image";

/** Body namespace for a card (main or named card kind). */
export interface QuillCardBody {
    /** When false, consumers must not accept or store body content for this card kind. Defaults to true. */
    enabled?: boolean;
    /** Example body content embedded verbatim in the blueprint body region. Fallback is "Write <card> body here." */
    example?: string;
    /** Block constructs this quill's plate does not typeset in this body;
     * absent or empty declines nothing. A body that holds one anyway draws a
     * non-fatal `plate::unsupported_construct` warning. Nothing verifies the
     * claim: absence from this list is not a promise the plate typesets it. */
    unsupported?: QuillBlockConstruct[];
}

/** Schema entry for a single field declared in a quill's `Quill.yaml`.
 *
 * One declaration, and no `required` key. `default` and `example` say what the
 * cell holds, and `default`'s absence is the obligation: a field nobody
 * declared a value for carries a `!must_fill` marker in the blueprint and warns
 * `validation::must_fill` while the document leaves it unauthored. Neither
 * gates render: an absent field blank-fills.
 */
export interface QuillFieldSchema {
    type: "string" | "number" | "integer" | "boolean" | "array" | "object" | "date" | "datetime" | "richtext" | "plaintext" | "enum";
    description?: string;
    default?: unknown;
    example?: unknown;
    /** The closed set of allowed values. Required on `type: "enum"`, and valid
     *  nowhere else. */
    values?: string[];
    /** Per-member field sets on a card-level `type: "enum"` field, keyed by
     *  member: the fields that exist only where the discriminant holds that
     *  member. Declaring it makes the field rest as a container,
     *  `{value: <member>, …that member's fields}`, rather than a bare string. */
    variants?: Record<string, Record<string, QuillFieldSchema>>;
    ui?: QuillFieldUi;
    properties?: Record<string, QuillFieldSchema>;
    items?: QuillFieldSchema;
    /** `true` on a `richtext` or `plaintext` field declared `inline`: the
     *  single-paragraph, container-free, island-free constraint. */
    inline?: boolean;
}

/** Schema entry for the main card or a named card kind. */
export interface QuillCardSchema {
    description?: string;
    fields: Record<string, QuillFieldSchema>;
    ui?: QuillCardUi;
    body?: QuillCardBody;
}

/**
 * Document schema returned by `Quill.schema`: the user-fillable fields only.
 * The quill reference (`${metadata.name}@${metadata.version}`) and card-kind
 * discriminators are document-level metadata, not schema fields.
 */
export interface QuillSchema {
    main: QuillCardSchema;
    /** Present only when the quill declares at least one named card kind. */
    card_kinds?: Record<string, QuillCardSchema>;
}

/**
 * Identity snapshot mirroring the `quill:` section of `Quill.yaml`. The schema
 * lives on `Quill.schema`; output formats are a resolved-backend capability read
 * from `Quillmark.supportedFormats`, not part of this config snapshot.
 */
export interface QuillMetadata {
    name: string;
    version: string;
    backend: string;
    author: string;
    description: string;
}
"#;

/// Mirrors `quillmark_core::CardWire`. `CardInput` is referenced by name via
/// `unchecked_param_type`.
#[wasm_bindgen(typescript_custom_section)]
const CARD_TS: &'static str = r#"
/**
 * A path to a value nested inside a field `value`: `string` keys and
 * `number` array indices, e.g. `["addr", "street"]` or `["recipients", 0, "name"]`.
 */
export type PathStep = string | number;

/** A field or comment entry in a `Card.payloadItems` list. */
export type PayloadItem =
    | {
          type: "field";
          key: string;
          value: unknown;
          fill?: boolean;
          /**
           * Paths to `!must_fill` markers nested *inside* `value` (the `value`
           * projection itself is fill-free). Absent when the field has no nested
           * placeholders. Preserved across `insertCard` / `makeCard`.
           */
          nestedFills?: PathStep[][];
      }
    | { type: "comment"; text: string; inline?: boolean };

/**
 * A single card block, as read back from a document. Every `Card` is a valid
 * `CardInput`, so a card read from one document pushes straight into another.
 *
 * `$` system entries are hoisted to named fields: `kind` (the `$kind`, empty
 * string when none), `quill` (`$quill` `name@version`, main card only), `ext`
 * (`$ext`), and `seed` (the `$seed` per-kind overlay map, main card only).
 * `payloadItems` carries user fields and comments in order.
 */
export interface Card {
    kind: string;
    quill?: string;
    ext?: Record<string, unknown>;
    seed?: Record<string, unknown>;
    payloadItems: PayloadItem[];
    /**
     * The card body as canonical `Content`, never a markdown string. For the
     * markdown projection call `exportMarkdown(card.body)`.
     */
    body: Content;
}

/**
 * A card written *into* a document, accepted by `Document.insertCard`. Like
 * `Card`, but `body` also takes a markdown `string`, and every field but `kind`
 * is optional (defaulting to no payload items and an empty body).
 */
export interface CardInput {
    kind: string;
    quill?: string;
    ext?: Record<string, unknown>;
    seed?: Record<string, unknown>;
    payloadItems?: PayloadItem[];
    body?: Content | string;
}

/**
 * Canonical richtext content: the model behind a card body and richtext fields.
 * One text sequence over a single coordinate space (Unicode scalar values):
 * `text` plus line attributes, anchored `marks`, and embedded `islands`. Every
 * edit is a splice; markdown is a projection, not the model.
 */
export interface Content {
    text: string;
    lines: ContentLine[];
    marks: ContentMark[];
    islands: ContentIsland[];
}

/** One `\n`-separated segment of `Content.text`, in order. `kind` is an open set:
 * an unknown role round-trips with opaque `attrs` and renders as a paragraph.
 * Every role spells its payload in `attrs`, known or not, so promoting one moves
 * no bytes. The open arm blocks discriminant narrowing, so read
 * `attrs.level`/`attrs.lang` behind a check of the arm you want. */
export type ContentLine = {
    containers: ContentContainer[];
    /** A within-block hard line break rather than a new block. Omitted (false) in the common case. */
    continues?: boolean;
} & ContentLineKind;

/** A line's block role, shared by `ContentLine` and the `setKind` op. */
export type ContentLineKind =
    | { kind: "para" }
    | { kind: "heading"; attrs: { level: number } }
    | { kind: "code"; attrs?: { lang?: string } }
    | { kind: "island" }
    | { kind: "rule" }
    | { kind: string; attrs?: unknown };

/** An ancestor block a line nests inside, outermost first. Open like
 * `ContentLine.kind`: an unrecognized container round-trips with opaque `attrs`
 * and renders transparently (its lines sit at the enclosing level).
 *
 * Two adjacent lines sit in the same container iff their whole path matches, so
 * `instance` is what tells one container from an adjacent sibling of identical
 * shape — two consecutive quotes, two consecutive lists — which contiguity
 * alone reads as one.
 *
 * **A writer owes a distinct value per adjacent sibling run**, not merely a
 * value. Runs of one shape sharing one arrive as one: a second list's items come
 * back as continuation paragraphs of the first, markers gone. The field is
 * required, so a checker reports the omission; it cannot report a `0` stamped on
 * both, which is the same write. A codec flattening a tree takes them from
 * `assignInstances` in `@quillmark/wasm/runtime` rather than by hand. Any
 * distinct pair works; a write is canonicalized to `0`/`1`.
 *
 * Reading is not the mirror of writing. Every read spells the field, the `0` on
 * a container with nothing to be told apart from included. A read also carries a
 * discriminator on pairs no writer had to spell: `1.` beside a list starting at
 * `3` differs by `start`, so those runs arrive apart with nothing written, and
 * the canonical form spends one anyway because Markdown reads only a list's
 * first number.
 *
 * Content parsed from a stored document is the one shape that arrives without
 * it — storage omits a zero — and needs a cast. */
export type ContentContainer =
    | {
          container: "list_item";
          attrs: { ordered: boolean; start: number; ordinal: number };
          instance: number;
      }
    | { container: "quote"; instance: number }
    | { container: string; attrs?: unknown; instance: number };

/** A mark over char range `[start, end)` into `Content.text`. The open `type`
 * arm blocks discriminant narrowing, so read a payload-carrying arm behind its
 * guard: `isLinkMark` (`attrs.url`) / `isAnchorMark` (`attrs.id`), from
 * `@quillmark/wasm/runtime`. An `anchor`'s `id` is a caller-supplied opaque
 * handle, unique per `Content` and invariant while the mark lives (positions
 * rebase, the id never does); it has no markdown projection and survives only
 * through the edit lane. */
export type ContentMark = { start: number; end: number } & (
    | { type: "strong" | "emph" | "underline" | "strike" | "code" }
    | { type: "link"; attrs: { url: string } }
    | { type: "anchor"; attrs: { id: string } }
    | { type: string; attrs?: unknown }
);

/** A cell in a `TableProps`. `marks` rides the prose `ContentMark` shape, but
 * each mark's `start`/`end` are USV offsets into this cell's `text`, not into
 * `Content.text`. */
export interface TableCell {
    text: string;
    marks: ContentMark[];
}

/** `props` of a `type: "table"` island: a pipe table normalized to one column
 * count that `header`, every row of `rows`, and `aligns` all share. */
export interface TableProps {
    header: TableCell[];
    rows: TableCell[][];
    /** Per-column alignment, one entry per column. */
    aligns: ("none" | "left" | "center" | "right")[];
}

/** `props` of a `type: "image"` island. */
export interface ImageProps {
    url: string;
    alt: string;
}

/** How faithfully the markdown projection can carry an island. Open: an unknown
 * class round-trips verbatim and reads as `unrepresentable`. */
export type ContentLossClass = "lossless" | "degraded" | "unrepresentable" | (string & {});

/** A structured object occupying one island slot in `Content.text`. `type` is an
 * open set: `props` is `TableProps` for `table` and `ImageProps` for `image`,
 * and any other type round-trips with opaque `props`. The open arm blocks
 * narrowing, so read `props` behind the `isTableIsland` / `isImageIsland`
 * guards (from `@quillmark/wasm/runtime`). */
export type ContentIsland = {
    id: string;
    loss: ContentLossClass;
} & (
    | { type: "table"; props: TableProps }
    | { type: "image"; props: ImageProps }
    | { type: string; props: unknown }
);

/**
 * A write address: one navigation concept for the whole `Document` surface. An
 * absent `field` targets the card body; an absent `card` targets the main card.
 * `{}` is the main-card body; `{ card: 2 }` the body of the composable card at
 * index 2; `{ field: "intro" }` the main card's `intro` field; `{ card: 2,
 * field: "intro" }` a card field.
 *
 * On the `Addr`-taking verbs a **bare string** is shorthand for `{ field: name }`
 * (`doc.storeField("qty", 3)`); a bare number is *not* an addr.
 *
 * `doc.pathFor(addr)` mints the address as its canonical `DocPath` string, the
 * anchor `Diagnostic.path` carries and `session.locate` / `session.fieldBoxes`
 * take. A card path is kind-qualified, so a hand-built one needs the card's
 * `$kind`, and a wrong-kind path matches nothing silently.
 *
 * An `Addr` names a field, never a value inside one: every verb that takes one
 * would then carry an element axis it cannot answer. The one read that reaches
 * inside takes the path as its own argument, `reader.getContentAt(addr, path)`.
 */
export interface Addr {
    card?: number;
    field?: string;
}

/**
 * A card-only address, taken by the card-scoped verbs (`storeFields`,
 * `storeExt`, `getExt`, `commitFields`, …). An absent `card` targets the main
 * card; a present `field` throws.
 */
export interface CardAddr {
    card?: number;
}

/**
 * A text-splice change set over the USV content (CodeMirror `ChangeSet`
 * semantics), returned by `revise` and by the `rebase` codec. Map a stored
 * position through it with `mapPos`.
 *
 * Applying one admits an `insert` string rather than storing it verbatim: `\r`
 * and the Unicode bidi controls drop, and a U+2028 or U+2029 line separator
 * becomes a space. Nothing reports the substitution.
 */
export interface Delta {
    ops: ({ retain: number } | { insert: string } | { delete: number })[];
}

/** Which side of a same-position insertion `mapPos` lands a point on. */
export type Assoc = "before" | "after";

/**
 * A mark edit in final-text coordinates (post-delta, post-line-op). `add` /
 * `remove` carry the `ContentMark` vocabulary; `removeAnchor` drops one identity
 * anchor by id. An `add` of an `anchor` requires a non-empty `id` not already
 * live in the field; a collision or the empty id throws.
 */
export type MarkOp =
    | ({ op: "add" | "remove"; start: number; end: number } & (
          | { type: "strong" | "emph" | "underline" | "strike" | "code" }
          | { type: "link"; url: string }
          | { type: "anchor"; id: string }
          | { type: string; attrs: unknown }
      ))
    | { op: "removeAnchor"; id: string };

/**
 * A line/block edit. `split`/`join` splice `\n` in post-`delta`,
 * post-`islandOps` coordinates; `setKind`/`setContainers`/`setContinues` touch
 * metadata. `setContinues` sets or clears a line's within-block hard-break flag
 * (`ContentLine.continues`); `continues: true` is rejected on line 0, on a line
 * whose containers differ from the line above, and after a heading, island or
 * rule, each of which is a block of one line.
 */
export type LineOp =
    | { op: "split"; at: number }
    | { op: "join"; line: number }
    | ({ op: "setKind"; line: number } & ContentLineKind)
    | { op: "setContainers"; line: number; containers: ContentContainer[] }
    | { op: "setContinues"; line: number; continues: boolean };

/**
 * An island edit: the only channel that reaches an island's payload, a table's
 * cells or an image's url. Both ops leave the field's text and marks alone, so
 * an island edit keeps every identity anchor in the field.
 *
 * `set` addresses an existing island by `id`; an unknown `id` throws. `insert`
 * places a new island's slot at `at` together with its entry, so a slot never
 * exists without an island behind it; its `id` must be non-empty and unused.
 * `at` is a position in the text the `delta` and this bundle's earlier island
 * ops left, so slots after `a` and `b` of `abc` go in at 1 and 3. A stale frame
 * misplaces slots and never throws. A `delta` insert string may not carry a
 * slot, which would orphan: split such a splice into the slot-free `delta` plus
 * one `insert` per slot.
 *
 * Deleting an island needs no op: a `delta` that removes its slot drops the
 * island whole, and a block island's line demotes to `para`. Re-landing it is an
 * `insert` of the full island under its original id; a pasted copy of a live
 * island mints a fresh one.
 *
 * A `set` stores the `loss` it is given; nothing re-derives the class from the
 * new `props`.
 *
 * An island is *inline* (a slot inside a paragraph) unless its line says
 * otherwise. A **block** island is one bundle of all three channels, in the
 * order they apply: `delta` inserts the `\n` that opens the line, `islandOps`
 * inserts the slot, `lineOps` tags the line `{ op: "setKind", kind: "island" }`.
 * `{ op: "split" }` cannot open that line, since line ops run after island ops.
 */
export type IslandOp =
    | ({ op: "set" } & ContentIsland)
    | ({ op: "insert"; at: number } & ContentIsland);

/**
 * A committed content edit bundle for `applyChange`, applied in order: a text
 * `delta`, then `islandOps`, then `lineOps`, then `markOps` (mark ranges are in
 * final-text coordinates). Every field is optional.
 *
 * Within each channel ops apply in sequence against the state the earlier ones
 * left: an island `insert`'s `at` counts earlier ops' slots, and `lineOps`
 * positions and indices renumber through earlier `split`/`join`.
 *
 * **Mark rebase.** `delta`, `islandOps` and `lineOps` each move text, and each
 * rebases the marks already in the field by one rule: a range mark's `start`
 * takes assoc `after` and its `end` `before`, so an insertion at either edge
 * grows text *outside* the span; a **zero-width** mark takes `before`, so an
 * insertion at its own position leaves it put. That last case is the one
 * position where the two assocs differ, and where an anchor most often sits.
 *
 * `markOps` name the result, so a caller emitting them predicts this rebase.
 * `mapMarks(content, bundle)` runs it instead: pass the bundle's text-moving
 * channels, diff the marks it returns against the ones you intend, and emit
 * only the difference. Reproducing the rule by hand is a second copy to drift.
 */
export interface ChangeBundle {
    delta?: Delta;
    islandOps?: IslandOp[];
    lineOps?: LineOp[];
    markOps?: MarkOp[];
}
"#;

/// Device pixels per side: the floor across browser canvas limits (~32k on
/// Chrome/Firefox, 16k on Safari). A request past it clamps `densityScale`
/// proportionally, reported on `PaintResult`.
#[cfg(any(feature = "typst", feature = "pdfform"))]
const MAX_BACKING_DIMENSION: u32 = 16384;

#[cfg(any(feature = "typst", feature = "pdfform"))]
fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        dur.as_millis() as f64
    }
}

/// Render engine: a backend registry and render dispatcher. Render build only:
/// the core build constructs and validates quills without it.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
pub struct Quillmark {
    inner: quillmark::Quillmark,
}

#[wasm_bindgen]
pub struct Quill {
    inner: quillmark::Quill,
}

/// Live render session: every read serves the current compile. `apply(doc)`
/// recompiles a whole document in place, transactionally — on throw the reads
/// keep serving the last-good compile. Geometry is per-compile, so re-read it
/// after each committed `apply`.
///
/// A zero-page document yields a valid session (`pageCount === 0`) whose
/// `paint(ctx, 0)` and `pageSize(0)` throw; branch on `pageCount === 0` rather
/// than catching.
#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
pub struct LiveSession {
    inner: quillmark_core::LiveSession,
    backend_id: String,
    /// Resolves a geometry region's plate-space per-kind ordinal to its
    /// `DocPath` absolute index. Refreshed on every committed `apply`.
    card_kinds: Vec<Option<String>>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
fn card_kinds_of(doc: &quillmark_core::Document) -> Vec<Option<String>> {
    doc.cards()
        .iter()
        .map(|c| c.kind().map(String::from))
        .collect()
}

/// Typed in-memory Quillmark document.
#[wasm_bindgen]
pub struct Document {
    inner: quillmark_core::Document,
    parse_warnings: Vec<quillmark_core::Diagnostic>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl Default for Quillmark {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
impl Quillmark {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Quillmark {
        Quillmark {
            inner: quillmark::Quillmark::new(),
        }
    }

    /// Open a live render session for `doc` against `quill`'s backend.
    #[wasm_bindgen(js_name = open)]
    pub fn open(&self, quill: &Quill, doc: &Document) -> Result<LiveSession, JsValue> {
        let session = self
            .inner
            .open(&quill.inner, &doc.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        Ok(LiveSession {
            inner: session,
            backend_id: quill.inner.backend_id().to_string(),
            card_kinds: card_kinds_of(&doc.inner),
        })
    }

    /// Render `doc` against `quill` in one shot. Convenience over `open` +
    /// `LiveSession.render`: an unset `output_format` falls back to the
    /// backend's first supported format.
    #[wasm_bindgen(js_name = render)]
    pub fn render(
        &self,
        quill: &Quill,
        doc: &Document,
        opts: Option<RenderOptions>,
    ) -> Result<RenderResult, JsValue> {
        let start = now_ms();
        let rust_opts: quillmark_core::RenderOptions = opts.unwrap_or_default().into();
        let result = self
            .inner
            .render(&quill.inner, &doc.inner, &rust_opts)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        let mut warnings: Vec<Diagnostic> =
            doc.parse_warnings.iter().cloned().map(Into::into).collect();
        warnings.extend(result.warnings.into_iter().map(Into::into));
        let kinds: Vec<Option<&str>> = doc.inner.cards().iter().map(|c| c.kind()).collect();
        Ok(RenderResult {
            artifacts: result.artifacts.into_iter().map(Into::into).collect(),
            warnings,
            output_format: result.output_format.into(),
            render_time_ms: now_ms() - start,
            regions: quillmark_core::regions_to_doc_path(result.regions, &kinds)
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    /// The output formats `quill`'s backend can emit; resolves the backend but
    /// compiles nothing. Throws `engine::backend_not_found` when no registered
    /// backend matches the quill's declared one.
    #[wasm_bindgen(js_name = supportedFormats, unchecked_return_type = "OutputFormat[]")]
    pub fn supported_formats(&self, quill: &Quill) -> Result<JsValue, JsValue> {
        let formats = self
            .inner
            .supported_formats(&quill.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        let out: Vec<crate::types::OutputFormat> = formats.iter().map(|f| (*f).into()).collect();
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        out.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("supportedFormats: serialization failed: {e}")).to_js_value()
        })
    }

    /// Whether `quill`'s backend can paint sessions to a canvas; `false` when the
    /// backend is unsupported. A cheap probe before mounting a preview UI. The
    /// authoritative answer is the session's `supportsCanvas` getter.
    #[wasm_bindgen(js_name = supportsCanvas)]
    pub fn supports_canvas(&self, quill: &Quill) -> bool {
        self.inner.supports_canvas(&quill.inner)
    }
}

#[wasm_bindgen]
impl Quill {
    /// Build a quill from a file tree. Pure: the declared backend is resolved
    /// later, at render time. Accepts a `Map<string, Uint8Array>` or a plain
    /// object.
    #[wasm_bindgen(js_name = fromTree)]
    pub fn from_tree(
        #[wasm_bindgen(unchecked_param_type = "Map<string, Uint8Array>")] tree: JsValue,
    ) -> Result<Quill, JsValue> {
        let root = file_tree_from_js_tree(&tree)?;
        let quill = quillmark::Quill::from_tree(root)
            .map_err(|diags| WasmError { diagnostics: diags }.to_js_value())?;
        Ok(Quill { inner: quill })
    }

    /// Flatten this quill back into its canonical file tree, the inverse of
    /// [`fromTree`](Self::from_tree). Keys are `"/"`-joined relative paths.
    ///
    /// This is how a quill crosses a WASM linear-memory boundary as data: a
    /// `Quill` built in one build cannot be passed to an engine in another, so
    /// `@quillmark/wasm/runtime` re-feeds this tree to the backend build's
    /// `Quill.fromTree` on demand.
    #[wasm_bindgen(js_name = toTree, unchecked_return_type = "Map<string, Uint8Array>")]
    pub fn to_tree(&self) -> JsValue {
        let map = js_sys::Map::new();
        for (path, contents) in self.inner.to_tree() {
            let bytes = Uint8Array::from(contents.as_slice());
            map.set(&JsValue::from_str(&path), &bytes);
        }
        map.into()
    }

    /// The *declared* backend identifier (e.g. `"typst"`): intent, not a
    /// resolved capability. Capability is read from the engine.
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.inner.backend_id().to_string()
    }

    #[wasm_bindgen(getter, js_name = blueprint)]
    pub fn blueprint(&self) -> String {
        self.inner.config().blueprint()
    }

    /// Document schema for the quill: the user-fillable fields plus their `ui`
    /// hints. Key order in `fields`/`properties` is declaration order, the
    /// ordering contract.
    #[wasm_bindgen(getter, js_name = schema, unchecked_return_type = "QuillSchema")]
    pub fn schema(&self) -> Result<JsValue, JsValue> {
        let value = self.inner.config().schema();
        serialize_or_throw(&value, "schema")
    }

    /// Identity snapshot of the `quill:` section of `Quill.yaml` plus any extra
    /// `quill:` keys. Pure config: output formats are a resolved-backend
    /// capability read from `Quillmark.supportedFormats`, not part of this.
    #[wasm_bindgen(getter, js_name = metadata, unchecked_return_type = "QuillMetadata")]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        let source = &self.inner;
        let config = source.config();

        let mut obj = serde_json::Map::new();
        obj.insert(
            "name".to_string(),
            serde_json::Value::String(config.name.clone()),
        );
        obj.insert(
            "version".to_string(),
            serde_json::Value::String(config.version.clone()),
        );
        obj.insert(
            "backend".to_string(),
            serde_json::Value::String(config.backend.clone()),
        );
        obj.insert(
            "author".to_string(),
            serde_json::Value::String(config.author.clone()),
        );
        obj.insert(
            "description".to_string(),
            serde_json::Value::String(config.description.clone()),
        );

        for (key, value) in source.metadata() {
            if quillmark_core::STANDARD_METADATA_KEYS.contains(&key.as_str()) {
                continue;
            }
            if obj.contains_key(key) {
                continue;
            }
            obj.insert(key.clone(), value.as_json().clone());
        }

        let val = serde_json::Value::Object(obj);
        serialize_or_throw(&val, "metadata")
    }

    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (empty when the document is valid). Forwards the canonical
    /// `validation::*` diagnostics the engine emits, including the non-fatal
    /// `validation::must_fill` warning per `!must_fill` marker left behind.
    #[wasm_bindgen(js_name = validate, unchecked_return_type = "Diagnostic[]")]
    pub fn validate(&self, doc: &Document) -> Result<JsValue, JsValue> {
        let diags = self.inner.validate(&doc.inner);
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        diags.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("validate: serialization failed: {e}")).to_js_value()
        })
    }

    /// Parse `markdown` and conform it against this quill: the primary ingestion
    /// path, and the bound twin of the schema-free `Document.fromMarkdown`. The
    /// returned document rests at its canonical form (a `richtext` field as a
    /// content object, a `plaintext` field as its literal string), so `getStored`
    /// answers by the field's declared codec, not by how the document was built.
    ///
    /// Parse warnings and the `conform::*` diagnostics both land on
    /// `doc.warnings`. Throws on a parse failure, or when `markdown` declares a
    /// `$quill` this quill does not answer to. To open a document whose `$quill`
    /// is stale, use `Document.fromMarkdown`, `setQuillRef`, then `quill.conform`.
    #[wasm_bindgen(js_name = parse)]
    pub fn parse(&self, markdown: &str) -> Result<Document, JsValue> {
        let parsed = self
            .inner
            .parse(markdown)
            .map_err(|e| WasmError::from(e.to_diagnostics()).to_js_value())?;
        Ok(Document {
            inner: parsed.document,
            parse_warnings: parsed.warnings,
        })
    }

    /// Land `doc`'s declared content fields at their canonical rest **in
    /// place**, returning the `conform::*` diagnostics for values that would not
    /// commit. The read-repair verb for a document that arrived through the
    /// transport door (`fromMarkdown`, `fromStored`, a stored row).
    ///
    /// Idempotent: an equal value is not rewritten, so YAML comments and stored
    /// bytes survive. A `!must_fill` marker anywhere in a field's value skips
    /// that field, and a value the strict write refuses stays as authored with a
    /// diagnostic. Throws when `doc` declares a different `$quill`, before any
    /// mutation.
    #[wasm_bindgen(js_name = conform, unchecked_return_type = "Diagnostic[]")]
    pub fn conform(&self, doc: &mut Document) -> Result<JsValue, JsValue> {
        let diags = self
            .inner
            .conform(&mut doc.inner)
            .map_err(WasmError::from)
            .map_err(|e| e.to_js_value())?;
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        diags.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("conform: serialization failed: {e}")).to_js_value()
        })
    }

    /// The resolved-value view of `doc`: the ABI under `reader.resolve()`. For
    /// every declared field, the value the render projection would use and the
    /// `FieldSource` rung it came from (`"authored" | "default" | "blank"`). The
    /// card body is a `body` sibling on its card, never a row in `fields`, and
    /// `null` when the kind enables no body. Value and provenance only;
    /// completeness stays `validate`'s.
    #[wasm_bindgen(js_name = _resolve, skip_typescript, unchecked_return_type = "Resolved")]
    pub fn resolve(&self, doc: &Document) -> Result<JsValue, JsValue> {
        let states = self.inner.resolve(&doc.inner);
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        states.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("resolve: serialization failed: {e}")).to_js_value()
        })
    }

    /// Seed a starter `Document` from the schema: the main card plus one instance
    /// of each composable card kind, each committing its fields' `example:`
    /// values and leaving every other field absent (interpolated at render as
    /// `default:`, else the field's blank). A field with both renders its example.
    #[wasm_bindgen(js_name = seedDocument)]
    pub fn seed_document(&self) -> Document {
        Document {
            inner: self.inner.seed_document(),
            parse_warnings: Vec::new(),
        }
    }

    /// Seed a starter main `Card` (carries `$quill`) from the schema: the
    /// `$kind: main` card of [`seedDocument`](Self::seed_document) alone.
    #[wasm_bindgen(js_name = seedMain, unchecked_return_type = "Card")]
    pub fn seed_main(&self) -> Result<JsValue, JsValue> {
        card_to_js(&self.inner.seed_main())
    }

    /// Seed a starter composable `Card` of the given kind (carries `$kind`),
    /// layering an optional per-kind seed `overlay` over the schema-example base
    /// (`overlay › example › absent`). `undefined` when `cardKind` is not
    /// declared in this quill's schema.
    ///
    /// Pass `document.seedOverlay(cardKind)` as `overlay` so a card added to a
    /// template-derived document inherits its curated starting values; omit it
    /// for the bare schema seed.
    #[wasm_bindgen(js_name = seedCard, unchecked_return_type = "Card | undefined")]
    pub fn seed_card(
        &self,
        card_kind: &str,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown> | undefined")]
        overlay: JsValue,
    ) -> Result<JsValue, JsValue> {
        let overlay = if overlay.is_undefined() || overlay.is_null() {
            None
        } else {
            let json = js_value_to_json(overlay, "seedCard")?;
            quillmark_core::SeedOverlay::from_json(&json)
        };
        match self.inner.seed_card(card_kind, overlay.as_ref()) {
            Some(core_card) => card_to_js(&core_card),
            None => Ok(JsValue::UNDEFINED),
        }
    }
}

#[wasm_bindgen]
impl Document {
    /// A blank document: a main card carrying only `$quill`, an empty body, and
    /// no composable cards. Absent fields resolve at render time (`default`, else
    /// the field's blank), so nothing the caller did not set reaches the output.
    /// For an example-filled starter use `Quill.seedDocument()`. Throws on an
    /// invalid quill reference.
    #[wasm_bindgen(constructor)]
    pub fn new(quill_ref: &str) -> Result<Document, JsValue> {
        let qr: quillmark_core::QuillReference = quill_ref
            .parse()
            .map_err(|e: String| invalid_quill_reference("Document", quill_ref, &e))?;
        Ok(Document {
            inner: quillmark_core::Document::new(qr),
            parse_warnings: Vec::new(),
        })
    }

    /// Parse markdown into a typed Document. Throws on parse errors.
    #[wasm_bindgen(js_name = fromMarkdown)]
    pub fn from_markdown(markdown: &str) -> Result<Document, JsValue> {
        let output = quillmark_core::Document::parse(markdown)
            .map_err(WasmError::from)
            .map_err(|e| e.to_js_value())?;

        Ok(Document {
            inner: output.document,
            parse_warnings: output.warnings,
        })
    }

    /// Reconstruct a `Document` from a versioned storage DTO string produced by
    /// [`toStored`](Document::to_stored). The result carries no parse-time warnings.
    /// Throws if `json` is not a valid storage DTO (malformed JSON, unknown
    /// `schema`, missing fields, or unparseable quill reference).
    #[wasm_bindgen(js_name = fromStored)]
    pub fn from_stored(json: &str) -> Result<Document, JsValue> {
        let inner: quillmark_core::Document = serde_json::from_str(json).map_err(|e| {
            WasmError::from(format!("fromStored: invalid storage DTO: {e}")).to_js_value()
        })?;
        Ok(Document {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Like [`fromStored`](Document::from_stored) but returns `undefined` instead
    /// of throwing when `json` is not a valid storage DTO, to discriminate format
    /// without exceptions as control flow.
    #[wasm_bindgen(js_name = tryFromStored)]
    pub fn try_from_stored(json: &str) -> Option<Document> {
        let inner: quillmark_core::Document = serde_json::from_str(json).ok()?;
        Some(Document {
            inner,
            parse_warnings: Vec::new(),
        })
    }

    /// Read the storage version tag from a raw storage DTO string without a full
    /// parse, or `undefined`. Unknown future versions come back as-is, which
    /// distinguishes "build too old" from "payload corrupt" when `fromStored`
    /// throws. This is the storage version, not a field schema, though the JSON
    /// key is spelled `"schema"`: that is the DTO's serde tag.
    #[wasm_bindgen(js_name = storageVersionOf)]
    pub fn storage_version_of(json: &str) -> Option<String> {
        quillmark_core::document::peek_storage_version(json)
    }

    /// Storage version this build writes via [`toStored`](Document::to_stored). The
    /// tag advances only when the wire format changes, not on every release.
    #[wasm_bindgen(js_name = currentStorageVersion)]
    pub fn current_storage_version() -> String {
        quillmark_core::document::STORAGE_V0_112_0.to_string()
    }

    /// Authoring-format rules for the card-yaml markdown surface, re-exposed from
    /// core. Constant across calls; read once and cache.
    #[wasm_bindgen(js_name = formatRules)]
    pub fn format_rules() -> String {
        quillmark_core::document::FORMAT_RULES.to_string()
    }

    /// Authoring-ergonomics header introducing a blueprint to an LLM/MCP consumer
    /// for the given `quillName`, re-exposed from core.
    #[wasm_bindgen(js_name = blueprintInstruction)]
    pub fn blueprint_instruction(quill_name: &str) -> String {
        quillmark_core::document::blueprint_instruction(quill_name)
    }

    /// The canonical `$quill` reference grammar as author-facing text: the same
    /// text the `parse::invalid_quill_reference` hint carries. Drive validation
    /// messages from this instead of re-stating the rule.
    #[wasm_bindgen(js_name = quillRefHint)]
    pub fn quill_ref_hint() -> String {
        quillmark_core::quill_ref_hint().to_string()
    }

    /// Render a Diagnostic as the canonical pretty-printed text, so it looks
    /// identical whichever consumer surfaces it.
    #[wasm_bindgen(js_name = formatDiagnostic)]
    pub fn format_diagnostic(diag: Diagnostic) -> String {
        let core: quillmark_core::Diagnostic = diag.into();
        core.fmt_pretty()
    }

    /// Emit canonical Quillmark Markdown. Round-trip safe: re-parsing the
    /// result produces a `Document` equal to `self` by value and by type.
    #[wasm_bindgen(js_name = toMarkdown)]
    pub fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// Serialize this document to a versioned storage DTO string. Prefer it over
    /// `toMarkdown` for persistence: the wire format is frozen per `schema`
    /// version and the output is byte-deterministic within one, so equal
    /// documents hash equal. Parse-time `warnings` are excluded.
    #[wasm_bindgen(js_name = toStored)]
    pub fn to_stored(&self) -> String {
        // Infallible: derived `Serialize` into a `String` buffer, no `io::Write`.
        serde_json::to_string(&self.inner).expect("Document serialization is infallible")
    }

    #[wasm_bindgen(js_name = clone)]
    pub fn clone_doc(&self) -> Document {
        Document {
            inner: self.inner.clone(),
            parse_warnings: self.parse_warnings.clone(),
        }
    }

    /// Replace this document's contents **in place** from a versioned storage DTO
    /// string: the mutating twin of [`fromStored`](Document::from_stored).
    /// Parse-time `warnings` are cleared. Throws on an invalid DTO, leaving the
    /// document unchanged.
    ///
    /// The cross-WASM-memory `Document` bridge: mutate a document on a
    /// backend-memory clone, then write the state back into the caller's
    /// canonical document, without the caller re-binding its variable.
    #[wasm_bindgen(js_name = loadStored)]
    pub fn load_stored(&mut self, json: &str) -> Result<(), JsValue> {
        let inner: quillmark_core::Document = serde_json::from_str(json).map_err(|e| {
            WasmError::from(format!("loadStored: invalid storage DTO: {e}")).to_js_value()
        })?;
        self.inner = inner;
        self.parse_warnings.clear();
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = quillRef)]
    pub fn quill_ref(&self) -> String {
        self.inner.quill_reference().to_string()
    }

    /// The document's main (entry) card. Allocates and serializes on each call.
    #[wasm_bindgen(getter, js_name = main, unchecked_return_type = "Card")]
    pub fn main(&self) -> Result<JsValue, JsValue> {
        card_to_js(self.inner.main())
    }

    #[wasm_bindgen(getter, js_name = cards, unchecked_return_type = "Card[]")]
    pub fn cards(&self) -> Result<JsValue, JsValue> {
        let cards: Vec<quillmark_core::CardWire> =
            self.inner.cards().iter().map(Into::into).collect();
        serialize_or_throw(&cards, "cards")
    }

    /// Read the **verbatim stored value** at `addr`: a field's raw payload value,
    /// or the body content when `addr.field` is absent. A bare string is `Addr`
    /// shorthand for `{ field }`. Needs no schema: the read echo of the verbatim
    /// `store*` write, distinct from the interpreted
    /// [`reader.get`](Self::reader_get). Reads are total over the field axis — an
    /// absent field is `undefined` — and only an out-of-range `addr.card` throws
    /// `edit::index_out_of_range`.
    ///
    /// A content field at rest has one stored form per codec: a `richtext` field
    /// holds the canonical content object, a `plaintext` field its literal
    /// string. A document from the bound door (`quill.parse` / `quill.conform`)
    /// is at rest; one from the transport door may rest as authored until it is
    /// conformed, and this read reports what is there. For the `Content` either
    /// way use `reader.getContent`.
    ///
    /// The body arm is typed `Content` and answers in the seam form, spelling
    /// every `ContentContainer.instance`. A field arm echoes the stored bytes,
    /// which omit a zero: verbatim is the contract, and is why it is `unknown`.
    #[wasm_bindgen(js_name = getStored, unchecked_return_type = "unknown")]
    pub fn get_stored(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let card = self.addr_card_ref(&addr)?;
        match &addr.field {
            None => serialize_or_throw(
                &quillmark_content::serial::to_seam_value(card.body()),
                "getStored",
            ),
            Some(field) => match card.payload().get(field) {
                Some(v) => serialize_or_throw(v.as_json(), "getStored"),
                None => Ok(JsValue::UNDEFINED),
            },
        }
    }

    /// The **body** markdown projection: an on-demand, lossy export (content-only
    /// marks do not survive markdown). A body's type is a format fact, not a
    /// schema fact, so this read stays quill-free, and a body is never absent.
    ///
    /// `addr` is an optional card address (absent = main). A present `field`
    /// throws: read a field's markdown through `quill.reader(doc).get(field)`,
    /// which interprets by declared type. An out-of-range `addr.card` throws.
    #[wasm_bindgen(js_name = bodyMarkdown, unchecked_return_type = "string")]
    pub fn get_markdown(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "CardAddr")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        if addr.field.is_some() {
            return Err(WasmError::from(
                "bodyMarkdown is body-only: read a field's markdown with \
                 quill.reader(doc).get(field)",
            )
            .to_js_value());
        }
        Ok(JsValue::from_str(&self.addr_card_ref(&addr)?.body_markdown()))
    }

    /// Interpreted read at `addr`, resolving the field's declared `type` from
    /// `quill`: the stable ABI under the runtime `reader.get`. The schema-plane
    /// twin of [`getStored`](Self::get_stored) — a `richtext` field returns its
    /// markdown projection, every other declared type its canonical value
    /// verbatim.
    ///
    /// A bare string is `Addr` shorthand for `{ field }`; `{ card, field }`
    /// targets a composable card. An **absent** field returns `undefined`, and an
    /// absent `addr.field` reads the body markdown. An undeclared name throws
    /// `edit::unknown_field` (the authority `bodyMarkdown` lacks), a `richtext`
    /// value that does not decode throws `edit::field_decode`, and an
    /// out-of-range `addr.card` throws.
    ///
    /// The `quill` handle is passed per call because a `Document` carries only a
    /// `$quill` reference, not the resolved schema.
    #[wasm_bindgen(js_name = _readerGet, skip_typescript, unchecked_return_type = "unknown")]
    pub fn reader_get(
        &self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let base = self.addr_base(&addr);
        let reader = quill.inner.reader(&self.inner);
        match &addr.field {
            None => Ok(JsValue::from_str(&match addr.card {
                None => reader.body_markdown(),
                Some(index) => reader
                    .card(index)
                    .map_err(|e| edit_error_to_js(&e, &base))?
                    .body_markdown(),
            })),
            Some(field) => {
                let read = match addr.card {
                    None => reader.get(field),
                    Some(index) => reader
                        .card(index)
                        .map_err(|e| edit_error_to_js(&e, &base))?
                        .get(field),
                }
                .map_err(|e| edit_error_to_js(&e, &base))?;
                match read {
                    None => Ok(JsValue::UNDEFINED),
                    Some(v) => serialize_or_throw(v.as_json(), "reader.get"),
                }
            }
        }
    }

    /// Interpreted **`Content`** read at `addr`: the stable ABI under the runtime
    /// `reader.getContent`. Where [`reader.get`](Self::reader_get) projects, this
    /// decodes the stored value through the codec the field's declared type names
    /// and returns canonical `Content`. Total over the storage form: a committed
    /// field and a parsed one both read back as a `Content`, so a content editor
    /// stops branching on how the document was built.
    ///
    /// A bare string is `Addr` shorthand for `{ field }`. An **absent** field
    /// returns `undefined`, and an absent `addr.field` reads the body `Content`.
    /// Throws `edit::unknown_field` for an undeclared name,
    /// `edit::field_not_content` for a declared type that is not a content leaf
    /// (`array<richtext>` carries content and still has no one `Content`),
    /// `edit::field_decode` for a value that decodes under neither encoding, and
    /// `edit::index_out_of_range` for a bad `addr.card`.
    #[wasm_bindgen(js_name = _readerGetContent, skip_typescript, unchecked_return_type = "Content | undefined")]
    pub fn reader_get_content(
        &self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let base = self.addr_base(&addr);
        match &addr.field {
            None => serialize_or_throw(
                &quillmark_content::serial::to_seam_value(self.addr_card_ref(&addr)?.body()),
                "reader.getContent",
            ),
            Some(field) => {
                let reader = quill.inner.reader(&self.inner);
                let read = match addr.card {
                    None => reader.get_content(field),
                    Some(index) => reader
                        .card(index)
                        .map_err(|e| edit_error_to_js(&e, &base))?
                        .get_content(field),
                }
                .map_err(|e| edit_error_to_js(&e, &base))?;
                match read {
                    None => Ok(JsValue::UNDEFINED),
                    Some(content) => serialize_or_throw(
                        &quillmark_content::serial::to_seam_value(&content),
                        "reader.getContent",
                    ),
                }
            }
        }
    }

    /// Interpreted **`Content`** read of a value nested inside the composite
    /// field at `addr`: the stable ABI under the runtime `reader.getContentAt`,
    /// and [`reader.getContent`](Self::reader_get_content) with the path spelled
    /// out. `path` is a `PathStep[]` from the field to the leaf — `[0]` an
    /// element of an `array<richtext>`, `["motto"]` an `object`'s content
    /// property, `[1, "notes"]` a leaf under both, `["controlled_by"]` a
    /// variant's cell.
    ///
    /// The codec is the leaf's declared type's, so the caller stops deciding
    /// what an element's stored bytes mean. Total over the storage form, as the
    /// whole-field read is.
    ///
    /// `undefined` when the field is absent **and when `path` names nothing in
    /// the stored value**: a repeater's row index goes stale between derive and
    /// read, and absence there is a read, not a fault. Throws
    /// `edit::unknown_field` for an undeclared name at any depth,
    /// `edit::field_not_content` when `path` resolves to no content leaf,
    /// `edit::field_decode` for a value that decodes under neither encoding
    /// (anchored at the addressed path), and `edit::index_out_of_range` for a
    /// bad `addr.card`. A body address throws: a body has no nested content
    /// address.
    #[wasm_bindgen(js_name = _readerGetContentAt, skip_typescript, unchecked_return_type = "Content | undefined")]
    pub fn reader_get_content_at(
        &self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "PathStep[]")] path: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("reader.getContentAt")?.to_string();
        let at = path_from_js(&path, "reader.getContentAt")?;
        let base = self.addr_base(&addr);
        let reader = quill.inner.reader(&self.inner);
        let read = match addr.card {
            None => reader.get_content_at(&field, &at),
            Some(index) => reader
                .card(index)
                .map_err(|e| edit_error_to_js(&e, &base))?
                .get_content_at(&field, &at),
        }
        .map_err(|e| edit_error_to_js(&e, &base))?;
        match read {
            None => Ok(JsValue::UNDEFINED),
            Some(content) => serialize_or_throw(
                &quillmark_content::serial::to_seam_value(&content),
                "reader.getContentAt",
            ),
        }
    }

    /// Whether the field at `addr` is marked `!must_fill`. A bare string is `Addr`
    /// shorthand for `{ field }`. `false` for an absent field and for a body
    /// address; only an out-of-range `addr.card` throws.
    #[wasm_bindgen(js_name = isFill)]
    pub fn is_fill(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<bool, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let card = self.addr_card_ref(&addr)?;
        Ok(match &addr.field {
            None => false,
            Some(field) => card.payload().is_fill(field),
        })
    }

    /// The whole `$ext` map at `addr` (a card address, absent `card` = main), or
    /// `undefined` when the card carries none: the `$ext` read that avoids
    /// serializing the whole card. Throws on a present `field` or an
    /// out-of-range card.
    #[wasm_bindgen(js_name = getExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn get_ext(
        &self,
        #[wasm_bindgen(unchecked_optional_param_type = "CardAddr")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("getExt")?;
        match self.addr_card_ref(&addr)?.ext() {
            Some(map) => serialize_or_throw(map, "getExt"),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// The value stored under `$ext[ns]` at `addr` (a card address, absent `card`
    /// = main), or `undefined`. Throws on a present `field` or an out-of-range
    /// card.
    #[wasm_bindgen(js_name = getExtNamespace, unchecked_return_type = "unknown")]
    pub fn get_ext_namespace(
        &self,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        ns: &str,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("getExtNamespace")?;
        match self.addr_card_ref(&addr)?.ext().and_then(|m| m.get(ns)) {
            Some(v) => serialize_or_throw(v, "getExtNamespace"),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Number of composable cards, excluding the main card.
    #[wasm_bindgen(getter, js_name = cardCount)]
    pub fn card_count(&self) -> usize {
        self.inner.cards().len()
    }

    /// A single composable card by index, so reading one need not materialize
    /// every card via [`cards`](Self::cards). An out-of-range `index` throws
    /// `edit::index_out_of_range`.
    #[wasm_bindgen(js_name = card, unchecked_return_type = "Card")]
    pub fn card(&self, index: usize) -> Result<JsValue, JsValue> {
        card_to_js(self.card_or_throw(index)?)
    }

    /// The main card's `$seed[kind]` overlay object, or `undefined`. Feeds
    /// `quill.seedCard(kind, overlay)` without serializing the whole main card,
    /// and keeps `seedCard` pure: the quill never reads the document.
    #[wasm_bindgen(js_name = seedOverlay, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn seed_overlay(&self, kind: &str) -> Result<JsValue, JsValue> {
        match self.inner.main().seed().and_then(|seed| seed.get(kind)) {
            Some(overlay) => serialize_or_throw(overlay, "seedOverlay"),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// `addr`'s canonical `DocPath` string, the anchor `Diagnostic.path` carries:
    /// `pathFor()` is `main.body`, `pathFor("intro")` `main.intro`,
    /// `pathFor({card: 2})` `cards.<kind>[2].body`.
    ///
    /// The kind is the card's stored `$kind` verbatim, not `validate`'s
    /// declared-kind filter, since a `Document` holds a `$quill` reference and no
    /// schema. That is the one edge where this path and a `validate` diagnostic
    /// path differ for the same card.
    ///
    /// **Total on the index axis**, unlike the `Addr` reads, which throw there:
    /// an out-of-range `{card: 7, field: "from"}` renders `cards[7].from`, which
    /// parses back and resolves to nothing rather than mis-targeting. Only a
    /// malformed address throws.
    #[wasm_bindgen(js_name = pathFor)]
    pub fn path_for(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<String, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let base = self.addr_base(&addr);
        Ok(match &addr.field {
            Some(field) => base.field(field),
            None => base.body(),
        }
        .to_string())
    }

    /// The composable card's own path, `cards.<kind>[index]`: the root
    /// [`pathFor`](Self::path_for) extends, for anchoring the card rather than
    /// one of its fields. Total on the index axis; out of range renders
    /// `cards[index]`.
    #[wasm_bindgen(js_name = cardPath)]
    pub fn card_path(&self, index: usize) -> String {
        self.addr_base(&Addr {
            card: Some(index),
            field: None,
        })
        .to_string()
    }

    /// Structural equality, excluding parse-time `warnings`.
    #[wasm_bindgen(js_name = equals)]
    pub fn equals(&self, other: &Document) -> bool {
        self.inner == other.inner
    }

    /// The non-fatal diagnostics of the load that produced this document: parse
    /// warnings, plus `conform::*` warnings when it came through `quill.parse`.
    /// Session state, not document value: `equals` and the storage DTO exclude
    /// it, and `fromStored` / `loadStored` clear it.
    #[wasm_bindgen(getter, js_name = warnings, unchecked_return_type = "Diagnostic[]")]
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let diags: Vec<Diagnostic> = self
            .parse_warnings
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        serialize_or_throw(&diags, "warnings")
    }

    /// Store a field verbatim at `addr`, deferring coercion to render; the typed
    /// write is [`commitField`](Document::commit_field). A bare string is `Addr`
    /// shorthand for `{ field }`; `{ card: 2, field: "qty" }` targets a
    /// composable card. Clears any `!must_fill` marker. A body address throws:
    /// write a body with `revise` / `overwrite`. Throws on an out-of-range card
    /// or a malformed name.
    #[wasm_bindgen(js_name = storeField)]
    pub fn store_field(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("storeField")?.to_string();
        let json = js_value_to_json(value, "storeField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        let base = self.addr_base(&addr);
        self.addr_card_mut(&addr)?
            .store_field(&field, qv)
            .map_err(|e| edit_error_to_js(&e, &base))
    }

    /// Store a field verbatim at `addr` and mark it `!must_fill`. A body address
    /// throws; same validation as [`storeField`](Document::store_field).
    #[wasm_bindgen(js_name = storeFill)]
    pub fn store_fill(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("storeFill")?.to_string();
        let json = js_value_to_json(value, "storeFill")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        let base = self.addr_base(&addr);
        self.addr_card_mut(&addr)?
            .store_fill(&field, qv)
            .map_err(|e| edit_error_to_js(&e, &base))
    }

    /// Store several fields verbatim and atomically on the card `addr` targets.
    /// `addr` is a **card address** (`{ card }`, absent = main) and comes first
    /// because `card` is itself a legal field name; a present `field` throws.
    /// Nothing is applied on error, and the thrown error's `diagnostics` carry
    /// one entry per offending field. Throws on an out-of-range card.
    #[wasm_bindgen(js_name = storeFields)]
    pub fn store_fields(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("storeFields")?;
        let batch = js_value_to_field_batch(&fields, "storeFields")?;
        let base = self.addr_base(&addr);
        self.addr_card_mut(&addr)?
            .store_fields(batch)
            .map_err(|errs| edit_errors_to_js(errs, &base))
    }

    /// Remove a field at `addr`, returning the removed value or `undefined`. A
    /// bare string is `Addr` shorthand for `{ field }`. A body address throws, as
    /// does an out-of-range card or a malformed name.
    #[wasm_bindgen(js_name = removeField)]
    pub fn remove_field(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("removeField")?.to_string();
        let base = self.addr_base(&addr);
        let removed = self
            .addr_card_mut(&addr)?
            .remove_field(&field)
            .map_err(|e| edit_error_to_js(&e, &base))?;
        Ok(match removed {
            Some(v) => serialize_or_throw(v.as_json(), "removeField")?,
            None => JsValue::UNDEFINED,
        })
    }

    /// Replace the opaque `$ext` map on the card `addr` targets (absent `card` =
    /// main). `value` must be a plain object. `$ext` carries out-of-band consumer
    /// state and never reaches the rendered output. Throws on a present `field`
    /// or an out-of-range card.
    #[wasm_bindgen(js_name = storeExt)]
    pub fn store_ext(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("storeExt")?;
        let map = js_value_to_object(&value, "storeExt")?;
        let base = self.addr_base(&addr);
        self.addr_card_mut(&addr)?
            .store_ext(map)
            .map_err(|e| edit_error_to_js(&e, &base))
    }

    /// Remove the `$ext` map on the card `addr` targets entirely, returning the
    /// previous map or `undefined`. Discards every namespace at once; prefer
    /// `removeExtNamespace`. Throws on a present `field` or an out-of-range card.
    #[wasm_bindgen(js_name = removeExt, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub fn remove_ext(
        &mut self,
        #[wasm_bindgen(unchecked_optional_param_type = "CardAddr")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("removeExt")?;
        ext_map_to_js(self.addr_card_mut(&addr)?.remove_ext())
    }

    /// Merge `value` into `$ext[ns]` on the card `addr` targets, preserving
    /// sibling namespaces: the recommended `$ext` write. Throws on a present
    /// `field` or an out-of-range card.
    #[wasm_bindgen(js_name = storeExtNamespace)]
    pub fn store_ext_namespace(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        ns: &str,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("storeExtNamespace")?;
        let json = js_value_to_json(value, "storeExtNamespace")?;
        let base = self.addr_base(&addr);
        self.addr_card_mut(&addr)?
            .store_ext_namespace(ns, json)
            .map_err(|e| edit_error_to_js(&e, &base))
    }

    /// Remove `$ext[ns]` on the card `addr` targets, returning its value or
    /// `undefined`; drops `$ext` once empty. `addr` is a card address (absent =
    /// main). Preserves sibling namespaces. Throws on a present `field` or an
    /// out-of-range card.
    #[wasm_bindgen(js_name = removeExtNamespace)]
    pub fn remove_ext_namespace(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        ns: &str,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("removeExtNamespace")?;
        json_value_to_js(self.addr_card_mut(&addr)?.remove_ext_namespace(ns))
    }

    /// Merge a card-kind's seed `overlay` into the **main** card's `$seed` map
    /// under `cardKind`, preserving sibling kinds; `$seed` is main-only, so this
    /// takes no address. Sets the starting values new cards of that kind spawn
    /// with. Throws if `overlay` cannot be serialized or nests too deep.
    #[wasm_bindgen(js_name = storeSeedOverlay)]
    pub fn store_seed_overlay(
        &mut self,
        card_kind: &str,
        overlay: JsValue,
    ) -> Result<(), JsValue> {
        let json = js_value_to_json(overlay, "storeSeedOverlay")?;
        // `$seed` is config-space, so an error here carries no field anchor.
        self.inner
            .main_mut()
            .store_seed_overlay(card_kind, json)
            .map_err(|e| edit_error_to_js(&e, &quillmark_core::DocPath::new()))
    }

    /// Remove `cardKind` from the main card's `$seed` map, returning its overlay
    /// or `undefined`; drops `$seed` entirely once empty. Sibling kinds survive.
    #[wasm_bindgen(js_name = removeSeedOverlay)]
    pub fn remove_seed_overlay(&mut self, card_kind: &str) -> Result<JsValue, JsValue> {
        json_value_to_js(self.inner.main_mut().remove_seed_overlay(card_kind))
    }

    /// Replace the QUILL reference string. Throws if `ref_str` is invalid.
    #[wasm_bindgen(js_name = setQuillRef)]
    pub fn set_quill_ref(&mut self, ref_str: &str) -> Result<(), JsValue> {
        let qr: quillmark_core::QuillReference = ref_str
            .parse()
            .map_err(|e: String| invalid_quill_reference("setQuillRef", ref_str, &e))?;
        self.inner.set_quill_ref(qr);
        Ok(())
    }

    /// **Overwrite** the content value at `addr` with exactly `rt`: value
    /// semantics, so the identity anchors of any previous value are gone. By
    /// anchor fate `overwrite` destroys, [`revise`](Document::revise) rebases,
    /// and [`applyChange`](Document::apply_change) preserves. An absent
    /// `addr.field` targets the body, an absent `addr.card` the main card.
    ///
    /// Throws on an out-of-range card, a malformed field name, or an `rt` that is
    /// not a canonical content object.
    #[wasm_bindgen(js_name = overwrite)]
    pub fn overwrite(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Content")] rt: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let content = js_to_authored_content(rt, "overwrite")?;
        let base = self.addr_base(&addr);
        let card = self.addr_card_mut(&addr)?;
        match &addr.field {
            None => {
                card.overwrite_body(content);
                Ok(())
            }
            Some(field) => card
                .overwrite_field(field, content)
                .map_err(|e| edit_error_to_js(&e, &base)),
        }
    }

    /// **Revise** the richtext value at `addr` from a markdown string: the
    /// default write path. Imports the markdown, diffs it against the current
    /// value, rebases surviving identity anchors, and returns the text `Delta` an
    /// editor bridge maps its own positions through (`mapPos`). An absent
    /// `addr.field` targets the body, an absent `addr.card` the main card; an
    /// absent field cold-imports from empty.
    ///
    /// Throws on an out-of-range card, a malformed field name, a present
    /// non-content field value, or an over-nested markdown input.
    #[wasm_bindgen(js_name = revise, unchecked_return_type = "Delta")]
    pub fn revise(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        markdown: &str,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let base = self.addr_base(&addr);
        let card = self.addr_card_mut(&addr)?;
        let delta = match &addr.field {
            None => card.revise_body(markdown),
            Some(field) => card.revise_field(field, markdown),
        }
        .map_err(|e| edit_error_to_js(&e, &base))?;
        serialize_or_throw(&delta, "revise")
    }

    /// Revise the content field at `addr` from authored text, typed *and*
    /// anchor-preserving: the ABI under `writer.reviseField`. Surviving anchors
    /// rebase as in [`revise`](Self::revise), then the diffed result is
    /// schema-conformed, so a `richtext(inline)` field rejects a multi-block
    /// result with `edit::field_not_inline`. Returns the text `Delta`. The codec
    /// is the declared type's: `richtext` diffs markdown, `plaintext` literal
    /// text.
    ///
    /// `addr` must name a field; a body address throws, since a body carries no
    /// field schema (use [`revise`](Self::revise)). An undeclared name throws
    /// `edit::unknown_field`, an out-of-range card throws.
    #[wasm_bindgen(js_name = _reviseField, skip_typescript, unchecked_return_type = "Delta")]
    pub fn revise_field_abi(
        &mut self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        text: &str,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("reviseField")?.to_string();
        let base = self.addr_base(&addr);
        let mut writer = quill.inner.writer(&mut self.inner);
        let delta = match addr.card {
            None => writer.revise_field(&field, text),
            Some(index) => writer
                .card(index)
                .map_err(|e| edit_error_to_js(&e, &base))?
                .revise_field(&field, text),
        }
        .map_err(|e| edit_error_to_js(&e, &base))?;
        serialize_or_throw(&delta, "reviseField")
    }

    /// **Apply** a committed content edit `bundle` at `addr`, the editor splice:
    /// text delta first, then island ops, then line ops, then mark ops (mark
    /// ranges in final-text coordinates), all-or-nothing. An absent `addr.field`
    /// targets the body, an absent `addr.card` the main card. The island channel
    /// moves an island alone, so anchors elsewhere in the field survive an edit
    /// `overwrite` would clear.
    ///
    /// Throws on an out-of-range card, a field that is not richtext, a malformed
    /// bundle, or an op that applies out of bounds; the value is unchanged on a
    /// failed apply.
    ///
    /// Each text-moving channel rebases the marks already in the field, by the
    /// rule on `ChangeBundle`; `mapMarks` answers where they land, so a caller
    /// building `markOps` need not predict it.
    #[wasm_bindgen(js_name = applyChange)]
    pub fn apply_change(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "ChangeBundle")] bundle: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let bundle = parse_change_bundle(&bundle, "applyChange")?;
        let base = self.addr_base(&addr);
        let card = self.addr_card_mut(&addr)?;
        match &addr.field {
            None => card.apply_body_change(&bundle),
            Some(field) => card.apply_field_change(field, &bundle),
        }
        .map_err(|e| edit_error_to_js(&e, &base))
    }

    /// Typed field write at `addr`, resolving the field's schema `type` from
    /// `quill`: the stable ABI under the runtime `writer.set`. One verb for every
    /// field type; the schema carries the `inline` constraint, so no type token
    /// is passed. A richtext-typed field stores canonical content, so identity
    /// marks (anchors, island ids) and content-only marks survive compiles and
    /// the storage DTO. Values use the encoding the seam already speaks: content
    /// object or markdown string for richtext, scalar/array/object otherwise.
    ///
    /// A bare string is `Addr` shorthand for `{ field }`. A body address throws,
    /// having no field schema. A declared field is strict-committed, so a
    /// mismatch throws now rather than at render, and an undeclared name throws
    /// `edit::unknown_field` rather than falling to the opaque store — use
    /// [`storeField`](Document::store_field) for that. Also throws
    /// `edit::field_coercion_failed` / `edit::field_decode` /
    /// `edit::field_not_inline` on a typed mismatch,
    /// `edit::invalid_field_name` on a malformed name, and
    /// `edit::index_out_of_range` on an out-of-range card.
    ///
    /// The `quill` handle is passed per call because a `Document` carries only a
    /// `$quill` reference, not the resolved schema.
    #[wasm_bindgen(js_name = _commitField, skip_typescript)]
    pub fn commit_field(
        &mut self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let field = addr.require_field("commitField")?.to_string();
        let json = js_value_to_json(value, "commitField")?;
        let qv = quillmark_core::QuillValue::from_json(json);
        let base = self.addr_base(&addr);
        let mut writer = quill.inner.writer(&mut self.inner);
        match addr.card {
            None => writer.set(&field, qv).map_err(|e| edit_error_to_js(&e, &base)),
            Some(index) => writer
                .card(index)
                .map_err(|e| edit_error_to_js(&e, &base))?
                .set(&field, qv)
                .map_err(|e| edit_error_to_js(&e, &base)),
        }
    }

    /// Batched twin of [`commitField`](Document::commit_field): typed-commit
    /// several fields on the card `addr` targets atomically. `addr` is a **card
    /// address** (absent = main; a present `field` throws). Same per-field
    /// diagnostic contract as [`storeFields`](Document::store_fields) — nothing
    /// applied on error, one `diagnostics` entry per offending field, including
    /// `edit::unknown_field` for undeclared names. Throws on an out-of-range
    /// card.
    #[wasm_bindgen(js_name = _commitFields, skip_typescript)]
    pub fn commit_fields(
        &mut self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("commitFields")?;
        let batch = js_value_to_field_batch(&fields, "commitFields")?;
        let base = self.addr_base(&addr);
        let mut writer = quill.inner.writer(&mut self.inner);
        match addr.card {
            None => writer
                .set_all(batch)
                .map_err(|errs| edit_errors_to_js(errs, &base)),
            Some(index) => writer
                .card(index)
                .map_err(|e| edit_error_to_js(&e, &base))?
                .set_all(batch)
                .map_err(|errs| edit_errors_to_js(errs, &base)),
        }
    }

    /// The values form at `addr`: the ABI under `reader.values()` (the main-card
    /// address, the whole document as `DocumentValues`) and
    /// `reader.card(i).values()` (`{ card }`, that card as `CardValues`). Every
    /// axis filled; `null` for a card without `$ext` and for a kindless card's
    /// `kind`; a present-null field as `null`. Total on the field axis: a
    /// content leaf that decodes under neither encoding rides out as stored.
    /// An out-of-range `addr.card` throws.
    #[wasm_bindgen(js_name = _readerValues, skip_typescript, unchecked_return_type = "DocumentValues | CardValues")]
    pub fn reader_values(
        &self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("readerValues")?;
        let base = self.addr_base(&addr);
        let reader = quill.inner.reader(&self.inner);
        let serializer = serde_wasm_bindgen::Serializer::new()
            .serialize_maps_as_objects(true)
            .serialize_missing_as_null(true);
        let serialized = match addr.card {
            None => reader.values().serialize(&serializer),
            Some(index) => reader
                .card(index)
                .map_err(|e| edit_error_to_js(&e, &base))?
                .values()
                .serialize(&serializer),
        };
        serialized.map_err(|e| {
            WasmError::from(format!("reader.values: serialization failed: {e}")).to_js_value()
        })
    }

    /// Write the values form at `addr`: the ABI under `writer.setValues` (the
    /// main-card address, a `DocumentValuesInput`) and
    /// `writer.card(i).setValues` (`{ card }`, a `CardValuesInput`).
    ///
    /// An absent axis is untouched; a present one is replaced. All-or-nothing:
    /// nothing is applied on error and every refused cell arrives as one
    /// `diagnostics` entry carrying its own `path` (`main.qty`,
    /// `cards.line_item[0].desc`). A cell whose value equals its projection is
    /// not written, so writing back an unedited `values` read is a no-op. A
    /// malformed `values` (an unknown key, a wrong member type) throws before
    /// any of it is read.
    #[wasm_bindgen(js_name = _setValues, skip_typescript)]
    pub fn set_values(
        &mut self,
        quill: &Quill,
        #[wasm_bindgen(unchecked_param_type = "CardAddr")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "DocumentValuesInput | CardValuesInput")]
        values: JsValue,
    ) -> Result<(), JsValue> {
        let addr = Addr::from_js(&addr)?;
        addr.require_card_only("setValues")?;
        let base = self.addr_base(&addr);
        let json = js_value_to_json(values, "setValues")?;
        let shape_error = |e: serde_json::Error| {
            WasmError::from(format!("setValues: invalid values shape: {e}")).to_js_value()
        };
        let mut writer = quill.inner.writer(&mut self.inner);
        match addr.card {
            None => {
                let values: quillmark_core::DocumentValues =
                    serde_json::from_value(json).map_err(shape_error)?;
                writer.set_values(&values).map_err(edit_errors_at_to_js)
            }
            Some(index) => {
                let values: quillmark_core::CardValues =
                    serde_json::from_value(json).map_err(shape_error)?;
                writer
                    .card(index)
                    .map_err(|e| edit_error_to_js(&e, &base))?
                    .set_values(&values)
                    .map_err(edit_errors_at_to_js)
            }
        }
    }

    /// Build a composable card of `kind`, typed-commit `fields` onto it, set its
    /// body from optional markdown, and place it: the ABI under `writer.addCard`.
    /// `at` absent appends, a number inserts at that index (`0..=cards.length`).
    /// Transactional: the card is committed in full before it joins the document,
    /// so a rejected field (or an invalid kind, body, or out-of-range `at`)
    /// leaves the document untouched. Field errors throw the same per-field
    /// bundle as [`commitFields`](Self::commit_fields); an invalid kind or body,
    /// or a bad position, throws a single-entry bundle keyed `$kind` / `$body`.
    #[wasm_bindgen(js_name = _addCard, skip_typescript)]
    pub fn add_card(
        &mut self,
        quill: &Quill,
        kind: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown>")] fields: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "string")] body: Option<String>,
        #[wasm_bindgen(unchecked_optional_param_type = "number")] at: Option<usize>,
    ) -> Result<(), JsValue> {
        let batch = match fields {
            Some(f) => js_value_to_field_batch(&f, "addCard")?,
            None => Vec::new(),
        };
        // The card is built before it joins the document, so field errors anchor
        // at bare names (empty base).
        quill
            .inner
            .writer(&mut self.inner)
            .add_card(kind, batch, body.as_deref(), at)
            .map_err(|errs| edit_errors_to_js(errs, &quillmark_core::DocPath::new()))
    }

    /// Build a fresh `Card` from a kind and a flat field map: the ergonomic
    /// constructor for `insertCard`, which also takes any `Card` object
    /// directly. Each `fields` entry becomes a card field in insertion order;
    /// `body` defaults to `""`.
    ///
    /// Checks only what a detached card can decide alone: field-name grammar and
    /// value depth. Kind validity is positional, so `insertCard` is its gate and
    /// any kind string is accepted here.
    #[wasm_bindgen(js_name = makeCard, unchecked_return_type = "Card")]
    pub fn make_card(
        kind: String,
        #[wasm_bindgen(unchecked_optional_param_type = "Record<string, unknown>")] fields: Option<
            JsValue,
        >,
        #[wasm_bindgen(unchecked_optional_param_type = "string")] body: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let field_map: serde_json::Map<String, serde_json::Value> = match fields {
            Some(fields) if !fields.is_undefined() && !fields.is_null() => {
                // A backstop under `Card::try_from`'s field-depth cap, catching
                // only what would trap before that check runs; see
                // `reject_deep_js_value` for why it precedes `from_value`.
                reject_deep_js_value(&fields, "makeCard")?;
                serde_wasm_bindgen::from_value(fields).map_err(|e| {
                    WasmError::from(format!("makeCard: `fields` must be an object: {e}"))
                        .to_js_value()
                })?
            }
            _ => serde_json::Map::new(),
        };
        let payload_items = field_map
            .into_iter()
            .map(|(key, value)| quillmark_core::PayloadItemWire::Field {
                key,
                value,
                fill: false,
                nested_fills: Vec::new(),
            })
            .collect();
        let mut string_wire = quillmark_core::CardWire::new(
            kind,
            serde_json::Value::String(body.unwrap_or_default()),
        );
        string_wire.payload_items = payload_items;
        // Round-trip through `Card` so the emitted card carries the content body,
        // not the raw authored string.
        let card =
            quillmark_core::Card::try_from(string_wire).map_err(|e| wire_error_to_js(&e))?;
        let wire = quillmark_core::CardWire::from(&card);
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        wire.serialize(&serializer).map_err(|e| {
            WasmError::from(format!("makeCard: serialization failed: {e}")).to_js_value()
        })
    }

    /// Insert a card: `at` absent appends, a number inserts at that index (in
    /// `0..=cards.length`). Accepts any `CardInput`, including a card read back
    /// out of a document. Throws if `card.kind` is not a valid kind name, or if
    /// `at` is out of range.
    #[wasm_bindgen(js_name = insertCard)]
    pub fn insert_card(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "CardInput")] card: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "number")] at: Option<usize>,
    ) -> Result<(), JsValue> {
        let core_card = js_to_card(&card)?;
        // A kind error anchors at the target slot; an append has no slot yet, so
        // its kind error carries no anchor.
        let base = at.map_or_else(quillmark_core::DocPath::new, |i| {
            quillmark_core::DocPath::card(None, i)
        });
        match at {
            Some(index) => self.inner.insert_card(index, core_card),
            None => self.inner.push_card(core_card),
        }
        .map_err(|e| edit_error_to_js(&e, &base))
    }

    #[wasm_bindgen(js_name = removeCard, unchecked_return_type = "Card | undefined")]
    pub fn remove_card(&mut self, index: usize) -> Result<JsValue, JsValue> {
        match self.inner.remove_card(index) {
            Some(core_card) => card_to_js(&core_card),
            None => Ok(JsValue::UNDEFINED),
        }
    }

    /// Move the card at `from` to position `to`. `from == to` is a no-op.
    #[wasm_bindgen(js_name = moveCard)]
    pub fn move_card(&mut self, from: usize, to: usize) -> Result<(), JsValue> {
        self.inner
            .move_card(from, to)
            .map_err(|e| edit_error_to_js(&e, &quillmark_core::DocPath::new()))
    }

    /// Replace the kind of the card at `index`. Payload and body are untouched;
    /// schema-aware migration is the caller's responsibility.
    /// Throws if `index` is out of range or `newKind` is invalid.
    #[wasm_bindgen(js_name = setCardKind)]
    pub fn set_card_kind(&mut self, index: usize, new_kind: &str) -> Result<(), JsValue> {
        self.inner
            .set_card_kind(index, new_kind)
            .map_err(|e| edit_error_to_js(&e, &quillmark_core::DocPath::card(None, index)))
    }

}

impl Document {
    fn card_mut_or_throw(&mut self, index: usize) -> Result<&mut quillmark_core::Card, JsValue> {
        let len = self.inner.cards().len();
        self.inner.card_mut(index).ok_or_else(|| {
            edit_error_to_js(
                &quillmark_core::EditError::IndexOutOfRange { index, len },
                &quillmark_core::DocPath::new(),
            )
        })
    }

    fn card_or_throw(&self, index: usize) -> Result<&quillmark_core::Card, JsValue> {
        let len = self.inner.cards().len();
        self.inner.cards().get(index).ok_or_else(|| {
            edit_error_to_js(
                &quillmark_core::EditError::IndexOutOfRange { index, len },
                &quillmark_core::DocPath::new(),
            )
        })
    }

    fn addr_card_mut(&mut self, addr: &Addr) -> Result<&mut quillmark_core::Card, JsValue> {
        match addr.card {
            None => Ok(self.inner.main_mut()),
            Some(index) => self.card_mut_or_throw(index),
        }
    }

    fn addr_card_ref(&self, addr: &Addr) -> Result<&quillmark_core::Card, JsValue> {
        match addr.card {
            None => Ok(self.inner.main()),
            Some(index) => self.card_or_throw(index),
        }
    }

    /// The `DocPath` card root an [`Addr`] targets, computed before the mutable
    /// borrow so every addressed mutator can pass it to [`edit_error_to_js`].
    fn addr_base(&self, addr: &Addr) -> quillmark_core::DocPath {
        match addr.card {
            None => quillmark_core::DocPath::main(),
            Some(index) => quillmark_core::DocPath::card(
                self.inner.cards().get(index).and_then(|c| c.kind()),
                index,
            ),
        }
    }
}

/// Mirrors the `Addr` TS interface. Unknown keys are rejected in
/// [`from_js`](Addr::from_js), not via `deny_unknown_fields`: `serde_wasm_bindgen`
/// looks up known fields rather than visiting every key, so it never enforces it.
#[derive(serde::Deserialize, Default)]
struct Addr {
    #[serde(default)]
    card: Option<usize>,
    #[serde(default)]
    field: Option<String>,
}

impl Addr {
    /// `undefined`/`null`/`{}` all mean the main-card body. Rejects a stray key
    /// first so a swapped-arg call (`storeFields(fields, {})`) throws instead of
    /// parsing as the empty main-card address and writing an empty batch.
    fn from_js(value: &JsValue) -> Result<Addr, JsValue> {
        if value.is_undefined() || value.is_null() {
            return Ok(Addr::default());
        }
        if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
            for key in js_sys::Object::keys(obj).iter() {
                if let Some(k) = key.as_string() {
                    if k != "card" && k != "field" {
                        return Err(WasmError::from(format!(
                            "addr has unknown key `{k}`; an address takes only \
                             `card` and `field`"
                        ))
                        .to_js_value());
                    }
                }
            }
        }
        serde_wasm_bindgen::from_value(value.clone())
            .map_err(|e| WasmError::from(format!("addr must be an Addr object: {e}")).to_js_value())
    }

    /// A bare string is shorthand for `{ field: name }`; a bare number is not an
    /// addr.
    fn from_js_or_string(value: &JsValue) -> Result<Addr, JsValue> {
        match value.as_string() {
            Some(field) => Ok(Addr {
                card: None,
                field: Some(field),
            }),
            None => Addr::from_js(value),
        }
    }

    fn require_field(&self, ctx: &str) -> Result<&str, JsValue> {
        self.field.as_deref().ok_or_else(|| {
            WasmError::from(format!(
                "{ctx}: a body address (no `field`) is not a field write, \
                 write the body with revise / overwrite / writer.reviseBody"
            ))
            .to_js_value()
        })
    }

    /// TS types cannot police this — an `Addr` variable flows into a `CardAddr`
    /// slot — so the runtime check is the contract.
    fn require_card_only(&self, ctx: &str) -> Result<(), JsValue> {
        if self.field.is_some() {
            return Err(WasmError::from(format!(
                "{ctx}: a card address takes only `card`, not `field`"
            ))
            .to_js_value());
        }
        Ok(())
    }
}

/// Decode a JS value as a canonical `Content` object, rejecting a markdown
/// string. The **storage** lane: content read back out of a document must keep
/// opening whatever it was written as. Host-authored content goes through
/// [`js_to_authored_content`].
fn js_to_content(value: JsValue, ctx: &str) -> Result<quillmark_content::Normalized, JsValue> {
    js_to_content_with(value, ctx, quillmark_content::serial::from_canonical_value)
}

/// [`js_to_content`] on the **authored** lane. The host is writing this content
/// now, so a built-in's payload spelled as a named sibling is a stale copy of
/// the encoding, and is reported instead of read as a guess at its intent.
fn js_to_authored_content(
    value: JsValue,
    ctx: &str,
) -> Result<quillmark_content::Normalized, JsValue> {
    js_to_content_with(value, ctx, quillmark_content::serial::from_authored_value)
}

fn js_to_content_with(
    value: JsValue,
    ctx: &str,
    read: fn(
        &serde_json::Value,
    ) -> Result<quillmark_content::Normalized, quillmark_content::serial::ParseError>,
) -> Result<quillmark_content::Normalized, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    if !json.is_object() {
        return Err(WasmError::from(format!(
            "{ctx}: expected a Content content object; for markdown use importMarkdown(md) first"
        ))
        .to_js_value());
    }
    read(&json).map_err(|e| {
        WasmError::from(format!("{ctx}: not a canonical Content content: {e}")).to_js_value()
    })
}

fn parse_change_bundle(
    value: &JsValue,
    ctx: &str,
) -> Result<quillmark_content::ChangeBundle, JsValue> {
    let json = js_value_to_json(value.clone(), ctx)?;
    quillmark_content::change_bundle_from_value(&json)
        .map_err(|e| WasmError::from(format!("{ctx}: {e}")).to_js_value())
}

/// Import a markdown string to canonical `Content`: the pure, document-free
/// codec. `overwrite(addr, importMarkdown(md))` spells the cold, anchor-losing
/// write; prefer `revise` for edit semantics. Throws on an over-nested input.
#[wasm_bindgen(js_name = importMarkdown, unchecked_return_type = "Content")]
pub fn import_markdown(markdown: &str) -> Result<JsValue, JsValue> {
    let content = quillmark_content::from_markdown(markdown)
        .map_err(|e| WasmError::from(format!("importMarkdown: {e}")).to_js_value())?;
    serialize_or_throw(
        &quillmark_content::serial::to_seam_value(&content),
        "importMarkdown",
    )
}

/// Export canonical `Content` to its markdown projection. Throws if `rt` is not
/// canonical content.
#[wasm_bindgen(js_name = exportMarkdown)]
pub fn export_markdown(
    #[wasm_bindgen(unchecked_param_type = "Content")] rt: JsValue,
) -> Result<String, JsValue> {
    let content = js_to_content(rt, "exportMarkdown")?;
    Ok(quillmark_content::to_markdown(&content))
}

/// Rebase `markdown` onto a `base` content: the document-free twin of `revise`,
/// returning the new `content` and the text `delta` (offsets are USV indices into
/// `Content.text`, surviving anchors rebased). Throws on an over-nested markdown
/// input or a non-content `base`.
#[wasm_bindgen(js_name = rebase, unchecked_return_type = "{ content: Content; delta: Delta }")]
pub fn rebase(
    #[wasm_bindgen(unchecked_param_type = "Content")] base: JsValue,
    markdown: &str,
) -> Result<JsValue, JsValue> {
    let base = js_to_content(base, "rebase")?;
    let (content, delta) = quillmark_content::diff_import(&base, markdown)
        .map_err(|e| WasmError::from(format!("rebase: {e}")).to_js_value())?;
    let out = serde_json::json!({
        "content": quillmark_content::serial::to_seam_value(&content),
        "delta": serde_json::to_value(&delta).unwrap_or(serde_json::Value::Null),
    });
    serialize_or_throw(&out, "rebase")
}

#[wasm_bindgen(typescript_custom_section)]
const DOCPATH_TS: &'static str = r#"
/**
 * One segment of a parsed `Diagnostic.path` (see `parseDocPath`). The head
 * carries the document-model root: `main` (only before `body`), a `card`
 * (`kind: null` is the unknown-kind `cards[i]` form), or a `field`; the tail is
 * `field` / `index` / a terminal `body`.
 */
export type DocPathSeg =
    | { seg: "main" }
    | { seg: "card"; kind: string | null; index: number }
    | { seg: "field"; name: string }
    | { seg: "index"; index: number }
    | { seg: "body" };
"#;

#[wasm_bindgen(typescript_custom_section)]
const VALUES_TS: &'static str = r#"
/**
 * One composable card in the values form (`reader.card(i).values()`). `kind`
 * is the stored `$kind`, `null` for a kindless card; a kind the schema does
 * not declare carries its fields verbatim, there being no declared type to
 * project them through. `ext` is `null` when the card carries no `$ext` and
 * `{}` for an explicit `$ext: {}`.
 */
export interface CardValues {
    kind: string | null;
    fields: Record<string, unknown>;
    body: string;
    ext: Record<string, unknown> | null;
}

/**
 * A document in the values form (`reader.values()`): the stored value with
 * every content leaf decoded to its codec's text (`richtext` markdown,
 * `plaintext` literal) at every depth, everything else as stored, a
 * present-null as `null`. `fields` carries the declared fields the card holds
 * in declaration order, then undeclared ones verbatim in authored order.
 *
 * Sparse — an absent field is an absent key, never its `default:` (`resolve`
 * is the view that blank-fills). A projection, never a storage format:
 * markdown carries no anchors, island ids or content-only marks, and
 * `$quill`, `$seed`, `!must_fill` markers and YAML comments are not carried.
 * Persist with `toStored`.
 *
 * Every axis is present on a read, so every `DocumentValues` is a valid
 * `DocumentValuesInput`, and writing one back unedited changes no bytes.
 */
export interface DocumentValues {
    fields: Record<string, unknown>;
    body: string;
    cards: CardValues[];
    ext: Record<string, unknown> | null;
}

/**
 * `writer.card(i).setValues` input, and one entry of
 * `DocumentValuesInput.cards`. An absent axis is untouched: an absent `kind`
 * keeps the card's, an absent `fields` / `body` / `ext` leaves that axis
 * alone (empty on a card being built). `ext: null` removes `$ext`. An
 * `undefined` member reads as absent, `null` as `null`.
 */
export interface CardValuesInput {
    kind?: string | null;
    fields?: Record<string, unknown>;
    body?: string;
    ext?: Record<string, unknown> | null;
}

/**
 * `writer.setValues` input. An absent axis is untouched; a present one is
 * replaced: `fields` is the whole truth for declared names, `cards` is the
 * card list, `body` the body, `ext: null` removes `$ext` and `{}` records an
 * explicit empty one. An `undefined` member reads as absent, `null` as `null`.
 */
export interface DocumentValuesInput {
    fields?: Record<string, unknown>;
    body?: string;
    cards?: CardValuesInput[];
    ext?: Record<string, unknown> | null;
}
"#;

#[wasm_bindgen(typescript_custom_section)]
const RESOLVED_TS: &'static str = r#"
/**
 * The commitment-ladder rung that produced a `ResolvedField.value`.
 *
 * A container has no rung of its own — it is a namespace, and its value is the
 * composition of its cells' — so it reports the strongest rung that contributed:
 * `authored` if the document wrote any of it, else `default` if any cell below
 * resolved to one, else `blank`.
 */
export type FieldSource = "authored" | "default" | "blank";

/**
 * One resolved row: its `name`, the value the render projection would use, and
 * the `FieldSource` rung it came from. Rows are an ordered array, so declaration
 * order is structural rather than object-key order.
 */
export interface ResolvedField {
    name: string;
    value: unknown;
    source: FieldSource;
}

/**
 * The main card's resolved rows in declaration order, plus its body row:
 * `null` when the main enables no body.
 */
export interface ResolvedMain {
    fields: ResolvedField[];
    body: ResolvedField | null;
}

/**
 * One composable card's resolved rows in declaration order, with its authored
 * `kind` (`null` for an unknown-kind card), its document-array `index`, and its
 * body row: `null` when the kind enables no body.
 */
export interface ResolvedCard {
    kind: string | null;
    index: number;
    fields: ResolvedField[];
    body: ResolvedField | null;
}

/**
 * The resolved-value view (`Quill.resolve`): the main card and every composable
 * card. Value and provenance only; completeness stays `Quill.validate`'s.
 */
export interface Resolved {
    main: ResolvedMain;
    cards: ResolvedCard[];
}
"#;

/// Parse a canonical document-model `Diagnostic.path` (`cards.<kind>[<i>].<field>`,
/// `main.body`, `recipients[0].name`) into structured [`DocPathSeg`] segments, so
/// a consumer routes on segments instead of regexing the string. Throws on a
/// malformed path.
#[wasm_bindgen(js_name = parseDocPath, unchecked_return_type = "DocPathSeg[]")]
pub fn parse_doc_path(path: &str) -> Result<JsValue, JsValue> {
    let doc_path = path
        .parse::<quillmark_core::DocPath>()
        .map_err(|e| WasmError::from(e.to_string()).to_js_value())?;
    // Via `serde_json::Value` so the tagged segments cross as plain objects,
    // sidestepping serde-wasm-bindgen's tagged-enum handling; `missing_as_null`
    // because the `DocPathSeg` contract is `kind: string | null`.
    let json = serde_json::to_value(&doc_path)
        .map_err(|e| WasmError::from(format!("parseDocPath: {e}")).to_js_value())?;
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    json.serialize(&serializer)
        .map_err(|e| WasmError::from(format!("parseDocPath: {e}")).to_js_value())
}

/// Serialize structured [`DocPathSeg`] segments back to the canonical path
/// string: the inverse of `parseDocPath`. Throws on a segment array the
/// deserializer rejects, and on an empty one.
#[wasm_bindgen(js_name = formatDocPath)]
pub fn format_doc_path(
    #[wasm_bindgen(unchecked_param_type = "DocPathSeg[]")] segs: JsValue,
) -> Result<String, JsValue> {
    let json = js_value_to_json(segs, "formatDocPath")?;
    let doc_path: quillmark_core::DocPath = serde_json::from_value(json)
        .map_err(|e| WasmError::from(format!("formatDocPath: {e}")).to_js_value())?;
    if doc_path.segs().is_empty() {
        return Err(WasmError::from("formatDocPath: empty path").to_js_value());
    }
    Ok(doc_path.to_string())
}

/// Where `bundle`'s text-moving channels (`delta`, then `islandOps`, then
/// `lineOps`) leave `content`'s marks: the final-text coordinates the bundle's
/// `markOps` are written in, under the rebase rule stated on `ChangeBundle`.
/// The document-free read an editor diffs against to decide which `markOps` to
/// emit, rather than reproducing that rule in its own language.
///
/// `bundle.markOps` are ignored. The answer is normalized, as the store's is:
/// marks a text move drops (out of range, zero-width formatting) are absent,
/// and same-kind runs a move left adjacent arrive already unioned, so a bundle
/// carrying no `markOps` names the marks the field will hold.
/// Throws on a non-content `content`, a malformed bundle, or an op that applies
/// out of bounds: `applyChange`'s errors on the same ops.
#[wasm_bindgen(js_name = mapMarks, unchecked_return_type = "ContentMark[]")]
pub fn map_marks(
    #[wasm_bindgen(unchecked_param_type = "Content")] content: JsValue,
    #[wasm_bindgen(unchecked_param_type = "ChangeBundle")] bundle: JsValue,
) -> Result<JsValue, JsValue> {
    let content = js_to_content(content, "mapMarks")?;
    let bundle = parse_change_bundle(&bundle, "mapMarks")?;
    let marks = content
        .map_marks(&bundle)
        .map_err(|e| {
            // `applyChange`'s wording on the same op, from the same variant.
            let e = quillmark_core::EditError::ContentApply(e);
            WasmError::from(format!("mapMarks: {e}")).to_js_value()
        })?;
    let out = serde_json::Value::Array(
        marks
            .iter()
            .map(quillmark_content::serial::mark_to_value)
            .collect(),
    );
    serialize_or_throw(&out, "mapMarks")
}

/// Map a base content position (a USV index into `Content.text`, not a UTF-16
/// offset) through a `delta` to its new position, holding a caret stable across
/// a `revise`. `assoc` decides the side of a same-position insertion (`"after"`
/// moves past it). Throws on a malformed `delta`.
///
/// This maps a position the *caller* holds. For the marks already in a field,
/// `mapMarks` applies the store's own assoc rule across every channel of a
/// `ChangeBundle`.
#[wasm_bindgen(js_name = mapPos)]
pub fn map_pos(
    #[wasm_bindgen(unchecked_param_type = "Delta")] delta: JsValue,
    pos: usize,
    #[wasm_bindgen(unchecked_param_type = "Assoc")] assoc: JsValue,
) -> Result<usize, JsValue> {
    let delta: quillmark_core::Delta = serde_wasm_bindgen::from_value(delta)
        .map_err(|e| WasmError::from(format!("mapPos: invalid delta: {e}")).to_js_value())?;
    let assoc = match assoc.as_string().as_deref() {
        Some("before") => quillmark_core::Assoc::Before,
        Some("after") => quillmark_core::Assoc::After,
        _ => {
            return Err(
                WasmError::from("mapPos: assoc must be \"before\" or \"after\"").to_js_value(),
            )
        }
    };
    Ok(delta.map_pos(pos, assoc))
}

/// The `$quill` reference refusal: one code and one hint for every door that
/// parses one, `ctx` naming the door.
fn invalid_quill_reference(ctx: &str, value: &str, reason: &str) -> JsValue {
    let diag = quillmark_core::Diagnostic::new(
        quillmark_core::Severity::Error,
        format!("{}: invalid reference '{}': {}", ctx, value, reason),
    )
    .with_code("parse::invalid_quill_reference".to_string())
    .with_hint(quillmark_core::quill_ref_hint().to_string());
    WasmError {
        diagnostics: vec![diag],
    }
    .to_js_value()
}

/// Maps `EditError` to a JS `Error` carrying one diagnostic, its `DocPath`
/// anchored relative to `base` (the card root the mutator ran against).
fn edit_error_to_js(err: &quillmark_core::EditError, base: &quillmark_core::DocPath) -> JsValue {
    let mut diagnostic =
        quillmark_core::Diagnostic::new(quillmark_core::Severity::Error, err.to_string())
            .with_code(err.code().to_string())
            .with_args(err.args());
    if let Some(path) = err.doc_path(base) {
        diagnostic = diagnostic.with_path(path.to_string());
    }
    WasmError {
        diagnostics: vec![diagnostic],
    }
    .to_js_value()
}

/// Maps a `WireError` to a JS `Error` carrying one diagnostic, under the code
/// the addressed mutator onto the same violation mints. The card is not placed,
/// so the diagnostic carries no `path`.
fn wire_error_to_js(err: &quillmark_core::WireError) -> JsValue {
    WasmError {
        diagnostics: vec![err.to_diagnostic()],
    }
    .to_js_value()
}

/// Batched twin of [`edit_error_to_js`]: one diagnostic per offending field.
fn edit_errors_to_js(
    errors: Vec<(String, quillmark_core::EditError)>,
    base: &quillmark_core::DocPath,
) -> JsValue {
    let diagnostics: Vec<quillmark_core::Diagnostic> = errors
        .into_iter()
        .map(|(name, err)| {
            quillmark_core::Diagnostic::new(quillmark_core::Severity::Error, err.to_string())
                .with_code(err.code().to_string())
                .with_args(err.args())
                .with_path(base.field(&name).to_string())
        })
        .collect();
    WasmError { diagnostics }.to_js_value()
}

/// The [`edit_errors_to_js`] twin for a batch spanning cards, where each
/// refusal carries the whole [`DocPath`](quillmark_core::DocPath) it anchors at
/// rather than a field name under one base.
fn edit_errors_at_to_js(
    errors: Vec<(quillmark_core::DocPath, quillmark_core::EditError)>,
) -> JsValue {
    let diagnostics: Vec<quillmark_core::Diagnostic> = errors
        .into_iter()
        .map(|(path, err)| {
            quillmark_core::Diagnostic::new(quillmark_core::Severity::Error, err.to_string())
                .with_code(err.code().to_string())
                .with_args(err.args())
                .with_path(path.to_string())
        })
        .collect();
    WasmError { diagnostics }.to_js_value()
}

/// Key order is preserved (`preserve_order` is on workspace-wide), so field
/// insertion order follows the object's.
fn js_value_to_field_batch(
    value: &JsValue,
    ctx: &str,
) -> Result<Vec<(String, quillmark_core::QuillValue)>, JsValue> {
    match js_value_to_json(value.clone(), ctx)? {
        serde_json::Value::Object(map) => Ok(map
            .into_iter()
            .map(|(name, v)| (name, quillmark_core::QuillValue::from_json(v)))
            .collect()),
        _ => Err(WasmError::from(format!("{}: fields must be a plain object", ctx)).to_js_value()),
    }
}

/// Read a `PathStep[]` in-field path: a string is an object key, a non-negative
/// integer an array index. A malformed step throws rather than being dropped —
/// a silently skipped step reads a different address and never says so. Flat by
/// construction, so no depth guard applies.
fn path_from_js(value: &JsValue, ctx: &str) -> Result<Vec<quillmark_core::PathSegment>, JsValue> {
    if !Array::is_array(value) {
        return Err(WasmError::from(format!(
            "{ctx}: `path` must be an array of string keys and non-negative integer indices"
        ))
        .to_js_value());
    }
    Array::from(value)
        .iter()
        .enumerate()
        .map(|(i, step)| {
            if let Some(key) = step.as_string() {
                return Ok(quillmark_core::PathSegment::Key(key));
            }
            match step.as_f64() {
                Some(n) if n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 => {
                    Ok(quillmark_core::PathSegment::Index(n as usize))
                }
                _ => Err(WasmError::from(format!(
                    "{ctx}: `path[{i}]` must be a string key or a non-negative integer index"
                ))
                .to_js_value()),
            }
        })
        .collect()
}

fn js_value_to_json(value: JsValue, ctx: &str) -> Result<serde_json::Value, JsValue> {
    reject_deep_js_value(&value, ctx)?;
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| WasmError::from(format!("{}: invalid value: {}", ctx, e)).to_js_value())
}

/// Refuse a JS value nesting past [`MAX_JSON_DEPTH`] **before**
/// `serde_wasm_bindgen` builds it: `from_value` recurses one frame per level, so
/// a deep enough input overflows the 1 MB wasm32 stack during conversion, and
/// that trap takes the module down rather than surfacing as a catchable error.
/// Iterative and early-exiting, so it neither overflows on the input it detects
/// nor walks past the limit. Arrays, plain objects, and `Map`s are the three
/// containers `serde_wasm_bindgen` descends into; each is charged one level.
fn reject_deep_js_value(value: &JsValue, ctx: &str) -> Result<(), JsValue> {
    const MAX: usize = quillmark_content::MAX_JSON_DEPTH;
    let mut stack: Vec<(JsValue, usize)> = vec![(value.clone(), 0)];
    while let Some((v, depth)) = stack.pop() {
        let children = if Array::is_array(&v) {
            Array::from(&v)
        } else if v.is_instance_of::<js_sys::Map>() {
            js_sys::Array::from(&v.unchecked_into::<js_sys::Map>().values())
        } else if v.is_object() && !v.is_null() {
            js_sys::Object::values(&v.unchecked_into::<js_sys::Object>())
        } else {
            continue;
        };
        if depth + 1 > MAX {
            return Err(WasmError::from(format!(
                "{ctx}: value nests deeper than {MAX} levels"
            ))
            .to_js_value());
        }
        stack.extend(children.iter().map(|c| (c, depth + 1)));
    }
    Ok(())
}

fn js_value_to_object(
    value: &JsValue,
    ctx: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, JsValue> {
    match js_value_to_json(value.clone(), ctx)? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(WasmError::from(format!("{}: $ext must be a plain object", ctx)).to_js_value()),
    }
}

/// Throws rather than falling back to `undefined`, which reads as "property
/// absent" and crashes callers far from the cause.
fn serialize_or_throw<T: serde::Serialize + ?Sized>(
    value: &T,
    what: &str,
) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&serializer)
        .map_err(|e| WasmError::from(format!("{what}: serialization failed: {e}")).to_js_value())
}

fn json_value_to_js(value: Option<serde_json::Value>) -> Result<JsValue, JsValue> {
    match value {
        Some(v) => serialize_or_throw(&v, "ext value"),
        None => Ok(JsValue::UNDEFINED),
    }
}

fn ext_map_to_js(
    map: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<JsValue, JsValue> {
    json_value_to_js(map.map(serde_json::Value::Object))
}

fn card_to_js(card: &quillmark_core::Card) -> Result<JsValue, JsValue> {
    serialize_or_throw(&quillmark_core::CardWire::from(card), "card")
}

fn js_to_card(value: &JsValue) -> Result<quillmark_core::Card, JsValue> {
    // `serde_wasm_bindgen` does not honor `#[serde(deny_unknown_fields)]` (it
    // looks up known fields rather than visiting every key), so enforce it here:
    // a flat `{ kind, fields }` object fails loudly instead of yielding a
    // silently-empty card.
    if let Some(obj) = value.dyn_ref::<js_sys::Object>() {
        const ALLOWED: &[&str] = &[
            "kind",
            "quill",
            "ext",
            "seed",
            "payloadItems",
            "body",
        ];
        for key in js_sys::Object::keys(obj).iter() {
            if let Some(k) = key.as_string() {
                if !ALLOWED.contains(&k.as_str()) {
                    return Err(WasmError::from(format!(
                        "card has unknown field `{k}`; expected a CardInput \
                         {{ kind, payloadItems, body, … }}: build one with \
                         Document.makeCard(kind, fields, body)"
                    ))
                    .to_js_value());
                }
            }
        }
    }
    reject_deep_js_value(value, "insertCard")?;
    let wire: quillmark_core::CardWire = serde_wasm_bindgen::from_value(value.clone())
        .map_err(|e| WasmError::from(format!("card must be a Card object: {e}")).to_js_value())?;
    quillmark_core::Card::try_from(wire).map_err(|e| wire_error_to_js(&e))
}

fn file_tree_from_js_tree(tree: &JsValue) -> Result<quillmark_core::FileTreeNode, JsValue> {
    let entries = js_tree_entries(tree)?;
    let mut root = quillmark_core::FileTreeNode::Directory {
        files: HashMap::new(),
    };

    for (path, value) in entries {
        let bytes = js_bytes_for_tree_entry(&path, value)?;
        root.insert(
            path.as_str(),
            quillmark_core::FileTreeNode::File { contents: bytes },
        )
        .map_err(|e| {
            WasmError::from(format!("Invalid tree path '{}': {}", path, e)).to_js_value()
        })?;
    }

    Ok(root)
}

fn js_tree_entries(tree: &JsValue) -> Result<Vec<(String, JsValue)>, JsValue> {
    if tree.is_instance_of::<js_sys::Map>() {
        let map = tree.clone().unchecked_into::<js_sys::Map>();
        let iter = js_sys::try_iter(&map.entries())
            .map_err(|e| {
                WasmError::from(format!("Failed to iterate Map entries: {:?}", e)).to_js_value()
            })?
            .ok_or_else(|| WasmError::from("Map entries are not iterable").to_js_value())?;

        let mut entries: Vec<(String, JsValue)> = Vec::new();
        for entry in iter {
            let pair = entry.map_err(|e| {
                WasmError::from(format!("Failed to read Map entry: {:?}", e)).to_js_value()
            })?;
            let pair = Array::from(&pair);
            let path = pair
                .get(0)
                .as_string()
                .ok_or_else(|| WasmError::from("quill Map key must be a string").to_js_value())?;
            let value = pair.get(1);
            entries.push((path, value));
        }
        return Ok(entries);
    }

    if tree.is_object() && !tree.is_null() {
        let obj = tree.clone().unchecked_into::<js_sys::Object>();
        let pairs = js_sys::Object::entries(&obj);
        let mut entries: Vec<(String, JsValue)> = Vec::with_capacity(pairs.length() as usize);
        for i in 0..pairs.length() {
            let pair = Array::from(&pairs.get(i));
            let path = pair.get(0).as_string().ok_or_else(|| {
                WasmError::from("quill object key must be a string").to_js_value()
            })?;
            entries.push((path, pair.get(1)));
        }
        return Ok(entries);
    }

    Err(
        WasmError::from("quill requires a Map<string, Uint8Array> or Record<string, Uint8Array>")
            .to_js_value(),
    )
}

fn js_bytes_for_tree_entry(path: &str, value: JsValue) -> Result<Vec<u8>, JsValue> {
    if !value.is_instance_of::<Uint8Array>() {
        return Err(WasmError::from(format!(
            "Invalid tree entry '{}': expected Uint8Array value",
            path
        ))
        .to_js_value());
    }

    let bytes = value.unchecked_into::<Uint8Array>();
    Ok(bytes.to_vec())
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen(typescript_custom_section)]
const CANVAS_PREVIEW_TS: &'static str = r#"
/**
 * Page dimensions in points (1 pt = 1/72 inch). Report-only: the painter sizes
 * the canvas itself from `PaintOptions`. `pageSize` is for callers that need
 * page geometry up-front, e.g. to lay out a scrollable list of canvases.
 */
export interface PageSize {
    widthPt: number;
    heightPt: number;
}

/**
 * Inputs to `LiveSession.paint`. Both default to `1`, must be finite and `> 0`,
 * and multiply to the effective rasterization scale.
 *
 * - `layoutScale`: layout-space pixels per point — CSS pixels per pt for an
 *   on-screen canvas — surfaced back as `layoutWidth` / `layoutHeight`.
 * - `densityScale`: backing-store density. Fold `window.devicePixelRatio`,
 *   in-app zoom, and `visualViewport.scale` into this one value; the default
 *   `1` produces a non-retina backing store.
 */
export interface PaintOptions {
    layoutScale?: number;
    densityScale?: number;
}

/**
 * Returned by `LiveSession.paint`.
 *
 * - `layoutWidth` / `layoutHeight`: the display box, in CSS pixels for an
 *   on-screen canvas, to drive `canvas.style.*`. Independent of `densityScale`.
 * - `pixelWidth` / `pixelHeight`: the backing store the painter wrote to
 *   `canvas.width` / `canvas.height`, `round(layout * densityScale)` unless the
 *   request exceeded 16384 px per side and `densityScale` was clamped to fit.
 * - `clamped`: `true` when that clamp fired, so the page renders soft at the
 *   same `canvas.style` size.
 * - `effectiveDensityScale`: the `densityScale` actually applied.
 *
 * The painter owns `canvas.width` / `canvas.height` and never touches
 * `canvas.style.*`. The write is a whole-backing-store `putImageData`, which
 * bypasses the 2D context transform, `globalAlpha`, and clip: give each visible
 * page its own `<canvas>`, since no compositing, sub-rect, or transform reaches
 * through `paint`. Under `OffscreenCanvasRenderingContext2D` the layout
 * dimensions are informational — there is no CSS box to apply them to.
 */
export interface PaintResult {
    layoutWidth: number;
    layoutHeight: number;
    pixelWidth: number;
    pixelHeight: number;
    clamped: boolean;
    effectiveDensityScale: number;
}
"#;

/// A backend plate-space geometry address → its `DocPath` string, keeping the
/// original when it does not fit the geometry grammar.
#[cfg(any(feature = "typst", feature = "pdfform"))]
fn plate_to_docpath(addr: &str, kinds: &[Option<&str>]) -> String {
    quillmark_core::plate_addr_to_doc_path(addr, kinds)
        .map(|p| p.to_string())
        .unwrap_or_else(|| addr.to_string())
}

/// A `DocPath` address (from a consumer) → its plate-space form for the backend,
/// keeping the original when it does not parse or place.
#[cfg(any(feature = "typst", feature = "pdfform"))]
fn docpath_to_plate(addr: &str, kinds: &[Option<&str>]) -> String {
    addr.parse::<quillmark_core::DocPath>()
        .ok()
        .and_then(|p| quillmark_core::doc_path_to_plate_addr(&p, kinds))
        .unwrap_or_else(|| addr.to_string())
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl LiveSession {
    /// Built once per query, never per region.
    fn kinds(&self) -> Vec<Option<&str>> {
        self.card_kinds.iter().map(|k| k.as_deref()).collect()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[wasm_bindgen]
impl LiveSession {
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// The backend that produced this session (e.g. `"typst"`).
    #[wasm_bindgen(getter, js_name = backendId)]
    pub fn backend_id(&self) -> String {
        self.backend_id.clone()
    }

    /// `true` iff `paint` and `pageSize` will succeed for this session.
    #[wasm_bindgen(getter, js_name = supportsCanvas)]
    pub fn supports_canvas(&self) -> bool {
        self.inner.supports_canvas()
    }

    /// Non-fatal diagnostics of the session's **current compile**, refreshed by
    /// each committed `apply`; a failed apply keeps the last-good compile's.
    /// Also appended to `RenderResult.warnings` on each `render()`.
    #[wasm_bindgen(getter, js_name = warnings, unchecked_return_type = "Diagnostic[]")]
    pub fn warnings(&self) -> Result<JsValue, JsValue> {
        let diags: Vec<Diagnostic> = self
            .inner
            .warnings()
            .iter()
            .cloned()
            .map(Into::into)
            .collect();
        serialize_or_throw(&diags, "warnings")
    }

    /// Recompile the session against `doc`: the edit verb of a live preview. The
    /// document compiles through the same pipeline as `open`, then swaps in
    /// transactionally — on throw every read keeps serving the last-good compile
    /// and the session recovers on the next successful `update`. On success,
    /// repaint `dirtyPages ∩ visible`.
    ///
    /// Distinct from [`applyChange`](Document::apply_change), which splices ops
    /// into a document; this recompiles a document the caller already mutated.
    #[wasm_bindgen(js_name = update)]
    pub fn update(&mut self, doc: &Document) -> Result<ChangeSet, JsValue> {
        let cs = self
            .inner
            .update(&doc.inner)
            .map_err(|e| WasmError::from(e).to_js_value())?;
        self.card_kinds = card_kinds_of(&doc.inner);
        Ok(ChangeSet {
            page_count: cs.page_count,
            dirty_pages: cs.dirty_pages,
        })
    }

    #[wasm_bindgen(js_name = render)]
    pub fn render(&self, opts: Option<RenderOptions>) -> Result<RenderResult, JsValue> {
        let start = now_ms();
        let rust_opts: quillmark_core::RenderOptions = opts.unwrap_or_default().into();

        let result = self
            .inner
            .render(&rust_opts)
            .map_err(|e| WasmError::from(e).to_js_value())?;

        Ok(RenderResult {
            artifacts: result.artifacts.into_iter().map(Into::into).collect(),
            warnings: result.warnings.into_iter().map(Into::into).collect(),
            output_format: result.output_format.into(),
            render_time_ms: now_ms() - start,
            regions: quillmark_core::regions_to_doc_path(result.regions, &self.kinds())
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    /// Schema-field geometry for this compiled session: each content field's
    /// **first placement** (one region per page it touches) plus widget and
    /// scalar-reference-site regions, keyed on the canonical `DocPath` address. A
    /// field may appear more than once, so group by `field` (see `FieldRegion`).
    /// A session-level query: no render, no byte artifact. The click direction is
    /// `fieldAt`. Empty for backends that place no schema fields.
    #[wasm_bindgen(js_name = regions, unchecked_return_type = "FieldRegion[]")]
    pub fn regions(&self) -> Result<JsValue, JsValue> {
        let regions: Vec<FieldRegion> = quillmark_core::regions_to_doc_path(self.inner.regions(), &self.kinds())
            .into_iter()
            .map(Into::into)
            .collect();
        serialize_or_throw(&regions, "regions")
    }

    /// The whole-field highlight boxes for `field`: one union rect per page over
    /// the field's `span`-bearing content segments, the union `regions()` leaves
    /// derived. **Content only**: a field placed solely as a scalar reference or
    /// a bound widget carries no `span` and returns `[]`, its box being a single
    /// `regions()` rect. Reflects the current compile.
    #[wasm_bindgen(js_name = fieldBoxes, unchecked_return_type = "FieldRegion[]")]
    pub fn field_boxes(&self, field: &str) -> Result<JsValue, JsValue> {
        let kinds = self.kinds();
        let plate = docpath_to_plate(field, &kinds);
        let boxes: Vec<FieldRegion> = quillmark_core::regions_to_doc_path(self.inner.field_boxes(&plate), &kinds)
            .into_iter()
            .map(Into::into)
            .collect();
        serialize_or_throw(&boxes, "fieldBoxes")
    }

    /// The schema field whose content is under a point on `page`: the `DocPath`
    /// address to focus in the editor, or `undefined` off any field's ink.
    /// `x`/`y` are PDF points with a **bottom-left** origin, the same space as
    /// `FieldRegion.rect`, so from a canvas click use
    /// `x = clickPx.x / renderScale`, `y = pageHeightPt - clickPx.y / renderScale`.
    /// Unlike `regions()`, *every* placement answers, not just the first.
    ///
    /// `tolPt` is how far off the ink a click still counts, in the same points,
    /// and defaults to `0` — exact. Convert the pointer slack a surface wants
    /// from CSS pixels at the scale it drew the page (`slackPx / renderScale`),
    /// so it stays the same size under the cursor as the page zooms. The
    /// nearest placement answers, so raising it only fills a miss.
    #[wasm_bindgen(js_name = fieldAt)]
    pub fn field_at(&self, page: usize, x: f32, y: f32, tol_pt: Option<f32>) -> Option<String> {
        self.inner
            .field_at(page, x, y, tol_pt.unwrap_or(0.0))
            .map(|f| plate_to_docpath(&f, &self.kinds()))
    }

    /// A point → **content position**: the field *and* a USV offset into its
    /// `Content`, for placing a caret or mapping a selection into the content
    /// model, or `undefined` off all content ink. `x`/`y`/`tolPt` are PDF
    /// points, bottom-left origin, as in `fieldAt`. The offset is cluster-exact
    /// and degrades to the containing segment's start on origin-less ink.
    ///
    /// `tolPt` earns the most here: the leading between two lines lies inside a
    /// paragraph and on no glyph, and under `tolPt` such a point takes the
    /// nearer line.
    #[wasm_bindgen(js_name = positionAt)]
    pub fn position_at(
        &self,
        page: usize,
        x: f32,
        y: f32,
        tol_pt: Option<f32>,
    ) -> Option<ContentHit> {
        self.inner
            .position_at(page, x, y, tol_pt.unwrap_or(0.0))
            .map(|mut hit| {
                hit.field = plate_to_docpath(&hit.field, &self.kinds());
                hit.into()
            })
    }

    /// A content position → **caret rect**, the reverse of `positionAt`: the box
    /// to draw a caret at, in the same bottom-left PDF-point space as
    /// `FieldRegion.rect`, its `span` collapsed to `[pos, pos]`. `undefined` when
    /// the field places no tracked content or the offset maps to no drawn glyph.
    #[wasm_bindgen(js_name = locate)]
    pub fn locate(&self, field: &str, pos: usize) -> Option<FieldRegion> {
        let kinds = self.kinds();
        let plate = docpath_to_plate(field, &kinds);
        self.inner.locate(&plate, pos).map(|mut r| {
            r.field = plate_to_docpath(&r.field, &kinds);
            r.into()
        })
    }

    /// Page dimensions in points (1 pt = 1/72 inch).
    /// Throws if the backend has no canvas painter or `page` is out of range.
    #[wasm_bindgen(js_name = pageSize, unchecked_return_type = "PageSize")]
    pub fn page_size(&self, page: usize) -> Result<JsValue, JsValue> {
        self.ensure_canvas("pageSize")?;
        let (width_pt, height_pt) = self
            .inner
            .page_size_pt(page)
            .ok_or_else(|| self.page_oob_error("pageSize", page))?;
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        PageSize {
            width_pt,
            height_pt,
        }
        .serialize(&serializer)
        .map_err(|e| WasmError::from(format!("pageSize: serialization failed: {e}")).to_js_value())
    }

    /// Paint `page` into a `CanvasRenderingContext2D` or
    /// `OffscreenCanvasRenderingContext2D`. The painter owns
    /// `canvas.width`/`height` (no `clearRect` needed); consumers own
    /// `canvas.style.*`. If `layoutScale * densityScale` exceeds 16384 px per
    /// side, `densityScale` is clamped and `PaintResult` reports it.
    ///
    /// `put_image_data` writes the whole backing store, bypassing the 2D
    /// context's transform, `globalAlpha`, and clip, so each visible page needs
    /// its own `<canvas>`: no compositing, sub-rect, or transform reaches through
    /// this call.
    ///
    /// Throws if the backend has no canvas painter, `page` is out of range, `ctx`
    /// is the wrong type, or either scale is non-finite or `<= 0`.
    #[wasm_bindgen(js_name = paint, unchecked_return_type = "PaintResult")]
    pub fn paint(
        &self,
        #[wasm_bindgen(
            unchecked_param_type = "CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D"
        )]
        ctx: JsValue,
        page: usize,
        #[wasm_bindgen(unchecked_param_type = "PaintOptions | undefined")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.ensure_canvas("paint")?;
        let canvas_ctx = CanvasCtx::from_js(&ctx)?;

        let (width_pt, height_pt) = self
            .inner
            .page_size_pt(page)
            .ok_or_else(|| self.page_oob_error("paint", page))?;

        let opts: PaintOptions = if opts.is_undefined() || opts.is_null() {
            PaintOptions::default()
        } else {
            serde_wasm_bindgen::from_value(opts).map_err(|e| {
                WasmError::from(format!("paint: invalid options: {e}")).to_js_value()
            })?
        };

        let layout_scale = opts.layout_scale.unwrap_or(1.0);
        let requested_density = opts.density_scale.unwrap_or(1.0);

        if !layout_scale.is_finite() || layout_scale <= 0.0 {
            return Err(WasmError::from(
                "paint: layoutScale must be a finite number greater than 0",
            )
            .to_js_value());
        }
        if !requested_density.is_finite() || requested_density <= 0.0 {
            return Err(WasmError::from(
                "paint: densityScale must be a finite number greater than 0",
            )
            .to_js_value());
        }

        let layout_width = (width_pt as f64) * (layout_scale as f64);
        let layout_height = (height_pt as f64) * (layout_scale as f64);

        let desired_w = (layout_width * requested_density as f64).round();
        let desired_h = (layout_height * requested_density as f64).round();
        let max_dim = desired_w.max(desired_h);

        let clamped = max_dim > MAX_BACKING_DIMENSION as f64;
        let effective_density = if clamped {
            (requested_density as f64) * (MAX_BACKING_DIMENSION as f64 / max_dim)
        } else {
            requested_density as f64
        };

        let render_scale = (layout_scale as f64) * effective_density;
        // Each factor is validated finite and positive, but the product (or the
        // f64->f32 cast) can still overflow: a zero-dimension page bypasses the
        // MAX_BACKING_DIMENSION clamp.
        if !render_scale.is_finite() || render_scale <= 0.0 || render_scale > f32::MAX as f64 {
            return Err(WasmError::from(
                "paint: computed render scale is non-finite or out of range",
            )
            .to_js_value());
        }

        // `page_size_pt(page)` succeeded above, so a `None` here is a backend
        // capability/impl disagreement, not a bad page index.
        let (pixel_w, pixel_h, mut rgba) = self
            .inner
            .render_rgba(page, render_scale as f32)
            .ok_or_else(|| {
                WasmError::from(format!(
                    "paint: backend '{}' reported a canvas painter but produced no raster \
                     for page {page} (render_rgba returned None on an in-range page)",
                    self.backend_id
                ))
                .to_js_value()
            })?;

        canvas_ctx.set_canvas_dims(pixel_w, pixel_h)?;

        let img = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(rgba.as_mut_slice()),
            pixel_w,
            pixel_h,
        )
        .map_err(|e| {
            WasmError::from(format!("paint: ImageData construction failed: {:?}", e)).to_js_value()
        })?;
        canvas_ctx.put_image_data(&img)?;

        let result = PaintResult {
            layout_width,
            layout_height,
            pixel_width: pixel_w,
            pixel_height: pixel_h,
            clamped,
            effective_density_scale: effective_density,
        };
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        result
            .serialize(&serializer)
            .map_err(|e| WasmError::from(format!("paint: serialization failed: {e}")).to_js_value())
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl LiveSession {
    fn ensure_canvas(&self, op: &str) -> Result<(), JsValue> {
        if self.inner.supports_canvas() {
            Ok(())
        } else {
            Err(WasmError::from(format!(
                "{op}: backend '{}' has no canvas painter",
                self.backend_id
            ))
            .to_js_value())
        }
    }

    fn page_oob_error(&self, op: &str, page: usize) -> JsValue {
        WasmError::from(format!(
            "{op}: page index {page} out of range (pageCount={})",
            self.inner.page_count()
        ))
        .to_js_value()
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
enum CanvasCtx<'a> {
    OnScreen(&'a web_sys::CanvasRenderingContext2d),
    OffScreen(&'a web_sys::OffscreenCanvasRenderingContext2d),
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
impl<'a> CanvasCtx<'a> {
    fn from_js(ctx: &'a JsValue) -> Result<Self, JsValue> {
        if let Some(c) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>() {
            return Ok(Self::OnScreen(c));
        }
        if let Some(c) = ctx.dyn_ref::<web_sys::OffscreenCanvasRenderingContext2d>() {
            return Ok(Self::OffScreen(c));
        }
        Err(WasmError::from(
            "paint: ctx must be CanvasRenderingContext2D or OffscreenCanvasRenderingContext2D",
        )
        .to_js_value())
    }

    fn set_canvas_dims(&self, width: u32, height: u32) -> Result<(), JsValue> {
        match self {
            Self::OnScreen(c) => {
                let canvas = c.canvas().ok_or_else(|| {
                    WasmError::from("paint: rendering context has no associated <canvas> element")
                        .to_js_value()
                })?;
                canvas.set_width(width);
                canvas.set_height(height);
            }
            Self::OffScreen(c) => {
                let canvas = c.canvas();
                canvas.set_width(width);
                canvas.set_height(height);
            }
        }
        Ok(())
    }

    fn put_image_data(&self, img: &web_sys::ImageData) -> Result<(), JsValue> {
        match self {
            Self::OnScreen(c) => c.put_image_data(img, 0.0, 0.0),
            Self::OffScreen(c) => c.put_image_data(img, 0.0, 0.0),
        }
    }
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Serialize)]
struct PageSize {
    #[serde(rename = "widthPt")]
    width_pt: f32,
    #[serde(rename = "heightPt")]
    height_pt: f32,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaintOptions {
    #[serde(default)]
    layout_scale: Option<f32>,
    #[serde(default)]
    density_scale: Option<f32>,
}

#[cfg(any(feature = "typst", feature = "pdfform"))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaintResult {
    layout_width: f64,
    layout_height: f64,
    pixel_width: u32,
    pixel_height: u32,
    clamped: bool,
    effective_density_scale: f64,
}
