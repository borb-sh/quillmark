# Security Policy

## Reporting a vulnerability

Report privately through GitHub: [open a draft security advisory](https://github.com/birdorbs/quillmark/security/advisories/new). Do not open a public issue for a vulnerability.

Include the Quillmark version, the surface (Rust, Python, WASM, CLI), and the smallest document or quill that reproduces it. The fix ships in a new release, and the advisory is published with it.

## Supported versions

Quillmark is pre-1.0. Fixes land on the latest release only; upgrade to receive them.

## What counts

The threat model is in [Operations § Untrusted input](docs/integration/operations.md#untrusted-input). In short: a document is untrusted data, and a quill is code the host chose to run.

A vulnerability, report it privately:

- A document that runs Typst code, reads a file outside its quill, or reaches the network.
- A quill that reads outside its own file tree or reaches the network.
- A document that panics Quillmark.
- A Markdown link in a document that reaches SVG or PDF output as a script link.

Not a vulnerability, open an ordinary issue:

- A document or quill whose render is slow or uses a lot of memory. Render has no budget; the host bounds it ([Operations § Render is not bounded](docs/integration/operations.md#render-is-not-bounded)). A cost far out of proportion to the input is still worth an issue.
- A panic from a quill you wrote. It is still a bug, so report it.
