"""Tests for the API requirements.

Python is a Tier-1 binding: field I/O flows through `quill.writer(doc)` /
`quill.reader(doc)`. `Document` carries the quill-free surface: parse, storage,
structure, `$ext` / `$seed`, and `remove_field`. There is no opaque field store
and no content lane (`overwrite` / `revise` / `apply_change` + codec); those are
WASM-only by scope.
"""

import datetime

import pytest
from quillmark import (
    Quillmark,
    Quill,
    Document,
    OutputFormat,
    QuillmarkError,
)
from conftest import QUILLS_PATH, _latest_version, raises_edit_code


def field(card, key):
    """Return the value of a named field from a card's payload_items list."""
    for item in card["payload_items"]:
        if item["type"] == "field" and item["key"] == key:
            return item["value"]
    return None


def has_field(card, key):
    """True when a named field exists in a card's payload_items."""
    return any(
        i["type"] == "field" and i["key"] == key for i in card["payload_items"]
    )


def field_keys(card):
    """Iterable of all field keys in a card, in source order."""
    return [i["key"] for i in card["payload_items"] if i["type"] == "field"]


def _richtext_form_quill():
    """The richtext_form fixture quill (headline: richtext inline, bio: richtext)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "richtext_form")))


def _taro_quill():
    """The taro fixture quill (main string fields; a `quotes` card kind)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "taro")))


def test_parsed_document_quill_ref():
    markdown_with_quill = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Test\n~~~\n\n# Content\n"
    parsed = Document.from_markdown(markdown_with_quill)
    assert parsed.quill_ref == "my_quill"

    markdown_without_quill = "# Just content\n\nNo card-yaml block here.\n"
    with pytest.raises(QuillmarkError):
        Document.from_markdown(markdown_without_quill)


def test_quill_properties(engine, taro_quill_dir):
    """Quill exposes its engine-free config surface; capability lives on the
    engine, not the quill."""
    quill = Quill.from_path(str(taro_quill_dir))

    metadata = quill.metadata
    assert isinstance(metadata, dict)
    assert metadata["name"] == "taro"
    # metadata is a pure config snapshot: no capability key baked in.
    assert "supportedFormats" not in metadata
    assert quill.backend_id == "typst"
    assert isinstance(quill.blueprint, str) and quill.blueprint != ""

    schema = quill.schema
    assert isinstance(schema, dict)
    assert "main" in schema
    assert "fields" in schema["main"]

    # Capability is resolved by the engine, against the quill.
    supported_formats = engine.supported_formats(quill)
    assert isinstance(supported_formats, list)
    assert OutputFormat.PDF in supported_formats


def test_registered_backends(engine):
    """The engine's backend roster: which backends this build compiled in, as
    opposed to which formats a given quill supports (`supported_formats`)."""
    backends = engine.registered_backends()
    assert isinstance(backends, list)
    assert all(isinstance(b, str) for b in backends)
    # The published wheel builds both backends in; order is not guaranteed.
    assert "typst" in backends


def test_full_workflow(engine):
    taro_dir = QUILLS_PATH / "taro"
    quill = Quill.from_path(str(_latest_version(taro_dir)))

    markdown = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test Author\nice_cream: Chocolate\ntitle: Test\n~~~\n\nContent.\n"
    parsed = Document.from_markdown(markdown)
    assert parsed.quill_ref == "taro"

    assert "taro" in quill.quill_ref
    assert quill.backend_id == "typst"
    assert OutputFormat.PDF in engine.supported_formats(quill)

    result = engine.render(quill, parsed, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].format == OutputFormat.PDF
    assert len(result.artifacts[0].bytes) > 0


def test_blank_document_constructor():
    """Document(quill_ref) starts blank: main card only, no fields, no cards. A
    field write needs a quill (the writer); the blank canvas itself is quill-free."""
    doc = Document("taro@0.1.0")
    assert doc.quill_ref == "taro@0.1.0"
    assert doc.card_count == 0
    assert doc.body["text"] == ""
    assert field_keys(doc.main) == []

    _taro_quill().writer(doc).set("title", "Hello")
    assert field(doc.main, "title") == "Hello"

    with pytest.raises(ValueError, match="QuillReference"):
        Document("not a valid ref!!")


def test_blank_document_renders(engine):
    """The programmatic flow end-to-end: blank canvas → typed writer → render."""
    quill = _taro_quill()

    doc = Document("taro@0.1.0")
    w = quill.writer(doc)
    w.set_all({"title": "Test", "author": "Test Author", "ice_cream": "Chocolate"})
    w.revise_body("Content.")

    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert len(result.artifacts[0].bytes) > 0


