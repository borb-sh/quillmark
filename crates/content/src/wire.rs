//! The canonical JSON form: every object's keys in ascending order, a key
//! absent where its value is empty, so equal content serializes to equal
//! bytes. A value spelling `lines` is a v1 value: it decodes as the line form
//! and migrates.

use crate::ids::{is_ext_name, is_id};
use crate::lines::island::IslandType;
use crate::lines::model::{is_font_family_name, sort_keys_owned};
use crate::lines::serial as v1;
use crate::model::{
    Anchor, Block, BlockKind, Codec, Container, ContainerKind, Content, Invariant, Island, Mark, MarkKind, Pos,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Why a value did not decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Shape(String),
    Json(String),
    UnknownName { axis: &'static str, name: String },
    /// A key the object at `at`, a change's step or a part of one, does not
    /// define.
    UnknownKey { at: &'static str, key: String },
    Invalid(Invariant),
    /// A v1 value that did not decode.
    Legacy(v1::ParseError),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Shape(s) => write!(f, "content json shape: {s}"),
            ParseError::Json(s) => write!(f, "content json parse: {s}"),
            ParseError::UnknownName { axis, name } => write!(f, "content vocabulary: unknown {axis} {name:?}"),
            ParseError::UnknownKey { at, key } => write!(f, "content json shape: {at} holds no key {key:?}"),
            ParseError::Invalid(inv) => write!(f, "content invariant: {inv}"),
            ParseError::Legacy(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<Invariant> for ParseError {
    fn from(inv: Invariant) -> Self {
        ParseError::Invalid(inv)
    }
}

pub(crate) fn shape(s: impl Into<String>) -> ParseError {
    ParseError::Shape(s.into())
}

/// Whether `v` is spelled in the v1 line form.
pub fn is_v1(v: &Value) -> bool {
    v.get("lines").is_some() && v.get("blocks").is_none()
}

impl Content {
    pub fn to_value(&self) -> Value {
        sort_keys_owned(content_value(self))
    }

    pub fn to_json(&self) -> String {
        let mut out = Vec::new();
        self.write_canonical(&mut out).expect("a value writes to memory");
        String::from_utf8(out).expect("JSON is UTF-8")
    }

    /// The canonical bytes, [`to_json`](Self::to_json)'s, written to `w` as
    /// they are made, with no [`Value`] built: what a hash digests.
    pub fn write_canonical(&self, w: impl std::io::Write) -> std::io::Result<()> {
        serde_json::to_writer(w, &canon::Field(self)).map_err(std::io::Error::other)
    }

    /// Decode and validate, refusing a value [`normalize`](Self::normalize)
    /// would rewrite ([`Content::check_canonical`]): the one spelling a write
    /// takes. The bounds ([`Content::check_bounds`]) are checked first. A v1
    /// value migrates.
    pub fn from_value(v: &Value) -> Result<Content, ParseError> {
        let c = match decoded(v)? {
            Decoded::Migrated(c) => c,
            Decoded::Read(c) => {
                c.validate()?;
                c.check_canonical()?;
                c
            }
        };
        Ok(c)
    }

    /// The stored-value read: decode, join a table cell or a footnote's note of
    /// several paragraphs with hard breaks, normalize and validate, answering whether the value
    /// read was not the canonical one, so its holder can store it again. The
    /// bounds are checked first, as [`from_value`](Self::from_value) checks
    /// them. A lenient read is this release's migration door: a later one
    /// reads stored values as `from_value` does.
    pub fn from_value_lenient(v: &Value) -> Result<Lenient, ParseError> {
        match decoded(v)? {
            Decoded::Migrated(content) => Ok(Lenient { content, rewritten: false }),
            Decoded::Read(read) => {
                let mut content = read.clone();
                let joined = !content.join_cells().is_empty();
                content.normalize();
                content.validate()?;
                let rewritten = joined || content != read;
                Ok(Lenient { content, rewritten })
            }
        }
    }

    /// [`from_value_lenient`](Self::from_value_lenient), lowered to the line
    /// form the renderers read.
    pub fn lines_from_value(v: &Value) -> Result<crate::lines::model::Normalized, ParseError> {
        Content::from_value_lenient(v).map(|read| crate::migrate::to_lines(&read.content))
    }

    pub fn from_json(s: &str) -> Result<Content, ParseError> {
        let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
        Content::from_value(&v)
    }
}

/// A stored value as [`Content::from_value_lenient`] reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Lenient {
    pub content: Content,
    /// Whether the value read was spelled otherwise than `content`: its
    /// canonical bytes differ from the ones read, and its hash moves once it
    /// is stored again.
    pub rewritten: bool,
}

enum Decoded {
    /// A v1 value, migrated: the canonical form, ids minted.
    Migrated(Content),
    /// A value as spelled, within its bounds.
    Read(Content),
}

/// `v` decoded within the JSON depth and its bounds, a v1 value migrated.
fn decoded(v: &Value) -> Result<Decoded, ParseError> {
    if crate::model::json_too_deep(v) {
        return Err(ParseError::Invalid(Invariant::JsonTooDeep { what: "content", max: crate::MAX_JSON_DEPTH }));
    }
    if is_v1(v) {
        let lines = v1::from_canonical_value(v).map_err(ParseError::Legacy)?;
        let mut c = crate::migrate::from_lines(&lines);
        c.check_bounds()?;
        c.normalize();
        c.validate()?;
        return Ok(Decoded::Migrated(c));
    }
    let c = decode(v)?;
    c.check_bounds()?;
    Ok(Decoded::Read(c))
}

/// The canonical form serialized as it is read: every object's keys in
/// ascending order, a key absent where its value is empty, an opaque bag's
/// keys sorted as it is written, byte for byte what [`Content::to_value`]
/// serializes to.
mod canon {
    use super::{block_attrs, container_attrs, mark_attrs};
    use crate::model::{Anchor, Block, Container, Content, Island, Mark};
    use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
    use serde_json::Value;

    pub(super) struct Field<'a>(pub &'a Content);
    struct Sorted<'a>(&'a Value);
    struct OfBlock<'a>(&'a Block);
    struct OfContainer<'a>(&'a Container);
    struct OfMark<'a>(&'a Mark);
    struct OfAnchor<'a>(&'a Anchor);
    struct OfIsland<'a>(&'a Island);

    /// A list, each item serialized through its view.
    struct Each<'a, T, W>(&'a [T], fn(&'a T) -> W);

    impl<'a, T, W: Serialize> Serialize for Each<'a, T, W> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut seq = s.serialize_seq(Some(self.0.len()))?;
            for item in self.0 {
                seq.serialize_element(&(self.1)(item))?;
            }
            seq.end()
        }
    }

    impl Serialize for Field<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let c = self.0;
            let mut m = s.serialize_map(None)?;
            if !c.anchors.is_empty() {
                m.serialize_entry("anchors", &Each(&c.anchors, OfAnchor))?;
            }
            m.serialize_entry("blocks", &Each(&c.blocks, OfBlock))?;
            if let Some(codec) = c.codec.name() {
                m.serialize_entry("codec", codec)?;
            }
            if !c.containers.is_empty() {
                m.serialize_entry("containers", &Each(&c.containers, OfContainer))?;
            }
            if !c.islands.is_empty() {
                m.serialize_entry("islands", &Each(&c.islands, OfIsland))?;
            }
            if !c.marks.is_empty() {
                m.serialize_entry("marks", &Each(&c.marks, OfMark))?;
            }
            m.serialize_entry("text", &c.text)?;
            m.end()
        }
    }

    impl Serialize for OfBlock<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let b = self.0;
            let mut m = s.serialize_map(None)?;
            if let Some(attrs) = block_attrs(&b.kind) {
                m.serialize_entry("attrs", &Sorted(&attrs))?;
            }
            m.serialize_entry("id", &b.id)?;
            m.serialize_entry("kind", b.kind.tag())?;
            if !b.path.is_empty() {
                m.serialize_entry("path", &b.path)?;
            }
            m.end()
        }
    }

    impl Serialize for OfContainer<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let c = self.0;
            let mut m = s.serialize_map(None)?;
            if let Some(attrs) = container_attrs(&c.kind) {
                m.serialize_entry("attrs", &Sorted(&attrs))?;
            }
            m.serialize_entry("id", &c.id)?;
            m.serialize_entry("kind", c.kind.tag())?;
            m.end()
        }
    }

    impl Serialize for OfMark<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mk = self.0;
            let mut m = s.serialize_map(None)?;
            if let Some(attrs) = mark_attrs(&mk.kind) {
                m.serialize_entry("attrs", &Sorted(&attrs))?;
            }
            m.serialize_entry("end", &mk.end)?;
            m.serialize_entry("start", &mk.start)?;
            m.serialize_entry("type", mk.kind.tag())?;
            m.end()
        }
    }

    impl Serialize for OfAnchor<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let a = self.0;
            let mut m = s.serialize_map(None)?;
            m.serialize_entry("end", &a.end)?;
            m.serialize_entry("id", &a.id)?;
            m.serialize_entry("start", &a.start)?;
            m.end()
        }
    }

    impl Serialize for OfIsland<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let i = self.0;
            let mut m = s.serialize_map(None)?;
            m.serialize_entry("id", &i.id)?;
            m.serialize_entry("props", &Sorted(&i.props))?;
            if !i.slots.is_empty() {
                m.serialize_entry("slots", &Slots(i))?;
            }
            m.serialize_entry("type", i.island_type.as_str())?;
            m.end()
        }
    }

    struct Slots<'a>(&'a Island);

    impl Serialize for Slots<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut m = s.serialize_map(Some(self.0.slots.len()))?;
            for (k, v) in &self.0.slots {
                m.serialize_entry(k, &Field(v))?;
            }
            m.end()
        }
    }

    impl Serialize for Sorted<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            match self.0 {
                Value::Object(map) => {
                    let mut keys: Vec<&String> = map.keys().collect();
                    keys.sort();
                    let mut m = s.serialize_map(Some(keys.len()))?;
                    for k in keys {
                        m.serialize_entry(k, &Sorted(&map[k]))?;
                    }
                    m.end()
                }
                Value::Array(items) => {
                    let mut seq = s.serialize_seq(Some(items.len()))?;
                    for item in items {
                        seq.serialize_element(&Sorted(item))?;
                    }
                    seq.end()
                }
                other => other.serialize(s),
            }
        }
    }
}

