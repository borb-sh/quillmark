//! Error types and diagnostics for parsing and rendering.
//!
//! A [`Diagnostic`] carries two independent, optional anchors:
//! [`Diagnostic::location`] is a source-text position (`file:line:column`) from
//! parsers and backend compilers; [`Diagnostic::path`] is a document-model
//! anchor into [`crate::document::Document`] from schema validation and
//! coercion, which run after line spans are gone.
//! [`DocPath`](crate::path::DocPath) is the only type that constructs, renders,
//! and parses that path form.

use std::collections::BTreeMap;

use crate::OutputFormat;

/// Build a [`Diagnostic::args`] map. Values pass through `serde_json`, so a
/// list arrives as a list and a count as a number.
macro_rules! diag_args {
    ($($key:literal => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::BTreeMap::<String, ::serde_json::Value>::new();
        $(map.insert($key.to_string(), ::serde_json::json!($value));)*
        map
    }};
}

pub(crate) use diag_args;

/// Maximum input size for markdown (10 MiB)
pub const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024;

/// Maximum YAML size (1 MiB)
pub const MAX_YAML_SIZE: usize = 1024 * 1024;

/// Maximum nesting depth for markdown structures (100 levels). Owned by the
/// markdown codecs in `quillmark-content` (the import guard) and re-exported
/// here so the typst backend's markup converter shares one limit: a document
/// that imports also renders, and vice versa.
pub use quillmark_content::MAX_NESTING_DEPTH;

/// Maximum nesting depth for an opaque JSON payload (128 levels). Owned by
/// `quillmark-content` and re-exported here so this crate's write surfaces and
/// the bindings' converters bound host values at the depth storage accepts.
pub use quillmark_content::MAX_JSON_DEPTH;

/// Maximum number of card blocks allowed per document
pub const MAX_CARD_COUNT: usize = 1000;

/// Maximum number of user fields allowed per card-yaml block. Counted after
/// `$`-key extraction, so system metadata is not charged against it.
pub const MAX_FIELD_COUNT: usize = 1000;

/// A YAML parse or emit failure, owned by this crate.
///
/// The YAML engine is an implementation detail: no public signature names it,
/// so this crate's major version is not chained to the engine's.
///
/// `line`/`column` are 1-indexed and present only when the engine located the
/// failure: always absent on the emit side, which has no input to point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    message: String,
    hint: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
}

impl YamlError {
    /// What went wrong, in YAML terms.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The concrete textual fix, when the failure is one this crate recognizes.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// 1-indexed line of the failure, when the engine located one.
    pub fn line(&self) -> Option<u32> {
        self.line
    }

    /// 1-indexed column of the failure, paired with [`Self::line`].
    pub fn column(&self) -> Option<u32> {
        self.column
    }

    /// A diagnostic under `code`, carrying the hint and (when the engine
    /// located the failure) a [`Location`] against `file`.
    pub fn to_diagnostic(&self, code: &str, file: &str) -> Diagnostic {
        let mut diag = Diagnostic::new(Severity::Error, self.message.clone())
            .with_code(code.to_string());
        if let (Some(line), Some(column)) = (self.line, self.column) {
            diag = diag.with_location(Location::new(file.to_string(), line, column));
        }
        match &self.hint {
            Some(h) => diag.with_hint(h.clone()),
            None => diag,
        }
    }

    /// `yaml` is the text that failed to parse: the hint derivation inspects
    /// it to name the offending construct.
    pub(crate) fn from_de(err: serde_saphyr::Error, yaml: &str) -> Self {
        // The enricher also strips the engine's own Rust API names, which it
        // appends to some messages.
        let enriched = crate::document::yaml_hints::enrich_yaml_error(&err.to_string(), yaml);
        let loc = err.location();
        Self {
            message: enriched.message,
            hint: enriched.hint,
            line: loc.and_then(|l| u32::try_from(l.line()).ok()),
            column: loc.and_then(|l| u32::try_from(l.column()).ok()),
        }
    }

