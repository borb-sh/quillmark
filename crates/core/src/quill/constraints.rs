//! The constraint checks on the **obligation** family: `min`/`max` on an array
//! and on a card kind, `min`/`max`/`step` on a number, `format`/`pattern` on a
//! string.
//!
//! Every diagnostic here is a `Severity::Warning` and none gates render, for
//! the reason `SCHEMAS.md` § "Value and obligation" gives about `must_fill`:
//! severity already *is* the render-gate signal, and a document over a row
//! limit still draws. The plate keeps its own rule for the surplus; nothing
//! here truncates.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::config::{range_violation, Leniency};
use super::{FieldSchema, FieldType, QuillConfig, StringFormat};
use crate::document::Document;
use crate::error::{diag_args, Diagnostic, Severity};
use crate::path::DocPath;
use crate::value::QuillValue;

/// Every constraint a document leaves unmet, across the main card, each
/// composable card, and the card stream's per-kind counts.
///
/// Values are judged in the form the render floor builds from them — the
/// top-level field conformed at [`Leniency::Render`], a value the floor refuses
/// kept as authored — so a scalar the floor reads as a number is range-checked
/// on that number, exactly as an enum's domain is checked on the string the
/// floor built.
pub(crate) fn validate_constraints(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (schema, card, base) in super::compose::schema_cards(config, doc) {
        let Some(schema) = schema else { continue };
        let payload = card.payload();
        for (name, field) in &schema.fields {
            // Conforming is the expensive half, so a field declaring nothing
            // never pays for it: `validate` runs on every editor keystroke, and
            // a quill with no constraint anywhere does no extra work at all.
            if !constrained(field) {
                continue;
            }
            let path = base.field(name);
            let value = payload.get(name).map(|v| {
                QuillConfig::conform_value(v, field, &path.to_string(), Leniency::Render)
                    .unwrap_or_else(|_| v.clone())
            });
            collect_field(field, value.as_ref(), &path, &mut out);
        }
    }

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for card in doc.cards() {
        if let Some(kind) = card.kind() {
            *counts.entry(kind).or_default() += 1;
        }
    }
    for kind in &config.card_kinds {
        if kind.min.is_none() && kind.max.is_none() {
            continue;
        }
        let actual = counts.get(kind.name.as_str()).copied().unwrap_or(0);
        out.extend(cardinality(
            &DocPath::card_kind(&kind.name),
            kind.min,
            kind.max,
            actual,
            Counted::Card,
        ));
    }
    out
}

/// Whether `field`'s type tree declares a constraint anywhere: the gate on the
/// walk below, over the schema alone.
fn constrained(field: &FieldSchema) -> bool {
    field.min.is_some()
        || field.max.is_some()
        || field.step.is_some()
        || field.format.is_some()
        || field.pattern.is_some()
        || field.items.as_deref().is_some_and(constrained)
        || field
            .properties
            .iter()
            .flat_map(|p| p.values())
            .any(|p| constrained(p))
        || field
            .variants
            .iter()
            .flat_map(|v| v.values())
            .flat_map(|set| set.values())
            .any(|c| constrained(c))
}

