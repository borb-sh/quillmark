//! Card-yaml system-sentinel (`#@quill:` / `#@kind:`) parsing and
//! reserved-name validation.
//!
//! Every `~~~card-yaml` block declares a system sentinel on its first
//! non-blank line: the root block declares `#@quill: <name>`, every
//! composable block declares `#@kind: <type>`. The `#@` prefix keeps the
//! sentinel out of the YAML payload (a bare `#` line is a YAML comment).

use crate::error::ParseError;

/// The reserved card kind that identifies the document's root block. Every
/// block declares `#@kind:`; the root block declares `#@kind: main`, and a
/// composable card may not use this kind.
pub(super) const MAIN_KIND: &str = "main";

/// Validate tag name follows pattern [a-z_][a-z0-9_]*
pub(super) fn is_valid_tag_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();
    let first = chars.next().unwrap();

    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }

    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }

    true
}

/// Parse a `#@`-prefixed system-sentinel line into its `(key, value)` pair.
///
/// `#@quill: example@0.1.0` parses to `("quill", "example@0.1.0")`;
/// `#@kind: endorsement` parses to `("kind", "endorsement")`. Surrounding
/// whitespace is trimmed from both halves. Returns `None` when `line` is not
/// a `#@` sentinel line (no `#@` prefix or no `:` separator).
pub(super) fn parse_system_sentinel(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("#@")?;
    let colon = rest.find(':')?;
    let key = rest[..colon].trim().to_string();
    let value = rest[colon + 1..].trim().to_string();
    Some((key, value))
}

/// Validate the YAML payload of a `~~~card-yaml` block.
///
/// The system sentinel lives in the `#@` line, never in the YAML body, so the
/// payload carries only user fields. This check rejects the reserved
/// wire-format keys (`QUILL`, `CARD`, `BODY`, `CARDS`) appearing as
/// user-defined fields — they would collide with [`crate::Document::to_plate_json`]'s
/// output. The parsed value is returned unchanged.
pub(super) fn validate_payload_yaml(
    parsed: serde_json::Value,
) -> Result<Option<serde_json::Value>, ParseError> {
    if let Some(mapping) = parsed.as_object() {
        for reserved in ["QUILL", "CARD", "BODY", "CARDS"] {
            if mapping.contains_key(reserved) {
                return Err(ParseError::InvalidStructure(format!(
                    "Reserved field name '{}' cannot be used in a card-yaml block",
                    reserved
                )));
            }
        }
    }
    Ok(Some(parsed))
}
