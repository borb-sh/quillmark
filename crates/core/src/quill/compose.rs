//! Consumer-facing operations on a [`Quill`]: validation, seeding, and the
//! blank-filled compile to backend wire JSON. Pure reads of the config.

use std::collections::HashSet;
use std::str::FromStr;

use indexmap::IndexMap;

use super::resolved::FieldSource;
use super::{
    seed, CardSchema, CoercionError, FieldSchema, FieldType, Leniency, Quill, QuillConfig,
    VARIANT_DISCRIMINANT_KEY,
};
use crate::normalize::{normalize_document, normalize_field_name};
use crate::quill::blank;
use crate::path::DocPath;
use crate::{
    Card, Diagnostic, Document, Payload, QuillValue, RenderError, SeedOverlay, Severity, Version,
};

impl Quill {
    /// [`QuillConfig::compile_data`] on this quill's config.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        self.config().compile_data(doc)
    }

    /// [`QuillConfig::compile_checked`] on this quill's config.
    pub fn compile_checked(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        self.config().compile_checked(doc)
    }

    /// Validate without backend compilation.
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
/// compiles data (e.g. a live session's `apply`) retain the config alone
/// rather than the whole quill with its font/package bytes.
impl QuillConfig {
    /// Applies coercion, validation, normalization, and **blank-filled render**:
    /// every absent schema field is resolved to its authored value, else its
    /// schema default, else the field's blank, in this plate-JSON
    /// projection only, never in the persisted document. A merely *incomplete*
    /// document compiles fine; only a *malformed* one (a value that won't
    /// coerce/validate) errors. A `!must_fill` placeholder never gates render:
    /// it surfaces as a non-fatal warning from `validate`. See
    /// `prose/canon/SCHEMAS.md`.
    pub fn compile_data(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        // The gate is the **one** coercion pass: `coerce_and_validate` conforms
        // every field (Render leniency, fallible) and validates, erroring on a
        // malformed document. The ladder below consumes its coerced, NFC-normalized
        // output rather than re-conforming: a document that reaches the ladder is
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
        // A card's `$body` is defined for the plate iff its kind resolves to a
        // body-enabled schema: the `$body` half of "absent on
        // undefined". Capture it here, where the schema is already in hand for
        // field lowering, and hand it to the plate builder, so the decision is
        // never re-derived from the serialized plate. (`$kind`, the document-
        // defined half, is gated structurally by `to_plate_json`.)
        let mut card_bodies: Vec<bool> = Vec::with_capacity(normalized.cards().len());
        let cards_resolved: Vec<Card> = normalized
            .cards()
            .iter()
            .map(|card| {
                let schema = self.card_kind(card.kind().unwrap_or(""));
                card_bodies.push(schema.is_some_and(|s| s.body_enabled()));
                let fields = match schema {
                    Some(schema) => {
                        plate_fields(ladder_sourced(schema, &card.payload().to_index_map()))
                    }
                    // Unknown-kind card: authored fields verbatim, no ladder, as
                    // the resolved-value view leaves it (`card_states`).
                    None => card.payload().to_index_map(),
                };
                Card::from_parts(rebuild_payload_with_meta(card, fields), card.body().clone())
            })
            .collect();

        Ok(Document::from_main_and_cards(final_main, cards_resolved)
            .to_plate_json_gated(self.main.body_enabled(), Some(&card_bodies)))
    }

