"""Tests for the quill-free mutator surface: card structure, fields, `$ext`, and
the markdown a mutated document emits. Parse and storage transport is
`test_parse.py`; typed field I/O is `test_writer_reader.py`."""

import datetime

import pytest
from quillmark import Document

from conftest import (
    field,
    field_keys,
    make_card,
    raises_edit_code,
    taro_quill,
)


SIMPLE_MD = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\nauthor: Alice\n~~~\n\nBody text.\n"

MD_WITH_CARDS = """\
~~~card-yaml
$quill: test_quill
$kind: main
title: Hello
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card one.

~~~card-yaml
$kind: summary
~~~

Card two.
"""


def test_parsed_document_quill_ref():
    markdown_with_quill = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Test\n~~~\n\n# Content\n"
    parsed = Document.from_markdown(markdown_with_quill)
    assert parsed.quill_ref == "my_quill"


def test_blank_document_constructor():
    """Document(quill_ref) starts blank: main card only, no fields, no cards."""
    doc = Document("taro@0.1.0")
    assert doc.quill_ref == "taro@0.1.0"
    assert doc.card_count == 0
    assert doc.body["text"] == ""
    assert field_keys(doc.main) == []

    with pytest.raises(ValueError):
        Document("not a valid ref!!")


def test_remove_card_then_insert_card_round_trips_fields():
    """A card returned by remove_card feeds straight back into insert_card with
    its fields intact: the one-Card-shape contract. Exercises the explicit
    quill/ext=None keys the dict carries against `deny_unknown_fields`."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(make_card("note", {"author": "Alice"}, "Body"))

    removed = doc.remove_card(0)
    # The returned dict carries explicit None for the absent $ entries.
    assert removed["quill"] is None
    assert field(removed, "author") == "Alice"

    doc.insert_card(removed)  # must not raise (deny_unknown_fields accepts the shape)
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert field(doc.cards[0], "author") == "Alice"  # field survived the round-trip
    assert doc.cards[0]["body"]["text"] == "Body"


def test_insert_card_accepts_content_dict_body():
    """`body` on an inserted card may be the canonical content dict (the shape
    `cards()`/`remove_card` emit), not just a markdown string: exercises
    `py_dict_to_card`'s content-dict input path."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card({"kind": "note", "body": "**Bold** body."})
    content_body = doc.cards[0]["body"]
    assert isinstance(content_body, dict)

    doc.insert_card({"kind": "note", "body": content_body})
    assert doc.cards[1]["body"]["text"] == doc.cards[0]["body"]["text"]


def test_wire_refusal_carries_the_mutator_code():
    """A card dict whose content violates an invariant raises QuillmarkError
    under the code the addressed mutator mints, not a bare ValueError: routing
    is `diagnostics[0].code`. A dict whose *shape* the binding cannot read at
    all stays a ValueError."""
    doc = Document.from_markdown(SIMPLE_MD)

    def with_field(key, value):
        return {
            "kind": "note",
            "payload_items": [{"type": "field", "key": key, "value": value}],
        }

    with raises_edit_code("edit::invalid_kind_name"):
        doc.insert_card(make_card("BadKind", {"x": 1}))
    with raises_edit_code("edit::invalid_field_name"):
        doc.insert_card(with_field("bad-name", 1))
    with raises_edit_code("parse::invalid_quill_reference"):
        doc.insert_card({"kind": "note", "quill": "@nope"})


def test_stale_flat_input_is_a_loud_error():
    """An unknown card key (a stale `fields`, an `id`) fails at deserialize time
    as a ValueError rather than yielding a card missing it."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="fields"):
        doc.insert_card({"kind": "note", "fields": {"x": 1}})
    with pytest.raises(ValueError):
        doc.insert_card({"kind": "note", "id": "first"})
    assert doc.cards == []


def test_negative_index_is_out_of_range():
    """Indices count from the front, so -1 addresses no card rather than the last
    one. Every index-taking surface answers it as it answers an index past the
    end: `edit::index_out_of_range` where the verb raises, `None` where it
    answers absence."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    with raises_edit_code("edit::index_out_of_range") as exc_info:
        doc.card(-1)
    assert exc_info.value.diagnostics[0].args["index"] == -1
    with raises_edit_code("edit::index_out_of_range"):
        doc.move_card(-1, 0)
    with raises_edit_code("edit::index_out_of_range"):
        doc.insert_card({"kind": "note"}, at=-1)
    with raises_edit_code("edit::index_out_of_range"):
        doc.store_ext({}, card=-1)
    with raises_edit_code("edit::index_out_of_range"):
        doc.card(2)
    assert doc.remove_card(-1) is None
    assert doc.remove_card(99) is None
    assert len(doc.cards) == 2

    quill = taro_quill()
    typed = Document("taro@0.1.0")
    ed = quill.writer(typed)
    ed.add_card("quotes", {"author": "Basho"})
    with raises_edit_code("edit::index_out_of_range"):
        ed.set("author", "Issa", card=-1)
    with raises_edit_code("edit::index_out_of_range"):
        quill.reader(typed).get("author", card=-1)


