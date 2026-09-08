//! The values shape: [`DocumentValues`] / [`CardValues`], read by
//! [`TypedReader::values`](crate::TypedReader::values) and written by
//! [`TypedWriter::set_values`](crate::TypedWriter::set_values). It is one of a
//! document's three forms; `SCHEMAS.md` § "The values form" separates them.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;

use super::{CardSchema, QuillConfig};
use crate::normalize::normalize_field_name;
use crate::reader::{project_value, ProjectMode};
use crate::Card;

/// A document in the values form: the fields the main card carries, its body as
/// markdown, its `$ext`, and every composable card. Every content leaf is its
/// codec's text at every depth the field's type tree reaches; every other value
/// is as stored, a present-null as `null`. Declared fields come first in
/// declaration order, then undeclared fields verbatim in authored order.
///
/// **A projection, never a storage format**, and a sparse one; persist a
/// document through [`StoredDocument`](crate::document). What a cycle does and
/// does not keep is `SCHEMAS.md` § "The values form".
///
/// The read fills every axis. On the way into
/// [`set_values`](crate::TypedWriter::set_values) each axis is optional and an
/// absent one is untouched, so `DocumentValues::default()` is the empty patch
/// and a full read written back is a byte no-op.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentValues {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<IndexMap<String, JsonValue>>,
    /// The main body's markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// In document order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<CardValues>>,
    /// The main card's `$ext`: `Some(None)` is `null`, a card carrying none;
    /// `Some(Some(map))` the map, `{}` included, which is an explicit
    /// `$ext: {}`.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ext: Option<Option<serde_json::Map<String, JsonValue>>>,
}

/// One composable card in the values form. `kind` is `Some(None)` (`null`) for
/// a kindless card. A kind the schema does not declare carries its fields
/// verbatim, there being no declared type to project them through.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardValues {
    /// Absent on input keeps the kind of the card at that position.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<IndexMap<String, JsonValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ext: Option<Option<serde_json::Map<String, JsonValue>>>,
}

/// Absent → `None`, `null` → `Some(None)`, a value → `Some(Some(v))`. Serde's
/// own `Option<Option<T>>` reads `null` as the outer `None`, which would make a
/// removal indistinguishable from an untouched axis.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

