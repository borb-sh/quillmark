//! The typed Word document the writer serializes.
//!
//! Every container that Word requires to end in a paragraph (a section, a
//! table cell, a header, a footer, a text box) holds a [`Blocks`], whose last
//! member is a [`Paragraph`] by its type. Lengths are integers in Word's own
//! units: [`Twips`] (1/20 pt), [`HalfPoints`] and [`Emu`] (1/12700 pt).

/// 1/20 of a point.
pub type Twips = i32;
/// Half a point: a font size.
pub type HalfPoints = u32;
/// English Metric Unit: 1/12700 of a point.
pub type Emu = i64;

/// A whole Word document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// At least one. The last section's properties close the body.
    pub sections: Vec<Section>,
    pub headers: Vec<Blocks>,
    pub footers: Vec<Blocks>,
    pub footnotes: Vec<Footnote>,
    pub styles: Styles,
    pub numbering: Numbering,
    pub fonts: Vec<FontDecl>,
    pub media: Vec<Media>,
    pub settings: Settings,
    pub info: Info,
}

/// Title and language for `docProps/core.xml`. No dates: Word stamps them on save.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub title: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub default_tab_stop: Twips,
    /// Whether the even-page header and footer of a section differ from the
    /// default ones.
    pub even_and_odd_headers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_tab_stop: 720,
            even_and_odd_headers: false,
        }
    }
}

/// A run of body blocks ending in a paragraph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Blocks {
    pub blocks: Vec<Block>,
    pub last: Paragraph,
}

impl Blocks {
    /// `paragraphs` in order; an empty list is one empty paragraph.
    pub fn from_paragraphs(mut paragraphs: Vec<Paragraph>) -> Self {
        let last = paragraphs.pop().unwrap_or_default();
        Self {
            blocks: paragraphs.into_iter().map(Block::Paragraph).collect(),
            last,
        }
    }

    /// `blocks` in order, closed by an empty paragraph when the last of them
    /// is not a paragraph.
    pub fn from_blocks(mut blocks: Vec<Block>) -> Self {
        let last = match blocks.pop() {
            Some(Block::Paragraph(p)) => p,
            Some(other) => {
                blocks.push(other);
                Paragraph::default()
            }
            None => Paragraph::default(),
        };
        Self { blocks, last }
    }

