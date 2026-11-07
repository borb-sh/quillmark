#!/usr/bin/env node
// Canon spine lint: enforces the doc spine and the link invariants specified
// in prose/README.md, and that a CONTENT.md glossary term is used in no other
// sense in prose/canon. Zero dependencies, no arguments.
//
// Every rule here is a gate, and every gate catches rot no reader would
// notice: a dead link, a dead anchor or path, or a retired word back in canon. Taste
// is the writer's job and the reviewer's. A check that can only warn does not
// belong.
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
// A backticked repo path: slashed, with a dotted basename (FILE_IN_ANCHOR's shape).
const CODE_PATH = /`([\w.-]+(?:\/[\w.-]+)*\/[\w-]+\.[a-z0-9]+)`/g;

// Line budget. Past this a line is a paragraph crammed onto one line, and a
// one-word fix rewrites the whole of it in the diff. prose/README.md sets a
// tighter target for writers; only the outer bound is mechanical.
const HARD = 700;

// Every file under `dir`, build and dependency output aside.
const SKIP = new Set(['target', 'node_modules', '.venv', 'pkg']);
const walkFiles = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    if (SKIP.has(e.name) || e.name.startsWith('.')) return [];
    const p = posix.join(dir, e.name);
    return e.isDirectory() ? walkFiles(p) : [p];
  });

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

// A path canon names in prose is a pointer like a link, and rots like one. It
// resolves from the repo root or as the tail of a file under `crates/`, where
// canon names a path from its crate or its `src/`.
const crateFiles = existsSync('crates') ? walkFiles('crates') : [];
for (const name of mdFiles('prose/canon')) {
  const file = join('prose/canon', name);
  for (const [, p] of readFileSync(file, 'utf8').matchAll(CODE_PATH))
    if (!existsSync(p) && !crateFiles.some((f) => f.endsWith(`/${p}`)))
      fail(file, `path \`${p}\` resolves to no file, from the repo root or under \`crates/\``);
}

for (const name of mdFiles('prose/references')) {
  const file = join('prose/references', name);
  const m = readFileSync(file, 'utf8').match(PROSE_LINK);
  if (m) fail(file, `links to another prose doc (\`${m[0]}\`): references are self-contained`);
}

for (const file of [...mdFiles('prose/canon').map((n) => join('prose/canon', n)), ...walkDocs('docs')])
  for (const [n, line] of proseLines(readFileSync(file, 'utf8')))
    if (line.length > HARD) fail(file, `line ${n} is ${line.length} chars (max ${HARD}): one claim per sentence; split the clauses into bullets or a table`);

// The glossary in CONTENT.md pins each term to one sense, and its Retired
// column lists the quoted words and collocations that name a term's old or
// second sense. None of them appears in canon outside the glossary's own
// table; fenced code and inline code, where an identifier keeps its name, are
// excluded. A trailing `*` matches any word ending.
const GLOSSARY = 'prose/canon/CONTENT.md';
if (existsSync(GLOSSARY)) {
  const lines = readFileSync(GLOSSARY, 'utf8').split('\n');
  const start = lines.indexOf('## Glossary');
  if (start < 0) fail(GLOSSARY, 'has no `## Glossary` section');
  const head = lines.findIndex((l, i) => i > start && /^\|\s*Term\s*\|/.test(l));
  const rows = [];
  for (let i = head + 2; head > start && lines[i]?.startsWith('|'); i++) rows.push(i);
  const retired = rows.flatMap((i) => {
    const cells = lines[i].split('|').slice(1, -1);
    const term = cells[0].trim();
    return [...(cells.at(-1) ?? '').matchAll(/"([^"]+)"/g)].map(([, w]) => {
      const stem = w.replace(/\*$/, '').replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/[ -]/g, '[\\s-]+');
      return { term, word: w, re: new RegExp(`\\b${stem}${w.endsWith('*') ? '\\w*' : '\\b'}`, 'i') };
    });
  });
  if (head > start && !retired.length) fail(GLOSSARY, 'glossary lists no retired words');
  const skip = new Set(rows);
  for (const name of mdFiles('prose/canon')) {
    const file = join('prose/canon', name);
    let fenced = false;
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((line, i) => {
        if (/^\s*(```|~~~)/.test(line)) return void (fenced = !fenced);
        if (fenced || (file === GLOSSARY && skip.has(i))) return;
        const prose = line.replace(/`[^`]*`/g, '');
        for (const { term, word, re } of retired)
          if (re.test(prose)) fail(file, `line ${i + 1} uses "${word}", retired by the glossary's \`${term}\` (CONTENT.md § Glossary)`);
      });
  }
}

if (problems.length) {
  for (const p of problems) console.error(`check-canon: ${p}`);
  process.exit(1);
}
console.log('check-canon: canon spine OK');
