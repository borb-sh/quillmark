//! Assembly of card-yaml blocks into a [`Document`]: the top-level parsing glue
//! over the fence scanner, the YAML payload parse, and `$` metadata extraction.
//!
//! Both `$` system metadata and user fields become [`PayloadItem`] variants in
//! one source-ordered list, so a comment attaches to whichever item precedes it
//! regardless of which side of the `$` boundary that is.

use crate::error::ParseError;
use crate::value::QuillValue;
use crate::{Diagnostic, Severity};

use super::fences::{find_metadata_blocks, RootFault};
use super::meta::{extract_meta_items, meta_key};
use super::payload::{Payload, PayloadItem};
use quillmark_content::Normalized;

/// The parse-time half of the markdown→content boundary
/// ([`super::import_body`]): an over-nesting failure becomes a [`ParseError`].
fn import_body_or_parse_error(md: &str) -> Result<Normalized, ParseError> {
    super::import_body(md).map_err(|e| ParseError::BodyImport(e.to_string()))
}
use super::prescan::{prescan_fence_content, CommentPathSegment, NestedComment, PreItem, PreScan};
use super::{Card, Document};

/// A `MissingQuill` message naming the specific malformation. LLM authors hit a
/// few recurring shapes — a bare YAML mapping with no fences, an opener whose
/// closer is missing or misspelt — where naming the concrete edit converges
/// faster than generic advice. `root_fault` is what the fence scanner saw at the
/// root position, so a document that *does* open correctly is never told to open
/// correctly.
fn missing_block_message(markdown: &str, root_fault: Option<&RootFault>) -> String {
    match root_fault {
        Some(RootFault::Unclosed {
            opener_line,
            near_closer,
            last_field,
        }) => {
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
            msg.push_str(" Add a line containing exactly `~~~` (three tildes, no info string) ");
            match last_field {
                Some(key) => msg.push_str(&format!("after the last field (`{}`), ", key)),
                None => msg.push_str("after the last field, "),
            }
            msg.push_str("before the prose body.");
            return msg;
        }
        Some(RootFault::InfoString { opener_line, info }) => {
            return format!(
                "Root card-yaml block opener at line {} is `~~~{}`, which opens an ordinary \
                 code block. The card-yaml opener carries no info string: drop `{}` and open \
                 with a bare `~~~` (the `card-yaml` and `yaml` info strings are also accepted).",
                opener_line + 1,
                info,
                info
            );
        }
        None => {}
    }

    let trimmed = markdown.trim_start();

    if trimmed.starts_with("$quill:") || trimmed.starts_with("quill:") {
        return "Missing required root card-yaml block. Your document starts with \
                YAML metadata but is missing the `~~~` fence. Wrap the \
                metadata: add a line `~~~` above the `$quill:` line and a \
                line containing exactly `~~~` (three tildes, no info string) below \
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
/// EOF when it is the last one.
fn body_after(markdown: &str, blocks: &[MetadataBlock], idx: usize) -> String {
    let start = blocks[idx].end;
    match blocks.get(idx + 1) {
        Some(next) => strip_blank_separator(&markdown[start..next.start]).to_string(),
        None => markdown[start..].to_string(),
    }
}

/// An intermediate representation of one `~~~ … ~~~` card-yaml block.
#[derive(Debug)]
pub(super) struct MetadataBlock {
    pub(super) start: usize, // Position of the opening `~~~`
    pub(super) end: usize,   // Position after the closing `~~~`
    pub(super) yaml_value: Option<serde_json::Value>, // Parsed YAML payload as JSON
    /// Typed `$` system-metadata payload items in source order.
    pub(super) meta_items: Vec<PayloadItem>,
    /// Pre-scan items (comments + fill-tagged field keys) in source order.
    pub(super) pre_items: Vec<PreItem>,
    /// Pre-scan nested comments (with structural paths).
    pub(super) pre_nested_comments: Vec<NestedComment>,
    /// Pre-scan nested `!must_fill` paths (rooted at the owning top-level key).
    pub(super) pre_nested_fills: Vec<Vec<CommentPathSegment>>,
    /// Pre-scan warnings (unknown-tag strips, ...).
    pub(super) pre_warnings: Vec<Diagnostic>,
}

/// The document-absolute, 1-indexed position of a YAML parse failure.
///
/// `parser` is the position the engine reports inside the string it parsed:
/// the fence content less the comment lines prescan drops
/// ([`PreScan::source_lines`] maps what survives back) and less the leading
/// whitespace `trim` removes. With no reported position the block's first
/// content line is the anchor.
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

    let cleaned_index = trimmed_prefix.matches('\n').count() + rel_line.saturating_sub(1);
    let source_index = pre
        .source_lines
        .get(cleaned_index)
        .or_else(|| pre.source_lines.last())
        .copied()
        .unwrap_or(0);

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

    if let Some(err) = pre.fill_target_errors.first() {
        return Err(ParseError::InvalidStructure(err.clone()));
    }

    // `!must_fill` is not permitted on `$` metadata keys: those are extracted into
    // typed values and have no placeholder semantics.
    for item in &pre.items {
        if let PreItem::Field { key, fill: true } = item {
            if key.starts_with('$') {
                return Err(ParseError::InvalidStructure(format!(
                    "`!must_fill` on `{}` is not permitted: system-metadata keys \
                     cannot be placeholders",
                    key
                )));
            }
        }
    }

    pre.warnings
        .extend(meta_rooted_fill_warnings(&pre.nested_fills));

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
                return Err(ParseError::YamlErrorWithLocation {
                    message: enriched.message,
                    line,
                    column,
                    block_index,
                    hint: enriched.hint,
                });
            }
        };
        let meta = extract_meta_items(&mut parsed)?;
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
        yaml_value,
        meta_items,
        pre_items: pre.items,
        pre_nested_comments: pre.nested_comments,
        pre_nested_fills: pre.nested_fills,
        pre_warnings: pre.warnings,
    })
}

