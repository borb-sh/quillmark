#import "@local/quillmark-helper:0.1.0": data, display, field-region

#set page(paper: "us-letter", margin: 1in)
#set text(size: 11pt)

// The banner's glyphs are drawn here rather than by the field, so nothing ties
// a click on them to `state` until `field-region` claims them.
#let banner(level) = box(fill: luma(230), inset: 6pt, radius: 2pt)[*#upper(level)*]

// An enum's blank is a value no `values:` list holds: here, nobody has said
// where the project stands. Guarding on it is what keeps an `else` over the
// three declared states from rendering one nobody picked.
#if data.state != "" {
  field-region("state")[#banner(data.state)]
}

= #data.project

// A blank date is `none`. `display` takes the field's *address*, so the printed
// date stays click-to-edit; `data.issued` is the `datetime` itself, for
// comparison, components, or a package that formats its own.
#if data.issued != none [
  Issued #display("issued", "[day padding:none] [month repr:long] [year]")
]

#if data.lead != "" [Lead: #data.lead]

#data.at("$body", default: "")

// `$kind` is document-defined: a kindless card carries none, so read it with a
// default and let every other kind fall through. A field declared on the kind
// arrives filled the way a `main` field does, so `card.title` is a plain read
// while `card.at("$body")` — a `$`-sigiled key — still takes its default.
#for card in data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "milestone" {
    heading(level: 2, card.title)
    // The card's own address, composed from its `$path` prefix: `display` takes
    // an address, so this one call site regions per card even though every
    // iteration shares one loop variable.
    if card.due != none {
      [Due #display(card.at("$path") + "due", "[year]-[month]-[day]")]
      parbreak()
    }
    card.at("$body", default: "")
  }
}
