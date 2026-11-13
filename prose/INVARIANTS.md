# Quillmark invariants

The rules a change to Quillmark must keep, the reason behind each, and the designs already weighed and rejected. Each rule here is one the code cannot carry where it is broken: its reason sits in another crate, in a storage promise, or in a design that looks attractive until its cost shows up a release later.

This page does not describe the system. The code, rustdoc, [`docs/`](../docs/) and [`references/`](references/) do that. A rule belongs here only if a capable reader of the code would plausibly break it. A change that breaks one either updates this page in the same commit, with the new reason, or is wrong.

## The architecture

**One markdown parser, run once, at ingest.** `pulldown-cmark` appears once in the workspace, in `quillmark-content`'s import. Every render path walks the content model; no backend parses markdown. The export is the one other caller: it re-imports a line to check itself, because CommonMark emphasis has corners no local rule captures, so export correctness is defined by import and the two codecs cannot change independently.

**Core is single-writer.** A write that names a base, the hash its writer read, lands only while the value still hashes to it; otherwise it is refused with `edit::stale_base`, carrying the current hash, and writes nothing. Core never merges. Concurrent editing, writers that answer late, sessions and history are `@quillmark/collab`'s, over Yjs. Rejected: a merge in core, which would need a history core does not keep and would fork the model collab already owns.

**Core reads no clock and no filesystem.** The render date is a required input every host supplies, and a session keeps the date it opened with. Disk walking lives in `quillmark`; core builds a quill from an in-memory tree or a `.quill` bundle. Both keep core deterministic and portable to WASM.

**A `Quill` is data.** It holds its file tree and parsed config, tagged with a declared backend id, and needs no engine or backend to construct or read. The `Engine` resolves the backend at render time, so whether a quill can render is the engine's answer, never the quill's.

**Backends are an in-workspace seam, not an extension point.** Registration is private and a session is built only from a hidden handle. A quill declares exactly one backend, so rendering one schema two ways is two quills. Preview (`page_size_pt`, `render_rgba`) is required of every session rather than declared, so a `None` from either says only that the page is out of range.

**No API stability promise before 1.0.** The workspace bumps its minor on nearly every release, which Cargo reads as a major for a `0.x` crate. `#[non_exhaustive]`, a sealed `Backend` and a semver-checks gate each promise a compatible release that a `0.x` cadence never makes; all three are the 1.0 tag's decision.

**No public signature names a dependency's type.** A third-party type in a published signature chains this crate's major version to that crate's, and to the choice of the dependency at all. YAML errors pass through one adapter for this reason, which also strips the parser's own API names from the message.

## Diagnostics

**Route on `code`, never on message text or type.** Every failure is a `Diagnostic`, and `RenderError` is a non-empty list of them with no taxonomy beyond their codes. Rejected: a failure type taxonomy, which would fork routing between types and codes and which a binding cannot carry anyway.

**Fatal means won't render, in both directions.** There are two severities and nothing else: `Error` blocks the stage that raised it, `Warning` never does. A fatal `validation::*` diagnostic means the document does not render, and a document that does not render carries one. Rejected: lint levels, warning promotion and a strict mode. A host that needs a warning to fail treats that code as an error itself.

**A code classifies; it never quotes.** Where a code is minted from a foreign message, as the Typst backend does, it comes from a closed set keyed on the message's shape, with one residual code for the rest. A code spelled from the message would put author text, a searched path or a symbol name, into a routing key and give the key one value per input.

**Codes are a published vocabulary.** [Diagnostic Codes](../docs/reference/diagnostic-codes.md) lists every code. A code leaves only in a release whose migration guide names it.

**The args contract.** `message` is the canonical English and `args` is what it interpolates, so a consumer with its own string table selects a sentence by `code` and fills it from `args`.

- Quillmark prose never rides under a key. Text minted per site stays in `message`; a coarser sentence the consumer owns beats a fluent one half in the wrong language.
- Values keep their JSON shape, since joining and pluralizing are locale decisions.
- `hint` is a function of `code` and `args`, so a datum a hint branches on is an arg. Rejected: a `hint_code`, which would be derived state on the wire.
- `path` is never an arg.
- Per code, keys are append-only and never retyped, and value spellings are as frozen as keys. A rename is a `!` change.

