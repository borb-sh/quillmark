//! The content model: one rich-text value per field. Flat text over one
//! coordinate space of Unicode scalar values, a block record per
//! `\n`-separated segment, containers by id, range marks, anchors, and islands
//! whose slots hold content of their own.

use crate::ids::is_id;
use crate::lines::island::IslandType;
use crate::lines::model::{self as line, is_font_family_name};
use crate::lines::normalize::{is_bidi_char, is_line_separator};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};

/// A position counted in Unicode scalar values.
pub type Usv = usize;

/// The slot an island occupies in the text.
pub const ISLAND_SLOT: char = '\u{FFFC}';

/// A hard line break inside a block.
pub const HARD_BREAK: char = '\u{2028}';

/// The slot a footnote's note lives in.
pub const NOTE_SLOT: &str = "note";

/// One content field.
#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    /// `\n` ends a block, [`ISLAND_SLOT`] is an island's slot and
    /// [`HARD_BREAK`] a break inside a block.
    pub text: String,
    /// One record per `\n`-separated segment, in order.
    pub blocks: Vec<Block>,
    /// In the order a block's path first names each.
    pub containers: Vec<Container>,
    /// Sorted, same-kind unioned, never over a `\n`.
    pub marks: Vec<Mark>,
    /// Sorted by range, then id.
    pub anchors: Vec<Anchor>,
    /// One per [`ISLAND_SLOT`], in slot order.
    pub islands: Vec<Island>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    /// Container ids, outermost first.
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Para,
    Heading { level: u8 },
    Code { lang: Option<String> },
    Rule,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: String,
    pub kind: ContainerKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerKind {
    List { ordered: bool, start: u64 },
    Item { checked: Option<bool> },
    Quote,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub start: Usv,
    pub end: Usv,
    pub kind: MarkKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkKind {
    Strong,
    Emph,
    Underline,
    Strike,
    Code,
    Link { url: String },
    FontFamily { name: String },
}

/// A comment handle over `[start, end)`; a point when the two are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub id: String,
    pub start: Usv,
    pub end: Usv,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Island {
    pub id: String,
    pub island_type: IslandType,
    pub props: JsonValue,
    pub slots: BTreeMap<String, Content>,
}

/// A position: an offset into a field's text, or one inside an island's slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pos {
    At(Usv),
    In { island: String, slot: String, at: Box<Pos> },
}

impl BlockKind {
    pub fn tag(&self) -> &'static str {
        match self {
            BlockKind::Para => "para",
            BlockKind::Heading { .. } => "heading",
            BlockKind::Code { .. } => "code",
            BlockKind::Rule => "rule",
        }
    }
}

impl ContainerKind {
    pub fn tag(&self) -> &'static str {
        match self {
            ContainerKind::List { .. } => "list",
            ContainerKind::Item { .. } => "item",
            ContainerKind::Quote => "quote",
        }
    }

    pub fn same_kind(&self, other: &ContainerKind) -> bool {
        self.tag() == other.tag()
    }
}

impl MarkKind {
    pub fn tag(&self) -> &'static str {
        self.to_line().tag()
    }

    /// One value per character: an add replaces what the range held.
    pub fn is_keyed(&self) -> bool {
        matches!(self, MarkKind::Link { .. } | MarkKind::FontFamily { .. })
    }

    pub(crate) fn to_line(&self) -> line::MarkKind {
        match self {
            MarkKind::Strong => line::MarkKind::Strong,
            MarkKind::Emph => line::MarkKind::Emph,
            MarkKind::Underline => line::MarkKind::Underline,
            MarkKind::Strike => line::MarkKind::Strike,
            MarkKind::Code => line::MarkKind::Code,
            MarkKind::Link { url } => line::MarkKind::Link { url: url.clone() },
            MarkKind::FontFamily { name } => line::MarkKind::FontFamily { name: name.clone() },
        }
    }

    pub(crate) fn from_line(kind: &line::MarkKind) -> Option<MarkKind> {
        Some(match kind {
            line::MarkKind::Strong => MarkKind::Strong,
            line::MarkKind::Emph => MarkKind::Emph,
            line::MarkKind::Underline => MarkKind::Underline,
            line::MarkKind::Strike => MarkKind::Strike,
            line::MarkKind::Code => MarkKind::Code,
            line::MarkKind::Link { url } => MarkKind::Link { url: url.clone() },
            line::MarkKind::FontFamily { name } => MarkKind::FontFamily { name: name.clone() },
            line::MarkKind::Anchor { .. } => return None,
        })
    }

    /// The `(type, attrs)` pair the wire spells, which orders marks.
    pub fn sort_key(&self) -> (String, String) {
        self.to_line().sort_key()
    }
}

