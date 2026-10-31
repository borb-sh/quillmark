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
use quillmark_content::model::Content;
use quillmark_content::units::{self, Utf16};
use quillmark_core::document::{Card, CardMut, EditError, Receipt, Target};
use quillmark_core::path::DocPath;
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const CONTENT_TS: &'static str = r#"
/**
 * One rich-text value: a card body or a `richtext` field. Flat `text` where a
 * `\n` ends a block, `U+2028` is a hard break and `U+FFFC` is an island's slot;
 * one block record per `\n`-separated segment; containers the blocks' paths
 * name; formatting marks; comment anchors; and islands, a table's cells and a
 * footnote's note each a nested `Content` in its `slots`. Every part carries an
 * id. Offsets are UTF-16 code units, JavaScript string indices, into the
 * `text` of the value holding them.
 */
export interface Content {
    text: string;
    blocks: ContentBlock[];
    containers?: ContentContainer[];
    marks?: ContentMark[];
    anchors?: ContentAnchor[];
    islands?: ContentIsland[];
}

/** A block's record: its kind and attributes, and its container path,
 *  outermost first. */
export type ContentBlock = { id: string; path?: string[] } & ContentBlockKind;

export type ContentBlockKind =
    | { kind: "para" }
    | { kind: "heading"; attrs: { level: number } }
    | { kind: "code"; attrs?: { lang?: string } }
    | { kind: "rule" };

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
    | { type: "font_family"; attrs: { name: string } };

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
);

/** A position: an offset into a value's text, or a path into an island's slot. */
export type Pos = number | { island: string; slot: string; at: Pos };

/** Which side of an insertion at a position `mapPos` keeps it to. */
export type Assoc = "before" | "after";

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
 *   record of each segment an inserted `\n` ends; `islands` the island of each
 *   inserted `U+FFFC`. `removed` is what the deleted text held, filled in on
 *   the receipt's `applied`.
 * - `setBlock` changes a block's kind, attributes or path.
 * - `container` adds a container or sets an existing one's attributes, or
 *   removes one no path names.
 * - `mark` adds or removes formatting over a range.
 * - `anchor` places, moves or removes a comment anchor.
 * - `island` replaces an island's props, or inserts an island's slot at `at`.
 * - `nested` applies a change inside one slot of an island.
 */
export type Step =
    | { splice: { at: number; delete?: string; insert: string;
          marks?: ({ start?: number; end?: number } & ContentMarkKind)[];
          blocks?: ContentBlock[]; islands?: ContentIsland[];
          removed?: { blocks?: ContentBlock[]; marks?: ContentMark[]; islands?: ContentIsland[] } } }
    | { setBlock: { id: string; kind?: ContentBlockKind["kind"]; attrs?: Record<string, unknown>; path?: string[] } }
    | { container: ContentContainer | { id: string; remove: true } }
    | { mark: { start: number; end: number; add: boolean } & ContentMarkKind }
    | { anchor: ContentAnchor | { id: string; remove: true } }
    | { island: { id: string; props: Record<string, unknown>; slots?: Record<string, Content> } | { at: number; island: ContentIsland } }
    | { nested: { island: string; slot: string; change: Change } };

/**
 * What a write did. `applied` is the change in its normalized form, which
 * `invert(applied, before)` undoes; `hash` is the value's new hash, the next
 * write's `base`. `droppedAnchors` names each anchor whose text the write
 * deleted, as it read before, and `movedAnchors` each surviving one whose text
 * it rewrote. `warnings` holds a `parse::dropped_construct` per construct a
 * revise's markdown dropped.
 */
export interface Receipt {
    applied: Change;
    hash: string;
    droppedAnchors: AnchorContext[];
    movedAnchors: { id: string; before: string; after: string }[];
    warnings: Diagnostic[];
}

/** The hash a writer read, which a write refuses with `edit::stale_base` when
 *  the value has changed since. Pass it: a write without one lands over
 *  whatever the value now holds. */
export interface WriteOptions {
    base?: string;
}

/** One write of `applyAll`. */
export interface Write {
    addr: Addr | string;
    change: Change;
    base?: string;
}

/**
 * A value as a merge of the shared state left it, for `repair`: shaped as
 * `Content`, its invariants unchecked. `islands` may be an object keyed by
 * island id, with `slot_ids` naming the island each `U+FFFC` of `text`
 * carries, `null` where it carries none; an `islands` array pairs with the
 * slots in order. An anchor placed as a range carries `range: true`, since at
 * zero width it cannot be told from a point.
 */
