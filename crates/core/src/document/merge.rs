//! The three-way merge under a whole-document write's [`DocumentBase::Stored`]: the
//! writer's change and the change made since its read, merged by structure.
//!
//! An object merges by key, an array by item aligned as a revise aligns them,
//! a string by line, and a leaf both sides changed keeps its value now and is
//! reported as a [`MergeConflict`].
//!
//! [`DocumentBase::Stored`]: super::DocumentBase::Stored

use std::cell::{Cell, RefCell};

use indexmap::IndexMap;
use quillmark_content::lines::delta::merge_lines;
use serde_json::{Map, Value as JsonValue};

use super::align::Pairing;
use super::edit::resolve_field_write;
use super::payload::MetaKey;
use super::revise::{align_cards, align_items};
use super::{Card, Document, Parsed, Revising};
use crate::path::DocPath;
use crate::quill::{FieldSchema, QuillConfig};
use crate::value::QuillValue;

/// A place the writer's change and the change since its read both changed,
/// left as it stands now. Each side is the value there, `None` where that side
/// holds nothing: a field or item absent, a card removed. A body is its
/// markdown as a string, and a whole card an object of its fields beside
/// `$kind` and `$body`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MergeConflict {
    /// Where the place stands in the written document; for a place the
    /// write left out, where it would stand.
    pub path: DocPath,
    /// The value the writer read.
    pub base: Option<JsonValue>,
    /// The value now, which the write keeps.
    pub current: Option<JsonValue>,
    /// The value the writer proposed.
    pub proposed: Option<JsonValue>,
}

impl MergeConflict {
    pub(crate) fn new(
        path: DocPath,
        base: Option<&JsonValue>,
        current: Option<&JsonValue>,
        proposed: Option<&JsonValue>,
    ) -> Self {
        MergeConflict {
            path,
            base: base.cloned(),
            current: current.cloned(),
            proposed: proposed.cloned(),
        }
    }
}

