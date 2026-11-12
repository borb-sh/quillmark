//! Parsing and typed in-memory model for Quillmark card-yaml documents.
//!
//! A [`Document`] holds a root [`Card`] plus ordered composable cards. The
//! format is specified in
//! [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md).

use serde::{Deserialize, Serialize};

use quillmark_content::lines::model::Lines;
use quillmark_content::markdown::from_markdown as import_markdown;
use quillmark_content::model::Content;

use crate::error::ParseError;
use crate::error::{Diagnostic, Severity};
use crate::version::QuillReference;

/// The stored spelling of a line-form value: under the ids of what it kept
/// from `old`, the value it is written over, or minted fresh where there is
/// none.
#[cfg(test)]
pub(crate) fn stored_over(old: Option<&serde_json::Value>, lines: &Lines) -> serde_json::Value {
    match held_value(old) {
        Some(held) => quillmark_content::diff::lift(&held, lines),
        None => quillmark_content::migrate::from_lines(lines),
    }
    .to_value()
}

/// `content` stored under `codec` over `old`, the value it is written over:
/// under the ids of what it kept from `old`, or its own where there is none.
pub(crate) fn stored_as(
    old: Option<&serde_json::Value>,
    content: &Content,
    codec: quillmark_content::model::Codec,
) -> serde_json::Value {
    let mut content = match held_value(old) {
        Some(held) => quillmark_content::diff::align(&held, content.clone()),
        None => content.clone(),
    };
    content.codec = codec;
    content.to_value()
}

fn held_value(old: Option<&serde_json::Value>) -> Option<Content> {
    old.filter(|v| v.is_object())
        .and_then(|v| Content::from_value_lenient(v).ok())
        .map(|read| read.content)
}

/// [`import_body`] lowered to the line form, for the tests that read it.
#[cfg(test)]
pub(crate) fn import_body_lines(md: &str) -> Result<Lines, ImportError> {
    import_body(md).map(|c| quillmark_content::migrate::to_lines(&c))
}

pub(crate) fn import_body(md: &str) -> Result<Content, ImportError> {
    import_markdown(md).map(|imported| imported.content)
}

/// [`import_body`], pushing a [`dropped_construct`] warning at `at` for each
/// construct the import drops.
pub(crate) fn import_body_at(
    md: &str,
    at: &crate::path::DocPath,
    warnings: &mut Vec<Diagnostic>,
) -> Result<Content, ImportError> {
    let imported = import_markdown(md)?;
    warnings.extend(dropped_constructs(imported.warnings, Some(at)));
    Ok(imported.content)
}

/// The diagnostic code a markdown import's dropped construct rides.
pub const DROPPED_CONSTRUCT: &str = "parse::dropped_construct";

/// The warning a markdown import owes what it could not carry: `count` of
/// `construct` dropped from one field. It carries no `path`: the caller that
/// knows the field's address attaches it. Non-fatal: the rest of the markdown
/// imports.
pub fn dropped_construct(warning: ImportWarning) -> Diagnostic {
    let ImportWarning { construct, count } = warning;
    let (message, hint) = dropped_message(&construct, count);
    let mut args = std::collections::BTreeMap::new();
    args.insert("construct".to_string(), construct.to_string().into());
    args.insert("count".to_string(), count.into());
    Diagnostic::new(Severity::Warning, message)
        .with_code(DROPPED_CONSTRUCT.to_string())
        .with_hint(hint)
        .with_args(args)
}

/// One [`dropped_construct`] per warning, each at `at` when given.
pub fn dropped_constructs(
    warnings: Vec<ImportWarning>,
    at: Option<&crate::path::DocPath>,
) -> Vec<Diagnostic> {
    let at = at.map(ToString::to_string);
    warnings
        .into_iter()
        .map(|w| match &at {
            Some(at) => dropped_construct(w).with_path(at.clone()),
            None => dropped_construct(w),
        })
        .collect()
}

/// The diagnostic code a stored content value read in a spelling other than
/// its canonical one rides.
pub const NONCANONICAL_READ: &str = "content::noncanonical_read";

/// The warning a stored read owes a content value it rewrote to its canonical
/// form: the value loads canonical, and storing the document again writes
/// those bytes.
pub fn noncanonical_read() -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        "a stored content value is not in canonical form, and reads as it".to_string(),
    )
    .with_code(NONCANONICAL_READ.to_string())
    .with_hint("store the document again to write its canonical form".to_string())
}

/// `words` each in backticks, joined by commas and `last` before the final one.
fn listed(words: &[&str], last: &str) -> String {
    let ticked: Vec<String> = words.iter().map(|w| format!("`{w}`")).collect();
    match ticked.split_last() {
        Some((tail, [])) => tail.clone(),
        Some((tail, init)) => format!("{} {last} {tail}", init.join(", ")),
        None => String::new(),
    }
}

