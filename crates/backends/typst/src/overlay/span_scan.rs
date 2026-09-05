//! Recover schema-field regions from *glyph spans*: the origin every drawn
//! frame item already carries. Content fields are codegen'd as markup block
//! bindings (`#let _qm_cN = [ .. ]`) in the generated helper `lib.typ`, so
//! every glyph's span nests inside its field's byte window; the backend
//! records that window plus a per-segment source map ([`FieldWindow`]) and
//! classifies each frame item by containment, two-tier: which window, then
//! which segment. Regions key on `(window, segment)`. A scalar the plate
//! interpolates directly (`#data.subject`) has no segments; its window comes
//! from [`scalar_windows`]. Spans survive any `show`-rule content rebuild
//! because they are a property of the glyph, not a sibling element.
//!
//! **Resolution goes through the compile's own helper source.** The session
//! serves reads from its last-good compile, but a failed `update` has already
//! written the *next* injection's helper text into the world; resolving the
//! served document's spans against that text would shift every range. Only
//! non-helper spans (plate, vendored packages: stable within a session)
//! resolve through the live world.
//!
//! **Marker claims.** A plate composes ink no field generated (a banner keyed
//! on a field, a package-built block). The helper's `field-region` brackets such
//! content with two invisible `metadata` markers, and the frame walk keeps a
//! stack of the open ones: ink that resolves to no window is claimed by the
//! innermost open marker instead of counting as foreign. Each *call* is its own
//! claim, so a wrapper invoked per card yields one region per card.
//!
//! The stack persists across pages so a claim can span a page break, which means
//! an open marker whose close never reaches the frame would claim every
//! unattributed hit to the end of the document. [`unclosed_claims`] names those
//! ahead of the scan and both queries suppress them, so an unbounded claim
//! yields nothing rather than everything.
//!
//! **First placement only.** Each span-window key's region is its first maximal
//! run of consecutive matching frame items. Span data cannot distinguish
//! "package chrome between two placements" from "a second placement" (both are a
//! gap of foreign spans), so later runs are not enumerated. One tolerance keeps
//! continuation pages covered: page marginals walk between one page's body and
//! the next's, so a run may resume on the immediately following page; a
//! same-page gap still ends it. A marker claim is exempt: its extent is
//! delimited explicitly, so every hit inside it accrues.
//!
//! Geometry composes the group-transform stack exactly like
//! `typst_layout::introspect::discover_frame`, transforming all four corners of
//! each item box (the stack may rotate or scale).

use std::collections::HashMap;
use std::ops::Range;

use typst::foundations::{Content, Label, Value};
use typst::introspection::Tag;
use typst::layout::{Frame, FrameItem, Point, Transform};
use typst::syntax::ast::{self, AstNode};
use typst::syntax::{DiagSpan, DiagSpanKind, FileId, LinkedNode, Source, Span, SyntaxKind};
use typst::utils::PicoStr;
use typst::World;
use typst_layout::PagedDocument;

use quillmark_core::{ContentHit, HitGranularity, RenderedRegion};

use crate::emit::SegmentMap;
use crate::world::QuillWorld;
use crate::AddressNode;

/// A tracked byte window in a compiled source: the schema field whose content
/// resolves into `range` of `file`. Content fields point at their generated
/// markup block in the helper `lib.typ`; scalars at their plate expression.
#[derive(Debug, Clone)]
pub(crate) struct FieldWindow {
    pub path: String,
    pub file: FileId,
    pub range: Range<usize>,
    /// `generated` ranges index the helper `lib.typ`. Empty for a scalar/widget
    /// site, whose whole first placement is one span-less region.
    pub segments: Vec<SegmentMap>,
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

    /// Gap from `(x, y)` to this box, zero inside it (edges included), absent
    /// when either side is not finite. The one measure both point queries rank
    /// by, so a tolerance of zero is exactly containment.
    /// `RenderedRegion::distance` carries why the finite check is load-bearing.
    fn distance(&self, x: f64, y: f64) -> Option<f64> {
        let corners = [self.min_x, self.min_y, self.max_x, self.max_y];
        let finite = x.is_finite() && y.is_finite() && corners.iter().all(|v| v.is_finite());
        finite.then(|| {
            let dx = (self.min_x - x).max(0.0).max(x - self.max_x);
            let dy = (self.min_y - y).max(0.0).max(y - self.max_y);
            dx.hypot(dy)
        })
    }
}

/// The nearest box within `tol` among `boxed`, which callers pass **in reverse
/// paint order**: ties keep the first seen, so the last-painted of equally near
/// items wins.
fn nearest<T>(
    boxed: impl Iterator<Item = (Aabb, T)>,
    x: f64,
    y: f64,
    tol: f64,
) -> Option<(f64, T)> {
    boxed
        .filter_map(|(b, item)| Some((b.distance(x, y)?, item)))
        .filter(|(d, _)| *d <= tol)
        .min_by(|(a, _), (b, _)| a.total_cmp(b))
}

/// Page-space (top-left origin) box → PDF-space (bottom-left origin) rect.
fn pdf_rect(b: &Aabb, page_h: f64) -> [f32; 4] {
    [
        b.min_x as f32,
        (page_h - b.max_y) as f32,
        b.max_x as f32,
        (page_h - b.min_y) as f32,
    ]
}

struct Hit {
    page: usize,
    class: HitClass,
    /// `Some` for any attributed ink, `None` for foreign ink.
    rect: Option<Aabb>,
}

/// One box-accruing bucket in the run scan.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    /// A source window and, on a content window, the segment inside it.
    Span(usize, Option<usize>),
    /// One `field-region` call instance, indexed into [`Markers::claims`].
    Marker(usize),
}

/// What a point query attributes a hit's ink to, boxable or not.
#[derive(Clone, Copy)]
enum Owner {
    Window(usize),
    Claim(usize),
}

/// Where a hit falls in the key space.
#[derive(Clone, Copy)]
enum HitClass {
    Boxable { key: Key },
    /// A content window's ink between its segments (brackets, container-open
    /// syntax). Suspends a *different* window's run like foreign ink, but is a
    /// no-op while its own window's segment is the run.
    Transparent { window: usize },
    /// Ink from a detached span: Typst's synthesized text decorations (the
    /// `underline`/`strike` line, drawn as a `Shape` mid-run) and list markers.
    /// Attributable to no field, and never a run-breaker: a decoration drawn
    /// between a field's own glyphs would otherwise orphan the rest of the line.
    Anonymous,
    /// No window and no open marker, but a *resolvable* span: page chrome,
    /// another field's text, vendored-package output. Breaks a run.
    Foreign,
}

impl HitClass {
    fn owner(self) -> Option<Owner> {
        match self {
            HitClass::Boxable { key: Key::Span(w, _) } => Some(Owner::Window(w)),
            HitClass::Boxable { key: Key::Marker(c) } => Some(Owner::Claim(c)),
            HitClass::Transparent { window } => Some(Owner::Window(window)),
            HitClass::Anonymous | HitClass::Foreign => None,
        }
    }
}

/// `(w, None)` splits by window kind: on a segment-less window it is the whole
/// placement's boxable key; on a content window it is inter-segment ink. Falling
/// through to `marker` only on an unresolved span is what makes nested tracked
/// ink outrank an enclosing `field-region`.
fn hit_class(
    resolved: Option<(usize, Option<usize>)>,
    detached: bool,
    windows: &[FieldWindow],
    marker: Option<usize>,
) -> HitClass {
    match resolved {
        None if detached => HitClass::Anonymous,
        None => match marker {
            Some(c) => HitClass::Boxable { key: Key::Marker(c) },
            None => HitClass::Foreign,
        },
        Some((w, Some(s))) => HitClass::Boxable { key: Key::Span(w, Some(s)) },
        Some((w, None)) if windows[w].segments.is_empty() => {
            HitClass::Boxable { key: Key::Span(w, None) }
        }
        Some((w, None)) => HitClass::Transparent { window: w },
    }
}

/// The label and metadata `kind` the helper's `field-region` stamps on its two
/// bracketing markers.
const REGION_LABEL: &str = "__qm_region__";

/// The `field-region` calls open at the current point of the frame walk, and
/// every claim the walk has discovered.
#[derive(Default)]
struct Markers {
    claims: Vec<String>,
    stack: Vec<usize>,
    /// Claim indices [`unclosed_claims`] found still open at the end of the
    /// document. Keeping them off `stack` leaves their ink foreign, exactly as
    /// if the plate had emitted no marker at all.
    suppressed: Vec<usize>,
}

impl Markers {
    fn suppressing(suppressed: &[usize]) -> Self {
        Self {
            suppressed: suppressed.to_vec(),
            ..Self::default()
        }
    }

    /// A close unwinds to the innermost open claim on `path`; one whose open
    /// never reached the frame (content Typst laid out elsewhere) is dropped
    /// rather than popping an unrelated claim. An open still on the stack when
    /// the walk ends is the mirror case, handled by [`unclosed_claims`]: it
    /// cannot be detected here, where the rest of the document is still ahead.
    ///
    /// `claims` grows on every open, suppressed or not, so an index addresses
    /// the same call in any walk of one document — including the page prefix
    /// [`Scan::field_at`] walks.
    fn edge(&mut self, path: &str, close: bool) {
        if !close {
            self.claims.push(path.to_string());
            let claim = self.claims.len() - 1;
            if !self.suppressed.contains(&claim) {
                self.stack.push(claim);
            }
        } else if let Some(i) = self.stack.iter().rposition(|&c| self.claims[c] == path) {
            self.stack.truncate(i);
        }
    }

