//! Assembly of card-yaml blocks into a [`Document`]: the top-level parsing glue
//! over the fence scanner, the YAML payload parse, and `$` metadata extraction.
//!
//! Both `$` system metadata and user fields become [`PayloadItem`] variants in
//! one source-ordered list, so a comment attaches to whichever item precedes it
//! regardless of which side of the `$` boundary that is.

use crate::error::ParseError;
use crate::value::{PathSegment, QuillValue};
use crate::error::{Diagnostic, Severity};

use super::fences::{find_metadata_blocks, UnclosedRoot};
use super::meta::{extract_meta_items, meta_key, yaml_type_name};
use super::payload::{MetaKey, Payload, PayloadItem};
use quillmark_content::model::Normalized;

/// The parse-time half of the markdown→content boundary
/// ([`super::import_body`]): an over-nesting failure becomes a [`ParseError`].
fn import_body_or_parse_error(md: &str) -> Result<Normalized, ParseError> {
    super::import_body(md).map_err(|e| ParseError::BodyImport(e.to_string()))
}
use super::prescan::{prescan_fence_content, NestedComment, PreItem, PreScan};
use super::{Card, Document};

/// A `MissingQuill` message naming the specific malformation. LLM authors hit a
/// few recurring shapes — a bare YAML mapping with no fences, an opener whose
/// closer is missing or misspelt — where naming the concrete edit converges
/// faster than generic advice. `unclosed_root` is what the fence scanner saw at
/// the root position, so a document that *does* open correctly is never told to
/// open correctly.
fn missing_block_message(markdown: &str, unclosed_root: Option<&UnclosedRoot>) -> String {
    if let Some(UnclosedRoot {
        opener_line,
        near_closer,
        last_field,
    }) = unclosed_root
    {
        let mut msg = format!(
            "Root card-yaml block opened at line {} is never closed.",
            opener_line + 1
        );
        if let Some((line, text)) = near_closer {
            msg.push_str(&format!(
                " The line `{}` at line {} does not close it: a closing fence is at \
                 column zero and at least as long as the opener.",
                text,
                line + 1
            ));
        }
        msg.push_str(" Add a line containing exactly `~~~` (three tildes) ");
        match last_field {
            Some(key) => msg.push_str(&format!("after the last field (`{}`), ", key)),
            None => msg.push_str("after the last field, "),
        }
        msg.push_str("before the prose body.");
        return msg;
    }

    let trimmed = markdown.trim_start();

    if trimmed.starts_with("---")
        && trimmed
            .lines()
            .any(|l| l.trim_start().starts_with("$quill:"))
    {
        return "Missing required root card-yaml block. Your document opens with \
                `---` YAML front matter; card-yaml blocks are fenced with `~~~`. \
                Replace the opening `---` and its closing `---` with a line \
                containing exactly `~~~` (three tildes)."
            .to_string();
    }

    if trimmed.starts_with("$quill:") || trimmed.starts_with("quill:") {
        return "Missing required root card-yaml block. Your document starts with \
                YAML metadata but is missing the `~~~` fence. Wrap the \
                metadata: add a line `~~~` above the `$quill:` line and a \
                line containing exactly `~~~` (three tildes) below \
                the last metadata field, before the prose body."
            .to_string();
    }

    "Missing required root card-yaml block. The document must open with a \
     `~~~` block declaring `$quill: <name>` (and `$kind: main`) as \
     the first two lines, closed by a line containing exactly `~~~`."
        .to_string()
}

/// Strip exactly one structural separator from the tail of a body slice.
///
/// A body followed by another block ends with that block's required blank line,
/// exactly one `\n` or `\r\n`, so stripping it leaves only authored content.
/// The emitter re-adds it via `ensure_blank_before_fence`.
fn strip_blank_separator(body: &str) -> &str {
    if let Some(rest) = body.strip_suffix("\r\n") {
        rest
    } else if let Some(rest) = body.strip_suffix('\n') {
        rest
    } else {
        body
    }
}

