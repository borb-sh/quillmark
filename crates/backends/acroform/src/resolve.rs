//! The value step: turn a value-free [`BoundWidget`] plus the document's
//! `compile_data` JSON into a stamp-spine [`FieldSpec`]. Kind, options, tooltip
//! and geometry are already resolved by [`crate::bind`]; so is multiline, which
//! the value can only widen.
//!
//! Binding is against `compile_data` — the same validated, blank-filled object
//! the Typst plate reads as `data.*` — so blank-fill, validation, defaults and
//! scalar coercion are inherited rather than re-implemented. Addressing is a
//! shallow path (`field`, `field.0`, `field.sub`); absent or null renders blank.

use quillmark_pdf::{CHECKBOX_ON_STATE, FieldSpec, FieldType};
use serde_json::Value;

use crate::bind::BoundWidget;

/// Build a [`FieldSpec`] for `widget`, resolving its bound value from `data`.
///
/// A text value holding a newline makes its widget multiline whatever the
/// schema said: a richtext of two paragraphs, an array, a block scalar. The
/// appearance stream stacks the lines, and a single-line widget shows a viewer
/// one, so the rule reads the string the widget draws. A value past the
/// widget's character cap stamps without `/MaxLen`, which would otherwise
/// leave a viewer to cut the value the document holds; the count is the
/// viewer's, UTF-16 units of the drawn string, since a non-ASCII `/V` is
/// UTF-16BE.
pub fn field_spec(widget: &BoundWidget, data: &Value) -> FieldSpec {
    let mut spec = FieldSpec::new(
        widget.name.clone(),
        widget.page,
        widget.rect,
        widget.field_type.clone(),
    );
    spec.value = match &widget.option {
        Some(option) => lookup(data, widget.schema_field.as_deref().unwrap_or_default())
            .is_some_and(|raw| raw.as_str() == Some(option))
            .then(|| CHECKBOX_ON_STATE.to_string()),
        None => resolve_value(&widget.field_type, widget.schema_field.as_deref(), data),
    };
    if let (FieldType::Text { multiline }, Some(value)) = (&mut spec.field_type, &spec.value)
        && value.contains('\n')
    {
        *multiline = true;
    }
    spec.max_len = widget.max_len.filter(|max| {
        spec.value
            .as_deref()
            .is_none_or(|v| v.encode_utf16().count() <= *max as usize)
    });
    spec.schema_field = widget.schema_field.clone();
    spec.tooltip = widget.tooltip.clone();
    spec
}

