//! `locate` over generated content and selectors: text from a few letters,
//! spaces, line breaks and footnote slots, so quotes repeat and contexts
//! collide. It never panics, a located range reads back as the quote, and an
//! anchor's own context finds it or nothing.

use proptest::prelude::*;

use crate::anchor::{anchor_contexts, locate, AnchorContext};
use crate::island::IslandType;
use crate::model::{Content, Island, Line, LineKind, Mark, MarkKind, Normalized, ISLAND_SLOT};

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

fn stripped(rt: &Content, (start, end): (usize, usize)) -> String {
    rt.text.chars().skip(start).take(end - start).filter(|&c| c != ISLAND_SLOT).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_located_range_reads_back_as_its_quote(
        field in text(60),
        quote in text(8),
        prefix in text(12),
        suffix in text(12),
        spans in prop::collection::vec((0usize..70, 0usize..12), 0..3),
    ) {
        let rt = content(&field);
        prop_assume!(rt.validate().is_ok());
        let strip = |s: String| s.replace(ISLAND_SLOT, "");
        let ctx = AnchorContext::new("c", strip(quote), strip(prefix), strip(suffix));
        let spans: Vec<(usize, usize)> = spans.into_iter().map(|(s, n)| (s, s + n)).collect();
        for inserted in [None, Some(spans.as_slice())] {
            if let Some((start, end)) = locate(&rt, &ctx, inserted) {
                prop_assert!(start <= end && end <= rt.len_usv());
                prop_assert_eq!(stripped(&rt, (start, end)), ctx.quote.clone());
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
        let (start, end) = (a.min(b).min(len), a.max(b).min(len));
        rt.marks.push(Mark::new(start, end, MarkKind::Anchor { id: "c".into() }));
        let rt = rt.into_normalized();
        prop_assume!(rt.validate().is_ok());
        let ctx = anchor_contexts(&rt).remove(0);
        let Some(found) = locate(&rt, &ctx, None) else { return Ok(()) };
        let chars: Vec<char> = rt.text.chars().collect();
        if ctx.quote.is_empty() {
            prop_assert!(found.0 <= start && start <= found.1, "{found:?} around {start}");
        } else if chars[start] != ISLAND_SLOT && chars[end - 1] != ISLAND_SLOT {
            prop_assert_eq!(found, (start, end));
        }
    }
}
