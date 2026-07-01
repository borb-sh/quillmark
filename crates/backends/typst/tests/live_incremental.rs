//! Spike (#778): measure the wins of a persistent, incremental preview compiler
//! and prove the mechanism structurally.
//!
//! Each mode is measured from a *cold* comemo cache. Two edit shapes are timed:
//! an **end** edit (appends to the last paragraph — byte offsets of everything
//! before it are unchanged) and a **mid** edit (changes the middle paragraph —
//! shifts the byte offsets of everything after it). The mid edit is where an
//! incremental red-green `Source` and an `eval`'d string diverge.
//!
//! Run: cargo test -p quillmark-typst --release --test live_incremental -- --nocapture

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use quillmark_core::{FileTreeNode, Quill};
use quillmark_typst::live::{bench, BodyMode, LiveSession};

const PLATE: &str = r#"#import "@local/quillmark-helper:0.1.0": data
#set page(width: 400pt, height: 300pt, margin: 20pt)
#set text(font: "Figtree", size: 10pt)
#data.at("$body")
"#;

const N: usize = 400;

fn minimal_quill() -> Quill {
    let yaml = b"quill:\n  name: spike\n  version: 0.1.0\n  backend: typst\n  description: incremental preview spike\n";
    let mut files = HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File { contents: yaml.to_vec() },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("minimal quill")
}

/// Load a real fixture quill from disk (real fonts + vendored packages), so its
/// world-build cost reflects a production scaffolding load. `None` if absent.
fn fixture_quill(name: &str, ver: &str) -> Option<Quill> {
    fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
        let mut files = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let p: PathBuf = entry?.path();
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_file() {
                files.insert(n, FileTreeNode::File { contents: fs::read(&p)? });
            } else if p.is_dir() {
                files.insert(n, walk(&p)?);
            }
        }
        Ok(FileTreeNode::Directory { files })
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("fixtures/resources/quills")
        .join(name)
        .join(ver);
    if !root.exists() {
        return None;
    }
    Quill::from_tree(walk(&root).ok()?).ok()
}

/// `n` markdown paragraphs; appending `marker` to paragraph `edit_at`.
fn body(n: usize, edit_at: Option<usize>, marker: &str) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str("Paragraph ");
        s.push_str(&i.to_string());
        s.push_str(". Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod.");
        if edit_at == Some(i) {
            s.push(' ');
            s.push_str(marker);
        }
        s.push_str("\n\n");
    }
    s
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct ModeStat {
    cold: std::time::Duration,
    end: std::time::Duration,
    end_reparsed: usize,
    mid: std::time::Duration,
    mid_reparsed: usize,
    pages: usize,
}

/// Min recompile over `k` *novel* edits at paragraph `pos`, each applied from a
/// reset-to-base (cache-hit) start state. Using a fresh marker per iteration
/// forces real compute (no whole-doc cache hit) and the min removes first-edit
/// warmup noise.
fn min_edit(
    s: &mut LiveSession,
    base: &str,
    pos: usize,
    k: usize,
) -> (std::time::Duration, usize) {
    let mut best = std::time::Duration::MAX;
    let mut reparsed = 0;
    for j in 0..k {
        s.apply_body(base); // reset to base — a comemo hit
        let a = s.apply_body(&body(N, Some(pos), &format!("MK{j}Z")));
        assert!(a.committed);
        if a.recompile < best {
            best = a.recompile;
            reparsed = a.reparsed_bytes;
        }
    }
    (best, reparsed)
}

/// Open a mode from a cold cache, then measure end/mid novel-edit cost.
fn measure_mode(quill: &Quill, mode: BodyMode) -> ModeStat {
    let base = body(N, None, "");
    comemo::evict(0);
    let t = std::time::Instant::now();
    let mut s = LiveSession::open(quill, PLATE, "{}", &base, mode).expect("open");
    let cold = t.elapsed();

    let (end, end_reparsed) = min_edit(&mut s, &base, N - 1, 5);
    let (mid, mid_reparsed) = min_edit(&mut s, &base, N / 2, 5);

    ModeStat {
        cold,
        end,
        end_reparsed,
        mid,
        mid_reparsed,
        pages: s.page_count(),
    }
}