/// `None` renders a blank widget: unbound, absent/null, signature, empty text,
/// unchecked checkbox, or a choice value matching no option.
fn resolve_value(
    field_type: &FieldType,
    schema_field: Option<&str>,
    data: &Value,
) -> Option<String> {
    let raw = lookup(data, schema_field?)?;
    match field_type {
        FieldType::Text { .. } => coerce_text(raw),
        // A matrix member binds its presence, and a held member reaches the
        // plate as its mapping of columns.
        FieldType::Checkbox => matches!(raw, Value::Bool(true) | Value::Object(_))
            .then(|| CHECKBOX_ON_STATE.to_string()),
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

/// Coerce a JSON value to display text. An empty array element joins as an
/// empty line; a value with no text at all becomes `None`, so the widget
/// carries no `/V`.
fn coerce_text(v: &Value) -> Option<String> {
    match v {
        Value::Array(arr) => {
            let lines: Vec<String> = arr.iter().filter_map(element_text).collect();
            lines
                .iter()
                .any(|l| !l.is_empty())
                .then(|| lines.join("\n"))
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
/// no plaintext form, so a table-only content is empty text: blank as a whole
/// value, an empty line as an array element, the session's
/// `backend::declined_construct` warning naming the table.
fn richtext_plaintext(v: &Value) -> Option<String> {
    let rt = quillmark_content::lines::serial::from_canonical_value(v).ok()?;
    Some(quillmark_content::lines::export::to_plaintext(&rt))
}

/// A choice value binds only if it matches one of the declared options exactly.
fn coerce_choice(v: &Value, options: &[String]) -> Option<String> {
    let s = match v {
        Value::Object(_) => return None,
        _ => element_text(v)?,
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
            "score": 42,
            "quals": { "flight_cc": { "detail": "x" }, "dco": {} }
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
        let rt = quillmark_content::lines::import::from_markdown("A **bold** claim.\n\nSecond line.")
            .unwrap()
            .content;
        let content = quillmark_content::lines::serial::to_canonical_value(&rt);
        assert_eq!(
            coerce_text(&content).as_deref(),
            Some("A bold claim.\nSecond line.")
        );
        let blank = quillmark_content::lines::serial::to_canonical_value(
            &quillmark_content::lines::model::Normalized::empty(),
        );
        assert_eq!(coerce_text(&blank), None);
        assert_eq!(coerce_text(&json!({ "x": 1 })), None);
    }

    fn el(md: &str) -> Value {
        quillmark_content::lines::serial::to_canonical_value(
            &quillmark_content::lines::import::from_markdown(md)
                .unwrap()
                .content,
        )
    }

    #[test]
    fn richtext_array_joins_element_plaintext() {
        let arr = Value::Array(vec![el("First **ref**."), el("Second _ref_.")]);
        assert_eq!(
            coerce_text(&arr).as_deref(),
            Some("First ref.\nSecond ref.")
        );
    }

    #[test]
    fn an_empty_array_element_joins_as_an_empty_line() {
        let rich = Value::Array(vec![el("HEADING"), el("Detail"), el(""), el("NEXT")]);
        assert_eq!(
            coerce_text(&rich).as_deref(),
            Some("HEADING\nDetail\n\nNEXT")
        );
        assert_eq!(
            coerce_text(&json!(["a", "", "b"])).as_deref(),
            Some("a\n\nb")
        );
        assert_eq!(coerce_text(&Value::Array(vec![el(""), el("")])), None);
        assert_eq!(coerce_text(&json!(["", ""])), None);
    }

    #[test]
    fn unbound_is_blank() {
        assert_eq!(
            resolve_value(&FieldType::Text { multiline: false }, None, &data()),
            None
        );
    }

    /// A matrix member binds its presence: a held member reaches the plate as
    /// its mapping of columns, an unheld one not at all.
    #[test]
    fn checkbox_reads_a_bool_or_a_members_presence() {
        let on = |f| resolve_value(&FieldType::Checkbox, Some(f), &data());
        for (field, checked) in [
            ("agree", true),
            ("decline", false),
            ("missing", false),
            ("quals.flight_cc", true),
            ("quals.dco", true),
            ("quals.cyber", false),
        ] {
            assert_eq!(on(field).is_some(), checked, "{field}");
        }
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

    fn text_widget(schema_field: &str) -> BoundWidget {
        BoundWidget {
            name: schema_field.to_uppercase(),
            schema_field: Some(schema_field.to_string()),
            page: 0,
            rect: [72.0, 700.0, 300.0, 720.0],
            field_type: FieldType::Text { multiline: false },
            tooltip: None,
            max_len: None,
            option: None,
        }
    }

    #[test]
    fn a_value_carrying_a_newline_reaches_the_stamp_multiline() {
        let two_paragraphs = quillmark_content::lines::serial::to_canonical_value(
            &quillmark_content::lines::import::from_markdown("One.\n\nTwo.")
                .unwrap()
                .content,
        );
        let data = json!({
            "bio": two_paragraphs,
            "address": "1 Main St\nSpringfield",
            "full_name": "Ada Lovelace",
        });
        let multiline = |field: &str| match field_spec(&text_widget(field), &data).field_type {
            FieldType::Text { multiline } => multiline,
            other => panic!("{field}: {other:?}"),
        };
        assert!(multiline("bio"), "richtext of two paragraphs");
        assert!(multiline("address"), "block scalar");
        assert!(
            !multiline("full_name"),
            "one line keeps the schema's answer"
        );
    }

    #[test]
    fn an_option_box_is_checked_by_its_value_alone() {
        let button = |option: &str| BoundWidget {
            field_type: FieldType::Checkbox,
            option: Some(option.into()),
            ..text_widget("color")
        };
        let checked = |option: &str, data: Value| field_spec(&button(option), &data).is_checked();
        assert!(checked("green", json!({ "color": "green" })));
        assert!(!checked("red", json!({ "color": "green" })));
        assert!(!checked("green", json!({ "color": "" })));
        assert!(!checked("green", json!({})));
    }

    #[test]
    fn a_value_past_the_cap_stamps_without_max_len() {
        let capped = BoundWidget {
            max_len: Some(5),
            ..text_widget("name")
        };
        let max_len = |name: &str| field_spec(&capped, &json!({ "name": name })).max_len;
        assert_eq!(max_len("Ada"), Some(5));
        assert_eq!(max_len("Ada L"), Some(5), "at the cap");
        assert_eq!(
            max_len("Ada Lovelace"),
            None,
            "a viewer would cut the value"
        );
        assert_eq!(
            max_len("abcd\u{1F600}"),
            None,
            "a viewer counts UTF-16 units"
        );
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