# ---------------------------------------------------------------------------
# Document surface: quill-free structure, removal, and $ext
# ---------------------------------------------------------------------------

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


def test_remove_field_existing():
    """remove_field removes and returns an existing main-card field."""
    doc = Document.from_markdown(SIMPLE_MD)
    val = doc.remove_field("title")
    assert val == "Hello"
    assert not has_field(doc.main, "title")


def test_insert_card_appends():
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card({"kind": "note", "body": "Card body."})
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert doc.cards[0]["body"]["text"] == "Card body."


def test_insert_card_invalid_kind():
    doc = Document.from_markdown(SIMPLE_MD)
    with raises_edit_code("edit::invalid_kind_name"):
        doc.insert_card({"kind": "BadKind"})


def test_remove_card_then_insert_card_round_trips_fields():
    """A card returned by remove_card feeds straight back into insert_card with
    its fields intact: the one-Card-shape contract. Exercises the explicit
    quill/ext=None keys the dict carries against `deny_unknown_fields`."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(Document.make_card("note", {"author": "Alice"}, "Body"))

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


def test_make_card_accepts_any_kind_insert_card_is_the_gate():
    """make_card accepts any kind string; insert_card is the gate.

    Not blanket permissiveness: make_card rejects a malformed field name or an
    over-deep value. The line is what a detached card can decide alone. Kind
    validity is positional, so only insert_card can rule on it, and it is the
    one reporting `edit::invalid_kind_name`."""
    card = Document.make_card("BadKind", {"x": 1})
    assert card["kind"] == "BadKind"  # construction succeeds
    doc = Document.from_markdown(SIMPLE_MD)
    with raises_edit_code("edit::invalid_kind_name"):
        doc.insert_card(card)


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
    with raises_edit_code("edit::invalid_field_name"):
        Document.make_card("note", {"bad-name": 1})


def test_stale_flat_input_is_a_loud_error():
    """A stale {kind, fields} dict fails loudly rather than yielding an empty
    card: `deny_unknown_fields` on the wire type rejects the unknown `fields`
    key at deserialize time (a ValueError), before any edit."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="fields"):
        doc.insert_card({"kind": "note", "fields": {"x": 1}})


def test_insert_card_at_front():
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.insert_card({"kind": "intro"}, at=0)
    assert doc.cards[0]["kind"] == "intro"
    assert doc.cards[1]["kind"] == "note"


def test_insert_card_out_of_range():
    """insert_card raises EditError when at > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with raises_edit_code("edit::index_out_of_range"):
        doc.insert_card({"kind": "note"}, at=5)


def test_remove_card():
    """remove_card removes and returns the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    removed = doc.remove_card(0)
    assert removed is not None
    assert removed["kind"] == "note"
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "summary"


def test_move_card_last_to_first():
    doc = Document.from_markdown(MD_WITH_CARDS)
    last = len(doc.cards) - 1
    doc.move_card(last, 0)
    assert doc.cards[0]["kind"] == "summary"
    assert doc.cards[1]["kind"] == "note"


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


def test_store_ext_namespace_preserves_siblings():
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext_namespace("presentation", {"title": "A"})
    doc.store_ext_namespace("agent", {"pinned": True})
    doc.store_ext_namespace("presentation", {"title": "B"})
    assert doc.main["ext"] == {
        "presentation": {"title": "B"},
        "agent": {"pinned": True},
    }


def test_remove_ext_namespace_clears_one_slot_and_drops_when_empty():
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext_namespace("presentation", {"title": "A"})
    doc.store_ext_namespace("tutorial", ["step-1", "step-2"])
    # Returns the removed value; siblings survive.
    assert doc.remove_ext_namespace("tutorial") == ["step-1", "step-2"]
    assert doc.main["ext"] == {"presentation": {"title": "A"}}
    # Removing the last namespace clears $ext entirely.
    doc.remove_ext_namespace("presentation")
    assert doc.main["ext"] is None
    # Absent namespace is a no-op returning None.
    assert doc.remove_ext_namespace("nope") is None


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


