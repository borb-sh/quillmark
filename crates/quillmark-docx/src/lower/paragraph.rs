use crate::layout::{
    BracketHint, Child, HAlign, ItemKind, ItemRef, LayoutDoc, LineEnd, NodeId, NodeKind, ParStyle,
    PointHint, Rgb,
};
use crate::model::{
    Align, BreakKind, Field, FirstLine, FontDecl, FontFamily, Hyperlink, Indent, Inline, Line,
    LinkTarget, NumRef, Paragraph, Placement, Run, RunContent, RunProps, TabKind, TabLeader,
    TabStop, VertAlign,
};
use crate::notice::Code;

use super::sections::is_roman;
use super::space::{Entry, line_props};
use super::styles::style_id;
use super::{Ctx, Lower, hex, tw};
use std::collections::BTreeMap;

/// Families whose metric-compatible Word face a Word machine has.
pub(super) const WORD_CLONES: &[(&str, &str)] = &[
    ("TeX Gyre Termes", "Times New Roman"),
    ("Liberation Serif", "Times New Roman"),
    ("Liberation Sans", "Arial"),
    ("TeX Gyre Heros", "Arial"),
    ("Liberation Mono", "Courier New"),
    ("TeX Gyre Cursor", "Courier New"),
    ("Carlito", "Calibri"),
    ("Caladea", "Cambria"),
];

/// The widths, in ems of the larger text beside it, from which a gap inside
/// a line that no glyph sets reads as a word space, and as a tab. Kerning and
/// the boundary between two runs leave no gap, and justification widens only
/// the space glyphs, so a gap this wide is spacing the engine set between
/// words: a word space below the tab width, which Word cannot tell from one,
/// and a tab to where the next run starts from it.
const GAP_EMS: (f64, f64) = (0.2, 0.4);

/// A child a paragraph's lines read into: an inline element, or a bracket
/// the reader found the paragraph reads through.
pub(super) fn read_into(k: &NodeKind) -> bool {
    k.is_inline()
        || matches!(
            k,
            NodeKind::Block { .. } | NodeKind::Align { .. } | NodeKind::Hint(_)
        )
}

/// The style a paragraph-forming node's probes resolved: its own, else its
/// nearest block's.
pub(super) fn style_of(doc: &LayoutDoc, n: NodeId) -> Option<ParStyle> {
    match &doc.nodes[n].kind {
        NodeKind::Par { style: Some(s), .. } | NodeKind::InlineRun { style: Some(s) } => Some(*s),
        _ => doc
            .ancestor(n, |k| matches!(k, NodeKind::Block { .. }))
            .and_then(|b| match doc.nodes[b].kind {
                NodeKind::Block { style, .. } => Some(style),
                _ => None,
            }),
    }
}

/// Whether `url` names the `javascript:` scheme, read as a browser reads
/// it: past leading spaces and controls, with tabs and line breaks ignored.
pub(super) fn runs_script(url: &str) -> bool {
    let scheme: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .take_while(|&c| c != ':')
        .collect();
    scheme.eq_ignore_ascii_case("javascript") && url.contains(':')
}

/// The Word face for a Typst family: a metric-compatible clone a Word
/// machine has, else the face the quill's `font` hint names, else the family
/// itself.
pub(super) fn word_font(family: &str, alt_names: &BTreeMap<String, String>) -> String {
    WORD_CLONES
        .iter()
        .find(|(f, _)| f.eq_ignore_ascii_case(family))
        .map(|(_, w)| w.to_string())
        .or_else(|| alt_names.get(&family.to_lowercase()).cloned())
        .unwrap_or_else(|| family.to_string())
}

