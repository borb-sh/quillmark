//! Which file an acroform quill's field spec is read from, and how a bad one
//! is reported.

use quillmark::{Document, Engine, OutputFormat, RenderError, RenderOptions};
use quillmark_core::quill::{FileTreeNode, Quill};
use quillmark_fixtures::acroform::{acroform, fields, widget};

mod common;

const DOC: &str = "~~~\n$quill: sample_form\n$kind: main\nfull_name: Ada Lovelace\n~~~\n";

/// `sample_form`'s tree with `Form.yaml` dropped and `files` added.
fn sample_form_with(files: &[(&str, &str)]) -> FileTreeNode {
    let mut tree = quillmark::tree_from_path(quillmark_fixtures::quills_path("sample_form"))
        .expect("load sample_form tree");
    let FileTreeNode::Directory { files: root } = &mut tree else {
        unreachable!("a quill tree is a directory")
    };
    root.remove("Form.yaml")
        .expect("the fixture ships Form.yaml");
    for (path, contents) in files {
        tree.insert(
            path,
            FileTreeNode::File {
                contents: contents.as_bytes().to_vec(),
            },
        )
        .expect("insert");
    }
    tree
}

fn fixture_form_yaml() -> String {
    std::fs::read_to_string(quillmark_fixtures::quills_path("sample_form").join("Form.yaml"))
        .expect("the fixture ships Form.yaml")
}

/// The fixture's field spec, written as JSON.
fn fixture_as_json() -> String {
    let value: serde_json::Value =
        quillmark_core::value::from_yaml_str(&fixture_form_yaml()).expect("the fixture parses");
    serde_json::to_string_pretty(&value).expect("serializes")
}

fn open(tree: FileTreeNode) -> Result<quillmark::LiveSession, RenderError> {
    let quill = Quill::from_tree(tree).expect("Quill.yaml loads");
    let doc = Document::parse(DOC).expect("parse markdown").document;
    Engine::new().open(&quill, &doc, common::test_date())
}

fn open_err(tree: FileTreeNode) -> quillmark::Diagnostic {
    let Err(err) = open(tree) else {
        panic!("the quill must not open")
    };
    let [diag] = err.diagnostics() else {
        panic!("one diagnostic: {err:?}")
    };
    diag.clone()
}

#[test]
fn json_content_under_form_yaml_reads_as_the_fixture_does() {
    let regions = |tree| open(tree).expect("opens").regions();
    let from_yaml = regions(sample_form_with(&[("Form.yaml", &fixture_form_yaml())]));
    assert!(!from_yaml.is_empty());
    assert_eq!(
        regions(sample_form_with(&[("Form.yaml", &fixture_as_json())])),
        from_yaml
    );
}

#[test]
fn a_legacy_spec_is_refused_with_or_without_a_form_yaml() {
    for (file, spec, code) in [
        ("form.json", fixture_as_json(), "acroform::legacy_form_json"),
        (
            "form.yaml",
            fixture_form_yaml(),
            "acroform::legacy_form_yaml",
        ),
    ] {
        let alone = open_err(sample_form_with(&[(file, &spec)]));
        assert_eq!(alone.code.as_deref(), Some(code));
        let hint = alone.hint.expect("names the fix");
        assert!(
            hint.contains(&format!("Rename `{file}` to `Form.yaml`")),
            "{hint}"
        );

        let both = open_err(sample_form_with(&[("Form.yaml", &spec), (file, &spec)]));
        assert_eq!(both.code.as_deref(), Some(code));
        let hint = both.hint.expect("names the fix");
        assert!(hint.starts_with("Keep one"), "{hint}");
    }
}

#[test]
fn a_quill_without_form_yaml_is_missing_its_spec() {
    let diag = open_err(sample_form_with(&[]));
    assert_eq!(diag.code.as_deref(), Some("acroform::missing_form_spec"));
    let hint = diag.hint.expect("names the fix");
    assert!(hint.contains("Form.yaml"), "{hint}");
}

#[test]
fn a_bad_form_yaml_locates_its_error() {
    let located = |spec: &str| {
        let diag = open_err(sample_form_with(&[("Form.yaml", spec)]));
        assert_eq!(diag.code.as_deref(), Some("acroform::invalid_form_spec"));
        let loc = diag.location.expect("located");
        assert_eq!(loc.file, "Form.yaml");
        (loc.line, loc.column)
    };
    let head = "schema: quillmark/form@0.2.0\nfields:\n  - name: FullName\n";
    assert_eq!(
        located(&format!(
            "{head}    schema_field: full_name\n    tooltip: Box 14b: name\n"
        )),
        (5, 21),
    );
    assert_eq!(
        located(&format!(
            "{head}    page: 0\n    rect: {{ x: 0, y: 0, w: 1, h: 1 }}\n"
        )),
        (5, 5),
    );
}

