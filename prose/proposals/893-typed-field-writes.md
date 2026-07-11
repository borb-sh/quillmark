# Typed field writes: schema-carried types, one commit dispatch

> **Issue**: [#893](https://github.com/quillmark-org/quillmark/issues/893) ·
> **Status**: proposal — for discussion, not committed design. Reviewed
> against the codebase; the open questions are resolved at the end.
> Symbol references are accurate on `integration/richtext` at time of writing.

## TL;DR

Make the schema — not the value, and not the method name — carry the field's
type at the write site, via three layers:

1. **One per-type dispatch** in core (`commit_value`), unifying
   `coerce_value_strict`'s arms with the strict write path behind a leniency
   mode — the single *write-side* encoding of "what a type accepts"
   (validation keeps its own read-only dispatch, synced as today).
2. **One typed writer per address**: `Card::commit_field(name, value,
   &FieldSchema)` — subsumes `set_field_richtext` and never grows a per-type
   sibling.
3. **A schema-bound editor** as the front door: `quill.editor(&mut doc)`
   resolves field types itself, so editors and MCP servers call one verb,
   `set`, and never pass `inline` or a type token.

The write surface stays **O(1) in corpus-backed types**: a new content model
(e.g. `Plaintext`) touches the `FieldType` enum and one dispatch arm — no new
methods in core, wasm, or Python. `QuillValue` stays JSON-shaped; the wire is
unchanged.

## The observation

The `O(types × addresses)` sprawl the issue projects comes from encoding the
type in the **method name** (`set_field_richtext`, `updateCardRichtextField`,
…). The issue's own forces point at the alternative carrier: the schema is
the type's only authoritative home (`FieldType` in
`crates/core/src/quill/types.rs`, including constraints like `inline` that no
value tag could carry), and the value-*transforming* dispatch on it already
lives in one place (`QuillConfig::coerce_value_strict`,
`crates/core/src/quill/config.rs`; `validation::validate_value` is a second,
read-only dispatch, kept in lockstep via shared helpers — `scalar_as_string`,
`decode_richtext_value` — and stays separate). So route writes through that
same transforming dispatch, parameterized by who supplies the schema — rather
than inventing a second type carrier (value tags) or a heuristic (shape
detection).

## Layer 1 — one commit dispatch

Extract the per-type logic into a single function beside the coercion code:

```rust
enum Leniency { Render, Write }   // Render = today's lenient cascade; Write = strict

/// The one encoding of "what a type accepts and what gets stored".
fn commit_value(value: &QuillValue, schema: &FieldSchema, mode: Leniency)
    -> Result<QuillValue, CommitError>;
```

- `coerce_value_strict` becomes a thin caller with `Leniency::Render`;
  behavior unchanged.
- The richtext arm's `mode` branch replaces the two open-coded sites that
  today encode "what a richtext value accepts": `Write` uses
  `decode_richtext_value` semantics (corpus object or markdown string only —
  no scalar→string reduction), `Render` keeps the `lenient_string` cascade,
  exactly the split the arm's own comment documents against `edit.rs`
  `set_field_richtext`.
- The mode axis is **dispatch-wide, not richtext-local**. The lenient cascade
  extends past the richtext arm: `Array` wraps a bare scalar into a
  singleton, `DateTime` maps `""` → null, `Boolean`/`Number` cross-coerce
  (`true` → `1`, `1` → `true`), and — critically — several arms *fall
  through unchanged* wherever coercion defers to the validation layer
  (`String` and `RichText` on a non-stringifiable shape, `Object` on a
  non-object). `Write` mode keeps the value-parsing normalizations
  (`"3"` → `3`, scalar → `[scalar]` — canonicalize-now includes them) but
  turns every defer-to-validation fall-through into a `CommitError`;
  otherwise `commit` silently stores a mismatched value (e.g. `42` into an
  `object` field) and the "fail now" promise breaks exactly where scalars
  are concerned. Whether the cross-type `Boolean`↔`Number` coercions survive
  in `Write` mode is a per-arm call to make during extraction; the rule is
  that each arm's `Write` contract is explicit, not inherited by accident.
- Each `FieldType` arm declares a **storage policy**: *canonical-at-write*
  (richtext and future corpus models — the committed form carries identity
  marks) vs *coerce-at-render* (scalars — a typed write stores the coerced
  canonical, but nothing forces scalars through this path; plain `set_field`
  stays opaque). Null passes through, matching the null ≡ absent rule (a
  divergence from `set_field_richtext`, which stores the empty corpus for
  null — the deprecated wrapper keeps that pin, see Layer 2).

