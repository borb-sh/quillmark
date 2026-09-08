//! Auto-generated Markdown blueprint for a Quill: an annotated reference
//! document dense enough to replace the schema for LLM consumers.
//!
//! `blueprint()` builds a [`Document`] and emits it through
//! [`Document::to_markdown`], so the blueprint round-trips through
//! `Document::parse` by construction: there is no second formatter.

use indexmap::IndexMap;

use super::{CardSchema, FieldSchema, FieldType, QuillConfig, VARIANT_DISCRIMINANT_KEY};
use crate::document::emit::{saphyr_emit_flow, saphyr_emit_scalar};
use crate::document::prescan::NestedComment;
use crate::document::{Card, Document, Payload, PayloadItem};
use crate::value::{PathSegment, QuillValue};
use serde_json::{Map as JsonMap, Value as JsonValue};

impl QuillConfig {
    /// Generate the canonical annotated Markdown blueprint for this quill:
    /// the authoring surface handed to LLMs and humans, with every must-fill
    /// cell carrying the `!must_fill` marker. The annotation grammar is
    /// `prose/canon/BLUEPRINT.md` §Annotation grammar; the function is total
    /// over any valid `QuillConfig`.
    ///
    /// The "filled-out" twin of the blueprint is **seeding**
    /// ([`Quill::seed_document`](crate::Quill::seed_document)), a committed
    /// [`Document`] rather than an annotated string. See
    /// `prose/canon/BLUEPRINT.md`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render:
    /// that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    ///
    /// [`Document`]: crate::Document
    pub fn blueprint(&self) -> String {
        let main_desc = collapse_opt(self.main.description.as_deref())
            .or_else(|| collapse_opt(Some(self.description.as_str())));

        let main = build_main_card(
            &self.main,
            &format!("{}@{}", self.name, self.version),
            main_desc,
        );
        let cards = self.card_kinds.iter().map(build_card).collect();

        Document::from_main_and_cards(main, cards).to_markdown()
    }
}

/// Whitespace-collapse a description into a single line; `None` when it
/// collapses to empty.
fn collapse_opt(text: Option<&str>) -> Option<String> {
    text.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|clean| !clean.is_empty())
}

/// The text after `# ` for a body region (`Write … here.` placeholder or the
/// configured `body.example`), wrapped so `to_markdown` emits it verbatim
/// after the closing fence. Empty when the card has no body.
fn body_text(card: &CardSchema, fallback_kind: &str) -> String {
    if !card.body_enabled() {
        return String::new();
    }
    let example = card.body.as_ref().and_then(|b| b.example.as_deref());
    let fallback = body_placeholder(fallback_kind);
    let text = example.unwrap_or(fallback.as_str());
    format!("\n{}\n", text)
}

/// The body text a kind declaring no `body.example` shows.
fn body_placeholder(kind: &str) -> String {
    format!("Write {kind} body here.")
}

/// Build the root card: `$quill` (with the `# keep verbatim` inline reminder),
/// `$kind: main`, the optional description own-line comment, then the fields.
fn build_main_card(card: &CardSchema, quill_ref: &str, description: Option<String>) -> Card {
    let reference = quill_ref
        .parse()
        .expect("quill name@version is always a valid QuillReference");
    let mut items = vec![
        PayloadItem::Quill { reference },
        PayloadItem::comment_inline("keep verbatim"),
        PayloadItem::Kind {
            value: "main".into(),
        },
    ];
    if let Some(desc) = description {
        items.push(PayloadItem::comment(desc));
    }
    append_fields(&mut items, card);
    Card::from_parts(
        Payload::from_items(items),
        // The empty-content fallback is defensive: a placeholder or a
        // load-validated example never over-nests.
        crate::document::import_body(&body_text(card, "main"))
            .unwrap_or_else(|_| quillmark_content::Normalized::empty()),
    )
}

/// Build a composable card: `$kind: <kind>`, the `composable (0..N)` role
/// comment, a comment naming it a deletable sample, the optional description,
/// then the fields.
fn build_card(card: &CardSchema) -> Card {
    let mut items = vec![
        PayloadItem::Kind {
            value: card.name.clone(),
        },
        PayloadItem::comment("composable (0..N)"),
        PayloadItem::comment("sample card; delete if not needed"),
    ];
    if let Some(desc) = collapse_opt(card.description.as_deref()) {
        items.push(PayloadItem::comment(desc));
    }
    append_fields(&mut items, card);
    Card::from_parts(
        Payload::from_items(items),
        crate::document::import_body(&body_text(card, &card.name))
            .unwrap_or_else(|_| quillmark_content::Normalized::empty()),
    )
}

