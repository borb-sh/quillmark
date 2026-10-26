//! The host's font library: the faces a `font_family` mark may set, apart from
//! a quill's own fonts and the embedded fallback, parsed once per engine and
//! shared by its sessions.

use std::sync::Arc;

use quillmark_core::error::{Diagnostic, RenderError, Severity};
use typst::foundations::Bytes;
use typst::text::Font;

use crate::world::{faces, MAX_COLLECTION_FACES};

/// The faces a host supplies, every one readable. Frozen at construction: a
/// session sees the library as it was when the engine was built.
#[derive(Debug, Default)]
pub struct FontLibrary {
    faces: Vec<Font>,
    /// Each face's file, the one buffer its `Font` reads.
    files: Vec<Arc<[u8]>>,
    /// Drawn at construction, so the family a library face is filed under is
    /// no name a plate can spell ahead of time.
    alias_key: u64,
}

impl FontLibrary {
    /// Every face in `files`, each a `.ttf`, `.otf`, `.ttc` or `.otc` file's
    /// bytes, or one diagnostic per file refused: `engine::font_unsupported`
    /// for another format, `engine::font_unreadable` for a file of the right
    /// format holding no face Typst reads. A collection keeps the faces Typst
    /// reads and drops the rest. Each file is held once, shared by its faces
    /// and by every [`FontFace`](quillmark_core::backend::FontFace) listing
    /// them.
    pub fn new<F: Into<Arc<[u8]>>>(files: impl IntoIterator<Item = F>) -> Result<Self, RenderError> {
        let mut library = FontLibrary {
            alias_key: alias_key(),
            ..FontLibrary::default()
        };
        let mut refused = Vec::new();
        for (i, file) in files.into_iter().enumerate() {
            let file: Arc<[u8]> = file.into();
            if let Some(format) = foreign_format(&file) {
                refused.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!("host font {i} is {format}, which the font library does not read"),
                    )
                    .with_code("engine::font_unsupported".to_string())
                    .with_hint("Supply the font as a .ttf, .otf, .ttc or .otc file.".to_string()),
                );
                continue;
            }
            match faces(Bytes::new(file.clone())) {
                Ok(read) => {
                    let before = library.faces.len();
                    library.faces.extend(read);
                    if library.faces.len() == before {
                        refused.push(unreadable(i, "holds no face Typst reads".to_string()));
                    }
                    library.files.resize(library.faces.len(), file);
                }
                Err(declared) => refused.push(unreadable(
                    i,
                    format!("declares {declared} faces, past the {MAX_COLLECTION_FACES} one file may hold"),
                )),
            }
        }
        if refused.is_empty() {
            Ok(library)
        } else {
            Err(RenderError::new(refused))
        }
    }

    pub(crate) fn faces(&self) -> &[Font] {
        &self.faces
    }

    /// Each face beside its file.
    pub(crate) fn faces_with_files(&self) -> impl Iterator<Item = (&Font, &Arc<[u8]>)> {
        self.faces.iter().zip(&self.files)
    }

    /// The family the font book files a library face of `family` under. No
    /// plate or fallback names it, so only a `font_family` mark, which lowers
    /// to it, reaches a library face.
    pub(crate) fn alias(&self, family: &str) -> String {
        format!("qm-host-{:016x}:{}", self.alias_key, family.to_lowercase())
    }
}

/// A key no plate can know: the OS's randomness, or failing that the
/// process's per-instance hash seed.
fn alias_key() -> u64 {
    getrandom::u64().unwrap_or_else(|_| {
        use std::hash::BuildHasher;
        std::hash::RandomState::new().hash_one(0u8)
    })
}

fn unreadable(i: usize, why: String) -> Diagnostic {
    Diagnostic::new(Severity::Error, format!("host font {i} {why}"))
        .with_code("engine::font_unreadable".to_string())
        .with_hint("Supply a TrueType or OpenType font file or collection.".to_string())
}

