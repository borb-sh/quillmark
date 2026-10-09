"""The parity corpus, `crates/fixtures/resources/parity/parity.json`, through the
Python binding's doors: its parse, storage load, body read, revise, validate and
render (prose/canon/PARITY.md § "The corpus")."""

import json

import pytest
from quillmark import Document, OutputFormat, Quill, Quillmark

from conftest import QUILLS_PATH, RESOURCES_PATH, _latest_version

CORPUS = json.loads((RESOURCES_PATH / "parity" / "parity.json").read_text(encoding="utf-8"))
SPELLED = [e for e in CORPUS if e["markdown"] is not None]
ANNOTATED = [e for e in CORPUS if "annotated" in e]
FRONTMATTER = "~~~\n$quill: table_demo@0.1.0\n$kind: main\ntitle: Parity\n~~~\n"


def name(entry):
    return entry["name"]


@pytest.fixture(scope="module")
def quill():
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "table_demo")))


def parse(body):
    return Document.from_markdown(f"{FRONTMATTER}\n{body}\n")


def holding(content):
    """A document whose main body is `content`, loaded from storage."""
    stored = json.loads(parse("").to_stored())
    stored["main"]["body"] = content
    return Document.from_stored(json.dumps(stored))


def dropped(warnings):
    return [
        {"construct": w.args["construct"], "count": w.args["count"]}
        for w in warnings
        if w.code == "parse::dropped_construct"
    ]


def anchors(content):
    return [m for m in content["marks"] if m["type"] == "anchor"]


def unanchored(content):
    return {**content, "marks": [m for m in content["marks"] if m["type"] != "anchor"]}


def assert_revised(doc, content, reimports):
    """A revise lands on `reimports` plus every anchor `content` held."""
    assert unanchored(doc.body) == unanchored(reimports)
    assert anchors(doc.body) == anchors(content)


def test_the_corpus_holds_every_kind_of_entry():
    assert SPELLED and ANNOTATED and any(e["markdown"] is None for e in CORPUS)


@pytest.mark.parametrize("entry", SPELLED, ids=name)
def test_a_spelling_parses_to_its_content_warning_at_the_body(entry):
    doc = parse(entry["markdown"])
    assert doc.body == entry["content"]
    assert dropped(doc.warnings) == entry["signals"]["import"]
    assert {w.path for w in doc.warnings} <= {"main.body"}


@pytest.mark.parametrize("entry", CORPUS, ids=name)
def test_stored_content_reads_back_round_trips_and_revises(entry, quill):
    content = entry["content"]
    reimports = entry.get("reimports", content)
    doc = holding(content)
    assert doc.body == content
    assert Document.from_stored(doc.to_stored()).body == content

    markdown = quill.reader(doc).body_markdown()
    again = parse(markdown)
    assert again.body == reimports
    assert again.warnings == []

    assert quill.writer(doc).revise_body(markdown) == []
    assert_revised(doc, content, reimports)


@pytest.mark.parametrize("entry", ANNOTATED, ids=name)
def test_an_annotated_read_imports_without_anchors_and_revises_keeping_them(entry, quill):
    content = entry["content"]
    imported = parse(entry["annotated"])
    assert imported.body == unanchored(content)
    assert imported.warnings == []

    doc = holding(content)
    assert quill.writer(doc).revise_body(entry["annotated"]) == []
    assert_revised(doc, content, entry.get("reimports", content))


@pytest.mark.parametrize("entry", CORPUS, ids=name)
def test_validate_and_render_report_the_entry_signals(entry, quill):
    signals = entry["signals"]
    doc = parse(entry["markdown"]) if entry["markdown"] is not None else holding(entry["content"])
    assert [d["code"] for d in quill.validate(doc)] == signals.get("validate", [])
    result = Quillmark().render(quill, doc, OutputFormat.SVG)
    assert [w.code for w in result.warnings] == signals["render"]
