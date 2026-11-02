/**
 * The Document API over the canonical flow:
 * `Quill.fromTree(tree)` → `Document.parse(markdown)` →
 * `engine.render(quill, doc, opts)`, against the Typst backend build.
 */

import { describe, it, expect } from 'vitest'
import {
  Engine,
  Quill,
  Document,
  importMarkdown,
  exportMarkdown,
  normalize,
  invert,
  diff,
  diffRange,
  inheritedMarks,
  mapPos,
  repair,
  isInline,
  isPlain,
  parseDocPath,
  formatDocPath,
  formatDiagnostic,
} from '@quillmark-wasm'
import * as renderBuild from '@quillmark-wasm'
import {
  makeQuill,
  makeCard,
  expectEditCode,
  initBuildSync,
  TEST_PLATE,
  TEST_MARKDOWN,
  MD_WITH_CARD,
} from './test-helpers.js'

initBuildSync(renderBuild, 'render')

/** Read a field value from a card's payloadItems list by key. */
const field = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

/** True when a field key is absent from a card's payloadItems. */
const hasField = (card, key) =>
  card.payloadItems.some((i) => i.type === 'field' && i.key === key)

describe('Document.parse', () => {
  it('reads quillRef, payload fields, body content, cards and warnings', () => {
    const doc = Document.parse(TEST_MARKDOWN)

    expect(doc.quillRef).toBe('test_quill')
    expect(field(doc.main, 'title')).toBe('Test Document')
    expect(field(doc.main, 'author')).toBe('Test Author')
    for (const key of ['quill', '$quill', '$body', '$cards']) {
      expect(hasField(doc.main, key)).toBe(false)
    }
    expect(doc.main.body.text).toContain('Hello World')
    expect(exportMarkdown(doc.main.body)).toContain('Hello World')
    expect(doc.cards).toEqual([])
    expect(doc.warnings).toEqual([])
  })

  it('should expose card fields and body', () => {
    const doc = Document.parse(MD_WITH_CARD)

    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(field(doc.cards[0], 'foo')).toBe('bar')
    expect(exportMarkdown(doc.cards[0].body)).toContain('Card body.')
  })
})

describe('Document.toMarkdown', () => {
  it('a mutated document survives emit → re-parse', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc._uncheckedStoreField('title', 'New Title')
    doc._uncheckedInsertCard(makeCard('note', { author: 'Alice' }, 'Hello'))
    doc._revise({}, 'Updated body')

    const doc2 = Document.parse(doc.toMarkdown())
    expect(field(doc2.main, 'title')).toBe('New Title')
    expect(exportMarkdown(doc2.main.body)).toBe('Updated body')
    expect(doc2.cards.length).toBe(1)
    expect(doc2.cards[0].kind).toBe('note')
    expect(field(doc2.cards[0], 'author')).toBe('Alice')
    expect(exportMarkdown(doc2.cards[0].body)).toBe('Hello')
  })
})

// The DTO's content rules are core's (`core/src/document/dto.rs`).
describe('Document JSON DTO: toStored / fromStored', () => {
  it('toStored emits a plain JSON string under the newest schema version', () => {
    const dto = Document.parse(TEST_MARKDOWN).toStored()
    expect(typeof dto).toBe('string')
    expect(JSON.parse(dto).schema).toBe(Document.currentStorageVersion())
    const carded = Document.parse(`${TEST_MARKDOWN}\n\n~~~\n$kind: note\n~~~\n`).toStored()
    expect(JSON.parse(carded).schema).toBe(Document.currentStorageVersion())
  })

  it('round-trips a mutated document with cards back to an equal handle', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc._uncheckedStoreField('title', 'New Title')
    doc._uncheckedInsertCard(makeCard('note', { author: 'Alice' }, 'Hello'))

    const restored = Document.fromStored(doc.toStored())

    expect(restored.equals(doc)).toBe(true)
    expect(field(restored.main, 'title')).toBe('New Title')
    expect(restored.cards[0].kind).toBe('note')
    expect(exportMarkdown(restored.cards[0].body)).toBe('Hello')
  })

  it('a row naming a construct outside the vocabulary does not open, but its tag still reads', () => {
    const dto = Document.parse(TEST_MARKDOWN).toStored()
    const outside = dto.replace('"kind":"para"', '"kind":"callout"')
    expect(outside).not.toBe(dto)
    expect(() => Document.fromStored(outside)).toThrow()
    expect(Document.storageVersionOf(outside)).toBe(JSON.parse(dto).schema)
  })

  it('storageVersionOf reads the schema tag off any payload, or undefined', () => {
    // A future version reads back as-is, though fromStored rejects it.
    const future = '{"schema":"quillmark/document@9.0.0","main":{}}'
    expect(() => Document.fromStored(future)).toThrow()
    expect(Document.storageVersionOf(future)).toBe('quillmark/document@9.0.0')

    expect(Document.storageVersionOf('{"foo":"bar"}')).toBeUndefined()
    expect(Document.storageVersionOf(TEST_MARKDOWN)).toBeUndefined()
  })
})

describe('Quill.fromTree + engine.render', () => {
  it('should accept a plain object tree (Record<string, Uint8Array>)', () => {
    const fromObject = Quill.fromTree(
      Object.fromEntries(makeQuill({ name: 'test_quill', plate: TEST_PLATE })),
    )
    expect(fromObject.backendId).toBe('typst')
    expect(fromObject.metadata.name).toBe('test_quill')
  })

  it('should reject non-object trees with a clear error', () => {
    expect(() => Quill.fromTree(42)).toThrow()
    expect(() => Quill.fromTree('string')).toThrow()
    expect(() => Quill.fromTree(null)).toThrow()
  })

  // `opts: undefined` is the two-argument call form, whose default is pdf.
  const RENDER_FORMAT_CASES = [
    { opts: undefined, mimeType: 'application/pdf' },
    { opts: { format: 'pdf' }, mimeType: 'application/pdf' },
    { opts: { format: 'svg' }, mimeType: 'image/svg+xml' },
  ]

  it('renders one Document repeatedly, once per format', () => {
    const engine = new Engine()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.parse(TEST_MARKDOWN)
    for (const { opts, mimeType } of RENDER_FORMAT_CASES) {
      const result = opts === undefined ? engine.render(quill, doc) : engine.render(quill, doc, opts)
      expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
      expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
      expect(result.artifacts[0].mimeType).toBe(mimeType)
    }
  })

  // A value conform cannot rest warns on `doc.warnings` and renders, and a
  // render's warnings are its own: an image under Typst declines there. Each
  // warning's `args` is declared `Record<string, unknown>`, so it must read as
  // one on the far side.
  it('a load warning and a render warning carry their args as plain objects', () => {
    const NOTES_QUILL_YAML = `quill:
  name: notes
  version: "1.0.0"
  backend: typst
  description: A quill holding a list of richtext notes

main:
  fields:
    notes:
      type: array
      items:
        type: richtext
        multiline: true
`
    const NOTES_PLATE = `#import "@local/quillmark-helper:0.2.0" as qm

#qm.data.at("$body")`

    const engine = new Engine()
    const quill = Quill.fromTree(
      makeQuill({ name: 'notes', plate: NOTES_PLATE, quillYaml: NOTES_QUILL_YAML }),
    )
    const doc = quill.parse(
      '~~~card-yaml\n$quill: notes\nnotes: [42]\n~~~\n\nAlpha ![a](https://example.com/a.png)\n',
    )

    const loaded = doc.warnings.find((d) => d.code === 'conform::field_decode')
    expect(loaded).toBeDefined()
    expect(loaded.args).not.toBeInstanceOf(Map)
    expect(loaded.args.field).toBe('notes')

    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.warnings.map((d) => d.code)).not.toContain('conform::field_decode')
    const declined = result.warnings.find((d) => d.code === 'backend::declined_construct')
    expect(declined).toBeDefined()
    expect(declined.args).not.toBeInstanceOf(Map)
    expect(declined.args.construct).toBe('image')
  })

  it('should throw a quill::name_mismatch error when the document quill ref differs from the quill name', () => {
    const engine = new Engine()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.parse(TEST_MARKDOWN.replace('test_quill', 'other_quill'))
    expectEditCode(() => engine.render(quill, doc, { format: 'pdf' }), 'quill::name_mismatch')
  })
})