    /// [`compile_data`](Self::compile_data) behind the `$quill` pairing check:
    /// the render door's whole preamble, in the one place that owns it. Every
    /// door that turns a document into plate data for *this* schema goes
    /// through here (`Quillmark::open` for a session's first compile,
    /// [`LiveSession::update`](crate::LiveSession::update) for each edit), so
    /// the pairing cannot be checked at one and skipped at the other.
    ///
    /// [`compile_data`](Self::compile_data) stays available unchecked for a
    /// caller that wants the plate alone (the CLI's `--output-data`), where no
    /// render follows and the pairing is the caller's to assert.
    pub fn compile_checked(&self, doc: &Document) -> Result<serde_json::Value, RenderError> {
        self.check_quill_reference(doc)?;
        self.compile_data(doc)
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
        // An incomplete document (absent fields or `!must_fill` placeholders)
        // renders fine via blank-fill. `validate_document` returns `Err` only
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
    /// (a different format, or an incompatible version of one) which yields
    /// undefined output, so it errors rather than warns.
    ///
    /// Every schema-bound door runs it, the bound ingestion
    /// ([`Quill::parse`](crate::Quill::parse) /
    /// [`Quill::conform`](crate::Quill::conform)) included, so the message names
    /// the pairing rather than a verb.
    ///
    /// Name is the prerequisite (a selector belongs to a *named* quill): a name
    /// mismatch (`quill::name_mismatch`) short-circuits and the version is left
    /// unevaluated; otherwise the selector is checked (`quill::version_mismatch`).
    /// The version parses infallibly in practice (validated at load); if it
    /// somehow doesn't, the version check is skipped.
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
    /// (an empty `Vec` when the document is valid).
    ///
    /// The editor-facing validation surface. Forwards the canonical
    /// `validation::*` diagnostics verbatim (same code, `path`, `hint`) so
    /// consumers route on the code without parsing message text: type
    /// mismatches, unknown card kinds, body-on-disabled-body, and the non-fatal
    /// warnings (`validation::must_fill`, `validation::out_of_variant`,
    /// `validation::example_unchanged`); the rest are blockers.
    ///
    /// **A blocker here means the document does not render, and a value the
    /// render floor refuses is a blocker here.** Values are judged in the form
    /// the floor builds from them (`conform_value` at `Leniency::Render`), so a
    /// bare scalar for an `array`, `"3"` for an `integer`, and a length-1 array
    /// for a `string` are valid, while a value the floor cannot conform is a
    /// `validation::type_mismatch` even where its authored shape reads
    /// well-typed. The leniencies are listed under `prose/canon/SCHEMAS.md`
    /// §"Type coercion".
    ///
    /// `validation::must_fill` has **two triggers**, covering disjoint failures
    /// under one code: a `!must_fill` marker the document carries
    /// (`validate_fills`), and a schema-side must-fill cell nobody authored
    /// (`validate_unauthored`). Neither subsumes the other — a document that
    /// never saw a blueprint carries no marker, and a seeded `example` is
    /// present, in-domain, and structurally indistinguishable from authored
    /// content — so both run, and the `trigger` arg tells a consumer which
    /// fired. Where both would fire on one path (a bare marker on an unauthored
    /// cell) the marker wins: its hint is the actionable one.
    ///
    /// `validation::example_unchanged` answers what `must_fill` cannot: *which*
    /// value a cell holds. The blueprint seats a defaultless field's `example:`
    /// in its value cell, so dropping the marker leaves a schema-valid,
    /// in-domain value nobody chose — `Duty Title` under a commander's name —
    /// and every other check reads it as authored content. A cell still
    /// carrying its marker is the marker's to report, by the same precedence:
    /// `must_fill` already names it, and the value it holds adds nothing.
    ///
    /// Field values, defaults, and presentation order are not part of this
    /// surface: read them from the [`Document`] payload and the quill schema
    /// (`quill.config().schema()`, whose key order is display order).
    pub fn validate(&self, doc: &Document) -> Vec<Diagnostic> {
        let mut diags = match self.config().validate_document(doc) {
            Ok(()) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
        };
        let marked = validate_fills(self.config(), doc);
        let claimed: HashSet<Option<String>> = marked.iter().map(|d| d.path.clone()).collect();
        diags.extend(marked);
        diags.extend(
            validate_unauthored(self.config(), doc)
                .into_iter()
                .filter(|d| !claimed.contains(&d.path)),
        );
        diags.extend(validate_variants(self.config(), doc));
        diags.extend(
            validate_examples(self.config(), doc)
                .into_iter()
                .filter(|d| !claimed.contains(&d.path)),
        );
        diags.extend(self.validate_seed(doc));
        diags
    }

    /// Advisory validation of the main card's `$seed` overlays.
    ///
    /// Seed overlays are editor-surface only: they never gate render
    /// (`compile_data` / `dry_run` ignore `$seed`), so every diagnostic here is
    /// a **warning** rooted at `$seed.<kind>[.<field>]`. An overlay keyed by a
    /// name that is not a declared `card_kind` is flagged; otherwise each
    /// overlaid field is checked against that kind's schema as the **document**
    /// value it is — an overlay cell is what `seed_card` commits into a new
    /// card — so the variant container is a spelling it accepts, a present-null
    /// cell reads as absent, and an omitted field raises nothing.
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
                for violation in super::validation::validate_field(field_schema, &qv, &field_path) {
                    diags.push(seed_violation_diagnostic(&violation));
                }
            }
        }
        diags
    }

    /// Seed a starter [`Document`]: the main card plus one instance of each
    /// declared composable card kind, each committing its fields' `example`
    /// values and leaving all other fields absent (interpolated at render:
    /// `default` → the field's blank). The committed, structured "filled-out" twin
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

