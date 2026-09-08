//! Canonical **live** wire form of a [`Card`] for language-binding APIs.
//!
//! [`CardWire`] is the single, core-owned translation between a [`Card`] and the
//! flat `{ kind, payloadItems, … }` shape the WASM and Python bindings exchange,
//! so the field/comment/`$`-entry mapping lives in one place.
//!
//! Separate from the versioned storage DTO (`document::dto`), which is frozen per
//! schema version: `CardWire` is the current API shape and evolves with the
//! bindings, and coupling the two would chain their change cadences.
//!
//! The `$` system entries are hoisted to named fields (`kind`, `quill`, `ext`,
//! `seed`); `payload_items` carries only user fields and comments, in order.
//! Nested comments are not represented here: they survive the Markdown and
//! storage round-trips, not this editable projection.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::payload::{MetaKey, Payload, PayloadItem};
use super::{Card, EditError};
use crate::error::diag_args;
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;
use crate::{Diagnostic, Severity};
use quillmark_content::Normalized;

/// One entry in a [`CardWire`]'s `payload_items`: a user field or a comment.
/// The `$` system entries are hoisted onto [`CardWire`] itself, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PayloadItemWire {
    /// A user-defined field.
    Field {
        key: String,
        value: JsonValue,
        /// `true` when the field itself is `key: !must_fill <value>` in source.
        #[serde(default)]
        fill: bool,
        /// Paths to `!must_fill` markers nested *inside* `value`, whose JSON
        /// projection is fill-free. Empty for a top-level-only or no-fill field.
        #[serde(
            default,
            rename = "nestedFills",
            alias = "nested_fills",
            skip_serializing_if = "Vec::is_empty"
        )]
        nested_fills: Vec<Vec<PathSegment>>,
    },
    /// A YAML comment line (text excludes the leading `#`).
    Comment {
        text: String,
        /// `true` for a trailing inline comment (`field: value # text`).
        #[serde(default)]
        inline: bool,
    },
}

/// Canonical live wire form of a [`Card`]. See the module docs.
///
/// Serializes to JS-facing camelCase (`payloadItems`); the snake_case
/// `payload_items` is also accepted on input for the Python binding.
/// `deny_unknown_fields` makes a stale flat `{ kind, fields }` shape fail
/// loudly rather than deserialize into an empty card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardWire {
    /// The block's `$kind` (e.g. `"endorsement"`); empty string when the block
    /// declares no `$kind`. Kept non-optional to match the binding read shape.
    #[serde(default)]
    pub kind: String,
    /// The block's `$quill` reference string (`name@version`), present on the
    /// main card only. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quill: Option<String>,
    /// The block's opaque `$ext` map, if declared. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<JsonMap<String, JsonValue>>,
    /// The block's `$seed` map (keyed by card-kind), if declared. Present on
    /// the main card only. Omitted when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<JsonMap<String, JsonValue>>,
    /// User fields and comments, in source order.
    #[serde(default, alias = "payload_items")]
    pub payload_items: Vec<PayloadItemWire>,
    /// The card body as canonical Content-JSON: the source-of-truth content
    /// model (a content object, `{text, lines, marks, islands}`). The empty content
    /// when absent. A markdown string is also accepted on input (imported), so an
    /// LLM/markdown writer can hand a string here.
    ///
    /// The **seam** form (`serial::to_seam_value`): this wire is a binding read
    /// that is also a binding write input, so every `Container::instance` is
    /// spelled and the read type can require it. `payload_items` stays in the
    /// storage form: verbatim is its contract, and is why the binding types it
    /// `unknown`.
    ///
    /// No `body_markdown` projection rides this wire: delimiter safety makes
    /// `to_markdown` re-parse every rendered line, so the `exportMarkdown(body)`
    /// codec at the binding boundary does it on demand instead.
    #[serde(default)]
    pub body: JsonValue,
}

impl CardWire {
    /// A card block with no `$`-prefixed system metadata. `kind` is the empty
    /// string for a block declaring none. `body` is canonical Content-JSON, or
    /// a markdown string the reader imports.
    pub fn new(kind: String, body: JsonValue) -> Self {
        Self {
            kind,
            quill: None,
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body,
        }
    }
}

