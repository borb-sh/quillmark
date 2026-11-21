// Word export hints, one function per kind. Each one emits only labelled
// metadata, so the PDF does not change. A misspelt function or argument, or a
// value of the wrong type, fails the compile.
//
// A quill package that must stay publishable without Quillmark vendors this
// file unchanged.

#let _level-fields = (
  format: str,
  prefix: str,
  suffix: str,
  underline: bool,
  visible: bool,
  indent: length,
)
#let _formats = ("decimal", "lower-letter", "upper-letter", "lower-roman", "upper-roman")

#let _typed(what, field, value, want) = assert(type(value) == want,
  message: what + ": " + field + " must be " + repr(want) + ", got " + repr(type(value)))

#let _marker(data) = [#metadata((kind: "__qm_hint__", ..data))<__qm_hint__>]

/// A bracket around `body`. It closes under the element it opens in.
#let _bracket(kind, body, ..data) = {
  _marker((hint: kind, edge: "open", ..data.named()))
  body
  _marker((hint: kind, edge: "close"))
}

/// `body` is one numbered list, with one dictionary in `levels` per level:
/// `format`, `prefix`, `suffix`, `underline`, `visible`, `indent`. `name`
/// names its paragraph styles.
#let list(levels, body, name: none) = {
  _typed("hints.list", "levels", levels, array)
  assert(levels.len() > 0, message: "hints.list: levels must be a non-empty array")
  for level in levels {
    assert(type(level) == dictionary,
      message: "hints.list: a level must be a dictionary, got " + repr(type(level)))
    for (key, value) in level {
      assert(key in _level-fields, message: "hints.list level: unknown field " + repr(key))
      _typed("hints.list level", key, value, _level-fields.at(key))
    }
    assert(level.at("format", default: "decimal") in _formats,
      message: "hints.list level: format must be one of " + _formats.join(", "))
  }
  if name == none {
    _bracket("list", body, levels: levels)
  } else {
    _typed("hints.list", "name", name, str)
    _bracket("list", body, levels: levels, name: name)
  }
}

/// `body` is a printed paragraph number and the space after it, at `level`
/// (from 1) of the `list` around it.
#let label(level, body) = {
  _typed("hints.label", "level", level, int)
  assert(level >= 1 and level <= 9, message: "hints.label: level must be an integer from 1 to 9")
  _bracket("label", body, level: level)
}

/// The paragraphs of `body` take the paragraph style `name`, and the one
/// typed after them takes `next`.
#let role(name, body, next: none) = {
  _typed("hints.role", "name", name, str)
  if next == none {
    _bracket("role", body, name: name)
  } else {
    _typed("hints.role", "next", next, str)
    _bracket("role", body, name: name, next: next)
  }
}

/// `body` is a grid whose rows are each one paragraph of label, tab and text.
#let tabbed(body) = _bracket("layout", body, form: "tabbed")

/// `body` is a `place` that belongs to the page header or footer.
#let chrome(body) = _bracket("chrome", body)

/// No page break between the paragraph this ends and the one after.
#let keep() = _marker((hint: "keep"))

/// Text in `family` names `alt` as its face in Word. Once per face.
#let font(family, alt: none) = {
  _typed("hints.font", "family", family, str)
  _typed("hints.font", "alt", alt, str)
  _marker((hint: "font", family: family, alt: alt))
}
