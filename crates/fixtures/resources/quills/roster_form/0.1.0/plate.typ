#import "@local/quillmark-helper:0.1.0": data

// The matrix reaches the plate total: every member present, in declaration
// order, carrying its own title and group. Printing the whole vocabulary needs
// no second copy of it here, and an unheld member's `detail` is its blank, so
// the tick and the annotation read without a guard.
#underline(data.title)

#for (id, member) in data.qualifications {
  [#(if member.held { "[x]" } else { "[ ]" }) #member.title (#member.group) #member.detail]
  linebreak()
}

#for line in data.endorsements [#line \ ]

#for tour in data.tours [#tour.unit — #tour.duration \ ]
