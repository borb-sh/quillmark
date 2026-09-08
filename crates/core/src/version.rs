//! Semantic versioning (MAJOR.MINOR.PATCH) for Quill template references.
//! Two-segment (`MAJOR.MINOR`) versions are also accepted; patch defaults to 0.

use std::fmt;
use std::str::FromStr;

/// Semantic version number. Two-segment input defaults patch to 0.
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
        return Err(format!("Invalid {} version '{}': must be a number", label, s));
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
    /// Match latest patch version in this minor series (e.g., "@2.1")
    Minor(u32, u32),
    /// Match latest minor/patch version in this major series (e.g., "@2")
    Major(u32),
    /// Match the highest version available (e.g., "@latest" or unspecified)
    Latest,
}

impl VersionSelector {
    /// Whether `v` satisfies this selector. A compatibility check, not
    /// resolution: `false` is the `quill::version_mismatch` render error.
    pub fn matches(&self, v: Version) -> bool {
        match self {
            VersionSelector::Exact(want) => *want == v,
            VersionSelector::Minor(major, minor) => v.major == *major && v.minor == *minor,
            VersionSelector::Major(major) => v.major == *major,
            VersionSelector::Latest => true,
        }
    }
}

impl FromStr for VersionSelector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // An absent selector is `latest`; a written `@` with nothing after it is
        // a typo, not a spelling of it.
        let (written, version_str) = match s.strip_prefix('@') {
            Some(rest) => (true, rest),
            None => (false, s),
        };

        if version_str.is_empty() {
            return if written {
                Err(format!(
                    "Invalid version selector '{}': `@` carries no selector; omit it for the latest version, or write one",
                    s
                ))
            } else {
                Ok(VersionSelector::Latest)
            };
        }
        if version_str == "latest" {
            return Ok(VersionSelector::Latest);
        }

        let parts: Vec<&str> = version_str.split('.').collect();

        match parts.len() {
            2 | 3 => {
                let version = Version::from_str(version_str)?;
                Ok(if parts.len() == 3 {
                    VersionSelector::Exact(version)
                } else {
                    VersionSelector::Minor(version.major, version.minor)
                })
            }
            1 => {
                let major = parse_segment(version_str, "major").map_err(|_| {
                    format!(
                        "Invalid version selector '{}': expected number, MAJOR.MINOR, MAJOR.MINOR.PATCH, or 'latest'",
                        version_str
                    )
                })?;
                Ok(VersionSelector::Major(major))
            }
            _ => Err(format!(
                "Invalid version selector '{}': expected number, MAJOR.MINOR, MAJOR.MINOR.PATCH, or 'latest'",
                version_str
            )),
        }
    }
}

impl fmt::Display for VersionSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionSelector::Exact(v) => write!(f, "@{}", v),
            VersionSelector::Minor(major, minor) => write!(f, "@{}.{}", major, minor),
            VersionSelector::Major(m) => write!(f, "@{}", m),
            VersionSelector::Latest => write!(f, "@latest"),
        }
    }
}

/// Canonical, author-facing `$quill` reference grammar.
const QUILL_REF_HINT: &str = "A $quill reference is `<name>` or `<name>@<selector>`. \
The name must match `[a-z_][a-z0-9_]*` (start with a lowercase letter or underscore, then \
lowercase letters, digits, or underscores). The optional version selector is \
`@MAJOR.MINOR.PATCH` (exact), `@MAJOR.MINOR` (latest patch in that minor series), `@MAJOR` \
(latest in that major series), or `@latest`; omitting the selector means latest.";

/// Single source of truth for the grammar [`QuillReference::from_str`] enforces:
/// bindings surface it and it rides as the `hint` on the
/// `parse::invalid_quill_reference` diagnostic, so the text cannot drift from
/// the parser.
pub fn quill_ref_hint() -> &'static str {
    QUILL_REF_HINT
}

/// Complete reference to a Quill template with name and version selector.
///
/// Name charset: `[a-z_][a-z0-9_]*`. Selector defaults to `Latest` when omitted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QuillReference {
    pub name: String,
    pub selector: VersionSelector,
}

impl QuillReference {
    pub fn new(name: String, selector: VersionSelector) -> Self {
        Self { name, selector }
    }

    pub fn latest(name: String) -> Self {
        Self {
            name,
            selector: VersionSelector::Latest,
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

        let name = name_part.to_string();

        // Same charset as a card kind. Quill *config* names are stricter
        // (`config.rs` rejects a leading underscore), so that predicate is not
        // interchangeable with this one.
        if !crate::document::is_valid_kind_name(&name) {
            return Err(format!(
                "Invalid Quill name '{}': must match [a-z_][a-z0-9_]*",
                name
            ));
        }

        let selector = if let Some(version_part) = version_part_opt {
            VersionSelector::from_str(&format!("@{}", version_part))?
        } else {
            VersionSelector::Latest
        };

        Ok(QuillReference { name, selector })
    }
}

impl fmt::Display for QuillReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.selector {
            VersionSelector::Latest => write!(f, "{}", self.name),
            _ => write!(f, "{}{}", self.name, self.selector),
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
        assert!(VersionSelector::from_str("@+2").is_err());
    }

