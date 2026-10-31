//! Quill versions (`MAJOR.MINOR.PATCH`) and the `$quill` references that
//! select them.

use std::fmt;
use std::str::FromStr;

/// A `MAJOR.MINOR.PATCH` version. [`FromStr`] also reads two-segment
/// `MAJOR.MINOR`, patch 0; a quill's own `version` is held to all three.
///
/// Field order is the comparison order: the derived `Ord` is semver precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

/// One numeric segment: ASCII digits, nothing else. `u32::from_str` alone also
/// takes a leading `+`, a spelling no `Display` writes back.
fn parse_segment(s: &str, label: &str) -> Result<u32, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "Invalid {} version '{}': must be a number",
            label, s
        ));
    }
    s.parse::<u32>()
        .map_err(|_| format!("Invalid {} version '{}': must be a number", label, s))
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();

        if !matches!(parts.len(), 2 | 3) {
            return Err(format!(
                "Invalid version format '{}': expected MAJOR.MINOR.PATCH or MAJOR.MINOR (e.g., '2.1.0' or '2.1')",
                s
            ));
        }

        let major = parse_segment(parts[0], "major")?;
        let minor = parse_segment(parts[1], "minor")?;
        let patch = if parts.len() == 3 {
            parse_segment(parts[2], "patch")?
        } else {
            0
        };

        Ok(Version {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Specifies which version of a Quill template to use.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionSelector {
    /// Match exactly this version (e.g., "@2.1.0")
    Exact(Version),
    /// Match any patch in this minor series (e.g., "@2.1")
    Minor(u32, u32),
    /// Match any version: the bare name, with no selector written
    Any,
}

impl VersionSelector {
    /// Whether `v` satisfies this selector. A compatibility check, not
    /// resolution: `false` is the `quill::version_mismatch` render error.
    pub fn matches(&self, v: Version) -> bool {
        match self {
            VersionSelector::Exact(want) => *want == v,
            VersionSelector::Minor(major, minor) => v.major == *major && v.minor == *minor,
            VersionSelector::Any => true,
        }
    }

    /// The selector as written after `@`, unprefixed: `2.1` or `2.1.0`.
    /// Empty is the typo `name@`, not a spelling of the bare name. A lone
    /// `MAJOR` is refused: the compatibility boundary is `MAJOR.MINOR`.
    pub(crate) fn from_token(token: &str) -> Result<Self, String> {
        if token.is_empty() {
            return Err(
                "Invalid version selector '@': `@` carries no selector; omit it to match any version, or write one"
                    .to_string(),
            );
        }
        if token == "latest" {
            return Err(
                "Invalid version selector '@latest': omit the selector to match any version"
                    .to_string(),
            );
        }

        match token.split('.').count() {
            2 => Version::from_str(token).map(|v| VersionSelector::Minor(v.major, v.minor)),
            3 => Version::from_str(token).map(VersionSelector::Exact),
            1 if token.bytes().all(|b| b.is_ascii_digit()) => Err(format!(
                "Invalid version selector '{token}': a major alone selects no compatible \
                 series; write `@{token}.MINOR` for one minor series, or \
                 `@{token}.MINOR.PATCH` for one version"
            )),
            _ => Err(format!(
                "Invalid version selector '{token}': expected MAJOR.MINOR or MAJOR.MINOR.PATCH"
            )),
        }
    }
}

impl FromStr for VersionSelector {
    type Err = String;

    /// The written selector, `@` and all. An empty string is an absent
    /// selector, which is [`VersionSelector::Any`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.strip_prefix('@') {
            Some(token) => Self::from_token(token),
            None if s.is_empty() => Ok(VersionSelector::Any),
            None => Self::from_token(s),
        }
    }
}

/// Canonical, author-facing `$quill` reference grammar.
const QUILL_REF_HINT: &str = "A $quill reference is `<name>` or `<name>@<selector>`, where \
`<name>` is a quill name or `<collection>.<name>`. Each name segment matches `[a-z][a-z0-9_]*` \
(a lowercase letter, then lowercase letters, digits, or underscores), at most 64 characters. \
The optional version selector is \
`@MAJOR.MINOR` (any patch in that series) or `@MAJOR.MINOR.PATCH` (exactly that version), each \
segment plain digits. The bare `<name>` matches any version; there is no `@MAJOR` and no \
`@latest`.";

