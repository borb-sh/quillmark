//! Lowering: a [`LayoutDoc`] to a Word [`Document`].
//!
//! One walk of the tree in reading order. Each paragraph-forming node is one
//! Word paragraph whose lines are joined back into running text; its kind
//! decides its style, numbering and keeps, and measurement decides only the
//! numbers: indents, spacing, line pitch, widths. Chrome and placed content
//! become page-anchored text boxes. Every text and image item the walk does
//! not place is counted under `docx::ink_dropped`.
//!
//! The walk yields each scope (the body, a cell, a text box, a footnote) as a
//! flat list of entries in reading order, and one pass over that list decides
//! what depends on a neighbour: the space above a paragraph, the gap above a
//! table, keeps, page breaks, and the paragraph a drawing is anchored in.

mod drawings;
mod lists;
mod notices;
mod paragraph;
mod sections;
mod space;
mod styles;
mod tables;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::layout::{
    ArtifactKind, BracketHint, Child, HAlign, ItemKind, ItemRef, LayoutDoc, LineEnd, NodeId,
    NodeKind, Notice, PointHint, Rgb, VAlign,
};
use crate::model::{
    Align, Block, Blocks, Columns, Document, FirstLine, FontDecl, Footnote, Indent, Info, Inline,
    Line, Media, PageMargins, PageNumbers, ParProps, Paragraph, Placement, Run, RunContent,
    RunProps, Section, SectionProps, SectionStart, Settings, Spacing, Style, Toc, Twips, VertAlign,
    Wrap,
};
use crate::notice::{Code, Tally};

use lists::Scheme;
use sections::{Pages, fill_inherited};
use space::{Entry, Hold, Placed, Prev, Strides, drop_opening_break};
use tables::tabbed_hint;

pub const CODE_FONT_NOT_EMBEDDED: &str = Code::FontNotEmbedded.as_str();
pub const CODE_PLACE_ANCHORED: &str = Code::PlaceAnchored.as_str();
pub const CODE_SHAPE_DROPPED: &str = Code::ShapeDropped.as_str();
pub const CODE_INK_DROPPED: &str = Code::InkDropped.as_str();
pub const CODE_LABEL_MISMATCH: &str = Code::LabelMismatch.as_str();
pub const CODE_TOC_FROZEN: &str = Code::TocFrozen.as_str();
pub const CODE_HINT_UNCHECKED: &str = Code::HintMisused.as_str();
pub const CODE_COLUMNS_FLATTENED: &str = Code::ColumnsFlattened.as_str();
pub const CODE_DIRECTION_DROPPED: &str = Code::DirectionDropped.as_str();
pub const CODE_UNSTRUCTURED_TEXT: &str = Code::UnstructuredText.as_str();
pub const CODE_PAGE_NUMBER_FROZEN: &str = Code::PageNumberFrozen.as_str();
pub const CODE_LINK_SCRIPT: &str = Code::LinkScript.as_str();

fn tw(pt: f64) -> Twips {
    (pt * 20.0).round() as Twips
}

/// An empty paragraph `height` twips tall, at least one.
fn spacer(height: Twips) -> Paragraph {
    Paragraph {
        props: ParProps {
            spacing: Some(Spacing {
                before: Some(0),
                after: Some(0),
                line: Some(Line::Exact(height.max(1))),
            }),
            ..ParProps::default()
        },
        content: Vec::new(),
    }
}

