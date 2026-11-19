//! What every walk of a laid-out document's frames agrees on: which ink is
//! out of the flow around it, and which tags are Quillmark's own markers.
//! The span scan and the Word reader both read frames through these.

use typst::foundations::{Content, Dict, Label, Value};
use typst::introspection::{Location, Tag};
use typst::layout::PlaceElem;
use typst::model::FootnoteElem;
use typst::pdf::ArtifactElem;
use typst::utils::PicoStr;
use typst_layout::PagedIntrospector;

/// A footnote entry or a float: content Typst lays out away from its anchor,
/// in a group whose logical parent is the anchoring element. A table cell
/// broken across pages is parented to the cell, and stays in flow.
pub(crate) fn relocated(flow: &PagedIntrospector, parent: Location) -> bool {
    flow.elements()
        .get_by_loc(&parent)
        .is_some_and(|e| e.is::<FootnoteElem>() || e.is::<PlaceElem>())
}

/// The location of an `ArtifactElem` start tag, or of any end tag: an end tag
/// names its element only by location, so the caller matches it against the
/// artifacts it opened.
pub(crate) fn artifact_edge(tag: &Tag) -> Option<(Location, bool)> {
    match tag {
        Tag::Start(elem, _) if elem.is::<ArtifactElem>() => Some((elem.location()?, false)),
        Tag::End(loc, ..) => Some((*loc, true)),
        _ => None,
    }
}

/// The value of `elem` when it is metadata labelled `kind` whose dict names
/// the same `kind`. A user attaching a reserved label to their own metadata
/// is ignored.
pub(crate) fn marker(elem: &Content, kind: &str) -> Option<Dict> {
    if elem.label()? != Label::new(PicoStr::intern(kind))? {
        return None;
    }
    let Ok(Value::Dict(d)) = elem.get_by_name("value") else {
        return None;
    };
    matches!(d.get("kind"), Ok(Value::Str(k)) if k.as_str() == kind).then_some(d)
}
