//! What moving a document from one quill to another does to each slot it can
//! fill.
//!
//! The comparison reads what a stored document can observe of a schema: each
//! kind's field names and whether it takes a body, and each field's type with
//! `shape`, `values`, `variants`, `items`, `properties`, `members` and `open`.
//! Labels, descriptions, `ui`, order, `example`, `seed`, `max` and the `?` of an
//! optional type are outside it. A `default` is outside it too, and is reported
//! beside the class, because it is what an unanswered slot prints.

use std::collections::BTreeSet;

use indexmap::IndexMap;
use serde::Serialize;

use super::{CardSchema, FieldSchema, FieldType, Quill, QuillConfig};

const MAIN: &str = "main";

/// The [`DeltaEntry::path`] of a kind's body.
pub const BODY_PATH: &str = "$body";

/// How a slot of the quill a document moves to reads what the document wrote
/// under the quill it moves from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeltaClass {
    /// Both quills declare the slot alike.
    Same,
    /// The target admits every value the source does, and more.
    Widened,
    /// The target refuses some value the source admits: a stored value stays
    /// as written and may fail validation.
    Narrowed,
    /// The target reads the slot as another type, so
    /// [`Quill::conform`](super::Quill::conform) rewrites a stored value.
    Retyped,
    /// Only the source declares the slot: a stored value stays in the document
    /// as unclaimed input, which renders nowhere and warns.
    Unclaimed,
    /// Only the target declares the slot.
    Added,
}

impl DeltaClass {
    /// Whether a value stored in the slot reads under the target as it read
    /// under the source.
    pub fn carries(self) -> bool {
        matches!(self, Self::Same | Self::Widened | Self::Added)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Same => 0,
            Self::Widened | Self::Added => 1,
            Self::Narrowed | Self::Unclaimed => 2,
            Self::Retyped => 3,
        }
    }

    /// The class of a slot whose parts class as `self` and `other`.
    fn worst(self, other: Self) -> Self {
        let folded = |class: Self| match class {
            Self::Added => Self::Widened,
            Self::Unclaimed => Self::Narrowed,
            class => class,
        };
        if other.rank() > self.rank() {
            folded(other)
        } else {
            folded(self)
        }
    }
}

/// One slot of a [`SchemaDelta`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeltaEntry {
    /// `main`, or a card kind's name.
    pub kind: String,
    /// The slot under the kind, as the schema address
    /// [`QuillConfig::field_at`] resolves under a card: a field (`skills`), a
    /// slot below it (`contact.email`, `skills.0.name`, `status.employer`), or
    /// [`BODY_PATH`]. `None` for a kind only one quill declares.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub class: DeltaClass,
    /// Whether both quills declare the slot and give it different `default:`s,
    /// so a document that leaves it unanswered prints differently.
    pub default_changed: bool,
}

/// What moving a document from one quill to another does to each slot.
///
/// An entry classed [`Retyped`](DeltaClass::Retyped),
/// [`Unclaimed`](DeltaClass::Unclaimed) or [`Added`](DeltaClass::Added) stands
/// for every slot beneath it, which get no entry of their own. A matrix is one
/// slot: its members and columns class the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaDelta {
    /// `main` first, then the target's kinds in its order, then the kinds only
    /// the source declares. Within a kind, the target's slots in its order,
    /// then the ones only the source declares, then the body.
    pub entries: Vec<DeltaEntry>,
}

impl SchemaDelta {
    /// Whether every value a document stores under the source reads the same
    /// under the target, so retargeting `$quill` is the whole move.
    pub fn carries(&self) -> bool {
        self.entries.iter().all(|e| e.class.carries())
    }

    /// The entries a move changes something at: every class but
    /// [`Same`](DeltaClass::Same), and every changed default.
    pub fn changes(&self) -> impl Iterator<Item = &DeltaEntry> {
        self.entries
            .iter()
            .filter(|e| e.class != DeltaClass::Same || e.default_changed)
    }

    pub(crate) fn between(from: &QuillConfig, to: &QuillConfig) -> Self {
        let mut entries = Vec::new();
        kind_delta(MAIN, Some(&from.main), Some(&to.main), &mut entries);
        for kind in &to.card_kinds {
            kind_delta(
                &kind.name,
                from.card_kind(&kind.name),
                Some(kind),
                &mut entries,
            );
        }
        for kind in &from.card_kinds {
            if to.card_kind(&kind.name).is_none() {
                kind_delta(&kind.name, Some(kind), None, &mut entries);
            }
        }
        Self { entries }
    }
}

impl Quill {
    /// What a document written for `from` meets when its `$quill` is retargeted
    /// to `to`: another quill, or another version of the same one. Reads the
    /// two schemas and no document.
    pub fn compare(from: &Quill, to: &Quill) -> SchemaDelta {
        SchemaDelta::between(from.config(), to.config())
    }
}

type Slots = Vec<(String, DeltaClass, bool)>;
type Fields<'a> = Vec<(&'a str, &'a FieldSchema)>;

fn top(fields: &IndexMap<String, FieldSchema>) -> Fields<'_> {
    fields.iter().map(|(k, v)| (k.as_str(), v)).collect()
}

fn boxed(fields: &IndexMap<String, Box<FieldSchema>>) -> Fields<'_> {
    fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_ref()))
        .collect()
}

