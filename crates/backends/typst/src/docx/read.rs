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
    ArtifactKind, Child, DocInfo, FontFace, FontId, Geometry, HAlign, ImageData, ImageFormat,
    ImageId, Item, ItemKind, ItemRef, LayoutDoc, Line, LineEnd, Marker, Node, NodeId, NodeKind,
    Notice, Page, PageSetup, ParStyle, Parity, PointHint, Rect, Rgb, Shape, Sides, TextRun,
};
use typst::foundations::{Content, Dict, StyleChain, Value};
use typst::introspection::{Location, Tag};
use typst::layout::{Alignment, Frame, FrameItem, HAlignment, Length, Point, Transform};
use typst::layout::{GridCell, GridElem, HideElem, PlaceElem};
use typst::model::{
    Destination, EmphElem, EnumElem, FigureCaption, FigureElem, FootnoteElem, HeadingElem,
    LinkElem, LinkTarget, ListElem, ParElem, StrongElem, TableCell, TableElem,
};
use typst::pdf::{ArtifactElem, PdfMarkerTag, PdfMarkerTagKind, TableCellKind};
use typst::text::{HighlightElem, StrikeElem, SubElem, SuperElem, UnderlineElem};
use typst::visualize::{ExchangeFormat, ImageKind, Paint, RasterFormat};
use typst_layout::{PagedDocument, PagedIntrospector};

use crate::tags::{marker, relocated};

pub(crate) const CODE_MARGINS_ASSUMED: &str = "docx::margins_assumed";
pub(crate) const CODE_IMAGE_DROPPED: &str = "docx::image_dropped";
const HINT: &str = "__qm_hint__";

