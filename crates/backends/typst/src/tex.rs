//! TeX math, as an equation island stores it, lowered to Typst math markup.
//!
//! Every atom the lowering writes is one character, a number, a string or a
//! call, and atoms are space-separated, so no run of letters reads as a Typst
//! identifier and no run of punctuation as a shorthand. A symbol command
//! lowers to its Unicode character, never a Typst symbol name, and ASCII
//! punctuation is escaped. A command outside the vocabulary draws as its own
//! name and is reported by [`unsupported`].

use std::collections::BTreeSet;

use quillmark_content::island::IslandType;
use quillmark_core::error::{Diagnostic, Severity};
use quillmark_core::path::DocPath;
use quillmark_core::quill::QuillConfig;
use quillmark_core::Content;

const CODE: &str = "typst::unsupported_tex";

/// How deep groups and arguments nest before the lowering draws what lies
/// further in flat.
const MAX_DEPTH: usize = 32;

/// The Typst equation for `tex`: `$…$`, or `$ … $` where `display`.
pub(crate) fn equation(tex: &str, display: bool) -> String {
    let body = lower(tex).markup;
    match (display, body.is_empty()) {
        (true, _) => format!("$ {body} $"),
        (false, true) => "$\"\"$".to_string(),
        (false, false) => format!("${body}$"),
    }
}

/// `tex` as Typst math, the commands it does not lower, and whether it
/// nests past [`MAX_DEPTH`].
pub(crate) struct Lowered {
    pub(crate) markup: String,
    pub(crate) unknown: BTreeSet<String>,
    pub(crate) deep: bool,
}

pub(crate) fn lower(tex: &str) -> Lowered {
    let (toks, spaced) = tokenize(tex).into_iter().unzip();
    let mut p = Parser {
        toks,
        spaced,
        at: 0,
        unknown: BTreeSet::new(),
        depth: 0,
        deep: false,
    };
    let mut nodes = Vec::new();
    while p.at < p.toks.len() {
        nodes.extend(p.seq());
        // A stray `}`, `\right` or `\end` closes nothing.
        p.at += 1;
    }
    Lowered {
        markup: render(&nodes),
        unknown: p.unknown,
        deep: p.deep,
    }
}

/// One `typst::unsupported_tex` per content field of the plate JSON `data`
/// whose equations hold a command outside the vocabulary, naming each.
pub(crate) fn unsupported(config: &QuillConfig, data: &serde_json::Value) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    config.each_plate_content(data, &mut |at: &DocPath, content: &Content| {
        let mut unknown = BTreeSet::new();
        let mut deep = false;
        for isl in content.islands.iter().filter(|i| i.island_type == IslandType::Math) {
            let tex = isl.props.get("tex").and_then(|v| v.as_str()).unwrap_or("");
            let lowered = lower(tex);
            unknown.extend(lowered.unknown);
            deep |= lowered.deep;
        }
        let warn = |message: String, hint: String| {
            Diagnostic::new(Severity::Warning, message)
                .with_code(CODE.to_string())
                .with_path(at.to_string())
                .with_hint(hint)
        };
        if !unknown.is_empty() {
            let named: Vec<String> = unknown.iter().map(|c| format!("`\\{c}`")).collect();
            let them = if named.len() == 1 { "it" } else { "them" };
            diags.push(warn(
                format!("an equation in this field uses {}, which the Typst backend does not lower", named.join(", ")),
                format!(
                    "the Typst backend lowers the TeX math commands docs/authoring/markdown-syntax.md lists; rewrite {them} with those"
                ),
            ));
        }
        if deep {
            diags.push(warn(
                format!("an equation in this field nests groups deeper than {MAX_DEPTH}, and draws what lies past that flat"),
                "flatten the equation's braces and arguments".to_string(),
            ));
        }
    });
    diags
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// `\name`, or `\` and one non-letter, without the `\`.
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    Tilde,
    Prime,
    Char(char),
}

/// The tokens of `tex`, each with whether whitespace precedes it, which only
/// `\text` reads.
fn tokenize(tex: &str) -> Vec<(Tok, bool)> {
    let mut toks = Vec::new();
    let mut chars = tex.chars().peekable();
    let mut spaced = false;
    while let Some(c) = chars.next() {
        let tok = match c {
            '\\' => match chars.next() {
                Some(l) if l.is_ascii_alphabetic() => {
                    let mut name = l.to_string();
                    while let Some(&n) = chars.peek().filter(|n| n.is_ascii_alphabetic()) {
                        name.push(n);
                        chars.next();
                    }
                    if name == "operatorname" && chars.peek() == Some(&'*') {
                        chars.next();
                    }
                    Tok::Cmd(name)
                }
                Some(s) => Tok::Cmd(s.to_string()),
                None => break,
            },
            '{' => Tok::Open,
            '}' => Tok::Close,
            '^' => Tok::Sup,
            '_' => Tok::Sub,
            '&' => Tok::Amp,
            '~' => Tok::Tilde,
            '\'' => Tok::Prime,
            '%' => {
                while chars.next_if(|&c| c != '\n').is_some() {}
                continue;
            }
            c if c.is_whitespace() => {
                spaced = true;
                continue;
            }
            c => Tok::Char(c),
        };
        toks.push((tok, std::mem::take(&mut spaced)));
    }
    toks
}

