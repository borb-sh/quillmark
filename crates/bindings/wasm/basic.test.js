/**
 * The Document API over the canonical flow:
 * `Quill.fromTree(tree)` → `Document.fromMarkdown(markdown)` →
 * `engine.render(quill, doc, opts)`, against the Typst backend build.
 */

import { describe, it, expect } from 'vitest'
import {
  Quillmark,
  Quill,
  Document,
  importMarkdown,
  exportMarkdown,
  rebase,
  mapPos,
  mapMarks,
  isInline,
  isPlain,
  parseDocPath,
  formatDocPath,
  formatDiagnostic,
} from '@quillmark-wasm'
import * as renderBuild from '@quillmark-wasm'
import { makeQuill, makeCard, expectEditCode, initBuildSync } from './test-helpers.js'

initBuildSync(renderBuild, 'render')

/** Read a field value from a card's payloadItems list by key. */
const field = (card, key) =>
  card.payloadItems.find((i) => i.type === 'field' && i.key === key)?.value

/** True when a field key is absent from a card's payloadItems. */
const hasField = (card, key) =>
  card.payloadItems.some((i) => i.type === 'field' && i.key === key)

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is a test document.`

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let body = data.at("$body")

= #title

#body`

describe('Document.fromMarkdown', () => {
  it('reads quillRef, payload fields, body content, cards and warnings', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

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
    const md = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Global body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card body.
`
    const doc = Document.fromMarkdown(md)

    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(field(doc.cards[0], 'foo')).toBe('bar')
    expect(exportMarkdown(doc.cards[0].body)).toContain('Card body.')
  })

  it('attaches err.diagnostics as a non-empty array on thrown errors', () => {
    try {
      Document.fromMarkdown('')
      throw new Error('fromMarkdown should have thrown')
    } catch (err) {
      expect(err.diagnostics.length).toBeGreaterThanOrEqual(1)
      expect(err.diagnostics[0]).toHaveProperty('message')
      expect(err.diagnostics[0]).toHaveProperty('severity')
    }
  })
})

describe('Document.toMarkdown', () => {
  it('a mutated document survives emit → re-parse', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('title', 'New Title')
    doc.insertCard(makeCard('note', { author: 'Alice' }, 'Hello'))
    doc.revise({}, 'Updated body')

    const doc2 = Document.fromMarkdown(doc.toMarkdown())
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
  it('toStored emits a plain JSON string carrying the current schema version', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const dto = doc.toStored()
    expect(typeof dto).toBe('string')
    expect(JSON.parse(dto).schema).toBe(Document.currentStorageVersion())
  })

  it('round-trips a mutated document with cards back to an equal handle', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('title', 'New Title')
    doc.insertCard(makeCard('note', { author: 'Alice' }, 'Hello'))

    const restored = Document.fromStored(doc.toStored())

    expect(restored.equals(doc)).toBe(true)
    expect(field(restored.main, 'title')).toBe('New Title')
    expect(restored.cards[0].kind).toBe('note')
    expect(exportMarkdown(restored.cards[0].body)).toBe('Hello')
  })

  it('a row naming a construct outside the vocabulary does not open, but its tag still reads', () => {
    const dto = Document.fromMarkdown(TEST_MARKDOWN).toStored()
    const outside = dto.replace('"kind":"para"', '"kind":"callout"')
    expect(outside).not.toBe(dto)
    expect(() => Document.fromStored(outside)).toThrow()
    expect(Document.storageVersionOf(outside)).toBe(Document.currentStorageVersion())
  })

  it('storageVersionOf reads the schema tag off any payload, or undefined', () => {
    // A future version reads back as-is, though fromStored rejects it.
    const future = '{"schema":"quillmark/document@0.99.0","main":{}}'
    expect(() => Document.fromStored(future)).toThrow()
    expect(Document.storageVersionOf(future)).toBe('quillmark/document@0.99.0')

    expect(Document.storageVersionOf('{"foo":"bar"}')).toBeUndefined()
    expect(Document.storageVersionOf(TEST_MARKDOWN)).toBeUndefined()
  })
})

describe('Document authoring text', () => {
  it('each static carries core text through', () => {
    expect(Document.formatRules().length).toBeGreaterThan(0)
    expect(Document.quillRefHint().length).toBeGreaterThan(0)
    expect(Document.blueprintInstruction('usaf_memo')).toContain('usaf_memo')
  })
})

describe('Quillmark.quill', () => {
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
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const { opts, mimeType } of RENDER_FORMAT_CASES) {
      const result = opts === undefined ? engine.render(quill, doc) : engine.render(quill, doc, opts)
      expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
      expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
      expect(result.artifacts[0].mimeType).toBe(mimeType)
    }
  })

  // `RenderResult.warnings` is the document's parse warnings ahead of the
  // render's own, and a parse warning carries `args`: a value conform cannot
  // rest warns and renders. `args` is declared `Record<string, unknown>`, so it
  // must read as one on the far side.
  it('a merged parse warning carries its args as a plain object', () => {
    const NOTES_QUILL_YAML = `quill:
  name: notes
  version: "1.0"
  backend: typst
  description: A quill holding a list of richtext notes

main:
  fields:
    notes:
      type: array
      items:
        type: richtext
`
    const NOTES_PLATE = `#import "@local/quillmark-helper:0.1.0": data

#data.at("$body")`

    const engine = new Quillmark()
    const quill = Quill.fromTree(
      makeQuill({ name: 'notes', plate: NOTES_PLATE, quillYaml: NOTES_QUILL_YAML }),
    )
    const doc = quill.parse('~~~card-yaml\n$quill: notes\nnotes: [42]\n~~~\n\nAlpha\n')

    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)

    const w = result.warnings.find((d) => d.code === 'conform::field_decode')
    expect(w).toBeDefined()
    expect(w.args).not.toBeInstanceOf(Map)
    expect(w.args.field).toBe('notes')
  })

  it('session.regions() is always a non-null array, keyed by DocPath', () => {
    // The body is a markdown content field, so it auto-tags one region, at the
    // canonical DocPath `main.body`: the backend's plate-space `$body` is
    // translated at the session boundary.
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const session = engine.open(quill, doc)
    const regions = session.regions()
    expect(Array.isArray(regions)).toBe(true)
    expect(regions.some((r) => r.field === 'main.body')).toBe(true)
    // No plate-space ordinal grammar crosses the boundary.
    expect(regions.some((r) => r.field.startsWith('$cards.') || r.field === '$body')).toBe(false)
    session.free()
  })

  it('should throw a quill::name_mismatch error when the document quill ref differs from the quill name', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN.replace('test_quill', 'other_quill'))
    expectEditCode(() => engine.render(quill, doc, { format: 'pdf' }), 'quill::name_mismatch')
  })
})