/// Append every field of a card as payload items, in [`group_fields`] order.
fn append_fields(items: &mut Vec<PayloadItem>, card: &CardSchema) {
    let registry: Vec<&str> = card
        .ui
        .as_ref()
        .and_then(|u| u.groups.as_ref())
        .map(|r| r.0.iter().map(|g| g.id.as_str()).collect())
        .unwrap_or_default();
    for field in group_fields(card.fields.values(), &registry) {
        append_field(items, field);
    }
}

/// Order fields by `ui.group`: ungrouped lead, then grouped clusters in
/// `registry` declaration order, each cluster keeping declaration order. The
/// clusters flatten into one field stream, grouping being positional only.
fn group_fields<'a, I: IntoIterator<Item = &'a FieldSchema>>(
    fields: I,
    registry: &[&str],
) -> Vec<&'a FieldSchema> {
    let mut groups: Vec<(Option<&str>, Vec<&FieldSchema>)> = Vec::new();
    for field in fields {
        let group = field.ui.as_ref().and_then(|u| u.group.as_deref());
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some(slot) => slot.1.push(field),
            None => groups.push((group, vec![field])),
        }
    }
    // Ungrouped is the implicit leading pseudo-group (rank 0); a grouped cluster
    // ranks by its registry position shifted past it. Load rejects a `ui.group`
    // the card's registry does not declare (`quill::unknown_group`,
    // `quill::implicit_group`), so the lookup resolves for any config that came
    // through it; a hand-built one sorts its stray group last.
    groups.sort_by_key(|(g, _)| match g {
        None => 0,
        Some(id) => registry
            .iter()
            .position(|o| o == id)
            .map_or(usize::MAX, |pos| pos + 1),
    });
    groups.into_iter().flat_map(|(_, fields)| fields).collect()
}

/// Append one top-level field. Dispatches typed tables (`array<object>`) and
/// typed dictionaries (`object` with `properties`) to their per-property
/// builders; everything else is a scalar/array cell.
fn append_field(items: &mut Vec<PayloadItem>, field: &FieldSchema) {
    if field.is_variant_bearing() {
        append_variant(items, field);
        return;
    }

    if typed_dict_props(field).is_some() || typed_table_props(field).is_some() {
        // The container key itself is untagged: `!must_fill` is rejected on a
        // mapping (`prose/references/markdown-spec.md` §3.4), so the obligation
        // lives on the leaves.
        push_leading(items, field, true);
        let (value, nested, fills) = container_cell(field, &[]);
        push_container_field(items, &field.name, value, nested, fills, field);
        return;
    }

    append_scalar(items, field);
}

fn typed_dict_props(field: &FieldSchema) -> Option<&IndexMap<String, Box<FieldSchema>>> {
    match field.r#type {
        FieldType::Object => field.properties.as_ref(),
        _ => None,
    }
}

fn typed_table_props(field: &FieldSchema) -> Option<&IndexMap<String, Box<FieldSchema>>> {
    match field.r#type {
        FieldType::Array => match field.items.as_deref() {
            Some(elem) => typed_dict_props(elem),
            None => None,
        },
        _ => None,
    }
}

