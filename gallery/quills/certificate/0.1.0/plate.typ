#import "@local/quillmark-helper:0.2.0" as qm

#let d = qm.data
#let colors = (gold: rgb("#b08d2b"), navy: rgb("#1f3a68"), crimson: rgb("#8c1c2c"))
#let accent = colors.at(d.seal, default: rgb("#1f3a68"))

#set page(paper: d.paper, flipped: true, margin: 0.6in)
#set text(font: "Gelasio", size: 12pt)
#set document(title: if d.heading != "" { qm.str(d.heading) } else { "Certificate" })

#let frame(body) = block(width: 100%, height: 100%, stroke: 2pt + accent, inset: 6pt,
  block(width: 100%, height: 100%, stroke: 0.6pt + accent, inset: 0.4in, body))

// Shrinks a name until it fits one line of the frame.
#let fit(body, size) = layout(region => {
  let s = size
  while s > 14pt and measure(text(size: s, body)).width > region.width { s -= 2pt }
  text(size: s, body)
})

#let seal-mark = if d.seal != "" {
  circle(radius: 0.55in, fill: accent, stroke: 2pt + accent.lighten(40%),
    align(center + horizon, text(fill: white, font: "Cinzel", size: 9pt, weight: "bold")[SEAL]))
}

#let signatures = if d.signatories.len() > 0 {
  grid(
    columns: d.signatories.len() * (1fr,),
    column-gutter: 0.5in,
    ..d.signatories.map(s => align(center)[
      #line(length: 100%, stroke: 0.6pt)
      #v(-0.4em)
      #qm.ink(s, "name") \
      #text(size: 10pt, style: "italic", qm.ink(s, "role"))
    ])
  )
}

#let names = qm.ink(d, "recipients")

#let page-for(i, name) = frame(align(center)[
  #v(0.15in)
  #text(font: "Cinzel", size: 30pt, weight: "bold", fill: accent, tracking: 2pt,
    qm.region("heading", if d.heading != "" { upper(qm.str(d.heading)) } else [CERTIFICATE]))
  #v(0.25in)
  #text(size: 13pt, style: "italic")[This is presented to]
  #v(0.1in)
  #fit(text(style: "italic", name), 40pt)
  #v(-0.15in)
  #line(length: 70%, stroke: 0.6pt + accent)
  #v(0.05in)
  #block(width: 80%, text(size: 14pt, qm.ink(d, "citation")))
  #v(0.15in)
  #if d.issuer != "" [#text(font: "Cinzel", size: 13pt, qm.ink(d, "issuer")) \ ]
  #if d.issued != none [#qm.ink(d, "issued", "[day padding:none] [month repr:long] [year]")]
  #v(1fr)
  #grid(columns: (1fr, auto), column-gutter: 0.4in, align: bottom, signatures, seal-mark)
  #if d.certificate_id != "" {
    place(bottom + right, dy: 0.3in, text(size: 7pt, fill: luma(110),
      qm.region("certificate_id", qm.str(d.certificate_id) + if d.recipients.len() > 1 { "-" + str(i + 1) })))
  }
])

#if names == none or names.len() == 0 {
  page-for(0, [])
} else {
  for (i, name) in names.enumerate() {
    if i > 0 { pagebreak() }
    page-for(i, name)
  }
}
