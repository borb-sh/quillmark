//! The parity corpus, `crates/fixtures/resources/parity/parity.json`, asserted
//! on every surface the engine owns (`prose/canon/PARITY.md` § "The corpus").

#![cfg(feature = "typst")]

use std::collections::BTreeSet;

use quillmark::{Diagnostic, Document, OutputFormat, Quill, Quillmark, RenderOptions};
use quillmark_content::{
    export::to_markdown,
    import::{from_markdown, ImportWarning},
    serial,
};
use quillmark_fixtures::{quills_path, resource_path};
use serde_json::{json, Value};

mod common;

const FRONTMATTER: &str = "~~~\n$quill: table_demo@0.1.0\n$kind: main\ntitle: Parity\n~~~\n";

#[test]
fn every_entry_holds_on_every_surface() {
    let corpus: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(resource_path("parity/parity.json")).expect("corpus reads"),
    )
    .expect("corpus is a JSON array");
    assert!(!corpus.is_empty(), "the corpus holds no entry");

    let mut names = BTreeSet::new();
    for entry in &corpus {
        let name = entry["name"].as_str().expect("every entry has a name");
        assert!(names.insert(name), "two entries named {name}");
    }

    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quills_path("table_demo")).expect("table_demo loads");
    let failures: Vec<String> = corpus
        .iter()
        .flat_map(|entry| {
            let name = entry["name"].as_str().unwrap_or_default();
            check(entry, &engine, &quill)
                .into_iter()
                .map(move |f| format!("{name}: {f}"))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn check(entry: &Value, engine: &Quillmark, quill: &Quill) -> Vec<String> {
    let mut failures = Vec::new();
    let stored = &entry["content"];
    let content = match serial::from_canonical_value(stored) {
        Ok(c) => c,
        Err(e) => return vec![format!("content does not decode: {e}")],
    };
    if serial::to_canonical_value(&content) != *stored {
        failures.push(format!("content is not canonical: {}", canonical(&content)));
    }
    let import_signals = &entry["signals"]["import"];

    let doc = match entry["markdown"].as_str() {
        Some(markdown) => {
            let imported = match from_markdown(markdown) {
                Ok(i) => i,
                Err(e) => return vec![format!("import fails: {e}")],
            };
            if serial::to_canonical_value(&imported.content) != *stored {
                failures.push(format!("imports as {}", canonical(&imported.content)));
            }
            let warnings: Value = imported
                .warnings
                .iter()
                .map(|ImportWarning::DroppedConstruct { construct, count }| {
                    json!({ "construct": construct, "count": count })
                })
                .collect();
            if warnings != *import_signals {
                failures.push(format!("import warns {warnings}"));
            }
            match from_markdown(&to_markdown(&content)) {
                Ok(again) if again.content == content => {}
                Ok(again) => failures.push(format!(
                    "to_markdown is no fixed point: re-imports as {}",
                    canonical(&again.content)
                )),
                Err(e) => failures.push(format!("to_markdown does not re-import: {e}")),
            }

            let parsed = match Document::parse(&format!("{FRONTMATTER}\n{markdown}\n")) {
                Ok(p) => p,
                Err(e) => return [failures, vec![format!("a body does not parse: {e}")]].concat(),
            };
            if parsed.document.main().body() != &content {
                failures.push(format!(
                    "a body imports as {}",
                    canonical(parsed.document.main().body())
                ));
            }
            let dropped = dropped_constructs(&parsed.warnings);
            if dropped != *import_signals {
                failures.push(format!("a parse warns {dropped}"));
            }
            parsed.document
        }
        None => {
            if import_signals.as_array().is_none_or(|a| !a.is_empty()) {
                failures.push("an entry with no markdown declares import signals".into());
            }
            let mut doc = Document::parse(FRONTMATTER).expect("frontmatter parses").document;
            doc.main_mut().overwrite_body(content);
            doc
        }
    };

    let lowering = match lowering(quill, &doc) {
        Ok(l) => l,
        Err(e) => return [failures, vec![e]].concat(),
    };
    for property in entry["typst"].as_array().into_iter().flatten() {
        let property = property.as_str().unwrap_or_default();
        if !lowering.contains(property) {
            failures.push(format!("lowering lacks {property:?}:\n{lowering}"));
        }
    }

    match engine.render(
        quill,
        &doc,
        common::test_date(),
        &RenderOptions::default().with_output_format(OutputFormat::Svg),
    ) {
        Ok(result) => {
            let codes: Value = result.warnings.iter().filter_map(|d| d.code.clone()).collect();
            if codes != entry["signals"]["render"] {
                failures.push(format!("render warns {codes}"));
            }
        }
        Err(e) => failures.push(format!("render fails: {e:?}")),
    }
    failures
}

fn canonical(content: &quillmark::Normalized) -> String {
    serial::to_canonical_value(content).to_string()
}

fn dropped_constructs(warnings: &[Diagnostic]) -> Value {
    warnings
        .iter()
        .filter(|d| d.code.as_deref() == Some("parse::dropped_construct"))
        .map(|d| json!({ "construct": d.args.get("construct"), "count": d.args.get("count") }))
        .collect()
}

/// The body's markup block in the generated helper, the one `emit_content`
/// writes: from its `#let` to the next top-level `#let` or doc comment.
fn lowering(quill: &Quill, doc: &Document) -> Result<String, String> {
    let data = quill
        .compile_checked(doc, common::test_date())
        .map_err(|e| format!("compile fails: {e:?}"))?;
    let workspace = quillmark::typst_workspace::workspace(quill, &data)
        .map_err(|e| format!("workspace fails: {e:?}"))?;
    let lib = workspace
        .files
        .iter()
        .find(|(path, _)| path.ends_with("lib.typ"))
        .map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned())
        .ok_or("the workspace holds no lib.typ")?;
    let Some(at) = lib.find("#let _qm_c0 = [\n") else {
        return Ok(String::new());
    };
    let block = &lib[at..];
    let end = ["\n#let ", "\n///"]
        .iter()
        .filter_map(|stop| block[1..].find(stop).map(|i| i + 1))
        .min()
        .unwrap_or(block.len());
    Ok(block[..end].to_string())
}
