#import "@local/quillmark-helper:0.2.0" as qm
#import "@local/tonguetoquill-usaf-memo:3.0.0": backmatter, date-pattern, frontmatter, indorsement, mainmatter

// The package computes with these as strings (`upper`, joins, emptiness), so
// each `text` field reaches it through `qm.str`.
#let strs(values) = values.map(qm.str)

// A memo has no "no style" state, so the blank takes the package's default.
// Resolved once here: `frontmatter` and the date's pattern must agree.
#let memo_style = if qm.data.memo_style != "" { qm.data.memo_style } else { "usaf" }

// A blank date is dated by hand at signing, so it renders as a text widget bound
// to the date's schema address: typeable in a PDF reader, and the one thing that
// gives the *unfilled* date a region to click. The package seats it in the slot
// a printed date would fill (`date-placeholder`).
//
// Styled to match the printed date it stands in for, since the two are
// alternatives for the same slot: the memo's Times-alike body face at the body
// size (auto-size would fit the widget box instead, landing well under body size
// and shrinking further as the signer types), and flush right, so a typed date
// ends at the margin exactly where `display-date` puts a filled one. Right is
// unreachable through geometry here, the widget being wider than the text it
// will hold either way.
//
// Sized in multiples of that face's own size rather than inches, because
// `font_size` is a document field with no declared ceiling: an inch width would
// stay put while the text inside it grew, and a fixed size clips where auto-size
// would shrink. The longest date either memo style produces sets just over 8em
// in Times ("September 28, 2026", the DAF ordering, at 8.03em; USAF's "28
// September 2026" is 7.78em), so 10em clears the worst case at any body size.
// Wider than `date-placeholder`'s reserved span on purpose: the widget hangs off
// that span's right edge and overruns leftwards, into the whitespace a printed
// date grows into.
#let dating-field(name, field) = {
  let date_size = qm.data.font_size * 1pt
  qm.form-field(
    name,
    type: "text",
    width: 10 * date_size,
    height: date_size,
    field: field,
    font: "times",
    size: date_size,
    align: "right",
  )
}

// Frontmatter configuration
#show: frontmatter.with(
  // Letterhead configuration
  letterhead_title: qm.str(qm.data.letterhead_title),
  letterhead_caption: strs(qm.data.letterhead_caption),
  letterhead_seal_subtitle: qm.str(qm.data.letterhead_seal_subtitle),
  // Every enum branch below covers `values ∪ blank`: an `else` that swallows the
  // blank renders a variant nobody picked. Here the blank omits the seal, which
  // is what `frontmatter`'s own `letterhead_seal: none` default means.
  ..if qm.data.letterhead_seal != "" {
    (letterhead_seal: if qm.data.letterhead_seal == "dod" {
      image("assets/dod_seal.png", alt: "Seal of the Department of Defense")
    } else {
      image("assets/dow_seal.png", alt: "Seal of the Department of War")
    })
  },

  // Date. `qm.data.date` is the native `datetime` and would render identically,
  // but its ink would be born inside the package and carry no schema address.
  // `qm.ink` places the field's ink instead: the glyphs are born in the
  // generated helper, so the memo date stays click-to-edit however deep the
  // package formats it. A blank date yields `none`, which seats `dating_field`
  // in the date's slot instead.
  date: qm.ink(qm.data, "date", date-pattern(memo-style: memo_style)),
  dating_field: dating-field("Date", "date"),

  // Receiver information
  memo_for: strs(qm.data.memo_for),

  // Sender information (omitted for Memorandum for Record). Every declared
  // field arrives filled, so the empty *value* is the signal here, not absence.
  ..if qm.data.memo_from.len() > 0 { (memo_from: strs(qm.data.memo_from)) },

  // Subject line
  subject: qm.data.subject,

  references: qm.data.references,
  footer_tag_line: qm.data.tag_line,
  classification_level: qm.data.classification.value,
  dissemination: qm.str(qm.data.dissemination),

  // CUI designation indicator block fields (DoDM 5200.48). `classification`
  // declares a `CUI` variant, so these four exist only where the discriminant
  // reads CUI and the branch is what makes reading them total: inside it every
  // declared field of that world is present, outside it none is. The package's
  // own `cui_*: none` defaults cover the worlds that omit them.
  ..if qm.data.classification.value == "CUI" {
    (
      cui_controlled_by: qm.str(qm.data.classification.controlled_by),
      cui_category: qm.str(qm.data.classification.category),
      cui_limited_dissemination: qm.str(qm.data.classification.limited_dissemination),
      cui_poc: qm.str(qm.data.classification.poc),
    )
  },

  // USAF vs DAF memorandum style (date format, body indentation). `frontmatter`
  // asserts membership of ("usaf", "daf"), which the blank resolution above
  // already guarantees.
  memo_style: memo_style,

  // Font size
  font_size: qm.data.font_size * 1pt,

  // List recipients in vertical list
  memo_for_cols: 1,
)