describe('Document editor surface: storeField / removeField', () => {
  it('storeField writes a field and removeField returns it, undefined when absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('title', 'Updated')
    expect(field(doc.main, 'title')).toBe('Updated')
    expect(doc.removeField('title')).toBe('Updated')
    expect(hasField(doc.main, 'title')).toBe(false)
    expect(doc.removeField('title')).toBeUndefined()
  })

  it('both throw edit::invalid_field_name on an invalid name', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => doc.storeField('$body', 'x'), 'edit::invalid_field_name')
    expectEditCode(() => doc.storeField('bad-name', 'x'), 'edit::invalid_field_name')
    expectEditCode(() => doc.removeField('$quill'), 'edit::invalid_field_name')
  })
})

describe('Document blank-canvas constructor', () => {
  it('new Document(quillRef) starts blank and builds up', () => {
    const doc = new Document('test_quill')
    expect(doc.quillRef).toBe('test_quill')
    expect(doc.cards.length).toBe(0)
    expect(exportMarkdown(doc.main.body)).toBe('')
    doc.storeFields({}, { title: 'Hello' })
    expect(field(doc.main, 'title')).toBe('Hello')
  })

  it('throws on an invalid quill reference, coded as setQuillRef codes it', () => {
    expectEditCode(
      () => new Document('not a valid ref!!'),
      'parse::invalid_quill_reference',
    )
  })
})

