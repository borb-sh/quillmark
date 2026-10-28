//! Auto-generated Markdown blueprint for a Quill: an annotated reference
//! document dense enough to replace the schema for LLM consumers.
//!
//! `blueprint()` builds a [`Document`] and emits it through
//! [`Document::to_markdown`], so the blueprint round-trips through
//! `Document::parse` by construction: there is no second formatter.

use indexmap::IndexMap;

use super::{
    CardSchema, FieldSchema, FieldType, QuillConfig, VariantFields, MATRIX_TITLE_KEY,
    VARIANT_DISCRIMINANT_KEY,
};
use crate::document::emit::{emit_mapping_lines, saphyr_emit_flow};
use crate::document::prescan::NestedComment;
use crate::document::{Card, Document, Payload, PayloadItem};
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;
use quillmark_content::model::Normalized;
use serde_json::{Map as JsonMap, Value as JsonValue};

impl QuillConfig {
    /// Generate the canonical annotated Markdown blueprint for this quill:
    /// the authoring surface handed to LLMs and humans. The annotation grammar
    /// is `prose/canon/BLUEPRINT.md` §Annotation grammar; the function is total
    /// over any valid `QuillConfig`.
    ///
    /// The result is guaranteed schema-valid and parseable (every key
    /// present, every value type-correct). It is *not* guaranteed to render:
    /// that is the quill authoring contract on `plate.typ`; see
    /// `prose/canon/BLUEPRINT.md` §Guarantees.
    ///
    /// [`Document`]: crate::document::Document
    pub fn blueprint(&self) -> String {
        let main = build_main_card(&self.main, super::seed::main_reference(self));
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

/// The comment naming a field or card: `<title> — <description>`, or
/// whichever of the two is declared.
fn label_line(title: Option<&str>, description: Option<&str>) -> Option<String> {
    match (collapse_opt(title), collapse_opt(description)) {
        (Some(title), Some(desc)) => Some(format!("{title} — {desc}")),
        (title, desc) => title.or(desc),
    }
}

/// The line closing the payload of a kind taking no body: `# no body`. A body
/// is left empty as a field's cell is.
fn push_body_line(items: &mut CardItems, card: &CardSchema) {
    if !card.body_enabled() {
        items.push(PayloadItem::comment("no body"));
    }
}

/// A card's payload under construction: entries in source order beside the
/// comments nested inside their values, which [`Payload`] keys by owner.
#[derive(Default)]
struct CardItems {
    items: Vec<PayloadItem>,
    nested: Vec<NestedComment>,
}

impl CardItems {
    fn push(&mut self, item: PayloadItem) {
        self.items.push(item);
    }

    /// Adopt the comments nested inside entry `key`, rebasing each onto the
    /// payload-absolute path `Payload` addresses them by.
    fn adopt_nested(&mut self, key: &str, nested: Vec<NestedComment>) {
        self.nested.extend(nested.into_iter().map(|nc| {
            let mut path = Vec::with_capacity(nc.container_path.len() + 1);
            path.push(PathSegment::Key(key.to_string()));
            path.extend(nc.container_path);
            NestedComment {
                container_path: path,
                ..nc
            }
        }));
    }

    fn into_payload(self) -> Payload {
        Payload::from_items_with_nested(self.items, self.nested)
    }
}

/// Build the root card: `$quill` (with the `# keep verbatim` inline reminder),
/// `$kind: main` carrying the optional [`label_line`] inline, the fields, then
/// the [`push_body_line`]. Inline, the label cannot read as the first field's.
fn build_main_card(card: &CardSchema, reference: QuillReference) -> Card {
    let mut items = CardItems::default();
    items.push(PayloadItem::Quill { reference });
    items.push(PayloadItem::comment_inline("keep verbatim"));
    items.push(PayloadItem::Kind {
        value: "main".into(),
    });
    if let Some(label) = label_line(card.title.as_deref(), card.description.as_deref()) {
        items.push(PayloadItem::comment_inline(label));
    }
    append_fields(&mut items, card);
    push_body_line(&mut items, card);
    Card::from_parts(items.into_payload(), Normalized::empty())
}

/// Build a composable card: `$kind: <kind>` carrying the optional
/// [`label_line`] inline, the `composable (0..N)` role comment, a comment
/// naming it a deletable sample, the fields, then the [`push_body_line`].
fn build_card(card: &CardSchema) -> Card {
    let mut items = CardItems::default();
    items.push(PayloadItem::Kind {
        value: card.name.clone(),
    });
    if let Some(label) = label_line(card.title.as_deref(), card.description.as_deref()) {
        items.push(PayloadItem::comment_inline(label));
    }
    items.push(PayloadItem::comment("composable (0..N)"));
    items.push(PayloadItem::comment("sample card; delete if not needed"));
    append_fields(&mut items, card);
    push_body_line(&mut items, card);
    Card::from_parts(items.into_payload(), Normalized::empty())
}

/// Append every field of a card as payload items, in declaration order: the
/// loader settles groups into it (`config::settle_groups`).
fn append_fields(items: &mut CardItems, card: &CardSchema) {
    for field in card.fields.values() {
        append_field(items, field);
    }
}

/// Append one top-level field. Dispatches typed tables (`array<object>`) and
/// typed dictionaries (`object` with `properties`) to their per-property
/// builders; everything else is a scalar/array cell.
fn append_field(items: &mut CardItems, field: &FieldSchema) {
    if field.is_variant_bearing() {
        append_variant(items, field);
        return;
    }

    if matches!(field.r#type, FieldType::Matrix { .. }) {
        push_leading(items, field);
        push_container_field(items, &field.name, matrix_cell(field), Vec::new(), field);
        return;
    }

    if typed_dict_props(field).is_some() || typed_table_props(field).is_some() {
        push_leading(items, field);
        for line in dormant_table(field) {
            items.push(PayloadItem::comment(line));
        }
        let (value, nested) = container_cell(field, &[]);
        push_container_field(items, &field.name, value, nested, field);
        return;
    }

    append_scalar(items, field);
}

/// The `default:` a cell prints. A blank one (`""`, `[]`, `{}`) renders exactly
/// as no default does, so it prints as none; `0` and `false` are values.
fn shown_default(field: &FieldSchema) -> Option<&JsonValue> {
    let default = field.default.as_ref()?.as_json();
    let blank = match default {
        JsonValue::String(s) => s.is_empty(),
        JsonValue::Array(rows) => rows.is_empty(),
        JsonValue::Object(map) => map.is_empty(),
        _ => false,
    };
    (!blank).then_some(default)
}

/// A matrix's blueprint cell: its `default:`, shippable as-is as an array's is,
/// else the empty mapping, the sparse spelling of a vocabulary nobody has
/// ticked. The roster rides the inline annotation, so expanding every member
/// here would show a model twenty-seven subforms to delete and filler it must
/// not ship.
fn matrix_cell(field: &FieldSchema) -> JsonValue {
    shown_default(field)
        .cloned()
        .unwrap_or_else(|| JsonValue::Object(JsonMap::new()))
}

/// The placeholder id an open matrix's `# e.g.` line keys its added item by,
/// suffixed where the roster declares it.
const ADDED_ITEM_EG: &str = "new_item";

/// The `# e.g.` text for a matrix declaring columns or open: its first member
/// held, each column at [`column_hint`], then on an open matrix an added item
/// keyed [`ADDED_ITEM_EG`] with its `title` beside the columns. A closed
/// checklist has no line, the bare tick being its whole spelling.
fn matrix_eg(field: &FieldSchema) -> Option<String> {
    let columns = field.matrix_columns();
    let open = field.r#type.is_open_matrix();
    if columns.is_empty() && !open {
        return None;
    }
    let first = field.r#type.matrix_roster().keys().next()?;
    let cells = |title: Option<&FieldSchema>| -> Vec<String> {
        title
            .map(|t| (MATRIX_TITLE_KEY, t))
            .into_iter()
            .chain(columns.iter().map(|(name, col)| (name.as_str(), col.as_ref())))
            .map(|(name, col)| format!("{}: {}", flow_scalar(name), column_hint(col)))
            .collect()
    };
    let member = cells(None);
    let held = if member.is_empty() {
        "true".to_string()
    } else {
        format!("{{{}}}", member.join(", "))
    };
    let mut members = vec![format!("{}: {held}", flow_scalar(first))];
    let title = field
        .added_item
        .as_deref()
        .and_then(|item| item.properties.as_ref()?.get(MATRIX_TITLE_KEY));
    if let Some(title) = title {
        let roster = field.r#type.matrix_roster();
        let id = std::iter::once(ADDED_ITEM_EG.to_string())
            .chain((2..).map(|n| format!("{ADDED_ITEM_EG}_{n}")))
            .find(|id| !roster.contains_key(id))
            .expect("a finite roster leaves an id free");
        members.push(format!("{id}: {{{}}}", cells(Some(title)).join(", ")));
    }
    Some(format!("{{{}}}", members.join(", ")))
}

/// One column's value in a matrix hint: `default:` › its container shape ›
/// its inline annotation's `<type>[<format>]`, quoted where flow syntax would
/// split it.
fn column_hint(col: &FieldSchema) -> String {
    if let Some(value) = shown_default(col) {
        return saphyr_emit_flow(value);
    }
    if matches!(col.r#type, FieldType::Matrix { .. }) {
        return "{}".into();
    }
    if let Some(props) = typed_dict_props(col) {
        return flow_hint_mapping(props);
    }
    if let Some(row) = typed_table_props(col) {
        if rowless(col, row) {
            return "[]".into();
        }
        return format!("[{}]", flow_hint_mapping(row));
    }
    flow_scalar(&type_expression(col))
}

fn flow_hint_mapping(props: &IndexMap<String, Box<FieldSchema>>) -> String {
    let cells: Vec<String> = props
        .iter()
        .map(|(name, prop)| format!("{}: {}", flow_scalar(name), column_hint(prop)))
        .collect();
    format!("{{{}}}", cells.join(", "))
}

fn flow_scalar(key: &str) -> String {
    saphyr_emit_flow(&JsonValue::String(key.to_string()))
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

/// Push the leading prose comments for a *top-level* field: the
/// [`label_line`], the cap, then a matrix's `# e.g.` hint.
fn push_leading(items: &mut CardItems, field: &FieldSchema) {
    if let Some(label) = label_line(field.title.as_deref(), field.description.as_deref()) {
        items.push(PayloadItem::comment(label));
    }
    if let Some(cap) = cap_hint(field) {
        items.push(PayloadItem::comment(cap));
    }
    if let Some(eg) = matrix_eg(field) {
        items.push(PayloadItem::comment(format!("e.g. {eg}")));
    }
}

/// The `# up to <N>` leading line for a capped array, `# up to <N> characters`
/// for a capped text field, in the own-line form `# composable (0..N)` already
/// takes for a card kind's cardinality. An array's elements have no leading
/// line of their own, so their cap rides the array's (`each up to <N>
/// characters`). A limit the blueprint does not show is a limit the MCP flow
/// learns from prose, which is what `max:` exists to stop.
fn cap_hint(field: &FieldSchema) -> Option<String> {
    let own = field.max.map(|max| match field.r#type {
        FieldType::Text { .. } | FieldType::RichText { .. } => format!("up to {max} characters"),
        _ => format!("up to {max}"),
    });
    let each = field
        .items
        .as_deref()
        .filter(|items| items.r#type.codec().is_some())
        .and_then(|items| items.max)
        .map(|max| format!("each up to {max} characters"));
    match (own, each) {
        (Some(own), Some(each)) => Some(format!("{own}, {each}")),
        (own, each) => own.or(each),
    }
}

/// A leaf's cell: its `default:`, else empty.
fn scalar_value(field: &FieldSchema) -> JsonValue {
    shown_default(field).cloned().unwrap_or(JsonValue::Null)
}

/// Append a scalar / scalar-array / richtext field as a single payload field
/// plus its trailing inline type annotation.
fn append_scalar(items: &mut CardItems, field: &FieldSchema) {
    push_leading(items, field);
    items.push(PayloadItem::Field {
        key: field.name.clone(),
        value: QuillValue::from_json(scalar_value(field)),
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the per-property body of a defaultless typed container into `map`:
/// each property at its own cell in declaration order, plus the nested comments
/// ([`label_line`] + a matrix's `# e.g.` + [`dormant_table`] + inline type annotation,
/// addressed by `container_path`/slot). `prefix` is the container path of the
/// mapping relative to the value it is rendered in (`[]` for a typed dict,
/// `[Key(<field>), Index(0)]` for a typed table's dormant row).
///
/// A property's slot is where it lands in `map`, so a caller seating its own
/// cell first (a variant's discriminant) shifts the rest by handing over a
/// mapping that already holds it.
fn build_property_mapping(
    map: &mut JsonMap<String, JsonValue>,
    props: &IndexMap<String, Box<FieldSchema>>,
    prefix: &[PathSegment],
) -> Vec<NestedComment> {
    let mut nested = Vec::new();
    for prop in props.values().map(|b| b.as_ref()) {
        let slot = map.len();
        if let Some(label) = label_line(prop.title.as_deref(), prop.description.as_deref()) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: label,
                inline: false,
            });
        }
        if let Some(cap) = cap_hint(prop) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: cap,
                inline: false,
            });
        }
        if let Some(eg) = matrix_eg(prop) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: format!("e.g. {eg}"),
                inline: false,
            });
        }
        nested.extend(
            dormant_table(prop)
                .into_iter()
                .map(|line| world_comment(prefix, slot, line)),
        );
        let mut path = prefix.to_vec();
        path.push(PathSegment::Key(prop.name.clone()));
        let (json, sub_nested) = property_cell(prop, &path);
        map.insert(prop.name.clone(), json);
        nested.extend(sub_nested);
        nested.push(NestedComment {
            container_path: prefix.to_vec(),
            position: slot,
            text: type_expression(prop),
            inline: true,
        });
    }
    nested
}

