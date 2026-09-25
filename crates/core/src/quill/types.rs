//! Quill schema and core type definitions.
use std::collections::HashMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

/// The control a field asks an editor to draw, where the shape admits more than
/// one and the default reads wrong. Drawing it is a **request**: a consumer that
/// cannot honor it falls back to its own choice for the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldLayout {
    /// A typed table drawn as a grid, one row per element and one column per
    /// property. Valid only on an `array` whose `items` is an `object`
    /// (`quill::invalid_ui`), and a contract that every column is a
    /// [`FieldType::is_leaf`] type (`quill::table_column_not_flat`).
    Table,
}

/// A field's `ui:` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
    /// The control the field asks for; see [`FieldLayout`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<FieldLayout>,
    /// Label for an `enum`'s blank option. Absent, a consumer renders a
    /// conventional label of its own: naming the void is not every enum
    /// author's job. Its own key rather than an entry in a member-label map,
    /// because the blank is not a member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blank_title: Option<String>,
}

/// A block construct a body can hold, and the vocabulary
/// [`BodyCardSchema::unsupported`] declines one in: the block kinds the content
/// model distinguishes, minus the paragraph, which is the floor and cannot be
/// declined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockConstruct {
    Heading,
    Rule,
    Code,
    List,
    Quote,
    Table,
    Image,
}

impl BlockConstruct {
    /// The name this construct declares under, and the value that rides
    /// `plate::unsupported_construct`'s `construct` arg.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Rule => "rule",
            Self::Code => "code",
            Self::List => "list",
            Self::Quote => "quote",
            Self::Table => "table",
            Self::Image => "image",
        }
    }
}

impl std::fmt::Display for BlockConstruct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The keys [`BodyCardSchema`] deserializes, for the hint on a rejected
/// `body:` section.
pub(crate) const BODY_CARD_SCHEMA_KEYS: &[&str] = &["enabled", "example", "unsupported"];

/// Body namespace configuration for a card kind
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyCardSchema {
    /// When false, consumers must not accept or store body content for instances of this card kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Embedded verbatim in the blueprint body region; falls back to `Write <card> body here.` when absent.
    /// Has no effect when `enabled` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// The block constructs this quill's plate does not typeset in this body.
    /// An editor reads it off the schema and declines the gesture before the
    /// author makes it; content arriving by another door draws
    /// `plate::unsupported_construct` on the pre-render walk. A claim about the
    /// plate that nothing verifies (`ERROR.md`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported: Vec<BlockConstruct>,
}

/// The keys [`UiCardSchema`] deserializes, for the hint on a rejected `ui:`
/// section.
pub(crate) const UI_CARD_SCHEMA_KEYS: &[&str] = &["groups"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCardSchema {
    /// The card's group registry: the visible table of contents that names
    /// every group a field may reference and fixes their display order. A
    /// field's `ui.group` is a *reference* into this registry, validated at
    /// load. Absent when the card declares no groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<GroupRegistry>,
}

/// One entry in a card's [`GroupRegistry`]. The `id` decouples identity from
/// label as a field's snake_case key decouples from its `title`: renaming
/// the label breaks no `ui.group` reference and no persisted per-group state.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupSchema {
    /// snake_case identity; rides the registry map key (or list item) on the wire.
    pub id: String,
    /// `None` derives the label from `id` (`memo_for` → "Memo For"), as a field
    /// label derives from its key.
    pub title: Option<String>,
}

/// A card's ordered group registry (`main.ui.groups` or a card kind's
/// `ui.groups`). Declaration order is display order, so it is held as a `Vec`
/// whichever surface form it was authored in: a sequence of ids
/// (`[addressing, letterhead]`, titles derived) or a mapping of id to
/// attributes (`{ letterhead: { title: … } }`). Serializes back as the
/// mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRegistry(pub Vec<GroupSchema>);

/// The attribute block of a registry entry in the mapping authoring/emission
/// form (`id: { title: … }`). A bare `id:` (null) or `id: {}` carries no
/// override.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupEntryDef {
    title: Option<String>,
}

