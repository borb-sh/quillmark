//! The content model across the JS boundary: `Content` and `Change` values
//! with offsets in UTF-16 code units, the write primitive `apply` and its
//! producers, and the pure helpers over changes.

use super::{
    Addr, Document, edit_error_to_js, index_out_of_range, js_value_to_json, reject_unknown_keys,
    serialize_or_throw,
};
use crate::error::WasmError;
use crate::types::Diagnostic;
use quillmark_content::change::{self, Assoc, Change};
use quillmark_content::structural::{self, StructuralEdit, Wrap};
use quillmark_content::model::Content;
use quillmark_content::units::{self, Utf16};
use quillmark_content::wire::ParseError;
use quillmark_core::document::{Card, CardMut, Edit, EditError, Receipt, Target, WriteOptions, check_change, check_content_value};
use quillmark_core::path::DocPath;
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const CONTENT_TS: &'static str = r#"
/**
 * One content value: a card body, a `richtext` field, or a `text` field,
 * which `codec: "plain"` marks and which holds only paragraphs in no
 * container, with no mark and no island, read and written as its literal
 * text. Flat `text` where a
 * `\n` opens a block, `U+2028` is a hard break and `U+FFFC` is an island's
 * character; one block record per `\n`-separated segment, each `\n` carrying the
 * record of the block it opens and `blocks[0]` the first block's; containers
 * the blocks' paths name; formatting marks; comment anchors; and islands, a
 * table's cells and a footnote's note each a nested `Content` in its `slots`.
 * Every part carries an id. Offsets are UTF-16 code units, JavaScript string
 * indices, into the `text` of the value holding them.
 */
export interface Content {
    text: string;
    blocks: ContentBlock[];
    containers?: ContentContainer[];
    marks?: ContentMark[];
    anchors?: ContentAnchor[];
    islands?: ContentIsland[];
    codec?: "plain";
}

/** A block's record: its kind and attributes, and its container path,
 *  outermost first. A kind, mark type or island type spelled `x-` and more
 *  is one this build does not know, held as written: a block reads as a
 *  paragraph, a mark formats nothing, an island renders as nothing. */
export type ContentBlock = { id: string; path?: string[] } & ContentBlockKind;

export type ContentBlockKind =
    | { kind: "para" }
    | { kind: "heading"; attrs: { level: number } }
    | { kind: "code"; attrs?: { lang?: string } }
    | { kind: "rule" }
    | { kind: `x-${string}`; attrs?: Record<string, unknown> };

/** A list, a list item or a quote. An item's number is computed, never stored. */
export type ContentContainer = { id: string } & (
    | { kind: "list"; attrs: { ordered: boolean; start: number } }
    | { kind: "item"; attrs?: { checked?: boolean } }
    | { kind: "quote" }
);

/** Formatting over `[start, end)`. Same-kind marks union; `link` and
 *  `font_family` hold one value per character. */
export type ContentMark = { start: number; end: number } & ContentMarkKind;

export type ContentMarkKind =
    | { type: "strong" | "emph" | "underline" | "strike" | "code" }
    | { type: "link"; attrs: { url: string } }
    | { type: "font_family"; attrs: { name: string } }
    | { type: `x-${string}`; attrs?: Record<string, unknown> };

/** A comment handle over `[start, end)` with a host-minted id; a point when
 *  the two are equal. */
export interface ContentAnchor {
    id: string;
    start: number;
    end: number;
}

/** `props` of a `table` island. `columns` are column ids; each row maps a
 *  column id to the slot holding its cell. `rows[0]` is the header row, and
 *  `head` counts header rows from the top, absent at 1. `aligns` and `widths`
 *  are keyed by column id, `cell_attrs` by slot id. */
export interface TableProps {
    columns: string[];
    rows: { id: string; cells: Record<string, string> }[];
    head?: number;
    aligns?: Record<string, "left" | "center" | "right">;
    widths?: Record<string, number>;
    align?: "left" | "center" | "right";
    cell_attrs?: Record<string, {
        align?: "left" | "center" | "right";
        valign?: "top" | "middle" | "bottom";
        colspan?: number;
        rowspan?: number;
    }>;
}

/** An island: the object one `U+FFFC` of `text` stands for. A `table`'s cells
 *  and a `footnote`'s `note` are its `slots`. */
export type ContentIsland = { id: string; slots?: Record<string, Content> } & (
    | { type: "table"; props: TableProps }
    | { type: "image"; props: { url: string; alt: string } }
    | { type: "footnote"; props: Record<string, never> }
    | { type: "math"; props: { tex: string; display?: true } }
    | { type: `x-${string}`; props: Record<string, unknown> }
);

/**
 * An edit: a list of steps, each reading the value the step before it left.
 * `apply` runs them all or none.
 */
export interface Change {
    steps: Step[];
}