fn hex(c: Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// The Word document for `doc` and what the export carries less of.
pub fn lower(doc: &LayoutDoc) -> (Document, Vec<Notice>) {
    let pages = Pages::new(doc);
    let strides = Strides::new(doc);
    let mut l = Lower::new(doc, &pages, &strides);
    let document = l.document();
    let mut seen = HashSet::new();
    let notices = doc
        .notices
        .iter()
        .cloned()
        .chain(l.notices())
        .filter(|n| seen.insert((n.code.clone(), n.message.clone())))
        .collect();
    (document, notices)
}

impl<'a> Lower<'a> {
    fn new(doc: &'a LayoutDoc, pages: &'a Pages<'a>, strides: &'a Strides<'a>) -> Self {
        Lower {
            doc,
            pages,
            strides,
            consumed: HashSet::new(),
            fonts: BTreeMap::new(),
            numbering: Default::default(),
            footnotes: Vec::new(),
            notes: HashMap::new(),
            media: Vec::new(),
            media_of: HashMap::new(),
            z: 0,
            headings: BTreeSet::new(),
            unlisted: BTreeSet::new(),
            builtin: BTreeSet::new(),
            toc_levels: BTreeSet::new(),
            toc_prefixed: false,
            label_prefix: None,
            footer_floats: Vec::new(),
            skip: HashSet::new(),
            schemes: Vec::new(),
            counters: HashMap::new(),
            extra_styles: Vec::new(),
            roles: BTreeMap::new(),
            alt_names: doc
                .pages
                .iter()
                .flat_map(|p| &p.items)
                .filter_map(|i| match &i.kind {
                    ItemKind::Mark(PointHint::Font { family, alt }) => {
                        Some((family.to_lowercase(), alt.clone()))
                    }
                    _ => None,
                })
                .collect(),
            tally: Tally::default(),
        }
    }
}

#[derive(Clone, Default)]
struct Ctx {
    align: Option<HAlign>,
    /// The x of the container's left edge, which indents are measured from.
    left: f64,
    /// The container's width, for centred chrome.
    width: f64,
    /// The page whose number chrome text prints.
    page_number: Option<usize>,
    list: Option<(u32, u8)>,
    /// The `list` hint around: its scheme, its `w:num`, and whether it is
    /// the scheme's first instance.
    hint_list: Option<(usize, u32, bool)>,
    /// Inside a table of contents entry at this level.
    toc: Option<u8>,
}

struct Lower<'a> {
    doc: &'a LayoutDoc,
    pages: &'a Pages<'a>,
    strides: &'a Strides<'a>,
    consumed: HashSet<ItemRef>,
    fonts: BTreeMap<String, FontDecl>,
    numbering: crate::model::Numbering,
    footnotes: Vec<Footnote>,
    notes: HashMap<NodeId, usize>,
    media: Vec<Media>,
    media_of: HashMap<usize, usize>,
    z: u32,
    headings: BTreeSet<u8>,
    unlisted: BTreeSet<u8>,
    /// Word's own paragraph styles in use, by id and name.
    builtin: BTreeSet<(&'static str, &'static str)>,
    toc_levels: BTreeSet<u8>,
    /// An entry of the table of contents being lowered prints a page number
    /// that is no plain numeral.
    toc_prefixed: bool,
    /// A printed list number the next paragraph opens with.
    label_prefix: Option<String>,
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
    /// The Word face a family with no metric clone is named as, from the
    /// quill's `font` hint, keyed by the family in lower case.
    alt_names: BTreeMap<String, String>,
    tally: Tally,
}

impl<'a> Lower<'a> {
    fn kind(&self, n: NodeId) -> &NodeKind {
        &self.doc.nodes[n].kind
    }

    fn first_page(&self, n: NodeId) -> Option<u32> {
        self.doc.nodes[n].frames.first().map(|f| f.0)
    }