    pub fn iter(&self) -> impl Iterator<Item = BlockRef<'_>> {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => BlockRef::Paragraph(p),
                Block::Table(t) => BlockRef::Table(t),
                Block::Toc(t) => BlockRef::Toc(t),
            })
            .chain(std::iter::once(BlockRef::Paragraph(&self.last)))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BlockRef<'a> {
    Paragraph(&'a Paragraph),
    Table(&'a Table),
    Toc(&'a Toc),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Toc(Toc),
}

/// One Word section: its body and its page setup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub body: Blocks,
    pub props: SectionProps,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionProps {
    /// Indices into [`Document::headers`].
    pub header: HeaderRefs,
    /// Indices into [`Document::footers`].
    pub footer: HeaderRefs,
    /// How this section starts after the one before it.
    pub start: SectionStart,
    pub page_width: Twips,
    pub page_height: Twips,
    pub landscape: bool,
    pub margins: PageMargins,
    pub page_numbers: Option<PageNumbers>,
    pub columns: Option<Columns>,
    pub title_page: bool,
}

impl Default for SectionProps {
    fn default() -> Self {
        Self {
            header: HeaderRefs::default(),
            footer: HeaderRefs::default(),
            start: SectionStart::NextPage,
            page_width: 12240,
            page_height: 15840,
            landscape: false,
            margins: PageMargins::default(),
            page_numbers: None,
            columns: None,
            title_page: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeaderRefs {
    pub default: Option<usize>,
    pub first: Option<usize>,
    pub even: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStart {
    NextPage,
    OddPage,
    EvenPage,
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageMargins {
    pub top: Twips,
    pub right: Twips,
    pub bottom: Twips,
    pub left: Twips,
    /// From the page's top edge to the header's top.
    pub header: Twips,
    /// From the page's bottom edge to the footer's bottom.
    pub footer: Twips,
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top: 1440,
            right: 1440,
            bottom: 1440,
            left: 1440,
            header: 720,
            footer: 720,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageNumbers {
    pub format: NumFormat,
    /// Written only where the number restarts.
    pub start: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub count: u32,
    pub space: Twips,
}

/// A paragraph: its properties and inline content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paragraph {
    pub props: ParProps,
    pub content: Vec<Inline>,
}

impl Paragraph {
    pub fn text(props: ParProps, run: RunProps, text: impl Into<String>) -> Self {
        Self {
            props,
            content: vec![Inline::Run(Run {
                props: run,
                content: vec![RunContent::Text(text.into())],
            })],
        }
    }
}

/// `w:pPr`, field by field in schema order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParProps {
    /// A style id from [`Styles`].
    pub style: Option<String>,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub widow_control: Option<bool>,
    pub numbering: Option<NumRef>,
    pub tabs: Vec<TabStop>,
    pub spacing: Option<Spacing>,
    pub indent: Option<Indent>,
    pub align: Option<Align>,
    /// 0-based; `Some(9)` is Word's "body text" level, outside the outline.
    pub outline_level: Option<u8>,
    /// Properties of the paragraph mark.
    pub mark: Option<RunProps>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumRef {
    /// 0-based level.
    pub level: u8,
    pub num: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStop {
    pub kind: TabKind,
    pub pos: Twips,
    pub leader: TabLeader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Left,
    Center,
    Right,
    Decimal,
    /// Removes an inherited stop at the same position.
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabLeader {
    None,
    Dot,
    Hyphen,
    Underscore,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spacing {
    pub before: Option<Twips>,
    pub after: Option<Twips>,
    pub line: Option<Line>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Exact(Twips),
    AtLeast(Twips),
    /// In 240ths of single spacing.
    Auto(i32),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Indent {
    pub left: Option<Twips>,
    pub right: Option<Twips>,
    pub first: Option<FirstLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLine {
    Indent(Twips),
    Hanging(Twips),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Both,
}

/// Inline content of a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Run(Run),
    Hyperlink(Hyperlink),
    /// A complex field: `instruction` and the stored result.
    Field(Field),
    /// A rich-text content control.
    Control(Control),
    BookmarkStart { name: String, id: u32 },
    BookmarkEnd { id: u32 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub props: RunProps,
    pub content: Vec<RunContent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunContent {
    Text(String),
    Tab,
    Break(BreakKind),
    Drawing(Drawing),
    /// A reference to [`Document::footnotes`] by index.
    FootnoteRef(usize),
    /// The number mark inside a footnote's own text.
    FootnoteMark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    Line,
    Page,
}

/// `w:rPr`, field by field in schema order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunProps {
    pub style: Option<String>,
    /// The face, by name: ascii, high ANSI and complex script alike.
    pub font: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub strike: bool,
    pub no_proof: bool,
    /// `RRGGBB`.
    pub color: Option<String>,
    /// Extra advance per character.
    pub spacing: Option<Twips>,
    /// Character scale in percent.
    pub scale: Option<u32>,
    /// Raise (positive) or lower, in half-points.
    pub position: Option<i32>,
    pub size: Option<HalfPoints>,
    pub underline: bool,
    /// A background fill, `RRGGBB`, as cell shading on the run.
    pub shade: Option<String>,
    pub vert: Option<VertAlign>,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hyperlink {
    pub target: LinkTarget,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    Url(String),
    Bookmark(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// For example `PAGE` or `TOC \o "1-3" \h`.
    pub instruction: String,
    pub props: RunProps,
    /// What the field shows until it is updated.
    pub result: Vec<Run>,
    pub locked: bool,
}

/// A field whose stored result is whole paragraphs, such as a table of
/// contents. The field opens in the first entry and closes in the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Toc {
    pub instruction: String,
    pub locked: bool,
    pub entries: Vec<Paragraph>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// The schema address.
    pub tag: String,
    /// What Word shows on the control's tab.
    pub title: String,
    pub kind: ControlKind,
    pub content: Vec<Inline>,
    /// Shown while the control is empty.
    pub placeholder: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    RichText,
    Date,
}

/// A picture, a text box or a shape, in line or anchored to the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub placement: Placement,
    pub width: Emu,
    pub height: Emu,
    pub content: DrawingContent,
    /// Alternative text.
    pub descr: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    Inline,
    Anchor(Anchor),
}

/// A page-anchored object: offsets from the page's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub x: Emu,
    pub y: Emu,
    pub behind_text: bool,
    /// Stacking order: a larger value paints later.
    pub z: u32,
    pub wrap: Wrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    None,
    TopAndBottom,
    Square,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawingContent {
    /// An index into [`Document::media`].
    Picture(usize),
    TextBox(TextBox),
    Shape(Shape),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextBox {
    pub body: Blocks,
    /// Whether text wraps at the box's width.
    pub wrap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub geometry: ShapeGeometry,
    /// `RRGGBB`.
    pub fill: Option<String>,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeGeometry {
    Rect,
    /// From the top-left to the bottom-right of the drawing's box, flipped
    /// vertically when `rising`.
    Line { rising: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub width: Emu,
    /// `RRGGBB`.
    pub color: String,
}

/// A picture's bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub kind: MediaKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Png,
    Jpeg,
}

impl MediaKind {
    pub fn extension(self) -> &'static str {
        match self {
            MediaKind::Png => "png",
            MediaKind::Jpeg => "jpeg",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Footnote {
    /// At least one paragraph; the first carries the [`RunContent::FootnoteMark`].
    pub body: Blocks,
}

/// A table: its grid, rows and cells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub props: TableProps,
    /// Column widths.
    pub grid: Vec<Twips>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableProps {
    pub style: Option<String>,
    pub width: Option<Twips>,
    pub indent: Option<Twips>,
    pub borders: Option<Borders>,
    /// The fixed layout, so Word keeps the grid's widths.
    pub fixed: bool,
    pub cell_margins: Option<CellMargins>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Border {
    /// In eighths of a point.
    pub size: u32,
    /// `RRGGBB`.
    pub color: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellMargins {
    pub top: Twips,
    pub left: Twips,
    pub bottom: Twips,
    pub right: Twips,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub props: RowProps,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowProps {
    pub cant_split: bool,
    /// The row repeats at the top of each page the table continues on.
    pub header: bool,
    pub min_height: Option<Twips>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cell {
    pub props: CellProps,
    pub body: Blocks,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellProps {
    pub width: Option<Twips>,
    pub span: u32,
    pub merge: Option<VMerge>,
    pub borders: Option<Borders>,
    /// `RRGGBB`.
    pub fill: Option<String>,
    pub no_wrap: bool,
    pub margins: Option<CellMargins>,
    pub valign: Option<VAlign>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    Restart,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

/// `styles.xml`: document defaults and named styles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Styles {
    pub default_run: RunProps,
    pub default_par: ParProps,
    pub styles: Vec<Style>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub id: String,
    /// A built-in style is named as Word names it (`heading 1`, `toc 2`) and
    /// written without `customStyle`.
    pub name: String,
    pub kind: StyleKind,
    pub builtin: bool,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub ui_priority: Option<u32>,
    pub semi_hidden: bool,
    pub quick: bool,
    /// A style names its parent in `based_on`, so `par.style`, `par.mark` and
    /// `run.style` are not written here.
    pub par: ParProps,
    pub run: RunProps,
}

impl Style {
    pub fn paragraph(id: impl Into<String>, name: impl Into<String>, builtin: bool) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: StyleKind::Paragraph,
            builtin,
            based_on: None,
            next: None,
            ui_priority: None,
            semi_hidden: false,
            quick: false,
            par: ParProps::default(),
            run: RunProps::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    Paragraph,
    Character,
}

/// `numbering.xml`: list schemes and their instances.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Numbering {
    pub abstracts: Vec<AbstractNum>,
    pub nums: Vec<Num>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AbstractNum {
    pub id: u32,
    /// Eight hex digits.
    pub nsid: String,
    /// Up to nine, level 0 first.
    pub levels: Vec<Level>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub start: u32,
    pub format: NumFormat,
    pub style: Option<String>,
    pub suffix: LevelSuffix,
    /// `%1`, `%2` .. stand for the levels' counters.
    pub text: String,
    pub align: Align,
    pub indent: Indent,
    pub tabs: Vec<TabStop>,
    pub run: RunProps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelSuffix {
    Tab,
    Space,
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumFormat {
    Decimal,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    Bullet,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Num {
    pub id: u32,
    pub abstract_id: u32,
    /// `(level, start)` pairs.
    pub starts: Vec<(u8, u32)>,
}

/// A face named in `fontTable.xml`.
#[derive(Debug, Clone, PartialEq)]
pub struct FontDecl {
    pub name: String,
    pub alt_name: Option<String>,
    pub family: FontFamily,
    pub pitch_fixed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Roman,
    Swiss,
    Modern,
    Auto,
}