def test_card_reads_one_card_without_projecting_the_rest():
    """card(i) is the card-indexed twin of `main`: the same dict shape `cards`
    projects, for one card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    assert doc.card(0)["kind"] == "note"
    assert doc.card(0) == doc.cards[0]
    assert doc.card(1)["kind"] == "summary"


def test_store_ext_rejects_non_dict():
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="must be a dict"):
        doc.store_ext("nope")


def test_unsupported_value_types_are_refused():
    """At the boundary, rather than as the value's repr — a tuple stored as
    "('a', 'b')" only surfaces as garbage at render or read-back."""
    doc = Document.from_markdown(SIMPLE_MD)
    for bad in [("a", "b"), {"x"}, b"bytes", object()]:
        with pytest.raises(ValueError, match="no JSON form"):
            doc.store_ext({"k": bad})


def test_dates_keep_their_stringified_form():
    """The three types whose `str()` is the spelling a field reads."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext(
        {
            "d": datetime.date(2026, 8, 30),
            "t": datetime.time(9, 30),
            "dt": datetime.datetime(2026, 8, 30, 9, 30),
        }
    )
    assert doc.main["ext"]["d"] == "2026-08-30"
    assert doc.main["ext"]["t"] == "09:30:00"
    assert doc.main["ext"]["dt"] == "2026-08-30 09:30:00"


def test_namespace_scoped_write_is_a_merge_over_the_whole_map():
    """`main["ext"]`'s read shape is store_ext's write shape, which is what
    makes the whole-map verb enough for a consumer owning one namespace."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"presentation": {"title": "A"}})
    doc.store_ext({**doc.main["ext"], "agent": {"pinned": True}})
    doc.store_ext({**doc.main["ext"], "presentation": {"title": "B"}})
    assert doc.main["ext"] == {
        "presentation": {"title": "B"},
        "agent": {"pinned": True},
    }


def test_ext_mutators_on_main_and_card():
    """store_ext / remove_ext take one `card=` selector over the whole axis;
    remove_ext answers with what it took."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.store_ext({"agent": {"n": 1}})
    doc.store_ext({"agent": {"note": "y"}}, card=0)
    assert doc.cards[0]["ext"] == {"agent": {"note": "y"}}
    assert doc.remove_ext(card=0) == {"agent": {"note": "y"}}
    assert doc.cards[0]["ext"] is None
    assert doc.remove_ext() == {"agent": {"n": 1}}
    assert doc.main["ext"] is None
    assert doc.remove_ext() is None
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_ext(card=5)


def test_a_mutation_sequence_keeps_its_order_and_its_invariants():
    """Every positional verb, read through the order it leaves behind: append
    lands at the end, `at=` before the card holding that index, `move_card`
    lifts and reinserts, `remove_card` closes the gap."""
    doc = Document.from_markdown(SIMPLE_MD)

    def kinds():
        return [c["kind"] for c in doc.cards]

    doc.insert_card(make_card("note", {"text": "hi"}, "Card body."))
    assert kinds() == ["note"]
    assert doc.cards[0]["body"]["text"] == "Card body."

    doc.insert_card({"kind": "summary"})
    doc.insert_card({"kind": "appendix"})
    assert kinds() == ["note", "summary", "appendix"]

    doc.insert_card({"kind": "intro"}, at=1)
    assert kinds() == ["note", "intro", "summary", "appendix"]

    doc.move_card(3, 0)
    assert kinds() == ["appendix", "note", "intro", "summary"]

    removed = doc.remove_card(2)
    assert removed["kind"] == "intro"
    assert kinds() == ["appendix", "note", "summary"]

    assert doc.remove_field("author") == "Alice"


def test_to_markdown_general_round_trip():
    """A typed-writer mutation survives emit → re-parse with structure intact."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")

    # Mutate: typed field + typed body + a quill-free card.
    quill.writer(doc).set("title", "New Title")
    doc.insert_card(make_card("note", {"author": "Alice"}, "Hello"))
    quill.writer(doc).revise_body("Updated body")

    doc2 = Document.from_markdown(doc.to_markdown())
    assert field(doc2.main, "title") == "New Title"
    assert doc2.body["text"].rstrip("\n") == "Updated body"
    assert len(doc2.cards) == 1
    assert doc2.cards[0]["kind"] == "note"
    assert field(doc2.cards[0], "author") == "Alice"
    assert doc2.cards[0]["body"]["text"] == "Hello"
