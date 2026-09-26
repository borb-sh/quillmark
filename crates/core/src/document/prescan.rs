//! Pre-scan of a card-yaml block's YAML payload to recover what serde_saphyr
//! discards: comments and tags.
//!
//! Top-level comments become [`super::PayloadItem::Comment`]. Comments inside
//! block mappings/sequences are captured with their structural path and an
//! ordinal, which the emitter re-injects at (see [`NestedComment`]). The lines
//! themselves stay in the cleaned YAML, where they are comments to serde_saphyr
//! too.
//!
//! A tag on a block key's value is recorded at the key's path, for the
//! assembler to warn on; the YAML parser applies a core `!!` tag, ignores any
//! other, and keeps no tag. A tag anywhere else the parser drops unrecorded.

use crate::value::PathSegment;

/// One ordered hint extracted from the fence body. `Field` captures only the
/// key; the value comes from serde_saphyr. An inline `Comment` immediately
/// follows its host `Field` in the item stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreItem {
    Field { key: String },
    Comment { text: String, inline: bool },
}

/// A comment inside a nested mapping or sequence.
///
/// `container_path` locates the immediate parent. For own-line comments
/// (`inline = false`), `position` is the child slot ordinal (`0..=child_count`,
/// where `child_count` means "after all children"). For inline comments
/// (`inline = true`), `position` is the host child's index; orphaned inlines
/// degrade to own-line at emit time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedComment {
    pub container_path: Vec<PathSegment>,
    pub position: usize,
    pub text: String,
    pub inline: bool,
}

/// Output of [`prescan_fence_content`].
#[derive(Debug, Clone, Default)]
pub(crate) struct PreScan {
    /// YAML fed to serde_saphyr, trailing comments cut. Line-for-line with the
    /// fence content — comment lines pass through, being comments to
    /// the parser too — so a reported position needs no mapping to travel back
    /// to a document position.
    pub cleaned_yaml: String,
    /// Top-level fields and comments in source order.
    pub items: Vec<PreItem>,
    pub nested_comments: Vec<NestedComment>,
    /// Paths of the tagged nodes, relative to the fence root (the first
    /// segment is the owning top-level key).
    pub unsupported_tags: Vec<Vec<PathSegment>>,
}

#[derive(Debug)]
struct Frame {
    indent: usize,
    path: Vec<PathSegment>,
    child_count: usize,
}

/// The slot a key or dash line fills, where a comment trailing its value's
/// lines attaches.
#[derive(Debug, Clone)]
enum Host {
    Field,
    /// The line's own trailer slot, the own-line slot right after its value,
    /// and the count of nested comments recorded ahead of the line.
    Child {
        trailer: Slot,
        after: Slot,
        recorded: usize,
    },
}

#[derive(Debug, Clone)]
struct Slot {
    container_path: Vec<PathSegment>,
    position: usize,
}

