//! The schema a reference quill projects, against a golden file. It lives here
//! rather than beside `QuillConfig` because reading a quill off the filesystem
//! is `quillmark::quill_from_path`'s job: core is filesystem-agnostic.
//!
//! `UPDATE_GOLDEN=1 cargo test -p quillmark --test schema_snapshot` rewrites it.

use std::fs;

#[test]
fn schema_snapshot_usaf_memo_0_2_0() {
    let quill = quillmark::quill_from_path(quillmark_fixtures::resource_path(
        "quills/usaf_memo/0.2.0",
    ))
    .expect("load usaf_memo fixture");
    let config = quill.config();

    let yaml = config.schema_yaml().expect("project the schema");
    let golden_path =
        quillmark_fixtures::resource_path("quills/usaf_memo/0.2.0/__golden__/schema.yaml");
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        fs::write(&golden_path, &yaml).expect("write golden");
    }
    assert_eq!(
        yaml,
        fs::read_to_string(&golden_path).expect("read golden"),
        "schema.yaml drifted"
    );

    let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).expect("parse yaml");
    assert_eq!(config.schema(), parsed, "schema.yaml json/yaml parity");
    assert!(parsed.get("main").and_then(|v| v.get("fields")).is_some());
    assert!(parsed.get("card_kinds").is_some());
    assert!(parsed.get("ref").is_none() && parsed.get("example").is_none());
    assert!(yaml.contains("ui:"), "schema.yaml must include ui hints");
}
