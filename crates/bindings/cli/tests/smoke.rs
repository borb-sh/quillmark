//! Every subcommand, once, against a fixture quill. The bin carries
//! `test = false` (its name collides with the library crate), so these drive
//! the built executable: arg parsing, exit status, and the bytes that land on
//! stdout/stderr. Depth belongs to the core tests the commands delegate to.

use std::path::PathBuf;
use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_quillmark"))
}

fn taro() -> PathBuf {
    quillmark_fixtures::quills_path("taro")
}

fn run(args: &[&str]) -> Output {
    cli()
        .args(args)
        .output()
        .expect("the built binary is executable")
}

/// Exit 0, with stderr echoed on failure so a red run names its own cause.
fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "`quillmark {}` exited {:?}\nstderr: {}",
        args.join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

#[test]
fn info_prints_the_quill_identity() {
    let quill = taro();
    let stdout = ok(&["info", quill.to_str().unwrap()]);
    assert!(stdout.contains("taro"), "info omits the quill name: {stdout}");
}

#[test]
fn info_json_is_parseable() {
    let quill = taro();
    let stdout = ok(&["info", quill.to_str().unwrap(), "--json"]);
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json emits one JSON document");
    assert!(value.is_object(), "--json emits an object: {value}");
}

#[test]
fn schema_emits_yaml_naming_a_declared_field() {
    let quill = taro();
    let stdout = ok(&["schema", quill.to_str().unwrap()]);
    // taro's own field, so a generic or empty dump fails.
    assert!(
        stdout.contains("ice_cream"),
        "schema omits a declared field: {stdout}"
    );
}

#[test]
fn blueprint_emits_a_card_yaml_fence() {
    let quill = taro();
    let stdout = ok(&["blueprint", quill.to_str().unwrap()]);
    assert!(
        stdout.contains("$quill:"),
        "blueprint carries no `$quill` line: {stdout}"
    );
}

#[test]
fn validate_accepts_a_shipped_quill() {
    let quill = taro();
    ok(&["validate", quill.to_str().unwrap()]);
}

/// A quill directory carrying just `yaml`, for the failure paths no shipped
/// fixture covers.
fn quill_with_config(yaml: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("Quill.yaml"), yaml).expect("write Quill.yaml");
    dir
}

