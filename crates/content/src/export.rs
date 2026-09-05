//! Markdown export: content → markdown, per island type.
//!
//! Marks become syntax; identity ([`MarkKind::Anchor`]) marks are **omitted**,
//! surviving across edits via diff-rebase rather than the projection. The
//! contract is the **content fixed point**: for a content `rt` from
//! [`crate::import::from_markdown`], `from_markdown(to_markdown(rt)) == rt`
//! modulo island loss class. Markdown source is not canonical; the content is.
//!
//! ## Export is defined by import
//!
//! The `render_marked_core` safety net settles a line's markdown by re-parsing
//! it with [`crate::import::from_markdown`] and dropping marks until the text
//! comes back intact. CommonMark's emphasis algorithm (delimiter-run matching,
//! the rule of 3, `\*`-escape adjacency) has corners no local rule captures, and
//! a local approximation drops good marks on the cases it gets wrong; verifying
//! against the parser is the only check exactly as strict as the format. The
//! cost is that an importer change moves exporter output, and that every
//! [`to_markdown`] call depends on `pulldown_cmark`.
//!
//! ## Documented codec limits
//!
//! Four shapes markdown cannot represent do **not** round-trip. A mark spanning
//! a hard break splits into two per-line marks, and an empty first line in a
//! hard-break block is dropped, markdown having no blank-then-forced-break
//! syntax (`tests::known_hard_break_limits`). An image `alt` loses its edge
//! whitespace: the parser trims alt *after* decoding the character reference
//! that carries an edge run everywhere else (`tests::known_image_alt_limit`).
//! A link or image `url` carrying a line ending comes back percent-encoded
//! (`%0A`/`%0D`): CommonMark admits none in a destination, and the import
//! decodes no percent escape
//! (`tests::a_url_carrying_a_line_ending_still_projects_as_a_link`). Each arises
//! only from adversarial delimiter/break placement, a hand-built island prop, or
//! a stored url the authored lanes refuse.

use crate::island::KnownIslandType;
use crate::model::{Container, Island, LineKind, Mark, MarkKind, Content, Normalized, ISLAND_SLOT};

/// Render a content to markdown. An island projects by **type**: a type this
/// build knows emits its markdown, any other a placeholder comment.
///
/// [`Loss`](crate::model::Loss) does not gate the emit and is not read here: it
/// *describes* fidelity for a consumer to surface, while the type decides
/// whether a projection exists at all. So a known type stamped with a loss class
/// this build lacks still emits its table.
pub fn to_markdown(rt: &Normalized) -> String {
    let segments = line_segments(rt);
    let ctx = Ctx {
        rt,
        segments: &segments,
    };
    let mut out = String::new();
    emit_block(&ctx, 0..rt.lines.len(), 0, &mut out);
    // `to_markdown` projects a *value*, not a file: it emits no final newline,
    // so `writer.set("subject", "Hello")` reads back as `"Hello"`. Document-file
    // writers own the file-final newline, and import is newline-insensitive, so
    // dropping it is round-trip-invisible.
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Render a content to plaintext: [`Content::text`] with island slots
/// ([`ISLAND_SLOT`]) removed. The lossy sibling of [`to_markdown`]: it drops
/// every mark and island, keeping only literal text.
///
/// Tables and images having no plaintext form is a **decided limitation**: the
/// pdfform backend fills a form field from this projection, so a field bound to
/// a table-bearing content renders the surrounding text and silently omits the
/// table, rather than emitting a row/tab dump that would read as faithful.
pub fn to_plaintext(rt: &Content) -> String {
    rt.text.chars().filter(|&c| c != ISLAND_SLOT).collect()
}

struct Ctx<'a> {
    rt: &'a Content,
    segments: &'a [Segment],
}

/// One line's char range `[start, end)` into the content, with the matching byte
/// range and the count of island slots before the line, so a caller indexes
/// the content text and the island list in O(1) without rescanning.
///
/// **Deliberately not `#[non_exhaustive]`**, unlike the model types it indexes:
/// it is a derived view that [`line_segments`] recomputes whole, and its five
/// fields are one line's position in the two coordinate spaces the model has. A
/// sixth field means a third space, which moves the model first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// USV index of the line's first char.
    pub start: usize,
    /// USV index one past the line's last char (its `\n`, or the content end).
    pub end: usize,
    /// Byte offset of `start` in the content text.
    pub byte_start: usize,
    /// Byte offset of `end` in the content text.
    pub byte_end: usize,
    /// Count of [`ISLAND_SLOT`] chars before `start`.
    pub slots_before: usize,
}

/// Per-line char/byte ranges and slot prefixes over a content, in line order.
pub fn line_segments(rt: &Content) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(rt.lines.len());
    let mut start = 0usize;
    let mut byte_start = 0usize;
    let mut slots_before = 0usize;
    let mut line_slots = 0usize;
    let mut pos = 0usize;
    for (b, c) in rt.text.char_indices() {
        if c == '\n' {
            segs.push(Segment {
                start,
                end: pos,
                byte_start,
                byte_end: b,
                slots_before,
            });
            start = pos + 1;
            byte_start = b + 1; // `\n` is one byte
            slots_before += line_slots;
            line_slots = 0;
        } else if c == ISLAND_SLOT {
            line_slots += 1;
        }
        pos += 1;
    }
    segs.push(Segment {
        start,
        end: pos,
        byte_start,
        byte_end: rt.text.len(),
        slots_before,
    });
    // Defensive: a malformed content (lines.len() != segments) still gets one
    // segment per line so indexing never panics.
    let total_slots = slots_before + line_slots;
    while segs.len() < rt.lines.len() {
        segs.push(Segment {
            start: pos,
            end: pos,
            byte_start: rt.text.len(),
            byte_end: rt.text.len(),
            slots_before: total_slots,
        });
    }
    segs
}

/// One open level of the block walk: `at` is the next line of `range` to emit,
/// `buf` the markdown emitted for the level so far, and `container` the one
/// whose syntax prefixes `buf` when the level closes (`None` at the root).
struct Frame<'a> {
    container: Option<&'a Container>,
    at: usize,
    range: std::ops::Range<usize>,
    depth: usize,
    first_block: bool,
    buf: String,
}

impl<'a> Frame<'a> {
    fn open(container: Option<&'a Container>, range: std::ops::Range<usize>, depth: usize) -> Self {
        Frame {
            container,
            at: range.start,
            range,
            depth,
            first_block: true,
            buf: String::new(),
        }
    }
}

/// Emit the lines in `range`, all sharing the container prefix of length
/// `depth`. Leaf lines (containers.len() == depth) render at this level;
/// deeper lines are grouped by their `depth`-th container and open a level of
/// their own, which [`close_container`] prefixes on the way back out.
///
/// A frame stack, not recursion, for the reason [`Normalized`] states: the
/// token says canonical, not valid, so a container path reaching here nests
/// past [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH) freely, and a call
/// frame per level aborts the process where this returns a projection.
fn emit_block(ctx: &Ctx, range: std::ops::Range<usize>, depth: usize, out: &mut String) {
    let lines = &ctx.rt.lines;
    let mut stack = vec![Frame::open(None, range, depth)];
    while let Some(frame) = stack.last_mut() {
        if frame.at < frame.range.end {
            let i = frame.at;
            block_separator(&mut frame.buf, frame.first_block);
            frame.first_block = false;
            if lines[i].containers.len() > frame.depth {
                let item = crate::traverse::items(lines, i..frame.range.end, frame.depth)
                    .next()
                    .expect("a line with a container at `depth` opens an item");
                frame.at = item.range.end;
                let child = Frame::open(Some(item.container), item.range, frame.depth + 1);
                stack.push(child);
            } else {
                let seg = crate::traverse::segment(lines, i..frame.range.end, frame.depth);
                frame.at = seg.end;
                emit_leaf_block(ctx, seg, &mut frame.buf);
            }
            continue;
        }
        let done = stack.pop().expect("`last_mut` just yielded this frame");
        match stack.last_mut() {
            Some(parent) => {
                let key = done
                    .container
                    .expect("only the root frame opens without a container");
                close_container(key, &done.buf, &mut parent.buf);
            }
            None => out.push_str(&done.buf),
        }
    }
}

fn block_separator(out: &mut String, first_block: bool) {
    if !first_block {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
}

/// Close a container level: prefix each line of `inner`, the block emitted for
/// it, with the container's markdown syntax.
fn close_container(key: &Container, inner: &str, out: &mut String) {
    match key {
        Container::ListItem {
            ordered,
            start,
            ordinal,
            instance,
        } => {
            // CommonMark starts a new list at a change of bullet char or of
            // ordered delimiter, and stores neither, so the marker is where the
            // `instance` alternation lands: it spells two adjacent lists apart
            // in the projection exactly as `instance` does in the model.
            //
            // `+` rather than `*` for the second bullet: `* ***` is four
            // asterisks separated by spaces, which reads as a thematic break
            // and would take the item with it (see
            // `rule_opening_a_list_item_keeps_its_item`).
            let marker = if *ordered {
                // `start`/`ordinal` are unbounded `u64` and `validate` does not
                // ceiling them, so a corrupt/adversarial content can drive the
                // sum past `u64::MAX`; saturate rather than panic (or wrap under
                // release overflow-checks) on the render path.
                let n = start.saturating_add(*ordinal);
                if instance % 2 == 0 {
                    format!("{n}. ")
                } else {
                    format!("{n}) ")
                }
            } else if instance % 2 == 0 {
                "- ".to_string()
            } else {
                "+ ".to_string()
            };
            let indent = " ".repeat(marker.len());
            prefix_lines(inner, &marker, &indent, out);
        }
        Container::Quote { .. } => {
            // `> ` on content lines, `>` on blank lines so paragraphs stay in
            // one quote on re-import.
            prefix_quote(inner, out);
        }
        // A container this build does not know has no markdown syntax to prefix
        // with, so it projects transparently and survives via storage.
        Container::Unknown { .. } => out.push_str(inner),
    }
}

/// Prefix the first produced line with `first`, the rest with `cont`.
fn prefix_lines(inner: &str, first: &str, cont: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx == 0 {
            out.push_str(first);
            out.push_str(line);
        } else {
            out.push('\n');
            if line.is_empty() {
                // blank continuation line: no trailing indent
            } else {
                out.push_str(cont);
                out.push_str(line);
            }
        }
    }
}