impl Mark {
    pub fn new(start: Usv, end: Usv, kind: MarkKind) -> Self {
        Mark { start, end, kind }
    }
}

impl Anchor {
    pub fn new(id: impl Into<String>, start: Usv, end: Usv) -> Self {
        Anchor { id: id.into(), start, end }
    }

    pub fn is_point(&self) -> bool {
        self.start == self.end
    }
}

impl Island {
    pub fn new(id: impl Into<String>, island_type: IslandType) -> Self {
        Island {
            id: id.into(),
            island_type,
            props: JsonValue::Object(Default::default()),
            slots: BTreeMap::new(),
        }
    }

    pub fn with_props(mut self, props: JsonValue) -> Self {
        self.props = props;
        self
    }

    pub fn with_slots(mut self, slots: BTreeMap<String, Content>) -> Self {
        self.slots = slots;
        self
    }

    /// The slot ids the island's props reference, in reading order.
    pub fn slot_order(&self) -> Vec<String> {
        match self.island_type {
            IslandType::Table => crate::table::Table::read(&self.props)
                .map(|t| t.slot_order())
                .unwrap_or_default(),
            IslandType::Footnote => vec![NOTE_SLOT.to_string()],
            IslandType::Image | IslandType::Math => Vec::new(),
        }
    }
}

impl Pos {
    pub fn at(n: Usv) -> Pos {
        Pos::At(n)
    }
}

/// What the model refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invariant {
    /// A character the text cannot hold: `\r`, a bidi control, or a line
    /// separator other than [`HARD_BREAK`].
    ForbiddenChar(char),
    BlockCountMismatch { blocks: usize, segments: usize },
    IslandSlotMismatch { slots: usize, islands: usize },
    BadId { id: String },
    DuplicateId { id: String },
    UnknownContainer { block: String, container: String },
    /// A path whose kinds do not nest: an item outside a list, a list holding
    /// anything but an item, or a path ending in a list.
    BadPath { block: String },
    NestingTooDeep { block: String, depth: usize, max: usize },
    BadHeadingLevel(u8),
    BadLang(String),
    BadMark { start: Usv, end: Usv, len: Usv },
    BadMarkValue,
    BadAnchor { id: String },
    BadIsland { id: String, reason: &'static str },
    JsonTooDeep { what: &'static str, max: usize },
    TableTooLarge { cells: usize, max: usize },
    /// A slot holds what only a field's own text may: anything but paragraphs.
    BadSlot { island: String, slot: String },
    /// A container whose blocks are not one contiguous run.
    SplitContainer { id: String },
    /// A container under two different parent paths.
    TwoParents { id: String },
    UnusedContainer { id: String },
    BreakInHeading { block: String },
    TextInRule { block: String },
    SlotInCode { block: String },
    BlockIslandNotAlone { island: String },
}

impl std::fmt::Display for Invariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invariant::ForbiddenChar(c) => write!(f, "the text holds U+{:04X}, which it cannot", *c as u32),
            Invariant::BlockCountMismatch { blocks, segments } => {
                write!(f, "{blocks} block records for {segments} segments")
            }
            Invariant::IslandSlotMismatch { slots, islands } => write!(f, "{slots} island slots for {islands} islands"),
            Invariant::BadId { id } => write!(f, "`{id}` is not an id"),
            Invariant::DuplicateId { id } => write!(f, "the id `{id}` is held twice"),
            Invariant::UnknownContainer { block, container } => {
                write!(f, "block `{block}` names container `{container}`, which does not exist")
            }
            Invariant::BadPath { block } => write!(f, "block `{block}` has a path whose containers do not nest"),
            Invariant::NestingTooDeep { block, depth, max } => {
                write!(f, "block `{block}` nests {depth} deep, past the limit of {max}")
            }
            Invariant::BadHeadingLevel(l) => write!(f, "heading level {l} is outside 1 to 6"),
            Invariant::BadLang(l) => write!(f, "`{l}` is not a code language"),
            Invariant::BadMark { start, end, len } => write!(f, "mark [{start}, {end}) does not fit text of length {len}"),
            Invariant::BadMarkValue => write!(f, "a mark carries a value it cannot"),
            Invariant::BadAnchor { id } => write!(f, "anchor `{id}` is out of range or has no id"),
            Invariant::BadIsland { id, reason } => write!(f, "island `{id}`: {reason}"),
            Invariant::JsonTooDeep { what, max } => write!(f, "{what} nest past {max} levels"),
            Invariant::TableTooLarge { cells, max } => write!(f, "a table of {cells} cells is past the limit of {max}"),
            Invariant::BadSlot { island, slot } => {
                write!(f, "slot `{slot}` of island `{island}` holds more than paragraphs")
            }
            Invariant::SplitContainer { id } => write!(f, "container `{id}` spans two separate runs"),
            Invariant::TwoParents { id } => write!(f, "container `{id}` sits under two different paths"),
            Invariant::UnusedContainer { id } => write!(f, "container `{id}` holds no block"),
            Invariant::BreakInHeading { block } => write!(f, "heading `{block}` holds a line break"),
            Invariant::TextInRule { block } => write!(f, "rule `{block}` holds text"),
            Invariant::SlotInCode { block } => write!(f, "code block `{block}` holds an island"),
            Invariant::BlockIslandNotAlone { island } => {
                write!(f, "island `{island}` is set as a block and shares its block with other text")
            }
        }
    }
}