/**
 * One step, an object of one key naming it.
 *
 * - `splice` replaces `delete`, which must be the text at `at`, with `insert`.
 *   `marks` is the formatting the inserted text carries, offsets into
 *   `insert`, a mark without `start`/`end` covering all of it; `blocks` the
 *   record of each block an inserted `\n` opens; `islands` the island of each
 *   inserted `U+FFFC`. `removed` is what the deleted text held, filled in on
 *   the receipt's `applied`.
 * - `setBlock` changes a block's kind, attributes or path.
 * - `container` adds a container or sets an existing one's attributes, or
 *   removes one no path names.
 * - `mark` adds or removes formatting over a range.
 * - `anchor` places a comment anchor under an id the value holds nowhere,
 *   moves the one its text holds (`move: true`), or removes one.
 * - `island` replaces an island's props, or inserts an island's character at `at`.
 * - `nested` applies a change inside one slot of an island.
 *
 * A record a step puts, a block a splice opens, a container it sets or an
 * island it inserts, may carry a placeholder id, `?` and an id (`"?1"`), and a
 * step may name it, as a `setBlock` path or a `nested` island does: the write
 * mints an id for it past every id the value holds, and the receipt's `minted`
 * names it.
 *
 * A placement of a held id, and a move or removal of one the text does not
 * hold, is refused (`edit::content_apply`).
 *
 * A `mark` step and an `anchor` placement or move take `quote`, the text their range
 * covered when the writer read it, and an island insert takes `block`, the id
 * of the block the writer read `at` in. A step whose `quote` or `block` the
 * value no longer holds is refused as stale (`edit::content_apply`), as a
 * splice is whose `delete` the value no longer holds. The receipt's `applied`
 * fills both in.
 */
export type Step =
    | { splice: { at: number; delete?: string; insert: string;
          marks?: ({ start?: number; end?: number } & ContentMarkKind)[];
          blocks?: ContentBlock[]; islands?: ContentIsland[];
          removed?: { blocks?: ContentBlock[]; marks?: ContentMark[]; islands?: ContentIsland[] } } }
    | { setBlock: { id: string; kind?: ContentBlockKind["kind"]; attrs?: Record<string, unknown>; path?: string[] } }
    | { container: ContentContainer | { id: string; remove: true } }
    | { mark: { start: number; end: number; add: boolean; quote?: string } & ContentMarkKind }
    | { anchor: (ContentAnchor & { quote?: string; move?: true }) | { id: string; remove: true } }
    | { island: { id: string; props: Record<string, unknown>; slots?: Record<string, Content> } | { at: number; island: ContentIsland; block?: string } }
    | { nested: { island: string; slot: string; change: Change } };

/**
 * What a write did. `applied` is the change in its normalized form, which
 * `invert(applied, before)` undoes; `hash` is the value's new hash, the next
 * write's `base`. `droppedAnchors` names each anchor the value held and holds
 * nowhere now, as it read before: every one a `revise` or `replace` lost, and
 * for an `apply` every one but those its `anchor` steps removed.
 * `movedAnchors` names each surviving one the write moved, a point included,
 * with the text it covered before and after and `reason`, the first of these
 * that holds: `position`, an anchor step put it other than where the write's
 * text edits carry it; `block`, its start sits in another block; `context`,
 * its text or the eight characters either side read otherwise.
 * `warnings` holds a `parse::dropped_construct` per construct a revise's
 * markdown dropped.
 * `minted` maps each
 * placeholder the change gave a record it put to the id the write minted for
 * it; a structural edit's own are `"?0"`, `"?1"` and on, in the order it puts them.
 */
export interface Receipt {
    applied: Change;
    hash: string;
    droppedAnchors: DroppedAnchor[];
    movedAnchors: MovedAnchor[];
    warnings: Diagnostic[];
    minted: Record<string, string>;
}

/** A surviving anchor a write moved, and what moved. */
export interface MovedAnchor {
    id: string;
    before: string;
    after: string;
    reason: "position" | "block" | "context";
}

/**
 * A receipt's dropped anchor: its selector, which `reattach` takes as given,
 * and the `island` and `slot` that held it, absent for an anchor in the
 * value's own text.
 */
export interface DroppedAnchor extends AnchorContext {
    island?: string;
    slot?: string;
}

/** `base` is the hash a writer read. A value that hashes otherwise refuses
 *  the write with `edit::stale_base`, its `args.current` the hash now. Pass it:
 *  a write without one lands over whatever the value now holds. */
export interface WriteOptions {
    base?: string;
}

/** `splitBlock`'s options: a write's, and `id`, the new block's id, which
 *  the write mints where absent. */
export interface SplitBlockOptions extends WriteOptions {
    id?: string;
}

/** A range of a value's text, UTF-16 offsets: `diffRange`'s `from` and `to`
 *  are into `before`. */
