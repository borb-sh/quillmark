"""Tests for Document: the quill-free transport surface (parse, storage, clone).
The mutator surface is `test_document.py`; typed field I/O is
`test_writer_reader.py`."""

import pytest

from quillmark import Document, QuillmarkError

from conftest import field, has_field, raises_edit_code


def test_payload_access(taro_md):
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in (field(doc.main, "title") or "")
    # `$`-prefixed metadata is not exposed as payload fields
    assert not has_field(doc.main, "$body")
    assert not has_field(doc.main, "$cards")
    assert not has_field(doc.main, "$quill")


def test_body_is_content_dict(taro_md):
    """`body` is the canonical content dict (source of truth); its `text` carries
    the plain USV text quill-free. The markdown projection is
    `quill.reader(doc).body_markdown()`."""
    doc = Document.from_markdown(taro_md)
    assert isinstance(doc.body, dict)
    assert "nutty" in doc.body["text"]


def test_body_spells_every_container_instance():
    """The seam form: `instance` decides whether two adjacent same-shape runs
    weld, and it is a field a writer owes. Storage omits a zero; a read hands
    back a container path a write can take, so it spells one."""
    md = "~~~card-yaml\n$quill: taro\n$kind: main\n~~~\n\n> a\n\n- b\n"
    doc = Document.from_markdown(md)
    containers = [c for line in doc.body["lines"] for c in line["containers"]]
    assert [c["container"] for c in containers] == ["quote", "list_item"]
    assert [c["instance"] for c in containers] == [0, 0]
    # `instance` is an envelope key and stays a sibling; the shape a member
    # names rides `attrs`, whether or not this build knows the name.
    assert containers[1]["attrs"] == {"ordered": False, "ordinal": 0, "start": 1}
    assert "attrs" not in containers[0]


def test_body_empty_when_absent():
    md = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n~~~\n"
    doc = Document.from_markdown(md)
    assert doc.body["text"] == ""


def test_cards_access():
    md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Main\n~~~\n\nGlobal body.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    assert len(doc.cards) == 1
    card = doc.cards[0]
    assert card["kind"] == "note"
    assert field(card, "foo") == "bar"
    assert "Card body." in card["body"]["text"]


def test_json_dto_round_trip(taro_md):
    """to_stored emits a versioned DTO string that from_stored round-trips."""
    doc = Document.from_markdown(taro_md)

    dto = doc.to_stored()
    assert isinstance(dto, str)
    assert "quillmark/document@0.112.0" in dto

    restored = Document.from_stored(dto)
    assert restored.quill_ref == doc.quill_ref
    assert restored.to_markdown() == doc.to_markdown()


def test_json_dto_rejects_invalid_input():
    """from_stored rejects an unknown schema tag and malformed JSON."""
    with pytest.raises(QuillmarkError):
        Document.from_stored('{"schema":"quillmark/document@0.99.0","main":{}}')
    with pytest.raises(QuillmarkError):
        Document.from_stored("not json at all")


def test_json_dto_drops_parse_warnings():
    """A DTO-reconstructed document carries no parse-time warnings."""
    # An unknown YAML tag triggers a `parse::unsupported_yaml_tag` warning.
    warn_md = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\nweird: !custom value\n~~~\n\nBody\n"
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    restored = Document.from_stored(doc.to_stored())
    assert restored.warnings == []


def test_schema_version_of_returns_unknown_future_versions():
    # Note: this would be rejected by from_stored, but storage_version_of returns it
    # so callers can distinguish "build too old" from "payload corrupt".
    future = '{"schema":"quillmark/document@0.99.0"}'
    assert Document.storage_version_of(future) == "quillmark/document@0.99.0"


def test_schema_version_of_returns_none_for_non_dto():
    """storage_version_of returns None when the input is not a schema-tagged object."""
    assert Document.storage_version_of("not json") is None
    assert Document.storage_version_of('{"foo":"bar"}') is None


def test_current_schema_version_matches_emitted_tag(taro_md):
    doc = Document.from_markdown(taro_md)
    dto = doc.to_stored()

    current = Document.current_storage_version()
    assert isinstance(current, str)
    assert Document.storage_version_of(dto) == current


def test_clone_isolates_mutations(taro_md):
    """Mutating a clone does not affect the original."""
    doc = Document.from_markdown(taro_md)
    cloned = doc.clone()

    cloned.remove_field("title")
    assert has_field(doc.main, "title")
    assert not has_field(cloned.main, "title")


