//! Consumer-facing operations on a [`Quill`]: validation, seeding, and the
//! blank-filled compile to backend wire JSON. Pure reads of the config.

use std::collections::HashSet;
use std::str::FromStr;

use indexmap::IndexMap;

use super::resolved::FieldSource;
use super::{
    seed, CalendarDate, CardSchema, CoercionError, FieldSchema, FieldType, Leniency, Quill,
    QuillConfig, MATRIX_HELD_KEY, MATRIX_TITLE_KEY, TODAY, VARIANT_DISCRIMINANT_KEY,
};
use crate::normalize::{normalize_document, normalize_field_name};
use crate::quill::blank;
use crate::path::DocPath;
use crate::{
    document::{Card, Document, Payload, SeedOverlay},
    error::{Diagnostic, RenderError, Severity},
    value::QuillValue,
    version::Version,
};

impl Quill {
    /// [`QuillConfig::compile_data`] on this quill's config.
    pub fn compile_data(
        &self,
        doc: &Document,
        today: CalendarDate,
    ) -> Result<serde_json::Value, RenderError> {
        self.config().compile_data(doc, today)
    }

    /// [`QuillConfig::compile_checked`] on this quill's config.
    pub fn compile_checked(
        &self,
        doc: &Document,
        today: CalendarDate,
    ) -> Result<serde_json::Value, RenderError> {
        self.config().compile_checked(doc, today)
    }

    /// [`QuillConfig::dry_run`] on this quill's config.
    pub fn dry_run(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().dry_run(doc)
    }

    /// [`QuillConfig::check_quill_reference`] on this quill's config.
    pub(crate) fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        self.config().check_quill_reference(doc)
    }
}

/// The document→data compile is a pure config read: coercion, validation,
/// normalization, and blank-fill consult only the parsed schemas, never the
/// quill's file tree. Living on [`QuillConfig`] lets a consumer that only
/// compiles data (e.g. a live session's `update`) retain the config alone
/// rather than the whole quill with its font/package bytes.
impl QuillConfig {
    /// Coercion, validation, normalization and blank-filled render into the
    /// plate-JSON projection (`prose/canon/SCHEMAS.md` § "Blank-filled render").
    /// An unanswered cell compiles fine; only a *malformed* value — one that
    /// will not coerce or validate — errors.
    ///
    /// `today` is the render date a [`TODAY`] date renders as.
    pub fn compile_data(
        &self,
        doc: &Document,
        today: CalendarDate,
    ) -> Result<serde_json::Value, RenderError> {
        // The one coercion pass. The ladder below consumes its coerced,
        // NFC-normalized output rather than re-conforming, so the plate is the
        // sourced ladder with its rungs dropped.
        let coerced = self.coerce_and_validate(doc)?;
        let normalized = normalize_document(coerced);

        let final_main = Card::from_parts(
            rebuild_payload_with_meta(
                normalized.main(),
                plate_fields(ladder_sourced(
                    &self.main,
                    &normalized.main().payload().to_index_map(),
                    today,
                )),
            ),
            normalized.main().body().clone(),
        );
        // A card's `$body` is defined for the plate iff its kind resolves to a
        // body-enabled schema. Captured here, where the schema is already in
        // hand for field lowering, so the decision is never re-derived from the
        // serialized plate; `$kind` is gated structurally by the plate builder.
        let mut card_bodies: Vec<bool> = Vec::with_capacity(normalized.cards().len());
        let cards_resolved: Vec<Card> = normalized
            .cards()
            .iter()
            .map(|card| {
                let schema = self.card_kind(card.kind().unwrap_or(""));
                card_bodies.push(schema.is_some_and(|s| s.body_enabled()));
                let fields = match schema {
                    Some(schema) => {
                        plate_fields(ladder_sourced(schema, &card.payload().to_index_map(), today))
                    }
                    // No ladder, as `resolved::card_states` leaves it.
                    None => card.payload().to_index_map(),
                };
                Card::from_parts(rebuild_payload_with_meta(card, fields), card.body().clone())
            })
            .collect();

        Ok(Document::from_main_and_cards(final_main, cards_resolved)
            .to_plate_json_gated(self.main.body_enabled(), Some(&card_bodies)))
    }

    /// [`compile_data`](Self::compile_data) behind the `$quill` pairing check.
    /// Every door that turns a document into plate data for *this* schema goes
    /// through here (`Quillmark::open` for a session's first compile,
    /// [`LiveSession::update`](crate::session::LiveSession::update) for each
    /// edit), so the pairing cannot be checked at one and skipped at the other.
    /// [`compile_data`](Self::compile_data) stays available unchecked where no
    /// render follows and the pairing is the caller's to assert (the CLI's
    /// `--output-data`).
    pub fn compile_checked(
        &self,
        doc: &Document,
        today: CalendarDate,
    ) -> Result<serde_json::Value, RenderError> {
        self.check_quill_reference(doc)?;
        self.compile_data(doc, today)
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

        self.validate_document(&coerced_doc).map_err(|errors| {
            RenderError::new(errors.iter().map(|e| e.to_diagnostic()).collect())
        })?;

        Ok(coerced_doc)
    }

    /// Enforce the document's `$quill` reference (`name@selector`) against this
    /// quill. Every schema-bound door runs it, the bound ingestion
    /// ([`Quill::parse`](crate::quill::Quill::parse) /
    /// [`Quill::conform`](crate::quill::Quill::conform)) included, so the
    /// message names the pairing rather than a verb.
    ///
    /// A selector belongs to a *named* quill, so `quill::name_mismatch`
    /// short-circuits and leaves the version unevaluated; otherwise the selector
    /// is checked (`quill::version_mismatch`). A quill version that will not
    /// parse — load validates it, so nothing in practice — skips that check.
    pub(crate) fn check_quill_reference(&self, doc: &Document) -> Result<(), RenderError> {
        let doc_ref = doc.quill_reference();

        if doc_ref.name.as_str() != self.name {
            return Err(quill_mismatch(
                format!(
                    "document declares $quill '{}' but was paired with '{}'",
                    doc_ref, self.name
                ),
                "quill::name_mismatch",
                "use the quill named by $quill, or update the $quill name",
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
                "use a quill whose version satisfies the selector, or update the $quill selector",
            ));
        }

        Ok(())
    }
}

