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
    ArtifactKind, BracketHint, Child, Geometry, HAlign, HintLevel, ImageFormat, ItemKind, ItemRef,
    LayoutDoc, LineEnd, Marker, NodeId, NodeKind, Notice, NumberFormat, ParStyle, PointHint, Rect,
    Rgb, VAlign,
};
use crate::model::{
    AbstractNum, Align, Anchor, Block, Blocks, Border, Borders, BreakKind, Cell, CellMargins,
    CellProps, Columns, Document, Drawing, DrawingContent, Field, FirstLine, FontDecl, FontFamily,
    Footnote, HeaderRefs, Hyperlink, Indent, Info, Inline, Level, LevelSuffix, Line, LinkTarget,
    Media, MediaKind, Num, NumFormat, NumRef, PageMargins, PageNumbers, ParProps, Paragraph,
    Placement, Row, RowProps, Run, RunContent, RunProps, Section, SectionProps, SectionStart,
    Settings, ShapeGeometry, Spacing, Stroke, Style, Styles, TabKind, TabLeader, TabStop, Table,
    TableProps, TextBox, Toc, Twips, VMerge, VertAlign, Wrap,
};

pub const CODE_FONT_NOT_EMBEDDED: &str = "docx::font_not_embedded";
pub const CODE_PLACE_ANCHORED: &str = "docx::place_anchored";
pub const CODE_SHAPE_DROPPED: &str = "docx::shape_dropped";
pub const CODE_INK_DROPPED: &str = "docx::ink_dropped";
pub const CODE_LABEL_MISMATCH: &str = "docx::label_mismatch";
pub const CODE_TOC_FROZEN: &str = "docx::toc_frozen";
pub const CODE_HINT_UNCHECKED: &str = "docx::hint_unchecked";
pub const CODE_UNSTRUCTURED_TEXT: &str = "docx::unstructured_text";

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
        unlisted: BTreeSet::new(),
        toc_levels: BTreeSet::new(),
        toc_prefixed: false,
        frozen: 0,
        label_prefix: None,
        strides: strides(doc),
        prev_bottom: None,
        table_gap: None,
        footer_floats: Vec::new(),
        skip: HashSet::new(),
        schemes: Vec::new(),
        counters: HashMap::new(),
        extra_styles: Vec::new(),
        roles: BTreeMap::new(),
        chrome_places: Vec::new(),
        alt_names: BTreeMap::new(),
        keep: false,
        mismatches: 0,
        untabbed: 0,
        unstructured: 0,
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
    /// The `list` hint around: its scheme, its `w:num`, and whether it is
    /// the scheme's first instance.
    hint_list: Option<(usize, u32, bool)>,
    /// Inside a table of contents entry at this level.
    toc: Option<u8>,
}

/// A numbering scheme a `list` hint names: its levels and its first `w:num`.
struct Scheme {
    name: String,
    abstract_id: u32,
    levels: Vec<HintLevel>,
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
    unlisted: BTreeSet<u8>,
    toc_levels: BTreeSet<u8>,
    /// An entry of the table of contents being lowered prints a page number
    /// that is no plain numeral.
    toc_prefixed: bool,
    frozen: usize,
    /// A printed list number the next paragraph opens with.
    label_prefix: Option<String>,
    /// The usual baseline stride of paragraphs of one face, size and leading.
    strides: HashMap<StrideKey, f64>,
    /// Where the last body paragraph's last line box ends.
    prev_bottom: Option<(u32, f64)>,
    /// The space a table just lowered needs above it, which the paragraph
    /// before it carries as space after.
    table_gap: Option<Twips>,
    /// Bottom floats of the first page, which its footer holds and reserves.
    footer_floats: Vec<NodeId>,
    /// Ink a paragraph leaves out: the label a live number replaces.
    skip: HashSet<ItemRef>,
    schemes: Vec<Scheme>,
    /// Per `w:num`, the count Word reaches at each level.
    counters: HashMap<u32, [u64; 9]>,
    /// The level styles of each scheme.
    extra_styles: Vec<Style>,
    /// Role style id to its name and the role after it.
    roles: BTreeMap<String, (String, Option<String>)>,
    /// Placed objects a `chrome` hint gives to a page: page, node, footer.
    chrome_places: Vec<(usize, NodeId, bool)>,
    /// A face's name for readers without it, from the quill's `font` hint.
    alt_names: BTreeMap<String, String>,
    /// A `keep` the paragraph before it has yet to take.
    keep: bool,
    mismatches: usize,
    untabbed: usize,
    unstructured: usize,
}

/// A role's or a scheme's level's style id: its name without spaces or
/// punctuation.
fn style_id(name: &str) -> String {
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if id.is_empty() { "Role".into() } else { id }
}

/// A child a paragraph's lines read into: an inline element, or a bracket
/// the reader found the paragraph reads through.
fn read_into(k: &NodeKind) -> bool {
    k.is_inline()
        || matches!(
            k,
            NodeKind::Block { .. } | NodeKind::Align { .. } | NodeKind::Hint(_)
        )
}

type StrideKey = (usize, i64, i64);

