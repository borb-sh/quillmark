//! Between the content model and the line form. [`from_lines`] is the v1
//! migration and the last step of a markdown import: a line run joined by
//! hard breaks becomes one block, container runs become containers by id, and
//! anchor marks become anchors, every id minted deterministically. [`to_lines`]
//! is the lowering the markdown codecs and the renderers read, computing the
//! ordinals and run parity the line form spells.

use crate::ids::{Minter, prefix};
use crate::lines::model::{self as line, Container as LineContainer, Line, LineKind};
use crate::model::{
    Anchor, Block, BlockKind, Container, ContainerKind, Content, HARD_BREAK, Island, Mark, MarkKind,
};
use std::collections::HashMap;

/// A line-form value as content, ids minted in reading order.
pub fn from_lines(rt: &line::Content) -> Content {
    let mut text = String::with_capacity(rt.text.len());
    let mut lines = rt.lines.iter();
    let mut segs = rt.text.split('\n');
    let mut starts: Vec<&Line> = Vec::new();
    if let (Some(first), Some(seg)) = (lines.next(), segs.next()) {
        text.push_str(seg);
        starts.push(first);
    }
    for (l, seg) in lines.zip(segs) {
        if l.continues {
            text.push(HARD_BREAK);
        } else {
            text.push('\n');
            starts.push(l);
        }
        text.push_str(seg);
    }
    let mut blocks: Vec<Block> = Vec::with_capacity(starts.len());
    let mut containers: Vec<Container> = Vec::new();
    let mut minter = Minter::default();
    let mut open: Vec<Open> = Vec::new();
    let mut prev: Option<&[LineContainer]> = None;
    for (n, l) in starts.iter().enumerate() {
        let mut path = Vec::new();
        let mut opened = false;
        for (d, here) in l.containers.iter().enumerate() {
            let before = prev.and_then(|p| p.get(d));
            let same_run =
                !opened && before.is_some_and(|b| b.same_run(here) && b.instance() == here.instance());
            if !same_run {
                open.truncate(d);
                opened = true;
                let open_here = match here {
                    LineContainer::ListItem { ordered, start, checked, .. } => {
                        let list = minter.mint(prefix::LIST);
                        let item = minter.mint(prefix::ITEM);
                        containers.push(Container::new(&list, ContainerKind::List { ordered: *ordered, start: *start }));
                        containers.push(Container::new(&item, ContainerKind::Item { checked: *checked }));
                        Open::List { list, item }
                    }
                    LineContainer::Quote { .. } => {
                        let quote = minter.mint(prefix::QUOTE);
                        containers.push(Container::new(&quote, ContainerKind::Quote));
                        Open::Quote(quote)
                    }
                };
                open.push(open_here);
            } else if let (LineContainer::ListItem { checked, .. }, Some(b)) = (here, before)
                && b != here
            {
                open.truncate(d + 1);
                opened = true;
                let item = minter.mint(prefix::ITEM);
                containers.push(Container::new(&item, ContainerKind::Item { checked: *checked }));
                if let Some(Open::List { item: held, .. }) = open.get_mut(d) {
                    *held = item;
                }
            }
            match &open[d] {
                Open::List { list, item } => {
                    path.push(list.clone());
                    path.push(item.clone());
                }
                Open::Quote(q) => path.push(q.clone()),
            }
        }
        open.truncate(l.containers.len());
        prev = Some(&l.containers);
        let kind = match &l.kind {
            LineKind::Para => BlockKind::Para,
            LineKind::Heading { level } => BlockKind::Heading { level: *level },
            LineKind::Code { lang } => BlockKind::Code { lang: lang.clone().filter(|l| !l.is_empty()) },
            LineKind::Rule => BlockKind::Rule,
        };
        blocks.push(Block::new(format!("{}{n}", prefix::BLOCK), kind).with_path(path));
    }
    let mut marks = Vec::new();
    let mut anchors = Vec::new();
    for m in &rt.marks {
        match &m.kind {
            line::MarkKind::Anchor { id } => anchors.push(Anchor::new(id.clone(), m.start, m.end)),
            kind => {
                if let Some(kind) = MarkKind::from_line(kind) {
                    marks.push(Mark::new(m.start, m.end, kind));
                }
            }
        }
    }
    let mut island_ids = Minter::default();
    let islands = rt
        .islands
        .iter()
        .map(|i| {
            let id = island_ids.keep_or_mint(&i.id, prefix::ISLAND);
            let (props, slots) = crate::table::from_line_props(i.island_type, &i.props, &mut Minter::default());
            Island { id, island_type: i.island_type, props: object(props), slots }
        })
        .collect();
    let mut out = Content { text, blocks, containers, marks, anchors, islands };
    out.normalize();
    out
}

