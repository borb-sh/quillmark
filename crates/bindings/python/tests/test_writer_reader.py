"""Tests for the typed field surface: `quill.writer(doc)` writes,
`quill.reader(doc)` reads, both against the quill's schema. The quill-free
mutator surface is `test_document.py`."""

import pytest
from quillmark import Document, Quill, QuillmarkError

from conftest import (
    TARO_MARKDOWN,
    field,
    has_field,
    raises_edit_code,
    richtext_quill,
    taro_quill,
)


def test_writer_set_all_reports_every_unknown_field():
    """set_all is all-or-nothing and reports one diagnostic per undeclared name:
    externally-sourced keys surface every violation at once."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    with raises_edit_code("edit::unknown_field") as exc_info:
        quill.writer(doc).set_all({"title": "ok", "stray1": "x", "stray2": "y"})
    paths = [d.path for d in exc_info.value.diagnostics]
    assert "main.stray1" in paths and "main.stray2" in paths
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


def test_writer_add_card_transactional_and_positioned():
    """add_card fuses make + typed commit + insert at `at=`; a refusal leaves
    the doc untouched."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    first = ed.add_card("quotes", {"author": "First"}, "A quote body.")
    ed.add_card("quotes", {"author": "Second"}, at=0)
    assert [field(c, "author")["text"] for c in doc.cards] == ["Second", "First"]
    assert first == {"id": doc.cards[1]["id"], "warnings": []}
    with raises_edit_code("edit::unknown_field"):
        ed.add_card("quotes", {"stray": "x"})
    with raises_edit_code("edit::index_out_of_range"):
        ed.add_card("quotes", {"author": "x"}, at=99)
    assert len(doc.cards) == 2


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
    assert doc.to_markdown_at(card=0) == "A quote body."

    ed.set("author", "Issa", card=0)
    assert field(doc.cards[0], "author")["text"] == "Issa"
    assert v.get("author", card=0) == "Issa"

    with raises_edit_code("edit::unknown_field"):
        v.get("stray", card=0)
    with raises_edit_code("edit::index_out_of_range"):
        ed.set("author", "x", card=9)
    with raises_edit_code("edit::index_out_of_range"):
        v.get("author", card=9)




def test_writer_revise_field():
    """revise_field diff-imports markdown into a richtext field under the same
    guards as `set`, returning the receipt a content write answers."""
    quill = richtext_quill()
    doc = Document("sample_form@0.1.0")
    w = quill.writer(doc)
    receipt = w.revise("make it **bold**", "bio")
    assert sorted(receipt) == ["applied", "dropped_anchors", "hash", "minted", "moved_anchors", "warnings"]
    assert receipt["applied"]["steps"][0]["splice"]["insert"] == "make it bold"
    assert receipt["hash"] == w.hash_at("bio")
    assert (receipt["dropped_anchors"], receipt["moved_anchors"], receipt["warnings"]) == ([], [], [])
    assert quill.reader(doc).get("bio") == "make it **bold**"
    (dropped,) = w.revise("make it <kbd>**bold**</kbd>", "bio")["warnings"]
    assert (dropped.code, dropped.path) == ("parse::dropped_construct", "main.bio")
    assert quill.reader(doc).get("bio") == "make it **bold**"
    with raises_edit_code("edit::field_not_single_line"):
        w.revise("line one\n\nline two", "headline")
    with raises_edit_code("edit::unknown_field"):
        w.revise("x", "nope")


def test_writer_set_returns_what_its_markdown_dropped():
    """set and set_all return a parse::dropped_construct per construct a
    richtext string drops, at the string's path."""
    quill = richtext_quill()
    w = quill.writer(Document("sample_form@0.1.0"))
    (dropped,) = w.set("bio", "make it <kbd>**bold**</kbd>")
    assert (dropped.code, dropped.path) == ("parse::dropped_construct", "main.bio")
    (dropped,) = w.set_all({"bio": "plain", "headline": "a <span>b</span>"})
    assert (dropped.code, dropped.path) == ("parse::dropped_construct", "main.headline")
    assert w.set_all({"bio": "plain"}) == []