impl<'de> Deserialize<'de> for GroupRegistry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RegistryVisitor;
        impl<'de> serde::de::Visitor<'de> for RegistryVisitor {
            type Value = GroupRegistry;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a sequence of group ids or a mapping of group id to attributes")
            }

            // Sequence form: `[addressing, letterhead]`, bare ids, titles derived.
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<GroupRegistry, A::Error> {
                let mut groups = Vec::new();
                while let Some(id) = seq.next_element::<String>()? {
                    groups.push(GroupSchema { id, title: None });
                }
                Ok(GroupRegistry(groups))
            }

            // Mapping form: `{ addressing: {}, letterhead: { title: … } }`.
            // A null or `{}` value carries no override; declaration order is
            // preserved by serde_json's `preserve_order`.
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<GroupRegistry, A::Error> {
                let mut groups = Vec::new();
                while let Some((id, def)) = map.next_entry::<String, Option<GroupEntryDef>>()? {
                    groups.push(GroupSchema {
                        id,
                        title: def.and_then(|d| d.title),
                    });
                }
                Ok(GroupRegistry(groups))
            }
        }
        deserializer.deserialize_any(RegistryVisitor)
    }
}

impl Serialize for GroupRegistry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Canonical form: the mapping, so a title override has a home and the
        // registry key (identity) is explicit. A title-less entry emits an
        // empty object; the map's declaration order carries the display-order
        // contract on the wire.
        #[derive(Serialize)]
        struct GroupEntryOut<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            title: Option<&'a str>,
        }
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for group in &self.0 {
            map.serialize_entry(
                &group.id,
                &GroupEntryOut {
                    title: group.title.as_deref(),
                },
            )?;
        }
        map.end()
    }
}

/// Schema definition for a card kind (composable content blocks)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing)]
    pub name: String,
    /// The kind's label, a literal. Absent, a consumer humanizes
    /// [`name`](Self::name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Declaration order is display order: the map preserves Quill.yaml key
    /// order end to end (parse, iteration, `schema()` emission), so ordering
    /// needs no side-channel knob and no `ui` one exists.
    pub fields: IndexMap<String, FieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiCardSchema>,
    /// Controls whether a body editor is shown and provides optional guide text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyCardSchema>,
}

impl CardSchema {
    /// A card kind's name and its ordered field map. `title`, `description`,
    /// `ui`, and `body` start absent.
    pub fn new(name: String, fields: IndexMap<String, FieldSchema>) -> Self {
        Self {
            name,
            title: None,
            description: None,
            fields,
            ui: None,
            body: None,
        }
    }
}

impl CardSchema {
    /// Default values declared on this card's fields, keyed by field name. Fields with no `default` are omitted.
    pub fn defaults(&self) -> HashMap<String, QuillValue> {
        self.fields
            .iter()
            .filter_map(|(name, field)| field.default.as_ref().map(|v| (name.clone(), v.clone())))
            .collect()
    }

    /// Returns true if body content is permitted for instances of this card.
    /// Defaults to true when no `body` namespace is declared.
    pub fn body_enabled(&self) -> bool {
        self.body.as_ref().and_then(|b| b.enabled).unwrap_or(true)
    }
}

/// A field's declared `type:`. Each type's meaning and grammar is the
/// `SCHEMAS.md` §"Quill.yaml DSL" table.
///
/// Serializes as its type token ([`as_str`](Self::as_str)) and deserializes by
/// parsing one ([`from_str`](Self::from_str)), so the token is the whole `type:`
/// value: the prose types' single-line shape rides the sibling `inline:` key,
/// folded into the variant payload here.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Date,
    DateTime,
    /// Formatted prose over the canonical content model,
    /// [`Content`](quillmark_content::model::Content); markdown is a projection of it.
    RichText {
        /// Exactly one `Para` line, no container, no islands. Enforced at
        /// coercion, validation, and load-time literal import.
        inline: bool,
    },
    /// The same [`Content`](quillmark_content::model::Content) through a *literal*
    /// codec ([`from_plaintext`](quillmark_content::import::from_plaintext) /
    /// [`to_plaintext`](quillmark_content::export::to_plaintext)): `*hi*` is four
    /// characters, verbatim both ways, never emphasis.
    PlainText {
        /// A single line, enforced where [`RichText`](Self::RichText)'s is.
        inline: bool,
    },
    /// A closed string domain, `values` in declaration order. The blank (`""`)
    /// is accepted beside them and is never one of them.
    Enum { values: Vec<String> },
    /// A closed vocabulary someone ticks: `roster` is member id to display
    /// title in declaration order, and every member holds a synthesized
    /// [`MATRIX_HELD_KEY`] beside the field's declared columns. A namespace,
    /// not a cell (`prose/canon/SCHEMAS.md` §"Cells and namespaces").
    Matrix { roster: IndexMap<String, String> },
}

