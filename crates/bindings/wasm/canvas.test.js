/**
 * Canvas preview. Vitest runs in Node with no DOM, so the canvas globals are
 * polyfilled far enough for wasm-bindgen's `instanceof` checks, capturing
 * `putImageData` into a buffer. Pixel-perfect correctness needs a real browser;
 * this catches a broken downcast, mis-sized buffer, swapped channels, a missing
 * demultiply, or a panic.
 */

import { describe, it, expect, beforeAll } from 'vitest'

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
    // Copy the byte view so the test can inspect pixels even if Rust later
    // reuses the underlying buffer.
    this.calls.push({
      width: img.width,
      height: img.height,
      data: new Uint8ClampedArray(img.data),
      dx,
      dy,
    })
  }
}

// In real browsers, OffscreenCanvasRenderingContext2D and
// CanvasRenderingContext2D do NOT share an inheritance chain: they're
// siblings. Defining the polyfill as an independent class (not a subclass)
// ensures the Rust-side `instanceof` dispatch actually exercises the
// second branch, instead of matching `CanvasRenderingContext2D` via
// inheritance.
class FakeOffscreenCanvasRenderingContext2D {
  constructor() {
    this.calls = []
    this.canvas = { width: 0, height: 0 }
  }
  putImageData(img, dx, dy) {
    this.calls.push({
      width: img.width,
      height: img.height,
      data: new Uint8ClampedArray(img.data),
      dx,
      dy,
    })
  }
}

beforeAll(() => {
  globalThis.ImageData = FakeImageData
  globalThis.CanvasRenderingContext2D = FakeCanvasRenderingContext2D
  globalThis.OffscreenCanvasRenderingContext2D = FakeOffscreenCanvasRenderingContext2D
})

const renderBuild = await import('@quillmark-wasm')
const { Quillmark, Quill, Document } = renderBuild
const { makeQuill, makeSampleFormQuill, SAMPLE_FORM_MARKDOWN, initBuildSync } = await import(
  './test-helpers.js'
)

initBuildSync(renderBuild, 'render')

const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Canvas Test
~~~

# Hello canvas
`

const TEST_PLATE = `#import "@local/quillmark-helper:0.1.0": data
= #data.title

#data.at("$body")`

function openQuill() {
  const engine = new Quillmark()
  const quill = Quill.fromTree(makeQuill({ name: 'test_quill', plate: TEST_PLATE }))
  return { engine, quill }
}

function openSession() {
  const { engine, quill } = openQuill()
  return engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
}

// An `array<richtext(inline)>` field: the backend regions each element on its
// own plate-space address (`references.<i>`), the shape whose translation the
// `arrayElementSession` test below pins.
const ARRAY_QUILL_YAML = `quill:
  name: array_quill
  version: "1.0.0"
  backend: typst
  description: Array-element addressing

typst:
  plate_file: plate.typ

main:
  fields:
    references:
      type: array
      items:
        type: richtext
        inline: true
`

const ARRAY_PLATE = `#import "@local/quillmark-helper:0.1.0": data
#for r in data.references [ #r \\ ]
`

const ARRAY_MARKDOWN = `~~~card-yaml
$quill: array_quill
$kind: main
references:
  - First reference line.
  - Second reference line.
~~~
`

function arrayElementSession() {
  const engine = new Quillmark()
  const quill = Quill.fromTree(
    makeQuill({ name: 'array_quill', plate: ARRAY_PLATE, quillYaml: ARRAY_QUILL_YAML }),
  )
  return engine.open(quill, Document.fromMarkdown(ARRAY_MARKDOWN))
}

/** Asserts a captured `putImageData` call's RGBA buffer carries both visible
 * ink (non-white, opaque pixels) and opaque background: catches a rasterizer
 * regression that wrote zeros, swapped channels, or skipped demultiply.
 * Shared by the typst and acroform paint tests below; the two rasterizers
 * differ, but this ink/opacity scan is the same check on either buffer. */
function expectInkAndOpaquePixels(call) {
  let inkPixels = 0
  let opaquePixels = 0
  for (let i = 0; i < call.data.length; i += 4) {
    const [r, g, b, a] = [call.data[i], call.data[i + 1], call.data[i + 2], call.data[i + 3]]
    if (a > 0 && (r < 250 || g < 250 || b < 250)) inkPixels++
    if (a === 255) opaquePixels++
  }
  expect(inkPixels).toBeGreaterThan(0)
  expect(opaquePixels).toBeGreaterThan(0)
}

