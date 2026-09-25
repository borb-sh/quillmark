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

/// Each read-only command against a shipped quill: `schema` names taro's own
/// field, so a generic or empty dump fails.
#[test]
fn read_commands_print_the_quill() {
    let quill = taro();
    for (cmd, needle) in [("info", "taro"), ("schema", "ice_cream"), ("blueprint", "$quill:")] {
        let stdout = ok(&[cmd, quill.to_str().unwrap()]);
        assert!(stdout.contains(needle), "{cmd} omits {needle:?}: {stdout}");
    }
    ok(&["validate", quill.to_str().unwrap()]);
}

/// A document `usaf_memo` renders with a warning: a number where its
/// `references` hold richtext, which conform leaves as authored.
fn warning_doc(dir: &tempfile::TempDir) -> PathBuf {
    let doc = dir.path().join("warning.md");
    std::fs::write(
        &doc,
        "~~~card-yaml\n$quill: usaf_memo\n$kind: main\nreferences: [42]\n~~~\n\none\n",
    )
    .expect("write the input document");
    doc
}

/// A taro document long enough to span pages.
fn long_doc(dir: &tempfile::TempDir) -> PathBuf {
    let doc = dir.path().join("long.md");
    let body: String = (0..120)
        .map(|i| format!("Paragraph {i} of a body long enough to span pages.\n\n"))
        .collect();
    std::fs::write(
        &doc,
        format!("~~~card-yaml\n$quill: taro\ntitle: Long\nauthor: Tester\n~~~\n\n{body}"),
    )
    .expect("write the input document");
    doc
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

/// The quill authoring contract, which only a render reaches: this plate indexes
/// a card the seed carries and the empty document does not, so `render` and
/// `validate --no-render` both pass it and the default check does not.
#[test]
fn validate_renders_the_empty_document_a_seed_render_would_miss() {
    let dir = quill_with_config(
        r#"quill:
  name: brittle
  version: 0.1.0
  backend: typst
  description: A plate that indexes a card the empty document does not carry
typst:
  plate_file: plate.typ
main:
  fields:
    title:
      description: title of document
      type: string
card_kinds:
  note:
    description: A note
    fields:
      author:
        description: who wrote it
        type: string
        example: A. Author
"#,
    );
    std::fs::write(
        dir.path().join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n\
         #data.title\n\
         #data.at(\"$cards\").at(0).author\n",
    )
    .expect("write plate.typ");
    let path = dir.path().to_str().unwrap();

    let out = run(&["validate", path]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a plate that cannot render the empty document exited {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.matches("cli::canonical_document_failed").count() == 1
            && stderr.contains("empty"),
        "the empty document alone should fail: {stderr}"
    );

    ok(&["validate", path, "--no-render"]);
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

/// `-o` names a directory that does not exist yet, and the artifact still lands
/// there.
#[test]
fn render_writes_a_pdf_creating_parent_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join("deeper").join("out.pdf");

    let out = run(&["render", taro().to_str().unwrap(), "-o", path.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "render exited nonzero: {stderr}");

    let bytes = std::fs::read(&path).expect("the -o file exists");
    assert!(
        bytes.starts_with(b"%PDF-"),
        "output is not a PDF (first bytes: {:?})",
        &bytes[..bytes.len().min(8)]
    );
}

/// A warning line on stdout does not garble a message, it corrupts the PDF the
/// caller is redirecting. `render` parses through the bound door, so the value
/// conform cannot rest warns.
#[test]
fn chatter_does_not_contaminate_the_stdout_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = warning_doc(&dir);
    let memo = quillmark_fixtures::quills_path("usaf_memo");
    let out = run(&[
        "render",
        memo.to_str().unwrap(),
        doc.to_str().unwrap(),
        "--stdout",
    ]);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "render --stdout failed: {stderr}");
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
        stderr.contains("conform::field_decode"),
        "the warning went somewhere other than stderr: {stderr}"
    );
}