#[derive(Clone, Copy, PartialEq)]
enum Key {
    Elem(Location),
    Bracket(&'static str),
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
    out: LayoutDoc,
    stack: Vec<Open>,
    saved: Vec<(Location, Vec<Open>)>,
    by_loc: HashMap<Location, NodeId>,
    fonts: HashMap<(String, bool, bool), FontId>,
    images: HashMap<u128, ImageId>,
    origins: HashMap<NodeId, (u32, Point)>,
    page: u32,
    probe: Option<Dict>,
    number_text: Option<String>,
    images_dropped: usize,
}

/// The Layout IR of `document`.
pub(crate) fn read(document: &PagedDocument, title: Option<String>) -> LayoutDoc {
    let mut r = Reader {
        flow: document.introspector(),
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
        images_dropped: 0,
    };
    let mut assumed = 0;
    for (i, page) in document.pages().iter().enumerate() {
        r.page = i as u32;
        r.probe = None;
        r.number_text = None;
        let size = [page.frame.width().to_pt(), page.frame.height().to_pt()];
        r.out.pages.push(Page {
            size,
            setup: PageSetup {
                margins: Sides::default(),
                columns: 1,
                gutter: 0.0,
                flipped: false,
            },
            number: page.number as u64,
            number_text: None,
            items: Vec::new(),
        });
        r.walk(&page.frame, Transform::identity());
        let setup = r.probe.as_ref().and_then(|d| page_setup(d, size));
        if setup.is_none() {
            assumed += 1;
        }
        let p = &mut r.out.pages[i];
        p.setup = setup.unwrap_or(PageSetup {
            margins: Sides {
                top: 72.0,
                right: 72.0,
                bottom: 72.0,
                left: 72.0,
            },
            columns: 1,
            gutter: 0.0,
            flipped: false,
        });
        p.number_text = r.number_text.take();
    }
    if assumed > 0 {
        r.notice(
            CODE_MARGINS_ASSUMED,
            format!("{assumed} page(s) carry no page probe, so their margins are taken as 1 in"),
        );
    }
    if r.images_dropped > 0 {
        let n = r.images_dropped;
        r.notice(
            CODE_IMAGE_DROPPED,
            format!("{n} image(s) are not PNG or JPEG and are left out of the Word file"),
        );
    }
    let mut out = r.out;
    let origins = r.origins;
    form_paragraphs(&mut out);
    form_lines(&mut out);
    measure_frames(&mut out);
    measure_tables(&mut out, &origins);
    out
}

impl Reader<'_> {
    fn notice(&mut self, code: &str, message: String) {
        self.out.notices.push(Notice {
            code: code.to_string(),
            message,
        });
    }

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
            self.stack.truncate(i);
        }
    }

    fn ink(&mut self, kind: ItemKind) {
        let Some(node) = self.top() else { return };
        let page = &mut self.out.pages[self.page as usize];
        let at = ItemRef {
            page: self.page,
            index: page.items.len() as u32,
        };
        page.items.push(Item { node, kind });
        self.out.nodes[node].children.push(Child::Item(at));
    }

    fn walk(&mut self, frame: &Frame, ts: Transform) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
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
                }
                FrameItem::Tag(Tag::Start(elem, _)) => self.start(elem, pos.transform(ts)),
                FrameItem::Tag(Tag::End(loc, ..)) => {
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
                    let bold = info.variant.weight.to_number() >= 600;
                    let italic = !matches!(info.variant.style, typst::text::FontStyle::Normal);
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
                    let scale = ts.sy.get().abs();
                    let size = text.size.to_pt() * scale;
                    let width: f64 = text
                        .glyphs
                        .iter()
                        .map(|g| g.x_advance.at(text.size).to_pt())
                        .sum::<f64>()
                        * scale;
                    let origin = pos.transform(ts);
                    self.ink(ItemKind::Text(TextRun {
                        origin: [origin.x.to_pt(), origin.y.to_pt()],
                        text: text.text.to_string(),
                        font,
                        size,
                        fill: rgb(&text.fill),
                        width,
                    }));
                }
                FrameItem::Shape(shape, _) => {
                    if self.top().is_none() || self.hidden() {
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
                    let ImageKind::Raster(raster) = image.kind() else {
                        self.images_dropped += 1;
                        continue;
                    };
                    let format = match raster.format() {
                        RasterFormat::Exchange(ExchangeFormat::Png) => ImageFormat::Png,
                        RasterFormat::Exchange(ExchangeFormat::Jpg) => ImageFormat::Jpeg,
                        _ => {
                            self.images_dropped += 1;
                            continue;
                        }
                    };
                    let hash = typst::utils::hash128(raster.data());
                    let next = self.out.images.len();
                    let id = *self.images.entry(hash).or_insert(next);
                    if id == next {
                        self.out.images.push(ImageData {
                            format,
                            bytes: raster.data().to_vec(),
                        });
                    }
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

    fn start(&mut self, elem: &Content, at: Point) {
        if let Some(d) = marker(elem, HINT) {
            if !self.hidden() {
                self.hint(&d);
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
            let node = self.add_node(NodeKind::Cell {
                col: smart_index(cell.x.get(StyleChain::default())),
                row: smart_index(cell.y.get(StyleChain::default())),
                colspan: cell.colspan.get(StyleChain::default()).get() as u32,
                rowspan: cell.rowspan.get(StyleChain::default()).get() as u32,
                header,
            });
            self.origins.insert(node, (self.page, at));
            self.by_loc.insert(loc, node);
            self.push(Key::Elem(loc), Some(node), false);
            return;
        }
        if let Some(cell) = elem.to_packed::<GridCell>() {
            let node = self.add_node(NodeKind::Cell {
                col: smart_index(cell.x.get(StyleChain::default())),
                row: smart_index(cell.y.get(StyleChain::default())),
                colspan: cell.colspan.get(StyleChain::default()).get() as u32,
                rowspan: cell.rowspan.get(StyleChain::default()).get() as u32,
                header: false,
            });
            self.origins.insert(node, (self.page, at));
            self.by_loc.insert(loc, node);
            self.push(Key::Elem(loc), Some(node), false);
            return;
        }
        let Some(kind) = node_kind(elem) else { return };
        if self.top().is_none() {
            self.push(Key::Elem(loc), None, false);
            return;
        }
        let node = self.add_node(kind);
        self.by_loc.insert(loc, node);
        self.push(Key::Elem(loc), Some(node), false);
    }

    fn hint(&mut self, d: &Dict) {
        let get = |k: &str| d.get(k).ok().cloned().unwrap_or(Value::None);
        let Value::Str(hint) = get("hint") else {
            return;
        };
        let size = length(&get("size"), 0.0).unwrap_or(11.0);
        let close = matches!(get("edge"), Value::Str(s) if s.as_str() == "close");
        match hint.as_str() {
            "page" => self.probe = Some(d.clone()),
            "pnum" => {
                if let Value::Str(s) = get("s") {
                    self.number_text
                        .get_or_insert_with(String::new)
                        .push_str(s.as_str());
                }
            }
            "br" => self.ink(ItemKind::Mark(PointHint::Br)),
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
                };
                let par = self.stack.iter().rev().find_map(|o| {
                    o.node
                        .filter(|&n| matches!(self.out.nodes[n].kind, NodeKind::Par { .. }))
                });
                if let Some(n) = par {
                    if let NodeKind::Par {
                        style: s @ None, ..
                    } = &mut self.out.nodes[n].kind
                    {
                        *s = Some(style);
                    }
                }
            }
            "block" | "align" | "columns" | "box" => {
                let key = Key::Bracket(match hint.as_str() {
                    "block" => "block",
                    "align" => "align",
                    "columns" => "columns",
                    _ => "box",
                });
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
                        let node = self.add_node(kind);
                        self.push(key, Some(node), false);
                    }
                    None => self.push(key, None, false),
                }
            }
            _ => {}
        }
    }
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
            PdfMarkerTagKind::ListItemLabel | PdfMarkerTagKind::TermsItemLabel => {
                Some(NodeKind::ListLabel)
            }
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
        return Some(NodeKind::Place {
            float: place.float.get(styles),
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
    } else if elem.is::<HighlightElem>() {
        NodeKind::Highlight
    } else if elem.is::<SubElem>() {
        NodeKind::Sub
    } else if elem.is::<SuperElem>() {
        NodeKind::Super
    } else if elem.is::<ListElem>() {
        NodeKind::List {
            marker: Marker::Bullet,
        }
    } else if elem.is::<EnumElem>() {
        NodeKind::List {
            marker: Marker::Numbered,
        }
    } else if elem.is::<TableElem>() || elem.is::<GridElem>() {
        NodeKind::Table {
            columns: Vec::new(),
            header_rows: 0,
            grid: elem.is::<GridElem>(),
        }
    } else if elem.is::<FigureElem>() {
        NodeKind::Figure
    } else if elem.is::<FigureCaption>() {
        NodeKind::Caption
    } else {
        return None;
    };
    Some(kind)
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
    Some(PageSetup {
        margins,
        columns,
        gutter: 0.04 * size[0],
        flipped: matches!(get("flipped"), Value::Bool(true)),
    })
}

