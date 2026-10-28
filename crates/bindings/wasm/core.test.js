/**
 * The Typst-less `@quillmark/wasm/core` bundle: a `Quill` loads, validates,
 * seeds, and exposes schema/blueprint/metadata with no engine and no render
 * surface. The render suites cover the superset.
 */
import { describe, it, expect } from 'vitest'
import * as core from '@quillmark-wasm/core'
import { Quill, Document } from '@quillmark-wasm/core'
import { initBuildSync, makeCard } from './test-helpers.js'

initBuildSync(core, 'core')

const enc = new TextEncoder()
const dec = new TextDecoder()

// Card field accessor (mirrors the payloadItems shape used across the suite).
const field = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

// A minimal quill with one schema field: no plate/font needed; core never
// renders, it only reads config.
function makeCoreQuill() {
  const yaml = `quill:
  name: core_test
  version: "1.0.0"
  backend: typst
  description: Core build smoke test
main:
  fields:
    title:
      type: text
      description: Document title
`
  return new Map([['Quill.yaml', enc.encode(yaml)]])
}

describe('@quillmark/wasm/core surface', () => {
  it('loads a quill with no engine and carries no render API', () => {
    // The engine and session live only in the render build.
    expect(core.Engine).toBeUndefined()
    expect(core.LiveSession).toBeUndefined()
    const quill = Quill.fromTree(makeCoreQuill())
    expect(quill.backendId).toBe('typst')
    expect(quill.metadata.name).toBe('core_test')
  })

  it('a quill round-trips through its .quill bundle, and other bytes are refused by code', () => {
    const quill = Quill.fromTree(makeCoreQuill())
    const bundle = quill.toBundle()
    expect(bundle).toBeInstanceOf(Uint8Array)
    expect([...Quill.fromBundle(bundle).toTree()]).toEqual([...quill.toTree()])

    let caught
    try {
      Quill.fromBundle(enc.encode('not a zip'))
    } catch (e) {
      caught = e
    }
    expect(caught?.diagnostics?.[0]?.code).toBe('quill::bundle_invalid')
  })

  it('schema, blueprint, seed, and validate work without a backend', () => {
    const quill = Quill.fromTree(makeCoreQuill())

    expect(quill.schema.main).toBeDefined()
    expect(quill.schema.main.fields.title).toBeDefined()

    expect(typeof quill.blueprint).toBe('string')
    expect(quill.blueprint.length).toBeGreaterThan(0)

    const doc = quill.seedDocument()
    expect(doc).toBeInstanceOf(Document)

    const main = quill.seedMain()
    expect(main.kind).toBe('main')
    expect(main.payloadItems).toEqual([])

    expect(quill.validate(doc)).toEqual([])
  })

  it("emptyDocument is the blank document under the quill's own reference", () => {
    const quill = Quill.fromTree(makeCoreQuill())
    const empty = quill.emptyDocument()

    expect(empty).toBeInstanceOf(Document)
    expect(empty.equals(new Document('core_test@1.0'))).toBe(true)
    expect(empty.cards.length).toBe(0)
  })

  it('exampleDocument commits the declared examples, undefined without one', () => {
    const plain = makeCoreQuill()
    plain.set('example.md', enc.encode('~~~\n$quill: core_test\ntitle: From the file\n~~~\n'))
    expect(Quill.fromTree(plain).exampleDocument()).toBeUndefined()

    const tree = makeCoreQuill()
    const yaml = dec.decode(tree.get('Quill.yaml'))
      .replace('      description: Document title\n', '      description: Document title\n      example: A made-up title\n')
      .concat('  body:\n    example: Made-up prose.\n')
    tree.set('Quill.yaml', enc.encode(yaml))
    const quill = Quill.fromTree(tree)

    const doc = quill.exampleDocument()
    expect(doc.quillRef).toBe('core_test@1.0')
    expect(field(doc.main, 'title')).toBe('A made-up title')
    expect(doc.toMarkdown()).toContain('Made-up prose.')
    expect(quill.schema.main.fields.title.example).toBe('A made-up title')
    expect(quill.schema.main.body.example).toBe('Made-up prose.')
  })

  it('seedCard commits a $seed overlay, and no field without one', () => {
    const yaml = `quill:
  name: seed_core
  version: "1.0.0"
  backend: typst
  description: Seed overlay smoke test
main:
  fields:
    title:
      type: text
card_kinds:
  note:
    fields:
      author:
        type: text
`
    const quill = Quill.fromTree(new Map([['Quill.yaml', enc.encode(yaml)]]))

    const doc = Document.fromMarkdown(
      '~~~\n$quill: seed_core@1.0.0\n$kind: main\n$seed:\n  note:\n    author: Custom Author\n~~~\n',
    )

    // The per-kind overlay is read off main.seed[kind] (undefined for unknown).
    const overlay = doc.main.seed?.note
    expect(overlay.author).toBe('Custom Author')
    expect(doc.main.seed?.missing).toBeUndefined()

    // seedCard commits the overlay; omitting it commits no field.
    expect(field(quill.seedCard('note', overlay), 'author')).toBe('Custom Author')
    expect(quill.seedCard('note').payloadItems).toEqual([])
    // Total over the kind axis: an undeclared kind is undefined, not a throw.
    expect(quill.seedCard('missing')).toBeUndefined()

    // storeSeedOverlay writes an overlay; main.seed reads it back; remove clears.
    const doc2 = Document.fromMarkdown('~~~\n$quill: seed_core@1.0.0\n$kind: main\n~~~\n')
    doc2.storeSeedOverlay('note', { author: 'Written' })
    expect(doc2.main.seed?.note.author).toBe('Written')
    doc2.removeSeedOverlay('note')
    expect(doc2.main.seed?.note).toBeUndefined()
  })
})

