//! Line-oriented fence scanner for card-yaml blocks.
//!
//! A column-zero `~~~` fence (three or more tildes) opens a card-yaml block
//! whatever its info string. Openers inside an ordinary CommonMark fenced code
//! block are literal content, so a backtick fence writes one in prose.
//!
//! Ordinary fences are read inside CommonMark containers, list items and block
//! quotes: a fence in a container closes where the container does, so a
//! column-zero line, which ends every container, is never shielded by one.

use std::borrow::Cow;

use crate::error::ParseError;
use crate::error::{Diagnostic, Severity};
use quillmark_content::normalize::fence_opener;

use super::assemble::MetadataBlock;

pub(super) struct Lines<'a> {
    pub(super) source: &'a str,
    pub(super) starts: Vec<usize>, // byte offset of each line's first character
}

impl<'a> Lines<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        let mut starts = Vec::new();
        starts.push(0);
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { source, starts }
    }
    pub(super) fn len(&self) -> usize {
        self.starts.len()
    }
    pub(super) fn line_start(&self, k: usize) -> usize {
        self.starts[k]
    }
    /// Byte position after line k's trailing `\n`, or end-of-source.
    pub(super) fn line_end_inclusive(&self, k: usize) -> usize {
        if k + 1 < self.starts.len() {
            self.starts[k + 1]
        } else {
            self.source.len()
        }
    }
    pub(super) fn line_text(&self, k: usize) -> &'a str {
        let start = self.starts[k];
        let mut end = self.line_end_inclusive(k);
        if end > start && self.source.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && self.source.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        &self.source[start..end]
    }
    pub(super) fn is_blank(&self, k: usize) -> bool {
        self.line_text(k).chars().all(char::is_whitespace)
    }
}

/// `Some((char, run_len, is_closing))` when `line` opens a CommonMark fenced
/// code block, or closes the one named by `open_fence`.
pub(super) fn code_fence_on_line(
    line: &str,
    open_fence: Option<(u8, usize)>,
) -> Option<(u8, usize, bool)> {
    let (c, n, info) = fence_opener(line)?;
    match open_fence {
        Some((open_char, open_len)) => {
            (c == open_char && n >= open_len && info.trim_matches([' ', '\t']).is_empty())
                .then_some((c, n, true))
        }
        None => Some((c, n, false)),
    }
}

/// The tilde-run length (`>= 3`) when `line` opens a card-yaml block.
///
/// Spec §3.2. The info string is not read: every column-zero tilde fence is a
/// card, so a language-tagged one is no escape. The opener must be at column
/// zero — an indented `~~~` is a valid CommonMark code fence, and claiming it
/// would split at an offset the body renderer disagrees with. A longer run is
/// accepted and normalised on emit; its closer must be at least as long
/// (CommonMark fence matching).
fn card_yaml_opener_run(line: &str) -> Option<usize> {
    if line.starts_with(' ') {
        return None;
    }
    match code_fence_on_line(line, None) {
        Some((b'~', run, false)) => Some(run),
        _ => None,
    }
}

/// `text` with each tab widened to the next multiple-of-four column, the tab
/// stop CommonMark measures indentation by.
fn expand_tabs(text: &str) -> Cow<'_, str> {
    if !text.contains('\t') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut col = 0;
    for c in text.chars() {
        if c == '\t' {
            let width = 4 - col % 4;
            out.extend(std::iter::repeat_n(' ', width));
            col += width;
        } else {
            out.push(c);
            col += 1;
        }
    }
    Cow::Owned(out)
}

fn leading_spaces(text: &str) -> usize {
    text.bytes().take_while(|&b| b == b' ').count()
}

/// A list item opening `rest`.
struct ListMarker {
    /// The offset of its content column.
    width: usize,
    /// Whether it may interrupt a paragraph its line continues (CommonMark
    /// §5.2: not empty, and a bullet or an ordered item starting at 1).
    interrupts: bool,
    /// Whether nothing follows the marker on its line.
    empty: bool,
}

