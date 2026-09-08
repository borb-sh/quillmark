//! Canonical Markdown emission for [`Document`].
//!
//! Scalar emission (quoting, escaping, multi-line handling) is delegated to
//! `serde-saphyr`, the library the parser uses, so emit and parse are symmetric
//! by construction: what saphyr quotes on emit it reads back as a string, and
//! the YAML 1.1 edge cases ad-hoc quoting misses (`on`/`yes`/`off`, leading-zero
//! integers) are handled there. `prefer_block_scalars: false` keeps multi-line
//! strings inline as double-quoted scalars, so no `|` / `>` forms are emitted.
//!
//! This module owns the surrounding structure: fences, `$` metadata lines, field
//! ordering, indentation, and comment interleaving.

use serde_json::Value as JsonValue;
use serde_saphyr::{FlowMap, FlowSeq, SerializerOptions};

use super::payload::PayloadItem;
use super::prescan::NestedComment;
use crate::value::PathSegment;
use super::{Card, Document};

impl Document {
    /// Emit canonical Quillmark Markdown from this document.
    ///
    /// # Contract
    ///
    /// 1. **Type-fidelity round-trip.** `Document::parse(&doc.to_markdown())`
    ///    returns a `Document` equal to `doc` by value *and* by type variant.
    ///    `QuillValue::String("on")` round-trips as a string, never as a bool;
    ///    `QuillValue::String("01234")` never as an integer.
    ///
    ///    **Content-field carve-out.** A richtext field committed as a canonical
    ///    content object (and the card `$body`) projects to its markdown form, so
    ///    identity marks and content-only marks do not survive a
    ///    `to_markdown`→`from_markdown` round-trip. The storage DTO is the
    ///    lossless carrier; the guarantee above holds for every other field.
    ///
    /// 2. **Emit-idempotent.** `to_markdown` is a pure function of `doc`; two
    ///    calls on the same `doc` return byte-equal strings.
    ///
    /// Byte-equality with the *original source* is **not** guaranteed.
    ///
    /// The emitted form, and what survives it:
    /// `prose/references/markdown-spec.md` §9 and §3.4.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();

        // Bodies are content values whose markdown is an export projection, so a
        // round-trip canonicalizes them (leading and trailing blank lines are
        // dropped: a value, not a file).
        emit_block(&mut out, self.main());
        append_body(&mut out, &self.main().body_markdown());

        // The separator is normalised before each block, so edited bodies that
        // lack a trailing blank line still round-trip.
        for card in self.cards() {
            ensure_blank_before_fence(&mut out);
            emit_block(&mut out, card);
            append_body(&mut out, &card.body_markdown());
        }

        // The body projection carries no trailing newline; the emitted document
        // is a file, so it owns its final one.
        if !out.ends_with('\n') {
            out.push('\n');
        }

        out
    }
}

/// Append a card's markdown body after its closing fence, separated by one
/// blank line (the conventional card-yaml shape). Empty bodies append nothing:
/// the fence closes and the next block (or EOF) follows.
fn append_body(out: &mut String, body: &str) {
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
    }
}

fn emit_meta_line(out: &mut String, key: &str, value: &str, trailer: Option<&str>) {
    out.push('$');
    out.push_str(key);
    out.push_str(": ");
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(value.to_string())));
    push_trailer(out, trailer);
    out.push('\n');
}

/// Emit an out-of-band meta block (`$ext` / `$seed`). `nested` carries comments
/// at paths relative to the value tree. Meta maps never carry `!must_fill`.
fn emit_meta_block(
    out: &mut String,
    key: &str,
    value: &serde_json::Map<String, JsonValue>,
    trailer: Option<&str>,
    nested: &[NestedComment],
) {
    if value.is_empty() {
        out.push_str(key);
        out.push_str(": {}");
        push_trailer(out, trailer);
        out.push('\n');
        return;
    }
    out.push_str(key);
    out.push(':');
    push_trailer(out, trailer);
    out.push('\n');
    emit_mapping_children(
        out,
        value,
        2,
        EmitCtx {
            nested,
            ..EmitCtx::EMPTY
        },
    );
}

