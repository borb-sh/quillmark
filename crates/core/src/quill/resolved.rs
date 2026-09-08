//! The resolved-value view: [`Quill::resolve`].
//!
//! For every declared field, the value the render projection would use and
//! the [`FieldSource`] rung it came from, cut through the same producer
//! (`super::compose::resolve_card_sourced`) the render plate cuts: never a
//! parallel precedence policy.

use indexmap::IndexMap;
use serde::Serialize;

use super::compose::resolve_card_sourced;
use super::{CardSchema, Quill, QuillConfig};
use crate::{Card, Document, QuillValue};

/// The rung of the commitment ladder that produced a [`ResolvedField::value`].
/// Serializes lowercase (`"authored" | "default" | "blank"`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldSource {
    /// The authored value: the document's own content.
    Authored,
    /// The schema `default:` (or its content form for a content field).
    Default,
    /// The field's [`blank`](crate::quill::blank): its spelling of
    /// "explicitly nothing", and the render floor.
    Blank,
}

impl FieldSource {
    /// Height on the ladder: how strongly the value claims to be the real
    /// answer. Private, so the two operations below are the ladder's only
    /// order — a consumer reads a rung, it never compares two.
    fn commitment(self) -> u8 {
        match self {
            Self::Blank => 0,
            Self::Default => 1,
            Self::Authored => 2,
        }
    }

    /// The more committed of two rungs.
    ///
    /// A container has no rung of its own — it is a namespace, and its value is
    /// the composition of its cells' — so it reports the strongest rung that
    /// contributed to it: authored if the document wrote any of it, else
    /// `default` if any cell below resolved to one, else the floor.
    pub(crate) fn join(self, other: Self) -> Self {
        if other.commitment() > self.commitment() {
            other
        } else {
            self
        }
    }

    /// This rung, held down to `ceiling`.
    pub(crate) fn capped_at(self, ceiling: Self) -> Self {
        if self.commitment() > ceiling.commitment() {
            ceiling
        } else {
            self
        }
    }
}

/// One resolved row: its `name`, the value the render projection would use, and
/// the [`FieldSource`] rung that value came from. A row carries its own name so
/// declaration order is structural: an ordered array, not JSON object key
/// order. The card body is a [`ResolvedMain::body`] / [`ResolvedCard::body`]
/// sibling, never a row in `fields`, so a consumer iterating declared fields
/// never trips over it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedField {
    pub name: String,
    pub value: QuillValue,
    pub source: FieldSource,
}

/// The main card's resolved rows in declaration order, plus its body row when
/// the main enables a body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedMain {
    pub fields: Vec<ResolvedField>,
    pub body: Option<ResolvedField>,
}

/// One composable card's resolved rows, with its authored `kind` (present even
/// for an unknown kind, which carries its fields verbatim), its document-array
/// `index`, and its body row when the kind enables a body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedCard {
    pub kind: Option<String>,
    pub index: usize,
    pub fields: Vec<ResolvedField>,
    pub body: Option<ResolvedField>,
}

/// The whole resolved-value view: the main card and every composable card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub main: ResolvedMain,
    pub cards: Vec<ResolvedCard>,
}

impl Quill {
    /// The resolved-value view of `doc` against this quill's schema.
    ///
    /// For every declared field, the value [`compile_data`] emits into the plate:
    /// the two cut the *same* sourced ladder (`ladder_sourced`) over equal
    /// coerced input, so the value is the plate's by construction: tagged with
    /// the [`FieldSource`] rung it came from. Completeness and errors stay
    /// [`Quill::validate`]'s; this view carries no diagnostics.
    ///
    /// [`compile_data`]: Quill::compile_data
    pub fn resolve(&self, doc: &Document) -> Resolved {
        resolve_document(self.config(), doc)
    }
}

