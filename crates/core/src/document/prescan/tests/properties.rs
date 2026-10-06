//! The prescan reads comments and tags out of YAML it did not write.
//!
//! Generated documents carry a comment at any or every slot, written with each
//! block sequence under a key both ways: indented past the key, and at its
//! column. Both spellings read back into the slots the generator wrote, and
//! `to_markdown` round-trips the document. Written in any layout, with comments
//! at any column, a document settles after one emission. Over arbitrary fence
//! bodies neither the prescan nor the parse panics.

use std::collections::HashMap;

use proptest::prelude::*;
use serde_json::{Map, Value};

use super::super::prescan_fence_content;
use crate::document::{Document, NestedComment, PayloadItem};
use crate::value::{PathSegment, QuillValue};

#[derive(Debug, Clone)]
enum Node {
    Word(String),
    Absent,
    /// An empty value under this tag.
    Tagged(&'static str),
    EmptyMap,
    EmptySeq,
    Map(Vec<(String, Node)>),
    /// A mapping whose second entries come in through a merge (`<<`) written
    /// among its own: it reads its own keys, then each merged one it does not
    /// hold.
    Merged(Vec<(String, Node)>, Vec<(String, Node)>),
    Seq(Vec<Node>),
}

impl Node {
    /// A mapping's own entries and the ones it merges.
    fn mapping(&self) -> Option<(&[(String, Node)], &[(String, Node)])> {
        match self {
            Node::Map(entries) => Some((entries, &[])),
            Node::Merged(own, merged) => Some((own, merged)),
            _ => None,
        }
    }

    fn json(&self) -> Value {
        match self {
            Node::Word(w) => Value::String(w.clone()),
            Node::Absent => Value::Null,
            Node::Tagged(tag) if reads_text(tag) => Value::String(String::new()),
            Node::Tagged(_) => Value::Null,
            Node::EmptyMap => Value::Object(Map::new()),
            Node::EmptySeq => Value::Array(Vec::new()),
            Node::Map(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.json()))
                    .collect(),
            ),
            Node::Merged(own, merged) => Value::Object(
                own.iter()
                    .chain(merged.iter().filter(|(k, _)| !holds(own, k)))
                    .map(|(k, v)| (k.clone(), v.json()))
                    .collect(),
            ),
            Node::Seq(items) => Value::Array(items.iter().map(Node::json).collect()),
        }
    }
}

/// `!!str` and `!` read an empty value as text; `!!null` and an unknown tag as
/// null.
fn reads_text(tag: &str) -> bool {
    matches!(tag, "!!str" | "!")
}