impl std::error::Error for Invariant {}

/// Whether the text may hold `c`.
pub fn is_text_char(c: char) -> bool {
    c != '\r' && !is_bidi_char(c) && (c == HARD_BREAK || !is_line_separator(c))
}

impl Default for Content {
    fn default() -> Self {
        Content::empty()
    }
}

impl Content {
    /// One empty paragraph.
    pub fn empty() -> Self {
        Content {
            text: String::new(),
            blocks: vec![Block::new("b0", BlockKind::Para)],
            containers: Vec::new(),
            marks: Vec::new(),
            anchors: Vec::new(),
            islands: Vec::new(),
        }
    }

    pub fn len_usv(&self) -> Usv {
        self.text.chars().count()
    }

    /// Each block's `[start, end)` in the text, its `\n` excluded.
    pub fn block_ranges(&self) -> Vec<(Usv, Usv)> {
        let mut out = Vec::with_capacity(self.blocks.len());
        let mut start = 0;
        let mut i = 0;
        for c in self.text.chars() {
            if c == '\n' {
                out.push((start, i));
                start = i + 1;
            }
            i += 1;
        }
        out.push((start, i));
        out
    }

    /// The index of the block holding `pos`; a `\n` belongs to the block it
    /// ends.
    pub fn block_at(&self, pos: Usv) -> usize {
        self.text.chars().take(pos).filter(|&c| c == '\n').count()
    }

    pub fn container(&self, id: &str) -> Option<&Container> {
        self.containers.iter().find(|c| c.id == id)
    }

    pub fn island(&self, id: &str) -> Option<&Island> {
        self.islands.iter().find(|i| i.id == id)
    }

    pub fn anchor(&self, id: &str) -> Option<&Anchor> {
        self.anchors.iter().find(|a| a.id == id)
    }

    /// One paragraph in no container, with no island: the shape a one-line
    /// field holds.
    pub fn is_inline(&self) -> bool {
        self.islands.is_empty()
            && self.blocks.len() == 1
            && self.blocks[0].kind == BlockKind::Para
            && self.blocks[0].path.is_empty()
    }

    /// Paragraphs in no container, with no mark and no island: the shape a
    /// `text` field holds.
    pub fn is_plain(&self) -> bool {
        self.marks.is_empty()
            && self.islands.is_empty()
            && self.blocks.iter().all(|b| b.kind == BlockKind::Para && b.path.is_empty())
    }

    pub fn is_blank(&self) -> bool {
        self.text.chars().all(|c| c.is_whitespace())
    }