fn prefix_quote(inner: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            out.push('>');
        } else {
            out.push_str("> ");
            out.push_str(line);
        }
    }
}

fn emit_code(ctx: &Ctx, range: std::ops::Range<usize>, lang: Option<&str>, out: &mut String) {
    // Choose a fence long enough to not collide with backtick runs in content.
    let mut max_ticks = 0usize;
    for i in range.clone() {
        max_ticks = max_ticks.max(longest_backtick_run(seg_str(ctx, i)));
    }
    let fence = "`".repeat(max_ticks.max(2) + 1);
    out.push_str(&fence);
    if let Some(l) = lang {
        out.push_str(l);
    }
    out.push('\n');
    for i in range {
        out.push_str(seg_str(ctx, i));
        out.push('\n');
    }
    out.push_str(&fence);
}

/// Emit one leaf block: the lines `range.start` (a block start) plus any
/// continuation lines. A paragraph block joins its lines with a markdown hard
/// break (`\` + newline); a code block renders one fence; a heading/island is a
/// single line.
fn emit_leaf_block(ctx: &Ctx, range: std::ops::Range<usize>, out: &mut String) {
    let first = &ctx.rt.lines[range.start];
    match &first.kind {
        LineKind::Code { lang } => emit_code(ctx, range, lang.as_deref(), out),
        LineKind::Island => {
            if let Some(isl) = slot_island(ctx, range.start) {
                emit_island(isl, out);
            }
        }
        LineKind::Heading { level } => {
            for _ in 0..*level {
                out.push('#');
            }
            out.push(' ');
            // Headings never carry continuations (import maps a hard break in a
            // heading to a space), so only the first line contributes.
            let mut inline = render_inline(ctx, range.start, false);
            // A trailing `#` run reads as an ATX closing sequence on re-import
            // (`# a #` → heading text "a"). One escaped hash defeats the whole
            // closer, and `\#` re-imports as a literal `#`.
            if inline.ends_with('#') {
                inline.pop();
                inline.push_str("\\#");
            }
            out.push_str(&inline);
        }
        // An unknown block role projects as a paragraph: the role is lost to
        // markdown (it round-trips through storage), the text is not.
        LineKind::Para | LineKind::Unknown { .. } => {
            let parts: Vec<String> = range.map(|i| render_inline(ctx, i, true)).collect();
            out.push_str(&parts.join("\\\n"));
        }
        // `***`, not `---`: all three break spellings import alike, but `---` is
        // also a setext underline, a front-matter fence, and — with the spaces a
        // list prefix supplies — a bullet line, so `- ` + `---` re-imports as a
        // top-level break, losing the item. `***` survives every container
        // prefix.
        LineKind::Rule => out.push_str("***"),
    }
}

fn seg_str<'a>(ctx: &'a Ctx, i: usize) -> &'a str {
    let seg = &ctx.segments[i];
    &ctx.rt.text[seg.byte_start..seg.byte_end]
}

/// The island backing the single slot on a block-island line `i`.
fn slot_island<'a>(ctx: &'a Ctx, i: usize) -> Option<&'a Island> {
    ctx.rt.islands.get(ctx.segments[i].slots_before)
}

fn emit_island(isl: &Island, out: &mut String) {
    match KnownIslandType::parse(&isl.island_type) {
        Some(KnownIslandType::Table) => emit_table(isl, out),
        Some(KnownIslandType::Image) => emit_image(isl, out),
        None => {
            // A comment placeholder re-imports as no content text (HTML comments
            // are stripped), so the island survives via storage instead.
            out.push_str("<!-- island:");
            // The type is an open wire string no lane constrains: a `-->` closes
            // the comment early and a line break ends the HTML block, either way
            // leaking the rest as content text. Nothing reads the type back out
            // of markdown.
            for c in isl.island_type.chars() {
                match c {
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    c if c.is_control() => out.push(' '),
                    c => out.push(c),
                }
            }
            out.push_str(" -->");
        }
    }
}

fn emit_table(isl: &Island, out: &mut String) {
    let header = isl.props.get("header").and_then(|v| v.as_array());
    let rows = isl.props.get("rows").and_then(|v| v.as_array());
    let aligns = isl.props.get("aligns").and_then(|v| v.as_array());
    let cols = header.map(|h| h.len()).unwrap_or(0);
    if cols == 0 {
        return;
    }
    // Cells are canonical `{text, marks}`; each cell's markdown is rebuilt from
    // that structure, so nothing re-parses markdown and `import(export(table))`
    // is a fixed point.
    out.push_str("| ");
    if let Some(h) = header {
        out.push_str(&h.iter().map(render_cell_md).collect::<Vec<_>>().join(" | "));
    }
    out.push_str(" |\n|");
    for k in 0..cols {
        let a = aligns
            .and_then(|a| a.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        out.push_str(match a {
            "left" => " :--- |",
            "center" => " :---: |",
            "right" => " ---: |",
            _ => " --- |",
        });
    }
    if let Some(rs) = rows {
        for row in rs {
            if let Some(r) = row.as_array() {
                // Pad/truncate so a ragged island (one that skipped
                // normalization) still emits a rectangular table.
                let mut cells: Vec<String> = r.iter().map(render_cell_md).collect();
                cells.resize(cols, String::new());
                out.push_str("\n| ");
                out.push_str(&cells.join(" | "));
                out.push_str(" |");
            }
        }
    }
}

fn emit_image(isl: &Island, out: &mut String) {
    let url = isl.props.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let alt = isl.props.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    // Alt is inline content of `![…]`: escape it like a link's display text so a
    // `]`/`\`/`&`/delimiter can't terminate the markup or decode on re-import.
    out.push_str("![");
    out.push_str(&escape_run(&alt.chars().collect::<Vec<_>>(), false));
    out.push_str("](");
    emit_url(url, out);
    out.push(')');
}

/// Emit a link/image destination. A bare (unbracketed) destination round-trips
/// only for CommonMark's unbracketed form: no whitespace, control char, `<`,
/// `>`, `\`, or `&`, and balanced parentheses. Anything else is angle-wrapped,
/// with `<`, `>`, `\` and `&` backslash-escaped, since unescaped `<`/`>` are
/// illegal even inside the wrap and `\`/`&` would be consumed as an escape or
/// entity reference on re-import.
///
/// Every character re-imports to itself except the two
/// [`url_is_writable`] refuses: a line ending is illegal inside the wrap too, so
/// it is percent-encoded and comes back in that form.
fn emit_url(url: &str, out: &mut String) {
    if url_is_bare_safe(url) {
        out.push_str(url);
        return;
    }
    out.push('<');
    for c in url.chars() {
        match c {
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            '<' | '>' | '\\' | '&' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('>');
}

/// Whether a link or image `url` survives [`to_markdown`] unchanged. CommonMark
/// admits no line ending in a destination, bare or angle-wrapped, so an authored
/// lane that stores a url refuses one
/// ([`crate::serial::reject_unwritable_link_url`],
/// [`crate::serial::island_from_authored_value`]) and [`emit_url`]
/// percent-encodes the one a storage-lane or hand-built content still holds.
pub(crate) fn url_is_writable(url: &str) -> bool {
    !url.contains(['\n', '\r'])
}

fn url_is_bare_safe(url: &str) -> bool {
    let mut depth: i32 = 0;
    for c in url.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            '<' | '>' | '\\' | '&' => return false,
            c if c.is_whitespace() || c.is_control() => return false,
            _ => {}
        }
    }
    depth == 0
}

fn render_inline(ctx: &Ctx, i: usize, escape_leading_block: bool) -> String {
    let seg = &ctx.segments[i];
    let line_start = seg.start;
    let text = seg_str(ctx, i);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();

    let (code_ranges, fmt, links) = bucket_marks(&ctx.rt.marks, line_start, n, false);

    // Leading ordered-list marker: a line whose text starts `<digits>.` or
    // `<digits>)` would re-import as an ordered list, so escape that punctuation.
    let escape_punct_at = if escape_leading_block {
        let lead_digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
        if lead_digits > 0 && lead_digits < n && matches!(chars[lead_digits], '.' | ')') {
            Some(lead_digits)
        } else {
            None
        }
    } else {
        None
    };

    let slots_before_line = seg.slots_before;
    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        escape_punct_at,
        escape_leading_block,
        false, // prose text does not escape `|`
        |pos_local| {
            let before = slots_before_line
                + chars[..pos_local]
                    .iter()
                    .filter(|&&c| c == ISLAND_SLOT)
                    .count();
            ctx.rt.islands.get(before).map(|isl| {
                let mut markup = String::new();
                emit_island(isl, &mut markup);
                SlotMarkup {
                    markup,
                    projects: KnownIslandType::parse(&isl.island_type).is_some(),
                }
            })
        },
    )
}