/// The prose body following `blocks[idx]`: up to the next block's opener, or to
/// EOF when it is the last one, with each demoted block in that span replaced
/// by its code fence.
fn body_after(
    markdown: &str,
    blocks: &[MetadataBlock],
    demoted: &[Demoted],
    idx: usize,
) -> String {
    let start = blocks[idx].end;
    let end = blocks.get(idx + 1).map_or(markdown.len(), |next| next.start);
    let mut body = String::new();
    let mut cursor = start;
    for d in demoted.iter().filter(|d| d.start >= start && d.end <= end) {
        body.push_str(&markdown[cursor..d.start]);
        body.push_str(&d.code);
        cursor = d.end;
    }
    body.push_str(&markdown[cursor..end]);
    if idx + 1 < blocks.len() {
        body.truncate(strip_blank_separator(&body).len());
    }
    body
}

/// A block after the root that names no `$kind`: no schema can claim it, so it
/// stays in the body above as code.
struct Demoted {
    start: usize,
    end: usize,
    /// The block as a backtick fence. CommonMark closes a tilde fence on a
    /// closer indented up to three spaces, which a YAML block scalar holds, so
    /// the tilde form could end early; a backtick run longer than any inside
    /// the payload closes exactly where the card scanner did.
    code: String,
    warning: Diagnostic,
}

impl Demoted {
    fn new(markdown: &str, block: &MetadataBlock) -> Self {
        let content = &markdown[block.content_start..block.content_end];
        let longest = content
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat((longest + 1).max(3));
        let info = opener_info(markdown, block.start).filter(|info| !info.contains('`'));
        let newline = if markdown[..block.end].ends_with('\n') { "\n" } else { "" };
        Demoted {
            start: block.start,
            end: block.end,
            code: format!("{fence}{}\n{content}{fence}{newline}", info.unwrap_or("")),
            warning: missing_kind_warning(line_of(markdown, block.start), &block.meta_items),
        }
    }
}

/// An intermediate representation of one `~~~ … ~~~` card-yaml block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize, // Position of the opening `~~~`
    pub(super) end: usize,   // Position after the closing `~~~`
    /// The payload between the fences: from the line after the opener to the
    /// closer's line.
    pub(super) content_start: usize,
    pub(super) content_end: usize,
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML payload as JSON
    /// Typed `$` system-metadata payload items in source order.
    pub(super) meta_items: Vec<PayloadItem>,
    /// Pre-scan items (comments + field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// The document-absolute, 1-indexed position of a YAML parse failure.
///
/// `parser` is the position the engine reports inside the string it parsed: the
/// fence content, line-for-line, less the leading whitespace `trim` removes.
/// With no reported position the block's first content line is the anchor.
fn document_position(
    markdown: &str,
    content_start: usize,
    pre: &PreScan,
    parser: Option<(usize, usize)>,
) -> (usize, usize) {
    let first_line = markdown[..content_start].lines().count() + 1;
    let Some((rel_line, rel_column)) = parser else {
        return (first_line, 1);
    };

    let cleaned = &pre.cleaned_yaml;
    let trimmed_prefix = &cleaned[..cleaned.len() - cleaned.trim_start().len()];
    let source_index = trimmed_prefix.matches('\n').count() + rel_line.saturating_sub(1);

    // Only the first parsed line lost leading whitespace to `trim`.
    let column = if rel_line <= 1 {
        let head = trimmed_prefix.rsplit('\n').next().unwrap_or("");
        rel_column + head.chars().count()
    } else {
        rel_column
    };

    (first_line + source_index, column)
}

