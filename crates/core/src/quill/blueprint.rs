//! Auto-generated Markdown blueprint for a Quill: an annotated reference
//! document dense enough to replace the schema for LLM consumers.
//!
//! `blueprint()` builds a [`Document`] and emits it through
//! [`Document::to_markdown`], so the blueprint round-trips through
//! `Document::parse` by construction: there is no second formatter.

use indexmap::IndexMap;

use super::{
    CardSchema, FieldSchema, FieldType, QuillConfig, VariantFields, MATRIX_HELD_KEY,
    VARIANT_DISCRIMINANT_KEY,
};
use crate::document::emit::{emit_mapping_lines, saphyr_emit_flow, saphyr_emit_scalar};
use crate::document::prescan::NestedComment;
use crate::document::{Card, Document, Payload, PayloadItem};
use crate::value::{PathSegment, QuillValue};
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
        let main_desc = collapse_opt(self.main.description.as_deref())
            .or_else(|| collapse_opt(Some(self.description.as_str())));

        let main = build_main_card(
            &self.main,
            &format!("{}@{}", self.name, self.version),
            label_line(self.main.title.as_deref(), main_desc.as_deref()),
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

/// The comment naming a field or card: `<title> — <description>`, or
/// whichever of the two is declared.
fn label_line(title: Option<&str>, description: Option<&str>) -> Option<String> {
    match (collapse_opt(title), collapse_opt(description)) {
        (Some(title), Some(desc)) => Some(format!("{title} — {desc}")),
        (title, desc) => title.or(desc),
    }
}

/// A body's `body.example`, as the `# body e.g.` line closing the payload
/// above it. The body itself is left empty, as a field's cell is.
fn push_body_example(items: &mut CardItems, card: &CardSchema) {
    if !card.body_enabled() {
        return;
    }
    if let Some(example) = card.body.as_ref().and_then(|b| b.example.as_deref()) {
        let example = JsonValue::String(example.trim_end().to_string());
        items.push(PayloadItem::comment(format!("body e.g. {}", saphyr_emit_scalar(&example))));
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
/// the body's example.
/// Inline, the label cannot read as the first field's.
fn build_main_card(card: &CardSchema, quill_ref: &str, label: Option<String>) -> Card {
    let reference = quill_ref
        .parse()
        .expect("quill name@version is always a valid QuillReference");
    let mut items = CardItems::default();
    items.push(PayloadItem::Quill { reference });
    items.push(PayloadItem::comment_inline("keep verbatim"));
    items.push(PayloadItem::Kind {
        value: "main".into(),
    });
    if let Some(label) = label {
        items.push(PayloadItem::comment_inline(label));
    }
    append_fields(&mut items, card);
    push_body_example(&mut items, card);
    Card::from_parts(items.into_payload(), Normalized::empty())
}

/// Build a composable card: `$kind: <kind>` carrying the optional
/// [`label_line`] inline, the `composable (0..N)` role comment, a comment
/// naming it a deletable sample, the fields, then the body's example.
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
    push_body_example(&mut items, card);
    Card::from_parts(items.into_payload(), Normalized::empty())
}

/// Append every field of a card as payload items, in [`group_fields`] order.
fn append_fields(items: &mut CardItems, card: &CardSchema) {
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
fn append_field(items: &mut CardItems, field: &FieldSchema) {
    if field.is_variant_bearing() {
        append_variant(items, field);
        return;
    }

    if matches!(field.r#type, FieldType::Matrix { .. }) {
        push_leading(items, field);
        push_container_field(items, &field.name, matrix_cell(), Vec::new(), field);
        return;
    }

    if typed_dict_props(field).is_some() || typed_table_props(field).is_some() {
        push_leading(items, field);
        let (value, nested) = container_cell(field, &[]);
        push_container_field(items, &field.name, value, nested, field);
        for line in dormant_table(field) {
            items.push(PayloadItem::comment(line));
        }
        return;
    }

    append_scalar(items, field);
}

/// A matrix's blueprint cell: the empty mapping, which is the sparse spelling
/// of a vocabulary nobody has ticked. The roster rides the inline annotation, so
/// expanding every member here would show a model twenty-seven subforms to
/// delete and a seed it must not ship.
fn matrix_cell() -> JsonValue {
    JsonValue::Object(JsonMap::new())
}

/// The `# e.g.` text for a matrix declaring columns: its first member held,
/// each column at [`column_hint`]. A matrix holds no `example:` of its own, so
/// the slot is free; a checklist has no line, the bare tick being its whole
/// spelling.
fn matrix_eg(field: &FieldSchema) -> Option<String> {
    let columns = field.matrix_columns();
    if columns.is_empty() {
        return None;
    }
    let first = field.r#type.matrix_roster().keys().next()?;
    let cells: Vec<String> = std::iter::once(format!("{MATRIX_HELD_KEY}: true"))
        .chain(columns.iter().map(|(name, col)| {
            format!("{}: {}", flow_scalar(name), column_hint(col))
        }))
        .collect();
    Some(format!("{{{}: {{{}}}}}", flow_scalar(first), cells.join(", ")))
}

/// One column's value in a matrix hint: `example:` › `default:` › its
/// container shape › its inline annotation's `<type>[<format>]`.
fn column_hint(col: &FieldSchema) -> String {
    if let Some(value) = col.example.as_ref().or(col.default.as_ref()) {
        return saphyr_emit_flow(value.as_json());
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
    type_expression(col)
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
/// [`label_line`], the cap, then the `# e.g.` hint.
fn push_leading(items: &mut CardItems, field: &FieldSchema) {
    if let Some(label) = label_line(field.title.as_deref(), field.description.as_deref()) {
        items.push(PayloadItem::comment(label));
    }
    if let Some(cap) = cap_hint(field) {
        items.push(PayloadItem::comment(cap));
    }
    if let Some(eg) = eg_text(field) {
        items.push(PayloadItem::comment(format!("e.g. {eg}")));
    }
}

/// The text after `# e.g. ` for a field: its `example:`, else a matrix's
/// member hint.
fn eg_text(field: &FieldSchema) -> Option<String> {
    field.example.as_ref().map(eg_hint).or_else(|| matrix_eg(field))
}

/// The `# up to <N>` leading line for a capped array, in the own-line form
/// `# composable (0..N)` already takes for a card kind's cardinality. A limit
/// the blueprint does not show is a limit the MCP flow learns from prose, which
/// is what `max:` exists to stop.
fn cap_hint(field: &FieldSchema) -> Option<String> {
    field.max.map(|max| format!("up to {max}"))
}

/// A leaf's cell: its `default:`, else empty. An `example:` never answers a
/// cell; it rides the `# e.g.` hint.
fn scalar_value(field: &FieldSchema) -> JsonValue {
    field
        .default
        .as_ref()
        .map_or(JsonValue::Null, |d| d.as_json().clone())
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
/// ([`label_line`] + `# e.g.` + inline type annotation, addressed by
/// `container_path`/slot). `prefix` is the container
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
        if let Some(eg) = eg_text(prop) {
            nested.push(NestedComment {
                container_path: prefix.to_vec(),
                position: slot,
                text: format!("e.g. {eg}"),
                inline: false,
            });
        }
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
        nested.extend(
            dormant_table(prop)
                .into_iter()
                .map(|line| world_comment(prefix, slot + 1, line)),
        );
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
        return (matrix_cell(), Vec::new());
    }
    if typed_dict_props(prop).is_some() || typed_table_props(prop).is_some() {
        return container_cell(prop, path);
    }
    (scalar_value(prop), Vec::new())
}

/// A container's value plus the nested comments its subtree carries, at `path`
/// relative to the field value (`[]` at card level).
///
/// An **array** `default:` is shippable as-is, so it renders verbatim. A
/// **typed dictionary** holds no literal (`quill::default_on_namespace`), so it
/// always expands per property.
fn container_cell(field: &FieldSchema, path: &[PathSegment]) -> (JsonValue, Vec<NestedComment>) {
    if let Some(props) = typed_dict_props(field) {
        let mut map = JsonMap::new();
        let nested = build_property_mapping(&mut map, props, path);
        return (JsonValue::Object(map), nested);
    }

    let row_props = typed_table_props(field).unwrap_or_else(|| {
        unreachable!("container_cell is reached only for a typed dictionary or a typed table")
    });
    match field.default.as_ref().map(|d| d.as_json()) {
        Some(default) => (default.clone(), Vec::new()),
        None if rowless(field, row_props) => (JsonValue::Array(Vec::new()), Vec::new()),
        None => {
            let mut row_path = path.to_vec();
            row_path.push(PathSegment::Index(0));
            let mut row = JsonMap::new();
            let nested = build_property_mapping(&mut row, row_props, &row_path);
            (JsonValue::Array(vec![JsonValue::Object(row)]), nested)
        }
    }
}

/// A row type declaring no properties is schema-invalid in practice, and a
/// `max: 0` table holds no row at all: neither has a row to show.
fn rowless(field: &FieldSchema, row_props: &IndexMap<String, Box<FieldSchema>>) -> bool {
    row_props.is_empty() || field.max == Some(0)
}

/// The lines of a `default: []` table's field holding its synthetic row, to
/// follow the live `[]` commented out: the config-file spelling of an
/// alternative, taken by deleting the live line and uncommenting these.
fn dormant_table(field: &FieldSchema) -> Vec<String> {
    let Some(row_props) = typed_table_props(field) else {
        return Vec::new();
    };
    let empty_default = matches!(
        field.default.as_ref().map(|d| d.as_json()),
        Some(JsonValue::Array(rows)) if rows.is_empty()
    );
    if !empty_default || rowless(field, row_props) {
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
        FieldType::Date => "date<YYYY-MM-DD | today>".into(),
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
                "  tags: # array<string>\n",
                "  address: # object\n",
                "    # The city\n",
                "    city: # string\n",
                "    zip: \"00000\" # string\n",
            )),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "refs: # array<object>\n",
                "  - org: # string\n",
                "    lead: # object\n",
                "      email: # string\n",
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
            city: { type: string, default: Reston }
            zip: { type: string, default: "20190" }
"#)
        .blueprint();
        assert!(t.contains("  address: # object\n"), "{t}");
        assert!(t.contains("    city: Reston # string\n"), "{t}");
    }

    #[test]
    fn a_markdown_example_surfaces_as_eg_hint_not_inline_value() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio: { type: richtext, example: "Hello world" }
"#)
        .blueprint();
        assert!(t.contains("# e.g. Hello world\nbio: # richtext<markdown>\n"));
    }

    /// An example documents shape, never an answer: the cell no `default:`
    /// holds stays empty and the example rides the hint.
    #[test]
    fn an_example_never_takes_the_cell() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    classification: { type: enum, values: [UNCLASSIFIED, CUI], example: UNCLASSIFIED }
"#)
        .blueprint();
        assert!(
            t.contains("# e.g. UNCLASSIFIED\nclassification: # enum<UNCLASSIFIED | CUI>\n"),
            "{t}"
        );
    }

    #[test]
    fn a_blank_default_emits_a_blank_cell() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    note: { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("\nnote: \"\" # string\n"), "{t}");

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
    fn an_array_example_renders_as_a_flow_hint_with_context_quoting() {
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
        assert!(
            t.contains(
                "# e.g. [Mr. John Doe, 123 Main St, \"Anytown, USA\"]\nrecipient: # array<string>\n"
            ),
            "{t}"
        );
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
    title: { type: string }
    size: { type: number, default: 11 }
    flag: { type: boolean, default: false }
    issued: { type: date }
    published: { type: datetime }
    refs: { type: array, default: [], items: { type: string } }
"#)
        .blueprint();
        assert!(t.contains("title: # string\n"));
        assert!(t.contains("size: 11 # number\n"));
        assert!(t.contains("flag: false # boolean\n"));
        assert!(t.contains("issued: # date<YYYY-MM-DD | today>\n"));
        assert!(t.contains("published: # datetime<YYYY-MM-DDThh:mm[:ss]>\n"));
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
        assert!(t.contains("counts: # array<integer>\n"), "{t}");
        assert!(
            t.contains("sections: # array<richtext<markdown>>\n"),
            "{t}"
        );
        assert!(t.contains("tags: # array<string>\n"), "{t}");
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
    }

    #[test]
    fn endorsed_markdown_default_is_a_literal_block() {
        let t = cfg(r###"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    bio:
      type: richtext
      default: "## About me\n\nHello."
"###)
        .blueprint();
        assert!(
            t.contains("bio: |- # richtext<markdown>\n  ## About me\n\n  Hello.\n"),
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
quill: { name: taro, version: 0.1.0, backend: typst, description: x }
main:
  fields:
    flavor: { type: string, default: taro }
"#)
        .blueprint();
        assert!(t.starts_with(
            "~~~\n$quill: taro@0.1.0 # keep verbatim\n$kind: main # x\nflavor: taro # string\n"
        ));
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
                "~~~\n$quill: taro@0.1.0 # keep verbatim\n$kind: main # A taro order form.\n"
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
            "~~~\n$kind: note # A short note appended to the document.\n# composable (0..N)\n# sample card; delete if not needed\nauthor: # string\n"
        ));
    }

    #[test]
    fn a_title_leads_the_comment_naming_its_field_or_card() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  title: Memorandum
  fields:
    contact: { type: string, title: Point of contact, description: Who answers questions. }
    office: { type: string, title: Office symbol }
    address:
      type: object
      properties:
        city: { type: string, title: City }
card_kinds:
  note:
    title: Marginal note
    description: A short note.
    fields:
      text: { type: string }
"#)
        .blueprint();
        for line in [
            "$kind: main # Memorandum — x\n",
            "# Point of contact — Who answers questions.\ncontact: # string\n",
            "# Office symbol\noffice: # string\n",
            "  # City\n  city: # string\n",
            "$kind: note # Marginal note — A short note.\n",
        ] {
            assert!(t.contains(line), "missing {line:?}:\n{t}");
        }
    }

    /// A body is a cell: it stays empty, and its example rides the `# body
    /// e.g.` line closing the payload above it, named so it cannot read as the
    /// last field's.
    #[test]
    fn a_body_example_rides_an_eg_line_over_an_empty_body() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Dear Sir or Madam,\n\nI am writing to...\n"
  fields:
    to: { type: string }
