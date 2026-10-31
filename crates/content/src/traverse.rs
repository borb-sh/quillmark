//! The loops that group adjacent lines: by container, applying the rule
//! [`Container::same_run`] states, and by continuation.

use crate::model::{Container, Line};
use std::ops::Range;

/// A maximal span of adjacent lines sharing one container at a depth.
#[derive(Debug, Clone, PartialEq)]
pub struct Span<'a> {
    pub container: &'a Container,
    pub range: Range<usize>,
}

/// The container runs at `depth` within `range`: adjacent lines whose container
/// at `depth` is one instance — same [`Container::same_run`] shape *and* same
/// [`Container::instance`].
///
/// A line carrying no container at `depth` belongs to no run, so it both ends
/// the run before it and is skipped rather than yielded.
///
/// Container identity is path plus contiguity, so `range` must be one parent's
/// span: over a range spanning two parents, two like runs under them read as
/// one. An [`items`] span is the parent bound to descend through.
pub fn runs(lines: &[Line], range: Range<usize>, depth: usize) -> Spans<'_> {
    Spans {
        lines,
        end: range.end.min(lines.len()),
        at: range.start,
        depth,
        same: |a, b| a.same_run(b) && a.instance() == b.instance(),
    }
}

/// The items at `depth` within `range`: adjacent lines whose whole container at
/// `depth` is equal, so a list's items separate while an item spanning several
/// paragraphs stays whole. Skips a line carrying no container at `depth`, as
/// [`runs`] does.
///
/// An item is a parent: what nests inside one is bounded by its span, never by
/// its run's.
pub fn items(lines: &[Line], range: Range<usize>, depth: usize) -> Spans<'_> {
    Spans {
        lines,
        end: range.end.min(lines.len()),
        at: range.start,
        depth,
        same: |a, b| a == b,
    }
}

/// The segment opening at `range.start`: that line plus every following one
/// that continues it — a paragraph's hard-break run, or a code fence's lines —
/// bounded by `range`.
///
/// A continuation counts only at the same nesting: a `continues` line whose
/// container path differs ends the segment rather than joining across the
/// boundary.
pub fn segment(lines: &[Line], range: Range<usize>, depth: usize) -> Range<usize> {
    let end = range.end.min(lines.len());
    let mut j = (range.start + 1).min(end);
    while j < end && lines[j].containers.len() == depth && lines[j].continues {
        j += 1;
    }
    range.start..j
}

/// Iterator over [`runs`] or [`items`].
pub struct Spans<'a> {
    lines: &'a [Line],
    end: usize,
    at: usize,
    depth: usize,
    same: fn(&Container, &Container) -> bool,
}

impl<'a> Iterator for Spans<'a> {
    type Item = Span<'a>;

    fn next(&mut self) -> Option<Span<'a>> {
        while self.at < self.end && self.lines[self.at].containers.len() <= self.depth {
            self.at += 1;
        }
        if self.at >= self.end {
            return None;
        }
        let start = self.at;
        let container = &self.lines[start].containers[self.depth];
        let mut j = start + 1;
        while j < self.end
            && self.lines[j]
                .containers
                .get(self.depth)
                .is_some_and(|c| (self.same)(c, container))
        {
            j += 1;
        }
        self.at = j;
        Some(Span {
            container,
            range: start..j,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Content, LineKind};

    fn li(ordinal: u64, instance: u64) -> Container {
        Container::ListItem {
            ordered: false,
            start: 1,
            ordinal,
            checked: None,
            instance,
        }
    }

    fn content(paths: &[Vec<Container>]) -> Content {
        let text = vec!["x"; paths.len()].join("\n");
        let lines = paths
            .iter()
            .map(|p| {
                let mut l = Line::new(LineKind::Para);
                l.containers = p.clone();
                l
            })
            .collect();
        Content::new(text, lines)
    }

    fn spans(it: Spans<'_>) -> Vec<Range<usize>> {
        it.map(|s| s.range).collect()
    }

    #[test]
    fn a_run_spans_its_items_and_an_item_spans_its_paragraphs() {
        let rt = content(&[vec![li(0, 0)], vec![li(0, 0)], vec![li(1, 0)]]);
        assert_eq!(spans(runs(&rt.lines, 0..3, 0)), vec![0..3]);
        assert_eq!(spans(items(&rt.lines, 0..3, 0)), vec![0..2, 2..3]);
    }

    #[test]
    fn instance_ends_a_run_that_shape_alone_would_weld() {
        let rt = content(&[vec![li(0, 0)], vec![li(0, 1)]]);
        assert_eq!(spans(runs(&rt.lines, 0..2, 0)), vec![0..1, 1..2]);
    }

    #[test]
    fn a_line_without_a_container_at_depth_ends_a_run_and_is_skipped() {
        let rt = content(&[vec![li(0, 0)], vec![], vec![li(0, 0)]]);
        assert_eq!(spans(runs(&rt.lines, 0..3, 0)), vec![0..1, 2..3]);
    }

    #[test]
    fn a_range_bounds_the_parent_two_like_runs_nest_under() {
        let rt = content(&[
            vec![li(0, 0), Container::Quote { instance: 0 }],
            vec![li(1, 0), Container::Quote { instance: 0 }],
        ]);
        assert_eq!(spans(runs(&rt.lines, 0..2, 1)), vec![0..2]);
        let per_item: Vec<_> = items(&rt.lines, 0..2, 0)
            .flat_map(|item| spans(runs(&rt.lines, item.range, 1)))
            .collect();
        assert_eq!(per_item, vec![0..1, 1..2]);
    }

    fn seg(paths: &[Vec<Container>], continues: &[bool], depth: usize) -> Range<usize> {
        let mut rt = content(paths);
        for (line, &c) in rt.lines.iter_mut().zip(continues) {
            line.continues = c;
        }
        segment(&rt.lines, 0..rt.lines.len(), depth)
    }

    #[test]
    fn a_segment_takes_the_continuations_at_its_own_depth() {
        assert_eq!(seg(&[vec![], vec![], vec![]], &[false, true, true], 0), 0..3);
        assert_eq!(seg(&[vec![], vec![], vec![]], &[false, true, false], 0), 0..2);
    }

    #[test]
    fn a_continuation_at_another_depth_ends_the_segment() {
        let paths = &[vec![], vec![li(0, 0)]];
        assert_eq!(seg(paths, &[false, true], 0), 0..1);
    }

    #[test]
    fn a_range_end_past_the_last_line_is_clamped() {
        let rt = content(&[vec![li(0, 0)]]);
        assert_eq!(spans(runs(&rt.lines, 0..99, 0)), vec![0..1]);
        assert_eq!(segment(&rt.lines, 0..99, 0), 0..1);
        assert_eq!(segment(&[], 0..99, 0), 0..0);
    }
}