/// The producer behind [`Quill::resolve`] and
/// [`TypedReader::resolve`](crate::TypedReader::resolve).
pub(crate) fn resolve_document(config: &QuillConfig, doc: &Document) -> Resolved {
    let (fields, body) = resolve_card_fields(&config.main, doc.main());
    let main = ResolvedMain { fields, body };
    let cards = doc
        .cards()
        .iter()
        .enumerate()
        .map(|(index, card)| card_states(config, card, index))
        .collect();
    Resolved { main, cards }
}

/// Resolve one card (main or a schema-declared kind) into its ordered
/// [`ResolvedField`] rows and its body row (present iff the kind enables a body).
///
/// The value and source of every field come from the one shared resolver
/// [`resolve_card_sourced`] (the same producer [`compile_data`] cuts for the
/// plate) so the two projections cannot drift. This layer only re-cuts the
/// **presentation order**: declared fields first in declaration order (the canon
/// ordering contract, carried structurally by the row array, not the validation
/// walker's alphabetical sort), then undeclared authored fields in authored
/// order. The body is a sibling row, never an entry in `fields`.
///
/// [`compile_data`]: crate::Quill::compile_data
fn resolve_card_fields(schema: &CardSchema, card: &Card) -> (Vec<ResolvedField>, Option<ResolvedField>) {
    let sourced: IndexMap<String, (QuillValue, FieldSource)> = resolve_card_sourced(schema, card);
    let mut fields = Vec::new();

    // Declared rows in schema declaration order. Every declared field is present
    // in the map (the resolver's ladder inserts each one) so the lookup holds.
    for (name, _field_schema) in &schema.fields {
        let (value, source) = sourced
            .get(name)
            .cloned()
            .expect("resolve_card_sourced emits every declared field");
        fields.push(ResolvedField {
            name: name.clone(),
            value,
            source,
        });
    }

    // Undeclared authored fields, appended in authored order under their NFC
    // keys: the schema is a floor, not an allowlist, so these reach both
    // projections too, value verbatim, source Authored.
    for (name, (value, source)) in &sourced {
        if !schema.fields.contains_key(name) {
            fields.push(ResolvedField {
                name: name.clone(),
                value: value.clone(),
                source: *source,
            });
        }
    }

    let body = schema.body_enabled().then(|| body_state(card));
    (fields, body)
}

/// Resolve one composable card. A card whose `$kind` names a schema resolves
/// through the ladder; an unknown-kind card (declared `$kind` with no schema, or
/// a kindless card) carries its authored fields verbatim: no coercion, no
/// ladder, no `$body` row.
fn card_states(config: &QuillConfig, card: &Card, index: usize) -> ResolvedCard {
    // The raw authored kind rides the entry even when it names no schema: the
    // card reports what it *claimed* to be.
    let kind = card.kind().map(String::from);
    match card.kind().and_then(|k| config.card_kind(k)) {
        Some(schema) => {
            let (fields, body) = resolve_card_fields(schema, card);
            ResolvedCard {
                kind,
                index,
                fields,
                body,
            }
        }
        None => {
            // An unknown-kind card carries its authored fields verbatim: no
            // schema, no ladder, and `to_index_map` drops `$` keys, so no body.
            let fields = card
                .payload()
                .to_index_map()
                .into_iter()
                .map(|(name, value)| ResolvedField {
                    name,
                    value,
                    source: FieldSource::Authored,
                })
                .collect();
            ResolvedCard {
                kind,
                index,
                fields,
                body: None,
            }
        }
    }
}

