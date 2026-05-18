//! Assembly of card-yaml blocks into a [`Document`].
//!
//! This module contains the top-level parsing glue: it calls the fence scanner,
//! parses each block's `#@` system sentinel, and assembles a typed [`Document`]
//! from the pieces.

use std::str::FromStr;

use crate::error::ParseError;
use crate::value::QuillValue;
use crate::version::QuillReference;
use crate::Diagnostic;

use super::fences::find_metadata_blocks;
use super::frontmatter::{Frontmatter, FrontmatterItem};
use super::prescan::{prescan_fence_content, NestedComment, PreItem};
use super::sentinel::{is_valid_tag_name, parse_system_sentinel, validate_payload_yaml};
use super::{Card, Document, Sentinel};

/// Strip exactly one structural separator from the tail of a body slice.
///
/// Every card-yaml block requires a blank line immediately above it. When a
/// body is followed by another block, the raw slice ends with that blank
/// line's terminator — exactly one `\n` or `\r\n`. This helper strips that
/// single line ending so stored bodies contain only authored content. The
/// emitter re-adds the separator on output via `ensure_blank_before_fence`.
fn strip_blank_separator(body: &str) -> &str {
    if let Some(rest) = body.strip_suffix("\r\n") {
        rest
    } else if let Some(rest) = body.strip_suffix('\n') {
        rest
    } else {
        body
    }
}

/// An intermediate representation of one `~~~card-yaml … ~~~` block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize, // Position of the opening `~~~card-yaml`
    pub(super) end: usize,   // Position after the closing `~~~`
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML payload as JSON
    pub(super) tag: Option<String>, // Card kind from `#@kind:` (composable blocks)
    pub(super) quill_ref: Option<String>, // Quill reference from `#@quill:` (root block)
    /// Pre-scan items (comments + fill-tagged field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// Creates serde_saphyr Options with security budgets configured.
fn yaml_parse_options() -> serde_saphyr::Options {
    let budget = serde_saphyr::Budget {
        max_depth: super::limits::MAX_YAML_DEPTH,
        ..Default::default()
    };
    serde_saphyr::Options {
        budget: Some(budget),
        ..Default::default()
    }
}

/// Split a block's raw content into its `#@` system-sentinel line (the first
/// non-blank line) and the YAML payload that follows it.
///
/// Returns `(Some(sentinel_line), payload)` when a non-blank line is found, or
/// `(None, "")` when the block content is entirely blank.
fn split_sentinel_line(content: &str) -> (Option<&str>, &str) {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let line_text = line.trim_end_matches(['\n', '\r']);
        if line_text.trim().is_empty() {
            offset += line.len();
            continue;
        }
        return (Some(line_text), &content[offset + line.len()..]);
    }
    (None, "")
}

/// Process one recognised `~~~card-yaml` block and build a [`MetadataBlock`].
///
/// `block_start` / `block_end` bound the whole block (used to slice card
/// bodies). `content_start` / `content_end` bound the block content between
/// the `~~~card-yaml` opener and its `~~~` closer.
///
/// `block_index` discriminates the root block (index 0, which must declare
/// `#@quill:`) from composable blocks (which must declare `#@kind:`).
pub(super) fn build_block(
    markdown: &str,
    block_start: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
    block_index: usize,
) -> Result<MetadataBlock, ParseError> {
    let raw_content = &markdown[content_start..content_end];

    // Check YAML size limit (spec §8)
    if raw_content.len() > crate::error::MAX_YAML_SIZE {
        return Err(ParseError::InputTooLarge {
            size: raw_content.len(),
            max: crate::error::MAX_YAML_SIZE,
        });
    }

    // Separate the `#@` system sentinel from the YAML payload.
    let (sentinel_line, yaml_payload) = split_sentinel_line(raw_content);
    let (tag, quill_ref) = resolve_sentinel(block_index, sentinel_line)?;

    // Run the pre-scan over the YAML payload to extract top-level comments,
    // `!fill` markers, and warn on unsupported tags / nested comments.
    let pre = prescan_fence_content(yaml_payload);

    if let Some(err) = pre.fill_target_errors.first() {
        return Err(ParseError::InvalidStructure(err.clone()));
    }

    let content = pre.cleaned_yaml.trim().to_string();
    let yaml_value = if content.is_empty() {
        None
    } else {
        let parsed = match serde_saphyr::from_str_with_options::<serde_json::Value>(
            &content,
            yaml_parse_options(),
        ) {
            Ok(parsed) => parsed,
            Err(e) => {
                let line = markdown[..block_start].lines().count() + 1;
                return Err(ParseError::YamlErrorWithLocation {
                    message: e.to_string(),
                    line,
                    block_index,
                });
            }
        };
        validate_payload_yaml(parsed)?
    };

    // Per-block field-count check (spec §8)
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        if map.len() > crate::error::MAX_FIELD_COUNT {
            return Err(ParseError::InputTooLarge {
                size: map.len(),
                max: crate::error::MAX_FIELD_COUNT,
            });
        }
    }

    Ok(MetadataBlock {
        start: block_start,
        end: block_end,
        yaml_value,
        tag,
        quill_ref,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_warnings: pre.warnings,
    })
}

