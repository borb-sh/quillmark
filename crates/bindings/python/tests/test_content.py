"""The content model across the Python boundary: the write primitive `apply`,
its producers `revise` and `replace`, the reads, and the pure helpers over
changes. Offsets count code points."""

import pytest
from quillmark import Document, diff, export_markdown, import_markdown, is_inline, is_plain, normalize, repair

from conftest import by_place, raises_edit_code

MD = "~~~card-yaml\n$quill: test_quill\n~~~\n\nhello world\n"


def test_a_receipt_carries_its_patches_and_undo_takes_the_write_back():
    doc = Document.parse(MD)
    before = doc.read()
    heads = doc.heads()
    receipt = doc.apply({"steps": [{"splice": {"at": 0, "delete": "hello", "insert": "hi"}}]}, heads=heads)
    assert doc.read()["text"] == "hi world"
    assert (receipt["before"], receipt["heads"]) == (heads, doc.heads())
    assert receipt["patches"] == [
        {"action": "splice", "path": ["main", "body", "text", 1], "value": "hi"},
        {"action": "del", "path": ["main", "body", "text", 3], "length": 5},
    ]
    assert receipt["dropped_anchors"] == [] and receipt["warnings"] == []
    doc.apply({"steps": []}, heads=heads)
    foreign = Document.parse(MD)
    foreign.apply({"steps": [{"splice": {"at": 0, "insert": "elsewhere "}}]})
    with raises_edit_code("edit::stale_base"):
        doc.apply({"steps": []}, heads=foreign.heads())
    with pytest.raises(ValueError):
        doc.apply({"steps": []}, heads=["never-read"])
    doc.undo(receipt)
    assert doc.read() == before


def test_a_refused_change_writes_nothing():
    doc = Document.parse(MD)
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
    doc = Document.parse(MD)
    doc.apply({"steps": [{"splice": {"at": 0, "insert": "XX "}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "a1", "start": 6, "end": 11, "quote": "world"}}]})
    receipt = doc.apply({"steps": [{"mark": {"start": 9, "end": 14, "type": "strong", "add": True}}]})
    assert receipt["patches"] == [{"action": "mark", "path": ["main", "body", "text"], "marks": [{"name": "strong", "value": True, "start": 10, "end": 15}]}]


def test_a_write_at_stale_heads_merges_with_the_change_since():
    doc = Document.parse(MD)
    heads = doc.heads()
    doc.apply({"steps": [{"splice": {"at": 0, "insert": "XX "}}]})
    doc.apply({"steps": [{"anchor": {"id": "a1", "start": 6, "end": 11, "quote": "world"}}]}, heads=heads)
    assert doc.read()["anchors"] == [{"id": "a1", "start": 9, "end": 14}]
    doc.apply({"steps": [{"splice": {"at": 9, "delete": "w", "insert": "W"}}]})
    doc.apply({"steps": [{"splice": {"at": 6, "delete": "world", "insert": "there"}}]}, heads=heads)
    assert doc.read()["text"] in ("XX hello thereW", "XX hello Wthere")

    block = doc.read()["blocks"][0]["id"]

    def heading(level):
        return {"steps": [{"setBlock": {"id": block, "kind": "heading", "attrs": {"level": level}}}]}

    read = doc.heads()
    doc.apply(heading(2))
    doc.apply(heading(3), heads=read)
    assert doc.read()["blocks"][0]["attrs"] == {"level": 3}


def test_a_placeholder_is_minted_under_the_writers_actor():
    doc = Document.parse(MD)
    change = {"steps": [{"splice": {"at": 5, "insert": "\nnew", "blocks": [{"id": "?n", "kind": "para"}]}}]}
    doc.set_actor("ann")
    assert doc.actor == "ann"
    doc.apply(change)
    minted = doc.read()["blocks"][1]["id"]
    assert minted.endswith("@" + "ann".encode().hex())
    with raises_edit_code("edit::invalid_actor"):
        doc.set_actor("a:b")
    assert doc.actor == "ann"


def test_a_reserved_x_name_rides_as_written():
    doc = Document.parse(MD)
    value = {"text": "Note", "blocks": [{"id": "b0", "kind": "x-callout", "attrs": {"tone": "warm"}}], "marks": [{"start": 0, "end": 4, "type": "x-glow"}]}
    doc.replace(value)
    assert by_place(doc.read()) == by_place(value)
    with pytest.raises(Exception, match="callout"):
        doc.replace({**value, "blocks": [{"id": "b0", "kind": "callout"}]})


def test_offsets_count_code_points():
    doc = Document.parse("~~~card-yaml\n$quill: test_quill\n~~~\n\n😀 **bold**\n")
    assert doc.read()["marks"] == [{"start": 2, "end": 6, "type": "strong"}]
    doc.apply({"steps": [{"splice": {"at": 1, "delete": "", "insert": "!"}}]})
    assert doc.read()["text"] == "😀! bold"


def test_revise_and_replace_name_the_anchors_they_drop():
    doc = Document.parse("~~~card-yaml\n$quill: test_quill\n~~~\n\nThe committee met on Monday to plan.\n")
    doc.apply({"steps": [{"anchor": {"id": "m1", "start": 14, "end": 27}}]})
    assert doc.to_markdown_at(annotated=True)["markdown"] == 'The committee <qm-anchor ref="m1"></qm-anchor>met on Monday to plan.'
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
    doc = Document.parse(MD)
    before = doc.read()
    doc2 = Document.parse("~~~card-yaml\n$quill: test_quill\n~~~\n\nhello brave world\n")
    doc.apply(diff(before, doc2.read()))
    assert doc.read()["text"] == "hello brave world"