/// Process one recognised card-yaml block into a [`MetadataBlock`].
///
/// `block_start` / `block_end` bound the whole block; `content_start` /
/// `content_end` the content between the fences. `block_index` is used only for
/// YAML-error location context.
pub(super) fn build_block(
    markdown: &str,
    block_start: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
    block_index: usize,
) -> Result<MetadataBlock, ParseError> {
    let raw_content = &markdown[content_start..content_end];

    if raw_content.len() > crate::error::MAX_YAML_SIZE {
        return Err(ParseError::InputTooLarge {
            size: raw_content.len(),
            max: crate::error::MAX_YAML_SIZE,
        });
    }

    let mut pre = prescan_fence_content(raw_content);

    let content = pre.cleaned_yaml.trim().to_string();
    let (meta_items, yaml_value) = if content.is_empty() {
        (Vec::new(), None)
    } else {
        let mut parsed = match serde_saphyr::from_str::<serde_json::Value>(&content) {
            Ok(parsed) => parsed,
            Err(e) => {
                let enriched = super::yaml_hints::enrich_yaml_error(&e.to_string(), &content);
                let (line, column) = document_position(
                    markdown,
                    content_start,
                    &pre,
                    e.location()
                        .map(|l| (l.line() as usize, l.column() as usize)),
                );
                // Below the root, tilde-fenced code is the usual source.
                let hint = match (block_index, enriched.hint) {
                    (0, hint) => hint,
                    (_, Some(hint)) => Some(format!("{hint} {TILDE_CODE_HINT}")),
                    (_, None) => Some(TILDE_CODE_HINT.to_string()),
                };
                return Err(ParseError::YamlErrorWithLocation {
                    message: enriched.message,
                    line,
                    column,
                    block_index,
                    hint,
                });
            }
        };
        let meta = extract_meta_items(&mut parsed)?;
        drop_retired_fills(&mut parsed, &pre.retired_fills, &mut pre.warnings);
        (meta, Some(parsed))
    };

    // Field-count check (spec §8), after `$`-key extraction so the bound is on
    // user-data fields.
    if let Some(serde_json::Value::Object(ref map)) = yaml_value {
        if map.len() > crate::error::MAX_FIELD_COUNT {
            return Err(ParseError::TooManyFields {
                count: map.len(),
                max: crate::error::MAX_FIELD_COUNT,
            });
        }
    }

    Ok(MetadataBlock {
        start: block_start,
        end: block_end,
        content_start,
        content_end,
        yaml_value,
        meta_items,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_warnings: pre.warnings,
    })
}