/// Single source of truth for the grammar [`QuillReference::from_str`] enforces:
/// bindings surface it and it rides as the `hint` on the
/// `parse::invalid_quill_reference` diagnostic, so the text cannot drift from
/// the parser.
pub fn quill_ref_hint() -> &'static str {
    QUILL_REF_HINT
}

pub const MAX_QUILL_NAME_LEN: usize = 64;

/// Whether `name` is a quill name: `[a-z][a-z0-9_]*`, at most
/// [`MAX_QUILL_NAME_LEN`] bytes. A collection name is held to the same rule.
pub fn is_quill_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_QUILL_NAME_LEN
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A `$quill` reference: `[<collection>.]<name>[@<selector>]`.
///
/// `name` is the quill's own `Quill.yaml` name. `collection` names the
/// collection a host resolves it in; Quillmark never reads it, so a loaded
/// quill answers a reference whatever collection it names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QuillReference {
    pub collection: Option<String>,
    pub name: String,
    pub selector: VersionSelector,
}

impl QuillReference {
    pub fn new(name: String, selector: VersionSelector) -> Self {
        Self {
            collection: None,
            name,
            selector,
        }
    }
}

impl FromStr for QuillReference {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let separator_idx = s.find('@');

        let (name_part, version_part_opt) = match separator_idx {
            Some(idx) => (&s[..idx], Some(&s[idx + 1..])),
            None => (s, None),
        };

        if name_part.is_empty() {
            return Err("Quill name cannot be empty".to_string());
        }

        let (collection, name) = match name_part.split_once('.') {
            Some((collection, name)) => (Some(collection), name),
            None => (None, name_part),
        };

        for segment in collection.iter().chain([&name]) {
            if !is_quill_name(segment) {
                return Err(format!(
                    "Invalid Quill name '{}': each name segment must match [a-z][a-z0-9_]* \
                     and be at most {} characters",
                    name_part, MAX_QUILL_NAME_LEN
                ));
            }
        }

        let selector = match version_part_opt {
            Some(token) => VersionSelector::from_token(token)?,
            None => VersionSelector::Any,
        };

        Ok(QuillReference {
            collection: collection.map(str::to_string),
            name: name.to_string(),
            selector,
        })
    }
}