/// Whether a leaf's `example:` surfaces as a `# e.g.` hint rather than as the
/// cell's own value: a `default:` already occupies the cell, or the leaf is
/// richtext, whose cell an example never inlines (see `scalar_value`). The gate
/// is a value-axis question and stays keyed on `default`: `must_fill` never
/// moves an example between the cell and the hint.
fn eg_hinted(field: &FieldSchema) -> bool {
    field.default.is_some() || matches!(field.r#type, FieldType::RichText { .. })
}

/// Push the leading prose comments for a *top-level* field: the description,
/// then the `# e.g.` hint. `eg_when` gates the hint: a leaf surfaces it under
/// `eg_hinted`, while typed containers always surface it (their example never
/// inlines).
fn push_leading(items: &mut Vec<PayloadItem>, field: &FieldSchema, eg_when: bool) {
    if let Some(desc) = collapse_opt(field.description.as_deref()) {
        items.push(PayloadItem::comment(desc));
    }
    if eg_when {
        if let Some(eg) = field.example.as_ref() {
            items.push(PayloadItem::comment(format!("e.g. {}", eg_hint(eg))));
        }
    }
}

/// The cell's `(value, fill)` for a scalar/array/richtext leaf. The value is
/// `default:` › `example:` › bare null; the marker is `default:`'s absence, so
/// an `example` always arrives under it.
fn scalar_cell(field: &FieldSchema) -> (JsonValue, bool) {
    (scalar_value(field), field.must_fill())
}

/// The value half. A richtext leaf never inlines an example: its `example:`
/// surfaces as a `# e.g.` hint instead (see `append_scalar`), so its
/// defaultless cell is always bare.
fn scalar_value(field: &FieldSchema) -> JsonValue {
    if let Some(default) = &field.default {
        return default.as_json().clone();
    }
    if matches!(field.r#type, FieldType::RichText { .. }) {
        return JsonValue::Null;
    }
    match field.example.as_ref() {
        Some(eg) => eg.as_json().clone(),
        None => JsonValue::Null,
    }
}

/// Append a scalar / scalar-array / richtext field as a single payload field
/// plus its trailing inline type annotation.
fn append_scalar(items: &mut Vec<PayloadItem>, field: &FieldSchema) {
    push_leading(items, field, eg_hinted(field));
    let (json, fill) = scalar_cell(field);
    items.push(PayloadItem::Field {
        key: field.name.clone(),
        value: QuillValue::from_json(json),
        fill,
        nested_comments: Vec::new(),
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the per-property body of a defaultless typed container into `map`:
/// each property at its own cell in declaration order, plus the nested comments
/// (description + `# e.g.` + inline type annotation, addressed by
/// `container_path`/slot) and the nested fill paths. `prefix` is the container
/// path of the mapping relative to the field value (`[]` for a typed dict,
/// `[Index(0)]` for a typed table's synthetic row).
///
/// A property's slot is where it lands in `map`, so a caller seating its own
/// cell first (a variant's discriminant) shifts the rest by handing over a
/// mapping that already holds it.
fn build_property_mapping(
    map: &mut JsonMap<String, JsonValue>,
    props: &IndexMap<String, Box<FieldSchema>>,
    prefix: &[PathSegment],
) -> (Vec<NestedComment>, Vec<Vec<PathSegment>>) {
    let mut nested = Vec::new();
    let mut fills = Vec::new();
    for prop in props.values().map(|b| b.as_ref()) {
        let slot = map.len();
        if let Some(desc) = collapse_opt(prop.description.as_deref()) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: desc,
                inline: false,
            });
        }
        if eg_hinted(prop) {
            if let Some(eg) = prop.example.as_ref() {
                nested.push(NestedComment {
                    container_path: prefix.to_vec(),
                    position: slot,
                    text: format!("e.g. {}", eg_hint(eg)),
                    inline: false,
                });
            }
        }
        let mut path = prefix.to_vec();
        path.push(PathSegment::Key(prop.name.clone()));
        let (json, sub_nested, sub_fills) = property_cell(prop, &path);
        map.insert(prop.name.clone(), json);
        nested.extend(sub_nested);
        fills.extend(sub_fills);
        nested.push(NestedComment {
            container_path: prefix.to_vec(),
            position: slot,
            text: type_expression(prop),
            inline: true,
        });
    }
    (nested, fills)
}

/// One property's contribution to its parent's mapping: its value, and the
/// nested comments and fill paths its own subtree carries. `path` is the
/// property's address relative to the field value.
fn property_cell(
    prop: &FieldSchema,
    path: &[PathSegment],
) -> (JsonValue, Vec<NestedComment>, Vec<Vec<PathSegment>>) {
    if typed_dict_props(prop).is_some() || typed_table_props(prop).is_some() {
        return container_cell(prop, path);
    }
    let (json, fill) = scalar_cell(prop);
    let fills = if fill { vec![path.to_vec()] } else { Vec::new() };
    (json, Vec::new(), fills)
}

/// A container's value plus the nested comments and fill paths its subtree
/// carries, at `path` relative to the field value (`[]` at card level).
///
/// An **array** `default:` is shippable as-is, so it renders verbatim and its
/// subtree carries neither marker nor annotation. A **typed dictionary** holds
/// no literal (`quill::default_on_namespace`), so it always expands per
/// property.
fn container_cell(
    field: &FieldSchema,
    path: &[PathSegment],
) -> (JsonValue, Vec<NestedComment>, Vec<Vec<PathSegment>>) {
    if let Some(props) = typed_dict_props(field) {
        let mut map = JsonMap::new();
        let (nested, fills) = build_property_mapping(&mut map, props, path);
        return (JsonValue::Object(map), nested, fills);
    }

    let row_props = typed_table_props(field).unwrap_or_else(|| {
        unreachable!("container_cell is reached only for a typed dictionary or a typed table")
    });
    match field.default.as_ref().map(|d| d.as_json()) {
        // `[]` included: an array default stays inline rather than expanding.
        Some(default) => (default.clone(), Vec::new(), Vec::new()),
        // A row type declaring no properties is schema-invalid in practice:
        // emit a type-valid empty array rather than a null synthetic row.
        None if row_props.is_empty() => (JsonValue::Array(Vec::new()), Vec::new(), Vec::new()),
        None => {
            let mut row_path = path.to_vec();
            row_path.push(PathSegment::Index(0));
            let mut row = JsonMap::new();
            let (nested, fills) = build_property_mapping(&mut row, row_props, &row_path);
            (
                JsonValue::Array(vec![JsonValue::Object(row)]),
                nested,
                fills,
            )
        }
    }
}

/// Append a variant-bearing enum: the container, its discriminant cell, the
/// fields of the world that discriminant selects, and a
/// `# when <MEMBER>: <fields>` line for every other world that owns a field set
/// (`prose/canon/BLUEPRINT.md` § "Enum variants").
fn append_variant(items: &mut Vec<PayloadItem>, field: &FieldSchema) {
    push_leading(items, field, field.default.is_some());
    if let Some(variants) = &field.variants {
        for (member, fields) in variants {
            let names: Vec<&str> = fields.keys().map(String::as_str).collect();
            items.push(PayloadItem::comment(format!(
                "when {member}: {}",
                names.join(", ")
            )));
        }
    }

    let member = scalar_value(field);
    let mut map = JsonMap::new();
    let mut fills = Vec::new();
    map.insert(
        VARIANT_DISCRIMINANT_KEY.to_string(),
        match &member {
            // A null discriminant would read as "no cell"; the blank is the
            // enum's own spelling of an unanswered choice, and it round-trips.
            JsonValue::Null => JsonValue::String(String::new()),
            other => other.clone(),
        },
    );
    if field.must_fill() {
        fills.push(vec![PathSegment::Key(VARIANT_DISCRIMINANT_KEY.to_string())]);
    }

    // A cell is a field of the container, so it expands as one: the mapping
    // already holding the discriminant is what seats the live world one slot
    // past it.
    let live = member.as_str().and_then(|m| field.variant_fields(m));
    let nested = match live {
        Some(fields) => {
            let (nested, sub_fills) = build_property_mapping(&mut map, fields, &[]);
            fills.extend(sub_fills);
            nested
        }
        None => Vec::new(),
    };

    push_container_field(
        items,
        &field.name,
        JsonValue::Object(map),
        nested,
        fills,
        field,
    );
}

/// Push a typed-container field (value + nested comments + nested fills) and
/// its trailing inline type annotation. The top-level `fill` flag is always
/// `false`: typed containers are tagged on their leaves, never the container.
fn push_container_field(
    items: &mut Vec<PayloadItem>,
    key: &str,
    value: JsonValue,
    nested_comments: Vec<NestedComment>,
    fills: Vec<Vec<PathSegment>>,
    field: &FieldSchema,
) {
    let mut quill_value = QuillValue::from_json(value);
    for path in &fills {
        quill_value.set_fill_at(path);
    }
    items.push(PayloadItem::Field {
        key: key.to_string(),
        value: quill_value,
        fill: false,
        nested_comments,
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the inline annotation body (without the leading `# `): purely the
/// structural type expression `<type>[<format>]`. Shippability is carried by
/// the value cell alone (a concrete value is shippable as-is, a `!must_fill`
/// marker asks to be filled) so the annotation needs no cell-state tag.
fn type_expression(field: &FieldSchema) -> String {
    if let Some(values) = &field.enum_values {
        return format!("enum<{}>", values.join(" | "));
    }
    match field.r#type {
        FieldType::String => "string".into(),
        FieldType::Number => "number".into(),
        FieldType::Integer => "integer".into(),
        FieldType::Boolean => "boolean".into(),
        FieldType::Object => "object".into(),
        // The type names the role; the `<markdown>` format slot names the
        // surface encoding an author writes (and `to_markdown` re-emits).
        FieldType::RichText { inline: false } => "richtext<markdown>".into(),
        FieldType::RichText { inline: true } => "richtext(inline)<markdown>".into(),
        // The `<plain>` format slot names the literal codec (`from_plaintext`/
        // `to_plaintext`): content the author navigates but which takes no
        // markup, distinct from richtext's `<markdown>` surface.
        FieldType::PlainText { inline: false } => "plaintext<plain>".into(),
        FieldType::PlainText { inline: true } => "plaintext(inline)<plain>".into(),
        // `enum` fields always carry `enum_values`, so the early return above
        // handles them; this arm is the defensive fallback for a valueless enum.
        FieldType::Enum => "enum".into(),
        FieldType::Date => "date<YYYY-MM-DD>".into(),
        FieldType::DateTime => "datetime<YYYY-MM-DDThh:mm[:ss]>".into(),
        // The element type comes from `items`; a scalar element gives
        // `array<string>`/`array<integer>`/`array<markdown>`, an object
        // element gives `array<object>`.
        FieldType::Array => {
            let item = field
                .items
                .as_ref()
                .map(|it| type_expression(it))
                .unwrap_or_else(|| "string".into());
            format!("array<{}>", item)
        }
    }
}

/// Format an example value as a compact one-line hint. Arrays and objects
/// render as YAML flow collections (`[a, b, c]`, `{k: v}`) so multi-element
/// shape information is preserved without expanding into multiple comment
/// lines.
fn eg_hint(example: &QuillValue) -> String {
    match example.as_json() {
        v @ (serde_json::Value::Array(_) | serde_json::Value::Object(_)) => saphyr_emit_flow(v),
        val => saphyr_emit_scalar(val),
    }
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;
    use crate::Document;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    /// Marker, annotation and description reach a leaf at whatever depth it is
    /// declared, and a `default:` still covers the subtree under it.
    #[test]
    fn a_container_expands_at_every_depth() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        tags: { type: array, items: { type: string } }
        address:
          type: object
          properties:
            city: { type: string, description: The city }
            zip: { type: string, default: "00000" }
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
          lead:
            type: object
            properties:
              email: { type: string }
"#)
        .blueprint();

        assert!(
            t.contains(concat!(
                "contact: # object\n",
                "  tags: !must_fill # array<string>\n",
                "  address: # object\n",
                "    # The city\n",
                "    city: !must_fill # string\n",
                "    zip: \"00000\" # string\n",
            )),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "refs: # array<object>\n",
                "  - org: !must_fill # string\n",
                "    lead: # object\n",
                "      email: !must_fill # string\n",
            )),
            "{t}"
        );

        // A blueprint is written to be parsed back, markers at depth included.
        let doc1 = Document::parse(&t).expect("blueprint must parse").document;
        let doc2 = Document::parse(&doc1.to_markdown())
            .expect("re-emit must parse")
            .document;
        assert_eq!(doc1, doc2, "a deep blueprint must round-trip");
    }

    /// A nested container's cells carry their own defaults, and a covered leaf
    /// asks for nothing.
    #[test]
    fn a_nested_containers_leaf_defaults_cover_their_own_cells() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        address:
          type: object
          properties:
            city: { type: string, default: Reston }
            zip: { type: string, default: "20190" }
"#)
        .blueprint();
        assert!(t.contains("  address: # object\n"), "{t}");
        assert!(!t.contains("address: !must_fill"), "{t}");
        assert!(t.contains("Reston"), "{t}");
        assert!(
            !t.contains("city: !must_fill"),
            "a covered leaf asks for nothing: {t}"
        );
    }

    #[test]
    fn must_fill_markdown_example_surfaces_as_eg_hint_not_inline_value() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, example: "Hello world" }
