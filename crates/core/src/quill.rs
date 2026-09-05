//! The `Quill` type: portable, validated quill data.

mod blueprint;
pub(crate) mod compose;
mod config;
pub(crate) mod conform;
mod resolved;
pub(crate) mod values;
mod fill;
mod formats;
mod ignore;
mod load;
mod schema;
mod schema_yaml;
pub(crate) mod support;
mod seed;
mod tree;
mod types;
pub(crate) mod validation;

pub use config::{CoercionError, QuillConfig};
pub(crate) use config::Leniency;
pub use conform::BoundParseError;
pub use resolved::{FieldSource, Resolved, ResolvedCard, ResolvedField, ResolvedMain};
pub use values::{CardValues, DocumentValues};
pub(crate) use values::{card_fields, card_values, project_field};
pub(crate) use resolved::resolve_document;
pub use fill::blank;
pub use formats::{parse_date, parse_datetime};
pub use ignore::QuillIgnore;
pub use schema::{build_transform_schema, CONTENT_MEDIA_TYPE, QUILLMARK_INLINE_KEY};
pub use support::UNSUPPORTED_CONSTRUCT;
pub use tree::FileTreeNode;
pub use validation::ValidationError;
pub use types::{
    BlockConstruct, BodyCardSchema, CardSchema, FieldSchema, FieldType, GroupRegistry, GroupSchema,
    UiCardSchema, UiFieldSchema, VariantFields, VARIANT_DISCRIMINANT_KEY,
};

use std::collections::HashMap;

use crate::value::QuillValue;

/// The quill-config keys every binding surfaces as typed, top-level fields.
/// Bindings exclude these from the unstructured-metadata passthrough, so a
/// typed field is never emitted twice.
pub const STANDARD_METADATA_KEYS: &[&str] =
    &["name", "backend", "description", "version", "author"];

/// Portable, validated quill data: the file bundle, parsed config, and
/// metadata of an authored quill, tagged with its *declared* backend id.
///
/// A `Quill` holds no backend and needs no engine to construct or use: every
/// method here is a pure read of its parsed config. Rendering is the engine's
/// job. Construct with [`Quill::from_tree`] (pure) or
/// `quillmark::quill_from_path` (filesystem stays out of core).
#[derive(Clone)]
pub struct Quill {
    pub(crate) metadata: HashMap<String, QuillValue>,
    pub(crate) config: QuillConfig,
    pub(crate) files: FileTreeNode,
}

impl Quill {
    /// The quill's declared name.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// The backend identifier declared in Quill.yaml (e.g. `"typst"`).
    pub fn backend_id(&self) -> &str {
        &self.config.backend
    }

    /// Quill-specific metadata parsed from Quill.yaml.
    pub fn metadata(&self) -> &HashMap<String, QuillValue> {
        &self.metadata
    }

    /// The parsed schema configuration.
    pub fn config(&self) -> &QuillConfig {
        &self.config
    }

    /// A schema-bound [`TypedWriter`](crate::TypedWriter) over `doc`: the front
    /// door for typed field writes.
    pub fn writer<'a>(&'a self, doc: &'a mut crate::document::Document) -> crate::TypedWriter<'a> {
        crate::TypedWriter::new(&self.config, doc)
    }

    /// A schema-bound [`TypedReader`](crate::TypedReader) over `doc`: the read
    /// twin of [`writer`](Self::writer). Interprets each field by its declared
    /// type, and reads an undeclared name as the typo it is rather than as
    /// absent.
    pub fn reader<'a>(&'a self, doc: &'a crate::document::Document) -> crate::TypedReader<'a> {
        crate::TypedReader::new(&self.config, doc)
    }

    /// The in-memory file tree for this quill.
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