/// The fixture with `extra` bound fields, rendered over `favorite_color: green`.
fn render_with_fields(extra: &[serde_json::Value]) -> lopdf::Document {
    let mut spec: serde_json::Value =
        quillmark_core::value::from_yaml_str(&fixture_form_yaml()).expect("the fixture parses");
    spec["fields"]
        .as_array_mut()
        .expect("fields")
        .extend_from_slice(extra);
    let tree = sample_form_with(&[("Form.yaml", &spec.to_string())]);
    let quill = Quill::from_tree(tree).expect("Quill.yaml loads");
    let markdown = DOC.replace("full_name: Ada Lovelace\n", "favorite_color: green\n");
    let doc = Document::parse(&markdown).expect("parse markdown").document;
    let result = Engine::new()
        .render(
            &quill,
            &doc,
            common::test_date(),
            &RenderOptions::default().with_output_format(OutputFormat::Pdf),
        )
        .expect("render ok");
    lopdf::Document::load_mem(&result.artifacts[0].bytes).expect("reparse")
}

fn name_of(obj: &lopdf::Object) -> String {
    String::from_utf8(obj.as_name().unwrap().to_vec()).unwrap()
}

/// The widgets a field holds, each the dictionary of one kid.
fn kids<'a>(pdf: &'a lopdf::Document, field: &lopdf::Dictionary) -> Vec<&'a lopdf::Dictionary> {
    field
        .get(b"Kids")
        .expect("a field of several widgets holds them as kids")
        .as_array()
        .unwrap()
        .iter()
        .map(|kid| {
            pdf.get_object(kid.as_reference().unwrap())
                .unwrap()
                .as_dict()
                .unwrap()
        })
        .collect()
}

#[test]
fn a_box_per_option_is_one_radio_field_holding_the_chosen_value() {
    let boxes: Vec<serde_json::Value> = ["red", "green", "blue"]
        .iter()
        .enumerate()
        .map(|(i, color)| {
            serde_json::json!({
                "name": format!("Color_{color}"),
                "schema_field": "favorite_color",
                "option": color,
                "page": 0,
                "rect": { "x": 72 + 20 * i, "y": 700, "w": 12, "h": 12 },
            })
        })
        .collect();
    let pdf = render_with_fields(&boxes);
    let af = acroform(&pdf);
    let radio = widget(&pdf, af, "Color_red");
    let flags = radio.get(b"Ff").unwrap().as_i64().unwrap();
    assert_eq!(flags & (1 << 15), 1 << 15, "a radio field");
    assert_eq!(
        flags & (1 << 14),
        1 << 14,
        "a filler cannot untick its answer"
    );
    assert_eq!(name_of(radio.get(b"V").unwrap()), "green");

    let buttons = kids(&pdf, radio);
    let states: Vec<String> = buttons
        .iter()
        .map(|b| name_of(b.get(b"AS").unwrap()))
        .collect();
    assert_eq!(states, ["Off", "green", "Off"]);
    for (button, color) in buttons.iter().zip(["red", "green", "blue"]) {
        let ap = button.get(b"AP").unwrap().as_dict().unwrap();
        let n = ap.get(b"N").unwrap().as_dict().unwrap();
        assert!(n.has(color.as_bytes()) && n.has(b"Off"), "{color}");
        assert!(
            !button.has(b"T"),
            "a button is a widget, not a field of its own"
        );
    }
}

#[test]
fn the_boxes_binding_one_field_are_widgets_of_the_first_ones_field() {
    let boxes: Vec<serde_json::Value> = ["Color_top", "Color_bottom"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            serde_json::json!({
                "name": name,
                "schema_field": "favorite_color",
                "page": 0,
                "rect": { "x": 72, "y": 600 + 40 * i, "w": 200, "h": 20 },
            })
        })
        .collect();
    let pdf = render_with_fields(&boxes);
    let af = acroform(&pdf);
    let field = widget(&pdf, af, "FavoriteColor");
    assert_eq!(field.get(b"FT").unwrap().as_name().unwrap(), b"Ch");
    assert_eq!(
        kids(&pdf, field).len(),
        3,
        "the fixture's box and the two added"
    );
    let names: Vec<&[u8]> = fields(&pdf, af).map(|(t, _)| t).collect();
    assert!(
        !names.contains(&&b"Color_top"[..]) && !names.contains(&&b"Color_bottom"[..]),
        "{names:?}"
    );
}
