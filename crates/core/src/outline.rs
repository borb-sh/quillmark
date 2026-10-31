//! The schema-bound outline: what a document holds and what it still owes,
//! addressed, without its text. A writer reads it to choose the one card or
//! field it reads and writes next, at that address's scoped annotated read.

use quillmark_content::model::{LineKind, Normalized, ISLAND_SLOT};
use quillmark_content::serial::from_canonical_value;
use serde_json::Value as JsonValue;

use crate::document::{Card, Document};
use crate::path::DocPath;
use crate::quill::validation::validate_document_values;
use crate::quill::{CardSchema, FieldType, QuillConfig};

/// The outline [`TypedReader::outline`](crate::reader::TypedReader::outline)
/// reads: the main card, then each composable card in order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Outline {
    pub cards: Vec<OutlineCard>,
}

/// One card of an [`Outline`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutlineCard {
    /// `main`, or `cards.<kind>[<i>]`.
    pub path: DocPath,
    /// The card's `$kind`, `main` on the main card.
    pub kind: String,
    /// The card's `$id`; `None` on the main card.
    pub id: Option<String>,
    /// Each field the card's schema declares, in display order. A card of a
    /// kind the quill does not declare lists none.
    pub fields: Vec<OutlineField>,
    /// Each heading of the body, in order.
    pub headings: Vec<OutlineHeading>,
    /// Each anchor of the card's fields and body, in the order
    /// [`Document::anchor_contexts`] reads them.
    pub anchors: Vec<OutlineAnchor>,
}

/// One declared field of an [`OutlineCard`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutlineField {
    pub name: String,
    pub path: DocPath,
    pub state: FieldState,
}

/// Whether a declared field is answered: the outline's pending signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldState {
    /// It holds a value the schema accepts.
    Ok,
    /// It is absent, null, or holds a blank string, an empty list or map, or a
    /// blank content.
    Empty,
    /// It holds a value [`Quill::validate`](crate::quill::Quill::validate)
    /// reports a `validation::*` error at or under its path.
    Invalid,
}

impl FieldState {
    pub fn as_str(self) -> &'static str {
        match self {
            FieldState::Ok => "ok",
            FieldState::Empty => "empty",
            FieldState::Invalid => "invalid",
        }
    }
}

/// One heading of a body: its line in the body's content and its text, island
/// slots removed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutlineHeading {
    pub line: usize,
    pub level: u8,
    pub text: String,
}

/// One anchor of an [`OutlineCard`], by id and the value holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutlineAnchor {
    pub id: String,
    pub path: DocPath,
}

pub(crate) fn outline(config: &QuillConfig, doc: &Document) -> Outline {
    let errors: Vec<String> = validate_document_values(config, doc, &mut Vec::new())
        .iter()
        .map(|e| e.path().to_string())
        .collect();
    let anchors = doc.anchor_contexts();
    let main = (DocPath::main(), doc.main(), Some(&config.main));
    let cards = doc.cards().iter().enumerate().map(|(i, card)| {
        let schema = card.kind().and_then(|kind| config.card_kind(kind));
        (DocPath::card(card.kind(), i), card, schema)
    });
    let cards = std::iter::once(main)
        .chain(cards)
        .map(|(path, card, schema)| OutlineCard {
            kind: card.kind().unwrap_or("main").to_string(),
            id: card.id().map(str::to_string),
            fields: schema.map(|schema| fields(schema, card, &path, &errors)).unwrap_or_default(),
            headings: headings(card.body()),
            anchors: anchors
                .iter()
                .filter(|a| a.path.segs().first() == path.segs().first())
                .map(|a| OutlineAnchor { id: a.id.clone(), path: a.path.clone() })
                .collect(),
            path,
        })
        .collect();
    Outline { cards }
}

