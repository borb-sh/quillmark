"""The parity corpus, `crates/fixtures/resources/parity/parity.json`, across the
Python boundary: every entry's content crosses out of a parse and a stored load.
The semantics are `crates/quillmark/tests/parity.rs`'s (prose/canon/PARITY.md
§ "The corpus")."""

import json

import pytest
from quillmark import Document

from conftest import RESOURCES_PATH

CORPUS = json.loads((RESOURCES_PATH / "parity" / "parity.json").read_text(encoding="utf-8"))
SPELLED = [e for e in CORPUS if e["markdown"] is not None]
FRONTMATTER = "~~~\n$quill: table_demo@0.1.0\n$kind: main\ntitle: Parity\n~~~\n"


def name(entry):
    return entry["name"]


def parse(body):
    return Document.from_markdown(f"{FRONTMATTER}\n{body}\n")


@pytest.mark.parametrize("entry", SPELLED, ids=name)
def test_a_spelling_parses_to_its_content_warning_at_the_body(entry):
    doc = parse(entry["markdown"])
    assert doc.body == entry["content"]
    dropped = [{"construct": w.args["construct"], "count": w.args["count"]} for w in doc.warnings]
    assert dropped == entry["signals"]["import"]
    assert {(w.code, w.path) for w in doc.warnings} <= {("parse::dropped_construct", "main.body")}


@pytest.mark.parametrize("entry", CORPUS, ids=name)
def test_stored_content_reads_back(entry):
    stored = json.loads(parse("").to_stored())
    stored["main"]["body"] = entry["content"]
    doc = Document.from_stored(json.dumps(stored))
    assert doc.body == entry["content"]
    assert Document.from_stored(doc.to_stored()).body == entry["content"]
