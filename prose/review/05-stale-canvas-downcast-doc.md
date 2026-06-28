# 05 — Stale `as_any`/downcast doc on the canvas seam

**Severity:** nit **Category:** smelly (stale doc) **Status:** Open

**Location:** `crates/core/src/session.rs:73` (doc on `RenderSession::handle`)

## Finding

The doc comment on `RenderSession::handle` still says bindings reach
backend-specific surfaces by downcasting via `SessionHandle::as_any` (the
`typst_session_of` pattern). That was the *old* canvas mechanism. This branch
generalized the canvas seam: the WASM painter now dispatches generically through
`page_size_pt` / `render_rgba` on `RenderSession`, with **no downcast**.

A grep confirms `typst_session_of` has **zero in-tree callers** — only its
definition remains.

## Why it matters

- The whole point of the PREVIEW.md rework was to retire the "canvas is
  Typst-only, downcast to reach it" premise. The doc still instructs the retired
  pattern, so it actively misdirects a contributor wiring a new backend toward a
  downcast they shouldn't need.
- It is documentation drift, not a code bug — but it sits on the public seam that
  the new design is most proud of.

## Fix

Update the comment to describe the generic seam as the canonical path
(`page_size_pt` / `render_rgba` on the session), and reframe `as_any` as a
last-resort escape hatch for a backend with a richer typed surface — not the
default. Optionally note that `typst_session_of` is now callerless and a
candidate for removal.

## Notes

- The ARCHITECTURE.md `RenderSession` entry (line ~50) already describes the new
  generic seam correctly — so this is a localized rustdoc lag, and the canon is
  ahead of the code comment. Aligning the rustdoc to the canon closes it.
