# Quillmark: Python bindings

Python bindings for Quillmark, a schema-driven document framework.

Maintained by [borb-sh](https://github.com/borb-sh).

## Installation

```bash
pip install quillmark
```

The package is typed: it ships `py.typed` and stubs for the whole surface.

## Quick Start

```python
from quillmark import Engine, Quill, OutputFormat

engine = Engine()                          # backend registry + render dispatcher
quill = Quill.from_path("path/to/quill")   # a document format, as declarative data

markdown = """~~~
$quill: my_quill
$kind: main
title: Hello World
~~~

# Hello
"""

doc = quill.parse(markdown)
result = engine.render(quill, doc, OutputFormat.PDF)
result.artifacts[0].save("output.pdf")
```

## API surface

Field I/O flows through `quill.writer(doc)` and `quill.reader(doc)`, the
schema-bound writer and reader. `Document` carries the quill-free surface:
parse, storage, structure, `$ext` / `$seed`, and `remove_field`. Names follow
`snake_case`, and the shared model (the `Document` / `Card` shapes,
`Diagnostic`s, the storage DTO) is identical to [`@quillmark/wasm`](../wasm)'s.

### `Engine`

```python
engine = Engine()
engine.registered_backends()              # ['typst', 'acroform'] (order not guaranteed)
engine.render(quill, doc, OutputFormat.PDF)      # ppi=, pages=, regions=, title= optional
                                          # today=datetime.date, default the local date
engine.supported_formats(quill)           # [OutputFormat.PDF, ...] (raises if backend unregistered)
```

### `Quill`

```python
quill = Quill.from_path("path/to/quill")  # pure config load: no backend resolved here

quill.backend_id            # "typst" (declared backend)
quill.schema                # structured dict of the quill's document schema
quill.authoring_schema      # JSON Schema of the authoring document an LLM writes
quill.metadata              # `name`, `version`, `backend`, `author`, `description`, then `license`, `homepage` when declared
quill.quill_ref             # "name@version"

doc     = quill.parse(markdown)           # parse + conform to the quill's schema, the primary ingestion path
diags   = quill.conform(doc)              # the same walk in place on a transported document ([] = at rest)
diags   = quill.validate(doc)             # list of diagnostic dicts ([] = valid): validation::*,
                                          # parse::dropped_construct, backend::declined_construct
empty   = quill.empty_document()          # the empty Document: $quill + $kind: main, nothing committed
doc     = quill.from_authoring(reply)     # an LLM's JSON document, conformed like parse
reply   = quill.to_authoring(doc)         # the document as JSON: rich text as Markdown strings
seed    = quill.seed_document()           # starter Document: one card per kind, each its kind's seed
example = quill.example_document()        # the seed with every declared example committed; None without one
main    = quill.seed_main()               # just the $kind: main card (dict, like doc.main)
card    = quill.seed_card("note")         # one starter composable card (dict), None if kind undeclared

writer  = quill.writer(doc)               # schema-bound typed writer
reader  = quill.reader(doc)               # schema-bound interpreted reader
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
                                          # set, set_all and add_card return the parse::dropped_construct
                                          # warnings their markdown raised: [Diagnostic, ...]
                                          # revise_body and revise_field return their receipt:
                                          # {"delta", "warnings": [Diagnostic, ...], "dropped_anchors": [{"id", "line"}]}
w.revise_document(markdown)               # whole-document write: cards align by $kind and text, anchors rebase,
                                          # then conform; returns {"dropped_anchors": [{"id", "path", "line"}],
                                          # "warnings": [Diagnostic, ...], "from": [int | None]}, clears doc.warnings
w.remove_card(0)
w.set("author", "Issa", card=0)           # every verb takes card=: None is main, an int the composable card
```

### `Reader`: `quill.reader(doc)`

The read twin of `Writer`. `get` reads each field in the values form: every
content leaf in its type tree as its codec's text (a richtext leaf to markdown, a
text leaf to its literal text), everything else as stored, never coerced.
`get_content` is the same read at the other end of the codec, handing back the
field's `Content` as a dict whichever lane stored it.

```python
v = quill.reader(doc)
v.get("bio")                              # richtext → markdown str; scalar → as stored; absent or present-null → None
                                          # undeclared name raises UnknownField; undecodable content raises FieldDecode
v.get_content("bio")                      # the `Content` dict {text, lines, marks, islands}; absent → None
                                          # a type that is not a content leaf raises FieldNotContent
v.body_markdown()                         # the main body markdown (quill-free body read)
v.get("author", card=0)                   # a card field, interpreted by its $kind schema
v.body_markdown(card=0)
doc.card(0)["kind"]                       # the composable card's $kind
```

