//! Generates the virtual `@local/quillmark-helper` package that provides
//! document data to Typst plates.
//!
//! `lib.typ` is regenerated per render as pure source text: no runtime data
//! processing. What each schema node lowers to, and why ink is keyed by schema
//! address instead of carried on the value: `prose/canon/PLATE_DATA.md`.
//!
//! Output is **canonical**: dict keys emit in sorted order at every level (via
//! [`sorted`]), so equal data produces byte-equal source regardless of the
//! caller's field order. `$cards` array order is semantic and preserved. A
//! `matrix` node fixes its key order with `quillmark:roster` and gets it
//! ([`ordered`]); that order is a property of the schema, so byte-equality holds
//! there too.

use std::collections::HashMap;
use std::ops::Range;

use crate::emit::{
    emit_content, emit_content_inline, escape_string, EmitError, Emission, SegmentMap,
};
use crate::families::Families;
use crate::SchemaMeta;
use quillmark_content::serial::from_canonical_value;
use quillmark_core::quill::{
    CONTENT_MEDIA_TYPE, PLATE_PATH_KEY as PATH_KEY, QUILLMARK_MULTILINE_KEY, QUILLMARK_ROSTER_KEY,
};

pub const HELPER_VERSION: &str = "0.2.0";
pub const HELPER_NAMESPACE: &str = "local";
pub const HELPER_NAME: &str = "quillmark-helper";

const LIB_TYP_TEMPLATE: &str = include_str!("lib.typ.template");

/// One generated content block's source map in the produced `lib.typ`: the
/// bracketed `block` range (brackets included) plus the emitter's
/// [`SegmentMap`]s, rebased so every `generated` range indexes `lib.typ`
/// directly. Every glyph the block places carries a span resolving into `block`.
#[derive(Debug)]
pub struct ContentMap {
    pub path: String,
    pub block: Range<usize>,
    pub segments: Vec<SegmentMap>,
}

/// The source plus each ink window's [`ContentMap`]: every content block, then
/// every entry of the `_qm-ink` table. `Err` only on data the seam already
/// refuses: a content past the nesting bound, or a non-blank date that will not
/// parse ([`EmitError`]).
pub fn generate_lib_typ(
    data: &serde_json::Value,
    meta: &SchemaMeta,
    families: &Families,
) -> Result<(String, Vec<ContentMap>), EmitError> {
    let mut cg = Codegen::new(meta);
    let empty = serde_json::Map::new();
    let data_obj = data.as_object().unwrap_or(&empty);
    let data_literal = cg.emit_data(data_obj);
    // The recursive walk returns infallibly and records its failure here.
    if let Some(e) = cg.emit_error.take() {
        return Err(e);
    }
    let ink_literal = ink_literal(cg.ink);
    let literal = |set: &std::collections::BTreeSet<String>| {
        wrap_array(set.iter().map(|f| format!("\"{}\"", escape_string(f))).collect())
    };
    let (held_literal, offered_literal) = (literal(&families.held), literal(&families.offered));

    // Placeholders are located in the *raw template* (trusted static text), never
    // in already-substituted output, so document data spelling a placeholder
    // cannot hijack a splice point.
    let mut out = String::with_capacity(
        LIB_TYP_TEMPLATE.len()
            + cg.blocks.len()
            + data_literal.len()
            + ink_literal.src.len()
            + held_literal.len()
            + offered_literal.len(),
    );
    let mut cursor = 0usize;
    let mut blocks_at = 0usize;
    let mut ink_at = 0usize;
    for (slot, value) in [
        ("{version}", HELPER_VERSION),
        ("{version}", HELPER_VERSION),
        ("{meta_literal}", meta.meta_literal()),
        ("{held_literal}", held_literal.as_str()),
        ("{offered_literal}", offered_literal.as_str()),
        ("{content_blocks}", cg.blocks.as_str()),
        ("{data_literal}", data_literal.as_str()),
        ("{ink_literal}", ink_literal.src.as_str()),
    ] {
        let rel = LIB_TYP_TEMPLATE[cursor..]
            .find(slot)
            .unwrap_or_else(|| panic!("lib.typ.template carries the {slot} slot in order"));
        let at = cursor + rel;
        out.push_str(&LIB_TYP_TEMPLATE[cursor..at]);
        match slot {
            "{content_blocks}" => blocks_at = out.len(),
            "{ink_literal}" => ink_at = out.len(),
            _ => {}
        }
        out.push_str(value);
        cursor = at + slot.len();
    }
    out.push_str(&LIB_TYP_TEMPLATE[cursor..]);

    // Rebase every recorded window from section-relative to `lib.typ`-relative.
    let mut windows: Vec<ContentMap> = cg
        .windows
        .into_iter()
        .map(|(path, block, segments)| ContentMap {
            path,
            block: (block.start + blocks_at)..(block.end + blocks_at),
            segments: segments
                .into_iter()
                .map(|s| rebase_segment(s, blocks_at))
                .collect(),
        })
        .collect();
    windows.extend(ink_literal.windows.into_iter().map(|(path, block)| ContentMap {
        path,
        block: (block.start + ink_at)..(block.end + ink_at),
        segments: Vec::new(),
    }));
    Ok((out, windows))
}

/// Shifts generated byte offsets only; content (USV) offsets are unaffected.
fn rebase_segment(mut s: SegmentMap, shift: usize) -> SegmentMap {
    s.generated = (s.generated.start + shift)..(s.generated.end + shift);
    for run in &mut s.runs {
        run.1 = (run.1.start + shift)..(run.1.end + shift);
    }
    s
}

/// Accumulates the generated helper across one render.
struct Codegen<'m> {
    meta: &'m SchemaMeta,
    blocks: String,
    windows: Vec<(String, Range<usize>, Vec<SegmentMap>)>,
    counter: usize,
    emit_error: Option<EmitError>,
    /// `(schema address, entry)` per address with ink. Backs `_qm-ink`.
    ink: Vec<(String, Frag)>,
}

