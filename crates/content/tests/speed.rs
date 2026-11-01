//! How long a long change takes, in a release build: `cargo test --release -p
//! quillmark-content --test speed -- --ignored --nocapture`. Each case is a
//! document of `n` paragraphs of about 72 characters and a change of one step
//! per paragraph. The 288k-character case fails past [`BUDGET`].

use quillmark_content::change::{AnchorStep, Change, MarkStep, Splice, Step, apply, invert, rebase};
use quillmark_content::diff::diff;
use quillmark_content::model::{Anchor, Block, BlockKind, Content, Mark, MarkKind};
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
            8 => Step::Anchor(AnchorStep::Set { anchor: Anchor::new(format!("{tag}-{k}"), at, at + len), quote: None }),
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
/// for the rest, which a pass that walks the whole text per step overruns.
const BUDGET: [(&str, f64); 5] = [("apply", 500.0), ("invert", 1000.0), ("revise", 1500.0), ("rebase", 2500.0), ("utf16", 500.0)];

fn measure(n: usize) -> (usize, usize, [f64; 5]) {
    let d = document(n);
    let mine = change(&d, "holds", "carries", "m");
    let theirs = change(&d, "end", "close", "t");
    let after = apply(&d, &mine).unwrap().content;
    let times = [
        time(|| apply(&d, &mine).unwrap()),
        time(|| invert(&mine, &d).unwrap()),
        time(|| apply(&d, &diff(&d, &after).unwrap().change).unwrap()),
        time(|| rebase(&d, &mine, &theirs).unwrap()),
        time(|| change_to_scalars(&change_from_scalars(&mine, &d, Utf16).unwrap(), &d, Utf16).unwrap()),
    ];
    (d.text.chars().count(), mine.steps.len(), times.map(|t| t.as_secs_f64() * 1000.0))
}

#[test]
#[ignore = "a release-build timing, run on its own"]
fn a_long_change_applies_inverts_revises_and_rebases_in_time() {
    print!("{:>8} {:>6}", "chars", "steps");
    for (what, _) in BUDGET {
        print!(" {:>10}", format!("{what} ms"));
    }
    println!();
    let mut over = Vec::new();
    for n in [1000, 4000] {
        let (chars, steps, times) = measure(n);
        print!("{chars:>8} {steps:>6}");
        for t in times {
            print!(" {t:>10.1}");
        }
        println!();
        if n == 4000 {
            over.extend(BUDGET.iter().zip(times).filter(|((_, limit), t)| t > limit).map(|((what, limit), t)| format!("{what} {t:.1} ms > {limit} ms")));
        }
    }
    assert!(over.is_empty(), "past budget: {}", over.join(", "));
}
