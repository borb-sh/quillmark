//! Line-oriented fence scanner for card-yaml blocks.
//!
//! A column-zero `~~~` fence (three or more tildes) opens a card-yaml block
//! whatever its info string. Openers inside an ordinary CommonMark fenced code
//! block are literal content, so a backtick fence writes one in prose.

use crate::error::ParseError;
use crate::{Diagnostic, Severity};

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
    let indent = line.as_bytes().iter().take_while(|&&b| b == b' ').count();
    if indent > 3 {
        return None;
    }
    let trimmed = &line[indent..];
    let bytes = trimmed.as_bytes();
    let &first = bytes.first()?;

    if first != b'`' && first != b'~' {
        return None;
    }
    let run_len = bytes.iter().take_while(|&&b| b == first).count();
    if run_len < 3 {
        return None;
    }
    let rest = &trimmed[run_len..];
    match open_fence {
        Some((open_char, open_len)) => {
            if first == open_char
                && run_len >= open_len
                && rest.chars().all(|c| c == ' ' || c == '\t')
            {
                Some((first, run_len, true))
            } else {
                None
            }
        }
        None => Some((first, run_len, false)),
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

/// Used by the `Quill.yaml` `body.example` guard, so the blueprint-corruption
/// check stays in lock-step with the parser.
pub(crate) fn is_card_yaml_opener_line(line: &str) -> bool {
    card_yaml_opener_run(line).is_some()
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
        .filter_map(|text| super::prescan::key_end(text).map(|end| text[..end].to_string()))
        .last()
}

fn near_closer_below(lines: &Lines<'_>, opener_k: usize) -> Option<(usize, String)> {
    ((opener_k + 1)..lines.len()).find_map(|j| {
        let trimmed = lines.line_text(j).trim();
        (!trimmed.is_empty() && trimmed.bytes().all(|b| b == b'~')).then(|| (j, trimmed.to_string()))
    })
}

/// Find all card-yaml metadata blocks. A block requires a blank line above it,
/// so body round-tripping stays stable.
pub(super) fn find_metadata_blocks(markdown: &str) -> Result<FenceScan, ParseError> {
    let lines = Lines::new(markdown);
    let mut blocks: Vec<MetadataBlock> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    let mut unclosed_root: Option<UnclosedRoot> = None;
    // (char, run_len, opener_line_index) of an open ordinary code fence.
    let mut open_code_fence: Option<(u8, usize, usize)> = None;

    let mut k: usize = 0;
    while k < lines.len() {
        let text = lines.line_text(k);

        // Inside an ordinary code block: openers are literal content.
        if let Some((ch, min, _opener)) = open_code_fence {
            if let Some((_, _, true)) = code_fence_on_line(text, Some((ch, min))) {
                open_code_fence = None;
            }
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
                open_code_fence = Some((b'~', open_run, k));
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
                open_code_fence = Some((b'~', open_run, k));
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
            k = cj + 1;
            continue;
        }

        // Any other fence opener is an ordinary fenced code block.
        if let Some((ch, run_len, _)) = code_fence_on_line(text, None) {
            open_code_fence = Some((ch, run_len, k));
        }
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
    if let Some((_, _, opener_line)) = open_code_fence {
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
