"""Tests for the engine and quill surface: loading a quill, what it exposes
engine-free, and what the engine answers about it."""
import pytest
from quillmark import Engine, Quill, Document, OutputFormat, QuillmarkError


def test_quill_properties(engine, taro_quill_dir):
    """Quill.from_path loads engine-free, validated config data; capability
    lives on the engine, not the quill."""
    quill = Quill.from_path(str(taro_quill_dir))

    metadata = quill.metadata
    assert metadata["name"] == "taro"
    # The key order BINDINGS.md pins across both surfaces.
    assert list(metadata) == ["name", "version", "backend", "author", "description"]
    assert quill.backend_id == "typst"

    assert "fields" in quill.schema["main"]
    assert OutputFormat.PDF in engine.supported_formats(quill)


def test_a_document_crosses_the_authoring_form(taro_quill_dir):
    quill = Quill.from_path(str(taro_quill_dir))
    schema = quill.authoring_schema
    assert schema["properties"]["fields"]["properties"]["ice_cream"]["default"] == "taro"
    assert schema["$defs"]["quotes"]["properties"]["kind"] == {"const": "quotes"}

    authored = {
        "fields": {"title": "Hello", "author": None},
        "body": "Some *prose*.",
        "cards": [{"kind": "quotes", "fields": {"author": "Nibs"}, "body": "Taro."}],
    }
    doc = quill.from_authoring(authored)
    assert doc.quill_ref == "taro@0.1"
    del authored["fields"]["author"]
    assert quill.to_authoring(doc) == authored

    with pytest.raises(QuillmarkError) as caught:
        quill.from_authoring({"fields": [], "cards": [{}]})
    assert [(d.code, d.path, d.args.get("pointer")) for d in caught.value.diagnostics] == [
        ("authoring::invalid_shape", None, "/fields"),
        ("authoring::invalid_shape", None, "/cards/0/kind"),
    ]


def test_a_quill_round_trips_through_its_bundle_and_path(taro_quill_dir, tmp_path):
    quill = Quill.from_path(str(taro_quill_dir))
    bundle = quill.to_bundle()
    assert Quill.from_bundle(bundle).to_bundle() == bundle

    path = tmp_path / "taro.quill"
    path.write_bytes(bundle)
    assert Quill.from_path(str(path)).metadata == quill.metadata

    with pytest.raises(QuillmarkError) as caught:
        Quill.from_bundle(b"not a zip")
    assert caught.value.diagnostics[0].code == "quill::bundle_invalid"


def test_metadata_carries_a_declared_license_and_homepage(tmp_path):
    (tmp_path / "Quill.yaml").write_text(
        "quill:\n"
        "  name: licensed\n"
        "  version: '1.0.0'\n"
        "  backend: typst\n"
        "  description: A licensed quill\n"
        "  license: Apache-2.0 OR MIT\n"
        "  homepage: https://example.com/quills\n"
    )
    metadata = Quill.from_path(str(tmp_path)).metadata
    assert list(metadata) == [
        "name", "version", "backend", "author", "description", "license", "homepage",
    ]
    assert metadata["license"] == "Apache-2.0 OR MIT"
    assert metadata["homepage"] == "https://example.com/quills"


def test_quill_from_path_bad_backend_loads_then_fails_at_render(tmp_path):
    """An unregistered backend crosses as a plain `backend_id` string on a
    loaded quill and as a raised QuillmarkError at render. Which engine calls
    resolve the backend is core's contract
    (`crates/quillmark/tests/quill_engine_test.rs`)."""
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )

    # Engine-free load succeeds: the config is valid, the backend is not resolved.
    quill = Quill.from_path(str(quill_dir))
    assert quill.backend_id == "nonexistent"
    assert quill.metadata["backend"] == "nonexistent"

    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\nBody.\n"
    )
    with pytest.raises(QuillmarkError):
        Engine().render(quill, doc, OutputFormat.PDF)


def test_warnings_carry_the_loads_advisories(taro_quill_dir, tmp_path):
    """A config warning reaches the host off the loaded quill."""
    assert Quill.from_path(str(taro_quill_dir)).warnings == []

    quill_dir = tmp_path / "warn_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "warn"\n  version: "1.0.0"\n  backend: "typst"\n  description: "W"\n'
        "main:\n  fields:\n    title: { type: text }\n"
        "card_kinds:\n  skills:\n    body:\n      enabled: false\n"
        "    fields:\n      items: { type: array, items: { type: text } }\n"
    )

    quill = Quill.from_path(str(quill_dir))
    assert [d.code for d in quill.warnings] == ["quill::bodiless_card_kind"]