describe('Document editor surface: storeFields', () => {
  it('storeFields applies every entry, in object order', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeFields({}, { subtitle: 'A subtitle', pages: 3 })
    expect(field(doc.main, 'subtitle')).toBe('A subtitle')
    expect(field(doc.main, 'pages')).toBe(3)
  })

  it('a failed batch throws one diagnostic per bad field and applies nothing', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    try {
      doc.storeFields({}, { ok_field: 'v', 'bad-name': 'v', 'also bad': 'v' })
      throw new Error('storeFields should have thrown')
    } catch (err) {
      expect(err.diagnostics.map((d) => d.path)).toEqual(['main.bad-name', 'main.also bad'])
      expect(err.diagnostics.map((d) => d.code)).toEqual([
        'edit::invalid_field_name',
        'edit::invalid_field_name',
      ])
    }
    expect(hasField(doc.main, 'ok_field')).toBe(false)
  })

  it('storeFields rejects a non-object argument', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.storeFields({}, 'not an object')).toThrow(/plain object/)
  })

  it('storeFields({ card }) is the card-indexed twin of storeFields', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard(makeCard('note', { foo: 'bar' }))
    doc.storeFields({ card: 0 }, { foo: 'baz', extra: 1 })
    expect(field(doc.cards[0], 'foo')).toBe('baz')
    expect(field(doc.cards[0], 'extra')).toBe(1)
    expectEditCode(() => doc.storeFields({ card: 99 }, { foo: 'v' }), 'edit::index_out_of_range')
  })

  it('an address with an unknown key throws instead of parsing as {}', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    // `Addr::from_js` rejects a stray key: a typo, or the fields object misread
    // as an address, is caught loud rather than silently parsed as the empty
    // main-card address.
    expect(() => doc.storeFields({ crad: 0 }, { title: 'x' })).toThrow(/unknown key/)
    // The swapped-arg failure this guards: fields handed where the address
    // belongs. Their keys are unknown to `Addr`, so the write throws instead of
    // parsing as `{}` and writing an empty batch to main.
    expect(() => doc.storeFields({ title: 'x' })).toThrow(/unknown key/)
    expect(field(doc.main, 'title')).not.toBe('x')
  })
})

