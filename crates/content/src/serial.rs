//! Canonical JSON serialization: the freeze.
//!
//! Byte-deterministic within this schema: equal [`Content`] values (by
//! `PartialEq` after [`Content::normalize`]) serialize to byte-equal JSON,
//! insensitive to the order marks/islands were discovered in. Three order
//! sources are closed here and in `normalize`: mark order (canonical sort),
//! island order (slot position), and object-key order inside island `props` /
//! unknown-mark `attrs` (recursively sorted).
//!
//! Two fixed points, and they are not the same promise. **Bytes**:
//! `to_canonical_json(from_canonical_json(b)) == b` for canonical `b`, what a
//! consumer hashing stored documents spends. **Values**:
//! `from_canonical_json(to_canonical_json(rt)) == rt` for a normalized `rt`,
//! which holds only while every discriminator's encoding is injective. An axis
//! can keep the first and lose the second: a value that encodes to some *other*
//! value's bytes moves nothing on disk and still fails its own round trip.
//!
//! The seam encoding and the storage encoding are the *same* canonical form.

use crate::model::{
    canonicalize_keys, Container, Invariant, Island, Line, LineKind, Loss, Mark,
    MarkKind, Content, Normalized, Usv,
};
use serde_json::{Map, Value};
use std::borrow::Cow;

