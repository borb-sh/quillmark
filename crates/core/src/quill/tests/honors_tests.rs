use super::*;
use indexmap::IndexMap;
use serde_json::json;

const DECLARING: &str = r#"
honors:
  cell: [valign, align]
  table: [breakable, widths, align]
  elements:
    keep:
      scope: block
    stamp:
      scope: inline
      attrs:
        tone: { type: enum, values: [red, blue] }
        size: { type: integer, default: 3 }
        note: { type: string }
main:
  fields:
    intro: { type: richtext }
"#;

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().filter_map(|d| d.code.as_deref()).collect()
}

#[test]
fn a_declaration_loads_in_vocabulary_order_and_emits_in_the_schema() {
    let config = config_with_sections(DECLARING).expect("loads");
    assert_eq!(
        config.honors.knobs.iter().copied().collect::<Vec<_>>(),
        TableKnob::ALL.to_vec()
    );
    assert_eq!(config.honors.elements["keep"].scope, ElementScope::Block);
    assert_eq!(
        config.schema()["honors"],
        json!({
            "table": ["widths", "align", "breakable"],
            "cell": ["align", "valign"],
            "elements": {
                "keep": { "scope": "block" },
                "stamp": {
                    "scope": "inline",
                    "attrs": {
                        "tone": { "type": "enum", "values": ["red", "blue"] },
                        "size": { "type": "integer", "default": 3 },
                        "note": { "type": "string" }
                    }
                }
            }
        })
    );
}

#[test]
fn a_quill_declaring_nothing_emits_and_teaches_as_one_without_the_section() {
    let bare = config_with_sections("main:\n  fields:\n    intro: { type: richtext }\n").unwrap();
    let empty = config_with_sections(
        "honors: { table: [], elements: {} }\nmain:\n  fields:\n    intro: { type: richtext }\n",
    )
    .unwrap();
    assert!(empty.honors.is_empty());
    assert_eq!(empty.schema(), bare.schema());
    assert!(bare.schema().get("honors").is_none());
    assert_eq!(empty.blueprint(), bare.blueprint());
}

#[test]
fn a_malformed_declaration_is_a_load_error_naming_its_class() {
    let cases = [
        ("honors: [table]", "quill::invalid_honors"),
        ("honors: { tables: [widths] }", "quill::invalid_honors"),
        ("honors: { table: widths }", "quill::invalid_honors"),
        ("honors: { table: [width] }", "quill::invalid_honors"),
        ("honors: { table: [valign] }", "quill::invalid_honors"),
        ("honors: { cell: [widths] }", "quill::invalid_honors"),
        ("honors: { table: [align, align] }", "quill::invalid_honors"),
        ("honors: { table: [1] }", "quill::invalid_honors"),
        ("honors: { elements: [keep] }", "quill::invalid_honors"),
        ("honors: { elements: { Keep: { scope: block } } }", "quill::invalid_element_name"),
        ("honors: { elements: { keep_it: { scope: block } } }", "quill::invalid_element_name"),
        ("honors: { elements: { 'keep-': { scope: block } } }", "quill::invalid_element_name"),
        ("honors: { elements: { table: { scope: block } } }", "quill::invalid_element_name"),
        ("honors: { elements: { cell: { scope: inline } } }", "quill::invalid_element_name"),
        ("honors: { elements: { anchor: { scope: inline } } }", "quill::invalid_element_name"),
        ("honors: { elements: { keep: block } }", "quill::invalid_element"),
        ("honors: { elements: { keep: {} } }", "quill::invalid_element"),
        ("honors: { elements: { keep: { scope: span } } }", "quill::invalid_element"),
        ("honors: { elements: { keep: { scope: block, title: K } } }", "quill::invalid_element"),
        ("honors: { elements: { keep: { scope: block, attrs: [a] } } }", "quill::invalid_element"),
        (
            "honors: { elements: { keep: { scope: block, attrs: { style: { type: string } } } } }",
            "quill::invalid_element_attr",
        ),
        (
            "honors: { elements: { keep: { scope: block, attrs: { onload: { type: string } } } } }",
            "quill::invalid_element_attr",
        ),
        (
            "honors: { elements: { keep: { scope: block, attrs: { Note: { type: string } } } } }",
            "quill::invalid_element_attr",
        ),
        (
            "honors: { elements: { keep: { scope: block, attrs: { note: { type: richtext } } } } }",
            "quill::invalid_element_attr",
        ),
        (
            "honors: { elements: { keep: { scope: block, attrs: { note: { type: array, items: { type: string } } } } } }",
            "quill::invalid_element_attr",
        ),
        (
            "honors: { elements: { keep: { scope: block, attrs: { note: { type: color } } } } }",
            "quill::invalid_element_attr",
        ),
    ];
    for (section, code) in cases {
        let errors = config_with_sections(&format!("{section}\nmain:\n  fields: {{}}\n"))
            .expect_err(section);
        assert_eq!(codes(&errors), vec![code], "{section}");
    }
}