describe('Document editor surface: setQuillRef / overwrite / revise', () => {
  it('setQuillRef changes the quillRef and refuses an invalid one', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setQuillRef('new_quill')
    expect(doc.quillRef).toBe('new_quill')
    expectEditCode(
      () => doc.setQuillRef('INVALID QUILL REF WITH SPACES'),
      'parse::invalid_quill_reference',
    )
  })

  it('revise({}, md) revises the main body and returns the text delta', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const delta = doc.revise({}, 'Body from **markdown**.')
    expect(exportMarkdown(doc.main.body)).toBe('Body from **markdown**.')
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('overwrite({}, rt) writes a content object', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.overwrite({}, importMarkdown('Content **body** here.'))
    expect(doc.main.body.text).toBe('Content body here.')
    expect(exportMarkdown(doc.main.body)).toBe('Content **body** here.')
  })

  it('overwrite rejects a non-content value (markdown must go through importMarkdown)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.overwrite({}, 'plain markdown')).toThrow()
    expect(() => doc.overwrite({}, { not: 'a content' })).toThrow()
  })

  // Island `props` and unknown `attrs` are opaque host payload, and
  // every consumer of them (key canonicalization, the hash key, the JS→JSON
  // conversion, the tree's own drop) recurses one frame per level. On wasm32 the
  // stack is 1 MB and an overflow is a trap that takes the module down rather than
  // an error the host can catch, so an over-deep value must throw and leave the
  // module serving. `overwrite` is the reachable door: the value arrives from JS and
  // one loop builds it.
  it('overwrite rejects a deeply nested props instead of trapping the module', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    const rt = importMarkdown('body')
    rt.islands = [{ id: 'i1', type: 'image', props: deep }]
    // Matched on the message: a slot/shape complaint would pass a bare toThrow
    // while the depth door stayed open.
    expect(() => doc.overwrite({}, rt)).toThrow(/nests deeper/)
    doc.overwrite({}, importMarkdown('after'))
    expect(doc.main.body.text).toBe('after')
  })

  // Every door taking opaque host JSON carries the guard, not just `overwrite`:
  // a payload item's `value` crosses on the same `serde_wasm_bindgen` recursion.
  it('insertCard rejects a deeply nested payload item value instead of trapping the module', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    const card = makeCard('note', { ok: 1 }, 'Hello')
    card.payloadItems[0].value = deep
    expect(() => doc.insertCard(card)).toThrow(/nests deeper/)
    doc.insertCard(makeCard('note', { ok: 2 }, 'Hello'))
    expect(doc.cardCount).toBe(1)
  })
})

describe('Content codec: importMarkdown / exportMarkdown / rebase / mapPos', () => {
  it('importMarkdown ∘ exportMarkdown round-trips a body', () => {
    const rt = importMarkdown('A **bold** line.')
    expect(typeof rt).toBe('object')
    expect(rt.text).toBe('A bold line.')
    expect(exportMarkdown(rt)).toBe('A **bold** line.')
  })

  it('answers the canonical form on every Content lane, a zero instance omitted', () => {
    // A read is a write input, and the one form is what a host writes straight
    // back. `instance` costs a key only where two adjacent runs would weld.
    const written = (rt) => rt.lines.flatMap((l) => l.containers).map((c) => c.instance)

    expect(written(importMarkdown('> a\n\n- b'))).toEqual([undefined, undefined])
    expect(written(rebase(importMarkdown('> a'), '> a\n\n- b').content)).toEqual([
      undefined,
      undefined,
    ])
    expect(written(importMarkdown('- a\n\n* b'))).toEqual([undefined, 1])

    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: commit_test\n~~~\n\n> a\n\n- b')
    expect(written(doc.main.body)).toEqual([undefined, undefined])
    expect(written(doc.getStored({}))).toEqual([undefined, undefined])

    // The round trip the one form buys: what a read hands back writes back
    // unchanged.
    doc.overwrite({}, doc.getStored({}))
    expect(doc.bodyMarkdown()).toBe('> a\n\n- b')
  })

  it('rebase computes a content + delta and mapPos maps a position through it', () => {
    const base = importMarkdown('hello world')
    const { content, delta } = rebase(base, 'hello brave world')
    expect(content.text).toBe('hello brave world')
    expect(Array.isArray(delta.ops)).toBe(true)
    // A caret at the end of "hello " stays; one after "world" shifts past "brave ".
    expect(mapPos(delta, 6, 'before')).toBe(6)
    expect(mapPos(delta, 11, 'after')).toBe(17)
  })
})