impl<'m> Codegen<'m> {
    fn new(meta: &'m SchemaMeta) -> Self {
        Self {
            meta,
            blocks: String::new(),
            windows: Vec::new(),
            counter: 0,
            emit_error: None,
            ink: Vec::new(),
        }
    }

    /// Binds `ec` as a `#let _qm_cN = [\n{markup}\n]` block. The `\n` wrap opens
    /// the block at a line boundary so line-anchored markup parses. An inline
    /// emission escapes its anchors and binds as `[{markup}]`, since a wrapping
    /// `\n` prints as a space at the value's edge. Windows are
    /// recorded in `self.blocks` coordinates; `generate_lib_typ` shifts them
    /// once more by the block section's start.
    fn content_block(&mut self, path: &str, ec: Emission, inline: bool) -> String {
        let id = format!("_qm_c{}", self.counter);
        self.counter += 1;
        self.blocks.push_str("#let ");
        self.blocks.push_str(&id);
        self.blocks.push_str(" = ");
        let start = self.blocks.len();
        let wrap = if inline { "" } else { "\n" };
        self.blocks.push('[');
        self.blocks.push_str(wrap);
        // The emitter's `generated` offsets are relative to the markup body.
        let markup_at = self.blocks.len();
        self.blocks.push_str(&ec.markup);
        self.blocks.push_str(wrap);
        self.blocks.push(']');
        let end = self.blocks.len();
        self.blocks.push('\n');
        let segments = ec
            .segments
            .into_iter()
            .map(|s| rebase_segment(s, markup_at))
            .collect();
        self.windows.push((path.to_string(), start..end, segments));
        id
    }

    /// The native cell. Blank ⇒ `none`, so a plate's `!= none` guard is
    /// untouched. A non-blank value that will not parse raises
    /// `backend::invalid_date` from here, the one site that parses, which is what
    /// makes the check total over depth.
    fn date_field(&mut self, path: &str, s: &str, kind: DateKind) -> Option<String> {
        match datetime_constructor(s, kind) {
            Some(constructor) => Some(constructor),
            None if s.is_empty() => None,
            None => {
                self.emit_error.get_or_insert(EmitError::InvalidDate {
                    field: path.to_string(),
                    value: s.to_string(),
                });
                None
            }
        }
    }

    /// The data cell and, for a non-blank content, the block binding it
    /// references. A blank content lowers to `""`, not a block; a value that is
    /// not a valid content (never produced by the seam) degrades to its value
    /// literal.
    fn content_field(
        &mut self,
        path: &str,
        value: &serde_json::Value,
        inline: bool,
    ) -> (String, Option<String>) {
        let emit = if inline {
            emit_content_inline
        } else {
            emit_content
        };
        match from_canonical_value(value) {
            Ok(rt) if !rt.is_blank() => match emit(&rt) {
                Ok(ec) => {
                    let id = self.content_block(path, ec, inline && rt.is_inline());
                    (id.clone(), Some(id))
                }
                Err(e) => {
                    self.emit_error.get_or_insert(e);
                    ("\"\"".to_string(), None)
                }
            },
            Ok(_) => ("\"\"".to_string(), None),
            Err(_) => (lit(value), None),
        }
    }

    fn emit_data(&mut self, obj: &serde_json::Map<String, serde_json::Value>) -> String {
        let mut dict = Container::default();
        for (key, value) in sorted(obj) {
            let access = format!("data.at(\"{}\")", escape_string(key));
            if key == "$cards" {
                if let Some(cards) = value.as_array() {
                    dict.items.push(format!("\"$cards\": {}", self.emit_cards(cards, &access)));
                    continue;
                }
            }
            let node = self.meta.field_node(key);
            dict.push(key, self.emit_value(key, &access, node, value).0);
        }
        dict.finish(Some(""))
    }

    /// A card with no string `$kind` passes through as a value literal, assigned
    /// no ordinal or `$path`.
    fn emit_cards(&mut self, cards: &[serde_json::Value], access: &str) -> String {
        let mut ordinals: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::with_capacity(cards.len());
        for (i, card) in cards.iter().enumerate() {
            let obj = match card.as_object() {
                Some(o) => o,
                None => {
                    out.push(lit(card));
                    continue;
                }
            };
            match obj.get("$kind").and_then(|v| v.as_str()) {
                Some(kind) => {
                    let n = ordinals.entry(kind.to_string()).or_insert(0);
                    let prefix = format!("$cards.{kind}.{n}.");
                    *n += 1;
                    out.push(self.emit_card(obj, kind, &prefix, &format!("{access}.at({i})")));
                }
                None => out.push(lit(card)),
            }
        }
        wrap_array(out)
    }

    fn emit_card(
        &mut self,
        obj: &serde_json::Map<String, serde_json::Value>,
        kind: &str,
        prefix: &str,
        access: &str,
    ) -> String {
        let props = self.meta.card_props(kind);
        let mut dict = Container::default();
        for (key, value) in sorted(obj) {
            let node = props.and_then(|p| p.get(key));
            let lowered = self.emit_value(
                &format!("{prefix}{key}"),
                &format!("{access}.at(\"{}\")", escape_string(key)),
                node,
                value,
            );
            dict.push(key, lowered.0);
        }
        dict.finish(Some(prefix))
    }