enum Open {
    List { list: String, item: String },
    Quote(String),
}

fn object(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(_) => v,
        _ => serde_json::Value::Object(Default::default()),
    }
}

/// The content as the line form spells it: each hard break a `\n` opening a
/// continuation line, each list and item one list-item container with its
/// ordinal, adjacent runs told apart by `instance`, anchors as anchor marks.
pub fn to_lines(c: &Content) -> line::Normalized {
    let text: String = c.text.chars().map(|ch| if ch == HARD_BREAK { '\n' } else { ch }).collect();
    let kinds: HashMap<&str, (usize, &ContainerKind)> =
        c.containers.iter().enumerate().map(|(n, k)| (k.id.as_str(), (n, &k.kind))).collect();
    let mut ordinals: HashMap<&str, HashMap<&str, u64>> = HashMap::new();
    let mut lines = Vec::with_capacity(c.blocks.len());
    for (b, seg) in c.blocks.iter().zip(c.text.split('\n')) {
        let mut path = Vec::new();
        let mut ids = b.path.iter().map(String::as_str).peekable();
        while let Some(id) = ids.next() {
            let Some(&(n, kind)) = kinds.get(id) else { continue };
            match kind {
                ContainerKind::List { ordered, start } => {
                    let Some(item) = ids.next() else { break };
                    let checked = match kinds.get(item) {
                        Some((_, ContainerKind::Item { checked })) => *checked,
                        _ => None,
                    };
                    let items = ordinals.entry(id).or_default();
                    let next = items.len() as u64;
                    let ordinal = *items.entry(item).or_insert(next);
                    path.push(LineContainer::ListItem {
                        ordered: *ordered,
                        start: *start,
                        ordinal,
                        checked,
                        instance: n as u64,
                    });
                }
                ContainerKind::Quote => path.push(LineContainer::Quote { instance: n as u64 }),
                ContainerKind::Item { .. } => {}
            }
        }
        let kind = match &b.kind {
            BlockKind::Para => LineKind::Para,
            BlockKind::Heading { level } => LineKind::Heading { level: *level },
            BlockKind::Code { lang } => LineKind::Code { lang: lang.clone() },
            BlockKind::Rule => LineKind::Rule,
        };
        lines.push(Line::new(kind.clone()).with_containers(path.clone()));
        for _ in seg.chars().filter(|&ch| ch == HARD_BREAK) {
            lines.push(Line::new(kind.clone()).with_containers(path.clone()).with_continues(true));
        }
    }
    let mut marks: Vec<line::Mark> =
        c.marks.iter().map(|m| line::Mark::new(m.start, m.end, m.kind.to_line())).collect();
    marks.extend(
        c.anchors
            .iter()
            .map(|a| line::Mark::new(a.start, a.end, line::MarkKind::Anchor { id: a.id.clone() })),
    );
    let islands = c
        .islands
        .iter()
        .map(|i| line::Island::new(i.id.clone(), i.island_type).with_props(crate::table::to_line_props(i)))
        .collect();
    line::Content::new(text, lines).with_marks(marks).with_islands(islands).into_normalized()
}
