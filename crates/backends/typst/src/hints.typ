// Word export hints. Each one emits only labelled metadata, so the PDF does
// not change. A misspelt kind, edge or field fails the compile with a message.
//
// A quill package that must stay publishable without Quillmark vendors this
// file unchanged.

#let _fields = (
  list: (bracket: true, required: ("v", "levels"), allowed: ("v", "name", "levels")),
  label: (bracket: true, required: ("level",), allowed: ("level",)),
  role: (bracket: true, required: ("name",), allowed: ("name", "next")),
  layout: (bracket: true, required: ("form",), allowed: ("form",)),
  chrome: (bracket: true, required: (), allowed: ()),
  keep: (bracket: false, required: (), allowed: ()),
  font: (bracket: false, required: ("family", "alt"), allowed: ("family", "alt")),
)

#let _level-fields = ("format", "prefix", "suffix", "underline", "visible", "indent")
#let _formats = ("decimal", "lower-letter", "upper-letter", "lower-roman", "upper-roman")

#let _check(kind, data, bracket) = {
  assert(type(kind) == str and kind in _fields,
    message: "hint: unknown kind " + repr(kind) + "; expected one of " + _fields.keys().join(", "))
  let spec = _fields.at(kind)
  assert(spec.bracket == bracket,
    message: "hint: " + kind + " is a " + if spec.bracket { "bracket; use hint" } else { "point; use hint-point" })
  for key in data.keys() {
    assert(key in spec.allowed, message: "hint " + kind + ": unknown field " + repr(key))
  }
  for key in spec.required {
    assert(key in data, message: "hint " + kind + ": missing field " + repr(key))
  }
  if kind == "list" {
    assert(data.v == 1, message: "hint list: v must be 1")
    assert(type(data.levels) == array and data.levels.len() > 0,
      message: "hint list: levels must be a non-empty array")
    for level in data.levels {
      for key in level.keys() {
        assert(key in _level-fields, message: "hint list: unknown level field " + repr(key))
      }
      assert(level.at("format", default: "decimal") in _formats,
        message: "hint list: format must be one of " + _formats.join(", "))
    }
  }
  if kind == "label" {
    assert(type(data.level) == int and data.level >= 1 and data.level <= 9,
      message: "hint label: level must be an integer from 1 to 9")
  }
  if kind == "layout" {
    assert(data.form == "tabbed", message: "hint layout: form must be \"tabbed\"")
  }
}

#let _marker(data) = [#metadata((kind: "__qm_hint__", ..data))<__qm_hint__>]

/// A point hint: `keep` between two blocks, or `font` once per face.
#let hint-point(kind, ..data) = {
  _check(kind, data.named(), false)
  _marker((hint: kind, ..data.named()))
}

/// A bracket hint around `body`: `list`, `label`, `role`, `layout` or
/// `chrome`. It closes under the element it opens in.
#let hint(kind, body, ..data) = {
  _check(kind, data.named(), true)
  _marker((hint: kind, edge: "open", ..data.named()))
  body
  _marker((hint: kind, edge: "close"))
}

/// `body`, then a `keep`: no page break between it and the block after.
#let keep-with-next(body) = {
  body
  hint-point("keep")
}