/// Test-only convenience over [`decompose_with_warnings`]; the shipping entry
/// [`super::Document::parse`] keeps the warnings.
#[cfg(test)]
pub(super) fn decompose(markdown: &str) -> Result<Document, crate::error::ParseError> {
    decompose_with_warnings(markdown).map(|(doc, _)| doc)
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
    let mut blocks = scan.blocks;
    let warnings = scan.warnings;

    if blocks.is_empty() {
        return Err(crate::error::ParseError::MissingQuill(
            missing_block_message(markdown, scan.root_fault.as_ref()),
        ));
    }

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
        std::mem::take(&mut root_block.pre_nested_fills),
        root_block.yaml_value.take(),
    )?;
    if main_payload.kind().is_none() {
        main_payload.set_kind("main");
    }
    let mut warnings = warnings;
    for w in &blocks[0].pre_warnings {
        warnings.push(w.clone());
    }

    let global_body = body_after(markdown, &blocks, 0);

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
            .any(|m| matches!(m, PayloadItem::Meta { key, .. } if key.is_root_only()))
        {
            return Err(ParseError::InvalidStructure(
                "A composable card-yaml block must not carry `$seed`: only the \
                 document's root block carries seeding overlays."
                    .to_string(),
            ));
        }

        let block = &mut blocks[idx];
        let card_payload = build_payload(
            std::mem::take(&mut block.meta_items),
            std::mem::take(&mut block.pre_items),
            std::mem::take(&mut block.pre_nested_comments),
            std::mem::take(&mut block.pre_nested_fills),
            block.yaml_value.take(),
        )
        .map_err(|e| match e {
            ParseError::InvalidStructure(msg) => {
                ParseError::InvalidStructure(format!("Invalid YAML in card block: {}", msg))
            }
            other => other,
        })?;
        for w in &blocks[idx].pre_warnings {
            warnings.push(w.clone());
        }

        let card_body = body_after(markdown, &blocks, idx);

        cards.push(Card::from_parts(
            card_payload,
            import_body_or_parse_error(&card_body)?,
        ));
    }

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