def test_copy_module_clones(taro_md):
    """The standard library copy module works on Document."""
    import copy

    doc = Document.from_markdown(taro_md)

    shallow = copy.copy(doc)
    deep = copy.deepcopy(doc)

    assert shallow == doc
    assert deep == doc

    shallow.remove_field("title")
    assert has_field(doc.main, "title")


def test_equals_and_eq(taro_md):
    """equals and == both compare structurally; warnings are ignored."""
    doc1 = Document.from_markdown(taro_md)
    doc2 = Document.from_markdown(taro_md)

    assert doc1.equals(doc2)
    assert doc1 == doc2


def test_remove_field_on_card_returns_value():
    """remove_field(name, card=i) removes and returns a composable card field's
    value: `remove` has no lane, one verb over the whole card axis."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\nbaz: qux\n~~~\n"
    )
    doc = Document.from_markdown(md)

    removed = doc.remove_field("foo", card=0)
    assert removed == "bar"
    assert not has_field(doc.cards[0], "foo")
    assert field(doc.cards[0], "baz") == "qux"


def test_remove_field_on_card_out_of_range():
    md = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n"
    doc = Document.from_markdown(md)
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_field("foo", card=0)


def test_diagnostic_str_is_canonical_pretty_text():
    warn_md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\n"
        "weird: !custom value\n~~~\n\nBody\n"
    )
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    diag = doc.warnings[0]
    pretty = str(diag)
    assert isinstance(pretty, str) and pretty.strip() != ""
    assert diag.message in pretty
    assert "Diagnostic(" in repr(diag)


def test_document_authoring_text_helpers():
    rules = Document.format_rules()
    assert isinstance(rules, str) and rules.strip() != ""

    hint = Document.quill_ref_hint()
    assert isinstance(hint, str) and hint.strip() != ""

    instr = Document.blueprint_instruction("taro")
    assert isinstance(instr, str) and "taro" in instr


def _nest(levels, leaf):
    """Wrap `leaf` in `levels` nested {"a": …} objects, built iteratively."""
    v = leaf
    for _ in range(levels):
        v = {"a": v}
    return v


def test_depth_bound_matches_core_container_levels():
    """py_to_json_at and core's json_depth_exceeds reject the identical shape.

    The cutoff is container levels (128), not nodes: a scalar leaf at the
    bottom is not charged a level, so exactly 128 nested objects are accepted
    and 129 are rejected: whether the deepest container holds a scalar or
    another (non-empty) container. Exercised through `store_ext`, the opaque
    door whose whole argument crosses `py_to_json`, so the mapping it takes is
    itself the outermost charged level and the nesting below it is one short of
    the constant.
    """
    def storing(value):
        Document("depth_test").store_ext({"ns": value})

    # Scalar-terminated: the wrapper plus 127 objects with a scalar leaf is the limit.
    storing(_nest(127, 1))
    with pytest.raises((QuillmarkError, ValueError)):
        storing(_nest(128, 1))

    # Container-terminated: the deepest container, not its contents, occupies
    # the last level, so the boundary is identical.
    storing(_nest(126, [1, 2, 3]))
    with pytest.raises((QuillmarkError, ValueError)):
        storing(_nest(127, [1, 2, 3]))


def test_nested_fill_exposed_as_nested_fills():
    """A nested !must_fill marker is exposed as `nestedFills` on the field item
    and survives the storage round-trip (binding parity for nested fills)."""
    md = (
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\n"
        "addr:\n  street: !must_fill\n  city: Anytown\n~~~\n"
    )
    doc = Document.from_markdown(md)
    addr = next(i for i in doc.main["payload_items"] if i.get("key") == "addr")
    assert addr.get("nestedFills") == [["street"]], addr
    # Storage round-trip preserves the nested marker.
    restored = Document.from_stored(doc.to_stored())
    assert "street: !must_fill" in restored.to_markdown()


def test_nested_fill_insert_card_round_trip():
    """A card dict carrying `nestedFills` can be inserted and the nested marker
    is reconstructed on emit (the dict -> Card serde path reads it back)."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: x\n~~~\n"
    )
    doc.insert_card(
        {
            "kind": "note",
            "payload_items": [
                {
                    "type": "field",
                    "key": "addr",
                    "value": {"street": None, "city": "A"},
                    "nestedFills": [["street"]],
                }
            ],
            "body": "",
        }
    )
    assert "street: !must_fill" in doc.to_markdown()