/// No unnumbered file sits beside the numbered pages, claiming to be the whole
/// document, and `--stdout` refuses loudly rather than writing page one as the
/// document.
#[test]
fn multi_page_svg_writes_one_file_per_page_and_refuses_stdout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = long_doc(&dir);
    let (quill, doc) = (taro(), doc.to_str().unwrap().to_owned());
    let quill = quill.to_str().unwrap();

    let out = dir.path().join("out.svg");
    ok(&["render", quill, &doc, "-f", "svg", "-o", out.to_str().unwrap()]);
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

    let out = run(&["render", quill, &doc, "-f", "svg", "--stdout"]);
    assert!(!out.status.success(), "multi-page --stdout exited 0");
    assert!(
        out.stdout.is_empty(),
        "a refused --stdout still wrote {} bytes",
        out.stdout.len()
    );
}

/// taro obliges `author` and `title`; `titel` is a typo of the second.
const TYPO_DOC: &str = "~~~card-yaml\n$quill: taro\ntitel: Hello\n~~~\n\nBody.\n";

/// A warning passes and `--strict` fails it; an error fails either way, and a
/// document that fails to read or parse does not stop the ones after it being
/// checked.
#[test]
fn check_lists_every_diagnostic_and_strict_fails_on_a_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let typo = dir.path().join("typo.md");
    std::fs::write(&typo, TYPO_DOC).expect("write the typo document");
    let bad = dir.path().join("bad.md");
    std::fs::write(&bad, "~~~card-yaml\n$quill: taro\ntitle: [1\n~~~\n")
        .expect("write the malformed document");
    let binary = dir.path().join("binary.md");
    std::fs::write(&binary, b"\xff\xfe").expect("write the non-UTF-8 document");
    let quill = taro();
    let (quill, typo, bad, binary) = (
        quill.to_str().unwrap(),
        typo.to_str().unwrap(),
        bad.to_str().unwrap(),
        binary.to_str().unwrap(),
    );

    let out = run(&["check", quill, typo]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "warnings alone failed check: {stderr}");
    assert!(
        stderr.contains("validation::unknown_field"),
        "check omits the warning: {stderr}"
    );

    let out = run(&["check", "--strict", quill, typo]);
    assert_eq!(out.status.code(), Some(1), "--strict passed a warning");

    let out = run(&["check", quill, binary, bad, typo]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "a parse error passed check: {stderr}");
    assert!(
        stderr.contains("cli::unreadable_document")
            && stderr.contains("parse::")
            && stderr.contains("validation::unknown_field"),
        "check stopped at a failing document: {stderr}"
    );
}

