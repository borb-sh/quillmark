//! Consumer-facing operations on a [`Quill`]: validation, seeding, and the
//! zero-filled compile to backend wire JSON. All pure reads of the quill's
//! config — no backend, no engine (those live in the `quillmark` crate).

use std::str::FromStr;

use indexmap::IndexMap;

use super::resolved::FieldSource;
use super::{seed, CardSchema, FieldSchema, FieldType, Leniency, Quill, QuillConfig};
use crate::normalize::{normalize_document, normalize_field_name};
use crate::quill::zero_value;
use crate::path::DocPath;
use crate::{
    Card, Diagnostic, Document, Payload, QuillValue, RenderError, SeedOverlay, Severity, Version,
};

impl Quill {
    /// [`QuillConfig::compile_data`] on this quill's config.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        self.config().compile_data(doc)
    }

    /// Validate without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().dry_run(doc)
    }

    /// [`QuillConfig::check_quill_reference`] on this quill's config.
    pub fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().check_quill_reference(doc)
    }
}

/// The document→data compile is a pure config read: coercion, validation,
/// normalization, and zero-fill consult only the parsed schemas — never the
/// quill's file tree. Living on [`QuillConfig`] lets a consumer that only
/// compiles data (e.g. a live session's `apply`) retain the config alone
/// rather than the whole quill with its font/package bytes.
impl QuillConfig {
    /// Applies coercion, validation, normalization, and **zero-filled render**:
    /// every absent schema field is resolved to its authored value, else its
    /// schema default, else its type-empty zero value — in this plate-JSON
    /// projection only, never in the persisted document. A merely *incomplete*
    /// document compiles fine; only a *malformed* one (a value that won't
    /// coerce/validate) errors. A `!must_fill` placeholder never gates render —
    /// it surfaces as a non-fatal warning from `validate`. See
    /// `prose/canon/SCHEMAS.md`.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        // The gate is the **one** coercion pass: `coerce_and_validate` conforms
        // every field (Render leniency, fallible) and validates, erroring on a
        // malformed document. The ladder below consumes its coerced, NFC-normalized
        // output rather than re-conforming — a document that reaches the ladder is
        // already Render-conformed, so the plate is the sourced ladder with its
        // rungs dropped. `resolve()` runs the total (keep-raw) conform for its own
        // fallibility-free path; both cut the same [`ladder_sourced`].
        let coerced = self.coerce_and_validate(doc)?;
        let normalized = normalize_document(coerced)?;

        let final_main = Card::from_parts(
            rebuild_payload_with_meta(
                normalized.main(),
                plate_fields(ladder_sourced(
                    &self.main,
                    &normalized.main().payload().to_index_map(),
                )),
            ),
            normalized.main().body().clone(),
        );
        let cards_resolved: Vec<Card> = normalized
            .cards()
            .iter()
            .map(|card| {
                let fields = match self.card_kind(card.kind().unwrap_or("")) {
                    Some(schema) => {
                        plate_fields(ladder_sourced(schema, &card.payload().to_index_map()))
                    }
                    // Unknown-kind card: authored fields verbatim, no ladder — as
                    // the resolved-value view leaves it (`card_states`).
                    None => card.payload().to_index_map(),
                };
                Card::from_parts(rebuild_payload_with_meta(card, fields), card.body().clone())
            })
            .collect();

