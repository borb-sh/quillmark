//! Core types for rendering and output formats.

/// Output formats supported by backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub enum OutputFormat {
    Svg,
    Pdf,
    /// Raster output.
    Png,
}

impl OutputFormat {
    /// Every output format, in a stable order.
    ///
    /// A slice, not an array: an array's length is part of its type, so a
    /// fourth format would break every caller that names this type.
    pub const ALL: &'static [OutputFormat] =
        &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

    /// The lowercase string id (`"pdf"`, `"svg"`, `"png"`): the format ↔ string
    /// mapping every binding and the CLI share.
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Pdf => "pdf",
            OutputFormat::Svg => "svg",
            OutputFormat::Png => "png",
        }
    }

    /// The IANA MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            OutputFormat::Pdf => "application/pdf",
            OutputFormat::Svg => "image/svg+xml",
            OutputFormat::Png => "image/png",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a string does not name an [`OutputFormat`]. The field
/// is the input as parsed, lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseOutputFormatError(pub String);

impl ParseOutputFormatError {
    pub fn new(input: impl Into<String>) -> Self {
        Self(input.into())
    }
}

impl std::fmt::Display for ParseOutputFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let choices: Vec<&str> = OutputFormat::ALL.iter().map(|fmt| fmt.as_str()).collect();
        write!(
            f,
            "Invalid output format: {}. Must be one of: {}",
            self.0,
            choices.join(", ")
        )
    }
}

impl std::error::Error for ParseOutputFormatError {}

impl std::str::FromStr for OutputFormat {
    type Err = ParseOutputFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pdf" => Ok(OutputFormat::Pdf),
            "svg" => Ok(OutputFormat::Svg),
            "png" => Ok(OutputFormat::Png),
            other => Err(ParseOutputFormatError::new(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn output_format_str_round_trips() {
        for &fmt in OutputFormat::ALL {
            assert_eq!(OutputFormat::from_str(fmt.as_str()), Ok(fmt));
            assert_eq!(fmt.to_string(), fmt.as_str());
            assert_eq!(
                OutputFormat::from_str(&fmt.as_str().to_uppercase()),
                Ok(fmt)
            );
        }
    }

    #[test]
    fn unknown_output_format_names_the_input_lowercased() {
        assert_eq!(
            OutputFormat::from_str("Docx"),
            Err(ParseOutputFormatError::new("docx"))
        );
        assert_eq!(ParseOutputFormatError::new("docx").0, "docx");
    }

    #[test]
    fn ppi_falls_back_to_the_default() {
        assert_eq!(RenderOptions::default().ppi_or_default(), 144.0);
        assert_eq!(
            RenderOptions::default().with_ppi(300.0).ppi_or_default(),
            300.0
        );
    }
}

/// An artifact produced by rendering.
#[derive(Debug)]
#[non_exhaustive]
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub output_format: OutputFormat,
}

impl Artifact {
    pub fn new(bytes: Vec<u8>, output_format: OutputFormat) -> Self {
        Self {
            bytes,
            output_format,
        }
    }
}

/// Internal rendering options.
///
/// Built from [`Default`] and narrowed by the `with_*` setters:
///
/// ```
/// use quillmark_core::{OutputFormat, RenderOptions};
///
/// let opts = RenderOptions::default()
///     .with_output_format(OutputFormat::Png)
///     .with_ppi(300.0);
/// ```
///
/// `#[non_exhaustive]` forbids every out-of-crate struct expression, functional
/// update included, so the setters are the construction path. Assigning an
/// individual field still works: `opts.ppi = Some(300.0)` is the setter.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RenderOptions {
    pub output_format: Option<OutputFormat>,
    /// Pixels per inch for raster output formats (e.g., PNG).
    /// Ignored for vector/document formats (PDF, SVG).
    /// `None` resolves to [`RenderOptions::DEFAULT_PPI`] through
    /// [`ppi_or_default`](RenderOptions::ppi_or_default).
    ///
    /// Must be finite and positive, and small enough to keep every rendered
    /// page under [`MAX_RASTER_PIXELS`](crate::MAX_RASTER_PIXELS); a raster
    /// backend refuses anything else with a `backend::invalid_raster_scale`
    /// [`RenderError`](crate::RenderError).
    pub ppi: Option<f32>,
    /// Optional 0-based page indices to render (e.g., `vec![0, 2]` for
    /// the first and third pages). `None` renders all pages. Any index
    /// `>= page_count` fails with a `RenderError` carrying the
    /// `typst::page_index_out_of_bounds` code: call
    /// `LiveSession::page_count()` first if validation is needed.
    /// Backends that do not support page selection (notably PDF) fail with a
    /// `RenderError` carrying `typst::pdf_page_selection_not_supported` when
    /// this is `Some`.
    pub pages: Option<Vec<usize>>,
    /// Override for the PDF `/Info` `/Producer` metadata string. `None` uses
    /// the backend default (`Quillmark <version>` for the Typst backend).
    /// Applies to PDF output only; ignored by SVG/PNG.
    pub producer: Option<String>,
    /// Populate [`RenderResult::regions`](crate::RenderResult) with the
    /// schema-field geometry sidecar, for consumers without a live session.
    /// Default `false`, so exports pay no introspection cost.
    ///
    /// The sidecar always describes the **whole document**: page indices are
    /// document-space and unaffected by a `pages` subset selection.
    pub regions: bool,
}

impl RenderOptions {
    /// The pixels per inch every raster backend uses when [`ppi`](Self::ppi) is
    /// `None`: 2x at 72 pt/inch.
    pub const DEFAULT_PPI: f32 = 144.0;

    /// [`ppi`](Self::ppi), or [`DEFAULT_PPI`](Self::DEFAULT_PPI).
    pub fn ppi_or_default(&self) -> f32 {
        self.ppi.unwrap_or(Self::DEFAULT_PPI)
    }

    pub fn with_output_format(mut self, output_format: OutputFormat) -> Self {
        self.output_format = Some(output_format);
        self
    }

    pub fn with_ppi(mut self, ppi: f32) -> Self {
        self.ppi = Some(ppi);
        self
    }

    pub fn with_pages(mut self, pages: Vec<usize>) -> Self {
        self.pages = Some(pages);
        self
    }

    pub fn with_producer(mut self, producer: String) -> Self {
        self.producer = Some(producer);
        self
    }

    pub fn with_regions(mut self, regions: bool) -> Self {
        self.regions = regions;
        self
    }
}
