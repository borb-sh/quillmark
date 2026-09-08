//! Exhaustive guard on the container canonical form.
//!
//! `Content::normalize` derives `ordinal` and `instance` from run structure, and
//! the direct-write lane — a client building containers by hand and committing
//! them through `SetContainers` or a storage decode — is where every shape the
//! Markdown importer never mints comes from. Two properties, checked over every
//! container-path sequence in a small space rather than sampled:
//!
//! 1. **Idempotence.** `normalize` is the fixed point the canonical
//!    serialization commits to, so a second pass must change nothing.
//! 2. **One canonical form per document.** `from_markdown(to_markdown(rt)) == rt`
//!    for normalized `rt`, which is `export`'s contract. A break here means two
//!    normalized contents project to one markdown, so the model holds a
//!    distinction it cannot write down.

use quillmark_content::model::{Container, Content, Line, LineKind, Normalized};
use quillmark_content::{from_markdown, to_markdown};

/// Containers a hand-built path can hold. `Unknown` is excluded from the
/// round-trip half only — it projects transparently, so Markdown cannot carry
/// it — but is exercised for idempotence.
fn alphabet(unknown: bool) -> Vec<Container> {
    let mut v = Vec::new();
    for ordered in [false, true] {
        for ordinal in [0u64, 1] {
            for instance in [0u64, 1] {
                v.push(Container::ListItem {
                    ordered,
                    start: 1,
                    ordinal,
                    instance,
                });
            }
        }
    }
    v.push(Container::Quote { instance: 0 });
    v.push(Container::Quote { instance: 1 });
    if unknown {
        v.push(Container::Unknown {
            tag: "x".into(),
            attrs: serde_json::Value::Null,
            instance: 0,
        });
    }
    v
}

/// Every path of depth 1..=2 over the alphabet.
fn paths(unknown: bool) -> Vec<Vec<Container>> {
    let a = alphabet(unknown);
    let mut out: Vec<Vec<Container>> = a.iter().map(|c| vec![c.clone()]).collect();
    for outer in &a {
        for inner in &a {
            out.push(vec![outer.clone(), inner.clone()]);
        }
    }
    out
}

fn build(paths: &[&Vec<Container>]) -> Normalized {
    let text = (0..paths.len())
        .map(|i| ((b'a' + i as u8) as char).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let lines = paths
        .iter()
        .map(|p| Line::new(LineKind::Para).with_containers((*p).clone()))
        .collect();
    Content::new(text, lines).into_normalized()
}

#[test]
fn normalize_is_idempotent_over_every_two_line_path_pair() {
    let ps = paths(true);
    let mut n = 0usize;
    for a in &ps {
        for b in &ps {
            let rt = build(&[a, b]);
            assert_eq!(rt.validate(), Ok(()), "invalid after normalize: {rt:?}");
            let again = rt.clone().into_content().into_normalized();
            assert_eq!(again, rt, "normalize not idempotent for {a:?} then {b:?}");
            n += 1;
        }
    }
    eprintln!("  pairs checked: {n}");
    assert!(n > 400);
}

/// An inner run must not continue across an *item* boundary: two inner lists
/// under two outer items are two lists, so each restarts at ordinal 0 and
/// neither needs a discriminator — and the markdown projection has to agree.
#[test]
fn every_normalized_pair_is_a_markdown_fixed_point() {
    let ps = paths(false);
    let mut broken = Vec::new();
    let mut n = 0usize;
    for a in &ps {
        for b in &ps {
            let rt = build(&[a, b]);
            let md = to_markdown(&rt);
            let back = from_markdown(&md).expect("re-imports");
            if back != rt {
                broken.push(format!("{a:?} then {b:?} -> {md:?}"));
            }
            n += 1;
        }
    }
    eprintln!("  pairs checked: {n}, broken: {}", broken.len());
    for b in broken.iter().take(5) {
        eprintln!("    {b}");
    }
    assert!(broken.is_empty(), "{} pairs are not fixed points", broken.len());
}

/// Three deep, where an item boundary at depth 0 has to reset both the ordinal
/// and the discriminator at depth 1.
#[test]
fn triples_over_the_list_and_quote_alphabet_are_fixed_points() {
    let ps: Vec<Vec<Container>> = paths(false)
        .into_iter()
        .filter(|p| p.len() == 2)
        .collect();
    let mut broken = 0usize;
    let mut n = 0usize;
    for a in ps.iter().step_by(3) {
        for b in ps.iter().step_by(3) {
            for c in ps.iter().step_by(5) {
                let rt = build(&[a, b, c]);
                let again = rt.clone().into_content().into_normalized();
                assert_eq!(again, rt, "not idempotent: {a:?} {b:?} {c:?}");
                if from_markdown(&to_markdown(&rt)).unwrap() != rt {
                    broken += 1;
                }
                n += 1;
            }
        }
    }
    eprintln!("  triples checked: {n}, broken: {broken}");
    assert_eq!(broken, 0);
}

/// Identity and projection are separate rules, and a `start`-only difference
/// falls between them. `same_run` counts `start`, so the two runs arrive apart
/// with no discriminator written; Markdown reads only a list's first number, so
/// the canonical form spends one to stay a fixed point. The alphabet above
/// holds `start` at 1, the axis it misses.
#[test]
fn a_start_only_difference_separates_the_runs_and_still_costs_a_discriminator() {
    let one = Container::ListItem {
        ordered: true,
        start: 1,
        ordinal: 0,
        instance: 0,
    };
    let three = Container::ListItem {
        ordered: true,
        start: 3,
        ordinal: 0,
        instance: 0,
    };
    assert!(!one.same_run(&three), "start is part of the shape");
    assert!(one.same_weld(&three), "markdown cannot carry the second start");

    let rt = build(&[&vec![one], &vec![three]]);
    assert_eq!(rt.lines[1].containers[0].instance(), 1);
    assert_eq!(from_markdown(&to_markdown(&rt)).expect("re-imports"), rt);
}

/// The projection rule is coarser than the identity rule, never finer: two runs
/// the walks read as one can never need a discriminator to stay apart. `start`
/// is the axis the two differ on, so the space here carries both.
#[test]
fn same_run_implies_same_weld() {
    let mut space = alphabet(true);
    for c in alphabet(true) {
        if let Container::ListItem {
            ordered,
            ordinal,
            instance,
            ..
        } = c
        {
            space.push(Container::ListItem {
                ordered,
                start: 3,
                ordinal,
                instance,
            });
        }
    }
    for a in &space {
        for b in &space {
            assert!(
                !a.same_run(b) || a.same_weld(b),
                "the identity rule joins a pair the projection parts: {a:?} / {b:?}"
            );
        }
    }
}
