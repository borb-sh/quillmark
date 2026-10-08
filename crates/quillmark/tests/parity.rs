//! The parity corpus, `crates/fixtures/resources/parity/parity.json`, asserted
//! on every surface the engine owns (`prose/canon/PARITY.md` § "The corpus").

#![cfg(feature = "typst")]

use std::collections::BTreeSet;

use quillmark::{Diagnostic, Document, OutputFormat, Quill, Quillmark, RenderOptions};
use quillmark_content::{
    export::{to_markdown, to_markdown_annotated},
    import::{from_markdown, ImportWarning},
    model::MarkKind,
    serial,
};
use quillmark_fixtures::{quills_path, resource_path};
use serde_json::{json, Value};

mod common;

/// The quill declaring no knob, then the one declaring every knob.
const QUILLS: [&str; 2] = ["table_demo", "table_honors"];

fn frontmatter(quill: &str) -> String {
    format!("~~~\n$quill: {quill}@0.1.0\n$kind: main\ntitle: Parity\n~~~\n")
}

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
    let quills = QUILLS.map(|name| {
        quillmark::quill_from_path(quills_path(name)).unwrap_or_else(|e| panic!("{name} loads: {e:?}"))
    });
    let failures: Vec<String> = corpus
        .iter()
        .flat_map(|entry| {
            let name = entry["name"].as_str().unwrap_or_default();
            check(entry, &engine, &quills)
                .into_iter()
                .map(move |f| format!("{name}: {f}"))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// What an entry expects of one quill's surfaces: its own `typst` and
/// `signals`, or, for the declaring quill, its `declared` override of each.
struct Expected<'a> {
    typst: &'a Value,
    render: &'a Value,
    validate: Value,
}

impl<'a> Expected<'a> {
    fn of(entry: &'a Value, declaring: bool) -> Self {
        let declared = declaring.then(|| entry.get("declared")).flatten();
        let typst = declared.and_then(|d| d.get("typst")).unwrap_or(&entry["typst"]);
        let signals = declared.and_then(|d| d.get("signals")).unwrap_or(&entry["signals"]);
        Expected {
            typst,
            render: &signals["render"],
            validate: signals.get("validate").cloned().unwrap_or(json!([])),
        }
    }
}

fn check(entry: &Value, engine: &Quillmark, quills: &[Quill; 2]) -> Vec<String> {
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
    if let Some(annotated) = entry.get("annotated") {
        failures.extend(check_annotated(annotated, &content));
    }

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

            let parsed = match Document::parse(&format!("{}\n{markdown}\n", frontmatter(QUILLS[0]))) {
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
            let mut doc = Document::parse(&frontmatter(QUILLS[0])).expect("frontmatter parses").document;
            doc.main_mut().overwrite_body(content);
            doc
        }
    };

    for (quill, declaring) in quills.iter().zip([false, true]) {
        let mut doc = doc.clone();
        if declaring {
            let mut bound = Document::parse(&frontmatter(quill.name())).expect("frontmatter parses").document;
            bound.main_mut().overwrite_body(doc.main().body().clone());
            doc = bound;
        }
        let expected = Expected::of(entry, declaring);
        failures.extend(
            surfaces(&expected, engine, quill, &doc)
                .into_iter()
                .map(|f| format!("{}: {f}", quill.name())),
        );
    }
    failures
}

/// The lowering, `validate` and a one-shot render of `doc` through `quill`.
fn surfaces(expected: &Expected, engine: &Quillmark, quill: &Quill, doc: &Document) -> Vec<String> {
    let mut failures = Vec::new();
    let lowering = match lowering(quill, doc) {
        Ok(l) => l,
        Err(e) => return vec![e],
    };
    let Some(properties) = expected.typst.as_array() else {
        return vec!["typst is not an array".into()];
    };
    for property in properties {
        let Some(property) = property.as_str() else {
            failures.push(format!("typst holds a non-string {property}"));
            continue;
        };
        if !lowering.contains(property) {
            failures.push(format!("lowering lacks {property:?}:\n{lowering}"));
        }
    }

    let validated = quill.validate(doc);
    let codes: Value = validated.iter().filter_map(|d| d.code.clone()).collect();
    if codes != expected.validate {
        failures.push(format!("validate warns {codes}"));
    }

    match engine.render(
        quill,
        doc,
        common::test_date(),
        &RenderOptions::default().with_output_format(OutputFormat::Svg),
    ) {
        Ok(result) => {
            let codes: Value = result.warnings.iter().filter_map(|d| d.code.clone()).collect();
            if codes != *expected.render {
                failures.push(format!("render warns {codes}"));
            }
            let at_validate = declines(&validated, "validation::declined_construct");
            let at_render = declines(&result.warnings, "backend::declined_construct");
            if at_validate != at_render {
                failures.push(format!(
                    "validate declines {at_validate:?} where the render declines {at_render:?}"
                ));
            }
        }
        Err(e) => failures.push(format!("render fails: {e:?}")),
    }
    failures
}

/// `annotated` is the content's annotated read, and imports, warning nothing,
/// as the content without its anchors.
fn check_annotated(annotated: &Value, content: &quillmark::Normalized) -> Vec<String> {
    let Some(annotated) = annotated.as_str() else {
        return vec!["annotated is not a string".into()];
    };
    let mut failures = Vec::new();
    let read = to_markdown_annotated(content).markdown;
    if read != annotated {
        failures.push(format!("to_markdown_annotated writes {read:?}"));
    }
    let mut unanchored = content.clone().into_content();
    unanchored.marks.retain(|m| !matches!(m.kind, MarkKind::Anchor { .. }));
    match from_markdown(annotated) {
        Ok(i) if i.content == unanchored.into_normalized() && i.warnings.is_empty() => {}
        Ok(i) => failures.push(format!(
            "annotated imports as {}, warning {:?}",
            canonical(&i.content),
            i.warnings
        )),
        Err(e) => failures.push(format!("annotated does not import: {e}")),
    }
    failures
}

fn declines(diags: &[Diagnostic], code: &str) -> Vec<(String, String, String)> {
    let mut declines: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some(code))
        .map(|d| {
            let arg = |k: &str| d.args.get(k).map(|v| v.to_string()).unwrap_or_default();
            (d.path.clone().unwrap_or_default(), arg("construct"), arg("count"))
        })
        .collect();
    declines.sort();
    declines
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
/// writes: the `_qm_cN` block the data literal's `$body` names, from its `#let`
/// to the next top-level `#let` or doc comment. A blank body lowers to `""`
/// and has no block.
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
    let id = match lib.split_once("\"$body\": _qm_c") {
        Some((_, rest)) => {
            let n = rest.bytes().take_while(u8::is_ascii_digit).count();
            format!("_qm_c{}", &rest[..n])
        }
        None if lib.contains("\"$body\": \"\"") => return Ok(String::new()),
        None => return Err("the helper's data literal names no `$body` block".into()),
    };
    let at = lib
        .find(&format!("#let {id} = [\n"))
        .ok_or_else(|| format!("the helper binds no `{id}` block"))?;
    let block = &lib[at..];
    let end = ["\n#let ", "\n///"]
        .iter()
        .filter_map(|stop| block[1..].find(stop).map(|i| i + 1))
        .min()
        .unwrap_or(block.len());
    Ok(block[..end].to_string())
}