export interface TextRange {
    from: number;
    to: number;
}

/** What `blockAt` answers: the block holding an offset, its index among the
 *  value's blocks, and its container path, outermost first. */
export interface Located {
    id: string;
    index: number;
    path: string[];
}

/** A position in a value: an offset into its text, or one inside a
 *  slot of an island. */
export type Pos = number | { island: string; slot: string; at: Pos };

/**
 * Where `mapPos` carried a position, and what around it the change deleted:
 * the character on its `assoc` side (`deleted`), the one before, the one
 * after, or both in one deletion (`deletedAcross`).
 */
export interface MapResult {
    pos: Pos;
    deleted: boolean;
    deletedBefore: boolean;
    deletedAfter: boolean;
    deletedAcross: boolean;
}

/** What `wrap` opens around the blocks it wraps: a quote, or a list with an
 *  item per wrapped child, each `checked` as given. */
export type WrapKind = { kind: "quote" } | { kind: "list"; ordered: boolean; checked?: boolean };

/**
 * A value as a merge of concurrent edits left it, for `repair`: shaped as
 * `Content`, its invariants unchecked. `islands` is an array paired with the
 * slots in order, or an object keyed by island id with `slot_ids` naming the
 * island each `U+FFFC` carries, `null` for none. An anchor placed as a range
 * carries `range: true`, since one collapsed to zero width reads as a point
 * otherwise. An offset inside a character moves to its start, and one past
 * the text to its end.
 */
export interface RawContent {
    text: string;
    blocks: ContentBlock[];
    containers?: ContentContainer[];
    marks?: ContentMark[];
    anchors?: (ContentAnchor & { range?: boolean })[];
    islands?: (ContentIsland & { slots?: Record<string, RawContent> })[]
        | Record<string, Omit<ContentIsland, "id"> & { slots?: Record<string, RawContent> }>;
    slot_ids?: (string | null)[];
    codec?: "plain";
}

/**
 * One mend `repair` made. `inserts` marks one whose write-back inserts text
 * or an island's slot, so that writing it back twice lands it twice; a mend
 * without it writes back the same however often it is written.
 */
export type Repair = { inserts?: true } & (
| { repair: "missing_container"; block: string; container: string }
| { repair: "two_parents"; container: string }
| { repair: "split_run"; container: string; fresh: string }
| { repair: "orphan_slot"; island: string | null }
| { repair: "orphan_island"; island: string }
| { repair: "shared_island"; island: string; fresh: string }
| { repair: "split_block"; island: string; block: string }
| { repair: "empty_cell"; island: string; slot: string }
| { repair: "collapsed_anchor"; id: string }
| { repair: "invalid"; reason: string }
| { repair: "in_slot"; island: string; slot: string; inner: Repair }
);

/** What `repair` read a merged value as: the valid `content`, each mend,
 *  and `change`, the change from the merged text to the content's, which
 *  `mapPos` carries a position through to where repair puts an anchor
 *  there; no mend and no step for a value already valid. */
export interface Repaired {
    content: Content;
    repairs: Repair[];
    change: Change;
}

"#;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReceiptJs {
    applied: Value,
    hash: String,
    dropped_anchors: Vec<DroppedAnchorJs>,
    moved_anchors: Vec<MovedJs>,
    warnings: Vec<Diagnostic>,
    minted: std::collections::BTreeMap<String, String>,
}

/// A receipt's dropped anchor: its selector, and the island and slot that
/// held it, absent for one in the value's own text.
#[derive(serde::Serialize)]
struct DroppedAnchorJs {
    #[serde(flatten)]
    selector: super::AnchorContextJs,
    #[serde(skip_serializing_if = "Option::is_none")]
    island: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slot: Option<String>,
}

#[derive(serde::Serialize)]
struct MovedJs {
    id: String,
    before: String,
    after: String,
    reason: &'static str,
}

/// A `WriteOptions` from JS: the `base` the writer read.
struct WriteOpts {
    base: Option<String>,
}

impl WriteOpts {
    /// The options of `verb`, which also reads the keys `extra` names.
    fn from_js(opts: &JsValue, verb: &str, extra: &[&str]) -> Result<WriteOpts, JsValue> {
        let mut out = WriteOpts { base: None };
        if opts.is_undefined() || opts.is_null() {
            return Ok(out);
        }
        let invalid = |e: String| WasmError::from(format!("{verb}: {e}")).to_js_value();
        let keys: Vec<&str> = ["base", "baseHash"].into_iter().chain(extra.iter().copied()).collect();
        reject_unknown_keys(opts, &keys, "options", |k| {
            format!("{verb}: options have unknown key `{k}`; a write takes {}", keys.iter().filter(|k| **k != "baseHash").map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", "))
        })?;
        for key in ["base", "baseHash"] {
            if !js_sys::Reflect::has(opts, &key.into()).unwrap_or(false) {
                continue;
            }
            let v = js_sys::Reflect::get(opts, &key.into()).unwrap_or(JsValue::UNDEFINED);
            // A key present with no string is a caller bug (a lookup that found
            // nothing, a missing `await`), and would drop into an unguarded write.
            let Some(hash) = v.as_string() else {
                return Err(invalid(format!("{key} must be a string when given")));
            };
            if out.base.replace(hash).is_some() {
                return Err(invalid("name base or baseHash, not both".into()));
            }
        }
        Ok(out)
    }

    fn as_ref(&self) -> WriteOptions<'_> {
        WriteOptions { base: self.base.as_deref() }
    }
}

