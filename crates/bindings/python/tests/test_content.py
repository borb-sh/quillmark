"""The content model across the Python boundary: the write primitive `apply`,
its producers `revise` and `replace`, the reads, and the pure helpers over
changes. Offsets count code points."""

import pytest
from quillmark import Document, compose, diff, inherited_marks, invert, join_blocks, map_pos, rebase, repair, split_block

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


def test_rebase_maps_a_change_past_another():
    base = {"text": "hello world", "blocks": [{"id": "b0", "kind": "para"}]}
    typed = {"steps": [{"splice": {"at": 0, "delete": "", "insert": "XX "}}]}
    mark = {"steps": [{"mark": {"start": 6, "end": 11, "type": "strong", "add": True, "quote": "world"}}]}
    assert rebase(base, mark, typed)["steps"][0]["mark"] == {"start": 9, "end": 14, "type": "strong", "add": True, "quote": "world"}
    with raises_edit_code("edit::rebase_conflict") as err:
        rebase(base, mark, {"steps": [{"splice": {"at": 5, "delete": " world", "insert": ""}}]})
    assert (err.value.diagnostics[0].args["step"], err.value.diagnostics[0].args["reason"]) == (0, "deleted")


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
    assert (map_pos(a, 5, "before")["pos"], map_pos(a, 5, "after")["pos"]) == (5, 6)
    gone = map_pos({"steps": [{"splice": {"at": 0, "delete": "he", "insert": ""}}]}, 1, "after")
    assert gone == {"pos": 0, "deleted": True, "deleted_before": True, "deleted_after": True, "deleted_across": True}
    with raises_edit_code("edit::content_apply"):
        map_pos({"steps": [{"splice": {"at": 0, "delete": "\ufffc", "insert": ""}}]}, {"island": "isl-0", "slot": "s0", "at": 0}, "after")
    with pytest.raises(ValueError):
        map_pos(a, 5, "left")

    doc = Document.from_markdown(MD)
    before = doc.read()
    doc2 = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\nhello brave world\n")
    diffed = diff(before, doc2.read())
    assert diffed["warnings"] == []
    doc.apply(diffed["change"])
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
        doc.apply_all([{"card": "nobody", "change": {"steps": []}}])
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


def test_split_block_and_join_blocks_keep_the_text_on_its_id():
    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\n# Title\n\nbody\n")
    before = doc.read()
    title, body = (b["id"] for b in before["blocks"])
    split = split_block(before, 6, "b9")
    assert split == {"steps": [{"splice": {"at": 5, "delete": "", "insert": "\n", "blocks": [{"id": "b9", "kind": "para"}]}}]}
    doc.apply(split)
    assert [b["id"] for b in doc.read()["blocks"]] == [title, "b9", body]

    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\n# Title\n\nbody\n")
    doc.apply(join_blocks(doc.read(), body))
    assert doc.read()["blocks"] == [{"id": title, "kind": "heading", "attrs": {"level": 1}}]
    with raises_edit_code("edit::content_apply"):
        join_blocks(before, title)


def test_inherited_marks_grow_strong_at_its_end_in_code_points():
    doc = Document.from_markdown("~~~card-yaml\n$quill: test_quill\n~~~\n\n😀 **bold** z\n")
    before = doc.read()
    assert inherited_marks(before, 6) == [{"type": "strong"}]
    assert inherited_marks(before, 2) == []
    with raises_edit_code("edit::content_apply"):
        inherited_marks(before, 99)


def test_repair_makes_a_merged_state_valid_in_code_points():
    out = repair(
        {
            "text": "😀ab￼cd",
            "blocks": [{"id": "b0", "kind": "para", "path": ["l9"]}],
            "anchors": [{"id": "c-1", "start": 1, "end": 3}, {"id": "c-2", "start": 2, "end": 2, "range": True}],
            "slot_ids": ["t0"],
            "islands": {"m0": {"type": "math", "props": {"tex": "x"}}},
        }
    )
    assert out["content"]["text"] == "😀abcd"
    assert out["content"]["anchors"] == [{"id": "c-1", "start": 1, "end": 3}]
    assert [r["repair"] for r in out["repairs"]] == [
        "orphan_slot",
        "orphan_island",
        "missing_container",
        "collapsed_anchor",
    ]
    assert repair(out["content"]) == {"content": out["content"], "repairs": []}
    with pytest.raises(ValueError):
        repair({"blocks": []})
