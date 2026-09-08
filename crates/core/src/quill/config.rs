//! Quill configuration parsing and normalization.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;

use serde::{Deserialize, Serialize};

use crate::error::{Diagnostic, Severity, diag_args};
use crate::value::QuillValue;

use super::types::{BODY_CARD_SCHEMA_KEYS, UI_CARD_SCHEMA_KEYS, VARIANT_DISCRIMINANT_KEY};
use super::{BodyCardSchema, CardSchema, FieldSchema, FieldType, GroupRegistry, UiCardSchema};

/// Canonical string text for a bare scalar unambiguously representable as a
/// string: a boolean (`true`/`false`) or number (`47`, `1.0`). `None` for
/// `null` (≡ absent), strings (already strings), and collections.
///
/// Shared by `QuillConfig::conform_value` (to adopt the value) and
/// `validation::validate_value` (to accept it), so coercion and validation
/// never disagree about which bare scalars a `string` field accepts.
pub(crate) fn scalar_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Reduce a lenient value to its authored-string form: a bare string, the
/// sole element of a length-1 array when that element is a string (the
/// array-unwrap leniency), or a bare scalar's canonical text (via
/// [`scalar_as_string`]). `None` for anything else (a multi-element array, an
/// object, null), leaving the caller's own fallback to apply.
///
/// Shared by the `String` and `Content` coercion branches, which both reduce
/// a lenient value to a string before adopting it (as the field value itself,
/// or as markdown to import).
fn lenient_string(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(s) = value
        .as_array()
        .filter(|a| a.len() == 1)
        .and_then(|a| a[0].as_str())
    {
        return Some(s.to_string());
    }
    scalar_as_string(value)
}

/// Top-level configuration for a Quillmark project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuillConfig {
    pub name: String,
    /// `quill.description`: the quill itself, not `main.description`, which
    /// describes the main card's schema.
    pub description: String,
    /// The entry-point card schema (parsed from the Quill.yaml `main:` section).
    pub main: CardSchema,
    /// Named, composable card-kind schemas (parsed from the Quill.yaml
    /// `card_kinds:` section). Does not include `main`.
    pub card_kinds: Vec<CardSchema>,
    /// Names the backend that renders this quill: a registered
    /// [`Backend::id`](crate::Backend::id).
    pub backend: String,
    /// The quill's own semantic version, checked at render against the
    /// selector a document's `$quill` carries.
    pub version: String,
    pub author: String,
    /// The top-level YAML section whose key matches `backend`.
    #[serde(default)]
    pub backend_config: HashMap<String, QuillValue>,
}

impl QuillConfig {
    /// The four fields `Quill.yaml` requires. `description`, `author`,
    /// `card_kinds`, and `backend_config` start empty.
    ///
    /// This bypasses [`Self::from_yaml_with_warnings`] and its validation, so a
    /// config built here can hold shapes the parser refuses. Loading a quill
    /// goes through that path; this one is for a caller assembling a schema in
    /// memory.
    pub fn new(name: String, backend: String, version: String, main: CardSchema) -> Self {
        Self {
            name,
            description: String::new(),
            main,
            card_kinds: Vec::new(),
            backend,
            version,
            author: String::new(),
            backend_config: HashMap::new(),
        }
    }
}

/// One card-schema block: the `main:` section or a `card_kinds.<name>:` entry.
/// It fixes the key set and each block's outer shape; `fields`, `ui`, and `body`
/// stay raw so their own parsers can report per-block diagnostics.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardSchemaDef {
    pub description: Option<String>,
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub ui: Option<serde_json::Value>,
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CoercionError {
    #[error("cannot coerce `{value}` to type `{target}` at `{path}`: {reason}")]
    Uncoercible {
        path: String,
        value: String,
        target: String,
        reason: String,
    },
}

impl CoercionError {
    fn uncoercible(
        path: &str,
        value: impl std::fmt::Display,
        target: &str,
        reason: impl Into<String>,
    ) -> Self {
        CoercionError::Uncoercible {
            path: path.to_string(),
            value: value.to_string(),
            target: target.to_string(),
            reason: reason.into(),
        }
    }

    /// The facts this error's message interpolates. See
    /// [`Diagnostic::args`](crate::error::Diagnostic::args).
    ///
    /// `path` stays behind: it is a schema-space anchor that `ERROR.md`
    /// § "Three grammars, one that crosses" keeps engine-internal. So does
    /// `reason`, English minted at ~20 coercion arms and sometimes wrapping a
    /// decode error's own prose, which a key would interpolate into a
    /// translated sentence.
    pub fn args(&self) -> BTreeMap<String, serde_json::Value> {
        match self {
            CoercionError::Uncoercible {
                path: _,
                value,
                target,
                reason: _,
            } => diag_args! {
                "value" => value,
                "target" => target,
            },
        }
    }
}

/// The leniency [`QuillConfig::conform_value`] coerces under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leniency {
    /// The render floor's forgiving cascade: a shape a type cannot adopt falls
    /// through unchanged for the validation layer to report.
    Render,
    /// The strict typed write ([`Card::commit_field`](crate::document::Card::commit_field)),
    /// and the resting form [`Quill::conform`](crate::Quill::conform) lands a
    /// content field at.
    ///
    /// "Strict" is asymmetric by target, not absolute. `string` and `array` are
    /// universal sinks, so a scalar→`string` (`true` → `"true"`) and a
    /// scalar→singleton-`array` wrap stay lenient even here, both crossings
    /// being lossless and unambiguous; only the lossy or ambiguous ones
    /// (scalar→`object`, `String`→`number`/`bool`, `Boolean`↔`Number`) are
    /// rejected. So a strict write still reshapes toward `string`/`array` while
    /// refusing to invent structure or reinterpret a scalar's type.
    Write,
}

impl QuillConfig {
    pub fn card_kind(&self, name: &str) -> Option<&CardSchema> {
        self.card_kinds.iter().find(|card| card.name == name)
    }

    /// The declaration view: the user-fillable fields of the main card and each
    /// named card kind, `ui` hints included. The quill reference and the
    /// card-kind discriminators are document-level metadata, not fields, so
    /// they do not appear (`SCHEMAS.md` §"Schema emission").
    ///
    /// Key order is the ordering contract: fields, nested properties, and card
    /// kinds all emit in declaration order, so a consumer walking the maps in
    /// key order renders the authored layout.
    pub fn schema(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        let main_value =
            serde_json::to_value(&self.main).expect("CardSchema is always serializable");
        obj.insert("main".to_string(), main_value);

        if !self.card_kinds.is_empty() {
            let mut card_kinds = serde_json::Map::new();
            for card in &self.card_kinds {
                let card_value =
                    serde_json::to_value(card).expect("CardSchema is always serializable");
                card_kinds.insert(card.name.clone(), card_value);
            }
            obj.insert(
                "card_kinds".to_string(),
                serde_json::Value::Object(card_kinds),
            );
        }

        serde_json::Value::Object(obj)
    }

    /// Coerce the main card's user fields.
    pub fn coerce_payload(
        &self,
        payload: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        Self::coerce_fields(&self.main, None, payload)
    }

    /// Coerce one card's user fields, returning the input unchanged when the
    /// card kind is unknown.
    pub fn coerce_card(
        &self,
        card_kind: &str,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let Some(card_schema) = self.card_kind(card_kind) else {
            return Ok(fields.clone());
        };
        Self::coerce_fields(card_schema, Some(card_kind), fields)
    }