impl Quill {
    /// Validate `doc` against this quill's schema, returning every diagnostic
    /// (an empty `Vec` when the document is valid): the editor-facing surface.
    ///
    /// The `validation::*` diagnostics are forwarded verbatim — same code,
    /// `path`, `hint` — so a consumer routes on the code without parsing message
    /// text. A blocker here means the document does not render, and a value the
    /// render floor refuses is a blocker here: values are judged in the form the
    /// floor builds from them (`conform_value` at `Leniency::Render`).
    /// `prose/canon/SCHEMAS.md` §"Type coercion" and §"Native validation" carry
    /// the leniencies.
    ///
    /// Field values, defaults, and presentation order are not part of this
    /// surface: read them from the [`Document`] payload and the quill schema
    /// (`quill.config().schema()`, whose key order is display order).
    pub fn validate(&self, doc: &Document) -> Vec<Diagnostic> {
        let mut diags = match self.config().validate_document(doc) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
        };
        diags.extend(validate_unclaimed(self.config(), doc));
        diags.extend(validate_variants(self.config(), doc));
        diags.extend(validate_cardinality(self.config(), doc));
        diags.extend(self.validate_seed(doc));
        diags
    }

    /// Advisory validation of the main card's `$seed` overlays: editor-surface
    /// only, never gating render, so every diagnostic is a **warning** rooted at
    /// `$seed.<kind>[.<field>]`.
    ///
    /// An overlaid field is checked as the **document** value it is — an overlay
    /// cell is what `seed_card` commits into a new card — so the variant
    /// container is a spelling it accepts, a present-null cell reads as absent,
    /// and an omitted field raises nothing. The reserved `$body` key is the body
    /// override, not a field.
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
                let field_path = DocPath::new().field("$seed").field(kind).field(field);
                if field == "$body" {
                    if !card_schema.body_enabled() {
                        diags.push(
                            Diagnostic::new(
                                Severity::Warning,
                                format!(
                                    "`$seed.{kind}.$body` seeds no body: card kind `{kind}` \
                                     declares `body.enabled: false`"
                                ),
                            )
                            .with_code("validation::seed_unknown_field".to_string())
                            .with_path(field_path.to_string()),
                        );
                    }
                    continue;
                }
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
                for violation in super::validation::validate_field(field_schema, &qv, &field_path) {
                    diags.push(seed_violation_diagnostic(&violation));
                }
            }
        }
        diags
    }

    /// The **empty document**: a main card carrying `$quill` and `$kind: main`,
    /// no field committed, no composable card, no body. The type-minimal valid
    /// input, and the document the quill authoring contract binds a plate to
    /// render (`prose/canon/BLUEPRINT.md` §Guarantees).
    ///
    /// The leanest of the three canonical documents, beside the annotated
    /// [`blueprint`](crate::quill::QuillConfig::blueprint) and the
    /// [`seed_document`](Self::seed_document).
    pub fn empty_document(&self) -> Document {
        seed::empty_document(self)
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each with an empty body and every field
    /// absent (interpolated at render: `default` → the field's blank). See the
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
    /// committing an optional per-kind [`SeedOverlay`]'s fields and body;
    /// `None` if the kind is not declared.
    /// Use to add a new card to a document: pass the document's `$seed` entry
    /// for the kind (`doc.main().seed().and_then(|m| m.get(card_kind)).and_then(SeedOverlay::from_json)`)
    /// so a card spawned into a template-derived document inherits its curated
    /// starting values, and `None` for the bare schema seed.
    pub fn seed_card(&self, card_kind: &str, overlay: Option<&SeedOverlay>) -> Option<Card> {
        seed::seed_card_for_kind(self, card_kind, overlay)
    }
}

/// A single-diagnostic quill-mismatch failure. `path` is unset: the
/// mismatch is the root `$quill` line, not a field.
fn quill_mismatch(message: String, code: &str, hint: &str) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, message)
            .with_code(code.to_string())
            .with_hint(hint.to_string()),
    )
}

/// Render a seed-overlay validation error as a **warning**-severity diagnostic:
/// seed overlays are advisory and never gate render. The error's `path` is
/// already rooted at `$seed.<kind>.<field>` by the caller.
fn seed_violation_diagnostic(v: &super::validation::ValidationError) -> Diagnostic {
    let mut diag = Diagnostic::new(Severity::Warning, v.to_string())
        .with_code(v.code().to_string())
        .with_path(v.path().to_string())
        .with_args(v.args());
    if let Some(hint) = v.hint() {
        diag = diag.with_hint(hint);
    }
    diag
}

/// Wrap a coercion error into a `validation::coercion_failed` failure.
/// `Diagnostic::path` is unset: coercion runs before structured validation, and
/// the anchor the error does carry is schema-space (see
/// [`CoercionError::args`](super::config::CoercionError::args)).
fn coercion_error(e: CoercionError) -> RenderError {
    RenderError::from_diag(
        Diagnostic::new(Severity::Error, e.to_string())
            .with_code("validation::coercion_failed".to_string())
            .with_args(e.args())
            .with_hint("Ensure all fields can be coerced to their declared types".to_string()),
    )
}

/// The total (keep-raw) resolver behind
/// [`Quill::resolve`](crate::quill::Quill::resolve): conform each authored
/// value under Render leniency, NFC-normalize the key, then cut the shared
/// [`ladder_sourced`]. `compile_data` reaches the same rows through its own
/// fallible conform, and a document that passes that gate never takes the
/// keep-raw branch, so the two cut one ladder over equal input.
pub(crate) fn resolve_card_sourced(
    schema: &CardSchema,
    card: &Card,
    today: CalendarDate,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    ladder_sourced(schema, &conform_card_render(schema, card), today)
}

/// Conform one card's authored fields under Render leniency, keep-raw on
/// failure, NFC-normalizing each key. Every validated ingress (parse, the
/// mutators) restricts field names to ASCII, which is NFC-invariant, so the
/// normalization only respells keys on a directly-constructed payload
/// (`Payload::from_index_map`). A value Render coercion cannot conform is kept
/// raw and the ladder reads it Authored.
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

/// The shared sourced ladder both canon projections cut, the render-fidelity
/// plate ([`compile_data`](QuillConfig::compile_data)) and the resolved-value
/// view ([`Quill::resolve`](crate::quill::Quill::resolve)), over an
/// already-coerced, NFC-normalized field map. For every declared field it
/// reports the value the render projection uses and the [`FieldSource`] rung
/// that produced it; undeclared authored fields carry through verbatim as
/// [`Authored`](FieldSource::Authored), the schema being a floor, not an
/// allowlist.
///
/// Field order is authored-first with declared-but-absent fields appended: the
/// render plate's order. Each projection re-cuts the presentation order it wants
/// from this one map.
pub(crate) fn ladder_sourced(
    schema: &CardSchema,
    coerced: &IndexMap<String, QuillValue>,
    today: CalendarDate,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    // Insert on an existing key preserves its authored position, which is what
    // makes the order authored-first with declared-but-absent appended.
    let mut out: IndexMap<String, (QuillValue, FieldSource)> = coerced
        .iter()
        .map(|(name, value)| (name.clone(), (value.clone(), FieldSource::Authored)))
        .collect();
    for (name, field_schema) in &schema.fields {
        out.insert(
            name.clone(),
            resolve_value_sourced(coerced.get(name), field_schema, today),
        );
    }
    out
}

