//! Model to `.docx` bytes. Each property struct is written field by field in
//! the order ECMA-376 gives its children, so the output needs no reordering.
//! Drawing and control ids come from one counter in writing order, and
//! relationship ids are fixed names, so equal models give equal bytes.

mod xml;
pub(crate) mod zip;

use std::collections::BTreeSet;

use crate::model::*;
use xml::Xml;

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT_MAIN: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

/// The namespace declarations of every part that holds body content.
const BODY_NS: &[(&str, &str)] = &[
    ("xmlns:w", NS_W),
    ("xmlns:r", NS_R),
    ("xmlns:wp", NS_WP),
    ("xmlns:a", NS_A),
    ("xmlns:pic", NS_PIC),
    ("xmlns:wps", NS_WPS),
    ("xmlns:mc", NS_MC),
];

/// The `.docx` package for `doc`.
pub fn write_docx(doc: &Document) -> Vec<u8> {
    let mut ids = 0u32;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut rels_parts = Vec::new();

    let mut ctx = PartCtx::new(&mut ids);
    let body = document_xml(doc, &mut ctx);
    let doc_rels = ctx.rels;
    rels_parts.push(("word/_rels/document.xml.rels".to_string(), doc_rels.clone()));

    let mut hf = Vec::new();
    for (kind, list) in [("header", &doc.headers), ("footer", &doc.footers)] {
        for (i, blocks) in list.iter().enumerate() {
            let mut ctx = PartCtx::new(&mut ids);
            let root = if kind == "header" { "w:hdr" } else { "w:ftr" };
            let mut x = Xml::new();
            x.open(root, BODY_NS);
            write_blocks(&mut x, blocks, &mut ctx);
            x.close(root);
            let name = format!("{kind}{}.xml", i + 1);
            if !ctx.rels.is_empty() {
                rels_parts.push((format!("word/_rels/{name}.rels"), ctx.rels.clone()));
            }
            hf.push((format!("word/{name}"), x.finish()));
        }
    }

    let footnotes = (!doc.footnotes.is_empty()).then(|| {
        let mut ctx = PartCtx::new(&mut ids);
        let xml = footnotes_xml(doc, &mut ctx);
        if !ctx.rels.is_empty() {
            rels_parts.push((
                "word/_rels/footnotes.xml.rels".to_string(),
                ctx.rels.clone(),
            ));
        }
        xml
    });

    parts.push(("[Content_Types].xml".into(), content_types(doc)));
    parts.push(("_rels/.rels".into(), package_rels()));
    parts.push(("docProps/core.xml".into(), core_xml(&doc.info)));
    parts.push(("docProps/app.xml".into(), app_xml()));
    parts.push(("word/document.xml".into(), body));
    parts.push(("word/styles.xml".into(), styles_xml(&doc.styles)));
    parts.push(("word/numbering.xml".into(), numbering_xml(&doc.numbering)));
    parts.push(("word/settings.xml".into(), settings_xml(doc)));
    parts.push(("word/fontTable.xml".into(), font_table_xml(&doc.fonts)));
    parts.extend(hf);
    if let Some(f) = footnotes {
        parts.push(("word/footnotes.xml".into(), f));
    }
    for (name, rels) in rels_parts {
        let main = name == "word/_rels/document.xml.rels";
        parts.push((name, rels.finish(doc, main)));
    }
    for (i, m) in doc.media.iter().enumerate() {
        parts.push((media_name(i, m.kind), m.bytes.clone()));
    }
    zip::zip(&parts)
}

fn media_name(i: usize, kind: MediaKind) -> String {
    format!("word/media/image{}.{}", i + 1, kind.extension())
}

/// The relationships one part's content needs.
#[derive(Clone, Default)]
struct Rels {
    media: BTreeSet<usize>,
    links: Vec<String>,
}

impl Rels {
    fn is_empty(&self) -> bool {
        self.media.is_empty() && self.links.is_empty()
    }

    fn finish(&self, doc: &Document, main: bool) -> Vec<u8> {
        let mut x = Xml::new();
        x.open(
            "Relationships",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/relationships",
            )],
        );
        let rel = |x: &mut Xml, id: &str, kind: &str, target: &str| {
            x.empty(
                "Relationship",
                &[
                    ("Id", id),
                    ("Type", &format!("{REL}/{kind}")),
                    ("Target", target),
                ],
            );
        };
        if main {
            rel(&mut x, "rIdStyles", "styles", "styles.xml");
            rel(&mut x, "rIdNumbering", "numbering", "numbering.xml");
            rel(&mut x, "rIdSettings", "settings", "settings.xml");
            rel(&mut x, "rIdFontTable", "fontTable", "fontTable.xml");
            if !doc.footnotes.is_empty() {
                rel(&mut x, "rIdFootnotes", "footnotes", "footnotes.xml");
            }
            for i in 0..doc.headers.len() {
                rel(
                    &mut x,
                    &format!("rIdHdr{}", i + 1),
                    "header",
                    &format!("header{}.xml", i + 1),
                );
            }
            for i in 0..doc.footers.len() {
                rel(
                    &mut x,
                    &format!("rIdFtr{}", i + 1),
                    "footer",
                    &format!("footer{}.xml", i + 1),
                );
            }
        }
        for &i in &self.media {
            let target = media_name(i, doc.media[i].kind);
            rel(
                &mut x,
                &format!("rIdImg{}", i + 1),
                "image",
                &target["word/".len()..],
            );
        }
        for (i, url) in self.links.iter().enumerate() {
            x.empty(
                "Relationship",
                &[
                    ("Id", &format!("rIdLink{}", i + 1)),
                    ("Type", &format!("{REL}/hyperlink")),
                    ("Target", url),
                    ("TargetMode", "External"),
                ],
            );
        }
        x.close("Relationships");
        x.finish()
    }
}

