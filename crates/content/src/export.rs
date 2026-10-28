//! Markdown export: content → markdown, per island type.
//!
//! Marks become syntax; identity ([`MarkKind::Anchor`]) marks are **omitted**,
//! surviving across edits via diff-rebase rather than the projection.
//! [`to_markdown_annotated`] spells each at its start as a read-only tag the
//! import drops. The contract is the **content fixed point**: for a content `rt` from
//! [`crate::import::from_markdown`], `from_markdown(to_markdown(rt)) == rt`.
//! Markdown source is not canonical; the content is.
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
//! ## Codec limits
//!
//! These shapes markdown cannot represent do **not** round-trip. An empty line
//! in a hard-break block drops, and so does an empty paragraph beside another
//! block, markdown spelling neither. An image `alt` loses its edge
//! whitespace: the parser trims alt *after* decoding the character reference
//! that carries an edge run everywhere else. A link or image `url` carrying a line ending comes back
//! percent-encoded (`%0A`/`%0D`): CommonMark admits none in a destination, and
//! the import decodes no percent escape. A `url` or `alt` carrying a character
//! the import does not admit there (a bidi control, a line separator, a line
//! ending in an `alt`) comes back rewritten. A link over a footnote reference
//! comes back as two links, one each side: link text reads no reference. An
//! equation whose `tex` its dollars cannot read back as itself comes back as
//! other markup. A table's slot sharing its line comes back on a line of its
//! own, the prose either side a block of its own: markdown writes a table only
//! as a block. Each arises only from adversarial delimiter/break placement, a
//! hand-built island prop or mark, a stored value, a merge of concurrent edits,
//! or an editor's empty line.

use std::borrow::Cow;

use crate::island::IslandType;
use crate::model::{
    Container, Island, LineKind, Mark, MarkKind, Content, Normalized, Usv, ISLAND_SLOT,
};

/// Render a content to markdown.
pub fn to_markdown(rt: &Normalized) -> String {
    let rt = &*block_islands_alone(rt);
    let segments = line_segments(rt);
    let labels = note_labels(rt);
    project(&Ctx {
        rt,
        segments: &segments,
        labels: &labels,
        tags: &[],
    })
}

/// A [`to_markdown_annotated`] read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotated {
    /// [`to_markdown`]'s markdown with the anchor tags it can hold.
    pub markdown: String,
    /// Every prose anchor, spelled or not, in `(start, id)` order.
    pub anchors: Vec<AnchorRead>,
}

/// One prose anchor of an [`Annotated`] read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorRead {
    pub id: String,
    /// The text of the content line the anchor's start sits on, island slots
    /// removed: what locates the anchor where a writer lost its tag.
    pub line: String,
}

/// [`to_markdown`] with each prose anchor spelled
/// `<qm-anchor ref="ID"></qm-anchor>` at its `start`, a read-only
/// spelling the import drops, so the markdown imports as [`to_markdown`]'s
/// does. A range anchor reads as its start.
///
/// A tag sits after the delimiters of the marks closing at its position and
/// before those opening there. Inside a code span or a link it moves to the
/// span's start. A code-block line, a block island's line and an empty line
/// hold none, and neither does a table cell or a footnote's note, whose
/// anchors are not listed.
/// Where a line's tags change what it imports to, they move to its end, and
/// where that changes it too, the line holds none.
pub fn to_markdown_annotated(rt: &Normalized) -> Annotated {
    let rt = &*block_islands_alone(rt);
    let segments = line_segments(rt);
    let labels = note_labels(rt);
    let plain = Ctx {
        rt,
        segments: &segments,
        labels: &labels,
        tags: &[],
    };
    let mut found: Vec<(Usv, &str)> = rt
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } => Some((m.start, id.as_str())),
            _ => None,
        })
        .collect();
    found.sort_unstable();
    let mut tags: Vec<Vec<(Usv, &str)>> = vec![Vec::new(); segments.len()];
    let mut anchors = Vec::with_capacity(found.len());
    for (start, id) in found {
        let (i, read) = anchor_read(&plain, start, id);
        anchors.push(read);
        let seg = &segments[i];
        let holds_tags = rt
            .lines
            .get(i)
            .is_some_and(|l| !matches!(l.kind, LineKind::Code { .. }))
            && seg.start < seg.end
            && start <= seg.end
            && block_island(&plain, i).is_none();
        if holds_tags {
            tags[i].push((start - seg.start, id));
        }
    }
    let markdown = project(&Ctx {
        rt,
        segments: &segments,
        labels: &labels,
        tags: &tags,
    });
    Annotated { markdown, anchors }
}

/// The anchors [`to_markdown_annotated`] lists for `rt` whose id `keep`
/// holds, in its order, read without projecting `rt`.
pub fn anchors_where(rt: &Normalized, keep: impl Fn(&str) -> bool) -> Vec<AnchorRead> {
    let rt = &*block_islands_alone(rt);
    let mut found: Vec<(Usv, &str)> = rt
        .marks
        .iter()
        .filter_map(|m| match &m.kind {
            MarkKind::Anchor { id } if keep(id) => Some((m.start, id.as_str())),
            _ => None,
        })
        .collect();
    if found.is_empty() {
        return Vec::new();
    }
    found.sort_unstable();
    let segments = line_segments(rt);
    let labels = note_labels(rt);
    let ctx = Ctx {
        rt,
        segments: &segments,
        labels: &labels,
        tags: &[],
    };
    found.into_iter().map(|(start, id)| anchor_read(&ctx, start, id).1).collect()
}

/// `rt` as the projection writes it: each block-only island's slot that shares
/// its line broken onto a line of its own
/// ([`Content::split_block_islands`]), marks and anchors moved with the text.
fn block_islands_alone(rt: &Normalized) -> Cow<'_, Normalized> {
    if !rt.islands.iter().any(|i| i.island_type.block_only()) {
        return Cow::Borrowed(rt);
    }
    let chars: Vec<char> = rt.text.chars().collect();
    if crate::model::inline_block_islands(&chars, &rt.islands).next().is_none() {
        return Cow::Borrowed(rt);
    }
    let mut broken = rt.clone().into_content();
    broken.split_block_islands();
    Cow::Owned(broken.into_normalized())
}

/// The anchor `id` starting at `start`, and the index of the line it sits on.
fn anchor_read(ctx: &Ctx, start: Usv, id: &str) -> (usize, AnchorRead) {
    let segments = ctx.segments;
    let i = segments.partition_point(|s| s.end < start).min(segments.len() - 1);
    let read = AnchorRead {
        id: id.to_string(),
        line: seg_str(ctx, i).chars().filter(|&c| c != ISLAND_SLOT).collect(),
    };
    (i, read)
}

fn project(ctx: &Ctx) -> String {
    let mut out = String::new();
    emit_block(ctx, 0..ctx.rt.lines.len(), 0, &mut out);
    // `to_markdown` projects a *value*, not a file: it emits no final newline,
    // so `writer.set("subject", "Hello")` reads back as `"Hello"`. Document-file
    // writers own the file-final newline, and import is newline-insensitive, so
    // dropping it is round-trip-invisible.
    while out.ends_with('\n') {
        out.pop();
    }
    // Each note is defined after the body, at the root, where a blank line
    // closes whatever container the body ended in.
    for (isl, label) in ctx.rt.islands.iter().zip(ctx.labels) {
        if isl.island_type == IslandType::Footnote {
            out.push_str(&format!("\n\n[^{label}]: "));
            out.push_str(&render_note_md(&isl.props));
        }
    }
    out
}

/// Per island, the label its footnote reference is written with: the count of
/// footnotes up to and including it, so the notes number `1, 2, …` in text
/// order. Re-import pairs a reference with its definition by label and keeps
/// none of it.
fn note_labels(rt: &Content) -> Vec<usize> {
    rt.islands
        .iter()
        .scan(0, |n, isl| {
            *n += usize::from(isl.island_type == IslandType::Footnote);
            Some(*n)
        })
        .collect()
}

/// Render a content to plaintext: [`Content::text`] with island slots
/// ([`ISLAND_SLOT`]) removed. The lossy sibling of [`to_markdown`]: it drops
/// every mark and island, keeping only literal text.
///
/// Tables and images having no plaintext form is a **decided limitation**: the
/// acroform backend fills a form field from this projection, so a field bound to
/// a table-bearing content renders the surrounding text and omits the table,
/// under a `backend::declined_construct` warning, rather than emitting a
/// row/tab dump that would read as faithful.
pub fn to_plaintext(rt: &Content) -> String {
    rt.text.chars().filter(|&c| c != ISLAND_SLOT).collect()
}

/// The character count a text cap (`max:`) measures on a plaintext projection:
/// Unicode scalar values, trailing line breaks aside.
pub fn char_count(text: &str) -> usize {
    text.trim_end_matches(['\n', '\r']).chars().count()
}

struct Ctx<'a> {
    rt: &'a Content,
    /// One entry per [`Content::lines`] entry.
    segments: &'a [Segment],
    /// One entry per island: [`note_labels`].
    labels: &'a [usize],
    /// Per line, the anchor tags it holds at line-local positions, in
    /// `(position, id)` order; empty for the plain projection.
    tags: &'a [Vec<(Usv, &'a str)>],
}

/// One line's char range `[start, end)` into the content, with the matching byte
/// range and the count of island slots before the line, so a caller indexes the
/// content text and the island list in O(1) without rescanning. A derived view
/// [`line_segments`] recomputes whole.
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
/// `head` the line opening its first block written, `buf` the markdown emitted
/// for the level so far, and `container` the one whose syntax prefixes `buf`
/// when the level closes (`None` at the root).
struct Frame<'a> {
    container: Option<&'a Container>,
    at: usize,
    head: Option<usize>,
    range: std::ops::Range<usize>,
    depth: usize,
    buf: String,
}

