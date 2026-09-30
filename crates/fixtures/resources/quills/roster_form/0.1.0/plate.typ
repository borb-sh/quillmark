#import "@local/quillmark-helper:0.1.0": data, field-region, ink, roster

// `roster` is the vocabulary the page prints: every member in roster order,
// held or not, then each item the document adds. Printing the roster as the
// schema lists it is the type's purpose, so the plate asserts the order on
// every render.
#let rows = roster(data, "qualifications")
#assert.eq(
  rows.slice(0, 5).map(row => row.id),
  ("sq_cc_candidate", "flight_cc", "dodin_ops", "dco", "cyber_200"),
  message: "the roster reached the plate as " + repr(rows.map(row => row.id)),
)

#underline(data.title)

// Each row claims its member's address, so an unticked box is clickable too. A
// held row's columns read without a guard; an added item's title is a cell,
// so its ink keeps its own address inside the claim.
#for row in rows {
  let title = if row.held { ink(row.value).at("title", default: row.title) } else { row.title }
  field-region(row.path)[#(if row.held { "[x]" } else { "[ ]" }) #title]
  if row.held [ #ink(row.value).detail]
  linebreak()
}

#for line in data.endorsements [#line \ ]

#for tour in data.tours [#tour.unit — #tour.duration \ ]