/// The sidecar tables threaded through the recursive emit: `path` is the
/// container path the current node sits at, `nested` and `fills` the whole
/// block's comment and `!must_fill` tables.
#[derive(Clone, Copy)]
struct EmitCtx<'a> {
    path: &'a [PathSegment],
    nested: &'a [NestedComment],
    fills: &'a [Vec<PathSegment>],
}

impl<'a> EmitCtx<'a> {
    const EMPTY: Self = Self {
        path: &[],
        nested: &[],
        fills: &[],
    };

    fn at(self, path: &'a [PathSegment]) -> Self {
        Self { path, ..self }
    }

    /// Fill sets are small, so a linear scan beats building a hash set per field.
    fn is_fill(self, path: &[PathSegment]) -> bool {
        self.fills.iter().any(|p| p.as_slice() == path)
    }
}

/// Where a field's key goes: on its own line at an indent, or right after the
/// `- ` the caller already wrote.
#[derive(Clone, Copy)]
enum KeyPos {
    Line(usize),
    SeqHead(usize),
}

impl KeyPos {
    fn write_key(self, out: &mut String, key: &str) {
        match self {
            KeyPos::Line(indent) => {
                push_indent(out, indent);
                emit_key_at(out, key, indent);
            }
            KeyPos::SeqHead(_) => emit_key(out, key),
        }
    }

    fn map_indent(self) -> usize {
        match self {
            KeyPos::Line(i) | KeyPos::SeqHead(i) => i + 2,
        }
    }

    fn seq_indent(self) -> usize {
        match self {
            KeyPos::Line(i) => i + 2,
            KeyPos::SeqHead(i) => i + 4,
        }
    }
}

fn emit_block(out: &mut String, card: &Card) {
    out.push_str("~~~\n");
    emit_payload_items(out, card.payload().items());
    out.push_str("~~~\n");
}

/// Walk the unified item list and emit each entry. An `inline: true` comment
/// immediately following a non-comment item is consumed as that item's trailer.
fn emit_payload_items(out: &mut String, items: &[PayloadItem]) {
    let mut i = 0;
    while i < items.len() {
        let trailer = items.get(i + 1).and_then(|next| match next {
            PayloadItem::Comment { text, inline: true } => Some(text.as_str()),
            _ => None,
        });
        let mut consumed_trailer = trailer.is_some();

        match &items[i] {
            PayloadItem::Quill { reference } => {
                emit_meta_line(out, "quill", &reference.to_string(), trailer);
            }
            PayloadItem::Kind { value } => {
                emit_meta_line(out, "kind", value, trailer);
            }
            PayloadItem::Meta {
                key,
                value,
                nested_comments,
            } => {
                emit_meta_block(out, key.as_str(), value, trailer, nested_comments);
            }
            PayloadItem::Field {
                key,
                value,
                fill,
                nested_comments,
            } => {
                // card-yaml is the human-authored surface, so a stored content
                // object projects back to markdown here. The projection runs
                // marker or no marker: a seeded `example` on a must-fill content
                // field carries one, and the raw object has no card-yaml
                // spelling. Once projected the cell is a scalar, which is the
                // shape `!must_fill` emits against.
                if let Some(markdown) = project_content_field(value.as_json()) {
                    emit_field_at(
                        out,
                        key,
                        &JsonValue::String(markdown),
                        KeyPos::Line(0),
                        *fill,
                        EmitCtx::EMPTY,
                        trailer,
                    );
                    i += if consumed_trailer { 2 } else { 1 };
                    continue;
                }
                // Nested fill markers; the top-level one rides on `*fill`.
                let fills = value.fill_paths();
                emit_field_at(
                    out,
                    key,
                    value.as_json(),
                    KeyPos::Line(0),
                    *fill,
                    EmitCtx {
                        nested: nested_comments,
                        fills: &fills,
                        ..EmitCtx::EMPTY
                    },
                    trailer,
                );
            }
            PayloadItem::Comment { text, .. } => {
                push_comment_line(out, 0, text);
                consumed_trailer = false;
            }
        }
        i += if consumed_trailer { 2 } else { 1 };
    }
}

