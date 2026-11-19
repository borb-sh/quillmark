//! The writer's package: the parts Word requires, relationships that resolve,
//! children in schema order, and bytes that depend on the model alone.

use quillmark_docx::model::*;
use quillmark_docx::write_docx;
use zip::unzip;

mod zip;

fn run(text: &str) -> Run {
    Run {
        props: RunProps::default(),
        content: vec![RunContent::Text(text.into())],
    }
}

fn par(text: &str) -> Paragraph {
    Paragraph {
        props: ParProps::default(),
        content: vec![Inline::Run(run(text))],
    }
}

fn every_par_prop() -> ParProps {
    ParProps {
        style: Some("Body".into()),
        keep_next: true,
        keep_lines: true,
        page_break_before: true,
        widow_control: Some(true),
        numbering: Some(NumRef { level: 1, num: 1 }),
        tabs: vec![TabStop {
            kind: TabKind::Right,
            pos: 9360,
            leader: TabLeader::Dot,
        }],
        spacing: Some(Spacing {
            before: Some(240),
            after: Some(0),
            line: Some(Line::Exact(279)),
        }),
        indent: Some(Indent {
            left: Some(720),
            right: Some(0),
            first: Some(FirstLine::Hanging(360)),
        }),
        align: Some(Align::Both),
        outline_level: Some(0),
        mark: Some(RunProps {
            size: Some(24),
            ..RunProps::default()
        }),
    }
}

fn every_run_prop() -> RunProps {
    RunProps {
        style: Some("Strong".into()),
        font: Some("Times New Roman".into()),
        bold: Some(true),
        italic: Some(false),
        strike: true,
        no_proof: true,
        color: Some("FF0000".into()),
        spacing: Some(-8),
        scale: Some(96),
        position: Some(-3),
        size: Some(21),
        underline: true,
        shade: Some("0000FF".into()),
        vert: Some(VertAlign::Superscript),
        lang: Some("en-US".into()),
    }
}

fn anchored(content: DrawingContent) -> Drawing {
    Drawing {
        placement: Placement::Anchor(Anchor {
            x: 914400,
            y: 457200,
            behind_text: false,
            z: 3,
            wrap: Wrap::None,
        }),
        width: 1270000,
        height: 254000,
        content,
        descr: Some("seal".into()),
    }
}

