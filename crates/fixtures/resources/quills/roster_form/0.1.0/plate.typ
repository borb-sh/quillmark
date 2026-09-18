#import "@local/quillmark-helper:0.1.0": data

// The matrix reaches the plate total and in roster order: every member present,
// carrying its own title and group. Declaration order is the whole point of the
// type — a chart prints its vocabulary as the schema groups it — and the
// backend's dict keys otherwise sort, so the plate asserts it on every render.
#let ids = data.qualifications.keys()
#assert.eq(
  ids,
  ("sq_cc_candidate", "flight_cc", "dodin_ops", "dco", "cyber_200"),
  message: "matrix members reached the plate as " + repr(ids),
)

#underline(data.title)

// An unheld member's columns are their blanks, so the tick and the annotation
// read without a guard.
#for (id, member) in data.qualifications {
  [#(if member.held { "[x]" } else { "[ ]" }) #member.title (#member.group) #member.detail]
  linebreak()
}

#for line in data.endorsements [#line \ ]

#for tour in data.tours [#tour.unit — #tour.duration \ ]
