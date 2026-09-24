/**
 * The canonical `@quillmark/wasm` API end to end: a CORE quill and
 * document handed to `Engine` render correctly, the engine cloning them into the
 * Typst backend's memory on demand without the caller ever seeing a backend
 * handle.
 */
import { describe, it, expect, beforeAll, afterAll } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {
  Engine,
  DocumentWriter,
  CardWriter,
  DocumentReader,
  CardReader,
  MAIN_CARD_ADDR,
  isQuillmarkError,
  assignInstances,
  init,
} from '@quillmark-wasm/runtime'
// The namespace too: the bind set below is derived from the exports rather than
// listed, so a fifth writer/reader class joins it by existing.
import * as runtime from '@quillmark-wasm/runtime'
// Pin that the runtime's Quill IS the internal core build's class (handed out,
// not a parallel wrapper). This imports the internal core artifact directly:
// `pkg/core` is NOT a public package subpath, it is the build the gate draws
// from.
import { Quill as CoreQuill, Document as CoreDocument } from '../../../pkg/core/wasm.js'
import {
  makeQuill,
  makeSampleFormQuill,
  expectEditCode,
  isClass,
  caughtFrom,
} from './test-helpers.js'

// The consumer contract, exercised as a consumer writes it: the gate is the only
// door to the core surface. This also instantiates the core build the `CoreQuill`
// identity pin below imports directly (same resolved file, same module
// instance).
const { Quill, Document, importMarkdown, exportMarkdown } = await init()

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let body = data.at("$body")

= #title

#body`

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is a test document.`

function makeRuntimeQuill() {
  return Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
}

// A quill declaring one construct its plate does not typeset, so `quill.parse`
// draws a `plate::unsupported_construct` warning off a body holding a rule.
const DECLINE_QUILL_YAML = `quill:
  name: decliner
  version: "1.0"
  backend: typst
  description: A quill that typesets no horizontal rule

main:
  body:
    unsupported: [rule]
  fields: {}
`

const DECLINE_PLATE = `#import "@local/quillmark-helper:0.1.0": data

#data.at("$body")`

const PKG_DIR = path.resolve(import.meta.dirname, '..', '..', '..', 'pkg')

/** Read a field value from a card's payloadItems list by key. */
const fieldOf = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

describe('@quillmark/wasm: surface', () => {
  // IMPLEMENTATION PIN: the gate hands out the internal core build's classes
  // verbatim (never wraps). There is exactly one public entry point, so this is
  // an internal structural fact rather than a cross-entry-point contract. If it
  // fails, a wrapper was put in front of the classes: a breaking change, not a
  // refactor. See runtime.js.
  it('hands out the internal core build classes verbatim (no parallel wrappers)', () => {
    expect(Quill).toBe(CoreQuill)
    expect(Document).toBe(CoreDocument)
  })

  it('builds a canonical Quill with a backendId and a round-tripping tree', () => {
    const quill = makeRuntimeQuill()
    expect(quill.backendId).toBe('typst')

    // toTree is the inverse of fromTree: re-materializing reproduces an
    // equivalent quill (same backend, same files).
    const tree = quill.toTree()
    expect(tree).toBeInstanceOf(Map)
    expect(tree.has('Quill.yaml')).toBe(true)
    const rebuilt = Quill.fromTree(tree)
    expect(rebuilt.backendId).toBe('typst')
  })

  // ERROR CONTRACT: every fallible method throws a real Error carrying a
  // non-empty `diagnostics` array (the QuillmarkError structural interface).
  // isQuillmarkError is the exported narrowing guard for it.
  it('throws satisfy isQuillmarkError with non-empty structured diagnostics', () => {
    let caught
    try {
      Document.fromMarkdown('~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: [unclosed\n~~~\n\nbody')
    } catch (e) {
      caught = e
    }
    expect(caught).toBeInstanceOf(Error)
    expect(isQuillmarkError(caught)).toBe(true)
    expect(caught.diagnostics.length).toBeGreaterThan(0)
    const d = caught.diagnostics[0]
    expect(typeof d.message).toBe('string')
    expect(d.severity).toBeDefined()
    // message derives from the diagnostics (first message or an aggregate)
    expect(caught.message.length).toBeGreaterThan(0)
  })

  it('isQuillmarkError rejects non-quillmark values', () => {
    expect(isQuillmarkError(new Error('plain'))).toBe(false) // no diagnostics
    expect(isQuillmarkError({ diagnostics: [] })).toBe(false) // not an Error
    expect(isQuillmarkError(undefined)).toBe(false)
    expect(isQuillmarkError('boom')).toBe(false)
    // structural acceptance: any Error carrying a diagnostics array narrows,
    // regardless of which build or WASM instance constructed it
    const foreign = Object.assign(new Error('x'), { diagnostics: [] })
    expect(isQuillmarkError(foreign)).toBe(true)
  })
})