/// Drop the source rungs from [`resolve_card_sourced`]'s map: the render plate
/// consumes the value half only; the resolved-value view keeps both.
fn plate_fields(
    sourced: IndexMap<String, (QuillValue, FieldSource)>,
) -> IndexMap<String, QuillValue> {
    sourced
        .into_iter()
        .map(|(name, (value, _source))| (name, value))
        .collect()
}

/// The value half of [`resolve_value_sourced`], discarding the rung tag: the
/// nested cut for a typed array's elements, whose rungs no projection surfaces —
/// an `array` is a cell, since arity is a fact no leaf carries, so its own rung
/// is the one its seed supplied.
fn resolve_value(
    value: Option<&QuillValue>,
    field: &FieldSchema,
    today: CalendarDate,
) -> QuillValue {
    resolve_value_sourced(value, field, today).0
}

/// Resolve one (possibly absent or null) value against its field schema,
/// reporting the [`FieldSource`] rung that produced it, and applying null ≡
/// absent recursively so no bare null reaches the plate.
///
/// Resolution is a descent, not a return (`prose/canon/SCHEMAS.md` § "Cells and
/// namespaces"): this picks a **seed** — the authored value, else the schema
/// `default:` — and hands it to [`compose`], which rebuilds a container from its
/// declared members whichever rung the seed came from and floors a leaf at its
/// [`blank`].
///
/// The rung is the seed's, joined with what the descent found
/// ([`FieldSource::join`]), computed by that same walk.
pub(crate) fn resolve_value_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
    today: CalendarDate,
) -> (QuillValue, FieldSource) {
    if field.is_variant_bearing() {
        return resolve_variant_sourced(value, field, today);
    }
    let (seed, source) = match value.filter(|v| !is_unanswered(v, field)) {
        Some(v) => (Some(v.clone()), FieldSource::Authored),
        None => match seed_default(field) {
            Some(default) => (Some(default), FieldSource::Default),
            None => (None, FieldSource::Blank),
        },
    };
    let (resolved, composed) = compose(seed.as_ref(), field, source, today);
    (resolved, source.join(composed))
}

