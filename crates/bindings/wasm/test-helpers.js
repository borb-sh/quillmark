import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { expect } from 'vitest'

const enc = new TextEncoder()

const PKG_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'pkg')

/**
 * Instantiate a generated build the suites drive directly. The public root has
 * `init` for this; these are internal artifacts, so they instantiate through
 * wasm-bindgen's own `initSync` off the bytes on disk.
 *
 * @param {{ initSync: (arg: { module: BufferSource }) => unknown }} mod the build's module namespace
 * @param {string} variant its directory under `pkg/`: `core` or `render`
 */
export function initBuildSync(mod, variant) {
  mod.initSync({ module: readFileSync(join(PKG_DIR, variant, 'wasm_bg.wasm')) })
}

/**
 * A `CardInput` from a kind, a flat field map and a body — the shape a host
 * writes for `insertCard`. `payloadItems` is the wire's own form: one
 * `{ type: 'field', key, value }` per field, in insertion order.
 *
 * @param {string} kind
 * @param {Record<string, unknown>} [fields]
 * @param {string} [body]
 */
export const makeCard = (kind, fields = {}, body = '') => ({
  kind,
  payloadItems: Object.entries(fields).map(([key, value]) => ({ type: 'field', key, value })),
  body,
})

/** The plate the suites' `test_quill` renders. */
export const TEST_PLATE = `#import "@local/quillmark-helper:0.2.0" as qm
#let title = qm.data.title
#let body = qm.data.at("$body")

= #title

#body`

/** The document the suites' `test_quill` parses. */
export const TEST_MARKDOWN = `~~~card-yaml
$quill: test_quill
$kind: main
title: Test Document
author: Test Author
~~~

# Hello World

This is a test document.`

/** A `test_quill` document with one `note` card. */
export const MD_WITH_CARD = `~~~card-yaml
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

/** An ES class, not a plain function. */
export const isClass = (v) => typeof v === 'function' && /^class[\s{]/.test(String(v))

/** The value `call` threw, `undefined` if it returned. */
export const caughtFrom = (call) => {
  try {
    call()
  } catch (e) {
    return e
  }
}

/**
 * Invoke `fn`, expect it to throw, and assert the thrown error's primary
 * diagnostic carries the namespaced `edit::*` `code`. Mutator identity rides on
 * `diagnostics[0].code`, not on message text; see prose/canon/ERROR.md.
 */
export function expectEditCode(fn, code) {
  let thrown
  try {
    fn()
  } catch (err) {
    thrown = err
  }
  expect(thrown, 'expected a throw, got none').toBeDefined()
  expect(thrown.diagnostics[0].code).toBe(code)
}

// Minimal font shipped with quillmark fixtures, loaded once. The Typst world
// rejects compilation when no fonts are present, so every test quill needs at
// least one: quills are responsible for shipping their own fonts, since
// quillmark-typst embeds no default fallback.
const __dirname = dirname(fileURLToPath(import.meta.url))
const TEST_FONT_PATH = join(
  __dirname,
  '../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/CopperplateCC/CopperplateCC-Heavy.otf',
)
const TEST_FONT_BYTES = new Uint8Array(readFileSync(TEST_FONT_PATH))

export function makeQuill({
  name = 'test_quill',
  plate = '#import "@local/quillmark-helper:0.2.0" as qm\n= Test',
  quillYaml,
} = {}) {
  const yaml = quillYaml ?? `quill:
  name: ${name}
  version: "1.0.0"
  backend: typst
  description: Test quill for smoke tests

typst:
  plate_file: plate.typ
`
  return new Map([
    ['Quill.yaml', enc.encode(yaml)],
    ['plate.typ', enc.encode(plate)],
    ['assets/fonts/test.otf', TEST_FONT_BYTES],
  ])
}

// The hand-authored `sample_form` fixture: an `acroform`-backend quill shipping a
// stripped background (`form.pdf`) and a value-free field spec (`Form.yaml`).
// Loaded as a tree so the canvas tests can drive the acroform backend
// (which rasterizes its stamped PDF) exactly like a typst quill.
const SAMPLE_FORM_DIR = join(__dirname, '../../fixtures/resources/quills/sample_form/0.1.0')

export function makeSampleFormQuill() {
  return new Map([
    ['Quill.yaml', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'Quill.yaml')))],
    ['form.pdf', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'form.pdf')))],
    ['Form.yaml', new Uint8Array(readFileSync(join(SAMPLE_FORM_DIR, 'Form.yaml')))],
  ])
}

// A filled sample_form document: binds the FullName text field (among others), so
// the raster carries visible field-value ink.
export const SAMPLE_FORM_MARKDOWN = `~~~
$quill: sample_form
$kind: main
full_name: Ada Lovelace
comments:
  - First comment line.
  - Second comment line.
agree: true
favorite_color: green
~~~
`
