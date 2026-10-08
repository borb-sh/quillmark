//! A quill's `honors:` section: the table knobs and carrier elements its plate
//! renders beyond prose (`prose/canon/QUILL.md` § "Honors").

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::Serialize;

use super::{FieldSchema, FieldType};
use quillmark_content::carrier::Element;
use quillmark_content::island::IslandType;
use quillmark_content::model::Content;

/// A table or cell layout key the content model stores and a quill renders only
/// where its `honors:` declares it. Column `aligns` is not one: every Typst quill
/// honors it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableKnob {
    Widths,
    Align,
    Breakable,
    CellAlign,
    CellValign,
}

impl TableKnob {
    pub const ALL: &'static [TableKnob] = &[
        Self::Widths,
        Self::Align,
        Self::Breakable,
        Self::CellAlign,
        Self::CellValign,
    ];

    /// The construct `validation::undeclared_construct` names this knob by:
    /// `table.widths`, `cell.align`, …
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Widths => "table.widths",
            Self::Align => "table.align",
            Self::Breakable => "table.breakable",
            Self::CellAlign => "cell.align",
            Self::CellValign => "cell.valign",
        }
    }

    /// Whether the knob is a cell's key rather than a table's.
    pub fn on_cell(self) -> bool {
        matches!(self, Self::CellAlign | Self::CellValign)
    }

    /// The knob's key in its props object, and in its `honors:` list.
    pub fn key(self) -> &'static str {
        match self {
            Self::Widths => "widths",
            Self::Align | Self::CellAlign => "align",
            Self::Breakable => "breakable",
            Self::CellValign => "valign",
        }
    }

    /// How many of `content`'s tables (a table knob) or cells (a cell knob)
    /// store this knob.
    pub fn count_in(self, content: &Content) -> usize {
        let tables = content
            .islands
            .iter()
            .filter(|i| i.island_type == IslandType::Table)
            .map(|i| &i.props);
        if !self.on_cell() {
            return tables.filter(|props| props.get(self.key()).is_some()).count();
        }
        fn slice(v: Option<&serde_json::Value>) -> &[serde_json::Value] {
            v.and_then(serde_json::Value::as_array).map(Vec::as_slice).unwrap_or_default()
        }
        let cells = |props: &serde_json::Value| {
            let rows = slice(props.get("rows")).iter().flat_map(|row| slice(Some(row)));
            slice(props.get("header"))
                .iter()
                .chain(rows)
                .filter(|cell| cell.get(self.key()).is_some())
                .count()
        };
        tables.map(cells).sum()
    }
}

impl std::fmt::Display for TableKnob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a declared element stands: around blocks, or around a run of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ElementScope {
    Block,
    Inline,
}

/// One `honors.elements` entry: a `quill-<name>` element the quill renders.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElementDecl {
    pub scope: ElementScope,
    /// Each attribute's schema, a scalar type, in declaration order.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub attrs: IndexMap<String, FieldSchema>,
}

/// What a quill's `honors:` declares. Empty, the quill renders tables and
/// elements as every quill does without one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Honors {
    pub knobs: BTreeSet<TableKnob>,
    /// Keyed by element name, the part of the tag after `quill-`.
    pub elements: IndexMap<String, ElementDecl>,
}

impl Honors {
    pub fn is_empty(&self) -> bool {
        self.knobs.is_empty() && self.elements.is_empty()
    }

    pub fn declares(&self, knob: TableKnob) -> bool {
        self.knobs.contains(&knob)
    }

    /// Each declared construct's canonical spelling, one markdown example
    /// apiece: a table carrying every declared knob, then each element.
    pub(crate) fn examples(&self) -> Vec<String> {
        let mut examples = Vec::new();
        if !self.knobs.is_empty() {
            examples.push(self.table_example());
        }
        for (name, decl) in &self.elements {
            let attrs = decl
                .attrs
                .iter()
                .map(|(attr, schema)| (attr.clone(), attr_example(schema)))
                .collect();
            let element = Element::new(name.clone(), attrs).expect("a loaded element is in the grammar");
            examples.push(match decl.scope {
                ElementScope::Block => element.wrap_block("Text."),
                ElementScope::Inline => format!("Some {} here.", element.wrap_inline("text")),
            });
        }
        examples
    }