// The typed-writer sugar binds a quill to a document once, so writes are bare
// `set` / `setAll` / `reviseField` / `card(i).set`: the JS twin of Rust's
// `quill.writer(doc)`. Each verb forwards to the right address, coerces the
// value through its declared type, and carries a refused write's diagnostic
// code and DocPath out to the caller unswallowed.
describe('@quillmark/wasm: DocumentWriter / CardWriter (bind the quill once)', () => {
  const EDITOR_QUILL_YAML = `quill:
  name: editor_test
  version: "1.0"
  backend: typst
  description: Typed writer sugar test

main:
  fields:
    subject:
      type: richtext
      inline: true
    qty:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: richtext
`
  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'editor_test', plate: TEST_PLATE, quillYaml: EDITOR_QUILL_YAML }))
  const blankDoc = () => Document.fromMarkdown('~~~card-yaml\n$quill: editor_test\n~~~\n\nBody.')

  it('quill.writer(doc) is the front door and returns a DocumentWriter', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    expect(ed).toBeInstanceOf(DocumentWriter)
    // The factory is sugar over the constructor: same class, no wrapping.
    expect(new DocumentWriter(quill, blankDoc())).toBeInstanceOf(DocumentWriter)
  })

  it('set / setAll bind the quill once and strict-commit main-card fields', () => {
    const ed = buildQuill().writer(blankDoc())
    ed.set('qty', '3') // schema field → strict coerce
    expect(fieldOf(ed.document.main, 'qty')).toBe(3)

    ed.setAll({ subject: 'Q3 **results**', qty: '5' })
    expect(fieldOf(ed.document.main, 'qty')).toBe(5)
  })

  it('reviseBody / reviseField write from markdown and return a Delta', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    expect(Array.isArray(ed.reviseBody('New **body**.').ops)).toBe(true)
    expect(ed.document.bodyMarkdown()).toBe('New **body**.')
    expect(Array.isArray(ed.reviseField('subject', 'Q3 **results**').ops)).toBe(true)
    expect(quill.reader(ed.document).get('subject')).toBe('Q3 **results**')
  })

  it('addCard commits fields and body; removeCard returns the card', () => {
    const ed = buildQuill().writer(blankDoc())
    // `body` here is the card's richtext FIELD; the third arg is the card body.
    ed.addCard('note', { body: 'Field **body**.' }, 'Card body text.')
    expect(ed.document.cards[0].kind).toBe('note')
    expect(exportMarkdown(fieldOf(ed.document.cards[0], 'body'))).toBe('Field **body**.')
    expect(exportMarkdown(ed.document.cards[0].body)).toBe('Card body text.')
    expect(ed.removeCard(0).kind).toBe('note')
    expect(ed.document.cards).toHaveLength(0)
  })

  it('card(i).set / reviseBody / reviseField address the composable card', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: editor_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    const ed = buildQuill().writer(doc)
    ed.card(0).set('body', 'Card **body**.')
    expect(exportMarkdown(fieldOf(doc.cards[0], 'body'))).toBe('Card **body**.')
    expect(Array.isArray(ed.card(0).reviseBody('Card body md.').ops)).toBe(true)
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card body md.')
    // card(i).reviseField is the typed, anchor-preserving field write.
    const delta = ed.card(0).reviseField('body', 'Revised **field**.')
    expect(exportMarkdown(fieldOf(doc.cards[0], 'body'))).toBe('Revised **field**.')
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('a bad card index throws at write time, not at card()', () => {
    const ed = buildQuill().writer(blankDoc())
    const cardEd = ed.card(9) // lazy: constructing the CardWriter never throws
    expect(cardEd).toBeInstanceOf(CardWriter)
    expectEditCode(() => cardEd.set('body', 'x'), 'edit::index_out_of_range')
  })

  it('set refuses an unknown name, a strict type mismatch and an inline violation', () => {
    const ed = buildQuill().writer(blankDoc())
    expectEditCode(() => ed.set('stray', 'x'), 'edit::unknown_field')
    expect(fieldOf(ed.document.main, 'stray')).toBeUndefined()
    expectEditCode(() => ed.set('qty', 'not-a-number'), 'edit::field_coercion_failed')
    expectEditCode(() => ed.set('subject', 'line one\n\nline two'), 'edit::field_not_inline')
  })

  it('a refused write carries the DocPath it anchors to, and pathFor mints the same one', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: editor_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    const ed = buildQuill().writer(doc)
    // The path a diagnostic thrown by `fn` carries.
    const pathOf = (fn) => {
      try {
        fn()
      } catch (err) {
        return err.diagnostics[0].path
      }
      throw new Error('expected a throw, got none')
    }
    // A main field conform error anchors at the rooted main field DocPath…
    expect(pathOf(() => ed.set('qty', 'not-a-number'))).toBe('main.qty')
    // …a card field is kind-qualified with its absolute index…
    expect(pathOf(() => ed.card(0).set('stray', 'x'))).toBe('cards.note[0].stray')
    // …and a structural out-of-range op anchors at the array slot.
    expect(pathOf(() => doc.moveCard(9, 0))).toBe('cards[9]')
    // `pathFor` mints what the anchor carries, so a consumer's path and the
    // engine's agree without a kind table of its own.
    expect(doc.pathFor({ card: 0, field: 'stray' })).toBe(
      pathOf(() => ed.card(0).set('stray', 'x')),
    )
  })

  it('setAll applies nothing when one entry is refused', () => {
    const ed = buildQuill().writer(blankDoc())
    expectEditCode(() => ed.setAll({ qty: '5', titel: 'oops' }), 'edit::unknown_field')
    expect(fieldOf(ed.document.main, 'qty')).toBeUndefined()
  })

  it('card(i).setAll typed-commits a batch and refuses an unknown name or index', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: editor_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    const ed = buildQuill().writer(doc)
    ed.card(0).setAll({ body: 'Card **body**.' })
    expect(exportMarkdown(fieldOf(doc.cards[0], 'body'))).toBe('Card **body**.')
    expectEditCode(() => ed.card(0).setAll({ stray: 'x' }), 'edit::unknown_field')
    expectEditCode(() => ed.card(9).setAll({ body: 'x' }), 'edit::index_out_of_range')
  })
})

