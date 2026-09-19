//! Drift guard for the names `runtime/runtime.js` and the `engine.rs` TS unions
//! spell for themselves instead of reading off a type. Each is checked against
//! the Rust vocabulary it mirrors, read through an exhaustive match so a new
//! member is a compile error here, where the mirror gets read.

use quillmark_content::island::IslandType;
use quillmark_content::model::{Container, LineKind, MarkKind};

const RUNTIME_JS: &str = include_str!("../runtime/runtime.js");

/// The wire `kind` of every line vocabulary member. Exhaustive on purpose.
fn line_kind_tags() -> Vec<&'static str> {
    let all = [
        LineKind::Para,
        LineKind::Heading { level: 1 },
        LineKind::Code { lang: None },
        LineKind::Rule,
    ];
    for k in &all {
        match k {
            LineKind::Para
            | LineKind::Heading { .. }
            | LineKind::Code { .. }
            | LineKind::Rule => {}
        }
    }
    all.iter().map(LineKind::tag).collect()
}

/// The wire `container` of every container vocabulary member.
fn container_tags() -> Vec<&'static str> {
    let all = [
        Container::ListItem {
            ordered: false,
            start: 1,
            ordinal: 0,
            instance: 0,
        },
        Container::Quote { instance: 0 },
    ];
    for c in &all {
        match c {
            Container::ListItem { .. } | Container::Quote { .. } => {}
        }
    }
    all.iter().map(Container::tag).collect()
}

/// The wire `type` of every mark vocabulary member.
fn mark_type_tags() -> Vec<&'static str> {
    let all = [
        MarkKind::Strong,
        MarkKind::Emph,
        MarkKind::Underline,
        MarkKind::Strike,
        MarkKind::Code,
        MarkKind::Link { url: String::new() },
        MarkKind::Anchor { id: String::new() },
    ];
    for k in &all {
        match k {
            MarkKind::Strong
            | MarkKind::Emph
            | MarkKind::Underline
            | MarkKind::Strike
            | MarkKind::Code
            | MarkKind::Link { .. }
            | MarkKind::Anchor { .. } => {}
        }
    }
    all.iter().map(MarkKind::tag).collect()
}

/// The same names are spelled a third time as TypeScript unions in
/// `src/engine.rs`; a union that lags a new built-in is a consumer that cannot
/// narrow the new arm. Arm *shape* varies, so this asserts only that the name
/// appears in the union.
#[test]
fn ts_unions_name_every_built_in() {
    const ENGINE_RS: &str = include_str!("../src/engine.rs");

    /// Bounded by the blank line before the next declaration, since an arm's own
    /// `;` separates its members and cannot terminate the search.
    fn ts_union(name: &str) -> &'static str {
        let decl = format!("export type {name} =");
        let start = ENGINE_RS
            .find(&decl)
            .unwrap_or_else(|| panic!("engine.rs has no `{decl}`"))
            + decl.len();
        let body = &ENGINE_RS[start..];
        &body[..body.find("\n\n").expect("unterminated type alias")]
    }

    // An island type carries no payload, so it is its own tag list.
    let island_types: Vec<_> = IslandType::ALL.iter().map(|k| k.as_str()).collect();

    for (union, names) in [
        (ts_union("ContentLineKind"), line_kind_tags()),
        (ts_union("ContentContainer"), container_tags()),
        (ts_union("ContentMarkKind"), mark_type_tags()),
        (ts_union("ContentIsland"), island_types),
    ] {
        for name in names {
            assert!(
                union.contains(&format!("\"{name}\"")),
                "a TS union in engine.rs is missing the `{name}` arm"
            );
        }
    }
}

/// `weldsWith` in `runtime.js` reads `WELD_KEYS` by tag and welds nothing under
/// a tag the table omits, so a container added without an entry under-stamps at
/// every JS consumer — a boundary that silently disappears, not an error. Only
/// the coverage is checked here; *which* keys an entry names is
/// `runtime.test.js` § "container run boundaries", which stamps a pair and
/// re-imports it, where `Content::normalize` re-mints against
/// `Container::same_weld` itself.
#[test]
fn js_weld_keys_cover_every_container() {
    const DECL: &str = "const WELD_KEYS = {";
    let start = RUNTIME_JS
        .find(DECL)
        .expect("runtime.js has no `const WELD_KEYS = {…`")
        + DECL.len();
    let rest = &RUNTIME_JS[start..];
    let body = &rest[..rest.find('}').expect("unterminated WELD_KEYS literal")];

    let tags: Vec<String> = body
        .split(']')
        .filter_map(|chunk| chunk.split_once(':'))
        .map(|(tag, _)| tag.trim().trim_start_matches(',').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(tags, container_tags());
}
