//! # quillmark-docx: the Typst-free Word writer
//!
//! A typed Word document ([`model::Document`]) and its writer, [`write_docx`],
//! which serializes it to a `.docx` package. The model makes the structural
//! rules of WordprocessingML hold by its types: every section, cell, header,
//! footer and text box ends in a paragraph, and each property struct is
//! written in schema order. The bytes are a function of the model alone: the
//! zip carries fixed timestamps and a fixed part order, and every id is
//! counted in writing order.
//!
//! [`layout::LayoutDoc`] is the engine-free laid-out document a backend's
//! reader fills, and [`lower::lower`] turns it into the model, with notices
//! for what the Word file carries less of.
//!
//! This crate depends on neither Typst nor `quillmark-core`.

pub mod layout;
pub mod lower;
pub mod model;
mod write;

pub use write::write_docx;

/// Reads a package [`write_docx`] wrote back into its parts, or `None`.
#[doc(hidden)]
pub fn unzip(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    write::zip::unzip(bytes)
}
