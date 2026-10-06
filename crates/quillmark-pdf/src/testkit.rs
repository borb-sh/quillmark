//! Base PDFs a test hands the spine: the traditional-xref page trees its input
//! contract admits, and the ones it refuses, which no fixture carries.
//!
//! Object ids are positional — 1 catalog, 2 page tree, a page/contents pair per
//! page from 3, then whatever a variant adds — so a test can splice bytes
//! against them (`3 0 obj` is the first page, and a one-page base ends at
//! `/Size 5`).

use pdf_writer::types::AnnotationType;
use pdf_writer::writers::{Annotation, Form};
use pdf_writer::{Array, Content, Finish, Name, Null, Pdf, Rect, Ref, Settings, TextStr};

/// The id [`BasePdf::null_object`] writes a `null` under, past every id a test
/// base numbers positionally.
pub const NULL_OBJECT: i32 = 99;

/// An id below [`NULL_OBJECT`] that no testkit base writes an object under.
pub const NO_OBJECT: i32 = 98;

#[derive(Clone, Copy)]
enum Rotation {
    Direct(i32),
    Indirect(i32),
}

/// How an `/Annots` array holds an annotation dictionary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Written in the array.
    Inline,
    /// An object the array references.
    Referenced,
}

#[derive(Clone, Copy)]
struct Annot {
    page: usize,
    subtype: AnnotationType,
    held: Held,
}

/// One element of a page's `/Annots`.
enum Entry {
    Reference(Ref),
    Inline(AnnotationType),
}

