/**
 * The Document API over the canonical flow:
 * `Quill.fromTree(tree)` → `Document.fromMarkdown(markdown)` →
 * `engine.render(quill, doc, opts)`, against the bundler build.
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
  parseDocPath,
  formatDocPath,
} from '@quillmark-wasm'
import * as typstBuild from '@quillmark-wasm'
import { makeQuill, expectEditCode, initBuildSync } from './test-helpers.js'

initBuildSync(typstBuild, 'backends/typst')

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
  it('should parse markdown with YAML payload', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(doc).toBeDefined()
    expect(doc.quillRef).toBe('test_quill')
  })

  it('should expose typed payload (no $quill / $body / $cards as fields)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(field(doc.main, 'title')).toBe('Test Document')
    expect(field(doc.main, 'author')).toBe('Test Author')
    // $-prefixed system metadata must NOT appear as payload fields
    expect(hasField(doc.main, 'quill')).toBe(false)
    expect(hasField(doc.main, '$quill')).toBe(false)
    expect(hasField(doc.main, '$body')).toBe(false)
    expect(hasField(doc.main, '$cards')).toBe(false)
  })

  it('should expose body as a content with a markdown projection', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // `body` is the canonical content (source-of-truth model); the markdown
    // projection is the on-demand `exportMarkdown(body)` codec.
    expect(typeof doc.main.body).toBe('object')
    expect(typeof doc.main.body.text).toBe('string')
    expect(doc.main.body.text).toContain('Hello World')
    expect(typeof exportMarkdown(doc.main.body)).toBe('string')
    expect(exportMarkdown(doc.main.body)).toContain('Hello World')
  })

  it('should expose cards as an array', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    expect(Array.isArray(doc.cards)).toBe(true)
    expect(doc.cards.length).toBe(0)
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

  it('should expose warnings array', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(Array.isArray(doc.warnings)).toBe(true)
    expect(doc.warnings.length).toBe(0)
  })

  it('should throw on invalid YAML payload', () => {
    const badMarkdown = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test
this is not valid yaml
~~~

# Content`

    expect(() => {
      Document.fromMarkdown(badMarkdown)
    }).toThrow()
  })

  it('should throw when $quill metadata is absent', () => {
    const markdownWithoutQuill = `~~~card-yaml
title: Default Test
author: Test Author
~~~

# Hello Default

This document has no $quill metadata.`

    expect(() => {
      Document.fromMarkdown(markdownWithoutQuill)
    }).toThrow()
  })

  it('attaches err.diagnostics as a non-empty array on thrown errors', () => {
    // Thrown errors normalise to a flat { message, diagnostics[] } shape
    // regardless of whether the underlying failure produced one diagnostic
    // or many.
    try {
      Document.fromMarkdown('')
      throw new Error('fromMarkdown should have thrown')
    } catch (err) {
      expect(Array.isArray(err.diagnostics)).toBe(true)
      expect(err.diagnostics.length).toBeGreaterThanOrEqual(1)
      expect(err.diagnostics[0]).toHaveProperty('message')
      expect(err.diagnostics[0]).toHaveProperty('severity')
      expect(err.message).toMatch(/Empty markdown input/)
    }
  })
})

// ---------------------------------------------------------------------------
// Document.toMarkdown: emitter integration tests
// ---------------------------------------------------------------------------

describe('Document.toMarkdown: fromMarkdown → mutate → emit → re-parse', () => {
  it('general round-trip: mutated document survives emit → re-parse', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const originalCardCount = doc.cards.length  // 0 for TEST_MARKDOWN

    // Mutate
    doc.storeField('title', 'New Title')
    doc.insertCard(Document.makeCard('note', { author: 'Alice' }, 'Hello'))
    doc.revise({}, 'Updated body')

    // Emit
    const emitted = doc.toMarkdown()
    expect(typeof emitted).toBe('string')
    expect(emitted.length).toBeGreaterThan(0)

    // Re-parse and assert structure survives.
    //
    // Note on trailing newlines: the global body is followed by a card fence,
    // so the wire format inserts a line terminator + F2 blank line between
    // them (`Updated body\n\n~~~card-yaml`). On re-parse the F2 blank is
    // stripped but the terminator stays, so `exportMarkdown(doc2.main.body) === 'Updated body\n'`. The card
    // body is at EOF and has no F2 separator, so it survives byte-for-byte.
    const doc2 = Document.fromMarkdown(emitted)
    expect(field(doc2.main, 'title')).toBe('New Title')
    expect(exportMarkdown(doc2.main.body)).toBe('Updated body')
    expect(doc2.cards.length).toBe(originalCardCount + 1)
    expect(doc2.cards[0].kind).toBe('note')
    expect(field(doc2.cards[0], 'author')).toBe('Alice')
    expect(exportMarkdown(doc2.cards[0].body)).toBe('Hello')
  })

  it('a JS string stays a JS string across emit → re-parse, even when YAML-ambiguous', () => {
    // `on` is a YAML 1.1 boolean; the emitter quotes it so it survives. The
    // nine-keyword table (booleans, null, octal-like, date-like) is the
    // emitter's contract, pinned in
    // `core/src/document/tests/ambiguous_strings_tests.rs`. What the boundary
    // owes is that the JS type does not change under it.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('flag_on', 'on')

    const doc2 = Document.fromMarkdown(doc.toMarkdown())

    expect(field(doc2.main, 'flag_on')).toBe('on')
  })
})

// ---------------------------------------------------------------------------
// Document.toStored / Document.fromStored: versioned storage DTO round-trip
// ---------------------------------------------------------------------------

describe('Document JSON DTO: toStored / fromStored', () => {
  // The DTO's content rules (what round-trips, what a reconstruction drops,
  // which payloads are refused) are core's
  // (`core/src/document/dto.rs`). At this boundary the questions are narrower:
  // does the DTO cross as a plain JSON string, does a handle survive the
  // round-trip, and do the JS-only statics (`tryFromStored`, `storageVersionOf`)
  // answer with `undefined` where their throwing twins throw.

  it('toStored emits a plain JSON string carrying the current schema version', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const dto = doc.toStored()
    expect(typeof dto).toBe('string')
    expect(JSON.parse(dto).schema).toBe(Document.currentStorageVersion())
  })

  it('round-trips a mutated document with cards back to an equal handle', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('title', 'New Title')
    doc.insertCard(Document.makeCard('note', { author: 'Alice' }, 'Hello'))

    const restored = Document.fromStored(doc.toStored())

    expect(restored.equals(doc)).toBe(true)
    expect(field(restored.main, 'title')).toBe('New Title')
    expect(restored.cards[0].kind).toBe('note')
    expect(exportMarkdown(restored.cards[0].body)).toBe('Hello')
  })

  it('fromStored throws on a payload it cannot accept', () => {
    expect(() =>
      Document.fromStored('{"schema":"quillmark/document@0.99.0","main":{}}'),
    ).toThrow()
  })

  it('tryFromStored is the non-throwing twin: a Document, or undefined', () => {
    const dto = Document.fromMarkdown(TEST_MARKDOWN).toStored()
    expect(Document.tryFromStored(dto).equals(Document.fromMarkdown(TEST_MARKDOWN))).toBe(true)

    expect(Document.tryFromStored('not json at all')).toBeUndefined()
    expect(
      Document.tryFromStored('{"schema":"quillmark/document@0.99.0","main":{}}'),
    ).toBeUndefined()
  })

  it('storageVersionOf reads the schema tag off any payload, or undefined', () => {
    const current = Document.fromMarkdown(TEST_MARKDOWN).toStored()
    expect(Document.storageVersionOf(current)).toBe(Document.currentStorageVersion())

    // A future version reads back as-is, even though fromStored would reject it.
    expect(
      Document.storageVersionOf('{"schema":"quillmark/document@0.99.0","main":{}}'),
    ).toBe('quillmark/document@0.99.0')

    expect(Document.storageVersionOf('{"foo":"bar"}')).toBeUndefined()
    expect(Document.storageVersionOf(TEST_MARKDOWN)).toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// Authoring text: core's canonical strings, re-exposed
// ---------------------------------------------------------------------------
//
// Four statics whose bodies are `quillmark_core` constants: the single source
// of truth an LLM/MCP consumer authors against. Wording is core's to assert.
// What the binding owns is that each one reaches JS at all: a re-export that
// returns "" is indistinguishable from a working one until a consumer pastes it
// into a prompt.

describe('Document authoring text', () => {
  it('formatRules and quillRefHint carry core text through', () => {
    expect(Document.formatRules().length).toBeGreaterThan(0)
    expect(Document.quillRefHint().length).toBeGreaterThan(0)
  })

  it('blueprintInstruction names the quill it introduces', () => {
    const text = Document.blueprintInstruction('usaf_memo')
    expect(text.length).toBeGreaterThan(0)
    expect(text).toContain('usaf_memo')
  })

  it('formatDiagnostic renders a real diagnostic as pretty text', () => {
    let diag
    try {
      Document.fromMarkdown(TEST_MARKDOWN).storeFields({}, { 'bad-name': 'v' })
    } catch (err) {
      diag = err.diagnostics[0]
    }
    expect(diag).toBeDefined()
    const pretty = Document.formatDiagnostic(diag)
    expect(pretty).toContain(diag.message)
    expect(pretty).toContain(diag.code)
  })
})

describe('Quillmark.quill', () => {
  it('should accept a plain object tree (Record<string, Uint8Array>)', () => {
    const engine = new Quillmark()
    const mapTree = makeQuill({ name: 'test_quill', plate: TEST_PLATE })
    const objectTree = Object.fromEntries(mapTree)

    const fromMap = Quill.fromTree(mapTree)
    const fromObject = Quill.fromTree(objectTree)

    expect(fromMap.backendId).toBe(fromObject.backendId)

    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const r1 = engine.render(fromMap, doc, { format: 'svg' })
    const r2 = engine.render(fromObject, doc, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
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

  it('should render markdown via quill.render(doc, opts) for each format', () => {
    for (const { opts, mimeType } of RENDER_FORMAT_CASES) {
      const engine = new Quillmark()
      const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
      const doc = Document.fromMarkdown(TEST_MARKDOWN)

      const result = opts === undefined ? engine.render(quill, doc) : engine.render(quill, doc, opts)

      expect(result).toBeDefined()
      expect(result.artifacts).toBeDefined()
      expect(result.artifacts.length).toBeGreaterThan(0)
      // The declared TS type is Uint8Array: assert the runtime matches so
      // consumers don't need to defensively coerce `new Uint8Array(bytes)`.
      expect(result.artifacts[0].bytes).toBeInstanceOf(Uint8Array)
      expect(result.artifacts[0].bytes.length).toBeGreaterThan(0)
      expect(result.artifacts[0].mimeType).toBe(mimeType)
    }
  })

  it('should allow rendering the same Document multiple times', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    const pdf = engine.render(quill, doc, { format: 'pdf' })
    const svg = engine.render(quill, doc, { format: 'svg' })

    expect(pdf.artifacts[0].mimeType).toBe('application/pdf')
    expect(svg.artifacts[0].mimeType).toBe('image/svg+xml')
  })

  // `RenderResult.warnings` is the document's parse warnings ahead of the
  // render's own, and a parse warning carries `args`: a declined construct the
  // body still holds warns and renders. `args` is declared
  // `Record<string, unknown>`, so it must read as one on the far side.
  it('a merged parse warning carries its args as a plain object', () => {
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

    const engine = new Quillmark()
    const quill = Quill.fromTree(
      makeQuill({ name: 'decliner', plate: DECLINE_PLATE, quillYaml: DECLINE_QUILL_YAML }),
    )
    const doc = quill.parse('~~~card-yaml\n$quill: decliner\n~~~\n\nAlpha\n\n---\n\nBeta\n')

    const result = engine.render(quill, doc, { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)

    const w = result.warnings.find((d) => d.code === 'plate::unsupported_construct')
    expect(w).toBeDefined()
    expect(w.args).not.toBeInstanceOf(Map)
    expect(w.args.construct).toBe('rule')
  })

  it('session.regions() is always a non-null array, keyed by DocPath', () => {
    // Regions are a session-level query, not on the render result. The document
    // body is a markdown content field, so it auto-tags one region; its address
    // is the canonical DocPath `main.body` (the backend's plate-space `$body` is
    // translated at the session boundary). The result is always an array.
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

    // Document declares a different quill name
    const otherMarkdown = `~~~card-yaml
$quill: other_quill
$kind: main
title: Mismatch Test
~~~

# Content`
    const doc = Document.fromMarkdown(otherMarkdown)

    try {
      engine.render(quill, doc, { format: 'pdf' })
      throw new Error('render should have thrown on a $quill name mismatch')
    } catch (err) {
      expect(Array.isArray(err.diagnostics)).toBe(true)
      expect(err.diagnostics[0].code).toBe('quill::name_mismatch')
    }
  })
})

// ---------------------------------------------------------------------------
// Document editor surface
// ---------------------------------------------------------------------------

describe('Document editor surface: storeField / removeField', () => {
  it('storeField inserts a new payload field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('subtitle', 'A subtitle')
    expect(field(doc.main, 'subtitle')).toBe('A subtitle')
  })

  it('storeField updates an existing field', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeField('title', 'Updated')
    expect(field(doc.main, 'title')).toBe('Updated')
  })

  it('storeField accepts uppercase field names verbatim (lowercase is canonical, not enforced)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['BODY', 'CARDS', 'Title', 'MixedCase_1']) {
      doc.storeField(name, 'x')
      expect(field(doc.main, name)).toBe('x')
    }
  })

  it('storeField throws edit::invalid_field_name for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expectEditCode(() => doc.storeField(name, 'x'), 'edit::invalid_field_name')
    }
  })

  it('storeField throws edit::invalid_field_name for an invalid name (hyphen)', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => doc.storeField('bad-name', 'x'), 'edit::invalid_field_name')
  })

  it('removeField returns the removed value', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const removed = doc.removeField('title')
    expect(removed).toBe('Test Document')
    expect(hasField(doc.main, 'title')).toBe(false)
  })

  it('removeField returns undefined when field absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeField('nonexistent')).toBeUndefined()
  })

  it('removeField throws edit::invalid_field_name for `$`-prefixed names', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    for (const name of ['$body', '$cards', '$quill', '$kind']) {
      expectEditCode(() => doc.removeField(name), 'edit::invalid_field_name')
    }
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
    doc.insertCard(Document.makeCard('note', { foo: 'bar' }))
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
  it('setQuillRef changes the quillRef', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.setQuillRef('new_quill')
    expect(doc.quillRef).toBe('new_quill')
  })

  it('setQuillRef throws on invalid reference', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(
      () => doc.setQuillRef('INVALID QUILL REF WITH SPACES'),
      'parse::invalid_quill_reference',
    )
  })

  it('revise({}, md) revises the main body and returns the text delta', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const delta = doc.revise({}, 'Body from **markdown**.')
    expect(exportMarkdown(doc.main.body)).toBe('Body from **markdown**.')
    // The receipt is a structured-clone-able change set.
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('overwrite({}, rt) writes a content object with value semantics', () => {
    // The content is the source-of-truth shape doc.main.body reads back; the
    // cold path spells importMarkdown at the call site.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const content = importMarkdown('Content **body** here.')
    expect(typeof content).toBe('object')
    doc.overwrite({}, content)
    expect(doc.main.body.text).toBe('Content body here.')
    expect(exportMarkdown(doc.main.body)).toBe('Content **body** here.')
  })

  it('overwrite({}, importMarkdown("")) clears the body', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.overwrite({}, importMarkdown(''))
    expect(exportMarkdown(doc.main.body)).toBe('')
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
    rt.islands = [{ id: 'i1', type: 'widget', loss: 'lossless', props: deep }]
    // Matched on the message: a slot/shape complaint would pass a bare toThrow
    // while the depth door stayed open.
    expect(() => doc.overwrite({}, rt)).toThrow(/nests deeper/)
    // Still alive: the guard errored rather than trapping, so the module keeps
    // serving. A trap would fail every later call in the file, not just this one.
    doc.overwrite({}, importMarkdown('after'))
    expect(doc.main.body.text).toBe('after')
  })

  // Every door taking opaque host JSON carries the guard, not just `overwrite`:
  // a card field value and a payload item's `value` cross on the same
  // `serde_wasm_bindgen` recursion, so each gets its own case.
  it('makeCard rejects a deeply nested field value instead of trapping the module', () => {
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    expect(() => Document.makeCard('note', { tree: deep })).toThrow(/nests deeper/)
    // Still serving: a trap would take every later call in the file with it.
    expect(Document.makeCard('note', { ok: 1 }).kind).toBe('note')
  })

  it('insertCard rejects a deeply nested payload item value instead of trapping the module', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    let deep = []
    for (let i = 0; i < 5000; i++) deep = [deep]
    const card = Document.makeCard('note', { ok: 1 }, 'Hello')
    card.payloadItems[0].value = deep
    expect(() => doc.insertCard(card)).toThrow(/nests deeper/)
    doc.insertCard(Document.makeCard('note', { ok: 2 }, 'Hello'))
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

  it('spells every container instance on a read, so a read is a write input', () => {
    // Storage omits a zero, the seam spells it. Requiring the field on the read
    // type is what reaches `overwrite` and `CardInput.body`, and that is honest
    // only while every `Content`-typed lane spells it.
    const spelled = (rt) => rt.lines.flatMap((l) => l.containers).map((c) => c.instance)

    expect(spelled(importMarkdown('> a\n\n- b'))).toEqual([0, 0])
    expect(spelled(rebase(importMarkdown('> a'), '> a\n\n- b').content)).toEqual([0, 0])

    const doc = Document.fromMarkdown('~~~card-yaml\n$quill: commit_test\n~~~\n\n> a\n\n- b')
    expect(spelled(doc.main.body)).toEqual([0, 0])
    expect(spelled(doc.getStored({}))).toEqual([0, 0])
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

  it('a card diagnostic routes on the head segment, no string parsing', () => {
    const [head] = parseDocPath('cards.indorsement[0].signature_block')
    expect(head.seg).toBe('card')
    expect(head.kind).toBe('indorsement')
    expect(head.index).toBe(0)
  })

  it('parseDocPath throws on a malformed path', () => {
    expect(() => parseDocPath('cards[')).toThrow()
    expect(() => parseDocPath('')).toThrow()
  })

  it('formatDocPath throws on an empty segment array', () => {
    // Symmetric with parseDocPath(''), which throws "empty path".
    expect(() => formatDocPath([])).toThrow()
  })
})

describe('Document-model path: pathFor / cardPath', () => {
  // Card 0 carries a `$kind`, card 1 does not: the two card roots.
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

~~~card-yaml
from: y
~~~

Kindless card.
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
      // A card root is kind-qualified off the live card's stored `$kind`…
      [doc.pathFor({ card: 0 }), 'cards.note[0].body'],
      [doc.pathFor({ card: 0, field: 'from' }), 'cards.note[0].from'],
      [doc.cardPath(0), 'cards.note[0]'],
      // …and unknown-kind when the card carries none.
      [doc.pathFor({ card: 1 }), 'cards[1].body'],
      [doc.pathFor({ card: 1, field: 'from' }), 'cards[1].from'],
      [doc.cardPath(1), 'cards[1]'],
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

  it('throws on a malformed address, as every Addr verb does', () => {
    const doc = Document.fromMarkdown(MD)
    expect(() => doc.pathFor({ crad: 0 })).toThrow()
  })
})

// The typed-commit ABI is `_commitField` / `_commitFields` (hidden from the
// `.d.ts`); `quill.writer(doc)` delegates here. These exercise the ABI directly.
describe('Document typed-commit ABI: _commitField / _commitFields', () => {
  const COMMIT_QUILL_YAML = `quill:
  name: commit_test
  version: "1.0"
  backend: typst
  description: Typed write smoke test

main:
  fields:
    subject:
      type: richtext
      inline: true
    intro:
      type: richtext
    qty:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: richtext
`
  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'commit_test', plate: TEST_PLATE, quillYaml: COMMIT_QUILL_YAML }))
  const blankDoc = () => Document.fromMarkdown('~~~card-yaml\n$quill: commit_test\n~~~\n\nBody.')

  it('commitField resolves the schema type: richtext string → content, integer "3" → 3', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    doc._commitField(quill, 'intro', 'A **bold** intro.')
    expect(typeof field(doc.main, 'intro')).toBe('object')
    // The markdown projection of a richtext field is exportMarkdown ∘ its content.
    expect(exportMarkdown(field(doc.main, 'intro'))).toBe('A **bold** intro.')

    doc._commitField(quill, 'qty', '3')
    expect(field(doc.main, 'qty')).toBe(3)
  })

  it('commitField rejects an unknown field as a typo and writes nothing', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    expectEditCode(() => doc._commitField(quill, 'stray', 'x'), 'edit::unknown_field')
    expect(hasField(doc.main, 'stray')).toBe(false)
    // Opaque storage stays available on purpose through the raw verb.
    doc.storeField('stray', 'x')
    expect(field(doc.main, 'stray')).toBe('x')
  })

  it('exportMarkdown composes on a committed richtext field; a scalar field is not a content', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    // Absent field: the value is undefined, nothing to project.
    expect(field(doc.main, 'nonexistent')).toBeUndefined()
    // A non-richtext scalar is stored verbatim, not a content object.
    doc.storeField('count', 3)
    expect(field(doc.main, 'count')).toBe(3)
    // A committed richtext field projects through the codec.
    doc._commitField(quill, 'intro', 'plain intro')
    expect(exportMarkdown(field(doc.main, 'intro'))).toBe('plain intro')
  })

  it('commitField fails a strict mismatch and a richtext(inline) violation', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    expectEditCode(() => doc._commitField(quill, 'qty', 'not-a-number'), 'edit::field_coercion_failed')
    expectEditCode(
      () => doc._commitField(quill, 'subject', 'line one\n\nline two'),
      'edit::field_not_inline',
    )
  })

  it('revise({field}) rebases a richtext field anchor and applyChange splices it', () => {
    const quill = buildQuill()
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

  it('mapMarks answers where applyChange puts a mark, at the position the assocs differ', () => {
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
    const bundle = { delta: { ops: [{ retain: 6 }, { insert: 'X' }, { retain: 5 }] } }

    // The trap: `mapPos` answers for a position the caller holds, and a caret
    // typing here moves past its own text. A zero-width mark in the field does
    // not — the store rebases it `before`.
    expect(mapPos(bundle.delta, 6, 'after')).toBe(7)
    const predicted = mapMarks(before, bundle)
    expect(predicted.find((m) => m.type === 'anchor')).toMatchObject({ start: 6, end: 6 })
    expect(predicted.find((m) => m.type === 'strong')).toMatchObject({ start: 7, end: 12 })

    doc.applyChange({}, bundle)
    expect(doc.main.body.marks).toEqual(predicted)
  })

  it('mapMarks carries a mark through every text-moving channel of one bundle', () => {
    const doc = blankDoc()
    doc.revise({}, 'hello world')
    doc.applyChange({}, { markOps: [{ op: 'add', start: 6, end: 6, type: 'anchor', attrs: { id: 'c1' } }] })
    const before = doc.main.body
    const bundle = {
      delta: { ops: [{ retain: 6 }, { insert: 'X' }, { retain: 5 }] },
      islandOps: [
        { op: 'insert', at: 6, id: 'i1', type: 'image', loss: 'lossless', props: { url: 'u', alt: 'a' } },
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

    // `continues:true` on line 0 has nothing to continue: rejected, value intact.
    expect(() =>
      doc.applyChange({}, { lineOps: [{ op: 'setContinues', line: 0, continues: true }] }),
    ).toThrow()
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
            loss: 'lossless',
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

    // An id no island carries throws rather than passing as a silent no-op.
    expect(() =>
      doc.applyChange({}, { islandOps: [{ op: 'set', id: 'nope', type: 'table', props: {} }] }),
    ).toThrow()
  })

  it('applyChange creates a block island in one bundle', () => {
    const doc = blankDoc()
    doc.revise({}, 'intro')
    // The three channels in the order they apply: the delta opens the line, the
    // island op fills it, setKind tags it. `split` could not open that line:
    // line ops run after island ops.
    doc.applyChange(
      {},
      {
        delta: { ops: [{ retain: 5 }, { insert: '\n' }] },
        islandOps: [
          {
            op: 'insert',
            at: 6,
            id: 'isl-new',
            type: 'image',
            loss: 'lossless',
            props: { url: 'ex.com/a.png', alt: 'a' },
          },
        ],
        lineOps: [{ op: 'setKind', line: 1, kind: 'island' }],
      },
    )
    expect(doc.main.body.islands.map((i) => i.id)).toEqual(['isl-new'])
    expect(exportMarkdown(doc.main.body)).toContain('![a](ex.com/a.png)')

    // A duplicate id is refused, and the failed bundle changes nothing.
    expect(() =>
      doc.applyChange(
        {},
        { islandOps: [{ op: 'insert', at: 0, id: 'isl-new', type: 'image', props: {} }] },
      ),
    ).toThrow()
    expect(doc.main.body.islands.length).toBe(1)
  })

  it('commitCardField resolves the card-kind schema and errors on a bad index', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: commit_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    doc._commitField(quill, { card: 0, field: 'body' }, 'Card **body**.')
    expect(exportMarkdown(field(doc.cards[0], 'body'))).toBe('Card **body**.')
    expectEditCode(() => doc._commitField(quill, { card: 9, field: 'body' }, 'x'), 'edit::index_out_of_range')
  })

  it('a mutator diagnostic carries the DocPath it anchors to', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: commit_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
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
    expect(pathOf(() => doc._commitField(quill, 'qty', 'not-a-number'))).toBe('main.qty')
    // …a card field is kind-qualified with its absolute index…
    expect(pathOf(() => doc._commitField(quill, { card: 0, field: 'stray' }, 'x'))).toBe(
      'cards.note[0].stray',
    )
    // …and a structural out-of-range op anchors at the array slot.
    expect(pathOf(() => doc.setCardKind(9, 'note'))).toBe('cards[9]')
    expect(pathOf(() => doc.moveCard(9, 0))).toBe('cards[9]')
    // `pathFor` mints what the anchor carries, so a consumer's path and the
    // engine's agree without a kind table of its own.
    expect(doc.pathFor({ card: 0, field: 'stray' })).toBe(
      pathOf(() => doc._commitField(quill, { card: 0, field: 'stray' }, 'x')),
    )
  })

  it('commitFields typed-commits a batch', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    doc._commitFields(quill, {}, { intro: 'A **bold** intro.', qty: '3' })
    // The values were coerced, not stored verbatim.
    expect(exportMarkdown(field(doc.main, 'intro'))).toBe('A **bold** intro.')
    expect(field(doc.main, 'qty')).toBe(3)
  })

  it('commitFields aborts the whole batch on a typo, reporting the unknown field', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    // `qty` is a schema field; `titel` is a typo the schema does not own: the
    // undeclared name aborts the all-or-nothing batch and nothing is applied.
    expectEditCode(() => doc._commitFields(quill, {}, { qty: '5', titel: 'oops' }), 'edit::unknown_field')
    expect(hasField(doc.main, 'qty')).toBe(false)
    expect(hasField(doc.main, 'titel')).toBe(false)
  })

  it('commitFields is all-or-nothing: a bad field aborts the whole batch', () => {
    const quill = buildQuill()
    const doc = blankDoc()
    // `subject` is richtext(inline); a multi-block value violates it, so nothing
    // is applied: `qty` must not linger.
    expectEditCode(
      () => doc._commitFields(quill, {}, { qty: '5', subject: 'line one\n\nline two' }),
      'edit::field_not_inline',
    )
    expect(hasField(doc.main, 'qty')).toBe(false)
  })

  it('commitCardFields typed-commits card fields and errors on a bad index', () => {
    const quill = buildQuill()
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: commit_test\n~~~\n\nMain.\n\n~~~card-yaml\n$kind: note\n~~~\n\nCard.',
    )
    doc._commitFields(quill, { card: 0 }, { body: 'Card **body**.' })
    expect(exportMarkdown(field(doc.cards[0], 'body'))).toBe('Card **body**.')
    // An undeclared field on the card aborts the batch.
    expectEditCode(() => doc._commitFields(quill, { card: 0 }, { stray: 'x' }), 'edit::unknown_field')
    expectEditCode(() => doc._commitFields(quill, { card: 9 }, { body: 'x' }), 'edit::index_out_of_range')
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
    doc.insertCard(Document.makeCard('note', {}, 'My card.'))
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(exportMarkdown(doc.cards[0].body)).toBe('My card.')
  })

  it('insertCard throws on invalid kind', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => doc.insertCard({ kind: 'BadKind' }), 'edit::invalid_kind_name')
  })

  it('removeCard → insertCard round-trips a card with fields (read shape == write shape)', () => {
    // The whole point of the one-Card-shape change: a card returned by
    // removeCard feeds straight back into insertCard with its fields intact.
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // `note` (foo: bar) + `summary`
    const initialCount = doc.cards.length
    const removed = doc.removeCard(0) // the `note` card
    expect(doc.cards.length).toBe(initialCount - 1)
    expect(field(removed, 'foo')).toBe('bar')

    doc.insertCard(removed) // re-push the returned card; fields must not drop
    expect(doc.cards.length).toBe(initialCount)
    const repushed = doc.cards[doc.cards.length - 1]
    expect(repushed.kind).toBe('note')
    expect(field(repushed, 'foo')).toBe('bar')
  })

  it('makeCard accepts any kind; insertCard is the kind gate', () => {
    // makeCard is pure data-shaping (permissive); the cards-list invariant is
    // enforced at insertion, not construction.
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const bad = Document.makeCard('BadKind', { x: 1 })
    expect(bad.kind).toBe('BadKind') // construction succeeds
    expectEditCode(() => doc.insertCard(bad), 'edit::invalid_kind_name') // insertion rejects
  })

  it('makeCard treats fields and body as optional', () => {
    // Both `fields` and `body` are omittable; a bare kind yields an empty card.
    // The `.d.ts` marks them `fields?` / `body?` to match (see makeCard's
    // unchecked_optional_param_type bindings).
    const bare = Document.makeCard('note')
    expect(bare.kind).toBe('note')
    expect(bare.payloadItems).toEqual([])
    expect(exportMarkdown(bare.body)).toBe('')
  })

  it('a stale { kind, fields } object is a loud error, not a silent empty card', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.insertCard({ kind: 'note', fields: { x: 1 } })).toThrow()
  })

  it('insertCard inserts at specified index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.insertCard({ kind: 'intro' }, 0)
    expect(doc.cards[0].kind).toBe('intro')
    expect(doc.cards[1].kind).toBe('note')
  })

  it('insertCard throws IndexOutOfRange when at > len', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expectEditCode(() => doc.insertCard({ kind: 'note' }, 5), 'edit::index_out_of_range')
  })

  it('removeCard removes and returns the card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    const removed = doc.removeCard(0)
    expect(removed).toBeDefined()
    expect(removed.kind).toBe('note')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('summary')
  })

  it('removeCard returns undefined when out of range', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.removeCard(0)).toBeUndefined()
  })

  it('moveCard swaps positions correctly', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(1, 0) // summary → front
    expect(doc.cards[0].kind).toBe('summary')
    expect(doc.cards[1].kind).toBe('note')
  })

  it('moveCard no-op when from == to', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.moveCard(0, 0)
    expect(doc.cards[0].kind).toBe('note')
  })

  it('moveCard throws IndexOutOfRange on out-of-range index', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expectEditCode(() => doc.moveCard(5, 0), 'edit::index_out_of_range')
  })

  it('setCardKind renames the kind in place', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    doc.setCardKind(0, 'annotation')
    expect(doc.cards[0].kind).toBe('annotation')
    // Payload items preserved across rename.
    expect(Array.isArray(doc.cards[0].payloadItems)).toBe(true)
  })

  it('setCardKind throws InvalidKindName for empty/uppercase/dashed kinds', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS)
    for (const bad of ['', 'BadKind', 'with-dash']) {
      expectEditCode(() => doc.setCardKind(0, bad), 'edit::invalid_kind_name')
    }
  })

  it('setCardKind throws IndexOutOfRange when index >= len', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARDS) // 2 cards
    expectEditCode(() => doc.setCardKind(5, 'annotation'), 'edit::index_out_of_range')
  })

  it('cardCount reports composable card count without allocating', () => {
    const empty = Document.fromMarkdown(TEST_MARKDOWN)
    expect(empty.cardCount).toBe(0)

    const two = Document.fromMarkdown(MD_WITH_CARDS)
    expect(two.cardCount).toBe(2)
    two.insertCard({ kind: 'extra' })
    expect(two.cardCount).toBe(3)
    two.removeCard(0)
    expect(two.cardCount).toBe(2)
  })
})

describe('Document.equals', () => {
  it('returns true for identical documents', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    expect(a.equals(b)).toBe(true)
  })

  it('returns true for clones', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = a.clone()
    expect(a.equals(b)).toBe(true)
  })

  it('returns false after a payload mutation', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.storeField('title', 'Different')
    expect(a.equals(b)).toBe(false)
  })

  it('returns false after a body mutation', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.revise({}, 'Different body')
    expect(a.equals(b)).toBe(false)
  })

  it('returns false after pushing a card', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(TEST_MARKDOWN)
    b.insertCard({ kind: 'note' })
    expect(a.equals(b)).toBe(false)
  })

  it('survives round-trip through toMarkdown / fromMarkdown', () => {
    const a = Document.fromMarkdown(TEST_MARKDOWN)
    const b = Document.fromMarkdown(a.toMarkdown())
    expect(a.equals(b)).toBe(true)
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

  it('setCardField sets a field on a card', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.storeField({ card: 0, field: 'content' }, 'hello')
    expect(field(doc.cards[0], 'content')).toBe('hello')
  })

  it('setCardField accepts uppercase names verbatim', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.storeField({ card: 0, field: 'BODY' }, 'x')
    expect(field(doc.cards[0], 'BODY')).toBe('x')
  })

  it('setCardField throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expectEditCode(() => doc.storeField({ card: 0, field: 'title' }, 'x'), 'edit::index_out_of_range')
  })

  it('removeCardField returns the removed value and deletes the key', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    const removed = doc.removeField({ card: 0, field: 'foo' })
    expect(removed).toBe('bar')
    expect(hasField(doc.cards[0], 'foo')).toBe(false)
  })

  it('removeCardField returns undefined when field absent', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    expect(doc.removeField({ card: 0, field: 'nonexistent' })).toBeUndefined()
  })

  it('removeCardField throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expectEditCode(() => doc.removeField({ card: 0, field: 'foo' }), 'edit::index_out_of_range')
  })

  it('revise({card:0}, md) revises a card body and returns the delta', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    const delta = doc.revise({ card: 0 }, 'New card body.')
    expect(exportMarkdown(doc.cards[0].body)).toBe('New card body.')
    expect(Array.isArray(delta.ops)).toBe(true)
  })

  it('revise({card:0}, md) throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expectEditCode(() => doc.revise({ card: 0 }, 'x'), 'edit::index_out_of_range')
  })

  it('overwrite({card:0}, rt) writes a content into a card body', () => {
    // The content is the shape doc.cards[i].body reads back; the card-indexed
    // twin of the main-body overwrite path.
    const content = importMarkdown('Card body from **markdown**.')
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.overwrite({ card: 0 }, content)
    expect(doc.cards[0].body.text).toBe(content.text)
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card body from **markdown**.')
  })

  it('overwrite({card:0}, importMarkdown("")) clears the card body', () => {
    const doc = Document.fromMarkdown(MD_WITH_CARD)
    doc.overwrite({ card: 0 }, importMarkdown(''))
    expect(doc.cards[0].body.text).toBe('')
  })

  it('overwrite({card:0}, ...) throws IndexOutOfRange when card absent', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN) // 0 cards
    expectEditCode(() => doc.overwrite({ card: 0 }, importMarkdown('x')), 'edit::index_out_of_range')
  })
})

describe('Document editor surface: parse→mutate→read round-trip', () => {
  it('mutated document reflects changes in subsequent reads', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)

    // Mutate
    doc.storeField('author', 'Bob')
    doc.revise({}, 'New body text.')
    doc.insertCard({ kind: 'note', body: 'Card content.' })
    doc.setQuillRef('updated_quill')

    // Assert state
    expect(field(doc.main, 'author')).toBe('Bob')
    expect(exportMarkdown(doc.main.body)).toBe('New body text.')
    expect(doc.cards.length).toBe(1)
    expect(doc.cards[0].kind).toBe('note')
    expect(exportMarkdown(doc.cards[0].body)).toBe('Card content.')
    expect(doc.quillRef).toBe('updated_quill')

    // Original title still present
    expect(field(doc.main, 'title')).toBe('Test Document')

    // Warnings untouched
    expect(Array.isArray(doc.warnings)).toBe(true)
  })
})

describe('Document editor surface: $ext mutators', () => {
  it('storeExt adds an opaque map readable via card.ext', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExt({}, { editor: { title: 'Greeting' } })
    expect(doc.main.ext.editor.title).toBe('Greeting')
  })

  it('storeExt rejects non-object values', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(() => doc.storeExt({}, 'nope')).toThrow(/must be a plain object/)
    expect(() => doc.storeExt({}, 42)).toThrow(/must be a plain object/)
  })

  it('$ext round-trips through toMarkdown', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExt({}, { agent: { pinned: true } })
    const reparsed = Document.fromMarkdown(doc.toMarkdown())
    expect(reparsed.main.ext.agent.pinned).toBe(true)
  })

  it('storeExtNamespace preserves sibling namespaces', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExtNamespace({}, 'editor', { title: 'A' })
    doc.storeExtNamespace({}, 'agent', { pinned: true })
    doc.storeExtNamespace({}, 'editor', { title: 'B' })
    expect(doc.main.ext.editor.title).toBe('B')
    expect(doc.main.ext.agent.pinned).toBe(true)
  })

  it('removeExtNamespace clears one slot and drops $ext once empty', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExtNamespace({}, 'editor', { title: 'A' })
    doc.storeExtNamespace({}, 'tutorial', ['step-1', 'step-2'])
    // Returns the removed value; siblings survive.
    expect(doc.removeExtNamespace({}, 'tutorial')).toEqual(['step-1', 'step-2'])
    expect(doc.main.ext.editor.title).toBe('A')
    expect(doc.main.ext.tutorial).toBeUndefined()
    // Removing the last namespace clears $ext entirely.
    doc.removeExtNamespace({}, 'editor')
    expect(doc.main.ext == null).toBe(true)
    // Absent namespace is a no-op returning undefined.
    expect(doc.removeExtNamespace({}, 'nope')).toBeUndefined()
  })

  it('removeExt returns the previous map and clears it', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExt({}, { agent: { n: 1 } })
    expect(doc.removeExt().agent.n).toBe(1)
    expect(doc.main.ext == null).toBe(true)
    expect(doc.removeExt()).toBeUndefined()
  })

  it('card-level ext mutators target the card at index', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard({ kind: 'note', body: 'x' })
    doc.storeExt({ card: 0 }, { agent: { note: 'y' } })
    expect(doc.cards[0].ext.agent.note).toBe('y')
    expect(doc.removeExt({ card: 0 }).agent.note).toBe('y')
    expect(doc.cards[0].ext == null).toBe(true)
  })

  it('card-level namespace mutators preserve siblings and clear when empty', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard({ kind: 'note', body: 'x' })
    doc.storeExtNamespace({ card: 0 }, 'editor', { title: 'A' })
    doc.storeExtNamespace({ card: 0 }, 'tutorial', ['step-1'])
    expect(doc.removeExtNamespace({ card: 0 }, 'tutorial')).toEqual(['step-1'])
    expect(doc.cards[0].ext.editor.title).toBe('A')
    doc.removeExtNamespace({ card: 0 }, 'editor')
    expect(doc.cards[0].ext == null).toBe(true)
  })

  it('card-level ext mutators throw IndexOutOfRange', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expectEditCode(() => doc.storeExt({ card: 5 }, {}), 'edit::index_out_of_range')
    expectEditCode(() => doc.removeExt({ card: 5 }), 'edit::index_out_of_range')
    expectEditCode(() => doc.storeExtNamespace({ card: 5 }, 'a', {}), 'edit::index_out_of_range')
    expectEditCode(() => doc.removeExtNamespace({ card: 5 }, 'a'), 'edit::index_out_of_range')
  })
})

// ---------------------------------------------------------------------------
// open + session.render
// ---------------------------------------------------------------------------

describe('Document editor surface: $ext reads', () => {
  it('getExt returns the whole map, undefined when the card carries none', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    expect(doc.getExt({})).toBeUndefined()
    doc.storeExt({}, { editor: { title: 'A' }, agent: { pinned: true } })
    expect(doc.getExt({})).toEqual({ editor: { title: 'A' }, agent: { pinned: true } })
  })

  it('getExtNamespace reads one slot, non-destructively', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeExtNamespace({}, 'tutorial', ['step-1', 'step-2'])
    expect(doc.getExtNamespace({}, 'tutorial')).toEqual(['step-1', 'step-2'])
    expect(doc.getExtNamespace({}, 'nope')).toBeUndefined()
    // Unlike removeExtNamespace, reading twice yields the same value.
    expect(doc.getExtNamespace({}, 'tutorial')).toEqual(['step-1', 'step-2'])
  })

  it('both reads are card-indexed and take a card address only', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard({ kind: 'note', body: 'x' })
    doc.storeExt({ card: 0 }, { agent: { note: 'y' } })
    expect(doc.getExt({ card: 0 }).agent.note).toBe('y')
    expect(doc.getExtNamespace({ card: 0 }, 'agent')).toEqual({ note: 'y' })
    expect(() => doc.getExt({ field: 'title' })).toThrow(/getExt/)
    expectEditCode(() => doc.getExt({ card: 5 }), 'edit::index_out_of_range')
    expectEditCode(() => doc.getExtNamespace({ card: 5 }, 'agent'), 'edit::index_out_of_range')
  })
})

describe('Document editor surface: storeFill / isFill', () => {
  it('storeFill stores the value and marks the field !must_fill', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeFill('subject', 'Subject of the Memorandum')
    expect(doc.isFill('subject')).toBe(true)
    expect(doc.toMarkdown()).toContain('subject: !must_fill Subject of the Memorandum')
  })

  it('storeField clears the marker storeFill set', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.storeFill('subject', 'x')
    doc.storeField('subject', 'x')
    expect(doc.isFill('subject')).toBe(false)
  })

  it('isFill is total over the field axis: only a bad card throws', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    // Absent field: truthfully unmarked, not an error.
    expect(doc.isFill('nonesuch')).toBe(false)
    // A body address (no `field`) is never a fill.
    expect(doc.isFill({})).toBe(false)
    expectEditCode(() => doc.isFill({ card: 5, field: 'title' }), 'edit::index_out_of_range')
  })

  it('storeFill is card-capable and rejects a body address', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    doc.insertCard({ kind: 'note', body: 'x' })
    doc.storeFill({ card: 0, field: 'signer' }, 'TBD')
    expect(doc.isFill({ card: 0, field: 'signer' })).toBe(true)
    expect(doc.isFill({ card: 0, field: 'other' })).toBe(false)
    expect(() => doc.storeFill({}, 'v')).toThrow(/storeFill/)
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

  it('should throw on out-of-bounds page indices', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)
    const oob = session.pageCount + 10

    expect(() => {
      session.render({ format: 'png', ppi: 80, pages: [0, oob] })
    }).toThrow(/out of bounds/)
  })

  it('should error when requesting page selection with PDF', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const session = engine.open(quill, doc)

    expect(() => {
      session.render({ format: 'pdf', pages: [0] })
    }).toThrow()
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

    // metadata mirrors the `quill:` section of Quill.yaml: identity only.
    const meta = quill.metadata
    expect(meta).toBeDefined()
    expect(meta.name).toBe('meta_test_quill')
    expect(meta.version).toBe('0.2.1')
    expect(meta.backend).toBe('typst')
    expect(meta.author).toBe('Unknown')
    expect(meta.description).toBe('Metadata test')
    // supportedFormats moved off metadata onto the engine.
    expect(meta.supportedFormats).toBeUndefined()
    const supportedFormats = engine.supportedFormats(quill)
    expect(Array.isArray(supportedFormats)).toBe(true)
    expect(supportedFormats.length).toBeGreaterThan(0)
    expect(meta.schema).toBeUndefined()

    // schema: user-fillable fields + ui hints. No QUILL/CARD sentinels.
    const schema = quill.schema
    expect(schema.main.description).toBe('The main card schema')
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.main.fields.QUILL).toBeUndefined()
    expect(schema.card_kinds.main).toBeUndefined()
    expect(schema.card_kinds.indorsement.fields.signature_block).toBeDefined()
    expect(schema.card_kinds.indorsement.fields.CARD).toBeUndefined()
  })

  it('orders the five standard keys first, then the extra keys sorted by name', () => {
    const EXTRAS_QUILL_YAML = `quill:
  name: meta_test_quill
  version: "0.2.1"
  backend: typst
  description: Metadata test

typst:
  zeta: z
  plate_file: plate.typ
  alpha: a
  nu: n
  beta: b
  mu: m
`
    const quill = Quill.fromTree(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: EXTRAS_QUILL_YAML }),
    )

    expect(Object.keys(quill.metadata)).toEqual([
      'name',
      'version',
      'backend',
      'author',
      'description',
      'typst_alpha',
      'typst_beta',
      'typst_mu',
      'typst_nu',
      'typst_plate_file',
      'typst_zeta',
    ])
  })

  it('metadata and schema are JSON.stringify-able (plain objects)', () => {
    const quill = Quill.fromTree(
      makeQuill({ name: 'meta_test_quill', plate: TEST_PLATE, quillYaml: META_QUILL_YAML }),
    )
    const meta = JSON.parse(JSON.stringify(quill.metadata))
    expect(meta.name).toBe('meta_test_quill')
    const schema = JSON.parse(JSON.stringify(quill.schema))
    expect(schema.main.fields.title).toBeDefined()
    expect(schema.main.fields.QUILL).toBeUndefined()
  })
})

describe('Document.clone', () => {
  it('returns an independent handle', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    clone.storeField('title', 'Changed')

    expect(field(doc.main, 'title')).toBe('Test Document')
    expect(field(clone.main, 'title')).toBe('Changed')
  })

  it('preserves parse-time warnings on the clone', () => {
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    expect(clone.warnings.length).toBe(doc.warnings.length)
  })

  it('produces a clone that renders equivalently to the original', () => {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
    const doc = Document.fromMarkdown(TEST_MARKDOWN)
    const clone = doc.clone()

    const r1 = engine.render(quill, doc, { format: 'svg' })
    const r2 = engine.render(quill, clone, { format: 'svg' })
    expect(r1.artifacts.length).toBe(r2.artifacts.length)
  })
})

// ---------------------------------------------------------------------------
// quill.validate: editor-facing schema validation
// Run via `npm test` after scripts/build-wasm.sh has produced the bundle;
// vitest loads it in a Node environment.
// ---------------------------------------------------------------------------

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
    const diags = quill.validate(Document.fromMarkdown(md))
    expect(Array.isArray(diags)).toBe(true)
    expect(diags.length).toBe(0)
  })

  it('forwards a type_mismatch with canonical code, path, and hint', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
title: "Hello"
count: "not-a-number"
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    const mismatch = diags.find((d) => d.code === 'validation::type_mismatch')
    expect(mismatch).toBeDefined()
    expect(mismatch.path).toBe('main.count')
    expect(typeof mismatch.hint).toBe('string')
  })

  it('result is JSON.stringify-able', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: validate_smoke_test
$kind: main
count: "nope"
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    const json = JSON.stringify(diags)
    expect(typeof json).toBe('string')
    expect(JSON.parse(json).length).toBe(diags.length)
  })
})

// ---------------------------------------------------------------------------
// Schema / blueprint / validation: the value and obligation axes
// ---------------------------------------------------------------------------
//
// A `default:` holds the cell and its value is shippable as rendered; without
// one the field is obliged, so the blueprint emits a `!must_fill` marker and
// render blank-fills it.
//
// The blueprint's exact text is pinned line-by-line in
// `core/src/quill/blueprint.rs`, and the authored/default/blank ladder in
// `core/src/quill/resolved.rs`. What is JS-facing, and lives here:
// the schema DTO's shape, the blueprint crossing as a string, and a
// `!must_fill` marker reaching both render and validate intact.
//
// See prose/canon/SCHEMAS.md.

describe('value / obligation schema model', () => {
  // The plate `unwrap`s `data.title` (obliged) and substitutes the optional
  // `data.subtitle` if present, so one quill carries both cell states.
  const SCHEMA_QUILL_YAML = `quill:
  name: schema_test
  version: "1.0"
  backend: typst
  description: value / obligation coverage

typst:
  plate_file: plate.typ

main:
  fields:
    title:
      type: string
      description: Document title (obliged, no default)
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

  it('render tolerates a `!must_fill` marker left in (non-fatal, blank-fills)', () => {
    const { engine, quill } = buildQuill()

    // The marker survives the boundary as a marker rather than a bare null, so
    // render blank-fills the field and succeeds.
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: !must_fill
~~~

# Body
`
    const result = engine.render(quill, Document.fromMarkdown(md), { format: 'svg' })
    expect(result.artifacts.length).toBeGreaterThan(0)
  })

  it('validate surfaces a non-fatal `validation::must_fill` warning per marker', () => {
    const { quill } = buildQuill()

    const md = `~~~card-yaml
$quill: schema_test
$kind: main
title: !must_fill
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    expect(
      diags.some(
        (d) =>
          d.code === 'validation::must_fill' &&
          d.severity === 'warning' &&
          d.path === 'main.title' &&
          typeof d.hint === 'string',
      ),
    ).toBe(true)
  })

  it('validate surfaces the same code for a cell the schema obliges and nobody authored', () => {
    const { quill } = buildQuill()

    // No marker anywhere: this document never saw a blueprint.
    const md = `~~~card-yaml
$quill: schema_test
$kind: main
~~~
`
    const diags = quill.validate(Document.fromMarkdown(md))
    const title = diags.find(
      (d) => d.code === 'validation::must_fill' && d.path === 'main.title',
    )
    expect(title).toBeDefined()
    expect(title.severity).toBe('warning')
    // `trigger` is what a consumer routes on, so pin it crossing the boundary.
    expect(title.args.trigger).toBe('unauthored')
  })
})

describe('nested !must_fill', () => {
  it('exposes nestedFills on a field item, surviving storage and insertCard', () => {
    const md = `~~~card-yaml
$quill: q@0.1
$kind: main
addr:
  street: !must_fill
  city: Anytown
~~~
`
    const doc = Document.fromMarkdown(md)
    const addr = doc.main.payloadItems.find((i) => i.key === 'addr')
    expect(addr.nestedFills).toEqual([['street']])

    // Storage round-trip preserves the nested marker.
    const restored = Document.fromStored(doc.toStored())
    expect(restored.toMarkdown()).toContain('street: !must_fill')

    // A card built with nestedFills survives insertCard → emit.
    const doc2 = Document.fromMarkdown(
      '~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: x\n~~~\n',
    )
    doc2.insertCard({
      kind: 'note',
      payloadItems: [
        {
          type: 'field',
          key: 'addr',
          value: { street: null, city: 'A' },
          nestedFills: [['street']],
        },
      ],
      body: '',
    })
    expect(doc2.toMarkdown()).toContain('street: !must_fill')
  })

  it('omits nestedFills for a field with no nested markers', () => {
    const doc = Document.fromMarkdown(
      '~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: Hello\n~~~\n',
    )
    const title = doc.main.payloadItems.find((i) => i.key === 'title')
    expect(title.nestedFills).toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// reader.resolve, through its `_resolve` ABI: the resolved-value view
// ---------------------------------------------------------------------------
//
// For every declared field: the value the render projection would use and the
// source rung it came from ("authored" | "default" | "blank"). Rows are an
// ordered array carrying their own `name`; the card body is a `body` sibling,
// not a row in `fields`. Value and provenance only: diagnostics stay
// validate(), guidance stays the schema. See prose/canon/SCHEMAS.md
// § "Value sources and projections".

describe('reader.resolve, through the _resolve ABI', () => {
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

  // Rows are an ordered array now; look one up by its `name`.
  const byName = (rows, name) => rows.find((r) => r.name === name)

  it('tags main rows with their authored / default / blank source', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: field_states_test
$kind: main
title: Hello
~~~
`
    const f = quill._resolve(Document.fromMarkdown(md)).main.fields

    // Declaration order is structural: the array order is the contract.
    expect(f.map((r) => r.name)).toEqual(['title', 'status', 'notes', 'count', 'author'])

    expect(byName(f, 'title').source).toBe('authored')
    expect(byName(f, 'title').value).toBe('Hello')
    expect(byName(f, 'status').source).toBe('default')
    expect(byName(f, 'status').value).toBe('draft')
    expect(byName(f, 'notes').source).toBe('blank')
    expect(byName(f, 'notes').value).toBe('')
  })

  it('carries the body as a `body` sibling, never a row in fields', () => {
    const quill = buildQuill()
    const authored = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
~~~

Hello body.
`
    const withBody = quill._resolve(Document.fromMarkdown(authored))
    expect(withBody.main.body).toBeDefined()
    expect(withBody.main.body.source).toBe('authored')
    // Not smuggled into the fields array under any `body` / `$body` name.
    expect(byName(withBody.main.fields, 'body')).toBeUndefined()
    expect(byName(withBody.main.fields, '$body')).toBeUndefined()

    const blank = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
~~~
`
    const noBody = quill._resolve(Document.fromMarkdown(blank))
    expect(noBody.main.body.source).toBe('blank')
  })

  it('carries value and source only: no diagnostics, no example', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
~~~
`
    const f = quill._resolve(Document.fromMarkdown(md)).main.fields
    // Each row is exactly { name, value, source }, schema guidance (example:)
    // and diagnostics read from quill.schema / quill.validate, not duplicated.
    const author = byName(f, 'author')
    expect(Object.keys(author).sort()).toEqual(['name', 'source', 'value'])
    expect('example' in author).toBe(false)
    expect('diagnostics' in author).toBe(false)
  })

  it('reports kind and index on a card entry', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
~~~

~~~card-yaml
$kind: note
label: L
~~~
Note body.
`
    const states = quill._resolve(Document.fromMarkdown(md))
    expect(states.cards.length).toBe(1)
    const card = states.cards[0]
    expect(card.kind).toBe('note')
    expect(card.index).toBe(0)
    expect(byName(card.fields, 'label').source).toBe('authored')
    expect(byName(card.fields, 'label').value).toBe('L')
  })

  it('keeps a render-uncoercible value raw (byte-for-byte with the plate)', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
count: "not-a-number"
~~~
`
    // A value the render coercion cannot conform is kept raw and Authored,
    // exactly as compile_data leaves it: the error surfaces via validate(),
    // not this view (which carries no diagnostics).
    const row = byName(quill._resolve(Document.fromMarkdown(md)).main.fields, 'count')
    expect(row.source).toBe('authored')
    expect(row.value).toBe('not-a-number')
    expect('diagnostics' in row).toBe(false)
  })

  it('result is JSON.stringify-able', () => {
    const quill = buildQuill()
    const md = `~~~card-yaml
$quill: field_states_test
$kind: main
title: T
~~~
`
    const states = quill._resolve(Document.fromMarkdown(md))
    const round = JSON.parse(JSON.stringify(states))
    expect(byName(round.main.fields, 'title').value).toBe('T')
  })
})


// ---------------------------------------------------------------------------
// reader.values / writer.setValues, through their `_readerValues` /
// `_setValues` ABI: the values form
// ---------------------------------------------------------------------------
//
// The shape's semantics (the per-axis rule, the walk's per-leaf projection, the
// write guard) are core's (`core/src/quill/values.rs`, `core/src/writer.rs`).
// At this boundary the questions are narrower: does the shape cross as a plain
// JS object with every axis present, does `null` cross both ways, does the
// write reach core at both scopes, and do refusals arrive as diagnostics
// carrying their own `path`.

describe('reader.values / writer.setValues', () => {
  const QUILL_YAML = `quill:
  name: values_test
  version: "1.0"
  backend: typst
  description: Values form coverage

main:
  fields:
    subject:
      type: richtext
      inline: true
    note:
      type: plaintext
    qty:
      type: integer
      default: 1
    paragraphs:
      type: array
      items:
        type: richtext

card_kinds:
  line_item:
    fields:
      desc:
        type: richtext
        inline: true
      qty:
        type: integer
`

  const buildQuill = () =>
    Quill.fromTree(makeQuill({ name: 'values_test', quillYaml: QUILL_YAML }))

  const MD = `~~~card-yaml
$quill: values_test
$kind: main
$ext:
  app:
    k: 1
subject: Hello **world**
note: a *literal* line
paragraphs:
  - Para **one**
~~~

Body prose.

~~~card-yaml
$kind: line_item
desc: Widget __A__
qty: "3"
~~~
Item note.
`

  it('reads every axis, content leaves as text at every depth, scalars as stored', () => {
    const quill = buildQuill()
    const v = quill.parse(MD)._readerValues(quill, {})

    expect(v.fields.subject).toBe('Hello **world**')
    expect(v.fields.note).toBe('a *literal* line')
    expect(v.fields.paragraphs).toEqual(['Para **one**'])
    expect('qty' in v.fields).toBe(false)
    expect(v.body).toBe('Body prose.')
    expect(v.ext).toEqual({ app: { k: 1 } })

    expect(v.cards).toEqual([
      {
        kind: 'line_item',
        fields: { desc: 'Widget **A**', qty: '3' },
        body: 'Item note.',
        ext: null,
      },
    ])
  })

  it('null crosses both ways: a missing $ext, a kindless card, a present-null field', () => {
    const quill = buildQuill()
    const doc = quill.parse(
      '~~~card-yaml\n$quill: values_test\n$kind: main\nsubject:\n~~~\n\n~~~card-yaml\nfoo: bar\n~~~\n',
    )
    const v = doc._readerValues(quill, {})
    expect(v.fields.subject).toBeNull()
    expect(v.ext).toBeNull()
    expect(v.cards[0].kind).toBeNull()
    expect(v.cards[0].fields).toEqual({ foo: 'bar' })
    expect(v.cards[0].ext).toBeNull()

    doc._setValues(quill, {}, { ext: { app: {} } })
    expect(doc._readerValues(quill, {}).ext).toEqual({ app: {} })
    doc._setValues(quill, {}, { ext: null })
    expect(doc._readerValues(quill, {}).ext).toBeNull()
  })

  it('writing back an unedited read changes no bytes', () => {
    const quill = buildQuill()
    const doc = quill.parse(MD)
    const before = doc.toStored()
    doc._setValues(quill, {}, doc._readerValues(quill, {}))
    expect(doc.toStored()).toBe(before)
  })

  it('an absent axis is untouched and a present one is replaced', () => {
    const quill = buildQuill()
    const doc = quill.parse(MD)
    doc._setValues(quill, {}, { fields: { subject: 'Goodbye *world*' } })

    const after = doc._readerValues(quill, {})
    expect(after.fields.subject).toBe('Goodbye *world*')
    expect('note' in after.fields).toBe(false)
    expect(after.body).toBe('Body prose.')
    expect(after.cards).toHaveLength(1)
    expect(after.ext).toEqual({ app: { k: 1 } })
  })

  it('the card scope reads and writes one slot', () => {
    const quill = buildQuill()
    const doc = quill.parse(MD)
    expect(doc._readerValues(quill, { card: 0 })).toEqual(doc._readerValues(quill, {}).cards[0])

    doc._setValues(quill, { card: 0 }, { fields: { desc: 'Gadget' } })
    expect(doc._readerValues(quill, { card: 0 }).fields).toEqual({ desc: 'Gadget' })
    expect(doc._readerValues(quill, {}).fields.subject).toBe('Hello **world**')

    expectEditCode(() => doc._setValues(quill, { card: 0 }, { fields: { bad: 1 } }), 'edit::unknown_field')
    expectEditCode(() => doc._readerValues(quill, { card: 7 }), 'edit::index_out_of_range')
    expectEditCode(() => doc._setValues(quill, { card: 7 }, {}), 'edit::index_out_of_range')
  })

  it('refusals arrive as diagnostics carrying their own path', () => {
    const quill = buildQuill()
    const doc = quill.parse(MD)
    const before = doc.toStored()
    let thrown
    try {
      doc._setValues(quill, {}, { fields: { nope: 'x' } })
    } catch (err) {
      thrown = err
    }
    expect(thrown, 'expected a throw, got none').toBeDefined()
    expect(thrown.diagnostics[0].code).toBe('edit::unknown_field')
    expect(thrown.diagnostics[0].path).toBe('main.nope')
    expect(doc.toStored()).toBe(before)
  })

  it('a malformed values object throws before anything is read', () => {
    const quill = buildQuill()
    const doc = quill.parse(MD)
    expect(() => doc._setValues(quill, {}, { feilds: {} })).toThrow()
  })
})