"#)
        .blueprint();
        assert!(t.contains("# e.g. Hello world\nbio: !must_fill # richtext<markdown>\n"));
    }

    /// A suggested value a human must still confirm: the example takes the cell
    /// no `default:` holds, and the derived obligation marks it there.
    #[test]
    fn an_example_takes_the_cell_it_suggests_under_the_marker() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: enum, values: [UNCLASSIFIED, CUI], example: UNCLASSIFIED }
"#)
        .blueprint();
        assert!(
            t.contains("classification: !must_fill UNCLASSIFIED # enum<UNCLASSIFIED | CUI>\n"),
            "{t}"
        );
        assert!(!t.contains("e.g."), "the example is the cell, not a hint: {t}");
    }

    #[test]
    fn a_blank_default_emits_an_unmarked_cell() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    note: { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("\nnote: \"\" # string\n"), "{t}");
        assert!(!t.contains("!must_fill"), "{t}");

        let doc = Document::parse(&t).expect("the blank cell parses").document;
        assert_eq!(doc, Document::parse(&doc.to_markdown()).expect("re-emit").document);
    }

    #[test]
    fn endorsed_field_with_example_does_not_use_example_as_value() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    status: { type: string, default: draft, example: final }
"#)
        .blueprint();
        assert!(t.contains("# e.g. final\nstatus: draft # string\n"));
    }

    #[test]
    fn must_fill_array_example_renders_as_block_sequence_with_context_quoting() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    recipient:
      type: array
      items: { type: string }
      example:
        - Mr. John Doe
        - 123 Main St
        - "Anytown, USA"
