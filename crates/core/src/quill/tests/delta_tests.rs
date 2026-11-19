//! `Quill::compare`: what a document written for one quill meets under another.

use crate::quill::{DeltaClass, QuillConfig, SchemaDelta};

use super::{config_with_sections, main_section};

const FROM: &str = r#"schema:
  main:
    fields:
      name: { type: text }
      summary: { type: text, shape: lines }
      level: { type: enum, values: [junior, senior], default: junior }
      years: { type: integer }
      started: { type: date }
      nickname: { type: text }
      contact:
        type: object
        properties:
          email: { type: text }
          fax: { type: text }
      skills:
        type: array
        items:
          type: object
          properties:
            name: { type: text }
            rating: { type: number }
      status:
        type: enum
        values: [employed, student]
        variants:
          employed:
            employer: { type: text }
            since: { type: date }
          student:
            school: { type: text }
      quals:
        type: matrix
        members: { cpr: CPR, pilot: Pilot }
        properties:
          expires: { type: date }
  job:
    fields:
      employer: { type: text }
  reference:
    body: { enabled: false }
    fields:
      who: { type: text }
"#;

const TO: &str = r#"schema:
  main:
    body: { enabled: false }
    fields:
      summary: { type: richtext, shape: lines }
      name: { type: text, title: Full name, description: As printed, max: 40 }
      level: { type: enum, values: [senior, junior, principal], default: senior }
      years: { type: number }
      started: { type: date? }
      accent: { type: text }
      contact:
        type: object
        properties:
          email: { type: text, shape: lines }
          phone: { type: text }
      skills:
        type: array
        items:
          type: object
          properties:
            name: { type: text }
            rating: { type: integer }
      status:
        type: enum
        values: [employed, student]
        variants:
          employed:
            employer: { type: text }
          student:
            school: { type: text }
            since: { type: date }
      quals:
        type: matrix
        members: { cpr: CPR }
        properties:
          expires: { type: date }
  job:
    fields:
      employer: { type: text }
      logo: { type: text }
  award:
    fields:
      title: { type: text }
"#;

fn delta(from: &QuillConfig, to: &QuillConfig) -> SchemaDelta {
    SchemaDelta::between(from, to)
}

fn listed(delta: &SchemaDelta) -> Vec<(&str, &str, DeltaClass)> {
    delta
        .entries
        .iter()
        .map(|e| (e.kind.as_str(), e.path.as_deref().unwrap_or(""), e.class))
        .collect()
}

#[test]
fn each_slot_is_classed_by_what_a_stored_value_meets() {
    use DeltaClass::*;
    let from = config_with_sections(FROM).unwrap();
    let to = config_with_sections(TO).unwrap();

    assert_eq!(
        listed(&delta(&from, &to)),
        [
            ("main", "summary", Retyped),
            ("main", "name", Same),
            ("main", "level", Widened),
            ("main", "years", Widened),
            ("main", "started", Same),
            ("main", "accent", Added),
            ("main", "contact", Same),
            ("main", "contact.email", Widened),
            ("main", "contact.phone", Added),
            ("main", "contact.fax", Unclaimed),
            ("main", "skills", Same),
            ("main", "skills.0", Same),
            ("main", "skills.0.name", Same),
            ("main", "skills.0.rating", Narrowed),
            ("main", "status", Same),
            ("main", "status.employer", Same),
            ("main", "status.school", Same),
            ("main", "status.since", Narrowed),
            ("main", "quals", Narrowed),
            ("main", "nickname", Unclaimed),
            ("main", "$body", Unclaimed),
            ("job", "employer", Same),
            ("job", "logo", Added),
            ("job", "$body", Same),
            ("award", "", Added),
            ("reference", "", Unclaimed),
        ]
    );
}