/// One property's contribution to its parent's mapping: its value, and the
/// nested comments its own subtree carries. `path` is the property's address
/// relative to the field value.
fn property_cell(prop: &FieldSchema, path: &[PathSegment]) -> (JsonValue, Vec<NestedComment>) {
    if prop.is_variant_bearing() {
        return variant_cell(prop, path);
    }
    if matches!(prop.r#type, FieldType::Matrix { .. }) {
        return (matrix_cell(prop), Vec::new());
    }
    if typed_dict_props(prop).is_some() || typed_table_props(prop).is_some() {
        return container_cell(prop, path);
    }
    (scalar_value(prop), Vec::new())
}

/// A container's value plus the nested comments its subtree carries, at `path`
/// relative to the field value (`[]` at card level).
///
/// A **typed table** is a cell: its `default:` renders verbatim, else `[]`, or
/// empty where optional, since `[]` answers it; either is led by its
/// [`dormant_table`]. A live row would ship an empty element. A **typed
/// dictionary** holds no literal (`quill::default_on_namespace`), so it always
/// expands per property.
fn container_cell(field: &FieldSchema, path: &[PathSegment]) -> (JsonValue, Vec<NestedComment>) {
    if let Some(props) = typed_dict_props(field) {
        let mut map = JsonMap::new();
        let nested = build_property_mapping(&mut map, props, path);
        return (JsonValue::Object(map), nested);
    }
    let rows = match shown_default(field) {
        Some(rows) => rows.clone(),
        None if field.optional => JsonValue::Null,
        None => JsonValue::Array(Vec::new()),
    };
    (rows, Vec::new())
}