/// Walk one declared cell and everything below it. The descent is the schema's,
/// as [`collect_unauthored_field`](super::compose) walks it: a variant
/// container descends into the world its discriminant selects, a typed
/// dictionary into its properties, an array into its elements.
fn collect_field(
    field: &FieldSchema,
    value: Option<&QuillValue>,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    if field.is_variant_bearing() {
        let json = value.map(|v| v.as_json());
        let object = json.and_then(|j| j.as_object());
        let member = field.selected_member(json);
        if let Some(fields) = field.variant_fields(&member) {
            for (name, schema) in fields {
                let cell = object
                    .and_then(|o| o.get(name))
                    .map(|j| QuillValue::from_json(j.clone()));
                collect_field(schema, cell.as_ref(), &path.field(name), out);
            }
        }
        return;
    }

    let present = value.filter(|v| !v.as_json().is_null());

    if let (FieldType::Object, Some(props)) = (&field.r#type, &field.properties) {
        let obj = present.and_then(|v| v.as_json().as_object());
        for (name, prop) in props {
            let cell = obj
                .and_then(|o| o.get(name))
                .map(|j| QuillValue::from_json(j.clone()));
            collect_field(prop, cell.as_ref(), &path.field(name), out);
        }
        return;
    }

    if field.r#type == FieldType::Array {
        if field.min.is_some() || field.max.is_some() {
            // The arity the floor builds: authored, else the array's own
            // `default:`, else the blank `[]`. A seed of another shape is a
            // `type_mismatch` the gate already names, and counting it would say
            // a second wrong thing about one value.
            let seed = present.map(QuillValue::as_json).or(field
                .default
                .as_ref()
                .map(QuillValue::as_json));
            if let Some(len) = seed.map_or(Some(0), |v| v.as_array().map(Vec::len)) {
                out.extend(cardinality(
                    path,
                    field.min.as_ref().and_then(serde_json::Number::as_u64),
                    field.max.as_ref().and_then(serde_json::Number::as_u64),
                    len,
                    Counted::Element,
                ));
            }
        }
        if let Some(items) = &field.items {
            let elements = present.and_then(|v| v.as_json().as_array());
            for (index, element) in elements.into_iter().flatten().enumerate() {
                let element = QuillValue::from_json(element.clone());
                collect_field(items, Some(&element), &path.index(index), out);
            }
        }
        return;
    }

    let Some(value) = present else { return };
    match field.r#type {
        FieldType::Number | FieldType::Integer => {
            if let Some(number) = value.as_json().as_f64() {
                if range_violation(field, number).is_some() {
                    out.push(out_of_range_warning(path, field, value.as_json()));
                }
            }
        }
        // The blank is accepted wherever a value is checked, as an enum's is:
        // a shape describes what a value looks like, not that there is one.
        FieldType::String => match (value.as_str(), field.format, &field.pattern) {
            (Some(""), _, _) | (None, _, _) => {}
            (Some(text), Some(format), _) if !matches_format(text, format) => {
                out.push(format_violation_warning(path, format.as_str()));
            }
            (Some(text), _, Some(pattern)) => {
                // Load rejects a pattern that will not compile
                // (`quill::invalid_pattern`), so a schema that got here has one.
                if Regex::new(pattern).is_ok_and(|re| !re.is_match(text)) {
                    out.push(format_violation_warning(path, pattern));
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// What a cardinality warning counts, which is the noun its sentence takes.
#[derive(Clone, Copy)]
pub(crate) enum Counted {
    Element,
    Card,
}

impl Counted {
    fn plural(self, n: usize) -> String {
        let noun = match self {
            Self::Element => "element",
            Self::Card => "card",
        };
        format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
    }
}

/// The warning for a count outside `min..=max`, or `None` when it is inside.
pub(crate) fn cardinality(
    path: &DocPath,
    min: Option<u64>,
    max: Option<u64>,
    actual: usize,
    counted: Counted,
) -> Option<Diagnostic> {
    let count = actual as u64;
    let declared = match (min, max) {
        (Some(min), _) if count < min => format!("at least {min}"),
        (_, Some(max)) if count > max => format!("at most {max}"),
        _ => return None,
    };
    let path = path.to_string();
    let mut args = diag_args! { "actual" => actual };
    if let Some(min) = min {
        args.insert("min".to_string(), min.into());
    }
    if let Some(max) = max {
        args.insert("max".to_string(), max.into());
    }
    Some(
        Diagnostic::new(
            Severity::Warning,
            format!(
                "`{path}` carries {carried}; the schema declares {declared}.",
                carried = counted.plural(actual),
            ),
        )
        .with_code("validation::cardinality".to_string())
        .with_path(path)
        .with_args(args)
        .with_hint(match counted {
            Counted::Element => "Add or remove rows to meet the declared count. The document \
                                 renders either way, and a plate keeps its own rule for the \
                                 surplus."
                .to_string(),
            Counted::Card => "Add or remove cards of this kind. The document renders either \
                              way, and a plate keeps its own rule for the surplus."
                .to_string(),
        }),
    )
}

/// The warning for a number outside its declared `min` / `max` / `step`.
pub(crate) fn out_of_range_warning(
    path: &DocPath,
    field: &FieldSchema,
    value: &serde_json::Value,
) -> Diagnostic {
    let reason = value
        .as_f64()
        .and_then(|n| range_violation(field, n))
        .unwrap_or_else(|| "is outside the declared range".to_string());
    let path = path.to_string();
    let mut args = diag_args! { "value" => value };
    for (key, bound) in [
        ("min", &field.min),
        ("max", &field.max),
        ("step", &field.step),
    ] {
        if let Some(bound) = bound {
            args.insert(key.to_string(), serde_json::Value::Number(bound.clone()));
        }
    }
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` holds {value}, which {reason}."),
    )
    .with_code("validation::out_of_range".to_string())
    .with_path(path)
    .with_args(args)
    .with_hint(
        "Bring the value inside the declared range, or widen the range on the field. The \
         value renders as written either way."
            .to_string(),
    )
}

/// The warning for a string that does not match its declared shape. Shares
/// `validation::format_violation` with the date grammars, which are the same
/// claim about a different type — at `Severity::Error` there, because a date the
/// grammar rejects has no wire form to lower, while a mis-typed URL renders as
/// the text it is.
pub(crate) fn format_violation_warning(path: &DocPath, format: &str) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` does not match expected format `{format}`."),
    )
    .with_code("validation::format_violation".to_string())
    .with_path(path)
    .with_args(diag_args! { "format" => format })
    .with_hint(
        "Correct the value, or drop the shape from the schema. The value renders as written \
         either way."
            .to_string(),
    )
}

/// A scheme and something after it. Deliberately short of RFC 3986: the claim
/// the quiver's plates make is "this is linkable", and everything past the
/// scheme is the viewer's to resolve.
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.\-]*:\S+$").expect("valid regex"));

/// A local part, an `@`, and a dotted domain. No attempt at the full grammar:
/// an address that reaches here is a person's to get right, and a validator
/// strict enough to be correct rejects addresses that deliver.
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^\s@]+@[^\s@.]+(\.[^\s@.]+)+$").expect("valid regex"));

