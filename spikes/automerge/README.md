# Automerge spike

Can a content value ([CONTENT.md](../../prose/canon/CONTENT.md)) live in an Automerge document with no loss, take every `Change` as Automerge operations, and converge under concurrent edits through `repair`? This crate is the experiment: its own workspace, outside the build, against `automerge` 0.12.

`cargo test` runs the three checks; `SESSIONS=1500 cargo test --release --test parity convergence_report -- --nocapture` runs a longer convergence report.

## The mapping

| Content | Automerge |
|---|---|
| `text` | A Text object, opening with a block marker for block 0, so value offset `n` is index `n + 1` |
| A `\n` and its block record | A block marker: `id`, `kind` (kind and attrs as one JSON value), `path` |
| A `U+FFFC` and its island | A marker in the text: `id`, `type`, `props` as Automerge maps and lists, `slots` a map of values of this same shape |
| `containers` | A map of container records by id |
| `marks` | Automerge marks, `ExpandMark::None`; `link` and `font_family` carry their attr as the mark value, an `x-` mark with attrs folds them into its name |
| A range anchor | A mark named `@` and its id |
| A point anchor | A cursor on the character after it |
| `codec` | A root key |

Reading a document produces the raw value `repair` takes, so a merged state reads as one valid value with no change to the model. Writing lowers each step's text as splices and markers, then sets each record, mark, anchor and island where it differs from the value `apply` gave.

## Results

| Check | Result |
|---|---|
| Every generated value reads back as itself, also after save and load | 300 of 300 |
| Every generated change, written as Automerge ops, reads back as `apply` gives, with no text diff fallback | 300 of 300 |
| Three peers editing at once, merging at random, read one value, also after save and load | 7,500 of 7,500 sessions, over five seeds; about 13% of them wrote a repair back |

The generators are `crates/content/tests/change_laws.rs`'s, copied: headings, lists, quotes, code, tables, footnotes, math, `x-` blocks, marks and islands, anchors in text and in slots, and every step kind including table row and column edits and island moves.

## What it found

- **The model fits.** Nothing in the content model needed changing, and repair's raw form is exactly what a CRDT projection produces.
- **Automerge cursors are not reliable.** `get_cursor_position` debug-asserts that its fast and slow seeks agree, and they disagree on a cursor whose element was deleted; the slow seek (`at: Some(heads)`) then refuses the cursor as invalid. A cursor can also land on a mark's boundary op rather than a character. With cursors for range anchors, two peers holding the same changes read an anchor at different positions. Range anchors are therefore marks here; point anchors are still cursors.
- **Automerge marks once disagreed with their own bytes.** Under an earlier version of this crate, which committed inside reads and resolved cursors at heads, one session in 300 had a peer whose in-memory marks differed from the same document saved and loaded. The current crate has not reproduced it in 7,500 sessions, so it is unexplained rather than ruled out.
- **The tests build automerge without debug assertions** (`Cargo.toml`), since the cursor seek's assertion fires on ordinary edits. A release build behaves the same way.
- **Kind and attrs must be one value.** As separate keys, a concurrent kind change and attrs change merge into a heading with no level, which the raw decoder refuses.

## Gaps against today's model and collab

- A range anchor whose text is all deleted vanishes rather than reading as collapsed, so repair cannot report it as dropped.
- Two concurrent moves of one range anchor leave the hull of both ranges.
- A repair that inserts still needs one writer, as CONTENT.md § "Merge repair" says; the report counts edits a peer skipped waiting for peer 0.
- A write commits more than one Automerge change per `Change`.
- Speed is unmeasured: a write builds per-character mark tables and scans the text for markers.

Against `@quillmark/collab` on Yjs, the mapping carries what collab drops: `x-` marks and anchors sync, a table's props merge key by key so a row insert and a column insert both land, and a write is its steps rather than a diff of values.