/// The body row (`name: "body"`). The value is byte-identical to the plate's
/// `$body` (canonical Content-JSON of the card body). A body has no `default:`
/// rung, so its source is only ever [`Authored`](FieldSource::Authored)
/// (non-blank) or [`Blank`](FieldSource::Blank) (blank).
fn body_state(card: &Card) -> ResolvedField {
    let value = QuillValue::from_json(quillmark_content::serial::to_canonical_value(card.body()));
    let source = if card.body().is_blank() {
        FieldSource::Blank
    } else {
        FieldSource::Authored
    };
    ResolvedField {
        name: "body".to_string(),
        value,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quill::quill_from_yaml;
    use crate::{Card, Document, Payload};

    fn parse(md: &str) -> Document {
        Document::parse(md).expect("document should parse").document
    }

    /// Look up a resolved row by name: rows are an ordered array.
    fn row<'a>(fields: &'a [ResolvedField], name: &str) -> &'a ResolvedField {
        fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no row `{name}`"))
    }

    fn has_row(fields: &[ResolvedField], name: &str) -> bool {
        fields.iter().any(|f| f.name == name)
    }

    const QUILL: &str = r#"
quill:
  name: fs_test
  version: "1.0"
  backend: typst
  description: Field-state tests
main:
  body:
    example: "Example body prose."
  fields:
    title:
      type: string
    status:
      type: string
      default: draft
    notes:
      type: string
    intro:
      type: richtext
      default: "**hi**"
    recipients:
      type: array
      items:
        type: object
        properties:
          name: { type: string }
card_kinds:
  note:
    fields:
      author:
        type: string
        example: A. Author
      tag:
        type: string
"#;

    // ── Sources ──────────────────────────────────────────────────────────────

    #[test]
    fn scalar_sources_authored_default_blank() {
        let quill = quill_from_yaml(QUILL);
        // title authored; status absent (has a default); notes absent (no default).
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: Hello\n~~~\n");
        let states = quill.resolve(&doc);
        let f = &states.main.fields;

        assert_eq!(row(f, "title").source, FieldSource::Authored);
        assert_eq!(row(f, "title").value.as_json(), &serde_json::json!("Hello"));

        assert_eq!(row(f, "status").source, FieldSource::Default);
        assert_eq!(row(f, "status").value.as_json(), &serde_json::json!("draft"));

        assert_eq!(row(f, "notes").source, FieldSource::Blank);
        assert_eq!(row(f, "notes").value.as_json(), &serde_json::json!(""));
    }

    #[test]
    fn richtext_default_reports_default_and_matches_plate() {
        let quill = quill_from_yaml(QUILL);
        // intro absent → its richtext `default:` (committed as content).
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n");
        let states = quill.resolve(&doc);
        let intro = row(&states.main.fields, "intro");

        assert_eq!(intro.source, FieldSource::Default);
        // The value is the content form of the default, byte-equal to the plate.
        let plate = quill.compile_data(&doc).expect("compile");
        assert_eq!(intro.value.as_json(), &plate["intro"]);
        // And it is content, not the raw markdown string.
        assert!(intro.value.as_json().is_object());
    }

    #[test]
    fn present_null_is_absent_takes_default_rung() {
        let quill = quill_from_yaml(QUILL);
        // `status:` is a present-null → treated as absent → default rung.
        let doc = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\nstatus:\n~~~\n");
        let states = quill.resolve(&doc);
        let status = row(&states.main.fields, "status");
        assert_eq!(status.source, FieldSource::Default);
        assert_eq!(status.value.as_json(), &serde_json::json!("draft"));
    }

    const CONTAINERS: &str = r#"
quill:
  name: shape_test
  version: "1.0"
  backend: typst
  description: Container-shape tests
main:
  fields:
    counts:
      type: array
      items:
        type: integer
    address:
      type: object
      properties:
        street: { type: string }
