//! Parity of a value held in Automerge with the content model: every value
//! reads back as itself, every change lowered from its trace reads back as
//! the value `apply` gives with nothing diffed, and peers editing at once,
//! some of them at heads they read earlier, read one valid value. `CASES`
//! and `SESSIONS` raise the case counts, `SEED` picks the sessions' seed.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::Held;
use quillmark_content::change::{Change, apply_traced};
use quillmark_content::model::Content;

fn reads_as(held: &Held, want: &Content) -> Result<(), TestCaseError> {
    let got = held.read();
    prop_assert!(got.repairs.is_empty(), "repairs {:?}\nraw {}", got.repairs, held.raw());
    let (g, w) = (got.content.to_value(), want.to_value());
    if g != w {
        let keys: Vec<String> = w.as_object().unwrap().keys().chain(g.as_object().unwrap().keys()).filter(|k| g.get(*k) != w.get(*k)).cloned().collect();
        let show: Vec<String> = keys.iter().map(|k| format!("{k}: got {} want {}", g.get(k).map_or("-".into(), |v| v.to_string()), w.get(k).map_or("-".into(), |v| v.to_string()))).collect();
        prop_assert!(false, "{}", show.join("\n"));
    }
    Ok(())
}

/// `change` written into `held`, which reads as `before`: lowered from its
/// trace, and the content `apply` gives.
fn write(held: &mut Held, before: &Content, change: &Change) -> Content {
    let (done, trace) = apply_traced(before, change).unwrap();
    held.write(&trace, &done.content);
    done.content
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases("CASES", 64), ..ProptestConfig::default() })]

    #[test]
    fn every_value_reads_back_as_itself(d in document()) {
        let mut held = Held::new(&d);
        reads_as(&held, &d)?;
        let loaded = Held::load(&held.save()).unwrap();
        reads_as(&loaded, &d)?;
    }

    #[test]
    fn every_change_lowered_from_its_trace_reads_as_applied(d in document(), ops in ops()) {
        let change = change_for(&d, &ops);
        let mut held = Held::new(&d);
        let after = write(&mut held, &d, &change);
        reads_as(&held, &after)?;
        prop_assert!(!held.sync_to(&after), "the text the trace lowered differs from the applied text");
        reads_as(&held, &after)?;
    }

    /// A change of more inserts than are shed one by one sets the formatting
    /// of each text it inserted into once, at its end.
    #[test]
    fn a_change_of_many_inserts_lowered_reads_as_applied(d in document(), ops in prop::collection::vec(ops(), 2..5)) {
        let change = change_for(&d, &ops.concat());
        let mut held = Held::new(&d);
        let after = write(&mut held, &d, &change);
        reads_as(&held, &after)?;
    }

    #[test]
    fn a_change_lowered_on_a_fork_at_earlier_heads_reads_as_applied_there(d in document(), first in ops(), ops in ops()) {
        let mut held = Held::new(&d);
        let heads = held.heads();
        write(&mut held, &d, &change_from(&d, &first, 500));
        let change = change_for(&d, &ops);
        let mut fork = held.fork_at(&heads, None).unwrap();
        reads_as(&fork, &d)?;
        let after = write(&mut fork, &d, &change);
        reads_as(&fork, &after)?;
        held.merge(&mut fork).unwrap();
        let merged = held.read();
        merged.content.validate().unwrap();
    }
}

#[derive(Debug, Default)]
struct Outcome {
    /// A local write that did not read back as the value `apply` gave.
    local: usize,
    /// Peers holding the same changes that read different values.
    peers: usize,
    /// A peer whose document read one raw value and another once saved and
    /// loaded.
    reload: usize,
    /// Merged states a peer wrote a repair back for.
    repaired: usize,
    /// Edits skipped while a repair that inserts waited for peer 0.
    waited: usize,
    /// Edits a peer wrote at heads it read before its last merge.
    stale: usize,
}

type Round = (usize, Vec<Op>, usize, bool);

