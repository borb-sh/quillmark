use std::collections::{BTreeSet, HashMap, HashSet};

use crate::layout::{
    ArtifactKind, BracketHint, Child, ItemKind, LayoutDoc, NodeId, NodeKind, Page, Parity,
    PointHint, VAlign,
};
use crate::model::{
    Blocks, HeaderRefs, Inline, NumFormat, Paragraph, Run, RunContent, RunProps, SectionStart,
    Twips,
};

use super::{Ctx, Lower, spacer, tw};

/// Gives each reference of `refs` that an earlier section made (`had`, for
/// the default and the first page) an empty part of `parts` when it has
/// none, since a Word section with no reference of a type shows the one
/// before it.
pub(super) fn fill_inherited(refs: &mut HeaderRefs, had: &mut [bool; 2], parts: &mut Vec<Blocks>) {
    for (slot, had) in [&mut refs.default, &mut refs.first].into_iter().zip(had) {
        if slot.is_none() && *had {
            parts.push(Blocks::from_paragraphs(vec![spacer(20)]));
            *slot = Some(parts.len() - 1);
        }
        *had |= slot.is_some();
    }
}

/// A printed page number as Word prints it: literal text around Word's
/// format of the page's count.
pub(super) struct Printed {
    pub(super) before: String,
    pub(super) format: NumFormat,
    pub(super) digits: String,
    pub(super) after: String,
}

/// How `page` prints its number, when that is literal text around one of
/// Word's formats of its count; the count's last occurrence is the number.
pub(super) fn printed_number(page: &Page) -> Option<Printed> {
    let text = page.number_text.as_deref()?.trim();
    let n = page.number;
    [
        NumFormat::Decimal,
        NumFormat::LowerRoman,
        NumFormat::UpperRoman,
        NumFormat::LowerLetter,
        NumFormat::UpperLetter,
    ]
    .into_iter()
    .filter_map(|format| {
        let digits = word_number(n, format)?;
        let joins: fn(char) -> bool = if format == NumFormat::Decimal {
            |c| c.is_ascii_digit()
        } else {
            char::is_alphabetic
        };
        let i = token_at(text, &digits, joins)?;
        Some(Printed {
            before: text[..i].to_string(),
            after: text[i + digits.len()..].to_string(),
            format,
            digits,
        })
    })
    .max_by_key(|p| text.len() - p.after.len())
}

/// `n` in one of Word's page number formats, where Word counts it the way
/// Typst does.
pub(super) fn word_number(n: u64, format: NumFormat) -> Option<String> {
    let letter = |base: u8| {
        (1..=26)
            .contains(&n)
            .then(|| char::from(base + n as u8 - 1).to_string())
    };
    match format {
        NumFormat::Decimal => Some(n.to_string()),
        NumFormat::LowerRoman => roman(n).map(|r| r.to_lowercase()),
        NumFormat::UpperRoman => roman(n),
        NumFormat::LowerLetter => letter(b'a'),
        NumFormat::UpperLetter => letter(b'A'),
        _ => None,
    }
}

pub(super) fn roman(mut n: u64) -> Option<String> {
    if !(1..4000).contains(&n) {
        return None;
    }
    let mut out = String::new();
    for (v, s) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    Some(out)
}

/// The last place `token` stands in `text` with no `joins` character on
/// either side.
pub(super) fn token_at(text: &str, token: &str, joins: fn(char) -> bool) -> Option<usize> {
    text.rmatch_indices(token).map(|(i, _)| i).find(|&i| {
        let before = text[..i].chars().next_back();
        let after = text[i + token.len()..].chars().next();
        !before.is_some_and(joins) && !after.is_some_and(joins)
    })
}

pub(super) fn is_roman(s: &str, upper: bool) -> bool {
    let set = if upper { "IVXLCDM" } else { "ivxlcdm" };
    !s.is_empty() && s.chars().all(|c| set.contains(c))
}

#[cfg(test)]
mod properties {
    use proptest::prelude::*;

    use super::printed_number;
    use crate::layout::{Page, PageSetup, Sides};
    use crate::model::NumFormat;

