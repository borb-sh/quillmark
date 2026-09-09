//! Drift guard for the names `runtime/runtime.js` and the `engine.rs` TS unions
//! spell for themselves instead of reading off a type. Each is checked against
//! the Rust vocabulary it mirrors, read through an exhaustive match so a new
//! member is a compile error here, where the mirror gets read.

use quillmark_content::island::IslandType;
use quillmark_content::model::{LineKind, MarkKind};
use quillmark_content::{Container, Loss};
use quillmark_core::quill::VARIANT_DISCRIMINANT_KEY;

const RUNTIME_JS: &str = include_str!("../runtime/runtime.js");

/// The wire `kind` of every line vocabulary member. Exhaustive on purpose.
fn line_kind_tags() -> Vec<&'static str> {
    let all = [
        LineKind::Para,
        LineKind::Heading { level: 1 },
        LineKind::Code { lang: None },
        LineKind::Island,
        LineKind::Rule,
    ];
    for k in &all {
        match k {
            LineKind::Para
            | LineKind::Heading { .. }
            | LineKind::Code { .. }
            | LineKind::Island
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

/// The `.d.ts` is pinned as a string *literal* type: widened to `string` it
/// would stop narrowing an index into the container.
#[test]
fn js_variant_discriminant_matches_the_rust_key() {
    const RUNTIME_DTS: &str = include_str!("../runtime/runtime.d.ts");
    let key = VARIANT_DISCRIMINANT_KEY;

    for (file, decl) in [
        (RUNTIME_JS, format!("export const VARIANT_DISCRIMINANT_KEY = '{key}'")),
        (
            RUNTIME_DTS,
            format!("export declare const VARIANT_DISCRIMINANT_KEY: '{key}'"),
        ),
    ] {
        assert!(file.contains(&decl), "the JS layer has no `{decl}`");
    }
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

    // The two island axes carry no payload, so each is its own tag list.
    let loss_names: Vec<_> = Loss::ALL.iter().map(|f| f.as_str()).collect();
    let island_types: Vec<_> = IslandType::ALL.iter().map(|k| k.as_str()).collect();

    for (union, names) in [
        (ts_union("ContentLineKind"), line_kind_tags()),
        (ts_union("ContentContainer"), container_tags()),
        (ts_union("ContentMark"), mark_type_tags()),
        (ts_union("ContentLossClass"), loss_names.clone()),
        (ts_union("ContentIsland"), island_types.clone()),
    ] {
        for name in names {
            assert!(
                union.contains(&format!("\"{name}\"")),
                "a TS union in engine.rs is missing the `{name}` arm"
            );
        }
    }
}

/// `weldsWith` in `runtime.js` re-spells the rule `Container::same_weld` owns:
/// which fields two adjacent runs must share for the Markdown projection to
/// read them as one. The Rust half here is read off the predicate rather than
/// restated, so a change to the rule fails here instead of welding two runs at
/// every JS consumer.
#[test]
fn js_weld_keys_match_the_rust_weld_rule() {
    fn body() -> &'static str {
        const DECL: &str = "const WELD_KEYS = {";
        let start = RUNTIME_JS
            .find(DECL)
            .expect("runtime.js has no `const WELD_KEYS = {…`")
            + DECL.len();
        let rest = &RUNTIME_JS[start..];
        &rest[..rest.find('}').expect("unterminated WELD_KEYS literal")]
    }

    fn keys(tag: &str) -> Vec<String> {
        let decl = format!("{tag}: [");
        let start = body()
            .find(&decl)
            .unwrap_or_else(|| panic!("WELD_KEYS has no `{tag}`"))
            + decl.len();
        let rest = &body()[start..];
        rest[..rest.find(']').expect("unterminated key list")]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_matches('\'').to_string())
            .collect()
    }

    let tags: Vec<String> = body()
        .split(']')
        .filter_map(|chunk| chunk.split_once(':'))
        .map(|(tag, _)| tag.trim().trim_start_matches(',').trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    // Every container carries an entry: `list_item` welds on a *subset* of its
    // payload (see the `start` case below), so a missing one cannot be stood in
    // for by comparing the bag whole.
    assert_eq!(tags, container_tags());

    let li = |ordered, start, ordinal, instance| Container::ListItem {
        ordered,
        start,
        ordinal,
        instance,
    };
    let base = li(false, 1, 0, 0);
    let reacts: Vec<&str> = [
        ("ordered", li(true, 1, 0, 0)),
        ("start", li(false, 3, 0, 0)),
        ("ordinal", li(false, 1, 1, 0)),
        ("instance", li(false, 1, 0, 1)),
    ]
    .into_iter()
    .filter(|(_, other)| !base.same_weld(other))
    .map(|(name, _)| name)
    .collect();
    assert_eq!(keys("list_item"), reacts);
    // `start` and `ordinal` ride `attrs` like everything else, and welding
    // ignores them, so comparing the bag whole spends a discriminator the
    // projection does not need.
    assert!(base.same_weld(&li(false, 3, 1, 0)));
    assert_ne!(base.attrs(), li(false, 3, 1, 0).attrs());

    assert!(keys("quote").is_empty());
    assert!(Container::Quote { instance: 0 }.same_weld(&Container::Quote { instance: 1 }));
}
