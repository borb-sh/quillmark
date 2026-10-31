//! Canonical JSON serialization: the freeze.
//!
//! Byte-deterministic within this schema: equal [`Content`] values (by
//! `PartialEq` after [`Content::normalize`]) serialize to byte-equal JSON,
//! insensitive to the order marks/islands were discovered in. Three order
//! sources are closed here and in `normalize`: mark order (canonical sort),
//! island order (slot position), and object-key order inside island `props`
//! (recursively sorted).
//!
//! Two fixed points, and they are not the same promise. **Bytes**:
//! `to_canonical_json(from_canonical_json(b)) == b` for canonical `b`, what a
//! consumer hashing stored documents spends. **Values**:
//! `from_canonical_json(to_canonical_json(rt)) == rt` for a normalized `rt`,
//! which holds only while every discriminator's encoding is injective. An axis
//! can keep the first and lose the second: a value that encodes to some *other*
//! value's bytes moves nothing on disk and still fails its own round trip.
//!
//! Storage, the render seam and the binding seam carry one canonical form.

use crate::model::{
    canonicalize_keys, is_font_family_name, Container, Island, Line, LineKind, Mark,
    MarkKind, Content, Normalized, Usv,
};
use serde_json::{Map, Value};
use std::borrow::Cow;

/// Why canonical-JSON parsing failed. Structural only: a well-formed producer
/// (this crate's serializer, storage, a binding) never trips these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Top-level JSON was not an object, or a required key was missing/mistyped.
    Shape(&'static str),
    /// The JSON itself did not parse.
    Json(String),
    /// The value parsed but violates a content invariant.
    Invalid(crate::model::Invariant),
    /// A discriminator outside its vocabulary. `axis` names the wire field
    /// (`line kind`, `container`, `mark type`, `island type`),
    /// `name` the value it carried.
    UnknownName {
        axis: &'static str,
        name: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Shape(s) => write!(f, "content json shape: {s}"),
            ParseError::Json(s) => write!(f, "content json parse: {s}"),
            ParseError::Invalid(inv) => write!(f, "content invariant: {inv:?}"),
            ParseError::UnknownName { axis, name } => {
                write!(f, "content vocabulary: unknown {axis} {name:?}")
            }
        }
    }
}
impl std::error::Error for ParseError {}

impl Normalized {
    /// Serialize to canonical JSON bytes. Every object key comes out in
    /// ascending order at every depth, so the bytes do **not** depend on
    /// `serde_json`'s `preserve_order` feature in the consumer's crate graph.
    ///
    /// On [`Normalized`] for the reason that token exists: canonical bytes need
    /// canonical input, and the mint is where a caller's mark/island order
    /// settles.
    pub fn to_canonical_json(&self) -> String {
        to_canonical_value(self).to_string()
    }
}

impl Content {
    /// Parse canonical JSON, normalize, and validate. Returns
    /// [`ParseError::Invalid`] for a content that violates its invariants, so
    /// storage cannot silently round-trip a malformed value.
    pub fn from_canonical_json(s: &str) -> Result<Normalized, ParseError> {
        let v: Value = serde_json::from_str(s).map_err(|e| ParseError::Json(e.to_string()))?;
        from_canonical_value(&v)
    }

    fn to_value(&self) -> Value {
        let mut root = Map::new();
        root.insert(
            "islands".into(),
            Value::Array(self.islands.iter().map(island_to_value).collect()),
        );
        root.insert(
            "lines".into(),
            Value::Array(self.lines.iter().map(line_to_value).collect()),
        );
        root.insert(
            "marks".into(),
            Value::Array(self.marks.iter().map(mark_to_value).collect()),
        );
        root.insert("text".into(), Value::String(self.text.clone()));
        Value::Object(root)
    }

    fn from_value(v: &Value) -> Result<Content, ParseError> {
        let obj = v.as_object().ok_or(ParseError::Shape("root not object"))?;
        let text = obj
            .get("text")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("text"))?
            .to_string();
        let lines = arr(obj, "lines")?
            .iter()
            .map(line_from_value)
            .collect::<Result<_, _>>()?;
        let marks = arr(obj, "marks")?
            .iter()
            .map(mark_from_value)
            .collect::<Result<_, _>>()?;
        let islands = arr(obj, "islands")?
            .iter()
            .map(island_from_value)
            .collect::<Result<_, _>>()?;
        Ok(Content {
            text,
            lines,
            marks,
            islands,
        })
    }
}

/// The canonical form as a structural [`Value`]: the recursively key-sorted
/// tree [`Normalized::to_canonical_json`] renders to bytes. A storage layer
/// embeds this as a nested object rather than an escaped string; serializing it
/// with `serde_json` is byte-identical to that JSON, independent of the
/// consumer's `preserve_order` feature.
pub fn to_canonical_value(rt: &Normalized) -> Value {
    let mut v = rt.to_value();
    // Scans and returns: the encoders emit their fixed keys in ascending order,
    // and every opaque bag under them was canonicalized by the mint. A key
    // inserted out of order is still repaired here, so the freeze holds without
    // the encoders having to be trusted for it.
    canonicalize_keys(&mut v);
    v
}

/// One map's own keys in ascending order, its values untouched. Shallow because
/// every value below is a scalar, an array of encoder objects that sorted
/// themselves, or a bag [`Content::normalize`] already canonicalized.
fn sort_own_keys(m: Map<String, Value>) -> Map<String, Value> {
    let mut entries: Vec<(String, Value)> = m.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.into_iter().collect()
}

/// Parse the canonical content form from a structural [`Value`], normalize, and
/// validate: the [`Value`]-input counterpart to
/// [`Content::from_canonical_json`].
pub fn from_canonical_value(v: &Value) -> Result<Normalized, ParseError> {
    seal(Content::from_value(v)?)
}

/// `v` with the retired spellings the decoder reads permanently written as the
/// encoder writes them: an island's `loss` key dropped, a line's `island` kind
/// spelled `para`, and a table's props settled as the mint settles them, a
/// `headless: true` or a span past what it covers among them. A value holding a
/// `term` line or a `description` or `element` container, whose drop can move
/// every line after it, is the canonical spelling of what it decodes to. A
/// caller comparing stored bytes against [`to_canonical_value`] compares this
/// instead, so an untagged content value in a retired spelling reads as content.
pub fn respell_retired(v: &Value) -> Value {
    let retired = arr_or_empty(v, "lines").iter().any(|line| {
        line.get("kind").and_then(Value::as_str) == Some("term")
            || arr_or_empty(line, "containers")
                .iter().any(|c| matches!(c.get("container").and_then(Value::as_str), Some("description" | "element")))
    });
    if retired && let Ok(decoded) = from_canonical_value(v) {
        return to_canonical_value(&decoded);
    }
    let mut v = v.clone();
    if let Some(islands) = v.get_mut("islands").and_then(Value::as_array_mut) {
        for island in islands.iter_mut().filter_map(Value::as_object_mut) {
            island.shift_remove("loss");
            if island.get("type").and_then(Value::as_str) == Some(crate::island::IslandType::Table.as_str())
                && let Some(props) = island.get_mut("props")
            {
                respell_table(props);
            }
        }
    }
    if let Some(lines) = v.get_mut("lines").and_then(Value::as_array_mut) {
        for line in lines.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(kind) = line.get_mut("kind").filter(|k| k.as_str() == Some("island")) {
                *kind = Value::String("para".into());
            }
        }
    }
    v
}

/// `props` settled as the mint settles a table's, where that changes what it
/// holds; props the mint keeps as they are stay in their stored key order, so
/// a comparison still sees keys out of order.
fn respell_table(props: &mut Value) {
    let stored = crate::model::sort_keys_owned(props.clone());
    let mut settled = stored.clone();
    normalize_table_props(&mut settled);
    let settled = crate::model::sort_keys_owned(settled);
    if settled != stored {
        *props = settled;
    }
}

/// Mint and check: the tail both wire lanes share, after the authored lane has
/// read its own refusals off the decode.
fn seal(rt: Content) -> Result<Normalized, ParseError> {
    let rt = rt.into_normalized();
    rt.validate().map_err(ParseError::Invalid)?;
    Ok(rt)
}

/// Read an opaque payload bag (`attrs`, `props`) off the wire, absent → `Null`.
///
/// **Depth-checked before the clone.** `Value::clone` spends a frame per level,
/// so an over-deep bag has to be refused while it is still borrowed from the
/// caller's `Value`: once owned the frames are already spent, and dropping it
/// spends them again.
fn bag_from_wire(
    o: &Map<String, Value>,
    key: &'static str,
    what: &'static str,
) -> Result<Value, ParseError> {
    let Some(v) = o.get(key) else {
        return Ok(Value::Null);
    };
    crate::model::check_json_depth(v, what).map_err(ParseError::Invalid)?;
    Ok(v.clone())
}

/// Read a wire position as a [`Usv`] index. **Checked**, not `as usize`: the
/// deployment target is wasm32, where the truncating cast turns `2^32 + 5` into
/// an in-range `5`, landing a mark at the wrong position instead of rejecting
/// the document.
pub(crate) fn usv_from(v: Option<&Value>, what: &'static str) -> Result<Usv, ParseError> {
    let n = v.and_then(Value::as_u64).ok_or(ParseError::Shape(what))?;
    Usv::try_from(n).map_err(|_| ParseError::Shape(what))
}

fn arr<'a>(obj: &'a Map<String, Value>, key: &'static str) -> Result<&'a Vec<Value>, ParseError> {
    obj.get(key)
        .and_then(Value::as_array)
        .ok_or(ParseError::Shape(key))
}

/// `v` as a slice, empty when it is not an array: the lenient counterpart to
/// [`arr`], since [`from_canonical_value`] owns the shape errors.
fn as_slice(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

fn arr_or_empty<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).map(as_slice).unwrap_or_default()
}

// The `@0.93.0` payload keys, per built-in name: the spelling that put a
// built-in's payload in named siblings. One frozen table, read from both sides —
// [`payload`] falls back to exactly the keys [`reject_legacy_siblings`] refuses
// — so a key a later promotion adds is in neither, and the two spellings stay
// split by release rather than by which names a build knows.

/// The `@0.93.0` payload keys of a built-in `kind`.
fn legacy_line_kind_keys(tag: &str) -> &'static [&'static str] {
    match tag {
        "heading" => &["level"],
        "code" => &["lang"],
        _ => &[],
    }
}

