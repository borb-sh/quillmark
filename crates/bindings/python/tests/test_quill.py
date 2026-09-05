"""Tests for quill loading."""
import pytest
from quillmark import Quillmark, Quill, Document, OutputFormat, QuillmarkError


def test_quill_from_path(taro_quill_dir):
    """Quill.from_path loads engine-free, validated config data."""
    quill = Quill.from_path(str(taro_quill_dir))
    assert quill is not None
    assert quill.metadata["name"] == "taro"
    assert quill.backend_id == "typst"


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


def test_metadata_orders_standard_keys_then_extras_sorted(tmp_path):
    """metadata's key order is a function of the quill: the five standard keys
    in their declared order, then the extra keys sorted by name."""
    quill_dir = tmp_path / "meta_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        "quill:\n"
        '  name: "meta_quill"\n'
        '  version: "0.1.0"\n'
        '  backend: "typst"\n'
        '  description: "Metadata test"\n'
        "typst:\n"
        "  zeta: z\n"
        "  plate_file: plate.typ\n"
        "  alpha: a\n"
        "  nu: n\n"
        "  beta: b\n"
        "  mu: m\n"
    )
    (quill_dir / "plate.typ").write_text("= Test\n")

    quill = Quill.from_path(str(quill_dir))

    assert list(quill.metadata) == [
        "name",
        "version",
        "backend",
        "author",
        "description",
        "typst_alpha",
        "typst_beta",
        "typst_mu",
        "typst_nu",
        "typst_plate_file",
        "typst_zeta",
    ]