pub(crate) fn content_value(c: &Content) -> Value {
    let mut m = Map::new();
    m.insert("text".into(), Value::String(c.text.clone()));
    m.insert("blocks".into(), c.blocks.iter().map(block_value).collect());
    if !c.containers.is_empty() {
        m.insert("containers".into(), c.containers.iter().map(container_value).collect());
    }
    if !c.marks.is_empty() {
        m.insert("marks".into(), c.marks.iter().map(mark_value).collect());
    }
    if !c.anchors.is_empty() {
        m.insert("anchors".into(), c.anchors.iter().map(anchor_value).collect());
    }
    if !c.islands.is_empty() {
        m.insert("islands".into(), c.islands.iter().map(island_value).collect());
    }
    if let Some(codec) = c.codec.name() {
        m.insert("codec".into(), Value::String(codec.into()));
    }
    Value::Object(m)
}

pub fn block_value(b: &Block) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(b.id.clone()));
    m.insert("kind".into(), Value::String(b.kind.tag().into()));
    if let Some(attrs) = block_attrs(&b.kind) {
        m.insert("attrs".into(), attrs);
    }
    if !b.path.is_empty() {
        m.insert("path".into(), b.path.iter().cloned().map(Value::String).collect());
    }
    Value::Object(m)
}

pub fn block_attrs(kind: &BlockKind) -> Option<Value> {
    match kind {
        BlockKind::Heading { level } => Some(obj([("level", (*level).into())])),
        BlockKind::Code { lang: Some(lang) } => Some(obj([("lang", lang.as_str().into())])),
        BlockKind::Ext { attrs, .. } if !attrs.is_empty() => Some(Value::Object(attrs.clone())),
        _ => None,
    }
}

