use serde_json::{Value, json};

use crate::document::{Document, STORAGE_V0_129_0, STORAGE_V0_135_0, peek_storage_version};

use super::parse;

fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        v => v,
    }
}

fn summary(row: &mut Value) -> &mut Value {
    let items = row["main"]["payload"]["items"]
        .as_array_mut()
        .expect("payload items");
    let item = items
        .iter_mut()
        .find(|i| i["key"] == "summary")
        .expect("the summary field");
    &mut item["value"]
}

/// A row as 0.130 saved it: the body's table and the `summary` field's table
/// each spelled `headless: true`, with no `head`.
fn saved_by_0_130() -> Value {
    let doc =
        parse("~~~card-yaml\n$quill: q@1.0\n$kind: main\nsummary: x\n~~~\n\n| h |\n|---|\n| c |\n");
    let mut row = serde_json::to_value(&doc).expect("the document serializes");
    row["schema"] = json!(STORAGE_V0_129_0);
    let mut body = quillmark_content::lines::serial::to_canonical_value(doc.main().body_lines());
    body["islands"][0]["props"]["headless"] = json!(true);
    let body = sorted(body);
    *summary(&mut row) = body.clone();
    row["main"]["body"] = body;
    row
}

fn table_props(content: &Value) -> &Value {
    &content["islands"][0]["props"]
}

/// A richtext field holding a 0.130 `headless: true` table is content: it
/// exports as a table, where it would write its YAML tree, and a markdown
/// edit re-imports it as content.
#[test]
fn a_fields_headless_table_from_0_130_reads_as_content() {
    let mut doc: Document = serde_json::from_value(saved_by_0_130()).expect("the row loads");
    let md = doc.to_markdown();
    assert!(!md.contains("islands:"), "the field emitted as YAML:\n{md}");
    assert_eq!(md.matches("<table>").count(), 2, "{md}");

    let revised = doc.revise(&md).expect("the export revises");
    assert!(revised.dropped_anchors.is_empty());
    let mut row = serde_json::to_value(&doc).expect("the document serializes");
    let field = summary(&mut row);
    assert!(field.is_object(), "the field re-imported as {field}");
    assert_eq!(table_props(field)["head"], json!(0));
}

/// A 0.130 `headless: true` table resaves under the newest tag as `head: 0`,
/// and the resave is a fixed point.
#[test]
fn a_headless_table_resaves_as_head_0() {
    let doc: Document = serde_json::from_value(saved_by_0_130()).expect("the row loads");
    let saved = serde_json::to_string(&doc).expect("the document serializes");
    assert_eq!(
        peek_storage_version(&saved).as_deref(),
        Some(STORAGE_V0_135_0)
    );
    let row: Value = serde_json::from_str(&saved).unwrap();
    let props = table_props(&row["main"]["body"]);
    assert_eq!((&props["head"], &props["headless"]), (&json!(0), &Value::Null));

    let again: Document = serde_json::from_str(&saved).expect("the resave loads");
    assert_eq!(serde_json::to_string(&again).unwrap(), saved);
}
