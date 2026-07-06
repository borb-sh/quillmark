//! Recover schema-field regions from *glyph spans* — the origin every drawn
//! frame item already carries.
//!
//! Every `Text` glyph (and `Shape`/`Image` item) in the laid-out frames
//! carries a [`Span`] pointing at the source expression that produced it.
//! Content fields are codegen'd as markup **block** bindings (`#let _qm_cN =
//! [ .. ]`) in the generated helper `lib.typ`; the file parser parses each
//! block, so every glyph carries its own syntax node's span (word/run
//! granularity) — all nested inside the block's byte range. The backend
//! records each block's **byte window** at generation time ([`FieldWindow`])
//! and the scan classifies a frame item by which window its resolved range
//! nests inside; per-node spans and a single uniform span both fall inside the
//! same window, so classification is by containment, not identity. A scalar the
//! plate interpolates directly (`#data.subject`) needs no codegen: its glyphs
//! carry a span at or around the reference expression in the plate, and
//! [`scalar_windows`] recovers those windows from the plate's syntax tree.
//! Spans survive *any* content rebuild (a `show`-rule pass that captures
//! paragraphs into a state buffer and re-emits them) because they are a
//! property of the glyph, not a sibling element a rebuild can drop.
//!
//! **Resolution goes through the compile's own helper source.** The session
//! serves reads from its last-good compile even after a failed `apply`, but a
//! failed apply has already written the *next* injection's helper text into
//! the world — resolving the served document's spans against that text would
//! shift or drop every byte range. The scan therefore resolves helper-file
//! spans against the [`Source`] snapshot the served document was compiled
//! from, and only non-helper spans (the plate, vendored packages — sources
//! that never change within a session) through the live world.
//!
//! **First placement only.** A window's region is its first maximal run of
//! consecutive matching frame items in document order — one region per page
//! that run touches, in page order. Span data cannot distinguish "package
//! chrome between two paragraphs of one placement" from "a second placement
//! of the same value" (both are a gap of foreign spans), so later runs are
//! not enumerated; the first run is the true start of the field's content,
//! and shrinks (never lies) when foreign ink interrupts it mid-page. One
//! tolerance keeps continuation pages covered: page marginals (headers,
//! footers, page numbers) walk between one page's body and the next's, so a
//! run interrupted by foreign ink may resume on the **immediately following
//! page** — a same-page gap still ends the run (that is exactly the
//! twice-placed case), at the cost that a *second* placement opening at the
//! top of the next page reads as a continuation (an over-report of that
//! field's own ink, never another field's). A scalar referenced at several
//! distinct plate sites costs nothing: each site is its own window, so each
//! surfaces independently.
//!
//! Geometry composes the group-transform stack exactly like
//! `typst_layout::introspect::discover_frame`, transforming all four corners
//! of each item box (the stack may rotate or scale). Boxes are computed only
//! for classified ink — foreign items matter to the scan solely as
//! run-breakers.

use std::collections::HashMap;
use std::ops::Range;

use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{DiagSpan, DiagSpanKind, FileId, LinkedNode, Source, Span, SyntaxKind};
use typst::World;
use typst_layout::PagedDocument;

use quillmark_core::RenderedRegion;

use crate::world::QuillWorld;

/// A tracked byte window in a compiled source: the schema field whose content
/// resolves into `range` of `file`. Content fields point at their generated
/// markup block (`#let _qm_cN = [ .. ]`) in the helper `lib.typ`; scalar
/// reference sites point at their expression in the plate.
#[derive(Debug, Clone)]
pub(crate) struct FieldWindow {
    pub path: String,
    pub file: FileId,
    pub range: Range<usize>,
    /// The content block's per-segment source map (`gen` ranges index the helper
    /// `lib.typ`), empty for scalar reference sites. Produced by the emitter and
    /// carried here for the Phase-3 region/nav rework, which keys regions to
    /// corpus ranges through these — no reader in Phase 2 yet.
    #[allow(dead_code)]
    pub segments: Vec<crate::emit::SegmentMap>,
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
/// resolved into, and — for classified ink only — its page-space box.
struct Hit {
    page: usize,
    window: Option<usize>,
    rect: Option<Aabb>,
}

/// Memoizing span → window-index classifier. A block's glyphs carry a handful
/// of distinct per-node spans (not one uniform span), so the range lookup runs
/// once per distinct span, not once per glyph. Helper-file spans resolve
/// against the served compile's own source snapshot (see the module doc);
/// everything else against the world.
struct Classifier<'a> {
    world: &'a QuillWorld,
    helper: &'a Source,
    windows: &'a [FieldWindow],
    memo: HashMap<Span, Option<usize>>,
}