pub fn container_value(c: &Container) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(c.id.clone()));
    m.insert("kind".into(), Value::String(c.kind.tag().into()));
    if let Some(attrs) = container_attrs(&c.kind) {
        m.insert("attrs".into(), attrs);
    }
    Value::Object(m)
}

pub fn container_attrs(kind: &ContainerKind) -> Option<Value> {
    match kind {
        ContainerKind::List { ordered, start } => Some(obj([("ordered", (*ordered).into()), ("start", (*start).into())])),
        ContainerKind::Item { checked: Some(c) } => Some(obj([("checked", (*c).into())])),
        _ => None,
    }
}

pub fn mark_value(m: &Mark) -> Value {
    let mut out = Map::new();
    out.insert("start".into(), m.start.into());
    out.insert("end".into(), m.end.into());
    out.insert("type".into(), Value::String(m.kind.tag().into()));
    if let Some(attrs) = mark_attrs(&m.kind) {
        out.insert("attrs".into(), attrs);
    }
    Value::Object(out)
}

pub fn mark_attrs(kind: &MarkKind) -> Option<Value> {
    match kind {
        MarkKind::Link { url } => Some(obj([("url", url.as_str().into())])),
        MarkKind::FontFamily { name } => Some(obj([("name", name.as_str().into())])),
        MarkKind::Ext { attrs, .. } if !attrs.is_empty() => Some(Value::Object(attrs.clone())),
        _ => None,
    }
}

