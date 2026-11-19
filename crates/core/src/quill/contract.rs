//! A schema's contract: what a stored document can observe of it.
//!
//! The contract is each kind's field names and whether it takes a body, and
//! each field's type with the keys that decide which values it admits:
//! `shape`, `values`, `variants`, `items`, `properties`, `members` and `open`.
//! Labels, descriptions, `ui`, order, `example`, `seed`, `default`, `max` and
//! the `?` of an optional type are outside it, so two quills that differ only
//! there read every document alike.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{CardSchema, FieldSchema, FieldType, QuillConfig};

const MAIN: &str = "main";

/// The path of a kind's body in a [`SchemaDelta`].
pub const BODY_PATH: &str = "$body";

/// How a slot of one schema reads what a document wrote under another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeltaClass {
    /// Both schemas declare the slot alike.
    Same,
    /// The target admits every value the source does, and more.
    Widened,
    /// The target refuses some value the source admits.
    Narrowed,
    /// The target reads the slot as another type.
    Retyped,
    /// The source declares the slot and the target does not: its value stays
    /// in the document, unclaimed.
    Stranded,
    /// The target declares the slot and the source does not.
    New,
}

impl DeltaClass {
    /// Whether a document's value in the slot reads under the target as it
    /// read under the source.
    pub fn is_lossless(self) -> bool {
        matches!(self, Self::Same | Self::Widened | Self::New)
    }
}

/// One slot of a [`SchemaDelta`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaEntry {
    /// `main`, or a card kind's name.
    pub kind: String,
    /// The slot under the kind: a field (`skills`), a cell under it
    /// (`contact.email`, `skills[].name`, `status:employed.employer`),
    /// [`BODY_PATH`], or empty for the kind itself.
    pub path: String,
    pub class: DeltaClass,
}

/// What moving a document from one schema to another does to each slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaDelta {
    /// Whether the two contracts are one, which is every entry being
    /// [`DeltaClass::Same`].
    pub identical: bool,
    /// Every slot either schema declares: `main` first, then the target's
    /// kinds in its order, then the kinds only the source declares.
    pub entries: Vec<DeltaEntry>,
}

impl SchemaDelta {
    /// Whether every value a document holds under the source reads the same
    /// under the target.
    pub fn is_lossless(&self) -> bool {
        self.entries.iter().all(|e| e.class.is_lossless())
    }

    /// The entries that are not [`DeltaClass::Same`].
    pub fn changes(&self) -> impl Iterator<Item = &DeltaEntry> {
        self.entries.iter().filter(|e| e.class != DeltaClass::Same)
    }
}

impl QuillConfig {
    /// The schema's contract in canonical form: keys sorted, an enum's values
    /// and a matrix's members as sorted sets.
    pub fn contract(&self) -> Value {
        let kinds: BTreeMap<&str, Value> = self
            .card_kinds
            .iter()
            .map(|kind| (kind.name.as_str(), kind_contract(kind)))
            .collect();
        json!({ "kinds": kinds, "main": kind_contract(&self.main) })
    }

    /// A digest of [`Self::contract`], lowercase hex: equal for two schemas
    /// exactly when every document reads alike under both.
    pub fn contract_fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"quillmark-contract-v1\n");
        hasher.update(self.contract().to_string().as_bytes());
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// What a document written under `source` meets under this schema.
    pub fn delta(&self, source: &QuillConfig) -> SchemaDelta {
        let mut entries = Vec::new();
        kind_delta(MAIN, Some(&source.main), Some(&self.main), &mut entries);
        let source_kind = |name: &str| source.card_kinds.iter().find(|k| k.name == name);
        for kind in &self.card_kinds {
            kind_delta(
                &kind.name,
                source_kind(&kind.name),
                Some(kind),
                &mut entries,
            );
        }
        for kind in &source.card_kinds {
            if !self.card_kinds.iter().any(|k| k.name == kind.name) {
                kind_delta(&kind.name, Some(kind), None, &mut entries);
            }
        }
        SchemaDelta {
            identical: entries.iter().all(|e| e.class == DeltaClass::Same),
            entries,
        }
    }
}

fn kind_contract(kind: &CardSchema) -> Value {
    json!({ "body": kind.body_enabled(), "fields": fields_contract(kind.fields.iter().map(|(k, v)| (k, v))) })
}

fn fields_contract<'a>(fields: impl Iterator<Item = (&'a String, &'a FieldSchema)>) -> Value {
    let sorted: BTreeMap<&str, Value> = fields
        .map(|(name, field)| (name.as_str(), field_contract(field)))
        .collect();
    json!(sorted)
}

fn boxed<'a>(
    fields: &'a IndexMap<String, Box<FieldSchema>>,
) -> impl Iterator<Item = (&'a String, &'a FieldSchema)> {
    fields.iter().map(|(name, field)| (name, field.as_ref()))
}