/// A row type declaring no properties is schema-invalid in practice, and a
/// `max: 0` table holds no row at all: neither has a row to show.
fn rowless(field: &FieldSchema, row_props: &IndexMap<String, Box<FieldSchema>>) -> bool {
    row_props.is_empty() || field.max == Some(0)
}

/// The lines of an empty table's field holding a synthetic row, to lead the
/// live `[]` commented out: the config-file spelling of an alternative, taken
/// by deleting the live line and uncommenting these.
fn dormant_table(field: &FieldSchema) -> Vec<String> {
    let Some(row_props) = typed_table_props(field) else {
        return Vec::new();
    };
    if shown_default(field).is_some() || rowless(field, row_props) {
        return Vec::new();
    }
    let row_path = [PathSegment::Key(field.name.clone()), PathSegment::Index(0)];
    let mut row = JsonMap::new();
    let nested = build_property_mapping(&mut row, row_props, &row_path);
    let mut map = JsonMap::new();
    map.insert(field.name.clone(), JsonValue::Array(vec![JsonValue::Object(row)]));
    emit_mapping_lines(&map, &nested).lines().map(str::to_owned).collect()
}

/// Append a variant-bearing enum: the container, its discriminant cell, then
/// every world under a `# when <MEMBER>:` header — the selected world's cells
/// live, every other world's commented out (`prose/canon/BLUEPRINT.md`
/// § "Enum variants").
fn append_variant(items: &mut CardItems, field: &FieldSchema) {
    push_leading(items, field);
    let (json, nested) = variant_cell(field, &[]);
    push_container_field(items, &field.name, json, nested, field);
}