card_kinds:
  note:
    fields:
      author: { type: string }
  skills:
    body: { enabled: false, example: unused }
    fields:
      items: { type: array, items: { type: string } }
"#)
        .blueprint();
        assert!(
            t.contains("to: # string\n# body e.g. \"Dear Sir or Madam,\\n\\nI am writing to...\"\n~~~\n\n~~~\n"),
            "{t}"
        );
        assert!(t.contains("author: # string\n~~~\n\n~~~\n"), "{t}");
        assert!(t.ends_with("items: # array<string>\n~~~\n"), "{t}");
        let doc = Document::parse(&t).expect("blueprint must parse").document;
        assert!(doc.main().body().is_blank());
        assert!(doc.cards().iter().all(|c| c.body().is_blank()));
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
    fn a_defaultless_typed_table_emits_a_synthetic_row() {
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
            "# Cited works.\nreferences: # array<object>\n  -\n    # Citing organization.\n    org: # string\n"
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
        assert!(t.contains("refs: # array<object>\n  - org: # string\n"));
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

    /// The empty cell stays shippable, and the field holding the row it hides
    /// follows commented out: deleting the live line and uncommenting the
    /// rest adds the row, at every depth.
    #[test]
    fn a_dormant_table_swaps_in_for_its_live_line() {
        let bp = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    attendees:
      type: array
      default: []
      items:
        type: object
        properties:
          name: { type: string, description: Full name. }
          voting: { type: boolean, default: false }
          tags: { type: array, default: [], items: { type: object, properties: { label: { type: string } } } }
    next: { type: string }
"#)
        .blueprint();
        let dormant = concat!(
            "attendees: [] # array<object>\n",
            "# attendees:\n",
            "#   -\n",
            "#     # Full name.\n",
            "#     name: # string\n",
            "#     voting: false # boolean\n",
            "#     tags: [] # array<object>\n",
            "#     # tags:\n",
            "#     #   - label: # string\n",
            "next: # string\n",
        );
        assert!(bp.contains(dormant), "{bp}");
        let doc = Document::parse(&bp).expect("blueprint must parse").document;
        assert_eq!(doc.to_markdown(), bp);

        let swapped = bp.replace(
            dormant,
            concat!(
                "attendees:\n",
                "  -\n",
                "    # Full name.\n",
                "    name: # string\n",
                "    voting: false # boolean\n",
                "    tags:\n",
                "      - label: # string\n",
                "next: # string\n",
            ),
        );
        let doc = Document::parse(&swapped).expect("swapped table must parse").document;
        let rows = doc.main().payload().get("attendees").expect("attendees");
        assert!(rows.as_json()[0]["tags"][0].get("label").is_some(), "{swapped}");
    }

    #[test]
    fn a_capped_out_table_with_empty_default_has_no_dormant_row() {
        let t = cfg(r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    refs:
      type: array
      default: []
      max: 0
      items:
        type: object
        properties:
          org: { type: string }
"#)
        .blueprint();
        assert!(t.contains("refs: [] # array<object>\n~~~"), "{t}");
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
        street: { type: string, description: Street line. }
        city:   { type: string }
        zip:    { type: string, default: "" }
"#)
        .blueprint();
        assert!(t.contains("# Mailing address.\naddress: # object\n"));
        assert!(t.contains("  # Street line.\n  street: # string\n"));
        assert!(t.contains("  city: # string\n"));
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
        assert!(t.contains("  street: # string\n"));
        assert!(t.contains("  city: \"\" # string\n"));
    }

    /// The `# e.g.` hint lands at every depth a property is declared.
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
            t.contains("# e.g. Top hello\nbio: # richtext<markdown>\n"),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "contact: # object\n",
                "  # e.g. Nested hello\n",
                "  bio: # richtext<markdown>\n",
            )),
            "{t}"
        );
        assert!(
            t.contains(concat!(
                "rows: # array<object>\n",
                "  -\n",
                "    # e.g. Row hello\n",
                "    bio: # richtext<markdown>\n",
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