describe('LiveSession canvas preview', () => {
  it('exposes pageCount, backendId, warnings, and pageSize on a Typst session', () => {
    const { engine, quill } = openQuill()

    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    expect(session.pageCount).toBeGreaterThan(0)
    expect(session.backendId).toBe('typst')
    expect(Array.isArray(session.warnings)).toBe(true)
    // Each entry crosses as a plain Diagnostic object, not a handle.
    for (const w of session.warnings) {
      expect(typeof w.severity).toBe('string')
      expect(typeof w.message).toBe('string')
    }

    const size = session.pageSize(0)
    expect(size.widthPt).toBeGreaterThan(0)
    expect(size.heightPt).toBeGreaterThan(0)
  })

  it('positionAt and locate cross the boundary as ContentHit / caret rect', () => {
    // Where the boxes land and which offsets they cover is geometry, owned by
    // `backends/typst/tests/content_regions.rs` and
    // `quillmark/tests/usaf_memo_regions_test.rs`. Here: the navigation verbs
    // reach JS with their declared shapes, addressed by DocPath, and a miss
    // maps `None` to `undefined` rather than throwing.
    const session = openSession()

    const bodyRegion = session.regions().find((r) => r.field === 'main.body' && r.span)
    expect(bodyRegion, 'a main.body segment region carries a span').toBeTruthy()
    const [x0, y0, x1, y1] = bodyRegion.rect
    const cy = (y0 + y1) / 2

    // Glyph layout decides which x lands on ink, so scan across the segment.
    let hit = null
    for (let f = 0.1; f <= 0.9 && !hit; f += 0.1) {
      hit = session.positionAt(bodyRegion.page, x0 + (x1 - x0) * f, cy)
    }
    expect(hit, 'positionAt resolves a point on the body ink').toBeTruthy()
    expect(hit.field).toBe('main.body')
    expect(typeof hit.pos).toBe('number')

    const caret = session.locate('main.body', hit.pos)
    expect(caret, 'locate reverses positionAt').toBeTruthy()
    expect(caret.field).toBe('main.body')
    expect(Array.isArray(caret.rect)).toBe(true)

    // A click far off any ink resolves to nothing.
    expect(session.positionAt(bodyRegion.page, 2, 2)).toBeFalsy()
  })

  it('addresses an array element as a bracketed DocPath index, both directions', () => {
    // The backend keys each `array<richtext>` element `references.<i>` in plate
    // space. The boundary translates that segment-wise, so a consumer sees the
    // one spelling `Diagnostic.path` also uses, and the `field`-taking verbs
    // accept it back.
    const session = arrayElementSession()

    const region = session.regions().find((r) => r.field === 'main.references[0]')
    expect(region, 'the first `references` element regions on a bracketed index').toBeTruthy()

    // The reverse leg: `fieldBoxes` and `locate` take that same string.
    const boxes = session.fieldBoxes('main.references[0]')
    expect(boxes.length, 'fieldBoxes answers on the element address').toBeGreaterThan(0)
    expect(boxes[0].field).toBe('main.references[0]')

    const caret = session.locate('main.references[0]', 0)
    expect(caret, 'locate answers on the element address').toBeTruthy()
    expect(caret.field).toBe('main.references[0]')

    // And the forward direction agrees: a point on the element's ink resolves
    // to the same string, so positionAt → locate composes.
    const [x0, y0, x1, y1] = region.rect
    const cy = (y0 + y1) / 2
    let hit = null
    for (let f = 0.1; f <= 0.9 && !hit; f += 0.1) {
      hit = session.positionAt(region.page, x0 + (x1 - x0) * f, cy)
    }
    expect(hit, 'positionAt resolves a point on the element ink').toBeTruthy()
    expect(hit.field).toBe('main.references[0]')
  })

  it('paint sizes the canvas backing store at scale pixels per point', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const scale = 1.5

    const ctx = new FakeCanvasRenderingContext2D()
    expect(session.paint(ctx, 0, scale)).toBeUndefined()

    expect(ctx.canvas.width).toBe(Math.round(widthPt * scale))
    expect(ctx.canvas.height).toBe(Math.round(heightPt * scale))

    expect(ctx.calls).toHaveLength(1)
    const call = ctx.calls[0]
    expect(call.dx).toBe(0)
    expect(call.dy).toBe(0)
    expect(call.width).toBe(ctx.canvas.width)
    expect(call.height).toBe(ctx.canvas.height)
    expect(call.data.length).toBe(call.width * call.height * 4)

    // The test plate renders a title heading, so the raster carries glyph ink
    // and an opaque page background.
    expectInkAndOpaquePixels(call)
  })

  it('paint defaults scale to 1', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)

    const ctx = new FakeCanvasRenderingContext2D()
    session.paint(ctx, 0)

    expect(ctx.canvas.width).toBe(Math.round(widthPt))
    expect(ctx.canvas.height).toBe(Math.round(heightPt))
  })

  it('also paints into an OffscreenCanvasRenderingContext2D', () => {
    const session = openSession()
    const ctx = new FakeOffscreenCanvasRenderingContext2D()
    session.paint(ctx, 0, 2)

    expect(ctx.calls).toHaveLength(1)
    expect(ctx.canvas.width).toBe(ctx.calls[0].width)
    expect(ctx.canvas.height).toBe(ctx.calls[0].height)
  })

  it('paint reduces the scale to keep the backing store within 16384 px a side', () => {
    const session = openSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const scale = (16384 / Math.max(widthPt, heightPt)) * 4

    const ctx = new FakeCanvasRenderingContext2D()
    session.paint(ctx, 0, scale)

    expect(Math.max(ctx.canvas.width, ctx.canvas.height)).toBeLessThanOrEqual(16384)
    // Reduced proportionally: the page keeps its aspect ratio.
    expect(ctx.canvas.width / ctx.canvas.height).toBeCloseTo(widthPt / heightPt, 2)
  })

  it('paint throws on a non-finite or non-positive scale', () => {
    const session = openSession()
    const ctx = new FakeCanvasRenderingContext2D()
    // An options object is refused rather than painted at the default.
    for (const scale of [0, -1, Number.NaN, Number.POSITIVE_INFINITY, { densityScale: 2 }]) {
      expect(() => session.paint(ctx, 0, scale)).toThrow(/scale/)
    }
  })

  it('throws an out-of-range error when paint is called with a bad page index', () => {
    const session = openSession()
    const ctx = new FakeCanvasRenderingContext2D()
    expect(() => session.paint(ctx, session.pageCount + 5)).toThrow(
      /out of range.*pageCount=/,
    )
  })
})