"#)
        .blueprint();
        assert!(t.contains(
            "recipient: !must_fill # array<string>\n  - Mr. John Doe\n  - 123 Main St\n  - Anytown, USA\n"
        ));
        assert!(!t.contains("# e.g."));
    }

    #[test]
    fn enum_endorsed_uses_enum_format_slot_and_no_eg() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    format: { type: enum, values: [standard, informal], default: standard }
"#)
        .blueprint();
        assert!(t.contains("format: standard # enum<standard | informal>\n"));
        assert!(!t.contains("e.g."));
    }

    #[test]
    fn enum_must_fill_renders_bare_marker() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    severity: { type: enum, values: [low, medium, high] }
"#)
        .blueprint();
        assert!(t.contains("severity: !must_fill # enum<low | medium | high>\n"));
    }

    #[test]
    fn every_field_carries_inline_type_and_cell_signal() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: date }
    published: { type: datetime }
    refs: { type: array, default: [], items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("title: !must_fill # string\n"));
        assert!(t.contains("size: 11 # number\n"));
        assert!(t.contains("flag: false # boolean\n"));
        assert!(t.contains("issued: !must_fill # date<YYYY-MM-DD>\n"));
        assert!(t.contains("published: !must_fill # datetime<YYYY-MM-DDThh:mm[:ss]>\n"));
        assert!(t.contains("refs: [] # array<string>\n"));
    }

    #[test]
    fn scalar_array_annotation_reflects_element_type() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    counts:   { type: array, items: { type: integer } }
    sections: { type: array, items: { type: richtext } }
    tags:     { type: array, items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("counts: !must_fill # array<integer>\n"), "{t}");
        assert!(
            t.contains("sections: !must_fill # array<richtext<markdown>>\n"),
            "{t}"
        );
        assert!(t.contains("tags: !must_fill # array<string>\n"), "{t}");
    }

    #[test]
    fn endorsed_empty_markdown_renders_empty_string() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, default: "" }
