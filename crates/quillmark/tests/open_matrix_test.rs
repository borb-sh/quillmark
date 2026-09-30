//! An open matrix's added items through a real Typst compile: the plate reads
//! them after the roster, and a region claimed at an added item's address both
//! compiles against that render's address tables and surfaces as a region.

#![cfg(feature = "typst")]

use quillmark::Quillmark;
use quillmark_fixtures::quills_path;

mod common;

#[test]
fn an_added_items_title_surfaces_a_region_at_its_own_address() {
    let quill = quillmark::quill_from_path(quills_path("roster_form")).expect("roster_form loads");
    let example = quill
        .example_document()
        .expect("roster_form carries an example")
        .expect("the example reads")
        .document;
    let session = Quillmark::new()
        .open(&quill, &example, common::test_date())
        .expect("the example opens");
    let fields: Vec<String> = session.regions().into_iter().map(|r| r.field).collect();

    for field in ["qualifications.wing_ig.title", "qualifications.aide_de_camp.title"] {
        assert!(fields.iter().any(|f| f == field), "{field} in {fields:?}");
    }
}