/// The total (keep-raw) resolver behind [`Quill::resolve`](crate::Quill::resolve):
/// conform each authored value under Render leniency (keep-raw on failure, the
/// fallibility-free path a consumer-side view needs), NFC-normalize the key, then
/// cut the shared [`ladder_sourced`]. The render plate reaches the same rows by a
/// different route: its gate does the fallible conform, and `compile_data` hands
/// the coerced result straight to `ladder_sourced`, so the two cut one ladder
/// over equal input (a document that passes the gate never takes the keep-raw
/// branch), never a parallel precedence policy.
pub(crate) fn resolve_card_sourced(
    schema: &CardSchema,
    card: &Card,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    ladder_sourced(schema, &conform_card_render(schema, card))
}

/// Conform one card's authored fields under Render leniency, keep-raw on failure,
/// NFC-normalizing each key: the total (infallible) coercion the resolved-value
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

/// The shared sourced ladder both canon projections cut, the render-fidelity
/// plate ([`compile_data`](QuillConfig::compile_data)) and the resolved-value view
/// ([`Quill::resolve`](crate::Quill::resolve)), over an already-coerced,
/// NFC-normalized field map. For every declared field it reports the value the
/// render projection uses and the [`FieldSource`] rung that produced it; undeclared
/// authored fields carry through verbatim ([`Authored`](FieldSource::Authored)):
/// the schema is a floor, not an allowlist.
///
/// Field order is authored-first with declared-but-absent fields appended: the
/// render plate's order. Each projection re-cuts the presentation order it wants
/// from this one value-and-source map (the view rows declared fields first in
/// declaration order) rather than re-deriving the ladder against a parallel
/// precedence policy (`prose/canon/SCHEMAS.md` § "Value sources and projections").
/// Null ≡ absent applies recursively inside [`resolve_value_sourced`], so no bare
/// null reaches either projection.
pub(crate) fn ladder_sourced(
    schema: &CardSchema,
    coerced: &IndexMap<String, QuillValue>,
) -> IndexMap<String, (QuillValue, FieldSource)> {
    // Undeclared authored fields seed the map in authored order (verbatim,
    // Authored); the declared fields then overlay in place (or append when
    // absent) each carrying its ladder value and the source rung that produced
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
fn resolve_value(value: Option<&QuillValue>, field: &FieldSchema) -> QuillValue {
    resolve_value_sourced(value, field).0
}

/// Resolve one (possibly absent or null) value against its field schema,
/// reporting the [`FieldSource`] rung that produced it, and applying null ≡
/// absent recursively so no bare null reaches the plate.
///
/// **The ladder is per cell, and resolution is a descent rather than a return.**
/// A cell is a leaf, an `array` (arity is a fact no leaf carries), or a variant
/// discriminant; an `object` is a *namespace*, whose value is the composition of
/// its cells'. So this picks a **seed** — the authored value, else the schema
/// `default:` — and then hands it to [`compose`], which rebuilds a container from
/// its declared members whichever rung the seed came from, and floors a leaf at
/// its [`blank`]. Absence is *inherited*, not terminal: an absent container makes
/// every cell below it absent, and each cell then cuts its own ladder.
///
/// This is what makes the plate total at every depth: a declared address is
/// present however much of its container the document left out
/// (`prose/canon/PLATE_DATA.md`), which is what lets a plate read one directly
/// rather than through a guarded accessor.
///
/// The rung is the seed's, joined with what the descent found
/// ([`FieldSource::join`]). It is the byproduct of the same walk that computes
/// the value, so the render projection ([`resolve_value`]) and the
/// resolved-value view cut the one commitment ladder rather than each re-deriving
/// precedence (`prose/canon/SCHEMAS.md` § "Value sources and projections").
pub(crate) fn resolve_value_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
) -> (QuillValue, FieldSource) {
    if field.is_variant_bearing() {
        return resolve_variant_sourced(value, field);
    }
    let (seed, source) = match value.filter(|v| !v.as_json().is_null()) {
        Some(v) => (Some(v.clone()), FieldSource::Authored),
        None => match seed_default(field) {
            Some(default) => (Some(default), FieldSource::Default),
            None => (None, FieldSource::Blank),
        },
    };
    let (resolved, composed) = compose(seed.as_ref(), field, source);
    (resolved, source.join(composed))
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
/// seed, so an absent property resolves to *its* `default:` before its blank. A
/// seed key the schema does not declare passes through verbatim, matching
/// `config::coerce_object_props`: the schema is a floor, not an allowlist, so an
/// undeclared `note:` on a typed dict reaches the plate instead of being
/// silently dropped.
///
/// `seed_rung` is the rung that supplied `seed`, and it ceilings the cells'
/// ([`compose_members`]).
fn compose(
    seed: Option<&QuillValue>,
    field: &FieldSchema,
    seed_rung: FieldSource,
) -> (QuillValue, FieldSource) {
    match (&field.r#type, &field.properties, &field.items) {
        (FieldType::Object, Some(props), _) if composes_as(seed, serde_json::Value::is_object) => {
            let obj = seed.and_then(|v| v.as_json().as_object());
            let mut out = serde_json::Map::new();
            let rung = compose_members(obj, props, seed_rung, &mut out);
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
            (QuillValue::from_json(serde_json::Value::Object(out)), rung)
        }
        (FieldType::Array, _, Some(items)) if composes_as(seed, serde_json::Value::is_array) => {
            let arr = seed
                .and_then(|v| v.as_json().as_array().cloned())
                .unwrap_or_default();
            let out: Vec<serde_json::Value> = arr
                .into_iter()
                .map(|e| resolve_value(Some(&QuillValue::from_json(e)), items).into_json())
                .collect();
            (
                QuillValue::from_json(serde_json::Value::Array(out)),
                FieldSource::Blank,
            )
        }
        _ => match seed {
            Some(v) => (v.clone(), FieldSource::Blank),
            None => (blank(field), FieldSource::Blank),
        },
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
/// live world of a variant container — compose identically; only the seed and
/// the discriminant differ.
///
/// `seed_rung` is where the seed itself came from, and it **ceilings** its
/// members': a value the document did not write may not read
/// [`Authored`](FieldSource::Authored) one level down.
/// [`resolve_value_sourced`] cannot tell a seeded value from a written one, so
/// without the ceiling a cell fed from a container `default:` would report
/// itself authored.
fn compose_members(
    seed: Option<&serde_json::Map<String, serde_json::Value>>,
    members: &IndexMap<String, Box<FieldSchema>>,
    seed_rung: FieldSource,
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
        let (value, source) = resolve_value_sourced(cell.as_ref(), schema);
        rung = rung.join(source.capped_at(ceiling));
        out.insert(name.clone(), value.into_json());
    }
    rung
}

/// Resolve a variant-bearing enum into the container the plate receives:
/// `{value: <member>}` plus, when that member owns a field set, exactly that
/// set — each cell blank-filled by the ordinary ladder.
///
/// **The container is a closed shape.** Only the live world's fields cross, so a
/// value stranded by a discriminant flip and a key belonging to another variant
/// are both dropped here rather than shipped alongside a tag that disowns them.
/// That is what makes the plate contract total *per world*: inside the branch a
/// plate is already obliged to write over `values ∪ blank`
/// (`prose/canon/SCHEMAS.md` § "Blank-filled render"), every declared field of
/// that world is present, so no guarded access is needed; outside it, none is.
///
/// The discriminant cuts the same ladder as any enum (authored › `default:` ›
/// blank), but the container's own rung is the *cell's*: one the document wrote
/// reads authored whichever rung filled the tag, joined with what its live
/// world's cells contributed ([`compose_members`]) — the rule every namespace
/// follows (`prose/canon/SCHEMAS.md` § "The resolved-value view").
fn resolve_variant_sourced(
    value: Option<&QuillValue>,
    field: &FieldSchema,
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

/// Surface every `!must_fill` marker as a non-fatal **warning**, root-and-nested
/// across the main card and every composable card.
///
/// The marker fires whether or not the cell carries a suggested value, and never
/// gates render (the cell blank-fills or uses its suggested value). A strict
/// consumer treats any outstanding marker as "not done".
fn validate_fills(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (_, card, path) in schema_cards(config, doc) {
        collect_fill_diags(card, &path, &mut diags);
    }
    diags
}

/// Every card of `doc`, the main card first, with the schema its kind resolves
/// to (`None` for an undeclared kind) and the [`DocPath`] it is reported under.
///
/// A card whose declared `$kind` has no schema drops the kind segment and stays
/// `cards[<i>]`, matching `validate_typed_document`; a schema-declared kind
/// qualifies as `cards.<kind>[<i>]`.
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

pub(crate) fn fill_warning(path: &DocPath) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` is marked `!must_fill`: a placeholder awaiting a value."),
    )
    .with_code("validation::must_fill".to_string())
    .with_path(path)
    .with_arg("trigger", "marker".into())
    .with_hint(
        "Replace the value and drop the `!must_fill` marker, or remove the marker if the \
         current value is intended."
            .to_string(),
    )
}

/// Surface every schema-side must-fill cell the document leaves **unauthored**
/// as a non-fatal warning, across the main card and every composable card. The
/// schema half of `validation::must_fill`, reaching documents that carry no
/// marker to read.
///
/// Unauthored is **absent-or-null**, never [`FieldSource`]: the rung is one per
/// top-level field, so a present typed dict reports `Authored` as a whole
/// ([`resolve_value_sourced`]) and a source-keyed check goes silent on a
/// must-fill property inside it.
fn validate_unauthored(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (schema, card, path) in schema_cards(config, doc) {
        let Some(schema) = schema else { continue };
        collect_unauthored_diags(schema, card, &path, &mut diags);
    }
    diags
}

fn collect_unauthored_diags(
    schema: &CardSchema,
    card: &Card,
    base: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    let payload = card.payload();
    for (name, field) in &schema.fields {
        collect_unauthored_field(field, payload.get(name), &base.field(name), out);
    }
}

/// Warn at each **cell** the schema obliges and the document leaves unauthored.
/// Cells sit where the blueprint stamps its markers (`prose/canon/BLUEPRINT.md`
/// § "Placeholder value precedence"), so the two triggers speak about the same
/// paths:
///
/// - A **typed dictionary** is never itself a cell: `!must_fill` is rejected on
///   a mapping (`prose/references/markdown-spec.md` §3.4). Recursion runs
///   present or absent, so an absent `address` warns at `address.street`, a path
///   an editor can resolve and a marker can occupy.
/// - Every **other** type is the cell, the array included, so `[]` is an
///   authored answer. A present array resolves its elements against the item
///   schema; an absent one has no index to anchor on and warns at the container.
fn collect_unauthored_field(
    field: &FieldSchema,
    value: Option<&QuillValue>,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    // A variant container is not itself a cell, for the reason a typed dictionary
    // is not: `!must_fill` is rejected on a mapping. Its cells are the
    // discriminant and — *only in the world the discriminant selects* — that
    // world's fields. This is where obligation becomes conditional: a `poc` with
    // no `default:` is obliged on a CUI memo and silent on every other one, which
    // is the thing `must_fill` alone cannot say.
    if field.is_variant_bearing() {
        let json = value.map(|v| v.as_json());
        let object = json.and_then(|j| j.as_object());
        // Pre-coercion the cell may still be the bare scalar; read either.
        let authored = FieldSchema::authored_member(json);

        let discriminant = path.field(VARIANT_DISCRIMINANT_KEY);
        if authored.is_none() && field.must_fill() {
            out.push(unauthored_warning(&discriminant));
        }
        let member = field.selected_member(json);

        if let Some(fields) = field.variant_fields(&member) {
            for (name, schema) in fields {
                let cell = object
                    .and_then(|o| o.get(name))
                    .map(|j| QuillValue::from_json(j.clone()));
                collect_unauthored_field(schema, cell.as_ref(), &path.field(name), out);
            }
        }
        return;
    }

    if let (FieldType::Object, Some(props)) = (&field.r#type, &field.properties) {
        let obj = value.and_then(|v| v.as_json().as_object());
        for (name, prop) in props {
            let pv = obj
                .and_then(|o| o.get(name))
                .map(|j| QuillValue::from_json(j.clone()));
            collect_unauthored_field(prop, pv.as_ref(), &path.field(name), out);
        }
        return;
    }

    let Some(present) = value.filter(|v| !v.as_json().is_null()) else {
        if field.must_fill() {
            out.push(unauthored_warning(path));
        }
        return;
    };

    if let (FieldType::Array, Some(items)) = (&field.r#type, &field.items) {
        for (index, element) in present.as_json().as_array().into_iter().flatten().enumerate() {
            let element = QuillValue::from_json(element.clone());
            collect_unauthored_field(items, Some(&element), &path.index(index), out);
        }
    }
}

/// Report every authored cell that belongs to a variant the discriminant does
/// not select, across the main card and every composable card.
///
/// **Carried, not dropped.** The value stays in the document and the diagnostic
/// is non-fatal, because the alternative punishes the ordinary editor gesture:
/// choose CUI, fill the block, flip to UNCLASSIFIED to compare, flip back. A
/// coercion-time drop would spend the author's answers on that flip; gating
/// render would hand them an undraftable document. So the wire stays honest —
/// the render floor emits only the live world — and the document keeps what a
/// human typed until a human clears it.
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
        if !field.is_variant_bearing() {
            continue;
        }
        let Some(json) = payload.get(name).map(|v| v.as_json()) else {
            continue;
        };
        let Some(object) = json.as_object() else {
            continue;
        };
        let member = field.selected_member(Some(json));
        let live = field.variant_fields(&member);
        for key in object.keys() {
            if key == VARIANT_DISCRIMINANT_KEY || live.is_some_and(|f| f.contains_key(key)) {
                continue;
            }
            // A key no variant declares is an undeclared field, which every
            // other surface carries without comment; only a key some *other*
            // world owns is the stranded case worth naming.
            let Some(owner) = field.variants.as_ref().and_then(|variants| {
                variants
                    .iter()
                    .find(|(_, set)| set.contains_key(key))
                    .map(|(member, _)| member.clone())
            }) else {
                continue;
            };
            out.push(out_of_variant_warning(
                &base.field(name).field(key),
                &owner,
                &member,
            ));
        }
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

pub(crate) fn unauthored_warning(path: &DocPath) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` must be filled in: nobody has authored a value."),
    )
    .with_code("validation::must_fill".to_string())
    .with_path(path)
    .with_arg("trigger", "unauthored".into())
    .with_hint(
        "Author a value. To record that empty is the intended answer, write the field's \
         blank explicitly rather than leaving it out."
            .to_string(),
    )
}

/// Surface every cell and body the document leaves at the value its schema
/// *shows*, across the main card and every composable card.
///
/// The blueprint seats a defaultless field's `example:` in the value cell under
/// the `!must_fill` marker (`prose/canon/BLUEPRINT.md` § "Placeholder value
/// precedence"), and the marker is the only thing distinguishing it from an
/// answer: an example is present, type-valid and in-domain, so coercion,
/// validation and the unauthored predicate all read it as content. Drop the
/// marker and a memo ships `Duty Title` under a signature block with a clean
/// validate. This walk reads the same declarations the blueprint stamps from,
/// and says the cell still holds what the schema showed.
///
/// It is about *which* value, never *whether* one was authored, so it declines
/// the cells [`validate_unauthored`] owns: a field declaring no `example:` has
/// nothing to compare against and is silent here whatever it holds.
fn validate_examples(config: &QuillConfig, doc: &Document) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (schema, card, path) in schema_cards(config, doc) {
        let Some(schema) = schema else { continue };
        let payload = card.payload();
        for (name, field) in &schema.fields {
            collect_example_field(field, payload.get(name), &path.field(name), &mut diags);
        }
        collect_body_example(schema, card, &path, &mut diags);
    }
    diags
}

