/**
 * The canonical `@quillmark/wasm/runtime` API end to end: a CORE quill and
 * document handed to `Engine` render correctly, the engine cloning them into the
 * Typst backend's memory on demand without the caller ever seeing a backend
 * handle.
 */
import { describe, it, expect, beforeAll } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'
import {
  Engine,
  DocumentWriter,
  CardWriter,
  DocumentReader,
  CardReader,
  MAIN_CARD_ADDR,
  isQuillmarkError,
  isUnknownLine,
  isUnknownContainer,
  isUnknownMark,
  isUnknownIsland,
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
  SAMPLE_FORM_MARKDOWN,
  expectEditCode,
  isClass,
  caughtFrom,
} from './test-helpers.js'

// The consumer contract, exercised as a consumer writes it: the gate is the only
// door to the core surface. This also instantiates the core build the `CoreQuill`
// identity pin below imports directly (same resolved file, same module
// instance).
const { Quill, Document, importMarkdown, exportMarkdown, parseDocPath } = await init()

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

describe('@quillmark/wasm/runtime: surface', () => {
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

  it('parses a Document via the re-exported core class', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.quillRef).toBe('test_quill')
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
// `quill.writer(doc)`. Every verb is a one-line delegation to the underscored
// `_commit*` / `_reviseField` ABI on the raw `Document` class (hidden from the
// `.d.ts`), whose error-code matrix is exercised at that altitude in
// `basic.test.js`. What is the sugar's own, and what these pin: each verb
// forwards to the right ABI call with the right address, and errors propagate
// rather than being swallowed.
describe('@quillmark/wasm/runtime: DocumentWriter / CardWriter (bind the quill once)', () => {
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

  it('an ABI error propagates through the sugar rather than being swallowed', () => {
    const ed = buildQuill().writer(blankDoc())
    expectEditCode(() => ed.set('stray', 'x'), 'edit::unknown_field')
    expect(fieldOf(ed.document.main, 'stray')).toBeUndefined()
  })

  it('setValues writes the document, an unedited read is a no-op, and undefined is absent', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    ed.setValues({ fields: { subject: 'Q3 **results**', qty: '5' }, ext: { app: { k: 1 } } })
    expect(fieldOf(ed.document.main, 'qty')).toBe(5)

    const before = ed.document.toStored()
    ed.setValues(quill.reader(ed.document).values())
    expect(ed.document.toStored()).toBe(before)

    // `undefined` is absent (untouched) where the wasm boundary alone would
    // fold it to `null` (a removal); an unnamed declared field is removed.
    ed.setValues({ fields: { subject: 'Q4', qty: undefined }, ext: undefined, cards: undefined })
    const v = quill.reader(ed.document).values()
    expect(v.fields).toEqual({ subject: 'Q4' })
    expect(v.ext).toEqual({ app: { k: 1 } })
    ed.setValues({ ext: null })
    expect(quill.reader(ed.document).values().ext).toBeNull()
  })

  it('reader.resolve is the render view beside reader.values', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    quill.writer(doc).setValues({ fields: { subject: 'Hi' } })
    const reader = quill.reader(doc)
    expect(reader.values().fields).toEqual({ subject: 'Hi' })
    const resolved = reader.resolve()
    expect(resolved.main.fields.find((f) => f.name === 'subject')).toMatchObject({
      source: 'authored',
    })
    expect(resolved.main.fields.find((f) => f.name === 'qty').source).not.toBe('authored')
  })

  it('reviseBody writes the main body from markdown and returns a Delta', () => {
    const ed = buildQuill().writer(blankDoc())
    const delta = ed.reviseBody('New **body**.')
    expect(ed.document.bodyMarkdown()).toBe('New **body**.')
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('reviseField writes a richtext field typed, and returns a Delta', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    const delta = ed.reviseField('subject', 'Q3 **results**')
    expect(quill.reader(ed.document).get('subject')).toBe('Q3 **results**')
    // The anchor-preserving receipt is a structured-clone-able change set.
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('addCard fuses make + typed commit + push', () => {
    const ed = buildQuill().writer(blankDoc())
    // `body` here is the card's richtext FIELD; the third arg is the card body.
    ed.addCard('note', { body: 'Field **body**.' }, 'Card body text.')
    expect(ed.document.cards).toHaveLength(1)
    expect(ed.document.cards[0].kind).toBe('note')
    expect(exportMarkdown(fieldOf(ed.document.cards[0], 'body'))).toBe('Field **body**.')
    expect(exportMarkdown(ed.document.cards[0].body)).toBe('Card body text.')
  })

  it('removeCard drops the card and returns it', () => {
    const ed = buildQuill().writer(blankDoc())
    ed.addCard('note', { body: 'x' })
    const removed = ed.removeCard(0)
    expect(removed.kind).toBe('note')
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

  it('getStored reads raw values quill-free; bodyMarkdown is body-only (field half retired)', () => {
    const quill = buildQuill()
    const ed = quill.writer(blankDoc())
    ed.set('qty', '3')
    ed.set('subject', 'Q3 **results**')
    ed.reviseBody('Main **body**.')
    // Transport reads stay quill-free on the Document.
    expect(ed.document.getStored('qty')).toBe(3)
    expect(ed.document.getStored('missing')).toBeUndefined()
    // bodyMarkdown is the body read; a field address throws: a field's markdown
    // reads through the schema-plane view.
    expect(ed.document.bodyMarkdown()).toBe('Main **body**.')
    expect(() => ed.document.bodyMarkdown({ field: 'subject' })).toThrow(/body-only/)
    expect(quill.reader(ed.document).get('subject')).toBe('Q3 **results**')
    // reader.get carries schema authority: an unknown name throws (vs `undefined`
    // from the quill-free transport `Document.getStored` above).
    expectEditCode(() => quill.reader(ed.document).get('missing'), 'edit::unknown_field')
  })
})

// The typed-reader sugar is the read twin of the writer above: bind the quill
// once and read each field by its declared type (a richtext field to markdown,
// every other type verbatim) with schema authority, so an unknown field name
// throws rather than reading back `undefined` off the quill-free `Document`.
describe('@quillmark/wasm/runtime: DocumentReader / CardReader (the schema-plane read)', () => {
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

  it('a richtext field holding a scalar throws FieldDecode', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('subject', 3) // opaque write puts a bare number under richtext
    expectEditCode(() => quill.reader(doc).get('subject'), 'edit::field_decode')
  })

  it('an absent field addr reads the body markdown, quill-free', () => {
    const quill = buildQuill()
    const v = quill.reader(seededDoc(quill))
    expect(v.bodyMarkdown()).toBe('Main **body**.')
    expect(v.get({})).toBe('Main **body**.') // {} = main body, equals bodyMarkdown()
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

  // getContent is the same read at the other end of the codec: the `Content`, not
  // the projection. Rest is per-codec now, so what it spans is a document at
  // rest versus one the transport door left as authored.
  it('getContent returns the Content for a conformed field and an authored one', () => {
    const quill = buildQuill()
    // At rest: the writer (like the bound door) stores the canonical Content.
    const committed = seededDoc(quill)
    expect(typeof committed.getStored('subject')).toBe('object')
    // Transport door: a markdown-authored field rests as authored until it is
    // conformed.
    const parsed = Document.fromMarkdown(
      '~~~card-yaml\n$quill: view_test\nsubject: Q3 **results**\n~~~\n\nBody.'
    )
    expect(typeof parsed.getStored('subject')).toBe('string')

    const a = quill.reader(committed).getContent('subject')
    const b = quill.reader(parsed).getContent('subject')
    expect(a.text).toBe('Q3 results')
    expect(b.text).toBe(a.text)
    expect(b.marks).toEqual(a.marks)
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
    const quill = buildQuill()
    const doc = quill.parse('~~~card-yaml\n$quill: view_test\nsubject: 42\n~~~\n\nBody.')
    expect(doc.getStored('subject')).toBe(42) // no silent retype
    expect(doc.warnings.map((d) => d.code)).toContain('conform::field_decode')
    expect(doc.warnings[0].severity).toBe('warning')
  })

  it('nothing conforms under the wrong quill', () => {
    const quill = buildQuill()
    const md = '~~~card-yaml\n$quill: other_quill\nsubject: hi\n~~~\n\nBody.'
    let caught
    try {
      quill.parse(md)
    } catch (e) {
      caught = e
    }
    expect(isQuillmarkError(caught)).toBe(true)
    expect(caught.diagnostics[0].code).toBe('quill::name_mismatch')

    // The transport door still opens it, and conform reports the same mismatch
    // without touching the document.
    const doc = Document.fromMarkdown(md)
    const before = doc.toStored()
    expectEditCode(() => quill.conform(doc), 'quill::name_mismatch')
    expect(doc.toStored()).toBe(before)
  })

  it('getContent decodes by declared type: markdown for richtext, literal for plaintext', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      "~~~card-yaml\n$quill: view_test\nsubject: 'a *literal* line'\nnote: 'a *literal* line'\n~~~\n\nBody."
    )
    const v = quill.reader(doc)
    // Same stored bytes, two codecs: only the declared type says which.
    expect(v.getContent('subject').text).toBe('a literal line')
    expect(v.getContent('note').text).toBe('a *literal* line')
  })

  it('getContent: absence, unknown name, non-content type, body addr, cards', () => {
    const quill = buildQuill()
    const doc = seededDoc(quill)
    const v = quill.reader(doc)
    expectEditCode(() => v.getContent('nope'), 'edit::unknown_field')
    expectEditCode(() => v.getContent('qty'), 'edit::field_not_content')
    expect(v.getContent({}).text).toBe('Main body.') // absent field = body Content
    expect(v.card(0).getContent('body').text).toBe('A card field.')
    expectEditCode(() => v.card(9).getContent('body'), 'edit::index_out_of_range')

    const empty = quill.reader(
      Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    )
    expect(empty.getContent('subject')).toBeUndefined()
  })

  // A read is also a write input, so what it hands back has to be legal to hand
  // straight back in. `instance` is the field a writer owes, and storage omits a
  // zero; every `Content`-typed lane spells it.
  it('a Content read spells every container instance', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      "~~~card-yaml\n$quill: view_test\nparagraphs: ['> a']\n~~~\n\n> a\n\n- b"
    )
    const v = quill.reader(doc)
    const spelled = (rt) => rt.lines.flatMap((l) => l.containers).map((c) => c.instance)
    expect(spelled(v.getContent({}))).toEqual([0, 0])
    expect(spelled(v.getContentAt('paragraphs', [0]))).toEqual([0])
  })

  // getContentAt is that read one axis in: an element's codec is a schema fact,
  // so naming the element is what keeps the judgement out of the consumer.
  it('getContentAt reads an element the same from either resting form', () => {
    const quill = buildQuill()
    const parsed = Document.fromMarkdown(
      "~~~card-yaml\n$quill: view_test\nrecipients: ['a *literal* line']\nparagraphs: ['Q3 **results**']\n~~~\n\nBody."
    )
    expect(typeof parsed.getStored('paragraphs')[0]).toBe('string')
    const bound = quill.parse(
      "~~~card-yaml\n$quill: view_test\nrecipients: ['a *literal* line']\nparagraphs: ['Q3 **results**']\n~~~\n\nBody."
    )
    expect(bound.getStored('recipients')[0]).toBe('a *literal* line')
    expect(typeof bound.getStored('paragraphs')[0]).toBe('object')

    for (const [f, text] of [
      ['recipients', 'a *literal* line'],
      ['paragraphs', 'Q3 results'],
    ]) {
      const a = quill.reader(parsed).getContentAt(f, [0])
      const b = quill.reader(bound).getContentAt(f, [0])
      expect(a.text).toBe(text) // decoded at the element's declared codec
      expect(b.text).toBe(a.text)
      expect(b.marks).toEqual(a.marks)
    }
  })

  it('getContentAt reaches an object property and a leaf under both', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('letterhead', { motto: 'Fly **fight**', code: '9' })
    doc.storeField('rows', [{}, { notes: 'a *note*' }])
    const v = quill.reader(doc)
    expect(v.getContentAt('letterhead', ['motto']).text).toBe('Fly fight')
    expect(v.getContentAt('rows', [1, 'notes']).text).toBe('a note')
    expect(v.getContentAt('rows', [0, 'notes'])).toBeUndefined() // declared, unstored
    expect(v.getContentAt('subject', [])).toBeUndefined() // empty path IS getContent
  })

  it('getContentAt: stale index, no content leaf, undeclared name, cards', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('recipients', ['a'])
    doc.storeField('tags', ['x'])
    const v = quill.reader(doc)
    expect(v.getContentAt('recipients', [7])).toBeUndefined()
    expect(v.getContentAt('paragraphs', [0])).toBeUndefined() // field absent
    expectEditCode(() => v.getContentAt('tags', [0]), 'edit::field_not_content')
    expectEditCode(() => v.getContentAt('qty', [0]), 'edit::field_not_content')
    expectEditCode(() => v.getContentAt('letterhead', ['nope']), 'edit::unknown_field')
    expectEditCode(() => v.getContentAt('nope', [0]), 'edit::unknown_field')
    expectEditCode(() => v.card(9).getContentAt('lines', [0]), 'edit::index_out_of_range')
    expect(() => v.getContentAt('recipients', [null])).toThrow(/path\[0\]/)
    expect(() => v.getContentAt('recipients', 0)).toThrow(/`path` must be an array/)
    expect(() => v.getContentAt({}, [0])).toThrow(/body address/)
  })

  it('an undecodable element anchors its diagnostic at the element', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: view_test\n~~~\n\nBody.')
    doc.storeField('paragraphs', ['ok', 3])
    let caught
    try {
      quill.reader(doc).getContentAt('paragraphs', [1])
    } catch (e) {
      caught = e
    }
    const diag = caught.diagnostics[0]
    expect(diag.code).toBe('edit::field_decode')
    expect(diag.path).toBe('main.paragraphs[1]')
    expect(diag.args.field).toBe('paragraphs')
    expect(parseDocPath(diag.path)).toEqual([
      { seg: 'main' },
      { seg: 'field', name: 'paragraphs' },
      { seg: 'index', index: 1 },
    ])
  })

  it('card(i).getContentAt reads an element through the $kind schema', () => {
    const quill = buildQuill()
    const doc = seededDoc(quill)
    doc.storeField({ card: 0, field: 'lines' }, ['a *b*'])
    const v = quill.reader(doc)
    expect(v.card(0).getContentAt('lines', [0]).text).toBe('a *b*') // literal codec
    expect(v.card(0).getContentAt('lines', [4])).toBeUndefined()
  })
})

