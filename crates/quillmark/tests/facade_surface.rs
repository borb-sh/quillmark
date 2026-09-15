//! Compile gate on the `quillmark` re-export list: this file names only
//! `quillmark::`, and annotates every binding explicitly, so a type dropping
//! out of `crates/quillmark/src/lib.rs` stops it compiling.

use std::collections::HashMap;

use quillmark::{
    BoundParseError, CardReader, Delta, Document, EditError, FileTreeNode, ImportError, Normalized,
    Parsed, Quill, QuillReference, QuillValue, TypedReader, TypedWriter,
};

const QUILL: &str = r#"
quill:
  name: facade_surface
  version: "1.0"
  backend: typst
  description: Facade re-export surface test

main:
  fields:
    title:
      type: string
    subject:
      type: richtext
      inline: true

card_kinds:
  note:
    fields:
      body:
        type: richtext
"#;

fn quill() -> Quill {
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: QUILL.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("from_tree")
}

#[test]
fn authoring_spells_through_the_facade() {
    let quill = quill();
    let reference: QuillReference = "facade_surface".parse().expect("reference parses");
    let mut doc = Document::new(reference);

    let mut writer: TypedWriter = quill.writer(&mut doc);
    let written: Result<(), EditError> = writer.set("title", "Hello");
    written.expect("title is a declared string field");

    let title: Option<&QuillValue> = doc.main().payload().get("title");
    assert_eq!(title.and_then(|v| v.as_str()), Some("Hello"));
}

#[test]
fn bound_parse_spells_through_the_facade() {
    let quill = quill();
    let md = "~~~\n$quill: facade_surface\n$kind: main\ntitle: Hello\n~~~\n\n# Body\n";

    let parsed: Result<Parsed, BoundParseError> = quill.parse(md);
    let parsed = parsed.expect("document matches the quill");
    assert_eq!(
        parsed
            .document
            .main()
            .payload()
            .get("title")
            .and_then(|v| v.as_str()),
        Some("Hello")
    );

    let elsewhere = md.replace("facade_surface", "other_quill");
    let mismatch: Result<Parsed, BoundParseError> = quill.parse(&elsewhere);
    assert!(mismatch.is_err(), "a $quill naming another quill fails");
}

#[test]
fn typed_read_spells_through_the_facade() {
    let quill = quill();
    let reference: QuillReference = "facade_surface".parse().expect("reference parses");
    let mut doc = Document::new(reference);
    {
        let mut writer: TypedWriter = quill.writer(&mut doc);
        writer.set("subject", "Hello **world**").expect("richtext");
        writer
            .add_card("note", [("body", "a *card*")], None, None)
            .expect("note is a declared card kind");
    }

    let reader: TypedReader = quill.reader(&doc);

    let subject: Option<QuillValue> = reader.get("subject").expect("subject is declared");
    assert_eq!(subject.as_ref().and_then(|v| v.as_str()), Some("Hello **world**"));

    let typo: Result<Option<QuillValue>, EditError> = reader.get("nope");
    assert!(matches!(typo, Err(EditError::UnknownField { .. })), "{typo:?}");

    let card: CardReader = reader.card(0).expect("the note card resolves its schema");
    assert_eq!(card.kind(), Some("note"));
    let body: Option<QuillValue> = card.get("body").expect("body is declared on `note`");
    assert_eq!(body.as_ref().and_then(|v| v.as_str()), Some("a *card*"));
}

#[test]
fn content_lane_spells_through_the_facade() {
    let quill = quill();
    let md = "~~~\n$quill: facade_surface\n$kind: main\nsubject: Hello **world**\n~~~\n\n# Body\n";
    let mut doc = quill.parse(md).expect("document matches the quill").document;

    let body: &Normalized = doc.main().body();
    assert_eq!(body.text, "Body", "the body rests as content, not as markdown");

    let reader: TypedReader = quill.reader(&doc);
    let read: Option<Normalized> = reader.get_content("subject").expect("subject is declared");
    let subject: Normalized = read.expect("the parsed richtext field is present");
    assert_eq!(subject.text, "Hello world", "the emphasis rides a mark, not the text");
    assert_eq!(subject.marks.len(), 1);

    let over_nested = "> ".repeat(200) + "deep";
    let refusal: Result<Delta, EditError> = doc.main_mut().revise_body(over_nested);
    let Err(EditError::Import(ImportError::NestingTooDeep { depth, max })) = refusal else {
        panic!("markdown nested past the codec's limit is refused, not imported: {refusal:?}");
    };
    assert!(depth > max, "the refusal names the depth that passed the limit");
}

#[cfg(feature = "typst")]
#[test]
fn preview_regions_spell_through_the_facade() {
    use quillmark::{ContentHit, HitGranularity, LiveSession, Quillmark, RenderedRegion};

    let engine = Quillmark::new();
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path("usaf_memo"))
        .expect("usaf_memo should load");
    let parsed = quill.seed_document();
    let session: LiveSession = engine.open(&quill, &parsed).expect("open a session");

    let regions: Vec<RenderedRegion> = session.regions();
    // `field_boxes` and `position_at` are content-only, so the query needs a
    // span-bearing region rather than a scalar-reference or widget one.
    let region: &RenderedRegion = regions
        .iter()
        .find(|r| r.span.is_some())
        .expect("the seeded memo places at least one content field");

    let boxes: Vec<RenderedRegion> = session.field_boxes(&region.field);
    assert!(!boxes.is_empty(), "a content field unions to at least one box");

    let cx = (region.rect[0] + region.rect[2]) / 2.0;
    let cy = (region.rect[1] + region.rect[3]) / 2.0;
    assert_eq!(session.field_at(region.page, cx, cy, 0.0).as_deref(), Some(region.field.as_str()));

    let hit: Option<ContentHit> = session.position_at(region.page, cx, cy, 0.0);
    let hit: ContentHit = hit.expect("a content region's centre hit-tests to a content position");
    assert_eq!(hit.field, region.field);
    let _granularity: Option<HitGranularity> = hit.granularity;

    let located: Option<RenderedRegion> = session.locate(&region.field, 0);
    assert!(located.is_some(), "a field with a region locates position 0");
}

/// A server holds one `Quillmark` and renders on many threads.
#[test]
fn engine_and_inputs_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<quillmark::Quillmark>();
    assert_send_sync::<Quill>();
    assert_send_sync::<Document>();
}
