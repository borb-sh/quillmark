//! Recover schema-field regions from *glyph spans* — the origin every drawn
//! frame item already carries.
//!
//! Every `Text` glyph (and `Shape`/`Image` item) in the laid-out frames
//! carries a [`Span`] pointing at the source expression that produced it.
//! `eval(str, mode: "markup")` stamps its whole result with the *call-site
//! argument's* span (`SpanMode::Uniform`), so content evaluated at a
//! codegen-emitted per-field call site resolves to that site's byte range in
//! the generated helper `lib.typ` — the backend records each site's **byte
//! window** at generation time ([`FieldWindow`]) and the scan classifies a
//! frame item by which window its resolved range nests inside. A scalar the
//! plate interpolates directly (`#data.subject`) needs no codegen: its glyphs
//! carry the reference expression's own span in the plate, and
//! [`scalar_windows`] recovers those windows from the plate's syntax tree.
//! Spans survive *any* content rebuild (a `show`-rule pass that captures
//! paragraphs into a state buffer and re-emits them) because they are a
//! property of the glyph, not a sibling element a rebuild can drop.
//!
//! **First placement only.** A window's region is its first maximal run of
//! consecutive matching frame items in document order — one region per page
//! that run touches, in page order. Span data cannot distinguish "package
//! chrome between two paragraphs of one placement" from "a second placement of
//! the same value" (both are a gap of foreign spans), so later runs are not
//! enumerated; the first run is provably the true start of the field's
//! content, and shrinks (never lies) when foreign ink interrupts it. A scalar
//! referenced at several distinct plate sites is the exception that costs
//! nothing: each site is its own window, so each surfaces independently.
//!
//! Resolution uses [`WorldExt::range`] — the same helper `error_mapping.rs`
//! uses for diagnostics. Geometry composes the group-transform stack exactly
//! like `typst_layout::introspect::discover_frame`, transforming all four
//! corners of each item box (the stack may rotate or scale).

use std::collections::HashMap;
use std::ops::Range;

use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{FileId, LinkedNode, Source, Span, SyntaxKind};
use typst::WorldExt;
use typst_layout::PagedDocument;

use quillmark_core::RenderedRegion;

use crate::world::QuillWorld;

/// A tracked byte window in a compiled source: the schema field whose content
/// resolves into `range` of `file`. Content fields point at their generated
/// eval call site in the helper `lib.typ`; scalar reference sites point at
/// their expression in the plate.
#[derive(Debug, Clone)]
pub(crate) struct FieldWindow {
    pub path: String,
    pub file: FileId,
    pub range: Range<usize>,
}

/// An axis-aligned box accumulated in page-space (top-left origin) pt.
#[derive(Clone, Copy)]
struct Aabb {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Aabb {
    fn of(corners: [Point; 4], ts: Transform) -> Self {
        let mut b = Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        };
        for c in corners {
            let p = c.transform(ts);
            let (x, y) = (p.x.to_pt(), p.y.to_pt());
            b.min_x = b.min_x.min(x);
            b.min_y = b.min_y.min(y);
            b.max_x = b.max_x.max(x);
            b.max_y = b.max_y.max(y);
        }
        b
    }

    fn union(&mut self, o: Aabb) {
        self.min_x = self.min_x.min(o.min_x);
        self.min_y = self.min_y.min(o.min_y);
        self.max_x = self.max_x.max(o.max_x);
        self.max_y = self.max_y.max(o.max_y);
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        self.min_x <= x && x <= self.max_x && self.min_y <= y && y <= self.max_y
    }
}

/// One drawn frame item, classified: which tracked window (if any) its span
/// resolved into, and its page-space box.
struct Hit {
    page: usize,
    window: Option<usize>,
    rect: Aabb,
}

/// Memoizing span → window-index classifier. Uniform eval spans repeat across
/// every glyph of a field's content, so the `world.range` lookup runs once per
/// distinct span, not once per glyph.
struct Classifier<'a> {
    world: &'a QuillWorld,
    windows: &'a [FieldWindow],
    memo: HashMap<Span, Option<usize>>,
}

impl Classifier<'_> {
    fn classify(&mut self, span: Span) -> Option<usize> {
        if let Some(&w) = self.memo.get(&span) {
            return w;
        }
        let resolved = span.id().zip(self.world.range(span));
        let w = resolved.and_then(|(file, range)| {
            self.windows.iter().position(|win| {
                win.file == file && win.range.start <= range.start && range.end <= win.range.end
            })
        });
        self.memo.insert(span, w);
        w
    }
}