/// Warn at each cell whose value is the one its own declaration shows.
///
/// The recursion is [`collect_unauthored_field`]'s, for the same reason: the
/// cells it reaches are where the blueprint stamps its values. A **typed
/// dictionary** and a **variant container** are namespaces, so each member is
/// judged against its own `example:`; an **array** is judged element-wise
/// against the same index of the array literal its field shows, so an author who
/// edited the first line and left the second still hears about the second.
/// Absence is not this walk's business: an absent cell holds no value to
/// recognize.
fn collect_example_field(
    field: &FieldSchema,
    value: Option<&QuillValue>,
    path: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    let Some(present) = value.filter(|v| !v.as_json().is_null()) else {
        return;
    };
    let json = present.as_json();

    if field.is_variant_bearing() {
        let authored = FieldSchema::authored_member(Some(json));
        let shown = field
            .example
            .as_ref()
            .and_then(|e| FieldSchema::authored_member(Some(e.as_json())));
        if let Some(shown) = shown.filter(|s| Some(*s) == authored) {
            out.push(example_unchanged_warning(
                &path.field(VARIANT_DISCRIMINANT_KEY),
                shown,
            ));
        }
        if let Some(fields) = field.variant_fields(&field.selected_member(Some(json))) {
            for (name, schema) in fields {
                let cell = json
                    .as_object()
                    .and_then(|o| o.get(name))
                    .map(|j| QuillValue::from_json(j.clone()));
                collect_example_field(schema, cell.as_ref(), &path.field(name), out);
            }
        }
        return;
    }

    if let (FieldType::Object, Some(props)) = (&field.r#type, &field.properties) {
        for (name, prop) in props {
            let pv = json
                .as_object()
                .and_then(|o| o.get(name))
                .map(|j| QuillValue::from_json(j.clone()));
            collect_example_field(prop, pv.as_ref(), &path.field(name), out);
        }
        return;
    }

    if let (FieldType::Array, Some(items)) = (&field.r#type, &field.items) {
        // The array's own `example:` is the whole literal, so the element's
        // counterpart is the one at its index; an element that is not the shown
        // one still descends, since a typed table's cells carry examples of
        // their own.
        let shown = field.example.as_ref().and_then(|e| e.as_json().as_array());
        for (index, element) in json.as_array().into_iter().flatten().enumerate() {
            let at = path.index(index);
            match shown.and_then(|eg| eg.get(index)).filter(|eg| *eg == element) {
                Some(eg) => out.push(example_unchanged_warning(&at, eg)),
                None => collect_example_field(
                    items,
                    Some(&QuillValue::from_json(element.clone())),
                    &at,
                    out,
                ),
            }
        }
        return;
    }

    if let Some(shown) = shown_example(field, json) {
        out.push(example_unchanged_warning(path, shown.as_json()));
    }
}