/// Route `marks` into the three lists [`render_marked_core`] takes, each range
/// clipped to the `n` chars starting at `line_start` and rebased to local
/// offsets. Anchors are dropped: the projection has no syntax for them.
/// `drop_empty` also drops a range the clip left empty, which a cell wants and
/// a prose line does not.
fn bucket_marks(
    marks: &[Mark],
    line_start: usize,
    n: usize,
    drop_empty: bool,
) -> (
    Vec<(usize, usize)>,
    Vec<(usize, usize, &MarkKind)>,
    Vec<(usize, usize, &str)>,
) {
    let mut code_ranges: Vec<(usize, usize)> = Vec::new();
    let mut fmt: Vec<(usize, usize, &MarkKind)> = Vec::new();
    let mut links: Vec<(usize, usize, &str)> = Vec::new();
    for m in marks {
        if m.end <= line_start || m.start >= line_start + n {
            continue;
        }
        let s = m.start.saturating_sub(line_start).min(n);
        let e = m.end.saturating_sub(line_start).min(n);
        if drop_empty && s >= e {
            continue;
        }
        match &m.kind {
            MarkKind::Anchor { .. } => {}
            MarkKind::Code => code_ranges.push((s, e)),
            MarkKind::Link { url } => links.push((s, e, url.as_str())),
            _ => fmt.push((s, e, &m.kind)),
        }
    }
    (code_ranges, fmt, links)
}

/// One island slot's markdown, and whether re-importing that markdown yields a
/// slot back. A type with no markdown projection renders as a placeholder
/// comment, which re-imports as no content text.
struct SlotMarkup {
    markup: String,
    projects: bool,
}

/// Render marks over a standalone char slice to markdown: the projection's mark
/// boundary sweep, shared by prose lines and table cells. `code_ranges`/`fmt`/
/// `links` are the marks clipped to `chars` (local offsets); `escape_pipe` adds
/// `|`→`\|` for cells; `island_markup_at` renders an island slot (prose) or
/// yields `None` (cells carry no slot).
///
/// The model permits free (Peritext-style) overlap but markdown syntax nests.
/// The sweep closes every mark ending at a boundary and reopens the deeper
/// survivors, so a partial overlap lowers to balanced markdown
/// (`**ab~~cd~~**~~ef~~`), which re-imports to the same content for marks with
/// *distinct* delimiters. Two shapes the sweep can't express are clipped first:
///
/// - **Atomic spans** (`code`/`link`) can't carry a partial wrap, and the
///   sweep's cursor jumps their interior, so a wrap edge inside would be left
///   unbalanced. [`clip_fmt_to_atomic`] pulls such edges to the span boundary.
/// - **`*`/`**` share a delimiter character**, so a reopened `*` abutting a `**`
///   merges into an ambiguous `***` run CommonMark re-segments wrong.
///   [`clip_asterisk_overlap`] nests the two by truncation: a documented codec
///   limit, keeping the text but losing the crossing tail.
#[allow(clippy::too_many_arguments)]
fn render_marked_core(
    chars: &[char],
    code_ranges: &[(usize, usize)],
    fmt: &[(usize, usize, &MarkKind)],
    links: &[(usize, usize, &str)],
    escape_punct_at: Option<usize>,
    escape_leading_block: bool,
    escape_pipe: bool,
    island_markup_at: impl Fn(usize) -> Option<SlotMarkup>,
) -> String {
    let n = chars.len();

    // A code span is emitted verbatim between backticks, so a slot inside it
    // would land raw and re-import as nothing, taking the island with it.
    // Markdown has no honest encoding (`` `![x](y)` `` is *literal* text), so
    // split each code range around every slot it covers and let the island
    // render between the surviving code spans.
    let mut code_ranges: Vec<(usize, usize)> = code_ranges
        .iter()
        .flat_map(|&(s, e)| split_around_slots(chars, s, e))
        .collect();
    // The sweep reaches the atomic spans with a cursor, so both lists are put in
    // start order here rather than re-scanned at every position.
    code_ranges.sort_unstable();
    let code_ranges = &code_ranges[..];
    let mut links: Vec<(usize, usize, &str)> = links.to_vec();
    links.sort_by_key(|&(s, _, _)| s);
    let links = &links[..];

    // Clip the wrapping marks so the sweep only ever sees a representable shape.
    let mut fmt: Vec<(usize, usize, &MarkKind)> = fmt.to_vec();
    let mut atomics: Vec<(usize, usize)> = code_ranges.to_vec();
    atomics.extend(links.iter().map(|(s, e, _)| (*s, *e)));
    clip_fmt_to_atomic(&mut fmt, &atomics);
    clip_asterisk_overlap(&mut fmt);

    // The slice's edge whitespace runs, found on the content chars: the heading,
    // quote, or list prefix and the hard break's `\` land around them later. An
    // all-whitespace slice is one leading run.
    let lead_edge = chars
        .iter()
        .take_while(|c| edge_space_ref(**c).is_some())
        .count();
    let trail_edge = n - chars[lead_edge..]
        .iter()
        .rev()
        .take_while(|c| edge_space_ref(**c).is_some())
        .count();

    // `fmt` by start, longest span (outer) first at a tie. `pos` only ever
    // advances, so one cursor over this order opens every mark exactly once.
    // Built once here, not per sweep: the net below sweeps the same `fmt` up to
    // `PROBE_BUDGET` times.
    let mut by_start: Vec<usize> = (0..fmt.len()).collect();
    by_start.sort_by(|&a, &b| fmt[a].0.cmp(&fmt[b].0).then(fmt[b].1.cmp(&fmt[a].1)));

    // One mark sweep over the marks `keep` selects (indices into `fmt`) → inline
    // markdown.
    let sweep = |keep: &[bool]| -> String {
        let mut out = String::new();
        // Marks currently open, outermost first. Storing the `fmt` index (not
        // `(end, kind)`) keeps each open mark's identity, so a reopened mark
        // re-emits its OWN delimiter.
        let mut stack: Vec<usize> = Vec::new();
        let (mut oi, mut li, mut ci) = (0usize, 0usize, 0usize);
        let mut pos = 0usize;
        while pos <= n {
            if let Some(idx) = stack.iter().position(|&fi| fmt[fi].1 == pos) {
                let mut reopen: Vec<usize> = Vec::new();
                while stack.len() > idx {
                    let fi = stack.pop().unwrap();
                    out.push_str(delim_close(fmt[fi].2));
                    if fmt[fi].1 != pos {
                        reopen.push(fi);
                    }
                }
                for fi in reopen.into_iter().rev() {
                    out.push_str(delim_open(fmt[fi].2));
                    stack.push(fi);
                }
            }
            // Open formatting marks starting here BEFORE any atomic run, so a
            // formatting mark beginning at the same position as inline code/link
            // still wraps it rather than being dropped. A mark whose start the
            // cursor already passed was jumped over by an atomic span's interior
            // and never opens; clipping keeps that set empty.
            while oi < by_start.len() && fmt[by_start[oi]].0 < pos {
                oi += 1;
            }
            while oi < by_start.len() && fmt[by_start[oi]].0 == pos {
                let fi = by_start[oi];
                oi += 1;
                if !keep[fi] {
                    continue;
                }
                out.push_str(delim_open(fmt[fi].2));
                stack.push(fi);
            }
            // A link is emitted atomically as [text](url). Nested marks in link
            // text are not supported, but islands are (a linked image is
            // `[![alt](src)](url)`), so each char routes through
            // `island_markup_at`; emitting the slice raw would leak the slot.
            while li < links.len() && links[li].0 < pos {
                li += 1;
            }
            if let Some(&(ls, le, url)) = links.get(li).filter(|l| l.0 == pos) {
                out.push('[');
                for (i, &c) in chars[ls..le].iter().enumerate() {
                    if c == ISLAND_SLOT {
                        if let Some(slot) = island_markup_at(ls + i) {
                            out.push_str(&slot.markup);
                        }
                    } else {
                        escape_char_into(c, i == 0, escape_pipe, &mut out);
                    }
                }
                out.push_str("](");
                emit_url(url, &mut out);
                out.push(')');
                pos = le;
                continue;
            }
            // A code range is atomic.
            while ci < code_ranges.len() && code_ranges[ci].0 < pos {
                ci += 1;
            }
            if let Some(&(cs, ce)) = code_ranges.get(ci).filter(|r| r.0 == pos) {
                let content: String = chars[cs..ce].iter().collect();
                let ticks = longest_backtick_run(&content) + 1;
                let fence = "`".repeat(ticks.max(1));
                // CommonMark folds an edge backtick into the fence and strips
                // one space off each side of a span that begins and ends with
                // one, unless it is all spaces; the pad defeats both.
                let pad = content.starts_with('`')
                    || content.ends_with('`')
                    || (content.starts_with(' ')
                        && content.ends_with(' ')
                        && content.chars().any(|c| c != ' '));
                out.push_str(&fence);
                if pad {
                    out.push(' ');
                }
                out.push_str(&content);
                if pad {
                    out.push(' ');
                }
                out.push_str(&fence);
                pos = ce;
                continue;
            }
            if pos < n {
                let c = chars[pos];
                if c == ISLAND_SLOT {
                    if let Some(slot) = island_markup_at(pos) {
                        out.push_str(&slot.markup);
                    }
                } else if Some(pos) == escape_punct_at {
                    out.push('\\');
                    out.push(c);
                } else if let Some(esc) = edge_space_ref(c)
                    && (pos < lead_edge || pos >= trail_edge)
                {
                    out.push_str(esc);
                } else {
                    escape_char_into(c, pos == 0 && escape_leading_block, escape_pipe, &mut out);
                }
            }
            pos += 1;
        }
        // Clipping keeps every wrap `end` reachable, so this normally drains
        // nothing.
        while let Some(fi) = stack.pop() {
            out.push_str(delim_close(fmt[fi].2));
        }
        out
    };

    // Verify-and-drop safety net. An editor's `apply_mark_ops` can build a mark
    // over a span markdown can't represent, and it lowers to a `**`/`*`/`~~` run
    // pulldown re-reads as literal text, leaking a delimiter into the content.
    // Re-parse the rendered line and, if its plain text drifted, search for a set
    // of flanking marks that keeps the text intact.
    let is_flanking = |k: &MarkKind| {
        matches!(k, MarkKind::Strong | MarkKind::Emph | MarkKind::Strike)
    };
    let out = sweep(&vec![true; fmt.len()]);
    if !fmt.iter().any(|m| is_flanking(m.2)) {
        return out;
    }
    // The probe wraps the fragment in `,…,`: parsed standalone, a leading `0. ` /
    // `# ` / `> ` would read as a list/heading/quote marker and drop a good mark.
    // A punctuation sentinel blocks every leading-block construct, preserves edge
    // whitespace, and is flanking-equivalent to the line start/end it replaces,
    // so it never masks or invents a leak.
    //
    // A slot whose island has no markdown projection re-imports as nothing, so
    // the expected text drops it; every other slot stays, so the probe still
    // catches a leak that eats an island's markup.
    let expected: String = chars
        .iter()
        .enumerate()
        .filter(|&(i, &c)| c != ISLAND_SLOT || island_markup_at(i).is_some_and(|s| s.projects))
        .map(|(_, c)| c)
        .collect();
    let want = format!(",{expected},");
    let text_safe = |md: &str| {
        crate::import::from_markdown(&format!(",{md},"))
            .map(|rt| rt.text == want)
            .unwrap_or(false)
    };
    if text_safe(&out) {
        return out;
    }
    // The flanking marks in document order. That order is the re-add priority
    // below (when two marks can't both survive, the earlier one wins) and is the
    // one user-visible choice in this search, so it is fixed here rather than
    // falling out of the traversal.
    let cands: Vec<usize> = (0..fmt.len()).filter(|&i| is_flanking(fmt[i].2)).collect();
    // Render with only the flanking marks in `keep`; every other mark rides.
    let render = |keep: &[usize]| -> String {
        let mut mask: Vec<bool> = fmt.iter().map(|m| !is_flanking(m.2)).collect();
        for &i in keep {
            mask[i] = true;
        }
        sweep(&mask)
    };
    // Drop the whole flanking set, then re-add by halves: a chunk that stays
    // text-safe is accepted whole, one that doesn't splits and its halves are
    // retried, a lone mark that still leaks is dropped. `m` marks cost ~2·log(m)
    // probes when one is at fault and 2 when none can survive. Greedy, so the
    // result is a maximal text-safe set, not necessarily the largest one.
    //
    // Dropping every flanking mark is the floor the search can't go below, so if
    // even that leaks, no re-add can fix it. A lone candidate has nowhere to
    // split, so the floor is already its answer.
    let mut out = render(&[]);
    if cands.len() == 1 || !text_safe(&out) {
        return out;
    }
    let mut kept: Vec<usize> = Vec::new();
    // A chunk `(lo, hi)` and the `kept` length at which its trial is *already
    // known to fail*: a right half popped with every mark of its left sibling
    // accepted re-renders exactly the chunk their parent failed on, so it splits
    // without spending a probe.
    let mid = cands.len() / 2;
    let mut work: Vec<(usize, usize, usize)> = vec![(mid, cands.len(), mid), (0, mid, usize::MAX)];
    let mut budget = PROBE_BUDGET;
    while let Some((lo, hi, known_bad_at)) = work.pop() {
        let split = |work: &mut Vec<_>, kept_len: usize| {
            if hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                work.push((mid, hi, kept_len + (mid - lo)));
                work.push((lo, mid, usize::MAX));
            }
        };
        if known_bad_at == kept.len() {
            split(&mut work, kept.len());
            continue;
        }
        if budget == 0 {
            break;
        }
        budget -= 1;
        // `work` is a left-to-right DFS, so every accepted mark precedes this
        // chunk and `trial` stays in document order.
        let trial: Vec<usize> = kept.iter().chain(&cands[lo..hi]).copied().collect();
        let md = render(&trial);
        if text_safe(&md) {
            kept = trial;
            out = md;
        } else {
            split(&mut work, kept.len());
        }
    }
    out
}