    /// Emission has no input to point at, so no position and no hint.
    pub(crate) fn from_ser(err: serde_saphyr::ser::Error) -> Self {
        Self {
            message: err.to_string(),
            hint: None,
            line: None,
            column: None,
        }
    }
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The message already opens with the position and the engine's caret
        // diagram; line/column are the structured reading of the same fact.
        f.write_str(&self.message)
    }
}

impl std::error::Error for YamlError {}

/// Fatality is this two-value ladder and nothing else: `Error` blocks the
/// stage that emits it, `Warning` never does. There is no lint-level
/// configuration and no warning-to-error promotion; an informational aside is
/// a [`Diagnostic::hint`], not a severity.
///
/// The enum is open; a `_` arm should escalate to [`Severity::Error`], since
/// over-reporting an unrecognized level is safer than hiding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Severity {
    /// Blocks the stage that emits it.
    Error,
    /// Non-fatal.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Location {
    /// Source file name, e.g. `"plate.typ"` or `"input.md"`.
    pub file: String,
    /// 1-indexed line.
    pub line: u32,
    /// 1-indexed column.
    pub column: u32,
}

impl Location {
    pub fn new(file: String, line: u32, column: u32) -> Self {
        Self { file, line, column }
    }
}

/// Structured diagnostic information.
///
/// Cause chains are walked eagerly at construction, so a `Diagnostic` stays
/// `Clone` and serializable across every binding boundary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable error code, e.g. `"parse::empty_input"` or `"typst::type_error"`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    pub message: String,
    /// Source-text anchor, set by parsers and backend compilers. Independent
    /// of [`Self::path`]; the two may co-exist.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub location: Option<Location>,
    /// Document-model anchor: a [`DocPath`](crate::path::DocPath) rendering,
    /// set by schema validation and coercion.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
    /// The facts [`Self::message`] interpolates, keyed by name: with
    /// [`Self::code`], the substitution unit a consumer needs to word this
    /// diagnostic in its own language.
    ///
    /// One code carries one key set, tabulated in `prose/canon/ERROR.md`
    /// § "Diagnostic args" and tested against it. Values keep their JSON
    /// shape, so joining and pluralizing stay the consumer's locale decisions.
    /// Engine prose never rides under a key.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub args: BTreeMap<String, serde_json::Value>,
    /// Flattened cause chain, outermost first. Upstream English.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_chain: Vec<String>,
}

impl Diagnostic {
    pub fn new(severity: Severity, message: String) -> Self {
        Self {
            severity,
            code: None,
            message,
            location: None,
            path: None,
            hint: None,
            args: BTreeMap::new(),
            source_chain: Vec::new(),
        }
    }

    pub fn with_code(mut self, code: String) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_location(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }

    pub fn with_hint(mut self, hint: String) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Attach the message's substitution facts. See [`Self::args`].
    pub fn with_args(mut self, args: BTreeMap<String, serde_json::Value>) -> Self {
        self.args = args;
        self
    }

    /// Attach one substitution fact, for a code minted inline rather than from
    /// an error enum's `args()`. See [`Self::args`].
    pub fn with_arg(mut self, key: &str, value: serde_json::Value) -> Self {
        self.args.insert(key.to_string(), value);
        self
    }