/// What writing one part's content shares: the id counter and the part's
/// relationships.
struct PartCtx<'a> {
    ids: &'a mut u32,
    rels: Rels,
}

impl<'a> PartCtx<'a> {
    fn new(ids: &'a mut u32) -> Self {
        Self {
            ids,
            rels: Rels::default(),
        }
    }

    fn next_id(&mut self) -> u32 {
        *self.ids += 1;
        *self.ids
    }

    fn link(&mut self, url: &str) -> String {
        let i = match self.rels.links.iter().position(|u| u == url) {
            Some(i) => i,
            None => {
                self.rels.links.push(url.to_string());
                self.rels.links.len() - 1
            }
        };
        format!("rIdLink{}", i + 1)
    }
}

fn document_xml(doc: &Document, ctx: &mut PartCtx) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:document", BODY_NS);
    x.open("w:body", &[]);
    let n = doc.sections.len();
    for (i, section) in doc.sections.iter().enumerate() {
        for block in &section.body.blocks {
            write_block(&mut x, block, ctx);
        }
        let props = (i + 1 < n).then_some(&section.props);
        write_paragraph(&mut x, &section.body.last, props, ctx);
    }
    if let Some(last) = doc.sections.last() {
        write_sect_pr(&mut x, &last.props);
    } else {
        x.empty("w:p", &[]);
        write_sect_pr(&mut x, &SectionProps::default());
    }
    x.close("w:body");
    x.close("w:document");
    x.finish()
}

fn write_blocks(x: &mut Xml, blocks: &Blocks, ctx: &mut PartCtx) {
    for block in &blocks.blocks {
        write_block(x, block, ctx);
    }
    write_paragraph(x, &blocks.last, None, ctx);
}

fn write_block(x: &mut Xml, block: &Block, ctx: &mut PartCtx) {
    match block {
        Block::Paragraph(p) => write_paragraph(x, p, None, ctx),
        Block::Table(t) => write_table(x, t, ctx),
        Block::Toc(t) => write_toc(x, t, ctx),
    }
}

fn write_toc(x: &mut Xml, toc: &Toc, ctx: &mut PartCtx) {
    let begin = |x: &mut Xml| {
        x.open("w:r", &[]);
        let lock = [("w:fldCharType", "begin"), ("w:fldLock", "1")];
        x.empty("w:fldChar", if toc.locked { &lock[..] } else { &lock[..1] });
        x.close("w:r");
        x.open("w:r", &[]);
        x.leaf(
            "w:instrText",
            &[("xml:space", "preserve")],
            &format!(" {} ", toc.instruction),
        );
        x.close("w:r");
        x.open("w:r", &[]);
        x.empty("w:fldChar", &[("w:fldCharType", "separate")]);
        x.close("w:r");
    };
    let end = |x: &mut Xml| {
        x.open("w:r", &[]);
        x.empty("w:fldChar", &[("w:fldCharType", "end")]);
        x.close("w:r");
    };
    let n = toc.entries.len();
    if n == 0 {
        x.open("w:p", &[]);
        begin(x);
        end(x);
        x.close("w:p");
        return;
    }
    for (i, p) in toc.entries.iter().enumerate() {
        x.open("w:p", &[]);
        write_par_props(x, &p.props, None);
        if i == 0 {
            begin(x);
        }
        for inline in &p.content {
            write_inline(x, inline, ctx);
        }
        if i + 1 == n {
            end(x);
        }
        x.close("w:p");
    }
}

fn write_paragraph(x: &mut Xml, p: &Paragraph, sect: Option<&SectionProps>, ctx: &mut PartCtx) {
    x.open("w:p", &[]);
    write_par_props(x, &p.props, sect);
    for inline in &p.content {
        write_inline(x, inline, ctx);
    }
    x.close("w:p");
}

fn on_off(x: &mut Xml, tag: &str, on: bool) {
    if on {
        x.empty(tag, &[]);
    }
}

fn par_props_empty(p: &ParProps) -> bool {
    *p == ParProps::default()
}

fn write_par_props(x: &mut Xml, p: &ParProps, sect: Option<&SectionProps>) {
    if par_props_empty(p) && sect.is_none() {
        return;
    }
    x.open("w:pPr", &[]);
    write_par_props_body(x, p, false);
    if let Some(s) = sect {
        write_sect_pr(x, s);
    }
    x.close("w:pPr");
}

/// `in_style` leaves out the style reference and the paragraph mark, which a
/// style's own properties cannot hold.
fn write_par_props_body(x: &mut Xml, p: &ParProps, in_style: bool) {
    if let (Some(s), false) = (&p.style, in_style) {
        x.val("w:pStyle", s);
    }
    on_off(x, "w:keepNext", p.keep_next);
    on_off(x, "w:keepLines", p.keep_lines);
    on_off(x, "w:pageBreakBefore", p.page_break_before);
    match p.widow_control {
        Some(true) => x.empty("w:widowControl", &[]),
        Some(false) => x.val("w:widowControl", "0"),
        None => {}
    }
    if let Some(n) = p.numbering {
        x.open("w:numPr", &[]);
        x.val("w:ilvl", &n.level.to_string());
        x.val("w:numId", &n.num.to_string());
        x.close("w:numPr");
    }
    write_tabs(x, &p.tabs);
    if let Some(s) = &p.spacing {
        write_spacing(x, s);
    }
    if let Some(i) = &p.indent {
        write_indent(x, i);
    }
    if let Some(a) = p.align {
        x.val("w:jc", align_val(a));
    }
    if let Some(l) = p.outline_level {
        x.val("w:outlineLvl", &l.to_string());
    }
    if let (Some(r), false) = (&p.mark, in_style) {
        write_run_props(x, r, false);
    }
}

