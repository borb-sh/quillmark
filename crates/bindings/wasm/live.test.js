/**
 * A value's live Automerge document across the WASM boundary: `save` and
 * `load`, and the sync protocol two documents run through
 * `generateSyncMessage` / `receiveSyncMessage` (prose/canon/CONTENT.md
 * § "Live state and merge").
 */
import { describe, it, expect } from 'vitest'
import { init } from '@quillmark-wasm/runtime'
import { expectEditCode } from './test-helpers.js'

const { Document, SyncState, importMarkdown } = await init()

const parse = (body) => Document.parse(`~~~\n$quill: q\n~~~\n\n${body}\n`)

/** Run the sync protocol between `a` and `b` at `addr` until neither has a
 * message, answering the receipts each received. */
function sync(a, b, addr = {}) {
  const [toB, toA] = [Document.syncState(), Document.syncState()]
  const receipts = { a: [], b: [] }
  for (let round = 0; round < 16; round++) {
    const forB = a.generateSyncMessage(addr, toB)
    if (forB) receipts.b.push(b.receiveSyncMessage(addr, toA, forB))
    const forA = b.generateSyncMessage(addr, toA)
    if (forA) receipts.a.push(a.receiveSyncMessage(addr, toB, forA))
    if (!forA && !forB) return receipts
  }
  throw new Error('the sync did not settle')
}

describe('the live document', () => {
  it('saves to bytes another document loads, a base read before the load stale', () => {
    const doc = parse('Hello world')
    doc.apply({}, { steps: [{ splice: { at: 5, delete: '', insert: ' brave' } }] })
    const bytes = doc.save({})
    expect(bytes).toBeInstanceOf(Uint8Array)

    const other = parse('else')
    const before = other.hash({})
    const receipt = other.load({}, bytes)
    expect(other.read({})).toEqual(doc.read({}))
    expect(receipt.hash).toBe(doc.hash({}))
    expect(receipt.applied.steps.length).toBeGreaterThan(0)
    expectEditCode(() => other.apply({}, { steps: [] }, { base: before }), 'edit::stale_base')

    expectEditCode(() => other.load({}, new Uint8Array([1, 2, 3])), 'edit::bad_live_bytes')
    expectEditCode(() => other.receiveSyncMessage({}, Document.syncState(), new Uint8Array([1, 2, 3])), 'edit::bad_live_bytes')
    expect(other.read({})).toEqual(doc.read({}))
  })

  it('brings two documents parsed from one value to one value, receipts in UTF-16', () => {
    const a = parse('😀 one two')
    const b = parse('😀 one two')
    a.apply({}, { steps: [{ splice: { at: 3, delete: '', insert: 'X' } }] }, { actor: 'ann' })
    b.apply({}, { steps: [{ splice: { at: 10, delete: '', insert: ' three' } }] }, { actor: 'bo' })

    const receipts = sync(a, b)
    expect(a.read({})).toEqual(b.read({}))
    expect(a.read({}).text).toBe('😀 Xone two three')
    expect(a.hash({})).toBe(b.hash({}))
    const splices = receipts.b.flatMap((r) => r.applied.steps).map((s) => s.splice)
    expect(splices.map((s) => s.at)).toEqual([3])
    const again = sync(a, b)
    expect([...again.a, ...again.b].flatMap((r) => r.applied.steps)).toEqual([])
  })

  it('names in the receipt an anchor a merge took out, and a field syncs as the body does', () => {
    const a = parse('Hello brave new world')
    a.apply({}, { steps: [{ anchor: { id: 'c-1', start: 6, end: 15 } }] })
    const b = parse('else')
    b.load({}, a.save({}))
    a.apply({}, { steps: [{ splice: { at: 6, delete: 'brave ', insert: '' } }] }, { actor: 'ann' })
    b.apply({}, { steps: [{ splice: { at: 11, delete: ' new', insert: '' } }] }, { actor: 'bo' })

    const receipts = sync(a, b)
    expect(a.read({}).text).toBe('Hello  world')
    expect(b.read({})).toEqual(a.read({}))
    for (const side of [receipts.a, receipts.b]) {
      expect(side.flatMap((r) => r.droppedAnchors)).toEqual([expect.objectContaining({ id: 'c-1', path: 'main.body' })])
    }

    a.replace({ field: 'note' }, importMarkdown('a note').content, { actor: 'ann' })
    sync(a, b, { field: 'note' })
    expect(b.read({ field: 'note' })).toEqual(a.read({ field: 'note' }))
    expect(Document.syncState()).toBeInstanceOf(SyncState)
  })

  it('refuses bytes or a message from the live document of another value, however equal', () => {
    const two = Document.parse('~~~\n$quill: q\n~~~\n\n~~~card-yaml\n$kind: note\n$id: aaa\n~~~\n\nHello\n\n~~~card-yaml\n$kind: note\n$id: bbb\n~~~\n\nHello\n')
    const [aaa, bbb] = [{ card: 'aaa' }, { card: 'bbb' }]
    const err = expectEditCode(() => two.load(bbb, two.save(aaa)), 'edit::bad_live_bytes')
    expect(err.diagnostics[0].args.reason).toBe('address')

    const peer = Document.parse(two.toMarkdown())
    const [toPeer, toTwo] = [Document.syncState(), Document.syncState()]
    peer.receiveSyncMessage(aaa, toTwo, two.generateSyncMessage(bbb, toPeer))
    const reply = peer.generateSyncMessage(aaa, toTwo)
    const refused = expectEditCode(() => two.receiveSyncMessage(bbb, toPeer, reply), 'edit::bad_live_bytes')
    expect(refused.diagnostics[0].args.reason).toBe('address')
  })
})
