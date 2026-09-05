# Changelog

## Unreleased

- fix(wasm): **an unregistered backend rejects with `engine::backend_not_found`
  like every other failure.** The four `Engine` verbs threw a bare `Error` for a
  quill whose declared `backend:` has no loader, so `isQuillmarkError` answered
  `false` and the README's own `catch` example re-threw it as a foreign
  failure. The rejection now carries one diagnostic under the code core and the
  Python binding already raise, hinting the registered backend ids.
- change(core)!: **YAML nesting depth is the parser's to bound, and
  `MAX_YAML_DEPTH` is gone.** The constant set `serde_saphyr`'s depth budget
  and doubled as the bound on host values crossing into the document, so one
  number spoke for two unrelated limits. YAML parsing now runs on
  `serde_saphyr`'s own budget, and the write surfaces (`store_field`,
  `store_ext`, the wire and storage DTOs, the bindings' converters) bound
  values at `MAX_JSON_DEPTH` (128), the depth storage already accepts —
  raising the accepted depth on those paths from 100. Read the cap from
  `quillmark_core::error::MAX_JSON_DEPTH`; `§8 Limits` no longer fixes a YAML
  nesting number.
- fix(typst): **a compile diagnostic carries a code from a closed set.** The
  code was the message up to its first `:`, so a missing asset minted
  `typst::file not found (searched at assets/logo.png)` — the author's path
  inside what consumers route on, one code per input — and a URL truncated
  mid-value to `typst::file not found (searched at https`. Most Typst messages
  hold no `:` at all, so the whole sentence became the code. The mapping now
  classifies: `typst::file_not_found`, `typst::unknown_variable`,
  `typst::type_error`, and `typst::compile` for every message the set does not
  name, warnings included. Typst's sentence, and the path it searched, stay in
  `message`.
- fix(core): **an unclosed root `~~~` block is reported as unclosed.** A
  document that opens with `~~~` and `$quill` but never closes the fence drew
  the generic `parse::missing_quill` text, telling the author to open a block
  they had already opened while the scanner's unclosed-fence signal was
  dropped. That signal now reaches the diagnostic: the message names the
  opener's line, the field to close after, and — for a `~~` run or an indented
  `~~~` — the line that failed to close it. A root opener carrying a foreign
  info string (`~~~metadata`) is named the same way.
- fix(core): **a card-yaml parse failure carries a document `Location`.**
  `YamlErrorWithLocation` keeps the engine's line and column, translated
  through the comment lines prescan drops and the leading whitespace `trim`
  removes onto the document's own coordinates, and `to_diagnostic()` sets them
  as the `Location` (`input.md`). The message names the block (`YAML error in
  the root card-yaml block: …`, `… in card-yaml block 2: …`) in place of the
  block-relative `at line N (block K)` prefix, and `args` no longer carries
  `line`.
- feat(core): **`~~~yaml` opens a card-yaml block**, a second non-canonical
  alias beside `~~~card-yaml`; both re-emit as bare `~~~`. A YAML *code* block
  in prose is a backtick fence (```` ```yaml ````), unchanged.
- feat(core): **`validation::example_unchanged`: a cell still holding the value
  its schema showed.** `Quill::validate` warns (non-fatal, beside
  `validation::must_fill`) where an authored value is the field's `example:`,
  or a body is its `body.example` or the `Write <kind> body here.` placeholder
  the blueprint generates for a kind declaring none. The blueprint seats a
  defaultless field's example in its value cell under the `!must_fill` marker
  and a seed commits one on every field that declares it, so dropping the
  marker leaves a value that is present, type-valid, in-domain and nobody's
  answer — a signed memo reading `Duty Title` under the commander's name, with
  a clean validate. Arrays are compared element-wise against the same index of
  the shown literal, so a half-edited list names the element left behind; a
  typed dictionary and a variant container are walked per cell. A field
  declaring no `example:` never fires: absence is `must_fill`'s question, and
  neither does a cell still carrying its marker, which `must_fill` already
  names. The `trigger` arg says which cell spoke (`field` or `body`) and
  `example` carries the shown value.

- feat(core): **the values form: `reader.values()` reads a document as plain
  values and `writer.set_values(values)` writes them back.** A document has
  three forms: *stored* (verbatim, quill-free), *values* (stored with every
  content leaf decoded to its codec's text — `richtext` markdown, `plaintext`
  literal — at every depth), and *resolved* (values blank-filled,
  render-coerced and rung-tagged). `DocumentValues` / `CardValues` are the
  middle one: `{fields, body, cards: [{kind, fields, body, ext}], ext}`, every
  axis present on a read, bodies as markdown, `$ext` on the main card and each
  card (`null` when none), `kind` `null` for a kindless card, a present-null
  as `null`, declared fields first then undeclared ones verbatim. A read never
  coerces: `qty: "3"` reads `"3"` here and `3` only in `resolve`. **Sparse**
  (an absent field is absent, never its `default:`) and **total** (a leaf that
  decodes under neither encoding rides out as stored where `get` raises). A
  projection, never a storage format: markdown carries no anchors, island ids
  or content-only marks, and `$quill`, `$seed`, `!must_fill` markers and YAML
  comments are not carried. `reader.card(i).values()` /
  `writer.card(i).set_values` are the same pair for one card.
  `set_values` is the typed lane widened to the document: **an absent axis is
  untouched, a present one is replaced.** `fields` is the whole truth for
  declared names (an unnamed one is removed; an undeclared one the card holds
  is accepted unchanged, refused changed, left alone unnamed), `cards` *is*
  the card list (matched by position and kind, a differing kind rebuilds the
  slot, past the end appends, past the list removes; an absent `kind` keeps
  the card's), `body` is replaced, `ext: null` removes `$ext` and `{}` records
  an explicit empty one. All-or-nothing, every refusal under its own `DocPath`
  (`main.qty`, `cards.line_item[0].desc`). **A cell whose incoming value
  equals its projection is not written**, so `set_values(reader.values())`
  moves no bytes on any document the bound door admits, carrying through what
  a re-import cannot reproduce: identity anchors, content-only marks,
  `!must_fill` markers, YAML comments, a leaf that decodes under neither
  encoding, a scalar shorthand, an explicit `$ext: {}`. A changed content cell
  is a cold import, as on `set`; `revise_field` per cell keeps anchors.
- feat(core)!: **`reader.get` answers in the values form, and `ReadValue` is
  gone.** `get` returns the plain value: every content leaf in the field's
  type tree as its codec's text, descending `items` / `properties` /
  `variants`, so `reader.get("paragraphs")` is `["Para **one**", …]` and a
  mixed object projects its content property beside its verbatim scalars,
  where an `array<richtext>` used to return the stored content objects. A
  present-null reads `null` rather than `""`. A leaf that does not decode
  raises `edit::field_decode` anchored at the element (`main.paragraphs[1]`).
  `get(name)` equals `values().fields[name]` on every field that decodes.
  `reader.getContent` / `getContentAt` are unchanged.
- feat(bindings)!: **`resolve` lives on the reader.** `quill.resolve(doc)`
  becomes `quill.reader(doc).resolve()`, beside `values()`: a verb that needs a
  schema lives on the cursor, and the two whole-document reads sit together.
  WASM-only, as before.

- feat(bindings)!: **the storage DTO verbs name their lane, not their encoding.**
  `Document.toJson` / `fromJson` / `tryFromJson` / `loadJson` become `toStored` /
  `fromStored` / `tryFromStored` / `loadStored`, and Python's `to_json` /
  `from_json` / `try_from_json` become `to_stored` / `from_stored` /
  `try_from_stored`. `storageVersionOf` and `currentStorageVersion` are
  unchanged, already naming storage. "Stored" is the at-rest form throughout, so
  the pair completes the family `getStored` started; the bare `store` stays the
  field-write lane's verb, which is why the adjective carries the
  document-level pair rather than a `store` / `load` pair. The old names are
  removed rather than aliased. Stored blobs, the `schema` tag, and every byte
  these verbs write are untouched.
- fix(pdf): **the object index skips literal strings, `%`-comments and stream
  bodies, so `N G obj` bytes carried as content cannot shadow the real object.**
  The scan accepted any `<id> <gen> obj` at a token boundary and a later
  occurrence overwrites an earlier one, so a header spelled inside a string value
  (`/Subject (see 4 0 obj)`) or inside raw stream data displaced the real
  object's offset, and every read of that object parsed from the false position.
  `find_endobj_end` skips stream bodies too: `endobj` bytes in stream data
  truncated the object body.
- fix(pdf): **an inheritable page attribute resolves along the page's own
  ancestor chain, not the root `/Pages` node alone.** `/Rotate` and `/MediaBox`
  were read from the page dict and then from the root, so a base whose
  intermediate `/Pages` node carries `/Rotate 90` passed the rotation guard and
  every stamped widget landed a quarter turn off, and a page inheriting its
  `/MediaBox` from an intermediate node was flipped against the root's page
  height. The `/Kids` walk carries each page's ancestor ids, nearest first, and
  both readers consult the page dict then that chain (ISO 32000-1 §7.7.3.4).
- fix(pdfform): **flatten keeps the background's resources and its own
  `/Contents`.** `/Resources` is inheritable, so writing a fresh one onto a page
  that carried none shadowed the ancestor's dict and unbound every name the
  background stream selects; the effective dict is now resolved up `/Parent`,
  inlined onto the page and extended there. The drawn fonts take names free in
  that dict (`Helv2` where `Helv` is taken), since a second binding for a name
  the background uses rebinds it under a last-wins parser. A `/Contents`
  reference naming an *array* object expands to its elements instead of being
  wrapped, which had left an array as an element of the `/Contents` array.
- refactor(pdf): **one ancestor chain per page.** `PdfUpdate::resolve_pages`
  returns `Vec<Page>` rather than page ids: each `Page` carries its `/Pages`
  ancestors from the `/Kids` walk and resolves any inheritable attribute
  through `Page::inherited_attribute`. Flatten reads `/Resources` through it
  instead of climbing `/Parent` on its own, so rotation, media box and
  resources answer from the same chain under the same cycle and depth guards.
- fix(core): **`Quill::resolve` keeps a mis-shaped container value raw rather
  than blanking it under the document's own label.** A seed the render
  coercion cannot conform — `rows: abc` on an `array`, `addr: 5` on a typed
  dictionary, a list where a variant container belongs — was rebuilt from the
  schema anyway, so the row showed an empty container still tagged
  `authored`. The container arms now compose an absent or already-shaped seed
  only, and anything else falls through to the keep-raw path
  `conform_card_render` documents. The render gate refuses the shape, so the
  plate is unchanged.
- fix(content): **a U+2028 or U+2029 in document text becomes a space.**
  Typst's lexer reads both as line breaks, so one mid-paragraph reopens
  `at_start` and the characters behind it are read as a block marker:
  `"intro\u{2028}- item"` rendered a bullet, `- item` on its own line. No
  escape reaches them — a `\` before whitespace is Typst's own linebreak — so
  the separators join `\r` and the bidi controls as characters the content
  forbids, refused by `validate` and replaced at every text ingress:
  `from_plaintext`, markdown import, and an `Op::Insert` through
  `apply_text_delta`. A space rather than a drop, both being Unicode
  whitespace, so the words either side stay parted.
- fix(content): **a change bundle whose `retain`/`delete` counts sum past
  `usize` is a base mismatch, not a panic.** `Delta::expected_base_len` summed
  the counts unchecked, so a host-authored bundle carrying
  `{"retain": 18446744073709551615}` aborted in debug and wrapped in release,
  where the wrapped total let `apply` slice past the base. The sum saturates,
  and the saturated length exceeds any real base, so `try_apply` and
  `apply_field_change` return the `DeltaBaseMismatch` the contract already
  names. No wire or API change.
- fix(content): **`diff_import` carries unknown marks forward beside anchors.**
  The rebase loop matched `MarkKind::Anchor` alone, so a full-document rewrite
  through the stale-text writer lane dropped every open-set mark, even one over
  text the rewrite left untouched. It now rebases every non-formatting mark —
  formatting is what the fresh import re-derives, and the rest lives in the
  content but not in markdown — so an unknown tag and its attrs survive a
  revise the way an anchor does.
- fix(content): **a code span whose content touches its fence exports with the
  CommonMark space pad.** Export emitted `fence + content + fence`, so an edge
  backtick joined the fence run (text `` `a `` came back as ` ```a`` `) and a
  span that begins and ends with a space lost one off each side on re-import
  (`" a "` came back as `"a"`). A pad space now flanks the content in exactly
  those two cases, which import strips back off; a span of nothing but spaces
  is exempt from the strip and so stays unpadded.
- fix(content): **a mark flanking an unknown island's placeholder survives
  export.** The verify-and-drop net re-imports the rendered line and expects
  the line's own text back, island slot included, but a type this build has no
  projection for renders as a comment placeholder that re-imports as nothing.
  The probe could never match, so every `**` / `*` / `~~` on such a line was
  dropped as unrepresentable. The expected text omits the slots of islands with
  no markdown projection and keeps every other one, so the probe measures
  delimiter leakage alone and still reads an image's slot back.
- fix(content): **a heading, an island and a rule take no continuation lines.**
  `segment` grouped a `continues` line into the block above, but export renders
  only the first line of those three kinds, so `SetContinues` on the line after
  a heading validated clean and exported `"# a"` with the continuation dropped,
  while the Typst emitter still rendered it. `LineKind::takes_continuations`
  names the kinds a continuation is legal after (`Para`, `Code`, `Unknown`);
  `SetContinues` refuses the write (`ApplyError::ContinuesSingleLineBlock`),
  `normalize` clears a flag `SetKind` or a stored document left there, and
  `validate` rejects a hand-built one (`Invariant::ContinuesSingleLineBlock`),
  the repair-or-refuse split `ContinuesAcrossContainers` already carries.
- refactor(content): **code-block import filters its text through
  `Inline::push_text`.** `push_code_line` differed from it only in dropping a
  `\n` where `push_text` spaces one, and its segments come from a
  `split('\n')`, so the two agree on every input it receives.
  `change_bundle_from_value` reads `delta` with a single lookup and
  deserializes it from the borrowed `Value` rather than a clone, and
  `op_array`'s absent and null cases are one `Option::filter`.
- fix(core): **a new `$` entry lands after the preceding `$` line's inline
  comment, not between the line and its comment.** `Payload::upsert_meta`
  inserted one past the last lower-ranked `$` item, which is the index the
  trailing comment occupies, and emit reads a trailer as belonging to whatever
  item precedes it: `$quill: q@1.0 # note` with no explicit `$kind` emitted
  `$kind: main # note`. The insert now steps over that comment, so the four
  callers — the `$kind` synthesis on parse, `store_ext`, `store_seed_overlay`,
  `set_quill_ref` — leave the trailer on its own key and `parse(to_markdown())`
  holds.
- fix(core): **a field name is ASCII as written, not as it normalises.**
  `is_valid_field_name` ran NFC before matching `[A-Za-z_][A-Za-z0-9_]*`,
  which no non-ASCII name can pass except a canonical singleton that composes
  to ASCII: `store_field("\u{212A}elvin", …)` was accepted, emitted verbatim,
  and re-read as a nested key, so the document did not survive
  `parse(to_markdown())`. The check reads the name's own characters, matching
  the raw bytes the parser's key grammar accepts.
- fix(core): **`Card::store_fields` refuses a `!must_fill` marker targeting a
  mapping, as `Card::store_field` does.** The batch checked the field-name
  grammar and value depth only, so a `QuillValue` carrying a marker on a
  nested object node was stored, emitted with the marker dropped, and refused
  by the `@0.92.0` storage DTO on reload. `edit::validate_fill_targets` runs
  per field in the same all-or-nothing pass, so the offending name rides the
  batch's error vector as `edit::fill_on_mapping` beside every other
  violation. The WASM `storeFields` inherits it.
- fix(core): **a variant container the document wrote reads `authored`
  whichever rung filled its discriminant.** `resolve()` lifted a present
  container off the blank rung only, so `classification: {}` reported
  `default` where the schema declared one and `authored` where it did not —
  the reported rung turning on the schema rather than the document. A present
  container is `authored`, as any other present value is; a present-null still
  reads as absent and keeps the discriminant's rung. The value the row carries
  is unchanged.
- fix(core): **a nested richtext `example:` reaches the blueprint as its
  `# e.g.` hint.** A richtext cell never inlines its example, and the
  per-property builder for typed-dict properties and typed-table rows gated the
  hint on `default:` alone, so a defaultless richtext property's `example:`
  appeared in neither the cell nor a hint. Both gates are one helper now, and a
  property's `example:` behaves as a card-level field's does at every depth, as
  BLUEPRINT.md § Typed dictionaries states.
- fix(core): **a type mismatch names the field's own declared type, so `date`,
  `datetime` and `enum` report themselves.** The validator collapsed the three
  onto `string`, so `due: 20260101` against `type: date` read "schema declares
  `string`. Either provide a value of type `string` or change the schema's
  `type:` to `integer`" — a type the schema does not declare and an exit that
  discards the field's format. The declared type now has one source,
  `FieldType::as_str`, which the schema-literal path already re-derived for its
  own messages. `validation::type_mismatch` carries the name in `args.expected`,
  so a consumer reading that key sees `date`, `datetime` or `enum` where it saw
  `string`.
- fix(core): **a `$seed` overlay cell is validated as the document value it
  is.** `validate` judged each cell as a Quill.yaml schema literal, a context
  that refuses the container spelling of a variant-bearing enum and reads a
  present-null as a typed value, so `classification: { value: CUI, note:
  hello }` and `author: null` each drew a `validation::type_mismatch` warning
  while `seed_card` committed both. An overlay cell is what `seed_card` writes
  into the new card, so it takes the same pass a card's own fields take: the
  container is a spelling it accepts, null reads as absent, and a mistyped
  cell still warns. Overlays stay advisory and never gate render.
- fix(core)!: **the §8 count caps report a count, not a byte size.** The
  card-count and per-block field-count caps raised `parse::input_too_large`,
  whose one message shape is `Input too large: {size} bytes (max: {max}
  bytes)`, so 1001 fields read as 1001 bytes and the `size` arg rode out under
  a code whose canon row says bytes. Each cap has its own variant and code:
  `ParseError::TooManyFields` / `parse::too_many_fields` and
  `ParseError::TooManyCards` / `parse::too_many_cards`, both carrying `count`
  and `max`. `parse::input_too_large` keeps the two byte caps, document size
  and YAML payload size. A consumer routing count overflow on
  `parse::input_too_large` reads the two new codes instead.
- refactor(core): **`set_values` refuses a kindless card slot through the card
  constructor rather than a hand-built error.** `plan_slot` carried an early
  return minting `edit::invalid_kind_name` for a position holding no card and
  naming no kind; flattening the kind to `Option<&str>` sends that case to
  `build_card`, where `Card::new("")` raises the same error at the same
  `cards[<i>]` path.
- refactor(core)!: **`ParseOutputFormatError` is `#[non_exhaustive]` and gains
  `new`.** It was the one `pub`-field type in core's root modules without the
  attribute COMPATIBILITY.md's struct rule asks for. Its field stays readable;
  an out-of-crate struct literal becomes `ParseOutputFormatError::new(input)`.
- fix(pdf): **a base PDF whose trailer `/Size` sits above `i32::MAX` is refused
  rather than panicking mid-stamp.** `alloc_id` bounded only `u32` overflow, so
  a `/Size` in `2^31 ..= 2^32-2` handed out ids that cast to a negative `i32`
  and pdf-writer's `Ref::new` panicked ("indirect reference out of valid
  range") — a crafted `form.pdf` opened cleanly and then took down the process,
  and with it a WASM module. `alloc_id` stops at `i32::MAX`, and every id that
  becomes a reference goes through a checked `to_ref`, which also covers the
  base page ids that never pass through `alloc_id`. The refusal carries the
  existing `pdf::write` id-space error.
- fix(pdfform): **a flattened value asserts black fill and the default text
  state before it draws.** The appended stream opened with `q` and set only
  `Tf`, and a page's `/Contents` array is one stream, so a background's
  unpaired `0.9 g` or `3 Tr` — a shaded field box, a scanned form's invisible
  OCR layer — carried into the drawn value and rastered it near-white or
  blank, with no diagnostic. Both writers now open with
  `0 g 0 Tr 0 Tc 0 Tw 100 Tz 0 Ts`, the state the stamped `/DA` starts from.
  SVG/PNG/canvas only: the AcroForm PDF deliverable is stamped, not flattened,
  so no artifact changes.
- fix(core): **the default `field_at` hands a tie to the later-painted
  placement.** It ranked `regions()` with `min_by`, which keeps the first of
  equal distances, so two placements on one rect resolved to the one
  underneath — against the documented contract and against the Typst backend,
  which overrides `field_at` with later-wins. The default now walks `regions()`
  in reverse, so a pdfform quill whose widgets overlap answers with the
  last-stamped one. A backend that mixes widget and content regions through the
  default overrides `field_at` to hand a widget the tie, as Typst does.
- fix(typst): **an image in a content field resolves a quill asset.**
  `![logo](assets/logo.svg)` in a `richtext` field lowers to `#image(..)`
  inside the helper package's `lib.typ`, and Typst resolves an image path
  against the root of the file holding the call, never leaving it; assets sat
  under the project root alone, so the one thing an image island lowers to
  failed the render outright. `QuillWorld` registers each `assets/` file under
  the helper package's file id as well, one `Bytes` behind both ids, and a
  content image names an asset by its quill-root path, rooted or relative, the
  path a plate names it by.
- fix(typst): **a `form-field` widget's rect is the box it prints in, whatever
  the layout context.** The helper emitted its `<__qm_field__>` metadata beside
  the box rather than inside it, and a tag's own position is the line's baseline
  inline and the flow cursor's left edge in a block: an inline widget reported
  a rect one box-height low, and one under `#align(center, ..)` or
  `#align(right, ..)` reported the left margin. The tag rides in the box body,
  whose origin is the box's top-left in every layout context, so
  `session.regions()`, `fieldAt`, and the stamped AcroForm `/Rect` all land on
  the widget. A plate that compensated for the offset shifts by that much.
- refactor(typst): **a compile's form fields cross to the PDF spine as one
  derivation.** `Compiled` carries `field_specs: Vec<FieldSpec>` built with the
  compile: `widget_regions` is `regions_of` over it and `render_document_pages`
  stamps it directly, where each PDF render had rebuilt the specs from the
  placements, page-height scan included. A placement naming a page outside the
  document now fails the compile alongside the extraction errors it sits with,
  instead of emptying the session's regions and surfacing at render.
- fix(fuzz): **the wide-payload property requires the parse to succeed and to
  keep every field.** `fuzz_decompose_large_payload` swallowed a parse `Err`
  and asserted `payload().len() <= size`, a bound `Payload::len` cannot
  exceed, so a parse that refused the input or dropped every field passed. It
  expects the parse and pins `len() == size`, which holds at every generated
  width since all sit under `MAX_FIELD_COUNT`. The README's `parse_fuzz.rs`
  row names what the file generates and `conform_fuzz.rs` gets the row it
  lacked.
- refactor(core): **every surface that refuses a non-content richtext value
  spells one sentence.** `Codec::decode_field` builds the shape-mismatch
  message and names the shape that arrived (`expected a richtext content
  object or a markdown string, got a number`); the wire `$body` reader and the
  richtext write coercion route through it instead of carrying their own
  copies, so the schema-bound read and the strict projection name the shape
  too. `Card::store_ext` bounds `$ext` depth through
  `value::depth_check_meta_map`, the check the wire and the storage DTO run.
  Every diagnostic code is unchanged.
- refactor(core): **`FieldViolation` spells its own message, once.** The parse,
  wire and storage boundaries each re-spelled the three field-invariant
  reasons, and the wording had drifted apart ("invalid data-field name `x`:
  field names must match…" vs `invalid field name "x": must match…` vs a keyless
  "field names must match…"). `Display` gives the reason alone — the form
  `WireError::InvalidField` carries beside its own `key` — and
  `FieldViolation::message(key)` names the key inline, which all three
  boundaries wrap. The parse path's dead `FillOnMapping` arm goes with the
  match it lived in: `validate_field` returns `InvalidName` / `TooDeep` only.
- refactor(core): **the prescan's frame stack carries no `kind`.**
  `Frame.kind` and `FrameKind` were written at every push and read nowhere
  but the write that filled them in, so both are gone, along with the
  `ensure_frame_at_indent` parameter that carried the value.
- docs(content,pdf,core): **six internal docs state what the code does.**
  `NestingTooDeep` and `SetContainers` name the Typst emitter as the recursive
  one, markdown export walking an explicit stack; the PDF reader's input
  contract reads `bounded-tree`, a `/Pages` tree of any depth reaching each node
  once and staying under 100 000 nodes; `flat_nested_comments` links
  `from_items_with_flat_nested`, which resolves; `build-wasm.sh` puts the
  `rm -rf pkg` on this script never removing files, CI caching the wasm build
  dir alone. `spec_conformance_probe` names its body case for the parse pass
  that does the strip and covers `normalize_document` with a field-name NFC
  case. The hint on a malformed `main.body` lists `unsupported`, reading the key
  list off one const beside `BodyCardSchema`.
- fix(cli): **`render` parses a `MARKDOWN_FILE` through the bound door, so
  `conform::*` and `plate::unsupported_construct` warnings reach stderr.** The
  command called `Document::parse`, the transport door, which runs neither the
  conform walk nor the declined-construct walk: a `usaf_memo` body carrying a
  `***` rendered with an empty stderr where every other surface warns. It calls
  `Quill::parse` instead, and the existing splice carries that door's warnings
  into `RenderResult.warnings` unchanged. Rendered bytes are the same — the
  coercion pass already ran in `compile_data` — and a `$quill` naming another
  quill refuses at parse rather than at compile, with the same diagnostics and
  the same exit 1. The seeded path (no `MARKDOWN_FILE`) is unchanged.
- fix(cli): **`validate` states each failure once.** A failing run printed its
  own summary and then a second copy through the error it returned, which
  `main` labelled `[ERROR] Invalid argument:` — including on a load failure,
  which is a quill config failure and not an argument the caller got wrong. The
  command returns `CliError::Reported` once it has written the per-diagnostic
  lines and the summary, and a load failure carries the loader's `RenderError`
  the way every other subcommand does. Exit status stays 1 on every path.
- fix(cli): **`schema -o` and `blueprint -o` create the parent directories
  their path names, as `render -o` does.** Both wrote with a bare `fs::write`,
  so `schema ./q -o nested/dir/s.yaml` failed with "No such file or
  directory" while the same path under `render -o` succeeded. The CLI's
  `write_output` splits into `write_stdout(bytes)` and `write_file(path,
  bytes, announce)` — dropping the unreachable arm that refused neither
  stdout nor a path — and all three `-o` flags route through `write_file`.
  `schema` and `blueprint` pass `announce: false` and stay silent on stdout;
  `render` announces unless `--quiet`.
- refactor(quillmark): **`Quillmark::render` resolves the backend once.** It
  called `supported_formats` and `open`, each resolving the quill's backend
  separately; it now holds the resolved backend and asks it for both. Same
  diagnostics in the same order.
- fix(cli): **`-f PDF` writes `example.pdf`.** `--format` parses
  case-insensitively, but the derived output filename, the `example.<fmt>`
  fallback and the multi-page `--stdout` refusal interpolated the flag as
  typed, so `-f PDF` wrote `example.PDF` and `-f Svg` named `Svg` in its
  message. All three read the parsed format's lowercase id.
- fix(core): **a card fence with CRLF line endings parses as its LF twin
  does.** The prescan splits the fence body on `\n`, so every CRLF line
  reached the matchers with a trailing `\r`: a bare `x: !must_fill` matched
  neither spelling the fill-tag stripper accepts, so the marker was read as an
  unknown tag — dropped with a `parse::unsupported_yaml_tag` warning, a second
  `parse::fill_marker_unsupported_position` warning, a `null` value and an
  `x: null` emit — and a lone `-` opening a sequence item was not one. The
  scan strips one trailing `\r` per line up front, so the cleaned YAML and
  every captured comment are `\n`-only.
- fix(core): **`key: !must_fill` written inside a block scalar or a quoted
  scalar is that scalar's text, and warns about nothing.** The
  `parse::fill_marker_unsupported_position` check re-read every cleaned line
  after the prescan, block-scalar bodies among them, and accepted any `:` plus
  whitespace as the tag's left boundary, so `note: |` over
  `see key: !must_fill here` kept the literal and still reported a marker
  lost. The prescan now decides the warning per line as it reads one, where a
  block-scalar body and a quoted value are known for what they are; the four
  positions the marker genuinely cannot survive — a flow map, a flow
  sequence, a bare sequence element, a flow value at depth — warn as before.
- fix(core): **a field write clears a root `!must_fill` bit riding on the
  value it is handed.** The payload item's own `fill` flag is the one carrier
  of a root marker — emit, the wire and the storage DTO all read it there —
  while `QuillValue::set_fill_at(&[])` marks the value tree's root, and a
  value stored after that call kept both bits, disagreeing. `Payload` clears
  the tree's root bit on insert, so `is_fill` and `QuillValue::fill` answer
  alike and a document compares equal to itself across the markdown and the
  storage round-trip, where the split state emitted one document and compared
  as another. Parse skips a nested-fill path naming only its own key, the
  route reaching the same split from source.
- fix(core): **a comment between a bare `-` and the item's first key stays
  inside the item.** The prescan records such a comment against the item, but
  emit wrote it above the `- ` line, where it re-parses as a comment on the
  sequence — a parse-emit-parse inequality on input the parser accepts without
  a warning, settling only on the second emit. A sequence item whose mapping
  carries an own-line comment before its first key emits in the bare-dash form,
  the comment and the mapping indented under it, so the first emit is the fixed
  point. A blueprint's typed-table row takes that form too, its first
  property's description sitting inside the row with the rest.
- fix(pdf): **a `/Rotate` the reader cannot read as an integer refuses the
  stamp.** `assert_unrotated_pages` parsed the raw value and treated a failure
  as an absence, so `/Rotate 7 0 R` — legal for any dict value — climbed past
  the page and fell to the default zero, stamping every widget in unrotated
  user space onto a page the viewer turns. The first *present* value along the
  ancestor chain now binds: a direct integer is checked as before
  (`pdf::rotated_page` when it is not a multiple of 360), anything else is
  `pdf::parse`, matching how `/MediaBox` treats an unresolvable value.
- fix(cli): **`--quiet` silences `--verbose` entirely.** Seven of the nine
  `--verbose` lines in `render` printed regardless of `--quiet`, only the two
  after the compile checking it, so `--verbose --quiet` still narrated the
  load and the parse. `execute` resolves the pair once up front, and the help
  text's "Suppress all non-error output" is what the flag does.
- refactor(core,typst,pdfform): **the default PPI is stated once, as
  `RenderOptions::DEFAULT_PPI`.** Each raster backend carried its own
  `DEFAULT_PPI = 144.0` const, one of them documented as mirroring a core
  constant that did not exist, so changing the default meant editing three
  files. `RenderOptions::ppi_or_default()` resolves the option against that
  constant and both backends call it. Additive on the core API; the resolved
  value is unchanged.

- fix(core): **body prose left inside a card block is told to close the block,
  not to wrap itself in a block scalar.** A closing `~~~` placed after the prose
  body fails YAML on the first prose line, and `simple key expected` answered
  every such line with the wrapped-scalar advice: rewriting the memo as
  `body: |` keeps the body inside the block and fails again. The hint now reads
  the flagged line the parser names — no `key:` outside quotes, sentence-shaped
  or after a blank line, and not the tail of an unfinished `key: value` — and
  names the real fix: the line reads as prose, and body text belongs after the
  closing `~~~`, so close the block before it. A genuine plain scalar wrapped
  onto a second line keeps the block-scalar hint.
- fix(core): **a leading space before a top-level key gets its own hint.** One
  stray space folds the line into the preceding plain scalar, and YAML raises
  the same `mapping values are not allowed` an unquoted `:` inside a value
  raises — so the hint sent the reader hunting for a colon that is not in the
  block, and four models quoted the `subject:` above it instead. Where the
  flagged line starts with a space, reads as `key:` or `key: value`, and
  follows a column-zero key line, the hint names the space: top-level fields
  begin at column 0, remove the one before `date:`. Anything else keeps the
  quote-the-colon advice.
- fix(pdf): **a dict ending in a hex string parses to its real `>>`.** The
  scanner stepped over literal strings and `%`-comments but read a hex string
  as ordinary bytes, so the string's own `>` abutting the dict's `>>` closed
  the dict one byte early: `<< /T <41>>>` read as ` /T <41`, every
  `find_dict_value` on that inner swallowed the rest as one hex string, and a
  `/Producer` stamp rewrote the `/Info` with an unterminated `<…` — a title
  lost to any reader. `skip_string_or_comment` steps a `<` that no `<` follows
  to just past its `>`, which the dict, array and `endobj` scans all inherit.
  The trigger is real: pdf-writer's compact mode, which krilla and typst-pdf
  use, writes a non-ASCII `/Title` or `/Author` exactly this way.

## v0.112.0 - 2026-09-01

- feat(content)!: **every vocabulary member spells its payload in `attrs`.** The
  canonical content had two spellings for one thing: a built-in put its payload
  in named siblings (`{"kind":"heading","level":1}`), an unknown put it in one
  opaque bag, and which a document used depended on whether the build that wrote
  it happened to know the name. Promoting a name from unknown to built-in was
  therefore an encoding change, and five mechanisms existed to bridge the split.
  Now `Heading{level}` ⇄ `attrs.level`, `Code{lang}` ⇄ `attrs.lang`, `Link{url}`
  ⇄ `attrs.url`, `Anchor{id}` ⇄ `attrs.id`, and `ListItem`'s
  `ordered`/`start`/`ordinal` ⇄ the same three under `attrs`, with the envelope
  keys (`kind`/`type`/`container`, `containers`, `continues`, `start`/`end`,
  `instance`) staying siblings and an empty bag omitted. The Rust types are
  unchanged — typed in memory, uniform on the wire. `fold_legacy_attrs`, the
  `reject_*_attrs` family, `MarkKind::ord` and its placement rule, and
  `RESERVED_*`'s wire role are all deleted; the canonical mark tie-break becomes
  `MarkKind::sort_key`, the `(type, attrs)` pair the wire carries, which two
  builds compute identically whether or not either knows the member. **Stored
  documents need no migration**: the decoder reads the old spelling wherever the
  `attrs` bag is absent. That is the load-bearing half — a `richtext` field
  rests as a content object inside an opaque payload value carrying no schema
  tag, so most stored content reaches no migration and a walk cannot safely find
  it (`$ext` is arbitrary host data by contract). The fallback is frozen: unlike
  the fold, a promotion neither grows it nor inherits it. The **break is at the
  seam**: a host reading `line.level` / `line.lang` / `mark.url` / `mark.id` or a
  list item's shape reads them under `attrs` (the TS narrowing types move too, so
  `typecheck` reports it), and the authored lane — `overwrite`, `install`,
  `CardInput.body`, and the op wire — *rejects* the old spelling rather than
  reading it, so a stale write throws instead of landing somewhere it did not
  aim. A foreign bag beside a built-in becomes legal in exchange, dropping unread
  as it always did in effect. Canonical bytes move (`attrs` on built-ins, and
  coincident marks reorder on the new tie-break), so **content hashes recompute
  once**; generated Typst nests coincident wraps the other way for identical
  glyphs, and `exportMarkdown` nests them the same way (`**~~x~~**` becomes
  `~~**x**~~`, parsing back to the same content), so goldens and any hash over
  either projection move too. A consumer holding bare seam JSON outside a
  stored document gets a hard break, having no tag to dispatch on. The storage
  tag becomes `quillmark/document@0.112.0`; `@0.93.0` rows migrate forward on
  read. See [0.111 → 0.112](docs/migrations/0.111-to-0.112.md).