fn write_tabs(x: &mut Xml, tabs: &[TabStop]) {
    if tabs.is_empty() {
        return;
    }
    x.open("w:tabs", &[]);
    for t in tabs {
        let kind = match t.kind {
            TabKind::Left => "left",
            TabKind::Center => "center",
            TabKind::Right => "right",
            TabKind::Decimal => "decimal",
            TabKind::Clear => "clear",
        };
        let pos = t.pos.to_string();
        match t.leader {
            TabLeader::None => x.empty("w:tab", &[("w:val", kind), ("w:pos", &pos)]),
            leader => {
                let leader = match leader {
                    TabLeader::Dot => "dot",
                    TabLeader::Hyphen => "hyphen",
                    _ => "underscore",
                };
                x.empty(
                    "w:tab",
                    &[("w:val", kind), ("w:leader", leader), ("w:pos", &pos)],
                );
            }
        }
    }
    x.close("w:tabs");
}

fn write_spacing(x: &mut Xml, s: &Spacing) {
    let mut attrs: Vec<(&str, String)> = Vec::new();
    if let Some(b) = s.before {
        attrs.push(("w:before", b.to_string()));
    }
    if let Some(a) = s.after {
        attrs.push(("w:after", a.to_string()));
    }
    match s.line {
        Some(Line::Exact(v)) => {
            attrs.push(("w:line", v.to_string()));
            attrs.push(("w:lineRule", "exact".into()));
        }
        Some(Line::AtLeast(v)) => {
            attrs.push(("w:line", v.to_string()));
            attrs.push(("w:lineRule", "atLeast".into()));
        }
        Some(Line::Auto(v)) => {
            attrs.push(("w:line", v.to_string()));
            attrs.push(("w:lineRule", "auto".into()));
        }
        None => {}
    }
    let attrs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    x.empty("w:spacing", &attrs);
}

fn write_indent(x: &mut Xml, i: &Indent) {
    let mut attrs: Vec<(&str, String)> = Vec::new();
    if let Some(l) = i.left {
        attrs.push(("w:left", l.to_string()));
    }
    if let Some(r) = i.right {
        attrs.push(("w:right", r.to_string()));
    }
    match i.first {
        Some(FirstLine::Indent(v)) => attrs.push(("w:firstLine", v.to_string())),
        Some(FirstLine::Hanging(v)) => attrs.push(("w:hanging", v.to_string())),
        None => {}
    }
    let attrs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    x.empty("w:ind", &attrs);
}

fn align_val(a: Align) -> &'static str {
    match a {
        Align::Start => "left",
        Align::Center => "center",
        Align::End => "right",
        Align::Both => "both",
    }
}

fn write_run_props(x: &mut Xml, r: &RunProps, in_style: bool) {
    if *r == RunProps::default() {
        return;
    }
    x.open("w:rPr", &[]);
    if let (Some(s), false) = (&r.style, in_style) {
        x.val("w:rStyle", s);
    }
    if let Some(f) = &r.font {
        x.empty("w:rFonts", &[("w:ascii", f), ("w:hAnsi", f), ("w:cs", f)]);
    }
    let flag = |x: &mut Xml, tag: &str, v: Option<bool>| match v {
        Some(true) => x.empty(tag, &[]),
        Some(false) => x.val(tag, "0"),
        None => {}
    };
    flag(x, "w:b", r.bold);
    flag(x, "w:i", r.italic);
    on_off(x, "w:strike", r.strike);
    on_off(x, "w:noProof", r.no_proof);
    if let Some(c) = &r.color {
        x.val("w:color", c);
    }
    if let Some(s) = r.spacing {
        x.val("w:spacing", &s.to_string());
    }
    if let Some(w) = r.scale {
        x.val("w:w", &w.to_string());
    }
    if let Some(p) = r.position {
        x.val("w:position", &p.to_string());
    }
    if let Some(sz) = r.size {
        x.val("w:sz", &sz.to_string());
        x.val("w:szCs", &sz.to_string());
    }
    if r.underline {
        x.val("w:u", "single");
    }
    if let Some(fill) = &r.shade {
        x.empty(
            "w:shd",
            &[("w:val", "clear"), ("w:color", "auto"), ("w:fill", fill)],
        );
    }
    match r.vert {
        Some(VertAlign::Superscript) => x.val("w:vertAlign", "superscript"),
        Some(VertAlign::Subscript) => x.val("w:vertAlign", "subscript"),
        None => {}
    }
    if let Some(l) = &r.lang {
        x.val("w:lang", l);
    }
    x.close("w:rPr");
}