## Layer 2 — one typed writer per address

```rust
impl Card {
    /// Write-time commit: validate/normalize per the field's schema type and
    /// store the canonical form. The typed sibling of the opaque `set_field`.
    pub fn commit_field(&mut self, name: &str, value: impl Into<QuillValue>,
                        schema: &FieldSchema) -> Result<(), EditError>;
}
```

- `set_field_richtext(name, value, inline)` reduces to a deprecated wrapper
  (builds a `richtext { inline }` `FieldSchema`, delegates). Its
  `inline: bool` parameter disappears from the API: the schema *is* the
  parameter. The wrapper keeps two behavior pins: it stores the empty corpus
  for `null` (where `commit_field` passes null through, per the null ≡
  absent rule), and it maps `CommitError` back to the existing
  `EditError::FieldRichtextDecode` / `FieldRichtextNotInline` variants — the
  wasm and Python error mappers key on `variant_name`, and binding tests
  match those message shapes.
- The verb `commit` matches the issue's vocabulary ("write-time commit") and
  names the two write disciplines: **`set_field` = store opaque, coerce at
  render** (keystroke-level state, data-in-flight) vs **`commit_field` =
  canonicalize now, fail now** (editor blur/save, agent writes). Neither
  discipline is forced on the other.
- `apply_field_richtext_change` (delta splicing) stays per-model by nature:
  incremental change bundles are content-model-native ops, not field writes,
  and a future `Plaintext` model brings its own delta vocabulary. It does not
  count against the O(1) goal.

## Layer 3 — the schema-bound editor

`commit_field` still asks the caller to fetch a `FieldSchema` per call. Every
consumer that wants typed writes (editor, MCP server) already holds the
resolved `Quill`/`QuillConfig` — it renders with it. Bind once:

```rust
let mut ed = quill.editor(&mut doc);   // TypedEditor<'a>: &mut Document + &QuillConfig
ed.set("subject", "Q3 results")?;      // schema: richtext(inline) → strict corpus commit
ed.set("qty", "3")?;                   // schema: integer → strict coerce, stores 3
ed.card(2)?.set("desc", corpus_json)?; // card kind → CardSchema → field type
ed.set_all([...])?;                    // batched, all-or-nothing, mirrors set_fields
```

- A field absent from the schema stores opaquely (mirrors `coerce_payload`'s
  passthrough for unknown fields; an unknown card kind degrades the same way,
  mirroring `coerce_card`; the field bag stays a bag). The editor should say
  which happened (e.g. `set` returns `Committed::Typed | Opaque`): unlike the
  render path's bag, an editor's caller usually meant a schema field, and a
  typo'd name silently storing opaque is otherwise invisible until
  validation.
- This answers the "Document lacks the resolved schema" force at the editor
  boundary — binding once, not a tag on every value, not a parameter on
  every call.

## Per-consumer surfaces

- **WASM editor** — the wasm `Quill` exposes a `schema` getter already; pass
  the handle per call (wasm-bindgen cannot hold a long-lived `&mut Document`
  inside a bound object): `doc.commitField(quill, name, value)` and
  `doc.commitCardField(quill, index, name, value)`. Two methods total for all
  current and future types; `updateCardRichtextField` deprecates into them.
  Values stay in the wire encoding the seam already speaks (corpus object |
  markdown string | scalar) — no new envelope.
- **Python** — `card.commit_field(name, value, schema)` is net-new surface:
  Python today has no richtext field writer at all (only
  `set_field`/`set_fill`/`set_fields`), so nothing deprecates. A bound
  `quill.editor(doc)` cannot hold `TypedEditor<'a>`'s `&mut Document` (pyo3
  classes carry no lifetimes); it would hold a `Py<Document>` and borrow per
  call, or Python ships the per-call form only. Slots into the
  [PROGRAMMATIC.md](../canon/PROGRAMMATIC.md) flow.
- **MCP servers / LLM agents** — an LLM patches a field with a markdown
  string; the commit imports it to corpus at write, so identity marks live on
  the stored value from that moment, with no per-type tool schema. The MCP
  tool surface stays `set(field, value)`; `get_specs` already tells the model
  the types.
- **Batch generators (DB row → PDF)** — unchanged: opaque `set_field` +
  render coercion; they never pay the typed path.
- **Storage / diff / sync tooling** — commit-at-write makes the stored form
  canonical and stable, so DTO diffs are semantic rather than encoding noise,
  and patch/CRDT layers converge on one representation.