/// The variant container at `path`: the discriminant cell, then every world
/// under a `# when <MEMBER>:` header. The worlds seat themselves in whichever
/// container holds the discriminant, so a typed dictionary's property reaches
/// this through [`property_cell`] carrying its own path.
fn variant_cell(field: &FieldSchema, path: &[PathSegment]) -> (JsonValue, Vec<NestedComment>) {
    let member = scalar_value(field);
    let mut map = JsonMap::new();
    map.insert(
        VARIANT_DISCRIMINANT_KEY.to_string(),
        match &member {
            // A null discriminant would read as "no cell"; the blank is the
            // enum's own spelling of an unanswered choice, and it round-trips.
            JsonValue::Null => JsonValue::String(String::new()),
            other => other.clone(),
        },
    );

    let selected = member.as_str();
    let mut nested = Vec::new();
    for (name, world) in field.variants.iter().flatten() {
        // A cell is a field of the container, so it expands as one. A world's
        // slot is where the mapping has reached, which seats a dormant block
        // where its live form would sit.
        let slot = map.len();
        nested.push(world_comment(path, slot, format!("when {name}:")));
        if Some(name.as_str()) == selected {
            nested.extend(build_property_mapping(&mut map, world, path));
        } else {
            nested.extend(dormant_world(world, path, slot));
        }
    }

    (JsonValue::Object(map), nested)
}