/// [`dropped_construct`]'s message, naming what dropped, and its hint, naming
/// the spelling that keeps it.
fn dropped_message(construct: &DroppedConstruct, n: usize) -> (String, String) {
    use quillmark_content::markdown::carrier::RESERVED_ATTRS;
    use quillmark_content::table::{CELL_VALIGNS, TABLE_ALIGNS};
    const TIGHT: &str =
        "Markdown on the lines under a tag line drops with it, up to the next blank line.";
    const BESIDE: &str = "A tag line beside a line of other tags drops with the markdown under it, up to the next blank line.";
    let some = |one: &str, many: &str| {
        if n == 1 {
            format!("a {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let some_in_field = |one: &str, many: &str| {
        if n == 1 {
            format!("a {one} in this field was")
        } else {
            format!("{n} {many} in this field were")
        }
    };
    let attr_dropped = |tag: &str, attr: &str, hint: String| {
        let on = if n == 1 {
            String::new()
        } else {
            format!(" on {n} tags")
        };
        (
            format!(
                "markdown import dropped the `{attr}` attribute of `<{tag}>`{on} in this field"
            ),
            hint,
        )
    };
    let tag = construct.to_string();
    match construct {
        DroppedConstruct::Footnote => (
            format!(
                "markdown import dropped {} in this field",
                some("footnote", "footnotes")
            ),
            "A footnote is a reference `[^label]` in prose, outside a table cell or another note, and one \
             definition `[^label]: note` naming the same label."
                .to_string(),
        ),
        DroppedConstruct::FootnoteSize => (
            format!(
                "markdown import kept {} in this field as written text",
                some("footnote reference", "footnote references")
            ),
            format!(
                "A reference copies its whole note, and after the first reading of each note the copies share                  {} characters; the references past it stay as `[^label]`. Cite a long note once.",
                quillmark_content::MAX_NOTE_COPIES
            ),
        ),
        DroppedConstruct::NoteBlock(block) => (
            format!(
                "markdown import kept the text of {} inside a footnote and dropped its structure",
                some(&format!("{block} block"), &format!("{block} blocks"))
            ),
            "A footnote's note is inline text: each paragraph, list item or other block in it becomes a line \
             of that text."
                .to_string(),
        ),
        DroppedConstruct::Bidi => (
            format!(
                "markdown import dropped {} from this field",
                some("bidi control", "bidi controls")
            ),
            "Content text admits no bidi formatting control (U+061C, U+200E, U+200F, U+202A–U+202E, \
             U+2066–U+2069); the renderer orders mixed-direction text by the Unicode bidi algorithm."
                .to_string(),
        ),
        DroppedConstruct::Math => (
            format!(
                "markdown import kept {} as written text in this field",
                some("equation", "equations")
            ),
            "An equation `$…$` or `$$…$$` reads as one in prose, outside a table cell, an image's alt text \
             and a footnote's note, around TeX that is not blank; write `\\$` for a dollar sign that opens none."
                .to_string(),
        ),
        DroppedConstruct::Table => (
            format!(
                "markdown import closed {} in this field where its HTML block ended",
                some("`<table>`", "`<table>`s")
            ),
            "An HTML table is one HTML block: a blank line inside it ends the table there, and what follows \
             reads as markdown. Close each `<table>` with `</table>` before any blank line."
                .to_string(),
        ),
        DroppedConstruct::TableRow => (
            format!(
                "markdown import padded {} in this field to its table's columns",
                some("table row", "table rows")
            ),
            "An HTML table is as wide as its widest row. A row leaves out each cell a `rowspan` above it covers, \
             and a row short of the width pads with empty cells."
                .to_string(),
        ),
        DroppedConstruct::TableSize => (
            format!(
                "markdown import cut {} in this field at {} cells",
                some("table", "tables"),
                quillmark_content::MAX_TABLE_CELLS
            ),
            "A table's rows times its columns stay within the bound; the rows past it drop. Split the table."
                .to_string(),
        ),
        DroppedConstruct::TableText => (
            format!(
                "markdown import dropped {} outside every cell of an HTML table in this field",
                some("run of text", "runs of text")
            ),
            "Text in an HTML table sits in a `<td>` or `<th>`; a `<caption>` and text between rows drop.".to_string(),
        ),
        DroppedConstruct::TableAttr { tag: element, attr } => attr_dropped(
            element,
            attr,
            match (element.as_str(), attr.as_str()) {
                ("table", "widths") => {
                    "`widths` is a whole number from 1 to 2^53 - 1, or `auto`, per column, such as `widths=\"2 1 auto\"`."
                        .to_string()
                }
                ("table", "aligns") => {
                    "`aligns` is `none`, `left`, `center` or `right` per column, such as `aligns=\"left none right\"`."
                        .to_string()
                }
                ("table" | "td" | "th", "align") => format!("`align` is {}.", listed(&TABLE_ALIGNS, "or")),
                ("table", _) => "`<table>` takes `widths`, `aligns` and `align`.".to_string(),
                (_, "colspan" | "rowspan") => format!("`{attr}` is a whole number from 1."),
                (_, "valign") => format!("`valign` is {}.", listed(&CELL_VALIGNS, "or")),
                ("td" | "th", _) => "`<td>` and `<th>` take `colspan`, `rowspan`, `align` and `valign`.".to_string(),
                _ => format!("`<{element}>` takes no attributes."),
            },
        ),
        DroppedConstruct::ElementAttr { element, attr }
            if attr == "ref" && quillmark_content::markdown::carrier::RESERVED.contains(&element.as_str()) =>
        {
            let (what, spelling) = match element == quillmark_content::markdown::carrier::ANCHOR {
                true => ("anchor", ("tag", "tags")),
                false => ("island", ("token", "tokens")),
            };
            let element = DroppedConstruct::Element(element.clone());
            (
                format!(
                    "markdown revise dropped {} naming no {what} the stored value holds, or one an earlier \
                     tag names, in this field",
                    some(&format!("`<{element}>` {}", spelling.0), &format!("`<{element}>` {}", spelling.1))
                ),
                format!(
                    "A `ref` names a {what} of the value the annotated read was taken from, once per {what}: \
                     copy the tag from the read, or remove it. An anchor without its tag keeps its place by \
                     the text around it."
                ),
            )
        }
        DroppedConstruct::ElementAttr { element, attr } => attr_dropped(
            &DroppedConstruct::Element(element.clone()).to_string(),
            attr,
            format!(
                "An attribute name is lowercase letters, digits and `_`, opening with a letter, once per tag, \
                 and none of `{}` or a name opening `on`.",
                RESERVED_ATTRS.join("`, `")
            ),
        ),
        DroppedConstruct::BadName(_) => (
            format!(
                "markdown import does not carry raw HTML: {} dropped",
                some_in_field(&format!("`<{tag}>` tag"), &format!("`<{tag}>` tags"))
            ),
            format!(
                "An element name after `qm-` is lowercase words of letters and digits joined by single `-`, \
                 opening with a letter. {TIGHT}"
            ),
        ),
        DroppedConstruct::Element(name) if name == quillmark_content::markdown::carrier::SPAN => (
            format!(
                "markdown import dropped {} in this field",
                some(&format!("`<{tag}>` tag"), &format!("`<{tag}>` tags"))
            ),
            format!(
                "A `<{tag}>` pair sets a `family` inside one line of text and closes with `</{tag}>` there; \
                 HTML reads `<{tag}/>` as an open tag. A family is a run's, never a block's. {BESIDE}"
            ),
        ),
        DroppedConstruct::Element(name) if name == quillmark_content::markdown::carrier::ISLAND => (
            format!(
                "markdown import dropped {} in this field",
                some(&format!("`<{tag}>` token"), &format!("`<{tag}>` tokens"))
            ),
            format!(
                "A `<{tag}>` token stands for an island of the stored value an annotated read was taken from, \
                 in prose: only a revise of that value reads it. A plain import, a table cell, a footnote and \
                 an image's alt text drop it."
            ),
        ),
        DroppedConstruct::Element(_) => (
            format!(
                "markdown import dropped {} in this field",
                some(&format!("`<{tag}>` tag"), &format!("`<{tag}>` tags"))
            ),
            format!("A `<{tag}>` tag carries nothing: what it wraps stays and the tags drop. Remove them."),
        ),
        DroppedConstruct::Tag(name) => match name.as_str() {
            "u" => (
                format!("markdown import dropped {} in this field", some("`<u>` tag", "`<u>` tags")),
                "A `<u>` takes no attributes and closes with `</u>` in the paragraph, heading, list item or \
                 table cell it opens in."
                    .to_string(),
            ),
            "pre" | "script" | "style" | "textarea" => (
                format!(
                    "markdown import does not carry raw HTML: {} dropped",
                    some_in_field(&format!("`<{tag}>` block"), &format!("`<{tag}>` blocks"))
                ),
                format!("A `<{tag}>` block drops whole, through its closing tag. Write code in a backtick fence."),
            ),
            _ => (
                format!(
                    "markdown import does not carry raw HTML: {} dropped",
                    some_in_field(&format!("`<{tag}>` tag"), &format!("`<{tag}>` tags"))
                ),
                "Raw HTML imports nothing but `<u>`, `<br>`, a `<table>` and `<qm-span>`. Where a line \
                 opens with any other tag, its block runs to the next blank line and drops whole, markdown \
                 included; a blank line under the tag line keeps what follows."
                    .to_string(),
            ),
        },
    }
}

/// Which encoding a [`Codec::decode_value`] failure came from, so a call site
/// can prefix its diagnostic per encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentDecodeError {
    NotContent(String),
    BadMarkdown(String),
}

impl ContentDecodeError {
    pub(crate) fn into_message(self) -> String {
        match self {
            ContentDecodeError::NotContent(m) | ContentDecodeError::BadMarkdown(m) => m,
        }
    }
}

/// A content codec: which authored string a [`Content`](quillmark_content::lines::model::Content) field accepts, and which
/// text a stored content projects back to. Both codecs also accept a canonical
/// content object, so a codec is exactly the string end of the round trip. The
/// declared type names one (`FieldType::codec`), and every schema-bound
/// content read and projection runs the codec it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Codec {
    /// A string is markdown, and a content projects back to markdown — lossy,
    /// content-only marks not surviving.
    Richtext,
    /// A string is literal text (`*hi*` stays four characters), and a content
    /// projects back verbatim.
    Text,
}