impl Classifier<'_> {
    /// Resolve `span` to its byte range in whichever file it came from — the
    /// same unpack `WorldExt::range` performs, with the helper file routed to
    /// the served compile's snapshot instead of the world. Shared by
    /// [`classify`](Self::classify) and the PR-F two-tier segment probe
    /// (`classify_two_tier`, spike-prototype only, `#[cfg(test)]`).
    fn resolve_range(&self, span: Span) -> Option<(FileId, Range<usize>)> {
        match DiagSpan::from(span).get() {
            DiagSpanKind::Detached => None,
            DiagSpanKind::Number { id, num, sub_range } => {
                let range = if id == self.helper.id() {
                    self.helper.range(num, sub_range)
                } else {
                    self.world
                        .source(id)
                        .ok()
                        .and_then(|s| s.range(num, sub_range))
                };
                range.map(|r| (id, r))
            }
            DiagSpanKind::Range { id, range } => Some((id, range)),
        }
    }

    fn classify(&mut self, span: Span) -> Option<usize> {
        if let Some(&w) = self.memo.get(&span) {
            return w;
        }
        let w = self.resolve_range(span).and_then(|(file, range)| {
            self.windows.iter().position(|win| {
                win.file == file && win.range.start <= range.start && range.end <= win.range.end
            })
        });
        self.memo.insert(span, w);
        w
    }
}

/// Walk one page frame in document order, emitting one [`Hit`] per drawn item
/// — per glyph for text (a text run may mix spans), per item for shapes and
/// images (each carries a single span). Boxes are computed for classified
/// ink only.
fn collect_page_hits(frame: &Frame, page: usize, cls: &mut Classifier, out: &mut Vec<Hit>) {
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
                        let window = cls.classify(glyph.span.0);
                        let rect = window.is_some().then(|| {
                            let offset = Point::new(
                                glyph.x_offset.at(text.size),
                                glyph.y_offset.at(text.size),
                            );
                            let lo = Point::new(cursor.x + offset.x, cursor.y + bb.min.y);
                            let hi =
                                Point::new(cursor.x + offset.x + advance.x, cursor.y + bb.max.y);
                            item_aabb(*pos, lo, hi, ts)
                        });
                        out.push(Hit { page, window, rect });
                        cursor += advance;
                    }
                }
                FrameItem::Shape(shape, span) => {
                    let window = cls.classify(*span);
                    let rect = window.is_some().then(|| {
                        let bb = shape.geometry.bbox(shape.stroke.as_ref());
                        item_aabb(*pos, bb.min, bb.max, ts)
                    });
                    out.push(Hit { page, window, rect });
                }
                FrameItem::Image(_, size, span) => {
                    let window = cls.classify(*span);
                    let rect = window
                        .is_some()
                        .then(|| item_aabb(*pos, Point::zero(), size.to_point(), ts));
                    out.push(Hit { page, window, rect });
                }
                _ => {}
            }
        }
    }
    walk(frame, Transform::identity(), page, cls, out);
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

/// Per-window first-run state. The run currently accruing is not represented
/// here — at most one window can be in-run at a time (any hit forecloses
/// every other window's run), so the scan tracks it as a single cursor and
/// this enum carries only the out-of-run states.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    NotSeen,
    /// Interrupted by foreign ink; may resume on page `last_page + 1` only.
    Suspended {
        last_page: usize,
    },
    Done,
}

