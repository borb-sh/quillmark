//! .quillignore parsing and path matching.
use std::path::Path;

use glob::{MatchOptions, Pattern};

/// `*` stops at a path separator, so a pattern spans one path segment unless it
/// spells the separators out — the gitignore reading of a `.quillignore` line.
const MATCH: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

#[derive(Debug, Clone)]
pub struct QuillIgnore {
    rules: Vec<Rule>,
}

/// One `.quillignore` line, parsed once at construction.
#[derive(Debug, Clone)]
enum Rule {
    /// `dir/`: the entry itself and everything beneath it.
    Dir(String),
    /// A literal name: the whole path, or the basename at any depth.
    Name(String),
    /// A glob, matched against the whole path and against the basename, so a
    /// slash-free pattern applies at any depth and a slashed one is anchored
    /// at the bundle root. As in gitignore, `*` does not cross `/`.
    ///
    /// The line it was written as matches too: `[` opens a character class and
    /// is also an ordinary character in a filename, so `Cinzel[wght].ttf` names
    /// both a glob over `Cinzelw.ttf`/`Cinzelg.ttf`/… and the variable font
    /// sitting in the usaf_memo fixture. Both readings ignore.
    Glob(Pattern, String),
}

impl Default for QuillIgnore {
    /// Built-in ignore set used when a quill directory has no `.quillignore`
    /// file. Skips VCS metadata, build artifacts, and dependency caches.
    fn default() -> Self {
        Self::new(vec![
            ".git/".to_string(),
            ".gitignore".to_string(),
            ".quillignore".to_string(),
            "target/".to_string(),
            "node_modules/".to_string(),
        ])
    }
}

impl QuillIgnore {
    pub fn new(patterns: Vec<String>) -> Self {
        Self {
            rules: patterns.into_iter().map(Rule::parse).collect(),
        }
    }

    pub fn from_content(content: &str) -> Self {
        Self::new(
            content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_string)
                .collect(),
        )
    }

    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path
            .as_ref()
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let basename = path.rsplit_once('/').map_or(path.as_str(), |(_, name)| name);
        self.rules.iter().any(|rule| rule.matches(&path, basename))
    }
}

impl Rule {
    fn parse(pattern: String) -> Self {
        if let Some(prefix) = pattern.strip_suffix('/') {
            return Rule::Dir(prefix.to_string());
        }
        if !pattern.contains(['*', '?', '[']) {
            return Rule::Name(pattern);
        }
        match Pattern::new(&pattern) {
            Ok(glob) => Rule::Glob(glob, pattern),
            // An unparseable glob still matches the name it spells out.
            Err(_) => Rule::Name(pattern),
        }
    }

    fn matches(&self, path: &str, basename: &str) -> bool {
        match self {
            Rule::Dir(prefix) => path
                .strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
            Rule::Name(name) => path == name || basename == name,
            Rule::Glob(glob, raw) => {
                path == raw
                    || basename == raw
                    || glob.matches_with(path, MATCH)
                    || glob.matches_with(basename, MATCH)
            }
        }
    }
}