#[derive(Debug, Clone)]
enum Node {
    /// Markup for one atom, written as is.
    Atom(String),
    Group(Vec<Node>),
    Call(&'static str, Vec<Vec<Node>>),
    Attach { base: Box<Node>, b: Option<Vec<Node>>, t: Option<Vec<Node>> },
    Align,
    Break,
}

fn render(nodes: &[Node]) -> String {
    nodes.iter().map(render_node).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn render_node(node: &Node) -> String {
    match node {
        Node::Atom(s) => s.clone(),
        Node::Group(nodes) => render(nodes),
        Node::Call(name, args) => format!("{name}({})", args.iter().map(|a| arg(a)).collect::<Vec<_>>().join(", ")),
        Node::Attach { base, b, t } => {
            let mut out = format!("attach({}", arg(std::slice::from_ref(base)));
            if let Some(b) = b {
                out.push_str(&format!(", b: {}", arg(b)));
            }
            if let Some(t) = t {
                out.push_str(&format!(", t: {}", arg(t)));
            }
            out.push(')');
            out
        }
        Node::Align => "&".to_string(),
        // A linebreak is `\` before whitespace; before anything else it escapes.
        Node::Break => "\\ ".to_string(),
    }
}

/// A call argument: never empty, which Typst refuses.
fn arg(nodes: &[Node]) -> String {
    let s = render(nodes);
    if s.is_empty() {
        "\"\"".to_string()
    } else {
        s
    }
}

/// A character of the TeX as one atom. A non-ASCII one is a `\u{…}` escape,
/// one token whatever the grapheme rules join it to.
fn atom(c: char) -> Node {
    Node::Atom(match c {
        '-' => "−".to_string(),
        '*' => "∗".to_string(),
        c if c.is_ascii_punctuation() => format!("\\{c}"),
        c if c.is_ascii() => c.to_string(),
        c => format!("\\u{{{:x}}}", u32::from(c)),
    })
}

fn string(s: &str) -> Node {
    Node::Atom(format!("\"{}\"", crate::emit::escape_string(s)))
}

fn space(em: &str) -> Node {
    Node::Atom(format!("#h({em}em)"))
}

fn op(name: &str, limits: bool) -> Node {
    let limits = if limits { ", limits: #true" } else { "" };
    Node::Atom(format!("op(\"{name}\"{limits})"))
}

struct Parser {
    toks: Vec<Tok>,
    /// Per token, whether whitespace precedes it.
    spaced: Vec<bool>,
    at: usize,
    unknown: BTreeSet<String>,
    /// How many groups and arguments enclose the cursor.
    depth: usize,
    deep: bool,
}

impl Parser {
    /// Whether the cursor sits [`MAX_DEPTH`] deep, which marks the
    /// equation deep.
    fn too_deep(&mut self) -> bool {
        self.deep |= self.depth >= MAX_DEPTH;
        self.depth >= MAX_DEPTH
    }

    /// What [`Self::seq`] reads, as one flat run of its characters and
    /// symbols.
    fn flat(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut depth = 0usize;
        while let Some(tok) = self.peek() {
            match tok {
                Tok::Close if depth == 0 => break,
                Tok::Cmd(c) if depth == 0 && (c == "right" || c == "end") => break,
                Tok::Open => depth += 1,
                Tok::Close => depth -= 1,
                Tok::Char(c) => nodes.push(atom(*c)),
                Tok::Cmd(c) => nodes.extend(symbol(c).map(|s| Node::Atom(s.to_string()))),
                _ => {}
            }
            self.at += 1;
        }
        nodes
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }

    /// Atoms up to a `}`, `\right`, `\end` or the end, which it leaves unread.
    fn seq(&mut self) -> Vec<Node> {
        if self.too_deep() {
            return self.flat();
        }
        self.depth += 1;
        let nodes = self.seq_items();
        self.depth -= 1;
        nodes
    }

    fn seq_items(&mut self) -> Vec<Node> {
        let mut nodes: Vec<Node> = Vec::new();
        loop {
            match self.peek() {
                None | Some(Tok::Close) => break,
                Some(Tok::Cmd(c)) if c == "right" || c == "end" => break,
                Some(Tok::Cmd(c)) if matches!(c.as_str(), "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle") => {
                    let name = match c.as_str() {
                        "displaystyle" => "display",
                        "textstyle" => "inline",
                        "scriptstyle" => "script",
                        _ => "sscript",
                    };
                    self.at += 1;
                    let rest = self.seq();
                    nodes.push(Node::Call(name, vec![rest]));
                    break;
                }
                Some(Tok::Cmd(c)) if c == "limits" || c == "nolimits" => {
                    let name = if c == "limits" { "limits" } else { "scripts" };
                    self.at += 1;
                    if let Some(base) = nodes.pop() {
                        nodes.push(Node::Call(name, vec![vec![base]]));
                    }
                    self.scripts(&mut nodes);
                }
                Some(Tok::Sup | Tok::Sub | Tok::Prime) => {
                    if nodes.is_empty() || matches!(nodes.last(), Some(Node::Align | Node::Break)) {
                        nodes.push(Node::Atom(String::new()));
                    }
                    self.scripts(&mut nodes);
                }
                Some(Tok::Char(c)) if c.is_ascii_digit() => {
                    let mut num = String::new();
                    while let Some(Tok::Char(c)) = self.peek().cloned() {
                        let digit_after = matches!(self.toks.get(self.at + 1), Some(Tok::Char(d)) if d.is_ascii_digit());
                        if c.is_ascii_digit() || (c == '.' && digit_after && !num.is_empty()) {
                            num.push(c);
                            self.at += 1;
                        } else {
                            break;
                        }
                    }
                    nodes.push(Node::Atom(num));
                    self.scripts(&mut nodes);
                }
                _ => {
                    let node = self.atom_node();
                    nodes.extend(node);
                    self.scripts(&mut nodes);
                }
            }
        }
        nodes
    }

    /// The `^`, `_` and `'` after the last node, attached to it.
    fn scripts(&mut self, nodes: &mut Vec<Node>) {
        let (mut b, mut t): (Option<Vec<Node>>, Option<Vec<Node>>) = (None, None);
        let mut primes = 0;
        loop {
            match self.peek() {
                Some(Tok::Prime) => {
                    primes += 1;
                    self.at += 1;
                }
                Some(Tok::Sup) => {
                    self.at += 1;
                    let mut sup: Vec<Node> = (0..primes).map(|_| Node::Atom("′".into())).collect();
                    primes = 0;
                    sup.extend(self.arg());
                    t.get_or_insert_with(Vec::new).extend(sup);
                }
                Some(Tok::Sub) => {
                    self.at += 1;
                    let sub = self.arg();
                    b.get_or_insert_with(Vec::new).extend(sub);
                }
                _ => break,
            }
        }
        if primes > 0 {
            t.get_or_insert_with(Vec::new).extend((0..primes).map(|_| Node::Atom("′".into())));
        }
        if b.is_none() && t.is_none() {
            return;
        }
        let base = nodes.pop().unwrap_or(Node::Atom(String::new()));
        nodes.push(Node::Attach { base: Box::new(base), b, t });
    }

    /// A command's or script's argument: a `{…}` group or one token's atom.
    fn arg(&mut self) -> Vec<Node> {
        match self.peek() {
            Some(Tok::Open) => {
                self.at += 1;
                let inner = self.seq();
                if self.peek() == Some(&Tok::Close) {
                    self.at += 1;
                }
                inner
            }
            None | Some(Tok::Close) => Vec::new(),
            Some(Tok::Char(c)) => {
                let c = *c;
                self.at += 1;
                vec![atom(c)]
            }
            _ => self.atom_node(),
        }
    }

    /// The source text of a `{…}` group, for `\text` and `\operatorname`.
    fn text_arg(&mut self) -> String {
        if self.peek() != Some(&Tok::Open) {
            return match self.next() {
                Some(Tok::Char(c)) => c.to_string(),
                _ => String::new(),
            };
        }
        self.at += 1;
        let mut depth = 0;
        let mut out = String::new();
        while self.at < self.toks.len() {
            if self.spaced[self.at] {
                out.push(' ');
            }
            let Some(t) = self.next() else { break };
            match t {
                Tok::Open => depth += 1,
                Tok::Close if depth == 0 => break,
                Tok::Close => depth -= 1,
                Tok::Char(c) => out.push(c),
                Tok::Tilde => out.push('\u{a0}'),
                Tok::Prime => out.push('\''),
                Tok::Amp => out.push('&'),
                Tok::Sup => out.push('^'),
                Tok::Sub => out.push('_'),
                Tok::Cmd(c) if c.chars().count() == 1 && !c.chars().all(char::is_alphabetic) => {
                    out.push_str(if c == " " || c == "," || c == ";" { " " } else { &c });
                }
                Tok::Cmd(c) => {
                    out.push('\\');
                    out.push_str(&c);
                }
            }
        }
        out
    }

    /// The atom at the cursor, with whatever arguments its command reads.
    fn atom_node(&mut self) -> Vec<Node> {
        if self.too_deep() {
            return match self.next() {
                Some(Tok::Char(c)) => vec![atom(c)],
                Some(Tok::Cmd(c)) => symbol(&c).map(|s| Node::Atom(s.to_string())).into_iter().collect(),
                _ => Vec::new(),
            };
        }
        self.depth += 1;
        let nodes = self.atom_items();
        self.depth -= 1;
        nodes
    }

    fn atom_items(&mut self) -> Vec<Node> {
        let Some(tok) = self.next() else { return Vec::new() };
        match tok {
            Tok::Open => {
                let inner = self.seq();
                if self.peek() == Some(&Tok::Close) {
                    self.at += 1;
                }
                vec![Node::Group(inner)]
            }
            Tok::Amp => vec![Node::Align],
            Tok::Tilde => vec![space("0.3333")],
            Tok::Char(c) => vec![atom(c)],
            Tok::Close | Tok::Sup | Tok::Sub | Tok::Prime => Vec::new(),
            Tok::Cmd(name) => self.command(&name),
        }
    }

    fn delimiter(&mut self) -> String {
        match self.next() {
            Some(Tok::Char('.')) | None => String::new(),
            Some(Tok::Char(c)) => render_node(&atom(c)),
            Some(Tok::Cmd(c)) => match c.as_str() {
                "{" | "lbrace" => "\\{".into(),
                "}" | "rbrace" => "\\}".into(),
                "|" | "Vert" | "lVert" | "rVert" => "‖".into(),
                "vert" | "lvert" | "rvert" | "mid" => "\\|".into(),
                "backslash" => "\\\\".into(),
                other => symbol(other).map(str::to_string).unwrap_or_else(|| {
                    self.unknown.insert(other.to_string());
                    String::new()
                }),
            },
            Some(_) => String::new(),
        }
    }

    fn command(&mut self, name: &str) -> Vec<Node> {
        if let Some(sym) = symbol(name) {
            return vec![Node::Atom(sym.to_string())];
        }
        if let Some(&(_, limits)) = OPERATORS.iter().find(|(n, _)| *n == name) {
            return vec![op(name, limits)];
        }
        if let Some(&(_, mark)) = ACCENTS.iter().find(|(n, _)| *n == name) {
            let body = self.arg();
            return vec![Node::Call("accent", vec![body, vec![string(mark)]])];
        }
        if let Some(&(_, style)) = STYLES.iter().find(|(n, _)| *n == name) {
            let body = self.arg();
            return vec![match style {
                "upbold" => Node::Call("upright", vec![vec![Node::Call("bold", vec![body])]]),
                style => Node::Call(style, vec![body]),
            }];
        }
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.arg();
                let den = self.arg();
                vec![Node::Call("frac", vec![num, den])]
            }
            "binom" | "dbinom" | "tbinom" => {
                let n = self.arg();
                let k = self.arg();
                vec![Node::Call("binom", vec![n, k])]
            }
            "sqrt" => {
                let index = self.bracket();
                let body = self.arg();
                vec![match index {
                    Some(index) => Node::Call("root", vec![index, body]),
                    None => Node::Call("sqrt", vec![body]),
                }]
            }
            "overline" => vec![Node::Call("overline", vec![self.arg()])],
            "underline" => vec![Node::Call("underline", vec![self.arg()])],
            "overbrace" => vec![Node::Call("overbrace", vec![self.arg()])],
            "underbrace" => vec![Node::Call("underbrace", vec![self.arg()])],
            "text" | "textrm" | "textnormal" | "mbox" | "textup" | "hbox" => vec![string(&self.text_arg())],
            "textit" | "textsl" => vec![Node::Call("italic", vec![vec![string(&self.text_arg())]])],
            "textbf" => vec![Node::Call("bold", vec![vec![string(&self.text_arg())]])],
            "texttt" => vec![Node::Call("mono", vec![vec![string(&self.text_arg())]])],
            "textsf" => vec![Node::Call("sans", vec![vec![string(&self.text_arg())]])],
            "operatorname" => {
                let name = self.text_arg();
                vec![Node::Atom(format!("op(\"{}\")", crate::emit::escape_string(&name)))]
            }
            "left" => {
                let open = self.delimiter();
                let body = self.seq();
                let close = if self.peek() == Some(&Tok::Cmd("right".into())) {
                    self.at += 1;
                    self.delimiter()
                } else {
                    String::new()
                };
                let mut inner = vec![Node::Atom(open)];
                inner.extend(body);
                inner.push(Node::Atom(close));
                vec![Node::Call("lr", vec![inner])]
            }
            "middle" => vec![Node::Call("mid", vec![vec![Node::Atom(self.delimiter())]])],
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr" | "Bigr" | "biggr"
            | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => vec![Node::Atom(self.delimiter())],
            "begin" => self.environment(),
            "\\" | "newline" | "cr" => {
                self.bracket();
                vec![Node::Break]
            }
            "," | "thinspace" => vec![space("0.1667")],
            ":" | ">" | "medspace" => vec![space("0.2222")],
            ";" | "thickspace" => vec![space("0.2778")],
            "!" | "negthinspace" => vec![space("-0.1667")],
            " " | "space" => vec![space("0.3333")],
            "enspace" => vec![space("0.5")],
            "quad" => vec![space("1")],
            "qquad" => vec![space("2")],
            "{" | "}" | "$" | "%" | "#" | "&" | "_" => vec![atom(name.chars().next().unwrap_or(' '))],
            "|" => vec![Node::Atom("‖".into())],
            "not" => {
                let mut next = self.atom_node();
                let negated = match next.last() {
                    Some(Node::Atom(s)) => NEGATED.iter().find(|(from, _)| from == s).map(|&(_, to)| to),
                    _ => None,
                };
                match negated {
                    Some(to) => *next.last_mut().expect("a negated atom") = Node::Atom(to.to_string()),
                    None => {
                        self.unknown.insert("not".to_string());
                    }
                }
                next
            }
            "bmod" | "mod" => vec![op("mod", false)],
            "pmod" | "pod" => {
                let body = self.arg();
                let mut inner = vec![Node::Atom("\\(".into())];
                if name == "pmod" {
                    inner.push(op("mod", false));
                }
                inner.extend(body);
                inner.push(Node::Atom("\\)".into()));
                inner
            }
            "mathop" | "mathbin" | "mathrel" | "mathord" | "mathopen" | "mathclose" | "mathpunct" | "mathinner" => {
                self.arg()
            }
            "phantom" | "vphantom" | "hphantom" => vec![Node::Call("hide", vec![self.arg()])],
            "stackrel" | "overset" => {
                let top = self.arg();
                let base = self.arg();
                vec![Node::Attach { base: Box::new(Node::Call("limits", vec![base])), b: None, t: Some(top) }]
            }
            "underset" => {
                let bottom = self.arg();
                let base = self.arg();
                vec![Node::Attach { base: Box::new(Node::Call("limits", vec![base])), b: Some(bottom), t: None }]
            }
            _ => {
                self.unknown.insert(name.to_string());
                vec![string(&format!("\\{name}"))]
            }
        }
    }

