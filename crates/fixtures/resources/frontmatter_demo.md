~~~card-yaml
#@quill: test_quill
#@kind: main
title: Quillmark Card-YAML Demo
author: Quillmark Team
date: 2024-01-01
version: 1.0
tags:
  - demo
  - card-yaml
  - yaml
  - markdown
description: >
  This document demonstrates Quillmark's ability to parse
  card-yaml metadata blocks and separate them from markdown content.
~~~

# Welcome to Quillmark Frontmatter Demo

This document demonstrates the new frontmatter parsing capabilities of Quillmark.

## How It Works

Quillmark now parses **YAML frontmatter** at the beginning of markdown documents. The frontmatter fields are extracted into a dictionary, while the markdown body is converted to the target format.

### Key Features

- **YAML Parsing**: Supports standard YAML syntax in frontmatter
- **Field Extraction**: All frontmatter fields are available as dictionary entries  
- **Body Separation**: Only the markdown body (not frontmatter) gets converted
- **Backward Compatible**: Documents without frontmatter work as before

## Example Usage

When you process this document with Quillmark:

1. The **frontmatter** is parsed into fields like `title`, `author`, `subject`, etc.
2. The **body** (this markdown content) is converted to the target format
3. Backends can access both the frontmatter dictionary and the converted body

### Supported YAML Types

- **Strings**: `title: "My Document"`
- **Numbers**: `version: 1.0` 
- **Arrays**: `tags: [demo, yaml]`
- **Objects**: `author: {name: "John", email: "john@example.com"}`
- **Multi-line**: Using `>` or `|` syntax

## Implementation

The parsing logic is implemented in `quillmark-core`:

```rust
use quillmark_core::{decompose, BODY_FIELD};

let parsed = decompose(markdown_content)?;
let frontmatter_title = parsed.get_field("title");
let body_content = parsed.body();
```

This enables clean separation of concerns between document metadata and content.

---

*This document was generated to demonstrate Quillmark's frontmatter capabilities.*