//! `quill.license` and `quill.homepage`: optional, and held to their shapes
//! when present.
use super::*;

mod properties;

fn load(keys: &str) -> Result<(QuillConfig, Vec<Diagnostic>), Vec<Diagnostic>> {
    QuillConfig::from_yaml_with_warnings(&with_header(keys))
}

#[test]
fn license_and_homepage_load_trimmed_and_as_written_and_stay_absent_when_omitted() {
    let (config, warnings) = load(
        "  license: \" (MIT OR Apache-2.0 WITH LLVM-exception) AND LicenseRef-Seal \"\n  \
         homepage: HTTPS://example.com:8443/quills?tab=memo#top\n",
    )
    .expect("both are well-formed");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        config.license.as_deref(),
        Some("(MIT OR Apache-2.0 WITH LLVM-exception) AND LicenseRef-Seal")
    );
    assert_eq!(
        config.homepage.as_deref(),
        Some("HTTPS://example.com:8443/quills?tab=memo#top")
    );

    let (bare, warnings) = load("").expect("both are optional");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!((bare.license, bare.homepage), (None, None));
}

#[test]
fn a_license_off_the_spdx_grammar_is_refused_with_its_fix() {
    for (license, fix) in [
        ("MIT/Apache-2.0", Some("'MIT OR Apache-2.0'")),
        ("MIT or Apache-2.0", Some("'MIT OR Apache-2.0'")),
        ("Apache 2.0", Some("'Apache-2.0'")),
        ("GPL-2.0+", Some("'GPL-2.0-or-later'")),
        ("MIT OR apache-2.0", Some("'Apache-2.0'")),
        ("gnu gpl v2", None),
        ("MIT OR", None),
        ("LicenseRef-", None),
        ("NOASSERTION", None),
        ("\"MIT\\nOR Apache-2.0\"", None),
        ("\"MIT\\u2028\"", None),
        ("\"MIT\\u2029OR Apache-2.0\"", None),
        ("\"MIT\\u0085\"", None),
        ("\"MIT\\vOR Apache-2.0\"", None),
        ("\"MIT\\f\"", None),
        ("42", None),
        ("~", None),
    ] {
        let refused = load(&format!("  license: {license}\n")).expect_err(license);
        assert_eq!(codes(&refused), ["quill::invalid_license"], "{license}");
        let hint = refused[0].hint.as_deref().unwrap_or_default();
        match fix {
            Some(fix) => assert!(
                hint.contains(fix),
                "{license}: the hint names {fix}: {hint}"
            ),
            None => assert!(
                !hint.starts_with("Did you mean"),
                "{license}: no guess: {hint}"
            ),
        }
    }

    let refused = load("  license: MIT WITH Seal-exception\n").expect_err("not a listed exception");
    let hint = refused[0].hint.as_deref().unwrap_or_default();
    assert!(
        !hint.contains("LicenseRef"),
        "a license ref cannot follow WITH: {hint}"
    );
}

#[test]
fn a_deprecated_license_loads_with_a_warning_naming_the_current_id() {
    let (config, warnings) = load("  license: GPL-3.0\n").expect("a deprecated id loads");
    assert_eq!(config.license.as_deref(), Some("GPL-3.0"));
    assert_eq!(codes(&warnings), ["quill::deprecated_license"]);
    let hint = warnings[0].hint.as_deref().unwrap_or_default();
    assert!(hint.contains("'GPL-3.0-only'"), "{hint}");
}

#[test]
fn a_homepage_loads_only_as_a_web_url_naming_a_host_and_no_user() {
    for homepage in [
        "https://例え.jp/",
        "http://[::1]:80/x",
        "https://1.2.3.4",
        "https://a.example:0",
    ] {
        load(&format!("  homepage: {homepage}\n")).expect(homepage);
    }
    for homepage in [
        "github.com/org/quills",
        "ftp://example.com/quills",
        "https:///quills",
        "https://..",
        "https://ghp_token@github.com/org/quills",
        "https://github.com@evil.example/",
        "https://example.com:99999/",
        "https://example.com:8a/",
        "https://[::1/",
        "https://[fe80::1%25eth0]/",
        "https://exa%41mple.com/",
        "https://a<b>.com/",
        "https://999.1.1.1/",
        "\"https://exa\\u200Bmple.com/\"",
        "\"https://example.com/my quills\"",
        "42",
        "~",
    ] {
        let refused = load(&format!("  homepage: {homepage}\n")).expect_err(homepage);
        assert_eq!(codes(&refused), ["quill::invalid_homepage"], "{homepage}");
    }
}
