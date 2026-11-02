"""The content model across the Python boundary: the write primitive `apply`,
its producers `revise` and `replace`, the reads, and the pure helpers over
changes. Offsets count code points."""

import pytest
from quillmark import Document, diff, invert, repair

from conftest import raises_edit_code

MD = "~~~card-yaml\n$quill: test_quill\n~~~\n\nhello world\n"


def test_a_receipt_carries_the_normalized_change_which_invert_undoes():
    doc = Document.from_markdown(MD)
    before = doc.read()
    base = doc.hash_at()
    receipt = doc.apply({"steps": [{"splice": {"at": 0, "delete": "hello", "insert": "hi"}}]}, base=base)
    assert doc.read()["text"] == "hi world"
    assert receipt["hash"] == doc.hash_at()
    assert receipt["dropped_anchors"] == [] and receipt["warnings"] == []
    assert receipt["rebased"] is False
    assert doc.apply({"steps": []}, base=base)["rebased"] is True
    with raises_edit_code("edit::stale_base"):
        doc.apply({"steps": []}, base="never-read")
    doc.apply(invert(receipt["applied"], before), base=receipt["hash"])
    assert doc.read() == before


def test_a_refused_change_writes_nothing():
    doc = Document.from_markdown(MD)
    before = doc.read()
    change = {"steps": [{"mark": {"start": 0, "end": 5, "type": "strong", "add": True}}, {"splice": {"at": 99, "insert": "x"}}]}
    with raises_edit_code("edit::content_apply"):
        doc.apply(change)
    assert doc.read() == before
    with pytest.raises(ValueError):
        doc.apply({"steps": "nope"})
    with pytest.raises(ValueError, match="delet"):
        doc.apply({"steps": [{"splice": {"at": 0, "delet": "hello", "insert": "hi"}}]})
    with pytest.raises(ValueError, match="qoute"):
        doc.apply({"steps": [{"mark": {"start": 0, "end": 5, "type": "strong", "add": True, "qoute": "hello"}}]})


def test_a_quoted_step_refuses_text_its_writer_did_not_read():
    doc = Document.from_markdown(MD)
    doc.apply({"steps": [{"splice": {"at": 0, "insert": "XX "}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "a1", "start": 6, "end": 11, "quote": "world"}}]})
    receipt = doc.apply({"steps": [{"mark": {"start": 9, "end": 14, "type": "strong", "add": True}}]})
    assert receipt["applied"]["steps"][0]["mark"]["quote"] == "world"


def test_a_stale_base_rebases_over_the_change_since():
    doc = Document.from_markdown(MD)
    base = doc.hash_at()
    doc.apply({"steps": [{"splice": {"at": 0, "insert": "XX "}}]})
    receipt = doc.apply({"steps": [{"anchor": {"id": "a1", "start": 6, "end": 11, "quote": "world"}}]}, base=base)
    assert receipt["rebased"] is True
    assert doc.read()["anchors"] == [{"id": "a1", "start": 9, "end": 14}]
    doc.apply({"steps": [{"splice": {"at": 9, "delete": "w", "insert": "W"}}]})
    with raises_edit_code("edit::rebase_conflict") as err:
        doc.apply({"steps": [{"splice": {"at": 6, "delete": "world", "insert": "there"}}]}, base=base)
    assert err.value.diagnostics[0].args == {"current": doc.hash_at(), "step": 0, "reason": "deleted"}


def test_a_reserved_x_name_rides_as_written():
    doc = Document.from_markdown(MD)
    value = {"text": "Note", "blocks": [{"id": "b0", "kind": "x-callout", "attrs": {"tone": "warm"}}], "marks": [{"start": 0, "end": 4, "type": "x-glow"}]}
    doc.replace(value)
    assert doc.read() == value
    with pytest.raises(Exception, match="callout"):
        doc.replace({**value, "blocks": [{"id": "b0", "kind": "callout"}]})


def test_offsets_count_code_points():
    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\n😀 **bold**\n")
    assert doc.read()["marks"] == [{"start": 2, "end": 6, "type": "strong"}]
    doc.apply({"steps": [{"splice": {"at": 1, "delete": "", "insert": "!"}}]})
    assert doc.read()["text"] == "😀! bold"


