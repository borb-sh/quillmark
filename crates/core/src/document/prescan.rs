//! Pre-scan of a card-yaml block's YAML payload to recover what serde_saphyr
//! discards: comments and `!must_fill` tags.
//!
//! Top-level comments become [`super::PayloadItem::Comment`]. Comments inside
//! block mappings/sequences are captured with their structural path and an
//! ordinal, which the emitter re-injects at (see [`NestedComment`]). A
//! `!must_fill` tag is stripped from the cleaned YAML, so serde_saphyr sees a
//! plain scalar, and recorded as a `fill` marker.
//!
//! `!must_fill` is the only recognized fill tag; every other custom tag is
//! dropped with a `parse::unsupported_yaml_tag` warning, value kept.

use crate::value::PathSegment;
use crate::Diagnostic;
use crate::Severity;

/// One ordered hint extracted from the fence body. `Field` captures only the
/// `fill` flag; the value comes from serde_saphyr. An inline `Comment`
/// immediately follows its host `Field` in the item stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreItem {
    Field { key: String, fill: bool },
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
    /// YAML with `!must_fill` tags stripped and comment lines removed; fed to serde_saphyr.
    pub cleaned_yaml: String,
    /// The 0-indexed line of the fence content each line of
    /// [`Self::cleaned_yaml`] came from. Own-line comments are dropped rather
    /// than blanked, so the two numberings diverge and a parse failure needs
    /// this to travel back to a document position.
    pub source_lines: Vec<usize>,
    /// Top-level fields and comments in source order.
    pub items: Vec<PreItem>,
    pub nested_comments: Vec<NestedComment>,
    /// Paths of nested fields tagged `!must_fill`, relative to the fence root
    /// (the first segment is the owning top-level key). Applied onto the
    /// value tree by the assembler. Top-level fills ride on `PreItem::Field`.
    pub nested_fills: Vec<Vec<PathSegment>>,
    pub warnings: Vec<Diagnostic>,
}

/// The emitted YAML lines paired with the source line each came from.
#[derive(Debug)]
struct Cleaned {
    lines: Vec<String>,
    source_lines: Vec<usize>,
}

impl Cleaned {
    fn push(&mut self, source_line: usize, text: String) {
        self.lines.push(text);
        self.source_lines.push(source_line);
    }
}

#[derive(Debug)]
struct Frame {
    indent: usize,
    path: Vec<PathSegment>,
    child_count: usize,
}