    fn current(&self) -> Option<usize> {
        self.stack.last().copied()
    }
}

/// The `(claim, field)` of every `field-region` whose open marker reached a
/// frame but whose close never did, over the whole document. A claim an
/// enclosing close unwound past counts as closed: its extent was bounded, so
/// nothing ran away.
pub(crate) fn unclosed_claims(doc: &PagedDocument) -> Vec<(usize, String)> {
    let mut markers = Markers::default();
    for page in doc.pages() {
        scan_markers(&page.frame, &mut markers);
    }
    markers
        .stack
        .iter()
        .map(|&c| (c, markers.claims[c].clone()))
        .collect()
}

/// `(field, is-close)` if `elem` is one of `field-region`'s markers.
fn region_marker(elem: &Content) -> Option<(String, bool)> {
    if elem.label()? != Label::new(PicoStr::intern(REGION_LABEL))? {
        return None;
    }
    let Ok(Value::Dict(d)) = elem.get_by_name("value") else {
        return None;
    };
    // A user attaching the reserved label to their own metadata is ignored.
    if !matches!(d.get("kind"), Ok(Value::Str(k)) if k.as_str() == REGION_LABEL) {
        return None;
    }
    let Ok(Value::Str(field)) = d.get("field") else {
        return None;
    };
    Some((
        field.to_string(),
        matches!(d.get("close"), Ok(Value::Bool(true))),
    ))
}

/// Memoizing span → two-tier classifier: the tree descent that resolves a span
/// to its byte range runs once per distinct span, not once per glyph, and both
/// the classification and the caret path read that one cache.
struct Classifier<'a> {
    world: &'a QuillWorld,
    helper: &'a Source,
    windows: &'a [FieldWindow],
    memo: HashMap<Span, Option<(FileId, Range<usize>)>>,
}

impl<'a> Classifier<'a> {
    fn new(world: &'a QuillWorld, helper: &'a Source, windows: &'a [FieldWindow]) -> Self {
        Self {
            world,
            helper,
            windows,
            memo: HashMap::new(),
        }
    }

    /// The same unpack `WorldExt::range` performs, with the helper file routed
    /// to the served compile's snapshot instead of the world.
    fn range_of(&mut self, span: Span) -> Option<(FileId, Range<usize>)> {
        if let Some(cached) = self.memo.get(&span) {
            return cached.clone();
        }
        let resolved = match DiagSpan::from(span).get() {
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
        };
        self.memo.insert(span, resolved.clone());
        resolved
    }

    /// Resolves to the innermost segment whose `generated` range contains the
    /// span, or `None` for inter-segment / segment-less ink.
    fn classify_seg(&mut self, span: Span) -> Option<(usize, Option<usize>)> {
        let (file, range) = self.range_of(span)?;
        self.windows
            .iter()
            .position(|win| {
                win.file == file && win.range.start <= range.start && range.end <= win.range.end
            })
            .map(|i| (i, self.seg_of(i, &range)))
    }

    /// Segments are `generated`-ordered and disjoint, so the sole candidate is
    /// the last one starting at or before `range.start`.
    fn seg_of(&self, window: usize, range: &Range<usize>) -> Option<usize> {
        let segs = &self.windows[window].segments;
        let i = segs.partition_point(|s| s.generated.start <= range.start);
        (i > 0 && segs[i - 1].generated.end >= range.end).then(|| i - 1)
    }
}

/// Records `page`'s ink against the marker stack, which pages before it may
/// have left open.
fn collect_page_hits(
    frame: &Frame,
    page: usize,
    cls: &mut Classifier,
    markers: &mut Markers,
    out: &mut Vec<Hit>,
) {
    walk_items(frame, Transform::identity(), page, &mut |page, item| match item {
        Item::Marker { path, close } => markers.edge(&path, close),
        Item::Ink { span, aabb, .. } => {
            let class = hit_class(
                cls.classify_seg(span),
                span.is_detached(),
                cls.windows,
                markers.current(),
            );
            // Foreign ink still emits a rect-less Hit so the run machine sees
            // the full ink sequence.
            let rect = class.owner().is_some().then(aabb);
            out.push(Hit { page, class, rect });
        }
    });
}

/// The marker half of [`collect_page_hits`] for a page whose ink a query
/// discards, since a claim opening there may still cover the page it wants.
/// Skipping the per-glyph walk keeps that lookbehind linear in frame items.
fn scan_markers(frame: &Frame, markers: &mut Markers) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => scan_markers(&group.frame, markers),
            FrameItem::Tag(Tag::Start(elem, _)) => {
                if let Some((path, close)) = region_marker(elem) {
                    markers.edge(&path, close);
                }
            }
            _ => {}
        }
    }
}

/// One frame item [`walk_items`] reports. The box is a thunk so a consumer that
/// discards foreign ink pays no box arithmetic.
enum Item<'a> {
    Ink {
        span: Span,
        /// Intra-node byte offset.
        offset: u16,
        aabb: &'a dyn Fn() -> Aabb,
    },
    Marker {
        path: String,
        close: bool,
    },
}

type ItemVisitor<'a> = dyn for<'b> FnMut(usize, Item<'b>) + 'a;

fn walk_items(frame: &Frame, ts: Transform, page: usize, visit: &mut ItemVisitor) {
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                let ts = ts
                    .pre_concat(Transform::translate(pos.x, pos.y))
                    .pre_concat(group.transform);
                walk_items(&group.frame, ts, page, visit);
            }
            FrameItem::Text(text) => {
                let bb = text.bbox();
                let mut cursor = Point::zero();
                for glyph in &text.glyphs {
                    let advance =
                        Point::new(glyph.x_advance.at(text.size), glyph.y_advance.at(text.size));
                    let offset =
                        Point::new(glyph.x_offset.at(text.size), glyph.y_offset.at(text.size));
                    let lo = Point::new(cursor.x + offset.x, cursor.y + bb.min.y);
                    let hi = Point::new(cursor.x + offset.x + advance.x, cursor.y + bb.max.y);
                    let p = *pos;
                    visit(
                        page,
                        Item::Ink {
                            span: glyph.span.0,
                            offset: glyph.span.1,
                            aabb: &|| item_aabb(p, lo, hi, ts),
                        },
                    );
                    cursor += advance;
                }
            }
            FrameItem::Shape(shape, span) => {
                let bb = shape.geometry.bbox(shape.stroke.as_ref());
                let p = *pos;
                visit(
                    page,
                    Item::Ink {
                        span: *span,
                        offset: 0,
                        aabb: &|| item_aabb(p, bb.min, bb.max, ts),
                    },
                );
            }
            FrameItem::Image(_, size, span) => {
                let sz = size.to_point();
                let p = *pos;
                visit(
                    page,
                    Item::Ink {
                        span: *span,
                        offset: 0,
                        aabb: &|| item_aabb(p, Point::zero(), sz, ts),
                    },
                );
            }
            FrameItem::Tag(Tag::Start(elem, _)) => {
                if let Some((path, close)) = region_marker(elem) {
                    visit(page, Item::Marker { path, close });
                }
            }
            _ => {}
        }
    }
}

/// All four corners transform: `ts` may rotate or scale.
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

/// Out-of-run states only: at most one window accrues at a time, tracked by
/// [`run_scan_machine`] as a single cursor.
#[derive(Clone, Copy, PartialEq)]
enum Run {
    NotSeen,
    /// Interrupted by foreign ink; may resume on page `last_page + 1` only.
    Suspended {
        last_page: usize,
    },
    Done,
}

/// One committed compile plus the tables its spans resolve against: the context
/// every region and point query shares. `unclosed` is supplied rather than
/// derived per query because whether a claim open at some page ever closes is
/// knowable only from the pages after it, which a single-page walk never
/// reaches.
pub(crate) struct Scan<'a> {
    pub(crate) doc: &'a PagedDocument,
    pub(crate) world: &'a QuillWorld,
    pub(crate) helper: &'a Source,
    pub(crate) windows: &'a [FieldWindow],
    pub(crate) unclosed: &'a [usize],
}

