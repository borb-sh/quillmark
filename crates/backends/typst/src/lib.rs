//! # Typst Backend for Quillmark
//!
//! This crate provides a complete Typst backend implementation that converts Markdown
//! documents to PDF and SVG formats via the Typst typesetting system.
//!
//! ## Overview
//!
//! The primary entry point is the [`TypstBackend`] struct, which implements the
//! [`Backend`] trait from `quillmark-core`. Users typically interact with this backend
//! through the high-level `Quill` API from the `quillmark` crate.
//!
//! ## Features
//!
//! - Converts CommonMark Markdown to Typst markup
//! - Compiles Typst documents to PDF and SVG formats
//! - Provides template filters for YAML data transformation
//! - Manages fonts, assets, and packages dynamically
//! - Embeds unsigned AcroForm signature widgets via the
//!   `signature-field` helper (see `signature-field` in the `lib.typ`
//!   helper package; only the PDF output carries the widget — SVG and
//!   PNG render an invisible placeholder)
//! - Thread-safe for concurrent rendering
//!
//! ## Modules
//!
//! - [`convert`] - Markdown to Typst conversion utilities
//! - [`compile`] - Typst to PDF/SVG compilation functions
//!
//! Note: The `error_mapping` module provides internal utilities for converting Typst
//! diagnostics to Quillmark diagnostics and is not part of the public API.

pub mod compile;
pub mod convert;
mod error_mapping;

pub mod helper;
mod sig_overlay;
mod world;

/// Utilities exposed for fuzzing tests.
/// Not intended for general use.
#[doc(hidden)]
pub mod fuzz_utils {
    pub use super::helper::inject_json;
}

use convert::mark_to_typst;
use quillmark_core::{
    quill::build_transform_schema, session::SessionHandle, Backend, Diagnostic, OutputFormat,
    QuillSource, QuillValue, RenderError, RenderOptions, RenderResult, RenderSession, Severity,
};
use std::any::Any;

/// Typst backend implementation for Quillmark.
#[derive(Debug)]
pub struct TypstBackend;

const SUPPORTED_FORMATS: &[OutputFormat] =
    &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

/// Typst-specific render session.
///
/// Holds the cached `PagedDocument` produced by [`Backend::open`] and exposes
/// Typst-only operations (page geometry, raster rendering) used by the WASM
/// canvas painter. Reach this from a [`RenderSession`] via
/// [`typst_session_of`].
#[derive(Debug)]
pub struct TypstSession {
    document: typst::layout::PagedDocument,
    page_count: usize,
    /// Extracted once at `open`. Consumed by PDF inject; unused for SVG/PNG.
    sig_placements: Vec<sig_overlay::SigPlacement>,
}

impl TypstSession {
    /// Page dimensions in Typst points (1 pt = 1/72 inch).
    ///
    /// Returns `None` if `page` is out of range.
    pub fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let frame = &self.document.pages.get(page)?.frame;
        let size = frame.size();
        Some((size.x.to_pt() as f32, size.y.to_pt() as f32))
    }

    /// Render `page` to a non-premultiplied RGBA8 buffer at `scale`× the
    /// natural 72 ppi (i.e. `scale = 1` → 1 device pixel per Typst pt).
    ///
    /// Returns `(width_px, height_px, rgba)`. The buffer is `width_px *
    /// height_px * 4` bytes, row-major, ready to hand to `ImageData` or any
    /// other RGBA consumer. Returns `None` if `page` is out of range.
    pub fn render_rgba(&self, page: usize, scale: f32) -> Option<(u32, u32, Vec<u8>)> {
        let p = self.document.pages.get(page)?;
        let pixmap = typst_render::render(p, scale);
        let width = pixmap.width();
        let height = pixmap.height();
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for px in pixmap.pixels() {
            let c = px.demultiply();
            rgba.push(c.red());
            rgba.push(c.green());
            rgba.push(c.blue());
            rgba.push(c.alpha());
        }
        Some((width, height, rgba))
    }
}

