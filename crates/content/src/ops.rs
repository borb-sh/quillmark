//! Island, line and mark op channels: structural edits separate from text
//! splices. All three apply after [`Content::apply_text_delta`] in one
//! [`ChangeBundle`], in that order; mark ranges are in final-text coordinates.

use crate::delta::{Assoc, Delta, Op};
use crate::model::{
    line_kind_mismatch, Container, Island, Line, LineKind, LineKindMismatch, Mark, MarkKind,
    Content, Usv, ISLAND_SLOT,
};
use crate::normalize::admit_char;
use crate::usv::char_to_byte;
use serde::Deserialize;
use std::borrow::Cow;

/// A mark edit in final-text coordinates (post-delta, post-line-op).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MarkOp {
    /// Add a mark over `[start, end)`. An anchor `kind` must carry a non-empty
    /// `id` not already live in the field ([`ApplyError::AnchorIdCollision`] /
    /// [`ApplyError::EmptyAnchorId`]).
    Add {
        start: Usv,
        end: Usv,
        kind: MarkKind,
    },
    /// Un-format `kind` over `[start, end)`: subtract the range from each
    /// overlapping same-kind *formatting* mark, keeping the non-overlapping
    /// fragments. Non-formatting (identity/unknown) handles cannot be
    /// range-fragmented, so an overlapping one is dropped whole; anchors
    /// normally go through [`MarkOp::RemoveAnchor`].
    Remove {
        start: Usv,
        end: Usv,
        kind: MarkKind,
    },
    /// Drop one identity anchor by id.
    RemoveAnchor { id: String },
}

/// A line/block edit. Split/join splice `\n` in `text`; set ops touch metadata
/// only.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LineOp {
    /// Paragraph break at `at`: insert `\n` and split the line metadata.
    Split { at: Usv },
    /// Join line `line` with the next: remove the `\n` between them.
    Join { line: usize },
    /// Replace a line's block role.
    SetKind { line: usize, kind: LineKind },
    /// Replace a line's container path.
    SetContainers {
        line: usize,
        containers: Vec<Container>,
    },
    /// Set (or clear) a line's `continues` flag: whether it continues the
    /// previous line's block across a within-block hard break (a markdown hard
    /// break, a code fence's interior line) rather than starting a new block.
    /// Split, join and text-delta `\n` insertion all mint `continues: false`
    /// lines, so this is the only op that reaches the flag. Setting it on line 0
    /// is [`ApplyError::FirstLineContinues`]; setting it after a block that
    /// renders one line is [`ApplyError::ContinuesSingleLineBlock`].
    SetContinues { line: usize, continues: bool },
}

/// An island edit: the channel that reaches [`Island`] payloads, which no other
/// channel carries. Both ops act on one island entry, leaving the slot in place,
/// so identity anchors elsewhere in the field survive an island edit.
///
/// Removal needs no op: a text delta that deletes a slot drops the backing
/// entry ([`Content::apply_text_delta`]'s cascade). That drop is whole, so
/// re-landing the island is an [`IslandOp::Insert`] carrying the [`Island`]
/// itself. A *block* island's line demotes to `Para` when its slot goes, so
/// re-landing one re-tags the line too.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum IslandOp {
    /// Replace the entry `island.id` names, in place. The id is the target *and*
    /// the stored value, so an island cannot be renamed through this op. An id no
    /// island carries is [`ApplyError::UnknownIslandId`], never a silent no-op.
    ///
    /// `props`, `island_type` and `loss` all come from the op; nothing derives
    /// `loss` from the props.
    Set { island: Island },
    /// Insert an island: the [`ISLAND_SLOT`] at `at` and its backing entry in
    /// one op, so a slot never exists without the [`Island`] behind it.
    ///
    /// `at` is a USV position in the text the delta and this bundle's earlier
    /// island ops left: each insert splices its slot before the next op reads
    /// the text, so of two inserts at one position the later one lands first.
    /// The entry files at its slot-order index in that same frame. A stale frame
    /// misplaces slots and never errors.
    ///
    /// The id must be non-empty and unique in the field
    /// ([`ApplyError::EmptyIslandId`], [`ApplyError::IslandIdCollision`]); a
    /// delete earlier in the same bundle frees its id for reuse here.
    ///
    /// **Block islands.** The slot alone is an *inline* island (a slot in a
    /// `Para`). A block island is that slot alone on its own line under
    /// [`LineKind::Island`], which takes three channels in one bundle: the text
    /// delta inserts the `\n`, this op inserts the slot, and
    /// [`LineOp::SetKind`] tags the line. That order is why island ops run
    /// *before* line ops: `SetKind` validates the kind against the text already
    /// on the line. `LineOp::Split` cannot stand in for the delta's `\n`: it
    /// runs in the later stage.
    ///
    /// A slot inserted onto a line whose kind names its content (`Code`, `Rule`)
    /// contradicts that kind; `normalize` demotes the line to `Para` at the end
    /// of the bundle rather than failing it.
    Insert { at: Usv, island: Island },
}

/// One committed field edit: a text delta and the three op channels, applied in
/// field order (delta → islands → lines → marks) by
/// [`Content::apply_field_change`]. [`Default`] is the identity bundle, so a
/// caller names only the channels it uses:
/// `ChangeBundle { delta, ..Default::default() }`.
///
/// Within a channel ops apply in sequence: op *n*'s coordinates read the state
/// ops `0..n` left, not the frame the channel opens in. That is USV positions
/// for an island insert and line indices for [`LineOp::Split`] /
/// [`LineOp::Join`], which renumber every later line. A stale frame stays in
/// range, so the bundle applies cleanly and lands the wrong document.
///
/// # Mark rebase
///
/// `delta`, `island_ops` and `line_ops` each move text, and each rebases the
/// marks already in the field by one rule:
///
/// | mark edge | [`Assoc`] | an insertion at that exact position |
/// |---|---|---|
/// | a range's `start` | `After` | grows text *outside* the span |
/// | a range's `end` | `Before` | grows text *outside* the span |
/// | a zero-width mark | `Before` | leaves the mark put |
///
/// `mark_ops` name the result, so a caller emitting them predicts this rebase.
/// [`Content::map_marks`] runs it rather than reproducing it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeBundle {
    /// The text splice; the identity delta (no ops) is no text change.
    pub delta: Delta,
    /// Island edits, in post-delta coordinates.
    pub island_ops: Vec<IslandOp>,
    /// Line edits, in post-delta, post-island-op coordinates.
    pub line_ops: Vec<LineOp>,
    /// Mark edits, in final-text coordinates.
    pub mark_ops: Vec<MarkOp>,
}

impl Default for ChangeBundle {
    fn default() -> Self {
        ChangeBundle {
            delta: Delta { ops: Vec::new() },
            island_ops: Vec::new(),
            line_ops: Vec::new(),
            mark_ops: Vec::new(),
        }
    }
}

impl ChangeBundle {
    /// A bundle carrying `delta` and no ops: the per-keystroke splice.
    pub fn from_delta(delta: Delta) -> Self {
        ChangeBundle {
            delta,
            ..Default::default()
        }
    }

    fn is_delta_only(&self) -> bool {
        self.island_ops.is_empty() && self.line_ops.is_empty() && self.mark_ops.is_empty()
    }
}

// The op readers below reuse `serial`'s hand-written readers for `MarkKind` /
// `LineKind` / `Container`, so an `applyChange` bundle speaks the same shapes
// the content read surface does rather than a second serde-derived dialect.
// The wire is a reading direction: bundles are authored on the JS/Python side,
// and no encoder answers these.

use crate::serial::{
    container_from_authored_value, island_from_value, line_kind_from_authored_value,
    mark_from_authored_value, usv_from, ParseError,
};
use serde_json::Value;

/// Decode a [`MarkOp`] from its wire object (`{op, start, end, type, attrs}` for
/// `add`/`remove`, `{op, id}` for `removeAnchor`). `add`/`remove` read the mark
/// vocabulary on the authored lane, which refuses the `@0.93.0` payload
/// spelling rather than guessing which of the two a stale host meant.
pub fn mark_op_from_value(v: &Value) -> Result<MarkOp, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("mark op"))?;
    match o.get("op").and_then(Value::as_str) {
        Some("add") => {
            let mark = mark_from_authored_value(v)?;
            Ok(MarkOp::Add {
                start: mark.start,
                end: mark.end,
                kind: mark.kind,
            })
        }
        Some("remove") => {
            let mark = mark_from_authored_value(v)?;
            Ok(MarkOp::Remove {
                start: mark.start,
                end: mark.end,
                kind: mark.kind,
            })
        }
        Some("removeAnchor") => Ok(MarkOp::RemoveAnchor {
            id: o
                .get("id")
                .and_then(Value::as_str)
                .ok_or(ParseError::Shape("removeAnchor id"))?
                .to_string(),
        }),
        _ => Err(ParseError::Shape("mark op kind")),
    }
}

/// Decode a [`LineOp`] from its wire object. `setKind` carries the line-kind
/// discriminant (`kind` plus its `attrs`) flattened alongside `op`/`line`.
pub fn line_op_from_value(v: &Value) -> Result<LineOp, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("line op"))?;
    let line = || usv_from(o.get("line"), "line op line");
    match o.get("op").and_then(Value::as_str) {
        Some("split") => Ok(LineOp::Split {
            at: usv_from(o.get("at"), "split at")?,
        }),
        Some("join") => Ok(LineOp::Join { line: line()? }),
        Some("setKind") => Ok(LineOp::SetKind {
            line: line()?,
            kind: line_kind_from_authored_value(v)?,
        }),
        Some("setContainers") => Ok(LineOp::SetContainers {
            line: line()?,
            containers: o
                .get("containers")
                .and_then(Value::as_array)
                .ok_or(ParseError::Shape("setContainers containers"))?
                .iter()
                .map(container_from_authored_value)
                .collect::<Result<_, _>>()?,
        }),
        Some("setContinues") => Ok(LineOp::SetContinues {
            line: line()?,
            continues: o
                .get("continues")
                .and_then(Value::as_bool)
                .ok_or(ParseError::Shape("setContinues continues"))?,
        }),
        _ => Err(ParseError::Shape("line op kind")),
    }
}

