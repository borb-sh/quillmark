"""The values form: ``reader.values`` and ``writer.set_values``.

The shape's semantics are core's (``core/src/quill/values.rs``,
``core/src/writer.rs``). At this boundary the questions are narrower: does the
shape arrive as a plain dict with every key present, does ``None`` cross both
ways where an absent key is untouched, does the write reach core at both
scopes, and do refusals arrive as diagnostics carrying their own ``path``.
"""

import pytest

from quillmark import Quill
from conftest import raises_edit_code


QUILL_YAML = """quill:
  name: py_values
  version: "1.0"
  backend: typst
  description: Values form coverage

typst:
  plate_file: plate.typ

main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
    qty:
      type: integer
      default: 1
    paragraphs:
      type: array
      items:
        type: richtext

card_kinds:
  line_item:
    fields:
      desc:
        type: richtext
        inline: true
      qty:
        type: integer
"""

MD = """~~~card-yaml
$quill: py_values@1.0
$kind: main
$ext:
  app:
    k: 1
subject: Hello **world**
note: a *literal* line
paragraphs:
  - Para **one**
~~~

Body prose.

~~~card-yaml
$kind: line_item
desc: Widget __A__
qty: "3"
~~~
Item note.
"""


def make_quill(tmp_path):
    quill_dir = tmp_path / "quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(QUILL_YAML)
    (quill_dir / "plate.typ").write_text("#set page(width: 100pt, height: 100pt)\n")
    return Quill.from_path(str(quill_dir))


def test_values_reads_every_axis_content_as_text_and_scalars_as_stored(tmp_path):
    quill = make_quill(tmp_path)
    values = quill.reader(quill.parse(MD)).values()

    assert values["fields"]["subject"] == "Hello **world**"
    assert values["fields"]["note"] == "a *literal* line"
    assert values["fields"]["paragraphs"] == ["Para **one**"]
    assert "qty" not in values["fields"], "an absent field is not its `default`"
    assert values["body"] == "Body prose."
    assert values["ext"] == {"app": {"k": 1}}

    (card,) = values["cards"]
    assert card == {
        "kind": "line_item",
        "fields": {"desc": "Widget **A**", "qty": "3"},
        "body": "Item note.",
        "ext": None,
    }


def test_none_crosses_both_ways(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(
        "~~~card-yaml\n$quill: py_values@1.0\n$kind: main\nsubject:\n~~~\n\n"
        "~~~card-yaml\nfoo: bar\n~~~\n"
    )
    values = quill.reader(doc).values()
    assert values["fields"]["subject"] is None, "a present-null is not authored-empty"
    assert values["ext"] is None
    assert values["cards"][0]["kind"] is None
    assert values["cards"][0]["fields"] == {"foo": "bar"}

    quill.writer(doc).set_values({"ext": {"app": {}}})
    assert quill.reader(doc).values()["ext"] == {"app": {}}
    quill.writer(doc).set_values({"ext": None})
    assert quill.reader(doc).values()["ext"] is None


def test_writing_back_an_unedited_read_changes_no_bytes(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(MD)
    before = doc.to_stored()
    quill.writer(doc).set_values(quill.reader(doc).values())
    assert doc.to_stored() == before


def test_an_absent_key_is_untouched_and_a_present_axis_is_replaced(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(MD)
    quill.writer(doc).set_values({"fields": {"subject": "Goodbye *world*"}})

    after = quill.reader(doc).values()
    assert after["fields"]["subject"] == "Goodbye *world*"
    assert "note" not in after["fields"], "an unnamed declared field is removed"
    assert after["body"] == "Body prose.", "the body key was absent"
    assert len(after["cards"]) == 1, "the cards key was absent"
    assert after["ext"] == {"app": {"k": 1}}


def test_the_card_scope_reads_and_writes_one_slot(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(MD)
    reader = quill.reader(doc)
    assert reader.values(card=0) == reader.values()["cards"][0]

    quill.writer(doc).set_values({"fields": {"desc": "Gadget"}}, card=0)
    assert reader.values(card=0)["fields"] == {"desc": "Gadget"}
    assert reader.values()["fields"]["subject"] == "Hello **world**"

    with raises_edit_code("edit::unknown_field") as excinfo:
        quill.writer(doc).set_values({"fields": {"bad": 1}}, card=0)
    assert excinfo.value.diagnostics[0].path == "cards.line_item[0].bad"

    with raises_edit_code("edit::index_out_of_range"):
        quill.writer(doc).set_values({}, card=7)


def test_an_undeclared_name_is_refused_under_its_own_path(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(MD)
    before = doc.to_stored()
    with raises_edit_code("edit::unknown_field") as excinfo:
        quill.writer(doc).set_values({"fields": {"nope": "x"}})
    assert excinfo.value.diagnostics[0].path == "main.nope"
    assert doc.to_stored() == before, "an all-or-nothing batch writes nothing"


def test_a_malformed_shape_raises_value_error(tmp_path):
    quill = make_quill(tmp_path)
    doc = quill.parse(MD)
    with pytest.raises(ValueError):
        quill.writer(doc).set_values({"feilds": {}})
