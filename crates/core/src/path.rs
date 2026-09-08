//! Canonical document-model paths.
//!
//! [`DocPath`] is the workspace's one serializer and parser for
//! [`Diagnostic::path`](crate::error::Diagnostic::path), the anchor into a
//! typed [`Document`](crate::document::Document). No site assembles a path with
//! `format!` and no consumer regexes one apart: the exported [`FromStr`] parser
//! is [`Display`](std::fmt::Display)'s inverse.
//!
//! # Grammar
//!
//! ```text
//! path   := root segment*
//! root   := "main"                          // the main card
//!         | "cards" "." kind "[" index "]"   // typed card
//!         | "cards" "[" index "]"            // unknown-kind card (the only bare-index root)
//! segment:= "." field | "[" index "]" | ".body"
//! kind   := [a-z_][a-z0-9_]*
//! field  := (plain | escape)*
//! plain  := any byte other than "." "[" "]" "\"
//! escape := "\" ("." | "[" | "]" | "\")
//! ```
//!
//! A field name is what the document carries, not an identifier: a nested YAML
//! map key is unconstrained, so `!must_fill` collection mints `main.m.0` and
//! `main.m.a-b`. A name is free to contain `.`, `[`, `]` or `\` itself:
//! `Display` escapes each with a leading `\` wherever it occurs, and `FromStr`
//! undoes exactly that when it reads a field word back, so every name
//! round-trips rather than only ones that happen to avoid the four
//! meta-characters. A name with none of them renders unescaped. So **an
//! all-digit name reads back as a name, never an index** (`main.m.0` is
//! `Field{"0"}`), and the plate-space `.N` index spelling is translated at
//! the geometry boundary (`region.rs`) instead.
//!
//! Every document-model path is **rooted**, which makes the grammar total
//! against a field named for a root: a main field literally named `cards` is
//! `main.cards`. One residual: a field literally named `body` renders
//! `<root>.body` and collides with the body terminal, accepted, not guarded.
//!
//! This is the document-model namespace, distinct from the plate-JSON
//! `data.$cards` array template authors see: sigiled `$cards` is glue delivered
//! to the backend, unsigiled `cards` is a path into the document. Config-space
//! anchors (`$seed.<kind>.<field>`, Quill.yaml schema-literal owner labels)
//! ride the same serializer with their prefix as a leading
//! [`field`](DocPath::field) segment: the one **unrooted** form, verbatim and
//! never parsed.

use crate::value::PathSegment;
use std::fmt;
use std::str::FromStr;

/// One segment of a [`DocPath`].
///
/// Serde-tagged (`{ "seg": "field", "name": "x" }`) so the WASM parser hands
/// the editor a structured array it routes on, never a string it splits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "seg", rename_all = "lowercase")]
pub enum DocSeg {
    /// The main-card root.
    Main,
    /// A composable card by document-array index. `kind: None` is the
    /// unknown-kind whole-card form (`cards[<i>]`), the only bare-index root.
    Card { kind: Option<String>, index: usize },
    /// An object field or map key.
    Field { name: String },
    /// An array index.
    Index { index: usize },
    /// A card or main body (`.body`), always terminal.
    Body,
}

/// A canonical document-model path: an ordered [`DocSeg`] list with one
/// [`Display`](std::fmt::Display) serializer and one [`FromStr`] parser. The
/// grammar is in the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DocPath {
    segs: Vec<DocSeg>,
}

impl DocPath {
    /// The empty base for a config-space path (`$seed.<kind>`): the one
    /// unrooted form. A document-model path roots at [`main`](Self::main) or
    /// [`card`](Self::card) instead.
    pub fn new() -> Self {
        Self::default()
    }

    /// The main-card root, `main`.
    pub fn main() -> Self {
        Self {
            segs: vec![DocSeg::Main],
        }
    }

    /// The main body anchor, `main.body`.
    pub fn main_body() -> Self {
        Self {
            segs: vec![DocSeg::Main, DocSeg::Body],
        }
    }

    /// A composable card root. `kind: None` is the unknown-kind whole-card
    /// form `cards[<i>]`; `Some(k)` is `cards.<k>[<i>]`.
    pub fn card(kind: Option<&str>, index: usize) -> Self {
        Self {
            segs: vec![DocSeg::Card {
                kind: kind.map(str::to_owned),
                index,
            }],
        }
    }

