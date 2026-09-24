//! Translates Typst diagnostics into Quillmark
//! [`Diagnostic`](quillmark_core::error::Diagnostic) values.

use crate::world::QuillWorld;
use quillmark_core::error::{Diagnostic, Location, Severity};
use typst::diag::SourceDiagnostic;

pub(crate) fn map_typst_errors(errors: &[SourceDiagnostic], world: &QuillWorld) -> Vec<Diagnostic> {
    errors
        .iter()
        .map(|e| map_single_diagnostic(e, world))
        .collect()
}

fn map_single_diagnostic(error: &SourceDiagnostic, world: &QuillWorld) -> Diagnostic {
    let severity = match error.severity {
        typst::diag::Severity::Error => Severity::Error,
        typst::diag::Severity::Warning => Severity::Warning,
    };

    let location = resolve_span_to_location(error.span, world);

    let hint = error.hints.first().map(|h| h.v.to_string()).or_else(|| {
        (error.message == "unable to get the current date").then(|| {
            "The plate reads `datetime.today()` and the render was given no date: pass \
             `today` to the render call."
                .to_string()
        })
    });

    let mut diag = Diagnostic::new(severity, error.message.to_string());
    diag.code = Some(classify(&error.message).to_string());
    diag.location = location;
    diag.hint = hint;
    diag
}

/// Typst renders a type mismatch two ways: the cast machinery's
/// `expected <a>, found <b>`, and the operand families of its `ops` module.
fn is_type_error(message: &str) -> bool {
    const OPERAND_MISMATCH: &[&str] = &[
        "cannot add ",
        "cannot apply ",
        "cannot compare ",
        "cannot divide ",
        "cannot join ",
        "cannot multiply ",
        "cannot subtract ",
    ];

    (message.starts_with("expected ") && message.contains(", found "))
        || OPERAND_MISMATCH.iter().any(|p| message.starts_with(p))
}

/// Typst has no error codes, so one is minted from the message's *shape*. The
/// set is closed: an unrecognized message takes `typst::compile` rather than a
/// code spelled by the message itself, which would carry author-supplied text
/// into a routing key and leave that key unbounded.
fn classify(message: &str) -> &'static str {
    if message.starts_with("file not found (searched at ") {
        "typst::file_not_found"
    } else if message.starts_with("unknown variable: ") {
        "typst::unknown_variable"
    } else if is_type_error(message) {
        "typst::type_error"
    } else {
        "typst::compile"
    }
}

fn resolve_span_to_location(span: typst::syntax::DiagSpan, world: &QuillWorld) -> Option<Location> {
    use typst::{World, WorldExt};

    // A diagnostic from an injected helper or vendored package reports
    // coordinates in that file, not the plate. Detached spans fall back to main.
    let source_id = span.id().unwrap_or_else(|| world.main());
    let source = world.source(source_id).ok()?;
    let range = world.range(span)?;

    let (line, column) = line_and_column(source.text(), range.start);

    Some(Location::new(
        world.display_path(source_id),
        line as u32,
        column as u32,
    ))
}