pub(crate) fn prescan_fence_content(content: &str) -> PreScan {
    let mut out = PreScan::default();

    let lines: Vec<&str> = content.split('\n').collect();
    let mut cleaned: Vec<String> = Vec::with_capacity(lines.len());

    let mut stack: Vec<Frame> = vec![Frame {
        indent: 0,
        path: Vec::new(),
        child_count: 0,
    }];

    // Indent of the `key:` line that opened the current block scalar, if any.
    let mut block_scalar_indent: Option<usize> = None;
    // A value spanning lines, whose later lines are its text, never a key.
    let mut open: Option<FlowScan> = None;
    // The last `key:` or `-` left its node to a later line.
    let mut node_below = false;
    // The slot of the last `key:` or `-`, which owns `open`.
    let mut host = Host::Field;

    for raw_line in &lines {
        // The split is on `\n`, so a CRLF line ends in `\r`. Dropped once here:
        // every matcher below, and the cleaned YAML, see `\n`-only lines.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let indent = leading_space_count(line);
        let trimmed = &line[indent..];

        if trimmed.is_empty() {
            cleaned.push(line.to_string());
            continue;
        }

        // Inside a block scalar, deeper-indented lines are literal text: a
        // heading, a bullet, or a `key: value` line must pass through verbatim.
        // A line at or below the key's indent ends the scalar.
        if let Some(key_indent) = block_scalar_indent {
            if indent > key_indent {
                cleaned.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }

        // Inside a quoted scalar a `#` line is text too.
        if let Some(scan) = open.take_if(|scan| scan.quote.is_some()) {
            open = continue_value(&mut out, scan, trimmed, &host);
            cleaned.push(line.to_string());
            continue;
        }

        while let Some(frame) = stack.last() {
            if frame.indent > indent {
                stack.pop();
            } else {
                break;
            }
        }

        // Case 1: own-line comment.
        if trimmed.starts_with('#') {
            let text = strip_comment_marker(trimmed);
            let frame = stack.last().expect("root frame always present");

            if frame.path.is_empty() {
                // Top-level comment: preserve via PreItem::Comment.
                out.items.push(PreItem::Comment {
                    text: text.to_string(),
                    inline: false,
                });
            } else {
                out.nested_comments.push(NestedComment {
                    container_path: frame.path.clone(),
                    position: frame.child_count,
                    text: text.to_string(),
                    inline: false,
                });
            }
            // Emitted, not dropped: a comment is a comment to the parser too,
            // and keeping the line keeps the numbering the source's.
            cleaned.push(line.to_string());
            continue;
        }

        if let Some(scan) = open.take() {
            open = continue_value(&mut out, scan, trimmed, &host);
            cleaned.push(line.to_string());
            continue;
        }

        // Case 2: sequence item line (`- ...`).
        if trimmed == "-" || trimmed.starts_with("- ") {
            let frame_idx = ensure_frame_at_indent(&mut stack, indent);
            let frame = &mut stack[frame_idx];
            let item_index = frame.child_count;
            frame.child_count += 1;
            let parent_path: Vec<PathSegment> = frame.path.clone();
            let item_path: Vec<PathSegment> = {
                let mut p = parent_path.clone();
                p.push(PathSegment::Index(item_index));
                p
            };
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }
            let mut after = Slot {
                container_path: parent_path.clone(),
                position: item_index + 1,
            };

            // `strip_prefix` rather than a byte range: user content follows,
            // and a byte index could land inside a multi-byte codepoint.
            let after_dash_full = trimmed.strip_prefix("- ").unwrap_or("");
            let (after_dash, trailing_comment) = split_dash_trailing_comment(after_dash_full);
            let after_dash_trimmed = after_dash.trim_start();
            let inline_indent_offset = indent + 2 + (after_dash.len() - after_dash_trimmed.len());

            // The first key of a sequence-item mapping sits on the dash line, so
            // case 4 never sees it.
            let mut dash_key_block_scalar = false;
            if after_dash_trimmed.is_empty() {
                node_below = true;
                stack.push(Frame {
                    indent: indent + 2,
                    path: item_path,
                    child_count: 0,
                });
            } else if let Some((key, _, after_colon)) =
                split_nested_key(after_dash_trimmed)
            {
                let mut key_path = item_path.clone();
                key_path.push(PathSegment::Key(key));
                record_tag(&mut out, &after_colon, &key_path);
                dash_key_block_scalar = is_block_scalar_header(&after_colon);
                open = opens_past_line(&after_colon);
                node_below = node_text(&after_colon).is_empty();
                // Past the first key's value, ahead of the item's next key.
                after = Slot {
                    container_path: item_path.clone(),
                    position: 1,
                };
                stack.push(Frame {
                    indent: inline_indent_offset,
                    path: item_path,
                    child_count: 1,
                });
                if opens_nested_block(&after_colon) {
                    stack.push(Frame {
                        indent: inline_indent_offset + 2,
                        path: key_path,
                        child_count: 0,
                    });
                }
            } else {
                open = opens_past_line(after_dash_trimmed);
                node_below = node_text(after_dash_trimmed).is_empty();
            }
            host = Host::Child {
                trailer: Slot {
                    container_path: parent_path.clone(),
                    position: item_index,
                },
                after,
                recorded: out.nested_comments.len(),
            };

            if let Some(c) = &trailing_comment {
                out.nested_comments.push(NestedComment {
                    container_path: parent_path,
                    position: item_index,
                    text: strip_comment_marker(c).to_string(),
                    inline: true,
                });
            }
            if trailing_comment.is_some() {
                let head = format!("{:width$}", "", width = indent);
                let body = if after_dash.trim_end().is_empty() {
                    "-".to_string()
                } else {
                    format!("- {}", after_dash.trim_end())
                };
                cleaned.push(format!("{}{}", head, body));
            } else {
                cleaned.push(line.to_string());
            }

            // For a `- |-` item the content is indented past the dash, so the
            // dash line's indent is the block-scalar boundary; for `- key: |` it
            // is the key's column, where the item's next key sits.
            if is_block_scalar_header(after_dash_trimmed) {
                block_scalar_indent = Some(indent);
            } else if dash_key_block_scalar {
                block_scalar_indent = Some(inline_indent_offset);
            }
            continue;
        }

        // Case 3: top-level field line.
        let is_top_level = indent == 0;
        if is_top_level {
            if let Some((key, after_colon)) = split_key(line) {
                let (value_part, trailing_comment) = split_trailing_comment(&after_colon);

                let key_path = vec![PathSegment::Key(key.clone())];
                record_tag(&mut out, &value_part, &key_path);

                out.items.push(PreItem::Field { key: key.clone() });
                host = Host::Field;

                let root = &mut stack[0];
                root.child_count += 1;

                while stack.len() > 1 {
                    stack.pop();
                }

                if opens_nested_block(&value_part) {
                    stack.push(Frame {
                        indent: 2,
                        path: key_path,
                        child_count: 0,
                    });
                }

                cleaned.push(format!("{}:{}", key, value_part));
                open = opens_past_line(&value_part);
                node_below = node_text(&value_part).is_empty();

                if let Some(c) = trailing_comment {
                    out.items.push(PreItem::Comment {
                        text: strip_comment_marker(&c).to_string(),
                        inline: true,
                    });
                }

                if is_block_scalar_header(&value_part) {
                    block_scalar_indent = Some(indent);
                }

                continue;
            }
        }

        // Case 4: nested key line inside a block mapping.
        if let Some((key, source_key, after_colon)) = split_nested_key(trimmed) {
            let frame_idx = ensure_frame_at_indent(&mut stack, indent);
            let frame = &mut stack[frame_idx];
            let key_index = frame.child_count;
            frame.child_count += 1;
            let parent_path: Vec<PathSegment> = frame.path.clone();
            let key_path: Vec<PathSegment> = {
                let mut p = parent_path.clone();
                p.push(PathSegment::Key(key.clone()));
                p
            };
            while stack.len() > frame_idx + 1 {
                stack.pop();
            }
            host = Host::Child {
                trailer: Slot {
                    container_path: parent_path.clone(),
                    position: key_index,
                },
                after: Slot {
                    container_path: parent_path.clone(),
                    position: key_index + 1,
                },
                recorded: out.nested_comments.len(),
            };

            let (value_part, trailing_comment) = split_trailing_comment(&after_colon);

            record_tag(&mut out, &value_part, &key_path);

            if let Some(c) = trailing_comment {
                out.nested_comments.push(NestedComment {
                    container_path: parent_path,
                    position: key_index,
                    text: strip_comment_marker(&c).to_string(),
                    inline: true,
                });
                let head = format!("{:width$}", "", width = indent);
                cleaned.push(format!("{}{}:{}", head, source_key, value_part));
            } else {
                cleaned.push(line.to_string());
            }

            if opens_nested_block(&value_part) {
                stack.push(Frame {
                    indent: indent + 2,
                    path: key_path,
                    child_count: 0,
                });
            }

            if is_block_scalar_header(&value_part) {
                block_scalar_indent = Some(indent);
            }
            open = opens_past_line(&value_part);
            node_below = node_text(&value_part).is_empty();
            continue;
        }

        if std::mem::take(&mut node_below) {
            open = opens_past_line(trimmed);
            // A scalar holds no children, so the frame opened for it goes.
            if stack.len() > 1 && stack.last().is_some_and(|f| f.child_count == 0) {
                let frame = stack.pop().expect("more than the root frame");
                rehome_comments(&mut out, &frame.path, &host);
            }
        }
        cleaned.push(line.to_string());
    }

    out.cleaned_yaml = cleaned.join("\n");
    out
}