    /// `memo@` is a typo for `memo`, not a spelling of it: an absent selector
    /// means latest, a written one has to say something.
    #[test]
    fn a_trailing_at_is_not_a_selector() {
        assert!(QuillReference::from_str("memo@").is_err());
        assert!(VersionSelector::from_str("@").is_err());
        assert_eq!(
            VersionSelector::from_str("").unwrap(),
            VersionSelector::Latest,
            "no selector at all is still latest"
        );
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
    fn test_version_selector_parsing() {
        let exact = VersionSelector::from_str("@2.1.0").unwrap();
        assert_eq!(exact, VersionSelector::Exact(Version::new(2, 1, 0)));

        let minor = VersionSelector::from_str("@2.1").unwrap();
        assert_eq!(minor, VersionSelector::Minor(2, 1));

        let major = VersionSelector::from_str("@2").unwrap();
        assert_eq!(major, VersionSelector::Major(2));

        let latest1 = VersionSelector::from_str("@latest").unwrap();
        assert_eq!(latest1, VersionSelector::Latest);

        let latest2 = VersionSelector::from_str("").unwrap();
        assert_eq!(latest2, VersionSelector::Latest);
    }

    #[test]
    fn test_version_selector_without_at() {
        let exact = VersionSelector::from_str("2.1.0").unwrap();
        assert_eq!(exact, VersionSelector::Exact(Version::new(2, 1, 0)));

        let minor = VersionSelector::from_str("2.1").unwrap();
        assert_eq!(minor, VersionSelector::Minor(2, 1));

        let major = VersionSelector::from_str("2").unwrap();
        assert_eq!(major, VersionSelector::Major(2));
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

        let major = VersionSelector::Major(2);
        assert!(major.matches(v2_1_0));
        assert!(major.matches(v2_2_0));
        assert!(!major.matches(v3_0_0));

        let latest = VersionSelector::Latest;
        assert!(latest.matches(v2_1_0));
        assert!(latest.matches(v3_0_0));
    }

    #[test]
    fn test_version_selector_display() {
        assert_eq!(
            VersionSelector::Exact(Version::new(2, 1, 0)).to_string(),
            "@2.1.0"
        );
        assert_eq!(VersionSelector::Minor(2, 1).to_string(), "@2.1");
        assert_eq!(VersionSelector::Major(2).to_string(), "@2");
        assert_eq!(VersionSelector::Latest.to_string(), "@latest");
    }

    #[test]
    fn test_quill_reference_parsing() {
        let ref1 = QuillReference::from_str("resume_template@2.1.0").unwrap();
        assert_eq!(ref1.name, "resume_template");
        assert_eq!(ref1.selector, VersionSelector::Exact(Version::new(2, 1, 0)));

        let ref1b = QuillReference::from_str("resume_template@2.1").unwrap();
        assert_eq!(ref1b.selector, VersionSelector::Minor(2, 1));

        let ref2 = QuillReference::from_str("resume_template@2").unwrap();
        assert_eq!(ref2.selector, VersionSelector::Major(2));

        let ref3 = QuillReference::from_str("resume_template@latest").unwrap();
        assert_eq!(ref3.selector, VersionSelector::Latest);

        let ref4 = QuillReference::from_str("resume_template").unwrap();
        assert_eq!(ref4.name, "resume_template");
        assert_eq!(ref4.selector, VersionSelector::Latest);
    }

    #[test]
    fn test_quill_reference_invalid_names() {
        assert!(QuillReference::from_str("Resume@2.1.0").is_err());
        assert!(QuillReference::from_str("1resume@2.1.0").is_err());
        assert!(QuillReference::from_str("resume-template@2.1.0").is_err());
        assert!(QuillReference::from_str("resume.template@2.1.0").is_err());
        assert!(QuillReference::from_str("resume_template@2.1.0").is_ok());
        assert!(QuillReference::from_str("_private@2.1.0").is_ok());
        assert!(QuillReference::from_str("template2@2.1.0").is_ok());
    }

    #[test]
    fn test_quill_reference_display() {
        let ref1 = QuillReference::new(
            "resume".to_string(),
            VersionSelector::Exact(Version::new(2, 1, 0)),
        );
        assert_eq!(ref1.to_string(), "resume@2.1.0");

        let ref1b = QuillReference::new("resume".to_string(), VersionSelector::Minor(2, 1));
        assert_eq!(ref1b.to_string(), "resume@2.1");

        let ref2 = QuillReference::new("resume".to_string(), VersionSelector::Major(2));
        assert_eq!(ref2.to_string(), "resume@2");

        let ref3 = QuillReference::new("resume".to_string(), VersionSelector::Latest);
        assert_eq!(ref3.to_string(), "resume");
    }
}