fn write_inline(x: &mut Xml, inline: &Inline, ctx: &mut PartCtx) {
    match inline {
        Inline::Run(r) => write_run(x, r, ctx),
        Inline::Hyperlink(h) => {
            match &h.target {
                LinkTarget::Url(url) => {
                    let id = ctx.link(url);
                    x.open("w:hyperlink", &[("r:id", &id)]);
                }
                LinkTarget::Bookmark(name) => x.open("w:hyperlink", &[("w:anchor", name)]),
            }
            for r in &h.runs {
                write_run(x, r, ctx);
            }
            x.close("w:hyperlink");
        }
        Inline::Field(f) => {
            let char_run = |x: &mut Xml, kind: &str, lock: bool| {
                x.open("w:r", &[]);
                write_run_props(x, &f.props, false);
                if lock {
                    x.empty("w:fldChar", &[("w:fldCharType", kind), ("w:fldLock", "1")]);
                } else {
                    x.empty("w:fldChar", &[("w:fldCharType", kind)]);
                }
                x.close("w:r");
            };
            char_run(x, "begin", f.locked);
            x.open("w:r", &[]);
            write_run_props(x, &f.props, false);
            x.leaf(
                "w:instrText",
                &[("xml:space", "preserve")],
                &format!(" {} ", f.instruction),
            );
            x.close("w:r");
            char_run(x, "separate", false);
            for r in &f.result {
                write_run(x, r, ctx);
            }
            char_run(x, "end", false);
        }
        Inline::Control(c) => {
            let id = ctx.next_id().to_string();
            x.open("w:sdt", &[]);
            x.open("w:sdtPr", &[]);
            x.val("w:alias", &c.title);
            x.val("w:tag", &c.tag);
            x.val("w:id", &id);
            on_off(x, "w:showingPlcHdr", c.placeholder);
            match c.kind {
                ControlKind::RichText => x.empty("w:richText", &[]),
                ControlKind::Date => {
                    x.open("w:date", &[]);
                    x.val("w:dateFormat", "d MMMM yyyy");
                    x.val("w:lid", "en-US");
                    x.val("w:storeMappedDataAs", "dateTime");
                    x.val("w:calendar", "gregorian");
                    x.close("w:date");
                }
            }
            x.close("w:sdtPr");
            x.open("w:sdtContent", &[]);
            for i in &c.content {
                write_inline(x, i, ctx);
            }
            x.close("w:sdtContent");
            x.close("w:sdt");
        }
        Inline::BookmarkStart { name, id } => {
            x.empty(
                "w:bookmarkStart",
                &[("w:id", &id.to_string()), ("w:name", name)],
            );
        }
        Inline::BookmarkEnd { id } => x.empty("w:bookmarkEnd", &[("w:id", &id.to_string())]),
    }
}

fn write_run(x: &mut Xml, r: &Run, ctx: &mut PartCtx) {
    x.open("w:r", &[]);
    write_run_props(x, &r.props, false);
    for c in &r.content {
        match c {
            RunContent::Text(t) => {
                let preserve = t.starts_with(' ') || t.ends_with(' ') || t.contains("  ");
                if preserve {
                    x.leaf("w:t", &[("xml:space", "preserve")], t);
                } else {
                    x.leaf("w:t", &[], t);
                }
            }
            RunContent::Tab => x.empty("w:tab", &[]),
            RunContent::Break(BreakKind::Line) => x.empty("w:br", &[]),
            RunContent::Break(BreakKind::Page) => x.empty("w:br", &[("w:type", "page")]),
            RunContent::Drawing(d) => write_drawing(x, d, ctx),
            RunContent::FootnoteRef(i) => {
                x.empty("w:footnoteReference", &[("w:id", &(i + 1).to_string())]);
            }
            RunContent::FootnoteMark => x.empty("w:footnoteRef", &[]),
        }
    }
    x.close("w:r");
}