describe('Document editor surface: storeField / removeField', () => {
  it('storeField writes a field and removeField returns it, undefined when absent', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc._uncheckedStoreField('title', 'Updated')
    expect(field(doc.main, 'title')).toBe('Updated')
    expect(doc.removeField('title')).toBe('Updated')
    expect(hasField(doc.main, 'title')).toBe(false)
    expect(doc.removeField('title')).toBeUndefined()
  })

  it('both throw edit::invalid_field_name on an invalid name', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    expectEditCode(() => doc._uncheckedStoreField('$body', 'x'), 'edit::invalid_field_name')
    expectEditCode(() => doc._uncheckedStoreField('bad-name', 'x'), 'edit::invalid_field_name')
    expectEditCode(() => doc.removeField('$quill'), 'edit::invalid_field_name')
  })

  it('a field holds its content as a body does: canonical on store, refused when invalid', () => {
    const { content } = importMarkdown('Hello *world*')
    const anchored = { ...content, anchors: [{ id: 'a1', start: 0, end: 5 }] }
    const a = Document.parse(TEST_MARKDOWN)
    const b = a.clone()
    a._uncheckedStoreField('blurb', Object.fromEntries(Object.entries(anchored).reverse()))
    b._uncheckedStoreField('blurb', anchored)
    expect(a.equals(b)).toBe(true)
    expect(a.toStored()).toBe(b.toStored())
    expect(a.toMarkdown()).toContain('blurb: Hello *world*')
    expectEditCode(() => a._uncheckedStoreField('blurb', { ...content, marks: [{ type: 'bold', start: 4, end: 2 }] }), 'edit::field_decode')
  })
})

describe('Document blank-canvas constructor', () => {
  it('new Document(quillRef) starts blank and builds up', () => {
    const doc = new Document('test_quill')
    expect(doc.quillRef).toBe('test_quill')
    expect(doc.cards.length).toBe(0)
    expect(exportMarkdown(doc.main.body)).toBe('')
    doc._uncheckedStoreField('title', 'Hello')
    expect(field(doc.main, 'title')).toBe('Hello')
  })

  it('throws on an invalid quill reference, coded as setQuillRef codes it', () => {
    expectEditCode(
      () => new Document('not a valid ref!!'),
      'parse::invalid_quill_reference',
    )
  })
})

describe('Document editor surface: addresses', () => {
  it('an address with an unknown key throws instead of parsing as {}', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    // `Addr::from_js` rejects a stray key: a typo, or the fields object misread
    // as an address, is caught loud rather than silently parsed as the empty
    // main-card address.
    expect(() => doc._uncheckedStoreField({ crad: 0, field: 'title' }, 'x')).toThrow(/unknown key/)
    expect(field(doc.main, 'title')).not.toBe('x')
  })

  it('an address is a plain object, and every own key of it counts', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    const write = (addr) => () => doc._uncheckedStoreField(addr, 'x')
    expect(write(new Map([['card', 0]]))).toThrow('addr must be a plain object, not a `Map`')
    expect(write(Object.create({ card: 0 }))).toThrow('addr must be a plain object')
    expect(write(Object.defineProperty({}, 'crad', { value: 0 }))).toThrow('unknown key `crad`')
    expect(write(Object.create(Object.assign(Object.create(null), { crad: 0 })))).toThrow('unknown key `crad`')
    expect(field(doc.main, 'title')).not.toBe('x')
  })

  it('an address whose read throws is refused, and the document stays writable', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    const trap = new Proxy({}, { ownKeys: () => { throw new Error('ownKeys trap') } })
    expect(() => doc._uncheckedStoreField(trap, 'x')).toThrow(
      'addr must be a plain object, not one whose read throws: ownKeys trap'
    )
    doc._uncheckedStoreField('title', 'y')
    expect(field(doc.main, 'title')).toBe('y')
  })
})

describe('Document editor surface: setQuillRef / replace / revise', () => {
  it('setQuillRef changes the quillRef and refuses an invalid one', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc.setQuillRef('new_quill')
    expect(doc.quillRef).toBe('new_quill')
    expectEditCode(
      () => doc.setQuillRef('INVALID QUILL REF WITH SPACES'),
      'parse::invalid_quill_reference',
    )
  })

  it('revise({}, md) revises the main body and returns the content receipt', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    const { applied, hash, warnings } = doc._revise({}, 'Body from **markdown**.')
    expect(exportMarkdown(doc.main.body)).toBe('Body from **markdown**.')
    expect(Array.isArray(applied.steps)).toBe(true)
    expect(hash).toBe(doc.hashAt({}))
    expect(warnings).toEqual([])
  })

  it('replace({}, rt) writes a content object', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc.replace({}, importMarkdown('Content **body** here.').content)
    expect(doc.main.body.text).toBe('Content body here.')
    expect(exportMarkdown(doc.main.body)).toBe('Content **body** here.')
  })

  it('replace({}, content) lands a content and carries the anchors it omits', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc.replace({}, importMarkdown('hello world').content)
    doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 6, end: 11 } }] })
    const { warnings } = doc.replace({}, importMarkdown('hello brave **world**').content)
    expect(exportMarkdown(doc.main.body)).toBe('hello brave **world**')
    expect(doc.main.body.anchors).toEqual([{ id: 'c1', start: 12, end: 17 }])
    expect(warnings).toEqual([])
  })

  it('replace({field}, content) carries a field anchor and refuses a non-content value', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc.revise({ field: 'note' }, 'hello world')
    doc.apply({ field: 'note' }, { steps: [{ anchor: { id: 'c1', start: 0, end: 5 } }] })
    doc.replace({ field: 'note' }, importMarkdown('oh hello world').content)
    expect(doc.getStored({ field: 'note' }).anchors).toEqual([{ id: 'c1', start: 3, end: 8 }])
    doc._uncheckedStoreField('count', 3)
    expectEditCode(() => doc.replace({ field: 'count' }, importMarkdown('x').content), 'edit::field_decode')
  })

  it('replace rejects a non-content value (markdown must go through importMarkdown)', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    expect(() => doc.replace({}, 'plain markdown')).toThrow()
    expect(() => doc.replace({}, { not: 'a content' })).toThrow()
  })

  // Island `props` and unknown `attrs` are opaque host payload, and
  // every consumer of them (key canonicalization, the hash key, the JS→JSON
  // conversion, the tree's own drop) recurses one frame per level. On wasm32 the
  // stack is 1 MB and an overflow is a trap that takes the module down rather than
  // an error the host can catch, so an over-deep value must throw and leave the
  // module serving. `replace` is the reachable door: the value arrives from JS and
  // one loop builds it.
  it('replace rejects a deeply nested props instead of trapping the module', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    const rt = importMarkdown('body').content
    rt.islands = [{ id: 'i1', type: 'image', props: deep }]
    // Matched on the message: a slot/shape complaint would pass a bare toThrow
    // while the depth door stayed open.
    expect(() => doc.replace({}, rt)).toThrow(/nests deeper/)
    doc.replace({}, importMarkdown('after').content)
    expect(doc.main.body.text).toBe('after')
  })

  // Every door taking opaque host JSON carries the guard, not just `replace`:
  // a payload item's `value` crosses on the same `serde_wasm_bindgen` recursion.
  it('insertCard rejects a deeply nested payload item value instead of trapping the module', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    const card = makeCard('note', { ok: 1 }, 'Hello')
    card.payloadItems[0].value = deep
    expect(() => doc._uncheckedInsertCard(card)).toThrow(/nests deeper/)
    doc._uncheckedInsertCard(makeCard('note', { ok: 2 }, 'Hello'))
    expect(doc.cardCount).toBe(1)
  })
})