impl Codec {
    /// The codec a content object names: [`Codec::Text`] for one spelling
    /// `codec: "plain"`, [`Codec::Richtext`] for any other.
    pub(crate) fn of_object(value: &serde_json::Value) -> Codec {
        match value.get("codec").and_then(serde_json::Value::as_str) {
            Some("plain") => Codec::Text,
            _ => Codec::Richtext,
        }
    }

    /// The content codec this codec rests under.
    pub(crate) fn content(self) -> quillmark_content::model::Codec {
        match self {
            Codec::Richtext => quillmark_content::model::Codec::Rich,
            Codec::Text => quillmark_content::model::Codec::Plain,
        }
    }

    /// The schema keyword declaring this codec, carried verbatim on
    /// [`EditError::FieldDecode`].
    pub(crate) fn name(self) -> &'static str {
        match self {
            Codec::Richtext => edit::CODEC_RICHTEXT,
            Codec::Text => edit::CODEC_TEXT,
        }
    }

    /// How this codec reads a stored string, for the shape-mismatch message.
    fn string_form(self) -> &'static str {
        match self {
            Codec::Richtext => "a markdown string",
            Codec::Text => "a string",
        }
    }

    /// Decode a JSON value in either accepted encoding: a canonical content
    /// object, or an authored string read this codec's way, with what a
    /// markdown import dropped (none from an object or literal text). `None`
    /// when the value is neither an object nor a string; the call site handles
    /// those shapes and maps the error into its own type.
    pub(crate) fn decode_value(
        self,
        value: &serde_json::Value,
    ) -> Option<Result<quillmark_content::markdown::Imported, ContentDecodeError>> {
        let whole = |content| quillmark_content::markdown::Imported {
            content,
            warnings: Vec::new(),
        };
        match value {
            serde_json::Value::Object(_) => Some(
                Content::from_value_lenient(value)
                    .map(|read| whole(read.content))
                    .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            ),
            serde_json::Value::String(s) => Some(match self {
                Codec::Richtext => quillmark_content::markdown::from_markdown(s)
                    .map_err(|e| ContentDecodeError::BadMarkdown(e.to_string())),
                Codec::Text => Ok(whole(quillmark_content::markdown::plain(s))),
            }),
            _ => None,
        }
    }

    /// The shape-mismatch message for a value in neither accepted encoding,
    /// naming the shape that arrived. Every surface that refuses an unshaped
    /// richtext value spells this one sentence.
    pub(crate) fn unshaped_message(self, value: &serde_json::Value) -> String {
        format!(
            "expected a {} content object or {}, got {}",
            self.name(),
            self.string_form(),
            match value {
                serde_json::Value::Bool(_) => "a boolean",
                serde_json::Value::Number(_) => "a number",
                serde_json::Value::Array(_) => "an array",
                _ => "an unsupported value",
            }
        )
    }

    /// [`decode_value`](Self::decode_value) closed over the shapes a stored field
    /// can hold: a null is the empty content (null ≡ absent), and anything
    /// neither object nor string is a decode failure.
    pub(crate) fn decode_field(
        self,
        value: &serde_json::Value,
    ) -> Result<Content, ContentDecodeError> {
        match self.decode_value(value) {
            Some(result) => result.map(|imported| imported.content),
            None if value.is_null() => Ok(Content::empty_as(self.content())),
            None => Err(ContentDecodeError::NotContent(self.unshaped_message(value))),
        }
    }

    /// [`decode_field`](Self::decode_field) as a content value: an object
    /// keeps its ids, and a string imports with fresh ones.
    pub(crate) fn decode_content(
        self,
        value: &serde_json::Value,
    ) -> Result<Content, ContentDecodeError> {
        match value {
            serde_json::Value::Object(_) => Content::from_value(value)
                .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            serde_json::Value::String(s) => match self {
                Codec::Richtext => quillmark_content::markdown::from_markdown(s)
                    .map(|i| i.content)
                    .map_err(|e| ContentDecodeError::BadMarkdown(e.to_string())),
                Codec::Text => Ok(quillmark_content::markdown::plain(s)),
            },
            serde_json::Value::Null => Ok(Content::empty_as(self.content())),
            _ => Err(ContentDecodeError::NotContent(self.unshaped_message(value))),
        }
    }

    /// [`decode_content`](Self::decode_content) for a value the document
    /// already holds: an object reads as a stored value does
    /// ([`Content::from_value_lenient`]), the canonical form of what it spells.
    pub(crate) fn read_content(
        self,
        value: &serde_json::Value,
    ) -> Result<Content, ContentDecodeError> {
        match value {
            serde_json::Value::Object(_) => Content::from_value_lenient(value)
                .map(|read| read.content)
                .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            _ => self.decode_content(value),
        }
    }

    /// Project a content back to this codec's text: the inverse of its string
    /// encoding.
    pub(crate) fn project(self, content: &Content) -> String {
        match self {
            Codec::Richtext => quillmark_content::markdown::to_markdown(content),
            Codec::Text => quillmark_content::markdown::to_plaintext(content),
        }
    }
}

