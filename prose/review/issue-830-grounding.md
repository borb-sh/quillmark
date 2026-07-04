# Issue #830 grounding — the content block tree against the 0.92.1 tree

Verification of [#830](https://github.com/quillmark-org/quillmark/issues/830)
(canonical `Content` block tree, markdown as a projection) against `main` at
`28b29ea`. Every claim below is checked against code; citations are `file:line`
at that commit. Sections: what checks out, what needs correction, costs the
issue does not price, internal tensions, what the current tree actively
supports, and sequencing.

## 1. Verdict in brief

The issue's mechanical claims are largely accurate — the `$id` plumbing, the
seam locations, the projection precedents, and the migration history all check
out. Five claims do not survive contact with the code as stated (§3): the
emit-path savings are relocations rather than deletions, the region run
machine is *not* untouched, the cited precedents are lossless where this
projection is deliberately lossy, the byte-range-fragility argument is
overstated for the use case it cites, and the JSON seam blocks less than
claimed. Four costs are unpriced (§4): a generative storage migration that
collides with the byte-determinism contract, explicit spec reversals on `$id`,
a wire/revision layer that is entirely greenfield and partly out-of-repo, and
the rename's ecosystem surface plus the markdown strings that remain in
schemas. The pivotal judgment call is not technical (§6).

## 2. Claims verified

| Claim | Evidence |
|---|---|
| `normalize_markdown`: CRLF + bidi strip | `normalize.rs:130-134`; also a third pass the issue omits, HTML-comment fence repair (`normalize.rs:63-126`). Runs once in `compile_data` (`compose.rs:50`), on card bodies only |
| `convert_content_value` stores a bare string into the data tree | `lib.rs:659-679` (Typst backend); array arm for `markdown[]` at `:664-676` |
| Underline smuggled via `<u>` + bespoke event fixer | `MarkdownFixer`, `convert.rs:569-861`; `<u>` rewrite at `:843-852`; `<u>` is the one allowlisted HTML tag (markdown-spec §6.2, `:399-409`) |
| `$id` fully plumbed: parse, one-per-card, emission, DTO since V0_82_0, `CardWire` hoist | `meta.rs:93-95`; `payload.rs:340-342` (invariant enforced by `upsert_meta`, `:469-495`); `emit.rs:196-198`; `dto.rs:166-167` and `:243-244`; `wire.rs:114-115` |
| Nothing mints `$id`; uniqueness enforced nowhere | zero generation sites in product code; `makeCard` hardcodes `id: None` (`engine.rs:921`); duplicate ids across cards load silently; spec disclaims uniqueness (markdown-spec `:191-193`) |
| Every prior storage migration was structural | V0_81→V0_82 `dto.rs:783-788`, V0_82→V0_92 `dto.rs:610-614`; both self-describe as purely structural |
| `QuillValue` precedent: derived projection, annotated tree | `value.rs:1-10`, `:26-47` (`Node` tree + `OnceLock` lazy JSON); QUILL_VALUE.md:14 |
| Engine vocabulary is already "content" | `SchemaMeta::content_fields` (`lib.rs:695`), `ContentWindow` (`helper.rs:34-37`), `Codegen::content_block` (`helper.rs:115-129`) |
| No revision identity exists | no monotonic revision or content address anywhere; `page_hashes` (`lib.rs:111-135`) is a per-page render-frame fingerprint for repaint, not document identity |

## 3. Corrections

### 3.1 "Kills MarkdownFixer and most of the escape layer" — relocation, not deletion

The markdown surface stays first-class, so the markdown *import* codec keeps
the fixer (`<u>` allowlisting and `***` adjacency are event-stream concerns of
parsing markdown — `convert.rs:569-861`, ~293 LOC) and the normalize layer
(`normalize.rs`) forever. They leave the render-hot path; no line of them is
deleted.

The escape layer's fate depends on an emit choice the issue does not make.
The region machinery requires generated *source text* — glyph spans resolve to
helper-file byte ranges (`span_scan.rs:137-168`), and the injection model
compiles `#let _qm_cN = [ .. ]` source (`helper.rs:115-129`). A tree emitter
that produces Typst markup keeps `escape_markup` (`convert.rs:37-53`)
verbatim. Markup-context escaping can be sidestepped only by emitting text
runs as string literals in code context, which keeps `escape_string` and
changes the emitted shape. Either way the emit path replaces ~430 LOC of
`push_typst` with a tree walker of comparable size. The real wins are
structural — per-block generated ranges recorded natively, no markdown
re-parse per `apply` — not LOC deletion.

### 3.2 "The Typst-side machinery is untouched" — the run machine is not

The issue carries two constraints that contradict each other as stated:
"sub-windows register **alongside** field windows" and "**leaf windows only**
— disjointness is the invariant that keeps the single-cursor run machine
sound."

The scan is a single-cursor machine: at most one window is in-run; any hit for
a different window — or unclassified ink — forecloses the current run, and a
same-page resume is terminal (`span_scan.rs:249-259`, `:293-322`).
Classification is first-match containment over slice order
(`span_scan.rs:160-164`); "narrowest" exists only as an ordering discipline
(chains before wides, `span_scan.rs:421-442`, asserted at `:655-665`).

- **Both window sets live (nested):** a list marker glyph carries the item
  node's span, wider than any block window, so it classifies to the enclosing
  *field* window. Interleaved marker/text hits then mutually foreclose the
  field run and the block runs — the issue's own "collapses to the first
  bullet marker" failure, reproduced one level up.
- **Leaf-only (field windows dropped):** marker glyphs classify to nothing.
  `field_at` on a bullet returns no field — a regression; today the
  whole-field window catches item-node spans — and unclassified marker ink
  suspends the in-run block window mid-field.

`field_at` itself is safe under nesting: it is per-hit classification with no
run machine (`span_scan.rs:366-396`, `lib.rs:417-434`), so narrowest-first
ordering suffices there. But `regions()` needs a genuine redesign — a
hierarchy-aware scan that tolerates enclosing-window hits without foreclosing,
or a two-pass scan, plus a decision about marker ink in the unioned field
boxes (excluded, or attributed to an adjacent block). The glyph-span scan and
byte-window substrate are indeed reusable; "untouched" is wrong for the part
that turns hits into regions.

### 3.3 The cited precedents are lossless; this projection is not

`StoredDocument`⇄`Document` and `QuillValue`⇄JSON are mechanical,
deterministic, and lossless in both directions. #830's markdown projection
deliberately drops block IDs on export, and its inverse is a *matcher* with
policy knobs. That breaks two current contracts the issue does not mention:

- PROGRAMMATIC.md's "all three surfaces produce the same `Document`" — under
  #830, `from_markdown(to_markdown(doc))` differs from `doc` in identity.
  Round-trip guarantees (BLUEPRINT.md "round-trips by construction";
  `every_quill_blueprint_round_trips_and_renders`) need redefinition as
  equality-modulo-identity.
- Storage byte-determinism is consumed for content-hashing
  (DOCUMENT_STORAGE.md:88-103). Canonical tree serialization includes IDs, so
  two field-identical documents hash differently. Inherent to identity, but a
  contract change for hashing consumers.

"The same move one level down" undersells the move: neither precedent has a
lossy projection requiring heuristic re-alignment.

### 3.4 Byte-range fragility is overstated for the use case that motivates it

`RenderedRegion` is derived per compile and served against that compile's
snapshot — `apply` is transactional and `regions`/`field_at` read the retained
`helper_source` the served document was compiled from (`lib.rs:71-75`,
`:316-342`). Click-to-focus (#829's stated goal) needs only per-compile
validity, which byte ranges provide. Cross-revision stability matters for
decorations that persist across edits, matching import, and collab — i.e.
motivations 2 and 3, not motivation 1. The honest statement is that #829 is
refused not because its key fails its own use case but because it is a dead
end for the editor/collab future. That is a bet on §6, and should be argued as
one.

### 3.5 The JSON seam blocks less than claimed

`transform_markdown_fields` (`lib.rs:802-836`) and `emit_field`
(`helper.rs:216-250`) are both backend-internal; there is no user filter stage
between them. A sidecar map `field path → Vec<(source range, generated
range)>` rides beside the data tree without touching the JSON contract —
#829's own sizing ("a few dozen lines threading offsets that already exist")
was correct. The seam argument is real only for consumers *outside* the
backend. Structure is not lost at the seam; it is never materialized outside
`mark_to_typst`'s event loop in the first place (`convert.rs:862-874`).

## 4. Unpriced costs

### 4.1 The migration must mint — and minting collides with determinism

The issue prices the frozen-parser wrinkle but not the mint wrinkle. IDs are
"minted once, never derived from content" — i.e. nondeterministic — yet the
read chain is a pure function with no write-back hook (`dto.rs:413-427`); the
storage crate never persists. "Materialize-and-rewrite on first load" has no
home in the current architecture, so a V0_92 row re-migrates on *every* load
until the consumer re-persists — under nondeterministic minting, different IDs
per load, dangling every anchor keyed on them. Deterministic derivation at the
migration boundary (position + content hash) is effectively mandatory, and is
an explicit carve-out from "never derived from content." Combined with the
markdown parse, this is the first generative migration in a chain whose every
prior step is self-describedly structural (`dto.rs:610-614`, `:783-788`), and
it needs *both* the frozen importer and the deterministic mint.

### 4.2 `$id` adoption is a spec reversal, not just three policies

markdown-spec `:191-193` promises "no validation, no uniqueness requirement;
carried through round-trip unchanged"; PROGRAMMATIC.md:79 repeats it. The
matcher's duplicate policy (new-card + fresh mint) rewrites an author-supplied
`$id`, violating "carried unchanged"; engine minting changes `Document::new` /
`push_card` semantics; and today no binding can set a card id at all
(`engine.rs:921`; no `updateCardId`). Each flip is small; together they are
normative spec changes across markdown-spec, PROGRAMMATIC.md, and the binding
surfaces, not adoption of dormant plumbing alone.

### 4.3 Revision and matching are greenfield, partly out-of-repo

The wire is index-addressed whole-card replacement (`engine.rs:990-1052`);
the only change-detection primitive is whole-document `equals()` debouncing
(`engine.rs:661-666`). Per-card base-revision detection, revision-tagged
session reads, and the matching importer are net-new across core, three
bindings, and the TongueToQuill MCP server — which is not in this repository.
"The MCP surface contract requires preservation" is a contract this repo can
publish but not enforce; the base-revision token must be carried by a server
and clients that version independently.

### 4.4 The rename's ecosystem surface, and the markdown that remains

`markdown` → `content` touches `FieldType` (`types.rs:84-126`), lowering
(`schema.rs:33-42`), blueprint special-casing (`blueprint.rs:230,245,417` and
BLUEPRINT.md's annotation grammar, where `markdown` is a documented type-slot
name with field-specific rendering rules), markdown-spec §6, every fixture
`Quill.yaml`, and external quivers (usaf_memo syncs from airmark-quiver,
#828). `Quill.yaml` has no schema-version negotiation, so either `markdown`
stays accepted as an alias indefinitely or the rename is a coordinated
ecosystem migration. The reserved `content(inline)` parameter implies a new
type grammar in `Quill.yaml` and the blueprint inline annotation.

Markdown strings also survive *inside* the model: schema `default:` /
`example:` for content fields, `body.example`, and `$seed.<kind>.$body`
(CARDS.md:127-158) remain authored markdown, cold-imported at use. Zero-fill
and seeding then mint fresh block IDs per materialization — a defaulted
content field gets new `(field, block id)` region keys every compile. Probably
acceptable; should be stated, or defaults imported once at `Quill` load and
cached.

### 4.5 Where the tree crosses the wire is undecided

`to_plate_json` emits `$body` and content fields as strings
(`mod.rs:301-341`); that shape is the core→backend contract (PLATE_DATA.md),
what the bindings serialize, and what every backend receives, including
`pdfform`. "Engine consumes Content" implies tree-on-the-wire — a plate-JSON
contract change for all backends — while keeping the wire markdown means the
Typst backend re-imports per render, reinstating the parse the model exists to
kill. The issue needs to pick.

### 4.6 Test estate

~1,890 LOC of exact-markup golden assertions in `convert.rs` (from `:876`),
plus `content_regions.rs`, `live_apply.rs`, `markdown_field_test.rs`,
`spec_conformance_probe.rs`, and the quiver round-trip suite, all assert on
the string model — before adding the ID-preservation conformance suite the
issue itself specs. Phase 1 touches every crate; this is plausibly the largest
single change in the repo's history, superseding a proposal (#829) sized at a
few dozen lines.

## 5. Internal tensions

- **"LWW per field" without a clock.** "No sync metadata in the model" plus a
  carried base revision yields three-way *conflict detection*, not
  last-writer-wins *ordering*; two writers off the same base have no defined
  winner. The contract should say "per-field three-way merge with a documented
  tiebreak," or admit the clock.
- **"Alongside field windows" vs "leaf windows only"** — §3.2.
- **"Never derived from content" vs migration determinism** — §4.1.
- **Open union vs text intents.** Unknown block types round-trip "through edit
  APIs," but `replace text range` / `set mark` require knowing where an
  unknown block's runs live. Either unknown blocks are opaque to text intents
  (editors degrade harder than "renderers degrade" suggests) or the union
  fixes a closed text-carrying envelope (runs and children in fixed slots,
  types open only in `props`). The latter is the workable shape; the issue
  should say so.

## 6. What the current tree actively supports

- **Keeping IDs out of generated source is confirmed right.** Canonical
  sorted codegen (`helper.rs:329-342`) and span-excluding `page_hashes`
  (`lib.rs:84-135`) mean an external block-ID→range table composes with
  #801's reuse guarantees: byte-identical emit for reorder-only applies
  survives, and no ID churn can dirty a page.
- **`$id`'s structural plumbing is genuinely complete** at parse, invariant,
  emission, DTO, and wire (§2) — adoption is policy, exactly as claimed.
- **`field_at` needs no rework** under nested windows (§3.2) — the pain is
  isolated to `regions()`.
- **`QuillValue` is a ready implementation template** — annotated `IndexMap`
  tree, lazy cached projection, no data mutators (`value.rs:26-47`).

## 7. Sequencing

Everything above prices out against one question the issue takes as given:
**is the structured editor a scheduled consumer or a speculative one?** The
web-app integration passes (#782, #801) show a live preview consumer; nothing
in this tree evidences a block-editor consumer. Two coherent postures:

- **Editor is scheduled.** #830's refusal of #829 is right. But move the
  editor-binding spike from a bullet inside phase 1 to a **phase-0 gate** —
  ProseMirror/Lexical/Loro disagree precisely in the corners the model
  hardcodes (mark identity, adjacent-mark merge, block-split semantics), and
  the storage schema freezes around whichever semantics land. And split phase
  1: (a) `Content` type + markdown⇄Content codecs + property suite, landed
  engine-off and exercised against the fixture corpus; (b) Typst emit codec +
  region re-key (delivers #829's feature); (c) storage bump + migration
  *last*, after the type has survived (a) and (b). As written, phase 1
  bundles the storage freeze with the type's first landing — maximum-regret
  ordering.
- **Editor is speculative.** Then #830 parks the only user-visible deliverable
  (paragraph regions) behind the largest rewrite in the repo's history, when
  the sidecar bridge (§3.5) delivers it in dozens of lines whose throwaway
  cost is a rounding error against phase 1. Regions are per-compile either
  way (§3.4); the bridge forecloses nothing.

The grounded recommendation: make the phase-0 spike the decision gate for the
whole proposal, land the bridge if the editor is more than a release away, and
resolve §4.1 and §4.5 in the issue text before any code — those two are the
decisions that cannot be revised cheaply after a schema freeze.
