//! The line form: the content model as one line record per `\n`-separated
//! segment, with container paths, ordinals and run parity spelled on every
//! line. A v1 stored value decodes from it; the markdown codec and the
//! renderers read the model, the export through [`crate::view`].

pub mod island;
pub mod model;
pub mod serial;
pub mod traverse;
