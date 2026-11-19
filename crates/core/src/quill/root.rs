//! The `schema:` block and a derived quill's root.
//!
//! Both are resolved on the parsed `Quill.yaml` value, ahead of the loader:
//! [`lift_schema`] spells a `schema:` block as the `main:` and `card_kinds:`
//! sections the loader reads, and [`resolve`] lays a derived quill's own
//! declarations over its root's. The loader then validates the resolved
//! schema as it validates any other.

use std::str::FromStr;

use serde::Serialize;
use serde_json::{Map, Value};

use super::QuillConfig;
use crate::error::{Diagnostic, Severity};
use crate::version::{QuillReference, VersionSelector};

/// Where a derived quill's tree holds its root's `Quill.yaml`, byte for byte.
pub const ROOT_COPY_PATH: &str = "root/Quill.yaml";

/// The keys a derived quill may set on a field its root declares: the ones no
/// stored document observes.
const REFINABLE_FIELD_KEYS: &[&str] = &["title", "description", "default", "example", "ui", "max"];

/// The keys a derived quill may set on a kind its root declares, beside
/// `fields` and `body`.
const REFINABLE_KIND_KEYS: &[&str] = &["title", "description", "ui", "seed"];

/// The keys a derived quill may set on the body of a kind its root declares.
const REFINABLE_BODY_KEYS: &[&str] = &["description", "example"];

/// The root a derived quill takes the base of its schema from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuillRoot {
    pub name: String,
    pub version: String,
    /// The root's own [contract fingerprint](QuillConfig::contract_fingerprint).
    pub fingerprint: String,
}

fn error(code: &str, message: String) -> Diagnostic {
    Diagnostic::new(Severity::Error, message).with_code(code.to_string())
}

/// The `name` and exact `version` a `Quill.yaml`'s `quill.root` names, for a
/// loader deciding which root to copy in. `None` where it names none, or names
/// one the load refuses.
pub fn root_reference(quill_yaml: &str) -> Option<(String, String)> {
    let value: Value = crate::value::parse_yaml(quill_yaml).ok()?;
    let reference = value.get("quill")?.get("root")?.as_str()?;
    parse_reference(reference).ok()
}

fn parse_reference(reference: &str) -> Result<(String, String), String> {
    let parsed = QuillReference::from_str(reference)?;
    match (parsed.collection, parsed.selector) {
        (None, VersionSelector::Exact(version)) => Ok((parsed.name, version.to_string())),
        _ => Err("a root is one exact version, `name@MAJOR.MINOR.PATCH`".to_string()),
    }
}

/// Spell a top-level `schema:` block as the `main:` and `card_kinds:` sections
/// the loader reads: `schema.main` is `main`, and every other key a card kind.
pub(super) fn lift_schema(value: &mut Value, errors: &mut Vec<Diagnostic>) {
    let Some(top) = value.as_object_mut() else {
        return;
    };
    let Some(schema) = top.remove("schema") else {
        return;
    };
    if top.contains_key("main") || top.contains_key("card_kinds") {
        errors.push(
            error(
                "quill::schema_and_sections",
                "Quill.yaml declares both a `schema:` block and a top-level `main:` or \
                 `card_kinds:` section"
                    .to_string(),
            )
            .with_hint("Move `main` and each card kind under `schema:`, as peers.".to_string()),
        );
        return;
    }
    match schema {
        Value::Null => {}
        Value::Object(kinds) => {
            let mut card_kinds = Map::new();
            for (name, kind) in kinds {
                if name == "main" {
                    top.insert(name, kind);
                } else {
                    card_kinds.insert(name, kind);
                }
            }
            if !card_kinds.is_empty() {
                top.insert("card_kinds".to_string(), Value::Object(card_kinds));
            }
        }
        _ => errors.push(
            error(
                "quill::invalid_schema_section",
                "'schema' section must be an object (mapping of kind names to schemas)".to_string(),
            )
            .with_hint("Declare `main` and each card kind as a key under `schema:`.".to_string()),
        ),
    }
}