/// Probes the verify-and-drop net will spend on one line before giving up and
/// dropping the flanking marks it hasn't cleared. Resolving `m` of them exactly
/// costs at most `2m-2` probes, so at 64 no line carrying 32 or fewer loses a
/// mark to the budget. The ceiling keeps export cost linear in document size
/// whatever marks are thrown at it.
const PROBE_BUDGET: usize = 64;

/// [`clip_range_to_atomic`] over every wrapping mark, dropping the ones an
/// atomic span swallowed whole.
fn clip_fmt_to_atomic(fmt: &mut Vec<(usize, usize, &MarkKind)>, atomics: &[(usize, usize)]) {
    for m in fmt.iter_mut() {
        clip_range_to_atomic(&mut m.0, &mut m.1, atomics);
    }
    fmt.retain(|m| m.0 < m.1);
}

/// The atomic-balance rule for a single `[*start, *end)` range against a set of
/// atomic spans. A range edge landing strictly inside an atomic `[cs, ce)` span
/// is pulled to that span's boundary (`start`→`ce`, `end`→`cs`); an edge outside
/// every span is untouched, and a range that strictly *contains* a span keeps
/// both edges. Applying the spans in sequence is order-independent across
/// ranges, so a caller may loop ranges or spans on the outside; a range whose
/// edges cross after clipping was swallowed whole and is left empty for the
/// caller to drop. Shared with the Typst backend's inline emitter.
pub fn clip_range_to_atomic(start: &mut usize, end: &mut usize, atomics: &[(usize, usize)]) {
    for &(cs, ce) in atomics {
        if cs < *start && *start < ce {
            *start = ce;
        }
        if cs < *end && *end < ce {
            *end = cs;
        }
    }
}

/// The maximal slot-free subranges of `[start, end)`: the range itself when it
/// covers no [`ISLAND_SLOT`], else one range per run between slots (empty runs
/// dropped). Used to keep a code span off an island it cannot represent.
fn split_around_slots(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let end = end.min(chars.len());
    if !chars[start.min(end)..end].contains(&ISLAND_SLOT) {
        return vec![(start, end)];
    }
    let mut out = Vec::new();
    let mut run = start;
    for (i, _) in chars[start..end]
        .iter()
        .enumerate()
        .map(|(i, c)| (start + i, c))
        .filter(|&(_, &c)| c == ISLAND_SLOT)
    {
        if run < i {
            out.push((run, i));
        }
        run = i + 1;
    }
    if run < end {
        out.push((run, end));
    }
    out
}

/// Nest `strong`/`emph` marks that partially overlap, by truncating the
/// later-opening one to its enclosing sibling's end. Both render as runs of the
/// same character, so a reopened `*` abutting a `**` would merge into an
/// ambiguous `***`. Truncation keeps the nested portion of both marks, dropping
/// only the crossing tail. Marks with distinct delimiters are left to the
/// sweep's close-and-reopen, which round-trips them exactly.
fn clip_asterisk_overlap(fmt: &mut [(usize, usize, &MarkKind)]) {
    let is_ast = |k: &MarkKind| matches!(k, MarkKind::Strong | MarkKind::Emph);
    // Asterisk-family marks, outermost first (start asc, then longer span first).
    let mut idx: Vec<usize> = (0..fmt.len()).filter(|&i| is_ast(fmt[i].2)).collect();
    idx.sort_by(|&a, &b| fmt[a].0.cmp(&fmt[b].0).then(fmt[b].1.cmp(&fmt[a].1)));
    // Ends of the enclosing ancestors still open at the current mark's start.
    let mut open_ends: Vec<usize> = Vec::new();
    for &i in &idx {
        let (s, mut e, _) = fmt[i];
        while open_ends.last().is_some_and(|&end| end <= s) {
            open_ends.pop();
        }
        if let Some(&parent_end) = open_ends.last() {
            if parent_end < e {
                e = parent_end;
                fmt[i].1 = e;
            }
        }
        open_ends.push(e);
    }
}