/// The `example:` `value` is still sitting at, `None` where the field declares
/// none or the document wrote something else. A prose field shows its example
/// twice — the markdown literal a blueprint inlines and the imported content a
/// seed commits ([`FieldSchema::example_content`]) — and they are one
/// declaration, so either match reports the declaration.
fn shown_example<'a>(field: &'a FieldSchema, value: &serde_json::Value) -> Option<&'a QuillValue> {
    let example = field.example.as_ref()?;
    let matched = example.as_json() == value
        || field
            .example_content
            .as_ref()
            .is_some_and(|content| content.as_json() == value);
    matched.then_some(example)
}

/// Warn when a card's body is the text the blueprint writes into it: the
/// configured `body.example`, or the placeholder generated for a kind that
/// declares none ([`super::blueprint::body_placeholder`]).
///
/// The comparison is against the markdown projection, the form the blueprint
/// emitted and an author edits, re-imported so a body that only round-tripped
/// through the content model (`__b__` → `**b**`) still counts as untouched.
fn collect_body_example(
    schema: &CardSchema,
    card: &Card,
    base: &DocPath,
    out: &mut Vec<Diagnostic>,
) {
    if !schema.body_enabled() {
        return;
    }
    let authored = card.body_markdown();
    let authored = authored.trim();
    if authored.is_empty() {
        return;
    }
    let shown = match schema.body.as_ref().and_then(|b| b.example.as_deref()) {
        Some(example) => example.to_string(),
        None => super::blueprint::body_placeholder(&schema.name),
    };
    if authored == shown.trim() || canonical_markdown(&shown).as_deref() == Some(authored) {
        out.push(body_example_unchanged_warning(&base.body(), shown.trim()));
    }
}