/// Decompose markdown into a typed [`Document`], returning any non-fatal warnings
/// collected during fence scanning.
pub(super) fn decompose_with_warnings(
    markdown: &str,
) -> Result<(Document, Vec<Diagnostic>), crate::error::ParseError> {
    let markdown = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);

    if markdown.trim().is_empty() {
        return Err(crate::error::ParseError::EmptyInput(
            "Empty markdown input cannot be parsed as a Quillmark Document. \
             Provide at least a root card-yaml block declaring `$quill: <name>`."
                .to_string(),
        ));
    }

    if markdown.len() > crate::error::MAX_INPUT_SIZE {
        return Err(crate::error::ParseError::InputTooLarge {
            size: markdown.len(),
            max: crate::error::MAX_INPUT_SIZE,
        });
    }

    // The first block is the document root; the rest are composable cards.
    let scan = find_metadata_blocks(markdown)?;
    let mut warnings = scan.warnings;

    if scan.blocks.is_empty() {
        return Err(crate::error::ParseError::MissingQuill(
            missing_block_message(markdown, scan.unclosed_root.as_ref()),
        ));
    }

    // A block after the root that names no `$kind` is no card: no schema can
    // claim it. It stays in the body above as the fenced code block CommonMark
    // reads it as, which `body_after` spans once the block leaves the list.
    let mut blocks: Vec<MetadataBlock> = Vec::with_capacity(scan.blocks.len());
    let mut demoted: Vec<Demoted> = Vec::new();
    for (idx, block) in scan.blocks.into_iter().enumerate() {
        let kinded = block
            .meta_items
            .iter()
            .any(|m| matches!(m, PayloadItem::Kind { .. }));
        if idx == 0 || kinded {
            blocks.push(block);
        } else {
            demoted.push(Demoted::new(markdown, &block));
        }
    }

    // Composable cards are every block after the root that stays a card
    // (spec §8).
    let card_count = blocks.len() - 1;
    if card_count > crate::error::MAX_CARD_COUNT {
        return Err(ParseError::TooManyCards {
            count: card_count,
            max: crate::error::MAX_CARD_COUNT,
        });
    }

    // Warnings keep document order: a demoted block's lands before those of
    // the first card below it.
    let mut pending = demoted.iter().map(|d| (d.start, &d.warning)).peekable();
    let mut drain_before = |pos: usize, warnings: &mut Vec<Diagnostic>| {
        while let Some((_, w)) = pending.next_if(|(start, _)| *start < pos) {
            warnings.push(w.clone());
        }
    };

    let root_mapping = payload_mapping(markdown, &mut blocks[0])?;
    let root_block = &blocks[0];
    let has_root_quill = root_block
        .meta_items
        .iter()
        .any(|m| matches!(m, PayloadItem::Quill { .. }));
    if !has_root_quill {
        return Err(ParseError::MissingQuill(
            "The document's root card-yaml block must declare `$quill: <name>` as \
             its first line (e.g. `$quill: usaf_memo@0.2.0`)."
                .to_string(),
        ));
    }

    // The root's `$kind` is `main` by position (markdown-spec.md §3.3): an
    // omitted one is synthesised below; any other value is a parse error.
    let root_kind = root_block.meta_items.iter().find_map(|m| match m {
        PayloadItem::Kind { value } => Some(value.as_str()),
        _ => None,
    });
    if let Some(other) = root_kind {
        if other != "main" {
            return Err(ParseError::InvalidStructure(format!(
                "The document's root card-yaml block has `$kind: {}`, but \
                 `main` is reserved for the document root. Remove the line \
                 (the root's kind is `main` by position) or change it to \
                 `$kind: main`.",
                other
            )));
        }
    }

    // `set_kind` inserts at the canonical position, so canonical input
    // round-trips byte-equal and non-canonical input converges on first emit
    // (markdown-spec.md §9).
    let root_block = &mut blocks[0];
    let mut main_payload = build_payload(
        std::mem::take(&mut root_block.meta_items),
        std::mem::take(&mut root_block.pre_items),
        std::mem::take(&mut root_block.pre_nested_comments),
        root_mapping,
    )?;
    if main_payload.kind().is_none() {
        main_payload.set_kind("main");
    }
    for w in &blocks[0].pre_warnings {
        warnings.push(w.clone());
    }

    let global_body = body_after(markdown, &blocks, &demoted, 0);

    let main = Card::from_parts(main_payload, import_body_or_parse_error(&global_body)?);

    let mut cards: Vec<Card> = Vec::new();
    for idx in 1..blocks.len() {
        let block = &blocks[idx];

        // Only the root block binds the document to a quill.
        if block
            .meta_items
            .iter()
            .any(|m| matches!(m, PayloadItem::Quill { .. }))
        {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not declare `$quill`: only \
                 the document's root block binds the document to a quill."
                    .to_string(),
            ));
        }

        // `main` is reserved for the document root.
        let kind_is_main = block.meta_items.iter().any(|m| match m {
            PayloadItem::Kind { value } => value == "main",
            _ => false,
        });
        if kind_is_main {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not declare `$kind: main`: \
                 `main` is reserved for the document root."
                    .to_string(),
            ));
        }

        // Seeding overlays live on the document root only (like `$quill`).
        if block
            .meta_items
            .iter()
            .any(|m| matches!(m, PayloadItem::Meta { key: MetaKey::Seed, .. }))
        {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not carry `$seed`: only the \
                 document's root block carries seeding overlays."
                    .to_string(),
            ));
        }

        let block = &mut blocks[idx];
        let card_mapping = payload_mapping(markdown, block)?;
        let card_payload = build_payload(
            std::mem::take(&mut block.meta_items),
            std::mem::take(&mut block.pre_items),
            std::mem::take(&mut block.pre_nested_comments),
            card_mapping,
        )
        .map_err(|e| match e {
            ParseError::InvalidStructure(msg) => {
                ParseError::InvalidStructure(format!("Invalid YAML in card block: {}", msg))
            }
            other => other,
        })?;
        drain_before(blocks[idx].start, &mut warnings);
        for w in &blocks[idx].pre_warnings {
            warnings.push(w.clone());
        }

        let card_body = body_after(markdown, &blocks, &demoted, idx);

        cards.push(Card::from_parts(
            card_payload,
            import_body_or_parse_error(&card_body)?,
        ));
    }

    drain_before(usize::MAX, &mut warnings);

    let doc = Document::from_main_and_cards(main, cards);

    Ok((doc, warnings))
}

