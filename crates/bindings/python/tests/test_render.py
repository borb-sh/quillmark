"""Tests for rendering workflow."""

import datetime

import pytest

from quillmark import OutputFormat, Document, Quill, QuillmarkError


def test_pdf_artifact_reads_share_one_buffer(engine, taro_quill_dir, taro_md, tmp_path):
    """Re-reading `artifacts` hands back the same objects, and `bytes` is what `save` wrote."""
    quill = Quill.from_path(str(taro_quill_dir))
    result = engine.render(quill, Document.from_markdown(taro_md), OutputFormat.PDF)

    artifact = result.artifacts[0]
    assert result.artifacts[0] is artifact
    assert result.format == OutputFormat.PDF
    assert artifact.format == OutputFormat.PDF
    assert artifact.mime_type == "application/pdf"

    output_path = tmp_path / "output.pdf"
    artifact.save(str(output_path))
    assert artifact.bytes
    assert output_path.read_bytes() == artifact.bytes


def test_engine_render_svg_page_selection(engine, taro_quill_dir, taro_md):
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    subset = engine.render(quill, parsed, OutputFormat.SVG, pages=[0])
    assert len(subset.artifacts) == 1
    assert subset.format == OutputFormat.SVG
    assert subset.artifacts[0].format == OutputFormat.SVG


def test_engine_render_name_mismatch_errors(engine, taro_quill_dir):
    """A render error crosses as QuillmarkError with its diagnostic `code` intact.

    `$quill` mismatch is the representative case; that it is a hard error at all
    (and the version-selector half of the rule) is pinned in
    `crates/quillmark/tests/version_mismatch_test.rs`.
    """
    quill = Quill.from_path(str(taro_quill_dir))

    # Build a document that names a different quill
    mismatch_md = (
        "~~~card-yaml\n"
        "$quill: completely_different_quill\n"
        "$kind: main\n"
        "author: Test Author\n"
        "ice_cream: Chocolate\n"
        "title: Mismatch Test\n"
        "~~~\n\nContent.\n"
    )
    parsed = Document.from_markdown(mismatch_md)

    with pytest.raises(QuillmarkError) as exc_info:
        engine.render(quill, parsed)

    assert "quill::name_mismatch" in [d.code for d in exc_info.value.diagnostics]


def test_engine_render_negative_page_is_out_of_bounds(engine, taro_quill_dir, taro_md):
    """Page indices count from the first page, so a negative one is refused under
    the code a page past the last is refused under."""
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    with pytest.raises(QuillmarkError) as exc_info:
        engine.render(quill, parsed, OutputFormat.SVG, pages=[-1])

    assert exc_info.value.diagnostics[0].code == "backend::page_index_out_of_bounds"


def test_engine_render_regions_sidecar(engine, taro_quill_dir, taro_md):
    """render(.., regions=True) populates the schema-field geometry sidecar.

    Mirrors the WASM regions contract: each entry is a dict carrying `field`,
    `page`, `rect`, and `span` (the covered USV content range for content ink,
    `None` for a scalar/widget). `field` is a `DocPath` address, translated from
    the backend's plate space at the boundary, so the main body reads
    `main.body` and a card field `cards.<kind>[<i>].<field>`. The taro plate
    interpolates the body, so it auto-tags at least one segment carrying a span.
    """
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    result = engine.render(quill, parsed, OutputFormat.PDF, regions=True)

    regions = result.regions
    assert regions
    for r in regions:
        assert isinstance(r["field"], str) and not r["field"].startswith("$")
        assert isinstance(r["page"], int)
        assert isinstance(r["rect"], list) and len(r["rect"]) == 4
        assert r["span"] is None or (isinstance(r["span"], list) and len(r["span"]) == 2)
    body_segments = [r for r in regions if r["field"] == "main.body"]
    assert any(r["span"] is not None for r in body_segments)


def test_parse_error_carries_diagnostics():
    """Parse failures raise QuillmarkError with a non-empty `.diagnostics` list."""
    invalid_md = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: [unclosed bracket\n~~~\n"
    with pytest.raises(QuillmarkError) as exc_info:
        Document.from_markdown(invalid_md)
    assert exc_info.value.diagnostics
    assert all(d.code for d in exc_info.value.diagnostics)


def test_quill_load_error_carries_diagnostics(tmp_path):
    """A malformed config fails at `Quill.from_path`; only backend resolution
    is deferred to render."""
    bogus = tmp_path / "not_a_quill"
    bogus.mkdir()
    (bogus / "Quill.yaml").write_text("quill: { name: x }\n")

    with pytest.raises(QuillmarkError) as exc_info:
        Quill.from_path(str(bogus))
    assert exc_info.value.diagnostics


def test_render_dates_a_today_field(engine, tmp_path):
    """`today` is the render date: a given `datetime.date`, else the local one, and the plate's `datetime.today()` agrees."""

    def dated_quill(name, check):
        root = tmp_path / name
        root.mkdir()
        (root / "Quill.yaml").write_text(
            "quill:\n  name: dated\n  version: 0.1.0\n  backend: typst\n"
            "  description: A field dated by the day it renders\n"
            "typst:\n  plate_file: plate.typ\n"
            "main:\n  fields:\n    issued: { type: date, default: today }\n"
        )
        (root / "plate.typ").write_text(
            '#import "@local/quillmark-helper:0.1.0": data\n'
            "#assert.eq(data.issued, datetime.today())\n"
            f"#assert({check})\n"
        )
        return Quill.from_path(str(root))

    doc = Document.from_markdown("~~~card-yaml\n$quill: dated\n$kind: main\n~~~\n")

    pinned = dated_quill("pinned", "data.issued == datetime(year: 2026, month: 3, day: 14)")
    engine.render(pinned, doc, OutputFormat.SVG, today=datetime.date(2026, 3, 14))

    local = dated_quill("local", "data.issued != none")
    engine.render(local, doc, OutputFormat.SVG)