describe('Document comment anchors: anchorContexts / reattach / droppedAnchors', () => {
  const MD = '~~~\n$quill: q\n~~~\n\nThe committee met on Monday to plan.\n\n~~~\n$kind: note\n~~~\n\nA note.\n'
  const anchoredDoc = () => {
    const doc = Document.parse(MD)
    doc.apply({}, { steps: [{ anchor: { id: 'm1', start: 14, end: 27 } }] })
    return doc
  }

  it('anchorContexts reads each anchor as a selector at its path', () => {
    const doc = anchoredDoc()
    const context = { id: 'm1', path: 'main.body', quote: 'met on Monday', prefix: 'The committee ', suffix: ' to plan.' }
    expect(doc.anchorContexts()).toEqual([context])
    expect(doc.anchorContext('m1')).toEqual(context)
    expect(doc.anchorContext('nope')).toBeUndefined()
  })

  it('revise and replace name the anchors they drop', () => {
    const doc = anchoredDoc()
    expect(doc.revise({}, 'The committee met on Monday to plan, again.').droppedAnchors).toEqual([])
    const context = { id: 'm1', path: 'main.body', quote: 'met on Monday', prefix: 'The committee ', suffix: ' to plan, again.' }
    expect(doc.revise({}, 'Nothing like it.\n\nAt all.').droppedAnchors).toEqual([context])
    const other = anchoredDoc()
    expect(other.replace({}, importMarkdown('Nothing like it.\n\nAt all.').content).droppedAnchors).toEqual([
      { ...context, suffix: ' to plan.' },
    ])
    expect(anchoredDoc()._revise({}, 'Nothing like it.\n\nAt all.').droppedAnchors).toEqual([{ ...context, suffix: ' to plan.' }])
  })

  it('reattach places a dropped anchor where its quote and context are found once', () => {
    const doc = anchoredDoc()
    const before = doc.anchorContexts()
    const { applied } = doc._revise({}, 'Minutes.\n\nThe committee met on Monday to plan.')
    expect(doc.anchorContexts().map((c) => c.id)).toEqual(['m1'])

    const reopened = Document.parse(doc.toMarkdown())
    const { reattached, orphaned } = reopened.reattach(before)
    expect(orphaned).toEqual([])
    expect(reattached).toEqual([{ ...before[0], prefix: 'Minutes.\nThe committee ' }])
    expect(reopened.anchorContexts()).toEqual(doc.anchorContexts())

    const strict = Document.parse(doc.toMarkdown())
    expect(strict.reattach(before, { 'main.body': applied }).orphaned).toEqual(before)
    expect(() => strict.reattach(before, { 'main.body': { ops: [] } })).toThrow(/reattach/)
  })

  it('a typed revise answers its change in UTF-16, over the value it revised', () => {
    const doc = anchoredDoc()
    doc._revise({}, '😀😀 alpha gamma')
    const { applied } = doc._revise({}, '😀😀 alpha 😀 beta gamma')
    expect(mapPos(applied, 10, 'before').pos).toBe(10)
    expect(mapPos(applied, 11, 'after').pos).toBe(19)
  })

  it('reattach answers a live id without writing and refuses a malformed selector', () => {
    const doc = anchoredDoc()
    const held = doc.clone()
    const elsewhere = { id: 'm1', path: 'cards.note[0].body', quote: 'note', prefix: 'A ', suffix: '.' }
    expect(doc.reattach([elsewhere])).toEqual({ reattached: doc.anchorContexts(), orphaned: [] })
    expect(doc.equals(held)).toBe(true)
    expect(() => doc.reattach([{ ...elsewhere, path: 'main.[' }])).toThrow(/reattach/)
    expect(() => doc.reattach([{ id: 'x' }])).toThrow(/reattach/)
  })
})

describe('Card ids', () => {
  const MD = '~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nA note about the orchard rows.\n'

  it('a composable card carries its $id through markdown and placement', () => {
    const doc = Document.parse(MD)
    const id = doc.cards[0].id
    expect(id).toMatch(/^c[a-z2-9]{5}$/)
    expect(doc.main.id).toBeUndefined()
    expect(doc.toMarkdown()).toContain(`$kind: note\n$id: ${id}\n`)
    expect(Document.parse(doc.toMarkdown()).cards[0].id).toBe(id)

    doc._uncheckedInsertCard(makeCard('note', {}, 'Placed.'))
    expect(doc.cards[1].id).toMatch(/^c[a-z2-9]{5}$/)
    expect(doc.cards[1].id).not.toBe(id)
    doc._uncheckedInsertCard({ ...makeCard('note'), id: 'chosen' })
    expect(doc.cards[2].id).toBe('chosen')
    expectEditCode(() => doc._uncheckedInsertCard({ ...makeCard('note'), id }), 'edit::duplicate_card_id')
    expectEditCode(() => doc._uncheckedInsertCard({ ...makeCard('note'), id: '-x' }), 'edit::invalid_card_id')
  })

  it('a live client places a card under its actor and carries the id it answers', () => {
    const a = Document.parse(MD)
    const b = a.clone()
    const minted = a._uncheckedInsertCard(makeCard('note', {}, 'From a.'), 1, { actor: '7' })
    expect(minted).toBe(a.cards[1].id)
    expect(minted).toMatch(/^7-c[a-z2-9]{5}$/)
    expect(b._uncheckedInsertCard(makeCard('note', {}, 'From b.'), 1, { actor: '8' })).not.toBe(minted)
    expect(b._uncheckedInsertCard({ ...makeCard('note', {}, 'From a.'), id: minted }, 1)).toBe(minted)
    expectEditCode(() => a._uncheckedInsertCard(makeCard('note'), 0, { actor: 'a:b' }), 'edit::invalid_actor')
  })

  it('a selector names its card, and reattach follows the card wherever it moved', () => {
    const doc = Document.parse(MD)
    doc.apply({ card: doc.cards[0].id }, { steps: [{ anchor: { id: 'n1', start: 17, end: 29 } }] })
    const [selector] = doc.anchorContexts()
    expect(selector).toMatchObject({ path: 'cards.note[0].body', cardId: doc.cards[0].id, quote: 'orchard rows' })
    expect(doc.toMarkdown(undefined, { annotated: true }).anchors[0].cardId).toBe(doc.cards[0].id)

    const reopened = Document.parse(doc.toMarkdown())
    reopened._uncheckedInsertCard(makeCard('note', {}, 'An earlier note.'), 0)
    const { reattached, orphaned } = reopened.reattach([selector])
    expect(orphaned).toEqual([])
    expect(reattached[0].path).toBe('cards.note[1].body')
    expect(reopened.reattach([{ ...selector, id: 'n2', cardId: 'nobody' }]).orphaned.length).toBe(1)
  })
})