- feat(core)!: **`fieldAt` and `positionAt` take a pointer tolerance, and
  resolve to the nearest ink rather than the first containing it.** A glyph's
  box is its run's ink height by its own advance, so a text column answers over
  a fraction of the area it occupies: the boxes of one line abut, but the
  leading between two lines is inside the paragraph and on no glyph. An 11pt
  paragraph is live over about two thirds of its own height at default leading
  and under half of it double-spaced, which is the whole of why clicking the
  preview to place a caret misses. Both queries now take `tol` in PDF points
  (`tolPt`, optional, on the WASM seam) and answer with the nearest placement
  within it, a radius in both axes. The caller derives it from the scale it drew
  the page at, slack being a property of the pointer rather than of the
  document: a tolerance fixed in points shrinks under the cursor exactly as the
  target does. Ranking by distance rather than growing each rect is what keeps
  the answer the nearer item's — outset boxes overlap, and a first match over
  them decides by paint order — and makes the tolerance a pure widening:
  containment is distance zero, so no point that resolves exactly changes answer
  however high `tol` goes, and later-painted still wins a tie.
  `RenderedRegion::distance` is the measure, and `contains` is now stated in
  terms of it. The break is the added argument on
  `SessionHandle::{field_at,position_at}` and their `LiveSession` forwarders,
  which a type checker reports; `0.0` is the previous behaviour exactly. The
  WASM argument is optional and defaults to `0`, so no JS caller changes.
- fix(content): **markdown export escapes what markdown strips at a line's
  edges.** A line's leading and trailing space/tab crossed verbatim, so
  `from_markdown(to_markdown(rt))` dropped it — and where the leading run
  opened an indented code block, or hid a `- ` / `# ` / `> ` / `N. ` marker
  from the position-0 escapes, the re-import rewrote the line's kind and
  containers with it: `"    foo"` returned as `"foo"` under `Code`, and
  `"   - item"` as `"item"` inside a `ListItem` nobody wrote. Neither takes
  adversarial input, since `apply_text_delta` and `from_plaintext` both mint
  edge whitespace: a `Codec::Plaintext` field holding an indented sample was
  corrupted by `Document::to_markdown`. The verify-and-drop net could not catch
  it, being gated on flanking marks — a mark-free line never reaches it, and
  the `,…,` probe wrapping the lines that do blocks every leading-block
  construct by design, so it reads such a line as safe while the `**` ships
  into the text. Space and tab at an edge now cross as character references
  (`&#32;` / `&#9;`), markdown having no backslash escape for whitespace. A
  literal `&` still escapes, so text authored as `&#32;` returns as itself. An
  image `alt` is the one edge run a reference cannot carry — the parser trims
  alt after decoding it — and joins the module's documented codec limits.
  Emitted markdown moves for content holding an edge run.
- fix(content): **a `=` run on a continuation line stays text.** It underlined
  the paragraph line above it into a setext heading, taking the hard break's
  `\` into the text with it: `"abc\n==="` returned as an H1 reading `"abc\"`.
  `\=` joins the block starters escaped at position 0.
- fix(content): **an unknown island's type cannot leave its placeholder.**
  `emit_island` wrote `island_type` — an open wire string no lane constrains —
  raw into `<!-- island:… -->`, where a `-->` closes the comment early and a
  line break ends the HTML block, either way leaking the rest as content text.
  At column zero that opens whatever the rest spells, a `~~~` card fence
  included, which the document layer reads as another card. The type crosses
  with `<` / `>` as entities and control characters as spaces; nothing reads it
  back out of markdown.
- fix(content): **an inline unknown island keeps its line whole.**
  `fix_html_comment_fences` inserted a newline after any `-->` carrying text
  behind it, but only a comment opening a line — at most three spaces of indent
  — starts an HTML block. One reached mid-line is inline HTML and swallows
  nothing, so an inline island turned `ab` into `"a b"`. The repair fires for
  the block-opening case alone.
- fix(core): **the payload ingresses refuse what no parse can produce.** The
  wire `TryFrom`, the storage DTO and `push_card` / `insert_card` each admitted
  a payload whose markdown does not read back: a duplicate field key, more
  fields than `MAX_FIELD_COUNT`, a repeated `$` entry, a comment whose text
  spans lines, and a composable card carrying `$quill` or `$seed`. The comment
  is the one that fails quietly — `#` opens a single line, so a text of
  `"hi\ninjected: pwned"` emits bare YAML after the first line and re-reads as
  a field nobody wrote. The rest emit markdown the parser rejects, and a
  composable `$quill` also trips the `debug_assert` in `from_main_and_cards`,
  which the rebuild behind `compile_data` runs: a render panicked on a debug
  build where it owed an error. `PayloadViolation` carries the verdict —
  `FieldViolation`'s seam one level up, reading the item list rather than one
  item — which the wire maps to `WireError::InvalidPayload` and the DTO to
  `StorageError::Malformed`, so both boundaries share one message. The
  `$quill` / `$seed` half is positional, since a `CardWire` is equally how the
  main card is read back, so it sits at placement beside the `$kind` gate as
  `EditError::RootOnlyEntry` (`edit::root_only_entry`). The DTO refuses a root
  `$kind` other than `main` and synthesises an absent one, as the parser does.
  Additive: `validate_payload`, `PayloadViolation`, `MetaKey::ALL`. Both error
  enums are `#[non_exhaustive]`, so no compiling caller changes, and no
  document the format calls readable is refused.
- fix(cli): **`render -f svg` / `-f png` writes every page.** The Typst backend
  emits one artifact per page and the command wrote `artifacts.first()`,
  discarding the rest with no warning, so a multi-page document produced a
  single-page file that looked complete. A multi-page render now writes one
  numbered file per page: `out.svg` becomes `out-1.svg`, `out-2.svg`, ….  Page
  one is numbered too, so no unnumbered file claims to be the whole document.
  `--stdout` carries one artifact and refuses a multi-page render.
- fix(core): **a `!must_fill` marker on a mapping is refused at every ingress,
  not just at parse.** `Card::store_fill`, the `CardWire` boundary and the
  `@0.92.0` storage DTO took `fill: true` against an object and emitted
  `x: a: 1`, which `Document::parse` then refuses — breaking the emit round
  trip. The rule the parser enforces now sits beside the other field invariants
  as `edit::validate_fill_targets`, raising `edit::fill_on_mapping`. A canonical
  content object stays legal: emit projects it to its markdown scalar first,
  which is the shape `!must_fill` emits against.
- fix(core): **a comment after a nested key that needs quoting keeps its
  position.** A comment's position is its index among its mapping's children,
  and the prescan matched only a bare `[A-Za-z_][A-Za-z0-9_]*` nested key. So
  `"a b": 1`, which the emitter writes itself, was not counted, and every comment
  after it in that mapping round-tripped one slot early. The prescan reads a
  nested key in both spellings the emitter writes at depth: quoted, and plain
  with characters the bare form excludes.
- fix(pdfform): **an array-bound text widget is multiline.** `resolve::coerce_text`
  joins an array's elements with newlines unconditionally while the widget took
  `multiline` from `ui`, defaulting to false: a viewer collapsed the `/V` at its
  first line while the flattened SVG/PNG stacked every line, so the interactive
  PDF and the raster disagreed.
- fix(core): **a version segment is plain digits, and a `$quill` `@` carries a
  selector.** `u32::from_str` accepts a leading `+`, so `memo@+2.+1` parsed as
  `Minor(2, 1)` and `version: "+1.0"` loaded, though `quill_ref_hint` promises
  digits and neither spelling re-`Display`s to itself; and `memo@` yielded an
  empty selector silently read as `latest`. An absent selector still means
  latest.
- fix(core): **an unquoted numeric `version:` keeps its fraction.** The loader
  accepts a YAML number by intent but converted it through `f64::to_string`,
  which drops the fraction: `version: 1.0` became `"1"` and failed validation
  with a hint naming the `'1.0'` the author had written. An unquoted `1.10` is
  the YAML number `1.1` before the loader sees it, so VERSIONING.md's quote-it
  rule now names that rather than the `x.0` case.
- fix(python): **a value with no JSON form raises instead of storing its
  `repr`.** `py_to_json_at` fell through to `str()`, so a tuple stored
  `"('a', 'b')"`, a `bytes` stored `"b'...'"` and a `set` stored `"{'x'}"` —
  silently, surfacing as garbage at render or read-back. `datetime.date`,
  `datetime.datetime` and `datetime.time` keep the stringified form the
  fallback existed for; everything else is a `ValueError`, which is what the
  WASM lane already refuses.
- fix(python): **the type stub declares `Diagnostic.args`**, the localization
  channel the class exposes. Under a type checker `diag.args` was an attribute
  error on a `@final` class.
- docs(python): **the error contract names both classes the binding raises.**
  `errors.rs` claimed every raised exception is `QuillmarkError` carrying
  diagnostics, while an argument the binding cannot convert raises `ValueError`
  — the behavior the binding's own tests pin. Engine refusals carry diagnostics;
  an unconvertible argument raises `ValueError` before the engine is called.
  Stated in the module doc and in the error-handling guide.
- fix(wasm): **`new Document(ref)` refuses an invalid reference with the code
  and hint `setQuillRef` attaches.** The same input was classified two ways by
  one binding: `parse::invalid_quill_reference` plus the canonical grammar hint,
  or a bare message. Both doors mint it through one helper now.
- fix(typst): **a literal `;` after emitted inline markup survives to the
  page.** Typst's markup parser reads a semicolon directly after an embedded
  code expression as that expression's terminator and renders nothing, so
  ``Use `--force`; otherwise`` lowered to `#raw("--force"); otherwise` and
  dropped the character. `continues_expr` guarded `(` and `.ident` for the same
  reason; it guards `;` now, and the emitter writes the same `\` before it.
- fix(typst): **document text reaches the page as the characters it holds, not
  Typst's substitutions for them.** `escape_markup` escaped `~`, whose lexer
  shorthand is a non-breaking space, and left the rest of that class active:
  `pages 3--5` rendered an en dash, `wait...` an ellipsis, `-5` a minus sign,
  and `-?` an invisible soft hyphen, taking both authored characters off the
  page. A mark decided it too, since Typst reads the text behind one as a fresh
  token: `x-5` stayed literal but `**x**-5` lowered to `#strong[x]-5`, a minus
  sign the content never held. Each shorthand's head is escaped now, which is
  enough — what one leaves behind is too short to re-form it. **Smart quotes
  stay.** `'` and `"` are an element with a set rule, not a lexer shorthand, so
  a quill picks its own typography with `#set smartquote(enabled: false)`; the
  emitter escaping them would settle that for every quill with no way back.
  Documents holding a dash pair, an ellipsis or a signed number render
  differently.
- fix(typst): **an island that renders as nothing no longer joins the text
  either side of it.** An island type this build does not know, and an empty
  table island, emit no markup, so the two text runs their slot separated abut
  in the output — where the escapers, which run per text run, see one side of
  the join at a time. `a/`, such an island, `/b` wrote `a//b`: a Typst comment
  that swallowed the rest of the paragraph. The emitter guards that seam with
  the same `\` it writes at a line anchor and an expression tail.
- fix(typst): **the caret one past a field's last character resolves to the last
  glyph, not the paragraph's first.** `Scan::locate` admits the end position but
  `forward_pos` matched runs half-open, so the most common caret position while
  typing fell through to the segment's generated start. A position no run
  contains now resolves against the nearest preceding run.
- fix(typst): **a vendored package whose manifest declares a non-semver
  `version` or an unusable `entrypoint` path warns instead of failing the
  session.** Both aborted `QuillWorld::new` even when the plate never imports
  that package, where an unparseable manifest, an unusable asset path, and a
  missing entrypoint already warn and carry on. A non-semver `version` skips the
  package; an unusable `entrypoint` path keeps the files already loaded and
  skips only the check that the entrypoint is among them.
- fix(wasm): **`RenderResult` crosses a diagnostic's `args` as the
  `Record<string, unknown>` it declares.** It returns through tsify's ABI, whose
  default serializer emits a `Map` for a map-typed field, where every
  hand-serialized diagnostic path passes `serialize_maps_as_objects(true)`.
  `RenderResult.warnings` puts the document's parse warnings ahead of the
  render's own, and a `plate::unsupported_construct` parse warning carries
  `args`: a quill declining a construct its body still holds renders
  successfully, and `args.construct` read back `undefined`. `RenderResult` and
  `Diagnostic` declare `hashmap_as_object`, and a `const` assertion holds each
  to it.
- fix(content): **a table island whose `aligns` or a row is not an array
  normalizes to one the store can reload.** `normalize_table_props` repaired a
  non-array `header` but fell through for the other two, while `table_shape_error`
  read them as width 0 — so an island op carrying `{"aligns": "bogus"}` was
  accepted and serialized, and the exact bytes then failed to reopen with
  `TableAlignsMismatch`. Normalizing a table now makes its shape check pass, as
  `KnownIslandType::normalize_props` promises.
- fix(content): **a code block's `lang` is reduced to an identifier on the
  storage lane and refused on the authored one.** `sanitize_lang` ran on import
  only, so the storage decode and the `setKind` op wire took any string and
  `emit_code` wrote it onto the fence header verbatim: a `lang` of
  `"rust\ninjected line"` exported a content line the document never had, and a
  backtick made the fence illegal. The storage decode sanitizes, so a blob
  written that way still opens; the authored wire raises a shape error, since
  the host is writing now and the repair would be silent.
- fix(core): **a variant-bearing enum refuses a non-string `default:` at load,
  as a plain enum already does.** The scalar branch applied the render floor's
  leniency to a schema literal, so `default: 1` against `values: ["1"]` loaded
  clean and then selected nothing: `selected_member`, `resolve_variant_sourced`
  and the absent-discriminant fallback all read the default through `as_str`, so
  the field compiled to the blank world instead.
- fix(core): **a seeded variant container commits at its resting form.**
  `seed_parts` runs every other field through the strict write, but
  `seed_variant` pushed its assembled container straight through, so a `$seed`
  overlay for a richtext cell rested as raw markdown and the next `conform`
  rewrote bytes on a document nobody edited — the divergence the shared write
  exists to prevent.

## v0.111.0 - 2026-08-30

- feat(wasm): **`mapMarks(content, bundle)` answers where a `ChangeBundle`'s
  text-moving channels leave a field's marks**, the coordinates its `markOps`
  are written in. Each of `delta`, `islandOps` and `lineOps` rebases the marks
  already in the field — a range's `start` takes assoc `after` and its `end`
  `before`, a zero-width mark takes `before` — and that rule reached the
  boundary only as a comment on a private method, so an editor deciding which
  `markOps` to emit had to reimplement it, and one that read the range rule as
  the whole rule drifted an anchor a character on text typed at the anchor's own
  position. `Content::map_marks` and `Content::apply_field_change` walk one
  channel list, so the prediction and the store cannot answer a position
  differently, and the answer is normalized as the store's is, so a bundle
  carrying no `markOps` names the marks the field will hold. The rule is stated
  on `ChangeBundle` and in `BINDINGS.md`.

- fix(core): **a `plaintext` field declared `inline: true` keeps the flag on the
  declaration wire.** `FieldSchema::serialize` projected the flag back out of
  the type enum with a `RichText { inline: true }` match only, so
  `type: plaintext, inline: true` serialized as `{"type":"plaintext"}` — WASM
  `quill.schema()`, the Python binding, and the CLI `schema` command all lost
  the single-line constraint, and a serde round-trip degraded the field to
  `inline: false`.
- fix(core): **a comment on a CRLF line no longer carries its `\r` into the
  emitted document.** The prescan splits on `\n`, so a trailing- or own-line
  comment slice ran to end-of-line including the `\r`; it rode through the DTO
  and wire and was written back verbatim, against `to_markdown`'s "line endings:
  `\n` only". Field values were never affected. A `\r` *inside* a comment still
  reaches emit.
- fix(facade): **loading a path that names no directory says so.** The walk
  answered a missing root with an empty tree, so `quill_from_path("/typo")`
  failed later with `Quill.yaml not found in file tree`, pointing at the
  bundle's contents instead of the path. Python's `Quill.from_path` surfaced
  that directly; the CLI's pre-checks are gone but for `validate`'s
  missing-`Quill.yaml` one, gated on the directory existing so it names the
  bundle a real directory lacks without shadowing the loader's answer for a
  typo. `validate`'s load-failure summary names no file, that branch covering a
  missing directory too.
- fix(content): **`LineOp::SetKind` refuses a heading level outside `1..=6`.**
  The arm checked kind/text agreement but not the level, so a Rust caller could
  apply `Heading { level: 9 }` and leave a content whose `validate()` fails and
  whose export emits `#########` — read back as a literal-hash paragraph on the
  next import. The JSON wires already range-checked it. New
  `ApplyError::BadHeadingLevel`.