fn target(addr: &Addr) -> Target<'_> {
    Target::from(addr.field.as_deref())
}

fn card_base(doc: &quillmark_core::document::Document, addr: &Addr) -> DocPath {
    match addr.card {
        None => DocPath::main(),
        Some(index) => DocPath::card(doc.cards().get(index).and_then(|c| c.kind()), index),
    }
}

fn card_of<'a>(doc: &'a quillmark_core::document::Document, addr: &Addr) -> Result<&'a Card, JsValue> {
    match addr.card {
        None => Ok(doc.main()),
        Some(index) => doc.cards().get(index).ok_or_else(|| index_out_of_range(index, doc.cards().len())),
    }
}

fn card_mut_of<'a>(doc: &'a mut quillmark_core::document::Document, addr: &Addr) -> Result<CardMut<'a>, JsValue> {
    let len = doc.cards().len();
    match addr.card {
        None => Ok(doc.main_mut()),
        Some(index) => doc.card_mut(index).ok_or_else(|| index_out_of_range(index, len)),
    }
}

fn value_path(base: &DocPath, addr: &Addr) -> DocPath {
    match &addr.field {
        None => base.body(),
        Some(field) => base.field(field),
    }
}

fn apply_error(e: change::ApplyError) -> JsValue {
    bound_error(EditError::change(e))
}

fn bound_error(e: EditError) -> JsValue {
    WasmError { diagnostics: vec![e.to_diagnostic(None)] }.to_js_value()
}

/// A `Content` from JS, offsets in UTF-16 code units.
pub(super) fn content_in(value: JsValue, ctx: &str) -> Result<Content, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    if !json.is_object() {
        return Err(WasmError::from(format!(
            "{ctx}: expected a Content object; for markdown use importMarkdown(md).content first"
        ))
        .to_js_value());
    }
    check_content_value(&json).map_err(bound_error)?;
    Content::from_value_in(&json, Utf16).map_err(|e| match &e {
        ParseError::Invalid(inv) if EditError::bound(inv).is_some() => bound_error(EditError::bound(inv).expect("a bound")),
        _ => WasmError::from(format!("{ctx}: not a Content: {e}")).to_js_value(),
    })
}

/// [`content_in`] for a value a producer hands to be normalized: read as a
/// stored value is, its canonical form.
pub(super) fn content_normalized(value: JsValue, ctx: &str) -> Result<Content, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    if !json.is_object() {
        return Err(WasmError::from(format!("{ctx}: expected a Content object")).to_js_value());
    }
    check_content_value(&json).map_err(bound_error)?;
    Content::from_value_lenient_in(&json, Utf16).map(|read| read.content).map_err(|e| match &e {
        ParseError::Invalid(inv) if EditError::bound(inv).is_some() => bound_error(EditError::bound(inv).expect("a bound")),
        _ => WasmError::from(format!("{ctx}: not a Content: {e}")).to_js_value(),
    })
}

/// `c` for JS, offsets in UTF-16 code units.
pub(super) fn content_out(c: &Content, ctx: &str) -> Result<JsValue, JsValue> {
    serialize_or_throw(&c.to_value_in(Utf16), ctx)
}

/// A change from JS, still in UTF-16 code units.
fn change_in(value: JsValue, ctx: &str) -> Result<Change, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    let change = Change::from_value_in(&json, Utf16).map_err(|e| WasmError::from(format!("{ctx}: not a Change: {e}")).to_js_value())?;
    check_change(&change).map_err(bound_error)?;
    Ok(change)
}

/// A stored payload value with each content value in it, at any depth, moved
/// to UTF-16 code units.
pub(super) fn payload_out(v: &mut Value) {
    if units::is_content_value(v) {
        units::value_from_scalars(v, Utf16);
        return;
    }
    match v {
        Value::Array(items) => items.iter_mut().for_each(payload_out),
        Value::Object(m) => m.values_mut().for_each(payload_out),
        _ => {}
    }
}

