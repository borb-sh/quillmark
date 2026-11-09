/**
 * Block ops addressed by id across the boundary: offsets in UTF-16, ids held
 * across writes, a retired id refused.
 */
import { describe, it, expect } from 'vitest'
import * as core from '@quillmark-wasm/core'
import { Document } from '@quillmark-wasm/core'
import { initBuildSync } from './test-helpers.js'

initBuildSync(core, 'core')

const SRC = '~~~\n$quill: q\n~~~\n\n# Title 🎉\n\nAlpha **one**.\n\n- item a\n- item b\n'

describe('block ops', () => {
  it('reads blocks with UTF-16 ranges and writes by id', () => {
    const doc = Document.parse(SRC)
    const blocks = doc.blocks({})
    const text = doc.read({}).text
    expect(blocks.map((b) => text.slice(b.start, b.end))).toEqual(['Title 🎉', 'Alpha one.', 'item a', 'item b'])
    const [title, alpha, a, b] = blocks.map((x) => x.id)
    const r = doc.applyBlocks({}, [
      { op: 'replace', id: alpha, markdown: 'Alpha _two_.' },
      { op: 'insertAfter', id: b, markdown: 'item c' },
      { op: 'delete', id: a },
    ])
    const after = doc.blocks({})
    expect(after.map((x) => x.id).slice(0, 3)).toEqual([title, alpha, b])
    expect(r.opened[1]).toEqual([after[3].id])
    expect(after.map((x) => x.markdown)).toEqual(['# Title 🎉', 'Alpha *two*.', '- item b', '- item c'])
    expect(() => doc.applyBlocks({}, [{ op: 'delete', id: a }])).toThrow()
  })
})