impl<'a> Scan<'a> {
    fn classifier(&self) -> Classifier<'a> {
        Classifier::new(self.world, self.helper, self.windows)
    }

    /// `page`'s ink, or every page's when `page` is `None`, plus the marker
    /// stack the walk ended on. Pages before `page` are walked for their markers
    /// alone, since a claim opening there may still cover the page wanted;
    /// skipping their glyphs keeps that lookbehind linear in frame items.
    fn hits(&self, page: Option<usize>) -> (Vec<Hit>, Markers) {
        let mut cls = self.classifier();
        let mut markers = Markers::suppressing(self.unclosed);
        let mut hits = Vec::new();
        for (i, p) in self.doc.pages().iter().enumerate() {
            match page {
                Some(want) if i < want => scan_markers(&p.frame, &mut markers),
                Some(want) if i > want => break,
                _ => collect_page_hits(&p.frame, i, &mut cls, &mut markers, &mut hits),
            }
        }
        (hits, markers)
    }

    /// Each span window's **first placement** and each `field-region` call's
    /// whole claim: one [`RenderedRegion`] per page a run touches, PDF
    /// bottom-left rects, sorted (page, field, key order). A claim in `unclosed`
    /// accrues nothing and so surfaces no region.
    pub(crate) fn regions(&self) -> Vec<RenderedRegion> {
        let (hits, markers) = self.hits(None);

        // Claims are only known after the walk, so keys extend the window keys.
        let mut keys = flatten_keys(self.windows);
        keys.extend((0..markers.claims.len()).map(Key::Marker));
        let boxes = run_scan_machine(&keys, &hits);

        let mut out: Vec<(RenderedRegion, usize)> = Vec::new();
        for (ki, key) in keys.iter().enumerate() {
            let (path, span) = match *key {
                Key::Span(wi, seg) => {
                    let window = &self.windows[wi];
                    let span = seg.map(|s| {
                        let c = &window.segments[s].content;
                        [c.start, c.end]
                    });
                    (window.path.clone(), span)
                }
                // Geometry with no content address, like a scalar site or a widget.
                Key::Marker(ci) => (markers.claims[ci].clone(), None),
            };
            for (page, b) in &boxes[ki] {
                let Some(page_h) = self.doc.pages().get(*page).map(|p| p.frame.size().y.to_pt())
                else {
                    continue;
                };
                let mut region = RenderedRegion::new(path.clone(), *page, pdf_rect(b, page_h));
                region.span = span;
                out.push((region, ki));
            }
        }
        // `ki` orders window-major then segment-ascending, a stable tiebreak.
        out.sort_by(|(a, ai), (b, bi)| (a.page, &a.field, *ai).cmp(&(b.page, &b.field, *bi)));
        out.into_iter().map(|(r, _)| r).collect()
    }

    /// The schema field under a point (`x`/`y` in PDF bottom-left points), or
    /// within `tol` points of one, and the gap it answered at. Unlike
    /// [`regions`](Self::regions) every placement answers, not just the first.
    /// The nearest tracked ink wins, later-painted on a tie; untracked ink never
    /// occludes. The gap is what ranks this lane against another.
    pub(crate) fn field_at(
        &self,
        page: usize,
        x: f32,
        y: f32,
        tol: f32,
    ) -> Option<(f32, String)> {
        let frame = &self.doc.pages().get(page)?.frame;
        let page_h = frame.size().y.to_pt();
        let (x, y) = (x as f64, page_h - y as f64);

        let (hits, markers) = self.hits(Some(page));

        let (gap, hit) = nearest(
            hits.iter().rev().filter_map(|h| Some((h.rect?, h))),
            x,
            y,
            tol as f64,
        )?;
        let owner = hit.class.owner()?;
        Some((
            gap as f32,
            match owner {
                Owner::Window(w) => self.windows[w].path.clone(),
                Owner::Claim(c) => markers.claims[c].clone(),
            },
        ))
    }

    /// A point (PDF bottom-left points) → content position in a content field,
    /// resolving to the nearest glyph within `tol` points. Degrades to the
    /// segment's content start when the resolved node nests inside no single
    /// run (a multi-line `#raw` block, or structural ink).
    pub(crate) fn position_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<ContentHit> {
        if self.windows.is_empty() {
            return None;
        }
        let frame = &self.doc.pages().get(page)?.frame;
        let page_h = frame.size().y.to_pt();
        let (px, py) = (x as f64, page_h - y as f64);

        let mut cls = self.classifier();
        let mut hits = Vec::new();
        walk_glyphs(frame, page, &mut cls, None, &mut hits);

        // Ink with no segment has no content position.
        let (_, hit) = nearest(
            hits.iter()
                .rev()
                .filter(|g| g.seg.is_some())
                .map(|g| (g.rect, g)),
            px,
            py,
            tol as f64,
        )?;
        let window = &self.windows[hit.window];
        let segmap = &window.segments[hit.seg?];
        let (pos, granularity) = invert_hit(self.helper, segmap, &hit.node, hit.offset);
        Some(ContentHit::new(window.path.clone(), pos).with_granularity(granularity))
    }

    /// A content position → caret rect: the box of the frame glyph whose
    /// resolved node covers `pos`, with `span` collapsed to `[pos, pos]`.
    pub(crate) fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
        if self.windows.is_empty() {
            return None;
        }
        let (wi, window) = self
            .windows
            .iter()
            .enumerate()
            .find(|(_, w)| w.path == field && !w.segments.is_empty())?;
        let seg_idx = window
            .segments
            .iter()
            .position(|s| s.content.start <= pos && pos <= s.content.end)?;
        let target_gen = forward_pos(self.helper, &window.segments[seg_idx], pos);

        let mut cls = self.classifier();
        let mut hits = Vec::new();
        for (page, p) in self.doc.pages().iter().enumerate() {
            walk_glyphs(&p.frame, page, &mut cls, Some((wi, Some(seg_idx))), &mut hits);
        }

        // A covering glyph always beats a non-covering one, so a caret near a
        // run edge still resolves; `min_by_key` keeps the first on ties.
        let g = hits.iter().min_by_key(|g| {
            let covers = g.node.start <= target_gen && target_gen < g.node.end;
            let caret = g.node.start + g.offset as usize;
            (
                !covers,
                caret > target_gen,
                (caret as isize - target_gen as isize).unsigned_abs(),
            )
        })?;
        let page_h = self.doc.pages().get(g.page)?.frame.size().y.to_pt();
        Some(
            RenderedRegion::new(field.to_string(), g.page, pdf_rect(&g.rect, page_h))
                .with_span([pos, pos]),
        )
    }
}

/// Window-major, segment-ascending: the order regions sort by.
fn flatten_keys(windows: &[FieldWindow]) -> Vec<Key> {
    let mut keys = Vec::new();
    for (wi, w) in windows.iter().enumerate() {
        if w.segments.is_empty() {
            keys.push(Key::Span(wi, None));
        } else {
            keys.extend((0..w.segments.len()).map(|s| Key::Span(wi, Some(s))));
        }
    }
    keys
}

/// Each key's boxes per page, indexed parallel to `keys`. `current` is the one
/// key whose run is accruing; a boxable hit for a different key (or foreign ink)
/// suspends it, and a suspended span-window run resumes only on the immediately
/// following page. A [`Key::Marker`] run always resumes: its extent is the
/// bracketing markers, so an interruption inside it is never a second placement.
fn run_scan_machine(keys: &[Key], hits: &[Hit]) -> Vec<Vec<(usize, Aabb)>> {
    let key_index: HashMap<Key, usize> =
        keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let mut state = vec![Run::NotSeen; keys.len()];
    let mut boxes: Vec<Vec<(usize, Aabb)>> = vec![Vec::new(); keys.len()];
    let mut current: Option<(usize, usize)> = None; // (key index, last_page)

    for hit in hits {
        match hit.class {
            HitClass::Boxable { key } => {
                let ki = key_index[&key];
                if current.map(|(c, _)| c) == Some(ki) {
                    accrue(&mut boxes[ki], hit);
                    current = Some((ki, hit.page));
                } else {
                    if let Some((c, last_page)) = current.take() {
                        state[c] = Run::Suspended { last_page };
                    }
                    let resumes = match state[ki] {
                        _ if matches!(keys[ki], Key::Marker(_)) => true,
                        Run::NotSeen => true,
                        Run::Suspended { last_page } => hit.page == last_page + 1,
                        Run::Done => false,
                    };
                    if resumes {
                        accrue(&mut boxes[ki], hit);
                        current = Some((ki, hit.page));
                    } else {
                        state[ki] = Run::Done;
                    }
                }
            }
            HitClass::Transparent { window } => match current {
                // Transparent only while this field's own segment is the run,
                // else interleaved placements merge into one lying box.
                Some((c, _)) if matches!(keys[c], Key::Span(w, _) if w == window) => {}
                _ => {
                    if let Some((c, last_page)) = current.take() {
                        state[c] = Run::Suspended { last_page };
                    }
                }
            },
            HitClass::Anonymous => {}
            HitClass::Foreign => {
                if let Some((c, last_page)) = current.take() {
                    state[c] = Run::Suspended { last_page };
                }
            }
        }
    }
    boxes
}

/// Pages are nondecreasing in walk order, so a page transition opens a new box.
fn accrue(boxes: &mut Vec<(usize, Aabb)>, hit: &Hit) {
    let rect = hit.rect.expect("classified hits carry a box");
    match boxes.last_mut() {
        Some((page, b)) if *page == hit.page => b.union(rect),
        _ => boxes.push((hit.page, rect)),
    }
}

/// The finer counterpart to [`Hit`], carrying the node range and intra-node
/// offset that [`position_at`]/[`locate`] need and a region scan discards.
struct GlyphHit {
    page: usize,
    rect: Aabb,
    node: Range<usize>,
    /// Typst types the intra-node span offset as `u16`, so a text node wider
    /// than 64 KiB saturates it: the caret floors to the cluster boundary
    /// instead of the exact glyph. No emitted node approaches that bound.
    offset: u16,
    window: usize,
    seg: Option<usize>,
}

