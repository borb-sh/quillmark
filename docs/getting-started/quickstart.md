# Quickstart

From nothing to a PDF. Every path below renders the same two-file quill: a
document format that declares one field, `title`, and prints it above the
document body. To try Quillmark without installing anything, open the
[playground](https://borb-sh.github.io/quillmark-js/playground).

## 1. Write a quill

A quill is a document format: a schema for a document's fields, a layout for
its pages, and the assets the layout uses. On disk it is a directory. Create
`my_quill/Quill.yaml`:

```yaml
quill:
  name: my_quill
  backend: typst
  version: "1.0.0"
  description: A title above the body

main:
  fields:
    title:
      type: text
      description: The heading at the top of the page
```

and `my_quill/plate.typ`, the Typst layout that prints it, which the backend
finds by its name:

```typst
#import "@local/quillmark-helper:0.2.0" as qm

#set page(width: 6in, height: 4in, margin: 0.75in)

= #qm.ink(qm.data, "title")

#qm.data.at("$body", default: "")
```

## 2. Write a document

Next to `my_quill/`, create `document.md`. The `~~~` block on top holds the
fields, and `$quill` names the quill that renders them:

```markdown
~~~
$quill: my_quill
title: Hello, Quillmark
~~~

This paragraph is the document body. *Markdown* works here.
```

## 3. Render it

=== "CLI"

    ```bash
    cargo install quillmark-cli --locked
    quillmark render my_quill document.md
    ```

    `cargo install` builds from source, so it needs a
    [Rust toolchain](https://rustup.rs). Prebuilt binaries need none:
    `cargo binstall quillmark-cli` installs one, and each
    [GitHub Release](https://github.com/borb-sh/quillmark/releases) carries the
    archives ([Installation](../cli/reference.md#installation)).
    `render` writes `document.pdf`. `quillmark check my_quill document.md`
    reports what the document gets wrong without rendering, and
    `quillmark schema my_quill` prints the fields the quill expects.

=== "Python"

    ```bash
    pip install quillmark
    ```

    ```python
    from pathlib import Path
    from quillmark import Engine, Quill, OutputFormat

    quill = Quill.from_path("my_quill")
    doc = quill.parse(Path("document.md").read_text())

    result = Engine().render(quill, doc, OutputFormat.PDF)
    result.artifacts[0].save("document.pdf")
    ```

=== "JavaScript"

    ```bash
    npm install @quillmark/wasm
    ```

    The package runs in Node and the browser, so this builds the quill in
    memory rather than reading the directory:

    ```javascript
    import { init, Engine } from "@quillmark/wasm";

    const { Quill } = await init();
    const enc = new TextEncoder();

    const quill = Quill.fromTree(new Map([
      ["Quill.yaml", enc.encode(`quill:
      name: my_quill
      backend: typst
      version: "1.0.0"
      description: A title above the body

    main:
      fields:
        title:
          type: text
          description: The heading at the top of the page
    `)],
      ["plate.typ", enc.encode(`#import "@local/quillmark-helper:0.2.0" as qm

    #set page(width: 6in, height: 4in, margin: 0.75in)

    = #qm.ink(qm.data, "title")

    #qm.data.at("$body", default: "")
    `)],
    ]));

    const doc = quill.parse(`~~~
    $quill: my_quill
    title: Hello, Quillmark
    ~~~

    This paragraph is the document body. *Markdown* works here.
    `);

    const engine = new Engine();
    const result = await engine.render(quill, doc, { format: "pdf" });
    const pdfBytes = result.artifacts[0].bytes;
    ```

    Top-level `await` runs in Node and in a module script. A bundler targeting
    older browsers (Vite's default does) refuses it: run the code inside an
    `async` function, or set the build target to `es2022`.

    The first `render` loads the Typst backend, a separate multi-MB binary;
    `init` alone loads only the small core build, enough to parse and validate.
    `init` is idempotent and concurrency-safe, so await it at each entry point
    (route loader, hydration path, worker) rather than threading one result
    around. A type annotation needs no await: `Quill` and `Document` are also
    exported as types (`import type { Quill } from "@quillmark/wasm"`).

=== "Rust"

    ```bash
    cargo add quillmark
    ```

    ```rust
    use quillmark::{quill_from_path, Engine, OutputFormat, RenderOptions};

    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let quill = quill_from_path("my_quill")?;
        let doc = quill.parse(&std::fs::read_to_string("document.md")?)?.document;

        // A render reads no clock: the caller passes the render date.
        let today = "2026-03-14".parse()?;
        let options = RenderOptions::default().with_output_format(OutputFormat::Pdf);
        let result = Engine::new().render(&quill, &doc, today, &options)?;

        std::fs::write("document.pdf", &result.artifacts[0].bytes)?;
        Ok(())
    }
    ```

## Next

- Get something wrong on purpose: misspell `title`, or drop the `$` from
  `$quill`. The diagnostic names the mistake and the fix.
- [Creating Quills](../quills/creating-quills.md): more fields, dates, and
  repeating cards.
- [Markdown Syntax](../authoring/markdown-syntax.md) and
  [card-yaml Blocks](../authoring/card-yaml.md): what a document can say.
- [Integrating Quillmark](../integration/index.md): documents built in code,
  error handling, storage, and [live preview](../integration/live-preview.md)
  in a canvas.