    /// Lower one value against the schema node that declares it, recursing on
    /// shape, into its data expression and, where [`inks`] holds for the node,
    /// the ink an array of it holds in its place. `path` is the value's schema
    /// address and `access` the expression reading it off `data`, so every
    /// `_qm-ink` entry keys on an address the recursion produced and reads the
    /// value the data literal holds.
    ///
    /// A value whose shape contradicts its declaration reaches here through a
    /// direct `update`, never the seam, and falls to its literal with `none` ink.
    fn emit_value(
        &mut self,
        path: &str,
        access: &str,
        node: Option<&serde_json::Value>,
        value: &serde_json::Value,
    ) -> (String, Option<Frag>) {
        let (expr, ink) = match (lowering(node), value) {
            (Lower::Content { inline }, serde_json::Value::Object(_)) => {
                let (expr, block) = self.content_field(path, value, inline);
                let ink = block.map(Frag::from);
                if let Some(ink) = &ink {
                    self.ink.push((path.to_string(), ink.clone()));
                }
                (expr, ink)
            }
            (Lower::Date(kind), serde_json::Value::String(s)) => {
                // The window covers exactly the `text(..)` call: the node whose
                // span the produced glyphs carry.
                let text = |args: &str| Frag::window(path, format!("text({access}.display({args}))"));
                match self.date_field(path, s, kind) {
                    Some(constructor) => {
                        let mut entry = Frag::from("(..args) => ");
                        entry.append(text("..args"));
                        self.ink.push((path.to_string(), entry));
                        (constructor, Some(text("")))
                    }
                    None => {
                        self.ink.push((path.to_string(), Frag::from("(..args) => none")));
                        ("none".to_string(), None)
                    }
                }
            }
            // A blank date still answers a pattern, with `none`.
            (Lower::Date(_), serde_json::Value::Null) => {
                self.ink.push((path.to_string(), Frag::from("(..args) => none")));
                ("none".to_string(), None)
            }
            (Lower::Array(items), serde_json::Value::Array(elems)) => {
                let (exprs, elem_inks): (Vec<String>, Vec<Option<Frag>>) = elems
                    .iter()
                    .enumerate()
                    .map(|(i, elem)| {
                        self.emit_value(&format!("{path}.{i}"), &format!("{access}.at({i})"), items, elem)
                    })
                    .unzip();
                let ink = inks(items).then(|| {
                    Frag::wrap(
                        elem_inks
                            .into_iter()
                            .map(|i| i.unwrap_or_else(|| Frag::from("none")))
                            .collect(),
                        "()",
                    )
                });
                if let Some(ink) = &ink {
                    self.ink.push((path.to_string(), ink.clone()));
                }
                (wrap_array(exprs), ink)
            }
            (Lower::Object(props, order, added), serde_json::Value::Object(obj)) => {
                let mut dict = Container::default();
                for (key, elem) in ordered(obj, &order) {
                    let node = props.get(key).or(added);
                    let lowered = self.emit_value(
                        &format!("{path}.{key}"),
                        &format!("{access}.at(\"{}\")", escape_string(key)),
                        node,
                        elem,
                    );
                    dict.push(key, lowered.0);
                }
                // A matrix's own dictionary keeps exactly its members as keys:
                // `roster` reads it through the dictionary declaring it.
                let prefix = format!("{path}.");
                (dict.finish((!is_matrix(node)).then_some(prefix.as_str())), None)
            }
            (
                Lower::Native,
                serde_json::Value::String(_)
                | serde_json::Value::Number(_)
                | serde_json::Value::Bool(_),
            ) if node.is_some() => {
                let ink = Frag::window(path, format!("[#({access})]"));
                self.ink.push((path.to_string(), ink.clone()));
                (lit(value), Some(ink))
            }
            _ => (lit(value), None),
        };
        let ink = ink.or_else(|| inks(node).then(|| Frag::from("none")));
        (expr, ink)
    }
}

/// Whether a field declared by `node` has ink: every declared type a plate
/// prints, and an array of them. A container does not; it carries `$path` for
/// its own cells' ink to address by.
fn inks(node: Option<&serde_json::Value>) -> bool {
    let Some(n) = node else {
        return false;
    };
    match lowering(node) {
        Lower::Content { .. } | Lower::Date(_) => true,
        Lower::Array(items) => inks(items),
        Lower::Object(..) => false,
        Lower::Native => node_type(n) != Some("object"),
    }
}

/// A span of generated source and the ink windows inside it, as
/// `(schema address, byte range)` relative to the fragment's start. Composing
/// fragments shifts their windows, so the ink table knows where each of its
/// entries' glyphs are born without a second pass over the text.
#[derive(Default, Clone)]
struct Frag {
    src: String,
    windows: Vec<(String, Range<usize>)>,
}

impl From<String> for Frag {
    fn from(src: String) -> Self {
        Self {
            src,
            windows: Vec::new(),
        }
    }
}

impl From<&str> for Frag {
    fn from(src: &str) -> Self {
        Self::from(src.to_string())
    }
}

impl Frag {
    /// One ink expression whose glyphs are born inside `src`, windowed whole.
    fn window(path: &str, src: String) -> Self {
        Self {
            windows: vec![(path.to_string(), 0..src.len())],
            src,
        }
    }

    fn push_str(&mut self, s: &str) {
        self.src.push_str(s);
    }

    fn append(&mut self, other: Frag) {
        let shift = self.src.len();
        self.src.push_str(&other.src);
        self.windows.extend(
            other
                .windows
                .into_iter()
                .map(|(path, r)| (path, (r.start + shift)..(r.end + shift))),
        );
    }

    /// [`wrap_array`] / [`wrap_dict`] over fragments; `empty` is the literal for
    /// no items.
    fn wrap(items: Vec<Frag>, empty: &str) -> Frag {
        if items.is_empty() {
            return Frag::from(empty);
        }
        let mut out = Frag::from("(");
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.append(item);
        }
        out.push_str(",)");
        out
    }
}