    fn page(number: u64, text: &str) -> Page {
        Page {
            size: [612.0, 792.0],
            setup: PageSetup {
                margins: Sides {
                    top: 72.0,
                    right: 72.0,
                    bottom: 72.0,
                    left: 72.0,
                },
                columns: 1,
                gutter: 0.0,
                flipped: false,
                header_ascent: 0.0,
                footer_descent: 0.0,
            },
            number,
            number_text: Some(text.into()),
            items: Vec::new(),
        }
    }

    /// The literal text before, the format and the literal text after.
    fn split(number: u64, text: &str) -> Option<(String, NumFormat, String)> {
        printed_number(&page(number, text)).map(|p| (p.before, p.format, p.after))
    }

    fn roman(mut n: u32) -> String {
        let table = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];
        let mut out = String::new();
        for (v, s) in table {
            while n >= v {
                out.push_str(s);
                n -= v;
            }
        }
        out
    }

    proptest! {
        #[test]
        fn a_roman_page_number_has_a_roman_format(n in 1u32..4000) {
            let r = roman(n);
            prop_assert_eq!(split(n as u64, &r), Some((String::new(), NumFormat::LowerRoman, String::new())));
            prop_assert_eq!(
                split(n as u64, &r.to_uppercase()),
                Some((String::new(), NumFormat::UpperRoman, String::new()))
            );
        }

        #[test]
        fn a_decimal_page_number_is_decimal(n in 0u64..100000) {
            prop_assert_eq!(split(n, &n.to_string()), Some((String::new(), NumFormat::Decimal, String::new())));
        }

        #[test]
        fn a_prefixed_page_number_keeps_its_prefix_as_text(n in 1u64..1000, chapter in 1u64..30) {
            prop_assert_eq!(
                split(n, &format!("{chapter}-{n}")),
                Some((format!("{chapter}-"), NumFormat::Decimal, String::new()))
            );
            prop_assert_eq!(
                split(n, &format!("A-{n}")),
                Some(("A-".to_string(), NumFormat::Decimal, String::new()))
            );
        }

        #[test]
        fn a_number_that_is_not_its_count_is_not_split(n in 1u64..1000) {
            prop_assert_eq!(split(n, &(n + 1).to_string()), None);
        }
    }
}

/// What a section holds once for all its pages: size, margins, columns and
/// orientation.
type SetupKey = (Twips, Twips, [Twips; 4], u32, bool);

/// What a page's header or footer shows, its page number as the literal text
/// and format it is printed in.
type Signature = Vec<(i64, String)>;

/// The pages as Word sections see them, read once before the walk: the runs
/// of pages each section holds and how each starts, and each page's chrome
/// and printed number.
pub(super) struct Pages<'a> {
    doc: &'a LayoutDoc,
    /// Per page, the text its number displays as, trimmed, when not empty.
    number: Vec<Option<&'a str>>,
    printed: Vec<Option<Printed>>,
    /// The places a `chrome` hint holds.
    places: Vec<NodeId>,
    /// Per page, its header and its footer chrome.
    chrome: Vec<[Vec<NodeId>; 2]>,
    signatures: Vec<[Signature; 2]>,
    /// The text around a page number that chrome prints on more than one
    /// number, which makes a chrome run holding the number a page number.
    numbered: HashSet<(String, String)>,
    pub(super) groups: Vec<Vec<usize>>,
    pub(super) starts: Vec<SectionStart>,
}

impl<'a> Pages<'a> {
    pub(super) fn new(doc: &'a LayoutDoc) -> Self {
        let places = chrome_places(doc);
        let chrome = (0..doc.pages.len())
            .map(|p| [false, true].map(|footer| chrome_on(doc, &places, p, footer)))
            .collect();
        let mut pages = Pages {
            doc,
            number: doc
                .pages
                .iter()
                .map(|p| {
                    p.number_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                })
                .collect(),
            printed: doc.pages.iter().map(printed_number).collect(),
            places: places.into_iter().map(|c| c.1).collect(),
            chrome,
            signatures: Vec::new(),
            numbered: HashSet::new(),
            groups: Vec::new(),
            starts: Vec::new(),
        };
        pages.numbered = pages.numbered_runs();
        pages.signatures = (0..doc.pages.len())
            .map(|p| [0, 1].map(|f| pages.signature(&pages.chrome[p][f], p)))
            .collect();
        pages.groups = pages.section_pages();
        pages.starts = (0..pages.groups.len())
            .map(|s| pages.section_start(s))
            .collect();
        pages
    }

