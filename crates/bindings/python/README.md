# Quillmark: Python bindings

Python bindings for Quillmark, a schema-driven document engine.

Maintained by [TTQ](https://tonguetoquill.com).

## Installation

```bash
pip install quillmark
```

The package is typed: it ships `py.typed` and stubs for the whole surface, so
mypy, Pyright, and IDE completion see real signatures rather than `Any`.

## Quick Start

```python
from quillmark import Quillmark, Quill, Document, OutputFormat

engine = Quillmark()                       # backend registry + render dispatcher
quill = Quill.from_path("path/to/quill")   # portable, declarative config data

markdown = """~~~
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
"""

parsed = Document.from_markdown(markdown)
result = engine.render(quill, parsed, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")
```

## API surface

Field I/O flows through `quill.writer(doc)` and `quill.reader(doc)`, the
schema-bound write/read front doors. `Document` carries the quill-free surface:
parse, storage, structure, `$ext` / `$seed`, and `remove_field`. The opaque field
store and the anchor-preserving content lane are WASM-only by scope, as are the
render session and canvas preview; Python renders in one shot via
`engine.render`. Names follow `snake_case`, and the shared model (the `Document`
/ `Card` shapes, `Diagnostic`s, the storage DTO) is identical to
[`@quillmark/wasm`](../wasm)'s.

A `Quill` is portable, declarative config data, and `quill.metadata` a pure,
infallible snapshot of the `quill:` section. The engine resolves the declared
backend, so only the format probe (`supported_formats`) and `render` raise
`engine::backend_not_found`.

### `Quillmark`

```python
engine = Quillmark()
engine.registered_backends()              # ['typst', 'pdfform'] (order not guaranteed)
engine.render(quill, parsed, OutputFormat.PDF)   # ppi=, pages=, producer= optional
engine.supported_formats(quill)           # [OutputFormat.PDF, ...] (raises if backend unregistered)
```

### `Quill`

```python
quill = Quill.from_path("path/to/quill")  # pure config load: no backend resolved here

quill.backend_id            # "typst" (declared backend)
quill.blueprint             # auto-generated annotated Markdown blueprint
quill.schema                # structured dict of the quill's document schema
quill.metadata              # pure config snapshot of the quill: section (never raises)
quill.quill_ref             # "name@version"

doc     = quill.parse(markdown)           # the bound door: parse + conform, the primary ingestion path
diags   = quill.conform(doc)              # the same walk in place on a transported document ([] = at rest)
diags   = quill.validate(parsed)          # list of validation::* diagnostic dicts ([] = valid)
seed    = quill.seed_document()           # starter Document seeded from `example:` values
main    = quill.seed_main()               # just the $kind: main card (dict, like doc.main)
card    = quill.seed_card("note")         # one starter composable card (dict), None if kind undeclared

writer  = quill.writer(doc)               # schema-bound typed write front door
reader  = quill.reader(doc)                 # schema-bound interpreted read front door
```

### `Writer`: `quill.writer(doc)`

Resolves each field's type from the bound quill, so a name the schema does not
declare is a typo (`UnknownField`), not a fallback. Holds both handles by
reference and owns neither: bind, write, discard.

```python
w = quill.writer(doc)
w.set("title", "On Taro")                 # typed-commit one field (mismatch raises now)
w.set_all({"title": "T", "author": "A"})  # atomic batch; one diagnostic per bad field
w.revise_body("A **taro** essay.")        # body write (edit semantics; a body has no field schema)
w.revise_field("bio", "make it **bold**") # typed *and* anchor-preserving content write (codec by declared type)
w.add_card("quotes", {"author": "Basho"}, "…", at=None)  # make + typed commit + insert (at appends/inserts)
w.remove_card(0)
w.card(0).set("author", "Issa")           # a CardWriter: .index, .kind, .set, .set_all, .set_values, .revise_body, .revise_field
w.set_values({"fields": {"title": "Hi"}})  # the values form: a present key replaces its axis, an absent one is untouched
```

### `Reader`: `quill.reader(doc)`

The read twin of `Writer`. `get` reads each field in the values form: every
content leaf in its type tree as its codec's text (a richtext leaf to markdown, a
plaintext leaf to its literal text), everything else as stored, never coerced.
`get_content` is the same read at the other end of the codec, handing back the
field's `Content` as a dict whichever lane stored it. `values` is the whole
document in the same form, and `writer.set_values` writes it back.

```python
v = quill.reader(doc)
v.get("bio")                              # richtext → markdown str; scalar → as stored; absent or present-null → None
                                          # undeclared name raises UnknownField; undecodable content raises FieldDecode
v.get_content("bio")                      # the `Content` dict {text, lines, marks, islands}; absent → None
                                          # a type that is not a content leaf raises FieldNotContent
v.body_markdown()                         # the main body markdown (quill-free body read)
v.values()                                # {"fields", "body", "cards", "ext"}; never raises; set_values(v.values()) is a no-op
v.card(0).kind                            # the composable card's $kind
v.card(0).get("author")                   # a card field, interpreted by its $kind schema
v.card(0).values()                        # {"kind", "fields", "body", "ext"} for one card
v.card(0).body_markdown()
```