    /// This path extended by a field segment. The name is stored verbatim, so
    /// a config-space prefix (`$seed.<kind>`) can ride as an opaque head.
    pub fn field(&self, name: &str) -> Self {
        self.pushing(DocSeg::Field {
            name: name.to_owned(),
        })
    }

    /// This path extended by an array index segment.
    pub fn index(&self, index: usize) -> Self {
        self.pushing(DocSeg::Index { index })
    }

    /// This path extended by the terminal body segment.
    pub fn body(&self) -> Self {
        self.pushing(DocSeg::Body)
    }

    /// This path extended by a value-relative [`PathSegment`], the bridge from
    /// the value-tree walk.
    pub fn segment(&self, seg: &PathSegment) -> Self {
        match seg {
            PathSegment::Key(k) => self.field(k),
            PathSegment::Index(i) => self.index(*i),
        }
    }

    pub fn segs(&self) -> &[DocSeg] {
        &self.segs
    }

    fn pushing(&self, seg: DocSeg) -> Self {
        let mut segs = self.segs.clone();
        segs.push(seg);
        Self { segs }
    }
}

impl fmt::Display for DocPath {
    /// A `Field` takes a leading `.` unless it heads the path; `Index` and
    /// `Body` never do; the roots are self-contained heads.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.segs.iter().enumerate() {
            match seg {
                DocSeg::Main => f.write_str("main")?,
                DocSeg::Card { kind: Some(k), index } => write!(f, "cards.{k}[{index}]")?,
                DocSeg::Card { kind: None, index } => write!(f, "cards[{index}]")?,
                DocSeg::Field { name } => {
                    if i != 0 {
                        f.write_str(".")?;
                    }
                    write_escaped_field(f, name)?;
                }
                DocSeg::Index { index } => write!(f, "[{index}]")?,
                DocSeg::Body => f.write_str(".body")?,
            }
        }
        Ok(())
    }
}

/// Write a field name with `.`, `[`, `]` and `\` escaped by a leading `\`, the
/// exact inverse of the unescaping [`scan`] does when it reads a field word.
/// A name with none of the four bytes writes unchanged.
fn write_escaped_field(f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    if !name.contains(['.', '[', ']', '\\']) {
        return f.write_str(name);
    }
    for c in name.chars() {
        if matches!(c, '.' | '[' | ']' | '\\') {
            f.write_str("\\")?;
        }
        write!(f, "{c}")?;
    }
    Ok(())
}

/// A [`DocPath`] parse failure. The parser is total over every path
/// [`Display`](std::fmt::Display) emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocPathParseError {
    pub input: String,
    pub reason: &'static str,
}

impl fmt::Display for DocPathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid document path '{}': {}", self.input, self.reason)
    }
}

impl std::error::Error for DocPathParseError {}

impl FromStr for DocPath {
    type Err = DocPathParseError;

    /// The inverse of [`Display`](std::fmt::Display), total over every emitted
    /// path.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason: &'static str| DocPathParseError {
            input: s.to_owned(),
            reason,
        };
        if s.is_empty() {
            return Err(err("empty path"));
        }

        // A `main`/`cards` head is reclassed into its root below.
        let segs = scan(s).map_err(err)?;

        // A main field literally named `body` reads back as the body: the
        // accepted residual collision.
        if matches!(segs.first(), Some(DocSeg::Field { name }) if name == "main") {
            let rest = &segs[1..];
            if matches!(rest, [DocSeg::Field { name }] if name == "body") {
                return Ok(DocPath::main_body());
            }
            let mut out = vec![DocSeg::Main];
            out.extend_from_slice(rest);
            return Ok(DocPath { segs: out });
        }

        // A `cards` word that fits no card-root shape (no index) is an
        // ordinary field named `cards`.
        if matches!(segs.first(), Some(DocSeg::Field { name }) if name == "cards") {
            if let Some((card, rest)) = parse_card_root(&segs) {
                let mut segs = vec![card];
                segs.extend(tail_segs(rest));
                return Ok(DocPath { segs });
            }
        }

        // An unrooted chain is a config-space anchor, never a document address.
        Ok(DocPath { segs })
    }
}

