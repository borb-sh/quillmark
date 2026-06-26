//! Error mapping utilities for converting Typst diagnostics to Quillmark diagnostics.

use crate::world::QuillWorld;
use quillmark_core::{Diagnostic, Location, Severity};
use typst::diag::SourceDiagnostic;

/// Converts Typst diagnostics to Quillmark diagnostics.
pub fn map_typst_errors(errors: &[SourceDiagnostic], world: &QuillWorld) -> Vec<Diagnostic> {
    errors
        .iter()
        .map(|e| map_single_diagnostic(e, world))
        .collect()
}

/// Converts a single Typst diagnostic to a Quillmark diagnostic.
fn map_single_diagnostic(error: &SourceDiagnostic, world: &QuillWorld) -> Diagnostic {
    let severity = match error.severity {
        typst::diag::Severity::Error => Severity::Error,
        typst::diag::Severity::Warning => Severity::Warning,
    };

    // Extract location from span
    let location = resolve_span_to_location(error.span, world);

    // Get first hint if available
    let mut hint = error.hints.first().map(|h| h.v.to_string());

    // When the span can't be resolved to a location, the error almost always
    // originated in dynamically-evaluated content (a quill `eval` of a field
    // value), whose ephemeral source was never registered in the world. The
    // bare Typst message (e.g. "unclosed label", "unknown variable: general")
    // is then a dead end: no file, no line, no anchor. If Typst gave us no
    // hint of its own, attach a generic one that points the caller at the
    // likely culprit so the diagnostic stays actionable.
    if location.is_none() && hint.is_none() {
        hint = Some(
            "This error originated in dynamically evaluated content (a quill `eval` of a \
             field value); check field values for unescaped Typst-significant characters \
             (`#`, `@`, `$`, unmatched brackets)."
                .to_string(),
        );
    }

    // Extract error code from message (simple heuristic)
    let code = Some(format!(
        "typst::{}",
        error.message.split(':').next().unwrap_or("error").trim()
    ));

    Diagnostic {
        severity,
        code,
        message: error.message.to_string(),
        location,
        path: None,
        hint,
        source_chain: Vec::new(),
    }
}

/// Resolves a Typst diagnostic span to a Quillmark Location.
fn resolve_span_to_location(span: typst::syntax::DiagSpan, world: &QuillWorld) -> Option<Location> {
    use typst::{World, WorldExt};

    // Resolve the span against its OWN source file. A diagnostic originating in
    // an injected helper package or a vendored package must report coordinates
    // (and a path) in that file, not in main.typ. Spans with no file id (the
    // detached span) fall back to main.
    let source_id = span.id().unwrap_or_else(|| world.main());
    let source = world.source(source_id).ok()?;
    let range = world.range(span)?;

    let text = source.text();
    let line = text[..range.start].matches('\n').count() + 1;
    let column = range.start - text[..range.start].rfind('\n').map_or(0, |pos| pos + 1) + 1;

    Some(Location {
        file: source.id().vpath().get_without_slash().to_string(),
        line: line as u32,
        column: column as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillmark_core::{FileTreeNode, Quill};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use typst::diag::SourceDiagnostic;
    use typst::syntax::Span;

    /// Build a `QuillWorld` from the `usaf_memo` fixture so we have a valid main
    /// source to resolve (or fail to resolve) spans against.
    fn fixture_world() -> Option<QuillWorld> {
        fn walk(dir: &Path) -> std::io::Result<FileTreeNode> {
            let mut files = HashMap::new();
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let p: PathBuf = entry.path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_file() {
                    files.insert(
                        name,
                        FileTreeNode::File {
                            contents: fs::read(&p)?,
                        },
                    );
                } else if p.is_dir() {
                    files.insert(name, walk(&p)?);
                }
            }
            Ok(FileTreeNode::Directory { files })
        }

        let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("resources")
            .join("quills")
            .join("usaf_memo")
            .join("0.2.0");
        if !quill_path.exists() {
            return None;
        }
        let tree = walk(&quill_path).expect("walk fixture");
        let source = Quill::from_tree(tree).expect("load source");
        Some(QuillWorld::new(&source, "// Test").expect("create world"))
    }

    /// A diagnostic whose span can't be resolved to a location — the case
    /// behind errors from `eval`'d runtime strings (issue #745) — must degrade
    /// to a generic hint pointing at dynamically-evaluated field content.
    #[test]
    fn unresolvable_span_gets_generic_hint() {
        let Some(world) = fixture_world() else {
            return;
        };

        // A detached span carries no file id and no resolvable range, exactly
        // like the ephemeral source produced by `eval(string, ...)`.
        let diag = SourceDiagnostic::error(Span::detached(), "unknown variable: general");
        let mapped = map_single_diagnostic(&diag, &world);

        assert!(
            mapped.location.is_none(),
            "detached span should not resolve to a location"
        );
        let hint = mapped
            .hint
            .expect("unresolvable diagnostic must carry a hint");
        assert!(
            hint.contains("dynamically evaluated content"),
            "hint should point at dynamically-evaluated field content, got: {hint:?}"
        );
        // The original Typst message is preserved verbatim.
        assert_eq!(mapped.message, "unknown variable: general");
    }

    /// When Typst already supplied a hint, we keep it rather than overwriting
    /// it with the generic one — even if the span doesn't resolve.
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

    #[test]
    fn test_severity_mapping() {
        // Ensure Typst severity maps correctly
        assert_eq!(
            match typst::diag::Severity::Error {
                typst::diag::Severity::Error => Severity::Error,
                typst::diag::Severity::Warning => Severity::Warning,
            },
            Severity::Error
        );

        assert_eq!(
            match typst::diag::Severity::Warning {
                typst::diag::Severity::Error => Severity::Error,
                typst::diag::Severity::Warning => Severity::Warning,
            },
            Severity::Warning
        );
    }
}