/// Why a richtext value did not become stored content, classified so each layer
/// words the failure in its own error vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RichtextValueError {
    /// An accepted encoding that failed to decode.
    Decode(ContentDecodeError),
    /// Neither a canonical content object nor a markdown string.
    Unshaped,
    /// The field is one line and the content spans more than one paragraph.
    NotSingleLine,
}

/// The contract for a richtext value that must *become* stored content: decode
/// either accepted encoding, enforce one line unless multiline, canonicalize, and hand back what
/// the import dropped. The strict typed write and the schema-literal companion
/// cache share it and differ only in the diagnostic they render from the error.
pub(crate) fn canonical_richtext_value(
    value: &serde_json::Value,
    inline: bool,
) -> Result<(serde_json::Value, Vec<ImportWarning>), RichtextValueError> {
    let (mut content, warnings) = match value {
        serde_json::Value::Object(_) => (
            Content::from_value(value).map_err(|e| {
                RichtextValueError::Decode(ContentDecodeError::NotContent(e.to_string()))
            })?,
            Vec::new(),
        ),
        serde_json::Value::String(s) => {
            let imported = quillmark_content::markdown::from_markdown(s).map_err(|e| {
                RichtextValueError::Decode(ContentDecodeError::BadMarkdown(e.to_string()))
            })?;
            (imported.content, imported.warnings)
        }
        _ => return Err(RichtextValueError::Unshaped),
    };
    if inline && !content.is_inline() {
        return Err(RichtextValueError::NotSingleLine);
    }
    content.codec = quillmark_content::model::Codec::Rich;
    Ok((content.to_value(), warnings))
}

/// Plain content less its trailing whitespace and breaks: a one-line `text`
/// value's form before its single-line check, so the newline a clip-chomped
/// YAML `|` scalar keeps is no second line. Its anchors hold, cut at the new
/// end.
pub(crate) fn trim_line_end(mut content: Content) -> Content {
    let keep = content.text.trim_end().len();
    if keep == content.text.len() {
        return content;
    }
    content.text.truncate(keep);
    content
        .blocks
        .truncate(content.text.matches('\n').count() + 1);
    cut_anchors(&mut content);
    content
}

