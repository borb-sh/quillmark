//! Parsing and typed in-memory model for Quillmark card-yaml documents.
//!
//! A [`Document`] holds a root [`Card`] plus ordered composable cards. The
//! format is specified in
//! [markdown-spec.md](https://github.com/borb-sh/quillmark/blob/main/prose/references/markdown-spec.md).

use serde::{Deserialize, Serialize};

use quillmark_automerge::content::lines::export::ExportLoss;
use quillmark_automerge::content::lines::import::{Imported, from_markdown as import_markdown};
use quillmark_automerge::content::lines::model::Lines;
use quillmark_automerge::content::model::Content;
use quillmark_automerge::content::units::HostUnits;

use crate::error::ParseError;
use crate::error::{Diagnostic, Severity};
use crate::version::QuillReference;

/// The stored spelling of a line-form value: under the ids of what it kept
/// from `old`, the value it is written over, or minted fresh where there is
/// none.
#[cfg(test)]
pub(crate) fn stored_over(old: Option<&serde_json::Value>, lines: &Lines) -> serde_json::Value {
    lifted_over(old, lines).to_value()
}

/// [`stored_over`] under `codec`, whatever `old` held.
pub(crate) fn stored_as(
    old: Option<&serde_json::Value>,
    lines: &Lines,
    codec: quillmark_automerge::content::model::Codec,
) -> serde_json::Value {
    let mut content = lifted_over(old, lines);
    content.codec = codec;
    content.to_value()
}

fn lifted_over(old: Option<&serde_json::Value>, lines: &Lines) -> Content {
    let held = old.filter(|v| v.is_object()).and_then(|v| Content::from_value_lenient(v).ok()).map(|read| read.content);
    match held {
        Some(held) => quillmark_automerge::content::diff::lift(&held, lines),
        None => quillmark_automerge::content::migrate::from_lines(lines),
    }
}

pub(crate) fn import_body(md: &str) -> Result<Lines, ImportError> {
    import_markdown(md).map(|imported| imported.content)
}

/// [`import_body`], pushing a [`dropped_construct`] warning at `at` for each
/// construct the import drops.
pub(crate) fn import_body_at(
    md: &str,
    at: &crate::path::DocPath,
    warnings: &mut Vec<Diagnostic>,
) -> Result<Lines, ImportError> {
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
    Diagnostic::new(Severity::Warning, "a stored content value is not in canonical form, and reads as it".to_string())
        .with_code(NONCANONICAL_READ.to_string())
        .with_hint("store the document again to write its canonical form".to_string())
}

/// The diagnostic code a markdown export's loss rides.
pub const LOST_CONSTRUCT: &str = "export::lost_construct";

