// Word export hints. Each one emits only labelled metadata, so the PDF does
// not change. A misspelt kind or field, or a value of the wrong type, fails the
// compile with a message.
//
// A quill package that must stay publishable without Quillmark vendors this
// file unchanged.

#let _fields = (
  list: (bracket: true, required: ("levels",), fields: (name: str, levels: array)),
  label: (bracket: true, required: ("level",), fields: (level: int)),
  role: (bracket: true, required: ("name",), fields: (name: str, next: str)),
  layout: (bracket: true, required: ("form",), fields: (form: str)),
  chrome: (bracket: true, required: (), fields: (:)),
  keep: (bracket: false, required: (), fields: (:)),
  font: (bracket: false, required: ("family", "alt"), fields: (family: str, alt: str)),
)

#let _level-fields = (
  format: str,
  prefix: str,
  suffix: str,
  underline: bool,
  visible: bool,
  indent: length,
)
#let _formats = ("decimal", "lower-letter", "upper-letter", "lower-roman", "upper-roman")

#let _typed(what, fields, data) = {
  for (key, value) in data {
    assert(key in fields, message: what + ": unknown field " + repr(key))
    let want = fields.at(key)
    assert(type(value) == want,
      message: what + ": " + key + " must be " + repr(want) + ", got " + repr(type(value)))
  }
}

#let _check(kind, data, bracket) = {
  assert(type(kind) == str and kind in _fields,
    message: "hint: unknown kind " + repr(kind) + "; expected one of " + _fields.keys().join(", "))
  let spec = _fields.at(kind)
  assert(spec.bracket == bracket,
    message: "hint: " + kind + " is a " + if spec.bracket { "bracket; use hint" } else { "point; use hint-point" })
  _typed("hint " + kind, spec.fields, data)
  for key in spec.required {
    assert(key in data, message: "hint " + kind + ": missing field " + repr(key))
  }
  if kind == "list" {
    assert(data.levels.len() > 0, message: "hint list: levels must be a non-empty array")
    for level in data.levels {
      assert(type(level) == dictionary,
        message: "hint list: a level must be a dictionary, got " + repr(type(level)))
      _typed("hint list level", _level-fields, level)
      assert(level.at("format", default: "decimal") in _formats,
        message: "hint list: format must be one of " + _formats.join(", "))
    }
  }
  if kind == "label" {
    assert(data.level >= 1 and data.level <= 9,
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
