//! Structural ids: blocks, containers, islands, slots, rows and columns.
//! Import, migration and a revise mint them deterministically, `{prefix}{n}`
//! past the highest number a field already holds under that prefix, or the
//! lowest number no held id takes where the highest is `u64::MAX`; a live
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

/// The prefix of the reserved extension names.
pub const EXT_PREFIX: &str = "x-";

/// Whether `name` is a reserved extension name: [`EXT_PREFIX`] and at least
/// one more scalar value, an [id](is_id) as a whole. A block kind, mark type
/// or island type spelled so is one this build does not know, held as written.
pub fn is_ext_name(name: &str) -> bool {
    name.len() > EXT_PREFIX.len() && name.starts_with(EXT_PREFIX) && is_id(name)
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
                if let Some(past) = n.checked_add(1) {
                    *next = (*next).max(past);
                }
            }
        }
        self.held.insert(id);
    }

    pub fn holds(&self, id: &str) -> bool {
        self.held.contains(id)
    }

    /// The first `{prefix}{n}` past the highest `n` held under `prefix`, or,
    /// where that `n` is `u64::MAX`, the lowest `n` no held id takes.
    pub fn mint(&mut self, prefix: &str) -> String {
        let held = &self.held;
        let next = self.next.entry(prefix.to_string()).or_insert_with(|| {
            held.iter()
                .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
                .max()
                .and_then(|n| n.checked_add(1))
                .unwrap_or(0)
        });
        let mut id = format!("{prefix}{next}");
        while held.contains(&id) {
            *next = next.wrapping_add(1);
            id = format!("{prefix}{next}");
        }
        *next = next.wrapping_add(1);
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
