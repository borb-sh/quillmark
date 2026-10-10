//! Card alignment for a whole-document revise: which stored card each incoming
//! composable card revises, when cards carry no id.

use std::collections::HashMap;

use quillmark_content::delta::{word_similarity, MAX_DIFFED_WORDS, MIN_WORD_SIMILARITY};

/// One composable card as alignment reads it: its `$kind` and a text to
/// measure similarity over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot<'a> {
    pub kind: &'a str,
    pub text: &'a str,
}

/// Above this much estimated diff work, summed over the same-kind pairs as
/// `(words + words)²`, similarity is text equality alone.
const MAX_DIFF_WORK: usize = 50_000_000;

/// Above this many unpaired cards on one side times the other, the cards that
/// are not twins pair by position alone.
const MAX_TABLE_CELLS: usize = 4_000_000;

/// The similarity, in thousandths, a pair needs to align by text. Below it two
/// cards pair only by position, between the cards that aligned by text.
const MIN_PAIR_PERMILLE: u64 = (MIN_WORD_SIMILARITY * 1000.0) as u64;

/// How an incoming card came to revise a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pairing {
    /// Their texts match, or resemble each other more than any rival pairing.
    Text,
    /// Same kind and unlike texts, both in one gap the text pairs leave, taken
    /// in order.
    Position,
}

/// For each incoming card, the stored card it revises and how they paired, or
/// `None` for an inserted card. Stored cards no entry names are removed.
///
/// Only cards of one `$kind` pair. A card whose `(kind, text)` occurs exactly
/// once on each side pairs with its twin wherever either sits, so a reorder of
/// unchanged cards keeps every pairing. The rest align as a diff over the kind
/// sequence that maximizes how far each pair's similarity clears
/// [`MIN_PAIR_PERMILLE`], so a deleted card and an inserted one never outweigh
/// one edited card. Cards left unpaired in each gap, between two consecutive
/// text pairs or before the first or after the last, then pair in order by
/// kind.
pub(crate) fn align(stored: &[Slot<'_>], incoming: &[Slot<'_>]) -> Vec<Option<(usize, Pairing)>> {
    let mut out = vec![None; incoming.len()];
    let mut taken = vec![false; stored.len()];
    for (j, i) in unique_twins(stored, incoming) {
        out[j] = Some((i, Pairing::Text));
        taken[i] = true;
    }

    let rest_stored: Vec<usize> = (0..stored.len()).filter(|&i| !taken[i]).collect();
    let rest_incoming: Vec<usize> = (0..incoming.len()).filter(|&j| out[j].is_none()).collect();
    let stored_words = words(stored, &rest_stored);
    let incoming_words = words(incoming, &rest_incoming);
    let (n, m) = (rest_stored.len(), rest_incoming.len());
    let same_kind = |a: usize, b: usize| stored[rest_stored[a]].kind == incoming[rest_incoming[b]].kind;

    if n.saturating_mul(m) > MAX_TABLE_CELLS {
        pair_by_position((0, 0), (n, m), &same_kind, &rest_stored, &rest_incoming, &mut out);
        return out;
    }

    let mut work = 0usize;
    for a in 0..n {
        for b in 0..m {
            if same_kind(a, b) {
                let size = (stored_words[a].len() + incoming_words[b].len()).min(2 * MAX_DIFFED_WORDS);
                work = work.saturating_add(size * size);
            }
        }
    }
    let exact_only = work > MAX_DIFF_WORK;

    let gain = |a: usize, b: usize| -> Option<u64> {
        if !same_kind(a, b) {
            return None;
        }
        let sim = if stored[rest_stored[a]].text == incoming[rest_incoming[b]].text {
            1000
        } else if exact_only {
            0
        } else {
            similarity_permille(&stored_words[a], &incoming_words[b])
        };
        (sim >= MIN_PAIR_PERMILLE).then(|| sim - MIN_PAIR_PERMILLE + 1)
    };

    let mut table = vec![vec![0u64; m + 1]; n + 1];
    let mut gains = vec![vec![None; m]; n];
    for a in (0..n).rev() {
        for b in (0..m).rev() {
            let g = gain(a, b);
            gains[a][b] = g;
            let paired = g.map_or(0, |g| g + table[a + 1][b + 1]);
            table[a][b] = paired.max(table[a + 1][b]).max(table[a][b + 1]);
        }
    }

    let mut gap = (0, 0);
    let (mut a, mut b) = (0, 0);
    while a < n && b < m {
        match gains[a][b] {
            Some(g) if g + table[a + 1][b + 1] == table[a][b] => {
                pair_by_position(gap, (a, b), &same_kind, &rest_stored, &rest_incoming, &mut out);
                out[rest_incoming[b]] = Some((rest_stored[a], Pairing::Text));
                a += 1;
                b += 1;
                gap = (a, b);
            }
            _ if table[a + 1][b] == table[a][b] => a += 1,
            _ => b += 1,
        }
    }
    pair_by_position(gap, (n, m), &same_kind, &rest_stored, &rest_incoming, &mut out);
    out
}