// MAIN_CARD_ADDR names the empty main-card address `{}` the card-scoped verbs
// take, so a main-card batch write reads as intent (`storeFields(MAIN_CARD_ADDR,
// fields)`) rather than as an anonymous `{}`. It IS `{}` (frozen), so it is a
// pure alias: `{}` and `undefined` stay equally valid.
describe('@quillmark/wasm/runtime: MAIN_CARD_ADDR (the named main-card address)', () => {
  it('is a frozen, empty card address: {} with a name', () => {
    expect(MAIN_CARD_ADDR).toEqual({})
    expect(Object.isFrozen(MAIN_CARD_ADDR)).toBe(true)
  })

  it('targets the main card on a card-scoped verb, identically to {}', () => {
    const named = new Document('editor_test')
    named.storeFields(MAIN_CARD_ADDR, { title: 'Hello', qty: 3 })
    expect(fieldOf(named.main, 'title')).toBe('Hello')
    expect(fieldOf(named.main, 'qty')).toBe(3)

    // Same effect as the bare empty-address spelling: a pure alias.
    const empty = new Document('editor_test')
    empty.storeFields({}, { title: 'Hello', qty: 3 })
    expect(named.main.payloadItems).toEqual(empty.main.payloadItems)
  })

  it('carries $ext onto the main card too', () => {
    const doc = new Document('editor_test')
    doc.storeExt(MAIN_CARD_ADDR, { editor: { pinned: true } })
    expect(doc.main.ext.editor.pinned).toBe(true)
  })
})

