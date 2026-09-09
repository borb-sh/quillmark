//! Unified payload representation.
//!
//! A [`Payload`] carries a card-yaml block's whole YAML content in source order
//! as [`PayloadItem`] variants: typed `$` system metadata, user fields, and
//! comments. One source-ordered list is the canonical storage, so a comment
//! adjacent to a `$` line round-trips through the same mechanism as one
//! adjacent to a user field.
//!
//! Comments inside a structured value live on the [`Payload`] itself, at paths
//! whose head segment names the entry that owns them — the form prescan
//! produces and the storage DTO stores, so neither end converts.
//!
//! The map-keyed accessors filter to [`PayloadItem::Field`]; `$` entries have
//! dedicated typed accessors.

use indexmap::IndexMap;
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::prescan::NestedComment;
use crate::value::{PathSegment, QuillValue};
use crate::version::QuillReference;

/// Which out-of-band system-metadata map a [`PayloadItem::Meta`] carries.
///
/// `$ext` and `$seed` are the same shape: an opaque `Map<String, Value>` that
/// never reaches the plate JSON but round-trips through Markdown and the storage
/// DTO. They differ in canonical sort rank, root-only-ness, and whether the
/// seeding layer interprets them ([`crate::SeedOverlay::from_json`] reads
/// `$seed`; `$ext` stays opaque).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaKey {
    /// `$ext`: opaque out-of-band consumer state (editor renames, agent
    /// annotations). Allowed on any card.
    Ext,
    /// `$seed`: per-card-kind seed overlays. **Root-only** (like `$quill`).
    Seed,
}

impl MetaKey {
    /// The literal source key (`"$ext"` / `"$seed"`).
    pub fn as_str(self) -> &'static str {
        match self {
            MetaKey::Ext => "$ext",
            MetaKey::Seed => "$seed",
        }
    }

    /// Parse the source key (`"$ext"` / `"$seed"`), or `None` for any other key.
    pub fn from_key_str(key: &str) -> Option<Self> {
        match key {
            "$ext" => Some(MetaKey::Ext),
            "$seed" => Some(MetaKey::Seed),
            _ => None,
        }
    }

    /// Canonical sort rank among typed `$` entries (after `$kind`).
    fn rank(self) -> u8 {
        match self {
            MetaKey::Ext => 2,
            MetaKey::Seed => 3,
        }
    }
}

/// One entry in a [`Payload`]: a typed `$` system metadata entry, a user field,
/// or a comment line.
///
/// The live in-memory model, deliberately **not** `Serialize`/`Deserialize`:
/// storage goes through the versioned DTOs in `document::dto`.
#[derive(Debug, Clone, PartialEq)]
pub enum PayloadItem {
    /// `$quill` system metadata, holding the parsed quill reference.
    Quill { reference: QuillReference },
    /// `$kind` system metadata: the card's kind name.
    Kind { value: String },
    /// `$ext` / `$seed` system metadata: an opaque mapping discriminated by
    /// [`MetaKey`], never emitted into the plate JSON.
    Meta {
        key: MetaKey,
        value: JsonMap<String, JsonValue>,
    },
    /// A user-defined YAML field, optionally tagged `!must_fill`.
    Field {
        key: String,
        value: QuillValue,
        /// `true` when the field was written as `key: !must_fill <value>` or
        /// `key: !must_fill` in source.
        fill: bool,
    },
    /// A YAML comment. Text excludes the leading `#` and one optional space.
    ///
    /// An `inline` comment (`field: value # text`) attaches to the item that
    /// immediately precedes it; an orphan degrades to an own-line comment.
    Comment { text: String, inline: bool },
}

impl PayloadItem {
    /// Shorthand for a plain (non-fill) field entry.
    #[cfg(test)]
    pub(crate) fn field(key: impl Into<String>, value: QuillValue) -> Self {
        PayloadItem::Field {
            key: key.into(),
            value,
            fill: false,
        }
    }

