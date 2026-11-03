"""A value's live Automerge document across the Python boundary: `save` and
`load`, and the sync protocol two documents run through
`generate_sync_message` / `receive_sync_message`. Offsets count code points."""

from quillmark import Document, SyncState, import_markdown

from conftest import raises_edit_code


def parse(body):
    return Document.parse(f"~~~\n$quill: q\n~~~\n\n{body}\n")


def sync(a, b, field=None):
    """Run the sync protocol between `a` and `b` at `field` until neither has
    a message, returning the receipts each received."""
    to_b, to_a = Document.sync_state(), Document.sync_state()
    receipts = {"a": [], "b": []}
    for _ in range(16):
        for_b = a.generate_sync_message(to_b, field=field)
        if for_b is not None:
            receipts["b"].append(b.receive_sync_message(to_a, for_b, field=field))
        for_a = b.generate_sync_message(to_a, field=field)
        if for_a is not None:
            receipts["a"].append(a.receive_sync_message(to_b, for_a, field=field))
        if for_a is None and for_b is None:
            return receipts
    raise AssertionError("the sync did not settle")


def test_a_live_document_saves_to_bytes_another_loads():
    doc = parse("Hello world")
    doc.apply({"steps": [{"splice": {"at": 5, "delete": "", "insert": " brave"}}]})
    data = doc.save()
    assert isinstance(data, bytes)

    other = parse("else")
    before = other.hash_at()
    receipt = other.load(data)
    assert other.read() == doc.read()
    assert receipt["hash"] == doc.hash_at()
    assert receipt["applied"]["steps"]
    with raises_edit_code("edit::stale_base"):
        other.apply({"steps": []}, base=before)

    with raises_edit_code("edit::bad_live_bytes"):
        other.load(b"\x01\x02\x03")
    with raises_edit_code("edit::bad_live_bytes"):
        other.receive_sync_message(Document.sync_state(), b"\x01\x02\x03")
    assert other.read() == doc.read()


def test_two_documents_parsed_from_one_value_sync_to_one_value():
    a, b = parse("😀 one two"), parse("😀 one two")
    a.apply({"steps": [{"splice": {"at": 2, "delete": "", "insert": "X"}}]}, actor="ann")
    b.apply({"steps": [{"splice": {"at": 9, "delete": "", "insert": " three"}}]}, actor="bo")

    receipts = sync(a, b)
    assert a.read() == b.read()
    assert a.read()["text"] == "😀 Xone two three"
    splices = [s["splice"]["at"] for r in receipts["b"] for s in r["applied"]["steps"]]
    assert splices == [2]
    again = sync(a, b)
    assert [s for r in again["a"] + again["b"] for s in r["applied"]["steps"]] == []


def test_a_receipt_names_an_anchor_a_merge_took_out():
    a = parse("Hello brave new world")
    a.apply({"steps": [{"anchor": {"id": "c-1", "start": 6, "end": 15}}]})
    b = parse("else")
    b.load(a.save())
    a.apply({"steps": [{"splice": {"at": 6, "delete": "brave ", "insert": ""}}]}, actor="ann")
    b.apply({"steps": [{"splice": {"at": 11, "delete": " new", "insert": ""}}]}, actor="bo")

    receipts = sync(a, b)
    assert a.read()["text"] == "Hello  world"
    assert b.read() == a.read()
    for side in receipts.values():
        dropped = [d for r in side for d in r["dropped_anchors"]]
        assert [(d["id"], d["path"]) for d in dropped] == [("c-1", "main.body")]

    a.replace(import_markdown("a note")["content"], field="note", actor="ann")
    sync(a, b, field="note")
    assert b.read(field="note") == a.read(field="note")
    assert isinstance(Document.sync_state(), SyncState)


def test_bytes_or_a_message_from_another_values_live_document_are_refused():
    two = Document.parse(
        "~~~\n$quill: q\n~~~\n\n~~~card-yaml\n$kind: note\n$id: aaa\n~~~\n\nHello\n\n"
        "~~~card-yaml\n$kind: note\n$id: bbb\n~~~\n\nHello\n"
    )
    with raises_edit_code("edit::bad_live_bytes") as err:
        two.load(two.save(card="aaa"), card="bbb")
    assert err.value.diagnostics[0].args["reason"] == "address"

    peer = Document.parse(two.to_markdown())
    to_peer, to_two = Document.sync_state(), Document.sync_state()
    peer.receive_sync_message(to_two, two.generate_sync_message(to_peer, card="bbb"), card="aaa")
    reply = peer.generate_sync_message(to_two, card="aaa")
    with raises_edit_code("edit::bad_live_bytes") as err:
        two.receive_sync_message(to_peer, reply, card="bbb")
    assert err.value.diagnostics[0].args["reason"] == "address"