#[test]
fn incremental_preview_spike() {
    let quill = minimal_quill();

    // ---- stage 1: scaffolding cost, alone (no compile, no comemo) ----
    let build_min = (0..3).map(|_| bench::world_build_ms(&quill, PLATE)).min().unwrap();
    let build_real = fixture_quill("usaf_memo", "0.2.0")
        .map(|q| (0..3).map(|_| bench::world_build_ms(&q, "")).min().unwrap());

    // ---- per-mode incremental cost, each from a cold cache ----
    let inc = measure_mode(&quill, BodyMode::IncludeSource);
    let evb = measure_mode(&quill, BodyMode::EvalBlob);

    // ---- dirty-set demo + transactional rollback (IncludeSource) ----
    comemo::evict(0);
    let mut live =
        LiveSession::open(&quill, PLATE, "{}", &body(N, None, ""), BodyMode::IncludeSource)
            .expect("open live");
    let pages = live.page_count();

    let before = live.page_signatures();
    live.apply_body(&body(N, Some(N - 1), "ENDD"));
    let end_dirty = dirty_count(&before, &live.page_signatures());

    live.apply_body(&body(N, None, "")); // reset
    let before = live.page_signatures();
    live.apply_body(&body(N, Some(2), "STARTD"));
    let start_dirty = dirty_count(&before, &live.page_signatures());

    let good_pages = live.page_count();
    let bad = live.apply_body_markup("#panic(\"boom\")");
    assert!(!bad.committed, "broken compile must not commit");
    assert_eq!(live.page_count(), good_pages, "last-good retained on failure");

    // ---------------------------- report --------------------------------------
    eprintln!("\n=== #778 incremental-preview spike ({N} paragraphs, {pages} pages) ===");
    eprintln!("stage-1 world build, minimal quill : {:7.2} ms", ms(build_min));
    if let Some(b) = build_real {
        eprintln!("stage-1 world build, usaf_memo     : {:7.2} ms  (real fonts+packages)", ms(b));
    }
    eprintln!("                         cold open |   end edit   |   mid edit");
    eprintln!(
        "IncludeSource  : {:8.1} ms | {:6.1} ms {:>7}B | {:6.1} ms {:>7}B",
        ms(inc.cold), ms(inc.end), inc.end_reparsed, ms(inc.mid), inc.mid_reparsed
    );
    eprintln!(
        "EvalBlob       : {:8.1} ms | {:6.1} ms {:>7}B | {:6.1} ms {:>7}B",
        ms(evb.cold), ms(evb.end), evb.end_reparsed, ms(evb.mid), evb.mid_reparsed
    );
    eprintln!("mid-edit speedup (Eval/Include): {:.2}x", ms(evb.mid) / ms(inc.mid));
    eprintln!("reparse bytes, end edit: include {} vs eval {}", inc.end_reparsed, evb.end_reparsed);
    eprintln!("dirty pages: end {end_dirty}/{pages}, start {start_dirty}/{pages}");
    eprintln!("=========================================================\n");

    // -------------------------- structural asserts ----------------------------
    assert!(inc.pages > 5 && evb.pages > 5);
    // Incrementality of reparse: an end edit reparses a small tail in include
    // mode but the whole body-bearing literal in eval mode.
    assert!(inc.end_reparsed < 1000, "include end reparsed {}B", inc.end_reparsed);
    assert!(evb.end_reparsed > 10_000, "eval end reparsed {}B (expected whole blob)", evb.end_reparsed);
    // Flow layout: end edit dirties a small suffix.
    assert!(end_dirty <= 2, "end edit dirty pages = {end_dirty}");
    assert!(start_dirty >= end_dirty);
}

fn dirty_count(before: &[u128], after: &[u128]) -> usize {
    let n = before.len().max(after.len());
    (0..n).filter(|&i| before.get(i) != after.get(i)).count()
}
