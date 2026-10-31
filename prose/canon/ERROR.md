# Error Handling System

> **Implementation**: `crates/core/src/`

## TL;DR

Every failure travels as a `Diagnostic`: severity, namespaced `code`, message,
optional text `location` and document-model `path`. `RenderError` carries a
non-empty `Vec<Diagnostic>` and has no failure taxonomy beyond them:
consumers route on codes, not types. Warnings ride the same currency and
never block.

## Types

**`Severity`**: `Error` | `Warning`. Fatality is this two-value ladder and
nothing else: `Error` blocks the stage that emits it, `Warning` never does.
There is no lint-level configuration and no warning-to-error promotion; an
informational aside is a `hint` on the diagnostic it annotates, not a
severity.

**`Location`**: file name, line (1-indexed), column (1-indexed)

**`Diagnostic`**: severity, optional error `code`, `message`, optional `location` (text anchor: file/line/column), optional `path` (document-model anchor, dotted/bracketed path into the typed `Document`, set by schema validation/coercion), optional `hint`. `location` and `path` are independent and may co-exist.

**`ParseError`**: parsing-stage error enum, `InputTooLarge`, `TooManyFields`, `TooManyCards`, `InvalidStructure`, `EmptyInput`, `MissingQuill`, `InvalidQuillReference`, `BodyImport`, `PayloadNotMapping`, `MissingKind`, `YamlErrorWithLocation`; converts to `Diagnostic` via `to_diagnostic()`. The `InvalidQuillReference` case (`parse::invalid_quill_reference`) attaches the canonical `$quill` grammar (`quill_ref_hint()`) as the diagnostic hint. That hint is the single source of truth for the reference grammar: bindings surface it verbatim (e.g. WASM `Document.quillRefHint`) rather than re-stating the rule.

The `PayloadNotMapping` case (`parse::payload_not_mapping`) is a card-yaml payload that is YAML but not a mapping, usually tilde-fenced code. It locates at the block's opening fence, and its hint names the backtick fence, spelled with the opener's info string.

The `BodyImport` case (`parse::body_import`) is a body nested past the bound (markdown spec §8), which the bound sets so that every body that imports renders under Typst. It carries the body's `path`, `main.body` or `cards.<kind>[i].body`.

The `MissingKind` case (`parse::missing_kind`) is a mapping payload after the root that names no `$kind`: every block after the root is a card, and a card names its kind. The block is a card missing its kind line or tilde-fenced code whose text reads as a mapping, and the parser cannot tell which. It locates at the opening fence, and its hint names both fixes: the `$kind:` line, and the backtick fence spelled with the opener's info string.

The diagnostic's `message` is the variant's `Display` rendering, so the
`#[error]` attribute is the one place a variant's English is spelled and a Rust
caller formatting `{err}` reads the same sentence a binding reads off `message`.

**`YamlError`**: the one adapter every `serde-saphyr` error passes through. Sanitizes the message (the parser appends its own Rust API names (`from_str_multiple`, `DuplicateKeyPolicy`) which `yaml_hints::enrich_yaml_error` strips), derives the hint, and carries the 1-indexed line/column the parser located; `to_diagnostic(code, file)` renders all three. The emit side has no input to point at, so it carries neither position nor hint.

It exists so no public signature names `serde-saphyr`. A third-party error type in a published signature chains this crate's major to that crate's, and to the choice of parser at all; the workspace pins `~1.0` and keeps the parser an implementation detail. That promise covers the message as much as the type, which is why the sanitizer is inside the adapter rather than beside it.

Three surfaces return one directly (`value::from_yaml_str`, `QuillValue::from_yaml_str`, `QuillConfig::schema_yaml`). `value::from_yaml_str` deserializes into the caller's type, so a missing or mistyped key is located as a syntax error is. `QuillConfig::from_yaml_with_warnings` converts through it to `quill::yaml_parse_error`, and the acroform backend to `acroform::invalid_form_spec` against `Form.yaml`. The card-yaml path does not travel this way: it becomes `ParseError::YamlErrorWithLocation`, which additionally knows the enclosing block.

Its `line`/`column` are document coordinates, not block-relative ones:

- The parser reports a position inside the string it parsed: the fence content with its CRLF line ends read as LF, line-for-line, minus the whitespace `trim` takes off the front. The assembler translates that position onto the document.
- `to_diagnostic()` renders it as a `Location` against `DOCUMENT_FILE` (`input.md`). Markdown reaches core as a string, so the anchor names the input rather than a path on disk.
- The message names the block instead of repeating a number (`YAML error in the root card-yaml block: …`, `… in card-yaml block 2: …`). The parser's own snippet inside it stays block-relative, as the parser rendered it.