/// Identical neighbouring runs as one, and neighbours under one link as one
/// hyperlink.
pub(super) fn merge(items: Vec<(Option<String>, Inline)>) -> Vec<Inline> {
    let mut merged: Vec<(Option<String>, Inline)> = Vec::new();
    for (link, inline) in items {
        if let (Some((l, Inline::Run(prev))), Inline::Run(run)) = (merged.last_mut(), &inline) {
            let text_only = |r: &Run| r.content.iter().all(|c| matches!(c, RunContent::Text(_)));
            if *l == link && prev.props == run.props && text_only(prev) && text_only(run) {
                let add: String = run
                    .content
                    .iter()
                    .map(|c| match c {
                        RunContent::Text(t) => t.as_str(),
                        _ => "",
                    })
                    .collect();
                if let Some(RunContent::Text(t)) = prev.content.last_mut() {
                    t.push_str(&add);
                    continue;
                }
            }
        }
        merged.push((link, inline));
    }
    let mut out: Vec<Inline> = Vec::new();
    let mut open: Option<(String, Vec<Run>)> = None;
    for (link, inline) in merged {
        match (link, inline) {
            (Some(url), Inline::Run(run)) => match &mut open {
                Some((u, runs)) if *u == url => runs.push(run),
                _ => {
                    if let Some((u, runs)) = open.take() {
                        out.push(Inline::Hyperlink(Hyperlink {
                            target: LinkTarget::Url(u),
                            runs,
                        }));
                    }
                    open = Some((url, vec![run]));
                }
            },
            (_, inline) => {
                if let Some((u, runs)) = open.take() {
                    out.push(Inline::Hyperlink(Hyperlink {
                        target: LinkTarget::Url(u),
                        runs,
                    }));
                }
                out.push(inline);
            }
        }
    }
    if let Some((u, runs)) = open {
        out.push(Inline::Hyperlink(Hyperlink {
            target: LinkTarget::Url(u),
            runs,
        }));
    }
    out
}

