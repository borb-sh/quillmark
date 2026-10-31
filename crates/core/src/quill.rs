//! The `Quill` type: portable, validated quill data.

mod authoring;
mod bundle;
pub(crate) mod compose;
mod config;
pub(crate) mod conform;
mod example;
mod resolved;
mod fill;
mod formats;
mod ignore;
mod load;
mod package_meta;
mod schema;
mod schema_yaml;
mod seed;
mod tree;
mod types;
pub(crate) mod validation;

pub use authoring::AUTHORING_INVALID_SHAPE;
pub use bundle::{
    BUNDLE_EXTENSION, BUNDLE_MEDIA_TYPE, MAX_BUNDLE_BYTES, MAX_BUNDLE_ENTRIES, MAX_BUNDLE_FILE_BYTES,
};
pub use config::{CoercionError, QuillConfig};
pub(crate) use config::{
    Leniency, RICHTEXT_SINGLE_LINE_TARGET, TEXT_SINGLE_LINE_TARGET,
};
pub use compose::PLATE_PATH_KEY;
pub use conform::BoundParseError;
pub use resolved::{FieldSource, Resolved, ResolvedCard, ResolvedField, ResolvedMain};
pub(crate) use resolved::resolve_document;
pub use fill::blank;
pub use formats::{parse_date, parse_datetime, CalendarDate, ParseDateError, TODAY};
pub use ignore::QuillIgnore;
pub use schema::{
    build_transform_schema, SchemaCodec, AUTHORING_KIND_KEY, CONTENT_MEDIA_TYPE,
    MARKDOWN_MEDIA_TYPE, QUILLMARK_MAX_ITEMS_KEY, QUILLMARK_MAX_LENGTH_KEY,
    QUILLMARK_MULTILINE_KEY, QUILLMARK_ROSTER_KEY,
};
pub use tree::FileTreeNode;
pub use validation::ValidationError;
pub use types::{
    BlockConstruct, BodyCardSchema, CardSchema, FieldLayout, FieldSchema, FieldType, GroupRegistry,
    GroupSchema, UiCardSchema, UiFieldSchema, VariantFields, MATRIX_HELD_KEY,
    MATRIX_RESERVED_COLUMNS, MATRIX_TITLE_KEY, VARIANT_DISCRIMINANT_KEY,
};

/// Portable, validated quill data: the file bundle and parsed config of an
/// authored quill, tagged with its *declared* backend id.
///
/// A `Quill` holds no backend and needs no engine to construct or use: every
/// method here is a pure read of its parsed config. Rendering is the engine's
/// job. Construct with [`Quill::from_tree`] or [`Quill::from_bundle`] (pure),
/// or `quillmark::quill_from_path` (filesystem stays out of core).
#[derive(Clone)]
pub struct Quill {
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
    pub(crate) warnings: Vec<crate::error::Diagnostic>,
}

impl Quill {
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The backend identifier declared in Quill.yaml (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        &self.config.backend
    }

    pub fn config(&self) -> &QuillConfig {
        &self.config
    }

    /// The advisory diagnostics the load collected: what is wrong with this
    /// quill short of refusing it. They ride the quill so every construction
    /// door keeps them, and a host reads them whenever it likes rather than at
    /// the one moment the loader returns.
    pub fn warnings(&self) -> &[crate::error::Diagnostic] {
        &self.warnings
    }

    /// A schema-bound [`TypedWriter`](crate::writer::TypedWriter) over `doc`:
    /// the front door for typed field writes.
    pub fn writer<'a>(
        &'a self,
        doc: &'a mut crate::document::Document,
    ) -> crate::writer::TypedWriter<'a> {
        crate::writer::TypedWriter::new(&self.config, doc)
    }

    /// A schema-bound [`TypedReader`](crate::reader::TypedReader) over `doc`:
    /// the read twin of [`writer`](Self::writer). Interprets each field by its
    /// declared type, and reads an undeclared name as the typo it is rather
    /// than as absent.
    pub fn reader<'a>(
        &'a self,
        doc: &'a crate::document::Document,
    ) -> crate::reader::TypedReader<'a> {
        crate::reader::TypedReader::new(&self.config, doc)
    }

    pub fn files(&self) -> &FileTreeNode {
        &self.files
    }

    /// Flatten this quill's file bundle into `(path, contents)` pairs, the
    /// inverse of [`Quill::from_tree`]'s input: how a quill crosses a process
    /// or WASM boundary as plain data. Every file is preserved, empty
    /// directories are not.
    pub fn to_tree(&self) -> Vec<(String, Vec<u8>)> {
        self.files.flatten()
    }
}

impl std::fmt::Debug for Quill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quill")
            .field("name", &self.config.name)
            .field("backend_id", &self.config.backend)
            .field("files", &"<FileTreeNode>")
            .finish()
    }
}

#[cfg(test)]
mod tests;

/// Build a minimal [`Quill`] from inline `Quill.yaml` with no filesystem deps.
#[cfg(test)]
pub(crate) fn quill_from_yaml(yaml: &str) -> Quill {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: yaml.as_bytes().to_vec(),
        },
    );
    Quill::from_tree(FileTreeNode::Directory { files }).expect("quill_from_yaml: from_tree failed")
}

/// The render date core's tests compile with.
#[cfg(test)]
pub(crate) fn test_date() -> CalendarDate {
    CalendarDate::new(2026, 3, 14).expect("a calendar day")
}

/// The canonical content a `text` cell carries for `s`.
#[cfg(test)]
pub(crate) fn text(s: &str) -> serde_json::Value {
    quillmark_content::serial::to_canonical_value(&quillmark_content::import::from_plaintext(s))
}

/// A field schema parsed from `yaml`.
#[cfg(test)]
pub(crate) fn field(yaml: &str) -> FieldSchema {
    let value = crate::value::QuillValue::from_yaml_str(yaml).unwrap();
    FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
}

/// An empty document pinned to `memo@1.0.0`.
#[cfg(test)]
pub(crate) fn blank_doc() -> crate::document::Document {
    use std::str::FromStr;
    crate::document::Document::new(crate::version::QuillReference::from_str("memo@1.0.0").unwrap())
}

/// Whether any of `diags` carries `code`.
#[cfg(test)]
pub(crate) fn has_code(diags: &[crate::error::Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code.as_deref() == Some(code))
}