describe('@quillmark/wasm/runtime: open-set membership guards', () => {
  // One known name per axis, not the whole table: membership is a `Set.has`,
  // uniform across members, and the tables themselves are pinned against the
  // Rust constants by `crates/bindings/wasm/tests/known_names_drift.rs`.
  it('answers known-vs-unknown on all four axes', () => {
    expect(isUnknownLine({ kind: 'heading', attrs: { level: 2 }, containers: [] })).toBe(false)
    expect(isUnknownLine({ kind: 'callout', attrs: {}, containers: [] })).toBe(true)

    expect(isUnknownContainer({ container: 'quote' })).toBe(false)
    expect(isUnknownContainer({ container: 'indent', attrs: {} })).toBe(true)

    expect(isUnknownMark({ start: 0, end: 1, type: 'strong' })).toBe(false)
    expect(isUnknownMark({ start: 0, end: 1, type: 'highlight', attrs: {} })).toBe(true)

    expect(isUnknownIsland({ id: 'i1', type: 'table', props: {}, loss: 'lossless' })).toBe(false)
    expect(isUnknownIsland({ id: 'i1', type: 'widget', props: {}, loss: 'lossless' })).toBe(true)
  })

  it('reports a missing or non-string discriminant as not-unknown, never throwing', () => {
    // A malformed value is not an unknown construct: it is malformed, and the
    // decoder rejects it. The guard must not turn one into the other.
    for (const bad of [{}, { kind: 7 }, null, undefined]) {
      expect(isUnknownLine(bad)).toBe(false)
    }
  })
})