/// 1-indexed line and column of `offset` in `text`. The column counts
/// *characters*, which is what an editor jumps to: over bytes, every multi-byte
/// glyph earlier on the line would inflate it.
fn line_and_column(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    let line_start = prefix.rfind('\n').map_or(0, |pos| pos + 1);
    (
        prefix.matches('\n').count() + 1,
        prefix[line_start..].chars().count() + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TypstBackend;
    use quillmark_core::{
        backend::Backend,
        quill::{FileTreeNode, Quill},
        types::{OutputFormat, RenderOptions},
    };
    use typst::diag::SourceDiagnostic;
    use typst::syntax::Span;

    /// `None` when the fixture is absent (a stripped checkout).
    fn walk_fixture() -> Option<FileTreeNode> {
        let quill_path = quillmark_fixtures::quills_path("usaf_memo");
        if !quill_path.exists() {
            return None;
        }
        Some(quillmark::tree_from_path(quill_path).expect("walk fixture"))
    }

    fn fixture_world() -> Option<QuillWorld> {
        let tree = walk_fixture()?;
        let source = Quill::from_tree(tree).expect("load source");
        let plate = crate::read_plate(&source).expect("plate");
        Some(QuillWorld::new(&source, &plate).expect("create world"))
    }

    /// The fixture's `typst.plate_file: plate.typ` makes the backend read this.
    fn source_with_plate(plate: &str) -> Option<Quill> {
        let mut tree = walk_fixture()?;
        if let FileTreeNode::Directory { files } = &mut tree {
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File {
                    contents: plate.as_bytes().to_vec(),
                },
            );
        }
        Some(Quill::from_tree(tree).expect("load source"))
    }

    /// Messages Typst 0.15.1 emits, taken from a compile of each case.
    const REAL_MESSAGES: &[(&str, &str)] = &[
        (
            "file not found (searched at assets/marc.png)",
            "typst::file_not_found",
        ),
        (
            "file not found (searched at https:/example.com/x.png)",
            "typst::file_not_found",
        ),
        ("unknown variable: general", "typst::unknown_variable"),
        ("cannot add integer and string", "typst::type_error"),
        ("expected length, found string", "typst::type_error"),
        ("cannot apply 'not' to integer", "typst::type_error"),
        ("unclosed delimiter", "typst::compile"),
        ("expected expression", "typst::compile"),
        ("unknown font family: nosuchfontfamily", "typst::compile"),
    ];

    /// The table, and the property the code exists for: a routing key a
    /// consumer can match and a string table can hold, carrying none of the
    /// message it came from — the two `file not found` rows differ only in an
    /// author-supplied path and share their code.
    #[test]
    fn every_message_takes_a_slug_from_the_closed_set_carrying_none_of_itself() {
        for (message, code) in REAL_MESSAGES {
            assert_eq!(&classify(message), code, "classifying {message:?}");
            let slug = code.strip_prefix("typst::").expect("namespaced");
            assert!(
                !slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "`{code}` is not a snake_case slug"
            );
            assert!(
                !code.contains(message),
                "`{code}` carries the message it was minted from"
            );
        }
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let text = "#let café = 1\n#(café + x)\n";
        assert_eq!(line_and_column(text, 0), (1, 1));
        // `x` on line 2: nine characters precede it, ten bytes.
        assert_eq!(line_and_column(text, text.find(" x)").unwrap() + 1), (2, 10));
    }

    #[test]
    fn unresolvable_span_keeps_existing_typst_hint() {
        let Some(world) = fixture_world() else {
            return;
        };

        let diag = SourceDiagnostic::error(Span::detached(), "unexpected closing bracket")
            .with_hint("try using a backslash escape: \\]");
        let mapped = map_single_diagnostic(&diag, &world);

        assert!(mapped.location.is_none());
        assert_eq!(
            mapped.hint.as_deref(),
            Some("try using a backslash escape: \\]"),
            "an existing Typst hint must not be overwritten"
        );
    }

    /// The closed set holds against a real compile, not only against the
    /// message table: Typst spells the path it searched into the message, and
    /// the code stays the shape's.
    #[test]
    fn a_missing_file_codes_by_shape_through_a_compile() {
        let Some(source) = source_with_plate(
            "#set page(width: 200pt, height: 200pt)\n#image(\"assets/marc.png\")\n",
        ) else {
            return;
        };

        let diags = match TypstBackend.open(&source, &serde_json::json!({}), None) {
            Ok(session) => session
                .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
                .expect_err("a missing image should fail to compile")
                .into_diagnostics(),
            Err(err) => err.into_diagnostics(),
        };

        let diag = diags
            .iter()
            .find(|d| d.message.starts_with("file not found"))
            .expect("expected a file-not-found diagnostic");

        assert_eq!(diag.code.as_deref(), Some("typst::file_not_found"));
        assert!(
            diag.message.contains("assets/marc.png"),
            "the searched path stays in the message: {}",
            diag.message
        );
    }

    const EVAL_ERROR_PLATE: &str =
        "#set page(width: 400pt, height: 300pt)\n#eval(\"#general\", mode: \"markup\")\n";

    #[test]
    fn resolvable_eval_error_is_unchanged() {
        let Some(source) = source_with_plate(EVAL_ERROR_PLATE) else {
            return;
        };

        // Compilation happens during `open`, so the error may surface there.
        let diags = match TypstBackend.open(&source, &serde_json::json!({}), None) {
            Ok(session) => session
                .render(&RenderOptions::default().with_output_format(OutputFormat::Pdf))
                .expect_err("eval of `#general` should fail to compile")
                .into_diagnostics(),
            Err(err) => err.into_diagnostics(),
        };
        assert!(
            !diags.is_empty(),
            "compilation error must carry diagnostics"
        );

        let diag = diags
            .iter()
            .find(|d| d.message.contains("unknown variable: general"))
            .expect("expected the `unknown variable: general` diagnostic");

        assert!(
            diag.location.is_some(),
            "this eval error resolves to the call site; expected a location, got None"
        );
    }
}