fn find<'a>(fields: &Fields<'a>, name: &str) -> Option<&'a FieldSchema> {
    fields
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, field)| *field)
}

fn kind_delta(
    kind: &str,
    from: Option<&CardSchema>,
    to: Option<&CardSchema>,
    entries: &mut Vec<DeltaEntry>,
) {
    let mut push = |path: Option<String>, class: DeltaClass, default_changed: bool| {
        entries.push(DeltaEntry {
            kind: kind.to_string(),
            path,
            class,
            default_changed,
        })
    };
    let (from, to) = match (from, to) {
        (Some(from), Some(to)) => (from, to),
        (Some(_), None) => return push(None, DeltaClass::Unclaimed, false),
        (None, Some(_)) => return push(None, DeltaClass::Added, false),
        (None, None) => return,
    };
    let mut slots = Slots::new();
    fields_delta("", &top(&from.fields), &top(&to.fields), &mut slots);
    slots.push((
        BODY_PATH.to_string(),
        match (from.body_enabled(), to.body_enabled()) {
            (true, false) => DeltaClass::Unclaimed,
            (false, true) => DeltaClass::Added,
            _ => DeltaClass::Same,
        },
        false,
    ));
    for (path, class, default_changed) in slots {
        push(Some(path), class, default_changed);
    }
}

fn fields_delta(prefix: &str, from: &Fields, to: &Fields, slots: &mut Slots) {
    for (name, field) in to {
        let path = format!("{prefix}{name}");
        match find(from, name) {
            Some(written) => field_delta(&path, written, field, DeltaClass::Same, slots),
            None => slots.push((path, DeltaClass::Added, false)),
        }
    }
    for (name, _) in from {
        if find(to, name).is_none() {
            slots.push((format!("{prefix}{name}"), DeltaClass::Unclaimed, false));
        }
    }
}

fn sets<T: Ord>(from: &BTreeSet<T>, to: &BTreeSet<T>) -> DeltaClass {
    if from == to {
        DeltaClass::Same
    } else if to.is_superset(from) {
        DeltaClass::Widened
    } else {
        DeltaClass::Narrowed
    }
}

/// The slots of a field both quills declare. `at_least` is a class the field
/// holds whatever its own declaration says.
fn field_delta(
    path: &str,
    from: &FieldSchema,
    to: &FieldSchema,
    at_least: DeltaClass,
    slots: &mut Slots,
) {
    let own = match (&from.r#type, &to.r#type) {
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
            if from.is_variant_bearing() != to.is_variant_bearing() {
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
            let added_items = match (was_open, open) {
                (true, false) => DeltaClass::Narrowed,
                (false, true) => DeltaClass::Widened,
                _ => DeltaClass::Same,
            };
            let mut columns = Slots::new();
            fields_delta(
                "",
                &boxed(from.matrix_columns()),
                &boxed(to.matrix_columns()),
                &mut columns,
            );
            columns
                .into_iter()
                .fold(members.worst(added_items), |class, (_, column, _)| {
                    class.worst(column)
                })
        }
        (written, read) if written.as_str() == read.as_str() => DeltaClass::Same,
        _ => DeltaClass::Retyped,
    };
    let own = own.worst(at_least);
    slots.push((path.to_string(), own, from.default != to.default));
    if own == DeltaClass::Retyped {
        return;
    }

    if let (Some(written), Some(read)) = (&from.items, &to.items) {
        field_delta(&format!("{path}.0"), written, read, DeltaClass::Same, slots);
    }
    if let (Some(written), Some(read)) = (from.namespace_props(), to.namespace_props()) {
        fields_delta(&format!("{path}."), &boxed(written), &boxed(read), slots);
    }
    if let (Some(written), Some(read)) = (&from.variants, &to.variants) {
        variants_delta(path, written, read, slots);
    }
}

type Variants = IndexMap<String, super::VariantFields>;

fn cells(variants: &Variants) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for name in variants.values().flat_map(|set| set.keys()) {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    names
}

fn cell<'a>(variants: &'a Variants, name: &str) -> Option<&'a FieldSchema> {
    variants
        .values()
        .find_map(|set| set.get(name))
        .map(Box::as_ref)
}

fn worlds<'a>(variants: &'a Variants, name: &str) -> BTreeSet<&'a str> {
    variants
        .iter()
        .filter(|(_, set)| set.contains_key(name))
        .map(|(member, _)| member.as_str())
        .collect()
}

/// A variant container's cells, each one field whichever worlds hold it. A
/// cell that a world of the source holds and the same world of the target does
/// not is at best narrowed, since a value stored in that world is stranded.
fn variants_delta(path: &str, from: &Variants, to: &Variants, slots: &mut Slots) {
    for name in cells(to) {
        let cell_path = format!("{path}.{name}");
        match (cell(from, name), cell(to, name)) {
            (Some(written), Some(read)) => {
                let at_least = if worlds(to, name).is_superset(&worlds(from, name)) {
                    DeltaClass::Same
                } else {
                    DeltaClass::Narrowed
                };
                field_delta(&cell_path, written, read, at_least, slots);
            }
            _ => slots.push((cell_path, DeltaClass::Added, false)),
        }
    }
    for name in cells(from) {
        if cell(to, name).is_none() {
            slots.push((format!("{path}.{name}"), DeltaClass::Unclaimed, false));
        }
    }
}