/// `render` prints the input its page leaves out.
#[test]
fn render_warns_on_unclaimed_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = dir.path().join("typo.md");
    std::fs::write(&doc, TYPO_DOC).expect("write the input document");

    let out = run(&[
        "render",
        taro().to_str().unwrap(),
        doc.to_str().unwrap(),
        "-o",
        dir.path().join("typo.pdf").to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "render exited nonzero: {stderr}");
    assert!(
        stderr.contains("validation::unknown_field"),
        "the undeclared key raised no warning: {stderr}"
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
/// missing. Exit 1 rather than any non-zero code: a panic exits differently,
/// so a script reading the status can tell a refusal from a crash.
#[test]
fn a_missing_quill_path_exits_one_naming_the_path_on_every_command() {
    for cmd in ["info", "schema", "validate"] {
        let out = run(&[cmd, "/nonexistent/quill/path"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "`quillmark {cmd}`: {stderr}");
        assert!(
            stderr.contains("/nonexistent/quill/path") && !stderr.contains("Quill.yaml"),
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

/// An `-o` extension naming a format supplies an omitted `-f` and refuses a
/// disagreeing one before any file is written; an extension naming none is
/// written as given.
#[test]
fn the_output_extension_reconciles_with_the_format_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let quill = taro();
    let quill = quill.to_str().unwrap();
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();

    ok(&["render", quill, "-o", &path("inferred.SVG")]);
    let svg = std::fs::read_to_string(dir.path().join("inferred.SVG"))
        .expect("render wrote the inferred file");
    assert!(svg.contains("<svg"), "-o inferred.SVG did not write SVG");

    let out = run(&[
        "render",
        quill,
        "-f",
        "png",
        "-o",
        &path("clash.pdf"),
        "--output-data",
        &path("clash.json"),
    ]);
    assert_eq!(out.status.code(), Some(1), "-f png -o clash.pdf was not refused");
    let written: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|name| name != "inferred.SVG")
        .collect();
    assert!(written.is_empty(), "a refused render wrote {written:?}");

    ok(&["render", quill, "-f", "svg", "-o", &path("page.bin")]);
    let svg = std::fs::read_to_string(dir.path().join("page.bin")).expect("page.bin exists");
    assert!(svg.contains("<svg"), "-f svg -o page.bin did not write SVG");
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

/// `--quiet` silences both streams a successful render writes: the warning on
/// stderr and the destination line on stdout.
#[test]
fn quiet_silences_the_warning_and_the_destination_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let doc = warning_doc(&dir);
    let out_path = dir.path().join("out.pdf");
    let memo = quillmark_fixtures::quills_path("usaf_memo");

    let out = run(&[
        "render",
        memo.to_str().unwrap(),
        doc.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "--quiet",
    ]);

    assert!(out.status.success(), "exited {:?}", out.status.code());
    assert!(
        out.stderr.is_empty(),
        "--quiet let the warning through:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "--quiet let the destination line through:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `--today` is the render date a `today` field renders as; without it the CLI
/// supplies the local date, and a plate asking `datetime.today()` still renders.
#[test]
fn render_dates_a_today_field() {
    let dir = quill_with_config(
        r#"quill:
  name: dated
  version: 0.1.0
  backend: typst
  description: A field dated by the day it renders
typst:
  plate_file: plate.typ
main:
  fields:
    issued: { type: date, default: today }
"#,
    );
    std::fs::write(
        dir.path().join("plate.typ"),
        "#import \"@local/quillmark-helper:0.1.0\": data\n\
         #assert.eq(data.issued, datetime.today())\n",
    )
    .expect("write plate.typ");
    let quill = dir.path().to_str().unwrap();
    let data = |name: &str| -> serde_json::Value {
        let file = dir.path().join(name);
        serde_json::from_str(&std::fs::read_to_string(file).expect("data written"))
            .expect("data is JSON")
    };
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();

    ok(&[
        "render",
        quill,
        "--today",
        "2026-03-14",
        "-o",
        &path("pinned.svg"),
        "--output-data",
        &path("pinned.json"),
    ]);
    assert_eq!(data("pinned.json")["issued"], "2026-03-14");

    ok(&["render", quill, "-o", &path("local.svg"), "--output-data", &path("local.json")]);
    let local = data("local.json")["issued"].as_str().unwrap_or_default().to_owned();
    assert!(local.parse::<quillmark::CalendarDate>().is_ok(), "{local:?} is not a date");

    ok(&["validate", quill]);
    assert_eq!(run(&["render", quill, "--today", "today"]).status.code(), Some(2));
}

/// `workspace` writes the helper package where the printed `--package-path`
/// points, and the command names the quill's plate.
#[test]
fn workspace_writes_the_helper_and_prints_the_typst_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("ws");
    let quill = taro();
    let stdout = ok(&["workspace", quill.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(out
        .join("packages/local/quillmark-helper/0.1.0/lib.typ")
        .is_file());
    let command = stdout
        .lines()
        .find(|l| l.starts_with("typst watch"))
        .unwrap_or_else(|| panic!("no typst command: {stdout}"));
    assert!(
        command.contains(&format!("--package-path {}", out.join("packages").display()))
            && command.contains(&quill.join("plate.typ").display().to_string()),
        "{command}"
    );
}

/// A workspace inside the quill would load as quill files on the next read, so
/// `workspace` refuses one and writes nothing.
#[test]
fn workspace_refuses_a_directory_inside_the_quill() {
    let dir = quill_with_config(
        "quill:\n  name: w\n  version: 0.1.0\n  backend: typst\n  description: w\n\
         typst:\n  plate_file: plate.typ\n",
    );
    std::fs::write(dir.path().join("plate.typ"), "hi\n").expect("write plate.typ");
    let quill = dir.path().to_str().unwrap();
    let inside = dir.path().join("sub/../ws");
    let out = run(&["workspace", quill, "-o", inside.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.path().join("ws").exists());
}

/// A path the shell would split is quoted in the printed command.
#[test]
fn workspace_quotes_a_path_with_a_space() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("my ws");
    let stdout = ok(&["workspace", taro().to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(
        stdout.contains(&format!("--package-path '{}'", out.join("packages").display())),
        "{stdout}"
    );
}
