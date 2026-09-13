//! One schema address grammar, two implementations: acroform's `bind` walks the
//! `QuillConfig`, and the Typst helper's `_qm-known-path` walks the address tree
//! `SchemaMeta` derives from the transform schema. `PLATE_DATA.md` promises a
//! plate author that one address binds on either backend, and `GRAMMAR` is what
//! holds the two to it.
//!
//! A `$body` address is outside that table: a body is content rather than a
//! bindable field, so acroform's resolver roots none.

#![cfg(all(feature = "typst", feature = "acroform"))]

use quillmark::{Backend, FileTreeNode, Quill};
use quillmark_typst::TypstBackend;
use std::collections::HashMap;

/// Every position the schema admits, containers nested inside containers and a
/// variant cell holding a typed table among them, declared on `main` and again
/// on a card kind so each address has its card twin.
const YAML: &str = r#"
quill:
  name: address_grammar
  version: 0.1.0
  backend: typst
  description: every position the nesting contract admits
typst:
  plate_file: plate.typ
main:
  fields:
    subject:
      type: string
    tags:
      type: array
      items:
        type: string
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
          num: { type: string }
          lead:
            type: object
            properties:
              email: { type: string }
    address:
      type: object
      properties:
        city: { type: string }
        street: { type: string }
        geo:
          type: object
          properties:
            lat: { type: number }
        lines:
          type: array
          items:
            type: string
    grid:
      type: array
      items:
        type: array
        items:
          type: integer
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI]
      default: ""
      variants:
        CUI:
          poc: { type: string }
          history:
            type: array
            items:
              type: object
              properties:
                when: { type: string }
card_kinds:
  endorsement:
    fields:
      from:
        type: string
      tags:
        type: array
        items:
          type: string
      refs:
        type: array
        items:
          type: object
          properties:
            org: { type: string }
            num: { type: string }
      origin:
        type: object
        properties:
          office: { type: string }
          city: { type: string }
          geo:
            type: object
            properties:
              lat: { type: number }
      level:
        type: enum
        values: [FIRST, SECOND]
        default: ""
        variants:
          SECOND:
            endorser: { type: string }
"#;

/// Every address both backends must resolve, and every one both must refuse.
///
/// An index is not bounds-checked: `refs.9` resolves against a document carrying
/// one ref, because the grammar asks about the schema alone.
const GRAMMAR: &[(&str, bool)] = &[
    ("subject", true),
    ("tags", true),
    ("tags.0", true),
    ("refs", true),
    ("refs.0", true),
    ("refs.9", true),
    ("refs.0.org", true),
    ("refs.0.lead", true),
    ("refs.0.lead.email", true),
    ("address", true),
    ("address.city", true),
    ("address.geo.lat", true),
    ("address.lines", true),
    ("address.lines.0", true),
    ("grid.0", true),
    ("grid.0.0", true),
    ("classification", true),
    ("classification.value", true),
    ("classification.poc", true),
    ("classification.history", true),
    ("classification.history.0", true),
    ("classification.history.0.when", true),
    ("$cards.endorsement.0.from", true),
    ("$cards.endorsement.9.from", true),
    ("$cards.endorsement.0.tags", true),
    ("$cards.endorsement.0.tags.0", true),
    ("$cards.endorsement.0.refs", true),
    ("$cards.endorsement.0.refs.0", true),
    ("$cards.endorsement.0.refs.0.org", true),
    ("$cards.endorsement.0.origin", true),
    ("$cards.endorsement.0.origin.office", true),
    ("$cards.endorsement.0.origin.geo.lat", true),
    ("$cards.endorsement.0.level", true),
    ("$cards.endorsement.0.level.value", true),
    ("$cards.endorsement.0.level.endorser", true),
    ("nonesuch", false),
    ("subject.0", false),
    ("subject.poc", false),
    ("tags.0.org", false),
    ("refs.org", false),
    ("refs.0.undeclared", false),
    ("address.9", false),
    ("address.zip", false),
    ("address.city.0", false),
    ("address.geo.lon", false),
    ("address.geo.lat.0", false),
    ("address.lines.0.0", false),
    ("refs.0.lead.phone", false),
    ("refs.0.lead.email.0", false),
    ("grid.0.0.0", false),
    ("grid.0.x", false),
    ("classification.0", false),
    ("classification.undeclared", false),
    ("classification.value.oops", false),
    ("classification.history.0.nosuch", false),
    ("classification.history.x", false),
    ("$cards", false),
    ("$cards.endorsement", false),
    ("$cards.endorsement.0", false),
    ("$cards.0.from", false),
    ("$cards.nosuch.0.from", false),
    ("$cards.endorsement.x.from", false),
    ("$cards.endorsement.0.nosuch", false),
    ("$cards.endorsement.0.from.0", false),
    ("$cards.endorsement.0.tags.0.org", false),
    ("$cards.endorsement.0.origin.office.0", false),
    ("$cards.endorsement.0.origin.geo.lon", false),
];