/// The markdown projection of a richtext-valued field, or `None` when `value` is
/// not a canonical content object.
///
/// Projecting keeps card-yaml markdown-clean rather than carrying a nested
/// `{text, lines, marks, islands}` tree. It is lossy: island ids and
/// content-only marks do not survive, and the storage DTO is the lossless
/// carrier.
///
/// The guard requires the object to serialize back to a **byte-identical**
/// canonical content, so a user object that merely resembles one stays
/// structural. Either canonical form counts. `store_field` writes what it is
/// given verbatim, so a content read off the seam rests here spelling its zero
/// `instance`s; a typed commit rests in the storage form. The comparison
/// is on the serialized *strings*: under `serde_json/preserve_order`, `Value`'s
/// `PartialEq` is order-independent, so a `Value` guard would also project a
/// content-canonical object whose keys are in non-canonical order.
pub(super) fn project_content_field(value: &JsonValue) -> Option<String> {
    if !value.is_object() {
        return None;
    }
    let rt = quillmark_content::serial::from_canonical_value(value).ok()?;
    let as_written = serde_json::to_string(value).ok()?;
    let storage = serde_json::to_string(&quillmark_content::serial::to_canonical_value(&rt)).ok()?;
    let seam = serde_json::to_string(&quillmark_content::serial::to_seam_value(&rt)).ok()?;
    if as_written != storage && as_written != seam {
        return None;
    }
    Some(quillmark_content::export::to_markdown(&rt))
}

/// Ensure `out` ends with `\n\n` so the next fence has a blank line above it.
/// No-op on empty `out`: a block at line 1 needs no separator.
fn ensure_blank_before_fence(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

/// Emit own-line nested comments at `position` in the context path (inline
/// comments are handled by `find_inline_trailer`).
fn emit_own_line_pending(out: &mut String, ctx: EmitCtx<'_>, position: usize, indent: usize) {
    for c in ctx.nested {
        if c.position == position && !c.inline && c.container_path.as_slice() == ctx.path {
            push_comment_line(out, indent, &c.text);
        }
    }
}

/// Whether [`emit_own_line_pending`] has anything to write at `position`.
fn has_own_line_pending(ctx: EmitCtx<'_>, position: usize) -> bool {
    ctx.nested
        .iter()
        .any(|c| c.position == position && !c.inline && c.container_path.as_slice() == ctx.path)
}

/// Return the inline trailer for `position` in the context path. If multiple
/// inline comments share the slot, returns the first and emits the rest as
/// own-line.
fn find_inline_trailer<'a>(
    out: &mut String,
    ctx: EmitCtx<'a>,
    position: usize,
    indent: usize,
) -> Option<&'a str> {
    let mut chosen: Option<&str> = None;
    for c in ctx.nested {
        if c.position == position && c.inline && c.container_path.as_slice() == ctx.path {
            if chosen.is_none() {
                chosen = Some(c.text.as_str());
            } else {
                push_comment_line(out, indent, &c.text);
            }
        }
    }
    chosen
}

/// Emit orphan inline comments (`position >= container_len`) as own-line.
fn emit_orphan_inlines(out: &mut String, ctx: EmitCtx<'_>, container_len: usize, indent: usize) {
    for c in ctx.nested {
        if c.inline && c.position >= container_len && c.container_path.as_slice() == ctx.path {
            push_comment_line(out, indent, &c.text);
        }
    }
}

fn push_comment_line(out: &mut String, indent: usize, text: &str) {
    push_indent(out, indent);
    out.push_str("# ");
    out.push_str(text);
    out.push('\n');
}

fn push_trailer(out: &mut String, trailer: Option<&str>) {
    if let Some(t) = trailer {
        out.push_str(" # ");
        out.push_str(t);
    }
}

