#import "@local/quillmark-helper:0.2.0" as qm
#let hints = qm.hints

#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 12pt)
#set par(spacing: 12pt)

#hints.hint-point("font", family: "Roboto", alt: "Arial")
#hints.hint("chrome", place(top + center, dy: -36pt)[*NUMBERED LETTER*])

#hints.hint("role", name: "Letter Heading", next: "Letter Body")[
  #hints.hint("layout", form: "tabbed", grid(columns: (72pt, 1fr), [FROM:], [#qm.data.from]))
  #hints.hint("layout", form: "tabbed", grid(columns: (72pt, 1fr), [SUBJECT:], [#qm.data.subject]))
]

#let levels = (
  (format: "decimal", suffix: ".  ", indent: 0pt),
  (format: "lower-letter", suffix: ".  ", indent: 18pt),
)

#hints.hint("list", name: "Letter Paragraph", levels: levels)[
  #hints.hint("role", name: "Letter Body")[
    #hints.hint("label", level: 1)[1.#h(1em)]The first paragraph is long enough that it wraps onto a second line, so its number has text after it on both lines of the page.

    #h(18pt)#hints.hint("label", level: 2)[a.#h(1em)]A sub-paragraph sits under its parent's text.

    #hints.hint("label", level: 1)[2.#h(1em)]The second paragraph is the last one.
  ]
]

#hints.keep-with-next[Respectfully,]

#pad(left: 252pt)[SIGNER NAME]