    /// Walk `source`'s cause chain eagerly into [`Self::source_chain`].
    pub fn with_source(mut self, source: &(dyn std::error::Error + 'static)) -> Self {
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(source);
        while let Some(err) = current {
            self.source_chain.push(err.to_string());
            current = err.source();
        }
        self
    }

    pub fn fmt_pretty(&self) -> String {
        let mut result = format!(
            "[{}] {}",
            match self.severity {
                Severity::Error => "ERROR",
                Severity::Warning => "WARN",
            },
            self.message
        );

        if let Some(ref code) = self.code {
            result.push_str(&format!(" ({})", code));
        }

        if let Some(ref loc) = self.location {
            result.push_str(&format!("\n  --> {}:{}:{}", loc.file, loc.line, loc.column));
        }

        if let Some(ref path) = self.path {
            result.push_str(&format!("\n  at {}", path));
        }

        if let Some(ref hint) = self.hint {
            result.push_str(&format!("\n  hint: {}", hint));
        }

        result
    }

}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum ParseError {
    #[error("Input too large: {size} bytes (max: {max} bytes)")]
    InputTooLarge { size: usize, max: usize },

    /// A card-yaml block carries more user fields than [`MAX_FIELD_COUNT`]
    /// (spec §8), counted after `$`-key extraction. Code
    /// `parse::too_many_fields`.
    #[error("Too many fields in one card-yaml block: {count} (max: {max})")]
    TooManyFields { count: usize, max: usize },

    /// A document carries more composable cards than [`MAX_CARD_COUNT`]
    /// (spec §8). Code `parse::too_many_cards`.
    #[error("Too many cards: {count} (max: {max})")]
    TooManyCards { count: usize, max: usize },

    /// A card-yaml block is not a well-formed document structure. The message
    /// is minted at the site that refused it. Code `parse::invalid_structure`.
    #[error("{0}")]
    InvalidStructure(String),

    /// Markdown input was empty or whitespace-only. Code `parse::empty_input`.
    #[error("{0}")]
    EmptyInput(String),

    /// The document is missing its root `~~~` card-yaml block, or that block
    /// does not declare the required `$quill` system metadata.
    /// Code `parse::missing_quill`.
    #[error("{0}")]
    MissingQuill(String),

    /// A `$quill` reference failed to parse as a [`crate::version::QuillReference`].
    /// Code `parse::invalid_quill_reference`; carries
    /// [`crate::version::quill_ref_hint`] as its diagnostic hint.
    #[error("Invalid $quill reference '{value}': {reason}")]
    InvalidQuillReference {
        value: String,
        /// The `from_str` violation.
        reason: String,
    },

    /// A card body's markdown could not be imported into the content model:
    /// today only when container nesting exceeds
    /// [`MAX_NESTING_DEPTH`]. Code `parse::body_import`.
    #[error("{0}")]
    BodyImport(String),

    #[error("YAML error in {}: {message}", block_label(*block_index))]
    YamlErrorWithLocation {
        message: String,
        /// 1-indexed line of the failure in the source document, not in the
        /// block's YAML payload: the two numberings meet here.
        line: usize,
        /// 1-indexed column, paired with `line`.
        column: usize,
        /// 0-indexed metadata block.
        block_index: usize,
        hint: Option<String>,
    },
}

/// Name of the card-yaml block at `block_index`, by position: the parse failed
/// before `$kind` was readable.
fn block_label(block_index: usize) -> String {
    match block_index {
        0 => "the root card-yaml block".to_string(),
        n => format!("card-yaml block {}", n),
    }
}

/// The document a [`ParseError`] points at. Markdown reaches the engine as a
/// string, so the anchor names the input rather than a path on disk.
pub const DOCUMENT_FILE: &str = "input.md";

impl ParseError {
    /// The facts this error's message interpolates. See [`Diagnostic::args`].
    ///
    /// The four `String` variants contribute no keys: `EmptyInput` is one
    /// fixed sentence, and the rest carry prose minted per-site.
    pub fn args(&self) -> BTreeMap<String, serde_json::Value> {
        match self {
            ParseError::InputTooLarge { size, max } => diag_args! {
                "size" => size,
                "max" => max,
            },
            ParseError::TooManyFields { count, max } => diag_args! {
                "count" => count,
                "max" => max,
            },
            ParseError::TooManyCards { count, max } => diag_args! {
                "count" => count,
                "max" => max,
            },
            ParseError::InvalidStructure(_) => diag_args! {},
            ParseError::EmptyInput(_) => diag_args! {},
            ParseError::MissingQuill(_) => diag_args! {},
            ParseError::BodyImport(_) => diag_args! {},
            // `reason` is English prose and stays in `message`.
            ParseError::InvalidQuillReference { value, reason: _ } => diag_args! {
                "value" => value,
            },
            // The coordinates ride on the diagnostic's `location`; `message` is
            // the YAML engine's own prose and keeps no key.
            ParseError::YamlErrorWithLocation {
                message: _,
                line: _,
                column: _,
                block_index,
                hint: _,
            } => diag_args! {
                "blockIndex" => block_index,
            },
        }
    }