    /// Every family a `font_family` mark names, the text's and then each
    /// slot's, in mark order with repeats.
    pub fn font_families(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .marks
            .iter()
            .filter_map(|m| match &m.kind {
                MarkKind::FontFamily { name } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for island in &self.islands {
            for slot in island.slots.values() {
                out.extend(slot.font_families());
            }
        }
        out
    }

    /// Every anchor id the field holds, its text's and each slot's.
    pub fn anchor_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = self.anchors.iter().map(|a| a.id.clone()).collect();
        for island in &self.islands {
            for slot in island.slots.values() {
                out.extend(slot.anchor_ids());
            }
        }
        out
    }

    /// The content at `island`'s `slot`.
    pub fn slot(&self, island: &str, slot: &str) -> Option<&Content> {
        self.island(island)?.slots.get(slot)
    }

    pub fn slot_mut(&mut self, island: &str, slot: &str) -> Option<&mut Content> {
        self.islands.iter_mut().find(|i| i.id == island)?.slots.get_mut(slot)
    }

    /// The canonical form: marks cut at each `\n`, settled, unioned and
    /// sorted; anchors sorted; containers in first-use order, unused ones
    /// dropped; island props key-sorted and settled; slots likewise.
    pub fn normalize(&mut self) {
        self.normalize_marks();
        self.sort_anchors();
        self.order_containers(true);
        for island in &mut self.islands {
            crate::table::normalize_island(island);
            for slot in island.slots.values_mut() {
                slot.normalize();
            }
        }
    }

    pub(crate) fn sort_anchors(&mut self) {
        self.anchors.sort_by(|a, b| (a.start, a.end, &a.id).cmp(&(b.start, b.end, &b.id)));
    }

    pub(crate) fn normalize_marks(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let marks: Vec<line::Mark> = std::mem::take(&mut self.marks)
            .into_iter()
            .map(|m| line::Mark::new(m.start, m.end, m.kind.to_line()))
            .collect();
        let marks = line::normalize_marks(line::split_marks_at_newlines(&chars, line::normalize_marks(marks)));
        let mut out: Vec<Mark> = marks
            .into_iter()
            .filter_map(|m| MarkKind::from_line(&m.kind).map(|k| Mark::new(m.start, m.end, k)))
            .collect();
        union_across_breaks(&chars, &mut out);
        self.marks = out;
    }

    /// Containers in the order paths first name them; with `drop`, the ones no
    /// path names leave.
    pub(crate) fn order_containers(&mut self, drop: bool) {
        let mut order: HashMap<&str, usize> = HashMap::new();
        for b in &self.blocks {
            for id in &b.path {
                let n = order.len();
                order.entry(id.as_str()).or_insert(n);
            }
        }
        let rank: HashMap<String, usize> = order.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        if drop {
            self.containers.retain(|c| rank.contains_key(&c.id));
        }
        self.containers
            .sort_by_key(|c| rank.get(&c.id).copied().unwrap_or(usize::MAX));
    }

    /// [`check`](Self::check) and [`check_relations`](Self::check_relations):
    /// everything a stored value holds to.
    pub fn validate(&self) -> Result<(), Invariant> {
        self.check()?;
        self.check_relations()?;
        let mut ids = HashSet::new();
        self.check_anchor_ids(&mut ids)
    }

    /// What every step keeps: the characters, the counts, the ids, the
    /// ranges and each path's nesting.
    pub fn check(&self) -> Result<(), Invariant> {
        let mut slots = 0usize;
        let mut segments = 1usize;
        let mut len: Usv = 0;
        for c in self.text.chars() {
            if !is_text_char(c) {
                return Err(Invariant::ForbiddenChar(c));
            }
            match c {
                ISLAND_SLOT => slots += 1,
                '\n' => segments += 1,
                _ => {}
            }
            len += 1;
        }
        if self.blocks.len() != segments {
            return Err(Invariant::BlockCountMismatch { blocks: self.blocks.len(), segments });
        }
        if self.islands.len() != slots {
            return Err(Invariant::IslandSlotMismatch { slots, islands: self.islands.len() });
        }
        let mut containers: HashMap<&str, &ContainerKind> = HashMap::new();
        for c in &self.containers {
            if !is_id(&c.id) {
                return Err(Invariant::BadId { id: c.id.clone() });
            }
            if containers.insert(c.id.as_str(), &c.kind).is_some() {
                return Err(Invariant::DuplicateId { id: c.id.clone() });
            }
        }
        let mut block_ids = HashSet::new();
        for b in &self.blocks {
            if !is_id(&b.id) {
                return Err(Invariant::BadId { id: b.id.clone() });
            }
            if !block_ids.insert(b.id.as_str()) {
                return Err(Invariant::DuplicateId { id: b.id.clone() });
            }
            match &b.kind {
                BlockKind::Heading { level } if !(1..=6).contains(level) => {
                    return Err(Invariant::BadHeadingLevel(*level));
                }
                BlockKind::Code { lang: Some(lang) } if !is_lang(lang) => {
                    return Err(Invariant::BadLang(lang.clone()));
                }
                _ => {}
            }
            check_path(b, &containers)?;
        }
        for m in &self.marks {
            if m.start >= m.end || m.end > len {
                return Err(Invariant::BadMark { start: m.start, end: m.end, len });
            }
            match &m.kind {
                MarkKind::FontFamily { name } if !is_font_family_name(name) => return Err(Invariant::BadMarkValue),
                _ => {}
            }
        }
        for a in &self.anchors {
            if a.id.is_empty() || a.start > a.end || a.end > len {
                return Err(Invariant::BadAnchor { id: a.id.clone() });
            }
        }
        let mut island_ids = HashSet::new();
        for island in &self.islands {
            if !is_id(&island.id) {
                return Err(Invariant::BadId { id: island.id.clone() });
            }
            if !island_ids.insert(island.id.as_str()) {
                return Err(Invariant::DuplicateId { id: island.id.clone() });
            }
            check_island(island)?;
        }
        Ok(())
    }

    /// What a whole change keeps: each container's blocks one contiguous run
    /// under one parent path, every container in use, and each block's kind
    /// agreeing with its text.
    pub fn check_relations(&self) -> Result<(), Invariant> {
        let mut seen: HashMap<&str, (usize, &[String])> = HashMap::new();
        for (i, b) in self.blocks.iter().enumerate() {
            for (k, id) in b.path.iter().enumerate() {
                let prefix = &b.path[..k];
                match seen.get_mut(id.as_str()) {
                    None => {
                        seen.insert(id.as_str(), (i, prefix));
                    }
                    Some((last, held)) => {
                        if *held != prefix {
                            return Err(Invariant::TwoParents { id: id.clone() });
                        }
                        if *last + 1 != i {
                            return Err(Invariant::SplitContainer { id: id.clone() });
                        }
                        *last = i;
                    }
                }
            }
        }
        for c in &self.containers {
            if !seen.contains_key(c.id.as_str()) {
                return Err(Invariant::UnusedContainer { id: c.id.clone() });
            }
        }
        let mut slot = 0usize;
        let chars: Vec<char> = self.text.chars().collect();
        for (b, (s, e)) in self.blocks.iter().zip(self.block_ranges()) {
            let seg = &chars[s..e];
            match b.kind {
                BlockKind::Heading { .. } if seg.contains(&HARD_BREAK) => {
                    return Err(Invariant::BreakInHeading { block: b.id.clone() });
                }
                BlockKind::Rule if !seg.is_empty() => return Err(Invariant::TextInRule { block: b.id.clone() }),
                BlockKind::Code { .. } if seg.contains(&ISLAND_SLOT) => {
                    return Err(Invariant::SlotInCode { block: b.id.clone() });
                }
                _ => {}
            }
            for &c in seg {
                if c == ISLAND_SLOT {
                    let island = &self.islands[slot];
                    if island.island_type.block_only() && (seg.len() != 1 || b.kind != BlockKind::Para) {
                        return Err(Invariant::BlockIslandNotAlone { island: island.id.clone() });
                    }
                    slot += 1;
                }
            }
        }
        for island in &self.islands {
            for slot in island.slots.values() {
                slot.check_relations()?;
            }
        }
        Ok(())
    }

    /// Anchor ids unique over the text and every slot.
    pub(crate) fn check_anchor_ids<'a>(&'a self, ids: &mut HashSet<&'a str>) -> Result<(), Invariant> {
        for a in &self.anchors {
            if !ids.insert(a.id.as_str()) {
                return Err(Invariant::DuplicateId { id: a.id.clone() });
            }
        }
        for island in &self.islands {
            for slot in island.slots.values() {
                slot.check_anchor_ids(ids)?;
            }
        }
        Ok(())
    }

