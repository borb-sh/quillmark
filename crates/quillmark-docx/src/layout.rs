//! The Layout IR: a laid-out document as a tree in reading order whose leaves
//! are ink, plus each page's items in paint order.
//!
//! Points, `f64`, the page's top-left corner as origin, every length
//! absolute. Nothing here names a Word concept or a Typst element: a reader
//! fills it from its own engine's output, and lowering turns
//! it into a [`Document`](crate::model::Document).

/// An index into [`LayoutDoc::nodes`]. Node 0 is the root.
pub type NodeId = usize;
/// An index into [`LayoutDoc::images`].
pub type ImageId = usize;
/// An index into [`LayoutDoc::fonts`].
pub type FontId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDoc {
    pub pages: Vec<Page>,
    pub nodes: Vec<Node>,
    pub fonts: Vec<FontFace>,
    pub images: Vec<ImageData>,
    pub info: DocInfo,
    /// What the reader itself found that the export carries less of.
    pub notices: Vec<Notice>,
}

impl Default for LayoutDoc {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            nodes: vec![Node::new(None, NodeKind::Root)],
            fonts: Vec::new(),
            images: Vec::new(),
            info: DocInfo::default(),
            notices: Vec::new(),
        }
    }
}

impl LayoutDoc {
    pub fn item(&self, r: ItemRef) -> &Item {
        &self.pages[r.page as usize].items[r.index as usize]
    }