describe('@quillmark/wasm/runtime: container run boundaries', () => {
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

  // A stamped `1` is the helper saying these two would weld.
  const stamp = (a, b) => assignInstances([a, b]).map((c) => c.instance)

  it('stamps on what the markdown projection can carry, not on equality', () => {
    // `start` differs and they still weld: CommonMark reads only a list's first
    // number, so the projection cannot carry the second one.
    const list = (over) => ({ ...LIST, attrs: { ...LIST.attrs, ...over } })
    expect(stamp(LIST, list({ start: 3 }))).toEqual([0, 1])
    expect(stamp(LIST, list({ ordinal: 4 }))).toEqual([0, 1])
    expect(stamp(QUOTE, QUOTE)).toEqual([0, 1])
    // A shape the projection can tell apart needs no discriminator.
    expect(stamp(LIST, list({ ordered: true }))).toEqual([0, 0])

    // An unknown container's boundary lives in storage, and its whole `attrs`
    // is its shape.
    const indent = (n) => ({ container: 'indent', attrs: { n } })
    expect(stamp(indent(1), indent(1))).toEqual([0, 1])
    expect(stamp(indent(1), indent(2))).toEqual([0, 0])
  })

  it('alternates only across runs that would weld', () => {
    expect(assignInstances([LIST, LIST, LIST]).map((c) => c.instance)).toEqual([0, 1, 0])
    expect(assignInstances([LIST, QUOTE, LIST]).map((c) => c.instance)).toEqual([0, 0, 0])
    // A block carrying no container at this depth parts the runs on its own.
    expect(assignInstances([LIST, null, LIST]).map((c) => c && c.instance)).toEqual([0, null, 0])
    expect(assignInstances([]).length).toBe(0)
  })

  it('is what keeps a flattened tree from welding two lists into one item', () => {
    const [a, b] = assignInstances([LIST, LIST])
    const stamped = importMarkdown(exportMarkdown(content(a, b)))
    expect(stamped.lines.map((l) => l.containers[0].instance)).toEqual([0, 1])
    expect(stamped.lines.map((l) => l.containers[0].attrs.ordinal)).toEqual([0, 0])

    // The same paths without the discriminator: one item spanning two
    // paragraphs, the second marker gone, and no error anywhere.
    const welded = importMarkdown(exportMarkdown(content(LIST, LIST)))
    expect(welded.lines.map((l) => l.containers[0].instance)).toEqual([0, 0])
    expect(welded.lines.map((l) => l.containers[0].attrs.ordinal)).toEqual([0, 0])
  })
})

