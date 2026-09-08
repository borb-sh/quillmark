//! Conform-on-load: the bound door that lands a document at its canonical
//! rest (`prose/canon/SCHEMAS.md` § "Content fields rest per codec").
//!
//! The walk is the typed write driven by the schema instead of by a caller:
//! every content-bearing field goes through the same [`resolve_field_write`]
//! the typed writer commits through. What differs is the failure posture:
//! the writer refuses, conform leaves the value authored and warns.

use crate::document::edit::resolve_field_write;
use crate::document::{Card, Document, EditError, Parsed, PayloadItem};
use crate::path::DocPath;
use crate::quill::config::field_contains_content;
use crate::{Diagnostic, ParseError, Quill, QuillValue, RenderError, Severity};

use super::CardSchema;

/// The failure of the bound door ([`Quill::parse`]): the markdown did not
/// parse, or it parsed under a `$quill` this quill does not answer to. Nothing
/// conforms under the wrong schema, so the mismatch is an error and not a
/// warning; [`to_diagnostics`](Self::to_diagnostics) flattens either half for a
/// consumer that only routes on codes.
#[derive(Debug, thiserror::Error)]
pub enum BoundParseError {
    /// The markdown is not a well-formed card-yaml document.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// The document is well-formed but declares a different `$quill`
    /// (`quill::name_mismatch` / `quill::version_mismatch`).
    #[error(transparent)]
    Mismatch(#[from] RenderError),
}

impl BoundParseError {
    /// Every diagnostic this failure carries: the one parse diagnostic, or the
    /// mismatch's list.
    pub fn to_diagnostics(&self) -> Vec<Diagnostic> {
        match self {
            BoundParseError::Parse(e) => vec![e.to_diagnostic()],
            BoundParseError::Mismatch(e) => e.diagnostics().to_vec(),
        }
    }
}

impl Quill {
    /// Parse `markdown` and conform it against this quill: the **primary
    /// ingestion path**, and the one that lands the document at canonical rest.
    ///
    /// [`Document::parse`] followed by [`conform`](Self::conform), returning the
    /// same [`Parsed`] record with the conform diagnostics appended to
    /// `warnings`. A `$quill` naming a different quill fails here rather than
    /// conforming under the wrong schema; the escape hatch for a stale reference
    /// is the transport door ([`Document::parse`], retarget `$quill`, then
    /// [`conform`](Self::conform)).
    ///
    /// Bootstrap needs nothing extra: `$quill` is mandatory at the root, so the
    /// schema-free parse is the sniffer. Resolve the quill it names, then enter
    /// here.
    pub fn parse(&self, markdown: &str) -> Result<Parsed, BoundParseError> {
        let Parsed {
            mut document,
            mut warnings,
        } = Document::parse(markdown)?;
        warnings.extend(self.conform(&mut document)?);
        warnings.extend(self.unsupported_constructs(&document));
        Ok(Parsed { document, warnings })
    }