impl<'a> Frame<'a> {
    fn open(container: Option<&'a Container>, range: std::ops::Range<usize>, depth: usize) -> Self {
        Frame {
            container,
            at: range.start,
            head: None,
            range,
            depth,
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
            if lines[i].containers.len() > frame.depth {
                let item = crate::traverse::items(lines, i..frame.range.end, frame.depth)
                    .next()
                    .expect("a line with a container at `depth` opens an item");
                block_separator(&mut frame.buf, frame.head.is_none());
                frame.head.get_or_insert(i);
                frame.at = item.range.end;
                let child = Frame::open(Some(item.container), item.range, frame.depth + 1);
                stack.push(child);
            } else {
                let seg = crate::traverse::segment(lines, i..frame.range.end, frame.depth);
                frame.at = seg.end;
                // Markdown spells an empty paragraph with nothing, so beside
                // another block one writes nothing; a container's only block
                // keeps its line.
                if is_empty_para(ctx, seg.clone()) && (frame.head.is_some() || seg.end < frame.range.end) {
                    continue;
                }
                block_separator(&mut frame.buf, frame.head.is_none());
                frame.head.get_or_insert(i);
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
                let head = done.head.unwrap_or(done.range.start);
                let lead = &lines[head];
                let prose_head = lead.containers.len() == done.depth
                    && lead.kind == LineKind::Para
                    && block_island(ctx, head).is_none();
                close_container(key, &done.buf, prose_head, &mut parent.buf);
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
/// it, with the container's markdown syntax. `prose_head` says the level opens
/// on a paragraph line of its own.
fn close_container(key: &Container, inner: &str, prose_head: bool, out: &mut String) {
    match key {
        Container::ListItem {
            ordered,
            start,
            ordinal,
            checked,
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
            let head = inner.split('\n').next().unwrap_or("");
            // Whether the item's first block moves to the line below its
            // marker. A task marker opens its item's paragraph, so text after
            // it on its line is read as that paragraph whatever block it
            // spells. A marker run that spells a thematic break outranks the
            // items spelling it: three nested empty bullets emit `- - - `,
            // which re-imports as a `Rule` with the nesting gone. Changing a
            // marker char is not the way out — a different bullet char starts
            // a new list, resetting `ordinal` on this item and every one after
            // it, and the empty item can have non-empty siblings. Moving the
            // content costs no marker and no list identity, and the check runs
            // per level, so a run of any depth breaks into pieces of two.
            let (lead, below) = match checked {
                Some(done) => (
                    format!("{marker}{} ", if *done { "[x]" } else { "[ ]" }),
                    !prose_head || head.is_empty(),
                ),
                None => (marker.clone(), is_thematic_break(&format!("{marker}{head}"))),
            };
            // An empty item keeps its lead's trailing space: a task marker
            // reads as one only before whitespace, and the item can end the
            // markdown.
            if below && !inner.is_empty() {
                out.push_str(lead.trim_end());
                out.push('\n');
                prefix_lines(inner, &indent, &indent, out);
            } else {
                prefix_lines(inner, &lead, &indent, out);
            }
        }
        Container::Quote { .. } => {
            // `> ` on content lines, `>` on blank lines so paragraphs stay in
            // one quote on re-import.
            prefix_quote(inner, out);
        }
    }
}

/// CommonMark's thematic break: three or more of one of `-`, `_`, `*`, spaces
/// and tabs between them and nowhere else, under at most three of indent.
fn is_thematic_break(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return false;
    }
    let Some(c) = rest.chars().next().filter(|c| matches!(c, '-' | '_' | '*')) else {
        return false;
    };
    let mut n = 0;
    for ch in rest.chars() {
        if ch == c {
            n += 1;
        } else if ch != ' ' && ch != '\t' {
            return false;
        }
    }
    n >= 3
}

/// Prefix the first produced line with `first`, the rest but blank ones with
/// `cont`.
fn prefix_lines(inner: &str, first: &str, cont: &str, out: &mut String) {
    for (idx, line) in inner.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if idx == 0 || !line.is_empty() {
            out.push_str(if idx == 0 { first } else { cont });
            out.push_str(line);
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
/// break (`\` + newline); a code block renders one fence; a heading is a single
/// line. A block island's line is a `Para` whose slot's markup *is* the block,
/// so the island writes the whole line and `normalize` leaves it no
/// continuation to join to it.
fn emit_leaf_block(ctx: &Ctx, range: std::ops::Range<usize>, out: &mut String) {
    // Ahead of the kind, which cannot tell this line from prose. Markdown has
    // no syntax wrapping a block in an inline mark: `[<pipe table>](u)`
    // re-imports as prose with the island gone, so a mark reaching over the
    // slot projects as nothing.
    if let Some(isl) = block_island(ctx, range.start) {
        emit_island(isl, 0, out);
        return;
    }
    let first = &ctx.rt.lines[range.start];
    let dollar = block_escapes_dollar(ctx, range.clone());
    match &first.kind {
        LineKind::Code { lang } => emit_code(ctx, range, lang.as_deref(), out),
        LineKind::Heading { level } => {
            for _ in 0..*level {
                out.push('#');
            }
            out.push(' ');
            // Headings never carry continuations (import maps a hard break in a
            // heading to a space), so only the first line contributes.
            out.push_str(&render_inline(ctx, range.start, true, dollar));
        }
        // An empty line between hard breaks writes nothing, which a trailing
        // `\` would spell as text.
        LineKind::Para => {
            let parts: Vec<String> = range
                .map(|i| render_inline(ctx, i, false, dollar))
                .filter(|part| !part.is_empty())
                .collect();
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

/// Whether the block in `range` writes each `$` of its text as `\$`: one
/// that holds a `$` and, counting each equation's two delimiters, another.
/// A lone `$` pairs with nothing, so `$5` stays as written, and an equation
/// can span the block's hard breaks.
fn block_escapes_dollar(ctx: &Ctx, range: std::ops::Range<usize>) -> bool {
    let (mut dollars, mut maths) = (0, 0);
    for i in range {
        let text = seg_str(ctx, i);
        dollars += text.matches('$').count();
        let first = ctx.segments[i].slots_before;
        let slots = text.matches(ISLAND_SLOT).count();
        maths += ctx.rt.islands[first.min(ctx.rt.islands.len())..]
            .iter()
            .take(slots)
            .filter(|isl| isl.island_type == IslandType::Math)
            .count();
    }
    escapes_dollar(dollars, maths)
}

/// Whether a fragment holding `dollars` `$` chars in its text and `maths`
/// equations writes each `$` escaped.
fn escapes_dollar(dollars: usize, maths: usize) -> bool {
    dollars > 0 && dollars + 2 * maths >= 2
}

/// Whether the block in `range` is a paragraph whose every line is empty.
fn is_empty_para(ctx: &Ctx, range: std::ops::Range<usize>) -> bool {
    ctx.rt.lines[range.start].kind == LineKind::Para && range.into_iter().all(|i| seg_str(ctx, i).is_empty())
}

fn seg_str<'a>(ctx: &'a Ctx, i: usize) -> &'a str {
    let seg = &ctx.segments[i];
    &ctx.rt.text[seg.byte_start..seg.byte_end]
}

/// [`Content::block_island_at`] for line `i`, off the segment index this pass
/// already carries.
fn block_island<'a>(ctx: &'a Ctx, i: usize) -> Option<&'a Island> {
    let isl = ctx.rt.islands.get(ctx.segments[i].slots_before);
    isl.filter(|_| crate::model::is_block_island_line(seg_str(ctx, i), isl))
}

/// An island's markup; a footnote's is its reference, written with `label`
/// ([`note_labels`]).
fn emit_island(isl: &Island, label: usize, out: &mut String) {
    match isl.island_type {
        IslandType::Table => emit_table(isl, out),
        IslandType::Image => emit_image(isl, out),
        IslandType::Footnote => out.push_str(&format!("[^{label}]")),
        IslandType::Math => emit_math(isl, out),
    }
}

/// `$tex$`, or `$$tex$$` for a display equation.
fn emit_math(isl: &Island, out: &mut String) {
    let tex = isl.props.get("tex").and_then(|v| v.as_str()).unwrap_or("");
    let display = isl.props.get("display") == Some(&serde_json::Value::Bool(true));
    let delim = if display { "$$" } else { "$" };
    out.push_str(delim);
    out.push_str(tex);
    out.push_str(delim);
}

/// Whether an equation's `tex`, as it rests, reads back as itself between its
/// delimiters: `$tex$`, or `$$tex$$` where `display`, parsed alone. An empty
/// inline `tex`, an unescaped `$` and braces that do not balance are what it
/// refuses. The authored lanes refuse the rest
/// ([`crate::serial::from_authored_value`],
/// [`crate::serial::island_from_authored_value`]).
pub(crate) fn tex_is_writable(tex: &str, display: bool) -> bool {
    let delim = if display { "$$" } else { "$" };
    let md = format!("{delim}{tex}{delim}");
    let mut events = pulldown_cmark::Parser::new_ext(&md, crate::import::options())
        .filter(|e| !matches!(e, pulldown_cmark::Event::Start(_) | pulldown_cmark::Event::End(_)));
    let read = match (events.next(), events.next()) {
        (Some(pulldown_cmark::Event::InlineMath(t)), None) if !display => t,
        (Some(pulldown_cmark::Event::DisplayMath(t)), None) if display => t,
        _ => return false,
    };
    &*read == tex
}

/// A table as a GFM pipe table, or as HTML where it holds what a pipe table
/// cannot write ([`crate::html_table::writes_html`]).
fn emit_table(isl: &Island, out: &mut String) {
    let header = isl.props.get("header").and_then(|v| v.as_array());
    let rows = isl.props.get("rows").and_then(|v| v.as_array());
    let aligns = isl.props.get("aligns").and_then(|v| v.as_array());
    let cols = header.map(|h| h.len()).unwrap_or(0);
    if cols == 0 {
        return;
    }
    if crate::html_table::writes_html(&isl.props) {
        out.push_str(&crate::html_table::write(&isl.props, |cell| render_cell_md(cell, Frag::HtmlCell)));
        return;
    }
    // Cells are canonical `{text, marks}`; each cell's markdown is rebuilt from
    // that structure, so nothing re-parses markdown and `import(export(table))`
    // is a fixed point.
    let pipe_cell = |cell: &serde_json::Value| render_cell_md(cell, Frag::Cell);
    let mut table = String::from("| ");
    if let Some(h) = header {
        table.push_str(&h.iter().map(pipe_cell).collect::<Vec<_>>().join(" | "));
    }
    table.push_str(" |\n|");
    for k in 0..cols {
        let a = aligns
            .and_then(|a| a.get(k))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        table.push_str(match a {
            "left" => " :--- |",
            "center" => " :---: |",
            "right" => " ---: |",
            _ => " --- |",
        });
    }
    for r in rows.into_iter().flatten().filter_map(|row| row.as_array()) {
        // Pad/truncate so a ragged island (one that skipped normalization)
        // still emits a rectangular table.
        let mut cells: Vec<String> = r.iter().map(pipe_cell).collect();
        cells.resize(cols, String::new());
        table.push_str("\n| ");
        table.push_str(&cells.join(" | "));
        table.push_str(" |");
    }
    out.push_str(&table);
}

fn emit_image(isl: &Island, out: &mut String) {
    let url = isl.props.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let alt = isl.props.get("alt").and_then(|v| v.as_str()).unwrap_or("");
    // Alt is inline content of `![…]`: escape it like a link's display text so a
    // `]`/`\`/`&`/delimiter can't terminate the markup or decode on re-import.
    out.push_str("![");
    let dollar = escapes_dollar(alt.matches('$').count(), 0);
    out.push_str(&escape_run(&alt.chars().collect::<Vec<_>>(), Esc { frag: Frag::Prose, dollar }));
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
/// it is percent-encoded, ahead of the choice of form, and comes back in that
/// form.
fn emit_url(url: &str, out: &mut String) {
    let url = match url_is_writable(url) {
        true => std::borrow::Cow::Borrowed(url),
        false => std::borrow::Cow::Owned(url.replace('\n', "%0A").replace('\r', "%0D")),
    };
    if url_is_bare_safe(&url) {
        out.push_str(&url);
        return;
    }
    out.push('<');
    for c in url.chars() {
        match c {
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

/// Line `i`'s inline markdown, as a heading's text after its `#`s or as a
/// paragraph line, which escapes what would open a block. `dollar` escapes
/// each `$` ([`block_escapes_dollar`]).
fn render_inline(ctx: &Ctx, i: usize, heading: bool, dollar: bool) -> String {
    let escape_leading_block = !heading;
    let seg = &ctx.segments[i];
    let line_start = seg.start;
    let text = seg_str(ctx, i);
    let chars: Vec<char> = text.chars().collect();

    let (code_ranges, fmt, links) = bucket_marks(&ctx.rt.marks, line_start, chars.len(), false);
    let mut slot = seg.slots_before;
    let note_labels: Vec<Option<usize>> = chars
        .iter()
        .map(|&c| {
            if c != ISLAND_SLOT {
                return None;
            }
            slot += 1;
            let isl = ctx.rt.islands.get(slot - 1)?;
            (isl.island_type == IslandType::Footnote).then(|| ctx.labels[slot - 1])
        })
        .collect();
    // A footnote reference inside link text is not read as one, so a link
    // over a reference parts around it.
    let links: Vec<(usize, usize, &str)> = links
        .into_iter()
        .flat_map(|(s, e, url)| {
            let mut parts = Vec::new();
            let mut run = s;
            for at in (s..e).filter(|&at| note_labels[at].is_some()) {
                if run < at {
                    parts.push((run, at, url));
                }
                run = at + 1;
            }
            if run < e {
                parts.push((run, e, url));
            }
            parts
        })
        .collect();
    // The probe reads the line back alone, so it carries a definition for
    // each reference the line writes.
    let probe_tail: String = note_labels
        .iter()
        .flatten()
        .map(|label| format!("\n\n[^{label}]: x"))
        .collect();

    let escape_punct_at = if escape_leading_block {
        list_marker_punct(&chars)
    } else {
        None
    };

    let (mut md, mut points) = render_marked_core(
        &chars,
        &code_ranges,
        &fmt,
        &links,
        escape_punct_at,
        escape_leading_block,
        Esc { frag: Frag::Prose, dollar },
        &probe_tail,
        |pos_local| {
            let before = seg.slots_before + chars[..pos_local].iter().filter(|&&c| c == ISLAND_SLOT).count();
            ctx.rt.islands.get(before).map(|isl| {
                let mut markup = String::new();
                emit_island(isl, ctx.labels[before], &mut markup);
                markup
            })
        },
    );
    // A trailing `#` run reads as an ATX closing sequence on re-import
    // (`# a #` → heading text "a"). One escaped hash defeats the whole closer,
    // and `\#` re-imports as a literal `#`. A tag after the run sits after the
    // escape.
    if heading && md.ends_with('#') {
        let end = md.len();
        md.insert(end - 1, '\\');
        for point in points.iter_mut().filter(|p| **p == end) {
            *point += 1;
        }
    }
    match ctx.tags.get(i).filter(|t| !t.is_empty()) {
        Some(tags) => annotate(md, &points, tags),
        None => md,
    }
}

/// Where a line's text starts `<digits>.` or `<digits>)`, the punctuation to
/// escape so it does not re-import as an ordered list.
fn list_marker_punct(chars: &[char]) -> Option<usize> {
    let lead_digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    (lead_digits > 0 && lead_digits < chars.len() && matches!(chars[lead_digits], '.' | ')'))
        .then_some(lead_digits)
}

/// `md`, one line's markdown, with each of `tags` at its position's tag point
/// in `points`. A line whose tags change what it imports to holds them at its
/// end instead, and none where that changes it too: each spelling is read
/// back as the net reads one, inside `,…,`.
fn annotate(md: String, points: &[usize], tags: &[(Usv, &str)]) -> String {
    let reads = |line: &str| {
        crate::import::from_markdown(&format!(",{line},"))
            .ok()
            .map(|i| i.content)
    };
    let Some(plain) = reads(&md) else {
        return md;
    };
    let mut inline = String::new();
    let mut at = 0;
    for &(pos, id) in tags {
        let point = points[pos];
        inline.push_str(&md[at..point]);
        inline.push_str(&anchor_tag(id));
        at = point;
    }
    inline.push_str(&md[at..]);
    if reads(&inline).as_ref() == Some(&plain) {
        return inline;
    }
    let mut at_end = md.clone();
    for &(_, id) in tags {
        at_end.push_str(&anchor_tag(id));
    }
    if reads(&at_end).as_ref() == Some(&plain) {
        return at_end;
    }
    md
}

/// The carrier's canonical spelling of an anchor `id`.
fn anchor_tag(id: &str) -> String {
    crate::carrier::Element::spelling(crate::carrier::ANCHOR, [("ref".to_string(), id.to_string())].into())
        .expect("`anchor` and `ref` are in the carrier grammar")
        .wrap_inline("")
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

/// Render marks over a standalone char slice to markdown: the projection's mark
/// boundary sweep, shared by prose lines, table cells and footnote notes.
/// `code_ranges`/`fmt`/`links` are the marks clipped to `chars` (local
/// offsets); `esc` says which of the three `chars` is and whether a `$`
/// escapes; `probe_tail` follows a prose line when the net reads it back;
/// `island_markup_at` renders an island slot (prose) or yields `None` (cells
/// and notes carry no slot).
///
/// Returns the markdown and each position's tag point, one entry per position
/// `0..=n`: the byte offset in the markdown after the delimiters of the marks
/// closing there and before those opening there. A position inside a code
/// span or link takes the span's start's.
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
///   [`clip_asterisk_overlap`] nests the two by truncation, keeping the text and
///   losing the crossing tail.
#[allow(clippy::too_many_arguments)]
fn render_marked_core(
    chars: &[char],
    code_ranges: &[(usize, usize)],
    fmt: &[(usize, usize, &MarkKind)],
    links: &[(usize, usize, &str)],
    escape_punct_at: Option<usize>,
    escape_leading_block: bool,
    esc: Esc,
    probe_tail: &str,
    island_markup_at: impl Fn(usize) -> Option<String>,
) -> (String, Vec<usize>) {
    let escape_pipe = esc.frag == Frag::Cell;
    let n = chars.len();

    // A code span is emitted verbatim between backticks, so a slot inside it
    // would land raw and re-import as nothing, taking the island with it, and a
    // cell's `\n` would land raw and split the pipe row. Markdown has no honest
    // encoding for either (`` `![x](y)` `` and `` `<br>` `` are *literal* text),
    // so split each code range around every slot and `\n` it covers and let
    // the island or break render between the surviving code spans.
    //
    // A code span in an HTML table's cell ends at the subset's next tag, so a
    // range holding one splits around it too, the tag written as escaped text.
    let mut code_ranges: Vec<(usize, usize)> = code_ranges
        .iter()
        .flat_map(|&(s, e)| split_around_slots(chars, s, e))
        .flat_map(|(s, e)| match esc.frag {
            Frag::HtmlCell => split_around_table_tags(chars, s, e),
            _ => vec![(s, e)],
        })
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
    // A clip can land an edge on a cell's `\n`, where storage trims a mark's
    // edge, so the probe below would read back a mark other than the one asked.
    for m in fmt.iter_mut() {
        while m.0 < m.1 && chars[m.0] == '\n' {
            m.0 += 1;
        }
        while m.1 > m.0 && chars[m.1 - 1] == '\n' {
            m.1 -= 1;
        }
    }
    fmt.retain(|m| m.0 < m.1);
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
    //
    // Two marks over the *same* span have no outer by length, and which one takes
    // the inside decides whether either survives: `*` pairs only against
    // non-punctuation, so `~~*a*~~` holds where `*~~a~~*` leaves both delimiters
    // as literal text. The asterisk family goes innermost, against the content it
    // needs; `~~` and `<u>` pair regardless of what abuts them.
    let ast_last = |k: &MarkKind| u8::from(matches!(k, MarkKind::Strong | MarkKind::Emph));
    let mut by_start: Vec<usize> = (0..fmt.len()).collect();
    by_start.sort_by(|&a, &b| {
        fmt[a].0
            .cmp(&fmt[b].0)
            .then(fmt[b].1.cmp(&fmt[a].1))
            .then(ast_last(fmt[a].2).cmp(&ast_last(fmt[b].2)))
    });

    // An underline is a `<u>` tag pair. At one position its tags sit outside
    // the delimiters closing and opening there, so a tag's `<` and `>` never
    // abut a delimiter from inside its run, where they would change how it
    // flanks. An underline spanning the position stays open around the
    // delimiters, apart from their nesting.
    let is_underline = |fi: usize| matches!(fmt[fi].2, MarkKind::Underline);

    // A font mark is a `qm-span` pair, its tags outside the underlines' at a
    // position as theirs are outside the delimiters'. Font marks never overlap,
    // so neither do the pairs: one closes where the next opens.
    let is_font = |fi: usize| matches!(fmt[fi].2, MarkKind::FontFamily { .. });

    // One mark sweep over the marks `keep` selects (indices into `fmt`) → inline
    // markdown and its tag points.
    let sweep = |keep: &[bool], d: Delims| -> (String, Vec<usize>) {
        let mut out = String::new();
        let mut points = Vec::with_capacity(n + 1);
        // Marks currently open, outermost first, underlines and delimiters
        // apart. Storing the `fmt` index (not `(end, kind)`) keeps each open
        // mark's identity, so a reopened mark re-emits its OWN delimiter.
        let mut stack: Vec<usize> = Vec::new();
        let mut underlines: Vec<usize> = Vec::new();
        let mut font: Option<usize> = None;
        let (mut oi, mut li, mut ci) = (0usize, 0usize, 0usize);
        let mut pos = 0usize;
        while pos <= n {
            // A mark whose start the cursor already passed was jumped over by
            // an atomic span's interior and never opens; clipping keeps that
            // set empty.
            while oi < by_start.len() && fmt[by_start[oi]].0 < pos {
                oi += 1;
            }
            let from = oi;
            while oi < by_start.len() && fmt[by_start[oi]].0 == pos {
                oi += 1;
            }
            let opening: Vec<usize> = by_start[from..oi].iter().copied().filter(|&fi| keep[fi]).collect();
            let mut reopen: Vec<usize> = Vec::new();
            if let Some(idx) = stack.iter().position(|&fi| fmt[fi].1 == pos) {
                while stack.len() > idx {
                    let fi = stack.pop().unwrap();
                    out.push_str(delim(fmt[fi].2, d));
                    if fmt[fi].1 != pos {
                        reopen.push(fi);
                    }
                }
            }
            let mut reopen_underlines: Vec<usize> = Vec::new();
            if let Some(idx) = underlines.iter().position(|&fi| fmt[fi].1 == pos) {
                while underlines.len() > idx {
                    let fi = underlines.pop().unwrap();
                    out.push_str("</u>");
                    if fmt[fi].1 != pos {
                        reopen_underlines.push(fi);
                    }
                }
            }
            if let Some(fi) = font.filter(|&fi| fmt[fi].1 == pos) {
                out.push_str(&font_pair(fmt[fi].2).close_tag());
                font = None;
            }
            let point = out.len();
            points.push(point);
            if let Some(fi) = opening.iter().copied().find(|&fi| is_font(fi)) {
                out.push_str(&font_pair(fmt[fi].2).open_tag());
                font = Some(fi);
            }
            let fresh = opening.iter().copied().filter(|&fi| is_underline(fi));
            for fi in reopen_underlines.into_iter().rev().chain(fresh) {
                out.push_str("<u>");
                underlines.push(fi);
            }
            // Open formatting marks starting here BEFORE any atomic run, so a
            // formatting mark beginning at the same position as inline code/link
            // still wraps it rather than being dropped.
            let fresh = opening.iter().copied().filter(|&fi| !is_underline(fi) && !is_font(fi));
            for fi in reopen.into_iter().rev().chain(fresh) {
                out.push_str(delim(fmt[fi].2, d));
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
                // `[^1][…](…)` reads the reference as text, so an empty
                // comment, which the import drops, parts the two.
                if ls > 0 && chars[ls - 1] == ISLAND_SLOT && out.ends_with(']') {
                    out.push_str("<!---->");
                }
                out.push('[');
                for (i, &c) in chars[ls..le].iter().enumerate() {
                    if c == ISLAND_SLOT {
                        if let Some(slot) = island_markup_at(ls + i) {
                            out.push_str(&slot);
                        }
                    } else {
                        escape_char_into(c, i == 0, esc, &mut out);
                    }
                }
                out.push_str("](");
                let mut dest = String::new();
                emit_url(url, &mut dest);
                push_piped(&dest, escape_pipe, &mut out);
                out.push(')');
                points.resize(le, point);
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
                push_piped(&content, escape_pipe, &mut out);
                if pad {
                    out.push(' ');
                }
                out.push_str(&fence);
                points.resize(ce, point);
                pos = ce;
                continue;
            }
            if pos < n {
                let c = chars[pos];
                if c == ISLAND_SLOT {
                    if let Some(slot) = island_markup_at(pos) {
                        out.push_str(&slot);
                    }
                } else if Some(pos) == escape_punct_at
                    // A bare `!` before a link's `[` or a reference's `[^`
                    // would open an image.
                    || (c == '!' && links.get(li).is_some_and(|l| l.0 == pos + 1))
                    || (c == '!' && chars.get(pos + 1) == Some(&ISLAND_SLOT))
                    // After a footnote reference, `(` would read `[^1](…)` as
                    // a link and `:` would read `[^1]:` as a definition.
                    || (matches!(c, '(' | ':') && pos > 0 && chars[pos - 1] == ISLAND_SLOT && out.ends_with(']'))
                {
                    out.push('\\');
                    out.push(c);
                } else if let Some(space) = edge_space_ref(c)
                    && (pos < lead_edge || pos >= trail_edge)
                {
                    out.push_str(space);
                } else {
                    escape_char_into(c, pos == 0 && escape_leading_block, esc, &mut out);
                }
            }
            pos += 1;
        }
        // Clipping keeps every wrap `end` reachable, so this normally drains
        // nothing.
        while let Some(fi) = stack.pop() {
            out.push_str(delim(fmt[fi].2, d));
        }
        out.push_str(&"</u>".repeat(underlines.len()));
        if let Some(fi) = font {
            out.push_str(&font_pair(fmt[fi].2).close_tag());
        }
        (out, points)
    };

    // Verify-and-drop safety net. An editor's `apply_mark_ops` can build a mark
    // over a span markdown can't represent, and it lowers to a `**`/`*`/`~~` run
    // pulldown re-reads as literal text, leaking a delimiter into the content.
    // Re-parse the rendered line and, if its plain text drifted, search for a set
    // of flanking marks that keeps the text intact. Underlines and font marks
    // are read back beside them and dropped the same way: one the line cannot
    // hold comes back as other markup or as nothing.
    let is_flanking = |k: &MarkKind| {
        matches!(
            k,
            MarkKind::Strong | MarkKind::Emph | MarkKind::Strike | MarkKind::Underline | MarkKind::FontFamily { .. }
        )
    };
    let all = vec![true; fmt.len()];
    if !fmt.iter().any(|m| is_flanking(m.2)) {
        return sweep(&all, DELIM_SPELLINGS[0]);
    }
    // The probe wraps the fragment in `,…,`: parsed standalone, a leading `0. ` /
    // `# ` / `> ` would read as a list/heading/quote marker and drop a good mark.
    // A punctuation sentinel blocks every leading-block construct, preserves edge
    // whitespace, and is flanking-equivalent to the line start/end it replaces,
    // so it never masks or invents a leak.
    //
    // A slot whose island has no markdown projection re-imports as nothing, so
    // the expected text drops it; every other slot stays, so the probe still
    // catches a leak that eats an island's markup. A dropped slot shortens every
    // position after it, so the same pass records where each char lands:
    // `kept[i]` is char `i`'s position in the probe, `kept[n]` the end.
    let mut expected = String::with_capacity(n);
    let mut kept: Vec<Usv> = Vec::with_capacity(n + 1);
    let mut at = 1;
    for (i, &c) in chars.iter().enumerate() {
        kept.push(at);
        if c != ISLAND_SLOT || island_markup_at(i).is_some() {
            expected.push(c);
            at += 1;
        }
    }
    kept.push(at);
    let want = format!(",{expected},");
    // The flanking marks a `keep` set asks for, in the probe's coordinates and the
    // form re-import hands back: a mark the sweep splits around an overlap
    // re-imports as one span, so both sides are normalized before they are compared.
    let want_marks = |keep: &[bool]| {
        crate::model::normalize_marks(
            fmt.iter()
                .enumerate()
                .filter(|&(i, m)| keep[i] && is_flanking(m.2))
                .map(|(_, &(s, e, k))| Mark::new(kept[s], kept[e], k.clone()))
                .collect(),
        )
    };
    // `None` where the rendering leaks a delimiter into the text, else whether the
    // marks it was asked to carry came back as themselves. A mark that re-imports
    // over different text is not the mark the content held: `**a**_b_**c**` lowers
    // to a `***` run CommonMark re-segments into one `Strong` over all three spans,
    // spending no character and moving where bold starts and ends.
    //
    // A cell fragment is read back as a cell and a note as a note, where `<br>`
    // is a `\n` wherever it sits; prose reads a break at a mark edge or a
    // second one in a row differently.
    let probe = |md: &str, want_marks: &[Mark]| -> Option<bool> {
        let (text, marks) = match esc.frag {
            Frag::Cell => {
                let rt = crate::import::from_markdown(&format!("| h |\n| --- |\n| ,{md}, |")).ok()?.content;
                let cell = rt.islands.first()?.props.get("rows")?.get(0)?.get(0)?;
                crate::serial::parse_cell(cell)
            }
            Frag::HtmlCell => {
                let rt = crate::import::from_markdown(&format!("<table><tr><td>,{md},</td></tr></table>")).ok()?.content;
                crate::serial::parse_cell(rt.islands.first()?.props.get("header")?.get(0)?)
            }
            Frag::Note => {
                let rt = crate::import::from_markdown(&format!("a[^1]\n\n[^1]: ,{md},")).ok()?.content;
                crate::serial::parse_cell(&rt.islands.first()?.props)
            }
            Frag::Prose => {
                let rt = crate::import::from_markdown(&format!(",{md},{probe_tail}")).ok()?.content.into_content();
                (rt.text, rt.marks)
            }
        };
        if text != want {
            return None;
        }
        let got: Vec<&Mark> = marks.iter().filter(|m| is_flanking(&m.kind)).collect();
        Some(got.into_iter().eq(want_marks.iter()))
    };
    // Re-spell before dropping anything. The leak is rarely an unrepresentable
    // span; it is two same-character delimiters abutting: a `Strong` closing `**`
    // against the `*` of an `Emph` that starts where it ends makes a `***` run,
    // and CommonMark re-segments that run rather than pairing it as written.
    // Markdown spells both kinds a second way, and `__a\*\*__*b*` carries what
    // `**a\*\***​*b*` loses — same content, delimiters that cannot merge. `**`/`*`
    // is swept first and returned whole on a line it already round-trips, so the
    // `_` forms are reached only where the leader is *shown* to lose something,
    // and each is verified before it is used. Four probes at most.
    let intent = want_marks(&all);
    for &d in &DELIM_SPELLINGS {
        let cand = sweep(&all, d);
        if probe(&cand.0, &intent) == Some(true) {
            return cand;
        }
    }
    // The flanking marks in document order. That order is the re-add priority
    // below (when two marks can't both survive, the earlier one wins) and is the
    // one user-visible choice in this search, so it is fixed here rather than
    // falling out of the traversal.
    let cands: Vec<usize> = (0..fmt.len()).filter(|&i| is_flanking(fmt[i].2)).collect();
    // Render with only the flanking marks in `keep`; every other mark rides.
    // The default spelling: a line reaching here has had all four rejected with
    // every mark on, so the choice is which marks survive, not how they are spelled.
    let mask_of = |keep: &[usize]| -> Vec<bool> {
        let mut mask: Vec<bool> = fmt.iter().map(|m| !is_flanking(m.2)).collect();
        for &i in keep {
            mask[i] = true;
        }
        mask
    };
    let render = |keep: &[usize]| sweep(&mask_of(keep), DELIM_SPELLINGS[0]);
    let survives = |md: &str, keep: &[usize]| probe(md, &want_marks(&mask_of(keep))) == Some(true);
    // Drop the whole flanking set, then re-add by halves: a chunk that survives is
    // accepted whole, one that doesn't splits and its halves are retried, a lone
    // mark that still doesn't is dropped. `m` marks cost ~2·log(m) probes when one
    // is at fault and 2 when none can survive. Greedy, so the result is a maximal
    // surviving set, not necessarily the largest one.
    //
    // Dropping every flanking mark is the floor the search can't go below, so if
    // even that fails, no re-add can fix it. A lone candidate has nowhere to
    // split, so the floor is already its answer.
    let mut out = render(&[]);
    if cands.len() == 1 || !survives(&out.0, &[]) {
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
        if survives(&md.0, &trial) {
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

/// The maximal subranges of `[start, end)` free of [`ISLAND_SLOT`] and `\n`:
/// the range itself when it covers neither, else one range per run between
/// them (empty runs dropped). Used to keep a code span off an island or a cell
/// break it cannot represent.
fn split_around_slots(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let cut = |c: char| c == ISLAND_SLOT || c == '\n';
    let end = end.min(chars.len());
    if !chars[start.min(end)..end].iter().any(|&c| cut(c)) {
        return vec![(start, end)];
    }
    let mut out = Vec::new();
    let mut run = start;
    for (i, _) in chars[start..end]
        .iter()
        .enumerate()
        .map(|(i, c)| (start + i, c))
        .filter(|&(_, &c)| cut(c))
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

/// The pieces of the code range `start..end` of `chars` outside each of the
/// HTML table subset's tags its text holds.
fn split_around_table_tags(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let text: String = chars[start..end].iter().collect();
    let mut at = start;
    let mut out = Vec::new();
    for (piece, tag) in crate::html_table::tag_pieces(&text) {
        let len = text[piece].chars().count();
        if !tag {
            out.push((at, at + len));
        }
        at += len;
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
/// sweep, in a pipe table with `|`→`\|` escaping so the cell survives
/// re-import through `pulldown`'s pipe splitting. A cell is flat inline: no
/// islands, no leading-block escape.
fn render_cell_md(v: &serde_json::Value, frag: Frag) -> String {
    let (text, marks) = crate::serial::parse_cell(v);
    let chars: Vec<char> = text.chars().collect();
    let (code_ranges, fmt, links) = bucket_marks(&marks, 0, chars.len(), true);
    let esc = Esc { frag, dollar: escapes_dollar(text.matches('$').count(), 0) };
    render_marked_core(&chars, &code_ranges, &fmt, &links, None, false, esc, "", |_| None).0
}

/// A footnote's note as the text of its definition: a cell's sweep on one
/// source line, opening a block as a paragraph line does.
fn render_note_md(v: &serde_json::Value) -> String {
    let (text, marks) = crate::serial::parse_cell(v);
    let chars: Vec<char> = text.chars().collect();
    let (code_ranges, fmt, links) = bucket_marks(&marks, 0, chars.len(), true);
    let esc = Esc { frag: Frag::Note, dollar: escapes_dollar(text.matches('$').count(), 0) };
    let punct = list_marker_punct(&chars);
    render_marked_core(&chars, &code_ranges, &fmt, &links, punct, true, esc, "", |_| None).0
}

/// What a fragment [`render_marked_core`] renders is, and so the escapes it
/// takes and how its net reads it back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Frag {
    Prose,
    /// A pipe table's cell: `|` escaped, a `\n` a `<br>`.
    Cell,
    /// An HTML table's cell: a `\n` a `<br>`.
    HtmlCell,
    /// A footnote's note: a `\n` a `<br>`.
    Note,
}

/// The escapes a fragment's text takes: its [`Frag`]'s, and `$` as `\$`
/// where `dollar`.
#[derive(Clone, Copy)]
struct Esc {
    frag: Frag,
    dollar: bool,
}

/// Which of markdown's two spellings each asterisk-family kind is emitted with.
/// The pair is chosen per line, not per mark: a run is only ambiguous where two
/// delimiters *of the same character* abut, so swapping one kind's spelling is
/// what breaks the run up.
#[derive(Clone, Copy)]
struct Delims {
    strong: &'static str,
    emph: &'static str,
}

/// The spellings the net tries, in order. `**`/`*` leads because `_` cannot do
/// intraword emphasis (CommonMark flanking), so `_a_你` re-imports as literal
/// text where `*a*你` emphasizes correctly; the rest are reached only once the
/// leader has been *shown* to lose text, and each is verified before it is used.
const DELIM_SPELLINGS: [Delims; 4] = [
    Delims { strong: "**", emph: "*" },
    Delims { strong: "__", emph: "*" },
    Delims { strong: "**", emph: "_" },
    Delims { strong: "__", emph: "_" },
];

/// The `qm-span` pair a `font_family` mark writes.
fn font_pair(kind: &MarkKind) -> crate::carrier::Element {
    match kind {
        MarkKind::FontFamily { name } => crate::carrier::span::pair(name),
        _ => unreachable!("only a font_family mark is a qm-span pair"),
    }
}

/// The delimiter that opens and closes `kind`. Code, link, anchor, underline
/// and font family are written elsewhere.
fn delim(kind: &MarkKind, d: Delims) -> &'static str {
    match kind {
        MarkKind::Strong => d.strong,
        MarkKind::Emph => d.emph,
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

/// Push `s`, a code span's content or a link's destination, with each `|`
/// as `\|` where `escape_pipe`: a table row splits its cells at a `|` inside
/// either, and reads `\|` as `|` before it parses the cell.
fn push_piped(s: &str, escape_pipe: bool, out: &mut String) {
    if escape_pipe {
        out.push_str(&s.replace('|', "\\|"));
    } else {
        out.push_str(s);
    }
}

fn escape_run(chars: &[char], esc: Esc) -> String {
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        escape_char_into(*c, i == 0, esc, &mut s);
    }
    s
}

/// Push `c` into `out` escaped so it re-imports as literal text: the char
/// verbatim, or a `&'static str` escape. `leading` also escapes block-starter
/// chars that would otherwise open a heading/list/quote. A cell adds
/// `|`→`\|`, and a cell or note `\n`→`<br>`, so each survives on its one
/// source line; `esc.dollar` adds `$`→`\$`.
fn escape_char_into(c: char, leading: bool, esc: Esc, out: &mut String) {
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
        '|' if esc.frag == Frag::Cell => "\\|",
        '\n' if esc.frag != Frag::Prose => "<br>",
        '$' if esc.dollar => "\\$",
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
    use crate::model::{Line, Mark, MarkKind};

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
    /// and no mint rejects.
    #[test]
    fn nesting_past_what_validate_allows_projects_rather_than_overflowing() {
        const DEPTH: usize = 10_000;
        let mut line = Line::new(LineKind::Para);
        line.containers = vec![Container::Quote { instance: 0 }; DEPTH];
        let rt = Content::new("x".to_string(), vec![line]).into_normalized();
        assert!(rt.validate().is_err(), "the shape `validate` refuses");
        assert_eq!(to_markdown(&rt), format!("{}x", "> ".repeat(DEPTH)));
    }

    /// Coincident marks nest in the stored order, `MarkKind::sort_key` at a tie,
    /// so this projection's bytes move when that key does. Both spellings
    /// re-import to one content: the fixed point is over the content, not over
    /// the markdown.
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
        assert_eq!(from_markdown("**~~x~~**").unwrap().content, rt);
    }

    fn li(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: false,
            start: 1,
            ordinal,
            checked: None,
            instance,
        }]
    }

    fn oli(ordinal: u64, instance: u64) -> Vec<Container> {
        vec![Container::ListItem {
            ordered: true,
            start: 1,
            ordinal,
            checked: None,
            instance,
        }]
    }

    fn note(text: &str, id: &str) -> Island {
        Island::new(id.into(), IslandType::Footnote).with_props(serde_json::json!({ "text": text, "marks": [] }))
    }

    fn math(tex: &str, display: bool, id: &str) -> Island {
        let props = if display {
            serde_json::json!({ "tex": tex, "display": true })
        } else {
            serde_json::json!({ "tex": tex })
        };
        Island::new(id.into(), IslandType::Math).with_props(props)
    }

    /// A reference abutting what would read it as a link or a definition
    /// escapes that char; a note opening on what would open a block escapes
    /// it, its edge line breaks dropping and an inner one written as `<br>`.
    #[test]
    fn a_footnote_reference_and_its_note_round_trip() {
        let rt = Content::new(
            "\u{FFFC}: a\u{FFFC}(b)\n\u{FFFC}".into(),
            vec![Line::new(LineKind::Para), Line::new(LineKind::Heading { level: 2 })],
        )
        .with_islands(vec![note("\n# x\ny\n", "isl-0"), note("1. z", "isl-1"), note("", "isl-2")])
        .into_normalized();
        rt.validate().expect("validates");
        assert_eq!(crate::serial::parse_cell(&rt.islands[0].props).0, "# x\ny");
        let md = to_markdown(&rt);
        assert_eq!(
            md,
            "[^1]\\: a[^2]\\(b)\n\n## [^3]\n\n[^1]: \\# x<br>y\n\n[^2]: 1\\. z\n\n[^3]: "
        );
        assert_eq!(from_markdown(&md).unwrap().content, rt);
    }

    /// Link text holds no footnote reference, so a link over one parts around
    /// it: the text and the note come back, the link as two.
    #[test]
    fn a_link_over_a_footnote_reference_parts_around_it() {
        let rt = Content::new("a\u{FFFC}b".into(), vec![Line::new(LineKind::Para)])
            .with_marks(vec![Mark::new(0, 3, MarkKind::Link { url: "u".into() })])
            .with_islands(vec![note("n", "isl-0")])
            .into_normalized();
        let md = to_markdown(&rt);
        assert_eq!(md, "[a](u)[^1]<!---->[b](u)\n\n[^1]: n");
        let back = from_markdown(&md).unwrap().content;
        assert_eq!((&back.text, &back.islands), (&rt.text, &rt.islands));
        assert_eq!(back.marks.len(), 2);
    }

    /// An equation writes between its delimiters, and a `$` of the text
    /// escapes only where another `$` or an equation could pair with it.
    #[test]
    fn math_and_the_dollars_beside_it_round_trip() {
        let cases: &[(&str, Vec<Island>, &str)] = &[
            ("a\u{FFFC} costs $5", vec![math("x^2", false, "isl-0")], "a$x^2$ costs \\$5"),
            ("$5 or $6", vec![], "\\$5 or \\$6"),
            ("$5", vec![], "$5"),
            ("\u{FFFC}", vec![math("\\frac{a}{b}", true, "isl-0")], "$$\\frac{a}{b}$$"),
            ("p\u{FFFC}q", vec![math("\\{a\\} \\$", false, "isl-0")], "p$\\{a\\} \\$$q"),
        ];
        for (text, islands, md) in cases {
            let rt = Content::new((*text).into(), vec![Line::new(LineKind::Para)])
                .with_islands(islands.clone())
                .into_normalized();
            assert_eq!(to_markdown(&rt), *md, "{text:?}");
            assert_eq!(from_markdown(md).unwrap().content, rt, "{md:?}");
        }
        let cell = Content::new("\u{FFFC}".into(), vec![Line::new(LineKind::Para)])
            .with_islands(vec![Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
                "header": [{"text": "$a$", "marks": []}],
                "aligns": ["none"],
                "rows": [],
            }))])
            .into_normalized();
        let md = to_markdown(&cell);
        assert!(md.starts_with("| \\$a\\$ |"), "{md:?}");
        assert_eq!(from_markdown(&md).unwrap().content, cell);
    }

    /// TeX reads back between its delimiters only where it opens and closes
    /// on no space, holds no bare `$`, and balances its braces.
    #[test]
    fn tex_is_writable_where_its_delimiters_read_it_back() {
        for (tex, display, writable) in [
            ("x", false, true),
            ("a \\$ b", false, true),
            ("\\{", false, true),
            ("", false, false),
            ("", true, true),
            ("a$b", false, false),
            ("{a", false, false),
            ("a}", true, false),
        ] {
            assert_eq!(tex_is_writable(tex, display), writable, "{tex:?} {display}");
        }
    }

    /// A task marker opens its item's paragraph: an item opening on any other
    /// block writes the marker alone above it, and an empty one ends on a
    /// space, the marker reading as one only before whitespace.
    #[test]
    fn a_task_item_writes_its_marker_where_it_reads_back() {
        for md in ["- [x] a\n\n- [ ]\n  # h", "1. [ ]\n   > q\n\n2. [x] ", "- a\n\n  - [ ] "] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(to_markdown(&rt), md);
        }
    }

    /// `instance` spells these, and the marker alternation CommonMark already
    /// reads carries them through markdown. Each is checked *from storage*, not
    /// from an import, since a client writing `Content` directly is the lane
    /// that mints them.
    #[test]
    fn adjacent_sibling_containers_project_and_return() {
        let cases: &[(Normalized, &str)] = &[
            // Two one-item lists, which weld into one without a discriminator
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
            assert_eq!(&from_markdown(&md).unwrap().content, rt, "{md:?} did not return");
        }
    }

    /// `+ ***` is a list item holding a thematic break; `* ***` would be four
    /// spaced asterisks, a break outright that takes the item with it — which is
    /// why the second bullet is `+`.
    #[test]
    fn alternate_markers_do_not_collide_with_a_rule() {
        let mut rt = stored("a\n\nb", vec![li(0, 0), li(0, 1), li(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_normalized();
        rt.validate().expect("validates");
        assert_eq!(to_markdown(&rt), "- a\n\n+ ***\n\n+ b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap().content, rt);

        let mut rt = stored("a\n\nb", vec![oli(0, 0), oli(0, 1), oli(1, 1)]).into_content();
        rt.lines[1].kind = LineKind::Rule;
        let rt = rt.into_normalized();
        assert_eq!(to_markdown(&rt), "1. a\n\n1) ***\n\n2) b");
        assert_eq!(from_markdown(&to_markdown(&rt)).unwrap().content, rt);
    }

    fn round_trips(md: &str) {
        let rt = from_markdown(md).unwrap().content;
        let md2 = to_markdown(&rt);
        let rt2 = from_markdown(&md2).unwrap().content;
        assert_eq!(
            rt, rt2,
            "content not a fixed point.\n  in:  {md:?}\n  mid: {md2:?}"
        );
    }

    /// A `!` before a link stays text, not the `!` of an image.
    #[test]
    fn a_bang_before_a_link_round_trips() {
        round_trips("\\![i](y)");
        round_trips("wow!![i](y)");
    }

    /// The link arm must route its display text through the island renderer
    /// rather than emit the slot char raw, which re-imports as nothing.
    #[test]
    fn link_over_island_slot_round_trips() {
        round_trips("[![a cat](cat.png)](https://e.com)");
        round_trips("[a ![cat](cat.png) b](https://e.com)");
        assert_eq!(
            to_markdown(&from_markdown("[![a cat](cat.png)](https://e.com)").unwrap().content),
            "[![a cat](cat.png)](https://e.com)"
        );
    }

    /// Editor-buildable, not importable. A code span is emitted verbatim, so it
    /// splits around the slot, keeping both the text and the island.
    #[test]
    fn code_mark_over_island_slot_keeps_the_island() {
        let mut rt = from_markdown("a ![x](y.png) b").unwrap().content.into_content();
        rt.marks.push(Mark { start: 0, end: 5, kind: MarkKind::Code });
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()));
        let md = to_markdown(&rt);
        assert_eq!(md, "`a `![x](y.png)` b`");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, rt.text);
        assert_eq!(rt2.islands.len(), 1);
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
                island_type: IslandType::Image,
                props: serde_json::Value::Null,
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
            ("hard_break", "line one\\\nline two"),
            ("blockquote", "> quoted text"),
            ("link", "see [our site](https://example.com) now"),
            ("table", "| a | b |\n| --- | --- |\n| 1 | 2 |"),
            ("image", "see ![a cat](cat.png) here"),
            ("element", "<qm-keep note=\"x\">\n\npara\n\n</qm-keep>"),
            ("element_around_list", "<qm-keep>\n\n- a\n\n- b\n\n</qm-keep>"),
            ("element_in_item", "- <qm-keep>\n\n  a\n\n  </qm-keep>"),
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
            let rt = from_markdown(&format!("one\n\n{src}\n\ntwo")).unwrap().content;
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
        assert_eq!(to_markdown(&from_markdown("* ---").unwrap().content), "- ***");
        // The shapes that never collide, pinned against a fix that trades one
        // collision for another: swapping the bullet marker to `*`/`+` starts a
        // *new* list, resetting `ordinal` on this item and every one after.
        round_trips("1. ---");
        round_trips("- one\n\n  ---");
        round_trips("- a\n- ***\n- c");
    }

    /// The other half of that collision: not a rule *inside* an item but the
    /// markers themselves. Three nested empty bullets spell `- - - `, a break
    /// that outranks the items spelling it, and the nesting is gone after one
    /// pass. The content moves off the marker line rather than changing a
    /// marker char — the last case is why, since the empty item shares its list
    /// with `a` and a bullet char change would take `a` into a new list.
    #[test]
    fn a_marker_run_that_spells_a_rule_breaks_its_line() {
        for md in ["+ + +", "+ + + + +", "> + + +", "+ + +\n    + a", "+ + + a"] {
            round_trips(md);
        }
    }

    #[test]
    fn literal_asterisks_escaped() {
        round_trips("2 * 3 = 6 and a_b_c");
    }

    #[test]
    fn hard_break_in_list_item() {
        round_trips("- one\\\ntwo\n- three");
    }

    #[test]
    fn leading_ordered_marker_escaped() {
        let mut rt = from_markdown("x").unwrap().content.into_content();
        rt.text = "1. not a list".into();
        let rt = rt.into_normalized();
        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.lines[0].kind, LineKind::Para);
        assert!(back.lines[0].containers.is_empty());
        assert_eq!(back, rt);
    }

    /// Markdown spells an empty paragraph, and an empty line between hard
    /// breaks, with nothing: beside other content each writes nothing, where a
    /// list item's would push what follows out of the list and a trailing
    /// break's `\` would read as text. A container's only line keeps it.
    #[test]
    fn an_empty_line_beside_other_content_writes_nothing() {
        let item = |checked| Container::ListItem { ordered: false, start: 1, ordinal: 0, checked, instance: 0 };
        let quote = Container::Quote { instance: 0 };
        for (rt, md) in [
            (hard_break_block("a\n"), "a"),
            (hard_break_block("a\n\nb"), "a\\\nb"),
            (stored("\nb", vec![vec![], vec![]]), "b"),
            (stored("\nb", vec![vec![item(None)]; 2]), "- b"),
            (stored("\nb", vec![vec![item(Some(true))]; 2]), "- [x] b"),
            (stored("\nb", vec![vec![item(Some(true))], vec![item(Some(true)), item(None)]]), "- [x]\n  - b"),
            (stored("\nb", vec![vec![quote.clone()]; 2]), "> b"),
            (stored("", vec![vec![item(Some(false))]]), "- [ ] "),
        ] {
            let out = to_markdown(&rt);
            assert_eq!(out, md);
            assert_eq!(to_markdown(&from_markdown(&out).unwrap().content), out);
        }
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
                    &from_markdown(&md).unwrap().content,
                    &rt,
                    "{text:?} under {containers:?} did not return: {md:?}"
                );
            }
            let mut rt = stored(text, vec![vec![]]).into_content();
            rt.lines[0].kind = LineKind::Heading { level: 2 };
            let rt = rt.into_normalized();
            let md = to_markdown(&rt);
            assert_eq!(
                &from_markdown(&md).unwrap().content,
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
            assert_eq!(&from_markdown(&md).unwrap().content, &rt, "{text:?} → {md:?}");
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
            assert_eq!(&from_markdown(&md).unwrap().content, &rt, "{text:?} → {md:?}");
        }
    }

    /// The verify-and-drop net wraps its probe in `,…,` to block every
    /// leading-block construct, so it cannot see the indented code block an
    /// unescaped edge run opens: it reads such a line as safe while the `**`
    /// ships into the text. The escape is the only thing holding this line.
    #[test]
    fn a_marked_line_with_edge_whitespace_keeps_its_delimiters_out_of_the_text() {
        let rt = marked("    foo", vec![Mark::new(4, 7, MarkKind::Strong)]);
        let back = from_markdown(&to_markdown(&rt)).unwrap().content;
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
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![],
            islands: vec![
                Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
                    "aligns": ["none"],
                    "header": [cell(" h ")],
                    "rows": [[cell("  a")], [cell("b  ")], [cell("   ")]],
                })),
            ],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "table island invalid");
        let md = to_markdown(&rt);
        assert_eq!(&from_markdown(&md).unwrap().content, &rt, "cell edges lost: {md:?}");
    }

    /// The island writes its line whole, so a mark reaching over its slot
    /// projects as nothing: `[<pipe table>](u)` re-imports as prose with the
    /// island gone, taking the table out of the document. Landing a table
    /// inside a link run is how a host reaches the shape — the `\n` that opens
    /// the island's line extends the mark over its slot.
    #[test]
    fn a_mark_reaching_over_a_block_islands_slot_does_not_wrap_its_markup() {
        let mut rt = from_markdown("[abc](u)").unwrap().content;
        rt.apply_field_change(&crate::ops::ChangeBundle {
            delta: crate::delta::Delta {
                ops: vec![
                    crate::delta::Op::Retain(1),
                    crate::delta::Op::Insert("\n\n".into()),
                    crate::delta::Op::Retain(2),
                ],
            },
            island_ops: vec![crate::ops::IslandOp::Insert {
                at: 2,
                island: Island::new("isl-t".into(), IslandType::Table).with_props(
                    serde_json::json!({
                        "aligns": ["none"],
                        "header": [{"marks": [], "text": "h"}],
                        "rows": [[{"marks": [], "text": "c"}]],
                    }),
                ),
            }],
            ..Default::default()
        })
        .expect("the slot is alone on its line");
        assert_eq!(rt.text, format!("a\n{ISLAND_SLOT}\nbc"));
        assert_eq!(
            rt.marks,
            vec![
                Mark::new(0, 1, MarkKind::Link { url: "u".into() }),
                Mark::new(2, 3, MarkKind::Link { url: "u".into() }),
                Mark::new(4, 6, MarkKind::Link { url: "u".into() }),
            ],
            "the link splits at each line join, the slot's line included"
        );

        let md = to_markdown(&rt);
        assert_eq!(md, "[a](u)\n\n| h |\n| --- |\n| c |\n\n[bc](u)");
        assert_eq!(
            from_markdown(&md).unwrap().content.islands.len(),
            1,
            "the table left the document: {md:?}"
        );
    }

    #[test]
    fn table_with_formatted_cells_round_trips() {
        round_trips("| Name | Note |\n| --- | --- |\n| **bold** | _italic_ |");
        round_trips("| A |\n| --- |\n| **b** and _i_ `c` [d](https://e.com) ~~e~~ |");
        round_trips("| A |\n| --- |\n| <u>under</u> |");
        // A literal pipe inside a cell survives via `\|` re-escaping on export.
        round_trips("| A |\n| --- |\n| a \\| b |");
    }

    /// A row splits its cells at a `|` inside a code span or a link
    /// destination too, so export writes it `\|` there as in text.
    #[test]
    fn a_pipe_in_a_cells_code_span_or_link_url_stays_in_its_cell() {
        for (body, text, url) in [
            ("`a\\|b`", "a|b", None),
            ("`a\\\\|b`", "a\\|b", None),
            ("[x](https://e.com/a\\|b)", "x", Some("https://e.com/a|b")),
            ("[x](<https://e.com/a\\\\\\|b c>)", "x", Some("https://e.com/a\\|b c")),
        ] {
            let md = format!("| h |\n| --- |\n| {body} |");
            let rt = from_markdown(&md).unwrap().content;
            let cell = &rt.islands[0].props["rows"][0][0];
            assert_eq!(cell["text"], text, "{md:?}");
            assert_eq!(cell["marks"][0]["attrs"]["url"].as_str(), url, "{md:?}");
            assert_eq!(to_markdown(&rt), md, "{md:?}");
        }
    }

    fn grid_table(props: serde_json::Value) -> Normalized {
        with_islands(&ISLAND_SLOT.to_string(), vec![Island::new("isl-0".into(), IslandType::Table).with_props(props)])
    }

    /// Storage decides the spelling: a table holding nothing a pipe table
    /// cannot write is pipes, one key it cannot write makes it HTML, and
    /// dropping that key makes it pipes again. A value outside its set writes
    /// nothing, and each spelling reads back as the table it writes.
    #[test]
    fn a_table_is_html_exactly_where_pipes_cannot_carry_it() {
        use serde_json::json;
        let cell = |text: &str| json!({"marks": [], "text": text});
        let plain = json!({"aligns": ["right", "none"], "header": [cell("a"), cell("b")], "rows": [[cell("1"), cell("")], [cell("2"), cell("3")]]});
        let keys: &[(&str, serde_json::Value, bool)] = &[
            ("/head", json!(0), true),
            ("/head", json!(2), true),
            ("/widths", json!([1, null]), true),
            ("/align", json!("center"), true),
            ("/rows/0/0/align", json!("left"), true),
            ("/header/1/valign", json!("bottom"), true),
            ("/rows/0/0/colspan", json!(2), true),
            ("/header/0/rowspan", json!(2), false),
            ("/rows/1/1/rowspan", json!(3), false),
            ("/rows/0/0/colspan", json!(1), false),
            ("/rows/0/0/valign", json!("horizon"), false),
            ("/head", json!(1), false),
        ];
        let pipes = to_markdown(&grid_table(plain.clone()));
        assert!(pipes.starts_with("| a | b |"), "{pipes}");
        for (at, value, html) in keys {
            let mut props = plain.clone();
            let (parent, key) = at.rsplit_once('/').unwrap();
            props.pointer_mut(parent).unwrap().as_object_mut().unwrap().insert(key.into(), value.clone());
            let rt = grid_table(props);
            let md = to_markdown(&rt);
            assert_eq!(md.starts_with("<table"), *html, "{at}: {value} wrote {md}");
            if !html {
                assert_eq!(md, pipes, "{at}: {value}");
            }
            let back = from_markdown(&md).unwrap();
            assert!(back.warnings.is_empty(), "{at}: {value}: {:?}", back.warnings);
            if *value != json!("horizon") {
                assert_eq!(back.content, rt, "{at}: {value} wrote {md}");
            }
        }
    }

    /// An HTML cell's text writes so no `<` reads as a tag and no `&` as an
    /// entity, a link destination holding a table tag included, and a `|`
    /// needs no escape there. A code span never holds a table tag, so a code
    /// mark over one writes it as escaped text between the spans either side.
    #[test]
    fn an_html_cell_writes_what_would_read_as_markup() {
        use serde_json::json;
        let text = "a | </td> &amp; <tr> x";
        let marks = json!([
            {"start": 2, "end": 9, "type": "code"},
            {"start": 16, "end": 20, "type": "link", "attrs": {"url": "td x</tr>"}},
            {"start": 0, "end": 1, "type": "strong"},
        ]);
        let rt = grid_table(json!({
            "aligns": ["none"], "head": 0,
            "header": [{"marks": marks, "text": text}],
            "rows": [],
        }));
        let md = to_markdown(&rt);
        assert_eq!(
            md,
            "<table>\n<tbody>\n<tr><td>**a** `| `\\</td> \\&amp; [\\<tr>](<td x\\</tr\\>>) x</td></tr>\n</tbody>\n</table>"
        );
        let back = from_markdown(&md).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        let (text_back, marks_back) = crate::serial::parse_cell(&back.content.islands[0].props["header"][0]);
        assert_eq!(text_back, text);
        assert!(marks_back.contains(&Mark::new(2, 4, MarkKind::Code)), "{marks_back:?}");
        assert_eq!(to_markdown(&back.content), md);
    }

    #[test]
    fn formatted_cell_marks_are_structured_not_reparsed() {
        let rt = from_markdown("| H |\n| --- |\n| **bold** |").unwrap().content;
        let cell = &rt.islands[0].props["rows"][0][0];
        assert_eq!(cell["text"], "bold");
        assert_eq!(cell["marks"][0]["type"], "strong");
        assert_eq!(cell["marks"][0]["start"], 0);
        assert_eq!(cell["marks"][0]["end"], 4);
        assert!(to_markdown(&rt).contains("**bold**"));
    }

    #[test]
    fn a_cell_line_break_round_trips_as_br() {
        for body in [
            "line1<br>line2",
            "a <br> b",
            "<br>a",
            "a<br>",
            "a<br><br>b",
            "**a<br>b**",
            "\\<br>",
        ] {
            let md = format!("| h |\n| --- |\n| {body} |");
            let rt = from_markdown(&md).unwrap().content;
            assert_eq!(to_markdown(&rt), md, "cell {body:?}");
        }
        let rt = from_markdown("| h |\n| --- |\n| \\<br> |").unwrap().content;
        assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "<br>");
    }

