//! How long the content engine takes on long values, in a release build:
//! `cargo test --release -p quillmark-content --test speed -- --ignored
//! --nocapture`. A long change is a document of `n` paragraphs of about 72
//! characters and a change of one step per paragraph, and its 288k-character
//! case fails past [`BUDGET`]. A long value is `n` list items, quotes and
//! paragraphs nested in containers, with `x-` marks and anchors, and its
//! 20000-item case fails past [`VALUE_BUDGET`]. The interactive target, a
//! field of 20000 blocks or 500000 characters, holds [`TARGET`]: a plain
//! list's revise, replace and repair in time, and import and decode linear.

use quillmark_automerge::{Held, revise};
use quillmark_content::change::{AnchorStep, Change, MarkStep, Splice, Step, apply, apply_traced, invert};
use quillmark_content::diff::{diff, replace};
use quillmark_content::markdown::{from_markdown, to_markdown};
use quillmark_content::model::{Anchor, Block, BlockKind, Content, Mark, MarkKind};
use quillmark_content::repair::{Raw, repair};
use serde_json::{Map, Value, json};
use quillmark_content::units::{Utf16, change_from_scalars, change_to_scalars};
use std::hint::black_box;
use std::time::{Duration, Instant};

const PARA: &str = "holds **some bold words**, _a few italic ones_, and a [link](https://x.io) to end it.";

fn document(n: usize) -> Content {
    let md: Vec<String> = (0..n)
        .map(|i| match i % 25 {
            0 => format!("## Section {i:05} holds the end of a part"),
            _ => format!("Para {i:05} {PARA}"),
        })
        .collect();
    let mut c = quillmark_content::markdown::from_markdown(&md.join("\n\n")).unwrap().content;
    let anchors: Vec<Anchor> = c
        .block_ranges()
        .iter()
        .enumerate()
        .filter(|(k, _)| k % 10 == 3)
        .map(|(k, &(s, _))| Anchor::new(format!("c-{k}"), s + 5, s + 10))
        .collect();
    c.anchors = anchors;
    c.normalize();
    c.validate().unwrap();
    c
}

/// One step per block, front to back, each at an offset the steps before it
/// moved: `word` rewritten in most, and a mark, an anchor or a split in the
/// rest.
fn change(d: &Content, word: &str, to: &str, tag: &str) -> Change {
    let texts = d.block_texts();
    let mut shift = 0isize;
    let mut steps = Vec::new();
    for (k, &(start, _)) in d.block_ranges().iter().enumerate() {
        let text = &texts[k];
        let Some(byte) = text.find(word) else { continue };
        let at = (start + text[..byte].chars().count()).wrapping_add_signed(shift);
        let len = word.chars().count();
        steps.push(match k % 10 {
            7 => Step::Mark(MarkStep::Add { mark: Mark::new(at, at + len, MarkKind::Strong), quote: None }),
            8 => Step::Anchor(AnchorStep::Place { anchor: Anchor::new(format!("{tag}-{k}"), at, at + len), quote: None }),
            9 => {
                shift += 1;
                let block = Block::new(format!("{tag}{k}"), BlockKind::Para);
                Step::Splice(Splice { blocks: vec![block], ..Splice::new(at + len, "", "\n") })
            }
            _ => {
                shift += to.chars().count() as isize - len as isize;
                Step::Splice(Splice::new(at, word, to))
            }
        });
    }
    Change::new(steps)
}

fn time<T>(f: impl Fn() -> T) -> Duration {
    let runs: usize = std::env::var("SPEED_RUNS").ok().and_then(|r| r.parse().ok()).unwrap_or(3);
    let mut best = Duration::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        black_box(f());
        best = best.min(t.elapsed());
    }
    best
}

/// The 288k-character case's limits in milliseconds: five times the targets
/// of 100 to apply and 200 to invert, and about ten times what a laptop takes
/// for the diff and the UTF-16 round trip, which a pass that walks the whole
/// text per step overruns. The merge is a stale write on a live document that
/// has taken another change since, and its limit is about 1.3 times what it
/// takes.
const BUDGET: [(&str, f64); 5] = [("apply", 500.0), ("invert", 1000.0), ("diff", 1500.0), ("merge", 2500.0), ("utf16", 500.0)];