/// Plain content as one paragraph, each break a space. The length holds, so
/// a rendered region's offsets still index the stored value, and so do its
/// anchors.
pub(crate) fn fold_line_breaks(mut content: Content) -> Content {
    content.text = content
        .text
        .chars()
        .map(|c| match c {
            '\n' | quillmark_content::model::HARD_BREAK => ' ',
            c => c,
        })
        .collect();
    content.blocks.truncate(1);
    if let Some(block) = content.blocks.first_mut() {
        block.kind = quillmark_content::model::BlockKind::Para;
        block.path.clear();
    }
    content.containers.clear();
    cut_anchors(&mut content);
    content
}

fn cut_anchors(content: &mut Content) {
    let len = content.len_scalars();
    for a in &mut content.anchors {
        a.start = a.start.min(len);
        a.end = a.end.min(len);
    }
    content.normalize();
}

pub(crate) mod align;
pub mod anchors;
pub mod assemble;
pub mod base;
pub mod card_id;
pub mod dto;
pub mod edit;
pub mod emit;
pub mod fences;
pub mod meta;
pub mod payload;
pub(crate) mod prescan;
pub mod revise;
pub mod wire;
pub mod write;
pub(crate) mod yaml_hints;

pub use anchors::AnchorSelector;
pub use base::{HASH_TAG, StaleBase, content_hash, value_hash};
pub use dto::{
    STORAGE_V0_124_0, STORAGE_V0_129_0, STORAGE_V0_131_0, STORAGE_V0_134_0, STORAGE_V0_135_0,
    StorageError, StoredDocument, peek_storage_version,
};
pub use edit::{CardMut, EditError, Unchecked};
pub use emit::{AnnotatedMarkdown, DocumentAnchor};
pub use meta::{CardKindError, is_valid_kind_name, validate_composable_kind};
pub use payload::{MetaKey, Payload, PayloadItem};
/// Returned by [`Document::reattach`], so nameable from here.
pub use quillmark_content::anchors::Reattached;
/// Carried by [`EditError::Import`], so nameable from here.
pub use quillmark_content::markdown::ImportError;
/// Answered by [`Card::annotated`] and the content crate's anchor reads, so nameable from here.
pub use quillmark_content::markdown::{AnchorRead, Annotated};
/// Taken by [`dropped_construct`], so nameable from here.
pub use quillmark_content::markdown::{DroppedConstruct, ImportWarning};
pub use revise::DocumentRevised;
// Reachable through `Payload::nested_comments`, so nameable from here.
pub use prescan::NestedComment;
pub use wire::{CardWire, PayloadItemWire, WireError};
pub use write::{
    Edit, Located, MAX_CHANGE_STEPS, Receipt, Target, WriteOptions, check_change,
    check_content_value,
};

/// Where Quillmark Markdown departs from plain Markdown and YAML, as rules
/// surfaced verbatim to LLM/MCP consumers and to the bindings. The single
/// source of truth: bindings call in rather than re-stating the rules.
pub const MARKDOWN_RULES: &str = "Quillmark Markdown rules:
\u{2022} Block opener and closer are EXACTLY `~~~` (three tildes). The opener's info string is ignored \u{2014} `~~~card-yaml` is accepted and re-emits as a bare `~~~`.
\u{2022} A blank line must precede every `~~~` block opener (unless it is line 1), and the opener must be at column zero (no leading spaces). An indented `~~~` is an ordinary code block, not a card.
\u{2022} The first block is the root and MUST contain `$quill: <name>` or `$quill: <name>@<selector>`. Its `$kind` is `main` by position \u{2014} an explicit `$kind: main` is accepted but not required. Every later block is a composable card and MUST declare `$kind: <card_kind>`.
\u{2022} Reserved `$`-keys: `$quill`, `$kind`, `$id`, `$ext`, `$seed`. User fields use lowercase snake_case.
\u{2022} A composable card's `$id` line names it across edits: keep it when you edit or move the card, leave it out of a new card (one is minted), and never copy it onto a second card.
\u{2022} Prose body is the text after a block's closing `~~~`, up to the next opener or EOF. To include a literal fenced code block in prose, use a backtick fence (```); any column-zero `~~~` block is parsed as card metadata.
\u{2022} A blank or null value (`field:`, `field: null`, `field: ~`) is the same as omitting the field: it falls back to the field's default, else its blank. `field: \"\"` is kept as written, an explicit empty value.
\u{2022} Plain-scalar values cannot start with `*` or `&` (YAML alias/anchor markers) and cannot contain `: ` (colon-space). For markdown emphasis, embedded colons, or other special prefixes, quote the value: `field: '**bold**'` or `field: \"Name: subtitle\"`. Multi-line values use `|-`, not multi-line quoted scalars.
\u{2022} In a body or richtext value, `<qm-span family=\"<family>\">text</qm-span>` sets a run's font family. Pairs nest, the innermost winning and the outer resuming past it. Name only a family the host's font library supplies: any other keeps the surrounding font and warns.
\u{2022} Markdown read from an annotated read carries `<qm-anchor ref=\"\u{2026}\"></qm-anchor>` tags and `<qm-island ref=\"\u{2026}\"></qm-island>` tokens. Keep each tag before the text it marks, moving it with that text; drop a tag only with its text. Keep each token where its island belongs: it stands for a table, image or equation kept as stored. Never write a `ref` the read did not hold.";

#[cfg(test)]
mod tests;

/// The record of one load: the [`Document`] and any non-fatal warnings.
/// Returned by both [`Document::parse`] and the bound
/// [`Quill::parse`](crate::quill::Quill::parse), whose `warnings` also carry
/// the `conform::*` ones. Warnings live here and only here: `Document` is the
/// value, `Parsed` the load event.
#[derive(Debug)]
#[must_use = "carries parse warnings; read `.document`/`.warnings` or bind it"]
pub struct Parsed {
    pub document: Document,
    pub warnings: Vec<Diagnostic>,
}