    #[test]
    fn every_br_spelling_is_one_cell_break() {
        for br in ["<br/>", "<br />", "<BR>", "<br clear=\"all\">"] {
            let rt = from_markdown(&format!("| h |\n| --- |\n| a{br}b |")).unwrap().content;
            assert_eq!(rt.islands[0].props["rows"][0][0]["text"], "a\nb", "{br}");
            assert_eq!(to_markdown(&rt), "| h |\n| --- |\n| a<br>b |", "{br}");
        }
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

    /// A one-cell table island, the block-only type.
    fn table() -> Island {
        Island::new("isl-0".into(), IslandType::Table).with_props(serde_json::json!({
            "aligns": ["none"],
            "header": [{"marks": [], "text": "h"}],
            "rows": [[{"marks": [], "text": "c"}]],
        }))
    }

    /// A table is block markup, so a slot a paragraph's prose holds gets the
    /// line its markup needs from the write, the mint having left the text as
    /// it was. The prose on each side keeps its marks in the coordinates the
    /// break leaves, and what re-imports is the broken content.
    #[test]
    fn a_block_island_inside_a_paragraph_is_written_on_a_line_of_its_own() {
        let rt = Content {
            text: format!("a{ISLAND_SLOT}bold"),
            lines: vec![Line::new(LineKind::Para)],
            marks: vec![Mark::new(2, 6, MarkKind::Strong)],
            islands: vec![table()],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        assert_eq!(rt.text, format!("a{ISLAND_SLOT}bold"), "the mint moved text");

        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.text, format!("a\n{ISLAND_SLOT}\nbold"), "{md:?}");
        assert_eq!(back.marks, vec![Mark::new(4, 8, MarkKind::Strong)]);
        assert_eq!(to_markdown(&back), md, "the broken content is a fixed point");

        let mut anchored = rt.clone().into_content();
        anchored.marks.push(anchor(2, 6, "c1"));
        let read = to_markdown_annotated(&anchored.into_normalized());
        assert_eq!(read.markdown.replace(&tag("c1"), ""), md);
        assert!(read.markdown.contains(&format!("{}**bold**", tag("c1"))), "{:?}", read.markdown);
        assert_eq!(read.anchors[0].line, "bold");
    }

    /// A block island renders one line, so a hard-break continuation after the
    /// line the write breaks out would be text it never reaches. The stored
    /// flag stays, since the slot shares its line there, and the write clears it
    /// on the copy it breaks.
    #[test]
    fn a_hard_break_after_a_broken_line_keeps_its_text() {
        let mut second = Line::new(LineKind::Para);
        second.continues = true;
        let rt = Content {
            text: format!("a{ISLAND_SLOT}\nmore"),
            lines: vec![Line::new(LineKind::Para), second],
            marks: vec![],
            islands: vec![table()],
        }
        .into_normalized();
        assert_eq!(rt.validate(), Ok(()), "hand-built content invalid");
        assert!(rt.lines[1].continues);

        let md = to_markdown(&rt);
        let back = from_markdown(&md).unwrap().content;
        assert_eq!(back.text, format!("a\n{ISLAND_SLOT}\nmore"), "{md:?}");
    }

    /// An image's markup does re-import as a slot, so the net still expects one
    /// back and a mark that would eat it is dropped. Here both `**` sit against
    /// the image's punctuation, where CommonMark flanking refuses them.
    #[test]
    fn a_mark_leaking_around_an_image_slot_is_still_dropped() {
        let image = || {
            Island::new("isl-0".into(), IslandType::Image)
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
        let back = from_markdown(&md).unwrap().content;
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
        assert_eq!(from_markdown(&md).unwrap().content, before_slot);
    }

    /// An image `alt` is the one place the character reference cannot carry an
    /// edge run: the parser decodes it, then trims alt.
    #[test]
    fn known_image_alt_limit() {
        let rt = with_islands(
            &ISLAND_SLOT.to_string(),
            vec![
                Island::new("isl-0".into(), IslandType::Image)
                    .with_props(serde_json::json!({"alt": " a ", "url": "u"})),
            ],
        );
        let back = from_markdown(&to_markdown(&rt)).unwrap().content;
        assert_eq!(
            back.islands[0].props["alt"], "a",
            "if this ever round-trips, promote it out of the known-limits list"
        );
    }

    /// A mark over a line join imports as one mark per line, the form export
    /// writes, so the first import is already the fixed point.
    #[test]
    fn a_mark_across_a_hard_break_is_a_fixed_point() {
        let link = MarkKind::Link { url: "https://e.com".into() };
        for (md, kind) in [
            ("**a\\\nx**", MarkKind::Strong),
            ("**a<br>x**", MarkKind::Strong),
            ("*a\\\nx*", MarkKind::Emph),
            ("~~a\\\nx~~", MarkKind::Strike),
            ("<u>a<br>x</u>", MarkKind::Underline),
            ("[a\\\nx](https://e.com)", link),
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(rt.text, "a\nx", "{md}");
            assert_eq!(
                rt.marks,
                vec![Mark::new(0, 1, kind.clone()), Mark::new(2, 3, kind)],
                "{md}"
            );
            assert_eq!(from_markdown(&to_markdown(&rt)).unwrap().content, rt, "{md}");
        }
    }

    #[test]
    fn anchor_marks_omitted_but_text_survives() {
        let mut rt = from_markdown("comment target here").unwrap().content.into_content();
        rt.marks.push(Mark {
            start: 8,
            end: 14,
            kind: MarkKind::Anchor { id: "c1".into() },
        });
        let rt = rt.into_normalized();
        let md = to_markdown(&rt);
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "comment target here");
        assert!(!md.contains("c1"));
    }

    fn anchor(start: usize, end: usize, id: &str) -> Mark {
        Mark::new(start, end, MarkKind::Anchor { id: id.into() })
    }

    fn tag(id: &str) -> String {
        format!("<qm-anchor ref=\"{id}\"></qm-anchor>")
    }

    /// The read imports as the plain projection does, anchors gone.
    fn reads_as_plain(read: &Annotated, rt: &Normalized) {
        let plain = from_markdown(&to_markdown(rt)).unwrap().content;
        assert!(plain.marks.iter().all(|m| !matches!(m.kind, MarkKind::Anchor { .. })));
        assert_eq!(from_markdown(&read.markdown).unwrap().content, plain, "{:?}", read.markdown);
    }

    fn read_ids(read: &Annotated) -> Vec<&str> {
        read.anchors.iter().map(|a| a.id.as_str()).collect()
    }

    /// A tag sits after the delimiters closing at its position and before those
    /// opening there, so it parts no delimiter run from its text. A range anchor
    /// reads as its start, and two anchors at one position spell in id order.
    #[test]
    fn a_tag_sits_between_the_marks_closing_and_opening_at_its_start() {
        let rt = marked(
            "abcd ef",
            vec![
                Mark::new(0, 2, MarkKind::Strong),
                Mark::new(2, 4, MarkKind::Strike),
                anchor(0, 0, "open"),
                anchor(2, 7, "range"),
                anchor(4, 4, "close"),
                anchor(4, 4, "also"),
            ],
        );
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            format!(
                "{}**ab**{}~~cd~~{}{} ef",
                tag("open"),
                tag("range"),
                tag("also"),
                tag("close")
            )
        );
        assert_eq!(read_ids(&read), ["open", "range", "also", "close"]);
        assert!(read.anchors.iter().all(|a| a.line == "abcd ef"));
        reads_as_plain(&read, &rt);
    }