pub fn anchor_value(a: &Anchor) -> Value {
    obj([("id", a.id.as_str().into()), ("start", a.start.into()), ("end", a.end.into())])
}

pub fn island_value(i: &Island) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(i.id.clone()));
    m.insert("type".into(), Value::String(i.island_type.as_str().into()));
    m.insert("props".into(), i.props.clone());
    if !i.slots.is_empty() {
        m.insert(
            "slots".into(),
            Value::Object(i.slots.iter().map(|(k, v)| (k.clone(), content_value(v))).collect()),
        );
    }
    Value::Object(m)
}

pub fn pos_value(p: &Pos) -> Value {
    match p {
        Pos::At(n) => (*n).into(),
        Pos::In { island, slot, at } => {
            obj([("island", island.as_str().into()), ("slot", slot.as_str().into()), ("at", pos_value(at))])
        }
    }
}

pub fn pos_from_value(v: &Value) -> Result<Pos, ParseError> {
    if let Some(n) = v.as_u64() {
        return Ok(Pos::At(usize_of(n)));
    }
    let island = str_key(v, "island")?;
    let slot = str_key(v, "slot")?;
    let at = v.get("at").ok_or_else(|| shape("a position inside a slot names `at`"))?;
    Ok(Pos::In { island, slot, at: Box::new(pos_from_value(at)?) })
}

pub(crate) fn obj<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn usize_of(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

pub(crate) fn str_key(v: &Value, key: &str) -> Result<String, ParseError> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| shape(format!("`{key}` is not a string")))
}

pub(crate) fn usize_key(v: &Value, key: &str) -> Result<usize, ParseError> {
    v.get(key).and_then(Value::as_u64).map(usize_of).ok_or_else(|| shape(format!("`{key}` is not a whole number")))
}

pub(crate) fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], ParseError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(shape(format!("`{key}` is not an array"))),
    }
}

pub(crate) fn attrs(v: &Value) -> Result<Map<String, Value>, ParseError> {
    match v.get("attrs") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(_) => Err(shape("`attrs` is not an object")),
    }
}

/// Decode without normalizing or validating.
pub(crate) fn decode(v: &Value) -> Result<Content, ParseError> {
    if !v.is_object() {
        return Err(shape("content is not an object"));
    }
    let text = str_key(v, "text")?;
    let blocks = match v.get("blocks") {
        Some(Value::Array(a)) => a.iter().map(block_from_value).collect::<Result<_, _>>()?,
        _ => return Err(shape("`blocks` is not an array")),
    };
    let containers = array(v, "containers")?.iter().map(container_from_value).collect::<Result<_, _>>()?;
    let marks = array(v, "marks")?.iter().map(mark_from_value).collect::<Result<_, _>>()?;
    let anchors = array(v, "anchors")?.iter().map(anchor_from_value).collect::<Result<_, _>>()?;
    let islands = array(v, "islands")?.iter().map(island_from_value).collect::<Result<_, _>>()?;
    Ok(Content { text, blocks, containers, marks, anchors, islands, codec: codec_from_value(v)? })
}

pub(crate) fn codec_from_value(v: &Value) -> Result<Codec, ParseError> {
    match v.get("codec") {
        None | Some(Value::Null) => Ok(Codec::Rich),
        Some(Value::String(name)) => {
            Codec::parse(name).ok_or_else(|| ParseError::UnknownName { axis: "codec", name: name.clone() })
        }
        Some(_) => Err(shape("`codec` is not a string")),
    }
}

pub fn block_from_value(v: &Value) -> Result<Block, ParseError> {
    let id = str_key(v, "id")?;
    let kind = block_kind_from(&str_key(v, "kind")?, &attrs(v)?)?;
    let path = match v.get("path") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|p| p.as_str().map(str::to_string).ok_or_else(|| shape("a path entry is not a string")))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(shape("`path` is not an array")),
    };
    Ok(Block { id, kind, path })
}

pub fn block_kind_from(kind: &str, attrs: &Map<String, Value>) -> Result<BlockKind, ParseError> {
    Ok(match kind {
        "para" => BlockKind::Para,
        "heading" => {
            let level = attrs
                .get("level")
                .and_then(Value::as_u64)
                .filter(|l| (1..=6).contains(l))
                .ok_or_else(|| shape("a heading's `level` is not 1 to 6"))?;
            BlockKind::Heading { level: level as u8 }
        }
        "code" => BlockKind::Code {
            lang: match attrs.get("lang") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(shape("a code block's `lang` is not a string")),
            },
        },
        "rule" => BlockKind::Rule,
        other if is_ext_name(other) => BlockKind::Ext { name: other.to_string(), attrs: attrs.clone() },
        other => return Err(ParseError::UnknownName { axis: "block kind", name: other.to_string() }),
    })
}