/// The tick a [`FieldType::Matrix`] synthesizes on every member, beside the
/// declared columns. Reserved: a column may not declare it
/// (`quill::matrix_reserved_column`).
pub const MATRIX_HELD_KEY: &str = "held";

/// The per-member wire key a matrix projection writes from its roster. Not a
/// cell: it carries no address, takes no literal, and a document authoring one
/// is overwritten at the projection.
pub const MATRIX_TITLE_KEY: &str = "title";

/// The member keys a matrix writes itself, and which a column may therefore not
/// declare (`quill::matrix_reserved_column`).
pub const MATRIX_RESERVED_COLUMNS: &[&str] = &[MATRIX_HELD_KEY, MATRIX_TITLE_KEY];

impl FieldType {
    /// The `type:` token alone. An `enum`'s domain and a prose type's `inline`
    /// ride sibling keys that the loader's parse folds in, so both payloads
    /// rest at their default here.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "string" => Some(FieldType::String),
            "number" => Some(FieldType::Number),
            "integer" => Some(FieldType::Integer),
            "boolean" => Some(FieldType::Boolean),
            "array" => Some(FieldType::Array),
            "object" => Some(FieldType::Object),
            "date" => Some(FieldType::Date),
            "datetime" => Some(FieldType::DateTime),
            "richtext" => Some(FieldType::RichText { inline: false }),
            "plaintext" => Some(FieldType::PlainText { inline: false }),
            "enum" => Some(FieldType::Enum { values: Vec::new() }),
            "matrix" => Some(FieldType::Matrix {
                roster: IndexMap::new(),
            }),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Number => "number",
            FieldType::Integer => "integer",
            FieldType::Boolean => "boolean",
            FieldType::Array => "array",
            FieldType::Object => "object",
            FieldType::Date => "date",
            FieldType::DateTime => "datetime",
            FieldType::RichText { .. } => "richtext",
            FieldType::PlainText { .. } => "plaintext",
            FieldType::Enum { .. } => "enum",
            FieldType::Matrix { .. } => "matrix",
        }
    }

    /// Whether a value of this type rests in one cell: it carries no nested
    /// schema, so nothing addresses below it. Prose is a leaf whatever its
    /// `inline` — how tall a cell renders is the consumer's judgement, what it
    /// contains is not.
    ///
    /// The type alone answers: an `enum` is a leaf here even on a field whose
    /// [`FieldSchema::variants`] address cells below it.
    ///
    /// Exhaustive by construction: a type joining the vocabulary answers here
    /// or does not compile.
    pub fn is_leaf(&self) -> bool {
        match self {
            FieldType::String
            | FieldType::Number
            | FieldType::Integer
            | FieldType::Boolean
            | FieldType::Date
            | FieldType::DateTime
            | FieldType::RichText { .. }
            | FieldType::PlainText { .. }
            | FieldType::Enum { .. } => true,
            FieldType::Array | FieldType::Object | FieldType::Matrix { .. } => false,
        }
    }

    /// A matrix's roster, member id to display title in declaration order.
    /// Empty for every other type.
    pub fn matrix_roster(&self) -> &IndexMap<String, String> {
        static EMPTY: std::sync::OnceLock<IndexMap<String, String>> = std::sync::OnceLock::new();
        match self {
            FieldType::Matrix { roster } => roster,
            _ => EMPTY.get_or_init(IndexMap::new),
        }
    }
}

impl Serialize for FieldType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FieldType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        FieldType::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown field type: {s:?}")))
    }
}

/// A `type:` value as written: the type token, and whether a trailing `?`
/// declares the cell [optional](FieldSchema::optional).
#[derive(Debug)]
struct TypeToken {
    r#type: FieldType,
    optional: bool,
}

