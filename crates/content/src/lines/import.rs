//! The markdown import lowered to the line form, for the readers and tests
//! still on it.

use crate::carry::Pin;
use crate::lines::model::{Content, ISLAND_SLOT, Line, LineKind, Lines};
pub use crate::markdown::import::{DroppedConstruct, ImportError, ImportWarning};

/// A markdown import: the content, and what the markdown spelled that it
/// could not carry, in order of each construct's first occurrence.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings; read `.warnings` or bind it"]
pub struct Imported {
    pub content: Lines,
    pub warnings: Vec<ImportWarning>,
}

/// [`crate::markdown::from_markdown`] lowered to the line form, for the
/// readers still on it.
pub fn from_markdown(markdown: &str) -> Result<Imported, ImportError> {
    let imported = crate::markdown::from_markdown(markdown)?;
    Ok(Imported {
        content: crate::migrate::to_lines(&imported.content),
        warnings: imported.warnings,
    })
}

/// [`crate::markdown::from_markdown_unresolved`] lowered to the line form.
pub fn from_markdown_unresolved(markdown: &str) -> Result<Imported, ImportError> {
    let imported = crate::markdown::from_markdown_unresolved(markdown)?;
    Ok(Imported {
        content: crate::migrate::to_lines(&imported.content),
        warnings: imported.warnings,
    })
}

/// A [`from_markdown_against`] import.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "carries the import's warnings and pins; read them or bind it"]
pub struct Against {
    /// The content, each token's island in it and no anchor.
    pub content: Lines,
    /// [`from_markdown`]'s warnings, with each tag and token reported in
    /// place of a token's drop.
    pub warnings: Vec<ImportWarning>,
    /// Each anchor a tag pins, in position order.
    pub pins: Vec<Pin>,
}

/// [`crate::diff::revise`]'s read of `markdown` against `stored`, lowered to
/// the line form.
pub fn from_markdown_against(markdown: &str, stored: &Content) -> Result<Against, ImportError> {
    let read =
        crate::markdown::import::walk_against(markdown, &crate::migrate::from_lines(stored))?;
    Ok(Against {
        content: crate::migrate::to_lines(&read.content),
        warnings: read.warnings,
        pins: read.pins,
    })
}

/// Import plain text (literal) into a [`Content`]: the literal-codec sibling of
/// [`from_markdown`]. Every character is content, never syntax: `*hi*` is four
/// literal chars, not emphasis. With [`crate::lines::export::to_plaintext`] it pins the
/// fixed point `to_plaintext(from_plaintext(s)) == s` for any `s` free of `\r`,
/// bidi controls, line separators (VT, FF, NEL, U+2028, U+2029), and
/// [`ISLAND_SLOT`].
///
/// Line structure is **derived, not stored**: a lone `\n` between two non-empty
/// segments is a within-paragraph break ([`Line::continues`] `true`); a blank
/// line is a paragraph boundary. The text is stored verbatim, so the round trip
/// is byte-exact however structure is later re-derived.
pub fn from_plaintext(s: &str) -> Lines {
    // Boundary cleanup so the content invariants hold; clean plaintext passes
    // through untouched.
    let text: String = s
        .chars()
        .filter(|&c| c != ISLAND_SLOT)
        .filter_map(crate::model::admit_char)
        .collect();
    // One line per `\n`-separated segment. A single streaming pass carries the
    // prior segment's non-emptiness, so line 0 is `false` and no intermediate
    // segment vector is allocated.
    let mut prev_nonempty = false;
    let lines = text
        .split('\n')
        .map(|seg| {
            let continues = prev_nonempty && !seg.is_empty();
            prev_nonempty = !seg.is_empty();
            Line::new(LineKind::Para).with_continues(continues)
        })
        .collect();
    Content {
        text,
        lines,
        marks: Vec::new(),
        islands: Vec::new(),
    }
    .into_lines()
}