def test_writer_revise_document():
    """revise_document replaces the document from markdown, conforms it, and
    returns the warnings, clearing the load's; a foreign `$quill` raises and
    changes nothing."""
    quill = richtext_quill()
    doc = quill.parse("~~~\n$quill: sample_form@0.1.0\n~~~\n\n<span>Body.</span>\n")
    assert [d.code for d in doc.warnings] == ["parse::dropped_construct"]
    w = quill.writer(doc)
    w.revise("make it **bold**", "bio")
    receipt = w.revise_document(
        "~~~\n$quill: sample_form@0.1.0\nbio: make it <kbd>**bold**</kbd>\n~~~\n\nBody.\n"
    )
    assert sorted(receipt) == ["dropped_anchors", "from", "warnings"]
    assert (receipt["dropped_anchors"], receipt["from"]) == ([], [])
    (dropped,) = receipt["warnings"]
    assert (dropped.code, dropped.path) == ("parse::dropped_construct", "main.bio")
    assert quill.reader(doc).get("bio") == "make it **bold**"
    assert doc.warnings == []
    before = doc.to_markdown()
    with pytest.raises(QuillmarkError):
        w.revise_document("~~~\n$quill: other\n~~~\n")
    assert doc.to_markdown() == before


def test_a_stale_base_hash_refuses_naming_the_hash_now():
    """A revise or a document revise lands under the hash now and refuses a
    stale one, naming the hash now and writing nothing."""
    quill = richtext_quill()
    doc = quill.parse("~~~\n$quill: sample_form@0.1.0\n~~~\n\nAlpha.\n\nBeta.\n")
    w = quill.writer(doc)
    read, hash, body_hash = doc.to_markdown(), w.hash(), w.hash_at()
    assert body_hash == doc.hash_at()
    landed = w.revise("Alpha, edited.\n\nBeta.", base_hash=body_hash)
    held = doc.to_stored()
    with raises_edit_code("edit::stale_base") as exc_info:
        w.revise("Alpha.\n\nBeta, mine.", base_hash=body_hash)
    assert exc_info.value.diagnostics[0].args == {"current": landed["hash"]}
    with raises_edit_code("edit::stale_base"):
        w.revise_document(read, base_hash=hash)
    assert doc.to_stored() == held
    w.revise_document(doc.to_markdown().replace("Beta.", "Beta, rewritten."), base_hash=w.hash())
    assert doc.to_markdown().endswith("Alpha, edited.\n\nBeta, rewritten.\n")
    with pytest.raises(TypeError):
        w.revise("x", base_content=doc.read())
    with pytest.raises(TypeError):
        w.revise_document(read, base_stored=held)


def test_a_field_hash_reads_through_the_schema():
    """A richtext field hashes its content whether it rests as a markdown
    string or as conformed content, so a conform between the read and the
    write does not stale the base; an undeclared name raises."""
    quill = richtext_quill()
    doc = Document.parse("~~~\n$quill: sample_form@0.1.0\nbio: Hello *world*\n~~~\n")
    w = quill.writer(doc)
    hash, whole = w.hash_at("bio"), w.hash()
    assert hash == doc.hash_at("bio")
    quill.conform(doc)
    assert (w.hash_at("bio"), w.hash()) == (hash, whole)
    w.revise("Hello _world_, hi", "bio", base_hash=hash)
    with raises_edit_code("edit::unknown_field"):
        w.hash_at("nope")


def test_an_anchor_survives_the_field_and_document_revises():
    """revise_field and revise_document carry an anchor the field holds."""
    quill = richtext_quill()
    doc = Document("sample_form@0.1.0")
    quill.writer(doc).revise("make it bold here", "bio")
    doc.apply({"steps": [{"anchor": {"id": "a1", "start": 8, "end": 12}}]}, field="bio")

    w = quill.writer(doc)
    assert w.revise("now make it bold here", "bio")["dropped_anchors"] == []
    assert doc.read("bio")["anchors"] == [{"id": "a1", "start": 12, "end": 16}]
    receipt = w.revise_document("~~~\n$quill: sample_form@0.1.0\nbio: so now make it bold here\n~~~\n")
    assert receipt["dropped_anchors"] == []
    assert doc.read("bio")["anchors"] == [{"id": "a1", "start": 15, "end": 19}]


