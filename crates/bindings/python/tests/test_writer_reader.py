"""Tests for the typed field surface: `quill.writer(doc)` writes,
`quill.reader(doc)` reads, both against the quill's schema. The quill-free
mutator surface is `test_document.py`."""

import pytest
from quillmark import Document, Quill, QuillmarkError

from conftest import (
    field,
    has_field,
    raises_edit_code,
    richtext_form_quill,
    taro_quill,
)


def test_writer_set_rejects_unknown_field():
    """An undeclared name is a typo on the typed path: it raises, nothing lands."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    with raises_edit_code("edit::unknown_field"):
        quill.writer(doc).set("stray", "x")
    assert not has_field(doc.main, "stray")


def test_writer_set_all_reports_every_unknown_field():
    """set_all is all-or-nothing and reports one diagnostic per undeclared name:
    externally-sourced keys surface every violation at once."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    with raises_edit_code("edit::unknown_field") as exc_info:
        quill.writer(doc).set_all({"title": "ok", "stray1": "x", "stray2": "y"})
    paths = [d.path for d in exc_info.value.diagnostics]
    assert "main.stray1" in paths and "main.stray2" in paths
    # All-or-nothing: even the valid `title` did not land.
    assert not has_field(doc.main, "title")


def test_every_mutator_verb_anchors_its_diagnostic_at_one_doc_path():
    """One rooted anchor per refusal, whichever verb refused, so a consumer
    routes on `path` without knowing which one did. `add_card` is the exception
    it names: the card is committed before it joins the document, so it has no
    slot to anchor in and its bundle keys the bare `$kind`."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    writer = quill.writer(doc)
    writer.add_card("quotes", {"author": "Basho"})

    def path_of(call):
        with pytest.raises(QuillmarkError) as excinfo:
            call()
        return excinfo.value.diagnostics[0].path

    cases = [
        ("set", lambda: writer.set("stray", "x"), "main.stray"),
        ("set_all", lambda: writer.set_all({"stray": "x"}), "main.stray"),
        ("set card=", lambda: writer.set("stray", "x", card=0), "cards.quotes[0].stray"),
        ("add_card", lambda: writer.add_card("quotes", {}, at=99), "$kind"),
        ("move_card", lambda: doc.move_card(9, 0), "cards[9]"),
    ]
    for verb, call, expected in cases:
        assert path_of(call) == expected, verb


def test_writer_add_card_transactional():
    """add_card fuses make + typed commit + insert; a typo leaves the doc untouched."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"}, "A quote body.")
    assert len(doc.cards) == 1
    assert field(doc.cards[0], "author") == "Basho"
    with raises_edit_code("edit::unknown_field"):
        ed.add_card("quotes", {"stray": "x"})
    assert len(doc.cards) == 1  # nothing joined the document


def test_writer_add_card_positioned():
    """add_card(..., at=i) is one atomic positioned typed insert."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "First"})
    ed.add_card("quotes", {"author": "Second"}, at=0)  # insert at the front
    assert field(doc.cards[0], "author") == "Second"
    assert field(doc.cards[1], "author") == "First"
    with raises_edit_code("edit::index_out_of_range"):
        ed.add_card("quotes", {"author": "x"}, at=99)
    assert len(doc.cards) == 2  # the out-of-range insert landed nothing


def test_card_selector_targets_the_composable_card_on_both_lanes():
    """`card=i` reads and writes through the kind's schema, and an index
    addressing no card raises on either lane."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"}, "A quote body.")
    v = quill.reader(doc)

    assert doc.card(0)["kind"] == "quotes"
    assert v.get("author", card=0) == "Basho"
    assert v.body_markdown(card=0) == "A quote body."

    ed.set("author", "Issa", card=0)
    assert field(doc.cards[0], "author") == "Issa"
    assert v.get("author", card=0) == "Issa"

    with raises_edit_code("edit::unknown_field"):
        v.get("stray", card=0)
    with raises_edit_code("edit::index_out_of_range"):
        ed.set("author", "x", card=9)
    with raises_edit_code("edit::index_out_of_range"):
        v.get("author", card=9)


def test_writer_set_coerces_richtext_to_content():
    """A richtext field commits the canonical content, not the authored markdown."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).set("bio", "A **bold** intro.")
    value = field(doc.main, "bio")
    assert isinstance(value, dict)  # stored as the content dict, not a string
    assert value["text"] == "A bold intro."


def test_writer_set_rejects_inline_violation():
    """A richtext(inline) field rejects multi-block content at the write."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).set("headline", "line one\n\nline two")