/// One summary, not one per printer: the command writes its own, so the error
/// it returns must not carry a second.
#[test]
fn a_failing_validate_prints_one_summary() {
    let dir = quill_with_config(
        r#"quill:
  name: broken
  version: 0.1.0
  backend: typst
  description: Names a plate that is not there
typst:
  plate_file: absent.typ
main:
  fields:
    title:
      description: title of document
      type: string
"#,
    );

    let out = run(&["validate", dir.path().to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a failing validate exited {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let summaries = stderr
        .lines()
        .filter(|line| line.contains("Validation failed"))
        .count();
    assert_eq!(summaries, 1, "expected one summary line: {stderr}");
}

/// A config that will not load is a quill failure, and reads as one.
#[test]
fn an_unloadable_quill_is_not_an_invalid_argument() {
    let dir = quill_with_config("quill:\n  name: broken\n  backend: typst\n");

    let out = run(&["validate", dir.path().to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an unloadable quill exited {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("quill::missing_version"),
        "the load diagnostics are missing: {stderr}"
    );
    assert!(
        !stderr.contains("Invalid argument"),
        "a load failure is labelled an invalid argument: {stderr}"
    );
}

/// The two commands that take `-o`.
#[test]
fn output_flag_writes_the_named_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let quill = taro();

    for (cmd, name) in [("schema", "schema.yaml"), ("blueprint", "blueprint.md")] {
        let path = dir.path().join(name);
        ok(&[cmd, quill.to_str().unwrap(), "-o", path.to_str().unwrap()]);
        let written = std::fs::read_to_string(&path).expect("the -o file exists");
        assert!(!written.trim().is_empty(), "{cmd} -o wrote an empty file");
    }
}

/// `-o` names a directory that does not exist yet, as `render -o` allows, and
/// the file still lands there without a word on stdout.
#[test]
fn output_flag_creates_parent_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let quill = taro();

    for (cmd, name) in [("schema", "s.yaml"), ("blueprint", "b.md")] {
        let path = dir.path().join("nested").join(cmd).join(name);
        let stdout = ok(&[cmd, quill.to_str().unwrap(), "-o", path.to_str().unwrap()]);
        assert!(stdout.is_empty(), "{cmd} -o wrote to stdout: {stdout}");
        let written = std::fs::read_to_string(&path).expect("the -o file exists");
        assert!(!written.trim().is_empty(), "{cmd} -o wrote an empty file");
    }
}

/// The one command that reaches a backend, seeded (no markdown argument).
#[test]
fn render_writes_a_pdf() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.pdf");
    let quill = taro();

    ok(&[
        "render",
        quill.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);

    let bytes = std::fs::read(&out).expect("render wrote its output file");
    assert!(
        bytes.starts_with(b"%PDF-"),
        "output is not a PDF (first bytes: {:?})",
        &bytes[..bytes.len().min(8)]
    );
}

/// A `--verbose` line on stdout does not garble a message, it corrupts the PDF
/// the caller is redirecting.
#[test]
fn verbose_does_not_contaminate_the_stdout_artifact() {
    let quill = taro();
    let out = run(&["render", quill.to_str().unwrap(), "--stdout", "--verbose"]);
    assert!(out.status.success(), "render --stdout --verbose failed");
    assert!(
        out.stdout.starts_with(b"%PDF-"),
        "stdout starts with {:?}, not PDF bytes",
        String::from_utf8_lossy(&out.stdout[..out.stdout.len().min(40)])
    );
    assert!(
        out.stdout.ends_with(b"%%EOF\n") || out.stdout.ends_with(b"%%EOF"),
        "stdout has trailing bytes after the PDF trailer"
    );
    assert!(
        !out.stderr.is_empty(),
        "--verbose emitted nothing on stderr, so the chatter went somewhere else"
    );
}

#[test]
fn render_svg_honours_the_format_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.svg");
    let quill = taro();

    ok(&[
        "render",
        quill.to_str().unwrap(),
        "-f",
        "svg",
        "-o",
        out.to_str().unwrap(),
    ]);

    let svg = std::fs::read_to_string(&out).expect("render wrote its output file");
    assert!(svg.contains("<svg"), "output is not SVG: {}", &svg[..svg.len().min(80)]);
}

/// No unnumbered file sits beside the numbered pages, claiming to be the whole
/// document.
#[test]
fn multi_page_svg_writes_one_file_per_page() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = dir.path().join("long.md");
    let body: String = (0..120)
        .map(|i| format!("Paragraph {i} of a body long enough to span pages.\n\n"))
        .collect();
    std::fs::write(
        &doc,
        format!("~~~card-yaml\n$quill: taro\ntitle: Long\nauthor: Tester\n~~~\n\n{body}"),
    )
    .expect("write the input document");

    let out = dir.path().join("out.svg");
    ok(&[
        "render",
        taro().to_str().unwrap(),
        doc.to_str().unwrap(),
        "-f",
        "svg",
        "-o",
        out.to_str().unwrap(),
    ]);

    assert!(
        !out.exists(),
        "an unnumbered out.svg sits beside the numbered pages"
    );
    for page in 1..=2 {
        let path = dir.path().join(format!("out-{page}.svg"));
        let svg = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("page {page} was not written: {e}"));
        assert!(svg.contains("<svg"), "page {page} is not SVG");
    }
}