/// Resolve the `#@` system sentinel for a block, returning `(tag, quill_ref)`.
///
/// The root block (index 0) must declare `#@quill: <name>`; every composable
/// block must declare `#@kind: <type>`.
#[allow(clippy::type_complexity)]
fn resolve_sentinel(
    block_index: usize,
    sentinel_line: Option<&str>,
) -> Result<(Option<String>, Option<String>), ParseError> {
    let parsed = sentinel_line.and_then(parse_system_sentinel);

    if block_index == 0 {
        match parsed {
            Some((key, value)) if key == "quill" => {
                if value.is_empty() {
                    return Err(ParseError::InvalidStructure(
                        "`#@quill:` system sentinel has no value — expected `#@quill: <name>`"
                            .to_string(),
                    ));
                }
                Ok((None, Some(value)))
            }
            Some((key, _)) => Err(ParseError::MissingQuillField(format!(
                "The document's first card-yaml block must declare `#@quill: <name>` (found `#@{}:`).",
                key
            ))),
            None => Err(ParseError::MissingQuillField(
                "The document's first card-yaml block is missing its `#@quill: <name>` system sentinel."
                    .to_string(),
            )),
        }
    } else {
        match parsed {
            Some((key, value)) if key == "kind" => {
                if !is_valid_tag_name(&value) {
                    return Err(ParseError::InvalidStructure(format!(
                        "Invalid card kind '{}': must match pattern [a-z_][a-z0-9_]*",
                        value
                    )));
                }
                Ok((Some(value), None))
            }
            Some((key, _)) if key == "quill" => Err(ParseError::InvalidStructure(
                "`#@quill` may only be declared by the document's first card-yaml block"
                    .to_string(),
            )),
            Some((key, _)) => Err(ParseError::InvalidStructure(format!(
                "A composable card-yaml block must declare `#@kind: <type>` (found `#@{}:`).",
                key
            ))),
            None => Err(ParseError::InvalidStructure(
                "A composable card-yaml block is missing its `#@kind: <type>` system sentinel."
                    .to_string(),
            )),
        }
    }
}

/// Decompose markdown, discarding warnings. Test- and `from_markdown`-facing.
pub(super) fn decompose(markdown: &str) -> Result<Document, crate::error::ParseError> {
    decompose_with_warnings(markdown).map(|(doc, _)| doc)
}