    /// This error as a [`Diagnostic`]: the `Display` rendering as `message`,
    /// under the variant's `parse::*` code, with the location, hint and
    /// [`args`](Self::args) the variant carries. The `#[error]` attribute is
    /// the one place a variant's English is spelled.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let base = Diagnostic::new(Severity::Error, self.to_string());
        let diag = match self {
            ParseError::InputTooLarge { .. } => {
                base.with_code("parse::input_too_large".to_string())
            }
            ParseError::TooManyFields { .. } => {
                base.with_code("parse::too_many_fields".to_string())
            }
            ParseError::TooManyCards { .. } => base.with_code("parse::too_many_cards".to_string()),
            ParseError::InvalidStructure(_) => {
                base.with_code("parse::invalid_structure".to_string())
            }
            ParseError::EmptyInput(_) => base.with_code("parse::empty_input".to_string()),
            ParseError::MissingQuill(_) => base.with_code("parse::missing_quill".to_string()),
            ParseError::BodyImport(_) => base.with_code("parse::body_import".to_string()),
            ParseError::InvalidQuillReference { .. } => base
                .with_code("parse::invalid_quill_reference".to_string())
                .with_hint(crate::version::quill_ref_hint().to_string()),
            ParseError::YamlErrorWithLocation {
                line, column, hint, ..
            } => {
                let d = base
                    .with_code("parse::yaml_error_with_location".to_string())
                    .with_location(Location::new(
                        DOCUMENT_FILE.to_string(),
                        *line as u32,
                        *column as u32,
                    ));
                match hint {
                    Some(h) => d.with_hint(h.clone()),
                    None => d,
                }
            }
        };
        diag.with_args(self.args())
    }
}

/// Main error type for rendering operations: a non-empty collection of
/// [`Diagnostic`]s.
///
/// There is no failure taxonomy beyond the diagnostics themselves: route on
/// each diagnostic's namespaced `code` (`parse::*`, `validation::*`,
/// `quill::*`, `typst::*`, `backend::*`, `engine::*`), not on a type. Every
/// consumer and binding handles rendering failure through this one shape.
#[derive(Debug)]
pub struct RenderError {
    diags: Vec<Diagnostic>,
}

impl RenderError {
    /// Wrap `diags` as a failure. Non-emptiness is only `debug_assert!`ed, so
    /// a release build can construct an empty `RenderError`; `Display` carries
    /// a fallback for that case.
    pub fn new(diags: Vec<Diagnostic>) -> Self {
        debug_assert!(
            !diags.is_empty(),
            "RenderError requires at least one diagnostic"
        );
        Self { diags }
    }

    /// Wrap a single diagnostic as a failure.
    pub fn from_diag(diag: Diagnostic) -> Self {
        Self { diags: vec![diag] }
    }

    /// A failure carrying one error diagnostic under `code`, the shape most
    /// engine-side refusals take. A diagnostic needing a hint, a path or args
    /// builds one and goes through [`from_diag`](Self::from_diag).
    pub fn coded(code: &str, message: impl Into<String>) -> Self {
        Self::from_diag(
            Diagnostic::new(Severity::Error, message.into()).with_code(code.to_string()),
        )
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diags
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diags
    }