    /// The header (`footer` false) or footer chrome on `page`: its artifacts
    /// and the places a `chrome` hint sends there.
    pub(super) fn chrome_of(&self, page: usize, footer: bool) -> &[NodeId] {
        &self.chrome[page][footer as usize]
    }

    fn signature_of(&self, page: usize, footer: bool) -> &Signature {
        &self.signatures[page][footer as usize]
    }

    /// How `page` prints its number, when Word can print it.
    pub(super) fn printed(&self, page: usize) -> Option<&Printed> {
        self.printed[page].as_ref()
    }

    pub(super) fn is_chrome_place(&self, n: NodeId) -> bool {
        self.places.contains(&n)
    }

    /// What `nodes`' chrome shows on `page`, with that run's x left out.
    fn signature(&self, nodes: &[NodeId], page: usize) -> Signature {
        let shown = self.printed(page).map_or_else(
            || "#".into(),
            |p| format!("{}#{:?}{}", p.before, p.format, p.after),
        );
        let mut out = Vec::new();
        for &n in nodes {
            for m in self.doc.subtree(n) {
                for c in &self.doc.nodes[m].children {
                    if let Child::Item(r) = c {
                        match &self.doc.item(*r).kind {
                            ItemKind::Text(t) if self.number_in(&t.text, page).is_some() => {
                                let (a, b) = self.number_in(&t.text, page).expect("a number");
                                out.push((
                                    (t.origin[1] * 10.0).round() as i64,
                                    format!("{}{shown}{}", &t.text[..a], &t.text[b..]),
                                ))
                            }
                            ItemKind::Text(t) => out.push((
                                (t.origin[1] * 10.0).round() as i64,
                                format!("{}@{}", t.text, (t.origin[0] * 10.0).round()),
                            )),
                            ItemKind::Image { rect, image, .. }
                            | ItemKind::Raster { rect, image, .. } => out.push((
                                (rect.y0 * 10.0).round() as i64,
                                format!("img{image}@{}", (rect.x0 * 10.0).round()),
                            )),
                            _ => {}
                        }
                    }
                }
            }
        }
        out
    }

    fn setup_key(&self, i: usize) -> SetupKey {
        let p = &self.doc.pages[i];
        let m = p.setup.margins;
        (
            tw(p.size[0]),
            tw(p.size[1]),
            [tw(m.top), tw(m.right), tw(m.bottom), tw(m.left)],
            p.setup.columns,
            p.setup.flipped,
        )
    }

    /// How section `s` starts: on the next odd or even page when Typst broke
    /// to one, on the same page when only its chrome or numbering changes
    /// and Typst broke the page where it was full, else on a new page.
    fn section_start(&self, s: usize) -> SectionStart {
        let groups = &self.groups;
        let (Some(&first), Some(&before)) = (
            groups[s].first(),
            s.checked_sub(1).and_then(|p| groups[p].last()),
        ) else {
            return SectionStart::NextPage;
        };
        match self.opening_break(first) {
            Some(Some(Parity::Odd)) => SectionStart::OddPage,
            Some(Some(Parity::Even)) => SectionStart::EvenPage,
            Some(None) => SectionStart::NextPage,
            None if self.setup_key(first) != self.setup_key(before) => SectionStart::NextPage,
            None => SectionStart::Continuous,
        }
    }

    /// The parity a page Typst broke to was asked for, when it was broken
    /// to: such a page carries a break mark before its first text outside
    /// chrome.
    fn opening_break(&self, page: usize) -> Option<Option<Parity>> {
        let chrome = |n: NodeId| {
            self.doc
                .ancestor(n, |k| matches!(k, NodeKind::Artifact { .. }))
                .is_some()
        };
        self.doc.pages[page]
            .items
            .iter()
            .find_map(|i| match i.kind {
                ItemKind::Mark(PointHint::Break { to, .. }) => Some(Some(to)),
                ItemKind::Text(_) if !chrome(i.node) => Some(None),
                _ => None,
            })
            .flatten()
    }