/// A dormant world's cells as own-line comments. `to_markdown` writes the `# `
/// itself, so there is no prefixing step here and what a reader uncomments is
/// byte-for-byte the line the live world would show.
fn dormant_world(world: &VariantFields, path: &[PathSegment], slot: usize) -> Vec<NestedComment> {
    let mut map = JsonMap::new();
    // Rendered standalone, so the block's own lines are cut against its own
    // root; `path` seats the finished lines in the container holding them.
    let nested = build_property_mapping(&mut map, world, &[]);
    emit_mapping_lines(&map, &nested)
        .lines()
        .map(|line| world_comment(path, slot, line))
        .collect()
}

fn world_comment(path: &[PathSegment], slot: usize, text: impl Into<String>) -> NestedComment {
    NestedComment {
        container_path: path.to_vec(),
        position: slot,
        text: text.into(),
        inline: false,
    }
}

/// Push a typed-container field (value + nested comments) and its trailing
/// inline type annotation.
fn push_container_field(
    items: &mut CardItems,
    key: &str,
    value: JsonValue,
    nested_comments: Vec<NestedComment>,
    field: &FieldSchema,
) {
    items.adopt_nested(key, nested_comments);
    items.push(PayloadItem::Field {
        key: key.to_string(),
        value: QuillValue::from_json(value),
    });
    items.push(PayloadItem::comment_inline(type_expression(field)));
}

/// Build the inline annotation body (without the leading `# `): purely the
/// structural type expression `<type>[<format>]`. The value cell carries the
/// cell's state (a concrete value is shippable as-is, an empty cell awaits
/// one), so the annotation needs no cell-state tag.
fn type_expression(field: &FieldSchema) -> String {
    let expression = declared_type_expression(field);
    if field.optional {
        format!("{expression}?")
    } else {
        expression
    }
}

fn declared_type_expression(field: &FieldSchema) -> String {
    match &field.r#type {
        FieldType::Enum { values } => format!("enum<{}>", values.join(" | ")),
        // The roster rides the format slot as an enum's domain does: the whole
        // vocabulary in one line, so a model cannot invent a member.
        FieldType::Matrix { .. } => {
            let ids: Vec<&str> = field
                .r#type
                .matrix_roster()
                .keys()
                .map(String::as_str)
                .collect();
            format!("matrix<{}>", ids.join(" | "))
        }
        FieldType::Number => "number".into(),
        FieldType::Integer => "integer".into(),
        FieldType::Boolean => "boolean".into(),
        FieldType::Object => "object".into(),
        // The type names the role; the `<markdown>` format slot names the
        // surface encoding an author writes (and `to_markdown` re-emits).
        FieldType::RichText { multiline: false } => "richtext<markdown>".into(),
        FieldType::RichText { multiline: true } => "richtext(multiline)<markdown>".into(),
        // The `<plain>` format slot names the literal codec (`from_plaintext`/
        // `to_plaintext`): content the author navigates but which takes no
        // markup, distinct from richtext's `<markdown>` surface.
        FieldType::Text { multiline: false } => "text<plain>".into(),
        FieldType::Text { multiline: true } => "text(multiline)<plain>".into(),
        FieldType::Date => "date<YYYY-MM-DD | today>".into(),
        FieldType::DateTime => "datetime<YYYY-MM-DDThh:mm[:ss]>".into(),
        // The element type comes from `items`; a scalar element gives
        // `array<text<plain>>`/`array<integer>`, an object element gives
        // `array<object>`.
        FieldType::Array => {
            let item = field
                .items
                .as_ref()
                .map(|it| type_expression(it))
                .unwrap_or_else(|| "text<plain>".into());
            format!("array<{}>", item)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;
    use crate::document::Document;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid yaml")
    }

    /// Annotation and description reach a leaf at whatever depth it is declared,
    /// and a `default:` still covers the subtree under it.
    #[test]
    fn a_container_expands_at_every_depth() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        tags: { type: array, items: { type: text } }
        address:
          type: object
          properties:
            city: { type: text, description: The city }
            zip: { type: text, default: "00000" }
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: text }
          lead:
            type: object
            properties:
              email: { type: text }