    /// The summary line shared by `Display` and every binding's exception
    /// message: the sole diagnostic's `message` for one, an
    /// `"<N> error(s): <first message>"` aggregate for more. Bindings delegate
    /// here rather than re-deriving the rule.
    pub fn summary_message(diags: &[Diagnostic]) -> String {
        match diags {
            [d] => d.message.clone(),
            [first, ..] => format!("{} error(s): {}", diags.len(), first.message),
            [] => "render error".to_string(),
        }
    }
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", Self::summary_message(&self.diags))
    }
}

impl std::error::Error for RenderError {}

impl From<ParseError> for RenderError {
    fn from(err: ParseError) -> Self {
        RenderError::from_diag(err.to_diagnostic())
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct RenderResult {
    pub artifacts: Vec<crate::Artifact>,
    pub warnings: Vec<Diagnostic>,
    pub output_format: OutputFormat,
    /// Schema-field geometry, populated only when
    /// [`RenderOptions::regions`](crate::RenderOptions) is set. Page indices
    /// are document-space even under a `pages` subset render.
    pub regions: Vec<crate::RenderedRegion>,
}

impl RenderResult {
    pub fn new(artifacts: Vec<crate::Artifact>, output_format: OutputFormat) -> Self {
        Self {
            artifacts,
            warnings: Vec::new(),
            output_format,
            regions: Vec::new(),
        }
    }
}

pub fn print_errors(err: &RenderError) {
    for d in err.diagnostics() {
        eprintln!("{}", d.fmt_pretty());
    }
}

/// One sample per [`ParseError`] variant.
#[cfg(test)]
fn parse_error_samples() -> Vec<ParseError> {
    vec![
        ParseError::InputTooLarge { size: 2, max: 1 },
        ParseError::TooManyFields { count: 2, max: 1 },
        ParseError::TooManyCards { count: 2, max: 1 },
        ParseError::InvalidStructure("x".into()),
        ParseError::EmptyInput("x".into()),
        ParseError::MissingQuill("x".into()),
        ParseError::BodyImport("x".into()),
        ParseError::InvalidQuillReference {
            value: "a@b".into(),
            reason: "x".into(),
        },
        ParseError::YamlErrorWithLocation {
            message: "x".into(),
            line: 3,
            column: 1,
            block_index: 1,
            hint: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One spelling per variant: the `#[error]` attribute, which the
    /// diagnostic renders rather than restates.
    #[test]
    fn parse_diagnostic_message_is_the_display_rendering() {
        for err in super::parse_error_samples() {
            assert_eq!(err.to_diagnostic().message, err.to_string(), "{err:?}");
        }
    }

    #[test]
    fn test_diagnostic_with_source_chain() {
        let root_err = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let diag =
            Diagnostic::new(Severity::Error, "Rendering failed".to_string()).with_source(&root_err);

        assert_eq!(diag.source_chain.len(), 1);
        assert!(diag.source_chain[0].contains("File not found"));
    }

    #[test]
    fn test_render_error_display_aggregates_multi_diagnostic() {
        let err = RenderError::new(vec![
            Diagnostic::new(Severity::Error, "a".to_string()),
            Diagnostic::new(Severity::Error, "b".to_string()),
        ]);
        assert_eq!(err.to_string(), "2 error(s): a");
    }
}

/// The canon table in `prose/canon/ERROR.md` § "Diagnostic args" is a consumer
/// contract, so it is tested rather than maintained by hand.
#[cfg(test)]
mod args_canon {
    use std::collections::BTreeMap;

    use crate::document::EditError;
    use crate::quill::{CoercionError, ValidationError};

    /// `code` → its arg keys, sorted. Every variant of every enum on the
    /// structured surface appears once.
    fn minted() -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut add = |code: &str, args: BTreeMap<String, serde_json::Value>| {
            let keys: Vec<String> = args.keys().cloned().collect();
            assert!(
                out.insert(code.to_string(), keys).is_none(),
                "two samples for `{code}`: one code carries one payload"
            );
        };

        for e in [
            ValidationError::TypeMismatch {
                path: "main.n".into(),
                expected: "string".into(),
                actual: "integer".into(),
                source_token: "42".into(),
                default: Some("\"x\"".into()),
            },
            ValidationError::EnumViolation {
                path: "main.tone".into(),
                value: "loud".into(),
                allowed: vec!["quiet".into()],
            },
            ValidationError::FormatViolation {
                path: "main.when".into(),
                format: "date".into(),
            },
            ValidationError::UnknownCard {
                path: "cards[0]".into(),
                card: "ghost".into(),
            },
            ValidationError::BodyDisabled {
                path: "cards.sig[0].body".into(),
                card: "sig".into(),
            },
            ValidationError::NotInline {
                path: "main.title".into(),
            },
            ValidationError::NotPlain {
                path: "main.title".into(),
            },
        ] {
            add(e.code(), e.args());
        }

        for e in [
            EditError::InvalidFieldName("9bad".into()),
            EditError::unknown_field("nope"),
            EditError::InvalidKindName("Bad".into()),
            EditError::ReservedKind,
            EditError::RootOnlyEntry {
                key: "$quill".into(),
            },
            EditError::IndexOutOfRange { index: 3, len: 1 },
            EditError::ValueTooDeep { max: 8 },
            EditError::FillOnMapping {
                field: "addr".into(),
            },
            EditError::Import(quillmark_content::import::ImportError::NestingTooDeep {
                depth: 9,
                max: 8,
            }),
            EditError::FieldDecode {
                field: "body".into(),
                at: Vec::new(),
                codec: crate::document::edit::CODEC_RICHTEXT.into(),
                message: "x".into(),
            },
            EditError::FieldNotContent {
                field: "qty".into(),
                at: Vec::new(),
                declared: "integer".into(),
            },
            EditError::FieldNotInline {
                field: "body".into(),
                codec: crate::document::edit::CODEC_PLAINTEXT.into(),
            },
            EditError::FieldCoercionFailed {
                field: "n".into(),
                target: "integer".into(),
                message: "x".into(),
            },
            EditError::ContentApply(quillmark_content::ApplyError::LineOutOfRange {
                line: 3,
                lines: 1,
            }),
        ] {
            add(e.code(), e.args());
        }

        // The `conform::*` family: the strict write's refusals, re-namespaced by
        // `conform_diagnostic`. Minted through that function rather than
        // re-derived, so the table cannot drift from the code that stamps it.
        for e in [
            EditError::InvalidFieldName("9bad".into()),
            EditError::ValueTooDeep { max: 8 },
            EditError::FieldNotInline {
                field: "body".into(),
                codec: crate::document::edit::CODEC_RICHTEXT.into(),
            },
            EditError::FieldDecode {
                field: "body".into(),
                at: Vec::new(),
                codec: crate::document::edit::CODEC_PLAINTEXT.into(),
                message: "x".into(),
            },
            EditError::FieldCoercionFailed {
                field: "n".into(),
                target: "integer".into(),
                message: "x".into(),
            },
        ] {
            let diag = crate::quill::conform::conform_diagnostic(&e, &crate::DocPath::main());
            add(
                diag.code.as_deref().expect("conform diagnostics carry a code"),
                diag.args,
            );
        }

        for e in super::parse_error_samples() {
            let diag = e.to_diagnostic();
            add(diag.code.as_deref().expect("parse errors carry a code"), diag.args);
        }

        // Two codes are minted beside their error rather than from a variant:
        // `compose::coercion_error` wraps the whole `CoercionError`, and
        // `compose::fill_warning` has no error type at all.
        add(
            "validation::coercion_failed",
            CoercionError::Uncoercible {
                path: "card_kinds.sig.n".into(),
                value: "\"x\"".into(),
                target: "integer".into(),
                reason: "string is not a valid integer".into(),
            }
            .args(),
        );
        // Two constructors, one code: sampling one and pinning the other against
        // it is what stops the pair from drifting into two key sets.
        let path = crate::path::DocPath::main().field("subject");
        let marker = crate::quill::compose::fill_warning(&path);
        let unauthored = crate::quill::compose::unauthored_warning(&path);
        assert_eq!(
            marker.args.keys().collect::<Vec<_>>(),
            unauthored.args.keys().collect::<Vec<_>>(),
            "both `validation::must_fill` triggers must carry one key set"
        );
        add("validation::must_fill", marker.args);
        // Two constructors again, one per cell a blueprint writes a value into.
        let field_example = crate::quill::compose::example_unchanged_warning(
            &path,
            &serde_json::json!("Duty Title"),
        );
        let body_example = crate::quill::compose::body_example_unchanged_warning(
            &crate::path::DocPath::main().body(),
            "Write main body here.",
        );
        assert_eq!(
            field_example.args.keys().collect::<Vec<_>>(),
            body_example.args.keys().collect::<Vec<_>>(),
            "both `validation::example_unchanged` triggers must carry one key set"
        );
        add("validation::example_unchanged", field_example.args);
        // Built at the variant walk rather than from an error type: a stranded
        // value is well-formed, so there is nothing to fail.
        add(
            "validation::out_of_variant",
            crate::quill::compose::out_of_variant_warning(&path, "CUI", "UNCLASSIFIED").args,
        );
        // Built at the pre-render walk rather than from an error type: a quill
        // declares the construct, so there is nothing to fail.
        add("plate::unsupported_construct", {
            let mut args = BTreeMap::new();
            args.insert("construct".to_string(), "rule".into());
            args.insert("count".to_string(), 3.into());
            args
        });

        out
    }

    /// The canon table's rows. `—` is no keys; a trailing `?` marks a
    /// conditional key, which [`minted`]'s sample supplies.
    fn declared() -> BTreeMap<String, Vec<String>> {
        let canon = include_str!("../../../prose/canon/ERROR.md");
        let mut rows = canon
            .lines()
            .skip_while(|l| !l.starts_with("| Code | Args | Outcome |"))
            .skip(2)
            .take_while(|l| l.starts_with('|'));

        let mut out = BTreeMap::new();
        for row in &mut rows {
            let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
            assert_eq!(cells.len(), 3, "malformed canon row: {row}");
            let code = cells[0].trim_matches('`').to_string();
            let keys = if cells[1] == "—" {
                Vec::new()
            } else {
                let mut keys: Vec<String> = cells[1]
                    .split(',')
                    .map(|k| k.trim().trim_end_matches('?').trim_matches('`').to_string())
                    .collect();
                keys.sort();
                keys
            };
            assert!(out.insert(code, keys).is_none(), "duplicate canon row: {row}");
        }
        assert!(!out.is_empty(), "canon args table not found in ERROR.md");
        out
    }

    /// A code off the table carries no args, so a consumer's template falls
    /// back rather than half-filling.
    #[test]
    fn out_of_scope_codes_carry_no_args() {
        let diags = crate::quill::QuillConfig::from_yaml_with_warnings(
            r#"
Quill:
  name: t
  version: "1.0"
  backend: typst
  description: A slot whose literal contradicts its declared type

main:
  fields:
    title:
      type: string
      default: 42
"#,
        )
        .expect_err("a default that contradicts its type fails config validation");

        assert!(
            diags.iter().any(|d| d
                .code
                .as_deref()
                .is_some_and(|c| c.starts_with("quill::"))),
            "expected a quill:: diagnostic, got {:?}",
            diags.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        for d in &diags {
            assert!(
                d.args.is_empty(),
                "`{:?}` is off the canon table and must carry no args",
                d.code
            );
        }
    }

    #[test]
    fn diagnostic_args_match_canon() {
        assert_eq!(
            declared(),
            minted(),
            "`ERROR.md` § \"Diagnostic args\" and the minted args disagree"
        );
    }
}