describe('Content predicates: isInline / isPlain', () => {
  it('judge the inline and plaintext constraints on a Content, throwing on a non-content', () => {
    expect(isInline(importMarkdown('One **bold** line.'))).toBe(true)
    expect(isInline(importMarkdown('One.\n\nTwo.'))).toBe(false)
    expect(isInline(importMarkdown('- item'))).toBe(false)
    expect(isPlain(importMarkdown('One.\n\nTwo.'))).toBe(true)
    expect(isPlain(importMarkdown('One **bold** line.'))).toBe(false)
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
  it('renders severity, message, code, location, path and hint in that order', () => {
    expect(
      formatDiagnostic({
        severity: 'error',
        code: 'validation::coercion_failed',
        message: 'bad date',
        location: { file: 'main.typ', line: 3, column: 7 },
        path: 'cards.memo[0].date',
        hint: 'use ISO 8601',
      }),
    ).toBe(
      '[ERROR] bad date (validation::coercion_failed)\n' +
        '  --> main.typ:3:7\n' +
        '  at cards.memo[0].date\n' +
        '  hint: use ISO 8601',
    )
  })

  it('tags the severity and omits every part the diagnostic does not carry', () => {
    expect(formatDiagnostic({ severity: 'warning', message: 'meh' })).toBe('[WARN] meh')
    expect(formatDiagnostic({ severity: 'error', message: 'bare' })).toBe('[ERROR] bare')
    expect(formatDiagnostic({ severity: 'error', message: 'bare', code: 'c' })).toBe(
      '[ERROR] bare (c)',
    )
    expect(
      formatDiagnostic({
        severity: 'error',
        message: 'bare',
        location: { file: 'f', line: 1, column: 2 },
      }),
    ).toBe('[ERROR] bare\n  --> f:1:2')
    expect(formatDiagnostic({ severity: 'error', message: 'bare', path: 'main.body' })).toBe(
      '[ERROR] bare\n  at main.body',
    )
    expect(formatDiagnostic({ severity: 'error', message: 'bare', hint: 'try' })).toBe(
      '[ERROR] bare\n  hint: try',
    )
  })

  it('throws on a value that is not a diagnostic', () => {
    expect(() => formatDiagnostic({ message: 'no severity' })).toThrow()
    expect(() => formatDiagnostic({ severity: 'fatal', message: 'bad ladder' })).toThrow()
    expect(() => formatDiagnostic(null)).toThrow()
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
    const doc = Document.fromMarkdown(MD)
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
    const doc = Document.fromMarkdown(MD)
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

// One `applyChange` bundle carries three channels — a text delta, island ops
// and line ops — over a field or body whose anchors survive the splice, and
// `mapMarks` answers where each mark lands before the bundle is applied.
describe('Document applyChange: the anchor-preserving change bundle', () => {
  const blankDoc = () => Document.fromMarkdown('~~~card-yaml\n$quill: commit_test\n~~~\n\nBody.')

  it('revise({field}) rebases a richtext field anchor and applyChange splices it', () => {
    const doc = blankDoc()
    // revise the field from markdown (edit semantics), then splice a formatting
    // mark over "bold" via applyChange.
    doc.revise({ field: 'intro' }, 'make it bold here')
    doc.applyChange(
      { field: 'intro' },
      { markOps: [{ op: 'add', start: 8, end: 12, type: 'strong' }] },
    )
    expect(exportMarkdown(field(doc.main, 'intro'))).toBe('make it **bold** here')
    // An out-of-bounds op leaves the value unchanged (all-or-nothing).
    expect(() =>
      doc.applyChange({ field: 'intro' }, { markOps: [{ op: 'add', start: 999, end: 1000, type: 'emph' }] }),
    ).toThrow()
  })

  it('mapMarks answers where applyChange puts a mark across every text-moving channel', () => {
    const doc = blankDoc()
    doc.revise({}, 'hello world')
    doc.applyChange(
      {},
      {
        markOps: [
          { op: 'add', start: 6, end: 6, type: 'anchor', attrs: { id: 'c1' } },
          { op: 'add', start: 6, end: 11, type: 'strong' },
        ],
      },
    )
    const before = doc.main.body
    const bundle = {
      delta: { ops: [{ retain: 6 }, { insert: 'X' }, { retain: 5 }] },
      islandOps: [
        { op: 'insert', at: 6, id: 'i1', type: 'image', props: { url: 'u', alt: 'a' } },
      ],
      lineOps: [{ op: 'split', at: 6 }],
    }

    const predicted = mapMarks(before, bundle)
    doc.applyChange({}, bundle)
    expect(doc.main.body.marks).toEqual(predicted)
    expect(predicted.find((m) => m.type === 'anchor')).toMatchObject({ start: 6, end: 6 })

    // A bundle `applyChange` would refuse throws here rather than answering.
    expect(() => mapMarks(before, { islandOps: [{ op: 'insert', at: 99, id: 'i2', type: 'image', props: {} }] })).toThrow()
  })

  it('applyChange setContinues lowers a hard break op-wise', () => {
    const doc = blankDoc()
    // Two paragraph lines (a delta-inserted `\n` mints `continues:false`), so
    // export separates them with a blank line: two blocks.
    doc.revise({}, 'one two')
    doc.applyChange({}, { delta: { ops: [{ retain: 3 }, { insert: '\n' }, { retain: 4 }] } })
    expect(exportMarkdown(doc.main.body)).toContain('\n\n')

    // setContinues turns the boundary into a within-block hard break: one block,
    // no blank-line separator, and identity anchors ride through (op, not overwrite).
    doc.applyChange({}, { lineOps: [{ op: 'setContinues', line: 1, continues: true }] })
    expect(exportMarkdown(doc.main.body)).not.toContain('\n\n')
    expect(doc.main.body.lines[1].continues).toBe(true)

    // `continues:true` on line 0 has nothing to continue: the mint clears it,
    // and a cleared flag is absent from the wire rather than spelled `false`.
    doc.applyChange({}, { lineOps: [{ op: 'setContinues', line: 0, continues: true }] })
    expect(doc.main.body.lines[0].continues).toBeUndefined()
    expect(doc.main.body.lines[1].continues).toBe(true)
  })

  it('applyChange islandOps edits an island without costing the field its anchors', () => {
    const doc = blankDoc()
    doc.revise({}, 'intro\n\n| H |\n| --- |\n| a |')
    const island = doc.main.body.islands[0]
    expect(island.type).toBe('table')

    // An anchor over "intro", above the table: the thing an `overwrite` would drop.
    doc.applyChange({}, { markOps: [{ op: 'add', start: 0, end: 5, type: 'anchor', attrs: { id: 'c1' } }] })

    doc.applyChange(
      {},
      {
        islandOps: [
          {
            op: 'set',
            id: island.id,
            type: 'table',
            props: {
              header: [{ text: 'H', marks: [] }],
              rows: [[{ text: 'b', marks: [] }]],
              aligns: ['none'],
            },
          },
        ],
      },
    )
    expect(doc.main.body.islands[0].props.rows[0][0].text).toBe('b')
    expect(doc.main.body.marks.some((m) => m.type === 'anchor' && m.attrs.id === 'c1')).toBe(true)

    expect(() =>
      doc.applyChange({}, { islandOps: [{ op: 'set', id: 'nope', type: 'table', props: {} }] }),
    ).toThrow()
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

  it('insertCard appends a card when at is omitted', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard(makeCard('note', {}, 'My card.'))
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(exportMarkdown(doc.cards[0].body)).toBe('My card.')
  })

  it('insertCard throws on invalid kind', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => doc.insertCard({ kind: 'BadKind' }), 'edit::invalid_kind_name')
  })

  it('removeCard → insertCard round-trips a card with fields (read shape == write shape)', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    const removed = doc.removeCard(0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['summary'])
    expect(field(removed, 'foo')).toBe('bar')
    expect(doc.removeCard(5)).toBeUndefined()

    doc.insertCard(removed)
    const repushed = doc.cards[1]
    expect(repushed.kind).toBe('note')
    expect(field(repushed, 'foo')).toBe('bar')
  })

  it('a stale card or item shape is a loud error, not a silent drop', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.insertCard({ kind: 'note', fields: { x: 1 } })).toThrow()
    const stale = { type: 'field', key: 'x', value: 'Example', fill: true }
    expect(() => doc.insertCard({ kind: 'note', payloadItems: [stale] })).toThrow(/fill/)
  })

  it('a card the wire refuses carries the code its addressed mutator mints', () => {
    // Two doors onto one violation: what storeField / setQuillRef throw for a
    // name and a reference is what a card built from a wire throws.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const withField = (key, value) => ({
      kind: 'note',
      payloadItems: [{ type: 'field', key, value }],
    })

    expectEditCode(() => doc.insertCard(withField('bad-name', 1)), 'edit::invalid_field_name')
    expectEditCode(
      () => doc.insertCard({ kind: 'note', quill: '@nope' }),
      'parse::invalid_quill_reference',
    )
  })

  it('insertCard inserts at an index and refuses one past the end', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.insertCard({ kind: 'intro' }, 0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['intro', 'note', 'summary'])
    expect(doc.cardCount).toBe(3)
    expectEditCode(() => doc.insertCard({ kind: 'note' }, 5), 'edit::index_out_of_range')
  })

  it('moveCard reorders and refuses an out-of-range index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(1, 0)
    expect(doc.cards.map((c) => c.kind)).toEqual(['summary', 'note'])
    expectEditCode(() => doc.moveCard(5, 0), 'edit::index_out_of_range')
  })
})