// The core bundle's reason to exist is the editor: the full Document mutation +
// persistence surface must work with Typst absent, not merely be present.
describe('@quillmark/wasm/core Document editing (Typst-free)', () => {
  it('keyed card reads mirror the card write verbs', () => {
    const doc = Document.fromMarkdown(`~~~
$quill: core_test
$kind: main
title: Draft
~~~

# Body`)
    doc.insertCard(makeCard('note', { author: 'Alice' }, 'A note body.'))

    // getStored: value keyed by name; undefined when the field is absent.
    expect(doc.getStored({ card: 0, field: 'author' })).toBe('Alice')
    expect(doc.getStored({ card: 0, field: 'missing' })).toBeUndefined()

    // bodyMarkdown is the card body read (card address); a field address throws.
    // A field's markdown reads through the schema-plane view,
    // quill.reader(doc).card(i).get(name).
    expect(doc.bodyMarkdown({ card: 0 })).toContain('A note body.')
    expect(() => doc.bodyMarkdown({ card: 0, field: 'author' })).toThrow(/body-only/)

    // An out-of-range index is a boundary error: it throws, the way the card
    // write verbs do, rather than reading back as undefined/"".
    expect(() => doc.getStored({ card: 1, field: 'author' })).toThrow()
    expect(() => doc.bodyMarkdown({ card: 1 })).toThrow()
  })

  it('single-card and seed-overlay reads', () => {
    const doc = Document.fromMarkdown(`~~~
$quill: core_test
$kind: main
title: Draft
~~~

# Body`)
    doc.insertCard({ kind: 'note', body: 'A' })
    doc.insertCard({ kind: 'note', body: 'B' })
    expect(() => doc.insertCard({ kind: 'note', stray: 'x', body: 'C' })).toThrow(/stray/)

    // card(i) reads one whole card without materializing the cards array.
    expect(doc.card(1).kind).toBe('note')
    expect(doc.card(1).body.text).toBe('B')
    expect(() => doc.card(2)).toThrow() // out of range is a boundary error

    // seedOverlay reads one $seed[kind] entry off the main card cheaply, the
    // overlay you feed straight into quill.seedCard(kind, overlay).
    doc.storeSeedOverlay('note', { author: 'Seeded' })
    expect(doc.seedOverlay('note')).toEqual({ author: 'Seeded' })
    expect(doc.seedOverlay('absent')).toBeUndefined()
  })
})
