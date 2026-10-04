//! The prescan reads comments and tags out of YAML it did not write.
//!
//! Generated documents carry a comment at any or every slot, written with each
//! block sequence under a key both ways: indented past the key, and at its
//! column. Both spellings read back into the slots the generator wrote, and
//! `to_markdown` round-trips the document. Written in any layout, with comments
//! at any column, a document settles after one emission. Over arbitrary fence
//! bodies neither the prescan nor the parse panics.

use proptest::prelude::*;
use serde_json::{Map, Value};

use super::super::prescan_fence_content;
use crate::document::{Document, NestedComment, PayloadItem};
use crate::value::PathSegment;

#[derive(Debug, Clone)]
enum Node {
    Word(String),
    Absent,
    EmptyMap,
    EmptySeq,
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
}

impl Node {
    fn json(&self) -> Value {
        match self {
            Node::Word(w) => Value::String(w.clone()),
            Node::Absent => Value::Null,
            Node::EmptyMap => Value::Object(Map::new()),
            Node::EmptySeq => Value::Array(Vec::new()),
            Node::Map(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.json()))
                    .collect(),
            ),
            Node::Seq(items) => Value::Array(items.iter().map(Node::json).collect()),
        }
    }
}

fn distinct(entries: Vec<(String, Node)>) -> Vec<(String, Node)> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|(k, _)| seen.insert(k.clone()))
        .collect()
}

fn arb_entries(node: impl Strategy<Value = Node>) -> impl Strategy<Value = Vec<(String, Node)>> {
    prop::collection::vec(("k[a-z0-9]{0,2}", node), 1..4).prop_map(distinct)
}

fn arb_node() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        4 => "v[a-z0-9]{0,3}".prop_map(Node::Word),
        1 => Just(Node::Absent),
        1 => Just(Node::EmptyMap),
        1 => Just(Node::EmptySeq),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            arb_entries(inner.clone()).prop_map(Node::Map),
            prop::collection::vec(inner, 1..4).prop_map(Node::Seq),
        ]
    })
}

/// Which slots carry a comment, cycled: `[true]` is every slot.
fn arb_fill() -> impl Strategy<Value = Vec<bool>> {
    prop_oneof![
        Just(vec![true]),
        prop::collection::vec(prop::bool::weighted(0.7), 1..48),
    ]
}

#[derive(Debug, PartialEq)]
enum Item {
    Field(String),
    Comment(String, bool),
}

/// Writes a payload as a hand might, recording the slot each comment takes in
/// the document model. The layout is `to_markdown`'s but for `zero`, which
/// writes a sequence under a key at the key's column.
struct Render {
    zero: bool,
    fill: Vec<bool>,
    slot: usize,
    count: usize,
    src: String,
    items: Vec<Item>,
    nested: Vec<NestedComment>,
}

fn child(path: &[PathSegment], segment: PathSegment) -> Vec<PathSegment> {
    let mut path = path.to_vec();
    path.push(segment);
    path
}

fn trailer(text: &Option<String>) -> String {
    text.as_ref().map_or_else(String::new, |t| format!(" # {t}"))
}

impl Render {
    fn new(zero: bool, fill: Vec<bool>) -> Self {
        Self {
            zero,
            fill,
            slot: 0,
            count: 0,
            src: String::new(),
            items: Vec::new(),
            nested: Vec::new(),
        }
    }

    fn comment(&mut self) -> Option<String> {
        let on = self.fill[self.slot % self.fill.len()];
        self.slot += 1;
        on.then(|| {
            self.count += 1;
            format!("c{}", self.count)
        })
    }

    fn line(&mut self, column: usize, text: &str) {
        self.src.push_str(&" ".repeat(column));
        self.src.push_str(text);
        self.src.push('\n');
    }

    fn mark(&mut self, path: &[PathSegment], position: usize, text: &str, inline: bool) {
        self.nested.push(NestedComment {
            container_path: path.to_vec(),
            position,
            text: text.to_string(),
            inline,
        });
    }

