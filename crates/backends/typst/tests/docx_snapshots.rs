//! What lowering decides from each case's neighbours, against a snapshot in
//! `tests/snapshots/docx/`: each section's blocks in order, with every
//! paragraph's space above and below, line height, keeps, page break and
//! the drawings it holds, the same inside cells, text boxes, contents and
//! footnotes, then the notices' codes. `QUILLMARK_UPDATE_SNAPSHOTS=1` writes
//! the snapshots afresh.

mod common;
mod word;

use std::fmt::Write;
use std::path::PathBuf;

use quillmark_core::quill::Quill;
use quillmark_docx::model::{
    Block, Blocks, Document, DrawingContent, Inline, Line, Paragraph, Run, RunContent, SectionStart,
};

const PAGE: &str = "#set page(width: 300pt, height: 300pt, margin: 30pt)\n";
const CHROME: &str =
    "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"1\", header: [Head])\n";

fn run_text(r: &Run, out: &mut String) {
    for c in &r.content {
        match c {
            RunContent::Text(t) => out.push_str(t),
            RunContent::Tab => out.push('\t'),
            RunContent::Break(_) => out.push('\n'),
            _ => {}
        }
    }
}

/// A paragraph's text, cut to 40 characters.
fn text(p: &Paragraph) -> String {
    let mut out = String::new();
    for i in &p.content {
        match i {
            Inline::Run(r) => run_text(r, &mut out),
            Inline::Hyperlink(h) => h.runs.iter().for_each(|r| run_text(r, &mut out)),
            Inline::Field(f) => f.result.iter().for_each(|r| run_text(r, &mut out)),
            Inline::Control(_) => out.push_str("[control]"),
            Inline::BookmarkStart { .. } | Inline::BookmarkEnd { .. } => {}
        }
    }
    out.chars().take(40).collect()
}

fn paragraph(p: &Paragraph, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let sp = p.props.spacing.unwrap_or_default();
    let twips = |t: Option<i32>| t.map_or("-".to_string(), |t| t.to_string());
    let _ = write!(
        out,
        "{pad}P {:?} before={} after={} line={}",
        text(p),
        twips(sp.before),
        twips(sp.after),
        match sp.line {
            Some(Line::Exact(t)) => t.to_string(),
            Some(Line::AtLeast(t)) => format!("{t}+"),
            Some(Line::Auto(t)) => format!("auto {t}"),
            None => "-".into(),
        }
    );
    for (on, name) in [
        (p.props.keep_next, "keep_next"),
        (p.props.keep_lines, "keep_lines"),
        (p.props.page_break_before, "page_break"),
    ] {
        if on {
            let _ = write!(out, " {name}");
        }
    }
    out.push('\n');
    let drawings = p.content.iter().flat_map(|i| match i {
        Inline::Run(r) => r.content.as_slice(),
        _ => &[],
    });
    for c in drawings {
        let RunContent::Drawing(d) = c else { continue };
        match &d.content {
            DrawingContent::Picture(_) => {
                let _ = writeln!(out, "{pad}  picture");
            }
            DrawingContent::Shape(_) => {
                let _ = writeln!(out, "{pad}  shape");
            }
            DrawingContent::TextBox(b) => {
                let _ = writeln!(out, "{pad}  box");
                blocks(
                    b.body.blocks.iter().map(Some).chain([None]),
                    &b.body,
                    depth + 2,
                    out,
                );
            }
        }
    }
}

fn blocks<'a>(
    list: impl Iterator<Item = Option<&'a Block>>,
    body: &Blocks,
    depth: usize,
    out: &mut String,
) {
    let pad = "  ".repeat(depth);
    for b in list {
        match b {
            None => paragraph(&body.last, depth, out),
            Some(Block::Paragraph(p)) => paragraph(p, depth, out),
            Some(Block::Table(t)) => {
                let _ = writeln!(out, "{pad}T");
                for (r, row) in t.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        let _ = writeln!(out, "{pad}  cell {r},{c}");
                        let list = cell.body.blocks.iter().map(Some).chain([None]);
                        blocks(list, &cell.body, depth + 2, out);
                    }
                }
            }
            Some(Block::Toc(toc)) => {
                let _ = writeln!(out, "{pad}C");
                for p in &toc.entries {
                    paragraph(p, depth + 1, out);
                }
            }
        }
    }
}

