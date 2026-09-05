//! Schema-field geometry, queried from a compiled
//! [`LiveSession`](crate::LiveSession) via
//! [`regions`](crate::LiveSession::regions) and
//! [`field_at`](crate::LiveSession::field_at).
//!
//! A region ties a rectangle on the rendered page to the quill schema field
//! that produced it. `regions` answers *field → rectangle*, `field_at` answers
//! *point → field*.
//!
//! Three producers feed regions, all keyed on the schema path:
//!
//! - **Content fields** are tracked by the spans their glyphs carry, so the
//!   origin survives a package that rebuilds the content. A field that draws
//!   nothing surfaces no region: present-but-empty is not placed.
//! - **Direct scalar references** — each `data.<field>` expression in the
//!   plate is its own site, so a scalar in both header and footer surfaces
//!   twice. `#upper(data.subject)` attributes the whole expression as long as
//!   it holds one reference, and a read stepping into a declared container
//!   (`data.classification.poc`) attributes the property rather than the
//!   container. A read through a `let` alias bound exactly once to one whole
//!   chain tracks where that chain would. Not tracked: an expression mixing
//!   several fields, a value laundered past the alias (a function parameter, a
//!   destructured binding), and card scalars read from the per-card loop
//!   variable (one site shared by every instance — bind a widget for those).
//! - **Form-field widgets** carry a schema path explicitly. A widget that
//!   binds none produces no region: only schema-addressable fields surface.
//!
//! **First placement only.** A content value placed twice surfaces one region
//! set, because span data cannot distinguish "package chrome interrupting one
//! placement" from "a second placement", and a union would claim the ink
//! between them. That placement is one region per page it touches, in page
//! order: page marginals between one page's body and the next's do not end it,
//! but foreign ink within a page shrinks the region to the placement's true
//! start. Later placements stay reachable point-wise through
//! [`field_at`](crate::LiveSession::field_at), since a concrete point
//! identifies one drawn item.
//!
//! Regions are an overlay sidecar, never a compositing input: every canvas
//! backend hands back a complete page raster. A one-shot byte render carries
//! the same sidecar on request ([`RenderOptions::regions`](crate::RenderOptions)).
//! Empty for backends that place no schema fields.

