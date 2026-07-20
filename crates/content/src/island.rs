//! Island types — the closed dispatch authority.
//!
//! [`Island::island_type`](crate::model::Island::island_type) stays an **open**
//! string on the wire: a genuinely-unknown type (one this build was never taught,
//! arriving via storage) round-trips opaque. [`KnownIslandType`] is that string's
//! *parse* into the closed set this build understands, and every site that
//! dispatches on a type matches [`KnownIslandType::parse`]. The `Some(k)` arm is
//! exhaustive over the enum, so **adding a variant is a compile error at every
//! dispatch site** — a known type wired into only some sites cannot reach the
//! `None` path, which is reserved for the open set.
//!
//! The behavior a type must supply lives here as one method per seam
//! ([`default_loss`](KnownIslandType::default_loss),
//! [`cell_marks`](KnownIslandType::cell_marks),
//! [`normalize_props`](KnownIslandType::normalize_props),
//! [`shape_error`](KnownIslandType::shape_error)); the two projections that can't
//! live in this crate — markdown emit ([`crate::export`]) and Typst emit (the
//! typst backend) — match on the enum where they are, so the exhaustiveness
//! guarantee still crosses the crate boundary. The table codec itself stays in
//! [`crate::serial`]; this module dispatches into it.

use crate::model::{Invariant, Island, Loss, Mark};
use serde_json::Value;

/// The island types this build understands. The wire discriminator is the open
/// string [`Island::island_type`](crate::model::Island::island_type); this is its
/// closed parse. Adding a variant forces every dispatch arm — here and in the two
/// emitters — to be supplied before the workspace compiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownIslandType {
    /// `{header, rows, aligns}` with inline `{text, marks}` cells. Mark-carrying,
    /// shape-validated (one column count, `\n`-free cells).
    Table,
    /// `{url, alt}`. No cell model, no shape invariants.
    Image,
}

impl KnownIslandType {
    /// The wire discriminator. `parse(k.as_str()) == Some(k)` for every variant
    /// (the [`known_types_round_trip`](self) test pins it).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Image => "image",
        }
    }

    /// Parse a wire discriminator into the closed set. `None` is the open-set
    /// escape hatch — a genuinely-unknown type, round-tripped opaque — never a
    /// known type wired into only some sites.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "table" => Some(Self::Table),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    /// The best markdown-projection loss class this type achieves — the ceiling
    /// the importer stamps at mint. A per-island [`Loss`] may sit *below* it (a
    /// table cell dropping an inline image's url degrades that instance) but never
    /// above; centralizing the ceiling here keeps the mint sites from disagreeing.
    pub fn default_loss(self) -> Loss {
        match self {
            Self::Table => Loss::Lossless,
            Self::Image => Loss::Lossless,
        }
    }

    /// This type's `(text, marks)` cells — the set that participates in mark
    /// normalization and cell-mark validation. Empty for a type with no cell
    /// model, so neither normalize nor validate can silently skip a new type
    /// (which would void the canonical-bytes guarantee for its cells).
    pub fn cell_marks(self, props: &Value) -> Vec<(String, Vec<Mark>)> {
        match self {
            Self::Table => crate::serial::table_cells(props),
            Self::Image => Vec::new(),
        }
    }

    /// Repair this type's props to canonical shape in place (a no-op for a type
    /// with no shape invariants) — the normalize-side dispatch.
    pub fn normalize_props(self, props: &mut Value) {
        match self {
            Self::Table => crate::serial::normalize_table_props(props),
            Self::Image => {}
        }
    }

    /// This type's shape violation, if any (`None` for a well-formed or shape-free
    /// island) — the validate-side twin of [`normalize_props`](Self::normalize_props),
    /// which guarantees this returns `None`.
    pub fn shape_error(self, props: &Value) -> Option<Invariant> {
        match self {
            Self::Table => crate::serial::table_shape_error(props),
            Self::Image => None,
        }
    }
}

/// Repair an island's props to its type's canonical shape in place (a no-op for
/// an unknown type — its opaque props are preserved verbatim). The normalize-side
/// island-type dispatch, called from [`Content::normalize`](crate::Content).
pub(crate) fn normalize_island_structure(island: &mut Island) {
    if let Some(k) = KnownIslandType::parse(&island.island_type) {
        k.normalize_props(&mut island.props);
    }
}

/// An island's `(text, marks)` cells for validation (empty for a type with no
/// cell model, and for an unknown type). The validate-side twin of
/// [`normalize_island_structure`].
pub(crate) fn island_cell_marks(island: &Island) -> Vec<(String, Vec<Mark>)> {
    match KnownIslandType::parse(&island.island_type) {
        Some(k) => k.cell_marks(&island.props),
        None => Vec::new(),
    }
}

/// An island's shape violation, if any (`None` for a well-formed, shape-free, or
/// unknown island). For a known type, [`normalize_island_structure`] guarantees
/// this returns `None`.
pub(crate) fn island_shape_error(island: &Island) -> Option<Invariant> {
    KnownIslandType::parse(&island.island_type).and_then(|k| k.shape_error(&island.props))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_round_trip() {
        for k in [KnownIslandType::Table, KnownIslandType::Image] {
            assert_eq!(KnownIslandType::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn unknown_type_parses_to_none() {
        assert_eq!(KnownIslandType::parse("figure"), None);
        assert_eq!(KnownIslandType::parse(""), None);
    }
}