"#)
        .blueprint();
        assert!(t.contains("bio: \"\" # richtext<markdown>\n"));
        assert!(!t.contains("|-"));
        assert!(!t.contains("!must_fill"));
    }

    #[test]
    fn endorsed_markdown_default_inlines_quoted() {
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: richtext
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(t.contains("bio: \"## About me\\n\\nHello.\" # richtext<markdown>\n"));
        assert!(!t.contains("|-"));
    }

    #[test]
    fn root_header_carries_quill_reminder_and_no_role_comment() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(t.starts_with("~~~\n$quill: taro@0.1.0 # keep verbatim\n$kind: main\n# x\n"));
        assert!(t.contains("\nWrite main body here.\n"));
    }

    /// A description that collapses to nothing is no description: the main card
    /// falls through to the quill's own rather than emitting an empty comment.
    #[test]
    fn a_whitespace_only_main_description_emits_no_empty_comment() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: A taro order form. }
main:
  description: "   "
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(
            t.starts_with(
                "~~~\n$quill: taro@0.1.0 # keep verbatim\n$kind: main\n# A taro order form.\n"
            ),
            "{t}"
        );
        assert!(!t.contains("\n#\n") && !t.contains("\n# \n"), "{t}");
    }

    #[test]
    fn card_fence_carries_composable_annotation() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    description: A short note appended to the document.
    fields:
      author: { type: string }
"#)
        .blueprint();
        assert!(t.contains(
            "~~~\n$kind: note\n# composable (0..N)\n# sample card; delete if not needed\n# A short note appended to the document.\n"
        ));
    }

    #[test]
    fn body_disabled_card_omits_body_placeholder() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  skills:
    body: { enabled: false }
    fields:
      items: { type: array, items: { type: string } }
"#)
        .blueprint();
        let after = &t[t.find("$kind: skills").unwrap()..];
        assert!(!after.contains("skills body"));
    }

    #[test]
    fn main_body_example_appears_verbatim() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Dear Sir or Madam,\n\nI am writing to..."
  fields:
    to: { type: string }
"#)
        .blueprint();
        assert!(t.contains("\nDear Sir or Madam,\n\nI am writing to...\n"));
        assert!(!t.contains("Write main body here."));
    }

    #[test]
    fn card_body_placeholder_uses_card_name() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  indorsement:
    fields:
      from: { type: string }
"#)
        .blueprint();
        assert!(t.contains("\nWrite indorsement body here.\n"));
    }

    #[test]
    fn ui_groups_cluster_fields_without_emitting_banner() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  ui: { groups: [addressing, letterhead] }
  fields:
    memo_for: { type: array, items: { type: string }, ui: { group: addressing } }
    subject: { type: string, ui: { group: addressing } }
    letterhead_title: { type: string, default: HQ, ui: { group: letterhead } }
    notes: { type: string }
"#)
        .blueprint();
        let after_quill = &t[t.find("$quill:").unwrap()..];
        assert!(!after_quill.contains("===="));
        let notes = after_quill.find("notes:").unwrap();
        let memo_for = after_quill.find("memo_for:").unwrap();
        let letterhead = after_quill.find("letterhead_title:").unwrap();
        assert!(notes < memo_for);
        assert!(memo_for < letterhead);
    }

    #[test]
    fn typed_table_must_fill_emits_synthetic_row_with_leaf_markers() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    references:
      type: array
      description: Cited works.
      items:
        type: object
        properties:
          org: { type: string, description: Citing organization. }
          year: { type: integer, default: 0, description: Publication year. }
