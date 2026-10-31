//! Recover schema-field regions from *glyph spans*: the origin every drawn
//! frame item already carries, surviving a `show`-rule content rebuild because
//! it is a property of the glyph and not of a sibling element. Every ink window
//! is generated source in the helper `lib.typ` — a content field's markup block
//! binding (`#let _qm_cN = [ .. ]`), a `_qm-ink` entry — so every glyph's span
//! nests inside its field's byte window; the walk classifies each frame item by
//! containment, two-tier — which window ([`FieldWindow`]), then which segment —
//! and regions key on `(window, segment)`. A scalar or a date window has no
//! segments.
//!
//! What a region means to a consumer — the producers, the marker-claim
//! fallback, the first-placement rule: `prose/canon/PREVIEW.md`.
//!
//! Geometry composes the group-transform stack exactly like
//! `typst_layout::introspect::discover_frame`, transforming all four corners of
//! each item box (the stack may rotate or scale).

use std::collections::HashMap;
use std::ops::Range;

use typst::foundations::{Content, Label, Value};
use typst::introspection::{Location, Tag};
use typst::layout::{Frame, FrameItem, PlaceElem, Point, Transform};
use typst::model::FootnoteElem;
use typst::pdf::ArtifactElem;
use typst::syntax::{DiagSpan, DiagSpanKind, Source, Span};
use typst::utils::PicoStr;
use typst_layout::{PagedDocument, PagedIntrospector};

use quillmark_core::region::{ContentHit, HitGranularity, RenderedRegion};

use crate::emit::SegmentMap;

/// A tracked byte window in the helper `lib.typ`: the schema field whose ink
/// resolves into `range`. A content field points at its generated markup block,
/// a scalar or a date at its `_qm-ink` entry.
#[derive(Debug, Clone)]
pub(crate) struct FieldWindow {
    pub path: String,
    pub range: Range<usize>,
    /// `generated` ranges index the helper `lib.typ`. Empty for a scalar or a
    /// date, whose whole first placement is one span-less region.
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
    /// One `region` call instance, indexed into [`Markers::claims`].
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
    Boxable {
        key: Key,
    },
    /// A content window's ink between its segments (brackets, container-open
    /// syntax). Suspends a *different* window's run like foreign ink, but is a
    /// no-op while its own window's segment is the run.
    Transparent {
        window: usize,
    },
    /// Ink from a detached span that no claim takes: Typst's synthesized text
    /// decorations (the `underline`/`strike` line, drawn as a `Shape` mid-run),
    /// and list markers outside a claim. Attributable to no field, and never a
    /// run-breaker: a decoration drawn between a field's own glyphs would
    /// otherwise orphan the rest of the line.
    Anonymous,
    /// No window and no open marker, but a *resolvable* span: page chrome,
    /// another field's text, vendored-package output. Breaks a run.
    Foreign,
}

impl HitClass {
    fn owner(self) -> Option<Owner> {
        match self {
            HitClass::Boxable {
                key: Key::Span(w, _),
            } => Some(Owner::Window(w)),
            HitClass::Boxable {
                key: Key::Marker(c),
            } => Some(Owner::Claim(c)),
            HitClass::Transparent { window } => Some(Owner::Window(window)),
            HitClass::Anonymous | HitClass::Foreign => None,
        }
    }
}

/// `(w, None)` splits by window kind: on a segment-less window it is the whole
/// placement's boxable key; on a content window it is inter-segment ink. Falling
/// through to `marker` only on an unresolved span is what makes nested tracked
/// ink outrank an enclosing `region`.
///
/// A claim takes a detached glyph, which is text the plate printed (a `str`
/// cast to content in an argument, a list bullet), but never a detached
/// `Shape` or `Image`: Typst paints an underline or strike stroke after its
/// run's glyphs, inside their box, so a claimed stroke would win
/// [`Scan::field_at`]'s tie against the content field beneath it.
fn hit_class(
    resolved: Option<(usize, Option<usize>)>,
    detached: bool,
    glyph: bool,
    windows: &[FieldWindow],
    marker: Option<usize>,
) -> HitClass {
    match resolved {
        None if detached && !(glyph && marker.is_some()) => HitClass::Anonymous,
        None => match marker {
            Some(c) => HitClass::Boxable {
                key: Key::Marker(c),
            },
            None => HitClass::Foreign,
        },
        Some((w, Some(s))) => HitClass::Boxable {
            key: Key::Span(w, Some(s)),
        },
        Some((w, None)) if windows[w].segments.is_empty() => HitClass::Boxable {
            key: Key::Span(w, None),
        },
        Some((w, None)) => HitClass::Transparent { window: w },
    }
}