/// One of every construct the model holds, across two sections.
fn kitchen() -> Document {
    let mut body = vec![
        Block::Paragraph(Paragraph {
            props: every_par_prop(),
            content: vec![
                Inline::BookmarkStart {
                    name: "_top".into(),
                    id: 0,
                },
                Inline::Run(Run {
                    props: every_run_prop(),
                    content: vec![
                        RunContent::Text("  spaced  ".into()),
                        RunContent::Tab,
                        RunContent::Break(BreakKind::Line),
                        RunContent::FootnoteRef(0),
                    ],
                }),
                Inline::BookmarkEnd { id: 0 },
                Inline::Hyperlink(Hyperlink {
                    target: LinkTarget::Url("https://example.com/?a=1&b=<2>".into()),
                    runs: vec![run("out")],
                }),
                Inline::Hyperlink(Hyperlink {
                    target: LinkTarget::Bookmark("_top".into()),
                    runs: vec![run("in")],
                }),
                Inline::Control(Control {
                    tag: "main.date".into(),
                    title: "Date".into(),
                    kind: ControlKind::Date,
                    content: vec![Inline::Run(run("Date"))],
                    placeholder: true,
                }),
            ],
        }),
        Block::Table(Table {
            props: TableProps {
                style: None,
                width: Some(9360),
                indent: Some(0),
                borders: Some(Borders {
                    top: Some(Border {
                        size: 4,
                        color: "808080".into(),
                    }),
                    ..Borders::default()
                }),
                fixed: true,
                cell_margins: Some(CellMargins {
                    top: 0,
                    left: 108,
                    bottom: 0,
                    right: 108,
                }),
            },
            grid: vec![4680, 4680],
            rows: vec![Row {
                props: RowProps {
                    cant_split: true,
                    header: true,
                    min_height: Some(300),
                },
                cells: vec![
                    Cell {
                        props: CellProps {
                            width: Some(4680),
                            span: 1,
                            merge: Some(VMerge::Restart),
                            borders: Some(Borders::default()),
                            fill: Some("D9D9D9".into()),
                            no_wrap: true,
                            margins: Some(CellMargins::default()),
                            valign: Some(VAlign::Center),
                        },
                        body: Blocks::from_paragraphs(vec![par("a")]),
                    },
                    Cell {
                        props: CellProps {
                            span: 1,
                            ..CellProps::default()
                        },
                        body: Blocks::from_paragraphs(vec![]),
                    },
                ],
            }],
        }),
        Block::Toc(Toc {
            instruction: "TOC \\o \"1-3\" \\h".into(),
            locked: true,
            entries: vec![par("Intro\t1"), par("End\t2")],
        }),
    ];
    body.push(Block::Paragraph(Paragraph {
        props: ParProps::default(),
        content: vec![
            Inline::Run(Run {
                props: RunProps::default(),
                content: vec![
                    RunContent::Drawing(anchored(DrawingContent::Picture(0))),
                    RunContent::Drawing(anchored(DrawingContent::TextBox(TextBox {
                        body: Blocks::from_paragraphs(vec![par("CUI")]),
                        wrap: true,
                    }))),
                    RunContent::Drawing(Drawing {
                        placement: Placement::Inline,
                        ..anchored(DrawingContent::Shape(Shape {
                            geometry: ShapeGeometry::Line { rising: true },
                            fill: None,
                            stroke: Some(Stroke {
                                width: 6350,
                                color: "000000".into(),
                            }),
                        }))
                    }),
                ],
            }),
            Inline::Field(Field {
                instruction: "PAGE".into(),
                props: RunProps::default(),
                result: vec![run("1")],
                locked: false,
            }),
        ],
    }));
    let first = Section {
        body: Blocks::from_blocks(body),
        props: SectionProps {
            header: HeaderRefs {
                default: Some(0),
                first: Some(1),
                even: None,
            },
            footer: HeaderRefs {
                default: Some(0),
                first: None,
                even: None,
            },
            title_page: true,
            page_numbers: Some(PageNumbers {
                format: NumFormat::LowerRoman,
                start: Some(1),
            }),
            ..SectionProps::default()
        },
    };
    let second = Section {
        body: Blocks::from_paragraphs(vec![par("landscape")]),
        props: SectionProps {
            start: SectionStart::OddPage,
            landscape: true,
            page_width: 15840,
            page_height: 12240,
            columns: Some(Columns {
                count: 2,
                space: 360,
            }),
            ..SectionProps::default()
        },
    };
    let header_with_picture = Blocks::from_paragraphs(vec![Paragraph {
        props: ParProps::default(),
        content: vec![Inline::Run(Run {
            props: RunProps::default(),
            content: vec![RunContent::Drawing(anchored(DrawingContent::Picture(0)))],
        })],
    }]);
    Document {
        sections: vec![first, second],
        headers: vec![Blocks::from_paragraphs(vec![par("h")]), header_with_picture],
        footers: vec![Blocks::from_paragraphs(vec![par("f")])],
        footnotes: vec![Footnote {
            body: Blocks::from_paragraphs(vec![Paragraph {
                props: ParProps::default(),
                content: vec![Inline::Run(Run {
                    props: RunProps::default(),
                    content: vec![RunContent::FootnoteMark, RunContent::Text("note".into())],
                })],
            }]),
        }],
        styles: Styles {
            default_run: RunProps {
                size: Some(24),
                ..RunProps::default()
            },
            default_par: ParProps {
                widow_control: Some(true),
                ..ParProps::default()
            },
            styles: vec![
                Style::paragraph("Normal", "Normal", true),
                Style {
                    based_on: Some("Normal".into()),
                    next: Some("Normal".into()),
                    quick: true,
                    par: every_par_prop(),
                    run: every_run_prop(),
                    ..Style::paragraph("Body", "Memo Body", false)
                },
            ],
        },
        numbering: Numbering {
            abstracts: vec![AbstractNum {
                id: 0,
                nsid: "0A1B2C3D".into(),
                levels: vec![Level {
                    start: 1,
                    format: NumFormat::Decimal,
                    style: Some("Body".into()),
                    suffix: LevelSuffix::Nothing,
                    text: "%1.  ".into(),
                    align: Align::Start,
                    indent: Indent {
                        left: Some(0),
                        right: None,
                        first: Some(FirstLine::Indent(0)),
                    },
                    tabs: vec![],
                    run: RunProps::default(),
                }],
            }],
            nums: vec![Num {
                id: 1,
                abstract_id: 0,
                starts: vec![(0, 1)],
            }],
        },
        fonts: vec![FontDecl {
            name: "Times New Roman".into(),
            alt_name: None,
            family: FontFamily::Roman,
            pitch_fixed: false,
        }],
        media: vec![Media {
            kind: MediaKind::Png,
            bytes: vec![0x89, b'P', b'N', b'G'],
        }],
        settings: Settings::default(),
        info: Info {
            title: Some("Kitchen & sink".into()),
            language: Some("en-US".into()),
        },
    }
}