#[test]
fn a_backend_typesetting_no_table_honors_no_table_knob_and_no_element() {
    for honors in ["{ cell: [align] }", "{ elements: { keep: { scope: block } } }"] {
        let yaml = format!(
            "quill: {{ name: f, version: 1.0.0, backend: acroform, description: x }}\n\
             honors: {honors}\nmain:\n  fields: {{}}\n"
        );
        let errors = QuillConfig::from_yaml_with_warnings(&yaml).expect_err(honors);
        assert_eq!(codes(&errors), vec!["quill::invalid_honors"], "{honors}");
    }
}

#[test]
fn an_element_coerces_its_declared_attributes_and_copies_the_rest() {
    let config = config_with_sections(DECLARING).unwrap();
    let attrs: IndexMap<String, QuillValue> = [
        ("size", QuillValue::from_json(json!("4"))),
        ("other", QuillValue::from_json(json!("x"))),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let coerced = config.coerce_element("stamp", &attrs).expect("coerces");
    assert_eq!(coerced["size"].as_json(), &json!(4));
    assert_eq!(coerced["other"].as_json(), &json!("x"));
    assert_eq!(config.coerce_element("unknown", &attrs).unwrap(), attrs);
}

/// The attributes an element renders with: each declared one coerced, kept as
/// written where the coercion refuses it, a declared default filling one left
/// out, every other one as written.
#[test]
fn an_element_renders_with_its_coerced_attributes_and_declared_defaults() {
    let honors = config_with_sections(DECLARING).unwrap().honors;
    let attrs = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    };
    let json = |attrs: std::collections::BTreeMap<String, QuillValue>| -> serde_json::Value {
        attrs.into_iter().map(|(k, v)| (k, v.into_json())).collect()
    };
    assert_eq!(
        json(honors.element_attrs("stamp", &attrs(&[("tone", "red"), ("other", "x")]))),
        json!({ "tone": "red", "size": 3, "other": "x" })
    );
    assert_eq!(
        json(honors.element_attrs("stamp", &attrs(&[("size", "4")]))),
        json!({ "size": 4 })
    );
    assert_eq!(
        json(honors.element_attrs("stamp", &attrs(&[("size", "big")]))),
        json!({ "size": "big" })
    );
    assert_eq!(json(honors.element_attrs("unknown", &attrs(&[("a", "1")]))), json!({ "a": "1" }));
    assert!(honors.element("stamp", ElementScope::Inline).is_some());
    assert!(honors.element("stamp", ElementScope::Block).is_none());
}

/// One warning per content field and element it uses at a scope the quill
/// does not declare, counting runs and marks, prose and cells alike.
#[test]
fn validate_warns_once_per_field_and_undeclared_element() {
    let body = "\n<quill-keep>\n\na\n\n</quill-keep>\n\n<quill-keep>\n\nb\n\n</quill-keep>\n\n\
                <quill-stamp>c</quill-stamp> <quill-keep>d</quill-keep>\n\n\
                | <quill-stamp>e</quill-stamp> |\n| --- |\n";
    let md = format!("~~~\n$quill: q@1.0\n$kind: main\n~~~\n{body}");
    let doc = Document::parse(&md).expect("parses").document;
    let undeclared = |sections: &str| -> Vec<(String, serde_json::Value)> {
        let quill = crate::quill::quill_from_yaml(&with_header(sections));
        quill
            .validate(&doc)
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("validation::undeclared_construct"))
            .map(|d| (d.path.unwrap_or_default(), json!(d.args)))
            .collect()
    };
    assert_eq!(
        undeclared("main:\n  fields: {}\n"),
        vec![
            ("main.body".to_string(), json!({ "construct": "element.keep", "count": 3 })),
            ("main.body".to_string(), json!({ "construct": "element.stamp", "count": 2 })),
        ]
    );
    assert_eq!(
        undeclared(DECLARING),
        vec![("main.body".to_string(), json!({ "construct": "element.keep", "count": 1 }))]
    );
}

