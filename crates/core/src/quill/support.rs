//! What a plate declines to typeset, and the warning a body draws for
//! holding it anyway.
//!
//! The quill declares it per body ([`BodyCardSchema::unsupported`]) because
//! core cannot observe it: the plate's output is ink, and a construct that
//! drew none is indistinguishable from one the content never held. Nothing
//! checks the claim — an undeclared drop stays silent.

use std::collections::BTreeMap;

use quillmark_content::{Content, KnownIslandType, LineKind};

use crate::document::Document;
use crate::path::DocPath;
use crate::quill::types::BlockConstruct;
use crate::{Diagnostic, Quill, Severity};

use super::CardSchema;

/// The diagnostic code every declined construct rides.
pub const UNSUPPORTED_CONSTRUCT: &str = "plate::unsupported_construct";

impl Quill {
    /// One `plate::unsupported_construct` warning per (body, construct) this
    /// quill declares it does not typeset and `doc` holds anyway.
    ///
    /// Pre-render and schema-level, alongside `conform::*`: [`Quill::parse`]
    /// appends these to the `Parsed.warnings` it returns, and the walk is
    /// stateless, so a repeat call re-emits the identical set. A quill that
    /// declares nothing returns empty, which is every quill until one opts in.
    ///
    /// Non-fatal by construction. The content is representable, stores, and
    /// round-trips; it is the page that will not carry it, and that is the
    /// author's call to make once told.
    pub fn unsupported_constructs(&self, doc: &Document) -> Vec<Diagnostic> {
        let config = self.config();
        let mut diags = Vec::new();
        collect(
            &config.main,
            doc.main().body(),
            &DocPath::main_body(),
            &mut diags,
        );
        for (index, card) in doc.cards().iter().enumerate() {
            let kind = card.kind().unwrap_or("");
            // An unknown kind declares no body, so it declines nothing; the
            // render gate passes it and so does this.
            let Some(schema) = config.card_kind(kind) else {
                continue;
            };
            let path = DocPath::card(Some(kind), index).body();
            collect(schema, card.body(), &path, &mut diags);
        }
        diags
    }
}

/// Append one diagnostic per declared construct `body` actually holds, in the
/// declaration's own order so the set is stable across calls.
fn collect(schema: &CardSchema, body: &Content, path: &DocPath, out: &mut Vec<Diagnostic>) {
    let Some(declared) = schema.body.as_ref().map(|b| &b.unsupported) else {
        return;
    };
    // No `is_blank` short-circuit: blankness is about text, and a rule carries
    // none, so a body that is nothing but the construct being declined reads as
    // blank and would skip the walk that exists to catch it.
    if declared.is_empty() {
        return;
    }
    let present = census(body);
    for construct in declared {
        let Some(&count) = present.get(construct) else {
            continue;
        };
        let mut args = BTreeMap::new();
        args.insert("construct".to_string(), construct.as_str().into());
        args.insert("count".to_string(), count.into());
        out.push(
            Diagnostic::new(
                Severity::Warning,
                format!(
                    "this quill does not typeset {}: {count} in this body will not reach the page",
                    plural(*construct, count)
                ),
            )
            .with_code(UNSUPPORTED_CONSTRUCT.to_string())
            .with_path(path.to_string())
            .with_args(args),
        );
    }
}

/// English enough for the engine's own sentence; a consumer wording this
/// itself reads `construct` and `count` off `args` instead.
pub(crate) fn plural(construct: BlockConstruct, count: usize) -> String {
    let name = match construct {
        BlockConstruct::Heading => "heading",
        BlockConstruct::Rule => "horizontal rule",
        BlockConstruct::Code => "code block",
        BlockConstruct::List => "list",
        BlockConstruct::Quote => "block quote",
        BlockConstruct::Table => "table",
        BlockConstruct::Image => "image",
    };
    if count == 1 {
        format!("a {name}")
    } else {
        format!("{name}s")
    }
}

