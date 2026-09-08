//! Quill schema and core type definitions.
use std::collections::HashMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::QuillValue;

/// A field's `ui:` block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFieldSchema {
    /// Display label for the field: decoupled from the snake_case wire key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiline: Option<bool>,
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
/// `body:` section. `example_content` is `#[serde(skip)]` and unauthorable.
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
    /// Canonical-content form of [`example`](Self::example), imported once at
    /// quill load and cached here. `None` when there is no example or the schema
    /// was built outside the loader, in which case consumers fall back to
    /// importing `example`.
    #[serde(skip)]
    pub example_content: Option<QuillValue>,
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
pub(crate) const UI_CARD_SCHEMA_KEYS: &[&str] = &["title", "groups"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiCardSchema {
    /// Display label for the card kind: literal string or `{field_name}`
    /// template. See `docs/quills/quill-yaml-reference.md`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The card's group registry: the visible table of contents that names
    /// every group a field may reference and fixes their display order. A
    /// field's `ui.group` is a *reference* into this registry, validated at
    /// load. Absent when the card declares no groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<GroupRegistry>,
}

/// One entry in a card's [`GroupRegistry`]. The `id` decouples identity from
/// label as a field's snake_case key decouples from its `ui.title`: renaming
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardSchema {
    /// The map key carries this on the wire; skipped during serialization to avoid duplication.
    #[serde(skip_serializing, default)]
    pub name: String,
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
    /// A card kind's name and its ordered field map. `description`, `ui`, and
    /// `body` start absent.
    pub fn new(name: String, fields: IndexMap<String, FieldSchema>) -> Self {
        Self {
            name,
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
    /// [`Content`](quillmark_content::Content); markdown is a projection of it.
    RichText {
        /// Exactly one `Para` line, no container, no islands. Enforced at
        /// coercion, validation, and load-time literal import.
        inline: bool,
    },
    /// The same [`Content`](quillmark_content::Content) through a *literal*
    /// codec ([`from_plaintext`](quillmark_content::from_plaintext) /
    /// [`to_plaintext`](quillmark_content::to_plaintext)): `*hi*` is four
    /// characters, verbatim both ways, never emphasis.
    PlainText {
        /// A single line, enforced where [`RichText`](Self::RichText)'s is.
        inline: bool,
    },
    /// A closed finite domain, its members carried in
    /// [`FieldSchema::enum_values`] and string-valued only.
    Enum,
}

impl FieldType {
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
            "enum" => Some(FieldType::Enum),
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
            FieldType::Enum => "enum",
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

/// The field set one enum member brings into play, in declaration order.
pub type VariantFields = IndexMap<String, Box<FieldSchema>>;

/// The key carrying the discriminant inside a variant-bearing enum's value.
///
/// Reserved: a variant may not declare a field under this name
/// (`quill::variant_reserved_field_name`).
pub const VARIANT_DISCRIMINANT_KEY: &str = "value";

/// Schema definition for a template field. `default:` answers both the value
/// axis and the obligation one ([`must_fill`](Self::must_fill)); `SCHEMAS.md`
/// §"Value and obligation: one declaration" is the rule.
///
/// The prose types' single-line constraint has **one** carrier, the
/// `inline` payload on [`FieldType::RichText`] / [`FieldType::PlainText`]. The
/// wire's sibling `inline:` key folds into it at deserialize (through
/// [`from_quill_value`](Self::from_quill_value)) and the hand-written
/// `Serialize` re-emits it from there, so the flag cannot live in two places
/// that disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSchema {
    /// The map key carries this on the wire; not serialized, to avoid duplication.
    pub name: String,
    pub r#type: FieldType,
    pub description: Option<String>,
    /// The value most authors want; interpolated when the field is omitted.
    /// Its presence is the whole of [`must_fill()`](Self::must_fill).
    pub default: Option<QuillValue>,
    /// A value matching the desired type and shape but not the value most
    /// authors want; documents shape only and never renders as the value.
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    /// The members of an `enum` field; no other type accepts them.
    /// Serializes as `values`.
    pub enum_values: Option<Vec<String>>,
    /// Per-member field sets on an `enum` field, keyed by member (a subset of
    /// [`enum_values`](Self::enum_values); the blank owns no set). Declaring it
    /// is what turns the field into a container (`SCHEMAS.md` §"Enum
    /// variants").
    pub variants: Option<IndexMap<String, VariantFields>>,
    /// A typed dictionary's properties, in declaration order.
    pub properties: Option<IndexMap<String, Box<FieldSchema>>>,
    /// Element schema, required on every `array` field. A typed table's element
    /// is an `object` carrying its own `properties`.
    pub items: Option<Box<FieldSchema>>,
    /// Canonical-content form of [`default`](Self::default) for a
    /// content-bearing field, imported once at quill load and never serialized.
    /// The render floor commits it uncoerced, so a content default crosses the
    /// seam as content rather than as a re-imported string. `None` for a field
    /// bearing no content leaf, a null or absent default, or a schema built
    /// outside the loader.
    pub default_content: Option<QuillValue>,
    /// Canonical-content form of [`example`](Self::example), cached and absent
    /// under the same conditions as
    /// [`default_content`](Self::default_content). Seeding commits it.
    pub example_content: Option<QuillValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSchemaDef {
    pub r#type: FieldType,
    pub description: Option<String>,
    pub default: Option<QuillValue>,
    pub example: Option<QuillValue>,
    pub ui: Option<UiFieldSchema>,
    /// The domain of a `type: enum` field, and the only spelling of one.
    /// Lands in [`FieldSchema::enum_values`].
    pub values: Option<Vec<String>>,
    /// Per-member field sets, keyed by member. Lands in
    /// [`FieldSchema::variants`].
    pub variants: Option<serde_json::Map<String, serde_json::Value>>,
    // Nested schema support
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
    // Element schema for arrays.
    pub items: Option<serde_json::Value>,
    pub inline: Option<bool>,
}

impl FieldSchema {
    pub fn new(name: String, r#type: FieldType, description: Option<String>) -> Self {
        Self {
            name,
            r#type,
            description,
            default: None,
            example: None,
            ui: None,
            enum_values: None,
            variants: None,
            properties: None,
            items: None,
            default_content: None,
            example_content: None,
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

    /// Whether a human must author this cell: the one answer the blueprint's
    /// marker, the seeding stamp and the `Quill::validate` predicate all read.
    /// Keyed on `default`'s *presence*, so a `default: ""` stays a skippable
    /// cell rather than becoming a marker.
    pub fn must_fill(&self) -> bool {
        self.default.is_none()
    }

    pub fn from_quill_value(key: String, value: &QuillValue) -> Result<Self, String> {
        let def: FieldSchemaDef = serde_json::from_value(value.clone().into_json())
            .map_err(|e| format!("Failed to parse field schema: {}", e))?;
        // The sole inline sync point: past here the variant payload is the
        // flag's one carrier.
        let r#type = Self::resolve_prose_inline(def.r#type, def.inline)?;
        let enum_values = Self::resolve_enum_values(&r#type, def.values)?;
        let schema = Self {
            name: key.clone(),
            r#type,
            description: def.description,
            default: def.default,
            example: def.example,
            ui: def.ui,
            enum_values,
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
            // Filled by the loader's post-pass, which alone imports and
            // validates the literals; a bare `from_quill_value` leaves them empty.
            default_content: None,
            example_content: None,
        };
        Ok(schema)
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

    /// Resolve the domain into [`FieldSchema::enum_values`]: `type: enum`
    /// requires a non-empty `values:` list, and `values:` elsewhere is an error.
    fn resolve_enum_values(
        r#type: &FieldType,
        values_key: Option<Vec<String>>,
    ) -> Result<Option<Vec<String>>, String> {
        match r#type {
            FieldType::Enum => match values_key {
                Some(v) if !v.is_empty() => Ok(Some(v)),
                _ => Err("type: enum requires a non-empty values: list".to_string()),
            },
            other => {
                if values_key.is_some() {
                    return Err(format!(
                        "values: is only valid on type: enum, not on type: {}",
                        other.as_str()
                    ));
                }
                Ok(None)
            }
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
        let len = 1
            + inline.is_some() as usize
            + self.description.is_some() as usize
            + self.default.is_some() as usize
            + self.example.is_some() as usize
            + self.ui.is_some() as usize
            + self.enum_values.is_some() as usize
            + self.variants.is_some() as usize
            + self.properties.is_some() as usize
            + self.items.is_some() as usize;
        // Field order matches the struct declaration (what a derived impl
        // emits), so `inline` trails the block and golden snapshots hold.
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("type", &self.r#type)?;
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
        if let Some(v) = &self.enum_values {
            map.serialize_entry("values", v)?;
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
        if let Some(v) = inline {
            map.serialize_entry("inline", &v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FieldSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Through `from_quill_value`, so the `inline:` fold has one path.
        // `name` is filled from the map key by the container; a bare schema
        // deserializes nameless.
        let value = serde_json::Value::deserialize(deserializer)?;
        FieldSchema::from_quill_value(String::new(), &QuillValue::from_json(value))
            .map_err(serde::de::Error::custom)
    }
}
