//! The markdown generator the import's property tests share: pieces of
//! markdown joined into blocks, tables of generated cells, and blocks placed in
//! a list item or a quote. Each test supplies its own leaves and wrappers.

use proptest::prelude::*;

use crate::import::ImportWarning;

/// Generated markdown, the words it must import and the construct each
/// opening tag in it reports.
#[derive(Debug, Clone, Default)]
pub(crate) struct Piece {
    pub(crate) md: String,
    pub(crate) words: Vec<String>,
    pub(crate) reported: Vec<String>,
    /// Ends in a pipe table, which takes in the lines after it as rows until a
    /// blank line.
    pub(crate) table: bool,
    /// The pipe tables a `qm-table` wrapper around it would hold.
    pub(crate) tables: usize,
    /// Holds a block other than a pipe table, a comment or a tag line: what
    /// keeps a `qm-table` wrapper around it from folding.
    pub(crate) other: bool,
    /// Ends in a closing tag line, whose block takes in the lines after it
    /// until a blank line.
    pub(crate) closes: bool,
    /// A `qm-cell` pair, which folds and reports nothing as a whole table
    /// cell.
    pub(crate) cell: bool,
}

impl Piece {
    pub(crate) fn text(md: String) -> Piece {
        Piece { md, ..Piece::default() }
    }

    pub(crate) fn word(w: String) -> Piece {
        Piece { md: w.clone(), words: vec![w], ..Piece::default() }
    }

    pub(crate) fn join(pieces: Vec<Piece>, sep: &str) -> Piece {
        let mut out = Piece::default();
        for (i, p) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.md.push_str(sep);
            }
            out.md.push_str(&p.md);
            out.words.extend(p.words);
            out.reported.extend(p.reported);
            out.tables += p.tables;
            out.other |= p.other;
        }
        out
    }
}

pub(crate) fn word() -> impl Strategy<Value = String> {
    "[a-z]{2,5}[0-9]"
}

/// A pipe table of 1 to 3 columns and 2 or 3 rows of `cell`s.
pub(crate) fn table<S: Strategy<Value = Piece>>(cell: fn() -> S) -> impl Strategy<Value = Piece> {
    (1usize..4).prop_flat_map(move |cols| {
        prop::collection::vec(prop::collection::vec(cell(), cols), 2..4).prop_map(move |rows| {
            let mut lines: Vec<Piece> = rows
                .into_iter()
                .map(|row| {
                    let cells = row.into_iter().map(|cell| match cell.cell {
                        true => Piece { reported: Vec::new(), ..cell },
                        false => cell,
                    });
                    let mut p = Piece::join(cells.collect(), " | ");
                    p.md = format!("| {} |", p.md);
                    p
                })
                .collect();
            lines.insert(1, Piece::text(format!("|{}", "---|".repeat(cols))));
            Piece { table: true, tables: 1, other: false, ..Piece::join(lines, "\n") }
        })
    })
}

/// Blocks in a list item or a quote.
pub(crate) fn contained(inner: impl Strategy<Value = Piece>) -> impl Strategy<Value = Piece> {
    (inner, any::<bool>()).prop_map(|(mut p, list)| {
        p.other = true;
        p.md = if list { prefixed(&p.md, "- ", "  ") } else { prefixed(&p.md, "> ", "> ") };
        p
    })
}

pub(crate) fn prefixed(md: &str, first: &str, rest: &str) -> String {
    md.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let p = if i == 0 { first } else { rest };
            if line.is_empty() { p.trim_end().to_string() } else { format!("{p}{line}") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each construct `reported` names with its count, sorted.
pub(crate) fn tally(reported: &[String]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for name in reported {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some((_, c)) => *c += 1,
            None => out.push((name.clone(), 1)),
        }
    }
    out.sort();
    out
}

/// Each warning's construct as `parse::dropped_construct` names it, with its
/// count, sorted.
pub(crate) fn counted(warnings: &[ImportWarning]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = warnings
        .iter()
        .map(|ImportWarning { construct, count }| (construct.to_string(), *count))
        .collect();
    out.sort();
    out
}