// Mainmatter. The body's region needs no recovery step here: the package's
// render-body rebuilds paragraphs through a state buffer (AFH 33-337
// auto-numbering), but the rebuilt glyphs keep their spans, which is what
// the backend reads regions from.
#mainmatter[
  #qm.data.at("$body")
]

// Backmatter
#backmatter(
  // Signature block
  signature_block: strs(qm.data.signature_block),
  signing_field: qm.signature-field("Signature", field: "signature_block"),

  cc: strs(qm.data.cc),
  distribution: strs(qm.data.distribution),
  attachments: strs(qm.data.attachments),
)

// Indorsements - iterate through CARDS array and filter by CARD tag
#for (i, card) in qm.data.at("$cards").enumerate() {
  if card.at("$kind", default: none) == "indorsement" {
    // The quillmark helper leaves an unset/whitespace-only markdown body as
    // the empty string `""`; only non-empty bodies are eval'd into content.
    // Pass truly empty content (`[]`) in the empty case so indorsement can
    // collapse the body's surrounding spacing.
    let body = card.at("$body", default: "")
    let body_content = if type(body) == str { [] } else { body }
    // Per AFH 33-337 Ch. 14, an indorsement is dated when the endorser signs
    // it (distinct from the originating memo's date). The signing date is
    // generally unknown at compile time and filled in by hand, so a blank date
    // renders a fill-in line rather than stamping the compile date.
    // The card's own date ink, addressed through its `$path` prefix, so a
    // per-card call yields a per-card region even though every iteration
    // shares one `card` loop variable. `none` for a blank date, which is the
    // fill-in case below.
    let resolved_date = qm.ink(card, "date", date-pattern(memo-style: memo_style))
    // A filled date regions through that call; a blank one through the widget.
    let dating_field = dating-field("Ind_" + str(i) + "_Date", card.at("$path") + "date")
    // The card's `$path` prefix composes its canonical schema addresses
    // (`$cards.indorsement.<n>.…`, per-kind ordinal): the absolute loop
    // index `i` is NOT that ordinal once kinds interleave, so it stays a
    // widget-name suffix only. The card body's region rides its own glyph
    // spans through the package rebuild, per-card because each card's body
    // has its own backend-generated eval site.
    indorsement(
      from: qm.str(card.from),
      // `for` is a Typst keyword, so this one declared field cannot be reached
      // as `card.for`; the `.at` is forced by syntax, not by any doubt that the
      // field is there.
      to: qm.str(card.at("for")),
      signature_block: strs(card.signature_block),
      signing_field: qm.signature-field(
        "Ind_" + str(i) + "_Signature",
        field: card.at("$path") + "signature_block",
      ),
      // Same shape: `indorsement` asserts `format`'s membership, and an
      // indorsement has no "no layout" state. `action` needs no such guard —
      // the package reads a blank action as "no action line".
      ..if card.format != "" { (format: card.format) },
      date: resolved_date,
      dating_field: dating_field,
      action: card.action,
      body_content,
    )
  }
}