/// Walk every page in document order, emitting one [`Hit`] per drawn item —
/// per glyph for text (a text run may mix spans), per item for shapes and
/// images (each carries a single span).
fn collect_hits(doc: &PagedDocument, cls: &mut Classifier, out: &mut Vec<Hit>) {
    fn walk(frame: &Frame, ts: Transform, page: usize, cls: &mut Classifier, out: &mut Vec<Hit>) {
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Group(group) => {
                    let ts = ts
                        .pre_concat(Transform::translate(pos.x, pos.y))
                        .pre_concat(group.transform);
                    walk(&group.frame, ts, page, cls, out);
                }
                FrameItem::Text(text) => {
                    let bb = text.bbox();
                    let mut cursor = Point::zero();
                    for glyph in &text.glyphs {
                        let advance = Point::new(
                            glyph.x_advance.at(text.size),
                            glyph.y_advance.at(text.size),
                        );
                        let offset =
                            Point::new(glyph.x_offset.at(text.size), glyph.y_offset.at(text.size));
                        let lo = Point::new(cursor.x + offset.x, cursor.y + bb.min.y);
                        let hi =
                            Point::new(cursor.x + offset.x + advance.x, cursor.y + bb.max.y);
                        out.push(Hit {
                            page,
                            window: cls.classify(glyph.span.0),
                            rect: item_aabb(*pos, lo, hi, ts),
                        });
                        cursor += advance;
                    }
                }
                FrameItem::Shape(shape, span) => {
                    let bb = shape.geometry.bbox(shape.stroke.as_ref());
                    out.push(Hit {
                        page,
                        window: cls.classify(*span),
                        rect: item_aabb(*pos, bb.min, bb.max, ts),
                    });
                }
                FrameItem::Image(_, size, span) => {
                    out.push(Hit {
                        page,
                        window: cls.classify(*span),
                        rect: item_aabb(*pos, Point::zero(), size.to_point(), ts),
                    });
                }
                _ => {}
            }
        }
    }

    for (page, p) in doc.pages().iter().enumerate() {
        walk(&p.frame, Transform::identity(), page, cls, out);
    }
}

/// An item box (corners `lo`..`hi` relative to the item anchor `pos`, in local
/// frame space) mapped to page space via `ts`. All four corners transform —
/// `ts` may rotate or scale.
fn item_aabb(pos: Point, lo: Point, hi: Point, ts: Transform) -> Aabb {
    Aabb::of(
        [
            Point::new(pos.x + lo.x, pos.y + lo.y),
            Point::new(pos.x + hi.x, pos.y + lo.y),
            Point::new(pos.x + lo.x, pos.y + hi.y),
            Point::new(pos.x + hi.x, pos.y + hi.y),
        ],
        ts,
    )
}

/// Per-window first-run tracking state.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    NotSeen,
    InRun,
    Done,
}

/// Scan the compiled document and return each window's **first placement** —
/// one [`RenderedRegion`] per page the placement's run touches, PDF
/// bottom-left rects, sorted (page, field, window order). Best-effort like the
/// widget path: an unresolvable span simply matches no window.
pub(crate) fn scan(
    doc: &PagedDocument,
    world: &QuillWorld,
    windows: &[FieldWindow],
) -> Vec<RenderedRegion> {
    if windows.is_empty() {
        return Vec::new();
    }
    let mut cls = Classifier {
        world,
        windows,
        memo: HashMap::new(),
    };
    let mut hits = Vec::new();
    collect_hits(doc, &mut cls, &mut hits);

    // First maximal run per window: any intervening foreign item closes it.
    // Pages within a run stay in walk order (nondecreasing), so per-page boxes
    // are pushed, not keyed.
    let mut state = vec![Run::NotSeen; windows.len()];
    let mut boxes: Vec<Vec<(usize, Aabb)>> = vec![Vec::new(); windows.len()];
    for hit in &hits {
        for (i, st) in state.iter_mut().enumerate() {
            if hit.window == Some(i) {
                match *st {
                    Run::NotSeen | Run::InRun => {
                        *st = Run::InRun;
                        match boxes[i].last_mut() {
                            Some((page, b)) if *page == hit.page => b.union(hit.rect),
                            _ => boxes[i].push((hit.page, hit.rect)),
                        }
                    }
                    Run::Done => {}
                }
            } else if *st == Run::InRun {
                *st = Run::Done;
            }
        }
    }

    let mut out: Vec<(RenderedRegion, usize)> = Vec::new();
    for (i, window) in windows.iter().enumerate() {
        for (page, b) in &boxes[i] {
            let Some(page_h) = doc.pages().get(*page).map(|p| p.frame.size().y.to_pt()) else {
                continue;
            };
            out.push((
                RenderedRegion {
                    field: window.path.clone(),
                    page: *page,
                    rect: [
                        b.min_x as f32,
                        (page_h - b.max_y) as f32,
                        b.max_x as f32,
                        (page_h - b.min_y) as f32,
                    ],
                },
                i,
            ));
        }
    }
    out.sort_by(|(a, ai), (b, bi)| (a.page, &a.field, *ai).cmp(&(b.page, &b.field, *bi)));
    out.into_iter().map(|(r, _)| r).collect()
}