        let mut plate =
            Document::from_main_and_cards(final_main, cards_resolved).to_plate_json();
        gate_undefined_body(self, &mut plate);
        Ok(plate)
    }

    /// Validate without backend compilation.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.check_quill_reference(doc)?;
        self.coerce_and_validate(doc).map(|_| ())
    }

    fn coerce_and_validate(&self, doc: &Document) -> Result<Document, RenderError> {
        let coerced_payload = self
            .coerce_payload(&doc.main().payload().to_index_map())
            .map_err(coercion_error)?;

        let mut coerced_cards: Vec<Card> = Vec::with_capacity(doc.cards().len());
        for card in doc.cards() {
            let coerced_fields = self
                .coerce_card(card.kind().unwrap_or(""), &card.payload().to_index_map())
                .map_err(coercion_error)?;
            coerced_cards.push(Card::from_parts(
                rebuild_payload_with_meta(card, coerced_fields),
                card.body().clone(),
            ));
        }

        let coerced_main = Card::from_parts(
            rebuild_payload_with_meta(doc.main(), coerced_payload),
            doc.main().body().clone(),
        );
        let coerced_doc = Document::from_main_and_cards(coerced_main, coerced_cards);

        // Only *malformed* input is fatal (a value that won't coerce/validate).
        // An incomplete document — absent fields or `!must_fill` placeholders —
        // renders fine via zero-fill. `validate_document` returns `Err` only
        // with a non-empty error list; each error keeps its own `path` for UI
        // navigation.
        self.validate_document(&coerced_doc).map_err(|errors| {
            RenderError::new(errors.iter().map(|e| e.to_diagnostic()).collect())
        })?;

        Ok(coerced_doc)
    }

    /// Enforce the document's `$quill` reference (`name@selector`) against this
    /// quill, failing with a `quill::name_mismatch` / `quill::version_mismatch`
    /// diagnostic if either component diverges. The document is well-formed; it
    /// was paired with the wrong quill
    /// — a different format, or an incompatible version of one — which yields
    /// undefined output, so it errors rather than warns.
    ///
    /// Name is the prerequisite (a selector belongs to a *named* quill): a name
    /// mismatch (`quill::name_mismatch`) short-circuits and the version is left
    /// unevaluated; otherwise the selector is checked (`quill::version_mismatch`).
    /// The version parses infallibly in practice (validated at load); if it
    /// somehow doesn't, the version check is skipped.
    pub fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        let doc_ref = doc.quill_reference();

        if doc_ref.name.as_str() != self.name {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but was rendered with '{}'",
                    doc_ref, self.name
                ),
                "quill::name_mismatch",
                "render with the quill named by $quill, or update the $quill name",
            ));
        }

        let Ok(quill_version) = Version::from_str(&self.version) else {
            return Ok(());
        };
        if !doc_ref.selector.matches(quill_version) {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but the loaded quill is version '{}'",
                    doc_ref, quill_version
                ),
                "quill::version_mismatch",
                "render with a quill whose version satisfies the selector, or update the $quill selector",
            ));
        }

        Ok(())
    }
}