/// [`payload_out`]'s inverse, for a payload value a host writes. A content
/// value whose offsets fall inside a character is refused.
pub(super) fn payload_in(v: &mut Value, ctx: &str) -> Result<(), JsValue> {
    if units::is_content_value(v) {
        return if units::value_to_scalars(v, Utf16) {
            Ok(())
        } else {
            Err(WasmError::from(format!("{ctx}: a content offset falls inside a character or past its text"))
                .to_js_value())
        };
    }
    match v {
        Value::Array(items) => items.iter_mut().try_for_each(|i| payload_in(i, ctx)),
        Value::Object(m) => m.values_mut().try_for_each(|i| payload_in(i, ctx)),
        _ => Ok(()),
    }
}

/// Run one write at `addr` on `doc`, its receipt as it crosses.
fn write(
    doc: &mut quillmark_core::document::Document,
    addr: &Addr,
    edit: Edit<'_>,
    opts: WriteOptions<'_>,
) -> Result<ReceiptJs, JsValue> {
    let card_base = card_base(doc, addr);
    let path = value_path(&card_base, addr);
    let to_js = |e: EditError| edit_error_to_js(&e, &card_base);
    let mut card = card_mut_of(doc, addr)?;
    let receipt: Receipt = card.write_in(target(addr), edit, opts, Utf16).map_err(to_js)?;
    let card_id = card.id().map(str::to_string);
    Ok(receipt_js(receipt, &path, card_id))
}

/// `receipt` as it crosses, its dropped anchors' selectors at `path` on the
/// card `card_id` names.
pub(super) fn receipt_js(receipt: Receipt, path: &quillmark_core::path::DocPath, card_id: Option<String>) -> ReceiptJs {
    let warnings = super::diags(receipt.diagnostics(Some(path)));
    ReceiptJs {
        applied: receipt.applied.to_value(),
        hash: receipt.hash,
        dropped_anchors: receipt
            .dropped_anchors
            .into_iter()
            .map(|a| {
                let (island, slot) = a.slot.map(|s| (s.island, s.slot)).unzip();
                DroppedAnchorJs {
                    selector: super::AnchorContextJs {
                        id: a.context.id,
                        path: path.to_string(),
                        card_id: card_id.clone(),
                        quote: a.context.quote,
                        prefix: a.context.prefix,
                        suffix: a.context.suffix,
                    },
                    island,
                    slot,
                }
            })
            .collect(),
        moved_anchors: receipt
            .moved_anchors
            .into_iter()
            .map(|m| MovedJs { id: m.id, before: m.before, after: m.after, reason: m.reason.name() })
            .collect(),
        warnings,
        minted: receipt.minted,
    }
}