/// The `@0.93.0` payload keys of a built-in `container`. `instance` is not among
/// them: it was an envelope key then and stays one.
fn legacy_container_keys(tag: &str) -> &'static [&'static str] {
    match tag {
        "list_item" => &["ordered", "ordinal", "start"],
        _ => &[],
    }
}

/// The `@0.93.0` payload keys of a built-in mark `type`. `start`/`end` are not
/// among them: they are the mark's own envelope.
fn legacy_mark_keys(ty: &str) -> &'static [&'static str] {
    match ty {
        "link" => &["url"],
        "anchor" => &["id"],
        _ => &[],
    }
}

/// One entry of a built-in's payload bag: `attrs.<key>`, or the named sibling
/// `<key>` where there is no bag and `key` is one of `legacy` — the `@0.93.0`
/// spelling, which storage still holds and no migration reaches.
///
/// Unambiguous because a bag's presence is a pure function of the value: a
/// built-in carrying a payload always writes one, so an absent bag means either
/// the sibling spelling or an empty payload, and those agree on every key. An
/// empty bag is an absent one here as it is everywhere else, so the two
/// spellings of "no payload" cannot answer differently.
fn payload<'a>(o: &'a Map<String, Value>, legacy: &[&str], key: &'static str) -> Option<&'a Value> {
    match o.get("attrs").filter(|a| !crate::model::is_empty_bag(a)) {
        Some(attrs) => attrs.get(key),
        None if legacy.contains(&key) => o.get(key),
        None => None,
    }
}

/// Write a payload bag into an encoder's own map, omitting an empty one:
/// presence is a pure function of the value, the rule `continues: false`
/// already follows.
fn insert_attrs(m: &mut Map<String, Value>, attrs: Cow<'_, Value>) {
    if !crate::model::is_empty_bag(&attrs) {
        m.insert("attrs".into(), attrs.into_owned());
    }
}

/// Encode a [`LineKind`] into its canonical `kind` object (`{"kind":"para"}`,
/// `{"attrs":{"level":n},"kind":"heading"}`, …).
pub fn line_kind_to_value(kind: &LineKind) -> Value {
    Value::Object(line_kind_fields(kind))
}

/// The same fields unwrapped, for [`line_to_value`], which flattens them beside
/// a line's own keys.
fn line_kind_fields(kind: &LineKind) -> Map<String, Value> {
    // One arm for the whole vocabulary: the tag *is* the discriminator and the
    // payload rides the `attrs` bag whether or not this build knows the role, so
    // a reader lacking it carries the line whole and the build that gains it
    // reads what the reader wrote.
    let mut m = Map::new();
    insert_attrs(&mut m, kind.attrs());
    m.insert("kind".into(), Value::String(kind.tag().to_string()));
    m
}

/// Decode a [`LineKind`] from an object carrying the canonical `kind` fields.
pub fn line_kind_from_value(v: &Value) -> Result<LineKind, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line"))?;
    // A missing/non-string `kind` is a shape error; a string outside the
    // vocabulary is `UnknownName`. A non-string is not a name.
    let tag = o
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("line kind"))?;
    let legacy = legacy_line_kind_keys(tag);
    match tag {
        "heading" => {
            let level = payload(o, legacy, "level")
                .and_then(Value::as_u64)
                .ok_or(ParseError::Shape("heading level"))?;
            if !(1..=6).contains(&level) {
                return Err(ParseError::Shape("heading level"));
            }
            Ok(LineKind::Heading { level: level as u8 })
        }
        "code" => Ok(LineKind::Code {
            lang: payload(o, legacy, "lang")
                .and_then(Value::as_str)
                .map(crate::import::sanitize_lang)
                .filter(|l| !l.is_empty()),
        }),
        // No encoder writes `island`; the decoder reads it because the
        // content encoding is not migratable — a `richtext` field and a `$seed`
        // overlay rest inside opaque payload values carrying no schema tag, so
        // no hop reaches them (DOCUMENT_STORAGE.md § "Adding a Schema Version").
        // A block island's line is a `Para` whose sole content is its slot, so
        // the name and the kind it decodes to spell one line.
        "island" | "para" => Ok(LineKind::Para),
        "rule" => Ok(LineKind::Rule),
        // A definition list's term reads as the paragraph it held, for the
        // reason `island` reads above.
        "term" => Ok(LineKind::Para),
        other => Err(ParseError::UnknownName {
            axis: "line kind",
            name: other.to_string(),
        }),
    }
}

fn line_to_value(line: &Line) -> Value {
    let mut m = line_kind_fields(&line.kind);
    m.insert(
        "containers".into(),
        Value::Array(line.containers.iter().map(container_to_value).collect()),
    );
    // Omitted when false: presence is a pure function of the value, so the
    // encoding stays deterministic.
    if line.continues {
        m.insert("continues".into(), Value::Bool(true));
    }
    // `line_kind_fields` merges `kind` in ahead of `containers`/`continues`,
    // which sort before it, so this one encoder cannot settle its order at the
    // insert.
    Value::Object(sort_own_keys(m))
}

fn line_from_value(v: &Value) -> Result<Line, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line"))?;
    let kind = line_kind_from_value(v)?;
    let mut containers = Vec::new();
    // A dropped container's run still parts the runs below it: its identity
    // folds into the instance of the next container kept, so two adjacent
    // lists each inside their own element stay two.
    let mut dropped: Option<u64> = None;
    for c in o
        .get("containers")
        .and_then(Value::as_array)
        .ok_or(ParseError::Shape("containers"))?
    {
        let instance = c.get("instance").and_then(Value::as_u64).unwrap_or(0);
        match container_from_value(c)? {
            None => dropped = Some(fold_run(dropped.unwrap_or(0), instance)),
            Some(mut kept) => {
                if let Some(run) = dropped.take() {
                    kept.set_instance(fold_run(run, kept.instance()));
                }
                containers.push(kept);
            }
        }
    }
    let continues = o.get("continues").and_then(Value::as_bool).unwrap_or(false);
    Ok(Line {
        kind,
        containers,
        continues,
    })
}

/// One instance from a dropped run's identity and the instance beneath it,
/// distinct from the instance alone so the run's edge survives the drop.
fn fold_run(run: u64, instance: u64) -> u64 {
    run.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29) ^ instance
}

/// Encode a [`Container`] into its canonical wire object. A zero `instance` is
/// omitted, so a row written before the field existed re-encodes byte for byte;
/// [`container_from_value`] reads an absent key as zero.
pub fn container_to_value(c: &Container) -> Value {
    let mut m = Map::new();
    insert_attrs(&mut m, c.attrs());
    m.insert("container".into(), Value::String(c.tag().to_string()));
    // Not payload: the discriminator that keeps two adjacent same-shape runs
    // apart is an envelope key, carried on every arm.
    if c.instance() != 0 {
        m.insert("instance".into(), Value::from(c.instance()));
    }
    Value::Object(m)
}

/// Decode a [`Container`] from its canonical wire object: `None` for a retired
/// container, a definition list's `description` or a `qm-*` `element`, whose
/// lines read as the blocks it wrapped, for the reason a `term` line reads as
/// a paragraph.
pub fn container_from_value(v: &Value) -> Result<Option<Container>, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("container"))?;
    let tag = o
        .get("container")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("container kind"))?;
    let instance = o.get("instance").and_then(Value::as_u64).unwrap_or(0);
    let legacy = legacy_container_keys(tag);
    match tag {
        "list_item" => Ok(Some(Container::ListItem {
            ordered: payload(o, legacy, "ordered")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            start: payload(o, legacy, "start")
                .and_then(Value::as_u64)
                .unwrap_or(1),
            ordinal: payload(o, legacy, "ordinal")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            checked: payload(o, legacy, "checked").and_then(Value::as_bool),
            instance,
        })),
        "quote" => Ok(Some(Container::Quote { instance })),
        "description" | "element" => Ok(None),
        other => Err(ParseError::UnknownName {
            axis: "container",
            name: other.to_string(),
        }),
    }
}

/// Encode a [`Mark`] (`start`, `end`, `type`, …) into its canonical wire object.
pub fn mark_to_value(mark: &Mark) -> Value {
    let mut m = Map::new();
    insert_attrs(&mut m, mark.kind.attrs());
    m.insert("end".into(), Value::from(mark.end));
    m.insert("start".into(), Value::from(mark.start));
    m.insert("type".into(), Value::String(mark.kind.tag().to_string()));
    Value::Object(m)
}

/// What every mark carries whatever its type.
struct MarkShape<'a> {
    fields: &'a Map<String, Value>,
    start: Usv,
    end: Usv,
    ty: &'a str,
}

/// A mark's fallible half: the prologue of [`mark_from_value`], where the
/// envelope is read and the range checked, before the `type` selects an arm.
fn mark_shape(v: &Value) -> Result<MarkShape<'_>, ParseError> {
    let fields = v.as_object().ok_or(ParseError::Shape("mark"))?;
    let start = usv_from(fields.get("start"), "mark start")?;
    let end = usv_from(fields.get("end"), "mark end")?;
    let ty = fields
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("mark type"))?;
    Ok(MarkShape {
        fields,
        start,
        end,
        ty,
    })
}

