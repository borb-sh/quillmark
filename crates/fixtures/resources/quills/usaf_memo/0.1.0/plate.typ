#import "@local/quillmark-helper:0.1.0": data
#import "@local/tonguetoquill-usaf-memo:1.0.0": backmatter, frontmatter, indorsement, mainmatter

// Frontmatter configuration
#show: frontmatter.with(
  // Letterhead configuration
  letterhead_title: data.main.letterhead_title,
  letterhead_caption: data.main.letterhead_caption,
  letterhead_seal: image("assets/dow_seal.jpg"),

  // Date
  date: data.main.date,

  // Receiver information
  memo_for: data.main.memo_for,

  // Sender information
  memo_from: data.main.memo_from,

  // Subject line
  subject: data.main.subject,

  // Optional references
  ..if "references" in data.main { (references: data.main.references) },

  // Optional footer tag line
  ..if "tag_line" in data.main { (footer_tag_line: data.main.tag_line) },

  // Optional classification level
  ..if "classification" in data.main { (classification_level: data.main.classification) },

  // Font size
  ..if "font_size" in data.main { (font_size: float(data.main.font_size) * 1pt) },

  // List recipients in vertical list
  memo_for_cols: 1,
)

// Mainmatter configuration
#mainmatter[
  #data.main.BODY
]

// Backmatter
#backmatter(
  // Signature block
  signature_block: data.main.signature_block,

  // Optional cc
  ..if "cc" in data.main { (cc: data.main.cc) },

  // Optional distribution
  ..if "distribution" in data.main { (distribution: data.main.distribution) },

  // Optional attachments
  ..if "attachments" in data.main { (attachments: data.main.attachments) },
)

// Indorsements - iterate through cards array and filter by CARD type
#for card in data.cards {
  if card.CARD == "indorsement" {
    indorsement(
      from: card.at("from", default: ""),
      to: card.at("for", default: ""),
      signature_block: card.signature_block,
      ..if "attachments" in card { (attachments: card.attachments) },
      ..if "cc" in card { (cc: card.cc) },
      format: card.at("format", default: "standard"),
      ..if "date" in card { (date: card.date) },
    )[
      #card.BODY
    ]
  }
}
