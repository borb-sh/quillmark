//! The reader: a laid-out Typst document to the Layout IR.
//!
//! One walk over the pages in paint order. A start tag of an element the IR
//! names opens a node under the innermost open one, its end tag closes it,
//! and ink joins the innermost open node. A bracket probe opens and closes a
//! node the same way; the par probe gives the paragraph it opens its style.
//! Inside a barrier (an artifact, or a group [`relocated`] from its anchor)
//! the flow's open nodes do not apply and are restored at its end; a
//! repeated table cell and a footnote's reference mark drop their ink.
//!
//! A post-pass then makes each run of inline children of a non-inline node a
//! paragraph, forms each paragraph's lines once, and measures frames and
//! table columns.

use std::collections::HashMap;

use quillmark_docx::layout::{
    ArtifactKind, BracketHint, Child, DocInfo, FontFace, FontId, Geometry, HAlign, HintLevel,
    ImageData, ImageFormat, ImageId, Item, ItemKind, ItemRef, LayoutDoc, Line, LineEnd, Marker,
    Node, NodeId, NodeKind, NumberFormat, Page, PageSetup, ParStyle, Parity, PointHint, Rect, Rgb,
    Shape, Sides, TextRun, VAlign,
};
use quillmark_docx::notice::{Code, Tally};
use typst::World;
use typst::foundations::{Content, Dict, Packed, Smart, StyleChain, Value};
use typst::introspection::{Location, Tag};
use typst::layout::{
    Abs, Alignment, Frame, FrameItem, GroupItem, HAlignment, Length, Point, Sides as TSides, Size,
    Transform,
};
use typst::layout::{GridCell, GridElem, HideElem, PlaceElem, Sizing, VAlignment};
use typst::math::EquationElem;
use typst::model::{
    Destination, EmphElem, EnumElem, FigureCaption, FigureElem, FootnoteElem, HeadingElem,
    LinkElem, LinkTarget, ListElem, ListMarker, Numbering, OutlineElem, OutlineEntry, ParElem,
    QuoteElem, StrongElem, TableCell, TableElem,
};
use typst::pdf::{ArtifactElem, PdfMarkerTag, PdfMarkerTagKind, TableCellKind};
use typst::text::{
    HighlightElem, RawElem, RawLine, StandardAxes, StrikeElem, SubElem, SuperElem, UnderlineElem,
};
use typst::visualize::{ExchangeFormat, ImageKind, Paint, RasterFormat};
use typst_layout::{PagedDocument, PagedIntrospector};

use crate::tags::{marker, relocated};

const HINT: &str = "__qm_hint__";

/// Typst's text size where nothing sets one.
const DEFAULT_TEXT_SIZE: f64 = 11.0;

/// The setup of a page with no page probe: 1 in margins.
const ASSUMED_SETUP: PageSetup = PageSetup {
    margins: Sides {
        top: 72.0,
        right: 72.0,
        bottom: 72.0,
        left: 72.0,
    },
    columns: 1,
    gutter: 0.0,
    flipped: false,
    header_ascent: 0.3 * 72.0,
    footer_descent: 0.3 * 72.0,
};