def test_writer_set_all_is_all_or_nothing():
    """A mid-batch inline violation aborts set_all: nothing lingers."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).set_all({"bio": "ok", "headline": "line one\n\nline two"})
    assert not has_field(doc.main, "bio")


def test_writer_revise_field_typed_and_anchor_preserving():
    """writer.revise_field is the typed, anchor-preserving richtext field write:
    diff-imports the markdown and schema-conforms the result."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).revise_field("bio", "make it **bold**")
    assert quill.reader(doc).get("bio") == "make it **bold**"


def test_writer_revise_field_rejects_inline_and_unknown():
    """revise_field conforms to the field schema (inline rejects multi-block) and
    rejects an undeclared name: same guards as `set`."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).revise_field("headline", "line one\n\nline two")
    with raises_edit_code("edit::unknown_field"):
        quill.writer(doc).revise_field("nope", "x")


def test_typed_set_clears_must_fill_marker():
    """seed → validate(must_fill) → typed `set` is the fill lifecycle: the typed
    commit lands a real value and clears the marker."""
    quill = taro_quill()
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: taro@0.1.0\n$kind: main\ntitle: !must_fill\n~~~\n"
    )
    def fills(path):
        return [
            d
            for d in quill.validate(doc)
            if d.get("code") == "validation::must_fill" and d.get("path") == path
        ]

    # Scoped to `main.title`: this quill's other defaultless field is obliged
    # too, and stays so.
    assert [d.get("args", {}).get("trigger") for d in fills("main.title")] == ["marker"]

    quill.writer(doc).set("title", "Real Title")
    assert fills("main.title") == []
    assert field(doc.main, "title") == "Real Title"


def test_view_interprets_by_declared_type():
    """view.get reads a richtext field as markdown and a scalar as its canonical value."""
    quill = richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    w = quill.writer(doc)
    w.set("bio", "A **bold** intro.")
    v = quill.reader(doc)
    assert w.document is doc and v.document is doc  # both hold the same object
    assert v.get("bio") == "A **bold** intro."  # richtext → markdown

    taro = taro_quill()
    tdoc = Document("taro@0.1.0")
    taro.writer(tdoc).set("author", "Ada")
    assert taro.reader(tdoc).get("author") == "Ada"  # scalar → canonical


def test_view_absence_returns_none_unknown_name_raises():
    """Absent → None; a name the schema does not declare raises (the schema authority)."""
    quill = richtext_form_quill()
    v = quill.reader(Document("richtext_form@0.1.0"))
    assert v.get("bio") is None  # absent, not a typo
    with raises_edit_code("edit::unknown_field"):
        v.get("nope")  # typo, not absent


def test_view_richtext_holding_scalar_raises_mismatch():
    """A present value that does not decode as richtext raises FieldDecode.

    Seated quill-free via `from_markdown`: a bare number under a richtext field."""
    quill = richtext_form_quill()
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: richtext_form@0.1.0\n$kind: main\nbio: 3\n~~~\n"
    )
    with raises_edit_code("edit::field_decode"):
        quill.reader(doc).get("bio")


def test_view_get_content_spans_both_storage_forms():
    """get_content returns the corpus whichever lane built the document.

    The writer commits a canonical content dict; a markdown parse leaves the
    authored string. Both read back as the same corpus, so the storage form
    stops being the caller's business."""
    quill = richtext_form_quill()
    committed = Document("richtext_form@0.1.0")
    quill.writer(committed).set("bio", "A **bold** intro.")
    assert isinstance(field(committed.main, "bio"), dict)  # committed lane

    parsed = Document.from_markdown(
        "~~~card-yaml\n$quill: richtext_form@0.1.0\n$kind: main\nbio: A **bold** intro.\n~~~\n"
    )
    assert isinstance(field(parsed.main, "bio"), str)  # parsed lane

    a = quill.reader(committed).get_content("bio")
    b = quill.reader(parsed).get_content("bio")
    assert a["text"] == "A bold intro."
    assert b["text"] == a["text"]
    assert b["marks"] == a["marks"]


def test_view_get_content_absence_unknown_and_non_content():
    """Absent → None; an undeclared name and a non-content type each raise."""
    quill = richtext_form_quill()
    assert quill.reader(Document("richtext_form@0.1.0")).get_content("bio") is None
    with raises_edit_code("edit::unknown_field"):
        quill.reader(Document("richtext_form@0.1.0")).get_content("nope")

    taro = taro_quill()
    tdoc = Document("taro@0.1.0")
    taro.writer(tdoc).set("author", "Ada")
    # A declared type carrying no content answers from the schema, not the
    # payload: `author` holds a string and is still not content.
    with raises_edit_code("edit::field_not_content"):
        taro.reader(tdoc).get_content("author")