    /// Runs of consecutive pages one Word section holds: one page setup, one
    /// way of printing the page number, a count that runs on, chrome that
    /// is the same on every page but the first, and no break to an odd or
    /// even page after the first.
    fn section_pages(&self) -> Vec<Vec<usize>> {
        let doc = self.doc;
        let numbering = |i: usize| {
            self.number[i]?;
            Some(self.printed(i).map(|p| (&p.before, p.format, &p.after)))
        };
        let chrome = |i: usize| [false, true].map(|footer| self.signature_of(i, footer));
        let mut out: Vec<Vec<usize>> = Vec::new();
        for i in 0..doc.pages.len() {
            match out.last_mut() {
                Some(g)
                    if self.setup_key(g[0]) == self.setup_key(i)
                        && numbering(g[0]) == numbering(i)
                        && doc.pages[i].number == doc.pages[i - 1].number + 1
                        && g.get(1).is_none_or(|&d| chrome(d) == chrome(i))
                        && !matches!(self.opening_break(i), Some(Some(_))) =>
                {
                    g.push(i)
                }
                _ => out.push(vec![i]),
            }
        }
        if out.is_empty() {
            out.push(Vec::new());
        }
        out
    }

    /// The text around each page number chrome prints that recurs around two
    /// or more different numbers.
    fn numbered_runs(&self) -> HashSet<(String, String)> {
        let mut seen: HashMap<(String, String), BTreeSet<String>> = HashMap::new();
        for page in 0..self.doc.pages.len() {
            let Some(number) = self.number[page] else {
                continue;
            };
            for footer in [false, true] {
                for &n in self.chrome_of(page, footer) {
                    for m in self.doc.subtree(n) {
                        for c in &self.doc.nodes[m].children {
                            let Child::Item(r) = c else { continue };
                            let ItemKind::Text(t) = &self.doc.item(*r).kind else {
                                continue;
                            };
                            if let Some(i) = token_at(&t.text, number, char::is_alphanumeric) {
                                let around = (
                                    t.text[..i].to_string(),
                                    t.text[i + number.len()..].to_string(),
                                );
                                seen.entry(around).or_default().insert(number.to_string());
                            }
                        }
                    }
                }
            }
        }
        seen.into_iter()
            .filter(|(_, numbers)| numbers.len() >= 2)
            .map(|(around, _)| around)
            .collect()
    }

    /// Where `page`'s printed number stands in a chrome text run, when the
    /// run is the number alone or text that recurs around other numbers.
    pub(super) fn number_in(&self, text: &str, page: usize) -> Option<(usize, usize)> {
        let number = self.number[page]?;
        let i = token_at(text, number, char::is_alphanumeric)?;
        let end = i + number.len();
        let (before, after) = (&text[..i], &text[end..]);
        let alone = before.trim().is_empty() && after.trim().is_empty();
        (alone
            || self
                .numbered
                .contains(&(before.to_string(), after.to_string())))
        .then_some((i, end))
    }
}

/// The places a `chrome` hint holds, each with its page and whether it sits
/// at the foot, outside any other place.
fn chrome_places(doc: &LayoutDoc) -> Vec<(usize, NodeId, bool)> {
    let mut out = Vec::new();
    for n in 0..doc.nodes.len() {
        let NodeKind::Place {
            valign: Some(v), ..
        } = doc.nodes[n].kind
        else {
            continue;
        };
        let chrome = doc
            .ancestor(n, |k| matches!(k, NodeKind::Hint(BracketHint::Chrome)))
            .is_some();
        let nested = doc.nodes[n].parent.is_some_and(|p| {
            doc.ancestor(p, |k| matches!(k, NodeKind::Place { .. }))
                .is_some()
        });
        let page = doc.nodes[n].frames.first().map(|f| f.0);
        if let (true, false, Some(page)) = (chrome, nested, page) {
            out.push((page as usize, n, v == VAlign::Bottom));
        }
    }
    out
}