fn write_drawing(x: &mut Xml, d: &Drawing, ctx: &mut PartCtx) {
    let shape = !matches!(d.content, DrawingContent::Picture(_));
    if shape {
        x.open("mc:AlternateContent", &[]);
        x.open("mc:Choice", &[("Requires", "wps")]);
    }
    x.open("w:drawing", &[]);
    let id = ctx.next_id();
    let (cx, cy) = (d.width.max(0).to_string(), d.height.max(0).to_string());
    let zero = [
        ("distT", "0"),
        ("distB", "0"),
        ("distL", "0"),
        ("distR", "0"),
    ];
    match d.placement {
        Placement::Inline => x.open("wp:inline", &zero),
        Placement::Anchor(a) => {
            let z = a.z.to_string();
            let mut attrs = zero.to_vec();
            attrs.extend([
                ("simplePos", "0"),
                ("relativeHeight", z.as_str()),
                ("behindDoc", if a.behind_text { "1" } else { "0" }),
                ("locked", "1"),
                ("layoutInCell", "1"),
                ("allowOverlap", "1"),
            ]);
            x.open("wp:anchor", &attrs);
            x.empty("wp:simplePos", &[("x", "0"), ("y", "0")]);
            x.open("wp:positionH", &[("relativeFrom", "page")]);
            x.leaf("wp:posOffset", &[], &a.x.to_string());
            x.close("wp:positionH");
            x.open("wp:positionV", &[("relativeFrom", "page")]);
            x.leaf("wp:posOffset", &[], &a.y.to_string());
            x.close("wp:positionV");
        }
    }
    x.empty("wp:extent", &[("cx", &cx), ("cy", &cy)]);
    x.empty(
        "wp:effectExtent",
        &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")],
    );
    if let Placement::Anchor(a) = d.placement {
        match a.wrap {
            Wrap::None => x.empty("wp:wrapNone", &[]),
            Wrap::TopAndBottom => x.empty("wp:wrapTopAndBottom", &[]),
            Wrap::Square => x.empty("wp:wrapSquare", &[("wrapText", "bothSides")]),
        }
    }
    let id_s = id.to_string();
    let name = format!("Object {id}");
    match &d.descr {
        Some(descr) => x.empty(
            "wp:docPr",
            &[("id", &id_s), ("name", &name), ("descr", descr)],
        ),
        None => x.empty("wp:docPr", &[("id", &id_s), ("name", &name)]),
    }
    x.empty("wp:cNvGraphicFramePr", &[]);
    x.open("a:graphic", &[]);
    let ext = |x: &mut Xml, flip: bool| {
        if flip {
            x.open("a:xfrm", &[("flipV", "1")]);
        } else {
            x.open("a:xfrm", &[]);
        }
        x.empty("a:off", &[("x", "0"), ("y", "0")]);
        x.empty("a:ext", &[("cx", &cx), ("cy", &cy)]);
        x.close("a:xfrm");
    };
    let geom = |x: &mut Xml, prst: &str| {
        x.open("a:prstGeom", &[("prst", prst)]);
        x.empty("a:avLst", &[]);
        x.close("a:prstGeom");
    };
    match &d.content {
        DrawingContent::Picture(m) => {
            ctx.rels.media.insert(*m);
            x.open("a:graphicData", &[("uri", NS_PIC)]);
            x.open("pic:pic", &[]);
            x.open("pic:nvPicPr", &[]);
            x.empty("pic:cNvPr", &[("id", "0"), ("name", &name)]);
            x.empty("pic:cNvPicPr", &[]);
            x.close("pic:nvPicPr");
            x.open("pic:blipFill", &[]);
            x.empty("a:blip", &[("r:embed", &format!("rIdImg{}", m + 1))]);
            x.open("a:stretch", &[]);
            x.empty("a:fillRect", &[]);
            x.close("a:stretch");
            x.close("pic:blipFill");
            x.open("pic:spPr", &[]);
            ext(x, false);
            geom(x, "rect");
            x.close("pic:spPr");
            x.close("pic:pic");
            x.close("a:graphicData");
        }
        DrawingContent::TextBox(tb) => {
            x.open("a:graphicData", &[("uri", NS_WPS)]);
            x.open("wps:wsp", &[]);
            x.empty("wps:cNvSpPr", &[("txBox", "1")]);
            x.open("wps:spPr", &[]);
            ext(x, false);
            geom(x, "rect");
            x.empty("a:noFill", &[]);
            x.open("a:ln", &[]);
            x.empty("a:noFill", &[]);
            x.close("a:ln");
            x.close("wps:spPr");
            x.open("wps:txbx", &[]);
            x.open("w:txbxContent", &[]);
            write_blocks(x, &tb.body, ctx);
            x.close("w:txbxContent");
            x.close("wps:txbx");
            let wrap = if tb.wrap { "square" } else { "none" };
            x.open(
                "wps:bodyPr",
                &[
                    ("rot", "0"),
                    ("vert", "horz"),
                    ("wrap", wrap),
                    ("lIns", "0"),
                    ("tIns", "0"),
                    ("rIns", "0"),
                    ("bIns", "0"),
                    ("anchor", "t"),
                    ("anchorCtr", "0"),
                ],
            );
            x.empty("a:noAutofit", &[]);
            x.close("wps:bodyPr");
            x.close("wps:wsp");
            x.close("a:graphicData");
        }
        DrawingContent::Shape(s) => {
            x.open("a:graphicData", &[("uri", NS_WPS)]);
            x.open("wps:wsp", &[]);
            x.empty("wps:cNvSpPr", &[]);
            x.open("wps:spPr", &[]);
            let (prst, flip) = match s.geometry {
                ShapeGeometry::Rect => ("rect", false),
                ShapeGeometry::Line { rising } => ("line", rising),
            };
            ext(x, flip);
            geom(x, prst);
            match &s.fill {
                Some(c) => solid(x, c),
                None => x.empty("a:noFill", &[]),
            }
            match &s.stroke {
                Some(st) => {
                    x.open("a:ln", &[("w", &st.width.max(0).to_string())]);
                    solid(x, &st.color);
                    x.close("a:ln");
                }
                None => {
                    x.open("a:ln", &[]);
                    x.empty("a:noFill", &[]);
                    x.close("a:ln");
                }
            }
            x.close("wps:spPr");
            x.empty("wps:bodyPr", &[]);
            x.close("wps:wsp");
            x.close("a:graphicData");
        }
    }
    x.close("a:graphic");
    match d.placement {
        Placement::Inline => x.close("wp:inline"),
        Placement::Anchor(_) => x.close("wp:anchor"),
    }
    x.close("w:drawing");
    if shape {
        x.close("mc:Choice");
        x.close("mc:AlternateContent");
    }
}

fn solid(x: &mut Xml, color: &str) {
    x.open("a:solidFill", &[]);
    x.empty("a:srgbClr", &[("val", color)]);
    x.close("a:solidFill");
}

fn write_border(x: &mut Xml, tag: &str, b: &Option<Border>) {
    match b {
        Some(b) => x.empty(
            tag,
            &[
                ("w:val", "single"),
                ("w:sz", &b.size.to_string()),
                ("w:space", "0"),
                ("w:color", &b.color),
            ],
        ),
        None => x.empty(tag, &[("w:val", "nil")]),
    }
}

fn write_borders(x: &mut Xml, tag: &str, b: &Borders) {
    x.open(tag, &[]);
    write_border(x, "w:top", &b.top);
    write_border(x, "w:left", &b.left);
    write_border(x, "w:bottom", &b.bottom);
    write_border(x, "w:right", &b.right);
    write_border(x, "w:insideH", &b.inside_h);
    write_border(x, "w:insideV", &b.inside_v);
    x.close(tag);
}