"#)
        .blueprint();

        assert!(
            t.contains(concat!(
                "contact: # object\n",
                "  tags: # array<text<plain>>\n",
                "  address: # object\n",
                "    # The city\n",
                "    city: # text<plain>\n",
                "    zip: \"00000\" # text<plain>\n",
            )),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "# refs:\n",
                "#   - org: # text<plain>\n",
                "#     lead: # object\n",
                "#       email: # text<plain>\n",
                "refs: [] # array<object>\n",
            )),
            "{t}"
        );

        // A blueprint is written to be parsed back, cells at depth included.
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
            city: { type: text, default: Reston }
            zip: { type: text, default: "20190" }
"#)
        .blueprint();
        assert!(t.contains("  address: # object\n"), "{t}");
        assert!(t.contains("    city: Reston # text<plain>\n"), "{t}");
    }

    /// A blank `default:` renders as no default does, so it prints as none, at
    /// every depth and in a matrix's `# e.g.` line; `0` and `false` are values.
    #[test]
    fn a_blank_default_prints_as_no_default() {
        let quill = |blank: &str| {
            let d = |value: &str| match blank {
                "" => String::new(),
                _ => format!(", default: {value}"),
            };
            format!(
                r#"
quill: {{ name: x, version: 1.0.0, backend: typst, description: x }}
main:
  fields:
    note: {{ type: text{s} }}
    issued: {{ type: date{s} }}
    stamp: {{ type: datetime{s} }}
    level: {{ type: enum, values: [low, high]{s} }}
    bio: {{ type: richtext, multiline: true{s} }}
    line: {{ type: text{s} }}
    tags: {{ type: array, items: {{ type: text }}{a} }}
    quals:
      type: matrix
      open: true
      members: {{ cc: CC }}
      properties: {{ detail: {{ type: text{s} }} }}{m}
    rows: {{ type: array, items: {{ type: object, properties: {{ org: {{ type: text{s} }} }} }}{a} }}
    address:
      type: object
      properties:
        street: {{ type: text{s} }}
        refs: {{ type: array, items: {{ type: object, properties: {{ v: {{ type: text }} }} }}{a} }}
"#,
                s = d("\"\""),
                a = d("[]"),
                m = match blank {
                    "" => String::new(),
                    _ => "\n      default: {}".into(),
                },
            )
        };
        let blank = cfg(&quill("blank")).blueprint();
        assert_eq!(blank, cfg(&quill("")).blueprint());
        assert!(!blank.contains("\"\""), "{blank}");

        let valued = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    count: { type: integer, default: 0 }
    active: { type: boolean, default: false }
"#)
        .blueprint();
        assert!(valued.contains("count: 0 # integer\nactive: false # boolean\n"), "{valued}");
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
    fn a_defaultless_enum_renders_an_empty_cell() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    severity: { type: enum, values: [low, medium, high] }
"#)
        .blueprint();
        assert!(t.contains("severity: # enum<low | medium | high>\n"));
    }

    #[test]
    fn every_field_carries_inline_type_and_cell_signal() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: text }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: date }
    published: { type: datetime }
"#)
        .blueprint();
        assert!(t.contains("title: # text<plain>\n"));
        assert!(t.contains("size: 11 # number\n"));
        assert!(t.contains("flag: false # boolean\n"));
        assert!(t.contains("issued: # date<YYYY-MM-DD | today>\n"));
        assert!(t.contains("published: # datetime<YYYY-MM-DDThh:mm[:ss]>\n"));
    }

    #[test]
    fn scalar_array_annotation_reflects_element_type() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    counts:   { type: array, items: { type: integer } }
    sections: { type: array, items: { type: richtext, multiline: true } }
    tags:     { type: array, items: { type: text } }
"#)
        .blueprint();
        assert!(t.contains("counts: # array<integer>\n"), "{t}");
        assert!(
            t.contains("sections: # array<richtext(multiline)<markdown>>\n"),
            "{t}"
        );
        assert!(t.contains("tags: # array<text<plain>>\n"), "{t}");
    }

    #[test]
    fn endorsed_markdown_default_is_a_literal_block() {
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: richtext
      multiline: true
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(
            t.contains("bio: |- # richtext(multiline)<markdown>\n  ## About me\n\n  Hello.\n"),
            "{t}"
        );
        let doc = Document::parse(&t).expect("the blueprint parses").document;
        assert_eq!(
            doc.main().payload().get("bio").and_then(|v| v.as_str()),
            Some("## About me\n\nHello.")
        );
    }

    #[test]
    fn root_header_carries_quill_reminder_and_no_role_comment() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: A taro order form. }
main:
  fields:
    flavor: { type: text, default: taro }
