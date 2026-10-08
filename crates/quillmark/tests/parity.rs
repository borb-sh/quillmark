//! The parity corpus, `crates/fixtures/resources/parity/parity.json`, asserted
//! on every surface the engine owns (`prose/canon/PARITY.md` § "The corpus").

#![cfg(feature = "typst")]

use std::collections::BTreeSet;

use quillmark::{Diagnostic, Document, Normalized, OutputFormat, Quill, Quillmark, RenderOptions};
use quillmark_content::{
    export::{to_markdown, to_markdown_annotated},
    import::{from_markdown, ImportWarning},
    model::{MarkKind, ISLAND_SLOT},
    ops::change_bundle_from_value,
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
    failures.extend(check_op_wire(stored, &content));
    let reimports = match entry.get("reimports").map(serial::from_canonical_value) {
        None => content.clone(),
        Some(Ok(c)) => c,
        Some(Err(e)) => return [failures, vec![format!("reimports does not decode: {e}")]].concat(),
    };
    failures.extend(check_fixed_point(&content, &reimports));
    failures.extend(check_revise(&content, &reimports, &to_markdown(&content)));
    let import_signals = &entry["signals"]["import"];
    if let Some(annotated) = entry.get("annotated") {
        failures.extend(check_annotated(annotated, &content));
        if let Some(annotated) = annotated.as_str() {
            failures.extend(
                check_revise(&content, &reimports, annotated)
                    .into_iter()
                    .map(|f| format!("annotated: {f}")),
            );
        }
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

/// `content` lands through the authored lane, and through one `ChangeBundle`
/// applied to an empty field: the text by `delta`, each island by an `insert` at
/// its slot, each line's kind, containers and `continues` by line ops, and every
/// mark by an `add`.
fn check_op_wire(stored: &Value, content: &Normalized) -> Vec<String> {
    let mut failures = Vec::new();
    match serial::from_authored_value(stored) {
        Ok(authored) if authored == *content => {}
        Ok(authored) => failures.push(format!("overwrite stores {}", canonical(&authored))),
        Err(e) => failures.push(format!("overwrite refuses: {e}")),
    }
    let bundle = match change_bundle_from_value(&bundle(stored)) {
        Ok(b) => b,
        Err(e) => return [failures, vec![format!("the bundle does not decode: {e}")]].concat(),
    };
    let mut applied = Normalized::empty();
    match applied.apply_field_change(&bundle) {
        Ok(()) if applied == *content => {}
        Ok(()) => failures.push(format!("the bundle lands {}", canonical(&applied))),
        Err(e) => failures.push(format!("the bundle refuses: {e:?}")),
    }
    failures
}

/// The wire bundle that authors `stored` onto an empty field.
fn bundle(stored: &Value) -> Value {
    let text = stored["text"].as_str().unwrap_or_default();
    let prose: String = text.chars().filter(|&c| c != ISLAND_SLOT).collect();
    let delta: Vec<Value> = [prose]
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(|p| json!({ "insert": p }))
        .collect();
    let slots = text.chars().enumerate().filter(|&(_, c)| c == ISLAND_SLOT).map(|(at, _)| at);
    let island_ops: Vec<Value> = slots
        .zip(stored["islands"].as_array().into_iter().flatten())
        .map(|(at, island)| with(island, json!({ "op": "insert", "at": at })))
        .collect();
    let line_ops: Vec<Value> = stored["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(line, l)| {
            let mut ops = vec![
                with(
                    &json!({ "kind": l["kind"], "attrs": l.get("attrs") }),
                    json!({ "op": "setKind", "line": line }),
                ),
                json!({ "op": "setContainers", "line": line, "containers": l["containers"] }),
            ];
            if let Some(continues) = l.get("continues") {
                ops.push(json!({ "op": "setContinues", "line": line, "continues": continues }));
            }
            ops
        })
        .collect();
    let mark_ops: Vec<Value> = stored["marks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| with(m, json!({ "op": "add" })))
        .collect();
    json!({
        "delta": { "ops": delta },
        "islandOps": island_ops,
        "lineOps": line_ops,
        "markOps": mark_ops,
    })
}

/// `object` with `keys` merged in, dropping a `null` value.
fn with(object: &Value, keys: Value) -> Value {
    let mut merged = object.as_object().cloned().unwrap_or_default();
    merged.extend(keys.as_object().cloned().unwrap_or_default());
    merged.retain(|_, v| !v.is_null());
    Value::Object(merged)
}

/// `to_markdown(content)` re-imports, warning nothing, to `reimports`: the
/// fixed point, and where markdown cannot spell the row, what the round trip
/// lands on.
fn check_fixed_point(content: &Normalized, reimports: &Normalized) -> Vec<String> {
    match from_markdown(&to_markdown(content)) {
        Ok(again) if again.content == *reimports && again.warnings.is_empty() => vec![],
        Ok(again) => vec![format!(
            "to_markdown re-imports as {}, warning {:?}",
            canonical(&again.content),
            again.warnings
        )],
        Err(e) => vec![format!("to_markdown does not re-import: {e}")],
    }
}

/// A body holding `content`, revised with `markdown`, lands on `reimports` plus
/// every anchor `content` holds, warning nothing.
fn check_revise(content: &Normalized, reimports: &Normalized, markdown: &str) -> Vec<String> {
    let mut expected = reimports.clone().into_content();
    expected.marks.extend(
        content
            .clone()
            .into_content()
            .marks
            .into_iter()
            .filter(|m| matches!(m.kind, MarkKind::Anchor { .. })),
    );
    let expected = expected.into_normalized();
    let mut doc = Document::parse(&frontmatter(QUILLS[0])).expect("frontmatter parses").document;
    doc.main_mut().overwrite_body(content.clone());
    match doc.main_mut().revise_body(markdown) {
        Ok(revised) if doc.main().body() == &expected && revised.warnings.is_empty() => vec![],
        Ok(revised) => vec![format!(
            "a revise lands {}, warning {:?}",
            canonical(doc.main().body()),
            revised.warnings
        )],
        Err(e) => vec![format!("a revise fails: {e:?}")],
    }
}

/// `annotated` is the content's annotated read, and imports, warning nothing,
/// as the content without its anchors.
fn check_annotated(annotated: &Value, content: &Normalized) -> Vec<String> {
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

fn canonical(content: &Normalized) -> String {
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