impl DocumentValues {
    /// A patch of the main fields alone: body, cards and `$ext` untouched.
    pub fn new(fields: IndexMap<String, JsonValue>) -> Self {
        Self {
            fields: Some(fields),
            ..Self::default()
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_cards(mut self, cards: Vec<CardValues>) -> Self {
        self.cards = Some(cards);
        self
    }

    /// `None` removes `$ext`; an empty map records an explicit `$ext: {}`.
    pub fn with_ext(mut self, ext: Option<serde_json::Map<String, JsonValue>>) -> Self {
        self.ext = Some(ext);
        self
    }
}

impl CardValues {
    /// A card of `kind` with these fields: body and `$ext` absent, which on a
    /// card being built is empty and on one being patched is untouched.
    pub fn new(kind: impl Into<String>, fields: IndexMap<String, JsonValue>) -> Self {
        Self {
            kind: Some(Some(kind.into())),
            fields: Some(fields),
            ..Self::default()
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// `None` removes `$ext`; an empty map records an explicit `$ext: {}`.
    pub fn with_ext(mut self, ext: Option<serde_json::Map<String, JsonValue>>) -> Self {
        self.ext = Some(ext);
        self
    }
}

/// One composable card read whole, every axis filled.
pub(crate) fn card_values(config: &QuillConfig, card: &Card) -> CardValues {
    let schema = card.kind().and_then(|k| config.card_kind(k));
    CardValues {
        kind: Some(card.kind().map(String::from)),
        fields: Some(card_fields(schema, card)),
        body: Some(card.body_markdown()),
        ext: Some(card.ext().cloned()),
    }
}

/// One card's authored fields: declared ones first in declaration order, each
/// projected at every content leaf below it, then undeclared ones verbatim in
/// authored order. The schema is a floor, not an allowlist, as it is for the
/// resolved view. A `None` schema is a card kind carrying no declaration, so
/// every field rides verbatim.
pub(crate) fn card_fields(schema: Option<&CardSchema>, card: &Card) -> IndexMap<String, JsonValue> {
    let payload = card.payload().to_index_map();
    let Some(schema) = schema else {
        return payload
            .into_iter()
            .map(|(name, value)| (name, value.as_json().clone()))
            .collect();
    };
    let mut out = IndexMap::with_capacity(payload.len());
    let mut undeclared = IndexMap::new();
    for (raw_name, value) in payload.iter() {
        match schema.fields.get_full(normalize_field_name(raw_name).as_str()) {
            Some((_, name, field_schema)) => {
                out.insert(name.clone(), project_field(name, value, field_schema));
            }
            None => {
                undeclared.insert(raw_name.clone(), value.as_json().clone());
            }
        }
    }
    let mut ordered: IndexMap<String, JsonValue> = schema
        .fields
        .keys()
        .filter_map(|name| out.shift_remove_entry(name))
        .collect();
    ordered.extend(undeclared);
    ordered
}

/// One authored field in the values form: every content leaf below it as its
/// codec's text, everything else verbatim, a leaf that decodes under neither
/// encoding riding out as stored.
///
/// [`TypedWriter::set_values`](crate::TypedWriter::set_values) guards each cell
/// against this rather than against the stored bytes, so a value that projects
/// equal is not rewritten: the only comparison under which an anchor-bearing
/// content, a `!must_fill` marker, or a scalar shorthand all count as
/// unchanged.
pub(crate) fn project_field(
    name: &str,
    value: &crate::QuillValue,
    schema: &crate::FieldSchema,
) -> JsonValue {
    project_value(
        name,
        value.as_json(),
        schema,
        &mut Vec::new(),
        ProjectMode::Total,
    )
    .expect("ProjectMode::Total never raises")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::quill::quill_from_yaml;
    use crate::{Document, Quill};

    pub(crate) const QUILL: &str = r#"
quill:
  name: memo
  version: "1.0"
  backend: typst
  description: Values example
main:
  fields:
    subject: { type: richtext, inline: true }
    note: { type: plaintext }
    qty: { type: integer, default: 1 }
    paragraphs:
      type: array
      items: { type: richtext }
    letterhead:
      type: object
      properties:
        motto: { type: richtext }
        code: { type: string }
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      variants:
        CUI:
          controlled_by: { type: plaintext }
          banner: { type: richtext }
card_kinds:
  line_item:
    fields:
      desc: { type: richtext, inline: true }
      qty: { type: integer }
"#;

    pub(crate) const DOC: &str = "\
~~~card-yaml
$quill: memo@1.0
$kind: main
$ext:
  editor:
    title: Q3 memo
subject: Hello **world**
note: a *literal* line
paragraphs:
  - Para **one**
  - Para two
letterhead:
  motto: Fly *fight*
  code: \"9\"
classification:
  value: CUI
  controlled_by: \"OPR: 49 FW\"
  banner: \"**CUI** banner\"
~~~

Body prose.

~~~card-yaml
$kind: line_item
$ext:
  myapp:
    row_id: 17
desc: Widget __A__
qty: \"3\"
~~~
Item note.
";

    pub(crate) fn parse(quill: &Quill, md: &str) -> Document {
        quill.parse(md).expect("document should parse").document
    }

    pub(crate) fn values_of(quill: &Quill, doc: &Document) -> DocumentValues {
        quill.reader(doc).values()
    }

    /// The read fills every axis, so a test may unwrap them.
    pub(crate) fn fields(v: &DocumentValues) -> &IndexMap<String, JsonValue> {
        v.fields.as_ref().expect("a read fills `fields`")
    }

    pub(crate) fn cards(v: &DocumentValues) -> &[CardValues] {
        v.cards.as_deref().expect("a read fills `cards`")
    }

    fn projected() -> DocumentValues {
        let quill = quill_from_yaml(QUILL);
        values_of(&quill, &parse(&quill, DOC))
    }

    #[test]
    fn content_leaves_project_at_every_depth() {
        let v = projected();
        let f = fields(&v);
        assert_eq!(f["subject"], serde_json::json!("Hello **world**"));
        assert_eq!(f["note"], serde_json::json!("a *literal* line"));
        assert_eq!(
            f["paragraphs"],
            serde_json::json!(["Para **one**", "Para two"]),
            "an array<richtext> exports at each element, not as stored content"
        );
        assert_eq!(
            f["letterhead"],
            serde_json::json!({"motto": "Fly *fight*", "code": "9"}),
            "a content property projects; a scalar sibling rides verbatim"
        );
        assert_eq!(
            f["classification"],
            serde_json::json!({
                "value": "CUI",
                "controlled_by": "OPR: 49 FW",
                "banner": "**CUI** banner",
            }),
            "each variant cell projects at its own codec, the discriminant verbatim"
        );
    }

    #[test]
    fn the_shape_is_sparse_and_never_coerces() {
        let v = projected();
        assert!(
            !fields(&v).contains_key("qty"),
            "an absent field is absent here: its `default: 1` is never materialized"
        );
        assert_eq!(
            cards(&v)[0].fields.as_ref().unwrap()["qty"],
            serde_json::json!("3"),
            "a scalar shorthand reads as stored; the resolved view is the coerced one"
        );
    }

    #[test]
    fn every_axis_is_filled_on_read() {
        let v = projected();
        assert_eq!(v.body.as_deref(), Some("Body prose."));
        assert_eq!(
            v.ext,
            Some(serde_json::json!({"editor": {"title": "Q3 memo"}}).as_object().cloned())
        );
        let card = &cards(&v)[0];
        assert_eq!(card.kind, Some(Some("line_item".to_string())));
        assert_eq!(card.body.as_deref(), Some("Item note."));
        assert_eq!(card.fields.as_ref().unwrap()["desc"], serde_json::json!("Widget **A**"));
        assert_eq!(
            card.ext,
            Some(serde_json::json!({"myapp": {"row_id": 17}}).as_object().cloned()),
            "$ext is the consumer's own card key and rides the shape"
        );
    }

    #[test]
    fn a_card_without_ext_reads_null_and_a_kindless_one_null_kind() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            &quill,
            "~~~card-yaml\n$quill: memo@1.0\n$kind: main\n~~~\n\n~~~card-yaml\nfoo: bar\n~~~\n",
        );
        let v = values_of(&quill, &doc);
        assert_eq!(v.ext, Some(None));
        let card = &cards(&v)[0];
        assert_eq!(card.kind, Some(None));
        assert_eq!(card.ext, Some(None));
        assert_eq!(
            serde_json::to_value(card).unwrap(),
            serde_json::json!({"kind": null, "fields": {"foo": "bar"}, "body": "", "ext": null})
        );
    }

    #[test]
    fn present_null_reads_as_null() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(&quill, "~~~card-yaml\n$quill: memo@1.0\n$kind: main\nsubject:\nqty:\n~~~\n");
        let v = values_of(&quill, &doc);
        assert_eq!(fields(&v)["subject"], JsonValue::Null);
        assert_eq!(fields(&v)["qty"], JsonValue::Null);
    }

    #[test]
    fn undeclared_fields_ride_verbatim_after_the_declared_ones() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(
            &quill,
            "~~~card-yaml\n$quill: memo@1.0\n$kind: main\nstray: whatever\nsubject: Hi\n~~~\n",
        );
        let v = values_of(&quill, &doc);
        assert_eq!(
            fields(&v).keys().collect::<Vec<_>>(),
            ["subject", "stray"],
            "declared first in declaration order, then undeclared in authored order"
        );
        assert_eq!(fields(&v)["stray"], serde_json::json!("whatever"));
    }