/// The label and metadata `kind` the helper's `region` stamps on its two
/// bracketing markers.
const REGION_LABEL: &str = "__qm_region__";

/// The `region` calls open at the current point of the frame walk, and
/// every claim the walk has discovered.
#[derive(Default)]
struct Markers {
    claims: Vec<String>,
    /// Each open claim beside the barrier depth it opened at.
    stack: Vec<(usize, usize)>,
    /// Claim indices [`unclosed_claims`] found still open at the end of the
    /// document. Keeping them off `stack` leaves their ink foreign, exactly as
    /// if the plate had emitted no marker at all.
    suppressed: Vec<usize>,
    /// The out-of-flow ink the walk is inside: a page marginal's
    /// `ArtifactElem`, or a [`relocated`] footnote or float by its anchor's
    /// location. The walk meets this ink where the page puts it, around the
    /// body's text, so a claim open across a page break would otherwise take it.
    barriers: Vec<Location>,
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
                self.stack.push((claim, self.barriers.len()));
            }
        } else if let Some(i) = self
            .stack
            .iter()
            .rposition(|&(c, _)| self.claims[c] == path)
        {
            self.stack.truncate(i);
        }
    }

    fn barrier(&mut self, loc: Location, end: bool) {
        if !end {
            self.barriers.push(loc);
        } else if let Some(i) = self.barriers.iter().rposition(|&l| l == loc) {
            self.barriers.truncate(i);
        }
    }

    /// A barrier hides every claim opened outside it, while one opened inside
    /// (a banner claimed in the header, a claim in a footnote) takes its ink.
    fn current(&self) -> Option<usize> {
        self.stack
            .last()
            .filter(|&&(_, depth)| depth == self.barriers.len())
            .map(|&(c, _)| c)
    }
}

/// A footnote entry or a float: content Typst lays out away from its anchor,
/// in a group whose logical parent is the anchoring element. A table cell
/// broken across pages is parented to the cell, and stays in flow.
fn relocated(flow: &PagedIntrospector, parent: Location) -> bool {
    flow.elements()
        .get_by_loc(&parent)
        .is_some_and(|e| e.is::<FootnoteElem>() || e.is::<PlaceElem>())
}

/// The location of an `ArtifactElem` start or end tag.
fn artifact_edge(tag: &Tag) -> Option<(Location, bool)> {
    match tag {
        Tag::Start(elem, _) if elem.is::<ArtifactElem>() => Some((elem.location()?, false)),
        Tag::End(loc, ..) => Some((*loc, true)),
        _ => None,
    }
}

/// The `(claim, field)` of every `region` call whose open marker reached a
/// frame but whose close never did, over the whole document. A claim an
/// enclosing close unwound past counts as closed: its extent was bounded, so
/// nothing ran away.
pub(crate) fn unclosed_claims(doc: &PagedDocument) -> Vec<(usize, String)> {
    let mut markers = Markers::default();
    for page in doc.pages() {
        scan_markers(&page.frame, doc.introspector(), &mut markers);
    }
    markers
        .stack
        .iter()
        .map(|&(c, _)| (c, markers.claims[c].clone()))
        .collect()
}

/// `(field, is-close)` if `elem` is one of `region`'s markers.
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
    helper: &'a Source,
    windows: &'a [FieldWindow],
    /// Built on the first helper span resolved: see [`node_ranges`].
    helper_nodes: Option<Vec<(u64, Range<usize>)>>,
    memo: HashMap<Span, Option<Range<usize>>>,
}