    /// Coerce every field the schema declares and copy the rest through. A
    /// `card_kind` anchors the error path under `card_kinds.<kind>.`; `None` is
    /// the main card, whose fields are named bare.
    fn coerce_fields(
        schema: &CardSchema,
        card_kind: Option<&str>,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in fields {
            if let Some(field_schema) = schema.fields.get(field_name) {
                let path: std::borrow::Cow<'_, str> = match card_kind {
                    Some(kind) => format!("card_kinds.{kind}.{field_name}").into(),
                    None => field_name.as_str().into(),
                };
                coerced.insert(
                    field_name.clone(),
                    Self::conform_value(field_value, field_schema, &path, Leniency::Render)?,
                );
            } else {
                coerced.insert(field_name.clone(), field_value.clone());
            }
        }
        Ok(coerced)
    }

    /// Validate a typed [`crate::document::Document`] against this configuration.
    pub fn validate_document(
        &self,
        doc: &crate::document::Document,
    ) -> Result<(), Vec<super::validation::ValidationError>> {
        super::validation::validate_typed_document(self, doc)
    }

    /// The one per-type dispatch: given a value, a field's schema, and a
    /// [`Leniency`] mode, validate/normalize the value to the canonical form the
    /// type stores. `Render` is the render floor's forgiving coercion; `Write`
    /// is the strict typed-write commit driving
    /// [`Card::commit_field`](crate::document::Card::commit_field).
    ///
    /// `validation::validate_value` conforms a document value through here at
    /// `Render` before judging it, so a type has one predicate. What validation
    /// adds is the arbitration this cannot carry — the enum domain, the datetime
    /// grammar, indexed element paths — over the value the floor built.
    pub(crate) fn conform_value(
        value: &QuillValue,
        field_schema: &super::FieldSchema,
        path: &str,
        mode: Leniency,
    ) -> Result<QuillValue, CoercionError> {
        use super::FieldType;

        let json_value = value.as_json();

        // Null ≡ absent, so a present-null passes through at every type.
        // Returning `value` rather than the JSON also preserves a `!must_fill`
        // marker riding on it: the fill flag is not part of the JSON projection.
        if json_value.is_null() {
            return Ok(value.clone());
        }

        if field_schema.is_variant_bearing() {
            return Self::conform_variant(json_value, field_schema, path, mode);
        }

        match field_schema.r#type {
            FieldType::Array => {
                let arr = if let Some(a) = json_value.as_array() {
                    a.clone()
                } else {
                    vec![json_value.clone()]
                };

                if let Some(items) = &field_schema.items {
                    let mut out = Vec::with_capacity(arr.len());
                    for (idx, elem) in arr.iter().enumerate() {
                        let coerced = Self::conform_value(
                            &QuillValue::from_json(elem.clone()),
                            items,
                            &format!("{path}[{idx}]"),
                            mode,
                        )?;
                        out.push(coerced.into_json());
                    }
                    Ok(QuillValue::from_json(serde_json::Value::Array(out)))
                } else {
                    // Defensive fallback: schema-load rejects any array without
                    // `items` (quill::array_missing_items), so a validated
                    // config never reaches here, pass the array through as-is.
                    Ok(QuillValue::from_json(serde_json::Value::Array(arr)))
                }
            }
            FieldType::Boolean => {
                if let Some(b) = json_value.as_bool() {
                    return Ok(QuillValue::from_json(serde_json::Value::Bool(b)));
                }
                if let Some(s) = json_value.as_str() {
                    let lower = s.to_lowercase();
                    if lower == "true" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(true)));
                    } else if lower == "false" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                    }
                }
                if mode == Leniency::Render {
                    if let Some(n) = json_value.as_i64() {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(n != 0)));
                    }
                    if let Some(n) = json_value.as_f64() {
                        if n.is_nan() {
                            return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                        }
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(
                            n.abs() > f64::EPSILON,
                        )));
                    }
                }

                Err(CoercionError::uncoercible(
                    path,
                    json_value,
                    "boolean",
                    "value is not coercible to boolean",
                ))
            }
            FieldType::Number => {
                if json_value.is_number() {
                    return Ok(value.clone());
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    if let Ok(f) = s.parse::<f64>() {
                        if let Some(num) = serde_json::Number::from_f64(f) {
                            return Ok(QuillValue::from_json(num.into()));
                        }
                    }
                    return Err(CoercionError::uncoercible(
                        path,
                        s,
                        "number",
                        "string is not a valid number",
                    ));
                }
                if mode == Leniency::Render {
                    if let Some(b) = json_value.as_bool() {
                        let n = if b { 1 } else { 0 };
                        return Ok(QuillValue::from_json(serde_json::Value::Number(
                            serde_json::Number::from(n),
                        )));
                    }
                }

                Err(CoercionError::uncoercible(
                    path,
                    json_value,
                    "number",
                    "value is not coercible to number",
                ))
            }
            FieldType::Integer => {
                if let Some(i) = json_value.as_i64() {
                    return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                }
                if let Some(u) = json_value.as_u64() {
                    if let Ok(i) = i64::try_from(u) {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::uncoercible(
                        path,
                        json_value,
                        "integer",
                        "integer value exceeds i64 range",
                    ));
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::uncoercible(
                        path,
                        s,
                        "integer",
                        "string is not a valid integer",
                    ));
                }
                if mode == Leniency::Render {
                    if let Some(b) = json_value.as_bool() {
                        let n = if b { 1 } else { 0 };
                        return Ok(QuillValue::from_json(serde_json::Value::Number(
                            serde_json::Number::from(n),
                        )));
                    }
                }

                Err(CoercionError::uncoercible(
                    path,
                    json_value,
                    "integer",
                    "value is not coercible to integer",
                ))
            }
            // An enum coerces as a string; domain membership is the validation
            // layer's, an out-of-domain string being a value error.
            FieldType::String | FieldType::Enum => {
                if json_value.is_string() {
                    return Ok(value.clone());
                }
                if let Some(text) = lenient_string(json_value) {
                    return Ok(QuillValue::from_json(serde_json::Value::String(text)));
                }
                match mode {
                    Leniency::Render => Ok(value.clone()),
                    Leniency::Write => Err(CoercionError::uncoercible(
                        path,
                        json_value,
                        field_schema.r#type.as_str(),
                        "value is not a string",
                    )),
                }
            }
            FieldType::PlainText { inline } => {
                // The literal codec and its two rests: `SCHEMAS.md`
                // §"Content fields rest per codec".
                let plain_check =
                    |rt: &quillmark_content::Content| -> Result<(), CoercionError> {
                        if !rt.is_plain() {
                            return Err(CoercionError::uncoercible(
                                path,
                                "<plaintext>",
                                "plaintext",
                                "plaintext carries no marks, islands, or block \
                                     formatting (lists, quotes, headings)",
                            ));
                        }
                        if inline && !rt.is_inline() {
                            return Err(CoercionError::uncoercible(
                                path,
                                "<plaintext>",
                                "plaintext(inline)",
                                "plaintext(inline) requires a single line",
                            ));
                        }
                        Ok(())
                    };
                let commit = |rt: &quillmark_content::Normalized| -> QuillValue {
                    match mode {
                        Leniency::Render => QuillValue::from_json(
                            quillmark_content::serial::to_canonical_value(rt),
                        ),
                        Leniency::Write => QuillValue::from_json(serde_json::Value::String(
                            quillmark_content::export::to_plaintext(rt),
                        )),
                    }
                };
                if json_value.is_object() {
                    let rt = quillmark_content::serial::from_canonical_value(json_value).map_err(
                        |e| {
                            CoercionError::uncoercible(
                                path,
                                "<object>",
                                "plaintext",
                                format!("not a valid content object: {e}"),
                            )
                        },
                    )?;
                    plain_check(&rt)?;
                    return Ok(commit(&rt));
                }
                let Some(text) = lenient_string(json_value) else {
                    return match mode {
                        Leniency::Render => Ok(value.clone()),
                        Leniency::Write => Err(CoercionError::uncoercible(
                            path,
                            json_value,
                            "plaintext",
                            "value is not a plaintext string or content",
                        )),
                    };
                };
                let rt = quillmark_content::from_plaintext(&text);
                plain_check(&rt)?;
                Ok(commit(&rt))
            }
            FieldType::RichText { inline } => {
                // The deliberately-lenient sibling of the strict decoders
                // (`document::canonical_richtext_value` at the write and literal
                // sites, `Codec::Richtext.decode_value` at the wire and
                // validation sites). The string branch below reduces a bare
                // scalar or length-1 array to text before importing, which a
                // strict decoder must not do, so it stays open-coded here.
                let inline_err = || {
                    CoercionError::uncoercible(
                        path,
                        "<richtext>",
                        "richtext(inline)",
                        "richtext(inline) requires a single paragraph line \
                             with no list/quote container and no islands",
                    )
                };
                let inline_check =
                    |rt: &quillmark_content::Content| -> Result<(), CoercionError> {
                        if inline && !rt.is_inline() {
                            return Err(inline_err());
                        }
                        Ok(())
                    };
                // The messages mirror `Card::commit_field`'s richtext error
                // variants, which the bindings key on.
                if mode == Leniency::Write {
                    use crate::document::RichtextValueError as E;
                    return crate::document::canonical_richtext_value(json_value, inline)
                        .map(QuillValue::from_json)
                        .map_err(|e| match e {
                            E::Decode(e) => CoercionError::uncoercible(
                                path,
                                "<richtext>",
                                "richtext",
                                e.into_message(),
                            ),
                            E::Unshaped => CoercionError::uncoercible(
                                path,
                                json_value,
                                "richtext",
                                crate::document::Codec::Richtext.unshaped_message(json_value),
                            ),
                            E::NotInline => inline_err(),
                        });
                }
                if json_value.is_object() {
                    let rt = quillmark_content::serial::from_canonical_value(json_value).map_err(
                        |e| {
                            CoercionError::uncoercible(
                                path,
                                "<object>",
                                "richtext",
                                format!("not a valid richtext content: {e}"),
                            )
                        },
                    )?;
                    inline_check(&rt)?;
                    return Ok(QuillValue::from_json(
                        quillmark_content::serial::to_canonical_value(&rt),
                    ));
                }
                let Some(markdown) = lenient_string(json_value) else {
                    // Neither content nor stringifiable: left for the validation
                    // layer, as the String branch leaves it.
                    return Ok(value.clone());
                };
                let rt = quillmark_content::import::from_markdown(&markdown).map_err(|e| {
                    CoercionError::uncoercible(
                        path,
                        &markdown,
                        "richtext",
                        format!("markdown import failed: {e}"),
                    )
                })?;
                inline_check(&rt)?;
                Ok(QuillValue::from_json(
                    quillmark_content::serial::to_canonical_value(&rt),
                ))
            }
            FieldType::Date | FieldType::DateTime => {
                let text = if let Some(s) = json_value.as_str() {
                    // The blank is a value: it survives coercion so the ladder
                    // sees a *present* cell and lets it outrank a `default:`,
                    // as `""` does for `string`. Nulling it here would make one
                    // authored literal mean "absent" for `date` and "explicitly
                    // nothing" for `string`.
                    if s.is_empty() {
                        return Ok(value.clone());
                    }
                    s.to_string()
                } else if let Some(arr) = json_value.as_array() {
                    if arr.len() == 1 {
                        if let Some(s) = arr[0].as_str() {
                            s.to_string()
                        } else {
                            return Err(CoercionError::uncoercible(
                                path,
                                json_value,
                                field_schema.r#type.as_str(),
                                "value must be a string",
                            ));
                        }
                    } else {
                        return Err(CoercionError::uncoercible(
                            path,
                            json_value,
                            field_schema.r#type.as_str(),
                            "value must be a single string",
                        ));
                    }
                } else {
                    return Err(CoercionError::uncoercible(
                        path,
                        json_value,
                        field_schema.r#type.as_str(),
                        "value must be a string",
                    ));
                };

                // Both types store verbatim; only the grammar differs, and
                // neither truncates (`formats`).
                let (valid, reason) = match field_schema.r#type {
                    FieldType::Date => {
                        (super::formats::is_valid_date(&text), "invalid date format")
                    }
                    _ => (
                        super::formats::is_valid_datetime(&text),
                        "invalid datetime format",
                    ),
                };
                if valid {
                    Ok(QuillValue::from_json(serde_json::Value::String(text)))
                } else {
                    Err(CoercionError::uncoercible(
                        path,
                        text,
                        field_schema.r#type.as_str(),
                        reason,
                    ))
                }
            }
            FieldType::Object => {
                if let Some(obj) = json_value.as_object() {
                    if let Some(props) = &field_schema.properties {
                        let coerced_obj = Self::coerce_object_props(obj, props, path, mode)?;
                        Ok(QuillValue::from_json(serde_json::Value::Object(
                            coerced_obj,
                        )))
                    } else {
                        Ok(value.clone())
                    }
                } else {
                    match mode {
                        Leniency::Render => Ok(value.clone()),
                        Leniency::Write => Err(CoercionError::uncoercible(
                            path,
                            json_value,
                            "object",
                            "value is not an object",
                        )),
                    }
                }
            }
        }
    }

    /// Walk `obj`'s keys, coercing any that match `props` against the matching
    /// schema and copying any others through verbatim. `parent_path` is the
    /// breadcrumb for the enclosing scope (e.g. `"foo[3]"` or `"foo"`); each
    /// child's path is `"{parent_path}.{k}"`.
    fn coerce_object_props(
        obj: &serde_json::Map<String, serde_json::Value>,
        props: &IndexMap<String, Box<super::FieldSchema>>,
        parent_path: &str,
        mode: Leniency,
    ) -> Result<serde_json::Map<String, serde_json::Value>, CoercionError> {
        let mut out = serde_json::Map::new();
        for (k, v) in obj {
            if let Some(prop_schema) = props.get(k) {
                let child_path = format!("{parent_path}.{k}");
                out.insert(
                    k.clone(),
                    Self::conform_value(
                        &QuillValue::from_json(v.clone()),
                        prop_schema,
                        &child_path,
                        mode,
                    )?
                    .into_json(),
                );
            } else {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    }

    /// Coerce a variant-bearing enum to its container form, `{value: <member>,
    /// …}`, from either authored shape: the bare scalar (`classification: CUI`)
    /// or the map. A key no variant declares carries through verbatim, and a key
    /// a *non-active* variant declares is coerced by that variant's schema and
    /// kept (`SCHEMAS.md` §"Enum variants").
    ///
    /// The cell lookup takes the first variant declaring a name without
    /// consulting the discriminant. That is total because
    /// `quill::variant_field_collision` rejects a name two worlds declare
    /// *differently*, so every repetition is the same declaration.
    fn conform_variant(
        json_value: &serde_json::Value,
        field_schema: &super::FieldSchema,
        path: &str,
        mode: Leniency,
    ) -> Result<QuillValue, CoercionError> {
        let discriminant = |v: &serde_json::Value| -> Option<String> {
            v.as_str()
                .map(str::to_string)
                .or_else(|| scalar_as_string(v))
        };

        let Some(object) = json_value.as_object() else {
            return match discriminant(json_value) {
                Some(member) => {
                    let mut out = serde_json::Map::new();
                    out.insert(
                        VARIANT_DISCRIMINANT_KEY.to_string(),
                        serde_json::Value::String(member),
                    );
                    Ok(QuillValue::from_json(serde_json::Value::Object(out)))
                }
                None => match mode {
                    Leniency::Render => Ok(QuillValue::from_json(json_value.clone())),
                    Leniency::Write => Err(CoercionError::uncoercible(
                        path,
                        json_value,
                        "enum",
                        "value is neither a member nor a variant container",
                    )),
                },
            };
        };

        let mut out = serde_json::Map::new();
        for (key, value) in object {
            if key == VARIANT_DISCRIMINANT_KEY {
                // Null ≡ absent: leave the key out so the ladder fills it.
                if value.is_null() {
                    continue;
                }
                match discriminant(value) {
                    Some(member) => {
                        out.insert(key.clone(), serde_json::Value::String(member));
                    }
                    None => match mode {
                        Leniency::Render => {
                            out.insert(key.clone(), value.clone());
                        }
                        Leniency::Write => {
                            return Err(CoercionError::uncoercible(
                                &format!("{path}.{key}"),
                                value,
                                "enum",
                                "value is not a string",
                            ));
                        }
                    },
                }
                continue;
            }
            match field_schema.variant_field(key) {
                Some(schema) => {
                    let coerced = Self::conform_value(
                        &QuillValue::from_json(value.clone()),
                        schema,
                        &format!("{path}.{key}"),
                        mode,
                    )?;
                    out.insert(key.clone(), coerced.into_json());
                }
                None => {
                    out.insert(key.clone(), value.clone());
                }
            }
        }
        Ok(QuillValue::from_json(serde_json::Value::Object(out)))
    }

    /// Recursively validate a field's structural shape. Every type nests at
    /// every depth; `at_card_level` gates the two keys that do not,
    /// `variants:` and `ui.group`.
    ///
    /// Returns the first violation as a ready-to-push [`Diagnostic`] whose
    /// message names `owner` (the field-name path, e.g. `rows[].tags`), or
    /// `None` when the shape is valid.
    fn validate_field_schema_shape(
        schema: &FieldSchema,
        owner: &str,
        at_card_level: bool,
    ) -> Option<Diagnostic> {
        let err = |code: &str, message: String| {
            Some(Diagnostic::new(Severity::Error, message).with_code(code.to_string()))
        };

        // `items` is only meaningful on arrays; `properties` only on objects.
        if schema.r#type != FieldType::Array && schema.items.is_some() {
            return err(
                "quill::items_not_supported",
                format!(
                    "Field '{owner}' declares 'items' but is not type: array. \
                     'items' (the element schema) is only valid on array fields."
                ),
            );
        }
        // `ui.group` clusters card-level fields only: the blueprint's grouping
        // pass never descends into object properties or array items, so a nested
        // `group` is an inert knob. Reject it rather than let it silently do
        // nothing, the same dead-knob class this walk exists to catch.
        if !at_card_level && schema.ui.as_ref().and_then(|u| u.group.as_ref()).is_some() {
            return err(
                "quill::nested_group_not_supported",
                format!(
                    "Field '{owner}' sets ui.group in a nested position. Grouping applies \
                     only to card-level fields; an object property or array item cannot \
                     join a group."
                ),
            );
        }

        if let Some(variants) = &schema.variants {
            // One level only, for the reason `SCHEMAS.md` §"Enum variants" gives.
            if !at_card_level {
                return err(
                    "quill::variant_placement",
                    format!(
                        "Field '{owner}' declares 'variants' in a nested position. \
                         Variants apply only to card-level enum fields; an object \
                         property, an array item, and another variant's field cannot \
                         carry one."
                    ),
                );
            }
            let Some(values) = &schema.enum_values else {
                return err(
                    "quill::variants_on_non_enum",
                    format!(
                        "Field '{owner}' declares 'variants' but is not type: enum. \
                         A variant names one member of a closed domain, so declare \
                         type: enum with a values: list."
                    ),
                );
            };
            if variants.is_empty() {
                return err(
                    "quill::variant_empty",
                    format!(
                        "Field '{owner}' has an empty 'variants' map. Declare at least \
                         one member's field set, or remove the key entirely."
                    ),
                );
            }
            for (member, fields) in variants {
                let member_owner = format!("{owner}.variants.{member}");
                if !values.iter().any(|v| v == member) {
                    return err(
                        "quill::variant_unknown_value",
                        format!(
                            "Field '{owner}' declares a variant for '{member}', which is \
                             not one of its 'values'. A variant keys on a declared member; \
                             the blank (\"\") is not one and owns no field set."
                        ),
                    );
                }
                if fields.is_empty() {
                    return err(
                        "quill::variant_empty",
                        format!(
                            "Field '{member_owner}' declares no fields. A variant exists to \
                             bring a field set into play; drop the member's entry if it \
                             brings none."
                        ),
                    );
                }
                for (name, field) in fields {
                    if name == VARIANT_DISCRIMINANT_KEY {
                        return err(
                            "quill::variant_reserved_field_name",
                            format!(
                                "Field '{member_owner}' declares a field named \
                                 '{VARIANT_DISCRIMINANT_KEY}', which carries the \
                                 discriminant itself. Rename the field."
                            ),
                        );
                    }
                    if !Self::is_snake_case_identifier(name) {
                        return err(
                            "quill::invalid_field_name",
                            format!(
                                "Invalid variant field key '{name}' on '{member_owner}': \
                                 field keys must be snake_case (lowercase letters, digits, \
                                 and underscores only), and capitalized field keys are \
                                 reserved."
                            ),
                        );
                    }
                    // A cell inherits the discriminant's `ui.group`.
                    if let Some(diag) = Self::validate_field_schema_shape(
                        field,
                        &format!("{member_owner}.{name}"),
                        false,
                    ) {
                        return Some(diag);
                    }
                    if let Some((first, _)) = variants
                        .iter()
                        .take_while(|(m, _)| *m != member)
                        .find_map(|(m, set)| set.get(name).map(|f| (m, f)))
                        .filter(|(_, first)| first.as_ref() != field.as_ref())
                    {
                        return err(
                            "quill::variant_field_collision",
                            format!(
                                "Field '{owner}' declares '{name}' differently under \
                                 '{first}' and '{member}'. A name is one cell of the \
                                 container whichever world brings it into play, so every \
                                 variant declaring it must declare it identically; give \
                                 the two readings separate names, or share one declaration \
                                 with a YAML anchor."
                            ),
                        );
                    }
                }
            }
        }

        match schema.r#type {
            FieldType::Object => {
                let Some(props) = &schema.properties else {
                    return err(
                        "quill::object_missing_properties",
                        format!(
                            "Field '{owner}' has type: object but no properties defined. \
                             Declare a properties map, or use type: array with \
                             items: {{ type: object, properties: … }} for a list of objects."
                        ),
                    );
                };
                if props.is_empty() {
                    return err(
                        "quill::object_empty_properties",
                        format!(
                            "Field '{owner}' has type: object with an empty properties map. \
                             Declare at least one property, or remove the field entirely."
                        ),
                    );
                }
                props.iter().find_map(|(name, prop)| {
                    Self::validate_field_schema_shape(prop, &format!("{owner}.{name}"), false)
                })
            }
            FieldType::Array => {
                if schema.properties.is_some() {
                    return err(
                        "quill::array_properties_not_supported",
                        format!(
                            "Field '{owner}' is type: array with a bare 'properties' map. \
                             Declare the element type under 'items' instead: for a list \
                             of objects use items: {{ type: object, properties: … }}."
                        ),
                    );
                }
                let Some(items) = &schema.items else {
                    return err(
                        "quill::array_missing_items",
                        format!(
                            "Field '{owner}' has type: array but no 'items' element schema. \
                             Declare the element type, e.g. items: {{ type: string }} \
                             for a list of strings or items: {{ type: object, \
                             properties: … }} for a list of objects."
                        ),
                    );
                };
                Self::validate_field_schema_shape(items, &format!("{owner}[]"), false)
            }
            // Scalars are leaves; nothing further to validate.
            _ => None,
        }
    }

    /// Reject multi-line descriptions. Single-line is required so the leading
    /// `# <description>` blueprint slot stays one line and the field-comment
    /// stack remains parseable for LLM consumers.
    fn validate_description_singleline(
        desc: Option<&str>,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(d) = desc {
            if d.contains('\n') {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{} description must be a single line; multi-line \
                             descriptions are not allowed.",
                            owner_label
                        ),
                    )
                    .with_code("quill::description_multiline".to_string()),
                );
            }
        }
    }

    /// Reject `>`, `;`, `|` in enum literals (reserved by the blueprint inline
    /// annotation grammar — `<format>` close, role separator, enum value
    /// separator — with no escape syntax), and reject `""`, which is the
    /// engine-supplied blank rather than a choice.
    fn validate_enum_literals(
        field: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(values) = &field.enum_values {
            for v in values {
                if v.is_empty() {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{} declares `\"\"` in `values:`. The blank is not a \
                                 choice: it is supplied by the engine and always \
                                 accepted.",
                                owner_label
                            ),
                        )
                        .with_code("quill::enum_blank_member".to_string())
                        .with_hint(
                            "Remove `\"\"` from `values:`; every enum accepts the blank \
                             already. Keep `default: \"\"` to leave the field optional, \
                             and declare a member such as `undecided` or `n_a` where \
                             the empty state is itself a choice someone makes."
                                .to_string(),
                        ),
                    );
                }
                if v.contains('>') || v.contains(';') || v.contains('|') {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{} enum value '{}' contains a reserved character \
                                 ('>', ';', or '|') that conflicts with the \
                                 blueprint inline annotation grammar.",
                                owner_label, v
                            ),
                        )
                        .with_code("quill::format_literal_reserved_char".to_string()),
                    );
                }
            }
        }
    }

    /// Recursively validate field-level blueprint constraints across the field,
    /// any nested object properties, and an array's element schema (`items`).
    fn validate_field_blueprint_constraints(
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        Self::validate_description_singleline(schema.description.as_deref(), owner_label, errors);
        Self::validate_enum_literals(schema, owner_label, errors);
        if schema.example.is_some() {
            Self::reject_namespace_literal("example", schema, owner_label, errors);
        }
        if schema.default.is_some() {
            Self::reject_namespace_literal("default", schema, owner_label, errors);
        }
        if let Some(v) = &schema.example {
            Self::validate_schema_slot("example", v, schema, owner_label, errors);
        }
        if let Some(v) = &schema.default {
            Self::validate_schema_slot("default", v, schema, owner_label, errors);
        }
        if let Some(props) = &schema.properties {
            for (name, prop) in props {
                let nested = format!("{}.{}", owner_label, name);
                Self::validate_field_blueprint_constraints(prop, &nested, errors);
            }
        }
        if let Some(variants) = &schema.variants {
            for (member, fields) in variants {
                for (name, field) in fields {
                    let nested = format!("{owner_label}.variants.{member}.{name}");
                    Self::validate_field_blueprint_constraints(field, &nested, errors);
                }
            }
        }
        if let Some(items) = &schema.items {
            let nested = format!("{}[]", owner_label);
            Self::validate_field_blueprint_constraints(items, &nested, errors);
        }
    }

    /// Validate a card's group registry and every card-level field's `ui.group`
    /// reference against it. Nested `ui.group` is already rejected upstream by
    /// [`validate_field_schema_shape`](Self::validate_field_schema_shape), so
    /// only card-level fields are considered here.
    ///
    /// With a registry present, `ui.group` is a *reference*: registry ids carry
    /// the same snake_case discipline as field keys and must be unique, and a
    /// reference to an id the registry does not declare is `quill::unknown_group`.
    /// A `ui.group` with no registry to reference is `quill::implicit_group`,
    /// one per card.
    fn validate_card_groups(label: &str, card: &CardSchema, errors: &mut Vec<Diagnostic>) {
        let referenced: Vec<&str> = card
            .fields
            .values()
            .filter_map(|f| f.ui.as_ref().and_then(|u| u.group.as_deref()))
            .collect();

        match card.ui.as_ref().and_then(|u| u.groups.as_ref()) {
            Some(GroupRegistry(groups)) => {
                let mut ids: HashSet<&str> = HashSet::new();
                for g in groups {
                    if !Self::is_snake_case_identifier(&g.id) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!(
                                    "{label} group id '{}' must be snake_case (lowercase letters, \
                                     digits, and underscores only); the display label goes in \
                                     'title:'.",
                                    g.id
                                ),
                            )
                            .with_code("quill::invalid_group_id".to_string()),
                        );
                    }
                    // Insert regardless of snake_case validity so a reference to
                    // an ill-named id resolves: one diagnostic, not a cascade.
                    if !ids.insert(g.id.as_str()) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("{label} declares group '{}' more than once.", g.id),
                            )
                            .with_code("quill::duplicate_group".to_string()),
                        );
                    }
                }
                let unresolved: BTreeSet<&str> =
                    referenced.iter().copied().filter(|g| !ids.contains(g)).collect();
                for group in unresolved {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{label} field references group '{group}', which is not declared \
                                 in ui.groups. Add it to the registry, or fix the reference."
                            ),
                        )
                        .with_code("quill::unknown_group".to_string()),
                    );
                }
            }
            None => {
                if !referenced.is_empty() {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{label} uses ui.group without a ui.groups registry. Declare the \
                                 groups under the card's ui.groups."
                            ),
                        )
                        .with_code("quill::implicit_group".to_string())
                        .with_hint(
                            "Add a ui.groups registry listing each group id, and reference the id \
                             from each field's ui.group."
                                .to_string(),
                        ),
                    );
                }
            }
        }
    }

    /// Refuse a `default:` / `example:` declared on a **typed dictionary**, a
    /// namespace rather than a cell (`SCHEMAS.md` §"Cells and namespaces"). The
    /// variant container refuses the same shape under
    /// `quill::{default,example}_type_mismatch`; an `array` keeps its literal,
    /// `items:` fixing the element type but never the arity.
    fn reject_namespace_literal(
        slot: &str,
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        let Some(props) = schema
            .properties
            .as_ref()
            .filter(|_| matches!(schema.r#type, FieldType::Object))
        else {
            return;
        };
        let names: Vec<&str> = props.keys().map(String::as_str).collect();
        errors.push(
            Diagnostic::new(
                Severity::Error,
                format!(
                    "{owner_label} declares type 'object' but carries a {slot}. A typed \
                     dictionary is a namespace, not a cell: each property holds its own {slot}."
                ),
            )
            .with_code(format!("quill::{slot}_on_namespace"))
            .with_hint(format!(
                "Move each value onto the property that holds it ({}), and remove the \
                 container's {slot}.",
                names.join(", ")
            )),
        );
    }

    /// Validate a single `example:` or `default:` literal against the declared
    /// schema, pushing `quill::*`-namespaced [`Diagnostic`]s for any violations.
    ///
    /// Delegates type/enum/format/recursion checking to
    /// [`super::validation::validate_schema_literal`] (the shared conformance
    /// primitive) then converts each [`ValidationError`] into a Quill.yaml
    /// load-time diagnostic with the appropriate `quill::{slot}_*` error code
    /// and author-friendly hint.
    fn validate_schema_slot(
        slot: &str,
        value: &QuillValue,
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        use super::validation::{validate_schema_literal, ValidationError};

        // A Quill.yaml schema-literal anchor (`$seed.<kind>`, a field label) is
        // config-space, not a document path; it rides the one serializer with
        // its prefix as an opaque head field.
        let owner_path = crate::path::DocPath::new().field(owner_label);
        for violation in validate_schema_literal(schema, value, &owner_path) {
            let diag = match &violation {
                ValidationError::TypeMismatch {
                    path,
                    expected: declared,
                    actual,
                    source_token,
                    ..
                } => {
                    // validation.rs says "number" for every JSON number outside
                    // `i64`: the fractional literal, which the YAML author calls
                    // a float, and the integer past the range, which is not one.
                    let display_actual = match actual.as_str() {
                        "number" if source_token.contains(['.', 'e', 'E']) => "float",
                        other => other,
                    };
                    // Show the offending value's content. A top-level mismatch
                    // renders the original literal (so arrays/objects show their
                    // contents); a nested mismatch is always a scalar, whose
                    // verbatim token is already the full value.
                    let preview = if path.as_str() == owner_label {
                        Self::literal_preview(value.as_json())
                    } else {
                        Self::truncate_preview(source_token)
                    };
                    let hint = if schema.is_variant_bearing() && actual == "object" {
                        let member = schema
                            .enum_values
                            .as_ref()
                            .and_then(|v| v.first())
                            .map(String::as_str)
                            .unwrap_or("<member>");
                        format!(
                            "Write the {slot} as the discriminant alone ({slot}: {member}); \
                             a variant's own field carries its {slot} on that field."
                        )
                    } else if actual == "number" || actual == "integer" {
                        let schema_type = if actual == "integer" {
                            "integer"
                        } else {
                            "number"
                        };
                        format!(
                            "Quote the {slot} as \"{raw}\" if the value is intentionally a \
                             string, or change the field type to '{schema_type}'.",
                            raw = source_token.trim_matches('"'),
                        )
                    } else if actual == "string" {
                        format!(
                            "Remove the quotes around the {slot} value to keep it a {declared}."
                        )
                    } else {
                        format!(
                            "Make the {slot} value a {declared}, or change the field type to match."
                        )
                    };
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{owner_label} declares type '{declared}' but {slot} is {display_actual} ({preview})."
                        ),
                    )
                    .with_code(format!("quill::{slot}_type_mismatch"))
                    .with_hint(hint)
                }
                ValidationError::EnumViolation {
                    path,
                    value: val,
                    allowed,
                } => {
                    let values_str = allowed
                        .iter()
                        .map(|v| format!("\"{}\"", v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{path} {slot} \"{val}\" is not one of the declared enum values [{values_str}]."
                        ),
                    )
                    .with_code(format!("quill::{slot}_not_in_enum"))
                    .with_hint(format!("Set the {slot} to one of: {values_str}."))
                }
                ValidationError::FormatViolation { path, format } => Diagnostic::new(
                    Severity::Error,
                    format!("{path} {slot} has an invalid {format} format."),
                )
                .with_code(format!("quill::{slot}_format_violation"))
                .with_hint(format!("Provide a valid {format} value for the {slot}.")),
                // UnknownCard and BodyDisabled cannot arise on a literal.
                // NotInline and NotPlain can, and `literal_content` reports
                // them at load.
                _ => continue,
            };
            errors.push(diag);
        }
    }

    /// A short preview of a value for an error message: strings quoted,
    /// everything else in its JSON form, truncated past 60 characters.
    fn literal_preview(value: &serde_json::Value) -> String {
        let raw = match value {
            serde_json::Value::String(s) => format!("\"{}\"", s),
            other => other.to_string(),
        };
        Self::truncate_preview(&raw)
    }

    fn truncate_preview(raw: &str) -> String {
        const MAX: usize = 60;
        if raw.chars().count() > MAX {
            let truncated: String = raw.chars().take(MAX).collect();
            format!("{}…", truncated)
        } else {
            raw.to_string()
        }
    }

    /// Parse an optional typed sub-section (`quill.ui`, `main.ui`, `main.body`).
    /// Absent is `None`; present-but-malformed is `None` plus a diagnostic, so a
    /// typo in the block is reported rather than silently dropping it.
    fn parse_section<T: serde::de::DeserializeOwned>(
        value: Option<&serde_json::Value>,
        label: &str,
        code: &str,
        hint: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> Option<T> {
        match serde_json::from_value::<T>(value?.clone()) {
            Ok(parsed) => Some(parsed),
            Err(e) => {
                errors.push(
                    Diagnostic::new(Severity::Error, format!("Invalid '{label}' block: {e}"))
                        .with_code(code.to_string())
                        .with_hint(hint.to_string()),
                );
                None
            }
        }
    }

    /// Parse one card-schema block (`main:` or a `card_kinds.<name>:` entry).
    /// `None` plus a diagnostic when the block is not a mapping or carries an
    /// unknown key, so a typo is reported rather than loading as an empty card.
    fn parse_card_schema_def(
        value: &serde_json::Value,
        label: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> Option<CardSchemaDef> {
        // Checked ahead of serde, which reads a sequence of the right length as
        // a struct's fields in declaration order.
        if !value.is_object() {
            errors.push(
                Diagnostic::new(
                    Severity::Error,
                    format!("'{label}' must be an object (mapping of card-schema keys)"),
                )
                .with_code("quill::invalid_card_schema".to_string()),
            );
            return None;
        }

        match serde_json::from_value::<CardSchemaDef>(value.clone()) {
            Ok(def) => Some(def),
            Err(e) => {
                errors.push(
                    Diagnostic::new(Severity::Error, format!("Failed to parse '{label}': {e}"))
                        .with_code("quill::invalid_card_schema".to_string()),
                );
                None
            }
        }
    }

    /// Parse fields from a JSON map into `FieldSchema`s (both `main.fields` and
    /// a card kind's `fields`), in declaration order: the source map preserves
    /// key order and the returned `IndexMap` keeps insertion order, so no
    /// ordering pass runs. `context` labels error messages (e.g.
    /// `"field schema"`, `"card_kind 'note' field"`).
    fn parse_fields(
        fields_map: &serde_json::Map<String, serde_json::Value>,
        context: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> IndexMap<String, FieldSchema> {
        let mut fields = IndexMap::new();

        for (field_name, field_value) in fields_map {
            if !Self::is_snake_case_identifier(field_name) {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "Invalid {} '{}': field keys must be snake_case \
                             (lowercase letters, digits, and underscores only), \
                             and capitalized field keys are reserved.",
                            context, field_name
                        ),
                    )
                    .with_code("quill::invalid_field_name".to_string()),
                );
                continue;
            }

            let quill_value = QuillValue::from_json(field_value.clone());
            match FieldSchema::from_quill_value(field_name.clone(), &quill_value) {
                Ok(schema) => {
                    if let Some(diag) =
                        Self::validate_field_schema_shape(&schema, field_name, true)
                    {
                        errors.push(diag);
                        continue;
                    }

                    let owner = format!("{} '{}'", context, field_name);
                    Self::validate_field_blueprint_constraints(&schema, &owner, errors);

                    fields.insert(field_name.clone(), schema);
                }
                Err(e) => {
                    let hint = Self::field_parse_hint(field_value);
                    let mut diag = Diagnostic::new(
                        Severity::Error,
                        format!("Failed to parse {} '{}': {}", context, field_name, e),
                    )
                    .with_code("quill::field_parse_error".to_string());
                    if let Some(h) = hint {
                        diag = diag.with_hint(h);
                    }
                    errors.push(diag);
                }
            }
        }

        fields
    }

    fn field_parse_hint(field_value: &serde_json::Value) -> Option<String> {
        if let Some(obj) = field_value.as_object() {
            if obj.contains_key("title") {
                return Some(
                    "'title' is not a valid field key; use 'description' instead.".to_string(),
                );
            }
        }
        None
    }

    fn is_snake_case_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }

        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    /// Parse QuillConfig from YAML content while collecting non-fatal warnings.
    ///
    /// Returns `Ok((config, warnings))` on success, or `Err(errors)` containing all
    /// parse/validation errors when the config is invalid. Errors are always collected
    /// exhaustively: callers see every problem, not just the first.
    pub fn from_yaml_with_warnings(
        yaml_content: &str,
    ) -> Result<(Self, Vec<Diagnostic>), Vec<Diagnostic>> {
        let mut warnings: Vec<Diagnostic> = Vec::new();
        let mut errors: Vec<Diagnostic> = Vec::new();

        // Declaration order survives this only under serde_json's
        // `preserve_order`, which the ordering contract rides end to end.
        let quill_yaml_val: serde_json::Value = match serde_saphyr::from_str(yaml_content) {
            Ok(v) => v,
            Err(e) => {
                // Through `YamlError` so this shares the one saphyr adapter:
                // the engine's Rust API names stripped, the hint derived, and
                // the position carried as a `Location`.
                return Err(vec![crate::error::YamlError::from_de(e, yaml_content)
                    .to_diagnostic("quill::yaml_parse_error", "Quill.yaml")]);
            }
        };

        // Everything below reads this section, so its absence is fatal here.
        let quill_section = match quill_yaml_val.get("quill") {
            Some(v) => v,
            None => {
                return Err(vec![Diagnostic::new(
                    Severity::Error,
                    "Missing required 'quill' section in Quill.yaml".to_string(),
                )
                .with_code("quill::missing_section".to_string())
                .with_hint(
                    "Add a 'quill:' section with name, backend, version, and description."
                        .to_string(),
                )]);
            }
        };

        const KNOWN_QUILL_KEYS: &[&str] =
            &["name", "backend", "description", "version", "author", "ui"];
        if let Some(quill_obj) = quill_section.as_object() {
            for key in quill_obj.keys() {
                if !KNOWN_QUILL_KEYS.contains(&key.as_str()) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Unknown key '{}' in 'quill:' section", key),
                        )
                        .with_code("quill::unknown_key".to_string())
                        .with_hint(format!("Valid keys are: {}", KNOWN_QUILL_KEYS.join(", "))),
                    );
                }
            }
        }

        let name = match quill_section.get("name").and_then(|v| v.as_str()) {
            Some(n) => {
                if !Self::is_snake_case_identifier(n) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "Invalid Quill name '{}': quill.name must be snake_case \
                                 (lowercase letters, digits, and underscores only).",
                                n
                            ),
                        )
                        .with_code("quill::invalid_name".to_string())
                        .with_hint(format!(
                            "Rename '{}' to '{}'",
                            n,
                            n.to_lowercase().replace('-', "_")
                        )),
                    );
                }
                n.to_string()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'name' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_name".to_string())
                    .with_hint(
                        "Add 'name: your_quill_name' under the 'quill:' section.".to_string(),
                    ),
                );
                String::new()
            }
        };

        let backend = match quill_section.get("backend").and_then(|v| v.as_str()) {
            Some(b) => b.to_string(),
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'backend' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_backend".to_string())
                    .with_hint("Add 'backend: typst' (or another supported backend).".to_string()),
                );
                String::new()
            }
        };

        let description = match quill_section.get("description").and_then(|v| v.as_str()) {
            Some(d) if !d.trim().is_empty() => {
                Self::validate_description_singleline(Some(d), "quill", &mut errors);
                d.to_string()
            }
            Some(_) => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "'description' field in 'quill' section cannot be empty".to_string(),
                    )
                    .with_code("quill::empty_description".to_string()),
                );
                String::new()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'description' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_description".to_string())
                    .with_hint("Add a brief 'description:' of what this quill is for.".to_string()),
                );
                String::new()
            }
        };

        let version = match quill_section.get("version") {
            Some(version_val) => {
                // A YAML `1.0` arrives as a number; rendering the JSON number
                // keeps its fraction, which `f64::to_string` drops (`1.0` → `1`).
                let raw = if let Some(s) = version_val.as_str() {
                    s.to_string()
                } else if version_val.is_number() {
                    version_val.to_string()
                } else {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "Invalid 'version' field format".to_string(),
                        )
                        .with_code("quill::invalid_version".to_string())
                        .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                    );
                    String::new()
                };
                if !raw.is_empty() {
                    use std::str::FromStr;
                    if let Err(e) = crate::version::Version::from_str(&raw) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("Invalid version '{}': {}", raw, e),
                            )
                            .with_code("quill::invalid_version".to_string())
                            .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                        );
                    }
                }
                raw
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'version' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_version".to_string())
                    .with_hint("Add 'version: 1.0' under the 'quill:' section.".to_string()),
                );
                String::new()
            }
        };

        let author = quill_section
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let ui_hint = format!(
            "Valid keys under 'ui' are: {}.",
            UI_CARD_SCHEMA_KEYS.join(", ")
        );
        let body_hint = format!(
            "Valid keys under 'body' are: {}.",
            BODY_CARD_SCHEMA_KEYS.join(", ")
        );

        let ui_section: Option<UiCardSchema> = Self::parse_section(
            quill_section.get("ui"),
            "quill.ui",
            "quill::invalid_ui",
            &ui_hint,
            &mut errors,
        );

        let mut backend_config = HashMap::new();
        if !backend.is_empty() {
            if let Some(section_val) = quill_yaml_val.get(&backend) {
                if let Some(table) = section_val.as_object() {
                    for (key, value) in table {
                        backend_config.insert(key.clone(), QuillValue::from_json(value.clone()));
                    }
                }
            }
        }

        // `fields` earns a targeted hint: it is the most common shape mistake.
        if let Some(top_obj) = quill_yaml_val.as_object() {
            for key in top_obj.keys() {
                let is_known = key == "quill"
                    || key == "main"
                    || key == "card_kinds"
                    || (!backend.is_empty() && key == &backend);
                if is_known {
                    continue;
                }

                let mut diag = Diagnostic::new(
                    Severity::Error,
                    format!("Unknown top-level section '{}'", key),
                )
                .with_code("quill::unknown_section".to_string());

                diag = if key == "fields" {
                    diag.with_hint(
                        "Root-level `fields` is not supported; use `main.fields` instead."
                            .to_string(),
                    )
                } else {
                    diag.with_hint(format!(
                        "Valid top-level sections are: quill, main, card_kinds{}",
                        if backend.is_empty() {
                            String::new()
                        } else {
                            format!(", {}", backend)
                        }
                    ))
                };

                errors.push(diag);
            }
        }

        let main_def = quill_yaml_val
            .get("main")
            .and_then(|main_val| Self::parse_card_schema_def(main_val, "main", &mut errors))
            .unwrap_or_default();

        let fields = match &main_def.fields {
            Some(fields_map) => Self::parse_fields(fields_map, "field schema", &mut errors),
            None => IndexMap::new(),
        };

        let main_ui: Option<UiCardSchema> = Self::parse_section(
            main_def.ui.as_ref(),
            "main.ui",
            "quill::invalid_ui",
            &ui_hint,
            &mut errors,
        );

        let main_body: Option<BodyCardSchema> = Self::parse_section(
            main_def.body.as_ref(),
            "main.body",
            "quill::invalid_body",
            &body_hint,
            &mut errors,
        );

        let main_description = main_def.description;
        Self::validate_description_singleline(main_description.as_deref(), "main", &mut errors);

        let mut main = CardSchema {
            name: "main".to_string(),
            description: main_description,
            fields,
            ui: main_ui.or(ui_section),
            body: main_body,
        };

        let mut card_kinds: Vec<CardSchema> = Vec::new();
        if let Some(card_kinds_val) = quill_yaml_val.get("card_kinds") {
            match card_kinds_val.as_object() {
                None => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "'card_kinds' section must be an object (mapping of kind names to schemas)".to_string(),
                        )
                        .with_code("quill::invalid_card_kinds".to_string()),
                    );
                }
                Some(card_kinds_table) => {
                    for (card_name, card_value) in card_kinds_table {
                        if !crate::document::is_valid_kind_name(card_name) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Invalid card-kind name '{}': names must match \
                                         [a-z_][a-z0-9_]* (lowercase letters, digits, and underscores only).",
                                        card_name
                                    ),
                                )
                                .with_code("quill::invalid_card_name".to_string()),
                            );
                            continue;
                        }

                        let label = format!("card_kinds.{}", card_name);
                        let card_def =
                            match Self::parse_card_schema_def(card_value, &label, &mut errors) {
                                Some(d) => d,
                                None => continue,
                            };

                        let card_fields = match &card_def.fields {
                            Some(card_fields_table) => Self::parse_fields(
                                card_fields_table,
                                &format!("card_kind '{}' field", card_name),
                                &mut errors,
                            ),
                            None => IndexMap::new(),
                        };

                        let card_ui: Option<UiCardSchema> = Self::parse_section(
                            card_def.ui.as_ref(),
                            &format!("{}.ui", label),
                            "quill::invalid_ui",
                            &ui_hint,
                            &mut errors,
                        );

                        let card_body: Option<BodyCardSchema> = Self::parse_section(
                            card_def.body.as_ref(),
                            &format!("{}.body", label),
                            "quill::invalid_body",
                            &body_hint,
                            &mut errors,
                        );

                        Self::validate_description_singleline(
                            card_def.description.as_deref(),
                            &format!("card_kind '{}'", card_name),
                            &mut errors,
                        );
                        card_kinds.push(CardSchema {
                            name: card_name.clone(),
                            description: card_def.description,
                            fields: card_fields,
                            ui: card_ui,
                            body: card_body,
                        });
                    }
                }
            }
        }

        let warn_example_unused = |label: &str, card: &CardSchema| -> Option<Diagnostic> {
            let body = card.body.as_ref()?;
            if !card.body_enabled() && body.example.is_some() {
                Some(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`{label}.body.example` is set but `{label}.body.enabled` is false; the example will have no effect"
                        ),
                    )
                    .with_code("quill::body_example_unused".to_string())
                    .with_hint(
                        "Set `body.enabled: true` to surface the example, or remove `body.example`."
                            .to_string(),
                    ),
                )
            } else {
                None
            }
        };
        // Every card the read-only checks below walk, under the label each names
        // it by. The checks stay one loop apiece: a diagnostic's position in the
        // vector is check-major, not card-major.
        let labeled: Vec<(String, &CardSchema)> = std::iter::once(("main".to_string(), &main))
            .chain(
                card_kinds
                    .iter()
                    .map(|card| (format!("card_kinds.{}", card.name), card)),
            )
            .collect();

        for (label, card) in &labeled {
            if let Some(d) = warn_example_unused(label, card) {
                warnings.push(d);
            }
        }

        for (label, card) in &labeled {
            Self::validate_card_groups(label, card, &mut errors);
        }

        let err_example_contains_fence = |label: &str,
                                          body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let example = body.as_ref()?.example.as_deref()?;
            if example_contains_fence_line(example) {
                Some(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{label}.body.example` contains a line that would be parsed as a `~~~` card-yaml block opener; this would corrupt the blueprint"
                        ),
                    )
                    .with_code("quill::body_example_contains_fence".to_string())
                    .with_hint(
                        "Remove or reword any column-zero line that opens a card-yaml block (`~~~`, a longer tilde run, or `~~~card-yaml`). For a literal fenced code block, use a backtick fence (```).".to_string(),
                    ),
                )
            } else {
                None
            }
        };
        for (label, card) in &labeled {
            if let Some(d) = err_example_contains_fence(label, &card.body) {
                errors.push(d);
            }
        }

        // Import every richtext `default` / `example` / `body.example` literal
        // once into its canonical-content companion cache: a pure function of the
        // Quill.yaml bytes, never serialized. This is where `richtext(inline)`
        // violations and malformed richtext literals surface as load errors, and
        // where seeding and the render floor later read a pre-validated content
        // instead of re-importing the markdown per document.
        populate_card_content(&mut main, "main", &mut errors);
        for card in &mut card_kinds {
            let label = format!("card_kinds.{}", card.name);
            populate_card_content(card, &label, &mut errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok((
            QuillConfig {
                name,
                description,
                main,
                card_kinds,
                backend,
                version,
                author,
                backend_config,
            },
            warnings,
        ))
    }
}