    /// Whether this content may sit in a slot: paragraphs in no container,
    /// holding no island.
    pub fn is_slot_shape(&self) -> bool {
        self.islands.is_empty()
            && self.containers.is_empty()
            && self.blocks.iter().all(|b| b.kind == BlockKind::Para && b.path.is_empty())
    }

    /// The text of each block, island slots and breaks as stored.
    pub fn block_texts(&self) -> Vec<String> {
        self.text.split('\n').map(str::to_string).collect()
    }
}

impl Block {
    pub fn new(id: impl Into<String>, kind: BlockKind) -> Self {
        Block { id: id.into(), kind, path: Vec::new() }
    }

    pub fn with_path(mut self, path: Vec<String>) -> Self {
        self.path = path;
        self
    }
}

impl Container {
    pub fn new(id: impl Into<String>, kind: ContainerKind) -> Self {
        Container { id: id.into(), kind }
    }
}

/// A code block's language: the identifier shape a fence spells unquoted.
pub fn is_lang(lang: &str) -> bool {
    !lang.is_empty() && crate::lines::import::sanitize_lang(lang) == lang
}

/// How deep a path nests, counted as the markdown codecs count it: an item
/// and a quote each one level, a list none of its own.
pub fn depth(path: &[String], kinds: impl Fn(&str) -> Option<ContainerKind>) -> usize {
    path.iter()
        .filter(|id| !matches!(kinds(id), Some(ContainerKind::List { .. })))
        .count()
}

