#import "@local/quillmark-helper:0.1.0": data
#import "@local/tonguetoquill-cmu-letter:0.1.0": backmatter, frontmatter, mainmatter

#show: frontmatter.with(
  wordmark: image("assets/cmu-wordmark.svg"),
  department: data.main.department,
  address: data.main.address,
  url: data.main.url,
  date: if "date" in data.main { data.main.date } else { datetime.today() },
  recipient: data.main.recipient,
)

#show: mainmatter

#data.main.BODY

#backmatter(
  signature_block: data.main.signature_block,
)