describe('Document.equals / clone', () => {
  it('compares by value across a clone, an emit round-trip and a mutation', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    expect(a.equals(Document.fromMarkdown(TEST_MARKDOWN))).toBe(true)
    expect(a.equals(Document.fromMarkdown(a.toMarkdown()))).toBe(true)

    const clone = a.clone()
    expect(a.equals(clone)).toBe(true)
    clone.storeField('title', 'Changed')
    expect(a.equals(clone)).toBe(false)
    expect(field(a.main, 'title')).toBe('Test Document')
  })
})

describe('Document editor surface: setCardField / overwrite / revise (card)', () => {
  const MD_WITH_CARD = `~~~card-yaml
$quill: test_quill
$kind: main
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card body.
`

  it('storeField / removeField take a card address', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.storeField({ card: 0, field: 'content' }, 'hello')
    expect(field(doc.cards[0], 'content')).toBe('hello')
    expect(doc.removeField({ card: 0, field: 'foo' })).toBe('bar')
    expect(hasField(doc.cards[0], 'foo')).toBe(false)
    expect(doc.removeField({ card: 0, field: 'foo' })).toBeUndefined()
  })

  it('revise / overwrite take a card address', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    const delta = doc.revise({ card: 0 }, 'New card body.')
    expect(exportMarkdown(doc.cards[0].body)).toBe('New card body.')
    expect(Array.isArray(delta.ops)).toBe(true)

    doc.overwrite({ card: 0 }, importMarkdown('Card body from **markdown**.'))
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card body from **markdown**.')
  })

  it('every card-addressed verb throws edit::index_out_of_range when the card is absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const addr = { card: 0, field: 'foo' }
    expectEditCode(() => doc.storeField(addr, 'x'), 'edit::index_out_of_range')
    expectEditCode(() => doc.removeField(addr), 'edit::index_out_of_range')
    expectEditCode(() => doc.revise({ card: 0 }, 'x'), 'edit::index_out_of_range')
    expectEditCode(() => doc.overwrite({ card: 0 }, importMarkdown('x')), 'edit::index_out_of_range')
  })
})