/// The `_qm-ink` table: each entry keyed by its address, in sorted order so a
/// reorder-only `update` produces byte-identical source. An array's entry and
/// its elements' each window the elements' glyphs, side by side rather than
/// nested.
fn ink_literal(mut entries: Vec<(String, Frag)>) -> Frag {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Frag::wrap(
        entries
            .into_iter()
            .map(|(path, entry)| {
                let mut out = Frag::from(format!("\"{}\": ", escape_string(&path)));
                out.append(entry);
                out
            })
            .collect(),
        "(:)",
    )
}

/// A dictionary under construction. A data key spelling `$path` is dropped, so
/// the generated one is the only one.
#[derive(Default)]
struct Container {
    items: Vec<String>,
}

impl Container {
    fn push(&mut self, key: &str, expr: String) {
        if key == PATH_KEY {
            return;
        }
        self.items.push(format!("\"{}\": {expr}", escape_string(key)));
    }

    /// `path` leads as `$path`, for `ink(dict, key)` and `roster(dict, key)` to
    /// address by. The `{..}` code block is a unit Typst's incremental reparser
    /// swaps alone, so a value edit inside a card or a row reparses that block,
    /// not the whole literal.
    fn finish(mut self, path: Option<&str>) -> String {
        if let Some(path) = path {
            self.items
                .insert(0, format!("\"{PATH_KEY}\": \"{}\"", escape_string(path)));
        }
        format!("{{{}}}", wrap_dict(self.items))
    }
}

/// What a schema node lowers to: the codegen walk's whole dispatch. Why only
/// the content types lower to content and a date does not:
/// `prose/canon/PLATE_DATA.md`.
enum Lower<'a> {
    /// `richtext` / `text`, which share everything downstream of this
    /// classification. `inline` lowers to pure inline markup, with no block
    /// terminator to warn inside a paragraph.
    Content { inline: bool },
    Date(DateKind),
    /// The element node, absent where an array declares no `items`.
    Array(Option<&'a serde_json::Value>),
    /// The declared members, the key order the schema fixes for them
    /// (`quillmark:roster`, empty where it fixes none), and the node every key
    /// they do not name lowers against (`additionalProperties`, an open
    /// matrix's added item).
    Object(
        &'a serde_json::Map<String, serde_json::Value>,
        Vec<&'a str>,
        Option<&'a serde_json::Value>,
    ),
    /// Every other declared type, plus every data key the schema does not
    /// declare and every container declaring no members to recurse on.
    Native,
}

fn lowering(node: Option<&serde_json::Value>) -> Lower<'_> {
    let Some(node) = node else {
        return Lower::Native;
    };
    let str_key = |key: &str| node.get(key).and_then(|v| v.as_str());
    if str_key("contentMediaType") == Some(CONTENT_MEDIA_TYPE) {
        return Lower::Content {
            inline: node.get(QUILLMARK_MULTILINE_KEY).and_then(|v| v.as_bool()) != Some(true),
        };
    }
    match str_key("format") {
        Some("date") => return Lower::Date(DateKind::Date),
        Some("date-time") => return Lower::Date(DateKind::DateTime),
        _ => {}
    }
    match node_type(node) {
        Some("array") => Lower::Array(node.get("items")),
        // A richtext node is `type: object` too, and the media type claimed it
        // above.
        Some("object") => match node.get("properties").and_then(|v| v.as_object()) {
            Some(props) => Lower::Object(
                props,
                declared_order(node),
                node.get("additionalProperties").filter(|v| v.is_object()),
            ),
            None => Lower::Native,
        },
        _ => Lower::Native,
    }
}

/// A node's declared type: its `type` string, or the non-`null` member of an
/// optional cell's `[<type>, "null"]` union.
pub(crate) fn node_type(node: &serde_json::Value) -> Option<&str> {
    match node.get("type")? {
        serde_json::Value::String(ty) => Some(ty),
        serde_json::Value::Array(union) => union
            .iter()
            .filter_map(|v| v.as_str())
            .find(|ty| *ty != "null"),
        _ => None,
    }
}

/// The two date field types, distinguished by their Typst `datetime(..)` arity.
#[derive(Clone, Copy)]
enum DateKind {
    Date,
    DateTime,
}

/// `None` for a string that does not parse (the empty string included), which
/// the caller lowers to `none`. Reuses the coercion layer's own parse, so a
/// value that reached here parses and `None` is the defensive arm.
fn datetime_constructor(s: &str, kind: DateKind) -> Option<String> {
    match kind {
        DateKind::Date => quillmark_core::quill::parse_date(s)
            .map(|(year, month, day)| format!("datetime(year: {year}, month: {month}, day: {day})")),
        DateKind::DateTime => {
            quillmark_core::quill::parse_datetime(s).map(|(year, month, day, hour, minute, second)| {
                format!(
                    "datetime(year: {year}, month: {month}, day: {day}, \
                     hour: {hour}, minute: {minute}, second: {second})"
                )
            })
        }
    }
}

/// Serialize a JSON value as a Typst literal expression, mirroring the value
/// model `json()` produces: `null` ⇒ `none`; integral `i64` numbers ⇒ int
/// literals; all other numbers ⇒ `float("..")` (Typst float *literals* take no
/// exponent syntax, but the `float(str)` constructor parses every finite
/// `f64`, and `json()` rejects non-finite numbers); strings via
/// [`escape_string`]; arrays and objects via [`wrap_array`] / [`wrap_dict`].
pub(crate) fn lit(v: &serde_json::Value) -> String {
    use serde_json::Value::*;
    match v {
        Null => "none".to_string(),
        Bool(b) => b.to_string(),
        Number(n) => {
            if let Some(i) = n.as_i64() {
                // Typst lexes a negative literal as unary `-` over an unsigned
                // magnitude, and `i64::MIN`'s magnitude overflows i64, so it
                // would not round-trip as a literal.
                if i == i64::MIN {
                    "(-9223372036854775807 - 1)".to_string()
                } else {
                    i.to_string()
                }
            } else {
                let f = n.as_f64().expect("json numbers are finite");
                format!("float(\"{f}\")")
            }
        }
        String(s) => format!("\"{}\"", escape_string(s)),
        Array(a) => wrap_array(a.iter().map(lit).collect()),
        Object(o) => wrap_dict(
            sorted(o)
                .into_iter()
                .map(|(k, v)| format!("\"{}\": {}", escape_string(k), lit(v)))
                .collect(),
        ),
    }
}

