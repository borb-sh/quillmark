//! The entries a filesystem walk leaves out of a quill bundle.
use std::ffi::OsStr;
use std::path::Path;

/// Dropped with their subtrees, at the bundle root.
const IGNORED_ROOTS: [&str; 3] = [".git", "target", "node_modules"];

/// Dropped wherever it sits.
const IGNORED_NAME: &str = ".gitignore";

#[derive(Debug, Clone, Default)]
pub struct QuillIgnore;

impl QuillIgnore {
    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        let mut components = path.as_ref().components().map(|c| c.as_os_str());
        let Some(root) = components.next() else {
            return false;
        };
        IGNORED_ROOTS.iter().any(|dir| root == OsStr::new(dir))
            || components.next_back().unwrap_or(root) == OsStr::new(IGNORED_NAME)
    }
}
