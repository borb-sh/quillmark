//! The host's font library: the registry tier between a quill's own fonts and
//! the embedded fallback, parsed once per engine and shared by its sessions.

use quillmark_core::error::{Diagnostic, RenderError, Severity};
use typst::foundations::Bytes;
use typst::text::Font;

use crate::world::{faces, MAX_COLLECTION_FACES};

/// The faces a host supplies, every one readable. Frozen at construction: a
/// session sees the library as it was when the engine was built.
#[derive(Debug, Default)]
pub struct FontLibrary {
    faces: Vec<Font>,
}

impl FontLibrary {
    /// Every face in `files`, each a `.ttf`, `.otf`, `.ttc` or `.otc` file's
    /// bytes, or one diagnostic per file refused: `engine::font_unsupported`
    /// for another format, `engine::font_unreadable` for a file of the right
    /// format holding no face Typst reads.
    pub fn new(files: Vec<Vec<u8>>) -> Result<Self, RenderError> {
        let mut faces_out = Vec::new();
        let mut refused = Vec::new();
        for (i, file) in files.into_iter().enumerate() {
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
            match faces(Bytes::new(file)) {
                Ok(read) => {
                    let read: Vec<Font> = read.collect();
                    if read.is_empty() {
                        refused.push(unreadable(i, "holds no face Typst reads".to_string()));
                    }
                    faces_out.extend(read);
                }
                Err(declared) => refused.push(unreadable(
                    i,
                    format!("declares {declared} faces, past the {MAX_COLLECTION_FACES} one file may hold"),
                )),
            }
        }
        if refused.is_empty() {
            Ok(FontLibrary { faces: faces_out })
        } else {
            Err(RenderError::new(refused))
        }
    }

    pub(crate) fn faces(&self) -> &[Font] {
        &self.faces
    }
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