### `RenderResult` / `Artifact`

```python
result.artifacts            # [Artifact, ...]
result.warnings             # [Diagnostic, ...]: quill.validate's but backend::declined_construct,
                            # then the compile's, backend::declined_construct among them;
                            # the load's stay on doc.warnings
result.format               # OutputFormat

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

Document.storage_version_of(blob)                # raw tag, even one this build does not read; None when not a DTO
Document.current_storage_version()               # newest tag this build writes; a row holding no vocabulary the previous tag lacks keeps the previous tag

Document.markdown_rules()                        # Quillmark Markdown rules (static text)
Document.quill_ref_hint()                        # $quill reference grammar (static text)

doc.clone()
doc.equals(other)
doc.card_count
doc.main; doc.cards; doc.body; doc.warnings      # total-read snapshots (dicts); body is a content dict
doc.card(0)                                      # one card, same dict shape as main (out of range raises)
doc.seed_overlay("note")                         # one $seed[kind] overlay, or None
doc.set_quill_ref("other@1.0")

# Structure (quill-free, a card kind is a name, not a schema fact):
doc.insert_card({"kind": "note", "body": "..."}, at=None)   # at appends/inserts
doc.insert_card({"kind": "note",                            # fields → payload items
                 "payload_items": [{"type": "field", "key": "x", "value": 1}]})
doc.remove_card(0)                               # returns the Card dict, or None
doc.move_card(2, 0)
doc.remove_field("title")                        # remove has no lane; card=i targets a composable card

# Out-of-band consumer state (never rendered):
doc.store_ext({"agent": {"pinned": True}})       # whole $ext map; card=i for a composable card
doc.store_ext({**doc.main["ext"], "agent": {"n": 1}})   # one namespace: merge, then store
doc.remove_ext()                                 # every namespace at once
doc.store_seed_overlay("note", {"tag": "T"})     # per-kind $seed overlay; new cards spawn with it
doc.remove_seed_overlay("note")

# Comment anchors (the thread lives in the host's store, keyed by the anchor id):
doc.anchor_contexts()                            # [{"id", "path", "quote", "prefix", "suffix"}, ...]
doc.anchor_context("c1")                         # one selector, or None
doc.reattach(selectors, deltas=None)             # re-add dropped anchors where the quote is found once:
                                                 # {"reattached": [...], "orphaned": [...]}
```

Setting a field's value is the writer's job (`quill.writer(doc).set(...)`): a
field write needs the schema, and `Document` is quill-free. Reading a field's
interpreted value is the reader's (`quill.reader(doc).get(...)`).

## Schema model

A field declares no `required:` key and an unanswered field draws no diagnostic; [`default:`](https://quillmark.dev/quills/quill-yaml-reference/#default) is what an unanswered field renders.

## Error contract

A Quillmark refusal raises `QuillmarkError`, carrying a non-empty `.diagnostics`
list of `Diagnostic` objects, the first an error; [Error Handling](https://quillmark.dev/integration/error-handling/) has the shape, the code namespaces and the warnings that follow. An argument the binding cannot read at all — a
non-finite float, a value with no JSON form, a malformed `path` sequence —
raises `ValueError` before core is called, described by no diagnostic.

```python
try:
    Document.from_markdown(bad_md)
except QuillmarkError as exc:
    for d in exc.diagnostics:
        print(d.severity, d.code, d.message, d.path)
        print(str(d))   # canonical pretty-printed text (matches CLI / WASM)
```

Card and page indices count from the front, so a negative one addresses nothing
rather than the last card or page: it takes the same answer as an index past the
end — `edit::index_out_of_range` where the verb raises, `None` where
`remove_card` answers absence.

Each refusal anchors at a rooted document path: `main.<field>` for a main field,
`cards.<kind>[<i>].<field>` for a card field, `cards[<i>]` for a structural
out-of-range op.

## Changelog

See the [changelog](https://github.com/borb-sh/quillmark/blob/main/CHANGELOG.md)
and the [GitHub Releases](https://github.com/borb-sh/quillmark/releases) page for
release notes and version history.

## Development

```bash
uv venv
source .venv/bin/activate
uv pip install maturin pytest
maturin develop
pytest
```

`maturin develop` builds the extension in the debug profile. Stay in the
activated venv: `uv sync`, `uv run`, and `pip install -e` each sync the project
through maturin's PEP 517 backend, a release build that replaces the debug
`.so`.

`python/quillmark/_quillmark.pyi` is hand-written and must track `src/`. No CI
job gates it, so after changing the surface run the stub against the built
module yourself:

```bash
uv pip install mypy
python -m mypy.stubtest --ignore-disjoint-bases quillmark
```

## License

Apache-2.0