    fn table_example(&self) -> String {
        let attrs = |on_cell: bool, values: &[(TableKnob, &str)]| -> BTreeMap<String, String> {
            values
                .iter()
                .filter(|(k, _)| k.on_cell() == on_cell && self.declares(*k))
                .map(|(k, v)| (k.key().to_string(), v.to_string()))
                .collect()
        };
        let values = [
            (TableKnob::Widths, "2 1"),
            (TableKnob::Align, "center"),
            (TableKnob::Breakable, "false"),
            (TableKnob::CellAlign, "right"),
            (TableKnob::CellValign, "bottom"),
        ];
        let cell_attrs = attrs(true, &values);
        let amount = if cell_attrs.is_empty() {
            "42".to_string()
        } else {
            Element::new("cell", cell_attrs)
                .expect("cell knobs are in the grammar")
                .wrap_inline("42")
        };
        let table = format!("| Item | Amount |\n| --- | --- |\n| Total | {amount} |");
        let table_attrs = attrs(false, &values);
        if table_attrs.is_empty() {
            return table;
        }
        Element::new("table", table_attrs)
            .expect("table knobs are in the grammar")
            .wrap_block(&table)
    }
}

/// An attribute's example value: its `default:`, else its enum's first member,
/// else its type's name.
fn attr_example(schema: &FieldSchema) -> String {
    if let Some(default) = schema
        .default
        .as_ref()
        .and_then(|d| super::config::scalar_as_string(d.as_json()).or_else(|| d.as_str().map(str::to_string)))
    {
        return default;
    }
    match &schema.r#type {
        FieldType::Enum { values } if !values.is_empty() => values[0].clone(),
        ty => ty.as_str().to_string(),
    }
}

impl Serialize for Honors {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let list = |on_cell: bool| -> Vec<&'static str> {
            self.knobs.iter().filter(|k| k.on_cell() == on_cell).map(|k| k.key()).collect()
        };
        let (table, cell) = (list(false), list(true));
        let mut map = serializer.serialize_map(None)?;
        if !table.is_empty() {
            map.serialize_entry("table", &table)?;
        }
        if !cell.is_empty() {
            map.serialize_entry("cell", &cell)?;
        }
        if !self.elements.is_empty() {
            map.serialize_entry("elements", &self.elements)?;
        }
        map.end()
    }
}

/// The keys `honors:` takes.
pub(crate) const HONORS_KEYS: &[&str] = &["table", "cell", "elements"];

/// The keys an `honors.elements` entry takes.
pub(crate) const ELEMENT_KEYS: &[&str] = &["scope", "attrs"];

/// The knob named `key` in the `honors.<list>` list, `list` being `table` or
/// `cell`.
pub(crate) fn knob(list: &str, key: &str) -> Option<TableKnob> {
    TableKnob::ALL
        .iter()
        .copied()
        .find(|k| k.key() == key && k.on_cell() == (list == "cell"))
}

/// The keys the `honors.<list>` list accepts, in vocabulary order.
pub(crate) fn knob_keys(list: &str) -> Vec<&'static str> {
    TableKnob::ALL
        .iter()
        .filter(|k| k.on_cell() == (list == "cell"))
        .map(|k| k.key())
        .collect()
}

/// Whether an attribute's type is one an element attribute may take: a scalar
/// the coercion reads from one attribute value.
pub(crate) fn is_attr_type(schema: &FieldSchema) -> bool {
    schema.variants.is_none()
        && matches!(
            schema.r#type,
            FieldType::String
                | FieldType::Enum { .. }
                | FieldType::Integer
                | FieldType::Number
                | FieldType::Boolean
                | FieldType::Date
                | FieldType::DateTime
        )
}