"#;

    fn shape_doc(fields: &str) -> Document {
        parse(&format!(
            "~~~card-yaml\n$quill: shape_test@1.0\n$kind: main\n{fields}~~~\n"
        ))
    }

    #[test]
    fn an_array_field_blanks_only_where_the_document_left_it_out() {
        let quill = quill_from_yaml(CONTAINERS);

        // `abc` conforms to no `integer[]`, so it reaches the ladder raw. The row
        // is the document's own text under the document's own label, not `[]`.
        let raw = quill.resolve(&shape_doc("counts: abc\n"));
        let r = row(&raw.main.fields, "counts");
        assert_eq!(r.source, FieldSource::Authored);
        assert_eq!(r.value.as_json(), &serde_json::json!("abc"));

        let absent = quill.resolve(&shape_doc(""));
        let r = row(&absent.main.fields, "counts");
        assert_eq!(r.source, FieldSource::Blank);
        assert_eq!(r.value.as_json(), &serde_json::json!([]));
    }

    #[test]
    fn a_typed_dict_blanks_only_where_the_document_left_it_out() {
        let quill = quill_from_yaml(CONTAINERS);

        let raw = quill.resolve(&shape_doc("address: 5\n"));
        let r = row(&raw.main.fields, "address");
        assert_eq!(r.source, FieldSource::Authored);
        assert_eq!(r.value.as_json(), &serde_json::json!(5));

        let absent = quill.resolve(&shape_doc(""));
        let r = row(&absent.main.fields, "address");
        assert_eq!(r.source, FieldSource::Blank);
        assert_eq!(r.value.as_json(), &serde_json::json!({ "street": "" }));
    }

    // ── Byte-for-byte with the render projection ─────────────────────────────
    // Both projections cut the one shared ladder (`ladder_sourced`), but over
    // separately-conformed input: the render gate's fallible conform vs the
    // view's keep-raw `conform_card_render`. This pins that those two conform
    // paths agree on a gated document, plus the plate-build wiring (order, meta,
    // body) against the row view.

    #[test]
    fn every_row_is_byte_for_byte_with_compile_data() {
        let quill = quill_from_yaml(QUILL);
        let md = "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\n\
                  title: Hello\nintro: \"**bold**\"\nrecipients:\n  - name: Alice\n~~~\n\n\
                  Body prose here.\n\n\
                  ~~~card-yaml\n$kind: note\nauthor: Zed\n~~~\nNote body.\n";
        let doc = parse(md);
        let states = quill.resolve(&doc);
        let plate = quill.compile_data(&doc).expect("compile");

        // Every declared main row equals its plate field; the body equals plate `$body`.
        for name in ["title", "status", "notes", "intro", "recipients"] {
            assert_eq!(
                row(&states.main.fields, name).value.as_json(),
                &plate[name],
                "main row `{name}` must be byte-for-byte with the plate"
            );
        }
        assert_eq!(
            states.main.body.as_ref().unwrap().value.as_json(),
            &plate["$body"]
        );

        // The card's declared rows equal its plate card; the body equals plate `$body`.
        let plate_card = &plate["$cards"][0];
        let card = &states.cards[0];
        for name in ["author", "tag"] {
            assert_eq!(
                row(&card.fields, name).value.as_json(),
                &plate_card[name],
                "card row `{name}` must be byte-for-byte with the plate"
            );
        }
        assert_eq!(
            card.body.as_ref().unwrap().value.as_json(),
            &plate_card["$body"]
        );
    }

    #[test]
    fn non_nfc_key_on_a_constructed_payload_rows_under_its_nfc_spelling() {
        // Every validated ingress (parse, the mutators) restricts field names
        // to ASCII, so a non-NFC key only enters through direct construction
        // (`Payload::from_index_map`). Render NFC-normalizes it between
        // coercion and the ladder; the view mirrors that, rowing it under the
        // NFC key the plate carries: not the raw decomposed one.
        let quill = quill_from_yaml(QUILL);
        let mut map = IndexMap::new();
        // `e` + U+0301 combining acute: NFC-composes to U+00E9.
        map.insert(
            "cafe\u{301}".to_string(),
            QuillValue::from_json(serde_json::json!("hot")),
        );
        let mut payload = Payload::from_index_map(map);
        payload.set_quill("fs_test@1.0".parse().unwrap());
        payload.set_kind("main");
        let main = Card::from_parts(payload, quillmark_content::Normalized::empty());
        let doc = Document::from_main_and_cards(main, Vec::new());
        let states = quill.resolve(&doc);

        assert!(!has_row(&states.main.fields, "cafe\u{301}"));
        let r = row(&states.main.fields, "caf\u{e9}");
        assert_eq!(r.source, FieldSource::Authored);
        assert_eq!(r.value.as_json(), &serde_json::json!("hot"));
    }

    // ── The body row ─────────────────────────────────────────────────────────

    #[test]
    fn body_row_authored_vs_blank_source() {
        let quill = quill_from_yaml(QUILL);

        let authored =
            parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n\nHello body.\n");
        assert_eq!(
            quill.resolve(&authored).main.body.unwrap().source,
            FieldSource::Authored
        );

        let blank = parse("~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n");
        let states = quill.resolve(&blank);
        let body = states.main.body.as_ref().unwrap();
        assert_eq!(body.source, FieldSource::Blank);
        assert!(body.value.as_json().is_object(), "blank body is empty content");
    }

    const BODY_DISABLED_QUILL: &str = r#"