/// Decode a [`Mark`] from its canonical wire object.
pub fn mark_from_value(v: &Value) -> Result<Mark, ParseError> {
    let MarkShape {
        fields: o,
        start,
        end,
        ty,
    } = mark_shape(v)?;
    let legacy = legacy_mark_keys(ty);
    let kind = match ty {
        "strong" => MarkKind::Strong,
        "emph" => MarkKind::Emph,
        "underline" => MarkKind::Underline,
        "strike" => MarkKind::Strike,
        "code" => MarkKind::Code,
        "link" => MarkKind::Link {
            url: payload(o, legacy, "url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        "font_family" => MarkKind::FontFamily {
            name: payload(o, legacy, "name")
                .and_then(Value::as_str)
                .filter(|name| is_font_family_name(name))
                .ok_or(ParseError::Shape("font_family name"))?
                .to_string(),
        },
        "anchor" => MarkKind::Anchor {
            id: payload(o, legacy, "id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        other => {
            return Err(ParseError::UnknownName {
                axis: "mark type",
                name: other.to_string(),
            })
        }
    };
    Ok(Mark { start, end, kind })
}

// Authored-lane readers, strict about the payload spelling.
//
// [`payload`] reads a built-in's payload from a named sibling where there is no
// bag, so `{"type": "link", "url": "…"}` decodes. The two lanes answer that
// oppositely. Storage (`Content::from_canonical_json`) stays lenient: content
// stored in that spelling is beyond any migration's reach, a `richtext` field
// resting as a content object under no schema tag. Authored (the `crate::ops`
// wire, and `overwrite` through [`from_authored_value`]) rejects it, the shape
// meaning a stale copy of the encoding.
//
// The rule is narrow on purpose: a *legacy payload key* beside the name that
// spelled it, nothing else, off the same frozen table [`payload`] reads.

/// [`line_kind_from_value`] for the authored lane: a legacy payload sibling, or
/// a `lang` the storage decode would reduce, is a shape error rather than a
/// silent repair.
pub(crate) fn line_kind_from_authored_value(v: &Value) -> Result<LineKind, ParseError> {
    reject_line_kind_legacy(v)?;
    reject_unwritable_lang(v)?;
    line_kind_from_value(v)
}

/// A `lang` [`crate::import::sanitize_lang`] would change is a shape error: the
/// emitter writes it onto the fence header unquoted, so the storage lane's
/// reduction of it is a repair the host did not ask for.
fn reject_unwritable_lang(v: &Value) -> Result<(), ParseError> {
    let Some(o) = v.as_object() else {
        return Ok(());
    };
    if o.get("kind").and_then(Value::as_str) != Some("code") {
        return Ok(());
    }
    let Some(lang) = payload(o, legacy_line_kind_keys("code"), "lang").and_then(Value::as_str)
    else {
        return Ok(());
    };
    if crate::import::sanitize_lang(lang) != lang {
        return Err(ParseError::Shape("code lang"));
    }
    Ok(())
}

/// [`container_from_value`] for the authored lane. See
/// [`line_kind_from_authored_value`].
pub(crate) fn container_from_authored_value(v: &Value) -> Result<Container, ParseError> {
    reject_container_legacy(v)?;
    container_from_value(v)?.ok_or_else(|| ParseError::UnknownName {
        axis: "container",
        name: v.get("container").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// [`mark_from_value`] for the authored lane. See [`line_kind_from_authored_value`].
pub(crate) fn mark_from_authored_value(v: &Value) -> Result<Mark, ParseError> {
    reject_mark_legacy(v)?;
    mark_from_value(v)
}

/// The authored lane's verdict on a mark, without building it. Table cells are
/// the only caller: [`parse_cell`] reads their marks leniently, so a cell mark
/// reaches no strict decode that would raise the error on its own.
pub(crate) fn reject_unreadable_mark(v: &Value) -> Result<(), ParseError> {
    reject_mark_legacy(v)?;
    reject_unwritable_link_url(v)?;
    mark_from_value(v)?;
    Ok(())
}

/// [`island_from_value`] for the authored lane: an image `url` the markdown
/// projection cannot write is a shape error rather than a silent rewrite, the
/// rule [`line_kind_from_authored_value`] carries for a code fence's `lang`.
pub(crate) fn island_from_authored_value(v: &Value) -> Result<Island, ParseError> {
    reject_unwritable_image_url(v)?;
    reject_unwritable_tex(v)?;
    island_from_value(v)
}

/// A link `url` [`crate::export::url_is_writable`] refuses is a shape error: the
/// emitter writes it into the destination slot of `[…](…)`, which admits no line
/// ending, so the export's percent-encoding of it is a rewrite the host did not
/// ask for.
///
/// The rule is on the lanes that **store** a url. An op naming a mark already in
/// the field matches on kind equality, and a url the model holds has no other
/// spelling, so refusing it there would leave a legacy link unremovable.
pub(crate) fn reject_unwritable_link_url(v: &Value) -> Result<(), ParseError> {
    let Some(o) = v.as_object() else {
        return Ok(());
    };
    if o.get("type").and_then(Value::as_str) != Some("link") {
        return Ok(());
    }
    reject_unwritable_url(
        payload(o, legacy_mark_keys("link"), "url").and_then(Value::as_str),
        "link url",
    )
}

/// [`reject_unwritable_link_url`] on an `image` island's `url` prop, which the
/// emitter writes into the same slot.
fn reject_unwritable_image_url(v: &Value) -> Result<(), ParseError> {
    let image = crate::island::IslandType::Image.as_str();
    if v.get("type").and_then(Value::as_str) != Some(image) {
        return Ok(());
    }
    reject_unwritable_url(
        v.get("props")
            .and_then(|p| p.get("url"))
            .and_then(Value::as_str),
        "image url",
    )
}

/// A `math` island whose `tex` the markdown projection cannot write between
/// its delimiters ([`crate::export::tex_is_writable`]), read as it rests once
/// settled. A `tex` that is no string is the props carrier's to hold.
fn reject_unwritable_tex(v: &Value) -> Result<(), ParseError> {
    if v.get("type").and_then(Value::as_str) != Some(crate::island::IslandType::Math.as_str()) {
        return Ok(());
    }
    let props = v.get("props");
    let display = props.and_then(|p| p.get("display")) == Some(&Value::Bool(true));
    match props.and_then(|p| p.get("tex")).and_then(Value::as_str) {
        Some(tex) if !crate::export::tex_is_writable(&settle_tex(tex), display) => {
            Err(ParseError::Shape("math tex"))
        }
        _ => Ok(()),
    }
}

fn reject_unwritable_url(url: Option<&str>, err: &'static str) -> Result<(), ParseError> {
    match url {
        Some(u) if !crate::export::url_is_writable(u) => Err(ParseError::Shape(err)),
        _ => Ok(()),
    }
}

/// [`from_canonical_value`] for a content the **host authored just now**: the
/// `overwrite` input, not a blob read back from storage. Same decode, plus the
/// legacy-spelling rule on every axis the decode reads — line kinds,
/// containers, prose marks, table-cell marks — and the writability rule on the
/// urls the projection spells.
pub fn from_authored_value(v: &Value) -> Result<Normalized, ParseError> {
    authored_lane_scan(v)?;
    seal(Content::from_value(v)?)
}

/// The authored-lane scan [`from_authored_value`] runs. Structural rather than a
/// blind recursive walk: an island's `props` is opaque host payload that may
/// legitimately contain an object spelled `{"type": "link", "url": …}`, and
/// rejecting that would make the carrier unable to carry.
fn authored_lane_scan(v: &Value) -> Result<(), ParseError> {
    for line in arr_or_empty(v, "lines") {
        reject_line_kind_legacy(line)?;
        for c in arr_or_empty(line, "containers") {
            reject_container_legacy(c)?;
            if container_from_value(c)?.is_none() {
                let name = c.get("container").and_then(Value::as_str).unwrap_or_default();
                return Err(ParseError::UnknownName {
                    axis: "container",
                    name: name.to_string(),
                });
            }
        }
    }
    // Both rules here: the strict decode `from_canonical_value` runs next reads
    // prose marks on the storage lane, so it carries neither.
    for m in arr_or_empty(v, "marks") {
        reject_mark_legacy(m)?;
        reject_unwritable_link_url(m)?;
    }
    // Cell marks ride the prose mark shape, so the rules follow them in, plus
    // the readability check, since no strict decode reaches them. Dispatch goes
    // through `IslandType`, so a new mark-carrying type is a compile error
    // here rather than a silent skip.
    for island in arr_or_empty(v, "islands") {
        let ty = island.get("type").and_then(Value::as_str).unwrap_or_default();
        match crate::island::IslandType::parse(ty) {
            Some(crate::island::IslandType::Table) => {
                let Some(props) = island.get("props") else {
                    continue;
                };
                for cell in table_cell_values(props) {
                    for m in arr_or_empty(cell, "marks") {
                        reject_unreadable_mark(m)?;
                    }
                }
            }
            Some(crate::island::IslandType::Footnote) => {
                if let Some(props) = island.get("props") {
                    arr_or_empty(props, "marks").iter().try_for_each(reject_unreadable_mark)?;
                }
            }
            // No cells; the one prop the projection writes is the url.
            Some(crate::island::IslandType::Image) => reject_unwritable_image_url(island)?,
            Some(crate::island::IslandType::Math) => reject_unwritable_tex(island)?,
            // This scan reads raw JSON; an unknown type is the decode's to refuse.
            None => {}
        }
    }
    Ok(())
}

fn reject_line_kind_legacy(v: &Value) -> Result<(), ParseError> {
    reject_legacy_siblings(v, "kind", legacy_line_kind_keys, "legacy kind payload")
}

fn reject_container_legacy(v: &Value) -> Result<(), ParseError> {
    reject_legacy_siblings(
        v,
        "container",
        legacy_container_keys,
        "legacy container payload",
    )
}

fn reject_mark_legacy(v: &Value) -> Result<(), ParseError> {
    reject_legacy_siblings(v, "type", legacy_mark_keys, "legacy mark payload")
}

/// Error when `v` spells a built-in's payload the `@0.93.0` way, as a named
/// sibling. A non-object or a missing/non-string discriminant is left to the
/// reader that follows, which reports the shape error in its own terms.
///
/// The storage lane reads that spelling; a host writing it *now* holds a stale
/// copy of the encoding, and the sibling it means is not necessarily the one
/// [`payload`] would read — a bag beside it wins, so the write would land
/// somewhere the host did not aim.
fn reject_legacy_siblings(
    v: &Value,
    discriminant: &str,
    legacy: fn(&str) -> &'static [&'static str],
    err: &'static str,
) -> Result<(), ParseError> {
    let Some(o) = v.as_object() else {
        return Ok(());
    };
    let Some(tag) = o.get(discriminant).and_then(Value::as_str) else {
        return Ok(());
    };
    if legacy(tag).iter().any(|k| o.contains_key(*k)) {
        return Err(ParseError::Shape(err));
    }
    Ok(())
}

/// Parse a table-cell object `{text, marks}` leniently: its plain text plus the
/// marks over it, their ranges USV offsets into that text and their wire shape
/// the one prose marks use. A malformed mark is skipped rather than failing.
/// Public so the typst emitter renders a cell through the same parse the codecs
/// use.
pub fn parse_cell(v: &Value) -> (String, Vec<Mark>) {
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let marks = v
        .get("marks")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|m| mark_from_value(m).ok()).collect())
        .unwrap_or_default();
    (text, marks)
}

/// Build a table-cell object `{marks, text}`.
pub(crate) fn cell_to_value(text: &str, marks: &[Mark]) -> Value {
    let mut m = Map::new();
    m.insert(
        "marks".into(),
        Value::Array(marks.iter().map(mark_to_value).collect()),
    );
    m.insert("text".into(), Value::String(text.to_string()));
    Value::Object(m)
}

/// Every cell object in a table island's props, header then each body row: the
/// undecoded half of [`table_cells`], walking the same cells in the same order.
pub(crate) fn table_cell_values(props: &Value) -> impl Iterator<Item = &Value> {
    let header = arr_or_empty(props, "header").iter();
    let rows = arr_or_empty(props, "rows")
        .iter()
        .flat_map(|row| as_slice(row).iter());
    header.chain(rows)
}

/// [`table_cell_values`], mutable.
pub(crate) fn table_cell_values_mut(props: &mut Value) -> Vec<&mut Value> {
    let Some(o) = props.as_object_mut() else {
        return Vec::new();
    };
    let mut cells = Vec::new();
    let (mut header, mut rows) = (None, None);
    for (key, value) in o.iter_mut() {
        match key.as_str() {
            "header" => header = Some(value),
            "rows" => rows = Some(value),
            _ => {}
        }
    }
    if let Some(Value::Array(header)) = header {
        cells.extend(header.iter_mut());
    }
    if let Some(Value::Array(rows)) = rows {
        for row in rows.iter_mut() {
            if let Value::Array(row) = row {
                cells.extend(row.iter_mut());
            }
        }
    }
    cells
}

/// Set a cell object's marks, its text and every other key kept.
pub(crate) fn set_cell_marks(cell: &mut Value, marks: &[Mark]) {
    let (text, _) = parse_cell(cell);
    match (cell.as_object_mut(), cell_to_value(&text, marks)) {
        (Some(o), Value::Object(fields)) => o.extend(fields),
        (_, canon) => *cell = canon,
    }
}

/// Every cell's `(text, marks)` in a table island's props: header then each
/// body row, in order. For [`Content::validate`]'s cell-mark invariant checks.
pub(crate) fn table_cells(props: &Value) -> Vec<(String, Vec<Mark>)> {
    table_cell_values(props).map(parse_cell).collect()
}

// The `table` serializer below is the primitive `crate::island` dispatches into for
// `IslandType::Table`; island-type dispatch itself lives there.

/// Repair a table island's props in place to the canonical shape:
///
/// - **One column count.** `cols` is the widest of the header, any body row, and
///   `aligns`; the header, each row, and `aligns` are padded up to it (padding
///   only grows: no cell is ever truncated). Materializing the count into the
///   header means the markdown projection (header-derived) and the Typst
///   projection (widest-row) agree on one number.
/// - **One break char.** A `\n` in a cell's text is a line break; a `\r` or a
///   [`crate::normalize::is_line_separator`] char becomes a space. A 1:1
///   replacement keeps char offsets stable, so the cell's marks stay in range.
/// - **Canonical cell marks.** Each cell's marks are re-normalized (formatting
///   edges trimmed off `\n` as prose's are, sort, same-kind union, drop
///   zero-width) so equal cells serialize to equal bytes.
/// - **Arrays where arrays belong.** A present non-array `header`, `aligns`, or
///   row carries no cells, so it becomes an empty array.
/// - **Layout keys absent at their default.** See [`normalize_table_layout`].
/// - **Spans that cover only what they can.** See [`normalize_table_spans`].
pub(crate) fn normalize_table_props(props: &mut Value) {
    let cols = table_cols(props);
    let widths = crate::island::TableLayout::of(props).widths;
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let aligns = obj.entry("aligns").or_insert_with(|| Value::Array(vec![]));
    if !aligns.is_array() {
        *aligns = Value::Array(vec![]);
    }
    if let Some(a) = aligns.as_array_mut() {
        while a.len() < cols {
            a.push(Value::String("none".into()));
        }
    }
    let canon_row = |row: &mut Value| {
        pad_row(row, cols);
        if let Some(r) = row.as_array_mut() {
            r.iter_mut().for_each(canon_cell);
        }
    };
    let header = obj.entry("header").or_insert_with(|| Value::Array(vec![]));
    if !header.is_array() {
        *header = Value::Array(vec![]);
    }
    canon_row(header);
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            if !row.is_array() {
                *row = Value::Array(vec![]);
            }
            canon_row(row);
        }
    }
    normalize_table_layout(obj, widths, cols);
    normalize_table_spans(props);
}