describe('Content model: importMarkdown / exportMarkdown', () => {
  it('importMarkdown ∘ exportMarkdown round-trips a body', () => {
    const { content: rt, warnings } = importMarkdown('A **bold** line.')
    expect(typeof rt).toBe('object')
    expect(rt.text).toBe('A bold line.')
    expect(exportMarkdown(rt)).toBe('A **bold** line.')
    expect(warnings).toEqual([])
  })

  it('importMarkdown imports the table a blank line sets apart from a centering div and warns of the div', () => {
    const { content, warnings } = importMarkdown('<div align="center">\n\n| a | b |\n|---|---|\n\n</div>')
    expect(content.islands.map((i) => i.type)).toEqual(['table'])
    expect(warnings).toEqual([
      {
        severity: 'warning',
        code: 'parse::dropped_construct',
        message: expect.any(String),
        hint: expect.any(String),
        args: { construct: 'div', count: 1 },
      },
    ])
  })

  it('answers the canonical form on every Content lane, adjacent runs apart by container id', () => {
    // A read is a write input, and the one form is what a host writes straight
    // back. Two adjacent lists are two containers, each its own id.
    const lists = (c) => c.blocks.map((b) => b.path[0])
    expect(lists(importMarkdown('- a\n\n* b').content)).toEqual(['l0', 'l1'])

    const doc = Document.parse('~~~card-yaml\n$quill: commit_test\n~~~\n\n> a\n\n- b')
    expect(doc.getStored({})).toEqual(doc.main.body)
    expect(doc.read({})).toEqual(doc.main.body)

    const held = doc.read({})
    expect(doc.replace({}, held).applied).toEqual({ steps: [] })
    expect(doc.read({})).toEqual(held)
    expect(doc.toMarkdown({})).toBe('> a\n\n- b')
  })

  it('offsets cross in UTF-16 code units', () => {
    const doc = Document.parse('~~~card-yaml\n$quill: test_quill\n~~~\n\n😀 **bold**\n')
    const { text, marks } = doc.read({})
    expect(marks).toEqual([{ start: 3, end: 7, type: 'strong' }])
    expect(text.slice(3, 7)).toBe('bold')

    const { applied } = doc.apply({}, { steps: [{ splice: { at: 2, delete: '', insert: '!' } }] })
    expect(doc.read({}).text).toBe('😀! bold')
    expect(applied.steps[0].splice.at).toBe(2)
    expectEditCode(() => doc.apply({}, { steps: [{ splice: { at: 1, delete: '', insert: 'x' } }] }), 'edit::content_apply')
    expect(() => doc.replace({}, { text: '😀', blocks: [{ id: 'b0', kind: 'para' }], marks: [{ start: 1, end: 2, type: 'strong' }] })).toThrow(/inside a character/)
  })

  it('a step holding a key it does not define is refused and writes nothing', () => {
    const doc = Document.parse('~~~card-yaml\n$quill: test_quill\n~~~\n\nhello world\n')
    const held = doc.read({})
    expect(() => doc.apply({}, { steps: [{ splice: { at: 0, delet: 'hello', insert: 'hi' } }] })).toThrow(/delet/)
    expect(() => doc.apply({}, { steps: [{ mark: { start: 0, end: 5, type: 'strong', add: true, qoute: 'hello' } }] })).toThrow(/qoute/)
    expect(doc.read({})).toEqual(held)
  })

  it('an auto-fit column is absent from widths, and a stored null crosses as null', () => {
    const md = '<table widths="1 2 auto">\n<tr><th>a</th><th>b</th><th>c</th></tr>\n<tr><td>1</td><td>2</td><td>3</td></tr>\n</table>'
    const { content } = importMarkdown(md)
    expect(content.islands[0].props.widths).toEqual({ k0: 1, k1: 2 })
    expect(importMarkdown(exportMarkdown(content)).content).toEqual(content)

    const doc = Document.parse('~~~card-yaml\n$quill: test_quill\nnote: null\n~~~\n')
    expect(doc.getStored('note')).toBeNull()
  })

  // A weight is a JS number, exact up to `Number.MAX_SAFE_INTEGER`; one past it
  // drops at the import rather than reaching a read that cannot carry it.
  it('a column weight crosses exactly up to the largest safe integer, and drops past it', () => {
    const table = (widths) =>
      `<table widths="${widths}">\n<tr><th>a</th><th>b</th></tr>\n<tr><td>1</td><td>2</td></tr>\n</table>`
    const { content, warnings } = importMarkdown(table(`${Number.MAX_SAFE_INTEGER} 1`))
    expect(warnings).toEqual([])
    expect(content.islands[0].props.widths).toEqual({ k0: Number.MAX_SAFE_INTEGER, k1: 1 })
    expect(exportMarkdown(content)).toContain(`widths="${Number.MAX_SAFE_INTEGER} 1"`)

    const doc = Document.parse('~~~card-yaml\n$quill: test_quill\n~~~\n')
    doc.replace({}, content)
    expect(Document.fromStored(doc.toStored()).main.body).toEqual(doc.main.body)
    expect(doc.main.body.islands).toEqual(content.islands)

    const past = importMarkdown(table('9007199254740992 1'))
    expect(past.content.islands[0].props.widths).toBeUndefined()
    expect(past.warnings.map((w) => w.args.construct)).toEqual(['table[widths]'])
  })
})

describe('Content predicates: isInline / isPlain', () => {
  it('judge the single-line and plain constraints on a Content, throwing on a non-content', () => {
    expect(isInline(importMarkdown('One **bold** line.').content)).toBe(true)
    expect(isInline(importMarkdown('One.\n\nTwo.').content)).toBe(false)
    expect(isInline(importMarkdown('- item').content)).toBe(false)
    expect(isPlain(importMarkdown('One.\n\nTwo.').content)).toBe(true)
    expect(isPlain(importMarkdown('One **bold** line.').content)).toBe(false)
    expect(() => isInline('One.')).toThrow()
    expect(() => isPlain({ not: 'a content' })).toThrow()
  })
})

describe('Document-model path: parseDocPath / formatDocPath', () => {
  // Every emitted shape routes on tagged segments, not on a regex.
  const cases = [
    ['main.title', [{ seg: 'main' }, { seg: 'field', name: 'title' }]],
    [
      'main.recipients[0].name',
      [
        { seg: 'main' },
        { seg: 'field', name: 'recipients' },
        { seg: 'index', index: 0 },
        { seg: 'field', name: 'name' },
      ],
    ],
    ['main.body', [{ seg: 'main' }, { seg: 'body' }]],
    ['cards[3]', [{ seg: 'card', kind: null, index: 3 }]],
    [
      'cards.indorsement[0].signature_block',
      [
        { seg: 'card', kind: 'indorsement', index: 0 },
        { seg: 'field', name: 'signature_block' },
      ],
    ],
    [
      'cards.skills[2].body',
      [{ seg: 'card', kind: 'skills', index: 2 }, { seg: 'body' }],
    ],
  ]

  it('parseDocPath and formatDocPath round-trip every emitted shape', () => {
    for (const [rendered, segs] of cases) {
      expect(parseDocPath(rendered)).toEqual(segs)
      expect(formatDocPath(segs)).toBe(rendered)
    }
  })

  it('both throw on a malformed or empty path', () => {
    expect(() => parseDocPath('cards[')).toThrow()
    expect(() => parseDocPath('')).toThrow()
    expect(() => formatDocPath([])).toThrow()
  })
})

describe('formatDiagnostic', () => {
  it('formats a plain diagnostic object and refuses an unknown severity', () => {
    const text = formatDiagnostic({
      severity: 'error',
      code: 'validation::coercion_failed',
      message: 'bad date',
    })
    expect(text).toContain('bad date')
    expect(text).toContain('validation::coercion_failed')
    expect(() => formatDiagnostic({ severity: 'fatal', message: 'bad ladder' })).toThrow()
  })
})