/// `current` and `proposed`, two edits of `base`, merged at `at`. Each side is
/// `None` where it holds nothing. Where both changed one leaf, the merge keeps
/// `current`'s and pushes a conflict.
pub(crate) fn merge_value(
    base: Option<&JsonValue>,
    current: Option<&JsonValue>,
    proposed: Option<&JsonValue>,
    at: &DocPath,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<JsonValue> {
    if proposed == base {
        return current.cloned();
    }
    if current == base || current == proposed {
        return proposed.cloned();
    }
    let merged = match (base, current, proposed) {
        (Some(JsonValue::Object(b)), Some(JsonValue::Object(c)), Some(JsonValue::Object(p))) => {
            Some(JsonValue::Object(merge_object(b, c, p, at, conflicts)))
        }
        (Some(JsonValue::Array(b)), Some(JsonValue::Array(c)), Some(JsonValue::Array(p))) => {
            merge_array(b, c, p, at, conflicts).map(JsonValue::Array)
        }
        (Some(JsonValue::String(b)), Some(JsonValue::String(c)), Some(JsonValue::String(p))) => {
            merge_lines(b, c, p).map(JsonValue::String)
        }
        _ => None,
    };
    match merged {
        Some(value) => Some(value),
        None => {
            conflicts.push(MergeConflict::new(at.clone(), base, current, proposed));
            current.cloned()
        }
    }
}

/// The keys of `current` in its order, then those `proposed` adds, each merged.
fn merge_object(
    base: &Map<String, JsonValue>,
    current: &Map<String, JsonValue>,
    proposed: &Map<String, JsonValue>,
    at: &DocPath,
    conflicts: &mut Vec<MergeConflict>,
) -> Map<String, JsonValue> {
    let mut out = Map::new();
    for key in union_keys(current.keys(), proposed.keys()) {
        let merged = merge_value(
            base.get(key),
            current.get(key),
            proposed.get(key),
            &at.field(key),
            conflicts,
        );
        if let Some(value) = merged {
            out.insert(key.clone(), value);
        }
    }
    out
}

/// `current`'s keys in order, then those only `proposed` holds.
pub(crate) fn union_keys<'k>(
    current: impl Iterator<Item = &'k String>,
    proposed: impl Iterator<Item = &'k String>,
) -> Vec<&'k String> {
    let mut keys: Vec<&String> = current.collect();
    for key in proposed {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// `None` when either side reordered the items, which no item merge can
/// carry: the caller reports the array whole.
fn merge_array(
    base: &[JsonValue],
    current: &[JsonValue],
    proposed: &[JsonValue],
    at: &DocPath,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<Vec<JsonValue>> {
    let current_pairs = align_items(base, current);
    let proposed_pairs = align_items(base, proposed);
    let found = std::cell::RefCell::new(Vec::new());
    let merged = merge_items(
        Sides {
            base,
            current,
            proposed,
        },
        &current_pairs,
        &proposed_pairs,
        |a, b| a == b,
        |j, b, c, p| {
            merge_value(
                Some(b),
                Some(c),
                Some(p),
                &at.index(j),
                &mut found.borrow_mut(),
            )
            .expect("a merge of three present values holds one")
        },
        |j, b, c, p| {
            found
                .borrow_mut()
                .push(MergeConflict::new(at.index(j), Some(b), c, p));
        },
    )?;
    conflicts.append(&mut found.into_inner());
    Some(merged)
}

/// Three sequences, `current` and `proposed` two edits of `base`.
pub(crate) struct Sides<'s, T> {
    pub base: &'s [T],
    pub current: &'s [T],
    pub proposed: &'s [T],
}

/// The items of `sides` merged, given for each item of `current` and of
/// `proposed` the item of `base` it revises (`None` for an inserted one).
///
/// Each side's inserts land where that side placed them, `current`'s first,
/// and an insert `proposed` shares with `current` lands once. An item removed
/// on one side is removed where the other left it as `base` holds it; where
/// the other changed it, `lost` reports it and it stands as `current` has it.
/// `both` merges an item both sides hold; each callback takes the index the
/// item takes in the result. `None` when either side reordered the items.
pub(crate) fn merge_items<T: Clone>(
    sides: Sides<'_, T>,
    current_pairs: &[Option<usize>],
    proposed_pairs: &[Option<usize>],
    same: impl Fn(&T, &T) -> bool,
    mut both: impl FnMut(usize, &T, &T, &T) -> T,
    mut lost: impl FnMut(usize, &T, Option<&T>, Option<&T>),
) -> Option<Vec<T>> {
    let Sides {
        base,
        current,
        proposed,
    } = sides;
    let current_of = placements(base.len(), current_pairs)?;
    let proposed_of = placements(base.len(), proposed_pairs)?;
    let mut out: Vec<T> = Vec::new();
    for gap in 0..=base.len() {
        let ours = &current_of.inserts[gap];
        for &j in ours {
            out.push(current[j].clone());
        }
        for &k in &proposed_of.inserts[gap] {
            if !ours.iter().any(|&j| same(&current[j], &proposed[k])) {
                out.push(proposed[k].clone());
            }
        }
        let Some(b) = base.get(gap) else { break };
        let at = out.len();
        match (current_of.held[gap], proposed_of.held[gap]) {
            (Some(j), Some(k)) => out.push(both(at, b, &current[j], &proposed[k])),
            (Some(j), None) => {
                if !same(&current[j], b) {
                    lost(at, b, Some(&current[j]), None);
                    out.push(current[j].clone());
                }
            }
            (None, Some(k)) => {
                if !same(&proposed[k], b) {
                    lost(at, b, None, Some(&proposed[k]));
                }
            }
            (None, None) => {}
        }
    }
    Some(out)
}

/// Where one side's items stand against `base`'s.
struct Placements {
    /// For each base item, the side's item revising it.
    held: Vec<Option<usize>>,
    /// For each gap before a base item, and the one after the last, the
    /// side's items inserted there, in order.
    inserts: Vec<Vec<usize>>,
}

/// `None` when the side's paired items do not keep `base`'s order.
fn placements(base_len: usize, pairs: &[Option<usize>]) -> Option<Placements> {
    let mut held = vec![None; base_len];
    let mut inserts = vec![Vec::new(); base_len + 1];
    let mut gap = 0;
    for (j, pair) in pairs.iter().enumerate() {
        match *pair {
            Some(i) => {
                if i < gap {
                    return None;
                }
                held[i] = Some(j);
                gap = i + 1;
            }
            None => inserts[gap].push(j),
        }
    }
    Some(Placements { held, inserts })
}

/// `current` and `proposed`, two edits of the document `base`, each a parse of
/// its annotated markdown, merged card by card: the main card with the main
/// card, composable cards aligned as a revise aligns them, and in each card
/// its fields by [`merge_value`] and its body by line. A merged field that
/// `config` declares and that no longer conforms keeps its value now and is
/// reported whole.
///
/// `None` where no merge can stand: a side reordered the cards, both changed
/// a card's `$quill`, `$seed` or `$ext` apart, or a merged card overflows its
/// field count.
pub(crate) fn merge_documents(
    config: &QuillConfig,
    base: Revising,
    current: Revising,
    proposed: Revising,
) -> Option<(Revising, Vec<MergeConflict>)> {
    let (base_main, base_cards) = split(base);
    let (current_main, current_cards) = split(current);
    let (proposed_main, proposed_cards) = split(proposed);
    let mut conflicts = Vec::new();

    let main = merge_card(
        (&base_main, &current_main, &proposed_main),
        &DocPath::main(),
        Some(&config.main.fields),
        &mut conflicts,
    )?;

    let cards_of = |items: &[Held]| -> Vec<Card> {
        items.iter().map(|(card, _)| card.clone()).collect()
    };
    let current_pairs = align_cards(&cards_of(&base_cards), &cards_of(&current_cards));
    let proposed_pairs = align_cards(&cards_of(&base_cards), &cards_of(&proposed_cards));
    let found = RefCell::new(conflicts);
    let failed = Cell::new(false);
    let cards = merge_items(
        Sides {
            base: &base_cards,
            current: &current_cards,
            proposed: &proposed_cards,
        },
        &paired(&current_pairs),
        &paired(&proposed_pairs),
        same_card,
        |j, b, c, p| {
            let kind = c.0.kind();
            let schema = kind.and_then(|k| config.card_kind(k)).map(|s| &s.fields);
            let at = DocPath::card(kind, j);
            match merge_card((b, c, p), &at, schema, &mut found.borrow_mut()) {
                Some(merged) => merged,
                None => {
                    failed.set(true);
                    c.clone()
                }
            }
        },
        |j, b, c, p| {
            found.borrow_mut().push(MergeConflict {
                path: DocPath::card(b.0.kind(), j),
                base: Some(card_json(b)),
                current: c.map(card_json),
                proposed: p.map(card_json),
            });
        },
    )?;
    if failed.get() {
        return None;
    }

    let mut bodies = vec![main.1];
    let mut merged_cards = Vec::with_capacity(cards.len());
    for (card, body) in cards {
        merged_cards.push(card);
        bodies.push(body);
    }
    let revising = Revising {
        parsed: Parsed {
            document: Document {
                main: main.0,
                cards: merged_cards,
                mints: Default::default(),
            },
            warnings: Vec::new(),
        },
        bodies,
    };
    Some((revising, found.into_inner()))
}

/// A card beside its body's markdown.
type Held = (Card, String);

fn paired(pairs: &[Option<(usize, Pairing)>]) -> Vec<Option<usize>> {
    pairs.iter().map(|pair| pair.map(|(i, _)| i)).collect()
}

/// A parse's main card and composable cards, each with its body's markdown.
fn split(revising: Revising) -> (Held, Vec<Held>) {
    let Document { main, cards, .. } = revising.parsed.document;
    let mut bodies = revising.bodies.into_iter();
    let main_body = bodies.next().unwrap_or_default();
    let cards = cards
        .into_iter()
        .map(|card| (card, bodies.next().unwrap_or_default()))
        .collect();
    ((main, main_body), cards)
}

/// Two cards alike in all a merge reads: `$kind`, `$ext`, fields and body.
/// `$id` stays out, since an inserted card's is minted per side.
fn same_card(a: &Held, b: &Held) -> bool {
    a.0.kind() == b.0.kind()
        && a.0.ext() == b.0.ext()
        && a.0.payload().to_index_map() == b.0.payload().to_index_map()
        && a.1 == b.1
}

/// A whole card as a conflict carries it: its fields beside `$kind` and
/// `$body`.
fn card_json((card, body): &Held) -> JsonValue {
    let mut out = Map::new();
    if let Some(kind) = card.kind() {
        out.insert("$kind".into(), kind.into());
    }
    for (key, value) in card.payload().iter() {
        out.insert(key.clone(), value.as_json().clone());
    }
    out.insert("$body".into(), body.trim_matches('\n').into());
    JsonValue::Object(out)
}

/// One card both sides hold, merged onto `current`'s, so its `$id` and the
/// comments of the fields it keeps stand.
fn merge_card(
    (base, current, proposed): (&Held, &Held, &Held),
    at: &DocPath,
    schema: Option<&IndexMap<String, FieldSchema>>,
    conflicts: &mut Vec<MergeConflict>,
) -> Option<Held> {
    let mut card = current.0.clone();
    let field =
        |side: &Held, key: &str| side.0.payload().get(key).map(|v| v.as_json().clone());
    let keys: Vec<String> = union_keys(current.0.payload().keys(), proposed.0.payload().keys())
        .into_iter()
        .cloned()
        .collect();
    for key in keys {
        let (b, c, p) = (
            field(base, &key),
            field(current, &key),
            field(proposed, &key),
        );
        let mut found = Vec::new();
        let mut merged = merge_value(
            b.as_ref(),
            c.as_ref(),
            p.as_ref(),
            &at.field(&key),
            &mut found,
        );
        if merged != c && merged != p {
            let declared = schema.and_then(|fields| fields.get(&key));
            if let (Some(value), Some(declared)) = (&merged, declared) {
                let value = QuillValue::from_json(value.clone());
                if resolve_field_write(&key, &value, declared, at).is_err() {
                    found = vec![MergeConflict::new(
                        at.field(&key),
                        b.as_ref(),
                        c.as_ref(),
                        p.as_ref(),
                    )];
                    merged = c.clone();
                }
            }
        }
        conflicts.append(&mut found);
        if merged != c {
            match merged {
                Some(value) => {
                    card.payload_mut()
                        .insert(key, QuillValue::from_json(value))
                        .ok()?;
                }
                None => {
                    card.payload_mut().remove(&key);
                }
            }
        }
    }

    let meta = |side: &Held, key: MetaKey| {
        side.0.payload().meta(key).cloned().map(JsonValue::Object)
    };
    for key in [MetaKey::Ext, MetaKey::Seed] {
        let c = meta(current, key);
        let mut found = Vec::new();
        let merged = merge_value(
            meta(base, key).as_ref(),
            c.as_ref(),
            meta(proposed, key).as_ref(),
            at,
            &mut found,
        );
        if !found.is_empty() {
            return None;
        }
        if merged != c {
            match merged {
                Some(JsonValue::Object(value)) => card.payload_mut().set_meta(key, value),
                None => {
                    card.payload_mut().take_meta(key);
                }
                Some(_) => return None,
            }
        }
    }

    let quill = |side: &Held| side.0.quill().cloned();
    let (b, c, p) = (quill(base), quill(current), quill(proposed));
    if p != b && c != b && c != p {
        return None;
    }
    if let (true, Some(reference)) = (p != b, p) {
        card.payload_mut().set_quill(reference);
    }

    let (b, c, p) = (&base.1, &current.1, &proposed.1);
    let body = if p == b {
        c.clone()
    } else if c == b || c == p {
        p.clone()
    } else {
        merge_lines(b, c, p).unwrap_or_else(|| {
            let text = |body: &String| Some(JsonValue::from(body.trim_matches('\n')));
            conflicts.push(MergeConflict {
                path: at.body(),
                base: text(b),
                current: text(c),
                proposed: text(p),
            });
            c.clone()
        })
    };
    Some((card, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    fn merge(b: JsonValue, c: JsonValue, p: JsonValue) -> (JsonValue, Vec<String>) {
        let mut conflicts = Vec::new();
        let merged = merge_value(
            Some(&b),
            Some(&c),
            Some(&p),
            &DocPath::main().field("f"),
            &mut conflicts,
        )
        .unwrap();
        (
            merged,
            conflicts.iter().map(|c| c.path.to_string()).collect(),
        )
    }

    #[test]
    fn edits_to_two_rows_both_land() {
        let base = json!([{"title": "A", "dates": "2020"}, {"title": "B", "dates": "2021"}]);
        let current = json!([{"title": "A", "dates": "2020-22"}, {"title": "B", "dates": "2021"}]);
        let proposed =
            json!([{"title": "A", "dates": "2020"}, {"title": "B, revised", "dates": "2021"}]);
        assert_eq!(
            merge(base, current, proposed),
            (
                json!([{"title": "A", "dates": "2020-22"}, {"title": "B, revised", "dates": "2021"}]),
                vec![]
            )
        );
    }

    #[test]
    fn one_leaf_both_changed_keeps_the_value_now_and_reports_it() {
        let base = json!({"a": "1", "b": "x"});
        let current = json!({"a": "2", "b": "x"});
        let proposed = json!({"a": "3", "b": "y"});
        let mut conflicts = Vec::new();
        let merged = merge_value(
            Some(&base),
            Some(&current),
            Some(&proposed),
            &DocPath::main().field("f"),
            &mut conflicts,
        );
        assert_eq!(merged, Some(json!({"a": "2", "b": "y"})));
        assert_eq!(
            conflicts,
            [MergeConflict::new(
                DocPath::main().field("f").field("a"),
                Some(&json!("1")),
                Some(&json!("2")),
                Some(&json!("3")),
            )]
        );
    }

    #[test]
    fn a_row_inserted_ahead_of_an_edited_row_keeps_the_edit_on_its_row() {
        let base = json!(["alpha one", "beta two", "gamma three"]);
        let current = json!(["zero", "alpha one", "beta two", "gamma three"]);
        let proposed = json!(["alpha one", "beta two, edited", "gamma three", "delta four"]);
        assert_eq!(
            merge(base, current, proposed),
            (
                json!([
                    "zero",
                    "alpha one",
                    "beta two, edited",
                    "gamma three",
                    "delta four"
                ]),
                vec![]
            )
        );
    }

    #[test]
    fn a_row_removed_on_one_side_and_edited_on_the_other_stays_and_is_reported() {
        let base = json!([{"t": "keep"}, {"t": "gone"}]);
        let current = json!([{"t": "keep"}]);
        let proposed = json!([{"t": "keep"}, {"t": "gone, edited"}]);
        assert_eq!(
            merge(base, current, proposed),
            (json!([{"t": "keep"}]), vec!["main.f[1]".to_string()])
        );
    }

    #[test]
    fn a_reorder_against_an_edit_is_one_conflict_at_the_array() {
        let base = json!(["one", "two", "three"]);
        let current = json!(["three", "one", "two"]);
        let proposed = json!(["one", "two, edited", "three"]);
        assert_eq!(
            merge(base, current.clone(), proposed),
            (current, vec!["main.f".to_string()])
        );
    }

    #[test]
    fn a_row_both_sides_insert_lands_once() {
        let base = json!(["one"]);
        let both = json!(["one", "two"]);
        assert_eq!(merge(base, both.clone(), both.clone()), (both, vec![]));
    }

    fn value() -> impl Strategy<Value = JsonValue> {
        let leaf = prop_oneof![
            Just(JsonValue::Null),
            any::<bool>().prop_map(JsonValue::Bool),
            (0i64..5).prop_map(|n| json!(n)),
            "[a-c]{0,3}".prop_map(JsonValue::String),
        ];
        leaf.prop_recursive(3, 16, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(JsonValue::Array),
                prop::collection::btree_map("[a-c]", inner, 0..4)
                    .prop_map(|m| JsonValue::Object(m.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #[test]
        fn a_side_that_kept_the_base_yields_the_other(b in value(), x in value()) {
            prop_assert_eq!(merge(b.clone(), b.clone(), x.clone()), (x.clone(), vec![]));
            prop_assert_eq!(merge(b.clone(), x.clone(), b.clone()), (x, vec![]));
        }

        #[test]
        fn one_change_made_on_both_sides_lands_once(b in value(), x in value()) {
            prop_assert_eq!(merge(b, x.clone(), x.clone()), (x, vec![]));
        }

        #[test]
        fn a_conflict_names_a_place_both_sides_changed(b in value(), c in value(), p in value()) {
            let mut conflicts = Vec::new();
            let _ = merge_value(Some(&b), Some(&c), Some(&p), &DocPath::main().field("f"), &mut conflicts);
            for conflict in conflicts {
                prop_assert_ne!(&conflict.current, &conflict.base);
                prop_assert_ne!(&conflict.proposed, &conflict.base);
                prop_assert_ne!(&conflict.current, &conflict.proposed);
            }
        }
    }
}
