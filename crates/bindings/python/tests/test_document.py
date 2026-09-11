"""Tests for the quill-free mutator surface: card structure, fields, `$ext`, and
the markdown a mutated document emits. Parse and storage transport is
`test_parse.py`; typed field I/O is `test_writer_reader.py`."""

import datetime

import pytest
from quillmark import Document, QuillmarkError

from conftest import (
    field,
    field_keys,
    has_field,
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

    markdown_without_quill = "# Just content\n\nNo card-yaml block here.\n"
    with pytest.raises(QuillmarkError):
        Document.from_markdown(markdown_without_quill)


def test_blank_document_constructor():
    """Document(quill_ref) starts blank: main card only, no fields, no cards. A
    field write needs a quill (the writer); the blank canvas itself is quill-free."""
    doc = Document("taro@0.1.0")
    assert doc.quill_ref == "taro@0.1.0"
    assert doc.card_count == 0
    assert doc.body["text"] == ""
    assert field_keys(doc.main) == []

    taro_quill().writer(doc).set("title", "Hello")
    assert field(doc.main, "title") == "Hello"

    with pytest.raises(ValueError, match="QuillReference"):
        Document("not a valid ref!!")


def test_remove_field_existing():
    """remove_field removes and returns an existing main-card field."""
    doc = Document.from_markdown(SIMPLE_MD)
    val = doc.remove_field("title")
    assert val == "Hello"
    assert not has_field(doc.main, "title")


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


def test_insert_card_is_the_kind_gate():
    """Kind validity is positional, so only insert_card can rule on it, and it
    is the one reporting `edit::invalid_kind_name`."""
    doc = Document.from_markdown(SIMPLE_MD)
    with raises_edit_code("edit::invalid_kind_name"):
        doc.insert_card(make_card("BadKind", {"x": 1}))


def test_wire_refusal_carries_the_mutator_code():
    """A card dict whose content violates an invariant raises QuillmarkError
    under the code the addressed mutator mints, not a bare ValueError: routing
    is `diagnostics[0].code`. A dict whose *shape* the binding cannot read at
    all stays a ValueError."""
    doc = Document.from_markdown(SIMPLE_MD)

    def with_field(key, value, fill=False):
        return {
            "kind": "note",
            "payload_items": [
                {"type": "field", "key": key, "value": value, "fill": fill}
            ],
        }

    with raises_edit_code("edit::invalid_field_name"):
        doc.insert_card(with_field("bad-name", 1))
    with raises_edit_code("edit::fill_on_mapping"):
        doc.insert_card(with_field("addr", {"a": 1}, fill=True))
    with raises_edit_code("parse::invalid_quill_reference"):
        doc.insert_card({"kind": "note", "quill": "@nope"})


def test_stale_flat_input_is_a_loud_error():
    """A stale {kind, fields} dict fails loudly rather than yielding an empty
    card: `deny_unknown_fields` on the wire type rejects the unknown `fields`
    key at deserialize time (a ValueError), before any edit."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="fields"):
        doc.insert_card({"kind": "note", "fields": {"x": 1}})


def test_insert_card_out_of_range():
    """insert_card raises EditError when at > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with raises_edit_code("edit::index_out_of_range"):
        doc.insert_card({"kind": "note"}, at=5)


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
    projects, for one card. Out of range raises, matching the write verbs."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    assert doc.card(0)["kind"] == "note"
    assert doc.card(0) == doc.cards[0]
    assert doc.card(1)["kind"] == "summary"
    with raises_edit_code("edit::index_out_of_range"):
        doc.card(2)


def test_card_input_rejects_an_id_key():
    """`id` is not a card key: the dict shape rejects it like any unknown."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(Exception):
        doc.insert_card({"kind": "note", "id": "first", "body": "A"})


def test_seed_overlay_reads_one_kind_off_the_main_card():
    """seed_overlay reads one `$seed[kind]` entry: the overlay that feeds
    quill.seed_card(kind, overlay): without projecting the whole main card.
    Total over the kind axis: an absent kind reads back None."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_seed_overlay("note", {"author": "Seeded"})
    assert doc.seed_overlay("note") == {"author": "Seeded"}
    assert doc.seed_overlay("absent") is None
    # Same entry the `main` dict carries, read cheaply.
    assert doc.seed_overlay("note") == doc.main["seed"]["note"]


def test_store_ext_adds_map():
    """store_ext stores an opaque map readable via card['ext']."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"presentation": {"title": "Greeting"}})
    assert doc.main["ext"] == {"presentation": {"title": "Greeting"}}


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


def test_ext_round_trips_through_markdown():
    """$ext survives emit → re-parse."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"agent": {"pinned": True}})
    reparsed = Document.from_markdown(doc.to_markdown())
    assert reparsed.main["ext"]["agent"]["pinned"] is True


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


def test_remove_ext_returns_previous_and_clears():
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"agent": {"n": 1}})
    assert doc.remove_ext() == {"agent": {"n": 1}}
    assert doc.main["ext"] is None
    assert doc.remove_ext() is None