/// Decode an [`IslandOp`] from its wire object. Both arms carry the island
/// vocabulary (`{id, type, props, loss}`) flattened alongside `op`.
pub fn island_op_from_value(v: &Value) -> Result<IslandOp, ParseError> {
    let o = v.as_object().ok_or(ParseError::Shape("island op"))?;
    let island = || island_from_value(v);
    match o.get("op").and_then(Value::as_str) {
        Some("set") => Ok(IslandOp::Set { island: island()? }),
        Some("insert") => Ok(IslandOp::Insert {
            at: usv_from(o.get("at"), "island insert at")?,
            island: island()?,
        }),
        _ => Err(ParseError::Shape("island op kind")),
    }
}

/// Lower a committed change bundle object (`{delta?, islandOps?, lineOps?,
/// markOps?}`) to core ops. A missing `delta` is the identity (no text change);
/// a missing/`null` op array is empty. Both camelCase and snake_case keys are
/// accepted, so the one reader serves the wasm and Python surfaces. The error is
/// a message string the binding wraps in its own error type.
pub fn change_bundle_from_value(v: &Value) -> Result<ChangeBundle, String> {
    let obj = v
        .as_object()
        .ok_or("bundle must be an object { delta?, islandOps?, lineOps?, markOps? }")?;
    let get = |snake: &str, camel: &str| obj.get(snake).or_else(|| obj.get(camel));
    let delta = match obj.get("delta") {
        Some(Value::Null) | None => Delta { ops: Vec::new() },
        Some(d) => Delta::deserialize(d).map_err(|e| format!("invalid delta: {e}"))?,
    };
    Ok(ChangeBundle {
        delta,
        island_ops: op_array(
            get("island_ops", "islandOps"),
            island_op_from_value,
            "islandOps",
        )?,
        line_ops: op_array(get("line_ops", "lineOps"), line_op_from_value, "lineOps")?,
        mark_ops: op_array(get("mark_ops", "markOps"), mark_op_from_value, "markOps")?,
    })
}

fn op_array<T>(
    value: Option<&Value>,
    convert: impl Fn(&Value) -> Result<T, ParseError>,
    what: &str,
) -> Result<Vec<T>, String> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let arr = value
        .as_array()
        .ok_or_else(|| format!("{what} must be an array"))?;
    arr.iter()
        .map(|v| convert(v).map_err(|e| format!("invalid {what}: {e}")))
        .collect()
}

/// Why an apply failed: range or line index out of bounds, or invariants
/// broken before normalization could repair them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApplyError {
    MarkOutOfRange {
        start: Usv,
        end: Usv,
        len: Usv,
    },
    LineOutOfRange {
        line: usize,
        lines: usize,
    },
    SplitPositionOutOfRange {
        at: Usv,
        len: Usv,
    },
    SplitAtNewline {
        at: Usv,
    },
    LineCountMismatch {
        lines: usize,
        segments: usize,
    },
    /// A [`LineOp::SetContinues`] set `continues: true` on line 0, which has
    /// nothing before it to continue. Refused because `normalize` does not
    /// repair it.
    FirstLineContinues,
    /// [`LineOp::SetContinues`] would make a line continue a block it is not in:
    /// its container path differs from the previous line's, and a within-block
    /// break lives inside one container.
    ContinuesAcrossContainers { line: usize },
    /// [`LineOp::SetContinues`] would make a line continue a block that renders
    /// one line ([`LineKind::takes_continuations`]): a heading, an island or a
    /// rule. Export reads the block's first line alone, so the write would
    /// silently drop the continuation's text.
    ContinuesSingleLineBlock { line: usize },
    /// The text delta's expected base length disagreed with the content:
    /// it was built against a different revision.
    DeltaBaseMismatch {
        expected: usize,
        actual: usize,
    },
    /// An `Op::Insert` carried a raw [`ISLAND_SLOT`], which would leave a slot
    /// with no backing [`Island`]. Islands are created through
    /// [`IslandOp::Insert`], never a text splice, so a whole-field splice
    /// carrying slots must be split into a slot-stripped delta plus one
    /// [`IslandOp::Insert`] per slot.
    IslandSlotInInsert,
    /// A [`MarkOp::Add`] of an anchor whose `id` is already live in the field.
    /// Rejected rather than replaced (which would retarget a live thread) or
    /// coexisting (which `RemoveAnchor` cannot disambiguate).
    AnchorIdCollision { id: String },
    /// A [`MarkOp::Add`] of an anchor with the empty `id`.
    EmptyAnchorId,
    /// An [`IslandOp::Set`] naming an `id` no island in the field carries.
    UnknownIslandId { id: String },
    /// An [`IslandOp::Insert`] whose `id` is already live in the field. `Set`
    /// addresses by id, so a duplicate is an island neither op can name.
    IslandIdCollision { id: String },
    /// An [`IslandOp::Insert`] carrying the empty `id`.
    EmptyIslandId,
    /// An [`IslandOp::Insert`] whose `at` is past the end of the text the delta
    /// and this bundle's earlier island ops left.
    IslandInsertOutOfRange { at: Usv, len: Usv },
    /// A [`LineOp::SetKind`] whose kind contradicts the line's text: tagging
    /// prose `Island` or `Rule`, or a slot-bearing line `Code`. Export trusts
    /// the kind over the text, so the write would silently drop the line's
    /// content.
    LineKindMismatch {
        line: usize,
        mismatch: LineKindMismatch,
    },
    /// A [`LineOp::SetContainers`] nested a line deeper than
    /// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH).
    NestingTooDeep {
        line: usize,
        depth: usize,
        max: usize,
    },
    /// A [`LineOp::SetKind`] naming a heading level outside `1..=6`. Refused
    /// because `normalize` does not repair it: the level reaches export as that
    /// many `#`, which CommonMark reads back as a literal-hash paragraph.
    BadHeadingLevel {
        line: usize,
        level: u8,
    },
}

impl Content {
    /// Splice `text` via `delta`, rebase marks, sync `lines` to `\n` changes,
    /// cascade island removal for any deleted slot, then normalize.
    ///
    /// Islands stay in lockstep with their [`ISLAND_SLOT`] chars: a delta that
    /// *deletes* a slot drops the corresponding [`Island`]; a delta that
    /// *inserts* a raw slot is rejected ([`ApplyError::IslandSlotInInsert`]).
    ///
    /// Inserted text is sanitized first, mirroring what `import` applies at the
    /// string boundary: `\r` and Unicode bidi controls are stripped, and a line
    /// separator (VT, FF, NEL, U+2028, U+2029) becomes a space — the chars
    /// [`Content::validate`] forbids.
    pub fn apply_text_delta(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        self.apply_text_delta_inner(delta)?;
        self.normalize();
        Ok(())
    }

    fn apply_text_delta_inner(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        // Checked up front so the content is untouched on this error.
        for op in &delta.ops {
            if let Op::Insert(s) = op {
                if s.contains(ISLAND_SLOT) {
                    return Err(ApplyError::IslandSlotInInsert);
                }
            }
        }

        // Sanitizing the whole delta up front keeps `try_apply` / `map_pos` /
        // line+island sync in agreement on one cleaned op stream.
        let sanitized = sanitize_inserts(delta);
        let delta = sanitized.as_ref();

        let old_chars: Vec<char> = self.text.chars().collect();
        // `try_apply` retains the untouched remainder implicitly, so a splice
        // may name only the region it changes.
        let new_text = delta
            .try_apply(&self.text)
            .map_err(|e| ApplyError::DeltaBaseMismatch {
                expected: e.expected,
                actual: e.actual,
            })?;
        let old_lines = std::mem::take(&mut self.lines);

        self.rebase_marks(delta);
        let new_len = new_text.chars().count();
        self.marks.retain(|m| {
            m.start <= m.end
                && m.end <= new_len
                && (m.start < m.end || !m.kind.is_formatting())
        });

        self.text = new_text;
        let old_islands = std::mem::take(&mut self.islands);
        (self.lines, self.islands) = sync_for_delta(&old_chars, old_lines, old_islands, delta);
        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }

    fn rebase_marks(&mut self, delta: &Delta) {
        for m in &mut self.marks {
            if m.start == m.end {
                let p = delta.map_pos(m.start, Assoc::Before);
                m.start = p;
                m.end = p;
            } else {
                m.start = delta.map_pos(m.start, Assoc::After);
                m.end = delta.map_pos(m.end, Assoc::Before);
            }
        }
    }

    /// Apply mark ops in final-text coordinates, then normalize.
    pub fn apply_mark_ops(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
        self.apply_mark_ops_inner(ops)?;
        self.normalize();
        Ok(())
    }