/// Three peers start from one seed and edit at once, merging at random.
/// Before editing, a peer whose merged state needs repair writes the repair
/// back, unless the repair inserts and the peer is not peer 0, which stands
/// for the session's server; such a peer only merges. On a round marked
/// stale a peer writes its edit at the heads it read before its last merge,
/// as a write whose base is a value the document has moved on from. Once
/// every peer holds every change, each peer and a reload of each are read.
fn session(d: &Content, rounds: &[Round], out: &mut Outcome) {
    let base = Held::new(d);
    let mut peers: Vec<Held> = (0..3).map(|_| base.clone()).collect();
    for (p, peer) in peers.iter_mut().enumerate() {
        peer.set_actor(Some(&format!("peer{p}")));
    }
    let mut last: Vec<Option<(Vec<quillmark_automerge::automerge::ChangeHash>, Content)>> = vec![None, None, None];
    for (n, (p, ops, q, stale)) in rounds.iter().enumerate() {
        let read = peers[*p].read();
        if !read.repairs.is_empty() {
            if read.repairs.iter().any(|r| r.inserts()) && *p != 0 {
                out.waited += 1;
                merge_into(&mut peers, *q, *p);
                continue;
            }
            out.repaired += 1;
            peers[*p].sync_to(&read.content);
            if reads_as(&peers[*p], &read.content).is_err() {
                out.local += 1;
            }
        }
        let k = 1000 * (n + 1) + 100 * p;
        match (stale, last[*p].take()) {
            (true, Some((heads, c))) => {
                out.stale += 1;
                let change = change_from(&c, ops, k);
                let mut fork = peers[*p].fork_at(&heads, Some(&format!("peer{p}"))).unwrap();
                let after = write(&mut fork, &c, &change);
                if reads_as(&fork, &after).is_err() {
                    out.local += 1;
                }
                peers[*p].merge(&mut fork).unwrap();
            }
            _ => {
                let change = change_from(&read.content, ops, k);
                let after = write(&mut peers[*p], &read.content, &change);
                if reads_as(&peers[*p], &after).is_err() {
                    out.local += 1;
                }
            }
        }
        let now = peers[*p].read();
        if now.repairs.is_empty() {
            last[*p] = Some((peers[*p].heads(), now.content));
        }
        merge_into(&mut peers, *q, *p);
    }
    for p in 1..3 {
        merge_into(&mut peers, 0, p);
    }
    for p in 1..3 {
        merge_into(&mut peers, p, 0);
    }
    let reads: Vec<serde_json::Value> = peers.iter().map(|h| h.read().content.to_value()).collect();
    if reads[0] != reads[1] || reads[0] != reads[2] {
        out.peers += 1;
    }
    for held in peers.iter_mut() {
        let live = held.raw();
        if Held::load(&held.save()).unwrap().raw() != live {
            out.reload += 1;
            break;
        }
    }
}

fn merge_into(peers: &mut [Held], to: usize, from: usize) {
    if to == from {
        return;
    }
    let mut other = peers[from].clone();
    peers[to].merge(&mut other).unwrap();
}

fn rounds() -> impl Strategy<Value = Vec<Round>> {
    prop::collection::vec((0usize..3, ops(), 0usize..3, prop::bool::weighted(0.25)), 1..14)
}

/// Runs generated sessions, `SESSIONS` of them from seed `SEED`, and holds
/// every one to converging: each local write reads back as `apply` gave,
/// peers holding every change read one value, and a reload reads as the
/// live document.
#[test]
fn peers_editing_at_once_converge() {
    use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
    let config = ProptestConfig { cases: cases("SESSIONS", 48), ..ProptestConfig::default() };
    let rng = match std::env::var("SEED").ok().and_then(|n| n.parse::<u8>().ok()) {
        Some(n) => TestRng::from_seed(RngAlgorithm::ChaCha, &[n; 32]),
        None => TestRng::deterministic_rng(RngAlgorithm::ChaCha),
    };
    let mut runner = TestRunner::new_with_rng(config, rng);
    let out = std::cell::RefCell::new(Outcome::default());
    let sessions = std::cell::Cell::new(0);
    runner
        .run(&(document(), rounds()), |(d, rs)| {
            sessions.set(sessions.get() + 1);
            session(&d, &rs, &mut out.borrow_mut());
            Ok(())
        })
        .unwrap();
    let out = out.into_inner();
    eprintln!("{} sessions: {out:?}", sessions.get());
    assert_eq!((out.local, out.peers, out.reload), (0, 0, 0), "{out:?}");
}