/// Emit a `key: <value>\n` pair with the key placed per `pos`.
///
/// Empty objects emit `key: {}\n`, empty arrays `key: []\n`. When `fill` is
/// `true`: scalars → `key: !must_fill <value>`, empty seqs → `key: !must_fill []`,
/// null → `key: !must_fill`, non-empty seqs → `key: !must_fill\n  - …`. A marked
/// mapping has no spelling, so every ingress refuses one
/// (`edit::validate_fill_targets`); one reaching here emits structurally, marker
/// dropped, rather than as a line no parser accepts.
fn emit_field_at(
    out: &mut String,
    key: &str,
    value: &JsonValue,
    pos: KeyPos,
    fill: bool,
    ctx: EmitCtx<'_>,
    inline_trailer: Option<&str>,
) {
    pos.write_key(out, key);
    if fill {
        match value {
            JsonValue::Null => {
                out.push_str(": !must_fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {
                out.push_str(": !must_fill ");
                emit_scalar(out, value);
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) if items.is_empty() => {
                out.push_str(": !must_fill []");
                push_trailer(out, inline_trailer);
                out.push('\n');
            }
            JsonValue::Array(items) => {
                out.push_str(": !must_fill");
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_sequence_children(out, items, pos.seq_indent(), ctx);
            }
            JsonValue::Object(map) => {
                out.push(':');
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_mapping_children(out, map, pos.map_indent(), ctx);
            }
        }
        return;
    }
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            out.push_str(": {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_mapping_children(out, map, pos.map_indent(), ctx);
        }
        JsonValue::Array(items) if items.is_empty() => {
            out.push_str(": []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(items) => {
            out.push(':');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, items, pos.seq_indent(), ctx);
        }
        _ => {
            out.push_str(": ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

fn emit_mapping_children(
    out: &mut String,
    map: &serde_json::Map<String, JsonValue>,
    child_indent: usize,
    ctx: EmitCtx<'_>,
) {
    for (i, (k, v)) in map.iter().enumerate() {
        emit_own_line_pending(out, ctx, i, child_indent);
        let trailer = find_inline_trailer(out, ctx, i, child_indent);
        let mut child_path = ctx.path.to_vec();
        child_path.push(PathSegment::Key(k.clone()));
        let child_fill = ctx.is_fill(&child_path);
        emit_field_at(
            out,
            k,
            v,
            KeyPos::Line(child_indent),
            child_fill,
            ctx.at(&child_path),
            trailer,
        );
    }
    emit_own_line_pending(out, ctx, map.len(), child_indent);
    emit_orphan_inlines(out, ctx, map.len(), child_indent);
}

fn emit_sequence_children(
    out: &mut String,
    items: &[JsonValue],
    base_indent: usize,
    ctx: EmitCtx<'_>,
) {
    for (i, item) in items.iter().enumerate() {
        emit_own_line_pending(out, ctx, i, base_indent);
        let trailer = find_inline_trailer(out, ctx, i, base_indent);
        let mut child_path = ctx.path.to_vec();
        child_path.push(PathSegment::Index(i));
        emit_sequence_item(out, item, base_indent, ctx.at(&child_path), trailer);
    }
    emit_own_line_pending(out, ctx, items.len(), base_indent);
    emit_orphan_inlines(out, ctx, items.len(), base_indent);
}

/// Emit a single `- <value>\n` sequence item. When the item is a mapping,
/// if both the seq-item trailer and the first key's trailer are present,
/// the inner one degrades to an own-line comment. A mapping carrying an
/// own-line comment before its first key takes the bare-dash form, the shape
/// the parser reads that comment back from.
fn emit_sequence_item(
    out: &mut String,
    value: &JsonValue,
    base_indent: usize,
    ctx: EmitCtx<'_>,
    inline_trailer: Option<&str>,
) {
    match value {
        JsonValue::Object(map) if map.is_empty() => {
            push_indent(out, base_indent);
            out.push_str("- {}");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Object(map) => {
            if has_own_line_pending(ctx, 0) {
                push_indent(out, base_indent);
                out.push('-');
                push_trailer(out, inline_trailer);
                out.push('\n');
                emit_mapping_children(out, map, base_indent + 2, ctx);
                return;
            }

            let mut first = true;
            for (i, (k, v)) in map.iter().enumerate() {
                if !first {
                    emit_own_line_pending(out, ctx, i, base_indent + 2);
                }
                let inner_trailer = find_inline_trailer(out, ctx, i, base_indent + 2);
                let mut child_path = ctx.path.to_vec();
                child_path.push(PathSegment::Key(k.clone()));
                let child_fill = ctx.is_fill(&child_path);
                if first {
                    let line_trailer = inline_trailer.or(inner_trailer);
                    push_indent(out, base_indent);
                    out.push_str("- ");
                    emit_field_at(
                        out,
                        k,
                        v,
                        KeyPos::SeqHead(base_indent),
                        child_fill,
                        ctx.at(&child_path),
                        line_trailer,
                    );
                    if let (Some(_), Some(loser)) = (inline_trailer, inner_trailer) {
                        push_comment_line(out, base_indent + 2, loser);
                    }
                    first = false;
                } else {
                    emit_field_at(
                        out,
                        k,
                        v,
                        KeyPos::Line(base_indent + 2),
                        child_fill,
                        ctx.at(&child_path),
                        inner_trailer,
                    );
                }
            }
            emit_own_line_pending(out, ctx, map.len(), base_indent + 2);
            emit_orphan_inlines(out, ctx, map.len(), base_indent + 2);
        }
        JsonValue::Array(inner) if inner.is_empty() => {
            push_indent(out, base_indent);
            out.push_str("- []");
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
        JsonValue::Array(inner) => {
            push_indent(out, base_indent);
            out.push('-');
            push_trailer(out, inline_trailer);
            out.push('\n');
            emit_sequence_children(out, inner, base_indent + 2, ctx);
        }
        _ => {
            push_indent(out, base_indent);
            out.push_str("- ");
            emit_scalar(out, value);
            push_trailer(out, inline_trailer);
            out.push('\n');
        }
    }
}

fn emit_scalar(out: &mut String, value: &JsonValue) {
    let s = saphyr_emit_scalar(value);
    out.push_str(&s);
}

/// Emit a *nested* mapping key through the same scalar path as values. Nested
/// keys are arbitrary user data, never name-validated, so one containing `:`/`#`,
/// a leading YAML indicator, edge whitespace, or a type-ambiguous form must be
/// quoted or the document re-parses to a different key.
fn emit_key(out: &mut String, key: &str) {
    out.push_str(&saphyr_emit_scalar(&JsonValue::String(key.to_string())));
}

/// Emit a mapping key at `indent`. Top-level field names (indent 0) are emitted
/// verbatim: the line-oriented prescan accepts only bare `[A-Za-z_][A-Za-z0-9_]*`
/// field names there, so quoting one would make it unparseable. Nested keys
/// (indent > 0) route through [`emit_key`] for correct YAML quoting.
fn emit_key_at(out: &mut String, key: &str, indent: usize) {
    if indent == 0 {
        out.push_str(key);
    } else {
        emit_key(out, key);
    }
}

fn saphyr_opts() -> SerializerOptions {
    serde_saphyr::ser_options! {
        prefer_block_scalars: false,
    }
}

pub(crate) fn saphyr_emit_scalar(value: &JsonValue) -> String {
    let mut buf = String::new();
    serde_saphyr::to_fmt_writer_with_options(&mut buf, value, saphyr_opts())
        .expect("saphyr scalar emission is infallible for JsonValue scalars");
    while buf.ends_with('\n') {
        buf.pop();
    }

    // Saphyr's emitter and parser disagree about which plain scalars are
    // string-safe: it emits `String`s unquoted that its own parser reads back as
    // a number (`_0` → 0) or as a different string (edge whitespace, which YAML
    // strips from plain scalars). So re-parse anything it emitted unquoted and
    // double-quote it ourselves unless it round-trips exactly. Edge whitespace
    // needs its own guard: a trailing Unicode-whitespace char survives the
    // isolated re-parse yet is still stripped in the real block context.
    if let JsonValue::String(s) = value {
        let unquoted = !buf.starts_with('"')
            && !buf.starts_with('\'')
            && !buf.starts_with('|')
            && !buf.starts_with('>');
        if unquoted {
            let has_edge_whitespace = !s.is_empty()
                && (s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace));
            let reparses_same = matches!(
                serde_saphyr::from_str::<JsonValue>(&buf),
                Ok(JsonValue::String(ref s2)) if s2 == s
            );
            if has_edge_whitespace || !reparses_same {
                return double_quote_string(s);
            }
        }
    }
    buf
}

/// JSON-style double-quoted fallback for strings saphyr would emit in a form
/// that loses bytes on parse (e.g. trailing-whitespace plain scalars).
fn double_quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (0x7F..=0x9F).contains(&(c as u32)) => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a `JsonValue` as a one-line YAML flow form (`[a, b]` / `{k: v}` /
/// flow-quoted scalar). Used for `# e.g.` hint lines in blueprint output.
pub(crate) fn saphyr_emit_flow(value: &JsonValue) -> String {
    let mut buf = String::new();
    let opts = saphyr_opts();
    match value {
        JsonValue::Array(items) => {
            let wrapped = FlowSeq(items.clone());
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow seq emission");
        }
        JsonValue::Object(map) => {
            let wrapped = FlowMap(map.clone());
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow map emission");
        }
        scalar => {
            // Wrap in FlowSeq so saphyr applies flow-context quoting, then strip `[`/`]`.
            let wrapped = FlowSeq(vec![scalar.clone()]);
            serde_saphyr::to_fmt_writer_with_options(&mut buf, &wrapped, opts)
                .expect("saphyr flow scalar emission");
            while buf.ends_with('\n') {
                buf.pop();
            }
            return buf
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(&buf)
                .to_string();
        }
    }
    while buf.ends_with('\n') {
        buf.pop();
    }
    buf
}

fn push_indent(out: &mut String, spaces: usize) {
    for _ in 0..spaces {
        out.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::QuillValue;

    fn assert_scalar_round_trips(value: serde_json::Value) {
        let mut yaml = String::from("~~~card-yaml\n$quill: q\n$kind: main\nv: ");
        yaml.push_str(&saphyr_emit_scalar(&value));
        yaml.push_str("\n~~~\n");
        let doc = crate::document::Document::parse(&yaml).unwrap_or_else(|e| {
            panic!(
                "failed to parse emitted scalar {:?}: {}\n{}",
                value, e, yaml
            )
        })
        .document;
        let parsed = doc.main().payload().get("v").expect("field 'v'").as_json();
        assert_eq!(
            parsed, &value,
            "scalar round-trip mismatch for {:?}: emitted as {:?}",
            value, yaml
        );
    }

    #[test]
    fn a_marked_content_cell_emits_its_markdown_projection() {
        // A canonical content object has no card-yaml spelling: emitted
        // unprojected it lands as a nested mapping that will not re-parse, and
        // drops the marker on the way.
        let mut payload = crate::document::Payload::new();
        payload.set_quill("q@1.0.0".parse().expect("reference"));
        payload.set_kind("main");
        let mut card = crate::document::Card::from_parts(
            payload,
            quillmark_content::Normalized::empty(),
        );
        let content = quillmark_content::import::from_markdown("Q3 results").expect("content");
        card.store_fill(
            "subject",
            QuillValue::from_json(quillmark_content::serial::to_canonical_value(&content)),
        )
        .expect("stored");

        let md = crate::document::Document::from_main_and_cards(card, Vec::new()).to_markdown();

        assert!(
            md.contains("subject: !must_fill Q3 results\n"),
            "the cell projects to markdown under its marker: {md}"
        );
        let reparsed = crate::document::Document::parse(&md).expect("re-parses").document;
        assert!(
            reparsed.main().payload().is_fill("subject"),
            "and the marker survives: {md}"
        );
    }

    #[test]
    fn saphyr_scalar_round_trips_ambiguous_strings() {
        for ambiguous in &[
            "on", "off", "yes", "no", "true", "false", "null", "~", "01234", "1e10",
        ] {
            assert_scalar_round_trips(serde_json::json!(*ambiguous));
        }
    }

    #[test]
    fn saphyr_scalar_round_trips_numeric_looking_strings() {
        // Saphyr's parser reads a leading-underscore-then-digits plain scalar
        // back as an integer (underscores are digit separators): `_0` → 0.
        for numericish in &["_0", "_1", "-_0", "__0"] {
            assert_scalar_round_trips(serde_json::json!(*numericish));
        }
    }

    #[test]
    fn string_underscore_zero_round_trips_via_document() {
        let src = "~~~card-yaml\n$quill: q\n$kind: main\na: \"_0\"\n~~~\n\nBody.\n";
        let doc = crate::document::Document::parse(src).expect("parse src").document;
        let emitted = doc.to_markdown();
        let reparsed =
            crate::document::Document::parse(&emitted).expect("re-parse emitted markdown").document;
        let value = reparsed
            .main()
            .payload()
            .get("a")
            .expect("field 'a'")
            .as_json();
        assert_eq!(
            value,
            &serde_json::Value::String("_0".to_string()),
            "String(\"_0\") must round-trip as a string; emitted:\n{}",
            emitted
        );
    }

    #[test]
    fn saphyr_scalar_round_trips_escapes() {
        assert_scalar_round_trips(serde_json::json!("a\\b\"c\nd\te"));
    }

    #[test]
    fn saphyr_scalar_round_trips_control_chars() {
        assert_scalar_round_trips(serde_json::json!("\x01\x1F"));
    }

    fn p(key: &str) -> Vec<PathSegment> {
        vec![PathSegment::Key(key.to_string())]
    }

    fn ctx(path: &[PathSegment]) -> EmitCtx<'_> {
        EmitCtx {
            path,
            ..EmitCtx::EMPTY
        }
    }

    #[test]
    fn empty_object_emitted() {
        let value = QuillValue::from_json(serde_json::json!({}));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "empty_map",
            value.as_json(),
            KeyPos::Line(0),
            false,
            ctx(&p("empty_map")),
            None,
        );
        assert_eq!(out, "empty_map: {}\n");
    }

    #[test]
    fn empty_object_keeps_inline_trailer() {
        let value = QuillValue::from_json(serde_json::json!({}));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "empty_map",
            value.as_json(),
            KeyPos::Line(0),
            false,
            ctx(&p("empty_map")),
            Some("orphan"),
        );
        assert_eq!(out, "empty_map: {} # orphan\n");
    }

    #[test]
    fn empty_array_emitted() {
        let value = QuillValue::from_json(serde_json::json!([]));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "empty_seq",
            value.as_json(),
            KeyPos::Line(0),
            false,
            ctx(&p("empty_seq")),
            None,
        );
        assert_eq!(out, "empty_seq: []\n");
    }

    #[test]
    fn scalar_field_with_inline_trailer() {
        let value = QuillValue::from_json(serde_json::json!("Hello"));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "title",
            value.as_json(),
            KeyPos::Line(0),
            false,
            ctx(&p("title")),
            Some("greeting"),
        );
        assert_eq!(out, "title: Hello # greeting\n");
    }

    #[test]
    fn container_field_with_inline_trailer_lands_on_key_line() {
        let value = QuillValue::from_json(serde_json::json!({"inner": 1}));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "outer",
            value.as_json(),
            KeyPos::Line(0),
            false,
            ctx(&p("outer")),
            Some("note"),
        );
        assert_eq!(out, "outer: # note\n  inner: 1\n");
    }

    #[test]
    fn fill_null_emits_bare_tag() {
        let value = QuillValue::from_json(serde_json::Value::Null);
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "recipient",
            value.as_json(),
            KeyPos::Line(0),
            true,
            ctx(&p("recipient")),
            None,
        );
        assert_eq!(out, "recipient: !must_fill\n");
    }

    #[test]
    fn fill_string_emits_tag_with_value() {
        let value = QuillValue::from_json(serde_json::json!("placeholder"));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "dept",
            value.as_json(),
            KeyPos::Line(0),
            true,
            ctx(&p("dept")),
            None,
        );
        assert_eq!(out, "dept: !must_fill placeholder\n");
    }

    #[test]
    fn fill_with_inline_trailer() {
        let value = QuillValue::from_json(serde_json::json!("placeholder"));
        let mut out = String::new();
        emit_field_at(
            &mut out,
            "dept",
            value.as_json(),
            KeyPos::Line(0),
            true,
            ctx(&p("dept")),
            Some("note"),
        );
        assert_eq!(out, "dept: !must_fill placeholder # note\n");
    }
}