    fn apply_mark_ops_inner(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
        let len = self.len_usv();
        for op in ops {
            match op {
                MarkOp::Add { start, end, kind } => {
                    if *start > *end || *end > len {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    if kind.is_formatting() && start == end {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    if let MarkKind::Anchor { id } = kind {
                        if id.is_empty() {
                            return Err(ApplyError::EmptyAnchorId);
                        }
                        if self
                            .marks
                            .iter()
                            .any(|m| matches!(&m.kind, MarkKind::Anchor { id: aid } if aid == id))
                        {
                            return Err(ApplyError::AnchorIdCollision { id: id.clone() });
                        }
                    }
                    self.marks.push(Mark {
                        start: *start,
                        end: *end,
                        kind: kind.clone(),
                    });
                }
                MarkOp::Remove { start, end, kind } => {
                    if *start > *end || *end > len {
                        return Err(ApplyError::MarkOutOfRange {
                            start: *start,
                            end: *end,
                            len,
                        });
                    }
                    let mut next = Vec::with_capacity(self.marks.len());
                    for m in self.marks.drain(..) {
                        if m.kind != *kind || !ranges_overlap(m.start, m.end, *start, *end) {
                            next.push(m);
                            continue;
                        }
                        // Identity/unknown handles have no range algebra to
                        // subtract: drop the overlapping one whole.
                        if !kind.is_formatting() {
                            continue;
                        }
                        // An edge-aligned removal yields a zero-width fragment
                        // here; `normalize` drops it.
                        if m.start < *start {
                            next.push(Mark {
                                start: m.start,
                                end: *start,
                                kind: m.kind.clone(),
                            });
                        }
                        if *end < m.end {
                            next.push(Mark {
                                start: *end,
                                end: m.end,
                                kind: m.kind.clone(),
                            });
                        }
                    }
                    self.marks = next;
                }
                MarkOp::RemoveAnchor { id } => {
                    self.marks
                        .retain(|m| !matches!(&m.kind, MarkKind::Anchor { id: aid } if aid == id));
                }
            }
        }
        Ok(())
    }

    /// Apply island ops: replace an entry by id, or insert a slot and its entry
    /// together.
    pub fn apply_island_ops(&mut self, ops: &[IslandOp]) -> Result<(), ApplyError> {
        self.apply_island_ops_inner(ops)?;
        self.normalize();
        Ok(())
    }

    fn apply_island_ops_inner(&mut self, ops: &[IslandOp]) -> Result<(), ApplyError> {
        for op in ops {
            match op {
                IslandOp::Set { island } => {
                    let idx = self
                        .islands
                        .iter()
                        .position(|i| i.id == island.id)
                        .ok_or_else(|| ApplyError::UnknownIslandId {
                            id: island.id.clone(),
                        })?;
                    // In place: the slot does not move, so no anchor pays.
                    self.islands[idx] = island.clone();
                }
                IslandOp::Insert { at, island } => {
                    if island.id.is_empty() {
                        return Err(ApplyError::EmptyIslandId);
                    }
                    if self.islands.iter().any(|i| i.id == island.id) {
                        return Err(ApplyError::IslandIdCollision {
                            id: island.id.clone(),
                        });
                    }
                    let chars: Vec<char> = self.text.chars().collect();
                    if *at > chars.len() {
                        return Err(ApplyError::IslandInsertOutOfRange {
                            at: *at,
                            len: chars.len(),
                        });
                    }
                    // Islands are stored in slot order.
                    let slot_idx = chars[..*at].iter().filter(|&&c| c == ISLAND_SLOT).count();
                    let byte = char_to_byte(&self.text, *at);
                    self.text.insert(byte, ISLAND_SLOT);
                    self.rebase_marks(&Delta {
                        ops: vec![Op::Retain(*at), Op::Insert(ISLAND_SLOT.to_string())],
                    });
                    self.islands.insert(slot_idx, island.clone());
                }
            }
        }
        Ok(())
    }

    /// Apply line ops: split/join splice `\n`; set ops touch metadata only.
    pub fn apply_line_ops(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
        self.apply_line_ops_inner(ops)?;
        self.normalize();
        Ok(())
    }

    fn apply_line_ops_inner(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
        for op in ops {
            match op {
                LineOp::Split { at } => self.split_line(*at)?,
                LineOp::Join { line } => self.join_line(*line)?,
                LineOp::SetKind { line, kind } => {
                    // Export reads the kind and never the segment, so an
                    // `Island`/`Rule` tag over prose projects the text away.
                    let seg = self
                        .text
                        .split('\n')
                        .nth(*line)
                        .ok_or(ApplyError::LineOutOfRange {
                            line: *line,
                            lines: self.lines.len(),
                        })?;
                    if let Some(mismatch) = line_kind_mismatch(kind, seg) {
                        return Err(ApplyError::LineKindMismatch {
                            line: *line,
                            mismatch,
                        });
                    }
                    if let LineKind::Heading { level } = kind
                        && !(1..=6).contains(level)
                    {
                        return Err(ApplyError::BadHeadingLevel {
                            line: *line,
                            level: *level,
                        });
                    }
                    let line = self.line_mut(*line)?;
                    line.kind = kind.clone();
                }
                LineOp::SetContainers { line, containers } => {
                    // The Typst emitter recurses one frame per container, so a
                    // path past this cap is a render refusal rather than markup.
                    // Same cap as import.
                    if containers.len() > crate::MAX_NESTING_DEPTH {
                        return Err(ApplyError::NestingTooDeep {
                            line: *line,
                            depth: containers.len(),
                            max: crate::MAX_NESTING_DEPTH,
                        });
                    }
                    let line = self.line_mut(*line)?;
                    line.containers = containers.clone();
                }
                LineOp::SetContinues { line, continues } => {
                    if *line == 0 && *continues {
                        return Err(ApplyError::FirstLineContinues);
                    }
                    // The same refusal one line further on: nothing precedes
                    // line 0 to continue, and the line before *this* one is a
                    // block in a different container, which is no more
                    // continuable.
                    if *continues
                        && self
                            .lines
                            .get(*line)
                            .zip(self.lines.get(line.wrapping_sub(1)))
                            .is_some_and(|(l, prev)| l.containers != prev.containers)
                    {
                        return Err(ApplyError::ContinuesAcrossContainers { line: *line });
                    }
                    // The kind side of the same refusal: a heading, an island
                    // and a rule are one line, so nothing after one is inside
                    // the block it claims to continue.
                    if *continues
                        && self
                            .lines
                            .get(*line)
                            .and(self.lines.get(line.wrapping_sub(1)))
                            .is_some_and(|prev| !prev.kind.takes_continuations())
                    {
                        return Err(ApplyError::ContinuesSingleLineBlock { line: *line });
                    }
                    let l = self.line_mut(*line)?;
                    l.continues = *continues;
                }
            }
        }
        Ok(())
    }

    /// One committed field edit bundle: text delta, then island ops, then line
    /// ops, then marks, canonicalized by a single terminal
    /// [`normalize`](Self::normalize).
    ///
    /// All-or-nothing: on any op's error `self` is left exactly as it was. A
    /// bundle carrying ops stages on a scratch copy and swaps in only once every
    /// stage succeeds; the pure-text-delta path skips that clone, since
    /// `apply_text_delta` validates before mutating.
    ///
    /// **Stage order is a coordinate contract**: each stage reads the text the
    /// earlier ones left. An island insert splices a slot, so a
    /// `LineOp::SetKind { kind: Island }` in the same bundle can only validate
    /// against a line that already carries it, and `Split`/`Join` and every mark
    /// range are then measured in a frame that includes the new slots.
    ///
    /// One terminal normalize suffices because split/join rebase marks through
    /// their `\n` splice, so the formatting-edge `\n`-trim commutes with the
    /// line ops, and `MarkOp::Remove` is coverage-set subtraction, which
    /// commutes with `normalize`'s same-kind union (`(A ∪ B) \ R = (A\R) ∪
    /// (B\R)`).
    pub fn apply_field_change(&mut self, bundle: &ChangeBundle) -> Result<(), ApplyError> {
        if bundle.is_delta_only() {
            return self.apply_text_delta(&bundle.delta);
        }
        let mut scratch = self.clone();
        scratch.apply_text_channels(bundle)?;
        scratch.apply_mark_ops_inner(&bundle.mark_ops)?;
        scratch.normalize();
        *self = scratch;
        Ok(())
    }

    /// Every channel of `bundle` that moves text, in bundle order. Sole caller
    /// of the three, so an editor reading [`map_marks`](Self::map_marks) and the
    /// store reading [`apply_field_change`](Self::apply_field_change) cannot
    /// answer a position differently.
    fn apply_text_channels(&mut self, bundle: &ChangeBundle) -> Result<(), ApplyError> {
        self.apply_text_delta_inner(&bundle.delta)?;
        self.apply_island_ops_inner(&bundle.island_ops)?;
        self.apply_line_ops_inner(&bundle.line_ops)
    }

    /// Where `bundle`'s text-moving channels leave the marks this content
    /// already holds: the final-text coordinates [`ChangeBundle::mark_ops`] are
    /// written in, under the rebase rule stated on [`ChangeBundle`].
    ///
    /// `bundle.mark_ops` are ignored, so an editor building them diffs its
    /// intended marks against this instead of predicting the rebase. The answer
    /// is [`normalize`](Self::normalize)d, as the store's is: marks a text move
    /// drops (out of range, zero-width formatting) are absent, and same-kind
    /// runs a move left adjacent arrive already unioned. A bundle whose
    /// `mark_ops` are empty therefore names the marks the field will hold.
    ///
    /// Errors are [`apply_field_change`](Self::apply_field_change)'s on the same
    /// ops, minus those only a mark op raises.
    pub fn map_marks(&self, bundle: &ChangeBundle) -> Result<Vec<Mark>, ApplyError> {
        let mut scratch = self.clone();
        scratch.apply_text_channels(bundle)?;
        scratch.normalize();
        Ok(scratch.marks)
    }

    fn line_mut(&mut self, line: usize) -> Result<&mut Line, ApplyError> {
        let lines = self.lines.len();
        self.lines
            .get_mut(line)
            .ok_or(ApplyError::LineOutOfRange { line, lines })
    }

    fn split_line(&mut self, at: Usv) -> Result<(), ApplyError> {
        let char_indices: Vec<(usize, char)> = self.text.char_indices().collect();
        let len = char_indices.len();
        if at > len {
            return Err(ApplyError::SplitPositionOutOfRange { at, len });
        }
        if at > 0 && char_indices[at - 1].1 == '\n' {
            return Err(ApplyError::SplitAtNewline { at });
        }
        if at < len && char_indices[at].1 == '\n' {
            return Err(ApplyError::SplitAtNewline { at });
        }

        // The newline count before `at` is the post-insert line index, since the
        // insertion lands at index `at`, not before it.
        let byte = char_indices.get(at).map_or(self.text.len(), |&(b, _)| b);
        let line_idx = char_indices[..at].iter().filter(|&(_, c)| *c == '\n').count();
        self.text.insert(byte, '\n');

        self.rebase_marks(&Delta {
            ops: vec![Op::Retain(at), Op::Insert("\n".to_string())],
        });

        let template = self
            .lines
            .get(line_idx)
            .cloned()
            .unwrap_or_else(|| Line::new(LineKind::Para));
        let mut new_line = template;
        new_line.continues = false;
        self.lines.insert(line_idx + 1, new_line);

        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }

    fn join_line(&mut self, line: usize) -> Result<(), ApplyError> {
        if line + 1 >= self.lines.len() {
            return Err(ApplyError::LineOutOfRange {
                line,
                lines: self.lines.len(),
            });
        }
        let nl = newline_at_line_boundary(&self.text, line)?;
        let byte = char_to_byte(&self.text, nl);
        self.text.remove(byte);

        self.rebase_marks(&Delta {
            ops: vec![Op::Retain(nl), Op::Delete(1)],
        });

        self.lines.remove(line + 1);

        if self.lines.len() != self.segment_count() {
            return Err(ApplyError::LineCountMismatch {
                lines: self.lines.len(),
                segments: self.segment_count(),
            });
        }
        Ok(())
    }
}

fn ranges_overlap(a0: Usv, a1: Usv, b0: Usv, b1: Usv) -> bool {
    a0 < b1 && b0 < a1
}

/// Put every `Op::Insert` under the [`admit_char`] contract — the chars
/// `validate()` rejects, dropped or spaced — borrowing the delta through
/// untouched when none carries one. A raw [`ISLAND_SLOT`] is refused separately.
fn sanitize_inserts(delta: &Delta) -> Cow<'_, Delta> {
    let needs_cleaning = delta
        .ops
        .iter()
        .any(|op| matches!(op, Op::Insert(s) if s.chars().any(|c| admit_char(c) != Some(c))));
    if !needs_cleaning {
        return Cow::Borrowed(delta);
    }
    let ops = delta
        .ops
        .iter()
        .map(|op| match op {
            Op::Insert(s) => Op::Insert(s.chars().filter_map(admit_char).collect()),
            other => other.clone(),
        })
        .collect();
    Cow::Owned(Delta { ops })
}

/// Walk `delta` over `old_chars` once, mirroring both structures the base chars
/// index: `\n` insert/delete in `lines`, and a deleted [`ISLAND_SLOT`] dropping
/// its island. A `Retain`/`Delete` reaching past the end of the base names no
/// char.
///
/// The line cursor sits *in* a line, `cur`; downstream of it is always the
/// untouched original suffix (`rest`), so lines are emitted in order rather than
/// by per-`\n` mid-`Vec` `remove`/`insert`. `cur == None` is the past-the-end
/// state on a malformed content (more `\n` than lines), where a split clones a
/// default line.
///
/// Islands are stored in slot order, so the Nth slot the walk passes backs the
/// Nth island.
fn sync_for_delta(
    old_chars: &[char],
    old_lines: Vec<Line>,
    old_islands: Vec<Island>,
    delta: &Delta,
) -> (Vec<Line>, Vec<Island>) {
    let mut rest = old_lines.into_iter();
    let mut lines: Vec<Line> = Vec::with_capacity(rest.len());
    let mut cur: Option<Line> = rest.next();
    let mut keep = vec![true; old_islands.len()];
    let mut slot = 0usize;
    let mut old = 0usize;

    for op in &delta.ops {
        match op {
            Op::Retain(n) | Op::Delete(n) => {
                let deleted = matches!(op, Op::Delete(_));
                let end = old.saturating_add(*n).min(old_chars.len());
                for &c in &old_chars[old..end] {
                    match c {
                        // A deleted '\n' merges the next original into `cur`.
                        '\n' if deleted => {
                            rest.next();
                        }
                        '\n' => {
                            lines.extend(cur.take());
                            cur = rest.next();
                        }
                        ISLAND_SLOT => {
                            if deleted && let Some(k) = keep.get_mut(slot) {
                                *k = false;
                            }
                            slot += 1;
                        }
                        _ => {}
                    }
                }
                old = end;
            }
            // A raw ISLAND_SLOT insert is rejected before this walk.
            Op::Insert(s) => {
                for c in s.chars() {
                    if c == '\n' {
                        let mut new_line = match cur.take() {
                            Some(line) => {
                                let clone = line.clone();
                                lines.push(line);
                                clone
                            }
                            None => Line::new(LineKind::Para),
                        };
                        new_line.continues = false;
                        cur = Some(new_line);
                    }
                }
            }
        }
    }

    lines.extend(cur);
    lines.extend(rest);
    let islands = old_islands
        .into_iter()
        .zip(keep)
        .filter_map(|(island, keep)| keep.then_some(island))
        .collect();
    (lines, islands)
}

fn newline_at_line_boundary(text: &str, line: usize) -> Result<Usv, ApplyError> {
    let mut current = 0usize;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            if current == line {
                return Ok(i);
            }
            current += 1;
        }
    }
    Err(ApplyError::LineOutOfRange {
        line,
        lines: text.chars().filter(|&c| c == '\n').count() + 1,
    })
}