/// The warning a markdown export owes what it does not spell: one
/// [`ExportLoss`] of a content, read by
/// [`to_markdown_checked`](quillmark_automerge::content::lines::export::to_markdown_checked).
/// `construct` is the mark's wire type, the island's, or `text`; a mark's
/// `start` and `end` are its range in the content's text.
pub fn lost_construct(loss: ExportLoss) -> Diagnostic {
    let mut args = std::collections::BTreeMap::new();
    let (message, hint) = match loss {
        ExportLoss::Mark {
            kind,
            start,
            end,
            now,
        } => {
            args.insert("construct".to_string(), kind.tag().into());
            args.insert("start".to_string(), start.into());
            args.insert("end".to_string(), end.into());
            let message = match now {
                Some(_) => format!(
                    "markdown export writes a `{}` mark over other text than it holds",
                    kind.tag()
                ),
                None => format!("markdown export drops a `{}` mark", kind.tag()),
            };
            (
                message,
                "A `*`, `_` or `~~` delimiter never sits against whitespace, so such a mark is written over \
                 its text without the whitespace at its edges; a mark markdown cannot nest is dropped."
                    .to_string(),
            )
        }
        ExportLoss::Island { island_type, .. } => {
            args.insert("construct".to_string(), island_type.as_str().into());
            (
                format!("markdown export writes a `{}` that reads back otherwise", island_type.as_str()),
                "The island holds a prop its markdown spelling cannot carry, such as edge whitespace in an \
                 image's alt text."
                    .to_string(),
            )
        }
        ExportLoss::Text => {
            args.insert("construct".to_string(), "text".into());
            (
                "markdown export writes text or blocks that read back otherwise".to_string(),
                "Markdown spells no empty line beside other content, and writes a table only as a block of \
                 its own."
                    .to_string(),
            )
        }
    };
    Diagnostic::new(Severity::Warning, message)
        .with_code(LOST_CONSTRUCT.to_string())
        .with_hint(hint)
        .with_args(args)
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
    use quillmark_automerge::content::lines::carrier::RESERVED_ATTRS;
    use quillmark_automerge::content::lines::island::{CELL_VALIGNS, TABLE_ALIGNS};
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
                quillmark_automerge::content::MAX_NOTE_COPIES
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
                quillmark_automerge::content::MAX_TABLE_CELLS
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
            if attr == "ref" && quillmark_automerge::content::lines::carrier::RESERVED.contains(&element.as_str()) =>
        {
            let (what, spelling) = match element == quillmark_automerge::content::lines::carrier::ANCHOR {
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
        DroppedConstruct::Element(name) if name == quillmark_automerge::content::lines::carrier::SPAN => (
            format!(
                "markdown import dropped {} in this field",
                some(&format!("`<{tag}>` tag"), &format!("`<{tag}>` tags"))
            ),
            format!(
                "A `<{tag}>` pair sets a `family` inside one line of text and closes with `</{tag}>` there; \
                 HTML reads `<{tag}/>` as an open tag. A family is a run's, never a block's. {BESIDE}"
            ),
        ),
        DroppedConstruct::Element(name) if name == quillmark_automerge::content::lines::carrier::ISLAND => (
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

/// A content codec: which authored string a [`Content`](quillmark_automerge::content::lines::model::Content) field accepts, and which
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
    pub(crate) fn content(self) -> quillmark_automerge::content::model::Codec {
        match self {
            Codec::Richtext => quillmark_automerge::content::model::Codec::Rich,
            Codec::Text => quillmark_automerge::content::model::Codec::Plain,
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
    ) -> Option<Result<Imported, ContentDecodeError>> {
        let whole = |content| Imported {
            content,
            warnings: Vec::new(),
        };
        match value {
            serde_json::Value::Object(_) => Some(
                quillmark_automerge::content::migrate::lines_from_value(value)
                    .map(whole)
                    .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            ),
            serde_json::Value::String(s) => Some(match self {
                Codec::Richtext => {
                    import_markdown(s).map_err(|e| ContentDecodeError::BadMarkdown(e.to_string()))
                }
                Codec::Text => Ok(whole(quillmark_automerge::content::migrate::to_lines(&quillmark_automerge::content::markdown::from_plaintext(s)))),
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
    ) -> Result<Lines, ContentDecodeError> {
        match self.decode_value(value) {
            Some(result) => result.map(|imported| imported.content),
            None if value.is_null() => Ok(Lines::empty()),
            None => Err(ContentDecodeError::NotContent(self.unshaped_message(value))),
        }
    }

    /// [`decode_field`](Self::decode_field) as a content value: an object
    /// keeps its ids, and a string imports with fresh ones.
    pub(crate) fn decode_content(self, value: &serde_json::Value) -> Result<Content, ContentDecodeError> {
        match value {
            serde_json::Value::Object(_) => {
                Content::from_value(value).map_err(|e| ContentDecodeError::NotContent(e.to_string()))
            }
            serde_json::Value::String(s) => match self {
                Codec::Richtext => quillmark_automerge::content::markdown::from_markdown(s)
                    .map(|i| i.content)
                    .map_err(|e| ContentDecodeError::BadMarkdown(e.to_string())),
                Codec::Text => Ok(quillmark_automerge::content::markdown::plain(s)),
            },
            serde_json::Value::Null => Ok(Content::empty_as(self.content())),
            _ => Err(ContentDecodeError::NotContent(self.unshaped_message(value))),
        }
    }

    /// [`decode_content`](Self::decode_content) for a value the document
    /// already holds: an object reads as a stored value does
    /// ([`Content::from_value_lenient`]), the canonical form of what it spells.
    pub(crate) fn read_content(self, value: &serde_json::Value) -> Result<Content, ContentDecodeError> {
        match value {
            serde_json::Value::Object(_) => Content::from_value_lenient(value)
                .map(|read| read.content)
                .map_err(|e| ContentDecodeError::NotContent(e.to_string())),
            _ => self.decode_content(value),
        }
    }

    /// Project a content back to this codec's text: the inverse of its string
    /// encoding.
    pub(crate) fn project(self, content: &Lines) -> String {
        match self {
            Codec::Richtext => quillmark_automerge::content::lines::export::to_markdown(content),
            Codec::Text => quillmark_automerge::content::markdown::to_plaintext(&quillmark_automerge::content::migrate::from_lines(content)),
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
            Content::from_value(value)
                .map_err(|e| RichtextValueError::Decode(ContentDecodeError::NotContent(e.to_string())))?,
            Vec::new(),
        ),
        serde_json::Value::String(s) => {
            let imported = quillmark_automerge::content::markdown::from_markdown(s)
                .map_err(|e| RichtextValueError::Decode(ContentDecodeError::BadMarkdown(e.to_string())))?;
            (imported.content, imported.warnings)
        }
        _ => return Err(RichtextValueError::Unshaped),
    };
    if inline && !content.is_inline() {
        return Err(RichtextValueError::NotSingleLine);
    }
    content.codec = quillmark_automerge::content::model::Codec::Rich;
    Ok((content.to_value(), warnings))
}

/// Plain content less its trailing whitespace: a one-line `text` value's form
/// before its single-line check, so the newline a clip-chomped YAML `|` scalar
/// keeps is no second line. Its anchors hold, cut at the new end.
pub(crate) fn trim_line_end(
    content: quillmark_automerge::content::lines::model::Lines,
) -> quillmark_automerge::content::lines::model::Lines {
    let text = quillmark_automerge::content::lines::export::to_plaintext(&content);
    let trimmed = text.trim_end();
    if trimmed.len() == text.len() {
        content
    } else {
        with_anchors_of(&content, quillmark_automerge::content::lines::import::from_plaintext(trimmed))
    }
}

/// Plain content on one line, each line break a space. The length holds, so a
/// rendered region's offsets still index the stored value, and so do its
/// anchors.
pub(crate) fn fold_line_breaks(
    content: quillmark_automerge::content::lines::model::Lines,
) -> quillmark_automerge::content::lines::model::Lines {
    let text = quillmark_automerge::content::lines::export::to_plaintext(&content);
    with_anchors_of(&content, quillmark_automerge::content::lines::import::from_plaintext(&text.replace('\n', " ")))
}

/// `to` holding `from`'s anchors, each cut to `to`'s length.
fn with_anchors_of(
    from: &quillmark_automerge::content::lines::model::Lines,
    to: quillmark_automerge::content::lines::model::Lines,
) -> quillmark_automerge::content::lines::model::Lines {
    use quillmark_automerge::content::lines::model::{Mark, MarkKind};
    let mut to = to.into_content();
    let len = to.text.chars().count();
    to.marks.extend(from.marks.iter().filter(|m| matches!(m.kind, MarkKind::Anchor { .. })).map(|m| {
        Mark::new(m.start.min(len), m.end.min(len), m.kind.clone())
    }));
    to.into()
}

pub(crate) mod align;
pub(crate) mod held;
mod land;
pub mod anchors;
pub mod assemble;
pub mod card_id;
mod dto;
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
pub use dto::{
    STORAGE_V0_134_0, STORAGE_V0_135_0, StorageError, StoredDocument, peek_storage_version,
};
pub use edit::{CardMut, EditError, Unchecked};
pub use emit::{AnnotatedMarkdown, DocumentAnchor};
pub use meta::{CardKindError, is_valid_kind_name, validate_composable_kind};
pub use payload::{FieldValue, MetaKey, Payload, PayloadItem};
/// Returned by [`Document::reattach`], so nameable from here.
pub use quillmark_automerge::content::anchors::Reattached;
/// Answered by [`Card::annotated`] and the content crate's anchor reads, so nameable from here.
pub use quillmark_automerge::content::markdown::{AnchorRead, Annotated};
/// Carried by [`EditError::Import`], so nameable from here.
pub use quillmark_automerge::content::markdown::ImportError;
/// Taken by [`dropped_construct`], so nameable from here.
pub use quillmark_automerge::content::markdown::{DroppedConstruct, ImportWarning};
pub use revise::DocumentRevised;
/// Answered by [`Document::heads`] and taken by [`Document::save_since`], so nameable from here.
pub use quillmark_automerge::model::Heads;
/// Taken and answered by [`Document::generate_sync_message`] and
/// [`Document::receive_sync_message`], so nameable from here.
pub use quillmark_automerge::model::{SyncMessage, SyncState};
/// Carried by [`EditError::bad_live_bytes`], so nameable from here.
pub use quillmark_automerge::model::LoadError;
// Reachable through `Payload::nested_comments`, so nameable from here.
pub use prescan::NestedComment;
pub use wire::{CardWire, PayloadItemWire, WireError};
pub use write::{Edit, Located, MAX_CHANGE_STEPS, Receipt, Target, WriteOptions, check_change, check_content_value};

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

/// Why [`Document::from_stored_any`] refuses.
#[derive(Debug, thiserror::Error)]
pub enum StoredError {
    /// Automerge bytes that [`Document::load`] refuses.
    #[error(transparent)]
    Bytes(EditError),
    /// A stored row that [`Document::from_stored`] refuses.
    #[error(transparent)]
    Row(serde_json::Error),
}

/// A parse ahead of a revise: the cards, their ids unsettled, the parse's
/// warnings, and each body's markdown, the main card's first.
#[derive(Debug)]
pub(crate) struct Revising {
    pub cards: Snapshot,
    pub warnings: Vec<Diagnostic>,
    pub bodies: Vec<String>,
}

/// A single card-yaml block (root or composable). `body` is the [`Content`]
/// of the prose after the closing fence: the empty content when none follows.
/// Markdown is a projection: [`Card::body_markdown`] re-emits it.
#[derive(Debug, Clone)]
pub struct Card {
    payload: Payload,
    body: Content,
    /// `body` lowered to the line form the codecs and renderers read, made
    /// when first read.
    lines: Lowered,
}

/// A body's line form, lowered when first read: a write that no renderer
/// reads pays nothing for it. Derived from the body, so it takes no part in
/// equality.
#[derive(Clone, Default)]
struct Lowered(std::sync::OnceLock<Lines>);

impl PartialEq for Lowered {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Lowered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Lowered")
    }
}

/// What a card body is built from: a line-form value migrates, minting its
/// ids, and a content value lands as it is.
pub(crate) trait IntoBody {
    fn into_body(self) -> Content;
}

impl IntoBody for Lines {
    fn into_body(self) -> Content {
        quillmark_automerge::content::migrate::from_lines(&self)
    }
}

impl IntoBody for Content {
    fn into_body(self) -> Content {
        self
    }
}

/// Two cards are equal where their payloads and bodies are, each content
/// value up to the ids its document gave its parts.
impl PartialEq for Card {
    fn eq(&self, other: &Self) -> bool {
        self.body.by_place() == other.body.by_place() && payload_by_place(&self.payload) == payload_by_place(&other.payload)
    }
}

/// `v` with each content value in it, at any depth, by place.
fn value_by_place(v: &serde_json::Value) -> serde_json::Value {
    if quillmark_automerge::content::units::is_content_value(v) {
        return Content::from_value(v).expect("a content value reads").by_place().to_value();
    }
    match v {
        serde_json::Value::Array(a) => a.iter().map(value_by_place).collect(),
        serde_json::Value::Object(o) => serde_json::Value::Object(o.iter().map(|(k, x)| (k.clone(), value_by_place(x))).collect()),
        v => v.clone(),
    }
}

pub(super) fn payload_by_place(p: &Payload) -> Payload {
    let items = p
        .items()
        .iter()
        .map(|item| match item {
            PayloadItem::Field { key, value } => PayloadItem::Field { key: key.clone(), value: value_by_place(value.to_value().as_json()).into() },
            other => other.clone(),
        })
        .collect();
    Payload::from_items_with_nested(items, p.nested_comments().to_vec())
}

impl Card {
    /// Create a `Card` from its parts without validation.
    pub(crate) fn from_parts(payload: Payload, body: impl IntoBody) -> Self {
        let body = body.into_body();
        Self { payload, body, lines: Lowered::default() }
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
        &mut self.payload
    }

    /// The card body: the stored content value. For the markdown projection
    /// use [`Card::body_markdown`].
    pub fn body(&self) -> &Content {
        &self.body
    }

    /// The body lowered to the line form the markdown codecs and the renderers
    /// read.
    pub(crate) fn body_lines(&self) -> &Lines {
        self.lines.0.get_or_init(|| quillmark_automerge::content::migrate::to_lines(&self.body))
    }

    /// The card body rendered back to its markdown projection. This is a
    /// derived view (`export ∘ body`), not stored state; a `Document` round-trip
    /// therefore canonicalizes the body (e.g. `__b__` → `**b**`).
    pub fn body_markdown(&self) -> String {
        quillmark_automerge::content::lines::export::to_markdown(self.body_lines())
    }

    pub(crate) fn set_body(&mut self, body: Content) {
        self.lines = Lowered::default();
        self.body = body;
        self.payload.touched_mut().body = true;
    }

    /// The parts its edits changed since this was last called: what a card
    /// edit writes into the document.
    pub(crate) fn take_touched(&mut self) -> payload::Touched {
        self.payload.take_touched()
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
        Some(codec.decode_field(self.payload.get(name)?.to_value().as_json()))
    }

    /// The text projection of a content-valued field (`project ∘ decode`),
    /// carrying [`field_content`](Card::field_content)'s `None`/`Ok`/`Err`.
    #[cfg(test)]
    pub(crate) fn field_text(
        &self,
        name: &str,
        codec: Codec,
    ) -> Option<Result<String, ContentDecodeError>> {
        Some(self.field_content(name, codec)?.map(|c| codec.project(&c)))
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

/// A Quillmark document: one Automerge document, from construction on, and
/// its cards read from it. Serde routes through [`StoredDocument`].
///
/// Parse-time warnings are *not* document state: they ride out-of-band on
/// [`Parsed`] from [`Document::parse`], the single owner. Equality and the
/// storage DTO therefore cover only structural content (`main` and `cards`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "StoredDocument")]
pub struct Document {
    view: Snapshot,
    mints: card_id::ActorMints,
    held: Held,
    units: Units,
}

/// A document's cards: what its Automerge document reads as, read-only. A
/// [`Document`] derefs to the one it holds, and
/// [`snapshot`](Document::snapshot) copies it out, held in no Automerge
/// document: the projection a render, a validation and a resolve read, and
/// nothing writes back.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub(crate) main: Card,
    pub(crate) cards: Vec<Card>,
}

impl Snapshot {
    /// The snapshot of `main` and `cards`, as a document holding them reads:
    /// `None` where `main` carries no `$quill` or a composable card no
    /// `$kind`. A host rebuilds one this way from cards it read elsewhere,
    /// such as the WASM runtime handing a document to its render build.
    pub fn new(main: Card, cards: Vec<Card>) -> Option<Snapshot> {
        (main.quill().is_some() && cards.iter().all(|c| c.kind().is_some())).then_some(Snapshot { main, cards })
    }

    pub fn main(&self) -> &Card {
        &self.main
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
    /// [`Document::card_mut`]. `None` when out of range.
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
    /// A snapshot stays schema-free: it receives the decision, not a schema.
    pub(crate) fn to_plate_json_gated(
        &self,
        main_body: bool,
        card_bodies: Option<&[bool]>,
    ) -> serde_json::Value {
        let mut map = serde_json::Map::new();

        map.insert(
            "$quill".to_string(),
            serde_json::Value::String(self.main.quill().expect("the root carries `$quill`").to_string()),
        );

        // The seam carries the body as canonical Content-JSON: a nested content
        // object, byte-identical to `to_canonical_json`, never a lossy markdown
        // string. The markdown projection is `body_markdown`.
        if main_body {
            map.insert(
                "$body".to_string(),
                self.main.body().to_value(),
            );
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
                    card_map.insert(
                        "$body".to_string(),
                        card.body().to_value(),
                    );
                }
                for (key, value) in card.payload.iter() {
                    card_map.insert(key.clone(), value.to_value().into_owned().into_json());
                }
                serde_json::Value::Object(card_map)
            })
            .collect();

        map.insert("$cards".to_string(), serde_json::Value::Array(cards_array));

        for (key, value) in self.main.payload.iter() {
            map.insert(key.clone(), value.to_value().into_owned().into_json());
        }

        serde_json::Value::Object(map)
    }
}

/// The units the handle's host counts text offsets in. No part of what the
/// document equals.
#[derive(Clone, Copy, Default)]
struct Units(HostUnits);

impl PartialEq for Units {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Units {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The Automerge document a [`Document`] is; the cards are its view, read
/// again after each write. No part of what the document equals.
#[derive(Clone)]
pub(crate) struct Held(quillmark_automerge::model::Doc);

impl PartialEq for Held {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Held")
    }
}

/// One card of a [`Document`] under edit: a `&mut Card` whose edits are
/// written into the document's Automerge document when it drops.
pub(crate) struct CardEdit<'a> {
    doc: &'a mut Document,
    at: Option<usize>,
    id: Option<String>,
}

impl<'a> CardEdit<'a> {
    fn new(doc: &'a mut Document, at: Option<usize>) -> Self {
        let id = doc.begin_edit(at);
        CardEdit { doc, at, id }
    }
}

impl std::ops::Deref for CardEdit<'_> {
    type Target = Card;

    fn deref(&self) -> &Card {
        self.doc.card_at(self.at)
    }
}

impl std::ops::DerefMut for CardEdit<'_> {
    fn deref_mut(&mut self) -> &mut Card {
        self.doc.card_raw_mut(self.at)
    }
}

impl Drop for CardEdit<'_> {
    fn drop(&mut self) {
        self.doc.end_edit(self.at, self.id.take());
    }
}

/// A document reads as its cards: every read of [`Snapshot`] is a read of
/// the view its Automerge document holds.
impl std::ops::Deref for Document {
    type Target = Snapshot;

    fn deref(&self) -> &Snapshot {
        &self.view
    }
}

impl Document {
    /// A new Automerge document holding `main` and `cards`, each part read
    /// back carrying the id that document gives it. Its first change is the
    /// whole build, under one actor at time 0, so equal cards build equal
    /// bytes in every process.
    pub(crate) fn assembled(main: Card, cards: Vec<Card>, mints: card_id::ActorMints) -> Self {
        debug_assert!(cards.iter().all(|c| c.id().is_some()), "a document is built once its card ids are settled");
        Self::read_from(held::build(&main, &cards), mints).expect("built cards read back")
    }

    /// The document `doc` holds, its cards read from it.
    pub(crate) fn read_from(mut doc: quillmark_automerge::model::Doc, mints: card_id::ActorMints) -> Result<Self, String> {
        let (main, cards) = held::read(&doc)?;
        if let Some(actor) = mints.actor() {
            doc.set_actor(actor);
        }
        Ok(Self { view: Snapshot { main, cards }, mints, held: Held(doc), units: Units::default() })
    }

    /// The cards this document reads as, copied out of it: a projection to
    /// render, validate or hand to another thread, which carries no history
    /// and writes nowhere.
    pub fn snapshot(&self) -> Snapshot {
        self.view.clone()
    }

    /// The actor this handle writes under, where one was set: each write's
    /// Automerge actor, and the scope of each card id a placement mints,
    /// `{actor}-{token}`. With none, writes take a random actor and card ids
    /// mint positionally.
    pub fn actor(&self) -> Option<&str> {
        self.mints.actor()
    }

    /// Writes under `actor` from now on. A live client sets one unique to its
    /// session, so two clients writing from one document never mint one id.
    /// One that is not an [actor](quillmark_automerge::content::ids::is_actor)
    /// is refused with `edit::invalid_actor`.
    pub fn set_actor(&mut self, actor: &str) -> Result<(), EditError> {
        if !quillmark_automerge::content::ids::is_actor(actor) {
            return Err(EditError::InvalidActor(actor.to_string()));
        }
        self.mints.set_actor(actor);
        self.held.0.set_actor(actor);
        Ok(())
    }

    /// A new handle on this document as a new peer: its cards and history,
    /// a random Automerge actor and no actor set, so what either handle
    /// writes merges into the other. `Clone` keeps the actor, which two
    /// handles that both write must not share.
    pub fn fork(&self) -> Document {
        Document {
            view: self.view.clone(),
            mints: card_id::ActorMints::default(),
            held: Held(self.held.0.fork()),
            units: self.units,
        }
    }

    /// The units this handle's host counts text offsets in: each write's
    /// edit counts its offsets in them, and each receipt's patches, a
    /// merge's included, count theirs. Scalars until
    /// [`set_units`](Self::set_units).
    pub fn units(&self) -> HostUnits {
        self.units.0
    }

    /// Counts text offsets in `units` from now on, as the host does: the
    /// WASM binding sets UTF-16 on every handle it makes.
    pub fn set_units(&mut self, units: HostUnits) {
        self.units = Units(units);
    }

    /// The Automerge document the cards are read from.
    pub(crate) fn doc(&mut self) -> &mut quillmark_automerge::model::Doc {
        &mut self.held.0
    }

    /// Reads the card at `at`, the main card where `None`, again from the
    /// Automerge document.
    pub(crate) fn reread_at(&mut self, at: Option<usize>) {
        let doc = &self.held.0;
        match at {
            None => self.view.main = held::read_at(doc, None),
            Some(i) => self.view.cards[i] = held::read_at(doc, Some(self.view.cards[i].id().expect("a held card has an id"))),
        }
    }

    /// The card at `at` to edit in place, the main card where `None`. Writes
    /// only through [`edit_card`](Self::edit_card), [`CardEdit`] or
    /// [`CardMut`], which carry the edit into the Automerge document.
    fn card_raw_mut(&mut self, at: Option<usize>) -> &mut Card {
        match at {
            None => &mut self.view.main,
            Some(i) => &mut self.view.cards[i],
        }
    }

    /// Runs `f` on the card at `at`, the main card where `None`, and writes
    /// the parts it changed into the Automerge document as one change.
    pub(crate) fn edit_card<R>(&mut self, at: Option<usize>, f: impl FnOnce(&mut Card) -> R) -> R {
        let id = self.begin_edit(at);
        let out = f(self.card_raw_mut(at));
        self.end_edit(at, id);
        out
    }

    /// Starts an edit of the card at `at`: what touched it before is
    /// forgotten. Answers the `$id` it is held under.
    fn begin_edit(&mut self, at: Option<usize>) -> Option<String> {
        let card = self.card_raw_mut(at);
        card.take_touched();
        card.id().map(str::to_string)
    }

    /// Writes the parts an edit touched of the card at `at`, held under
    /// `id`, into the Automerge document as one change, and takes what the
    /// document reads after into the cards: a part that took ids from the
    /// document carries them.
    fn end_edit(&mut self, at: Option<usize>, id: Option<String>) {
        let touched = self.card_raw_mut(at).take_touched();
        let doc = &mut self.held.0;
        if touched.is_empty() {
            return;
        }
        doc.track();
        let before = Heads::from(doc.heads());
        let card = match at {
            None => &self.view.main,
            Some(i) => &self.view.cards[i],
        };
        match (at, id.as_deref()) {
            (None, _) => held::write_parts(doc, None, card, &touched),
            (Some(_), Some(id)) if card.id() != Some(id) => held::rename_card(doc, id, card),
            (Some(_), Some(id)) => held::write_parts(doc, Some(id), card, &touched),
            (Some(_), None) => return,
        }
        doc.commit();
        held::refresh(doc, &before, &mut self.view.main, &mut self.view.cards, false, self.units.0);
    }

    /// Places `card` at `index` among the composable cards.
    pub(crate) fn insert_card_at(&mut self, index: usize, card: Card) {
        let card = held::insert_card(&mut self.held.0, index, &card);
        self.view.cards.insert(index, card);
    }

    /// Takes the composable card at `index` out of the document.
    pub(crate) fn remove_card_at(&mut self, index: usize) -> Card {
        let card = self.view.cards.remove(index);
        if let Some(id) = card.id() {
            held::remove_card(&mut self.held.0, id);
        }
        card
    }

    /// Moves the composable card at `from` to `to`.
    pub(crate) fn move_card_at(&mut self, from: usize, to: usize) {
        let card = self.view.cards.remove(from);
        if let Some(id) = card.id() {
            held::move_card(&mut self.held.0, id, to);
        }
        self.view.cards.insert(to, card);
    }

    /// Writes `main` and `cards` over the document's cards as one change,
    /// value by value.
    pub(crate) fn replace_cards(&mut self, main: Card, cards: Vec<Card>) {
        let doc = &mut self.held.0;
        doc.track();
        let before = Heads::from(doc.heads());
        held::write_over(doc, (&self.view.main, &self.view.cards), (&main, &cards));
        held::refresh(doc, &before, &mut self.view.main, &mut self.view.cards, true, self.units.0);
    }

    /// This document's cards with each id its Automerge document gave a
    /// content part spelled as that part's place, so documents of other
    /// histories that read alike compare and spell alike.
    #[cfg(test)]
    pub(crate) fn by_place(&self) -> Snapshot {
        let card = |c: &Card| Card::from_parts(payload_by_place(&c.payload), c.body.by_place());
        Snapshot { main: card(&self.view.main), cards: self.view.cards.iter().map(card).collect() }
    }

    /// Whether this document's history holds `heads`.
    pub fn holds(&mut self, heads: &Heads) -> bool {
        self.doc().holds(heads.hashes())
    }

    /// This document as it was at `heads`, read from its history: what a
    /// writer that read then read, built as a new document. `None` where the
    /// history does not hold `heads`.
    pub fn at(&mut self, heads: &Heads) -> Option<Document> {
        if !self.holds(heads) {
            return None;
        }
        let mints = self.mints.clone();
        let (main, cards) = self.doc().reading_at(heads.hashes(), |d| held::read(d).expect("a held document reads as cards"));
        Some(Self::assembled(main, cards, mints))
    }

    /// Lands the cards of `wanted`, made from `then`, this document as it was
    /// at `heads`, as one change at those heads, value by value: concurrent
    /// with every change since, and merged with them.
    pub(crate) fn land_at(&mut self, heads: &Heads, then: &Document, wanted: &Document) {
        let doc = self.doc();
        let before = Heads::from(doc.heads());
        doc.isolated(heads.hashes(), |d| held::write_over(d, (&then.main, &then.cards), (&wanted.main, &wanted.cards)));
        let doc = &mut self.held.0;
        held::refresh(doc, &before, &mut self.view.main, &mut self.view.cards, true, self.units.0);
    }

    /// The document as Automerge bytes, its whole history: what a peer
    /// [`load`](Document::load)s or [`merge`](Document::merge)s. A host
    /// stores the row, its [`StoredDocument`] JSON.
    pub fn save(&mut self) -> Vec<u8> {
        self.doc().save()
    }

    /// The document `bytes` hold: a [`save`](Document::save) or a merge of
    /// several. Bytes that do not decode, or hold no Quillmark document of a
    /// schema this build reads, are refused.
    pub fn load(bytes: &[u8]) -> Result<Self, EditError> {
        let doc = quillmark_automerge::model::Doc::load(bytes).map_err(EditError::bad_live_bytes)?;
        Self::read_from(doc, Default::default()).map_err(|_| EditError::bad_live_bytes(LoadError::Shape))
    }

    /// The document's revision: its Automerge heads.
    pub fn heads(&mut self) -> Heads {
        Heads::from(self.doc().heads())
    }

    /// The changes since `heads`, as bytes [`merge`](Document::merge) takes.
    pub fn save_since(&mut self, heads: &Heads) -> Vec<u8> {
        self.doc().save_since(heads.hashes())
    }

    /// The next message of the sync protocol with the peer `state` tracks,
    /// `None` where the peer has everything.
    pub fn generate_sync_message(&mut self, state: &mut SyncState) -> Option<SyncMessage> {
        self.doc().generate_sync_message(state)
    }

    /// Takes a peer's sync message and answers what it changed; one that
    /// leaves the document outside its schema is refused and undone.
    pub fn receive_sync_message(&mut self, state: &mut SyncState, message: SyncMessage) -> Result<Receipt, EditError> {
        let before = self.heads();
        self.doc().track();
        self.doc().receive_sync_message(state, message).map_err(EditError::bad_live_bytes)?;
        Ok(self.took(before))
    }

    /// Takes the changes `bytes` hold, another peer's save or
    /// [`save_since`](Document::save_since), and answers what they changed.
    pub fn merge(&mut self, bytes: &[u8]) -> Result<Receipt, EditError> {
        let before = self.heads();
        self.doc().track();
        self.doc().merge(bytes).map_err(EditError::bad_live_bytes)?;
        Ok(self.took(before))
    }

    /// The receipt of the changes taken since `before`, each card part they
    /// reached followed or read again.
    fn took(&mut self, before: Heads) -> Receipt {
        let doc = &mut self.held.0;
        let patches = held::refresh(doc, &before, &mut self.view.main, &mut self.view.cards, true, self.units.0);
        let heads = Heads::from(doc.heads());
        Receipt { before, heads, patches, dropped_anchors: Vec::new(), repairs: Vec::new(), warnings: Vec::new() }
    }

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
        Self::assembled(Card::from_parts(payload, Lines::empty()), Vec::new(), Default::default())
    }

    /// Create a `Document` from a pre-built main card and composable cards.
    /// `main` must carry `$quill`; composable cards must carry `$kind` and must
    /// not carry `$quill` or `$seed`.
    ///
    /// The invariants are `debug_assert`s because no caller can break them.
    /// This is crate-internal; every public door that places a card gates it
    /// ([`Unchecked::insert_card`](crate::document::Unchecked::insert_card),
    /// [`set_card_kind`](Self::set_card_kind), parse, and
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
        Self::assembled(main, cards, Default::default())
    }

    /// Read a stored row ([`StoredDocument`] JSON) into a [`Parsed`]: the
    /// [`Document`], and a [`noncanonical_read`] warning at each body or field
    /// whose content value the read rewrote to its canonical form. Fails as
    /// deserializing a `Document` from `json` fails.
    pub fn from_stored(json: &str) -> Result<Parsed, serde_json::Error> {
        Self::stored_row(json.as_bytes())
    }

    /// Read `data` as [`save`](Document::save) bytes when it starts as
    /// Automerge's bytes do, and as a stored row
    /// ([`from_stored`](Document::from_stored)) otherwise.
    pub fn from_stored_any(data: &[u8]) -> Result<Parsed, StoredError> {
        if data.starts_with(&quillmark_automerge::model::MAGIC) {
            let document = Self::load(data).map_err(StoredError::Bytes)?;
            return Ok(Parsed { document, warnings: Vec::new() });
        }
        Self::stored_row(data).map_err(StoredError::Row)
    }

    fn stored_row(data: &[u8]) -> Result<Parsed, serde_json::Error> {
        let stored: StoredDocument = serde_json::from_slice(data)?;
        dto::from_stored(stored).map_err(serde::de::Error::custom)
    }

    /// Parse card-yaml Markdown into a [`Parsed`]: the [`Document`] plus any
    /// non-fatal warnings. The single parse entry; a caller that wants only the
    /// document writes `Document::parse(md)?.document`. Errors on malformed
    /// YAML, a missing root `$quill`, an over-size input, and the other
    /// [`ParseError`] variants.
    #[doc(alias = "from_markdown")]
    pub fn parse(markdown: &str) -> Result<Parsed, ParseError> {
        let (Snapshot { main, mut cards }, mut warnings, _) = assemble::decompose_with_warnings(markdown, import_markdown)?;
        card_id::settle_reporting(&mut cards, &std::collections::HashSet::new(), &mut warnings);
        Ok(Parsed { document: Self::assembled(main, cards, Default::default()), warnings })
    }

    /// The cards of `markdown` for a revise, each holding the `$id` it
    /// spells, or none: each body's `qm-island` tokens drop silently, the
    /// revise resolving them against the stored card it aligns the body's
    /// card to, and each body's markdown is kept for it.
    pub(crate) fn parse_revising(markdown: &str) -> Result<Revising, ParseError> {
        let (cards, warnings, bodies) = assemble::decompose_with_warnings(
            markdown,
            quillmark_automerge::content::lines::import::from_markdown_unresolved,
        )?;
        Ok(Revising { cards, warnings, bodies })
    }

    pub fn main_mut(&mut self) -> CardMut<'_> {
        CardMut::new(self, None)
    }

    pub(crate) fn main_card_mut(&mut self) -> CardEdit<'_> {
        CardEdit::new(self, None)
    }

    /// The composable card at `index` to edit, `None` where none is.
    pub(crate) fn card_edit(&mut self, index: usize) -> Option<CardEdit<'_>> {
        (index < self.view.cards.len()).then(|| CardEdit::new(self, Some(index)))
    }
}