fn write_margins(x: &mut Xml, tag: &str, m: &CellMargins) {
    x.open(tag, &[]);
    for (side, v) in [
        ("w:top", m.top),
        ("w:left", m.left),
        ("w:bottom", m.bottom),
        ("w:right", m.right),
    ] {
        x.empty(side, &[("w:w", &v.max(0).to_string()), ("w:type", "dxa")]);
    }
    x.close(tag);
}

fn write_table(x: &mut Xml, t: &Table, ctx: &mut PartCtx) {
    x.open("w:tbl", &[]);
    x.open("w:tblPr", &[]);
    let p = &t.props;
    if let Some(s) = &p.style {
        x.val("w:tblStyle", s);
    }
    match p.width {
        Some(w) => x.empty("w:tblW", &[("w:w", &w.to_string()), ("w:type", "dxa")]),
        None => x.empty("w:tblW", &[("w:w", "0"), ("w:type", "auto")]),
    }
    if let Some(i) = p.indent {
        x.empty("w:tblInd", &[("w:w", &i.to_string()), ("w:type", "dxa")]);
    }
    if let Some(b) = &p.borders {
        write_borders(x, "w:tblBorders", b);
    }
    if p.fixed {
        x.empty("w:tblLayout", &[("w:type", "fixed")]);
    }
    if let Some(m) = &p.cell_margins {
        write_margins(x, "w:tblCellMar", m);
    }
    x.close("w:tblPr");
    x.open("w:tblGrid", &[]);
    for w in &t.grid {
        x.empty("w:gridCol", &[("w:w", &w.max(&0).to_string())]);
    }
    x.close("w:tblGrid");
    for row in &t.rows {
        x.open("w:tr", &[]);
        let rp = &row.props;
        if rp.cant_split || rp.header || rp.min_height.is_some() {
            x.open("w:trPr", &[]);
            on_off(x, "w:cantSplit", rp.cant_split);
            if let Some(h) = rp.min_height {
                x.empty(
                    "w:trHeight",
                    &[("w:val", &h.to_string()), ("w:hRule", "atLeast")],
                );
            }
            on_off(x, "w:tblHeader", rp.header);
            x.close("w:trPr");
        }
        for cell in &row.cells {
            x.open("w:tc", &[]);
            let cp = &cell.props;
            x.open("w:tcPr", &[]);
            match cp.width {
                Some(w) => x.empty("w:tcW", &[("w:w", &w.to_string()), ("w:type", "dxa")]),
                None => x.empty("w:tcW", &[("w:w", "0"), ("w:type", "auto")]),
            }
            if cp.span > 1 {
                x.val("w:gridSpan", &cp.span.to_string());
            }
            match cp.merge {
                Some(VMerge::Restart) => x.val("w:vMerge", "restart"),
                Some(VMerge::Continue) => x.empty("w:vMerge", &[]),
                None => {}
            }
            if let Some(b) = &cp.borders {
                write_borders(x, "w:tcBorders", b);
            }
            if let Some(fill) = &cp.fill {
                x.empty(
                    "w:shd",
                    &[("w:val", "clear"), ("w:color", "auto"), ("w:fill", fill)],
                );
            }
            on_off(x, "w:noWrap", cp.no_wrap);
            if let Some(m) = &cp.margins {
                write_margins(x, "w:tcMar", m);
            }
            match cp.valign {
                Some(VAlign::Top) => x.val("w:vAlign", "top"),
                Some(VAlign::Center) => x.val("w:vAlign", "center"),
                Some(VAlign::Bottom) => x.val("w:vAlign", "bottom"),
                None => {}
            }
            x.close("w:tcPr");
            write_blocks(x, &cell.body, ctx);
            x.close("w:tc");
        }
        x.close("w:tr");
    }
    x.close("w:tbl");
}

fn num_fmt(f: NumFormat) -> &'static str {
    match f {
        NumFormat::Decimal => "decimal",
        NumFormat::LowerLetter => "lowerLetter",
        NumFormat::UpperLetter => "upperLetter",
        NumFormat::LowerRoman => "lowerRoman",
        NumFormat::UpperRoman => "upperRoman",
        NumFormat::Bullet => "bullet",
        NumFormat::None => "none",
    }
}

fn write_sect_pr(x: &mut Xml, s: &SectionProps) {
    x.open("w:sectPr", &[]);
    let refs = |x: &mut Xml, tag: &str, prefix: &str, r: &HeaderRefs| {
        for (kind, idx) in [("default", r.default), ("first", r.first), ("even", r.even)] {
            if let Some(i) = idx {
                x.empty(
                    tag,
                    &[("w:type", kind), ("r:id", &format!("{prefix}{}", i + 1))],
                );
            }
        }
    };
    refs(x, "w:headerReference", "rIdHdr", &s.header);
    refs(x, "w:footerReference", "rIdFtr", &s.footer);
    let start = match s.start {
        SectionStart::NextPage => "nextPage",
        SectionStart::OddPage => "oddPage",
        SectionStart::EvenPage => "evenPage",
        SectionStart::Continuous => "continuous",
    };
    x.val("w:type", start);
    let (w, h) = (s.page_width.to_string(), s.page_height.to_string());
    if s.landscape {
        x.empty(
            "w:pgSz",
            &[("w:w", &w), ("w:h", &h), ("w:orient", "landscape")],
        );
    } else {
        x.empty("w:pgSz", &[("w:w", &w), ("w:h", &h)]);
    }
    let m = &s.margins;
    x.empty(
        "w:pgMar",
        &[
            ("w:top", &m.top.to_string()),
            ("w:right", &m.right.to_string()),
            ("w:bottom", &m.bottom.to_string()),
            ("w:left", &m.left.to_string()),
            ("w:header", &m.header.to_string()),
            ("w:footer", &m.footer.to_string()),
            ("w:gutter", "0"),
        ],
    );
    if let Some(pn) = &s.page_numbers {
        match pn.start {
            Some(st) => x.empty(
                "w:pgNumType",
                &[("w:fmt", num_fmt(pn.format)), ("w:start", &st.to_string())],
            ),
            None => x.empty("w:pgNumType", &[("w:fmt", num_fmt(pn.format))]),
        }
    }
    if let Some(c) = &s.columns {
        x.empty(
            "w:cols",
            &[
                ("w:num", &c.count.to_string()),
                ("w:space", &c.space.to_string()),
            ],
        );
    }
    on_off(x, "w:titlePg", s.title_page);
    x.close("w:sectPr");
}