/// The style a paragraph-forming node's probes resolved: its own, else its
/// nearest block's.
fn style_of(doc: &LayoutDoc, n: NodeId) -> Option<ParStyle> {
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

/// The face, size and leading a paragraph's first text is set in.
fn stride_key(doc: &LayoutDoc, n: NodeId) -> Option<StrideKey> {
    let t = doc.nodes[n]
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .find_map(|r| match &doc.item(*r).kind {
            ItemKind::Text(t) => Some(t),
            _ => None,
        })?;
    let leading = style_of(doc, n).map_or(0.0, |s| s.leading);
    Some((
        t.font,
        (t.size * 100.0).round() as i64,
        (leading * 100.0).round() as i64,
    ))
}

/// The median baseline stride per face, size and leading, over the
/// paragraphs with two or more lines on one page.
fn strides(doc: &LayoutDoc) -> HashMap<StrideKey, f64> {
    let mut all: HashMap<StrideKey, Vec<f64>> = HashMap::new();
    for n in 0..doc.nodes.len() {
        if !doc.nodes[n].kind.forms_paragraph() {
            continue;
        }
        let Some(key) = stride_key(doc, n) else {
            continue;
        };
        for w in doc.nodes[n].lines.windows(2) {
            if w[0].page == w[1].page {
                all.entry(key)
                    .or_default()
                    .push(w[1].baseline - w[0].baseline);
            }
        }
    }
    all.into_iter()
        .map(|(k, mut v)| {
            v.sort_by(f64::total_cmp);
            (k, v[v.len() / 2])
        })
        .collect()
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
        let mut root_late = false;
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
            self.settle(&mut bodies[current], &mut root_late);
            bodies[current].extend(blocks);
        }
        if !self.pending.is_empty() {
            let last = bodies.last_mut().expect("a section");
            last.push(Block::Paragraph(Paragraph::default()));
            self.attach_pending(last);
        }
        let roles = self.role_styles(&mut bodies);
        self.extra_styles.extend(roles);
        for f in self.fonts.values_mut() {
            if let Some(alt) = self.alt_names.get(&f.name) {
                f.alt_name = Some(alt.clone());
            }
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
                        header: tw((s.margins.top - s.header_ascent - 1.0).max(0.0)),
                        footer: tw((s.margins.bottom - s.footer_descent - 1.0).max(0.0)),
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

    /// One style per role holding what every paragraph of the role shares,
    /// over two or more paragraphs and leaving out the first one's space
    /// before and every keep; what a style holds leaves the paragraphs.
    fn role_styles(&self, bodies: &mut [Vec<Block>]) -> Vec<Style> {
        let mut out = Vec::new();
        for (id, (name, next)) in &self.roles {
            let mut pars: Vec<&mut ParProps> = bodies
                .iter_mut()
                .flatten()
                .filter_map(|b| match b {
                    Block::Paragraph(p) if p.props.style.as_deref() == Some(id) => {
                        Some(&mut p.props)
                    }
                    _ => None,
                })
                .collect();
            let mut par = ParProps::default();
            if pars.len() >= 2 {
                fn shared<T: Clone + PartialEq>(v: &[T]) -> Option<T> {
                    v.first().filter(|f| v.iter().all(|x| x == *f)).cloned()
                }
                let align: Vec<_> = pars.iter().map(|p| p.align).collect();
                if let Some(a) = shared(&align).flatten() {
                    par.align = Some(a);
                    pars.iter_mut().for_each(|p| p.align = None);
                }
                let indent: Vec<_> = pars.iter().map(|p| p.indent.clone()).collect();
                if let Some(i) = shared(&indent).flatten() {
                    par.indent = Some(i);
                    pars.iter_mut().for_each(|p| p.indent = None);
                }
                let tabs: Vec<_> = pars.iter().map(|p| p.tabs.clone()).collect();
                if let Some(t) = shared(&tabs).filter(|t| !t.is_empty()) {
                    par.tabs = t;
                    pars.iter_mut().for_each(|p| p.tabs.clear());
                }
                let line: Vec<_> = pars
                    .iter()
                    .map(|p| p.spacing.as_ref().and_then(|s| s.line))
                    .collect();
                let before: Vec<_> = pars[1..]
                    .iter()
                    .map(|p| p.spacing.as_ref().and_then(|s| s.before))
                    .collect();
                let line = shared(&line).flatten();
                let before = (before.len() >= 2)
                    .then(|| shared(&before).flatten())
                    .flatten();
                if line.is_some() || before.is_some() {
                    par.spacing = Some(Spacing {
                        before,
                        after: None,
                        line,
                    });
                    for (i, p) in pars.iter_mut().enumerate() {
                        if let Some(sp) = p.spacing.as_mut() {
                            if line.is_some() {
                                sp.line = None;
                            }
                            if before.is_some() && (i > 0 || sp.before == before) {
                                sp.before = None;
                            }
                        }
                    }
                }
            }
            out.push(Style {
                based_on: Some("Normal".into()),
                next: Some(
                    next.as_deref()
                        .map(style_id)
                        .unwrap_or_else(|| "Normal".into()),
                ),
                quick: true,
                par,
                ..Style::paragraph(id.clone(), name.clone(), false)
            });
        }
        let unused: BTreeMap<String, &String> = self
            .roles
            .values()
            .filter_map(|(_, next)| next.as_ref())
            .map(|n| (style_id(n), n))
            .filter(|(id, _)| !self.roles.contains_key(id))
            .collect();
        for (id, name) in unused {
            out.push(Style {
                based_on: Some("Normal".into()),
                next: Some(id.clone()),
                quick: true,
                ..Style::paragraph(id, name.clone(), false)
            });
        }
        out
    }

    fn notices(&self) -> Vec<Notice> {
        let mut out = Vec::new();
        if self.frozen > 0 {
            out.push(Notice {
                code: CODE_TOC_FROZEN.into(),
                message: format!(
                    "{} table(s) of contents print page numbers Word's PAGE field cannot, so \
                     they are locked against an update",
                    self.frozen
                ),
            });
        }
        if self.mismatches > 0 {
            out.push(Notice {
                code: CODE_LABEL_MISMATCH.into(),
                message: format!(
                    "{} paragraph number(s) are not the number Word would count, so they stay as \
                     printed text",
                    self.mismatches
                ),
            });
        }
        if self.unstructured > 0 {
            out.push(Notice {
                code: CODE_UNSTRUCTURED_TEXT.into(),
                message: format!(
                    "{} line(s) of text that no element tags are set as a paragraph each, as \
                     printed",
                    self.unstructured
                ),
            });
        }
        if self.untabbed > 0 {
            out.push(Notice {
                code: CODE_HINT_UNCHECKED.into(),
                message: format!(
                    "{} grid(s) marked as tabbed lines wrap a cell before the last or hold a \
                     table, so they stay tables",
                    self.untabbed
                ),
            });
        }
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
                    ItemKind::Text(_) | ItemKind::Image { .. } | ItemKind::Raster { .. } => {
                        ink += 1
                    }
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
        styles.extend(self.extra_styles.iter().cloned());
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
        for &level in &self.unlisted {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                quick: true,
                par: ParProps {
                    keep_next: true,
                    ..ParProps::default()
                },
                ..Style::paragraph(
                    format!("UnlistedHeading{level}"),
                    format!("Unlisted Heading {level}"),
                    false,
                )
            });
        }
        for &level in &self.toc_levels {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                ui_priority: Some(39),
                ..Style::paragraph(format!("TOC{level}"), format!("toc {level}"), true)
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
        let mut late = false;
        for c in &kids {
            let blocks = self.child(c, ctx);
            self.settle(&mut out, &mut late);
            out.extend(blocks);
        }
        out
    }

    /// What the child just lowered asks of the paragraph before it: the
    /// space a table needs above it, as that paragraph's space after. When
    /// no paragraph comes before it at this level, the request is `late`
    /// and belongs to the level above.
    fn settle(&mut self, before: &mut [Block], late: &mut bool) {
        if *late || (self.table_gap.is_none() && !self.keep) {
            return;
        }
        if self.keep {
            match before.last_mut() {
                Some(Block::Paragraph(p)) => {
                    p.props.keep_next = true;
                    self.keep = false;
                }
                Some(Block::Table(t)) => {
                    for row in &mut t.rows {
                        row.props.cant_split = true;
                    }
                    self.keep = false;
                }
                Some(_) => self.keep = false,
                None => {
                    *late = true;
                    return;
                }
            }
        }
        if self.table_gap.is_none() {
            return;
        }
        match before.last_mut() {
            Some(Block::Paragraph(p)) => {
                let gap = self.table_gap.take();
                p.props.spacing.get_or_insert_with(Spacing::default).after = gap;
            }
            Some(_) => self.table_gap = None,
            None => *late = true,
        }
    }

    fn loose_item(&mut self, r: ItemRef, ctx: &Ctx) -> Vec<Block> {
        match &self.doc.item(r).kind {
            ItemKind::Mark(PointHint::Keep) => {
                self.consumed.insert(r);
                self.keep = true;
                Vec::new()
            }
            ItemKind::Mark(PointHint::Font { family, alt }) => {
                self.consumed.insert(r);
                self.alt_names.insert(family.clone(), alt.clone());
                Vec::new()
            }
            ItemKind::Mark(PointHint::Break { .. }) => {
                self.consumed.insert(r);
                if !ctx.nested {
                    self.break_pending = true;
                }
                Vec::new()
            }
            ItemKind::Shape(shape) => {
                self.consumed.insert(r);
                let decoration = matches!(
                    self.kind(self.doc.item(r).node),
                    NodeKind::Underline | NodeKind::Strike | NodeKind::Highlight { .. }
                );
                if !decoration {
                    let drawing = self.shape(shape);
                    self.pending.push(drawing);
                }
                Vec::new()
            }
            ItemKind::Image { .. } | ItemKind::Raster { .. } => {
                self.consumed.insert(r);
                let (image, rect, alt) = match &self.doc.item(r).kind {
                    ItemKind::Image { image, rect, alt } => (*image, *rect, alt.clone()),
                    ItemKind::Raster { image, rect } => (*image, *rect, None),
                    _ => unreachable!(),
                };
                let rect = &rect;
                let drawing = self.picture(image, *rect, alt, Placement::Inline);
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
        if kind == NodeKind::Unstructured && !self.doc.nodes[n].lines.is_empty() {
            let lines = self.doc.nodes[n].lines.clone();
            self.unstructured += lines.len();
            return lines
                .into_iter()
                .filter_map(|line| {
                    let line = crate::layout::Line {
                        end: LineEnd::Last,
                        ..line
                    };
                    self.paragraph_of(n, vec![line], ctx)
                })
                .map(Block::Paragraph)
                .collect();
        }
        if kind.forms_paragraph() && !self.doc.nodes[n].lines.is_empty() {
            return self
                .paragraph(n, ctx)
                .into_iter()
                .map(Block::Paragraph)
                .collect();
        }
        match kind {
            NodeKind::Hint(BracketHint::List { name, levels }) => {
                let list = self.scheme(&name, &levels);
                let ctx = Ctx {
                    hint_list: Some(list),
                    ..ctx.clone()
                };
                self.children(n, &ctx)
            }
            NodeKind::Table { .. } if self.tabbed_grid(n) => match self.tabbed(n, ctx) {
                Some(blocks) => blocks,
                None => {
                    self.untabbed += 1;
                    vec![Block::Table(self.table(n, ctx))]
                }
            },
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
            NodeKind::Place { float, valign } => {
                let chrome = self
                    .doc
                    .ancestor(n, |k| matches!(k, NodeKind::Hint(BracketHint::Chrome)))
                    .is_some();
                if let (true, Some(v), Some(page)) = (chrome, valign, self.first_page(n)) {
                    self.chrome_places
                        .push((page as usize, n, v == VAlign::Bottom));
                    return Vec::new();
                }
                if float && valign == Some(VAlign::Bottom) && self.first_page(n) == Some(0) {
                    self.footer_floats.push(n);
                    return Vec::new();
                }
                let mut drawings = self.text_box(n, ctx);
                if float {
                    if let Some(Placement::Anchor(a)) =
                        drawings.first_mut().map(|d| &mut d.placement)
                    {
                        a.wrap = Wrap::TopAndBottom;
                    }
                } else {
                    self.placed += 1;
                }
                self.pending.extend(drawings);
                Vec::new()
            }
            NodeKind::ListLabel => {
                self.consume_subtree(n);
                Vec::new()
            }
            NodeKind::Toc { depth } => self.toc(n, depth, ctx),
            NodeKind::TocEntry { level } => {
                let ctx = Ctx {
                    toc: Some(level),
                    ..ctx.clone()
                };
                self.children(n, &ctx)
            }
            _ => self.children(n, ctx),
        }
    }

    /// A table of contents: its title paragraphs, then one field whose
    /// stored result is its entries.
    fn toc(&mut self, n: NodeId, depth: Option<u8>, ctx: &Ctx) -> Vec<Block> {
        let mut before = Vec::new();
        let mut entries = Vec::new();
        let mut after = Vec::new();
        self.toc_prefixed = false;
        for c in self.doc.nodes[n].children.clone() {
            let entry =
                matches!(c, Child::Node(m) if matches!(self.kind(m), NodeKind::TocEntry { .. }));
            let blocks = self.child(&c, ctx);
            if entry {
                entries.extend(blocks.into_iter().filter_map(|b| match b {
                    Block::Paragraph(p) => Some(p),
                    _ => None,
                }));
            } else if entries.is_empty() {
                before.extend(blocks);
            } else {
                after.extend(blocks);
            }
        }
        if entries.is_empty() {
            before.extend(after);
            return before;
        }
        let locked = self.toc_prefixed;
        self.frozen += locked as usize;
        before.push(Block::Toc(Toc {
            instruction: format!(
                "TOC \\o \"1-{}\" \\h \\z \\u",
                depth.unwrap_or(9).clamp(1, 9)
            ),
            locked,
            entries,
        }));
        before.extend(after);
        before
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
        let lines = self.doc.nodes[n].lines.clone();
        self.paragraph_of(n, lines, ctx)
    }

    /// One Word paragraph of some of `n`'s lines: all of them, or one line
    /// of text no element tags. The objects and number the node holds go
    /// with its first line.
    fn paragraph_of(
        &mut self,
        n: NodeId,
        lines: Vec<crate::layout::Line>,
        ctx: &Ctx,
    ) -> Option<Paragraph> {
        let node = &self.doc.nodes[n];
        let first = lines.first()?;
        let opens = node.lines.first().is_some_and(|l| l.items == first.items);
        let first_x = first.x0;
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
        let pitch = self.pitch(n, &lines, style);
        let tall = lines.iter().flat_map(|l| &l.items).any(|r| {
            matches!(&self.doc.item(*r).kind, ItemKind::Raster { rect, .. } if rect.height() > pitch)
        });
        let mut props = self.par_frame(ctx, style, first.page, first.baseline, pitch, false);
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
        if opens {
            self.inline_shapes(n);
            for place in self.inline_places(n) {
                let drawings = self.text_box(place, ctx);
                self.placed += 1;
                self.pending.extend(drawings);
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
                    self.mismatches += 1;
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
        let mut content = self.runs(n, &lines, ctx);
        if let Some(label) = self.label_prefix.take() {
            content.insert(
                0,
                Inline::Run(Run {
                    props: RunProps::default(),
                    content: vec![RunContent::Text(label), RunContent::Tab],
                }),
            );
        }
        let mut para = Paragraph { props, content };
        self.apply_break(&mut para, first.page, ctx);
        let last = lines.last().expect("a line");
        self.prev = Some(Prev::Baseline(last.page, last.baseline));
        if !ctx.nested {
            self.prev_bottom = Some((last.page, last.baseline + 0.2 * pitch));
        }
        Some(para)
    }

    /// The `w:num` for one instance of the scheme a `list` hint names: the
    /// scheme's own on its first instance, else a new one restarting at 1.
    fn scheme(&mut self, name: &str, levels: &[HintLevel]) -> (usize, u32, bool) {
        let num = self.numbering.nums.len() as u32 + 1;
        if let Some(i) = self.schemes.iter().position(|s| s.name == name) {
            let abstract_id = self.schemes[i].abstract_id;
            self.numbering.nums.push(Num {
                id: num,
                abstract_id,
                starts: vec![(0, 1)],
            });
            return (i, num, false);
        }
        let abstract_id = self.numbering.abstracts.len() as u32;
        let levels: Vec<HintLevel> = (0..9)
            .map(|i| {
                levels
                    .get(i)
                    .or(levels.last())
                    .cloned()
                    .unwrap_or(HintLevel {
                        format: NumberFormat::Decimal,
                        prefix: String::new(),
                        suffix: ".".into(),
                        underline: false,
                        visible: true,
                        indent: 0.0,
                    })
            })
            .collect();
        let id = style_id(name);
        let mut word = Vec::new();
        for (i, l) in levels.iter().enumerate() {
            let indent = Indent {
                left: Some(0),
                right: None,
                first: Some(if l.indent >= 0.0 {
                    FirstLine::Indent(tw(l.indent))
                } else {
                    FirstLine::Hanging(tw(-l.indent))
                }),
            };
            let style = format!("{id}{}", i + 1);
            word.push(Level {
                start: 1,
                format: if l.visible {
                    match l.format {
                        NumberFormat::Decimal => NumFormat::Decimal,
                        NumberFormat::LowerLetter => NumFormat::LowerLetter,
                        NumberFormat::UpperLetter => NumFormat::UpperLetter,
                        NumberFormat::LowerRoman => NumFormat::LowerRoman,
                        NumberFormat::UpperRoman => NumFormat::UpperRoman,
                    }
                } else {
                    NumFormat::None
                },
                style: Some(style.clone()),
                suffix: LevelSuffix::Nothing,
                text: if l.visible {
                    format!("{}%{}{}", l.prefix, i + 1, l.suffix)
                } else {
                    String::new()
                },
                align: Align::Start,
                indent: indent.clone(),
                tabs: Vec::new(),
                run: RunProps {
                    underline: l.underline,
                    ..RunProps::default()
                },
            });
            self.extra_styles.push(Style {
                based_on: Some("Normal".into()),
                quick: i < 4,
                par: ParProps {
                    numbering: Some(NumRef {
                        level: i as u8,
                        num,
                    }),
                    indent: Some(indent),
                    ..ParProps::default()
                },
                ..Style::paragraph(style, format!("{name} {}", i + 1), false)
            });
        }
        self.numbering.abstracts.push(AbstractNum {
            id: abstract_id,
            nsid: format!("{:08X}", 0x5153_0000 + abstract_id),
            levels: word,
        });
        self.numbering.nums.push(Num {
            id: num,
            abstract_id,
            starts: vec![(0, 1)],
        });
        self.schemes.push(Scheme {
            name: name.to_string(),
            abstract_id,
            levels,
        });
        (self.schemes.len() - 1, num, true)
    }

    /// The first `label` hint among `n`'s inline content.
    fn label_in(&self, n: NodeId) -> Option<NodeId> {
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
    fn role_of(&self, n: NodeId) -> Option<(String, Option<String>)> {
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

    /// Whether a `layout(form: "tabbed")` hint holds the grid `n`, through
    /// probe brackets only.
    fn tabbed_grid(&self, n: NodeId) -> bool {
        let mut at = self.doc.nodes[n].parent;
        while let Some(m) = at {
            match self.kind(m) {
                NodeKind::Hint(BracketHint::Tabbed) => return true,
                NodeKind::Block { .. } | NodeKind::Align { .. } => at = self.doc.nodes[m].parent,
                _ => return false,
            }
        }
        false
    }

    /// The paragraph-forming nodes of `cell`, in reading order.
    fn cell_paragraphs(&self, cell: NodeId) -> Vec<NodeId> {
        self.doc
            .subtree(cell)
            .into_iter()
            .filter(|&m| self.kind(m).forms_paragraph() && !self.doc.nodes[m].lines.is_empty())
            .collect()
    }

    /// A tabbed grid as one paragraph per row: the cells' text joined by a
    /// tab each, a tab stop at each column, and the last column's text
    /// hanging at its x; `None` when a cell before the last wraps or the
    /// last holds a table.
    fn tabbed(&mut self, n: NodeId, ctx: &Ctx) -> Option<Vec<Block>> {
        let NodeKind::Table { columns, .. } = self.kind(n).clone() else {
            return None;
        };
        let mut rows: BTreeMap<u32, Vec<(u32, NodeId)>> = BTreeMap::new();
        for m in self.doc.subtree(n) {
            if let NodeKind::Cell { col, row, .. } = *self.kind(m) {
                if self
                    .doc
                    .ancestor(self.doc.nodes[m].parent.unwrap_or(0), |k| {
                        matches!(k, NodeKind::Table { .. })
                    })
                    == Some(n)
                {
                    rows.entry(row).or_default().push((col, m));
                }
            }
        }
        for cells in rows.values_mut() {
            cells.sort();
            let (_, last) = *cells.last()?;
            for &(_, c) in &cells[..cells.len() - 1] {
                let lines: usize = self
                    .cell_paragraphs(c)
                    .iter()
                    .map(|&p| self.doc.nodes[p].lines.len())
                    .sum();
                if lines > 1 {
                    return None;
                }
            }
            if self
                .doc
                .subtree(last)
                .iter()
                .any(|&m| m != last && matches!(self.kind(m), NodeKind::Table { .. }))
            {
                return None;
            }
        }
        for m in self.doc.subtree(n) {
            for c in self.doc.nodes[m].children.clone() {
                if let Child::Item(r) = c {
                    if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) {
                        self.consumed.insert(r);
                    }
                }
            }
        }
        let role = self.role_of(n);
        let mut out = Vec::new();
        for cells in rows.values() {
            let (_, last) = *cells.last().expect("a cell");
            let pars: Vec<NodeId> = cells
                .iter()
                .flat_map(|&(_, c)| self.cell_paragraphs(c))
                .collect();
            let Some(&lead) = self.cell_paragraphs(last).first().or(pars.first()) else {
                continue;
            };
            let first = pars
                .iter()
                .filter_map(|&p| self.doc.nodes[p].lines.first())
                .min_by(|a, b| {
                    (a.page, a.baseline)
                        .partial_cmp(&(b.page, b.baseline))
                        .unwrap()
                })?
                .clone();
            let style = style_of(self.doc, lead);
            let lead_lines = self.doc.nodes[lead].lines.clone();
            let pitch = self.pitch(lead, &lead_lines, style);
            let mut props = self.par_frame(ctx, style, first.page, first.baseline, pitch, false);
            let x = |l: &Self, c: NodeId, col: u32| {
                let inset = match l.kind(c) {
                    NodeKind::Cell { inset, .. } => inset.left,
                    _ => 0.0,
                };
                columns.get(col as usize).copied().unwrap_or(ctx.left) + inset - ctx.left
            };
            let end = style.is_some_and(|s| s.align == HAlign::End)
                || self
                    .doc
                    .ancestor(lead, |k| matches!(k, NodeKind::Align { h: HAlign::End }))
                    .is_some_and(|a| {
                        self.doc.ancestor(a, |k| matches!(k, NodeKind::Cell { .. })) == Some(last)
                    });
            let mut content = Vec::new();
            let mut tabs = Vec::new();
            for (i, &(col, c)) in cells.iter().enumerate() {
                if i > 0 {
                    let pos = ((x(self, c, col) * 20.0).ceil()) as Twips;
                    let last_cell = i + 1 == cells.len();
                    tabs.push(if last_cell && end {
                        TabStop {
                            kind: TabKind::Right,
                            pos: tw(columns.last().copied().unwrap_or(0.0) - ctx.left),
                            leader: TabLeader::None,
                        }
                    } else {
                        TabStop {
                            kind: TabKind::Left,
                            pos,
                            leader: TabLeader::None,
                        }
                    });
                    content.push(Inline::Run(Run {
                        props: RunProps::default(),
                        content: vec![RunContent::Tab],
                    }));
                }
                for (j, p) in self.cell_paragraphs(c).into_iter().enumerate() {
                    if j > 0 {
                        content.push(Inline::Run(Run {
                            props: RunProps::default(),
                            content: vec![RunContent::Break(BreakKind::Line)],
                        }));
                    }
                    let lines = self.doc.nodes[p].lines.clone();
                    content.extend(self.runs(p, &lines, ctx));
                }
            }
            if !end {
                if let Some(&(col, c)) = cells.last().filter(|_| cells.len() > 1) {
                    let hang = x(self, c, col);
                    props.indent = Some(Indent {
                        left: Some(tw(hang)),
                        right: None,
                        first: Some(FirstLine::Hanging(tw(hang))),
                    });
                }
            }
            props.tabs = tabs;
            if let Some((name, next)) = role.clone() {
                let id = style_id(&name);
                self.roles.entry(id.clone()).or_insert((name, next));
                props.style = Some(id);
            }
            let mut para = Paragraph { props, content };
            self.apply_break(&mut para, first.page, ctx);
            let last_line = self
                .cell_paragraphs(last)
                .last()
                .and_then(|&p| self.doc.nodes[p].lines.last().cloned())
                .unwrap_or(first);
            self.prev = Some(Prev::Baseline(last_line.page, last_line.baseline));
            if !ctx.nested {
                self.prev_bottom = Some((last_line.page, last_line.baseline + 0.2 * pitch));
            }
            out.push(Block::Paragraph(para));
        }
        Some(out)
    }

    /// A paragraph's exact line height: its own median stride, else the
    /// stride of paragraphs of its face, size and leading, else its style's
    /// leading plus its line box.
    fn pitch(&self, n: NodeId, lines: &[crate::layout::Line], style: Option<ParStyle>) -> f64 {
        let mut strides: Vec<f64> = lines
            .windows(2)
            .filter(|w| w[0].page == w[1].page)
            .map(|w| w[1].baseline - w[0].baseline)
            .collect();
        if !strides.is_empty() {
            strides.sort_by(f64::total_cmp);
            return strides[strides.len() / 2];
        }
        let size = lines
            .iter()
            .flat_map(|l| &l.items)
            .find_map(|r| match &self.doc.item(*r).kind {
                ItemKind::Text(t) => Some(t.size),
                _ => None,
            })
            .unwrap_or(11.0);
        stride_key(self.doc, n)
            .and_then(|k| self.strides.get(&k).copied())
            .or_else(|| {
                style
                    .filter(|s| s.top_edge + s.bottom_edge > 0.0)
                    .map(|s| s.leading + s.top_edge + s.bottom_edge)
            })
            .unwrap_or(1.2 * size)
    }

    /// The places inside `n`'s inline content, which a paragraph cannot hold.
    fn inline_places(&self, n: NodeId) -> Vec<NodeId> {
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

    /// The alignment of the Align bracket a paragraph reads all its content
    /// through.
    fn read_align(&self, n: NodeId) -> Option<HAlign> {
        let mut at = n;
        loop {
            let mut nodes = self.doc.nodes[at].children.iter().filter(|c| match c {
                Child::Node(_) => true,
                Child::Item(r) => matches!(self.doc.item(*r).kind, ItemKind::Text(_)),
            });
            let (Some(Child::Node(m)), None) = (nodes.next(), nodes.next()) else {
                return None;
            };
            match self.kind(*m) {
                NodeKind::Align { h } => return Some(*h),
                NodeKind::Block { .. } | NodeKind::Hint(_) => at = *m,
                _ => return None,
            }
        }
    }

    /// The runs of `n`'s lines, joined into running text.
    fn runs(&mut self, n: NodeId, lines: &[crate::layout::Line], ctx: &Ctx) -> Vec<Inline> {
        let mut out: Vec<(Option<String>, Inline)> = Vec::new();
        let mut tabbed = false;
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
                        let fill = chain.iter().any(|k| matches!(k, NodeKind::TocFill));
                        let page = chain.iter().any(|k| matches!(k, NodeKind::TocPage));
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
                        if page {
                            let s = t.text.trim();
                            let numeral = s.chars().all(|c| c.is_ascii_digit())
                                || is_roman(s, false)
                                || is_roman(s, true);
                            self.toc_prefixed |= !numeral;
                        }
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
                    ItemKind::Raster { image, rect } => {
                        let drawing = self.picture(*image, *rect, None, Placement::Inline);
                        out.push((
                            None,
                            Inline::Run(Run {
                                props: RunProps::default(),
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
        let page = self.first_page(note).unwrap_or(0) as usize;
        let left = self
            .doc
            .pages
            .get(page)
            .map_or(0.0, |p| p.setup.margins.left);
        let ctx = Ctx {
            nested: true,
            left,
            ..Ctx::default()
        };
        let number = self.doc.nodes[note].children.iter().find_map(|c| match c {
            Child::Node(m) if matches!(self.kind(*m), NodeKind::ListLabel) => Some(*m),
            _ => None,
        });
        let indent = number
            .and_then(|m| self.doc.nodes[m].frames.first())
            .map(|f| f.1.x0 - left);
        let mut blocks = self.children(note, &ctx);
        if let Some(Block::Paragraph(p)) = blocks.first_mut() {
            if let Some(x) = indent.filter(|x| x.abs() >= 0.25) {
                p.props.indent = Some(Indent {
                    left: None,
                    right: None,
                    first: Some(FirstLine::Indent(tw(x))),
                });
            }
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
                let num = self.numbering.nums.len() as u32 + 1;
                self.numbering.nums.push(Num {
                    id: num,
                    abstract_id: id,
                    starts: Vec::new(),
                });
                (num, 0)
            }
        };
        let (marker, label_end) = match self.kind(n) {
            NodeKind::List { marker, label_end } => (marker.clone(), *label_end),
            _ => (Marker::Printed, false),
        };
        let (labels, bodies) = self.list_parts(n);
        let label_frame = labels
            .first()
            .and_then(|&l| self.doc.nodes[l].frames.first())
            .map(|f| f.1);
        let body_x = bodies
            .first()
            .and_then(|&b| self.doc.nodes[b].frames.first())
            .map(|f| f.1.x0);
        if marker == Marker::Printed {
            let kids = self.doc.nodes[n].children.clone();
            let inner = Ctx {
                list: None,
                ..ctx.clone()
            };
            let mut out = Vec::new();
            for c in &kids {
                out.extend(self.printed_child(c, &inner));
            }
            return out;
        }
        let (format, text, start) = level_of(&marker, level);
        let left = body_x.map_or(18.0 * (level as f64 + 1.0), |x| x - ctx.left);
        let label_at = label_frame.map(|f| if label_end { f.x1 } else { f.x0 } - ctx.left);
        let hanging = label_at.map_or(18.0, |l| left - l);
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
            align: if label_end { Align::End } else { Align::Start },
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

    /// A child of a list whose numbers stay as printed: each label's text
    /// opens the paragraph of the body after it, with a tab between.
    fn printed_child(&mut self, c: &Child, ctx: &Ctx) -> Vec<Block> {
        let Child::Node(m) = c else {
            return self.child(c, ctx);
        };
        match self.kind(*m) {
            NodeKind::ListLabel => {
                self.consume_subtree(*m);
                let text = self.text_of(*m);
                self.label_prefix = Some(text.trim().to_string());
                Vec::new()
            }
            NodeKind::Block { .. } | NodeKind::Align { .. } => {
                let kids = self.doc.nodes[*m].children.clone();
                let mut out = Vec::new();
                for k in &kids {
                    out.extend(self.printed_child(k, ctx));
                }
                out
            }
            _ => self.child(c, ctx),
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
            content_sized,
            rows: tops,
            header_rows,
            ..
        } = self.kind(n).clone()
        else {
            unreachable!("a table node")
        };
        let mut cells: Vec<NodeId> = Vec::new();
        for m in self.doc.subtree(n) {
            let owner = self.doc.ancestor(m, |k| {
                matches!(k, NodeKind::Table { .. } | NodeKind::Cell { .. })
            });
            match self.kind(m) {
                NodeKind::Cell { .. }
                    if self
                        .doc
                        .ancestor(self.doc.nodes[m].parent.unwrap_or(0), |k| {
                            matches!(k, NodeKind::Table { .. })
                        })
                        == Some(n) =>
                {
                    cells.push(m)
                }
                _ => {}
            }
            if owner != Some(n) {
                continue;
            }
            for c in self.doc.nodes[m].children.clone() {
                if let Child::Item(r) = c {
                    if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) {
                        self.consumed.insert(r);
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
        let top = tops.first().copied();
        if let (Some((page, y)), Some((p, bottom))) = (top, self.prev_bottom) {
            if p == page && !ctx.nested {
                self.table_gap = Some(tw((y - bottom).max(0.0)));
            }
        }
        let forced = self.break_pending
            && !ctx.nested
            && match (self.prev, top) {
                (Some(Prev::Baseline(p, _) | Prev::Bottom(p, _)), Some((q, _))) => p < q,
                _ => false,
            };
        self.break_pending = false;
        let saved = self.prev;
        let border = |b: Option<(f64, Rgb)>| {
            b.map(|(w, c)| Border {
                size: ((w * 8.0).round() as u32).clamp(2, 96),
                color: hex(c),
            })
        };
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
                let Some(m) = here.or(covering) else {
                    row_cells.push(Cell {
                        props: CellProps {
                            width: grid.get(c).copied(),
                            span: 1,
                            ..CellProps::default()
                        },
                        body: Blocks::default(),
                    });
                    c += 1;
                    continue;
                };
                let NodeKind::Cell {
                    colspan,
                    rowspan,
                    fill,
                    inset,
                    borders,
                    ..
                } = self.kind(m).clone()
                else {
                    unreachable!()
                };
                let span = colspan.max(1).min((ncols - c) as u32);
                let (body, merge) = if here.is_some() {
                    let inner = Ctx {
                        nested: true,
                        left: columns.get(c).copied().unwrap_or(ctx.left) + inset.left,
                        width: grid.iter().skip(c).take(span as usize).sum::<i32>() as f64 / 20.0,
                        align: None,
                        page_number: None,
                        list: None,
                        hint_list: None,
                        toc: None,
                    };
                    self.prev = None;
                    let mut blocks = self.children(m, &inner);
                    let header = (r as u32) < header_rows;
                    for b in &mut blocks {
                        if let Block::Paragraph(p) = b {
                            p.props.keep_next |= header;
                        }
                    }
                    if forced && r == 0 && c == 0 {
                        if let Some(Block::Paragraph(p)) = blocks.first_mut() {
                            p.props.page_break_before = true;
                        }
                    }
                    (
                        Blocks::from_blocks(blocks),
                        (rowspan > 1).then_some(VMerge::Restart),
                    )
                } else {
                    (Blocks::default(), Some(VMerge::Continue))
                };
                let [t, rt, b, l] = borders;
                row_cells.push(Cell {
                    props: CellProps {
                        width: Some(grid.iter().skip(c).take(span as usize).sum()),
                        span,
                        merge,
                        borders: Some(Borders {
                            top: border(t),
                            left: border(l),
                            bottom: border(b),
                            right: border(rt),
                            inside_h: None,
                            inside_v: None,
                        }),
                        fill: fill.map(hex),
                        no_wrap: span == 1 && content_sized.get(c).copied().unwrap_or(false),
                        margins: Some(CellMargins {
                            top: tw(inset.top),
                            left: tw(inset.left),
                            bottom: tw(inset.bottom),
                            right: tw(inset.right),
                        }),
                        valign: None,
                    },
                    body,
                });
                c += span as usize;
            }
            let height = match (tops.get(r), tops.get(r + 1)) {
                (Some((p, a)), Some((q, b))) if p == q => Some(tw(b - a)),
                _ => None,
            };
            rows.push(Row {
                props: RowProps {
                    cant_split: true,
                    header: (r as u32) < header_rows,
                    min_height: height,
                },
                cells: row_cells,
            });
        }
        self.prev = saved;
        if let Some(&(p, f)) = self.doc.nodes[n].frames.last() {
            self.prev = Some(Prev::Bottom(p, f.y1));
            if !ctx.nested {
                self.prev_bottom = Some((p, f.y1));
            }
        }
        Table {
            props: TableProps {
                style: None,
                width: Some(grid.iter().sum()),
                indent: columns.first().map(|x| tw(x - ctx.left)),
                borders: None,
                fixed: true,
                cell_margins: None,
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

    /// A drawn line or box as a shape anchored to the page where it sits.
    fn shape(&mut self, shape: &crate::layout::Shape) -> Drawing {
        self.z += 1;
        Drawing {
            placement: Placement::Anchor(Anchor {
                x: emu(shape.rect.x0),
                y: emu(shape.rect.y0),
                behind_text: shape.fill.is_some(),
                z: self.z,
                wrap: Wrap::None,
            }),
            width: emu(shape.rect.width().max(0.05)),
            height: emu(shape.rect.height().max(0.05)),
            content: DrawingContent::Shape(crate::model::Shape {
                geometry: match shape.geometry {
                    Geometry::Line { rising } => ShapeGeometry::Line { rising },
                    Geometry::Rect => ShapeGeometry::Rect,
                },
                fill: shape.fill.map(hex),
                stroke: shape.stroke.map(|(w, c)| Stroke {
                    width: emu(w),
                    color: hex(c),
                }),
            }),
            descr: None,
        }
    }

    /// The shapes inside `n`'s inline content that no decoration explains,
    /// which a paragraph holds as anchored drawings.
    fn inline_shapes(&mut self, n: NodeId) {
        let kids = self.doc.nodes[n].children.clone();
        for c in kids {
            match c {
                Child::Item(r) if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) => {
                    self.loose_item(r, &Ctx::default());
                }
                Child::Node(m) if read_into(self.kind(m)) => self.inline_shapes(m),
                _ => {}
            }
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
            hint_list: None,
            toc: None,
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
            .chain(
                self.chrome_places
                    .iter()
                    .filter(|c| pages.contains(&c.0))
                    .copied(),
            )
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
            let floats: Vec<NodeId> = if footer && pages.first() == Some(&0) {
                std::mem::take(&mut self.footer_floats)
            } else {
                Vec::new()
            };
            if floats.is_empty() && pages.iter().all(|&p| of(p).is_empty()) {
                continue;
            }
            let sig = |l: &Self, p: usize| l.signature(&of(p), p);
            let first = pages[0];
            let second = pages.get(1).copied();
            let differs =
                !floats.is_empty() || second.is_some_and(|s| sig(self, first) != sig(self, s));
            let mut make = |l: &mut Self, p: usize, floats: &[NodeId]| -> usize {
                let ctx = Ctx {
                    page_number: l.doc.pages[p].number_text.clone(),
                    ..Ctx::default()
                };
                let mut drawings = Vec::new();
                for n in of(p) {
                    drawings.extend(l.text_box(n, &ctx));
                }
                let mut line = 20;
                if let Some(band) = floats
                    .iter()
                    .filter_map(|&f| l.doc.nodes[f].frames.first().map(|r| r.1.y0))
                    .reduce(f64::min)
                {
                    let page = &l.doc.pages[p];
                    let distance =
                        (page.setup.margins.bottom - page.setup.footer_descent - 1.0).max(0.0);
                    line = tw((page.size[1] - distance - band).max(1.0));
                    for &f in floats {
                        drawings.extend(l.text_box(f, &Ctx::default()));
                    }
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
                            line: Some(Line::Exact(line)),
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
                refs[part].first = Some(make(self, first, &floats));
                refs[part].default = match second {
                    Some(second) => Some(make(self, second, &[])),
                    None => None,
                };
            } else {
                refs[part].default = Some(make(self, first, &[]));
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
                            ItemKind::Image { rect, image, .. }
                            | ItemKind::Raster { rect, image } => out.push((
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

/// A list level's format, `lvlText` and start from the list's marker.
fn level_of(marker: &Marker, level: u8) -> (NumFormat, String, u32) {
    match marker {
        Marker::Bullet(glyph) => (NumFormat::Bullet, glyph.clone(), 1),
        Marker::Numbered {
            counters,
            suffix,
            start,
        } => {
            let first = (level as usize + 1).saturating_sub(counters.len());
            let mut text = String::new();
            for (i, (prefix, _)) in counters.iter().enumerate() {
                text.push_str(prefix);
                text.push_str(&format!("%{}", first + i + 1));
            }
            text.push_str(suffix);
            let format = match counters.last().map(|c| c.1) {
                Some(NumberFormat::LowerLetter) => NumFormat::LowerLetter,
                Some(NumberFormat::UpperLetter) => NumFormat::UpperLetter,
                Some(NumberFormat::LowerRoman) => NumFormat::LowerRoman,
                Some(NumberFormat::UpperRoman) => NumFormat::UpperRoman,
                _ => NumFormat::Decimal,
            };
            (format, text, (*start).clamp(0, u32::MAX as u64) as u32)
        }
        Marker::Printed => (NumFormat::None, String::new(), 1),
    }
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

    use super::number_format;
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
        fn a_roman_page_number_has_a_roman_format(n in 1u32..4000) {
            let r = roman(n);
            prop_assert_eq!(number_format(&r), Some(NumFormat::LowerRoman));
            prop_assert_eq!(number_format(&r.to_uppercase()), Some(NumFormat::UpperRoman));
        }

        #[test]
        fn a_decimal_page_number_is_decimal(n in 0u64..100000) {
            prop_assert_eq!(number_format(&n.to_string()), Some(NumFormat::Decimal));
        }
    }
}