fn list_marker(rest: &str) -> Option<ListMarker> {
    let bytes = rest.as_bytes();
    let at = leading_spaces(rest);
    if at > 3 {
        return None;
    }
    let (end, starts_at_one) = match bytes.get(at)? {
        b'-' | b'+' | b'*' => (at + 1, true),
        b'0'..=b'9' => {
            let digits = bytes[at..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            let end = at + digits;
            if digits > 9 || !matches!(bytes.get(end), Some(b'.' | b')')) {
                return None;
            }
            (end + 1, rest[at..end].trim_start_matches('0') == "1")
        }
        _ => return None,
    };
    let gap = bytes[end..].iter().take_while(|&&b| b == b' ').count();
    if end + gap == bytes.len() {
        return Some(ListMarker {
            width: end + 1,
            interrupts: false,
            empty: true,
        });
    }
    if gap == 0 {
        return None;
    }
    let width = if gap > 4 { end + 1 } else { end + gap };
    Some(ListMarker {
        width,
        interrupts: starts_at_one,
        empty: false,
    })
}

fn is_thematic_break(rest: &str) -> bool {
    let t = rest.trim_start_matches(' ');
    let Some(c) = t.bytes().next().filter(|c| matches!(c, b'-' | b'*' | b'_')) else {
        return false;
    };
    leading_spaces(rest) <= 3
        && t.bytes().all(|b| b == c || b == b' ' || b == b'\t')
        && t.bytes().filter(|&b| b == c).count() >= 3
}

/// A `=` or `-` run, which closes the paragraph above it as a setext heading.
fn is_setext_underline(rest: &str) -> bool {
    let t = rest.trim_matches(' ');
    leading_spaces(rest) <= 3
        && t.bytes()
            .next()
            .is_some_and(|c| (c == b'=' || c == b'-') && t.bytes().all(|b| b == c))
}

fn is_atx_heading(rest: &str) -> bool {
    let t = rest.trim_start_matches(' ');
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    leading_spaces(rest) <= 3
        && (1..=6).contains(&hashes)
        && matches!(t.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// A CommonMark container block: a list item, by its content width and
/// whether it holds no content yet, or a block quote.
enum Container {
    Item { width: usize, empty: bool },
    Quote,
}

/// The content of a block-quote line `rest`, past its `>` and one space.
fn quote_content(rest: &str) -> Option<&str> {
    let at = leading_spaces(rest);
    let after = rest.get(at..).filter(|_| at <= 3)?.strip_prefix('>')?;
    Some(after.strip_prefix(' ').unwrap_or(after))
}

/// How many of the open `containers` `line` continues, outermost first, and
/// the line past them. A blank line ends a quote, and a list item that holds no
/// content yet: an item starts with at most one blank line (CommonMark §5.2).
fn continued<'a>(containers: &[Container], line: &'a str) -> (usize, &'a str) {
    let mut rest = line;
    for (depth, container) in containers.iter().enumerate() {
        rest = match container {
            Container::Item { empty: true, .. } if rest.trim().is_empty() => return (depth, rest),
            Container::Item { .. } if rest.trim().is_empty() => "",
            Container::Item { width, .. } if leading_spaces(rest) >= *width => &rest[*width..],
            Container::Quote => match quote_content(rest) {
                Some(quoted) => quoted,
                None => return (depth, rest),
            },
            Container::Item { .. } => return (depth, rest),
        };
    }
    (containers.len(), rest)
}

/// Whether `rest`, a line short of its containers, opens a block, so it ends
/// them instead of continuing their paragraph lazily. Any list item does: the
/// §5.2 restriction binds only a line that continues the paragraph's containers.
fn opens_block(rest: &str) -> bool {
    fence_opener(rest).is_some()
        || is_thematic_break(rest)
        || is_atx_heading(rest)
        || quote_content(rest).is_some()
        || list_marker(rest).is_some()
}

/// The first line below `opener_k` whose text `closes` accepts as the closer.
fn closer_below(
    lines: &Lines<'_>,
    opener_k: usize,
    closes: impl Fn(&str) -> bool,
) -> Option<usize> {
    ((opener_k + 1)..lines.len()).find(|&j| closes(lines.line_text(j)))
}

/// A card-yaml opener declaring `$quill` that nothing closes. Carried out of
/// the scan so the `MissingQuill` message names the malformation instead of
/// describing the shape the author already wrote.
pub(super) struct UnclosedRoot {
    pub(super) opener_line: usize,
    /// The first all-tilde line below the opener: a run too short to close it,
    /// or an indented one.
    pub(super) near_closer: Option<(usize, String)>,
    /// The last top-level key of the payload the author wrote.
    pub(super) last_field: Option<String>,
}

pub(super) struct FenceScan {
    pub(super) blocks: Vec<MetadataBlock>,
    pub(super) warnings: Vec<Diagnostic>,
    pub(super) unclosed_root: Option<UnclosedRoot>,
}

/// The payload lines of a would-be block at `opener_k`: down to the first blank
/// line, which is where an author who forgot the closer stopped writing YAML.
fn payload_lines<'a>(lines: &'a Lines<'a>, opener_k: usize) -> impl Iterator<Item = usize> + 'a {
    ((opener_k + 1)..lines.len()).take_while(|&j| !lines.is_blank(j))
}

fn declares_quill_beneath(lines: &Lines<'_>, opener_k: usize) -> bool {
    payload_lines(lines, opener_k).any(|j| lines.line_text(j).trim_start().starts_with("$quill:"))
}

fn last_field_key(lines: &Lines<'_>, opener_k: usize) -> Option<String> {
    payload_lines(lines, opener_k)
        .map(|j| lines.line_text(j))
        .filter(|text| !text.starts_with(' '))
        .filter_map(|text| key_end(text).map(|end| text[..end].to_string()))
        .last()
}

/// Byte index of the `:` closing `line`'s leading key, or `None` when `line`
/// does not open with one. A key is `[a-zA-Z_][a-zA-Z0-9_]*`, optionally
/// `$`-prefixed for system keys.
fn key_end(line: &str) -> Option<usize> {
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

fn near_closer_below(lines: &Lines<'_>, opener_k: usize) -> Option<(usize, String)> {
    ((opener_k + 1)..lines.len()).find_map(|j| {
        let trimmed = lines.line_text(j).trim();
        (!trimmed.is_empty() && trimmed.bytes().all(|b| b == b'~'))
            .then(|| (j, trimmed.to_string()))
    })
}

/// Find all card-yaml metadata blocks. A block requires a blank line above it,
/// so body round-tripping stays stable.
pub(super) fn find_metadata_blocks(markdown: &str) -> Result<FenceScan, ParseError> {
    let lines = Lines::new(markdown);
    let mut blocks: Vec<MetadataBlock> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    let mut unclosed_root: Option<UnclosedRoot> = None;
    // (char, run_len, container_depth, opener_line_index) of an open ordinary
    // code fence.
    let mut open_code_fence: Option<(u8, usize, usize, usize)> = None;
    let mut containers: Vec<Container> = Vec::new();
    let mut in_paragraph = false;

    let mut k: usize = 0;
    while k < lines.len() {
        let text = lines.line_text(k);
        let columns = expand_tabs(text);
        let (matched, rest) = continued(&containers, &columns);
        if !rest.trim().is_empty() {
            for container in &mut containers[..matched] {
                if let Container::Item { empty, .. } = container {
                    *empty = false;
                }
            }
        }

        // Inside an ordinary code block: openers are literal content.
        if let Some((ch, min, depth, _opener)) = open_code_fence {
            if matched == depth {
                if let Some((_, _, true)) = code_fence_on_line(rest, Some((ch, min))) {
                    open_code_fence = None;
                }
                k += 1;
                continue;
            }
            open_code_fence = None;
        }

        let blank = rest.trim().is_empty();
        if matched < containers.len() {
            if in_paragraph && !blank && !opens_block(rest) {
                k += 1;
                continue;
            }
            containers.truncate(matched);
            in_paragraph = false;
        }
        if blank {
            in_paragraph = false;
            k += 1;
            continue;
        }

        if let Some(open_run) = card_yaml_opener_run(text) {
            // Without a blank line above, the block is delegated to CommonMark
            // as an ordinary `~~~` code block.
            let blank_above = k == 0 || lines.is_blank(k - 1);
            if !blank_above {
                warnings.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`~~~` card-yaml block at line {} has no blank line above it: it is treated as an ordinary code block, not a card-yaml block. Insert a blank line before it to register it.",
                            k + 1
                        ),
                    )
                    .with_code("parse::card_fence_missing_blank".to_string()),
                );
                open_code_fence = Some((b'~', open_run, 0, k));
                k += 1;
                continue;
            }

            // The closer must be a tilde run at least as long as the opener
            // (CommonMark fence matching) and at column zero (spec §3.2 / D2):
            // the payload is YAML, where indentation is structural, so an
            // indented `~~~` inside a block scalar is payload, never a closer.
            let closer_k = closer_below(&lines, k, |candidate| {
                !candidate.starts_with(' ')
                    && matches!(
                        code_fence_on_line(candidate, Some((b'~', open_run))),
                        Some((_, _, true))
                    )
            });
            let Some(cj) = closer_k else {
                // Per CommonMark an unclosed `~~~` fence is an ordinary code
                // block running to EOF, so delegate rather than erroring. The
                // end-of-document check below surfaces the warning.
                if blocks.is_empty() && unclosed_root.is_none() && declares_quill_beneath(&lines, k)
                {
                    unclosed_root = Some(UnclosedRoot {
                        opener_line: k,
                        near_closer: near_closer_below(&lines, k),
                        last_field: last_field_key(&lines, k),
                    });
                }
                open_code_fence = Some((b'~', open_run, 0, k));
                k += 1;
                continue;
            };

            let block = super::assemble::build_block(
                markdown,
                lines.line_start(k),
                lines.line_end_inclusive(k),
                lines.line_start(cj),
                lines.line_end_inclusive(cj),
                blocks.len(),
            )?;
            blocks.push(block);
            containers.clear();
            in_paragraph = false;
            k = cj + 1;
            continue;
        }

        // Any other fence opener is an ordinary fenced code block, possibly
        // behind container markers opened on the same line.
        let mut rest = rest;
        let mut interrupting = in_paragraph;
        in_paragraph = loop {
            if let Some((ch, run_len, _)) = code_fence_on_line(rest, None) {
                open_code_fence = Some((ch, run_len, containers.len(), k));
                break false;
            }
            if rest.trim().is_empty() || is_thematic_break(rest) || is_atx_heading(rest) {
                break false;
            }
            if leading_spaces(rest) >= 4 {
                break interrupting;
            }
            // Past the depth the body import admits, markers read as text: every
            // container closes at column zero, so no card turns on them.
            if containers.len() >= crate::error::MAX_NESTING_DEPTH {
                break true;
            }
            if interrupting && is_setext_underline(rest) {
                break false;
            }
            if let Some(quoted) = quote_content(rest) {
                containers.push(Container::Quote);
                rest = quoted;
            } else {
                match list_marker(rest) {
                    Some(marker) if marker.interrupts || !interrupting => {
                        containers.push(Container::Item {
                            width: marker.width,
                            empty: marker.empty,
                        });
                        rest = rest.get(marker.width..).unwrap_or("");
                    }
                    _ => break true,
                }
            }
            interrupting = false;
        };
        k += 1;
    }

    // Composable cards are every block after the root (spec §8).
    let card_count = blocks.len().saturating_sub(1);
    if card_count > crate::error::MAX_CARD_COUNT {
        return Err(ParseError::TooManyCards {
            count: card_count,
            max: crate::error::MAX_CARD_COUNT,
        });
    }

    // Card-yaml blocks below an unclosed opener were silently shielded, which
    // is almost never intended.
    if let Some((_, _, _, opener_line)) = open_code_fence {
        warnings.push(
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "Unclosed fenced code block opened at line {}: end-of-document reached without a matching closing fence. Any `~~~` card-yaml blocks after this line were treated as code and not parsed.",
                    opener_line + 1
                ),
            )
            .with_code("parse::unclosed_code_block".to_string()),
        );
    }

    Ok(FenceScan {
        blocks,
        warnings,
        unclosed_root,
    })
}