impl Quill {
    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty `Vec` when the document is valid).
    ///
    /// The editor-facing validation surface. Forwards the canonical
    /// `validation::*` diagnostics verbatim (same code, `path`, `hint`) so
    /// consumers route on the code without parsing message text: type
    /// mismatches, unknown card kinds, body-on-disabled-body, and the non-fatal
    /// `validation::must_fill` warning — the only non-fatal one; the rest are
    /// blockers. Field absence is not surfaced (it zero-fills at render).
    ///
    /// Field values, defaults, and presentation order are not part of this
    /// surface — read them from the [`Document`] payload and the quill schema
    /// (`quill.config().schema()`, whose key order is display order).
    pub fn validate(&self, doc: &Document) -> Vec<Diagnostic> {
        let mut diags = match self.config().validate_document(doc) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
        };
        diags.extend(validate_fills(self.config(), doc));
        diags.extend(self.validate_seed(doc));
        diags
    }

    /// Advisory validation of the main card's `$seed` overlays.
    ///
    /// Seed overlays are editor-surface only: they never gate render
    /// (`compile_data` / `dry_run` ignore `$seed`), so every diagnostic here is
    /// a **warning** rooted at `$seed.<kind>[.<field>]`. An overlay keyed by a
    /// name that is not a declared `card_kind` is flagged; otherwise each
    /// overlaid field is checked against that kind's schema with the same
    /// conformance core the schema's own `example:` / `default:` literals use
    /// (partial values allowed, no null/absence gating).
    /// The reserved `$body` key is the body override, not a field, and is
    /// skipped.
    fn validate_seed(&self, doc: &Document) -> Vec<Diagnostic> {
        let Some(seed_map) = doc.main().payload().seed() else {
            return Vec::new();
        };
        let config = self.config();
        let mut diags = Vec::new();
        for (kind, overlay) in seed_map {
            let Some(card_schema) = config.card_kind(kind) else {
                diags.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!("`$seed` overlay targets unknown card kind `{kind}`"),
                    )
                    .with_code("validation::seed_unknown_kind".to_string())
                    .with_path(DocPath::new().field("$seed").field(kind).to_string())
                    .with_hint(format!(
                        "Remove the `{kind}` overlay, or rename it to a declared card kind."
                    )),
                );
                continue;
            };
            let Some(obj) = overlay.as_object() else {
                diags.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!("`$seed.{kind}` must be a mapping of field overrides"),
                    )
                    .with_code("validation::seed_overlay_shape".to_string())
                    .with_path(DocPath::new().field("$seed").field(kind).to_string()),
                );
                continue;
            };
            for (field, value) in obj {
                if field == "$body" {
                    continue;
                }
                let field_path = DocPath::new().field("$seed").field(kind).field(field);
                let Some(field_schema) = card_schema.fields.get(field) else {
                    diags.push(
                        Diagnostic::new(
                            Severity::Warning,
                            format!("`$seed.{kind}.{field}` is not a field of card kind `{kind}`"),
                        )
                        .with_code("validation::seed_unknown_field".to_string())
                        .with_path(field_path.to_string()),
                    );
                    continue;
                };
                let qv = QuillValue::from_json(value.clone());
                for violation in
                    super::validation::validate_schema_literal(field_schema, &qv, &field_path)
                {
                    diags.push(seed_violation_diagnostic(&violation));
                }
            }
        }
        diags
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each committing its fields' `example`
    /// values and leaving all other fields absent (interpolated at render:
    /// `default` → type-empty zero). The committed, structured "filled-out" twin
    /// of the [`blueprint`](crate::quill::QuillConfig::blueprint). See the
    /// `seed` module.
    pub fn seed_document(&self) -> Document {
        seed::seed_document(self)
    }

    /// Seed a starter main [`Card`] (carries `$quill`). Use as the main card of
    /// a fresh document. See [`Quill::seed_document`].
    pub fn seed_main(&self) -> Card {
        seed::seed_main(self)
    }

    /// Seed a starter composable [`Card`] of the given kind (carries `$kind`),
    /// layering an optional per-kind [`SeedOverlay`] over the schema-example
    /// base (`overlay › example › absent`); `None` if the kind is not declared.
    /// Use to add a new card to a document — pass the document's `$seed` entry
    /// for the kind (`doc.main().seed().and_then(|m| m.get(card_kind)).and_then(SeedOverlay::from_json)`)
    /// so a card spawned into a template-derived document inherits its curated
    /// starting values, and `None` for the bare schema seed.
    pub fn seed_card(&self, card_kind: &str, overlay: Option<&SeedOverlay>) -> Option<Card> {
        seed::seed_card_for_kind(self, card_kind, overlay)
    }
}

/// A single-diagnostic quill-mismatch failure. `path` is unset — the
/// mismatch is the root `$quill` line, not a field.
fn quill_mismatch(message: String, code: &str, hint: &str) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, message)
            .with_code(code.to_string())
            .with_hint(hint.to_string()),
    )
}

/// Render a seed-overlay validation error as a **warning**-severity diagnostic
/// — seed overlays are advisory and never gate render. The error's `path` is
/// already rooted at `$seed.<kind>.<field>` by the caller.
fn seed_violation_diagnostic(v: &super::validation::ValidationError) -> Diagnostic {
    let mut diag = Diagnostic::new(Severity::Warning, v.to_string())
        .with_code(v.code().to_string())
        .with_path(v.path().to_string());
    if let Some(hint) = v.hint() {
        diag = diag.with_hint(hint);
    }
    diag
}