/// The header or footer artifacts on `page`, then the `places` there.
fn chrome_on(
    doc: &LayoutDoc,
    places: &[(usize, NodeId, bool)],
    page: usize,
    footer: bool,
) -> Vec<NodeId> {
    doc.nodes[0]
        .children
        .iter()
        .filter_map(|c| match c {
            Child::Node(n) => match &doc.nodes[*n].kind {
                NodeKind::Artifact { kind } if *kind != ArtifactKind::Other => {
                    let at = doc.nodes[*n].frames.first()?.0 as usize;
                    let low = matches!(kind, ArtifactKind::Footer | ArtifactKind::PageNumber);
                    (at == page && low == footer).then_some(*n)
                }
                _ => None,
            },
            Child::Item(_) => None,
        })
        .chain(
            places
                .iter()
                .filter(|c| c.0 == page && c.2 == footer)
                .map(|c| c.1),
        )
        .collect()
}

impl<'a> Lower<'a> {
    /// The header and footer parts of the section of `pages`.
    pub(super) fn chrome(
        &mut self,
        pages: &[usize],
        headers: &mut Vec<Blocks>,
        footers: &mut Vec<Blocks>,
    ) -> (HeaderRefs, HeaderRefs, bool) {
        let plan = self.pages;
        let mut title_page = false;
        let mut refs = [HeaderRefs::default(), HeaderRefs::default()];
        for (part, footer) in [(0, false), (1, true)] {
            let of = |p: usize| plan.chrome_of(p, footer);
            let sig = |p: usize| plan.signature_of(p, footer);
            let floats: Vec<NodeId> = if footer && pages.first() == Some(&0) {
                std::mem::take(&mut self.footer_floats)
            } else {
                Vec::new()
            };
            if floats.is_empty() && pages.iter().all(|&p| of(p).is_empty()) {
                continue;
            }
            let first = pages[0];
            let second = pages.get(1).copied();
            let differs = !floats.is_empty() || second.is_some_and(|s| sig(first) != sig(s));
            let mut make = |l: &mut Self, p: usize, floats: &[NodeId]| -> usize {
                let ctx = Ctx {
                    page_number: Some(p),
                    ..Ctx::default()
                };
                let mut drawings = Vec::new();
                for &n in of(p) {
                    drawings.extend(l.text_box(n, &ctx));
                }
                let mut line = 20;
                if let Some(band) = floats
                    .iter()
                    .filter_map(|&f| l.doc.nodes[f].frames.first().map(|r| r.1.y0))
                    .reduce(f64::min)
                {
                    let page = &l.doc.pages[p];
                    let distance =
                        (page.setup.margins.bottom - page.setup.footer_descent - 1.0).max(0.0);
                    line = tw((page.size[1] - distance - band).max(1.0));
                    for &f in floats {
                        drawings.extend(l.text_box(f, &Ctx::default()));
                    }
                }
                for &q in pages {
                    if q != p && sig(q) == sig(p) {
                        for &n in of(q) {
                            l.consume_subtree(n);
                        }
                    }
                }
                let para = Paragraph {
                    content: vec![Inline::Run(Run {
                        props: RunProps::default(),
                        content: drawings.into_iter().map(RunContent::Drawing).collect(),
                    })],
                    ..spacer(line)
                };
                let parts = if footer { &mut *footers } else { &mut *headers };
                parts.push(Blocks::from_paragraphs(vec![para]));
                parts.len() - 1
            };
            if differs {
                title_page = true;
                refs[part].first = Some(make(self, first, &floats));
                refs[part].default = match second {
                    Some(second) => Some(make(self, second, &[])),
                    None => None,
                };
            } else {
                refs[part].default = Some(make(self, first, &[]));
            }
        }
        if title_page {
            for r in &mut refs {
                if r.default.is_some() && r.first.is_none() {
                    r.first = r.default;
                }
            }
        }
        let [h, f] = refs;
        (h, f, title_page)
    }
}