/// Digits and the punctuation people write them with. Separators are a
/// locale's business, so only the digit count is held to anything.
static PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\+?[0-9][0-9 ()./\-]*$").expect("valid regex"));

fn matches_format(text: &str, format: StringFormat) -> bool {
    match format {
        StringFormat::Url => URL.is_match(text),
        StringFormat::Email => EMAIL.is_match(text),
        StringFormat::Phone => {
            PHONE.is_match(text) && text.chars().filter(char::is_ascii_digit).count() >= 7
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::error::Severity;

    fn diags(yaml: &str, markdown: &str) -> Vec<crate::error::Diagnostic> {
        let quill = crate::quill::quill_from_yaml(yaml);
        let doc = Document::parse(markdown).expect("parse").document;
        quill.validate(&doc)
    }

    fn at(diags: &[crate::error::Diagnostic], code: &str, path: &str) -> crate::error::Diagnostic {
        diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code) && d.path.as_deref() == Some(path))
            .unwrap_or_else(|| {
                panic!(
                    "no `{code}` at `{path}`; got {:?}",
                    diags.iter().map(|d| (&d.code, &d.path)).collect::<Vec<_>>()
                )
            })
            .clone()
    }

    const ROWS: &str = r#"
quill: { name: rows, version: "1.0", backend: typst, description: x }
main:
  fields:
    entries:
      type: array
      min: 1
      max: 2
      items: { type: string }
card_kinds:
  purpose:
    max: 1
    fields:
      label: { type: string }
  cover:
    min: 1
    fields:
      label: { type: string }
"#;

    #[test]
    fn an_array_outside_its_declared_count_warns_at_the_container() {
        let over = diags(
            ROWS,
            "~~~\n$quill: rows@1.0\n$kind: main\nentries: [a, b, c]\n~~~\n",
        );
        let d = at(&over, "validation::cardinality", "main.entries");
        assert_eq!(d.severity, Severity::Warning, "never a gate");
        assert_eq!(d.args.get("max"), Some(&serde_json::json!(2)));
        assert_eq!(d.args.get("actual"), Some(&serde_json::json!(3)));

        // An absent defaultless array blank-fills to `[]`, which is below the
        // floor, so the obligation reads as an unauthored must-fill cell does.
        let absent = diags(ROWS, "~~~\n$quill: rows@1.0\n$kind: main\n~~~\n");
        assert_eq!(
            at(&absent, "validation::cardinality", "main.entries")
                .args
                .get("actual"),
            Some(&serde_json::json!(0))
        );

        let inside = diags(
            ROWS,
            "~~~\n$quill: rows@1.0\n$kind: main\nentries: [a, b]\n~~~\n",
        );
        assert!(
            !inside.iter().any(|d| d.path.as_deref() == Some("main.entries")),
            "{inside:?}"
        );
    }

    #[test]
    fn a_card_kind_outside_its_declared_count_warns_at_the_kind() {
        let doc = "~~~\n$quill: rows@1.0\n$kind: main\nentries: [a]\n~~~\n\n\
                   ~~~\n$kind: purpose\nlabel: one\n~~~\n\n\
                   ~~~\n$kind: purpose\nlabel: two\n~~~\n";
        let d = diags(ROWS, doc);

        let surplus = at(&d, "validation::cardinality", "cards.purpose");
        assert_eq!(surplus.args.get("actual"), Some(&serde_json::json!(2)));
        assert_eq!(surplus.args.get("max"), Some(&serde_json::json!(1)));

        // A kind with no instance at all is still short of its floor: the count
        // is per document, not per card.
        let missing = at(&d, "validation::cardinality", "cards.cover");
        assert_eq!(missing.args.get("actual"), Some(&serde_json::json!(0)));
        assert_eq!(missing.args.get("min"), Some(&serde_json::json!(1)));
    }

    const RANGES: &str = r#"
quill: { name: ranges, version: "1.0", backend: typst, description: x }
main:
  fields:
    duration:
      type: number
      min: 0.5
      max: 4
      step: 0.5
      default: 2
    years:
      type: integer
      min: 6
      max: 18
      default: 12
"#;

    #[test]
    fn a_number_outside_its_range_or_off_its_step_warns() {
        for (value, key) in [("5", "max"), ("0.25", "min"), ("1.75", "step")] {
            let d = diags(
                RANGES,
                &format!("~~~\n$quill: ranges@1.0\n$kind: main\nduration: {value}\n~~~\n"),
            );
            let warning = at(&d, "validation::out_of_range", "main.duration");
            assert_eq!(warning.severity, Severity::Warning);
            assert!(
                warning.args.contains_key(key),
                "the bound it broke rides the args: {warning:?}"
            );
        }

        // An absent field blank-fills; the floor's `0` is not an answer anyone
        // gave, so it is not held to the range.
        let absent = diags(RANGES, "~~~\n$quill: ranges@1.0\n$kind: main\n~~~\n");
        assert!(
            !absent
                .iter()
                .any(|d| d.code.as_deref() == Some("validation::out_of_range")),
            "{absent:?}"
        );
    }

    const SHAPES: &str = r#"
quill: { name: shapes, version: "1.0", backend: typst, description: x }
main:
  fields:
    site: { type: string, format: url, default: "" }
    contact: { type: string, format: email, default: "" }
    phone: { type: string, format: phone, default: "" }
    symbol: { type: string, pattern: "^[A-Z0-9]+/[A-Z]+$", default: "" }
"#;

    #[test]
    fn a_string_off_its_declared_shape_warns_and_the_blank_never_does() {
        for (field, bad, good) in [
            ("site", "example.com", "https://example.com"),
            ("contact", "nobody", "a@example.com"),
            ("phone", "call me", "+1 (555) 010-4477"),
            ("symbol", "49fw/cc", "49FW/CC"),
        ] {
            let bad = diags(
                SHAPES,
                &format!("~~~\n$quill: shapes@1.0\n$kind: main\n{field}: \"{bad}\"\n~~~\n"),
            );
            let d = at(&bad, "validation::format_violation", &format!("main.{field}"));
            assert_eq!(d.severity, Severity::Warning, "a mis-typed value renders");

            for value in [good, ""] {
                let clean = diags(
                    SHAPES,
                    &format!("~~~\n$quill: shapes@1.0\n$kind: main\n{field}: \"{value}\"\n~~~\n"),
                );
                assert!(
                    !clean
                        .iter()
                        .any(|d| d.code.as_deref() == Some("validation::format_violation")),
                    "`{value}` must clear `{field}`: {clean:?}"
                );
            }
        }
    }

    /// The walk is the schema's, so a constraint declared at depth is checked
    /// at depth: a row cell, a dictionary property, a variant's field.
    #[test]
    fn constraints_are_checked_wherever_they_are_declared() {
        let yaml = r#"
quill: { name: deep, version: "1.0", backend: typst, description: x }
main:
  fields:
    tours:
      type: array
      items:
        type: object
        properties:
          duration: { type: number, min: 0.5, max: 4, step: 0.5 }
          tags: { type: array, max: 1, items: { type: string } }
    contact:
      type: object
      properties:
        site: { type: string, format: url }
"#;
        let d = diags(
            yaml,
            "~~~\n$quill: deep@1.0\n$kind: main\n\
             tours:\n  - duration: 9\n    tags: [a, b]\n\
             contact:\n  site: nope\n~~~\n",
        );
        at(&d, "validation::out_of_range", "main.tours[0].duration");
        at(&d, "validation::cardinality", "main.tours[0].tags");
        at(&d, "validation::format_violation", "main.contact.site");
    }
}