    /// `id` and its descendants' ids, depth first in reading order.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let at = out[i];
            let kids: Vec<NodeId> = self.nodes[at]
                .children
                .iter()
                .filter_map(|c| match c {
                    Child::Node(n) => Some(*n),
                    Child::Item(_) => None,
                })
                .collect();
            out.splice(i + 1..i + 1, kids);
            i += 1;
        }
        out
    }

    /// The nearest ancestor of `id`, `id` included, whose kind `pred` accepts.
    pub fn ancestor(&self, id: NodeId, pred: impl Fn(&NodeKind) -> bool) -> Option<NodeId> {
        let mut at = Some(id);
        while let Some(n) = at {
            if pred(&self.nodes[n].kind) {
                return Some(n);
            }
            at = self.nodes[n].parent;
        }
        None
    }

    /// The cells of `table` itself, not of a table nested in it, in reading
    /// order.
    pub fn cells_of(&self, table: NodeId) -> Vec<NodeId> {
        self.subtree(table)
            .into_iter()
            .filter(|&n| {
                matches!(self.nodes[n].kind, NodeKind::Cell { .. })
                    && self.ancestor(self.nodes[n].parent.unwrap_or(0), |k| {
                        matches!(k, NodeKind::Table { .. })
                    }) == Some(table)
            })
            .collect()
    }

    /// Whether `node` lies in a superscript or subscript inside `top`.
    pub fn raised(&self, mut node: NodeId, top: NodeId) -> bool {
        while node != top {
            if matches!(self.nodes[node].kind, NodeKind::Super | NodeKind::Sub) {
                return true;
            }
            match self.nodes[node].parent {
                Some(p) => node = p,
                None => return false,
            }
        }
        false
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocInfo {
    pub title: Option<String>,
    /// The document's language, as a BCP 47 tag. Where it is `None`, the
    /// language most of the text is set in.
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sides {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub size: [f64; 2],
    /// From the page probe, or assumed with a notice when it is missing.
    pub setup: PageSetup,
    /// The engine's logical page number.
    pub number: u64,
    /// What the page's number displays as, when the reader could see it.
    pub number_text: Option<String>,
    /// Paint order, which is also stacking order.
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    pub margins: Sides,
    pub columns: u32,
    pub gutter: f64,
    pub flipped: bool,
    /// How far the header's bottom sits above the top margin.
    pub header_ascent: f64,
    /// How far the footer's top sits below the bottom margin.
    pub footer_descent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub node: NodeId,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Text(TextRun),
    Image {
        image: ImageId,
        rect: Rect,
        alt: Option<String>,
    },
    Shape(Shape),
    /// Ink the IR cannot hold, as a picture of it.
    Raster {
        image: ImageId,
        rect: Rect,
        alt: Option<String>,
    },
    Mark(PointHint),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// The baseline start.
    pub origin: [f64; 2],
    pub text: String,
    pub font: FontId,
    pub size: f64,
    pub fill: Rgb,
    /// The advance width as laid out.
    pub width: f64,
    /// The advance width of the face's own advances and the tracking, as a
    /// writer that does not kern sets the text.
    pub natural: f64,
    /// The extra advance after each character, in points.
    pub tracking: Option<f64>,
    /// Whether a space in it is set wider or narrower than the face's own
    /// and the tracking make it, as justification sets one.
    pub justified: bool,
    /// The language the text is set in, as a BCP 47 tag: `de`, or `de-DE`
    /// with a region.
    pub lang: Option<String>,
}

impl TextRun {
    /// The run's box from an approximate ascent and descent.
    pub fn rect(&self) -> Rect {
        Rect {
            x0: self.origin[0],
            y0: self.origin[1] - 0.8 * self.size,
            x1: self.origin[0] + self.width,
            y1: self.origin[1] + 0.2 * self.size,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub rect: Rect,
    pub geometry: Geometry,
    pub fill: Option<Rgb>,
    pub stroke: Option<(f64, Rgb)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Geometry {
    /// From the rect's top-left corner to its bottom-right, or from its
    /// bottom-left to its top-right when `rising`.
    Line {
        rising: bool,
    },
    Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    Odd,
    Even,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PointHint {
    /// A forced line break.
    Br,
    /// A forced page break.
    Break { weak: bool, to: Option<Parity> },
    /// The reference mark of a footnote.
    FootnoteRef { note: NodeId },
    /// No page break between the block before and the block after.
    Keep,
    /// The name a reader without the face should use for `family`.
    Font { family: String, alt: String },
}

/// One level of a numbering scheme, as the quill describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct HintLevel {
    pub format: NumberFormat,
    pub prefix: String,
    /// What follows the number, spacing included.
    pub suffix: String,
    pub underline: bool,
    /// False for a level that counts and prints no number.
    pub visible: bool,
    /// The first line's indent from the column's edge.
    pub indent: f64,
}

/// What a quill says about the content between a bracket's open and close.
#[derive(Debug, Clone, PartialEq)]
pub enum BracketHint {
    /// One instance of the numbering scheme `name`.
    List {
        name: String,
        levels: Vec<HintLevel>,
    },
    /// The automatic number, at 1-based `level`, of the paragraph it sits in.
    Label { level: u8, value: Option<u64> },
    /// A named role for the paragraphs inside, and the role after them.
    Role { name: String, next: Option<String> },
    /// A grid that is a line of label, tab and text.
    Tabbed,
    /// A placed object that belongs to the page.
    Chrome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemRef {
    pub page: u32,
    pub index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    Node(NodeId),
    Item(ItemRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnd {
    /// A wrap: the line's text ends where the next line's continues, with
    /// the space the line broke at, if any, still in it.
    Soft,
    /// A wrap at a hyphen the engine inserted, which ends the line's text.
    Hyphenated,
    /// A line the engine did not end by wrapping: a forced break, or lines
    /// laid out apart, such as a stack's children.
    Forced,
    Last,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub page: u32,
    pub baseline: f64,
    pub x0: f64,
    pub x1: f64,
    /// Text items ordered by x, then the marks that sit on the line.
    pub items: Vec<ItemRef>,
    pub end: LineEnd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    /// Reading order.
    pub children: Vec<Child>,
    /// On paragraph-forming nodes.
    pub lines: Vec<Line>,
    /// The node's ink box, one per page it appears on.
    pub frames: Vec<(u32, Rect)>,
}

impl Node {
    pub fn new(parent: Option<NodeId>, kind: NodeKind) -> Self {
        Self {
            parent,
            kind,
            children: Vec::new(),
            lines: Vec::new(),
            frames: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HAlign {
    #[default]
    Start,
    Center,
    End,
}

/// What the par and block probes resolve from the style chain.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParStyle {
    pub align: HAlign,
    pub size: f64,
    pub leading: f64,
    pub spacing: f64,
    /// How far a line's box reaches above and below its baseline, from the
    /// text edges in force and the face; zero when they are not known.
    pub top_edge: f64,
    pub bottom_edge: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Horizon,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Header,
    Footer,
    Background,
    PageNumber,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberFormat {
    Decimal,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
}

/// What a list prints before each item, from the list's own fields.
#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    /// The glyph printed at this nesting depth.
    Bullet(String),
    /// The counters printed at this level, outermost first, each with the
    /// text before it, then the text after the last; `start` is the first
    /// item's number.
    Numbered {
        counters: Vec<(String, NumberFormat)>,
        suffix: String,
        start: u64,
    },
    /// Numbers no pattern describes, which the labels' ink holds as printed.
    Printed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Root,
    /// A run of in-flow ink under no paragraph-forming node.
    Unstructured,
    Par {
        justify: bool,
        first_line: f64,
        hanging: f64,
        style: Option<ParStyle>,
        /// The x where its lines start before any indent, on its first page.
        left: Option<f64>,
    },
    Heading {
        level: u8,
        outlined: bool,
        numbered: bool,
    },
    ListBody,
    /// A run of inline children of a non-inline node.
    InlineRun {
        style: Option<ParStyle>,
    },
    Caption,
    Strong,
    Emph,
    Underline,
    Strike,
    Highlight {
        fill: Rgb,
    },
    Sub,
    Super,
    Link {
        url: String,
    },
    /// Inline content that must not wrap.
    NoBreak,
    Block {
        sticky: bool,
        breakable: bool,
        above: f64,
        below: f64,
        style: ParStyle,
    },
    Align {
        h: HAlign,
    },
    Columns {
        count: u32,
        gutter: f64,
    },
    Figure,
    /// `columns` holds the n + 1 column boundaries; `content_sized` says
    /// which columns are as wide as their content; `rows` holds each row's
    /// page and top.
    Table {
        columns: Vec<f64>,
        content_sized: Vec<bool>,
        rows: Vec<(u32, f64)>,
        header_rows: u32,
        grid: bool,
    },
    /// A cell's `borders` are its top, right, bottom and left strokes.
    Cell {
        col: u32,
        row: u32,
        colspan: u32,
        rowspan: u32,
        header: bool,
        fill: Option<Rgb>,
        inset: Sides,
        borders: [Option<(f64, Rgb)>; 4],
    },
    List {
        marker: Marker,
        /// Whether each label is set flush against the body's edge.
        label_end: bool,
    },
    /// The label Typst prints before a list item's or a footnote entry's
    /// body.
    ListLabel,
    Place {
        float: bool,
        valign: Option<VAlign>,
    },
    Artifact {
        kind: ArtifactKind,
    },
    Footnote,
    Hint(BracketHint),
    /// A table of contents over the heading levels to `depth`, or all.
    Toc {
        depth: Option<u8>,
    },
    /// One entry of a table of contents, for a heading at `level`.
    TocEntry {
        level: u8,
    },
    /// The ink that leads from an entry's text to its page number.
    TocFill,
    /// An entry's page number as printed.
    TocPage,
    /// A block quote: its body and its attribution.
    Quote,
    /// A raw block.
    Raw,
    /// One line of a raw block.
    RawLine,
}

impl NodeKind {
    /// A node whose content Word holds as one paragraph.
    pub fn forms_paragraph(&self) -> bool {
        matches!(
            self,
            NodeKind::Par { .. }
                | NodeKind::Heading { .. }
                | NodeKind::ListBody
                | NodeKind::InlineRun { .. }
                | NodeKind::Caption
                | NodeKind::Unstructured
                | NodeKind::RawLine
        )
    }

    /// A node that sits inside a line of text.
    pub fn is_inline(&self) -> bool {
        matches!(
            self,
            NodeKind::Strong
                | NodeKind::Emph
                | NodeKind::Underline
                | NodeKind::Strike
                | NodeKind::Highlight { .. }
                | NodeKind::Sub
                | NodeKind::Super
                | NodeKind::Link { .. }
                | NodeKind::NoBreak
                | NodeKind::Hint(BracketHint::Label { .. })
                | NodeKind::TocFill
                | NodeKind::TocPage
        )
    }
}