/// A base PDF under construction: US Letter, `pages` pages, each drawing
/// nothing.
#[derive(Clone)]
pub struct BasePdf {
    pages: usize,
    media_box: [f32; 4],
    crop_box: Option<[f32; 4]>,
    rotation: Option<Rotation>,
    info_title: Option<String>,
    inline_annot: bool,
    annots: Vec<Annot>,
    indirect_annots: bool,
    acroform: bool,
    catalog_entries: Vec<(&'static str, Vec<u8>)>,
    page_entries: Vec<(&'static str, Vec<u8>)>,
    null_object: bool,
    pretty: bool,
}

impl BasePdf {
    pub fn letter(pages: usize) -> Self {
        BasePdf {
            pages,
            media_box: [0.0, 0.0, 612.0, 792.0],
            crop_box: None,
            rotation: None,
            info_title: None,
            inline_annot: false,
            annots: Vec::new(),
            indirect_annots: false,
            acroform: false,
            catalog_entries: Vec::new(),
            page_entries: Vec::new(),
            null_object: false,
            pretty: true,
        }
    }

    pub fn media_box(mut self, media_box: [f32; 4]) -> Self {
        self.media_box = media_box;
        self
    }

    pub fn crop_box(mut self, crop_box: [f32; 4]) -> Self {
        self.crop_box = Some(crop_box);
        self
    }

    /// `/Rotate deg` on every page.
    pub fn rotate(mut self, deg: i32) -> Self {
        self.rotation = Some(Rotation::Direct(deg));
        self
    }

    /// `/Rotate` as an indirect reference resolving to `deg`: a rotation the
    /// reader cannot read off the page dictionary.
    pub fn indirect_rotate(mut self, deg: i32) -> Self {
        self.rotation = Some(Rotation::Indirect(deg));
        self
    }

    /// An `/Info` whose producer is `Base` and whose last key is `title`.
    pub fn info_title(mut self, title: &str) -> Self {
        self.info_title = Some(title.into());
        self
    }

    /// One text annotation already in the first page's inline `/Annots`, under
    /// [`inline_annot_id`](Self::inline_annot_id).
    pub fn inline_annot(mut self) -> Self {
        self.inline_annot = true;
        self
    }

    /// A `subtype` annotation last in page `page`'s `/Annots`, held as `held`.
    pub fn annot(mut self, page: usize, subtype: AnnotationType, held: Held) -> Self {
        self.annots.push(Annot { page, subtype, held });
        self
    }

    /// Each page's `/Annots` as a reference to an array object, which the
    /// input contract refuses on a page the spine stamps.
    pub fn indirect_annots(mut self) -> Self {
        self.indirect_annots = true;
        self
    }

    /// A catalog `/AcroForm`, which the spine refuses rather than replace.
    pub fn acroform(mut self) -> Self {
        self.acroform = true;
        self
    }

    /// `/key` followed by `value`'s bytes on the catalog, in place of any value
    /// this builder writes there: a spelling pdf-writer does not write, such as
    /// a NUL before the value or a comment glued to it.
    pub fn catalog_raw(mut self, key: &'static str, value: impl Into<Vec<u8>>) -> Self {
        self.catalog_entries.push((key, value.into()));
        self
    }

    /// [`catalog_raw`](Self::catalog_raw) on every page. The page tree keeps
    /// its own `/MediaBox`, so a page nulling its one inherits it.
    pub fn page_raw(mut self, key: &'static str, value: impl Into<Vec<u8>>) -> Self {
        self.page_entries.push((key, value.into()));
        self
    }

    /// A `null` object under [`NULL_OBJECT`], for an entry to reference.
    pub fn null_object(mut self) -> Self {
        self.null_object = true;
        self
    }

    /// pdf-writer's compact mode, which hex-encodes a non-ASCII string.
    pub fn compact(mut self) -> Self {
        self.pretty = false;
        self
    }

    /// The id [`inline_annot`](Self::inline_annot) writes its annotation under.
    pub fn inline_annot_id(&self) -> i32 {
        3 + 2 * self.pages as i32
    }

    pub fn build(&self) -> Vec<u8> {
        let mut pdf = Pdf::with_settings(Settings {
            pretty: self.pretty,
        });
        let catalog_id = Ref::new(1);
        let page_tree_id = Ref::new(2);
        let leaves: Vec<(Ref, Ref)> = (0..self.pages)
            .map(|i| {
                let first = 3 + 2 * i as i32;
                (Ref::new(first), Ref::new(first + 1))
            })
            .collect();

        let mut next = self.inline_annot_id();
        let mut alloc = |wanted: bool| {
            wanted.then(|| {
                let id = Ref::new(next);
                next += 1;
                id
            })
        };
        let annot_id = alloc(self.inline_annot);
        let acroform_id = alloc(self.acroform);
        let rotate_id = alloc(matches!(self.rotation, Some(Rotation::Indirect(_))));
        let info_id = alloc(self.info_title.is_some());
        let annot_ids: Vec<Option<Ref>> = self
            .annots
            .iter()
            .map(|annot| alloc(annot.held == Held::Referenced))
            .collect();
        let entries: Vec<Vec<Entry>> = (0..self.pages)
            .map(|i| {
                let text_annot = annot_id.filter(|_| i == 0).map(Entry::Reference);
                let added = self
                    .annots
                    .iter()
                    .zip(&annot_ids)
                    .filter(|(annot, _)| annot.page == i)
                    .map(|(annot, id)| match *id {
                        Some(id) => Entry::Reference(id),
                        None => Entry::Inline(annot.subtype),
                    });
                text_annot.into_iter().chain(added).collect()
            })
            .collect();
        let array_ids: Vec<Option<Ref>> = entries
            .iter()
            .map(|entries| alloc(self.indirect_annots && !entries.is_empty()))
            .collect();

        let media = rect(self.media_box);
        {
            let mut catalog = pdf.catalog(catalog_id);
            catalog.pages(page_tree_id);
            if let Some(id) = acroform_id.filter(|_| !names(&self.catalog_entries, "AcroForm")) {
                catalog.pair(Name(b"AcroForm"), id);
            }
        }
        pdf.pages(page_tree_id)
            .kids(leaves.iter().map(|&(page, _)| page))
            .count(self.pages as i32)
            .media_box(media)
            .finish();

        for (i, &(page_id, content_id)) in leaves.iter().enumerate() {
            {
                let written = |key: &str| !names(&self.page_entries, key);
                let mut page = pdf.page(page_id);
                page.parent(page_tree_id).contents(content_id);
                if written("MediaBox") {
                    page.media_box(media);
                }
                if let Some(crop) = self.crop_box.filter(|_| written("CropBox")) {
                    page.pair(Name(b"CropBox"), rect(crop));
                }
                match self.rotation.filter(|_| written("Rotate")) {
                    Some(Rotation::Direct(deg)) => {
                        page.rotate(deg);
                    }
                    Some(Rotation::Indirect(_)) => {
                        page.pair(Name(b"Rotate"), rotate_id.expect("indirect /Rotate id"));
                    }
                    None => {}
                }
                if written("Annots") && !entries[i].is_empty() {
                    match array_ids[i] {
                        Some(id) => {
                            page.pair(Name(b"Annots"), id);
                        }
                        None => write_entries(page.insert(Name(b"Annots")).array(), &entries[i]),
                    }
                }
            }
            pdf.stream(content_id, &Content::new().finish());
            if let Some(id) = array_ids[i] {
                write_entries(pdf.indirect(id).array(), &entries[i]);
            }
        }

        if let Some(id) = annot_id {
            pdf.annotation(id)
                .subtype(AnnotationType::Text)
                .rect(Rect::new(10.0, 10.0, 30.0, 30.0));
        }
        for (annot, id) in self.annots.iter().zip(&annot_ids) {
            if let Some(id) = *id {
                annotate(pdf.annotation(id), annot.subtype);
            }
        }
        if let Some(id) = acroform_id {
            pdf.indirect(id).start::<Form>().fields([]).finish();
        }
        if let (Some(id), Some(Rotation::Indirect(deg))) = (rotate_id, self.rotation) {
            pdf.indirect(id).primitive(deg);
        }
        if let (Some(id), Some(title)) = (info_id, self.info_title.as_deref()) {
            pdf.document_info(id)
                .producer(TextStr("Base"))
                .title(TextStr(title));
        }
        if self.null_object {
            pdf.indirect(Ref::new(NULL_OBJECT)).primitive(Null);
        }
        let mut bytes = pdf.finish();
        for (key, value) in &self.catalog_entries {
            bytes = insert_entry(&bytes, catalog_id, key, value);
        }
        for &(page_id, _) in &leaves {
            for (key, value) in &self.page_entries {
                bytes = insert_entry(&bytes, page_id, key, value);
            }
        }
        bytes
    }
}

/// The bytes after a key that ISO 32000-1 reads as `null`, for
/// [`BasePdf::catalog_raw`] and [`BasePdf::page_raw`]: the keyword, after a NUL
/// as its white-space or before a comment glued to it (§7.2.2), and a reference
/// to a `null` object or to no object (§7.3.10), which a base built with
/// [`BasePdf::null_object`] resolves.
pub fn null_spellings() -> Vec<Vec<u8>> {
    vec![
        b" null".to_vec(),
        b"\0null".to_vec(),
        b" null%stripped\n".to_vec(),
        format!(" {NULL_OBJECT} 0 R").into_bytes(),
        format!(" {NO_OBJECT} 0 R").into_bytes(),
    ]
}

fn rect([x0, y0, x1, y1]: [f32; 4]) -> Rect {
    Rect::new(x0, y0, x1, y1)
}

fn write_entries(mut array: Array<'_>, entries: &[Entry]) {
    for entry in entries {
        match *entry {
            Entry::Reference(id) => {
                array.item(id);
            }
            Entry::Inline(subtype) => annotate(array.push().start::<Annotation>(), subtype),
        }
    }
}

fn annotate(mut annot: Annotation<'_>, subtype: AnnotationType) {
    annot.subtype(subtype).rect(Rect::new(10.0, 40.0, 30.0, 60.0));
}

fn names(entries: &[(&str, Vec<u8>)], key: &str) -> bool {
    entries.iter().any(|&(name, _)| name == key)
}

/// `pdf` with `/key` and `value` written last in object `id`'s dictionary, and
/// every xref offset past them and `startxref` moved to match.
fn insert_entry(pdf: &[u8], id: Ref, key: &str, value: &[u8]) -> Vec<u8> {
    let object = find(pdf, format!("\n{} 0 obj\n", id.get()).as_bytes());
    let endobj = object + find(&pdf[object..], b"endobj");
    let at = object + rfind(&pdf[object..endobj], b">>");
    let entry = [format!(" /{key}").as_bytes(), value].concat();
    let mut out = [&pdf[..at], &entry, &pdf[at..]].concat();

    let xref = rfind(&out, b"\nxref\n") + 1;
    let header = xref + b"xref\n".len();
    let rows = header + find(&out[header..], b"\n") + 1;
    let trailer = rows + find(&out[rows..], b"trailer");
    // pdf-writer's rows are `OOOOOOOOOO GGGGG n\r\n`.
    for row in out[rows..trailer].chunks_exact_mut(20) {
        let offset: usize = std::str::from_utf8(&row[..10]).unwrap().parse().unwrap();
        if row[17] == b'n' && offset > at {
            row[..10].copy_from_slice(format!("{:010}", offset + entry.len()).as_bytes());
        }
    }
    let startxref = rfind(&out, b"startxref\n") + b"startxref\n".len();
    let digits = out[startxref..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    out.splice(startxref..startxref + digits, xref.to_string().into_bytes());
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("a testkit base carries the needle")
}

fn rfind(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
        .expect("a testkit base carries the needle")
}