/// Why canonical-JSON parsing failed. Structural only: a well-formed producer
/// (this crate's serializer, the seam, storage) never trips these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Top-level JSON was not an object, or a required key was missing/mistyped.
    Shape(&'static str),
    /// The JSON itself did not parse.
    Json(String),
    /// The value parsed but violates a content invariant.
    Invalid(crate::model::Invariant),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Shape(s) => write!(f, "content json shape: {s}"),
            ParseError::Json(s) => write!(f, "content json parse: {s}"),
            ParseError::Invalid(inv) => write!(f, "content invariant: {inv:?}"),
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

    fn to_value(&self, zero: ZeroInstance) -> Value {
        let mut root = Map::new();
        root.insert(
            "islands".into(),
            Value::Array(self.islands.iter().map(island_to_value).collect()),
        );
        root.insert(
            "lines".into(),
            Value::Array(self.lines.iter().map(|l| line_to_value(l, zero)).collect()),
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

/// Whether a zero `Container::instance` gets a key. The two forms decode
/// identically.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ZeroInstance {
    /// Storage: the key appears only where it is doing work, so a row written
    /// before the field existed re-encodes byte for byte.
    Omit,
    /// The seam: every container spells it, so a host reads back a container
    /// path it can hand straight to a write.
    Spell,
}

/// The **storage** canonical form as a structural [`Value`]: the recursively
/// key-sorted tree [`Normalized::to_canonical_json`] renders to bytes. A storage
/// layer embeds this as a nested object rather than an escaped string;
/// serializing it with `serde_json` is byte-identical to that JSON, independent
/// of the consumer's `preserve_order` feature.
///
/// [`to_seam_value`] is the same tree for a language binding, differing only in
/// a zero `instance`.
pub fn to_canonical_value(rt: &Normalized) -> Value {
    to_value_with(rt, ZeroInstance::Omit)
}

/// The **seam** form as a structural [`Value`]: [`to_canonical_value`] with
/// every `Container::instance` spelled, zero included.
///
/// A binding's read is also its write input — `overwrite(addr, reader.getContent(…))`,
/// `insertCard(removeCard(0))`. The discriminator that keeps two adjacent
/// same-shape runs apart is a field the host owes on the way back in, and a
/// field the read may omit is one the read type cannot require.
/// [`to_canonical_value`] keeps the omission, so stored bytes stay put.
pub fn to_seam_value(rt: &Normalized) -> Value {
    to_value_with(rt, ZeroInstance::Spell)
}

fn to_value_with(rt: &Normalized, zero: ZeroInstance) -> Value {
    let mut v = rt.to_value(zero);
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

/// [`bag_from_wire`] for a carrier axis' `attrs`: an empty bag reads as the
/// absent one. The encoder omits it either way, so collapsing here is what
/// makes a value decoded from one spelling equal one decoded from the other.
/// `normalize` cannot reach every such value — a [`crate::ops::MarkOp`] carries
/// a kind with no content to be normalized with.
///
/// Island `props` keeps both spellings, and takes [`bag_from_wire`] directly:
/// it is written on every island, so `{}` there is a value rather than an
/// omission.
fn attrs_from_wire(o: &Map<String, Value>, what: &'static str) -> Result<Value, ParseError> {
    let attrs = bag_from_wire(o, "attrs", what)?;
    Ok(if crate::model::is_empty_bag(&attrs) {
        Value::Null
    } else {
        attrs
    })
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
    // A missing/non-string `kind` is the one shape error here: the open set
    // absorbs unknown *names*, not malformed objects.
    let tag = o
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("line kind"))?;
    let legacy = legacy_line_kind_keys(tag);
    match tag {
        "para" => Ok(LineKind::Para),
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
        "island" => Ok(LineKind::Island),
        "rule" => Ok(LineKind::Rule),
        // Any other name is a block role this build lacks, kept opaque and
        // projected as `Para`, so the document still opens.
        other => Ok(LineKind::Unknown {
            tag: other.to_string(),
            attrs: attrs_from_wire(o, "line attrs")?,
        }),
    }
}

fn line_to_value(line: &Line, zero: ZeroInstance) -> Value {
    let mut m = line_kind_fields(&line.kind);
    m.insert(
        "containers".into(),
        Value::Array(
            line.containers
                .iter()
                .map(|c| container_to_value_with(c, zero))
                .collect(),
        ),
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
    let containers = o
        .get("containers")
        .and_then(Value::as_array)
        .ok_or(ParseError::Shape("containers"))?
        .iter()
        .map(container_from_value)
        .collect::<Result<_, _>>()?;
    let continues = o.get("continues").and_then(Value::as_bool).unwrap_or(false);
    Ok(Line {
        kind,
        containers,
        continues,
    })
}

/// Encode a [`Container`] into its storage wire object.
pub fn container_to_value(c: &Container) -> Value {
    container_to_value_with(c, ZeroInstance::Omit)
}

fn container_to_value_with(c: &Container, zero: ZeroInstance) -> Value {
    let mut m = Map::new();
    insert_attrs(&mut m, c.attrs());
    m.insert("container".into(), Value::String(c.tag().to_string()));
    // Not payload: the discriminator that keeps two adjacent same-shape runs
    // apart is an envelope key, carried on every arm including `Unknown`.
    if c.instance() != 0 || zero == ZeroInstance::Spell {
        m.insert("instance".into(), Value::from(c.instance()));
    }
    Value::Object(m)
}

/// Decode a [`Container`] from its canonical wire object.
pub fn container_from_value(v: &Value) -> Result<Container, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("container"))?;
    let tag = o
        .get("container")
        .and_then(Value::as_str)
        .ok_or(ParseError::Shape("container kind"))?;
    let instance = o.get("instance").and_then(Value::as_u64).unwrap_or(0);
    let legacy = legacy_container_keys(tag);
    match tag {
        "list_item" => Ok(Container::ListItem {
            ordered: payload(o, legacy, "ordered")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            start: payload(o, legacy, "start")
                .and_then(Value::as_u64)
                .unwrap_or(1),
            ordinal: payload(o, legacy, "ordinal")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            instance,
        }),
        "quote" => Ok(Container::Quote { instance }),
        // An unrecognized container round-trips opaque and projects
        // transparently.
        other => Ok(Container::Unknown {
            tag: other.to_string(),
            attrs: attrs_from_wire(o, "container attrs")?,
            instance,
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

/// A mark's fallible half: the prologue of [`mark_from_value`], and the whole of
/// what a caller wanting only the verdict needs. Building the [`MarkKind`]
/// cannot fail, and for an unknown tag it deep-clones the opaque `attrs` bag,
/// which a validity check has no reason to pay for.
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
        "anchor" => MarkKind::Anchor {
            id: payload(o, legacy, "id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        // Any other type name is an unknown mark, round-tripped opaque with
        // whatever `attrs` it carried.
        other => MarkKind::Unknown {
            tag: other.to_string(),
            attrs: attrs_from_wire(o, "mark attrs")?,
        },
    };
    Ok(Mark { start, end, kind })
}

// Authored-lane readers, strict about the payload spelling.
//
// [`payload`] reads a built-in's payload from a named sibling where there is no
// bag, so `{"type": "link", "url": "…"}` decodes. The two wire lanes want
// opposite answers to that, and the seam is authored-now vs read-back:
//
// - **Storage** (`Content::from_canonical_json`) stays lenient. Stored content
//   in that spelling is mostly beyond any migration's reach: a `richtext` field
//   rests as the content object inside an opaque payload value, with no schema
//   tag over it.
// - **Authored** (the `crate::ops` wire, and `install` through
//   [`from_authored_value`]) rejects it: the host is writing now, so the shape
//   means a stale copy of the encoding and the read is a guess at its intent.
//
// The rule is narrow on purpose: a *legacy payload key* beside the name that
// spelled it, nothing else. It reads the same frozen table [`payload`] does.

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
    container_from_value(v)
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
    mark_shape(v)?;
    Ok(())
}

/// [`island_from_value`] for the authored lane: an image `url` the markdown
/// projection cannot write is a shape error rather than a silent rewrite, the
/// rule [`line_kind_from_authored_value`] carries for a code fence's `lang`.
pub(crate) fn island_from_authored_value(v: &Value) -> Result<Island, ParseError> {
    reject_unwritable_image_url(v)?;
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
    let image = crate::island::KnownIslandType::Image.as_str();
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

fn reject_unwritable_url(url: Option<&str>, err: &'static str) -> Result<(), ParseError> {
    match url {
        Some(u) if !crate::export::url_is_writable(u) => Err(ParseError::Shape(err)),
        _ => Ok(()),
    }
}

/// [`from_canonical_value`] for a content the **host authored just now**: the
/// `install` input, not a blob read back from storage. Same decode, plus the
/// legacy-spelling rule on every axis [`Content::validate`] checks — line kinds,
/// containers, prose marks, table-cell marks — the writability rule on the urls
/// the projection spells, and the placement rule on a block-only island.
pub fn from_authored_value(v: &Value) -> Result<Normalized, ParseError> {
    authored_lane_scan(v)?;
    let rt = Content::from_value(v)?;
    reject_inline_block_island(&rt)?;
    seal(rt)
}

/// A block-only island's slot sharing its line with other content: markdown
/// writes a table as a block, so `Content::normalize` breaks the line around
/// one, splitting a paragraph the host did not ask to split. Read off the
/// decoded content, ahead of the mint that performs the break. The op wire
/// refuses the same placement ([`crate::ApplyError::BlockIslandNotAlone`]).
fn reject_inline_block_island(rt: &Content) -> Result<(), ParseError> {
    let chars: Vec<char> = rt.text.chars().collect();
    match crate::model::inline_block_islands(&chars, &rt.islands).next() {
        Some(_) => Err(ParseError::Shape("block island in a line's prose")),
        None => Ok(()),
    }
}

/// The authored-lane scan [`from_authored_value`] runs. Structural rather than a
/// blind recursive walk: an unknown's `attrs` is opaque host payload that may
/// legitimately contain an object spelled `{"type": "link", "url": …}`, and
/// rejecting that would make the carrier unable to carry.
fn authored_lane_scan(v: &Value) -> Result<(), ParseError> {
    for line in arr_or_empty(v, "lines") {
        reject_line_kind_legacy(line)?;
        for c in arr_or_empty(line, "containers") {
            reject_container_legacy(c)?;
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
    // through `KnownIslandType`, so a new mark-carrying type is a compile error
    // here rather than a silent skip.
    for island in arr_or_empty(v, "islands") {
        let ty = island.get("type").and_then(Value::as_str).unwrap_or_default();
        match crate::island::KnownIslandType::parse(ty) {
            Some(crate::island::KnownIslandType::Table) => {
                let Some(props) = island.get("props") else {
                    continue;
                };
                for cell in table_cell_values(props) {
                    for m in arr_or_empty(cell, "marks") {
                        reject_unreadable_mark(m)?;
                    }
                }
            }
            // No cells; the one prop the projection writes is the url.
            Some(crate::island::KnownIslandType::Image) => reject_unwritable_image_url(island)?,
            // An unknown type's props are opaque.
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

// A pipe-table cell is inline-only: its own plain `text` plus `marks` whose
// ranges are USV offsets into that text. The marks ride the same wire shape
// prose marks use, so nothing forks the encoding.

/// Parse a table-cell object `{text, marks}` leniently: its plain text plus the
/// marks over it. A malformed mark is skipped rather than failing. Public so the
/// typst emitter renders a cell through the same parse the codecs use.
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

/// Every cell's `(text, marks)` in a table island's props: header then each
/// body row, in order. For [`Content::validate`]'s cell-mark invariant checks.
pub(crate) fn table_cells(props: &Value) -> Vec<(String, Vec<Mark>)> {
    table_cell_values(props).map(parse_cell).collect()
}

// The `table` codec below is the primitive `crate::island` dispatches into for
// `KnownIslandType::Table`; island-type dispatch itself lives there.

/// Repair a table island's props in place to the canonical shape:
///
/// - **One column count.** `cols` is the widest of the header, any body row, and
///   `aligns`; the header, each row, and `aligns` are padded up to it (padding
///   only grows: no cell is ever truncated). Materializing the count into the
///   header means the markdown projection (header-derived) and the Typst
///   projection (widest-row) agree on one number.
/// - **Single-line cells.** Any `\n`/`\r` in a cell's text becomes a space (the
///   same rule import applies to soft/hard breaks). A 1:1 replacement keeps char
///   offsets stable, so the cell's marks stay in range.
/// - **Canonical cell marks.** Each cell's marks are re-normalized (sort,
///   same-kind union, drop zero-width) so equal cells serialize to equal bytes.
/// - **Arrays where arrays belong.** A present non-array `header`, `aligns`, or
///   row carries no cells, so it becomes an empty array rather than garbage the
///   validate-side twin then rejects.
pub(crate) fn normalize_table_props(props: &mut Value) {
    let cols = table_cols(props);
    let Some(obj) = props.as_object_mut() else {
        return;
    };
    let header = obj.entry("header").or_insert_with(|| Value::Array(vec![]));
    if !header.is_array() {
        *header = Value::Array(vec![]);
    }
    pad_row(header, cols);
    if let Some(h) = header.as_array_mut() {
        h.iter_mut().for_each(canon_cell);
    }
    let aligns = obj.entry("aligns").or_insert_with(|| Value::Array(vec![]));
    if !aligns.is_array() {
        *aligns = Value::Array(vec![]);
    }
    if let Some(a) = aligns.as_array_mut() {
        while a.len() < cols {
            a.push(Value::String("none".into()));
        }
    }
    if let Some(rows) = obj.get_mut("rows").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            if !row.is_array() {
                *row = Value::Array(vec![]);
            }
            pad_row(row, cols);
            if let Some(r) = row.as_array_mut() {
                r.iter_mut().for_each(canon_cell);
            }
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

/// Pad a cell array (header or body row) up to `cols` with empty cells. Never
/// shrinks: `cols` is the widest, so a shorter array only grows.
fn pad_row(v: &mut Value, cols: usize) {
    if let Some(arr) = v.as_array_mut() {
        while arr.len() < cols {
            arr.push(cell_to_value("", &[]));
        }
    }
}

/// Every char a downstream lexer reads as a line break, the separators
/// [`crate::normalize::is_line_separator`] names included. A cell is one line.
fn is_cell_break(c: char) -> bool {
    c == '\n' || c == '\r' || crate::normalize::is_line_separator(c)
}

/// De-newline a cell's text (each line break → a space, 1:1 so mark offsets
/// hold) and re-normalize its marks. Writes back into the cell's **own** object
/// rather than minting a fresh one, so a key this build does not recognize
/// survives.
fn canon_cell(cell: &mut Value) {
    let (text, marks) = parse_cell(cell);
    let text = if text.contains(is_cell_break) {
        text.replace(is_cell_break, " ")
    } else {
        text
    };
    let canon = cell_to_value(&text, &crate::model::normalize_marks(marks));
    match (cell.as_object_mut(), canon) {
        // Overwrite the canonical keys, leave the rest.
        (Some(o), Value::Object(fields)) => o.extend(fields),
        // A non-object cell holds no keys to preserve.
        (_, canon) => *cell = canon,
    }
}

/// A table island's shape violation, if any: the widths the header, `aligns`,
/// and each body row must share (the header width), plus the `\n`-free-cell rule.
/// The validate-side twin of [`normalize_table_props`].
pub(crate) fn table_shape_error(props: &Value) -> Option<Invariant> {
    // A present-but-non-array header can't carry column cells: `normalize`
    // rewrites it to an empty array, so an un-normalized one is a hand-built
    // degenerate island. (An absent header is a zero-column table, which is
    // well-formed: `empty_table_is_valid`.)
    if props.get("header").is_some_and(|h| !h.is_array()) {
        return Some(Invariant::TableHeaderNotArray);
    }
    let cols = props
        .get("header")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let aligns = props
        .get("aligns")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    if aligns != cols {
        return Some(Invariant::TableAlignsMismatch { aligns, cols });
    }
    if let Some(rows) = props.get("rows").and_then(Value::as_array) {
        for (i, row) in rows.iter().enumerate() {
            let width = row.as_array().map(|a| a.len()).unwrap_or(0);
            if width != cols {
                return Some(Invariant::TableRaggedRow {
                    row: i,
                    width,
                    cols,
                });
            }
        }
    }
    for (i, cell) in table_cell_values(props).enumerate() {
        let text = cell.get("text").and_then(Value::as_str).unwrap_or_default();
        if text.contains(is_cell_break) {
            return Some(Invariant::TableCellNewline { cell: i });
        }
    }
    None
}

pub(crate) fn island_to_value(island: &Island) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), Value::String(island.id.clone()));
    m.insert("loss".into(), island.loss.as_str().into());
    m.insert("props".into(), island.props.clone());
    m.insert("type".into(), Value::String(island.island_type.clone()));
    Value::Object(m)
}

pub(crate) fn island_from_value(v: &Value) -> Result<Island, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("island"))?;
    Ok(Island {
        id: o
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("island id"))?
            .to_string(),
        island_type: o
            .get("type")
            .and_then(Value::as_str)
            .ok_or(ParseError::Shape("island type"))?
            .to_string(),
        props: bag_from_wire(o, "props", "island props")?,
        // The class is carried whether or not this build interprets it; a
        // missing key is the faithful class.
        loss: o
            .get("loss")
            .and_then(Value::as_str)
            .map_or(Loss::LOSSLESS, Loss::new),
    })
}

#[cfg(test)]
mod tests {

    /// `instance` is written only where it is doing work, so a row stored
    /// before the field existed decodes, re-encodes, and content-hashes exactly
    /// as it did: the discriminator costs bytes only in the documents that
    /// carry an adjacent same-shape sibling.
    #[test]
    fn instance_is_absent_from_the_wire_until_it_is_needed() {
        let plain = r#"{"islands":[],"lines":[{"containers":[{"attrs":{"ordered":false,"ordinal":0,"start":1},"container":"list_item"}],"kind":"para"},{"containers":[{"attrs":{"ordered":false,"ordinal":1,"start":1},"container":"list_item"}],"kind":"para"}],"marks":[],"text":"a\nb"}"#;
        let rt = Content::from_canonical_json(plain).expect("decodes");
        assert_eq!(rt.to_canonical_json(), plain, "byte layout moved");
        assert!(rt.lines.iter().all(|l| l.containers[0].instance() == 0));

        // Two adjacent one-item lists: the one shape that spends the key.
        let two = r#"{"islands":[],"lines":[{"containers":[{"attrs":{"ordered":false,"ordinal":0,"start":1},"container":"list_item"}],"kind":"para"},{"containers":[{"attrs":{"ordered":false,"ordinal":0,"start":1},"container":"list_item","instance":1}],"kind":"para"}],"marks":[],"text":"a\nb"}"#;
        let rt = Content::from_canonical_json(two).expect("decodes");
        assert_eq!(rt.to_canonical_json(), two);
        assert_eq!(rt.lines[1].containers[0].instance(), 1);

        // Any distinct value a producer picks reads as the same two runs and
        // rests on the canonical pair.
        let raw = two.replace(r#""instance":1"#, r#""instance":37"#);
        let rt2 = Content::from_canonical_json(&raw).expect("decodes");
        assert_eq!(rt2, rt);
        assert_eq!(rt2.to_canonical_json(), two);
    }

    /// One value, two forms: each decodes to the other's content.
    #[test]
    fn the_seam_spells_a_zero_instance_storage_omits() {
        let storage = r#"{"islands":[],"lines":[{"containers":[{"container":"quote"}],"kind":"para"}],"marks":[],"text":"a"}"#;
        let seam = r#"{"islands":[],"lines":[{"containers":[{"container":"quote","instance":0}],"kind":"para"}],"marks":[],"text":"a"}"#;

        let rt = Content::from_canonical_json(storage).expect("decodes");
        assert_eq!(rt.to_canonical_json(), storage);
        assert_eq!(to_seam_value(&rt).to_string(), seam);
        assert_eq!(Content::from_canonical_json(seam).expect("decodes"), rt);
    }

    use super::*;
    use crate::model::{Fidelity, Line, LineKind};

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
        let cases: [(Value, &'static str); 4] = [
            (
                serde_json::json!({"text":"\u{fffc}","lines":[{"kind":"island","containers":[]}],
                  "marks":[],"islands":[{"id":"i1","type":"widget","loss":"lossless","props":deep}]}),
                "island props",
            ),
            (
                serde_json::json!({"text":"x","lines":[{"kind":"para","containers":[]}],
                  "marks":[{"start":0,"end":1,"type":"sparkle","attrs":deep}],"islands":[]}),
                "mark attrs",
            ),
            (
                serde_json::json!({"text":"x","lines":[{"kind":"callout","containers":[],"attrs":deep}],
                  "marks":[],"islands":[]}),
                "line attrs",
            ),
            (
                serde_json::json!({"text":"x","lines":[{"kind":"para",
                  "containers":[{"container":"indent","attrs":deep}]}],"marks":[],"islands":[]}),
                "container attrs",
            ),
        ];
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
    /// `Value` lane costs no stored population. Stated as the implication rather
    /// than an offset, since `serde_json::from_str`'s own limit counts from the
    /// document root, not from the bag.
    #[test]
    fn json_depth_cap_admits_every_storable_payload() {
        let content = |props: Value| {
            serde_json::json!({"text":"\u{fffc}","lines":[{"kind":"island","containers":[]}],
              "marks":[],"islands":[{"id":"i1","type":"widget","loss":"lossless","props":props}]})
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
        // `callout` is unknown, so the bag *is* its payload and is retained.
        assert_eq!(
            from_canonical_value(&line("callout")),
            Err(ParseError::Invalid(Invariant::JsonTooDeep {
                what: "line attrs",
                max: crate::MAX_JSON_DEPTH,
            }))
        );
    }

    #[test]
    fn deep_json_payload_is_rejected_on_the_op_wire() {
        let deep = nested_arrays(1_000);
        let op = serde_json::json!({"op":"add","start":0,"end":1,"type":"sparkle","attrs":deep});
        assert!(matches!(
            crate::ops::mark_op_from_value(&op),
            Err(ParseError::Invalid(Invariant::JsonTooDeep { .. }))
        ));
        let op = serde_json::json!({"op":"setKind","line":0,"kind":"callout","attrs":deep});
        assert!(matches!(
            crate::ops::line_op_from_value(&op),
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
            kind: LineKind::Island,
            containers: vec![],
            continues: false,
        }];
        one.islands = vec![Island {
            id: "i1".into(),
            island_type: "table".into(),
            props: serde_json::json!({"b": 1, "a": 2}),
            loss: Loss::LOSSLESS,
        }];
        let mut two = one.clone();
        two.islands[0].props = serde_json::json!({"a": 2, "b": 1}); // keys reversed
        assert_eq!(
            one.into_normalized().to_canonical_json(),
            two.into_normalized().to_canonical_json()
        );
    }

    /// Every encoder emits its own keys in ascending order, so
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
            MarkKind::Anchor { id: "a".into() },
            MarkKind::Unknown {
                tag: "kbd".into(),
                attrs: bag(),
            },
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
                instance: 0,
            },
            Container::Quote { instance: 0 },
            Container::Unknown {
                tag: "indent".into(),
                attrs: bag(),
                instance: 0,
            },
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
            LineKind::Island,
            LineKind::Rule,
            LineKind::Unknown {
                tag: "callout".into(),
                attrs: bag(),
            },
        ];
        for kind in line_kinds {
            for continues in [false, true] {
                let line = Line {
                    kind: kind.clone(),
                    containers: containers.clone(),
                    continues,
                };
                for zero in [ZeroInstance::Omit, ZeroInstance::Spell] {
                    assert!(
                        sorted(&line_to_value(&line, zero)),
                        "line {:?} ({zero:?})",
                        line.kind
                    );
                }
            }
        }

        for island_type in ["table", "image", "widget"] {
            let island = Island {
                id: "i1".into(),
                island_type: island_type.into(),
                props: bag(),
                loss: Loss::LOSSLESS,
            };
            assert!(sorted(&island_to_value(&island)), "island {island_type}");
        }
    }

    /// The whole assembled tree, table cells and all: what the backstop actually
    /// scans.
    #[test]
    fn the_canonical_tree_needs_no_repair() {
        use crate::model::is_value_key_sorted;
        let mut rt = Content::empty();
        rt.text = "hi\n\u{FFFC}".into();
        rt.lines = vec![
            Line {
                kind: LineKind::Heading { level: 2 },
                containers: vec![Container::Quote { instance: 0 }],
                continues: false,
            },
            Line {
                kind: LineKind::Island,
                containers: vec![],
                continues: false,
            },
        ];
        rt.marks = vec![Mark {
            start: 0,
            end: 2,
            kind: MarkKind::Link { url: "u".into() },
        }];
        rt.islands = vec![Island {
            id: "i1".into(),
            island_type: "table".into(),
            props: serde_json::json!({
                "header": [{"text": "h", "marks": [{"start": 0, "end": 1, "type": "emph"}]}],
                "rows": [[{"text": "r", "marks": []}]],
                "aligns": ["none"],
            }),
            loss: Loss::LOSSLESS,
        }];
        rt.normalize();
        assert_eq!(rt.validate(), Ok(()));
        for zero in [ZeroInstance::Omit, ZeroInstance::Spell] {
            assert!(is_value_key_sorted(&rt.to_value(zero)), "{zero:?}");
        }
    }

    #[test]
    fn golden_bytes_are_feature_independent() {
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

    #[test]
    fn reserved_unknown_tag_rejected() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        rt.marks = vec![Mark {
            start: 0,
            end: 4,
            kind: MarkKind::Unknown {
                tag: "strong".into(),
                attrs: serde_json::json!({}),
            },
        }];
        assert!(matches!(
            rt.validate(),
            Err(crate::model::Invariant::ReservedUnknownTag(_))
        ));
    }

    /// A class this build lacks is **carried**, not rewritten, so opening the
    /// document does not move its content hash; reading it degrades to the safe
    /// end.
    #[test]
    fn unknown_loss_class_round_trips_and_reads_unrepresentable() {
        let json = concat!(
            r#"{"islands":[{"id":"i1","loss":"partial","props":{},"type":"widget"}],"#,
            r#""lines":[{"containers":[],"kind":"island"}],"marks":[],"text":"￼"}"#
        );
        let rt = Content::from_canonical_json(json).unwrap();
        assert_eq!(rt.islands[0].loss, Loss::new("partial"));
        assert_eq!(rt.islands[0].loss.fidelity(), Fidelity::Unrepresentable);
        assert_eq!(rt.to_canonical_json(), json);
    }

    /// Every class `Fidelity` names round-trips to its own level, so the closed
    /// view and the wire spellings cannot drift apart.
    #[test]
    fn every_fidelity_level_round_trips_through_its_class() {
        assert_eq!(Loss::new("lossless"), Loss::LOSSLESS);
        for &f in Fidelity::ALL {
            assert_eq!(Loss::new(f.as_str()).fidelity(), f);
        }
    }

    /// A `kind`/`container` this build lacks decodes to `Unknown` and re-encodes
    /// byte-identically, so the document opens and the construct survives.
    #[test]
    fn unknown_line_kind_and_container_round_trip_opaque() {
        let json = concat!(
            r#"{"islands":[],"lines":[{"attrs":{"variant":"warn"},"containers":"#,
            r#"[{"attrs":{"depth":2},"container":"indent"}],"kind":"callout"}],"#,
            r#""marks":[],"text":"heads up"}"#
        );
        let rt = Content::from_canonical_json(json).unwrap();
        assert_eq!(
            rt.lines[0].kind,
            LineKind::Unknown {
                tag: "callout".into(),
                attrs: serde_json::json!({"variant": "warn"}),
            }
        );
        assert_eq!(
            rt.lines[0].containers,
            vec![Container::Unknown {
                tag: "indent".into(),
                attrs: serde_json::json!({"depth": 2}),
                instance: 0,
            }]
        );
        assert_eq!(rt.to_canonical_json(), json);
        // An attrs-free unknown decodes too (`attrs` is null, not a shape error).
        let bare = r#"{"islands":[],"lines":[{"containers":[],"kind":"footnote"}],"marks":[],"text":"x"}"#;
        let rt = Content::from_canonical_json(bare).unwrap();
        assert_eq!(
            rt.lines[0].kind,
            LineKind::Unknown {
                tag: "footnote".into(),
                attrs: Value::Null,
            }
        );
        // A missing/non-string discriminator is still a shape error: the open
        // set absorbs unknown *names*, not malformed objects.
        for bad in [
            r#"{"islands":[],"lines":[{"containers":[]}],"marks":[],"text":"x"}"#,
            r#"{"islands":[],"lines":[{"containers":[{"container":7}],"kind":"para"}],"marks":[],"text":"x"}"#,
        ] {
            assert!(matches!(
                Content::from_canonical_json(bad),
                Err(ParseError::Shape(_))
            ));
        }
    }

    /// An unknown line kind / container may not reuse a built-in name: it would
    /// serialize as the built-in and parse back as one, dropping its attrs.
    #[test]
    fn reserved_block_vocabulary_names_rejected() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        rt.lines[0].kind = LineKind::Unknown {
            tag: "heading".into(),
            attrs: serde_json::json!({}),
        };
        assert_eq!(
            rt.validate(),
            Err(Invariant::ReservedUnknownLineKind("heading".into()))
        );
        rt.lines[0].kind = LineKind::Para;
        rt.lines[0].containers = vec![Container::Unknown {
            tag: "quote".into(),
            attrs: serde_json::json!({}),
            instance: 0,
        }];
        assert_eq!(
            rt.validate(),
            Err(Invariant::ReservedUnknownContainer("quote".into()))
        );
    }

    /// The authored lane refuses the `@0.93.0` payload spelling the storage lane
    /// still reads. A host writing it now holds a stale copy of the encoding,
    /// and where a bag sits beside it the sibling it meant is not the one
    /// [`payload`] reads. The last case is a cell mark that will not parse at
    /// all, the one axis with no strict decode behind it.
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
                r#"{"islands":[{"id":"i1","loss":"lossless","props":{"aligns":["none"],"#,
                r#""header":[{"marks":[{"end":1,"start":0,"type":"link","url":"u"}],"text":"h"}],"#,
                r#""rows":[[{"marks":[],"text":"r"}]]},"type":"table"}],"#,
                r#""lines":[{"containers":[],"kind":"island"}],"marks":[],"text":"￼"}"#
            ),
            // table cell mark with no `type` at all
            concat!(
                r#"{"islands":[{"id":"i1","loss":"lossless","props":{"aligns":["none"],"#,
                r#""header":[{"marks":[{"end":1,"start":0}],"text":"h"}],"#,
                r#""rows":[[{"marks":[],"text":"r"}]]},"type":"table"}],"#,
                r#""lines":[{"containers":[],"kind":"island"}],"marks":[],"text":"￼"}"#
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

    /// An unknown's `attrs` is opaque host payload and may contain an object
    /// spelled like a reserved mark; rejecting that would make the carrier
    /// unable to carry.
    #[test]
    fn authored_lane_leaves_opaque_attrs_payload_alone() {
        let json = concat!(
            r#"{"islands":[],"lines":[{"attrs":{"nested":{"attrs":{},"type":"link"}},"#,
            r#""containers":[],"kind":"callout"}],"marks":[],"text":"x"}"#
        );
        let v: Value = serde_json::from_str(json).unwrap();
        let rt = from_authored_value(&v).unwrap();
        assert_eq!(rt.to_canonical_json(), json);
    }

    /// Opaque block attrs are hash input, so their key order must not leak into
    /// the canonical bytes.
    #[test]
    fn unknown_block_attrs_key_order_does_not_leak() {
        let mut one = Content::empty();
        one.text = "hi".into();
        one.lines[0].kind = LineKind::Unknown {
            tag: "callout".into(),
            attrs: serde_json::json!({"b": 1, "a": 2}),
        };
        one.lines[0].containers = vec![Container::Unknown {
            tag: "indent".into(),
            attrs: serde_json::json!({"y": 1, "x": 2}),
            instance: 0,
        }];
        let mut two = one.clone();
        two.lines[0].kind = LineKind::Unknown {
            tag: "callout".into(),
            attrs: serde_json::json!({"a": 2, "b": 1}),
        };
        two.lines[0].containers = vec![Container::Unknown {
            tag: "indent".into(),
            attrs: serde_json::json!({"x": 2, "y": 1}),
            instance: 0,
        }];
        assert_eq!(
            one.clone().into_normalized().to_canonical_json(),
            two.clone().into_normalized().to_canonical_json()
        );
        one.normalize();
        two.normalize();
        assert_eq!(one, two, "normalize canonicalizes the live model too");
    }

    #[test]
    fn unknown_mark_round_trips_opaque() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        rt.marks = vec![Mark {
            start: 0,
            end: 4,
            kind: MarkKind::Unknown {
                tag: "highlight".into(),
                attrs: serde_json::json!({"color": "yellow"}),
            },
        }];
        let rt = rt.into_normalized();
        let json = rt.to_canonical_json();
        let back = Content::from_canonical_json(&json).unwrap();
        assert_eq!(back.marks[0].kind, rt.marks[0].kind);
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

    /// The two lanes answer a `lang` the emitter cannot write oppositely: the
    /// storage decode reduces it so the blob still opens, the authored wire
    /// refuses it so the host hears about it.
    #[test]
    fn an_unwritable_code_lang_sanitizes_on_storage_and_is_refused_when_authored() {
        let v = serde_json::json!({"kind": "code", "attrs": {"lang": "rust\ninjected line"}});
        assert_eq!(
            line_kind_from_value(&v).unwrap(),
            LineKind::Code {
                lang: Some("rust".to_string())
            }
        );
        assert!(matches!(
            line_kind_from_authored_value(&v),
            Err(ParseError::Shape("code lang"))
        ));
    }

    /// The same split on a link or image `url`: CommonMark admits no line
    /// ending in a destination, so a lane that stores one refuses it, while
    /// storage keeps what it holds and [`crate::export::emit_url`]
    /// percent-encodes it.
    #[test]
    fn an_unwritable_url_decodes_on_storage_and_is_refused_when_authored() {
        let mark =
            serde_json::json!({"type": "link", "start": 0, "end": 1, "attrs": {"url": "a\nb"}});
        assert_eq!(
            mark_from_value(&mark).unwrap().kind,
            MarkKind::Link { url: "a\nb".into() }
        );
        assert!(matches!(
            reject_unwritable_link_url(&mark),
            Err(ParseError::Shape("link url"))
        ));

        let island =
            serde_json::json!({"id": "i1", "type": "image", "props": {"alt": "a", "url": "u\rv"}});
        assert_eq!(island_from_value(&island).unwrap().props["url"], "u\rv");
        assert!(matches!(
            island_from_authored_value(&island),
            Err(ParseError::Shape("image url"))
        ));
    }

    /// The whole-content authored door (`install`, `overwrite`) carries the
    /// url rule as deep as the values the projection writes.
    #[test]
    fn the_authored_content_door_refuses_an_unwritable_url() {
        for json in [
            concat!(
                r#"{"islands":[],"lines":[{"containers":[],"kind":"para"}],"#,
                r#""marks":[{"attrs":{"url":"a\nb"},"end":1,"start":0,"type":"link"}],"text":"x"}"#
            ),
            concat!(
                r#"{"islands":[{"id":"i1","loss":"lossless","props":{"alt":"a","url":"u\nv"},"#,
                r#""type":"image"}],"lines":[{"containers":[],"kind":"para"}],"marks":[],"text":"￼"}"#
            ),
        ] {
            let v: Value = serde_json::from_str(json).unwrap();
            assert!(
                matches!(from_authored_value(&v), Err(ParseError::Shape(_))),
                "accepted: {json}"
            );
            assert!(
                Content::from_canonical_json(json).is_ok(),
                "storage lane rejected: {json}"
            );
        }
    }

    /// The same split on a block-only island's placement. A table's markdown is
    /// a block, so the storage lane takes the shape a blob holds and the mint
    /// gives the slot the line its markup needs, while a host authoring that
    /// placement is told, rather than having its paragraph split for it. An
    /// inline island keeps its position on both lanes.
    #[test]
    fn an_inline_block_island_splits_on_storage_and_is_refused_when_authored() {
        let slot = crate::model::ISLAND_SLOT;
        let content = |island_type: &str, props: &str| {
            format!(
                concat!(
                    r#"{{"islands":[{{"id":"isl-0","loss":"lossless","props":{},"#,
                    r#""type":"{}"}}],"lines":[{{"containers":[],"kind":"para"}}],"#,
                    r#""marks":[],"text":"a￼b"}}"#
                ),
                props, island_type
            )
        };
        let table = content(
            "table",
            r#"{"aligns":["none"],"header":[{"marks":[],"text":"h"}],"rows":[[{"marks":[],"text":"c"}]]}"#,
        );
        let rt = Content::from_canonical_json(&table).expect("storage lane accepts");
        assert_eq!(rt.text, format!("a\n{slot}\nb"), "not split at the load");
        assert_eq!(rt.lines[1].kind, LineKind::Island);
        let v: Value = serde_json::from_str(&table).unwrap();
        assert!(matches!(from_authored_value(&v), Err(ParseError::Shape(_))));

        let image = content("image", r#"{"alt":"a","url":"u"}"#);
        let rt = Content::from_canonical_json(&image).expect("storage lane accepts");
        assert_eq!(rt.text, format!("a{slot}b"), "inline island moved");
        let v: Value = serde_json::from_str(&image).unwrap();
        assert!(from_authored_value(&v).is_ok(), "inline island refused");
    }

    /// A stored blob carrying the placement loads, comes back split, and is
    /// then a fixed point of the markdown projection with its island and its
    /// marks intact: the write reads a shape that is already right. The mark
    /// spans the slot, so the rebase the split owes it is what keeps `bold`
    /// addressed.
    #[test]
    fn a_stored_inline_block_island_survives_the_markdown_round_trip() {
        let slot = crate::model::ISLAND_SLOT;
        let json = concat!(
            r#"{"islands":[{"id":"isl-0","loss":"lossless","props":{"aligns":["none"],"#,
            r#""header":[{"marks":[],"text":"h"}],"rows":[[{"marks":[],"text":"c"}]]},"#,
            r#""type":"table"}],"lines":[{"containers":[],"kind":"para"}],"#,
            r#""marks":[{"end":6,"start":2,"type":"strong"}],"text":"a￼bold"}"#
        );
        let rt = Content::from_canonical_json(json).expect("storage lane accepts");
        assert_eq!(rt.validate(), Ok(()), "loaded content invalid");
        assert_eq!(rt.text, format!("a\n{slot}\nbold"));
        assert_eq!(rt.lines.len(), 3);
        assert_eq!(rt.lines[1].kind, LineKind::Island);
        assert_eq!(rt.marks, vec![Mark::new(4, 8, MarkKind::Strong)]);

        let md = crate::export::to_markdown(&rt);
        let back = crate::import::from_markdown(&md).expect("re-imports");
        assert_eq!(back, rt, "{md:?}");
    }

    /// The `@0.93.0` spelling — every built-in's payload in named siblings —
    /// decodes unchanged. The frozen artifact: delete it with the sibling read,
    /// once no stored content is left in that shape, which no schema tag can
    /// answer since a `richtext` field rests as a content object under no tag of
    /// its own.
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
            Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                instance: 0,
            }
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
        // An unknown's bag is opaque payload, never a source of named fields.
        let unknown = serde_json::json!({"kind": "callout", "attrs": {"kind": "heading", "level": 2}});
        assert_eq!(
            line_kind_from_value(&unknown).unwrap(),
            LineKind::Unknown {
                tag: "callout".into(),
                attrs: serde_json::json!({"kind": "heading", "level": 2}),
            }
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

    /// An empty bag is not a bag: it is one of the two spellings of *no
    /// payload*, so it cannot out-vote the sibling read the way a real one
    /// does. Reading it as a bag would fail a `heading` outright and renumber a
    /// list item in silence.
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
            Container::ListItem {
                ordered: true,
                start: 3,
                ordinal: 1,
                instance: 0,
            }
        );
    }

    /// An empty bag has one spelling on the wire (absent) and one in memory
    /// (`Null`), on all three carrier axes. Without the second, the *value*
    /// fixed point wobbles: `{}` would encode to bytes that decode to `Null`,
    /// so a content would not equal its own round trip.
    #[test]
    fn an_empty_bag_has_one_spelling_on_each_side() {
        let build = |attrs: Value| {
            let mut rt = Content::empty();
            rt.text = "ab".into();
            rt.lines = vec![Line {
                kind: LineKind::Unknown {
                    tag: "callout".into(),
                    attrs: attrs.clone(),
                },
                containers: vec![Container::Unknown {
                    tag: "indent".into(),
                    attrs: attrs.clone(),
                    instance: 0,
                }],
                continues: false,
            }];
            rt.marks = vec![Mark {
                start: 0,
                end: 2,
                kind: MarkKind::Unknown {
                    tag: "kbd".into(),
                    attrs,
                },
            }];
            rt.into_normalized()
        };

        let null = build(Value::Null);
        let empty = build(serde_json::json!({}));
        // One in-memory spelling: `normalize` collapses the two.
        assert_eq!(null, empty);
        // One wire spelling: absent, on every axis.
        let json = null.to_canonical_json();
        assert!(!json.contains("attrs"), "{json}");
        assert_eq!(empty.to_canonical_json(), json);
        // …so both fixed points hold, the value one included.
        let back = Content::from_canonical_json(&json).unwrap();
        assert_eq!(back, null);
        assert_eq!(back.to_canonical_json(), json);
    }

    /// Promotion stops being an encoding change: the bytes written while a name
    /// was outside the vocabulary are the bytes the build that knows it reads,
    /// so no lane's answer flips on the release that promotes the name.
    #[test]
    fn promotion_does_not_move_the_encoding() {
        let doc = |kind: &str| {
            serde_json::json!({
                "islands": [],
                "lines": [{"attrs": {"level": 2}, "containers": [], "kind": kind}],
                "marks": [],
                "text": "hi",
            })
        };
        // Outside `RESERVED_LINE_KINDS` today, and what `"callout"` becomes the
        // release it is promoted: one spelling, accepted by both lanes either
        // side of that release.
        for kind in ["callout", "heading"] {
            assert!(from_authored_value(&doc(kind)).is_ok(), "{kind}");
            assert!(from_canonical_value(&doc(kind)).is_ok(), "{kind}");
        }
        // And the payload survives the promotion rather than dropping unread.
        assert_eq!(
            line_kind_from_value(&doc("heading")["lines"][0]).unwrap(),
            LineKind::Heading { level: 2 }
        );
    }

    /// The canonical tie-break is the `(type, attrs)` pair the wire carries, so
    /// a build that knows a member and one that reads it as `Unknown` order it
    /// identically — against every other member, not only the built-ins.
    #[test]
    fn the_mark_tie_break_is_what_the_wire_carries() {
        let all = [
            MarkKind::Strong,
            MarkKind::Emph,
            MarkKind::Underline,
            MarkKind::Strike,
            MarkKind::Code,
            MarkKind::Link { url: "u".into() },
            MarkKind::Anchor { id: "a".into() },
            MarkKind::Unknown {
                tag: "kbd".into(),
                attrs: Value::Null,
            },
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
                | MarkKind::Anchor { .. }
                | MarkKind::Unknown { .. } => {}
            }
        }
        for k in &all {
            // What a build lacking the name reconstructs from the same bytes.
            let wire = mark_to_value(&Mark::new(0, 1, k.clone()));
            let unknowing = MarkKind::Unknown {
                tag: wire["type"].as_str().unwrap().to_string(),
                attrs: wire.get("attrs").cloned().unwrap_or(Value::Null),
            };
            assert_eq!(k.sort_key(), unknowing.sort_key(), "{k:?}");
        }
    }

    /// Formatting-class membership is stored meaning: two adjacent unknowns are
    /// two marks, two adjacent formatting marks are one. Promoting a tag into
    /// the class therefore rewrites documents nobody edited.
    #[test]
    fn formatting_class_membership_decides_adjacent_union() {
        let mut rt = Content::empty();
        rt.text = "abcd".into();
        let unknown = |start, end| Mark {
            start,
            end,
            kind: MarkKind::Unknown {
                tag: "kbd".into(),
                attrs: serde_json::json!({}),
            },
        };
        rt.marks = vec![unknown(0, 2), unknown(2, 4)];
        rt.normalize();
        assert_eq!(rt.marks.len(), 2);
        rt.marks = vec![
            Mark {
                start: 0,
                end: 2,
                kind: MarkKind::Strong,
            },
            Mark {
                start: 2,
                end: 4,
                kind: MarkKind::Strong,
            },
        ];
        rt.normalize();
        assert_eq!(rt.marks.len(), 1);
    }

}
