//! The line form: the content model as one line record per `\n`-separated
//! segment, with container paths, ordinals and run parity spelled on every
//! line. It is the intermediate the markdown codecs and the render lowering
//! read and write, and the shape a v1 stored value decodes from.

pub mod anchor;
pub mod carrier;
pub mod delta;
pub mod export;
mod html;
mod html_table;
pub mod import;
pub mod island;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod serial;
pub mod traverse;
