//! The line form: the content model as one line record per `\n`-separated
//! segment, with container paths, ordinals and run parity spelled on every
//! line. The markdown import walks into it, the anchor contexts and a revise's
//! carry read it, and a v1 stored value decodes from it; the markdown export
//! and the renderers read the model through [`crate::view`].

pub mod anchor;
pub mod carrier;
pub mod delta;
pub mod export;
mod html;
pub(crate) mod html_table;
pub mod import;
pub mod island;
pub mod model;
pub mod normalize;
pub mod serial;
pub mod traverse;
