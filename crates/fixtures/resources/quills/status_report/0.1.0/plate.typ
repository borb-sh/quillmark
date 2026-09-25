#import "@local/quillmark-helper:0.1.0": data, display, field-region, ink

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

// `$kind` is document-defined: a card may name a kind this quill does not
// declare, so let every other kind fall through. A field declared on the kind
// arrives filled the way a `main` field does, so `card.title` is a plain read
// while `card.at("$body")` — a `$`-sigiled key — still takes its default.
#for card in data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "milestone" {
    // Every iteration reads through one loop variable, so a print of
    // `card.title` names no card. Its ink is the card's own: compute with
    // `card`, print with `ink(card)`, and format a date with `display(card, ..)`.
    heading(level: 2, ink(card).title)
    if card.due != none {
      [Due #display(card, "due", "[year]-[month]-[day]")]
      parbreak()
    }
    card.at("$body", default: "")
  }
}