impl<'de> Deserialize<'de> for TypeToken {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let (token, optional) = match s.trim().strip_suffix('?') {
            Some(token) => (token, true),
            None => (s.as_str(), false),
        };
        let r#type = FieldType::from_str(token)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown field type: {s:?}")))?;
        Ok(Self { r#type, optional })
    }
}

/// The field set one enum member brings into play, in declaration order.
pub type VariantFields = IndexMap<String, Box<FieldSchema>>;

/// The key carrying the discriminant inside a variant-bearing enum's value.
///
/// Reserved: a variant may not declare a field under this name
/// (`quill::variant_reserved_field_name`).
pub const VARIANT_DISCRIMINANT_KEY: &str = "value";

/// Schema definition for a template field.
///
/// The prose types' single-line constraint and an `enum`'s domain each have
/// **one** carrier, the [`FieldType`] payload. The wire's sibling `inline:` and
/// `values:` keys fold into it at parse and the hand-written `Serialize`
/// re-emits them from there, so neither can live in two places that disagree.
///
/// The type serializes and does not deserialize: the parse and the shape walk
/// that follows it are halves of one gate, and loading runs both. What
/// [`new`](Self::new) and a field assignment build answers to neither, which is
/// the state the `None` arms below name.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    /// The map key carries this on the wire; not serialized, to avoid duplication.
    pub name: String,
    pub r#type: FieldType,
    /// `type: <token>?`: an unanswered cell renders `none` rather than its
    /// type's blank. Exclusive with [`default`](Self::default)
    /// (`quill::optional_default`), and refused on a namespace
    /// (`quill::optional_namespace`).
    pub optional: bool,
    /// The field's label, a literal. Absent, a consumer humanizes
    /// [`name`](Self::name). Refused on an array's element
    /// (`quill::title_on_items`).
    pub title: Option<String>,
    pub description: Option<String>,
    /// The value most authors want; interpolated when the field is omitted.
    pub default: Option<QuillValue>,
    /// A value matching the desired type and shape but not the value most
    /// authors want; documents shape only and never renders as the value.
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    /// Per-member field sets on an `enum` field, keyed by member (a subset of
    /// the domain the [`FieldType::Enum`] payload carries; the blank owns no
    /// set). Declaring it is what turns the field into a container
    /// (`SCHEMAS.md` §"Enum variants").
    pub variants: Option<IndexMap<String, VariantFields>>,
    /// A typed dictionary's properties, in declaration order.
    ///
    /// `Some` on every `object` this crate hands out: `config::parse_fields`
    /// rejects the absence (`quill::object_missing_properties`) and the empty
    /// map (`quill::object_empty_properties`), and loading is the only way in.
    /// A schema built by field assignment over [`new`](Self::new) keeps `None`,
    /// which is the state the `None` arms across `crates/core/src/quill/`
    /// absorb.
    pub properties: Option<IndexMap<String, Box<FieldSchema>>>,
    /// Element schema, required on every `array` field. A typed table's element
    /// is an `object` carrying its own `properties`.
    ///
    /// `Some` under the same gate as [`properties`](Self::properties), which
    /// rejects the absence as `quill::array_missing_items`.
    pub items: Option<Box<FieldSchema>>,
    /// The element count past which an `array` overflows the page it is laid
    /// out on: page geometry, so `Quill::validate` warns
    /// (`validation::cardinality`) rather than gating the render. Valid only on
    /// an `array`.
    pub max: Option<u32>,
    /// A `matrix`'s members as the object schemas they desugar to, member id to
    /// `{held, …columns}`, in roster order. Derived at parse from the
    /// [`FieldType::Matrix`] roster and `properties:` (the columns), which stay
    /// the authored carriers, so it is not serialized.
    pub members: Option<IndexMap<String, Box<FieldSchema>>>,
    /// Canonical-content form of [`default`](Self::default) for a
    /// content-bearing field, imported once at quill load and never serialized.
    /// The render floor commits it uncoerced, so a content default crosses the
    /// seam as content rather than as a re-imported string. `None` for a field
    /// bearing no content leaf, a null or absent default, or a schema built
    /// outside the loader.
    pub default_content: Option<QuillValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub r#type: TypeToken,
    pub title: Option<String>,
    pub description: Option<String>,
    pub default: Option<QuillValue>,
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    /// The domain of a `type: enum` field, and the only spelling of one.
    /// Lands in the [`FieldType::Enum`] payload.
    pub values: Option<Vec<String>>,
    /// Per-member field sets, keyed by member. Lands in
    /// [`FieldSchema::variants`].
    pub variants: Option<serde_json::Map<String, serde_json::Value>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    // Element schema for arrays.
    pub items: Option<serde_json::Value>,
    pub inline: Option<bool>,
    /// An `array`'s element cap. Lands in [`FieldSchema::max`].
    pub max: Option<u32>,
    /// The roster of a `type: matrix` field, and the only spelling of one.
    /// Lands in the [`FieldType::Matrix`] payload.
    pub members: Option<IndexMap<String, String>>,
}