/// `VersionSelector` has no `Display`: `Any` is written as an absence, which
/// reads as nothing outside a reference.
impl fmt::Display for QuillReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(collection) = &self.collection {
            write!(f, "{}.", collection)?;
        }
        f.write_str(&self.name)?;
        match self.selector {
            VersionSelector::Exact(v) => write!(f, "@{}", v),
            VersionSelector::Minor(major, minor) => write!(f, "@{}.{}", major, minor),
            VersionSelector::Any => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v = Version::from_str("2.1.0").unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
        assert_eq!(v.to_string(), "2.1.0");
    }

    #[test]
    fn test_version_parsing_two_segment_fills_patch_zero() {
        let v = Version::from_str("2.1").unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 0);
        assert_eq!(v.to_string(), "2.1.0");
    }

    #[test]
    fn test_version_invalid() {
        assert!(Version::from_str("2").is_err());
        assert!(Version::from_str("2.1.0.0").is_err());
        assert!(Version::from_str("abc").is_err());
        assert!(Version::from_str("2.x").is_err());
        assert!(Version::from_str("2.1.x").is_err());
    }

    /// The grammar `quill_ref_hint` promises is plain digits, and `Display`
    /// never writes a sign back.
    #[test]
    fn a_signed_segment_is_not_a_version() {
        assert!(Version::from_str("+2.1").is_err());
        assert!(Version::from_str("2.+1").is_err());
        assert!(Version::from_str("2.1.+0").is_err());
        assert!(QuillReference::from_str("memo@+2.+1").is_err());
        assert!(VersionSelector::from_str("@+2.1").is_err());
    }

    /// `memo@` is a typo for `memo`, not a spelling of it: an absent selector
    /// matches any version, a written one has to say something.
    #[test]
    fn a_trailing_at_is_not_a_selector() {
        assert!(QuillReference::from_str("memo@").is_err());
        assert!(VersionSelector::from_str("@").is_err());
    }

    #[test]
    fn test_version_ordering() {
        let v1_0_0 = Version::new(1, 0, 0);
        let v1_0_1 = Version::new(1, 0, 1);
        let v1_1_0 = Version::new(1, 1, 0);
        let v2_0_0 = Version::new(2, 0, 0);
        let v2_1_0 = Version::new(2, 1, 0);

        assert!(v1_0_0 < v1_0_1);
        assert!(v1_0_1 < v1_1_0);
        assert!(v1_1_0 < v2_0_0);
        assert!(v2_0_0 < v2_1_0);
        assert_eq!(v1_0_0, v1_0_0);
    }

    #[test]
    fn test_version_selector_without_at() {
        let exact = VersionSelector::from_str("2.1.0").unwrap();
        assert_eq!(exact, VersionSelector::Exact(Version::new(2, 1, 0)));

        let minor = VersionSelector::from_str("2.1").unwrap();
        assert_eq!(minor, VersionSelector::Minor(2, 1));
    }

    #[test]
    fn test_version_selector_matches() {
        let v2_1_0 = Version::new(2, 1, 0);
        let v2_1_3 = Version::new(2, 1, 3);
        let v2_2_0 = Version::new(2, 2, 0);
        let v3_0_0 = Version::new(3, 0, 0);

        let exact = VersionSelector::Exact(v2_1_0);
        assert!(exact.matches(v2_1_0));
        assert!(!exact.matches(v2_1_3));
        assert!(!exact.matches(v2_2_0));

        let minor = VersionSelector::Minor(2, 1);
        assert!(minor.matches(v2_1_0));
        assert!(minor.matches(v2_1_3));
        assert!(!minor.matches(v2_2_0));
        assert!(!minor.matches(v3_0_0));

        let any = VersionSelector::Any;
        assert!(any.matches(v2_1_0));
        assert!(any.matches(v3_0_0));
    }

    /// The bare name is the only way to match any version: `@latest` is
    /// refused, so no second spelling of `Any` exists to be erased by a round
    /// trip.
    #[test]
    fn at_latest_is_not_a_selector() {
        assert!(QuillReference::from_str("memo@latest").is_err());
        assert!(VersionSelector::from_str("@latest").is_err());
    }

    #[test]
    fn test_quill_reference_invalid_names() {
        assert!(QuillReference::from_str("Resume@2.1.0").is_err());
        assert!(QuillReference::from_str("1resume@2.1.0").is_err());
        assert!(QuillReference::from_str("resume-template@2.1.0").is_err());
        assert!(QuillReference::from_str("_private@2.1.0").is_err());
        assert!(QuillReference::from_str("a.b.c@2.1.0").is_err());
        assert!(QuillReference::from_str(".resume").is_err());
        assert!(QuillReference::from_str("borb.").is_err());
        assert!(QuillReference::from_str("Borb.resume").is_err());
        assert!(QuillReference::from_str(&"a".repeat(65)).is_err());
        assert!(QuillReference::from_str(&"a".repeat(64)).is_ok());
        assert!(QuillReference::from_str("resume_template@2.1.0").is_ok());
        assert!(QuillReference::from_str("template2@2.1.0").is_ok());
    }

    #[test]
    fn a_collection_prefixes_the_name() {
        let reference = QuillReference::from_str("borb.resume@2.1").unwrap();
        assert_eq!(reference.collection.as_deref(), Some("borb"));
        assert_eq!(reference.name, "resume");
        assert_eq!(reference.selector, VersionSelector::Minor(2, 1));
        assert_eq!(reference.to_string(), "borb.resume@2.1");
    }

    /// Every reference prints the one spelling that parses back to it; `Any`
    /// is the bare name.
    #[test]
    fn test_quill_reference_display_round_trips() {
        for (selector, written) in [
            (
                VersionSelector::Exact(Version::new(2, 1, 0)),
                "resume@2.1.0",
            ),
            (VersionSelector::Minor(2, 1), "resume@2.1"),
            (VersionSelector::Any, "resume"),
        ] {
            let reference = QuillReference::new("resume".to_string(), selector);
            assert_eq!(reference.to_string(), written);
            assert_eq!(QuillReference::from_str(written).unwrap(), reference);
        }
    }
}