// The typed-reader sugar is the read twin of the writer above: bind the quill
// once and read each field by its declared type (a richtext field to markdown,
// every other type verbatim) with schema authority, so an unknown field name
// throws rather than reading back `undefined` off the quill-free `Document`.
describe('@quillmark/wasm: DocumentReader / CardReader (the schema-plane read)', () => {
  const VIEW_QUILL_YAML = `quill:
  name: view_test
  version: "1.0"
  backend: typst
  description: Typed reader sugar test

main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
    qty:
      type: integer
    recipients:
      type: array
      items:
        type: plaintext
    paragraphs:
      type: array
      items:
        type: richtext
    tags:
      type: array
      items:
        type: string
    letterhead:
      type: object
      properties:
        motto:
          type: richtext
        code:
          type: string
    rows:
      type: array
      items:
        type: object
        properties:
          notes:
            type: richtext

card_kinds:
  note:
    fields:
      body:
        type: richtext
      lines:
        type: array
        items:
          type: plaintext
`
  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'view_test', plate: TEST_PLATE, quillYaml: VIEW_QUILL_YAML }))
  const seededDoc = (quill) => {
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nMain **body**.')
    const w = quill.writer(doc)
    w.set('subject', 'Q3 **results**')
    w.set('qty', '3')
    w.addCard('note', { body: 'A *card* field.' }, 'Card body.')
    return doc
  }

  it('quill.reader(doc) is the front door and returns a DocumentReader', () => {
    const quill = buildQuill()
    const v = quill.reader(seededDoc(quill))
    expect(v).toBeInstanceOf(DocumentReader)
    expect(new DocumentReader(quill, seededDoc(quill))).toBeInstanceOf(DocumentReader)
  })

  it('interprets by declared type: richtext → markdown, plaintext → literal, scalar → canonical', () => {
    const quill = buildQuill()
    const doc = seededDoc(quill)
    quill.writer(doc).set('note', 'a *literal* line') // marks verbatim under plaintext
    const v = quill.reader(doc)
    expect(v.get('subject')).toBe('Q3 **results**') // richtext projects to markdown
    expect(v.get('note')).toBe('a *literal* line') // plaintext projects verbatim
    expect(v.get('qty')).toBe(3) // scalar returns canonical
  })

  it('absence returns undefined; an unknown name throws (schema authority)', () => {
    const quill = buildQuill()
    const v = quill.reader(Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.'))
    expect(v.get('subject')).toBeUndefined() // absent, not a typo
    expectEditCode(() => v.get('nope'), 'edit::unknown_field') // typo, not absent
  })

  it('an absent field addr reads the body markdown', () => {
    const quill = buildQuill()
    const v = quill.reader(seededDoc(quill))
    expect(v.bodyMarkdown()).toBe('Main **body**.')
    expect(v.get({})).toBe('Main **body**.')
  })

  it('card(i).get reads a card field through its $kind schema', () => {
    const quill = buildQuill()
    const v = quill.reader(seededDoc(quill))
    expect(v.card(0).kind).toBe('note')
    expect(v.card(0).get('body')).toBe('A *card* field.')
    expect(v.card(0).bodyMarkdown()).toBe('Card body.')
    expectEditCode(() => v.card(0).get('nope'), 'edit::unknown_field')
  })

  it('a bad card index throws at read time, not at card()', () => {
    const quill = buildQuill()
    const cardReader = quill.reader(seededDoc(quill)).card(9)
    expect(cardReader).toBeInstanceOf(CardReader)
    expectEditCode(() => cardReader.get('body'), 'edit::index_out_of_range')
  })

  // The bound door is what makes a stored form a property of the codec rather
  // than of the construction lane.
  it('the bound door lands both codecs at their canonical rest', () => {
    const quill = buildQuill()
    const md =
      "~~~card-yaml\n$quill: view_test\nsubject: Q3 **results**\nnote: 'a *literal* line'\n~~~\n\nBody."
    const bound = quill.parse(md)
    expect(typeof bound.getStored('subject')).toBe('object') // richtext: the Content object
    expect(bound.getStored('note')).toBe('a *literal* line') // plaintext: the literal
    expect(bound.warnings).toEqual([])

    // conform is the same walk on a document that arrived any other way, and it
    // converges to identical bytes. A second pass is a no-op.
    const transported = Document.fromMarkdown(md)
    expect(quill.conform(transported)).toEqual([])
    expect(transported.equals(bound)).toBe(true)
    expect(quill.conform(transported)).toEqual([])
    expect(transported.toStored()).toBe(bound.toStored())
  })

  it('a value the strict write refuses rests authored with a conform warning', () => {
    const doc = buildQuill().parse('~~~card-yaml\n$quill: view_test\nsubject: 42\n~~~\n\nBody.')
    expect(doc.getStored('subject')).toBe(42)
    expect(doc.warnings.map((d) => d.code)).toContain('conform::field_decode')
  })

  it('nothing conforms under the wrong quill', () => {
    const quill = buildQuill()
    const md = '~~~card-yaml\n$quill: other_quill\nsubject: hi\n~~~\n\nBody.'
    expectEditCode(() => quill.parse(md), 'quill::name_mismatch')

    // The transport door still opens it, and conform reports the same mismatch
    // without touching the document.
    const doc = Document.fromMarkdown(md)
    const before = doc.toStored()
    expectEditCode(() => quill.conform(doc), 'quill::name_mismatch')
    expect(doc.toStored()).toBe(before)
  })

  it('getContent reads a field, the body, and a card field as Content', () => {
    const quill = buildQuill()
    const v = quill.reader(seededDoc(quill))
    expect(v.getContent('subject').text).toBe('Q3 results')
    expect(v.getContent({}).text).toBe('Main body.')
    expect(v.card(0).getContent('body').text).toBe('A card field.')
    expectEditCode(() => v.getContent('qty'), 'edit::field_not_content')
    expectEditCode(() => v.card(9).getContent('body'), 'edit::index_out_of_range')
  })

  // A read is also a write input, so what it hands back has to be legal to hand
  // straight back in. Both reads answer in the one canonical form, which omits a
  // zero `instance`.
  it('a Content read answers the canonical form', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      "~~~card-yaml\n$quill: view_test\nparagraphs: ['> a']\n~~~\n\n> a\n\n- b"
    )
    const v = quill.reader(doc)
    const written = (rt) => rt.lines.flatMap((l) => l.containers).map((c) => c.instance)
    expect(written(v.getContent({}))).toEqual([undefined, undefined])
    expect(written(v.getContentAt('paragraphs', [0]))).toEqual([undefined])
  })

  it('a Content written through set keeps its anchors and island ids', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    const w = quill.writer(doc)
    const v = quill.reader(doc)
    w.set('paragraphs', ['Alpha ![pic](u) bold'])

    const rt = v.getContentAt('paragraphs', [0])
    rt.marks.push({ type: 'anchor', attrs: { id: 'c1' }, start: 0, end: 5 })
    rt.islands[0].id = 'isl-7'
    w.set('paragraphs', [rt])

    const back = v.getContentAt('paragraphs', [0])
    expect(back.marks.find((m) => m.type === 'anchor')).toMatchObject({ attrs: { id: 'c1' } })
    expect(back.islands[0].id).toBe('isl-7')
  })

  it('getContentAt takes a mixed index/key path, on the document and on a card', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('rows', [{}, { notes: 'a *note*' }])
    quill.writer(doc).addCard('note', { lines: ['a *b*'] })
    const v = quill.reader(doc)
    expect(v.getContentAt('rows', [1, 'notes']).text).toBe('a note')
    expect(v.getContentAt('rows', [0, 'notes'])).toBeUndefined()
    expect(v.card(0).getContentAt('lines', [0]).text).toBe('a *b*')
    expectEditCode(() => v.card(9).getContentAt('lines', [0]), 'edit::index_out_of_range')
    expect(() => v.getContentAt('rows', [null])).toThrow(/path\[0\]/)
    expect(() => v.getContentAt('rows', 0)).toThrow(/`path` must be an array/)
  })
})