describe('Document editor surface: $ext', () => {
  it('storeExt / getExt / removeExt round the whole map on the main card', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
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
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard({ kind: 'note', body: 'x' })
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
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

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
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
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

typst:
  plate_file: plate.typ

main:
  description: The main card schema
  fields:
    title:
      type: string
      description: The title

card_kinds:
  indorsement:
    description: Indorsement
    fields:
      signature_block:
        type: string
`

  it('exposes identity on metadata and schemas on dedicated getters', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )

    // metadata mirrors the `quill:` section of Quill.yaml: identity only, and
    // its key order is the one BINDINGS.md pins across both surfaces.
    const meta = quill.metadata
    expect(Object.keys(meta)).toEqual(['name', 'version', 'backend', 'author', 'description'])
    expect(meta.name).toBe('meta_test_quill')
    expect(meta.version).toBe('0.2.1')
    expect(meta.backend).toBe('typst')
    expect(meta.author).toBe('Unknown')
    expect(meta.description).toBe('Metadata test')
    // `supportedFormats` is the engine's answer, not the quill's metadata.
    expect(meta.supportedFormats).toBeUndefined()
    expect(engine.supportedFormats(quill).length).toBeGreaterThan(0)
    expect(meta.schema).toBeUndefined()

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
      type: string

card_kinds:
  skills:
    body:
      enabled: false
      example: This example is unused
    fields:
      items:
        type: array
        items:
          type: string
`
    const quill = Quill.fromTree(
      makeQuill({ name: 'warn_quill', plate: TEST_PLATE, quillYaml: WARNING_QUILL_YAML }),
    )

    expect(quill.warnings.map((d) => d.code)).toEqual([
      'quill::body_example_unused',
      'quill::bodiless_card_kind',
    ])
    expect(formatDiagnostic(quill.warnings[0])).toContain('(quill::body_example_unused)')
  })
})