fn words<'a>(slots: &[Slot<'a>], rest: &[usize]) -> Vec<Vec<&'a str>> {
    rest.iter().map(|&k| slots[k].text.split_whitespace().collect()).collect()
}

/// Pairs the cards of `[from, to)` on each side in order, each incoming card
/// with the next stored card of its kind.
fn pair_by_position(
    from: (usize, usize),
    to: (usize, usize),
    same_kind: &impl Fn(usize, usize) -> bool,
    rest_stored: &[usize],
    rest_incoming: &[usize],
    out: &mut [Option<(usize, Pairing)>],
) {
    let mut a = from.0;
    for b in from.1..to.1 {
        if let Some(found) = (a..to.0).find(|&a| same_kind(a, b)) {
            out[rest_incoming[b]] = Some((rest_stored[found], Pairing::Position));
            a = found + 1;
        }
    }
}

/// `(incoming, stored)` index pairs of cards whose `(kind, text)` occurs once
/// on each side.
fn unique_twins(stored: &[Slot<'_>], incoming: &[Slot<'_>]) -> Vec<(usize, usize)> {
    fn singles<'s>(slots: &[Slot<'s>]) -> HashMap<(&'s str, &'s str), Option<usize>> {
        let mut seen: HashMap<(&str, &str), Option<usize>> = HashMap::new();
        for (index, slot) in slots.iter().enumerate() {
            seen.entry((slot.kind, slot.text))
                .and_modify(|e| *e = None)
                .or_insert(Some(index));
        }
        seen
    }
    let stored = singles(stored);
    let mut pairs: Vec<(usize, usize)> = singles(incoming)
        .into_iter()
        .filter_map(|(key, j)| Some((j?, (*stored.get(&key)?)?)))
        .collect();
    pairs.sort_unstable();
    pairs
}

fn similarity_permille(a: &[&str], b: &[&str]) -> u64 {
    (word_similarity(a, b).clamp(0.0, 1.0) * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots<'a>(cards: &[(&'a str, &'a str)]) -> Vec<Slot<'a>> {
        cards.iter().map(|&(kind, text)| Slot { kind, text }).collect()
    }

    fn run(stored: &[(&str, &str)], incoming: &[(&str, &str)]) -> Vec<Option<usize>> {
        align(&slots(stored), &slots(incoming))
            .into_iter()
            .map(|pair| pair.map(|(i, _)| i))
            .collect()
    }

    #[test]
    fn identical_sequences_align_positionally() {
        let cards = [("a", "one"), ("b", "two"), ("a", "one")];
        assert_eq!(run(&cards, &cards), vec![Some(0), Some(1), Some(2)]);
    }

    #[test]
    fn a_reorder_of_unchanged_cards_keeps_every_pairing() {
        let stored = [("note", "alpha text"), ("note", "beta text"), ("memo", "gamma")];
        let incoming = [("memo", "gamma"), ("note", "beta text"), ("note", "alpha text")];
        assert_eq!(run(&stored, &incoming), vec![Some(2), Some(1), Some(0)]);
    }

    #[test]
    fn an_edited_card_pairs_with_the_stored_card_it_resembles() {
        let stored = [
            ("note", "the quick brown fox jumps"),
            ("note", "lorem ipsum dolor sit amet"),
        ];
        let incoming = [("note", "lorem ipsum dolor sit amet, consectetur")];
        assert_eq!(run(&stored, &incoming), vec![Some(1)]);
    }

    #[test]
    fn only_cards_of_one_kind_pair() {
        let stored = [("note", "same words here")];
        let incoming = [("memo", "same words here")];
        assert_eq!(run(&stored, &incoming), vec![None]);
    }

    #[test]
    fn of_two_crossing_pairs_the_more_similar_one_aligns() {
        let stored = [("a", "x1"), ("b", "keep me"), ("c", "gone")];
        let incoming = [("d", "new"), ("b", "keep me edited"), ("a", "x1 changed")];
        assert_eq!(run(&stored, &incoming), vec![None, Some(1), None]);
    }

    #[test]
    fn a_deleted_and_an_inserted_card_do_not_outweigh_an_edited_one() {
        let stored = [
            ("note", "owner: Ann\nstatus: done\n\nWrite the intro section."),
            ("note", "owner: Bob\nstatus: open\n\nReview the budget table."),
        ];
        let incoming = [
            ("note", "owner: Bob\nstatus: done\n\nReview the budget table and sign off."),
            ("note", "owner: Cy\nstatus: open\n\nDraft the appendix."),
        ];
        let aligned = align(&slots(&stored), &slots(&incoming));
        assert_eq!(aligned, vec![Some((1, Pairing::Text)), None]);
    }

    #[test]
    fn a_rewritten_card_in_place_pairs_by_position() {
        let stored = [("a", "head"), ("note", "one two three"), ("b", "tail")];
        let incoming = [("a", "head"), ("note", "four five six"), ("b", "tail")];
        let aligned = align(&slots(&stored), &slots(&incoming));
        assert_eq!(aligned[1], Some((1, Pairing::Position)));
    }
}