/// The mutations that re-establish the invariant, forwarded.
///
/// Each normalizes before it returns, on an error too: an op list that fails
/// partway leaves its earlier ops applied. So the token states that the value
/// is canonical, not that the edit landed.
///
/// Any other edit takes [`into_content`](crate::model::Normalized::into_content).
impl crate::model::Normalized {
    fn seal(&mut self, applied: Result<(), ApplyError>) -> Result<(), ApplyError> {
        if applied.is_err() {
            self.as_content_mut().normalize();
        }
        applied
    }

    pub fn apply_text_delta(&mut self, delta: &Delta) -> Result<(), ApplyError> {
        let applied = self.as_content_mut().apply_text_delta(delta);
        self.seal(applied)
    }

    pub fn apply_mark_ops(&mut self, ops: &[MarkOp]) -> Result<(), ApplyError> {
        let applied = self.as_content_mut().apply_mark_ops(ops);
        self.seal(applied)
    }

    pub fn apply_island_ops(&mut self, ops: &[IslandOp]) -> Result<(), ApplyError> {
        let applied = self.as_content_mut().apply_island_ops(ops);
        self.seal(applied)
    }

    pub fn apply_line_ops(&mut self, ops: &[LineOp]) -> Result<(), ApplyError> {
        let applied = self.as_content_mut().apply_line_ops(ops);
        self.seal(applied)
    }

    pub fn apply_field_change(&mut self, bundle: &ChangeBundle) -> Result<(), ApplyError> {
        let applied = self.as_content_mut().apply_field_change(bundle);
        self.seal(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta::diff;
    use crate::import::from_markdown;

    #[test]
    fn mark_op_wire_decodes_each_variant() {
        let cases = vec![
            (
                serde_json::json!({"op": "add", "start": 0, "end": 3, "type": "strong"}),
                MarkOp::Add {
                    start: 0,
                    end: 3,
                    kind: MarkKind::Strong,
                },
            ),
            (
                serde_json::json!({
                    "op": "add", "start": 1, "end": 2, "type": "link", "attrs": {"url": "https://x"},
                }),
                MarkOp::Add {
                    start: 1,
                    end: 2,
                    kind: MarkKind::Link {
                        url: "https://x".into(),
                    },
                },
            ),
            (
                serde_json::json!({
                    "op": "remove", "start": 4, "end": 6, "type": "anchor", "attrs": {"id": "c1"},
                }),
                MarkOp::Remove {
                    start: 4,
                    end: 6,
                    kind: MarkKind::Anchor { id: "c1".into() },
                },
            ),
            (
                serde_json::json!({"op": "removeAnchor", "id": "c2"}),
                MarkOp::RemoveAnchor { id: "c2".into() },
            ),
        ];
        for (v, op) in cases {
            assert_eq!(mark_op_from_value(&v).unwrap(), op, "decode: {v}");
        }
    }

    #[test]
    fn line_op_wire_decodes_each_variant() {
        let cases = vec![
            (
                serde_json::json!({"op": "split", "at": 5}),
                LineOp::Split { at: 5 },
            ),
            (
                serde_json::json!({"op": "join", "line": 1}),
                LineOp::Join { line: 1 },
            ),
            (
                serde_json::json!({"op": "setKind", "line": 0, "kind": "heading", "attrs": {"level": 2}}),
                LineOp::SetKind {
                    line: 0,
                    kind: LineKind::Heading { level: 2 },
                },
            ),
            (
                serde_json::json!({
                    "op": "setContainers", "line": 2, "containers": [{"container": "quote"}],
                }),
                LineOp::SetContainers {
                    line: 2,
                    containers: vec![Container::Quote { instance: 0 }],
                },
            ),
            (
                serde_json::json!({
                    "op": "setKind", "line": 0, "kind": "callout", "attrs": {"variant": "warn"},
                }),
                LineOp::SetKind {
                    line: 0,
                    kind: LineKind::Unknown {
                        tag: "callout".into(),
                        attrs: serde_json::json!({"variant": "warn"}),
                    },
                },
            ),
            (
                serde_json::json!({
                    "op": "setContainers", "line": 2,
                    "containers": [{"container": "indent", "attrs": {"depth": 2}}],
                }),
                LineOp::SetContainers {
                    line: 2,
                    containers: vec![Container::Unknown {
                        tag: "indent".into(),
                        attrs: serde_json::json!({"depth": 2}),
                        instance: 0,
                    }],
                },
            ),
            (
                serde_json::json!({"op": "setContinues", "line": 1, "continues": true}),
                LineOp::SetContinues {
                    line: 1,
                    continues: true,
                },
            ),
            (
                serde_json::json!({"op": "setContinues", "line": 3, "continues": false}),
                LineOp::SetContinues {
                    line: 3,
                    continues: false,
                },
            ),
        ];
        for (v, op) in cases {
            assert_eq!(line_op_from_value(&v).unwrap(), op, "decode: {v}");
        }
    }

    /// The op wire is authored-now, so it refuses the `@0.93.0` payload
    /// spelling the storage lane still reads: a host writing it holds a stale
    /// copy of the encoding, and the write would land where it did not aim.
    #[test]
    fn op_wire_rejects_the_legacy_payload_spelling() {
        let bad = serde_json::json!({
            "op": "setKind", "line": 0, "kind": "heading", "level": 2,
        });
        assert!(matches!(line_op_from_value(&bad), Err(ParseError::Shape(_))));
        let bad = serde_json::json!({
            "op": "setContainers", "line": 0,
            "containers": [{"container": "list_item", "ordered": true}],
        });
        assert!(matches!(line_op_from_value(&bad), Err(ParseError::Shape(_))));
        let bad = serde_json::json!({
            "op": "add", "start": 0, "end": 1, "type": "link", "url": "u",
        });
        assert!(matches!(mark_op_from_value(&bad), Err(ParseError::Shape(_))));

        // One spelling per name: a built-in's payload rides the bag exactly as
        // an unknown's does, and a foreign bag on a built-in drops unread.
        for ok in [
            serde_json::json!({"op": "setKind", "line": 0, "kind": "callout", "attrs": {"tone": "warn"}}),
            serde_json::json!({"op": "setKind", "line": 0, "kind": "heading", "attrs": {"level": 2}}),
            serde_json::json!({"op": "setKind", "line": 0, "kind": "para", "attrs": {"tone": "warn"}}),
        ] {
            assert!(line_op_from_value(&ok).is_ok(), "rejected: {ok}");
        }
    }

    #[test]
    fn delta_serde_shape() {
        let d = Delta {
            ops: vec![Op::Retain(2), Op::Insert("hi".into()), Op::Delete(1)],
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"ops": [{"retain": 2}, {"insert": "hi"}, {"delete": 1}]})
        );
        assert_eq!(serde_json::from_value::<Delta>(v).unwrap(), d);
    }