/// Failure converting a [`CardWire`] back into a [`Card`].
///
/// Building a card from a wire is a mutator door, so every violation travels
/// under the [`code`](Self::code) the *addressed* mutator onto it mints —
/// [`Card::store_field`]'s for a field name, `parse::invalid_quill_reference`
/// for a `$quill` string — and a binding routes one door as it routes the other.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WireError {
    /// The `quill` string is not a valid `name@version` reference.
    #[error("invalid `quill` reference {value:?}: {reason}")]
    InvalidQuillReference { value: String, reason: String },
    /// A field, an `$ext`/`$seed` map, the body, or the item list as a whole
    /// violates an invariant the mutators enforce.
    #[error(transparent)]
    Edit(#[from] EditError),
}

impl WireError {
    /// The namespaced diagnostic `code`, one per violation and the same the
    /// addressed mutator onto it mints. Taxonomy: `prose/canon/ERROR.md`.
    pub fn code(&self) -> &'static str {
        match self {
            WireError::InvalidQuillReference { .. } => "parse::invalid_quill_reference",
            WireError::Edit(err) => err.code(),
        }
    }

    /// The [`Diagnostic`] a binding raises for this refusal, carrying
    /// [`code`](Self::code) and the facts its message interpolates. The card is
    /// not placed yet, so nothing anchors it: no `path` rides here.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let diag = Diagnostic::new(Severity::Error, self.to_string())
            .with_code(self.code().to_string());
        match self {
            WireError::InvalidQuillReference { value, .. } => diag
                .with_args(diag_args! { "value" => value })
                .with_hint(crate::version::quill_ref_hint().to_string()),
            WireError::Edit(err) => diag.with_args(err.args()),
        }
    }
}

impl From<&Card> for CardWire {
    fn from(card: &Card) -> Self {
        let mut wire = CardWire {
            kind: String::new(),
            quill: None,
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body: quillmark_content::serial::to_seam_value(card.body()),
        };
        for item in card.payload().items() {
            match item {
                PayloadItem::Quill { reference } => wire.quill = Some(reference.to_string()),
                PayloadItem::Kind { value } => wire.kind = value.clone(),
                PayloadItem::Meta {
                    key: MetaKey::Ext,
                    value,
                    ..
                } => wire.ext = Some(value.clone()),
                PayloadItem::Meta {
                    key: MetaKey::Seed,
                    value,
                    ..
                } => wire.seed = Some(value.clone()),
                PayloadItem::Field {
                    key, value, fill, ..
                } => {
                    let nested_fills = value.nonroot_fill_paths().collect();
                    wire.payload_items.push(PayloadItemWire::Field {
                        key: key.clone(),
                        value: value.as_json().clone(),
                        fill: *fill,
                        nested_fills,
                    })
                }
                PayloadItem::Comment { text, inline } => {
                    wire.payload_items.push(PayloadItemWire::Comment {
                        text: text.clone(),
                        inline: *inline,
                    })
                }
            }
        }
        wire
    }
}

impl TryFrom<CardWire> for Card {
    type Error = WireError;

    fn try_from(wire: CardWire) -> Result<Self, Self::Error> {
        let items = wire
            .payload_items
            .into_iter()
            .map(|item| match item {
                PayloadItemWire::Field {
                    key,
                    value,
                    fill,
                    nested_fills,
                } => {
                    let refuse =
                        |v| WireError::Edit(super::edit::edit_error_from_violation(&key, v));
                    super::edit::validate_field(&key, &value).map_err(refuse)?;
                    let mut qv = QuillValue::from_json(value);
                    for path in &nested_fills {
                        qv.set_fill_at(path);
                    }
                    // The fill-target check reads the wire's nested markers off
                    // the value, so it runs after `set_fill_at`.
                    super::edit::validate_fill_targets(&qv, fill).map_err(refuse)?;
                    Ok(PayloadItem::Field {
                        key,
                        value: qv,
                        fill,
                        nested_comments: Vec::new(),
                    })
                }
                PayloadItemWire::Comment { text, inline } => {
                    Ok(PayloadItem::Comment { text, inline })
                }
            })
            .collect::<Result<Vec<_>, WireError>>()?;

        // Applying each `$` entry through its setter keeps the canonical
        // `$quill < $kind < $ext < $seed` ordering regardless of input order.
        let mut payload = Payload::from_items(items);
        if let Some(value) = wire.quill {
            let reference = QuillReference::from_str(&value)
                .map_err(|reason| WireError::InvalidQuillReference { value, reason })?;
            payload.set_quill(reference);
        }
        // No `$kind` check, and none on `$quill`/`$seed` above: all three are
        // positional — `main` is right for the root and reserved for a
        // composable card, `$quill`/`$seed` bind the root — and a `CardWire` is
        // equally how the main card is read back and rewritten, so it carries no
        // signal of which it is. All three belong to `push_card`/`insert_card`.
        // Checking only the grammar here would split one user-facing concept
        // across two error types and shadow the routable `EditError` code.
        if !wire.kind.is_empty() {
            payload.set_kind(wire.kind);
        }
        let too_deep = |max| WireError::Edit(EditError::ValueTooDeep { max });
        if let Some(ext) = wire.ext {
            payload.set_ext(crate::value::depth_check_meta_map(ext, too_deep)?);
        }
        if let Some(seed) = wire.seed {
            payload.set_seed(crate::value::depth_check_meta_map(seed, too_deep)?);
        }
        super::edit::validate_payload(&payload)
            .map_err(|v| WireError::Edit(EditError::InvalidPayload(v)))?;
        let body = body_from_wire(&wire.body)?;
        Ok(Card::from_parts(payload, body))
    }
}

