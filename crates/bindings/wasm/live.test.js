/**
 * The document's Automerge history across the WASM boundary: `save` and
 * `fromBytes`, `heads`, `saveSince` and `merge`, and the sync protocol two
 * documents run through `generateSyncMessage` / `receiveSyncMessage`
 * (prose/canon/DOCUMENT_STORAGE.md).
 */
import { describe, it, expect } from 'vitest'
import { init } from '@quillmark-wasm/runtime'
import { expectEditCode } from './test-helpers.js'

const { Document, SyncState } = await init()

const parse = (body) => Document.parse(`~~~\n$quill: q\n~~~\n\n${body}\n`)

/** Run the sync protocol between `a` and `b` until neither has a message,
 * answering how many messages crossed. */
function sync(a, b, states = [new SyncState(), new SyncState()]) {
  const [toB, toA] = states
  let crossed = 0
  for (let i = 0; i < 16; i++) {
    const forB = a.generateSyncMessage(toB)
    if (forB) {
      b.receiveSyncMessage(toA, forB)
      crossed++
    }
    const forA = b.generateSyncMessage(toA)
    if (forA) {
      a.receiveSyncMessage(toB, forA)
      crossed++
    }
    if (!forA && !forB) return crossed
  }
  throw new Error('the sync did not settle')
}

const splice = (at, insert) => ({ steps: [{ splice: { at, delete: '', insert } }] })

describe('a document as Automerge bytes', () => {
  it('saves to bytes fromBytes reads back', () => {
    const doc = parse('Hello world')
    doc.apply({}, splice(5, ' brave'))
    const bytes = doc.save()
    expect(bytes).toBeInstanceOf(Uint8Array)

    const other = Document.fromBytes(bytes)
    expect(other.read({})).toEqual(doc.read({}))
    expect(other.heads()).toEqual(doc.heads())
    expect(other.toMarkdown()).toBe(doc.toMarkdown())
    const err = expectEditCode(() => Document.fromBytes(new Uint8Array([1, 2, 3])), 'edit::bad_live_bytes')
    expect(err.diagnostics[0].args.reason).toBe('decode')
  })

  it('merges the changes since heads into a peer', () => {
    const a = parse('Hello world')
    const b = Document.fromBytes(a.save())
    const heads = a.heads()
    a.apply({}, splice(5, ' brave'), { actor: 'ann' })
    b.apply({}, splice(11, '!'), { actor: 'b0' })

    b.merge(a.saveSince(heads))
    a.merge(b.save())
    expect(a.read({}).text).toBe('Hello brave world!')
    expect(b.read({})).toEqual(a.read({}))
    expect([...a.heads()].sort()).toEqual([...b.heads()].sort())
  })
})

describe('the sync protocol', () => {
  it('brings two documents from one save to one value', () => {
    const a = parse('😀 one two')
    const b = Document.fromBytes(a.save())
    a.apply({}, splice(3, 'X'), { actor: 'ann' })
    b.apply({}, splice(10, ' three'), { actor: 'b0' })

    const states = [new SyncState(), new SyncState()]
    expect(sync(a, b, states)).toBeGreaterThan(0)
    expect(a.read({}).text).toBe('😀 Xone two three')
    expect(b.read({})).toEqual(a.read({}))
    expect(sync(a, b, states)).toBe(0)
    expectEditCode(() => a.receiveSyncMessage(new SyncState(), new Uint8Array([1, 2, 3])), 'edit::bad_live_bytes')
  })

  it('keeps a sync state across encode and decode', () => {
    const a = parse('Hello world')
    const b = Document.fromBytes(a.save())
    const states = [new SyncState(), new SyncState()]
    sync(a, b, states)
    a.apply({}, splice(5, ' brave'), { actor: 'ann' })

    sync(a, b, states.map((s) => SyncState.decode(s.encode())))
    expect(b.read({}).text).toBe('Hello brave world')
    const err = expectEditCode(() => SyncState.decode(new Uint8Array([1, 2, 3])), 'edit::bad_live_bytes')
    expect(err.diagnostics[0].args.reason).toBe('decode')
  })
})