export type RawContent = Omit<Content, "islands" | "anchors" | "blocks"> & {
    blocks?: ContentBlock[];
    anchors?: (ContentAnchor & { range?: boolean })[];
    islands?: ContentIsland[] | Record<string, Omit<ContentIsland, "id" | "slots"> & { slots?: Record<string, RawContent> }>;
    slot_ids?: (string | null)[];
};

/** One change `repair` made. */
export type Repair =
    | { repair: "missing_container"; block: string; container: string }
    | { repair: "two_parents"; container: string }
    | { repair: "split_run"; container: string; fresh: string }
    | { repair: "orphan_slot"; island: string | null }
    | { repair: "orphan_island"; island: string }
    | { repair: "shared_island"; island: string; fresh: string }
    | { repair: "split_block"; island: string; block: string }
    | { repair: "collapsed_anchor"; id: string }
    | { repair: "invalid"; reason: string }
    | { repair: "in_slot"; island: string; slot: string; inner: Repair };

/** A repaired value and what changed to make it valid. */
export interface Repaired {
    content: Content;
    repairs: Repair[];
}
"#;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptJs {
    applied: Value,
    hash: String,
    dropped_anchors: Vec<super::AnchorContextJs>,
    moved_anchors: Vec<MovedJs>,
    warnings: Vec<Diagnostic>,
}

#[derive(serde::Serialize)]
struct MovedJs {
    id: String,
    before: String,
    after: String,
}

/// The `base` of a `WriteOptions`.
fn base_from_js(opts: &JsValue, verb: &str) -> Result<Option<String>, JsValue> {
    if opts.is_undefined() || opts.is_null() {
        return Ok(None);
    }
    let invalid = |e: String| WasmError::from(format!("{verb}: {e}")).to_js_value();
    if js_sys::Reflect::has(opts, &"baseMarkdown".into()).unwrap_or(false) {
        return Err(invalid(
            "a write takes the hash it read as `base`; a markdown base has no change to merge against".into(),
        ));
    }
    reject_unknown_keys(opts, &["base", "baseHash"], "options", |k| {
        format!("{verb}: options have unknown key `{k}`; a write takes only `base`")
    })?;
    let mut found = None;
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
        if found.replace(hash).is_some() {
            return Err(invalid("name base or baseHash, not both".into()));
        }
    }
    Ok(found)
}

fn target(addr: &Addr) -> Target<'_> {
    match &addr.field {
        None => Target::Body,
        Some(name) => Target::Field(name),
    }
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
    WasmError { diagnostics: vec![EditError::Change(e).to_diagnostic(None)] }.to_js_value()
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
    Content::from_value_in(&json, Utf16)
        .map_err(|e| WasmError::from(format!("{ctx}: not a Content: {e}")).to_js_value())
}

/// `c` for JS, offsets in UTF-16 code units.
pub(super) fn content_out(c: &Content, ctx: &str) -> Result<JsValue, JsValue> {
    serialize_or_throw(&c.to_value_in(Utf16), ctx)
}