def test_card_ext_mutators():
    """store_ext / remove_ext with card=i target the composable card at index:
    the same verbs the main card uses, one `card=` selector over the whole axis."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.store_ext({"agent": {"note": "y"}}, card=0)
    assert doc.cards[0]["ext"] == {"agent": {"note": "y"}}
    assert doc.remove_ext(card=0) == {"agent": {"note": "y"}}
    assert doc.cards[0]["ext"] is None


def test_card_ext_mutators_out_of_range():
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with raises_edit_code("edit::index_out_of_range"):
        doc.store_ext({}, card=0)
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_ext(card=0)


def test_mutators_do_not_touch_warnings():
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.remove_field("title")
    doc.insert_card({"kind": "new_card"})
    doc.store_ext({"agent": {"n": 1}})
    assert list(doc.warnings) == initial


def test_a_mutation_sequence_keeps_its_order_and_its_invariants():
    """Every positional verb, read through the order it leaves behind: append
    lands at the end, `at=` before the card holding that index, `move_card`
    lifts and reinserts, `remove_card` closes the gap. The document stays
    internally consistent across the whole run."""
    import re

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

    # Payload mutation (quill-free removal), which answers with what it took.
    assert doc.remove_field("author") == "Alice"

    # Every surviving payload key passes the user-field regex.
    field_name_re = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
    for key in field_keys(doc.main):
        assert field_name_re.match(key), f"invalid key '{key}' found in payload"

    # Every card kind is lowercase-valid (just check non-empty and lowercase)
    for kind in kinds():
        assert kind and kind == kind.lower(), f"invalid kind '{kind}'"

    # Document identity preserved
    assert doc.quill_ref == "test_quill"


def test_to_markdown_general_round_trip():
    """A typed-writer mutation survives emit → re-parse with structure intact."""
    quill = taro_quill()
    doc = Document("taro@0.1.0")

    # Mutate: typed field + typed body + a quill-free card.
    quill.writer(doc).set("title", "New Title")
    doc.insert_card(make_card("note", {"author": "Alice"}, "Hello"))
    quill.writer(doc).revise_body("Updated body")

    # Emit
    emitted = doc.to_markdown()
    assert isinstance(emitted, str) and len(emitted) > 0

    # Re-parse and assert structure survives
    doc2 = Document.from_markdown(emitted)
    assert field(doc2.main, "title") == "New Title"
    assert doc2.body["text"].rstrip("\n") == "Updated body"
    assert len(doc2.cards) == 1
    assert doc2.cards[0]["kind"] == "note"
    assert field(doc2.cards[0], "author") == "Alice"
    assert doc2.cards[0]["body"]["text"] == "Hello"


def test_to_markdown_ambiguous_string_survival():
    """YAML-keyword string values survive emit → re-parse as strings.

    "on", "off", "yes", "no", "true", "false", "null" are all YAML
    booleans/null in permissive parsers. The emitter must double-quote them
    so they survive a re-parse as strings, not bools or null. Seated as card
    fields, whose values emit through the same card-yaml writer the main card
    uses.
    """
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(
        make_card(
            "note",
            {
                "flag_on": "on",
                "flag_off": "off",
                "flag_yes": "yes",
                "flag_no": "no",
                "str_true": "true",
                "str_false": "false",
                "str_null": "null",
                "octal_str": "01234",
                "date_str": "2024-01-15",
            },
        )
    )

    doc2 = Document.from_markdown(doc.to_markdown())
    card = doc2.cards[0]

    # Every value must survive as a string, not be re-interpreted
    assert field(card, "flag_on") == "on"
    assert field(card, "flag_off") == "off"
    assert field(card, "flag_yes") == "yes"
    assert field(card, "flag_no") == "no"
    assert field(card, "str_true") == "true"
    assert field(card, "str_false") == "false"
    assert field(card, "str_null") == "null"
    assert field(card, "octal_str") == "01234"
    assert field(card, "date_str") == "2024-01-15"