def test_a_dropped_anchor_is_named_and_reattached_where_its_text_is_found_once():
    """Each revise receipt names the anchors it dropped; anchor_contexts reads
    the selector a host keeps, and reattach places it again under the exact
    rule, or reports it orphaned."""
    quill = richtext_quill()
    doc = quill.parse("~~~\n$quill: sample_form@0.1.0\n~~~\n\nThe committee met on Monday to plan.\n")
    doc.apply({"steps": [{"anchor": {"id": "m1", "start": 14, "end": 27}}]})
    context = {
        "id": "m1",
        "path": "main.body",
        "card_id": None,
        "quote": "met on Monday",
        "prefix": "The committee ",
        "suffix": " to plan.",
    }
    assert doc.anchor_contexts() == [context]
    assert doc.anchor_context("m1") == context
    assert doc.anchor_context("nope") is None

    w = quill.writer(doc)
    receipt = w.revise("Nothing like it.\n\nAt all.")
    assert receipt["dropped_anchors"] == [context]
    assert doc.reattach([context], {"main.body": receipt["applied"]}) == {"reattached": [], "orphaned": [context]}

    receipt = w.revise_document("~~~\n$quill: sample_form@0.1.0\n~~~\n\nMinutes.\n\nThe committee met on Monday to plan.\n")
    assert receipt["dropped_anchors"] == []
    out = doc.reattach([context])
    assert out == {"reattached": [{**context, "prefix": "Minutes.\nThe committee "}], "orphaned": []}
    assert doc.reattach([context]) == {"reattached": out["reattached"], "orphaned": []}
    with pytest.raises(ValueError):
        doc.reattach([{**context, "path": "main.["}])


def test_markdown_writes_return_their_drops_at_the_body_written():
    """revise_body and add_card return the import's drops, each at the body
    it wrote."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    w = quill.writer(doc)

    def drops(warnings):
        return [(d.code, d.path, d.args["construct"]) for d in warnings]

    assert drops(w.revise("a <kbd>b</kbd>")["warnings"]) == [
        ("parse::dropped_construct", "main.body", "kbd")
    ]
    assert drops(w.add_card("quotes", {"author": "A"}, "x <span>y</span>")["warnings"]) == [
        ("parse::dropped_construct", "cards.quotes[0].body", "span")
    ]
    assert drops(w.add_card("quotes", {"author": "B"}, "<em>z</em>", at=0)["warnings"]) == [
        ("parse::dropped_construct", "cards.quotes[0].body", "em")
    ]
    assert drops(w.revise("c <kbd>d</kbd>", card=1)["warnings"]) == [
        ("parse::dropped_construct", "cards.quotes[1].body", "kbd")
    ]
    assert doc.to_markdown_at(card=1) == "c d"


def test_reader_interprets_by_declared_type():
    """reader.get reads richtext as markdown, absence as None; an undeclared
    name raises."""
    quill = richtext_quill()
    doc = Document("sample_form@0.1.0")
    w = quill.writer(doc)
    v = quill.reader(doc)
    assert v.get("bio") is None
    w.set("bio", "A **bold** intro.")
    assert w.document is doc and v.document is doc
    assert v.get("bio") == "A **bold** intro."
    with raises_edit_code("edit::unknown_field"):
        v.get("nope")


def test_reader_richtext_holding_scalar_raises_mismatch():
    """A present value that does not decode as richtext raises FieldDecode.

    Seated quill-free via `Document.parse`: a bare number under a richtext field."""
    quill = richtext_quill()
    doc = Document.parse(
        "~~~card-yaml\n$quill: sample_form@0.1.0\n$kind: main\nbio: 3\n~~~\n"
    )
    with raises_edit_code("edit::field_decode"):
        quill.reader(doc).get("bio")


def test_reader_get_content_spans_both_storage_forms():
    """get_content returns the corpus whichever lane built the document.

    The writer commits a canonical content dict; a markdown parse leaves the
    authored string. Both read back as the same corpus, so the storage form
    stops being the caller's business."""
    quill = richtext_quill()
    committed = Document("sample_form@0.1.0")
    quill.writer(committed).set("bio", "A **bold** intro.")
    assert isinstance(field(committed.main, "bio"), dict)  # committed lane

    parsed = Document.parse(
        "~~~card-yaml\n$quill: sample_form@0.1.0\n$kind: main\nbio: A **bold** intro.\n~~~\n"
    )
    assert isinstance(field(parsed.main, "bio"), str)  # parsed lane

    a = quill.reader(committed).get_content("bio")
    b = quill.reader(parsed).get_content("bio")
    assert a["text"] == "A bold intro."
    assert b["text"] == a["text"]
    assert b["marks"] == a["marks"]