#[wasm_bindgen]
impl Document {
    /// The `Content` at `addr`: an absent `addr.field` reads the body, an
    /// absent `addr.card` the main card, and an absent field the empty
    /// content. Throws on an out-of-range card, a malformed field name, or a
    /// present field value that is not content.
    #[wasm_bindgen(js_name = read, unchecked_return_type = "Content")]
    pub fn read(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr, &self.inner)?;
        let content = card_of(&self.inner, &addr)?
            .read(target(&addr))
            .map_err(|e| edit_error_to_js(&e, &card_base(&self.inner, &addr)))?;
        content_out(&content, "read")
    }

    /// **Apply** `change` to the value at `addr`, all its steps or none, and
    /// answer the `Receipt`. `opts.base` is the hash the writer read
    /// (`hash(addr)`), and the change's offsets are into that value: a value
    /// that hashes otherwise throws `edit::stale_base`. A refused change throws
    /// `edit::content_apply` and writes nothing.
    #[wasm_bindgen(js_name = apply, unchecked_return_type = "Receipt")]
    pub fn apply(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let opts = WriteOpts::from_js(&opts, "apply", &[])?;
        let addr = Addr::for_write(&addr, &self.inner, "apply")?;
        let change = change_in(change, "apply")?;
        let receipt = write(&mut self.inner, &addr, Edit::Apply(change), opts.as_ref())?;
        serialize_or_throw(&receipt, "apply")
    }

    /// Rewrite the value at `addr` as `markdown`, an edit of the read
    /// `toMarkdown(addr, { annotated: true })` gives: the anchors and islands
    /// whose `qm-*` tags it keeps stay put, and the rest keeps what a
    /// word-aligned diff keeps; what markdown does not spell is gone. Answers the
    /// `Receipt`, its `warnings` the import's. Throws as `apply`, and on an
    /// over-nested markdown input.
    #[wasm_bindgen(js_name = revise, unchecked_return_type = "Receipt")]
    pub fn revise_markdown(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        markdown: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let opts = WriteOpts::from_js(&opts, "revise", &[])?;
        let addr = Addr::for_write(&addr, &self.inner, "revise")?;
        let receipt = write(&mut self.inner, &addr, Edit::Revise(markdown), opts.as_ref())?;
        serialize_or_throw(&receipt, "revise")
    }

    /// Write `content` over the value at `addr`, carrying each anchor it does
    /// not name where a diff keeps its text and reporting the rest in
    /// `droppedAnchors`. Throws as `apply`, and on a `content` that is not a
    /// `Content`.
    #[wasm_bindgen(js_name = replace, unchecked_return_type = "Receipt")]
    pub fn replace(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Content")] content: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let opts = WriteOpts::from_js(&opts, "replace", &[])?;
        let addr = Addr::for_write(&addr, &self.inner, "replace")?;
        let content = content_in(content, "replace")?;
        let receipt = write(&mut self.inner, &addr, Edit::Replace(content), opts.as_ref())?;
        serialize_or_throw(&receipt, "replace")
    }

    /// **Enter** at `at` (UTF-16, into the value `opts.base` names): split
    /// the block holding it there, the text keeping its id and the new block
    /// `opts.id`, or one the write mints and names in `minted` under `"?0"`.
    /// Answers the `Receipt`; throws as `apply`.
    #[wasm_bindgen(js_name = splitBlock, unchecked_return_type = "Receipt")]
    pub fn split_block(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        at: u32,
        #[wasm_bindgen(unchecked_optional_param_type = "SplitBlockOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let id = string_opt(&opts, "id", "splitBlock")?;
        let edit = StructuralEdit::SplitBlock { at: at as usize, id };
        self.structural(addr, edit, opts, "splitBlock", &["id"])
    }

    /// **Backspace** at the start of block `id`: join it onto the block
    /// before it, under that block's record. Answers the `Receipt`; throws as
    /// `apply`, and `edit::content_apply` for the first block.
    #[wasm_bindgen(js_name = joinBlocks, unchecked_return_type = "Receipt")]
    pub fn join_blocks(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        id: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.structural(addr, StructuralEdit::JoinBlocks { id: id.to_string() }, opts, "joinBlocks", &[])
    }

    /// Delete `[start, end)` (UTF-16): where the range runs from a block's
    /// start into a later block, the block that keeps the text takes the
    /// shape of the block the range ends in. Answers the `Receipt`; throws as
    /// `apply`.
    #[wasm_bindgen(js_name = deleteRange, unchecked_return_type = "Receipt")]
    pub fn delete_range(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        start: u32,
        end: u32,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.structural(addr, StructuralEdit::DeleteRange { start: start as usize, end: end as usize }, opts, "deleteRange", &[])
    }

    /// **Tab** over the blocks `first` to `last` (`first` alone when absent):
    /// nest their items under the item before them. Answers the `Receipt`;
    /// throws as `apply`, and `edit::content_apply` where `first` is in no
    /// item, the range leaves its list, or its first item is the list's
    /// first.
    #[wasm_bindgen(js_name = indent, unchecked_return_type = "Receipt")]
    pub fn indent(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        first: &str,
        last: Option<String>,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let last = last.unwrap_or_else(|| first.to_string());
        self.structural(addr, StructuralEdit::Indent { first: first.to_string(), last }, opts, "indent", &[])
    }

    /// **Shift-Tab** over the blocks `first` to `last` (`first` alone when
    /// absent): lift them out of the innermost item or quote holding `first`.
    /// Answers the `Receipt`; throws as `apply`, and `edit::content_apply`
    /// where `first` is in neither or the range leaves it.
    #[wasm_bindgen(js_name = outdent, unchecked_return_type = "Receipt")]
    pub fn outdent(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        first: &str,
        last: Option<String>,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let last = last.unwrap_or_else(|| first.to_string());
        self.structural(addr, StructuralEdit::Outdent { first: first.to_string(), last }, opts, "outdent", &[])
    }

    /// Wrap the blocks `first` to `last` in a quote or a list, around the
    /// children of their innermost shared item or quote the range touches; a
    /// list the range sits in is wrapped whole. Answers the `Receipt`, the
    /// containers it opens named in `minted`; throws as `apply`.
    #[wasm_bindgen(js_name = wrap, unchecked_return_type = "Receipt")]
    pub fn wrap(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        first: &str,
        last: &str,
        #[wasm_bindgen(unchecked_param_type = "WrapKind")] kind: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let kind = js_value_to_json(kind, "wrap")?;
        let with = match kind.get("kind").and_then(Value::as_str) {
            Some("quote") => Wrap::Quote,
            Some("list") => Wrap::List {
                ordered: kind.get("ordered").and_then(Value::as_bool).unwrap_or(false),
                checked: kind.get("checked").and_then(Value::as_bool),
            },
            _ => return Err(WasmError::from("wrap: expected { kind: \"quote\" } or { kind: \"list\", ordered }").to_js_value()),
        };
        let edit = StructuralEdit::Wrap { first: first.to_string(), last: last.to_string(), with };
        self.structural(addr, edit, opts, "wrap", &[])
    }

    /// The block holding offset `at` (UTF-16) of the value at `addr`: its
    /// `id`, its `index` among the value's blocks and its container `path`;
    /// at a block's end, that block. Throws `edit::content_apply` where `at`
    /// is past the text or inside a surrogate pair, and as `read`.
    #[wasm_bindgen(js_name = blockAt, unchecked_return_type = "Located")]
    pub fn block_at(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        at: u32,
    ) -> Result<JsValue, JsValue> {
        let addr = Addr::from_js_or_string(&addr, &self.inner)?;
        let card = card_of(&self.inner, &addr)?;
        let to_js = |e: EditError| edit_error_to_js(&e, &card_base(&self.inner, &addr));
        let content = card.read(target(&addr)).map_err(to_js)?;
        let at = offset_in(&content, at)?;
        let located = card.block_at(target(&addr), at).map_err(to_js)?;
        let out = serde_json::json!({ "id": located.id, "index": located.index, "path": located.path });
        serialize_or_throw(&out, "blockAt")
    }
}