fn styles_xml(s: &Styles) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:styles", &[("xmlns:w", NS_W)]);
    x.open("w:docDefaults", &[]);
    x.open("w:rPrDefault", &[]);
    write_run_props(&mut x, &s.default_run, true);
    x.close("w:rPrDefault");
    x.open("w:pPrDefault", &[]);
    if !par_props_empty(&s.default_par) {
        x.open("w:pPr", &[]);
        write_par_props_body(&mut x, &s.default_par, true);
        x.close("w:pPr");
    }
    x.close("w:pPrDefault");
    x.close("w:docDefaults");
    for st in &s.styles {
        let kind = match st.kind {
            StyleKind::Paragraph => "paragraph",
            StyleKind::Character => "character",
        };
        let mut attrs = vec![("w:type", kind)];
        if st.id == "Normal" && st.kind == StyleKind::Paragraph {
            attrs.push(("w:default", "1"));
        }
        if !st.builtin {
            attrs.push(("w:customStyle", "1"));
        }
        attrs.push(("w:styleId", &st.id));
        x.open("w:style", &attrs);
        x.val("w:name", &st.name);
        if let Some(b) = &st.based_on {
            x.val("w:basedOn", b);
        }
        if let Some(n) = &st.next {
            x.val("w:next", n);
        }
        if let Some(p) = st.ui_priority {
            x.val("w:uiPriority", &p.to_string());
        }
        on_off(&mut x, "w:semiHidden", st.semi_hidden);
        on_off(&mut x, "w:qFormat", st.quick);
        if !par_props_empty(&st.par) {
            x.open("w:pPr", &[]);
            write_par_props_body(&mut x, &st.par, true);
            x.close("w:pPr");
        }
        write_run_props(&mut x, &st.run, true);
        x.close("w:style");
    }
    x.close("w:styles");
    x.finish()
}

fn numbering_xml(n: &Numbering) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:numbering", &[("xmlns:w", NS_W)]);
    for a in &n.abstracts {
        x.open("w:abstractNum", &[("w:abstractNumId", &a.id.to_string())]);
        x.val("w:nsid", &a.nsid);
        x.val("w:multiLevelType", "multilevel");
        for (i, l) in a.levels.iter().enumerate().take(9) {
            x.open("w:lvl", &[("w:ilvl", &i.to_string())]);
            x.val("w:start", &l.start.to_string());
            x.val("w:numFmt", num_fmt(l.format));
            if let Some(s) = &l.style {
                x.val("w:pStyle", s);
            }
            let suff = match l.suffix {
                LevelSuffix::Tab => "tab",
                LevelSuffix::Space => "space",
                LevelSuffix::Nothing => "nothing",
            };
            x.val("w:suff", suff);
            x.val("w:lvlText", &l.text);
            x.val("w:lvlJc", align_val(l.align));
            x.open("w:pPr", &[]);
            write_tabs(&mut x, &l.tabs);
            write_indent(&mut x, &l.indent);
            x.close("w:pPr");
            write_run_props(&mut x, &l.run, true);
            x.close("w:lvl");
        }
        x.close("w:abstractNum");
    }
    for num in &n.nums {
        x.open("w:num", &[("w:numId", &num.id.to_string())]);
        x.val("w:abstractNumId", &num.abstract_id.to_string());
        for (lvl, start) in &num.starts {
            x.open("w:lvlOverride", &[("w:ilvl", &lvl.to_string())]);
            x.val("w:startOverride", &start.to_string());
            x.close("w:lvlOverride");
        }
        x.close("w:num");
    }
    x.close("w:numbering");
    x.finish()
}

fn settings_xml(doc: &Document) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:settings", &[("xmlns:w", NS_W)]);
    x.val(
        "w:defaultTabStop",
        &doc.settings.default_tab_stop.to_string(),
    );
    on_off(
        &mut x,
        "w:evenAndOddHeaders",
        doc.settings.even_and_odd_headers,
    );
    x.val("w:characterSpacingControl", "doNotCompress");
    if !doc.footnotes.is_empty() {
        x.open("w:footnotePr", &[]);
        x.empty("w:footnote", &[("w:id", "-1")]);
        x.empty("w:footnote", &[("w:id", "0")]);
        x.close("w:footnotePr");
    }
    x.open("w:compat", &[]);
    x.empty(
        "w:compatSetting",
        &[
            ("w:name", "compatibilityMode"),
            ("w:uri", "http://schemas.microsoft.com/office/word"),
            ("w:val", "15"),
        ],
    );
    x.close("w:compat");
    x.close("w:settings");
    x.finish()
}

