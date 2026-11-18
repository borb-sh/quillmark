//! Lowering: a [`LayoutDoc`] to a Word [`Document`].
//!
//! One walk of the tree in reading order. Each paragraph-forming node is one
//! Word paragraph whose lines are joined back into running text; its kind
//! decides its style, numbering and keeps, and measurement decides only the
//! numbers: indents, spacing, line pitch, widths. Chrome and placed content
//! become page-anchored text boxes. Every text and image item the walk does
//! not place is counted under `docx::ink_dropped`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::layout::{
    ArtifactKind, Child, Geometry, HAlign, ImageFormat, ItemKind, ItemRef, LayoutDoc, LineEnd,
    Marker, NodeId, NodeKind, Notice, ParStyle, PointHint, Rect, Rgb,
};
use crate::model::{
    AbstractNum, Align, Anchor, Block, Blocks, Border, Borders, BreakKind, Cell, CellMargins,
    CellProps, Columns, Document, Drawing, DrawingContent, Field, FirstLine, FontDecl, FontFamily,
    Footnote, HeaderRefs, Hyperlink, Indent, Info, Inline, Level, LevelSuffix, Line, LinkTarget,
    Media, MediaKind, Num, NumFormat, NumRef, PageMargins, PageNumbers, ParProps, Paragraph,
    Placement, Row, RowProps, Run, RunContent, RunProps, Section, SectionProps, SectionStart,
    Settings, Spacing, Style, Styles, TabKind, TabLeader, TabStop, Table, TableProps, TextBox,
    Twips, VMerge, VertAlign, Wrap,
};

pub const CODE_FONT_NOT_EMBEDDED: &str = "docx::font_not_embedded";
pub const CODE_PLACE_ANCHORED: &str = "docx::place_anchored";
pub const CODE_SHAPE_DROPPED: &str = "docx::shape_dropped";
pub const CODE_INK_DROPPED: &str = "docx::ink_dropped";

/// Families whose metric-compatible Word face a Word machine has.
const WORD_CLONES: &[(&str, &str)] = &[
    ("TeX Gyre Termes", "Times New Roman"),
    ("Liberation Serif", "Times New Roman"),
    ("Liberation Sans", "Arial"),
    ("TeX Gyre Heros", "Arial"),
    ("Liberation Mono", "Courier New"),
    ("TeX Gyre Cursor", "Courier New"),
    ("Carlito", "Calibri"),
    ("Caladea", "Cambria"),
];

const EMU_PER_PT: f64 = 12700.0;

fn tw(pt: f64) -> Twips {
    (pt * 20.0).round() as Twips
}

fn emu(pt: f64) -> i64 {
    (pt * EMU_PER_PT).round() as i64
}

fn hex(c: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// The Word document for `doc` and what the export carries less of.
pub fn lower(doc: &LayoutDoc) -> (Document, Vec<Notice>) {
    let mut l = Lower {
        doc,
        consumed: HashSet::new(),
        fonts: BTreeMap::new(),
        unmapped: BTreeSet::new(),
        numbering: Default::default(),
        footnotes: Vec::new(),
        notes: HashMap::new(),
        media: Vec::new(),
        media_of: HashMap::new(),
        pending: Vec::new(),
        z: 0,
        break_pending: false,
        prev: None,
        placed: 0,
        headings: BTreeSet::new(),
        skip: HashSet::new(),
    };
    let document = l.document();
    let mut notices = doc.notices.clone();
    notices.extend(l.notices());
    (document, notices)
}

#[derive(Clone, Copy)]
enum Prev {
    Baseline(u32, f64),
    Bottom(u32, f64),
}

#[derive(Clone, Default)]
struct Ctx {
    align: Option<HAlign>,
    /// The x of the container's left edge, which indents are measured from.
    left: f64,
    /// The container's width, for centred chrome.
    width: f64,
    /// Inside a cell, text box or footnote: no space above the first line
    /// and no page breaks.
    nested: bool,
    page_number: Option<String>,
    list: Option<(u32, u8)>,
}

struct Lower<'a> {
    doc: &'a LayoutDoc,
    consumed: HashSet<ItemRef>,
    fonts: BTreeMap<String, FontDecl>,
    unmapped: BTreeSet<String>,
    numbering: crate::model::Numbering,
    footnotes: Vec<Footnote>,
    notes: HashMap<NodeId, usize>,
    media: Vec<Media>,
    media_of: HashMap<usize, usize>,
    pending: Vec<Drawing>,
    z: u32,
    break_pending: bool,
    prev: Option<Prev>,
    placed: usize,
    headings: BTreeSet<u8>,
    skip: HashSet<ItemRef>,
}

