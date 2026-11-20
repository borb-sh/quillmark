use crate::layout::{Child, HintLevel, ItemKind, Marker, NodeId, NodeKind, NumberFormat};
use crate::model::{
    AbstractNum, Align, FirstLine, Indent, Level, LevelSuffix, Num, NumFormat, NumRef, ParProps,
    RunProps, Style, TabKind, TabLeader, TabStop,
};

use super::space::Entry;
use super::styles::style_id;
use super::{Ctx, Lower, tw};

/// A numbering scheme a `list` hint names: its levels and its first `w:num`.
pub(super) struct Scheme {
    pub(super) name: String,
    pub(super) abstract_id: u32,
    pub(super) levels: Vec<HintLevel>,
}

pub(super) fn num_format(f: NumberFormat) -> NumFormat {
    match f {
        NumberFormat::Decimal => NumFormat::Decimal,
        NumberFormat::LowerLetter => NumFormat::LowerLetter,
        NumberFormat::UpperLetter => NumFormat::UpperLetter,
        NumberFormat::LowerRoman => NumFormat::LowerRoman,
        NumberFormat::UpperRoman => NumFormat::UpperRoman,
    }
}

/// A list level's format, `lvlText` and start from the list's marker.
pub(super) fn level_of(marker: &Marker, level: u8) -> (NumFormat, String, u32) {
    match marker {
        Marker::Bullet(glyph) => (NumFormat::Bullet, glyph.clone(), 1),
        Marker::Numbered {
            counters,
            suffix,
            start,
        } => {
            let first = (level as usize + 1).saturating_sub(counters.len());
            let mut text = String::new();
            for (i, (prefix, _)) in counters.iter().enumerate() {
                text.push_str(prefix);
                text.push_str(&format!("%{}", first + i + 1));
            }
            text.push_str(suffix);
            let format = counters
                .last()
                .map_or(NumFormat::Decimal, |c| num_format(c.1));
            (format, text, (*start).clamp(0, u32::MAX as u64) as u32)
        }
        Marker::Printed => (NumFormat::None, String::new(), 1),
    }
}

impl<'a> Lower<'a> {
    /// The `w:num` for one instance of the scheme a `list` hint names: the
    /// scheme's own on its first instance, else a new one restarting at 1.
    pub(super) fn scheme(&mut self, name: &str, levels: &[HintLevel]) -> (usize, u32, bool) {
        let num = self.numbering.nums.len() as u32 + 1;
        if let Some(i) = self.schemes.iter().position(|s| s.name == name) {
            let abstract_id = self.schemes[i].abstract_id;
            self.numbering.nums.push(Num {
                id: num,
                abstract_id,
                starts: vec![(0, 1)],
            });
            return (i, num, false);
        }
        let abstract_id = self.numbering.abstracts.len() as u32;
        let levels: Vec<HintLevel> = (0..9)
            .map(|i| {
                levels
                    .get(i)
                    .or(levels.last())
                    .cloned()
                    .unwrap_or(HintLevel {
                        format: NumberFormat::Decimal,
                        prefix: String::new(),
                        suffix: ".".into(),
                        underline: false,
                        visible: true,
                        indent: 0.0,
                    })
            })
            .collect();
        let id = style_id(name);
        let mut word = Vec::new();
        for (i, l) in levels.iter().enumerate() {
            let indent = Indent {
                left: Some(0),
                right: None,
                first: Some(if l.indent >= 0.0 {
                    FirstLine::Indent(tw(l.indent))
                } else {
                    FirstLine::Hanging(tw(-l.indent))
                }),
            };
            let style = format!("{id}{}", i + 1);
            word.push(Level {
                start: 1,
                format: if l.visible {
                    num_format(l.format)
                } else {
                    NumFormat::None
                },
                style: Some(style.clone()),
                suffix: LevelSuffix::Nothing,
                text: if l.visible {
                    format!("{}%{}{}", l.prefix, i + 1, l.suffix)
                } else {
                    String::new()
                },
                align: Align::Start,
                indent: indent.clone(),
                tabs: Vec::new(),
                run: RunProps {
                    underline: l.underline,
                    ..RunProps::default()
                },
            });
            self.extra_styles.push(Style {
                based_on: Some("Normal".into()),
                quick: i < 4,
                par: ParProps {
                    numbering: Some(NumRef {
                        level: i as u8,
                        num,
                    }),
                    indent: Some(indent),
                    ..ParProps::default()
                },
                ..Style::paragraph(style, format!("{name} {}", i + 1), false)
            });
        }
        self.numbering.abstracts.push(AbstractNum {
            id: abstract_id,
            nsid: format!("{:08X}", 0x5153_0000 + abstract_id),
            levels: word,
        });
        self.numbering.nums.push(Num {
            id: num,
            abstract_id,
            starts: vec![(0, 1)],
        });
        self.schemes.push(Scheme {
            name: name.to_string(),
            abstract_id,
            levels,
        });
        (self.schemes.len() - 1, num, true)
    }