"#)
        .blueprint();
        assert!(t.contains(
            "# Cited works.\nreferences: # array<object>\n  -\n    # Citing organization.\n    org: !must_fill # string\n"
        ));
        assert!(t.contains("    # Publication year.\n    year: 0 # integer\n"));
    }

    #[test]
    fn typed_table_with_example_keeps_eg_line_and_synthetic_row() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      example:
        - { org: ACME, year: 2020 }
      items:
        type: object
        properties:
          org: { type: string }
          year: { type: integer, default: 0 }
"#)
        .blueprint();
        assert!(t.contains("# e.g. [{org: ACME, year: 2020}]\n"));
        assert!(t.contains("refs: # array<object>\n  - org: !must_fill # string\n"));
        assert!(t.contains("    year: 0 # integer\n"));
    }

    #[test]
    fn typed_table_endorsed_renders_default_rows() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default:
        - { org: ACME }
      items:
        type: object
        properties:
          org: { type: string }
"#)
        .blueprint();
        assert!(t.contains("refs: # array<object>\n  - org: ACME\n"));
        assert!(!t.contains("refs: # array<object>\n  -\n"));
    }

    #[test]
    fn typed_table_with_empty_default_renders_inline() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default: []
      items:
        type: object
        properties:
          org: { type: string }
"#)
        .blueprint();
        assert!(
            t.contains("refs: [] # array<object>\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
    }

    #[test]
    /// A wholly skippable dictionary is spelled as a type-empty `default:` on
    /// each property.
    fn typed_dict_with_type_empty_property_defaults_asks_for_nothing() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string, default: "" }
        zip:    { type: integer, default: 0 }
"#)
        .blueprint();
        assert!(
            t.contains("address: # object\n  street: \"\" # string\n  zip: 0 # integer\n"),
            "wrong rendering: {t}"
        );
        assert!(!t.contains("{}"), "no bare empty object expected: {t}");
        assert!(!t.contains("!must_fill"), "no markers expected: {t}");
    }

    #[test]
    fn typed_dict_must_fill_emits_per_property_annotations() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      description: Mailing address.
      properties:
        street: { type: string, description: Street line. }
        city:   { type: string }
        zip:    { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("# Mailing address.\naddress: # object\n"));
        assert!(t.contains("  # Street line.\n  street: !must_fill # string\n"));
        assert!(t.contains("  city: !must_fill # string\n"));
        assert!(t.contains("  zip: \"\" # string\n"));
    }

    #[test]
    fn typed_dict_endorsed_per_property_renders_block_mapping() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string, default: "5000 Forbes Ave" }
        city:   { type: string, default: Pittsburgh }
"#)
        .blueprint();
        assert!(t.contains("address: # object\n"));
        assert!(
            t.contains("  street: 5000 Forbes Ave # string\n")
                || t.contains("  street: \"5000 Forbes Ave\" # string\n")
        );
        assert!(t.contains("  city: Pittsburgh # string\n"));
        assert!(
            !t.contains("street: !must_fill"),
            "a defaulted leaf asks for nothing: {t}"
        );
    }

    #[test]
    fn typed_dict_property_example_keeps_its_own_eg_line() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string }
        city:   { type: string, default: "", example: Cupertino }
"#)
        .blueprint();
        assert!(t.contains("address: # object\n"));
        assert!(t.contains("# e.g. Cupertino\n"), "{t}");
        assert!(t.contains("  street: !must_fill # string\n"));
        assert!(t.contains("  city: \"\" # string\n"));
    }

    /// A richtext cell never inlines its example, so the `# e.g.` hint is the
    /// only place the example can land — at every depth a property is declared.
    #[test]
    fn a_richtext_example_surfaces_as_an_eg_hint_at_every_depth() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, example: Top hello }
    contact:
      type: object
      properties:
        bio: { type: richtext, example: Nested hello }
    rows:
      type: array
      items:
        type: object
        properties:
          bio: { type: richtext, example: Row hello }