"#)
        .blueprint();
        assert!(
            t.starts_with(
                "~~~\n$quill: taro@0.1 # keep verbatim\n$kind: main\nflavor: taro # text<plain>\n"
            ),
            "{t}"
        );
        assert!(!t.contains("A taro order form."), "{t}");
    }

    #[test]
    fn a_whitespace_only_main_description_emits_no_empty_comment() {
        let t = cfg(r#"
quill: { name: taro, version: 0.1.0, backend: typst, description: A taro order form. }
main:
  description: "   "
  fields:
    flavor: { type: text, default: taro }
"#)
        .blueprint();
        assert!(
            t.starts_with("~~~\n$quill: taro@0.1 # keep verbatim\n$kind: main\nflavor:"),
            "{t}"
        );
    }

    #[test]
    fn card_fence_carries_composable_annotation() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: text }
card_kinds:
  note:
    description: A short note appended to the document.
    fields:
      author: { type: text }
"#)
        .blueprint();
        assert!(t.contains(
            "~~~\n$kind: note # A short note appended to the document.\n# composable (0..N)\n# sample card; delete if not needed\nauthor: # text<plain>\n"
        ));
    }

    #[test]
    fn a_title_leads_the_comment_naming_its_field_or_card() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  title: Memorandum
  description: A formal memo.
  fields:
    contact: { type: text, title: Point of contact, description: Who answers questions. }
    office: { type: text, title: Office symbol }
    address:
      type: object
      properties:
        city: { type: text, title: City }
card_kinds:
  note:
    title: Marginal note
    description: A short note.
    fields:
      text: { type: text }
"#)
        .blueprint();
        for line in [
            "$kind: main # Memorandum — A formal memo.\n",
            "# Point of contact — Who answers questions.\ncontact: # text<plain>\n",
            "# Office symbol\noffice: # text<plain>\n",
            "  # City\n  city: # text<plain>\n",
            "$kind: note # Marginal note — A short note.\n",
        ] {
            assert!(t.contains(line), "missing {line:?}:\n{t}");
        }
    }

    /// A body is a cell: it stays empty, and a kind taking none closes its
    /// payload with `# no body`.
    #[test]
    fn a_body_line_closes_the_payload_over_an_empty_body() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    to: { type: text }
card_kinds:
  note:
    fields:
      author: { type: text }
  skills:
    body: { enabled: false }
    fields:
      items: { type: array, items: { type: text } }
"#)
        .blueprint();
        assert!(t.contains("to: # text<plain>\n~~~\n\n~~~\n"), "{t}");
        assert!(t.contains("author: # text<plain>\n~~~\n\n~~~\n"), "{t}");
        assert!(t.ends_with("items: # array<text<plain>>\n# no body\n~~~\n"), "{t}");
        let doc = Document::parse(&t).expect("blueprint must parse").document;
        assert!(doc.main().body().is_blank());
        assert!(doc.cards().iter().all(|c| c.body().is_blank()));
    }

    #[test]
    fn groups_print_in_display_order_without_a_banner() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  ui:
    groups:
      addressing: { title: Addressing }
      letterhead: {}
  fields:
    letterhead_title: { type: text, default: HQ, ui: { group: letterhead } }
    memo_for: { type: array, items: { type: text }, ui: { group: addressing } }
    notes: { type: text }
    subject: { type: text, ui: { group: addressing } }
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
          org: { type: text }