def test_card_ext_namespace_mutators():
    """store/remove_ext_namespace with card=i preserve siblings and clear when empty."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.store_ext_namespace("presentation", {"title": "A"}, card=0)
    doc.store_ext_namespace("tutorial", ["step-1"], card=0)
    assert doc.remove_ext_namespace("tutorial", card=0) == ["step-1"]
    assert doc.cards[0]["ext"] == {"presentation": {"title": "A"}}
    doc.remove_ext_namespace("presentation", card=0)
    assert doc.cards[0]["ext"] is None


def test_card_ext_mutators_out_of_range():
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with raises_edit_code("edit::index_out_of_range"):
        doc.store_ext({}, card=0)
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_ext(card=0)
    with raises_edit_code("edit::index_out_of_range"):
        doc.store_ext_namespace("a", {}, card=0)
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_ext_namespace("a", card=0)


def test_mutators_do_not_touch_warnings():
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.remove_field("title")
    doc.insert_card({"kind": "new_card"})
    doc.store_ext_namespace("agent", {"n": 1})
    assert list(doc.warnings) == initial


def test_invariants_after_mutation_sequence():
    """After a sequence of mutations the document must be internally consistent."""
    import re

    doc = Document.from_markdown(SIMPLE_MD)

    # Add and manipulate cards
    doc.insert_card(Document.make_card("note", {"text": "hi"}))
    doc.insert_card({"kind": "summary"})
    doc.insert_card({"kind": "appendix"})
    doc.insert_card({"kind": "intro"}, at=1)  # note, intro, summary, appendix
    doc.move_card(3, 0)                        # appendix, note, intro, summary
    doc.remove_card(2)                         # appendix, note, summary

    # Mutate payload (quill-free removal)
    doc.remove_field("author")

    # Assertions: every payload key passes the user-field regex.
    field_name_re = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
    for key in field_keys(doc.main):
        assert field_name_re.match(key), f"invalid key '{key}' found in payload"

    # Every card kind is lowercase-valid (just check non-empty and lowercase)
    for card in doc.cards:
        kind = card["kind"]
        assert kind and kind == kind.lower(), f"invalid kind '{kind}'"

    # Document identity preserved
    assert doc.quill_ref == "test_quill"


# ---------------------------------------------------------------------------
# Emitter integration tests (fromMarkdown → mutate → emit → re-parse)
# ---------------------------------------------------------------------------


def test_to_markdown_general_round_trip():
    """A typed-writer mutation survives emit → re-parse with structure intact."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")

    # Mutate: typed field + typed body + a quill-free card.
    quill.writer(doc).set("title", "New Title")
    doc.insert_card(Document.make_card("note", {"author": "Alice"}, "Hello"))
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
    fields via the quill-free `make_card`, whose values emit through the same
    card-yaml writer the main card uses.
    """
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(
        Document.make_card(
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


# ── Tier-1 typed writer: quill.writer(doc) front door ────────────────────────


def test_writer_front_door_set_and_reads():
    """quill.writer(doc).set writes; the reads live on quill.reader(doc)."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.set("author", "Ada")
    ed.set_all({"title": "On Taro"})
    assert ed.document is doc  # holds the same object, mutated in place

    v = quill.reader(doc)
    assert v.get("author") == "Ada"
    assert v.get("ice_cream") is None  # declared but absent → None
    ed.revise_body("A **taro** essay.")
    assert v.body_markdown() == "A **taro** essay."


def test_writer_set_rejects_unknown_field():
    """An undeclared name is a typo on the typed path: it raises, nothing lands."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    with raises_edit_code("edit::unknown_field"):
        quill.writer(doc).set("stray", "x")
    assert not has_field(doc.main, "stray")


def test_writer_set_all_reports_every_unknown_field():
    """set_all is all-or-nothing and reports one diagnostic per undeclared name
    (path = field name): externally-sourced keys surface every violation at once."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    with raises_edit_code("edit::unknown_field") as exc_info:
        quill.writer(doc).set_all({"title": "ok", "stray1": "x", "stray2": "y"})
    paths = [d.path for d in exc_info.value.diagnostics]
    assert "stray1" in paths and "stray2" in paths
    # All-or-nothing: even the valid `title` did not land.
    assert not has_field(doc.main, "title")


def test_writer_add_card_transactional():
    """add_card fuses make + typed commit + insert; a typo leaves the doc untouched."""
    quill = _taro_quill()
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
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "First"})
    ed.add_card("quotes", {"author": "Second"}, at=0)  # insert at the front
    assert field(doc.cards[0], "author") == "Second"
    assert field(doc.cards[1], "author") == "First"
    with raises_edit_code("edit::index_out_of_range"):
        ed.add_card("quotes", {"author": "x"}, at=99)
    assert len(doc.cards) == 2  # the out-of-range insert landed nothing