/// Returns true if any line in `text` would be parsed as a card-yaml block
/// opener by the document parser, which would corrupt the blueprint's document
/// structure when the example is embedded verbatim as body content.
///
/// Delegates to the parser's own opener predicate
/// ([`crate::document::fences::is_card_yaml_opener_line`]) so the guard stays
/// in lock-step with fence detection: a column-zero tilde fence (three or more
/// tildes) whose info string is empty or `card-yaml`. Backtick fences,
/// language-tagged `~~~` fences, and indented fences are ordinary code blocks
/// and are not flagged.
fn example_contains_fence_line(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        crate::document::fences::is_card_yaml_opener_line(line)
    })
}

/// Whether a field's type tree contains any content leaf: the gate for caching
/// a content companion. Both `richtext` and its literal-codec sibling `plaintext`
/// are content leaves; a scalar (`string`, `integer`, `enum`, …) never carries
/// one; an `array<richtext>` or an `object` with a content property does.
pub(crate) fn field_contains_content(field: &FieldSchema) -> bool {
    match &field.r#type {
        FieldType::RichText { .. } | FieldType::PlainText { .. } => true,
        FieldType::Array => field.items.as_deref().is_some_and(field_contains_content),
        FieldType::Object => field
            .properties
            .as_ref()
            .is_some_and(|p| p.values().any(|f| field_contains_content(f))),
        // A variant container bears content when any world's cell does. Which
        // world is live is a value-time fact and this is a schema question, so
        // the union answers it: a cell that can hold content means the
        // container's companions, resting form and seed must all handle one.
        FieldType::Enum => field.variants.as_ref().is_some_and(|v| {
            v.values()
                .flat_map(|set| set.values())
                .any(|f| field_contains_content(f))
        }),
        _ => false,
    }
}

