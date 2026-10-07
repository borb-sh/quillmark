//! Card alignment for a whole-document revise: which stored card each incoming
//! composable card revises, when cards carry no id.

use std::collections::HashMap;

/// One composable card as alignment reads it: its `$kind` and a text to
/// measure similarity over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot<'a> {
    pub kind: &'a str,
    pub text: &'a str,
}

/// Above this many same-kind pairs, similarity is text equality alone: a word
/// diff per pair would make the alignment quadratic in diffs.
const MAX_SCORED_PAIRS: usize = 10_000;

/// Above this many chars in either text, a pair's similarity is the share of
/// its common prefix and suffix rather than a word diff.
const MAX_DIFFED_CHARS: usize = 20_000;

/// For each incoming card, the index of the stored card it revises, or `None`
/// for an inserted card. Stored cards no entry names are removed.
///
/// Only cards of one `$kind` pair. A card whose `(kind, text)` occurs exactly
/// once on each side pairs with its twin wherever either sits, so a reorder of
/// unchanged cards keeps every pairing. The rest align as a diff over the kind
/// sequence that maximizes total text similarity, so within a run of one kind
/// an edited card pairs with the stored card it most resembles.
pub(crate) fn align(stored: &[Slot<'_>], incoming: &[Slot<'_>]) -> Vec<Option<usize>> {
    let mut out = vec![None; incoming.len()];
    let mut taken = vec![false; stored.len()];
    for (j, i) in unique_twins(stored, incoming) {
        out[j] = Some(i);
        taken[i] = true;
    }

    let rest_stored: Vec<usize> = (0..stored.len()).filter(|&i| !taken[i]).collect();
    let rest_incoming: Vec<usize> = (0..incoming.len()).filter(|&j| out[j].is_none()).collect();
    let pairs = rest_stored
        .iter()
        .flat_map(|&i| rest_incoming.iter().map(move |&j| (i, j)))
        .filter(|&(i, j)| stored[i].kind == incoming[j].kind)
        .count();
    let exact_only = pairs > MAX_SCORED_PAIRS;

    // A pair scores one plus its similarity in thousandths, so any same-kind
    // pair beats leaving both unpaired and similarity decides between pairings.
    let score = |i: usize, j: usize| -> Option<u64> {
        let (a, b) = (stored[i], incoming[j]);
        (a.kind == b.kind).then(|| 1 + similarity_permille(a.text, b.text, exact_only))
    };

    let (n, m) = (rest_stored.len(), rest_incoming.len());
    let mut table = vec![vec![0u64; m + 1]; n + 1];
    let mut scores = vec![vec![None; m]; n];
    for a in (0..n).rev() {
        for b in (0..m).rev() {
            let s = score(rest_stored[a], rest_incoming[b]);
            scores[a][b] = s;
            let paired = s.map_or(0, |s| s + table[a + 1][b + 1]);
            table[a][b] = paired.max(table[a + 1][b]).max(table[a][b + 1]);
        }
    }
    let (mut a, mut b) = (0, 0);
    while a < n && b < m {
        match scores[a][b] {
            Some(s) if s + table[a + 1][b + 1] == table[a][b] => {
                out[rest_incoming[b]] = Some(rest_stored[a]);
                a += 1;
                b += 1;
            }
            _ if table[a + 1][b] == table[a][b] => a += 1,
            _ => b += 1,
        }
    }
    out
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

fn similarity_permille(a: &str, b: &str, exact_only: bool) -> u64 {
    if a == b {
        return 1000;
    }
    if exact_only {
        return 0;
    }
    let ratio = if a.len().max(b.len()) > MAX_DIFFED_CHARS {
        affix_ratio(a, b)
    } else {
        similar::TextDiff::from_words(a, b).ratio()
    };
    (ratio.clamp(0.0, 1.0) * 1000.0).round() as u64
}

/// The share of both texts their common prefix and suffix cover.
fn affix_ratio(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let shorter = a.len().min(b.len());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(shorter - prefix)
        .take_while(|(x, y)| x == y)
        .count();
    (2 * (prefix + suffix)) as f32 / total as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots<'a>(cards: &[(&'a str, &'a str)]) -> Vec<Slot<'a>> {
        cards.iter().map(|&(kind, text)| Slot { kind, text }).collect()
    }

    fn run(stored: &[(&str, &str)], incoming: &[(&str, &str)]) -> Vec<Option<usize>> {
        align(&slots(stored), &slots(incoming))
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
    fn affix_ratio_measures_shared_ends() {
        assert_eq!(affix_ratio("abcd", "abcd"), 1.0);
        assert_eq!(affix_ratio("abXd", "abYd"), 0.75);
        assert_eq!(affix_ratio("", "xy"), 0.0);
    }
}