- fix(typst): **diagnostic columns count characters, not bytes.** Any multi-byte
  character earlier on the source line inflated the reported column, which an
  editor reads as a jump target.
- fix(pdf): **a non-finite widget `/Rect` is refused (`pdf::bad_rect`) rather
  than written.** `form.json` rect values deserialize as plain `f32` and
  saturate to `inf`, and `flip_rect` arithmetic can reach `NaN`; pdf-writer
  prints a non-finite float verbatim, so `stamp` returned `Ok` with `inf`/`NaN`
  tokens in the output — no PDF number grammar admits them. `flatten` guards the
  geometry it draws. Matches the posture `font_size` already took. `regions_of`
  is still unguarded.
- fix(pdf): **a base PDF that already carries an `/AcroForm` is refused
  (`pdf::existing_acroform`).** The catalog rewrite appended a second
  `/AcroForm` key without looking, leaving a dict the spec does not define and
  the old form's widgets live in the preserved page `/Annots`. Stripping was
  already the documented authoring rule; it is now checked.
- fix(pdf): **`fonts_used` registers only the faces a `/DA` names.** Only `Text`
  and `Choice` widgets write one, so a checkbox or signature spec carrying
  `Times`/`Courier` emitted an unreferenced Type1 object and a dead `/DR /Font`
  entry into every stamped PDF.
- refactor(core): the unreachable null arm in the `Date`/`DateTime` coercion is
  deleted (`conform_value` returns on any null before the type match), and
  `Version` derives the ordering its field order already spells.

## v0.110.0 - 2026-08-26

- **breaking** wasm: **the seam spells a container's `instance`, so the read
  type can require it and every write lane reports an omission.** 0.109 gave
  `Container` the discriminator that tells one container from an adjacent
  sibling of identical shape, and left it an obligation no checker asked for;
  the op lane got a requirement, the whole-`Content` lane — `overwrite`,
  `CardInput.body`, the one a codec flattening a tree writes through — did not.
  It could not: one encoder served storage and the bindings, writing the key
  only where it was non-zero, and a field a read may omit is a field a write
  cannot require. `serial::to_seam_value` is that encoder with every `instance`
  spelled, and the bindings take it on every lane typed `Content`
  (`reader.getContent{,At}`, `getStored` on a body, `importMarkdown`, `rebase`,
  and the card wire behind `document.main` / `cards` / `card(i)` / `removeCard`
  / `makeCard` / `seedMain` / `seedCard`). `ContentContainer.instance` becomes
  required, `ContentContainerInput` is deleted, and `LineOp.setContainers` takes
  `ContentContainer` — a net-smaller surface than 0.109's. The break is every
  hand-built container literal, which is the one a type checker does report.
  A checker still cannot report a `0` stamped on every run, which is the write
  that welds them: `assignInstances` is the rule, not the type. Storage keeps
  the omission and stored blobs re-encode byte for byte; so do the render lanes,
  and so does what the seam types `unknown` (`getStored` on a field,
  `PayloadItem.value`), which answers with the stored bytes verbatim. Content
  parsed from a stored document is the one shape that needs a cast.
  [Guide](docs/migrations/0.109-to-0.110.md)
- fix(core): **`Quill::validate` refuses only what the render floor refuses.**
  Validation ran its own read-side type dispatch over the authored value while
  the render door validated the *coerced* one, so five of seven types had values
  that rendered and were fatally `validation::type_mismatch` at once — a bare
  scalar for an `array`, `"3"` for an `integer`, `1` for a `boolean`, a
  length-1 array for a `string` or `date`. `airmark`'s `usaf_memo@0.2` declares
  `letterhead_caption` as an `array`, and the bare scalar a starter template
  spells it with — a valid spelling of a one-element list — audited as fatally
  invalid across every document seeded from it, each rendering correctly.

  `validate_value` now conforms each document value through `conform_value` at
  `Leniency::Render` before judging it, so a type has one predicate and **a
  fatal `validation::*` diagnostic means the document does not render**.
  Conforming runs per node, so one refused element no longer mistypes its
  siblings: `counts: [true, "abc"]` under `integer` items is one mismatch, at
  `counts[1]`. Two consequences for a consumer routing on codes: a value the
  floor adopts raises nothing where it previously raised
  `validation::type_mismatch`, and a bare scalar the floor stringifies into an
  `enum` field is now domain-checked on that string, so `grade: 5` against
  `values: [alpha, beta]` is `validation::enum_violation` where it was
  previously silent — the diagnostic the render door already raised. Schema
  literals (`example:`, `default:`) stay strict.

## v0.109.1 - 2026-08-24

- fix(typst): **a `field-region` claim around inline content no longer widens
  the line.** Each of the helper's two bracketing markers was written
  `#metadata(..) <__qm_region__>`, and the space before the label survived into
  the inline flow. It cost 2.715pt per marker at a 12pt body size, so a claim
  on a date or any mid-paragraph composition shifted the text around it by
  5.43pt, against the layout-neutral contract the helper documents.
  `form-field`'s marker carries the same shape and drops the space too, rather
  than keep depending on the `box` that follows it.

## v0.109.0 - 2026-08-24

- **breaking** content: **`Normalized` is the precondition the projections
  require, and every codec returns one.** A projection over a container tree
  owes totality, and `Content` alone does not say whether `normalize` has run —
  so `to_markdown` and `emit_content` each trusted a canonical shape their
  signature did not ask for. `Content::into_normalized` is the mint (infallible:
  canonicalizing is total, and the codecs go on calling `validate` after it),
  `Normalized::into_content` the way back out, and the token derefs to
  `&Content`, so **a read-only consumer needs no change** — `.text`, `.lines`,
  `.marks`, `.islands`, `validate()`, `is_inline()` all reach through. What
  moves is the signatures a caller names or a value it mutates:

  | Crate | 0.108 | 0.109 |
  |---|---|---|
  | `quillmark-content` | `from_markdown` / `from_plaintext` / `from_canonical_json` / `serial::from_canonical_value` / `serial::from_authored_value` → `Content` | → `Normalized` |
  | | `to_markdown(&Content)` | `to_markdown(&Normalized)` |
  | | `Content::to_canonical_json` | `Normalized::to_canonical_json` |
  | | `serial::to_canonical_value(&Content)` | `(&Normalized)` |
  | `quillmark-typst` | `emit::emit_content(&Content)` | `(&Normalized)` |
  | `quillmark-core` | `Card::body() -> &Content` | `-> &Normalized` |
  | | `Card::overwrite_body(Content)` / `overwrite_field(_, Content)` | `impl Into<Normalized>` — a `Content` still passes |
  | | `TypedReader::get_content{,_at}` / `CardReader::get_content{,_at}` → `Option<Content>` | `Option<Normalized>` |

  A consumer that *mutates* a decoded content takes the round trip, which is
  what the codecs used to run for it silently:

  ```rust
  // 0.108
  let mut rt = from_markdown(md)?;
  rt.marks.push(mark);
  rt.normalize();

  // 0.109
  let mut rt = from_markdown(md)?.into_content();
  rt.marks.push(mark);
  let rt = rt.into_normalized();
  ```

  The op channel needs none of that: `apply_text_delta`, `apply_mark_ops`,
  `apply_line_ops`, `apply_island_ops` and `apply_field_change` are forwarded on
  `Normalized` and re-establish the invariant on the error path as well as the
  success one. `to_plaintext` still takes `&Content` and reads a token through
  the deref, projecting `text` alone with no walk to make total.

- feat(content): **`quillmark_content::traverse` is where the container walks
  live**, `runs` (adjacent lines sharing one container instance at a depth),
  `items` (adjacent lines whose whole container is equal) and `segment` (a line
  plus the continuations at its own nesting). Five call sites across three
  crates had spelled these by hand, each with its own idea of when a run ends;
  `Span` and the walks are public so a consumer reading `Content.lines` groups
  them the way both projections and the quill census do.

- perf(content): **`serial::to_canonical_value` and `to_canonical_json` take a
  `Normalized`.**
  Both cloned the whole content and normalized the copy on every call, on a
  lane whose callers — the codecs, `Card::body`, the storage DTO — were already
  holding the canonical form. The token now carries that, so the serialize path
  spends an encode instead of a deep clone plus a repair pass. `to_canonical_json`
  moves from `Content` to `Normalized` with it; a caller holding a raw `Content`
  mints first, which is what the old body did for them silently.

- fix(core): **a document body that `validate` refuses is refused on write, not
  discovered on read.** `CanonicalContent`'s `Deserialize` parsed, normalized and
  validated; its `Serialize` validated nothing, and `Card::overwrite_body` takes a
  caller's content on the canonical-form token alone. A store could therefore
  accept bytes it could not read back. The serializer now validates too and fails
  with the invariant, at the boundary that cares and while the caller still holds
  the value that produced it.

- refactor(content): **the leaf-segment walk is one loop, not two.** `traverse`
  gains `segment` — the block-opening line plus every following one that
  continues it at the same nesting — and `export::emit_block` and the Typst
  emitter's `segment_end` both call it. The fifth duplicated traversal, the one
  #1364 did not list.

- fix(content): **`to_markdown` no longer aborts the process on a deeply nested
  content.** `Normalized` states that `normalize` has run, and `normalize`
  repairs where `validate` rejects: nothing about canonicalization brings a
  container path under `MAX_NESTING_DEPTH`, so a hand-built `Content` mints a
  token that `validate` refuses. `export::emit_block` recursed one frame per
  container level and overflowed the stack a few thousand levels down — a
  SIGABRT no caller can catch, against a Typst emitter that checks the depth up
  front and returns `EmitError::NestingTooDeep` for the same input. The walk is
  now an explicit frame stack, as `json_depth_exceeds` and the quill census
  already are, so the projection is total over every token its signature
  accepts. `Normalized`'s docs settle the half the newtype does not close: the
  token promises canonical, not valid — the mint stays infallible, the codecs go
  on calling `validate` after it, and a projection that takes one owes totality
  rather than trust. Only a Rust embedder hand-building a `Content` reaches the
  shape; every decode lane (`from_markdown`, `from_canonical_value`, storage,
  WASM, Python) rejects the depth already.

- fix(typst): **a container inside a list item no longer terminates the list.**
  The item's continuation indent reached its leaf path only, so a quote inside
  an item opened at column 0 — where Typst ends the enclosing list. The item's
  later blocks came back as top-level paragraphs and the next item started a
  fresh list, which renumbers an ordered one from the quote on. A fence and a
  nested list escaped it by reaching that indented leaf path; a transparent
  unknown container did not, and neither would any container added later.
  Indentation is now the walk's rather than each construct's: one rule opens
  every block, leaf and container alike, at the enclosing list depth, so what
  the content nests, the markup nests.

- fix(content): **a `continues` line that crosses a container boundary no longer
  survives.** A within-block break lives inside one container, and `LineOp::Join`
  mints the crossing shape whenever it merges two lines of differing paths — the
  line after the seam keeps continuing across it. Both projections already read
  the flag as dead there (`export::emit_block` and `emit::segment_end` each
  require the depth to match before absorbing a continuation), so `normalize`
  now clears it, which states what was already true and changes nothing
  observable. `Content::validate` gains `Invariant::ContinuesAcrossContainers`
  to catch a hand-built content that skipped `normalize`, and
  `LineOp::SetContinues` refuses the *deliberate* crossing up front with
  `ApplyError::ContinuesAcrossContainers` — the same repair-or-refuse split the
  line-kind rule already makes. This was the one relational line invariant
  nothing checked: `validate` is otherwise strictly per-line, while every
  container rule is a property of a line pair.

- fix(content): **two adjacent containers of one shape are no longer read as
  one.** Container identity is the container path plus contiguity, and the path
  carried nothing to tell one instance from the next, so two adjacent runs of
  equal shape welded: `[Quote], [Quote]` read as a single two-paragraph quote,
  and two one-item lists as a single item whose second line came back as an
  unnumbered continuation paragraph — the marker gone. `Container` now carries
  an `instance` discriminator on every arm, `Content::normalize` canonicalizes
  it to `0` (flipping to `1` only where the adjacent preceding run would
  otherwise weld), and the two projections read it. Four defects close with it:
  - `from_markdown("- a\n\n<!-- -->\n\n- b")` — the CommonMark idiom for
    spelling two lists apart — no longer destroys the second list's marker.
  - Two adjacent ordered lists typeset with their own numbering. They reached
    the Typst emitter as one run and `+` markers numbered the second list on
    from the first, so `1. 2.` / `1. 2.` rendered **1 2 3 4**. The run's first
    item now states its number, which resets Typst's running counter. Every
    ordered run's first item therefore lowers as `N. ` where a run starting at
    1 lowered as `+ `; the page is identical, the generated markup is not, so
    anything diffing or golden-comparing Typst source sees it.
  - `1. a` beside a list starting at `3` keeps that `start` through the
    Markdown projection. CommonMark reads only a list's first number, so
    `1. a\n\n3. b` re-imported as one list of two items and the `start` was
    lost — breaking the round-trip fixed point `export` documents. Adjacent
    lists now alternate their marker (`-`/`+`, `.`/`)`), which is how
    CommonMark itself spells two lists apart, so the boundary survives the
    projection with no comment marker in the authored file.
  - Adjacent `Unknown` containers of equal `(tag, attrs)` round-trip as two
    **through storage**, the lane they have: an unknown container has no
    Markdown syntax to alternate, so it projects transparently there as it
    always did. The open-set promise that a container this build does not know
    survives untouched is now total rather than holding up to an adjacency
    quotient.

  An item boundary is a parent boundary: two inner lists under two outer list
  items are two lists, so an inner run restarts its `ordinal` and needs no
  discriminator. `ordinal` is canonicalized alongside it, to a gapless 0-based index within
  its run, so `[5, 9]` and `[0, 1]` stop being two spellings of the same two
  items. `instance` is written to the wire only when non-zero, so a stored row
  that needs no discriminator — nearly all of them — keeps its exact bytes and
  its content hash. **Breaking for Rust consumers** that match `Container`
  exhaustively: `Quote` is now a struct variant, and `ListItem`/`Unknown` carry
  the extra field.

  On the TypeScript surface `instance` is optional — the wire omits a zero, so a
  read shape cannot require it — and nothing there stops compiling. A consumer
  that only reads needs no change. **A consumer that writes container paths owes
  the field**, which is every codec flattening a tree: two `bullet_list` nodes in
  a row are adjacent same-shape siblings, and omitting the discriminator lands
  them welded. Such a host keeps producing what it produced on 0.108, so the
  four defects above stay open for it until its codec stamps the discriminator.
  Nothing reports the omission. Two adjacent lines with equal paths are one
  container, which is also how a two-paragraph quote is spelled: the model
  cannot tell a boundary a writer meant from one it did not.

  The block census counts what the projections see, so two adjacent runs of one
  shape now count two where they counted one: a quill declining `list` or
  `quote` reports the construct at a document that has two of them where it
  reported one, and `plate::unsupported_construct` moves with it.

  A blob written here carries `instance` only where a document holds adjacent
  same-shape siblings, and a reader that predates the field ignores the key —
  so such a blob loads on 0.108 with the two runs welded, and re-saving there
  drops the boundary for good. A 0.109 host whose codec drops the key on
  write-back loses it the same way, with no version skew involved. The boundary
  is written by whatever produced the row — `from_markdown`, the CLI, any Rust
  caller — and survives only as far as the next writer that carries it.
  The `@0.93.0` tag is unchanged because every blob written before this release
  re-encodes byte for byte; the forward direction is the one that costs, and
  only for the documents that spend the key.

- fix(blueprint): **a variant's `object` or `array<object>` cell expands per
  property.** The cell went through the scalar path, so it rendered as
  `controlled_by: !must_fill # object` — a null where the schema wants a
  mapping, with every property's description, `default:` and type annotation
  dropped, and the marker on a path the obligation predicate never warns at.
  A cell is a field of its container and now expands as one, like every other
  surface already did.
- fix(core): **a `.quillignore` pattern holding more than one `*` ignores what
  it names.** The matcher handled exactly one wildcard and returned no match
  for the rest, so `**/*.tmp` and `*.sublime-*` were dead lines. Patterns now
  compile once through `glob::Pattern`, matched against the whole path and the
  basename. Two readings tighten to gitignore's: `*` stops at `/`, and a
  pattern spelling out a `/` anchors at the bundle root rather than matching
  any path that opens and closes with its halves. Both narrow what a line
  ignores, so a bundle can gain a file it used to drop: `assets/*` covers
  `assets/logo.png` and no longer `assets/icons/logo.png`, which `assets/**`
  or the directory line `assets/` covers. No in-tree quill spells either shape.
  A line always ignores the
  name it spells out as well: `[` opens a character class and is an ordinary
  character in a filename, so `Cinzel[wght].ttf` ignores both the variable font
  of that name and the class it describes.
- refactor: **`RenderError::coded(code, message)` is the one constructor for a
  single-error-diagnostic failure.** Nine sites across five crates spelled
  `from_diag(Diagnostic::new(Severity::Error, msg).with_code(code))` by hand,
  two of them as a per-crate `engine_err` helper the backends each carried
  their own copy of. Additive to `quillmark-core`'s public API; no code, message
  or shape changes.
- refactor(pdfform): **a session holds its flattened PDF parsed, not as bytes
  each render path reparses.** Flatten and parse now happen together in `open`
  and `update`, the two places `field_specs` are set, so the derived flat PDF
  moves only with the specs it comes from and `render_svg`/`render_png`/
  `render_rgba` paint parsed pages. A malformed flatten now surfaces from the
  call that produced it under one code, `pdfform::flat_parse_failed`, replacing
  the per-format `pdfform::svg_parse_failed` and `pdfform::png_parse_failed`
  raised at render time (neither documented, and both reachable only through a
  bug in this crate's own flatten). Opening a session fails on that bug now,
  including for a caller that only ever renders the AcroForm PDF, which is
  stamped from the base and reads nothing flattened.
- perf(pdf): **filling a PDF form no longer slows down with the size of its
  background or its page count.** Reading one object from the base walks every
  byte of it — the live copy is the last revision, so a scan cannot stop early
  — and nothing memoized that, so a stamp or flatten pass paid O(pages) whole-
  file scans and the live-edit path repaid them on every keystroke. The base's
  object offsets are now collected in one pass and each read is a lookup: a
  20-page 300 KB form stamps in 0.7 ms rather than 37 ms, flat in page count.

  **breaking** in `quillmark-pdf`: `PdfUpdate::begin` and
  `PdfUpdate::resolve_pages` take the `&ObjectIndex` the caller builds over the
  base rather than its bytes, and `reader::find_object_bytes` /
  `reader::object_dict` become `ObjectIndex::object_bytes` / `ObjectIndex::dict`.
- **breaking** content: the op wire is a reading direction. `mark_op_to_value`,
  `line_op_to_value` and `island_op_to_value` are removed from
  `quillmark-content` — an op bundle is authored on the JS/Python side and
  reaches Rust through `change_bundle_from_value`, so nothing in the workspace
  ever emitted one and every wire change was made twice, once in code no product
  path executes. The decoders are unchanged. Their round-trip tests become
  decoder tests over literal JSON, which is what the wire actually is: an
  encoder agreeing with its own reader never proved the shape a binding sends.
- perf(content): **canonical serialization stops rebuilding the tree it just
  built.** `to_canonical_value` normalized a copy — which already recursively
  key-sorts every opaque bag reachable from it (island `props`, an unknown's
  `attrs`) — and then ran a whole-tree `sort_keys_owned` over the encoded
  result, re-collecting and re-allocating every object and array in the document
  to reorder the handful of fixed keys the encoders insert themselves. The
  encoders now emit those keys in ascending order and the terminal pass is
  `canonicalize_keys`, which scans and returns when the tree is already
  canonical. Canonical bytes are unchanged, byte for byte; a tree that somehow
  arrives unsorted is still repaired rather than shipped.

  The public `container_to_value` and `mark_to_value`, and the crate-internal
  `island_to_value`, now emit their own keys in a different order. An unknown's `attrs` bag is
  untouched, as in 0.99, and nothing hashes the op wire.

## v0.108.3 - 2026-08-21

- fix(typst): **a paragraph holding one bare `/` renders instead of failing the
  compile.** Typst's heading `=`, list `-`/`+`/`N.`, and term `/` markers fire
  on a space after them *or* on the line ending there; the emitter's
  line-anchor guard tested only for the space, so a run that was one bare
  marker reached Typst unescaped — `/` as a term list whose colon is missing
  (`expected colon`), the other four as an empty heading, bullet or enum item.
  The guard now takes Typst's own test, and covers a list item's body head as
  well as column 0, that being a line start the parser reads as one.

- fix(typst): **bold text, a table cell or an indented paragraph opening with
  `-`, `=`, `+`, `/` or `N.` renders as that text.** Typst reads the head of
  every content block `[…]` as a line start of its own, so the marker in
  `**/ x**` or in a table cell reached it as a term list whose colon is missing
  and failed the compile, while `**- x**` drew a bullet list inside the bold.
  Indentation is trivia and holds that line start open behind it, so a
  paragraph beginning `  / x` failed the same way. The line-anchor guard now
  covers every position Typst reads as a line start.

- fix(typst): **text directly after inline code, bold or an image renders when
  it opens with `(` or `.name`.** Typst reads a `(` directly after a `#…`
  expression as that call's arguments and a `.` before an identifier as a field
  access, so the emitter's own `#raw(…)`, `#strong[…]` and `#image(…)` handed
  the text behind them to Typst as code — `` `x`(y) `` became a call on
  content, which fails the compile. Such a run now takes the same `\` prefix
  the line-anchor guard uses. Debug builds parse every emission with Typst's
  parser, so markup that reaches it as syntax fails a test rather than a render.

## v0.108.2 - 2026-08-20

- fix(core): **storage blobs tagged `@0.81.0` and `@0.82.0` load again.** Both
  tags migrate forward on read instead of failing as an unknown schema version.
  Migrations chain (`V0_81_0 → V0_82_0 → V0_92_0 → V0_93_0`); the write path is
  untouched, so re-serializing a migrated row emits `@0.93.0`.

  The `V0_82_0` hop is **lossy in one place**: the `$id` payload item is
  dropped, the live model having no counterpart for it. The alternative is
  refusing the row. A consumer that kept a key under `$id` re-establishes it
  under a `$ext` namespace it owns.

  This reverses the #929 retirement, whose two premises were both wrong. Rows
  do predate `@0.92.0` — #1327 reports stored `@0.81.0` ones. And `0.82.0` was
  not yanked: every published Quillmark version is live on crates.io, npm, and
  PyPI. Because no yank happened, `@0.82.0` names a shape
  union rather than a frozen format — every release from `0.83.0` through
  `0.91.0` stamped it — so the restored reader accepts the union (#1327)

- refactor!: **one definition per mechanism across the rust crates.** A
  whole-codebase simplify pass: the typst session compiles through one
  `recompile` pipeline whose derived tables (regions, span windows, page hashes)
  are built per commit instead of per query; the pdf reader's scan plumbing
  (`object_dict`, `open_trailer`, `dict_end`, `ws_end`) and the page-dict array
  splice are single definitions shared by stamp and flatten; the flattened PDF
  is shared by `Arc` instead of copied per render; emission, prescan, payload
  assembly and validation in core lose their duplicated dispatch and their
  per-keystroke deep clones. Two outputs shift: a blank pdfform document returns
  the base PDF unchanged rather than appending a revision carrying two
  unreferenced font objects, and three `quillmark-pdf` parse-failure messages
  share one `dict not parseable` spelling (codes unchanged). Dead public API is
  deleted: `FileTreeNode::{file_exists, dir_exists, list_files,
  list_subdirectories}`, `Payload::contains_key`, `PayloadItem::nested_comments`,
  `QuillValue::{string, integer, bool, null}` (the `From` impls and `from_json`
  are the constructors), `FieldSpec::{with_schema_field, with_value,
  with_tooltip}` (assign the fields), and `quillmark-content`'s
  `strip_bidi_formatting` / `fix_html_comment_fences` (internal to
  `normalize_markdown`). None had a caller in the workspace or bindings.

- fix: **an empty mapping emits as `{}` rather than losing its key.** `emit_field`
  dropped an empty-object field entirely, which composes with the block form of
  its parent into markdown no parser accepts: a `$ext` whose every namespace
  emptied wrote `$ext:` with no children, and re-parsed as null. That is the
  `parse::invalid_structure` a `@quillmark/svelte` host hit on save, once a
  tips-card dismissal had cleared `$ext.editor.tips`. One level down the same
  rule ran silent: `$ext` is the only slot type-checked, so an inner mapping
  turned null and the document parsed clean, handing a consumer back a type it
  never wrote. Plain user fields carried that loss too — `cfg: {opts: {}}`
  re-parsed as `cfg: null`, and the second emit then differed from the first.
  Empty mappings now emit `key: {}` at every depth. That is the spelling a
  sequence item (`- {}`) and a wholly empty `$ext: {}` already used, so an
  emptied container survives the round-trip as the value a consumer stored.

## v0.108.1 - 2026-08-19

- fix: **a content cell under `variants:` is readable at its codec.**
  `schema_at`, the walk behind `reader.get_content_at`, stepped `items` and
  `properties` where conform steps `variants` as well, so a key into a variant
  container fell to the catch-all and answered `edit::field_not_content` naming
  `enum` — for a cell the same config declares `plaintext`. Such a cell stored,
  conformed, seeded, validated and rendered, and nothing could read it back, so
  no consumer could mount a content editor over it. The walk unions the worlds,
  so a cell of a world that is not live reads absent rather than raising, as a
  stale row index does; a name no world declares is `edit::unknown_field`, and a
  variantless enum stays a scalar. The write is unchanged — a variant container
  has no per-cell op address and needs none, committing whole.

## v0.108.0 - 2026-08-18

- fix: **the value ladder is cut per cell, so the plate is total at every
  depth.** An absent container returned a value instead of descending, and
  everything below it was decided by that one branch: an absent `contact` never
  reached `contact.email`'s own `default:`, and a container `default: {name: A}`
  crossed whole, so a declared property it omitted was **missing from the
  plate** — a direct Typst read of it a compile error, on an address
  `form-field` still binds. Two spellings of the same state disagreed:
  authoring `contact: {}` rendered the leaf defaults that leaving `contact` out
  did not, and `default: {}` — documented as expanding to the blank-filled
  shape — emitted `{}` with no declared key at all. Resolution is now a descent:
  a rung supplies a *seed*, and the same composition runs over it whichever rung
  it came from, so absence is inherited rather than terminal and each cell cuts
  its own ladder. A partial element inside an `array` `default:` is completed
  against `items` as an authored element is. The variant container already
  worked this way and stops being the special case.
- **breaking** a `default:`/`example:` on an `object` with `properties` is a
  load error (`quill::default_on_namespace`, `quill::example_on_namespace`),
  naming the properties that hold it. A quill declaring one loaded before, so
  the upgrade reads as a quill that stopped loading rather than as a fix; no
  in-tree quill declares one. A typed dictionary is a namespace, not a cell:
  the container literal was a second declaration of a value the property
  already holds, and the two axes read different ones — `default: {name: A}`
  rendered `A` while `must_fill` derives per property and still reported `name`
  unauthored. It was also unchecked, so `default: {nope: 1}` loaded and crossed
  an undeclared key to the plate. This is the variant container's rule
  (`quill::default_type_mismatch`) generalized; an `array` keeps its literal,
  since `items:` fixes the element type but never the arity.
- **breaking** `must_fill:` is retired: obligation is a reading of `default:`,
  never a declaration of its own. Declaring the key is a load error
  (`quill::field_parse_error`) naming the migration that field's shape takes,
  `FieldSchema::must_fill()` is `default.is_none()`, and the raw
  `FieldSchema.must_fill` field is gone. Four of the five legacy declarations
  restate the derivation and migrate by **deletion**: `must_fill:` on a typed
  dictionary (a namespace carries no obligation — its leaves do), `must_fill:
  true` with no `default:`, and `must_fill: false` beside one. `must_fill:
  false` with no `default:` becomes `default: <the type's blank>` (`""`, `[]`,
  `0`, `false`) — already the corpus's most common `default:`. The fifth,
  `must_fill: true` beside a `default:`, is the one behavior deleted and the
  one judgment call: keep the `default:` to render the value unasked, or move
  it to `example:` to keep the ask. An example fills the blueprint cell the
  default vacated, seeds *carrying* the `!must_fill` marker where a
  `default:`-only field seeds nothing, and never renders — so an untouched
  document renders the blank rather than asserting a value nobody chose. For a
  `string` or `enum` the blueprint bytes are identical either way; three shapes
  are not. A `richtext` example never inlines, so its cell becomes a bare
  marker and the value survives only as the `# e.g.` hint. An
  `integer`/`number`/`boolean` blank is indistinguishable at the plate from an
  authored zero. On a variant container the two targets select different
  worlds: `default: CUI` renders the CUI world and obliges its cells, while
  `example: CUI` leaves the discriminant blank. Also removed:
  `quillmark:must_fill` from the transform schema, which is the wire *validity*
  contract, and an unauthored must-fill cell is wire-valid by design; the
  declaration view carries `default:` for a consumer that wants to derive.
  No in-tree quill declared the key and the declaration view emits only what an
  author wrote, so no emitted JSON changes for any real quill — the WASM
  `QuillFieldSchema` TS interface loses `must_fill?: boolean`, a compile-time
  break for editors typed against it.