    /// The key a [`Payload`]'s nested comments address this entry by: a field's
    /// own key, or the literal `$ext` / `$seed`. `None` for the entries that
    /// carry no structured value to nest inside.
    fn nested_owner_key(&self) -> Option<&str> {
        match self {
            PayloadItem::Field { key, .. } => Some(key),
            PayloadItem::Meta { key, .. } => Some(key.as_str()),
            _ => None,
        }
    }

    pub(crate) fn comment(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: false,
        }
    }

    pub(crate) fn comment_inline(text: impl Into<String>) -> Self {
        PayloadItem::Comment {
            text: text.into(),
            inline: true,
        }
    }

    /// Canonical sort rank for typed `$` entries: `$quill` < `$kind` < `$ext` <
    /// `$seed`. `None` for user fields and comments, which keep source order.
    fn meta_rank(&self) -> Option<u8> {
        match self {
            PayloadItem::Quill { .. } => Some(0),
            PayloadItem::Kind { .. } => Some(1),
            PayloadItem::Meta { key, .. } => Some(key.rank()),
            _ => None,
        }
    }
}

/// Ordered, comment-preserving payload of a card-yaml block: a **read view**
/// onto card-yaml storage, holding `$` entries, user fields, and comments
/// interleaved in source order.
///
/// Mutation is crate-internal. The invariants an edit must hold — at most one
/// `$quill` / `$kind` / `$ext` / `$seed`, no duplicate field keys, every field
/// name matching `[A-Za-z_][A-Za-z0-9_]*` — are not all expressible in the
/// mutators' signatures, so out-of-crate authoring goes through the verbs that
/// enforce them: `Card::store_field` / `store_ext` / `store_seed_overlay`,
/// `Document::set_quill_ref`, and [`TypedWriter`](crate::TypedWriter).
#[derive(Debug, Clone, PartialEq)]
pub struct Payload {
    items: Vec<PayloadItem>,
    nested_comments: Vec<NestedComment>,
}