/// Populate a field's `default_content` / `example_content` companion caches from
/// its markdown literals, and every nested declaration's from its own. No-op
/// where the type tree bears no content leaf, since nothing below it does either;
/// a failed import or a `richtext(inline)` violation is appended to `errors` as a
/// load diagnostic. The walk covers every declaration position, for the reason
/// `SCHEMAS.md` §"Document seeding" gives.
///
/// `card` labels the owning card and `path` the field's declaration path
/// (`dict.note`, `rows[].note`, `c.variants.CUI.note`), the spelling
/// `validate_field_blueprint_constraints` uses, so the two load passes anchor a
/// diagnostic the same way.
fn populate_field_content(
    field: &mut FieldSchema,
    card: &str,
    path: &str,
    errors: &mut Vec<Diagnostic>,
) {
    if !field_contains_content(field) {
        return;
    }
    let owner = format!("{card} field `{path}`");
    if let Some(default) = field.default.clone() {
        match literal_content(&default, field, &format!("{owner} `default`")) {
            Ok(content) => field.default_content = content,
            Err(d) => errors.push(d),
        }
    }
    if let Some(example) = field.example.clone() {
        match literal_content(&example, field, &format!("{owner} `example`")) {
            Ok(content) => field.example_content = content,
            Err(d) => errors.push(d),
        }
    }
    if let Some(props) = field.properties.as_mut() {
        for (name, prop) in props.iter_mut() {
            populate_field_content(prop, card, &format!("{path}.{name}"), errors);
        }
    }
    if let Some(variants) = field.variants.as_mut() {
        for (member, set) in variants.iter_mut() {
            for (name, cell) in set.iter_mut() {
                let nested = format!("{path}.variants.{member}.{name}");
                populate_field_content(cell, card, &nested, errors);
            }
        }
    }
    if let Some(items) = field.items.as_mut() {
        populate_field_content(items, card, &format!("{path}[]"), errors);
    }
}

