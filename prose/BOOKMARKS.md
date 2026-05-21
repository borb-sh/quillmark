# Bookmarks

Known gaps surfaced from code review. Each item is captured for later
attention — no commitment to a schedule. When an item turns into work, give
it a proposal in `prose/proposals/` and remove it from here.

## 1. No constructor for root cards

`crates/core/src/document/edit.rs:258` — `Card::new(kind)` constructs a
composable card. There is no equivalent for the document root; callers must
hand-build a `CardMetadata` and pass it to `Card::from_parts`, then assemble
via `Document::from_main_and_cards` which `debug_assert!`s the result.

No use case for programmatic root construction has surfaced yet (every
production path goes through `Document::from_markdown` or `from_json`), so
the asymmetry is a latent gap, not an active problem. Add a
`Card::new_root(quill: QuillReference)` plus a `Document::new(main, cards)`
the first time a caller actually needs to build a document from scratch.

## 2. `Document` clones on serialize

`crates/core/src/document/mod.rs:218` — `#[serde(into = "StoredDocument")]`
forces a by-value `From<Document>` conversion, which `.clone()`s every
field of every card. For "store this in a database" the per-write copy of
the whole document is wasted work.

`From<&Document> for DocumentV0_82_0` already exists; the path forward is
a manual `Serialize` impl on `Document` that converts by reference. The
deserialization side is unaffected (`try_from = "StoredDocument"` already
fits the `TryFrom<StoredDocument> for Document` shape).

## 3. `from_main_and_cards` overloads two intents

`crates/core/src/document/mod.rs:241` — `Document::from_main_and_cards`
accepts `warnings: Vec<Diagnostic>`, but every callsite that isn't the
parser passes `Vec::new()`. Storage-DTO reconstruction, programmatic
construction, and the parser all funnel through one constructor whose
warnings argument is meaningful in exactly one of those cases.

Split into `Document::from_parts(main, cards)` for the empty-warnings path
and `Document::from_parsed(main, cards, warnings)` (or keep the existing
name) for the parser. Makes the intent of each callsite obvious and stops
the DTO conversion from having to know it should pass empty warnings.