/// The content-position twin of [`collect_page_hits`]. Foreign and unresolvable
/// ink is skipped: it has no content address, marker claims included.
///
/// `only` restricts emission to a single `(window, seg)`: the caret path knows
/// its target segment up front, so it skips the box arithmetic and the
/// `GlyphHit` allocation for every other glyph in the document. `None` emits all
/// classified+resolved ink (the point-hit path needs the full set).
fn walk_glyphs(
    frame: &Frame,
    page: usize,
    cls: &mut Classifier,
    only: Option<(usize, Option<usize>)>,
    out: &mut Vec<GlyphHit>,
) {
    walk_items(frame, Transform::identity(), page, &mut |page, item| {
        let Item::Ink { span, offset, aabb } = item else {
            return;
        };
        let Some((w, seg)) = cls.classify_seg(span) else {
            return;
        };
        if only.is_some_and(|t| t != (w, seg)) {
            return;
        }
        if let Some((_, node)) = cls.range_of(span) {
            out.push(GlyphHit {
                page,
                rect: aabb(),
                node,
                offset,
                window: w,
                seg,
            });
        }
    });
}

/// The content USV offset a glyph's generated position maps to. With no owning
/// run (the `#raw` multi-line case, where every line shares one node wider than
/// any run) the safe degrade is the segment's content start, not a node-start
/// computation that could land outside every line's own text.
fn invert_hit(
    helper: &Source,
    segmap: &SegmentMap,
    node: &Range<usize>,
    offset: u16,
) -> (usize, HitGranularity) {
    let Some((content_r, gen_r, ctx)) = segmap
        .runs
        .iter()
        .find(|(_, g, _)| g.start <= node.start && node.end <= g.end)
    else {
        return (segmap.content.start, HitGranularity::Segment);
    };
    // `node.start + glyph.span.1` is the exact generated byte for markup text;
    // clamp inside the run against a boundary-hugging offset.
    let abs = (node.start + offset as usize)
        .min(gen_r.end.saturating_sub(1))
        .max(gen_r.start);
    let gen_text = &helper.text()[gen_r.clone()];
    let pos = content_r.start + crate::emit::invert_gen_offset(gen_text, *ctx, abs - gen_r.start);
    (pos, HitGranularity::Cluster)
}

/// The generated byte `pos` maps to: the run containing it, else the nearest
/// preceding run. A position in a structural gap — one past a hard break, or
/// one past the segment's last content character, where a caret sits while
/// typing — lands on that run's generated end, which
/// [`forward_content_offset`](crate::emit::forward_content_offset) saturates to.
/// The segment's generated window start is the floor only when no run precedes
/// `pos`.
fn forward_pos(helper: &Source, segmap: &SegmentMap, pos: usize) -> usize {
    let run = segmap
        .runs
        .iter()
        .find(|(c, _, _)| c.start <= pos && pos < c.end)
        .or_else(|| segmap.runs.iter().rev().find(|(c, _, _)| c.end <= pos));
    match run {
        Some((content_r, gen_r, ctx)) => {
            let gen_text = &helper.text()[gen_r.clone()];
            gen_r.start + crate::emit::forward_content_offset(gen_text, *ctx, pos - content_r.start)
        }
        None => segmap.generated.start,
    }
}

/// Byte windows for the plate's direct scalar references. Two windows per
/// reference site where they differ:
///
/// - the **chain** window: the `data.<field>` access widened to the outermost
///   postfix chain it heads (`data.refs.at(0)`), matching ink whose span is the
///   reference expression itself; and
/// - the **enclosing-expression** window, widened through surrounding call
///   arguments and operators (`#upper(data.subject)`). Emitted only when exactly
///   one reference sits inside it: `data.a + data.b` has no single owner.
///
/// Chain windows sort first, so ink resolving to the reference itself is never
/// claimed by a wider window.
///
/// A reference that steps into a declared container (`data.classification.poc`,
/// `data.refs.at(0).org`) anchors on the cell, so the region carries the address
/// the plate actually read rather than the container's.
///
/// A read through a single-assignment `let` alias ([`alias_bindings`]) is a
/// reference site like the chain it names, so naming a container before
/// stepping into it keeps the address. Laundering the tracker cannot follow —
/// a function parameter, a destructured binding, a per-card loop variable —
/// still carries the binding's span and needs a `field-region` claim.
pub(crate) fn scalar_windows(
    source: &Source,
    address_root: &AddressNode,
) -> Vec<(String, Range<usize>)> {
    let tables = Tables { root: address_root };
    let root = LinkedNode::new(source.root());
    let aliases = alias_bindings(&root, &tables);
    let mut anchors: Vec<(String, Range<usize>, Range<usize>)> = Vec::new();
    collect_anchors(&root, &tables, &aliases, &mut anchors);

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

/// The address tree the scan resolves a read against — the same one
/// [`_qm-known-path`] validates a `form-field` / `field-region` path against, so
/// a scanned region path is one a claim could bind.
struct Tables<'a> {
    root: &'a AddressNode,
}

impl Tables<'_> {
    fn field_names(&self) -> Vec<String> {
        self.root.props.keys().cloned().collect()
    }

    /// The property names an address offers a step into.
    ///
    /// Keyed on the address rather than the anchor, so an alias to a row
    /// (`#let r = data.refs.at(0)`) reaches the cell the chain it names does: the
    /// index step may have been taken in the initializer.
    fn property_keys(&self, path: &str) -> Vec<String> {
        self.root
            .resolve(path)
            .map(|node| node.props.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn is_indexable(&self, path: &str) -> bool {
        self.root
            .resolve(path)
            .is_some_and(|node| node.item.is_some())
    }
}

/// A read before `bound_at` reads whatever else held the name, so the position
/// bounds the alias the way lexical order does.
struct Alias {
    path: String,
    bound_at: usize,
}

/// Each identifier the plate binds exactly once, to exactly one `data` chain.
///
/// The single-binder rule is what makes an alias safe to follow: a name a
/// second `let`, a closure parameter, a loop pattern, an import, or an
/// assignment could rebind is dropped, so no window is ever minted over ink
/// belonging to another value. An initializer must be the chain and nothing
/// else — `#let x = upper(data.subject)` and `#let x = data.a + data.b` both
/// yield no alias, the first because the chain is not the whole expression and
/// the second because no single field owns it.
fn alias_bindings(root: &LinkedNode, tables: &Tables) -> HashMap<String, Alias> {
    let mut found = Binders::default();
    collect_binders(root, tables, &mut found);
    if found.wildcard_import {
        return HashMap::new();
    }
    found
        .candidates
        .into_iter()
        .filter(|(name, _)| found.counts.get(name) == Some(&1))
        .collect()
}

/// A wildcard import binds names in another file, so it disqualifies every
/// alias no matter which side of a binding it sits on.
#[derive(Default)]
struct Binders {
    counts: HashMap<String, usize>,
    candidates: Vec<(String, Alias)>,
    wildcard_import: bool,
}

fn collect_binders(node: &LinkedNode, tables: &Tables, out: &mut Binders) {
    let bind = |name: &str, out: &mut Binders| {
        *out.counts.entry(name.to_string()).or_default() += 1;
    };
    match node.kind() {
        SyntaxKind::LetBinding => {
            if let Some(binding) = node.cast::<ast::LetBinding>() {
                for ident in binding.kind().bindings() {
                    bind(ident.as_str(), out);
                }
                if let Some((name, alias)) = alias_candidate(node, binding, tables) {
                    out.candidates.push((name, alias));
                }
            }
        }
        SyntaxKind::Closure => {
            if let Some(closure) = node.cast::<ast::Closure>() {
                for ident in closure.name().into_iter() {
                    bind(ident.as_str(), out);
                }
                for param in closure.params().children() {
                    match param {
                        ast::Param::Pos(pattern) => {
                            for ident in pattern.bindings() {
                                bind(ident.as_str(), out);
                            }
                        }
                        // The name is the parameter; the default beside it binds
                        // nothing.
                        ast::Param::Named(named) => bind(named.name().as_str(), out),
                        ast::Param::Spread(spread) => {
                            if let Some(ident) = spread.sink_ident() {
                                bind(ident.as_str(), out);
                            }
                        }
                    }
                }
            }
        }
        SyntaxKind::ForLoop => {
            if let Some(loop_) = node.cast::<ast::ForLoop>() {
                for ident in loop_.pattern().bindings() {
                    bind(ident.as_str(), out);
                }
            }
        }
        SyntaxKind::DestructAssignment => {
            if let Some(assign) = node.cast::<ast::DestructAssignment>() {
                for ident in assign.pattern().bindings() {
                    bind(ident.as_str(), out);
                }
            }
        }
        SyntaxKind::ModuleImport => {
            if let Some(import) = node.cast::<ast::ModuleImport>() {
                match import.imports() {
                    Some(ast::Imports::Wildcard) => out.wildcard_import = true,
                    Some(ast::Imports::Items(items)) => {
                        for item in items.iter() {
                            bind(item.bound_name().as_str(), out);
                        }
                    }
                    None => {}
                }
            }
        }
        SyntaxKind::Binary => {
            if let Some(binary) = node.cast::<ast::Binary>() {
                if matches!(
                    binary.op(),
                    ast::BinOp::Assign
                        | ast::BinOp::AddAssign
                        | ast::BinOp::SubAssign
                        | ast::BinOp::MulAssign
                        | ast::BinOp::DivAssign
                ) {
                    if let ast::Expr::Ident(ident) = binary.lhs() {
                        bind(ident.as_str(), out);
                    }
                }
            }
        }
        _ => {}
    }
    for child in node.children() {
        collect_binders(&child, tables, out);
    }
}

fn alias_candidate(
    node: &LinkedNode,
    binding: ast::LetBinding,
    tables: &Tables,
) -> Option<(String, Alias)> {
    let ast::LetBindingKind::Normal(ast::Pattern::Normal(ast::Expr::Ident(name))) = binding.kind()
    else {
        return None;
    };
    let init = binding.init()?.to_untyped();
    let init = node.children().find(|c| std::ptr::eq(c.get(), init))?;
    let mut inner = Vec::new();
    collect_anchors(&init, tables, &HashMap::new(), &mut inner);
    let [(path, chain, _)] = inner.as_slice() else {
        return None;
    };
    (*chain == init.range()).then(|| {
        (
            name.as_str().to_string(),
            Alias {
                path: path.clone(),
                bound_at: node.range().end,
            },
        )
    })
}

/// Recursion continues into matched subtrees: a reference nested in another
/// chain's arguments is its own site.
fn collect_anchors(
    node: &LinkedNode,
    tables: &Tables,
    aliases: &HashMap<String, Alias>,
    out: &mut Vec<(String, Range<usize>, Range<usize>)>,
) {
    if let Some((path, anchor)) = data_access(node, tables, aliases) {
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
        collect_anchors(&child, tables, aliases, out);
    }
}

/// The schema path `node` reads and the node to widen from, for the two shapes
/// that name a field: a `data.<field>` / `data.at("<field>")` access, and a
/// read through a `let` alias. Either steps deeper where the field is a
/// container and the plate selected into it ([`step_into`]).
fn data_access<'a>(
    node: &LinkedNode<'a>,
    tables: &Tables,
    aliases: &HashMap<String, Alias>,
) -> Option<(String, LinkedNode<'a>)> {
    match node.kind() {
        SyntaxKind::FieldAccess => {
            let ast::Expr::Ident(target) = node.cast::<ast::FieldAccess>()?.target() else {
                return None;
            };
            if target.as_str() != "data" {
                return None;
            }
            let (name, anchor) = selected(node, &tables.field_names())?;
            Some(step_into(name, anchor, tables))
        }
        // Reads before the binding ends — its own pattern, its initializer —
        // are not reads of the field.
        SyntaxKind::Ident => {
            let alias = aliases.get(node.cast::<ast::Ident>()?.as_str())?;
            (node.range().start >= alias.bound_at && reads_its_binding(node))
                .then(|| step_into(alias.path.clone(), node.clone(), tables))
        }
        _ => None,
    }
}