impl FieldSchema {
    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            r#type,
            optional: false,
            title: None,
            description,
            default: None,
            example: None,
            ui: None,
            variants: None,
            properties: None,
            items: None,
            max: None,
            members: None,
            default_content: None,
        }
    }

    /// An `enum`'s declared choices; empty for every other type. A domain
    /// admits its members and the blank, so an empty one admits only the blank.
    pub fn domain(&self) -> &[String] {
        match &self.r#type {
            FieldType::Enum { values } => values,
            _ => &[],
        }
    }

    /// The fields `member` brings into play, or `None` where the field declares
    /// no variants, the member owns no set, or `member` is the blank (which
    /// activates nothing).
    pub fn variant_fields(&self, member: &str) -> Option<&VariantFields> {
        self.variants.as_ref()?.get(member)
    }

    /// The declaration of the cell `name` under *any* of this field's variants,
    /// active world or not. `quill::variant_field_collision` rejects
    /// disagreement at load, so the first match is the declaration.
    pub fn variant_field(&self, name: &str) -> Option<&FieldSchema> {
        self.variants
            .as_ref()?
            .values()
            .find_map(|set| set.get(name))
            .map(Box::as_ref)
    }

    /// Whether this field rests as a variant container (`{value: …, …}`) rather
    /// than a bare scalar. `variants:` is the one key that changes a resting
    /// shape.
    pub fn is_variant_bearing(&self) -> bool {
        self.variants.is_some()
    }

    /// The discriminant a document authored for a variant-bearing field, read
    /// off either shape: the container's [`VARIANT_DISCRIMINANT_KEY`] or a bare
    /// scalar that bypassed coercion. `None` where the cell is absent or null,
    /// which is what makes it the *authored* rung of the ladder.
    pub fn authored_member(value: Option<&serde_json::Value>) -> Option<&serde_json::Value> {
        match value {
            Some(serde_json::Value::Object(o)) => o.get(VARIANT_DISCRIMINANT_KEY),
            other => other,
        }
        .filter(|v| !v.is_null())
    }

    /// The member the ladder selects: the authored discriminant, else
    /// `default:`, else the blank.
    pub fn selected_member(&self, value: Option<&serde_json::Value>) -> String {
        Self::authored_member(value)
            .and_then(|v| v.as_str())
            .or_else(|| self.default.as_ref()?.as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// Parse one field's wire form. Crate-internal because it is half the gate:
    /// `config::parse_fields` runs `validate_field_schema_shape` over what this
    /// returns, and only the pair produces a schema of the shape the rest of
    /// this module reads.
    pub(crate) fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        // The sole sync point for `inline:` and `values:`: past here the type
        // payload is each key's one carrier.
        let optional = def.r#type.optional;
        let r#type = Self::resolve_prose_inline(def.r#type.r#type, def.inline)?;
        let r#type = Self::resolve_enum_domain(r#type, def.values)?;
        let r#type = Self::resolve_matrix_roster(r#type, def.members)?;
        let max = Self::resolve_array_max(&r#type, def.max)?;
        let schema = Self {
            name: key.clone(),
            r#type,
            optional,
            title: def.title,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            variants: match def.variants {
                Some(variants) => {
                    let mut out = IndexMap::new();
                    for (member, body) in variants {
                        let fields = body.as_object().ok_or_else(|| {
                            format!(
                                "variant '{member}' must be a map of field schemas, \
                                 written as it would be under `fields:`"
                            )
                        })?;
                        let mut set = VariantFields::new();
                        for (key, value) in fields {
                            let field = FieldSchema::from_quill_value(
                                key.clone(),
                                &QuillValue::from_json(value.clone()),
                            )?;
                            set.insert(key.clone(), Box::new(field));
                        }
                        out.insert(member, set);
                    }
                    Some(out)
                }
                None => None,
            },
            properties: if let Some(props) = def.properties {
                let mut p = IndexMap::new();
                for (key, value) in props {
                    let prop =
                        FieldSchema::from_quill_value(key.clone(), &QuillValue::from_json(value))?;
                    p.insert(key, Box::new(prop));
                }
                Some(p)
            } else {
                None
            },
            items: if let Some(items) = def.items {
                Some(Box::new(FieldSchema::from_quill_value(
                    format!("{key}[]"),
                    &QuillValue::from_json(items),
                )?))
            } else {
                None
            },
            max,
            members: None,
            // Filled by the loader's post-pass, which alone imports and
            // validates the literals; a bare `from_quill_value` leaves them empty.
            default_content: None,
        };
        let mut schema = schema;
        schema.rebuild_matrix_members()?;
        Ok(schema)
    }

    /// Expand a `matrix`'s roster into the per-member object schemas every
    /// container walk reads: one `object` per member id, carrying
    /// [`MATRIX_HELD_KEY`] beside the declared columns.
    ///
    /// The members are copies of the columns, so the loader re-expands once its
    /// content companions are imported.
    pub(crate) fn rebuild_matrix_members(&mut self) -> Result<(), String> {
        let ids: Vec<String> = self.r#type.matrix_roster().keys().cloned().collect();
        if ids.is_empty() {
            return Ok(());
        }
        let columns = self.properties.clone().unwrap_or_default();
        let mut members = IndexMap::new();
        for id in ids {
            let mut cells: IndexMap<String, Box<FieldSchema>> = IndexMap::new();
            let mut held =
                FieldSchema::new(MATRIX_HELD_KEY.to_string(), FieldType::Boolean, None);
            held.default = Some(QuillValue::from_json(serde_json::Value::Bool(false)));
            cells.insert(MATRIX_HELD_KEY.to_string(), Box::new(held));
            // A column spelling a reserved name is `quill::matrix_reserved_column`
            // at load; what the matrix writes itself stands whatever else the
            // shape pass finds.
            for (name, column) in columns
                .iter()
                .filter(|(n, _)| !MATRIX_RESERVED_COLUMNS.contains(&n.as_str()))
            {
                cells.insert(name.clone(), column.clone());
            }
            let mut member = FieldSchema::new(id.clone(), FieldType::Object, None);
            member.properties = Some(cells);
            members.insert(id, Box::new(member));
        }
        self.members = Some(members);
        Ok(())
    }

    /// The namespace a container field composes its value from: a typed
    /// dictionary's `properties`, or a matrix's per-member objects. `None` for
    /// every cell.
    pub fn namespace_props(&self) -> Option<&IndexMap<String, Box<FieldSchema>>> {
        match self.r#type {
            FieldType::Object => self.properties.as_ref(),
            FieldType::Matrix { .. } => self.members.as_ref(),
            _ => None,
        }
    }

    /// A matrix's declared columns: the cells beside [`MATRIX_HELD_KEY`] on
    /// every member. Empty for a checklist, and for every other type.
    pub fn matrix_columns(&self) -> &IndexMap<String, Box<FieldSchema>> {
        static EMPTY: std::sync::OnceLock<IndexMap<String, Box<FieldSchema>>> =
            std::sync::OnceLock::new();
        match self.r#type {
            FieldType::Matrix { .. } => self
                .properties
                .as_ref()
                .unwrap_or_else(|| EMPTY.get_or_init(IndexMap::new)),
            _ => EMPTY.get_or_init(IndexMap::new),
        }
    }

    /// Fold the sibling `inline:` key into a prose type's payload. Every other
    /// type rejects `inline:`, here and nowhere else.
    fn resolve_prose_inline(
        r#type: FieldType,
        inline: Option<bool>,
    ) -> Result<FieldType, String> {
        match (r#type, inline) {
            (FieldType::RichText { .. }, inline) => Ok(FieldType::RichText {
                inline: inline.unwrap_or(false),
            }),
            (FieldType::PlainText { .. }, inline) => Ok(FieldType::PlainText {
                inline: inline.unwrap_or(false),
            }),
            (_, Some(_)) => Err(
                "inline is only valid on prose types (type: richtext or type: plaintext); \
                 omit inline or declare a prose type"
                    .to_string(),
            ),
            (other, None) => Ok(other),
        }
    }

    /// Fold the sibling `values:` key into the [`FieldType::Enum`] payload:
    /// `type: enum` requires a non-empty list, and `values:` elsewhere is an
    /// error. Every other type rejects it, here and nowhere else.
    fn resolve_enum_domain(
        r#type: FieldType,
        values_key: Option<Vec<String>>,
    ) -> Result<FieldType, String> {
        match (r#type, values_key) {
            (FieldType::Enum { .. }, Some(values)) if !values.is_empty() => {
                Ok(FieldType::Enum { values })
            }
            (FieldType::Enum { .. }, _) => {
                Err("type: enum requires a non-empty values: list".to_string())
            }
            (other, Some(_)) => Err(format!(
                "values: is only valid on type: enum, not on type: {}",
                other.as_str()
            )),
            (other, None) => Ok(other),
        }
    }

    /// Fold the sibling `members:` key into the [`FieldType::Matrix`] payload:
    /// `type: matrix` requires a roster naming at least one member, and
    /// `members:` elsewhere is an error.
    fn resolve_matrix_roster(
        r#type: FieldType,
        members: Option<IndexMap<String, String>>,
    ) -> Result<FieldType, String> {
        match (r#type, members) {
            (FieldType::Matrix { .. }, Some(roster)) if !roster.is_empty() => {
                Ok(FieldType::Matrix { roster })
            }
            (FieldType::Matrix { .. }, _) => Err(
                "type: matrix requires a members: roster naming at least one member".to_string(),
            ),
            (other, Some(_)) => Err(format!(
                "members: is only valid on type: matrix, not on type: {}",
                other.as_str()
            )),
            (other, None) => Ok(other),
        }
    }

    /// Fold the sibling `max:` key: an element cap is arity, which only an
    /// `array` has.
    fn resolve_array_max(r#type: &FieldType, max: Option<u32>) -> Result<Option<u32>, String> {
        match (r#type, max) {
            (FieldType::Array, max) => Ok(max),
            (_, None) => Ok(None),
            (other, Some(_)) => Err(format!(
                "max: caps an array's element count and is only valid on type: array, \
                 not on type: {}",
                other.as_str()
            )),
        }
    }
}