describe('Document-model path: pathFor / cardPath', () => {
  const MD = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Main body.

~~~card-yaml
$kind: note
from: x
~~~

Kinded card.
`

  it('mints every address the Addr surface can name', () => {
    const doc = Document.parse(MD)
    const rows = [
      // An absent field is the body, an absent card the main card; a bare
      // string is the `{ field }` shorthand the Addr verbs take.
      [doc.pathFor(), 'main.body'],
      [doc.pathFor({}), 'main.body'],
      [doc.pathFor('intro'), 'main.intro'],
      [doc.pathFor({ field: 'intro' }), 'main.intro'],
      // A card root is kind-qualified off the live card's stored `$kind`.
      [doc.pathFor({ card: 0 }), 'cards.note[0].body'],
      [doc.pathFor({ card: 0, field: 'from' }), 'cards.note[0].from'],
      [doc.cardPath(0), 'cards.note[0]'],
    ]
    for (const [minted, expected] of rows) {
      expect(minted).toBe(expected)
      // A minted path parses back.
      expect(() => parseDocPath(minted)).not.toThrow()
    }
  })

  it('is total on the index axis, unlike the Addr reads', () => {
    const doc = Document.parse(MD)
    // An out-of-range card mints the unknown-kind root
    // `edit::index_out_of_range` anchors at, so a per-keystroke call needs no
    // `try`.
    expect(doc.pathFor({ card: 7, field: 'from' })).toBe('cards[7].from')
    expect(doc.cardPath(7)).toBe('cards[7]')
    expect(() => parseDocPath(doc.pathFor({ card: 7, field: 'from' }))).not.toThrow()
    // The reads at that same address throw.
    expectEditCode(() => doc.getStored({ card: 7, field: 'from' }), 'edit::index_out_of_range')
  })
})

// The write primitive and its pure helpers: `apply` runs a change all or
// none, its receipt's `applied` inverts, and `diff` answers without a
// document.
describe('Document apply: the write primitive', () => {
  const blankDoc = () => Document.parse('~~~card-yaml\n$quill: commit_test\n~~~\n\nBody.')

  it('revise({field}) carries a richtext field anchor and apply formats it', () => {
    const doc = blankDoc()
    doc.revise({ field: 'intro' }, 'make it bold here')
    doc.apply({ field: 'intro' }, { steps: [{ anchor: { id: 'a1', start: 8, end: 12 } }] })
    expect(doc.revise({ field: 'intro' }, 'now make it bold here').warnings).toEqual([])
    expect(doc.read({ field: 'intro' }).anchors).toEqual([{ id: 'a1', start: 12, end: 16 }])
    doc.apply({ field: 'intro' }, { steps: [{ mark: { start: 12, end: 16, type: 'strong', add: true } }] })
    expect(exportMarkdown(doc.read({ field: 'intro' }))).toBe('now make it **bold** here')

    // A refused step leaves the value as it was, the steps before it too.
    const held = doc.read({ field: 'intro' })
    const change = { steps: [{ mark: { start: 0, end: 3, type: 'emph', add: true } }, { splice: { at: 999, delete: '', insert: 'x' } }] }
    expectEditCode(() => doc.apply({ field: 'intro' }, change), 'edit::content_apply')
    expect(doc.read({ field: 'intro' })).toEqual(held)
  })

  it('a receipt carries the normalized change, which invert undoes, and the next base', () => {
    const doc = blankDoc()
    const before = doc.read({})
    const base = doc.hash({})
    const receipt = doc.apply({}, { steps: [{ splice: { at: 0, delete: 'Body', insert: 'Text' } }] }, { base })
    expect(receipt.hash).toBe(doc.hash({}))
    expect(receipt.applied.steps).toHaveLength(1)
    expect(doc.apply({}, { steps: [] }, { base }).rebased).toBe(true)
    expectEditCode(() => doc.apply({}, { steps: [] }, { base: 'never-read' }), 'edit::stale_base')
    expect(() => doc.apply({}, { steps: [] }, { baseMarkdown: 'Body.' })).toThrow(/base/)

    doc.apply({}, invert(receipt.applied, before), { base: receipt.hash })
    expect(doc.read({})).toEqual(before)
  })

  it('a stale base rebases a write over the change since, offsets in UTF-16 as the writer read them', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('😀 hello world').content)
    const base = doc.hash({})
    doc.apply({}, { steps: [{ splice: { at: 0, delete: '', insert: '🎉 ' } }] })
    const anchor = { steps: [{ anchor: { id: 'a1', start: 9, end: 14, quote: 'world' } }] }
    const receipt = doc.apply({}, anchor, { base })
    expect(receipt.rebased).toBe(true)
    expect(receipt.applied.steps[0].anchor).toMatchObject({ start: 12, end: 17 })
    expect(doc.read({}).anchors).toEqual([{ id: 'a1', start: 12, end: 17 }])

    const reworded = { steps: [{ splice: { at: 9, delete: 'world', insert: 'there' } }] }
    doc.apply({}, { steps: [{ splice: { at: 14, delete: 'r', insert: 'R' } }] })
    const err = expectEditCode(() => doc.apply({}, reworded, { base }), 'edit::rebase_conflict')
    expect(err.diagnostics[0].args).toMatchObject({ current: doc.hash({}), step: 0, reason: 'changed' })
  })

  it('a quoted step refuses text its writer did not read, offsets in UTF-16', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('😀 hello world').content)
    const anchor = { steps: [{ anchor: { id: 'a1', start: 9, end: 14, quote: 'world' } }] }
    doc.apply({}, { steps: [{ splice: { at: 0, delete: '', insert: 'XX ' } }] })
    expectEditCode(() => doc.apply({}, anchor), 'edit::content_apply')
    const receipt = doc.apply({}, { steps: [{ anchor: { id: 'a1', start: 12, end: 17, quote: 'world' } }] })
    expect(receipt.applied.steps[0].anchor.quote).toBe('world')
  })

  it('an anchor placement refuses a held id and a move moves the held one', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('hello world').content)
    doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 0, end: 5 } }] })
    expectEditCode(() => doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 6, end: 11 } }] }), 'edit::content_apply')
    expectEditCode(() => doc.apply({}, { steps: [{ anchor: { id: 'c9', start: 6, end: 11, move: true } }] }), 'edit::content_apply')
    const receipt = doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 6, end: 11, move: true } }] })
    expect(receipt.applied.steps[0].anchor).toEqual({ id: 'c1', start: 6, end: 11, quote: 'world', move: true })
    expect(doc.read({}).anchors).toEqual([{ id: 'c1', start: 6, end: 11 }])
  })

  it('a receipt names each anchor a write moved and what moved, a point included', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('😀 alpha beta gamma delta').content)
    doc.apply({}, { steps: [{ anchor: { id: 'p', start: 3, end: 3 } }, { anchor: { id: 'r', start: 9, end: 13 } }] })
    const receipt = doc.apply({}, { steps: [{ anchor: { id: 'p', start: 14, end: 14, move: true } }, { splice: { at: 13, delete: '', insert: '!' } }] })
    expect(receipt.movedAnchors).toEqual([
      { id: 'p', before: '', after: '', reason: 'position' },
      { id: 'r', before: 'beta', after: 'beta', reason: 'context' },
    ])
  })

  it('a reserved x- name rides as written, and another unknown one throws', () => {
    const doc = blankDoc()
    const value = { text: 'Note', blocks: [{ id: 'b0', kind: 'x-callout', attrs: { tone: 'warm' } }], marks: [{ start: 0, end: 4, type: 'x-glow' }] }
    doc.replace({}, value)
    expect(doc.read({})).toEqual(value)
    expect(exportMarkdown(value)).toBe('Note')
    expect(() => doc.replace({}, { ...value, blocks: [{ id: 'b0', kind: 'callout' }] })).toThrow(/callout/)
  })

  it('diff answers without a document', () => {
    const before = importMarkdown('hello world').content
    const change = diff(before, importMarkdown('hello brave world').content)
    const doc = blankDoc()
    doc.replace({}, before)
    doc.apply({}, change)
    expect(doc.read({}).text).toBe('hello brave world')
  })

  it('the structural edits write on the document in UTF-16 offsets and answer receipts', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('😀 ab\n\n- one\n- two').content)
    const base = doc.hash({})
    const split = doc.splitBlock({}, 4, { base, actor: 'ann' })
    expect(split.applied.steps[0].splice.at).toBe(4)
    expect(split.minted).toEqual({ '?0': 'ann:b0' })
    expect(doc.read({}).text).toBe('😀 a\nb\none\ntwo')
    const stale = doc.splitBlock({}, 1 + 2, { base, actor: 'bo', id: 'mine' })
    expect(stale.rebased).toBe(true)
    expect(doc.read({}).text).toBe('😀 \na\nb\none\ntwo')
    doc.joinBlocks({}, 'mine')
    doc.joinBlocks({}, 'ann:b0')
    let c = doc.read({})
    expect(c.text).toBe('😀 ab\none\ntwo')

    const [, first, second] = c.blocks.map((b) => b.id)
    doc.indent({}, second)
    expect(doc.read({}).blocks[2].path).toHaveLength(4)
    doc.outdent({}, second, undefined, { actor: 'ann' })
    expect(doc.read({}).blocks[2].path).toHaveLength(2)
    const wrapped = doc.wrap({}, c.blocks[0].id, c.blocks[0].id, { kind: 'quote' }, { actor: 'ann' })
    c = doc.read({})
    expect(c.blocks[0].path).toEqual([wrapped.minted['?0']])
    expect(c.containers.find((k) => k.id === 'ann:q0').kind).toBe('quote')
    expectEditCode(() => doc.indent({}, first), 'edit::content_apply')
    expectEditCode(() => doc.wrap({}, first, first, { kind: 'quote' }, { actor: 'a:b' }), 'edit::invalid_actor')

    doc.deleteRange({}, 0, 6)
    c = doc.read({})
    expect(c.text).toBe('one\ntwo')
    expect(c.blocks[0].path).toHaveLength(2)
    expect(doc.locate({}, 5)).toEqual({ id: c.blocks[1].id, index: 1, path: c.blocks[1].path })

    doc.replace({}, importMarkdown('**bold** x').content)
    expect(inheritedMarks(doc.read({}), 4)).toEqual([{ type: 'strong' }])

    const insert = { steps: [{ splice: { at: 0, delete: '', insert: '😀' } }] }
    expect(mapPos(insert, 0)).toMatchObject({ pos: 2, deleted: false })
    expect(mapPos(insert, 0, 'before').pos).toBe(0)
    expect(() => mapPos(insert, 0, 'sideways')).toThrow(/assoc/)
  })

  it('diffRange lowers a range an editor knows as one exact splice', () => {
    const before = importMarkdown('😀 Paragraph one\n\ntwo').content
    const after = { ...before, text: '😀 Paraxgraph one\ntwo' }
    const { steps } = diffRange(before, after, { from: 7, to: 7 })
    expect(steps).toHaveLength(1)
    expect(steps[0].splice).toMatchObject({ at: 7, delete: '', insert: 'x' })
    expect(diff(before, after).steps[0].splice.delete).toBe('Paragraph')
    expect(() => diffRange(before, after, { from: 7 })).toThrow(/range.to/)
  })

  it('a placeholder a change puts is filled under the actor and named in minted', () => {
    const doc = blankDoc()
    doc.replace({}, importMarkdown('one').content)
    const base = doc.hash({})
    doc.apply({}, { steps: [{ splice: { at: 3, insert: ' more' } }] })
    const change = { steps: [
      { splice: { at: 3, insert: '\nnew', blocks: [{ id: '?n', kind: 'para' }] } },
      { setBlock: { id: '?n', kind: 'heading', attrs: { level: 2 } } },
    ] }
    const receipt = doc.apply({}, change, { base, actor: 'ann' })
    expect(receipt).toMatchObject({ rebased: true, minted: { '?n': 'ann:b0' } })
    expect(doc.read({}).blocks[1]).toEqual({ id: 'ann:b0', kind: 'heading', attrs: { level: 2 } })
  })

  it('repair reads a merged state as a valid value, offsets in UTF-16', () => {
    const valid = importMarkdown('😀 **ab**').content
    expect(repair(valid)).toEqual({ content: valid, repairs: [], change: { steps: [] } })

    const merged = {
      text: '😀 ab\nc',
      blocks: [
        { id: 'b0', kind: 'para', path: ['gone'] },
        { id: 'b1', kind: 'para' },
      ],
      marks: [{ start: 3, end: 5, type: 'strong' }],
      anchors: [{ id: 'a', start: 4, end: 4, range: true }],
    }
    const { content, repairs } = repair(merged)
    expect(content.blocks[0].path).toBeUndefined()
    expect(content.marks).toEqual([{ start: 3, end: 5, type: 'strong' }])
    expect(content.anchors).toBeUndefined()
    expect(repairs).toEqual([
      { repair: 'missing_container', block: 'b0', container: 'gone' },
      { repair: 'collapsed_anchor', id: 'a' },
    ])
    expect(() => repair({ text: 1 })).toThrow(/repair/)

    const forbidden = { text: '😀\r ab', blocks: [{ id: 'b0', kind: 'para' }], anchors: [{ id: 'p', start: 4, end: 4 }] }
    const read = repair(forbidden)
    expect(read.change.steps).toEqual([{ splice: { at: 2, delete: '\r', insert: '' } }])
    expect(mapPos(read.change, 4).pos).toBe(read.content.anchors[0].start)
  })

  it('replace takes what normalize answers, and both refuse what is not valid', () => {
    const doc = blankDoc()
    const content = {
      text: 'a bold',
      blocks: [{ id: 'b0', kind: 'para' }],
      marks: [{ start: 2, end: 6, type: 'strong' }, { start: 3, end: 6, type: 'strong' }],
    }
    expect(normalize(content)).toEqual({ ...content, marks: [{ start: 2, end: 6, type: 'strong' }] })
    expect(() => doc.replace({}, content)).toThrow(/not in canonical form/)
    doc.replace({}, normalize(content))
    expect(doc.read({}).marks).toEqual(normalize(content).marks)
    expect(exportMarkdown(doc.read({}))).toBe('a\\\n**bold**')

    const table = importMarkdown('| H |\n| --- |\n| a |').content
    const shared = { ...table, text: 'a￼bold' }
    const message = (f) => {
      try {
        f()
      } catch (e) {
        return String(e.message ?? e).replace(/^\w+: /, '')
      }
      return null
    }
    expect(message(() => normalize(shared))).not.toBeNull()
    expect(message(() => normalize(shared))).toBe(message(() => doc.replace({}, shared)))
  })

  it('a hard break is a U+2028 inside one block, a split a new record', () => {
    const doc = blankDoc()
    doc.revise({}, 'one two')
    doc.apply({}, { steps: [{ splice: { at: 3, delete: ' ', insert: ' ' } }] })
    expect(doc.read({}).blocks).toHaveLength(1)
    expect(exportMarkdown(doc.read({}))).toBe('one\\\ntwo')

    doc.apply({}, { steps: [{ splice: { at: 3, delete: ' ', insert: '\n', blocks: [{ id: 'b9', kind: 'para' }] } }] })
    expect(doc.read({}).blocks.map((b) => b.id)).toEqual([doc.read({}).blocks[0].id, 'b9'])
    expect(exportMarkdown(doc.read({}))).toBe('one\n\ntwo')
  })

  it('a nested step edits a table cell without costing the field its anchors', () => {
    const doc = blankDoc()
    doc.revise({}, 'intro\n\n| H |\n| --- |\n| a |')
    const island = doc.read({}).islands[0]
    expect(island.type).toBe('table')
    doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 0, end: 5 } }] })

    const cell = island.props.rows[1].cells[island.props.columns[0]]
    const edit = { steps: [{ splice: { at: 0, delete: 'a', insert: 'b' } }] }
    doc.apply({}, { steps: [{ nested: { island: island.id, slot: cell, change: edit } }] })
    expect(doc.read({}).islands[0].slots[cell].text).toBe('b')
    expect(doc.read({}).anchors).toEqual([{ id: 'c1', start: 0, end: 5 }])

    expectEditCode(() => doc.apply({}, { steps: [{ island: { id: 'nope', props: {} } }] }), 'edit::content_apply')
  })
})

describe('Document editor surface: card mutations', () => {
  const MD_WITH_CARDS = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card one.

~~~card-yaml
$kind: summary
~~~

Card two.
`

  it('removeCard → insertCard round-trips a card with fields (read shape == write shape)', () => {
    const doc = Document.parse(MD_WITH_CARDS)
    const removed = doc.removeCard(0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['summary'])
    expect(field(removed, 'foo')).toBe('bar')
    expect(doc.removeCard(5)).toBeUndefined()

    doc._uncheckedInsertCard(removed)
    const repushed = doc.cards[1]
    expect(repushed.kind).toBe('note')
    expect(field(repushed, 'foo')).toBe('bar')
  })

  it('a card or item carrying an unknown key is a loud error, not a silent drop', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    expect(() => doc._uncheckedInsertCard({ kind: 'note', stray: 1 })).toThrow(/stray/)
    const item = { type: 'field', key: 'x', value: 'Example', stray: true }
    expect(() => doc._uncheckedInsertCard({ kind: 'note', payloadItems: [item] })).toThrow(/stray/)
    expect(doc.cardCount).toBe(0)
  })

  it('a card the wire refuses carries the code its addressed mutator mints', () => {
    // Two doors onto one violation: what storeField / setQuillRef throw for a
    // name and a reference is what a card built from a wire throws.
    const doc = Document.parse(TEST_MARKDOWN)
    const withField = (key, value) => ({
      kind: 'note',
      payloadItems: [{ type: 'field', key, value }],
    })

    expectEditCode(() => doc._uncheckedInsertCard({ kind: 'BadKind' }), 'edit::invalid_kind_name')
    expectEditCode(() => doc._uncheckedInsertCard(withField('bad-name', 1)), 'edit::invalid_field_name')
    expectEditCode(
      () => doc._uncheckedInsertCard({ kind: 'note', quill: '@nope' }),
      'parse::invalid_quill_reference',
    )
  })

  it('insertCard inserts at an index and refuses one past the end', () => {
    const doc = Document.parse(MD_WITH_CARDS)
    doc._uncheckedInsertCard({ kind: 'intro' }, 0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['intro', 'note', 'summary'])
    expect(doc.cardCount).toBe(3)
    expectEditCode(() => doc._uncheckedInsertCard({ kind: 'note' }, 5), 'edit::index_out_of_range')
  })

  it('moveCard reorders and refuses an out-of-range index', () => {
    const doc = Document.parse(MD_WITH_CARDS)
    doc.moveCard(1, 0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['summary', 'note'])
    expectEditCode(() => doc.moveCard(5, 0), 'edit::index_out_of_range')
  })
})

