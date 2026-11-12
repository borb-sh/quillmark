//! A revise's carry: what a value held that its rewrite does not spell,
//! carried onto the rewrite. Each anchor rides a character diff of the two
//! texts, or a verbatim move of the text it covers, and each island takes the
//! id of the stored island it continues, with the anchors of its cells whose
//! text holds.

use crate::delta::{Assoc, Delta, carry_range, diff};
use crate::model::{Anchor, Content, HARD_BREAK, Island, Scalar};
use similar::{Algorithm, DiffOp, capture_diff_slices};
use std::collections::HashSet;

/// An anchor tag's position in a revise's content: where the anchor of `id`
/// starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub at: Scalar,
    pub id: String,
}

/// The text as the diff reads it: each hard break a `\n`, as a paragraph
/// break is, so a break that changes kind is no edit to it.
fn diff_text(c: &Content) -> String {
    c.text
        .chars()
        .map(|ch| if ch == HARD_BREAK { '\n' } else { ch })
        .collect()
}

/// `new` holding each anchor of `base` the diff from `base` to `new` carries,
/// and each island the id of the `base` island it continues. `new` holds no
/// anchor of its own, as an import's output does not.
pub fn carry_onto(base: &Content, mut new: Content) -> (Content, Delta) {
    let (base_text, new_text) = (diff_text(base), diff_text(&new));
    let delta = diff(&base_text, &new_text);
    let base_chars: Vec<char> = base_text.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();
    let inserted = delta.inserted_spans();
    for a in &base.anchors {
        if let Some((start, end)) =
            carry_range(&delta, &base_chars, &new_chars, &inserted, a.start, a.end)
        {
            new.anchors.push(Anchor::new(a.id.clone(), start, end));
        }
    }
    carry_island_ids(&base.islands, &mut new);
    new.normalize();
    (new, delta)
}

/// [`carry_onto`] without the delta, diffing only when `base` holds an
/// anchor to carry.
pub fn carry_anchors(base: &Content, mut new: Content) -> Content {
    if base.anchors.is_empty() {
        carry_island_ids(&base.islands, &mut new);
        new.normalize();
        return new;
    }
    carry_onto(base, new).0
}

/// [`carry_onto`], then each anchor a pin names placed to start at the pin.
/// One whose carried start the pin already is keeps its range. Any other
/// ends where the diff carries its stored end, when that lands from the pin to
/// the end of the pin's line, and otherwise spans its stored length, cut at
/// the line's end. A pin naming no prose anchor of `base` pins nothing.
pub fn carry_pinned(base: &Content, new: Content, pins: &[Pin]) -> (Content, Delta) {
    let (mut next, delta) = carry_onto(base, new);
    if pins.is_empty() {
        return (next, delta);
    }
    let chars: Vec<char> = diff_text(&next).chars().collect();
    for pin in pins {
        let Some(stored) = base.anchors.iter().find(|a| a.id == pin.id) else {
            continue;
        };
        let carried = next.anchors.iter().position(|a| a.id == pin.id);
        if carried.is_some_and(|k| next.anchors[k].start == pin.at) {
            continue;
        }
        let line_end = chars[pin.at..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(chars.len(), |n| pin.at + n);
        let mapped = delta.map_pos(stored.end, Assoc::Before);
        let end = if (pin.at..=line_end).contains(&mapped) {
            mapped
        } else {
            pin.at + (stored.end - stored.start).min(line_end - pin.at)
        };
        if let Some(k) = carried {
            next.anchors.remove(k);
        }
        next.anchors.push(Anchor::new(pin.id.clone(), pin.at, end));
    }
    next.normalize();
    (next, delta)
}

/// Give each island of `new` the id of the `base` island it continues, and
/// every other island the next `isl-{n}` past the highest in `base`, so a
/// revise rewrites no island's id and gives none a dropped island's. An
/// island continues one equal to it, anchors aside, in the longest run of
/// equal islands the two sequences share, or, in a stretch between two of
/// those where both sides hold as many islands of the same types in order,
/// the one at its place: an edited table.
fn carry_island_ids(base: &[Island], new: &mut Content) {
    let key = |island: &Island| {
        let mut props = crate::view::layout(island);
        crate::lines::export::drop_anchor_marks(&mut props);
        (island.island_type.as_str().to_string(), props.to_string())
    };
    let old: Vec<_> = base.iter().map(key).collect();
    let fresh: Vec<_> = new.islands.iter().map(key).collect();
    let mut carried: Vec<Option<usize>> = vec![None; new.islands.len()];
    for op in capture_diff_slices(Algorithm::Myers, &old, &fresh) {
        let (from, to, len) = match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => (old_index, new_index, len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } if old_len == new_len
                && (0..old_len).all(|k| old[old_index + k].0 == fresh[new_index + k].0) =>
            {
                (old_index, new_index, old_len)
            }
            _ => continue,
        };
        for k in 0..len {
            carried[to + k] = Some(from + k);
        }
    }
    carry_slot_anchors(base, new, &carried);
    let taken: HashSet<&str> = base.iter().map(|i| i.id.as_str()).collect();
    let mut next = base
        .iter()
        .filter_map(|island| island.id.strip_prefix("isl-")?.parse::<u64>().ok())
        .max()
        .map_or(0, |n| u128::from(n) + 1);
    for (island, from) in new.islands.iter_mut().zip(carried) {
        island.id = from.map(|k| base[k].id.clone()).unwrap_or_else(|| {
            while taken.contains(format!("isl-{next}").as_str()) {
                next += 1;
            }
            let id = format!("isl-{next}");
            next += 1;
            id
        });
    }
}