/// A parse ahead of a revise: the [`Parsed`] document, its cards unsettled,
/// and each body's markdown, the main card's first.
#[derive(Debug)]
pub(crate) struct Revising {
    pub parsed: Parsed,
    pub bodies: Vec<String>,
}

/// A single card-yaml block (root or composable). `body` is the [`Content`]
/// of the prose after the closing fence: the empty content when none follows.
/// Markdown is a projection: [`Card::body_markdown`] re-emits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    payload: Payload,
    body: Content,
    hashes: Hashes,
    #[cfg(test)]
    lines: Lowered,
}

/// The body's line form for the tests that read it, lowered when first read
/// and no part of equality.
#[cfg(test)]
#[derive(Clone, Default)]
struct Lowered(std::sync::OnceLock<Lines>);

#[cfg(test)]
impl PartialEq for Lowered {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[cfg(test)]
impl std::fmt::Debug for Lowered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Lowered")
    }
}

/// The hash a write left on the body and on each richtext field it wrote,
/// so the next write reads it rather than hashing the value again. A body
/// set forgets the body's, and any payload write every field's. Derived from
/// the values, so it takes no part in equality.
#[derive(Clone, Default)]
pub(crate) struct Hashes {
    body: Option<String>,
    fields: std::collections::HashMap<String, String>,
}

impl Hashes {
    pub(crate) fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub(crate) fn field(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }
}

impl PartialEq for Hashes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Hashes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hashes")
    }
}

/// What a card body is built from: a line-form value migrates, minting its
/// ids, and a content value lands as it is.
pub(crate) trait IntoBody {
    fn into_body(self) -> Content;
}

impl IntoBody for Lines {
    fn into_body(self) -> Content {
        quillmark_content::migrate::from_lines(&self)
    }
}

impl IntoBody for Content {
    fn into_body(self) -> Content {
        self
    }
}

impl Card {
    /// Create a `Card` from its parts without validation.
    pub(crate) fn from_parts(payload: Payload, body: impl IntoBody) -> Self {
        let body = body.into_body();
        Self {
            payload,
            body,
            hashes: Hashes::default(),
            #[cfg(test)]
            lines: Lowered::default(),
        }
    }

    pub fn quill(&self) -> Option<&QuillReference> {
        self.payload.quill()
    }

    pub fn kind(&self) -> Option<&str> {
        self.payload.kind()
    }

    /// A composable card's `$id`: a [card id](card_id::is_card_id) no other
    /// card of its document holds, minted where none was given. `None` on the
    /// main card, and on a card not yet placed that spells none.
    pub fn id(&self) -> Option<&str> {
        self.payload.id()
    }

    /// Opaque `$ext` map for out-of-band extension data (UI editor state,
    /// agent annotations, …). Carried through Markdown and storage DTO
    /// round-trips; never emitted into the plate JSON backends consume.
    pub fn ext(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.payload.ext()
    }

    /// The card's card-yaml storage as a read view; writes go through the verbs
    /// [`Payload`] names.
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub(crate) fn payload_mut(&mut self) -> &mut Payload {
        self.hashes.fields.clear();
        &mut self.payload
    }

    /// The card body: the stored content value. For the markdown projection
    /// use [`Card::body_markdown`].
    pub fn body(&self) -> &Content {
        &self.body
    }

    /// The body lowered to the line form the markdown codecs and the renderers
    /// read.
    #[cfg(test)]
    pub(crate) fn body_lines(&self) -> &Lines {
        self.lines
            .0
            .get_or_init(|| quillmark_content::migrate::to_lines(&self.body))
    }

    /// The card body rendered back to its markdown projection. This is a
    /// derived view (`export ∘ body`), not stored state; a `Document` round-trip
    /// therefore canonicalizes the body (e.g. `__b__` → `**b**`).
    pub fn body_markdown(&self) -> String {
        quillmark_content::markdown::to_markdown(&self.body)
    }

    pub(crate) fn set_body(&mut self, body: Content) {
        #[cfg(test)]
        {
            self.lines = Lowered::default();
        }
        self.hashes.body = None;
        self.body = body;
    }

    /// Note the hash a write left at `field`, `None` the body.
    pub(crate) fn remember_hash(&mut self, field: Option<&str>, hash: String) {
        match field {
            None => self.hashes.body = Some(hash),
            Some(name) => {
                self.hashes.fields.insert(name.to_string(), hash);
            }
        }
    }

    /// Writes a line-form body over the stored one, keeping the ids of what it
    /// kept.
    pub(crate) fn set_body_lines(&mut self, lines: Lines) {
        let body = quillmark_content::diff::lift(&self.body, &lines);
        self.set_body(body);
    }

    /// Read a content-valued user field back through `codec`: the field-level
    /// twin of [`Card::body`].
    ///
    /// - `None`: the field is absent.
    /// - `Some(Ok(content))`: decoded content, from either of the codec's
    ///   encodings — a stored string and an already-canonical content object read
    ///   back the same.
    /// - `Some(Err(_))`: the field is present but decodes under neither (e.g. a
    ///   bare number an opaque `store_field` wrote).
    ///
    /// A `Document` carries no schema, so the caller names the codec the field is
    /// declared at; the schema-bound door is
    /// [`TypedReader::get_content`](crate::reader::TypedReader::get_content).
    #[cfg(test)]
    pub(crate) fn field_content(
        &self,
        name: &str,
        codec: Codec,
    ) -> Option<Result<Lines, ContentDecodeError>> {
        Some(
            codec
                .decode_field(self.payload.get(name)?.as_json())
                .map(|c| quillmark_content::migrate::to_lines(&c)),
        )
    }

