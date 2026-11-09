//! What block ids cost on a long field, against a transient anchor per block,
//! in a release build: `cargo test --release -p quillmark-content --test
//! blocks_speed -- --ignored --nocapture`.

use quillmark_content::blocks::{BlockOp, batch, blocks};
use quillmark_content::change::{AnchorStep, Change, Splice, Step, apply};
use quillmark_content::diff::revise;
use quillmark_content::markdown::{from_markdown, to_markdown};
use quillmark_content::model::{Anchor, Content};
use std::hint::black_box;
use std::time::{Duration, Instant};

const PARA: &str =
    "holds **some bold words**, _a few italic ones_, and a [link](https://x.io) to end it.";

fn document(n: usize) -> Content {
    let md: Vec<String> = (0..n)
        .map(|i| match i % 10 {
            0 => format!("## Section {i:05}"),
            1..=3 => format!("- Item {i:05} {PARA}"),
            _ => format!("Para {i:05} {PARA}"),
        })
        .collect();
    from_markdown(&md.join("\n\n")).unwrap().content
}

fn anchored(c: &Content) -> Content {
    let steps = c
        .block_ranges()
        .iter()
        .enumerate()
        .map(|(i, &(s, e))| {
            Step::Anchor(AnchorStep::Place {
                anchor: Anchor::new(format!("~b{i}"), s, e),
                quote: None,
            })
        })
        .collect();
    apply(c, &Change::new(steps)).unwrap().content
}

fn time<T>(f: impl Fn() -> T) -> Duration {
    let runs = 20;
    let t = Instant::now();
    for _ in 0..runs {
        black_box(f());
    }
    t.elapsed() / runs
}

#[test]
#[ignore]
fn block_ids_against_an_anchor_per_block() {
    for n in [300, 3000] {
        let c = document(n);
        let a = anchored(&c);
        let mid = c.block_ranges()[n / 2].0 + 3;
        let typing = Change::new(vec![Step::Splice(Splice::new(mid, "", "x"))]);
        let md = to_markdown(&c).replacen("Para 00005", "Para 00005 edited", 1);
        let ids: Vec<String> = c.blocks.iter().map(|b| b.id.clone()).collect();
        let ops: Vec<BlockOp> = (0..10)
            .map(|i| BlockOp::Replace {
                id: ids[i * n / 10 + 5].clone(),
                with: from_markdown("Rewritten **here**.").unwrap().content,
            })
            .collect();
        println!("{n} blocks, {} chars", c.text.chars().count());
        println!("  blocks() read          {:?}", time(|| blocks(&c)));
        println!(
            "  batch of 10 replaces   {:?}",
            time(|| batch(&c, &ops).unwrap())
        );
        println!("  place an anchor/block  {:?}", time(|| anchored(&c)));
        println!(
            "  keystroke, block ids   {:?}",
            time(|| apply(&c, &typing).unwrap())
        );
        println!(
            "  keystroke, +anchors    {:?}",
            time(|| apply(&a, &typing).unwrap())
        );
        println!(
            "  revise, block ids      {:?}",
            time(|| revise(&c, &md).unwrap())
        );
        println!(
            "  revise, +anchors       {:?}",
            time(|| revise(&a, &md).unwrap())
        );
    }
}