pub fn container_from_value(v: &Value) -> Result<Container, ParseError> {
    let id = str_key(v, "id")?;
    let kind = container_kind_from(&str_key(v, "kind")?, &attrs(v)?)?;
    Ok(Container { id, kind })
}

pub fn container_kind_from(kind: &str, attrs: &Map<String, Value>) -> Result<ContainerKind, ParseError> {
    Ok(match kind {
        "list" => ContainerKind::List {
            ordered: match attrs.get("ordered") {
                None => false,
                Some(v) => v.as_bool().ok_or_else(|| shape("a list's `ordered` is not a boolean"))?,
            },
            start: match attrs.get("start") {
                None => 1,
                Some(v) => v.as_u64().ok_or_else(|| shape("a list's `start` is not a whole number"))?,
            },
        },
        "item" => ContainerKind::Item {
            checked: match attrs.get("checked") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_bool().ok_or_else(|| shape("an item's `checked` is not a boolean"))?),
            },
        },
        "quote" => ContainerKind::Quote,
        other => return Err(ParseError::UnknownName { axis: "container kind", name: other.to_string() }),
    })
}

pub fn mark_from_value(v: &Value) -> Result<Mark, ParseError> {
    let start = usize_key(v, "start")?;
    let end = usize_key(v, "end")?;
    let kind = mark_kind_from(&str_key(v, "type")?, &attrs(v)?)?;
    Ok(Mark { start, end, kind })
}

pub fn mark_kind_from(ty: &str, attrs: &Map<String, Value>) -> Result<MarkKind, ParseError> {
    let string = |key: &str| {
        attrs
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| shape(format!("a {ty} mark's `{key}` is not a string")))
    };
    Ok(match ty {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "underline" => MarkKind::Underline,
        "strike" => MarkKind::Strike,
        "code" => MarkKind::Code,
        "link" => MarkKind::Link { url: string("url")? },
        "font_family" => {
            let name = string("name")?;
            if !is_font_family_name(&name) {
                return Err(shape(format!("`{name}` is not a font family")));
            }
            MarkKind::FontFamily { name }
        }
        other if is_ext_name(other) => MarkKind::Ext { name: other.to_string(), attrs: attrs.clone() },
        other => return Err(ParseError::UnknownName { axis: "mark type", name: other.to_string() }),
    })
}

pub fn anchor_from_value(v: &Value) -> Result<Anchor, ParseError> {
    Ok(Anchor { id: str_key(v, "id")?, start: usize_key(v, "start")?, end: usize_key(v, "end")? })
}