The table of codes and keys lives in [Diagnostic Codes § Args](../docs/reference/diagnostic-codes.md#args), and a test holds it to what the code mints. That test cannot catch a rename, which edits both sides in one commit, so the keys a named consumer outside this repository reads are pinned by a second list that does not move with the table.

**A warning rides the carrier of the stage that raised it.** Load warnings ride the quill, parse warnings the parse result, a markdown write's its receipt, and a compile's the session's current compile. A render reports what the render sees, and a session's warnings swap with its compile, so a failed edit keeps the last good set.

**Conform warns where a write would refuse.** For a value the strict write would refuse, `Quill::conform` raises that write's `edit::*` diagnostic re-namespaced `conform::*` and demoted to a warning, args and path intact, and leaves the value as authored. An ingestion pass must open a document it can repair, not reject it.

**One address crosses.** Every address that reaches a consumer, on a diagnostic, a mutator error or a region, is a `DocPath`, minted by one serializer, so a click and the diagnostic on the same place compare equal. The plate grammar (`$cards.<kind>.<ordinal>`) is published in every quill and keeps its spelling; it translates to a `DocPath` at the boundary and never crosses itself.

## The schema and the render

**What earns a `Quill.yaml` key.** A key earns its place when some surface is otherwise *wrong*, not merely underserved, and wrong needs a witness: a document that overflows, misrenders, validates falsely or is edited by the wrong control. A key then states its behavior on all four surfaces (plate, editor, authoring schema, `validate`), and "inert here, deliberately" is a valid answer that is written down. A preference is not a witness: a key costs its teaching surface from the day it exists.

**The commitment ladder: authored › `default:` › blank.** The render fills every absent value from the first source that has one. Quillmark never writes a `default` or a blank into a document. A persisted blank is indistinguishable from an authored empty, so it erases the absence signal and blinds a later schema migration to author intent, and a persisted default freezes it against a quill change.

**Null ≡ absent, finally.** A present-null value and an omitted one are one state, on every surface. `removeField` is the one unset verb. Rejected: a tri-state absent/null/value, which doubles every field's state space for one rarely authored distinction and makes a loaded-then-saved document sprout `field: null` lines.

**A blank is a property of the field, not a member of its domain.** An enum's blank is `""` and is never in `values:` (a load error), because a blank inside the domain is a real choice nobody made, and a cosmetic reorder of `values:` would change what an unanswered document renders. At `integer`, `number` and `boolean` the blank reads as an answer, so a field whose plate must see "unanswered" declares itself optional, `t?`, which renders `none`. Rejected: a uniform `none` blank, which Typst arithmetic and comparison reject, costing every plate the totality the blank exists to buy.

**A render fails only where Quillmark would have to invent what the author wrote.** Malformed input, a value that will not read as its type or markup the grammar cannot read, is fatal. Unanswered input blank-fills. Unclaimed input, an undeclared key or a card of an unknown kind, renders and warns, and stays in the document for a later schema to read. A line break in a one-line `text` folds to a space and warns. Malformed is fatal because the plate is total: every substitute (the blank, the default, a guessed coercion) asserts something the author did not write.

**Coercion is the type predicate; validation reads its result.** Validation judges each value in the form the render floor builds, so a type has one predicate. Document payloads are lenient (a bare scalar where text is expected, a one-element list), while schema literals are strict, so a schema never teaches authors a spelling it would refuse.

**Blank-fill is total at every depth.** A declared address is present on the plate however much of its container the document left out, so a plate reads it directly, never through a guarded accessor. This holds because a literal is declared where its value position is (a `default:` on a typed dictionary is a load error) and absence is inherited: each value position below an absent container cuts its own ladder.

**Variants open at most one level of conditionality.** An enum's `variants:` may sit on a card field or a typed dictionary's property, and is refused in an array element, a matrix column or another variant's field, a ban inherited by any object inside them. One level is what lets a form bind once at open, a plate branch once with no guard inside the branch, and one name be one field across worlds. A value stranded in a world the discriminant left is kept and warns, rather than being dropped at coercion, so flipping the discriminant and back loses no answers.

**The plate contract.** A quill's plate must render the empty document, its `$quill` and nothing else. A plate branching on an enum covers `values ∪ blank` exhaustively, since an `else` renders a member nobody chose. It reads a variant's fields only inside the branch that selects them. `quillmark validate` holds a quill outside the fixtures to this.

**A `default:` is an answer; a `seed:` is a prompt.** A default is what an unanswered field renders and is never persisted. A seed is starter content written into a new card, which then belongs to the card. A seed equal to what the field renders anyway is refused at load, since it freezes that floor into every card.

**Content fields are stored per codec, and the schema decides the codec.** Every content field is stored as a canonical content object, and a `text` field's spells `codec: "plain"`, so a schema-free path keeps the literal codec. `Quill::conform` derives the stored form from the current schema and value alone, holding no record of the type a value was written under.

**A text field has a type and a shape, and neither bends the other.** `type` says how characters are read, `text` literally and `richtext` as marks. `shape` says how much the value holds: `line`, `lines` or `blocks`, each holding all the one before it does. A newline and a blank line mean the same in every shape that holds them, and the body is a `richtext` of `blocks`. Syntax a shape does not hold is text, so a recipient numbered `1.` in a `lines` field is never a list. The schema-bound readers escape it in the source before the one parser runs, and a schema-free read takes a value as blocks. Rejected:

- A boolean `multiline`: formatted lines had no spelling, so a quill declared blocks and its plate split paragraphs back into lines.
- A line count such as `lines: 1`: it caps length and says nothing of structure.
- A newline rule per field: one keystroke meant two things across the fields of a form.

**A declared type change is a new quill version.** The next bound load rewrites every stored value in a retyped field, and scalar to content is lossy: markup in the string is consumed as structure. Nothing enforces the version bump, because an in-place schema edit at the same version matches every check.

**`$quill` is a pairing assertion, not a resolver.** Quillmark loads exactly one quill and enforces the reference against it: the name first, then the selector. It never picks among versions; resolution belongs to a higher layer. A selector names a compatible series, so there is no `@latest` and no lone major.

**A canonical ref is immutable within a process.** The caches between a document and its rendered output key on the ref, and none exposes invalidation, by design. To invalidate is to replace the `Quill` instance with a new one at a new ref.

**A PDF claims PDF/UA-1 only where it conforms, and no author opts in.** Every Typst PDF render tries UA-1; a refusal exports a plain tagged PDF and warns at the source, and a document with form fields exports plain because a stamped widget has no accessible name. Rejected: a `Quill.yaml` opt-in, which asks every author to know a standard, and a fatal refusal, which would stop a document from rendering over its accessibility.

**`Quill.yaml` parses strictly and reports every problem in one pass**, so a typo never loads as a silently ignored key. `quill.requires` is read first and reported alone, so a newer Quillmark's keys are not reported as typos by an older one.

## Writing a document

**A verb that needs a schema lives on the writer or reader; `Document` is quill-free.** The typed writer (`quill.writer(doc)`) and reader own every schema-shaped verb. `Document` holds the opaque store and the content writes, which navigate by address and never consult a schema. The verb names the write path: **store** is verbatim, **set** is typed, **apply**, **revise** and **replace** are the content write.

**Every write holds every invariant but the schema.** An unchecked write still holds names, bounds, card ids and canonical content. On the typed path an undeclared name is a typo, refused at the write, never a fallback to the opaque store.

**No revision counter.** Equality answers whether this is the content last compiled; a hash answers whether the value a writer read is still the value stored. A counter answers only whether something was written, so it re-applies on a reload that changed nothing and invalidates every base a hash would keep.

**Writers and card cursors are ephemeral.** They hold an address, never a cache, and read through the document on every call. A content write names its card by `$id`, because an index shifts onto another card when cards move, and an equal value there passes the base.

**The values form is a projection, never a storage format.** A read never coerces a scalar; canonicalizing is what a write does. A content value written through `set` is a cold import and loses its anchors; the revise is the write that keeps them.

## The content model

**One value per field, and nothing derived is stored.** A content value is flat text, block and container records by id, range marks, host-minted anchors, and islands holding nested content in slots. Nesting lives only in the blocks' container paths, so a container can never dangle or cycle. An item's number is computed on read.

**One spelling per value.** A write decodes only a value in canonical form and refuses any other; `normalize` is the producer's verb. Decoding then encoding is therefore the identity, and equal values serialize to equal bytes. A stored value is read leniently: one not in canonical form is rewritten to it with a `content::noncanonical_read` warning, so the host stores the row again.

**Every id Quillmark mints is a function of the values in play.** None comes from an RNG, clock or process state, so the same markdown imports to the same bytes and hashes hold.

- An import mints structural ids positionally, past the highest number the value holds.
- A write mints a new block's id from a digest of the value it lands on, so a retired block id returns only on a write against that same value, and a writer naming one is refused rather than landing on a stranger. Rejected: a stored high-water mark, state the value would carry for no reader but the minter.
- A card that spells no valid `$id` mints one from a hash of the first index no card holds.
- Anchor ids are the host's: no content determines what a comment refers to, so Quillmark mints none.

**The change laws hold for every change.** Apply then invert is the identity; compose agrees with applying in sequence; position mapping agrees with apply; parse and serialize round trip; every change yields a valid value or fails and changes nothing; repair is total and deterministic. They are property tests, and a change that breaks one is wrong.

**A `Change` is session-scoped.** Its JSON carries no version, so a host persists values, never changes.

**`apply` never grows a mark.** A splice states the marks its inserted text carries, so every inverse is exact. Growth at a mark's edge is a producer's default, not apply's.

**A split keeps the left half's id; a join keeps the left block's.** Neither rewrites a record a concurrent `setBlock` could be aimed at, and collab's Y form keeps the same boundary.

**Markdown is a projection, not storage.** A round trip loses anchors, ids, constructs markdown cannot spell and the author's exact spelling. A write that must keep them goes through `apply`, or through the annotated read, whose tags a revise reads back. A revise or replace that would remove an `x-` island refuses, since markdown cannot spell one and its writer may never have seen it.

**A newline is a line break, in Markdown as in the plain codec.** A newline inside a paragraph imports as a hard break and a blank line as a paragraph break, and export writes a hard break as a newline, so the two codecs read one keystroke one way. CommonMark's soft break is not kept: a field's YAML block and a form's text box hold the lines their writer typed, and folding them printed an address as one line. A heading, a table cell and a footnote's note are one line each and read a newline as a space. Rejected: the rule for fields alone, which left the body the one place Enter meant a space.

**No silent drop.** For every construct, quill and backend, the outcome is honored, declined with a signal, or refused at the write. A surface that cannot spell a construct carries it opaquely or refuses; it never re-encodes without it. The parity corpus (`crates/fixtures/resources/parity/`) pins every construct on every surface.

## Storage

**A stored row is a versioned envelope, frozen per tag.** Every blob carries a `schema` tag, and each tag has its own DTO tree, never changed once shipped. Readers dispatch on the tag and refuse an unknown one rather than guess. The tag is a hand-set constant tied to the last wire change, not the crate version.

**Serialization is byte-deterministic within a tag**, which content hashing depends on. Outside content, order is insertion order, because payload item order is source order and matters. Inside content, every object's keys are sorted, because order there carries no meaning.

**A stored row stays readable within a major release.** Within a major:

- the lenient read of a non-canonical value stays, however long ago it was stored;
- a bound a value is held to only loosens, since a lowered bound makes a stored row unreadable;
- a legacy tag's reader stays; a tag retires only in a major release, because Quillmark cannot count its hosts' rows;
- a change to the content encoding stays readable permanently, because content inside an untagged payload value is reachable by no migration hop.

**Adding a built-in vocabulary member is a storage-version event.** Block kinds, container kinds, mark types and island types are closed sets, and a name outside them refuses the row. Projecting an unknown kind as a paragraph would move canonical bytes on a read with no edit. A name under the reserved `x-` prefix is not a member: it rides as written, so a construct that must open in an older build ships under an `x-` name first.

**Closed envelopes, opaque carriers.** An unknown key on a content object refuses at a write's decode, so a newer build's key fails loudly in an older reader. Island props and `x-` attrs are opaque and round-trip untouched.

**A hash holds within one storage tag.** `Document::hash` digests the tag, so a storage bump moves it for an equal document.

## Comments

**The thread lives outside the document.** A comment is a thread the host stores, joined to the document by one anchor whose id is the thread's id. No body, author or status enters content, so a reply never moves the document's bytes.

**A write never deletes a thread.** Every write's receipt names each anchor it dropped. A dropped anchor is placed again by `reattach` or reported orphaned, and the thread stays in the host's store either way.

**Re-attachment is exact or nothing.** `reattach` places an anchor only where its quote and context are found exactly once, and is at least as strict as revise's own floor for moving one, or it would undo that refusal. Rejected: nearest match, partial quote and edit distance, each of which attaches a comment to text it never discussed.

**A transient anchor is never stored.** An anchor whose id opens with `~` moves with its text and rides the wire and every copy, but storage, the annotated read and every hash leave it out, so placing one never stales a writer's base. Equality still sees it, because a write lands only a value that differs. Rejected: a flag on the anchor, a stored key older readers refuse for a value never stored; refusing a stored `~` id on load, which tightens a bound on stored rows.

**A host keeps the stored document, never the markdown.** Plain markdown drops every anchor. Recovering from markdown by re-attaching every stored selector is a search that can orphan, so it is the fallback, not the design.

## The bindings

**Every binding verb is identical to core, or names its one forced difference**: an FFI constraint, a language idiom, or a surface the binding omits on purpose. WASM is the reference surface and new contract work lands there first; Python mirrors it within storage, render and the writer.

**A binding speaks its host's units.** Storage and core count Unicode scalar values. WASM converts to UTF-16 code units and Python speaks code points, at every surface that takes or answers an offset.

**WASM delivery follows the function kind.** A synchronous verb throws, a promise-returning verb rejects, and nothing does both. `init` returns its memoized promise by identity rather than being `async`, so its one guard rejects explicitly: a synchronous throw there escapes a caller's `.catch`.

**The `init` gate is the only way into WASM.** The core surface is reachable only from what `init` resolves to, so a call before instantiation is unwritable rather than a convention. The package builds with `--target web`. Rejected: `--target bundler`, whose fix rewrites the wasm import to a top-level await that lands on every consumer's static module graph.

**Exactly one copy of the WASM package per process.** Two copies are two linear memories and two `Document` classes, and every seam taking a handle refuses one from the other copy.

**An options object refuses a key it does not read.** The underlying reader ignores unknown keys, so a misspelled option would read as absent and the call would go ahead on its default.

## Preview

**Two verbs, one session type.** `render` is a stateless one-shot; `open` returns a `LiveSession` that owns preview. `update` is transactional: on failure the last good compile and its warnings stay live. It reports dirty pages, not new handles. Rejected: a frozen snapshot type and a change counter, which protect a cross-edit reader the single preview consumer does not have.

**A paint is a complete raster, and the painter owns the canvas.** The consumer never composites; regions are an overlay sidecar, never a compositing input. Rejected: a per-session raster cache, whose per-page buffers bring back the unbounded memory the viewport-bounded design exists to avoid; canvas as an `OutputFormat`, which would leak `wasm_bindgen` into core.

**A region comes only from what the plate declares**: ink generated through the helper, a `qm.region` claim, or a form widget. Rejected: inferring a region from plate syntax, whose lexical extent is not provenance and moves when a plate is rewritten into an equivalent form.

**A claim is a fallback, and ink belongs to exactly one field.** The innermost producer wins, so wrapping only adds regions and never moves one. A claim never takes a stroke, which would steal clicks from the content beneath, or out-of-flow ink opened outside it, such as page chrome, a footnote or a float.

**Hit tolerance is the caller's number.** A pointer's imprecision is a screen quantity, so only the consumer, who knows the display scale, can convert it to points. Quillmark holds no default.

## The Typst backend

**A declared type means the same thing at every depth.** Every type lowers to its native Typst value unless it has a canonical rendering, and only content does; a date lowers to a native `datetime`, because every rendering of a date is the plate's typographic decision. The lowering dispatches on the schema node beside each value, never on a table of field names, which is what makes it depth-invariant.

**Ink is addressed, not carried on the value.** A plate computes with `qm.data.x` and prints with `qm.ink(..)`. Ink born at its generated entry survives being passed through a binding, a parameter or a vendored package, and gives each card's field its own identity, which is what makes regions work.

**The helper imports as a module and in no other form.** Any other import of it is refused, so a helper export never shadows a plate's own names.

**No backend resolves a content image's url.** A document is quill-free but for `$quill`, which is a selector, so a path into one quill's file tree would bind documents to something nothing versions. An image declines with a signal instead.

**Escaping is total and one character to its bytes.** Document text can inject no markup, and the per-segment source map that click-to-caret reads stays exact. Smart quotes pass through, so the quill decides them with a set rule.

**One address grammar, two implementations.** The plate helper and core's `QuillConfig::field_at` each implement the schema address grammar, and a test holds both to one table, so one address binds on either backend. Rust code that resolves an address calls `field_at`: acroform binds by it, and a Typst widget's inherited tooltip reads its description through it, so a third walk would be a third implementation the table does not hold.