// For every declared field: the value the render projection would use and the
// source rung it came from ("authored" | "default" | "blank"). Rows are an
// ordered array carrying their own `name`; the card body is a `body` sibling,
// not a row in `fields`. Value and provenance only: diagnostics stay
// validate(), guidance stays the schema. See prose/canon/SCHEMAS.md
// § "Value sources and projections".
describe('@quillmark/wasm: reader.resolve (the resolved-value view)', () => {
  const QUILL_YAML = `quill:
  name: field_states_test
  version: "1.0"
  backend: typst
  description: Resolved-field view coverage

main:
  body:
    example: "Example body prose."
  fields:
    title:
      type: string
    status:
      type: string
      default: draft
    notes:
      type: string
    count:
      type: integer
    author:
      type: string
      example: A. Author

card_kinds:
  note:
    fields:
      label:
        type: string
`

  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'field_states_test', quillYaml: QUILL_YAML }))

  const resolveOf = (md) => buildQuill().reader(Document.fromMarkdown(md)).resolve()

  // Rows are an ordered array; look one up by its `name`.
  const byName = (rows, name) => rows.find((r) => r.name === name)

  const MAIN_ONLY = `~~~card-yaml
$quill: field_states_test
$kind: main
title: Hello
~~~
`

  it('crosses as ordered rows of { name, value, source }, a body sibling, and card entries', () => {
    const resolved = JSON.parse(
      JSON.stringify(
        resolveOf(`${MAIN_ONLY}
~~~card-yaml
$kind: note
label: L
~~~
Note body.
`)
      )
    )
    const f = resolved.main.fields
    expect(f.map((r) => r.name)).toEqual(['title', 'status', 'notes', 'count', 'author'])
    expect(byName(f, 'title')).toEqual({ name: 'title', value: 'Hello', source: 'authored' })
    expect(byName(f, 'status').source).toBe('default')
    expect(byName(f, 'notes').source).toBe('blank')
    expect(resolved.main.body.source).toBe('blank')

    const card = resolved.cards[0]
    expect(card.kind).toBe('note')
    expect(card.index).toBe(0)
    expect(byName(card.fields, 'label')).toMatchObject({ value: 'L', source: 'authored' })
  })
})

describe('@quillmark/wasm: MAIN_CARD_ADDR (the named main-card address)', () => {
  it('is a frozen {} that a card-scoped verb takes as the main card', () => {
    expect(MAIN_CARD_ADDR).toEqual({})
    expect(Object.isFrozen(MAIN_CARD_ADDR)).toBe(true)
    const doc = new Document('editor_test')
    doc.storeFields(MAIN_CARD_ADDR, { title: 'Hello', qty: 3 })
    expect(fieldOf(doc.main, 'title')).toBe('Hello')
    expect(fieldOf(doc.main, 'qty')).toBe(3)
  })
})

describe('@quillmark/wasm: container run boundaries', () => {
  const LIST = { container: 'list_item', attrs: { ordered: false, start: 1, ordinal: 0 } }
  const QUOTE = { container: 'quote' }
  const content = (a, b) => ({
    text: 'a\nb',
    lines: [
      { kind: 'para', containers: [a] },
      { kind: 'para', containers: [b] },
    ],
    marks: [],
    islands: [],
  })

  /**
   * One pair, both halves of the weld rule: `assignInstances` stamps
   * `expected`, and a round trip re-mints exactly that. Import runs
   * `Content::normalize`, which mints the minimum against
   * `Container::same_weld`, so a JS stamp too coarse comes back welded and one
   * too eager comes back dropped — the Rust predicate read by running it rather
   * than re-spelled. A read omits a zero, hence the `undefined`.
   */
  const remints = (a, b, expected) => {
    const [x, y] = assignInstances([a, b])
    expect([x, y].map((c) => c.instance)).toEqual(expected)
    const back = importMarkdown(exportMarkdown(content(x, y)))
    expect(back.lines.map((l) => l.containers[0].instance)).toEqual(
      expected.map((n) => n || undefined)
    )
  }

  it('stamps on what the markdown projection can carry, not on equality', () => {
    // `start` and `ordinal` differ and the runs still weld: CommonMark reads
    // only a list's first number, and an ordinal is positional.
    const list = (over) => ({ ...LIST, attrs: { ...LIST.attrs, ...over } })
    remints(LIST, list({ start: 3 }), [0, 1])
    remints(LIST, list({ ordinal: 4 }), [0, 1])
    remints(QUOTE, QUOTE, [0, 1])
    // A shape the projection can tell apart needs no discriminator.
    remints(LIST, list({ ordered: true }), [0, 0])
  })

  it('alternates only across runs that would weld', () => {
    expect(assignInstances([LIST, LIST, LIST]).map((c) => c.instance)).toEqual([0, 1, 0])
    expect(assignInstances([LIST, QUOTE, LIST]).map((c) => c.instance)).toEqual([0, 0, 0])
    // A block carrying no container at this depth parts the runs on its own.
    expect(assignInstances([LIST, null, LIST]).map((c) => c && c.instance)).toEqual([0, null, 0])
    expect(assignInstances([]).length).toBe(0)
  })
})