struct Package(Vec<(String, Vec<u8>)>);

impl Package {
    fn of(doc: &Document) -> Self {
        Package(unzip(&write_docx(doc)).expect("the writer's zip reads back"))
    }

    fn names(&self) -> Vec<&str> {
        self.0.iter().map(|(n, _)| n.as_str()).collect()
    }

    fn xml(&self, name: &str) -> String {
        let (_, bytes) = self
            .0
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name}"));
        String::from_utf8(bytes.clone()).expect("utf-8")
    }
}

fn local<'a>(n: roxmltree::Node<'a, 'a>) -> &'a str {
    n.tag_name().name()
}

/// `parent`'s element children, by local name, appear in `order`'s order.
fn in_order(parent: roxmltree::Node, order: &[&str]) {
    let names: Vec<&str> = parent
        .children()
        .filter(|c| c.is_element())
        .map(local)
        .collect();
    let ranks: Vec<usize> = names
        .iter()
        .map(|n| {
            order
                .iter()
                .position(|o| o == n)
                .unwrap_or_else(|| panic!("<{n}> is not a child of <{}>", local(parent)))
        })
        .collect();
    assert!(
        ranks.is_sorted(),
        "<{}> children out of schema order: {names:?}",
        local(parent)
    );
}

const PPR: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "widowControl",
    "numPr",
    "tabs",
    "spacing",
    "ind",
    "jc",
    "outlineLvl",
    "rPr",
    "sectPr",
];
const RPR: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "i",
    "strike",
    "noProof",
    "color",
    "spacing",
    "w",
    "position",
    "sz",
    "szCs",
    "u",
    "shd",
    "vertAlign",
    "lang",
];
const SECTPR: &[&str] = &[
    "headerReference",
    "footerReference",
    "type",
    "pgSz",
    "pgMar",
    "pgNumType",
    "cols",
    "titlePg",
];
const TBLPR: &[&str] = &[
    "tblStyle",
    "tblW",
    "tblInd",
    "tblBorders",
    "tblLayout",
    "tblCellMar",
];
const TCPR: &[&str] = &[
    "tcW",
    "gridSpan",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "vAlign",
];
const BORDERS: &[&str] = &["top", "left", "bottom", "right", "insideH", "insideV"];
const LVL: &[&str] = &[
    "start", "numFmt", "pStyle", "suff", "lvlText", "lvlJc", "pPr", "rPr",
];
const STYLE: &[&str] = &[
    "name",
    "basedOn",
    "next",
    "uiPriority",
    "semiHidden",
    "qFormat",
    "pPr",
    "rPr",
];
const ANCHOR: &[&str] = &[
    "simplePos",
    "positionH",
    "positionV",
    "extent",
    "effectExtent",
    "wrapNone",
    "wrapTopAndBottom",
    "wrapSquare",
    "docPr",
    "cNvGraphicFramePr",
    "graphic",
];
const SDTPR: &[&str] = &["alias", "tag", "id", "showingPlcHdr", "richText", "date"];

