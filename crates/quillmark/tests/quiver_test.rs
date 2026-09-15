//! The quill authoring contract: every quill in the fixtures quiver loads, and
//! each of its three canonical documents renders through the quill's own
//! template. A Typst plate is compiled code no unit test reaches, so a plate
//! edit is caught by these sweeps or not at all. The quill list is read from
//! the fixtures directory rather than spelled out, so a new fixture is covered
//! by existing.
//!
//! The three documents reach a template with different cells filled, and none
//! subsumes another. An empty document is the type-minimal valid input, so a
//! template that renders it degrades gracefully on any valid input. The
//! blueprint commits every `default:` and leaves every defaultless cell bare
//! under a `!must_fill` marker. The seed carries one card per declared kind,
//! commits every `example:` at its resting form, and omits every defaulted
//! field.
//!
//! A fourth document is authored rather than generated: the example a quill
//! declares under `quill.example`. The three above cannot drift from the
//! schema, because the schema emits them; an example can, so it is swept the
//! hardest — the sweep is the whole reason the file is safe to ship.

#![cfg(feature = "typst")]

use quillmark::{Document, OutputFormat, Quill, Quillmark, RenderOptions};
use quillmark_core::version::{Version, VersionSelector};
use quillmark_fixtures::{quill_names, quills_path};
use std::str::FromStr;
use std::sync::LazyLock;

static ENGINE: LazyLock<Quillmark> = LazyLock::new(Quillmark::new);

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
        let config = quill.config();
        let markdown = format!(
            "~~~\n$quill: {}@{}\n$kind: main\n~~~\n",
            config.name, config.version
        );
        let parsed = Document::parse(&markdown)
            .unwrap_or_else(|e| {
                panic!("quill '{name}' empty document failed to parse: {e:?}\n---\n{markdown}")
            })
            .document;

        let rendered = ENGINE
            .render(
                quill,
                &parsed,
                &RenderOptions::default().with_output_format(OutputFormat::Pdf),
            )
            .unwrap_or_else(|e| panic!("quill '{name}' failed to render: {e:?}\n---\n{markdown}"));
        assert!(
            !rendered.artifacts.is_empty(),
            "quill '{name}': render produced no artifacts"
        );
    }
}

#[test]
fn every_quill_blueprint_round_trips_and_renders() {
    for (name, quill) in QUIVER.iter() {
        let bp = quill.config().blueprint();
        let doc1 = Document::parse(&bp)
            .unwrap_or_else(|e| {
                panic!("quill '{name}' blueprint failed to parse: {e:?}\n---\n{bp}")
            })
            .document;
        let doc2 = Document::parse(&doc1.to_markdown())
            .unwrap_or_else(|e| panic!("quill '{name}' blueprint re-emit failed to parse: {e:?}"))
            .document;
        assert_eq!(doc1, doc2, "quill '{name}': blueprint must round-trip");

        ENGINE
            .render(
                quill,
                &doc1,
                &RenderOptions::default().with_output_format(OutputFormat::Pdf),
            )
            .unwrap_or_else(|e| {
                panic!("quill '{name}' blueprint failed to render: {e:?}\n---\n{bp}")
            });
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
                &RenderOptions::default().with_output_format(format),
            )
            .unwrap_or_else(|e| panic!("{name} failed to render its seed to {format:?}: {e:?}"));

        assert!(
            rendered.artifacts.first().is_some_and(|a| !a.bytes.is_empty()),
            "{name} rendered no {format:?} bytes"
        );
    }
}

/// A declared example is the quill's own worked instance, so it is held to
/// everything a reader takes it for: it parses clean, it pins this quill at
/// this version, it answers every obligation the schema states, and it renders
/// through the quill's own template. An example failing any of these teaches a
/// shape the quill no longer accepts, which is worse than shipping none.
///
/// The file is optional and a quill declaring none is skipped, so the sweep
/// asserts it saw at least one: a quiver that stopped declaring examples would
/// otherwise pass by vacuum.
#[test]
fn every_declared_example_binds_validates_and_renders() {
    let mut swept = 0;
    for (name, quill) in QUIVER.iter() {
        let Some(example) = quill.example() else {
            continue;
        };
        swept += 1;
        let config = quill.config();

        let parsed = Document::parse(example).unwrap_or_else(|e| {
            panic!("quill '{name}' example failed to parse: {e:?}\n---\n{example}")
        });
        assert!(
            parsed.warnings.is_empty(),
            "quill '{name}' example parsed with warnings: {:?}",
            parsed.warnings
        );
        let doc = parsed.document;

        let reference = doc
            .main()
            .quill()
            .unwrap_or_else(|| panic!("quill '{name}' example carries no `$quill` line"));
        assert_eq!(
            reference.name, config.name,
            "quill '{name}' example binds to a different quill"
        );
        let version = Version::from_str(&config.version).expect("a loaded quill's version parses");
        assert_eq!(
            reference.selector,
            VersionSelector::Exact(version),
            "quill '{name}' example must pin `$quill: {}@{}` exactly: an example documents one \
             version's shape, and a loose or stale selector is how it outlives it",
            config.name,
            config.version
        );

        let diags = quill.validate(&doc);
        assert!(
            diags.is_empty(),
            "quill '{name}' example must validate clean; a `!must_fill` marker or unanswered \
             obligation means it is a blueprint, not an example: {diags:?}"
        );

        let format = ENGINE
            .supported_formats(quill)
            .unwrap_or_else(|e| panic!("{name}'s backend should resolve: {e:?}"))
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{name}'s backend declares no output format"));
        let rendered = ENGINE
            .render(
                quill,
                &doc,
                &RenderOptions::default().with_output_format(format),
            )
            .unwrap_or_else(|e| {
                panic!("quill '{name}' example failed to render: {e:?}\n---\n{example}")
            });
        assert!(
            rendered.artifacts.first().is_some_and(|a| !a.bytes.is_empty()),
            "quill '{name}' example rendered no {format:?} bytes"
        );
    }
    assert!(
        swept > 0,
        "no fixture quill declares `quill.example`; this sweep proves nothing"
    );
}
