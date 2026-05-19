#import "@local/quillmark-helper:0.1.0": data

#set text(font: "Figtree")

// Advanced: Use show filter to color text
#show regex("(?i)taro"): it => text(fill: purple)[#it]

// Filters like `String` render to code mode automatically,
#underline(data.main.title)

// When using filters in markup mode,
// add `#` before the template expression to enter code mode.
*Author: #data.main.author*

*Favorite Ice Cream: #data.main.ice_cream*__


#data.main.BODY

// Present each card programatically
#for card in data.cards {
  if card.CARD == "quotes" [
    *#card.author*: _#card.BODY _
  ]
}


// Include an image with a dynamic asset
#if "picture" in data.main {
  image(data.main.picture)
}
