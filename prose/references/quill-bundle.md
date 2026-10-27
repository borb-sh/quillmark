# The `.quill` Bundle

> **Status**: Authoritative specification
> **Implementation**: `crates/core/src/quill/`

A `.quill` bundle is one quill's file tree as one ZIP archive. It is the form a quill takes wherever it travels as bytes: a download, an attachment, a database row, a request between platforms, the output of a tool that writes quills. Every runtime reads and writes it through one implementation, Quillmark's.

The bundle holds the tree and nothing else. It carries no manifest, no index, and no part shared with another bundle, and it names no file outside itself. Fonts, assets and vendored packages are files in the tree like any other.

## 1. Naming

- The file extension is `.quill`. A name may carry more before it (`memo@1.2.0.quill`, `memo@1.2.0.3f9c….quill`); only the extension belongs to the format.
- The media type is `application/vnd.quillmark.quill+zip`. It is not registered with IANA.
- A reader handed bytes identifies a bundle by them, not by a name or a media type, so a host may serve one as `application/zip` or `application/octet-stream`. A loader handed a path takes a file named `*.quill` as a bundle.

## 2. Container

A bundle is a ZIP archive as PKWARE's APPNOTE specifies it, within this subset:

- **One archive on one disk.** The end-of-central-directory record is found from the end of the bytes. An archive comment may follow it, and carries nothing. An archive split across disks is refused.
- **The central directory is authoritative.** Sizes, CRC-32s and the local header offset are read from it. A local header's own sizes may be zero under a data descriptor (general-purpose bit 3), and are not read.
- **No ZIP64.** Every count, size and offset fits its 16- or 32-bit field, which the budget (§4) guarantees. An archive holding a ZIP64 marker value in any of them is refused.
- **Stored or deflated.** Compression method 0 (stored) or 8 (deflate). Any other method is refused.
- **Not encrypted.** General-purpose bits 0 and 6 are clear.
- **Checked.** Each file unpacks to exactly the size it declares and matches the CRC-32 it declares.
- **Metadata carries nothing.** Timestamps, extra fields, entry comments and attributes are ignored, with one exception: an entry whose Unix mode marks it a symbolic link is refused.

## 3. The tree

- **The archive root is the quill root.** `Quill.yaml` is an entry named `Quill.yaml`, not one inside a wrapping folder. An archive holding the quill inside one folder fails as a missing `Quill.yaml`, and the refusal names the folder.
- **Names are relative paths.** An entry name is UTF-8 and separates segments with `/`. A name that starts with `/`, has an empty, `.` or `..` segment, or holds a `\`, `:` or NUL anywhere is refused.
- **A directory entry carries nothing.** A name ending in `/` is a directory entry. It holds zero bytes and may be absent, so an empty directory does not survive a bundle.
- **A path appears once.** An archive listing one path twice, or one path as a file and as a directory (`a` and `a/b`), is refused.
- **Names are compared as written.** Case is significant and no Unicode normalization applies.
- **Every file is in the tree.** Reading applies no ignore rules. A filesystem walk's ignore set is applied before a bundle is written, by the walk.

The tree a bundle holds then loads as any in-memory tree does, under the `Quill.yaml` rules.

## 4. Budget

A reader spends a budget on the sizes the central directory declares, before it inflates any entry:

| Bound | Limit |
| --- | --- |
| Entries in the central directory, directory entries included | 2048 |
| One file, unpacked | 32 MiB |
| All files, unpacked | 64 MiB |

The entry that trips a bound is refused. The limits are constants of the format, not options a reader raises: deflate tops out near 1032:1, so a megabyte of archive can declare a gigabyte, and the budget is what keeps a reader from allocating it. A writer spends the same budget, so a bundle no reader would take is refused where it is written.

## 5. Writing

Quillmark's writer emits one canonical form:

- File entries only, in bytewise path order.
- Each file deflated where that makes it smaller, and stored where it does not.
- Every timestamp 1980-01-01 00:00, the UTF-8 name flag set, attributes zero, no extra fields and no comments.

The same tree therefore writes the same bytes, so a digest of a bundle is stable for one writer. Another writer's output is an equally valid bundle with other bytes, and nothing compares bundles byte for byte across writers.

## 6. Refusals

A bundle outside this specification is refused with one diagnostic:

| Code | Refused |
| --- | --- |
| `quill::bundle_invalid` | Bytes that are not a readable ZIP archive: no end record, a truncated or malformed structure, an entry that fails its size or CRC-32 |
| `quill::bundle_unsupported` | A ZIP feature §2 leaves out: ZIP64, multiple disks, encryption, another compression method, a symbolic link |
| `quill::bundle_path` | An entry name §3 refuses |
| `quill::bundle_too_large` | A bound of §4 |

A tree that reads and does not load fails as the in-memory constructor fails, `quill::missing_file` among its codes.

## 7. Surfaces

| Surface | Read | Write |
| --- | --- | --- |
| Rust (`quillmark-core`) | `Quill::from_bundle(&[u8])` | `quill.to_bundle()` |
| Rust (`quillmark`) | `quill_from_path` on a `.quill` file | — |
| WASM (`@quillmark/wasm`) | `Quill.fromBundle(Uint8Array)` | `quill.toBundle()` |
| Python | `Quill.from_bundle(bytes)`, `Quill.from_path` on a `.quill` file | `quill.to_bundle()` |
| CLI | every command taking a quill path, but `workspace` | `quillmark pack` |