/// `shown` through the content model and back, the round trip a body takes on
/// its way into a document. `None` where the text does not import, which a
/// generated placeholder and a load-validated example never do.
fn canonical_markdown(shown: &str) -> Option<String> {
    let content = crate::document::import_body(shown).ok()?;
    Some(quillmark_content::export::to_markdown(&content).trim().to_string())
}

pub(crate) fn example_unchanged_warning(path: &DocPath, example: &serde_json::Value) -> Diagnostic {
    let path = path.to_string();
    let literal = serde_json::to_string(example).unwrap_or_else(|_| "null".to_string());
    Diagnostic::new(
        Severity::Warning,
        format!("Field `{path}` still holds the schema's example value `{literal}`."),
    )
    .with_code("validation::example_unchanged".to_string())
    .with_path(path)
    .with_arg("example", example.clone())
    .with_arg("trigger", "field".into())
    .with_hint(
        "Author the value this document means. An `example:` shows a cell's shape, never its \
         content."
            .to_string(),
    )
}

pub(crate) fn body_example_unchanged_warning(path: &DocPath, example: &str) -> Diagnostic {
    let path = path.to_string();
    Diagnostic::new(
        Severity::Warning,
        format!("Body `{path}` still holds the text the blueprint wrote there."),
    )
    .with_code("validation::example_unchanged".to_string())
    .with_path(path)
    .with_arg("example", example.into())
    .with_arg("trigger", "body".into())
    .with_hint(
        "Write the body this document means, in place of the example or placeholder text."
            .to_string(),
    )
}

#[cfg(test)]
mod must_fill_tests;

#[cfg(test)]
mod example_unchanged_tests;

#[cfg(test)]
mod tests {
    use super::*;
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

        let resolved = resolve_value(Some(&input), &schema).into_json();

        assert_eq!(
            resolved,
            json!({ "street": "1 Infinite Loop", "zip": 0, "note": "extra" })
        );
    }

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
        let main = Card::from_parts(main, quillmark_content::Normalized::empty());

        let mut unknown = Card::new("mystery").unwrap();
        unknown
            .store_fill("note", QuillValue::from_json(json!(null)))
            .unwrap();

        let mut kindless =
            Card::from_parts(Payload::new(), quillmark_content::Normalized::empty());
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
            resolve_value_sourced(None, &defaulted).1,
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
            resolve_value_sourced(None, &floored).1,
            FieldSource::Blank,
            "nothing below the floor contributed, so the container reports it"
        );

        let authored = QuillValue::from_json(json!({}));
        assert_eq!(
            resolve_value_sourced(Some(&authored), &floored).1,
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
