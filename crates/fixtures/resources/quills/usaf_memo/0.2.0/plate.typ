#import "@local/quillmark-helper:0.1.0": data, display, form-field, signature-field
#import "@local/tonguetoquill-usaf-memo:3.0.0": backmatter, date-pattern, frontmatter, indorsement, mainmatter

// A memo has no "no style" state, so the blank takes the package's default.
// Resolved once here: `frontmatter` and the date's pattern must agree.
#let memo_style = if data.memo_style != "" { data.memo_style } else { "usaf" }

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
  let date_size = data.font_size * 1pt
  form-field(
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
  letterhead_title: data.letterhead_title,
  letterhead_caption: data.letterhead_caption,
  letterhead_seal_subtitle: data.letterhead_seal_subtitle,
  // Every enum branch below covers `values ∪ blank`: an `else` that swallows the
  // blank renders a variant nobody picked. Here the blank omits the seal, which
  // is what `frontmatter`'s own `letterhead_seal: none` default means.
  ..if data.letterhead_seal != "" {
    (letterhead_seal: image(
      if data.letterhead_seal == "dod" {
        "assets/dod_seal.png"
      } else {
        "assets/dow_seal.png"
      }
    ))
  },

  // Date. `data.date` is the native `datetime` and would render identically,
  // but its ink would be born inside the package and carry no schema address.
  // `display` places the field's *content* projection instead: the glyphs are
  // born in the generated helper, so the memo date stays click-to-edit however
  // deep the package formats it. A blank date yields `none`, which seats
  // `dating_field` in the date's slot instead.
  date: display("date", date-pattern(memo-style: memo_style)),
  dating_field: dating-field("Date", "date"),

  // Receiver information
  memo_for: data.memo_for,

  // Sender information (omitted for Memorandum for Record). Every declared
  // field arrives filled, so the empty *value* is the signal here, not absence.
  ..if data.memo_from.len() > 0 { (memo_from: data.memo_from) },

  // Subject line
  subject: data.subject,

  references: data.references,
  footer_tag_line: data.tag_line,
  classification_level: data.classification.value,
  dissemination: data.dissemination,

  // CUI designation indicator block fields (DoDM 5200.48). `classification`
  // declares a `CUI` variant, so these four exist only where the discriminant
  // reads CUI and the branch is what makes reading them total: inside it every
  // declared field of that world is present, outside it none is. The package's
  // own `cui_*: none` defaults cover the worlds that omit them.
  ..if data.classification.value == "CUI" {
    (
      cui_controlled_by: data.classification.controlled_by,
      cui_category: data.classification.category,
      cui_limited_dissemination: data.classification.limited_dissemination,
      cui_poc: data.classification.poc,
    )
  },

  // USAF vs DAF memorandum style (date format, body indentation). `frontmatter`
  // asserts membership of ("usaf", "daf"), which the blank resolution above
  // already guarantees.
  memo_style: memo_style,

  // Font size
  font_size: data.font_size * 1pt,

  // List recipients in vertical list
  memo_for_cols: 1,
)

// Mainmatter. The body's region needs no recovery step here: the package's
// render-body rebuilds paragraphs through a state buffer (AFH 33-337
// auto-numbering), but the rebuilt glyphs keep their spans, which is what
// the backend reads regions from.
#mainmatter[
  #data.at("$body")
]

// Backmatter
#backmatter(
  // Signature block
  signature_block: data.signature_block,
  signing_field: signature-field("Signature", field: "signature_block"),

  cc: data.cc,
  distribution: data.distribution,
  attachments: data.attachments,
)

// Indorsements - iterate through CARDS array and filter by CARD tag
#for (i, card) in data.at("$cards").enumerate() {
  // `$kind` is document-defined: a card block with no `$kind:` line carries
  // none, so read it with a default rather than a bare `.at`.
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
    // The card's own address, composed from its `$path` prefix: `display`
    // takes an address, so a per-card call yields a per-card region even
    // though every iteration shares one `card` loop variable. `none` for a
    // blank date, which is the fill-in case below.
    let resolved_date = display(
      card.at("$path") + "date",
      date-pattern(memo-style: memo_style),
    )
    // A filled date regions through that call; a blank one through the widget.
    let dating_field = dating-field("Ind_" + str(i) + "_Date", card.at("$path") + "date")
    // The card's `$path` prefix composes its canonical schema addresses
    // (`$cards.indorsement.<n>.…`, per-kind ordinal): the absolute loop
    // index `i` is NOT that ordinal once kinds interleave, so it stays a
    // widget-name suffix only. The card body's region rides its own glyph
    // spans through the package rebuild, per-card because each card's body
    // has its own backend-generated eval site.
    indorsement(
      from: card.from,
      // `for` is a Typst keyword, so this one declared field cannot be reached
      // as `card.for`; the `.at` is forced by syntax, not by any doubt that the
      // field is there.
      to: card.at("for"),
      signature_block: card.signature_block,
      signing_field: signature-field(
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