/// Wrap a coercion error into a `validation::coercion_failed` failure.
/// `Diagnostic::path` is unset — coercion runs before structured validation.
fn coercion_error(e: impl std::fmt::Display) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, e.to_string())
            .with_code("validation::coercion_failed".to_string())
            .with_hint("Ensure all fields can be coerced to their declared types".to_string()),
    )
}

/// The total (keep-raw) resolver behind [`Quill::resolve`](crate::Quill::resolve):
/// conform each authored value under Render leniency (keep-raw on failure — the
/// fallibility-free path a consumer-side view needs), NFC-normalize the key, then
/// cut the shared [`ladder_sourced`]. The render plate reaches the same rows by a
/// different route — its gate does the fallible conform, and `compile_data` hands
/// the coerced result straight to `ladder_sourced` — so the two cut one ladder
/// over equal input (a document that passes the gate never takes the keep-raw
/// branch), never a parallel precedence policy.
pub(crate) fn resolve_card_sourced(
    schema: &CardSchema,
    card: &Card,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    ladder_sourced(schema, &conform_card_render(schema, card))
}

/// Conform one card's authored fields under Render leniency, keep-raw on failure,
/// NFC-normalizing each key — the total (infallible) coercion the resolved-value
/// view runs in place of the render gate's fallible one. Every validated ingress
/// (parse, the mutators) restricts field names to ASCII (NFC-invariant), so the
/// normalization only respells keys on a directly-constructed payload
/// (`Payload::from_index_map`), under the same NFC key the plate carries. A value
/// Render coercion cannot conform is kept raw (the ladder reads it Authored); on a
/// document that passes the gate that branch never fires, so this equals the gated
/// path byte-for-byte.
fn conform_card_render(schema: &CardSchema, card: &Card) -> IndexMap<String, QuillValue> {
    let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
    for (raw_name, value) in card.payload().to_index_map() {
        let name = normalize_field_name(&raw_name);
        let entry = match schema.fields.get(&raw_name) {
            Some(field_schema) => {
                QuillConfig::conform_value(&value, field_schema, &name, Leniency::Render)
                    .unwrap_or(value)
            }
            None => value,
        };
        coerced.insert(name, entry);
    }
    coerced
}

/// The shared sourced ladder both canon projections cut — the render-fidelity
/// plate ([`compile_data`](QuillConfig::compile_data)) and the resolved-value view
/// ([`Quill::resolve`](crate::Quill::resolve)) — over an already-coerced,
/// NFC-normalized field map. For every declared field it reports the value the
/// render projection uses and the [`FieldSource`] rung that produced it; undeclared
/// authored fields carry through verbatim ([`Authored`](FieldSource::Authored)) —
/// the schema is a floor, not an allowlist.
///
/// Field order is authored-first with declared-but-absent fields appended: the
/// render plate's order. Each projection re-cuts the presentation order it wants
/// from this one value-and-source map — the view rows declared fields first in
/// declaration order — rather than re-deriving the ladder against a parallel
/// precedence policy (`prose/canon/SCHEMAS.md` § "Value sources and projections").
/// Null ≡ absent applies recursively inside [`resolve_value_sourced`], so no bare
/// null reaches either projection.
pub(crate) fn ladder_sourced(
    schema: &CardSchema,
    coerced: &IndexMap<String, QuillValue>,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    // Undeclared authored fields seed the map in authored order (verbatim,
    // Authored); the declared fields then overlay in place — or append when
    // absent — each carrying its ladder value and the source rung that produced
    // it. Insert on an existing key preserves its authored position, so the
    // order is authored-first, declared-but-absent appended.
    let mut out: IndexMap<String, (QuillValue, FieldSource)> = coerced
        .iter()
        .map(|(name, value)| (name.clone(), (value.clone(), FieldSource::Authored)))
        .collect();
    for (name, field_schema) in &schema.fields {
        out.insert(
            name.clone(),
            resolve_value_sourced(coerced.get(name), field_schema),
        );
    }
    out
}