    /// A code span or link has no position inside it a tag could take without
    /// becoming its text, so the tag moves to the span's start.
    #[test]
    fn a_tag_inside_a_code_span_or_link_moves_to_its_start() {
        let rt = marked(
            "run code at site",
            vec![
                Mark::new(4, 8, MarkKind::Code),
                Mark::new(12, 16, MarkKind::Link { url: "u".into() }),
                Mark::new(12, 16, MarkKind::Strong),
                anchor(6, 6, "c"),
                anchor(14, 15, "l"),
            ],
        );
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            format!("run {}`code` at {}**[site](u)**", tag("c"), tag("l"))
        );
        reads_as_plain(&read, &rt);
    }

    /// A code-block line, a block island's line and an empty line have no
    /// inline slot that keeps their structure, so they hold no tag; their
    /// anchors are still listed. An anchor in a table cell is outside the
    /// anchor op surface and is neither.
    #[test]
    fn a_line_with_no_inline_slot_holds_no_tag() {
        let mut rt = from_markdown("```\nfn a() {}\n```\n\n| h |\n| --- |\n| c |\n\n- a\n-\n- b")
            .unwrap()
            .content
            .into_content();
        assert_eq!(rt.text, format!("fn a() {{}}\n{ISLAND_SLOT}\na\n\nb"));
        rt.marks.extend([anchor(3, 3, "code"), anchor(10, 10, "table"), anchor(14, 14, "empty")]);
        rt.islands[0].props["rows"][0][0]["marks"] =
            serde_json::json!([{"attrs": {"id": "cell"}, "end": 1, "start": 0, "type": "anchor"}]);
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(rt.islands[0].props["rows"][0][0]["marks"][0]["type"], "anchor");

        let read = to_markdown_annotated(&rt);
        assert_eq!(read.markdown, to_markdown(&rt));
        let lines: Vec<(&str, &str)> =
            read.anchors.iter().map(|a| (a.id.as_str(), a.line.as_str())).collect();
        assert_eq!(lines, [("code", "fn a() {}"), ("table", ""), ("empty", "")]);
    }

    /// A heading's trailing `#` keeps the escape that holds it out of an ATX
    /// closing sequence wherever its tag lands, so the read is the plain
    /// projection with tags added.
    #[test]
    fn a_heading_keeps_its_trailing_hash_escape_under_a_tag() {
        let mut rt = from_markdown("# a \\#").unwrap().content.into_content();
        assert_eq!(rt.text, "a #");
        rt.marks.extend([anchor(2, 2, "before"), anchor(3, 3, "after")]);
        let rt = rt.into_normalized();
        let read = to_markdown_annotated(&rt);
        assert_eq!(read.markdown, format!("# a {}\\#{}", tag("before"), tag("after")));
        reads_as_plain(&read, &rt);
    }

    /// The tag is the carrier's canonical spelling, so an id holding markup
    /// characters is entity-encoded and the import still drops the tag whole.
    #[test]
    fn an_anchor_id_is_entity_encoded() {
        let rt = marked("ab", vec![anchor(1, 1, "a\"&<b>")]);
        let read = to_markdown_annotated(&rt);
        assert_eq!(
            read.markdown,
            "a<qm-anchor ref=\"a&quot;&amp;&lt;b&gt;\"></qm-anchor>b"
        );
        assert_eq!(read_ids(&read), ["a\"&<b>"]);
        reads_as_plain(&read, &rt);
    }

    /// Every tag on a line moves to its end where a tag in place would change
    /// what the line imports to, and none is written where the end changes it
    /// too. The lines and points here are hand-made, so each rung is met.
    #[test]
    fn a_line_its_tags_would_change_holds_them_at_its_end_or_not_at_all() {
        // `!` and `[x](u)` parted by a tag are text and a link, not an image.
        assert_eq!(
            annotate("![x](u) y".into(), &[0, 1], &[(1, "t")]),
            format!("![x](u) y{}", tag("t"))
        );
        // A trailing `\` escapes the `<` that would open the tag.
        assert_eq!(annotate("a\\".into(), &[0, 1, 2], &[(2, "t")]), "a\\");
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
        let rt2 = from_markdown(&md).unwrap().content;
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

    /// An underline's tags sit outside the delimiters opening and closing at
    /// their position, so neither changes how a delimiter flanks.
    #[test]
    fn an_underline_sharing_an_edge_with_a_delimiter_writes_outside_it() {
        for (text, under, strong, md) in [
            ("xab", (1, 3), (1, 2), "x<u>**a**b</u>"),
            ("abx", (0, 2), (1, 2), "<u>a**b**</u>x"),
        ] {
            let rt = marked(
                text,
                vec![
                    Mark { start: under.0, end: under.1, kind: MarkKind::Underline },
                    Mark { start: strong.0, end: strong.1, kind: MarkKind::Strong },
                ],
            );
            assert_eq!(to_markdown(&rt), md);
            assert_eq!(from_markdown(md).unwrap().content, rt);
        }
    }

    /// A font mark is a `qm-span` pair, its tags outside the underlines' and
    /// delimiters' at a position, so a second export spells the first.
    /// Pairs never nest: nested markdown comes back as one flat pair per run.
    #[test]
    fn a_font_mark_writes_flat_pairs() {
        for (md, want) in [
            (
                "The <qm-span family=\"Carlito\">phrase</qm-span> and **bold <qm-span family=\"Arimo\">sans</qm-span>**.",
                "The <qm-span family=\"Carlito\">phrase</qm-span> and **bold <qm-span family=\"Arimo\">sans**</qm-span>.",
            ),
            (
                "<qm-span family=\"A\">a<qm-span family=\"B\">b</qm-span>c</qm-span>",
                "<qm-span family=\"A\">a</qm-span><qm-span family=\"B\">b</qm-span><qm-span family=\"A\">c</qm-span>",
            ),
            (
                "x<qm-span family=\"A\"><u>**a**</u>b</qm-span>",
                "x<qm-span family=\"A\"><u>**a**</u>b</qm-span>",
            ),
            (
                "*a<qm-span family=\"A\">b*c</qm-span>",
                "*a<qm-span family=\"A\">b*c</qm-span>",
            ),
            (
                "<qm-span family=\"A\">a `b` c</qm-span>",
                "<qm-span family=\"A\">a `b` c</qm-span>",
            ),
            (
                "| <qm-span family=\"A&#x7C;B &quot;C&quot;\">a</qm-span> |\n| --- |",
                "| <qm-span family=\"A&#x7C;B &quot;C&quot;\">a</qm-span> |\n| --- |",
            ),
            (
                "a[^1]\n\n[^1]: <qm-span family=\"A\">n</qm-span>",
                "a[^1]\n\n[^1]: <qm-span family=\"A\">n</qm-span>",
            ),
        ] {
            let rt = from_markdown(md).unwrap().content;
            assert_eq!(to_markdown(&rt), want, "export of {md:?}");
            assert_eq!(from_markdown(want).unwrap().content, rt, "{want:?} did not return");
        }

        let family = |start, end, name: &str| Mark { start, end, kind: MarkKind::FontFamily { name: name.into() } };
        for (marks, md) in [
            (
                vec![family(0, 2, "A"), family(2, 6, "B")],
                "<qm-span family=\"A\">ab</qm-span><qm-span family=\"B\">cdef</qm-span>",
            ),
            (
                vec![family(1, 3, "B"), family(4, 6, "A")],
                "a<qm-span family=\"B\">bc</qm-span>d<qm-span family=\"A\">ef</qm-span>",
            ),
        ] {
            let rt = marked("abcdef", marks);
            assert_eq!(to_markdown(&rt), md);
            assert_eq!(from_markdown(md).unwrap().content, rt);
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
        let rt2 = from_markdown(&md).unwrap().content;
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
            let rt = from_markdown(md).unwrap().content;
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
        let rt = from_markdown("\\&amp;").unwrap().content;
        assert_eq!(rt.text, "&amp;");
        let md = to_markdown(&rt);
        assert!(md.contains("\\&"), "the `&` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "&amp;", "entity-shaped text must not decode");
        assert_eq!(rt, rt2);
    }

    /// Heading text ending in a `#` run must not re-import as an ATX closing
    /// sequence: `# a #` would come back as "a", dropping the `#`.
    #[test]
    fn heading_trailing_hash_round_trips() {
        let rt = from_markdown("# a \\#").unwrap().content;
        assert_eq!(rt.text, "a #");
        let md = to_markdown(&rt);
        assert!(md.contains("\\#"), "trailing `#` must be escaped, got {md:?}");
        let rt2 = from_markdown(&md).unwrap().content;
        assert_eq!(rt2.text, "a #", "trailing `#` must survive");
        assert_eq!(rt, rt2);
        round_trips("# heading \\#\\#");
        round_trips("## title\\#");
    }

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
        let back = from_markdown(&to_markdown(&rt)).expect("re-imports").content;
        assert_eq!(to_markdown(&back), to_markdown(&rt), "one round trip settles it");
        assert_eq!(back.text, "t x", "the display text did not leak");
        assert_eq!(
            back.marks.first().map(|m| &m.kind),
            Some(&MarkKind::Link {
                url: "a%0Ab".into()
            })
        );

        let isl = crate::model::Island::new("i1".into(), IslandType::Image)
            .with_props(serde_json::json!({"alt": "a", "url": "u\rv"}));
        let rt = Content::new(format!("x{ISLAND_SLOT}"), vec![Line::new(LineKind::Para)])
            .with_islands(vec![isl])
            .into_normalized();
        let back = from_markdown(&to_markdown(&rt)).expect("re-imports").content;
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
            assert_eq!(from_markdown(&md).unwrap().content.text, text, "{label}: text drift");
        }
    }

    /// A mark whose delimiters merge with their neighbour's is re-spelled, not
    /// dropped: only a mark markdown cannot carry *at all* reaches the drop
    /// search. The ambiguous run costs the text in the first three shapes — a
    /// `Strong` ending in a literal `*` against a following `Emph`, two marks
    /// over one span where `*` must sit inside `~~` to pair — and only the marks
    /// in the last two, where every character comes back and the `***` between
    /// bold and the emphasis beside it re-segments into one `Strong` over all
    /// three spans.
    #[test]
    fn a_mark_is_respelled_before_it_is_dropped() {
        for (label, src) in [
            ("strong ending in `*`, then emph", "__a**__*b*"),
            ("the same over non-ASCII", "__౸**__*0_*———"),
            ("emph and strike over one span", "౸~~*¡± ±*~~"),
            ("bold, emph, bold", "**a±**_b_**c**"),
            ("the same over a literal `*`", "__*__*౸*__a**0__"),
        ] {
            let once = from_markdown(src).unwrap().content;
            assert!(once.marks.len() >= 2, "{label}: nothing to lose");
            let md = to_markdown(&once);
            let twice = from_markdown(&md).unwrap().content;
            assert_eq!(&twice.text, &once.text, "{label}: text drift. md: {md:?}");
            assert_eq!(&twice.marks, &once.marks, "{label}: mark lost. md: {md:?}");
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
        assert_eq!(from_markdown(&md).unwrap().content.text, text);
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
        assert_eq!(from_markdown(&md).unwrap().content.text, text);
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