impl SessionHandle for TypstSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);

        if !SUPPORTED_FORMATS.contains(&format) {
            return Err(RenderError::FormatNotSupported {
                diag: Box::new(
                    Diagnostic::new(
                        Severity::Error,
                        format!("{:?} not supported by typst backend", format),
                    )
                    .with_code("backend::format_not_supported".to_string())
                    .with_hint(format!("Supported formats: {:?}", SUPPORTED_FORMATS)),
                ),
            });
        }

        compile::render_document_pages(
            &self.document,
            opts.pages.as_deref(),
            format,
            opts.ppi,
            &self.sig_placements,
        )
    }

    fn page_count(&self) -> usize {
        self.page_count
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Borrow the [`TypstSession`] underlying a [`RenderSession`], if the session
/// was opened by the Typst backend.
///
/// Returns `None` for any other backend. Bindings that need Typst-only
/// capabilities (canvas paint, page geometry) call this to access them
/// without forcing core to know about backend specifics.
pub fn typst_session_of(session: &RenderSession) -> Option<&TypstSession> {
    session.handle().as_any().downcast_ref::<TypstSession>()
}

impl Backend for TypstBackend {
    fn id(&self) -> &'static str {
        "typst"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        plate_content: &str,
        source: &QuillSource,
        json_data: &serde_json::Value,
    ) -> Result<RenderSession, RenderError> {
        let transformed_json =
            transform_plate_json(json_data, &build_transform_schema(source.config()));

        let json_str =
            serde_json::to_string(&transformed_json).unwrap_or_else(|_| "{}".to_string());
        let document = compile::compile_to_document(source, plate_content, &json_str)?;
        let page_count = document.pages.len();
        let sig_placements = sig_overlay::extract(&document)?;
        let session = TypstSession {
            document,
            page_count,
            sig_placements,
        };
        Ok(RenderSession::new(Box::new(session)))
    }
}

impl Default for TypstBackend {
    /// Creates a new [`TypstBackend`] instance.
    fn default() -> Self {
        Self
    }
}

/// Check if a field schema indicates markdown content.
///
/// A field is considered markdown if it has:
/// - `contentMediaType = "text/markdown"`
fn is_markdown_field(field_schema: &serde_json::Value) -> bool {
    field_schema
        .get("contentMediaType")
        .and_then(|v| v.as_str())
        .map(|s| s == "text/markdown")
        .unwrap_or(false)
}

/// Check if a field schema indicates a date field.
///
/// A field is considered a date if it has:
/// - `type = "string"`
/// - `format = "date"`
fn is_date_field(field_schema: &serde_json::Value) -> bool {
    let is_string = field_schema
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s == "string")
        .unwrap_or(false);

    let is_date_format = field_schema
        .get("format")
        .and_then(|v| v.as_str())
        .map(|s| s == "date")
        .unwrap_or(false);

    is_string && is_date_format
}

