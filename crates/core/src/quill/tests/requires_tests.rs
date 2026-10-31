use super::*;

fn load(requires: &str, extra: &str) -> Result<(QuillConfig, Vec<Diagnostic>), Vec<Diagnostic>> {
    QuillConfig::from_yaml_with_warnings(&format!(
        "quill:\n  name: memo\n  version: \"1.0.0\"\n  backend: typst\n  description: memo\n  requires: {requires}\n{extra}"
    ))
}

#[test]
fn a_floor_this_release_meets_loads() {
    load("\">=0.1\"", "").expect("0.1 is below any Quillmark release");
    let release = format!(
        "\">={}.{}.{}\"",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    load(&release, "").expect("the floor is inclusive");
}

#[test]
fn a_floor_above_this_release_is_the_one_error() {
    let refused = load(
        "\">=9999.0\"",
        "  from_the_future: true\nnew_section: {}\ntypst:\n  from_the_future: true\n",
    )
        .expect_err("9999.0 is above this Quillmark release");
    assert_eq!(codes(&refused), ["quill::requires_unmet"]);
}

#[test]
fn a_floor_is_spelled_with_its_comparator() {
    for requires in ["\"1.0\"", "\">1.0\"", "\">=1\"", "\">=x.y\"", "\">= 1.0\"", "1.0", "[\">=1.0\"]"] {
        let refused = load(requires, "").expect_err(requires);
        assert_eq!(codes(&refused), ["quill::invalid_requires"], "{requires}");
    }
}

#[test]
fn a_misspelled_floor_is_reported_beside_the_other_errors() {
    let refused = load("\"1.0\"", "  colour: red\n").expect_err("two errors");
    assert_eq!(codes(&refused), ["quill::invalid_requires", "quill::unknown_key"]);
}