/// The workspace builds `serde_json` with `preserve_order`, so a map's own
/// iteration order is the caller's. Sorting makes the generated source a pure
/// function of the data's *values*, so a reorder-only `update` produces
/// byte-identical `lib.typ` and comemo reuses the whole compile.
fn sorted(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<(&String, &serde_json::Value)> {
    let mut entries: Vec<_> = obj.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// The schema's declared key order for a container node, `[]` where it declares
/// none: a `matrix`'s roster ids, the order a plate prints the vocabulary in.
fn declared_order(node: &serde_json::Value) -> Vec<&str> {
    node.get(QUILLMARK_ROSTER_KEY)
        .and_then(|v| v.as_array())
        .map(|pairs| pairs.iter().filter_map(|pair| pair.get(0)?.as_str()).collect())
        .unwrap_or_default()
}

/// Whether `node` declares a `matrix`.
fn is_matrix(node: Option<&serde_json::Value>) -> bool {
    node.is_some_and(|n| n.get(QUILLMARK_ROSTER_KEY).is_some())
}

/// `obj`'s entries in the schema's declared order where it declares one, keys it
/// does not name (an open matrix's added items) trailing in sorted order;
/// [`sorted`] otherwise.
///
/// The order comes from the schema, never from the data, so the generated source
/// stays a pure function of the data's values and comemo's reuse is untouched.
fn ordered<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    order: &[&str],
) -> Vec<(&'a String, &'a serde_json::Value)> {
    if order.is_empty() {
        return sorted(obj);
    }
    let mut entries: Vec<(&String, &serde_json::Value)> = order
        .iter()
        .filter_map(|id| obj.get_key_value(*id))
        .collect();
    entries.extend(sorted(obj).into_iter().filter(|(k, _)| !order.contains(&k.as_str())));
    entries
}

/// The trailing comma keeps a single element an array rather than a
/// parenthesized scalar.
fn wrap_array(items: Vec<String>) -> String {
    if items.is_empty() {
        "()".to_string()
    } else {
        format!("({},)", items.join(", "))
    }
}

fn wrap_dict(items: Vec<String>) -> String {
    if items.is_empty() {
        "(:)".to_string()
    } else {
        format!("({},)", items.join(", "))
    }
}

pub fn generate_typst_toml() -> String {
    format!(
        r#"[package]
name = "{name}"
version = "{version}"
namespace = "{namespace}"
entrypoint = "lib.typ"
"#,
        name = HELPER_NAME,
        version = HELPER_VERSION,
        namespace = HELPER_NAMESPACE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use quillmark_core::quill::CONTENT_MEDIA_TYPE;

    fn meta_from(schema: serde_json::Value) -> SchemaMeta {
        SchemaMeta::from_schema_json(&schema)
    }

    fn content(markdown: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(markdown).expect("import").content;
        quillmark_content::serial::to_canonical_value(&rt)
    }

    fn richtext_field() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "contentMediaType": CONTENT_MEDIA_TYPE,
            "quillmark:multiline": true
        })
    }

    fn inline_richtext_field() -> serde_json::Value {
        serde_json::json!({ "type": "object", "contentMediaType": CONTENT_MEDIA_TYPE })
    }

    #[test]
    fn inline_field_lowers_without_parbreak() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "subject": inline_richtext_field(),
            "intro": richtext_field(),
        }}));
        let data = serde_json::json!({
            "subject": content("A **bold** subject"),
            "intro": content("An intro."),
        });
        let (lib, _) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();
        assert!(
            lib.contains("[A #strong[bold] subject]"),
            "inline block should carry no parbreak: {lib}"
        );
        assert!(
            lib.contains("[\nAn intro.\n\n\n]"),
            "block field keeps its terminator: {lib}"
        );
    }

    /// An array field's elements lower against `items`, whose `quillmark:multiline` is absent.
    #[test]
    fn inline_array_lowers_each_element_without_parbreak() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "cc": { "type": "array", "items": inline_richtext_field() }
        }}));
        let data = serde_json::json!({ "cc": [content("First"), content("Second **bold**")] });
        let (lib, _) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();
        assert!(lib.contains("[First]"), "{lib}");
        assert!(lib.contains("[Second #strong[bold]]"), "{lib}");
    }

    #[test]
    fn content_field_becomes_a_referenced_block_with_a_bracket_window() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let data = serde_json::json!({ "intro": content("A **bold** intro.") });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        let block = "[\nA #strong[bold] intro.\n\n\n]";
        assert!(lib.contains(&format!("#let _qm_c0 = {block}")));
        assert!(lib.contains("\"intro\": _qm_c0"));

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].path, "intro");
        assert_eq!(&lib[windows[0].block.clone()], block);
    }

    /// The source map must be byte-accurate against the emitted `lib.typ`, not
    /// just against the emitter's own markup.
    #[test]
    fn segment_maps_index_the_generated_lib_typ() {
        let meta = meta_from(serde_json::json!({ "properties": { "intro": richtext_field() } }));
        let rt = quillmark_content::import::from_markdown("Hello **bold**.\n\nSecond para.")
            .expect("import").content;
        let data =
            serde_json::json!({ "intro": quillmark_content::serial::to_canonical_value(&rt) });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        assert_eq!(windows.len(), 1);
        let cm = &windows[0];
        assert_eq!(cm.segments.len(), 2);
        let chars: Vec<char> = rt.text.chars().collect();
        let mut saw_bold = false;
        for seg in &cm.segments {
            assert!(seg.generated.start <= seg.generated.end && seg.generated.end <= lib.len());
            for (content_range, generated, ctx) in &seg.runs {
                let src: String = chars[content_range.clone()].iter().collect();
                let expect = ctx.escape(&src);
                assert_eq!(&lib[generated.clone()], expect, "run inverts to its content slice");
                assert!(generated.start >= seg.generated.start && generated.end <= seg.generated.end);
                if src == "bold" {
                    saw_bold = true;
                }
            }
        }
        assert!(saw_bold, "the strong-wrapped run maps back to \"bold\"");
    }

    #[test]
    fn richtext_array_emits_a_block_per_nonblank_element() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "sections": {
                    "type": "array",
                    "items": richtext_field()
                }
            }
        }));
        let data = serde_json::json!({
            "sections": [content("one"), content(""), content("three")]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, vec!["sections.0", "sections.2"]);
        assert!(lib.contains("\"sections\": (_qm_c0, \"\", _qm_c1,)"));
    }

    #[test]
    fn date_fields_lower_to_native_datetimes_beside_a_display_closure() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "issued": { "type": "string", "format": "date" },
                "at": { "type": "string", "format": "date-time" },
                "signed": { "type": "string", "format": "date" }
            }
        }));
        let data = serde_json::json!({
            "issued": "2026-07-03",
            "at": "2026-07-03T09:30",
            "signed": null
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();
        // The data cell is the constructor itself: `data.issued.year()` and
        // `data.issued < data.at` are ordinary Typst.
        assert!(
            lib.contains("\"issued\": datetime(year: 2026, month: 7, day: 3)"),
            "{lib}"
        );
        assert!(
            lib.contains(
                "\"at\": datetime(year: 2026, month: 7, day: 3, hour: 9, minute: 30, second: 0)"
            ),
            "{lib}"
        );
        assert!(lib.contains("\"signed\": none"), "{lib}");

        // The ink table reads the cell rather than repeating it.
        assert!(
            lib.contains(r#""issued": (..args) => text(data.at("issued").display(..args))"#),
            "{lib}"
        );
        assert!(lib.contains(r#""signed": (..args) => none"#), "{lib}");
        let windows: Vec<(&str, &str)> = windows
            .iter()
            .map(|w| (w.path.as_str(), &lib[w.block.clone()]))
            .collect();
        assert!(
            windows.contains(&("issued", r#"text(data.at("issued").display(..args))"#)),
            "{windows:?}"
        );
        assert!(windows.iter().any(|(p, _)| *p == "at"), "{windows:?}");
        assert!(!windows.iter().any(|(p, _)| *p == "signed"), "{windows:?}");
    }

    /// Every nested position the one-level nesting contract admits, for both
    /// rich types.
    #[test]
    fn nested_dates_and_content_lower_exactly_as_card_level_ones_do() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "stamps": { "type": "array", "items": { "type": "string", "format": "date" } },
            "contact": { "type": "object", "properties": {
                "reply_by": { "type": "string", "format": "date" },
                "note": richtext_field(),
            }},
            "rows": { "type": "array", "items": { "type": "object", "properties": {
                "on": { "type": "string", "format": "date" },
                "notes": richtext_field(),
            }}},
        }}));
        let data = serde_json::json!({
            "stamps": ["2026-01-02"],
            "contact": { "reply_by": "2026-03-04", "note": content("a **nested** note") },
            "rows": [{ "on": "2026-05-06", "notes": content("row _one_") }],
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        assert!(
            lib.contains("\"stamps\": (datetime(year: 2026, month: 1, day: 2),)"),
            "an array element is a date: {lib}"
        );
        assert!(
            lib.contains("\"reply_by\": datetime(year: 2026, month: 3, day: 4)"),
            "an object property is a date: {lib}"
        );
        assert!(
            lib.contains("\"on\": datetime(year: 2026, month: 5, day: 6)"),
            "a typed-table row property is a date: {lib}"
        );
        assert!(
            lib.contains("[\na #strong[nested] note\n\n\n]"),
            "an object's content property is a markup block: {lib}"
        );
        assert!(
            lib.contains("[\nrow #emph[one]\n\n\n]"),
            "a row's content property is a markup block: {lib}"
        );
        // No canonical-content wire JSON survives anywhere in the data literal.
        assert!(!lib.contains("\"lines\""), "wire JSON leaked: {lib}");

        // Every projection keys on the address the recursion produced.
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        for expected in ["stamps.0", "contact.reply_by", "contact.note", "rows.0.on", "rows.0.notes"] {
            assert!(paths.contains(&expected), "missing {expected}: {paths:?}");
        }
    }

    /// A bad date must fail loudly at any depth, and must keep its code: a
    /// silent blank would ship a document dated wrong.
    #[test]
    fn an_unparseable_nested_date_is_a_coded_render_error() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "contact": { "type": "object", "properties": {
                "reply_by": { "type": "string", "format": "date" },
            }},
        }}));
        let data = serde_json::json!({ "contact": { "reply_by": "not-a-date" } });
        let err = generate_lib_typ(&data, &meta, &Families::default()).expect_err("a bad date stops codegen");
        assert_eq!(err.code(), "backend::invalid_date");
        assert!(err.to_string().contains("contact.reply_by"), "{err}");

        // A blank stays `none`, so a plate's `!= none` guard is untouched.
        let blank = serde_json::json!({ "contact": { "reply_by": "" } });
        let (lib, _) = generate_lib_typ(&blank, &meta, &Families::default()).unwrap();
        assert!(lib.contains("\"reply_by\": none"), "{lib}");
    }

    #[test]
    fn cards_carry_path_and_per_kind_ordinals() {
        let meta = meta_from(serde_json::json!({
            "properties": {},
            "$defs": {
                "note_card": {
                    "properties": {
                        "$body": richtext_field(),
                        "on": { "type": "string", "format": "date" }
                    }
                }
            }
        }));
        let data = serde_json::json!({
            "$cards": [
                { "$kind": "note", "$body": content("First body"), "on": "2026-01-02" },
                { "$kind": "note", "$body": content("Second body") }
            ]
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();
        // Each instance gets its own windows, keyed by the card's full address.
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        assert!(paths.contains(&"$cards.note.0.$body"), "{paths:?}");
        assert!(paths.contains(&"$cards.note.1.$body"), "{paths:?}");
        assert!(paths.contains(&"$cards.note.0.on"), "{paths:?}");
        assert!(lib.contains("\"$path\": \"$cards.note.0.\""));
        assert!(lib.contains("\"$path\": \"$cards.note.1.\""));
        assert!(
            lib.contains("\"on\": datetime(year: 2026, month: 1, day: 2)"),
            "{lib}"
        );
        assert!(
            lib.contains(r#""$cards.note.0.on": (..args) => text(data.at("$cards").at(0).at("on").display(..args))"#),
            "a card date's ink keys on its per-instance address: {lib}"
        );
    }

    /// Every printable address has one `_qm-ink` entry read off `data`, a blank
    /// date's answering `none`, an array's is its elements' ink, and every
    /// dictionary but a matrix carries `$path`. Each scalar entry is windowed on the
    /// address it prints, the array's beside its elements' rather than around
    /// them.
    #[test]
    fn the_ink_table_keys_each_printable_address_and_windows_its_glyphs() {
        let meta = meta_from(serde_json::json!({ "properties": {
            "subject": { "type": "string" },
            "count": { "type": "integer" },
            "due": { "type": ["string", "null"], "format": "date" },
            "tags": { "type": "array", "items": { "type": "string" } },
            "rows": { "type": "array", "items": { "type": "object", "properties": {
                "org": { "type": "string" },
            }}},
            "grid": { "type": "object", "properties": {
                "a": { "type": "object", "properties": { "x": { "type": "string" } } },
            }},
        }}));
        let data = serde_json::json!({
            "subject": "Widgets",
            "count": -3,
            "due": null,
            "tags": ["a", "b"],
            "rows": [{ "org": "AFRL", "$path": "spoofed" }],
            "grid": { "a": { "x": "1" } },
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        assert!(
            lib.contains(r#"#let data = {("$path": "", "count": -3, "due": none, "grid": {("$path": "grid.", "a": {("$path": "grid.a.", "x": "1",)},)}, "rows": ({("$path": "rows.0.", "org": "AFRL",)},), "subject": "Widgets","#),
            "data keys spelling `$path` are dropped: {lib}"
        );
        let table = lib
            .lines()
            .find(|l| l.starts_with("#let _qm-ink"))
            .expect("the ink table");
        for entry in [
            r#""count": [#(data.at("count"))]"#,
            r#""due": (..args) => none"#,
            r#""rows.0.org": [#(data.at("rows").at(0).at("org"))]"#,
            r#""tags": ([#(data.at("tags").at(0))], [#(data.at("tags").at(1))],)"#,
            r#""tags.1": [#(data.at("tags").at(1))]"#,
        ] {
            assert!(table.contains(entry), "{entry} in {table}");
        }
        assert!(!table.contains("\"rows\":"), "a typed table has no ink: {table}");

        let windows: Vec<(&str, &str)> = windows
            .iter()
            .map(|w| (w.path.as_str(), &lib[w.block.clone()]))
            .collect();
        assert_eq!(
            windows.iter().filter(|(p, _)| *p == "tags.0").count(),
            2,
            "the array's entry and the element's each window the element: {windows:?}"
        );
        for expected in [
            ("count", r#"[#(data.at("count"))]"#),
            ("grid.a.x", r#"[#(data.at("grid").at("a").at("x"))]"#),
            ("rows.0.org", r#"[#(data.at("rows").at(0).at("org"))]"#),
        ] {
            assert!(windows.contains(&expected), "{expected:?} in {windows:?}");
        }
        // `data` carries `$path` with no field of its own to print.
        let (lib, _) = generate_lib_typ(&serde_json::json!({ "grid": { "a": { "x": "1" } } }), &meta, &Families::default())
            .unwrap();
        assert!(lib.contains("#let data = {(\"$path\": \"\", \"grid\": "), "{lib}");
    }

    #[test]
    fn meta_literal_carries_the_address_tables() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "subject": { "type": "string" },
                "refs": { "type": "array", "items": { "type": "string" } }
            }
        }));
        let (lib, _) = generate_lib_typ(&serde_json::json!({}), &meta, &Families::default()).unwrap();
        assert!(lib.contains("#let _qm-meta = ("));
        // A scalar is a leaf, and a primitive-element array offers its index
        // step onto one: neither carries a `props` entry.
        assert!(
            lib.contains("\"props\": (\"refs\": (\"item\": (:),), \"subject\": (:),)"),
            "{lib}"
        );
    }

    /// A matrix's held set lowers its roster members in roster order and its
    /// added items after them sorted, each against its own node. Each member
    /// carries `$path` and the matrix none, and the address tables are the
    /// schema's alone, the added step and the roster included, whatever the
    /// data holds.
    #[test]
    fn a_matrix_lowers_in_roster_order_under_static_tables() {
        let member = |title: bool| {
            let mut props = serde_json::json!({ "detail": richtext_field() });
            if title {
                props["title"] = serde_json::json!({ "type": "string" });
            }
            serde_json::json!({ "type": "object", "properties": props })
        };
        let meta = meta_from(serde_json::json!({
            "properties": {
                "quals": {
                    "type": "object",
                    "quillmark:roster": [["dco", "DCO"], ["cyber", "Cyber"]],
                    "properties": { "dco": member(false), "cyber": member(false) },
                    "additionalProperties": member(true),
                }
            }
        }));
        let data = serde_json::json!({
            "quals": {
                "wing_ig": { "title": "Wing IG", "detail": content("81 TRW") },
                "cyber": { "detail": content("") },
                "aide": { "title": "Aide", "detail": content("") },
            }
        });
        let (lib, windows) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();

        let at = |key: &str| {
            lib.find(&format!("\"{key}\": {{"))
                .unwrap_or_else(|| panic!("{key} in {lib}"))
        };
        assert!(at("cyber") < at("aide") && at("aide") < at("wing_ig"), "{lib}");
        let paths: Vec<&str> = windows.iter().map(|w| w.path.as_str()).collect();
        for path in ["quals.wing_ig.detail", "quals.wing_ig.title"] {
            assert!(paths.contains(&path), "{path} in {paths:?}");
        }
        assert!(lib.contains("#let data = {(\"$path\": \"\", \"quals\": {(\"cyber\": {(\"$path\": \"quals.cyber.\""), "{lib}");

        let tables = |lib: &str| {
            lib.lines()
                .find(|l| l.starts_with("#let _qm-meta"))
                .expect("the address tables")
                .to_string()
        };
        let (bare, _) = generate_lib_typ(&serde_json::json!({}), &meta, &Families::default()).unwrap();
        assert_eq!(tables(&lib), tables(&bare));
        for piece in [
            "\"roster\": ((\"dco\", \"DCO\",), (\"cyber\", \"Cyber\",),)",
            "\"added\": (\"props\": (\"detail\": (:), \"title\": (:),),)",
        ] {
            assert!(tables(&lib).contains(piece), "{piece} in {}", tables(&lib));
        }
    }

    /// A richtext field is `type: object` and would answer the object arm, so
    /// the media type must claim it first — otherwise its wire JSON walks as a
    /// dictionary and reaches the plate as raw canonical content.
    #[test]
    fn a_content_node_is_classified_before_the_object_arm() {
        assert!(matches!(
            lowering(Some(&richtext_field())),
            Lower::Content { inline: false }
        ));
        assert!(matches!(
            lowering(Some(&inline_richtext_field())),
            Lower::Content { inline: true }
        ));
        // An undeclared data key and a scalar take the same arm.
        assert!(matches!(lowering(None), Lower::Native));
        assert!(matches!(
            lowering(Some(&serde_json::json!({ "type": "string" }))),
            Lower::Native
        ));
    }

    /// Slots are located in the raw template only.
    #[test]
    fn data_containing_placeholder_text_cannot_hijack_the_splice() {
        let meta = SchemaMeta::default();
        let data = serde_json::json!({
            "note": "quoting the template: {content_blocks} and {data_literal}"
        });
        let (lib, _) = generate_lib_typ(&data, &meta, &Families::default()).unwrap();
        let payload = lib.find("quoting the template").expect("payload present");
        let data_binding = lib.find("#let data =").expect("data binding present");
        assert!(
            payload > data_binding,
            "the payload sits inside the data literal, after the real slot"
        );
    }

    /// A reorder-only `update` must be a `Source::replace` no-op.
    #[test]
    fn reordered_input_emits_byte_identical_source() {
        let meta = meta_from(serde_json::json!({
            "properties": {
                "body": richtext_field(),
                "note": richtext_field(),
                "issued": { "type": "string", "format": "date" },
                "extra": { "type": "object" }
            },
            "$defs": {
                "note_card": {
                    "properties": {
                        "$body": richtext_field(),
                        "on": { "type": "string", "format": "date" }
                    }
                }
            }
        }));
        let a = serde_json::json!({
            "body": content("The body."),
            "note": content("The note."),
            "issued": "2026-01-02",
            "extra": { "x": 1, "y": 2 },
            "$cards": [ { "$kind": "note", "$body": content("Card body"), "on": "2027-03-04", "tag": "t" } ]
        });
        let b = serde_json::json!({
            "$cards": [ { "tag": "t", "on": "2027-03-04", "$body": content("Card body"), "$kind": "note" } ],
            "extra": { "y": 2, "x": 1 },
            "issued": "2026-01-02",
            "note": content("The note."),
            "body": content("The body.")
        });

        let (lib_a, win_a) = generate_lib_typ(&a, &meta, &Families::default()).unwrap();
        let (lib_b, win_b) = generate_lib_typ(&b, &meta, &Families::default()).unwrap();
        assert_eq!(lib_a, lib_b, "reordered input must emit identical source");
        let wa: Vec<_> = win_a.iter().map(|w| (&w.path, w.block.clone())).collect();
        let wb: Vec<_> = win_b.iter().map(|w| (&w.path, w.block.clone())).collect();
        assert_eq!(wa, wb, "windows must be identical too");
    }

    #[test]
    fn lit_serializes_the_json_value_model() {
        assert_eq!(lit(&serde_json::json!(null)), "none");
        assert_eq!(lit(&serde_json::json!(true)), "true");
        assert_eq!(lit(&serde_json::json!(42)), "42");
        assert_eq!(
            lit(&serde_json::json!(i64::MIN)),
            "(-9223372036854775807 - 1)"
        );
        assert_eq!(lit(&serde_json::json!(1.5)), "float(\"1.5\")");
        assert_eq!(lit(&serde_json::json!("hi")), "\"hi\"");
        assert_eq!(lit(&serde_json::json!([])), "()");
        assert_eq!(lit(&serde_json::json!([1])), "(1,)");
        assert_eq!(lit(&serde_json::json!({})), "(:)");
        assert_eq!(lit(&serde_json::json!({ "a": 1 })), "(\"a\": 1,)");
    }
}