/// Transform a Quillmark plate-wire document for the Typst backend.
///
/// The input is the `{ "main": {...}, "cards": [...] }` shape produced by
/// `Document::to_plate_json`. Markdown-typed fields on the main card and on
/// every card are converted to Typst markup with `mark_to_typst()`.
///
/// A `__meta__` key is added to the result carrying the names of converted
/// content fields and of date fields, which the `quillmark-helper` package
/// consumes to auto-evaluate markup strings and parse dates at Typst runtime.
fn transform_plate_json(json_data: &serde_json::Value, schema: &QuillValue) -> serde_json::Value {
    let schema_json = schema.as_json();
    let main_props = schema_json.get("properties").and_then(|v| v.as_object());
    let defs = schema_json.get("$defs").and_then(|v| v.as_object());

    // ── Main card ───────────────────────────────────────────────────────────
    let empty = serde_json::Map::new();
    let main_obj = json_data
        .get("main")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let (main_transformed, content_fields) = convert_markdown_fields(main_obj, main_props);
    let date_fields = date_field_names(main_props);

    // ── Cards ───────────────────────────────────────────────────────────────
    let mut transformed_cards: Vec<serde_json::Value> = Vec::new();
    if let Some(cards) = json_data.get("cards").and_then(|v| v.as_array()) {
        for card in cards {
            match card.as_object() {
                Some(card_obj) => {
                    let kind = card_obj.get("CARD").and_then(|v| v.as_str()).unwrap_or("");
                    let card_props = defs
                        .and_then(|d| d.get(&format!("{kind}_card")))
                        .and_then(|d| d.get("properties"))
                        .and_then(|v| v.as_object());
                    let (transformed, _) = convert_markdown_fields(card_obj, card_props);
                    transformed_cards.push(serde_json::Value::Object(transformed));
                }
                None => transformed_cards.push(card.clone()),
            }
        }
    }

    // ── Per-card-kind content/date field names (schema-driven) ───────────────
    let mut card_content_fields = serde_json::Map::new();
    let mut card_date_fields = serde_json::Map::new();
    if let Some(defs) = defs {
        for (def_name, def_schema) in defs {
            if let Some(card_kind) = def_name.strip_suffix("_card") {
                let props = def_schema.get("properties").and_then(|v| v.as_object());
                let content = markdown_field_names(props);
                if !content.is_empty() {
                    card_content_fields
                        .insert(card_kind.to_string(), serde_json::json!(content));
                }
                let dates = date_field_names(props);
                if !dates.is_empty() {
                    card_date_fields.insert(card_kind.to_string(), serde_json::json!(dates));
                }
            }
        }
    }

    let mut meta = serde_json::Map::new();
    meta.insert("content_fields".to_string(), serde_json::json!(content_fields));
    meta.insert(
        "card_content_fields".to_string(),
        serde_json::Value::Object(card_content_fields),
    );
    meta.insert("date_fields".to_string(), serde_json::json!(date_fields));
    meta.insert(
        "card_date_fields".to_string(),
        serde_json::Value::Object(card_date_fields),
    );

    let mut root = serde_json::Map::new();
    root.insert("main".to_string(), serde_json::Value::Object(main_transformed));
    root.insert("cards".to_string(), serde_json::Value::Array(transformed_cards));
    root.insert("__meta__".to_string(), serde_json::Value::Object(meta));
    serde_json::Value::Object(root)
}

/// Convert the markdown-typed fields of `obj` to Typst markup, using `props`
/// (the card's field-properties schema) to identify which fields are markdown.
///
/// Returns the rewritten object and the names of the fields actually
/// converted. Fields absent from `props` (e.g. the `QUILL`/`CARD`
/// discriminators) pass through unchanged.
fn convert_markdown_fields(
    obj: &serde_json::Map<String, serde_json::Value>,
    props: Option<&serde_json::Map<String, serde_json::Value>>,
) -> (serde_json::Map<String, serde_json::Value>, Vec<String>) {
    let mut result = obj.clone();
    let mut converted = Vec::new();
    if let Some(props) = props {
        for (name, value) in obj {
            if props.get(name).map(is_markdown_field).unwrap_or(false) {
                if let Some(content) = value.as_str() {
                    if let Ok(markup) = mark_to_typst(content) {
                        result.insert(name.clone(), serde_json::Value::String(markup));
                        converted.push(name.clone());
                    }
                }
            }
        }
    }
    (result, converted)
}