impl<'a> Classifier<'a> {
    /// `windows` are disjoint and ascending, so a glyph finds its window by
    /// binary search rather than a scan of the whole table.
    fn new(helper: &'a Source, windows: &'a [FieldWindow]) -> Self {
        debug_assert!(
            windows
                .windows(2)
                .all(|w| w[0].range.end <= w[1].range.start),
            "ink windows are appended in byte order and never nest"
        );
        Self {
            helper,
            windows,
            helper_nodes: None,
            memo: HashMap::new(),
        }
    }

    /// The byte range a helper span resolves to: the same unpack
    /// `WorldExt::range` performs, against the served compile's snapshot rather
    /// than the world. Every window lives in the helper, so a span in any other
    /// file resolves to none.
    fn range_of(&mut self, span: Span) -> Option<Range<usize>> {
        if let Some(cached) = self.memo.get(&span) {
            return cached.clone();
        }
        let resolved = match DiagSpan::from(span).get() {
            DiagSpanKind::Number { id, sub_range, .. } if id == self.helper.id() => {
                let helper = self.helper;
                let nodes = self.helper_nodes.get_or_insert_with(|| node_ranges(helper));
                let raw = span.into_raw().get();
                nodes.binary_search_by_key(&raw, |(k, _)| *k).ok().map(|i| {
                    let overall = nodes[i].1.clone();
                    sub_range.map_or(overall.clone(), |s| s.to_absolute(overall.start))
                })
            }
            DiagSpanKind::Range { id, range } if id == self.helper.id() => Some(range),
            _ => None,
        };
        self.memo.insert(span, resolved.clone());
        resolved
    }

    /// Resolves to the innermost segment whose `generated` range contains the
    /// span, or `None` for inter-segment / segment-less ink. The only candidate
    /// window is the last one starting at or before the span.
    fn classify_seg(&mut self, span: Span) -> Option<(usize, Option<usize>)> {
        let range = self.range_of(span)?;
        let k = self
            .windows
            .partition_point(|w| w.range.start <= range.start);
        let window = k
            .checked_sub(1)
            .filter(|&i| range.end <= self.windows[i].range.end)?;
        Some((window, self.seg_of(window, &range)))
    }

    /// Segments are `generated`-ordered and disjoint, so the sole candidate is
    /// the last one starting at or before `range.start`.
    fn seg_of(&self, window: usize, range: &Range<usize>) -> Option<usize> {
        let segs = &self.windows[window].segments;
        let i = segs.partition_point(|s| s.generated.start <= range.start);
        (i > 0 && segs[i - 1].generated.end >= range.end).then(|| i - 1)
    }
}

/// Every node of `source` as `(raw span, byte range)`, sorted by span. A file's
/// numbered spans ascend in pre-order, so the walk emits them sorted; this is
/// `Source::range` found by binary search, where `Source::range` walks each
/// level's siblings in turn and a data literal's thousand-row array makes that
/// walk quadratic over a query's glyphs.
fn node_ranges(source: &Source) -> Vec<(u64, Range<usize>)> {
    fn walk(node: &typst::syntax::SyntaxNode, at: usize, out: &mut Vec<(u64, Range<usize>)>) {
        if !node.span().is_detached() {
            out.push((node.span().into_raw().get(), at..at + node.len()));
        }
        let mut child_at = at;
        for child in node.children() {
            walk(child, child_at, out);
            child_at += child.len();
        }
    }
    let mut out = Vec::new();
    walk(source.root(), 0, &mut out);
    if !out.is_sorted_by_key(|(k, _)| *k) {
        out.sort_by_key(|(k, _)| *k);
    }
    out
}

