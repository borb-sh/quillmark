---
name: dense-prose
description: Write comments and docs at high semantic density — terse, present-tense, unsold, mostly self-documenting. Use when writing or reviewing code comments, prose/canon/, or docs/ for density and a consistent voice.
---

## Purpose

Comments and docs earn their bytes. Each should state a fact a reader cannot get
faster from the code itself, in the fewest words that stay correct. This skill
is the house voice: dense, present-tense, declarative, unsold. It composes with
two siblings and does not repeat them:

- **`prune-evolutionary-info`** owns present-tense rewriting — cutting "used to
  / no longer / as of 0.x" history. Invoke it for history.
- **`maintain-canon`** owns the `prose/canon/` doc spine (Title → Implementation
  anchor → TL;DR) and one-concept-per-page. Invoke it for canon structure.

This skill owns what they do not: marketing, over-explanation, and deliberation
narration.

## Prime directive: correctness over brevity

A comment makes a claim about code. Never shorten a claim you have not verified
against the code. Cutting words must not silently change meaning. When unsure a
statement is still true, leave it and keep the fact. Edits are surgical: touch a
line only when it breaks a rule; do not churn prose that is already dense and
correct. Over-editing is the main failure mode.

## What this skill owns

### 1. No marketing or persuasion

Remove words that sell rather than inform: *powerful, seamless, elegant, robust,
flexible, blazing(-fast), cutting-edge, state-of-the-art, first-class (citizen),
rich (set of), comprehensive, battle-tested, out of the box, leverage* (meaning
"use"), and *simply / just / easily* when they only imply ease. State the
capability plainly.

- "Partial documents are first-class citizens" → "A document need not be
  complete."
- "opts into canvas simply by overriding the seam" → "opts into canvas by
  overriding the seam."

Keep *just / simply / only* when they carry real meaning ("just sugar for the
`raw` element", "three or more tildes"). The word is not the violation; the sell
is.

### 2. Self-documenting first — cut over-explanation

The code is the primary documentation. A comment that restates it is noise.

- Delete comments that echo the code (`// increment i`; `/// The name` on a
  field named `name`). Prefer a clearer name over a comment.
- Collapse padded rustdoc scaffolding. A header of "## Key Functions / ## Quick
  Example / ## Detailed Documentation / For comprehensive details including: …"
  becomes a tight paragraph plus, at most, one runnable example.
- Do not enumerate a module's public items in its header — rustdoc lists them,
  and the hand-list rots. Describe the module's job instead.
- One good example beats three; drop "see X for comprehensive coverage" filler.

### 3. State the design, not the deliberation

Describe what is, not what was considered. Cut spike/deferred/rejected
narration; keep the resulting fact and, when it explains a present choice, the
rationale — minus the "we tried / earlier draft" framing.

- "Investigated as a spike but deferred — not needed" → "Not supported; the
  preview does not require it."
- "X was the deferred half and stays deferred by design" → "X is not carried,
  by design: <reason>."
- Rejected-alternative rationale: "A sub-handle would be justified only if paint
  shipped with click()" — keeps the *why*, sheds the *when*.

## Voice

Present tense. Lead with the invariant or contract, then the mechanism. Reuse
the codebase's terms-of-art (*card-yaml block, plate, quill, backend, seam,
Technique A*). Match the density of the best existing comments —
`crates/core/src/value.rs`, `crates/core/src/document/fences.rs`, and
`prose/canon/PREVIEW.md` are the exemplars.

## Scope

| Surface | Rule |
|---|---|
| Code & test comments, `prose/canon/`, `docs/` (non-migration) | Apply in full. |
| `docs/migrations/**` | **Never touch.** Era-accurate and immutable. |
| `prose/references/`, `prose/proposals/` | Strip marketing only. Specs and proposals legitimately discuss other/future states — leave that framing. |
| Load-bearing legacy (e.g. `core/src/document/dto.rs` versioned wire schemas) | Keep. The old-format description *is* current reader behavior; tighten wording, keep the fact. |
| Identifiers (fn / test / var names) | Never rename — out of scope, churn. |

## Workflow

1. **Sweep** — grep comments/docs for the marketing word-list above and for
   deliberation markers (`spike`, `deferred`, `considered`, `for now`,
   `eventually`, `we tried`). For history markers, use `prune-evolutionary-info`.
2. **Triage** — each hit: violation, or load-bearing fact in costume?
3. **Rewrite** in place — present tense, minimal, fact preserved. Fix any
   comment that contradicts the code rather than deleting it.
4. **Verify** — build and tests pass; no doctest broken; no test asserted the
   old wording.

## Done when

Comments and docs state what is, in the house voice: dense, present-tense,
unsold. No comment restates code; no header enumerates rotting lists; no prose
narrates deliberation. Backward-compat facts survive as current-state
statements.
