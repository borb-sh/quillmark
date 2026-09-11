"""Tests for the engine and quill surface: loading a quill, what it exposes
engine-free, and what the engine answers about it."""
import pytest
from quillmark import Quillmark, Quill, Document, OutputFormat, QuillmarkError, Severity


def test_quill_properties(engine, taro_quill_dir):
    """Quill.from_path loads engine-free, validated config data; capability
    lives on the engine, not the quill."""
    quill = Quill.from_path(str(taro_quill_dir))

    metadata = quill.metadata
    assert isinstance(metadata, dict)
    assert metadata["name"] == "taro"
    # The key order BINDINGS.md pins across both surfaces.
    assert list(metadata) == ["name", "version", "backend", "author", "description"]
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


def test_enum_members_are_hashable():
    """Both mirrors key a dict and enter a set, one slot per variant."""
    mime = {OutputFormat.PDF: "application/pdf", OutputFormat.SVG: "image/svg+xml"}
    assert mime[OutputFormat.PDF] == "application/pdf"
    assert len(set(OutputFormat.all())) == len(OutputFormat.all())
    assert len(set(Severity.all())) == len(Severity.all())


def test_quill_from_path_bad_backend_loads_then_fails_at_render(tmp_path):
    """An unregistered backend crosses as a plain `backend_id` string on a
    loaded quill and as a raised QuillmarkError at render. Which engine calls
    resolve the backend is core's contract
    (`crates/quillmark/tests/quill_engine_test.rs`)."""
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )

    # Engine-free load succeeds: the config is valid, the backend is not resolved.
    quill = Quill.from_path(str(quill_dir))
    assert quill.backend_id == "nonexistent"
    assert quill.metadata["backend"] == "nonexistent"

    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\nBody.\n"
    )
    with pytest.raises(QuillmarkError):
        Quillmark().render(quill, doc, OutputFormat.PDF)


def test_warnings_carry_the_loads_advisories(taro_quill_dir, tmp_path):
    """A config warning reaches the host off the loaded quill. Before, only the
    CLI's own loader door kept them and a Python host could not read them at
    all."""
    assert Quill.from_path(str(taro_quill_dir)).warnings == []

    quill_dir = tmp_path / "warn_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "warn"\n  version: "1.0"\n  backend: "typst"\n  description: "W"\n'
        "main:\n  fields:\n    title: { type: string }\n"
        "card_kinds:\n  skills:\n    body:\n      enabled: false\n"
        "      example: This example is unused\n"
        "    fields:\n      items: { type: array, items: { type: string } }\n"
    )

    quill = Quill.from_path(str(quill_dir))
    assert [d.code for d in quill.warnings] == ["quill::body_example_unused"]