/// Names of the markdown-typed fields declared in a field-properties schema.
fn markdown_field_names(
    props: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Vec<String> {
    props
        .map(|p| {
            p.iter()
                .filter(|(_, fs)| is_markdown_field(fs))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Names of the date/date-time-typed fields declared in a field-properties schema.
fn date_field_names(props: Option<&serde_json::Map<String, serde_json::Value>>) -> Vec<String> {
    props
        .map(|p| {
            p.iter()
                .filter(|(_, fs)| is_date_field(fs))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_backend_info() {
        let backend = TypstBackend;
        assert_eq!(backend.id(), "typst");
        assert!(backend.supported_formats().contains(&OutputFormat::Pdf));
        assert!(backend.supported_formats().contains(&OutputFormat::Svg));
    }

    #[test]
    fn test_is_markdown_field() {
        let markdown_schema = json!({
            "type": "string",
            "contentMediaType": "text/markdown"
        });
        assert!(is_markdown_field(&markdown_schema));

        let string_schema = json!({
            "type": "string"
        });
        assert!(!is_markdown_field(&string_schema));

        let other_media_type = json!({
            "type": "string",
            "contentMediaType": "text/plain"
        });
        assert!(!is_markdown_field(&other_media_type));
    }

    #[test]
    fn test_is_date_field() {
        let date_schema = json!({
            "type": "string",
            "format": "date"
        });
        assert!(is_date_field(&date_schema));

        let date_time_schema = json!({
            "type": "string",
            "format": "date-time"
        });
        assert!(!is_date_field(&date_time_schema));

        let non_string_date_schema = json!({
            "type": "number",
            "format": "date"
        });
        assert!(!is_date_field(&non_string_date_schema));
    }

    #[test]
    fn test_transform_plate_json_basic() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "BODY": { "type": "string", "contentMediaType": "text/markdown" }
            }
        }));

        let json_data = json!({
            "main": { "title": "My Title", "BODY": "This is **bold** text." },
            "cards": []
        });

        let result = transform_plate_json(&json_data, &schema);

        // title should be unchanged
        assert_eq!(result["main"]["title"], json!("My Title"));

        // BODY should be converted to Typst markup
        let body = result["main"]["BODY"].as_str().unwrap();
        assert!(body.contains("#strong[bold]"));
    }

    #[test]
    fn test_transform_plate_json_no_markdown() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "count": { "type": "number" }
            }
        }));

        let json_data = json!({
            "main": { "title": "My Title", "count": 42 },
            "cards": []
        });

        let result = transform_plate_json(&json_data, &schema);

        // All fields should be unchanged
        assert_eq!(result["main"]["title"], json!("My Title"));
        assert_eq!(result["main"]["count"].as_i64(), Some(42));
    }

    #[test]
    fn test_transform_plate_json_card_markdown() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {},
            "$defs": {
                "quote_card": {
                    "type": "object",
                    "properties": {
                        "BODY": { "type": "string", "contentMediaType": "text/markdown" }
                    }
                }
            }
        }));

        let json_data = json!({
            "main": {},
            "cards": [ { "CARD": "quote", "BODY": "_italic_ text" } ]
        });

        let result = transform_plate_json(&json_data, &schema);

        let body = result["cards"][0]["BODY"].as_str().unwrap();
        assert!(body.contains("#emph[italic]"));
        assert_eq!(result["cards"][0]["CARD"], json!("quote"));
    }

    #[test]
    fn test_transform_plate_json_collects_top_level_date_metadata() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "date": { "type": "string", "format": "date" },
                "timestamp": { "type": "string", "format": "date-time" }
            }
        }));

        let json_data = json!({ "main": { "title": "My Title" }, "cards": [] });

        let result = transform_plate_json(&json_data, &schema);
        assert_eq!(result["__meta__"]["date_fields"], json!(["date"]));
    }

    #[test]
    fn test_transform_plate_json_collects_card_date_metadata() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {},
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": {
                        "date": { "type": "string", "format": "date" },
                        "created_at": { "type": "string", "format": "date-time" },
                        "BODY": { "type": "string", "contentMediaType": "text/markdown" }
                    }
                }
            }
        }));

        let json_data = json!({ "main": {}, "cards": [] });
        let result = transform_plate_json(&json_data, &schema);

        assert_eq!(
            result["__meta__"]["card_date_fields"]["indorsement"],
            json!(["date"])
        );
    }
}