/// Loudly, rather than by writing page one as the document.
#[test]
fn multi_page_stdout_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = dir.path().join("long.md");
    let body: String = (0..120)
        .map(|i| format!("Paragraph {i} of a body long enough to span pages.\n\n"))
        .collect();
    std::fs::write(
        &doc,
        format!("~~~card-yaml\n$quill: taro\ntitle: Long\nauthor: Tester\n~~~\n\n{body}"),
    )
    .expect("write the input document");

    let out = run(&[
        "render",
        taro().to_str().unwrap(),
        doc.to_str().unwrap(),
        "-f",
        "svg",
        "--stdout",
    ]);
    assert!(!out.status.success(), "multi-page --stdout exited 0");
    assert!(
        out.stdout.is_empty(),
        "a refused --stdout still wrote {} bytes",
        out.stdout.len()
    );
}

/// `render` parses through the bound door, so a construct the quill declares
/// its plate does not typeset reaches stderr instead of vanishing: `usaf_memo`
/// declares `body.unsupported: [rule]`, and a `***` leaves the page unmarked.
#[test]
fn a_declined_construct_warns_on_stderr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = dir.path().join("rule.md");
    std::fs::write(
        &doc,
        "~~~card-yaml\n$quill: usaf_memo\n$kind: main\n~~~\n\none\n\n***\n\ntwo\n",
    )
    .expect("write the input document");

    let memo = quillmark_fixtures::quills_path("usaf_memo");
    let out_pdf = dir.path().join("rule.pdf");
    let out = run(&[
        "render",
        memo.to_str().unwrap(),
        doc.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "render exited nonzero: {stderr}");
    assert!(
        stderr.contains("plate::unsupported_construct"),
        "the declined construct raised no warning: {stderr}"
    );
}

/// Exit 1 rather than any non-zero code: a panic exits differently, so a script
/// reading the status can tell a refusal from a crash.
#[test]
fn absent_quill_exits_one_with_stderr() {
    let out = run(&["info", "/nonexistent/quill/path"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a clean exit 1, got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.stderr.is_empty(),
        "absent quill wrote nothing to stderr"
    );
}

/// Exit 2 rather than 1: a script reading the status can tell an invocation
/// `clap` rejected from a command that ran and refused.
#[test]
fn a_usage_error_exits_two_with_stderr() {
    let out = run(&["render", "--bogus"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected a usage exit 2, got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stderr.is_empty(), "usage error wrote nothing to stderr");
}

/// Every command routes a typo'd path through the loader, which names the path
/// rather than the `Quill.yaml` a directory that does not exist cannot be
/// missing.
#[test]
fn a_missing_quill_path_names_the_path_on_every_command() {
    for cmd in ["info", "schema", "validate"] {
        let out = run(&[cmd, "/nonexistent/quill/path"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Quill directory not found"),
            "`quillmark {cmd}` on a missing path: {stderr}"
        );
    }
}

#[test]
fn unknown_format_fails_loudly() {
    let quill = taro();
    let out = run(&["render", quill.to_str().unwrap(), "-f", "docx", "--stdout"]);
    assert!(!out.status.success(), "unknown format exited 0");
    assert!(
        !out.stderr.is_empty(),
        "unknown format wrote nothing to stderr"
    );
}


/// `--format` parses case-insensitively, and the derived filename takes the
/// parsed format's id, not the flag as typed.
#[test]
fn format_casing_does_not_reach_the_output_filename() {
    let dir = tempfile::tempdir().expect("tempdir");
    let quill = taro();

    let out = cli()
        .current_dir(dir.path())
        .args(["render", quill.to_str().unwrap(), "-f", "PDF", "--quiet"])
        .output()
        .expect("the built binary is executable");
    assert!(
        out.status.success(),
        "render -f PDF exited {:?}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.path().join("example.pdf").is_file(),
        "example.pdf missing; dir holds {:?}",
        std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>()
    );
}

/// `--quiet` wins over `--verbose`: no progress line reaches stderr.
#[test]
fn quiet_silences_verbose() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out_path = dir.path().join("out.pdf");
    let quill = taro();

    let out = run(&[
        "render",
        quill.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "--verbose",
        "--quiet",
    ]);
    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        out.stderr.is_empty(),
        "--quiet let --verbose through:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "--quiet let the destination line through");
}