/// Reconstruct a table cell's markdown from its `{text, marks}`: the prose mark
/// sweep with `|`→`\|` escaping so the cell survives re-import through
/// `pulldown`'s pipe splitting. A cell is flat inline: no islands, no
/// leading-block escape.
fn render_cell_md(v: &serde_json::Value) -> String {
    let (text, marks) = crate::serial::parse_cell(v);
    let chars: Vec<char> = text.chars().collect();
    let (code_ranges, fmt, links) = bucket_marks(&marks, 0, chars.len(), true);
    render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        None,
        false,
        true,
        |_| None,
    )
}

fn delim_open(kind: &MarkKind) -> &'static str {
    match kind {
        MarkKind::Strong => "**",
        // `*`, not `_`: `_` cannot do intraword emphasis (CommonMark flanking),
        // so `_a_你` re-imports as literal text; `*a*你` emphasizes correctly.
        MarkKind::Emph => "*",
        MarkKind::Underline => "<u>",
        MarkKind::Strike => "~~",
        // Code/Link/Anchor are handled elsewhere.
        _ => "",
    }
}

fn delim_close(kind: &MarkKind) -> &'static str {
    match kind {
        MarkKind::Strong => "**",
        MarkKind::Emph => "*",
        MarkKind::Underline => "</u>",
        MarkKind::Strike => "~~",
        _ => "",
    }
}

/// The character reference for a space or tab, the whitespace markdown strips
/// from the edges of a line and of a table cell. Backslash escapes cover ASCII
/// punctuation only, so an edge run has no escaped spelling; the reference
/// re-imports as the character and renders as the character. Every other
/// whitespace character, and either of these away from an edge, survives
/// verbatim.
fn edge_space_ref(c: char) -> Option<&'static str> {
    match c {
        ' ' => Some("&#32;"),
        '\t' => Some("&#9;"),
        _ => None,
    }
}

fn escape_run(chars: &[char], escape_pipe: bool) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        escape_char_into(*c, i == 0, escape_pipe, &mut s);
    }
    s
}

/// Push `c` into `out` escaped so it re-imports as literal text: the char
/// verbatim, or a `&'static str` escape. `leading` also escapes block-starter
/// chars that would otherwise open a heading/list/quote; `escape_pipe` adds
/// `|`→`\|`, so a table cell survives `pulldown`'s pipe split.
fn escape_char_into(c: char, leading: bool, escape_pipe: bool, out: &mut String) {
    let esc: &str = match c {
        '\\' => "\\\\",
        '*' => "\\*",
        '_' => "\\_",
        '`' => "\\`",
        '[' => "\\[",
        ']' => "\\]",
        '<' => "\\<",
        '~' => "\\~",
        // `&` starts a CommonMark entity reference, decoded on re-import, so
        // `&word;`-shaped text would collapse to the entity's character. Always
        // escaped: detecting "would form an entity" is not worth the fragility.
        '&' => "\\&",
        '|' if escape_pipe => "\\|",
        '#' if leading => "\\#",
        '>' if leading => "\\>",
        '-' if leading => "\\-",
        '+' if leading => "\\+",
        // A leading `=` run underlines the paragraph line above it into a setext
        // heading, so only a continuation line can start one. One escaped `=`
        // defeats the whole underline.
        '=' if leading => "\\=",
        other => {
            out.push(other);
            return;
        }
    };
    out.push_str(esc);
}

