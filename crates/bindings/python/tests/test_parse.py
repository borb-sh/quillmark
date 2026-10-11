"""Tests for Document: the quill-free transport surface (parse, storage, clone).
The mutator surface is `test_document.py`; typed field I/O is
`test_writer_reader.py`."""

import pytest

from quillmark import Document, QuillmarkError

from conftest import field, has_field, raises_edit_code


def test_projection(taro_md):
    """`main` exposes payload fields but not `$` metadata; `body` is the content
    dict; `cards` projects each card's kind, fields and body."""
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in field(doc.main, "title")
    assert not has_field(doc.main, "$quill")
    assert "nutty" in doc.body["text"]
    assert len(doc.cards) == 1
    card = doc.cards[0]
    assert card["kind"] == "quotes"
    assert field(card, "author") == "Albert Einstein"
    assert "mistake" in card["body"]["text"]


def test_json_dto_round_trip(taro_md):
    """to_stored emits a DTO tagged with the current version that from_stored
    round-trips."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_stored()
    assert Document.storage_version_of(dto) == Document.current_storage_version()

    restored = Document.from_stored(dto)
    assert restored.quill_ref == doc.quill_ref
    assert restored.to_markdown() == doc.to_markdown()


def test_json_dto_rejects_invalid_input():
    """from_stored rejects an unknown schema tag and malformed JSON."""
    with pytest.raises(QuillmarkError):
        Document.from_stored('{"schema":"quillmark/document@0.99.0","main":{}}')
    with pytest.raises(QuillmarkError):
        Document.from_stored("not json at all")


def test_storage_version_of():
    """A future tag is returned so callers can tell "build too old" from
    "payload corrupt"; a non-DTO input is None."""
    future = '{"schema":"quillmark/document@0.99.0"}'
    assert Document.storage_version_of(future) == "quillmark/document@0.99.0"
    assert Document.storage_version_of("not json") is None
    assert Document.storage_version_of('{"foo":"bar"}') is None


def test_clone_and_copy_isolate_mutations(taro_md):
    """clone(), copy.copy and copy.deepcopy compare equal and are independent."""
    import copy

    doc = Document.from_markdown(taro_md)
    for dup in (doc.clone(), copy.copy(doc), copy.deepcopy(doc)):
        assert dup == doc and dup.equals(doc)
        dup.remove_field("title")
        assert has_field(doc.main, "title")


def test_remove_field_on_card():
    """remove_field(name, card=i) removes and returns a composable card field."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\nbaz: qux\n~~~\n"
    )
    doc = Document.from_markdown(md)
    assert doc.remove_field("foo", card=0) == "bar"
    assert not has_field(doc.cards[0], "foo")
    assert field(doc.cards[0], "baz") == "qux"
    with raises_edit_code("edit::index_out_of_range"):
        doc.remove_field("foo", card=1)


def test_diagnostic_str_and_repr():
    warn_md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\n"
        "weird: !custom value\n~~~\n\nBody\n"
    )
    diag = Document.from_markdown(warn_md).warnings[0]
    assert diag.code == "parse::unsupported_yaml_tag"
    assert diag.message in str(diag)
    assert "Diagnostic(" in repr(diag)


def test_document_authoring_text_helpers():
    rules = Document.markdown_rules()
    assert isinstance(rules, str) and rules.strip() != ""

    hint = Document.quill_ref_hint()
    assert isinstance(hint, str) and hint.strip() != ""


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