describe('@quillmark/wasm/runtime: Engine (hidden core→backend crossing)', () => {
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

  it('renders to SVG and reports supported formats / canvas capability', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const svg = await engine.render(quill, doc, { format: 'svg' })
    expect(svg.outputFormat).toBe('svg')

    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('svg')
    expect(typeof (await engine.supportsCanvas(quill))).toBe('boolean')
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

  it('manifest-backed capability probes do NOT load the backend', async () => {
    // A descriptor-form counting loader: it carries the same manifest the
    // default registry uses, so probes answer from the manifest (no load),
    // while still counting any real binary load triggered by render.
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Descriptor WITH a manifest → probes answer from the manifest, no load.
    const formats = await engine.supportedFormats(quill)
    expect(formats).toContain('pdf')
    expect(typeof (await engine.supportsCanvas(quill))).toBe('boolean')
    expect(loaded).toBe(0)

    // A real render still triggers exactly one load.
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded).toBe(1)
  })

  it('manifest formats cannot drift from the loaded backend (drift guard)', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // What the static manifest reports (no load).
    const manifestFormats = await engine.supportedFormats(quill)
    const manifestCanvas = await engine.supportsCanvas(quill)

    // Force the backend to actually load, then ask the real engine directly.
    await engine.render(quill, doc, { format: 'svg' })
    const mod = await import('../../../pkg/backends/typst/wasm.js')
    const backendEngine = new mod.Quillmark()
    const backendQuill = mod.Quill.fromTree(quill.toTree())
    try {
      const realFormats = backendEngine.supportedFormats(backendQuill)
      const realCanvas = backendEngine.supportsCanvas(backendQuill)
      // The manifest must match what the binary reports, both directions.
      expect([...manifestFormats].sort()).toEqual([...realFormats].sort())
      expect(manifestCanvas).toBe(realCanvas)
    } finally {
      backendQuill.free()
    }
  })

  it('pdfform manifest cannot drift from the loaded backend (drift guard)', async () => {
    // Same drift guard as typst, but for the pdfform backend: the static
    // `{ formats, canvas }` manifest in DEFAULT_BACKENDS must match what the
    // loaded pdfform binary actually reports.
    const engine = new Engine()
    const quill = Quill.fromTree(makeSampleFormQuill())
    expect(quill.backendId).toBe('pdfform')
    const doc = Document.fromMarkdown(SAMPLE_FORM_MARKDOWN)

    // What the static manifest reports (no load).
    const manifestFormats = await engine.supportedFormats(quill)
    const manifestCanvas = await engine.supportsCanvas(quill)
    expect([...manifestFormats].sort()).toEqual(['pdf', 'png', 'svg'])
    expect(manifestCanvas).toBe(true)

    // Force the pdfform backend to load, then ask the real engine directly.
    await engine.render(quill, doc, { format: 'pdf' })
    const mod = await import('../../../pkg/backends/pdfform/wasm.js')
    const backendEngine = new mod.Quillmark()
    const backendQuill = mod.Quill.fromTree(quill.toTree())
    try {
      const realFormats = backendEngine.supportedFormats(backendQuill)
      const realCanvas = backendEngine.supportsCanvas(backendQuill)
      expect([...manifestFormats].sort()).toEqual([...realFormats].sort())
      expect(manifestCanvas).toBe(realCanvas)
    } finally {
      backendQuill.free()
    }
  })

  it('throws at construction for a malformed backend descriptor (names the id)', () => {
    // A backend entry must be a descriptor `{ load, formats, canvas }`; a bare thunk is rejected.
    expect(() => new Engine({ backends: { typst: () => import('../../../pkg/backends/typst/wasm.js') } })).toThrow(
      /typst/
    )
    // Missing/invalid manifest fields also fail fast at construction.
    expect(
      () => new Engine({ backends: { mybackend: { load: () => Promise.resolve({}), canvas: true } } })
    ).toThrow(/mybackend/)
    expect(
      () =>
        new Engine({
          backends: { mybackend: { load: () => Promise.resolve({}), formats: ['pdf'], canvas: 'yes' } }
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
            const real = await import('../../../pkg/backends/typst/wasm.js')
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
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    return { engine, fromTreeCalls: () => fromTreeCalls }
  }

  it('caches the backend quill clone: rendering twice materializes it once', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quill, doc, { format: 'svg' })
    await engine.render(quill, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(1)
  })

  it('caches per canonical instance: two different quills → two materializations', async () => {
    const { engine, fromTreeCalls } = fromTreeCountingEngine()
    const quillA = makeRuntimeQuill()
    const quillB = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    await engine.render(quillA, doc, { format: 'svg' })
    await engine.render(quillB, doc, { format: 'svg' })
    expect(fromTreeCalls()).toBe(2)
  })

  it('opens an iterative session, renders pages, and frees it', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = await engine.open(quill, doc)
    try {
      expect(session.pageCount).toBeGreaterThan(0)
      expect(session.backendId).toBe('typst')
      const page = session.render({ format: 'svg' })
      expect(page.artifacts.length).toBeGreaterThan(0)
    } finally {
      session.free()
    }
  })

  // GUARD for the class of bug where a method is declared in runtime.d.ts and
  // implemented in the backend build, but the hand-written canonical LiveSession
  // wrapper (runtime.js) forgets to forward it: `fieldAt` is the case in point.
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
      expect(typeof session.supportsCanvas).toBe('boolean')
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

      // fieldAt: the delegation that was missing. Hit-test the centre
      // of the body region's rect ([x0, y0, x1, y1], bottom-left PDF points)
      // (guaranteed ink for the single-line body (see SMOKE_MARKDOWN above))
      // and expect it to resolve back through the wrapper as its DocPath. Off
      // any field's ink (the page corner) the contract is undefined.
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
      const paintResult = session.paint(ctx, body.page)
      expect(paintResult.pixelWidth).toBeGreaterThan(0)

      // update: recompile in place.
      expect(typeof session.update).toBe('function')
      const cs = session.update(Document.fromMarkdown(SMOKE_MARKDOWN))
      expect(Array.isArray(cs.dirtyPages)).toBe(true)
    } finally {
      session.free()
    }
  })

  it('renders repeatedly from the same quill (clone-on-demand, no shared handle)', async () => {
    const engine = new Engine()
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const a = await engine.render(quill, doc, { format: 'svg' })
    const b = await engine.render(quill, doc, { format: 'svg' })
    expect(a.artifacts.length).toBeGreaterThan(0)
    expect(b.artifacts.length).toBeGreaterThan(0)
  })

  it('throws a clear error for an unregistered backend', async () => {
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
    await expect(engine.render(quill, doc)).rejects.toThrow(/no backend registered/)
  })

  it('accepts a custom backend descriptor override', async () => {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    await engine.render(quill, doc, { format: 'svg' })
    expect(loaded).toBe(1)
  })

  // A counting descriptor loader for the lazy-load / coalescing invariants below.
  function countingEngine() {
    let loaded = 0
    const engine = new Engine({
      backends: {
        typst: {
          load: () => {
            loaded++
            return import('../../../pkg/backends/typst/wasm.js')
          },
          formats: ['pdf', 'svg', 'png'],
          canvas: true
        }
      }
    })
    return { engine, loaded: () => loaded }
  }

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
    // so a synchronous free() right after the call cannot race the clone.
    // Regression pin for the "null pointer passed to rust" panic:
    // each engine below is fresh, so its first call has the load pending when
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
    // Exercises the teardown path when the doc clone (Document.fromStored) throws:
    // the quill clone is already materialized and cached (NOT freed here, that
    // is the T3 caching contract), only the per-call doc clone is freed in the
    // finally. We can only assert the error surfaces (cache/leak state is not
    // observable from JS), but this pins the throw path #withClones guards.
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
            const real = await import('../../../pkg/backends/typst/wasm.js')
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
          canvas: true,
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
// crosses between them; every seam taking one checks, and throws in contract
// naming the duplicate install and `npm ls`. See runtime.js § "Handles from
// another copy". These pin that the rule is UNIFORM: the by-reference core
// methods (which wasm-bindgen would reject anyway, as a bare `Error`), the
// writer and reader binds, `Engine`, and `LiveSession.update`. The last two are
// the seams that cross as data and would otherwise silently work.
//
// A foreign handle is modelled two ways: a stand-in carrying the serializer a
// real handle has (the shape most likely to slip a check), and a second copy of
// the built core artifact on disk, which is a genuinely different class over a
// different linear memory.
describe('@quillmark/wasm/runtime: handles from another copy (duplicate install)', () => {
  const foreignDoc = (doc) => ({ toStored: () => doc.toStored() })
  const foreignQuill = (quill) => ({
    toTree: () => quill.toTree(),
    backendId: quill.backendId,
  })

  /** Every rejection is in contract, codes `runtime::foreign_handle`, and names `npm ls`. */
  const assertForeign = (caught, method) => {
    // wasm-bindgen's bare `_assertClass` throw (`expected instance of Document`
    // at a value that IS a Document) fails every line below.
    expect(isQuillmarkError(caught)).toBe(true)
    expect(caught.diagnostics[0].code).toBe('runtime::foreign_handle')
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

  it('rejects a foreign Document on the by-reference core methods', () => {
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const other = Document.fromMarkdown(TEST_MARKDOWN)

    expectForeign(() => doc.equals(foreignDoc(other)), 'Document.equals')
    expectForeign(() => quill.validate(foreignDoc(doc)), 'Quill.validate')

    // A local handle still takes the generated path unchanged.
    expect(doc.equals(other)).toBe(true)
  })

  it('rejects a non-handle argument with a distinct code, naming the method', () => {
    // A different bug with a different cure: `npm ls` is the wrong advice for a
    // caller who passed null, so the two do not share a diagnostic.
    const quill = makeRuntimeQuill()
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const [call, method] of [
      [() => doc.equals(null), 'Document.equals'],
      [() => quill.validate({}), 'Quill.validate'],
    ]) {
      const caught = caughtFrom(call)
      expect(isQuillmarkError(caught)).toBe(true)
      expect(caught.message).toContain(method)
      expect(caught.diagnostics[0].code).toBe('runtime::not_a_document')
    }
    expectEditCode(() => quill.writer(null), 'runtime::not_a_document')
    expect(() => new DocumentWriter(null, doc)).toThrow(/expected a Quill/)
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
    beforeAll(async () => {
      const src = path.join(PKG_DIR, 'core')
      const dst = path.join(PKG_DIR, 'dup-core')
      fs.rmSync(dst, { recursive: true, force: true })
      fs.cpSync(src, dst, { recursive: true })
      copyB = await import(/* @vite-ignore */ path.join(dst, 'wasm.js'))
      // A second copy is a second instantiation: its own memory, its own
      // classes, and its own init.
      copyB.initSync({ module: fs.readFileSync(path.join(dst, 'wasm_bg.wasm')) })
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

      expectForeign(() => docA.equals(docB), 'Document.equals')
      expectForeign(() => quillA.validate(docB), 'Quill.validate')
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

  // Re-evaluating the runtime module against a cached, and so already patched,
  // core build must not wrap the wrappers: the Vite HMR / shared Vitest worker
  // case the `Symbol.for` marker exists for. A copy of runtime.js beside the
  // original re-evaluates while its relative `../core/wasm.js` import still
  // resolves to the cached module.
  it('patches once across a re-evaluation of the runtime module', async () => {
    const before = Document.prototype.equals
    expect(before[Symbol.for('@quillmark/wasm:handle-checked')]).toBe(true)

    // The twin has to sit BESIDE the original for its `../core/wasm.js` import
    // to resolve to the same cached module. `pkg/runtime/` is a published
    // directory, so remove it as soon as it is loaded rather than leave a stray
    // file that a publish from an unrebuilt pkg/ would ship.
    const twin = path.join(PKG_DIR, 'runtime', 'runtime.hmr.js')
    fs.copyFileSync(path.join(PKG_DIR, 'runtime', 'runtime.js'), twin)
    let reevaluated
    try {
      reevaluated = await import(/* @vite-ignore */ twin)
    } finally {
      fs.rmSync(twin, { force: true })
    }

    // The twin's gate has its own memo but draws from the same cached core
    // module, so it hands out that module's classes: instantiating is a no-op
    // and the identity holds across the re-evaluation.
    expect((await reevaluated.init()).Document).toBe(Document)
    expect(Document.prototype.equals).toBe(before)
    expect(Quill.prototype.validate[Symbol.for('@quillmark/wasm:handle-checked')]).toBe(true)
  })
})