fn field_contract(field: &FieldSchema) -> Value {
    let mut contract: BTreeMap<&str, Value> = BTreeMap::new();
    contract.insert("type", json!(field.r#type.as_str()));
    match &field.r#type {
        FieldType::Text { shape } | FieldType::RichText { shape } => {
            contract.insert("shape", json!(shape.as_str()));
        }
        FieldType::Enum { values } => {
            contract.insert("values", json!(values.iter().collect::<BTreeSet<_>>()));
        }
        FieldType::Matrix { roster, open } => {
            contract.insert("members", json!(roster.keys().collect::<BTreeSet<_>>()));
            contract.insert("open", json!(open));
        }
        _ => {}
    }
    if let Some(variants) = &field.variants {
        let sets: BTreeMap<&str, Value> = variants
            .iter()
            .map(|(member, set)| (member.as_str(), fields_contract(boxed(set))))
            .collect();
        contract.insert("variants", json!(sets));
    }
    if let Some(items) = &field.items {
        contract.insert("items", field_contract(items));
    }
    if let Some(properties) = &field.properties {
        contract.insert("properties", fields_contract(boxed(properties)));
    }
    json!(contract)
}

fn kind_delta(
    kind: &str,
    source: Option<&CardSchema>,
    target: Option<&CardSchema>,
    entries: &mut Vec<DeltaEntry>,
) {
    let mut push = |path: String, class: DeltaClass| {
        entries.push(DeltaEntry {
            kind: kind.to_string(),
            path,
            class,
        })
    };
    let (source, target) = match (source, target) {
        (Some(source), Some(target)) => (source, target),
        (Some(_), None) => return push(String::new(), DeltaClass::Stranded),
        (None, Some(_)) => return push(String::new(), DeltaClass::New),
        (None, None) => return,
    };
    let mut slots = Vec::new();
    fields_delta(
        "",
        &source
            .fields
            .iter()
            .map(|(k, v)| (k, v))
            .collect::<Vec<_>>(),
        &target
            .fields
            .iter()
            .map(|(k, v)| (k, v))
            .collect::<Vec<_>>(),
        &mut slots,
    );
    slots.push((
        BODY_PATH.to_string(),
        match (source.body_enabled(), target.body_enabled()) {
            (true, false) => DeltaClass::Stranded,
            (false, true) => DeltaClass::New,
            _ => DeltaClass::Same,
        },
    ));
    for (path, class) in slots {
        push(path, class);
    }
}

type Slots = Vec<(String, DeltaClass)>;
type Fields<'a> = [(&'a String, &'a FieldSchema)];

fn find<'a>(fields: &Fields<'a>, name: &str) -> Option<&'a FieldSchema> {
    fields
        .iter()
        .find(|(candidate, _)| candidate.as_str() == name)
        .map(|(_, field)| *field)
}

fn listed(fields: &Option<IndexMap<String, Box<FieldSchema>>>) -> Vec<(&String, &FieldSchema)> {
    fields.iter().flat_map(boxed).collect()
}

/// The slots of two field sets under `prefix`: the target's in its order, then
/// the ones only the source declares.
fn fields_delta(prefix: &str, source: &Fields, target: &Fields, slots: &mut Slots) {
    for (name, field) in target {
        let path = format!("{prefix}{name}");
        match find(source, name.as_str()) {
            Some(written) => field_delta(&path, written, field, slots),
            None => slots.push((path, DeltaClass::New)),
        }
    }
    for (name, _) in source {
        if find(target, name.as_str()).is_none() {
            slots.push((format!("{prefix}{name}"), DeltaClass::Stranded));
        }
    }
}

fn sets<T: Ord>(source: &BTreeSet<T>, target: &BTreeSet<T>) -> DeltaClass {
    if source == target {
        DeltaClass::Same
    } else if target.is_superset(source) {
        DeltaClass::Widened
    } else {
        DeltaClass::Narrowed
    }
}

fn field_delta(path: &str, source: &FieldSchema, target: &FieldSchema, slots: &mut Slots) {
    let own = match (&source.r#type, &target.r#type) {
        (FieldType::Integer, FieldType::Number) => DeltaClass::Widened,
        (FieldType::Number, FieldType::Integer) => DeltaClass::Narrowed,
        (FieldType::Text { shape: written }, FieldType::Text { shape })
        | (FieldType::RichText { shape: written }, FieldType::RichText { shape }) => {
            match shape.cmp(written) {
                std::cmp::Ordering::Equal => DeltaClass::Same,
                std::cmp::Ordering::Greater => DeltaClass::Widened,
                std::cmp::Ordering::Less => DeltaClass::Narrowed,
            }
        }
        (FieldType::Enum { values: written }, FieldType::Enum { values }) => {
            if source.is_variant_bearing() != target.is_variant_bearing() {
                DeltaClass::Retyped
            } else {
                sets(
                    &written.iter().collect::<BTreeSet<_>>(),
                    &values.iter().collect::<BTreeSet<_>>(),
                )
            }
        }
        (
            FieldType::Matrix {
                roster: written,
                open: was_open,
            },
            FieldType::Matrix { roster, open },
        ) => {
            let members = sets(
                &written.keys().collect::<BTreeSet<_>>(),
                &roster.keys().collect::<BTreeSet<_>>(),
            );
            let added = match (was_open, open) {
                (true, false) => DeltaClass::Narrowed,
                (false, true) => DeltaClass::Widened,
                _ => DeltaClass::Same,
            };
            members.max(added)
        }
        (written, read) if written.as_str() == read.as_str() => DeltaClass::Same,
        _ => DeltaClass::Retyped,
    };
    slots.push((path.to_string(), own));
    if own == DeltaClass::Retyped {
        return;
    }

    if let (Some(written), Some(read)) = (&source.items, &target.items) {
        field_delta(&format!("{path}[]"), written, read, slots);
    }
    if source.properties.is_some() || target.properties.is_some() {
        fields_delta(
            &format!("{path}."),
            &listed(&source.properties),
            &listed(&target.properties),
            slots,
        );
    }
    if let (Some(written), Some(read)) = (&source.variants, &target.variants) {
        for (member, set) in read {
            let prefix = format!("{path}:{member}.");
            let written_set: Vec<_> = written.get(member).into_iter().flat_map(boxed).collect();
            fields_delta(
                &prefix,
                &written_set,
                &boxed(set).collect::<Vec<_>>(),
                slots,
            );
        }
        for (member, set) in written {
            if !read.contains_key(member) {
                let prefix = format!("{path}:{member}.");
                fields_delta(&prefix, &boxed(set).collect::<Vec<_>>(), &[], slots);
            }
        }
    }
}
