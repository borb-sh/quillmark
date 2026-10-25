//! Family resolution at compile: a `font_family` mark naming a family the
//! session's fonts lack sets no family, and warns once per field.

use std::collections::BTreeMap;

use quillmark_core::error::{Diagnostic, Severity};
use quillmark_core::path::DocPath;
use quillmark_core::quill::QuillConfig;
use quillmark_core::Content;

/// The families a session sets running text in, lowercased.
pub(crate) type Families = std::collections::BTreeSet<String>;

const CODE: &str = "typst::unknown_font_family";

/// One `typst::unknown_font_family` per family a content field of the plate
/// JSON `data` names and `families` lacks, spelled as the field first does.
pub(crate) fn unknown(config: &QuillConfig, data: &serde_json::Value, families: &Families) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    config.each_plate_content(data, &mut |at: &DocPath, content: &Content| {
        let mut missing: BTreeMap<String, String> = BTreeMap::new();
        for name in content.font_families() {
            let key = name.to_lowercase();
            if !families.contains(&key) {
                missing.entry(key).or_insert(name);
            }
        }
        for name in missing.into_values() {
            diags.push(
                Diagnostic::new(
                    Severity::Warning,
                    format!("this field sets text in `{name}`, a family none of the session's fonts holds, so that text keeps the surrounding font"),
                )
                .with_code(CODE.to_string())
                .with_arg("family", name.clone().into())
                .with_path(at.to_string())
                .with_hint("ship the family in the quill or the host's font library, or choose a family they hold".to_string()),
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

    fn quill(plate_head: &str) -> Quill {
        let plate = format!(
            "#import \"@local/quillmark-helper:0.2.0\" as qm\n#set page(width: 400pt, height: 300pt, margin: 20pt)\n{plate_head}\n#qm.data.prose\n"
        );
        let mut files = HashMap::new();
        files.insert("Quill.yaml".to_string(), FileTreeNode::File { contents: YAML.as_bytes().to_vec() });
        files.insert("plate.typ".to_string(), FileTreeNode::File { contents: plate.into_bytes() });
        Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
    }

    fn data(md: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(md).expect("import").content;
        serde_json::json!({ "prose": quillmark_content::serial::to_canonical_value(&rt) })
    }

    /// Each run of text `prose` places, as `(text, family)`, and the session's
    /// warnings.
    fn runs(plate_head: &str, md: &str, library: FontLibrary) -> (Vec<(String, String)>, Vec<quillmark_core::error::Diagnostic>) {
        let quill = quill(plate_head);
        let data = data(md);
        let meta = SchemaMeta::from_schema_json(quillmark_core::quill::build_transform_schema(quill.config()).as_json());
        let mut world = world::QuillWorld::new(&quill, &read_plate(&quill).expect("plate"), &Arc::new(library))
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
    fn a_run_sets_the_family_that_ranks_first_over_it() {
        let library = FontLibrary::new(vec![figtree()]).expect("library");
        let md = "plain <qm-font family=\"figtree\">outer <qm-font family=\"Roboto\">inner</qm-font> after `mono`</qm-font> \
                  <qm-font family=\"Nope\">gone</qm-font> <qm-font family=\"Nope\">kept</qm-font>";
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
        session.update_data(&data("<qm-font family=\"Nope\">a</qm-font>")).expect("update");
        assert_eq!(unknown(session.warnings()), 1);
    }

    #[test]
    fn a_math_face_is_no_family_a_run_sets() {
        let (runs, warnings) = runs(
            "",
            "<qm-font family=\"Noto Sans Math\">abc</qm-font> <qm-font family=\"New Computer Modern Math\">def</qm-font>",
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
            "<qm-font family=\"Figtree\">x中</qm-font>",
            library,
        );
        assert_eq!(family(&runs, "x"), "Figtree");
    }

    #[test]
    fn a_footnote_reference_takes_its_run_and_its_note_only_its_own_marks() {
        let library = FontLibrary::new(vec![figtree()]).expect("library");
        let (runs, _) = runs(
            "",
            "a <qm-font family=\"Figtree\">b[^1] c</qm-font>\n\n[^1]: note <qm-font family=\"Figtree\">own</qm-font>",
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
        let (runs, _) = runs("", "<qm-font family=\"Figtree\">a $x$ b</qm-font>", library);
        assert_eq!(family(&runs, "a"), "Figtree");
        assert_ne!(family(&runs, "𝑥"), "Figtree", "{runs:?}");
        assert_eq!(family(&runs, "b"), "Figtree");
    }
}