/// A Block bracket around inline content only, which a paragraph reads
/// through.
fn transparent(doc: &LayoutDoc, id: NodeId) -> bool {
    matches!(
        doc.nodes[id].kind,
        NodeKind::Block { .. } | NodeKind::Align { .. }
    ) && doc.nodes[id].children.iter().all(|c| match c {
        Child::Item(r) => inline_item(doc, *r),
        Child::Node(n) => inline_node(doc, *n),
    })
}

fn inline_item(doc: &LayoutDoc, r: ItemRef) -> bool {
    matches!(
        doc.item(r).kind,
        ItemKind::Text(_) | ItemKind::Mark(PointHint::Br | PointHint::FootnoteRef { .. })
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

/// Every run of inline children of a node that does not form a paragraph by
/// itself, or that holds a non-inline child, becomes an `InlineRun` (an
/// `Unstructured` run at the root and in out-of-flow containers).
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
                NodeKind::Root | NodeKind::Artifact { .. } | NodeKind::Footnote => {
                    NodeKind::Unstructured
                }
                _ => NodeKind::InlineRun { style },
            };
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
            for c in children {
                if inline_child(doc, &c) {
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
        for r in items {
            match &doc.item(r).kind {
                ItemKind::Text(t) => {
                    let same = lines.last().is_some_and(|l| {
                        l.page == r.page && (l.baseline - t.origin[1]).abs() <= 0.45 * t.size
                    });
                    if !same {
                        if let Some(l) = lines.last_mut() {
                            if l.end == LineEnd::Last {
                                l.end = LineEnd::Soft { ate_space: false };
                            }
                        }
                        lines.push(Line {
                            page: r.page,
                            baseline: t.origin[1],
                            x0: t.origin[0],
                            x1: t.origin[0] + t.width,
                            items: Vec::new(),
                            end: LineEnd::Last,
                        });
                    }
                    let l = lines.last_mut().expect("a line is open");
                    l.x0 = l.x0.min(t.origin[0]);
                    l.x1 = l.x1.max(t.origin[0] + t.width);
                    l.items.push(r);
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
            let x = |r: &ItemRef| match &doc.item(*r).kind {
                ItemKind::Text(t) => t.origin[0],
                _ => f64::MAX,
            };
            l.items.sort_by(|a, b| x(a).total_cmp(&x(b)));
            if matches!(l.end, LineEnd::Soft { .. }) {
                let last = l.items.iter().rev().find_map(|r| match &doc.item(*r).kind {
                    ItemKind::Text(t) => Some(t.text.as_str()),
                    _ => None,
                });
                l.end = match last {
                    Some(t) if t.ends_with('\u{ad}') => LineEnd::Hyphenated,
                    Some(t) => LineEnd::Soft {
                        ate_space: t.ends_with(char::is_whitespace),
                    },
                    None => LineEnd::Soft { ate_space: false },
                };
            }
        }
        doc.nodes[id].lines = lines;
    }
}

fn item_rect(doc: &LayoutDoc, r: ItemRef) -> Option<Rect> {
    match &doc.item(r).kind {
        ItemKind::Text(t) => Some(t.rect()),
        ItemKind::Image { rect, .. } => Some(*rect),
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

/// A table's column boundaries are its cells' origins, closed by the
/// table's right edge; its header rows are the rows of its header cells.
fn measure_tables(doc: &mut LayoutDoc, origins: &HashMap<NodeId, (u32, Point)>) {
    for id in 0..doc.nodes.len() {
        if !matches!(doc.nodes[id].kind, NodeKind::Table { .. }) {
            continue;
        }
        let cells: Vec<NodeId> = doc
            .subtree(id)
            .into_iter()
            .filter(|&n| {
                matches!(doc.nodes[n].kind, NodeKind::Cell { .. })
                    && doc.ancestor(n, |k| matches!(k, NodeKind::Table { .. })) == Some(id)
            })
            .collect();
        let mut starts: HashMap<u32, f64> = HashMap::new();
        let mut header = 0;
        let mut count = 0;
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
                if let Some((_, at)) = origins.get(&c) {
                    let x = at.x.to_pt();
                    starts.entry(col).and_modify(|v| *v = v.min(x)).or_insert(x);
                }
                if h {
                    header = header.max(row + 1);
                }
            }
        }
        let right = doc.nodes[id]
            .frames
            .first()
            .map(|f| f.1.x1)
            .unwrap_or_default();
        let mut columns: Vec<f64> = (0..count)
            .map(|c| starts.get(&c).copied().unwrap_or(f64::NAN))
            .collect();
        columns.push(right);
        for i in (0..count as usize).rev() {
            if columns[i].is_nan() {
                columns[i] = columns[i + 1];
            }
        }
        if let NodeKind::Table {
            columns: c,
            header_rows,
            ..
        } = &mut doc.nodes[id].kind
        {
            *c = columns;
            *header_rows = header;
        }
    }
}