// Which documents produce which diagnostics is core's, pinned in
// `crates/quillmark/tests/validate_test.rs`. Here: the result crosses as a
// plain JS array, and a diagnostic keeps its `code` / `path` / `hint`.
describe('quill.validate', () => {
  const QUILL_YAML = `quill:
  name: validate_smoke_test
  version: "1.0"
  backend: typst
  description: Smoke test for validate

main:
  fields:
    title:
      type: string
    count:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: string
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
    expect(quill.validate(Document.fromMarkdown(md))).toEqual([])
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
    const diags = JSON.parse(JSON.stringify(quill.validate(Document.fromMarkdown(md))))
    const mismatch = diags.find((d) => d.code === 'validation::type_mismatch')
    expect(mismatch.path).toBe('main.count')
    expect(typeof mismatch.hint).toBe('string')
  })
})

// The blueprint text and the authored/default/blank ladder are core's
// (`core/src/quill/blueprint.rs`, `core/src/quill/resolved.rs`). Here: the
// schema DTO's shape and an unanswered cell reaching render.
describe('value schema model', () => {
  // The plate reads `data.title` (no default) and substitutes the optional
  // `data.subtitle` if present, so one quill carries both cell states.
  const SCHEMA_QUILL_YAML = `quill:
  name: schema_test
  version: "1.0"
  backend: typst
  description: value coverage

typst:
  plate_file: plate.typ

main:
  fields:
    title:
      type: string
      description: Document title (no default)
    subtitle:
      type: string
      default: "Untitled subtitle"
      description: Document subtitle (defaulted, shippable)
`

  const SCHEMA_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#let title = data.title
#let subtitle = data.at("subtitle", default: "")
#let body = data.at("$body")

= #title

#subtitle

#body`

  const buildQuill = () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(
      makeQuill({
        name: 'schema_test',
        plate: SCHEMA_PLATE,
        quillYaml: SCHEMA_QUILL_YAML,
      }),
    )
    return { engine, quill }
  }

  it('schema fields carry no `required` axis, and blueprint crosses as a string', () => {
    const { quill } = buildQuill()
    const fields = quill.schema.main.fields

    // Cell status is implied by `default:` presence, not a `required` axis.
    expect('required' in fields.title).toBe(false)
    expect(fields.title.default).toBeUndefined()
    expect(fields.subtitle.default).toBe('Untitled subtitle')

    expect(typeof quill.blueprint).toBe('string')
    expect(quill.blueprint.length).toBeGreaterThan(0)
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
    const result = engine.render(quill, Document.fromMarkdown(md), { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
  })
})
