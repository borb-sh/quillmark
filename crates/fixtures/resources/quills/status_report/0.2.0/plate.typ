#import "@local/quillmark-helper:0.2.0" as qm

#set page(paper: "us-letter", margin: 1in)
#set text(size: 11pt)

// A stored enum value is an id. The words it prints are the plate's.
#let state-text = (on_track: "On track", at_risk: "At risk", blocked: "Blocked")

// The banner's glyphs are drawn here rather than by the field, so nothing ties
// a click on them to `state` until `qm.region` claims them.
#let banner(level) = box(fill: luma(230), inset: 6pt, radius: 2pt)[*#upper(state-text.at(level))*]

// An enum's blank is a value no `values:` list holds: here, nobody has said
// where the project stands. Guarding on it is what keeps an `else` over the
// three declared states from rendering one nobody picked.
#if qm.data.state != "" {
  qm.region("state")[#banner(qm.data.state)]
}

// PDF/UA-1 rejects an empty heading, so a blank title prints no heading.
#if qm.data.project != "" [= #qm.ink(qm.data, "project")]

// A blank date is `none`. `qm.ink` places the date's ink, so the printed date
// stays click-to-edit; `qm.data.issued` is the `datetime` itself, for
// comparison, components, or a package that formats its own.
#if qm.data.issued != none [
  Issued #qm.ink(qm.data, "issued", "[day padding:none] [month repr:long] [year]")
]

#if qm.data.lead != "" [Lead: #qm.ink(qm.data, "lead")]

#qm.data.at("$body", default: "")

// `$kind` is document-defined: a card may name a kind this quill does not
// declare, so let every other kind fall through. A field declared on the kind
// arrives filled the way a `main` field does, so `card.title` is a plain read
// while `card.at("$body")` — a `$`-sigiled key — still takes its default.
#for card in qm.data.at("$cards", default: ()) {
  if card.at("$kind", default: none) == "milestone" {
    // Every iteration reads through one loop variable, so a print of
    // `card.title` names no card. Its ink is the card's own: compute with
    // `card`, and print and format with `qm.ink(card, ..)`.
    if card.title != "" { heading(level: 2, qm.ink(card, "title")) }
    if card.due != none {
      [Due #qm.ink(card, "due", "[year]-[month]-[day]")]
      parbreak()
    }
    card.at("$body", default: "")
  }
}
