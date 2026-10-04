//! Base PDFs a test hands the spine: the traditional-xref page trees its input
//! contract admits, and the ones it refuses, which no fixture carries.
//!
//! Object ids are positional — 1 catalog, 2 page tree, a page/contents pair per
//! page from 3, then whatever a variant adds — so a test can splice bytes
//! against them (`3 0 obj` is the first page, and a one-page base ends at
//! `/Size 5`).

use pdf_writer::types::AnnotationType;
use pdf_writer::writers::Form;
use pdf_writer::{Content, Finish, Name, Null, Pdf, Rect, Ref, Settings, TextStr};

#[derive(Clone, Copy)]
enum Rotation {
    Direct(i32),
    Indirect(i32),
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
    acroform: bool,
    catalog_nulls: Vec<&'static str>,
    page_nulls: Vec<&'static str>,
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
            acroform: false,
            catalog_nulls: Vec::new(),
            page_nulls: Vec::new(),
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

    /// A catalog `/AcroForm`, which the spine refuses rather than replace.
    pub fn acroform(mut self) -> Self {
        self.acroform = true;
        self
    }

    /// `/key null` on the catalog, in place of any value this builder writes
    /// there.
    pub fn catalog_null(mut self, key: &'static str) -> Self {
        self.catalog_nulls.push(key);
        self
    }

    /// `/key null` on every page, in place of any value this builder writes
    /// there. The page tree keeps its own `/MediaBox`, so a page nulling its
    /// one inherits it.
    pub fn page_null(mut self, key: &'static str) -> Self {
        self.page_nulls.push(key);
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

        let media = rect(self.media_box);
        {
            let mut catalog = pdf.catalog(catalog_id);
            catalog.pages(page_tree_id);
            if let Some(id) = acroform_id.filter(|_| !self.catalog_nulls.contains(&"AcroForm")) {
                catalog.pair(Name(b"AcroForm"), id);
            }
            for key in &self.catalog_nulls {
                catalog.pair(Name(key.as_bytes()), Null);
            }
        }
        pdf.pages(page_tree_id)
            .kids(leaves.iter().map(|&(page, _)| page))
            .count(self.pages as i32)
            .media_box(media)
            .finish();

        for (i, &(page_id, content_id)) in leaves.iter().enumerate() {
            {
                let written = |key: &str| !self.page_nulls.contains(&key);
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
                if let (0, Some(id), true) = (i, annot_id, written("Annots")) {
                    page.annotations([id]);
                }
                for key in &self.page_nulls {
                    page.pair(Name(key.as_bytes()), Null);
                }
            }
            pdf.stream(content_id, &Content::new().finish());
        }

        if let Some(id) = annot_id {
            pdf.annotation(id)
                .subtype(AnnotationType::Text)
                .rect(Rect::new(10.0, 10.0, 30.0, 30.0));
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
        pdf.finish()
    }
}

fn rect([x0, y0, x1, y1]: [f32; 4]) -> Rect {
    Rect::new(x0, y0, x1, y1)
}