/// An island that continues one takes each anchor of the slot at the same
/// place in it whose text is unchanged, unless `new` already holds the id.
fn carry_slot_anchors(base: &[Island], new: &mut Content, carried: &[Option<usize>]) {
    let mut held: HashSet<String> = new.anchor_ids().into_iter().collect();
    for (island, from) in new.islands.iter_mut().zip(carried) {
        let Some(from) = *from else { continue };
        let stored = &base[from];
        if stored.slots.values().all(|s| s.anchors.is_empty()) {
            continue;
        }
        for (was, now) in stored.slot_order().iter().zip(island.slot_order()) {
            let (Some(was), Some(now)) = (stored.slots.get(was), island.slots.get_mut(&now)) else {
                continue;
            };
            if was.text != now.text {
                continue;
            }
            now.anchors.extend(
                was.anchors
                    .iter()
                    .filter(|a| held.insert(a.id.clone()))
                    .cloned(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::from_markdown;

    /// The carry rewrites no island id: an island inserted above keeps the
    /// rest theirs and takes the next past them, a table edited in place keeps
    /// its own, a deletion leaves the survivors theirs, and a rewrite mints
    /// past every id the field held.
    #[test]
    fn the_carry_rewrites_no_island_id() {
        let tables = |heads: &[&str]| {
            let md: Vec<String> = heads
                .iter()
                .map(|h| format!("| {h} |\n|---|\n| 1 |"))
                .collect();
            from_markdown(&md.join("\n\n")).unwrap().content
        };
        let ids = |c: &Content| c.islands.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        let base = tables(&["a", "b"]);
        assert_eq!(ids(&base), ["isl-0", "isl-1"]);

        let inserted = carry_anchors(&base, tables(&["z", "a", "b"]));
        assert_eq!(ids(&inserted), ["isl-2", "isl-0", "isl-1"]);
        let edited = carry_anchors(&inserted, tables(&["z", "A", "b"]));
        assert_eq!(ids(&edited), ["isl-2", "isl-0", "isl-1"]);
        let deleted = carry_anchors(&edited, tables(&["A", "b"]));
        assert_eq!(ids(&deleted), ["isl-0", "isl-1"]);
        let rewritten = carry_anchors(&deleted, tables(&["x", "y", "w"]));
        assert_eq!(ids(&rewritten), ["isl-2", "isl-3", "isl-4"]);

        let mut top = tables(&["a"]);
        top.islands[0].id = format!("isl-{}", u64::MAX);
        let grown = carry_anchors(&top, tables(&["a", "b", "c"]));
        assert_eq!(
            ids(&grown),
            [
                format!("isl-{}", u64::MAX),
                format!("isl-{}", u128::from(u64::MAX) + 1),
                format!("isl-{}", u128::from(u64::MAX) + 2),
            ]
        );
    }
}
