//! Structural ids: blocks, containers, islands, slots, rows and columns.
//! Import, migration and a revise mint them deterministically, `{prefix}{n}`
//! past the highest number a field already holds under that prefix; a live
//! client mints its own.

use std::collections::{HashMap, HashSet};

/// The longest id, in scalar values.
pub const MAX_ID_LEN: usize = 128;

/// Whether `id` may name a structural part: 1 to [`MAX_ID_LEN`] scalar values,
/// none of them whitespace, a control, a bidi control or a slot.
pub fn is_id(id: &str) -> bool {
    let mut n = 0;
    for c in id.chars() {
        n += 1;
        if n > MAX_ID_LEN
            || c.is_whitespace()
            || c.is_control()
            || crate::lines::normalize::is_bidi_char(c)
            || c == crate::model::ISLAND_SLOT
        {
            return false;
        }
    }
    n > 0
}

/// Mints `{prefix}{n}` ids no held id takes.
#[derive(Debug, Clone, Default)]
pub struct Minter {
    held: HashSet<String>,
    next: HashMap<String, u64>,
}

impl Minter {
    pub fn new<I, S>(held: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Minter { held: held.into_iter().map(Into::into).collect(), next: HashMap::new() }
    }

    pub fn hold(&mut self, id: impl Into<String>) {
        let id = id.into();
        for (prefix, next) in &mut self.next {
            if let Some(n) = id.strip_prefix(prefix.as_str()).and_then(|n| n.parse::<u64>().ok()) {
                *next = (*next).max(n + 1);
            }
        }
        self.held.insert(id);
    }

    pub fn holds(&self, id: &str) -> bool {
        self.held.contains(id)
    }

    /// The first `{prefix}{n}` past the highest `n` held under `prefix`.
    pub fn mint(&mut self, prefix: &str) -> String {
        let held = &self.held;
        let next = self.next.entry(prefix.to_string()).or_insert_with(|| {
            held.iter()
                .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
                .max()
                .map_or(0, |n| n + 1)
        });
        let mut id = format!("{prefix}{next}");
        while held.contains(&id) {
            *next += 1;
            id = format!("{prefix}{next}");
        }
        *next += 1;
        self.held.insert(id.clone());
        id
    }

    /// `want` when it is an id no one holds, else a fresh one under `prefix`.
    pub fn keep_or_mint(&mut self, want: &str, prefix: &str) -> String {
        if is_id(want) && !self.held.contains(want) {
            self.hold(want);
            want.to_string()
        } else {
            self.mint(prefix)
        }
    }
}

/// The prefix each kind of part mints under.
pub mod prefix {
    pub const BLOCK: &str = "b";
    pub const LIST: &str = "l";
    pub const ITEM: &str = "i";
    pub const QUOTE: &str = "q";
    pub const ISLAND: &str = "isl-";
    pub const ROW: &str = "r";
    pub const COLUMN: &str = "k";
    pub const SLOT: &str = "s";
}