/// Whether the identifier reads what the name holds, rather than spelling a name
/// of its own: a key selected off another value (`styles.subject`), a named
/// argument or dict key (`text(subject: 12pt)`, `(subject: [Ink])`), an item
/// named inside an import.
///
/// [`alias_bindings`] bounds which *name* an anchor may follow, this which
/// *occurrence*. Both are needed: a schema field name collides freely with the
/// parameter names of a callee the plate never defines (`date`, `caption`,
/// `align`, `subject`), and a window over one would carry a wrong address over
/// ink the field never drew. [`select_property`] tests the mirror of this for
/// the step it takes.
fn reads_its_binding(node: &LinkedNode) -> bool {
    let Some(parent) = node.parent() else {
        return true;
    };
    match parent.kind() {
        SyntaxKind::FieldAccess => parent
            .cast::<ast::FieldAccess>()
            .is_some_and(|access| access.target().to_untyped() == node.get()),
        SyntaxKind::Named => parent
            .cast::<ast::Named>()
            .is_some_and(|named| named.expr().to_untyped() == node.get()),
        SyntaxKind::ImportItemPath | SyntaxKind::RenamedImportItem => false,
        _ => true,
    }
}

/// The address the read reaches into `name`: every index (`refs.0`) and property
/// (`classification.poc`, `refs.0.org`) step the plate actually takes, or `name`
/// itself where it takes none.
///
/// Each step is its own address, so a whole-row read anchors on the row rather
/// than on the table, and a read that stops halfway anchors where it stopped.
fn step_into<'a>(
    name: String,
    anchor: LinkedNode<'a>,
    tables: &Tables,
) -> (String, LinkedNode<'a>) {
    let mut name = name;
    let mut anchor = anchor;
    loop {
        if tables.is_indexable(&name) {
            if let Some((index, row)) = select_index(&anchor) {
                name = format!("{name}.{index}");
                anchor = row;
                continue;
            }
        }
        match select_property(&anchor, &tables.property_keys(&name)) {
            Some((key, deeper)) => {
                name = format!("{name}.{key}");
                anchor = deeper;
            }
            None => return (name, anchor),
        }
    }
}

/// The declared key `node`'s parent selects off it — `.<key>` or
/// `.at("<key>")` — and the node that selection widens to.
fn select_property<'a>(
    node: &LinkedNode<'a>,
    keys: &[String],
) -> Option<(String, LinkedNode<'a>)> {
    let parent = node.parent()?;
    if parent.cast::<ast::FieldAccess>()?.target().to_untyped() != node.get() {
        return None;
    }
    selected(parent, keys)
}

/// The key `access` selects out of `keys` and the node the selection widens to.
/// One grammar, two spellings: `.<key>` names it directly, `.at("<key>")`
/// reaches the keys an identifier cannot spell.
fn selected<'a>(access: &LinkedNode<'a>, keys: &[String]) -> Option<(String, LinkedNode<'a>)> {
    let field = access.cast::<ast::FieldAccess>()?.field();
    if keys.iter().any(|k| k == field.as_str()) {
        return Some((field.as_str().to_string(), access.clone()));
    }
    (field.as_str() == "at")
        .then(|| select_by_at(access, keys))
        .flatten()
}

/// The array index `node`'s parent selects off it, and the node that selection
/// widens to. `.at(n)` is the only spelling: Typst has no `.0` field access for an
/// array element. Any non-negative literal is admitted, matching
/// [`_qm-known-path`]'s digit test — a negative index lexes as unary minus over
/// the magnitude, so it never reaches the `Int` arm and needs no sign check.
fn select_index<'a>(node: &LinkedNode<'a>) -> Option<(i64, LinkedNode<'a>)> {
    let parent = node.parent()?;
    let access = parent.cast::<ast::FieldAccess>()?;
    if access.target().to_untyped() != node.get() || access.field().as_str() != "at" {
        return None;
    }
    let (ast::Expr::Int(index), call) = at_selector(parent)? else {
        return None;
    };
    Some((index.get(), call))
}

/// The `.at("<key>")` spelling of a property step. Reaches the keys an
/// identifier cannot spell.
fn select_by_at<'a>(
    access: &LinkedNode<'a>,
    keys: &[String],
) -> Option<(String, LinkedNode<'a>)> {
    let (ast::Expr::Str(key), call) = at_selector(access)? else {
        return None;
    };
    let key = key.get().to_string();
    keys.contains(&key).then_some((key, call))
}