#[test]
fn the_blueprint_closes_the_root_payload_with_each_declared_construct() {
    let blueprint = config_with_sections(DECLARING).unwrap().blueprint();
    let expected = "\
intro: # richtext<markdown>
# markup this quill honors:
# <quill-table align=\"center\" breakable=\"false\" widths=\"2 1\">
#
# | Item | Amount |
# | --- | --- |
# | Total | <quill-cell align=\"right\" valign=\"bottom\">42</quill-cell> |
#
# </quill-table>
#
# <quill-keep>
#
# Text.
#
# </quill-keep>
#
# Some <quill-stamp note=\"string\" size=\"3\" tone=\"red\">text</quill-stamp> here.
~~~
";
    assert!(blueprint.contains(expected), "{blueprint}");
    let doc = Document::parse(&blueprint).expect("the blueprint parses").document;
    assert!(doc.main().body().is_blank());
}

/// Each knob the blueprint's table carries is one the import folds into the
/// table or cell it wraps.
#[test]
fn the_blueprint_table_imports_carrying_each_declared_knob() {
    let config = config_with_sections(
        "honors: { table: [widths, align, breakable], cell: [align, valign] }\nmain:\n  fields: {}\n",
    )
    .unwrap();
    let blueprint = config.blueprint();
    let example: String = blueprint
        .lines()
        .skip_while(|l| *l != "# markup this quill honors:")
        .skip(1)
        .take_while(|l| l.starts_with('#'))
        .map(|l| format!("{}\n", l.trim_start_matches('#').strip_prefix(' ').unwrap_or("")))
        .collect();
    let content = crate::document::import_body(&example).expect("imports");
    let props = &content.islands[0].props;
    assert_eq!(content.islands.len(), 1, "{example}");
    for &knob in TableKnob::ALL {
        assert_eq!(knob.count_in(&content), 1, "{knob} in {props}");
    }
}

#[test]
fn validate_warns_once_per_field_and_undeclared_knob() {
    let body = "\n<quill-table align=\"center\" widths=\"1 2\">\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n</quill-table>\n\n\
                <quill-table align=\"right\">\n\n| c |\n| --- |\n| 3 |\n\n</quill-table>\n";
    let md = format!("~~~\n$quill: q@1.0\n$kind: main\n~~~\n{body}");
    let doc = Document::parse(&md).expect("parses").document;
    let undeclared = |sections: &str| -> Vec<(String, serde_json::Value)> {
        let quill = crate::quill::quill_from_yaml(&with_header(sections));
        quill
            .validate(&doc)
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("validation::undeclared_construct"))
            .map(|d| (d.path.unwrap_or_default(), json!(d.args)))
            .collect()
    };
    assert_eq!(
        undeclared("main:\n  fields: {}\n"),
        vec![
            ("main.body".to_string(), json!({ "construct": "table.widths", "count": 1 })),
            ("main.body".to_string(), json!({ "construct": "table.align", "count": 2 })),
        ]
    );
    assert_eq!(
        undeclared("honors: { table: [align] }\nmain:\n  fields: {}\n"),
        vec![("main.body".to_string(), json!({ "construct": "table.widths", "count": 1 }))]
    );
    assert!(undeclared("honors: { table: [align, widths] }\nmain:\n  fields: {}\n").is_empty());
}

#[test]
fn a_backend_declining_tables_raises_no_undeclared_knob() {
    let quill = crate::quill::quill_from_yaml(
        "quill: { name: f, version: 1.0.0, backend: acroform, description: x }\nmain:\n  fields: {}\n",
    );
    let md = "~~~\n$quill: f@1.0.0\n$kind: main\n~~~\n\n<quill-table align=\"center\">\n\n| a |\n| --- |\n| 1 |\n\n</quill-table>\n";
    let doc = Document::parse(md).expect("parses").document;
    let codes: Vec<_> = quill.validate(&doc).into_iter().filter_map(|d| d.code).collect();
    assert!(codes.contains(&"validation::declined_construct".to_string()), "{codes:?}");
    assert!(!codes.contains(&"validation::undeclared_construct".to_string()), "{codes:?}");
}