    #[test]
    fn an_unknown_kind_card_carries_its_fields_verbatim() {
        let quill = quill_from_yaml(QUILL);
        let md = "~~~card-yaml\n$quill: memo@1.0\n$kind: main\n~~~\n\n\
                  ~~~card-yaml\n$kind: mystery\nfoo: bar\n~~~\nStray.\n";
        let v = values_of(&quill, &parse(&quill, md));
        let card = &cards(&v)[0];
        assert_eq!(card.kind, Some(Some("mystery".to_string())));
        assert_eq!(card.fields.as_ref().unwrap()["foo"], serde_json::json!("bar"));
        assert_eq!(card.body.as_deref(), Some("Stray."));
    }

    #[test]
    fn a_leaf_that_decodes_under_neither_encoding_passes_verbatim() {
        let quill = quill_from_yaml(QUILL);
        let mut doc = parse(&quill, "~~~card-yaml\n$quill: memo@1.0\n$kind: main\n~~~\n");
        doc.main_mut()
            .store_field(
                "paragraphs",
                crate::QuillValue::from_json(serde_json::json!([{"not": "content"}])),
            )
            .unwrap();
        let v = values_of(&quill, &doc);
        assert_eq!(
            fields(&v)["paragraphs"],
            serde_json::json!([{"not": "content"}]),
            "the whole-document read is total: a dirty leaf rides out rather than failing it"
        );
        assert!(
            quill.reader(&doc).get("paragraphs").is_err(),
            "the single-cell read raises on the same leaf"
        );
    }

