//! The value step: turn a value-free [`BoundWidget`] plus the document's
//! `compile_data` JSON into a stamp-spine [`FieldSpec`]. Kind, options,
//! multiline, tooltip and geometry are already resolved by [`crate::bind`].
//!
//! Binding is against `compile_data` — the same validated, blank-filled object
//! the Typst plate reads as `data.*` — so blank-fill, validation, defaults and
//! scalar coercion are inherited rather than re-implemented. Addressing is a
//! shallow path (`field`, `field.0`, `field.sub`); absent or null renders blank.

use quillmark_pdf::{FieldSpec, FieldType, CHECKBOX_ON_STATE};
use serde_json::Value;

use crate::bind::BoundWidget;

/// Build a [`FieldSpec`] for `widget`, resolving its bound value from `data`.
pub fn field_spec(widget: &BoundWidget, data: &Value) -> FieldSpec {
    let mut spec = FieldSpec::new(
        widget.name.clone(),
        widget.page,
        widget.rect,
        widget.field_type.clone(),
    );
    spec.value = resolve_value(&widget.field_type, widget.schema_field.as_deref(), data);
    spec.schema_field = widget.schema_field.clone();
    spec.tooltip = widget.tooltip.clone();
    spec
}

/// `None` renders a blank widget: unbound, absent/null, signature, empty text,
/// unchecked checkbox, or a choice value matching no option.
fn resolve_value(field_type: &FieldType, schema_field: Option<&str>, data: &Value) -> Option<String> {
    let raw = lookup(data, schema_field?)?;
    match field_type {
        FieldType::Text { .. } => coerce_text(raw),
        FieldType::Checkbox => is_truthy(raw).then(|| CHECKBOX_ON_STATE.to_string()),
        FieldType::Choice { options } => coerce_choice(raw, options),
        FieldType::Signature => None,
    }
}

fn lookup<'a>(data: &'a Value, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let root = parts.next()?;
    if root == "$cards" {
        return lookup_card(data, parts);
    }
    descend(data.get(root)?, parts)
}

/// The bind step rejects absolute indexing, so the first segment is always a
/// kind and the second its instance index.
fn lookup_card<'a, 'p, I>(data: &'a Value, mut parts: I) -> Option<&'a Value>
where
    I: Iterator<Item = &'p str>,
{
    let cards = data.get("$cards")?.as_array()?;
    let kind = parts.next()?;
    let i: usize = parts.next()?.parse().ok()?;
    let card = cards
        .iter()
        .filter(|c| c.get("$kind").and_then(Value::as_str) == Some(kind))
        .nth(i)?;
    descend(card, parts)
}

fn descend<'a, 'p, I>(start: &'a Value, parts: I) -> Option<&'a Value>
where
    I: Iterator<Item = &'p str>,
{
    let mut cur = start;
    for seg in parts {
        cur = match seg.parse::<usize>() {
            Ok(idx) => cur.get(idx)?,
            Err(_) => cur.get(seg)?,
        };
    }
    Some(cur)
}

/// Stringify a JSON number as the Typst producer does, so both backends bind
/// identical text. `Number::to_string` keeps the JSON literal form (`42.0` →
/// `"42.0"`), but Typst decodes to `Int`/`Float` and prints via Rust `Display`,
/// dropping the trailing `.0`; float-backed numbers take that path here.
fn number_to_string(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() => f.to_string(),
        _ => n.to_string(),
    }
}

/// Coerce a JSON value to display text. An empty result becomes `None`, so the
/// widget carries no `/V`.
fn coerce_text(v: &Value) -> Option<String> {
    match v {
        Value::Array(arr) => {
            let s = arr
                .iter()
                .filter_map(element_text)
                .collect::<Vec<_>>()
                .join("\n");
            (!s.is_empty()).then_some(s)
        }
        _ => element_text(v).filter(|s| !s.is_empty()),
    }
}

/// Display text for one scalar or richtext object. An empty string survives
/// here and is blanked by the caller, which also handles array elements.
fn element_text(e: &Value) -> Option<String> {
    match e {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(number_to_string(n)),
        Value::Bool(b) => Some(b.to_string()),
        Value::Object(_) => richtext_plaintext(e),
        _ => None,
    }
}

/// A richtext content's plaintext, island slots stripped. Tables and images have
/// no plaintext form, so a table-only content binds blank with no diagnostic.
fn richtext_plaintext(v: &Value) -> Option<String> {
    let rt = quillmark_content::serial::from_canonical_value(v).ok()?;
    let text = quillmark_content::export::to_plaintext(&rt);
    (!text.is_empty()).then_some(text)
}

/// A boolean schema field arrives as a JSON bool; strings and numbers are
/// handled defensively.
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1" | "y" | "checked"
        ),
        _ => false,
    }
}

