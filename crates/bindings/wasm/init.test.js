/**
 * The initialization contract: the gate is the only door to the core surface,
 * and it is idempotent, concurrency-safe, and identical in every environment.
 *
 * Its own file because the assertions are init-order-sensitive: the memo and
 * source-conflict cases need a module registry where nothing has initialized,
 * which vitest's per-file isolation provides.
 */
import { describe, it, expect } from 'vitest'
import { execFileSync } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { dirname, join } from 'node:path'
import { init, isQuillmarkError } from '@quillmark-wasm/runtime'
import * as runtime from '@quillmark-wasm/runtime'
import * as core from '@quillmark-wasm/core'

const PKG_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'pkg')

/** A parseable document: the root card-yaml block every document opens with. */
const MAIN_CARD_DOC = `~~~
$quill: test_quill
$kind: main
title: Init
~~~

# Hello`

/** Await `promise`, expect a rejection, and return its primary diagnostic. */
const rejectionFrom = async (promise) => {
  let thrown
  try {
    await promise
  } catch (err) {
    thrown = err
  }
  expect(thrown, 'expected a rejection, got none').toBeDefined()
  expect(isQuillmarkError(thrown)).toBe(true)
  return thrown.diagnostics[0]
}

// The precondition is carried by the shape of the surface, not by a note: a
// value that needs the instance is reachable only through the gate, so a call
// site cannot express the pre-init mistake and no load order can make one pass
// in dev and fail in production. These two cases are what hold that.
describe('the gated surface', () => {
  // Membership is DERIVED, so this is a gate and not a copied list: the core
  // build's exports minus its instantiation machinery, which `init` owns and no
  // consumer calls (`default` is wasm-bindgen's entry, `initSync` its sync twin,
  // `start` the start-section panic-hook install). A new core export that never
  // reaches CORE_SURFACE fails here rather than going missing from the public
  // API.
  const INSTANTIATION = ['default', 'initSync', 'start']
  const expected = Object.keys(core)
    .filter((name) => !INSTANTIATION.includes(name))
    .sort()

  it('is exactly the core build, minus what init owns', async () => {
    const surface = await init()
    expect(Object.keys(surface).sort()).toEqual(expected)
    // Verbatim: the same objects the core build exports, never wrappers.
    for (const name of expected) expect(surface[name]).toBe(core[name])
  })

  it('is reachable no other way', () => {
    // A static export of any of these would reopen the door the type system
    // cannot otherwise close.
    for (const name of expected) expect(runtime[name]).toBeUndefined()
  })
})

describe('init', () => {
  // Identity, not just equivalence: the memo is RETURNED, which is why `init`
  // is not declared `async` (an `async` body wraps a fresh promise per call).
  it('memoizes the promise, so concurrent callers share one instantiation', async () => {
    const a = init()
    const b = init()
    expect(a).toBe(b)
    const [x, y, z] = await Promise.all([a, b, init()])
    expect(y).toBe(x)
    expect(z).toBe(x)
  })

  it('resolves to a frozen surface: one consumer cannot reshape it for another', async () => {
    const surface = await init()
    expect(Object.isFrozen(surface)).toBe(true)
  })

  it('unlocks the sync surface', async () => {
    const { importMarkdown, Document } = await init()
    expect(importMarkdown('# Hi')).toBeDefined()
    expect(Document.fromMarkdown(MAIN_CARD_DOC)).toBeDefined()
  })

  // Silently ignoring a second, different source would leave a consumer
  // believing they chose the binary they are running.
  it('refuses a different source once initialized', async () => {
    const d = await rejectionFrom(init(new Uint8Array(8)))
    expect(d.code).toBe('runtime::init_conflict')
  })

  // The rule (delivery follows the function kind) at the one export that could
  // break it: a promise return type cannot declare a synchronous throw, so a
  // throw here would escape the `init(BYTES).catch(…)` the declaration invites.
  it('delivers the conflict as a rejection, not a synchronous throw', () => {
    /** @type {Promise<unknown> | undefined} */
    let returned
    expect(() => {
      returned = init(new Uint8Array(8))
    }).not.toThrow()
    expect(returned).toBeInstanceOf(Promise)
    returned.catch(() => {})
  })
})

// The package imports and runs under plain Node, with no bundler, no plugin,
// and no aliases. Nothing inside vitest can prove it, since vite resolves
// `#quillmark-env` and the asset URL itself, so this shells out to a bare node.
describe('plain Node, no bundler', () => {
  it('imports, initializes with no arguments, and works', () => {
    const runtimeUrl = pathToFileURL(join(PKG_DIR, 'runtime', 'runtime.js')).href
    const script = `
      const { init } = await import(${JSON.stringify(runtimeUrl)});
      const { importMarkdown } = await init();
      if (!importMarkdown('# Hello')) throw new Error('no content');
      console.log('OK');
    `
    const out = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    expect(out.trim()).toBe('OK')
  })

  it('reports a bad source as init_failed, and recovers on retry', () => {
    const runtimeUrl = pathToFileURL(join(PKG_DIR, 'runtime', 'runtime.js')).href
    const script = `
      const { init } = await import(${JSON.stringify(runtimeUrl)});
      let code;
      try { await init(new Uint8Array([0, 1, 2, 3])); }
      catch (e) { code = e.diagnostics?.[0]?.code; }
      if (code !== 'runtime::init_failed') throw new Error('got ' + code);
      // Self-heal: the failed attempt must not poison the retry.
      const { importMarkdown } = await init();
      if (!importMarkdown('# Hello')) throw new Error('no content');
      console.log('OK');
    `
    const out = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    expect(out.trim()).toBe('OK')
  })
})
