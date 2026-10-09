/**
 * The parity corpus, `crates/fixtures/resources/parity/parity.json`, through the
 * WASM binding's content doors: the markdown codec and a document's parse, the
 * authored and op-wire lanes, storage, revise and rebase, and the annotated read
 * (prose/canon/PARITY.md § "The corpus").
 */
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { init } from '@quillmark-wasm/runtime'

const { Document, importMarkdown, exportMarkdown, rebase } = await init()

const CORPUS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'fixtures', 'resources', 'parity', 'parity.json')
const corpus = JSON.parse(readFileSync(CORPUS, 'utf8'))
const FRONTMATTER = '~~~\n$quill: table_demo@0.1.0\n$kind: main\ntitle: Parity\n~~~\n'
const SLOT = '￼'

const blank = () => Document.fromMarkdown(FRONTMATTER)

const holding = (content) => {
  const doc = blank()
  doc.overwrite({}, content)
  return doc
}

const anchors = (content) => content.marks.filter((m) => m.type === 'anchor')

const unanchored = (content) => ({ ...content, marks: content.marks.filter((m) => m.type !== 'anchor') })

const dropped = (warnings) =>
  warnings.map((w) => ({ code: w.code, path: w.path, construct: w.args.construct, count: w.args.count }))

const expected = (signals, path) =>
  signals.import.map((s) => ({ code: 'parse::dropped_construct', path, ...s }))

/** A revise lands on `reimports` plus every anchor `content` held. */
const expectRevised = (revised, content, reimports) => {
  expect(unanchored(revised)).toStrictEqual(unanchored(reimports))
  expect(anchors(revised)).toStrictEqual(anchors(content))
}

/**
 * The op-wire bundle that authors `content` onto an empty field: the text by
 * `delta`, each island by an `insert` at its slot, each line's kind, containers
 * and `continues` by line ops, and every mark by an `add`.
 */
const bundle = (content) => {
  const chars = [...content.text]
  const prose = chars.filter((c) => c !== SLOT).join('')
  const slots = chars.flatMap((c, at) => (c === SLOT ? [at] : []))
  return {
    delta: { ops: prose ? [{ insert: prose }] : [] },
    islandOps: slots.map((at, i) => ({ ...content.islands[i], op: 'insert', at })),
    lineOps: content.lines.flatMap((l, line) => [
      { op: 'setKind', line, kind: l.kind, ...(l.attrs && { attrs: l.attrs }) },
      { op: 'setContainers', line, containers: l.containers },
      ...(l.continues === undefined ? [] : [{ op: 'setContinues', line, continues: l.continues }]),
    ]),
    markOps: content.marks.map((m) => ({ ...m, op: 'add' })),
  }
}

describe('parity.json', () => {
  it('holds spelled, unspelled and annotated entries', () => {
    expect(corpus.some((e) => e.markdown !== null)).toBe(true)
    expect(corpus.some((e) => e.markdown === null)).toBe(true)
    expect(corpus.some((e) => e.annotated !== undefined)).toBe(true)
  })

  for (const entry of corpus) {
    const { name, markdown, content, signals } = entry
    const reimports = entry.reimports ?? content

    describe(name, () => {
      if (markdown !== null) {
        it('imports to its content, warning its signals', () => {
          const imported = importMarkdown(markdown)
          expect(imported.content).toStrictEqual(content)
          expect(dropped(imported.warnings)).toStrictEqual(expected(signals, undefined))

          const doc = Document.fromMarkdown(`${FRONTMATTER}\n${markdown}\n`)
          expect(doc.main.body).toStrictEqual(content)
          expect(dropped(doc.warnings)).toStrictEqual(expected(signals, 'main.body'))
        })
      }

      it('lands through overwrite, applyChange and storage', () => {
        const doc = holding(content)
        expect(doc.main.body).toStrictEqual(content)
        expect(Document.fromStored(doc.toStored()).main.body).toStrictEqual(content)

        const applied = blank()
        applied.applyChange({}, bundle(content))
        expect(applied.main.body).toStrictEqual(content)
      })

      it('exports markdown that re-imports, warning nothing', () => {
        expect(importMarkdown(exportMarkdown(content))).toStrictEqual({ content: reimports, warnings: [] })
      })

      it('revises and rebases keeping its anchors, warning nothing', () => {
        const md = exportMarkdown(content)
        const doc = holding(content)
        expect(doc.revise({}, md).warnings).toStrictEqual([])
        expectRevised(doc.main.body, content, reimports)

        const rebased = rebase(content, md)
        expect(rebased.warnings).toStrictEqual([])
        expectRevised(rebased.content, content, reimports)
      })

      if (entry.annotated !== undefined) {
        it('reads annotated, imports without its anchors and revises keeping them', () => {
          const doc = holding(content)
          const read = doc.toAnnotatedMarkdown()
          expect(read.markdown).toContain(entry.annotated)
          expect(read.anchors.map((a) => [a.id, a.path])).toStrictEqual(
            anchors(content).map((m) => [m.attrs.id, 'main.body']),
          )

          expect(importMarkdown(entry.annotated)).toStrictEqual({ content: unanchored(content), warnings: [] })
          expect(doc.revise({}, entry.annotated).warnings).toStrictEqual([])
          expectRevised(doc.main.body, content, reimports)
        })
      }
    })
  }
})
