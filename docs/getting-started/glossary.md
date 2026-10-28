# Glossary

The words Quillmark uses, each in one place. [Concepts](concepts.md) shows how the pieces fit together.

## backend

What turns a quill and a document into pages. `typst` draws pages from a [plate](#plate); `acroform` fills the fields of an existing PDF form. See [Typst Backend](../quills/typst-backend.md) and [PDF Form Backend](../quills/acroform-backend.md).

## blank

The value a field holds when nobody answered it: `""` for text and enums, `0` for a number, `false` for a boolean, an empty list for a list, and `none` for a date. Every field accepts its blank, and a plate has to render a document whose fields are all blank. See [Blank values](../quills/typst-backend.md#blank-values).

## blueprint

A fill-in-the-blanks version of a document, generated from a quill's `Quill.yaml`. Each field is listed with its description and type as comments, ready for a person or an LLM to fill in. `quillmark blueprint <quill>` prints it. See [Blueprint & Seeding](../quills/blueprint.md).

## body

The Markdown under a card's `~~~` block: the prose of the document, or of one card.

## card

One `~~~` block of a document and the Markdown below it. The first card is the **main card**, which names the quill with `$quill`. Each later card names its kind with `$kind`, such as a P.S. note or an action item, and a document can hold any number of them. See [card-yaml Blocks](../authoring/card-yaml.md).

## card kind

A kind of card a quill accepts, declared under `card_kinds` in `Quill.yaml` with its own fields. See [`card_kinds`](../quills/quill-yaml-reference.md#card_kinds-section).

## default

The value a field takes when a document leaves it out, declared as `default:` in `Quill.yaml`. A field with no default renders its [blank](#blank). See [`default`](../quills/quill-yaml-reference.md#default).

## example

A quill filled in with made-up values: the seeded document with each `example` its fields and bodies declare in `Quill.yaml` committed. A quill field's `example` is also the blueprint's `# e.g.` line.

## field

One named value a card holds, such as `sender` or `date`, declared under `fields` in `Quill.yaml` with a type.

## ink

Text on the rendered page that stays linked to the field it came from, so a click on it in a live preview finds that field. A plate prints a field as ink with `qm.ink(..)`. See [Editor Regions](../quills/editor-regions.md).

## plate

The Typst file, `plate.typ`, that lays out the pages of a `typst` quill. It reads the document's values through the helper package, `qm`.

## quill

A document format: the fields a document fills in, the layout that prints them, and the assets the layout uses. On disk, a directory holding `Quill.yaml`, a plate or PDF form, and any fonts or images. A document names its quill in its `$quill` line.

## quillkit

The command-line tool for writing quills: it starts a quiver, tests it, and serves studio. See [quillkit & studio](../quills/quillkit.md).

## quiver

A collection of quills, kept as an npm project: a `Quiver.yaml` and each quill under `quills/<name>/<version>/`. An app loads a quiver to offer its quills.

## region

An area of the rendered page that maps a click back to a field. Ink makes one. When a plate composes text itself, such as the words an enum id stands for, `qm.region(..)` claims that text for a field. See [Editor Regions](../quills/editor-regions.md).

## seed

The starter document generated from a quill: the main card plus one card of each kind, each field at its default or blank. It is what a new document opens as. See [Blueprint & Seeding](../quills/blueprint.md).

## studio

A browser page, served by `quillkit studio`, that shows a quill's form beside its rendered page and updates as you edit. See [quillkit & studio](../quills/quillkit.md).