impl Lower<'_> {
    fn kind(&self, n: NodeId) -> &NodeKind {
        &self.doc.nodes[n].kind
    }

    fn first_page(&self, n: NodeId) -> Option<u32> {
        self.doc.nodes[n].frames.first().map(|f| f.0)
    }

    fn document(&mut self) -> Document {
        let doc = self.doc;
        let groups = section_pages(doc);
        let mut sections: Vec<Section> = Vec::new();
        let mut bodies: Vec<Vec<Block>> = vec![Vec::new(); groups.len()];
        let section_of = |page: u32| {
            groups
                .iter()
                .position(|g| g.contains(&(page as usize)))
                .unwrap_or(0)
        };
        let root = doc.nodes[0].children.clone();
        let mut current = 0;
        for c in root {
            let page = match c {
                Child::Node(n) => {
                    if matches!(self.kind(n), NodeKind::Artifact { .. } | NodeKind::Footnote) {
                        continue;
                    }
                    self.first_page(n)
                }
                Child::Item(r) => Some(r.page),
            };
            if let Some(p) = page {
                current = section_of(p);
            }
            let left = doc.pages[groups[current][0]].setup.margins.left;
            let ctx = Ctx {
                left,
                ..Ctx::default()
            };
            let blocks = self.child(&c, &ctx);
            bodies[current].extend(blocks);
        }
        if !self.pending.is_empty() {
            let last = bodies.last_mut().expect("a section");
            last.push(Block::Paragraph(Paragraph::default()));
            self.attach_pending(last);
        }
        let mut headers = Vec::new();
        let mut footers = Vec::new();
        let mut prev_number: Option<u64> = None;
        for (i, (pages, body)) in groups.iter().zip(bodies).enumerate() {
            let first = &doc.pages[pages[0]];
            let s = first.setup;
            let (header, footer, title_page) = self.chrome(pages, &mut headers, &mut footers);
            let format = first.number_text.as_deref().and_then(number_format);
            let restart = prev_number.is_none_or(|p| p + 1 != first.number) && first.number != 1
                || prev_number.is_some_and(|p| p + 1 != first.number);
            let page_numbers = format.map(|format| PageNumbers {
                format,
                start: restart.then_some(first.number as u32),
            });
            prev_number = Some(doc.pages[*pages.last().expect("a page")].number);
            sections.push(Section {
                body: Blocks::from_blocks(body),
                props: SectionProps {
                    header,
                    footer,
                    start: if i == 0 {
                        SectionStart::NextPage
                    } else {
                        SectionStart::NextPage
                    },
                    page_width: tw(first.size[0]),
                    page_height: tw(first.size[1]),
                    landscape: s.flipped,
                    margins: PageMargins {
                        top: tw(s.margins.top),
                        right: tw(s.margins.right),
                        bottom: tw(s.margins.bottom),
                        left: tw(s.margins.left),
                        header: tw((s.margins.top / 2.0).min(36.0)),
                        footer: tw((s.margins.bottom / 2.0).min(36.0)),
                    },
                    page_numbers,
                    columns: (s.columns > 1).then_some(Columns {
                        count: s.columns,
                        space: tw(s.gutter),
                    }),
                    title_page,
                },
            });
        }
        Document {
            sections,
            headers,
            footers,
            footnotes: std::mem::take(&mut self.footnotes),
            styles: self.styles(),
            numbering: std::mem::take(&mut self.numbering),
            fonts: self.fonts.values().cloned().collect(),
            media: std::mem::take(&mut self.media),
            settings: Settings {
                default_tab_stop: 720,
                even_and_odd_headers: false,
            },
            info: Info {
                title: doc.info.title.clone(),
                language: doc.info.language.clone(),
            },
        }
    }

    fn notices(&self) -> Vec<Notice> {
        let mut out = Vec::new();
        for family in &self.unmapped {
            out.push(Notice {
                code: CODE_FONT_NOT_EMBEDDED.into(),
                message: format!(
                    "the Word file names the font {family} without embedding it, so Word \
                     substitutes another where it is not installed"
                ),
            });
        }
        if self.placed > 0 {
            out.push(Notice {
                code: CODE_PLACE_ANCHORED.into(),
                message: format!(
                    "{} placed object(s) are anchored to the page beside the paragraph that \
                     follows them, so they do not move with the text",
                    self.placed
                ),
            });
        }
        let mut shapes = 0;
        let mut ink = 0;
        for (p, page) in self.doc.pages.iter().enumerate() {
            for (i, item) in page.items.iter().enumerate() {
                let r = ItemRef {
                    page: p as u32,
                    index: i as u32,
                };
                if self.consumed.contains(&r) {
                    continue;
                }
                match item.kind {
                    ItemKind::Text(_) | ItemKind::Image { .. } => ink += 1,
                    ItemKind::Shape(_) => shapes += 1,
                    ItemKind::Mark(_) => {}
                }
            }
        }
        if shapes > 0 {
            out.push(Notice {
                code: CODE_SHAPE_DROPPED.into(),
                message: format!(
                    "{shapes} drawn line(s) or box(es) that no table or decoration explains \
                     are left out of the Word file"
                ),
            });
        }
        if ink > 0 {
            out.push(Notice {
                code: CODE_INK_DROPPED.into(),
                message: format!("{ink} piece(s) of text or image are left out of the Word file"),
            });
        }
        out
    }

    fn styles(&self) -> Styles {
        let mut styles = vec![Style {
            quick: true,
            ..Style::paragraph("Normal", "Normal", true)
        }];
        for &level in &self.headings {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                quick: true,
                ui_priority: Some(9),
                par: ParProps {
                    keep_next: true,
                    outline_level: Some(level - 1),
                    ..ParProps::default()
                },
                ..Style::paragraph(format!("Heading{level}"), format!("heading {level}"), true)
            });
        }
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        let mut families: HashMap<String, usize> = HashMap::new();
        for page in &self.doc.pages {
            for item in &page.items {
                if let ItemKind::Text(t) = &item.kind {
                    *sizes.entry((t.size * 2.0).round() as u32).or_default() += t.text.len();
                    let f = word_font(&self.doc.fonts[t.font].family);
                    *families.entry(f).or_default() += t.text.len();
                }
            }
        }
        fn top<K: Clone + Ord>(m: &HashMap<K, usize>) -> Option<K> {
            let mut v: Vec<(&K, &usize)> = m.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            v.first().map(|e| e.0.clone())
        }
        Styles {
            default_run: RunProps {
                font: top(&families),
                size: top(&sizes),
                ..RunProps::default()
            },
            default_par: ParProps {
                spacing: Some(Spacing {
                    before: None,
                    after: Some(0),
                    line: None,
                }),
                ..ParProps::default()
            },
            styles,
        }
    }

    fn child(&mut self, c: &Child, ctx: &Ctx) -> Vec<Block> {
        match c {
            Child::Node(n) => self.node(*n, ctx),
            Child::Item(r) => self.loose_item(*r, ctx),
        }
    }

    fn children(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Block> {
        let kids = self.doc.nodes[n].children.clone();
        let mut out = Vec::new();
        for c in &kids {
            out.extend(self.child(c, ctx));
        }
        out
    }

    fn loose_item(&mut self, r: ItemRef, ctx: &Ctx) -> Vec<Block> {
        match &self.doc.item(r).kind {
            ItemKind::Mark(PointHint::Break { .. }) => {
                self.consumed.insert(r);
                if !ctx.nested {
                    self.break_pending = true;
                }
                Vec::new()
            }
            ItemKind::Image { image, rect, alt } => {
                self.consumed.insert(r);
                let drawing = self.picture(*image, *rect, alt.clone(), Placement::Inline);
                let mut p = Paragraph {
                    props: self.par_frame(ctx, None, r.page, rect.y1, rect.height(), true),
                    content: vec![Inline::Run(Run {
                        props: RunProps::default(),
                        content: vec![RunContent::Drawing(drawing)],
                    })],
                };
                self.apply_break(&mut p, r.page, ctx);
                self.prev = Some(Prev::Bottom(r.page, rect.y1));
                vec![Block::Paragraph(p)]
            }
            _ => Vec::new(),
        }
    }

    fn node(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Block> {
        let kind = self.kind(n).clone();
        if kind.forms_paragraph() && !self.doc.nodes[n].lines.is_empty() {
            return self
                .paragraph(n, ctx)
                .into_iter()
                .map(Block::Paragraph)
                .collect();
        }
        match kind {
            NodeKind::Table { .. } => vec![Block::Table(self.table(n, ctx))],
            NodeKind::List { .. } => self.list(n, ctx),
            NodeKind::Block {
                sticky, breakable, ..
            } => {
                let mut blocks = self.children(n, ctx);
                let count = blocks.len();
                for (i, b) in blocks.iter_mut().enumerate() {
                    if let Block::Paragraph(p) = b {
                        if !breakable {
                            p.props.keep_lines = true;
                            if i + 1 < count {
                                p.props.keep_next = true;
                            }
                        }
                        if sticky && i + 1 == count {
                            p.props.keep_next = true;
                        }
                    }
                }
                blocks
            }
            NodeKind::Align { h } => {
                let ctx = Ctx {
                    align: Some(h),
                    ..ctx.clone()
                };
                self.children(n, &ctx)
            }
            NodeKind::Place { .. } => {
                let drawings = self.text_box(n, ctx);
                self.placed += 1;
                self.pending.extend(drawings);
                Vec::new()
            }
            NodeKind::ListLabel => {
                self.consume_subtree(n);
                Vec::new()
            }
            _ => self.children(n, ctx),
        }
    }

    fn consume_subtree(&mut self, n: NodeId) {
        for m in self.doc.subtree(n) {
            for c in &self.doc.nodes[m].children {
                if let Child::Item(r) = c {
                    self.consumed.insert(*r);
                }
            }
        }
    }

    fn apply_break(&mut self, p: &mut Paragraph, page: u32, ctx: &Ctx) {
        if ctx.nested {
            return;
        }
        let page_of_prev = match self.prev {
            Some(Prev::Baseline(p, _) | Prev::Bottom(p, _)) => Some(p),
            None => None,
        };
        if self.break_pending && page_of_prev.is_some_and(|q| q < page) {
            p.props.page_break_before = true;
        }
        self.break_pending = false;
        if !self.pending.is_empty() {
            let drawings = std::mem::take(&mut self.pending);
            p.content.insert(
                0,
                Inline::Run(Run {
                    props: RunProps::default(),
                    content: drawings.into_iter().map(RunContent::Drawing).collect(),
                }),
            );
        }
    }

    fn attach_pending(&mut self, blocks: &mut [Block]) {
        if let Some(Block::Paragraph(p)) = blocks.last_mut() {
            let drawings = std::mem::take(&mut self.pending);
            p.content.push(Inline::Run(Run {
                props: RunProps::default(),
                content: drawings.into_iter().map(RunContent::Drawing).collect(),
            }));
        }
    }

    /// Paragraph properties from where a block of `height` whose first
    /// baseline is `baseline` sits.
    fn par_frame(
        &self,
        ctx: &Ctx,
        style: Option<ParStyle>,
        page: u32,
        baseline: f64,
        pitch: f64,
        object: bool,
    ) -> ParProps {
        let before = if ctx.nested {
            0.0
        } else {
            match self.prev {
                Some(Prev::Baseline(p, y)) if p == page => baseline - y - pitch,
                Some(Prev::Bottom(p, y)) if p == page => baseline - y - 0.8 * pitch,
                Some(_) if !self.break_pending => style.map_or(0.0, |s| s.spacing),
                _ => {
                    let top = self.doc.pages[page as usize].setup.margins.top;
                    baseline - top - 0.8 * pitch
                }
            }
        };
        let before = if object {
            before.max(0.0) - 0.2 * pitch
        } else {
            before
        };
        ParProps {
            spacing: Some(Spacing {
                before: Some(tw(before.max(0.0))),
                after: None,
                line: (!object).then_some(Line::Exact(tw(pitch))),
            }),
            ..ParProps::default()
        }
    }

    fn paragraph(&mut self, n: NodeId, ctx: &Ctx) -> Option<Paragraph> {
        let node = &self.doc.nodes[n];
        let lines = node.lines.clone();
        let first = lines.first()?;
        let kind = node.kind.clone();
        let style = match &kind {
            NodeKind::Par { style, .. } | NodeKind::InlineRun { style } => *style,
            _ => self
                .doc
                .ancestor(n, |k| matches!(k, NodeKind::Block { .. }))
                .and_then(|b| match self.doc.nodes[b].kind {
                    NodeKind::Block { style, .. } => Some(style),
                    _ => None,
                }),
        };
        let size = lines
            .iter()
            .flat_map(|l| &l.items)
            .find_map(|r| match &self.doc.item(*r).kind {
                ItemKind::Text(t) => Some(t.size),
                _ => None,
            })
            .unwrap_or(11.0);
        let strides: Vec<f64> = lines
            .windows(2)
            .filter(|w| w[0].page == w[1].page)
            .map(|w| w[1].baseline - w[0].baseline)
            .collect();
        let pitch = if strides.is_empty() {
            let leading = style.map_or(0.65 * size, |s| s.leading);
            0.7 * size + leading
        } else {
            let mut s = strides.clone();
            s.sort_by(f64::total_cmp);
            s[s.len() / 2]
        };
        let mut props = self.par_frame(ctx, style, first.page, first.baseline, pitch, false);
        let justify = matches!(kind, NodeKind::Par { justify: true, .. });
        let align = ctx.align.or(style.map(|s| s.align)).unwrap_or_default();
        props.align = match (align, justify) {
            (HAlign::Center, _) => Some(Align::Center),
            (HAlign::End, _) => Some(Align::End),
            (HAlign::Start, true) => Some(Align::Both),
            (HAlign::Start, false) => None,
        };
        if align == HAlign::Start && ctx.list.is_none() {
            let (field_first, hanging) = match kind {
                NodeKind::Par {
                    first_line,
                    hanging,
                    ..
                } => (first_line, hanging),
                _ => (0.0, 0.0),
            };
            let (left, first_off) = if lines.len() >= 2 {
                let rest = lines[1..].iter().map(|l| l.x0).fold(f64::MAX, f64::min);
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
        if let NodeKind::Heading { level, .. } = kind {
            let level = level.clamp(1, 9);
            self.headings.insert(level);
            props.style = Some(format!("Heading{level}"));
        }
        if let Some((num, level)) = ctx.list {
            props.numbering = Some(NumRef { level, num });
        }
        for place in self.inline_places(n) {
            let drawings = self.text_box(place, ctx);
            self.placed += 1;
            self.pending.extend(drawings);
        }
        let mut para = Paragraph {
            props,
            content: self.runs(n, &lines, ctx),
        };
        self.apply_break(&mut para, first.page, ctx);
        let last = lines.last().expect("a line");
        self.prev = Some(Prev::Baseline(last.page, last.baseline));
        Some(para)
    }

    /// The places inside `n`'s inline content, which a paragraph cannot hold.
    fn inline_places(&self, n: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for c in &self.doc.nodes[n].children {
            if let Child::Node(m) = c {
                match self.kind(*m) {
                    NodeKind::Place { .. } => out.push(*m),
                    k if k.is_inline() => out.extend(self.inline_places(*m)),
                    _ => {}
                }
            }
        }
        out
    }

    /// The runs of `n`'s lines, joined into running text.
    fn runs(&mut self, n: NodeId, lines: &[crate::layout::Line], ctx: &Ctx) -> Vec<Inline> {
        let mut out: Vec<(Option<String>, Inline)> = Vec::new();
        for (li, line) in lines.iter().enumerate() {
            let texts: Vec<ItemRef> = line.items.clone();
            let last_text = texts
                .iter()
                .rposition(|r| matches!(self.doc.item(*r).kind, ItemKind::Text(_)));
            for (i, r) in texts.iter().enumerate() {
                if self.skip.contains(r) {
                    continue;
                }
                self.consumed.insert(*r);
                let item = self.doc.item(*r);
                match &item.kind {
                    ItemKind::Text(t) => {
                        let chain = self.chain(item.node, n);
                        let mut text = t.text.clone();
                        if chain.iter().any(|k| matches!(k, NodeKind::NoBreak)) {
                            text = text.replace(' ', "\u{a0}").replace('-', "\u{2011}");
                        }
                        if Some(i) == last_text && line.end == LineEnd::Hyphenated {
                            text = text.trim_end_matches('\u{ad}').to_string();
                        }
                        let props = self.run_props(t, &chain);
                        let link = chain.iter().find_map(|k| match k {
                            NodeKind::Link { url } => Some(url.clone()),
                            _ => None,
                        });
                        let is_number = ctx
                            .page_number
                            .as_deref()
                            .is_some_and(|s| !s.is_empty() && text.trim() == s);
                        let inline = if is_number {
                            Inline::Field(Field {
                                instruction: "PAGE".into(),
                                props: props.clone(),
                                result: vec![Run {
                                    props,
                                    content: vec![RunContent::Text(text.trim().to_string())],
                                }],
                                locked: false,
                            })
                        } else {
                            Inline::Run(Run {
                                props,
                                content: vec![RunContent::Text(text)],
                            })
                        };
                        out.push((link, inline));
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
        merge(out)
    }

    /// The kinds from `item_node` up to and including `top`.
    fn chain(&self, item_node: NodeId, top: NodeId) -> Vec<NodeKind> {
        let mut out = Vec::new();
        let mut at = Some(item_node);
        while let Some(n) = at {
            out.push(self.doc.nodes[n].kind.clone());
            if n == top {
                break;
            }
            at = self.doc.nodes[n].parent;
        }
        out
    }

    fn run_props(&mut self, t: &crate::layout::TextRun, chain: &[NodeKind]) -> RunProps {
        let face = &self.doc.fonts[t.font];
        let font = self.font(&face.family, face.monospace);
        let has = |f: fn(&NodeKind) -> bool| chain.iter().any(f);
        RunProps {
            font: Some(font),
            bold: face.bold.then_some(true),
            italic: face.italic.then_some(true),
            strike: has(|k| matches!(k, NodeKind::Strike)),
            underline: has(|k| matches!(k, NodeKind::Underline)),
            color: (t.fill != Rgb(0, 0, 0)).then(|| hex(t.fill)),
            size: Some((t.size * 2.0).round().max(2.0) as u32),
            shade: has(|k| matches!(k, NodeKind::Highlight)).then(|| "FFFF00".to_string()),
            vert: if has(|k| matches!(k, NodeKind::Super)) {
                Some(VertAlign::Superscript)
            } else if has(|k| matches!(k, NodeKind::Sub)) {
                Some(VertAlign::Subscript)
            } else {
                None
            },
            ..RunProps::default()
        }
    }

    fn font(&mut self, family: &str, monospace: bool) -> String {
        let name = word_font(family);
        if name == family {
            self.unmapped.insert(family.to_string());
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

    fn footnote(&mut self, note: NodeId) -> usize {
        if let Some(&i) = self.notes.get(&note) {
            return i;
        }
        let i = self.footnotes.len();
        self.notes.insert(note, i);
        self.footnotes.push(Footnote {
            body: Blocks::default(),
        });
        let saved = (
            self.prev,
            self.break_pending,
            std::mem::take(&mut self.pending),
        );
        let ctx = Ctx {
            nested: true,
            left: self.doc.nodes[note].frames.first().map_or(0.0, |f| f.1.x0),
            ..Ctx::default()
        };
        self.drop_note_number(note);
        let mut blocks = self.children(note, &ctx);
        if let Some(Block::Paragraph(p)) = blocks.first_mut() {
            p.content.insert(
                0,
                Inline::Run(Run {
                    props: RunProps {
                        vert: Some(VertAlign::Superscript),
                        ..RunProps::default()
                    },
                    content: vec![RunContent::FootnoteMark],
                }),
            );
            p.content.insert(
                1,
                Inline::Run(Run {
                    props: RunProps::default(),
                    content: vec![RunContent::Text(" ".into())],
                }),
            );
        }
        self.footnotes[i].body = Blocks::from_blocks(blocks);
        (self.prev, self.break_pending, self.pending) = saved;
        i
    }

    /// The entry's own number, which Word writes as the footnote's mark.
    fn drop_note_number(&mut self, note: NodeId) {
        let mut texts = self
            .doc
            .subtree(note)
            .into_iter()
            .flat_map(|n| self.doc.nodes[n].children.clone())
            .filter_map(|c| match c {
                Child::Item(r) => match &self.doc.item(r).kind {
                    ItemKind::Text(t) => Some((r, t.clone())),
                    _ => None,
                },
                Child::Node(_) => None,
            });
        if let (Some((r, a)), Some((_, b))) = (texts.next(), texts.next()) {
            if a.size < b.size && a.text.chars().count() <= 4 {
                self.consumed.insert(r);
                self.skip.insert(r);
            }
        }
    }

    fn list(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Block> {
        let (num, level) = match ctx.list {
            Some((num, level)) => (num, (level + 1).min(8)),
            None => {
                let id = self.numbering.abstracts.len() as u32;
                self.numbering.abstracts.push(AbstractNum {
                    id,
                    nsid: format!("{:08X}", 0x5153_0000 + id),
                    levels: Vec::new(),
                });
                let num = id + 1;
                self.numbering.nums.push(Num {
                    id: num,
                    abstract_id: id,
                    starts: Vec::new(),
                });
                (num, 0)
            }
        };
        let marker = match self.kind(n) {
            NodeKind::List { marker } => marker.clone(),
            _ => Marker::Bullet,
        };
        let (labels, bodies) = self.list_parts(n);
        let label_text = labels.first().map(|&l| self.text_of(l)).unwrap_or_default();
        let label_x = labels
            .first()
            .and_then(|&l| self.doc.nodes[l].frames.first())
            .map(|f| f.1.x0);
        let body_x = bodies
            .first()
            .and_then(|&b| self.doc.nodes[b].frames.first())
            .map(|f| f.1.x0);
        let (format, text, start) = level_of(&label_text, level, &marker);
        let left = body_x.map_or(18.0 * (level as f64 + 1.0), |x| x - ctx.left);
        let hanging = match (label_x, body_x) {
            (Some(l), Some(b)) => b - l,
            _ => 18.0,
        };
        let abs = self
            .numbering
            .nums
            .iter()
            .find(|m| m.id == num)
            .map(|m| m.abstract_id)
            .unwrap_or(0) as usize;
        let levels = &mut self.numbering.abstracts[abs].levels;
        while levels.len() <= level as usize {
            let i = levels.len() as u8;
            levels.push(Level {
                start: 1,
                format: NumFormat::Decimal,
                style: None,
                suffix: LevelSuffix::Tab,
                text: format!("%{}.", i + 1),
                align: Align::Start,
                indent: Indent {
                    left: Some(tw(18.0 * (i as f64 + 1.0))),
                    right: None,
                    first: Some(FirstLine::Hanging(tw(18.0))),
                },
                tabs: Vec::new(),
                run: RunProps::default(),
            });
        }
        levels[level as usize] = Level {
            start,
            format,
            style: None,
            suffix: LevelSuffix::Tab,
            text,
            align: Align::Start,
            indent: Indent {
                left: Some(tw(left)),
                right: None,
                first: Some(FirstLine::Hanging(tw(hanging.max(0.0)))),
            },
            tabs: vec![TabStop {
                kind: TabKind::Left,
                pos: tw(left),
                leader: TabLeader::None,
            }],
            run: RunProps::default(),
        };
        let inner = Ctx {
            list: Some((num, level)),
            ..ctx.clone()
        };
        let kids = self.doc.nodes[n].children.clone();
        let mut out = Vec::new();
        for c in &kids {
            out.extend(self.list_child(c, &inner, left));
        }
        out
    }

    fn list_child(&mut self, c: &Child, ctx: &Ctx, left: f64) -> Vec<Block> {
        let Child::Node(m) = c else {
            return self.child(c, ctx);
        };
        let m = *m;
        match self.kind(m) {
            NodeKind::ListBody if self.doc.nodes[m].lines.is_empty() => {
                let mut blocks = Vec::new();
                let kids = self.doc.nodes[m].children.clone();
                let mut numbered = false;
                for k in &kids {
                    let is_list = matches!(k, Child::Node(x) if matches!(self.kind(*x), NodeKind::List { .. }));
                    let inner = if numbered && !is_list {
                        Ctx {
                            list: None,
                            left: ctx.left + left,
                            ..ctx.clone()
                        }
                    } else {
                        ctx.clone()
                    };
                    let produced = self.child(k, &inner);
                    numbered |= produced.iter().any(|b| matches!(b, Block::Paragraph(_)));
                    blocks.extend(produced);
                }
                blocks
            }
            NodeKind::ListBody | NodeKind::ListLabel | NodeKind::List { .. } => self.node(m, ctx),
            _ => {
                let kids = self.doc.nodes[m].children.clone();
                if matches!(
                    self.kind(m),
                    NodeKind::Block { .. } | NodeKind::Align { .. }
                ) {
                    let mut out = Vec::new();
                    for k in &kids {
                        out.extend(self.list_child(k, ctx, left));
                    }
                    out
                } else {
                    self.node(m, ctx)
                }
            }
        }
    }

    /// The labels and bodies of `n`'s own items, not of a list nested in it.
    fn list_parts(&self, n: NodeId) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut labels = Vec::new();
        let mut bodies = Vec::new();
        let mut stack: Vec<NodeId> = vec![n];
        let mut order = Vec::new();
        while let Some(at) = stack.pop() {
            order.push(at);
            for c in self.doc.nodes[at].children.iter().rev() {
                if let Child::Node(m) = c {
                    match self.kind(*m) {
                        NodeKind::List { .. } => {}
                        NodeKind::ListLabel => labels.push(*m),
                        NodeKind::ListBody => bodies.push(*m),
                        _ => stack.push(*m),
                    }
                }
            }
        }
        labels.sort();
        bodies.sort();
        (labels, bodies)
    }

    fn text_of(&self, n: NodeId) -> String {
        self.doc
            .subtree(n)
            .into_iter()
            .flat_map(|m| self.doc.nodes[m].children.clone())
            .filter_map(|c| match c {
                Child::Item(r) => match &self.doc.item(r).kind {
                    ItemKind::Text(t) => Some(t.text.clone()),
                    _ => None,
                },
                Child::Node(_) => None,
            })
            .collect()
    }

    fn table(&mut self, n: NodeId, ctx: &Ctx) -> Table {
        let NodeKind::Table {
            columns,
            header_rows,
            ..
        } = self.kind(n).clone()
        else {
            unreachable!("a table node")
        };
        let mine = |l: &Self, m: NodeId| {
            l.doc.ancestor(l.doc.nodes[m].parent.unwrap_or(0), |k| {
                matches!(k, NodeKind::Table { .. })
            }) == Some(n)
        };
        let mut cells: Vec<NodeId> = Vec::new();
        let mut stroke: Option<(f64, Rgb)> = None;
        for m in self.doc.subtree(n) {
            if m != n && !mine(self, m) {
                continue;
            }
            if matches!(self.kind(m), NodeKind::Cell { .. }) {
                cells.push(m);
            }
            for c in self.doc.nodes[m].children.clone() {
                if let Child::Item(r) = c {
                    if let ItemKind::Shape(s) = &self.doc.item(r).kind {
                        if matches!(s.geometry, Geometry::Line { .. }) {
                            stroke = stroke.or(s.stroke);
                            self.consumed.insert(r);
                        }
                    }
                }
            }
        }
        let ncols = columns.len().saturating_sub(1).max(1);
        let grid: Vec<Twips> = if columns.len() >= 2 {
            columns
                .windows(2)
                .map(|w| tw((w[1] - w[0]).max(1.0)))
                .collect()
        } else {
            vec![tw(ctx.width.max(72.0))]
        };
        let nrows = cells
            .iter()
            .filter_map(|&c| match self.kind(c) {
                NodeKind::Cell { row, rowspan, .. } => Some(row + rowspan),
                _ => None,
            })
            .max()
            .unwrap_or(0) as usize;
        let inset = cells
            .iter()
            .find_map(|&c| {
                let col = match self.kind(c) {
                    NodeKind::Cell { col, .. } => *col as usize,
                    _ => return None,
                };
                let x = self
                    .doc
                    .subtree(c)
                    .into_iter()
                    .find_map(|m| self.doc.nodes[m].lines.first().map(|l| l.x0))?;
                columns.get(col).map(|c0| (x - c0).clamp(0.0, 20.0))
            })
            .unwrap_or(5.0);
        let saved = self.prev;
        let mut rows = Vec::new();
        for r in 0..nrows {
            let mut row_cells = Vec::new();
            let mut c = 0;
            while c < ncols {
                let here = cells.iter().copied().find(|&m| {
                    matches!(self.kind(m), NodeKind::Cell { col, row, .. } if *col as usize == c && *row as usize == r)
                });
                let covering = cells.iter().copied().find(|&m| {
                    matches!(self.kind(m), NodeKind::Cell { col, row, rowspan, .. }
                        if *col as usize == c && (*row as usize) < r && (*row + *rowspan) as usize > r)
                });
                let (span, body, merge) = match (here, covering) {
                    (Some(m), _) => {
                        let NodeKind::Cell {
                            colspan, rowspan, ..
                        } = *self.kind(m)
                        else {
                            unreachable!()
                        };
                        let inner = Ctx {
                            nested: true,
                            left: columns.get(c).copied().unwrap_or(ctx.left) + inset,
                            width: grid.iter().skip(c).take(colspan as usize).sum::<i32>() as f64
                                / 20.0,
                            align: None,
                            page_number: None,
                            list: None,
                        };
                        self.prev = None;
                        let blocks = self.children(m, &inner);
                        (
                            colspan,
                            Blocks::from_blocks(blocks),
                            (rowspan > 1).then_some(VMerge::Restart),
                        )
                    }
                    (None, Some(m)) => {
                        let colspan = match self.kind(m) {
                            NodeKind::Cell { colspan, .. } => *colspan,
                            _ => 1,
                        };
                        (colspan, Blocks::default(), Some(VMerge::Continue))
                    }
                    (None, None) => (1, Blocks::default(), None),
                };
                let span = span.max(1).min((ncols - c) as u32);
                row_cells.push(Cell {
                    props: CellProps {
                        width: Some(grid.iter().skip(c).take(span as usize).sum()),
                        span,
                        merge,
                        ..CellProps::default()
                    },
                    body,
                });
                c += span as usize;
            }
            rows.push(Row {
                props: RowProps {
                    cant_split: false,
                    header: (r as u32) < header_rows,
                    min_height: None,
                },
                cells: row_cells,
            });
        }
        self.prev = saved;
        let border = stroke.map(|(w, c)| Border {
            size: ((w * 8.0).round() as u32).clamp(2, 96),
            color: hex(c),
        });
        let frame = self.doc.nodes[n].frames.clone();
        let first_page = frame.first().map_or(0, |f| f.0);
        if let Some(&(p, f)) = frame.last() {
            self.prev = Some(Prev::Bottom(p, f.y1));
        }
        let _ = first_page;
        self.break_pending = false;
        let i = tw(inset);
        Table {
            props: TableProps {
                style: None,
                width: Some(grid.iter().sum()),
                indent: columns.first().map(|x| tw(x - ctx.left)),
                borders: border.map(|b| Borders {
                    top: Some(b.clone()),
                    left: Some(b.clone()),
                    bottom: Some(b.clone()),
                    right: Some(b.clone()),
                    inside_h: Some(b.clone()),
                    inside_v: Some(b),
                }),
                fixed: true,
                cell_margins: Some(CellMargins {
                    top: i,
                    left: i,
                    bottom: i,
                    right: i,
                }),
            },
            grid,
            rows,
        }
    }

    fn picture(
        &mut self,
        image: usize,
        rect: Rect,
        alt: Option<String>,
        placement: Placement,
    ) -> Drawing {
        let next = self.media.len();
        let index = *self.media_of.entry(image).or_insert(next);
        if index == next {
            let data = &self.doc.images[image];
            self.media.push(Media {
                kind: match data.format {
                    ImageFormat::Png => MediaKind::Png,
                    ImageFormat::Jpeg => MediaKind::Jpeg,
                },
                bytes: data.bytes.clone(),
            });
        }
        Drawing {
            placement,
            width: emu(rect.width().max(1.0)),
            height: emu(rect.height().max(1.0)),
            content: DrawingContent::Picture(index),
            descr: alt,
        }
    }

    /// `n`'s content as a page-anchored text box where its ink sits, then
    /// the boxes of the places inside it, since Word nests no text box in
    /// another.
    fn text_box(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Drawing> {
        let Some(&(page, rect)) = self.doc.nodes[n].frames.first() else {
            return Vec::new();
        };
        let first = self
            .doc
            .subtree(n)
            .into_iter()
            .find(|&m| !self.doc.nodes[m].lines.is_empty());
        let setup = self.doc.pages[page as usize].setup;
        let size = self.doc.pages[page as usize].size;
        let (col_left, col_width) = (
            setup.margins.left,
            size[0] - setup.margins.left - setup.margins.right,
        );
        let align = first
            .map(|m| {
                let style = match self.kind(m) {
                    NodeKind::Par { style, .. } | NodeKind::InlineRun { style } => *style,
                    _ => None,
                };
                let outer = self
                    .doc
                    .ancestor(m, |k| matches!(k, NodeKind::Align { .. }))
                    .and_then(|a| match self.kind(a) {
                        NodeKind::Align { h } => Some(*h),
                        _ => None,
                    });
                outer.or(style.map(|s| s.align)).unwrap_or_default()
            })
            .unwrap_or_default();
        let (x, width, wrap) = match align {
            HAlign::Start => (rect.x0, rect.width() + 2.0, false),
            _ => (col_left, col_width, true),
        };
        let saved = (
            self.prev,
            self.break_pending,
            std::mem::take(&mut self.pending),
        );
        self.prev = None;
        let inner = Ctx {
            nested: true,
            left: x,
            width,
            align: ctx.align,
            page_number: ctx.page_number.clone(),
            list: None,
        };
        let blocks = match self.kind(n) {
            NodeKind::Place { .. } | NodeKind::Artifact { .. } => self.children(n, &inner),
            _ => self.node(n, &inner),
        };
        let inner_boxes = std::mem::take(&mut self.pending);
        (self.prev, self.break_pending, self.pending) = saved;
        let has_content = blocks.iter().any(|b| match b {
            Block::Paragraph(p) => !p.content.is_empty(),
            _ => true,
        });
        if !has_content {
            return inner_boxes;
        }
        let lines: Vec<&crate::layout::Line> = self
            .doc
            .subtree(n)
            .into_iter()
            .filter(|&m| {
                self.doc.ancestor(m, |k| {
                    matches!(k, NodeKind::Place { .. } | NodeKind::Artifact { .. })
                }) == Some(n)
            })
            .flat_map(|m| self.doc.nodes[m].lines.iter())
            .filter(|l| l.page == page)
            .collect();
        let (top, height) = match (lines.first(), lines.last()) {
            (Some(a), Some(b)) => {
                let size = rect.height().min(14.0).max(6.0);
                let pitch = if lines.len() >= 2 {
                    (b.baseline - a.baseline) / (lines.len() - 1) as f64
                } else {
                    size * 1.2
                };
                let top = a.baseline - 0.8 * pitch;
                (top, (b.baseline - top + 0.4 * pitch).max(rect.y1 - top))
            }
            _ => (rect.y0, rect.height()),
        };
        self.z += 1;
        let mut out = vec![Drawing {
            placement: Placement::Anchor(Anchor {
                x: emu(x),
                y: emu(top),
                behind_text: matches!(
                    self.kind(n),
                    NodeKind::Artifact {
                        kind: ArtifactKind::Background
                    }
                ),
                z: self.z,
                wrap: Wrap::None,
            }),
            width: emu(width.max(1.0)),
            height: emu(height.max(1.0)),
            content: DrawingContent::TextBox(TextBox {
                body: Blocks::from_blocks(blocks),
                wrap,
            }),
            descr: None,
        }];
        out.extend(inner_boxes);
        out
    }

    /// The header and footer parts of the section of `pages`.
    fn chrome(
        &mut self,
        pages: &[usize],
        headers: &mut Vec<Blocks>,
        footers: &mut Vec<Blocks>,
    ) -> (HeaderRefs, HeaderRefs, bool) {
        let artifacts: Vec<(usize, NodeId, bool)> = self.doc.nodes[0]
            .children
            .iter()
            .filter_map(|c| match c {
                Child::Node(n) => match self.kind(*n) {
                    NodeKind::Artifact { kind } => {
                        let page = self.first_page(*n)? as usize;
                        pages.contains(&page).then_some((
                            page,
                            *n,
                            matches!(kind, ArtifactKind::Footer | ArtifactKind::PageNumber),
                        ))
                    }
                    _ => None,
                },
                Child::Item(_) => None,
            })
            .collect();
        let mut title_page = false;
        let mut refs = [HeaderRefs::default(), HeaderRefs::default()];
        for (part, footer) in [(0, false), (1, true)] {
            let of = |p: usize| -> Vec<NodeId> {
                artifacts
                    .iter()
                    .filter(|a| a.0 == p && a.2 == footer)
                    .map(|a| a.1)
                    .collect()
            };
            if pages.iter().all(|&p| of(p).is_empty()) {
                continue;
            }
            let sig = |l: &Self, p: usize| l.signature(&of(p), p);
            let first = pages[0];
            let second = pages.get(1).copied();
            let differs = second.is_some_and(|s| sig(self, first) != sig(self, s));
            let mut make = |l: &mut Self, p: usize| -> usize {
                let ctx = Ctx {
                    page_number: l.doc.pages[p].number_text.clone(),
                    ..Ctx::default()
                };
                let mut drawings = Vec::new();
                for n in of(p) {
                    drawings.extend(l.text_box(n, &ctx));
                }
                let target = sig(l, p);
                for &q in pages {
                    if q != p && sig(l, q) == target {
                        for n in of(q) {
                            l.consume_subtree(n);
                        }
                    }
                }
                let para = Paragraph {
                    props: ParProps {
                        spacing: Some(Spacing {
                            before: Some(0),
                            after: Some(0),
                            line: Some(Line::Exact(20)),
                        }),
                        ..ParProps::default()
                    },
                    content: vec![Inline::Run(Run {
                        props: RunProps::default(),
                        content: drawings.into_iter().map(RunContent::Drawing).collect(),
                    })],
                };
                let parts = if footer { &mut *footers } else { &mut *headers };
                parts.push(Blocks::from_paragraphs(vec![para]));
                parts.len() - 1
            };
            if differs {
                title_page = true;
                refs[part].first = Some(make(self, first));
                refs[part].default = Some(make(self, second.expect("a second page")));
            } else {
                refs[part].default = Some(make(self, first));
            }
        }
        if title_page {
            for r in &mut refs {
                if r.default.is_some() && r.first.is_none() {
                    r.first = r.default;
                }
            }
        }
        let [h, f] = refs;
        (h, f, title_page)
    }

    /// What a page's chrome shows, its number's text and x left out.
    fn signature(&self, nodes: &[NodeId], page: usize) -> Vec<(i64, String)> {
        let number = self.doc.pages[page].number_text.clone().unwrap_or_default();
        let mut out = Vec::new();
        for &n in nodes {
            for m in self.doc.subtree(n) {
                for c in &self.doc.nodes[m].children {
                    if let Child::Item(r) = c {
                        match &self.doc.item(*r).kind {
                            ItemKind::Text(t) if !number.is_empty() && t.text.trim() == number => {
                                out.push(((t.origin[1] * 10.0).round() as i64, "#".into()))
                            }
                            ItemKind::Text(t) => out.push((
                                (t.origin[1] * 10.0).round() as i64,
                                format!("{}@{}", t.text, (t.origin[0] * 10.0).round()),
                            )),
                            ItemKind::Image { rect, image, .. } => out.push((
                                (rect.y0 * 10.0).round() as i64,
                                format!("img{image}@{}", (rect.x0 * 10.0).round()),
                            )),
                            _ => {}
                        }
                    }
                }
            }
        }
        out
    }
}

/// Runs of consecutive pages with one page setup.
fn section_pages(doc: &LayoutDoc) -> Vec<Vec<usize>> {
    let key = |i: usize| {
        let p = &doc.pages[i];
        let m = p.setup.margins;
        (
            tw(p.size[0]),
            tw(p.size[1]),
            [tw(m.top), tw(m.right), tw(m.bottom), tw(m.left)],
            p.setup.columns,
            p.setup.flipped,
        )
    };
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..doc.pages.len() {
        match out.last_mut() {
            Some(g) if key(g[0]) == key(i) => g.push(i),
            _ => out.push(vec![i]),
        }
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

fn is_roman(s: &str, upper: bool) -> bool {
    let set = if upper { "IVXLCDM" } else { "ivxlcdm" };
    !s.is_empty() && s.chars().all(|c| set.contains(c))
}

fn roman_value(s: &str) -> u32 {
    let v = |c: char| match c.to_ascii_lowercase() {
        'i' => 1,
        'v' => 5,
        'x' => 10,
        'l' => 50,
        'c' => 100,
        'd' => 500,
        'm' => 1000,
        _ => 0,
    };
    let chars: Vec<u32> = s.chars().map(v).collect();
    let mut total = 0i64;
    for (i, &c) in chars.iter().enumerate() {
        if chars.get(i + 1).is_some_and(|&n| n > c) {
            total -= c as i64;
        } else {
            total += c as i64;
        }
    }
    total.max(1) as u32
}

/// The Word format of a displayed page number.
fn number_format(text: &str) -> Option<NumFormat> {
    let t = text.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        Some(NumFormat::Decimal)
    } else if is_roman(t, false) {
        Some(NumFormat::LowerRoman)
    } else if is_roman(t, true) {
        Some(NumFormat::UpperRoman)
    } else if t.chars().count() == 1 && t.chars().all(|c| c.is_ascii_lowercase()) {
        Some(NumFormat::LowerLetter)
    } else if t.chars().count() == 1 && t.chars().all(|c| c.is_ascii_uppercase()) {
        Some(NumFormat::UpperLetter)
    } else {
        None
    }
}

/// A list level's format, `lvlText` and start, from the first label Typst
/// printed at that level.
fn level_of(label: &str, level: u8, marker: &Marker) -> (NumFormat, String, u32) {
    let label = label.trim();
    if matches!(marker, Marker::Bullet) || !label.chars().any(char::is_alphanumeric) {
        let bullet = if label.is_empty() { "\u{2022}" } else { label };
        return (NumFormat::Bullet, bullet.to_string(), 1);
    }
    let start = label.find(char::is_alphanumeric).unwrap_or(0);
    let end = label
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(label.len(), |(i, c)| i + c.len_utf8());
    let (prefix, core, suffix) = (&label[..start], &label[start..end], &label[end..]);
    let (format, value) = if core.chars().all(|c| c.is_ascii_digit()) {
        (NumFormat::Decimal, core.parse().unwrap_or(1))
    } else if is_roman(core, false) && core != "c" && core != "d" && core != "l" && core != "m" {
        (NumFormat::LowerRoman, roman_value(core))
    } else if is_roman(core, true) && core != "C" && core != "D" && core != "L" && core != "M" {
        (NumFormat::UpperRoman, roman_value(core))
    } else if core.chars().count() == 1 && core.chars().all(|c| c.is_ascii_lowercase()) {
        (
            NumFormat::LowerLetter,
            core.as_bytes()[0] as u32 - b'a' as u32 + 1,
        )
    } else if core.chars().count() == 1 && core.chars().all(|c| c.is_ascii_uppercase()) {
        (
            NumFormat::UpperLetter,
            core.as_bytes()[0] as u32 - b'A' as u32 + 1,
        )
    } else {
        (NumFormat::Decimal, 1)
    };
    (format, format!("{prefix}%{}{suffix}", level + 1), value)
}

/// The Word face for a Typst family: a metric-compatible clone a Word
/// machine has, else the family itself.
fn word_font(family: &str) -> String {
    WORD_CLONES
        .iter()
        .find(|(f, _)| f.eq_ignore_ascii_case(family))
        .map_or_else(|| family.to_string(), |(_, w)| w.to_string())
}

/// Identical neighbouring runs as one, and neighbours under one link as one
/// hyperlink.
fn merge(items: Vec<(Option<String>, Inline)>) -> Vec<Inline> {
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

#[cfg(test)]
mod properties {
    use proptest::prelude::*;

    use super::{level_of, number_format, roman_value};
    use crate::layout::Marker;
    use crate::model::NumFormat;

    fn roman(mut n: u32) -> String {
        let table = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];
        let mut out = String::new();
        for (v, s) in table {
            while n >= v {
                out.push_str(s);
                n -= v;
            }
        }
        out
    }

    proptest! {
        #[test]
        fn a_numbered_label_keeps_its_affixes_and_start(
            n in 1u32..4000,
            prefix in "[(\\[]?",
            suffix in "[.):]?",
            level in 0u8..9,
        ) {
            let label = format!("{prefix}{n}{suffix}");
            let (format, text, start) = level_of(&label, level, &Marker::Numbered);
            prop_assert_eq!(format, NumFormat::Decimal);
            prop_assert_eq!(text, format!("{prefix}%{}{suffix}", level + 1));
            prop_assert_eq!(start, n);
        }

        #[test]
        fn a_roman_number_reads_back_as_its_value(n in 1u32..4000) {
            let r = roman(n);
            prop_assert_eq!(roman_value(&r), n);
            prop_assert_eq!(number_format(&r), Some(NumFormat::LowerRoman));
            prop_assert_eq!(number_format(&r.to_uppercase()), Some(NumFormat::UpperRoman));
        }

        #[test]
        fn a_decimal_page_number_is_decimal(n in 0u64..100000) {
            prop_assert_eq!(number_format(&n.to_string()), Some(NumFormat::Decimal));
        }
    }
}