/// Populate every content companion on a card: each field's
/// `default`/`example` and each nested declaration's, plus the card's
/// `body.example` (block richtext, no inline constraint; skipped when the body is
/// disabled, since its example is inert).
fn populate_card_content(card: &mut CardSchema, label: &str, errors: &mut Vec<Diagnostic>) {
    for (name, field) in card.fields.iter_mut() {
        populate_field_content(field, label, name, errors);
    }
    if card.body_enabled() {
        if let Some(body) = card.body.as_mut() {
            if let Some(example) = body.example.clone() {
                match crate::document::import_body(&example) {
                    Ok(rt) => {
                        body.example_content = Some(QuillValue::from_json(
                            quillmark_content::serial::to_canonical_value(&rt),
                        ));
                    }
                    Err(e) => errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Failed to import {label} `body.example`: {e}"),
                        )
                        .with_code("quill::richtext_example_import".to_string()),
                    ),
                }
            }
        }
    }
}

/// Compute the canonical-content form of a richtext-bearing schema literal
/// (`default` / `example`), importing every markdown leaf once and enforcing
/// `richtext(inline)`. Recurses through `array` / `object` shapes, converting
/// only their richtext leaves and passing other elements through unchanged.
/// `Ok(None)` when the literal carries no importable richtext (a null value, or
/// a field the gate already cleared as non-richtext); `Err` is a load error.
fn literal_content(
    value: &QuillValue,
    field: &FieldSchema,
    label: &str,
) -> Result<Option<QuillValue>, Diagnostic> {
    let json = value.as_json();
    // Null ≡ absent: no data to import, so no companion is cached.
    if json.is_null() {
        return Ok(None);
    }
    match &field.r#type {
        FieldType::RichText { inline } => {
            use crate::document::{ContentDecodeError as D, RichtextValueError as E};
            crate::document::canonical_richtext_value(json, *inline)
                .map(|content| Some(QuillValue::from_json(content)))
                .map_err(|e| match e {
                    E::Decode(D::BadMarkdown(m)) => {
                        richtext_literal_error(label, &format!("markdown import failed: {m}"))
                    }
                    E::Decode(D::NotContent(m)) => {
                        richtext_literal_error(label, &format!("not a valid richtext content: {m}"))
                    }
                    E::Unshaped => richtext_literal_error(
                        label,
                        "expected a markdown string (richtext literals are authored as markdown)",
                    ),
                    E::NotInline => richtext_inline_error(label),
                })
        }
        FieldType::PlainText { inline } => {
            // Shares the one object-vs-string dispatch with the validation
            // shape check.
            let rt = match crate::document::Codec::Plaintext.decode_value(json) {
                Some(Ok(rt)) => rt,
                Some(Err(e)) => {
                    return Err(richtext_literal_error(
                        label,
                        &format!("not a valid richtext content: {}", e.into_message()),
                    ))
                }
                None => {
                    return Err(richtext_literal_error(
                        label,
                        "expected a plaintext string (plaintext literals are authored as literal text)",
                    ))
                }
            };
            if !rt.is_plain() {
                return Err(richtext_literal_error(
                    label,
                    "plaintext carries no marks, islands, or block formatting",
                ));
            }
            if *inline && !rt.is_inline() {
                return Err(richtext_inline_error(label));
            }
            Ok(Some(QuillValue::from_json(
                quillmark_content::serial::to_canonical_value(&rt),
            )))
        }
        FieldType::Array => {
            let Some(items) = field.items.as_deref() else {
                return Ok(None);
            };
            if !field_contains_content(items) {
                return Ok(None);
            }
            let arr = json.as_array().cloned().unwrap_or_default();
            let mut out = Vec::with_capacity(arr.len());
            for (idx, elem) in arr.iter().enumerate() {
                let elem_v = QuillValue::from_json(elem.clone());
                let content =
                    literal_content(&elem_v, items, &format!("{label}[{idx}]"))?.unwrap_or(elem_v);
                out.push(content.into_json());
            }
            Ok(Some(QuillValue::from_json(serde_json::Value::Array(out))))
        }
        FieldType::Object => {
            let Some(props) = field.properties.as_ref() else {
                return Ok(None);
            };
            if !props.values().any(|f| field_contains_content(f)) {
                return Ok(None);
            }
            let obj = json.as_object().cloned().unwrap_or_default();
            let mut out = serde_json::Map::new();
            for (k, v) in &obj {
                let converted = match props.get(k) {
                    Some(pschema) => {
                        let pv = QuillValue::from_json(v.clone());
                        literal_content(&pv, pschema, &format!("{label}.{k}"))?
                            .map(QuillValue::into_json)
                            .unwrap_or_else(|| v.clone())
                    }
                    None => v.clone(),
                };
                out.insert(k.clone(), converted);
            }
            Ok(Some(QuillValue::from_json(serde_json::Value::Object(out))))
        }
        _ => Ok(None),
    }
}

