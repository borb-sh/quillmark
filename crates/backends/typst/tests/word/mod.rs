//! A DOCX render read back as structure: paragraphs with their style,
//! alignment and breaks, sections with their numbering, and drawings with
//! the paragraph and page that anchor them.

#![allow(dead_code)]

use quillmark_core::backend::Backend;
use quillmark_core::types::{OutputFormat, RenderOptions};
use quillmark_typst::TypstBackend;
use serde_json::json;

use crate::common;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// One rendered DOCX: its parts and the codes of its warnings.
pub struct Word {
    pub parts: Vec<(String, Vec<u8>)>,
    pub codes: Vec<String>,
}

/// A paragraph that no other paragraph holds, in document order.
#[derive(Debug, Clone, Default)]
pub struct Par {
    pub text: String,
    pub style: Option<String>,
    pub jc: Option<String>,
    pub ind_left: Option<i64>,
    pub ind_first: Option<i64>,
    pub page_break_before: bool,
    pub keep_next: bool,
    /// Inside a table cell.
    pub in_table: bool,
    /// The paragraph carries a section break.
    pub ends_section: bool,
    /// Field instructions, trimmed, in order.
    pub fields: Vec<String>,
    /// The page it starts on, counting forced page and section breaks.
    pub page: usize,
    pub drawings: Vec<Drawing>,
    pub space_before: Option<i64>,
    pub space_after: Option<i64>,
    pub line: Option<i64>,
    /// The runs that hold text, outside drawings.
    pub runs: Vec<TextRun>,
}

#[derive(Debug, Clone, Default)]
pub struct TextRun {
    pub text: String,
    pub bold: bool,
    pub no_proof: bool,
    /// In half-points.
    pub size: Option<i64>,
    pub vert: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Drawing {
    pub anchored: bool,
    pub picture: bool,
    pub text: String,
    /// The text of each paragraph of a text box.
    pub lines: Vec<String>,
    /// The instructions of the fields a text box holds.
    pub fields: Vec<String>,
    pub x: Option<(String, i64)>,
    pub y: Option<(String, i64)>,
    pub z: Option<i64>,
    pub width: i64,
    pub height: i64,
    pub descr: Option<String>,
    /// The `w:position` of the run that holds it, in half-points.
    pub position: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub number_format: Option<String>,
    pub number_start: Option<i64>,
    pub title_page: bool,
    pub width: i64,
    pub height: i64,
    pub margin_top: i64,
    pub margin_left: i64,
    /// `(type, part)` of each header reference.
    pub headers: Vec<(String, String)>,
    pub footers: Vec<(String, String)>,
}

/// Renders `plate`, with the helper imported as `qm`, to DOCX.
pub fn render(plate: &str) -> Word {
    let plate = format!("#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}");
    render_quill(&common::quill_with_plate(&common::yaml(""), &plate))
}

/// [`render`] in the fixture quill whose fonts include TeX Gyre Termes.
pub fn render_hosted(plate: &str) -> Word {
    render_quill(&hosted(plate))
}

/// The fixture quill with fonts, holding `plate` with the helper as `qm`.
pub fn hosted(plate: &str) -> quillmark_core::quill::Quill {
    common::host_with_plate(&format!(
        "#import \"@local/quillmark-helper:0.2.0\" as qm\n{plate}"
    ))
}

/// The baseline Typst sets the first line holding `text` on, in points.
pub fn typst_baseline(quill: &quillmark_core::quill::Quill, text: &str) -> f64 {
    let doc =
        quillmark_typst::testing::layout(quill, &json!({}), common::test_date()).expect("layout");
    doc.nodes
        .iter()
        .flat_map(|n| &n.lines)
        .find(|l| {
            l.items.iter().any(|r| {
                matches!(&doc.item(*r).kind,
                    quillmark_docx::layout::ItemKind::Text(t) if t.text.trim() == text)
            })
        })
        .map(|l| l.baseline)
        .unwrap_or_else(|| panic!("no line {text:?}"))
}

fn render_quill(quill: &quillmark_core::quill::Quill) -> Word {
    let session = TypstBackend::default()
        .open(quill, &json!({}), common::test_date())
        .expect("open");
    let result = session
        .render(&RenderOptions::default().with_output_format(OutputFormat::Docx))
        .expect("render");
    Word {
        parts: unzip(&result.artifacts[0].bytes),
        codes: result
            .warnings
            .iter()
            .filter_map(|w| w.code.clone())
            .collect(),
    }
}

impl Word {
    pub fn xml(&self, name: &str) -> String {
        let (_, bytes) = self
            .parts
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no part {name}"));
        String::from_utf8(bytes.clone()).expect("utf-8")
    }

