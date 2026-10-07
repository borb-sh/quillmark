#!/usr/bin/env node
// Canon spine lint: enforces the doc spine and the link invariants specified
// in prose/README.md. Zero dependencies, no arguments.
//
// Every rule here is a gate, and every gate catches a dead link or a dead
// anchor: rot no reader would notice. Taste is the writer's job and the
// reviewer's. A check that can only warn does not belong.
import { readdirSync, readFileSync, existsSync, statSync } from 'node:fs';
import { join, posix } from 'node:path';

const problems = [];
const fail = (file, msg) => problems.push(`${file}: ${msg}`);

// A file path (a slashed token with a dotted basename) inside an anchor.
// Keys on path shape, not an extension list, so a new file type can't slip past.
const FILE_IN_ANCHOR = /[\w-]+\/[\w/-]*\.[a-z0-9]+\b/;
// A relative markdown link target to a .md file (an outbound prose link).
const PROSE_LINK = /\]\((?!https?:)[^)]*\.md(?=[)#])/;
// A backticked slashed token: an anchor's folder reference.
const ANCHOR_PATH = /`([\w.-]+\/[\w./-]*)`/g;
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

// Prose lines only: no fenced code, no table rows (a table row is a record, not
// a sentence, and rewrapping one is worse than leaving it long).
function proseLines(text) {
  const out = [];
  let fenced = false;
  text.split('\n').forEach((line, i) => {
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      return;
    }
    if (fenced || /^\s*\|/.test(line)) return;
    out.push([i + 1, line]);
  });
  return out;
}

for (const name of mdFiles('prose/canon')) {
  const file = join('prose/canon', name);
  const text = readFileSync(file, 'utf8');
  const lines = text.split('\n');

  if (name === 'INDEX.md') continue; // the index has no spine

  if (!lines[0]?.startsWith('# ')) fail(file, 'line 1 is not a `# Title`');

  // Anchor blockquote: contiguous `>` lines from line 3. Only Implementation
  // lines carry a path, so the file check scans the whole quote.
  if (!lines[2]?.startsWith('> ')) {
    fail(file, 'line 3 is not the anchor blockquote');
  } else {
    const quote = [];
    for (let i = 2; i < lines.length && lines[i].startsWith('>'); i++) quote.push(lines[i]);
    const impl = quote.filter((l) => l.startsWith('> **Implementation**:'));
    if (!impl.length) fail(file, 'anchor blockquote has no `> **Implementation**:` line');
    const m = quote.join('\n').match(FILE_IN_ANCHOR);
    if (m) fail(file, `Implementation anchor names a file (\`${m[0]}\`): anchors point at folders or modules`);

    // An anchor that no longer resolves is the rot the folder rule exists to
    // prevent; it only prevents it if something checks.
    for (const [, p] of impl.join('\n').matchAll(ANCHOR_PATH))
      if (!existsSync(p)) fail(file, `Implementation anchor \`${p}\` does not exist`);
  }

  const firstH2 = lines.find((l) => l.startsWith('## '));
  if (firstH2 !== '## TL;DR') fail(file, `first section is \`${firstH2 ?? '(none)'}\`: canon docs open with \`## TL;DR\``);
}

// INDEX is the only entry point canon promises, so a page missing from it is
// unreachable.
if (existsSync('prose/canon/INDEX.md')) {
  const index = readFileSync('prose/canon/INDEX.md', 'utf8');
  for (const name of mdFiles('prose/canon')) {
    if (name === 'INDEX.md') continue;
    if (!index.includes(`(${name})`)) fail('prose/canon/INDEX.md', `does not link \`${name}\`: every canon page is reachable from the index`);
  }
}

// A canon page links a neighbour's fact rather than copying it, which holds
// only while the link resolves. Fenced code is not a link.
for (const name of mdFiles('prose/canon')) {
  const file = join('prose/canon', name);
  let fenced = false;
  const text = readFileSync(file, 'utf8')
    .split('\n')
    .filter((line) => {
      const fence = /^\s*(```|~~~)/.test(line);
      if (fence) fenced = !fenced;
      return !fence && !fenced;
    })
    .join('\n');
  for (const [, target] of text.matchAll(REL_LINK)) {
    const resolved = posix.normalize(join('prose/canon', target.split('#')[0]));
    if (!existsSync(resolved)) fail(file, `link target \`${target}\` does not resolve`);
  }
}

for (const name of mdFiles('prose/references')) {
  const file = join('prose/references', name);
  const m = readFileSync(file, 'utf8').match(PROSE_LINK);
  if (m) fail(file, `links to another prose doc (\`${m[0]}\`): references are self-contained`);
}

for (const file of [...mdFiles('prose/canon').map((n) => join('prose/canon', n)), ...walkDocs('docs')])
  for (const [n, line] of proseLines(readFileSync(file, 'utf8')))
    if (line.length > HARD) fail(file, `line ${n} is ${line.length} chars (max ${HARD}): one claim per sentence; split the clauses into bullets or a table`);

if (problems.length) {
  for (const p of problems) console.error(`check-canon: ${p}`);
  process.exit(1);
}
console.log('check-canon: canon spine OK');