/// A table's layout keys, each absent at its default: `widths` holding only
/// weights and `null`s settles to `cols`, padded with `null` or truncated, and
/// all `null` is absent; a whole-number `head` settles to at most the row
/// count, absent at 1. `headless: true` sits beside `head: 0`, which a 0.130
/// reader cannot read, and is absent beside any other `head`; a stored
/// `headless: true` settles to `head: 0` where `head` holds no whole number. A
/// value outside its set stays as written.
fn normalize_table_layout(obj: &mut Map<String, Value>, widths: Option<Vec<Option<u64>>>, cols: usize) {
    if let Some(mut widths) = widths {
        widths.resize(cols, None);
        match widths.iter().any(Option::is_some) {
            true => obj.insert("widths".into(), widths.into_iter().map(|w| w.map_or(Value::Null, Value::from)).collect()),
            false => obj.remove("widths"),
        };
    }
    let headless = obj.get("headless").and_then(Value::as_bool);
    if headless == Some(true) && obj.get("head").and_then(Value::as_u64).is_none() {
        obj.insert("head".into(), 0.into());
    }
    let rows = 1 + obj.get("rows").and_then(Value::as_array).map_or(0, Vec::len);
    let head = obj.get("head").and_then(Value::as_u64).map(|head| usize::try_from(head).unwrap_or(usize::MAX).min(rows));
    match head {
        Some(1) => obj.remove("head"),
        Some(head) => obj.insert("head".into(), head.into()),
        None => None,
    };
    match head {
        Some(0) => obj.insert("headless".into(), true.into()),
        _ if headless.is_some() => obj.remove("headless"),
        _ => None,
    };
}

/// Each drawn cell's `colspan` and `rowspan` settled to what it covers
/// ([`crate::island::table_spans`]), absent at 1, and each covered cell's span
/// keys absent. A covered cell keeps its alignment, which it draws again once
/// no span covers it. A value outside its set stays as written.
fn normalize_table_spans(props: &mut Value) {
    use crate::island::CELL_SPANS;
    let spans = crate::island::table_spans(props);
    let settle = |row: &mut Value, spans: &[Option<(usize, usize)>]| {
        let cells = row.as_array_mut().into_iter().flatten().zip(spans);
        for (cell, span) in cells {
            let Some(cell) = cell.as_object_mut() else { continue };
            let settled = span.map_or([None, None], |(w, h)| [Some(w), Some(h)]);
            for (key, n) in CELL_SPANS.into_iter().zip(settled) {
                if !cell.get(key).and_then(Value::as_u64).is_some_and(|n| n >= 1) {
                    continue;
                }
                match n {
                    Some(n) if n > 1 => cell.insert(key.into(), n.into()),
                    _ => cell.remove(key),
                };
            }
        }
    };
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let mut spans = spans.iter();
    if let (Some(header), Some(row)) = (obj.get_mut("header"), spans.next()) {
        settle(header, row);
    }
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for (row, spans) in rows.iter_mut().zip(spans) {
            settle(row, spans);
        }
    }
}

/// A table's canonical column count: the widest of its header, any body row, and
/// its `aligns` array. Padding (never truncation) brings every part up to it.
fn table_cols(props: &Value) -> usize {
    let arr_len = |k: &str| props.get(k).and_then(Value::as_array).map(|a| a.len());
    let header = arr_len("header").unwrap_or(0);
    let aligns = arr_len("aligns").unwrap_or(0);
    let widest_row = props
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| r.as_array().map(|a| a.len()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    header.max(aligns).max(widest_row)
}

/// The slots a table's grid holds once padded: its rows, the header among
/// them, by its [column count](table_cols).
pub(crate) fn table_slots(props: &Value) -> usize {
    let rows = props.get("rows").and_then(Value::as_array).map_or(0, Vec::len);
    rows.saturating_add(1).saturating_mul(table_cols(props))
}

/// Pad a cell array (header or body row) up to `cols` with empty cells. Never
/// shrinks: `cols` is the widest, so a shorter array only grows.
fn pad_row(v: &mut Value, cols: usize) {
    if let Some(arr) = v.as_array_mut() {
        while arr.len() < cols {
            arr.push(cell_to_value("", &[]));
        }
    }
}

/// Every char a downstream lexer reads as a line break other than `\n`, the
/// separators [`crate::normalize::is_line_separator`] names included. A cell's
/// one line break is `\n`.
fn is_cell_break(c: char) -> bool {
    c == '\r' || crate::normalize::is_line_separator(c)
}

/// Space a cell's stray line-break chars (1:1, so mark offsets hold) and
/// re-normalize its marks. Writes back into the cell's **own** object rather
/// than minting a fresh one, so a key this build does not recognize survives.
pub(crate) fn canon_cell(cell: &mut Value) {
    let (text, mut marks) = parse_cell(cell);
    let text = if text.contains(is_cell_break) {
        text.replace(is_cell_break, " ")
    } else {
        text
    };
    if text.contains('\n') {
        // Union before the trim: the pieces a sweep reopens across a `\n` meet
        // at it, and trimmed first they would part there.
        marks = crate::model::normalize_marks(marks);
        let chars: Vec<char> = text.chars().collect();
        crate::model::trim_marks_off_newlines(&chars, &mut marks);
    }
    let canon = cell_to_value(&text, &crate::model::normalize_marks(marks));
    match (cell.as_object_mut(), canon) {
        // Overwrite the canonical keys, leave the rest.
        (Some(o), Value::Object(fields)) => o.extend(fields),
        // A non-object cell holds no keys to preserve.
        (_, canon) => *cell = canon,
    }
}

/// Repair a footnote's props in place: [`canon_cell`], then the note's edge
/// line breaks dropped, a note opening or closing on one having nothing there
/// to break.
pub(crate) fn normalize_note_props(props: &mut Value) {
    canon_cell(props);
    let (text, marks) = parse_cell(props);
    if !text.starts_with('\n') && !text.ends_with('\n') {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    let lead = chars.iter().take_while(|&&c| c == '\n').count();
    let end = lead.max(chars.len() - chars.iter().rev().take_while(|&&c| c == '\n').count());
    let clip = |at: Usv| at.clamp(lead, end) - lead;
    let marks: Vec<Mark> = marks
        .into_iter()
        .map(|m| Mark::new(clip(m.start), clip(m.end), m.kind))
        .collect();
    let text: String = chars[lead..end].iter().collect();
    if let (Some(o), Value::Object(fields)) =
        (props.as_object_mut(), cell_to_value(&text, &crate::model::normalize_marks(marks)))
    {
        o.extend(fields);
    }
}

/// Repair an equation's props in place: `tex` settled by [`settle_tex`], and
/// `display: false` absent. A `tex` that is no string rides as written.
pub(crate) fn normalize_math_props(props: &mut Value) {
    let Some(o) = props.as_object_mut() else {
        return;
    };
    if let Some(Value::String(tex)) = o.get_mut("tex") {
        let settled = settle_tex(tex);
        if settled != *tex {
            *tex = settled;
        }
    }
    if o.get("display") == Some(&Value::Bool(false)) {
        o.remove("display");
    }
}

/// TeX source as an equation stores it: each `%` comment dropped to the end
/// of its line, each line break or other control char a space, which TeX
/// reads it as, and the whitespace at either edge dropped.
pub fn settle_tex(tex: &str) -> String {
    let mut uncommented = String::with_capacity(tex.len());
    let (mut comment, mut escaped) = (false, false);
    for c in tex.chars() {
        let ends_line = c == '\n' || c == '\r' || crate::normalize::is_line_separator(c);
        comment &= !ends_line;
        if !comment && c == '%' && !escaped {
            comment = true;
        }
        if !comment {
            uncommented.push(c);
        }
        escaped = c == '\\' && !escaped;
    }
    let spaced: String = uncommented
        .chars()
        .map(|c| if c.is_control() || crate::normalize::is_line_separator(c) { ' ' } else { c })
        .filter(|&c| c != crate::model::ISLAND_SLOT)
        .collect();
    spaced.trim().to_string()
}

pub(crate) fn island_to_value(island: &Island) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(island.id.clone()));
    m.insert("props".into(), island.props.clone());
    m.insert("type".into(), Value::String(island.island_type.as_str().into()));
    Value::Object(m)
}