/// How many of each block construct `body` holds, which is the `count` arg a
/// `plate::unsupported_construct` diagnostic carries. Four counting rules:
///
/// - A container counts once per contiguous **run**: a three-item list is one
///   list, a multi-paragraph quote one quote. `quillmark_content::traverse`
///   decides where a run opens, so a sibling item continues its list while an
///   adjacent list of identical shape opens its own.
/// - Runs nest by *item*: a quote in each of two list items is two quotes.
/// - A leaf block counts per block, a code fence at its opening line.
/// - An island counts off `islands`, not off the line holding it, so an image
///   counts alone on its line and mid-sentence alike.
fn census(body: &Content) -> BTreeMap<BlockConstruct, usize> {
    use quillmark_content::model::Container;
    use quillmark_content::traverse::{items, runs};

    fn construct(container: &Container) -> Option<BlockConstruct> {
        match container {
            Container::ListItem { .. } => Some(BlockConstruct::List),
            Container::Quote { .. } => Some(BlockConstruct::Quote),
            // The open set: a container this build does not know names no
            // construct to decline.
            _ => None,
        }
    }

    /// A worklist, not recursion: nothing validates `body`'s nesting depth
    /// before this walk.
    fn count(
        lines: &[quillmark_content::model::Line],
        range: std::ops::Range<usize>,
        counts: &mut BTreeMap<BlockConstruct, usize>,
    ) {
        let mut work = vec![(range, 0)];
        while let Some((range, depth)) = work.pop() {
            for run in runs(lines, range, depth) {
                if let Some(construct) = construct(run.container) {
                    *counts.entry(construct).or_insert(0) += 1;
                }
                for item in items(lines, run.range, depth) {
                    work.push((item.range, depth + 1));
                }
            }
        }
    }

    let mut counts: BTreeMap<BlockConstruct, usize> = BTreeMap::new();
    count(&body.lines, 0..body.lines.len(), &mut counts);

    for island in &body.islands {
        match KnownIslandType::parse(&island.island_type) {
            Some(KnownIslandType::Table) => *counts.entry(BlockConstruct::Table).or_insert(0) += 1,
            Some(KnownIslandType::Image) => *counts.entry(BlockConstruct::Image).or_insert(0) += 1,
            // The open set: an island type this build does not know names no
            // construct to decline.
            None => {}
        }
    }

    for line in &body.lines {
        // Islands and containers are already counted; every other block kind
        // counts here.
        let construct = match &line.kind {
            LineKind::Heading { .. } => Some(BlockConstruct::Heading),
            LineKind::Rule => Some(BlockConstruct::Rule),
            LineKind::Code { .. } if !line.continues => Some(BlockConstruct::Code),
            _ => None,
        };
        if let Some(construct) = construct {
            *counts.entry(construct).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod census_tests {
    use super::*;

    fn count(md: &str, construct: BlockConstruct) -> usize {
        let body = quillmark_content::from_markdown(md).unwrap();
        census(&body).get(&construct).copied().unwrap_or(0)
    }

    #[test]
    fn a_run_counts_once_however_many_lines_or_items_it_spans() {
        assert_eq!(count("- a\n- b\n- c", BlockConstruct::List), 1);
        assert_eq!(count("> a\n>\n> b", BlockConstruct::Quote), 1);
    }

    #[test]
    fn two_adjacent_runs_of_one_shape_count_twice() {
        assert_eq!(count("- a\n\n+ b", BlockConstruct::List), 2);
        assert_eq!(count("> a\n\n<!-- -->\n\n> b", BlockConstruct::Quote), 2);
    }

    #[test]
    fn an_item_bounds_what_nests_inside_it() {
        assert_eq!(count("- > a\n\n- > b", BlockConstruct::Quote), 2);
        assert_eq!(count("- > a\n\n- > b", BlockConstruct::List), 1);
        assert_eq!(count("- - a\n\n- - b", BlockConstruct::List), 3);
    }

    /// `overwrite_body` never validates, so the walk sees whatever depth a
    /// client built.
    #[test]
    fn unvalidated_depth_does_not_overflow_the_stack() {
        use quillmark_content::model::{Container, Content, Line, LineKind};
        let mut line = Line::new(LineKind::Para);
        line.containers = vec![Container::Quote { instance: 0 }; 200_000];
        let body = Content::new("x".to_string(), vec![line]);
        assert_eq!(census(&body).get(&BlockConstruct::Quote), Some(&200_000));
    }

    #[test]
    fn a_run_bounds_what_nests_inside_it() {
        assert_eq!(count("> - a\n\n<!-- -->\n\n> - b", BlockConstruct::List), 2);
    }
}
