//! Actionable-hint enrichment for YAML parse errors.
//!
//! `serde_saphyr` reports in YAML jargon and leaks its own Rust API names into
//! advice, neither of which a caller can turn into a content edit. This module
//! post-processes a parser error string plus the offending YAML into a
//! sanitized message and an optional hint naming the concrete textual fix, both
//! carried on the resulting [`crate::error::Diagnostic`] so every binding
//! surfaces the same advice.

/// Output of [`enrich_yaml_error`]: a cleaned message plus an optional hint.
#[derive(Debug, Clone)]
pub(crate) struct EnrichedYamlError {
    /// Parser message with Rust API names stripped and prose normalized.
    pub message: String,
    /// Actionable hint suggesting the concrete fix, when one is recognized.
    pub hint: Option<String>,
}

/// Inspect a serde_saphyr error string against the YAML content it came from
/// and return an [`EnrichedYamlError`].
///
/// The content slice is the YAML payload of a single `~~~` card-yaml block
/// (the same string passed to the parser). The function never panics on
/// non-UTF8 byte offsets: all inspection is over `&str` / `chars()`.
pub(crate) fn enrich_yaml_error(raw: &str, content: &str) -> EnrichedYamlError {
    let sanitized = sanitize_message(raw);
    let hint = derive_hint(&sanitized, content);
    EnrichedYamlError {
        message: sanitized,
        hint,
    }
}

/// Strip Rust-API-name leakage (`from_multiple`, `DuplicateKeyPolicy`, …) from
/// the parser message: those identifiers mean nothing to a non-Rust caller.
fn sanitize_message(raw: &str) -> String {
    // The leading `;` / `,` goes too, so no trailing separator is left.
    const STRIPS: &[&str] = &[
        "; use from_multiple_with_options",
        "; use from_multiple or from_multiple_with_options",
        ", use from_multiple_with_options",
        ", use from_multiple or from_multiple_with_options",
        "; set DuplicateKeyPolicy in Options if acceptable",
        ", set DuplicateKeyPolicy in Options if acceptable",
        " use from_multiple or from_multiple_with_options",
        " use from_multiple_with_options",
        " set DuplicateKeyPolicy in Options if acceptable",
    ];

    let mut out = raw.to_string();
    for p in STRIPS {
        if let Some(idx) = out.find(p) {
            out.replace_range(idx..idx + p.len(), "");
        }
    }
    out = out.replace(" ; .", ".").replace(" , .", ".");
    out.trim_end_matches([',', ';', ' ']).to_string()
}