describe('Document.equals / clone', () => {
  it('compares by value across a clone, an emit round-trip and a mutation', () => {
    const a = Document.parse(TEST_MARKDOWN)
    expect(a.equals(Document.parse(TEST_MARKDOWN))).toBe(true)
    expect(a.equals(Document.parse(a.toMarkdown()))).toBe(true)

    const clone = a.clone()
    expect(a.equals(clone)).toBe(true)
    clone._uncheckedStoreField('title', 'Changed')
    expect(a.equals(clone)).toBe(false)
    expect(field(a.main, 'title')).toBe('Test Document')
  })
})

describe('Document editor surface: storeField / removeField / replace / revise (card)', () => {
  it('storeField / removeField take a card address', () => {
    const doc = Document.parse(MD_WITH_CARD)
    doc._uncheckedStoreField({ card: 0, field: 'content' }, 'hello')
    expect(field(doc.cards[0], 'content')).toBe('hello')
    expect(doc.removeField({ card: 0, field: 'foo' })).toBe('bar')
    expect(hasField(doc.cards[0], 'foo')).toBe(false)
    expect(doc.removeField({ card: 0, field: 'foo' })).toBeUndefined()
  })

  it('revise / replace take a card address', () => {
    const doc = Document.parse(MD_WITH_CARD)
    const { applied } = doc._revise({ card: 0 }, 'New card body.')
    expect(exportMarkdown(doc.cards[0].body)).toBe('New card body.')
    expect(Array.isArray(applied.steps)).toBe(true)

    doc.replace({ card: doc.cards[0].id }, importMarkdown('Card body from **markdown**.').content)
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card body from **markdown**.')
  })

  it('revise anchors a dropped construct at the address it wrote', () => {
    const doc = Document.parse(MD_WITH_CARD)
    const { warnings } = doc._revise({ card: 0 }, 'Card <span>body</span>.')
    expect(doc.cards[0].body.text).toBe('Card body.')
    expect(warnings).toEqual([
      expect.objectContaining({
        severity: 'warning',
        code: 'parse::dropped_construct',
        path: 'cards.note[0].body',
        args: { construct: 'span', count: 1 },
      }),
    ])
    expect(doc._revise({ field: 'intro' }, '<kbd>x</kbd>').warnings[0].path).toBe('main.intro')
  })

  it('the annotated toMarkdown spells each anchor at its start and lists it at its path', () => {
    const doc = Document.parse('~~~\n$quill: q\n~~~\n\nMain **body**.\n\n~~~\n$kind: note\n~~~\n\nA note.\n')
    doc.apply({}, { steps: [{ anchor: { id: 'b', start: 5, end: 9 } }] })
    doc.apply({ card: doc.cards[0].id }, { steps: [{ anchor: { id: 'n', start: 2, end: 2 } }] })
    const read = doc.toMarkdown(undefined, { annotated: true })
    expect(read.markdown).toContain('Main <qm-anchor ref="b"></qm-anchor>**body**.')
    expect(read.markdown).toContain('A <qm-anchor ref="n"></qm-anchor>note.')
    expect(read.anchors).toEqual([
      { id: 'b', path: 'main.body', line: 'Main body.' },
      { id: 'n', path: 'cards.note[0].body', cardId: doc.cards[0].id, line: 'A note.' },
    ])

    expect(Document.parse(read.markdown).equals(Document.parse(doc.toMarkdown()))).toBe(true)
    const body = doc.read({})
    expect(exportMarkdown(body, { annotated: true })).toBe(doc.toMarkdown({}, { annotated: true }).markdown)
    expect(exportMarkdown(body)).toBe('Main **body**.')
  })

  it('a revise reads the annotated read back: a moved tag moves its anchor, a ref nothing holds reports', () => {
    const doc = Document.parse('~~~\n$quill: q\n~~~\n\nMain body here.\n')
    doc.apply({}, { steps: [{ anchor: { id: 'b', start: 5, end: 9 } }] })
    const read = doc.toMarkdown(undefined, { annotated: true }).markdown
    expect(read).toContain('Main <qm-anchor ref="b"></qm-anchor>body here.')
    const moved = read.replace('Main <qm-anchor ref="b"></qm-anchor>body here.', 'Main body <qm-anchor ref="b"></qm-anchor>here.')
    const receipt = doc._revise({}, moved.split('~~~\n\n')[1].trim() + ' <qm-anchor ref="gone"></qm-anchor>')
    expect(receipt.droppedAnchors).toEqual([])
    expect(receipt.warnings).toEqual([
      expect.objectContaining({ code: 'parse::dropped_construct', args: { construct: 'qm-anchor[ref]', count: 1 } }),
    ])
    expect(doc.anchorContext('b')).toMatchObject({ quote: 'here' })
  })

  it('a cell anchor is read, and an id the field already holds is refused', () => {
    const doc = Document.parse('~~~\n$quill: q\n~~~\n\nLead.\n\n| a |\n|---|\n| cell text |\n')
    const { id, props } = doc.read({}).islands[0]
    const cell = props.rows[1].cells[props.columns[0]]
    doc.apply({}, { steps: [{ nested: { island: id, slot: cell, change: { steps: [{ anchor: { id: 'c1', start: 5, end: 9 } }] } } }] })
    expect(doc.anchorContext('c1')).toMatchObject({ path: 'main.body', quote: 'text', prefix: 'cell ', suffix: '' })
    expect(doc.toMarkdown(undefined, { annotated: true }).anchors).toEqual([{ id: 'c1', path: 'main.body', line: 'cell text' }])
    expect(Document.fromStored(doc.toStored()).warnings).toEqual([])

    expectEditCode(() => doc.apply({}, { steps: [{ anchor: { id: 'c1', start: 0, end: 4 } }] }), 'edit::content_apply')

    const { droppedAnchors } = doc.apply({}, { steps: [{ splice: { at: 6, delete: '\uFFFC', insert: '' } }] })
    expect(droppedAnchors).toEqual([{ id: 'c1', path: 'main.body', quote: 'text', prefix: 'cell ', suffix: '', island: id, slot: cell }])
  })

  it('every card-addressed verb throws edit::index_out_of_range when the card is absent', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    const addr = { card: 0, field: 'foo' }
    expectEditCode(() => doc._uncheckedStoreField(addr, 'x'), 'edit::index_out_of_range')
    expectEditCode(() => doc.removeField(addr), 'edit::index_out_of_range')
    expectEditCode(() => doc._revise({ card: 0 }, 'x'), 'edit::index_out_of_range')
    expectEditCode(() => doc.read({ card: 0 }), 'edit::index_out_of_range')
    expectEditCode(() => doc.read({ card: 'nobody' }), 'edit::unknown_card_id')
    expectEditCode(() => doc.replace({ card: 'nobody' }, importMarkdown('x').content), 'edit::unknown_card_id')
    expectEditCode(() => doc.apply({ card: 'nobody' }, { steps: [] }), 'edit::unknown_card_id')
  })

  it('a content write names its card by id, and lands on it wherever the card moved', () => {
    const doc = Document.parse('~~~\n$quill: q\n~~~\n\n~~~\n$kind: note\n~~~\n\nHello\n\n~~~\n$kind: note\n~~~\n\nHello\n')
    const id = doc.cards[1].id
    const base = doc.hash({ card: id })
    doc.moveCard(1, 0)
    expect(() => doc.apply({ card: 1 }, { steps: [] }, { base })).toThrow(/\$id/)
    doc.apply({ card: id }, { steps: [{ splice: { at: 5, delete: '', insert: ' world' } }] }, { base })
    expect(doc.cards.map((c) => [c.id, c.body.text])).toEqual([[id, 'Hello world'], [expect.any(String), 'Hello']])
  })
})