fn holds(entries: &[(String, Node)], key: &str) -> bool {
    entries.iter().any(|(k, _)| k == key)
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
        1 => prop::sample::select(&["!!str", "!", "!!null", "!t"][..]).prop_map(Node::Tagged),
        1 => Just(Node::EmptyMap),
        1 => Just(Node::EmptySeq),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            2 => arb_entries(inner.clone()).prop_map(Node::Map),
            1 => (arb_entries(inner.clone()), arb_entries(inner.clone()))
                .prop_map(|(own, merged)| Node::Merged(own, merged)),
            2 => prop::collection::vec(inner, 1..4).prop_map(Node::Seq),
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
/// writes a sequence under a key at the key's column, and `alias`, which
/// anchors a key's first spelling and writes every later one as its alias.
struct Render {
    zero: bool,
    alias: bool,
    /// The anchor on each key spelled so far.
    keys: HashMap<String, usize>,
    fill: Vec<bool>,
    slot: usize,
    count: usize,
    src: String,
    /// Each top-level item, with the slot it takes: its position, then `0`
    /// for an own-line comment, `1` for a field and `2` for a trailer.
    items: Vec<(usize, usize, Item)>,
    nested: Vec<NestedComment>,
}

/// Where a `Render` writes the merge bringing `merged` among `entries`, the
/// mapping's own, written from `from` on.
fn merge_at(entries: &[(String, Node)], merged: &[(String, Node)], from: usize) -> Option<usize> {
    let n = entries.len();
    (!merged.is_empty()).then(|| ((n + merged.len()) % (n + 1)).max(from))
}

/// How many of `merged` the mapping holding `entries` takes in.
fn brought(entries: &[(String, Node)], merged: &[(String, Node)]) -> usize {
    merged.iter().filter(|(k, _)| !holds(entries, k)).count()
}

/// The slot of a comment below the value of `entries[i]` that holds none: it
/// waits for the next child, the merge's keys or an own key, or follows the
/// last key.
fn next_slot(
    entries: &[(String, Node)],
    merged: &[(String, Node)],
    from: usize,
    i: usize,
) -> usize {
    let n = entries.len();
    if merge_at(entries, merged, from) == Some(i + 1) {
        n
    } else if i + 1 < n {
        i + 1
    } else {
        n + brought(entries, merged)
    }
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
    fn new(zero: bool, alias: bool, fill: Vec<bool>) -> Self {
        Self {
            zero,
            alias,
            keys: HashMap::new(),
            fill,
            slot: 0,
            count: 0,
            src: String::new(),
            items: Vec::new(),
            nested: Vec::new(),
        }
    }

    fn key(&mut self, k: &str) -> String {
        if !self.alias {
            return k.to_string();
        }
        if let Some(n) = self.keys.get(k) {
            return format!("*k{n} ");
        }
        let n = self.keys.len() + 1;
        self.keys.insert(k.to_string(), n);
        format!("&k{n} {k}")
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
        if path.is_empty() {
            let comment = Item::Comment(text.to_string(), inline);
            self.items.push((position, 2 * usize::from(inline), comment));
            return;
        }
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

    fn field(&mut self, path: &[PathSegment], position: usize, k: &str) {
        if path.is_empty() {
            self.items.push((position, 1, Item::Field(k.to_string())));
        }
    }

    /// A key's line and its value's lines, `lead` ahead of the key, which sits
    /// at `column`. `path` is the value's. Answers a comment written under a
    /// value that holds none, which the caller's slot after the entry takes.
    fn entry(
        &mut self,
        lead: String,
        column: usize,
        k: &str,
        v: &Node,
        path: &[PathSegment],
        t: &Option<String>,
    ) -> Option<String> {
        let t = trailer(t);
        let key = self.key(k);
        let head = |sep: &str| format!("{lead}{key}{sep}{t}");
        match v {
            Node::Word(w) => self.line(0, &head(&format!(": {w}"))),
            Node::Tagged(tag) if reads_text(tag) => {
                self.line(0, &head(&format!(": {tag}")));
                let c = self.comment()?;
                self.line(column + 2, &format!("# {c}"));
                return Some(c);
            }
            Node::Absent | Node::Tagged(_) | Node::EmptyMap | Node::EmptySeq => {
                let sep = match v {
                    Node::Tagged(tag) => format!(": {tag}"),
                    Node::EmptyMap => ": {}".to_string(),
                    Node::EmptySeq => ": []".to_string(),
                    _ => ":".to_string(),
                };
                self.line(0, &head(&sep));
                self.own(column + 2, path, 0);
            }
            Node::Map(entries) => {
                self.line(0, &head(":"));
                self.map(entries, &[], 0, column + 2, path);
            }
            Node::Merged(own, merged) => {
                self.line(0, &head(":"));
                self.map(own, merged, 0, column + 2, path);
            }
            Node::Seq(items) => {
                self.line(0, &head(":"));
                let dash = if self.zero { column } else { column + 2 };
                self.seq(items, dash, path, self.zero);
            }
        }
        None
    }

    /// A mapping's entries from `from` on, at `column`, and a merge among
    /// them past `from` bringing `merged`.
    fn map(
        &mut self,
        entries: &[(String, Node)],
        merged: &[(String, Node)],
        from: usize,
        column: usize,
        path: &[PathSegment],
    ) {
        let n = entries.len();
        let at = merge_at(entries, merged, from);
        for (i, (k, v)) in entries.iter().enumerate().skip(from) {
            if at == Some(i) {
                self.merge(entries, merged, column, path);
            }
            self.own(column, path, i);
            self.field(path, i, k);
            let t = self.comment();
            if let Some(t) = &t {
                self.mark(path, i, t, true);
            }
            let value = child(path, PathSegment::Key(k.clone()));
            if let Some(c) = self.entry(" ".repeat(column), column, k, v, &value, &t) {
                self.mark(path, next_slot(entries, merged, from, i), &c, false);
            }
        }
        if at == Some(n) {
            self.merge(entries, merged, column, path);
        }
        self.own(column, path, n + brought(entries, merged));
    }

    /// `<<:` at `column`, holding `merged` in a block mapping. Each comment
    /// takes its slot among the keys of the mapping: `own`'s, then each merged
    /// one `own` does not hold. Those on and inside a merged key `own` holds
    /// land ahead of where it would sit.
    fn merge(
        &mut self,
        own: &[(String, Node)],
        merged: &[(String, Node)],
        column: usize,
        path: &[PathSegment],
    ) {
        let mut seat = own.len();
        self.own(column, path, seat);
        let t = self.comment();
        self.line(column, &format!("<<:{}", trailer(&t)));
        if let Some(t) = &t {
            self.mark(path, seat, t, false);
        }
        for (k, v) in merged {
            let held = !holds(own, k);
            self.own(column + 2, path, seat);
            if held {
                self.field(path, seat, k);
            }
            let t = self.comment();
            if let Some(t) = &t {
                self.mark(path, seat, t, held);
            }
            let value = child(path, PathSegment::Key(k.clone()));
            let first = self.nested.len();
            let after = self.entry(" ".repeat(column + 2), column + 2, k, v, &value, &t);
            if !held {
                for c in self.nested.split_off(first) {
                    self.mark(path, seat, &c.text, false);
                }
            }
            seat += usize::from(held);
            if let Some(c) = after {
                self.mark(path, seat, &c, false);
            }
        }
        self.own(column + 2, path, seat);
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
            Node::Map(_) | Node::Merged(..) => {
                let (entries, merged) = item.mapping().expect("a mapping");
                let (k, v) = &entries[0];
                let value = child(&own, PathSegment::Key(k.clone()));
                let lead = self.comment();
                let first = self.comment();
                let after = if lead.is_some() || (t.is_some() && first.is_some()) {
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
                    self.entry(" ".repeat(dash + 2), dash + 2, k, v, &value, &first)
                } else {
                    // The dash line's trailer is the item's.
                    let t = t.or(first);
                    if let Some(t) = &t {
                        self.mark(path, i, t, true);
                    }
                    self.entry(format!("{}- ", " ".repeat(dash)), dash + 2, k, v, &value, &t)
                };
                if let Some(c) = after {
                    self.mark(&own, next_slot(entries, merged, 1, 0), &c, false);
                }
                self.map(entries, merged, 1, dash + 2, &own);
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
                    Node::Tagged(tag) => tag,
                    Node::EmptyMap => "{}",
                    Node::EmptySeq => "[]",
                    _ => "null",
                };
                self.line(dash, &format!("- {text}{}", trailer(&t)));
            }
        }
    }
}

