//! `locate` over generated content: text from a few letters, spaces, line
//! breaks and footnote slots, so quotes repeat and contexts collide. A
//! selector is cut from its field, at times with one character changed, so
//! most land and some miss. It never panics, it names a range exactly when the
//! rule counts one place, and an anchor's own context finds it or nothing.

use proptest::prelude::*;

use crate::lines::anchor::{
    anchor_contexts, locate, AnchorContext, ANCHOR_AGREE_USV, ANCHOR_CONTEXT_USV, MIN_ANCHOR_QUOTE,
};
use crate::lines::island::IslandType;
use crate::lines::model::{Content, Island, Line, LineKind, Mark, MarkKind, Normalized, ISLAND_SLOT};

const ALPHABET: [char; 5] = ['a', 'b', ' ', '\n', ISLAND_SLOT];

fn content(text: &str) -> Normalized {
    let lines = vec![Line::new(LineKind::Para); text.matches('\n').count() + 1];
    let islands = (0..text.matches(ISLAND_SLOT).count())
        .map(|i| {
            Island::new(format!("isl-{i}"), IslandType::Footnote)
                .with_props(serde_json::json!({ "text": "note", "marks": [] }))
        })
        .collect();
    Content::new(text.to_string(), lines).with_islands(islands).into_normalized()
}

fn text(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(vec!['a', 'b', 'a', 'b', ' ', '\n', ISLAND_SLOT]), 0..max)
        .prop_map(String::from_iter)
}

/// A field and a selector cut from it at `i..j`, its context the stored text
/// either side up to the window and the field's edges, and at most one of its
/// three parts with one character changed. Half the fields repeat one run, so
/// a selector's context also agrees at a twin.
fn field_and_selector() -> impl Strategy<Value = (String, AnchorContext)> {
    let periodic = (text(16), 2usize..6).prop_map(|(run, times)| run.repeat(times));
    prop_oneof![text(60), periodic]
        .prop_flat_map(|field| {
            let n = field.chars().count();
            let change = prop::option::of((0usize..3, 0usize..64, prop::sample::select(ALPHABET.to_vec())));
            (Just(field), 0..=n, 0..=n, change)
        })
        .prop_map(|(field, a, b, change)| {
            let chars: Vec<char> = field.chars().collect();
            let (i, j) = (a.min(b), a.max(b));
            let mut parts = [
                chars[i..j].to_vec(),
                chars[i.saturating_sub(ANCHOR_CONTEXT_USV)..i].to_vec(),
                chars[j..(j + ANCHOR_CONTEXT_USV).min(chars.len())].to_vec(),
            ];
            if let Some((part, at, c)) = change {
                let part = &mut parts[part];
                if !part.is_empty() {
                    let at = at % part.len();
                    part[at] = c;
                }
            }
            let [quote, prefix, suffix] = parts.map(String::from_iter);
            (field, AnchorContext::new("c", quote, prefix, suffix))
        })
}

/// Every place the rule counts, read off its statement in COMMENTS.md.
fn counted(text: &[char], ctx: &AnchorContext, inserted: Option<&[(usize, usize)]>) -> Vec<(usize, usize)> {
    let quote: Vec<char> = ctx.quote.chars().collect();
    let prefix: Vec<char> = ctx.prefix.chars().collect();
    let suffix: Vec<char> = ctx.suffix.chars().collect();
    if !quote.is_empty() && quote.len() < MIN_ANCHOR_QUOTE && !quote.iter().all(|&c| c == ISLAND_SLOT) {
        return Vec::new();
    }
    let (before, after) = (prefix.len().min(ANCHOR_AGREE_USV), suffix.len().min(ANCHOR_AGREE_USV));
    let agrees_before = |i: usize| i >= before && text[i - before..i] == prefix[prefix.len() - before..];
    let agrees_after = |j: usize| text.len() - j >= after && text[j..j + after] == suffix[..after];
    let overlaps = |(a, b): (usize, usize)| inserted.is_none_or(|spans| spans.iter().any(|&(s, e)| a < e && b > s));
    (0..=text.len())
        .map(|i| (i, i + quote.len()))
        .filter(|&(i, j)| j <= text.len() && text[i..j] == quote[..] && agrees_before(i) && agrees_after(j))
        .filter(|&(i, j)| overlaps(if i == j { (i - before, j + after) } else { (i, j) }))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_located_range_is_the_one_place_the_rule_counts(
        (field, ctx) in field_and_selector(),
        spans in prop::collection::vec((0usize..70, 0usize..12), 0..3),
    ) {
        let rt = content(&field);
        prop_assume!(rt.validate().is_ok());
        let chars: Vec<char> = field.chars().collect();
        let spans: Vec<(usize, usize)> = spans.into_iter().map(|(s, n)| (s, s + n)).collect();
        for inserted in [None, Some(spans.as_slice())] {
            let counted = counted(&chars, &ctx, inserted);
            let found = locate(&rt, &ctx, inserted);
            prop_assert_eq!(found, (counted.len() == 1).then(|| counted[0]), "counted {:?}", counted);
            if let Some((start, end)) = found {
                prop_assert_eq!(String::from_iter(&chars[start..end]), ctx.quote.clone());
            }
        }
    }

    #[test]
    fn locate_never_panics_on_any_selector(
        field in text(60),
        quote in text(8),
        prefix in text(40),
        suffix in text(40),
        spans in prop::collection::vec((0usize..70, 0usize..12), 0..3),
    ) {
        let rt = content(&field);
        prop_assume!(rt.validate().is_ok());
        let ctx = AnchorContext::new("c", quote, prefix, suffix);
        let spans: Vec<(usize, usize)> = spans.into_iter().map(|(s, n)| (s, s + n)).collect();
        for inserted in [None, Some(spans.as_slice())] {
            if let Some((start, end)) = locate(&rt, &ctx, inserted) {
                prop_assert!(start <= end && end <= rt.len_usv());
                prop_assert_eq!(rt.text.chars().skip(start).take(end - start).collect::<String>(), ctx.quote.clone());
            }
        }
    }

    #[test]
    fn an_anchors_own_context_finds_it_or_nothing(
        field in text(60),
        a in 0usize..61,
        b in 0usize..61,
    ) {
        let mut rt = content(&field).into_content();
        let len = rt.len_usv();
        rt.marks.push(Mark::new(a.min(b).min(len), a.max(b).min(len), MarkKind::Anchor { id: "c".into() }));
        let rt = rt.into_normalized();
        prop_assume!(rt.validate().is_ok());
        let Some(anchor) = rt.marks.iter().find(|m| matches!(m.kind, MarkKind::Anchor { .. })) else {
            return Ok(());
        };
        let ctx = anchor_contexts(&rt).remove(0);
        if let Some(found) = locate(&rt, &ctx, None) {
            prop_assert_eq!(found, (anchor.start, anchor.end));
        }
    }
}
