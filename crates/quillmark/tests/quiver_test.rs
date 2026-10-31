//! The plate contract: every quill in the fixtures quiver loads, and
//! each of its canonical documents renders through the quill's own
//! template. A Typst plate is compiled code no unit test reaches, so a plate
//! edit is caught by these sweeps or not at all. The quill list is read from
//! the fixtures directory rather than spelled out, so a new fixture is covered
//! by existing.
//!
//! The documents reach a template with different cells filled, and neither
//! subsumes the other. An empty document is the type-minimal valid input, so a
//! template that renders it degrades gracefully on any valid input. The seeded
//! document carries one card per declared kind, each body empty.
//!
//! A quill's example document, where it declares examples, is the one document
//! with values in every cell that has one, and renders without a diagnostic.

#![cfg(feature = "typst")]

use quillmark::{Engine, OutputFormat, Quill, RenderOptions};
use quillmark_fixtures::{quill_names, quills_path};
use std::sync::LazyLock;

mod common;

static ENGINE: LazyLock<Engine> = LazyLock::new(Engine::new);

/// Every fixture quill, loaded once for the whole binary rather than re-read by
/// each sweep.
static QUIVER: LazyLock<Vec<(String, Quill)>> = LazyLock::new(|| {
    quill_names()
        .into_iter()
        .map(|name| {
            let quill = quillmark::quill_from_path(quills_path(&name))
                .unwrap_or_else(|e| panic!("quill '{name}' failed to load: {e:?}"));
            (name, quill)
        })
        .collect()
});

#[test]
fn every_quill_renders_an_empty_document() {
    for (name, quill) in QUIVER.iter() {
        let empty = quill.empty_document();

        let rendered = ENGINE
            .render(
                quill,
                &empty,
                common::test_date(),
                &RenderOptions::default().with_output_format(OutputFormat::Pdf),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "quill '{name}' failed to render: {e:?}\n---\n{}",
                    empty.to_markdown()
                )
            });
        assert!(
            !rendered.artifacts.is_empty(),
            "quill '{name}': render produced no artifacts"
        );
    }
}

/// Each canonical document exports to an authoring document its quill's own
/// schema accepts, and crosses back unchanged, so what a model reads of a
/// document is the document it edits.
#[test]
fn every_quill_round_trips_its_documents_through_the_authoring_form() {
    for (name, quill) in QUIVER.iter() {
        let schema = jsonschema::draft202012::new(&quill.config().authoring_schema())
            .unwrap_or_else(|e| panic!("{name}'s authoring schema does not compile: {e}"));
        let documents = [quill.empty_document(), quill.seed_document()]
            .into_iter()
            .chain(quill.example_document());
        for doc in documents {
            let authored = quill.to_authoring(&doc);
            let refusals: Vec<String> = schema
                .iter_errors(&authored)
                .map(|e| format!("{}: {e}", e.instance_path()))
                .collect();
            assert!(
                refusals.is_empty(),
                "{name}: its schema refuses its own export {authored}\n{refusals:#?}"
            );
            let imported = quill
                .from_authoring(&authored)
                .unwrap_or_else(|e| panic!("{name} failed to import {authored}: {e:?}"));
            assert_eq!(
                imported.document, doc,
                "{name}: the authoring form must round-trip\n---\n{authored}"
            );
            let mut invented = authored.clone();
            invented["fields"]["zz_invented"] = serde_json::json!("x");
            assert!(
                !schema.is_valid(&invented),
                "{name}: its schema admits a field the quill does not declare"
            );
        }
    }
}

#[test]
fn every_quill_renders_its_seed_document() {
    for (name, quill) in QUIVER.iter() {
        let format = ENGINE
            .supported_formats(quill)
            .unwrap_or_else(|e| panic!("{name}'s backend should resolve: {e:?}"))
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{name}'s backend declares no output format"));

        let rendered = ENGINE
            .render(
                quill,
                &quill.seed_document(),
                common::test_date(),
                &RenderOptions::default().with_output_format(format),
            )
            .unwrap_or_else(|e| {
                panic!("{name} failed to render its seeded document to {format:?}: {e:?}")
            });

        assert!(
            rendered
                .artifacts
                .first()
                .is_some_and(|a| !a.bytes.is_empty()),
            "{name} rendered no {format:?} bytes"
        );
    }
}

#[test]
fn every_quill_declares_every_key_its_canonical_documents_write() {
    for (name, quill) in QUIVER.iter() {
        for doc in [quill.seed_document()]
            .into_iter()
            .chain(quill.example_document())
        {
            let unknown: Vec<_> = quill
                .validate(&doc)
                .into_iter()
                .filter(|d| d.code.as_deref() == Some("validation::unknown_field"))
                .collect();
            assert!(
                unknown.is_empty(),
                "{name}: {unknown:?}\n---\n{}",
                doc.to_markdown()
            );
        }
    }
}

#[test]
fn every_quill_example_renders_clean() {
    for (name, quill) in QUIVER.iter() {
        let Some(example) = quill.example_document() else {
            continue;
        };
        let mut diagnostics = quill.validate(&example);
        let rendered = ENGINE
            .render(
                quill,
                &example,
                common::test_date(),
                &RenderOptions::default().with_output_format(OutputFormat::Pdf),
            )
            .unwrap_or_else(|e| panic!("{name}'s example failed to render: {e:?}"));
        diagnostics.extend(rendered.warnings);
        assert!(diagnostics.is_empty(), "{name}'s example: {diagnostics:?}");
    }
}