/// The selector argument of the `<expr>.at(..)` call `access` heads, and the call
/// node the selection widens to. `access` is the `<expr>.at` field access.
fn at_selector<'a>(access: &LinkedNode<'a>) -> Option<(ast::Expr<'a>, LinkedNode<'a>)> {
    let parent = access.parent()?;
    // Cast off the underlying node, whose lifetime is the tree's: casting off the
    // `LinkedNode` would tie the returned expression to this borrow of `access`.
    let call: ast::FuncCall = parent.get().cast()?;
    let ast::Expr::FieldAccess(callee) = call.callee() else {
        return None;
    };
    if callee.to_untyped() != access.get() {
        return None;
    }
    let selector = call.args().items().find_map(|arg| match arg {
        ast::Arg::Pos(expr) => Some(expr),
        _ => None,
    })?;
    Some((selector, parent.clone()))
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

    /// The premise the whole mechanism stands on.
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
        let rt = quillmark_content::import::from_markdown("A probe paragraph, PROBETOKEN.")
            .expect("import");
        let data =
            serde_json::json!({ "intro": quillmark_content::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&data);
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(transformed.as_ref(), &meta)
            .expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let intro_idx = windows
            .iter()
            .position(|w| w.path == "intro")
            .expect("intro window");
        let mut cls = Classifier::new(&world, &helper, &windows);
        let mut hits = Vec::new();
        for (page, p) in doc.pages().iter().enumerate() {
            collect_page_hits(&p.frame, page, &mut cls, &mut Markers::default(), &mut hits);
        }
        assert!(
            hits.iter()
                .any(|h| matches!(h.class.owner(), Some(Owner::Window(w)) if w == intro_idx)),
            "block output glyphs must classify into the helper file's recorded window {:?}",
            windows[intro_idx].range
        );
    }

    /// `underline`/`strike` draw a decoration `Shape` with a detached span
    /// between the decorated glyphs and the trailing plain run; classifying it
    /// `Foreign` would truncate the region at the mark's start.
    #[test]
    fn decoration_marks_do_not_truncate_the_region() {
        use quillmark_content::model::{Line, LineKind, Mark, MarkKind, Content};
        const YAML: &str = r#"
quill:
  name: deco_probe
  version: 0.1.0
  backend: typst
  description: underline and strike region truncation probe
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: one paragraph with one decorated run
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 400pt, margin: 40pt)
#data.body
"#;
        const TEXT: &str = "Start uline and then a long trailing plain run of text.";
        let region_width = |kind: MarkKind| -> f32 {
            let rt = Content::new(TEXT.to_string(), vec![Line::new(LineKind::Para)])
                .with_marks(vec![Mark::new(6, 11, kind)])
                .into_normalized();
            let q = quill(YAML, PLATE);
            let plate = crate::read_plate(&q).expect("plate");
            let schema = quillmark_core::quill::build_transform_schema(q.config());
            let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
            let data =
                serde_json::json!({ "body": quillmark_content::serial::to_canonical_value(&rt) });
            let transformed = crate::transformed_data(&data);
            let mut world = QuillWorld::new(&q, &plate).expect("world");
            let windows = world
                .inject_helper_package(transformed.as_ref(), &meta)
                .expect("inject");
            let (doc, _) = compile_document(&world).expect("compile");
            let helper = world
                .source(QuillWorld::helper_fid("lib.typ"))
                .expect("helper source");
            let regions = Scan {
                doc: &doc,
                world: &world,
                helper: &helper,
                windows: &windows,
                unclosed: &[],
            }
            .regions();
            regions
                .iter()
                .filter(|r| r.field == "body")
                .map(|r| r.rect[2] - r.rect[0])
                .fold(0.0f32, f32::max)
        };

        // `strong` stands in for "no decoration shape".
        let baseline = region_width(MarkKind::Strong);
        assert!(
            baseline > 150.0,
            "sanity: full-line region is wide: {baseline}"
        );
        for kind in [MarkKind::Emph, MarkKind::Underline, MarkKind::Strike] {
            let w = region_width(kind.clone());
            assert!(
                w >= baseline * 0.9,
                "{kind:?} region width {w} truncated vs {baseline} baseline"
            );
        }
    }

    /// `(path, source text)` per window, over the fields the tests below address.
    fn probe_spans(src: &Source) -> Vec<(String, String)> {
        let root = AddressNode::from_schema(&serde_json::json!({
            "properties": {
                "subject": { "type": "string" },
                "other": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "refs": { "type": "array", "items": { "type": "object", "properties": {
                    "org": { "type": "string" },
                    "num": { "type": "string" },
                }}},
                "classification": { "type": "object", "properties": {
                    "value": { "type": "string" },
                    "poc": { "type": "string" },
                    "controlled by": { "type": "string" },
                }},
                "contact": { "type": "object", "properties": {
                    "address": { "type": "object", "properties": {
                        "city": { "type": "string" },
                    }},
                    "log": { "type": "array", "items": { "type": "object", "properties": {
                        "note": { "type": "string" },
                    }}},
                }},
            }
        }));
        scalar_windows(src, &root)
            .into_iter()
            .map(|(path, r)| (path, src.text()[r].to_string()))
            .collect()
    }

    fn has(spans: &[(String, String)], path: &str, text: &str) -> bool {
        spans.iter().any(|(p, t)| p == path && t == text)
    }

    /// A read that stops partway anchors where it stopped, so naming a
    /// container is its own address rather than the leaf's.
    #[test]
    fn scalar_windows_take_every_step_the_plate_takes() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.contact.address.city
#data.contact.address
#data.contact
#data.contact.log.at(0).note
#data.contact.address.city.oops
#let a = data.contact.address
#a.city
"#,
        );
        let spans = probe_spans(&src);
        for (path, text) in [
            ("contact.address.city", "data.contact.address.city"),
            ("contact.address", "data.contact.address"),
            ("contact", "data.contact"),
            ("contact.log.0.note", "data.contact.log.at(0).note"),
            // An undeclared step past a leaf mints nothing of its own: the read
            // anchors on the deepest declared cell it reached.
            ("contact.address.city", "data.contact.address.city.oops"),
            ("contact.address.city", "a.city"),
        ] {
            assert!(has(&spans, path, text), "{path} / {text}: {spans:?}");
        }
    }

    /// Naming the container before stepping into it must keep the address the
    /// direct chain carries.
    #[test]
    fn scalar_windows_follow_a_single_assignment_let_alias() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#let c = data.classification
#c.poc
#c.at("controlled by")
#c
#c.undeclared
#let s = data.subject
#s
#upper(s)
#let a = data.at("classification", default: (:))
#a.poc
"#,
        );
        let spans = probe_spans(&src);
        for (path, text) in [
            ("classification.poc", "c.poc"),
            ("classification.controlled by", "c.at(\"controlled by\")"),
            ("classification", "c"),
            ("subject", "s"),
            ("subject", "upper(s)"),
            ("classification.poc", "a.poc"),
        ] {
            assert!(has(&spans, path, text), "missing {path}/{text}: {spans:?}");
        }
        assert!(
            has(&spans, "classification", "c.undeclared"),
            "an undeclared key falls back to the container: {spans:?}"
        );
    }

    /// A name a second binder could rebind carries no alias: attributing
    /// another value's ink to the field is worse than surfacing no region.
    #[test]
    fn scalar_windows_drop_an_alias_any_second_binder_could_rebind() {
        let rebound = |plate: &str| {
            let src = Source::detached(&format!(
                "#import \"@local/quillmark-helper:0.1.0\": data\n{plate}\n"
            ));
            probe_spans(&src)
                .into_iter()
                .filter(|(p, _)| p.starts_with("classification"))
                .filter(|(_, t)| !t.starts_with("data"))
                .collect::<Vec<_>>()
        };
        for plate in [
            "#let c = data.classification\n#let c = \"other\"\n#c.poc",
            "#let c = data.classification\n#let f(c) = [#c.poc]\n#f(1)",
            "#let c = data.classification\n#let f(c: 1) = [#c.poc]\n#f()",
            "#let c = data.classification\n#for c in (1, 2) [#c.poc]",
            "#let c = data.classification\n#{ c = \"other\" }\n#c.poc",
            "#let c = data.classification\n#import \"x.typ\": *\n#c.poc",
            // The wildcard binds names this walk never sees, whichever side of
            // the binding it sits on.
            "#import \"x.typ\": *\n#let c = data.classification\n#c.poc",
            "#let (c, d) = (data.classification, 1)\n#c.poc",
        ] {
            assert!(
                rebound(plate).is_empty(),
                "alias survived a rebind in {plate:?}: {:?}",
                rebound(plate)
            );
        }
    }

    /// An occurrence spelling the alias as a *name* reads nothing off the field,
    /// so it anchors nothing: attributing a callee's argument to the field is a
    /// wrong address, worse than the missing one dropping the alias would leave.
    #[test]
    fn scalar_windows_anchor_an_alias_only_where_the_name_is_read() {
        let named = |plate: &str| {
            let src = Source::detached(&format!(
                "#import \"@local/quillmark-helper:0.1.0\": data\n{plate}\n"
            ));
            probe_spans(&src)
                .into_iter()
                .filter(|(_, t)| !t.starts_with("data"))
                .collect::<Vec<_>>()
        };
        for plate in [
            "#let other = data.other\n#text(other: 12pt)[Hello]",
            "#let other = data.other\n#figure(caption: [Cap], other: 1)[Body]",
            "#let other = data.other\n#let d = (other: [Inner])",
            "#let other = data.other\n#set text(other: 1)",
            "#let subject = data.subject\n#box[#mydict.subject]",
            // The bound name is the path's last segment, so an earlier segment
            // survives the name rule while naming a module, not a value.
            "#let other = data.other\n#import \"x.typ\": other.deeper",
        ] {
            assert!(
                named(plate).is_empty(),
                "a name position anchored in {plate:?}: {:?}",
                named(plate)
            );
        }
    }

    /// The collision is between a name and an occurrence, so the genuine read
    /// still carries the address while the key beside it carries none.
    #[test]
    fn scalar_windows_keep_the_genuine_read_beside_a_colliding_key() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#let subject = data.subject
#let styles = (subject: [UNRELATED INK])
#styles.subject
#subject
#upper(subject)
"#,
        );
        let spans = probe_spans(&src);
        for text in ["subject", "upper(subject)"] {
            assert!(has(&spans, "subject", text), "missing {text}: {spans:?}");
        }
        for text in ["subject: [UNRELATED INK]", "styles.subject"] {
            assert!(
                !spans.iter().any(|(_, t)| t == text),
                "{text:?} is no read of the field: {spans:?}"
            );
        }
    }

    /// An initializer that is not one whole chain names no single field.
    #[test]
    fn scalar_windows_alias_only_a_whole_data_chain() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#let w = upper(data.subject)
#w
#let m = data.subject + data.other
#m
"#,
        );
        let spans = probe_spans(&src);
        for text in ["w", "m"] {
            assert!(
                !spans.iter().any(|(_, t)| t == text),
                "{text:?} is no alias: {spans:?}"
            );
        }
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
        let spans = probe_spans(&src);
        for (path, text) in [
            ("subject", "data.subject"),
            ("subject", "data.at(\"subject\")"),
            ("refs.0", "data.refs.at(0)"),
            ("other", "data.other"),
            ("subject", "upper(data.subject)"),
        ] {
            assert!(has(&spans, path, text), "missing {path}/{text}: {spans:?}");
        }
        assert!(
            !spans
                .iter()
                .any(|(_, t)| t.contains("data.subject + data.other")),
            "multi-reference expressions are not attributed: {spans:?}"
        );
        let at = |text: &str| {
            spans
                .iter()
                .position(|(p, t)| p == "subject" && t == text)
                .unwrap()
        };
        assert!(
            at("data.subject") < at("upper(data.subject)"),
            "chains sort before wides: {spans:?}"
        );
    }

    /// The container step: a property read anchors on the property, so the
    /// region carries an address `_qm-known-path` would accept, while the
    /// container read whole still anchors on the container.
    #[test]
    fn scalar_windows_step_into_a_declared_container_property() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.classification.value
