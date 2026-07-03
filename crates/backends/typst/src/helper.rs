//! Generates the virtual `@local/quillmark-helper:0.1.0` package that
//! provides document data and helper functions to Typst plates.
//! The package exports `data` — a dictionary of document fields with markdown
//! and date fields auto-converted to Typst values.

use std::ops::Range;

use crate::convert::escape_string;

/// Exposed for fuzzing tests.
#[doc(hidden)]
pub fn inject_json(bytes: &str) -> String {
    format!("json(bytes(\"{}\"))", escape_string(bytes))
}

pub const HELPER_VERSION: &str = "0.1.0";
pub const HELPER_NAMESPACE: &str = "local";
pub const HELPER_NAME: &str = "quillmark-helper";

const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// A generated eval call site's byte window in the produced `lib.typ`: the
/// range of the string-literal argument (quotes included — the span `eval`
/// stamps on its result), keyed by the schema address of the content it
/// carries. The world layer pairs these with the helper's `FileId` for the
/// span scan.
pub struct ContentWindow {
    pub path: String,
    pub range: Range<usize>,
}

/// Generate `lib.typ` for the quillmark-helper package from JSON data plus
/// the per-render content entries — `(schema address, converted markup)`
/// pairs from `content_entries`, one per content field / `markdown[]` element
/// / card content field present in the data. Each entry becomes a textually
/// distinct `eval()` call site in the generated `_qm-content` dictionary, and
/// the returned windows record where.
pub fn generate_lib_typ(json_data: &str, content: &[(String, String)]) -> (String, Vec<ContentWindow>) {
    let escaped_json = escape_string(json_data);
    let with_data = LIB_TYP_TEMPLATE
        .replace("{version}", HELPER_VERSION)
        .replace("{escaped_json}", &escaped_json);

    const SLOT: &str = "{content_evals}";
    let slot = with_data
        .find(SLOT)
        .expect("lib.typ.template carries the {content_evals} slot");

    let (block, rel_windows) = content_evals(content);
    let mut src = String::with_capacity(with_data.len() - SLOT.len() + block.len());
    src.push_str(&with_data[..slot]);
    src.push_str(&block);
    src.push_str(&with_data[slot + SLOT.len()..]);

    let windows = rel_windows
        .into_iter()
        .map(|(path, r)| ContentWindow {
            path,
            range: (r.start + slot)..(r.end + slot),
        })
        .collect();
    (src, windows)
}

/// The generated `_qm-content` dictionary source, with each entry's
/// string-literal window relative to the block's own start. The window covers
/// the literal *including* both quotes — confirmed empirically: the uniform
/// span `eval` assigns is the argument expression's, which is the quoted
/// literal, not its contents.
fn content_evals(content: &[(String, String)]) -> (String, Vec<(String, Range<usize>)>) {
    if content.is_empty() {
        return ("#let _qm-content = (:)".to_string(), Vec::new());
    }
    let mut out = String::from("#let _qm-content = (\n");
    let mut windows = Vec::with_capacity(content.len());
    for (path, value) in content {
        out.push_str("  \"");
        out.push_str(&escape_string(path));
        out.push_str("\": eval(");
        let start = out.len();
        out.push('"');
        out.push_str(&escape_string(value));
        out.push('"');
        windows.push((path.clone(), start..out.len()));
        out.push_str(", mode: \"markup\"),\n");
    }
    out.push(')');
    (out, windows)
}

pub fn generate_typst_toml() -> String {
    format!(
        r#"[package]
name = "{name}"
version = "{version}"
namespace = "{namespace}"
entrypoint = "lib.typ"
"#,
        name = HELPER_NAME,
        version = HELPER_VERSION,
        namespace = HELPER_NAMESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_lib_typ_basic() {
        let json = r#"{"title":"Test","$body":"Hello","date":"2025-01-15","__meta__":{"content_fields":["$body"],"card_content_fields":{},"date_fields":["date"],"card_date_fields":{}}}"#;
        let (lib, windows) = generate_lib_typ(json, &[("$body".to_string(), "Hello".to_string())]);

        assert!(lib.contains("Version: 0.1.0"));
        assert!(lib.contains("json(bytes("));
        // The template must expose only the private `_parse-date` helper —
        // no public `parse-date` and no `eval-markup` symbol.
        assert!(!lib.contains("eval-markup"));
        assert!(lib.contains("#let _parse-date(s)"));
        assert!(!lib.contains("#let parse-date(s)"));
        assert!(lib.contains("meta.date_fields"));
        assert!(lib.contains("meta.card_date_fields"));
        // The marker system is gone: no `tagged`, no `_qm-tag`; cards still
        // carry their `$path` prefix for form-field address composition.
        assert!(!lib.contains("#let tagged"));
        assert!(!lib.contains("_qm-tag"));
        assert!(lib.contains("card.insert(\"$path\", prefix)"));
        // Each content entry becomes its own eval call site whose recorded
        // window is exactly the quoted literal.
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].path, "$body");
        assert_eq!(&lib[windows[0].range.clone()], "\"Hello\"");
    }

    #[test]
    fn windows_cover_escaped_literals_exactly() {
        let entries = vec![
            ("a".to_string(), "line one\nline \"two\"".to_string()),
            ("b.0".to_string(), "plain".to_string()),
        ];
        let (lib, windows) = generate_lib_typ("{}", &entries);
        assert_eq!(windows.len(), 2);
        assert_eq!(
            &lib[windows[0].range.clone()],
            "\"line one\\nline \\\"two\\\"\"",
            "the window covers the escaped literal, quotes included"
        );
        assert_eq!(&lib[windows[1].range.clone()], "\"plain\"");
    }

    #[test]
    fn no_content_entries_yields_an_empty_dict() {
        let (lib, windows) = generate_lib_typ("{}", &[]);
        assert!(lib.contains("#let _qm-content = (:)"));
        assert!(windows.is_empty());
    }

    #[test]
    fn test_generate_lib_typ_escapes_json() {
        let json = r#"{"title": "Test \"quoted\""}"#;
        let (lib, _) = generate_lib_typ(json, &[]);

        assert!(lib.contains("\\\""));
    }

    #[test]
    fn test_generate_lib_typ_handles_newlines() {
        let json = "{\n\"title\": \"Test\"\n}";
        let (lib, _) = generate_lib_typ(json, &[]);

        assert!(lib.contains("\\n"));
    }

    #[test]
    fn test_generate_typst_toml() {
        let toml = generate_typst_toml();

        assert!(toml.contains("name = \"quillmark-helper\""));
        assert!(toml.contains("version = \"0.1.0\""));
        assert!(toml.contains("namespace = \"local\""));
        assert!(toml.contains("entrypoint = \"lib.typ\""));
    }
}