    /// The text projection of a content-valued field (`project ∘ decode`),
    /// carrying [`field_content`](Card::field_content)'s `None`/`Ok`/`Err`.
    #[cfg(test)]
    pub(crate) fn field_text(
        &self,
        name: &str,
        codec: Codec,
    ) -> Option<Result<String, ContentDecodeError>> {
        Some(
            codec
                .decode_field(self.payload.get(name)?.as_json())
                .map(|c| codec.project(&c)),
        )
    }
}

/// A parsed, per-kind **seed overlay**: the sparse fields (and optional body) a
/// newly-added card of a given kind starts with. Built from a `$seed[<kind>]`
/// entry of the main card's [`Card::seed`] map, or a kind's `seed:` in
/// `Quill.yaml`, via [`SeedOverlay::from_json`], and committed by
/// [`crate::quill::Quill::seed_card`], a document's overlay replacing its
/// kind's seed whole.
/// The reserved inner key `$body` carries the body override; every other user
/// field becomes an entry, while any other `$`-prefixed key is reserved and
/// dropped. A present-null entry is kept, and `seed_card` reads it as absent.
/// Serializes back to the `$seed[<kind>]` shape, `$body` last.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SeedOverlay {
    /// Field-value overrides, keyed by field name.
    pub fields: indexmap::IndexMap<String, crate::value::QuillValue>,
    /// Body override, when the overlay declares a `$body` string.
    pub body: Option<String>,
}

impl SeedOverlay {
    /// Parse an overlay from a `$seed[<kind>]` JSON value, `None` when it is not
    /// a mapping:
    /// `doc.main().seed().and_then(|m| m.get(kind)).and_then(SeedOverlay::from_json)`.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        value.as_object().map(Self::from_json_map)
    }

    /// A non-string `$body` is ignored: no body override.
    fn from_json_map(map: &serde_json::Map<String, serde_json::Value>) -> Self {
        let mut fields = indexmap::IndexMap::new();
        let mut body = None;
        for (key, value) in map {
            if key == "$body" {
                if let Some(s) = value.as_str() {
                    body = Some(s.to_string());
                }
            } else if key.starts_with('$') {
                continue;
            } else {
                fields.insert(
                    key.clone(),
                    crate::value::QuillValue::from_json(value.clone()),
                );
            }
        }
        SeedOverlay { fields, body }
    }
}

impl Serialize for SeedOverlay {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let len = self.fields.len() + usize::from(self.body.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        for (key, value) in &self.fields {
            map.serialize_entry(key, value.as_json())?;
        }
        if let Some(body) = &self.body {
            map.serialize_entry("$body", body)?;
        }
        map.end()
    }
}

/// A fully-parsed Quillmark document. Serde routes through [`StoredDocument`].
///
/// Parse-time warnings are *not* document state: they ride out-of-band on
/// [`Parsed`] from [`Document::parse`], the single owner. Equality and the
/// storage DTO therefore cover only structural content (`main` and `cards`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "StoredDocument", try_from = "StoredDocument")]
pub struct Document {
    main: Card,
    cards: Vec<Card>,
}

impl Document {
    /// Create a blank document: a main card carrying only `$quill`, an empty
    /// body, and no composable cards. The programmatic blank canvas: every
    /// schema field is absent and resolves at render time (`default`, else
    /// the field's blank), so nothing the caller did not set reaches the
    /// output. For one card per declared kind, use `Quill::seed_document`.
    pub fn new(quill: QuillReference) -> Self {
        let mut payload = Payload::new();
        payload.set_quill(quill);
        // Parsed main cards always carry `$kind: main` (the parser normalizes
        // it in); match that shape so a blank document round-trips equal.
        payload.set_kind("main");
        Self {
            main: Card::from_parts(payload, Lines::empty()),
            cards: Vec::new(),
        }
    }

    /// Create a `Document` from a pre-built main card and composable cards.
    /// `main` must carry `$quill`; composable cards must carry `$kind` and must
    /// not carry `$quill` or `$seed`.
    ///
    /// The invariants are `debug_assert`s because no caller can break them.
    /// This is crate-internal; every public door that places a card gates it
    /// ([`Unchecked::insert_card`](crate::document::Unchecked::insert_card),
    /// parse, and
    /// `TryFrom<StoredDocument>` for external data, which checks all three and
    /// returns `StorageError::Malformed`), and a placed card is reachable only
    /// as a [`CardMut`], which carries no whole-card assignment.
    pub(crate) fn from_main_and_cards(main: Card, mut cards: Vec<Card>) -> Self {
        debug_assert!(main.quill().is_some(), "main card must carry `$quill`");
        debug_assert!(
            cards.iter().all(|c| c.kind().is_some()),
            "composable cards must carry `$kind`"
        );
        debug_assert!(
            cards.iter().all(|c| c.quill().is_none()),
            "composable cards must not carry `$quill`"
        );
        debug_assert!(
            cards.iter().all(|c| c.seed().is_none()),
            "composable cards must not carry `$seed`"
        );
        card_id::settle(&mut cards, &std::collections::HashSet::new());
        Self { main, cards }
    }

    /// Read a stored row ([`StoredDocument`] JSON) into a [`Parsed`]: the
    /// [`Document`], and a [`noncanonical_read`] warning at each body or field
    /// whose content value the read rewrote to its canonical form. Fails as
    /// deserializing a `Document` from `json` fails.
    pub fn from_stored(json: &str) -> Result<Parsed, serde_json::Error> {
        let stored: StoredDocument = serde_json::from_str(json)?;
        dto::from_stored(stored).map_err(serde::de::Error::custom)
    }

