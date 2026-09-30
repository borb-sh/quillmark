#import "@local/quillmark-helper:0.1.0": data, field-region

// The matrix reaches the plate total and in roster order: every member present,
// carrying its own title, then each item the document adds. Declaration order is
// the whole point of the type — a chart prints its vocabulary as the schema lists
// it — and the backend's dict keys otherwise sort, so the plate asserts it on
// every render.
#let roster = ("sq_cc_candidate", "flight_cc", "dodin_ops", "dco", "cyber_200")
#let ids = data.qualifications.keys()
#assert.eq(
  ids.slice(0, roster.len()),
  roster,
  message: "matrix members reached the plate as " + repr(ids),
)

#underline(data.title)

// An unheld member's columns are their blanks, so the tick and the annotation
// read without a guard. An added item's title is a cell with an address, which
// the region claims around the content its body places.
#for (id, member) in data.qualifications {
  let title = if id in roster { member.title } else {
    field-region(member.at("$path") + "title")[#member.title]
  }
  [#(if member.held { "[x]" } else { "[ ]" }) #title #member.detail]
  linebreak()
}

#for line in data.endorsements [#line \ ]

#for tour in data.tours [#tour.unit — #tour.duration \ ]