fn font_table_xml(fonts: &[FontDecl]) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:fonts", &[("xmlns:w", NS_W)]);
    for f in fonts {
        x.open("w:font", &[("w:name", &f.name)]);
        if let Some(a) = &f.alt_name {
            x.val("w:altName", a);
        }
        let family = match f.family {
            FontFamily::Roman => "roman",
            FontFamily::Swiss => "swiss",
            FontFamily::Modern => "modern",
            FontFamily::Auto => "auto",
        };
        x.val("w:family", family);
        x.val("w:pitch", if f.pitch_fixed { "fixed" } else { "variable" });
        x.close("w:font");
    }
    x.close("w:fonts");
    x.finish()
}

fn footnotes_xml(doc: &Document, ctx: &mut PartCtx) -> Vec<u8> {
    let mut x = Xml::new();
    x.open("w:footnotes", BODY_NS);
    for (kind, id, mark) in [
        ("separator", "-1", "w:separator"),
        ("continuationSeparator", "0", "w:continuationSeparator"),
    ] {
        x.open("w:footnote", &[("w:type", kind), ("w:id", id)]);
        x.open("w:p", &[]);
        x.open("w:pPr", &[]);
        x.empty(
            "w:spacing",
            &[("w:after", "0"), ("w:line", "240"), ("w:lineRule", "auto")],
        );
        x.close("w:pPr");
        x.open("w:r", &[]);
        x.empty(mark, &[]);
        x.close("w:r");
        x.close("w:p");
        x.close("w:footnote");
    }
    for (i, f) in doc.footnotes.iter().enumerate() {
        x.open("w:footnote", &[("w:id", &(i + 1).to_string())]);
        write_blocks(&mut x, &f.body, ctx);
        x.close("w:footnote");
    }
    x.close("w:footnotes");
    x.finish()
}

fn content_types(doc: &Document) -> Vec<u8> {
    let mut x = Xml::new();
    x.open(
        "Types",
        &[(
            "xmlns",
            "http://schemas.openxmlformats.org/package/2006/content-types",
        )],
    );
    let default = |x: &mut Xml, ext: &str, ct: &str| {
        x.empty("Default", &[("Extension", ext), ("ContentType", ct)]);
    };
    default(
        &mut x,
        "rels",
        "application/vnd.openxmlformats-package.relationships+xml",
    );
    default(&mut x, "xml", "application/xml");
    for (kind, ct) in [
        (MediaKind::Png, "image/png"),
        (MediaKind::Jpeg, "image/jpeg"),
    ] {
        if doc.media.iter().any(|m| m.kind == kind) {
            default(&mut x, kind.extension(), ct);
        }
    }
    let over = |x: &mut Xml, part: &str, ct: &str| {
        x.empty("Override", &[("PartName", part), ("ContentType", ct)]);
    };
    over(
        &mut x,
        "/word/document.xml",
        &format!("{CT_MAIN}.document.main+xml"),
    );
    over(&mut x, "/word/styles.xml", &format!("{CT_MAIN}.styles+xml"));
    over(
        &mut x,
        "/word/numbering.xml",
        &format!("{CT_MAIN}.numbering+xml"),
    );
    over(
        &mut x,
        "/word/settings.xml",
        &format!("{CT_MAIN}.settings+xml"),
    );
    over(
        &mut x,
        "/word/fontTable.xml",
        &format!("{CT_MAIN}.fontTable+xml"),
    );
    for i in 0..doc.headers.len() {
        over(
            &mut x,
            &format!("/word/header{}.xml", i + 1),
            &format!("{CT_MAIN}.header+xml"),
        );
    }
    for i in 0..doc.footers.len() {
        over(
            &mut x,
            &format!("/word/footer{}.xml", i + 1),
            &format!("{CT_MAIN}.footer+xml"),
        );
    }
    if !doc.footnotes.is_empty() {
        over(
            &mut x,
            "/word/footnotes.xml",
            &format!("{CT_MAIN}.footnotes+xml"),
        );
    }
    over(
        &mut x,
        "/docProps/core.xml",
        "application/vnd.openxmlformats-package.core-properties+xml",
    );
    over(
        &mut x,
        "/docProps/app.xml",
        "application/vnd.openxmlformats-officedocument.extended-properties+xml",
    );
    x.close("Types");
    x.finish()
}

fn package_rels() -> Vec<u8> {
    let mut x = Xml::new();
    x.open(
        "Relationships",
        &[(
            "xmlns",
            "http://schemas.openxmlformats.org/package/2006/relationships",
        )],
    );
    x.empty(
        "Relationship",
        &[
            ("Id", "rId1"),
            ("Type", &format!("{REL}/officeDocument")),
            ("Target", "word/document.xml"),
        ],
    );
    x.empty(
        "Relationship",
        &[
            ("Id", "rId2"),
            (
                "Type",
                "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
            ),
            ("Target", "docProps/core.xml"),
        ],
    );
    x.empty(
        "Relationship",
        &[
            ("Id", "rId3"),
            ("Type", &format!("{REL}/extended-properties")),
            ("Target", "docProps/app.xml"),
        ],
    );
    x.close("Relationships");
    x.finish()
}

fn core_xml(info: &Info) -> Vec<u8> {
    let mut x = Xml::new();
    x.open(
        "cp:coreProperties",
        &[
            (
                "xmlns:cp",
                "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
            ),
            ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
        ],
    );
    if let Some(t) = &info.title {
        x.leaf("dc:title", &[], t);
    }
    if let Some(l) = &info.language {
        x.leaf("dc:language", &[], l);
    }
    x.close("cp:coreProperties");
    x.finish()
}

fn app_xml() -> Vec<u8> {
    let mut x = Xml::new();
    x.open(
        "Properties",
        &[(
            "xmlns",
            "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        )],
    );
    x.leaf("Application", &[], "Quillmark");
    x.close("Properties");
    x.finish()
}
