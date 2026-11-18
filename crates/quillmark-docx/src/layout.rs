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
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocInfo {
    pub title: Option<String>,
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
    /// A wrap; `ate_space` when the space the line broke at ends its text.
    Soft {
        ate_space: bool,
    },
    /// A wrap at a hyphen the engine inserted, which ends the line's text.
    Hyphenated,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Header,
    Footer,
    Background,
    PageNumber,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    Bullet,
    Numbered,
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
    Highlight,
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
    /// `columns` holds the n + 1 column boundaries.
    Table {
        columns: Vec<f64>,
        header_rows: u32,
        grid: bool,
    },
    Cell {
        col: u32,
        row: u32,
        colspan: u32,
        rowspan: u32,
        header: bool,
    },
    List {
        marker: Marker,
    },
    ListLabel,
    Place {
        float: bool,
    },
    Artifact {
        kind: ArtifactKind,
    },
    Footnote,
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
                | NodeKind::Highlight
                | NodeKind::Sub
                | NodeKind::Super
                | NodeKind::Link { .. }
                | NodeKind::NoBreak
        )
    }
}
