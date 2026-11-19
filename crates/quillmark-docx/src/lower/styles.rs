use std::collections::{BTreeMap, HashMap};

use crate::layout::ItemKind;
use crate::model::{Block, ParProps, RunProps, Spacing, Style, Styles};

use super::Lower;
use super::paragraph::word_font;

/// A role's or a scheme's level's style id: its name without spaces or
/// punctuation.
pub(super) fn style_id(name: &str) -> String {
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if id.is_empty() { "Role".into() } else { id }
}

impl<'a> Lower<'a> {
    /// One style per role holding what every paragraph of the role shares,
    /// over two or more paragraphs and leaving out the first one's space
    /// before and every keep; what a style holds leaves the paragraphs.
    pub(super) fn role_styles(&self, bodies: &mut [Vec<Block>]) -> Vec<Style> {
        let mut out = Vec::new();
        for (id, (name, next)) in &self.roles {
            let mut pars: Vec<&mut ParProps> = bodies
                .iter_mut()
                .flatten()
                .filter_map(|b| match b {
                    Block::Paragraph(p) if p.props.style.as_deref() == Some(id) => {
                        Some(&mut p.props)
                    }
                    _ => None,
                })
                .collect();
            let mut par = ParProps::default();
            if pars.len() >= 2 {
                fn shared<T: Clone + PartialEq>(v: &[T]) -> Option<T> {
                    v.first().filter(|f| v.iter().all(|x| x == *f)).cloned()
                }
                let align: Vec<_> = pars.iter().map(|p| p.align).collect();
                if let Some(a) = shared(&align).flatten() {
                    par.align = Some(a);
                    pars.iter_mut().for_each(|p| p.align = None);
                }
                let indent: Vec<_> = pars.iter().map(|p| p.indent.clone()).collect();
                if let Some(i) = shared(&indent).flatten() {
                    par.indent = Some(i);
                    pars.iter_mut().for_each(|p| p.indent = None);
                }
                let tabs: Vec<_> = pars.iter().map(|p| p.tabs.clone()).collect();
                if let Some(t) = shared(&tabs).filter(|t| !t.is_empty()) {
                    par.tabs = t;
                    pars.iter_mut().for_each(|p| p.tabs.clear());
                }
                let line: Vec<_> = pars
                    .iter()
                    .map(|p| p.spacing.as_ref().and_then(|s| s.line))
                    .collect();
                let before: Vec<_> = pars[1..]
                    .iter()
                    .map(|p| p.spacing.as_ref().and_then(|s| s.before))
                    .collect();
                let line = shared(&line).flatten();
                let before = (before.len() >= 2)
                    .then(|| shared(&before).flatten())
                    .flatten();
                if line.is_some() || before.is_some() {
                    par.spacing = Some(Spacing {
                        before,
                        after: None,
                        line,
                    });
                    for (i, p) in pars.iter_mut().enumerate() {
                        if let Some(sp) = p.spacing.as_mut() {
                            if line.is_some() {
                                sp.line = None;
                            }
                            if before.is_some() && (i > 0 || sp.before == before) {
                                sp.before = None;
                            }
                        }
                    }
                }
            }
            out.push(Style {
                based_on: Some("Normal".into()),
                next: Some(
                    next.as_deref()
                        .map(style_id)
                        .unwrap_or_else(|| "Normal".into()),
                ),
                quick: true,
                par,
                ..Style::paragraph(id.clone(), name.clone(), false)
            });
        }
        let unused: BTreeMap<String, &String> = self
            .roles
            .values()
            .filter_map(|(_, next)| next.as_ref())
            .map(|n| (style_id(n), n))
            .filter(|(id, _)| !self.roles.contains_key(id))
            .collect();
        for (id, name) in unused {
            out.push(Style {
                based_on: Some("Normal".into()),
                next: Some(id.clone()),
                quick: true,
                ..Style::paragraph(id, name.clone(), false)
            });
        }
        out
    }

    pub(super) fn styles(&self) -> Styles {
        let mut styles = vec![Style {
            quick: true,
            ..Style::paragraph("Normal", "Normal", true)
        }];
        styles.extend(self.extra_styles.iter().cloned());
        for &level in &self.headings {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                quick: true,
                ui_priority: Some(9),
                par: ParProps {
                    keep_next: true,
                    outline_level: Some(level - 1),
                    ..ParProps::default()
                },
                ..Style::paragraph(format!("Heading{level}"), format!("heading {level}"), true)
            });
        }
        for &level in &self.unlisted {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                quick: true,
                par: ParProps {
                    keep_next: true,
                    ..ParProps::default()
                },
                ..Style::paragraph(
                    format!("UnlistedHeading{level}"),
                    format!("Unlisted Heading {level}"),
                    false,
                )
            });
        }
        for &(id, name) in &self.builtin {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                quick: true,
                ..Style::paragraph(id, name, true)
            });
        }
        for &level in &self.toc_levels {
            styles.push(Style {
                based_on: Some("Normal".into()),
                next: Some("Normal".into()),
                ui_priority: Some(39),
                ..Style::paragraph(format!("TOC{level}"), format!("toc {level}"), true)
            });
        }
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        let mut families: HashMap<String, usize> = HashMap::new();
        for page in &self.doc.pages {
            for item in &page.items {
                if let ItemKind::Text(t) = &item.kind {
                    *sizes.entry((t.size * 2.0).round() as u32).or_default() += t.text.len();
                    let f = word_font(&self.doc.fonts[t.font].family);
                    *families.entry(f).or_default() += t.text.len();
                }
            }
        }
        fn top<K: Clone + Ord>(m: &HashMap<K, usize>) -> Option<K> {
            let mut v: Vec<(&K, &usize)> = m.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            v.first().map(|e| e.0.clone())
        }
        Styles {
            default_run: RunProps {
                font: top(&families),
                size: top(&sizes),
                ..RunProps::default()
            },
            default_par: ParProps {
                spacing: Some(Spacing {
                    before: None,
                    after: Some(0),
                    line: None,
                }),
                widow_control: Some(true),
                ..ParProps::default()
            },
            styles,
        }
    }
}