    pub(super) fn list(&mut self, n: NodeId, ctx: &Ctx) -> Vec<Entry> {
        let (num, level) = match ctx.list {
            Some((num, level)) => (num, (level + 1).min(8)),
            None => {
                let id = self.numbering.abstracts.len() as u32;
                self.numbering.abstracts.push(AbstractNum {
                    id,
                    nsid: format!("{:08X}", 0x5153_0000 + id),
                    levels: Vec::new(),
                });
                let num = self.numbering.nums.len() as u32 + 1;
                self.numbering.nums.push(Num {
                    id: num,
                    abstract_id: id,
                    starts: Vec::new(),
                });
                (num, 0)
            }
        };
        let (marker, label_end) = match self.kind(n) {
            NodeKind::List { marker, label_end } => (marker.clone(), *label_end),
            _ => (Marker::Printed, false),
        };
        let (labels, bodies) = self.list_parts(n);
        let label_frame = labels
            .first()
            .and_then(|&l| self.doc.nodes[l].frames.first())
            .map(|f| f.1);
        let body_x = bodies
            .first()
            .and_then(|&b| self.doc.nodes[b].frames.first())
            .map(|f| f.1.x0);
        if marker == Marker::Printed {
            let kids = self.doc.nodes[n].children.clone();
            let inner = Ctx {
                list: None,
                ..ctx.clone()
            };
            let mut out = Vec::new();
            for c in &kids {
                out.extend(self.printed_child(c, &inner));
            }
            return out;
        }
        let (format, text, start) = level_of(&marker, level);
        let left = body_x.map_or(18.0 * (level as f64 + 1.0), |x| x - ctx.left);
        let label_at = label_frame.map(|f| if label_end { f.x1 } else { f.x0 } - ctx.left);
        let hanging = label_at.map_or(18.0, |l| left - l);
        let abs = self
            .numbering
            .nums
            .iter()
            .find(|m| m.id == num)
            .map(|m| m.abstract_id)
            .unwrap_or(0) as usize;
        let levels = &mut self.numbering.abstracts[abs].levels;
        while levels.len() <= level as usize {
            let i = levels.len() as u8;
            levels.push(Level {
                start: 1,
                format: NumFormat::Decimal,
                style: None,
                suffix: LevelSuffix::Tab,
                text: format!("%{}.", i + 1),
                align: Align::Start,
                indent: Indent {
                    left: Some(tw(18.0 * (i as f64 + 1.0))),
                    right: None,
                    first: Some(FirstLine::Hanging(tw(18.0))),
                },
                tabs: Vec::new(),
                run: RunProps::default(),
            });
        }
        levels[level as usize] = Level {
            start,
            format,
            style: None,
            suffix: LevelSuffix::Tab,
            text,
            align: if label_end { Align::End } else { Align::Start },
            indent: Indent {
                left: Some(tw(left)),
                right: None,
                first: Some(FirstLine::Hanging(tw(hanging.max(0.0)))),
            },
            tabs: vec![TabStop {
                kind: TabKind::Left,
                pos: tw(left),
                leader: TabLeader::None,
            }],
            run: RunProps::default(),
        };
        let inner = Ctx {
            list: Some((num, level)),
            ..ctx.clone()
        };
        let kids = self.doc.nodes[n].children.clone();
        let mut out = Vec::new();
        for c in &kids {
            out.extend(self.list_child(c, &inner, left));
        }
        out
    }