impl<'a> Lower<'a> {
    pub(super) fn paragraph(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Entry> {
        let lines = self.doc.nodes[n].lines.clone();
        self.paragraph_of(n, lines, ctx)
    }

    /// The paragraph style a paragraph-forming node is set in: its own
    /// probe's, else its block's.
    pub(super) fn par_style(&self, n: NodeId) -> Option<ParStyle> {
        match self.kind(n) {
            NodeKind::Par { style, .. } | NodeKind::InlineRun { style } => *style,
            _ => self
                .doc
                .ancestor(n, |k| matches!(k, NodeKind::Block { .. }))
                .and_then(|b| match self.doc.nodes[b].kind {
                    NodeKind::Block { style, .. } => Some(style),
                    _ => None,
                }),
        }
    }

    /// One Word paragraph of some of `n`'s lines: all of them, or one line
    /// of text no element tags. The objects and number the node holds go
    /// with its first line, after the drawings it anchors.
    pub(super) fn paragraph_of(
        &mut self,
        n: NodeId,
        lines: Vec<crate::layout::Line>,
        ctx: &Ctx,
    ) -> Vec<Entry> {
        let node = &self.doc.nodes[n];
        let Some(first) = lines.first() else {
            return Vec::new();
        };
        let opens = node.lines.first().is_some_and(|l| l.items == first.items);
        let first_x = first.x0;
        let kind = node.kind.clone();
        let style = self.par_style(n);
        let pitch = self.strides.pitch(n, &lines, style);
        let tall = lines.iter().flat_map(|l| &l.items).any(|r| {
            matches!(&self.doc.item(*r).kind, ItemKind::Raster { rect, .. } if rect.height() > pitch)
        });
        let mut props = line_props(pitch, false);
        if tall {
            if let Some(sp) = props.spacing.as_mut() {
                sp.line = Some(Line::Auto(240));
            }
        }
        let justify = matches!(kind, NodeKind::Par { justify: true, .. });
        let align = self
            .read_align(n)
            .or(ctx.align)
            .or(style.map(|s| s.align))
            .unwrap_or_default();
        props.align = match (align, justify) {
            (HAlign::Center, _) => Some(Align::Center),
            (HAlign::End, _) => Some(Align::End),
            (HAlign::Start, true) => Some(Align::Both),
            (HAlign::Start, false) => None,
        };
        if align == HAlign::Start && ctx.list.is_none() {
            let (field_first, hanging, edge) = match kind {
                NodeKind::Par {
                    first_line,
                    hanging,
                    left,
                    ..
                } => (first_line, hanging, left.filter(|_| opens)),
                _ => (0.0, 0.0, None),
            };
            let (left, first_off) = if lines.len() >= 2 {
                let rest = lines[1..].iter().map(|l| l.x0).fold(f64::MAX, f64::min);
                (rest - ctx.left, first.x0 - rest)
            } else if let Some(edge) = edge {
                let rest = edge + hanging;
                (rest - ctx.left, first.x0 - rest)
            } else {
                let delta = first.x0 - ctx.left;
                if field_first > 0.0 && (delta - field_first).abs() < 0.5 {
                    (0.0, field_first)
                } else if hanging > 0.0 {
                    (delta + hanging, -hanging)
                } else {
                    (delta, 0.0)
                }
            };
            let first = if first_off.abs() < 0.25 {
                None
            } else if first_off > 0.0 {
                Some(FirstLine::Indent(tw(first_off)))
            } else {
                Some(FirstLine::Hanging(tw(-first_off)))
            };
            let left = (left.abs() >= 0.25).then_some(tw(left));
            if left.is_some() || first.is_some() {
                props.indent = Some(Indent {
                    left,
                    right: None,
                    first,
                });
            }
        }
        if let NodeKind::Heading {
            level, outlined, ..
        } = kind
        {
            let level = level.clamp(1, 9);
            props.style = Some(if outlined {
                self.headings.insert(level);
                format!("Heading{level}")
            } else {
                self.unlisted.insert(level);
                format!("UnlistedHeading{level}")
            });
        }
        let builtin = if matches!(kind, NodeKind::Caption) {
            Some(("Caption", "caption"))
        } else if self
            .doc
            .ancestor(n, |k| matches!(k, NodeKind::Quote))
            .is_some()
        {
            Some(("Quote", "Quote"))
        } else {
            None
        };
        if let (None, Some(style)) = (&props.style, builtin) {
            self.builtin.insert(style);
            props.style = Some(style.0.into());
        }
        if let Some(level) = ctx.toc {
            let level = level.clamp(1, 9);
            self.toc_levels.insert(level);
            props.style = Some(format!("TOC{level}"));
            props.tabs = vec![TabStop {
                kind: TabKind::Right,
                pos: tw(first.x1 - ctx.left),
                leader: TabLeader::Dot,
            }];
        }
        if let Some((num, level)) = ctx.list {
            props.numbering = Some(NumRef { level, num });
        }
        let mut out = Vec::new();
        if opens {
            out = self.inline_shapes(n);
            for place in self.inline_places(n) {
                let drawings = self.text_box(place, ctx);
                self.tally.count(Code::PlaceAnchored, "", 1);
                out.extend(drawings.into_iter().map(|d| Entry::Drawing(first.page, d)));
            }
        }
        let label = self.label_in(n).filter(|_| opens);
        let mut numbered = None;
        if let (Some(lab), Some((scheme, num, first))) = (label, ctx.hint_list) {
            if let NodeKind::Hint(BracketHint::Label { level, value }) = self.kind(lab).clone() {
                let l = (level as usize - 1).min(8);
                let counts = self.counters.entry(num).or_insert([0; 9]);
                let next = counts[l] + 1;
                if value.is_some_and(|v| v != next) {
                    self.tally.count(Code::LabelMismatch, "", 1);
                    props.numbering = None;
                } else {
                    counts[l] = next;
                    for d in counts.iter_mut().skip(l + 1) {
                        *d = 0;
                    }
                    numbered = Some((scheme, num, first, l, lab));
                }
            }
        }
        if let Some((scheme, num, first, l, lab)) = numbered {
            let at = self.doc.nodes[lab]
                .frames
                .first()
                .map_or(first_x, |f| f.1.x0)
                - ctx.left;
            for m in self.doc.subtree(lab) {
                for c in &self.doc.nodes[m].children {
                    if let Child::Item(r) = c {
                        self.consumed.insert(*r);
                        self.skip.insert(*r);
                    }
                }
            }
            let level = &self.schemes[scheme].levels[l];
            props.style = Some(format!("{}{}", style_id(&self.schemes[scheme].name), l + 1));
            props.numbering = (!first).then_some(NumRef {
                level: l as u8,
                num,
            });
            props.indent = ((at - level.indent).abs() > 0.72).then(|| Indent {
                left: Some(0),
                right: None,
                first: Some(if at >= 0.0 {
                    FirstLine::Indent(tw(at))
                } else {
                    FirstLine::Hanging(tw(-at))
                }),
            });
        } else if props.style.is_none() {
            if let Some((name, next)) = self.role_of(n) {
                let id = style_id(&name);
                self.roles.entry(id.clone()).or_insert((name, next));
                props.style = Some(id);
            }
        }
        let (mut content, stops) = self.runs(n, &lines, ctx);
        props.tabs.extend(stops);
        props.tabs.sort_by_key(|t| t.pos);
        props.tabs.dedup_by_key(|t| t.pos);
        if let Some(label) = self.label_prefix.take() {
            content.insert(
                0,
                Inline::Run(Run {
                    props: RunProps::default(),
                    content: vec![RunContent::Text(label), RunContent::Tab],
                }),
            );
        }
        let last = lines.last().expect("a line");
        let at = self.placed(n, ctx, style, first, last, pitch);
        out.push(Entry::Par(Paragraph { props, content }, at));
        out
    }

    /// The first `label` hint among `n`'s inline content.
    pub(super) fn label_in(&self, n: NodeId) -> Option<NodeId> {
        self.doc.nodes[n].children.iter().find_map(|c| match c {
            Child::Node(m) => match self.kind(*m) {
                NodeKind::Hint(BracketHint::Label { .. }) => Some(*m),
                k if k.is_inline() => self.label_in(*m),
                _ => None,
            },
            Child::Item(_) => None,
        })
    }

    /// The role a paragraph sits in, or that brackets its inline content.
    pub(super) fn role_of(&self, n: NodeId) -> Option<(String, Option<String>)> {
        let role = |k: &NodeKind| match k {
            NodeKind::Hint(BracketHint::Role { name, next }) => Some((name.clone(), next.clone())),
            _ => None,
        };
        let mut at = Some(n);
        while let Some(m) = at {
            if let Some(r) = role(self.kind(m)) {
                return Some(r);
            }
            at = self.doc.nodes[m].parent;
        }
        self.doc
            .subtree(n)
            .into_iter()
            .find_map(|m| role(self.kind(m)))
    }

    /// The places inside `n`'s inline content, which a paragraph cannot hold.
    pub(super) fn inline_places(&self, n: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for c in &self.doc.nodes[n].children {
            if let Child::Node(m) = c {
                match self.kind(*m) {
                    NodeKind::Place { .. } => out.push(*m),
                    k if read_into(k) => out.extend(self.inline_places(*m)),
                    _ => {}
                }
            }
        }
        out
    }

    /// The alignment of the innermost Align bracket, or block set centred or
    /// to the end, that a paragraph reads all its content through.
    pub(super) fn read_align(&self, n: NodeId) -> Option<HAlign> {
        let mut at = n;
        let mut found = None;
        loop {
            let mut nodes = self.doc.nodes[at].children.iter().filter(|c| match c {
                Child::Node(_) => true,
                Child::Item(r) => matches!(self.doc.item(*r).kind, ItemKind::Text(_)),
            });
            let (Some(Child::Node(m)), None) = (nodes.next(), nodes.next()) else {
                return found;
            };
            match self.kind(*m) {
                NodeKind::Align { h } => return Some(*h),
                NodeKind::Block { style, .. } => {
                    if style.align != HAlign::Start {
                        found = Some(style.align);
                    }
                    at = *m;
                }
                NodeKind::Hint(_) => at = *m,
                _ => return found,
            }
        }
    }

    /// The runs of `n`'s lines, joined into running text, and the left tab
    /// stops the tabs among them set to, from the container's edge.
    pub(super) fn runs(
        &mut self,
        n: NodeId,
        lines: &[crate::layout::Line],
        ctx: &Ctx,
    ) -> (Vec<Inline>, Vec<TabStop>) {
        let mut out: Vec<(Option<String>, Inline)> = Vec::new();
        let mut stops: Vec<TabStop> = Vec::new();
        let mut tabbed = false;
        for (li, line) in lines.iter().enumerate() {
            // Where the last text run that is not raised ends, its size, and
            // whether whitespace ends it.
            let mut prev: Option<(f64, f64, bool)> = None;
            let texts: Vec<ItemRef> = line.items.clone();
            let main = texts
                .iter()
                .filter_map(|r| match &self.doc.item(*r).kind {
                    ItemKind::Text(t) if !self.doc.raised(self.doc.item(*r).node, n) => {
                        Some(t.size)
                    }
                    _ => None,
                })
                .reduce(f64::max);
            let last_text = texts
                .iter()
                .rposition(|r| matches!(self.doc.item(*r).kind, ItemKind::Text(_)));
            for (i, r) in texts.iter().enumerate() {
                if self.skip.contains(r) {
                    prev = None;
                    continue;
                }
                self.consumed.insert(*r);
                let item = self.doc.item(*r);
                if !matches!(item.kind, ItemKind::Text(_)) {
                    prev = None;
                }
                match &item.kind {
                    ItemKind::Text(t) => {
                        let chain = self.chain(item.node, n);
                        let fill = chain.iter().any(|k| matches!(k, NodeKind::TocFill));
                        let page = chain.iter().any(|k| matches!(k, NodeKind::TocPage));
                        let raised = self.doc.raised(item.node, n);
                        let before = prev.take();
                        let mut gap = None;
                        if !raised && !fill && !page && !tabbed {
                            prev = Some((
                                t.origin[0] + t.width,
                                t.size,
                                t.text.ends_with(char::is_whitespace),
                            ));
                            if let Some((end, size, spaced)) = before {
                                let em = size.max(t.size);
                                let spaced = spaced || t.text.starts_with(char::is_whitespace);
                                let wide = t.origin[0] - end;
                                gap = if wide >= GAP_EMS.1 * em {
                                    stops.push(TabStop {
                                        kind: TabKind::Left,
                                        pos: tw(t.origin[0] - ctx.left),
                                        leader: TabLeader::None,
                                    });
                                    Some(RunContent::Tab)
                                } else if wide >= GAP_EMS.0 * em && !spaced {
                                    Some(RunContent::Text(" ".into()))
                                } else {
                                    None
                                };
                            }
                        }
                        if (fill || page) && !tabbed {
                            tabbed = true;
                            if let Some((_, Inline::Run(r))) = out.last_mut() {
                                if let Some(RunContent::Text(t)) = r.content.last_mut() {
                                    let trimmed = t.trim_end().len();
                                    t.truncate(trimmed);
                                }
                            }
                            out.push((
                                None,
                                Inline::Run(Run {
                                    props: RunProps::default(),
                                    content: vec![RunContent::Tab],
                                }),
                            ));
                        }
                        if fill || (tabbed && !page && t.text.trim().is_empty()) {
                            continue;
                        }
                        let shown = t
                            .text
                            .trim_matches(|c: char| c.is_whitespace() || c == '\u{2060}');
                        if page && shown.is_empty() {
                            continue;
                        }
                        if page {
                            let numeral = shown.chars().all(|c| c.is_ascii_digit())
                                || is_roman(shown, false)
                                || is_roman(shown, true);
                            self.toc_prefixed |= !numeral;
                        }
                        let nobreak = chain.iter().any(|k| matches!(k, NodeKind::NoBreak));
                        let shaped = |s: &str| {
                            if nobreak {
                                s.replace(' ', "\u{a0}").replace('-', "\u{2011}")
                            } else {
                                s.to_string()
                            }
                        };
                        let mut text = shaped(if page { shown } else { &t.text });
                        if Some(i) == last_text && line.end == LineEnd::Hyphenated {
                            text = text.trim_end_matches('\u{ad}').to_string();
                        }
                        let mut props = self.run_props(t, &chain);
                        if let (Some(_), Some(main)) = (props.vert, main) {
                            props.size = Some((main * 2.0).round().max(2.0) as u32);
                        }
                        if let Some(content) = gap {
                            out.push((
                                None,
                                Inline::Run(Run {
                                    props: RunProps {
                                        underline: false,
                                        strike: false,
                                        shade: None,
                                        ..props.clone()
                                    },
                                    content: vec![content],
                                }),
                            ));
                        }
                        let link = chain.iter().find_map(|k| match k {
                            NodeKind::Link { url } if !runs_script(url) => Some(url.clone()),
                            _ => None,
                        });
                        let number = ctx.page_number.and_then(|p| {
                            let (a, b) = self.pages.number_in(&t.text, p)?;
                            let found = self.pages.printed(p);
                            if found.is_none() {
                                self.tally.count(Code::PageNumberFrozen, "", 1);
                            }
                            Some((a, b, found?))
                        });
                        if let Some((a, b, found)) = number {
                            let run = |text: String| Run {
                                props: props.clone(),
                                content: vec![RunContent::Text(text)],
                            };
                            let before = shaped(&format!("{}{}", &t.text[..a], found.before));
                            let after = shaped(&format!("{}{}", found.after, &t.text[b..]));
                            if !before.is_empty() {
                                out.push((link.clone(), Inline::Run(run(before))));
                            }
                            out.push((
                                link.clone(),
                                Inline::Field(Field {
                                    instruction: "PAGE".into(),
                                    props: props.clone(),
                                    result: vec![run(found.digits.clone())],
                                    locked: false,
                                }),
                            ));
                            if !after.is_empty() {
                                out.push((link, Inline::Run(run(after))));
                            }
                            continue;
                        }
                        out.push((
                            link,
                            Inline::Run(Run {
                                props,
                                content: vec![RunContent::Text(text)],
                            }),
                        ));
                    }
                    ItemKind::Raster { image, rect, alt } => {
                        let drawing = self.picture(*image, *rect, alt.clone(), Placement::Inline);
                        let position = (2.0 * (line.baseline - rect.y1)).round() as i32;
                        out.push((
                            None,
                            Inline::Run(Run {
                                props: RunProps {
                                    position: (position != 0).then_some(position),
                                    ..RunProps::default()
                                },
                                content: vec![RunContent::Drawing(drawing)],
                            }),
                        ));
                    }
                    ItemKind::Mark(PointHint::FootnoteRef { note }) => {
                        let index = self.footnote(*note);
                        out.push((
                            None,
                            Inline::Run(Run {
                                props: RunProps {
                                    vert: Some(VertAlign::Superscript),
                                    ..RunProps::default()
                                },
                                content: vec![RunContent::FootnoteRef(index)],
                            }),
                        ));
                    }
                    _ => {}
                }
            }
            if li + 1 < lines.len() && line.end == LineEnd::Forced {
                out.push((
                    None,
                    Inline::Run(Run {
                        props: RunProps::default(),
                        content: vec![RunContent::Break(BreakKind::Line)],
                    }),
                ));
            }
        }
        (merge(out), stops)
    }

    /// The kinds from `item_node` up to and including `top`.
    pub(super) fn chain(&self, item_node: NodeId, top: NodeId) -> Vec<&'a NodeKind> {
        let mut out = Vec::new();
        let mut at = Some(item_node);
        while let Some(n) = at {
            out.push(&self.doc.nodes[n].kind);
            if n == top {
                break;
            }
            at = self.doc.nodes[n].parent;
        }
        out
    }