fn sections(doc: &Document) -> String {
    let mut out = String::new();
    for (i, s) in doc.sections.iter().enumerate() {
        let start = match s.props.start {
            SectionStart::NextPage => "next page",
            SectionStart::OddPage => "odd page",
            SectionStart::EvenPage => "even page",
            SectionStart::Continuous => "continuous",
        };
        let _ = writeln!(out, "section {i}, {start}");
        let list = s.body.blocks.iter().map(Some).chain([None]);
        blocks(list, &s.body, 1, &mut out);
    }
    for (i, f) in doc.footnotes.iter().enumerate() {
        let _ = writeln!(out, "footnote {i}");
        let list = f.body.blocks.iter().map(Some).chain([None]);
        blocks(list, &f.body, 1, &mut out);
    }
    out
}

fn snapshot(quill: &Quill, data: &serde_json::Value) -> String {
    let layout =
        quillmark_typst::testing::layout(quill, data, common::test_date()).expect("layout");
    let (doc, notices) = quillmark_docx::lower::lower(&layout);
    let codes: Vec<&str> = notices.iter().map(|n| n.code.as_str()).collect();
    format!("{}notices: {}\n", sections(&doc), codes.join(" "))
}

fn check(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/docx")
        .join(format!("{name}.txt"));
    if std::env::var_os("QUILLMARK_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("create");
        std::fs::write(&path, actual).expect("write");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; QUILLMARK_UPDATE_SNAPSHOTS=1 writes it",
            path.display()
        )
    });
    if expected == actual {
        return;
    }
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.txt"));
    std::fs::write(&out, actual).expect("write");
    let line = expected
        .lines()
        .zip(actual.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
    let context = |s: &str| {
        s.lines()
            .skip(line.saturating_sub(3))
            .take(7)
            .collect::<Vec<_>>()
            .join("\n")
    };
    panic!(
        "{name} differs from line {}; this run is {}\n--- stored\n{}\n--- this run\n{}",
        line + 1,
        out.display(),
        context(&expected),
        context(actual)
    );
}

fn plate(name: &str, plate: &str) {
    check(name, &snapshot(&word::plain(plate), &serde_json::json!({})));
}

fn hosted(name: &str, plate: &str) {
    check(
        name,
        &snapshot(&word::hosted(plate), &serde_json::json!({})),
    );
}

fn fixture(name: &str) {
    let quill = quillmark::quill_from_path(quillmark_fixtures::quills_path(name)).expect("loads");
    assert_eq!(quill.backend_id(), "typst");
    let docs = [
        ("seed", Some(quill.seed_document())),
        ("example", quill.example_document()),
    ];
    for (which, doc) in docs {
        let Some(doc) = doc else { continue };
        let data = quill
            .compile_checked(&doc, common::test_date())
            .expect("data");
        check(&format!("fixture_{name}_{which}"), &snapshot(&quill, &data));
    }
}

#[test]
fn every_typst_fixture_has_a_case() {
    let covered = [
        "numbered_letter",
        "roster_form",
        "status_report",
        "table_demo",
        "taro",
        "usaf_memo",
    ];
    for name in quillmark_fixtures::quill_names() {
        let quill =
            quillmark::quill_from_path(quillmark_fixtures::quills_path(&name)).expect("loads");
        assert!(
            quill.backend_id() != "typst" || covered.contains(&name.as_str()),
            "{name} has no snapshot case"
        );
    }
}

#[test]
fn fixture_numbered_letter() {
    fixture("numbered_letter");
}

#[test]
fn fixture_roster_form() {
    fixture("roster_form");
}

#[test]
fn fixture_status_report() {
    fixture("status_report");
}

#[test]
fn fixture_table_demo() {
    fixture("table_demo");
}

#[test]
fn fixture_taro() {
    fixture("taro");
}

#[test]
fn fixture_usaf_memo() {
    fixture("usaf_memo");
}

macro_rules! plates {
    ($($kind:ident $name:ident: $plate:expr;)*) => {$(
        #[test]
        fn $name() {
            $kind(stringify!($name), &$plate);
        }
    )*};
}