fn longest_backtick_run(s: &str) -> usize {
    let mut max = 0;
    let mut run = 0;
    for c in s.chars() {
        if c == '`' {
            run += 1;
            max = max.max(run);
        } else {
            run = 0;
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::from_markdown;
    use crate::model::{Line, Loss, Mark};

    /// export∘import is the identity on the content.
    /// A content built by hand, as a client writing `Content` directly builds
    /// one, normalized the way every write lane normalizes it.
    fn stored(text: &str, containers: Vec<Vec<Container>>) -> Normalized {
        let lines = containers
            .into_iter()
            .map(|c| {
                let mut l = crate::model::Line::new(LineKind::Para);
                l.containers = c;
                l
            })
            .collect();
        let rt = Content::new(text.to_string(), lines).into_normalized();
        rt.validate().expect("stored content validates");
        rt
    }

    /// The lane [`Normalized`] does not close: a Rust embedder hand-builds a
    /// content nested past `MAX_NESTING_DEPTH`, which `normalize` cannot repair
    /// and no mint rejects. `DEPTH` clears the few-thousand-frame ceiling a
    /// test thread's stack holds.
    #[test]
    fn nesting_past_what_validate_allows_projects_rather_than_overflowing() {
        const DEPTH: usize = 10_000;
        let mut line = Line::new(LineKind::Para);
        line.containers = vec![Container::Quote { instance: 0 }; DEPTH];
        let rt = Content::new("x".to_string(), vec![line]).into_normalized();
        assert!(rt.validate().is_err(), "the shape `validate` refuses");
        assert_eq!(to_markdown(&rt), format!("{}x", "> ".repeat(DEPTH)));
    }

    /// Coincident marks nest in the order the canonical form stores, which is
    /// `MarkKind::sort_key` at a tie — the same rule the Typst emitter follows,
    /// and the reason this projection's bytes move when that key does. Both
    /// spellings re-import to one content, so the fixed point is over the
    /// content, not over the markdown.
    #[test]
    fn coincident_marks_project_in_the_stored_order() {
        let rt = Content::new("x".to_string(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![
                Mark::new(0, 1, MarkKind::Strong),
                Mark::new(0, 1, MarkKind::Strike),
            ])
            .into_normalized();
        rt.validate().expect("validates");
        assert_eq!(to_markdown(&rt), "~~**x**~~");
        assert_eq!(from_markdown("**~~x~~**").unwrap(), rt);
    }

    fn li(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: false,
            start: 1,
            ordinal,
            instance,
        }]
    }

    fn oli(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: true,
            start: 1,
            ordinal,
            instance,
        }]
    }

    /// The shapes #1357 recorded as inexpressible, now spelled by `instance`
    /// and carried through markdown by the marker alternation CommonMark
    /// already reads. Each is checked *from storage*, not from an import, since
    /// a client writing `Content` directly is the lane that mints them.
    #[test]
    fn adjacent_sibling_containers_project_and_return() {
        let cases: &[(Normalized, &str)] = &[
            // Two one-item lists: the shape that used to come back as one item
            // with an unnumbered continuation paragraph.
            (stored("a\nb", vec![li(0, 0), li(0, 1)]), "- a\n\n+ b"),
            // Which is still distinct from one item spanning two paragraphs.
            (stored("a\nb", vec![li(0, 0), li(0, 0)]), "- a\n\n  b"),
            // A one-item list then a longer one: no ordinal below 0 to restart
            // to, so nothing but `instance` can hold this apart.
            (
                stored("a\nb\nc", vec![li(0, 0), li(0, 1), li(1, 1)]),
                "- a\n\n+ b\n\n+ c",
            ),
            // Two adjacent quotes, which markdown spells with the blank line.
            (
                stored(
                    "a\nb",
                    vec![
                        vec![Container::Quote { instance: 0 }],
                        vec![Container::Quote { instance: 1 }],
                    ],
                ),
                "> a\n\n> b",
            ),
            // Still distinct from one quote of two paragraphs.
            (
                stored(
                    "a\nb",
                    vec![
                        vec![Container::Quote { instance: 0 }],
                        vec![Container::Quote { instance: 0 }],
                    ],
                ),
                "> a\n>\n> b",
            ),
        ];
        for (rt, expected) in cases {
            let md = to_markdown(rt);
            assert_eq!(&md, expected);
            assert_eq!(&from_markdown(&md).unwrap(), rt, "{md:?} did not return");
        }
    }

    /// The alternate markers meet the rule collision the primary ones already
    /// dodge. `+ ***` is a list item holding a thematic break; `* ***` would be
    /// four asterisks separated by spaces, which is a break outright and takes
    /// the item with it — which is why the second bullet is `+`.
    #[test]
    fn alternate_markers_do_not_collide_with_a_rule() {
        let mut rt = stored("a\n\nb", vec![li(0, 0), li(0, 1), li(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_normalized();
        rt.validate().expect("validates");
        assert_eq!(to_markdown(&rt), "- a\n\n+ ***\n\n+ b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap(), rt);

        let mut rt = stored("a\n\nb", vec![oli(0, 0), oli(0, 1), oli(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_normalized();
        assert_eq!(to_markdown(&rt), "1. a\n\n1) ***\n\n2) b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap(), rt);
    }

    /// Two ordered lists whose `start` differs: CommonMark reads only a list's
    /// *first* number, so without a marker change the second list's `start`
    /// re-imports as the first list's second item.
    #[test]
    fn adjacent_ordered_lists_keep_their_start() {
        let rt = from_markdown("1. a\n\n<!-- -->\n\n3. b").unwrap();
        assert_eq!(to_markdown(&rt), "1. a\n\n3) b");
        round_trips("1. a\n\n<!-- -->\n\n3. b");
    }

    fn round_trips(md: &str) {
        let rt = from_markdown(md).unwrap();
        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap();
        assert_eq!(
            rt, rt2,
            "content not a fixed point.\n  in:  {md:?}\n  mid: {md2:?}"
        );
    }

    /// The link arm must route its display text through the island renderer
    /// rather than emit the slot char raw, which re-imports as nothing.
    #[test]
    fn link_over_island_slot_round_trips() {
        round_trips("[![a cat](cat.png)](https://e.com)");
        round_trips("[a ![cat](cat.png) b](https://e.com)");
        assert_eq!(
            to_markdown(&from_markdown("[![a cat](cat.png)](https://e.com)").unwrap()),
            "[![a cat](cat.png)](https://e.com)"
        );
    }

    /// Editor-buildable, not importable. A code span is emitted verbatim, so it
    /// splits around the slot, keeping both the text and the island.
    #[test]
    fn code_mark_over_island_slot_keeps_the_island() {
        let mut rt = from_markdown("a ![x](y.png) b").unwrap().into_content();
        rt.marks.push(Mark { start: 0, end: 5, kind: MarkKind::Code });
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()));
        let md = to_markdown(&rt);
        assert_eq!(md, "`a `![x](y.png)` b`");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, rt.text);
        assert_eq!(rt2.islands.len(), 1);
    }

    #[test]
    fn unknown_block_vocabulary_projects_as_prose() {
        let rt = Content {
            text: "heads up\nstill inside".into(),
            lines: vec![
                Line {
                    kind: LineKind::Unknown {
                        tag: "callout".into(),
                        attrs: serde_json::json!({"variant": "warn"}),
                    },
                    containers: vec![Container::Unknown {
                        tag: "indent".into(),
                        attrs: serde_json::Value::Null,
                        instance: 0,
                    }],
                    continues: false,
                },
                Line {
                    kind: LineKind::Para,
                    containers: vec![Container::Unknown {
                        tag: "indent".into(),
                        attrs: serde_json::Value::Null,
                        instance: 0,
                    }],
                    continues: false,
                },
            ],
            marks: vec![Mark { start: 0, end: 5, kind: MarkKind::Strong }],
            islands: vec![],
        };
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(to_markdown(&rt), "**heads** up\n\nstill inside");
        // An unknown container inside a known one adds no prefix of its own.
        let mut rt = rt.into_content();
        rt.lines[0].containers.insert(0, Container::Quote { instance: 0 });
        rt.lines[1].containers.insert(0, Container::Quote { instance: 0 });
        assert_eq!(to_markdown(&rt.into_normalized()), "> **heads** up\n>\n> still inside");
    }

    #[test]
    fn plaintext_drops_marks_and_islands() {
        let rt = marked(
            "bold text",
            vec![Mark { start: 0, end: 4, kind: MarkKind::Strong }],
        );
        assert_eq!(to_plaintext(&rt), "bold text");
        let rt = Content {
            text: format!("see {ISLAND_SLOT} here"),
            lines: vec![Line { kind: LineKind::Para, containers: vec![], continues: false }],
            marks: vec![],
            islands: vec![Island {
                id: String::new(),
                island_type: "image".into(),
                props: serde_json::Value::Null,
                loss: Loss::UNREPRESENTABLE,
            }],
        }
        .into_normalized();
        assert_eq!(to_plaintext(&rt), "see  here");
    }

    /// A single-paragraph content with hand-placed `marks`: the free-overlap
    /// shapes an editor produces but markdown import never does.
    fn marked(text: &str, marks: Vec<Mark>) -> Normalized {
        let rt = Content {
            text: text.to_string(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks,
            islands: vec![],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "content invariants");
        rt
    }

    /// One deterministic fixed-point smoke per construct `properties.rs` fuzzes,
    /// labeled so a break localizes without a proptest seed. Constructs with no
    /// generator coverage stay as their own tests below.
    #[test]
    fn single_constructs_round_trip() {
        for (label, md) in [
            ("paragraph", "Hello world"),
            ("two_paragraphs", "one\n\ntwo"),
            ("marks", "a **b** _c_ ~~d~~ <u>e</u>"),
            ("heading", "## Title here"),
            ("inline_code", "run `cargo test` now"),
            ("bullet_list", "- a\n- b\n- c"),
            ("ordered_list", "3. a\n4. b"),
            ("multi_paragraph_item", "- first\n\n  second"),
            ("heading_item", "- # Title\n\n  body text"),
            ("rule_item", "* ---"),
            ("thematic_break", "one\n\n***\n\ntwo"),
            ("blockquote", "> quoted text"),
            ("link", "see [our site](https://example.com) now"),
            ("table", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("image", "see ![a cat](cat.png) here"),
        ] {
            println!("construct: {label}");
            round_trips(md);
        }
    }

    #[test]
    fn nested_marks() {
        round_trips("**bold _and italic_**");
    }

    #[test]
    fn code_block() {
        round_trips("```rust\nfn a() {}\nfn b() {}\n```");
    }

    #[test]
    fn thematic_break_canonicalizes_to_stars() {
        for src in ["---", "___", "- - -"] {
            let rt = from_markdown(&format!("one\n\n{src}\n\ntwo")).unwrap();
            let md = to_markdown(&rt);
            assert!(md.contains("\n\n***\n\n"), "source: {src}, got: {md:?}");
        }
    }

    /// A rule as a bullet item's first block is the one shape where marker and
    /// rule can spell one token: `- ` + `---` is four spaced dashes, a thematic
    /// break, and a break outranks a list item wherever both readings fit.
    #[test]
    fn rule_opening_a_list_item_keeps_its_item() {
        for md in ["* ---", "+ ---", "- ***", "- ___", "- - ***", "- > ***"] {
            round_trips(md);
        }
        assert_eq!(to_markdown(&from_markdown("* ---").unwrap()), "- ***");
        // The shapes that never collide, pinned against a fix that trades one
        // collision for another: swapping the bullet marker to `*`/`+` starts a
        // *new* list, resetting `ordinal` on this item and every one after.
        round_trips("1. ---");
        round_trips("- one\n\n  ---");
        round_trips("- a\n- ***\n- c");
    }

    #[test]
    fn literal_asterisks_escaped() {
        round_trips("2 * 3 = 6 and a_b_c");
    }

    #[test]
    fn hard_break_round_trips() {
        round_trips("line one\\\nline two");
    }

    #[test]
    fn hard_break_in_list_item() {
        round_trips("- one\\\ntwo\n- three");
    }

    #[test]
    fn leading_ordered_marker_escaped() {
        let mut rt = from_markdown("x").unwrap().into_content();
        rt.text = "1. not a list".into();
        let rt = rt.into_normalized();
        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap();
        assert_eq!(back.lines[0].kind, LineKind::Para);
        assert!(back.lines[0].containers.is_empty());
        assert_eq!(back, rt);
    }

    /// One Para block over the `\n`-separated lines of `text`, joined by hard
    /// breaks: the shape that puts a line under a paragraph line.
    fn hard_break_block(text: &str) -> Normalized {
        let lines = (0..text.split('\n').count())
            .map(|i| Line::new(LineKind::Para).with_continues(i > 0))
            .collect();
        let rt = Content::new(text.to_string(), lines).into_normalized();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        rt
    }

    /// Markdown strips space and tab from a line's edges, behind a heading,
    /// quote, or list prefix too. The escape also dissolves the block markers a
    /// whitespace prefix hides from the position-0 escapes: `   - item` parses
    /// as a bullet item, and the model holds it as one line of text.
    #[test]
    fn edge_whitespace_survives_export() {
        const TEXTS: &[&str] = &[
            " foo",
            "    foo",
            "\tfoo",
            "foo   ",
            "  foo  ",
            "   ",
            "   - item",
            "  # not a heading",
            "  1. not a list",
            "  > not a quote",
            // A trailing space keeps a heading's `#` out of the closing-sequence
            // shape the position-0 escapes do not reach.
            "a # ",
        ];
        for text in TEXTS {
            for containers in [vec![], vec![Container::Quote { instance: 0 }], li(0, 0)] {
                let rt = stored(text, vec![containers.clone()]);
                let md = to_markdown(&rt);
                assert_eq!(
                    &from_markdown(&md).unwrap(),
                    &rt,
                    "{text:?} under {containers:?} did not return: {md:?}"
                );
            }
            let mut rt = stored(text, vec![vec![]]).into_content();
            rt.lines[0].kind = LineKind::Heading { level: 2 };
            let rt = rt.into_normalized();
            let md = to_markdown(&rt);
            assert_eq!(
                &from_markdown(&md).unwrap(),
                &rt,
                "heading {text:?} did not return: {md:?}"
            );
        }
    }

    /// Every line of a hard-break block carries the escape, so an edge run
    /// ahead of the `\` survives too.
    #[test]
    fn edge_whitespace_survives_a_hard_break() {
        for text in ["abc   \n   def", "   \nabc", "abc\n   ", "\tabc\ndef\t"] {
            let rt = hard_break_block(text);
            let md = to_markdown(&rt);
            assert_eq!(&from_markdown(&md).unwrap(), &rt, "{text:?} → {md:?}");
        }
    }

    /// A `=` run continuing a paragraph line underlines it into a setext
    /// heading, taking the hard break's `\` into the text with it.
    #[test]
    fn a_setext_underline_on_a_continuation_line_stays_text() {
        for text in [
            "abc\n===", "abc\n=", "abc\n=== ", "abc\n---", "abc\n= b", "abc\n   ---",
            "abc\n  ===",
        ] {
            let rt = hard_break_block(text);
            let md = to_markdown(&rt);
            assert_eq!(&from_markdown(&md).unwrap(), &rt, "{text:?} → {md:?}");
        }
    }

    /// The verify-and-drop net wraps its probe in `,…,` to block every
    /// leading-block construct, so it cannot see the indented code block an
    /// unescaped edge run opens: it reads such a line as safe while the `**`
    /// ships into the text. The escape is the only thing holding this line.
    #[test]
    fn a_marked_line_with_edge_whitespace_keeps_its_delimiters_out_of_the_text() {
        let rt = marked("    foo", vec![Mark::new(4, 7, MarkKind::Strong)]);
        let back = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(back.text, "    foo");
        assert_eq!(back, rt);
    }

    /// A cell's edges are trimmed by the pipe split, so cell text carries the
    /// line escape.
    #[test]
    fn table_cell_edge_whitespace_round_trips() {
        let cell = |t: &str| serde_json::json!({"marks": [], "text": t});
        let rt = Content {
            text: ISLAND_SLOT.to_string(),
            lines: vec![Line::new(LineKind::Island)],
            marks: vec![],
            islands: vec![
                Island::new("isl-0".into(), "table".into()).with_props(serde_json::json!({
                    "aligns": ["none"],
                    "header": [cell(" h ")],
                    "rows": [[cell("  a")], [cell("b  ")], [cell("   ")]],
                })),
            ],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "table island invalid");
        let md = to_markdown(&rt);
        assert_eq!(&from_markdown(&md).unwrap(), &rt, "cell edges lost: {md:?}");
    }

    #[test]
    fn table_with_formatted_cells_round_trips() {
        round_trips("| Name | Note |\n| --- | --- |\n| **bold** | _italic_ |");
        round_trips("| A |\n| --- |\n| **b** and _i_ `c` [d](https://e.com) ~~e~~ |");
        round_trips("| A |\n| --- |\n| <u>under</u> |");
        // A literal pipe inside a cell survives via `\|` re-escaping on export.
        round_trips("| A |\n| --- |\n| a \\| b |");
    }

    #[test]
    fn formatted_cell_marks_are_structured_not_reparsed() {
        let rt = from_markdown("| H |\n| --- |\n| **bold** |").unwrap();
        let cell = &rt.islands[0].props["rows"][0][0];
        assert_eq!(cell["text"], "bold");
        assert_eq!(cell["marks"][0]["type"], "strong");
        assert_eq!(cell["marks"][0]["start"], 0);
        assert_eq!(cell["marks"][0]["end"], 4);
        assert!(to_markdown(&rt).contains("**bold**"));
    }

    /// A content of one Para line holding `text`, whose slots the `islands`
    /// back: the shape a wire island op mints.
    fn with_islands(text: &str, islands: Vec<Island>) -> Normalized {
        let rt = Content {
            text: text.to_string(),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![],
            islands,
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        rt
    }

    /// The unknown-island placeholder's whole property is that it re-imports as
    /// no content text. `island_type` is an open wire string no lane constrains,
    /// and a line break leaked at column zero opens whatever the rest spells —
    /// a heading, or a card fence the document layer reads as another card.
    #[test]
    fn an_unknown_island_type_cannot_escape_its_placeholder() {
        for island_type in [
            "widget",
            "x --> injected text <!-- ",
            "a\n\n# heading",
            "t\n\n~~~card-yaml\n$kind: injected\n~~~\n",
            "<!--",
            "-->",
            "a-",
            "",
        ] {
            let rt = with_islands(
                &format!("x{ISLAND_SLOT}"),
                vec![Island::new("isl-0".into(), island_type.into())],
            );
            let md = to_markdown(&rt);
            let back = from_markdown(&md).unwrap();
            assert_eq!(back.text, "x", "{island_type:?} leaked: {md:?}");
            assert_eq!(back.lines.len(), 1, "{island_type:?} split its block: {md:?}");
        }
    }

    /// An inline placeholder sits mid-paragraph, where a comment is inline HTML
    /// and swallows nothing after it. The block-fence repair leaves that one
    /// alone, so the text on either side stays one line.
    #[test]
    fn an_inline_unknown_island_leaves_its_line_whole() {
        let rt = with_islands(
            &format!("a{ISLAND_SLOT}b"),
            vec![Island::new("isl-0".into(), "widget".into())],
        );
        let back = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(back.text, "ab");
        assert_eq!(back.lines.len(), 1);
    }

    /// The placeholder re-imports as no content text, so the net's expected text
    /// carries no slot for it. Reading one back there would fail every probe and
    /// cost the line every mark markdown can carry.
    #[test]
    fn a_mark_flanking_an_unknown_island_placeholder_survives() {
        let rt = Content {
            text: format!("x{ISLAND_SLOT} bold"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(3, 7, MarkKind::Strong)],
            islands: vec![Island::new("isl-0".into(), "widget".into())],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        let md = to_markdown(&rt);
        assert_eq!(md, "x<!-- island:widget --> **bold**");
        let back = from_markdown(&md).unwrap();
        assert_eq!(back.text, "x bold");
        assert_eq!(back.marks, vec![Mark::new(2, 6, MarkKind::Strong)]);
    }

    /// An image's markup does re-import as a slot, so the net still expects one
    /// back and a mark that would eat it is dropped. Here both `**` sit against
    /// the image's punctuation, where CommonMark flanking refuses them.
    #[test]
    fn a_mark_leaking_around_an_image_slot_is_still_dropped() {
        let image = || {
            Island::new("isl-0".into(), "image".into())
                .with_props(serde_json::json!({"alt": "a", "url": "u"}))
        };
        let over_slot = Content {
            text: format!("a{ISLAND_SLOT}b"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(1, 2, MarkKind::Strong)],
            islands: vec![image()],
        }
        .into_normalized();
        let md = to_markdown(&over_slot);
        assert_eq!(md, "a![a](u)b");
        let back = from_markdown(&md).unwrap();
        assert_eq!(back.text, over_slot.text);
        assert_eq!(back.islands.len(), 1);
        assert!(back.marks.is_empty(), "leaking mark kept: {md:?}");
        // A mark on the same line whose delimiters do flank rides through.
        let before_slot = Content {
            text: format!("a{ISLAND_SLOT}b"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(0, 1, MarkKind::Strong)],
            islands: vec![image()],
        }
        .into_normalized();
        let md = to_markdown(&before_slot);
        assert_eq!(md, "**a**![a](u)b");
        assert_eq!(from_markdown(&md).unwrap(), before_slot);
    }

    /// An image `alt` is the one place the character reference cannot carry an
    /// edge run: the parser decodes it, then trims alt.
    #[test]
    fn known_image_alt_limit() {
        let rt = with_islands(
            &ISLAND_SLOT.to_string(),
            vec![
                Island::new("isl-0".into(), "image".into())
                    .with_props(serde_json::json!({"alt": " a ", "url": "u"})),
            ],
        );
        let back = from_markdown(&to_markdown(&rt)).unwrap();
        assert_eq!(
            back.islands[0].props["alt"], "a",
            "if this ever round-trips, promote it out of the known-limits list"
        );
    }

    #[test]
    fn known_hard_break_limits() {
        let rt = from_markdown("**one\\\ntwo**").unwrap();
        let rt2 = from_markdown(&to_markdown(&rt)).unwrap();
        assert!(
            rt != rt2,
            "if this ever round-trips, promote it out of the known-limits list"
        );
        assert_eq!(rt2.marks.len(), 2, "mark split across the hard break");
    }

    #[test]
    fn anchor_marks_omitted_but_text_survives() {
        let mut rt = from_markdown("comment target here").unwrap().into_content();
        rt.marks.push(Mark {
            start: 8,
            end: 14,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        let rt = rt.into_normalized();
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "comment target here");
        assert!(!md.contains("c1"));
    }

    /// `strong` and `emph` share the `*` delimiter, so their overlap is
    /// unrepresentable: a naive `**ab*cdef*` re-imports as literal text. Export
    /// nests them by truncation instead, keeping the text.
    #[test]
    fn overlapping_asterisk_marks_stay_text_safe() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Emph,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab*cd***ef", "balanced, no literal `**` leak");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "abcdef");
        // Documented limit: same-delimiter overlap degrades to its nested subset.
        assert_eq!(
            rt2.marks,
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong
                },
                Mark {
                    start: 2,
                    end: 4,
                    kind: MarkKind::Emph
                },
            ]
        );
    }

    /// Overlap between marks with *distinct* delimiters round-trips exactly: the
    /// close-and-reopen sweep lowers it to balanced, re-importable markdown.
    #[test]
    fn overlapping_distinct_delim_marks_round_trip_exactly() {
        for (k1, k2) in [
            (MarkKind::Strong, MarkKind::Strike),
            (MarkKind::Strike, MarkKind::Strong),
            (MarkKind::Emph, MarkKind::Strike),
            (MarkKind::Underline, MarkKind::Emph),
            (MarkKind::Strong, MarkKind::Underline),
        ] {
            let rt = marked(
                "abcdef",
                vec![
                    Mark {
                        start: 0,
                        end: 4,
                        kind: k1.clone(),
                    },
                    Mark {
                        start: 2,
                        end: 6,
                        kind: k2.clone(),
                    },
                ],
            );
            let md = to_markdown(&rt);
            let rt2 = from_markdown(&md).unwrap();
            assert_eq!(rt, rt2, "{k1:?}+{k2:?} overlap not a fixed point: {md:?}");
        }
    }

    /// A formatting mark partially overlapping an atomic `code` span clips to
    /// the text outside it, so the markdown stays balanced.
    #[test]
    fn wrap_over_code_stays_balanced() {
        let rt = marked(
            "abcdef",
            vec![
                Mark {
                    start: 0,
                    end: 4,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 2,
                    end: 6,
                    kind: MarkKind::Code,
                },
            ],
        );
        let md = to_markdown(&rt);
        assert_eq!(md, "**ab**`cdef`");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "abcdef");
    }

    /// CommonMark reads a code span's edge backtick as part of the fence run and
    /// strips one space off each side of a span that begins *and* ends with one,
    /// so both shapes need the pad the parser then removes. A span of nothing but
    /// spaces is exempt from the strip, so a pad there would grow it.
    #[test]
    fn code_span_edges_keep_their_pad() {
        for (md, text, want) in [
            ("`` `a ``", "`a", "`` `a ``"),
            ("`` a` ``", "a`", "`` a` ``"),
            ("`` `a` ``", "`a`", "`` `a` ``"),
            ("`` ` ``", "`", "`` ` ``"),
            ("`  a  `", " a ", "`  a  `"),
            ("`a`", "a", "`a`"),
            ("`  `", "  ", "`  `"),
        ] {
            let rt = from_markdown(md).unwrap();
            assert_eq!(rt.text, text, "import of {md:?}");
            assert_eq!(to_markdown(&rt), want);
            round_trips(md);
        }
    }

    /// Entity-shaped text must not re-import as the decoded entity: exporting
    /// the content text "&amp;" unescaped would re-import as "&".
    #[test]
    fn ampersand_and_entities_round_trip() {
        round_trips("a & b");
        round_trips("copyright \\&copy; sign");
        let rt = from_markdown("\\&amp;").unwrap();
        assert_eq!(rt.text, "&amp;");
        let md = to_markdown(&rt);
        assert!(md.contains("\\&"), "the `&` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "&amp;", "entity-shaped text must not decode");
        assert_eq!(rt, rt2);
    }

    /// Heading text ending in a `#` run must not re-import as an ATX closing
    /// sequence: `# a #` would come back as "a", dropping the `#`.
    #[test]
    fn heading_trailing_hash_round_trips() {
        let rt = from_markdown("# a \\#").unwrap();
        assert_eq!(rt.text, "a #");
        let md = to_markdown(&rt);
        assert!(md.contains("\\#"), "trailing `#` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.text, "a #", "trailing `#` must survive");
        assert_eq!(rt, rt2);
        round_trips("# heading \\#\\#");
        round_trips("## title\\#");
    }

    /// An unescaped special in an image alt terminates the markup early:
    /// `![a]b](x.png)` re-imports as prose with the image gone.
    #[test]
    fn image_alt_specials_round_trip() {
        round_trips("see ![a\\]b](x.png) here");
        round_trips("see ![a\\\\b](x.png) here");
        round_trips("see ![a&b](x.png) here");
        round_trips("see ![a\\*b\\_c](x.png) here");
        let rt = from_markdown("see ![a\\]b](x.png) here").unwrap();
        assert_eq!(rt.islands.len(), 1, "one image island");
        assert_eq!(rt.islands[0].props["alt"], "a]b");
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap();
        assert_eq!(rt2.islands.len(), 1, "image survived, got md {md:?}");
        assert_eq!(rt2.islands[0].props["alt"], "a]b");
    }

    /// Image and link URLs carrying the destination-terminating specials
    /// (spaces, unbalanced parens, `&`, `<`/`>`, backslash) all round-trip.
    #[test]
    fn url_specials_round_trip() {
        round_trips("a [t](<foo bar>) b");
        round_trips("see [t](https://en.wikipedia.org/wiki/Rust_(programming_language)) x");
        round_trips("see ![a](<x y.png>) here");
        round_trips("see [t](<a )b>) x");
        round_trips("see [t](<a&b>) x");
        round_trips("see [t](<a\\<b\\>c>) x");
        round_trips("see [t](<a\\\\b>) x");
    }

    /// `emit_url` angle-wraps only when a special forces it, so common URLs stay
    /// unbracketed.
    #[test]
    fn emit_url_bare_when_safe() {
        let mut bare = String::new();
        emit_url("https://ex.com/a(b)c", &mut bare);
        assert_eq!(bare, "https://ex.com/a(b)c", "balanced parens stay bare");
        let mut wrapped = String::new();
        emit_url("a b", &mut wrapped);
        assert_eq!(wrapped, "<a b>", "space forces the wrap");
        let mut esc = String::new();
        emit_url("a&<\\b", &mut esc);
        assert_eq!(esc, "<a\\&\\<\\\\b>", "specials escaped inside the wrap");
    }

    /// A line ending in a `url` has no destination spelling at all, so the
    /// writer percent-encodes it: the mark survives, addressing the encoded URL.
    /// Only a storage-lane or hand-built content reaches this — the authored
    /// lanes refuse the url outright.
    #[test]
    fn a_url_carrying_a_line_ending_still_projects_as_a_link() {
        let rt = Content::new("t x".to_string(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![Mark::new(
                0,
                1,
                MarkKind::Link {
                    url: "a\nb".into(),
                },
            )])
            .into_normalized();
        let back = from_markdown(&to_markdown(&rt)).expect("re-imports");
        assert_eq!(back.text, "t x", "the display text did not leak");
        assert_eq!(
            back.marks.first().map(|m| &m.kind),
            Some(&MarkKind::Link {
                url: "a%0Ab".into()
            })
        );

        let isl = crate::model::Island::new("i1".into(), "image".into())
            .with_props(serde_json::json!({"alt": "a", "url": "u\rv"}));
        let rt = Content::new(format!("x{ISLAND_SLOT}"), vec![Line::new(LineKind::Para)])
            .with_islands(vec![isl])
            .into_normalized();
        let back = from_markdown(&to_markdown(&rt)).expect("re-imports");
        assert_eq!(back.islands.len(), 1, "the island survived");
        assert_eq!(back.islands[0].props["url"], "u%0Dv");
    }

    /// A strong mark whose delimiters land where CommonMark won't read them as a
    /// run is dropped, and every other mark on the line is kept. Re-add order is
    /// document order, so a good mark before *or* after a leaking one lives.
    #[test]
    fn net_drops_only_the_leaking_marks() {
        for (label, text, spans, want) in [
            ("leaking alone", "a.b", vec![(0, 2)], "a.b"),
            ("representable alone", "ab cd", vec![(0, 2)], "**ab** cd"),
            (
                "leaking then representable",
                "a.b and cd ef",
                vec![(1, 3), (8, 10)],
                "a.b and **cd** ef",
            ),
            (
                "representable then leaking",
                "cd ef and a.b",
                vec![(0, 2), (11, 13)],
                "**cd** ef and a.b",
            ),
        ] {
            let rt = marked(
                text,
                spans
                    .into_iter()
                    .map(|(start, end)| Mark {
                        start,
                        end,
                        kind: MarkKind::Strong,
                    })
                    .collect(),
            );
            let md = to_markdown(&rt);
            assert_eq!(md, want, "{label}");
            assert_eq!(from_markdown(&md).unwrap().text, text, "{label}: text drift");
        }
    }

    /// At the [`PROBE_BUDGET`] guarantee (32 flanking marks, half unrepresentable)
    /// the budget covers the full search, so no representable mark is lost to it.
    #[test]
    fn net_resolves_a_line_at_the_budget_guarantee() {
        let (mut text, mut marks) = (String::new(), Vec::new());
        for i in 0..32 {
            let at = i * 6;
            if i % 2 == 0 {
                text.push_str("abcde "); // `**abcde**`: a run on both edges
                marks.push((at, at + 5));
            } else {
                text.push_str("abc.e "); // `**.e**`: cannot open against `.`
                marks.push((at + 3, at + 5));
            }
        }
        let text = text.trim_end();
        let rt = marked(
            text,
            marks
                .into_iter()
                .map(|(start, end)| Mark {
                    start,
                    end,
                    kind: MarkKind::Strong,
                })
                .collect(),
        );
        let md = to_markdown(&rt);
        assert_eq!(md.matches("**abcde**").count(), 16, "every good mark kept");
        assert_eq!(md.matches("**").count(), 32, "every leaking mark dropped");
        assert_eq!(from_markdown(&md).unwrap().text, text);
    }

    /// Past the budget the net still terminates and preserves the text, dropping
    /// what it has not cleared.
    #[test]
    fn net_bounds_a_pathological_line() {
        let text = "a.b, ".repeat(200);
        let text = text.trim_end();
        let rt = marked(
            text,
            (0..200)
                .map(|i| Mark {
                    start: i * 5 + 1,
                    end: i * 5 + 4,
                    kind: MarkKind::Strong,
                })
                .collect(),
        );
        let md = to_markdown(&rt);
        assert!(!md.contains('*'), "every leaking mark dropped: {md:?}");
        assert_eq!(from_markdown(&md).unwrap().text, text);
    }

    #[test]
    fn ordered_list_marker_saturates_on_overflow() {
        // `validate` does not ceiling `start`/`ordinal`, so a corrupt content can
        // carry `start == u64::MAX`.
        let json = format!(
            r#"{{"text":"x","lines":[{{"kind":"para","containers":[{{"container":"list_item","ordered":true,"start":{},"ordinal":5}}]}}],"marks":[],"islands":[]}}"#,
            u64::MAX
        );
        let rt = Content::from_canonical_json(&json).unwrap();
        let md = to_markdown(&rt);
        assert!(
            md.contains(&format!("{}. ", u64::MAX)),
            "marker saturates to u64::MAX: {md:?}"
        );
    }
}