/// Derive an actionable hint for `message`, given the YAML `content`.
fn derive_hint(message: &str, content: &str) -> Option<String> {
    let m = message.to_ascii_lowercase();

    // A plain scalar starting with `*` or `&` reads as a YAML alias or anchor.
    // LLMs writing `field: **bold**` trip this.
    if m.contains("alias references unknown anchor")
        || m.contains("anchor") && m.contains("not found")
    {
        return Some(anchor_alias_hint(
            content,
            "For markdown emphasis or a literal `*`/`&`, wrap the value in single quotes",
            "**bold text**",
        ));
    }

    // An unquoted value containing `:` reads as a nested mapping key.
    if m.contains("mapping values are not allowed") {
        if let Some(hint) = indented_top_level_key_hint(message, content) {
            return Some(hint);
        }
        if let Some((field, value)) = first_field_with_unquoted_colon(content) {
            return Some(format!(
                "Unquoted values cannot contain `:` (it starts a nested mapping key). \
                 Quote the value: `{field}: \"{value}\"`"
            ));
        }
        return Some(
            "Unquoted values cannot contain `:` (it starts a nested mapping key). \
             Wrap the value in double quotes; e.g. `field: \"value: with colon\"`."
                .to_string(),
        );
    }

    // A stray YAML document separator inside a card-yaml block.
    if m.contains("multiple yaml documents") {
        if content.lines().any(|l| l.trim_end() == "---") {
            return Some(
                "`---` is not a valid separator inside a card-yaml block (YAML \
                 reads it as a new-document marker). Close the metadata block with a \
                 line containing exactly `~~~` (three tildes) before starting the \
                 prose body."
                    .to_string(),
            );
        }
        return Some(
            "Only one YAML document is allowed per card-yaml block. Remove the \
             stray `---` separator and close the block with `~~~` before any prose."
                .to_string(),
        );
    }

    // A field declared twice in the same block.
    if m.contains("duplicate mapping key") || m.contains("duplicate key") {
        return Some(
            "Each field may appear at most once inside a card-yaml block. \
             Remove the duplicate line, or move it to a separate composable card."
                .to_string(),
        );
    }

    // A `- item` line where a mapping key was expected: mis-indented sequence,
    // or the field was meant to be a scalar.
    if m.contains("block sequence entries are not allowed") {
        return Some(
            "A `- item` list was found where a mapping key was expected. Either \
             indent the sequence two spaces under the key it belongs to \
             (`field:` newline `  - item`), or, if this field expects a single \
             scalar value, drop the `-` and put the value on the same line: \
             `field: value`."
                .to_string(),
        );
    }

    // A continuation line of a plain scalar read as a new key.
    if m.contains("simple key expected") || m.contains("simple key expect") {
        if let Some(flagged) = flagged_prose_line(&m, content) {
            return Some(format!(
                "Line {} (\"{}\") has no `key:` and reads as prose. Body text belongs \
                 after the closing `~~~`, not inside the block: close the block before it.",
                flagged.number,
                truncate_for_message(flagged.text)
            ));
        }
        return Some(
            "A second line of a value was read as a new mapping key (YAML \
             plain-scalar values stop at the next unindented line). For \
             multi-line text, use a block scalar: `field: |` then put each \
             line indented two spaces below. For a single-line value, keep it \
             on one line."
                .to_string(),
        );
    }

    // Anchor-scan failure: the alias case above under different wording
    // ("scanning an anchor or alias" rather than "anchor" + "not found").
    if m.contains("scanning an anchor") || m.contains("scanning an alias") {
        return Some(anchor_alias_hint(
            content,
            "Wrap the value in single quotes",
            "&literal value",
        ));
    }

    // A multi-line double-quoted scalar; block scalars are friendlier.
    if m.contains("invalid indentation in multiline quoted scalar")
        || (m.contains("indentation") && m.contains("quoted scalar"))
    {
        if let Some(field) = first_field_with_unterminated_dquote(content) {
            return Some(format!(
                "Multi-line text is easier to write as a block scalar:\n\
                 `{field}: |\\n  line one\\n  line two`"
            ));
        }
        return Some(
            "Multi-line text is easier to write as a block scalar: \
             `field: |` then put each line indented two spaces below."
                .to_string(),
        );
    }

    None
}

/// A line of `content` the parser flagged, with its 1-indexed number.
struct FlaggedLine<'a> {
    number: usize,
    text: &'a str,
}

/// The flagged line when it reads as body prose written inside the block rather
/// than the wrapped continuation of a plain scalar: no `key:`, sentence-shaped,
/// and not the tail of an unfinished field above it.
fn flagged_prose_line<'a>(message: &str, content: &'a str) -> Option<FlaggedLine<'a>> {
    let number = flagged_line_number(message)?;
    let lines: Vec<&str> = content.lines().collect();
    let text = lines.get(number.checked_sub(1)?)?.trim();
    if text.is_empty() || has_colon_outside_quotes(text) {
        return None;
    }

    let previous = number
        .checked_sub(2)
        .and_then(|i| lines.get(i))
        .map(|l| l.trim());
    let after_blank = matches!(previous, None | Some(""));
    if !after_blank && !text.contains(' ') && !ends_a_sentence(text) {
        return None;
    }

    // The competing reading: a plain scalar wrapped onto a second line. It needs
    // an unfinished `key: value` directly above and nothing continuing below,
    // and no sentence punctuation of its own.
    let wrapped_scalar = !after_blank
        && previous.is_some_and(looks_unterminated_scalar)
        && !ends_a_sentence(text)
        && !text.contains(',')
        && lines.iter().rposition(|l| !l.trim().is_empty()) == Some(number - 1);

    (!wrapped_scalar).then_some(FlaggedLine { number, text })
}

/// The 1-indexed line the parser named, read off the `line N column M` prefix
/// of its message (`at line N, column M` is accepted too).
fn flagged_line_number(message: &str) -> Option<usize> {
    let (_, rest) = message.split_once("line ")?;
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Whether `line` carries a `:` that YAML would read as a key separator.
fn has_colon_outside_quotes(line: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    for ch in line.chars() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ':' if !in_single && !in_double => return true,
            _ => {}
        }
    }
    false
}

fn ends_a_sentence(line: &str) -> bool {
    matches!(line.trim_end().chars().last(), Some('.' | '!' | '?'))
}