    #[test]
    fn get_is_values_restricted_to_one_field() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(&quill, DOC);
        let reader = quill.reader(&doc);
        let v = reader.values();
        for (name, value) in fields(&v) {
            assert_eq!(
                reader.get(name).unwrap().map(|q| q.as_json().clone()).as_ref(),
                Some(value),
                "`{name}`"
            );
        }
        let card = reader.card(0).unwrap();
        for (name, value) in card.values().fields.as_ref().unwrap() {
            assert_eq!(
                card.get(name).unwrap().map(|q| q.as_json().clone()).as_ref(),
                Some(value),
                "card `{name}`"
            );
        }
    }

    #[test]
    fn the_card_read_is_the_document_read_restricted_to_one_slot() {
        let quill = quill_from_yaml(QUILL);
        let doc = parse(&quill, DOC);
        let reader = quill.reader(&doc);
        assert_eq!(reader.card(0).unwrap().values(), cards(&reader.values())[0]);
    }

    #[test]
    fn the_shape_round_trips_through_serde() {
        let v = projected();
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(serde_json::from_value::<DocumentValues>(json).unwrap(), v);
    }

    #[test]
    fn serde_tells_absent_from_null() {
        let absent: DocumentValues = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(absent, DocumentValues::default());
        assert_eq!(absent.ext, None);

        let null: DocumentValues =
            serde_json::from_value(serde_json::json!({"ext": null, "cards": [{"kind": null}]}))
                .unwrap();
        assert_eq!(null.ext, Some(None));
        assert_eq!(null.cards.unwrap()[0].kind, Some(None));

        let out = serde_json::to_value(DocumentValues::default().with_ext(None)).unwrap();
        assert_eq!(out, serde_json::json!({"ext": null}), "a removal serializes as null");
        assert_eq!(
            serde_json::to_value(DocumentValues::default()).unwrap(),
            serde_json::json!({}),
            "the empty patch serializes to nothing"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_on_the_way_in() {
        assert!(
            serde_json::from_value::<DocumentValues>(serde_json::json!({"feilds": {}})).is_err(),
            "the hand-authored lane must fail loudly on a misspelled key"
        );
    }
}