/// One schema field placement's extent on a rendered page.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF points with a **bottom-left** origin: the
/// lower-left corner of the page as drawn, which is also the origin of the page
/// raster and of [`page_size_pt`](crate::LiveSession::page_size_pt). On a PDF
/// background that corner is the page's canvas box (`/CropBox` ∩ `/MediaBox`)
/// rather than user-space `(0, 0)`, so a widget's `/Rect` in the stamped PDF and
/// its region here differ by that box's origin.
///
/// `field` is **not** unique within a region set: a content field breaks into
/// one entry per segment and per page, a scalar referenced at several plate
/// sites yields one per site, and tracked content plus a bound widget yields
/// both. Consumers group by `field`, or take the derived per-page union from
/// [`field_boxes`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct RenderedRegion {
    /// The field's plate-space schema address as the backend keys it:
    /// `"signature_block"`, `"references.0"` (a numeric segment per array
    /// index), `"classification.poc"` (a container property) or
    /// `"$cards.<kind>.<ordinal>.<tail>"` (a per-kind ordinal).
    /// [`plate_addr_to_doc_path`] translates it to a canonical [`DocPath`];
    /// a core consumer reading `RenderedRegion` directly sees this form.
    pub field: String,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]`, PDF points, bottom-left origin.
    pub rect: [f32; 4],
    /// The content slice this box covers: USV `[start, end)` into the field's
    /// `Content`, `None` for a scalar reference site or a widget. Omitted from
    /// the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<[usize; 2]>,
}

impl RenderedRegion {
    /// A geometry entry with no content address. Content ink adds its slice
    /// with [`with_span`](Self::with_span).
    pub fn new(field: String, page: usize, rect: [f32; 4]) -> Self {
        Self {
            field,
            page,
            rect,
            span: None,
        }
    }

    /// Set [`span`](Self::span), the USV `[start, end)` this box covers.
    pub fn with_span(mut self, span: [usize; 2]) -> Self {
        self.span = Some(span);
        self
    }

    /// Whether the point (`x`, `y`, PDF points, bottom-left origin) on `page`
    /// falls inside this region, edges inclusive. Every `field_at` hit-test
    /// shares this predicate.
    pub fn contains(&self, page: usize, x: f32, y: f32) -> bool {
        self.distance(page, x, y) == Some(0.0)
    }

    /// Gap in PDF points from the point to this region's rect, zero inside it.
    /// `None` on another page, or when either side is not finite. What a
    /// tolerant hit-test ranks by, so a tolerance of zero admits exactly what
    /// [`contains`](Self::contains) does, and an absent gap is one no tolerance
    /// reaches.
    ///
    /// The finite check is load-bearing: `f32::max` returns the non-NaN side, so
    /// a NaN on either side otherwise collapses both axes to zero and reads as
    /// inside.
    pub fn distance(&self, page: usize, x: f32, y: f32) -> Option<f32> {
        let finite = x.is_finite() && y.is_finite() && self.rect.iter().all(|v| v.is_finite());
        (self.page == page && finite).then(|| {
            let dx = (self.rect[0] - x).max(0.0).max(x - self.rect[2]);
            let dy = (self.rect[1] - y).max(0.0).max(y - self.rect[3]);
            dx.hypot(dy)
        })
    }
}

/// The whole-field highlight boxes for `field`, derived from a region set: one
/// union rect per page, over that field's **`span`-bearing** (content) regions.
///
/// Pass the output of [`LiveSession::regions`](crate::LiveSession::regions) or
/// a one-shot [`RenderOptions::regions`](crate::RenderOptions) sidecar;
/// [`LiveSession::field_boxes`](crate::LiveSession::field_boxes) reads the
/// session's own. The union is a per-page *bounding* box, so it does cover
/// inter-paragraph whitespace the input regions leave out.
///
/// **Content only.** A scalar-reference site or a widget carries no `span`, so
/// a field placed only that way yields an empty result here: its highlight box
/// is a single region's `rect`, read straight from the set. Each returned
/// region carries the union `span` and the result is `page`-ascending.
pub fn field_boxes(regions: &[RenderedRegion], field: &str) -> Vec<RenderedRegion> {
    let mut by_page: Vec<RenderedRegion> = Vec::new();
    for r in regions
        .iter()
        .filter(|r| r.field == field && r.span.is_some())
    {
        let span = r.span.expect("filtered to span-bearing");
        match by_page.iter_mut().find(|acc| acc.page == r.page) {
            Some(acc) => {
                acc.rect[0] = acc.rect[0].min(r.rect[0]);
                acc.rect[1] = acc.rect[1].min(r.rect[1]);
                acc.rect[2] = acc.rect[2].max(r.rect[2]);
                acc.rect[3] = acc.rect[3].max(r.rect[3]);
                let s = acc.span.expect("union region carries a span");
                acc.span = Some([s[0].min(span[0]), s[1].max(span[1])]);
            }
            None => by_page.push(RenderedRegion {
                field: r.field.clone(),
                page: r.page,
                rect: r.rect,
                span: Some(span),
            }),
        }
    }
    by_page.sort_by_key(|r| r.page);
    by_page
}

// Address translation: plate-space geometry ⇄ DocPath.
//
// A backend keys a region on the plate-space address its compiled plate
// composes: a `$cards` sigil, dot separators, and per-kind ordinals. That
// grammar is the template-author contract inside the plate and must not cross
// to a consumer, so the session resolves the per-kind ordinal to the
// document-array absolute index (and back) against the current compile's
// ordered card kinds.
//
// The tail is parsed and rendered segment-wise, which is what keeps the minted
// `DocPath` stable across `Display` → `FromStr`: a tail carried whole into one
// `Field` renders `main.references.0`, which reparses as a field named `"0"`.
//
// `.N` reads as an index here and not in `DocPath::from_str`, because plate
// addresses are schema-derived and carry no digit map key, while a nested YAML
// map key is unconstrained (`collect_fill_diags` mints `main.m.0` as
// `Field{"0"}`). Teaching the parser `.N` would cost that reading.

use crate::path::{DocPath, DocSeg};

/// The absolute document-array index of the `ord`-th (0-based) card of `kind`.
/// `card_kinds` is the current compile's ordered card kinds, `None` per
/// kindless card.
fn abs_card_index(card_kinds: &[Option<&str>], kind: &str, ord: usize) -> Option<usize> {
    card_kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| **k == Some(kind))
        .nth(ord)
        .map(|(i, _)| i)
}

/// How many cards of the same kind precede absolute index `abs`, matching the
/// plate's `emit_cards` counter.
fn per_kind_ordinal(card_kinds: &[Option<&str>], abs: usize) -> Option<usize> {
    let kind = (*card_kinds.get(abs)?)?;
    Some(
        card_kinds[..abs]
            .iter()
            .filter(|k| **k == Some(kind))
            .count(),
    )
}

/// Rewrite each region's plate-space `field` to its [`DocPath`] string. The one
/// funnel every binding hands regions through, so a consumer never sees the two
/// address spaces mixed. An address outside the geometry grammar keeps its
/// original string.
pub fn regions_to_doc_path(
    mut regions: Vec<RenderedRegion>,
    card_kinds: &[Option<&str>],
) -> Vec<RenderedRegion> {
    for region in &mut regions {
        if let Some(path) = plate_addr_to_doc_path(&region.field, card_kinds) {
            region.field = path.to_string();
        }
    }
    regions
}

/// Extend `base` by a plate-space tail's segments. `None` for an empty piece, a
/// leading index, a non-final `$body`, or any other `$`-token.
fn plate_tail_to_segs(base: DocPath, tail: &str) -> Option<DocPath> {
    let mut path = base;
    let mut it = tail.split('.').enumerate().peekable();
    while let Some((i, piece)) = it.next() {
        let last = it.peek().is_none();
        if piece.is_empty() {
            return None;
        }
        path = if piece.bytes().all(|b| b.is_ascii_digit()) {
            // Neither `main` nor a card is an array, so a tail never opens on
            // an index.
            if i == 0 {
                return None;
            }
            path.index(piece.parse().ok()?)
        } else if piece == "$body" {
            if !last {
                return None;
            }
            path.body()
        } else if piece.starts_with('$') {
            return None;
        } else {
            path.field(piece)
        };
    }
    Some(path)
}

/// Translate a backend plate-space geometry address into a canonical
/// [`DocPath`], resolving the per-kind ordinal to the absolute card index via
/// `card_kinds`. The grammar is what geometry emits: `$body`, `<tail>` (main),
/// and `$cards.<kind>.<ord>.<tail>`. `None` for an address outside it, or one
/// naming a card the kind list cannot place.
pub fn plate_addr_to_doc_path(addr: &str, card_kinds: &[Option<&str>]) -> Option<DocPath> {
    if addr == "$body" {
        return Some(DocPath::main_body());
    }
    if let Some(rest) = addr.strip_prefix("$cards.") {
        let mut it = rest.splitn(3, '.');
        let kind = it.next()?;
        let ord: usize = it.next()?.parse().ok()?;
        let tail = it.next()?;
        let abs = abs_card_index(card_kinds, kind, ord)?;
        return plate_tail_to_segs(DocPath::card(Some(kind), abs), tail);
    }
    // A bare plate-space main field (`subject`) roots at `main` in `DocPath`
    // space; an unrecognized `$`-token is never a main field.
    if addr.starts_with('$') {
        return None;
    }
    plate_tail_to_segs(DocPath::main(), addr)
}

/// Render a [`DocPath`] tail in plate space, joining segments with `.`. `None`
/// for a tail plate space cannot spell: a leading index, a non-final body, a
/// card mid-path, or a field name that is empty, all ASCII digits, `$`-leading,
/// or carries `.` / `[` / `]` — each would reparse as a different segment.
fn plate_tail(tail: &[DocSeg]) -> Option<String> {
    let mut out = String::new();
    for (i, seg) in tail.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        match seg {
            DocSeg::Index { index } => {
                if i == 0 {
                    return None;
                }
                out.push_str(&index.to_string());
            }
            DocSeg::Body => {
                if i + 1 != tail.len() {
                    return None;
                }
                out.push_str("$body");
            }
            DocSeg::Field { name } => {
                if name.is_empty()
                    || name.bytes().all(|b| b.is_ascii_digit())
                    || name.starts_with('$')
                    || name.contains(['.', '[', ']'])
                {
                    return None;
                }
                out.push_str(name);
            }
            DocSeg::Main | DocSeg::Card { .. } => return None,
        }
    }
    Some(out)
}

/// Translate a canonical [`DocPath`] geometry address back to the backend
/// plate-space form (`main.body` → `$body`, `main.references[0]` →
/// `references.0`, `cards.<kind>[<abs>].<tail>` → `$cards.<kind>.<ord>.<tail>`),
/// resolving the absolute card index to its per-kind ordinal via `card_kinds`.
/// The inverse of [`plate_addr_to_doc_path`], for the `field`-taking queries.
/// `None` when the path is not a geometry address, names a card the kind list
/// cannot place, or is one plate space cannot spell.
pub fn doc_path_to_plate_addr(path: &DocPath, card_kinds: &[Option<&str>]) -> Option<String> {
    match path.segs() {
        [DocSeg::Main, DocSeg::Body] => Some("$body".to_string()),
        [DocSeg::Main, tail @ ..] if !tail.is_empty() => plate_tail(tail),
        [DocSeg::Card {
            kind: Some(kind),
            index,
        }, tail @ ..]
            if !tail.is_empty() =>
        {
            // The path must actually name the card that sits at `index`.
            if card_kinds.get(*index).copied().flatten() != Some(kind.as_str()) {
                return None;
            }
            let ord = per_kind_ordinal(card_kinds, *index)?;
            Some(format!("$cards.{kind}.{ord}.{}", plate_tail(tail)?))
        }
        _ => None,
    }
}

/// How precisely a [`ContentHit::pos`] resolved. Never sub-cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum HitGranularity {
    /// `pos` is the first content char of the grapheme cluster under the point.
    /// A caret UI can place the caret there directly.
    Cluster,
    /// The point landed on origin-less ink (list markers, numbering, a code
    /// fence's interior), so `pos` degraded to the containing segment's content
    /// start. A caret UI should read it as a segment selection.
    Segment,
}

/// A resolved point → content position: the schema field a click landed in and
/// the USV offset into that field's `Content`. The forward
/// [`position_at`](crate::LiveSession::position_at) direction, paired with
/// [`locate`](crate::LiveSession::locate) (content position → caret rect).
///
/// `pos` is **cluster-exact, not sub-character**: a hit inside a char that
/// escaped to several generated bytes (`*`→`\*`, `你`→3) floors to that
/// cluster's first content char. A click on origin-less ink degrades to the
/// containing segment's content start, and a click off all content ink resolves
/// to nothing. [`granularity`](Self::granularity) reports which happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ContentHit {
    /// The content field's schema path (same address space as
    /// [`RenderedRegion::field`]).
    pub field: String,
    /// USV offset into the field's `Content`.
    pub pos: usize,
    /// `None` when the backend does not report it (no source map, or an older
    /// wire payload). Omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<HitGranularity>,
}

impl ContentHit {
    /// A hit whose precision the backend does not report. A backend with a
    /// source map adds it with [`with_granularity`](Self::with_granularity).
    pub fn new(field: String, pos: usize) -> Self {
        Self {
            field,
            pos,
            granularity: None,
        }
    }

    pub fn with_granularity(mut self, granularity: HitGranularity) -> Self {
        self.granularity = Some(granularity);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_finite_point_is_outside_every_region() {
        let region = RenderedRegion::new("subject".to_string(), 0, [10.0, 10.0, 20.0, 20.0]);
        for (x, y) in [
            (f32::NAN, f32::NAN),
            (f32::NAN, 15.0),
            (15.0, f32::NAN),
            (f32::INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, 15.0),
        ] {
            assert!(!region.contains(0, x, y), "({x}, {y}) reads as inside");
            assert_ne!(region.distance(0, x, y), Some(0.0), "({x}, {y}) reads as zero gap");
        }
        assert!(region.contains(0, 15.0, 15.0));
        assert_eq!(region.distance(0, 15.0, 15.0), Some(0.0));

        // The rect is the other side of the same check: one non-finite box in a
        // `regions()` set answers every click on its page.
        let bad = RenderedRegion::new("subject".to_string(), 0, [f32::NAN; 4]);
        assert!(!bad.contains(0, 999.0, 999.0));
        assert_eq!(bad.distance(0, 999.0, 999.0), None);
    }

    #[test]
    fn optional_span_omitted_when_none() {
        let region = RenderedRegion {
            field: "subject".to_string(),
            page: 0,
            rect: [1.0, 2.0, 3.0, 4.0],
            span: None,
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(!json.contains("span"), "scalar region omits span: {json}");
        let back: RenderedRegion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
    }

    #[test]
    fn content_hit_round_trips_through_json() {
        let hit = ContentHit {
            field: "body".to_string(),
            pos: 42,
            granularity: Some(HitGranularity::Cluster),
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert!(json.contains("\"field\":\"body\"") && json.contains("\"pos\":42"));
        assert!(json.contains("\"granularity\":\"cluster\""), "{json}");
        let back: ContentHit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, hit);

        let seg = ContentHit {
            field: "body".to_string(),
            pos: 7,
            granularity: Some(HitGranularity::Segment),
        };
        let json = serde_json::to_string(&seg).unwrap();
        assert!(json.contains("\"granularity\":\"segment\""), "{json}");
        assert_eq!(serde_json::from_str::<ContentHit>(&json).unwrap(), seg);
    }

    fn content(field: &str, page: usize, rect: [f32; 4], span: [usize; 2]) -> RenderedRegion {
        RenderedRegion {
            field: field.to_string(),
            page,
            rect,
            span: Some(span),
        }
    }

    #[test]
    fn field_boxes_unions_span_bearing_segments_per_page() {
        let regions = vec![
            content("$body", 0, [10.0, 700.0, 200.0, 720.0], [0, 12]),
            content("$body", 0, [10.0, 660.0, 260.0, 680.0], [13, 40]),
            content("$body", 1, [10.0, 700.0, 150.0, 720.0], [41, 55]),
            content("subject", 0, [10.0, 740.0, 90.0, 752.0], [0, 5]),
        ];
        let boxes = field_boxes(&regions, "$body");
        assert_eq!(boxes.len(), 2, "one box per page $body touches");
        assert_eq!(boxes[0].page, 0);
        assert_eq!(boxes[0].rect, [10.0, 660.0, 260.0, 720.0], "page-0 union");
        assert_eq!(boxes[0].span, Some([0, 40]), "page-0 union span");
        assert_eq!(boxes[1].page, 1);
        assert_eq!(boxes[1].rect, [10.0, 700.0, 150.0, 720.0]);
    }

    #[test]
    fn field_boxes_empty_for_span_less_field() {
        let regions = vec![RenderedRegion {
            field: "subject".to_string(),
            page: 0,
            rect: [10.0, 740.0, 90.0, 752.0],
            span: None,
        }];
        assert!(field_boxes(&regions, "subject").is_empty());
    }

    /// Interleaved kinds, so the per-kind ordinal is not the absolute index.
    const KINDS: &[Option<&str>] = &[Some("note"), Some("annotation"), Some("note")];

    fn to_doc(addr: &str) -> Option<String> {
        plate_addr_to_doc_path(addr, KINDS).map(|p| p.to_string())
    }
    fn to_plate(path: &str) -> Option<String> {
        doc_path_to_plate_addr(&path.parse().unwrap(), KINDS)
    }

    #[test]
    fn plate_to_docpath_resolves_the_absolute_index() {
        assert_eq!(to_doc("$cards.note.1.on").as_deref(), Some("cards.note[2].on"));
        assert_eq!(to_doc("$cards.note.0.on").as_deref(), Some("cards.note[0].on"));
        assert_eq!(
            to_doc("$cards.annotation.0.text").as_deref(),
            Some("cards.annotation[1].text")
        );
        assert_eq!(to_doc("$body").as_deref(), Some("main.body"));
        assert_eq!(
            to_doc("$cards.note.1.$body").as_deref(),
            Some("cards.note[2].body")
        );
        assert_eq!(to_doc("signature_block").as_deref(), Some("main.signature_block"));
    }

    #[test]
    fn plate_to_docpath_parses_the_tail_segment_wise() {
        assert_eq!(to_doc("references.0").as_deref(), Some("main.references[0]"));
        assert_eq!(to_doc("address.city").as_deref(), Some("main.address.city"));
        assert_eq!(
            to_doc("$cards.note.1.refs.0").as_deref(),
            Some("cards.note[2].refs[0]")
        );
        assert_eq!(
            to_doc("$cards.note.0.addr.city").as_deref(),
            Some("cards.note[0].addr.city")
        );
    }

    /// Covers the string leg too: a binding hands `docpath_to_plate` the
    /// *rendered* path, so a `DocPath`-only round-trip would not catch a
    /// spelling that reparses to different segments.
    #[test]
    fn docpath_to_plate_is_the_inverse() {
        for plate in [
            "$body",
            "signature_block",
            "references.0",
            "address.city",
            "a.b.c.0.d",
            "$cards.note.0.on",
            "$cards.note.1.on",
            "$cards.annotation.0.text",
            "$cards.note.1.$body",
            "$cards.note.1.refs.0",
            "$cards.note.0.addr.city",
        ] {
            let doc = plate_addr_to_doc_path(plate, KINDS).unwrap();
            let rendered = doc.to_string();
            assert_eq!(rendered.parse::<DocPath>().ok(), Some(doc), "string leg {plate}");
            assert_eq!(to_plate(&rendered).as_deref(), Some(plate), "round-trip {plate}");
        }
    }

    #[test]
    fn translation_rejects_unplaceable_and_foreign_shapes() {
        // Only two `note`s exist.
        assert_eq!(to_doc("$cards.note.2.on"), None);
        // The kind disagrees with the slot.
        assert_eq!(
            doc_path_to_plate_addr(&"cards.annotation[0].x".parse().unwrap(), KINDS),
            None
        );
        // A document-model shape geometry never keys.
        assert_eq!(
            doc_path_to_plate_addr(&"recipients[0].name".parse().unwrap(), KINDS),
            None
        );
        assert_eq!(to_doc("$cards.note.1"), None);
        assert_eq!(to_doc("references."), None);
        assert_eq!(to_doc("refs.$seed"), None);
        assert_eq!(to_doc("a.$body.b"), None);
        assert_eq!(to_doc("0.x"), None);
    }

    #[test]
    fn docpath_to_plate_rejects_unspellable_paths() {
        for doc in [
            // A digit map key is a `Field`; spelling it `m.0` reads back as an
            // index.
            "main.m.0",
            "main[0].x",
            "cards[0].x",
            "main",
            "cards.note[0]",
        ] {
            assert_eq!(
                doc_path_to_plate_addr(&doc.parse().unwrap(), KINDS),
                None,
                "unspellable {doc}"
            );
        }
        // A non-final `Body`, unreachable through the parser, so built
        // segment-wise.
        assert_eq!(
            doc_path_to_plate_addr(&DocPath::main().body().field("x"), KINDS),
            None
        );
    }
}
