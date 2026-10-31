"""The content model across the Python boundary: the write primitive `apply`,
its producers `revise` and `replace`, the reads, and the pure helpers over
changes. Offsets count code points."""

import pytest
from quillmark import Document, compose, diff, invert, map_pos

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
    with raises_edit_code("edit::stale_base"):
        doc.apply({"steps": []}, base=base)
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


def test_apply_all_writes_every_value_or_none():
    doc = Document.from_markdown(MD)
    held = doc.to_stored()
    intro = {"field": "intro", "change": {"steps": [{"splice": {"at": 0, "insert": "Hi"}}]}}
    with raises_edit_code("edit::stale_base"):
        doc.apply_all([intro, {"change": {"steps": []}, "base": "stale"}])
    assert doc.to_stored() == held
    receipts = doc.apply_all([intro, {"change": {"steps": [{"splice": {"at": 0, "delete": "h", "insert": "H"}}]}}])
    assert [r["hash"] for r in receipts] == [doc.hash_at("intro"), doc.hash_at()]
    assert (doc.read("intro")["text"], doc.read()["text"]) == ("Hi", "Hello world")


def test_compose_map_pos_and_diff_answer_without_a_document():
    a = {"steps": [{"splice": {"at": 5, "delete": "", "insert": "a"}}]}
    b = {"steps": [{"splice": {"at": 6, "delete": "", "insert": "b"}}]}
    assert compose(a, b) == {"steps": [{"splice": {"at": 5, "delete": "", "insert": "ab"}}]}
    assert (map_pos(a, 5, "before"), map_pos(a, 5, "after")) == (5, 6)
    assert map_pos({"steps": [{"splice": {"at": 0, "delete": "he", "insert": ""}}]}, 1, "after") is None
    with pytest.raises(ValueError):
        map_pos(a, 5, "left")

    doc = Document.from_markdown(MD)
    before = doc.read()
    doc2 = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\nhello brave world\n")
    doc.apply(diff(before, doc2.read()))
    assert doc.read()["text"] == "hello brave world"