    /// A `[…]` optional argument, read as math.
    fn bracket(&mut self) -> Option<Vec<Node>> {
        if self.peek() != Some(&Tok::Char('[')) {
            return None;
        }
        let start = self.at + 1;
        let mut depth = 0;
        let mut end = None;
        for (i, t) in self.toks.iter().enumerate().skip(start) {
            match t {
                Tok::Open => depth += 1,
                Tok::Close if depth == 0 => break,
                Tok::Close => depth -= 1,
                Tok::Char(']') if depth == 0 => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end?;
        let mut inner = Parser {
            toks: self.toks[start..end].to_vec(),
            spaced: self.spaced[start..end].to_vec(),
            at: 0,
            unknown: BTreeSet::new(),
            depth: self.depth,
            deep: false,
        };
        let mut nodes = Vec::new();
        while inner.at < inner.toks.len() {
            nodes.extend(inner.seq());
            inner.at += 1;
        }
        self.unknown.append(&mut inner.unknown);
        self.deep |= inner.deep;
        self.at = end + 1;
        Some(nodes)
    }

    fn environment(&mut self) -> Vec<Node> {
        let env = self.text_arg();
        if matches!(env.trim_end_matches('*'), "array" | "subarray" | "alignedat" | "alignat") {
            self.text_arg();
        }
        if env.ends_with("matrix*") {
            self.bracket();
        }
        let body = self.seq();
        if self.peek() == Some(&Tok::Cmd("end".into())) {
            self.at += 1;
            self.text_arg();
        }
        let rows = rows(body);
        let delim = match env.trim_end_matches('*') {
            "matrix" | "smallmatrix" | "array" | "subarray" => Some("#none"),
            "pmatrix" => Some("\"(\""),
            "bmatrix" => Some("\"[\""),
            "Bmatrix" => Some("\"{\""),
            "vmatrix" => Some("\"|\""),
            "Vmatrix" => Some("\"‖\""),
            _ => None,
        };
        if let Some(delim) = delim {
            let rows: Vec<String> = rows
                .iter()
                .map(|cells| cells.iter().map(|c| arg(c)).collect::<Vec<_>>().join(", "))
                .collect();
            return vec![Node::Atom(format!("mat(delim: {delim}, {})", rows.join("; ")))];
        }
        let lines: Vec<String> = rows
            .iter()
            .map(|cells| {
                let line = cells.iter().map(|c| render(c)).collect::<Vec<_>>().join(" & ");
                // A linebreak's `\` reads as one only before whitespace.
                match line.trim() {
                    l if l.ends_with('\\') => format!("{l} "),
                    l => l.to_string(),
                }
            })
            .collect();
        if env == "cases" || env == "dcases" {
            let lines: Vec<String> = lines.into_iter().map(|l| if l.trim().is_empty() { "\"\"".into() } else { l }).collect();
            return vec![Node::Atom(format!("cases({})", lines.join(", ")))];
        }
        if !matches!(env.trim_end_matches('*'), "aligned" | "align" | "gathered" | "gather" | "split" | "alignedat" | "eqnarray" | "multline") {
            self.unknown.insert(format!("begin{{{env}}}"));
        }
        vec![Node::Atom(lines.join(" \\ "))]
    }
}

/// An environment's body as rows of `&`-separated cells, split at `\\`; a
/// trailing empty row is dropped.
fn rows(body: Vec<Node>) -> Vec<Vec<Vec<Node>>> {
    let mut rows = vec![vec![Vec::new()]];
    for node in body {
        match node {
            Node::Break => rows.push(vec![Vec::new()]),
            Node::Align => rows.last_mut().expect("a row").push(Vec::new()),
            node => rows.last_mut().expect("a row").last_mut().expect("a cell").push(node),
        }
    }
    if rows.len() > 1 && rows.last().is_some_and(|r| r.len() == 1 && r[0].is_empty()) {
        rows.pop();
    }
    rows
}

/// Operator names, upright, and whether each takes limits in display.
const OPERATORS: &[(&str, bool)] = &[
    ("arccos", false), ("arcsin", false), ("arctan", false), ("arg", false), ("cos", false),
    ("cosh", false), ("cot", false), ("coth", false), ("csc", false), ("deg", false),
    ("det", true), ("dim", false), ("exp", false), ("gcd", true), ("hom", false),
    ("inf", true), ("ker", false), ("lg", false), ("lim", true), ("liminf", true),
    ("limsup", true), ("ln", false), ("log", false), ("max", true), ("min", true),
    ("Pr", true), ("sec", false), ("sin", false), ("sinh", false), ("sup", true),
    ("tan", false), ("tanh", false), ("lcm", true), ("argmax", true), ("argmin", true),
];

/// Accent commands and the combining mark each sets over its argument.
const ACCENTS: &[(&str, &str)] = &[
    ("hat", "\u{302}"), ("widehat", "\u{302}"), ("tilde", "\u{303}"), ("widetilde", "\u{303}"),
    ("bar", "\u{304}"), ("vec", "\u{20d7}"), ("overrightarrow", "\u{20d7}"),
    ("overleftarrow", "\u{20d6}"), ("dot", "\u{307}"), ("ddot", "\u{308}"),
    ("dddot", "\u{20db}"), ("check", "\u{30c}"), ("breve", "\u{306}"), ("acute", "\u{301}"),
    ("grave", "\u{300}"), ("mathring", "\u{30a}"),
];

/// Font commands and the Typst style function each lowers to; `upbold` is
/// upright bold.
const STYLES: &[(&str, &str)] = &[
    ("mathbf", "upbold"), ("boldsymbol", "bold"), ("bm", "bold"), ("mathrm", "upright"),
    ("mathit", "italic"), ("mathbb", "bb"), ("mathcal", "cal"), ("mathscr", "scr"),
    ("mathfrak", "frak"), ("mathsf", "sans"), ("mathtt", "mono"), ("mathnormal", "italic"),
];

/// A `\not` before a relation that has its own negated character.
const NEGATED: &[(&str, &str)] = &[
    ("\\=", "≠"), ("∈", "∉"), ("⊂", "⊄"), ("⊃", "⊅"), ("⊆", "⊈"), ("⊇", "⊉"), ("≡", "≢"),
    ("∼", "≁"), ("≈", "≉"), ("≅", "≇"), ("\\<", "≮"), ("\\>", "≯"), ("≤", "≰"), ("≥", "≱"),
    ("∣", "∤"), ("∥", "∦"), ("∋", "∌"),
];

fn symbol(name: &str) -> Option<&'static str> {
    SYMBOLS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Symbol commands and the character each draws.
const SYMBOLS: &[(&str, &str)] = &[
    // Greek.
    ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"), ("epsilon", "ϵ"),
    ("varepsilon", "ε"), ("zeta", "ζ"), ("eta", "η"), ("theta", "θ"), ("vartheta", "ϑ"),
    ("iota", "ι"), ("kappa", "κ"), ("varkappa", "ϰ"), ("lambda", "λ"), ("mu", "μ"),
    ("nu", "ν"), ("xi", "ξ"), ("omicron", "ο"), ("pi", "π"), ("varpi", "ϖ"), ("rho", "ρ"),
    ("varrho", "ϱ"), ("sigma", "σ"), ("varsigma", "ς"), ("tau", "τ"), ("upsilon", "υ"),
    ("phi", "ϕ"), ("varphi", "φ"), ("chi", "χ"), ("psi", "ψ"), ("omega", "ω"),
    ("digamma", "ϝ"), ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"),
    ("Xi", "Ξ"), ("Pi", "Π"), ("Sigma", "Σ"), ("Upsilon", "Υ"), ("Phi", "Φ"), ("Psi", "Ψ"),
    ("Omega", "Ω"), ("varGamma", "𝛤"), ("varDelta", "𝛥"), ("varTheta", "𝛩"),
    ("varLambda", "𝛬"), ("varXi", "𝛯"), ("varPi", "𝛱"), ("varSigma", "𝛴"),
    ("varUpsilon", "𝛶"), ("varPhi", "𝛷"), ("varPsi", "𝛹"), ("varOmega", "𝛺"),
    // Letterlike.
    ("aleph", "ℵ"), ("beth", "ℶ"), ("gimel", "ℷ"), ("hbar", "ℏ"), ("hslash", "ℏ"),
    ("ell", "ℓ"), ("Re", "ℜ"), ("Im", "ℑ"), ("partial", "∂"), ("infty", "∞"),
    ("nabla", "∇"), ("emptyset", "∅"), ("varnothing", "∅"), ("forall", "∀"), ("exists", "∃"),
    ("nexists", "∄"), ("neg", "¬"), ("lnot", "¬"), ("top", "⊤"), ("bot", "⊥"),
    ("angle", "∠"), ("measuredangle", "∡"), ("triangle", "△"), ("square", "□"),
    ("Box", "□"), ("prime", "′"), ("backprime", "‵"), ("imath", "ı"), ("jmath", "ȷ"),
    ("complement", "∁"), ("surd", "√"), ("dagger", "†"), ("dag", "†"), ("S", "§"),
    ("checkmark", "✓"), ("clubsuit", "♣"), ("diamondsuit", "♢"), ("heartsuit", "♡"),
    ("spadesuit", "♠"), ("flat", "♭"), ("natural", "♮"), ("sharp", "♯"),
    // Binary operators.
    ("pm", "±"), ("mp", "∓"), ("times", "×"), ("div", "÷"), ("cdot", "⋅"), ("ast", "∗"),
    ("star", "⋆"), ("circ", "∘"), ("bullet", "∙"), ("oplus", "⊕"), ("ominus", "⊖"),
    ("otimes", "⊗"), ("oslash", "⊘"), ("odot", "⊙"), ("cap", "∩"), ("cup", "∪"),
    ("uplus", "⊎"), ("sqcap", "⊓"), ("sqcup", "⊔"), ("vee", "∨"), ("lor", "∨"),
    ("wedge", "∧"), ("land", "∧"), ("setminus", "∖"), ("smallsetminus", "∖"), ("wr", "≀"),
    ("diamond", "⋄"), ("bigtriangleup", "△"), ("bigtriangledown", "▽"),
    ("triangleleft", "◁"), ("triangleright", "▷"), ("amalg", "⨿"), ("bigcirc", "◯"),
    ("ltimes", "⋉"), ("rtimes", "⋊"), ("centerdot", "⋅"), ("dotplus", "∔"),
    // Relations.
    ("leq", "≤"), ("le", "≤"), ("geq", "≥"), ("ge", "≥"), ("neq", "≠"), ("ne", "≠"),
    ("leqslant", "⩽"), ("geqslant", "⩾"), ("ll", "≪"), ("gg", "≫"), ("lll", "⋘"),
    ("ggg", "⋙"), ("equiv", "≡"), ("sim", "∼"), ("simeq", "≃"), ("approx", "≈"),
    ("cong", "≅"), ("asymp", "≍"), ("propto", "∝"), ("doteq", "≐"), ("in", "∈"),
    ("notin", "∉"), ("ni", "∋"), ("owns", "∋"), ("subset", "⊂"), ("supset", "⊃"),
    ("subseteq", "⊆"), ("supseteq", "⊇"), ("subsetneq", "⊊"), ("supsetneq", "⊋"),
    ("nsubseteq", "⊈"), ("nsupseteq", "⊉"), ("sqsubset", "⊏"), ("sqsupset", "⊐"),
    ("sqsubseteq", "⊑"), ("sqsupseteq", "⊒"), ("vdash", "⊢"), ("dashv", "⊣"),
    ("models", "⊨"), ("perp", "⟂"), ("parallel", "∥"), ("nparallel", "∦"), ("mid", "∣"),
    ("nmid", "∤"), ("prec", "≺"), ("succ", "≻"), ("preceq", "⪯"), ("succeq", "⪰"),
    ("bowtie", "⋈"), ("smile", "⌣"), ("frown", "⌢"), ("coloneqq", "≔"), ("eqqcolon", "≕"),
    ("triangleq", "≜"), ("lesssim", "≲"), ("gtrsim", "≳"), ("nless", "≮"), ("ngtr", "≯"),
    ("nleq", "≰"), ("ngeq", "≱"), ("nsim", "≁"), ("ncong", "≇"), ("therefore", "∴"),
    ("because", "∵"), ("vDash", "⊨"), ("Vdash", "⊩"),
    // Arrows.
    ("leftarrow", "←"), ("gets", "←"), ("rightarrow", "→"), ("to", "→"),
    ("leftrightarrow", "↔"), ("Leftarrow", "⇐"), ("Rightarrow", "⇒"),
    ("Leftrightarrow", "⇔"), ("longleftarrow", "⟵"), ("longrightarrow", "⟶"),
    ("longleftrightarrow", "⟷"), ("Longleftarrow", "⟸"), ("Longrightarrow", "⟹"),
    ("Longleftrightarrow", "⟺"), ("iff", "⟺"), ("implies", "⟹"), ("impliedby", "⟸"),
    ("mapsto", "↦"), ("longmapsto", "⟼"), ("uparrow", "↑"), ("downarrow", "↓"),
    ("updownarrow", "↕"), ("Uparrow", "⇑"), ("Downarrow", "⇓"), ("Updownarrow", "⇕"),
    ("nearrow", "↗"), ("searrow", "↘"), ("swarrow", "↙"), ("nwarrow", "↖"),
    ("hookleftarrow", "↩"), ("hookrightarrow", "↪"), ("leftharpoonup", "↼"),
    ("leftharpoondown", "↽"), ("rightharpoonup", "⇀"), ("rightharpoondown", "⇁"),
    ("rightleftharpoons", "⇌"), ("leftrightharpoons", "⇋"), ("leadsto", "⇝"),
    ("nrightarrow", "↛"), ("nleftarrow", "↚"), ("nRightarrow", "⇏"), ("nLeftarrow", "⇍"),
    ("twoheadrightarrow", "↠"), ("rightrightarrows", "⇉"), ("leftleftarrows", "⇇"),
    ("circlearrowleft", "↺"), ("circlearrowright", "↻"),
    // Large operators.
    ("sum", "∑"), ("prod", "∏"), ("coprod", "∐"), ("int", "∫"), ("iint", "∬"),
    ("iiint", "∭"), ("oint", "∮"), ("oiint", "∯"), ("bigcup", "⋃"), ("bigcap", "⋂"),
    ("bigsqcup", "⨆"), ("bigvee", "⋁"), ("bigwedge", "⋀"), ("bigoplus", "⨁"),
    ("bigotimes", "⨂"), ("bigodot", "⨀"), ("biguplus", "⨄"),
    // Delimiters.
    ("langle", "⟨"), ("rangle", "⟩"), ("lfloor", "⌊"), ("rfloor", "⌋"), ("lceil", "⌈"),
    ("rceil", "⌉"), ("lbrace", "\\{"), ("rbrace", "\\}"), ("lbrack", "\\["),
    ("rbrack", "\\]"), ("vert", "\\|"), ("Vert", "‖"), ("lvert", "\\|"), ("rvert", "\\|"),
    ("lVert", "‖"), ("rVert", "‖"), ("backslash", "\\\\"), ("ulcorner", "⌜"),
    ("urcorner", "⌝"), ("llcorner", "⌞"), ("lrcorner", "⌟"), ("llbracket", "⟦"),
    ("rrbracket", "⟧"),
    // Dots.
    ("dots", "…"), ("ldots", "…"), ("cdots", "⋯"), ("vdots", "⋮"), ("ddots", "⋱"),
    ("dotsc", "…"), ("dotsb", "⋯"), ("dotsm", "⋯"), ("dotsi", "⋯"), ("dotso", "…"),
    ("colon", "\\:"), ("lq", "‘"), ("rq", "’"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn typst(tex: &str) -> String {
        lower(tex).markup
    }

    #[test]
    fn letters_stay_apart_and_punctuation_escapes() {
        assert_eq!(typst("ab"), "a b");
        assert_eq!(typst("x-y=3.14"), "x − y \\= 3.14");
        assert_eq!(typst("f(x), [a|b]"), "f \\( x \\) \\, \\[ a \\| b \\]");
        assert_eq!(typst("a->b"), "a − \\> b");
    }

    #[test]
    fn commands_lower_to_calls_and_characters() {
        assert_eq!(typst("\\frac{a}{b}"), "frac(a, b)");
        assert_eq!(typst("\\frac12"), "frac(1, 2)");
        assert_eq!(typst("\\sqrt[3]{x}"), "root(3, x)");
        assert_eq!(typst("x_i^2"), "attach(x, b: i, t: 2)");
        assert_eq!(typst("\\sum_{i=1}^n"), "attach(∑, b: i \\= 1, t: n)");
        assert_eq!(typst("f''(x)"), "attach(f, t: ′ ′) \\( x \\)");
        assert_eq!(typst("\\alpha\\leq\\infty"), "α ≤ ∞");
        assert_eq!(typst("\\sin x"), "op(\"sin\") x");
        assert_eq!(typst("\\text{if } x"), "\"if \" x");
        assert_eq!(typst("\\mathbb{R}"), "bb(R)");
        assert_eq!(typst("\\hat x"), "accent(x, \"\u{302}\")");
        assert_eq!(typst("\\left( x \\right."), "lr(\\( x)");
        assert_eq!(typst("\\not\\in"), "∉");
        assert_eq!(typst("a\\,b"), "a #h(0.1667em) b");
    }

    #[test]
    fn environments_lower_to_matrices_cases_and_aligned_rows() {
        assert_eq!(typst("\\begin{pmatrix}a&b\\\\c&\\end{pmatrix}"), "mat(delim: \"(\", a, b; c, \"\")");
        assert_eq!(typst("\\begin{cases}1&x>0\\\\0&\\text{else}\\end{cases}"), "cases(1 & x \\> 0, 0 & \"else\")");
        assert_eq!(typst("\\begin{aligned}a&=b\\\\&=c\\end{aligned}"), "a & \\= b \\ & \\= c");
    }

    #[test]
    fn an_unknown_command_draws_its_name_and_is_named() {
        let lowered = lower("\\foo{x} + \\begin{weird}y\\end{weird}");
        assert_eq!(lowered.markup, "\"\\\\foo\" x \\+ y");
        assert_eq!(lowered.unknown, BTreeSet::from(["foo".to_string(), "begin{weird}".to_string()]));
    }

    /// What a symbol, accent or negation draws, the embedded math face covers.
    #[test]
    fn the_embedded_math_face_draws_every_character_the_vocabulary_writes() {
        let (name, data) = crate::world::MATH_FONT;
        let font = typst::text::Font::new(typst::foundations::Bytes::new(data.to_vec()), 0).expect(name);
        let written = SYMBOLS
            .iter()
            .chain(ACCENTS)
            .chain(NEGATED)
            .flat_map(|(_, s)| s.chars())
            .chain("−∗′".chars());
        let missing: String = written
            .filter(|&c| !c.is_ascii() && !font.info().coverage.contains(c as u32))
            .collect();
        assert!(missing.is_empty(), "{name} has no glyph for {missing}");
    }

    proptest::proptest! {
        /// Whatever the TeX, the equation parses as Typst markup.
        #[test]
        fn any_tex_lowers_to_markup_that_parses(tex in r##"(\\[a-zA-Z]{1,6}|\\.|[{}^_&~'\[\]()|,;:"#$%\\a-z0-9 .=+<>!?*/𑵆éα\u{302}-]|\\begin\{([a-z]+|aligned|cases|pmatrix)\}|\\end\{([a-z]+|aligned|cases|pmatrix)\}|\\left.|\\right.){0,24}"##, display: bool) {
            let markup = equation(&tex, display);
            let errors = typst::syntax::parse(&markup).errors_and_warnings().0;
            proptest::prop_assert!(errors.is_empty(), "{:?} lowered to {:?}: {:?}", tex, markup, errors);
        }
    }

    #[test]
    fn a_linebreak_in_a_group_ends_no_line_on_its_backslash() {
        for tex in [r"\begin{aligned}{a\\}\end{aligned}", r"\begin{cases}{a\\}\end{cases}", r"a\\"] {
            let markup = equation(tex, false);
            assert!(markup.ends_with(" $") || !markup.trim_end_matches('$').ends_with('\\'), "{tex:?} lowered to {markup:?}");
            let errors = typst::syntax::parse(&format!("{markup} after $b$")).errors_and_warnings().0;
            assert!(errors.is_empty(), "{tex:?} lowered to {markup:?}: {errors:?}");
        }
    }

    #[test]
    fn nesting_past_the_depth_draws_flat_and_marks_the_equation_deep() {
        for tex in ["{".repeat(5000), r"\sqrt{".repeat(2000), r"\left(".repeat(2000), r"\not".repeat(5000), r"\frac".repeat(5000)] {
            let lowered = lower(&tex);
            assert!(lowered.deep, "{}", &tex[..12]);
            let errors = typst::syntax::parse(&equation(&tex, false)).errors_and_warnings().0;
            assert!(errors.is_empty(), "{}: {errors:?}", &tex[..12]);
        }
        assert!(!lower(r"\frac{\sqrt{x^{2}}}{y}").deep);
    }

    #[test]
    fn what_draws_nothing_is_named_and_what_takes_an_argument_reads_it() {
        assert_eq!(lower(r"a \\[4pt] b").markup, "a \\  b");
        assert_eq!(lower(r"\begin{alignedat}{2} x \end{alignedat}").markup, "x");
        assert_eq!(lower(r"\begin{pmatrix*}[r] x \end{pmatrix*}").markup, "mat(delim: \"(\", x)");
        assert_eq!(lower("a % b").markup, "a");
        for tex in [r"\not{=}", r"\not", r"\left\lgroup x \right."] {
            assert!(!lower(tex).unknown.is_empty(), "{tex:?}");
        }
    }

    #[test]
    fn stray_braces_and_closers_lower_to_valid_markup() {
        for tex in ["}", "{", "a}b", "\\right)", "\\end{x}", "^", "_{", "\\sqrt[", "\\left(", "x^", "''", "&", "\\\\"] {
            let markup = equation(tex, false);
            let errors = typst::syntax::parse(&markup).errors_and_warnings().0;
            assert!(errors.is_empty(), "{tex:?} lowered to {markup:?}: {errors:?}");
        }
    }
}
