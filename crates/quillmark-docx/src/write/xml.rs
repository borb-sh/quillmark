//! A minimal XML emitter. Text and attribute values are escaped here and
//! nowhere else, and a character XML 1.0 cannot carry is dropped, so any
//! string the model holds serializes to well-formed XML.

use std::fmt::Write as _;

pub(crate) struct Xml {
    out: String,
}

/// Whether XML 1.0 can carry `c` at all.
fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

pub(crate) fn escape_into(out: &mut String, s: &str, attr: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\t' | '\n' if attr => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c if allowed(c) => out.push(c),
            _ => {}
        }
    }
}

impl Xml {
    pub(crate) fn new() -> Self {
        let mut out = String::with_capacity(4096);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        Self { out }
    }

    fn start(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(tag);
        for (k, v) in attrs {
            self.out.push(' ');
            self.out.push_str(k);
            self.out.push_str("=\"");
            escape_into(&mut self.out, v, true);
            self.out.push('"');
        }
    }

    pub(crate) fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.start(tag, attrs);
        self.out.push('>');
    }

    pub(crate) fn close(&mut self, tag: &str) {
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push('>');
    }

    pub(crate) fn empty(&mut self, tag: &str, attrs: &[(&str, &str)]) {
        self.start(tag, attrs);
        self.out.push_str("/>");
    }

    /// `<tag w:val="val"/>`.
    pub(crate) fn val(&mut self, tag: &str, val: &str) {
        self.empty(tag, &[("w:val", val)]);
    }

    pub(crate) fn text(&mut self, s: &str) {
        escape_into(&mut self.out, s, false);
    }

    /// An element holding only `text`.
    pub(crate) fn leaf(&mut self, tag: &str, attrs: &[(&str, &str)], text: &str) {
        self.open(tag, attrs);
        self.text(text);
        self.close(tag);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.out.into_bytes()
    }
}
