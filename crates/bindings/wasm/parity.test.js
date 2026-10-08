/**
 * The parity corpus as the package ships it, `pkg/parity.json`: every entry
 * with a markdown spelling round-trips through the markdown codec a consumer
 * reaches, and an `annotated` read imports as its content without anchors
 * (prose/canon/PARITY.md § "The corpus").
 */
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { init } from '@quillmark-wasm/runtime'

const { importMarkdown, exportMarkdown } = await init()

const PKG_DIR = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'pkg')
const corpus = JSON.parse(readFileSync(join(PKG_DIR, 'parity.json'), 'utf8'))
const spelled = corpus.filter((entry) => entry.markdown !== null)
const annotated = corpus.filter((entry) => entry.annotated !== undefined)

describe('pkg/parity.json', () => {
  it('holds entries with a markdown spelling', () => {
    expect(spelled.length).toBeGreaterThan(0)
  })

  for (const { name, markdown, content, signals } of spelled) {
    it(name, () => {
      const imported = importMarkdown(markdown)
      expect(imported.content).toEqual(content)
      expect(importMarkdown(exportMarkdown(content)).content).toEqual(content)
      expect(
        imported.warnings.map((w) => ({
          code: w.code,
          construct: w.args.construct,
          count: w.args.count,
        })),
      ).toEqual(
        signals.import.map((s) => ({ code: 'parse::dropped_construct', ...s })),
      )
    })
  }

  for (const { name, annotated: read, content } of annotated) {
    it(`${name}, annotated`, () => {
      const imported = importMarkdown(read)
      expect(imported.content).toEqual({
        ...content,
        marks: content.marks.filter((m) => m.type !== 'anchor'),
      })
      expect(imported.warnings).toEqual([])
    })
  }
})
