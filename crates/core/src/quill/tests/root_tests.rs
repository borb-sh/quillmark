//! The `schema:` block, a derived quill's root, and the contract both are
//! compared by.

use super::*;

const ROOT: &str = r#"quill:
  name: resume
  version: "1.2.0"
  backend: typst
  description: A resume
schema:
  main:
    fields:
      name: { type: text }
      summary: { type: richtext, shape: lines, title: Summary }
      level: { type: enum, values: [junior, senior] }
  experience:
    title: Experience
    fields:
      employer: { type: text }
      years: { type: integer }
"#;

fn derived(sections: &str) -> String {
    format!(
        "quill:\n  name: resume_modern\n  version: \"0.1.0\"\n  backend: typst\n  \
         description: A modern resume\n  root: resume@1.2.0\n{sections}"
    )
}

fn resolve(sections: &str, root: Option<&str>) -> Result<QuillConfig, Vec<Diagnostic>> {
    QuillConfig::from_yaml_with_root(&derived(sections), root).map(|(config, _)| config)
}

fn refusal(sections: &str, root: Option<&str>) -> Vec<String> {
    let errors = resolve(sections, root).expect_err("the load is refused");
    codes(&errors).into_iter().map(str::to_string).collect()
}

fn class(delta: &SchemaDelta, kind: &str, path: &str) -> DeltaClass {
    delta
        .entries
        .iter()
        .find(|e| e.kind == kind && e.path == path)
        .unwrap_or_else(|| panic!("the delta holds {kind} {path}: {delta:#?}"))
        .class
}

#[test]
fn a_schema_block_is_the_sections_it_spells() {
    let block = config_with_sections(
        "schema:\n  main:\n    fields:\n      name: { type: text }\n  job:\n    fields:\n      \
         employer: { type: text }\n",
    )
    .unwrap();
    let sections = config_with_sections(
        "main:\n  fields:\n    name: { type: text }\ncard_kinds:\n  job:\n    fields:\n      \
         employer: { type: text }\n",
    )
    .unwrap();
    assert_eq!(block, sections);

    let both = config_with_sections("schema:\n  main: {}\nmain: {}\n").unwrap_err();
    assert_eq!(codes(&both), ["quill::schema_and_sections"]);
}