- fix: **seeding descends into a container's `example:`.** A dictionary with no
  `example:` of its own seeded nothing, so a property's `example:` was
  unreachable at every projection — the render floor never emits an example, and
  the blueprint is a different document. A seed is now composed from whatever
  its cells commit, sparse at every depth, and stays absent when none of them
  commit anything. Markers ride the cell they belong to.
- fix: `resolve()`'s rung is honest for a container. It has no rung of its own,
  so it reports the strongest that contributed: `authored` when the document
  wrote any of it, else `default` when a cell below resolved to one, else the
  floor. An absent container over defaulted cells read `blank` while rendering
  those defaults, which is the fact an editor ghosts from. Nothing inside a
  container the document did not author reads `authored`. A variant container
  counts its live world's cells the same way, so writing one of them lifts a
  container whose discriminant fell to the schema's `default:`.

- chore(deps): the Typst floor moves to 0.15.1. The workspace already resolved
  there under the 0.15.0 caret; the pin now names the version the tree is built
  and tested against. `pdf-writer` stays at 0.15.0, still the version
  `typst-pdf` → `krilla` forces and the newest published.
- docs: `0.107-to-0.108.md`, the guide for this step. It leads with the two load
  errors, since both reject the quill rather than the document, and gives
  `must_fill: true` beside a `default:` — the one behavior deleted — the space
  its judgment call needs. `BLUEPRINT.md` § "Typed dictionaries" loses the `{}`
  expansion and the container-literal renderings with the cascade that produced
  them, and states the nesting the 0.107 collapse admits.

## v0.107.0 - 2026-08-17

- fix(typst): `display(field, ..)` validates its address against the schema, the
  assert `form-field` and `field-region` already carry. It is the one helper keyed
  by address rather than by value, and it was the one accepting an address the
  schema does not have: `display("issed", "[year]")` compiled, drew nothing, and
  reported nothing — the failure a plate author is least placed to see, a card
  address being a string the plate builds by concatenating `$path`. `_qm-display`
  cannot catch it, carrying an entry per *present* date, so a blank date and a
  typo are absent from it alike; `_qm-known-path` answers about the schema and
  tells the two apart. A known address carrying no date still returns `none`, so
  a `== none` fallback is unchanged: the assert is about the address, the `none`
  about the value.
- fix: **a container's own `default:` reaches the plate as content.** The render
  floor read `default_content` only for a `richtext`/`plaintext` leaf. An
  `object` or `array` carrying its `default:` on the container fell through to
  the raw literal instead, crossing as unimported markdown where every other
  content position delivers a canonical content object. The companion was
  already cached and never read. The floor now keys off the cache — present is
  the form to commit, absent over a content-bearing tree blank-fills — which
  covers leaf and container alike and drops the type test. `usaf_memo`'s
  `references` (`array<richtext(inline)>`, `default: []`) carried the same
  defect, invisible only because the list was empty.
- **breaking** a container-shaped `default:`/`example:` on a variant-bearing
  enum is a load error (`quill::default_type_mismatch`,
  `quill::example_type_mismatch`). A quill declaring one loaded before, so the
  upgrade reads as a quill that stopped loading rather than as a fix.
  The container is the shape a *document* writes. As a schema literal it cached
  no content form and yielded no discriminant, so the field blank-filled in
  silence as if nothing were declared. The diagnostic names the discriminant
  spelling instead, which is where a world's cells carry their own literals.
  Scalar literals are unaffected.

- feat: **every type nests at every depth**. A property or an element is an
  ordinary field, so it carries whatever a card-level field carries, itself
  included: `object<array<string>>`, `array<array<integer>>`, a typed table whose
  row holds a typed dictionary, and a variant cell holding either.
  `quill::nested_object_not_supported` and `quill::nested_array_not_supported`
  are gone, and `ShapePosition`'s three positions collapse to the one question
  the walk still asks — is this card level, where `variants:` and `ui.group` are
  the two keys that live. A widening: every quill that loaded before loads
  unchanged.
  The depth budget was what the flat address tables existed for.
  `SchemaMeta` carried six name-keyed tables (`array_fields`, `object_fields`
  and their card twins) so the helper's `_qm-known-path` could enumerate two
  suffix steps rather than derive a grammar; they are replaced by one address
  tree — the schema pruned to the steps it offers — that the helper and the span
  scan both walk, converging on the unbounded descent `pdfform::bind` always
  had. Three components deriving an address become one walk each side of the
  seam, and `quillmark/tests/address_grammar.rs` pins the two against the deep
  shapes as well as the shallow ones.
  `variants:` stays card-level, now on its own reasoning rather than by
  inheriting the depth ban: a variant's shape is a function of the schema *and*
  the discriminant, and the union projection, the once-bound form, the plate's
  single branch and `validation::out_of_variant` each hold because that gap is
  one level deep ([SCHEMAS.md](prose/canon/SCHEMAS.md) §"Enum variants"). An
  array-valued variant cell used to report `nested_array_not_supported`, whose
  message named array elements and object properties — neither the situation;
  the shape is now legal and `quill::variant_placement` is left saying only what
  it means.
- fix: `blueprint()` expands a container at every depth. `build_property_mapping`
  spent each property through the scalar builder, so a nested `object` or typed
  table rendered as `key: null # object` — its own properties, their markers and
  their annotations absent — where the same shape one level up expanded. It now
  recurses, and the card-level and nested paths are one implementation, so a
  `default:` covers its subtree identically wherever it is declared. No
  document changes shape: the shapes this fixes could not be declared before.
- fix: a content leaf's `default:` reaches the plate from **every** position it
  can be declared in, not only card level. The load pass that imports each
  richtext/plaintext literal into its companion cache walked the card's field
  map, so an `object` property, a typed-table row property and a variant cell
  each kept their authored `default:` and cached nothing; the render floor read
  the leaf's empty companion and blank-filled. A document authoring only the
  container (`dict: {}`, `rows: [{}]`, `c: {value: CUI}`) rendered correctly
  with the author's default missing, and nothing upstream had anything to
  report. The walk now recurses `properties` / `items` / `variants`, the shapes
  `field_contains_content` already descended, so such a document now renders
  **with** the author's default — a render-output change for any quill that
  declared one. Importing a literal is also what checks it, so a nested
  `richtext(inline)` violation — in a `default:` or an `example:` — now fails
  load as a card-level one always has, naming the leaf's declaration path.
  **Breaking on that second count**: the literal loaded before, so a quill
  carrying one stops loading. Nested `example:` *surfacing* was never broken:
  the blueprint prints the raw literal at every depth.
- **breaking** typst: a plate's direct read of a typed-table row cell regions on
  the cell (`refs.0.org`), where it regioned on the whole array before — a
  *wrong* address, not a missing one, routing a click on the org cell to the
  entire table. The span scan was the third component deriving a schema address
  and the one left at the one-level ceiling: 0.106 lifted the lowering walk and
  `_qm-known-path` to the row property, so the three no longer agreed, and the
  scan is the one that decides what a *read* is attributed to. It now takes the
  index step (`.at(n)`, the only spelling Typst has for an array index) and then
  the row property, gated on the `array_fields` table. Each step is its own
  address, so a whole-row read names the row (`refs.0`) and a primitive
  element's read names the element (`tags.0`); a negative index and an
  undeclared row key mint nothing and fall back as before. Consumers keying on
  the array's address for element ink see the narrower address instead. Explicit
  `field-region` / `form-field` claims are unchanged, and the alias lane keeps
  parity: `#let row = data.refs.at(0)` … `#row.org` regions on `refs.0.org`.
- test: one table pins the schema address grammar on both backends
  (`quillmark/tests/address_grammar.rs`), covering every position the nesting
  contract admits, each position's card twin, and the rejects that bound each
  step. `PLATE_DATA.md` promises a plate author that one address binds on
  either backend, and the grammar is written twice to keep it — an unbounded
  schema walk in `pdfform::bind`, an enumeration of the suffix forms in the
  Typst helper's `_qm-known-path` — reading two different projections of the
  same `QuillConfig`, so either side can move alone. `pdfform` exports
  `resolves_schema_address` (`#[doc(hidden)]`) so the pin can ask both the same
  question. A body address is the plate grammar's alone and is pinned as such.

- **breaking** typst: lowering dispatches on the schema node beside each value
  rather than on tables of top-level field names, so a declared type means the
  same thing wherever it is declared. A `date`, `richtext` or `plaintext`
  declared inside an `object` or an `array` row reached the plate as its raw
  wire value before — a bare string for a date, and for a rich field the
  *internal canonical-content JSON*, rendered as a Typst dict — while the same
  type one level up lowered correctly. Ten of the twelve nested positions the
  schema admits degraded that way, silently: core coerced and validated the
  value correctly, so nothing upstream had anything to report. `contact.note`
  is now a markup block, `contact.reply_by` and `rows.0.on` are `datetime`s,
  and `_qm-plaintext` gains the nested entries that closed the
  `plaintext(field)` escape hatch. The walk is the inverse of the one
  `build_transform_schema` builds the node with, so it cannot be shallower than
  the schema is.
- **breaking** typst: a `date` / `datetime` field lowers to a **native**
  `datetime`, not the `(value:, display:)` wrapper. `data.issued.year()`,
  `data.issued < data.due` and handing the field to a datetime-consuming
  package are ordinary Typst; `.value` and the paren form `(data.issued.display)(..)`
  are hard Typst compile errors, never a silent degrade, and all consumers are
  first party. A date has no canonical rendering the way authored text does —
  every rendering of `2026-01-02` is a typographic decision the plate owns — so
  it lowers to its value and reaches ink by address instead.
- feat(typst): `display(field, ..args)`, a date field's content projection,
  keyed by schema address rather than carried on the value. It places rendered
  ink whose glyphs are born in generated source, so a date formatted through a
  `#let` binding, a per-card loop variable, or a vendored package keeps a
  region on its schema field — the affordance the value-object existed to buy,
  now available to any date at any depth without shaping the value. `none` for
  a blank date, so a `== none` fallback still fires. The rule plates follow:
  want a value → `data.<field>`; want clickable ink → `display("<field>", ..)`.
- feat: a variant cell may carry **any type a card field may**, prose and dates
  included — `quill::variant_field_type` is gone. The load error existed because
  lowering read flat top-level name tables that could not descend into a
  container, so a `date` or `richtext` cell inside a variant would have loaded
  clean and reached the plate as its raw wire value; the schema-node walk reads
  the cell's own declaration, leaving the ceiling nothing to protect. Every value
  surface already descended per live-world cell — coercion through
  `conform_value`, validation through `validate_value`, the render floor through
  `resolve_value` — so the widening needed one real fix:
  `field_contains_content` returned `false` for a variant container on the
  strength of this very guard, which would have silently skipped the content
  companion caches, the resting-form conversion and the seed path for a variant
  content cell. It now answers on the union of the worlds' cells. Containers
  are included: "every type nests at every depth" lands in this same release, so
  a variant cell holds a typed table or a typed dictionary like any other
  position. `variants:` itself stays card-level (`quill::variant_placement`) on
  the reasoning stated there.

- **breaking** typst: the `plaintext(field)` helper and its `_qm-plaintext`
  table are removed. Shipped in 0.94 as the sanctioned content→`str` coercion,
  it never acquired a caller: no plate, no vendored package, and no binding
  surface referenced it, and the `create-auto-grid` consumer its own docstring
  cited passes an `array<string>` rather than a content field, so the only
  things exercising it were its three tests. It also carried a three-way name
  collision with the `plaintext` field type and that type's document-layer
  resting shape, which took a standing caveat in canon and the template to hold
  down. A plate that needs a `str` from a content field now has no route, which
  is the honest state of the requirement: reinstating it is additive and cheap
  when a plate actually asks.
- feat(typst): a typed table's row property (`refs.0.org`) is a writable
  schema address. `form-field(field:)` and `field-region` capped at one suffix
  step while pdfform's resolver descended unboundedly, so a shape
  `ShapePosition` explicitly admits bound on one backend only, against
  `PLATE_DATA.md`'s claim that one address binds on either. `array_fields` now
  carries each array's row property names, the same shape `object_fields`
  already had.
- fix(typst): a non-blank date the shared parsers reject raises
  `backend::invalid_date` from codegen rather than a pre-pass over top-level
  name tables, so the check covers every depth. Only a direct `apply` can
  deliver one; coercion parses the same way.
- feat(typst): a scalar read through a `let` alias regions on the address the
  chain it names would carry, so `#let c = data.classification` … `#c.poc`
  surfaces `classification.poc` where it surfaced nothing at all — not the
  container's address, absent. Binding a container once and stepping into it
  three times is the refactor 0.106's property addressing invites, and it cost
  the address silently, the document still rendering correctly. An alias holds
  only where the plate binds the name exactly once to one whole `data` chain: a
  name a second `let`, a closure parameter, a loop pattern, an import, or an
  assignment could rebind is dropped rather than risk attributing another
  value's ink to the field, and a wildcard import disqualifies every alias.
  Which name is followed is half the rule; which *occurrence* is the other half,
  since a schema field name collides freely with the parameter names of a callee
  the plate never defines (`date`, `title`, `caption`, `align`, `subject`). Only
  an occurrence that reads the binding anchors: an identifier spelling the alias
  as a named argument (`#text(size: 12pt)`), a dict key, another value's field
  (`#styles.subject`) or an imported item's path draws no ink off the field, and
  a window minted over one would carry a *wrong* address rather than a missing
  one.
  Laundering past that — a function parameter, a destructured binding, a
  per-card loop variable — is unchanged and still needs a `field-region` claim,
  now stated for plate authors under "Which Reads Get Regions" in the Typst
  backend guide. Content and date fields are unaffected: their ink is born in
  generated code.

- feat(wasm): `VARIANT_DISCRIMINANT_KEY` joins the runtime's static exports,
  beside `MAIN_CARD_ADDR`. v0.106.0 announced the constant as new API but shipped
  it to Rust only, leaving a JS consumer reading or writing a variant container
  to spell `"value"` itself — a hardcoded copy of the one value whose purpose is
  to not be hardcoded, at the seam where the two can drift unobserved, since the
  key crosses the boundary inside untyped container data. The `.d.ts` types it as
  the string *literal*, which `string` would stop narrowing an index into the
  container. `known_names_drift.rs` pins both spellings against the Rust
  constant, the guard the hand-spelled name tables beside it already carry.
  `VariantFields` stays Rust-only: it is a type alias the TypeScript surface
  already inlines as `QuillFieldSchema.variants`, naming no shape a consumer
  builds.
- docs: `0.105-to-0.106.md`, the migration guide v0.106.0 shipped without. It
  leads with the region-address shift rather than the three `!` entries: the
  changelog is organized by feature, and the address step is one clause inside a
  long entry while being the item most likely to break a working consumer, since
  `FieldRegion.field` is a bare `string` that no type checker reports a grammar
  change under. The guide states both gates on the step, and points a consumer at
  `doc.pathFor` for the prefix grammar it would otherwise match as a literal —
  with that helper's limit stated, a field name passing through it verbatim.
  `CONTRIBUTING.md` gains the two rules that would have caught the gap: `!` marks
  an observable-contract shift even where no type changes, and a release carrying
  one ships its guide.
- docs: `0.106-to-0.107.md`, this release's guide, under the rule the entry above
  adds. It leads with the region-address index step — `main.refs[0].org` where
  `main.refs` stood, a wrong address rather than a missing one — and then with
  the two schema literals that stop a quill loading, since those read as a build
  that broke rather than as a fix. `ERROR.md` names `display(..)` as the
  address-keyed template-author contract, `plaintext(..)` having been removed
  here.

## v0.106.0 - 2026-08-16

- feat(typst,pdfform): a schema address may step one property into a declared
  container, so `form-field(field: "classification.poc")` and
  `field-region("address.city")` name a cell rather than the container holding
  it. Two generated address tables gate the step the way `array_fields` gates
  the index step, and a typed dictionary and a variant container reach both
  alike: `classification.value` addresses the discriminant, `classification.poc`
  a variant cell in any world. The pdfform binder descends a variant container
  to match, so one address binds on either backend where `address.city` bound
  only on pdfform and asserted on Typst. **Region addresses shift** for a plate
  that reads a container property directly: `#data.classification.poc` regions
  as `classification.poc` where it regioned as `classification`, and `fieldAt`
  answers the same. A container read whole is unchanged, as is a read of a key
  the container does not declare.
- feat(core,wasm)!: an `enum` may declare `variants:`, a per-member field set
  that exists only in the world where the discriminant holds that member. This
  is the DSL's first cross-field shape, and it replaces the `cui_`-prefix
  convention with one the engine checks: `must_fill` inside a variant keeps its
  ordinary `default:`-presence derivation, so it reads *required in this world* —
  a `poc` obliged on a CUI memo and silent on every other one, the thing
  `must_fill` alone could not say. **Breaking**: declaring `variants:` changes
  the field's resting shape at every projection, from a bare string to a
  container, `{value: <member>, …that member's fields}`; the bare scalar
  (`classification: CUI`) is still accepted as the spelling of a world carrying
  no answers, and coercion normalizes both. The wire carries exactly the live
  world, so a plate reads a variant field inside the `values ∪ blank` branch it
  already owes the enum, and inside that branch every declared field is present
  and needs no guard. A value stranded by a discriminant flip is kept and warned
  (`validation::out_of_variant`), never dropped at coercion or gated at render.
  The ceiling is enforced at load, not discovered at render: a variant carries
  plain data only, sits at card level only, and cannot declare `value`. The
  transform schema projects the container with every world's fields flattened
  under `properties`, since a binding built once against a schema must address a
  field today's document has not selected; member scoping stays on the
  declaration view, where `schema()` emits `variants:` keyed by member.
  `FieldSchema` gains `variants` and `variant_field` (the cell a name declares
  under any world); `VariantFields` and `VARIANT_DISCRIMINANT_KEY` are new.
- feat(fixtures)!: `usaf_memo`'s four `cui_*` fields move under
  `classification`'s `CUI` variant as `controlled_by`, `poc`, `category`, and
  `limited_dissemination`. `controlled_by` and `poc` drop their `default: ""`
  and are therefore obliged — on a CUI memo only, which is what DoDM 5200.48
  actually requires and what the flat spelling could state only in
  `description:` prose. A document writes `classification: {value: CUI, …}` and
  a plate reads `data.classification.value`.
- feat(typst): `field-region(field, body)` claims the ink `body` draws for a
  schema field, so a plate can tie content it *composes* — a banner keyed on a
  field, a package-built block, a computed table — to `session.regions()` and
  `session.fieldAt(..)`. Layout-neutral: `body` is returned untouched between two
  invisible `metadata` markers. It is a **fallback** claim, never an override:
  ink already tracked to a field keeps that field, so wrapping is purely
  additive and cannot retarget. Each *call* claims independently, so a wrapper
  invoked once per card yields one region per card — the way a card's scalar
  fields get regions at all, reading as they do from a loop variable that carries
  no per-instance identity. The marker stack persists across pages so a claim can
  span a page break, which leaves a claim whose closing marker never reaches a
  frame bounded by nothing: it would take every unattributed piece of ink to the
  end of the document. Those are found before the scan and suppressed in both the
  region and point queries — an unbounded claim yields nothing rather than
  everything — and reported as a `typst::unclosed_field_region` warning naming
  the field, since only the plate author can act on it. Typst does not separate
  the two markers on its own — they are siblings in content flow — but a plate
  emitting the call's return value in parts can.
- feat(typst,pdf): `form-field` takes `font`, `size`, and `align`, so an
  injected widget's value can be set to match the type around it. A widget was
  fixed at Helvetica, auto-size, left: auto-size makes the rendered size a
  function of both box height and how much the user has typed, and left
  justification cannot be overcome by geometry, because a fillable box is sized
  for the longest plausible value rather than the value in it. A right-aligned
  fill-in — a USAF memo's date, say — was unreachable. `font` is one of
  `"helvetica"`/`"times"`/`"courier"`, a widget being unable to carry a font
  program; `size` is an absolute length or `auto` for the old behavior; `align`
  is `"left"`/`"center"`/`"right"` and lands in `/Q`. All three are rejected on
  `"checkbox"` and `"signature"`, which carry no variable text. `FieldSpec`
  gains `font`, `font_size`, and `align` (`FormFont` and `TextAlign` are new).
  A field that sets none of them stamps byte-identically to before, and
  `pdfform` is untouched: `form.json` still carries no styling, so the flatten
  path and canvas preview are unchanged.