def test_reader_get_content_absence_unknown_and_non_content():
    """Absent → None; an undeclared name and a non-content type each raise."""
    quill = richtext_quill()
    assert quill.reader(Document("sample_form@0.1.0")).get_content("bio") is None
    with raises_edit_code("edit::unknown_field"):
        quill.reader(Document("sample_form@0.1.0")).get_content("nope")

    doc = Document("sample_form@0.1.0")
    quill.writer(doc).set("favorite_color", "red")
    # A declared type carrying no content answers from the schema, not the
    # payload: `favorite_color` holds a string and is still not content.
    with raises_edit_code("edit::field_not_content"):
        quill.reader(doc).get_content("favorite_color")


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
        type: text
        multiline: true
    paragraphs:
      type: array
      items:
        type: richtext
        multiline: true
    tags:
      type: array
      items:
        type: enum
        values: [x]
    rows:
      type: array
      items:
        type: object
        properties:
          notes:
            type: richtext
            multiline: true

card_kinds:
  note:
    fields:
      lines:
        type: array
        items:
          type: text
          multiline: true
"""


@pytest.fixture
def element_quill(tmp_path):
    """A quill declaring every content-bearing composite shape."""
    root = tmp_path / "element_test" / "0.1.0"
    root.mkdir(parents=True)
    (root / "Quill.yaml").write_text(ELEMENT_QUILL_YAML)
    (root / "plate.typ").write_text('#import "@local/quillmark-helper:0.2.0" as qm\n')
    return Quill.from_path(str(root))


def test_get_content_at_round_trips_an_elements_anchor_and_island_id(element_quill):
    """The nested Content read is the lossless one.

    An anchor and an island id ride read -> edit -> `set` intact; the same
    loop through `get`, which projects to text, keeps neither."""
    doc = Document("element_test@0.1.0")
    w = element_quill.writer(doc)
    v = element_quill.reader(doc)
    w.set("paragraphs", ["Plain", "Alpha ![pic](u) bold"])

    rt = v.get_content_at("paragraphs", [1])
    rt["anchors"] = [{"id": "c1", "start": 0, "end": 5}]
    rt["islands"][0]["id"] = "isl-7"  # off the positional mint, so a re-mint shows
    w.set("paragraphs", [v.get_content_at("paragraphs", [0]), rt])

    back = v.get_content_at("paragraphs", [1])
    assert back["anchors"] == [{"id": "c1", "start": 0, "end": 5}]
    assert back["islands"][0]["id"] == "isl-7"

    text = v.get("paragraphs")
    assert all(isinstance(t, str) for t in text)
    w.set("paragraphs", text)
    lost = v.get_content_at("paragraphs", [1])
    assert "anchors" not in lost
    assert lost["islands"][0]["id"] == "isl-0"


def test_get_content_at_path_and_card_selector(element_quill):
    """`path` walks to the leaf's own codec, a path naming nothing stored reads
    None, a malformed step and a bare `str` are the argument's error, and
    `card=` addresses a composable card's schema."""
    doc = Document.parse(
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


def test_to_markdown_at_reads_the_main_body():
    """to_markdown_at() with no address reads the main body markdown."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")
    quill.writer(doc).revise("A **taro** essay.")
    assert doc.to_markdown_at() == "A **taro** essay."


def test_a_text_field_revise_answers_one_splice_over_its_literal_text():
    """A `text` field's receipt is one splice over the literal text, in code
    points."""
    doc = taro_quill().parse(TARO_MARKDOWN)
    w = taro_quill().writer(doc)
    w.revise("a \U0001F600 \\*b\\*", "title")
    receipt = w.revise("a \U0001F600 \\*b\\* c", "title")
    assert receipt["applied"] == {"steps": [{"splice": {"at": 9, "delete": "", "insert": " c", "removed": {}}}]}
    assert receipt["hash"] == w.hash_at("title")
    assert w.revise("a \U0001F600 \\*b\\* c", "title")["applied"] == {"steps": []}