def test_revise_and_replace_name_the_anchors_they_drop():
    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\nThe committee met on Monday to plan.\n")
    doc.apply({"steps": [{"anchor": {"id": "m1", "start": 14, "end": 27}}]})
    assert doc.annotated() == 'The committee <qm-anchor ref="m1"></qm-anchor>met on Monday to plan.'
    receipt = doc.revise("The board met on Monday to plan.")
    assert receipt["dropped_anchors"] == []
    assert doc.read()["anchors"] == [{"id": "m1", "start": 10, "end": 23}]

    other = doc.clone()
    receipt = other.replace({"text": "Nothing.", "blocks": [{"id": "b0", "kind": "para"}]})
    assert receipt["dropped_anchors"] == [
        {"id": "m1", "path": "main.body", "card_id": None, "quote": "met on Monday", "prefix": "The board ", "suffix": " to plan."}
    ]
    assert "anchors" not in other.read()


def test_diff_answers_without_a_document():
    doc = Document.from_markdown(MD)
    before = doc.read()
    doc2 = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\nhello brave world\n")
    doc.apply(diff(before, doc2.read()))
    assert doc.read()["text"] == "hello brave world"


def test_a_content_write_names_its_card_by_id_wherever_it_moved():
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: test_quill\n~~~\n\n~~~card-yaml\n$kind: note\n~~~\n\nHello\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n\nHello\n"
    )
    card = doc.cards[1]["id"]
    base = doc.hash_at(card=card)
    doc.move_card(1, 0)
    with pytest.raises(ValueError):
        doc.apply({"steps": []}, card=1, base=base)
    with raises_edit_code("edit::unknown_card_id"):
        doc.apply({"steps": []}, card="nobody")
    doc.apply({"steps": [{"splice": {"at": 5, "insert": " world"}}]}, card=card, base=base)
    assert [(c["id"] == card, doc.read(card=c["id"])["text"]) for c in doc.cards] == [(True, "Hello world"), (False, "Hello")]


def test_deleting_an_island_names_the_anchors_of_its_slots():
    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\nLead.\n\n| a |\n|---|\n| cell text |\n")
    island = doc.read()["islands"][0]
    cell = island["props"]["rows"][1]["cells"][island["props"]["columns"][0]]
    anchor = {"anchor": {"id": "c1", "start": 5, "end": 9}}
    doc.apply({"steps": [{"nested": {"island": island["id"], "slot": cell, "change": {"steps": [anchor]}}}]})
    receipt = doc.apply({"steps": [{"splice": {"at": 6, "delete": "\ufffc", "insert": ""}}]})
    assert receipt["dropped_anchors"] == [
        {"id": "c1", "path": "main.body", "card_id": None, "quote": "text", "prefix": "cell ", "suffix": "", "island": island["id"], "slot": cell}
    ]


def test_an_anchor_placement_refuses_a_held_id_and_a_move_moves_it():
    doc = Document.from_markdown(MD)
    doc.apply({"steps": [{"anchor": {"id": "c1", "start": 0, "end": 5}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "c1", "start": 6, "end": 11}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "c9", "start": 6, "end": 11, "move": True}}]})
    receipt = doc.apply({"steps": [{"anchor": {"id": "c1", "start": 6, "end": 11, "move": True}}]})
    assert receipt["applied"]["steps"][0]["anchor"]["move"] is True
    assert doc.read()["anchors"] == [{"id": "c1", "start": 6, "end": 11}]


def test_repair_answers_the_change_from_the_merged_text():
    merged = {"text": "a\rb", "blocks": [{"id": "b0", "kind": "para"}], "anchors": [{"id": "p", "start": 2, "end": 2}]}
    read = repair(merged)
    assert read["content"]["text"] == "ab"
    assert read["change"] == {"steps": [{"splice": {"at": 1, "delete": "\r", "insert": ""}}]}
    assert read["content"]["anchors"] == [{"id": "p", "start": 1, "end": 1}]
    valid = {"text": "ab", "blocks": [{"id": "b0", "kind": "para"}]}
    assert repair(valid) == {"content": valid, "repairs": [], "change": {"steps": []}}


def test_a_receipt_names_each_anchor_a_write_moved_and_what_moved():
    doc = Document.from_markdown(MD)
    doc.apply({"steps": [{"anchor": {"id": "p", "start": 0, "end": 0}}, {"anchor": {"id": "r", "start": 6, "end": 11}}]})
    receipt = doc.apply({"steps": [
        {"anchor": {"id": "p", "start": 5, "end": 5, "move": True}},
        {"splice": {"at": 5, "delete": " ", "insert": "\n", "blocks": [{"id": "b9", "kind": "para"}]}},
    ]})
    assert receipt["moved_anchors"] == [
        {"id": "p", "before": "", "after": "", "reason": "position"},
        {"id": "r", "before": "world", "after": "world", "reason": "block"},
    ]