/// Records `page`'s ink against the marker stack, which pages before it may
/// have left open.
fn collect_page_hits(
    frame: &Frame,
    page: usize,
    flow: &PagedIntrospector,
    cls: &mut Classifier,
    markers: &mut Markers,
    out: &mut Vec<Hit>,
) {
    walk_items(
        frame,
        Transform::identity(),
        page,
        &mut |page, item| match item {
            Item::Marker { path, close } => markers.edge(&path, close),
            Item::Artifact { loc, end } => markers.barrier(loc, end),
            Item::Parent { loc, end } => {
                if relocated(flow, loc) {
                    markers.barrier(loc, end);
                }
            }
            Item::Ink {
                span, glyph, aabb, ..
            } => {
                let class = hit_class(
                    cls.classify_seg(span),
                    span.is_detached(),
                    glyph,
                    cls.windows,
                    markers.current(),
                );
                // Foreign ink still emits a rect-less Hit so the run machine sees
                // the full ink sequence.
                let rect = class.owner().is_some().then(aabb);
                out.push(Hit { page, class, rect });
            }
        },
    );
}

/// The marker half of [`collect_page_hits`] for a page whose ink a query
/// discards, since a claim opening there may still cover the page it wants.
/// Skipping the per-glyph walk keeps that lookbehind linear in frame items.
fn scan_markers(frame: &Frame, flow: &PagedIntrospector, markers: &mut Markers) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => {
                let barrier = group
                    .parent
                    .map(|p| p.location)
                    .filter(|&l| relocated(flow, l));
                if let Some(loc) = barrier {
                    markers.barrier(loc, false);
                }
                scan_markers(&group.frame, flow, markers);
                if let Some(loc) = barrier {
                    markers.barrier(loc, true);
                }
            }
            FrameItem::Tag(tag) => {
                if let Tag::Start(elem, _) = tag {
                    if let Some((path, close)) = region_marker(elem) {
                        markers.edge(&path, close);
                    }
                }
                if let Some((loc, end)) = artifact_edge(tag) {
                    markers.barrier(loc, end);
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
        /// A glyph rather than a `Shape` or `Image`.
        glyph: bool,
        aabb: &'a dyn Fn() -> Aabb,
    },
    Marker {
        path: String,
        close: bool,
    },
    /// An `ArtifactElem` tag. An end tag names its element only by location,
    /// so every end tag reports, and [`Markers::barrier`] matches it.
    Artifact {
        loc: Location,
        end: bool,
    },
    /// Entering or leaving a group with a logical parent.
    Parent {
        loc: Location,
        end: bool,
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
                let parent = group.parent.map(|p| p.location);
                if let Some(loc) = parent {
                    visit(page, Item::Parent { loc, end: false });
                }
                walk_items(&group.frame, ts, page, visit);
                if let Some(loc) = parent {
                    visit(page, Item::Parent { loc, end: true });
                }
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
                            glyph: true,
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
                        glyph: false,
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
                        glyph: false,
                        aabb: &|| item_aabb(p, Point::zero(), sz, ts),
                    },
                );
            }
            FrameItem::Tag(tag) => {
                if let Tag::Start(elem, _) = tag {
                    if let Some((path, close)) = region_marker(elem) {
                        visit(page, Item::Marker { path, close });
                    }
                }
                if let Some((loc, end)) = artifact_edge(tag) {
                    visit(page, Item::Artifact { loc, end });
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
    pub(crate) helper: &'a Source,
    pub(crate) windows: &'a [FieldWindow],
    pub(crate) unclosed: &'a [usize],
}

impl<'a> Scan<'a> {
    fn classifier(&self) -> Classifier<'a> {
        Classifier::new(self.helper, self.windows)
    }

    /// `page`'s ink, or every page's when `page` is `None`, plus the marker
    /// stack the walk ended on. Pages before `page` are walked for their markers
    /// alone, since a claim opening there may still cover the page wanted;
    /// skipping their glyphs keeps that lookbehind linear in frame items.
    fn hits(&self, page: Option<usize>) -> (Vec<Hit>, Markers) {
        let mut cls = self.classifier();
        let mut markers = Markers::suppressing(self.unclosed);
        let mut hits = Vec::new();
        let flow = self.doc.introspector();
        for (i, p) in self.doc.pages().iter().enumerate() {
            match page {
                Some(want) if i < want => scan_markers(&p.frame, flow, &mut markers),
                Some(want) if i > want => break,
                _ => collect_page_hits(&p.frame, i, flow, &mut cls, &mut markers, &mut hits),
            }
        }
        (hits, markers)
    }

    /// Each span window's **first placement** and each `region` call's
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
                // Geometry with no content address, like a scalar's ink.
                Key::Marker(ci) => (markers.claims[ci].clone(), None),
            };
            for (page, b) in &boxes[ki] {
                let Some(page_h) = self
                    .doc
                    .pages()
                    .get(*page)
                    .map(|p| p.frame.size().y.to_pt())
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
    pub(crate) fn field_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<(f32, String)> {
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
            walk_glyphs(
                &p.frame,
                page,
                &mut cls,
                Some((wi, Some(seg_idx))),
                &mut hits,
            );
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
    let key_index: HashMap<Key, usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
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
/// offset that [`position_at`](Scan::position_at)/[`locate`](Scan::locate) need and a region scan discards.
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
        let Item::Ink {
            span, offset, aabb, ..
        } = item
        else {
            return;
        };
        let Some((w, seg)) = cls.classify_seg(span) else {
            return;
        };
        if only.is_some_and(|t| t != (w, seg)) {
            return;
        }
        if let Some(node) = cls.range_of(span) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_document;
    use crate::world::QuillWorld;
    use quillmark_core::quill::{FileTreeNode, Quill};
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
      multiline: true
      description: a probe field
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#qm.data.intro
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(
            q.config(),
            quillmark_core::quill::SchemaCodec::Content,
        );
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let rt = quillmark_content::import::from_markdown("A probe paragraph, PROBETOKEN.")
            .expect("import")
            .content;
        let data =
            serde_json::json!({ "intro": quillmark_content::serial::to_canonical_value(&rt) });
        let mut world = QuillWorld::new(&q, &plate, &crate::FontLibrary::default()).expect("world");
        let windows = world.inject_helper_package(&data, &meta).expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let intro_idx = windows
            .iter()
            .position(|w| w.path == "intro")
            .expect("intro window");
        let mut cls = Classifier::new(&helper, &windows);
        let mut hits = Vec::new();
        for (page, p) in doc.pages().iter().enumerate() {
            collect_page_hits(
                &p.frame,
                page,
                doc.introspector(),
                &mut cls,
                &mut Markers::default(),
                &mut hits,
            );
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
        use quillmark_content::model::{Content, Line, LineKind, Mark, MarkKind};
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
    prose:
      type: richtext
      multiline: true
      description: one paragraph with one decorated run
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#qm.data.prose
"#;
        const TEXT: &str = "Start uline and then a long trailing plain run of text.";
        let region_width = |kind: MarkKind| -> f32 {
            let rt = Content::new(TEXT.to_string(), vec![Line::new(LineKind::Para)])
                .with_marks(vec![Mark::new(6, 11, kind)])
                .into_normalized();
            let q = quill(YAML, PLATE);
            let plate = crate::read_plate(&q).expect("plate");
            let schema = quillmark_core::quill::build_transform_schema(
                q.config(),
                quillmark_core::quill::SchemaCodec::Content,
            );
            let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
            let data =
                serde_json::json!({ "prose": quillmark_content::serial::to_canonical_value(&rt) });
            let mut world =
                QuillWorld::new(&q, &plate, &crate::FontLibrary::default()).expect("world");
            let windows = world.inject_helper_package(&data, &meta).expect("inject");
            let (doc, _) = compile_document(&world).expect("compile");
            let helper = world
                .source(QuillWorld::helper_fid("lib.typ"))
                .expect("helper source");
            let regions = Scan {
                doc: &doc,
                helper: &helper,
                windows: &windows,
                unclosed: &[],
            }
            .regions();
            regions
                .iter()
                .filter(|r| r.field == "prose")
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
    prose:
      type: richtext
      multiline: true
      description: a two-item list
"#;
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#qm.data.prose
"#;
        let q = quill(YAML, PLATE);
        let plate = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(
            q.config(),
            quillmark_core::quill::SchemaCodec::Content,
        );
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let rt = quillmark_content::import::from_markdown("- Item ONE\n- Item TWO")
            .expect("import")
            .content;
        let data =
            serde_json::json!({ "prose": quillmark_content::serial::to_canonical_value(&rt) });
        let mut world = QuillWorld::new(&q, &plate, &crate::FontLibrary::default()).expect("world");
        let windows = world.inject_helper_package(&data, &meta).expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");

        let win_idx = windows
            .iter()
            .position(|w| w.path == "prose")
            .expect("prose window");
        assert_eq!(
            windows[win_idx].segments.len(),
            2,
            "one segment per list item"
        );

        let mut cls = Classifier::new(&helper, &windows);
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
        assert_eq!(
            field_only_hits, 0,
            "list markers produce no (window, None) ink"
        );
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
  description: claim probe
typst:
  plate_file: plate.typ
main:
  fields:
    prose:
      type: richtext
      multiline: true
      description: the body
    classification:
      type: enum
      values: [UNCLASSIFIED, SECRET]
      description: a scalar the plate interpolates
"#;

    /// A compiled plate with the inputs both queries take. Mirrors `open`'s
    /// window assembly, so a probe reads what a session would.
    struct Probe {
        doc: PagedDocument,
        helper: Source,
        windows: Vec<FieldWindow>,
    }

    fn probe(plate: &str, data: serde_json::Value) -> Probe {
        let q = quill(REGION_YAML, plate);
        let plate_src = crate::read_plate(&q).expect("plate");
        let schema = quillmark_core::quill::build_transform_schema(
            q.config(),
            quillmark_core::quill::SchemaCodec::Content,
        );
        let meta = crate::SchemaMeta::from_schema_json(schema.as_json());
        let mut world =
            QuillWorld::new(&q, &plate_src, &crate::FontLibrary::default()).expect("world");
        let windows = world.inject_helper_package(&data, &meta).expect("inject");
        let (doc, _) = compile_document(&world).expect("compile");
        let helper = world
            .source(QuillWorld::helper_fid("lib.typ"))
            .expect("helper source");
        Probe {
            doc,
            helper,
            windows,
        }
    }

    impl Probe {
        /// What `open` stores on the session: the claims left open at the end
        /// of the walk, by index.
        fn unclosed(&self) -> Vec<usize> {
            unclosed_claims(&self.doc)
                .into_iter()
                .map(|(c, _)| c)
                .collect()
        }

        fn scan<'a>(&'a self, unclosed: &'a [usize]) -> Scan<'a> {
            Scan {
                doc: &self.doc,
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

    fn prose(markdown: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(markdown)
            .expect("import")
            .content;
        serde_json::json!({
            "prose": quillmark_content::serial::to_canonical_value(&rt),
            "classification": "UNCLASSIFIED",
        })
    }

    /// The premise of the wrapper: ink a package function draws carries that
    /// function's spans, so only the markers can attribute it.
    #[test]
    fn a_claim_takes_ink_no_window_tracks() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let banner(level) = box(stroke: 1pt, inset: 4pt)[#upper(level)]
#qm.region("classification")[#banner("secret")]
"#;
        let regions = probe_regions(PLATE, prose("A paragraph."));
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
    fn a_claim_yields_to_the_fields_nested_inside_it() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#qm.region("classification")[
  #line(length: 100pt)
  #qm.data.prose
]
"#;
        let regions = probe_regions(PLATE, prose("Body PROBETOKEN text."));
        assert!(
            regions
                .iter()
                .any(|r| r.field == "prose" && r.span.is_some()),
            "the nested content field keeps its own span-bearing region: {regions:?}"
        );
        assert!(
            regions.iter().any(|r| r.field == "classification"),
            "the wrapper still claims the rule it drew itself: {regions:?}"
        );
    }

    #[test]
    fn nested_scalar_ink_outranks_the_wrapper() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#qm.region("prose")[Level: #qm.ink(qm.data, "classification")]
"#;
        let regions = probe_regions(PLATE, prose("Unused."));
        assert!(
            regions.iter().any(|r| r.field == "classification"),
            "the scalar's ink keeps its own region inside the wrapper: {regions:?}"
        );
    }

    /// Two calls must not collapse into one shared first placement, or a
    /// per-card wrapper would surface a single card.
    #[test]
    fn each_region_call_claims_separately() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let stamp(n) = box(stroke: 1pt, inset: 4pt)[#n]
#qm.region("classification")[#stamp("one")]
Interleaved plate chrome.
#qm.region("classification")[#stamp("two")]
"#;
        let regions = probe_regions(PLATE, prose("Unused."));
        let claimed = regions
            .iter()
            .filter(|r| r.field == "classification")
            .count();
        assert_eq!(claimed, 2, "one region per call: {regions:?}");
    }

    /// An interruption must not truncate a claim: its extent is explicit, so
    /// there is no second-placement ambiguity to be conservative about.
    #[test]
    fn a_claim_accrues_across_an_interruption() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 400pt, height: 400pt, margin: 40pt)
#let chrome() = box(width: 40pt)[--]
#qm.region("classification")[LEFTMOST #chrome() #qm.data.prose RIGHTMOST]
"#;
        let regions = probe_regions(PLATE, prose("mid"));
        let claim = regions
            .iter()
            .find(|r| r.field == "classification")
            .expect("the claim surfaces");
        let body_box = regions
            .iter()
            .find(|r| r.field == "prose")
            .expect("the nested field surfaces");
        // Bottom-left origin: the trailing text sits below the nested field.
        assert!(
            claim.rect[1] < body_box.rect[1],
            "the claim reaches past the nested field's ink to its own trailing \
             text: claim {:?} vs prose {:?}",
            claim.rect,
            body_box.rect
        );
    }

    /// Typst never separates the two markers on its own, so the way to strand an
    /// open is a plate emitting the call's return value in parts.
    const STRANDED_OPEN: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
#let r = qm.region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#qm.data.prose

PLATE TEXT
"#;

    #[test]
    fn an_open_marker_whose_close_never_lands_is_reported_unclosed() {
        let p = probe(STRANDED_OPEN, prose("Body text."));
        assert_eq!(
            unclosed_claims(&p.doc),
            vec![(0, "classification".to_string())],
            "the stranded open is named, so a plate author can act on it"
        );
    }

    /// A claim left open would own every unattributed hit to the end of the
    /// document.
    #[test]
    fn an_unclosed_claim_surfaces_no_region_at_all() {
        let long = prose(&"A long paragraph of body text. ".repeat(60));
        let regions = probe_regions(STRANDED_OPEN, long.clone());
        assert!(
            regions.iter().any(|r| r.field == "prose"),
            "the fields that do resolve are untouched: {regions:?}"
        );
        assert!(
            !regions.iter().any(|r| r.field == "classification"),
            "the unbounded claim yields nothing rather than the plate's trailing ink: \
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
        let p = probe(
            STRANDED_OPEN,
            prose(&"A long paragraph of body text. ".repeat(60)),
        );
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
            p.scan(&p.unclosed())
                .field_at(last, cx, cy, 0.0)
                .map(|(_, f)| f),
            None,
            "suppressed, plate ink under a runaway claim answers to no field"
        );
    }

    /// Suppression is per claim, not per field name: a second, well-formed call
    /// on the same field keeps its region.
    #[test]
    fn a_balanced_claim_beside_an_unclosed_one_still_surfaces() {
        const PLATE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
#let r = qm.region("classification")[#box(stroke: 1pt)[X]]
#r.children.at(0)
#qm.region("classification")[#box(stroke: 1pt)[OK]]
"#;
        let regions = probe_regions(PLATE, prose("Body."));
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
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
#let inner = qm.region("prose")[#box(stroke: 1pt)[I]]
#qm.region("classification")[#inner.children.at(0) #box(stroke: 1pt)[C]]
#qm.data.prose
"#;
        let regions = probe_regions(PLATE, prose("Text."));
        assert!(
            regions.iter().any(|r| r.field == "prose"),
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
            transparent_hit(0, 0),                    // field 0's own structural ink
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
