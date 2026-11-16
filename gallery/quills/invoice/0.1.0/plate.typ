#import "@local/quillmark-helper:0.2.0" as qm

#set page(paper: "a4", margin: (x: 2cm, y: 2.2cm))
#set text(size: 10pt)

#let d = qm.data
#let symbols = (USD: "$", EUR: "€", GBP: "£", CAD: "CA$", AUD: "A$", JPY: "¥")
#let places = if d.currency == "JPY" { 0 } else { 2 }

// Typst has no grouped number format; this draws 1234.5 as 1,234.50.
#let money(x) = {
  let neg = x < 0
  let scaled = calc.round(calc.abs(x) * calc.pow(10, places))
  let whole = calc.floor(scaled / calc.pow(10, places))
  let frac = scaled - whole * calc.pow(10, places)
  let digits = str(int(whole))
  let groups = ()
  while digits.len() > 3 {
    groups.insert(0, digits.slice(digits.len() - 3))
    digits = digits.slice(0, digits.len() - 3)
  }
  groups.insert(0, digits)
  let s = groups.join(",")
  if places > 0 {
    let f = str(int(frac))
    s += "." + "0" * (places - f.len()) + f
  }
  (if neg { "−" } else { "" }) + symbols.at(d.currency, default: "") + s
}

#let terms = d.terms.value
#let due = if terms == "due_on_date" { d.terms.due } else if d.issued == none or terms == "" { none } else {
  let days = (due_on_receipt: 0, net_15: 15, net_30: 30, net_60: 60).at(terms)
  d.issued + duration(days: days)
}
#let fmt = "[day padding:none] [month repr:long] [year]"

#let amount(row) = if row.unit_price == none { none } else { row.quantity * row.unit_price }
#let subtotal = d.items.map(amount).filter(a => a != none).sum(default: 0)
#let tax = if d.tax_rate == none { none } else { subtotal * d.tax_rate / 100 }
#let total = subtotal + if tax == none { 0 } else { tax }

#grid(
  columns: (1fr, auto),
  [
    #text(size: 14pt, weight: "bold", qm.ink(d.seller, "name"))
    #stack(spacing: 0.5em,
      qm.ink(d.seller, "address"),
      qm.ink(d.seller, "email"),
      if d.seller.tax_id != "" [Tax ID: #qm.ink(d.seller, "tax_id")],
    )
  ],
  align(right)[
    #text(size: 22pt, weight: "bold", tracking: 1pt)[INVOICE] \
    #if d.number != "" [No. #qm.ink(d, "number") \ ]
    #if d.issued != none [Issued #qm.ink(d, "issued", fmt) \ ]
    #if due != none {
      qm.region("terms")[*Due #due.display(fmt)*]
    }
  ],
)

#v(1.2em)
#if d.client.name != "" or d.client.address != "" [
  #text(size: 8pt, fill: luma(90), weight: "bold")[BILL TO] \
  #text(weight: "bold", qm.ink(d.client, "name")) \
  #qm.ink(d.client, "address")
  #if d.client.reference != "" [\ Purchase order #qm.ink(d.client, "reference")]
]

#v(1.2em)
#table(
  columns: (1fr, auto, auto, auto),
  stroke: (x, y) => if y == 0 { (bottom: 0.8pt) } else { (bottom: 0.3pt + luma(200)) },
  align: (x, y) => if x == 0 { left } else { right },
  table.header([*Description*], [*Qty*], [*Unit price*], [*Amount*]),
  ..d.items.map(row => (
    qm.ink(row, "description"),
    ..if row.unit_price == none { ([], [], []) } else {(
      qm.ink(row, "quantity"),
      qm.region(row.at("$path") + "unit_price", money(row.unit_price)),
      money(amount(row)),
    )},
  )).flatten(),
)

#align(right, grid(
  columns: (auto, 7em),
  row-gutter: 0.6em,
  align: right,
  [Subtotal], money(subtotal),
  ..if tax != none { (qm.region("tax_rate")[Tax (#d.tax_rate%)], money(tax)) },
  grid.cell(colspan: 2, line(length: 100%, stroke: 0.8pt)),
  [*Total due (#d.currency)*], [*#money(total)*],
))

#let body = d.at("$body", default: "")
#if body != "" {
  v(1.5em)
  body
}

#let p = d.payment
#if p.account != "" or p.bank != "" {
  v(1.5em)
  block(fill: luma(245), inset: 10pt, width: 100%)[
    #text(size: 8pt, fill: luma(90), weight: "bold")[HOW TO PAY] \
    #if p.account_name != "" [Account name: #qm.ink(p, "account_name") \ ]
    #if p.bank != "" [Bank: #qm.ink(p, "bank") \ ]
    #if p.account != "" [Account: #qm.ink(p, "account") \ ]
    #if p.routing != "" [Routing: #qm.ink(p, "routing") \ ]
    #if d.number != "" [Reference: #qm.ink(d, "number")]
  ]
}
