//! `Form.yaml` is text Quillmark did not write. Over YAML-shaped text under a
//! valid tag, the parse refuses rather than panics. A spec `serde_json` wrote,
//! compact or pretty, reads back as the value it serialized, floats bit for
//! bit, unless a string holds DEL, a C1 control other than U+0085, U+FFFE or
//! U+FFFF raw, as `serde_json` writes them; the parser refuses those with a
//! located error.

use proptest::prelude::*;
use serde_json::{json, Value};

use super::super::{FormParseError, FormSpec};

/// The characters the parser takes raw inside a JSON-style double-quoted
/// string: YAML 1.2's `c-printable`.
fn yaml_printable(c: char) -> bool {
    matches!(c,
        '\t' | '\n' | '\r' | ' '..='~' | '\u{85}' | '\u{A0}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// Strings over characters YAML reads specially beside arbitrary ones, so both
/// a printable spec and a refused one are reached.
fn arb_text() -> impl Strategy<Value = String> {
    let special = prop::sample::select(vec![
        ':', '#', ' ', '\'', '"', '\\', '-', '&', '*', '!', '|', '>', '%', '@', '`', '\n', '\t',
        '\u{7F}', '\u{85}', '\u{9F}', '\u{2028}', '\u{FEFF}', '\u{FFFE}',
    ]);
    proptest::collection::vec(prop_oneof![special, any::<char>()], 0..8)
        .prop_map(|chars| chars.into_iter().collect())
}

fn arb_rect() -> impl Strategy<Value = Value> {
    let coord = || prop_oneof![(0u32..2000).prop_map(Value::from), (-1e6f64..1e6).prop_map(Value::from)];
    (coord(), coord(), coord(), coord()).prop_map(|(x, y, w, h)| json!({ "x": x, "y": y, "w": w, "h": h }))
}

fn arb_field() -> impl Strategy<Value = Value> {
    (arb_text(), arb_text(), 0u32..8, arb_rect(), proptest::option::of(arb_text()))
        .prop_map(|(name, schema_field, page, rect, tooltip)| {
            let mut f = json!({ "name": name, "schema_field": schema_field, "page": page, "rect": rect });
            if let Some(t) = tooltip {
                f["tooltip"] = Value::from(t);
            }
            f
        })
}

fn arb_widget() -> impl Strategy<Value = Value> {
    let kind = prop_oneof![
        any::<bool>().prop_map(|m| json!({ "type": "text", "multiline": m })),
        Just(json!({ "type": "checkbox" })),
        proptest::collection::vec(arb_text(), 0..4).prop_map(|o| json!({ "type": "choice", "options": o })),
        Just(json!({ "type": "signature" })),
    ];
    (arb_text(), 0u32..8, arb_rect(), kind).prop_map(|(name, page, rect, mut w)| {
        w["name"] = Value::from(name);
        w["page"] = Value::from(page);
        w["rect"] = rect;
        w
    })
}

fn arb_spec() -> impl Strategy<Value = Value> {
    (
        proptest::collection::vec(arb_field(), 0..4),
        proptest::collection::vec(arb_widget(), 0..4),
    )
        .prop_map(|(fields, widgets)| {
            json!({ "schema": "quillmark/form@0.2.0", "fields": fields, "widgets": widgets })
        })
}

proptest! {
    #[test]
    fn arbitrary_text_after_a_valid_tag_parses_or_refuses(
        tokens in proptest::collection::vec(
            prop::sample::select(vec![
                "fields:", "widgets:", "\n", "  ", "- ", "name: ", "schema_field: ", "page: ",
                "rect: ", "type: ", "options: ", "tooltip: ", "{", "}", "[", "]", ", ", "x: ",
                "0", "-1", "1e9", "text", "choice", "&a ", "*a", "<<: ", "!!str ", "#c", "\"",
                "'", ": ", "|", ">", "---", "...", "~", "null", "\u{FEFF}", "\u{85}", "\t",
            ]),
            0..64,
        ),
    ) {
        let text = format!("schema: quillmark/form@0.2.0\n{}", tokens.concat());
        let _ = FormSpec::parse(text.as_bytes());
    }

    #[test]
    fn json_a_tool_wrote_reads_back_as_written(spec in arb_spec()) {
        let printable = spec.to_string().chars().all(yaml_printable);
        for text in [spec.to_string(), serde_json::to_string_pretty(&spec).unwrap()] {
            match quillmark_core::value::from_yaml_str::<Value>(&text) {
                Ok(read) => {
                    prop_assert!(printable);
                    prop_assert_eq!(&read, &spec);
                }
                Err(_) => {
                    prop_assert!(!printable);
                    let Err(err @ FormParseError::Yaml(_)) = FormSpec::parse(text.as_bytes()) else {
                        panic!("a YAML refusal is the spec's refusal too");
                    };
                    prop_assert!(err.to_diagnostic().location.is_some());
                }
            }
        }
    }
}