### `RenderResult` / `Artifact`

```python
result.artifacts            # [Artifact, ...]
result.warnings             # [Diagnostic, ...]
result.format               # OutputFormat
result.render_time_ms       # float

artifact.format             # OutputFormat
artifact.bytes              # bytes
artifact.mime_type          # 'application/pdf', 'image/svg+xml', ...
artifact.save("out.pdf")
```

### `Document`

```python
doc = Document("my_quill")                       # blank canvas: $quill only, no fields, no cards
doc = Document.from_markdown(markdown)
emitted = doc.to_markdown()

stored   = doc.to_stored()
restored = Document.from_stored(stored)
maybe    = Document.try_from_stored(blob)          # None when not a DTO

Document.storage_version_of(blob)                # raw tag (incl. unknown futures)
Document.current_storage_version()               # what this build writes

Document.format_rules()                          # card-yaml authoring rules (static text)
Document.quill_ref_hint()                        # $quill reference grammar (static text)
Document.blueprint_instruction("taro")           # LLM/MCP blueprint header for a quill

doc.clone()
doc.equals(other)
doc.card_count
doc.main; doc.cards; doc.body; doc.warnings      # total-read snapshots (dicts); body is a content dict
doc.card(0)                                      # one card, same dict shape as main (out of range raises)
doc.seed_overlay("note")                         # one $seed[kind] overlay, or None
doc.set_quill_ref("other@1.0")

# Structure (quill-free, a card kind is a name, not a schema fact):
doc.insert_card(Document.make_card("note", {"x": 1}, "..."), at=None)  # at appends/inserts
doc.remove_card(0)                               # returns the Card dict, or None
doc.move_card(2, 0); doc.set_card_kind(0, "summary")
doc.remove_field("title")                        # remove has no lane; card=i targets a composable card

# Out-of-band consumer state (never rendered):
doc.store_ext({"agent": {"pinned": True}})       # whole $ext map; card=i for a composable card
doc.store_ext_namespace("agent", {"n": 1})       # one slot, siblings preserved; card=i too
doc.remove_ext_namespace("agent"); doc.remove_ext()
doc.store_seed_overlay("note", {"tag": "T"})     # per-kind $seed overlay; new cards spawn with it
doc.remove_seed_overlay("note")
```

Setting a field's value is the writer's job (`quill.writer(doc).set(...)`): a
field write needs the schema, and `Document` is quill-free. Reading a field's
interpreted value is the reader's (`quill.reader(doc).get(...)`).

## Schema model

A field carries two independent axes, and no `required:` one on `FieldSchema`.

**Value** — what the cell holds. With a `default:`, the blueprint renders that
value under a type-only `# <type>` annotation and the render path uses it when
the document omits the field. Without one, an `example` takes the cell as a
suggested value, and an absent field blank-fills.

**Obligation** — whether a human must author the field, read off `default:`'s
absence: a defaulted field asks nobody, a defaultless one asks. An obliged
field carries the `!must_fill` marker in the blueprint, and validation emits the
non-fatal `validation::must_fill` warning while the document leaves it
unauthored — from either of two triggers, named by the diagnostic's `trigger`
arg: `marker` for a marker the document still carries, `unauthored` for a cell
the schema obliges and the document never filled. Authoring the field's blank
discharges the obligation; clearing the key does not.

Neither axis gates render. Partial documents are accepted, and
`engine.render(quill, doc)` raises only for malformed input.

## Error contract

Every failure raises `QuillmarkError`, carrying a non-empty `.diagnostics` list
of `Diagnostic` objects.

```python
try:
    Document.from_markdown(bad_md)
except QuillmarkError as exc:
    for d in exc.diagnostics:
        print(d.severity, d.code, d.message, d.path)
        print(str(d))   # canonical pretty-printed text (matches CLI / WASM)
```

Mutator failures (invalid field names, kind names, out-of-range indices) carry a
namespaced `edit::*` `code` on `diagnostics[0]`: `edit::invalid_field_name`,
`edit::unknown_field`, `edit::index_out_of_range`, `edit::field_coercion_failed`,
…. Route on `diagnostics[0].code`, never on message text.

Each one anchors at a rooted document path: `path` is `main.<field>` for a main
field, `cards.<kind>[<i>].<field>` for a card field, and `cards[<i>]` for a
structural out-of-range op, whichever verb refused. A card `add_card` rejects
before placing it has no slot yet, so its bundle keys the bare `$kind` / `$body`
or field name.

## Changelog

See the [changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/borb-sh/quillmark/releases) page for
release notes and version history.

## Development

```bash
uv venv
uv pip install -e ".[dev]"
uv run pytest
```

`python/quillmark/_quillmark.pyi` is hand-written and must track `src/`. No CI
job gates it, so after changing the surface run the stub against the built
module yourself:

```bash
uv pip install mypy
uv run python -m mypy.stubtest --ignore-disjoint-bases quillmark
```

## License

Apache-2.0