"#)
        .blueprint();

        assert!(
            t.contains("# e.g. Top hello\nbio: !must_fill # richtext<markdown>\n"),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "contact: # object\n",
                "  # e.g. Nested hello\n",
                "  bio: !must_fill # richtext<markdown>\n",
            )),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "rows: # array<object>\n",
                "  -\n",
                "    # e.g. Row hello\n",
                "    bio: !must_fill # richtext<markdown>\n",
            )),
            "{t}"
        );

        let doc1 = Document::parse(&t).expect("blueprint must parse").document;
        let doc2 = Document::parse(&doc1.to_markdown())
            .expect("re-emit must parse")
            .document;
        assert_eq!(doc1, doc2, "the hinted blueprint must round-trip");
    }

    /// A typed dictionary is a namespace, not a cell: a literal on the
    /// container is refused at load, naming the properties that hold it.
    #[test]
    fn a_literal_on_a_typed_dictionary_is_a_load_error() {
        for (slot, code) in [
            ("default", "quill::default_on_namespace"),
            ("example", "quill::example_on_namespace"),
        ] {
            let yaml = format!(
                r#"
quill: {{ name: x, version: 1.0.0, backend: typst, description: x }}
main:
  fields:
    address:
      type: object
      {slot}: {{ street: "5000 Forbes Ave" }}
      properties:
        street: {{ type: string }}
        city:   {{ type: string }}
"#
            );
            let err = QuillConfig::from_yaml(&yaml).expect_err("must refuse");
            assert!(err.contains(code), "expected {code}; got {err}");
            assert!(
                err.contains("street") && err.contains("city"),
                "the hint names the properties that hold the {slot}: {err}"
            );
        }
    }

    const LETTER_QUILL: &str = r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A formal letter. }
main:
  fields:
    to:
      type: string
      description: Recipient name.
    subject:
      type: string
    date:
      type: datetime
    priority:
      type: enum
      values: [normal, urgent]
      default: normal
    attachments:
      type: array
      items: { type: string }
      default: []
      example:
        - report.pdf
card_kinds:
  enclosure:
    description: An enclosure attached to the letter.
    fields:
      label: { type: string }
      pages: { type: integer, default: 1 }
"#;

    #[test]
    fn typed_table_synthetic_row_blueprint_round_trips() {
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string, description: Citing organization. }
          year: { type: integer, default: 0, description: Publication year. }
"#)
        .blueprint();
        let doc1 = Document::parse(&bp).expect("blueprint must parse").document;
        let doc2 = Document::parse(&doc1.to_markdown())
            .expect("re-emit must parse")
            .document;
        assert_eq!(doc1, doc2, "typed-table blueprint must round-trip");
    }

    #[test]
    fn must_fill_markers_round_trip_and_survive_as_fill() {
        let bp = cfg(r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A letter. }
main:
  fields:
    recipient:
      type: array
      items: { type: string }
      example: [Mr. John Doe, "Anytown, USA"]
    subject: { type: string }
    date: { type: datetime }
"#)
        .blueprint();

        let doc1 = Document::parse(&bp).expect("blueprint must parse").document;
        let payload = doc1.main().payload();
        for key in ["recipient", "subject", "date"] {
            assert!(
                payload.is_fill(key),
                "`{key}` must carry the fill marker:\n{bp}"
            );
        }
        assert_eq!(
            payload
                .get("recipient")
                .and_then(|v| v.as_json().as_array().map(|a| a.len())),
            Some(2),
            "recipient suggested value should survive: {bp}"
        );

        let md2 = doc1.to_markdown();
        let doc2 = Document::parse(&md2).expect("re-emitted markdown must parse").document;
        assert_eq!(doc1, doc2, "blueprint must round-trip idempotently");
    }

    #[test]
    fn blueprint_round_trips_idempotently() {
        let bp = cfg(LETTER_QUILL).blueprint();
        let doc1 = Document::parse(&bp).expect("blueprint must parse").document;
        let md2 = doc1.to_markdown();
        let doc2 = Document::parse(&md2).expect("round-tripped markdown must parse").document;
        assert_eq!(
            doc1, doc2,
            "Document must be equal after blueprint → parse → emit → parse"
        );
    }

    #[test]
    fn type_ambiguous_string_defaults_round_trip_as_strings() {
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    version:     { type: string, default: "1.0" }
    activation:  { type: string, default: "on" }
    code:        { type: string, default: "01234" }
    placeholder: { type: string, default: "null" }
    yes_flag:    { type: string, default: "yes" }
"#)
        .blueprint();

        let doc = Document::parse(&bp).expect("blueprint must parse").document;
        let payload = doc.main().payload();
        for (key, expected) in [
            ("version", "1.0"),
            ("activation", "on"),
            ("code", "01234"),
            ("placeholder", "null"),
            ("yes_flag", "yes"),
        ] {
            let v = payload.get(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(
                v.as_str().is_some(),
                "field {key} must round-trip as a string, got {:?}\nBlueprint:\n{}",
                v,
                bp
            );
            assert_eq!(v.as_str().unwrap(), expected, "field {key}: value mismatch");
        }
    }
}