/// Validate a user field entering the payload from the parse path, mapping a
/// violation to `ParseError::InvalidStructure` (spec §10).
fn validate_parsed_field(key: &str, value: &serde_json::Value) -> Result<(), ParseError> {
    crate::document::edit::validate_field(key, value)
        .map_err(|v| ParseError::InvalidStructure(v.message(key)))
}

/// Take the typed `$` item for `key` out of `typed`, leaving its slot empty.
fn take_meta_item(typed: &mut [Option<PayloadItem>], key: &str) -> Option<PayloadItem> {
    typed
        .iter_mut()
        .find(|slot| slot.as_ref().and_then(meta_key) == Some(key))?
        .take()
}

/// The block's payload as a mapping, an empty or null payload reading as an
/// empty one. Anything else is refused at the opener's line: in practice the
/// root, since a card's `$kind` makes its payload a mapping.
fn payload_mapping(
    markdown: &str,
    block: &mut MetadataBlock,
) -> Result<serde_json::Map<String, serde_json::Value>, ParseError> {
    match block.yaml_value.take() {
        Some(serde_json::Value::Object(map)) => Ok(map),
        Some(serde_json::Value::Null) | None => Ok(serde_json::Map::new()),
        Some(other) => Err(ParseError::PayloadNotMapping {
            line: line_of(markdown, block.start),
            info: opener_info(markdown, block.start).map(str::to_string),
            actual: yaml_type_name(&other),
        }),
    }
}

const TILDE_CODE_HINT: &str =
    "Every column-zero `~~~` block is card YAML: fence code with backticks (```) instead.";

fn missing_kind_warning(line: usize, meta: &[PayloadItem]) -> Diagnostic {
    let hint = if meta.iter().any(|m| matches!(m, PayloadItem::Quill { .. })) {
        "Only a document's first block declares `$quill`. To make this block a card, \
         replace `$quill` with a `$kind: <kind>` line; to keep it as code, fence it with \
         backticks (```)."
    } else if meta
        .iter()
        .any(|m| matches!(m, PayloadItem::Meta { key: MetaKey::Seed, .. }))
    {
        "Only a document's first block carries `$seed`. To make this block a card, \
         replace `$seed` with a `$kind: <kind>` line; to keep it as code, fence it with \
         backticks (```)."
    } else {
        "Add a `$kind: <kind>` line to make it a card. To keep it as code, fence it with \
         backticks (```)."
    };
    Diagnostic::new(
        Severity::Warning,
        format!(
            "The `~~~` block at line {line} names no `$kind`, so it reads as a code block \
             in the body above, not a card."
        ),
    )
    .with_code("parse::missing_kind".to_string())
    .with_location(crate::error::Location::new(
        crate::error::DOCUMENT_FILE.to_string(),
        line as u32,
        1,
    ))
    .with_hint(hint.to_string())
}

