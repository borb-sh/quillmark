#import "@local/quillmark-helper:0.2.0" as qm
#let hints = qm.hints

#set page(width: 612pt, height: 792pt, margin: 72pt)
#set text(size: 12pt)
#set par(spacing: 12pt)

#hints.font("Roboto", alt: "Arial")
#hints.chrome(place(top + center, dy: -36pt)[*NUMBERED LETTER*])

#hints.role("Letter Heading", next: "Letter Body")[
  #hints.tabbed(grid(columns: (72pt, 1fr), [FROM:], [#qm.data.from]))
  #hints.tabbed(grid(columns: (72pt, 1fr), [SUBJECT:], [#qm.data.subject]))
]

#let levels = (
  (format: "decimal", suffix: ".  ", indent: 0pt),
  (format: "lower-letter", suffix: ".  ", indent: 18pt),
)

#hints.list(levels, name: "Letter Paragraph")[
  #hints.role("Letter Body")[
    #hints.label(1)[1.#h(1em)]The first paragraph is long enough that it wraps onto a second line, so its number has text after it on both lines of the page.

    #h(18pt)#hints.label(2)[a.#h(1em)]A sub-paragraph sits under its parent's text.

    #hints.label(1)[2.#h(1em)]The second paragraph is the last one.
  ]
]

Respectfully,#hints.keep()

#pad(left: 252pt)[SIGNER NAME]