describe('@quillmark/wasm: Engine (hidden core→backend crossing)', () => {
  // Warm the lazy Typst-backend import + first Typst compile once, outside any
  // timed test. `Engine.render` dynamically `import()`s the backend wasm binary
  // on first render: a one-time cost (large module instantiation) that on a
  // cold CI runner alone can approach the per-test ceiling. Paying it here keeps
  // the individual render tests warm (sub-second, like the SVG case) so a tight
  // per-test `testTimeout` still catches a genuine hang. The hook carries its own
  // generous timeout for the cold load.
  beforeAll(async () => {
    await new Engine().render(makeRuntimeQuill(), Document.fromMarkdown(TEST_MARKDOWN), {
      format: 'pdf',
    })
  }, 120000)

  it('renders a core Quill + Document to PDF without exposing a backend handle', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const result = await engine.render(quill, doc, { format: 'pdf' })
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.outputFormat).toBe('pdf')
    expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
    expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)

    // The caller's canonical handles survive the render (clones were transient
    // and freed inside the engine; the originals are untouched).
    expect(quill.backendId).toBe('typst')
    expect(doc.quillRef).toBe('test_quill')
  })

  it('renders to SVG and reports supported formats', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const svg = await engine.render(quill, doc, { format: 'svg' })
    expect(svg.outputFormat).toBe('svg')

    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('svg')
  })

  // ERROR.md § "Warning flow": `RenderResult.warnings` is pipeline order, the
  // load half ahead of the compile's. Only the runtime layer can merge them —
  // the document clone it renders comes through `fromStored`, which carries no
  // warnings, so the backend build's own merge has nothing to prepend.
  it('render fronts RenderResult.warnings with the load warnings, leaving doc.warnings intact', async () => {
    const quill = Quill.fromTree(
      makeQuill({ name: 'decliner', plate: DECLINE_PLATE, quillYaml: DECLINE_QUILL_YAML }),
    )
    const doc = quill.parse('~~~card-yaml\n$quill: decliner\n~~~\n\nAlpha\n\n---\n\nBeta\n')
    const loadCodes = doc.warnings.map((d) => d.code)
    expect(loadCodes).toContain('plate::unsupported_construct')

    const result = await new Engine().render(quill, doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
    expect(result.warnings.slice(0, loadCodes.length)).toEqual(doc.warnings)
    expect(doc.warnings.map((d) => d.code)).toEqual(loadCodes)
  })

  // A counting descriptor loader for the lazy-load / coalescing invariants below.
  function countingEngine() {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/render/wasm.js')
          },
          formats: ['pdf', 'svg', 'png']
        }
      }
    })
    return { engine, loaded: () => loaded }
  }

  it('the manifest-backed format probe does NOT load the backend', async () => {
    const { engine, loaded } = countingEngine()
    const quill = makeRuntimeQuill()
    expect(await engine.supportedFormats(quill)).toContain('pdf')
    expect(loaded()).toBe(0)
    await engine.render(quill, Document.fromMarkdown(TEST_MARKDOWN), { format: 'svg' })
    expect(loaded()).toBe(1)
  })

  it('no backend manifest drifts from the loaded backend', async () => {
    const engine = new Engine()
    const mod = await import('../../../pkg/render/wasm.js')
    for (const quill of [makeRuntimeQuill(), Quill.fromTree(makeSampleFormQuill())]) {
      const manifestFormats = await engine.supportedFormats(quill)
      const backendQuill = mod.Quill.fromTree(quill.toTree())
      try {
        const realFormats = new mod.Quillmark().supportedFormats(backendQuill)
        expect([...manifestFormats].sort(), quill.backendId).toEqual([...realFormats].sort())
      } finally {
        backendQuill.free()
      }
    }
  })

  it('throws at construction for a malformed backend descriptor (names the id)', () => {
    // A backend entry must be a descriptor `{ load, formats }`; a bare thunk is rejected.
    expect(() => new Engine({ backends: { typst: () => import('../../../pkg/render/wasm.js') } })).toThrow(
      /typst/
    )
    // Missing/invalid manifest fields also fail fast at construction.
    expect(
      () => new Engine({ backends: { mybackend: { load: () => Promise.resolve({}) } } })
    ).toThrow(/mybackend/)
    expect(
      () =>
        new Engine({
          backends: { mybackend: { load: () => Promise.resolve({}), formats: 'pdf' } }
        })
    ).toThrow(/mybackend/)
  })

  // A loader that wraps the real backend module so `Quill.fromTree` calls are
  // counted (and still delegate to the real implementation). Used to prove the
  // per-Engine quill-clone cache materializes the backend quill once per
  // canonical instance instead of per render/open call.
  function fromTreeCountingEngine(options) {
    let fromTreeCalls = 0
    const engine = new Engine({
      ...options,
      backends: {
        typst: {
          load: async () => {
            const real = await import('../../../pkg/render/wasm.js')
            const wrappedQuill = new Proxy(real.Quill, {
              get(target, prop, receiver) {
                if (prop === 'fromTree') {
                  return (...args) => {
                    fromTreeCalls++
                    return target.fromTree(...args)
                  }
                }
                return Reflect.get(target, prop, receiver)
              }
            })
            return new Proxy(real, {
              get(target, prop, receiver) {
                if (prop === 'Quill') return wrappedQuill
                return Reflect.get(target, prop, receiver)
              }
            })
          },
          formats: ['pdf', 'svg', 'png']
        }
      }
    })
    return { engine, fromTreeCalls: () => fromTreeCalls }
  }

  it('caches the backend quill clone once per canonical instance', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quillA = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quillA, doc, { format: 'svg' })
    await engine.render(quillA, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(1)
    await engine.render(makeRuntimeQuill(), doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(2)
  })

  // GUARD for the class of bug where a method is declared in runtime.d.ts and
  // implemented in the backend build, but the hand-written canonical LiveSession
  // wrapper (runtime.js) forgets to forward it.
  // The type-level drift test (runtime.types.test-d.ts) only checks structural type
  // compatibility, so a wrapper that TYPE-checks but has no matching JS method
  // sails through it and throws `X is not a function` at runtime. This calls
  // EVERY documented LiveSession member on a live canonical session, so a
  // dropped delegation surfaces here instead of only in a consumer.
  it('canonical LiveSession forwards every documented method to the inner session', async () => {
    // paint() downcasts its argument to a 2D context via wasm-bindgen's
    // `instanceof` check, so it needs these globals present (Node has no DOM).
    class FakeImageData {
      constructor(data, width, height) {
        this.data = data
        this.width = width
        this.height = height
      }
    }
    class FakeCanvasRenderingContext2D {
      constructor() {
        this.calls = []
        this.canvas = { width: 0, height: 0 }
      }
      putImageData(img, dx, dy) {
        this.calls.push({ width: img.width, height: img.height, dx, dy })
      }
    }
    globalThis.ImageData = FakeImageData
    globalThis.CanvasRenderingContext2D = FakeCanvasRenderingContext2D

    // A SINGLE-LINE $body, deliberately. `fieldAt` hit-tests per-glyph ink
    // boxes, so the probe point below (the region rect's centre) must land on
    // ink: a one-line body's region rect IS that line's contiguous glyph
    // boxes, so its centre is ink by construction. TEST_MARKDOWN's
    // heading+paragraph body has an inter-line gap at the union rect's
    // centre, where fieldAt correctly answers undefined.
    const SMOKE_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Smoke Test
author: Smoke Author
~~~

A single line of body ink.`

    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(SMOKE_MARKDOWN)
    const session = await engine.open(quill, doc)
    try {
      // Getters.
      expect(session.pageCount).toBeGreaterThan(0)
      expect(session.backendId).toBe('typst')
      expect(Array.isArray(session.warnings)).toBe(true)

      // render.
      expect(typeof session.render).toBe('function')
      expect(session.render({ format: 'svg' }).artifacts.length).toBeGreaterThan(0)

      // regions: the body markdown content field auto-tags one region, keyed
      // by the canonical DocPath `main.body`.
      expect(typeof session.regions).toBe('function')
      const regions = session.regions()
      const body = regions.find((r) => r.field === 'main.body')
      expect(body).toBeDefined()

      // pageSize.
      const size = session.pageSize(body.page)
      expect(size.widthPt).toBeGreaterThan(0)
      expect(size.heightPt).toBeGreaterThan(0)

      // fieldAt: the centre of the body region's rect ([x0, y0, x1, y1],
      // bottom-left PDF points) is ink, and resolves to its DocPath; the page
      // corner is off any field's ink and answers undefined.
      expect(typeof session.fieldAt).toBe('function')
      const [x0, y0, x1, y1] = body.rect
      const hit = session.fieldAt(body.page, (x0 + x1) / 2, (y0 + y1) / 2)
      expect(hit).toBe('main.body')
      expect(session.fieldAt(body.page, 1, 1)).toBeUndefined()

      // fieldBoxes: the whole-field union helper. A single-line body has one
      // span-bearing segment, so its box unions to one rect covering that line.
      expect(typeof session.fieldBoxes).toBe('function')
      const boxes = session.fieldBoxes('main.body')
      expect(boxes.length).toBe(1)
      expect(boxes[0].field).toBe('main.body')
      expect(boxes[0].span).toBeDefined()
      // A field with no span-bearing region has no derived content box.
      expect(session.fieldBoxes('does_not_exist')).toEqual([])

      // positionAt: the fine-grained click direction, carrying the granularity
      // signal. A hit on the single line's ink is cluster-exact.
      expect(typeof session.positionAt).toBe('function')
      const chit = session.positionAt(body.page, (x0 + x1) / 2, (y0 + y1) / 2)
      expect(chit.field).toBe('main.body')
      expect(chit.granularity).toBe('cluster')

      // The tolerance is an argument, and a wrapper forwarding the method while
      // dropping an argument type-checks and silently answers as if the caller
      // never passed one. So it is asserted by its effect: a point clear of the
      // line's own ink misses, and the same point within a tolerance spanning
      // that clearance lands on the field it is nearest.
      const clear = (y1 - y0) * 2
      expect(session.positionAt(body.page, (x0 + x1) / 2, y1 + clear)).toBeUndefined()
      expect(session.positionAt(body.page, (x0 + x1) / 2, y1 + clear, clear * 2).field).toBe(
        'main.body'
      )
      expect(session.fieldAt(body.page, (x0 + x1) / 2, y1 + clear)).toBeUndefined()
      expect(session.fieldAt(body.page, (x0 + x1) / 2, y1 + clear, clear * 2)).toBe('main.body')

      // paint.
      expect(typeof session.paint).toBe('function')
      const ctx = new FakeCanvasRenderingContext2D()
      session.paint(ctx, body.page, 1)
      expect(ctx.canvas.width).toBeGreaterThan(0)

      // update: recompile in place.
      expect(typeof session.update).toBe('function')
      const cs = session.update(Document.fromMarkdown(SMOKE_MARKDOWN))
      expect(Array.isArray(cs.dirtyPages)).toBe(true)
    } finally {
      session.free()
    }
  })

  it('rejects an unregistered backend with engine::backend_not_found, probes like renders', async () => {
    const engine = new Engine()
    // A quill whose declared backend has no loader.
    const yaml = `quill:
  name: mystery
  version: "1.0.0"
  backend: doesnotexist
  description: no backend registered
main:
  fields:
    title:
      type: string
      example: x
`
    const quill = Quill.fromTree(new Map([['Quill.yaml', new TextEncoder().encode(yaml)]]))
    const doc = quill.seedDocument()
    for (const [call, verb] of [
      [() => engine.render(quill, doc), 'engine.render'],
      [() => engine.supportedFormats(quill), 'engine.supportedFormats'],
    ]) {
      const caught = await call().then(
        () => expect.unreachable(`${verb} resolved against an unregistered backend`),
        (e) => e
      )
      expect(isQuillmarkError(caught)).toBe(true)
      expect(caught.diagnostics[0].code).toBe('engine::backend_not_found')
    }
  })

  it('does NOT load the backend for sync core work: only on first render (lazy)', async () => {
    const { engine, loaded } = countingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Sync core surface (schema / validate / seed) touches no backend.
    expect(quill.schema).toBeDefined()
    quill.validate(doc)
    quill.seedDocument().free?.()
    expect(loaded()).toBe(0)

    // First render triggers exactly one backend load.
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded()).toBe(1)
  })

  it('coalesces concurrent first renders into a single backend load', async () => {
    const { engine, loaded } = countingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await Promise.all([
      engine.render(quill, doc, { format: 'svg' }),
      engine.render(quill, doc, { format: 'svg' }),
      engine.render(quill, doc, { format: 'svg' })
    ])
    expect(loaded()).toBe(1)
  })

  it('caller may free() its handles as soon as render/open returns (pre-await snapshot)', async () => {
    // Both caller handles are snapshotted before the first await inside
    // render/open (the backend load: a real suspension point on first call),
    // so a synchronous free() right after the call cannot race the clone. Each
    // engine below is fresh, so its first call has the load pending when
    // free() runs.
    const renderEngine = new Engine()
    const renderQuill = makeRuntimeQuill()
    const renderDoc = Document.fromMarkdown(TEST_MARKDOWN)
    const pendingRender = renderEngine.render(renderQuill, renderDoc, { format: 'svg' })
    renderDoc.free()
    renderQuill.free()
    const result = await pendingRender
    expect(result.artifacts.length).toBeGreaterThan(0)

    const openEngine = new Engine()
    const openQuill = makeRuntimeQuill()
    const openDoc = Document.fromMarkdown(TEST_MARKDOWN)
    const pendingOpen = openEngine.open(openQuill, openDoc)
    openDoc.free()
    openQuill.free()
    const session = await pendingOpen
    try {
      expect(session.pageCount).toBeGreaterThan(0)
    } finally {
      session.free()
    }
  })

  it('propagates a clone-construction failure (doc clone), leaving the quill clone cached', async () => {
    // The quill clone is already materialized and cached, and stays cached;
    // only the per-call doc clone is freed in the `finally`. Cache and leak
    // state are not observable from JS, so the assertion is the error itself.
    //
    // The failure is injected through the backend REGISTRY, not through a
    // stand-in Document: both caller handles are checked before the clone runs
    // (see "handles from another copy" below), so they have to be real. Same
    // Proxy-over-the-real-module shape as fromTreeCountingEngine, so the quill
    // clone left cached is a real backend quill and only `fromStored` misbehaves.
    const engine = new Engine({
      backends: {
        typst: {
          load: async () => {
            const real = await import('../../../pkg/render/wasm.js')
            const refusingDocument = new Proxy(real.Document, {
              get(target, prop, receiver) {
                if (prop === 'fromStored') {
                  return () => {
                    throw new Error('doc clone refused')
                  }
                }
                return Reflect.get(target, prop, receiver)
              },
            })
            return new Proxy(real, {
              get(target, prop, receiver) {
                if (prop === 'Document') return refusingDocument
                return Reflect.get(target, prop, receiver)
              },
            })
          },
          formats: ['pdf', 'svg', 'png'],
        },
      },
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    await expect(engine.render(quill, doc)).rejects.toThrow('doc clone refused')
  })
})

// A duplicate install is two copies of this package: two core builds, two
// linear memories, two distinct `Quill`/`Document` classes. A handle never
// crosses between them. The writer and reader binds, `Engine` and
// `LiveSession.update` check and throw in contract, naming `npm ls`; the last
// two are the seams that cross as data and would otherwise silently work. A
// by-reference core method is left to wasm-bindgen's own `_assertClass`, which
// refuses it as a bare `Error`.
//
// A foreign handle is modelled two ways: a stand-in carrying the serializer a
// real handle has (the shape most likely to slip a check), and a second copy of
// the built core artifact on disk, which is a genuinely different class over a
// different linear memory.
describe('@quillmark/wasm: handles from another copy (duplicate install)', () => {
  const foreignDoc = (doc) => ({ toStored: () => doc.toStored() })
  const foreignQuill = (quill) => ({
    toTree: () => quill.toTree(),
    backendId: quill.backendId,
  })

  /** Every rejection is in contract, names the method, and names `npm ls`. */
  const assertForeign = (caught, method) => {
    // wasm-bindgen's bare `_assertClass` throw (`expected instance of Document`
    // at a value that IS a Document) fails every line below.
    expect(isQuillmarkError(caught)).toBe(true)
    expect(caught.diagnostics[0].code).toMatch(/^runtime::not_a_(quill|document)$/)
    expect(caught.message).toContain(method)
    expect(caught.diagnostics[0].hint).toMatch(/npm ls @quillmark\/wasm/)
    return caught
  }
  const expectForeign = (call, method) => assertForeign(caughtFrom(call), method)
  // Kept separate from the sync form rather than folded into one async helper:
  // a forgotten `await` on an async assertion passes silently.
  const expectForeignAsync = (promise, method) =>
    promise.then(
      () => expect.unreachable(`${method} resolved instead of refusing a foreign handle`),
      (e) => assertForeign(e, method)
    )

  it('rejects a non-handle argument at a bind, naming the method', () => {
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => quill.writer(null), 'runtime::not_a_document')
    expectEditCode(() => new DocumentWriter(null, doc), 'runtime::not_a_quill')
  })

  it('leaves a by-reference core method to wasm-bindgen, which refuses it too', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const other = Document.fromMarkdown(TEST_MARKDOWN)

    // Out of contract by design: `_assertClass` throws a bare Error here, so a
    // caller routing on diagnostics sees nothing. Cheap to state, and the line
    // that fails if a guard is ever put back without a contract to match.
    expect(isQuillmarkError(caughtFrom(() => doc.equals(foreignDoc(other))))).toBe(false)

    // A local handle still takes the generated path unchanged.
    expect(doc.equals(other)).toBe(true)
  })

  // Both handle positions on every bind, derived: an exported class whose bare
  // construction refuses for want of a `Quill` takes handles, so it is a bind.
  // `Engine` and `LiveSession` construct bare, so the filter drops them without
  // naming them. Cross-checked against the method name each bind reports, so a
  // fifth bind fails here until it is named.
  const BIND_METHOD = new Map([
    [DocumentWriter, 'quill.writer(doc)'],
    [CardWriter, 'writer.card(index)'],
    [DocumentReader, 'quill.reader(doc)'],
    [CardReader, 'reader.card(index)'],
  ])

  it('refuses a foreign handle in either position at every writer and reader bind', () => {
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const binds = Object.values(runtime).filter(
      (v) => isClass(v) && caughtFrom(() => new v())?.diagnostics?.[0]?.code === 'runtime::not_a_quill'
    )
    expect(new Set(binds)).toEqual(new Set(BIND_METHOD.keys()))
    for (const bind of binds) {
      const method = BIND_METHOD.get(bind)
      // The third argument is the card index, ignored by the two binds that
      // take only two.
      expectForeign(() => new bind(foreignQuill(quill), doc, 0), method)
      expectForeign(() => new bind(quill, foreignDoc(doc), 0), method)
    }
  })

  // The `Quill` factories in front of two of those binds; the loop above
  // constructs directly.
  it('refuses a foreign Document at the writer and reader entry points', () => {
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectForeign(() => quill.writer(foreignDoc(doc)), 'quill.writer(doc)')
    expectForeign(() => quill.reader(foreignDoc(doc)), 'quill.reader(doc)')
  })

  // Engine is the seam with no `_assertClass` to front-run: it crosses into
  // backend memory as data, so a foreign handle would render correctly and pay
  // a per-copy quill clone cache for it. It checks instead. Inside the class the
  // check is structural (`#backendOf` is the only route to `backendId`, and
  // `#withClones` checks the doc); these guard the boundary.
  //
  // The quill half is DERIVED: a fifth verb is held to the rule with no label to
  // get wrong, and a verb naming a `Quill` without reading it fails the same
  // line as one that forgot the check.
  it('holds every Engine verb to the quill-first rule (derived)', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const verbs = Object.getOwnPropertyNames(Engine.prototype).filter((n) => n !== 'constructor')
    expect(verbs.length).toBeGreaterThan(0)
    for (const verb of verbs) {
      // A getter takes no argument, so nothing gates it.
      expect(typeof Object.getOwnPropertyDescriptor(Engine.prototype, verb).value).toBe('function')
      // `await` normalizes the sync and promise-returning verbs.
      let caught
      try {
        await engine[verb](foreignQuill(quill), doc)
      } catch (e) {
        caught = e
      }
      expect(caught, `engine.${verb} accepted a foreign Quill`).toBeDefined()
      assertForeign(caught, `engine.${verb}`)
    }
  })

  // The document half stays hand-placed. Deriving it is unsound: the stand-in
  // doc carries `toStored`, so a verb skipping `requireLocalDoc` would succeed and
  // pass a derived assertion. Which verbs take a `Document` is guarded
  // structurally inside `#withClones` and named here at the boundary.
  it('refuses a foreign Document at every Engine entry point taking one', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    await expectForeignAsync(
      engine.render(quill, foreignDoc(doc)),
      'engine.render(quill, doc)'
    )
    await expectForeignAsync(engine.open(quill, foreignDoc(doc)), 'engine.open(quill, doc)')
  })

  it('refuses a foreign Document on session.update', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const session = await engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    try {
      const next = Document.fromMarkdown(TEST_MARKDOWN.replace('Hello World', 'Next'))
      expectForeign(() => session.update(foreignDoc(next)), 'session.update(doc)')
      // The session is untouched by the refusal and still applies a local doc.
      expect(session.update(next).pageCount).toBe(session.pageCount)
    } finally {
      session.free()
    }
  }, 120000)

  // The real shape: a SECOND COPY of the built core artifact on disk, which is
  // what npm produces. A query suffix is not enough, since `wasm.js?x`
  // re-evaluates but still imports the cached `./wasm_bg.js`, leaving the
  // classes identical. Copying the directory forks the module graph and the
  // linear memory.
  describe('against a second copy of the core build on disk', () => {
    let copyB
    let copyRoot
    beforeAll(async () => {
      copyRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'quillmark-core-copy-'))
      const dst = path.join(copyRoot, 'core')
      fs.cpSync(path.join(PKG_DIR, 'core'), dst, { recursive: true })
      copyB = await import(/* @vite-ignore */ path.join(dst, 'wasm.js'))
      // A second copy is a second instantiation: its own memory, its own
      // classes, and its own init.
      copyB.initSync({ module: fs.readFileSync(path.join(dst, 'wasm_bg.wasm')) })
    })
    afterAll(() => {
      if (copyRoot) fs.rmSync(copyRoot, { recursive: true, force: true })
    })

    it('is genuinely a different class over a different memory', () => {
      expect(copyB.Document).not.toBe(Document)
      expect(copyB.Quill).not.toBe(Quill)
    })

    it('refuses copy B handles everywhere, in contract', async () => {
      const quillA = makeRuntimeQuill()
      const docA = Document.fromMarkdown(TEST_MARKDOWN)
      const docB = copyB.Document.fromMarkdown(TEST_MARKDOWN)
      const quillB = copyB.Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))

      expectForeign(() => quillA.writer(docB), 'quill.writer(doc)')
      expectForeign(() => quillA.reader(docB), 'quill.reader(doc)')
      expectForeign(() => new DocumentWriter(quillB, docA), 'quill.writer(doc)')

      const engine = new Engine()
      await expectForeignAsync(engine.render(quillA, docB), 'engine.render(quill, doc)')
      await expectForeignAsync(
        engine.supportedFormats(quillB),
        'engine.supportedFormats(quill)'
      )

      // Nothing we did freed or mutated the caller's handles.
      expect(docB.quillRef).toBe('test_quill')
      expect(quillB.backendId).toBe('typst')
    })

    it('reads copy B handles fine through copy B, which is the point', () => {
      // The rule is about CROSSING, not about copy B being defective. Copy B's
      // own classes work together; only mixing them throws.
      const docB = copyB.Document.fromMarkdown(TEST_MARKDOWN)
      const quillB = copyB.Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
      expect(quillB.validate(docB)).toBeDefined()
    })
  })
})