describe('Document editor surface: $ext', () => {
  it('storeExt / getExt / removeExt round the whole map on the main card', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    expect(doc.getExt({})).toBeUndefined()
    const ext = { editor: { title: 'A' }, agent: { pinned: true } }
    doc.storeExt({}, ext)
    expect(doc.main.ext).toEqual(ext)
    expect(doc.getExt({})).toEqual(ext)
    expect(doc.removeExt()).toEqual(ext)
    expect(doc.main.ext == null).toBe(true)
    expect(doc.removeExt()).toBeUndefined()
    expect(() => doc.storeExt({}, 'nope')).toThrow()
    expect(() => doc.storeExt({}, 42)).toThrow()
  })

  it('card-level ext verbs target the card at index and take a card address only', () => {
    const doc = Document.parse(TEST_MARKDOWN)
    doc._uncheckedInsertCard({ kind: 'note', body: 'x' })
    doc.storeExt({ card: 0 }, { agent: { note: 'y' } })
    expect(doc.cards[0].ext.agent.note).toBe('y')
    expect(doc.getExt({ card: 0 }).agent.note).toBe('y')
    expect(doc.removeExt({ card: 0 }).agent.note).toBe('y')
    expect(doc.cards[0].ext == null).toBe(true)
    expect(() => doc.getExt({ field: 'title' })).toThrow()
    expectEditCode(() => doc.storeExt({ card: 5 }, {}), 'edit::index_out_of_range')
    expectEditCode(() => doc.removeExt({ card: 5 }), 'edit::index_out_of_range')
    expectEditCode(() => doc.getExt({ card: 5 }), 'edit::index_out_of_range')
  })
})