/// A choice value binds only if it matches one of the declared options exactly.
fn coerce_choice(v: &Value, options: &[String]) -> Option<String> {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => number_to_string(n),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    options.iter().any(|o| o == &s).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> Value {
        json!({
            "full_name": "Ada Lovelace",
            "comments": ["line one", "line two"],
            "agree": true,
            "decline": false,
            "favorite_color": "green",
            "bad_color": "purple",
            "empty": "",
            "score": 42
        })
    }

    fn text(field: &str) -> Option<String> {
        resolve_value(&FieldType::Text { multiline: false }, Some(field), &data())
    }

    #[test]
    fn text_binds_scalar_and_joins_arrays() {
        assert_eq!(text("full_name"), Some("Ada Lovelace".into()));
        assert_eq!(
            resolve_value(
                &FieldType::Text { multiline: true },
                Some("comments"),
                &data()
            ),
            Some("line one\nline two".into())
        );
        assert_eq!(text("score"), Some("42".into()));
    }

    #[test]
    fn number_stringification_matches_typst_producer() {
        assert_eq!(coerce_text(&json!(42.0)), Some("42".into()));
        assert_eq!(coerce_text(&json!(1e10)), Some("10000000000".into()));
        assert_eq!(coerce_text(&json!(42)), Some("42".into()));
        assert_eq!(coerce_text(&json!(42.5)), Some("42.5".into()));
        let opts = vec!["42".to_string()];
        assert_eq!(coerce_choice(&json!(42.0), &opts), Some("42".into()));
    }

    #[test]
    fn array_index_addressing() {
        assert_eq!(text("comments.0"), Some("line one".into()));
        assert_eq!(text("comments.1"), Some("line two".into()));
        assert_eq!(text("comments.9"), None);
    }

    #[test]
    fn empty_and_absent_are_blank() {
        assert_eq!(text("empty"), None);
        assert_eq!(text("does_not_exist"), None);
    }

    #[test]
    fn richtext_content_lowers_to_plaintext() {
        let rt =
            quillmark_content::import::from_markdown("A **bold** claim.\n\nSecond line.").unwrap();
        let content = quillmark_content::serial::to_canonical_value(&rt);
        assert_eq!(
            coerce_text(&content).as_deref(),
            Some("A bold claim.\nSecond line.")
        );
        let blank =
            quillmark_content::serial::to_canonical_value(&quillmark_content::Normalized::empty());
        assert_eq!(coerce_text(&blank), None);
        assert_eq!(coerce_text(&json!({ "x": 1 })), None);
    }

    #[test]
    fn richtext_array_joins_element_plaintext() {
        let el = |md: &str| {
            quillmark_content::serial::to_canonical_value(
                &quillmark_content::import::from_markdown(md).unwrap(),
            )
        };
        let arr = Value::Array(vec![el("First **ref**."), el("Second _ref_.")]);
        assert_eq!(
            coerce_text(&arr).as_deref(),
            Some("First ref.\nSecond ref.")
        );
    }

    #[test]
    fn unbound_is_blank() {
        assert_eq!(
            resolve_value(&FieldType::Text { multiline: false }, None, &data()),
            None
        );
    }

    #[test]
    fn checkbox_truthiness() {
        let on = |f| resolve_value(&FieldType::Checkbox, Some(f), &data());
        assert_eq!(on("agree"), Some(CHECKBOX_ON_STATE.to_string()));
        assert_eq!(on("decline"), None);
        assert_eq!(on("missing"), None);
    }

    #[test]
    fn choice_must_match_option() {
        let opts = vec!["red".to_string(), "green".to_string(), "blue".to_string()];
        let kind = FieldType::Choice { options: opts };
        assert_eq!(
            resolve_value(&kind, Some("favorite_color"), &data()),
            Some("green".into())
        );
        assert_eq!(resolve_value(&kind, Some("bad_color"), &data()), None);
    }

    #[test]
    fn signature_never_binds() {
        assert_eq!(
            resolve_value(&FieldType::Signature, Some("full_name"), &data()),
            None
        );
    }

    /// Two indorsements with a note between them, so by-kind indexing must skip it.
    fn card_data() -> Value {
        json!({
            "$cards": [
                { "$kind": "indorsement", "from": "Alice", "agree": true },
                { "$kind": "note",        "from": "ignored" },
                { "$kind": "indorsement", "from": "Bob",   "agree": false }
            ]
        })
    }

    fn card_text(path: &str) -> Option<String> {
        resolve_value(
            &FieldType::Text { multiline: false },
            Some(path),
            &card_data(),
        )
    }

    #[test]
    fn card_by_kind_index() {
        assert_eq!(card_text("$cards.indorsement.0.from"), Some("Alice".into()));
        assert_eq!(card_text("$cards.indorsement.1.from"), Some("Bob".into()));
        assert_eq!(card_text("$cards.note.0.from"), Some("ignored".into()));
        assert_eq!(card_text("$cards.indorsement.2.from"), None);
        assert_eq!(card_text("$cards.memo.0.from"), None);
    }

    #[test]
    fn card_coercion_runs_per_widget_type() {
        let agree = |path| resolve_value(&FieldType::Checkbox, Some(path), &card_data());
        assert_eq!(
            agree("$cards.indorsement.0.agree"),
            Some(CHECKBOX_ON_STATE.to_string())
        );
        assert_eq!(agree("$cards.indorsement.1.agree"), None);
    }

    #[test]
    fn card_malformed_paths_are_blank() {
        assert_eq!(card_text("$cards.indorsement"), None);
        assert_eq!(card_text("$cards"), None);
        assert_eq!(card_text("$cards.indorsement.0.missing"), None);
        // `$cards.0.from` reads `0` as a kind, matching no card.
        assert_eq!(card_text("$cards.0.from"), None);
    }

    #[test]
    fn is_truthy_string_and_number_variants() {
        for s in ["true", "Yes", " ON ", "1", "y", "Checked"] {
            assert!(is_truthy(&json!(s)), "{s:?} should be truthy");
        }
        for s in ["false", "no", "0", "", "maybe", "off"] {
            assert!(!is_truthy(&json!(s)), "{s:?} should be falsy");
        }
        assert!(is_truthy(&json!(42)));
        assert!(is_truthy(&json!(-1)));
        assert!(!is_truthy(&json!(0)));
        assert!(!is_truthy(&json!(null)));
        assert!(!is_truthy(&json!([true])));
        assert!(!is_truthy(&json!({"a": 1})));
    }

    #[test]
    fn coerce_text_array_filters_non_string_elements() {
        assert_eq!(
            coerce_text(&json!([null, "a", { "x": 1 }, 2, true])),
            Some("a\n2\ntrue".into())
        );
        assert_eq!(coerce_text(&json!([null, { "x": 1 }])), None);
        assert_eq!(coerce_text(&json!([])), None);
    }

}
