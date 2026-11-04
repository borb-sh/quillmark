//! Sessions of three peers editing one document at once, merging at random,
//! some writes at heads the peer read earlier: each local write reads back
//! as the value `apply` gives in text and structure, peers holding the same
//! changes read one value, and a reload reads as the live document. `CASES`
//! raises the case count, `SEED` picks the sessions' seed.
//!
//! Past a few hundred sessions a live document that took a write at earlier
//! heads can read an anchor in an island's slot that a reload of it does
//! not, so peers holding the same changes read different values; a peer
//! reloaded after each such write never does.

mod common;

use common::*;
use proptest::prelude::*;
use quillmark_automerge::automerge::{ActorId, ChangeHash, ObjId};
use quillmark_automerge::model::Doc;
use quillmark_automerge::content::change::apply;
use quillmark_automerge::content::model::Content;

#[derive(Debug, Default)]
struct Outcome {
    /// A local write that did not read back as the value `apply` gave.
    local: usize,
    /// Peers holding the same changes that read different values.
    peers: usize,
    /// A peer whose document read one value live and another once saved
    /// and loaded.
    reload: usize,
    /// Rounds a peer only merged, its read needing a repair, which a
    /// change made against the repaired read cannot lower onto.
    repaired: usize,
    /// Edits a peer wrote at heads it read before its last merge.
    stale: usize,
}

type Round = (usize, Vec<Op>, usize, bool);

/// A change made against `read` written into `doc` at `heads`, and whether
/// the value reads as `apply` gives in text and structure: at the change it
/// wrote, which holds `heads` and none of the changes since.
fn write(doc: &mut Doc, root: &ObjId, read: &Content, ops: &[Op], k: usize, heads: Option<&[ChangeHash]>) -> bool {
    let change = change_from(read, ops, k);
    let want = apply(read, &change).unwrap().content;
    let before = doc.heads();
    doc.apply(root, &change, heads).unwrap();
    let got = match heads {
        Some(h) => {
            let mut mine = doc.heads();
            mine.retain(|x| !before.contains(x));
            if mine.is_empty() {
                mine = h.to_vec();
            }
            doc.view_at(root, &mine).content
        }
        None => doc.view(root).content,
    };
    structure(&got) == structure(&want)
}

/// Three peers start from one seed and edit at once, merging at random. On
/// a round marked stale a peer writes its edit at the heads it read before
/// its last merge, as a write whose base is a value the document has moved
/// on from. Once every peer holds every change, each peer and a reload of
/// each are read.
fn session(d: &Content, rounds: &[Round], out: &mut Outcome) {
    let mut base = Doc::new();
    let main = base.main();
    let root = base.put_value(&main, "body", d);
    base.commit();
    let mut peers: Vec<Doc> = (0..3).map(|_| base.clone()).collect();
    for (p, peer) in peers.iter_mut().enumerate() {
        peer.set_actor(ActorId::from(format!("peer{p}").as_bytes()));
    }
    let mut last: Vec<Option<(Vec<ChangeHash>, Content)>> = vec![None, None, None];
    for (n, (p, ops, q, stale)) in rounds.iter().enumerate() {
        let read = peers[*p].view(&root);
        if !read.repairs.is_empty() {
            out.repaired += 1;
            merge_into(&mut peers, *q, *p);
            continue;
        }
        let k = 1000 * (n + 1) + 100 * p;
        let ok = match (stale, last[*p].take()) {
            (true, Some((heads, c))) => {
                out.stale += 1;
                write(&mut peers[*p], &root, &c, ops, k, Some(&heads))
            }
            _ => write(&mut peers[*p], &root, &read.content, ops, k, None),
        };
        if !ok {
            out.local += 1;
        }
        let now = peers[*p].view(&root);
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
    let reads: Vec<serde_json::Value> = peers.iter().map(|h| h.view(&root).content.to_value()).collect();
    if reads[0] != reads[1] || reads[0] != reads[2] {
        out.peers += 1;
    }
    for doc in peers.iter_mut() {
        if Doc::load(&doc.save()).unwrap().view(&root) != doc.view(&root) {
            out.reload += 1;
            break;
        }
    }
}

fn merge_into(peers: &mut [Doc], to: usize, from: usize) {
    if to == from {
        return;
    }
    let bytes = peers[from].save();
    peers[to].merge(&bytes).unwrap();
}

fn rounds() -> impl Strategy<Value = Vec<Round>> {
    prop::collection::vec((0usize..3, ops(), 0usize..3, prop::bool::weighted(0.25)), 1..14)
}

/// Runs generated sessions, `CASES` of them from seed `SEED`, and holds
/// every one to converging.
#[test]
fn peers_editing_at_once_converge() {
    use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
    let config = ProptestConfig { cases: cases("CASES", 48), ..ProptestConfig::default() };
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