/// Read a [`CardWire::body`] into a [`Content`] content. The body is the source
/// of truth and reads through the richtext codec whatever the schema declares,
/// so its accepted encodings are [`super::Codec::decode_field`]'s.
fn body_from_wire(body: &JsonValue) -> Result<Normalized, WireError> {
    super::Codec::Richtext.decode_field(body).map_err(|e| {
        WireError::Edit(super::edit::field_decode(
            "$body",
            &[],
            super::Codec::Richtext,
            e,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Codec;
    use serde_json::json;

    /// Nested `!must_fill` markers inside a field value survive Card → wire →
    /// Card via the `nestedFills` path list (the JSON projection is fill-free).
    #[test]
    fn card_wire_round_trips_nested_fill() {
        let mut addr = QuillValue::from_json(json!({"street": null, "city": "Anytown"}));
        assert!(addr.set_fill_at(&[PathSegment::Key("street".to_string())]));
        let payload = Payload::from_items(vec![PayloadItem::Field {
            key: "addr".to_string(),
            value: addr,
            fill: false,
            nested_comments: Vec::new(),
        }]);
        let card = Card::from_parts(payload, quillmark_content::Normalized::empty());

        let wire = CardWire::from(&card);
        let as_json = serde_json::to_value(&wire).unwrap();
        assert_eq!(
            as_json["payloadItems"][0]["nestedFills"],
            json!([["street"]]),
            "nested fill path rides the wire as a JS array; JSON value stays fill-free"
        );
        assert_eq!(
            as_json["payloadItems"][0]["value"],
            json!({"street": null, "city": "Anytown"})
        );

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "nested fill must survive Card → wire → Card");
    }

    /// A `nestedFills` path is untagged JSON — a string per key, a number per
    /// index — so it crosses the binding wire as a plain JS array.
    #[test]
    fn nested_fill_path_segments_are_untagged() {
        let mut value = QuillValue::from_json(json!({"to": [{"name": null}]}));
        assert!(value.set_fill_at(&[
            PathSegment::Key("to".to_string()),
            PathSegment::Index(0),
            PathSegment::Key("name".to_string()),
        ]));
        let payload = Payload::from_items(vec![PayloadItem::Field {
            key: "recipients".to_string(),
            value,
            fill: false,
            nested_comments: Vec::new(),
        }]);
        let wire = CardWire::from(&Card::from_parts(
            payload,
            quillmark_content::Normalized::empty(),
        ));

        let as_json = serde_json::to_value(&wire).unwrap();
        assert_eq!(
            as_json["payloadItems"][0]["nestedFills"],
            json!([["to", 0, "name"]]),
            "a key rides as a JSON string and an index as a JSON number"
        );

        let back: CardWire = serde_json::from_value(as_json).expect("JSON → wire");
        assert_eq!(back, wire, "an untagged path deserializes back unchanged");
    }

    /// The emitted `key: !must_fill` has no line for a block mapping, so the
    /// wire refuses one where parse does — at the root and nested.
    #[test]
    fn card_wire_refuses_a_fill_marked_mapping() {
        let field = |key: &str, value: JsonValue, fill: bool, nested: Vec<Vec<PathSegment>>| {
            let mut wire = CardWire::new("note".to_string(), JsonValue::Null);
            wire.payload_items.push(PayloadItemWire::Field {
                key: key.to_string(),
                value,
                fill,
                nested_fills: nested,
            });
            wire
        };

        let err = Card::try_from(field("x", json!({"a": 1}), true, Vec::new()))
            .expect_err("a fill-marked mapping is refused");
        assert!(
            matches!(&err, WireError::Edit(EditError::FillOnMapping { field }) if field == "x"),
            "{err:?}"
        );

        let nested = vec![vec![PathSegment::Key("inner".to_string())]];
        let err = Card::try_from(field("addr", json!({"inner": {"a": 1}}), false, nested))
            .expect_err("a nested fill on a mapping is refused");
        assert!(
            matches!(&err, WireError::Edit(EditError::FillOnMapping { field }) if field == "addr"),
            "{err:?}"
        );

        // The one mapping a marker may target: emit projects it to a markdown
        // scalar first.
        let content = quillmark_content::import::from_markdown("Q3 results").expect("content");
        let canonical = quillmark_content::serial::to_canonical_value(&content);
        Card::try_from(field("subject", canonical, true, Vec::new()))
            .expect("a fill-marked content object still crosses");
    }

    /// A content object rides the wire structurally and losslessly, so an
    /// `underline` with no markdown projection survives Card → wire → Card.
    #[test]
    fn card_wire_round_trips_content_field_losslessly() {
        use quillmark_content::model::{Mark, MarkKind};

        let mut card = Card::new("note").unwrap();
        let mut content = quillmark_content::import::from_markdown("underlined intro").unwrap().into_content();
        content.marks.push(Mark::new(0, 10, MarkKind::Underline));
        let content = content.into_normalized();
        let json = quillmark_content::serial::to_canonical_value(&content);
        let schema = crate::quill::FieldSchema::new(
            "intro".to_string(),
            crate::quill::FieldType::RichText { inline: false },
            None,
        );
        card.commit_field("intro", crate::QuillValue::from_json(json), &schema)
            .unwrap();

        let wire = CardWire::from(&card);
        let as_json = serde_json::to_value(&wire).unwrap();
        assert!(as_json["payloadItems"][0]["value"].is_object());

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "content field must survive Card → wire → Card");
        let read = back.field_content("intro", Codec::Richtext).unwrap().unwrap();
        assert!(read.marks.iter().any(|m| matches!(m.kind, MarkKind::Underline)));
    }

    /// A field-and-comment card with `$kind` round-trips Card → wire → Card.
    #[test]
    fn card_wire_round_trips_fields_and_comment() {
        let mut payload = Payload::from_items(vec![
            PayloadItem::comment("a note"),
            PayloadItem::field("title", QuillValue::from_json(json!("Hi"))),
            PayloadItem::Field {
                key: "count".to_string(),
                value: QuillValue::from_json(json!(3)),
                fill: true,
                nested_comments: Vec::new(),
            },
        ]);
        payload.set_kind("note");
        let card = Card::from_parts(payload, crate::document::import_body("body text").unwrap());

        let wire = CardWire::from(&card);
        assert_eq!(wire.kind, "note");
        assert_eq!(wire.payload_items.len(), 3);

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card, "Card → wire → Card must be identity");
    }

    /// `$quill` (main card) survives the round-trip and parses back.
    #[test]
    fn card_wire_round_trips_quill() {
        let mut payload = Payload::from_index_map(Default::default());
        payload.set_quill("memo@1.2.3".parse().unwrap());
        payload.set_kind("main");
        let card = Card::from_parts(payload, quillmark_content::Normalized::empty());

        let wire = CardWire::from(&card);
        assert_eq!(wire.quill.as_deref(), Some("memo@1.2.3"));

        let back = Card::try_from(wire).expect("wire → card");
        assert_eq!(back, card);
    }

    /// The wire JSON uses camelCase `payloadItems` and the `type`-tagged items.
    #[test]
    fn card_wire_json_shape() {
        let card = Card::try_from(CardWire {
            kind: "note".to_string(),
            quill: None,
            ext: None,
            seed: None,
            payload_items: vec![PayloadItemWire::Field {
                key: "x".to_string(),
                value: json!(1),
                fill: false,
                nested_fills: Vec::new(),
            }],
            body: JsonValue::Null,
        })
        .unwrap();
        let json = serde_json::to_value(CardWire::from(&card)).unwrap();
        assert_eq!(json["kind"], json!("note"));
        assert_eq!(json["payloadItems"][0]["type"], json!("field"));
        assert_eq!(json["payloadItems"][0]["key"], json!("x"));
        assert!(json.get("quill").is_none(), "absent quill is omitted");
    }

    /// A malformed `quill` string is a typed error, not a panic, and carries
    /// the code and grammar hint of every door that parses a reference.
    #[test]
    fn card_wire_rejects_bad_quill() {
        let err = Card::try_from(CardWire {
            kind: String::new(),
            quill: Some("@nope".to_string()),
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body: JsonValue::Null,
        })
        .unwrap_err();
        assert_eq!(err.code(), "parse::invalid_quill_reference");
        assert!(
            err.to_diagnostic().hint.is_some(),
            "every door that parses a `$quill` reference carries the grammar"
        );
    }

    /// Two doors onto one violation: a card built from a wire refuses under the
    /// code the addressed mutator mints for the same field, so `insertCard` and
    /// `storeField` are routable alike.
    #[test]
    fn card_wire_refuses_a_field_under_the_mutator_code() {
        let refused = |key: &str, value: JsonValue, fill: bool| {
            let mut wire = CardWire::new("note".to_string(), JsonValue::Null);
            wire.payload_items.push(PayloadItemWire::Field {
                key: key.to_string(),
                value,
                fill,
                nested_fills: Vec::new(),
            });
            Card::try_from(wire).expect_err("the wire refuses it")
        };
        let mut card = Card::new("note").unwrap();
        let deep = (0..=quillmark_content::MAX_JSON_DEPTH)
            .fold(json!(1), |v, _| json!({ "a": v }));

        for (wire_err, mutator_err) in [
            (
                refused("bad-name", json!(1), false),
                card.store_field("bad-name", QuillValue::from_json(json!(1)))
                    .unwrap_err(),
            ),
            (
                refused("deep", deep.clone(), false),
                card.store_field("deep", QuillValue::from_json(deep))
                    .unwrap_err(),
            ),
            (
                refused("addr", json!({"a": 1}), true),
                card.store_fill("addr", QuillValue::from_json(json!({"a": 1})))
                    .unwrap_err(),
            ),
        ] {
            assert_eq!(wire_err.code(), mutator_err.code(), "{wire_err:?}");
            assert_eq!(
                wire_err.to_diagnostic().args,
                mutator_err.args(),
                "{wire_err:?}"
            );
        }
    }

    /// `$body` reads through the richtext codec: a null is the empty content, a
    /// markdown string imports, and any other shape is refused under the codec's
    /// own sentence, naming the shape that arrived.
    #[test]
    fn card_wire_body_reads_through_the_richtext_codec() {
        let with_body = |body: JsonValue| Card::try_from(CardWire::new("note".to_string(), body));

        assert_eq!(
            with_body(JsonValue::Null).unwrap().body(),
            &quillmark_content::Normalized::empty()
        );
        assert_eq!(
            Codec::Richtext.project(with_body(json!("hi *there*")).unwrap().body()),
            "hi *there*"
        );

        let err = with_body(json!(true)).unwrap_err();
        assert!(
            matches!(&err, WireError::Edit(EditError::FieldDecode { field, message, .. })
                if field == "$body"
                    && message
                        == "expected a richtext content object or a markdown string, got a boolean"),
            "got {err:?}"
        );
        for (body, tail) in [(json!(3), "a number"), (json!([]), "an array")] {
            let err = with_body(body).unwrap_err();
            assert!(
                matches!(&err, WireError::Edit(EditError::FieldDecode { message, .. })
                    if message.ends_with(tail)),
                "got {err:?}"
            );
        }
    }

    /// Construction accepts a kind the mutators reject: `make_card` is
    /// permissive data-shaping and `insert_card` is the gate, so the grammar
    /// check belongs there, not here.
    #[test]
    fn card_wire_accepts_any_kind() {
        let card = Card::try_from(CardWire {
            kind: "BadKind".to_string(),
            quill: None,
            ext: None,
            seed: None,
            payload_items: Vec::new(),
            body: JsonValue::Null,
        })
        .expect("construction does not police the kind grammar");
        assert_eq!(card.kind(), Some("BadKind"));
    }
}