impl Payload {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            nested_comments: Vec::new(),
        }
    }

    /// No `$` entries, no comments, no fill markers.
    pub(crate) fn from_index_map(map: IndexMap<String, QuillValue>) -> Self {
        let items = map
            .into_iter()
            .map(|(key, value)| PayloadItem::Field {
                key,
                value,
                fill: false,
            })
            .collect();
        Self::from_items(items)
    }

    pub(crate) fn from_items(items: Vec<PayloadItem>) -> Self {
        Self {
            items,
            nested_comments: Vec::new(),
        }
    }

    /// `nested_comments` addresses its owners by the head path segment, which
    /// is how prescan produces them and how the storage DTO stores them. An
    /// entry matching no item is inert: every reader filters by owner.
    pub(crate) fn from_items_with_nested(
        items: Vec<PayloadItem>,
        nested_comments: Vec<NestedComment>,
    ) -> Self {
        Self {
            items,
            nested_comments,
        }
    }

    /// Comments inside this payload's structured values, at paths whose head
    /// segment names the owning entry: a field's key, or the literal `$ext` /
    /// `$seed`. One list for the whole payload, which is how prescan produces
    /// them and how the storage DTO stores them.
    pub fn nested_comments(&self) -> &[NestedComment] {
        &self.nested_comments
    }

    /// The comments owned by entry `key`, rebased onto that entry's own value.
    pub(crate) fn nested_comments_for(&self, key: &str) -> Vec<NestedComment> {
        self.nested_comments
            .iter()
            .filter_map(|nc| {
                let (PathSegment::Key(head), rest) = nc.container_path.split_first()? else {
                    return None;
                };
                (head == key).then(|| NestedComment {
                    container_path: rest.to_vec(),
                    position: nc.position,
                    text: nc.text.clone(),
                    inline: nc.inline,
                })
            })
            .collect()
    }

    /// Drop the comments nested inside entry `key`. Every path that replaces or
    /// removes an entry runs it: the new value need not carry the positions the
    /// old one's comments sat at.
    fn prune_nested(&mut self, key: &str) {
        self.nested_comments.retain(|nc| {
            !matches!(nc.container_path.first(), Some(PathSegment::Key(k)) if k == key)
        });
    }

    /// Ordered iterator over raw items (`$` entries, fields, comments).
    pub fn items(&self) -> &[PayloadItem] {
        &self.items
    }

    /// Rename user field `from` to `to`, carrying the comments nested inside it.
    /// No-op when no such field exists; the caller owns `to` being a well-formed
    /// name not already present.
    pub(crate) fn rename_field(&mut self, from: &str, to: String) {
        let Some(slot) = self.items.iter_mut().find_map(|i| match i {
            PayloadItem::Field { key, .. } if key == from => Some(key),
            _ => None,
        }) else {
            return;
        };
        *slot = to.clone();
        for nc in &mut self.nested_comments {
            if matches!(nc.container_path.first(), Some(PathSegment::Key(k)) if k == from) {
                nc.container_path[0] = PathSegment::Key(to.clone());
            }
        }
    }

    /// Remove and return the first item matching `pred`, with the comments
    /// nested inside it.
    fn take_item(&mut self, pred: impl Fn(&PayloadItem) -> bool) -> Option<PayloadItem> {
        let pos = self.items.iter().position(pred)?;
        let item = self.items.remove(pos);
        if let Some(key) = item.nested_owner_key() {
            let key = key.to_string();
            self.prune_nested(&key);
        }
        Some(item)
    }

    /// The `$quill` reference, if declared.
    pub fn quill(&self) -> Option<&QuillReference> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Quill { reference } => Some(reference),
            _ => None,
        })
    }

    pub fn kind(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Kind { value } => Some(value.as_str()),
            _ => None,
        })
    }

    pub(crate) fn meta(&self, want: MetaKey) -> Option<&JsonMap<String, JsonValue>> {
        self.items.iter().find_map(|i| match i {
            PayloadItem::Meta { key, value, .. } if *key == want => Some(value),
            _ => None,
        })
    }

    /// Opaque: never interpreted, never emitted into the plate JSON.
    pub fn ext(&self) -> Option<&JsonMap<String, JsonValue>> {
        self.meta(MetaKey::Ext)
    }

    /// The raw `$seed` map, keyed by card-kind. Never reaches the plate JSON;
    /// index it by kind and pass the entry to [`crate::SeedOverlay::from_json`]
    /// for a parsed overlay.
    pub fn seed(&self) -> Option<&JsonMap<String, JsonValue>> {
        self.meta(MetaKey::Seed)
    }

    /// Set or replace the `$quill` entry, at its canonical position (before any
    /// `$kind` / `$ext` / `$seed`). Comments are untouched.
    pub(crate) fn set_quill(&mut self, reference: QuillReference) {
        self.upsert_meta(PayloadItem::Quill { reference });
    }

    /// Set or replace the `$kind` entry. Same insertion rules as
    /// [`set_quill`](Self::set_quill).
    pub(crate) fn set_kind(&mut self, kind: impl Into<String>) {
        self.upsert_meta(PayloadItem::Kind { value: kind.into() });
    }

    /// Set or replace an out-of-band meta entry at its canonical position.
    /// Nested comments on a replaced entry are dropped (the new value may not
    /// contain matching positions).
    pub(crate) fn set_meta(&mut self, key: MetaKey, value: JsonMap<String, JsonValue>) {
        self.prune_nested(key.as_str());
        self.upsert_meta(PayloadItem::Meta { key, value });
    }

    /// Set or replace the `$ext` entry, after `$quill` / `$kind` and before any
    /// user field. Nested comments on a replaced entry are dropped.
    pub(crate) fn set_ext(&mut self, value: JsonMap<String, JsonValue>) {
        self.set_meta(MetaKey::Ext, value);
    }

    /// Set or replace the `$seed` entry, after `$quill` / `$kind` / `$ext` and
    /// before any user field. Nested comments on a replaced entry are dropped.
    pub(crate) fn set_seed(&mut self, value: JsonMap<String, JsonValue>) {
        self.set_meta(MetaKey::Seed, value);
    }

    /// Remove an out-of-band meta entry, returning the previous map if any.
    /// Any nested comments attached to the entry are dropped.
    pub(crate) fn take_meta(&mut self, want: MetaKey) -> Option<JsonMap<String, JsonValue>> {
        match self.take_item(|i| matches!(i, PayloadItem::Meta { key, .. } if *key == want))? {
            PayloadItem::Meta { value, .. } => Some(value),
            _ => unreachable!(),
        }
    }

    /// Remove the `$ext` entry, returning the previous map if any. Any
    /// nested comments attached to the entry are dropped.
    pub(crate) fn take_ext(&mut self) -> Option<JsonMap<String, JsonValue>> {
        self.take_meta(MetaKey::Ext)
    }

    fn upsert_meta(&mut self, new: PayloadItem) {
        let new_rank = new
            .meta_rank()
            .expect("upsert_meta only accepts $-typed items");
        for slot in self.items.iter_mut() {
            if slot.meta_rank() == Some(new_rank) {
                *slot = new;
                return;
            }
        }
        let insert_at = self
            .items
            .iter()
            .position(|i| matches!(i.meta_rank(), Some(r) if r > new_rank))
            .unwrap_or_else(|| {
                // Insert after the last lower-ranked `$` item and before any
                // non-`$` entry: keeps the `$` ordering without displacing
                // user fields. That item's inline trailer sits at the same
                // index and stays with it.
                let after = self
                    .items
                    .iter()
                    .rposition(|i| matches!(i.meta_rank(), Some(r) if r < new_rank))
                    .map_or(0, |p| p + 1);
                match self.items.get(after) {
                    Some(PayloadItem::Comment { inline: true, .. }) => after + 1,
                    _ => after,
                }
            });
        self.items.insert(insert_at, new);
    }

    /// Iterator over user `(key, &value)` pairs. Excludes `$` entries and
    /// comments; preserves source order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &QuillValue)> + '_ {
        self.items.iter().filter_map(|item| match item {
            PayloadItem::Field { key, value, .. } => Some((key, value)),
            _ => None,
        })
    }

    /// Iterator over user field keys.
    pub fn keys(&self) -> impl Iterator<Item = &String> + '_ {
        self.items.iter().filter_map(|item| match item {
            PayloadItem::Field { key, .. } => Some(key),
            _ => None,
        })
    }

    /// Number of *user-field* items (`$` entries and comments excluded).
    pub fn len(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, PayloadItem::Field { .. }))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Look up a user-field value by key. `$` entries are invisible here: use
    /// [`quill`](Self::quill) / [`kind`](Self::kind) / [`ext`](Self::ext) /
    /// [`seed`](Self::seed).
    pub fn get(&self, key: &str) -> Option<&QuillValue> {
        self.items.iter().find_map(|item| match item {
            PayloadItem::Field { key: k, value, .. } if k == key => Some(value),
            _ => None,
        })
    }

    /// `true` if a user field with this key is marked `!must_fill`.
    pub fn is_fill(&self, key: &str) -> bool {
        self.items.iter().any(|item| match item {
            PayloadItem::Field { key: k, fill, .. } => k == key && *fill,
            _ => false,
        })
    }

    /// Insert or update a user field, clearing any `!must_fill` marker.
    /// Preserves position for an existing key; appends a new one. `$` entries
    /// and comments are untouched; replacing a field discards its
    /// `nested_comments` (the new value tree may not carry matching positions).
    ///
    /// Carries no field-invariant check: the caller has already validated the
    /// exact stored `(name, value)`, as `Card::store_field` does.
    pub(crate) fn insert_unchecked(
        &mut self,
        key: impl Into<String>,
        value: QuillValue,
    ) -> Option<QuillValue> {
        self.insert_item(key.into(), value, false)
    }

    /// [`insert_unchecked`](Self::insert_unchecked) marking the field a
    /// `!must_fill` placeholder.
    pub(crate) fn insert_fill_unchecked(
        &mut self,
        key: impl Into<String>,
        value: QuillValue,
    ) -> Option<QuillValue> {
        self.insert_item(key.into(), value, true)
    }

    /// Insert or replace field `key` with `value`, setting its fill marker.
    /// Position-preserving for an existing key, append otherwise. The item's
    /// `fill` flag is the one carrier of a root marker, so a root fill bit on
    /// the incoming tree is cleared rather than stored beside it.
    fn insert_item(
        &mut self,
        key: String,
        mut value: QuillValue,
        fill: bool,
    ) -> Option<QuillValue> {
        value.clear_root_fill();
        for item in self.items.iter_mut() {
            if let PayloadItem::Field {
                key: k,
                value: v,
                fill: item_fill,
            } = item
            {
                if k == &key {
                    let old = std::mem::replace(v, value);
                    *item_fill = fill;
                    self.prune_nested(&key);
                    return Some(old);
                }
            }
        }
        self.items.push(PayloadItem::Field { key, value, fill });
        None
    }

    /// Remove a user field by key, returning its value. Comments and `$`
    /// entries are untouched.
    pub(crate) fn remove(&mut self, key: &str) -> Option<QuillValue> {
        match self.take_item(|item| matches!(item, PayloadItem::Field { key: k, .. } if k == key))? {
            PayloadItem::Field { value, .. } => Some(value),
            _ => unreachable!(),
        }
    }

    /// Project the user-field portion into an `IndexMap<String, QuillValue>`.
    /// Comments, fill markers, and `$` entries are dropped. Preserves order.
    pub fn to_index_map(&self) -> IndexMap<String, QuillValue> {
        let mut map = IndexMap::new();
        for item in &self.items {
            if let PayloadItem::Field { key, value, .. } = item {
                map.insert(key.clone(), value.clone());
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qv(s: &str) -> QuillValue {
        QuillValue::from_json(serde_json::json!(s))
    }

    #[test]
    fn insert_new_appends_after_meta() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        fm.insert_unchecked("title", qv("Hello"));
        let last = fm.items().last().unwrap();
        assert!(matches!(last, PayloadItem::Field { key, .. } if key == "title"));
    }

    #[test]
    fn insert_existing_preserves_position() {
        let mut fm = Payload::new();
        fm.insert_unchecked("a", qv("1"));
        fm.insert_unchecked("b", qv("2"));
        fm.insert_unchecked("a", qv("updated"));
        let keys: Vec<&String> = fm.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(fm.get("a").unwrap().as_str(), Some("updated"));
    }

    #[test]
    fn insert_clears_fill() {
        let mut fm = Payload::new();
        fm.insert_fill_unchecked("k", qv("placeholder"));
        assert!(fm.is_fill("k"));
        fm.insert_unchecked("k", qv("user value"));
        assert!(!fm.is_fill("k"));
    }

    #[test]
    fn unchecked_insert_stores_what_it_is_handed() {
        let mut fm = Payload::new();
        fm.insert_unchecked("bad name", qv("v"));
        assert_eq!(fm.items().len(), 1);
    }

    #[test]
    fn map_style_iter_skips_meta_and_comments() {
        let mut fm = Payload::new();
        fm.set_quill("foo@0.1".parse().unwrap());
        fm.set_kind("main");
        fm.insert_unchecked("title", qv("Hello"));
        let items = fm.items().to_vec();
        let mut items_with_comment = items;
        items_with_comment.insert(2, PayloadItem::comment("c"));
        let fm = Payload::from_items(items_with_comment);
        let pairs: Vec<(String, String)> = fm
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect();
        assert_eq!(pairs, vec![("title".to_string(), "Hello".to_string())]);
        assert_eq!(fm.kind(), Some("main"));
    }

    #[test]
    fn set_quill_inserts_at_position_zero() {
        let mut fm = Payload::new();
        fm.set_kind("main");
        fm.set_quill("foo@0.1".parse().unwrap());
        assert!(matches!(fm.items()[0], PayloadItem::Quill { .. }));
        assert!(matches!(fm.items()[1], PayloadItem::Kind { .. }));
    }

    #[test]
    fn set_replaces_in_place_preserving_comments() {
        let mut fm = Payload::from_items(vec![
            PayloadItem::Quill {
                reference: "foo@0.1".parse().unwrap(),
            },
            PayloadItem::comment_inline("trailing"),
            PayloadItem::Kind {
                value: "main".into(),
            },
        ]);
        fm.set_quill("bar@0.2".parse().unwrap());
        assert_eq!(fm.quill().unwrap().to_string(), "bar@0.2");
        assert_eq!(fm.items().len(), 3);
        assert!(matches!(fm.items()[1], PayloadItem::Comment { .. }));
    }

    #[test]
    fn remove_leaves_comments_and_meta_alone() {
        let mut fm = Payload::from_items(vec![
            PayloadItem::Quill {
                reference: "q".parse().unwrap(),
            },
            PayloadItem::Kind {
                value: "main".into(),
            },
            PayloadItem::comment("header"),
            PayloadItem::field("a", qv("1")),
            PayloadItem::comment("mid"),
            PayloadItem::field("b", qv("2")),
        ]);
        let removed = fm.remove("a").unwrap();
        assert_eq!(removed.as_str(), Some("1"));
        assert!(matches!(fm.items()[0], PayloadItem::Quill { .. }));
        assert!(matches!(fm.items()[1], PayloadItem::Kind { .. }));
        let comments: Vec<&str> = fm
            .items()
            .iter()
            .filter_map(|item| match item {
                PayloadItem::Comment { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(comments, vec!["header", "mid"]);
    }

    /// A nested comment addressed to `key`, one segment deep.
    fn nested(key: &str, text: &str) -> NestedComment {
        NestedComment {
            container_path: vec![PathSegment::Key(key.to_string())],
            position: 0,
            text: text.to_string(),
            inline: false,
        }
    }

    fn owners(fm: &Payload) -> Vec<&str> {
        fm.nested_comments()
            .iter()
            .map(|nc| match &nc.container_path[0] {
                PathSegment::Key(k) => k.as_str(),
                PathSegment::Index(_) => unreachable!("rooted at a key"),
            })
            .collect()
    }

    fn payload_with_nested() -> Payload {
        Payload::from_items_with_nested(
            vec![
                PayloadItem::Meta {
                    key: MetaKey::Ext,
                    value: JsonMap::new(),
                },
                PayloadItem::field("a", qv("1")),
                PayloadItem::field("b", qv("2")),
            ],
            vec![nested("a", "in a"), nested("b", "in b"), nested("$ext", "in ext")],
        )
    }

    /// Comments key off the owning entry, so touching one entry leaves every
    /// other entry's comments where they were.
    #[test]
    fn replacing_an_entry_prunes_only_its_own_nested_comments() {
        let mut fm = payload_with_nested();
        fm.insert_unchecked("a", qv("updated"));
        assert_eq!(owners(&fm), vec!["b", "$ext"]);

        let mut fm = payload_with_nested();
        fm.set_ext(JsonMap::new());
        assert_eq!(owners(&fm), vec!["a", "b"]);
    }

    #[test]
    fn removing_an_entry_takes_its_nested_comments_with_it() {
        let mut fm = payload_with_nested();
        fm.remove("a").expect("a is a field");
        assert_eq!(owners(&fm), vec!["b", "$ext"]);

        let mut fm = payload_with_nested();
        fm.take_meta(MetaKey::Ext).expect("$ext is present");
        assert_eq!(owners(&fm), vec!["a", "b"]);
    }

    /// Renaming carries them: the key is how a comment finds its value, so a
    /// rename that left the head behind would orphan every comment inside it.
    #[test]
    fn renaming_a_field_carries_its_nested_comments() {
        let mut fm = payload_with_nested();
        fm.rename_field("a", "renamed".to_string());
        assert_eq!(owners(&fm), vec!["renamed", "b", "$ext"]);
        assert_eq!(fm.nested_comments_for("renamed").len(), 1);
        assert!(fm.nested_comments_for("a").is_empty());
    }

    #[test]
    fn nested_comments_for_rebases_onto_the_entry() {
        let fm = Payload::from_items_with_nested(
            vec![PayloadItem::field("a", qv("1"))],
            vec![NestedComment {
                container_path: vec![
                    PathSegment::Key("a".to_string()),
                    PathSegment::Index(2),
                ],
                position: 1,
                text: "deep".to_string(),
                inline: false,
            }],
        );
        let got = fm.nested_comments_for("a");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].container_path, vec![PathSegment::Index(2)]);
        assert_eq!(got[0].position, 1);
    }
}