**`RenderError`**: the main rendering error, a struct carrying a non-empty
`Vec<Diagnostic>` (`RenderError::new` / `from_diag` / `coded(code, message)`
for the one-error-diagnostic case every Quillmark-side refusal takes;
`diagnostics()` borrows,
`into_diagnostics()` consumes). There is no failure taxonomy beyond the
diagnostics themselves: the machine-routable identity of a failure is each
diagnostic's namespaced `code`, and consumers route on codes, not on a type.
Multi-problem stages (validation, quill config, backend
compilation) carry several diagnostics so every problem reaches the caller in
one pass. The first is an error. A `Warning` after the errors is one the
failing stage raised before it failed, carried because it can explain the
failure ([Warning flow](#warning-flow)). `Display` follows the count-based
message rule shared with both bindings: the primary diagnostic's message for a
single error, an `"<N> error(s): <first message>"` aggregate for more,
warnings not counted.

Notable codes: `quill::name_mismatch` / `quill::version_mismatch`, the
document is well-formed but paired with the wrong quill (see
[VERSIONING.md](VERSIONING.md)); `backend::update_unsupported`: the default
for a backend session that does not override the incremental-`update` seam
(both built-in backends override it); `backend::format_not_supported`: the
requested format is outside the backend's `supported_formats`, one code on
every backend so a caller matches the condition once;
`backend::invalid_raster_scale`: a `RenderOptions.ppi` or a canvas scale
that is not finite and positive, or that would rasterize a side of a page past
`MAX_RASTER_SIDE`, and on `paint` a scale under `f32::MIN_POSITIVE` — ppi and
canvas scale are the same quantity in two units, so they share the code, and
the refusal speaks the unit passed: its message names the value, and its hint,
past the ceiling or on `paint`, a value that fits, a ppi on an export
and a scale on the canvas;
`backend::page_index_out_of_bounds` / `backend::page_selection_not_supported`:
a `RenderOptions::pages` selection naming a page the document does not have, or
asked of a format the backend emits whole (PDF on both built-in backends), both
minted by the shared constructors in `crates/core/src/backend.rs`;
`engine::backend_not_found`: the quill's declared backend is not registered;
`engine::font_unsupported` / `engine::font_unreadable`: a host font file
`Engine::with_fonts` refuses, one per file, in a format other than `.ttf`,
`.otf`, `.ttc` and `.otc` or holding no face Typst reads
([ARCHITECTURE.md](ARCHITECTURE.md#fonts)).

`pdf::*` is the AcroForm stamping spine's own namespace (`pdf::parse`,
`pdf::write`, `pdf::rotated_page`, `pdf::bad_rect`, …): `quillmark-pdf` carries
the code on its `PdfError` and the `From` impl forwards it intact into a
`RenderError`, so a spine refusal routes the same whichever backend drove it.

**`edit::*`: mutator diagnostics.** Document and card mutators fail with the
`EditError` enum (`crates/core/src/document/edit.rs`), one namespaced code per
variant via `EditError::code()` (`edit::invalid_field_name`,
`edit::unknown_field`, `edit::index_out_of_range`, `edit::field_coercion_failed`, …).
Both bindings stamp that code onto the `Diagnostic` they raise: the mutator
peer of the render-path namespaces. Identity is the code, never message text:
routing coercion-vs-undeclared is `edit::field_coercion_failed` vs.
`edit::unknown_field`, read off `diagnostics[0].code`.

A revise guarded by what its writer read (a `Base`) refuses under two more:
`edit::stale_base` when the value no longer hashes to the writer's
`baseHash`, and `edit::rebase_conflict` when the writer's edit of its
`baseMarkdown` and the change made since touch the same lines. Both carry
`current`, the hash of the value now: a writer reads the value again, checks
that read against `current`, and writes from it. Retrying the same markdown
under `current` lands the revert the base refused. A refusal on a field
anchors at the field. The whole-document lane raises them through
`BoundParseError::Base`, as it raises a parse failure, before any mutation.

Building a card from a `CardWire` (`insertCard`) is the same
surface reached without an address, so it refuses under the same codes:
`WireError` carries the `EditError` its addressed twin raises, and
`WireError::code()` is that code. Two of its codes are not `edit::*`, or not
reachable elsewhere. A malformed `quill` string carries
`parse::invalid_quill_reference` and the grammar hint, the code core mints
wherever a reference is parsed. And `edit::invalid_payload` names a malformed
item list rather than a malformed field (a duplicate key, a field count past the
§8 bound, a `$` entry twice, a comment spanning lines). Three of the four reach
this door alone; the count also reaches an addressed mutator, because it is the
one invariant a caller holding a `(name, value)` pair cannot check for itself —
so `storeField` on a full card refuses under the code the wire already spells
for it, anchored at the card that is full.

**`RenderResult`**: successful result carrying artifacts, output format, and non-fatal `Vec<Diagnostic>` warnings

## Warning flow

Warnings travel the same `Diagnostic` currency as errors, on these producer
families:

- **`quill::*` load warnings**: the second half of
  `QuillConfig::from_yaml_with_warnings`, carried on the loaded quill and read
  off `Quill::warnings()`, so every binding host reaches them. A warning here
  never fails the load. Four codes:
  - `quill::bodiless_card_kind`: a card kind declaring `body.enabled: false`,
    which is the loader's one view of a row in card costume
    ([CARDS.md](CARDS.md#card-row-matrix)). It advises where a load error
    would overreach — the loader sees a correlate, not the fact.
  - `quill::deprecated_groups`: a card placing fields with the legacy
    `ui.group`, which a top-level `object` replaces, nesting the fields under
    its id ([SCHEMAS.md](SCHEMAS.md#grouping)).
  - `quill::deprecated_flat`: a field asking `ui.layout: flat`, which no
    editor honors ([SCHEMAS.md](SCHEMAS.md#schema-emission)).
  - `quill::field_after_object`: a card-level field outside every group
    declared after one, which draws above the groups instead
    ([SCHEMAS.md](SCHEMAS.md#grouping)).
- **Parse warnings**: the `warnings` on the `Parsed` that `Document::parse`
  returns (e.g. a `~~~` opener missing its blank line), which both bindings
  keep on `doc.warnings` beside the `conform::*` set `Quill::parse` appends.
  They report the load, so no render carries them: a revise or a store can
  change what the document holds after it. The CLI's `render`, which parses
  and renders in one command, prints them ahead of the render's. A tag warning
  (`parse::unsupported_yaml_tag`) anchors at the tagged node's `path`, a
  card's under its stored `$kind` as `pathFor` mints it. One on a `$` key or
  inside `$ext` or `$seed`, which have no document address, carries none.
  A card whose `$id` the parse replaced, one outside the card-id grammar or
  one an earlier card holds, adds `parse::card_id_reminted` with the `id` it
  spelled, anchored at the card (`cards.<kind>[<i>]`) after every card's
  other warnings. A whole-document revise adds one at the revised card for
  an inserted card whose `$id` is not a card id or a stored card held.
  A stored document's load (WASM `fromStored` / `loadStored`, Python
  `from_stored`; Rust `Document::anchor_collisions`) adds
  `parse::anchor_id_collision` with the `id`, one per anchor id a body or
  content value holds more than once with a table cell or footnote note
  taking part, anchored at the value's path, and no other warning.
  A body's markdown import adds `parse::dropped_construct`, one per construct
  it dropped, with its `count`, anchored at the body (`main.body`,
  `cards.<kind>[<i>].body`) after its card's tag warnings. The `construct` is:
  - a raw tag's lowercase name: `div`, a `qm-*` name outside the carrier
    grammar such as `qm-a--b`, `qm-anchor` where its block drops
    markdown, or a tag in an HTML table outside the subset, such as
    `caption`;
  - `qm-<name>` for the open tag of a name outside `qm-span`, `qm-anchor`
    and `qm-island`, which carries nothing, and for a `qm-span` tag that
    drops: left unclosed, self-closing, setting no family, or on a tag line;
  - `qm-island` for an island token a cold import drops, or one a revise
    reads where no island sits;
  - `qm-span[<attr>]` for one `qm-span` attribute that drops;
  - `qm-anchor[ref]` and `qm-island[ref]` for a revise's anchor tag or island
    token naming nothing the stored value holds, or an anchor an earlier tag
    names;
  - `table` for an HTML table its block ends before its `</table>`, at a
    blank line or another `<table>`;
  - `table:row` for an HTML table's row too short for its grid,
    `table:text` for text outside every cell, and `table:size` for a
    table's rows past the `MAX_TABLE_CELLS` slots a document's tables share;
  - `<tag>[<attr>]` for one attribute of an HTML table's tag that drops,
    such as `td[nowrap]` or `table[widths]`;
  - `footnote` for a footnote definition nothing references, a second
    definition of one label, or a reference in a table cell, an image's alt
    text or a note;
  - `footnote[<block>]` for a block in a note, which keeps its text as lines
    of the note: `heading`, `list`, `code`, `quote`, `table` or `rule`;
  - `math` for an equation in a table cell, an image's alt text or a note,
    or one whose TeX is blank once its edge whitespace drops, which keeps its
    source as text.

  A parse reports a body's drops alone, since it knows no field's type: a
  `richtext` field's markdown string reports its drops at its path from
  `Quill::conform`, which `Quill::parse` runs, and from `Quill::validate`
  while the field still holds the string.
- **`parse::dropped_construct` off a markdown write.** The diagnostic a parse
  adds, minted by `quillmark_core::document::dropped_construct`, also rides
  the write that imported the markdown. A revise returns it on its `Revised`
  receipt beside the `Delta`: unanchored from a `Card` verb, which does not
  know its address, and at the body or field from the typed writer and WASM
  `revise`. A whole-document revise (`Document::revise`, the writer's
  `revise_document`) carries the parse's warnings, then each revised content
  field's at the field, on `DocumentRevised.warnings`. `add_card` returns it
  at the placed card's body, WASM `importMarkdown` and `rebase` beside the
  content with no `path`. Python's `add_card` returns them as a list, and its
  `revise_body` / `revise_field` / `revise_document` under the dict receipt's
  `warnings`, as `Diagnostic` objects. A typed `set` / `set_all` returns
  a `richtext` string's drops at the string's path (WASM `set` / `setAll`,
  Python `set` / `set_all`), and `add_card` its fields' ahead of its body's. A
  conform reports a string's drops beside its `conform::*` warnings, and
  `validate` a string's the field still holds. A card inserted with a string
  body drops without it.
- **`export::lost_construct` off a markdown export.** The receipt of an
  export: `quillmark_content::export::to_markdown_checked` reads its markdown
  back and names each part of the content it does not carry, and
  `quillmark_core::document::lost_construct` mints one warning per part. The
  `construct` is a mark's wire type (`strong`, `link`, …) with its `start`
  and `end` in the content's text, an island's type (`table`, `image`, …), or
  `text` where the text or its blocks read back otherwise. WASM
  `exportMarkdownChecked` returns them beside the markdown with no `path`. A `*`,
  `_` or `~~` mark is written over its text without the whitespace at its
  edges, and names its full range here.
- **`conform::*`: resting-form warnings.** `Quill::conform` returns one per
  declared content field whose value the strict write refuses, and
  `Quill::parse` appends them to the `Parsed.warnings` the parse produced. Each
  is the `edit::*` diagnostic that write would have raised, re-namespaced and
  demoted to `Severity::Warning` with its `args` and `path` intact: the value
  rests as authored rather than being refused or silently retyped, so the state
  is repairable. The walk is stateless, so a repeat conform re-emits the
  identical set. Its scope is the content fields: a field whose type tree bears
  no content leaf never enters the walk, so a scalar the strict write would
  refuse raises no `conform::*` warning. It reaches `validation::*` only if the
  *render floor* also refuses it (`validation::type_mismatch`); the floor is
  more lenient than the write, and what it adopts is valid.
- **Validation warnings**: `Quill::validate(doc)` returns every
  `validation::*` diagnostic, mixing severities. Severity is the class
  [SCHEMAS.md](SCHEMAS.md#what-blocks-a-render) assigns: an `Error` is
  malformed input, and the document does not render; a `Warning` is unclaimed
  or folded input, which renders. The warnings are
  `cardinality`, `over_length`, `out_of_variant`, `unknown_card`, `body_disabled`,
  `unknown_field`, `line_break_folded`, and the
  `$seed` checks, which warn
  whatever their class because no render reads `$seed`. Beside them it
  returns the `backend::declined_construct` warnings a render raises.
  The render gate consults only the fatal set. A one-shot render
  (`Engine::render`) carries every one of these warnings on
  `RenderResult.warnings`, the declines among the compile's and the rest
  ahead of them. A
  session carries none: its warnings are its current compile's, so its editor
  reads `Quill::validate` beside it. The CLI's `render` prints them
  ([CLI.md](CLI.md)). Values
  are judged in the form the render floor builds from them
  ([SCHEMAS.md](SCHEMAS.md) § "Type coercion").
- **`backend::declined_construct`: declined-construct warnings.** A backend
  that declines a construct outright says so itself: one diagnostic per
  (content field, construct) carrying `backend`, `construct` and `count` in
  `args` and the field's `DocPath` in `path`, minted by
  `quillmark_core::backend::declined_construct`. Raised at the compile that
  dropped the construct, so it rides the session's compile warnings, and
  `LiveSession` mints it from the plate data rather than the backend. The Typst
  backend declines `image` in content
  ([CONVERT.md](CONVERT.md#declined-images)), and the acroform backend every
  construct but the paragraph. `quillmark_core::backend::declines` is the
  table, keyed by backend id. `Quill::validate` walks the plate a render
  compiles, schema defaults and coercion applied, and returns the same
  diagnostics, byte for byte, before any render. A document the render floor
  refuses compiles nothing, so it draws none.
- **`typst::unsupported_tex`: a TeX command an equation cannot lower.** The
  Typst compile warns once per content field whose equations hold a command
  outside the vocabulary the lowering knows ([CONVERT.md](CONVERT.md#equations)),
  naming each, with the field's `DocPath` in `path`. Each draws as its own
  name, save a `\not` and a `\left` delimiter, which draw nothing. A field
  whose equations nest groups past the depth the lowering reads warns once
  more under the code, naming the depth. It rides the session's compile warnings, and `validate` has no twin
  for it.
- **`typst::unknown_font_family`: a family the host library lacks.** The
  Typst compile warns once per (content field, family) whose font runs name a
  family the host's font library does not hold, a quill's own or fallback
  family included, compared lowercased and spelled as
  the field first spells it, with the field's `DocPath` in `path` and the
  family in `args.family` ([CONVERT.md](CONVERT.md#font-runs)). Its text keeps the surrounding family.
  It rides the session's compile warnings, and `validate`, which holds no
  fonts, has no twin for it. The engine has no strict mode: a host needing a
  hard failure treats the code as an error.
- **`typst::missing_glyph`: text no font draws.** The Typst compile warns
  once, naming each character the laid-out pages draw as a `.notdef` box,
  whitespace and default-ignorables aside. It rides the session's compile
  warnings.
- **Compile warnings**: the Typst backend maps the compiler's non-fatal
  diagnostics (font fallback, overfull pages, …) through the same span
  resolution as errors. They are state of the session's current compile:
  exposed via `LiveSession::warnings()` (the `SessionHandle::warnings` seam,
  default empty), refreshed by each committed `update`: a failed update keeps
  the last-good compile *and* its warnings, and appended to
  `RenderResult.warnings` on every `render()`, including the one-shot
  `open` → `render` path.

Ordering in a merged `RenderResult.warnings` is pipeline order: validation
warnings first, then compile warnings, with no dedup across families.
`backend::declined_construct` dedups within itself, per field: its producer
sees every occurrence at once, so the occurrences collapse into `count`.

A Typst `open` failing once its world has loaded, in codegen or the compile,
leaves no session to serve the quill-load warnings ([below](#typst)), so its
`RenderError` carries them after its errors. An `#import` of a package the
load skipped fails as `typst::file_not_found`, and only
`typst::package_manifest` says why. A failed `update` carries none: the
session goes on serving them.

## Bindings Error Delegation

Python and WASM bindings delegate to core types:

- **Python**: `PyDiagnostic` wraps `Diagnostic`. Every raised exception is `QuillmarkError` (a single type). Every exception carries a `diagnostics` list; `str(exc)` follows the shared count-based message rule.
- **WASM**: `WasmError` carries a single `diagnostics: Vec<Diagnostic>` (always non-empty). The thrown JS `Error` has a `.diagnostics` array attached and a `.message` derived from `diagnostics` by the same count-based rule. Consumers read `err.diagnostics[0]` for the primary diagnostic and iterate `err.diagnostics` for the rest. Parse failures (`Document.fromMarkdown`) carry the same shape; including the `parse::input_too_large` diagnostic for inputs over `MAX_INPUT_SIZE` (10 MiB) and the `edit::*` codes for post-parse mutators.

**WASM delivery follows the function kind, not the failure kind.** A synchronous verb throws; a promise-returning verb rejects; nothing does both.

- The promise-returning surface is `init` plus the five `Engine` verbs (`render`, `open`, `load`, `supportedFormats`, `fontFaces`). A programming error reached through one of them (a handle that is not this copy's, `runtime::not_a_quill` / `runtime::not_a_document`; an unregistered backend, `engine::backend_not_found`; a refused host font, `engine::font_unsupported` / `engine::font_unreadable`) arrives as a rejection like any other failure.
- A `WebAssembly.RuntimeError` from a backend build's `render`, `open`, or `LiveSession` `update` / `render` / `paint` becomes `runtime::backend_trapped`, the trap on `cause`, and so does a `RangeError` or `InternalError` for stack or memory exhaustion, the JavaScript engines' report of a native stack overflow inside wasm. The trap leaves the module-scoped instance undefined; the host discards the realm.
- `init` is the one promise-returning export not declared `async`: its memo is returned by identity rather than re-wrapped per call. Its conflict guard therefore returns `Promise.reject(runtime::init_conflict)` where an `async` body would have converted a throw.
- A synchronous throw from `init` would lose the rule at exactly one export, and silently: `Promise<void>` cannot declare it, so the declaration invites `init(BYTES).catch(…)` and the throw escapes.

## Backend Error Mapping

### Typst

Typst diagnostics mapped via `map_typst_errors()`:
- Severity levels mapped (Error/Warning)
- Spans resolved to file/line/column
- Error codes: a **closed set**, keyed off the message's shape
- Hints: Typst's first, else one the mapping verifies (below)

Typst has no error codes of its own, so the mapping mints one. It classifies
rather than quotes: `typst::file_not_found` (a file the world refused),
`typst::unknown_variable` (a plate naming a symbol that is not in scope),
`typst::type_error` (a value that is not what the position wanted), and
`typst::compile` for every message the set does not name. Errors and warnings
are classified alike.

The residual bucket is what keeps the set closed, and a code spelled by the
message instead is what it rules out: that would carry author-supplied text —
a searched path, a symbol name — into a routing key, and give the key one
value per input. Typst's sentence stays in `message`, which is where the
searched path is read. Classification reads that English, so a reworded
message degrades to `typst::compile` rather than minting a code of its own.

Typst hints nothing for a missing file, so the mapping adds one hint of its
own. A bare path resolves from the directory of the file naming it, so a module
below the quill root misses `assets/…` at the root. A `typst::file_not_found`
raised there hints the rooted spelling (`/assets/logo.svg`) when the world loads
a file at it, and only then.

See `crates/backends/typst/src/error_mapping.rs`.

**Quill-load warnings** are the backend's other warning source, hand-coded
rather than derived from a Typst diagnostic: `typst::path_skipped` (a file
Typst's `VirtualPath` rejected: asset or package file alike),
`typst::package_manifest`, `typst::package_entrypoint_missing`, and
`typst::font_collection_skipped` (a `.ttc` or `.otc` declaring more than 256
faces). Each marks a
file the world had to skip, which otherwise surfaces only as an unresolved
`#import` pointing at the plate instead of at the defect. They are properties
of the quill, not of a compile, so `QuillWorld` holds them and the session
serves them ahead of every compile's own: an `update` swaps the compile half
and keeps these.

**Plate refusals** fail `open` and `workspace` before a world exists:
`typst::plate_missing` (the quill holds no `plate.typ` at its root and
declares no `typst.plate_file`, or `typst.plate_file` names no file in the
quill), `typst::plate_path_invalid` (`typst.plate_file` is not a string,
names no path once its `.` and empty steps drop, or names a file at a path
Typst's `VirtualPath` refuses, such as one holding a `\`), and
`typst::invalid_utf8`. The plate is the one file a refused path
cannot merely skip: the world loads it at its own path, and any stand-in name
may belong to a file the quill holds.

A project source importing the helper any way but as a module fails `open` and
`workspace` with `typst::helper_import` before anything compiles, one error per
import, located at it ([PLATE_DATA.md](PLATE_DATA.md#typst-helper-package)).

## Validation message contract

Field-level validation diagnostics (`validation::type_mismatch`) emit a single
canonical shape:

- **Field path**: the document-model anchor of the offending field
  (`recipient`, `cards.indorsement[2].author`); see [Document-model
  paths](#document-model-paths).
- **Source token**: the YAML scalar that triggered the error, rendered
  verbatim in its YAML-canonical form (`42`, `null`, `true`, `""`). Strings
  appear quoted; primitives appear bare.
- **Schema declaration**: the field's declared type and, when present,
  its default. Defaults render with the same verbatim formatting.
- **Both exits when applicable**: the message names two ways out. The
  parser does not silently coerce; the message is the lever.

Example messages:

```
Field `urgent` got string `"maybe"`, schema declares `boolean`.
Either write `true` or `false`, or change the schema's `type:` to
`text`.
```

A present-null value (`subtitle:`, `subtitle: null`, `subtitle: ~`) is treated
exactly like an omitted field: null ≡ absent, it coerces and validates clean,
and it blank-fills at render (authored › `default:` › blank). A document that
answers nothing therefore produces no field-level diagnostic at all.

Implementation: `crates/core/src/quill/validation.rs` (the `ValidationError`
`Display` impl).

## Document-model paths

`Diagnostic.path` is a **document-model** anchor into a typed `Document`:
one canonical grammar, one serializer, one parser: `DocPath`
(`crates/core/src/path.rs`). Every emit site constructs a `DocPath`; no site
assembles a path with `format!`, so Quillmark never ships two shapes for one
anchor. A refusal whose value never reached the document model has no such
anchor and carries no `path`: `authoring::invalid_shape` names its place in
the authoring document as an RFC 6901 pointer in `args.pointer`
([AUTHORING.md](AUTHORING.md#import)).

| Anchor | Path |
|---|---|
| Main-card field | `main.recipient` |
| Nested in an array of objects | `main.recipients[0].name` |
| Main body | `main.body` |
| Typed card (whole) | `cards.indorsement[0]` |
| Field on a typed card | `cards.indorsement[0].signature_block` |
| Body on a typed card | `cards.indorsement[0].body` |
| Card of a kind the quill does not declare | `cards[0]` |

Every path is **rooted**: a main field at `main.<field>`, a card field
kind-qualified at `cards.<kind>[<index>].<field>` (kind and document-array index
fused so a consumer gets both without a second lookup). A card of an undeclared
kind has no declared kind to qualify with, so `cards[<index>]`, and every field path
under it, is the only bare-index form. Rooting keeps the
grammar total against a field named for a root (`main.cards`, `main.main`), and
a quill refuses a field named `body` (`quill::reserved_field_name`), the one field
name that would collide with the body terminal. Field
names and card kinds exclude `.`, `[`, `]`, so the rendered form round-trips;
the WASM build exports `parseDocPath` / `formatDocPath` (structured
`DocPathSeg[]` ↔ string) so a consumer routes on segments instead of regexing
the string.

The boundary **mints** as well as parses: `doc.pathFor(addr)` / `doc.cardPath(i)` render a write address as its path, so a consumer holding an `Addr` never restates the kind lookup a card root needs. A wrong-kind path is compared as a string and matches nothing, silently. The mint is quill-free, reading the card's stored `$kind` verbatim: the rule the mutator anchors and the geometry translation use, not `validate`'s declared-kind filter. That filter is the one edge where a minted path and a validation diagnostic differ for the same card.

`DocPath` is the anchor on **every** address that crosses to a consumer, not
only `Diagnostic.path`. Mutator (`edit::*`) diagnostics carry it (a field error
at `main.<field>` or `cards.<kind>[<i>].<field>`, a structural out-of-range op at
`cards[<i>]`), `set_all`'s batch keying on a field name under one card's base;
and `LiveSession` geometry (`regions` / `fieldAt` / `positionAt` / `locate`)
keys on it: the session translates the backend's plate-space form to `DocPath`
at the boundary, segment by segment — the `$cards.<kind>.<ordinal>` head to the
absolute index, a numeric tail segment to a bracketed array index
(`references.0` → `main.references[0]`) — so one parser routes diagnostics and
geometry alike, and a geometry address and the validation diagnostic on the
same place are the same string.

**Three grammars, one that crosses.** Only `DocPath` reaches a consumer. The
other two stay backend/template-internal and are named here so they are not
confused with it:

- **Plate JSON**: the sigiled `data.$cards` a template author composes
  ([CARDS.md](CARDS.md)), and the plate-space geometry address a plate's `$path`
  mints: a `.`-separated run under an optional `$cards.<kind>.<ordinal>` head,
  where an all-digit segment is an array index (`references.0`) and `$body` the
  body terminal. The ordinal counts cards of that kind, where a `DocPath` card
  index counts every card (with cards `note`, `annotation`, `note`,
  `$cards.note.1` is `cards.note[2]`). A template-author contract (`qm.form-field(field: "refs.2")`,
  `qm.ink(..)`, every published `Form.yaml`), so it keeps its own spelling
  (renaming it is a blast radius) and translates to `DocPath` before it crosses.
- **Schema-space coercion anchors**: `CoercionError` keeps its own
  `card_kinds.<kind>.<field>` / bare-field anchors, a schema-declaration
  namespace, not a document path. Where a coercion becomes an
  `edit::field_coercion_failed`, the binding re-anchors it in `DocPath` space at the
  field being written; the raw schema-space anchor does not cross.

Config-space anchors (`$seed.<kind>.<field>`, Quill.yaml schema-literal owner
labels) ride the `DocPath` serializer with their prefix as a leading segment.

## Diagnostic args

`message` is the canonical English rendering. `args` is what it interpolates, keyed by name, so a consumer with its own string table selects a sentence by `code` and fills it itself. `code` + `args` is the substitution unit; `code` + `message` is not.

Values keep their JSON shape (`allowed` arrives as a list, `len` as a number) because joining and pluralizing are locale decisions, not Quillmark's.

**Quillmark prose never rides under a key.** Where a message bottoms out in text minted per-site (`CoercionError`'s `reason`, the ~20 `parse::invalid_structure` sites, another codec's error), that text stays in `message` and contributes no arg. A consumer's own sentence is then coarser than ours (`edit::field_coercion_failed` says which field and which target type, not which of a dozen ways the value failed to become one). That is the contract, not a gap in it: a coarser sentence a consumer owns beats a fluent one half in the wrong language. The full-fidelity alternative (a key holding English) is worse than omission, because the fallback below cannot fire on a key that is present.

**Falling back is wholesale.** A formatter whose template needs a key that is absent renders `message`, never a sentence with a hole in it. It takes `hint` from Quillmark in the same breath: the hint is the same English as the message tail (`ValidationError`'s `Display` appends it verbatim), so translating one and passing the other through ships a two-language diagnostic. Localize a code and you own both sentences; fall back and you take both.

**`hint` needs no code of its own.** It is a function of `code` and `args`. Most hints are per-code constants. `type_mismatch_hint` branches only on whether a default exists, which is why `default` is present as a key exactly when the schema declares one, rather than present-and-null; the two card-kind hints branch only on whether `allowed` is empty. Any datum a hint branches on is message-relevant by definition and belongs in `args`; a parallel `hint_code` would be derived state on the wire, free to drift.

**Anchors do not travel twice.** `path` is never an arg. `field` and `kind` are, even though `doc_path` also folds them into the anchor; the rule bars the assembled path string, and recovering a name from one is unsound anyway, since `DocPath` renders field segments unescaped and parses on `.` and `[`, so exactly the malformed names `edit::invalid_field_name` reports can round-trip into different segments. `CoercionError`'s `path` stays out for the separate reason below in § "Three grammars": it is a schema-space anchor and does not cross.

**Growth is additive.** Per code, keys are append-only and never retyped, and value spellings are as frozen as the keys: `sourceToken`'s verbatim-YAML form (`42`, `null`, `""`), `actual`'s `integer`-vs-`number` vocabulary. Codes themselves are governed the same way, which is what makes them safe to key a string table on. Without that, an upgrade turns into a silent English regression in every localized consumer.

### Coverage

The table is the contract, so it is tested like one: `diagnostic_args_match_canon` in `crates/core/src/error.rs` fails if code and canon disagree, and its twin asserts the codes off the table carry nothing. Each `args()` binds every field rather than eliding with `..`, so a new field on a variant does not compile until it decides whether it is message-relevant.

That pair holds code and table together and cannot hold either still: a rename edits both in one commit and passes. The keys a consumer outside this repository reads are pinned separately, by `args_keys_a_named_consumer_reads` beside them — a list that does not move when the table does, so renaming one of those keys fails here rather than downstream, a release later.

Three outcomes, and the wire tells them apart only with this table in hand, since "no keys" reads identically for the second and third:

- **structured**: keys present; the consumer writes the whole sentence.
- **code-determined**: no keys needed; the sentence follows from `code` and the anchor alone. Fully localizable.
- **fallback**: render `message`.

| Code | Args | Outcome |
|---|---|---|
| `validation::type_mismatch` | `expected`, `actual`, `sourceToken`, `default`? | structured |
| `validation::enum_violation` | `value`, `allowed`, `open`? | structured |
| `validation::held_stored` | — | code-determined |
| `validation::format_violation` | `format` | structured |
| `validation::unknown_card` | `allowed`, `card` | structured |
| `validation::body_disabled` | `card` | structured |
| `validation::unknown_field` | `field`, `suggestion`?, `container`?, `variant`? | structured |
| `validation::coercion_failed` | `value`, `target` | structured, coarser |
| `validation::out_of_variant` | `variant`, `selected` | structured |
| `validation::cardinality` | `max`, `actual` | structured |
| `validation::over_length` | `max`, `actual` | structured |
| `validation::seed_unknown_kind` | — | code-determined |
| `validation::seed_overlay_shape` | — | code-determined |
| `validation::seed_unknown_field` | — | code-determined |
| `validation::not_single_line` | — | code-determined |
| `validation::not_plain` | — | code-determined |
| `validation::line_break_folded` | — | code-determined |
| `edit::invalid_field_name` | `field` | structured |
| `edit::unknown_field` | `field` | structured |
| `edit::invalid_kind_name` | `kind` | structured |
| `edit::index_out_of_range` | `index`, `len` | structured |
| `edit::too_many_cards` | `max` | structured |
| `edit::value_too_deep` | `max` | structured |
| `edit::field_not_single_line` | `field`, `codec` | structured |
| `edit::field_not_content` | `field`, `declared` | structured |
| `edit::field_coercion_failed` | `field`, `target` | structured, coarser |
| `edit::field_decode` | `field`, `codec` | structured, coarser |
| `edit::reserved_kind` | — | code-determined |
| `edit::root_only_entry` | `key` | structured |
| `edit::invalid_card_id` | `id` | structured |
| `edit::duplicate_card_id` | `id` | structured |
| `edit::import` | — | fallback |
| `edit::content_apply` | — | fallback |
| `edit::invalid_payload` | — | fallback |
| `edit::stale_base` | `current` | structured |
| `edit::rebase_conflict` | `current` | structured |
| `conform::invalid_field_name` | `field` | structured |
| `conform::value_too_deep` | `max` | structured |
| `conform::field_not_single_line` | `field`, `codec` | structured |
| `conform::field_coercion_failed` | `field`, `target` | structured, coarser |
| `conform::field_decode` | `field`, `codec` | structured, coarser |
| `parse::input_too_large` | `size`, `max` | structured |
| `parse::too_many_fields` | `count`, `max` | structured |
| `parse::too_many_cards` | `count`, `max` | structured |
| `parse::invalid_quill_reference` | `value` | structured, coarser |
| `parse::yaml_error_with_location` | `blockIndex` | structured, coarser |
| `parse::payload_not_mapping` | `actual`, `info`? | structured |
| `parse::missing_kind` | `info`? | structured |
| `parse::empty_input` | — | code-determined |
| `parse::unsupported_yaml_tag` | — | code-determined |
| `parse::dropped_construct` | `construct`, `count` | structured |
| `parse::card_id_reminted` | `id` | structured |
| `parse::anchor_id_collision` | `id` | structured |
| `export::lost_construct` | `construct`, `start`?, `end`? | structured |
| `parse::invalid_structure` | — | fallback |
| `parse::missing_quill` | — | fallback |
| `parse::body_import` | — | fallback |
| `backend::declined_construct` | `backend`, `construct`, `count` | structured |

`parse::missing_quill` looks code-determined and is not: it picks one of three sentences by re-reading the source, and no field records which.

Every `conform::*` row is reachable only through the content-field walk, so the family is narrower than its `edit::*` twin. `conform::field_coercion_failed` fires for a scalar nested inside a content-bearing field: an `array` of `richtext`, an `object` with a `richtext` property. A top-level scalar never reaches it: it surfaces as `validation::format_violation` or `validation::type_mismatch` where the render floor refuses it too, and nowhere where the floor adopts it.

### Scope

`validation::*`, `edit::*`, and `parse::*` are the codes an end user meets, and the table covers them. The rest carry no args, which is the domain's shape rather than a phase of the work:

- **`quill::*`** is quill-authoring. Its reader is a template author debugging `Quill.yaml`, the one audience for whom canonical English is the deliverable. Three of its codes are `format!`-built per slot besides.
- **`typst::*`** is a closed set (`error_mapping.rs`), but a coarse one: the compile codes classify a Typst diagnostic without taking it apart, so the detail stays in the message that carries it.

Because those codes carry no args, every consumer template falls back on them by the rule above. That is what makes a surface covering a third of the codes total rather than partial.

## Error Presentation

**Pretty printing** (`Diagnostic::fmt_pretty()`):
```
[ERROR] Undefined variable (E001)
  --> template.typ:10:5
  hint: Check variable spelling
```

One printer, reached from every surface: the CLI, Python's `str(diagnostic)`, and WASM's `formatDiagnostic(d)`. A consumer surfacing Quillmark's English calls it rather than holding a copy of the layout. A consumer wording its own selects by `code` + `args` (§ "Diagnostic args") and owns the layout with the sentence.

**Consolidated printing**: the CLI pretty-prints every diagnostic a `RenderError` carries, one per line, to stderr.

**Machine-readable**: all diagnostic types implement `serde::Serialize`.