#[test]
fn a_move_reverses_class_by_class() {
    use DeltaClass::*;
    let from = config_with_sections(FROM).unwrap();
    let to = config_with_sections(TO).unwrap();
    let reverse = |class| match class {
        Widened => Narrowed,
        Narrowed => Widened,
        Added => Unclaimed,
        Unclaimed => Added,
        class => class,
    };
    let key =
        |(kind, path, class): (&str, &str, DeltaClass)| (kind.to_string(), path.to_string(), class);

    let mut there: Vec<_> = listed(&delta(&from, &to))
        .into_iter()
        .map(|(kind, path, class)| key((kind, path, reverse(class))))
        // The cell moved worlds, which strands a stored value either way.
        .filter(|(_, path, _)| path != "status.since")
        .collect();
    let mut back: Vec<_> = listed(&delta(&to, &from))
        .into_iter()
        .map(key)
        .filter(|(_, path, _)| path != "status.since")
        .collect();
    there.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    back.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    assert_eq!(there, back);
}

#[test]
fn every_path_is_an_address_of_the_quill_that_declares_it() {
    let from = config_with_sections(FROM).unwrap();
    let to = config_with_sections(TO).unwrap();

    for entry in &delta(&from, &to).entries {
        let Some(path) = entry
            .path
            .as_deref()
            .filter(|p| *p != crate::quill::BODY_PATH)
        else {
            continue;
        };
        let address = match entry.kind.as_str() {
            "main" => path.to_string(),
            kind => format!("$cards.{kind}.0.{path}"),
        };
        let declaring = if entry.class == DeltaClass::Unclaimed {
            &from
        } else {
            &to
        };
        assert!(declaring.field_at(&address).is_ok(), "{address}");
    }
}

#[test]
fn only_what_a_document_observes_and_a_default_count_as_a_change() {
    let from = config_with_sections(FROM).unwrap();
    assert!(delta(&from, &from).changes().next().is_none());

    let relabelled = config_with_sections(
        &FROM
            .replace("name: { type: text }\n    summary", "name: { type: text?, title: Name, description: d, example: Ada, max: 9, ui: { compact: true } }\n    summary")
            .replace("values: [junior, senior]", "values: [senior, junior]"),
    )
    .unwrap();
    assert_ne!(relabelled, from);
    assert!(delta(&from, &relabelled).changes().next().is_none());

    let redefaulted =
        config_with_sections(&FROM.replace("default: junior", "default: senior")).unwrap();
    let moved = delta(&from, &redefaulted);
    assert!(moved.carries());
    assert_eq!(
        moved
            .changes()
            .map(|e| (e.path.as_deref(), e.class, e.default_changed))
            .collect::<Vec<_>>(),
        [(Some("level"), DeltaClass::Same, true)]
    );
}

#[test]
fn a_matrix_is_one_slot_classed_by_its_members_and_columns() {
    use DeltaClass::*;
    let matrix = |members: &str, rest: &str| {
        config_with_sections(&main_section(&format!(
            "  fields:\n    quals:\n      type: matrix\n      members: {members}\n{rest}"
        )))
        .unwrap()
    };
    let class = |from: &QuillConfig, to: &QuillConfig| {
        let moved = delta(from, to);
        assert_eq!(moved.entries.len(), 2, "{moved:?}");
        moved.entries[0].class
    };
    let base = matrix("{ cpr: CPR }", "");
    let more = matrix("{ cpr: CPR, pilot: Pilot }", "");
    let open = matrix("{ cpr: CPR }", "      open: true\n");
    let dated = matrix(
        "{ cpr: CPR }",
        "      properties:\n        expires: { type: date }\n",
    );
    let timed = matrix(
        "{ cpr: CPR }",
        "      properties:\n        expires: { type: datetime }\n",
    );

    assert_eq!(class(&base, &more), Widened);
    assert_eq!(class(&more, &base), Narrowed);
    assert_eq!(class(&base, &open), Widened);
    assert_eq!(class(&open, &more), Narrowed);
    assert_eq!(class(&base, &dated), Widened);
    assert_eq!(class(&dated, &base), Narrowed);
    assert_eq!(class(&dated, &timed), Retyped);
}