    pub(super) fn list_child(&mut self, c: &Child, ctx: &Ctx, left: f64) -> Vec<Entry> {
        let Child::Node(m) = c else {
            return self.child(c, ctx);
        };
        let m = *m;
        match self.kind(m) {
            NodeKind::ListBody if self.doc.nodes[m].lines.is_empty() => {
                let mut blocks = Vec::new();
                let kids = self.doc.nodes[m].children.clone();
                let mut numbered = false;
                for k in &kids {
                    let is_list = matches!(k, Child::Node(x) if matches!(self.kind(*x), NodeKind::List { .. }));
                    let inner = if numbered && !is_list {
                        Ctx {
                            list: None,
                            left: ctx.left + left,
                            ..ctx.clone()
                        }
                    } else {
                        ctx.clone()
                    };
                    let produced = self.child(k, &inner);
                    numbered |= produced.iter().any(|e| matches!(e, Entry::Par(..)));
                    blocks.extend(produced);
                }
                blocks
            }
            NodeKind::ListBody | NodeKind::ListLabel | NodeKind::List { .. } => self.node(m, ctx),
            _ => {
                let kids = self.doc.nodes[m].children.clone();
                if matches!(
                    self.kind(m),
                    NodeKind::Block { .. } | NodeKind::Align { .. }
                ) {
                    let mut out = Vec::new();
                    for k in &kids {
                        out.extend(self.list_child(k, ctx, left));
                    }
                    out
                } else {
                    self.node(m, ctx)
                }
            }
        }
    }

    /// A child of a list whose numbers stay as printed: each label's text
    /// opens the paragraph of the body after it, with a tab between.
    pub(super) fn printed_child(&mut self, c: &Child, ctx: &Ctx) -> Vec<Entry> {
        let Child::Node(m) = c else {
            return self.child(c, ctx);
        };
        match self.kind(*m) {
            NodeKind::ListLabel => {
                self.consume_subtree(*m);
                let text = self.text_of(*m);
                self.label_prefix = Some(text.trim().to_string());
                Vec::new()
            }
            NodeKind::Block { .. } | NodeKind::Align { .. } => {
                let kids = self.doc.nodes[*m].children.clone();
                let mut out = Vec::new();
                for k in &kids {
                    out.extend(self.printed_child(k, ctx));
                }
                out
            }
            _ => self.child(c, ctx),
        }
    }

    /// The labels and bodies of `n`'s own items, not of a list nested in it.
    pub(super) fn list_parts(&self, n: NodeId) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut labels = Vec::new();
        let mut bodies = Vec::new();
        let mut stack: Vec<NodeId> = vec![n];
        let mut order = Vec::new();
        while let Some(at) = stack.pop() {
            order.push(at);
            for c in self.doc.nodes[at].children.iter().rev() {
                if let Child::Node(m) = c {
                    match self.kind(*m) {
                        NodeKind::List { .. } => {}
                        NodeKind::ListLabel => labels.push(*m),
                        NodeKind::ListBody => bodies.push(*m),
                        _ => stack.push(*m),
                    }
                }
            }
        }
        labels.sort();
        bodies.sort();
        (labels, bodies)
    }

    pub(super) fn text_of(&self, n: NodeId) -> String {
        self.doc
            .subtree(n)
            .into_iter()
            .flat_map(|m| self.doc.nodes[m].children.clone())
            .filter_map(|c| match c {
                Child::Item(r) => match &self.doc.item(r).kind {
                    ItemKind::Text(t) => Some(t.text.clone()),
                    _ => None,
                },
                Child::Node(_) => None,
            })
            .collect()
    }
}
