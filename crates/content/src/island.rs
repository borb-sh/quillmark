//! Island types: the dispatch authority over
//! [`Island::island_type`](crate::model::Island::island_type).

use crate::model::{Invariant, Loss, Mark};
use serde_json::Value;

/// The island types. Closed: a wire `type` outside this set is
/// [`ParseError::UnknownName`](crate::serial::ParseError::UnknownName).
///
/// Every emitter dispatches over the whole set: an island type wired into some
/// and not others projects the island away silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandType {
    /// `{header, rows, aligns}` with inline `{text, marks}` cells. Mark-carrying,
    /// shape-validated (one column count, `\n`-free cells).
    Table,
    /// `{url, alt}`. No cell model, no shape invariants.
    Image,
}

impl IslandType {
    /// Every known type, for a reader that needs the closed set whole.
    pub const ALL: &'static [IslandType] = &[IslandType::Table, IslandType::Image];

    /// The wire discriminator; `parse(k.as_str()) == Some(k)` for every variant.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Image => "image",
        }
    }

    /// Parse a wire discriminator; `parse(k.as_str()) == Some(k)` for every
    /// variant.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "table" => Some(Self::Table),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    /// The best markdown-projection loss class this type achieves: the ceiling
    /// the importer stamps at mint. A per-island [`Loss`] may sit below it, never
    /// above.
    pub fn default_loss(self) -> Loss {
        match self {
            Self::Table => Loss::Lossless,
            Self::Image => Loss::Lossless,
        }
    }

    /// Whether this type's markdown projection is a **block**: markup no
    /// paragraph line can hold, so its slot has to sit alone on its line. A
    /// pipe table is one; an image is inline (`![alt](url)`).
    pub fn block_only(self) -> bool {
        match self {
            Self::Table => true,
            Self::Image => false,
        }
    }

    /// This type's `(text, marks)` cells: the set that participates in mark
    /// normalization and cell-mark validation. Empty for a type with no cell
    /// model.
    pub fn cell_marks(self, props: &Value) -> Vec<(String, Vec<Mark>)> {
        match self {
            Self::Table => crate::serial::table_cells(props),
            Self::Image => Vec::new(),
        }
    }

    /// Repair this type's props to canonical shape in place; a no-op for a type
    /// with no shape invariants.
    pub fn normalize_props(self, props: &mut Value) {
        match self {
            Self::Table => crate::serial::normalize_table_props(props),
            Self::Image => {}
        }
    }

    /// This type's shape violation, if any (`None` for a well-formed or shape-free
    /// island): the validate-side twin of [`normalize_props`](Self::normalize_props),
    /// which guarantees this returns `None`.
    pub fn shape_error(self, props: &Value) -> Option<Invariant> {
        match self {
            Self::Table => crate::serial::table_shape_error(props),
            Self::Image => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_round_trip() {
        for k in [IslandType::Table, IslandType::Image] {
            assert_eq!(IslandType::parse(k.as_str()), Some(k));
        }
    }

    /// `normalize_props` guarantees `shape_error` returns `None`. An island
    /// op's props are untyped at the wire, so a scalar where an array belongs is
    /// a shape the two halves have to agree on.
    #[test]
    fn normalize_repairs_every_props_shape_validate_refuses() {
        for props in [
            serde_json::json!({"header": ["h"], "aligns": "bogus", "rows": [["a"]]}),
            serde_json::json!({"header": "bogus", "aligns": ["left"], "rows": [["a"]]}),
            serde_json::json!({"header": ["h"], "aligns": ["left"], "rows": ["bogus"]}),
            serde_json::json!({"header": ["h"], "aligns": 7, "rows": [[], null]}),
        ] {
            let mut props = props;
            IslandType::Table.normalize_props(&mut props);
            assert_eq!(
                IslandType::Table.shape_error(&props),
                None,
                "normalized props still refused: {props}"
            );
        }
    }
}