/// Scan a path into segments. Root/terminal words (`main`/`cards`/`body`) scan
/// as fields and are reclassed by the caller. Enforced here: no empty word,
/// digits inside brackets, and a field word's escapes are all well-formed.
fn scan(s: &str) -> Result<Vec<DocSeg>, &'static str> {
    let mut segs = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    // Head word (paths never open with `.` or `[`).
    if bytes[0] == b'.' || bytes[0] == b'[' {
        return Err("path must start with a name");
    }
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                let end = s[i..].find(']').map(|o| i + o).ok_or("unclosed '['")?;
                let digits = &s[i + 1..end];
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("index is not a number");
                }
                let index = digits.parse().map_err(|_| "index out of range")?;
                segs.push(DocSeg::Index { index });
                i = end + 1;
            }
            b'.' => {
                let start = i + 1;
                i = word_end(bytes, start);
                if i == start {
                    return Err("empty segment after '.'");
                }
                segs.push(DocSeg::Field { name: unescape_field(&s[start..i])? });
            }
            _ => {
                let start = i;
                i = word_end(bytes, start);
                segs.push(DocSeg::Field { name: unescape_field(&s[start..i])? });
            }
        }
    }
    Ok(segs)
}

/// The index just past a word: the run up to the next unescaped `.` or `[`.
/// A `\` escapes whatever byte follows it, so `\.` and `\[` don't end the
/// word early; stepping one byte at a time (rather than skipping two on a
/// `\`) keeps this on UTF-8 boundaries even when the escaped byte begins a
/// multi-byte character.
fn word_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    let mut escaped = false;
    while i < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[i] == b'\\' {
            escaped = true;
        } else if bytes[i] == b'.' || bytes[i] == b'[' {
            break;
        }
        i += 1;
    }
    i
}

/// Undo [`write_escaped_field`]'s escaping on a word [`word_end`] just
/// bounded: `\.`, `\[`, `\]`, `\\` decode to the literal character. Any other
/// byte after a `\`, or a `\` with nothing after it, is a parse error.
fn unescape_field(raw: &str) -> Result<String, &'static str> {
    if !raw.contains('\\') {
        return Ok(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(e @ ('.' | '[' | ']' | '\\')) => out.push(e),
            _ => return Err("invalid escape in field name"),
        }
    }
    Ok(out)
}

/// Match a `cards` head against the two card-root shapes. `None` when neither
/// fits, and `cards` is then a field, not a root.
fn parse_card_root(segs: &[DocSeg]) -> Option<(DocSeg, &[DocSeg])> {
    match segs {
        // cards[<i>] …
        [DocSeg::Field { .. }, DocSeg::Index { index }, rest @ ..] => {
            Some((DocSeg::Card { kind: None, index: *index }, rest))
        }
        // cards.<kind>[<i>] …
        [DocSeg::Field { .. }, DocSeg::Field { name: kind }, DocSeg::Index { index }, rest @ ..] => {
            Some((
                DocSeg::Card {
                    kind: Some(kind.clone()),
                    index: *index,
                },
                rest,
            ))
        }
        _ => None,
    }
}

