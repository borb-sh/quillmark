//! The canonical JSON form: every object's keys in ascending order, a key
//! absent where its value is empty, so equal content serializes to equal
//! bytes. A value spelling `lines` is a v1 value: it decodes as the line form
//! and migrates.

use crate::ids::is_id;
use crate::lines::island::IslandType;
use crate::lines::model::{is_font_family_name, sort_keys_owned};
use crate::lines::serial as v1;
use crate::model::{
    Anchor, Block, BlockKind, Container, ContainerKind, Content, Invariant, Island, Mark, MarkKind, Pos,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Why a value did not decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Shape(String),
    Json(String),
    UnknownName { axis: &'static str, name: String },
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
        serde_json::to_string(&self.to_value()).unwrap_or_default()
    }

    /// Decode, normalize and validate. A v1 value migrates.
    pub fn from_value(v: &Value) -> Result<Content, ParseError> {
        if crate::model::json_too_deep(v) {
            return Err(ParseError::Invalid(Invariant::JsonTooDeep {
                what: "content",
                max: crate::MAX_JSON_DEPTH,
            }));
        }
        let mut c = if is_v1(v) {
            let lines = v1::from_canonical_value(v).map_err(ParseError::Legacy)?;
            crate::migrate::from_lines(&lines)
        } else {
            decode(v)?
        };
        c.normalize();
        c.validate()?;
        Ok(c)
    }

    pub fn from_json(s: &str) -> Result<Content, ParseError> {
        let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
        Content::from_value(&v)
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
    Ok(Content { text, blocks, containers, marks, anchors, islands })
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

    #[test]
    fn a_container_no_block_names_is_dropped() {
        let mut v = memo();
        v["containers"].as_array_mut().unwrap().push(json!({"id": "q9", "kind": "quote"}));
        let c = Content::from_value(&v).unwrap();
        assert!(c.container("q9").is_none());
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