fn measure(n: usize) -> (usize, usize, [f64; 5]) {
    let d = document(n);
    let mine = change(&d, "holds", "carries", "m");
    let theirs = change(&d, "end", "close", "t");
    let after = apply(&d, &mine).unwrap().content;
    let (ours, ours_trace) = apply_traced(&d, &mine).unwrap();
    let (since, since_trace) = apply_traced(&d, &theirs).unwrap();
    let mut live = Held::new(&d, "");
    let heads = live.heads();
    live.write(&since_trace, &since.content);
    let times = [
        time(|| apply(&d, &mine).unwrap()),
        time(|| invert(&mine, &d).unwrap()),
        time(|| apply(&d, &diff(&d, &after).unwrap()).unwrap()),
        time(|| {
            let mut live = live.clone();
            let at_base = live.fork_at(&heads, None).unwrap();
            let mut fork = at_base.clone();
            fork.write(&ours_trace, &ours.content);
            assert!(Held::restyled(&at_base, &fork, &live, &ours_trace, &ours.applied).is_none());
            let (at_ours, at_theirs) = (fork.heads(), live.heads());
            live.merge(&mut fork).unwrap();
            live.clip(&at_ours, &at_theirs, &ours_trace);
            live.read()
        }),
        time(|| change_to_scalars(&change_from_scalars(&mine, &d, Utf16).unwrap(), &d, Utf16).unwrap()),
    ];
    (d.text.chars().count(), mine.steps.len(), times.map(|t| t.as_secs_f64() * 1000.0))
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn a_long_change_applies_inverts_diffs_and_merges_in_time() {
    let mut table = format!("{:>8} {:>6}", "chars", "steps");
    for (what, _) in BUDGET {
        table += &format!(" {:>10}", format!("{what} ms"));
    }
    let mut over = Vec::new();
    for n in [1000, 4000] {
        let (chars, steps, times) = measure(n);
        table += &format!("\n{chars:>8} {steps:>6}");
        for t in times {
            table += &format!(" {t:>10.1}");
        }
        if n == 4000 {
            over.extend(BUDGET.iter().zip(times).filter(|((_, limit), t)| t > limit).map(|((what, limit), t)| format!("{what} {t:.1} ms > {limit} ms")));
        }
    }
    println!("{table}");
    assert!(over.is_empty(), "past budget: {}", over.join(", "));
}

fn ext(name: &str, n: usize) -> MarkKind {
    let mut attrs = Map::new();
    attrs.insert("n".into(), n.into());
    MarkKind::Ext { name: format!("x-{name}"), attrs }
}

/// `n` items: runs of a nested bullet list, an ordered list and a quote
/// between paragraphs, with an `x-` mark on one item in three and an anchor on
/// one in seven.
fn long_value(n: usize) -> (String, Content) {
    let md: Vec<String> = (0..n)
        .map(|i| match i % 40 {
            0 => format!("\n## Part {i:05}\n"),
            1..=19 if i % 5 == 3 => format!("  - nested {i:05} item with *one* word"),
            1..=19 => format!("- item {i:05} carries a word"),
            20 => format!("\nThe paragraph {i:05} sits between lists.\n"),
            21..=34 => format!("{}. step {i:05} carries a word", i - 20),
            _ => format!("\n> quoted {i:05} carries a word"),
        })
        .collect();
    let md = md.join("\n");
    let mut c = from_markdown(&md).unwrap().content;
    let ranges = c.block_ranges();
    c.marks.extend(ranges.iter().enumerate().filter(|(k, _)| k % 3 == 1).map(|(k, &(s, e))| Mark::new(s, e.min(s + 6), ext("tag", k % 4))));
    c.anchors = ranges.iter().enumerate().filter(|(k, _)| k % 7 == 2).map(|(k, &(s, _))| Anchor::new(format!("a-{k}"), s + 2, s + 4)).collect();
    c.normalize();
    c.validate().unwrap();
    assert!(!c.containers.is_empty() && c.marks.iter().any(|m| matches!(m.kind, MarkKind::Ext { .. })));
    (md, c)
}

/// A value whose one `x-` mark per line would be cut at every break of the
/// text it spans: refused at decode, before it multiplies.
fn multiplying_value(n: usize) -> Value {
    let text = vec!["ab"; n].join("\n");
    let len = text.chars().count();
    let blocks: Vec<Value> = (0..n).map(|i| json!({"id": format!("b{i}"), "kind": "para"})).collect();
    let marks: Vec<Value> = (0..n).map(|i| json!({"start": 0, "end": len, "type": "x-a", "attrs": {"n": i}})).collect();
    json!({"text": text, "blocks": blocks, "marks": marks})
}

/// The 20000-item value's limits in milliseconds, about five times what a
/// loaded laptop takes: a lookup by scan per block overruns revise and
/// replace, a walk per repaired container overruns repair, and a mark cut at
/// every break before it is counted overruns refuse.
const VALUE_BUDGET: [(&str, f64); 7] =
    [("splice", 600.0), ("revise", 2500.0), ("replace", 1500.0), ("repair", 1000.0), ("import", 600.0), ("decode", 600.0), ("refuse", 100.0)];

fn measure_value(n: usize) -> (usize, [f64; 7]) {
    let (md, d) = long_value(n);
    let middle = d.block_ranges()[d.blocks.len() / 2].0;
    let one = Change::new(vec![Step::Splice(Splice::new(middle, "", "new "))]);
    let after = apply(&d, &one).unwrap().content;
    let written = to_markdown(&d);
    let at = written.len() / 2 + written[written.len() / 2..].find("word").unwrap();
    let revised = format!("{}phrase{}", &written[..at], &written[at + 4..]);
    let value = d.to_value();
    let multiplying = multiplying_value(2000);
    let times = [
        time(|| apply(&d, &one).unwrap()),
        time(|| revise(&d, &revised).unwrap()),
        time(|| replace(&d, &after).unwrap()),
        time(|| repair(Raw::from(&d))),
        time(|| from_markdown(&md).unwrap()),
        time(|| Content::from_value(&value).unwrap()),
        time(|| Content::from_value(&multiplying).unwrap_err()),
    ];
    (d.text.chars().count(), times.map(|t| t.as_secs_f64() * 1000.0))
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn a_long_value_writes_revises_repairs_and_decodes_in_time() {
    let mut table = format!("{:>6} {:>8}", "items", "chars");
    for (what, _) in VALUE_BUDGET {
        table += &format!(" {:>11}", format!("{what} ms"));
    }
    let mut over = Vec::new();
    for n in [5000, 20000] {
        let (chars, times) = measure_value(n);
        table += &format!("\n{n:>6} {chars:>8}");
        for t in times {
            table += &format!(" {t:>11.1}");
        }
        if n == 20000 {
            over.extend(VALUE_BUDGET.iter().zip(times).filter(|((_, limit), t)| t > limit).map(|((what, limit), t)| format!("{what} {t:.1} ms > {limit} ms")));
        }
    }
    println!("{table}");
    assert!(over.is_empty(), "past budget: {}", over.join(", "));
}

/// The interactive target's limits in milliseconds: a one-word revise and a
/// replace of a 20000-item list, and a repair of a valid 10000-item one.
const TARGET: [(&str, f64); 3] = [("revise", 300.0), ("replace", 300.0), ("repair", 50.0)];

/// What a CI runner may take beyond a laptop: each limit in [`TARGET`] is
/// checked at this multiple of itself.
const HEADROOM: f64 = 2.0;

/// The most import or decode may grow from 10000 items to 20000: twice, and
/// the allocator's and caches' share past it, under the 4 of a quadratic pass.
const LINEAR: f64 = 3.0;

fn plain_list(n: usize) -> (String, Content) {
    let md: Vec<String> = (0..n).map(|i| format!("- item {i:05} carries a word")).collect();
    let md = md.join("\n");
    let c = from_markdown(&md).unwrap().content;
    (md, c)
}

fn one_word(d: &Content) -> String {
    let written = to_markdown(d);
    let at = written.len() / 2 + written[written.len() / 2..].find("word").unwrap();
    format!("{}phrase{}", &written[..at], &written[at + 4..])
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn the_interactive_target_holds_its_budgets_and_grows_linearly() {
    let (_, list) = plain_list(20000);
    let revised = one_word(&list);
    let after = revise(&list, &revised).unwrap().content;
    let (_, half) = plain_list(10000);
    let ms = |t: Duration| t.as_secs_f64() * 1000.0;
    let times = [
        ms(time(|| revise(&list, &revised).unwrap())),
        ms(time(|| replace(&list, &after).unwrap())),
        ms(time(|| repair(Raw::from(&half)))),
    ];
    let mut over = Vec::new();
    for ((what, limit), t) in TARGET.iter().zip(times) {
        println!("{what:>8} {t:>8.1} ms (target {limit} ms)");
        if t > limit * HEADROOM {
            over.push(format!("{what} {t:.1} ms > {limit} ms x {HEADROOM}"));
        }
    }
    let (md_small, small) = long_value(10000);
    let (md_large, large) = long_value(20000);
    let (v_small, v_large) = (small.to_value(), large.to_value());
    let growth = [
        ("import", ms(time(|| from_markdown(&md_large).unwrap())) / ms(time(|| from_markdown(&md_small).unwrap()))),
        ("decode", ms(time(|| Content::from_value(&v_large).unwrap())) / ms(time(|| Content::from_value(&v_small).unwrap()))),
    ];
    for (what, ratio) in growth {
        println!("{what:>8} x{ratio:.2} from 10000 items to 20000");
        if ratio > LINEAR {
            over.push(format!("{what} grows x{ratio:.2} > x{LINEAR}"));
        }
    }
    assert!(over.is_empty(), "past budget: {}", over.join(", "));
}