/// Null, and an optional enum's `""`: the blank spelled in a document, which
/// an optional cell renders as `none` like any other unanswered one.
fn is_unanswered(value: &QuillValue, field: &FieldSchema) -> bool {
    match value.as_json() {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => {
            s.is_empty() && field.optional && matches!(field.r#type, FieldType::Enum { .. })
        }
        _ => false,
    }
}

/// The `default:` a cell enters the descent with, in the form the plate takes it.
///
/// `default_content` holds the imported form, cached at load wherever the type
/// tree bears a content leaf. The ladder injects a default without re-coercing
/// it, so the cache is the only safe source: a raw `default` would cross as
/// unimported markdown. A content-bearing tree whose companion is absent
/// therefore has *no* seed — the gate `populate_field_content` is written
/// against.
fn seed_default(field: &FieldSchema) -> Option<QuillValue> {
    if let Some(content) = field.default_content.clone() {
        return Some(content);
    }
    if crate::quill::config::field_contains_content(field) {
        return None;
    }
    field.default.clone()
}

/// Build `field`'s value from `seed`, the value its own rung supplied (`None`
/// where no rung above the floor had one), and report the strongest rung any
/// cell below contributed. Terminates because each recursion descends strictly
/// into the schema tree.
///
/// A typed dictionary's cells each cut their own ladder over their slice of the
/// seed, so an absent property resolves to *its* `default:` before its blank,
/// and an undeclared seed key passes through verbatim as
/// `config::coerce_object_props` passes it. `seed_rung` ceilings the cells'
/// ([`compose_members`]).
fn compose(
    seed: Option<&QuillValue>,
    field: &FieldSchema,
    seed_rung: FieldSource,
    today: CalendarDate,
) -> (QuillValue, FieldSource) {
    if seed.is_none() && field.optional {
        return (blank(field), FieldSource::Blank);
    }
    match (&field.r#type, field.namespace_props(), &field.items) {
        (FieldType::Object | FieldType::Matrix { .. }, Some(props), _)
            if composes_as(seed, serde_json::Value::is_object) =>
        {
            let obj = seed.and_then(|v| v.as_json().as_object());
            let mut out = serde_json::Map::new();
            let rung = compose_members(obj, props, seed_rung, today, &mut out);
            // Preserve undeclared keys verbatim; only rebuild the ones the
            // schema names. Skips keys already emitted above so a declared
            // property keeps its resolved (blank-filled) value.
            if let Some(o) = obj {
                for (k, v) in o {
                    if !props.contains_key(k) {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            close_matrix_wire(field, &mut out);
            (QuillValue::from_json(serde_json::Value::Object(out)), rung)
        }
        (FieldType::Array, _, Some(items)) if composes_as(seed, serde_json::Value::is_array) => {
            let arr = seed
                .and_then(|v| v.as_json().as_array().cloned())
                .unwrap_or_default();
            let out: Vec<serde_json::Value> = arr
                .into_iter()
                .map(|e| resolve_value(Some(&QuillValue::from_json(e)), items, today).into_json())
                .collect();
            (
                QuillValue::from_json(serde_json::Value::Array(out)),
                FieldSource::Blank,
            )
        }
        _ => match seed {
            Some(v) if is_today(v, field) => (
                QuillValue::from_json(serde_json::Value::String(today.to_string())),
                FieldSource::Blank,
            ),
            Some(v) => (v.clone(), FieldSource::Blank),
            None => (blank(field), FieldSource::Blank),
        },
    }
}

fn is_today(value: &QuillValue, field: &FieldSchema) -> bool {
    matches!(field.r#type, FieldType::Date) && value.as_str() == Some(TODAY)
}

/// Whether a stored matrix member reads as ticked, judged as the ladder judges
/// it: the spelling coercion reads (`matrix_member_spelling`), with the tick
/// itself put through the render floor's own boolean coercion. Reading the raw
/// scalar instead would call `held: "false"` ticked where the plate calls it
/// unticked.
fn is_held(member: &FieldSchema, stored: &serde_json::Value) -> bool {
    let Some(spelled) = super::config::matrix_member_spelling(stored) else {
        return false;
    };
    let (Some(raw), Some(schema)) = (
        spelled.get(MATRIX_HELD_KEY),
        member
            .properties
            .as_ref()
            .and_then(|p| p.get(MATRIX_HELD_KEY)),
    ) else {
        return false;
    };
    QuillConfig::conform_value(
        &QuillValue::from_json(raw.clone()),
        schema,
        MATRIX_HELD_KEY,
        Leniency::Render,
    )
    .ok()
    .and_then(|v| v.as_json().as_bool())
    .unwrap_or(false)
}

/// Write a matrix's roster onto the composed members, and close the wire over
/// the unheld ones.
///
/// `title` is the projection's, not the document's: a matrix carries it on every
/// member whatever the document holds, so a plate reads a label it never has to
/// look up. An unheld member's columns render at their blanks for the reason a
/// variant's unselected world does not render at all — the wire carries the live
/// world only, so a plate reads `held` and its columns without a guard and never
/// prints a stranded answer. What the document retains under an unticked member
/// is a fact about the stored form alone.
///
/// A no-op for every other type.
fn close_matrix_wire(field: &FieldSchema, out: &mut serde_json::Map<String, serde_json::Value>) {
    let roster = field.r#type.matrix_roster();
    if roster.is_empty() {
        return;
    }
    let columns = field.matrix_columns();
    for (id, title) in roster {
        let Some(serde_json::Value::Object(member)) = out.get_mut(id) else {
            continue;
        };
        let held = member
            .get(MATRIX_HELD_KEY)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !held {
            for (name, column) in columns {
                member.insert(name.clone(), blank(column).into_json());
            }
        }
        member.insert(
            MATRIX_TITLE_KEY.to_string(),
            serde_json::Value::String(title.clone()),
        );
    }
}

/// Whether `seed` composes as the container: absent, so the blank container is
/// the whole answer, or already of `shape`.
///
/// A present seed of another shape (`rows: abc` on an `array`, `addr: 5` on a
/// typed dictionary) is kept raw by the scalar arm instead, as
/// [`conform_card_render`] keeps it: [`resolve_value_sourced`] reports the seed's
/// own rung, so a rebuilt container would carry the document's label over content
/// the document never wrote. The gate refuses the shape
/// (`validation::type_mismatch`), so only the ungated views meet it.
fn composes_as(seed: Option<&QuillValue>, shape: fn(&serde_json::Value) -> bool) -> bool {
    seed.is_none_or(|v| shape(v.as_json()))
}

/// Resolve every declared member of a namespace over its slice of `seed` into
/// `out`, reporting the strongest rung any of them contributed. The two
/// namespaces a schema can spell — a typed dictionary's `properties` and the
/// live world of a variant container — compose identically.
///
/// `seed_rung` **ceilings** its members': [`resolve_value_sourced`] cannot tell
/// a seeded value from a written one, so without the ceiling a cell fed from a
/// container `default:` would report itself authored.
fn compose_members(
    seed: Option<&serde_json::Map<String, serde_json::Value>>,
    members: &IndexMap<String, Box<FieldSchema>>,
    seed_rung: FieldSource,
    today: CalendarDate,
    out: &mut serde_json::Map<String, serde_json::Value>,
) -> FieldSource {
    let ceiling = match seed_rung {
        FieldSource::Authored => FieldSource::Authored,
        _ => FieldSource::Default,
    };
    let mut rung = FieldSource::Blank;
    for (name, schema) in members {
        let cell = seed
            .and_then(|o| o.get(name))
            .map(|j| QuillValue::from_json(j.clone()));
        let (value, source) = resolve_value_sourced(cell.as_ref(), schema, today);
        rung = rung.join(source.capped_at(ceiling));
        out.insert(name.clone(), value.into_json());
    }
    rung
}

/// Resolve a variant-bearing enum into the container the plate receives:
/// `{value: <member>}` plus, when that member owns a field set, exactly that
/// set — each cell blank-filled by the ordinary ladder. Only the live world's
/// fields cross, which is the closed shape `prose/canon/SCHEMAS.md` §"Enum
/// variants" describes.
///
/// The discriminant cuts the same ladder as any enum, but the container's own
/// rung is the *cell's*: one the document wrote reads authored whichever rung
/// filled the tag, joined with what its live world's cells contributed
/// ([`compose_members`]).
fn resolve_variant_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
    today: CalendarDate,
) -> (QuillValue, FieldSource) {
    let present = value.filter(|v| !v.as_json().is_null());
    // A present seed that is neither the container nor a bare member name is
    // kept raw, as any mis-shaped container is ([`composes_as`]): the rung is
    // the document's, and a blank world under it would read as an answer the
    // document gave.
    if let Some(raw) =
        present.filter(|v| !v.as_json().is_object() && v.as_json().as_str().is_none())
    {
        return (raw.clone(), FieldSource::Authored);
    }
    let authored = present.map(|v| v.as_json());
    // Coercion normalizes to the container, so the authored discriminant is the
    // `value` key. A bare scalar that bypassed coercion (a serde-built payload)
    // still reads, keeping this total.
    let authored_member = FieldSchema::authored_member(authored).and_then(|v| v.as_str());

    let (member, source) = match authored_member {
        Some(member) => (member.to_string(), FieldSource::Authored),
        None => match field.default.as_ref().and_then(|d| d.as_str()) {
            Some(default) => (default.to_string(), FieldSource::Default),
            None => (String::new(), FieldSource::Blank),
        },
    };
    let source = if present.is_some() {
        FieldSource::Authored
    } else {
        source
    };

    let mut out = serde_json::Map::new();
    out.insert(
        VARIANT_DISCRIMINANT_KEY.to_string(),
        serde_json::Value::String(member.clone()),
    );
    // The cells are seeded from the authored container, never from the
    // discriminant's `default:` — a member the schema chose brings no values
    // with it — so their ceiling is whether the document wrote the container,
    // not which rung supplied the tag.
    let seed_rung = match present {
        Some(_) => FieldSource::Authored,
        None => FieldSource::Blank,
    };
    let cells = match field.variant_fields(&member) {
        Some(fields) => compose_members(
            authored.and_then(|j| j.as_object()),
            fields,
            seed_rung,
            today,
            &mut out,
        ),
        None => FieldSource::Blank,
    };
    (
        QuillValue::from_json(serde_json::Value::Object(out)),
        source.join(cells),
    )
}

/// Build a [`Payload`] from a coerced/defaulted field map, re-attaching `$quill`
/// / `$kind` from `source`. Comments are dropped: this payload feeds
/// backend rendering, not round-trip storage.
fn rebuild_payload_with_meta(source: &Card, fields: IndexMap<String, QuillValue>) -> Payload {
    let mut payload = Payload::from_index_map(fields);
    if let Some(q) = source.quill() {
        payload.set_quill(q.clone());
    }
    if let Some(k) = source.kind() {
        payload.set_kind(k.to_string());
    }
    payload
}

/// Every card of `doc`, the main card first, with the schema its kind resolves
/// to (`None` for an undeclared kind) and the [`DocPath`] it is reported under.
///
/// A card whose `$kind` has no schema drops the kind segment and stays
/// `cards[<i>]`; a schema-declared kind qualifies as `cards.<kind>[<i>]`.
fn schema_cards<'a>(
    config: &'a QuillConfig,
    doc: &'a Document,
) -> impl Iterator<Item = (Option<&'a CardSchema>, &'a Card, DocPath)> {
    std::iter::once((Some(&config.main), doc.main(), DocPath::main())).chain(
        doc.cards().iter().enumerate().map(move |(index, card)| {
            let schema = card.kind().and_then(|k| config.card_kind(k));
            let kind = card.kind().filter(|_| schema.is_some());
            (schema, card, DocPath::card(kind, index))
        }),
    )
}

/// Report the input no declaration claims: a card whose `$kind` is
/// undeclared, body prose under `body.enabled: false`, and a key no
/// declaration at its position names. Each renders without the input
/// (`prose/canon/SCHEMAS.md` § "What blocks a render"), so each is a warning.
fn validate_unclaimed(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let kinds: Vec<&str> = config.card_kinds.iter().map(|k| k.name.as_str()).collect();
    let mut diags = Vec::new();
    for (schema, card, path) in schema_cards(config, doc) {
        let Some(schema) = schema else {
            if let Some(kind) = card.kind() {
                diags.push(unknown_card_warning(&path, kind, &kinds));
            }
            continue;
        };
        // A whitespace-only body is empty: only meaningful prose warns.
        if !schema.body_enabled() && !card.body().is_blank() {
            diags.push(body_disabled_warning(&path.body(), &schema.name));
        }
        let declared: Declared = schema.fields.iter().map(|(n, f)| (n.as_str(), f)).collect();
        let authored: Vec<(&str, &serde_json::Value)> = card
            .payload()
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_json()))
            .collect();
        collect_unknown_keys(&declared, &authored, &path, &mut diags);
    }
    diags
}

/// A mapping's declared fields in declaration order, which breaks a
/// suggestion tie.
type Declared<'a> = IndexMap<&'a str, &'a FieldSchema>;

/// A declared variant-bearing field and the member its authored value selects.
type Container<'a> = (&'a str, &'a FieldSchema, String);

/// Warn at each key of one authored mapping that `declared` does not name, and
/// descend into each one it does. `$` keys never reach here: the payload
/// iterator excludes them, and nested ones are ordinary keys.
fn collect_unknown_keys(
    declared: &Declared,
    authored: &[(&str, &serde_json::Value)],
    base: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    // Built at the first unknown key, once per mapping: a mapping of `n`
    // unknown keys stays linear in `n`.
    let mut hints: Option<(Vec<&str>, Vec<Container>)> = None;
    for &(key, value) in authored {
        let path = base.field(key);
        if let Some(field) = declared.get(key) {
            collect_unknown_in(field, value, &path, out);
            continue;
        }
        let (unwritten, containers) = hints.get_or_insert_with(|| {
            let written: HashSet<&str> = authored.iter().map(|(k, _)| *k).collect();
            let unwritten = declared.keys().copied().filter(|n| !written.contains(n)).collect();
            let containers = declared
                .iter()
                .filter(|(_, field)| field.is_variant_bearing())
                .map(|(&name, &field)| {
                    let value = authored.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
                    (name, field, field.selected_member(value))
                })
                .collect();
            (unwritten, containers)
        });
        let owner = variant_owner(key, containers);
        let suggestion = match owner {
            Some(_) => None,
            None => nearest_name(key, unwritten),
        };
        out.push(unknown_field_warning(&path, key, suggestion, owner));
    }
}

/// The unknown-key walk below one declared field, over the namespaces its value
/// opens: a typed dictionary, a matrix member, an array's elements, the live
/// world of a variant container. A key some other world declares is
/// `out_of_variant`'s, and a matrix key naming no member `enum_violation`'s.
fn collect_unknown_in(
    field: &FieldSchema,
    json: &serde_json::Value,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    if field.is_variant_bearing() {
        let Some(object) = json.as_object() else { return };
        let live = field.variant_fields(&field.selected_member(Some(json)));
        let declared: Declared = live
            .into_iter()
            .flatten()
            .map(|(n, f)| (n.as_str(), f.as_ref()))
            .collect();
        let authored: Vec<(&str, &serde_json::Value)> = object
            .iter()
            .filter(|(k, _)| *k != VARIANT_DISCRIMINANT_KEY)
            .filter(|(k, _)| {
                live.is_some_and(|f| f.contains_key(*k)) || field.variant_field(k).is_none()
            })
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        collect_unknown_keys(&declared, &authored, path, out);
        return;
    }
    if let (FieldType::Matrix { .. }, Some(members)) = (&field.r#type, field.namespace_props()) {
        let Some(object) = json.as_object() else { return };
        for (id, cell) in object {
            let (Some(member), Some(spelled)) =
                (members.get(id), super::config::matrix_member_spelling(cell))
            else {
                continue;
            };
            collect_unknown_in(member, &serde_json::Value::Object(spelled), &path.field(id), out);
        }
        return;
    }
    if let Some(props) = field.namespace_props() {
        let Some(object) = json.as_object() else { return };
        let declared: Declared = props.iter().map(|(n, f)| (n.as_str(), f.as_ref())).collect();
        let authored: Vec<(&str, &serde_json::Value)> =
            object.iter().map(|(k, v)| (k.as_str(), v)).collect();
        collect_unknown_keys(&declared, &authored, path, out);
        return;
    }
    if let (FieldType::Array, Some(items)) = (&field.r#type, &field.items) {
        // The floor wraps a bare value as the one element it lays out.
        match json.as_array() {
            Some(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    collect_unknown_in(items, element, &path.index(index), out);
                }
            }
            None => collect_unknown_in(items, json, &path.index(0), out),
        }
    }
}