fn check_path(b: &Block, containers: &HashMap<&str, &ContainerKind>) -> Result<(), Invariant> {
    let mut after_list = false;
    let mut levels = 0usize;
    for id in &b.path {
        let Some(kind) = containers.get(id.as_str()) else {
            return Err(Invariant::UnknownContainer { block: b.id.clone(), container: id.clone() });
        };
        let is_item = matches!(kind, ContainerKind::Item { .. });
        if is_item != after_list {
            return Err(Invariant::BadPath { block: b.id.clone() });
        }
        after_list = matches!(kind, ContainerKind::List { .. });
        if !after_list {
            levels += 1;
        }
    }
    if after_list {
        return Err(Invariant::BadPath { block: b.id.clone() });
    }
    if levels > crate::MAX_NESTING_DEPTH {
        return Err(Invariant::NestingTooDeep { block: b.id.clone(), depth: levels, max: crate::MAX_NESTING_DEPTH });
    }
    let mut seen = HashSet::new();
    if !b.path.iter().all(|id| seen.insert(id)) {
        return Err(Invariant::BadPath { block: b.id.clone() });
    }
    Ok(())
}

fn check_island(island: &Island) -> Result<(), Invariant> {
    if line::json_depth_exceeds(&island.props, crate::MAX_JSON_DEPTH) {
        return Err(Invariant::JsonTooDeep { what: "island props", max: crate::MAX_JSON_DEPTH });
    }
    if !island.props.is_object() {
        return Err(Invariant::BadIsland { id: island.id.clone(), reason: "props is not an object" });
    }
    let bad = |reason| Invariant::BadIsland { id: island.id.clone(), reason };
    match island.island_type {
        IslandType::Table => crate::table::check(island)?,
        IslandType::Footnote => {
            if island.slots.len() != 1 || !island.slots.contains_key(NOTE_SLOT) {
                return Err(bad("a footnote holds one slot, `note`"));
            }
        }
        IslandType::Math => {
            if !island.slots.is_empty() {
                return Err(bad("an equation holds no slot"));
            }
            if !island.props.get("tex").is_some_and(JsonValue::is_string) {
                return Err(bad("an equation's `tex` is not a string"));
            }
        }
        IslandType::Image => {
            if !island.slots.is_empty() {
                return Err(bad("an image holds no slot"));
            }
            if !island.props.get("url").is_some_and(JsonValue::is_string) {
                return Err(bad("an image's `url` is not a string"));
            }
        }
    }
    for (id, slot) in &island.slots {
        if !is_id(id) {
            return Err(Invariant::BadId { id: id.clone() });
        }
        slot.check()?;
        if !slot.is_slot_shape() {
            return Err(Invariant::BadSlot { island: island.id.clone(), slot: id.clone() });
        }
    }
    Ok(())
}

/// Union same-kind marks whose gap holds nothing but hard breaks, so a break
/// inside a formatted run is one spelling.
fn union_across_breaks(chars: &[char], marks: &mut Vec<Mark>) {
    if !chars.contains(&HARD_BREAK) || marks.len() < 2 {
        return;
    }
    let mut groups: BTreeMap<(String, String), Vec<Mark>> = BTreeMap::new();
    for m in marks.drain(..) {
        groups.entry(m.kind.sort_key()).or_default().push(m);
    }
    for (_, mut ms) in groups {
        ms.sort_by_key(|m| (m.start, m.end));
        let mut cur = ms[0].clone();
        for m in ms.into_iter().skip(1) {
            if m.start >= cur.end && chars[cur.end..m.start].iter().all(|&c| c == HARD_BREAK) && m.start > cur.end {
                cur.end = m.end;
            } else {
                marks.push(std::mem::replace(&mut cur, m));
            }
        }
        marks.push(cur);
    }
    marks.sort_by_cached_key(|m| (m.start, m.end, m.kind.sort_key()));
}

/// Whether a content value nests past what a slot and an island's props may.
pub(crate) fn json_too_deep(v: &JsonValue) -> bool {
    line::json_depth_exceeds(v, crate::MAX_JSON_DEPTH + 8)
}