    fn document(&mut self) -> Document {
        let doc = self.doc;
        let groups = &self.pages.groups;
        let starts = &self.pages.starts;
        let mut sections: Vec<Section> = Vec::new();
        let section_of = |page: u32| space::section_of(groups, page);
        let root = doc.nodes[0].children.clone();
        let mut entries = Vec::new();
        let mut current = 0;
        for c in root {
            let page = match c {
                Child::Node(n) => {
                    if self.separator(n) {
                        self.consume_subtree(n);
                        continue;
                    }
                    if matches!(
                        self.kind(n),
                        NodeKind::Artifact {
                            kind: ArtifactKind::Header
                                | ArtifactKind::Footer
                                | ArtifactKind::Background
                                | ArtifactKind::PageNumber
                        } | NodeKind::Footnote
                    ) {
                        continue;
                    }
                    self.first_page(n)
                }
                Child::Item(r) => Some(r.page),
            };
            entries.push(Entry::Child(page));
            if let Some(p) = page {
                current = section_of(p);
                let last = match c {
                    Child::Node(n) => doc.nodes[n].frames.last().map(|f| f.0),
                    Child::Item(r) => Some(r.page),
                };
                let to = last.map_or(current, section_of);
                if to > current
                    && starts[current + 1..=to] == vec![SectionStart::Continuous; to - current][..]
                {
                    current = to;
                }
            }
            let ctx = Ctx {
                left: doc.pages[groups[current][0]].setup.margins.left,
                ..Ctx::default()
            };
            entries.push(Entry::Section(current));
            entries.extend(self.child(&c, &ctx));
        }
        let mut bodies = space::body(doc, groups, entries);
        for (body, start) in bodies.iter_mut().zip(starts) {
            if matches!(start, SectionStart::OddPage | SectionStart::EvenPage) {
                drop_opening_break(body);
            }
        }
        let roles = self.role_styles(&mut bodies);
        self.extra_styles.extend(roles);
        let mut headers = Vec::new();
        let mut footers = Vec::new();
        let mut had = [[false; 2]; 2];
        let mut prev_number: Option<u64> = None;
        for ((pages, body), &start) in groups.iter().zip(bodies).zip(starts) {
            let first = &doc.pages[pages[0]];
            let s = first.setup;
            let (mut header, mut footer, title_page) =
                self.chrome(pages, &mut headers, &mut footers);
            let [had_header, had_footer] = &mut had;
            fill_inherited(&mut header, had_header, &mut headers);
            fill_inherited(&mut footer, had_footer, &mut footers);
            let format = self.pages.printed(pages[0]).map(|p| p.format);
            let restart = match prev_number {
                Some(p) => p + 1 != first.number,
                None => first.number != 1,
            };
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
                    start,
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

    fn child(&mut self, c: &Child, ctx: &Ctx) -> Vec<Entry> {
        match c {
            Child::Node(n) => self.node(*n, ctx),
            Child::Item(r) => self.loose_item(*r, ctx),
        }
    }

    fn children(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Entry> {
        self.held(n, ctx, Hold::None)
    }

    /// `n`'s children, with the keeps `hold` puts on their paragraphs.
    fn held(&mut self, n: NodeId, ctx: &Ctx, hold: Hold) -> Vec<Entry> {
        let kids = self.doc.nodes[n].children.clone();
        let mut out = vec![Entry::Open];
        for c in &kids {
            out.push(Entry::Child(self.child_page(c)));
            out.extend(self.child(c, ctx));
        }
        out.push(Entry::Close(hold));
        out
    }

    fn loose_item(&mut self, r: ItemRef, ctx: &Ctx) -> Vec<Entry> {
        match &self.doc.item(r).kind {
            ItemKind::Mark(PointHint::Keep) => {
                self.consumed.insert(r);
                vec![Entry::Keep]
            }
            ItemKind::Mark(PointHint::Font { .. }) => {
                self.consumed.insert(r);
                Vec::new()
            }
            ItemKind::Mark(PointHint::Break { .. }) => {
                self.consumed.insert(r);
                vec![Entry::Break]
            }
            ItemKind::Shape(shape) => {
                self.consumed.insert(r);
                let decoration = matches!(
                    self.kind(self.doc.item(r).node),
                    NodeKind::Underline | NodeKind::Strike | NodeKind::Highlight { .. }
                );
                if decoration {
                    return Vec::new();
                }
                vec![Entry::Drawing(r.page, self.shape(shape))]
            }
            ItemKind::Image { .. } | ItemKind::Raster { .. } => {
                self.consumed.insert(r);
                let (image, rect, alt) = match &self.doc.item(r).kind {
                    ItemKind::Image { image, rect, alt } => (*image, *rect, alt.clone()),
                    ItemKind::Raster { image, rect, alt } => (*image, *rect, alt.clone()),
                    _ => unreachable!(),
                };
                let rect = &rect;
                let drawing = self.picture(image, *rect, alt, Placement::Inline);
                let mut p = Paragraph {
                    props: space::line_props(rect.height(), true),
                    content: vec![Inline::Run(Run {
                        props: RunProps::default(),
                        content: vec![RunContent::Drawing(drawing)],
                    })],
                };
                let node = self.doc.item(r).node;
                p.props.align = match ctx.align.or(self.par_style(node).map(|s| s.align)) {
                    Some(HAlign::Center) => Some(Align::Center),
                    Some(HAlign::End) => Some(Align::End),
                    _ => None,
                };
                let at = Placed {
                    page: r.page,
                    baseline: rect.y1,
                    pitch: rect.height(),
                    key: None,
                    spacing: 0.0,
                    object: true,
                    end: Prev::Bottom(r.page, rect.y1),
                    bottom: None,
                };
                vec![Entry::Par(p, at)]
            }
            _ => Vec::new(),
        }
    }

    fn node(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Entry> {
        let kind = self.kind(n).clone();
        if kind == NodeKind::Unstructured && !self.doc.nodes[n].lines.is_empty() {
            let lines = self.doc.nodes[n].lines.clone();
            self.tally.count(Code::UnstructuredText, "", lines.len());
            return lines
                .into_iter()
                .flat_map(|line| {
                    let line = crate::layout::Line {
                        end: LineEnd::Last,
                        ..line
                    };
                    self.paragraph_of(n, vec![line], ctx)
                })
                .collect();
        }
        if kind.forms_paragraph() && !self.doc.nodes[n].lines.is_empty() {
            return self.paragraph(n, ctx);
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
            NodeKind::Table { .. } if tabbed_hint(self.doc, n).is_some() => {
                match self.tabbed(n, ctx) {
                    Some(entries) => entries,
                    None => {
                        self.tally.count(Code::GridUntabbed, "", 1);
                        self.table(n, ctx)
                    }
                }
            }
            NodeKind::Table { .. } => self.table(n, ctx),
            NodeKind::List { .. } => self.list(n, ctx),
            NodeKind::Block {
                sticky, breakable, ..
            } => self.held(n, ctx, Hold::Block { sticky, breakable }),
            NodeKind::Align { h } => {
                let ctx = Ctx {
                    align: Some(h),
                    ..ctx.clone()
                };
                self.children(n, &ctx)
            }
            NodeKind::Columns { count, .. } if count > 1 => {
                self.tally.count(Code::ColumnsFlattened, "", 1);
                self.children(n, ctx)
            }
            NodeKind::Artifact {
                kind: ArtifactKind::Other,
            } if self.doc.nodes[n].parent == Some(0) => {
                let drawings = self.text_box(n, ctx);
                self.tally.count(Code::PlaceAnchored, "", drawings.len());
                let page = self.first_page(n).unwrap_or_default();
                drawings
                    .into_iter()
                    .map(|d| Entry::Drawing(page, d))
                    .collect()
            }
            NodeKind::Place { float, valign } => {
                if self.pages.is_chrome_place(n) {
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
                    self.tally.count(Code::PlaceAnchored, "", 1);
                }
                let page = self.first_page(n).unwrap_or_default();
                drawings
                    .into_iter()
                    .map(|d| Entry::Drawing(page, d))
                    .collect()
            }
            NodeKind::ListLabel => {
                self.consume_subtree(n);
                Vec::new()
            }
            NodeKind::Toc { depth } => self.toc(n, depth, ctx),
            NodeKind::Figure => self.held(n, ctx, Hold::Figure),
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
    fn toc(&mut self, n: NodeId, depth: Option<u8>, ctx: &Ctx) -> Vec<Entry> {
        let mut before = Vec::new();
        let mut entries = Vec::new();
        let mut after = Vec::new();
        self.toc_prefixed = false;
        for c in self.doc.nodes[n].children.clone() {
            let entry = matches!(c, Child::Node(m) if self.doc.subtree(m).iter().any(|&e| {
                matches!(self.kind(e), NodeKind::TocEntry { .. })
            }));
            let produced = self.child(&c, ctx);
            if entry {
                entries.extend(produced);
            } else if !entries.iter().any(|e| matches!(e, Entry::Par(..))) {
                before.extend(produced);
            } else {
                after.extend(produced);
            }
        }
        let locked = self.toc_prefixed;
        if entries.iter().any(|e| matches!(e, Entry::Par(..))) {
            self.tally.count(Code::TocFrozen, "", locked as usize);
        }
        let toc = Toc {
            instruction: format!(
                "TOC \\o \"1-{}\" \\h \\z \\u",
                depth.unwrap_or(9).clamp(1, 9)
            ),
            locked,
            entries: Vec::new(),
        };
        before.push(Entry::Toc(toc, entries));
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

    /// Whether `n` is the rule Typst draws above a page's footnotes: an
    /// artifact that is not chrome, holding only shapes, on a page with a
    /// footnote. Word draws its own.
    fn separator(&self, n: NodeId) -> bool {
        let NodeKind::Artifact {
            kind: ArtifactKind::Other,
        } = self.kind(n)
        else {
            return false;
        };
        let page = self.first_page(n);
        let footnoted = self.doc.nodes[0].children.iter().any(|c| {
            matches!(c, Child::Node(f) if matches!(self.kind(*f), NodeKind::Footnote)
                && self.first_page(*f) == page)
        });
        let shapes_only = self.doc.subtree(n).iter().all(|&m| {
            self.doc.nodes[m].children.iter().all(|c| match c {
                Child::Item(r) => matches!(self.doc.item(*r).kind, ItemKind::Shape(_)),
                Child::Node(_) => true,
            })
        });
        footnoted && shapes_only
    }

    fn child_page(&self, c: &Child) -> Option<u32> {
        match c {
            Child::Node(n) => self.first_page(*n),
            Child::Item(r) => Some(r.page),
        }
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
        let page = self.first_page(note).unwrap_or(0) as usize;
        let left = self
            .doc
            .pages
            .get(page)
            .map_or(0.0, |p| p.setup.margins.left);
        let ctx = Ctx {
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
        let (mut blocks, _) = space::nested(self.children(note, &ctx));
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
        i
    }
}