/// The deepest frame at `indent`, pushing a new one if the current top is
/// shallower.
fn ensure_frame_at_indent(stack: &mut Vec<Frame>, indent: usize) -> usize {
    let top_idx = stack.len() - 1;
    let top = &stack[top_idx];

    if top.indent == indent {
        return top_idx;
    }

    let parent_path = top.path.clone();
    stack.push(Frame {
        indent,
        path: parent_path,
        child_count: 0,
    });
    stack.len() - 1
}

/// `scan` past one more line of its value, while the value stays open. The
/// line's trailing comment attaches to `host`: as its inline trailer, or, when
/// a comment already sits on the host's line or inside the value, as an
/// own-line comment right after the value.
fn continue_value(
    out: &mut PreScan,
    mut scan: FlowScan,
    text: &str,
    host: &Host,
) -> Option<FlowScan> {
    if let Some(i) = scan.line(text) {
        let text = strip_comment_marker(&text[i..]).to_string();
        match host {
            Host::Field => {
                let inline = matches!(out.items.last(), Some(PreItem::Field { .. }));
                out.items.push(PreItem::Comment { text, inline });
            }
            Host::Child {
                trailer,
                after,
                recorded,
            } => {
                let trailed = out.nested_comments.len() > *recorded;
                let slot = if trailed { after } else { trailer };
                out.nested_comments.push(NestedComment {
                    container_path: slot.container_path.clone(),
                    position: slot.position,
                    text,
                    inline: !trailed,
                });
            }
        }
    }
    scan.is_open().then_some(scan)
}