"#)
        .blueprint();
        assert!(t.contains("refs: # array<object>\n  - org: ACME\n"));
        assert!(!t.contains("refs: # array<object>\n  -\n"));
    }

    /// Takes a dormant table: deletes the live `key: []` line and uncomments
    /// the field's block above it, from its `# key:` line down.
    fn swap_in(text: &str, live: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let at = lines.iter().position(|l| *l == live).expect("live line");
        let indent = &live[..live.len() - live.trim_start().len()];
        let key = live.trim_start().split(':').next().unwrap();
        let prefix = format!("{indent}# ");
        let head = format!("{prefix}{key}:");
        let start = (0..at).rev().find(|&i| lines[i] == head).expect("dormant head");
        let mut out: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i == at {
                continue;
            }
            if (start..at).contains(&i) {
                out.push(format!("{indent}{}", &line[prefix.len()..]));
            } else {
                out.push(line.to_string());
            }
        }
        out.join("\n") + "\n"
    }

    /// The empty cell stays shippable, and the field holding the row it hides
    /// leads it commented out: deleting the live line and uncommenting the
    /// block adds the row, at every depth and as a row's first cell.
    #[test]
    fn a_dormant_table_swaps_in_for_its_live_line() {
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    attendees:
      type: array
      description: Who attended.
      default: []
      items:
        type: object
        properties:
          name: { type: text, description: Full name. }
          voting: { type: boolean, default: false }
          tags: { type: array, default: [], items: { type: object, properties: { label: { type: text } } } }
    outer:
      type: array
      items:
        type: object
        properties:
          inner: { type: array, default: [], items: { type: object, properties: { v: { type: text } } } }
          z: { type: text }
"#)
        .blueprint();
        assert!(
            bp.contains(concat!(
                "# Who attended.\n",
                "# attendees:\n",
                "#   -\n",
                "#     # Full name.\n",
                "#     name: # text<plain>\n",
                "#     voting: false # boolean\n",
                "#     # tags:\n",
                "#     #   - label: # text<plain>\n",
                "#     tags: [] # array<object>\n",
                "attendees: [] # array<object>\n",
                "# outer:\n",
                "#   -\n",
                "#     # inner:\n",
                "#     #   - v: # text<plain>\n",
                "#     inner: [] # array<object>\n",
                "#     z: # text<plain>\n",
                "outer: [] # array<object>\n",
            )),
            "{bp}"
        );
        let doc = Document::parse(&bp).expect("blueprint must parse").document;
        assert_eq!(doc.to_markdown(), bp);

        let swapped = swap_in(&bp, "attendees: [] # array<object>");
        let swapped = swap_in(&swapped, "    tags: [] # array<object>");
        let swapped = swap_in(&swapped, "outer: [] # array<object>");
        let swapped = swap_in(&swapped, "    inner: [] # array<object>");
        let doc = Document::parse(&swapped).expect("swapped tables must parse").document;
        let payload = doc.main().payload();
        let attendees = payload.get("attendees").expect("attendees").as_json();
        assert!(attendees[0]["tags"][0].get("label").is_some(), "{swapped}");
        let outer = payload.get("outer").expect("outer").as_json();
        assert!(outer[0]["inner"][0].get("v").is_some(), "{swapped}");
        assert!(outer[0].get("z").is_some(), "{swapped}");
    }

    #[test]
    fn a_capped_out_table_has_no_dormant_row() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      max: 0
      items:
        type: object
        properties:
          org: { type: text }
"#)
        .blueprint();
        assert!(t.contains("refs: [] # array<object>\n~~~"), "{t}");
    }

    #[test]
    fn a_typed_dict_emits_per_property_annotations() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      description: Mailing address.
      properties:
        street: { type: text, description: Street line. }
        city:   { type: text }
"#)
        .blueprint();
        assert!(t.contains("# Mailing address.\naddress: # object\n"));
        assert!(t.contains("  # Street line.\n  street: # text<plain>\n"));
        assert!(t.contains("  city: # text<plain>\n"));
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
        street: { type: text, default: "5000 Forbes Ave" }
        city:   { type: text, default: Pittsburgh }
"#)
        .blueprint();
        assert!(t.contains("address: # object\n"));
        assert!(
            t.contains("  street: 5000 Forbes Ave # text<plain>\n")
                || t.contains("  street: \"5000 Forbes Ave\" # text<plain>\n")
        );
        assert!(t.contains("  city: Pittsburgh # text<plain>\n"));
    }

    /// A typed dictionary is a namespace, not a cell: a literal on the
    /// container is refused at load, naming the properties that hold it.
    #[test]
    fn a_literal_on_a_typed_dictionary_is_a_load_error() {
        let err = QuillConfig::from_yaml(
            r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    address:
      type: object
      default: { street: "5000 Forbes Ave" }
      properties:
        street: { type: text }
        city:   { type: text }
"#,
        )
        .expect_err("must refuse");
        assert!(err.contains("quill::default_on_namespace"), "{err}");
        assert!(
            err.contains("street") && err.contains("city"),
            "the hint names the properties that hold the default: {err}"
        );
    }

    const LETTER_QUILL: &str = r#"
quill: { name: letter, version: 1.0.0, backend: typst, description: A formal letter. }
main:
  fields:
    to:
      type: text
      description: Recipient name.
    subject:
      type: text
    date:
      type: datetime
    priority:
      type: enum
      values: [normal, urgent]
      default: normal
    attachments:
      type: array
      items: { type: text }
      default: []
card_kinds:
  enclosure:
    description: An enclosure attached to the letter.
    fields:
      label: { type: text }
      pages: { type: integer, default: 1 }
"#;

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
    version:     { type: text, default: "1.0" }
    activation:  { type: text, default: "on" }
    code:        { type: text, default: "01234" }
    placeholder: { type: text, default: "null" }
    yes_flag:    { type: text, default: "yes" }
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