/// Whether `line` is a `key: <plain multi-word value>` a reader would take as
/// running on: the shape whose next line is a scalar continuation, not a key.
fn looks_unterminated_scalar(line: &str) -> bool {
    let Some((key, value)) = line.split_once(':') else {
        return false;
    };
    if key.is_empty() || key.contains(char::is_whitespace) || key.starts_with(['#', '-']) {
        return false;
    }
    let value = value.trim();
    if !value.contains(' ') || value.starts_with(['\'', '"', '|', '>', '[', '{', '&', '*', '#']) {
        return false;
    }
    !matches!(value.chars().last(), Some('.' | '!' | '?' | ',' | ';'))
}

/// The line as quoted in a hint, elided past a readable prefix.
fn truncate_for_message(text: &str) -> String {
    const MAX: usize = 48;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX).collect();
    format!("{}…", head.trim_end())
}

/// The alias/anchor hint, naming the offending field when one is recognizable.
/// `advice` is the sentence before the example; `example` the quoted value shown.
fn anchor_alias_hint(content: &str, advice: &str, example: &str) -> String {
    match first_field_with_unquoted_prefix(content, &['*', '&']) {
        Some(field) => format!(
            "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
             alias/anchor indicators). {advice}: `{field}: '{example}'`"
        ),
        None => format!(
            "Plain-scalar values cannot start with `*` or `&` (reserved as YAML \
             alias/anchor indicators). {advice}; e.g. `field: '{example}'`."
        ),
    }
}

/// The `key: value` lines of `content` whose key could be a YAML mapping key,
/// values leading-trimmed. A comment or sequence line surfaces as a key
/// starting with `#` / `-`, which callers filter as their scan requires.
fn key_value_lines(content: &str) -> impl Iterator<Item = (&str, &str)> {
    content.lines().filter_map(|line| {
        let (key, rest) = line.trim_start().split_once(':')?;
        (!key.is_empty() && !key.contains(' ')).then(|| (key, rest.trim_start()))
    })
}

/// The first `key: <scalar>` line whose scalar starts with one of `prefixes`.
/// Only the first line of each plain mapping entry is scanned: multi-line values
/// cannot raise an alias/anchor error.
fn first_field_with_unquoted_prefix(content: &str, prefixes: &[char]) -> Option<String> {
    for (key, value) in key_value_lines(content) {
        if key.starts_with('#') {
            continue;
        }
        let Some(first) = value.chars().next() else {
            continue;
        };
        // Skip quoted scalars: they wouldn't trigger the anchor/alias error.
        if first == '\'' || first == '"' {
            continue;
        }
        if prefixes.contains(&first) {
            return Some(key.trim().to_string());
        }
    }
    None
}

/// `line` as a mapping key, when its shape is `key:` or `key: value`.
fn mapping_key(line: &str) -> Option<&str> {
    let (key, rest) = line.split_once(':')?;
    let plain =
        !key.is_empty() && !key.contains(' ') && !key.starts_with('#') && !key.starts_with('-');
    (plain && (rest.is_empty() || rest.starts_with(' '))).then_some(key)
}

/// The hint for a top-level key indented by a stray leading space: YAML folds
/// such a line into the preceding plain scalar and reports the same "mapping
/// values are not allowed" as an unquoted colon does, with no colon in sight.
fn indented_top_level_key_hint(message: &str, content: &str) -> Option<String> {
    let number = flagged_line_number(message)?;
    let lines: Vec<&str> = content.lines().collect();
    let flagged = lines.get(number.checked_sub(1)?)?;
    if !flagged.starts_with(' ') {
        return None;
    }
    let key = mapping_key(flagged.trim_start())?;
    let previous = lines[..number - 1]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())?;
    if previous.starts_with([' ', '\t']) {
        return None;
    }
    mapping_key(previous)?;
    Some(format!(
        "Line {number} starts with a space. Top-level fields must begin at \
         column 0; remove the leading space before `{key}:`."
    ))
}

/// The first `key: <value>` line whose unquoted value contains a second `:`,
/// which is what raises "mapping values are not allowed in this context".
fn first_field_with_unquoted_colon(content: &str) -> Option<(String, String)> {
    for (key, value) in key_value_lines(content) {
        if key.starts_with('#') || key.starts_with('-') {
            continue;
        }
        let first = value.chars().next();
        if matches!(first, Some('\'') | Some('"') | Some('|') | Some('>')) {
            continue;
        }
        if value.contains(':') {
            // Strip a trailing comment if any.
            let value_clean = match value.split_once(" #") {
                Some((v, _)) => v.trim_end(),
                None => value.trim_end(),
            };
            return Some((key.trim().to_string(), value_clean.to_string()));
        }
    }
    None
}