/// Drop the source rungs from [`resolve_card_sourced`]'s map — the render plate
/// consumes the value half only; the resolved-value view keeps both.
fn plate_fields(
    sourced: IndexMap<String, (QuillValue, FieldSource)>,
) -> IndexMap<String, QuillValue> {
    sourced
        .into_iter()
        .map(|(name, (value, _source))| (name, value))
        .collect()
}

/// Drop `$body` from the render plate wherever the schema defines no body — the
/// `$body` half of issue 1030's **absent on undefined**: a `$`-metadata field
/// appears on a card exactly when the schema defines it there. [`to_plate_json`]
/// is schema-free and emits `$body` for every card and the root; this is the one
/// place that knows [`body_enabled`], so it applies the schema gate here.
///
/// - Root `$body`: kept iff the main card enables a body.
/// - Card `$body`: kept iff the card's `$kind` names a declared, **body-enabled**
///   kind. A body-disabled kind, an unknown kind (no schema), and a kindless card
///   (no `$kind` key — [`to_plate_json`] already omitted it, its document-defined
///   half) all resolve to "no body slot", matching the resolved-value view
///   ([`Quill::resolve`](crate::Quill::resolve) omits the body row) and the
///   transform schema ([`build_transform_schema`](crate::quill::build_transform_schema)
///   drops `$body` for a body-disabled kind). Unknown-kind and kindless cards are
///   unreachable on this path — `validate_document` rejects them before render —
///   so in practice only the body-disabled edge fires; the rest is the rule made
///   total.
///
/// [`to_plate_json`]: crate::Document::to_plate_json
/// [`body_enabled`]: crate::quill::CardSchema::body_enabled
fn gate_undefined_body(config: &QuillConfig, plate: &mut serde_json::Value) {
    let Some(obj) = plate.as_object_mut() else {
        return;
    };
    if !config.main.body_enabled() {
        obj.shift_remove("$body");
    }
    let Some(cards) = obj.get_mut("$cards").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for card in cards {
        let Some(card_obj) = card.as_object_mut() else {
            continue;
        };
        let body_defined = card_obj
            .get("$kind")
            .and_then(|v| v.as_str())
            .and_then(|kind| config.card_kind(kind))
            .is_some_and(|schema| schema.body_enabled());
        if !body_defined {
            card_obj.shift_remove("$body");
        }
    }
}

/// The value half of [`resolve_value_sourced`], discarding the rung tag — the
/// nested-recursion helper for a typed dictionary's properties and a typed
/// array's elements, where the source of an inner cell is not surfaced (a
/// present dict/array is [`Authored`](FieldSource::Authored) as a whole). Both
/// canon projections cut the sourced ladder through [`resolve_card_sourced`];
/// this is the inner value-only cut beneath it.
fn resolve_value(value: Option<&QuillValue>, field: &FieldSchema) -> QuillValue {
    resolve_value_sourced(value, field).0
}