/// The page probe's record in `frame`.
fn page_probe(frame: &Frame) -> Option<Dict> {
    frame.items().find_map(|(_, item)| match item {
        FrameItem::Group(g) => page_probe(&g.frame),
        FrameItem::Tag(Tag::Start(elem, _)) => marker(elem, HINT)
            .filter(|d| matches!(d.get("hint"), Ok(Value::Str(h)) if h.as_str() == "page")),
        _ => None,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Key {
    Elem(Location),
    Bracket(Bracket),
}

/// A bracket a hint or probe opens and closes by name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bracket {
    List,
    Label,
    Role,
    Layout,
    Chrome,
    Repeat,
    Block,
    Align,
    Columns,
    Box,
}

impl Bracket {
    fn named(name: &str) -> Option<Bracket> {
        Some(match name {
            "list" => Bracket::List,
            "label" => Bracket::Label,
            "role" => Bracket::Role,
            "layout" => Bracket::Layout,
            "chrome" => Bracket::Chrome,
            "repeat" => Bracket::Repeat,
            "block" => Bracket::Block,
            "align" => Bracket::Align,
            "columns" => Bracket::Columns,
            "box" => Bracket::Box,
            _ => return None,
        })
    }

    /// A quill's own hint, which closes only under the element it opened in.
    fn quill(self) -> bool {
        matches!(
            self,
            Bracket::List | Bracket::Label | Bracket::Role | Bracket::Layout | Bracket::Chrome
        )
    }
}

#[derive(Clone, Copy)]
struct Open {
    key: Key,
    /// `None` drops the ink inside.
    node: Option<NodeId>,
    hidden: bool,
}

struct Reader<'a> {
    flow: &'a PagedIntrospector,
    /// The sources an equation's alt text is taken from.
    world: &'a dyn World,
    out: LayoutDoc,
    stack: Vec<Open>,
    saved: Vec<(Location, Vec<Open>)>,
    by_loc: HashMap<Location, NodeId>,
    fonts: HashMap<(String, bool, bool), FontId>,
    images: HashMap<u128, ImageId>,
    origins: HashMap<NodeId, (u32, Point)>,
    page: u32,
    /// The last page record on this page, and the page number text after
    /// it. The background's record comes first and the foreground's last,
    /// and only the foreground sees a page counter update the page's body
    /// makes.
    probe: Option<Dict>,
    number_text: Option<String>,
    /// A par or block node's text edges, resolved against the face of its
    /// first text.
    edges: HashMap<NodeId, (Value, Value)>,
    /// A table node's column sizes and the text size an `em` resolves at.
    sizings: HashMap<NodeId, (Vec<Sizing>, f64)>,
    /// Inside ink already carried as a picture: tags still open and close
    /// nodes, and nothing else is read.
    in_raster: u32,
    /// The outline entry whose leader has closed, and the node holding the
    /// page number after it.
    toc_led: Option<(NodeId, Option<NodeId>)>,
    /// The text size the page probe records for the page being read, which
    /// it paints last.
    page_text: f64,
    /// The equations carried as pictures whose end tag is still to come.
    equations: Vec<Location>,
    /// A display equation's picture, until the block that shows it opens.
    held: Option<ItemKind>,
    tally: Tally,
}

/// The Layout IR of `document`, compiled from `world`'s sources.
pub(crate) fn read(
    document: &PagedDocument,
    world: &dyn World,
    title: Option<String>,
) -> LayoutDoc {
    let mut r = Reader {
        flow: document.introspector(),
        world,
        out: LayoutDoc {
            info: DocInfo {
                title,
                language: None,
            },
            ..LayoutDoc::default()
        },
        stack: Vec::new(),
        saved: Vec::new(),
        by_loc: HashMap::new(),
        fonts: HashMap::new(),
        images: HashMap::new(),
        origins: HashMap::new(),
        page: 0,
        probe: None,
        number_text: None,
        edges: HashMap::new(),
        sizings: HashMap::new(),
        in_raster: 0,
        toc_led: None,
        page_text: DEFAULT_TEXT_SIZE,
        equations: Vec::new(),
        held: None,
        tally: Tally::default(),
    };
    for (i, page) in document.pages().iter().enumerate() {
        r.page = i as u32;
        r.probe = None;
        r.number_text = None;
        r.page_text = page_probe(&page.frame)
            .and_then(|d| length(&d.get("size").ok()?.clone(), 0.0))
            .unwrap_or(DEFAULT_TEXT_SIZE);
        let size = [page.frame.width().to_pt(), page.frame.height().to_pt()];
        r.out.pages.push(Page {
            size,
            setup: ASSUMED_SETUP,
            number: page.number as u64,
            number_text: None,
            items: Vec::new(),
        });
        r.walk(&page.frame, Transform::identity());
        let setup = r.probe.as_ref().and_then(|d| page_setup(d, size));
        if setup.is_none() {
            r.tally.count(Code::MarginsAssumed, "", 1);
        }
        let p = &mut r.out.pages[i];
        p.setup = setup.unwrap_or(ASSUMED_SETUP);
        p.number_text = r.number_text.take();
    }
    r.out.notices = r.tally.notices();
    let mut out = r.out;
    let origins = r.origins;
    let sizings = std::mem::take(&mut r.sizings);
    hoist_breaks(&mut out);
    form_paragraphs(&mut out);
    form_lines(&mut out);
    measure_frames(&mut out);
    measure_tables(&mut out, &origins, &sizings);
    out
}

/// The `Cell` node of a table or grid cell, whose elements share these
/// fields.
macro_rules! cell_kind {
    ($cell:expr, $header:expr, $size:expr) => {{
        let (fill, inset, borders) = cell_style(
            $cell.inset.get(StyleChain::default()),
            $cell.fill.get_ref(StyleChain::default()),
            &$cell.stroke.get_cloned(StyleChain::default()),
            $size,
        );
        NodeKind::Cell {
            col: smart_index($cell.x.get(StyleChain::default())),
            row: smart_index($cell.y.get(StyleChain::default())),
            colspan: $cell.colspan.get(StyleChain::default()).get() as u32,
            rowspan: $cell.rowspan.get(StyleChain::default()).get() as u32,
            header: $header,
            fill,
            inset,
            borders,
        }
    }};
}

impl Reader<'_> {
    fn top(&self) -> Option<NodeId> {
        match self.stack.last() {
            Some(o) => o.node,
            None => Some(0),
        }
    }

    fn hidden(&self) -> bool {
        self.stack.last().is_some_and(|o| o.hidden)
    }

    fn add_node(&mut self, kind: NodeKind) -> NodeId {
        let parent = self.top().unwrap_or(0);
        self.add_node_under(parent, kind)
    }

    fn add_node_under(&mut self, parent: NodeId, kind: NodeKind) -> NodeId {
        let id = self.out.nodes.len();
        self.out.nodes.push(Node::new(Some(parent), kind));
        self.out.nodes[parent].children.push(Child::Node(id));
        id
    }

    fn push(&mut self, key: Key, node: Option<NodeId>, hide: bool) {
        let hidden = hide || self.hidden();
        self.stack.push(Open { key, node, hidden });
    }

    fn close(&mut self, key: Key) {
        if let Some(i) = self.stack.iter().rposition(|o| o.key == key) {
            let open = self.stack[i + 1..]
                .iter()
                .filter(|o| matches!(o.key, Key::Bracket(k) if k.quill()))
                .count();
            if open > 0 {
                self.tally.raise(Code::HintUnclosed, "");
            }
            self.stack.truncate(i);
        }
    }

    /// Closes the quill bracket `kind` opened under the innermost open
    /// element; a close with no such open is dropped.
    fn close_hint(&mut self, kind: Bracket) {
        for i in (0..self.stack.len()).rev() {
            match self.stack[i].key {
                Key::Bracket(k) if k == kind => {
                    self.stack.truncate(i);
                    return;
                }
                Key::Elem(_) => return,
                Key::Bracket(_) => {}
            }
        }
    }

    fn toc_entry(&self) -> Option<NodeId> {
        self.stack
            .iter()
            .rev()
            .filter_map(|o| o.node)
            .find(|&n| matches!(self.out.nodes[n].kind, NodeKind::TocEntry { .. }))
    }

    /// The node an outline entry's page number goes in: everything the entry
    /// prints after its leader closes.
    fn toc_page(&mut self) -> Option<NodeId> {
        let (entry, page) = self.toc_led?;
        if self.toc_entry() != Some(entry) {
            self.toc_led = None;
            return None;
        }
        if let Some(page) = page {
            return Some(page);
        }
        let page = self.add_node_under(self.top()?, NodeKind::TocPage);
        self.toc_led = Some((entry, Some(page)));
        Some(page)
    }

    fn ink(&mut self, kind: ItemKind) {
        let Some(node) = self.top() else { return };
        self.ink_under(node, kind);
    }

    fn ink_under(&mut self, node: NodeId, kind: ItemKind) {
        if self.in_raster == 0 {
            self.add_item(node, kind);
        }
    }

    fn add_item(&mut self, node: NodeId, kind: ItemKind) {
        let page = &mut self.out.pages[self.page as usize];
        let at = ItemRef {
            page: self.page,
            index: page.items.len() as u32,
        };
        page.items.push(Item { node, kind });
        self.out.nodes[node].children.push(Child::Item(at));
    }

    fn image(&mut self, format: ImageFormat, bytes: &[u8]) -> ImageId {
        let hash = typst::utils::hash128(bytes);
        let next = self.out.images.len();
        let id = *self.images.entry(hash).or_insert(next);
        if id == next {
            self.out.images.push(ImageData {
                format,
                bytes: bytes.to_vec(),
            });
        }
        id
    }

    /// A picture of `item`'s ink in place: whether one was made, or `None`
    /// when no ink of it shows on the page.
    fn raster(&mut self, item: &FrameItem, pos: Point, ts: Transform) -> Option<bool> {
        let made = self.picture(item, pos, ts, None, None)?;
        Some(made.map(|kind| self.ink(kind)).is_some())
    }

    /// A picture of `item`'s ink on the page, at 300 dpi or at the density
    /// that keeps its longer side within a PNG render's, and across `span`
    /// when one is given: `None` when no ink of it shows on the page, and
    /// `Some(None)` when it could not be drawn.
    fn picture(
        &mut self,
        item: &FrameItem,
        pos: Point,
        ts: Transform,
        alt: Option<String>,
        span: Option<(f64, f64)>,
    ) -> Option<Option<ItemKind>> {
        if self.top().is_none() || self.hidden() {
            return None;
        }
        let [w, h] = self.out.pages[self.page as usize].size;
        let rect = ink_box(item, pos, ts).map(|r| {
            let (x0, x1) = span.map_or((r.x0, r.x1), |(a, b)| (a.min(r.x0), b.max(r.x1)));
            Rect {
                x0: x0.max(0.0),
                y0: r.y0.max(0.0),
                x1: x1.min(w),
                y1: r.y1.min(h),
            }
        })?;
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let mut inner = Frame::soft(Size::zero());
        inner.push(pos, item.clone());
        let mut group = GroupItem::new(inner);
        group.transform = Transform::translate(Abs::pt(-rect.x0), Abs::pt(-rect.y0)).pre_concat(ts);
        let mut frame = Frame::hard(Size::new(Abs::pt(rect.width()), Abs::pt(rect.height())));
        frame.push(Point::zero(), FrameItem::Group(group));
        let page = typst_layout::Page {
            frame,
            bleed: TSides::default(),
            fill: Smart::Custom(None),
            numbering: None,
            supplement: Content::empty(),
            number: 1,
        };
        let side = f64::from(quillmark_core::backend::MAX_RASTER_SIDE);
        let scale = (300.0_f64 / 72.0).min(side / rect.width().max(rect.height()));
        let pixmap = typst_render::render(&page, &crate::compile::render_options(scale as f32));
        let Ok(png) = pixmap.encode_png() else {
            return Some(None);
        };
        let image = self.image(ImageFormat::Png, &png);
        self.tally.count(Code::Rasterized, "", 1);
        Some(Some(ItemKind::Raster { image, rect, alt }))
    }

    /// Carries the equation `eq` as one picture of everything painted
    /// between its start tag at `at` and its end tag in `rest`, as wide as
    /// its advance when it is inline. A display equation's picture is held
    /// for the block that shows it. An equation whose end tag lies in
    /// another frame is left to be read as text.
    fn equation(
        &mut self,
        eq: &Packed<EquationElem>,
        at: Point,
        rest: &[(Point, FrameItem)],
        ts: Transform,
    ) {
        if self.in_raster > 0 || self.top().is_none() || self.hidden() {
            return;
        }
        let Some(loc) = eq.location() else { return };
        let end = rest
            .iter()
            .position(|(_, item)| matches!(item, FrameItem::Tag(Tag::End(l, ..)) if *l == loc));
        let Some(end) = end else {
            self.tally.count(Code::EquationText, "", 1);
            return;
        };
        let mut inner = Frame::soft(Size::zero());
        for (pos, item) in &rest[..end] {
            inner.push(*pos, item.clone());
        }
        let group = FrameItem::Group(GroupItem::new(inner));
        let span = (!eq.block.get(StyleChain::default())).then(|| {
            (
                at.transform(ts).x.to_pt(),
                rest[end].0.transform(ts).x.to_pt(),
            )
        });
        let source = eq.span().id().and_then(|id| {
            let source = self.world.source(id).ok()?;
            let range = source.find(eq.span())?.range();
            Some(source.text()[range].trim_matches('$').trim().to_string())
        });
        let alt = eq
            .alt
            .get_cloned(StyleChain::default())
            .map(|a| a.to_string())
            .or(source)
            .unwrap_or_else(|| eq.body.plain_text().to_string());
        match self.picture(&group, Point::zero(), ts, Some(alt), span) {
            Some(Some(kind)) => {
                match span {
                    Some(_) => self.ink(kind),
                    None => self.held = Some(kind),
                }
                self.in_raster += 1;
                self.equations.push(loc);
            }
            Some(None) => self.tally.count(Code::PictureFailed, "", 1),
            None => {}
        }
    }

    /// Places the held picture of a display equation in the innermost open
    /// node. The walk is inside that picture then, so the raster guard of
    /// [`Self::ink_under`] does not apply.
    fn place_held(&mut self) {
        if let (Some(kind), Some(node)) = (self.held.take(), self.top()) {
            self.add_item(node, kind);
        }
    }

    fn walk(&mut self, frame: &Frame, ts: Transform) {
        let items = frame.items().as_slice();
        for (i, (pos, item)) in items.iter().enumerate() {
            match item {
                FrameItem::Group(group) => {
                    let raster = self.in_raster == 0
                        && needs_raster(group, ts)
                        && match self.raster(item, *pos, ts) {
                            Some(false) => {
                                self.tally.count(Code::PictureFailed, "", 1);
                                false
                            }
                            _ => true,
                        };
                    if raster {
                        self.in_raster += 1;
                    }
                    let ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    let barrier = group
                        .parent
                        .map(|p| p.location)
                        .filter(|&l| relocated(self.flow, l));
                    match barrier {
                        Some(loc) => {
                            let node = self.by_loc.get(&loc).copied().unwrap_or(0);
                            let outer = std::mem::replace(
                                &mut self.stack,
                                vec![Open {
                                    key: Key::Elem(loc),
                                    node: Some(node),
                                    hidden: false,
                                }],
                            );
                            self.walk(&group.frame, ts);
                            self.stack = outer;
                        }
                        None => self.walk(&group.frame, ts),
                    }
                    if raster {
                        self.in_raster -= 1;
                    }
                }
                FrameItem::Tag(Tag::Start(elem, _)) => {
                    if let Some(eq) = elem.to_packed::<EquationElem>() {
                        self.equation(eq, *pos, &items[i + 1..], ts);
                    }
                    let open = self.stack.len();
                    self.start(elem, pos.transform(ts));
                    if self.held.is_some() && self.stack.len() > open {
                        self.place_held();
                    }
                }
                FrameItem::Tag(Tag::End(loc, ..)) => {
                    if self.equations.last() == Some(loc) {
                        self.place_held();
                        self.equations.pop();
                        self.in_raster -= 1;
                    }
                    if let Some(i) = self.saved.iter().rposition(|(l, _)| l == loc) {
                        self.stack = self.saved.remove(i).1;
                        self.saved.truncate(i);
                    } else {
                        self.close(Key::Elem(*loc));
                    }
                }
                FrameItem::Text(text) => {
                    if self.top().is_none() || self.hidden() {
                        continue;
                    }
                    let info = text.font.info();
                    let axis = |tag| {
                        text.font
                            .variations()
                            .0
                            .iter()
                            .find(|(t, _)| *t == tag)
                            .map(|(_, v)| v.0 as f64)
                    };
                    let weight =
                        axis(StandardAxes::WGHT).unwrap_or(info.variant.weight.to_number() as f64);
                    let bold = weight >= 600.0;
                    let italic = axis(StandardAxes::ITAL).map_or_else(
                        || {
                            axis(StandardAxes::SLNT).map_or(
                                !matches!(info.variant.style, typst::text::FontStyle::Normal),
                                |s| s != 0.0,
                            )
                        },
                        |i| i >= 0.5,
                    );
                    let family = info.family.clone();
                    let monospace = info.flags.contains(typst::text::FontFlags::MONOSPACE);
                    let next = self.out.fonts.len();
                    let font = *self
                        .fonts
                        .entry((family.clone(), bold, italic))
                        .or_insert(next);
                    if font == next {
                        self.out.fonts.push(FontFace {
                            family,
                            bold,
                            italic,
                            monospace,
                        });
                    }
                    if !self.edges.is_empty() {
                        self.resolve_edges(text);
                    }
                    let scale = ts.sy.get().abs();
                    let size = text.size.to_pt() * scale;
                    let width: f64 = text
                        .glyphs
                        .iter()
                        .map(|g| g.x_advance.at(text.size).to_pt())
                        .sum::<f64>()
                        * scale;
                    let tracking = tracking(text);
                    let natural: f64 = (text
                        .glyphs
                        .iter()
                        .map(|g| {
                            text.font
                                .x_advance(g.id)
                                .unwrap_or(g.x_advance)
                                .at(text.size)
                                .to_pt()
                        })
                        .sum::<f64>()
                        + tracking.unwrap_or(0.0) * text.glyphs.len().saturating_sub(1) as f64)
                        * scale;
                    let origin = pos.transform(ts);
                    let run = ItemKind::Text(TextRun {
                        origin: [origin.x.to_pt(), origin.y.to_pt()],
                        text: text.text.to_string(),
                        font,
                        size,
                        fill: rgb(&text.fill),
                        width,
                        natural,
                        tracking: tracking.map(|t| t * scale),
                        lang: Some(match &text.region {
                            Some(region) => format!("{}-{}", text.lang.as_str(), region.as_str()),
                            None => text.lang.as_str().to_string(),
                        }),
                    });
                    match self.toc_page() {
                        Some(page) => self.ink_under(page, run),
                        None => self.ink(run),
                    }
                }
                FrameItem::Shape(shape, _) => {
                    if self.top().is_none() || self.hidden() || self.in_raster > 0 {
                        continue;
                    }
                    let flat = |p: &Paint| matches!(p, Paint::Solid(_));
                    if matches!(shape.geometry, typst::visualize::Geometry::Curve(_))
                        || shape.fill.as_ref().is_some_and(|p| !flat(p))
                        || shape.stroke.as_ref().is_some_and(|s| !flat(&s.paint))
                    {
                        self.raster(item, *pos, ts);
                        continue;
                    }
                    let bb = shape.geometry.bbox(shape.stroke.as_ref());
                    let rect = transformed(*pos, bb.min, bb.max, ts);
                    let geometry = match shape.geometry {
                        typst::visualize::Geometry::Line(to) => Geometry::Line {
                            rising: (to.x.to_pt() > 0.0) != (to.y.to_pt() > 0.0)
                                && to.y.to_pt() != 0.0,
                        },
                        _ => Geometry::Rect,
                    };
                    self.ink(ItemKind::Shape(Shape {
                        rect,
                        geometry,
                        fill: shape.fill.as_ref().map(rgb),
                        stroke: shape
                            .stroke
                            .as_ref()
                            .map(|s| (s.thickness.to_pt(), rgb(&s.paint))),
                    }));
                }
                FrameItem::Image(image, size, _) => {
                    if self.top().is_none() || self.hidden() {
                        continue;
                    }
                    if self.in_raster > 0 {
                        continue;
                    }
                    let format = match image.kind() {
                        ImageKind::Raster(raster) => match raster.format() {
                            RasterFormat::Exchange(ExchangeFormat::Png) => {
                                Some((ImageFormat::Png, raster))
                            }
                            RasterFormat::Exchange(ExchangeFormat::Jpg) => {
                                Some((ImageFormat::Jpeg, raster))
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some((format, raster)) = format else {
                        self.raster(item, *pos, ts);
                        continue;
                    };
                    let id = self.image(format, raster.data());
                    let rect = transformed(*pos, Point::zero(), size.to_point(), ts);
                    self.ink(ItemKind::Image {
                        image: id,
                        rect,
                        alt: image.alt().map(str::to_string),
                    });
                }
                _ => {}
            }
        }
    }

    fn add_cell(&mut self, kind: NodeKind, loc: Location, at: Point) {
        let node = self.add_node(kind);
        self.origins.insert(node, (self.page, at));
        self.by_loc.insert(loc, node);
        self.push(Key::Elem(loc), Some(node), false);
    }

    fn start(&mut self, elem: &Content, at: Point) {
        if let Some(d) = marker(elem, HINT) {
            if !self.hidden() {
                self.hint(&d, at);
            }
            return;
        }
        let Some(loc) = elem.location() else { return };
        if let Some(artifact) = elem.to_packed::<ArtifactElem>() {
            let kind = match artifact.kind.get(StyleChain::default()) {
                typst::pdf::ArtifactKind::Header => ArtifactKind::Header,
                typst::pdf::ArtifactKind::Footer => ArtifactKind::Footer,
                typst::pdf::ArtifactKind::Watermark => ArtifactKind::Background,
                typst::pdf::ArtifactKind::PageNumber => ArtifactKind::PageNumber,
                _ => ArtifactKind::Other,
            };
            let node = self.add_node_under(0, NodeKind::Artifact { kind });
            let outer = std::mem::take(&mut self.stack);
            self.saved.push((loc, outer));
            self.push(Key::Elem(loc), Some(node), false);
            return;
        }
        if elem.is::<HideElem>() {
            let node = self.top();
            self.push(Key::Elem(loc), node, true);
            return;
        }
        if elem.is::<FootnoteElem>() {
            let note = self.add_node_under(0, NodeKind::Footnote);
            self.by_loc.insert(loc, note);
            if !self.hidden() {
                self.ink(ItemKind::Mark(PointHint::FootnoteRef { note }));
            }
            self.push(Key::Elem(loc), None, false);
            return;
        }
        if let Some(cell) = elem.to_packed::<TableCell>() {
            if cell.is_repeated.get(StyleChain::default()) {
                self.push(Key::Elem(loc), None, false);
                return;
            }
            let header = matches!(
                cell.kind.get(StyleChain::default()),
                typst::foundations::Smart::Custom(TableCellKind::Header(..))
            );
            let kind = cell_kind!(cell, header, self.text_size());
            self.add_cell(kind, loc, at);
            return;
        }
        if let Some(cell) = elem.to_packed::<GridCell>() {
            let kind = cell_kind!(cell, false, self.text_size());
            self.add_cell(kind, loc, at);
            return;
        }
        if let Some(m) = elem.to_packed::<PdfMarkerTag>() {
            if matches!(m.kind, PdfMarkerTagKind::Label)
                && self
                    .top()
                    .is_some_and(|n| matches!(self.out.nodes[n].kind, NodeKind::Footnote))
            {
                let node = self.add_node(NodeKind::ListLabel);
                self.push(Key::Elem(loc), Some(node), false);
                return;
            }
        }
        if elem
            .to_packed::<LinkElem>()
            .is_some_and(|link| !matches!(link.dest, LinkTarget::Dest(Destination::Url(_))))
            && self.top().is_some()
            && self.in_raster == 0
            && !self.hidden()
            && self.toc_entry().is_none()
        {
            self.tally.count(Code::LinkInternal, "", 1);
        }
        if elem.is::<RawLine>() {
            let in_raw = self
                .stack
                .iter()
                .filter_map(|o| o.node)
                .any(|n| matches!(self.out.nodes[n].kind, NodeKind::Raw));
            if in_raw && self.top().is_some() {
                let node = self.add_node(NodeKind::RawLine);
                self.push(Key::Elem(loc), Some(node), false);
            }
            return;
        }
        let list = self.list_kind(elem);
        let Some(kind) = list.or_else(|| node_kind(elem)) else {
            return;
        };
        if self.top().is_none() {
            self.push(Key::Elem(loc), None, false);
            return;
        }
        let label = match self.stack.last() {
            Some(Open {
                key: Key::Bracket(Bracket::Label),
                node: Some(n),
                ..
            }) if matches!(kind, NodeKind::Par { .. })
                && self.out.nodes[*n].children.is_empty() =>
            {
                self.stack.pop()
            }
            _ => None,
        };
        let node = self.add_node(kind);
        if let Some(open) = label {
            // Typst tags a paragraph after the inline tags it starts with, so a
            // label that opens a paragraph moves inside it.
            let n = open.node.expect("a label node");
            let parent = self.out.nodes[n].parent.expect("a parent");
            self.out.nodes[parent]
                .children
                .retain(|c| *c != Child::Node(n));
            self.out.nodes[n].parent = Some(node);
            self.out.nodes[node].children.push(Child::Node(n));
            self.by_loc.insert(loc, node);
            self.push(Key::Elem(loc), Some(node), false);
            self.stack.push(open);
            return;
        }
        let sizing = elem
            .to_packed::<TableElem>()
            .map(|t| t.columns.get_ref(StyleChain::default()).0.to_vec())
            .or_else(|| {
                elem.to_packed::<GridElem>()
                    .map(|g| g.columns.get_ref(StyleChain::default()).0.to_vec())
            });
        if let Some(sizing) = sizing {
            let size = self.text_size();
            self.sizings.insert(node, (sizing, size));
        }
        self.by_loc.insert(loc, node);
        self.push(Key::Elem(loc), Some(node), false);
    }

    /// The text size the innermost block or paragraph probe recorded, else
    /// the page's.
    fn text_size(&self) -> f64 {
        self.stack
            .iter()
            .rev()
            .filter_map(|o| o.node)
            .find_map(|n| match &self.out.nodes[n].kind {
                NodeKind::Block { style, .. } => Some(style.size),
                NodeKind::Par { style: Some(s), .. } => Some(s.size),
                _ => None,
            })
            .unwrap_or(self.page_text)
    }

    /// The text edges of the open par and block nodes still waiting for a
    /// face, from `text`'s.
    fn resolve_edges(&mut self, text: &typst::text::TextItem) {
        let open: Vec<NodeId> = self.stack.iter().filter_map(|o| o.node).collect();
        for n in open {
            let Some((top, bottom)) = self.edges.remove(&n) else {
                continue;
            };
            let m = text.font.metrics();
            let size = text.size.to_pt();
            let edge = |v: &Value, above: bool| -> f64 {
                let em = |e: typst::layout::Em| e.get() * size;
                match v {
                    Value::Str(s) => match s.as_str() {
                        "ascender" | "bounds" if above => em(m.ascender),
                        "cap-height" => em(m.cap_height),
                        "x-height" => em(m.x_height),
                        "descender" | "bounds" => -em(m.descender),
                        _ => 0.0,
                    },
                    v => length(v, size).unwrap_or(0.0),
                }
            };
            let (t, b) = (edge(&top, true), edge(&bottom, false));
            match &mut self.out.nodes[n].kind {
                NodeKind::Par { style: Some(s), .. } | NodeKind::Block { style: s, .. } => {
                    s.top_edge = t;
                    s.bottom_edge = b;
                }
                _ => {}
            }
        }
    }

    /// How many lists of the kind `pred` accepts enclose the walk.
    fn depth(&self, pred: impl Fn(&Marker) -> bool) -> usize {
        self.stack
            .iter()
            .filter_map(|o| o.node)
            .filter(|&n| matches!(&self.out.nodes[n].kind, NodeKind::List { marker, .. } if pred(marker)))
            .count()
    }

    /// A list's marker from its own fields at its nesting depth.
    fn list_kind(&mut self, elem: &Content) -> Option<NodeKind> {
        let styles = StyleChain::default();
        let end = |a: Alignment| matches!(a.x(), Some(HAlignment::Right | HAlignment::End));
        if let Some(list) = elem.to_packed::<ListElem>() {
            let depth = self.depth(|m| matches!(m, Marker::Bullet(_)));
            let glyph = match list.marker.get_ref(styles) {
                ListMarker::Content(v) if !v.is_empty() => v[depth % v.len()].plain_text(),
                _ => Default::default(),
            };
            let glyph = if glyph.trim().is_empty() {
                self.tally.raise(Code::BulletReplaced, "");
                "\u{2022}".to_string()
            } else {
                glyph.trim().to_string()
            };
            return Some(NodeKind::List {
                marker: Marker::Bullet(glyph),
                label_end: end(list.marker_align.get(styles)),
            });
        }
        let list = elem.to_packed::<EnumElem>()?;
        let depth = self.depth(|m| !matches!(m, Marker::Bullet(_)));
        let start = match list.start.get(styles) {
            typst::foundations::Smart::Custom(n) => n,
            typst::foundations::Smart::Auto => 1,
        };
        let marker = match list.numbering.get_ref(styles) {
            Numbering::Pattern(p) if !list.reversed.get(styles) => {
                let format = |sys: &dyn Fn() -> Option<&'static str>| match sys() {
                    Some("1") => Some(NumberFormat::Decimal),
                    Some("a") => Some(NumberFormat::LowerLetter),
                    Some("A") => Some(NumberFormat::UpperLetter),
                    Some("i") => Some(NumberFormat::LowerRoman),
                    Some("I") => Some(NumberFormat::UpperRoman),
                    _ => None,
                };
                let piece = |k: usize| {
                    let (prefix, sys) = &p.pieces[k.min(p.pieces.len() - 1)];
                    Some((prefix.to_string(), format(&|| sys.shorthand())?))
                };
                let counters: Option<Vec<(String, NumberFormat)>> = if list.full.get(styles) {
                    (0..=depth).map(piece).collect()
                } else {
                    let (_, f) = piece(depth)?;
                    let prefix = p.pieces.first().map(|(s, _)| s.to_string());
                    Some(vec![(prefix.unwrap_or_default(), f)])
                };
                match counters {
                    Some(counters) if !p.pieces.is_empty() => Marker::Numbered {
                        counters,
                        suffix: p.suffix.to_string(),
                        start,
                    },
                    _ => Marker::Printed,
                }
            }
            _ => Marker::Printed,
        };
        if marker == Marker::Printed {
            self.tally.raise(Code::NumbersPrinted, "");
        }
        Some(NodeKind::List {
            marker,
            label_end: end(list.number_align.get(styles)),
        })
    }

    /// Reads the probe or hint `d`, whose tag sits at `at`.
    fn hint(&mut self, d: &Dict, at: Point) {
        let get = |k: &str| d.get(k).ok().cloned().unwrap_or(Value::None);
        let Value::Str(hint) = get("hint") else {
            return;
        };
        let size = length(&get("size"), 0.0).unwrap_or(11.0);
        let close = matches!(get("edge"), Value::Str(s) if s.as_str() == "close");
        match hint.as_str() {
            "page" => {
                self.probe = Some(d.clone());
                self.number_text = None;
            }
            "pnum" => {
                if let Value::Str(s) = get("s") {
                    self.number_text
                        .get_or_insert_with(String::new)
                        .push_str(s.as_str());
                }
            }
            "br" => self.ink(ItemKind::Mark(PointHint::Br)),
            "keep" => self.ink(ItemKind::Mark(PointHint::Keep)),
            "font" => {
                if let (Value::Str(family), Value::Str(alt)) = (get("family"), get("alt")) {
                    self.ink(ItemKind::Mark(PointHint::Font {
                        family: family.to_string(),
                        alt: alt.to_string(),
                    }));
                }
            }
            "list" | "label" | "role" | "layout" | "chrome" => {
                let kind = Bracket::named(hint.as_str()).expect("a quill bracket");
                if close {
                    self.close_hint(kind);
                    return;
                }
                let size = self.text_size();
                let text = |v: Value| match v {
                    Value::Str(s) => Some(s.to_string()),
                    _ => None,
                };
                let bracket = match kind {
                    Bracket::List => BracketHint::List {
                        name: text(get("name")).unwrap_or_else(|| "List".into()),
                        levels: match get("levels") {
                            Value::Array(a) => {
                                a.iter().filter_map(|l| hint_level(l, size)).collect()
                            }
                            _ => Vec::new(),
                        },
                    },
                    Bracket::Label => BracketHint::Label {
                        level: match get("level") {
                            Value::Int(n) => n.clamp(1, 9) as u8,
                            _ => 1,
                        },
                    },
                    Bracket::Role => BracketHint::Role {
                        name: text(get("name")).unwrap_or_default(),
                        next: text(get("next")),
                    },
                    Bracket::Layout => BracketHint::Tabbed,
                    _ => BracketHint::Chrome,
                };
                match self.top() {
                    Some(_) => {
                        let node = self.add_node(NodeKind::Hint(bracket));
                        self.push(Key::Bracket(kind), Some(node), false);
                    }
                    None => self.push(Key::Bracket(kind), None, false),
                }
            }
            "break" => {
                let to = match get("to") {
                    Value::Str(s) if s.as_str() == "odd" => Some(Parity::Odd),
                    Value::Str(s) if s.as_str() == "even" => Some(Parity::Even),
                    _ => None,
                };
                let weak = matches!(get("weak"), Value::Bool(true));
                self.ink(ItemKind::Mark(PointHint::Break { weak, to }));
            }
            "par" => {
                let style = ParStyle {
                    align: halign(&get("a")),
                    size,
                    leading: length(&get("leading"), size).unwrap_or(0.65 * size),
                    spacing: length(&get("spacing"), size).unwrap_or(1.2 * size),
                    top_edge: 0.0,
                    bottom_edge: 0.0,
                };
                let par = self.stack.iter().rev().find_map(|o| {
                    o.node
                        .filter(|&n| matches!(self.out.nodes[n].kind, NodeKind::Par { .. }))
                });
                if let Some(n) = par {
                    if let NodeKind::Par {
                        style: s @ None,
                        left,
                        ..
                    } = &mut self.out.nodes[n].kind
                    {
                        *s = Some(style);
                        *left = Some(at.x.to_pt());
                        self.edges.insert(n, (get("top"), get("bottom")));
                    }
                }
            }
            "repeat" => {
                let entry = self.toc_entry();
                if close {
                    self.close(Key::Bracket(Bracket::Repeat));
                    self.toc_led = entry.map(|e| (e, None));
                    return;
                }
                let node = match (entry, self.top()) {
                    (Some(_), Some(_)) => Some(self.add_node(NodeKind::TocFill)),
                    _ => None,
                };
                self.push(Key::Bracket(Bracket::Repeat), node, false);
            }
            "block" | "align" | "columns" | "box" => {
                let key = Key::Bracket(Bracket::named(hint.as_str()).expect("a probe bracket"));
                if close {
                    self.close(key);
                    return;
                }
                let kind = match hint.as_str() {
                    "block" => {
                        let spacing = length(&get("spacing"), size).unwrap_or(1.2 * size);
                        NodeKind::Block {
                            sticky: matches!(get("sticky"), Value::Bool(true)),
                            breakable: !matches!(get("breakable"), Value::Bool(false)),
                            above: length(&get("above"), size).unwrap_or(spacing),
                            below: length(&get("below"), size).unwrap_or(spacing),
                            style: ParStyle {
                                align: halign(&get("a")),
                                size,
                                leading: length(&get("leading"), size).unwrap_or(0.65 * size),
                                spacing,
                                top_edge: 0.0,
                                bottom_edge: 0.0,
                            },
                        }
                    }
                    "align" => NodeKind::Align {
                        h: halign(&get("a")),
                    },
                    "columns" => NodeKind::Columns {
                        count: match get("count") {
                            Value::Int(n) => n.max(1) as u32,
                            _ => 1,
                        },
                        gutter: length(&get("gutter"), size).unwrap_or(0.0),
                    },
                    _ => NodeKind::NoBreak,
                };
                match self.top() {
                    Some(_) => {
                        let block = matches!(kind, NodeKind::Block { .. });
                        let node = self.add_node(kind);
                        if block {
                            self.edges.insert(node, (get("top"), get("bottom")));
                        }
                        self.push(key, Some(node), false);
                    }
                    None => self.push(key, None, false),
                }
            }
            _ => self.tally.raise(Code::HintUnknown, hint.as_str()),
        }
    }
}

/// The brackets a quill emits, as against the probes'.

/// One level of a `list` hint; an `em` indent resolves at `size`.
fn hint_level(v: &Value, size: f64) -> Option<HintLevel> {
    let Value::Dict(d) = v else { return None };
    let get = |k: &str| d.get(k).ok().cloned().unwrap_or(Value::None);
    let text = |k: &str| match get(k) {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    };
    let format = match get("format") {
        Value::Str(s) => match s.as_str() {
            "lower-letter" => NumberFormat::LowerLetter,
            "upper-letter" => NumberFormat::UpperLetter,
            "lower-roman" => NumberFormat::LowerRoman,
            "upper-roman" => NumberFormat::UpperRoman,
            _ => NumberFormat::Decimal,
        },
        _ => NumberFormat::Decimal,
    };
    Some(HintLevel {
        format,
        prefix: text("prefix"),
        suffix: text("suffix"),
        underline: matches!(get("underline"), Value::Bool(true)),
        visible: !matches!(get("visible"), Value::Bool(false)),
        indent: length(&get("indent"), size),
    })
}

fn node_kind(elem: &Content) -> Option<NodeKind> {
    let styles = StyleChain::default();
    if let Some(par) = elem.to_packed::<ParElem>() {
        let ind = |v: Length| v.abs.to_pt();
        return Some(NodeKind::Par {
            justify: par.justify.get(styles),
            first_line: ind(par.first_line_indent.get(styles).amount()),
            hanging: ind(par.hanging_indent.get(styles)),
            style: None,
            left: None,
        });
    }
    if let Some(h) = elem.to_packed::<HeadingElem>() {
        let level = match h.level.get(styles) {
            typst::foundations::Smart::Custom(l) => l.get(),
            typst::foundations::Smart::Auto => h.depth.get(styles).get() + h.offset.get(styles),
        };
        return Some(NodeKind::Heading {
            level: level.min(9) as u8,
            outlined: h.outlined.get(styles),
            numbered: h.numbering.get_ref(styles).is_some(),
        });
    }
    if let Some(m) = elem.to_packed::<PdfMarkerTag>() {
        return match m.kind {
            PdfMarkerTagKind::ListItemLabel => Some(NodeKind::ListLabel),
            PdfMarkerTagKind::ListItemBody | PdfMarkerTagKind::TermsItemBody => {
                Some(NodeKind::ListBody)
            }
            _ => None,
        };
    }
    if let Some(link) = elem.to_packed::<LinkElem>() {
        return match &link.dest {
            LinkTarget::Dest(Destination::Url(url)) => Some(NodeKind::Link {
                url: url.as_str().to_string(),
            }),
            _ => None,
        };
    }
    if let Some(place) = elem.to_packed::<PlaceElem>() {
        let valign = match place.alignment.get(styles) {
            typst::foundations::Smart::Custom(a) => match a.y() {
                Some(VAlignment::Top) => Some(VAlign::Top),
                Some(VAlignment::Horizon) => Some(VAlign::Horizon),
                Some(VAlignment::Bottom) => Some(VAlign::Bottom),
                None => None,
            },
            typst::foundations::Smart::Auto => None,
        };
        return Some(NodeKind::Place {
            float: place.float.get(styles),
            valign,
        });
    }
    let kind = if elem.is::<StrongElem>() {
        NodeKind::Strong
    } else if elem.is::<EmphElem>() {
        NodeKind::Emph
    } else if elem.is::<UnderlineElem>() {
        NodeKind::Underline
    } else if elem.is::<StrikeElem>() {
        NodeKind::Strike
    } else if let Some(h) = elem.to_packed::<HighlightElem>() {
        NodeKind::Highlight {
            fill: h
                .fill
                .get_ref(styles)
                .as_ref()
                .map_or(Rgb(255, 255, 0), rgb),
        }
    } else if elem.is::<SubElem>() {
        NodeKind::Sub
    } else if elem.is::<SuperElem>() {
        NodeKind::Super
    } else if elem.is::<TableElem>() || elem.is::<GridElem>() {
        NodeKind::Table {
            columns: Vec::new(),
            content_sized: Vec::new(),
            rows: Vec::new(),
            header_rows: 0,
            grid: elem.is::<GridElem>(),
        }
    } else if let Some(o) = elem.to_packed::<OutlineElem>() {
        NodeKind::Toc {
            depth: o.depth.get(styles).map(|d| d.get().min(9) as u8),
        }
    } else if let Some(e) = elem.to_packed::<OutlineEntry>() {
        NodeKind::TocEntry {
            level: e.level.get().min(9) as u8,
        }
    } else if elem
        .to_packed::<QuoteElem>()
        .is_some_and(|q| q.block.get(styles))
    {
        NodeKind::Quote
    } else if elem
        .to_packed::<RawElem>()
        .is_some_and(|r| r.block.get(styles))
    {
        NodeKind::Raw
    } else if elem.is::<FigureElem>() {
        NodeKind::Figure
    } else if elem.is::<FigureCaption>() {
        NodeKind::Caption
    } else {
        return None;
    };
    Some(kind)
}

/// A group the IR cannot carry item by item: a rotation, a skew or an
/// unequal scale, or a clip that cuts its ink.
fn needs_raster(group: &GroupItem, ts: Transform) -> bool {
    let t = ts.pre_concat(group.transform);
    if t.kx.get().abs() > 1e-6
        || t.ky.get().abs() > 1e-6
        || t.sx.get() < 0.0
        || t.sy.get() < 0.0
        || (t.sx.get().abs() - t.sy.get().abs()).abs() > 1e-6
    {
        return true;
    }
    let Some(clip) = &group.clip else {
        return false;
    };
    let c = clip.bbox(None);
    let dy = 0.5_f64.max(0.25 * max_text_size(&group.frame));
    frame_box(&group.frame, Transform::identity()).is_some_and(|r| {
        r.x0 < c.min.x.to_pt() - 0.5
            || r.y0 < c.min.y.to_pt() - dy
            || r.x1 > c.max.x.to_pt() + 0.5
            || r.y1 > c.max.y.to_pt() + dy
    })
}

/// The extra advance `text` sets after each glyph, in points, where it is
/// unambiguous: more than half of the glyphs before the last, and at least
/// two, carry the same nonzero advance over the face's own. Kerning varies
/// by pair and justification widens only spaces, so neither reads as
/// tracking. The last glyph is left out: Typst sets none after a tracked
/// span ends.
fn tracking(text: &typst::text::TextItem) -> Option<f64> {
    let deltas: Vec<i64> = text.glyphs[..text.glyphs.len().saturating_sub(1)]
        .iter()
        .map(|g| {
            let own = text.font.x_advance(g.id).unwrap_or(g.x_advance);
            ((g.x_advance - own).at(text.size).to_pt() * 100.0).round() as i64
        })
        .collect();
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for &d in &deltas {
        *counts.entry(d).or_default() += 1;
    }
    let (&delta, &n) = counts.iter().max_by_key(|&(d, n)| (*n, -d.abs()))?;
    (delta != 0 && n >= 2 && 2 * n > deltas.len()).then(|| delta as f64 / 100.0)
}

/// The largest text size in `frame`, since glyphs rise past a box's text
/// edges and a clip at that edge cuts nothing a reader would miss.
fn max_text_size(frame: &Frame) -> f64 {
    frame
        .items()
        .map(|(_, item)| match item {
            FrameItem::Text(t) => t.size.to_pt(),
            FrameItem::Group(g) => max_text_size(&g.frame),
            _ => 0.0,
        })
        .fold(0.0, f64::max)
}

fn frame_box(frame: &Frame, ts: Transform) -> Option<Rect> {
    frame
        .items()
        .filter_map(|(pos, item)| ink_box(item, *pos, ts))
        .reduce(|a, b| a.union(&b))
}

/// The box `item` inks at `pos` under `ts`, clipped where a group clips.
fn ink_box(item: &FrameItem, pos: Point, ts: Transform) -> Option<Rect> {
    match item {
        FrameItem::Text(text) => {
            let face = text.font.ttf();
            let em = |v: i16| Abs::pt(v as f64 / face.units_per_em() as f64 * text.size.to_pt());
            let mut x = Abs::zero();
            let mut out: Option<Rect> = None;
            for g in &text.glyphs {
                let at = x + g.x_offset.at(text.size);
                if let Some(b) = face.glyph_bounding_box(ttf_parser::GlyphId(g.id)) {
                    let lo = Point::new(at + em(b.x_min), -em(b.y_max) - g.y_offset.at(text.size));
                    let hi = Point::new(at + em(b.x_max), -em(b.y_min) - g.y_offset.at(text.size));
                    let r = transformed(pos, lo, hi, ts);
                    out = Some(out.map_or(r, |o| o.union(&r)));
                }
                x += g.x_advance.at(text.size);
            }
            out
        }
        FrameItem::Shape(shape, _) => {
            let bb = shape.geometry.bbox(shape.stroke.as_ref());
            Some(transformed(pos, bb.min, bb.max, ts))
        }
        FrameItem::Image(_, size, _) => Some(transformed(pos, Point::zero(), size.to_point(), ts)),
        FrameItem::Group(group) => {
            let inner = ts
                .pre_concat(Transform::translate(pos.x, pos.y))
                .pre_concat(group.transform);
            let r = frame_box(&group.frame, inner)?;
            match &group.clip {
                Some(clip) => {
                    let c = clip.bbox(None);
                    let c = transformed(Point::zero(), c.min, c.max, inner);
                    let r = Rect {
                        x0: r.x0.max(c.x0),
                        y0: r.y0.max(c.y0),
                        x1: r.x1.min(c.x1),
                        y1: r.y1.min(c.y1),
                    };
                    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
                }
                None => Some(r),
            }
        }
        _ => None,
    }
}

fn smart_index(v: typst::foundations::Smart<usize>) -> u32 {
    match v {
        typst::foundations::Smart::Custom(i) => i as u32,
        typst::foundations::Smart::Auto => 0,
    }
}

fn rgb(paint: &Paint) -> Rgb {
    match paint {
        Paint::Solid(c) => {
            let [r, g, b, _] = c.to_vec4_u8();
            Rgb(r, g, b)
        }
        _ => Rgb(0, 0, 0),
    }
}

fn transformed(pos: Point, lo: Point, hi: Point, ts: Transform) -> Rect {
    let corners = [
        Point::new(lo.x, lo.y),
        Point::new(hi.x, lo.y),
        Point::new(lo.x, hi.y),
        Point::new(hi.x, hi.y),
    ];
    let mut r = Rect {
        x0: f64::MAX,
        y0: f64::MAX,
        x1: f64::MIN,
        y1: f64::MIN,
    };
    for c in corners {
        let p = (pos + c).transform(ts);
        r.x0 = r.x0.min(p.x.to_pt());
        r.y0 = r.y0.min(p.y.to_pt());
        r.x1 = r.x1.max(p.x.to_pt());
        r.y1 = r.y1.max(p.y.to_pt());
    }
    r
}

fn halign(v: &Value) -> HAlign {
    match v.clone().cast::<Alignment>().ok().and_then(Alignment::x) {
        Some(HAlignment::Center) => HAlign::Center,
        Some(HAlignment::Right | HAlignment::End) => HAlign::End,
        _ => HAlign::Start,
    }
}

/// An absolute length in points, an `em` resolved against `size`.
fn length(v: &Value, size: f64) -> Option<f64> {
    match v {
        Value::Length(l) => Some(l.abs.to_pt() + l.em.get() * size),
        Value::Relative(r) if r.rel.is_zero() => Some(r.abs.abs.to_pt() + r.abs.em.get() * size),
        _ => None,
    }
}

/// A page margin side: `auto` is Typst's 2.5/21 of the shorter side, a ratio
/// is of the page's extent along that side's axis.
fn margin_side(v: &Value, extent: f64, short: f64, size: f64) -> Option<f64> {
    match v {
        Value::Auto => Some(2.5 / 21.0 * short),
        Value::Ratio(r) => Some(r.get() * extent),
        Value::Relative(r) => {
            Some(r.rel.get() * extent + r.abs.abs.to_pt() + r.abs.em.get() * size)
        }
        v => length(v, size),
    }
}

fn page_setup(d: &Dict, size: [f64; 2]) -> Option<PageSetup> {
    let get = |k: &str| d.get(k).ok().cloned().unwrap_or(Value::None);
    let text = length(&get("size"), 0.0).unwrap_or(11.0);
    let short = size[0].min(size[1]);
    let margin = get("margin");
    let side = |keys: &[&str], extent: f64| -> Option<f64> {
        match &margin {
            Value::Dict(m) => {
                let v = keys
                    .iter()
                    .find_map(|k| m.get(k).ok())
                    .cloned()
                    .unwrap_or(Value::Auto);
                margin_side(&v, extent, short, text)
            }
            v => margin_side(v, extent, short, text),
        }
    };
    let margins = Sides {
        top: side(&["top", "y", "rest"], size[1])?,
        bottom: side(&["bottom", "y", "rest"], size[1])?,
        left: side(&["left", "inside", "x", "rest"], size[0])?,
        right: side(&["right", "outside", "x", "rest"], size[0])?,
    };
    let columns = match get("columns") {
        Value::Int(n) => n.max(1) as u32,
        _ => 1,
    };
    let body = size[0] - margins.left - margins.right;
    Some(PageSetup {
        margins,
        columns,
        gutter: margin_side(&get("gutter"), body, short, text)?,
        flipped: matches!(get("flipped"), Value::Bool(true)),
        header_ascent: margin_side(&get("ascent"), margins.top, short, text)?,
        footer_descent: margin_side(&get("descent"), margins.bottom, short, text)?,
    })
}

/// A bracket around inline content only, which a paragraph reads through:
/// a quill hint anywhere, and a Block or Align only as the one bracket of
/// that kind inside an element that already forms a paragraph, since
/// elsewhere each block starts a paragraph of its own.
fn transparent(doc: &LayoutDoc, id: NodeId) -> bool {
    let reads_through = match doc.nodes[id].kind {
        NodeKind::Hint(_) => true,
        NodeKind::Block { .. } | NodeKind::Align { .. } => {
            in_paragraph(doc, id) && only_bracket(doc, id)
        }
        _ => false,
    };
    reads_through
        && doc.nodes[id].children.iter().all(|c| match c {
            Child::Item(r) => inline_item(doc, *r),
            Child::Node(n) => inline_node(doc, *n),
        })
}

/// Whether the nearest element above `id`, past brackets and inline
/// elements, is one Typst lays out as a paragraph.
fn in_paragraph(doc: &LayoutDoc, id: NodeId) -> bool {
    let mut at = doc.nodes[id].parent;
    while let Some(p) = at {
        match &doc.nodes[p].kind {
            NodeKind::Par { .. }
            | NodeKind::Heading { .. }
            | NodeKind::ListBody
            | NodeKind::Caption
            | NodeKind::TocEntry { .. }
            | NodeKind::InlineRun { .. }
            | NodeKind::Unstructured
            | NodeKind::RawLine => return true,
            NodeKind::Block { .. } | NodeKind::Align { .. } | NodeKind::Hint(_) => {}
            k if k.is_inline() => {}
            _ => return false,
        }
        at = doc.nodes[p].parent;
    }
    false
}

/// Whether `id` is its parent's only Block or Align child.
fn only_bracket(doc: &LayoutDoc, id: NodeId) -> bool {
    let Some(parent) = doc.nodes[id].parent else {
        return false;
    };
    doc.nodes[parent]
        .children
        .iter()
        .filter(|c| {
            matches!(c, Child::Node(n) if matches!(
                doc.nodes[*n].kind,
                NodeKind::Block { .. } | NodeKind::Align { .. }
            ))
        })
        .count()
        == 1
}

fn inline_item(doc: &LayoutDoc, r: ItemRef) -> bool {
    matches!(
        doc.item(r).kind,
        ItemKind::Text(_)
            | ItemKind::Raster { .. }
            | ItemKind::Mark(PointHint::Br | PointHint::FootnoteRef { .. })
    )
}

fn inline_node(doc: &LayoutDoc, n: NodeId) -> bool {
    doc.nodes[n].kind.is_inline() || transparent(doc, n)
}

fn inline_child(doc: &LayoutDoc, c: &Child) -> bool {
    match c {
        Child::Item(r) => inline_item(doc, *r),
        Child::Node(n) => inline_node(doc, *n),
    }
}

/// A page break that opens or closes an element, as a show rule's
/// `pagebreak` does around a heading, is moved out to stand before or after
/// it, so the element keeps its own kind.
fn hoist_breaks(doc: &mut LayoutDoc) {
    let is_break = |doc: &LayoutDoc, c: &Child| matches!(c, Child::Item(r) if matches!(doc.item(*r).kind, ItemKind::Mark(PointHint::Break { .. })));
    for id in (0..doc.nodes.len()).rev() {
        let Some(parent) = doc.nodes[id].parent else {
            continue;
        };
        let children = &doc.nodes[id].children;
        let lead = children.iter().take_while(|c| is_break(doc, c)).count();
        if lead == children.len() {
            continue;
        }
        let trail = children
            .iter()
            .rev()
            .take_while(|c| is_break(doc, c))
            .count();
        if lead == 0 && trail == 0 {
            continue;
        }
        let mut kept = std::mem::take(&mut doc.nodes[id].children);
        let after: Vec<Child> = kept.split_off(kept.len() - trail);
        let before: Vec<Child> = kept.drain(..lead).collect();
        doc.nodes[id].children = kept;
        for c in before.iter().chain(&after) {
            if let Child::Item(r) = c {
                doc.pages[r.page as usize].items[r.index as usize].node = parent;
            }
        }
        let siblings = &mut doc.nodes[parent].children;
        let at = siblings
            .iter()
            .position(|c| matches!(c, Child::Node(n) if *n == id))
            .expect("a node is among its parent's children");
        siblings.splice(at + 1..at + 1, after);
        siblings.splice(at..at, before);
    }
}

/// Every run of inline children of a node that does not form a paragraph by
/// itself, or that holds a non-inline child, becomes an `InlineRun` (an
/// `Unstructured` run at the root and in an artifact that is not chrome).
fn form_paragraphs(doc: &mut LayoutDoc) {
    let mut id = 0;
    while id < doc.nodes.len() {
        let kind = doc.nodes[id].kind.clone();
        let mixed = doc.nodes[id].children.iter().any(|c| !inline_child(doc, c));
        let groups = !kind.is_inline()
            && !transparent(doc, id)
            && (!kind.forms_paragraph() || mixed)
            && !matches!(kind, NodeKind::ListLabel);
        if groups {
            let style = doc
                .ancestor(id, |k| matches!(k, NodeKind::Block { .. }))
                .and_then(|b| match doc.nodes[b].kind {
                    NodeKind::Block { style, .. } => Some(style),
                    _ => None,
                });
            let run_kind = match kind {
                NodeKind::Root
                | NodeKind::Artifact {
                    kind: ArtifactKind::Other,
                } => NodeKind::Unstructured,
                _ => NodeKind::InlineRun { style },
            };
            let inline: Vec<bool> = doc.nodes[id]
                .children
                .iter()
                .map(|c| inline_child(doc, c))
                .collect();
            let children = std::mem::take(&mut doc.nodes[id].children);
            let mut out = Vec::new();
            let mut run: Vec<Child> = Vec::new();
            let flush = |doc: &mut LayoutDoc, run: &mut Vec<Child>, out: &mut Vec<Child>| {
                let has_text = run.iter().any(|c| match c {
                    Child::Item(r) => matches!(doc.item(*r).kind, ItemKind::Text(_)),
                    Child::Node(n) => doc
                        .subtree(*n)
                        .iter()
                        .flat_map(|&m| doc.nodes[m].children.clone())
                        .any(|c| {
                            matches!(c, Child::Item(r) if matches!(doc.item(r).kind, ItemKind::Text(_)))
                        }),
                });
                if run.is_empty() {
                    return;
                }
                if !has_text {
                    out.append(run);
                    return;
                }
                let n = doc.nodes.len();
                let mut node = Node::new(Some(id), run_kind.clone());
                node.children = std::mem::take(run);
                for c in &node.children {
                    match c {
                        Child::Node(m) => doc.nodes[*m].parent = Some(n),
                        Child::Item(r) => {
                            doc.pages[r.page as usize].items[r.index as usize].node = n
                        }
                    }
                }
                doc.nodes.push(node);
                out.push(Child::Node(n));
            };
            for (c, inline) in children.into_iter().zip(inline) {
                if inline {
                    run.push(c);
                } else {
                    flush(doc, &mut run, &mut out);
                    out.push(c);
                }
            }
            flush(doc, &mut run, &mut out);
            doc.nodes[id].children = out;
        }
        id += 1;
    }
}

/// The text and marks a paragraph-forming node reads, in reading order,
/// through its inline children.
fn inline_items(doc: &LayoutDoc, id: NodeId, out: &mut Vec<ItemRef>) {
    for c in &doc.nodes[id].children {
        match c {
            Child::Item(r) => out.push(*r),
            Child::Node(n) if inline_node(doc, *n) => inline_items(doc, *n, out),
            Child::Node(_) => {}
        }
    }
}

fn form_lines(doc: &mut LayoutDoc) {
    for id in 0..doc.nodes.len() {
        if !doc.nodes[id].kind.forms_paragraph()
            || doc.nodes[id].children.iter().any(|c| !inline_child(doc, c))
        {
            continue;
        }
        let mut items = Vec::new();
        inline_items(doc, id, &mut items);
        let mut lines: Vec<Line> = Vec::new();
        // Each line's main size, and whether an item that is not raised or
        // lowered set its baseline.
        let mut mains: Vec<(f64, bool)> = Vec::new();
        for r in items {
            match &doc.item(r).kind {
                ItemKind::Text(t) => {
                    let raised = doc.raised(doc.item(r).node, id);
                    let y = t.origin[1];
                    // A raised or lowered item joins the line it follows,
                    // within an em of its baseline.
                    let same = lines
                        .last()
                        .zip(mains.last())
                        .is_some_and(|(l, &(size, main))| {
                            let size = size.max(t.size);
                            let reach = if raised || !main { size } else { 0.45 * size };
                            l.page == r.page
                                && (l.baseline - y).abs() <= reach
                                && (!raised || t.origin[0] >= l.x1 - 1.0)
                        });
                    if !same {
                        if let Some(l) = lines.last_mut() {
                            if l.end == LineEnd::Last {
                                l.end = LineEnd::Soft;
                            }
                        }
                        lines.push(Line {
                            page: r.page,
                            baseline: y,
                            x0: t.origin[0],
                            x1: t.origin[0] + t.width,
                            items: Vec::new(),
                            end: LineEnd::Last,
                        });
                        mains.push((t.size, !raised));
                    }
                    let l = lines.last_mut().expect("a line is open");
                    let m = mains.last_mut().expect("a line is open");
                    if !raised && (!m.1 || t.size > m.0) {
                        l.baseline = y;
                        *m = (t.size, true);
                    }
                    l.x0 = l.x0.min(t.origin[0]);
                    l.x1 = l.x1.max(t.origin[0] + t.width);
                    l.items.push(r);
                }
                ItemKind::Raster { rect, .. } => {
                    let on = lines.last_mut().filter(|l| {
                        l.page == r.page
                            && rect.y0 <= l.baseline + 1.0
                            && rect.y1 >= l.baseline - 1.0
                    });
                    match on {
                        Some(l) => {
                            l.x0 = l.x0.min(rect.x0);
                            l.x1 = l.x1.max(rect.x1);
                            l.items.push(r);
                        }
                        None => {
                            lines.push(Line {
                                page: r.page,
                                baseline: rect.y1,
                                x0: rect.x0,
                                x1: rect.x1,
                                items: vec![r],
                                end: LineEnd::Last,
                            });
                            mains.push((0.0, true));
                        }
                    }
                }
                ItemKind::Mark(m) => {
                    if let Some(l) = lines.last_mut() {
                        if matches!(m, PointHint::Br) {
                            l.end = LineEnd::Forced;
                        }
                        l.items.push(r);
                    }
                }
                _ => {}
            }
        }
        for l in &mut lines {
            // A mark keeps its place after the ink it follows in reading
            // order, so a footnote's reference stays where it is printed.
            let mut at = f64::MIN;
            let mut keyed: Vec<(f64, ItemRef)> = l
                .items
                .iter()
                .map(|r| {
                    match &doc.item(*r).kind {
                        ItemKind::Text(t) => at = t.origin[0],
                        ItemKind::Raster { rect, .. } => at = rect.x0,
                        _ => {}
                    }
                    (at, *r)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            l.items = keyed.into_iter().map(|(_, r)| r).collect();
            if l.end == LineEnd::Soft {
                let last = l.items.iter().rev().find_map(|r| match &doc.item(*r).kind {
                    ItemKind::Text(t) => Some(t.text.as_str()),
                    _ => None,
                });
                if last.is_some_and(|t| t.ends_with('\u{ad}')) {
                    l.end = LineEnd::Hyphenated;
                }
            }
        }
        for i in 1..lines.len() {
            if lines[i - 1].end == LineEnd::Soft && !could_wrap(doc, &lines[i - 1], &lines[i]) {
                lines[i - 1].end = LineEnd::Forced;
            }
        }
        doc.nodes[id].lines = lines;
    }
}

/// Whether Typst's line breaker could have ended `line` where `next` starts.
/// A link breaks between any two characters, and an inline object on either
/// side offers a break; otherwise a word joined to the next word, or closing
/// punctuation to a word, is no break opportunity, so the lines were laid out
/// apart, as a stack's children are.
fn could_wrap(doc: &LayoutDoc, line: &Line, next: &Line) -> bool {
    let text = |r: Option<&ItemRef>| match r.map(|r| (r, &doc.item(*r).kind)) {
        Some((r, ItemKind::Text(t))) => Some((*r, t.text.as_str())),
        _ => None,
    };
    let inked = |r: &&ItemRef| !matches!(doc.item(**r).kind, ItemKind::Mark(_));
    let (Some((r, end)), Some((_, start))) = (
        text(line.items.iter().rfind(inked)),
        text(next.items.iter().find(inked)),
    ) else {
        return true;
    };
    if doc
        .ancestor(doc.item(r).node, |k| matches!(k, NodeKind::Link { .. }))
        .is_some()
    {
        return true;
    }
    let word = |c: char| {
        c.is_alphanumeric()
            && ((c as u32) < 0x0E00
                || (0x10A0..0x1100).contains(&(c as u32))
                || (0x1E00..0x2000).contains(&(c as u32)))
    };
    let (Some(last), Some(first)) = (end.chars().next_back(), start.chars().next()) else {
        return true;
    };
    let holds_after = word(last) || matches!(last, '.' | ',' | ':' | ';' | ')' | ']');
    !(holds_after && word(first))
}

fn item_rect(doc: &LayoutDoc, r: ItemRef) -> Option<Rect> {
    match &doc.item(r).kind {
        ItemKind::Text(t) => Some(t.rect()),
        ItemKind::Image { rect, .. } | ItemKind::Raster { rect, .. } => Some(*rect),
        ItemKind::Shape(s) => Some(s.rect),
        ItemKind::Mark(_) => None,
    }
}

fn measure_frames(doc: &mut LayoutDoc) {
    let mut order = doc.subtree(0);
    order.reverse();
    for id in order {
        let mut frames: Vec<(u32, Rect)> = Vec::new();
        let mut add = |page: u32, rect: Rect| match frames.iter_mut().find(|(p, _)| *p == page) {
            Some((_, f)) => *f = f.union(&rect),
            None => frames.push((page, rect)),
        };
        for c in &doc.nodes[id].children {
            match c {
                Child::Item(r) => {
                    if let Some(rect) = item_rect(doc, *r) {
                        add(r.page, rect);
                    }
                }
                Child::Node(n) => {
                    for &(p, f) in &doc.nodes[*n].frames {
                        add(p, f);
                    }
                }
            }
        }
        frames.sort_by_key(|f| f.0);
        doc.nodes[id].frames = frames;
    }
}

/// A table's column boundaries are its cells' origins, the last one closed
/// by the column's own size: a fixed or relative width, the container's
/// edge for a fraction, or the widest content plus its inset for `auto`.
/// Its rows' tops are its cells' origins; its header rows are the rows of
/// its header cells.
fn measure_tables(
    doc: &mut LayoutDoc,
    origins: &HashMap<NodeId, (u32, Point)>,
    sizings: &HashMap<NodeId, (Vec<Sizing>, f64)>,
) {
    for id in 0..doc.nodes.len() {
        if !matches!(doc.nodes[id].kind, NodeKind::Table { .. }) {
            continue;
        }
        let cells = doc.cells_of(id);
        let (sizing, size) = sizings.get(&id).cloned().unwrap_or_default();
        let mut starts: HashMap<u32, f64> = HashMap::new();
        let mut tops: HashMap<u32, (u32, f64)> = HashMap::new();
        let mut header = 0;
        let mut count = sizing.len() as u32;
        for &c in &cells {
            if let NodeKind::Cell {
                col,
                row,
                colspan,
                header: h,
                ..
            } = doc.nodes[c].kind
            {
                count = count.max(col + colspan);
                if let Some(&(page, at)) = origins.get(&c) {
                    let x = at.x.to_pt();
                    starts.entry(col).and_modify(|v| *v = v.min(x)).or_insert(x);
                    let y = at.y.to_pt();
                    tops.entry(row)
                        .and_modify(|v| {
                            if (page, y) < *v {
                                *v = (page, y)
                            }
                        })
                        .or_insert((page, y));
                }
                if h {
                    header = header.max(row + 1);
                }
            }
        }
        let (left, right) = container(doc, id);
        let last = count.saturating_sub(1);
        let last_start = starts.get(&last).copied().unwrap_or(left);
        let em = |l: Length| l.abs.to_pt() + l.em.get() * size;
        let end = match sizing.get(last as usize).or(sizing.last()) {
            Some(Sizing::Rel(r)) => last_start + r.rel.get() * (right - left) + em(r.abs),
            Some(Sizing::Fr(_)) => right,
            _ => cells
                .iter()
                .filter_map(|&c| match doc.nodes[c].kind {
                    NodeKind::Cell {
                        col,
                        colspan,
                        inset,
                        ..
                    } if col + colspan == count => Some(
                        doc.nodes[c].frames.first().map_or(last_start, |f| f.1.x1) + inset.right,
                    ),
                    _ => None,
                })
                .fold(last_start, f64::max),
        };
        let mut columns: Vec<f64> = (0..count)
            .map(|c| starts.get(&c).copied().unwrap_or(f64::NAN))
            .collect();
        columns.push(end);
        for i in (0..count as usize).rev() {
            if columns[i].is_nan() {
                columns[i] = columns[i + 1];
            }
        }
        let content_sized: Vec<bool> = (0..count as usize)
            .map(|c| matches!(sizing.get(c).or(sizing.last()), Some(Sizing::Auto) | None))
            .collect();
        let mut rows: Vec<(u32, u32, f64)> =
            tops.into_iter().map(|(r, (p, y))| (r, p, y)).collect();
        rows.sort_by_key(|r| r.0);
        if let NodeKind::Table {
            columns: c,
            content_sized: cs,
            rows: rs,
            header_rows,
            ..
        } = &mut doc.nodes[id].kind
        {
            *c = columns;
            *cs = content_sized;
            *rs = rows.into_iter().map(|(_, p, y)| (p, y)).collect();
            *header_rows = header;
        }
    }
}

/// The left and right edges a table at `id` lays out in: the inside of the
/// cell it sits in, else its page's text area.
fn container(doc: &LayoutDoc, id: NodeId) -> (f64, f64) {
    let outer = doc.nodes[id]
        .parent
        .and_then(|p| doc.ancestor(p, |k| matches!(k, NodeKind::Cell { .. })));
    if let Some(cell) = outer {
        if let NodeKind::Cell {
            col,
            colspan,
            inset,
            ..
        } = doc.nodes[cell].kind
        {
            let table = doc.ancestor(cell, |k| matches!(k, NodeKind::Table { .. }));
            if let Some(NodeKind::Table { columns, .. }) = table.map(|t| &doc.nodes[t].kind) {
                if let (Some(a), Some(b)) = (
                    columns.get(col as usize),
                    columns.get((col + colspan) as usize),
                ) {
                    return (a + inset.left, b - inset.right);
                }
            }
        }
    }
    let page = doc.nodes[id].frames.first().map_or(0, |f| f.0) as usize;
    let p = &doc.pages[page];
    (p.setup.margins.left, p.size[0] - p.setup.margins.right)
}

/// A cell's fill, inset and strokes from its resolved fields; an `em`
/// resolves at `size`.
fn cell_style(
    inset: Smart<TSides<Option<typst::layout::Rel<Length>>>>,
    fill: &Smart<Option<Paint>>,
    stroke: &TSides<Option<Option<std::sync::Arc<typst::visualize::Stroke>>>>,
    size: f64,
) -> (Option<Rgb>, Sides, [Option<(f64, Rgb)>; 4]) {
    let len = |l: Length| l.abs.to_pt() + l.em.get() * size;
    let inset = match inset {
        Smart::Custom(i) => {
            let side = |v: Option<typst::layout::Rel<Length>>| v.map_or(0.0, |r| len(r.abs));
            Sides {
                top: side(i.top),
                right: side(i.right),
                bottom: side(i.bottom),
                left: side(i.left),
            }
        }
        Smart::Auto => Sides::default(),
    };
    let fill = match fill {
        Smart::Custom(Some(p)) => Some(rgb(p)),
        _ => None,
    };
    let side = |v: &Option<Option<std::sync::Arc<typst::visualize::Stroke>>>| match v {
        Some(Some(s)) => {
            let width = match s.thickness {
                Smart::Custom(l) => len(l),
                Smart::Auto => 1.0,
            };
            let color = match &s.paint {
                Smart::Custom(p) => rgb(p),
                Smart::Auto => Rgb(0, 0, 0),
            };
            Some((width, color))
        }
        _ => None,
    };
    let borders = [
        side(&stroke.top),
        side(&stroke.right),
        side(&stroke.bottom),
        side(&stroke.left),
    ];
    (fill, inset, borders)
}
