use crate::layout::{
    ArtifactKind, Child, Geometry, HAlign, ImageFormat, ItemKind, NodeId, NodeKind, Rect,
};
use crate::model::{
    Anchor, Block, Blocks, Drawing, DrawingContent, Media, MediaKind, Placement, ShapeGeometry,
    Stroke, TextBox, Wrap,
};

use super::paragraph::read_into;
use super::space::{self, Entry};
use super::{Ctx, Lower, hex};

pub(super) const EMU_PER_PT: f64 = 12700.0;

pub(super) fn emu(pt: f64) -> i64 {
    (pt * EMU_PER_PT).round() as i64
}

impl<'a> Lower<'a> {
    pub(super) fn picture(
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
    pub(super) fn shape(&mut self, shape: &crate::layout::Shape) -> Drawing {
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
    pub(super) fn inline_shapes(&mut self, n: NodeId) -> Vec<Entry> {
        let kids = self.doc.nodes[n].children.clone();
        let mut out = Vec::new();
        for c in kids {
            match c {
                Child::Item(r) if matches!(self.doc.item(r).kind, ItemKind::Shape(_)) => {
                    out.extend(self.loose_item(r, &Ctx::default()));
                }
                Child::Node(m) if read_into(self.kind(m)) => out.extend(self.inline_shapes(m)),
                _ => {}
            }
        }
        out
    }

    /// `n`'s content as a page-anchored text box where its ink sits, then
    /// the boxes of the places inside it, since Word nests no text box in
    /// another.
    pub(super) fn text_box(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Drawing> {
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
        let inner = Ctx {
            left: x,
            width,
            align: ctx.align,
            page_number: ctx.page_number,
            ..Ctx::default()
        };
        self.boxed += 1;
        let entries = match self.kind(n) {
            NodeKind::Place { .. } | NodeKind::Artifact { .. } => self.children(n, &inner),
            _ => self.node(n, &inner),
        };
        self.boxed -= 1;
        let (blocks, inner) = space::nested(entries, None);
        let inner_boxes: Vec<Drawing> = inner.into_iter().map(|(_, d)| d).collect();
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
}