/// Decompose markdown into a typed [`Document`], returning any non-fatal warnings
/// collected during fence scanning.
pub(super) fn decompose_with_warnings(
    markdown: &str,
) -> Result<(Document, Vec<Diagnostic>), crate::error::ParseError> {
    // Strip a leading UTF-8 BOM if present.
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);

    if markdown.trim().is_empty() {
        return Err(crate::error::ParseError::EmptyInput(
            "Empty markdown input cannot be parsed as a Quillmark Document. \
             Provide at least a root card-yaml block declaring `#@quill: <name>`."
                .to_string(),
        ));
    }

    // Check input size limit
    if markdown.len() > crate::error::MAX_INPUT_SIZE {
        return Err(crate::error::ParseError::InputTooLarge {
            size: markdown.len(),
            max: crate::error::MAX_INPUT_SIZE,
        });
    }

    // Find all card-yaml blocks. The scanner guarantees block 0 carries the
    // `#@quill` sentinel and every later block carries `#@kind`.
    let (blocks, warnings) = find_metadata_blocks(markdown)?;

    if blocks.is_empty() {
        return Err(crate::error::ParseError::MissingQuillField(
            "Missing required root card-yaml block. The document must open with a \
             `~~~card-yaml` block declaring `#@quill: <name>`."
                .to_string(),
        ));
    }

    // Block 0 is always the root `#@quill` block.
    let frontmatter_block = &blocks[0];
    let quill_tag = frontmatter_block.quill_ref.clone().ok_or_else(|| {
        ParseError::MissingQuillField(
            "The document's first card-yaml block must declare `#@quill: <name>`.".to_string(),
        )
    })?;

    // Build the root block's frontmatter item list.
    let frontmatter = build_frontmatter_from_pre_and_parsed(
        &frontmatter_block.pre_items,
        &frontmatter_block.pre_nested_comments,
        &frontmatter_block.yaml_value,
    )?;
    // Surface pre-scan warnings (nested-comment drops, unsupported tags).
    let mut warnings = warnings;
    for w in &frontmatter_block.pre_warnings {
        warnings.push(w.clone());
    }

    // Global body: between the end of block 0 and the start of the first
    // composable card block (or EOF). When a block follows, the slice ends
    // with the blank-line separator — strip it so stored bodies contain only
    // authored content.
    let body_start = blocks[0].end;
    let first_card_block = blocks.iter().skip(1).find(|b| b.tag.is_some());
    let (body_end, body_is_followed_by_fence) = match first_card_block {
        Some(b) => (b.start, true),
        None => (markdown.len(), false),
    };
    let global_body_raw = &markdown[body_start..body_end];
    let global_body = if body_is_followed_by_fence {
        strip_blank_separator(global_body_raw).to_string()
    } else {
        global_body_raw.to_string()
    };

    // Parse composable card blocks into typed Cards.
    let mut cards: Vec<Card> = Vec::new();
    for (idx, block) in blocks.iter().enumerate() {
        if let Some(ref tag_name) = block.tag {
            let card_frontmatter = build_frontmatter_from_pre_and_parsed(
                &block.pre_items,
                &block.pre_nested_comments,
                &block.yaml_value,
            )
            .map_err(|e| match e {
                ParseError::InvalidStructure(msg) => ParseError::InvalidStructure(format!(
                    "Invalid YAML in card block '{}': {}",
                    tag_name, msg
                )),
                other => other,
            })?;
            for w in &block.pre_warnings {
                warnings.push(w.clone());
            }

            // Card body: between this block's end and the next block's start.
            let card_body_start = block.end;
            let has_next_block = idx + 1 < blocks.len();
            let card_body_end = if has_next_block {
                blocks[idx + 1].start
            } else {
                markdown.len()
            };
            let card_body_raw = &markdown[card_body_start..card_body_end];
            let card_body = if has_next_block {
                strip_blank_separator(card_body_raw).to_string()
            } else {
                card_body_raw.to_string()
            };

            cards.push(Card::new_with_sentinel(
                Sentinel::Card(tag_name.clone()),
                card_frontmatter,
                card_body,
            ));
        }
    }

    let quill_ref = QuillReference::from_str(&quill_tag).map_err(|e| {
        ParseError::InvalidStructure(format!("Invalid #@quill reference '{}': {}", quill_tag, e))
    })?;

    let main = Card::new_with_sentinel(Sentinel::Main(quill_ref), frontmatter, global_body);
    let doc = Document::from_main_and_cards(main, cards, warnings.clone());

    Ok((doc, warnings))
}

/// Build a [`Frontmatter`] from the pre-scan items and the parsed YAML
/// mapping.
///
/// The pre-scan defined source order for fields and comments; the parsed
/// YAML defined the typed value for each key. We walk pre-scan order, pulling
/// each field's value from `parsed`. Any field the pre-scan didn't catch is
/// appended at the end in parsed-map order so we never drop values.
fn build_frontmatter_from_pre_and_parsed(
    pre_items: &[PreItem],
    pre_nested_comments: &[NestedComment],
    yaml_value: &Option<serde_json::Value>,
) -> Result<Frontmatter, ParseError> {
    let mapping = match yaml_value {
        Some(serde_json::Value::Object(map)) => map.clone(),
        Some(serde_json::Value::Null) | None => serde_json::Map::new(),
        Some(_) => {
            return Err(ParseError::InvalidStructure(
                "expected a mapping".to_string(),
            ));
        }
    };

    let mut items: Vec<FrontmatterItem> = Vec::new();
    let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for pre in pre_items {
        match pre {
            PreItem::Comment { text, inline } => {
                items.push(FrontmatterItem::Comment {
                    text: text.clone(),
                    inline: *inline,
                });
            }
            PreItem::Field { key, fill } => {
                if let Some(value) = mapping.get(key).cloned() {
                    // `!fill` applies to scalars and sequences. Mappings are
                    // rejected because top-level `type: object` is unsupported.
                    if *fill && value.is_object() {
                        return Err(ParseError::InvalidStructure(format!(
                            "`!fill` on key `{}` targets a mapping; `!fill` is supported on scalars and sequences only",
                            key
                        )));
                    }
                    items.push(FrontmatterItem::Field {
                        key: key.clone(),
                        value: QuillValue::from_json(value),
                        fill: *fill,
                    });
                    consumed.insert(key.clone());
                }
            }
        }
    }

    // Append any parsed-map keys that the pre-scan didn't capture.
    for (key, value) in &mapping {
        if consumed.contains(key) {
            continue;
        }
        items.push(FrontmatterItem::Field {
            key: key.clone(),
            value: QuillValue::from_json(value.clone()),
            fill: false,
        });
    }

    Ok(Frontmatter::from_items_with_nested(
        items,
        pre_nested_comments.to_vec(),
    ))
}
