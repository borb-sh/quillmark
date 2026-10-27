#import "@local/quillmark-helper:0.2.0" as qm

// The whole point of this fixture: `$body` carries a GFM table, which the
// markdown -> Content -> Typst path lowers to `#table(...)`. Rendering the
// body end-to-end is the coverage: alignment, formatted cells, and ragged
// rows all reach the emitter here, not just the inline codec's unit tests.
#underline(qm.ink(qm.data, "title"))

#qm.data.at("$body")
