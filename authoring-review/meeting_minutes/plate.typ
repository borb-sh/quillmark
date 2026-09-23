#import "@local/quillmark-helper:0.1.0": data, display, field-region, signature-field

#let cap(s) = if s == "" { s } else { upper(s.first()) + s.slice(1) }
#let clock(address) = display(address, "[hour repr:12 padding:none]:[minute] [period case:lower]")
#let person(p) = if p.role == "" { p.name } else [#p.name (#p.role)]

#set document(title: data.organization + " minutes")
#set page(
  paper: "us-letter",
  margin: 1in,
  header: if data.confidential {
    align(center, text(fill: red.darken(20%), weight: "bold", tracking: 2pt)[CONFIDENTIAL])
  },
  footer: context align(center, text(size: 9pt, fill: luma(90))[
    #data.body_name minutes · page #counter(page).display() of #counter(page).final().first()
  ]),
)
#set text(size: 11pt)
#set par(justify: true)
#show table: set par(justify: false)
#set heading(numbering: "1.")
#show heading.where(level: 1): set text(size: 13pt)

#align(center)[
  #text(size: 17pt, weight: "bold", data.organization) \
  #text(size: 12pt)[Minutes of the #cap(data.meeting_type) Meeting of the #data.body_name] \
  #display("date", "[weekday], [month repr:long] [day padding:none], [year]")
]

#v(0.5em)

#{
  let rows = ()
  if data.called_to_order != none { rows += ([*Called to order*], clock("called_to_order")) }
  if data.location != "" { rows += ([*Location*], [#data.location]) }
  if data.chair != "" { rows += ([*Presiding*], [#data.chair]) }
  if data.secretary != "" { rows += ([*Recording*], [#data.secretary]) }
  if rows.len() > 0 { table(columns: (auto, 1fr), stroke: none, inset: (x: 0pt, y: 3pt), column-gutter: 1em, ..rows) }
}

#let present = data.attendees.filter(a => a.present)
#let absent = data.attendees.filter(a => not a.present)
#let voting-present = present.filter(a => a.voting).len()

#if data.attendees.len() > 0 {
  field-region("attendees")[
    *Present:* #if present.len() == 0 [none] else { present.map(person).join(", ") }. \
    *Absent:* #if absent.len() == 0 [none] else { absent.map(person).join(", ") }.
  ]
}

#if data.quorum > 0 {
  field-region("quorum")[
    #if voting-present >= data.quorum [
      With #voting-present voting members present, a quorum of #data.quorum was established.
    ] else [
      *Only #voting-present voting members were present; quorum (#data.quorum) was not met.*
    ]
  ]
}

#data.at("$body", default: "")

#let items = data.at("$cards", default: ()).filter(c => c.at("$kind", default: none) == "agenda_item")
#let all-actions = ()

#for card in items {
  let path = card.at("$path")
  field-region(path + "topic", heading(level: 1, card.topic))
  if card.presenter != "" {
    field-region(path + "presenter", emph[Presented by #card.presenter.])
    parbreak()
  }
  card.at("$body", default: "")

  let o = card.outcome
  if o.value == "motion" {
    block(inset: (left: 10pt, y: 6pt), stroke: (left: 2pt + luma(150)), width: 100%)[
      *Motion:* #o.motion_text \
      #field-region(path + "outcome.moved_by")[Moved by #o.moved_by]#if o.seconded_by != "" [, seconded by #o.seconded_by]. \
      Vote: #o.votes_for for, #o.votes_against against, #o.abstentions abstaining. \
      #field-region(path + "outcome.result", if o.result == "" [_No result recorded._] else [*Motion #o.result.*])
    ]
  }

  if card.action_items.len() > 0 {
    field-region(path + "action_items", table(
      columns: (auto, 1fr, auto),
      stroke: 0.5pt + luma(180),
      table.header([*Owner*], [*Action*], [*Due*]),
      ..card.action_items.map(a => (
        a.owner,
        a.task,
        if a.due == none [—] else { a.due.display("[month repr:short] [day padding:none]") },
      )).flatten()
    ))
  }
  all-actions += card.action_items.map(a => (topic: card.topic, ..a))
}

#if all-actions.len() > 0 [
  #heading(numbering: none)[Action item summary]
  #table(
    columns: (auto, 1fr, auto, auto),
    stroke: 0.5pt + luma(180),
    table.header([*Owner*], [*Action*], [*Item*], [*Due*]),
    ..all-actions.sorted(key: a => a.owner).map(a => (
      a.owner, a.task, a.topic,
      if a.due == none [—] else { a.due.display("[year]-[month]-[day]") },
    )).flatten()
  )
]

#v(1em)
#if data.adjourned != none [
  There being no further business, the meeting adjourned at #clock("adjourned").
]
#if data.next_meeting != none [
  The next meeting is scheduled for #display("next_meeting", "[month repr:long] [day padding:none], [year]").
]

#v(2em)
Respectfully submitted,

#signature-field("secretary_signature", width: 220pt, height: 40pt, field: "secretary")
#line(length: 220pt, stroke: 0.5pt)
#v(-0.6em)
#data.secretary, Secretary
