# Creating Quills

A quill is a document format: the fields a document fills in, the layout that prints them, and the assets the layout uses. Quills live in a **quiver**, a collection of quills kept as an npm project. This tutorial starts a quiver with one quill in it, opens it in **studio** (a form beside a live page), and then changes the quill one step at a time.

You need [Node.js](https://nodejs.org) 22 or later. Nothing else is installed by hand.

The quill here uses the `typst` backend, which draws each page from a Typst file you write. If you already have a PDF form to fill, the `acroform` backend fills that form instead: start at the [PDF Form Backend (acroform)](acroform-backend.md).

## 1. Start a quiver

```sh
npx quillkit@latest init my-quiver
cd my-quiver
npm install
npx quillkit studio
```

`init` writes a quiver holding one working quill, a letter. `studio` serves it and prints a local address. Open that address: the letter's form is on one side and its rendered page on the other. Edit a field and the page repaints. Click text on the page and the form jumps to the field that printed it.

Keep studio running. When you save a file in the quiver, studio repacks it; reload the page to see the change.

[quillkit & studio](quillkit.md) covers the tool itself. This page is about the quill.

## 2. What `init` wrote

```
my-quiver/
├── Quiver.yaml
├── package.json
├── .gitignore
├── .prettierignore
└── quills/
    └── letter/
        └── 0.1.0/
            ├── Quill.yaml
            ├── plate.typ
            └── example.md
```

- **`Quiver.yaml`** names the collection and describes it in one line.
- **`package.json`** pins `quillkit`, `@quillmark/quiver` and `@quillmark/wasm`. `npm test` runs `quillkit test`, which renders every quill in the quiver and fails on any error or warning.
- **`.prettierignore`** keeps prettier out of `quills/` ([why](quillkit.md#prettier)). Keep this file if your project runs prettier.
- **`quills/letter/0.1.0/`** is the quill. A quill lives at `quills/<name>/<version>/`, and both must match what its `Quill.yaml` declares.

The quill itself is three files:

- **`Quill.yaml`**: the quill's name and version, and the fields a document fills in.
- **`plate.typ`**: the [Typst](https://typst.app/docs/tutorial/) file that lays out the page. Each quill with the `typst` backend has a plate.
- **`example.md`**: a sample document. Studio opens it, and `quillkit test` renders it.

The next four steps read these files one idea at a time.

## 3. Fields

`Quill.yaml` opens with the quill's identity:

```yaml
quill:
  name: letter
  version: 0.1.0
  backend: typst
  description: A one-page letter with a date, a subject line and optional P.S. notes
```

`name`, `version`, `backend` and `description` are all required, and `name` is `snake_case`. Under `main.fields` are the fields of the document itself. The first two are plain text:

```yaml
main:
  description: A letter from one person to another.
  fields:
    sender:
      type: text
      description: Your name, as it appears in the letterhead and the signature.
    recipient:
      type: text
      description: Who the letter is addressed to.
```

`plate.typ` prints them:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#text(size: 16pt, weight: "bold", qm.ink(qm.data, "sender"))

#if qm.data.recipient != "" [Dear #qm.ink(qm.data, "recipient"),]

#qm.data.at("$body", default: "")
```

The plate reads the document through the helper package, imported `as qm`:

- **`qm.data`** holds the document's values. Use it to compute: `qm.data.recipient != ""` tests whether a recipient was given.
- **`qm.ink(qm.data, "sender")`** prints a field. Text printed this way stays linked to its field, which is what lets a click on the page in studio find the field.
- **`$body`** is the Markdown under the document's fields.

A field nobody filled in arrives as its **blank**: `""` for text. The plate checks for it, so an empty letter prints no stray "Dear ,". Every plate has to render a document with every field blank, because that is the document an author starts from.

To pick between `text`, `richtext` and `enum`, see [Picking a text type](#picking-a-text-type) below.

## 4. A date

```yaml
    date:
      type: date
      default: today
      description: YYYY-MM-DD, or today to date the letter when it is rendered.
```

`type: date` holds a calendar date, written `2026-01-15`. `default` is the value a document gets when it leaves the field out, and `today` is the date of the render. The plate prints it with a pattern:

```typst
#align(right, qm.ink(qm.data, "date", "[month repr:long] [day padding:none], [year]"))
```

`qm.data.date` is a Typst `datetime`, for comparing or taking apart. See [Dates](typst-backend.md#dates).

## 5. A choice from a list

```yaml
    closing:
      type: enum
      values: [sincerely, regards, thanks]
      default: sincerely
      description: The word before your signature.
```

An `enum` stores one of its `values`, which are ids rather than the words printed. The plate maps each id to its words:

```typst
#let closing-text = (sincerely: "Sincerely", regards: "Kind regards", thanks: "With thanks")
#if qm.data.closing != "" {
  qm.region("closing")[#closing-text.at(qm.data.closing),]
}
```

An enum can also be blank, `""`, so the plate checks for it. The words here are drawn by the plate rather than printed with `qm.ink`, so `qm.region("closing")` is what links them back to the field. See [Tying Composed Content to a Field](editor-regions.md#tying-composed-content-to-a-field).

List only real choices in `values`. Quillmark supplies the blank itself, and declaring `""` is a load error. See [the blank](quill-yaml-reference.md#the-blank-values-is-for-choices-not-for-the-absence-of-one).

## 6. A card

A **card** is a repeatable part of a document, such as a P.S. note. `Quill.yaml` declares each kind of card under `card_kinds`:

```yaml
card_kinds:
  postscript:
    title: P.S.
    description: A short note after the signature.
```

In a document, each card is its own `~~~` block naming its kind, followed by its own Markdown body. From `example.md`:

```markdown
~~~
$kind: postscript
~~~

The diagrams follow by the evening post.
```

The plate loops over the document's cards:

```typst
#for card in qm.data.at("$cards", default: ()) {
  let body = card.at("$body", default: "")
  if card.at("$kind", default: none) == "postscript" and body != "" [P.S. #body]
}
```

A card kind can declare fields of its own. See [Body, arrays, and cards](typst-backend.md#body-arrays-and-cards) and [`card_kinds`](quill-yaml-reference.md#card_kinds-section).

## 7. Add a field

Now change the quill. Add a title to print under the signature. In `Quill.yaml`, under `main.fields`:

```yaml
    signer_title:
      type: text
      default: ""
      description: Your title, printed under your name. Leave blank for none.
```

At the end of the signature in `plate.typ`, after `#qm.ink(qm.data, "sender")`:

```typst
#if qm.data.signer_title != "" [ \ #qm.ink(qm.data, "signer_title")]
```

Save both files and reload studio. The form has a new field, and what you type in it prints under the signature. Add a value to `example.md`, between its `~~~` lines, so the sample shows it too:

```yaml
signer_title: Translator
```

Then run the gate:

```sh
npm test
```

`quillkit test` renders the quill's starter document and its `example.md`, and fails on any error or warning, such as a value `example.md` gives a field the quill does not declare.

## 8. The CLI

The `quillmark` CLI renders a quill straight to a file and inspects it, which suits scripts and CI. [Install it](../cli/reference.md#installation), then from the quiver's root:

```sh
quillmark render quills/letter/0.1.0 quills/letter/0.1.0/example.md -o letter.pdf
quillmark check --strict --regions quills/letter/0.1.0 quills/letter/0.1.0/example.md
```

`check --regions` lists each field the example fills that no click on the page can reach: a field the plate never prints, or prints without `qm.ink` or `qm.region`. See the [CLI Reference](../cli/reference.md).

## Picking a text type

Three types hold text, and two questions pick one:

1. **Is the set of allowed values closed?** Then it is an `enum`.
2. Otherwise: **should `*text*` render as emphasis, or stay literal?** (`richtext` over `text`.)

`text` is one line and `richtext` one paragraph; `multiline: true` admits more, and a `richtext` declares `multiline:` either way. A name, a URL or a reference key is a one-line `text`, and an abstract or a summary is a `richtext`. The letter needs no `richtext` field: its prose is the document body, which is already rich text. A plate that computes with a `text` value (`upper`, a comparison, `set document(author: ..)`) reads it through [`qm.str`](typst-backend.md#text-as-a-str).

Pick before documents exist. Changing a declared type reinterprets every value already stored in that field, and `text` → `richtext` is lossy: see [Choosing among `enum`, `text`, and `richtext`](quill-yaml-reference.md#choosing-among-enum-text-and-richtext).

Use `default` for the value most authors will accept as-is. A field with no `default` is one the author is expected to answer; left unanswered, it renders its blank. See [`default`](quill-yaml-reference.md#default). For every field type, UI hint and constraint, see the [Quill.yaml Reference](quill-yaml-reference.md).

## A larger plate

`status_report` puts the same ideas in a longer plate: card kinds with fields of their own, a date on each card, and an enum mapped to a banner. Its `Quill.yaml`:

```yaml
--8<-- "crates/fixtures/resources/quills/status_report/0.2.0/Quill.yaml"
```

Its `plate.typ`:

```typst
--8<-- "crates/fixtures/resources/quills/status_report/0.2.0/plate.typ"
```

Three things it adds to the letter:

- **Printing a card's fields.** Every card in the loop arrives through one variable, so `qm.ink(card, "title")` prints the field of that card, and `qm.ink(card, "due", ..)` formats its date. Compute with `card`, print with `qm.ink(card, ..)`. See [Print with `qm.ink`](editor-regions.md#print-with-ink).
- **A guard on a blank enum.** An `else` on a branch over the declared values would also catch the blank and print a state nobody picked, so the plate tests `qm.data.state != ""` first.
- **`#set` rules.** Page and text defaults are ordinary Typst. See [Typesetting](typst-backend.md#typesetting).

## Next steps

- [quillkit & studio](quillkit.md): the authoring loop, the gate, and publishing a quiver as a site
- [Quill.yaml Reference](quill-yaml-reference.md): every field type, UI hint, `card_kinds`, and the `typst` section
- [Typst Backend](typst-backend.md): reading data, card loops, and the helper package
- [Quill Versioning](versioning.md): when a change needs a new version directory
- [Typst's tutorial](https://typst.app/docs/tutorial/): the language a plate is written in

**Tip:** A quill ships everything in its directory except `.git/`, `target/`, `node_modules/` and what `.gitignore` names, so keep scratch files out of it.