pub fn island_from_value(v: &Value) -> Result<Island, ParseError> {
    let id = str_key(v, "id")?;
    let ty = str_key(v, "type")?;
    let island_type =
        IslandType::parse(&ty).ok_or(ParseError::UnknownName { axis: "island type", name: ty.clone() })?;
    let props = match v.get("props") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err(shape("`props` is not an object")),
    };
    let slots = match v.get("slots") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, s)| {
                if !is_id(k) {
                    return Err(ParseError::Invalid(Invariant::BadId { id: k.clone() }));
                }
                Ok((k.clone(), decode(s)?))
            })
            .collect::<Result<_, ParseError>>()?,
        Some(_) => return Err(shape("`slots` is not an object")),
    };
    Ok(Island { id, island_type, props, slots })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn para(text: &str) -> Value {
        json!({"text": text, "blocks": [{"id": "b0", "kind": "para"}]})
    }

    fn memo() -> Value {
        json!({
            "text": "Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\n\u{FFFC}",
            "blocks": [
                {"id": "b0", "kind": "heading", "attrs": {"level": 1}},
                {"id": "b1", "kind": "para"},
                {"id": "b2", "kind": "para", "path": ["l0", "i0"]},
                {"id": "b3", "kind": "para", "path": ["l0", "i1"]},
                {"id": "b4", "kind": "para"}
            ],
            "containers": [
                {"id": "l0", "kind": "list", "attrs": {"ordered": false, "start": 1}},
                {"id": "i0", "kind": "item"},
                {"id": "i1", "kind": "item", "attrs": {"checked": true}}
            ],
            "marks": [{"start": 28, "end": 31, "type": "strong"}],
            "anchors": [{"id": "c-7f3a", "start": 23, "end": 27}],
            "islands": [{
                "id": "t0", "type": "table",
                "props": {
                    "columns": ["k1", "k2"],
                    "rows": [
                        {"id": "r0", "cells": {"k1": "s0", "k2": "s1"}},
                        {"id": "r1", "cells": {"k1": "s2", "k2": "s3"}}
                    ]
                },
                "slots": {"s0": para("Q"), "s1": para("Revenue"), "s2": para("Q3"), "s3": para("4.1M")}
            }]
        })
    }

    #[test]
    fn the_stored_value_reads_and_writes_itself() {
        let c = Content::from_value(&memo()).unwrap();
        assert_eq!(c.blocks[3].path, ["l0", "i1"]);
        assert_eq!(c.anchors[0], Anchor::new("c-7f3a", 23, 27));
        assert_eq!(c.islands[0].slot_order(), ["s0", "s1", "s2", "s3"]);
        assert_eq!(c.to_value(), sort_keys_owned(memo()));
        assert_eq!(Content::from_json(&c.to_json()).unwrap(), c);
    }

    #[test]
    fn a_value_that_breaks_the_model_is_refused() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("block count", Box::new(|v| v["blocks"].as_array_mut().unwrap().truncate(4))),
            ("duplicate id", Box::new(|v| v["blocks"][1]["id"] = json!("b0"))),
            ("missing container", Box::new(|v| v["blocks"][2]["path"] = json!(["l9", "i0"]))),
            ("item without list", Box::new(|v| v["blocks"][2]["path"] = json!(["i0"]))),
            ("split run", Box::new(|v| v["blocks"][4]["path"] = json!(["l0", "i0"]))),
            ("island count", Box::new(|v| v["islands"] = json!([]))),
            ("slot not in grid", Box::new(|v| v["islands"][0]["slots"]["s9"] = para("x"))),
            ("anchor past end", Box::new(|v| v["anchors"][0]["end"] = json!(99))),
            ("anchor id twice", Box::new(|v| {
                v["islands"][0]["slots"]["s3"]["anchors"] = json!([{"id": "c-7f3a", "start": 0, "end": 1}])
            })),
            ("break in heading", Box::new(|v| {
                v["text"] = json!("Quarterly\u{2028}memo\nRevenue grew 12%.\nShip v2\nDraft spec\n\u{FFFC}")
            })),
            ("text beside a block island", Box::new(|v| {
                v["text"] = json!("Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\nx\u{FFFC}");
                v["marks"] = json!([]);
            })),
        ];
        for (name, edit) in cases {
            let mut v = memo();
            edit(&mut v);
            assert!(matches!(Content::from_value(&v), Err(ParseError::Invalid(_))), "{name}");
        }
    }

    /// A reserved `x-` name on each axis this build reads, as a newer build
    /// might have written it.
    fn extended() -> Value {
        json!({
            "text": "Note\n\u{FFFC} glows",
            "blocks": [{"id": "b0", "kind": "x-callout", "attrs": {"tone": "warm", "icon": {"set": "a"}}}, {"id": "b1", "kind": "para"}],
            "marks": [{"start": 0, "end": 4, "type": "x-glow", "attrs": {"hue": 3}}, {"start": 2, "end": 4, "type": "strong"}],
            "islands": [{
                "id": "isl-0", "type": "x-chart", "props": {"series": [1, 2], "z": null},
                "slots": {"cap": {"text": "Q3", "blocks": [{"id": "b0", "kind": "x-caption"}], "anchors": [{"id": "c-1", "start": 0, "end": 2}]}}
            }]
        })
    }

    #[test]
    fn a_reserved_name_rides_as_written_and_another_unknown_one_refuses() {
        let c = Content::from_value(&extended()).unwrap();
        assert_eq!(c.blocks[0].kind.tag(), "x-callout");
        assert_eq!(c.islands[0].island_type, IslandType::Ext("x-chart".into()));
        assert_eq!(c.to_value(), sort_keys_owned(extended()));
        assert_eq!(Content::from_json(&c.to_json()).unwrap(), c);

        let unknown: Vec<(&str, Box<dyn Fn(&mut Value)>)> = vec![
            ("block kind", Box::new(|v| v["blocks"][0]["kind"] = json!("callout"))),
            ("mark type", Box::new(|v| v["marks"][0]["type"] = json!("glow"))),
            ("island type", Box::new(|v| v["islands"][0]["type"] = json!("chart"))),
            ("bare prefix", Box::new(|v| v["blocks"][0]["kind"] = json!("x-"))),
            ("container kind", Box::new(|v| {
                v["blocks"][1]["path"] = json!(["q0"]);
                v["containers"] = json!([{"id": "q0", "kind": "x-box"}]);
            })),
        ];
        for (axis, edit) in unknown {
            let mut v = extended();
            edit(&mut v);
            assert!(matches!(Content::from_value(&v), Err(ParseError::UnknownName { .. })), "{axis}");
        }
    }

    #[test]
    fn a_reserved_name_lowers_to_what_this_build_reads() {
        let c = Content::from_value(&extended()).unwrap();
        let lines = crate::migrate::to_lines(&c);
        assert_eq!(lines.lines[0].kind, crate::lines::model::LineKind::Para);
        assert!(lines.marks.iter().all(|m| m.kind.tag() != "x-glow"));
        let md = crate::markdown::to_markdown_checked(&c);
        assert_eq!(md.markdown, "No**te**\n\n glows");
        assert!(!md.losses.is_empty());
        let annotated = crate::markdown::to_markdown_annotated(&c).markdown;
        let r = crate::diff::revise(&c, &annotated).unwrap();
        assert_eq!(crate::change::apply(&c, &r.change).unwrap().content, c, "{annotated}");
    }

    /// Marks of distinct attributes over many blocks, each cut at every
    /// break, refuse before they are cut: at decode, and at the step of a
    /// change that would pass the bound. A repair keeps those within it.
    #[test]
    fn marks_a_value_would_multiply_refuse_before_they_are_cut() {
        use crate::change::{ApplyError, Change, MarkStep, Step, apply};
        let n = 200;
        let text = vec!["ab"; n].join("\n");
        let len = text.chars().count();
        let ext = |i: usize| json!({"start": 0, "end": len, "type": "x-a", "attrs": {"n": i}});
        let blocks: Vec<Value> = (0..n).map(|i| json!({"id": format!("b{i}"), "kind": "para"})).collect();
        let marked = json!({"text": text, "blocks": blocks, "marks": (0..n).map(ext).collect::<Vec<_>>()});
        let too_many = |e: &Invariant| matches!(e, Invariant::TooManyMarks { .. });
        assert!(matches!(Content::from_value(&marked), Err(ParseError::Invalid(e)) if too_many(&e)));

        let (repaired, repairs, _) = crate::repair::repair(crate::repair::Raw::from_value(&marked).unwrap());
        assert!(repaired.marks.len() <= crate::max_marks(len));
        assert!(repairs.iter().any(|r| matches!(r, crate::repair::Repair::Invalid(e) if too_many(e))));

        let bare = Content::from_value(&json!({"text": text, "blocks": blocks})).unwrap();
        let steps = (0..n).map(|i| Step::Mark(MarkStep::Add { mark: mark_from_value(&ext(i)).unwrap(), quote: None }));
        let refused = apply(&bare, &Change::new(steps.collect()));
        assert!(matches!(refused, Err(ApplyError::Invalid(e)) if too_many(&e)));

        let anchors: Vec<Value> = (0..=crate::MAX_ANCHORS).map(|i| json!({"id": format!("c{i}"), "start": 0, "end": 0})).collect();
        let pinned = json!({"text": "", "blocks": [{"id": "b0", "kind": "para"}], "anchors": anchors});
        assert!(matches!(Content::from_value(&pinned), Err(ParseError::Invalid(Invariant::TooManyAnchors { .. }))));
    }

    /// Each spelling `normalize` would rewrite or drop is refused by the
    /// write's decode, naming what it would change, and read by the stored
    /// read as the canonical value, reported rewritten.
    #[test]
    fn a_value_normalize_would_rewrite_is_refused_at_a_write_and_read_when_stored() {
        use Invariant::{BadMark, NotCanonical, UnusedContainer};
        let not = |what| NotCanonical { what };
        let equation = |tex: &str, display: Value| {
            json!({"id": "m0", "type": "math", "props": {"tex": tex, "display": display}})
        };
        let cases: Vec<(&str, Invariant, Box<dyn Fn(&mut Value)>)> = vec![
            ("reversed mark", BadMark { start: 31, end: 28, len: 53 }, Box::new(|v| v["marks"] = json!([{"start": 31, "end": 28, "type": "strong"}]))),
            ("empty mark", BadMark { start: 28, end: 28, len: 53 }, Box::new(|v| v["marks"] = json!([{"start": 28, "end": 28, "type": "strong"}]))),
            ("mark over a newline", not("marks"), Box::new(|v| v["marks"] = json!([{"start": 14, "end": 15, "type": "emph"}, {"start": 28, "end": 31, "type": "strong"}]))),
            ("x- mark over a newline", not("marks"), Box::new(|v| v["marks"] = json!([{"start": 14, "end": 15, "type": "x-a"}, {"start": 28, "end": 31, "type": "strong"}]))),
            ("mark across a block", not("marks"), Box::new(|v| v["marks"] = json!([{"start": 10, "end": 20, "type": "strong"}]))),
            ("marks that touch", not("marks"), Box::new(|v| v["marks"] = json!([{"start": 28, "end": 29, "type": "strong"}, {"start": 29, "end": 31, "type": "strong"}]))),
            ("marks out of order", not("marks"), Box::new(|v| v["marks"] = json!([{"start": 28, "end": 31, "type": "strong"}, {"start": 15, "end": 16, "type": "emph"}]))),
            ("anchors out of order", not("anchors"), Box::new(|v| v["anchors"] = json!([{"id": "c-7f3a", "start": 23, "end": 27}, {"id": "c-1", "start": 0, "end": 2}]))),
            ("unused container", UnusedContainer { id: "q9".into() }, Box::new(|v| v["containers"].as_array_mut().unwrap().push(json!({"id": "q9", "kind": "quote"})))),
            ("containers out of order", not("containers"), Box::new(|v| v["containers"].as_array_mut().unwrap().swap(1, 2))),
            ("head at its default", not("island props"), Box::new(|v| v["islands"][0]["props"]["head"] = json!(1))),
            ("head past the rows", not("island props"), Box::new(|v| v["islands"][0]["props"]["head"] = json!(9))),
            ("align at none", not("island props"), Box::new(|v| v["islands"][0]["props"]["aligns"] = json!({"k1": "none"}))),
            ("untrimmed tex", not("island props"), Box::new(move |v| {
                v["text"] = json!("Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\u{FFFC}\n\u{FFFC}");
                v["islands"].as_array_mut().unwrap().insert(0, equation(" x ", json!(true)));
            })),
            ("display at its default", not("island props"), Box::new(move |v| {
                v["text"] = json!("Quarterly memo\nRevenue grew 12%.\nShip v2\nDraft spec\u{FFFC}\n\u{FFFC}");
                v["islands"].as_array_mut().unwrap().insert(0, equation("x", json!(false)));
            })),
            ("a cell's marks", not("marks"), Box::new(|v| {
                v["islands"][0]["slots"]["s1"]["marks"] = json!([{"start": 3, "end": 7, "type": "emph"}, {"start": 0, "end": 3, "type": "emph"}]);
            })),
        ];
        for (name, refused, edit) in cases {
            let mut v = memo();
            edit(&mut v);
            assert_eq!(Content::from_value(&v), Err(ParseError::Invalid(refused)), "{name}");
            let read = Content::from_value_lenient(&v).unwrap();
            assert!(read.rewritten, "{name}");
            assert_eq!(Content::from_value(&read.content.to_value()).as_ref(), Ok(&read.content), "{name}");
        }
        let read = Content::from_value_lenient(&memo()).unwrap();
        assert_eq!((read.content.to_value(), read.rewritten), (sort_keys_owned(memo()), false));
    }

    #[test]
    fn a_v1_value_migrates() {
        let v1 = json!({
            "text": "one\ntwo\nthree",
            "lines": [
                {"kind": "para", "containers": [{"container": "list_item", "attrs": {"ordered": true, "start": 3, "ordinal": 0}}]},
                {"kind": "para", "continues": true, "containers": [{"container": "list_item", "attrs": {"ordered": true, "start": 3, "ordinal": 0}}]},
                {"kind": "para", "containers": [{"container": "list_item", "attrs": {"ordered": true, "start": 3, "ordinal": 1}}]}
            ],
            "marks": [{"start": 0, "end": 3, "type": "anchor", "attrs": {"id": "c1"}}],
            "islands": []
        });
        let c = Content::from_value(&v1).unwrap();
        assert_eq!(c.text, "one\u{2028}two\nthree");
        assert_eq!(c.blocks.len(), 2);
        assert_eq!(c.blocks[0].path[0], c.blocks[1].path[0]);
        assert_ne!(c.blocks[0].path[1], c.blocks[1].path[1]);
        assert_eq!(c.anchors, [Anchor::new("c1", 0, 3)]);
        assert!(c.marks.is_empty());
        assert_eq!(Content::from_value(&v1).unwrap().to_json(), c.to_json());
    }
}