- **Read-back parity** — the same dispatch can later grow a `project`
  operation (richtext → markdown string, plaintext → text), generalizing
  `field_markdown` into one schema-keyed `ed.get(name)` instead of per-type
  projections. Same O(1) argument; can ship separately.

## Rejected shapes

- **Value self-identification (type tag on `QuillValue`)** — breaks the
  "JSON-shaped, nothing more" invariant with crate-wide blast radius,
  complicates wire/DTO/`.qmd` round-trips and schema-less load, and still
  cannot carry schema-side constraints like `inline` — the schema gets
  consulted anyway.
- **Shape auto-detection** — ambiguous the moment a second corpus model
  exists (two canonical corpora are both objects; disambiguating requires a
  tag, see above). `decode_richtext_value`'s object-vs-string dispatch
  survives *inside* the richtext commit arm, as a codec detail rather than a
  type oracle.
- **Per-type writers** — the `O(types × addresses)` future the issue exists
  to avoid; every new writer also re-encodes the strict/lenient split by
  hand.

## Growth story (the acceptance test)

Adding a `Plaintext` corpus type = one `FieldType` variant + one
`commit_value` arm + the per-type companions every `FieldType` already owes
(`validate_value` arm, `type_name` mapping, blueprint/transform-schema
annotations) (+ optionally one `project` arm; + its own delta ops if
incremental editing is wanted). The O(1) claim is about methods per address:
no new methods on `Card`, `Document`, the wasm surface, the Python surface,
or any MCP tool schema.

## Migration

1. Extract `commit_value` from `coerce_value_strict`; coercion calls it with
   `Leniency::Render` (behavior-preserving; existing coercion tests and the
   `coerce_fuzz` properties pin it). Extend the fuzz harness to
   `Leniency::Write` — same no-panic/idempotence/error-quality properties,
   plus `commit ∘ commit = commit`.
2. Add `Card::commit_field`; reimplement `set_field_richtext` over it and
   deprecate.
3. Add `TypedEditor` (core) and the binding methods; deprecate
   `updateCardRichtextField` / `setFieldRichtext` in wasm.
4. (Separate, optional) the `project` read-back generalization.

## Open questions — resolved

- **Verb and type names: keep `commit_field`; rename the shared dispatch.**
  "Commit" is already the codebase's word for write-time canonicalization
  (`set_field_richtext` "commits the corpus form at write"; seeding and the
  render floor "commit" cached corpora). But the shared dispatch is not a
  commit in `Leniency::Render` mode — name the function for the operation
  (e.g. `conform_value(value, schema, mode)`; the Layer-1 sketch's
  `commit_value` reads better as this) and let `commit_field` own the verb.
  `TypedEditor` over `SchemaWriter`: the type is slated to grow read-back
  (`ed.get`), which "Writer" forecloses; the binding surfaces mirror verbs,
  not the Rust type name, so the stakes are low.
- **A typed scalar write stores the coerced canonical.** Three grounds.
  Stored == rendered: coercion is idempotent (fuzz-pinned, `coerce_fuzz.rs`
  property T3), so a committed field is a fixed point of the render path.
  Policy uniformity: the richtext commit already re-canonicalizes even an
  already-canonical corpus object; storing authored scalar forms would give
  the one dispatch two storage policies. And the storage/diff/sync payoff
  exists only for canonical storage. Authored-form fidelity (a text box
  holding `"3.10"` for a number field) belongs on the opaque path until
  blur — that is the `set_field` / `commit_field` discipline split, not a
  reason to weaken the commit.
- **No `commit_fill`; fill stays on the opaque path.** A `!must_fill` value
  is a placeholder documenting shape, not endorsed content — committing is
  what *clears* fill (every payload `insert` drops the marker).
  Mechanically, the dispatch operates on the fill-free `as_json` projection
  and rebuilds values via `from_json`, so nested fill markers would not
  survive it (only the null passthrough preserves one, per the note in
  `coerce_value_strict`). And a committed richtext fill would emit
  `!must_fill` followed by a canonical corpus object in card-yaml —
  destroying the markdown placeholder `blueprint()` exists to produce for
  humans and LLMs.
- **No engine-side resolution in wasm.** Nothing in the stack resolves a
  quill from a document's `$quill` reference: the Rust engine registers
  *backends* only and takes the quill explicitly (`Quillmark::open(&self,
  quill, doc)`), and the wasm surface mirrors it (`render(doc, quill)`).
  `engine.commitField(doc, …)` would require a quill registry that does not
  exist and add a second write path with version-skew failure modes (the
  doc's ref vs. whatever the registry holds). Explicit quill handle only; if
  a registry ever lands, the sugar can follow it.