/// The sibling variant container, and the member, whose world declares `key`:
/// the variant cell written beside its discriminant instead of under it. Each
/// container comes with its selected member, which wins where several worlds
/// declare the name.
fn variant_owner<'a>(
    key: &str,
    containers: &[Container<'a>],
) -> Option<(&'a str, &'a str)> {
    containers.iter().find_map(|(name, field, selected)| {
        let variants = field.variants.as_ref()?;
        let member = variants
            .get_key_value(selected.as_str())
            .filter(|(_, set)| set.contains_key(key))
            .or_else(|| variants.iter().find(|(_, set)| set.contains_key(key)))?
            .0;
        Some((*name, member.as_str()))
    })
}

/// The candidate `key` most likely misspells: the closest by edit distance,
/// ignoring case, within a third of the key's length (at least one edit). The
/// earliest declared wins a tie.
fn nearest_name<'a>(key: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let key_lower = key.to_lowercase();
    let len = key_lower.chars().count();
    let limit = (key.chars().count() / 3).max(1);
    candidates
        .iter()
        .map(|&c| (c.to_lowercase(), c))
        // Edit distance is at least the length difference.
        .filter(|(lower, _)| lower.chars().count().abs_diff(len) <= limit)
        .map(|(lower, c)| (edit_distance(&key_lower, &lower), c))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| c)
}

/// Optimal-string-alignment distance: insertions, deletions, substitutions,
/// and adjacent transpositions, each one edit.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut before: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(before[j - 2] + 1);
            }
        }
        before = std::mem::replace(&mut prev, cur);
    }
    prev[b.len()]
}