/// The first `key: "...` line whose double-quoted scalar does not close on the
/// same line: a proxy for a multi-line double-quoted scalar.
fn first_field_with_unterminated_dquote(content: &str) -> Option<String> {
    for (key, value) in key_value_lines(content) {
        if !value.starts_with('"') {
            continue;
        }
        let body = &value[1..];
        let mut closed = false;
        let mut prev_backslash = false;
        for ch in body.chars() {
            if prev_backslash {
                prev_backslash = false;
                continue;
            }
            if ch == '\\' {
                prev_backslash = true;
                continue;
            }
            if ch == '"' {
                closed = true;
                break;
            }
        }
        if !closed {
            return Some(key.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_from_multiple_advice() {
        let raw =
            "multiple YAML documents detected; use from_multiple or from_multiple_with_options";
        let out = sanitize_message(raw);
        assert_eq!(out, "multiple YAML documents detected");
    }

    #[test]
    fn strips_duplicate_key_policy_advice() {
        let raw =
            "duplicate mapping key: organizations, set DuplicateKeyPolicy in Options if acceptable";
        let out = sanitize_message(raw);
        assert_eq!(out, "duplicate mapping key: organizations");
    }

    #[test]
    fn hint_for_alias_unknown_anchor_names_field() {
        let content = "title: Doc\nbluf: **Increased maritime activity**\n";
        let enriched = enrich_yaml_error("alias references unknown anchor", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bluf"), "hint did not name the field: {hint}");
        assert!(hint.contains("single quotes"));
    }

    #[test]
    fn hint_for_mapping_values_names_field_and_value() {
        let content = "system_name: Node.js Service: Order Processing API\n";
        let enriched = enrich_yaml_error("mapping values are not allowed in this context", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("system_name"));
        assert!(hint.contains("Node.js Service: Order Processing API"));
        assert!(hint.contains("Quote"));
    }

    #[test]
    fn hint_for_indented_key_names_the_leading_space() {
        let content = concat!(
            "subject: Near-Miss Involving Aerospace Ground Equipment Tug on the Flightline\n",
            " date: 2026-04-02\n",
            "authority_line: \"\"\n"
        );
        let enriched = enrich_yaml_error(
            "error: line 2 column 6: mapping values are not allowed in this context",
            content,
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("Line 2 starts with a space"), "{hint}");
        assert!(hint.contains("`date:`"), "{hint}");
        assert!(!hint.contains("double quotes"), "{hint}");
    }

    #[test]
    fn hint_for_indented_bare_key_names_the_leading_space() {
        let content = "subject: Near-Miss On The Flightline\n distribution:\nauthority_line: \"\"\n";
        let enriched = enrich_yaml_error(
            "error: line 2 column 14: mapping values are not allowed in this context",
            content,
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("Line 2 starts with a space"), "{hint}");
        assert!(hint.contains("`distribution:`"), "{hint}");
    }

    #[test]
    fn hint_for_unquoted_colon_survives_the_indent_branch() {
        let enriched = enrich_yaml_error(
            "error: line 1 column 13: mapping values are not allowed in this context",
            "field: value: with colon\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("field"), "{hint}");
        assert!(hint.contains("value: with colon"), "{hint}");
        assert!(hint.contains("Quote"), "{hint}");
    }

    #[test]
    fn indented_key_hint_reads_the_real_parser_message() {
        let content = concat!(
            "subject: Near-Miss Involving Aerospace Ground Equipment Tug on the Flightline\n",
            " date: 2026-04-02\n",
            "authority_line: \"\"\n"
        );
        let raw = serde_saphyr::from_str::<serde_json::Value>(content)
            .expect_err("the indented key should not parse")
            .to_string();
        let hint = enrich_yaml_error(&raw, content)
            .hint
            .expect("hint should be set");
        assert!(hint.contains("Line 2 starts with a space"), "{hint}");
        assert!(hint.contains("`date:`"), "{hint}");
    }

    #[test]
    fn indented_key_after_a_prose_line_falls_through() {
        let enriched = enrich_yaml_error(
            "error: line 2 column 6: mapping values are not allowed in this context",
            "a long plain scalar with no colon\n date: 2026-04-02\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("double quotes"), "{hint}");
    }

    #[test]
    fn hint_for_multiple_documents_calls_out_dash_separator() {
        let content = "title: Doc\n---\n";
        let enriched = enrich_yaml_error("multiple YAML documents detected", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("`---`"));
        assert!(hint.contains("`~~~`"));
    }

    #[test]
    fn hint_for_duplicate_key_is_actionable() {
        let enriched = enrich_yaml_error("duplicate mapping key: organizations", "");
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("at most once"));
    }

    #[test]
    fn hint_for_multiline_dquote_suggests_block_scalar() {
        let content = "bullets: \"- one\n- two\n- three\"\n";
        let enriched = enrich_yaml_error("invalid indentation in multiline quoted scalar", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bullets"));
        assert!(hint.contains("block scalar"));
    }

    #[test]
    fn returns_no_hint_for_unrecognized_messages() {
        let enriched = enrich_yaml_error("something unrelated", "");
        assert!(enriched.hint.is_none());
        assert_eq!(enriched.message, "something unrelated");
    }

    #[test]
    fn hint_for_block_sequence_in_mapping_context() {
        let enriched = enrich_yaml_error(
            "block sequence entries are not allowed in this context",
            "section_headers:\n- Title\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("`- item` list"));
        assert!(hint.contains("indent"));
    }

    #[test]
    fn hint_for_simple_key_expected_suggests_block_scalar() {
        let enriched = enrich_yaml_error(
            "simple key expected at line 17, column 1",
            "summary: This is a long\nsummary across multiple lines\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("block scalar"));
        assert!(hint.contains("|"));
    }

    #[test]
    fn hint_for_wrapped_scalar_survives_a_located_line() {
        let enriched = enrich_yaml_error(
            "error: line 2 column 1: simple key expected ':'",
            "summary: This is a long\nsummary across multiple lines\n",
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("block scalar"), "{hint}");
        assert!(!hint.contains("reads as prose"), "{hint}");
    }

    #[test]
    fn hint_for_prose_inside_block_says_close_the_block() {
        let content = "title: Near-Miss Report\ndate: 2026-03-14\n\
                       88th Communications Squadron, Wright-Patterson AFB\n\
                       This memorandum documents a near-miss on the flight line.\n";
        let enriched = enrich_yaml_error("error: line 3 column 1: simple key expected ':'", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("Line 3"), "{hint}");
        assert!(hint.contains("88th Communications Squadron"), "{hint}");
        assert!(hint.contains("reads as prose"), "{hint}");
        assert!(hint.contains("`~~~`"), "{hint}");
        assert!(!hint.contains("block scalar"), "{hint}");
    }

    #[test]
    fn hint_for_prose_after_a_blank_line_says_close_the_block() {
        let content = "title: Memo\n\nThis memorandum documents a near-miss on the flight line.\n";
        let enriched = enrich_yaml_error("error: line 3 column 1: simple key expected ':'", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("Line 3"), "{hint}");
        assert!(hint.contains("reads as prose"), "{hint}");
        assert!(!hint.contains("block scalar"), "{hint}");
    }

    #[test]
    fn hint_for_prose_right_after_a_multi_word_field_says_close_the_block() {
        let content = "title: Near-Miss Report\n88th Communications Squadron, Wright-Patterson AFB\n";
        let enriched = enrich_yaml_error("error: line 2 column 1: simple key expected ':'", content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("reads as prose"), "{hint}");
    }

    #[test]
    fn prose_hint_elides_a_long_line() {
        let long = "This memorandum documents a near-miss that occurred during the flight line inspection.";
        let content = format!("title: Memo\n\n{long}\n");
        let enriched =
            enrich_yaml_error("error: line 3 column 1: simple key expected ':'", &content);
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains('…'), "{hint}");
        assert!(!hint.contains("inspection"), "{hint}");
    }

    #[test]
    fn hint_for_simple_key_expect_colon_variant_also_matches() {
        let enriched = enrich_yaml_error("simple key expect ':'", "");
        assert!(enriched.hint.is_some());
    }

    #[test]
    fn hint_for_scanning_anchor_names_field() {
        let content = "title: Doc\nbluf: &unquoted ampersand\n";
        let enriched = enrich_yaml_error(
            "while scanning an anchor or alias, did not find expected alphabetic or numeric character",
            content,
        );
        let hint = enriched.hint.expect("hint should be set");
        assert!(hint.contains("bluf"));
        assert!(hint.contains("single quotes"));
    }

    #[test]
    fn does_not_panic_on_multibyte_content() {
        let content = "briefer: Maj Sarah Chen — INDOPACOM/A2\nbluf: **\u{201c}peer\u{201d}**\n";
        let _ = enrich_yaml_error("alias references unknown anchor", content);
        let _ = enrich_yaml_error("mapping values are not allowed in this context", content);
    }
}
