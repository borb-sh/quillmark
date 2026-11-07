# prose/

Contributor-facing documentation, in two parts:

- **[`INVARIANTS.md`](INVARIANTS.md)**: the rules a change to Quillmark must keep, why each holds, and the designs already rejected.
- **`references/`**: authoritative, standalone specifications. Each reference is self-contained: it makes no outbound links to other prose docs, so it can be cited freely from user docs and code comments without forming a cycle.

[`docs/`](../docs/) is the published MkDocs site for quill authors and integrators: the task and the lookup, such as authoring a quill, persisting a document, or the codes a diagnostic carries.

## What earns a rule a place

`INVARIANTS.md` holds what the code cannot carry where it would be broken: a reason that sits in another crate, a storage promise, or a design that looks attractive until its cost shows up a release later. A rule earns a place only if a capable reader of the code would plausibly break it.

Everything else lives with the surface that owns it:

- what a module does, its API and its edge cases: rustdoc;
- what an author or integrator looks up, including any catalogue of codes, keys, flags or verbs: `docs/`;
- a grammar or a format: `references/`.

A rule names no function or file it does not need, because names rot while a rule holds. A change that breaks a rule updates it in the same commit, with the new reason.

## Line budget

A prose line caps at **700 characters** (the gate) and should stay under **300**, which is the target you write to. Past that, a line is a paragraph crammed onto one line: dense by the byte, unskimmable by the claim, and unreviewable in a diff, where a one-word fix rewrites the whole line. One claim per sentence. Fenced code and table rows are exempt: a table row is a record, not a sentence.

## Enforcement

`scripts/check-prose.mjs` runs in CI and gates the mechanical half of the above: that `prose/` holds this page, `INVARIANTS.md` and `references/` only, that links resolve, that references link out to no prose doc, and the 700-character cap over `INVARIANTS.md` and `docs/`. It only ever fails. The rest of this page is the writer's and the reviewer's.
