//! The incremental-update envelope shared by the stamp and flatten paths: open a
//! base PDF (validate the reader's input contract, read the trailer, seed the
//! object-id counter, optionally stamp `/Info` `/Producer`) and close it with one
//! incremental-update append. Each path supplies only the objects in between.

use crate::error::PdfError;
use crate::reader::{
    append_incremental_update, assert_unrotated_pages, err, find_dict_value, open_trailer,
    read_info_source, walk_page_tree, ObjectIndex, Page, UpdatedObject,
};
use crate::writer::apply_producer_stamp;
use crate::FieldSpec;

const CODE_PARSE: &str = "pdf::update_parse";

/// One incremental-update revision in progress.
#[non_exhaustive]
pub struct PdfUpdate {
    xref_offset: usize,
    /// The base PDF's catalog (`/Root`) object id.
    pub catalog_id: u32,
    /// Next free object id, seeded at the trailer `/Size`. Hand out via
    /// [`alloc_id`](crate::writer::alloc_id).
    pub next_id: u32,
    /// Objects to write in this revision; callers push their own onto it.
    pub objects: Vec<UpdatedObject>,
    /// `Some` when the producer stamp allocated a fresh `/Info`, which
    /// [`finish`](PdfUpdate::finish) makes the new trailer's `/Info`, in place
    /// of whatever the base's trailer held.
    new_info_ref: Option<u32>,
}

impl PdfUpdate {
    /// Open the indexed base for an incremental update. The caller then pushes
    /// its objects onto [`objects`](Self::objects) and calls
    /// [`finish`](Self::finish) with the base's bytes.
    pub fn begin(idx: &ObjectIndex, producer: Option<&str>) -> Result<Self, PdfError> {
        let (xref_offset, trailer, catalog_id) = open_trailer(idx.bytes(), CODE_PARSE)?;
        if find_dict_value(trailer, "Encrypt").is_some() {
            return Err(err(
                "pdf::encrypted",
                "PDF is encrypted; the stamp spine does not handle encrypted PDFs",
            ));
        }
        // The new trailer re-references the catalog as `/Root <id> 0 R`, so a
        // non-zero-generation catalog would be silently corrupted even when only
        // the producer is stamped (catalog not itself overwritten).
        idx.assert_overwrite_gen_zero(catalog_id, "catalog (/Root)")?;
        let size = find_dict_value(trailer, "Size")
            .and_then(|v| std::str::from_utf8(v.trim_ascii()).ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| err(CODE_PARSE, "/Size missing or malformed in trailer"))?;
        // One counter seeded at `/Size`, so created ids never collide with the
        // base's. `alloc_id` bounds it: a malformed large `/Size` errors instead
        // of handing out an id that collides or that no reference admits.
        let mut next_id = size;
        let mut objects: Vec<UpdatedObject> = Vec::new();
        let mut new_info_ref = None;
        if let Some(producer) = producer {
            let info = read_info_source(trailer);
            new_info_ref = apply_producer_stamp(idx, info, producer, &mut next_id, &mut objects)?;
        }

        Ok(Self {
            xref_offset,
            catalog_id,
            next_id,
            objects,
            new_info_ref,
        })
    }

    /// Resolve the base's pages in document order, each carrying the ancestor
    /// chain its inheritable attributes resolve along, bounds-checking every
    /// field's `page` so a spec targeting a non-existent page errors rather than
    /// panicking later.
    pub fn resolve_pages(
        &self,
        idx: &ObjectIndex,
        fields: &[FieldSpec],
    ) -> Result<Vec<Page>, PdfError> {
        let pages = walk_page_tree(idx, self.catalog_id)?;
        let page_count = pages.len();
        let mut checked = vec![false; page_count];
        let mut targeted = Vec::new();
        for spec in fields {
            if spec.page >= page_count {
                return Err(err(
                    CODE_PARSE,
                    format!(
                        "field {:?} targets page {} but the PDF has {page_count} page(s)",
                        spec.name, spec.page
                    ),
                ));
            }
            if checked[spec.page] {
                continue;
            }
            // A targeted page is overwritten and referenced as gen 0, so a
            // non-zero-generation page would be silently corrupted.
            idx.assert_overwrite_gen_zero(pages[spec.page].id, "page")?;
            checked[spec.page] = true;
            targeted.push(&pages[spec.page]);
        }
        // Geometry is written in unrotated user space, so a rotated target page
        // would mis-place every field.
        assert_unrotated_pages(idx, targeted)?;
        Ok(pages)
    }

    /// Serialize the accumulated objects onto `pdf` via one incremental-update
    /// append, threading in a freshly-allocated `/Info` when there is one.
    pub fn finish(self, pdf: Vec<u8>) -> Result<Vec<u8>, PdfError> {
        append_incremental_update(
            pdf,
            self.xref_offset,
            self.catalog_id,
            self.next_id,
            self.new_info_ref,
            &self.objects,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{find_startxref, find_trailer_dict, parse_indirect_ref};

    /// A base whose whole trailer dict is `entries`, over one catalog object.
    fn base_with_trailer(entries: &str) -> Vec<u8> {
        let head = "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
        format!(
            "{head}xref\n0 1\n0000000000 65535 f \ntrailer\n<< {entries} >>\n\
             startxref\n{}\n%%EOF\n",
            head.len()
        )
        .into_bytes()
    }

    fn stamped(base: &[u8]) -> Vec<u8> {
        let idx = ObjectIndex::new(base);
        PdfUpdate::begin(&idx, Some("Quillmark test"))
            .expect("begin")
            .finish(base.to_vec())
            .expect("finish")
    }

    #[test]
    fn a_non_reference_trailer_info_becomes_one_reference_carrying_its_entries() {
        for (value, title) in [
            ("<< /Title (x) >>", Some(&b"(x)"[..])),
            ("(not a dictionary)", None),
        ] {
            let base = base_with_trailer(&format!("/Size 6 /Root 1 0 R /Info {value}"));
            let out = stamped(&base);

            let trailer =
                find_trailer_dict(&out, find_startxref(&out).unwrap()).expect("new trailer");
            assert_eq!(
                trailer.windows(5).filter(|w| *w == b"/Info").count(),
                1,
                "trailer: {}",
                String::from_utf8_lossy(trailer)
            );
            let (info_id, _) = find_dict_value(trailer, "Info")
                .and_then(parse_indirect_ref)
                .expect("/Info is an indirect reference");
            let info = ObjectIndex::new(&out)
                .dict(info_id, CODE_PARSE, "/Info")
                .expect("/Info dict");
            assert_eq!(
                find_dict_value(info, "Producer").unwrap().trim_ascii(),
                b"(Quillmark test)"
            );
            assert_eq!(
                find_dict_value(info, "Title").map(<[u8]>::trim_ascii),
                title
            );
        }
    }
}