/// Resolve one (possibly absent or null) value against its field schema,
/// reporting the [`FieldSource`] rung that produced it, and applying null ≡
/// absent recursively so no bare null reaches the plate:
///
/// - A null or absent value becomes the schema `default:`
///   ([`Default`](FieldSource::Default)), else the type-empty [`zero_value`]
///   ([`Zero`](FieldSource::Zero)).
/// - A present **typed dictionary** is rebuilt from its declared properties so a
///   null/absent property zero-fills and the projection matches the schema shape.
///   Source keys the schema does not declare pass through verbatim, matching
///   `config::coerce_object_props`'s coercion-time behavior — the schema is a
///   floor, not an allowlist, so an undeclared `note:` on a typed dict reaches
///   the plate instead of being silently dropped.
/// - A present **typed array** resolves each element against the item schema, so
///   a null element zero-fills in place.
/// - Any other present value is returned unchanged.
///
/// Every present shape is [`Authored`](FieldSource::Authored) (the nested
/// zero-fill inside a dict/array is a projection detail, not a source change).
/// The source is the byproduct of the same branch that computes the value, so
/// the render projection ([`resolve_value`]) and the field-state view cut the
/// one commitment ladder rather than each re-deriving precedence
/// (`prose/canon/SCHEMAS.md` § "Value sources and projections").
pub(crate) fn resolve_value_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
) -> (QuillValue, FieldSource) {
    let present = value.filter(|v| !v.as_json().is_null());
    let Some(v) = present else {
        // A content-bearing field (`richtext` or its literal sibling `plaintext`)
        // commits the *content* form of its default (`default_content`, cached at
        // load by `from_yaml`), so the seam carries canonical Content-JSON the
        // backend can classify. It must NOT fall through to the raw `default`:
        // the ladder injects this default without re-coercing it (coercion
        // touched only authored values), so a bare authored string here would
        // reach the plate uncoerced and be misread. A
        // content field with no cached `default_content` (only reachable via a
        // serde-built `QuillConfig`, never `from_yaml`) zero-fills to the empty
        // content.
        if matches!(
            field.r#type,
            FieldType::RichText { .. } | FieldType::PlainText { .. }
        ) {
            return match field.default_content.clone() {
                Some(content) => (content, FieldSource::Default),
                None => (zero_value(field), FieldSource::Zero),
            };
        }
        // Non-content: `default_content` is always `None`, so use the raw
        // `default`, then the type-empty zero.
        return match field.default.clone() {
            Some(default) => (default, FieldSource::Default),
            None => (zero_value(field), FieldSource::Zero),
        };
    };
    let resolved = match (&field.r#type, &field.properties, &field.items) {
        (FieldType::Object, Some(props), _) => {
            let obj = v.as_json().as_object();
            let mut out = serde_json::Map::new();
            for (pname, pschema) in props {
                let pv = obj
                    .and_then(|o| o.get(pname))
                    .map(|j| QuillValue::from_json(j.clone()));
                out.insert(
                    pname.clone(),
                    resolve_value(pv.as_ref(), pschema).into_json(),
                );
            }
            // Preserve undeclared keys verbatim; only rebuild the ones the
            // schema names. Skips keys already emitted above so a declared
            // property keeps its resolved (zero-filled) value.
            if let Some(o) = obj {
                for (k, v) in o {
                    if !props.contains_key(k) {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            QuillValue::from_json(serde_json::Value::Object(out))
        }
        (FieldType::Array, _, Some(items)) => {
            let arr = v.as_json().as_array().cloned().unwrap_or_default();
            let out: Vec<serde_json::Value> = arr
                .into_iter()
                .map(|e| resolve_value(Some(&QuillValue::from_json(e)), items).into_json())
                .collect();
            QuillValue::from_json(serde_json::Value::Array(out))
        }
        _ => v.clone(),
    };
    (resolved, FieldSource::Authored)
}

/// Build a [`Payload`] from a coerced/defaulted field map, re-attaching `$quill`
/// / `$kind` / `$id` from `source`. Comments are dropped — this payload feeds
/// backend rendering, not round-trip storage.
fn rebuild_payload_with_meta(source: &Card, fields: IndexMap<String, QuillValue>) -> Payload {
    let mut payload = Payload::from_index_map(fields);
    if let Some(q) = source.quill() {
        payload.set_quill(q.clone());
    }
    if let Some(k) = source.kind() {
        payload.set_kind(k.to_string());
    }
    if let Some(id) = source.id() {
        payload.set_id(id.to_string());
    }
    payload
}

/// Surface every `!must_fill` marker as a non-fatal **warning**, root-and-nested
/// across the main card and every composable card.
///
/// The marker fires whether or not the cell carries a suggested value, and never
/// gates render (the cell zero-fills or uses its suggested value). A strict
/// consumer treats any outstanding marker as "not done".
fn validate_fills(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    collect_fill_diags(doc.main(), &DocPath::main(), &mut diags);
    for (index, card) in doc.cards().iter().enumerate() {
        // A card whose declared `$kind` has no schema drops the kind segment and
        // stays `cards[<i>]`, matching `validate_typed_document`; a
        // schema-declared kind qualifies as `cards.<kind>[<i>]`.
        let kind = card.kind().filter(|k| config.card_kind(k).is_some());
        collect_fill_diags(card, &DocPath::card(kind, index), &mut diags);
    }
    diags
}

/// Append a `validation::must_fill` warning for each marker in `card`'s fields.
fn collect_fill_diags(card: &Card, base: &DocPath, out: &mut Vec<Diagnostic>) {
    let payload = card.payload();
    for (key, value) in payload {
        let field_path = base.field(key);
        // Root marker (the field-level `fill` flag) plus any nested markers
        // carried on the value tree, each rebased onto the field path.
        if payload.is_fill(key) {
            out.push(fill_warning(&field_path));
        }
        for nested in value.nonroot_fill_paths() {
            let nested_path = nested.iter().fold(field_path.clone(), |p, s| p.segment(s));
            out.push(fill_warning(&nested_path));
        }
    }
}

fn fill_warning(path: &DocPath) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` is marked `!must_fill` — a placeholder awaiting a value."),
    )
    .with_code("validation::must_fill".to_string())
    .with_path(path)
    .with_hint(
        "Replace the value and drop the `!must_fill` marker, or remove the marker if the \
         current value is intended."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(yaml: &str) -> FieldSchema {
        let value = QuillValue::from_yaml_str(yaml).unwrap();
        FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
    }

    // A typed dictionary carrying a key the schema does not declare keeps that
    // key in the resolved projection (regression guard for #803: the schema is
    // a floor, not an allowlist). Declared-but-absent properties still zero-fill.
    #[test]
    fn typed_dict_preserves_undeclared_keys() {
        let schema = field(
            r#"
type: object
properties:
  street: { type: string }
  zip: { type: integer }
"#,
        );
        let input = QuillValue::from_json(json!({ "street": "1 Infinite Loop", "note": "extra" }));

        let resolved = resolve_value(Some(&input), &schema).into_json();

        assert_eq!(
            resolved,
            json!({ "street": "1 Infinite Loop", "zip": 0, "note": "extra" })
        );
    }

    // A card whose declared `$kind` has no schema anchors its `!must_fill`
    // warning at the bare-index root `cards[<i>].<field>` — matching
    // `validate_typed_document`'s unknown-card path — never `cards.<kind>[<i>]`.
    // A truly kindless card (no `$kind`) stays bare-index the same way.
    #[test]
    fn unknown_kind_card_fill_path_is_bare_index() {
        use crate::document::Payload;

        let config = QuillConfig::from_yaml(
            r#"
quill:
  name: fills_test
  backend: typst
  description: fill path tests
  version: 1.0.0
main:
  fields:
    title:
      type: string
      default: ""
card_kinds:
  known:
    fields:
      note:
        type: string
"#,
        )
        .unwrap();

        let mut main = Payload::new();
        main.set_quill("fills_test@1.0.0".parse().unwrap());
        main.set_kind("main");
        let main = Card::from_parts(main, quillmark_content::Content::empty());

        // Index 0: a card whose `$kind` ("mystery") is not a declared card kind.
        let mut unknown = Card::new("mystery").unwrap();
        unknown
            .store_fill("note", QuillValue::from_json(json!(null)))
            .unwrap();

        // Index 1: a kindless card (no `$kind` at all).
        let mut kindless =
            Card::from_parts(Payload::new(), quillmark_content::Content::empty());
        kindless
            .store_fill("memo", QuillValue::from_json(json!(null)))
            .unwrap();

        let doc = Document::from_main_and_cards(main, vec![unknown, kindless]);
        let paths: Vec<String> = validate_fills(&config, &doc)
            .iter()
            .filter_map(|d| d.path.clone())
            .collect();

        assert!(
            paths.contains(&"cards[0].note".to_string()),
            "unknown-kind card fill must anchor at the bare index; got {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("cards.mystery")),
            "unknown-kind card fill must NOT carry the kind segment; got {paths:?}"
        );
        assert!(
            paths.contains(&"cards[1].memo".to_string()),
            "kindless card fill must anchor at the bare index; got {paths:?}"
        );
    }

    // Same pass-through inside a typed-table row (the Array→Object recursion).
    #[test]
    fn typed_table_row_preserves_undeclared_keys() {
        let schema = field(
            r#"
type: array
items:
  type: object
  properties:
    name: { type: string }
"#,
        );
        let input = QuillValue::from_json(json!([{ "name": "ACME", "year": 2020 }]));

        let resolved = resolve_value(Some(&input), &schema).into_json();

        assert_eq!(resolved, json!([{ "name": "ACME", "year": 2020 }]));
    }

    // ── "Absent on undefined": the render plate drops `$body` wherever the
    //    schema defines none (issue 1030). The `$kind` half is structural in
    //    `to_plate_json`; these pin the schema-gated `$body` half. Only the
    //    body-disabled edge is reachable here — `validate_document` rejects an
    //    unknown-kind or kindless card before render.

    fn plate_of(yaml: &str, md: &str) -> serde_json::Value {
        let config = QuillConfig::from_yaml(yaml).expect("valid quill");
        let doc = Document::parse(md).expect("parse").document;
        config.compile_data(&doc).expect("compile")
    }

    #[test]
    fn body_disabled_kind_omits_dollar_body() {
        let plate = plate_of(
            r#"
quill: { name: bd, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  stamp:
    body:
      enabled: false
    fields:
      label: { type: string }
"#,
            "~~~card-yaml\n$quill: bd@1.0.0\n$kind: main\ntitle: T\n~~~\n\n\
             ~~~card-yaml\n$kind: stamp\nlabel: L\n~~~\n",
        );
        let card = &plate["$cards"][0];
        assert_eq!(card["$kind"], "stamp", "$kind is document-defined, kept");
        assert_eq!(card["label"], "L", "declared fields kept");
        assert!(
            card.get("$body").is_none(),
            "a body-disabled kind carries no $body in the plate; got {card}"
        );
    }

    #[test]
    fn body_disabled_main_omits_root_dollar_body() {
        let plate = plate_of(
            r#"
quill: { name: bdm, version: 1.0.0, backend: typst, description: x }
main:
  body:
    enabled: false
  fields:
    title: { type: string }
"#,
            "~~~card-yaml\n$quill: bdm@1.0.0\n$kind: main\ntitle: T\n~~~\n",
        );
        assert_eq!(plate["title"], "T");
        assert!(
            plate.get("$body").is_none(),
            "a body-disabled main carries no root $body; got {plate}"
        );
    }

    #[test]
    fn body_enabled_keeps_dollar_body() {
        let plate = plate_of(
            r#"
quill: { name: be, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    fields:
      tag: { type: string }
"#,
            "~~~card-yaml\n$quill: be@1.0.0\n$kind: main\ntitle: T\n~~~\n\n\
             Main body.\n\n\
             ~~~card-yaml\n$kind: note\ntag: x\n~~~\nNote body.\n",
        );
        assert_eq!(
            plate["$body"]["text"], "Main body.",
            "a body-enabled main keeps its $body"
        );
        let card = &plate["$cards"][0];
        assert_eq!(
            card["$body"]["text"], "Note body.",
            "a body-enabled kind keeps its $body content object"
        );
    }
}