plates! {
    plate sibling_blocks: format!(
        "{PAGE}#block[First paragraph in its own block.]\n\
         #block[Second paragraph in its own block.]\n\
         #block[A third, #underline[underlined] in its block.]\n"
    );
    plate aligned_box_in_a_grid_cell: format!(
        "{PAGE}#grid(columns: (1fr, 2in), [Left], align(right, box[18 November 2026]))\n\
         #align(center)[Centred alone]\n"
    );
    plate footer_of_two_lines:
        "#set page(width: 300pt, height: 300pt, margin: 40pt, numbering: \"1\", \
         footer: [#align(center)[#context counter(page).display()] #align(center)[MARKING]])\n\
         Body text.\n";
    plate heading_show_rule_breaks_the_page: format!(
        "{PAGE}#show heading.where(level: 1): it => {{ pagebreak(weak: true); it }}\n\
         = First chapter\nSome text.\n= Second chapter\nMore text.\n== A section\nEven more.\n"
    );
    plate roman_front_matter:
        "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"i\")\n\
         Front one.\n#pagebreak()\nFront two.\n\
         #set page(numbering: \"1\")\n#counter(page).update(1)\n\
         Body one.\n#pagebreak()\nBody two.\n";
    plate chapter_numbers:
        "#set page(width: 300pt, height: 300pt, margin: 30pt)\n\
         #let chapter(c) = (..n) => str(c) + \"-\" + str(n.pos().first())\n\
         #set page(numbering: chapter(1))\n\
         One A.\n#pagebreak()\nOne B.\n\
         #set page(numbering: chapter(2))\n#counter(page).update(1)\n\
         Two A.\n#pagebreak()\nTwo B.\n";
    plate appendix_number:
        "#set page(width: 300pt, height: 300pt, margin: 30pt, \
         numbering: (..n) => \"A-\" + str(n.pos().first()))\n\
         Appendix page.\n#pagebreak()\nAnother.\n";
    plate number_word_cannot_count:
        "#set page(width: 300pt, height: 300pt, margin: 30pt, numbering: \"α\")\n\
         One.\n#pagebreak()\nTwo.\n";
    plate fixed_page_alone: format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[Cover title]\n\
         #place(top + left, dx: 25pt, dy: 32pt)[Over it]\n"
    );
    plate two_fixed_pages: format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[First page]\n#pagebreak()\n\
         #place(top + left, dx: 20pt, dy: 30pt)[Second page]\n"
    );
    plate fixed_page_between_flowing_pages: format!(
        "{PAGE}Before.\n#pagebreak()\n#place(top + left, dx: 20pt, dy: 30pt)[Fixed]\n\
         #pagebreak()\nAfter.\n"
    );
    plate fixed_cover_before_new_margins: format!(
        "{PAGE}#place(top + left, dx: 20pt, dy: 30pt)[Cover]\n#set page(margin: 60pt)\nBody.\n"
    );
    plate custom_outline_entry:
        "#set page(width: 300pt, height: 300pt, margin: 30pt, \
         numbering: (..n) => \"1-\" + str(n.pos().first()))\n\
         #show outline.entry: it => block(link(it.element.location(), \
         [#it.body() #box(width: 1fr, repeat[.]) #it.page()]))\n\
         #outline()\n= Intro\nText.\n#pagebreak()\n= Second\n";
    plate footnotes_on_two_pages: format!(
        "{PAGE}First page text.#footnote[A note.]\n#pagebreak()\n\
         Second page text, no note.\n#pagebreak()\nThird page text.#footnote[Another note.]\n"
    );
    plate em_column: format!(
        "{PAGE}#set text(size: 10pt)\n#table(columns: (1fr, 1em + 90pt), [a], [b])\n"
    );
    plate ink_past_the_page: format!(
        "{PAGE}Text.\n#rotate(5deg, reflow: false, rect(width: 30in, height: 30in))\n"
    );
    plate space_above_a_table: format!(
        "{PAGE}Above.\n#table(columns: 1, inset: 8pt, [Cell])\n"
    );
    plate mirrored_content: format!(
        "{PAGE}Normal line.\n\n#scale(x: -100%)[Mirrored words here]\n\n\
         #scale(y: -100%)[Upside down words]\n"
    );
    plate replaced_list_markers: format!(
        "{PAGE}#set list(marker: box(width: 4pt, height: 4pt, fill: black))\n\
         - one\n- two\n\nBetween.\n\n- three\n"
    );
    plate internal_link: format!(
        "{PAGE}= Scope <scope>\n\nAs #link(<scope>)[the scope] says, and \
         #link(\"https://example.com\")[the site].\n"
    );
    plate script_link: format!(
        "{PAGE}Press #link(\" JavaScript:alert(1)\")[here] or #link(\"https://example.com\")[there].\n"
    );
    plate terms: format!(
        "{PAGE}/ Alpha: The first description.\n/ Beta: The second description.\n"
    );
    plate raw_block: format!("{PAGE}Before.\n\n```\nfirst line\nsecond line\n```\n");
    plate inline_raw: format!("{PAGE}Run `code` inline.\n");
    plate quote_and_caption: format!(
        "{PAGE}#quote(block: true, attribution: [Someone])[Quoted words.]\n\n\
         #figure(block[The body.], caption: [The caption.])\n\nAfter.\n"
    );
    hosted memo_heading_stack:
        "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
         #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #set par(leading: 0.5em, spacing: 0pt)\n\
         #set block(spacing: 0pt)\n\
         Dated line\n\
         #v(13.95pt)\n\
         #grid(columns: (auto, auto, 1fr), [MEMORANDUM FOR], \"  \", grid([123 OG/CC], [123 MXG/CC]))\n\
         #v(13.95pt)\n\
         #grid(columns: (auto, auto, 1fr), [FROM:], \"  \", [123 CS/SCX])\n\
         #v(13.95pt)\n\
         #grid(columns: (auto, auto, 1fr), [SUBJECT:], \"  \", [Annual Review])\n\
         #v(13.95pt)\n\
         Body text.\n";
    plate space_inside_grid_cells: format!(
        "{PAGE}Body text.\n#grid(columns: (1fr, 1fr), \
         [#v(14pt)\n#v(42pt)\nLeft name\n\nLeft title], \
         [Above the gap\n\n#v(42pt)\nRight name\n\nRight title])\n"
    );
    plate two_grids_a_gap_apart: format!(
        "{PAGE}#grid(columns: (auto, 1fr), [One], [Left])\n#v(20pt)\n\
         #grid(columns: (auto, 1fr), [Two], [Right])\nAfter.\n"
    );
    hosted two_grids_in_a_block:
        "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
         #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #set par(leading: 0.5em, spacing: 0pt)\n\
         #set block(spacing: 0pt)\n\
         Last line.\n\
         #v(40pt)\n\
         #block(breakable: false, sticky: true)[\
         #grid(columns: (auto, 1fr), [1st Ind, 123 OG/CC], align(right)[14 November 2026])\n\
         #v(13.95pt)\n\
         #grid(columns: (auto, auto, 1fr), [MEMORANDUM FOR], \"  \", [123 CS/SCX])]\n\
         #v(13.95pt)\n\
         Body text.\n";
    hosted content_sized_column_with_room: format!(
        "{PAGE}#set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #grid(columns: 2, column-gutter: 1fr, [123 OG/CC], [])\n"
    );
    hosted tabbed_label:
        "#set page(width: 612pt, height: 792pt, margin: 72pt)\n\
         #set text(font: \"TeX Gyre Termes\", size: 12pt)\n\
         #qm.hints.hint(\"layout\", form: \"tabbed\")[#grid(columns: (auto, auto, 1fr), \
         [SUBJECT:], [], [Annual Review of Every Office of Record in the Wing, with a \
         subject long enough to wrap under itself])]\n";
    plate superscript: format!(
        "{PAGE}The 1#super[st] Indorsement sits in a paragraph long enough to wrap onto a \
         second line.\n\nA plain paragraph that is also long enough to wrap onto a second line.\n"
    );
    plate inline_math: format!("{PAGE}Inline $x_i + y^2$ in a line.\n\nThe next paragraph.\n");
    plate display_math: format!(
        "{PAGE}Before.\n\n$ x = (a + b) / 2 $\n\nAfter the display.\n"
    );
    plate wrapping_equation: format!(
        "{PAGE}Before $a^2 + b^2 + c^2 + d^2 + e^2 + f^2 + g^2 + h^2 + i^2 + j^2 + k^2 + l^2$ \
         after.\n\nThe next paragraph, long enough that it wraps onto a second line too.\n"
    );
    plate indent_kinds: format!(
        "{PAGE}#h(2em)One line gap.\n\n\
         #h(2em)A gap that opens a paragraph long enough to wrap onto a second line.\n\n\
         #pad(left: 2em)[One line padded.]\n\n\
         #pad(left: 2em)[A padded paragraph that is long enough to wrap onto a second line.]\n\n\
         #set par(first-line-indent: 1.5em)\nPlain first.\n\nIndented one line.\n\n\
         Indented and long enough that it wraps onto a second line as well.\n"
    );
    plate heading_through_a_centred_block: format!(
        "{PAGE}#show heading: it => {{\n  set align(center)\n  \
         block(width: 100%, text(weight: \"bold\", it.body))\n}}\n\
         = Summary\n\nA body paragraph.\n"
    );
    plate numbered_paragraphs_across_pages: format!(
        "#let hints = qm.hints\n{PAGE}#set par(spacing: 6pt)\n\
         #hints.hint(\"list\", v: 1, levels: ((format: \"decimal\", suffix: \". \"),))[\n{}]\n",
        (1..=16)
            .map(|i| format!(
                "#hints.hint(\"label\", level: 1)[{i}.#h(4pt)]Paragraph {i}.\n\n#v(8pt)\n\n"
            ))
            .collect::<String>()
    );
    plate list_across_pages: format!(
        "{PAGE}Before the list.\n\n{}",
        (1..=30).map(|i| format!("- Item {i}\n")).collect::<String>()
    );
    plate place_after_a_full_table:
        "#set page(width: 300pt, height: 200pt, margin: 36pt)\n\
         #table(columns: 1fr, rows: 110pt, [A tall cell.])\n\
         #place(top + right)[Placed.]\n\
         Next paragraph after the place, long enough to need a few lines so that it cannot \
         fit at the foot of the first page.\n\nSecond paragraph.\n";
    plate section_with_no_footer: format!(
        "{PAGE}#set page(footer: [Foot A])\nFirst page.\n#pagebreak()\nSecond page.\n\
         #set page(footer: none)\nThird page, with no footer.\n"
    );
    plate running_header:
        "#set page(width: 300pt, height: 200pt, margin: 36pt, header: context {\n  \
         let hs = query(heading.where(level: 1).before(here()))\n  \
         if hs.len() > 0 [Running #hs.last().body] else [Front]\n})\n\
         = Alpha\n#lorem(70)\n\n= Beta\n#lorem(60)\n\n= Gamma\nShort.\n";
    plate hints_misused: format!(
        "#let hints = qm.hints\n{PAGE}\
         #hints.hint(\"label\", level: 1)[1.#h(4pt)]A label outside any list.\n\n\
         #hints.hint(\"layout\", form: \"tabbed\")[A paragraph, not a grid.]\n\n\
         #hints.hint(\"chrome\")[A paragraph, not a place.]\n"
    );
    plate hints_applied: format!(
        "#let hints = qm.hints\n{PAGE}\
         #hints.hint(\"layout\", form: \"tabbed\")[#grid(columns: (auto, 1fr), [FROM:], [Here])]\n\
         #hints.hint(\"chrome\")[#place(top + right)[Mark]]\n\
         #hints.hint(\"list\", v: 1, levels: ((format: \"decimal\", suffix: \". \"),))[\n\
         #hints.hint(\"label\", level: 1)[1.#h(4pt)]In a list.\n]\n"
    );
    plate form_fields: format!(
        "{PAGE}Name: #qm.form-field(\"name\", type: \"text\", value: \"Ann\")\n\n\
         Signed: #qm.form-field(\"sig\", type: \"signature\")\n"
    );
    plate columns_in_the_flow: format!(
        "{PAGE}Before.\n\n#columns(2)[Left column text.\n#colbreak()\nRight column text.]\n"
    );
    hosted right_to_left: format!(
        "{PAGE}Left to right.\n\n#text(font: \"Liberation Mono\", lang: \"he\")[\u{5e9}\u{5dc}\u{5d5}\u{5dd}]\n"
    );
    plate breaks_to_odd_and_even: format!(
        "{PAGE}One.\n#pagebreak(to: \"odd\")\nThree.\n#pagebreak(to: \"even\")\n\
         #table(columns: 2)[A][B]\n"
    );
    plate wrapped_paragraph_and_heading: format!(
        "{CHROME}= Title\nA paragraph with words enough that it wraps onto a second line of text \
         and then a third line here.\n\nLine one \\\nline two\n"
    );
    plate lists_tables_and_footnotes: format!(
        "{CHROME}- one\n- two\n\n+ first\n+ second\n\n\
         #table(columns: 2, table.header[H1][H2], [a], [b])\nNoted#footnote[The note.].\n"
    );
    plate page_setup_and_chrome: format!("{CHROME}one\n#pagebreak()\ntwo\n");
    plate highlight_rule_and_picture: format!(
        "{CHROME}Before #highlight(fill: blue)[blue] and #box(circle(radius: 3pt, fill: red)) \
         after.\n#line(length: 100%)\n"
    );
    plate table_and_first_page_float: format!(
        "{CHROME}A paragraph with words enough that it wraps onto a second line of text here.\n\n\
         One line.\n\n\
         #table(columns: (auto, 1fr), fill: (x, y) => if y == 0 {{ gray }}, stroke: 0.5pt + red,\n\
         [Key], [Value])\n\
         #place(bottom, float: true)[Reserved band]\n"
    );
    plate label_the_count_does_not_reach: format!(
        "#let hints = qm.hints\n{PAGE}\
         #hints.hint(\"list\", v: 1, levels: ((format: \"decimal\", suffix: \". \"),))[\n\
         #hints.hint(\"label\", level: 1)[3.#h(4pt)]Third, but first.\n]\n"
    );
    plate unknown_and_open_hints: format!(
        "{PAGE}#metadata((kind: \"__qm_hint__\", hint: \"mystery\"))<__qm_hint__>\
         Open #metadata((kind: \"__qm_hint__\", hint: \"role\", edge: \"open\", name: \"Lost\"))<__qm_hint__> here.\n\n\
         After it.\n"
    );
    plate outline: format!(
        "{CHROME}#outline()\n= Intro\nText.\n== Sub\n#pagebreak()\n= Second\n"
    );
    plate outline_with_prefixed_numbers:
        "#set page(width: 300pt, height: 300pt, margin: 30pt, \
         numbering: (..n) => \"A-\" + str(n.pos().first()))\n#outline()\n= Intro\n";
    plate keep_with_next: format!(
        "{PAGE}Before.\n\n#qm.hints.keep-with-next[Kept.]\n\nNext.\n"
    );
    plate keep_opening_a_cell: format!(
        "{PAGE}Before.\n\n#table(columns: 1, [#qm.hints.hint-point(\"keep\")Cell text.])\n\nAfter.\n"
    );
    plate keep_opening_a_footnote: format!(
        "{PAGE}Body text.#footnote[#qm.hints.hint-point(\"keep\")The note.]\n\nNext.\n"
    );
    plate keep_opening_a_block: format!(
        "{PAGE}Before.\n\n#block[#qm.hints.hint-point(\"keep\")Inside.]\n\nAfter.\n"
    );
    plate keep_after_a_table: format!(
        "{PAGE}#table(columns: 1, [Cell])\n#qm.hints.hint-point(\"keep\")\nAfter.\n"
    );
    plate table_in_a_block: format!(
        "{PAGE}Before.\n\n#block[#table(columns: 1, [Cell])]\n\nAfter.\n"
    );
    plate table_after_a_page_break: format!(
        "{PAGE}One.\n#pagebreak()\n#table(columns: 1, [Cell])\nAfter.\n"
    );
    plate page_break_in_an_align: format!(
        "{PAGE}One.\n#align(center)[Two.\n#pagebreak()\nThree.]\nFour.\n"
    );
    plate place_in_a_cell: format!(
        "{PAGE}Before.\n\n#table(columns: 1, [Text #place(top + right)[P]])\n\nAfter.\n"
    );
    plate place_on_a_later_page: format!(
        "{PAGE}One.\n#pagebreak()\n#place(top + left, dx: 20pt, dy: 30pt)[Placed]\nTwo.\n"
    );
    plate nested_lists: format!(
        "{PAGE}+ one\n  + one a\n  + one b\n+ two\n  - bullet\n\nAfter.\n"
    );
    plate table_with_spans: format!(
        "{PAGE}#table(columns: 3, table.cell(colspan: 2)[Wide], [C], table.cell(rowspan: 2)[Tall], \
         [B], [C2], [B3], [C3])\n"
    );
    plate shapes_in_flow: format!(
        "{PAGE}Before.\n\n#rect(width: 40pt, height: 20pt)\n\nAfter.\n#line(length: 50%)\n"
    );
}