- fix(fixtures): the `usaf_memo` indorsement date widget is set in the memo's
  own 12pt Times and ends on the right margin, where the date it stands in for
  would have ended. It was auto-sized Helvetica starting at the fill-in rule's
  left end. Sizing it exposed that the rule-width box clips a real date — "28
  September 2026" sets 93pt at 12pt Times against a 72pt box, and a fixed size
  clips where auto-size had silently shrunk — so the widget is now 10em wide
  and hangs off the rule's right edge, overrunning leftwards into the
  whitespace a printed date grows into. Sized in ems of its own face rather
  than inches because `font_size` is a document field with no ceiling: an inch
  width would stay put while the text inside it grew. 10em clears both
  orderings at any body size (DAF's "September 28, 2026" is the widest at
  8.03em; USAF's "28 September 2026" is 7.78em). `date-placeholder-line` seats
  it with a measured `dx` rather than `place(bottom + right)`, Typst clamping
  an overflowing alignment back to zero, which leaves `right` indistinguishable
  from `left`. That helper draws no rule now and is named `date-placeholder`
  rather than `date-placeholder-line`: the widget carries the date, and a rule
  under a widget wider than it underlines only the fraction of the value narrow
  enough to sit over it. It is package-internal, not exported from `lib.typ`.
- fix(core): a name two variants declare *differently* is a load error,
  `quill::variant_field_collision`. The name is one cell of the container
  whichever world brings it into play — neither the coercion lookup nor the
  transform schema consults the discriminant to fill it — so two readings of it
  coerced a live value under the other world's type: a document selecting a
  world whose `note` is `integer` had its `42` coerced to `"42"` by a sibling
  world's `string` and then failed `validation::type_mismatch`, undraftable and
  blamed for a string it never wrote. Identical declarations, which is what
  repeating a shared field set or sharing a YAML anchor produces, collapse to
  that one cell without loss and stay legal.
- fix(core,wasm,python)!: `EditError::UnknownField` carries the in-field path
  `FieldDecode` and `FieldNotContent` carry. A property an `object` field does
  not declare — `get_content_at("address", [Key("zip")])` against an `address`
  with no `zip` — reported `field 'zip' is not declared in the schema`, which
  reads as a claim about a top-level field and collides outright when a real
  top-level field shares the name. It now reports
  `field 'address.zip' is not declared in the schema` and anchors the
  diagnostic at `main.address.zip`, so the caller can tell an undeclared
  property from an undeclared field. **Breaking**: the variant is a struct
  variant, `UnknownField { field, at }`, matching the two siblings; `field`
  stays a bare field name, and the `edit::unknown_field` code, its `field` arg
  and the whole-field message are unchanged.
- fix(python): declaring `license-files` ships the `LICENSE` the sdist metadata
  names, which PyPI rejected the sdist for lacking. v0.104.0 and v0.105.0 are
  wheels only.

## v0.105.0 - 2026-08-14

- feat(core,wasm,python)!: a `Content` nested inside a composite field is
  readable at its own codec. `TypedReader::get_content_at(name, path)` (and the
  `CardReader` twin, `reader.getContentAt(addr, path)` in JS,
  `reader.get_content_at(name, path)` in Python) walks a `PathSegment` path
  through the field schema — `items` for an index, `properties` for a key — to
  the leaf whose declared type names the codec, then decodes through the same
  dispatch the whole-field read uses. So an `array<richtext>` element, an
  `object`'s content property and a leaf under both each read back the same
  `Content` whatever their resting form, where before every one of them
  answered `FieldNotContent` and the consumer had to decide for itself what the
  stored bytes meant (#1243). The empty path *is* `get_content`. A path naming
  nothing in the stored value reads absent rather than throwing: an editor's
  row index goes stale between derive and read, and that is the axis a repeater
  mutates. `Addr` deliberately gains no element axis — the path is the read's
  own argument, since `storeField` / `isFill` / `applyChange` could not answer
  one. **Breaking**: `EditError::FieldDecode` and `EditError::FieldNotContent`
  each gain an `at: Vec<PathSegment>` field carrying the in-field path, so the
  diagnostic anchors at `main.paragraphs[1]` and parses back to those segments;
  `field` stays a bare field name and `args` is unchanged. `FieldNotContent`
  now names the type *reached*, so a `string[]` element reports `string` rather
  than the field's `array`.
- feat(core,wasm)!: `must_fill:` on a field declares the **obligation** axis.
  `default:` carried the fill value and the obligation signal on one bit, so
  only that 2x2's diagonal was reachable; a safe value that still wants a
  human's confirmation (`default: UNCLASSIFIED` with `must_fill: true`) and a
  genuinely optional field with nothing to suggest (`must_fill: false`) now
  each have a spelling. Left unset it derives `default.is_none()`, so no
  existing quill's blueprint marker set changes. `Quill::validate` gains a
  second trigger under the one `validation::must_fill` code, named by a
  `trigger` arg: `marker` for a `!must_fill` tag the document carries, and
  `unauthored` where the schema obliges a cell the document leaves absent or
  present-null. The second closes a hole — `validate_fills` walked only the
  payload, so a hand-written or programmatically built document drew no
  completeness signal whatever `Quill.yaml` declared. **Breaking**: a merely
  incomplete document no longer validates clean. Absence is still never
  *malformed* and still never gates render, but a consumer reading "any
  diagnostic ⇒ not done" now sees a warning per unauthored obliged cell on
  documents that were silent. The obligation keys on cell presence rather than
  the resolved source rung, so a must-fill leaf inside a container someone
  touched still warns; a typed dict is never itself a cell and recurses to its
  leaves, while an array is one cell, `[]` being a real answer. Authoring the
  field's blank discharges it and `field: null` does not: null ≡ absent stays
  unqualified on the value ladder, but obligation asks whether a human made a
  call. Seeding stamps the marker on example-seeded obliged cells, so a fresh
  seed and an empty document report the same cells, and the transform schema
  carries `quillmark:must_fill` (#1255).
- fix(core,wasm,pdfform)!: a field's **blank** — its spelling of "explicitly
  nothing" — is a property of the field rather than a member of its type's
  domain, and an `enum`'s is `""`. The render floor for a defaultless enum
  returned `values.first()`: a choice nobody made, indistinguishable at the
  plate from a deliberate one and reachable from a cosmetic `values:` reorder.
  An unanswered enum now renders `""`, so a reorder is render-safe for every
  document and only removing or renaming a member breaks
  ([VERSIONING.md](prose/canon/VERSIONING.md)). **The accepted domain widens to
  `values ∪ blank` for *every* enum**, defaulted ones included: `format: ""`
  was a fatal `EnumViolation` and now coerces, validates and reaches the plate.
  **A plate must therefore branch exhaustively over `values ∪ blank`** — an
  `else` fallback re-opens exactly the fabrication the blank closes, and a
  downstream package that asserts membership fails the compile outright. Note
  `data.at(key, default: X)` is not a guard here: blank-filled render makes
  every declared key present, so its `default:` is dead code and the blank
  flows through. **Breaking**: `zero_value` → `blank`, `FieldSource::Zero` →
  `Blank` and its wire token `"zero"` → `"blank"`; `""` declared in `values:`
  is a load error (`quill::enum_blank_member`), the engine supplying the blank
  instead; and `date: ""` renders blank rather than falling back to a
  `default:`, settling a three-way disagreement between coercion, validation
  and the floor. `default: ""` stays valid and keeps its meaning — `values:`
  enumerates choices, `default:` is a value, and the blank is a legal value
  that is never a choice. Additive: `ui.blank_title` labels an enum's blank and
  rides the transform schema as `quillmark:blank_title`; that schema's `enum:`
  leads with the blank, so a standard JSON-Schema validator accepts what the
  engine accepts, and pdfform Choice widgets lead their options with it too. A
  consumer's picker must keep the blank selectable and re-selectable — returning
  to it is how an author clears a cell back to unset.
  `integer`, `number` and `boolean` keep `0` / `false` as their blank,
  indistinguishable from an authored zero — a permanent seam, since a wire
  `none` would cost the totality the floor exists to buy. Full guide:
  [0.104 → 0.105](docs/migrations/0.104-to-0.105.md) (#1254).
- fix(fixtures,docs): the three fixture plates that dispatch on `$kind` read it
  with a bare `card.at("$kind")`, which panics on a kindless card, and guarded
  declared fields against an absence blank-filled render makes impossible.
  Plate authors copy the fixtures rather than `PLATE_DATA.md`, so the fixtures
  were teaching both the unsafe metadata read and a dead presence check.
  `classic_resume` carried the live consequence: `url` is declared with no
  `default:`, so the floor delivers `""`, `default: none` never fires, and the
  package's `url != none` test always passes — an empty Courier element where
  the block should have been skipped. Its `subheading-*` guard cost an empty
  grid row the same way. Declared fields now guard their *value*, and
  `docs/quills/typst-backend.md` states the rule as a table over the three key
  kinds. `fixture_quills_render_test` renders every fixture quill's seed
  document — the net that was missing, since `classic_resume`'s plate had no
  test reaching it (#1256, #1257).
- test(typst): `plaintext` reaches regions and navigation by inheritance — the
  render floor coerces its resting literal to a content object, the backend
  classifies that object by `contentMediaType` alone, and the shared lowering
  emits it with a segment map — and every step was load-bearing and untested,
  with the classification predicates named for richtext so the sharing read as
  a coincidence. Pinned at engine altitude, because a test driving the backend
  directly hand-builds the content object, bypassing the floor's coercion, and
  would stay green through a regression that silently empties `regions()`. The
  predicates are `is_content_field` / `is_content_array_field` /
  `is_inline_content_field`, and `PREVIEW.md` names plaintext beside richtext
  in its producer list (#1247, #1250).

## v0.104.0 - 2026-08-13

- feat(core,wasm): a quill declares, per body, the block constructs its plate
  does not typeset (`main.body.unsupported`, `card_kinds.<k>.body.unsupported`;
  names from `heading`, `rule`, `code`, `list`, `quote`, `table`, `image`).
  A body holding one anyway draws the non-fatal `plate::unsupported_construct`,
  a fifth warning family, on the pre-render walk `Quill::parse` runs beside
  `conform`: one diagnostic per (body, construct) carrying the count in `args`
  and the body's path, so occurrences collapse rather than scatter. The
  declaration also rides `QuillConfig::schema()` to the editor, which is the
  half a render-time warning could not serve: it answers before the gesture.
  Nothing verifies a declaration — a plate that drops an undeclared construct
  stays as silent as before. `usaf_memo` declares `rule`; empty everywhere
  else, so no existing quill's schema or warnings change.
- fix(fixtures): `usaf_memo`'s `render-body` drained its heading buffer in the
  three shapes that used to discard it. A heading with nothing after it (the
  buffer died with the loop, taking a list item's bullet with it), a heading
  whose next element opened a *different* list item (its text was delivered
  into that item), and a heading following a heading (the assignment overwrote
  the earlier one) each lost their text with nothing in the render to say so.
  The run-in style is unchanged where it was right: a heading joins the next
  block of its own item, or the next paragraph at top level.
- fix(content)!: `to_markdown` writes `***` for a thematic break, not `---`.
  `- ` + `---` is four dashes separated by spaces, which re-imports as a
  top-level break, so a rule as a bullet item's first block lost its item on
  every markdown round-trip. The canonical spelling is now the one with the
  fewest other readings (`---` is also a setext underline and the root-block
  front-matter opener). Exported markdown changes for documents holding a
  rule; the content model, wire data and rendered output do not.
- refactor(core,pdfform,cli,wasm)!: the `enum:` modifier on `type: string`
  retires. `type: enum` with a `values:` list is the one spelling of a finite
  string domain; `enum:` on any type is now `quill::field_parse_error`, whose
  message names the replacement — it is the only diagnostic a quill written
  against the modifier ever received, since the deprecation shipped in 0.94
  with no warning code behind it. `QuillConfig::schema()` re-emits every
  domain as `values:`, so a consumer reading `enum:` off the schema echo (the
  wasm `QuillFieldSchema.enum`, dropped here) reads `values:` instead. The
  `usaf_memo` and `sample_form` fixtures migrate; wire data and rendered
  output are unchanged, the projections being domain-keyed already.
- fix(core): `build_transform_schema` keys a field's finite domain on the
  domain itself rather than the `Enum` token, joining the render floor, the
  pdfform widget kind and the blueprint annotation. Under the retired
  spelling every `usaf_memo` enum — `classification`, `format`, `action` —
  projected as a bare `{"type":"string"}`, so a consumer building a
  JSON-Schema validator from the transform schema accepted
  `classification: "banana"` while pdfform drew the six-option dropdown for
  the same field and `QuillConfig` rejected the value at coercion (#1237)
- fix(core)!: geometry addresses parse segment-wise, so `locate` and
  `fieldBoxes` answer for an address deeper than one segment. The translation
  boundary folded a plate address's whole tail into one `Field`, so
  `references.0` minted `main.references.0` — a string that reparses as a field
  literally named `0`, and that the reverse direction refused outright. Both
  spellings returned `None`, leaving caret placement and whole-field highlight
  dead for **every** `array<richtext>` element (the flagship memo's
  `references` among them) and for every nested key a pdfform widget binds
  (`address.city`). `region.rs` now reads and renders a plate tail one segment
  at a time: an all-digit segment is an array index, `$body` the body terminal,
  anything else a field or map key.
- change(wasm, python)!: `RenderedRegion.field`, `FieldRegion.field` and
  `ContentHit.field` spell an array element bracketed — `main.references.0`
  becomes `main.references[0]` — on `regions()`, `fieldAt`, `positionAt` and
  `RenderResult.regions`. This is the spelling schema validation already emits,
  so a `Diagnostic.path` and the geometry address for one place are now the same
  string. A consumer finding an address's children by prefix (`startsWith(`${field}.`)`)
  needs the `[` opener too, and any heuristic reading a trailing all-digit field
  name as a lost index is dead.
- feat(wasm): `doc.pathFor(addr)` mints an `Addr` as the canonical `DocPath`
  string `Diagnostic.path` carries and `session.locate` / `session.fieldBoxes`
  take; `doc.cardPath(i)` is the card's own root. `Document` computed the
  kind-qualified root for every addressed write and did not hand it out, so a
  consumer building a path restated the kind lookup, the `Addr` defaults and the
  range guard — and a wrong-kind path is compared as a string, matching nothing
  and drawing no highlight without throwing. Both are quill-free (the stored
  `$kind` verbatim) and total on the index axis: a path is an anchor, not a
  read, so a per-keystroke call needs no `try` (#1225)
- change(wasm)!: `@quillmark/wasm` declares `engines: { node: ">=24" }`, the
  tier CI builds and tests the bindings on and the one both devcontainers hand a
  contributor. Nothing in the package requires it at runtime, so a Node 22
  install fails `engines` checking without failing at import.
- docs: `docs/migrations/0.103-to-0.104.md` carries the four breaks — the
  retired `enum:` modifier, the bracketed index spelling, the `***` thematic
  break and the Node floor — with the prefix-match, trailing-digit and stored
  -markdown shapes a consumer has to fix, and the two additive surfaces
  (`pathFor` / `cardPath`, and the `plate::unsupported_construct` family a
  code-routing consumer gains an arm for).
- test(core): three characterization tests pin the render floor's two
  type-domain edges (a defaultless enum, top-level and nested in a typed
  dictionary) and an authored empty `date` beside an empty `string`, so the
  coercion difference between the two is one test's diff. Every shipped quill
  declares a `default:` on every enum and none authors an empty `date`, so the
  fixture suite reached neither path. A fourth carries a `!must_fill` tag on two
  example-seeded cells through seed → store → load → conform. Refs #1234

## v0.103.0 - 2026-08-09

- docs: `docs/integration/operations.md`, carrying what the other integration
  pages leave unsaid: that **render is not bounded** — no deadline, no
  cancellation, and the parse limits do not carry through — with the
  worker-termination recipe that is the only abort a browser has; that
  `Quillmark`, `Quill` and `Document` are `Send + Sync`, pinned by a test rather
  than asserted; `comemo` eviction as what a long-lived process's memory tracks;
  the no-network, no-ambient-filesystem isolation properties; and that a panic
  is terminal on every surface.
- docs: `parse::input_too_large` carries four of the five §8 caps, separable
  only by its `max` arg, which `error-handling.md` now says where it names the
  code.
- test(cli): `quillmark-cli` gets its first tests. The bin carries
  `test = false`, so twelve cases drive the built executable instead — every
  subcommand, `-o` and `--stdout`, PDF and SVG output, and the error paths,
  which must exit 1 rather than panic. The crate had no `[dev-dependencies]`
  and no workflow invoked it (#1068).
- fix(cli)!: `render --verbose` writes its progress lines to stderr, as the
  warning printer already did. Under `--stdout` they went to stdout ahead of and
  after the artifact, so `quillmark render q --stdout --verbose > out.pdf`
  produced a PDF with `Loading quill from: …` before its header and
  `Rendering completed successfully` past its trailer. A script that parses
  `--verbose` output from stdout reads it from stderr now.
- test(fuzz): `pdf_fuzz` covers the AcroForm stamp spine's byte-level reads,
  the one hand-rolled parser with no fuzz target. Arbitrary bytes, and a real
  form truncated, single-byte-corrupted, or spliced, all through
  `page_media_boxes` / `PdfUpdate::begin` / `stamp`. The oracle is no panic:
  nothing in the workspace catches unwind, so a panic there kills the CLI and
  the Python extension and poisons the WASM module. No failures found.
- fix(core): `MAX_FIELD_COUNT`'s rustdoc said "per document"; the check is per
  card-yaml block, counted after `$`-key extraction.
- refactor(wasm)!: `init()` resolves to the core surface, and it is the only way
  to reach one. `Quill`, `Document`, `importMarkdown`, `exportMarkdown`,
  `rebase`, `mapPos`, `parseDocPath` and `formatDocPath` leave the static
  exports of `@quillmark/wasm`: `const { Quill, Document } = await init()`
  replaces the value import. The precondition was carried entirely by
  `init`'s signature, and a floating promise is an ESLint rule rather than a
  `tsc` diagnostic, so a call site that skipped the await type-checked and then
  passed or failed by load order. It now has no name to call. `Engine`,
  `MAIN_CARD_ADDR`, `isQuillmarkError`, the open-set guards and the
  writer/reader classes are unchanged, needing no instance or gated by their
  arguments; the `Quill` / `Document` **type** exports are unchanged, so
  annotations and `import type` compile as before. Class identity is untouched:
  the gate hands out the core build's classes verbatim, and `instanceof` stays
  the whole membership test. `runtime::not_initialized` and the build-time
  sentinel that raised it retire with the door they guarded. Rust, Python,
  documents and stored blobs are unaffected. See
  `docs/migrations/0.102-to-0.103.md`

## v0.102.0 - 2026-08-04

The pre-1.0 vocabulary reset. Verbs, diagnostic codes, and two words that meant
different things at different altitudes. Documents and stored blobs are
untouched: no document reparses, no blob remigrates, and the storage wire format
is byte-identical, but nearly every consumer touches at least one renamed verb.
The diagnostic codes are the part that could not wait, since consumers route on
them and 1.0 freezes them. All breaking changes are covered by
`docs/migrations/0.101-to-0.102.md`. Only two behaviors change, both called out
below. Separately, `@quillmark/wasm` ships `--target web`, which makes
`await init()` mandatory and the bundler plugins the package used to demand
unnecessary.

- refactor(core,wasm,python)!: the verb vocabulary is reset to say what the code
  does. `install_body` / `install_field` / `doc.install` become `overwrite_*` /
  `doc.overwrite`: the content lane is a ladder sorted by the fate of the
  identity anchors already on the value (overwrite destroys, revise rebases,
  apply preserves), and only the first keeps nothing, which grouping the three as
  "identity-aware" hid. `LiveSession::apply` becomes `update`, taking the trait
  seam, the feature-gated raw-plate seam, and `backend::apply_unsupported` →
  `backend::update_unsupported` with it, so the content lane's `applyChange`
  splice is no longer a homograph of a whole-document recompile.
  `apply_field_richtext_change` becomes `apply_field_change`, matching its
  schema-blind neighbour `revise_field`, which never carried the codec in its
  name. `store_seed_namespace` / `remove_seed_namespace` become
  `store_seed_overlay` / `remove_seed_overlay`, closing the asymmetry with the
  `seedOverlay` read and the `SeedOverlay` type that already shipped: `$seed` is
  keyed by a validated card-kind, `$ext` by a free-form consumer namespace.
  `getMarkdown` and reader `get_body` become `bodyMarkdown` / `body_markdown`,
  the name core already used, so one projection has one name on every surface
  (#1186). See `docs/migrations/0.101-to-0.102.md`
- refactor(core,wasm,python)!: the mutator and validation diagnostic codes are
  corrected, and the content-field failures carry the codec that ran.
  `edit::field_richtext_decode` becomes `edit::field_decode` and
  `edit::field_richtext_not_inline` becomes `edit::field_not_inline`, each
  gaining a `codec` arg (`"richtext"` or `"plaintext"`): both were raised for
  plaintext failures under a richtext name, and `plaintext(inline)` fell through
  to the generic code. `edit::field_conform` becomes
  `edit::field_coercion_failed`, killing the `conform::field_conform` stutter and
  landing the variant beside its real twin `validation::coercion_failed`, minted
  from the same `CoercionError`. `richtext::not_inline` and
  `plaintext::not_plain` become `validation::not_inline` / `validation::not_plain`,
  stage-namespaced like every other code. The `conform::*` twins rename in
  lockstep. Route on the code and read `codec` for the lane (#1186). See
  `docs/migrations/0.101-to-0.102.md`
- fix(core)!: a splice against an absent field is no longer a decode error. The
  writer `set_body` / `setBody` becomes `revise_body` / `reviseBody` and returns
  the text `Delta` it was already computing (a body carries no field schema, so a
  typed-lane verb had nothing to type); Python discards the receipt, as on
  `revise_field`. `apply_field_change` on an absent field splices against the
  empty content, as `revise_field` diffs against it, instead of reporting
  `FieldRichtextDecode { message: "field is absent" }`: the one place in the API
  where a missing field was not simply `None`. No error is lost. A bundle that
  expected content still fails, and now reports the condition it hit, since the
  text delta declares the base length it was computed against, so a stale splice
  lands as `edit::content_apply`. Route a vanished-field check on that code, or
  check presence before splicing. These are the release's only behavior changes
  (#1186). See `docs/migrations/0.101-to-0.102.md`
- refactor(core)!: `EditError::variant_name` is removed and the typed primitives
  leave the documented surface. The bare variant name was a second discriminator
  kept in lockstep with `code()` that nothing routed on: both binding error
  mappers stamp `code()` onto the `Diagnostic` they raise, and `ERROR.md` has
  said identity is the code since the `edit::*` family landed. Assert on `code()`
  or match the enum, which is `#[non_exhaustive]` either way. `Card::commit_field`
  and `Card::revise_field_checked` become `#[doc(hidden)]`: a resolved
  `FieldSchema` argument was the only thing telling them from their opaque and
  schema-blind neighbours, and disambiguating by argument is a third mechanism
  beside the receiver and the verb. `quill.writer(&mut doc)` is the typed door on
  every surface now, core included; the primitives stay callable on the same
  terms as the other hidden items (#1186). See `docs/migrations/0.101-to-0.102.md`
- docs(content,core)!: restoring a deleted island re-lands its original id, and
  the island channel states two contracts it had left to the implementation. The
  never-ambient minting rule (continue the field's positional `isl-{n}` sequence,
  never a UUID or a clock reading) covers a *new* island; a delete frees its id
  and the id travels back with the island, so an editor that mints fresh on undo
  renames it, moving the content hash of a document the user believes they
  restored. A pasted copy of a live island is new and mints fresh. Alongside it:
  an island op's `at` is *sequenced*, counting the text the delta and this
  bundle's earlier island ops left rather than the shared post-delta frame; and a
  producer's whole-field diff that carries an island slot must split into the
  slot-free `delta` plus one `Insert` per slot, since a slot in an insert string
  orphans. Neither is a behavior change, and a producer that guessed wrong got a
  wrong document rather than an error (#1185)
- feat(wasm)!: the package ships `--target web` and the runtime owns
  instantiation, so `await init()` once at startup is required before any export
  is used. `--target bundler` emitted `import * as wasm from "./wasm_bg.wasm"`,
  which no browser and no bundler resolves natively; the plugin that fixed it
  rewrote the import into a top-level await, and because the runtime statically
  re-exports core, that await landed on the static module graph of everything
  importing `@quillmark/wasm`: a permanent constraint on consumer architecture
  and a blank SvelteKit route in Safari's dev server that neither Chrome nor
  `vite build` showed. In exchange, `vite-plugin-wasm` and
  `vite-plugin-top-level-await` are no longer needed, a static import is safe
  anywhere including SSR, and plain Node can import the package at all (the ESM
  `.wasm` import needed `--experimental-wasm-modules`). Only
  `optimizeDeps: { exclude: ['@quillmark/wasm'] }` stays, for Vite's dev server.
  `init` memoizes its promise, so several entry points share one instantiation
  and a failed attempt clears the memo for a retry; backends are not the
  consumer's to initialize, since `Engine` instantiates one inside its lazy load.
  Reaching the surface early throws `runtime::not_initialized` naming the fix,
  and `build-wasm.sh` asserts both the guard's anchors and that no artifact
  carries a `.wasm` import or a top-level await. `initSync` is not exported: the
  capability remains through `init(source)`, which takes bytes, a `Response`, a
  `WebAssembly.Module`, or a URL (#1189). See `docs/migrations/0.101-to-0.102.md`

## v0.101.0 - 2026-08-03

- refactor(core): gate the raw-plate seam behind a feature, and fixes from review
- docs(core): state the conform gate's codes instead of linking a private item
- refactor(core): bind a live session to its quill so apply takes a Document
- feat(quillmark): the facade names what the read and preview flows return
- docs(content,wasm): density pass over the island channel's prose
- feat(content,core,wasm)!: reach islands through the op vocabulary
- refactor(core)!: `Payload` becomes a read view
- refactor(core)!: collapse the schema-free field projection
- refactor(core)!: fold `Quill`'s file queries into `FileTreeNode`
- fix(core): repoint the doc references the `from_yaml` removal orphaned
- refactor(quillmark): move the facade gate off the front page, and stop tests reaching past it
- docs: migration guide for the 0.101 surface removals
- refactor(core)!: drop the lossy `QuillConfig::from_yaml`
- refactor(core)!: `Document::from_main_and_cards` becomes crate-internal
- feat(quillmark): the facade covers authoring, and the examples enter through the bound door


## v0.100.0 - 2026-08-03

A content field gets one resting form, and the last reserved `$` key with no
reader is removed. All breaking changes are covered by
`docs/migrations/0.99-to-0.100.md`. Stored documents load unchanged, but a row
read through the bound door converges once: read-repair, not a schema-version
event. One ordering matters, and the guide's "Legacy data" section states it —
conform a stored population before exporting markdown from it.

- refactor(core,wasm,python)!: content fields have one resting form, enforced at
  load. `Quill::conform(&mut doc)` is the primitive and `Quill::parse(md)`
  (parse, then conform) the convenience — the documented primary ingestion path,
  `quill.parse` / `quill.conform` on both bindings. A `richtext` field rests as
  the canonical content object, a `plaintext` field as its **literal string**, so
  the stored shape is a property of the codec instead of the construction lane:
  `equals` and content hashes stop separating semantically identical documents.
  The typed writer commits `plaintext` as a string, and `revise_field` diffs it
  through the literal codec (a byte-identical revise of `a \*b\*` used to commit
  `a *b*`). `Document::parse` / `Document.fromMarkdown` stay exactly as they
  were, demoted to the transport/repair door. Conform is idempotent, a byte no-op
  on an already-canonical document, and reports a `conform::*` warning where the
  strict write refuses rather than retyping or rejecting; a `$quill` naming
  another quill errors before any mutation (#1160, #1162). See
  `docs/migrations/0.99-to-0.100.md`
- fix(core)!: markdown exported from a `plaintext` field resting as a content
  object is markdown-escaped. Emit is schema-free and cannot tell a `plaintext`
  content from a `richtext` one, so `a *literal* line` leaves as
  `a \*literal\* line` and re-parses with the backslashes as characters — one
  more layer per save cycle. Only the typed writer produced that rest, and the
  string rest above deletes it rather than managing it: load, conform, and
  re-store a population before exporting markdown from it. Markdown already
  exported under ≤0.99 is corrupt at rest, its escapes indistinguishable from
  authored ones, so re-export it from the conformed rows (#1159). See
  `docs/migrations/0.99-to-0.100.md`
- fix(core,wasm,python)!: a `plaintext` field resting as a string reads through
  the **literal** codec, not markdown — `note: 'a *literal* line'` read back as
  `a literal line` while render and validation kept the asterisks. Only the
  string lane was wrong; the committed-object lane always decoded correctly, so
  a consumer that pre-escaped a `plaintext` field to survive the read drops the
  escaping. Alongside it, `reader.get_content` / `reader.getContent` returns a
  content field's `Content` corpus whichever lane stored it, so a consumer
  holding a corpus editor stops branching on the wire shape. `EditError` gains
  `FieldNotContent` (`edit::field_not_content`) for a declared type that is not a
  content leaf; core adds `Card::field_plaintext_content` (#1154). See
  `docs/migrations/0.99-to-0.100.md`
- refactor(core,wasm,python)!: card `$id` is removed — the reserved key, its
  resolver (`Document::find_card` / `doc.cardIndexById` / `doc.card_index_by_id`),
  the uniqueness contract (`EditError::CardIdCollision` / `EmptyCardId`, the
  `parse::card_id_*` warnings, the storage rejection), `Card::id` /
  `Payload::{id, set_id, take_id}` / `Document::{set_card_id, remove_card_id}`,
  the `PayloadItem::Id` and `CardWire.id` wire members, and the projected `id` on
  both bindings' card shape. Nothing in the engine read it and it never reached a
  backend, so what is left after removing the machinery that served the resolver
  is `$ext` with a reserved name. A block declaring `$id` no longer parses and a
  blob carrying an `id` payload item no longer loads: a hard cutover, no
  tolerate-and-ignore window. Per-card consumer keys move to `$ext` under a
  namespace you own, with no uniqueness, no collision check, and no repair
  (#1151). See `docs/migrations/0.99-to-0.100.md`
- refactor(content)!: `Content`, `Line`, `Mark`, and `Island` take
  `#[non_exhaustive]` — the four public structs the 0.99 sweep missed, that pass
  having run as two issues split by crate. Their literals give way to `new` plus
  the `with_*` setters on the same terms as the rest of the API; every field stays
  `pub`, so reading and assigning are unchanged. `Delta`, `Segment`, and
  `BaseLengthMismatch` stay open deliberately and now say so in their rustdoc.
  A Rust source break only: nothing about the wire, the canonical bytes, or the
  bindings moves (#1146). See `docs/migrations/0.99-to-0.100.md`
- feat(core,wasm,python): `Diagnostic.args` — the facts `message` interpolates,
  keyed by name, so a consumer with its own string table selects a sentence by
  `code` and fills it itself. Values keep their JSON shape (a list arrives as a
  list, a count as a number), engine prose never rides under a key, and a
  formatter missing a key falls back to `message` wholesale. `prose/canon/ERROR.md`
  § "Diagnostic args" tabulates the keys per code and a test fails when code and
  canon disagree (#1130)
- fix(core): the `$quill` mismatch message and hint name the pairing rather than
  the verb. `check_quill_reference` gates every schema-bound door now, not the
  render path alone, so a `quill.parse` failure no longer reads "was rendered
  with". The codes (`quill::name_mismatch` / `quill::version_mismatch`) are
  unchanged
- test(fuzz): the resting-form invariant gains a target, stated as three
  properties — conform is a fixed point, parse-then-conform equals typed-write
  per content field, and a document through the markdown surface and back settles
  after one pass (exactly, for `plaintext`, whose codec is lossless both ways)
- docs: the cycle's stale pages are repaired. Both binding READMEs gain the bound
  door and the corpus read, `revise_field` is documented per declared type on all
  four surfaces instead of as a markdown-only richtext verb, and four canon claims
  that outran the tree are corrected


## v0.99.0 - 2026-08-01

The 1.0.0 API freeze lands ahead of the tag, and the content codec closes its
last open gaps. All breaking changes are covered by
`docs/migrations/0.98-to-0.99.md`. Stored documents are unaffected: a `0.98` blob
loads byte-identically and `0.99` writes the same bytes for the same content.

- refactor(core,content,pdf,pdfform)!: the public API opens. 75 public types take
  `#[non_exhaustive]` — nothing in the workspace carried it before — so an
  exhaustive `match` needs a `_` arm and a struct literal gives way to `new` plus
  `with_*` setters. `Backend` is sealed, `OutputFormat::ALL` and
  `Content::RESERVED_{MARK_TYPES,LINE_KINDS,CONTAINERS}` become slices, and
  `RenderOptions { .., ..Default::default() }` becomes
  `RenderOptions::default().with_output_format(fmt)`. Four stay exhaustive and
  say so: the storage DTOs, frozen per schema version, plus
  `quillmark_pdf::FieldType`, `KnownIslandType`, and `Fidelity`, where an
  out-of-crate `_` arm is silent (a field that draws nothing, an island dropped
  from the projection, a fidelity rung nothing warns about). The rules are
  canonized in `prose/canon/COMPATIBILITY.md` (#1090, #1103). See
  `docs/migrations/0.98-to-0.99.md`
- refactor(core)!: the YAML engine leaves the public API. `QuillValue::from_yaml_str`
  and `QuillConfig::schema_yaml` return `quillmark_core::YamlError` instead of
  `serde_saphyr` types, so a `0.0.x` dependency release is no longer a break to
  `quillmark-core`. The message is sanitized (the engine's own Rust API names are
  stripped), and `from_yaml_str` gains the `MAX_YAML_DEPTH` budget its siblings
  already carried (#1099, #1101). See `docs/migrations/0.98-to-0.99.md`
- fix(content)!: the reserved-name rule reaches the wire. `attrs` beside a
  built-in discriminator resolved to the built-in and dropped the payload in
  silence; the authored lane now refuses it on all four axes, where a host writes
  it. Reading never got stricter — a blob from before a promotion still opens.
  A table cell keeps its own unknown keys too, canonicalization now rewriting it
  in place rather than minting a fresh `{text, marks}` (#1084, #1085, #1086,
  #1092). See `docs/migrations/0.98-to-0.99.md`
- fix(content)!: opaque payload depth is bounded at `MAX_JSON_DEPTH` (128) on the
  `Value` lane, where an unbounded one took the WASM module down with a
  stack-overflow trap rather than a catchable error. The WASM guard sits on the
  JS side of the boundary, since `serde_wasm_bindgen` recurses while building the
  value, and covers every door that takes opaque host JSON: `install` and
  `applyChange`, plus `makeCard`'s field values and `insertCard`'s payload items
  (#1093). See `docs/migrations/0.98-to-0.99.md`
- fix(content)!: island `loss` becomes the fifth open set. An unrecognized class
  round-trips verbatim instead of being rewritten to `unrepresentable`, so merely
  opening a document no longer moves its content hash. `Loss` opens on the island
  `type` axis' terms rather than the block axes': it becomes an opaque string
  wrapper with `LOSSLESS` / `DEGRADED` / `UNREPRESENTABLE` consts, one value per
  wire string, so a built-in's name has no second spelling and needs no
  reserved-name rule. `Fidelity` is the closed view `Loss::fidelity` returns, and
  is where a consumer switches; `Loss` consequently loses its `Copy` derive
  (#1091, #1142). See `docs/migrations/0.98-to-0.99.md`
- refactor(core,typst,pdf)!: workspace-internal seams leave the published
  surface. `quillmark-pdf`'s `reader`/`writer` modules and `quillmark_typst::emit`
  become `#[doc(hidden)]`; the op-wire encoders emit an unknown's `attrs` in
  caller key order, the redundant per-encoder sort having been dropped (canonical
  content bytes are unchanged, the terminal sort still running) (#1095). See
  `docs/migrations/0.98-to-0.99.md`
- fix(wasm)!: a `Quill` or `Document` from a second copy of `@quillmark/wasm` is
  refused everywhere, as a `QuillmarkError` coded `runtime::foreign_handle` that
  names the cause and hints `npm ls @quillmark/wasm`. 0.98 half-worked there:
  `Engine` was duck-typed, so a quill from copy A rendered on an engine from copy
  B at a per-copy clone cache nobody could see, while `Document.equals`,
  `Quill.validate`, `Quill.resolve` and the typed writer met wasm-bindgen's bare
  `expected instance of Document` at a value that *is* a `Document`. The check
  covers `Engine` (`render`, `open`, `supportedFormats`, `supportsCanvas`),
  `LiveSession.apply`, the writer and reader binds, and the three by-reference
  core methods; a value that is not a handle at all keeps its own
  `runtime::not_a_document` / `runtime::not_a_quill`. Nothing changes for a
  one-copy install (#1132, #1136). See `docs/migrations/0.98-to-0.99.md`
- feat(wasm): `isUnknownLine` / `isUnknownContainer` / `isUnknownMark` /
  `isUnknownIsland` answer known-vs-unknown on each open set, so a consumer no
  longer enumerates built-in names in its own source. `ContentLineKind` is
  re-exported from the package entry point, so a `setKind` op type-checks without
  a cast
- feat(python): the Tier-1 gaps close. `doc.card(i)`, `doc.card_index_by_id(id)`,
  and `doc.seed_overlay(kind)` are the single-card, `$id`, and seed reads WASM
  already had, and the wheel ships `py.typed` plus stubs, so mypy and Pyright see
  real signatures where the surface used to resolve to `Any` (#1011)
- fix(typst): four quill-load defects — a skipped asset, an unparseable
  `typst.toml`, a skipped package file, a declared-but-absent entrypoint —
  become `RenderResult` warnings (`typst::path_skipped`,
  `typst::package_manifest`, `typst::package_entrypoint_missing`) instead of
  `eprintln!` that wasm32 has nowhere to print (#1102)
- fix(wasm): the npm package states the license the workspace actually grants.
  `package.json` declared `MIT OR Apache-2.0` where every Rust crate, the
  workspace manifest, and the only `LICENSE` file in the tree are `Apache-2.0`,
  and the package shipped no license text at all: `build-wasm.sh` copied
  `LICENSE-MIT` and `LICENSE-APACHE`, neither of which exists. It now copies
  `LICENSE`, or refuses to produce a package
- ci: the release gates the tag actually needs. New `package` (builds every
  publishable crate from its own archive and asserts each ships its `LICENSE`),
  `msrv` (holds `rust-version` to something true), and `audit` (bare `cargo
  audit` over the lockfile) jobs; the workspace moves to edition 2024 and
  declares MSRV 1.92. The `semver` job is dropped — it compared the tree's
  unbumped version against itself — and `COMPATIBILITY.md` names the writer and
  reviewer as what holds the promise instead (#1105, #1106, #1107, #1108)
- ci: the rustdoc gate covers the whole workspace. A bare `cargo doc` walks
  default-members and never lints a crate outside it — the blind spot that let
  the `Delta` links rot on the WASM surface and four more in the published
  `quillmark-content`. `--workspace` needs no `--exclude` and covers the next
  such crate on the day it lands
- test(fuzz): the four JSON decode lanes the bindings expose gain coverage
  (#1104)
- docs(canon): `COMPATIBILITY.md` states the crate-API promise — what
  `#[non_exhaustive]` does and does not buy, when to mark an enum, and what no
  attribute sweep catches
- docs(all): the em-dash leaves comments and prose, folded to a colon, comma,
  semicolon, or parentheses across ~2900 sites. `dense-prose` banned it while
  every exemplar it named used it; the corpus now matches the rule. A handful of
  diagnostic and CLI message strings repunctuate with it (`edit::body_only`,
  `validation::must_fill`, the pdfform bind errors, `--help`); codes, severities,
  and paths are unchanged. The character stays where it is the subject rather
  than punctuation: the WinAnsi encoding table, the YAML en/em-dash fixtures, and
  `docs/migrations/` (#1135)
- chore(core): `serde-saphyr` moves to `1.0`. The two call sites that built
  `Options`/`SerializerOptions` with struct-literal-plus-`..Default::default()`
  now go through the crate's own `options!`/`ser_options!`/`budget!` macros,
  which the 1.0 release requires since both structs are `#[non_exhaustive]`.
  `serde_saphyr` types stay out of `quillmark-core`'s public API (see the YAML
  engine entry above), so nothing downstream moves

## v0.98.0 - 2026-07-28

Five breaking changes, all covered by `docs/migrations/0.97-to-0.98.md`.
Stored documents are unaffected: a `0.97` blob loads byte-identically and
`0.98` writes the same bytes for the same content.

- feat(core,content,wasm)!: the block vocabulary opens. A line's `kind` and a
  container's name join the mark `type` and island `type` as open sets — an
  unrecognized value round-trips opaque and renders as its nearest safe
  neighbor instead of failing the load, so adding a block construct is no
  longer a document schema-version event. `LineKind` and `Container` each gain
  an `Unknown { tag, attrs }` variant (exhaustive matches need an arm) and
  `Line` / `LineKind` / `Container` drop their `Eq` derive, matching `MarkKind`;
  `Invariant` gains `ReservedUnknownLineKind` / `ReservedUnknownContainer`, so
  an unknown may not reuse a built-in name. `ContentLine.kind` and
  `ContentContainer.container` gain an open TS arm, so a bare discriminant check
  no longer narrows — `isHeadingLine` / `isCodeLine` / `isListItemContainer`
  join the existing guards in `@quillmark/wasm/runtime`. `Loss` describes
  fidelity; it does not gate export (#1054). See
  `docs/migrations/0.97-to-0.98.md`
- refactor(core,wasm,python,cli)!: `OutputFormat::Txt` is removed — the Rust
  variant, Python's `OutputFormat.TXT`, the TS `'txt'` arm. No backend listed it
  in `SUPPORTED_FORMATS`, so every path reaching it failed at render time;
  `--format txt` and `from_str("txt")` now fail at the argument instead, and
  there is no replacement format (#1058). See
  `docs/migrations/0.97-to-0.98.md`
- refactor(typst,pdfform)!: `format_not_supported` was three codes for one
  condition. Both backends now emit `backend::format_not_supported`, alongside
  the sibling `backend::apply_unsupported` — route on the namespaced code
  (#1057). See `docs/migrations/0.97-to-0.98.md`
- refactor(core)!: dead public surface is removed — `ReadValue::as_text` /
  `as_value` (match the variants), `Quill::list_files` / `list_subdirectories`
  (`quill.files()` owns the walk), and `QuillValue`'s eight `Deref`-shadowed
  accessors (`QuillValue: Deref<Target = serde_json::Value>` resolves each call
  unchanged — no action). `prescan`'s helpers and `reader::err` drop to
  `pub(crate)` (#1066, #1064). See `docs/migrations/0.97-to-0.98.md`
- fix(python)!: rendered regions cross the boundary as `DocPath`. Python
  returned plate-space addresses no document API accepts; the plate→`DocPath`
  translation now lives in core as `regions_to_doc_path` and both bindings call
  it, so `RenderResult.regions[].field` reads `main.body` /
  `cards.<kind>[<i>].<field>` as WASM already did (#1063). See
  `docs/migrations/0.97-to-0.98.md`
- fix(content): three codec holes close. A mark spanning an island slot no
  longer swallows it; a `LineKind` that disagrees with its segment is caught
  (`Invariant::LineKindMismatch`, carrying a `LineKindMismatch` reason); and
  decode is bounded — `MAX_NESTING_DEPTH` is enforced at the door, and a wire
  position is read checked rather than `as usize`, which on wasm32 landed
  `2^32 + 5` at position `5` (#1051)
- fix(content): export's verify-and-drop safety net is bounded per line rather
  than per mark; `CONVERT.md` states the import↔export coupling the net implies
  (#1052)
- fix(content): markdown import keeps a literal `*` that abuts strong or
  emphasis — `a***a**` imports as `a`, `*`, strong `a`, where the deleted fixup
  dropped the typed star. Removing it also fixes a backslash escape or entity in
  such a span re-entering the content as literal source bytes (#1053)
- feat(core): `FileTreeNode`, `QuillIgnore`, `QuillConfig`, the schema types,
  and `ValidationError` are nameable from `quillmark` and from `quillmark_core`'s
  root, so in-memory quill construction and schema reading need no direct core
  dependency (#1055)
- refactor(core,pdfform): `From<PdfError> for RenderError` replaces two
  byte-identical `map_pdf_err` copies; `lopdf`, `js-sys` and `wasm-bindgen`
  hoist into `[workspace.dependencies]`, the no-op `default-features = false`
  pins drop, and `publish` is explicit on every crate (#1070, #1064)
- docs(core): the card-kind validation line is documented where it lives —
  construction (`Document::make_card`, `TryFrom<CardWire>`) is permissive
  data-shaping and insertion is the gate, returning `edit::invalid_kind_name`
- docs(canon): `ERROR.md` stops documenting a `fmt_pretty_with_source()` that
  does not exist and states what carries the source chain: serialization to
  both bindings, no Rust formatter. `error.rs`'s divergent path-grammar copy
  gives way to a pointer at `path.rs` and the canon anchor table (#1061, #1069)
- docs(canon): prune — canon stops re-documenting what `docs/` owns and states
  each fact once; the lint checks truth rather than shape, and the CI job table
  lists every job
- test: test clusters go table-driven, wrong-altitude binding tests move down to
  core, tests that pin foreign behavior or assert nothing are dropped, and one
  walker and one quill builder replace the per-suite copies (#1060, #1056,
  #1065, #1062, #1067)
- ci: the zero-backend configuration is built and tested — `--workspace
  --all-features` forces `typst` on, so that branch never compiled (#1068)


## v0.97.0 - 2026-07-24

- core: one take_item primitive behind the three payload removers
- core: fold the incoming-card guard shared by push_card/insert_card
- tests: dense-prose fix on a muddy reinstatement comment
- canon/docs: §Card-id identity — the third twin; wasm surface follows
- core: card $id is the durable handle — unique per document, guarded
- wasm: add island/mark discriminant guards; USV docs on mapPos/rebase
- ci: lint the wasm binding's rustdoc to catch intra-doc breakage
- Canonize anchor-id policy: caller-supplied, unique, invariant
- Fix stale resolve() docs: body is a sibling, not a `$body` row
- docs: prune per pruning pass — archive old migrations, cut duplication
- docs: public integration pages for the high/medium doc gaps
- docs: dense-prose sweep and accuracy fixes in docs/ and canon
- docs(wasm): fix broken [`Delta`] intra-doc links (#1034)
- wasm: rename Document.get → getStored so the verbatim read carries its lane


## v0.96.0 - 2026-07-23

- docs(spec): keep plate JSON out of the markdown spec
- docs(core): fix doc-lint and mkdocs-strict CI failures
- core: gate plate $body at construction, not post-hoc strip
- docs(core): trim duplicated $kind comment in to_plate_json
- core: plate $body/$kind absent on undefined (#1030)
- core: compile_data conforms once — ladder consumes the gate's coerced output
- rename: view()→reader(), fieldStates()→resolve() across core and bindings
- core: one shared resolver behind compile_data and field_states
- content: dense-prose + simplify pass on the #1002 decision
- content: decide island-id ↔ content-hash determinism (#1002)
- Re-key runtime.d.ts geometry docs onto the canonical DocPath form
- Canon lint cleanup: dedup spine restatements, fold CI job, shape-based anchor check
- Canon docs infra: single-source the spine in prose/README.md, enforce in CI
- release: strip the seed coverage comment by construction; fix blank-notes fallback
- Reject signed years in date/datetime grammar (#1008)
- core: silence unused_must_use on Payload::insert in tests
- DocPath: root every main-field address at `main`
- fieldStates: ordered rows carrying `name`, body as a sibling (drop `$body`)
- Descope phase 5: cut the conformance suite, contractVersion, and phase-plan docs
- Contract phase 5 — conformance suite + contractVersion
- Contract phase 4 — typed-surface completion: QuillCardUi.groups + island props
- Phase 3 doc: separate tested value-parity from structural source-rung claim
- Fix rustdoc intra-doc links on EditError::doc_path / RenderedRegion.field
- simplify pass: hoist kinds vec, unify render-sidecar addressing, merge doc_path arms
- Contract phases 2–3: geometry DocPath unification, mutator paths, lean fieldStates
- Repivot the contract rework around consumer evidence (phase-doc rewrite)
- Qualify card fill diagnostics by schema-declared kind only (#1014)
- Contract phase 2 — canonical DocPath (+ phase 1 wasm test fix) (#1012)
- edit:: diagnostic codes on mutator failures (contract phase 1) (#1006)
- Add document-contract rework phase plan (#1005)
- content: centralize island type dispatch behind KnownIslandType (#985) (#1001)


## v0.95.1 - 2026-07-19



## v0.95.0 - 2026-07-19

- release: re-cut to finish binding dists after the first publish run cancelled mid-flight
- **breaking** typst: a present `date` / `datetime` field lowers to a click-to-edit value-object — `(value: datetime(..), display: (..args) => text(value.display(..args)))` — instead of a bare Typst `datetime`, so the rendered glyphs are born at a generated `text(..)` node carrying a region keyed on the field's schema path: a date placed by a vendored package, or a card's date riding a shared loop variable, is click-to-edit. A blank date stays `none`, so `!= none` guards hold. Plates migrate two shapes: `(data.f.display)("…")` (paren form — the stored `display` is a closure on a dict, not a method) and `data.f.value` for anything native (comparison, `.year()`-family components, datetime-consuming packages). The tonguetoquill flagship quills' `display-date` dispatches on `type(date)`, since their `datetime.today()` blank-date fallback stays a native datetime (#990)
- **breaking** core,typst,pdfform: split the `datetime` field type into strict `date` and `datetime` (#717/#799 resolved) — `date` accepts a bare `YYYY-MM-DD` and rejects any time component; `datetime` accepts offset-less wall-clock `YYYY-MM-DDThh:mm[:ss]` (seconds zero-filled) and rejects timezone offsets, the space separator, fractional seconds, and bare dates. Offsets are rejected, never dropped (the engine does no zone math); storage stays verbatim; no truncation in either direction. A `date` lowers to the three-component Typst `datetime(year:, month:, day:)` (unchanged emission) and a `datetime` to the six-component constructor, carrying the wall-clock time. The transform schema marks `date` as `format: "date"` (keeping `format: "date-time"` for `datetime`), and the blueprint reads `date<YYYY-MM-DD>` / `datetime<YYYY-MM-DDThh:mm[:ss]>`. Most deployed `datetime` fields hold a bare date and migrate to `type: date` with byte-identical data; the fixtures (usaf_memo, cmu_letter) do so. No deprecation alias — `datetime` rejecting a bare date is the decided end-state (#991)
- **breaking** python: the binding commits to the typed lanes — field I/O flows through `quill.writer(doc)` / `quill.view(doc)` exclusively and `Document` is quill-free data and structure. Removed (WASM-only by scope, not lag — their audience is not a Python audience): the opaque field store (`store_field` / `store_fields` / `store_fill` and card twins), the content lane (`install` / `revise` / `apply_change`), the quill-free field reads (`get` / `get_card_field`), and the module codec fns (`import_markdown` / `export_markdown` / `rebase` / `map_pos`). The composable `$ext` / field-remove card twins fold onto a trailing `card=None` selector and `push_card` folds into `insert_card(card, at=None)`. Mirrored additions: `writer.revise_field` (the `Delta` receipt is not surfaced), `writer.add_card(kind, fields=None, body=None, at=None)`, `writer.card(i).kind`, and the `Writer` / `CardWriter` / `View` / `CardView` handle classes exported from the package (#970)
- **breaking** core,wasm,python: complete the `RichText` → `Content` residual sweep (#976 folds into #982) — retire the last informal-`corpus` / model-`RichText` *identifiers* the mechanical rename missed: `CorpusHit` → `ContentHit`, `EditError::CorpusApply` → `EditError::ContentApply`, `RichtextDecodeError::NotCorpus` → `NotContent` (the codec-specific `RichtextDecodeError` type itself is kept), storage DTO `CanonicalRichText` → `CanonicalContent` (serde is unchanged — no wire migration), and the model-generic Typst emitter `emit_richtext` / `emit_richtext_inline` → `emit_content` / `emit_content_inline` (they lower any `Content`, richtext *or* plaintext). Also fixes the `ParseError` display strings `richtext json …` → `content json …`, the doubled-word find-replace debris (`content content model` → `content model`), and stale prose/comments across canon and rustdoc. Schema/codec names are untouched — `richtext` / `plaintext` tokens, `FieldType::{RichText,PlainText}`, `field_richtext` / `FieldRichtext*` / `apply_field_richtext_change`, `richtext(inline)` — they name codecs, not the model. "corpus" is purged from the tree entirely, including the ordinary-English test-set names that meant a *collection* of fixtures (`fixture_corpus` → `fixtures`, `synthetic_corpus` → `synthetic_inputs`) (#982)
- **breaking** core,wasm,python: a schema-bound read view — `Quill::view(&doc)` / `quill.view(doc)`, the read twin of `quill.writer(doc)`. `view.get(addr)` interprets each field by its declared type (a `richtext` field → markdown, a `plaintext` field → its literal text via the plaintext codec, every other type → its canonical value verbatim), returns absent as `undefined` / `None`, and — the authority the quill-free `getMarkdown` lacks — throws `UnknownField` for a name the schema does not declare and `FieldRichtextDecode` for a content field holding an undecodable value. Core `TypedReader::get` returns a `ReadValue` (`Markdown`/`Plaintext`/`Value`); `view.card(i)` is the card cursor; core adds `Card::field_plaintext` (the `to_plaintext` twin of `field_markdown`). **`getMarkdown`'s field half retires**: `getMarkdown` / `get_markdown` / `get_card_markdown` are now body-only (WASM `getMarkdown` takes a `CardAddr`, a present `field` throws; Python drops the `name` parameter) — a field's markdown is read through `view.get`. The quill-free body projection stays on `Document` (#978)
- **breaking** content: one delta-application contract — implicit trailing retain is `try_apply`'s semantics (a short delta retains the untouched remainder; the error is over-consumption only), `apply` panics on an over-long delta instead of clamping (clamping is silent corruption), and `extend_to_base` is removed. `split_line` / `join_line` rebase marks through their one-char `\n` splice with `map_pos` — the same mapping the text-delta channel uses — so marks no longer drift across line ops and `apply_field_change` canonicalizes once (a single terminal normalize instead of one per stage); line sync rebuilds in one forward pass instead of per-`\n` `Vec` splices. Mark ops are specified in final-text coordinates (post-delta, post-line-op — the frame they validate against) (#926, #987)
- **breaking** core: storage blobs tagged `@0.81.0` / `@0.82.0` fail as an unknown schema version — the read-only `V0_81_0` / `V0_82_0` DTO trees and their forward migrations are retired (nothing persisted on this lineage predates `@0.92.0`; `0.82.0` was yanked). `V0_92_0` stays the oldest shape read, and its payload types back the current write path. DOCUMENT_STORAGE.md records variant retirement as the policy when no stored population remains (#929)
- **breaking** core,wasm,python: the markdown projection stops appending a trailing newline — `to_markdown` projects a *value*, not a file, so `field_markdown` / `body_markdown` (WASM `getMarkdown` / `exportMarkdown`, Python `export_markdown` / `get_markdown`) no longer grow a `\n`; `writer.set("subject", "Hello")` reads back as `"Hello"`, not `"Hello\n"`. The content fixed point is unchanged (import is newline-insensitive) (#965)
- **breaking** all: rename the content genus off its codec's name — crate `quillmark-richtext` → `quillmark-content`, type `RichText` → `Content` (and `RichTextLine`/`RichTextContainer`/`RichTextMark`/`RichTextIsland` → `ContentLine`/…), const `RICHTEXT_MEDIA_TYPE` → `CONTENT_MEDIA_TYPE` and its wire string `application/quillmark-richtext+json` → `application/quillmark-content+json`, `#[serde(skip)]` companion caches `FieldSchema::{default,example}_corpus` → `_content`, `SegmentMap.corpus: Range<usize>` → `.content`, Typst-emitter `EmittedContent` → `Emission` (it is markup + source map, not a Typst `content` value). Schema tokens `richtext` / `plaintext`, `FieldType::{RichText,PlainText}` variants, and the codec-specific `field_richtext` / `FieldRichtext*` / `apply_field_richtext_change` / `richtext(inline)` surface are unchanged — those name codecs, not the model. Canonical body JSON is nameless, so stored documents don't migrate; `contentMediaType` consumers pin to the new spelling. Retires the informal "corpus" noun to end the code/prose split (#976)
- **breaking** core,wasm,python: `getMarkdown` / `get_markdown` / `get_card_markdown` stop conflating an absent field with a present-but-not-richtext one — a present field that does not decode as richtext (a scalar/array/object a `storeField` wrote) now throws `FieldRichtextDecode` instead of reading back `undefined` / `""`; absence still returns the absent shape. Core `Card::field_markdown` becomes `Option<Result<String, RichtextDecodeError>>` (the projection twin of `field_richtext`). Rule: absence returns, mismatch raises; read the raw value with `get` (#968)
- feat(core,wasm): typed, anchor-preserving field revise — `TypedWriter::revise_field` / `CardWriter::revise_field` and `writer.reviseField` / `writer.card(i).reviseField` wrap core `Card::revise_field_checked` (diff-rebase surviving anchors, then schema-conform the result); the schema-bound verb lives on the writer, where the schema is (#957, #966)
- **breaking** wasm: the quill-taking `Document` methods become the hidden ABI under the writer — `commitField` / `commitFields` / `addCard` → `_commitField` / `_commitFields` / `_addCard`, dropped from the `.d.ts`; remove `doc.reviseChecked` (no runtime consumer — use `writer.reviseField`). The visible `Document` class then carries zero quill-taking methods (#966)
- **breaking** core: rename `EditError::BodyImport` → `EditError::Import` (message `body import failed:` → `markdown import failed:`) — the variant also fires on field-path imports (`revise_field`), where "body" misnamed it (#966)
- **breaking** wasm: fold `pushCard` into `insertCard(card, at?)` — one insertion verb per lane, absent `at` appends; `insertCard`'s parameters reorder to `(card, at?)`. Delete the deprecated `replaceBody` alias (use `revise({}, md)` or `writer.setBody`) (#961)
- feat(core,wasm): positioned card insert — `TypedWriter::add_card` / `writer.addCard` and the `addCard` ABI take an `at` position, so a positioned typed insert is one atomic call instead of `addCard` + `moveCard`; add `TypedWriter::remove_card` (mirrors JS `writer.removeCard`) and a JS `CardWriter.kind` getter (mirrors core `CardWriter::kind()`) (#961)
- **breaking** core: `Payload::insert` / `insert_fill` now validate the field-name and value-depth invariant at the boundary and return `Result<_, FieldViolation>`, closing the `payload_mut().insert(...)` hole that let a direct caller build an invalid document; pre-validated internal callers use the new `pub(crate)` `insert_unchecked` / `insert_fill_unchecked` (#958)
- feat(wasm,core): single-card reads — `doc.card(i)` (throws out of range), `doc.cardIndexById(id)` (first match; `$id` is non-unique), and `doc.seedOverlay(kind)`, backed by core `Document::card(i)` / `find_card(id)`. Reading one card, resolving a `$id`, or fetching a `$seed` overlay no longer serializes the whole `cards` array or main card (#956)
- **breaking** core: parse warnings live only on `ParseOutput` — the redundant `Document::warnings` field + `warnings()` getter are dropped and `Document::from_main_and_cards` no longer takes a `warnings` param (`Document` `PartialEq` is now a plain derive) (#959)
- **breaking** core: collapse the two parse functions into one entry — `Document::from_markdown` and `Document::from_markdown_with_warnings` are removed in favor of `Document::parse(md) -> Result<Parsed, ParseError>`, and `ParseOutput` is renamed `Parsed`. A document-only caller writes `parse(md)?.document`. Bindings are unaffected: WASM `Document.fromMarkdown` / Python `Document.from_markdown` keep their names and their `doc.warnings` getter (#964)
- feat(wasm,python): keyed card reads `getCardField(index, name)` / `getCardMarkdown(index, name?)` (py `get_card_field` / `get_card_markdown`) — the card-indexed twins of `get` / `getMarkdown`, mirroring the `commitCardField` / `setCardField` write verbs so card reads no longer require a `payloadItems` walk (#953)
- feat(content,wasm,python): `LineOp::SetContinues { line, continues }` — hard breaks lower op-wise. Split, join, and a text-delta `\n` all mint `continues: false` lines, so a within-block hard break (a paragraph hard break, a code fence's interior line) had no op and fell back to a whole-install, losing that edit's identity anchors. Threaded through the wire codec into WASM `applyChange` (TS union updated) and Python; `continues: true` on line 0 is rejected with `ApplyError::FirstLineContinues` before the write, leaving the content untouched (#949)
- feat(wasm): the runtime root re-exports the edit vocabulary its own signatures reference — `Content` / `ContentLine` / `ContentContainer` / `ContentMark` / `ContentIsland`, `Addr` / `Delta` / `Assoc` / `LineOp` / `MarkOp` / `ChangeBundle`, `CardInput` / `PathStep` — as type-only exports (single entry point preserved; no `/core` subpath), with a presence guard so a dropped re-export fails `npm run typecheck` (#948)

## v0.94.0 - 2026-07-15

These notes cover everything since v0.92.1. No 0.93.x was separately
published — the 0.93 milestone folds into this release, so the upgrade path
from 0.92.1 is the `0.92-to-0.93` and `0.93-to-0.94` guides read in sequence.

- feat(wasm): the live-session / canvas-paint surface graduates from
  `@experimental` to stable — `Engine.open`, `LiveSession`, `apply` /
  `ChangeSet`, `paint` / `PaintOptions` / `PaintResult`, `PageSize`, and the
  `supportsCanvas` probe are now the committed preview API. The tag is dropped
  from the runtime `.d.ts` / `.js`, the wasm README, and `PREVIEW.md`; further
  shape changes follow the normal deprecation path rather than landing in any
  0.x. `Engine.render` / `supportedFormats` remain the one-shot path
- refactor(core)!: field ordering becomes fully structural — `ui.order` is
  removed and an authored `order:` is a load error. Field and card-kind display
  order is now the key order of the emitted schema (declaration order, backed by
  an `IndexMap`), and the auto-stamped `order:` integer disappears from
  `QuillConfig::schema()`; consumers walk the maps in key order instead of
  sorting on a stamped index. Typed-dictionary / typed-table-row properties
  render in declaration order, not alphabetically (#941). See
  `docs/migrations/0.93-to-0.94.md`
- feat(core)!: a card-level `ui.groups` registry gives groups identity and
  order. `ui.group` becomes a validated reference to a snake_case id
  (`quill::unknown_group` for a dangling ref); the registry's declaration order
  fixes group display order, labels derive from the id with a `title:` override,
  and a bare label-as-identity group is deprecated (`quill::implicit_group`). A
  nested `ui.group` is a load error (`quill::nested_group_not_supported`) (#941).
  See `docs/migrations/0.93-to-0.94.md`
- feat(core,typst,wasm,python)!: `plaintext` and a first-class `enum` join the
  schema. `plaintext` is navigable unformatted prose carried over the richtext
  corpus (a literal codec, with a `plaintext(field)` helper on the Typst side);
  `enum` is promoted to `type: enum` + `values:`, and the `enum:` modifier on
  `string` is deprecated for one release. `string` narrows to open scalar data
  (#938). See `docs/migrations/0.93-to-0.94.md`
- refactor(core)!: `type: richtext(inline)` retires — declare `type: richtext`
  with `inline: true`. The old token is a hard `quill::field_parse_error`, and
  `inline: true` on a non-richtext field is likewise rejected. Blueprint still
  emits `richtext(inline)<markdown>` and `build_transform_schema` gains
  `quillmark:inline: true`, both derived from the flag; documents and corpus
  wire shapes are unaffected. See `docs/migrations/0.93-to-0.94.md`
- refactor(pdfform)!: `form.json` slims to a binding layer (`form@0.2.0`). Bound
  `fields` drop `type` / `options` / `multiline` (derived from the schema
  field's kind, `enum` values, and `ui.multiline`); unbound widgets move to a
  `widgets` section; binding runs at load, so a bad `schema_field` fails with
  `pdfform::dangling_binding` / `pdfform::unbindable_field` instead of a silent
  blank. `form@0.1.0` is rejected and `$cards` absolute-index addressing is
  removed. Widget geometry is placed once at bind, not per render (#940). See
  `docs/migrations/0.93-to-0.94.md`
- refactor(core,richtext,wasm,python)!: the binding write surface settles into
  two tiers over a document-free corpus codec. `quill.writer(doc)` (wasm and
  Python alike) is the documented default — typed `set` / `set_all` / `setBody`
  / `addCard` / `card(i)` and quill-free `get` / `getMarkdown` reads — layered
  over the corpus lane (`importMarkdown` / `exportMarkdown` / `rebase` / `mapPos`
  plus the addressed `install` / `revise` / `applyChange` verbs) and the opaque
  `setField` primitive. The eager `bodyMarkdown` / `fieldMarkdown` projections
  and the per-address body writers retire pre-release; `replaceBody` /
  `replace_body` / `update_card_body` alias for one cycle; richtext fields gain
  the anchor-preserving `revise_field`; the addressed `commit(addr, …)` is
  deleted (subsumed by the writer). A core-vs-bindings parity table governs
  drift (#925, #932). See `docs/migrations/0.93-to-0.94.md`
- refactor(wasm)!: the `Card` shape splits by direction — a read `Card` always
  carries `body: RichText`, while `pushCard` / `insertCard` take a `CardInput`
  whose `body` still accepts a markdown string and whose non-`kind` fields are
  optional (#917). The card-write verbs become mechanical twins of their
  main-card names: `updateCardField` / `updateCardFields` rename to
  `setCardField` / `setCardFields` (#895). See `docs/migrations/0.93-to-0.94.md`
- fix(typst/overlay): underline / strike decoration ink no longer truncates
  `$body` field regions — the region geometry is taken before decoration strokes
  extend the glyph ink box, so a highlighted body field's box matches the text
  instead of the overrun (#937)
- chore: migrate org references `quillmark-org` → `borb-sh` across the tree
- fix(richtext): the markdown-export codec never leaks a delimiter into the
  corpus. An editor `apply_mark_ops` mark can wrap a span markdown can't
  represent (a `strong`/`emph`/`strike` edge on punctuation/symbols/whitespace,
  or abutting a literal `*`) — the run would re-import as literal `**`/`*`/`~~`
  text (bolding `a.` used to export `**a.**b`). `to_markdown` now verifies each
  rendered line by re-parse and drops any mark whose emission would alter the
  text, so the text always round-trips; only the unrepresentable formatting is
  lost. Import-domain corpora are unaffected (still an exact fixed point).
- feat(core,wasm,python)!: typed field writes via schema-carried types. One
  per-type write dispatch (`conform_value(value, schema, mode)`) unifies the
  render floor's coercion with a strict-write mode behind a `Leniency` flag; one
  typed writer per address, `Card::commit_field(name, value, &FieldSchema)`,
  dispatches on the schema — the write surface stays O(1) in field types. Adds
  `EditError::FieldConform` for non-richtext mismatches (richtext keeps
  `FieldRichtextDecode` / `FieldRichtextNotInline`). A schema-bound
  `TypedWriter` (`Quill::writer(&mut doc)`) is the front door: `set` / `set_all`
  resolve field types and strict-commit; a name the schema does not declare is a
  typo on the typed path, so it fails with `EditError::UnknownField` instead of
  falling to the opaque store (#918) — opaque storage stays available through the
  raw `set_field` / `setField` / `setCardField` verbs. Bindings gain
  `commitField` / `commitCardField` (wasm) and `commit_field` /
  `commit_card_field` (Python, net-new — Python had no richtext field writer).
  The pre-release richtext-specific writers are removed in the same cycle:
  `Card::set_field_richtext`, wasm `setRichtextField` / `updateCardRichtextField`
  — use the typed writer, which carries the `inline` constraint in the schema.
  Strict writes drop the render floor's cross-type `Boolean`↔`Number` coercions
  and fail a shape mismatch at the write, not at a later render (#893)
- remove(core,richtext,wasm)!: delete the incremental-edit surface — the
  per-field change log and everything layered on it: `richtext::ChangeLog` /
  `FieldChange` / `StaleRevision`; `LiveSession::revision` /
  `record_field_delta_at` / `record_field_change_at` / `ensure_base_revision` /
  `map_field_pos` / `apply_for_field_delta`; the WASM `applyFieldDelta` /
  `mapFieldPos` / `revision` and the `Delta` DTO; and the `revision` stamp on
  `RenderedRegion` / `CorpusHit` (and `FieldRegion` / `CorpusHit` on the wire).
  Anchoring a caret or selection across edits belongs to the editor's own
  transaction mapping (a ProseMirror / CodeMirror `StepMap`), not a parallel
  core-side position map: the bidirectional preview↔editor cursor bridge is
  `positionAt` / `locate` over the current compile, exact inverses that never
  consulted the change log. Whole-document `apply(doc)` stays the one edit verb.
  This dissolves #886's anchor-stranding half outright and drops the
  half-built delta path behind its per-keystroke-marshalling half; `Delta` /
  `diff` / `diff_import` / the mark & line op channels remain as the corpus
  writers' substrate (`replace_body`, `import_body_delta`, `apply_body_change`)
  (#886)
- feat(core,wasm): `field_boxes(field)` / `LiveSession.fieldBoxes(field)` derive
  the whole-field highlight — one union rect per page over the field's
  `span`-bearing content segments — so a "highlight the focused field" consumer
  stops reimplementing the span-filter + per-page union by hand. `regions()`
  stays the low-level disjoint truth (#829); the helper owns the union, and is
  content-only (a scalar-reference/widget-only field returns `[]`, its box being
  a single `regions()` rect). Core `field_boxes(&[RenderedRegion], field)` is a
  pure function so the one-shot `RenderResult.regions` sidecar gets it too (#884)
- feat(core,wasm): `CorpusHit.granularity` (`HitGranularity` = `cluster` |
  `segment`) reports whether `positionAt`'s `pos` resolved cluster-exact or
  floored to the containing segment's start (origin-less ink, a multi-line code
  fence's interior), so a caret UI trusts a `cluster` offset for the caret and
  treats a `segment` one as a segment selection instead of guessing. Additive-
  optional, omitted from the wire when the backend does not report it (#884)
- fix(wasm): `Engine.supportsCanvas` and `LiveSession.supportsCanvas` gain doc
  comments cross-referencing each other: the two are spelled identically but
  answer different questions (a pre-session backend estimate vs. this compile's
  authoritative answer, which can diverge — e.g. a 0-page document) — the
  divergence is now visible where each is used instead of only discoverable at
  runtime (#883)
- fix(core): drop two rustdoc intra-doc links from public items
  (`RichtextDecodeError`, `Card::set_field_richtext`) to the private
  `decode_richtext_value`, which `-D rustdoc::private-intra-doc-links` (part of
  the lint gate) rejects since the link can never resolve for a doc reader;
  reworded to a plain code span, matching the existing convention elsewhere in
  the same file for referencing a private helper from public docs
- fix(wasm): drop the `revision?` field from the public `CorpusHit`/`FieldRegion`
  types and the broken `{@link LiveSession.mapFieldPos}` / `.revision` references
  in `runtime.d.ts`. The delta API (`applyFieldDelta`/`revision`/`mapFieldPos`) is
  not forwarded through `runtime.js`, so no published consumer could reach the
  methods those fields pointed at, and the stamped `revision` was always `0` on
  the reachable read paths (whole-doc `apply` is revision-neutral). The public
  types no longer advertise a capability the shipped `LiveSession` doesn't expose
  (#850)
- refactor(core)!: `RenderSession` collapses into `LiveSession` — a persistent,
  incremental compiler that owns preview (#778). Reads (`render`, the canvas
  seam, `regions`) serve the session's current compile; the new transactional
  `apply(json_data)` recompiles in place (on `Err` every read keeps serving the
  last-good compile) and returns `ChangeSet { page_count, dirty_pages }` so a
  preview repaints `dirty ∩ visible`. Typst applies incrementally: the session
  persists its `QuillWorld` (fonts/packages/assets parsed once), swaps document
  data via `Source::replace`, and fingerprints visible page content for the
  dirty set; pdfform re-resolves + re-flattens (cheap by construction). New
  `RenderError::ApplyUnsupported` is the seam default. The callerless
  `typst_session_of` is removed. WASM: the `RenderSession` class is renamed
  `LiveSession` and gains `apply(doc): ChangeSet`; don't re-open per edit. The
  Typst backend now evicts `comemo`'s process-global cache after every compile,
  bounding memory over long editing sessions. See
  `docs/migrations/0.92-to-0.93.md`
- remove(dotnet)!: drop the .NET binding (`crates/bindings/dotnet`, the
  `quillmark-dotnet` crate, its `csharp/` managed layer, CI job, and NuGet
  publish workflow). Second-class and unmaintained relative to WASM/Python;
  removed rather than carried as bloat. Python and WASM are unaffected.
- refactor(core)!: field regions move from `RenderResult` to a session-level
  query, `RenderSession::regions()` (WASM `session.regions()`), and are keyed on
  the quill schema field path, not the backend widget. Only the interactive
  preview path wants region geometry; a one-shot byte render (PDF/PNG/SVG) does
  not, so `RenderResult.regions` is removed and the geometry is read once off the
  compiled session without a render. `RenderedRegion` (and the WASM
  `FieldRegion`) drop `name`/`kind`/`fieldType`/`value` for a single `field`
  carrying the schema address (e.g. `signature_block`); the pdfform AcroForm
  widget name no longer leaks. A region is emitted only for a schema-bound field
  — an unbound widget produces none. `RegionKind` is removed; the `quillmark-pdf`
  `FieldSpec` gains `schema_field` and `stamp`/`flatten` return plain bytes
  (`StampResult` is gone). Regions are geometry for overlays and canvas↔editor
  cross-navigation, never a compositing input (#773). See
  `docs/migrations/0.92-to-0.93.md`
- feat(pdfform)!: the `pdfform` backend now exports PNG and SVG as first-class
  `render()` output formats (`SUPPORTED_FORMATS == [Pdf, Svg, Png]`); PNG
  rasters at `RenderOptions::ppi` (default 144). The `preview` cargo feature is
  removed — the hayro raster/SVG/PNG seam is always linked, so SVG/PNG/canvas
  work out of the box rather than behind a flag. The `quillmark` crate's
  `pdfform-preview` feature is folded into `pdfform`; in the wasm crate both the
  `typst` and `pdfform` build variants link the `web-sys` canvas painter directly
- fix(quillmark-pdf): `find_dict_value` now walks the dict as strict
  key→value pairs, so a Name in *value* position (e.g. `/Subtype /Producer`)
  is no longer mis-matched as a key; the object/dict scanners also skip
  `%`-comments, so `endobj` or a key token inside a comment can't derail
  parsing of a base PDF. The `<<…>>`/`[…]` depth walkers (`extract_outer_dict`
  and `read_value_end`'s nested-dict/array branches) skip `%`-comments and
  literal `(…)` strings uniformly, so a `>>`/`]` carried inside a comment or
  string no longer truncates a dict/array and drops the keys after it
- feat(pdfform): add the Typst-free `pdfform` backend + shared `quillmark-pdf`
  AcroForm stamping spine; rewire Typst signatures onto the spine; thread a
  `regions` sidecar through `RenderResult` and generalize the raster-preview
  seam (#749, #750). See `prose/canon/ARCHITECTURE.md` and
  `docs/quills/pdfform-backend.md` for the shipped design.
- refactor(pdfform): PDF output is always an interactive AcroForm (Technique A).
  Value-flattening is internal machinery backing the SVG/PNG/canvas raster
  outputs, never a PDF deliverable. The public `RenderOptions.flatten` knob is
  removed across core and all four bindings (it was wired only in wasm, hardcoded
  `false` in Python, and ignored in .NET)
- fix(pdfform): the flatten path transcodes values to WinAnsi (with a
  `WinAnsiEncoding` font) so accented/Latin-1 text renders correctly in the
  raster output, and clips each value to its field box so long values can't
  overflow
- refactor(quillmark-pdf): hoist the shared PDF byte-serialization (object/text
  writers, `/Info /Producer` stamp) into `quillmark_pdf::writer`, consumed by
  both the stamp and flatten paths; `find_object_bytes` now matches any object
  generation and returns the live (last) revision
- docs(canon): canonize `$ext.editor.title` as the slot for a per-card display name
- refactor(core)!: remove the hand-set `Backend::supports_canvas()`; derive
  canvas capability from the one seam instead. `RenderSession::supports_canvas()`
  (authoritative, from `page_size_pt`) and `formats_support_canvas()`
  (pre-session hint, from output formats) replace it, so the capability can no
  longer disagree with what `paint` does. The engine and WASM `supportsCanvas`
  surfaces are unchanged in shape. See `docs/migrations/0.92-to-0.93.md`
- build(wasm)!: rename the WASM engine feature `render` → `typst` (now the
  default) and add a `pdfform` build variant, so a Typst-free
  PDF-form bundle can ship without Typst. From-source builders pass
  `--features typst` where they used `--features render`; the published JS API is
  unchanged. See `docs/migrations/0.92-to-0.93.md`

## v0.92.1 - 2026-06-22

- Accept uppercase field names; reserve only `$`-prefixed keys (#730)
- docs: canonize $ext.editor.title as per-card display name slot (#729)


## v0.92.0 - 2026-06-22

- 0.92 technical-debt sweep: correctness, $seed hardening, de-duplication (#727)
- dotnet: add $seed namespace writers (parity with Python/WASM)
- refactor(core): unify $ext/$seed into one out-of-band Meta concept
- Cleanup: simplify QuillWorld::font to a single expression
- Cleanup: de-narrate comments, sync canon binding tables with .NET
- dotnet: fix stale schema version (CI) + review-flagged polish
- docs(migration): cover the !fill → !must_fill rename in the 0.92 guide
- dotnet: fix native-lib copy to test project + two correctness bugs
- Remove Document.seed(kind) for strict ext/seed symmetry
- dotnet: expose $seed on the Card DTO
- refactor(dotnet): rename engine class Quillmark -> QuillmarkEngine
- Reject !fill: treat as a noncanonical tag, not a fill alias
- Fix binding build break + warn on unsupported fill positions
- fix(dotnet): resolve engine type in test namespace (CS0426)
- docs(dotnet): trim README to a dense, consumer-focused surface
- Address review nits: loud divergence, docs, coverage
- Add storage schema 0.92.0: persist nested !must_fill
- docs(canon): consolidate binding overviews into BINDINGS.md
- feat($seed): reject $seed on composable cards (root-only, like $quill)
- docs: move dotnet binding into canon, delete DESIGN.md
- docs: reframe QmBytes by-value return as a tested assumption, not a defect
- Carry nested !must_fill across the live wire (CardWire)
- Address review: trap FFI panics, fix depth-limit asymmetry, Equals contract
- Detect nested !must_fill on sequence-item inline first key
- docs($seed): correct two claims flagged in review
- Fix invalid '--' inside XML comment in Quillmark.csproj
- fix(docs): drop canon links that break mkdocs --strict
- Promote .NET binding: CI test job, NuGet release, first-class docs
- Spike: .NET binding symmetrical to the Python binding
- Capture and round-trip nested !must_fill markers
- Make QuillValue an annotated value tree (fill on nodes)
- fix(bindings): bump currentSchemaVersion to 0.92.0; add $seed JS test
- Rename !fill tag to !must_fill (accept !fill as deprecated alias)
- docs($seed): document the per-kind seed-overlay key across canon and spec
- test($seed): cover parse/emit/storage, overlay layering, advisory validation
- feat(core): first-class $seed key for per-card-kind seed overlays
- Remove RenderSession and canvas-preview APIs from Python binding (#722)


## v0.91.0 - 2026-06-17

- Upgrade Typst backend to 0.15 (#720)
- Security audit: resolve 10 findings, document 6 open issues (#719)
- Hygiene pass: simplifications, dead code removal, and docs cleanup (#718)


## v0.90.0 - 2026-06-10

- **Breaking (Rust API + bindings):** `Quill` is now engine-free, validated
  data. It no longer holds a backend; the `Quillmark` engine becomes a backend
  registry + render dispatcher. Rendering and capability move onto the engine:
  `render` / `open` / `supported_formats` / `supports_canvas` take `&quill`
  (JS: `engine.render(quill, doc)` etc.). The `engine.quill` / `quill_from_path`
  factory is removed — construct with `Quill::from_tree` (JS `Quill.fromTree`)
  or `quillmark::quill_from_path`. The backend-existence
  check moves from load time to render time (`UnsupportedBackend` now surfaces
  from the first engine call). `supportedFormats` leaves `Quill.metadata` (now
  pure config) for `engine.supportedFormats(quill)`. `Backend` gains a
  `supports_canvas()` capability method (default `false`; Typst `true`),
  retiring the `backend_id == "typst"` magic string. See
  [migration guide](docs/migrations/0.89-to-0.90.md).
- **Breaking (WASM/JS types):** `QuillMetadata` drops its `[key: string]: unknown`
  index signature. Code reading removed or unknown metadata properties (e.g.
  `quill.metadata.supportedFormats`) now fails at compile time with "Property
  does not exist" instead of silently returning `undefined` at runtime. Cast to
  `Record<string, unknown>` to reach extra `quill:` YAML keys if needed.
- **Breaking (Python API):** the Python binding adopts the engine-free shape.
  Render and capability move onto the `Quillmark` engine, taking a quill:
  `engine.render(quill, doc)` / `engine.open(quill, doc)` /
  `engine.supported_formats(quill)` / `engine.supports_canvas(quill)` (were
  `quill.render(doc)` etc.). `Quill.from_path(path)` replaces
  `Quillmark.quill_from_path(path)` — the engine is no longer a loader, and the
  loaded `Quill` is engine-free. `quill.metadata` no longer contains
  `supportedFormats` (read `engine.supported_formats(quill)`) and is now a pure,
  infallible config read. Backend resolution moves from load to render time:
  `UnsupportedBackend` surfaces from the first engine call, not from `from_path`.
  See the [migration guide](docs/migrations/0.89-to-0.90.md#python).
- **Breaking (Rust API):** `QuillSource` and the orchestration `Quill` collapse
  into one core type, `quillmark_core::Quill` (held by value; the vestigial
  `Arc` is dropped). `Backend::open` now takes `&Quill`; the consumer methods
  and the `seed` module move into core; `quill.source()` is gone
  (`quill.config()` is direct). Bindings already hid `QuillSource`, so JS/Python
  consumers are unaffected by the rename.
- **WASM packaging (single root export):** the root `@quillmark/wasm` import is
  now a hand-written **canonical layer** (`pkg/runtime/`) — it re-exports the
  Typst-less core's `Quill` / `Document` **verbatim** (same classes, no wrappers)
  and adds an async **`Engine`** (`render` / `open` / `supportedFormats` /
  `supportsCanvas`) as the canonical render API. The package `exports` map has
  exactly **one** public entry point, `.` (the canonical layer); the old
  `./render` and `./core` subpath exports are both **removed**. Engine-free
  editor/validation code (`Quill.fromTree`, `Document.fromMarkdown`) still loads
  only the small internal core binary (~0.66 MB gzip) — no backend is loaded
  until you render. The Typst backend binary is **private**
  (`pkg/backends/typst/`, not in the `exports` map): the `Engine`
  lazy-`import()`s it on first render, clones the quill/document into its memory as
  data (`Quill.toTree` → `fromTree`, `doc.toJson` → `fromJson`), and manages
  those clones internally (the validated quill clone is cached per instance;
  per-render document clones are freed) — consumers never import the backend or
  cross a WASM memory boundary themselves. `Quill.toTree()` is added to core for that crossing. A release-time
  size budget still guards the core artifact against Typst regressions.
- **WASM `Engine` (descriptor-only backend registry):** `new Engine({ backends })`
  takes backend entries in **descriptor form only** — `{ load, formats, canvas }`
  with `formats` and `canvas` **required**. The constructor validates each entry
  and throws (naming the backend id) at construction. The capability probes
  `supportedFormats` / `supportsCanvas` answer from this required manifest
  **unconditionally**, never loading a backend binary or cloning the quill. The
  bare-thunk loader form and its load+clone fallback path are removed.
- **WASM `Engine` (no invalidation API):** the unreleased
  `Engine.invalidate(quill)` / `invalidateAll()` methods are removed before
  release. The backend-clone cache is keyed on the canonical `Quill` instance in
  a `WeakMap`; a quill's contents never change after construction, so the only
  invalidation semantic is to drop/replace the instance (the clone is freed with
  it via the `WeakMap` + wasm-bindgen weak-refs). An explicit invalidation API
  will ship with its first real consumer. The load-bearing invariant — a
  canonical ref is immutable content within a runtime's lifespan — is now
  recorded in `prose/canon/VERSIONING.md` (Ref Immutability).
- **WASM `Engine` (session/canvas surface marked experimental):** `Engine.open`,
  `RenderSession`, `paint`, `PaintOptions`, `PaintResult`, `PageSize`, and the
  `supportsCanvas` probe are tagged `@experimental` in the shipped types and
  README: they ship ahead of their first production consumer (the designed
  canvas live-preview path) and may change shape in any 0.x release.
  `Engine.render` and `supportedFormats` are the stable surface.
- **WASM (typed error contract):** the root exports `QuillmarkError` — a
  structural interface (`Error & { diagnostics: Diagnostic[] }`) naming the
  shape every fallible method already throws — and an `isQuillmarkError(e)`
  guard to narrow caught `unknown`s. No runtime behavior change: the WASM
  layer still throws a plain `Error` with `diagnostics` attached (there is
  deliberately no error class — a structural check works across builds and
  WASM instances). Consumers can delete their hand-rolled
  `.diagnostics`-extraction casts.
- **Breaking (Rust API + bindings):** a document's `$quill` reference is now
  **enforced** against the loaded quill. Rendering with a quill whose *name*
  differs (`quill::name_mismatch`) or whose *version* falls outside the selector
  (`quill::version_mismatch`) is a hard error via the new
  `RenderError::QuillMismatch`, in both `render` and `dry_run`. Previously a name
  mismatch was only the `quill::ref_mismatch` warning and the version selector
  was unchecked. See [migration guide](docs/migrations/0.88-to-0.89.md).
- **Fix (WASM bindings):** `Document.makeCard`'s generated TypeScript now marks
  `fields` (and `body`) as optional (`fields?: Record<string, unknown>`,
  `body?: string`), matching the doc comment and runtime behavior. They were
  typed as required because `unchecked_param_type` drops the `?` marker; the
  bindings now use `unchecked_optional_param_type`. Callers can build a bare
  card with `Document.makeCard('kind')`.

## v0.89.1 - 2026-06-10

- chore(release): v0.89.1-rc.1 (#714)
- feat(wasm)!: 0.90 canonical API — engine-free Quill, single root export, typed errors; Python parity (#713)
- Proposal: WASM bindings split (core + render) via backend-decoupled Quill (#710)
- Add version selector matching and mismatch warnings (#708)
- docs: density-optimization pass on user-facing docs (#703)
- Remove role annotation from root block metadata header (#707)
- canon: audit and correct all prose/canon/ docs (#704)
- Fix makeCard fields/body typed as required in WASM .d.ts (#702)
- Update CLAUDE.md


## v0.89.1-rc.1 - 2026-06-10

- feat(wasm)!: 0.90 canonical API — engine-free Quill, single root export, typed errors; Python parity (#713)
- Proposal: WASM bindings split (core + render) via backend-decoupled Quill (#710)
- Add version selector matching and mismatch warnings (#708)
- docs: density-optimization pass on user-facing docs (#703)
- Remove role annotation from root block metadata header (#707)
- canon: audit and correct all prose/canon/ docs (#704)
- Fix makeCard fields/body typed as required in WASM .d.ts (#702)
- Update CLAUDE.md

## v0.88.0 - 2026-06-05

- **Breaking (bindings + Rust API):** a single canonical **`Card` wire shape** now
  flows in *both* directions. Core owns it as `quillmark_core::CardWire` (with
  `From<&Card>` / `TryFrom<CardWire>`); the WASM/Python bindings serialize and
  deserialize it instead of hand-rolling their own per-card translation. The
  flat `CardInput { kind, fields?, body? }` input type is **removed**:
  `Document.pushCard` / `insertCard` (`push_card` / `insert_card`) now accept the
  same `Card` shape they return (`{ kind, payloadItems, … }`), so a card from
  `cards` / `removeCard` / `quill.seedCard` feeds straight back in. Build a fresh
  card from a flat field map with the new **`Document.makeCard`** /
  `Document.make_card` helper. A stale `{ kind, fields }` object is now a loud
  error (`deny_unknown_fields`), not a silently-empty card. The seeded per-card
  getters `quill.seedMain` / `quill.seedCard` (`seed_main` / `seed_card`) are
  exposed on both bindings, mirroring the Rust `Quill::seed_main` / `seed_card`.
- **Breaking (Rust API):** `Document::push_card` now returns
  `Result<(), EditError>` and, with `insert_card`, validates that the card's
  `$kind` is a valid, non-reserved composable kind — the cards-list invariant is
  enforced at the edit op rather than incidentally at `Card::new`.
- **Breaking (bindings + Rust API):** the schema-aware **form view is removed**.
  `Quill::form` / `Quill::blank_main` / `Quill::blank_card` (and the
  `quill.form` / `blankMain` / `blankCard` bindings) are gone, along with the
  `Form` / `FormCard` / `FormFieldValue` / `FormFieldSource` types. Validation
  diagnostics now flow through `Quill::validate(&Document) -> Vec<Diagnostic>`
  (`quill.validate(doc)` in WASM/Python), which forwards the canonical
  `validation::*` diagnostics and keeps the non-fatal `validation::field_absent`
  completeness signal that `render` demotes. Field values/defaults/order are a
  `Document` × `quill.schema` join the consumer performs directly. See
  `docs/migrations/0.87-to-0.88.md`.
- **Breaking (diagnostics):** the validation code `validation::must_fill_absent`
  is renamed `validation::field_absent`. "Must-fill" is now scoped to the
  blueprint communication surface (the `<must-fill>` sentinel and the fatal
  `validation::must_fill_sentinel`); an *absent* field is a non-fatal
  completeness signal, not a fill requirement, since the render floor
  zero-fills it. The schema cell axis is renamed accordingly: the no-`default:`
  cell is **Unendorsed** (was "Must Fill"), the antonym of **Endorsed** —
  consumers routing on the old code or label must update. Internally
  `ValidationError::MustFillUnset { source }` splits into `FieldAbsent` and
  `MustFillSentinel` and the `MustFillSource` enum is removed.
- **Breaking (bindings + Rust API):** the `example` reference document is
  removed. `QuillConfig::example()` and the `Quill.example` (WASM) /
  `Quill.example` (Python) getters are gone. Its "show me a filled-out one"
  role is served by seeding — `Quill::seed_document()` / `Quill.seedDocument()`
  / `Quill.seed_document()` — which returns a committed `Document` rather than
  an annotated string. The CLI `render` with no input file now renders the
  seeded document. Nothing consumed the example document's annotations (the
  authoring surface is `blueprint()`), so the projection collapses into the
  seed: internally the `FillSource` fork in blueprint emission is gone and the
  blueprint always renders `default:` else the `<must-fill>` sentinel.
- **wasm:** lower the npm package `engines.node` floor from `>=24` to `>=22`.
  The runtime never required 24 — `--weak-refs` needs only Node 14.6+, and the
  `using` sugar that motivated the 24 floor is optional (a `try` / `finally`
  fallback covers Node 22). The aggressive floor hard-blocked installs on Node
  22 CI/dev images under `engine-strict`.
- **wasm:** `Document.makeCard(kind, fields?, body?)` now types `fields` as
  optional in the generated `.d.ts` (was required, contradicting its docs);
  omitting it yields an empty field map, as before.
- **docs:** fix the `Quill.schema` getter doc — the returned schema **includes**
  `ui` hints (it never stripped them); the stale "ui hints stripped" wording is
  corrected. The 0.87→0.88 migration guide now documents the `fill` flag's
  `!fill`-placeholder semantics and clarifies that seeding is example-filled,
  not a blank-form replacement.
- **blueprint:** flatten `group_fields` and drop the unused group label (#697).
- **docs:** document seeding (example → absent), fix a block-scalar prescan
  bug, and add commitment-ladder docs (#691).
- **docs(canon):** dedup field-resolution semantics into SCHEMAS (#692); note
  that released migration guides are era-accurate and immutable (#695); prune
  evolutionary information from comments and canon docs (#700).

## v0.87.3 - 2026-06-04

- Complete and consolidate the $ext mutator surface (#689)
- Complete the `$ext` mutator matrix with namespace-scoped removal and
  card-indexed namespace ops: `remove_ext_namespace` (Rust `Card`,
  `removeExtNamespace` WASM, `remove_ext_namespace` Python) plus
  `setCardExtNamespace` / `removeCardExtNamespace`. Deleting a sub-namespace
  is now the preferred way to clear `$ext` state — it preserves sibling
  consumers' slots and drops `$ext` entirely once empty, where `removeExt`
  remains a blunt clear-everything escape hatch.
- **Breaking (bindings):** the whole-map card mutator `updateCardExt` /
  `update_card_ext` is renamed `setCardExt` / `set_card_ext` for naming
  consistency with `setExt` on the main card.

## v0.87.2 - 2026-06-03

- Expose $ext write path through the editor surface and bindings (#687)
- Surface prose/canon entrypoint and fix canon documentation drift (#686)


## v0.87.1 - 2026-06-01

- Make $quill reference grammar a single source of truth (#684)
- Remove stale FieldType::Date references and add rejection test (#683)


## v0.87.0 - 2026-06-01

Arrays become first-class typed fields via a required `items` element
schema, datetime is unified under a single `type: datetime` accepting the
full YAML-1.1-style timestamp range (`FieldType::Date` is gone), and object
zero values are now shape-valid. This release tightens schema-load
validation in several places — empty `properties` maps and deeper array
nesting are now rejected — and consolidates the example/default conformance
checks behind one shared primitive. Documentation now ships from GitHub
Pages instead of Read the Docs.

### Breaking changes

These are schema-load cutovers for `Quill.yaml` authors; full before/after
steps are in `docs/migrations/0.86-to-0.87.md`.

- **Array fields now require an `items` element schema** (#672). Arrays
  previously carried a single untyped `Array` type; scalar arrays were
  never coerced or validated element-wise and were always annotated
  `array<string>`. Every array field must now declare `items`, and schema
  load rejects arrays without it. The bare-`properties`-on-an-array form
  (the old "typed table") is **removed** in favor of
  `items: { type: object, properties: … }`. Migration for a typed table:

  ```yaml
  # before
  rows:
    type: array
    properties: { name: { type: string }, qty: { type: integer } }
  # after
  rows:
    type: array
    items:
      type: object
      properties: { name: { type: string }, qty: { type: integer } }
  ```

  A scalar array adds `items` directly, e.g.
  `counts: { type: array, items: { type: integer } }`. Elements now coerce
  and validate against `items` (failing at the indexed path, e.g.
  `counts[1]`), and blueprint annotations reflect the element type
  (`array<integer>`, `array<markdown>`, …). Bundled quills and the
  `usaf_memo` golden schema are migrated.
- **`FieldType::Date` removed; use `type: datetime`** (#679). `type: date`
  no longer exists. `type: datetime` now accepts the full range from a bare
  `YYYY-MM-DD` date through RFC 3339 with offset (seconds optional, `T` or
  space separator). Datetime values gain calendar validation (e.g. Feb 30
  is now rejected), and JSON Schema output emits `format: date-time` for
  all datetime fields. The WASM `FieldType` union drops `"date"`. The
  blueprint hint is now `datetime<YYYY-MM-DD[Thh:mm:ss]>`.
- **Empty `properties: {}` on an object field is rejected** (#678). An
  empty properties map carries no information (the only conforming value is
  `{}`) and is almost always a mistake. It is now treated like a missing
  `properties` key and surfaces `quill::object_empty_properties`.
- **Deeper array nesting is rejected** (#673). The documented "one level of
  nesting" contract is now enforced in a single recursive pass, closing a
  gap where `array<object<array>>` and `object<array>` were silently
  accepted. A typed table row and a typed dictionary may carry scalar
  columns/properties only; deeper shapes fail with
  `quill::nested_array_not_supported`.

### Behavioral changes

- **Object zero values are now shape-valid** (#677). `zero_value` returned
  a bare `{}` for every object field, which failed validation on any object
  with `properties` (each absent property reported as `MustFillUnset`), so
  the zero-filled render path broke for object fields. An object with
  `properties` now recurses, zero-filling each property to its own
  type-empty leaf. `{}` remains the zero only for the property-less edge
  case.
- **`example:` values are now validated** (#680). The conformance check for
  `example`/`default` literals recurses into array items and object
  properties and validates datetime format — capabilities the old
  load-time path lacked, so previously-unvalidated `example:` values are
  now caught.

### Documentation & infrastructure

- **Docs hosting moved from Read the Docs to GitHub Pages** (#671). A new
  `docs.yml` workflow builds MkDocs (strict build as a PR check) and
  deploys to Pages on a published release; RCs are skipped. `.readthedocs.yaml`
  is removed and homepage/User Guide links point at the Pages URL.
- **Canon + docs: partial documents are first-class citizens** (#670). The
  docs and binding READMEs no longer claim Must Fill fields must be supplied
  before shipping. The only hard render gate is well-formedness (values
  coerce, no surviving `<must-fill>` sentinel); completeness is a hint
  surfaced by the form view. The `format-designer/` docs tree is renamed to
  `quills/`.
- A Migration section overview page was added and wired into the nav (#674).

### Internal

- Example/default validation is consolidated behind a single
  `validate_schema_literal` conformance core shared by `quillmark-core`
  config loading and the CLI `validate` command, with author-friendly
  diagnostics preserved (#680).
- Array and markdown handling collapse into recursive passes over the
  schema in both schema-shape validation and the Typst markdown transform
  (#673).
- Doc/comment fixes from the array-items review (#675).


## v0.86.0 - 2026-05-31

Documents now render even when incomplete, the canonical card-yaml fence
becomes a bare `~~~`, and the way placeholder/illustrative values are
produced is reworked. This release also fixes two markdown→Typst
conversion bugs and stamps a PDF `/Producer` field.

### Breaking changes

- **Bare `~~~` is now the canonical card-yaml fence** (was `~~~card-yaml`)
  (#662). Existing `~~~card-yaml` documents still parse, but `to_markdown`
  re-emits the bare `~~~` form, so a document's canonical bytes change on
  its first re-emit (relevant if you content-hash or byte-compare emitted
  markdown, or store blueprint goldens). A side effect: a column-zero
  `~~~` fence in a prose body is now read as a card-yaml block — use a
  backtick fence or a non-`card-yaml` info string (e.g. `~~~rust`) for a
  literal code block. Full details and corpus-migration steps:
  `docs/migrations/0.85-to-0.86.md`.
- **`fill_blueprint()` removed** from `quillmark_core` and `quillmark`,
  along with its re-exports (#657, #665). Callers no longer post-process a
  blueprint string: fillable/illustrative documents come from
  `QuillConfig::example()`, and the render path fills placeholders itself
  (see below).

### Behavioral changes

- **Incomplete documents render instead of erroring** (#665). An absent
  Must Fill field is no longer a render error. On the render path each
  schema field resolves to its authored value, else its `default:`, else a
  type-empty zero value — applied to the plate projection only, never
  persisted to the document. Only malformed input stays fatal: a surviving
  `<must-fill>` sentinel, or a value that won't coerce/validate.
  `quill.form(doc)` still reports completeness independently of the render
  gate.
- **`default` vs `example` clarified** (#665, #663, #658). `default` is the
  value most authors want and is interpolated when a field is omitted (an
  authored value always wins); `example` documents a field's shape only and
  never renders into output. Preview and illustrative fills now draw from a
  field's `example:` when present, falling back to the leanest type-valid
  value (`""`, `0`, `false`, `[]`, `{}`, first enum variant, empty body).

### Markdown → Typst fixes (#661)

- Code is now emitted as `#raw(...)` with a string literal instead of a
  backtick fence. This fixes fenced or inline code whose content contained
  a run of three-or-more backticks, which previously closed the block early
  and rendered as markup.
- Ordered-list start numbers are preserved — a list written `3.` / `4.` now
  renders starting at 3 instead of restarting at 1.

### New API

- `QuillConfig::example()`, plus `example` getters on the Python and WASM
  bindings (#665).
- `quillmark_core::zero_value` — the single source of truth for a field's
  type-minimal value, shared by blueprint emission and the render path
  (#665).
- `RenderOptions.producer` on the core, WASM, and Python render APIs (#656)
  — overrides the PDF `/Info` `/Producer` string, which now defaults to
  `Quillmark <version>` on every Typst-rendered PDF.

### Other fixes

- PDF rendering folds the `/Producer` stamp and the signature-field
  AcroForm injection into a single incremental-update pass, preserving
  Typst's `/Creator` (#656).
- `usaf_memo`: the signature widget is now overlaid at the 4.5in signature
  block (AFH 33-337) instead of the 1in left margin, and no longer consumes
  layout flow that could push the block out of position (#660); empty
  signature fields no longer carry the `APPEND_ONLY` flag (#654).