    fn own(&mut self, column: usize, path: &[PathSegment], position: usize) {
        if let Some(c) = self.comment() {
            self.line(column, &format!("# {c}"));
            self.mark(path, position, &c, false);
        }
    }

    fn root(&mut self, entries: &[(String, Node)]) {
        for (k, v) in entries {
            if let Some(c) = self.comment() {
                self.line(0, &format!("# {c}"));
                self.items.push(Item::Comment(c, false));
            }
            self.items.push(Item::Field(k.clone()));
            let t = self.comment();
            if let Some(t) = &t {
                self.items.push(Item::Comment(t.clone(), true));
            }
            self.entry(String::new(), 0, k, v, &[PathSegment::Key(k.clone())], &t);
        }
        if let Some(c) = self.comment() {
            self.line(0, &format!("# {c}"));
            self.items.push(Item::Comment(c, false));
        }
    }

    /// A key's line and its value's lines, `lead` ahead of the key, which sits
    /// at `column`. `path` is the value's.
    fn entry(
        &mut self,
        lead: String,
        column: usize,
        k: &str,
        v: &Node,
        path: &[PathSegment],
        t: &Option<String>,
    ) {
        let t = trailer(t);
        let head = |sep: &str| format!("{lead}{k}{sep}{t}");
        match v {
            Node::Word(w) => self.line(0, &head(&format!(": {w}"))),
            Node::Absent | Node::EmptyMap | Node::EmptySeq => {
                let sep = match v {
                    Node::EmptyMap => ": {}",
                    Node::EmptySeq => ": []",
                    _ => ":",
                };
                self.line(0, &head(sep));
                self.own(column + 2, path, 0);
            }
            Node::Map(entries) => {
                self.line(0, &head(":"));
                self.map(entries, 0, column + 2, path);
            }
            Node::Seq(items) => {
                self.line(0, &head(":"));
                let dash = if self.zero { column } else { column + 2 };
                self.seq(items, dash, path, self.zero);
            }
        }
    }

    fn map(&mut self, entries: &[(String, Node)], from: usize, column: usize, path: &[PathSegment]) {
        for (i, (k, v)) in entries.iter().enumerate().skip(from) {
            self.own(column, path, i);
            let t = self.comment();
            if let Some(t) = &t {
                self.mark(path, i, t, true);
            }
            let value = child(path, PathSegment::Key(k.clone()));
            self.entry(" ".repeat(column), column, k, v, &value, &t);
        }
        self.own(column, path, entries.len());
    }

    /// A comment at the dashes' column after the last item is the next key's
    /// when the dashes sit at that key's column, so one inside the sequence
    /// goes a column past them.
    fn seq(&mut self, items: &[Node], dash: usize, path: &[PathSegment], at_key: bool) {
        for (i, item) in items.iter().enumerate() {
            self.own(dash, path, i);
            let t = self.comment();
            self.item(dash, item, path, i, t);
        }
        self.own(dash + usize::from(at_key), path, items.len());
    }

    fn item(&mut self, dash: usize, item: &Node, path: &[PathSegment], i: usize, t: Option<String>) {
        let own = child(path, PathSegment::Index(i));
        match item {
            Node::Map(entries) => {
                let (k, v) = &entries[0];
                let value = child(&own, PathSegment::Key(k.clone()));
                let lead = self.comment();
                let first = self.comment();
                if lead.is_some() || (t.is_some() && first.is_some()) {
                    if let Some(t) = &t {
                        self.mark(path, i, t, true);
                    }
                    self.line(dash, &format!("-{}", trailer(&t)));
                    if let Some(c) = &lead {
                        self.line(dash + 2, &format!("# {c}"));
                        self.mark(&own, 0, c, false);
                    }
                    if let Some(f) = &first {
                        self.mark(&own, 0, f, true);
                    }
                    self.entry(" ".repeat(dash + 2), dash + 2, k, v, &value, &first);
                } else {
                    // The dash line's trailer is the item's.
                    let t = t.or(first);
                    if let Some(t) = &t {
                        self.mark(path, i, t, true);
                    }
                    self.entry(format!("{}- ", " ".repeat(dash)), dash + 2, k, v, &value, &t);
                }
                self.map(entries, 1, dash + 2, &own);
            }
            Node::Seq(inner) => {
                if let Some(t) = &t {
                    self.mark(path, i, t, true);
                }
                self.line(dash, &format!("-{}", trailer(&t)));
                self.seq(inner, dash + 2, &own, false);
            }
            scalar => {
                if let Some(t) = &t {
                    self.mark(path, i, t, true);
                }
                let text = match scalar {
                    Node::Word(w) => w.as_str(),
                    Node::EmptyMap => "{}",
                    Node::EmptySeq => "[]",
                    _ => "null",
                };
                self.line(dash, &format!("- {text}{}", trailer(&t)));
            }
        }
    }
}