#[test]
fn every_property_struct_writes_its_children_in_schema_order() {
    let pkg = Package::of(&kitchen());
    for part in [
        "word/document.xml",
        "word/styles.xml",
        "word/numbering.xml",
        "word/header2.xml",
    ] {
        let text = pkg.xml(part);
        let doc = roxmltree::Document::parse(&text).expect(part);
        for node in doc.descendants().filter(|n| n.is_element()) {
            match local(node) {
                "pPr" => in_order(node, PPR),
                "rPr" => in_order(node, RPR),
                "sectPr" => in_order(node, SECTPR),
                "tblPr" => in_order(node, TBLPR),
                "tcPr" => in_order(node, TCPR),
                "tblBorders" | "tcBorders" => in_order(node, BORDERS),
                "lvl" => in_order(node, LVL),
                "style" => in_order(node, STYLE),
                "anchor" => in_order(node, ANCHOR),
                "sdtPr" => in_order(node, SDTPR),
                _ => {}
            }
        }
    }
}

#[test]
fn a_package_holds_every_part_its_relationships_and_content_types_name() {
    let pkg = Package::of(&kitchen());
    let names = pkg.names();
    assert_eq!(
        names[0], "[Content_Types].xml",
        "Word reads the content types first"
    );
    for required in [
        "_rels/.rels",
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
        "word/settings.xml",
        "word/fontTable.xml",
        "word/numbering.xml",
        "word/footnotes.xml",
        "docProps/core.xml",
    ] {
        assert!(names.contains(&required), "missing {required}");
    }

    let types = pkg.xml("[Content_Types].xml");
    let types = roxmltree::Document::parse(&types).unwrap();
    let overridden: Vec<&str> = types
        .descendants()
        .filter(|n| local(*n) == "Override")
        .map(|n| n.attribute("PartName").unwrap().trim_start_matches('/'))
        .collect();
    let defaults: Vec<&str> = types
        .descendants()
        .filter(|n| local(*n) == "Default")
        .map(|n| n.attribute("Extension").unwrap())
        .collect();
    for name in &names {
        let ext = name.rsplit('.').next().unwrap();
        assert!(
            overridden.contains(name) || defaults.contains(&ext),
            "{name} has no content type"
        );
    }
    for part in &overridden {
        assert!(names.contains(part), "content type for absent part {part}");
    }

    for (name, _) in &pkg.0 {
        let Some(rest) = name.strip_suffix(".rels") else {
            continue;
        };
        let source = rest.replace("_rels/", "");
        let base = source.rsplit_once('/').map_or("", |(d, _)| d);
        let rels = pkg.xml(name);
        let rels = roxmltree::Document::parse(&rels).unwrap();
        let mut ids = Vec::new();
        for r in rels.descendants().filter(|n| local(*n) == "Relationship") {
            ids.push(r.attribute("Id").unwrap().to_string());
            if r.attribute("TargetMode") == Some("External") {
                continue;
            }
            let target = r.attribute("Target").unwrap();
            let path = if base.is_empty() || name == "_rels/.rels" {
                target.to_string()
            } else {
                format!("{base}/{target}")
            };
            assert!(names.contains(&path.as_str()), "{name} names absent {path}");
        }
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "duplicate relationship id in {name}"
        );

        let part = source;
        if let Some((_, body)) = pkg.0.iter().find(|(n, _)| *n == part) {
            let body = String::from_utf8(body.clone()).unwrap();
            let body = roxmltree::Document::parse(&body).unwrap();
            for n in body.descendants() {
                for a in n.attributes() {
                    if a.namespace()
                        == Some(
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                        )
                    {
                        assert!(
                            ids.iter().any(|i| i == a.value()),
                            "{part} names unknown {}",
                            a.value()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_section_break_rides_its_last_paragraph_and_the_last_section_closes_the_body() {
    let pkg = Package::of(&kitchen());
    let text = pkg.xml("word/document.xml");
    let doc = roxmltree::Document::parse(&text).unwrap();
    let body = doc.descendants().find(|n| local(*n) == "body").unwrap();
    let children: Vec<_> = body.children().filter(|c| c.is_element()).collect();
    assert_eq!(local(*children.last().unwrap()), "sectPr");
    let in_par: Vec<_> = body
        .descendants()
        .filter(|n| local(*n) == "sectPr" && local(n.parent().unwrap()) == "pPr")
        .collect();
    assert_eq!(in_par.len(), 1);
    let carrier = in_par[0].parent().unwrap().parent().unwrap();
    let next = carrier
        .next_siblings()
        .skip(1)
        .find(|s| s.is_element())
        .unwrap();
    assert!(
        next.descendants().any(|n| n.text() == Some("landscape")),
        "the break closes the first section"
    );

    for tc in doc.descendants().filter(|n| local(*n) == "tc") {
        let last = tc.children().filter(|c| c.is_element()).last().unwrap();
        assert_eq!(local(last), "p", "a cell ends in a paragraph");
    }
    let ids: Vec<&str> = doc
        .descendants()
        .filter(|n| local(*n) == "docPr")
        .map(|n| n.attribute("id").unwrap())
        .collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "drawing ids are unique");
}

#[test]
fn a_spanning_field_opens_in_its_first_entry_and_closes_in_its_last() {
    let pkg = Package::of(&kitchen());
    let text = pkg.xml("word/document.xml");
    let doc = roxmltree::Document::parse(&text).unwrap();
    let chars: Vec<(String, &str)> = doc
        .descendants()
        .filter(|n| local(*n) == "fldChar")
        .map(|n| {
            let p = n.ancestors().find(|a| local(*a) == "p").unwrap();
            let text: String = p
                .descendants()
                .filter_map(|d| (local(d) == "t").then(|| d.text()).flatten())
                .collect();
            (
                text,
                n.attributes()
                    .find(|a| a.name() == "fldCharType")
                    .unwrap()
                    .value(),
            )
        })
        .collect();
    let toc: Vec<_> = chars
        .iter()
        .filter(|(t, _)| t.starts_with("Intro") || t.starts_with("End"))
        .collect();
    assert_eq!(
        toc.iter()
            .map(|(t, k)| (t.split('\t').next().unwrap(), *k))
            .collect::<Vec<_>>(),
        [("Intro", "begin"), ("Intro", "separate"), ("End", "end")]
    );
}

#[test]
fn equal_models_write_equal_bytes() {
    assert_eq!(write_docx(&kitchen()), write_docx(&kitchen()));
}

#[test]
fn an_empty_document_writes_one_paragraph_and_its_section() {
    let pkg = Package::of(&Document::default());
    let text = pkg.xml("word/document.xml");
    let doc = roxmltree::Document::parse(&text).unwrap();
    let body = doc.descendants().find(|n| local(*n) == "body").unwrap();
    let kids: Vec<&str> = body
        .children()
        .filter(|c| c.is_element())
        .map(local)
        .collect();
    assert_eq!(kids, ["p", "sectPr"]);
}

#[test]
fn a_hyperlink_target_is_written_as_a_uri() {
    let link = Paragraph {
        props: ParProps::default(),
        content: vec![Inline::Hyperlink(Hyperlink {
            target: LinkTarget::Url("https://example.com/a b/\u{e9}?q=<x>&r=%zz%41#one#two".into()),
            runs: vec![run("out")],
        })],
    };
    let doc = Document {
        sections: vec![Section {
            body: Blocks::from_paragraphs(vec![link]),
            props: SectionProps::default(),
        }],
        ..Document::default()
    };
    let rels = Package::of(&doc).xml("word/_rels/document.xml.rels");
    let rels = roxmltree::Document::parse(&rels).unwrap();
    let targets: Vec<&str> = rels
        .descendants()
        .filter(|n| n.attribute("TargetMode") == Some("External"))
        .filter_map(|n| n.attribute("Target"))
        .collect();
    assert_eq!(
        targets,
        ["https://example.com/a%20b/%C3%A9?q=%3Cx%3E&r=%25zz%41#one%23two"]
    );
}
