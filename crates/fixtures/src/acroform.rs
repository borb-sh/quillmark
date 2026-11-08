//! AcroForm readers over a reparsed PDF: catalog → `/AcroForm` → `/Fields` →
//! `/T`.

use std::collections::HashMap;

use lopdf::{Dictionary, Document};

/// The catalog's `/AcroForm` dict.
pub fn acroform(doc: &Document) -> &Dictionary {
    let af_ref = doc
        .catalog()
        .expect("catalog")
        .get(b"AcroForm")
        .expect("/AcroForm")
        .as_reference()
        .expect("AcroForm indirect");
    doc.get_object(af_ref).unwrap().as_dict().unwrap()
}

/// Each `/Fields` widget of `af`, with its raw `/T`.
pub fn fields<'a>(
    doc: &'a Document,
    af: &'a Dictionary,
) -> impl Iterator<Item = (&'a [u8], &'a Dictionary)> {
    af.get(b"Fields")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let w = doc
                .get_object(f.as_reference().unwrap())
                .unwrap()
                .as_dict()
                .unwrap();
            (w.get(b"T").unwrap().as_str().unwrap(), w)
        })
}

/// The widget named `name`.
pub fn widget<'a>(doc: &'a Document, af: &'a Dictionary, name: &str) -> &'a Dictionary {
    fields(doc, af)
        .find(|(t, _)| *t == name.as_bytes())
        .map(|(_, w)| w)
        .unwrap_or_else(|| panic!("no field named {name}"))
}

/// The reparsed `pdf`, its AcroForm dict, and a `/T` to widget map.
pub fn parse(pdf: &[u8]) -> (Document, Dictionary, HashMap<String, Dictionary>) {
    let doc = Document::load_mem(pdf).expect("reparse");
    let af = acroform(&doc).clone();
    let by_name = fields(&doc, &af)
        .map(|(t, w)| (String::from_utf8_lossy(t).into_owned(), w.clone()))
        .collect();
    (doc, af, by_name)
}