/// Lay the declarations of a derived quill (`value`, already lifted) over its
/// root's, leaving `value` holding the resolved schema. `None` where `value`
/// names no root, or the root is refused.
pub(super) fn resolve(
    value: &mut Value,
    root_yaml: Option<&str>,
    errors: &mut Vec<Diagnostic>,
) -> Option<QuillRoot> {
    let reference = value.get("quill")?.get("root")?.clone();
    let Some(reference) = reference.as_str() else {
        errors.push(error(
            "quill::invalid_root",
            "Invalid 'root' in 'quill' section: expected `name@MAJOR.MINOR.PATCH` text".to_string(),
        ));
        return None;
    };
    let (name, version) = match parse_reference(reference) {
        Ok(parsed) => parsed,
        Err(reason) => {
            errors.push(
                error(
                    "quill::invalid_root",
                    format!("Invalid 'root' value '{reference}' in 'quill' section: {reason}"),
                )
                .with_hint("Name one quill version, such as 'resume@1.2.0'.".to_string()),
            );
            return None;
        }
    };

    let Some(root_yaml) = root_yaml else {
        errors.push(
            error(
                "quill::root_missing",
                format!(
                    "The quill names root '{reference}' and its tree holds no copy of it at \
                     '{ROOT_COPY_PATH}'"
                ),
            )
            .with_hint(format!(
                "Copy the root's Quill.yaml to '{ROOT_COPY_PATH}'; loading from a quiver's \
                 source layout and packing do it."
            )),
        );
        return None;
    };

    let refused = |reason: String| {
        error(
            "quill::root_invalid",
            format!("The copy of root '{reference}' at '{ROOT_COPY_PATH}' is refused: {reason}"),
        )
    };
    let mut root: Value = match crate::value::parse_yaml(root_yaml) {
        Ok(root) => root,
        Err(e) => {
            errors.push(refused(e.to_string()));
            return None;
        }
    };
    if root.get("quill").and_then(|q| q.get("root")).is_some() {
        errors.push(
            error(
                "quill::root_chain",
                format!("Root '{reference}' names a root of its own"),
            )
            .with_hint("A root is a bundled quill; derive from its root instead.".to_string()),
        );
        return None;
    }
    let held = |key: &str| {
        root.get("quill")
            .and_then(|q| q.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    if (held("name"), held("version")) != (name.clone(), version.clone()) {
        errors.push(error(
            "quill::root_mismatch",
            format!(
                "The quill names root '{reference}', and the copy at '{ROOT_COPY_PATH}' is \
                 '{}@{}'",
                held("name"),
                held("version")
            ),
        ));
        return None;
    }

    // The root loads as the quill it is, which checks its `requires` floor and
    // every schema rule, and gives the contract its fingerprint is taken from.
    let root_config = match QuillConfig::from_yaml_with_warnings(root_yaml) {
        Ok((config, _)) => config,
        Err(root_errors) => {
            errors.extend(root_errors.into_iter().map(|d| {
                let mut refused = refused(d.message.clone());
                refused.hint = d.hint;
                refused
            }));
            return None;
        }
    };

    let mut lift_errors = Vec::new();
    lift_schema(&mut root, &mut lift_errors);
    let own_main = value.as_object_mut().and_then(|top| top.remove("main"));
    let own_kinds = value
        .as_object_mut()
        .and_then(|top| top.remove("card_kinds"));

    let main = merge_kind("main", root.get("main").cloned(), own_main, errors);
    let mut kinds = match root.get("card_kinds") {
        Some(Value::Object(kinds)) => kinds.clone(),
        _ => Map::new(),
    };
    match own_kinds {
        None | Some(Value::Null) => {}
        Some(Value::Object(own)) => {
            for (kind, declared) in own {
                let merged = match kinds.get(&kind) {
                    Some(root_kind) => {
                        merge_kind(&kind, Some(root_kind.clone()), Some(declared), errors)
                    }
                    None => declared,
                };
                kinds.insert(kind, merged);
            }
        }
        // The loader reports the shape.
        Some(other) => {
            kinds = Map::new();
            if let Some(top) = value.as_object_mut() {
                top.insert("card_kinds".to_string(), other);
            }
        }
    }

    let top = value.as_object_mut()?;
    top.insert("main".to_string(), main);
    if !kinds.is_empty() {
        top.insert("card_kinds".to_string(), Value::Object(kinds));
    }

    Some(QuillRoot {
        name,
        version,
        fingerprint: root_config.contract_fingerprint(),
    })
}

/// One kind of the resolved schema: the root's declaration with the derived
/// quill's own fields after it and its refinements laid over.
fn merge_kind(
    kind: &str,
    root: Option<Value>,
    own: Option<Value>,
    errors: &mut Vec<Diagnostic>,
) -> Value {
    let mut resolved = match root {
        Some(Value::Object(root)) => root,
        _ => Map::new(),
    };
    let own = match own {
        None | Some(Value::Null) => return Value::Object(resolved),
        Some(Value::Object(own)) => own,
        // The loader reports the shape.
        Some(other) => return other,
    };

    for (key, declared) in own {
        match key.as_str() {
            "fields" => {
                let mut fields = match resolved.remove("fields") {
                    Some(Value::Object(fields)) => fields,
                    _ => Map::new(),
                };
                if let Value::Object(own_fields) = declared {
                    for (name, field) in own_fields {
                        merge_field(kind, name, field, &mut fields, errors);
                    }
                }
                resolved.insert(key, Value::Object(fields));
            }
            "body" => {
                let mut body = match resolved.remove("body") {
                    Some(Value::Object(body)) => body,
                    _ => Map::new(),
                };
                if let Value::Object(own_body) = declared {
                    for (body_key, body_value) in own_body {
                        if REFINABLE_BODY_KEYS.contains(&body_key.as_str()) {
                            body.insert(body_key, body_value);
                        } else {
                            errors.push(contract_key(kind, &format!("body.{body_key}")));
                        }
                    }
                }
                resolved.insert(key, Value::Object(body));
            }
            key if REFINABLE_KIND_KEYS.contains(&key) => {
                resolved.insert(key.to_string(), declared);
            }
            other => errors.push(contract_key(kind, other)),
        }
    }
    Value::Object(resolved)
}

fn merge_field(
    kind: &str,
    name: String,
    declared: Value,
    fields: &mut Map<String, Value>,
    errors: &mut Vec<Diagnostic>,
) {
    let own = declared.get("type").is_some();
    match (own, fields.get_mut(&name)) {
        (true, Some(_)) => errors.push(
            error(
                "quill::root_field_collision",
                format!("Field '{name}' on '{kind}' declares a type, and the root declares it too"),
            )
            .with_hint(
                "Rename the field, or drop `type` and the root's other keys to refine the root's \
                 field."
                    .to_string(),
            ),
        ),
        (true, None) => {
            fields.insert(name, declared);
        }
        (false, None) => errors.push(
            error(
                "quill::root_unknown_field",
                format!("Field '{name}' on '{kind}' declares no type, and the root declares no such field"),
            )
            .with_hint("Add `type` to declare a field of the quill's own.".to_string()),
        ),
        (false, Some(Value::Object(root_field))) => {
            let Value::Object(refinement) = declared else {
                return;
            };
            for (key, value) in refinement {
                if REFINABLE_FIELD_KEYS.contains(&key.as_str()) {
                    root_field.insert(key, value);
                } else {
                    errors.push(contract_key(kind, &format!("{name}.{key}")));
                }
            }
        }
        (false, Some(_)) => {}
    }
}

fn contract_key(kind: &str, key: &str) -> Diagnostic {
    error(
        "quill::root_contract_key",
        format!("'{key}' on '{kind}' is the root's to declare"),
    )
    .with_hint(format!(
        "A derived quill sets only what no stored document observes. On a field: {}. On a kind: \
         {}, and a body's {}.",
        REFINABLE_FIELD_KEYS.join(", "),
        REFINABLE_KIND_KEYS.join(", "),
        REFINABLE_BODY_KEYS.join(", "),
    ))
}