/// The schema field under a point — the forward (click → field) direction.
/// `x`/`y` are PDF points with a **bottom-left** origin, the same convention
/// as [`RenderedRegion::rect`]. Later-painted items win when boxes overlap.
/// Unlike [`scan`], every placement answers, not just the first: a concrete
/// point identifies one frame item, whose span is unambiguous however many
/// times its field is placed.
pub(crate) fn field_at(
    doc: &PagedDocument,
    world: &QuillWorld,
    windows: &[FieldWindow],
    page: usize,
    x: f32,
    y: f32,
) -> Option<String> {
    if windows.is_empty() {
        return None;
    }
    let page_h = doc.pages().get(page)?.frame.size().y.to_pt();
    let (x, y) = (x as f64, page_h - y as f64);

    let mut cls = Classifier {
        world,
        windows,
        memo: HashMap::new(),
    };
    let mut hits = Vec::new();
    collect_hits(doc, &mut cls, &mut hits);

    hits.iter()
        .rev()
        .find(|h| h.page == page && h.window.is_some() && h.rect.contains(x, y))
        .and_then(|h| h.window)
        .map(|w| windows[w].path.clone())
}

/// Byte windows for the plate's direct scalar references: every
/// `data.<field>` / `data.at("<field>")` expression whose field is a declared
/// schema field, widened to the outermost postfix chain it heads
/// (`data.refs.at(0)`, `data.name.upper()` — the produced value carries the
/// chain's span, not the inner access's). Each site is its own window, so a
/// field referenced in both a header and a footer surfaces both sites.
/// Content-field references also match harmlessly: their glyphs carry the
/// helper eval-site span, which no plate window contains.
///
/// Indirection is not chased: a value laundered through `#let s = data.x`
/// carries the binding's span, not a `data.x` site. Card fields read from the
/// per-card loop variable (`card.from`) have one shared expression site across
/// every card instance — no per-instance identity exists in span data — so
/// they are not tracked; a card *content* field is covered by its per-instance
/// generated eval site instead.
pub(crate) fn scalar_windows(source: &Source, fields: &[String]) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    walk_scalars(&LinkedNode::new(source.root()), fields, &mut out);
    out
}

fn walk_scalars(node: &LinkedNode, fields: &[String], out: &mut Vec<(String, Range<usize>)>) {
    if let Some((path, anchor)) = data_access(node, fields) {
        let mut window = anchor;
        // Widen to the outermost postfix chain headed by this access: climb
        // while the parent is a field access / method call / call on it.
        while let Some(parent) = window.parent() {
            match parent.kind() {
                SyntaxKind::FieldAccess | SyntaxKind::FuncCall => window = parent.clone(),
                _ => break,
            }
        }
        out.push((path, window.range()));
        // Children of a matched access cannot head another `data.` chain.
        return;
    }
    for child in node.children() {
        walk_scalars(&child, fields, out);
    }
}

