//! Property-based fuzz tests for Quillmark, built on `proptest`.

#[cfg(test)]
mod coerce_fuzz;

#[cfg(test)]
mod conform_fuzz;

#[cfg(test)]
mod decode_fuzz;

#[cfg(test)]
mod convert_fuzz;

#[cfg(test)]
mod emit_roundtrip_fuzz;

#[cfg(test)]
mod pdf_fuzz;