ELEMENT_QUILL_YAML = """quill:
  name: element_test
  version: 0.1.0
  backend: typst
  description: Content nested inside a composite field

typst:
  plate_file: plate.typ

main:
  fields:
    recipients:
      type: array
      items:
        type: plaintext
    paragraphs:
      type: array
      items:
        type: richtext
    tags:
      type: array
      items:
        type: string
    rows:
      type: array
      items:
        type: object
        properties:
          notes:
            type: richtext

card_kinds:
  note:
    fields:
      lines:
        type: array
        items:
          type: plaintext
"""


@pytest.fixture
def element_quill(tmp_path):
    """A quill declaring every content-bearing composite shape."""
    root = tmp_path / "element_test" / "0.1.0"
    root.mkdir(parents=True)
    (root / "Quill.yaml").write_text(ELEMENT_QUILL_YAML)
    (root / "plate.typ").write_text('#import "@local/quillmark-helper:0.1.0": data\n')
    return Quill.from_path(str(root))


def test_get_content_at_round_trips_an_elements_anchor_and_island_id(element_quill):
    """The nested Content read is the lossless one.

    An anchor mark and an island id ride read -> edit -> `set` intact; the same
    loop through `get`, which projects to text, keeps neither."""
    doc = Document("element_test@0.1.0")
    w = element_quill.writer(doc)
    v = element_quill.reader(doc)
    w.set("paragraphs", ["Plain", "Alpha ![pic](u) bold"])

    rt = v.get_content_at("paragraphs", [1])
    rt["marks"].append({"type": "anchor", "attrs": {"id": "c1"}, "start": 0, "end": 5})
    rt["islands"][0]["id"] = "isl-7"  # off the positional mint, so a re-mint shows
    w.set("paragraphs", [v.get_content_at("paragraphs", [0]), rt])

    back = v.get_content_at("paragraphs", [1])
    assert any(
        m["type"] == "anchor" and m["attrs"]["id"] == "c1" and (m["start"], m["end"]) == (0, 5)
        for m in back["marks"]
    )
    assert back["islands"][0]["id"] == "isl-7"

    text = v.get("paragraphs")
    assert all(isinstance(t, str) for t in text)
    w.set("paragraphs", text)
    lost = v.get_content_at("paragraphs", [1])
    assert not any(m["type"] == "anchor" for m in lost["marks"])
    assert lost["islands"][0]["id"] == "isl-0"


def test_get_content_at_path_and_card_selector(element_quill):
    """`path` walks to the leaf's own codec, a path naming nothing stored reads
    None, a malformed step and a bare `str` are the argument's error, and
    `card=` addresses a composable card's schema."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: element_test@0.1.0\n$kind: main\n"
        "recipients: ['a *literal* line']\nparagraphs: ['A **bold** intro.', 3]\n"
        "tags: ['x']\nrows:\n  - {}\n  - notes: a *note*\n~~~\n"
    )
    element_quill.writer(doc).add_card("note", {"lines": ["a *b*"]})
    v = element_quill.reader(doc)
    assert v.get_content_at("recipients", [0])["text"] == "a *literal* line"
    assert v.get_content_at("paragraphs", [0])["text"] == "A bold intro."
    assert v.get_content_at("rows", [1, "notes"])["text"] == "a note"
    assert v.get_content_at("lines", [0], card=0)["text"] == "a *b*"
    assert v.get_content_at("rows", [0, "notes"]) is None
    assert v.get_content_at("recipients", [7]) is None
    with raises_edit_code("edit::field_not_content"):
        v.get_content_at("tags", [0])
    with raises_edit_code("edit::unknown_field"):
        v.get_content_at("rows", [1, "nope"])
    with raises_edit_code("edit::index_out_of_range"):
        v.get_content_at("lines", [0], card=9)
    with pytest.raises(QuillmarkError) as excinfo:
        v.get_content_at("paragraphs", [1])
    assert excinfo.value.diagnostics[0].path == "main.paragraphs[1]"
    with pytest.raises(ValueError, match=r"path\[0\]"):
        v.get_content_at("recipients", [None])
    # A bare str is a sequence of one-character keys, so it would read
    # `rows.n.o.t.e.s` and say nothing.
    with pytest.raises(ValueError, match=r"bare str"):
        v.get_content_at("rows", "notes")


def test_view_body_read_is_quill_free():
    """view.body_markdown reads the main body markdown: the quill-free body read."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    quill.writer(doc).revise_body("A **taro** essay.")
    assert quill.reader(doc).body_markdown() == "A **taro** essay."
