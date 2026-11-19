//! The export compile's probes: show and set rules in the library's base
//! styles that record, as labelled metadata, what Typst lays out without
//! tagging it. Each rule returns its element unchanged, so the probed
//! document's ink is the live one's, which [`export`](super::export) checks.

use typst::Library;
use typst::comemo::Track;
use typst::engine::Sink;
use typst::foundations::{
    Content, Context, Dict, Element, Func, NativeElement, Recipe, Regex, Scope, Selector,
    Transformation,
};
use typst::introspection::EmptyIntrospector;
use typst::layout::{AlignElem, BlockElem, BoxElem, ColumnsElem, PageElem, PagebreakElem};
use typst::model::ParElem;
use typst::routines::SpanMode;
use typst::syntax::{Span, SyntaxMode};
use typst::text::LinebreakElem;
use typst::utils::LazyHash;

/// One rule per element key, `newline` for a line break a string spells as
/// `\n`, and `page`, the foreground every page carries. Values pass typed,
/// never as `repr` strings.
const PRELUDE: &str = r#"{
  let h(..d) = [#metadata((kind: "__qm_hint__", ..d.named()))<__qm_hint__>]
  (
    pagebreak: it => { it; h(hint: "break", weak: it.weak, to: it.to) },
    linebreak: it => { h(hint: "br"); it },
    newline: it => { h(hint: "br"); it },
    align: it => { h(hint: "align", edge: "open", a: it.alignment); it; h(hint: "align", edge: "close") },
    columns: it => { h(hint: "columns", edge: "open", count: it.count, gutter: it.gutter); it; h(hint: "columns", edge: "close") },
    block: it => {
      context h(hint: "block", edge: "open", a: align.alignment, size: text.size,
        sticky: it.at("sticky", default: false), breakable: it.at("breakable", default: true),
        above: it.at("above", default: auto), below: it.at("below", default: auto),
        leading: par.leading, spacing: par.spacing)
      it
      h(hint: "block", edge: "close")
    },
    box: it => { h(hint: "box", edge: "open"); it; h(hint: "box", edge: "close") },
    par: it => {
      context h(hint: "par", a: align.alignment, size: text.size, tracking: text.tracking,
        leading: par.leading, spacing: par.spacing, top: text.top-edge, bottom: text.bottom-edge)
      it
    },
    page: context {
      h(hint: "page", margin: page.margin, columns: page.columns, w: page.width, h: page.height,
        flipped: page.flipped, n: counter(page).get().first(), size: text.size)
      {
        show text: it => h(hint: "pnum", s: it.text)
        counter(page).display()
      }
    },
  )
}"#;

fn rules() -> [(&'static str, Element); 7] {
    [
        ("pagebreak", PagebreakElem::ELEM),
        ("linebreak", LinebreakElem::ELEM),
        ("align", AlignElem::ELEM),
        ("columns", ColumnsElem::ELEM),
        ("block", BlockElem::ELEM),
        ("box", BoxElem::ELEM),
        ("par", ParElem::ELEM),
    ]
}

/// `library` with the probes as its outermost styles, or `None` when the
/// prelude fails to evaluate.
pub(crate) fn with_probes(
    world: &dyn typst::World,
    library: &LazyHash<Library>,
) -> Option<Library> {
    let mut sink = Sink::new();
    let dict = (library.routines.eval_string)(
        world.track(),
        library,
        sink.track_mut(),
        EmptyIntrospector.track(),
        Context::none().track(),
        PRELUDE,
        SpanMode::Uniform(Span::detached()),
        SyntaxMode::Code,
        Scope::new(),
    )
    .ok()?
    .cast::<Dict>()
    .ok()?;
    let mut out = (**library).clone();
    for (key, elem) in rules() {
        let func = dict.get(key).ok()?.clone().cast::<Func>().ok()?;
        out.styles.apply_one(
            Recipe::new(
                Some(Selector::Elem(elem, None)),
                Transformation::Func(func),
                Span::detached(),
            )
            .into(),
        );
    }
    let newline = dict.get("newline").ok()?.clone().cast::<Func>().ok()?;
    out.styles.apply_one(
        Recipe::new(
            Some(Selector::Regex(Regex::new("\n").ok()?)),
            Transformation::Func(newline),
            Span::detached(),
        )
        .into(),
    );
    let page = dict.get("page").ok()?.clone().cast::<Content>().ok()?;
    out.styles
        .apply_one(typst::foundations::Property::new(PageElem::foreground, Some(page)).into());
    Some(out)
}
