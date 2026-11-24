# quillkit & studio

`quillkit` is the tool for writing quills. It keeps a **quiver**, a collection of quills kept as an npm project, and it serves **studio**, a browser page that puts a quill's form beside its rendered page.

quillkit is the authoring loop. The `quillmark` [CLI](../cli/reference.md) is for rendering to files, inspecting a quill, and CI scripts that don't run Node.

## Start

```sh
npx quillkit@latest init my-quiver
cd my-quiver
npm install
npx quillkit studio
```

`init` writes a quiver holding one starter quill. [Creating Quills](creating-quills.md) walks through each file it writes. quillkit needs Node.js 22 or later.

`@latest` asks for the newest release. Without it, `npx` may pick an older release that supports your Node version.

## The verbs

| Verb              | What it does                                                         |
| ----------------- | -------------------------------------------------------------------- |
| `quillkit init`   | write a new quiver holding one starter quill                         |
| `quillkit test`   | render every quill's starter document and `example.md`, and fail on any error or warning |
| `quillkit studio` | serve studio over the quiver, and repack it whenever a file is saved |
| `quillkit build`  | pack the quiver into files a browser can load                        |
| `quillkit site`   | write a deployable site: studio plus the packed quiver               |

## The loop

1. Run `npx quillkit studio` and open the address it prints.
2. Edit `Quill.yaml`, `plate.typ` or `example.md`, and save.
3. Reload studio. The quill you were on reopens with the change.
4. Run `npm test` before you commit, and in CI.

Studio is for looking: how the form reads, how the page looks, and where a click on the page lands. It shows errors but fails nothing. `quillkit test` is the check that passes or fails, and CI should run it.

Studio can download the page as a PDF, and its **Edit source** view shows the document's Markdown, so a document drafted in studio can be saved straight into `example.md`.

## Prettier

Prettier rewrites a card's `~~~` fence as a backtick fence, and a backtick fence opens no card, so a formatted document no longer parses. `init` writes a `.prettierignore` that keeps prettier out of the quills:

```
quills/
```

Add the same line to an existing project that runs prettier, along with any other directory holding Quillmark documents.

## More

[quillkit's README](https://github.com/birdorbs/quillmark-js/tree/main/packages/quillkit#readme) covers every flag, gating starter templates with `--templates`, and deploying a site to Vercel, Netlify or GitHub Pages.