def test_writer_card_cursor_set_and_body():
    """writer.card(i) targets the composable card; a bad index raises at the write."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"})
    ed.card(0).set("author", "Issa")
    assert field(doc.cards[0], "author") == "Issa"
    with raises_edit_code("edit::index_out_of_range"):
        ed.card(9).set("author", "x")


def test_writer_card_kind_getter():
    """writer.card(i).kind reads the bound card's $kind; a bad index raises."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"})
    assert ed.card(0).index == 0
    assert ed.card(0).kind == "quotes"
    with raises_edit_code("edit::index_out_of_range"):
        _ = ed.card(9).kind


def test_writer_set_coerces_richtext_to_content():
    """A richtext field commits the canonical content, not the authored markdown."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).set("bio", "A **bold** intro.")
    value = field(doc.main, "bio")
    assert isinstance(value, dict)  # stored as the content dict, not a string
    assert value["text"] == "A bold intro."


def test_writer_set_rejects_inline_violation():
    """A richtext(inline) field rejects multi-block content at the write."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).set("headline", "line one\n\nline two")


def test_writer_set_all_is_all_or_nothing():
    """A mid-batch inline violation aborts set_all: nothing lingers."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).set_all({"bio": "ok", "headline": "line one\n\nline two"})
    assert not has_field(doc.main, "bio")


def test_writer_revise_field_typed_and_anchor_preserving():
    """writer.revise_field is the typed, anchor-preserving richtext field write:
    diff-imports the markdown and schema-conforms the result."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).revise_field("bio", "make it **bold**")
    assert quill.reader(doc).get("bio") == "make it **bold**"


def test_writer_revise_field_rejects_inline_and_unknown():
    """revise_field conforms to the field schema (inline rejects multi-block) and
    rejects an undeclared name: same guards as `set`."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with raises_edit_code("edit::field_not_inline"):
        quill.writer(doc).revise_field("headline", "line one\n\nline two")
    with raises_edit_code("edit::unknown_field"):
        quill.writer(doc).revise_field("nope", "x")


def test_typed_set_clears_must_fill_marker():
    """With the opaque store gone, seed → validate(must_fill) → typed `set` is the
    fill lifecycle: the typed commit lands a real value and clears the marker."""
    quill = _taro_quill()
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


# ── Tier-1 typed reader: quill.reader(doc) front door ──────────────────────────


def test_view_interprets_by_declared_type():
    """view.get reads a richtext field as markdown and a scalar as its canonical value."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).set("bio", "A **bold** intro.")
    v = quill.reader(doc)
    assert v.document is doc  # holds the same object
    assert v.get("bio") == "A **bold** intro."  # richtext → markdown

    taro = _taro_quill()
    tdoc = Document("taro@0.1.0")
    taro.writer(tdoc).set("author", "Ada")
    assert taro.reader(tdoc).get("author") == "Ada"  # scalar → canonical


def test_view_absence_returns_none_unknown_name_raises():
    """Absent → None; a name the schema does not declare raises (the schema authority)."""
    quill = _richtext_form_quill()
    v = quill.reader(Document("richtext_form@0.1.0"))
    assert v.get("bio") is None  # absent, not a typo
    with raises_edit_code("edit::unknown_field"):
        v.get("nope")  # typo, not absent


def test_view_richtext_holding_scalar_raises_mismatch():
    """A present value that does not decode as richtext raises FieldDecode.

    Seated quill-free via `from_markdown` (a bare number under a richtext field),
    since the opaque store is gone."""
    quill = _richtext_form_quill()
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
    quill = _richtext_form_quill()
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
    quill = _richtext_form_quill()
    assert quill.reader(Document("richtext_form@0.1.0")).get_content("bio") is None
    with raises_edit_code("edit::unknown_field"):
        quill.reader(Document("richtext_form@0.1.0")).get_content("nope")

    taro = _taro_quill()
    tdoc = Document("taro@0.1.0")
    taro.writer(tdoc).set("author", "Ada")
    # A declared type carrying no content answers from the schema, not the
    # payload: `author` holds a string and is still not content.
    with raises_edit_code("edit::field_not_content"):
        taro.reader(tdoc).get_content("author")


# ── Tier-1 typed reader: the nested content read ───────────────────────────────

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
    letterhead:
      type: object
      properties:
        motto:
          type: richtext
        code:
          type: string
    rows:
      type: array
      items:
        type: object
        properties:
          notes:
            type: richtext