    pub(super) fn run_props(
        &mut self,
        t: &crate::layout::TextRun,
        chain: &[&NodeKind],
    ) -> RunProps {
        let face = &self.doc.fonts[t.font];
        let font = self.font(&face.family, face.monospace);
        let has = |f: fn(&NodeKind) -> bool| chain.iter().any(|k| f(k));
        RunProps {
            font: Some(font),
            bold: face.bold.then_some(true),
            italic: face.italic.then_some(true),
            strike: has(|k| matches!(k, NodeKind::Strike)),
            underline: has(|k| matches!(k, NodeKind::Underline)),
            color: (t.fill != Rgb(0, 0, 0)).then(|| hex(t.fill)),
            size: Some((t.size * 2.0).round().max(2.0) as u32),
            shade: chain.iter().find_map(|k| match k {
                NodeKind::Highlight { fill } => Some(hex(*fill)),
                _ => None,
            }),
            vert: if has(|k| matches!(k, NodeKind::Super)) {
                Some(VertAlign::Superscript)
            } else if has(|k| matches!(k, NodeKind::Sub)) {
                Some(VertAlign::Subscript)
            } else {
                None
            },
            no_proof: has(|k| matches!(k, NodeKind::RawLine)),
            ..RunProps::default()
        }
    }

    pub(super) fn font(&mut self, family: &str, monospace: bool) -> String {
        let name = word_font(family, &self.alt_names);
        if name == family {
            self.tally.count(Code::FontNotEmbedded, family, 1);
        }
        self.fonts.entry(name.clone()).or_insert_with(|| FontDecl {
            name: name.clone(),
            alt_name: (name != family).then(|| family.to_string()),
            family: if monospace {
                FontFamily::Modern
            } else if name.contains("Arial") || name.contains("Calibri") {
                FontFamily::Swiss
            } else {
                FontFamily::Roman
            },
            pitch_fixed: monospace,
        });
        name
    }
}
