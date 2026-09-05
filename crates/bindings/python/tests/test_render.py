"""Tests for rendering workflow."""

import pytest

from quillmark import OutputFormat, Document, Quill, QuillmarkError


def test_artifact_reads_share_one_buffer(engine, taro_quill_dir, taro_md, tmp_path):
    """Re-reading `artifacts` hands back the same objects, and `bytes` is what `save` wrote."""
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.PDF)

    artifact = result.artifacts[0]
    assert result.artifacts[0] is artifact

    output_path = tmp_path / "output.pdf"
    artifact.save(str(output_path))

    assert artifact.bytes
    assert output_path.read_bytes() == artifact.bytes


def test_engine_render_with_explicit_format(engine, taro_quill_dir, taro_md):
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.SVG)

    assert len(result.artifacts) > 0
    assert result.format == OutputFormat.SVG
    assert result.artifacts[0].format == OutputFormat.SVG


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

    codes = [d.code for d in exc_info.value.diagnostics]
    assert "quill::name_mismatch" in codes, f"expected name_mismatch error, got: {codes}"


def test_engine_render_page_selection(engine, taro_quill_dir, taro_md):
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    subset = engine.render(quill, parsed, OutputFormat.SVG, pages=[0])
    assert len(subset.artifacts) == 1
    assert subset.format == OutputFormat.SVG


def test_engine_render_full_document(engine, taro_quill_dir, taro_md):
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.PDF)

    assert len(result.artifacts) > 0
    assert result.format == OutputFormat.PDF
    assert result.artifacts[0].format == OutputFormat.PDF
    assert result.artifacts[0].mime_type == "application/pdf"


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
    assert isinstance(regions, list) and len(regions) > 0
    for r in regions:
        assert set(("field", "page", "rect", "span")).issubset(r.keys())
        assert isinstance(r["field"], str)
        assert isinstance(r["page"], int)
        assert isinstance(r["rect"], list) and len(r["rect"]) == 4
        assert r["span"] is None or (isinstance(r["span"], list) and len(r["span"]) == 2)
        # Plate-space spellings (`$body`, `$cards.<kind>.<ordinal>.`) must not
        # reach a caller: they name nothing any document API accepts.
        assert not r["field"].startswith("$"), (
            f"untranslated plate address: {r['field']}"
        )
    body_segments = [r for r in regions if r["field"] == "main.body"]
    assert body_segments, (
        f"expected a `main.body` region; got: {[r['field'] for r in regions]}"
    )
    assert any(r["span"] is not None for r in body_segments), (
        "a `main.body` content segment carries a content span"
    )


def test_parse_error_carries_diagnostics():
    """Parse failures raise QuillmarkError with a non-empty `.diagnostics` list.

    Matches WASM contract: single exception type, diagnostics uniformly attached.
    """
    invalid_md = """~~~card-yaml
$quill: test_quill
$kind: main
title: [unclosed bracket
~~~

Content
"""
    with pytest.raises(QuillmarkError) as exc_info:
        Document.from_markdown(invalid_md)

    exc = exc_info.value
    assert hasattr(exc, "diagnostics"), "exception should carry .diagnostics list"
    assert len(exc.diagnostics) >= 1, "diagnostics must be non-empty"
    assert all(hasattr(d, "message") for d in exc.diagnostics)


def test_quill_load_error_carries_diagnostics(tmp_path):
    """Quill-loading failures surface as QuillmarkError with diagnostics.

    A malformed *config* still fails at load time: `Quill.from_path` validates
    the config eagerly; only backend resolution is deferred to render.
    """
    bogus = tmp_path / "not_a_quill"
    bogus.mkdir()
    (bogus / "Quill.yaml").write_text("quill: { name: x }\n")  # missing required keys

    with pytest.raises(QuillmarkError) as exc_info:
        Quill.from_path(str(bogus))

    exc = exc_info.value
    assert hasattr(exc, "diagnostics") and len(exc.diagnostics) >= 1, (
        "quill-load failure must expose at least one diagnostic"
    )