describe('LiveSession canvas preview (acroform backend)', () => {
  function openAcroformQuill() {
    const engine = new Quillmark()
    const quill = Quill.fromTree(makeSampleFormQuill())
    return { engine, quill }
  }

  function openAcroformSession() {
    const { engine, quill } = openAcroformQuill()
    return engine.open(quill, Document.fromMarkdown(SAMPLE_FORM_MARKDOWN))
  }

  it('reports page geometry for a acroform quill', () => {
    const { engine, quill } = openAcroformQuill()

    // The acroform backend rasterizes its stamped PDF, so `pageSize` answers
    // rather than throwing.
    const session = engine.open(quill, Document.fromMarkdown(SAMPLE_FORM_MARKDOWN))
    expect(session.pageCount).toBeGreaterThan(0)
    expect(session.backendId).toBe('acroform')

    const size = session.pageSize(0)
    expect(size.widthPt).toBeGreaterThan(0)
    expect(size.heightPt).toBeGreaterThan(0)
  })

  it('paint sizes the canvas at scale and bakes field-value ink into the raster', () => {
    const session = openAcroformSession()
    const { widthPt, heightPt } = session.pageSize(0)
    const scale = 1.5

    const ctx = new FakeCanvasRenderingContext2D()
    session.paint(ctx, 0, scale)

    // toBeCloseTo precision -1 tolerates the rasterizer's per-axis rounding.
    expect(ctx.canvas.width).toBeCloseTo(Math.round(widthPt * scale), -1)
    expect(ctx.canvas.height).toBeCloseTo(Math.round(heightPt * scale), -1)

    expect(ctx.calls).toHaveLength(1)
    const call = ctx.calls[0]
    expect(call.width).toBe(ctx.canvas.width)
    expect(call.height).toBe(ctx.canvas.height)
    expect(call.data.length).toBe(call.width * call.height * 4)

    // COMPLETE-RASTER contract: "Ada Lovelace" et al. are baked into the
    // widgets' appearance streams, so the buffer must carry non-white opaque ink
    // (field values + form lines) AND opaque page background. A backend that
    // returned only a blank background (no values) would fail the ink check.
    expectInkAndOpaquePixels(call)
  })
})

describe('LiveSession.update', () => {
  it('recompiles in place and reports the dirty page set', () => {
    const { engine, quill } = openQuill()
    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    const before = session.pageCount

    const cs = session.update(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Edited Title'))
    )
    expect(cs.pageCount).toBe(before)
    expect(cs.pageCount).toBe(session.pageCount)
    expect(cs.dirtyPages).toContain(0)

    // Identical re-update → nothing dirty.
    const cs2 = session.update(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Edited Title'))
    )
    expect(cs2.dirtyPages).toEqual([])

    // Reads serve the new compile: the repainted page differs.
    session.free()
  })

  it('keeps the last-good compile when update throws, and recovers', () => {
    const { engine, quill } = openQuill()
    const session = engine.open(quill, Document.fromMarkdown(TEST_MARKDOWN))
    const before = session.pageCount

    // A document for the wrong quill fails the $quill reference check.
    const wrong = Document.fromMarkdown(
      TEST_MARKDOWN.replace('$quill: test_quill', '$quill: other_quill')
    )
    expect(() => session.update(wrong)).toThrow()

    // Every read still serves the last-good compile.
    expect(session.pageCount).toBe(before)
    const ctx = new FakeCanvasRenderingContext2D()
    session.paint(ctx, 0)
    expect(ctx.canvas.width).toBeGreaterThan(0)
    expect(ctx.calls.length).toBe(1)

    // The session recovers on the next good update.
    const cs = session.update(
      Document.fromMarkdown(TEST_MARKDOWN.replace('Canvas Test', 'Recovered'))
    )
    expect(cs.pageCount).toBe(session.pageCount)
    session.free()
  })
})