/// Move the comments recorded inside `path`, a frame opened for a value that
/// turned out a scalar, to the own-line slot after `host`.
fn rehome_comments(out: &mut PreScan, path: &[PathSegment], host: &Host) {
    let (moved, kept) = std::mem::take(&mut out.nested_comments)
        .into_iter()
        .partition(|c| c.container_path == path);
    out.nested_comments = kept;
    for c in moved {
        match host {
            Host::Field => out.items.push(PreItem::Comment {
                text: c.text,
                inline: false,
            }),
            Host::Child { after, .. } => out.nested_comments.push(NestedComment {
                container_path: after.container_path.clone(),
                position: after.position,
                ..c
            }),
        }
    }
}

fn strip_comment_marker(raw: &str) -> &str {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after)
}

fn leading_space_count(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// `true` when a field value is a YAML block-scalar header (`|` or `>`, with
/// optional chomping/indent indicators), past any tag or anchor. Unquoted
/// plain scalars cannot begin with these characters, so a leading `|`/`>`
/// unambiguously opens a literal/folded block whose following content lines
/// are text, not YAML structure.
fn is_block_scalar_header(value: &str) -> bool {
    node_text(value).starts_with(['|', '>'])
}

/// `true` when the indented lines under a `key:` line belong to its value: the
/// value is on those lines, or it is an empty flow collection (`[]`, `{}`)
/// whose own comments sit under it.
fn opens_nested_block(after_colon: &str) -> bool {
    let (v, _) = split_trailing_comment(after_colon);
    matches!(node_text(&v).trim_end(), "" | "[]" | "{}")
}

/// Byte index of the `:` closing `line`'s leading key, or `None` when `line`
/// does not open with one. A key is `[a-zA-Z_][a-zA-Z0-9_]*`, optionally
/// `$`-prefixed for system keys.
pub(super) fn key_end(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut i;
    if bytes[0] == b'$' {
        if bytes.len() < 2 || !(bytes[1].is_ascii_alphabetic() || bytes[1] == b'_') {
            return None;
        }
        i = 2;
    } else if bytes[0].is_ascii_alphabetic() || bytes[0] == b'_' {
        i = 1;
    } else {
        return None;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (i < bytes.len() && bytes[i] == b':').then_some(i)
}

/// Split a line into `(key, rest_after_colon)`, or `None` for non-key lines.
fn split_key(line: &str) -> Option<(String, String)> {
    let i = key_end(line)?;
    Some((line[..i].to_string(), line[i + 1..].to_string()))
}

/// Byte index of the `:` closing a *nested* key.
///
/// Nested keys are arbitrary user data, so this reads YAML's implicit-key
/// grammar rather than [`key_end`]'s field names: a quoted scalar, or a plain
/// scalar ending at the first `:` followed by whitespace or the line's end.
/// `og:title: x` is the key `og:title`.
fn nested_key_end(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let first = *bytes.first()?;
    if first == b'"' || first == b'\'' {
        let quote = first;
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' && quote == b'"' {
                i += 2;
                continue;
            }
            if bytes[i] == quote {
                // `''` inside a single-quoted scalar is one escaped quote.
                if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                return (bytes.get(i + 1) == Some(&b':')).then_some(i + 1);
            }
            i += 1;
        }
        return None;
    }
    let opens_plain = !PLAIN_SCALAR_EXCLUDED_FIRST.contains(&first)
        || (matches!(first, b'-' | b'?' | b':')
            && bytes.get(1).is_some_and(|b| !matches!(b, b' ' | b'\t')));
    if !opens_plain {
        return None;
    }
    for i in 1..bytes.len() {
        if bytes[i] == b'#' && matches!(bytes[i - 1], b' ' | b'\t') {
            return None;
        }
        if bytes[i] == b':' && matches!(bytes.get(i + 1), None | Some(b' ' | b'\t')) {
            return Some(i);
        }
    }
    None
}

/// The YAML indicators a plain scalar cannot open with, except that `-`, `?`
/// and `:` open one when a non-space follows (`-x`).
const PLAIN_SCALAR_EXCLUDED_FIRST: &[u8] = b"-?:,[]{}#&*!|>'\"%@`";

/// Split a nested key line into `(key, source spelling, rest_after_colon)`.
///
/// The two forms differ wherever the source spells the key with anything the
/// parser drops — quotes, whitespace before the `:`: paths carry the key the
/// YAML parser sees, the cleaned line keeps what was written. A quoted key that
/// does not decode is not a key.
fn split_nested_key(line: &str) -> Option<(String, String, String)> {
    let i = nested_key_end(line)?;
    let source = &line[..i];
    let key = match source.as_bytes().first() {
        Some(b'"') | Some(b'\'') => serde_saphyr::from_str::<String>(source).ok()?,
        _ => source.trim_end().to_string(),
    };
    Some((key, source.to_string(), line[i + 1..].to_string()))
}