"""


@pytest.fixture
def element_quill(tmp_path):
    """A quill declaring every content-bearing composite shape."""
    root = tmp_path / "element_test" / "0.1.0"
    root.mkdir(parents=True)
    (root / "Quill.yaml").write_text(ELEMENT_QUILL_YAML)
    (root / "plate.typ").write_text('#import "@local/quillmark-helper:0.1.0": data\n')
    return Quill.from_path(str(root))


def test_view_get_content_at_spans_both_storage_forms(element_quill):
    """An element reads back the same Content whatever its resting form.

    The bound door rests a plaintext element as its literal string and a richtext
    one as the canonical content object; the transport door leaves both as
    authored strings."""
    md = (
        "~~~card-yaml\n$quill: element_test@0.1.0\n$kind: main\n"
        "recipients: ['a *literal* line']\nparagraphs: ['A **bold** intro.']\n~~~\n"
    )
    parsed = Document.from_markdown(md)
    assert isinstance(field(parsed.main, "paragraphs")[0], str)

    bound = element_quill.parse(md)
    assert field(bound.main, "recipients")[0] == "a *literal* line"
    assert isinstance(field(bound.main, "paragraphs")[0], dict)

    for name, text in [("recipients", "a *literal* line"), ("paragraphs", "A bold intro.")]:
        a = element_quill.reader(parsed).get_content_at(name, [0])
        b = element_quill.reader(bound).get_content_at(name, [0])
        assert a["text"] == text  # decoded at the element's declared codec
        assert b["text"] == a["text"]
        assert b["marks"] == a["marks"]


def test_view_get_content_at_reaches_object_and_nested_leaves(element_quill):
    """The path is the model's own in-field axis, so an object property and a
    leaf under both array and object answer with the element."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: element_test@0.1.0\n$kind: main\n"
        "letterhead:\n  motto: Fly **fight**\n  code: '9'\n"
        "rows:\n  - {}\n  - notes: a *note*\n~~~\n"
    )
    v = element_quill.reader(doc)
    assert v.get_content_at("letterhead", ["motto"])["text"] == "Fly fight"
    assert v.get_content_at("rows", [1, "notes"])["text"] == "a note"
    assert v.get_content_at("rows", [0, "notes"]) is None  # declared, unstored


def test_view_get_content_at_stale_index_and_no_content_leaf(element_quill):
    """A stale row index reads absent; a path resolving to no content leaf raises."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: element_test@0.1.0\n$kind: main\n"
        "recipients: ['a']\ntags: ['x']\n~~~\n"
    )
    v = element_quill.reader(doc)
    assert v.get_content_at("recipients", [7]) is None
    assert v.get_content_at("paragraphs", [0]) is None  # field absent

    with raises_edit_code("edit::field_not_content"):
        v.get_content_at("tags", [0])  # `declared` names the element's type
    with raises_edit_code("edit::field_not_content"):
        v.get_content_at("recipients", [])  # the array itself has no one Content
    with raises_edit_code("edit::unknown_field"):
        v.get_content_at("letterhead", ["nope"])
    with pytest.raises(ValueError, match=r"path\[0\]"):
        v.get_content_at("recipients", [None])


def test_view_get_content_at_names_the_element_in_a_decode_failure(element_quill):
    """A failing element names itself, rather than reporting against the field.

    `convert_edit_error` mints no `path` on this surface for any edit error, so
    the message is where the element shows up here."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: element_test@0.1.0\n$kind: main\nparagraphs: ['ok', 3]\n~~~\n"
    )
    with pytest.raises(QuillmarkError) as excinfo:
        element_quill.reader(doc).get_content_at("paragraphs", [1])
    diag = excinfo.value.diagnostics[0]
    assert diag.code == "edit::field_decode"
    assert "paragraphs[1]" in diag.message


def test_view_body_read_is_quill_free():
    """view.body_markdown reads the main body markdown: the quill-free body read."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    quill.writer(doc).revise_body("A **taro** essay.")
    assert quill.reader(doc).body_markdown() == "A **taro** essay."


def test_view_card_cursor_reads_through_kind_schema():
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"}, "A quote body.")
    v = quill.reader(doc)
    assert v.card(0).kind == "quotes"
    assert v.card(0).get("author") == "Basho"
    assert v.card(0).body_markdown() == "A quote body."
    with raises_edit_code("edit::unknown_field"):
        v.card(0).get("stray")
    with raises_edit_code("edit::index_out_of_range"):
        v.card(9).get("author")