#data.classification.poc
#data.classification.at("controlled by")
#data.at("classification").poc
#data.classification
#data.classification.undeclared
#upper(data.classification.poc)
"#,
        );
        let spans = probe_spans(&src);
        for (path, text) in [
            ("classification.value", "data.classification.value"),
            ("classification.poc", "data.classification.poc"),
            (
                "classification.controlled by",
                "data.classification.at(\"controlled by\")",
            ),
            ("classification.poc", "data.at(\"classification\").poc"),
            ("classification", "data.classification"),
            ("classification.poc", "upper(data.classification.poc)"),
        ] {
            assert!(has(&spans, path, text), "missing {path}/{text}: {spans:?}");
        }
        // An undeclared key is no address, so the read falls back to the
        // container rather than minting `classification.undeclared`.
        assert!(
            !spans.iter().any(|(p, _)| p.ends_with(".undeclared")),
            "an undeclared key mints no address: {spans:?}"
        );
        assert!(
            has(&spans, "classification", "data.classification.undeclared"),
            "the undeclared read still anchors on the container: {spans:?}"
        );
    }

    #[test]
    fn scalar_windows_step_into_a_typed_table_row() {
        let src = Source::detached(
            r#"
#import "@local/quillmark-helper:0.1.0": data
#data.refs.at(0).org
#data.refs.at(12).at("num")
#data.at("refs").at(0).org
#let row = data.refs.at(0)
#row.org
#data.refs.at(0)
#data.tags.at(3)
#data.refs
#data.refs.at(0).undeclared
#data.refs.at(-1)
#data.tags.at(0).org
"#,
        );
        let spans = probe_spans(&src);
        for (path, text) in [
            ("refs.0.org", "data.refs.at(0).org"),
            ("refs.12.num", "data.refs.at(12).at(\"num\")"),
            ("refs.0.org", "data.at(\"refs\").at(0).org"),
            ("refs.0.org", "row.org"),
            ("refs.0", "data.refs.at(0)"),
            ("tags.3", "data.tags.at(3)"),
            ("refs", "data.refs"),
            // An undeclared row key is no address, so the read falls back to the
            // row; a primitive element offers no property at all.
            ("refs.0", "data.refs.at(0).undeclared"),
            ("tags.0", "data.tags.at(0).org"),
        ] {
            assert!(has(&spans, path, text), "missing {path}/{text}: {spans:?}");
        }
        // A negative index lexes as unary minus over the magnitude, so it never
        // matches the integer arm and the read stays on the table.
        assert!(
            has(&spans, "refs", "data.refs.at(-1)"),
            "a negative index mints no address: {spans:?}"
        );
    }

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

    /// Typst's synthesized list marker carries a detached span, so it lands in
    /// the "no window" bucket rather than producing `(window, None)` ink.
    #[test]
    fn two_tier_classification_resolves_each_segment_independently() {
        const YAML: &str = r#"
quill:
  name: two_tier_probe
  version: 0.1.0
  backend: typst
  description: two-tier classification probe
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
            quillmark_content::import::from_markdown("- Item ONE\n- Item TWO").expect("import");
        let data =
            serde_json::json!({ "body": quillmark_content::serial::to_canonical_value(&rt) });
        let transformed = crate::transformed_data(&data);
        let mut world = QuillWorld::new(&q, &plate).expect("world");
        let windows = world
            .inject_helper_package(transformed.as_ref(), &meta)
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

        let mut cls = Classifier::new(&world, &helper, &windows);
        let mut spans = Vec::new();
        for p in doc.pages().iter() {
            collect_spans(&p.frame, &mut spans);
        }

        let mut seg_hits = [0usize; 2];
        let mut field_only_hits = 0usize;
        let mut untracked_hits = 0usize;
        for span in spans {
            match cls.classify_seg(span) {
                Some((w, Some(s))) if w == win_idx => seg_hits[s] += 1,
                Some((w, None)) if w == win_idx => field_only_hits += 1,
                _ => untracked_hits += 1,
            }
        }
        assert!(
            seg_hits[0] > 0 && seg_hits[1] > 0,
            "each list item's own ink resolves to its own segment: {seg_hits:?}"
        );
        assert_eq!(field_only_hits, 0, "list markers produce no (window, None) ink");
        assert!(
            untracked_hits > 0,
            "the two markers are hit but resolve to no window (detached span)"
        );
    }

    const REGION_YAML: &str = r#"
quill:
  name: region_probe
  version: 0.1.0
  backend: typst
  description: field-region claim probe
typst:
  plate_file: plate.typ
main:
  fields:
    body:
      type: richtext
      description: the body
    classification:
      type: string
      description: a scalar the plate interpolates
"#;

    /// A compiled plate with the inputs both queries take. Mirrors `open`'s
    /// window assembly, so a probe reads what a session would.
    struct Probe {
        world: QuillWorld,
        doc: PagedDocument,
        helper: Source,
        windows: Vec<FieldWindow>,
    }

    fn probe(plate: &str, data: serde_json::Value) -> Probe {
        let q = quill(REGION_YAML, plate);
        let plate_src = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(q.config());
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let transformed = crate::transformed_data(&data);
        let mut world = QuillWorld::new(&q, &plate_src).expect("world");
        let mut windows = world
            .inject_helper_package(transformed.as_ref(), &meta)
            .expect("inject");
        let main_id = world.main();
        let src = world.source(main_id).expect("main source");
        windows.extend(
            scalar_windows(&src, &meta.root)
                .into_iter()
                .map(|(path, range)| FieldWindow {
                    path,
                    file: main_id,
                    range,
                    segments: Vec::new(),
                }),
        );
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");
        Probe {
            world,
            doc,
            helper,
            windows,
        }
    }

    impl Probe {
        /// What `open` stores on the session: the claims left open at the end
        /// of the walk, by index.
        fn unclosed(&self) -> Vec<usize> {
            unclosed_claims(&self.doc).into_iter().map(|(c, _)| c).collect()
        }

        fn scan<'a>(&'a self, unclosed: &'a [usize]) -> Scan<'a> {
            Scan {
                doc: &self.doc,
                world: &self.world,
                helper: &self.helper,
                windows: &self.windows,
                unclosed,
            }
        }
    }

    fn probe_regions(plate: &str, data: serde_json::Value) -> Vec<RenderedRegion> {
        let p = probe(plate, data);
        p.scan(&p.unclosed()).regions()
    }

    fn body(markdown: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(markdown).expect("import");
        serde_json::json!({
            "body": quillmark_content::serial::to_canonical_value(&rt),
            "classification": "UNCLASSIFIED",
        })
    }

    /// The premise of the wrapper: ink a package function draws carries that
    /// function's spans, so only the markers can attribute it.
    #[test]
    fn field_region_claims_ink_no_window_tracks() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let banner(level) = box(stroke: 1pt, inset: 4pt)[#upper(level)]
#field-region("classification")[#banner("secret")]
"#;
        let regions = probe_regions(PLATE, body("A paragraph."));
        let claimed: Vec<&RenderedRegion> = regions
            .iter()
            .filter(|r| r.field == "classification")
            .collect();
        assert_eq!(claimed.len(), 1, "one claim, one region: {regions:?}");
        assert!(claimed[0].span.is_none(), "a claim carries no content span");
        assert!(
            claimed[0].rect[2] > claimed[0].rect[0] && claimed[0].rect[3] > claimed[0].rect[1],
            "the claim boxes the banner's ink: {:?}",
            claimed[0].rect
        );
    }

    #[test]
    fn field_region_yields_to_the_fields_nested_inside_it() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 400pt, margin: 40pt)
#field-region("classification")[
  #line(length: 100pt)
  #data.body
]
"#;
        let regions = probe_regions(PLATE, body("Body PROBETOKEN text."));
        assert!(
            regions.iter().any(|r| r.field == "body" && r.span.is_some()),
            "the nested content field keeps its own span-bearing region: {regions:?}"
        );
        assert!(
            regions.iter().any(|r| r.field == "classification"),
            "the wrapper still claims the rule it drew itself: {regions:?}"
        );
    }

    #[test]
    fn a_nested_scalar_reference_outranks_the_wrapper() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 400pt, margin: 40pt)
#field-region("body")[Level: #data.classification]
"#;
        let regions = probe_regions(PLATE, body("Unused."));
        assert!(
            regions.iter().any(|r| r.field == "classification"),
            "the scalar site keeps its own region inside the wrapper: {regions:?}"
        );
    }

    /// Two calls must not collapse into one shared first placement, or a
    /// per-card wrapper would surface a single card.
    #[test]
    fn each_field_region_call_claims_separately() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let stamp(n) = box(stroke: 1pt, inset: 4pt)[#n]
#field-region("classification")[#stamp("one")]
Interleaved plate chrome.
#field-region("classification")[#stamp("two")]
"#;
        let regions = probe_regions(PLATE, body("Unused."));
        let claimed = regions.iter().filter(|r| r.field == "classification").count();
        assert_eq!(claimed, 2, "one region per call: {regions:?}");
    }

    /// An interruption must not truncate a claim: its extent is explicit, so
    /// there is no second-placement ambiguity to be conservative about.
    #[test]
    fn a_claim_accrues_across_an_interruption() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let chrome() = box(width: 40pt)[--]
