"""Marshalling tests for quill.validate and the seed verbs.

The diagnostic set and the seed layering rules are core's: `validate`'s
decision matrix lives in `crates/quillmark/tests/validate_test.rs`, the seed
semantics in `core/src/quill/seed/tests.rs`. What is pyo3's is the crossing:
each verb returns the right Python shape, and an error keeps its `code`.
"""

import json
import pytest

from quillmark import Document, Quill

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

QUILL_YAML_CONTENT = """quill:
  name: py_validate_smoke
  version: "1.0"
  backend: typst
  description: Python validate smoke test

main:
  fields:
    title:
      type: string
    count:
      type: integer
    byline:
      type: string
      example: FIRST LAST

card_kinds:
  note:
    fields:
      body:
        type: string
        default: TBD
      tag:
        type: string
        example: NOTE TAG
"""


def make_quill(tmp_path, yaml_content=QUILL_YAML_CONTENT):
    """Write a minimal quill directory and load it (engine-free)."""
    quill_dir = tmp_path / "quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(yaml_content)
    return Quill.from_path(quill_dir)


def _md(*lines):
    fields = "".join(f"{line}\n" for line in lines)
    return f"~~~card-yaml\n$quill: py_validate_smoke\n$kind: main\n{fields}~~~\n"


# ---------------------------------------------------------------------------
# Tests: validate()
# ---------------------------------------------------------------------------

def test_validate_returns_empty_list_for_clean_document(tmp_path):
    """A complete, well-formed document produces no diagnostics."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('title: "Hello"', "count: 1", 'byline: "A B"'))

    diags = quill.validate(doc)

    assert isinstance(diags, list)
    assert diags == []


def test_validate_forwards_type_mismatch(tmp_path):
    """A bad type surfaces with its canonical code, path, and hint."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('title: "Hello"', 'count: "not-a-number"'))

    diags = quill.validate(doc)
    mismatch = next(
        (d for d in diags if d.get("code") == "validation::type_mismatch"), None
    )
    assert mismatch is not None, f"expected type_mismatch; got: {diags}"
    assert mismatch["path"] == "main.count"
    assert mismatch.get("hint")


def test_validate_json_serializable(tmp_path):
    """The diagnostics list is fully JSON-serializable via json.dumps."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('count: "nope"'))

    diags = quill.validate(doc)
    dumped = json.dumps(diags)
    assert isinstance(dumped, str)
    assert len(json.loads(dumped)) == len(diags)


# ---------------------------------------------------------------------------
# Tests: seed_document (the Document-path starter; replaces blank_main/blank_card)
# ---------------------------------------------------------------------------

def test_seed_document_commits_examples(tmp_path):
    """seed_document returns a Document committing example values and leaving
    default-only fields absent (interpolated at render, not persisted)."""
    quill = make_quill(tmp_path)

    doc = quill.seed_document()
    md = doc.to_markdown()

    assert "FIRST LAST" in md, "byline example must be committed"
    assert "TBD" not in md, "note body default must not be persisted"


def test_seed_main_and_card(tmp_path):
    """seed_main / seed_card return per-card seeds (the Document.main / cards
    dict shape), each committing its fields' example; seed_card is None for an
    unknown kind."""
    quill = make_quill(tmp_path)

    main = quill.seed_main()
    assert main["kind"] == "main"
    assert "FIRST LAST" in json.dumps(main), "byline example must be committed"

    note = quill.seed_card("note")
    assert note["kind"] == "note"
    assert "NOTE TAG" in json.dumps(note), "tag example must be committed"

    assert quill.seed_card("missing") is None, "unknown kind must be None"


def test_document_seed_and_store_seed_overlay_round_trip(tmp_path):
    """main['seed'][kind] reads what store_seed_overlay wrote; the overlay
    feeds straight back into seed_card as a plain dict; remove_seed_overlay
    clears it."""

    def seed_of(document, kind):
        # The per-kind overlay lives on the main card's `$seed` map; there is
        # no `Document.seed` convenience.
        return (document.main["seed"] or {}).get(kind)

    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md())  # empty main card

    assert seed_of(doc, "note") is None
    doc.store_seed_overlay("note", {"tag": "WRITTEN"})
    assert seed_of(doc, "note")["tag"] == "WRITTEN"

    card = quill.seed_card("note", seed_of(doc, "note"))
    assert "WRITTEN" in json.dumps(card)

    doc.remove_seed_overlay("note")
    assert seed_of(doc, "note") is None


# ---------------------------------------------------------------------------
# Tests: the bound door (parse / conform)
# ---------------------------------------------------------------------------

BOUND_QUILL_YAML = """quill:
  name: py_bound_smoke
  version: "1.0"
  backend: typst
  description: Python bound-door smoke test

main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
"""


def _fields_of(card):
    """The card's user fields as a name → value map."""
    return {
        item["key"]: item["value"]
        for item in card["payload_items"]
        if item["type"] == "field"
    }


def _bound_md(*lines):
    fields = "".join(f"{line}\n" for line in lines)
    return f"~~~card-yaml\n$quill: py_bound_smoke\n$kind: main\n{fields}~~~\n\nBody.\n"


def test_parse_lands_both_codecs_at_rest(tmp_path):
    """The bound door's crossing: a richtext field arrives as the canonical
    content dict, a plaintext one as its literal string, and warnings ride the
    same `doc.warnings` carrier a `from_markdown` parse uses."""
    quill = make_quill(tmp_path, BOUND_QUILL_YAML)
    doc = quill.parse(_bound_md("subject: Q3 **results**", "note: 'a *literal* line'"))

    assert doc.warnings == []
    fields = _fields_of(doc.main)
    assert isinstance(fields["subject"], dict), "richtext rests as the corpus"
    assert fields["note"] == "a *literal* line", "plaintext rests as the literal"


def test_conform_converges_a_transported_document(tmp_path):
    """conform is the same walk on a document that arrived any other way: it
    returns a list, converges to the bound door's bytes, and is idempotent."""
    quill = make_quill(tmp_path, BOUND_QUILL_YAML)
    md = _bound_md("subject: Q3 **results**", "note: 'a *literal* line'")
    doc = Document.from_markdown(md)

    diags = quill.conform(doc)
    assert isinstance(diags, list) and diags == []
    assert doc.to_stored() == quill.parse(md).to_stored()
    assert quill.conform(doc) == []


def test_conform_reports_a_non_conforming_value_and_leaves_it_authored(tmp_path):
    """A value the strict write refuses rests as authored, carrying a
    `conform::*` warning on `doc.warnings` rather than raising."""
    quill = make_quill(tmp_path, BOUND_QUILL_YAML)
    doc = quill.parse(_bound_md("subject: 42"))

    assert "conform::field_decode" in [d.code for d in doc.warnings]
    assert _fields_of(doc.main)["subject"] == 42, "the value stays authored"


def test_the_wrong_quill_raises_before_any_mutation(tmp_path):
    """Nothing conforms under the wrong schema, through either verb."""
    from quillmark import QuillmarkError

    quill = make_quill(tmp_path, BOUND_QUILL_YAML)
    md = "~~~card-yaml\n$quill: other_quill\n$kind: main\nsubject: hi\n~~~\n\nBody.\n"

    with pytest.raises(QuillmarkError) as excinfo:
        quill.parse(md)
    assert excinfo.value.diagnostics[0].code == "quill::name_mismatch"

    doc = Document.from_markdown(md)
    before = doc.to_stored()
    with pytest.raises(QuillmarkError):
        quill.conform(doc)
    assert doc.to_stored() == before
