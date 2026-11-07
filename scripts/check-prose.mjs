#!/usr/bin/env node
// Prose lint: enforces the shape prose/README.md specifies. Zero dependencies,
// no arguments.
//
// Every rule here is a gate, and every gate catches rot no reader would
// notice: a dead link, a reference that links out, or a second design doc
// growing beside the one. Taste is the writer's job and the reviewer's. A
// check that can only warn does not belong.
import { readdirSync, readFileSync, existsSync, statSync } from 'node:fs';
import { join, posix } from 'node:path';

const problems = [];
const fail = (file, msg) => problems.push(`${file}: ${msg}`);

const INVARIANTS = 'prose/INVARIANTS.md';
// What prose/ holds. A design doc beside INVARIANTS.md is the catalogue this
// layout exists to keep out.
const PROSE_ENTRIES = new Set(['README.md', 'INVARIANTS.md', 'references']);
// A relative markdown link target to a .md file (an outbound prose link).
const PROSE_LINK = /\]\((?!https?:)[^)]*\.md(?=[)#])/;
// A relative markdown link target of any kind.
const REL_LINK = /\]\((?!https?:|#)([^)\s]+)/g;

// Line budget. Past this a line is a paragraph crammed onto one line, and a
// one-word fix rewrites the whole of it in the diff. prose/README.md sets a
// tighter target for writers; only the outer bound is mechanical.
const HARD = 700;

const mdFiles = (dir) =>
  existsSync(dir) ? readdirSync(dir).filter((n) => n.endsWith('.md')).sort() : [];

// Every .md under `dir`, minus `docs/migrations/`: released guides are
// era-stamped, so no rule applies to them.
const walkDocs = (dir) =>
  !existsSync(dir)
    ? []
    : readdirSync(dir)
        .sort()
        .flatMap((name) => {
          const p = join(dir, name);
          if (statSync(p).isDirectory()) return name === 'migrations' ? [] : walkDocs(p);
          return name.endsWith('.md') ? [p] : [];
        });

// Lines outside fenced code, with their line numbers.
function unfenced(text) {
  const out = [];
  let fenced = false;
  text.split('\n').forEach((line, i) => {
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      return;
    }
    if (!fenced) out.push([i + 1, line]);
  });
  return out;
}

for (const name of readdirSync('prose').sort())
  if (!PROSE_ENTRIES.has(name)) fail(`prose/${name}`, `prose/ holds README.md, INVARIANTS.md and references/ only`);

if (!existsSync(INVARIANTS)) fail(INVARIANTS, 'is missing');
else {
  const text = readFileSync(INVARIANTS, 'utf8');
  if (!text.startsWith('# ')) fail(INVARIANTS, 'line 1 is not a `# Title`');
  // A rule links a neighbour's fact rather than copying it, which holds only
  // while the link resolves.
  const body = unfenced(text).map(([, l]) => l).join('\n');
  for (const [, target] of body.matchAll(REL_LINK)) {
    const resolved = posix.normalize(join('prose', target.split('#')[0]));
    if (!existsSync(resolved)) fail(INVARIANTS, `link target \`${target}\` does not resolve`);
  }
}

for (const name of mdFiles('prose/references')) {
  const file = join('prose/references', name);
  const m = readFileSync(file, 'utf8').match(PROSE_LINK);
  if (m) fail(file, `links to another prose doc (\`${m[0]}\`): references are self-contained`);
}

// Prose lines only: no table rows, since a table row is a record, not a
// sentence, and rewrapping one is worse than leaving it long.
for (const file of [INVARIANTS, ...walkDocs('docs')].filter(existsSync))
  for (const [n, line] of unfenced(readFileSync(file, 'utf8')))
    if (!/^\s*\|/.test(line) && line.length > HARD)
      fail(file, `line ${n} is ${line.length} chars (max ${HARD}): one claim per sentence; split the clauses into bullets or a table`);

if (problems.length) {
  for (const p of problems) console.error(`check-prose: ${p}`);
  process.exit(1);
}
console.log('check-prose: prose OK');