#field-region("classification")[LEFTMOST #chrome() #data.body RIGHTMOST]
"#;
        let regions = probe_regions(PLATE, body("mid"));
        let claim = regions
            .iter()
            .find(|r| r.field == "classification")
            .expect("the claim surfaces");
        let body_box = regions
            .iter()
            .find(|r| r.field == "body")
            .expect("the nested field surfaces");
        // Bottom-left origin: the trailing text sits below the nested field.
        assert!(
            claim.rect[1] < body_box.rect[1],
            "the claim reaches past the nested field's ink to its own trailing \
             text: claim {:?} vs body {:?}",
            claim.rect,
            body_box.rect
        );
    }

    /// Typst never separates the two markers on its own, so the way to strand an
    /// open is a plate emitting the call's return value in parts.
    const STRANDED_OPEN: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 300pt, height: 200pt, margin: 20pt, header: [PAGE CHROME])
#let r = field-region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#data.body
"#;

    #[test]
    fn an_open_marker_whose_close_never_lands_is_reported_unclosed() {
        let p = probe(STRANDED_OPEN, body("Body text."));
        assert_eq!(
            unclosed_claims(&p.doc),
            vec![(0, "classification".to_string())],
            "the stranded open is named, so a plate author can act on it"
        );
    }

    /// A claim left open would own every unattributed hit to the end of the
    /// document, page chrome included.
    #[test]
    fn an_unclosed_claim_surfaces_no_region_at_all() {
        let long = body(&"A long paragraph of body text. ".repeat(60));
        let regions = probe_regions(STRANDED_OPEN, long.clone());
        assert!(
            regions.iter().any(|r| r.field == "body"),
            "the fields that do resolve are untouched: {regions:?}"
        );
        assert!(
            !regions.iter().any(|r| r.field == "classification"),
            "the unbounded claim yields nothing rather than every page's chrome: \
             {:?}",
            regions
                .iter()
                .filter(|r| r.field == "classification")
                .map(|r| r.page)
                .collect::<Vec<_>>()
        );
    }

    /// The point query cannot derive the suppression set itself, so this guards
    /// that it is threaded through.
    #[test]
    fn an_unclosed_claim_does_not_capture_clicks_on_later_pages() {
        let p = probe(STRANDED_OPEN, body(&"A long paragraph of body text. ".repeat(60)));
        assert!(p.doc.pages().len() > 1, "the runaway needs a later page");

        let last = p.doc.pages().len() - 1;
        let runaway = p
            .scan(&[])
            .regions()
            .into_iter()
            .find(|r| r.field == "classification" && r.page == last)
            .expect("unsuppressed, the claim reaches the last page");
        let (cx, cy) = (
            (runaway.rect[0] + runaway.rect[2]) / 2.0,
            (runaway.rect[1] + runaway.rect[3]) / 2.0,
        );
        assert_eq!(
            p.scan(&[]).field_at(last, cx, cy, 0.0).map(|(_, f)| f),
            Some("classification".to_string()),
            "sanity: unsuppressed, that ink routes to the stranded claim"
        );
        assert_eq!(
            p.scan(&p.unclosed()).field_at(last, cx, cy, 0.0).map(|(_, f)| f),
            None,
            "suppressed, page chrome under a runaway claim answers to no field"
        );
    }

    /// Suppression is per claim, not per field name: a second, well-formed call
    /// on the same field keeps its region.
    #[test]
    fn a_balanced_claim_beside_an_unclosed_one_still_surfaces() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 300pt, height: 200pt, margin: 20pt)
#let r = field-region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#field-region("classification")[#box(stroke: 1pt)[OK]]
"#;
        let regions = probe_regions(PLATE, body("Body."));
        assert_eq!(
            regions
                .iter()
                .filter(|r| r.field == "classification")
                .count(),
            1,
            "only the balanced call claims: {regions:?}"
        );
    }

    #[test]
    fn an_inner_claim_closed_by_its_enclosing_one_is_not_suppressed() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.1.0": data, field-region
#set page(width: 300pt, height: 200pt, margin: 20pt)
#let inner = field-region("body")[#box(stroke: 1pt)[I]]
#field-region("classification")[#inner.children.at(0) #box(stroke: 1pt)[C]]
#data.body
"#;
        let regions = probe_regions(PLATE, body("Text."));
        assert!(
            regions.iter().any(|r| r.field == "body"),
            "the inner claim is bounded by the outer close: {regions:?}"
        );
    }

    fn aabb(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Aabb {
        Aabb {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    fn boxable_hit(page: usize, key: Key, rect: Aabb) -> Hit {
        Hit {
            page,
            class: HitClass::Boxable { key },
            rect: Some(rect),
        }
    }

    fn transparent_hit(page: usize, window: usize) -> Hit {
        Hit {
            page,
            class: HitClass::Transparent { window },
            rect: None,
        }
    }

    fn foreign_hit(page: usize) -> Hit {
        Hit {
            page,
            class: HitClass::Foreign,
            rect: None,
        }
    }

    fn anonymous_hit(page: usize) -> Hit {
        Hit {
            page,
            class: HitClass::Anonymous,
            rect: None,
        }
    }

    /// [`run_scan_machine`] read as per-key page sequences, never-accrued keys
    /// dropped.
    fn key_pages(keys: &[Key], hits: &[Hit]) -> HashMap<Key, Vec<usize>> {
        run_scan_machine(keys, hits)
            .into_iter()
            .enumerate()
            .filter(|(_, b)| !b.is_empty())
            .map(|(i, b)| (keys[i], b.into_iter().map(|(p, _)| p).collect()))
            .collect()
    }

    /// `interrupt` between two boxable hits of one segment keeps the run, while
    /// identically-placed foreign ink ends it.
    fn assert_transparent_but_foreign_ink_is_not(interrupt: Hit) {
        let keys = vec![Key::Span(0, Some(0))];
        let (a, b) = (aabb(0.0, 0.0, 1.0, 1.0), aabb(10.0, 10.0, 11.0, 11.0));

        let transparent = vec![
            boxable_hit(0, Key::Span(0, Some(0)), a),
            interrupt,
            boxable_hit(0, Key::Span(0, Some(0)), b),
        ];
        let boxes = run_scan_machine(&keys, &transparent);
        assert_eq!(boxes[0].len(), 1, "one page-0 box");
        let (_, bx) = boxes[0][0];
        assert!(
            bx.min_x <= 0.0 && bx.max_x >= 11.0,
            "the box unions both hits: the run stayed unbroken"
        );

        let foreign = vec![
            boxable_hit(0, Key::Span(0, Some(0)), a),
            foreign_hit(0),
            boxable_hit(0, Key::Span(0, Some(0)), b),
        ];
        let boxes = run_scan_machine(&keys, &foreign);
        assert_eq!(boxes[0].len(), 1, "one page-0 box, unresumed");
        let (_, bx) = boxes[0][0];
        assert!(
            bx.max_x < 11.0,
            "only the first hit accrued: same-page foreign ink ends the run"
        );
    }

    #[test]
    fn field_only_ink_is_transparent_but_foreign_ink_is_not() {
        assert_transparent_but_foreign_ink_is_not(transparent_hit(0, 0));
    }

    #[test]
    fn anonymous_ink_is_transparent_but_foreign_ink_is_not() {
        assert_transparent_but_foreign_ink_is_not(anonymous_hit(0));
    }

    #[test]
    fn adjacent_segments_of_one_field_run_independently() {
        let keys = vec![Key::Span(0, Some(0)), Key::Span(0, Some(1))];
        let r = aabb(0.0, 0.0, 1.0, 1.0);
        let hits = vec![
            boxable_hit(0, Key::Span(0, Some(0)), r),
            transparent_hit(0, 0),
            boxable_hit(0, Key::Span(0, Some(1)), r),
        ];
        let pages = key_pages(&keys, &hits);
        assert_eq!(pages[&Key::Span(0, Some(0))], vec![0]);
        assert_eq!(pages[&Key::Span(0, Some(1))], vec![0]);
    }

    /// Transparency is relative to a *same-window* current run only.
    #[test]
    fn field_only_ink_still_suspends_a_different_fields_current_run() {
        let keys = vec![Key::Span(1, Some(0))];
        let r = aabb(0.0, 0.0, 1.0, 1.0);

        let cross_page = vec![
            boxable_hit(0, Key::Span(1, Some(0)), r), // field 1's segment 0, page 0
            transparent_hit(0, 0),           // field 0's own structural ink
            boxable_hit(1, Key::Span(1, Some(0)), r), // field 1's segment 0 resumes, page 1
        ];
        assert_eq!(
            key_pages(&keys, &cross_page)[&Key::Span(1, Some(0))],
            vec![0, 1],
            "a foreign field's field-only ink suspends the running field \
             (the page-turn tolerance still lets it resume)"
        );

        let same_page = vec![
            boxable_hit(0, Key::Span(1, Some(0)), r),
            transparent_hit(0, 0),
            boxable_hit(0, Key::Span(1, Some(0)), r),
        ];
        assert_eq!(
            key_pages(&keys, &same_page)[&Key::Span(1, Some(0))],
            vec![0],
            "no same-page resume: field-only ink is not a wildcard exception \
             to the foreign-ink suspension rule"
        );
    }
}