pub(crate) fn prescan_fence_content(content: &str) -> PreScan {
    let mut out = PreScan::default();

    let lines: Vec<&str> = content.split('\n').collect();
    let mut cleaned = Cleaned {
        lines: Vec::with_capacity(lines.len()),
        source_lines: Vec::with_capacity(lines.len()),
    };

    let mut stack: Vec<Frame> = vec![Frame {
        indent: 0,
        path: Vec::new(),
        child_count: 0,
    }];

    // Indent of the `key:` line that opened the current block scalar, if any.
    let mut block_scalar_indent: Option<usize> = None;

    let mut unsupported_fill_tag = false;

    for (source_line, raw_line) in lines.iter().enumerate() {
        // The split is on `\n`, so a CRLF line ends in `\r`. Dropped once here:
        // every matcher below, and the cleaned YAML, see `\n`-only lines.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let indent = leading_space_count(line);
        let trimmed = &line[indent..];

        if trimmed.is_empty() {
            cleaned.push(source_line, line.to_string());
            continue;
        }

        // Inside a block scalar, deeper-indented lines are literal text: a
        // heading, a bullet, or a `key: value` line must pass through verbatim.
        // A line at or below the key's indent ends the scalar.
        if let Some(key_indent) = block_scalar_indent {
            if indent > key_indent {
                cleaned.push(source_line, line.to_string());
                continue;
            }
            block_scalar_indent = None;
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

            // `strip_prefix` rather than a byte range: user content follows,
            // and a byte index could land inside a multi-byte codepoint.
            let after_dash_full = trimmed.strip_prefix("- ").unwrap_or("");
            let (after_dash, trailing_comment) = split_trailing_comment(after_dash_full);
            let after_dash_trimmed = after_dash.trim_start();
            let inline_indent_offset = indent + 2 + (after_dash.len() - after_dash_trimmed.len());

            // The first key of a sequence-item mapping sits on the dash line, so
            // case 4 never sees it. `dash_body_clean`, when set, is the
            // tag-stripped `key:value` rewritten onto the dash line.
            let mut dash_body_clean: Option<String> = None;
            if after_dash_trimmed.is_empty() {
                stack.push(Frame {
                    indent: indent + 2,
                    path: item_path,
                    child_count: 0,
                });
            } else if let Some((key, source_key, after_colon)) =
                split_nested_key(after_dash_trimmed)
            {
                let (fill, value_without_tag, had_non_fill_tag) =
                    record_fill_and_tags(&mut out, &after_colon, &key);
                unsupported_fill_tag |= value_has_unsupported_fill_tag(&value_without_tag);
                if fill {
                    let mut key_path = item_path.clone();
                    key_path.push(PathSegment::Key(key.clone()));
                    out.nested_fills.push(key_path);
                }
                if fill || had_non_fill_tag {
                    dash_body_clean = Some(format!("{}:{}", source_key, value_without_tag));
                }
                stack.push(Frame {
                    indent: inline_indent_offset,
                    path: item_path,
                    child_count: 1,
                });
            } else {
                unsupported_fill_tag |= value_has_unsupported_fill_tag(after_dash_trimmed);
            }

            if let Some(c) = &trailing_comment {
                out.nested_comments.push(NestedComment {
                    container_path: parent_path,
                    position: item_index,
                    text: strip_comment_marker(c).to_string(),
                    inline: true,
                });
            }
            if dash_body_clean.is_some() || trailing_comment.is_some() {
                let head = format!("{:width$}", "", width = indent);
                let body = match dash_body_clean {
                    Some(b) => format!("- {}", b),
                    None if after_dash.trim_end().is_empty() => "-".to_string(),
                    None => format!("- {}", after_dash.trim_end()),
                };
                cleaned.push(source_line, format!("{}{}", head, body));
            } else {
                cleaned.push(source_line, line.to_string());
            }

            // For a `- |-` item the content is indented past the dash, so the
            // dash line's indent is the block-scalar boundary.
            if is_block_scalar_header(after_dash_trimmed) {
                block_scalar_indent = Some(indent);
            }
            continue;
        }

        // Case 3: top-level field line.
        let is_top_level = indent == 0;
        if is_top_level {
            if let Some((key, after_colon)) = split_key(line) {
                let (value_part, trailing_comment) = split_trailing_comment(&after_colon);

                let (fill, value_without_tag, _) =
                    record_fill_and_tags(&mut out, &value_part, &key);
                unsupported_fill_tag |= value_has_unsupported_fill_tag(&value_without_tag);

                out.items.push(PreItem::Field {
                    key: key.clone(),
                    fill,
                });

                let root = &mut stack[0];
                root.child_count += 1;
                let key_path = vec![PathSegment::Key(key.clone())];

                while stack.len() > 1 {
                    stack.pop();
                }

                if has_empty_inline_value(&value_without_tag) {
                    stack.push(Frame {
                        indent: 2,
                        path: key_path,
                        child_count: 0,
                    });
                }

                cleaned.push(source_line, format!("{}:{}", key, value_without_tag));

                if let Some(c) = trailing_comment {
                    out.items.push(PreItem::Comment {
                        text: strip_comment_marker(&c).to_string(),
                        inline: true,
                    });
                }

                if is_block_scalar_header(&value_without_tag) {
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

            let (value_part, trailing_comment) = split_trailing_comment(&after_colon);

            let (fill, value_without_tag, _) = record_fill_and_tags(&mut out, &value_part, &key);
            unsupported_fill_tag |= value_has_unsupported_fill_tag(&value_without_tag);
            if fill {
                out.nested_fills.push(key_path.clone());
            }

            if trailing_comment.is_some() || fill {
                if let Some(c) = trailing_comment {
                    out.nested_comments.push(NestedComment {
                        container_path: parent_path,
                        position: key_index,
                        text: strip_comment_marker(&c).to_string(),
                        inline: true,
                    });
                }
                let head = format!("{:width$}", "", width = indent);
                cleaned.push(
                    source_line,
                    format!("{}{}:{}", head, source_key, value_without_tag),
                );
            } else {
                cleaned.push(source_line, line.to_string());
            }

            if has_empty_inline_value(&value_without_tag) {
                stack.push(Frame {
                    indent: indent + 2,
                    path: key_path,
                    child_count: 0,
                });
            }

            if is_block_scalar_header(&value_without_tag) {
                block_scalar_indent = Some(indent);
            }
            continue;
        }

        unsupported_fill_tag |= line_has_unsupported_fill_tag(line);
        cleaned.push(source_line, line.to_string());
    }

    if unsupported_fill_tag {
        out.warnings.push(
            Diagnostic::new(
                Severity::Warning,
                "a `!must_fill` marker appears in a flow collection or on a bare \
                 sequence element and is not preserved; use block style \
                 (`key: !must_fill`) to mark a placeholder"
                    .to_string(),
            )
            .with_code("parse::fill_marker_unsupported_position".to_string()),
        );
    }

    out.cleaned_yaml = cleaned.lines.join("\n");
    out.source_lines = cleaned.source_lines;
    out
}

/// True when `value` — the text after a `key:` or a `- `, with a block-style
/// marker already stripped — carries a `!must_fill` tag prescan cannot lift:
/// the whole element, or a member of a flow collection. A quoted scalar is
/// text, so a tag spelled inside one (`note: "see key: !must_fill here"`) does
/// not match.
fn value_has_unsupported_fill_tag(value: &str) -> bool {
    let value = value.trim_start();
    !value.starts_with(['"', '\'']) && fill_tag_in_node_position(value, true)
}

/// True when `line`, which prescan resolved into neither a key nor an element
/// (a flow collection continued across lines), carries a `!must_fill` tag
/// after flow punctuation. What opens the line is unknown, so a tag at its
/// head is not read as a node.
fn line_has_unsupported_fill_tag(line: &str) -> bool {
    fill_tag_in_node_position(line, false)
}

/// True when a fill tag in `text` stands where YAML reads a node: after flow
/// punctuation, or — when `head_is_node` — at the head of `text` itself.
fn fill_tag_in_node_position(text: &str, head_is_node: bool) -> bool {
    let mut from = 0;
    while let Some(rel) = text[from..].find(FILL_TAG) {
        let at = from + rel;
        let after = at + FILL_TAG.len();
        // Trailing boundary: a real tag ends at whitespace, flow
        // punctuation, or end of text, not mid-word (`!fillet`).
        let trailing_ok = text[after..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || matches!(c, ',' | '}' | ']'));
        // Leading boundary: the tag sits directly after `{` / `[` / `,`,
        // or after whitespace following `:` / `-` / `,` / `{` / `[`.
        let before = text[..at].trim_end_matches([' ', '\t']);
        let had_ws = before.len() != at;
        let leading_ok = match before.chars().last() {
            Some('{') | Some('[') | Some(',') => true,
            Some(':') | Some('-') => had_ws,
            None => head_is_node,
            _ => false,
        };
        if trailing_ok && leading_ok {
            return true;
        }
        from = after;
    }
    false
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

fn strip_comment_marker(raw: &str) -> &str {
    let after = raw.trim_start_matches('#');
    after.strip_prefix(' ').unwrap_or(after)
}

fn leading_space_count(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// `true` when a field value is a YAML block-scalar header (`|` or `>`, with
/// optional chomping/indent indicators). Unquoted plain scalars cannot begin
/// with these characters, so a leading `|`/`>` unambiguously opens a literal/
/// folded block whose following content lines are text, not YAML structure.
fn is_block_scalar_header(value: &str) -> bool {
    let t = value.trim_start();
    t.starts_with('|') || t.starts_with('>')
}

/// `true` when the value portion of a `key:` line is empty: real value is on
/// subsequent indented lines.
fn has_empty_inline_value(after_colon: &str) -> bool {
    let (v, _) = split_trailing_comment(after_colon);
    v.trim().is_empty()
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
/// [`key_end`]'s bare form plus the two spellings `emit_key` also writes at
/// depth, nested keys being arbitrary user data: a quoted scalar, and a plain
/// scalar carrying characters the bare form excludes (`a b`). A plain key ends
/// at the first `: `, or at a `:` closing the line — YAML's own boundary.
fn nested_key_end(line: &str) -> Option<usize> {
    if let Some(i) = key_end(line) {
        return Some(i);
    }
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
    if PLAIN_SCALAR_EXCLUDED_FIRST.contains(&first) {
        return None;
    }
    for i in 1..bytes.len() {
        if bytes[i] == b'#' && bytes[i - 1] == b' ' {
            return None;
        }
        if bytes[i] == b':' && matches!(bytes.get(i + 1), None | Some(b' ')) {
            return Some(i);
        }
    }
    None
}

/// The YAML indicators a plain scalar cannot open with. A key needing one is
/// emitted quoted, as is one carrying a ` #`, so neither is read as a plain key.
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
/// scalar only when it is the *first* character of the scalar, or appears
/// inside a flow collection (`[`/`{`). Inside a plain scalar, `'` and `"`
/// are ordinary characters: `x: it's fine # note` carries a comment.
fn split_trailing_comment(value: &str) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let Some(first) = bytes.iter().position(|b| !matches!(b, b' ' | b'\t')) else {
        return (value.to_string(), None);
    };
    match bytes[first] {
        // Quoted scalar: skip the quoted body, then scan for a comment. An
        // unterminated quote means the scalar continues on the next line:
        // no comment on this one.
        b'"' | b'\'' => match find_quote_end(bytes, first) {
            Some(end) => find_comment_from(value, end + 1),
            None => (value.to_string(), None),
        },
        // Flow collection: quotes open quoted scalars anywhere inside, so
        // track quote state across the whole value.
        b'[' | b'{' => split_flow_trailing_comment(value),
        // Plain scalar (or block-scalar header): quotes are ordinary
        // characters; only the whitespace-then-`#` rule applies.
        _ => find_comment_from(value, 0),
    }
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

/// Comment split for flow-collection values (`[…]` / `{…}`), where quoted
/// scalars can open anywhere: track quote state across the value and split
/// at the first whitespace-preceded `#` outside quotes.
fn split_flow_trailing_comment(value: &str) -> (String, Option<String>) {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut prev_was_ws = true;
    let mut in_dq = false;
    let mut in_sq = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\'' {
                in_sq = false;
            }
        } else {
            if b == b'"' {
                in_dq = true;
            } else if b == b'\'' {
                in_sq = true;
            } else if b == b'#' && prev_was_ws {
                let v = value[..i].trim_end().to_string();
                let c = value[i..].to_string();
                return (v, Some(c));
            }
        }
        prev_was_ws = matches!(b, b' ' | b'\t');
        i += 1;
    }
    (value.to_string(), None)
}

/// The placeholder tag. `!must_fill` is the only recognized fill tag; any
/// other custom tag is treated as a noncanonical tag: dropped with a
/// `parse::unsupported_yaml_tag` warning.
const FILL_TAG: &str = "!must_fill";

/// If `trimmed` begins with the fill tag (either the bare tag or the tag
/// followed by whitespace), return the remainder after the tag. A tag that
/// is merely a prefix of a longer word (e.g. `!fillet`) does not match.
fn strip_fill_tag(trimmed: &str) -> Option<&str> {
    if trimmed == FILL_TAG {
        return Some("");
    }
    let rest = trimmed.strip_prefix(FILL_TAG)?;
    (rest.starts_with(' ') || rest.starts_with('\t')).then_some(rest)
}

/// Inspect a field value for the `!must_fill` tag and other (noncanonical)
/// tags, warning onto `out` for a noncanonical tag. Returns
/// `(fill, value_without_tag, had_other_tag)`.
fn record_fill_and_tags(out: &mut PreScan, value: &str, key: &str) -> (bool, String, bool) {
    let trimmed = value.trim_start();
    let leading_ws_len = value.len() - trimmed.len();

    if trimmed.is_empty() {
        return (false, value.to_string(), false);
    }

    if let Some(rest) = strip_fill_tag(trimmed) {
        let rest_trim = rest.trim_start();
        let reconstructed = if rest_trim.is_empty() {
            value[..leading_ws_len].to_string()
        } else {
            format!(" {}", rest_trim)
        };
        return (true, reconstructed, false);
    }

    if trimmed.starts_with('!') {
        out.warnings.push(
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "YAML tag on key `{}` is not supported; the tag has been dropped and the value kept",
                    key
                ),
            )
            .with_code("parse::unsupported_yaml_tag".to_string()),
        );
        return (false, value.to_string(), true);
    }

    (false, value.to_string(), false)
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
                    fill: false,
                },
                PreItem::Comment {
                    text: "mid".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "author".to_string(),
                    fill: false,
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
                    fill: false,
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
    fn fill_alias_is_rejected_as_noncanonical_tag() {
        let input = "dept: !fill Department\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "dept".to_string(),
                fill: false,
            }]
        );
        assert!(
            out.warnings
                .iter()
                .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
            "`!fill` must warn as an unsupported tag"
        );
    }

    #[test]
    fn detects_must_fill_on_scalar() {
        let input = "dept: !must_fill Department\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "dept".to_string(),
                fill: true,
            }]
        );
        assert!(out.cleaned_yaml.contains("dept: Department"));
        assert!(!out.cleaned_yaml.contains("!must_fill"));
        assert!(!out.cleaned_yaml.contains("!fill"));
    }

    #[test]
    fn detects_bare_must_fill() {
        let input = "dept: !must_fill\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "dept".to_string(),
                fill: true,
            }]
        );
        assert!(!out.cleaned_yaml.contains("!must_fill"));
    }

    #[test]
    fn crlf_lines_carry_no_carriage_return_into_the_scan() {
        let input = "dept: !must_fill\r\n# note\r\ntitle: x # trailing\r\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![
                PreItem::Field {
                    key: "dept".to_string(),
                    fill: true,
                },
                PreItem::Comment {
                    text: "note".to_string(),
                    inline: false,
                },
                PreItem::Field {
                    key: "title".to_string(),
                    fill: false,
                },
                PreItem::Comment {
                    text: "trailing".to_string(),
                    inline: true,
                },
            ]
        );
        assert!(out.warnings.is_empty(), "got: {:?}", out.warnings);
        assert!(!out.cleaned_yaml.contains('\r'));
    }

    #[test]
    fn fillet_is_not_a_fill_tag() {
        let input = "x: !must_filler value\n";
        let out = prescan_fence_content(input);
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "x".to_string(),
                fill: false,
            }]
        );
    }

    #[test]
    fn unknown_tag_warns() {
        let input = "x: !custom value\n";
        let out = prescan_fence_content(input);
        assert!(
            out.warnings
                .iter()
                .any(|w| w.code.as_deref() == Some("parse::unsupported_yaml_tag")),
            "expected unsupported_yaml_tag warning"
        );
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
        assert!(
            !out.warnings
                .iter()
                .any(|w| w.code.as_deref() == Some("parse::comments_in_nested_yaml_dropped")),
            "nested comments are preserved, so no dropped-comment warning is emitted"
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
    fn fill_on_flow_sequence_allowed() {
        let input = "x: !must_fill [1, 2]\n";
        let out = prescan_fence_content(input);
        assert!(
            out.warnings.is_empty(),
            "expected no diagnostic; !must_fill on sequences is supported"
        );
        assert_eq!(
            out.items,
            vec![PreItem::Field {
                key: "x".to_string(),
                fill: true,
            }]
        );
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
    fn source_lines_map_cleaned_lines_back_over_dropped_comments() {
        let input = "# lead\ntitle: Doc\n\n# note\nrole: !must_fill\nbio: |\n  # not a comment\nend: x\n";
        let out = prescan_fence_content(input);

        assert_eq!(out.cleaned_yaml.split('\n').count(), out.source_lines.len());
        let source = |needle: &str| {
            let i = out
                .cleaned_yaml
                .split('\n')
                .position(|l| l.contains(needle))
                .expect("cleaned line present");
            out.source_lines[i]
        };
        assert_eq!(source("title:"), 1);
        assert_eq!(source("role:"), 4);
        assert_eq!(source("# not a comment"), 6);
        assert_eq!(source("end:"), 7);
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
