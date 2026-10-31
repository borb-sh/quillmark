/**
 * The pre-init contract: what the gate does NOT cover. These cases drive the
 * whole static surface before anything initializes, so "the pre-init mistake is
 * not expressible" is a property under test.
 *
 * Its own file because the cases need a module registry where nothing has
 * initialized; sharing a file with a case that awaits `init` would make the
 * coverage positional, since every probe passes once core is live.
 */
import { describe, it, expect, beforeEach } from 'vitest'
import * as runtime from '@quillmark-wasm/runtime'
import * as core from '@quillmark-wasm/core'
import { isClass, caughtFrom } from './test-helpers.js'

// Derived at module scope, so a new static export joins every case below by
// existing. `init` is the door itself, and the only export that may reach wasm.
const EXEMPT = Object.keys(runtime).filter((name) => name !== 'init')
const CLASSES = EXEMPT.filter((name) => isClass(runtime[name]))
const staticsOf = (C) =>
  Object.getOwnPropertyNames(C).filter((n) => !['length', 'name', 'prototype'].includes(n))

// Importing this file at all is itself a case: module evaluation must touch no
// wasm, and runtime.js patches `Quill.prototype` at import.
describe('@quillmark/wasm: the static surface, before init', () => {
  // The premise, without which every case below passes vacuously. Core is a
  // `--target web` build: its classes export synchronously and its functions
  // throw until something instantiates it.
  const coreIsUninstantiated = () => {
    try {
      core.importMarkdown('# Hi')
      return false
    } catch {
      return true
    }
  }
  beforeEach(() =>
    expect(coreIsUninstantiated(), 'core is instantiated; this file proves nothing').toBe(true)
  )

  it('covers every static export', () => {
    // The loops below pass vacuously over an empty set.
    expect(EXEMPT.length).toBeGreaterThan(0)
    expect(CLASSES.length).toBeGreaterThan(0)
  })

  // What this asserts is that the call needs no wasm instance, not that the
  // argument is tolerated. `{}` is one every guard and `isQuillmarkError` takes.

  it('answers every non-class export without an instance', () => {
    for (const name of EXEMPT.filter((n) => !CLASSES.includes(n))) {
      const value = runtime[name]
      if (typeof value === 'function')
        expect(() => value({}), name).not.toThrow()
      else if (value !== null && typeof value === 'object')
        expect(Object.isFrozen(value), name).toBe(true)
    }
  })

  it('exposes no static method and no inherited member on any exempt class', () => {
    for (const name of CLASSES) {
      const C = runtime[name]
      // A static is the one member shape an argument cannot gate, so it is
      // forbidden outright rather than held to the handle rule: a future one
      // has to confront the exemption.
      expect(staticsOf(C), `${name} carries a static member`).toEqual([])
      // Both chains, or a base class hides members from the enumeration above.
      expect(Object.getPrototypeOf(C), `${name} extends a base`).toBe(Function.prototype)
      expect(Object.getPrototypeOf(C.prototype), `${name}.prototype extends a base`).toBe(
        Object.prototype
      )
    }
  })

  it('constructs or refuses in contract, reaching no wasm', () => {
    for (const name of CLASSES) {
      const caught = caughtFrom(() => new runtime[name]())
      // Pre-init a class's only reachable member is its constructor, so this is
      // the whole pre-init contract for a class. The writer/reader binds refuse
      // for want of a Quill; `Engine` and `LiveSession` take no handle and
      // succeed, which is the exemption executed rather than declared.
      if (caught !== undefined) {
        expect(runtime.isQuillmarkError(caught), `${name} threw outside the model`).toBe(true)
      }
    }
  })
})