const PREAMBLE: &str = r#"#import "@local/quillmark-helper:0.1.0": field-region
#set page(width: 400pt, height: 1200pt, margin: 20pt)
"#;

fn quill(plate: &str) -> Quill {
    let mut files = HashMap::new();
    for (name, contents) in [("Quill.yaml", YAML), ("plate.typ", plate)] {
        files.insert(
            name.to_string(),
            FileTreeNode::File {
                contents: contents.as_bytes().to_vec(),
            },
        );
    }
    Quill::from_tree(FileTreeNode::Directory { files }).expect("load quill")
}

fn data() -> serde_json::Value {
    serde_json::json!({
        "subject": "Widgets",
        "tags": ["urgent"],
        "refs": [{ "org": "AFRL/RQ", "num": "2026-01", "lead": { "email": "lead@example.mil" } }],
        "address": {
            "city": "Dayton",
            "street": "1864 Fourth St",
            "geo": { "lat": 39.78 },
            "lines": ["Bldg 15", "Rm 200"],
        },
        "grid": [[1, 2], [3, 4]],
        "classification": {
            "value": "CUI",
            "poc": "Capt J. Smith",
            "history": [{ "when": "2026-01-02" }],
        },
        "$cards": [{
            "$kind": "endorsement",
            "from": "SAF/AA",
            "tags": ["routine"],
            "refs": [{ "org": "AFRL/RQ", "num": "2026-02" }],
            "origin": { "office": "SAF/AA", "city": "Dayton", "geo": { "lat": 38.88 } },
            "level": { "value": "SECOND", "endorser": "Col K. Lee" },
        }],
    })
}

fn claims(addresses: impl IntoIterator<Item = &'static str>) -> String {
    addresses
        .into_iter()
        .fold(PREAMBLE.to_string(), |mut plate, address| {
            plate.push_str(&format!("#field-region(\"{address}\")[x]\n"));
            plate
        })
}

fn compile(plate: &str) -> Result<quillmark::LiveSession, String> {
    TypstBackend
        .open(&quill(plate), &data())
        .map_err(|e| format!("{e}"))
}

#[test]
fn acroform_resolves_exactly_the_shared_grammar() {
    let quill = quill("");
    for (address, resolves) in GRAMMAR {
        assert_eq!(
            quillmark_acroform::resolves_schema_address(quill.config(), address),
            *resolves,
            "{address:?}"
        );
    }
}

#[test]
fn typst_admits_every_address_the_grammar_resolves() {
    let accepted = GRAMMAR.iter().filter(|(_, ok)| *ok).map(|(a, _)| *a);
    if let Err(err) = compile(&claims(accepted)) {
        panic!("every resolving address must compile: {err}");
    }
}

#[test]
fn typst_refuses_every_address_the_grammar_refuses() {
    for (address, _) in GRAMMAR.iter().filter(|(_, ok)| !*ok) {
        let Err(err) = compile(&claims([*address])) else {
            panic!("{address:?} must not compile");
        };
        assert!(err.contains("not a schema field address"), "{address:?}: {err}");
    }
}

#[test]
fn a_body_address_is_typst_only() {
    let bodies = ["$body", "$cards.endorsement.0.$body"];
    let quill = quill("");
    for address in bodies {
        assert!(
            !quillmark_acroform::resolves_schema_address(quill.config(), address),
            "{address:?}"
        );
    }
    if let Err(err) = compile(&claims(bodies)) {
        panic!("a plate claims a body address: {err}");
    }
}
