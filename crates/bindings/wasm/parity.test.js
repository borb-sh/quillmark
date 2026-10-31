/**
 * The parity corpus, `crates/fixtures/resources/parity/parity.json`, across the
 * WASM boundary: every entry's content crosses out of an import and a parse,
 * and in through `replace` and storage. The semantics are
 * `crates/quillmark/tests/parity.rs`'s (prose/canon/PARITY.md § "The corpus").
 */
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { init } from '@quillmark-wasm/runtime'

const { Document, importMarkdown } = await init()

const CORPUS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures', 'resources', 'parity', 'parity.json')
const corpus = JSON.parse(readFileSync(CORPUS, 'utf8'))
const FRONTMATTER = '~~~\n$quill: table_demo@0.1.0\n$kind: main\ntitle: Parity\n~~~\n'

const dropped = (warnings) =>
  warnings.map((w) => ({ code: w.code, path: w.path, construct: w.args.construct, count: w.args.count }))

// A write keeps the block records the value already holds, so `replace` lands
// every part of `content` but its block ids.
const unblocked = (c) => ({ ...c, blocks: c.blocks.map(({ id, ...block }) => block) })

const expected = (signals, path) =>
  signals.import.map((s) => ({ code: 'parse::dropped_construct', path, ...s }))

describe('parity.json', () => {
  for (const { name, markdown, content, signals } of corpus) {
    describe(name, () => {
      if (markdown !== null) {
        it('crosses out of an import and a parse, warnings and all', () => {
          const imported = importMarkdown(markdown)
          expect(imported.content).toStrictEqual(content)
          expect(dropped(imported.warnings)).toStrictEqual(expected(signals, undefined))

          const doc = Document.fromMarkdown(`${FRONTMATTER}\n${markdown}\n`)
          expect(doc.main.body).toStrictEqual(content)
          expect(dropped(doc.warnings)).toStrictEqual(expected(signals, 'main.body'))
        })
      }

      it('crosses in through replace and storage', () => {
        const doc = Document.fromMarkdown(FRONTMATTER)
        doc.replace({}, content)
        expect(unblocked(doc.read({}))).toStrictEqual(unblocked(content))

        const stored = JSON.parse(doc.toStored())
        stored.main.body = content
        const loaded = Document.fromStored(JSON.stringify(stored))
        expect(loaded.main.body).toStrictEqual(content)
        expect(Document.fromStored(loaded.toStored()).read({})).toStrictEqual(content)
      })
    })
  }
})