/// If `node` is a `data.<field>` access or a `data.at("<field>")` call head
/// with a declared field, its schema path and the node to widen from.
fn data_access<'a>(
    node: &LinkedNode<'a>,
    fields: &[String],
) -> Option<(String, LinkedNode<'a>)> {
    if node.kind() != SyntaxKind::FieldAccess {
        return None;
    }
    let access = node.cast::<ast::FieldAccess>()?;
    let ast::Expr::Ident(target) = access.target() else {
        return None;
    };
    if target.as_str() != "data" {
        return None;
    }
    let field = access.field();
    if fields.iter().any(|f| f == field.as_str()) {
        return Some((field.as_str().to_string(), node.clone()));
    }
    // `data.at("field")`: the parent call carries the field name as its first
    // positional string argument.
    if field.as_str() == "at" {
        let parent = node.parent()?;
        let call = parent.cast::<ast::FuncCall>()?;
        let ast::Expr::FieldAccess(callee) = call.callee() else {
            return None;
        };
        if callee.to_untyped() != node.get() {
            return None;
        }
        let first = call.args().items().find_map(|arg| match arg {
            ast::Arg::Pos(ast::Expr::Str(s)) => Some(s.get().to_string()),
            _ => None,
        })?;
        if fields.iter().any(|f| *f == first) {
            return Some((first, parent.clone()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_document;
    use crate::world::QuillWorld;
    use quillmark_core::{FileTreeNode, Quill};
    use std::collections::HashMap as Map;
    use typst::World;

    fn quill(yaml: &str, plate: &str) -> Quill {
        let mut files = Map::new();
        files.insert(
            "Quill.yaml".to_string(),
            FileTreeNode::File {
                contents: yaml.as_bytes().to_vec(),
            },
        );
        files.insert(
            "plate.typ".to_string(),
            FileTreeNode::File {
                contents: plate.as_bytes().to_vec(),
            },
        );
        Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
    }

    /// The premise the whole mechanism stands on: content produced by an
    /// `eval(.., mode: "markup")` call site *inside the generated helper
    /// package file* carries that call site's span, and `world.range`
    /// resolves it into the helper `lib.typ`'s byte space. (The spike
    /// validated plate-file call sites; the helper is a package source, which
    /// this pins down.)
    #[test]
    fn eval_output_spans_resolve_into_the_helper_file() {
        const YAML: &str = r#"
quill:
  name: span_probe
  version: 0.1.0
  backend: typst
  description: helper-file span resolution probe
typst:
  plate_file: plate.typ
main:
  fields:
    intro:
      type: markdown
      description: a probe field
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.intro
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let data = serde_json::json!({ "intro": "A probe paragraph, PROBETOKEN." });
        let (json_str, entries) =
            crate::transformed_data(&schema, &meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world.inject_helper_package(&json_str, &entries);
        let (doc, _) = compile_document(&world).expect("compile");

        let helper_id = QuillWorld::helper_fid("lib.typ");
        let helper_src = world.source(helper_id).expect("helper source");

        // Some glyph resolves into the helper file, inside the recorded eval
        // window for `intro`.
        let win = windows
            .iter()
            .find(|w| w.path == "intro")
            .expect("intro window");
        let mut resolved_in_window = false;
        for (_, page) in doc.pages().iter().enumerate() {
            let mut stack = vec![&page.frame];
            while let Some(frame) = stack.pop() {
                for (_, item) in frame.items() {
                    match item {
                        FrameItem::Group(g) => stack.push(&g.frame),
                        FrameItem::Text(text) => {
                            for glyph in &text.glyphs {
                                let span = glyph.span.0;
                                if span.id() == Some(helper_id) {
                                    if let Some(range) = world.range(span) {
                                        if win.range.start <= range.start
                                            && range.end <= win.range.end
                                        {
                                            resolved_in_window = true;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            resolved_in_window,
            "eval output glyphs must resolve into the helper file's recorded window \
             {:?}; helper source around it: {:?}",
            win.range,
            helper_src
                .text()
                .get(win.range.start.saturating_sub(20)..(win.range.end + 20).min(helper_src.text().len()))
        );
    }

    #[test]
    fn scalar_windows_find_direct_and_at_references_widened_to_chains() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.subject
#data.at("subject")
#data.refs.at(0)
#upper(data.subject)
#let s = data.other
"#,
        );
        let fields = vec![
            "subject".to_string(),
            "refs".to_string(),
            "other".to_string(),
        ];
        let wins = scalar_windows(&src, &fields);
        let text = src.text();
        let spans: Vec<(&str, &str)> = wins
            .iter()
            .map(|(p, r)| (p.as_str(), &text[r.clone()]))
            .collect();
        assert!(
            spans.contains(&("subject", "data.subject")),
            "direct access tracked: {spans:?}"
        );
        assert!(
            spans.contains(&("subject", "data.at(\"subject\")")),
            ".at() access tracked: {spans:?}"
        );
        assert!(
            spans.contains(&("refs", "data.refs.at(0)")),
            "postfix chain widened: {spans:?}"
        );
        // Two direct `data.subject` sites (bare + inside upper()) both track.
        assert_eq!(
            spans.iter().filter(|(p, _)| *p == "subject").count(),
            3,
            "every reference site is its own window: {spans:?}"
        );
        assert!(
            spans.contains(&("other", "data.other")),
            "a let-bound access still tracks its own site: {spans:?}"
        );
    }
}