pub(crate) fn island_from_value(v: &Value) -> Result<Island, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("island"))?;
    let name = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("island type"))?;
    let island_type =
        crate::island::IslandType::parse(name).ok_or_else(|| ParseError::UnknownName {
            axis: "island type",
            name: name.to_string(),
        })?;
    let props = bag_from_wire(o, "props", "island props")?;
    if island_type == crate::island::IslandType::Table {
        crate::model::check_table_cells(&props).map_err(ParseError::Invalid)?;
    }
    island_type.reject_unknown_cell_mark(&props)?;
    Ok(Island {
        id: o
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("island id"))?
            .to_string(),
        island_type,
        props,
    })
}

/// The mark vocabulary's verdict on an island's cells, for
/// [`crate::island::IslandType::reject_unknown_cell_mark`].
///
/// A cell mark reaches no other strict decode: [`parse_cell`] reads them
/// leniently and `canon_cell` writes the survivors back, so a name outside the
/// vocabulary would leave the stored bytes on a read with no edit rather than
/// refusing the row. A *malformed* cell mark stays skipped — that is the split
/// canon § "an unreadable table-cell mark" sets, and it is about shape, not
/// names.
pub(crate) fn reject_unknown_cell_mark_name<'a>(
    cells: impl IntoIterator<Item = &'a Value>,
) -> Result<(), ParseError> {
    for cell in cells {
        for m in arr_or_empty(cell, "marks") {
            if let Err(e @ ParseError::UnknownName { .. }) = mark_from_value(m) {
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    /// `instance` is written only where it is doing work, so it costs bytes only
    /// in the documents carrying an adjacent same-shape sibling. A spelled zero
    /// still decodes, which is what lets a producer holding an older read write
    /// it straight back.
    #[test]
    fn instance_is_written_only_where_it_works_and_a_spelled_zero_still_decodes() {
        let canonical = r#"{"islands":[],"lines":[{"containers":[{"container":"quote"}],"kind":"para"}],"marks":[],"text":"a"}"#;
        let spelled = r#"{"islands":[],"lines":[{"containers":[{"container":"quote","instance":0}],"kind":"para"}],"marks":[],"text":"a"}"#;
        let rt = Content::from_canonical_json(canonical).expect("decodes");
        assert_eq!(rt.to_canonical_json(), canonical);
        assert_eq!(Content::from_canonical_json(spelled).expect("decodes"), rt);

        // Two adjacent one-item lists: the shape that spends the key.
        let two = r#"{"islands":[],"lines":[{"containers":[{"attrs":{"ordered":false,"ordinal":0,"start":1},"container":"list_item"}],"kind":"para"},{"containers":[{"attrs":{"ordered":false,"ordinal":0,"start":1},"container":"list_item","instance":1}],"kind":"para"}],"marks":[],"text":"a\nb"}"#;
        let rt = Content::from_canonical_json(two).expect("decodes");
        assert_eq!(rt.to_canonical_json(), two, "byte layout moved");
    }

    use super::*;
    use crate::island::IslandType;
    use crate::model::{Invariant, Line, LineKind};

    fn sample() -> Content {
        Content {
            text: "hello world".into(),
            lines: vec![Line {
                kind: LineKind::Para,
                containers: vec![],
                continues: false,
            }],
            marks: vec![
                Mark {
                    start: 6,
                    end: 11,
                    kind: MarkKind::Strong,
                },
                Mark {
                    start: 0,
                    end: 5,
                    kind: MarkKind::Emph,
                },
            ],
            islands: vec![],
        }
    }

    /// Export recurses one frame per container, so a 20 000-deep path that
    /// decoded clean would abort the process on `to_markdown`.
    #[test]
    fn deep_container_nesting_is_rejected_at_decode() {
        let containers = vec![r#"{"container":"quote"}"#; 20_000].join(",");
        let json = format!(
            r#"{{"text":"hi","lines":[{{"kind":"para","containers":[{containers}]}}],"marks":[],"islands":[]}}"#
        );
        assert!(matches!(
            Content::from_canonical_json(&json),
            Err(ParseError::Invalid(Invariant::NestingTooDeep { .. }))
        ));
    }

    /// Build a `Value` nesting `depth` array levels, iteratively so *building*
    /// the fixture cannot overflow. Handling it still can (`Value`'s `Clone` and
    /// `Drop` both recurse), so the tests below probe just past the cap rather
    /// than at a depth that overflows the test itself.
    fn nested_arrays(depth: usize) -> Value {
        let mut v = Value::Null;
        for _ in 0..depth {
            v = Value::Array(vec![v]);
        }
        v
    }

    /// The string lane is bounded by its parser (`serde_json::from_str` refuses
    /// past 128); the `Value` lane is the host-authored one and has to refuse
    /// the same shape, since an unguarded deep `props` aborts the process.
    #[test]
    fn deep_json_payload_is_rejected_at_decode_on_the_value_lane() {
        let deep = nested_arrays(1_000);
        let cases: [(Value, &'static str); 1] = [(
            serde_json::json!({"text":"\u{fffc}","lines":[{"kind":"para","containers":[]}],
              "marks":[],"islands":[{"id":"i1","type":"image","props":deep}]}),
            "island props",
        )];
        for (v, what) in cases {
            assert_eq!(
                from_canonical_value(&v),
                Err(ParseError::Invalid(Invariant::JsonTooDeep {
                    what,
                    max: crate::MAX_JSON_DEPTH,
                })),
                "{what} accepted a 1 000-deep payload"
            );
            // The authored lane funnels through the same decode, so it refuses
            // the same shape rather than trapping on its own scan first.
            assert!(matches!(
                from_authored_value(&v),
                Err(ParseError::Invalid(Invariant::JsonTooDeep { .. }))
            ));
        }
    }

    /// The cap admits every payload a stored blob can carry, so closing the
    /// `Value` lane costs no stored population. Stated as the implication
    /// rather than an offset, `serde_json::from_str`'s own limit counting
    /// from the document root rather than from the bag.
    #[test]
    fn json_depth_cap_admits_every_storable_payload() {
        let content = |props: Value| {
            serde_json::json!({"text":"\u{fffc}","lines":[{"kind":"para","containers":[]}],
              "marks":[],"islands":[{"id":"i1","type":"image","props":props}]})
        };
        assert!(from_canonical_value(&content(nested_arrays(crate::MAX_JSON_DEPTH))).is_ok());
        assert!(from_canonical_value(&content(nested_arrays(crate::MAX_JSON_DEPTH + 1))).is_err());

        // Across the whole boundary region, string-lane-accepted implies
        // `Value`-lane-accepted. The converse does not hold and need not: the
        // string lane's root-relative count refuses a few depths the bag cap
        // allows.
        let mut storable = 0;
        for d in 1..=crate::MAX_JSON_DEPTH + 8 {
            let v = content(nested_arrays(d));
            if Content::from_canonical_json(&v.to_string()).is_ok() {
                storable = d;
                assert!(
                    from_canonical_value(&v).is_ok(),
                    "the bag cap refused a {d}-deep props the string lane accepts"
                );
            }
        }
        assert!(
            storable > 0 && storable <= crate::MAX_JSON_DEPTH,
            "string lane's deepest storable props was {storable}"
        );
    }

    /// The depth cap guards what the decode **retains**. A bag beside a built-in
    /// that carries no payload is read by nobody and cloned by nobody, so it
    /// costs no frames and needs no verdict: it drops with the caller's own
    /// `Value`, which spends the frames it was always going to spend.
    #[test]
    fn a_deep_bag_no_arm_reads_is_dropped_rather_than_refused() {
        let mut deep = Value::Null;
        for _ in 0..1_000 {
            deep = serde_json::json!({"a": deep});
        }
        let line = |kind: &str| {
            serde_json::json!({"text":"x","lines":[{"kind":kind,"containers":[],"attrs":deep}],
              "marks":[],"islands":[]})
        };
        // `para` carries no payload: the bag is foreign, and drops unread.
        assert!(from_canonical_value(&line("para")).is_ok());
    }

    /// A table whose grid pads past [`MAX_TABLE_CELLS`](crate::MAX_TABLE_CELLS)
    /// slots is refused before the mint pads it, on the stored lane and the op
    /// wire alike.
    #[test]
    fn a_table_past_its_cell_bound_is_refused() {
        let cell = serde_json::json!({"text": "", "marks": []});
        let props = |rows: usize| {
            serde_json::json!({"header": vec![cell.clone(); 1000], "rows": vec![[cell.clone()]; rows], "aligns": []})
        };
        let content = |props| {
            serde_json::json!({"text": "\u{FFFC}", "lines": [{"kind": "para", "containers": []}], "marks": [],
              "islands": [{"id": "isl-0", "type": "table", "props": props}]})
        };
        let rows = crate::MAX_TABLE_CELLS / 1000;
        assert!(from_canonical_value(&content(props(rows - 1))).is_ok());
        assert!(matches!(
            from_canonical_value(&content(props(rows))),
            Err(ParseError::Invalid(Invariant::TableTooLarge { cells, .. })) if cells == (rows + 1) * 1000
        ));
        let op = serde_json::json!({"op": "insert", "at": 0, "id": "i1", "type": "table", "props": props(rows)});
        assert!(matches!(
            crate::ops::island_op_from_value(&op),
            Err(ParseError::Invalid(Invariant::TableTooLarge { .. }))
        ));
    }

    #[test]
    fn deep_json_payload_is_rejected_on_the_op_wire() {
        let deep = nested_arrays(1_000);
        // An island's `props` is the one payload the op wire retains.
        let op = serde_json::json!({"op":"insert","at":0,
          "id":"i1","type":"image","props":deep});
        assert!(matches!(
            crate::ops::island_op_from_value(&op),
            Err(ParseError::Invalid(Invariant::JsonTooDeep { .. }))
        ));
    }

    /// A wire position past `usize` is refused, not truncated: on wasm32 the
    /// truncating cast lands a mark at the wrong position in a document that
    /// then validates clean. Refused by the checked read on 32-bit and by the
    /// range invariant on 64-bit.
    #[test]
    fn out_of_range_wire_position_is_refused() {
        let json = r#"{"text":"hello","lines":[{"kind":"para","containers":[]}],"marks":[{"start":4294967301,"end":4294967302,"type":"strong"}],"islands":[]}"#;
        assert!(Content::from_canonical_json(json).is_err());
        assert!(usv_from(Some(&Value::from(u64::MAX)), "x").is_ok() || usize::BITS < 64);
        assert!(usv_from(Some(&Value::from(-1i64)), "x").is_err());
    }

    #[test]
    fn island_props_key_order_does_not_leak() {
        let mut one = Content::empty();
        one.text = "\u{FFFC}".into();
        one.lines = vec![Line {
            kind: LineKind::Para,
            containers: vec![],
            continues: false,
        }];
        one.islands = vec![Island {
            id: "i1".into(),
            island_type: IslandType::Table,
            props: serde_json::json!({"b": 1, "a": 2}),
        }];
        let mut two = one.clone();
        two.islands[0].props = serde_json::json!({"a": 2, "b": 1}); // keys reversed
        assert_eq!(
            one.into_normalized().to_canonical_json(),
            two.into_normalized().to_canonical_json()
        );
    }

    /// Every encoder emits its own keys in ascending order and the mint
    /// settles every payload bag, so
    /// [`to_canonical_value`]'s backstop scans and returns instead of rebuilding
    /// the tree. A key inserted out of order still serializes canonically — the
    /// backstop repairs it — so nothing else notices the regression.
    #[test]
    fn encoders_emit_keys_in_ascending_order() {
        use crate::model::is_value_key_sorted;
        let bag = || serde_json::json!({"a": 1, "b": 2});
        let sorted = |v: &Value| is_value_key_sorted(v);

        let kinds = [
            MarkKind::Strong,
            MarkKind::Emph,
            MarkKind::Underline,
            MarkKind::Strike,
            MarkKind::Code,
            MarkKind::Link { url: "u".into() },
            MarkKind::FontFamily { name: "f".into() },
            MarkKind::Anchor { id: "a".into() },
        ];
        let marks: Vec<Mark> = kinds
            .iter()
            .map(|kind| Mark {
                start: 0,
                end: 1,
                kind: kind.clone(),
            })
            .collect();
        for m in &marks {
            assert!(sorted(&mark_to_value(m)), "mark {:?}", m.kind);
        }
        assert!(sorted(&cell_to_value("t", &marks)));

        let containers = vec![
            Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                checked: None,
                instance: 0,
            },
            Container::Quote { instance: 0 },
        ];
        for c in &containers {
            assert!(sorted(&container_to_value(c)), "container {c:?}");
        }

        let line_kinds = [
            LineKind::Para,
            LineKind::Heading { level: 2 },
            LineKind::Code {
                lang: Some("rust".into()),
            },
            LineKind::Code { lang: None },
            LineKind::Rule,
        ];
        for kind in line_kinds {
            for continues in [false, true] {
                let line = Line {
                    kind: kind.clone(),
                    containers: containers.clone(),
                    continues,
                };
                assert!(sorted(&line_to_value(&line)), "line {:?}", line.kind);
            }
        }

        for &island_type in IslandType::ALL {
            let island = Island {
                id: "i1".into(),
                island_type,
                props: bag(),
            };
            assert!(
                sorted(&island_to_value(&island)),
                "island {}",
                island_type.as_str()
            );
        }

        let mut rt = Content::empty();
        rt.text = "\u{FFFC}".into();
        rt.lines = vec![Line::new(LineKind::Para)];
        rt.islands = vec![Island::new("i1".into(), IslandType::Table).with_props(
            serde_json::json!({
                "header": [{"text": "h", "marks": [{"start": 0, "end": 1, "type": "emph"}]}],
                "rows": [[{"text": "r", "marks": []}]],
                "aligns": ["none"],
            }),
        )];
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()));
        assert!(sorted(&rt.to_value()), "the root, or a bag the mint settled");
    }

    #[test]
    fn canonical_json_bytes_are_frozen() {
        // If either string changes, the freeze changed: bump the schema version.
        let rt = sample().into_normalized();
        assert_eq!(
            rt.to_canonical_json(),
            r#"{"islands":[],"lines":[{"containers":[],"kind":"para"}],"marks":[{"end":5,"start":0,"type":"emph"},{"end":11,"start":6,"type":"strong"}],"text":"hello world"}"#
        );

        // A payload on all three carrier axes. The sample above spends none, so
        // on its own it sleeps through a change to *how* a payload is spelled —
        // which is the change a schema bump is most likely to be.
        let mut rt = Content::empty();
        rt.text = "hi".into();
        rt.lines = vec![Line {
            kind: LineKind::Heading { level: 2 },
            containers: vec![Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 0,
                checked: None,
                instance: 0,
            }],
            continues: false,
        }];
        rt.marks = vec![Mark {
            start: 0,
            end: 2,
            kind: MarkKind::Link { url: "u".into() },
        }];
        assert_eq!(
            rt.into_normalized().to_canonical_json(),
            concat!(
                r#"{"islands":[],"lines":[{"attrs":{"level":2},"containers":"#,
                r#"[{"attrs":{"ordered":true,"ordinal":0,"start":3},"container":"list_item"}],"#,
                r#""kind":"heading"}],"marks":[{"attrs":{"url":"u"},"end":2,"start":0,"type":"link"}],"#,
                r#""text":"hi"}"#
            )
        );
    }

    #[test]
    fn from_canonical_json_rejects_invalid() {
        // lines.len() != segment count.
        let bad =
            r#"{"text":"a\nb","lines":[{"kind":"para","containers":[]}],"marks":[],"islands":[]}"#;
        assert!(matches!(
            Content::from_canonical_json(bad),
            Err(ParseError::Invalid(_))
        ));
    }

    /// A row spelling a `loss` on an island opens, whatever the class, and the
    /// key is absent from what comes back: the read ignores a key the island
    /// vocabulary does not name rather than refusing the row or carrying it.
    #[test]
    fn a_stored_island_loss_class_opens_and_does_not_survive_the_read() {
        for class in ["lossless", "degraded", "unrepresentable", "partial"] {
            let row = format!(
                concat!(
                    r#"{{"islands":[{{"id":"i1","loss":"{class}","props":{{"alt":"a","url":"u"}},"#,
                    "\"type\":\"image\"}}],\"lines\":[{{\"containers\":[],\"kind\":\"para\"}}],",
                    "\"marks\":[],\"text\":\"\u{fffc}\"}}"
                ),
                class = class
            );
            let rt = Content::from_canonical_json(&row).expect("opens");
            assert!(
                !rt.to_canonical_json().contains("loss"),
                "`loss` survived the read of a `{class}` row"
            );
        }
    }

    /// Every axis is a closed set, and the two lanes agree: the name is
    /// refused, not carried, and the error names the axis and the name.
    #[test]
    fn an_unknown_name_is_refused_on_every_axis_and_both_lanes() {
        let doc = |islands: &str, lines: &str, marks: &str, text: &str| {
            format!(r#"{{"islands":[{islands}],"lines":[{lines}],"marks":[{marks}],"text":"{text}"}}"#)
        };
        let cases = [
            (
                "line kind",
                "callout",
                doc("", r#"{"containers":[],"kind":"callout"}"#, "", "hi"),
            ),
            (
                "container",
                "indent",
                doc(
                    "",
                    r#"{"containers":[{"container":"indent","instance":0}],"kind":"para"}"#,
                    "",
                    "hi",
                ),
            ),
            (
                "mark type",
                "highlight",
                doc(
                    "",
                    r#"{"containers":[],"kind":"para"}"#,
                    r#"{"end":2,"start":0,"type":"highlight"}"#,
                    "hi",
                ),
            ),
            (
                "island type",
                "widget",
                doc(
                    r#"{"id":"i1","props":{},"type":"widget"}"#,
                    r#"{"containers":[],"kind":"para"}"#,
                    "",
                    "\u{fffc}",
                ),
            ),
        ];
        for (axis, name, json) in cases {
            let v: Value = serde_json::from_str(&json).unwrap();
            for (lane, got) in [
                ("storage", from_canonical_value(&v)),
                ("authored", from_authored_value(&v)),
            ] {
                assert_eq!(
                    got.unwrap_err(),
                    ParseError::UnknownName {
                        axis,
                        name: name.to_string()
                    },
                    "{lane} lane accepted {axis} {name:?}"
                );
            }
        }
    }

    /// A table cell's marks are read leniently, so without a decoder arm of
    /// their own a name outside the vocabulary would be dropped by `canon_cell`
    /// and the row would open with its bytes moved. It is refused instead, on
    /// both lanes, like the mark axis everywhere else.
    #[test]
    fn an_unknown_cell_mark_name_is_refused_rather_than_dropped() {
        let cell_marks = |marks: &str| {
            format!(
                concat!(
                    r#"{{"islands":[{{"id":"i1","props":{{"aligns":["none"],"#,
                    r#""header":[{{"marks":[{marks}],"text":"h"}}],"#,
                    r#""rows":[[{{"marks":[],"text":"c"}}]]}},"type":"table"}}],"#,
                    "\"lines\":[{{\"containers\":[],\"kind\":\"island\"}}],\"marks\":[],\"text\":\"\u{fffc}\"}}"
                ),
                marks = marks
            )
        };
        let outside = cell_marks(r#"{"end":1,"start":0,"type":"highlight"}"#);
        let v: Value = serde_json::from_str(&outside).unwrap();
        for (lane, got) in [
            ("storage", from_canonical_value(&v)),
            ("authored", from_authored_value(&v)),
        ] {
            assert_eq!(
                got.unwrap_err(),
                ParseError::UnknownName {
                    axis: "mark type",
                    name: "highlight".to_string()
                },
                "{lane} lane accepted a cell mark type outside the vocabulary"
            );
        }

        // The split holds: a *malformed* cell mark is still skipped, so the row
        // opens. That rule is about shape, and predates the closure.
        let malformed = cell_marks(r#"{"end":"x","start":0,"type":"strong"}"#);
        let v: Value = serde_json::from_str(&malformed).unwrap();
        assert!(from_canonical_value(&v).is_ok());
    }

    /// A malformed discriminator is a shape error, not a vocabulary one: the
    /// closed set answers for names, and a non-string is not a name.
    #[test]
    fn a_malformed_discriminator_is_a_shape_error() {
        for bad in [
            r#"{"islands":[],"lines":[{"containers":[]}],"marks":[],"text":"x"}"#,
            r#"{"islands":[],"lines":[{"containers":[{"container":7}],"kind":"para"}],"marks":[],"text":"x"}"#,
            "{\"islands\":[{\"id\":\"i\",\"props\":{},\"type\":7}],\"lines\":[{\"containers\":[],\"kind\":\"island\"}],\"marks\":[],\"text\":\"\u{fffc}\"}",
        ] {
            assert!(
                matches!(
                    Content::from_canonical_json(bad),
                    Err(ParseError::Shape(_))
                ),
                "not a shape error: {bad}"
            );
        }
    }

    /// A host writing the `@0.93.0` spelling now holds a stale copy of the
    /// encoding, and where a bag sits beside it the sibling it meant is not
    /// the one [`payload`] reads. The last case is a cell mark that will not
    /// parse at all, the one axis with no strict decode behind it.
    #[test]
    fn authored_lane_rejects_the_legacy_payload_spelling() {
        let bad = [
            // line kind
            r#"{"islands":[],"lines":[{"containers":[],"kind":"heading","level":2}],"marks":[],"text":"x"}"#,
            // container
            r#"{"islands":[],"lines":[{"containers":[{"container":"list_item","ordered":true}],"kind":"para"}],"marks":[],"text":"x"}"#,
            // prose mark
            r#"{"islands":[],"lines":[{"containers":[],"kind":"para"}],"marks":[{"end":1,"start":0,"type":"link","url":"u"}],"text":"x"}"#,
            // table cell mark
            concat!(
                r#"{"islands":[{"id":"i1","props":{"aligns":["none"],"#,
                r#""header":[{"marks":[{"end":1,"start":0,"type":"link","url":"u"}],"text":"h"}],"#,
                r#""rows":[[{"marks":[],"text":"r"}]]},"type":"table"}],"#,
                r#""lines":[{"containers":[],"kind":"para"}],"marks":[],"text":"￼"}"#
            ),
            // table cell mark with no `type` at all
            concat!(
                r#"{"islands":[{"id":"i1","props":{"aligns":["none"],"#,
                r#""header":[{"marks":[{"end":1,"start":0}],"text":"h"}],"#,
                r#""rows":[[{"marks":[],"text":"r"}]]},"type":"table"}],"#,
                r#""lines":[{"containers":[],"kind":"para"}],"marks":[],"text":"￼"}"#
            ),
        ];
        for json in bad {
            let v: Value = serde_json::from_str(json).unwrap();
            assert!(
                matches!(from_authored_value(&v), Err(ParseError::Shape(_))),
                "accepted: {json}"
            );
            // The storage lane opens all five: stored content in that
            // spelling must keep loading, and most of it reaches no migration.
            assert!(
                Content::from_canonical_json(json).is_ok(),
                "storage lane rejected: {json}"
            );
        }
        // …and what storage does with the unreadable one: skips it, keeping the
        // document openable.
        let rt = Content::from_canonical_json(bad[4]).unwrap();
        assert!(rt.islands[0].props["header"][0]["marks"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    /// A foreign bag on a built-in is neither the legacy spelling nor a
    /// payload: it drops unread on a member that carries none, and is read past
    /// on one that does. Both lanes agree, the shape being unambiguous.
    #[test]
    fn a_foreign_bag_on_a_built_in_drops_unread() {
        let json = concat!(
            r#"{"islands":[],"lines":[{"attrs":{"tone":"warn"},"containers":"#,
            r#"[{"attrs":{"x":1},"container":"quote"}],"kind":"para"}],"#,
            r#""marks":[{"attrs":{"y":2},"end":1,"start":0,"type":"strong"}],"text":"x"}"#
        );
        let v: Value = serde_json::from_str(json).unwrap();
        let rt = from_authored_value(&v).expect("authored lane accepts");
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert_eq!(rt.marks[0].kind, MarkKind::Strong);
        assert_eq!(
            Content::from_canonical_json(json).expect("storage lane accepts"),
            rt
        );
        assert_eq!(
            rt.to_canonical_json(),
            r#"{"islands":[],"lines":[{"containers":[{"container":"quote"}],"kind":"para"}],"marks":[{"end":1,"start":0,"type":"strong"}],"text":"x"}"#
        );
    }

    /// An opaque carrier is not scanned: a foreign key inside a table island's
    /// `props` may hold a link-shaped value with a url the projection could not
    /// write, and the authored lane leaves it alone.
    #[test]
    fn authored_lane_leaves_opaque_props_payload_alone() {
        let json = concat!(
            r#"{"islands":[{"id":"i1","props":{"aligns":["none"],"#,
            r#""header":[{"marks":[],"text":"h"}],"note":{"type":"link","url":"a\nb"},"#,
            r#""rows":[[{"marks":[],"text":"c"}]]},"type":"table"}],"#,
            r#""lines":[{"containers":[],"kind":"para"}],"marks":[],"text":"￼"}"#
        );
        let v: Value = serde_json::from_str(json).unwrap();
        let rt = from_authored_value(&v).unwrap();
        assert_eq!(rt.to_canonical_json(), json);
    }

    /// A `lang` is written into a fence header unquoted, so every lane that
    /// mints a `Code` reduces it to the identifier shape the emitter assumes —
    /// the storage decode and the `setKind` op wire as much as the importer.
    #[test]
    fn decoded_code_lang_carries_the_sanitized_shape() {
        let cases: [(Value, Option<&str>); 5] = [
            (
                serde_json::json!({"kind": "code", "attrs": {"lang": "rust\ninjected line"}}),
                Some("rust"),
            ),
            (
                serde_json::json!({"kind": "code", "attrs": {"lang": "r`s"}}),
                Some("r"),
            ),
            (
                serde_json::json!({"kind": "code", "attrs": {"lang": " rust"}}),
                None,
            ),
            (
                serde_json::json!({"kind": "code", "attrs": {"lang": "c++ 17"}}),
                Some("c++"),
            ),
            // The sibling spelling reaches the same reduction.
            (
                serde_json::json!({"kind": "code", "lang": "r`s"}),
                Some("r"),
            ),
        ];
        for (v, want) in cases {
            assert_eq!(
                line_kind_from_value(&v).unwrap(),
                LineKind::Code {
                    lang: want.map(str::to_string)
                },
                "{v}"
            );
        }
    }

    /// The authored wire refuses a `lang` the emitter cannot write, where the
    /// storage decode reduces it so the blob still opens.
    #[test]
    fn an_unwritable_code_lang_is_refused_when_authored() {
        let v = serde_json::json!({"kind": "code", "attrs": {"lang": "rust\ninjected line"}});
        assert!(matches!(
            line_kind_from_authored_value(&v),
            Err(ParseError::Shape("code lang"))
        ));
    }

    /// The same split on a link or image `url`: CommonMark admits no line
    /// ending in a destination, so the whole-content authored door refuses one
    /// as deep as the values the projection writes, while storage keeps what it
    /// holds and [`crate::export::emit_url`] percent-encodes it.
    #[test]
    fn an_unwritable_url_decodes_on_storage_and_is_refused_when_authored() {
        for (json, refusal) in [
            (
                concat!(
                    r#"{"islands":[],"lines":[{"containers":[],"kind":"para"}],"#,
                    r#""marks":[{"attrs":{"url":"a\nb"},"end":1,"start":0,"type":"link"}],"text":"x"}"#
                ),
                "link url",
            ),
            (
                concat!(
                    r#"{"islands":[{"id":"i1","props":{"alt":"a","url":"u\rv"},"#,
                    r#""type":"image"}],"lines":[{"containers":[],"kind":"para"}],"marks":[],"text":"￼"}"#
                ),
                "image url",
            ),
        ] {
            let v: Value = serde_json::from_str(json).unwrap();
            assert_eq!(from_authored_value(&v), Err(ParseError::Shape(refusal)));
            assert_eq!(
                Content::from_canonical_json(json).unwrap().to_canonical_json(),
                json
            );
        }
    }

    /// A block-only island's slot sharing its line rests as given on both
    /// lanes: the mint moves no text, so a merge of concurrent edits stores as
    /// it is, and the markdown projection gives the slot its line.
    #[test]
    fn a_block_island_sharing_its_line_rests_as_given_on_both_lanes() {
        let json = concat!(
            r#"{"islands":[{"id":"isl-0","props":{"aligns":["none"],"#,
            r#""header":[{"marks":[],"text":"h"}],"rows":[[{"marks":[],"text":"c"}]]},"#,
            r#""type":"table"}],"lines":[{"containers":[],"kind":"para"}],"#,
            r#""marks":[{"end":6,"start":2,"type":"strong"}],"text":"a￼bold"}"#
        );
        let stored = Content::from_canonical_json(json).expect("storage lane accepts");
        assert_eq!(stored.validate(), Ok(()));
        assert_eq!(stored.to_canonical_json(), json);
        let authored = from_authored_value(&serde_json::from_str(json).unwrap()).expect("authored lane accepts");
        assert_eq!(authored, stored);
    }

    /// `island` names the line a block island sits on, which is a `Para`, and
    /// no encoder writes the name. It reads forever: the content encoding
    /// carries no schema tag of its own, so a `richtext` field or a `$seed`
    /// overlay spelling it is reachable by no migration hop. The row re-encodes
    /// as `para`, which is the byte movement the storage bump names.
    #[test]
    fn a_stored_island_line_kind_reads_as_the_para_it_projects() {
        assert_eq!(
            line_kind_from_value(&serde_json::json!({"kind": "island"})).unwrap(),
            LineKind::Para
        );

        let stored = concat!(
            r#"{"islands":[{"id":"isl-0","props":{"aligns":["none"],"#,
            r#""header":[{"marks":[],"text":"h"}],"rows":[[{"marks":[],"text":"c"}]]},"#,
            r#""type":"table"}],"lines":[{"containers":[],"kind":"island"}],"#,
            r#""marks":[],"text":"￼"}"#
        );
        let rt = Content::from_canonical_json(stored).expect("the old spelling loads");
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(
            rt.to_canonical_json(),
            stored.replace(r#""kind":"island""#, r#""kind":"para""#),
            "the reader moves the row's bytes and nothing else"
        );
        assert_eq!(
            crate::export::to_markdown(&rt),
            "| h |\n| --- |\n| c |",
            "the table still projects as a block"
        );
    }

    /// The `@0.93.0` spelling — every built-in's payload in named siblings —
    /// decodes unchanged. It goes with the sibling read, once no stored
    /// content is left in that shape: a question no schema tag can answer,
    /// since a `richtext` field rests as a content object under no tag.
    #[test]
    fn built_in_decoders_read_the_legacy_sibling_form() {
        let cases: [(Value, LineKind); 2] = [
            (
                serde_json::json!({"kind": "heading", "level": 2}),
                LineKind::Heading { level: 2 },
            ),
            (
                serde_json::json!({"kind": "code", "lang": "rust"}),
                LineKind::Code {
                    lang: Some("rust".into()),
                },
            ),
        ];
        for (v, want) in cases {
            assert_eq!(line_kind_from_value(&v).unwrap(), want);
        }
        let item = serde_json::json!({
            "container": "list_item", "ordered": true, "start": 3, "ordinal": 1
        });
        assert_eq!(
            container_from_value(&item).unwrap(),
            Some(Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                checked: None,
                instance: 0,
            })
        );
        for (v, want) in [
            (
                serde_json::json!({"start": 0, "end": 1, "type": "link", "url": "u"}),
                MarkKind::Link { url: "u".into() },
            ),
            (
                serde_json::json!({"start": 0, "end": 1, "type": "anchor", "id": "a1"}),
                MarkKind::Anchor { id: "a1".into() },
            ),
        ] {
            assert_eq!(mark_from_value(&v).unwrap().kind, want);
        }
        // Both spellings present: the bag is the spelling, and the sibling is
        // read only in its absence — so the bag wins and this stays a pure
        // fallback rather than a merge.
        let both = serde_json::json!({"kind": "heading", "level": 3, "attrs": {"level": 2}});
        assert_eq!(
            line_kind_from_value(&both).unwrap(),
            LineKind::Heading { level: 2 }
        );
        // Re-encode is the current spelling, so opening a legacy row and writing
        // it back moves its canonical bytes: read-repair, once per row.
        let legacy = r#"{"islands":[],"lines":[{"containers":[],"kind":"heading","level":2}],"marks":[],"text":"hi"}"#;
        assert_eq!(
            Content::from_canonical_json(legacy)
                .unwrap()
                .to_canonical_json(),
            r#"{"islands":[],"lines":[{"attrs":{"level":2},"containers":[],"kind":"heading"}],"marks":[],"text":"hi"}"#
        );
    }

    /// An empty bag is one of the two spellings of *no payload*, so it cannot
    /// out-vote the sibling read a real one does: reading it as a bag would
    /// fail a `heading` outright and renumber a list item in silence.
    #[test]
    fn an_empty_bag_does_not_shadow_the_legacy_sibling() {
        assert_eq!(
            line_kind_from_value(&serde_json::json!({
                "kind": "heading", "level": 2, "attrs": {}
            }))
            .unwrap(),
            LineKind::Heading { level: 2 }
        );
        assert_eq!(
            container_from_value(&serde_json::json!({
                "container": "list_item", "attrs": {}, "ordered": true, "start": 3, "ordinal": 1
            }))
            .unwrap(),
            Some(Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                checked: None,
                instance: 0,
            })
        );
    }

    /// A stored definition list or `qm-*` element loads as the blocks it held:
    /// a term line as a paragraph, a description or element container gone
    /// from every path. The authored lane refuses either container.
    #[test]
    fn a_retired_construct_loads_as_what_it_held() {
        let stored = serde_json::json!({
            "text": "T\nd\nk",
            "lines": [
                {"kind": "term", "containers": []},
                {"kind": "para", "containers": [{"container": "description"}]},
                {"kind": "para", "containers": [
                    {"container": "quote"},
                    {"container": "element", "attrs": {"$name": "keep"}},
                ]},
            ],
            "marks": [],
            "islands": [],
        });
        let loaded = from_canonical_value(&stored).unwrap();
        let shape: Vec<_> = loaded.lines.iter().map(|l| (l.kind.clone(), l.containers.clone())).collect();
        assert_eq!(
            shape,
            [
                (LineKind::Para, vec![]),
                (LineKind::Para, vec![]),
                (LineKind::Para, vec![Container::Quote { instance: 0 }]),
            ]
        );
        let element = serde_json::json!({"container": "element", "attrs": {"$name": "keep"}});
        assert!(matches!(
            container_from_authored_value(&element),
            Err(ParseError::UnknownName { axis: "container", .. })
        ));
        assert!(matches!(
            from_authored_value(&stored),
            Err(ParseError::UnknownName { axis: "container", .. })
        ));
    }

    /// The runs a dropped container parted stay parted: a list beside an
    /// element's list, or in an adjacent run of it, is a list of its own, and
    /// the stored value respells as what it loads to.
    #[test]
    fn a_dropped_container_still_parts_the_runs_below_it() {
        let item = |ordinal: u64, checked: bool| {
            serde_json::json!({"container": "list_item", "attrs": {"checked": checked, "ordered": false, "ordinal": ordinal, "start": 1}})
        };
        let keep = |instance: u64| serde_json::json!({"container": "element", "attrs": {"$name": "keep"}, "instance": instance});
        let stored = serde_json::json!({
            "text": "a\nb\nc",
            "lines": [
                {"kind": "para", "containers": [item(0, true)]},
                {"kind": "para", "containers": [keep(0), item(0, true)]},
                {"kind": "para", "containers": [keep(1), item(0, false)]},
            ],
            "marks": [],
            "islands": [],
        });
        let loaded = from_canonical_value(&stored).unwrap();
        assert_eq!(crate::export::to_markdown(&loaded), "- [x] a\n\n+ [x] b\n\n- [ ] c");
        assert_eq!(respell_retired(&stored), to_canonical_value(&loaded));
    }

    /// A font mark round-trips as its bag, an overlapping one settles to one
    /// family per character, and a bag naming no family the model admits is a
    /// shape error on both lanes.
    #[test]
    fn a_font_mark_decodes_only_a_family_it_can_hold() {
        let content = |marks: &str| format!(r#"{{"islands":[],"lines":[{{"containers":[],"kind":"para"}}],"marks":[{marks}],"text":"abc"}}"#);
        let flat = content(r#"{"attrs":{"name":"Carlito"},"end":1,"start":0,"type":"font_family"},{"attrs":{"name":"Arimo"},"end":2,"start":1,"type":"font_family"},{"attrs":{"name":"Carlito"},"end":3,"start":2,"type":"font_family"}"#);
        let rt = Content::from_canonical_json(&flat).expect("decodes");
        assert_eq!(rt.to_canonical_json(), flat);
        let json = content(r#"{"attrs":{"name":"Carlito"},"end":3,"start":0,"type":"font_family"},{"attrs":{"name":"Arimo"},"end":2,"start":1,"type":"font_family"}"#);
        let rt = Content::from_canonical_json(&json).expect("decodes");
        assert_eq!(rt.to_canonical_json(), flat);
        for mark in [
            r#"{"attrs":{"name":""},"end":2,"start":0,"type":"font_family"}"#,
            r#"{"attrs":{"name":" Carlito"},"end":2,"start":0,"type":"font_family"}"#,
            r#"{"end":2,"start":0,"type":"font_family"}"#,
        ] {
            let shape = "font_family name";
            let json = content(mark);
            assert_eq!(Content::from_canonical_json(&json), Err(ParseError::Shape(shape)), "{mark}");
            let value: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(from_authored_value(&value), Err(ParseError::Shape(shape)), "{mark}");
        }
    }

    /// The canonical tie-break is the `(type, attrs)` pair the wire carries,
    /// read back off the value, so canonical order is a function of the stored
    /// bytes rather than of variant declaration order.
    #[test]
    fn the_mark_tie_break_is_what_the_wire_carries() {
        let all = [
            MarkKind::Strong,
            MarkKind::Emph,
            MarkKind::Underline,
            MarkKind::Strike,
            MarkKind::Code,
            MarkKind::Link { url: "u".into() },
            MarkKind::FontFamily { name: "f".into() },
            MarkKind::Anchor { id: "a".into() },
        ];
        // Exhaustive on purpose: a new variant is a compile error here, where
        // the rule gets read.
        for k in &all {
            match k {
                MarkKind::Strong
                | MarkKind::Emph
                | MarkKind::Underline
                | MarkKind::Strike
                | MarkKind::Code
                | MarkKind::Link { .. }
                | MarkKind::FontFamily { .. }
                | MarkKind::Anchor { .. } => {}
            }
        }
        for k in &all {
            let wire = mark_to_value(&Mark::new(0, 1, k.clone()));
            let attrs = match wire.get("attrs") {
                Some(a) => crate::model::canonical_json_string(a),
                None => String::new(),
            };
            assert_eq!(
                k.sort_key(),
                (wire["type"].as_str().unwrap().to_string(), attrs),
                "{k:?}"
            );
        }
    }

}