impl Serialize for FieldSchema {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let inline = matches!(
            self.r#type,
            FieldType::RichText { inline: true } | FieldType::PlainText { inline: true }
        )
        .then_some(true);
        let values = match &self.r#type {
            FieldType::Enum { values } => Some(values),
            _ => None,
        };
        let roster = match &self.r#type {
            FieldType::Matrix { roster } => Some(roster),
            _ => None,
        };
        let len = 1
            + inline.is_some() as usize
            + self.title.is_some() as usize
            + self.description.is_some() as usize
            + self.default.is_some() as usize
            + self.example.is_some() as usize
            + self.ui.is_some() as usize
            + values.is_some() as usize
            + roster.is_some() as usize
            + self.variants.is_some() as usize
            + self.properties.is_some() as usize
            + self.items.is_some() as usize
            + self.max.is_some() as usize;
        // The emission order is what `usaf_memo/0.2.0/__golden__/schema.yaml`
        // pins: `values` between `ui` and `variants`, `inline` trailing the
        // block, both read off the type payload.
        let mut map = serializer.serialize_map(Some(len))?;
        if self.optional {
            map.serialize_entry("type", &format!("{}?", self.r#type.as_str()))?;
        } else {
            map.serialize_entry("type", &self.r#type)?;
        }
        if let Some(v) = &self.title {
            map.serialize_entry("title", v)?;
        }
        if let Some(v) = &self.description {
            map.serialize_entry("description", v)?;
        }
        if let Some(v) = &self.default {
            map.serialize_entry("default", v)?;
        }
        if let Some(v) = &self.example {
            map.serialize_entry("example", v)?;
        }
        if let Some(v) = &self.ui {
            map.serialize_entry("ui", v)?;
        }
        if let Some(v) = values {
            map.serialize_entry("values", v)?;
        }
        if let Some(v) = roster {
            map.serialize_entry("members", v)?;
        }
        if let Some(v) = &self.variants {
            map.serialize_entry("variants", v)?;
        }
        if let Some(v) = &self.properties {
            map.serialize_entry("properties", v)?;
        }
        if let Some(v) = &self.items {
            map.serialize_entry("items", v)?;
        }
        if let Some(v) = &self.max {
            map.serialize_entry("max", v)?;
        }
        if let Some(v) = inline {
            map.serialize_entry("inline", &v)?;
        }
        map.end()
    }
}