/// Split `value` into `(value_without_comment, trailing_comment)` following
/// YAML's rules. A `#` preceded by whitespace (or at value start) begins a
/// comment, except inside a quoted scalar, and a quote opens a quoted
/// scalar only as a node's first character, past any tag or anchor: the
/// value's, or a node's inside a flow collection (`[`/`{`). Inside a plain
/// scalar, `'` and `"` are ordinary characters: `x: it's fine # note` carries
/// a comment.
fn split_trailing_comment(value: &str) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let first = value.len() - node_text(value).len();
    match bytes.get(first) {
        // Quoted scalar: skip the quoted body, then scan for a comment. An
        // unterminated quote means the scalar continues on the next line:
        // no comment on this one.
        Some(b'"' | b'\'') => match find_quote_end(bytes, first) {
            Some(end) => find_comment_from(value, end + 1),
            None => (value.to_string(), None),
        },
        // Flow collection: a quoted scalar opens at any node inside, so
        // track quote state across the whole value.
        Some(b'[' | b'{') => split_flow_trailing_comment(value),
        // Plain scalar (or block-scalar header): quotes are ordinary
        // characters; only the whitespace-then-`#` rule applies.
        _ => find_comment_from(value, 0),
    }
}

/// [`split_trailing_comment`] for a sequence item's text after its `- `. When the
/// item opens a mapping with its first `key:`, the value after the colon is
/// what may open a quoted scalar.
fn split_dash_trailing_comment(after_dash: &str) -> (String, Option<String>) {
    let trimmed = after_dash.trim_start();
    if !trimmed.starts_with('#') {
        if let Some((_, _, after_colon)) = split_nested_key(trimmed) {
            let head = &after_dash[..after_dash.len() - after_colon.len()];
            let (value, comment) = split_trailing_comment(&after_colon);
            return (format!("{head}{value}"), comment);
        }
    }
    split_trailing_comment(after_dash)
}

/// Byte index of the closing quote of the quoted scalar opening at `start`,
/// honouring `\"` escapes in double quotes and `''` escapes in single quotes.
fn find_quote_end(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if quote == b'"' && b == b'\\' {
            i += 2;
            continue;
        }
        if b == quote {
            if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                i += 2; // '' is an escaped quote, not the closer
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Scan `value` from byte `from` for a `#` preceded by whitespace (or at the
/// scan start) and split there. Quote characters are not interpreted.
fn find_comment_from(value: &str, from: usize) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let mut prev_was_ws = true;
    for i in from..bytes.len() {
        let b = bytes[i];
        if b == b'#' && prev_was_ws {
            let v = value[..i].trim_end().to_string();
            let c = value[i..].to_string();
            return (v, Some(c));
        }
        prev_was_ws = matches!(b, b' ' | b'\t');
    }
    (value.to_string(), None)
}

/// Comment split for flow-collection values (`[…]` / `{…}`): split at the
/// first whitespace-preceded `#` outside a quoted scalar.
fn split_flow_trailing_comment(value: &str) -> (String, Option<String>) {
    match FlowScan::default().line(value) {
        Some(i) => (value[..i].trim_end().to_string(), Some(value[i..].to_string())),
        None => (value.to_string(), None),
    }
}

/// `value` past its leading whitespace and any tag or anchor ahead of the node.
fn node_text(value: &str) -> &str {
    let mut node = value.trim_start();
    while node.starts_with(['!', '&']) {
        let end = node.find([' ', '\t']).unwrap_or(node.len());
        node = node[end..].trim_start();
    }
    node
}

/// The scan a node leaves open past its line: a flow collection whose
/// brackets, or a quoted scalar whose quote, the line does not close.
fn opens_past_line(value: &str) -> Option<FlowScan> {
    let node = node_text(value);
    if !node.starts_with(['[', '{', '"', '\'']) {
        return None;
    }
    FlowScan::default().continued(node)
}

/// Where a [`FlowScan`] stands. A quote opens a quoted scalar only where a
/// node may start; anywhere else it is a plain scalar's own character.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FlowAt {
    /// At the value's start, or after `[`, `{`, `,`, a `:` / `?` indicator, or
    /// a tag or anchor.
    #[default]
    NodeStart,
    Plain,
    /// Past a quoted scalar or a closing bracket, where an adjacent `:` is an
    /// indicator (`{"a":1}`).
    NodeEnd,
}

/// The scan of a value that can span lines: a flow collection with the quoted
/// scalars inside it, or a quoted scalar alone. Its state carries from line to
/// line.
#[derive(Debug, Default)]
struct FlowScan {
    depth: usize,
    quote: Option<u8>,
    at: FlowAt,
}

impl FlowScan {
    fn is_open(&self) -> bool {
        self.depth > 0 || self.quote.is_some()
    }

    /// The scan past one more line, while the value stays open.
    fn continued(mut self, text: &str) -> Option<Self> {
        self.line(text);
        self.is_open().then_some(self)
    }

    /// Advance over one line of the value, returning the byte index of the `#`
    /// opening its trailing comment.
    fn line(&mut self, text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut after_ws = true;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(quote) = self.quote {
                if quote == b'"' && b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == quote {
                    // `''` inside a single-quoted scalar is one escaped quote.
                    if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    self.quote = None;
                    self.at = FlowAt::NodeEnd;
                    after_ws = false;
                }
                i += 1;
                continue;
            }
            match b {
                b' ' | b'\t' => {
                    after_ws = true;
                    i += 1;
                    continue;
                }
                b'#' if after_ws => return Some(i),
                b'!' | b'&' if self.at == FlowAt::NodeStart => {
                    while i < bytes.len() && !FLOW_PROPERTY_END.contains(&bytes[i]) {
                        i += 1;
                    }
                    after_ws = false;
                    continue;
                }
                b'"' | b'\'' if self.at == FlowAt::NodeStart => self.quote = Some(b),
                b'[' | b'{' => {
                    self.depth += 1;
                    self.at = FlowAt::NodeStart;
                }
                b']' | b'}' => {
                    self.depth = self.depth.saturating_sub(1);
                    self.at = FlowAt::NodeEnd;
                }
                b',' => self.at = FlowAt::NodeStart,
                b':' if self.at == FlowAt::NodeEnd || space_follows(bytes, i) => {
                    self.at = FlowAt::NodeStart
                }
                // `? ` is an indicator only where a node starts: `Why? 'cause` is text.
                b'?' if self.at == FlowAt::NodeStart && space_follows(bytes, i) => {}
                _ => self.at = FlowAt::Plain,
            }
            after_ws = false;
            i += 1;
        }
        None
    }
}