/// What `file` is when its signature is not a TrueType, OpenType or
/// collection one.
fn foreign_format(file: &[u8]) -> Option<&'static str> {
    match file.first_chunk::<4>() {
        Some([0, 1, 0, 0] | b"true" | b"OTTO" | b"ttcf") => None,
        Some(b"wOFF") => Some("a WOFF file"),
        Some(b"wOF2") => Some("a WOFF2 file"),
        _ => Some("not a font file"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use quillmark_core::backend::Backend;
    use quillmark_core::quill::{FileTreeNode, Quill};

    fn taro_font(name: &str) -> Vec<u8> {
        std::fs::read(quillmark_fixtures::quills_path("taro").join("assets/fonts").join(name)).expect("read font")
    }

    fn quill_shipping(fonts: &[(&str, Vec<u8>)]) -> Quill {
        let yaml = "quill:\n  name: t\n  version: 0.1.0\n  backend: typst\n  description: d\ntypst:\n  plate_file: plate.typ\n";
        let mut root = FileTreeNode::Directory { files: Default::default() };
        root.insert("Quill.yaml", FileTreeNode::File { contents: yaml.as_bytes().to_vec() }).unwrap();
        root.insert("plate.typ", FileTreeNode::File { contents: b"x".to_vec() }).unwrap();
        for (name, data) in fonts {
            root.insert(format!("assets/fonts/{name}"), FileTreeNode::File { contents: data.clone() }).unwrap();
        }
        Quill::from_tree(root).expect("quill")
    }

    /// The faces a session lists, as `(family, weight)`.
    fn faces_of(quill: &Quill, library: FontLibrary) -> Vec<(String, u16)> {
        crate::TypstBackend::new(Arc::new(library))
            .font_faces(quill)
            .into_iter()
            .map(|f| (f.family, f.weight))
            .collect()
    }

    /// A session lists the library's faces alone: the quill's own and the
    /// fallback set its plate, never a mark.
    #[test]
    fn a_session_lists_only_the_librarys_faces() {
        let host = || FontLibrary::new(vec![taro_font("Figtree-Bold.ttf")]).expect("library");
        let shipping = quill_shipping(&[("Figtree-Regular.ttf", taro_font("Figtree-Regular.ttf"))]);
        assert_eq!(faces_of(&shipping, host()), [("Figtree".into(), 700)]);
        assert_eq!(faces_of(&quill_shipping(&[]), host()), [("Figtree".into(), 700)]);
        assert_eq!(faces_of(&shipping, FontLibrary::default()), []);
    }

    /// Every listing hands out the library's own buffer, never a copy.
    #[test]
    fn a_listed_face_shares_the_librarys_file() {
        let backend = crate::TypstBackend::new(Arc::new(
            FontLibrary::new(vec![taro_font("Figtree-Regular.ttf")]).expect("library"),
        ));
        let quill = quill_shipping(&[]);
        let (first, second) = (backend.font_faces(&quill), backend.font_faces(&quill));
        assert!(Arc::ptr_eq(&first[0].data, &second[0].data));
        assert_eq!(&first[0].data[..], &taro_font("Figtree-Regular.ttf")[..]);
    }

    #[test]
    fn a_host_font_in_another_format_is_refused_by_its_signature() {
        let roboto = crate::world::FALLBACK_FONTS[0].1.to_vec();
        assert_eq!(FontLibrary::new(vec![roboto.clone()]).unwrap().faces().len(), 1);
        let err = FontLibrary::new(vec![
            roboto,
            b"wOF2rest".to_vec(),
            b"wOFFrest".to_vec(),
            b"<svg".to_vec(),
            b"OTTOtruncated".to_vec(),
        ])
        .unwrap_err();
        let codes: Vec<&str> = err.diagnostics().iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(
            codes,
            ["engine::font_unsupported", "engine::font_unsupported", "engine::font_unsupported", "engine::font_unreadable"],
            "one refusal per file past the readable first, in file order"
        );
    }
}