describe('quill.open + session.render', () => {
  it('should support open + session.render with pageCount', () => {
    const engine = new Engine()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.parse(TEST_MARKDOWN)

    const session = engine.open(quill, doc)
    expect(typeof session.pageCount).toBe('number')
    expect(session.pageCount).toBeGreaterThan(0)

    const defaultFmt = session.render()
    expect(defaultFmt.artifacts.length).toBeGreaterThan(0)
    expect(defaultFmt.artifacts[0].mimeType).toBe('application/pdf')

    const allPages = session.render({ format: 'svg' })
    expect(allPages.artifacts.length).toBe(session.pageCount)
    expect(allPages.artifacts[0].mimeType).toBe('image/svg+xml')

    const subset = session.render({ format: 'png', ppi: 80, pages: [0, 0] })
    expect(subset.artifacts.length).toBe(2)
    expect(subset.artifacts[0].mimeType).toBe('image/png')
  })

  it('refuses an out-of-bounds page and a page selection on PDF', () => {
    const engine = new Engine()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const session = engine.open(quill, Document.parse(TEST_MARKDOWN))
    expect(() => session.render({ format: 'png', ppi: 80, pages: [0, session.pageCount + 10] })).toThrow()
    expect(() => session.render({ format: 'pdf', pages: [0] })).toThrow()
  })
})

describe('quill.metadata', () => {
  const META_QUILL_YAML = `quill:
  name: meta_test_quill
  version: "0.2.1"
  backend: typst
  description: Metadata test
  license: Apache-2.0 OR MIT
  homepage: https://example.com/quills

typst:
  plate_file: plate.typ

main:
  description: The main card schema
  fields:
    title:
      type: text
      description: The title

card_kinds:
  indorsement:
    description: Indorsement
    fields:
      signature_block:
        type: text
`

  it('exposes identity on metadata and schemas on dedicated getters', () => {
    const quill = Quill.fromTree(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )

    // metadata mirrors the `quill:` section of Quill.yaml: identity only, and
    // its key order is the one BINDINGS.md pins across both surfaces.
    const meta = quill.metadata
    expect(Object.keys(meta)).toEqual([
      'name',
      'version',
      'backend',
      'author',
      'description',
      'license',
      'homepage',
    ])
    expect(meta.name).toBe('meta_test_quill')
    expect(meta.version).toBe('0.2.1')
    expect(meta.backend).toBe('typst')
    expect(meta.author).toBe('Unknown')
    expect(meta.description).toBe('Metadata test')
    expect(meta.license).toBe('Apache-2.0 OR MIT')
    expect(meta.homepage).toBe('https://example.com/quills')

    // An undeclared license or homepage is absent, not null.
    expect(Object.keys(Quill.fromTree(makeQuill()).metadata)).toEqual([
      'name',
      'version',
      'backend',
      'author',
      'description',
    ])

    // Plain objects, not Maps: they survive a JSON round-trip whole.
    const schema = JSON.parse(JSON.stringify(quill.schema))
    expect(schema.main.description).toBe('The main card schema')
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.card_kinds.main).toBeUndefined()
    expect(schema.card_kinds.indorsement.fields.signature_block).toBeDefined()
  })

  it('surfaces the load\'s advisory diagnostics on quill.warnings', () => {
    const clean = Quill.fromTree(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )
    expect(clean.warnings).toEqual([])

    const WARNING_QUILL_YAML = `quill:
  name: warn_quill
  version: "1.0.0"
  backend: typst
  description: Carries one advisory

typst:
  plate_file: plate.typ

main:
  fields:
    title:
      type: text

card_kinds:
  skills:
    body:
      enabled: false
    fields:
      items:
        type: array
        items:
          type: text
`
    const quill = Quill.fromTree(
      makeQuill({ name: 'warn_quill', plate: TEST_PLATE, quillYaml: WARNING_QUILL_YAML }),
    )

    expect(quill.warnings.map((d) => d.code)).toEqual(['quill::bodiless_card_kind'])
    expect(formatDiagnostic(quill.warnings[0])).toContain('(quill::bodiless_card_kind)')
  })
})

// Which documents produce which diagnostics is core's, pinned in
// `crates/quillmark/tests/validate_test.rs`. Here: the result crosses as a
// plain JS array, and a diagnostic keeps its `code` / `path` / `hint`.
describe('quill.validate', () => {
  const QUILL_YAML = `quill:
  name: validate_smoke_test
  version: "1.0.0"
  backend: typst
  description: Smoke test for validate

main:
  fields:
    title:
      type: text
    count:
      type: integer

card_kinds:
  note:
    fields:
      prose:
        type: text
`

  const buildQuill = () => {
    return Quill.fromTree(makeQuill({ name: 'validate_smoke_test', quillYaml: QUILL_YAML }))
  }

  it('returns an empty array for a complete, well-formed document', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "Hello"
count: 1
~~~
`
    expect(quill.validate(Document.parse(md))).toEqual([])
  })

  it('forwards a type_mismatch with canonical code, path, and hint as plain JSON', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "Hello"
count: "not-a-number"
~~~
`
    const diags = JSON.parse(JSON.stringify(quill.validate(Document.parse(md))))
    const mismatch = diags.find((d) => d.code === 'validation::type_mismatch')
    expect(mismatch.path).toBe('main.count')
    expect(typeof mismatch.hint).toBe('string')
  })
})

// The authoring document and the authored/default/blank ladder are core's
// (`core/src/quill/authoring.rs`, `core/src/quill/resolved.rs`). Here: the
// schema DTO's shape, the authoring form crossing, and an unanswered cell
// reaching render.
describe('value schema model', () => {
  // The plate reads `qm.data.title` (no default) and substitutes the optional
  // `qm.data.subtitle` if present, so one quill carries both cell states.
  const SCHEMA_QUILL_YAML = `quill:
  name: schema_test
  version: "1.0.0"
  backend: typst
  description: value coverage

typst:
  plate_file: plate.typ

main:
  fields:
    title:
      type: text
      description: Document title (no default)
    subtitle:
      type: text
      default: "Untitled subtitle"
      description: Document subtitle (defaulted, shippable)
`

  const SCHEMA_PLATE = `#import "@local/quillmark-helper:0.2.0" as qm
#let title = qm.data.title
#let subtitle = qm.data.at("subtitle", default: "")
#let body = qm.data.at("$body")

= #title

#subtitle

#body`

  const buildQuill = () => {
    const engine = new Engine()
    const quill = Quill.fromTree(
      makeQuill({
        name: 'schema_test',
        plate: SCHEMA_PLATE,
        quillYaml: SCHEMA_QUILL_YAML,
      }),
    )
    return { engine, quill }
  }

  it('schema fields carry their default, and so does the authoring schema', () => {
    const { quill } = buildQuill()
    const fields = quill.schema.main.fields

    expect(fields.title.default).toBeUndefined()
    expect(fields.subtitle.default).toBe('Untitled subtitle')

    const authoring = quill.authoringSchema.properties.fields.properties
    expect(authoring.title).toEqual({
      description: 'Document title (no default)',
      type: ['string', 'null'],
    })
    expect(authoring.subtitle.default).toBe('Untitled subtitle')
  })

  it('a document crosses the authoring form and back', () => {
    const { quill } = buildQuill()
    const doc = quill.fromAuthoring({ fields: { title: 'Hello', subtitle: null }, body: 'Some *prose*.' })
    expect(doc.quillRef).toBe('schema_test@1.0')
    expect(quill.toAuthoring(doc)).toEqual({ fields: { title: 'Hello' }, body: 'Some *prose*.' })

    let caught
    try {
      quill.fromAuthoring({ fields: [], cards: [{}] })
    } catch (e) {
      caught = e
    }
    expect(caught?.diagnostics?.map((d) => [d.code, d.path, d.args?.pointer])).toEqual([
      ['authoring::invalid_shape', undefined, '/fields'],
      ['authoring::invalid_shape', undefined, '/cards/0/kind'],
    ])
  })

  it('render blank-fills an unanswered cell', () => {
    const { engine, quill } = buildQuill()
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title:
~~~

# Body
`
    const result = engine.render(quill, Document.parse(md), { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
  })
})
