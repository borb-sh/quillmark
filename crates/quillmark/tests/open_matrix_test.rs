//! A matrix through a real Typst compile: `roster` hands the plate every member,
//! held or not, then the items a document adds, and each row's address compiles
//! against the address tables and surfaces as a region.

#![cfg(feature = "typst")]

use quillmark::Quillmark;
use quillmark_fixtures::quills_path;

mod common;

#[test]
fn every_printed_row_surfaces_a_region_at_its_own_address() {
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

    for field in [
        "qualifications.sq_cc_candidate",
        "qualifications.flight_cc.detail",
        "qualifications.wing_ig",
        "qualifications.wing_ig.title",
        "qualifications.aide_de_camp.title",
    ] {
        assert!(fields.iter().any(|f| f == field), "{field} in {fields:?}");
    }
}