fn items_of(doc: &Document) -> Vec<Item> {
    doc.main()
        .payload()
        .items()
        .iter()
        .filter_map(|item| match item {
            PayloadItem::Field { key, .. } => Some(Item::Field(key.clone())),
            PayloadItem::Comment { text, inline } => Some(Item::Comment(text.clone(), *inline)),
            _ => None,
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Every comment reads back into the slot it was written at, under either
    /// sequence indentation, and the document round-trips through
    /// `to_markdown`.
    #[test]
    fn every_comment_reads_back_into_its_slot(
        entries in arb_entries(arb_node()),
        fill in arb_fill(),
    ) {
        for zero in [false, true] {
            let mut render = Render::new(zero, fill.clone());
            render.root(&entries);
            let src = format!("~~~\n$quill: q\n$kind: main\n{}~~~\n", render.src);
            let doc = Document::parse(&src)
                .unwrap_or_else(|e| panic!("the rendering parses: {e}\n{src}"))
                .document;
            let payload = doc.main().payload();
            prop_assert_eq!(items_of(&doc), render.items, "{}", src);
            prop_assert_eq!(payload.nested_comments(), &render.nested[..], "{}", src);
            for (k, v) in &entries {
                prop_assert_eq!(payload.get(k).map(|q| q.as_json().clone()), Some(v.json()), "{}", src);
            }

            let md = doc.to_markdown();
            let back = Document::parse(&md)
                .unwrap_or_else(|e| panic!("the emission parses: {e}\n{md}"))
                .document;
            prop_assert_eq!(&back, &doc, "Source:\n{}\nEmitted:\n{}", src, md);
        }
    }
}

/// Writes a payload in a layout its picks draw: any indentation step, a
/// sequence at or past its key's column, a compact or bare dash, a value on its
/// key's line, below it or in a block scalar, a flow collection on one line or
/// several, a tag or anchor, and own-line comments at any column.
struct Scribble {
    picks: Vec<u8>,
    at: usize,
    anchors: usize,
    src: String,
}

impl Scribble {
    fn new(picks: Vec<u8>) -> Self {
        Self {
            picks,
            at: 0,
            anchors: 0,
            src: String::new(),
        }
    }

    fn pick(&mut self, n: usize) -> usize {
        let p = usize::from(self.picks[self.at % self.picks.len()]) % n;
        self.at += 1;
        p
    }

    fn line(&mut self, column: usize, text: &str) {
        self.src.push_str(&" ".repeat(column));
        self.src.push_str(text);
        self.src.push('\n');
    }

    fn notes(&mut self, reach: usize) {
        for _ in 0..self.pick(3) {
            let column = self.pick(reach + 1);
            self.line(column, "# n");
        }
    }

    fn tail(&mut self) -> &'static str {
        if self.pick(2) == 0 {
            " # t"
        } else {
            ""
        }
    }

    fn props(&mut self) -> String {
        match self.pick(6) {
            0 => "!t ".to_string(),
            1 => {
                self.anchors += 1;
                format!("&a{} ", self.anchors)
            }
            _ => String::new(),
        }
    }

    fn word(&mut self, w: &str) -> String {
        let props = self.props();
        match self.pick(5) {
            0 => format!("{props}\"{w} # q\""),
            _ => format!("{props}{w}"),
        }
    }

    fn root(&mut self, entries: &[(String, Node)]) {
        for (k, v) in entries {
            self.notes(4);
            self.entry(String::new(), 0, k, v);
        }
        self.notes(4);
    }

    /// `[w, v]` or `{k: w}` from flat words, on one line or broken after its
    /// first element with a trailer there, continuing at `column`.
    fn flow(&mut self, node: &Node, column: usize) -> Option<String> {
        let parts: Vec<String> = match node {
            Node::Seq(items) => items
                .iter()
                .map(|i| match i {
                    Node::Word(w) => Some(w.clone()),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            Node::Map(entries) => entries
                .iter()
                .map(|(k, v)| match v {
                    Node::Word(w) => Some(format!("{k}: {w}")),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            _ => return None,
        };
        if self.pick(3) != 0 {
            return None;
        }
        let (open, close) = if matches!(node, Node::Seq(_)) {
            ("[", "]")
        } else {
            ("{", "}")
        };
        let sep = if self.pick(2) == 0 {
            format!(", # f\n{}", " ".repeat(column))
        } else {
            ", ".to_string()
        };
        Some(format!("{open}{}{close}", parts.join(&sep)))
    }

    fn entry(&mut self, lead: String, column: usize, k: &str, v: &Node) {
        if let Some(flow) = self.flow(v, column + 2) {
            let t = self.tail();
            self.line(0, &format!("{lead}{k}: {flow}{t}"));
            return;
        }
        match v {
            Node::Word(w) if self.pick(5) == 0 => {
                let props = self.props();
                let t = self.tail();
                self.line(0, &format!("{lead}{k}: {props}|-{t}"));
                self.line(column + 2, w);
            }
            Node::Word(w) => {
                let word = self.word(w);
                if self.pick(4) == 0 {
                    let t = self.tail();
                    self.line(0, &format!("{lead}{k}:{t}"));
                    self.notes(column + 4);
                    let below = column + 1 + self.pick(3);
                    let t = self.tail();
                    self.line(below, &format!("{word}{t}"));
                } else {
                    let t = self.tail();
                    self.line(0, &format!("{lead}{k}: {word}{t}"));
                }
            }
            Node::Absent | Node::EmptyMap | Node::EmptySeq => {
                let sep = match v {
                    Node::EmptyMap => ": {}",
                    Node::EmptySeq => ": []",
                    _ => ":",
                };
                let t = self.tail();
                self.line(0, &format!("{lead}{k}{sep}{t}"));
                self.notes(column + 4);
            }
            Node::Map(entries) => {
                let t = self.tail();
                self.line(0, &format!("{lead}{k}:{t}"));
                let child = column + 1 + self.pick(4);
                self.notes(child + 2);
                self.block_map(entries, 0, child);
            }
            Node::Seq(items) => {
                let t = self.tail();
                self.line(0, &format!("{lead}{k}:{t}"));
                let dash = column + self.pick(4);
                self.notes(dash + 2);
                self.block_seq(items, dash);
            }
        }
    }

    fn block_map(&mut self, entries: &[(String, Node)], from: usize, column: usize) {
        for (i, (k, v)) in entries.iter().enumerate().skip(from) {
            if i > from {
                self.notes(column + 2);
            }
            self.entry(" ".repeat(column), column, k, v);
        }
        self.notes(column + 2);
    }

    fn block_seq(&mut self, items: &[Node], dash: usize) {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.notes(dash + 2);
            }
            self.item(" ".repeat(dash), dash, item);
        }
        self.notes(dash + 2);
    }

    /// An item whose dash sits at `dash`, `lead` ahead of it on its line.
    fn item(&mut self, lead: String, dash: usize, item: &Node) {
        if let Some(flow) = self.flow(item, dash + 2) {
            let t = self.tail();
            self.line(0, &format!("{lead}- {flow}{t}"));
            return;
        }
        match item {
            Node::Map(entries) if self.pick(2) == 0 => {
                let (k, v) = &entries[0];
                self.entry(format!("{lead}- "), dash + 2, k, v);
                self.block_map(entries, 1, dash + 2);
            }
            Node::Map(entries) => {
                let t = self.tail();
                self.line(0, &format!("{lead}-{t}"));
                let child = dash + 1 + self.pick(3);
                self.notes(child + 2);
                self.block_map(entries, 0, child);
            }
            Node::Seq(inner) if self.pick(2) == 0 => {
                self.item(format!("{lead}- "), dash + 2, &inner[0]);
                for next in &inner[1..] {
                    self.notes(dash + 4);
                    self.item(" ".repeat(dash + 2), dash + 2, next);
                }
                self.notes(dash + 4);
            }
            Node::Seq(inner) => {
                let t = self.tail();
                self.line(0, &format!("{lead}-{t}"));
                let child = dash + 1 + self.pick(3);
                self.notes(child + 2);
                self.block_seq(inner, child);
            }
            Node::Word(w) if self.pick(5) == 0 => {
                let props = self.props();
                let t = self.tail();
                self.line(0, &format!("{lead}- {props}|-{t}"));
                self.line(dash + 2, w);
            }
            Node::Word(w) => {
                let word = self.word(w);
                let t = self.tail();
                self.line(0, &format!("{lead}- {word}{t}"));
            }
            Node::Absent => {
                let t = self.tail();
                self.line(0, &format!("{lead}-{t}"));
            }
            Node::EmptyMap | Node::EmptySeq => {
                let empty = if matches!(item, Node::EmptyMap) { "{}" } else { "[]" };
                let t = self.tail();
                self.line(0, &format!("{lead}- {empty}{t}"));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Wherever a layout puts its comments, the document it parses to emits
    /// markdown that parses back to the same document.
    #[test]
    fn any_layout_settles_after_one_emission(
        entries in arb_entries(arb_node()),
        picks in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let mut scribble = Scribble::new(picks);
        scribble.root(&entries);
        let src = format!("~~~\n$quill: q\n{}~~~\n", scribble.src);
        let parsed = Document::parse(&src)
            .unwrap_or_else(|e| panic!("the layout parses: {e}\n{src}"))
            .document;
        let md = parsed.to_markdown();
        let back = Document::parse(&md)
            .unwrap_or_else(|e| panic!("the emission parses: {e}\nSource:\n{src}\nEmitted:\n{md}"))
            .document;
        prop_assert_eq!(&back, &parsed, "Source:\n{}\nEmitted:\n{}", src, md);
    }
}

/// One line of block YAML from the pieces that decide where a comment or tag
/// lands: indentation, dashes, a key, a value, a trailing comment.
fn arb_line() -> impl Strategy<Value = String> {
    let indent = prop::sample::select(&["", " ", "  ", "    ", "      "][..]);
    let dashes = prop::sample::select(&["", "", "- ", "- - ", "-"][..]);
    let key = prop::sample::select(&["", "k: ", "j: ", "k:", "\"q k\": ", "? ", "!t m: "][..]);
    let value = prop::sample::select(
        &["", "v", "w x", "[a, b]", "{a: 1}", "[]", "{}", "!t v", "&a v", "*a", "|", "null", "[a,", "'q"][..],
    );
    let comment = prop::sample::select(&["", "", " # c", "# own", " #"][..]);
    (indent, dashes, key, value, comment)
        .prop_map(|(i, d, k, v, c)| format!("{i}{d}{k}{v}{c}"))
}

fn arb_body() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_line(), 0..14).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn the_prescan_reads_any_body_without_panicking(body in arb_body()) {
        let _ = prescan_fence_content(&body);
    }

    #[test]
    fn the_prescan_reads_any_text_without_panicking(body in "\\PC{0,200}") {
        let _ = prescan_fence_content(&body);
    }

    /// The parse refuses or reads a fence body, never panics, prescan included.
    #[test]
    fn a_document_parse_refuses_rather_than_panics(body in arb_body()) {
        let _ = Document::parse(&format!("~~~\n$quill: q\n{body}\n~~~\n"));
    }
}