    pub fn has(&self, code: &str) -> bool {
        self.codes.iter().any(|c| c == code)
    }

    pub fn body(&self) -> Vec<Par> {
        paragraphs(&self.xml("word/document.xml"))
    }

    /// The body's paragraphs' texts, table cells included.
    pub fn texts(&self) -> Vec<String> {
        self.body().into_iter().map(|p| p.text).collect()
    }

    pub fn par(&self, text: &str) -> Par {
        let all = self.body();
        all.iter()
            .find(|p| p.text == text)
            .cloned()
            .unwrap_or_else(|| panic!("no paragraph {text:?} in {:?}", texts(&all)))
    }

    pub fn sections(&self) -> Vec<Section> {
        let rels = self.rels();
        let xml = self.xml("word/document.xml");
        let doc = roxmltree::Document::parse(&xml).expect("well-formed");
        doc.descendants()
            .filter(|n| n.has_tag_name((W, "sectPr")))
            .map(|s| {
                let child = |name: &str| s.children().find(|c| c.has_tag_name((W, name)));
                let num = |n: Option<roxmltree::Node>, a: &str| {
                    n.and_then(|n| n.attribute((W, a)))
                        .and_then(|v| v.parse::<i64>().ok())
                };
                let refs = |name: &str| {
                    s.children()
                        .filter(|c| c.has_tag_name((W, name)))
                        .map(|c| {
                            let id = c.attribute((R, "id")).unwrap_or_default();
                            let target = rels
                                .iter()
                                .find(|(i, _)| i == id)
                                .map(|(_, t)| format!("word/{t}"))
                                .unwrap_or_default();
                            (
                                c.attribute((W, "type")).unwrap_or_default().to_string(),
                                target,
                            )
                        })
                        .collect()
                };
                let pg = child("pgNumType");
                Section {
                    number_format: pg.and_then(|n| n.attribute((W, "fmt"))).map(str::to_string),
                    number_start: num(pg, "start"),
                    title_page: child("titlePg").is_some(),
                    width: num(child("pgSz"), "w").unwrap_or_default(),
                    height: num(child("pgSz"), "h").unwrap_or_default(),
                    margin_top: num(child("pgMar"), "top").unwrap_or_default(),
                    margin_left: num(child("pgMar"), "left").unwrap_or_default(),
                    headers: refs("headerReference"),
                    footers: refs("footerReference"),
                }
            })
            .collect()
    }

    /// The paragraphs of a header or footer part.
    pub fn part_paragraphs(&self, name: &str) -> Vec<Par> {
        paragraphs(&self.xml(name))
    }