/// The bytes ending a tag or an anchor inside a flow collection.
const FLOW_PROPERTY_END: &[u8] = b" \t,[]{}";

/// `true` when whitespace or the line's end follows the byte at `i`.
fn space_follows(bytes: &[u8], i: usize) -> bool {
    matches!(bytes.get(i + 1), None | Some(b' ' | b'\t'))
}

/// Record `path` onto `out` when `value`, the text after its key's `:`, opens
/// with a tag.
fn record_tag(out: &mut PreScan, value: &str, path: &[PathSegment]) {
    if value.trim_start().starts_with('!') {
        out.unsupported_tags.push(path.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_own_line_comments() {
        let input = "# top\ntitle: foo\n# mid\nauthor: bar\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Comment {
                    text: "top".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "title".to_string(),
                },
                PreItem::Comment {
                    text: "mid".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "author".to_string(),
                },
            ]
        );
        assert!(out.nested_comments.is_empty());
    }

    #[test]
    fn splits_trailing_comments() {
        let input = "title: foo # inline\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Field {
                    key: "title".to_string(),
                },
                PreItem::Comment {
                    text: "inline".to_string(),
                    inline: true,
                },
            ]
        );
        assert!(out.cleaned_yaml.contains("title: foo"));
        assert!(!out.cleaned_yaml.contains("inline"));
    }

    #[test]
    fn a_tag_is_recorded_and_left_for_the_parser() {
        for input in ["dept: !custom Department\n", "dept: !custom\n"] {
            let out = prescan_fence_content(input);
            assert_eq!(
                out.items,
                vec![PreItem::Field {
                    key: "dept".to_string(),
                }]
            );
            assert_eq!(out.unsupported_tags, vec![vec![PathSegment::Key("dept".to_string())]]);
            assert_eq!(out.cleaned_yaml, input);
        }
    }

    #[test]
    fn crlf_lines_carry_no_carriage_return_into_the_scan() {
        let input = "dept: !t\r\n# note\r\ntitle: x # trailing\r\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Field {
                    key: "dept".to_string(),
                },
                PreItem::Comment {
                    text: "note".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "title".to_string(),
                },
                PreItem::Comment {
                    text: "trailing".to_string(),
                    inline: true,
                },
            ]
        );
        assert_eq!(out.unsupported_tags, vec![vec![PathSegment::Key("dept".to_string())]]);
        assert!(!out.cleaned_yaml.contains('\r'));
    }

    #[test]
    fn nested_comment_in_sequence_captured() {
        let input = "arr:\n  # before-first\n  - a\n  # between\n  - b\n  # after-last\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![
                NestedComment {
                    container_path: vec![PathSegment::Key("arr".to_string())],
                    position: 0,
                    text: "before-first".to_string(),
                    inline: false,
                },
                NestedComment {
                    container_path: vec![PathSegment::Key("arr".to_string())],
                    position: 1,
                    text: "between".to_string(),
                    inline: false,
                },
                NestedComment {
                    container_path: vec![PathSegment::Key("arr".to_string())],
                    position: 2,
                    text: "after-last".to_string(),
                    inline: false,
                },
            ]
        );
    }

    #[test]
    fn nested_comment_in_mapping_captured() {
        let input = "outer:\n  # comment\n  inner: 1\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![PathSegment::Key("outer".to_string())],
                position: 0,
                text: "comment".to_string(),
                inline: false,
            }]
        );
    }

    /// An empty flow collection's comments sit under it, whether its key opens
    /// its own line or a sequence item's.
    #[test]
    fn a_comment_under_an_empty_flow_collection_is_inside_it() {
        let input = "rows: []\n  # - a\nrow:\n  - key: {}\n      # b\n    next: 1\n";
        let out = prescan_fence_content(input);
        let key = |k: &str| PathSegment::Key(k.to_string());
        assert_eq!(
            out.nested_comments,
            vec![
                NestedComment {
                    container_path: vec![key("rows")],
                    position: 0,
                    text: "- a".to_string(),
                    inline: false,
                },
                NestedComment {
                    container_path: vec![key("row"), PathSegment::Index(0), key("key")],
                    position: 0,
                    text: "b".to_string(),
                    inline: false,
                },
            ]
        );
    }

    #[test]
    fn deep_nested_comment_path() {
        let input = "outer:\n  inner:\n    # deep\n    leaf: 1\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![
                    PathSegment::Key("outer".to_string()),
                    PathSegment::Key("inner".to_string()),
                ],
                position: 0,
                text: "deep".to_string(),
                inline: false,
            }]
        );
    }

    #[test]
    fn comment_inside_seq_of_maps() {
        let input = "items:\n  - name: a\n    # inside-first\n    val: 1\n  - name: b\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![
                    PathSegment::Key("items".to_string()),
                    PathSegment::Index(0),
                ],
                position: 1,
                text: "inside-first".to_string(),
                inline: false,
            }]
        );
    }

    #[test]
    fn nested_inline_on_sequence_item() {
        let input = "arr:\n  - a # tail\n  - b\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![PathSegment::Key("arr".to_string())],
                position: 0,
                text: "tail".to_string(),
                inline: true,
            }]
        );
        assert!(out.cleaned_yaml.contains("- a\n"));
        assert!(!out.cleaned_yaml.contains("tail"));
    }

    #[test]
    fn nested_inline_on_mapping_field() {
        let input = "outer:\n  inner: 1 # tail\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.nested_comments,
            vec![NestedComment {
                container_path: vec![PathSegment::Key("outer".to_string())],
                position: 0,
                text: "tail".to_string(),
                inline: true,
            }]
        );
    }

    #[test]
    fn a_tag_on_a_nested_key_and_a_dash_line_records_its_path() {
        let input = "addr:\n  street: !custom Main\nto:\n  - name: !custom\n";
        let out = prescan_fence_content(input);
        let key = |k: &str| PathSegment::Key(k.to_string());
        assert_eq!(
            out.unsupported_tags,
            vec![
                vec![key("addr"), key("street")],
                vec![key("to"), PathSegment::Index(0), key("name")],
            ]
        );
        assert_eq!(out.cleaned_yaml, input);
    }

    #[test]
    fn sequence_with_multibyte_after_dash_does_not_panic() {
        // Multi-byte characters immediately after `- `. A byte-range slice here
        // panics with "byte index 2 is not a char boundary".
        let inputs = [
            "arr:\n  - – en-dash\n  - — em-dash\n",
            "arr:\n  - \u{2013}line\n  - \u{2014}line\n",
            "arr:\n  - \u{201C}smart-quoted\u{201D}\n",
            "arr:\n  - \u{1F600} emoji\n",
            "bullets: |\n  - (U) **A:** text\n  – (U) **B:** text\n",
        ];
        for input in inputs {
            let out = prescan_fence_content(input);
            assert_eq!(out.cleaned_yaml.lines().count(), input.lines().count());
        }
    }

    #[test]
    fn cleaned_yaml_is_line_for_line_with_its_source() {
        // A parse position is a source position only while this holds; comment
        // lines and block-scalar content that looks like structure both have to
        // leave the numbering alone.
        let input = "# lead\ntitle: Doc\n\n# note\nrole: x\nbio: |\n  # not a comment\nend: x\n";
        let out = prescan_fence_content(input);

        let cleaned: Vec<&str> = out.cleaned_yaml.split('\n').collect();
        assert_eq!(cleaned.len(), input.split('\n').count());
        let line_of = |needle: &str| {
            cleaned
                .iter()
                .position(|l| l.contains(needle))
                .expect("cleaned line present")
        };
        assert_eq!(line_of("title:"), 1);
        assert_eq!(line_of("role:"), 4);
        assert_eq!(line_of("# not a comment"), 6);
        assert_eq!(line_of("end:"), 7);
    }

    #[test]
    fn block_scalar_content_is_not_parsed_as_structure() {
        let input =
            "bio: |-\n  ## About me\n\n  - point one\n  role: engineer\n  Done.\nname: jane\n";
        let out = prescan_fence_content(input);

        assert!(
            out.cleaned_yaml.contains("## About me"),
            "block-scalar heading must survive: {:?}",
            out.cleaned_yaml
        );
        assert!(out.cleaned_yaml.contains("- point one"));
        assert!(out.cleaned_yaml.contains("role: engineer"));

        assert!(
            !out.items.iter().any(|i| matches!(
                i,
                PreItem::Comment { text, .. } if text.contains("About")
            )),
            "block-scalar `#` line must not become a comment"
        );
        assert!(
            !out.items
                .iter()
                .any(|i| matches!(i, PreItem::Field { key, .. } if key == "role")),
            "block-scalar `key:` line must not become a field"
        );

        let fields: Vec<&str> = out
            .items
            .iter()
            .filter_map(|i| match i {
                PreItem::Field { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(fields, vec!["bio", "name"]);
    }

    #[test]
    fn sequence_item_block_scalar_content_is_not_parsed_as_structure() {
        let input = "items:\n  - |-\n    ## Heading\n    - inner bullet\n    role: x\n  - second\n";
        let out = prescan_fence_content(input);

        assert!(
            out.cleaned_yaml.contains("## Heading"),
            "block-scalar heading inside a sequence item must survive: {:?}",
            out.cleaned_yaml
        );
        assert!(out.cleaned_yaml.contains("- inner bullet"));
        assert!(out.cleaned_yaml.contains("role: x"));
        assert!(
            !out.nested_comments
                .iter()
                .any(|c| c.text.contains("Heading")),
            "block-scalar `#` line must not become a nested comment"
        );
        assert!(out.cleaned_yaml.contains("- second"));
    }

    #[test]
    fn comment_after_plain_scalar_with_apostrophe() {
        // YAML: in a plain scalar, `'` is an ordinary character; the
        // whitespace-preceded `#` still starts a comment.
        let (v, c) = split_trailing_comment(" it's a test # note");
        assert_eq!(v, " it's a test");
        assert_eq!(c.as_deref(), Some("# note"));
    }

    #[test]
    fn hash_inside_quoted_scalar_is_not_a_comment() {
        let (v, c) = split_trailing_comment(" 'a # b'");
        assert_eq!(v, " 'a # b'");
        assert_eq!(c, None);

        let (v, c) = split_trailing_comment(" \"a # b\"");
        assert_eq!(v, " \"a # b\"");
        assert_eq!(c, None);
    }

    #[test]
    fn comment_after_quoted_scalar() {
        let (v, c) = split_trailing_comment(" 'a # b' # real");
        assert_eq!(v, " 'a # b'");
        assert_eq!(c.as_deref(), Some("# real"));

        // '' is an escaped quote, not the closer.
        let (v, c) = split_trailing_comment(" 'it''s # x' # real");
        assert_eq!(v, " 'it''s # x'");
        assert_eq!(c.as_deref(), Some("# real"));

        // \" is an escaped quote in double-quoted scalars.
        let (v, c) = split_trailing_comment(" \"a \\\" # b\" # real");
        assert_eq!(v, " \"a \\\" # b\"");
        assert_eq!(c.as_deref(), Some("# real"));
    }

    #[test]
    fn unterminated_quote_means_multiline_scalar_no_comment() {
        let (v, c) = split_trailing_comment(" \"starts here # not a comment");
        assert_eq!(v, " \"starts here # not a comment");
        assert_eq!(c, None);
    }

    #[test]
    fn flow_collection_tracks_quotes_anywhere() {
        let (v, c) = split_trailing_comment(" [a, \"b # c\"] # real");
        assert_eq!(v, " [a, \"b # c\"]");
        assert_eq!(c.as_deref(), Some("# real"));

        let (v, c) = split_trailing_comment(" [a, \"b # c\"]");
        assert_eq!(c, None);
        assert_eq!(v, " [a, \"b # c\"]");
    }

    #[test]
    fn hash_without_preceding_whitespace_is_not_a_comment() {
        let (v, c) = split_trailing_comment(" a#b");
        assert_eq!(v, " a#b");
        assert_eq!(c, None);
    }
}
