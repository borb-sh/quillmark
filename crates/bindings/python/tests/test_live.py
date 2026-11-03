"""The document's Automerge history across the Python boundary: `save` and
`load`, `heads`, `save_since` and `merge`, and the sync protocol two
documents run through `generate_sync_message` / `receive_sync_message`.
Offsets count code points."""

from quillmark import Document, SyncState

from conftest import raises_edit_code


def parse(body):
    return Document.parse(f"~~~\n$quill: q\n~~~\n\n{body}\n")


def sync(a, b, states=None):
    """Run the sync protocol between `a` and `b` until neither has a message,
    returning how many messages crossed."""
    to_b, to_a = states or (SyncState(), SyncState())
    crossed = 0
    for _ in range(16):
        for_b = a.generate_sync_message(to_b)
        if for_b is not None:
            b.receive_sync_message(to_a, for_b)
            crossed += 1
        for_a = b.generate_sync_message(to_a)
        if for_a is not None:
            a.receive_sync_message(to_b, for_a)
            crossed += 1
        if for_a is None and for_b is None:
            return crossed
    raise AssertionError("the sync did not settle")


def test_a_document_saves_to_bytes_load_reads_back():
    doc = parse("Hello world")
    doc.apply({"steps": [{"splice": {"at": 5, "delete": "", "insert": " brave"}}]})
    data = doc.save()
    assert isinstance(data, bytes)

    other = Document.load(data)
    assert other.read() == doc.read()
    assert other.heads() == doc.heads()
    assert other.to_markdown() == doc.to_markdown()

    with raises_edit_code("edit::bad_live_bytes") as err:
        Document.load(b"\x01\x02\x03")
    assert err.value.diagnostics[0].args["reason"] == "decode"


def test_changes_since_heads_merge_into_a_peer():
    a = parse("Hello world")
    b = Document.load(a.save())
    heads = a.heads()
    a.apply({"steps": [{"splice": {"at": 5, "delete": "", "insert": " brave"}}]}, actor="ann")
    b.apply({"steps": [{"splice": {"at": 11, "delete": "", "insert": "!"}}]}, actor="b0")

    b.merge(a.save_since(heads))
    a.merge(b.save())
    assert a.read()["text"] == b.read()["text"] == "Hello brave world!"
    assert sorted(a.heads()) == sorted(b.heads())
    before = a.read()
    a.merge(b"\x01\x02\x03")
    assert a.read() == before


def test_two_documents_from_one_save_sync_to_one_value():
    a = parse("😀 one two")
    b = Document.load(a.save())
    a.apply({"steps": [{"splice": {"at": 2, "delete": "", "insert": "X"}}]}, actor="ann")
    b.apply({"steps": [{"splice": {"at": 9, "delete": "", "insert": " three"}}]}, actor="b0")

    states = (SyncState(), SyncState())
    assert sync(a, b, states) > 0
    assert a.read() == b.read()
    assert a.read()["text"] == "😀 Xone two three"
    assert sync(a, b, states) == 0
    with raises_edit_code("edit::bad_live_bytes"):
        a.receive_sync_message(SyncState(), b"\x01\x02\x03")


def test_a_sync_state_survives_encode_and_decode():
    a = parse("Hello world")
    b = Document.load(a.save())
    states = (SyncState(), SyncState())
    sync(a, b, states=states)
    a.apply({"steps": [{"splice": {"at": 5, "delete": "", "insert": " brave"}}]}, actor="ann")

    decoded = tuple(SyncState.decode(s.encode()) for s in states)
    sync(a, b, states=decoded)
    assert b.read()["text"] == "Hello brave world"
    with raises_edit_code("edit::bad_live_bytes") as err:
        SyncState.decode(b"\x01\x02\x03")
    assert err.value.diagnostics[0].args["reason"] == "decode"