impl Document {
    fn structural(&mut self, addr: JsValue, edit: StructuralEdit, opts: JsValue, verb: &str, extra: &[&str]) -> Result<JsValue, JsValue> {
        let opts = WriteOpts::from_js(&opts, verb, extra)?;
        let addr = Addr::for_write(&addr, &self.inner, verb)?;
        let receipt = write(&mut self.inner, &addr, Edit::Structural(edit), opts.as_ref())?;
        serialize_or_throw(&receipt, verb)
    }
}

/// A string `key` of `opts`, absent where `opts` or the key is.
fn string_opt(opts: &JsValue, key: &str, verb: &str) -> Result<Option<String>, JsValue> {
    if opts.is_undefined() || opts.is_null() || !js_sys::Reflect::has(opts, &key.into()).unwrap_or(false) {
        return Ok(None);
    }
    let v = js_sys::Reflect::get(opts, &key.into()).unwrap_or(JsValue::UNDEFINED);
    if v.is_undefined() {
        return Ok(None);
    }
    v.as_string().map(Some).ok_or_else(|| WasmError::from(format!("{verb}: {key} must be a string when given")).to_js_value())
}

/// The change that undoes `change` on `before`. Throws `edit::content_apply`
/// where `change` does not apply to `before`.
#[wasm_bindgen(js_name = invert, unchecked_return_type = "Change")]
pub fn invert(
    #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Content")] before: JsValue,
) -> Result<JsValue, JsValue> {
    let before = content_in(before, "invert")?;
    let change = units::change_to_scalars(&change_in(change, "invert")?, &before, Utf16).map_err(apply_error)?;
    let after = change::apply(&before, &change).map_err(apply_error)?.content;
    let undo = change::invert(&change, &before).map_err(apply_error)?;
    let undo = units::change_from_scalars(&undo, &after, Utf16).map_err(apply_error)?;
    serialize_or_throw(&undo.to_value(), "invert")
}

/// A change taking `before` to `after`, word-aligned, for a writer with no
/// steps of its own. Throws `edit::content_apply` where `after` is not a
/// valid value.
#[wasm_bindgen(js_name = diff, unchecked_return_type = "Change")]
pub fn diff(
    #[wasm_bindgen(unchecked_param_type = "Content")] before: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Content")] after: JsValue,
) -> Result<JsValue, JsValue> {
    let (before, after) = (content_in(before, "diff")?, content_in(after, "diff")?);
    let change = quillmark_content::diff::diff(&before, &after).map_err(|e| apply_error(e.into()))?;
    let change = units::change_from_scalars(&change, &before, Utf16).map_err(apply_error)?;
    serialize_or_throw(&change.to_value(), "diff")
}

/// The valid value every peer holding the merged state `raw` reads, what was
/// mended to reach it, and the change from `raw`'s text to the value's, which
/// a peer maps its positions through: total and deterministic, every id it
/// mints derived from the ids involved. Throws only on a `raw` not shaped as
/// content.
#[wasm_bindgen(js_name = repair, unchecked_return_type = "Repaired")]
pub fn repair(#[wasm_bindgen(unchecked_param_type = "RawContent")] raw: JsValue) -> Result<JsValue, JsValue> {
    let json = js_value_to_json(raw, "repair")?;
    check_content_value(&json).map_err(bound_error)?;
    let raw = quillmark_content::repair::Raw::from_value_in(&json, Utf16)
        .map_err(|e| WasmError::from(format!("repair: not shaped as content: {e}")).to_js_value())?;
    let (content, repairs, change) = quillmark_content::repair::repair(raw.clone());
    let out = serde_json::json!({
        "content": content.to_value_in(Utf16),
        "repairs": repairs.iter().map(quillmark_content::repair::Repair::to_value).collect::<Vec<_>>(),
        "change": quillmark_content::repair::change_in(&raw, &change, Utf16).to_value(),
    });
    serialize_or_throw(&out, "repair")
}