pub(crate) fn unknown_field_warning(
    path: &DocPath,
    field: &str,
    suggestion: Option<&str>,
    owner: Option<(&str, &str)>,
) -> Diagnostic {
    let path = path.to_string();
    let hint = match (owner, suggestion) {
        (Some((container, variant)), _) => format!(
            "`{field}` is a field of `{container}` when it is `{variant}`: nest it under \
             `{container}`, beside `value: {variant}`."
        ),
        (None, Some(suggestion)) => {
            format!("Did you mean `{suggestion}`? Rename the key to it, or remove the key.")
        }
        (None, None) => "Remove the key, or rename it to a field the quill declares.".to_string(),
    };
    let mut diag = Diagnostic::new(
        Severity::Warning,
        format!(
            "Field `{path}` is not declared by this quill: the value is kept, and no declared \
             field reads it."
        ),
    )
    .with_code("validation::unknown_field".to_string())
    .with_path(path)
    .with_arg("field", field.into())
    .with_hint(hint);
    if let Some(suggestion) = suggestion {
        diag = diag.with_arg("suggestion", suggestion.into());
    }
    if let Some((container, variant)) = owner {
        diag = diag
            .with_arg("container", container.into())
            .with_arg("variant", variant.into());
    }
    diag
}

fn quoted_kinds(kinds: &[&str]) -> String {
    kinds
        .iter()
        .map(|k| format!("`{k}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn unknown_card_warning(path: &DocPath, kind: &str, kinds: &[&str]) -> Diagnostic {
    let path = path.to_string();
    let hint = if kinds.is_empty() {
        "This quill declares no card kinds: remove the card.".to_string()
    } else {
        format!(
            "Set `$kind` to one of this quill's card kinds: {}.",
            quoted_kinds(kinds)
        )
    };
    Diagnostic::new(
        Severity::Warning,
        format!("Card `{path}` names kind `{kind}`, which this quill does not declare."),
    )
    .with_code("validation::unknown_card".to_string())
    .with_path(path)
    .with_arg("card", kind.into())
    .with_arg("allowed", kinds.into())
    .with_hint(hint)
}

pub(crate) fn body_disabled_warning(path: &DocPath, card: &str) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!(
            "Card `{card}` has body content at `{path}`, but the card kind declares \
             `body.enabled: false`: the body will not render."
        ),
    )
    .with_code("validation::body_disabled".to_string())
    .with_path(path)
    .with_arg("card", card.into())
    .with_hint("Remove the body content, or set `body.enabled: true` on the card kind.".to_string())
}

/// Report every authored cell that belongs to a variant the discriminant does
/// not select, across the main card and every composable card. The value stays
/// in the document and the diagnostic is non-fatal (`prose/canon/SCHEMAS.md`
/// §"Enum variants": carried, not dropped).
fn validate_variants(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (schema, card, path) in schema_cards(config, doc) {
        let Some(schema) = schema else { continue };
        collect_variant_diags(schema, card, &path, &mut diags);
    }
    diags
}

fn collect_variant_diags(
    schema: &CardSchema,
    card: &Card,
    base: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    let payload = card.payload();
    for (name, field) in &schema.fields {
        let Some(json) = payload.get(name).map(|v| v.as_json()) else {
            continue;
        };
        collect_stranded(field, json, &base.field(name), out);
    }
}

/// Every stranded cell in one field's subtree.
///
/// The descent is the placement rule read as a walk: a world opens at card level
/// or in a typed dictionary's property, so this passes through an `object`'s
/// declared properties and stops at the container it finds. Nothing below a
/// container can be one — an element, a column and a cell all carry the ban, and
/// an object inherits it — so a found container is the end of the path.
fn collect_stranded(
    field: &FieldSchema,
    json: &serde_json::Value,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    let Some(object) = json.as_object() else {
        return;
    };
    if field.is_variant_bearing() {
        let member = field.selected_member(Some(json));
        let live = field.variant_fields(&member);
        for key in object.keys() {
            if key == VARIANT_DISCRIMINANT_KEY || live.is_some_and(|f| f.contains_key(key)) {
                continue;
            }
            // A key no variant declares is `unknown_field`'s; only a key some
            // *other* world owns is stranded.
            let Some(owner) = field.variants.as_ref().and_then(|variants| {
                variants
                    .iter()
                    .find(|(_, set)| set.contains_key(key))
                    .map(|(member, _)| member.clone())
            }) else {
                continue;
            };
            out.push(out_of_variant_warning(&path.field(key), &owner, &member));
        }
        return;
    }
    if !matches!(field.r#type, FieldType::Object) {
        return;
    }
    for (name, prop) in field.properties.iter().flatten() {
        let Some(cell) = object.get(name) else {
            continue;
        };
        collect_stranded(prop, cell, &path.field(name), out);
    }
}

pub(crate) fn out_of_variant_warning(path: &DocPath, owner: &str, member: &str) -> Diagnostic {
    let path = path.to_string();
    let selected = if member.is_empty() {
        "left blank".to_string()
    } else {
        format!("`{member}`")
    };
    Diagnostic::new(
        Severity::Warning,
        format!(
            "Field `{path}` belongs to the `{owner}` variant, but the discriminant is {selected}: \
             the value is kept and will not render."
        ),
    )
    .with_code("validation::out_of_variant".to_string())
    .with_path(path)
    .with_arg("variant", owner.into())
    .with_arg("selected", member.into())
    .with_hint(format!(
        "Select `{owner}` to bring the field back into play, or remove the field to drop the \
         value."
    ))
}

/// Report every `array` the document fills past its declared `max:`, across the
/// main card and every composable card.
///
/// `max:` is page geometry: the element count past which the surplus leaves the
/// page the field is laid out on. A warning, never a gate — a
/// document over the limit renders, with the plate's own rule for the surplus.
fn validate_cardinality(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (schema, card, path) in schema_cards(config, doc) {
        let Some(schema) = schema else { continue };
        let payload = card.payload();
        for (name, field) in &schema.fields {
            collect_cardinality_diags(field, payload.get(name), &path.field(name), &mut diags);
        }
    }
    diags
}