    #[test]
    fn apply_text_delta_rebases_marks() {
        let mut rt = from_markdown("hello").unwrap().into_content();
        rt.marks.push(Mark {
            start: 1,
            end: 4,
            kind: MarkKind::Strong,
        });
        let mut rt = rt.into_normalized();
        let d = diff("hello", "hXello");
        rt.apply_text_delta(&d).unwrap();
        let strong = rt
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::Strong))
            .unwrap();
        assert_eq!((strong.start, strong.end), (2, 5));
        assert_eq!(rt.text, "hXello");
    }

    fn anchored(text: &str, at: Usv) -> crate::model::Normalized {
        let mut rt = from_markdown(text).unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: at,
            end: at,
            kind: MarkKind::Anchor { id: "a1".into() },
        }])
        .unwrap();
        rt
    }

    fn anchor_at(rt: &Content) -> (Usv, Usv) {
        let m = rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "a1"))
            .expect("the anchor survives");
        (m.start, m.end)
    }

    fn strong_at(rt: &Content) -> (Usv, Usv) {
        let m = rt
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::Strong))
            .expect("the mark survives");
        (m.start, m.end)
    }

    /// A zero-width mark's own position is where the two assocs part, and where
    /// an anchor most often sits. Every text-moving channel answers it `Before`.
    #[test]
    fn an_insert_at_a_zero_width_marks_position_leaves_it_put() {
        let d = diff("hello world", "hello Xworld");
        assert_eq!(d.map_pos(6, Assoc::After), 7, "the answer not taken");

        let mut via_delta = anchored("hello world", 6);
        via_delta.apply_text_delta(&d).unwrap();
        assert_eq!(anchor_at(&via_delta), (6, 6));

        let mut via_island = anchored("hello world", 6);
        via_island
            .apply_island_ops(&[IslandOp::Insert {
                at: 6,
                island: image("i1"),
            }])
            .unwrap();
        assert_eq!(anchor_at(&via_island), (6, 6));

        let mut via_line = anchored("hello world", 6);
        via_line.apply_line_ops(&[LineOp::Split { at: 6 }]).unwrap();
        assert_eq!(anchor_at(&via_line), (6, 6));
    }

    /// An insertion at either edge of a range grows text outside the span.
    #[test]
    fn an_insert_at_a_range_marks_edge_stays_outside_the_span() {
        let mut at_start = from_markdown("hello world").unwrap();
        at_start
            .apply_mark_ops(&[MarkOp::Add {
                start: 6,
                end: 11,
                kind: MarkKind::Strong,
            }])
            .unwrap();
        at_start
            .apply_text_delta(&diff("hello world", "hello Xworld"))
            .unwrap();
        assert_eq!(strong_at(&at_start), (7, 12));

        let mut at_end = from_markdown("hello world").unwrap();
        at_end
            .apply_mark_ops(&[MarkOp::Add {
                start: 0,
                end: 5,
                kind: MarkKind::Strong,
            }])
            .unwrap();
        at_end
            .apply_text_delta(&diff("hello world", "helloX world"))
            .unwrap();
        assert_eq!(strong_at(&at_end), (0, 5));

        let mut at_end_island = from_markdown("hello world").unwrap();
        at_end_island
            .apply_mark_ops(&[MarkOp::Add {
                start: 0,
                end: 5,
                kind: MarkKind::Strong,
            }])
            .unwrap();
        at_end_island
            .apply_island_ops(&[IslandOp::Insert {
                at: 5,
                island: image("i1"),
            }])
            .unwrap();
        assert_eq!(strong_at(&at_end_island), (0, 5));
    }

    /// The reason an editor can diff against `map_marks` rather than reproduce
    /// the rebase: both readings walk one channel list, over every channel at
    /// once and at the position the assocs disagree on.
    #[test]
    fn map_marks_reports_where_apply_field_change_puts_them() {
        let mut rt = anchored("hello world", 6);
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 5,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        let bundle = ChangeBundle {
            delta: diff("hello world", "hello Xworld"),
            island_ops: vec![IslandOp::Insert {
                at: 6,
                island: image("i1"),
            }],
            line_ops: vec![LineOp::Split { at: 6 }],
            mark_ops: Vec::new(),
        };

        let predicted = rt.map_marks(&bundle).unwrap();
        rt.apply_field_change(&bundle).unwrap();
        assert_eq!(predicted, rt.marks);
        assert_eq!(anchor_at(&rt), (6, 6), "the anchor never left its position");
    }

    /// A move can leave two same-kind runs touching, and the store unions them.
    /// An editor holding the returned marks as its own model of the field would
    /// otherwise disagree with the next read.
    #[test]
    fn map_marks_reports_the_union_a_move_makes_adjacent() {
        // Both bundle shapes: `apply_field_change` splits on `is_delta_only`,
        // and the answer must match the store on either side of that split.
        for line_ops in [
            Vec::new(),
            vec![LineOp::SetKind {
                line: 0,
                kind: LineKind::Para,
            }],
        ] {
            let mut rt = from_markdown("ab cd").unwrap();
            rt.apply_mark_ops(&[
                MarkOp::Add {
                    start: 0,
                    end: 2,
                    kind: MarkKind::Strong,
                },
                MarkOp::Add {
                    start: 3,
                    end: 5,
                    kind: MarkKind::Strong,
                },
            ])
            .unwrap();
            let bundle = ChangeBundle {
                delta: diff("ab cd", "abcd"),
                line_ops,
                ..Default::default()
            };

            let predicted = rt.map_marks(&bundle).unwrap();
            rt.apply_field_change(&bundle).unwrap();
            assert_eq!(predicted, rt.marks);
            assert_eq!(strong_at(&rt), (0, 4), "the two runs are one");
        }
    }

    /// `map_marks` reads; a bundle it rejects leaves the content alone.
    #[test]
    fn map_marks_reports_an_out_of_bounds_bundle_without_touching_the_content() {
        let rt = anchored("hello world", 6);
        let before = rt.clone();
        let err = rt
            .map_marks(&ChangeBundle {
                island_ops: vec![IslandOp::Insert {
                    at: 99,
                    island: image("i1"),
                }],
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, ApplyError::IslandInsertOutOfRange { .. }));
        assert_eq!(rt.marks, before.marks);
        assert_eq!(rt.text, before.text);
    }

    #[test]
    fn apply_text_delta_pads_short_prepend() {
        // A prepend naming only its inserted text (no trailing retain) still
        // splices against the whole content.
        let mut rt = from_markdown("hello").unwrap();
        rt.apply_text_delta(&Delta {
            ops: vec![Op::Insert("NEW ".into())],
        })
        .unwrap();
        assert_eq!(rt.text, "NEW hello");
    }

    #[test]
    fn apply_text_delta_rejects_over_long_delta() {
        // Consuming more base than exists is a wrong-revision delta, not an
        // abbreviated one.
        let mut rt = from_markdown("hi").unwrap();
        assert!(matches!(
            rt.apply_text_delta(&Delta {
                ops: vec![Op::Retain(99)],
            }),
            Err(ApplyError::DeltaBaseMismatch { .. })
        ));
        assert_eq!(rt.text, "hi");
    }

    #[test]
    fn apply_field_change_rejects_a_bundle_whose_retains_overflow() {
        // The whole host lane: JSON bundle through the store, not a Delta
        // built in Rust.
        let bundle = change_bundle_from_value(&serde_json::json!({
            "delta": { "ops": [{ "retain": usize::MAX }, { "retain": 2 }] }
        }))
        .unwrap();
        let mut rt = from_markdown("hi").unwrap();
        assert!(matches!(
            rt.apply_field_change(&bundle),
            Err(ApplyError::DeltaBaseMismatch { .. })
        ));
        assert_eq!(rt.text, "hi");
    }

    #[test]
    fn apply_mark_ops_remove_punches_hole() {
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 6,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 2,
            end: 4,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(0, 2), (4, 6)]);
    }

    #[test]
    fn apply_mark_ops_remove_at_edge_leaves_no_zero_width() {
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 6,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 0,
            end: 2,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(2, 6)]);
    }

    #[test]
    fn apply_mark_ops_remove_covering_range_drops_mark() {
        let mut rt = from_markdown("abcdef").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 2,
            end: 4,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 0,
            end: 6,
            kind: MarkKind::Emph,
        }])
        .unwrap();
        assert!(!rt.marks.iter().any(|m| matches!(m.kind, MarkKind::Emph)));
    }

    #[test]
    fn apply_mark_ops_remove_non_formatting_drops_whole() {
        // `Value::Null` on both sides: the one in-memory spelling of an empty
        // bag, which `normalize` and the wire decode both settle on.
        let unknown = || MarkKind::Unknown {
            tag: "x".into(),
            attrs: Value::Null,
        };
        let mut rt = from_markdown("abcdef").unwrap().into_content();
        rt.marks.push(Mark {
            start: 0,
            end: 6,
            kind: unknown(),
        });
        let mut rt = rt.into_normalized();
        rt.apply_mark_ops(&[MarkOp::Remove {
            start: 2,
            end: 4,
            kind: unknown(),
        }])
        .unwrap();
        assert!(!rt
            .marks
            .iter()
            .any(|m| matches!(m.kind, MarkKind::Unknown { .. })));
    }

    #[test]
    fn line_op_split_and_join() {
        let mut rt = from_markdown("onetwo").unwrap();
        rt.apply_line_ops(&[LineOp::Split { at: 3 }]).unwrap();
        assert_eq!(rt.text, "one\ntwo");
        assert_eq!(rt.lines.len(), 2);

        rt.apply_line_ops(&[LineOp::Join { line: 0 }]).unwrap();
        assert_eq!(rt.text, "onetwo");
        assert_eq!(rt.lines.len(), 1);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn line_op_set_kind() {
        let mut rt = from_markdown("title").unwrap();
        rt.apply_line_ops(&[LineOp::SetKind {
            line: 0,
            kind: LineKind::Heading { level: 2 },
        }])
        .unwrap();
        assert!(matches!(rt.lines[0].kind, LineKind::Heading { level: 2 }));
    }

    #[test]
    fn line_op_set_kind_refuses_a_kind_the_text_contradicts() {
        let mut rt = from_markdown("hello world").unwrap();
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Island,
            }]),
            Err(ApplyError::LineKindMismatch {
                line: 0,
                mismatch: LineKindMismatch::IslandNotOneSlot,
            })
        );
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Rule,
            }]),
            Err(ApplyError::LineKindMismatch {
                line: 0,
                mismatch: LineKindMismatch::RuleNotEmpty,
            })
        );
        assert_eq!(rt.text, "hello world");
        assert_eq!(rt.lines[0].kind, LineKind::Para);
        assert_eq!(rt.validate(), Ok(()));

        // Tagging a table island's line `Code` would fence the slot, which
        // re-imports as nothing.
        let mut tbl = from_markdown("| a | b |\n|---|---|\n| 1 | 2 |").unwrap();
        assert_eq!(
            tbl.apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Code { lang: None },
            }]),
            Err(ApplyError::LineKindMismatch {
                line: 0,
                mismatch: LineKindMismatch::CodeHasSlot,
            })
        );
        assert_eq!(tbl.lines[0].kind, LineKind::Island);
    }

    /// The deliberate crossing is refused up front, where the incidental one
    /// (a `Join` across two paths) is repaired by `normalize`: the same split
    /// the line-kind rule makes.
    #[test]
    fn set_continues_across_a_container_boundary_is_refused() {
        let mut rt = from_markdown("- a\n\npara").unwrap();
        assert_ne!(rt.lines[0].containers, rt.lines[1].containers);
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetContinues {
                line: 1,
                continues: true
            }]),
            Err(ApplyError::ContinuesAcrossContainers { line: 1 })
        );

        // Inside one container it is an ordinary hard break.
        let mut rt = from_markdown("- a\n\n  b").unwrap();
        assert_eq!(rt.lines[0].containers, rt.lines[1].containers);
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetContinues {
                line: 1,
                continues: true
            }]),
            Ok(())
        );
        assert!(rt.lines[1].continues);
    }

    /// A heading, an island and a rule are one line in both projections, so a
    /// line continuing one is text neither emitter reaches. Refused on the
    /// deliberate channel, exactly as the container crossing is.
    #[test]
    fn set_continues_after_a_single_line_block_is_refused() {
        for markdown in ["# a\n\nb", "| h |\n| --- |\n| c |\n\nb", "***\n\nb"] {
            let mut rt = from_markdown(markdown).unwrap();
            let line = rt.lines.len() - 1;
            assert_eq!(
                rt.apply_line_ops(&[LineOp::SetContinues {
                    line,
                    continues: true
                }]),
                Err(ApplyError::ContinuesSingleLineBlock { line }),
                "{markdown}"
            );
            assert!(!rt.lines[line].continues);
            assert_eq!(crate::export::to_markdown(&rt), markdown, "{markdown}");
        }

        // `SetKind` reaches the shape from the other side, by retagging the
        // block a continuation already follows. That retag is accepted and the
        // terminal `normalize` clears the flag, so the continuation lands as
        // the paragraph it is rather than vanishing.
        let mut rt = from_markdown("a\\\nb").unwrap();
        assert!(rt.lines[1].continues, "a hard break is a continuation");
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Heading { level: 1 },
            }]),
            Ok(())
        );
        assert!(!rt.lines[1].continues);
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(crate::export::to_markdown(&rt), "# a\n\nb");
    }

    /// A `Join` merging two lines of differing paths leaves the line after the
    /// seam continuing across it. The op is accepted and the content is still
    /// storable, which is what putting the repair in `normalize` rather than
    /// `validate` buys.
    #[test]
    fn join_across_two_paths_leaves_a_valid_content() {
        let mut rt = from_markdown("- a\n\npara\\\nbroken").unwrap();
        let seam = rt
            .lines
            .iter()
            .position(|l| l.continues)
            .expect("the hard break is there");
        assert!(rt.apply_line_ops(&[LineOp::Join { line: seam - 2 }]).is_ok());
        assert_eq!(rt.validate(), Ok(()), "the join left a storable content");
    }

    #[test]
    fn line_op_set_containers_is_depth_capped() {
        let mut rt = from_markdown("hi").unwrap();
        let deep = vec![Container::Quote { instance: 0 }; crate::MAX_NESTING_DEPTH + 1];
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetContainers {
                line: 0,
                containers: deep,
            }]),
            Err(ApplyError::NestingTooDeep {
                line: 0,
                depth: crate::MAX_NESTING_DEPTH + 1,
                max: crate::MAX_NESTING_DEPTH,
            })
        );
        assert!(rt.lines[0].containers.is_empty());
    }

    #[test]
    fn line_op_set_kind_range_checks_the_heading_level() {
        let mut rt = from_markdown("t").unwrap();
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Heading { level: 9 },
            }]),
            Err(ApplyError::BadHeadingLevel { line: 0, level: 9 })
        );
        assert_eq!(rt.validate(), Ok(()));
        assert!(rt
            .apply_line_ops(&[LineOp::SetKind {
                line: 0,
                kind: LineKind::Heading { level: 6 },
            }])
            .is_ok());
    }

    #[test]
    fn line_op_set_continues_sets_and_clears() {
        let mut rt = from_markdown("one two").unwrap();
        rt.apply_text_delta(&diff("one two", "one\ntwo")).unwrap();
        assert!(!rt.lines[1].continues, "delta-split newline is a new block");

        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 1,
            continues: true,
        }])
        .unwrap();
        assert!(rt.lines[1].continues);
        assert_eq!(rt.validate(), Ok(()));
        assert_eq!(
            crate::export::to_markdown(&rt).matches("\n\n").count(),
            0,
            "a within-block hard break is not a paragraph boundary"
        );

        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 1,
            continues: false,
        }])
        .unwrap();
        assert!(!rt.lines[1].continues);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn line_op_set_continues_rejects_first_line() {
        let mut rt = from_markdown("one two").unwrap();
        rt.apply_text_delta(&diff("one two", "one\ntwo")).unwrap();
        let before = rt.clone();
        assert_eq!(
            rt.apply_line_ops(&[LineOp::SetContinues {
                line: 0,
                continues: true,
            }]),
            Err(ApplyError::FirstLineContinues)
        );
        assert_eq!(rt, before, "rejected op leaves the content untouched");
        // Clearing line 0 is a no-op, not an error.
        rt.apply_line_ops(&[LineOp::SetContinues {
            line: 0,
            continues: false,
        }])
        .unwrap();
        assert_eq!(rt.validate(), Ok(()));
    }

    fn island(id: &str) -> Island {
        Island {
            id: id.into(),
            island_type: "image".into(),
            props: serde_json::json!({}),
            loss: crate::model::Loss::LOSSLESS,
        }
    }

    #[test]
    fn delete_one_of_two_slots_removes_the_matching_island() {
        let mut rt = Content::empty();
        rt.text = format!("{ISLAND_SLOT}x{ISLAND_SLOT}");
        rt.lines = vec![Line {
            kind: LineKind::Para,
            containers: vec![],
            continues: false,
        }];
        rt.islands = vec![island("first"), island("second")];
        assert_eq!(rt.validate(), Ok(()));

        // Delete the FIRST slot (index 0): `￼x￼` -> `x￼`.
        let d = Delta {
            ops: vec![Op::Delete(1), Op::Retain(2)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, format!("x{ISLAND_SLOT}"));
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, "second");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_bidi_control_is_stripped() {
        // Import's Trojan-source defense is not bypassed by the delta channel.
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![
                Op::Retain(1),
                Op::Insert("\u{202E}".into()),
                Op::Retain(1),
            ],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "ab");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_line_separator_is_spaced() {
        // A space keeps the words apart without minting the line break Typst
        // would read, and which would make `- item` a bullet.
        for sep in ['\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}'] {
            let mut rt = from_markdown("ab").unwrap();
            let d = Delta {
                ops: vec![Op::Retain(2), Op::Insert(format!("{sep}- item"))],
            };
            rt.apply_text_delta(&d).unwrap();
            assert_eq!(rt.text, "ab - item", "for {sep:?}");
            assert_eq!(rt.lines.len(), 1);
            assert_eq!(rt.validate(), Ok(()));
        }
    }

    #[test]
    fn insert_crlf_keeps_the_newline_and_splits() {
        let mut rt = from_markdown("ab").unwrap();
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("\r\n".into()), Op::Retain(1)],
        };
        rt.apply_text_delta(&d).unwrap();
        assert_eq!(rt.text, "a\nb");
        assert_eq!(rt.lines.len(), 2);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn insert_of_clean_text_is_not_reallocated() {
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Insert("clean\n".into()), Op::Retain(1)],
        };
        assert!(matches!(sanitize_inserts(&d), Cow::Borrowed(_)));
    }

    fn mark_bundle(delta: Delta, mark_ops: Vec<MarkOp>) -> ChangeBundle {
        ChangeBundle {
            delta,
            mark_ops,
            ..Default::default()
        }
    }

    fn island_bundle(island_ops: Vec<IslandOp>) -> ChangeBundle {
        ChangeBundle {
            island_ops,
            ..Default::default()
        }
    }

    /// A one-cell table island's props, a shape `normalize` and `validate` accept.
    fn table_props(header: &str, cell: &str) -> serde_json::Value {
        serde_json::json!({
            "header": [{ "text": header, "marks": [] }],
            "rows": [[{ "text": cell, "marks": [] }]],
            "aligns": ["none"],
        })
    }

    fn image(id: &str) -> Island {
        Island::new(id.into(), "image".into())
            .with_props(serde_json::json!({ "url": "u", "alt": "a" }))
    }

    #[test]
    fn island_op_wire_decodes_each_variant() {
        let island = Island::new("isl-0".into(), "table".into())
            .with_props(table_props("H", "a"))
            .with_loss(crate::model::Loss::DEGRADED);
        let cases = vec![
            (
                serde_json::json!({
                    "op": "set", "id": "isl-0", "type": "table",
                    "props": table_props("H", "a"), "loss": "degraded",
                }),
                IslandOp::Set {
                    island: island.clone(),
                },
            ),
            (
                serde_json::json!({
                    "op": "insert", "at": 7, "id": "isl-0", "type": "table",
                    "props": table_props("H", "a"), "loss": "degraded",
                }),
                IslandOp::Insert { at: 7, island },
            ),
        ];
        for (v, op) in cases {
            assert_eq!(island_op_from_value(&v).unwrap(), op, "decode: {v}");
        }
    }

    /// An island payload edit moves the entry alone, so an anchor elsewhere in
    /// the field survives an edit a whole-value `install` would have cleared.
    #[test]
    fn island_set_edits_props_and_keeps_the_field_anchors() {
        let mut rt = from_markdown("intro\n\n| H |\n| --- |\n| a |").unwrap();
        assert_eq!(rt.islands.len(), 1, "one table island");
        let id = rt.islands[0].id.clone();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 5,
            kind: MarkKind::Anchor { id: "c1".into() },
        }])
        .unwrap();

        rt.apply_field_change(&island_bundle(vec![IslandOp::Set {
            island: Island::new(id.clone(), "table".into()).with_props(table_props("H", "b")),
        }]))
        .unwrap();

        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, id, "the id is target and stored value");
        assert_eq!(rt.islands[0].props, table_props("H", "b"));
        let anchor = rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("the anchor above the table survives the island edit");
        assert_eq!((anchor.start, anchor.end), (0, 5));
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn island_set_rejects_an_unknown_id() {
        let mut rt = from_markdown("| H |\n| --- |\n| a |").unwrap();
        let before = rt.clone();
        assert_eq!(
            rt.apply_field_change(&island_bundle(vec![IslandOp::Set {
                island: Island::new("isl-nope".into(), "table".into())
                    .with_props(table_props("H", "b")),
            }])),
            Err(ApplyError::UnknownIslandId {
                id: "isl-nope".into()
            })
        );
        assert_eq!(rt, before);
    }

    #[test]
    fn island_insert_adds_the_slot_and_its_entry() {
        let mut rt = from_markdown("ab").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 1,
            kind: MarkKind::Anchor { id: "c1".into() },
        }])
        .unwrap();

        rt.apply_field_change(&island_bundle(vec![IslandOp::Insert {
            at: 1,
            island: Island::new("isl-new".into(), "image".into())
                .with_props(serde_json::json!({ "url": "u", "alt": "a" })),
        }]))
        .unwrap();

        assert_eq!(rt.text, format!("a{ISLAND_SLOT}b"));
        assert_eq!(rt.islands.len(), 1);
        assert_eq!(rt.islands[0].id, "isl-new");
        assert_eq!(rt.validate(), Ok(()), "slot count matches the island list");
        let anchor = rt
            .marks
            .iter()
            .find(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1"))
            .expect("anchor survives");
        assert_eq!((anchor.start, anchor.end), (0, 1));
    }

    /// Op *n*'s `at` counts the slots ops `0..n` already spliced, not the shared
    /// post-delta frame; both assertions land differently under the
    /// post-delta-only reading, which errors neither way.
    #[test]
    fn island_inserts_apply_in_sequence() {
        let mut rt = from_markdown("xabc").unwrap();
        rt.apply_field_change(&ChangeBundle {
            // Post-delta: the deleted `x` is out of the frame the ops read.
            delta: diff("xabc", "abc"),
            island_ops: vec![
                IslandOp::Insert {
                    at: 1,
                    island: image("isl-b"),
                },
                // 3, not 2: op 0's slot is in the frame this op reads.
                IslandOp::Insert {
                    at: 3,
                    island: image("isl-c"),
                },
                // An earlier position emitted last, so its slot lands first.
                IslandOp::Insert {
                    at: 1,
                    island: image("isl-a"),
                },
            ],
            ..Default::default()
        })
        .unwrap();

        assert_eq!(
            rt.text,
            format!("a{ISLAND_SLOT}{ISLAND_SLOT}b{ISLAND_SLOT}c")
        );
        let ids: Vec<&str> = rt.islands.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["isl-a", "isl-b", "isl-c"], "slot order, not emission");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn slot_bearing_splice_splits_into_delta_and_insert() {
        let mut rt = from_markdown("ab").unwrap();
        let before = rt.clone();

        let paste = format!("x{ISLAND_SLOT}y");
        assert_eq!(
            rt.apply_field_change(&ChangeBundle::from_delta(Delta {
                ops: vec![Op::Retain(1), Op::Insert(paste)],
            })),
            Err(ApplyError::IslandSlotInInsert)
        );
        assert_eq!(rt, before, "the refusal commits nothing");

        rt.apply_field_change(&ChangeBundle {
            delta: Delta {
                ops: vec![Op::Retain(1), Op::Insert("xy".into())],
            },
            // The delta leaves `axyb`; the slot goes between `x` and `y`.
            island_ops: vec![IslandOp::Insert {
                at: 2,
                island: image("isl-p"),
            }],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(rt.text, format!("ax{ISLAND_SLOT}yb"));
        assert_eq!(rt.islands[0].id, "isl-p");
        assert_eq!(rt.validate(), Ok(()));
    }

    /// A block island's line demotes to `Para` when its slot goes: the kind
    /// stops matching the text and `normalize` repairs rather than fails.
    #[test]
    fn block_island_restore_retags_its_line() {
        let mut rt = from_markdown("intro").unwrap();
        rt.apply_field_change(&ChangeBundle {
            delta: diff("intro", "intro\n"),
            island_ops: vec![IslandOp::Insert {
                at: 6,
                island: image("isl-a"),
            }],
            line_ops: vec![LineOp::SetKind {
                line: 1,
                kind: LineKind::Island,
            }],
            ..Default::default()
        })
        .unwrap();
        let before = rt.clone();
        let held = rt.islands[0].clone();

        rt.apply_field_change(&ChangeBundle::from_delta(diff(&before.text, "intro\n")))
            .unwrap();
        assert!(rt.islands.is_empty());
        assert_eq!(rt.lines[1].kind, LineKind::Para, "demoted, not failed");

        // The line stayed open, so the restore needs no delta.
        rt.apply_field_change(&ChangeBundle {
            island_ops: vec![IslandOp::Insert { at: 6, island: held }],
            line_ops: vec![LineOp::SetKind {
                line: 1,
                kind: LineKind::Island,
            }],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(rt, before, "same content, original id and kind included");
    }

    /// An inserted island's id is caller-supplied on an anchor id's terms:
    /// non-empty and unused, since `Set` addresses by it.
    #[test]
    fn island_insert_id_and_position_rules() {
        let mut rt = from_markdown("ab").unwrap();
        assert_eq!(
            rt.apply_field_change(&island_bundle(vec![IslandOp::Insert {
                at: 1,
                island: image(""),
            }])),
            Err(ApplyError::EmptyIslandId)
        );
        assert_eq!(
            rt.apply_field_change(&island_bundle(vec![IslandOp::Insert {
                at: 9,
                island: image("isl-a"),
            }])),
            Err(ApplyError::IslandInsertOutOfRange { at: 9, len: 2 })
        );

        rt.apply_field_change(&island_bundle(vec![IslandOp::Insert {
            at: 1,
            island: image("isl-a"),
        }]))
        .unwrap();
        assert_eq!(
            rt.apply_field_change(&island_bundle(vec![IslandOp::Insert {
                at: 0,
                island: image("isl-a"),
            }])),
            Err(ApplyError::IslandIdCollision { id: "isl-a".into() })
        );
    }

    /// What the stage order buys: the delta opens the line, the island op fills
    /// it, `SetKind` tags it, and the field's anchors stay.
    #[test]
    fn block_island_lands_in_one_bundle() {
        let mut rt = from_markdown("intro").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 0,
            end: 5,
            kind: MarkKind::Anchor { id: "c1".into() },
        }])
        .unwrap();

        rt.apply_field_change(&ChangeBundle {
            delta: diff("intro", "intro\n"),
            island_ops: vec![IslandOp::Insert {
                at: 6,
                island: Island::new("isl-t".into(), "table".into())
                    .with_props(table_props("H", "a")),
            }],
            line_ops: vec![LineOp::SetKind {
                line: 1,
                kind: LineKind::Island,
            }],
            ..Default::default()
        })
        .unwrap();

        assert_eq!(rt.text, format!("intro\n{ISLAND_SLOT}"));
        assert_eq!(rt.lines[1].kind, LineKind::Island);
        assert_eq!(rt.validate(), Ok(()));
        assert!(rt
            .marks
            .iter()
            .any(|m| matches!(&m.kind, MarkKind::Anchor { id } if id == "c1")));
        assert!(
            crate::export::to_markdown(&rt).contains("| H |"),
            "the block island projects as a pipe table"
        );
    }

    #[test]
    fn apply_field_change_bundle_order() {
        let mut rt = from_markdown("abc").unwrap();
        let d = diff("abc", "abXc");
        rt.apply_field_change(&mark_bundle(
            d,
            vec![MarkOp::Add {
                start: 3,
                end: 4,
                kind: MarkKind::Strong,
            }],
        ))
        .unwrap();
        let strong = rt
            .marks
            .iter()
            .find(|m| matches!(m.kind, MarkKind::Strong))
            .unwrap();
        assert_eq!((strong.start, strong.end), (3, 4));
        assert_eq!(rt.text, "abXc");
    }

    #[test]
    fn apply_field_change_is_all_or_nothing() {
        let mut rt = from_markdown("abc").unwrap();
        let before = rt.clone();
        let d = diff("abc", "abXc");
        let err = rt.apply_field_change(&mark_bundle(
            d,
            vec![
                MarkOp::Add {
                    start: 0,
                    end: 2,
                    kind: MarkKind::Strong,
                },
                MarkOp::Add {
                    start: 99,
                    end: 100,
                    kind: MarkKind::Emph,
                },
            ],
        ));
        assert!(matches!(err, Err(ApplyError::MarkOutOfRange { .. })));
        assert_eq!(rt, before, "failed bundle must not mutate the content");
    }

    #[test]
    fn add_anchor_id_uniqueness() {
        let anchor = |id: &str| MarkKind::Anchor { id: id.into() };
        let add = |start, end, id: &str| MarkOp::Add {
            start,
            end,
            kind: anchor(id),
        };

        let noop = || diff("abcd", "abcd");

        let mut rt = from_markdown("abcd").unwrap();
        rt.apply_field_change(&mark_bundle(noop(), vec![add(0, 2, "x")]))
            .unwrap();
        assert_eq!(
            rt.apply_field_change(&mark_bundle(noop(), vec![add(2, 4, "x")])),
            Err(ApplyError::AnchorIdCollision { id: "x".into() })
        );

        let mut rt = from_markdown("abcd").unwrap();
        assert_eq!(
            rt.apply_field_change(&mark_bundle(noop(), vec![add(0, 2, "")])),
            Err(ApplyError::EmptyAnchorId)
        );

        // Remove-then-add of the same id in one bundle: ops apply in sequence,
        // so the id is free by the time the `add` runs.
        let mut rt = from_markdown("abcd").unwrap();
        rt.apply_field_change(&mark_bundle(noop(), vec![add(0, 2, "x")]))
            .unwrap();
        rt.apply_field_change(&mark_bundle(
            noop(),
            vec![MarkOp::RemoveAnchor { id: "x".into() }, add(2, 4, "x")],
        ))
        .unwrap();
        let anchors: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Anchor { .. }))
            .collect();
        assert_eq!(anchors.len(), 1);
        assert_eq!((anchors[0].start, anchors[0].end), (2, 4));
    }

    /// A `Heading{level}` line, its level a visible tag so a test can trace
    /// which original line landed where.
    fn tag_line(level: u8, continues: bool) -> Line {
        Line {
            kind: LineKind::Heading { level },
            containers: Vec::new(),
            continues,
        }
    }

    /// [`sync_for_delta`] on island-free content.
    fn sync_lines(old_chars: &[char], old_lines: Vec<Line>, delta: &Delta) -> Vec<Line> {
        sync_for_delta(old_chars, old_lines, Vec::new(), delta).0
    }

    /// `(tag, continues)` per line: a heading's level, 0 for `Para` (the default
    /// line), 255 for anything else.
    fn tags(lines: &[Line]) -> Vec<(u8, bool)> {
        lines
            .iter()
            .map(|l| match l.kind {
                LineKind::Heading { level } => (level, l.continues),
                LineKind::Para => (0, l.continues),
                _ => (255, l.continues),
            })
            .collect()
    }

    #[test]
    fn sync_lines_insert_newline_clones_split_line_and_clears_continues() {
        let old_chars: Vec<char> = "a\nbc".chars().collect();
        let l1 = Line {
            kind: LineKind::Heading { level: 5 },
            containers: vec![Container::Quote { instance: 0 }],
            continues: true,
        };
        let lines = vec![tag_line(1, false), l1.clone()];
        // Retain(3)[a\nb] moves to line 1; Insert("\n") splits it; Retain(1)[c].
        let d = Delta {
            ops: vec![Op::Retain(3), Op::Insert("\n".into()), Op::Retain(1)],
        };
        let out = sync_lines(&old_chars, lines, &d);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], l1, "first half is the untouched original line");
        assert_eq!(out[2].kind, LineKind::Heading { level: 5 });
        assert_eq!(out[2].containers, vec![Container::Quote { instance: 0 }]);
        assert!(!out[2].continues, "the split clone starts a new block");
    }

    #[test]
    fn sync_lines_delete_newline_drops_following_line() {
        let old_chars: Vec<char> = "a\nb\nc".chars().collect();
        let lines = vec![tag_line(1, false), tag_line(2, false), tag_line(3, false)];
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Delete(1), Op::Retain(3)],
        };
        let out = sync_lines(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(1, false), (3, false)]);
    }

    #[test]
    fn sync_walks_lines_and_islands_off_one_cursor() {
        // One delete run crosses a slot and then a '\n'; the retain behind it
        // has to land on slot 1, and the merged line has to be line 1's.
        let old_chars: Vec<char> = format!("{ISLAND_SLOT}\n{ISLAND_SLOT}").chars().collect();
        let d = Delta {
            ops: vec![Op::Delete(2), Op::Retain(1)],
        };
        let (lines, islands) = sync_for_delta(
            &old_chars,
            vec![tag_line(1, false), tag_line(2, false)],
            vec![island("first"), island("second")],
            &d,
        );
        assert_eq!(tags(&lines), vec![(1, false)]);
        assert_eq!(islands.iter().map(|i| &i.id).collect::<Vec<_>>(), ["second"]);
    }

    #[test]
    fn sync_lines_delete_trailing_newline_without_following_line_is_guarded() {
        // Malformed content: "a\n" is two segments but `lines` has one entry.
        let old_chars: Vec<char> = "a\n".chars().collect();
        let lines = vec![tag_line(1, false)];
        let d = Delta {
            ops: vec![Op::Retain(1), Op::Delete(1)],
        };
        let out = sync_lines(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(1, false)]);
    }

    #[test]
    fn sync_lines_stops_at_end_of_old_chars() {
        let old_chars: Vec<char> = "a\nb".chars().collect();
        let lines = vec![tag_line(1, false), tag_line(2, false)];
        let d = Delta {
            ops: vec![Op::Retain(99)],
        };
        assert_eq!(sync_lines(&old_chars, lines.clone(), &d), lines);
    }

    #[test]
    fn split_line_rebases_mark_across_the_split_point() {
        let mut rt = from_markdown("abcd").unwrap();
        rt.apply_mark_ops(&[MarkOp::Add {
            start: 1,
            end: 3,
            kind: MarkKind::Strong,
        }])
        .unwrap();
        rt.apply_line_ops(&[LineOp::Split { at: 2 }]).unwrap();
        assert_eq!(rt.text, "ab\ncd");
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        // [1..4) spans "b\nc": normalize keeps an interior `\n`, trimming only
        // leading/trailing boundaries.
        assert_eq!(strong, vec![(1, 4)]);
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn join_line_rebases_marks_to_final_text_coordinates() {
        let mut rt = from_markdown("ab").unwrap().into_content();
        rt.apply_text_delta(&diff("ab", "ab\ncd")).unwrap();
        rt.marks.push(Mark {
            start: 2,
            end: 4,
            kind: MarkKind::Strong,
        });
        let mut rt = rt.into_normalized();
        rt.apply_line_ops(&[LineOp::Join { line: 0 }]).unwrap();
        assert_eq!(rt.text, "abcd");
        let strong: Vec<_> = rt
            .marks
            .iter()
            .filter(|m| matches!(m.kind, MarkKind::Strong))
            .map(|m| (m.start, m.end))
            .collect();
        assert_eq!(strong, vec![(2, 3)], "strong lands on 'c', not 'd' or 'cd'");
        assert_eq!(rt.validate(), Ok(()));
    }

    #[test]
    fn field_change_terminal_normalize_matches_per_stage_normalize() {
        let start = from_markdown("hello world").unwrap();
        let text_delta = diff("hello world", "hello brave world");
        let line_ops = vec![LineOp::Split { at: 5 }]; // after "hello"
        let mark_ops = vec![MarkOp::Add {
            start: 0,
            end: 5,
            kind: MarkKind::Strong,
        }];

        let mut bundled = start.clone();
        bundled
            .apply_field_change(&ChangeBundle {
                delta: text_delta.clone(),
                line_ops: line_ops.clone(),
                mark_ops: mark_ops.clone(),
                ..Default::default()
            })
            .unwrap();

        let mut staged = start;
        staged.apply_text_delta(&text_delta).unwrap();
        staged.apply_line_ops(&line_ops).unwrap();
        staged.apply_mark_ops(&mark_ops).unwrap();

        assert_eq!(bundled, staged, "terminal normalize diverged from per-stage");
        assert_eq!(bundled.validate(), Ok(()));
    }

    #[test]
    fn sync_lines_select_all_delete_collapses_to_first_line() {
        let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
        let old_chars: Vec<char> = text.chars().collect();
        let lines: Vec<Line> = (0..=50).map(|i| tag_line((i % 200) as u8, false)).collect();
        assert_eq!(lines.len(), old_chars.iter().filter(|&&c| c == '\n').count() + 1);
        let d = Delta {
            ops: vec![Op::Delete(old_chars.len())],
        };
        let out = sync_lines(&old_chars, lines, &d);
        assert_eq!(tags(&out), vec![(0, false)], "only the first line survives");
    }

    #[test]
    fn sync_lines_insert_newline_past_end_appends_default() {
        // Malformed content: after the retain walks past the sole line, an
        // inserted '\n' has no line to clone and appends a default Para.
        let old_chars: Vec<char> = "a\n".chars().collect();
        let lines = vec![tag_line(1, false)];
        let d = Delta {
            ops: vec![Op::Retain(2), Op::Insert("\n".into())],
        };
        let out = sync_lines(&old_chars, lines, &d);
        assert_eq!(out.len(), 2);
        assert_eq!(tags(&out)[0], (1, false));
        assert_eq!(out[1].kind, LineKind::Para);
        assert!(out[1].containers.is_empty());
        assert!(!out[1].continues);
    }
}