/// Where `c` emits among the comments of fields holding `fields`: at each level
/// the place its path takes in the value, then its slot. A merge writes keys
/// past the source order, so the marks a `Render` makes in source order sort
/// by this.
fn emit_order(fields: &Map<String, Value>, c: &NestedComment) -> Vec<usize> {
    let mut order = Vec::new();
    let mut map = fields;
    let mut value: Option<&Value> = None;
    for segment in &c.container_path {
        let (place, next) = match (segment, value) {
            (PathSegment::Index(i), Some(Value::Array(items))) => (*i, &items[*i]),
            (PathSegment::Key(k), _) => {
                let place = map.keys().position(|x| x == k).expect("a key the value holds");
                (place, &map[k])
            }
            _ => unreachable!("a path into the value"),
        };
        order.extend([place, 2]);
        value = Some(next);
        if let Value::Object(inner) = next {
            map = inner;
        }
    }
    order.extend([c.position, usize::from(c.inline)]);
    order
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
    /// sequence indentation, under keys written as aliases and among the keys
    /// a merge brings, and the document round-trips through `to_markdown`.
    #[test]
    fn every_comment_reads_back_into_its_slot(
        entries in arb_entries(arb_node()),
        merged in prop::option::weighted(0.3, arb_entries(arb_node())),
        fill in arb_fill(),
    ) {
        let merged = merged.unwrap_or_default();
        let Value::Object(fields) = Node::Merged(entries.clone(), merged.clone()).json() else {
            unreachable!("a mapping reads as an object");
        };
        for (zero, alias) in [(false, false), (true, false), (false, true)] {
            let mut render = Render::new(zero, alias, fill.clone());
            render.map(&entries, &merged, 0, 0, &[]);
            let src = format!("~~~\n$quill: q\n$kind: main\n{}~~~\n", render.src);
            let doc = Document::parse(&src)
                .unwrap_or_else(|e| panic!("the rendering parses: {e}\n{src}"))
                .document;
            let payload = doc.main().payload();
            let mut items = render.items;
            items.sort_by_key(|&(position, kind, _)| (position, kind));
            let items: Vec<Item> = items.into_iter().map(|(.., item)| item).collect();
            prop_assert_eq!(items_of(&doc), items, "{}", src);
            let mut nested = render.nested;
            nested.sort_by_cached_key(|c| emit_order(&fields, c));
            prop_assert_eq!(payload.nested_comments(), &nested[..], "{}", src);
            for (k, v) in &fields {
                prop_assert_eq!(payload.get(k).map(QuillValue::as_json), Some(v), "{}", src);
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
/// key's line, below it, onto a continuation line or in a block scalar of any
/// style, a flow collection on one line or several, a tag or anchor, a key bare,
/// quoted or holding a space, and own-line comments at any column.
struct Scribble {
    picks: Vec<u8>,
    at: usize,
    anchors: usize,
    /// The anchor on each key text written with one.
    keys: HashMap<String, usize>,
    /// The anchors on the merges' values written so far.
    merges: Vec<usize>,
    src: String,
}

impl Scribble {
    fn new(picks: Vec<u8>) -> Self {
        Self {
            picks,
            at: 0,
            anchors: 0,
            keys: HashMap::new(),
            merges: Vec::new(),
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

    /// A missing value's tag or anchor, if any, past a space.
    fn bare(&mut self) -> String {
        match self.props().trim_end() {
            "" => String::new(),
            props => format!(" {props}"),
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

    /// `k` spelled as `spell` has it, anchored, or as an alias to an earlier
    /// key anchored with its text.
    fn key(&mut self, k: &str, column: usize) -> String {
        let (spelled, text) = spell(k, column);
        match self.keys.get(&text).copied() {
            Some(n) if self.pick(2) == 0 => format!("*k{n} "),
            _ if self.pick(4) == 0 => {
                self.anchors += 1;
                self.keys.insert(text, self.anchors);
                format!("&k{} {spelled}", self.anchors)
            }
            _ => spelled,
        }
    }

    fn entry(&mut self, lead: String, column: usize, k: &str, v: &Node) {
        let k = &self.key(k, column);
        if let Some(flow) = self.flow(v, column + 2) {
            let t = self.tail();
            self.line(0, &format!("{lead}{k}: {flow}{t}"));
            return;
        }
        match v {
            Node::Word(w) if self.pick(5) == 0 => {
                let props = self.props();
                let t = self.tail();
                self.line(0, &format!("{lead}{k}: {props}{}{t}", style(w)));
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
                    match broken(w, &word) {
                        Some((head, rest)) => {
                            self.line(0, &format!("{lead}{k}: {head}"));
                            self.line(column + 2, &format!("{rest}{t}"));
                        }
                        None => self.line(0, &format!("{lead}{k}: {word}{t}")),
                    }
                }
            }
            Node::Absent | Node::Tagged(_) | Node::EmptyMap | Node::EmptySeq => {
                let sep = match v {
                    Node::Tagged(tag) => format!(": {tag}"),
                    Node::EmptyMap => ": {}".to_string(),
                    Node::EmptySeq => ": []".to_string(),
                    _ => format!(":{}", self.bare()),
                };
                let t = self.tail();
                self.line(0, &format!("{lead}{k}{sep}{t}"));
                self.notes(column + 4);
            }
            Node::Map(_) | Node::Merged(..) => {
                let (entries, merged) = v.mapping().expect("a mapping");
                let t = self.tail();
                self.line(0, &format!("{lead}{k}:{t}"));
                let child = column + 1 + self.pick(4);
                self.notes(child + 2);
                self.block_map(entries, merged, 0, child);
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

    /// A mapping's entries from `from` on, at `column`, and a merge among
    /// them past `from` bringing `merged`.
    fn block_map(
        &mut self,
        entries: &[(String, Node)],
        merged: &[(String, Node)],
        from: usize,
        column: usize,
    ) {
        let at = (!merged.is_empty()).then(|| self.pick(entries.len() + 1).max(from));
        for (i, (k, v)) in entries.iter().enumerate().skip(from) {
            if i > from {
                self.notes(column + 2);
            }
            if at == Some(i) {
                self.merge(merged, " ".repeat(column), column);
                self.notes(column + 2);
            }
            self.entry(" ".repeat(column), column, k, v);
        }
        if at == Some(entries.len()) {
            self.notes(column + 2);
            self.merge(merged, " ".repeat(column), column);
        }
        self.notes(column + 2);
    }

    /// `<<` at `column`, `lead` ahead of it on its line, bringing `merged` as
    /// the picks spell it: a block or flow mapping, a sequence holding one, or
    /// an alias to a merge's value written before, under `!!merge` or not.
    fn merge(&mut self, merged: &[(String, Node)], lead: String, column: usize) {
        let key = ["<<", "<<", "!!merge <<"][self.pick(3)];
        let t = self.tail();
        let alias = self.pick(self.merges.len() + 2);
        if let Some(&n) = self.merges.get(alias) {
            self.line(0, &format!("{lead}{key}: *m{n}{t}"));
            return;
        }
        if let Some(flow) = self.flow(&Node::Map(merged.to_vec()), column + 2) {
            self.line(0, &format!("{lead}{key}: {flow}{t}"));
            return;
        }
        let anchor = (self.pick(3) == 0).then(|| {
            self.anchors += 1;
            self.anchors
        });
        let named = anchor.map_or_else(String::new, |n| format!(" &m{n}"));
        self.line(0, &format!("{lead}{key}:{named}{t}"));
        let child = column + 1 + self.pick(3);
        self.notes(child + 2);
        if self.pick(3) == 0 {
            self.item(" ".repeat(child), child, &Node::Map(merged.to_vec()));
            self.notes(child + 2);
        } else {
            self.block_map(merged, &[], 0, child);
        }
        self.merges.extend(anchor);
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
            Node::Merged(entries, merged) if self.pick(3) == 0 => {
                self.merge(merged, format!("{lead}- "), dash + 2);
                self.block_map(entries, &[], 0, dash + 2);
            }
            Node::Map(_) | Node::Merged(..) if self.pick(2) == 0 => {
                let (entries, merged) = item.mapping().expect("a mapping");
                let (k, v) = &entries[0];
                self.entry(format!("{lead}- "), dash + 2, k, v);
                self.block_map(entries, merged, 1, dash + 2);
            }
            Node::Map(_) | Node::Merged(..) => {
                let (entries, merged) = item.mapping().expect("a mapping");
                let t = self.tail();
                self.line(0, &format!("{lead}-{t}"));
                let child = dash + 1 + self.pick(3);
                self.notes(child + 2);
                self.block_map(entries, merged, 0, child);
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
                self.line(0, &format!("{lead}- {props}{}{t}", style(w)));
                self.line(dash + 2, w);
            }
            Node::Word(w) => {
                let word = self.word(w);
                let t = self.tail();
                match broken(w, &word) {
                    Some((head, rest)) => {
                        self.line(0, &format!("{lead}- {head}"));
                        self.line(dash + 2, &format!("{rest}{t}"));
                    }
                    None => self.line(0, &format!("{lead}- {word}{t}")),
                }
            }
            Node::Absent => {
                let bare = self.bare();
                let t = self.tail();
                self.line(0, &format!("{lead}-{bare}{t}"));
                self.notes(dash + 4);
            }
            Node::Tagged(tag) => {
                let t = self.tail();
                self.line(0, &format!("{lead}- {tag}{t}"));
                self.notes(dash + 4);
            }
            Node::EmptyMap | Node::EmptySeq => {
                let empty = if matches!(item, Node::EmptyMap) { "{}" } else { "[]" };
                let t = self.tail();
                self.line(0, &format!("{lead}- {empty}{t}"));
            }
        }
    }
}

/// A key as a hand might spell it, and the text it reads as: bare or quoted,
/// and past the root holding a space or ` #`.
fn spell(k: &str, column: usize) -> (String, String) {
    match (column, (k.len() + column) % 4) {
        (_, 0) => (k.to_string(), k.to_string()),
        (_, 1) => (format!("\"{k}\""), k.to_string()),
        (0, _) => (format!("'{k}'"), k.to_string()),
        (_, 2) => (format!("{k} x"), format!("{k} x")),
        _ => (format!("'{k} # y'"), format!("{k} # y")),
    }
}

fn style(w: &str) -> &'static str {
    ["|-", "|", ">-", ">"][w.len() % 4]
}

/// `word` carried onto a continuation line, plain or inside its quotes, when
/// `w` ends in a digit.
fn broken(w: &str, word: &str) -> Option<(String, String)> {
    if !w.ends_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(match word.strip_suffix('"') {
        Some(open) => (open.to_string(), "more\"".to_string()),
        None => (word.to_string(), "more".to_string()),
    })
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
        &["", "v", "w x", "[a, b]", "{a: 1}", "[]", "{}", "!t v", "&a v", "*a", "|", "null", "[a,", "'q", "!!str", "!"][..],
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