/// A card-root tail: a lone `body` is the card body, otherwise the scanned
/// chain stands.
fn tail_segs(rest: &[DocSeg]) -> Vec<DocSeg> {
    match rest {
        [DocSeg::Field { name }] if name == "body" => vec![DocSeg::Body],
        _ => rest.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(path: DocPath, rendered: &str) {
        assert_eq!(path.to_string(), rendered, "serialize");
        assert_eq!(
            rendered.parse::<DocPath>().expect("parse"),
            path,
            "parse back"
        );
    }

    #[test]
    fn main_field_and_nested() {
        round_trip(DocPath::main(), "main");
        round_trip(DocPath::main().field("title"), "main.title");
        round_trip(
            DocPath::main().field("recipients").index(0).field("name"),
            "main.recipients[0].name",
        );
    }

    #[test]
    fn main_body() {
        round_trip(DocPath::main_body(), "main.body");
    }

    /// `collect_fill_diags` mints this shape, so the reading is live.
    #[test]
    fn digit_field_name_is_a_key_not_an_index() {
        round_trip(DocPath::main().field("m").field("0"), "main.m.0");
        assert_eq!(
            "main.m.0".parse::<DocPath>().unwrap().segs()[2],
            DocSeg::Field {
                name: "0".to_string()
            }
        );
    }

    #[test]
    fn card_roots() {
        round_trip(DocPath::card(Some("indorsement"), 0), "cards.indorsement[0]");
        round_trip(DocPath::card(None, 3), "cards[3]");
    }

    #[test]
    fn card_field_and_body() {
        round_trip(
            DocPath::card(Some("indorsement"), 0).field("signature_block"),
            "cards.indorsement[0].signature_block",
        );
        round_trip(
            DocPath::card(Some("skills"), 2).body(),
            "cards.skills[2].body",
        );
        round_trip(
            DocPath::card(Some("indorsement"), 0)
                .field("recipients")
                .index(1)
                .field("name"),
            "cards.indorsement[0].recipients[1].name",
        );
    }

    #[test]
    fn body_is_reserved_only_as_a_root_terminal() {
        round_trip(
            DocPath::card(Some("k"), 0).field("body").field("x"),
            "cards.k[0].body.x",
        );
        round_trip(DocPath::main().field("x"), "main.x");
    }

    #[test]
    fn main_field_named_for_a_root_is_not_a_root() {
        round_trip(DocPath::main().field("cards"), "main.cards");
        round_trip(DocPath::main().field("main"), "main.main");
        // No index, so a config-space chain rather than a card.
        round_trip(DocPath::new().field("cards").field("foo"), "cards.foo");
    }

    #[test]
    fn config_space_anchor_is_the_unrooted_form() {
        round_trip(
            DocPath::new()
                .field("$seed")
                .field("indorsement")
                .field("author"),
            "$seed.indorsement.author",
        );
    }

    #[test]
    fn segment_bridge() {
        let base = DocPath::card(Some("k"), 0);
        assert_eq!(
            base.segment(&PathSegment::Key("addr".into()))
                .segment(&PathSegment::Index(2))
                .to_string(),
            "cards.k[0].addr[2]",
        );
    }

    #[test]
    fn parse_rejects_malformed() {
        for bad in [
            "", ".foo", "[0]", "foo[", "foo[a]", "foo[]", "a..b", "a.", "main.a\\", "main.a\\x",
        ] {
            assert!(bad.parse::<DocPath>().is_err(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn plain_field_name_renders_unescaped() {
        // Pinned exact output: unaffected by the escaping scheme.
        assert_eq!(DocPath::main().field("addr").to_string(), "main.addr");
    }

    #[test]
    fn field_name_containing_dot_round_trips_distinct_from_two_segments() {
        let dotted = DocPath::main().field("addr").field("a.b");
        round_trip(dotted.clone(), "main.addr.a\\.b");
        let split = DocPath::main().field("addr").field("a").field("b");
        assert_eq!(split.to_string(), "main.addr.a.b");
        assert_ne!(dotted, split);
        assert_ne!(dotted.to_string(), split.to_string());
    }

    #[test]
    fn field_name_containing_brackets_round_trips() {
        round_trip(
            DocPath::main().field("addr").field("a[0"),
            "main.addr.a\\[0",
        );
        round_trip(
            DocPath::main().field("addr").field("a]0"),
            "main.addr.a\\]0",
        );
        round_trip(
            DocPath::main().field("addr").field("[a][b]"),
            "main.addr.\\[a\\]\\[b\\]",
        );
    }

    #[test]
    fn field_name_containing_escape_char_round_trips() {
        round_trip(
            DocPath::main().field("addr").field(r"a\b"),
            "main.addr.a\\\\b",
        );
    }

    #[test]
    fn serde_round_trips_as_tagged_array() {
        let path = DocPath::card(Some("indorsement"), 0).field("sig");
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(
            json,
            r#"[{"seg":"card","kind":"indorsement","index":0},{"seg":"field","name":"sig"}]"#
        );
        assert_eq!(serde_json::from_str::<DocPath>(&json).unwrap(), path);
    }
}