/// A UTF-16 offset into `content`'s text, as a scalar offset; one inside a
/// surrogate pair or past the text is refused.
fn offset_in(content: &Content, at: u32) -> Result<usize, JsValue> {
    units::to_scalars(Utf16, &content.text, at as usize)
        .ok_or_else(|| apply_error(change::ApplyError::Stale { step: 0 }))
}

/// The change taking `before` to `after` where they differ over one range,
/// `range.from` to `range.to` of `before` (UTF-16), which `after` holds from
/// `range.from` to `range.to + after.length - before.length`, as a
/// ProseMirror `ReplaceStep` names it: one splice over the characters of the
/// range that differ, and the records, marks, anchors and islands as `diff`
/// gives them. Where the two differ outside the range, the answer is
/// `diff`'s. Throws `edit::content_apply` where `after` is not a valid value
/// or an offset falls inside a surrogate pair.
#[wasm_bindgen(js_name = diffRange, unchecked_return_type = "Change")]
pub fn diff_range(
    #[wasm_bindgen(unchecked_param_type = "Content")] before: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Content")] after: JsValue,
    #[wasm_bindgen(unchecked_param_type = "TextRange")] range: JsValue,
) -> Result<JsValue, JsValue> {
    let (before, after) = (content_in(before, "diffRange")?, content_in(after, "diffRange")?);
    reject_unknown_keys(&range, &["from", "to"], "range", |k| format!("diffRange: range has unknown key `{k}`; it takes `from` and `to`"))?;
    let bound = |key: &str| -> Result<u32, JsValue> {
        js_sys::Reflect::get(&range, &key.into())
            .ok()
            .and_then(|v| v.as_f64())
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64)
            .map(|n| n as u32)
            .ok_or_else(|| WasmError::from(format!("diffRange: range.{key} must be a non-negative integer")).to_js_value())
    };
    let (from, to) = (offset_in(&before, bound("from")?)?, offset_in(&before, bound("to")?)?);
    let change = quillmark_content::diff::diff_range(&before, &after, from, to).map_err(|e| apply_error(e.into()))?;
    let change = units::change_from_scalars(&change, &before, Utf16).map_err(apply_error)?;
    serialize_or_throw(&change.to_value(), "diffRange")
}

/// Typing at `at`: the formatting the typed text carries, for a splice's
/// `marks`, in canonical order. Throws `edit::content_apply` where `at` is
/// past the text.
#[wasm_bindgen(js_name = inheritedMarks, unchecked_return_type = "ContentMarkKind[]")]
pub fn inherited_marks(
    #[wasm_bindgen(unchecked_param_type = "Content")] content: JsValue,
    at: u32,
) -> Result<JsValue, JsValue> {
    let content = content_in(content, "inheritedMarks")?;
    let at = offset_in(&content, at)?;
    let kinds = structural::inherited_marks(&content, at).map_err(apply_error)?;
    let out: Vec<Value> = kinds
        .iter()
        .map(|k| {
            let mut m = serde_json::Map::new();
            m.insert("type".into(), k.tag().into());
            if let Some(attrs) = quillmark_content::wire::mark_attrs(k) {
                m.insert("attrs".into(), attrs);
            }
            Value::Object(m)
        })
        .collect();
    serialize_or_throw(&out, "inheritedMarks")
}

/// Where `pos` lands after `change`, and what around it the change deleted;
/// `assoc` (`"after"` by default) picks the side of text inserted at it.
/// Throws `edit::content_apply` where a splice deletes an island and names
/// none in `removed`, as the receipt's `applied` always does.
#[wasm_bindgen(js_name = mapPos, unchecked_return_type = "MapResult")]
pub fn map_pos(
    #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Pos")] pos: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "\"before\" | \"after\"")] assoc: JsValue,
) -> Result<JsValue, JsValue> {
    let change = change_in(change, "mapPos")?;
    let pos = quillmark_content::wire::pos_from_value(&js_value_to_json(pos, "mapPos")?)
        .map_err(|e| WasmError::from(format!("mapPos: not a position: {e}")).to_js_value())?;
    let assoc = match assoc.as_string().as_deref() {
        None | Some("after") => Assoc::After,
        Some("before") => Assoc::Before,
        Some(other) => {
            return Err(WasmError::from(format!("mapPos: assoc is \"before\" or \"after\", not {other:?}")).to_js_value());
        }
    };
    let r = change::map_pos_in(&change, &pos, assoc, Utf16).map_err(apply_error)?;
    let out = serde_json::json!({
        "pos": quillmark_content::wire::pos_value(&r.pos),
        "deleted": r.deleted,
        "deletedBefore": r.deleted_before,
        "deletedAfter": r.deleted_after,
        "deletedAcross": r.deleted_across,
    });
    serialize_or_throw(&out, "mapPos")
}