/// A change from JS, still in UTF-16 code units.
fn change_in(value: JsValue, ctx: &str) -> Result<Change, JsValue> {
    let json = js_value_to_json(value, ctx)?;
    Change::from_value_in(&json, Utf16).map_err(|e| WasmError::from(format!("{ctx}: not a Change: {e}")).to_js_value())
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

/// How a write builds its change from the value it read.
enum Edit<'a> {
    Apply(Change),
    Revise(&'a str),
    Replace(Content),
}

/// Run one write at `addr` on `doc`, its receipt as it crosses.
fn write(
    doc: &mut quillmark_core::document::Document,
    addr: &Addr,
    edit: Edit<'_>,
    base: Option<&str>,
) -> Result<ReceiptJs, JsValue> {
    let card_base = card_base(doc, addr);
    let path = value_path(&card_base, addr);
    let to_js = |e: EditError| edit_error_to_js(&e, &card_base);
    let target = target(addr);
    let before = card_of(doc, addr)?.read_at(target, base).map_err(to_js)?;
    let mut card = card_mut_of(doc, addr)?;
    let receipt: Receipt = match edit {
        Edit::Apply(change) => {
            let change = units::change_to_scalars(&change, &before, Utf16).map_err(|e| to_js(EditError::Change(e)))?;
            card.apply(target, &change, base)
        }
        Edit::Revise(markdown) => card.revise(target, markdown, base),
        Edit::Replace(content) => card.replace(target, &content, base),
    }
    .map_err(to_js)?;
    let card_id = card.id().map(str::to_string);
    let applied = units::change_from_scalars(&receipt.applied, &before, Utf16).map_err(apply_error)?;
    Ok(ReceiptJs {
        applied: applied.to_value(),
        hash: receipt.hash,
        dropped_anchors: receipt
            .dropped_anchors
            .into_iter()
            .map(|a| super::AnchorContextJs {
                id: a.id,
                path: path.to_string(),
                card_id: card_id.clone(),
                quote: a.quote,
                prefix: a.prefix,
                suffix: a.suffix,
            })
            .collect(),
        moved_anchors: receipt
            .moved_anchors
            .into_iter()
            .map(|m| MovedJs { id: m.id, before: m.before, after: m.after })
            .collect(),
        warnings: super::diags(quillmark_core::document::dropped_constructs(receipt.warnings, Some(&path))),
    })
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
        let addr = Addr::from_js_or_string(&addr)?;
        let content = card_of(&self.inner, &addr)?
            .read(target(&addr))
            .map_err(|e| edit_error_to_js(&e, &card_base(&self.inner, &addr)))?;
        content_out(&content, "read")
    }

    /// The markdown at `addr` with a `qm-*` tag for each anchor and island id
    /// its lines can hold: what `revise` reads back. Throws as `read`.
    #[wasm_bindgen(js_name = annotated)]
    pub fn annotated(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
    ) -> Result<String, JsValue> {
        let addr = Addr::from_js_or_string(&addr)?;
        let card = card_of(&self.inner, &addr)?;
        match &addr.field {
            None => Ok(card.body_annotated().markdown),
            Some(field) => card
                .field_annotated(field)
                .map(|a| a.markdown)
                .map_err(|e| edit_error_to_js(&e, &card_base(&self.inner, &addr))),
        }
    }

    /// **Apply** `change` to the value at `addr`, all its steps or none, and
    /// answer the `Receipt`. `opts.base` is the hash the writer read
    /// (`hash(addr)`), a stale one refused with `edit::stale_base`; a refused
    /// change throws `edit::content_apply` and writes nothing.
    #[wasm_bindgen(js_name = apply, unchecked_return_type = "Receipt")]
    pub fn apply(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let base = base_from_js(&opts, "apply")?;
        let addr = Addr::from_js_or_string(&addr)?;
        let change = change_in(change, "apply")?;
        let receipt = write(&mut self.inner, &addr, Edit::Apply(change), base.as_deref())?;
        serialize_or_throw(&receipt, "apply")
    }

    /// Apply each write in order, all of them or none, a `base` per write.
    /// The first refused write throws, the document as it was.
    #[wasm_bindgen(js_name = applyAll, unchecked_return_type = "Receipt[]")]
    pub fn apply_all(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Write[]")] writes: JsValue,
    ) -> Result<JsValue, JsValue> {
        if !js_sys::Array::is_array(&writes) {
            return Err(WasmError::from("applyAll: writes must be a Write[]").to_js_value());
        }
        let mut next = self.inner.clone();
        let mut receipts = Vec::new();
        for w in js_sys::Array::from(&writes).iter() {
            reject_unknown_keys(&w, &["addr", "change", "base"], "write", |k| {
                format!("applyAll: a write has unknown key `{k}`; a Write is {{ addr, change, base }}")
            })?;
            let get = |key: &str| js_sys::Reflect::get(&w, &key.into()).unwrap_or(JsValue::UNDEFINED);
            let addr = Addr::from_js_or_string(&get("addr"))?;
            let base = match get("base") {
                b if b.is_undefined() => None,
                b => Some(b.as_string().ok_or_else(|| {
                    WasmError::from("applyAll: a write's base must be a string when given").to_js_value()
                })?),
            };
            let change = change_in(get("change"), "applyAll")?;
            receipts.push(write(&mut next, &addr, Edit::Apply(change), base.as_deref())?);
        }
        self.inner = next;
        serialize_or_throw(&receipts, "applyAll")
    }

    /// Rewrite the value at `addr` as `markdown`: the anchors and islands
    /// whose `qm-*` tags it keeps (`annotated(addr)` spells them) stay put,
    /// and the rest keeps what a word-aligned diff keeps. Answers the
    /// `Receipt`, its `warnings` the import's. Throws as `apply`, and on an
    /// over-nested markdown input.
    #[wasm_bindgen(js_name = revise, unchecked_return_type = "Receipt")]
    pub fn revise_markdown(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        markdown: &str,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let base = base_from_js(&opts, "revise")?;
        let addr = Addr::from_js_or_string(&addr)?;
        let receipt = write(&mut self.inner, &addr, Edit::Revise(markdown), base.as_deref())?;
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
        let base = base_from_js(&opts, "replace")?;
        let addr = Addr::from_js_or_string(&addr)?;
        let content = content_in(content, "replace")?;
        let receipt = write(&mut self.inner, &addr, Edit::Replace(content), base.as_deref())?;
        serialize_or_throw(&receipt, "replace")
    }

    /// `replace` under its earlier name.
    #[wasm_bindgen(js_name = overwrite, unchecked_return_type = "Receipt")]
    pub fn overwrite(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Content")] content: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.replace(addr, content, opts)
    }

    /// `replace` under its earlier name.
    #[wasm_bindgen(js_name = reviseContent, unchecked_return_type = "Receipt")]
    pub fn revise_content(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Content")] content: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.replace(addr, content, opts)
    }

    /// `apply` under its earlier name.
    #[wasm_bindgen(js_name = applyChange, unchecked_return_type = "Receipt")]
    pub fn apply_change(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Addr | string")] addr: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "WriteOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.apply(addr, change, opts)
    }
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

/// `a` then `b` as one change: their steps, adjacent splices that touch
/// merged.
#[wasm_bindgen(js_name = compose, unchecked_return_type = "Change")]
pub fn compose(
    #[wasm_bindgen(unchecked_param_type = "Change")] a: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Change")] b: JsValue,
) -> Result<JsValue, JsValue> {
    let (a, b) = (change_in(a, "compose")?, change_in(b, "compose")?);
    serialize_or_throw(&change::compose_in(&a, &b, Utf16).to_value(), "compose")
}

/// Where `pos` lands after `change`, `null` where the change deleted it.
/// `assoc` is the side of an insertion at `pos` it keeps to.
#[wasm_bindgen(js_name = mapPos, unchecked_return_type = "Pos | null")]
pub fn map_pos(
    #[wasm_bindgen(unchecked_param_type = "Change")] change: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Pos")] pos: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Assoc")] assoc: JsValue,
) -> Result<JsValue, JsValue> {
    let change = change_in(change, "mapPos")?;
    let pos = quillmark_content::wire::pos_from_value(&js_value_to_json(pos, "mapPos")?)
        .map_err(|e| WasmError::from(format!("mapPos: not a Pos: {e}")).to_js_value())?;
    let assoc = match assoc.as_string().as_deref() {
        Some("before") => Assoc::Before,
        Some("after") => Assoc::After,
        _ => return Err(WasmError::from("mapPos: assoc must be \"before\" or \"after\"").to_js_value()),
    };
    match change::map_pos_in(&change, &pos, assoc, Utf16) {
        Some(p) => serialize_or_throw(&quillmark_content::wire::pos_value(&p), "mapPos"),
        None => Ok(JsValue::NULL),
    }
}

/// A change taking `before` to `after`, word-aligned, for a writer with no
/// steps of its own.
#[wasm_bindgen(js_name = diff, unchecked_return_type = "Change")]
pub fn diff(
    #[wasm_bindgen(unchecked_param_type = "Content")] before: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Content")] after: JsValue,
) -> Result<JsValue, JsValue> {
    let (before, after) = (content_in(before, "diff")?, content_in(after, "diff")?);
    let change = quillmark_content::diff::diff(&before, &after);
    let change = units::change_from_scalars(&change, &before, Utf16).map_err(apply_error)?;
    serialize_or_throw(&change.to_value(), "diff")
}

/// `raw` made valid by the merge-repair rules, and what changed. Total over
/// any value shaped as content, and deterministic: the same raw value always
/// repairs to the same content, so two peers repairing at once agree.
#[wasm_bindgen(js_name = repair, unchecked_return_type = "Repaired")]
pub fn repair(#[wasm_bindgen(unchecked_param_type = "RawContent")] raw: JsValue) -> Result<JsValue, JsValue> {
    let mut json = js_value_to_json(raw, "repair")?;
    units::raw_to_scalars(&mut json, Utf16);
    let raw = quillmark_content::repair::Raw::from_value(&json)
        .map_err(|e| WasmError::from(format!("repair: not shaped as content: {e}")).to_js_value())?;
    let (content, repairs) = quillmark_content::repair::repair(raw);
    let out = serde_json::json!({
        "content": content.to_value_in(Utf16),
        "repairs": repairs.iter().map(|r| r.to_value()).collect::<Vec<_>>(),
    });
    serialize_or_throw(&out, "repair")
}