    fn rels(&self) -> Vec<(String, String)> {
        let xml = self.xml("word/_rels/document.xml.rels");
        let doc = roxmltree::Document::parse(&xml).expect("well-formed");
        doc.descendants()
            .filter(|n| n.tag_name().name() == "Relationship")
            .map(|n| {
                (
                    n.attribute("Id").unwrap_or_default().to_string(),
                    n.attribute("Target").unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    /// Every drawing in the body with the page of the paragraph holding it.
    pub fn body_drawings(&self) -> Vec<(usize, Drawing)> {
        self.body()
            .into_iter()
            .flat_map(|p| p.drawings.into_iter().map(move |d| (p.page, d)))
            .collect()
    }

    /// Each paragraph's text and first baseline in points below the body's
    /// top, stacked as the export models Word on one page: a paragraph is its
    /// space before, its exact line with the baseline 0.8 down, and its space
    /// after; a row is its tallest cell, margins included, or its height.
    pub fn word_baselines(&self) -> Vec<(String, f64)> {
        let xml = self.xml("word/document.xml");
        let doc = roxmltree::Document::parse(&xml).expect("well-formed");
        let body = doc
            .descendants()
            .find(|n| n.has_tag_name((W, "body")))
            .expect("a body");
        let mut out = Vec::new();
        stack(body, 0.0, &mut out);
        out
    }

    /// [`Self::word_baselines`] of the first paragraph holding `text`.
    pub fn word_baseline(&self, text: &str) -> f64 {
        self.word_baselines()
            .into_iter()
            .find(|(t, _)| t == text)
            .map(|(_, y)| y)
            .unwrap_or_else(|| panic!("no paragraph {text:?}"))
    }
}

fn twips(n: Option<roxmltree::Node>, name: &str, attr: &str) -> f64 {
    n.and_then(|n| n.children().find(|c| c.has_tag_name((W, name))))
        .and_then(|c| c.attribute((W, attr)))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
        / 20.0
}

/// The blocks under `parent` stacked from `top`; returns their bottom.
fn stack(parent: roxmltree::Node, top: f64, out: &mut Vec<(String, f64)>) -> f64 {
    let mut y = top;
    for b in parent.children() {
        if b.has_tag_name((W, "p")) {
            let ppr = b.children().find(|c| c.has_tag_name((W, "pPr")));
            let line = twips(ppr, "spacing", "line");
            y += twips(ppr, "spacing", "before");
            let text: String = b
                .descendants()
                .filter(|n| {
                    n.has_tag_name((W, "t"))
                        && !n.ancestors().any(|a| a.has_tag_name((W, "drawing")))
                })
                .filter_map(|n| n.text())
                .collect();
            out.push((text, y + 0.8 * line));
            y += line + twips(ppr, "spacing", "after");
        } else if b.has_tag_name((W, "tbl")) {
            for tr in b.children().filter(|c| c.has_tag_name((W, "tr"))) {
                let trpr = tr.children().find(|c| c.has_tag_name((W, "trPr")));
                let mut height = twips(trpr, "trHeight", "val");
                for tc in tr.children().filter(|c| c.has_tag_name((W, "tc"))) {
                    let mar = tc
                        .children()
                        .find(|c| c.has_tag_name((W, "tcPr")))
                        .and_then(|p| p.children().find(|c| c.has_tag_name((W, "tcMar"))));
                    let inner = y + twips(mar, "top", "w");
                    let bottom = stack(tc, inner, out) + twips(mar, "bottom", "w");
                    height = height.max(bottom - y);
                }
                y += height;
            }
        }
    }
    y
}

pub fn texts(pars: &[Par]) -> Vec<String> {
    pars.iter().map(|p| p.text.clone()).collect()
}

/// The paragraphs of `xml` no other paragraph holds.
pub fn paragraphs(xml: &str) -> Vec<Par> {
    let doc = roxmltree::Document::parse(xml).expect("well-formed");
    let mut out: Vec<Par> = Vec::new();
    let mut page = 0;
    let mut break_next = false;
    for p in doc.descendants().filter(|n| {
        n.has_tag_name((W, "p")) && n.ancestors().skip(1).all(|a| !a.has_tag_name((W, "p")))
    }) {
        let ppr = p.children().find(|c| c.has_tag_name((W, "pPr")));
        let prop = |name: &str| ppr.and_then(|n| n.children().find(|c| c.has_tag_name((W, name))));
        let attr = |name: &str, a: &str| prop(name).and_then(|n| n.attribute((W, a)));
        let int = |name: &str, a: &str| attr(name, a).and_then(|v| v.parse::<i64>().ok());
        let in_table = p.ancestors().any(|a| a.has_tag_name((W, "tc")));
        let page_break_before = prop("pageBreakBefore").is_some();
        if !in_table && !out.is_empty() && (page_break_before || break_next) {
            page += 1;
        }
        break_next = false;
        let mut text = String::new();
        let mut fields = Vec::new();
        let mut drawings = Vec::new();
        for n in p.descendants() {
            if n.ancestors().any(|a| a.has_tag_name((W, "drawing")))
                && !n.has_tag_name((W, "drawing"))
            {
                continue;
            }
            if n.has_tag_name((W, "t")) {
                text.push_str(n.text().unwrap_or_default());
            } else if n.has_tag_name((W, "tab"))
                && !n.ancestors().any(|a| a.has_tag_name((W, "pPr")))
            {
                text.push('\t');
            } else if n.has_tag_name((W, "br")) {
                if n.attribute((W, "type")) == Some("page") {
                    break_next = true;
                } else {
                    text.push('\n');
                }
            } else if n.has_tag_name((W, "instrText")) {
                fields.push(n.text().unwrap_or_default().trim().to_string());
            } else if n.has_tag_name((W, "drawing")) {
                drawings.push(drawing(n));
            }
        }
        let runs = p
            .descendants()
            .filter(|r| {
                r.has_tag_name((W, "r")) && !r.ancestors().any(|a| a.has_tag_name((W, "drawing")))
            })
            .filter_map(|r| {
                let text: String = r
                    .children()
                    .filter(|t| t.has_tag_name((W, "t")))
                    .filter_map(|t| t.text())
                    .collect();
                let rpr = r.children().find(|c| c.has_tag_name((W, "rPr")));
                let has = |name: &str| {
                    rpr.is_some_and(|n| n.children().any(|c| c.has_tag_name((W, name))))
                };
                let val = |name: &str| {
                    rpr.and_then(|n| n.children().find(|c| c.has_tag_name((W, name))))
                        .and_then(|c| c.attribute((W, "val")))
                };
                (!text.is_empty()).then(|| TextRun {
                    text,
                    bold: has("b"),
                    no_proof: has("noProof"),
                    size: val("sz").and_then(|v| v.parse().ok()),
                    vert: val("vertAlign").map(str::to_string),
                })
            })
            .collect();
        let ends_section = prop("sectPr").is_some();
        if ends_section {
            break_next = true;
        }
        out.push(Par {
            text,
            style: attr("pStyle", "val").map(str::to_string),
            jc: attr("jc", "val").map(str::to_string),
            ind_left: int("ind", "left"),
            ind_first: int("ind", "firstLine"),
            page_break_before,
            keep_next: prop("keepNext").is_some(),
            in_table,
            ends_section,
            fields,
            page,
            drawings,
            space_before: int("spacing", "before"),
            space_after: int("spacing", "after"),
            line: int("spacing", "line"),
            runs,
        });
    }
    out
}

fn drawing(d: roxmltree::Node) -> Drawing {
    let anchor = d.children().find(|c| c.tag_name().name() == "anchor");
    let frame = anchor.or_else(|| d.children().find(|c| c.tag_name().name() == "inline"));
    let pos = |axis: &str| {
        let p = anchor?
            .children()
            .find(|c| c.tag_name().name() == format!("position{axis}"))?;
        let off = p
            .children()
            .find(|c| c.tag_name().name() == "posOffset")?
            .text()?
            .parse()
            .ok()?;
        Some((p.attribute("relativeFrom")?.to_string(), off))
    };
    let extent = frame.and_then(|f| f.children().find(|c| c.tag_name().name() == "extent"));
    let dim = |a: &str| {
        extent
            .and_then(|e| e.attribute(a))
            .and_then(|v| v.parse().ok())
            .unwrap_or_default()
    };
    Drawing {
        anchored: anchor.is_some(),
        picture: d.descendants().any(|n| n.tag_name().name() == "pic"),
        text: d
            .descendants()
            .filter(|n| n.has_tag_name((W, "t")))
            .filter_map(|n| n.text())
            .collect(),
        lines: d
            .descendants()
            .filter(|n| n.has_tag_name((W, "p")))
            .map(|p| {
                p.descendants()
                    .filter(|n| n.has_tag_name((W, "t")))
                    .filter_map(|n| n.text())
                    .collect()
            })
            .collect(),
        fields: d
            .descendants()
            .filter(|n| n.has_tag_name((W, "instrText")))
            .filter_map(|n| n.text())
            .map(|t| t.trim().to_string())
            .collect(),
        x: pos("H"),
        y: pos("V"),
        z: anchor
            .and_then(|a| a.attribute("relativeHeight"))
            .and_then(|v| v.parse().ok()),
        width: dim("cx"),
        height: dim("cy"),
        descr: d
            .descendants()
            .find(|n| n.tag_name().name() == "docPr")
            .and_then(|n| n.attribute("descr"))
            .map(str::to_string),
        position: d
            .ancestors()
            .find(|a| a.has_tag_name((W, "r")))
            .and_then(|r| r.children().find(|c| c.has_tag_name((W, "rPr"))))
            .and_then(|p| p.children().find(|c| c.has_tag_name((W, "position"))))
            .and_then(|p| p.attribute((W, "val")))
            .and_then(|v| v.parse().ok()),
    }
}

/// A package's parts by name, read through its central directory.
pub fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let end = bytes.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50, "an end of central directory");
    let count = u16_at(end + 10);
    let mut at = u32_at(end + 16);
    let mut out = Vec::new();
    for _ in 0..count {
        let method = u16_at(at + 10);
        let size = u32_at(at + 20);
        let name_len = u16_at(at + 28);
        let extra = u16_at(at + 30);
        let comment = u16_at(at + 32);
        let offset = u32_at(at + 42);
        let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
        let data_at = offset + 30 + u16_at(offset + 26) + u16_at(offset + 28);
        let raw = &bytes[data_at..data_at + size];
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(raw).expect("deflate"),
            m => panic!("compression method {m}"),
        };
        out.push((name, data));
        at += 46 + name_len + extra + comment;
    }
    out
}
