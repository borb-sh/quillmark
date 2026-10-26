//! Family resolution at compile: a `font_family` mark naming a family the
//! host's font library lacks sets no family, and warns once per field.

use std::collections::BTreeMap;

use quillmark_core::error::{Diagnostic, Severity};
use quillmark_core::path::DocPath;
use quillmark_core::quill::QuillConfig;
use quillmark_core::Content;

/// A session's running-text families, lowercased.
#[derive(Debug, Default, Clone)]
pub(crate) struct Families {
    /// The families a plate can name: the quill's and the fallback's.
    pub(crate) held: std::collections::BTreeSet<String>,
    /// The library's families, the ones a `font_family` mark may set, each
    /// beside the [`FontLibrary::alias`](crate::FontLibrary::alias) the book
    /// files its faces under.
    pub(crate) offered: BTreeMap<String, String>,
}

const CODE: &str = "typst::unknown_font_family";

/// One `typst::unknown_font_family` per family a content field of the plate
/// JSON `data` names and `families` does not offer, spelled as the field
/// first does.
pub(crate) fn unknown(config: &QuillConfig, data: &serde_json::Value, families: &Families) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    config.each_plate_content(data, &mut |at: &DocPath, content: &Content| {
        let mut missing: BTreeMap<String, String> = BTreeMap::new();
        for name in content.font_families() {
            let key = name.to_lowercase();
            if !families.offered.contains_key(&key) {
                missing.entry(key).or_insert(name);
            }
        }
        for name in missing.into_values() {
            diags.push(
                Diagnostic::new(
                    Severity::Warning,
                    format!("this field sets text in `{name}`, a family the host's font library does not hold, so that text keeps the surrounding font"),
                )
                .with_code(CODE.to_string())
                .with_arg("family", name.clone().into())
                .with_path(at.to_string())
                .with_hint("add the family to the host's font library, or choose a family it holds".to_string()),
            );
        }
    });
    diags
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use quillmark_core::quill::{FileTreeNode, Quill};
    use typst::layout::{Frame, FrameItem};

    use crate::{compile, read_plate, world, FontLibrary, SchemaMeta};

    const YAML: &str = "quill:\n  name: fonts\n  version: 0.1.0\n  backend: typst\n  description: font runs\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    prose:\n      type: richtext\n      multiline: true\n      description: body\n";

    fn figtree() -> Vec<u8> {
        std::fs::read(quillmark_fixtures::quills_path("taro").join("assets/fonts/Figtree-Regular.ttf"))
            .expect("read Figtree")
    }

    fn roboto() -> Vec<u8> {
        crate::world::FALLBACK_FONTS[0].1.to_vec()
    }

    fn quill(plate_head: &str) -> Quill {
        quill_shipping(plate_head, &[])
    }

    fn quill_shipping(plate_head: &str, fonts: &[(&str, Vec<u8>)]) -> Quill {
        let plate = format!(
            "#import \"@local/quillmark-helper:0.2.0\" as qm\n#set page(width: 400pt, height: 300pt, margin: 20pt)\n{plate_head}\n#qm.data.prose\n"
        );
        let mut root = FileTreeNode::Directory { files: HashMap::new() };
        root.insert("Quill.yaml", FileTreeNode::File { contents: YAML.as_bytes().to_vec() }).unwrap();
        root.insert("plate.typ", FileTreeNode::File { contents: plate.into_bytes() }).unwrap();
        for (name, data) in fonts {
            root.insert(format!("assets/fonts/{name}"), FileTreeNode::File { contents: data.clone() }).unwrap();
        }
        Quill::from_tree(root).expect("quill")
    }

    fn data(md: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(md).expect("import").content;
        serde_json::json!({ "prose": quillmark_content::serial::to_canonical_value(&rt) })
    }

    /// Each run of text `prose` places, as `(text, family)`, and the session's
    /// warnings.
    fn runs(plate_head: &str, md: &str, library: FontLibrary) -> (Vec<(String, String)>, Vec<quillmark_core::error::Diagnostic>) {
        runs_of(&quill(plate_head), md, library)
    }

    fn runs_of(quill: &Quill, md: &str, library: FontLibrary) -> (Vec<(String, String)>, Vec<quillmark_core::error::Diagnostic>) {
        let data = data(md);
        let meta = SchemaMeta::from_schema_json(quillmark_core::quill::build_transform_schema(quill.config()).as_json());
        let mut world = world::QuillWorld::new(quill, &read_plate(quill).expect("plate"), &Arc::new(library))
            .expect("world");
        world.inject_helper_package(&data, &meta).expect("inject");
        let (document, mut warnings) = compile::compile_document(&world).expect("compile");
        warnings.extend(super::unknown(quill.config(), &data, world.families()));

        fn walk(frame: &Frame, out: &mut Vec<(String, String)>) {
            for (_, item) in frame.items() {
                match item {
                    FrameItem::Group(g) => walk(&g.frame, out),
                    FrameItem::Text(t) => out.push((t.text.to_string(), t.font.info().family.clone())),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        for page in document.pages() {
            walk(&page.frame, &mut out);
        }
        (out, warnings)
    }

    fn family<'a>(runs: &'a [(String, String)], text: &str) -> &'a str {
        let (_, family) = runs.iter().find(|r| r.0.contains(text)).unwrap_or_else(|| panic!("{text:?} in {runs:?}"));
        family
    }

    /// A run sets its family ahead of the surrounding fonts, a nested family
    /// sets its own run and the outer resumes past it, and a family the
    /// session lacks keeps the surrounding font under one warning per field.
    #[test]
    fn a_run_sets_its_family_and_an_inner_one_overrides_it() {
        let library = FontLibrary::new(vec![figtree(), roboto()]).expect("library");
        let md = "plain <qm-span family=\"figtree\">outer <qm-span family=\"Roboto\">inner</qm-span> after `mono`</qm-span> \
                  <qm-span family=\"Nope\">gone</qm-span> <qm-span family=\"Nope\">kept</qm-span>";
        let (runs, warnings) = runs("", md, library);
        assert_eq!(family(&runs, "plain"), "Roboto");
        assert_eq!(family(&runs, "outer"), "Figtree");
        assert_eq!(family(&runs, "inner"), "Roboto");
        assert_eq!(family(&runs, "after"), "Figtree");
        assert_ne!(family(&runs, "mono"), "Figtree", "a code span keeps raw's own font: {runs:?}");
        assert_eq!(family(&runs, "gone"), "Roboto");
        let codes: Vec<(&str, &str)> = warnings
            .iter()
            .map(|d| (d.code.as_deref().unwrap_or(""), d.message.as_str()))
            .collect();
        assert_eq!(codes.len(), 1, "one warning for the field, none of Typst's own: {codes:?}");
        assert_eq!(codes[0].0, "typst::unknown_font_family");
        assert_eq!(warnings[0].path.as_deref(), Some("main.prose"));
        assert_eq!(warnings[0].args.get("family"), Some(&serde_json::json!("Nope")));
    }

    /// A family entering on `update` is checked as one present at `open` is.
    #[test]
    fn an_update_checks_the_families_it_brings() {
        use quillmark_core::backend::Backend;
        let today = quillmark_core::quill::CalendarDate::new(2026, 1, 1).expect("a calendar day");
        let mut session = crate::TypstBackend::default().open(&quill(""), &data("plain"), today).expect("open");
        let unknown = |warnings: &[quillmark_core::error::Diagnostic]| {
            warnings.iter().filter(|d| d.code.as_deref() == Some(super::CODE)).count()
        };
        assert_eq!(unknown(session.warnings()), 0);
        session.update_data(&data("<qm-span family=\"Nope\">a</qm-span>")).expect("update");
        assert_eq!(unknown(session.warnings()), 1);
    }

    /// A family only the quill or the fallback ships sets its plate, never a
    /// run: the library alone offers families.
    #[test]
    fn a_family_the_library_lacks_keeps_the_surrounding_font() {
        let quill = quill_shipping("", &[("Figtree-Regular.ttf", figtree())]);
        let (runs, warnings) = runs_of(
            &quill,
            "plain <qm-span family=\"Figtree\">own</qm-span> <qm-span family=\"Roboto\">fallback</qm-span>",
            FontLibrary::default(),
        );
        assert_eq!(family(&runs, "plain"), "Figtree");
        assert_eq!(family(&runs, "own"), "Figtree");
        let families: Vec<_> = warnings.iter().filter_map(|d| d.args.get("family")).collect();
        assert_eq!(families, [&serde_json::json!("Figtree"), &serde_json::json!("Roboto")]);
    }

    /// A library face sets only text a mark sets: neither a plate naming its
    /// family nor glyph fallback reaches it.
    #[test]
    fn a_library_face_sets_only_marked_text() {
        let (runs, _) = runs(
            "#set text(font: \"Figtree\")",
            "plain <qm-span family=\"Figtree\">set</qm-span>",
            FontLibrary::new(vec![figtree()]).expect("library"),
        );
        assert_eq!(family(&runs, "plain"), "Roboto");
        assert_eq!(family(&runs, "set"), "Figtree");

        let quill = quill_shipping("", &[("Figtree-Regular.ttf", figtree())]);
        let (runs, _) = runs_of(&quill, "plain Ж", FontLibrary::new(vec![roboto()]).expect("library"));
        assert!(runs.iter().all(|(_, family)| family == "Figtree"), "{runs:?}");
    }

    /// The family a library face is filed under is drawn per library, so a
    /// plate spelling an alias reaches no library face.
    #[test]
    fn a_plate_cannot_spell_a_library_faces_alias() {
        let library = || FontLibrary::new(vec![figtree()]).expect("library");
        assert_ne!(library().alias("Figtree"), library().alias("Figtree"));
        let (runs, _) = runs(
            "#set text(font: (\"qm-host:figtree\", \"Roboto\"))",
            "plain <qm-span family=\"Figtree\">set</qm-span>",
            library(),
        );
        assert_eq!(family(&runs, "plain"), "Roboto");
        assert_eq!(family(&runs, "set"), "Figtree");
    }

    #[test]
    fn a_math_face_is_no_family_a_run_sets() {
        let (runs, warnings) = runs(
            "",
            "<qm-span family=\"Noto Sans Math\">abc</qm-span> <qm-span family=\"New Computer Modern Math\">def</qm-span>",
            FontLibrary::default(),
        );
        assert_eq!(family(&runs, "abc"), "Roboto");
        assert_eq!(family(&runs, "def"), "Roboto");
        let families: Vec<_> = warnings.iter().filter_map(|d| d.args.get("family")).collect();
        assert_eq!(families, [&serde_json::json!("New Computer Modern Math"), &serde_json::json!("Noto Sans Math")]);
    }

    #[test]
    fn a_family_falls_through_to_the_plates_fonts_for_what_it_lacks() {
        let library = FontLibrary::new(vec![figtree()]).expect("library");
        let (runs, _) = runs(
            "#set text(font: (\"Roboto\", \"Nowhere\"))",
            "<qm-span family=\"Figtree\">x中</qm-span>",
            library,
        );
        assert_eq!(family(&runs, "x"), "Figtree");
    }

    #[test]
    fn a_footnote_reference_takes_its_run_and_its_note_only_its_own_marks() {
        let library = FontLibrary::new(vec![figtree()]).expect("library");
        let (runs, _) = runs(
            "",
            "a <qm-span family=\"Figtree\">b[^1] c</qm-span>\n\n[^1]: note <qm-span family=\"Figtree\">own</qm-span>",
            library,
        );
        let families: Vec<(&str, &str)> = runs.iter().map(|r| (r.0.as_str(), r.1.as_str())).collect();
        assert_eq!(
            families,
            [("a ", "Roboto"), ("b", "Figtree"), ("1", "Figtree"), (" c", "Figtree"), ("1", "Roboto"), ("note ", "Roboto"), ("own", "Figtree")]
        );
    }

    #[test]
    fn an_equation_breaks_the_run() {
        let library = FontLibrary::new(vec![figtree()]).expect("library");
        let (runs, _) = runs("", "<qm-span family=\"Figtree\">a $x$ b</qm-span>", library);
        assert_eq!(family(&runs, "a"), "Figtree");
        assert_ne!(family(&runs, "𝑥"), "Figtree", "{runs:?}");
        assert_eq!(family(&runs, "b"), "Figtree");
    }
}