    /// Parse card-yaml Markdown into a [`Parsed`]: the [`Document`] plus any
    /// non-fatal warnings. The single parse entry; a caller that wants only the
    /// document writes `Document::parse(md)?.document`. Errors on malformed
    /// YAML, a missing root `$quill`, an over-size input, and the other
    /// [`ParseError`] variants.
    #[doc(alias = "from_markdown")]
    pub fn parse(markdown: &str) -> Result<Parsed, ParseError> {
        let mut parsed = Self::parse_unsettled(markdown)?;
        let reserved = std::collections::HashSet::new();
        card_id::settle_reporting(&mut parsed.document.cards, &reserved, &mut parsed.warnings);
        Ok(parsed)
    }

    /// [`parse`](Self::parse) with each card holding the `$id` it spells, or
    /// none, for a revise to settle against the ids it started from.
    pub(crate) fn parse_unsettled(markdown: &str) -> Result<Parsed, ParseError> {
        assemble::decompose_with_warnings(markdown, import_markdown)
            .map(|(document, warnings, _)| Parsed { document, warnings })
    }

    /// [`parse_unsettled`](Self::parse_unsettled) for a revise: each body's
    /// `qm-island` tokens drop silently, the revise resolving them against the
    /// stored card it aligns the body's card to, and each body's markdown is
    /// kept for it.
    pub(crate) fn parse_revising(markdown: &str) -> Result<Revising, ParseError> {
        let (document, warnings, bodies) = assemble::decompose_with_warnings(
            markdown,
            quillmark_content::markdown::from_markdown_unresolved,
        )?;
        Ok(Revising {
            parsed: Parsed { document, warnings },
            bodies,
        })
    }

    pub fn main(&self) -> &Card {
        &self.main
    }

    pub fn main_mut(&mut self) -> CardMut<'_> {
        CardMut::new(&mut self.main)
    }

    /// The root block's `$quill` reference, which parse validates present.
    pub fn quill_reference(&self) -> QuillReference {
        self.main
            .quill()
            .cloned()
            .expect("root block's $quill is validated at parse time")
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    /// A single composable card by index: the immutable twin of
    /// [`card_mut`](Document::card_mut). `None` when out of range.
    pub fn card(&self, index: usize) -> Option<&Card> {
        self.cards.get(index)
    }

    /// The index of the composable card whose `$id` is `id`.
    pub fn card_index(&self, id: &str) -> Option<usize> {
        self.cards.iter().position(|card| card.id() == Some(id))
    }

    /// A composable card by its `$id`, which it keeps wherever it moves.
    pub fn card_by_id(&self, id: &str) -> Option<&Card> {
        self.card_index(id).map(|index| &self.cards[index])
    }

    pub(crate) fn main_card_mut(&mut self) -> &mut Card {
        &mut self.main
    }

    pub(crate) fn cards_vec_mut(&mut self) -> &mut Vec<Card> {
        &mut self.cards
    }

    /// Serialize to the JSON wire shape consumed by backend plates. This is
    /// the **only** place in `quillmark-core` that produces this shape:
    ///
    /// ```json
    /// {
    ///   "$quill": "<ref>",
    ///   "$body": { "text": "…", "blocks": [...], "containers": [...], "marks": [...], "islands": [...] },
    ///   "$cards": [{ "$kind": "<tag>", "$body": <content>, "<field>": <value>, ... }],
    ///   "<field>": <value>, ...
    /// }
    /// ```
    ///
    /// `$body` (global and per-card) is canonical Content-JSON: the content as
    /// a nested object, not a markdown string. Richtext payload fields likewise
    /// cross as content objects (committed at coercion time).
    ///
    /// `$`-prefixed keys carry document-level metadata (quill ref, body
    /// text, card list, card kind). User payload fields stay flat at the
    /// root: they cannot collide with `$` keys because user field names are
    /// never `$`-prefixed (they match `[A-Za-z_][A-Za-z0-9_]*`).
    ///
    /// Body presence is the caller's decision: the root carries `$body` iff
    /// `main_body`, and card *i* iff `card_bodies` is `None` (all present) or
    /// `card_bodies[i]` holds. The schema-gated render plate
    /// (`QuillConfig::compile_data`) passes the body-enabled bit it already
    /// resolved per card, so a card whose kind enables no body carries no
    /// `$body`, and the decision is never re-derived from the serialized plate.
    /// `Document` stays schema-free: it receives the decision, not a schema.
    pub(crate) fn to_plate_json_gated(
        &self,
        main_body: bool,
        card_bodies: Option<&[bool]>,
    ) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        map.insert(
            "$quill".to_string(),
            serde_json::Value::String(self.quill_reference().to_string()),
        );

        // The seam carries the body as canonical Content-JSON: a nested content
        // object, byte-identical to `to_canonical_json`, never a lossy markdown
        // string. The markdown projection is `body_markdown`.
        if main_body {
            map.insert("$body".to_string(), self.main.body().to_value());
        }

        let cards_array: Vec<serde_json::Value> = self
            .cards
            .iter()
            .enumerate()
            .map(|(i, card)| {
                let mut card_map = serde_json::Map::new();
                if let Some(kind) = card.kind() {
                    card_map.insert(
                        "$kind".to_string(),
                        serde_json::Value::String(kind.to_string()),
                    );
                }
                // `$body` iff the caller's schema defines a body for this card.
                if card_bodies.map_or(true, |f| f.get(i).copied().unwrap_or(true)) {
                    card_map.insert("$body".to_string(), card.body().to_value());
                }
                for (key, value) in card.payload.iter() {
                    card_map.insert(key.clone(), value.as_json().clone());
                }
                serde_json::Value::Object(card_map)
            })
            .collect();

        map.insert("$cards".to_string(), serde_json::Value::Array(cards_array));

        for (key, value) in self.main.payload.iter() {
            map.insert(key.clone(), value.as_json().clone());
        }

        serde_json::Value::Object(map)
    }
}