#[test]
fn a_derived_quill_resolves_over_its_root() {
    let root = QuillConfig::from_yaml_with_warnings(ROOT).unwrap().0;
    let quill = resolve(
        "schema:\n  main:\n    fields:\n      summary: { title: Profile, default: Hello }\n      \
         accent: { type: text }\n  experience:\n    description: Where you worked\n    fields:\n      \
         logo: { type: text }\n  project:\n    fields:\n      title: { type: text }\n",
        Some(ROOT),
    )
    .unwrap();

    assert_eq!(
        quill.main.fields.keys().collect::<Vec<_>>(),
        ["name", "summary", "level", "accent"]
    );
    let summary = &quill.main.fields["summary"];
    assert_eq!(summary.title.as_deref(), Some("Profile"));
    assert_eq!(summary.r#type, root.main.fields["summary"].r#type);
    assert!(summary.default.is_some());
    assert_eq!(
        quill
            .card_kinds
            .iter()
            .map(|k| k.name.as_str())
            .collect::<Vec<_>>(),
        ["experience", "project"]
    );
    let experience = &quill.card_kinds[0];
    assert_eq!(experience.title, root.card_kinds[0].title);
    assert_eq!(experience.description.as_deref(), Some("Where you worked"));
    assert_eq!(
        experience.fields.keys().collect::<Vec<_>>(),
        ["employer", "years", "logo"]
    );

    assert_eq!(
        quill.root,
        Some(QuillRoot {
            name: root.name.clone(),
            version: root.version.clone(),
            fingerprint: root.contract_fingerprint(),
        })
    );

    let onto_derived = quill.delta(&root);
    assert!(onto_derived.is_lossless());
    assert_eq!(class(&onto_derived, "main", "summary"), DeltaClass::Same);
    assert_eq!(class(&onto_derived, "main", "accent"), DeltaClass::New);
    assert_eq!(class(&onto_derived, "project", ""), DeltaClass::New);

    let onto_root = root.delta(&quill);
    assert!(!onto_root.is_lossless());
    assert_eq!(
        onto_root
            .changes()
            .map(|e| (e.kind.as_str(), e.path.as_str(), e.class))
            .collect::<Vec<_>>(),
        [
            ("main", "accent", DeltaClass::Stranded),
            ("experience", "logo", DeltaClass::Stranded),
            ("project", "", DeltaClass::Stranded),
        ]
    );
}

#[test]
fn a_derived_quill_with_no_schema_shares_its_roots_contract() {
    let root = QuillConfig::from_yaml_with_warnings(ROOT).unwrap().0;
    let quill = resolve("", Some(ROOT)).unwrap();
    assert_eq!(quill.contract_fingerprint(), root.contract_fingerprint());
    assert!(quill.delta(&root).identical);
}

#[test]
fn a_root_is_refused_where_it_cannot_be_the_base() {
    assert_eq!(refusal("", None), ["quill::root_missing"]);
    assert_eq!(
        refusal("", Some(&ROOT.replace("1.2.0", "1.2.1"))),
        ["quill::root_mismatch"]
    );
    assert_eq!(
        refusal(
            "",
            Some(&ROOT.replace("backend: typst", "backend: typst\n  root: base@1.0.0"))
        ),
        ["quill::root_chain"]
    );
    assert_eq!(
        refusal("", Some(&ROOT.replace("type: integer", "type: intger"))),
        ["quill::root_invalid"]
    );
    let ranged = derived("").replace("resume@1.2.0", "resume@1.2");
    let errors = QuillConfig::from_yaml_with_root(&ranged, Some(ROOT)).unwrap_err();
    assert_eq!(codes(&errors), ["quill::invalid_root"]);
}

#[test]
fn a_derived_quill_declares_only_what_its_root_leaves_open() {
    let main = |field: &str| format!("schema:\n  main:\n    fields:\n      {field}\n");
    assert_eq!(
        refusal(&main("name: { type: richtext }"), Some(ROOT)),
        ["quill::root_field_collision"]
    );
    assert_eq!(
        refusal(&main("nickname: { title: Nickname }"), Some(ROOT)),
        ["quill::root_unknown_field"]
    );
    assert_eq!(
        refusal(&main("summary: { shape: blocks }"), Some(ROOT)),
        ["quill::root_contract_key"]
    );
    assert_eq!(
        refusal(
            &main("level: { values: [junior, senior, staff] }"),
            Some(ROOT)
        ),
        ["quill::root_contract_key"]
    );
    assert_eq!(
        refusal(
            "schema:\n  experience:\n    body:\n      enabled: false\n",
            Some(ROOT)
        ),
        ["quill::root_contract_key"]
    );
}

#[test]
fn the_fingerprint_reads_the_contract_and_nothing_else() {
    let of = |sections: &str| {
        config_with_sections(sections)
            .unwrap()
            .contract_fingerprint()
    };
    let base = of(
        "schema:\n  main:\n    fields:\n      name: { type: text }\n      level: { type: \
                   enum, values: [junior, senior] }\n",
    );
    let restyled = of(
        "schema:\n  main:\n    title: Person\n    fields:\n      level: { type: \
                       enum, values: [senior, junior], default: senior }\n      name: { type: \
                       text, title: Full name, max: 80 }\n",
    );
    let reshaped = of(
        "schema:\n  main:\n    fields:\n      name: { type: text, shape: lines }\n      \
                       level: { type: enum, values: [junior, senior] }\n",
    );
    assert_eq!(base, restyled);
    assert_ne!(base, reshaped);
}

#[test]
fn a_delta_classes_each_slot_by_what_the_target_admits() {
    let source = config_with_sections(
        r#"schema:
  main:
    fields:
      count: { type: integer }
      note: { type: richtext, shape: blocks }
      level: { type: enum, values: [junior, senior] }
      when: { type: date }
      jobs:
        type: array
        items:
          type: object
          properties:
            employer: { type: text }
            city: { type: text }
  aside:
    fields:
      text: { type: text }
"#,
    )
    .unwrap();
    let target = config_with_sections(
        r#"schema:
  main:
    body:
      enabled: false
    fields:
      count: { type: number }
      note: { type: richtext, shape: line }
      level: { type: enum, values: [junior, senior, staff] }
      when: { type: text }
      jobs:
        type: array
        items:
          type: object
          properties:
            employer: { type: text, shape: lines }
            country: { type: text }
"#,
    )
    .unwrap();

    let delta = target.delta(&source);
    assert_eq!(
        delta
            .entries
            .iter()
            .map(|e| (e.kind.as_str(), e.path.as_str(), e.class))
            .collect::<Vec<_>>(),
        [
            ("main", "count", DeltaClass::Widened),
            ("main", "note", DeltaClass::Narrowed),
            ("main", "level", DeltaClass::Widened),
            ("main", "when", DeltaClass::Retyped),
            ("main", "jobs", DeltaClass::Same),
            ("main", "jobs[]", DeltaClass::Same),
            ("main", "jobs[].employer", DeltaClass::Widened),
            ("main", "jobs[].country", DeltaClass::New),
            ("main", "jobs[].city", DeltaClass::Stranded),
            ("main", BODY_PATH, DeltaClass::Stranded),
            ("aside", "", DeltaClass::Stranded),
        ]
    );
    assert!(!delta.identical);
    assert!(source.delta(&source).identical);
}

#[test]
fn a_tree_carries_its_roots_quill_yaml() {
    let own = derived("schema:\n  main:\n    fields:\n      accent: { type: text }\n");
    let quill = quill_from(&[
        ("Quill.yaml", own.as_bytes()),
        (ROOT_COPY_PATH, ROOT.as_bytes()),
    ]);
    assert!(quill.config().main.fields.contains_key("name"));
    assert!(quill.config().main.fields.contains_key("accent"));

    let errors = Quill::from_tree(tree(&[("Quill.yaml", own.as_bytes())])).unwrap_err();
    assert_eq!(codes(&errors), ["quill::root_missing"]);
}