describe('@quillmark/wasm: today (the render date a host supplies)', () => {
  const QUILL_YAML = `quill:
  name: dated
  version: "1.0"
  backend: typst
  description: A field dated by the day it renders

main:
  fields:
    issued: { type: date, default: today }
`
  const PLATE = `#import "@local/quillmark-helper:0.1.0": data
#assert.eq(data.issued, datetime.today())
#assert.eq(data.issued, datetime(year: 2026, month: 3, day: 14))`

  const quill = () => Quill.fromTree(makeQuill({ name: 'dated', plate: PLATE, quillYaml: QUILL_YAML }))
  const doc = () => Document.fromMarkdown('~~~card-yaml\n$quill: dated\n$kind: main\n~~~\n')
  const issued = (resolved) => resolved.main.fields.find((r) => r.name === 'issued')

  it('resolves `today` as the given date, else the local one', () => {
    expect(issued(quill().reader(doc()).resolve('2026-03-14'))).toMatchObject({
      value: '2026-03-14',
      source: 'default',
    })

    const now = new Date()
    const local = [now.getFullYear(), now.getMonth() + 1, now.getDate()]
      .map((n, i) => String(n).padStart(i === 0 ? 4 : 2, '0'))
      .join('-')
    expect(issued(quill().reader(doc()).resolve()).value).toBe(local)
  })

  it('renders the field and the plate on the same date, and refuses a non-date', async () => {
    const engine = new Engine()
    const result = await engine.render(quill(), doc(), { format: 'svg' }, '2026-03-14')
    expect(result.outputFormat).toBe('svg')

    const session = await engine.open(quill(), doc(), '2026-03-14')
    session.update(doc())
    session.free()

    await expect(engine.open(quill(), doc(), 'today')).rejects.toThrow('YYYY-MM-DD')
  })
})