fn build_payload(
    meta_items: Vec<PayloadItem>,
    pre_items: Vec<PreItem>,
    pre_nested_comments: Vec<NestedComment>,
    pre_nested_fills: Vec<Vec<CommentPathSegment>>,
    yaml_value: Option<serde_json::Value>,
) -> Result<Payload, ParseError> {
    let mut mapping = match yaml_value {
        Some(serde_json::Value::Object(map)) => map,
        Some(serde_json::Value::Null) | None => serde_json::Map::new(),
        Some(_) => {
            return Err(ParseError::InvalidStructure(
                "expected a mapping".to_string(),
            ));
        }
    };

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
            PreItem::Field { key, fill } => {
                if key.starts_with('$') {
                    if let Some(meta) = take_meta_item(&mut typed, &key) {
                        items.push(meta);
                    }
                    continue;
                }
                if let Some(value) = mapping.shift_remove(&key) {
                    if fill && value.is_object() {
                        return Err(ParseError::InvalidStructure(format!(
                            "`!must_fill` on key `{}` targets a mapping; `!must_fill` is supported on scalars and sequences only",
                            key
                        )));
                    }
                    validate_parsed_field(&key, &value)?;
                    let mut qv = QuillValue::from_json(value);
                    apply_nested_fills(&key, &mut qv, &pre_nested_fills)?;
                    items.push(PayloadItem::Field {
                        key,
                        value: qv,
                        fill,
                        nested_comments: Vec::new(),
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
        let mut qv = QuillValue::from_json(value);
        apply_nested_fills(&key, &mut qv, &pre_nested_fills)?;
        items.push(PayloadItem::Field {
            key,
            value: qv,
            fill: false,
            nested_comments: Vec::new(),
        });
    }

    Ok(Payload::from_items_with_flat_nested(
        items,
        pre_nested_comments,
    ))
}

/// Apply the nested `!must_fill` markers rooted at `key` onto `value`'s tree.
/// Paths are rooted at the owning top-level key, so the first segment is
/// stripped. A path that is nothing but that key names the root, whose marker
/// the item's own `fill` flag carries. A marker on a mapping node is rejected,
/// as at the top level.
fn apply_nested_fills(
    key: &str,
    value: &mut QuillValue,
    pre_nested_fills: &[Vec<CommentPathSegment>],
) -> Result<(), ParseError> {
    for path in pre_nested_fills {
        let Some((CommentPathSegment::Key(first), rest)) = path.split_first() else {
            continue;
        };
        if first != key || rest.is_empty() {
            continue;
        }
        if value.is_object_at(rest) {
            return Err(ParseError::InvalidStructure(format!(
                "`!must_fill` on `{}` targets a mapping; `!must_fill` is supported on scalars and sequences only",
                render_path(path)
            )));
        }
        // The path came from prescan over the same source, so a miss means
        // prescan and the YAML parser disagreed on structure.
        let applied = value.set_fill_at(rest);
        debug_assert!(
            applied,
            "prescan recorded a nested fill path that did not resolve against \
             the parsed value: `{}`",
            render_path(path)
        );
    }
    Ok(())
}

/// One warning per nested `!must_fill` path rooted at a `$` metadata key. A
/// `PayloadItem::Meta` value is a plain tree with no fill carrier, so the marker
/// reaches neither storage nor emit; the value under it survives, as in every
/// other unpreservable marker position.
fn meta_rooted_fill_warnings(nested_fills: &[Vec<CommentPathSegment>]) -> Vec<Diagnostic> {
    nested_fills
        .iter()
        .filter(|path| {
            matches!(path.first(), Some(CommentPathSegment::Key(k)) if k.starts_with('$'))
        })
        .map(|path| {
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "a `!must_fill` marker at `{}` is inside `$` system metadata and is \
                     not preserved; system-metadata values carry no placeholder markers",
                    render_path(path)
                ),
            )
            .with_code("parse::fill_marker_unsupported_position".to_string())
        })
        .collect()
}

/// Render a structural path as a dotted/bracketed string for diagnostics,
/// e.g. `addr.street` or `recipients[0].name`.
fn render_path(path: &[CommentPathSegment]) -> String {
    path.iter()
        .fold(crate::path::DocPath::new(), |p, seg| p.segment(seg))
        .to_string()
}