def test_a_content_write_names_its_card_by_id_wherever_it_moved():
    doc = Document.parse(
        "~~~card-yaml\n$quill: test_quill\n~~~\n\n~~~card-yaml\n$kind: note\n~~~\n\nHello\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n\nHello\n"
    )
    card = doc.cards[1]["id"]
    heads = doc.heads()
    doc.move_card(1, 0)
    with pytest.raises(ValueError):
        doc.apply({"steps": []}, card=1, heads=heads)
    with raises_edit_code("edit::unknown_card_id"):
        doc.apply({"steps": []}, card="nobody")
    doc.apply({"steps": [{"splice": {"at": 5, "insert": " world"}}]}, card=card, heads=heads)
    assert [(c["id"] == card, doc.read(card=c["id"])["text"]) for c in doc.cards] == [(True, "Hello world"), (False, "Hello")]


def test_deleting_an_island_names_the_anchors_of_its_slots():
    doc = Document.parse("~~~card-yaml\n$quill: test_quill\n~~~\n\nLead.\n\n| a |\n|---|\n| cell text |\n")
    island = doc.read()["islands"][0]
    cell = island["props"]["rows"][1]["cells"][island["props"]["columns"][0]]
    anchor = {"anchor": {"id": "c1", "start": 5, "end": 9}}
    doc.apply({"steps": [{"nested": {"island": island["id"], "slot": cell, "change": {"steps": [anchor]}}}]})
    receipt = doc.apply({"steps": [{"splice": {"at": 6, "delete": "\ufffc", "insert": ""}}]})
    assert receipt["dropped_anchors"] == [
        {"id": "c1", "path": "main.body", "card_id": None, "quote": "text", "prefix": "cell ", "suffix": "", "island": island["id"], "slot": cell}
    ]


def test_an_anchor_placement_refuses_a_held_id_and_a_move_moves_it():
    doc = Document.parse(MD)
    doc.apply({"steps": [{"anchor": {"id": "c1", "start": 0, "end": 5}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "c1", "start": 6, "end": 11}}]})
    with raises_edit_code("edit::content_apply"):
        doc.apply({"steps": [{"anchor": {"id": "c9", "start": 6, "end": 11, "move": True}}]})
    receipt = doc.apply({"steps": [{"anchor": {"id": "c1", "start": 6, "end": 11, "move": True}}]})
    assert receipt["patches"] == [{"action": "mark", "path": ["main", "body", "text"], "marks": [
        {"name": "anchor:c1", "value": None, "start": 1, "end": 6},
        {"name": "anchor:c1", "value": True, "start": 7, "end": 12},
    ]}]
    assert doc.read()["anchors"] == [{"id": "c1", "start": 6, "end": 11}]


def test_repair_answers_the_change_from_the_merged_text():
    merged = {"text": "a\rb", "blocks": [{"id": "b0", "kind": "para"}], "anchors": [{"id": "p", "start": 2, "end": 2}]}
    read = repair(merged)
    assert read["content"]["text"] == "ab"
    assert read["change"] == {"steps": [{"splice": {"at": 1, "delete": "\r", "insert": ""}}]}
    assert read["content"]["anchors"] == [{"id": "p", "start": 1, "end": 1}]
    valid = {"text": "ab", "blocks": [{"id": "b0", "kind": "para"}]}
    assert repair(valid) == {"content": valid, "repairs": [], "change": {"steps": []}}


def test_a_receipt_carries_the_block_a_write_put_and_the_point_anchor_it_moved():
    doc = Document.parse(MD)
    doc.apply({"steps": [{"anchor": {"id": "p", "start": 0, "end": 0}}, {"anchor": {"id": "r", "start": 6, "end": 11}}]})
    receipt = doc.apply({"steps": [
        {"anchor": {"id": "p", "start": 5, "end": 5, "move": True}},
        {"splice": {"at": 5, "delete": " ", "insert": "\n", "blocks": [{"id": "b9", "kind": "para"}]}},
    ]})
    paths = [p["path"][-2:] for p in receipt["patches"]]
    assert ["points", "p"] in paths and ["text", 6] in paths
    assert doc.read()["anchors"] == [{"id": "p", "start": 6, "end": 6}, {"id": "r", "start": 6, "end": 11}]


def test_the_markdown_import_and_export_and_the_predicates_hold_no_document():
    imported = import_markdown("Hello *world*.\n\n- one")
    content = imported["content"]
    assert imported["warnings"] == []
    assert export_markdown(content) == "Hello *world*.\n\n- one"
    assert not is_inline(content) and not is_plain(content)
    assert is_inline(import_markdown("Hello.")["content"])
    assert normalize(content) == content
    doc = Document.parse(MD)
    doc.apply({"steps": [{"anchor": {"id": "a", "start": 0, "end": 5}}]})
    read = export_markdown(doc.read(), annotated=True)
    assert read == {"markdown": '<qm-anchor ref="a"></qm-anchor>hello world', "anchors": [{"id": "a", "line": "hello world"}]}
    assert doc.to_markdown(annotated=True)["anchors"] == [{"id": "a", "path": "main.body", "card_id": None, "line": "hello world"}]