/// Warn at each over-filled array `field` holds, at whatever depth: an array
/// nested in a typed dictionary, a matrix member, a live variant world, or
/// another array's elements is capped by its own declaration. The walk descends
/// only what the document authored, since an absent array has no count to
/// exceed.
fn collect_cardinality_diags(
    field: &FieldSchema,
    value: Option<&QuillValue>,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    let Some(json) = value.map(|v| v.as_json()).filter(|j| !j.is_null()) else {
        return;
    };

    if field.is_variant_bearing() {
        let Some(object) = json.as_object() else { return };
        let member = field.selected_member(Some(json));
        if let Some(fields) = field.variant_fields(&member) {
            for (name, schema) in fields {
                let cell = object.get(name).map(|j| QuillValue::from_json(j.clone()));
                collect_cardinality_diags(schema, cell.as_ref(), &path.field(name), out);
            }
        }
        return;
    }

    if let Some(props) = field.namespace_props() {
        let Some(object) = json.as_object() else { return };
        // An unticked matrix member reaches the page at its blanks, so nothing
        // it stores can overflow one.
        let matrix = matches!(field.r#type, FieldType::Matrix { .. });
        for (name, prop) in props {
            let Some(cell) = object.get(name) else { continue };
            let cell = match matrix {
                false => QuillValue::from_json(cell.clone()),
                true => {
                    if !is_held(prop, cell) {
                        continue;
                    }
                    match super::config::matrix_member_spelling(cell) {
                        Some(spelled) => {
                            QuillValue::from_json(serde_json::Value::Object(spelled))
                        }
                        None => continue,
                    }
                }
            };
            collect_cardinality_diags(prop, Some(&cell), &path.field(name), out);
        }
        return;
    }

    if !matches!(field.r#type, FieldType::Array) {
        return;
    }
    // The count is the floor's, not the document's: a bare scalar on an array
    // wraps to one element there, so `max: 0` sees the row it will lay out.
    let elements = match json.as_array() {
        Some(elements) => elements.clone(),
        None => vec![json.clone()],
    };
    if let Some(max) = field.max {
        if elements.len() > max as usize {
            out.push(cardinality_warning(path, max, elements.len()));
        }
    }
    if let Some(items) = &field.items {
        for (index, element) in elements.iter().enumerate() {
            let element = QuillValue::from_json(element.clone());
            collect_cardinality_diags(items, Some(&element), &path.index(index), out);
        }
    }
}

pub(crate) fn cardinality_warning(path: &DocPath, max: u32, actual: usize) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!(
            "Field `{path}` holds {actual} elements but the quill lays out at most {max}: \
             the surplus will not fit the page."
        ),
    )
    .with_code("validation::cardinality".to_string())
    .with_path(path)
    .with_arg("max", max.into())
    .with_arg("actual", actual.into())
    .with_hint(format!(
        "Remove {} element(s), or move the surplus onto another document.",
        actual.saturating_sub(max as usize)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quill::test_date;
    use serde_json::json;

    fn field(yaml: &str) -> FieldSchema {
        let value = QuillValue::from_yaml_str(yaml).unwrap();
        FieldSchema::from_quill_value("field".to_string(), &value).unwrap()
    }

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

        let resolved = resolve_value(Some(&input), &schema, test_date()).into_json();

        assert_eq!(
            resolved,
            json!({ "street": "1 Infinite Loop", "zip": 0, "note": "extra" })
        );
    }

    #[test]
    fn unclaimed_cards_and_bodies_render_and_warn() {
        let config = QuillConfig::from_yaml(
            r#"
quill: { name: uc, version: 1.0.0, backend: typst, description: x }
main:
  body:
    enabled: false
  fields:
    title: { type: string }
card_kinds:
  stamp:
    body:
      enabled: false
    fields:
      label: { type: string }
"#,
        )
        .expect("valid quill");
        let md = "~~~\n$quill: uc@1.0.0\n$kind: main\ntitle: T\n~~~\n\nRoot prose.\n\n\
                  ~~~\n$kind: stamp\nlabel: L\n~~~\n\nStamp prose.\n\n\
                  ~~~\n$kind: stamp\n~~~\n\n   \n\n\
                  ~~~\n$kind: ghost\nnote: g\n~~~\n\nGhost prose.\n";
        let doc = Document::parse(md).expect("parse").document;

        let plate = config.compile_data(&doc, test_date()).expect("unclaimed input renders");
        let cards = plate["$cards"].as_array().unwrap();
        assert_eq!(cards.len(), 3, "every card rides `$cards` in document order");
        assert_eq!(cards[2]["$kind"], "ghost");
        assert_eq!(cards[2]["note"], "g");
        assert!(cards[2].get("$body").is_none());

        let warned: Vec<(String, String)> = validate_unclaimed(&config, &doc)
            .into_iter()
            .inspect(|d| assert_eq!(d.severity, Severity::Warning, "{d:?}"))
            .map(|d| (d.code.unwrap(), d.path.unwrap()))
            .collect();
        let expected = [
            ("validation::body_disabled", "main.body"),
            ("validation::body_disabled", "cards.stamp[0].body"),
            ("validation::unknown_card", "cards[2]"),
        ]
        .map(|(c, p)| (c.to_string(), p.to_string()));
        assert_eq!(warned, expected, "a whitespace-only body is empty");
        assert!(config.validate_document(&doc).is_ok(), "nothing here is fatal");
    }

    #[test]
    fn undeclared_keys_warn_at_every_depth_with_the_likeliest_fix() {
        let config = QuillConfig::from_yaml(
            r#"
quill: { name: uk, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    secretary: { type: string }
    outcome:
      type: enum
      values: [motion, report]
      variants:
        motion:
          moved_by: { type: string }
        report:
          presenter: { type: string }
    address:
      type: object
      properties:
        street: { type: string }
    rows:
      type: array
      items:
        type: object
        properties:
          name: { type: string }
    quals:
      type: matrix
      members:
        flight_cc: Flight CC
      properties:
        detail: { type: string, default: "" }
card_kinds:
  item:
    fields:
      presenter: { type: string }
"#,
        )
        .expect("valid quill");
        let md = "~~~\n$quill: uk@1.0.0\n$kind: main\nsecretery: Grace Hopper\n\
                  outcome: { value: motion, presenter: P, extra: Z }\nmoved_by: Ada\n\
                  address: { stret: Main }\nrows:\n  - { nme: A }\n\
                  quals:\n  flight_cc: { held: true, detial: x }\n~~~\n\n\
                  ~~~\n$kind: item\npresentr: Alan Turing\n~~~\n";
        let doc = Document::parse(md).expect("parse").document;

        assert!(config.compile_data(&doc, test_date()).is_ok(), "an undeclared key renders");
        assert!(config.validate_document(&doc).is_ok(), "nothing here is fatal");

        let warned: Vec<(String, Option<String>, Option<String>)> =
            validate_unclaimed(&config, &doc)
                .into_iter()
                .inspect(|d| {
                    assert_eq!(d.severity, Severity::Warning, "{d:?}");
                    assert_eq!(d.code.as_deref(), Some("validation::unknown_field"));
                })
                .map(|d| {
                    let arg = |k: &str| d.args.get(k).and_then(|v| v.as_str()).map(String::from);
                    let hint = arg("suggestion").or_else(|| {
                        Some(format!("{}/{}", arg("container")?, arg("variant")?))
                    });
                    (d.path.unwrap(), arg("field"), hint)
                })
                .collect();
        let expected = [
            ("main.secretery", "secretery", Some("secretary")),
            // `presenter` belongs to the unselected world: `out_of_variant`'s.
            ("main.outcome.extra", "extra", None),
            ("main.moved_by", "moved_by", Some("outcome/motion")),
            ("main.address.stret", "stret", Some("street")),
            ("main.rows[0].nme", "nme", Some("name")),
            ("main.quals.flight_cc.detial", "detial", Some("detail")),
            ("cards.item[0].presentr", "presentr", Some("presenter")),
        ]
        .map(|(p, f, h)| (p.to_string(), Some(f.to_string()), h.map(String::from)));
        assert_eq!(warned, expected);
    }

    fn plate_of(yaml: &str, md: &str) -> serde_json::Value {
        let config = QuillConfig::from_yaml(yaml).expect("valid quill");
        let doc = Document::parse(md).expect("parse").document;
        config.compile_data(&doc, test_date()).expect("compile")
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

    #[test]
    fn absent_defaultless_enum_floors_to_the_blank() {
        let plate = plate_of(
            r#"
quill: { name: ev, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    classification:
      type: enum
      values: [UNCLASSIFIED, CUI, SECRET]
"#,
            "~~~card-yaml\n$quill: ev@1.0.0\n$kind: main\ntitle: T\n~~~\n",
        );
        assert_eq!(
            plate["classification"], "",
            "an unanswered enum renders its blank, never a variant nobody chose; got {plate}"
        );
    }

    #[test]
    fn nested_defaultless_enum_floors_to_the_blank() {
        let plate = plate_of(
            r#"
quill: { name: env, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
    marking:
      type: object
      properties:
        level:
          type: enum
          values: [UNCLASSIFIED, CUI]
        note: { type: string }
"#,
            "~~~card-yaml\n$quill: env@1.0.0\n$kind: main\ntitle: T\n~~~\n",
        );
        assert_eq!(
            plate["marking"],
            json!({ "level": "", "note": "" }),
            "the recursive blank switches for a nested enum too; got {plate}"
        );
    }

    /// A blank clears the gate on *every* enum, not only defaultless ones, so
    /// `values ∪ blank` is the surface a plate must branch over.
    #[test]
    fn an_authored_blank_enum_clears_the_gate_and_reaches_the_plate() {
        let plate = plate_of(
            r#"
quill: { name: eb, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    seal:
      type: enum
      values: [dow, dod]
      default: dow
"#,
            "~~~card-yaml\n$quill: eb@1.0.0\n$kind: main\nseal: \"\"\n~~~\n",
        );
        assert_eq!(
            plate["seal"], "",
            "an authored blank outranks the default and is not a gate error; got {plate}"
        );
    }

    #[test]
    fn an_absent_container_reaches_every_leaf_default() {
        const YAML: &str = r#"
quill: { name: ac, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        name: { type: string }
        email: { type: string, default: "hi@example.com" }
        addr:
          type: object
          properties:
            city: { type: string, default: Pgh }
"#;
        let absent = plate_of(YAML, "~~~card-yaml\n$quill: ac@1.0.0\n$kind: main\n~~~\n");
        assert_eq!(
            absent["contact"],
            json!({ "name": "", "email": "hi@example.com", "addr": { "city": "Pgh" } }),
            "an absent container cuts each cell's own ladder, at every depth; got {absent}"
        );
        // Authoring the empty map is a no-op: the same cells, the same rungs.
        let authored = plate_of(
            YAML,
            "~~~card-yaml\n$quill: ac@1.0.0\n$kind: main\ncontact: {}\n~~~\n",
        );
        assert_eq!(
            absent["contact"], authored["contact"],
            "writing `contact: {{}}` must not change what renders; got {authored}"
        );
    }

    /// A value the blueprint shows is the value that renders when the author
    /// leaves that line alone: the "shippable as-is" affordance, at any depth.
    #[test]
    fn the_blueprint_and_the_plate_agree_cell_by_cell() {
        const YAML: &str = r#"
quill: { name: bp, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    contact:
      type: object
      properties:
        email: { type: string, default: "hi@example.com" }
        when: { type: date, default: "2026-01-01" }
        addr:
          type: object
          properties:
            city: { type: string, default: Pgh }
"#;
        let blueprint = QuillConfig::from_yaml(YAML).expect("valid quill").blueprint();
        let plate = plate_of(YAML, "~~~card-yaml\n$quill: bp@1.0.0\n$kind: main\n~~~\n");
        for shown in ["hi@example.com", "2026-01-01", "Pgh"] {
            assert!(
                blueprint.contains(shown),
                "the blueprint shows {shown}: {blueprint}"
            );
        }
        assert_eq!(
            plate["contact"],
            json!({ "email": "hi@example.com", "when": "2026-01-01", "addr": { "city": "Pgh" } }),
            "and the plate renders exactly those cells; got {plate}"
        );
    }

    /// An `array` is a cell: `items:` fixes the element type but never the
    /// arity, so it keeps its own `default:`.
    #[test]
    fn an_array_default_completes_its_elements_against_items() {
        const YAML: &str = r#"
quill: { name: ad, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    rows:
      type: array
      default: [{ who: A }]
      items:
        type: object
        properties:
          who: { type: string }
          role: { type: string, default: lead }
"#;
        let defaulted = plate_of(YAML, "~~~card-yaml\n$quill: ad@1.0.0\n$kind: main\n~~~\n");
        assert_eq!(
            defaulted["rows"],
            json!([{ "who": "A", "role": "lead" }]),
            "a partial default element blank-fills against `items`; got {defaulted}"
        );
        let authored = plate_of(
            YAML,
            "~~~card-yaml\n$quill: ad@1.0.0\n$kind: main\nrows:\n  - who: A\n~~~\n",
        );
        assert_eq!(
            defaulted["rows"], authored["rows"],
            "which rung supplied the row does not change its shape; got {authored}"
        );
    }

    #[test]
    fn a_containers_rung_is_the_strongest_its_cells_contributed() {
        let defaulted = field(
            r#"
type: object
properties:
  name: { type: string }
  email: { type: string, default: "hi@example.com" }
"#,
        );
        assert_eq!(
            resolve_value_sourced(None, &defaulted, test_date()).1,
            FieldSource::Default,
            "a cell below took its `default:`, so the container is not at the floor"
        );

        let floored = field(
            r#"
type: object
properties:
  name: { type: string }
"#,
        );
        assert_eq!(
            resolve_value_sourced(None, &floored, test_date()).1,
            FieldSource::Blank,
            "nothing below the floor contributed, so the container reports it"
        );

        let authored = QuillValue::from_json(json!({}));
        assert_eq!(
            resolve_value_sourced(Some(&authored), &floored, test_date()).1,
            FieldSource::Authored,
            "a container the document wrote is authored, however little it holds"
        );
    }

    #[test]
    fn authored_blank_date_outranks_the_default() {
        let plate = plate_of(
            r#"
quill: { name: dz, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    signed_on:
      type: date
      default: "2026-01-01"
    subtitle:
      type: string
      default: "a default"
"#,
            "~~~card-yaml\n$quill: dz@1.0.0\n$kind: main\nsigned_on: \"\"\nsubtitle: \"\"\n~~~\n",
        );
        assert_eq!(
            plate["signed_on"], "",
            "the blank date survives coercion and outranks the default; got {plate}"
        );
        assert_eq!(
            plate["subtitle"], "",
            "the blank string does the same: one spelling of \"explicitly nothing\" for both"
        );
    }
}