    /// Land `doc`'s declared content fields at their canonical rest, returning
    /// the `conform::*` diagnostics for the values that would not commit.
    ///
    /// The document's `$quill` is checked against this quill **before any
    /// mutation**, so a mismatch (`quill::name_mismatch` /
    /// `quill::version_mismatch`) leaves `doc` untouched.
    ///
    /// The walk covers the main card and every composable card whose `$kind`
    /// resolves, recursing through array `items` and object `properties`. Per
    /// field:
    ///
    /// - A `!must_fill` marker **anywhere** in the value skips the whole field:
    ///   the marker already names the state, and transporting one through a
    ///   reshaping coercion is ill-defined.
    /// - The value commits through the same strict write the typed writer runs,
    ///   so `richtext` lands as canonical content and `plaintext` as its literal
    ///   string; a refusal leaves the value authored and adds a diagnostic.
    /// - An equal value is **not written**: every write path clears the field's
    ///   `nested_comments`, so an unguarded conform would strip YAML comments and
    ///   move bytes on an untouched document.
    ///
    /// Undeclared fields, unknown card kinds, and nulls pass untouched, and a
    /// field whose declared type carries no content is the typed write's to
    /// canonicalize. Idempotent: a second call is a byte no-op and re-emits the
    /// identical diagnostics.
    pub fn conform(&self, doc: &mut Document) -> Result<Vec<Diagnostic>, RenderError> {
        self.check_quill_reference(doc)?;
        let config = self.config();
        let mut diags = Vec::new();
        conform_card(&config.main, doc.main_mut(), &DocPath::main(), &mut diags);
        for (index, card) in doc.cards_mut().iter_mut().enumerate() {
            // A card whose `$kind` declares no schema has no declared field to
            // conform: it passes untouched, as the render gate passes it. The
            // kind is copied out so the card is free to be borrowed mutably.
            let Some(kind) = card.kind().map(str::to_string) else {
                continue;
            };
            let Some(schema) = config.card_kind(&kind) else {
                continue;
            };
            conform_card(schema, card, &DocPath::card(Some(&kind), index), &mut diags);
        }
        Ok(diags)
    }
}

/// Conform one card's declared content fields in place. Field-name resolution
/// is the render gate's raw lookup (`schema.fields.get(name)`, no NFC respelling)
/// so the two walks cannot diverge on which fields count as declared.
fn conform_card(
    schema: &CardSchema,
    card: &mut Card,
    base: &DocPath,
    diags: &mut Vec<Diagnostic>,
) {
    let mut updates: Vec<(String, QuillValue)> = Vec::new();
    // Over `items()` rather than the map projection: a root `!must_fill` rides
    // on the payload item, nested ones on the value tree, and the skip rule
    // covers both.
    for item in card.payload().items() {
        let PayloadItem::Field {
            key: name,
            value,
            fill,
            ..
        } = item
        else {
            continue;
        };
        let Some(field) = schema.fields.get(name) else {
            continue;
        };
        // Only a field whose type tree bears a content leaf has a resting form
        // to enforce; a scalar field's shorthands are the typed write's to
        // canonicalize, not conform's. Inside a content-bearing field the whole
        // subtree conforms, which is what the typed write does to it too.
        if !field_contains_content(field) {
            continue;
        }
        // A marker anywhere in the value is the state: the payload item's own
        // flag carries a root marker, the value tree the nested ones. A null
        // needs no guard, since the strict write passes it through and the
        // no-op check below then skips the write.
        if *fill || !value.fill_paths().is_empty() {
            continue;
        }
        match resolve_field_write(name, value.clone(), field) {
            Ok(conformed) => {
                if &conformed != value {
                    updates.push((name.clone(), conformed));
                }
            }
            Err(e) => diags.push(conform_diagnostic(&e, base)),
        }
    }
    for (name, value) in updates {
        // Pre-validated by `resolve_field_write` (name and stored-value depth),
        // exactly as the typed commit's own insert.
        card.payload_mut().insert_unchecked(name, value);
    }
}

/// The `conform::*` diagnostic for a value the strict write refuses: the
/// `edit::*` class the typed write would have raised, re-namespaced and demoted
/// to a **warning**. Conform leaves the value authored, so this reports a
/// repairable departure rather than a refusal; the code family lets a consumer
/// route on "this field is not at rest" without parsing message text.
pub(crate) fn conform_diagnostic(err: &EditError, base: &DocPath) -> Diagnostic {
    let code = err.code().strip_prefix("edit::").unwrap_or(err.code());
    let mut diag = Diagnostic::new(Severity::Warning, err.to_string())
        .with_code(format!("conform::{code}"))
        .with_args(err.args());
    if let Some(path) = err.doc_path(base) {
        diag = diag.with_path(path.to_string());
    }
    diag
}

#[cfg(test)]
mod tests;