/// The 1-indexed line holding byte `pos`.
fn line_of(markdown: &str, pos: usize) -> usize {
    markdown[..pos].matches('\n').count() + 1
}

/// The info string on the opening fence at `start`, when it has one.
fn opener_info(markdown: &str, start: usize) -> Option<&str> {
    let opener = markdown[start..].lines().next().unwrap_or("");
    let info = opener.trim_start_matches('~').trim();
    (!info.is_empty()).then_some(info)
}

fn build_payload(
    meta_items: Vec<PayloadItem>,
    pre_items: Vec<PreItem>,
    pre_nested_comments: Vec<NestedComment>,
    mut mapping: serde_json::Map<String, serde_json::Value>,
) -> Result<Payload, ParseError> {
    // Typed `$` items, consumed at most once each; leftovers are appended in
    // source order. The assert pins `extract_meta_items` to the closed set, so a
    // regression upstream is loud rather than a silent drop.
    let mut typed: Vec<Option<PayloadItem>> = Vec::with_capacity(meta_items.len());
    for m in meta_items {
        meta_key(&m).expect(
            "build_payload: meta_items must contain only system variants \
             ($quill/$kind/$ext/$seed); got a Field or Comment",
        );
        typed.push(Some(m));
    }

    let mut items: Vec<PayloadItem> = Vec::new();

    for item in pre_items {
        match item {
            PreItem::Comment { text, inline } => {
                items.push(PayloadItem::Comment { text, inline });
            }
            PreItem::Field { key } => {
                if key.starts_with('$') {
                    if let Some(meta) = take_meta_item(&mut typed, &key) {
                        items.push(meta);
                    }
                    continue;
                }
                if let Some(value) = mapping.shift_remove(&key) {
                    validate_parsed_field(&key, &value)?;
                    items.push(PayloadItem::Field {
                        key,
                        value: QuillValue::from_json(value),
                    });
                }
            }
        }
    }

    // Drain `$` entries the prescan didn't reach, in source order, so the
    // conversion stays total.
    items.extend(typed.into_iter().flatten());

    for (key, value) in mapping {
        validate_parsed_field(&key, &value)?;
        items.push(PayloadItem::Field {
            key,
            value: QuillValue::from_json(value),
        });
    }

    Ok(Payload::from_items_with_nested(items, pre_nested_comments))
}

/// Null each node the retired `!must_fill` tag marked in the user fields of
/// `parsed`, which the `$` keys have already left. The value under the tag was
/// a placeholder, so the field reads as unanswered. A tag inside `$` metadata
/// is dropped and its value kept, as any other custom tag's.
fn drop_retired_fills(
    parsed: &mut serde_json::Value,
    paths: &[Vec<PathSegment>],
    warnings: &mut Vec<Diagnostic>,
) {
    for path in paths {
        let in_meta = matches!(path.first(), Some(PathSegment::Key(k)) if k.starts_with('$'));
        let message = if in_meta {
            format!(
                "YAML tag on `{}` is not supported; the tag has been dropped and the value kept",
                render_path(path)
            )
        } else {
            crate::value::null_at(parsed, path);
            format!(
                "`!must_fill` on `{}` is retired; the placeholder and any value under it \
                 have been dropped, leaving the field unanswered",
                render_path(path)
            )
        };
        warnings.push(
            Diagnostic::new(Severity::Warning, message)
                .with_code("parse::unsupported_yaml_tag".to_string()),
        );
    }
}

/// Render a structural path as a dotted/bracketed string for diagnostics,
/// e.g. `addr.street` or `recipients[0].name`.
fn render_path(path: &[PathSegment]) -> String {
    path.iter()
        .fold(crate::path::DocPath::new(), |p, seg| p.segment(seg))
        .to_string()
}
