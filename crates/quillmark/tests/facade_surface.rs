//! Compile gate on the re-exports `quillmark-cli` does not reach. This file
//! names only `quillmark::` and annotates every binding explicitly, so a type
//! dropping out of `crates/quillmark/src/lib.rs` stops it compiling.

use std::collections::HashMap;

use quillmark::{
    AnchorRead, AnchorSelector, Annotated, AnnotatedMarkdown, CalendarDate, CardReader, Delta, DocPath,
    Document, DocumentAnchor, DocumentRevised, EditError, ImportError, Normalized,
    Outline, OutlineCard, ParseDateError, Parsed, Quill, QuillReference, QuillValue, Reattached, Revised, TypedReader,
    TypedWriter,
};

mod common;

const QUILL: &str = r#"
quill:
  name: facade_surface
  version: "1.0.0"
  backend: typst
  description: Facade re-export surface test

main:
  fields:
    subject:
      type: richtext
      multiline: false

card_kinds:
  note:
    fields:
      prose:
        type: richtext
        multiline: true
"#;

fn quill() -> Quill {
    common::quill_from_files(&[("Quill.yaml", QUILL)])
}

#[test]
fn typed_write_and_read_spell_through_the_facade() {
    let quill = quill();
    let reference: QuillReference = "facade_surface".parse().expect("reference parses");
    let mut doc = Document::new(reference);
    {
        let mut writer: TypedWriter = quill.writer(&mut doc);
        writer.set("subject", "Hello **world**").expect("richtext");
        writer
            .add_card("note", [("prose", "a *card*")], None, None)
            .expect("note is a declared card kind");
    }

    let reader: TypedReader = quill.reader(&doc);

    let subject: Option<QuillValue> = reader.get("subject").expect("subject is declared");
    assert_eq!(subject.as_ref().and_then(|v| v.as_str()), Some("Hello **world**"));

    let typo: Result<Option<QuillValue>, EditError> = reader.get("nope");
    assert!(matches!(typo, Err(EditError::UnknownField { .. })), "{typo:?}");

    let card: CardReader = reader.card(0).expect("the note card resolves its schema");
    assert_eq!(card.kind(), Some("note"));
    let prose: Option<QuillValue> = card.get("prose").expect("prose is declared on `note`");
    assert_eq!(prose.as_ref().and_then(|v| v.as_str()), Some("a *card*"));
}

#[test]
fn content_lane_spells_through_the_facade() {
    let quill = quill();
    let md = "~~~\n$quill: facade_surface\n$kind: main\nsubject: Hello **world**\n~~~\n\n# Body\n";
    let parsed: Parsed = quill.parse(md).expect("document matches the quill");
    let mut doc = parsed.document;

    let _: &Normalized = doc.main().body();
    let reader: TypedReader = quill.reader(&doc);
    let _: Option<Normalized> = reader.get_content("subject").expect("subject is declared");
    let outline: Outline = reader.outline();
    let _: Option<&OutlineCard> = outline.cards.first();
    let _: Annotated = doc.main().body_annotated();

    let revised: Result<Revised, EditError> = doc.main_mut().revise_body("# Body");
    let Revised { delta, dropped_anchors, .. } = revised.expect("a heading imports");
    let _: Delta = delta;
    let _: Vec<AnchorRead> = dropped_anchors;
    let _: fn(ImportError) -> EditError = EditError::Import;

    let read: AnnotatedMarkdown = doc.to_markdown_annotated();
    let _: &[DocumentAnchor] = &read.anchors;

    let markdown = doc.to_markdown();
    let revised: DocumentRevised = quill
        .writer(&mut doc)
        .revise_document(&markdown)
        .expect("the document's own markdown revises");
    let _: &[DocumentAnchor] = &revised.dropped_anchors;
    let _: &[Option<usize>] = &revised.from;

    let selectors: Vec<AnchorSelector> = doc.anchor_contexts();
    let _: Option<AnchorSelector> = doc.anchor_context("c1");
    let deltas: HashMap<DocPath, Delta> = HashMap::new();
    let Reattached { reattached, orphaned, .. } = doc.reattach(&selectors, &deltas);
    let _: (Vec<AnchorSelector>, Vec<AnchorSelector>) = (reattached, orphaned);
}

#[test]
fn a_render_date_refusal_spells_through_the_facade() {
    let refused: Result<CalendarDate, ParseDateError> = "2026-02-30".parse();
    assert!(refused.is_err(), "February has no 30th");
}

#[cfg(feature = "typst")]
#[test]
fn preview_regions_spell_through_the_facade() {
    use quillmark::{
        ContentHit, Engine, FieldCoverage, HitGranularity, LiveSession, RenderedRegion,
    };

    let engine = Engine::new();
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("usaf_memo"))
        .expect("usaf_memo should load");
    let doc = quill.seed_document();
    let today: CalendarDate = "2026-03-14".parse().expect("a date");
    let session: LiveSession = engine.open(&quill, &doc, today).expect("open a session");

    let _: Vec<RenderedRegion> = session.regions();
    let _: Vec<RenderedRegion> = session.field_boxes("body");
    let _: Option<String> = session.field_at(0, 0.0, 0.0, 0.0);
    let hit: Option<ContentHit> = session.position_at(0, 0.0, 0.0, 0.0);
    let _: Option<Option<HitGranularity>> = hit.map(|h| h.granularity);
    let _: Option<RenderedRegion> = session.locate("body", 0);
    let _: Vec<FieldCoverage> = session.coverage();
}

/// A server holds one `Engine` and renders on many threads.
#[test]
fn engine_and_inputs_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<quillmark::Engine>();
    assert_send_sync::<Quill>();
    assert_send_sync::<Document>();
}