quill:
  name: bd_test
  version: "1.0"
  backend: typst
  description: Body-disabled test
main:
  fields:
    title:
      type: string
card_kinds:
  stamp:
    body:
      enabled: false
    fields:
      label:
        type: string
"#;

    #[test]
    fn body_disabled_kind_omits_body_row() {
        let quill = quill_from_yaml(BODY_DISABLED_QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: bd_test@1.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: stamp\nlabel: L\n~~~\nStray prose.\n",
        );
        let states = quill.resolve(&doc);
        let card = &states.cards[0];
        assert!(card.body.is_none(), "a body-disabled kind has no body row");
        assert!(has_row(&card.fields, "label"), "declared rows still present");
    }

    // ── Unknown-kind card ────────────────────────────────────────────────────

    #[test]
    fn unknown_kind_card_shape() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: mystery\nfoo: bar\n~~~\nUnread body.\n",
        );
        let states = quill.resolve(&doc);
        let card = &states.cards[0];

        assert_eq!(card.kind.as_deref(), Some("mystery"));
        assert_eq!(card.index, 0);
        // Authored fields only: no body row, no ladder.
        assert_eq!(row(&card.fields, "foo").source, FieldSource::Authored);
        assert_eq!(row(&card.fields, "foo").value.as_json(), &serde_json::json!("bar"));
        assert!(card.body.is_none());
    }

    // ── Undeclared authored field ────────────────────────────────────────────

    #[test]
    fn undeclared_authored_field_row_is_authored() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            "~~~card-yaml\n$quill: fs_test@1.0\n$kind: main\ntitle: T\nextra: whatever\n~~~\n",
        );
        let states = quill.resolve(&doc);
        let r = row(&states.main.fields, "extra");
        assert_eq!(r.source, FieldSource::Authored);
        assert_eq!(r.value.as_json(), &serde_json::json!("whatever"));
    }

    // ── Wire shape ───────────────────────────────────────────────────────────

    #[test]
    fn field_source_serializes_the_blank_token() {
        // The wire token consumers route on; `Quill.resolve` exposes it through
        // the Wasm surface, whose TS union must carry the same three spellings.
        let token = |s: FieldSource| serde_json::to_value(s).unwrap();
        assert_eq!(token(FieldSource::Authored), serde_json::json!("authored"));
        assert_eq!(token(FieldSource::Default), serde_json::json!("default"));
        assert_eq!(token(FieldSource::Blank), serde_json::json!("blank"));
    }

    #[test]
    fn field_state_is_name_value_and_source_only() {
        let state = ResolvedField {
            name: "x".to_string(),
            value: QuillValue::from_json(serde_json::json!("v")),
            source: FieldSource::Authored,
        };
        let json = serde_json::to_value(&state).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 3, "only name + value + source on the wire: {json}");
        assert!(
            obj.contains_key("name") && obj.contains_key("value") && obj.contains_key("source")
        );
    }
}