/// Scan the compiled document and return each window's **first placement** —
/// one [`RenderedRegion`] per page the placement's run touches, PDF
/// bottom-left rects, sorted (page, field, window order). Best-effort like the
/// widget path: an unresolvable span simply matches no window.
pub(crate) fn scan(
    doc: &PagedDocument,
    world: &QuillWorld,
    helper: &Source,
    windows: &[FieldWindow],
) -> Vec<RenderedRegion> {
    if windows.is_empty() {
        return Vec::new();
    }
    let mut cls = Classifier {
        world,
        helper,
        windows,
        memo: HashMap::new(),
    };

    // Single pass in document order: `current` is the one window whose first
    // run is accruing. A hit for another window (or untracked ink) suspends
    // it; a suspended run resumes only on the immediately following page
    // (page-marginal tolerance — see the module doc), otherwise it is done.
    let mut state = vec![Run::NotSeen; windows.len()];
    let mut boxes: Vec<Vec<(usize, Aabb)>> = vec![Vec::new(); windows.len()];
    let mut current: Option<(usize, usize)> = None; // (window, last_page)

    let mut hits = Vec::new();
    for (page, p) in doc.pages().iter().enumerate() {
        collect_page_hits(&p.frame, page, &mut cls, &mut hits);
    }
    for hit in &hits {
        match hit.window {
            Some(i) if current.map(|(c, _)| c) == Some(i) => {
                accrue(&mut boxes[i], hit);
                current = Some((i, hit.page));
            }
            Some(i) => {
                if let Some((c, last_page)) = current.take() {
                    state[c] = Run::Suspended { last_page };
                }
                match state[i] {
                    Run::NotSeen => {
                        accrue(&mut boxes[i], hit);
                        current = Some((i, hit.page));
                    }
                    Run::Suspended { last_page } if hit.page == last_page + 1 => {
                        accrue(&mut boxes[i], hit);
                        current = Some((i, hit.page));
                    }
                    Run::Suspended { .. } => state[i] = Run::Done,
                    Run::Done => {}
                }
            }
            None => {
                if let Some((c, last_page)) = current.take() {
                    state[c] = Run::Suspended { last_page };
                }
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

/// Union `hit` into the run's box for its page, opening a new per-page box at
/// a page transition (pages are nondecreasing in walk order).
fn accrue(boxes: &mut Vec<(usize, Aabb)>, hit: &Hit) {
    let rect = hit.rect.expect("classified hits carry a box");
    match boxes.last_mut() {
        Some((page, b)) if *page == hit.page => b.union(rect),
        _ => boxes.push((hit.page, rect)),
    }
}

/// The schema field under a point — the forward (click → field) direction.
/// `x`/`y` are PDF points with a **bottom-left** origin, the same convention
/// as [`RenderedRegion::rect`]. Unlike [`scan`], every placement answers, not
/// just the first: a concrete point identifies one frame item, whose span is
/// unambiguous however many times its field is placed. Among tracked ink the
/// later-painted item wins; untracked ink never occludes — a decorative
/// overlay does not swallow clicks on the field beneath it.
pub(crate) fn field_at(
    doc: &PagedDocument,
    world: &QuillWorld,
    helper: &Source,
    windows: &[FieldWindow],
    page: usize,
    x: f32,
    y: f32,
) -> Option<String> {
    if windows.is_empty() {
        return None;
    }
    let frame = &doc.pages().get(page)?.frame;
    let page_h = frame.size().y.to_pt();
    let (x, y) = (x as f64, page_h - y as f64);

    let mut cls = Classifier {
        world,
        helper,
        windows,
        memo: HashMap::new(),
    };
    let mut hits = Vec::new();
    collect_page_hits(frame, page, &mut cls, &mut hits);

    hits.iter()
        .rev()
        .find(|h| h.rect.is_some_and(|r| r.contains(x, y)))
        .and_then(|h| h.window)
        .map(|w| windows[w].path.clone())
}

/// Byte windows for the plate's direct scalar references. Two windows per
/// reference site where they differ:
///
/// - the **chain** window — the `data.<field>` / `data.at("<field>")` access
///   widened to the outermost postfix chain it heads (`data.refs.at(0)`,
///   `data.name.upper()`) — matching ink whose span is the reference
///   expression itself; and
/// - the **enclosing-expression** window — widened through surrounding call
///   arguments and operators (`#upper(data.subject)`, `#str(data.count)`) —
///   matching ink stamped with the whole wrapping expression's span. Emitted
///   only when exactly one reference sits inside it: an expression mixing two
///   fields (`data.a + data.b`) has no single owner and is not attributed.
///
/// Chain windows sort first, so ink resolving to the reference itself is
/// never claimed by a wider window. Each reference site is independent — a
/// field shown in both header and footer surfaces both sites. Not chased: a
/// value laundered through `#let s = data.x` carries the binding's span, and
/// card fields read from the per-card loop variable (`card.from`) have one
/// shared expression site across every card instance — no per-instance
/// identity exists in span data; a card *content* field is covered by its
/// per-instance generated eval site instead. Content-field references also
/// match harmlessly: their glyphs carry the helper eval-site span, which no
/// plate window contains.
pub(crate) fn scalar_windows(source: &Source, fields: &[String]) -> Vec<(String, Range<usize>)> {
    let mut anchors: Vec<(String, Range<usize>, Range<usize>)> = Vec::new();
    collect_anchors(&LinkedNode::new(source.root()), fields, &mut anchors);

    let mut out: Vec<(String, Range<usize>)> = anchors
        .iter()
        .map(|(path, chain, _)| (path.clone(), chain.clone()))
        .collect();
    for (path, chain, wide) in &anchors {
        if wide == chain {
            continue;
        }
        let inside = anchors
            .iter()
            .filter(|(_, c, _)| wide.start <= c.start && c.end <= wide.end)
            .count();
        if inside == 1 {
            out.push((path.clone(), wide.clone()));
        }
    }
    out
}

/// Recurse the whole tree collecting `(path, chain range, enclosing range)`
/// per reference site. Recursion continues into matched subtrees — a
/// reference nested in another chain's arguments is its own site.
fn collect_anchors(
    node: &LinkedNode,
    fields: &[String],
    out: &mut Vec<(String, Range<usize>, Range<usize>)>,
) {
    if let Some((path, anchor)) = data_access(node, fields) {
        // Chain: the outermost postfix chain headed by this access.
        let mut chain = anchor.clone();
        while let Some(parent) = chain.parent() {
            match parent.kind() {
                SyntaxKind::FieldAccess | SyntaxKind::FuncCall => chain = parent.clone(),
                _ => break,
            }
        }
        // Enclosing expression: widened through argument and operator
        // context, stopping at any statement/markup boundary.
        let mut wide = chain.clone();
        while let Some(parent) = wide.parent() {
            match parent.kind() {
                SyntaxKind::FieldAccess
                | SyntaxKind::FuncCall
                | SyntaxKind::Args
                | SyntaxKind::Named
                | SyntaxKind::Spread
                | SyntaxKind::Parenthesized
                | SyntaxKind::Unary
                | SyntaxKind::Binary => wide = parent.clone(),
                _ => break,
            }
        }
        out.push((path, chain.range(), wide.range()));
    }
    for child in node.children() {
        collect_anchors(&child, fields, out);
    }
}

/// If `node` is a `data.<field>` access or a `data.at("<field>")` call head
/// with a declared field, its schema path and the node to widen from.
fn data_access<'a>(node: &LinkedNode<'a>, fields: &[String]) -> Option<(String, LinkedNode<'a>)> {
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
        if fields.contains(&first) {
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

    /// The premise the whole mechanism stands on: content produced by a
    /// generated markup **block** binding (`#let _qm_cN = [ .. ]`) resolves
    /// into that block's recorded byte window in the helper `lib.typ` — a
    /// *package* source, not a plate file — through the production classifier.
    #[test]
    fn block_output_spans_resolve_into_the_helper_file() {
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
      type: richtext
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
        // The seam carries the corpus, not markdown.
        let rt = quillmark_richtext::import::from_markdown("A probe paragraph, PROBETOKEN.")
            .expect("import");
        let data =
            serde_json::json!({ "intro": quillmark_richtext::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(&transformed, &meta)
            .expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let intro_idx = windows
            .iter()
            .position(|w| w.path == "intro")
            .expect("intro window");
        let mut cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &windows,
            memo: HashMap::new(),
        };
        let mut hits = Vec::new();
        for (page, p) in doc.pages().iter().enumerate() {
            collect_page_hits(&p.frame, page, &mut cls, &mut hits);
        }
        assert!(
            hits.iter().any(|h| h.window == Some(intro_idx)),
            "block output glyphs must classify into the helper file's recorded window {:?}",
            windows[intro_idx].range
        );
    }

    #[test]
    fn scalar_windows_track_chains_and_single_owner_enclosing_expressions() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.subject
#data.at("subject")
#data.refs.at(0)
#upper(data.subject)
#(data.subject + data.other)
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
        for expected in [
            ("subject", "data.subject"),
            ("subject", "data.at(\"subject\")"),
            ("refs", "data.refs.at(0)"),
            ("other", "data.other"),
            // A wrapping call with a single reference owns its whole
            // expression: ink stamped with the outer call's span attributes
            // to the field.
            ("subject", "upper(data.subject)"),
        ] {
            assert!(spans.contains(&expected), "missing {expected:?}: {spans:?}");
        }
        // An expression mixing two fields has no single owner — no enclosing
        // window for either.
        assert!(
            !spans
                .iter()
                .any(|(_, t)| t.contains("data.subject + data.other")),
            "multi-reference expressions are not attributed: {spans:?}"
        );
        // Chain windows precede enclosing-expression windows, so ink at the
        // reference itself is never claimed by a wider window.
        let chain_pos = spans
            .iter()
            .position(|s| *s == ("subject", "data.subject"))
            .unwrap();
        let wide_pos = spans
            .iter()
            .position(|s| *s == ("subject", "upper(data.subject)"))
            .unwrap();
        assert!(chain_pos < wide_pos, "chains sort before wides: {spans:?}");
    }

    // -----------------------------------------------------------------
    // PR-F spike probe (Unknown 1) — two-tier `(window, Option<segment>)`
    // classification and the run-machine transparency question.
    //
    // Not wired into `scan`/`field_at`: a standalone proof that (a) the real
    // `Classifier`, extended to search a window's `segments`, produces the
    // three-way outcome PR-F needs, and (b) the single-cursor run machine
    // needs exactly one new arm — a transparent `(window, None)` case scoped
    // to the *same* window as whatever segment is currently accruing — to
    // stay correct. See `prose/plans/richtext/pr-f-spike-findings.md`.
    // -----------------------------------------------------------------

    /// Two-tier classification of a resolved span: which field window it
    /// falls in, and — if it also nests inside one of that window's
    /// `segments` — which one. `Some((w, None))` is block ink between
    /// segments (list markers, container-open syntax): the field's own ink,
    /// attributable to no specific segment.
    fn classify_two_tier(cls: &Classifier, span: Span) -> Option<(usize, Option<usize>)> {
        cls.resolve_range(span).and_then(|(file, range)| {
            cls.windows
                .iter()
                .position(|w| {
                    w.file == file && w.range.start <= range.start && range.end <= w.range.end
                })
                .map(|i| {
                    let seg = cls.windows[i]
                        .segments
                        .iter()
                        .position(|s| s.gen.start <= range.start && range.end <= s.gen.end);
                    (i, seg)
                })
        })
    }

    /// Every drawn item's span in a frame, geometry dropped — classification
    /// is all this probe needs.
    fn collect_spans(frame: &Frame, out: &mut Vec<Span>) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Group(group) => collect_spans(&group.frame, out),
                FrameItem::Text(text) => out.extend(text.glyphs.iter().map(|g| g.span.0)),
                FrameItem::Shape(_, span) => out.push(*span),
                FrameItem::Image(_, _, span) => out.push(*span),
                _ => {}
            }
        }
    }

    /// A real two-item list lowers to two segments (`segment_shape` in
    /// `emit.rs` pins this). Proves the two-tier construction classifies real
    /// compiled output correctly on the half that *is* exercised: each
    /// item's own ink resolves to its own segment.
    ///
    /// The other half — genuine `(window, None)` ink from container-open
    /// syntax — does **not** materialize here: Typst's synthesized list
    /// marker carries a **detached** span (`DiagSpanKind::Detached`, printed
    /// as `Span(1)` below), not one resolving into the helper file, so it
    /// lands in the plain "no window at all" bucket alongside package
    /// chrome — already-correct, unchanged behavior. A block-quote wrapper
    /// (`#quote(block: true)[...]`) draws no extra ink at all by default. Both
    /// were probed by hand (temporarily swapping this test's markdown input
    /// and eyeballing `Classifier::resolve_range` per glyph) before writing
    /// this comment; see the findings doc for the transcript. The
    /// `(window, None)` *mechanism* is still real and still needed — proved
    /// directly, independent of whichever container syntax does or doesn't
    /// exercise it today, by
    /// [`classify_two_tier_resolves_field_only_ink_between_segments`].
    #[test]
    fn two_tier_classification_resolves_each_segment_independently() {
        const YAML: &str = r#"
quill:
  name: two_tier_probe
  version: 0.1.0
  backend: typst
  description: PR-F Unknown-1 two-tier classification probe
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a two-item list
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.body
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let rt =
            quillmark_richtext::import::from_markdown("- Item ONE\n- Item TWO").expect("import");
        let data =
            serde_json::json!({ "body": quillmark_richtext::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(&transformed, &meta)
            .expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let win_idx = windows
            .iter()
            .position(|w| w.path == "body")
            .expect("body window");
        assert_eq!(
            windows[win_idx].segments.len(),
            2,
            "one segment per list item"
        );

        let cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &windows,
            memo: HashMap::new(),
        };
        let mut spans = Vec::new();
        for p in doc.pages().iter() {
            collect_spans(&p.frame, &mut spans);
        }

        let mut seg_hits = [0usize; 2];
        let mut field_only_hits = 0usize;
        let mut untracked_hits = 0usize;
        for span in spans {
            match classify_two_tier(&cls, span) {
                Some((w, Some(s))) if w == win_idx => seg_hits[s] += 1,
                Some((w, None)) if w == win_idx => field_only_hits += 1,
                _ => untracked_hits += 1,
            }
        }
        assert!(
            seg_hits[0] > 0 && seg_hits[1] > 0,
            "each list item's own ink resolves to its own segment: {seg_hits:?}"
        );
        assert_eq!(
            field_only_hits, 0,
            "list markers do not produce (window, None) ink — see the doc comment"
        );
        assert!(
            untracked_hits > 0,
            "the two markers are hit but resolve to no window (detached span)"
        );
    }

    /// The mechanism itself, independent of whether any *current* `emit.rs`
    /// container produces it: a real, resolvable span strictly between two
    /// recorded segments, but still inside the block window, must classify
    /// `(window, None)`.
    ///
    /// A real compile of "before **BOLD** after" gives three distinct,
    /// genuinely resolvable spans in one paragraph (a bold run is its own
    /// content child, so it and its plain-text neighbors carry different
    /// spans — unlike the undifferentiated multi-word prose in the sibling
    /// test above, which Typst folds into one span per paragraph). This test
    /// takes those three real spans and re-windows them by hand — segment 0
    /// = "before", segment 1 = "after", the bold run deliberately excluded —
    /// so the excluded span must classify `(window, None)`: real Typst spans,
    /// a synthetic window, proving `classify_two_tier`'s containment logic
    /// without depending on which container syntax does or doesn't produce
    /// such ink today.
    #[test]
    fn classify_two_tier_resolves_field_only_ink_between_segments() {
        const YAML: &str = r#"
quill:
  name: two_tier_mechanism_probe
  version: 0.1.0
  backend: typst
  description: PR-F Unknown-1 classify_two_tier mechanism probe
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: one paragraph, one bold run
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.body
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let rt =
            quillmark_richtext::import::from_markdown("before **BOLD** after").expect("import");
        let data =
            serde_json::json!({ "body": quillmark_richtext::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(&transformed, &meta)
            .expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");
        let real_win = windows
            .iter()
            .find(|w| w.path == "body")
            .expect("body window");
        assert_eq!(real_win.segments.len(), 1, "one paragraph, one segment");

        let cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &windows,
            memo: HashMap::new(),
        };
        let mut spans = Vec::new();
        for p in doc.pages().iter() {
            collect_spans(&p.frame, &mut spans);
        }
        // Group by resolved (file, range) in first-seen (document) order —
        // one entry per distinct real Typst node inside the field's window.
        let mut nodes: Vec<(Span, Range<usize>)> = Vec::new();
        for span in spans {
            if let Some((file, range)) = cls.resolve_range(span) {
                if file == real_win.file
                    && real_win.range.start <= range.start
                    && range.end <= real_win.range.end
                    && !nodes.iter().any(|(_, r)| *r == range)
                {
                    nodes.push((span, range));
                }
            }
        }
        nodes.sort_by_key(|(_, r)| r.start);
        // Word-level granularity: plain text and the space beside it are
        // separate nodes too (an empirical Unknown-2 finding — see the
        // findings doc), so "before" contributes more than one node. Split
        // on the node whose text is exactly "BOLD".
        let text = helper.text();
        let bold_idx = nodes
            .iter()
            .position(|(_, r)| &text[r.clone()] == "BOLD")
            .expect("a node holding exactly \"BOLD\": {nodes:?}");
        assert!(
            bold_idx > 0 && bold_idx + 1 < nodes.len(),
            "ink on both sides of BOLD: {nodes:?}"
        );
        let before = &nodes[..bold_idx];
        let (bold_span, _) = nodes[bold_idx].clone();
        let after = &nodes[bold_idx + 1..];

        // Re-window by hand: segment 0 spans the "before" nodes, segment 1
        // spans the "after" nodes, the bold run deliberately excluded from
        // both — the boundaries a real two-tier classifier would compute
        // from `emit.rs`'s recorded segment range, reconstructed here from
        // the real node ranges either side of it.
        let synthetic = vec![FieldWindow {
            path: "body".to_string(),
            file: real_win.file,
            range: real_win.range.clone(),
            segments: vec![
                crate::emit::SegmentMap {
                    corpus: 0..0,
                    gen: before.first().unwrap().1.start..before.last().unwrap().1.end,
                    runs: vec![],
                },
                crate::emit::SegmentMap {
                    corpus: 0..0,
                    gen: after.first().unwrap().1.start..after.last().unwrap().1.end,
                    runs: vec![],
                },
            ],
        }];
        let synthetic_cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &synthetic,
            memo: HashMap::new(),
        };
        for (span, range) in before {
            assert_eq!(
                classify_two_tier(&synthetic_cls, *span),
                Some((0, Some(0))),
                "a \"before\"-side node ({range:?}) -> segment 0"
            );
        }
        for (span, range) in after {
            assert_eq!(
                classify_two_tier(&synthetic_cls, *span),
                Some((0, Some(1))),
                "an \"after\"-side node ({range:?}) -> segment 1"
            );
        }
        assert_eq!(
            classify_two_tier(&synthetic_cls, bold_span),
            Some((0, None)),
            "the excluded bold run sits inside the block window but outside every segment"
        );
    }

    /// One classified hit in the flattened run machine's synthetic replay —
    /// the three outcomes `classify_two_tier` can produce, paired with the
    /// page the ink falls on.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum TierHit {
        /// `(window, Some(segment))` — a genuine segment hit.
        Segment(usize, usize),
        /// `(window, None)` — block ink between segments: the field's own
        /// ink, not attributable to any one segment.
        FieldOnly(usize),
        /// No window at all — untracked ink (today's plain `None` arm).
        Foreign,
    }

    /// The flattened two-tier run machine: same states (`Run::NotSeen` /
    /// `Suspended` / `Done`), same single global cursor, same page-`+1`
    /// continuation tolerance as `scan`'s loop above — plus the one arm
    /// `scan` today has no path for. `FieldOnly(w)` is a genuine no-op
    /// (`current` is untouched) exactly when `current` already belongs to
    /// window `w`; otherwise it is indistinguishable from foreign ink and
    /// must still suspend, or a second placement of an *unrelated* running
    /// field could merge across the gap into one lying box (the invariant
    /// `content_regions.rs::field_placed_twice_surfaces_first_region_...`
    /// pins for the whole-field case). Returns, per `(window, segment)` key,
    /// the pages each of its accruals landed on, in order — a stand-in for
    /// `scan`'s per-page box union, since geometry is irrelevant here.
    fn run_two_tier(hits: &[(usize, TierHit)]) -> HashMap<(usize, usize), Vec<usize>> {
        let mut state: HashMap<(usize, usize), Run> = HashMap::new();
        let mut boxes: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        let mut current: Option<((usize, usize), usize)> = None;

        for &(page, hit) in hits {
            match hit {
                TierHit::Segment(w, s) => {
                    let key = (w, s);
                    if current.map(|(c, _)| c) == Some(key) {
                        boxes.entry(key).or_default().push(page);
                        current = Some((key, page));
                    } else {
                        if let Some((c, last_page)) = current.take() {
                            state.insert(c, Run::Suspended { last_page });
                        }
                        match state.get(&key).copied().unwrap_or(Run::NotSeen) {
                            Run::NotSeen => {
                                boxes.entry(key).or_default().push(page);
                                current = Some((key, page));
                            }
                            Run::Suspended { last_page } if page == last_page + 1 => {
                                boxes.entry(key).or_default().push(page);
                                current = Some((key, page));
                            }
                            Run::Suspended { .. } => {
                                state.insert(key, Run::Done);
                            }
                            Run::Done => {}
                        }
                    }
                }
                TierHit::FieldOnly(w) => match current {
                    Some((c, _)) if c.0 == w => {} // transparent: same field, no segment
                    _ => {
                        if let Some((c, last_page)) = current.take() {
                            state.insert(c, Run::Suspended { last_page });
                        }
                    }
                },
                TierHit::Foreign => {
                    if let Some((c, last_page)) = current.take() {
                        state.insert(c, Run::Suspended { last_page });
                    }
                }
            }
        }
        boxes
    }

    /// The crux: `FieldOnly` ink between two hits of the *same* segment must
    /// not break its run, while genuinely untracked ink in the identical
    /// shape still does — proving the new arm is exactly the missing
    /// non-suspending path, without weakening suspension for ink that is not
    /// this field's own.
    #[test]
    fn field_only_ink_is_transparent_but_foreign_ink_is_not() {
        let fixed = vec![
            (0, TierHit::Segment(0, 0)),
            (0, TierHit::FieldOnly(0)),
            (0, TierHit::Segment(0, 0)),
        ];
        let boxes = run_two_tier(&fixed);
        assert_eq!(
            boxes[&(0, 0)],
            vec![0, 0],
            "field-only ink never breaks segment 0's run"
        );

        let foreign = vec![
            (0, TierHit::Segment(0, 0)),
            (0, TierHit::Foreign),
            (0, TierHit::Segment(0, 0)),
        ];
        let boxes = run_two_tier(&foreign);
        assert_eq!(
            boxes[&(0, 0)],
            vec![0],
            "genuinely untracked ink on the same page still ends the run, unresumed \
             (no page turn to satisfy the marginal tolerance) — exactly today's rule"
        );
    }

    /// Two adjacent segments of one field: the first segment's run and the
    /// second segment's run are tracked independently, exactly as two
    /// distinct top-level fields are today.
    #[test]
    fn adjacent_segments_of_one_field_run_independently() {
        let hits = vec![
            (0, TierHit::Segment(0, 0)),
            (0, TierHit::FieldOnly(0)), // e.g. the second item's bullet marker
            (0, TierHit::Segment(0, 1)),
        ];
        let boxes = run_two_tier(&hits);
        assert_eq!(boxes[&(0, 0)], vec![0]);
        assert_eq!(boxes[&(0, 1)], vec![0]);
    }

    /// The scoping the plan text does not spell out: transparency is
    /// relative to a *same-window* current run only. A different field's
    /// segment mid-run, interrupted by *this* field's own field-only ink,
    /// must still suspend — otherwise an interleaved second placement of the
    /// running field could merge across the gap into one lying box.
    #[test]
    fn field_only_ink_still_suspends_a_different_fields_current_run() {
        let hits = vec![
            (0, TierHit::Segment(1, 0)), // field 1's segment 0 starts, page 0
            (0, TierHit::FieldOnly(0)),  // field 0's own structural ink
            (1, TierHit::Segment(1, 0)), // field 1's segment 0 resumes, page 1
        ];
        let boxes = run_two_tier(&hits);
        assert_eq!(
            boxes[&(1, 0)],
            vec![0, 1],
            "a foreign field's field-only ink suspends the running field \
             (the page-turn tolerance still lets it resume)"
        );

        let same_page = vec![
            (0, TierHit::Segment(1, 0)),
            (0, TierHit::FieldOnly(0)),
            (0, TierHit::Segment(1, 0)),
        ];
        let boxes = run_two_tier(&same_page);
        assert_eq!(
            boxes[&(1, 0)],
            vec![0],
            "no same-page resume — field-only ink is not a wildcard exception \
             to the foreign-ink suspension rule"
        );
    }

    // -----------------------------------------------------------------
    // PR-F spike probe (Unknown 2, risk register risk 2) — does
    // `glyph.span.1` give usable per-character intra-node offsets, and
    // where does it degrade (raw string literals, list/enum numbering,
    // shaping clusters)? See `prose/plans/richtext/pr-f-spike-findings.md`
    // for the full write-up; this test pins the empirical findings so a
    // future Typst upgrade cannot silently change them unnoticed.
    // -----------------------------------------------------------------

    /// One glyph's classification-relevant facts: the resolved node range
    /// (`glyph.span.0`, unpacked), the intra-node offset (`glyph.span.1`),
    /// and the glyph's own text slice (via `glyph.range()` into the
    /// `TextItem`'s text) — enough to check whether `node.start + offset`
    /// lands on the right generated byte, without needing the geometry
    /// `collect_page_hits` computes.
    struct GlyphProbe {
        node: Range<usize>,
        offset: u16,
        text: String,
    }

    fn collect_glyph_probes(
        frame: &Frame,
        cls: &Classifier,
        win: &FieldWindow,
        out: &mut Vec<GlyphProbe>,
    ) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Group(group) => collect_glyph_probes(&group.frame, cls, win, out),
                FrameItem::Text(text) => {
                    for g in &text.glyphs {
                        if let Some((file, range)) = cls.resolve_range(g.span.0) {
                            if file == win.file
                                && win.range.start <= range.start
                                && range.end <= win.range.end
                            {
                                out.push(GlyphProbe {
                                    node: range,
                                    offset: g.span.1,
                                    text: text.text[g.range()].to_string(),
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A formatted paragraph (plain text + one `#strong[...]` run) plus a
    /// multi-line code fence, in one field — the two constructs the plan's
    /// risk 2 names. Findings pinned here (see the module-level comment
    /// above for the full write-up):
    ///
    /// - **Plain / bold markup text**: `glyph.span.1` is exact, byte-
    ///   granular, per character. Every resolved node nests inside exactly
    ///   one recorded `run`, and `run.gen.start + offset` (which for markup
    ///   text equals `node.start + offset`, since Typst's own node range is
    ///   already tight around that specific run) lands on the correct
    ///   generated byte, for every glyph tested.
    /// - **A multi-line `#raw(block: true, "...")` string literal**: every
    ///   line's glyphs resolve to the **identical** node range (the whole
    ///   call expression, not the string literal or any one line), and
    ///   `span.1` **resets to 0 at each physical line**. Two consequences,
    ///   pinned below: (a) `span.0` alone cannot tell which of the fence's N
    ///   lines a hit belongs to — the same `(node, offset)` pair is
    ///   ambiguous between lines; (b) the resolved node range does not fit
    ///   inside *any* recorded run's `gen` range (it is wider than every one
    ///   of them), so per-run inversion structurally fails — not just loses
    ///   precision. It *does* still fit inside the segment's `gen` range, so
    ///   segment-level classification (which segment, i.e. which field's
    ///   code fence) remains correct; only the finer run/line/char answer is
    ///   unavailable.
    #[test]
    fn glyph_span_1_precision_findings() {
        const YAML: &str = r#"
quill:
  name: span_precision_probe
  version: 0.1.0
  backend: typst
  description: PR-F Unknown-2 glyph.span.1 precision probe
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: a formatted paragraph plus a multi-line code fence
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.body
"#;
        // "difficult fickle" carries two "fi"/"ff"-adjacent clusters, probing
        // (inconclusively, see the findings doc) for shaping-ligature
        // collapse under Typst's default font.
        let md = "This is **bold** difficult fickle text.\n\n```\nfn add(a, b) {\n    return a + b;\n}\n```";
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let rt = quillmark_richtext::import::from_markdown(md).expect("import");
        let data =
            serde_json::json!({ "body": quillmark_richtext::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&meta, &data).expect("transform");
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(&transformed, &meta)
            .expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");
        let win = windows
            .iter()
            .find(|w| w.path == "body")
            .expect("body window");
        assert_eq!(
            win.segments.len(),
            2,
            "one paragraph segment, one code segment"
        );
        let (para_seg, code_seg) = (&win.segments[0], &win.segments[1]);
        assert!(
            code_seg.runs.len() >= 2,
            "the fence's multiple lines each recorded their own run: {:?}",
            code_seg.runs
        );

        let cls = Classifier {
            world: &world,
            helper: &helper,
            windows: &windows,
            memo: HashMap::new(),
        };
        let mut probes = Vec::new();
        for p in doc.pages().iter() {
            collect_glyph_probes(&p.frame, &cls, win, &mut probes);
        }
        assert!(!probes.is_empty(), "the field must place some glyphs");

        // ---- plain/bold markup text: node.start + offset is exact ----
        let mut checked_para_glyph = false;
        for probe in &probes {
            if probe.node.start >= para_seg.gen.start && probe.node.end <= para_seg.gen.end {
                // The node nests inside exactly one recorded run, and that
                // run's own `gen.start` is what `offset` is relative to.
                let owning_run = para_seg
                    .runs
                    .iter()
                    .find(|(_, gen, _)| gen.start <= probe.node.start && probe.node.end <= gen.end)
                    .unwrap_or_else(|| {
                        panic!("markup node {:?} must nest inside some run", probe.node)
                    });
                let absolute = probe.node.start + probe.offset as usize;
                assert!(
                    absolute >= owning_run.1.start && absolute < owning_run.1.end + 1,
                    "node.start + offset ({absolute}) must land inside the owning run {:?} for {:?}",
                    owning_run.1,
                    probe.text,
                );
                // Byte-exact: the character at `absolute` in the generated
                // source is exactly this glyph's own text.
                assert_eq!(
                    &helper.text()[absolute..absolute + probe.text.len()],
                    probe.text,
                    "node.start + offset must point at this glyph's own bytes"
                );
                checked_para_glyph = true;
            }
        }
        assert!(
            checked_para_glyph,
            "at least one paragraph glyph must be checked"
        );

        // ---- multi-line #raw string literal: the per-line collapse ----
        let code_probes: Vec<&GlyphProbe> = probes
            .iter()
            .filter(|p| p.node.start >= code_seg.gen.start && p.node.end <= code_seg.gen.end)
            .collect();
        assert!(!code_probes.is_empty(), "the code fence must place glyphs");

        // (a) every line shares the identical resolved node — span.0 alone
        // cannot disambiguate which line a hit belongs to.
        let distinct_nodes: std::collections::HashSet<Range<usize>> =
            code_probes.iter().map(|p| p.node.clone()).collect();
        assert_eq!(
            distinct_nodes.len(),
            1,
            "every raw-block glyph shares one node range regardless of physical line: {distinct_nodes:?}"
        );
        let raw_node = distinct_nodes.into_iter().next().unwrap();

        // (b) offset resets to 0 more than once — once per physical line,
        // not once for the whole multi-line literal.
        let resets = code_probes
            .windows(2)
            .filter(|w| w[1].offset == 0 && w[0].offset != 0)
            .count();
        assert!(
            resets >= 1,
            "offset must reset at a line boundary at least once across 3 lines: {:?}",
            code_probes.iter().map(|p| p.offset).collect::<Vec<_>>()
        );

        // (c) the shared node does not fit inside any single run's `gen`
        // range (it is wider than every one of them) — per-run inversion by
        // containment structurally fails here, though the node still fits
        // the *segment's* gen range (segment-level classification holds).
        assert!(
            raw_node.start >= code_seg.gen.start && raw_node.end <= code_seg.gen.end,
            "the raw call's node still nests inside the code segment"
        );
        assert!(
            !code_seg
                .runs
                .iter()
                .any(|(_, gen, _)| gen.start <= raw_node.start && raw_node.end <= gen.end),
            "no single run should contain the whole-call node — it is coarser than every run"
        );
    }
}
