//! `quill.license` and `quill.homepage` over generated text, carried into
//! `Quill.yaml` as JSON-escaped double-quoted scalars: SPDX terms from the
//! real list in every case, operators in every case, refs, parens, `+`, `/`
//! and arbitrary Unicode for a license; `http(s)://` and arbitrary text, user
//! names, hosts and ports for a homepage. The load never panics; a value it
//! keeps is the input (trimmed, for a license), and a value it refuses is one
//! error under its key's code.

use proptest::prelude::*;
use spdx::identifiers::{EXCEPTIONS, LICENSES};

use super::*;

/// The load of `key: text`, or `None` when the YAML envelope does not carry
/// `text` verbatim, which leaves the seam unexercised.
fn load_key(key: &str, text: &str) -> Option<Result<(QuillConfig, Vec<Diagnostic>), Vec<Diagnostic>>> {
    let yaml = with_header(&format!("  {key}: {}\n", serde_json::to_string(text).unwrap()));
    let carried = crate::value::parse_yaml::<serde_json::Value>(&yaml)
        .ok()
        .is_some_and(|v| v["quill"][key].as_str() == Some(text));
    carried.then(|| QuillConfig::from_yaml_with_warnings(&yaml))
}

fn license_term() -> impl Strategy<Value = String> {
    let listed = (0..LICENSES.len(), prop_oneof![24 => Just(0u8), 1 => Just(1), 1 => Just(2)])
        .prop_map(|(i, case)| match case {
            0 => LICENSES[i].name.to_string(),
            1 => LICENSES[i].name.to_lowercase(),
            _ => LICENSES[i].name.to_uppercase(),
        });
    let plus = prop_oneof![7 => Just(""), 1 => Just("+")];
    prop_oneof![
        24 => (listed, plus).prop_map(|(name, plus)| name + plus),
        2 => "(Document|License|Addition)Ref-[a-zA-Z0-9.:-]{0,4}",
        1 => prop_oneof![Just("NOASSERTION".to_string()), Just("NONE".to_string())],
        1 => "\\PC{1,3}",
    ]
}

fn license_expression() -> impl Strategy<Value = String> {
    let with = prop_oneof![5 => Just("WITH"), 1 => Just("with")];
    let exception = prop::option::weighted(0.2, (with, 0..EXCEPTIONS.len()));
    let leaf = (license_term(), exception).prop_map(|(term, exception)| match exception {
        Some((with, i)) => format!("{term} {with} {}", EXCEPTIONS[i].name),
        None => term,
    });
    leaf.prop_recursive(3, 16, 2, |inner| {
        let op = prop_oneof![
            8 => Just(" AND "),
            8 => Just(" OR "),
            1 => Just(" and "),
            1 => Just(" Or "),
            1 => Just("/"),
            1 => Just(" "),
            1 => Just("\n"),
        ];
        (inner.clone(), op, inner, any::<bool>()).prop_map(|(a, op, b, paren)| {
            let joined = format!("{a}{op}{b}");
            if paren { format!("({joined})") } else { joined }
        })
    })
}

fn license() -> impl Strategy<Value = String> {
    let pad = || prop_oneof![4 => Just(""), 2 => Just(" "), 1 => Just("\t "), 1 => Just("\n"), 1 => Just("\u{2028}")];
    prop_oneof![
        4 => (pad(), license_expression(), pad()).prop_map(|(a, e, b)| format!("{a}{e}{b}")),
        1 => any::<String>(),
    ]
}

fn homepage() -> impl Strategy<Value = String> {
    let scheme = prop_oneof![
        4 => Just("https"),
        2 => Just("HTTP"),
        1 => Just("ftp"),
        1 => Just("javascript"),
        1 => Just("https:/"),
    ];
    let user = prop_oneof![6 => Just(""), 1 => Just("user@"), 1 => Just("a:b@"), 1 => Just("@")];
    let host = prop_oneof![
        4 => "[a-z0-9-]{1,6}(\\.[a-z0-9.-]{0,6}){0,2}",
        1 => Just("例え.jp".to_string()),
        1 => "\\[[0-9a-f:.%]{0,8}\\]?",
        1 => "[0-9]{1,3}(\\.[0-9]{1,4}){0,4}",
        1 => "\\PC{0,4}",
    ];
    let port = prop_oneof![3 => Just(String::new()), 2 => ":[0-9]{1,5}", 1 => ":[0-9a-z+]{0,6}"];
    let shaped = (scheme, user, host, port, "\\PC{0,6}")
        .prop_map(|(scheme, user, host, port, rest)| format!("{scheme}://{user}{host}{port}/{rest}"));
    prop_oneof![
        4 => shaped,
        1 => any::<String>().prop_map(|text| format!("https://{text}")),
        1 => any::<String>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_license_loads_trimmed_or_is_refused_once(text in license()) {
        let Some(loaded) = load_key("license", &text) else { return Ok(()) };
        match loaded {
            Ok((config, warnings)) => {
                prop_assert!(
                    !text.contains(['\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}']),
                    "a line break loaded: {text:?}"
                );
                prop_assert_eq!(config.license.as_deref(), Some(text.trim()));
                prop_assert!(
                    codes(&warnings).iter().all(|code| *code == "quill::deprecated_license")
                        && warnings.len() <= 1,
                    "{text:?}: {warnings:?}"
                );
            }
            Err(errors) => prop_assert_eq!(codes(&errors), ["quill::invalid_license"], "{:?}", text),
        }
    }

    #[test]
    fn a_homepage_loads_as_written_or_is_refused_once(text in homepage()) {
        let Some(loaded) = load_key("homepage", &text) else { return Ok(()) };
        match loaded {
            Ok((config, warnings)) => {
                prop_assert!(warnings.is_empty(), "{text:?}: {warnings:?}");
                prop_assert_eq!(config.homepage.as_deref(), Some(text.as_str()));
            }
            Err(errors) => prop_assert_eq!(codes(&errors), ["quill::invalid_homepage"], "{:?}", text),
        }
    }
}