fn fields(schema: &CardSchema, card: &Card, at: &DocPath, errors: &[String]) -> Vec<OutlineField> {
    schema
        .fields
        .values()
        .map(|field| {
            let path = at.field(&field.name);
            let value = card.payload().get(&field.name).map(|v| v.as_json());
            let richtext = matches!(field.r#type, FieldType::RichText { .. });
            let state = match value {
                None => FieldState::Empty,
                Some(value) if is_empty(value, richtext) => FieldState::Empty,
                Some(_) if errors.iter().any(|e| is_at_or_under(e, &path.to_string())) => FieldState::Invalid,
                Some(_) => FieldState::Ok,
            };
            OutlineField { name: field.name.clone(), path, state }
        })
        .collect()
}

fn is_empty(value: &JsonValue, richtext: bool) -> bool {
    match value {
        JsonValue::Null => true,
        JsonValue::String(s) => s.trim().is_empty(),
        JsonValue::Array(items) => items.is_empty(),
        JsonValue::Object(map) if richtext => from_canonical_value(value).map_or(map.is_empty(), |c| c.is_blank()),
        JsonValue::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn is_at_or_under(error: &str, path: &str) -> bool {
    error
        .strip_prefix(path)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.') || rest.starts_with('['))
}

fn headings(body: &Normalized) -> Vec<OutlineHeading> {
    body.lines
        .iter()
        .zip(body.text.split('\n'))
        .enumerate()
        .filter_map(|(line, (meta, text))| match meta.kind {
            LineKind::Heading { level } => Some(OutlineHeading {
                line,
                level,
                text: text.chars().filter(|&c| c != ISLAND_SLOT).collect(),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Base;
    use crate::quill::QuillConfig;

    const QUILL_YAML: &str = "\
quill:
  name: memo
  backend: typst
  version: 1.0.0
  description: Outline test quill
main:
  fields:
    subject:
      type: richtext
      multiline: false
    count:
      type: integer
    note:
      type: text
      multiline: true
card_kinds:
  item:
    fields:
      summary:
        type: richtext
        multiline: true
";

    const DOC: &str = "~~~\n$quill: memo\nsubject: Quarterly plan\ncount: lots\n~~~\n\n# Goals\n\nShip the roads budget.\n\n## Risks\n\nWeather.\n\n~~~\n$kind: item\n$id: it1\nsummary: \"  \"\n~~~\n\nItem body.\n";

    fn anchored(content: &Normalized, word: &str, id: &str) -> Normalized {
        let start = content.text[..content.text.find(word).unwrap()].chars().count();
        let mut content = content.clone().into_content();
        let kind = quillmark_content::model::MarkKind::Anchor { id: id.into() };
        content.marks.push(quillmark_content::model::Mark::new(start, start + word.chars().count(), kind));
        content.into_normalized()
    }

    fn read() -> (QuillConfig, Document) {
        let config = QuillConfig::from_yaml(QUILL_YAML).expect("valid quill");
        let doc = Document::parse(DOC).expect("parses").document;
        (config, doc)
    }

    #[test]
    fn the_outline_names_each_card_field_state_heading_and_anchor() {
        let (config, mut doc) = read();
        let body = anchored(doc.main().body(), "roads budget", "c1");
        doc.main_mut().overwrite_body(body);
        let outline = outline(&config, &doc);
        let [main, item] = &outline.cards[..] else { panic!("{outline:?}") };

        assert_eq!((main.path.to_string(), main.kind.as_str(), main.id.as_deref()), ("main".into(), "main", None));
        let states: Vec<(&str, &str)> = main.fields.iter().map(|f| (f.name.as_str(), f.state.as_str())).collect();
        assert_eq!(states, [("subject", "ok"), ("count", "invalid"), ("note", "empty")]);
        let headings: Vec<(usize, u8, &str)> = main.headings.iter().map(|h| (h.line, h.level, h.text.as_str())).collect();
        assert_eq!(headings, [(0, 1, "Goals"), (2, 2, "Risks")]);
        assert_eq!(main.anchors, [OutlineAnchor { id: "c1".into(), path: DocPath::main_body() }]);

        assert_eq!((item.path.to_string(), item.kind.as_str(), item.id.as_deref()), ("cards.item[0]".into(), "item", Some("it1")));
        assert_eq!(item.fields.iter().map(|f| f.state).collect::<Vec<_>>(), [FieldState::Empty]);
        assert!(item.headings.is_empty() && item.anchors.is_empty());
    }

    /// The scoped read is what a revise at its address reads back: written back
    /// unchanged it moves nothing, and as a base it merges a writer's edit.
    #[test]
    fn a_scoped_read_is_the_revise_lanes_own_text() {
        let (_, mut doc) = read();
        let body = anchored(doc.main().body(), "roads budget", "c1");
        doc.main_mut().overwrite_body(body);
        let read = doc.main().body_annotated();
        assert!(read.markdown.contains("<qm-anchor ref=\"c1\"></qm-anchor>roads budget"), "{}", read.markdown);
        assert_eq!(read.anchors.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["c1"]);

        let stored = doc.main().body().clone();
        let revised = doc.main_mut().revise_body(&read.markdown).unwrap();
        assert!(revised.dropped_anchors.is_empty());
        assert_eq!(doc.main().body(), &stored);

        let edited = read.markdown.replace("Weather.", "Weather and cost.");
        let revised = doc.main_mut().revise_body_against(Base::Read(&read.markdown), &edited).unwrap();
        assert!(revised.dropped_anchors.is_empty());
        assert!(doc.main().body_markdown().contains("Weather and cost."));

        assert_eq!(doc.main().field_annotated("subject").unwrap().markdown, "Quarterly plan");
        assert_eq!(doc.main().field_annotated("absent").unwrap().markdown, "");
    }
}