/// A load diagnostic for a richtext schema literal that failed to import.
fn richtext_literal_error(label: &str, reason: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        format!("Failed to import richtext {label}: {reason}"),
    )
    .with_code("quill::richtext_example_import".to_string())
}

/// A load diagnostic for a `richtext(inline)` schema literal whose content spans
/// more than a single paragraph.
fn richtext_inline_error(label: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        format!(
            "richtext(inline) {label} must be a single paragraph (no blank lines, \
             headings, lists, quotes, or tables)"
        ),
    )
    .with_code("validation::not_inline".to_string())
    .with_hint(
        "Reduce the value to one paragraph, or change the field `type:` to `richtext`.".to_string(),
    )
}

#[cfg(test)]
impl QuillConfig {
    /// The config, or every load diagnostic joined into one pretty string.
    /// Flattening `Vec<Diagnostic>` drops code, hint, and location, so the
    /// shape stays off the published surface; a test asserting on message text
    /// is the one caller that loss costs nothing.
    /// [`from_yaml_with_warnings`](Self::from_yaml_with_warnings) is the real
    /// load path.
    pub(crate) fn from_yaml(yaml_content: &str) -> Result<Self, String> {
        Self::from_yaml_with_warnings(yaml_content)
            .map(|(config, _warnings)| config)
            .map_err(|diags| {
                diags
                    .iter()
                    .map(|d| d.fmt_pretty())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
    }
}
