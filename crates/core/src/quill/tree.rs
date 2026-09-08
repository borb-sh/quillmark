//! In-memory file tree representation for quill bundles.
use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
/// A node in a quill bundle's file tree. Out-of-crate callers build these;
/// `Quill::from_tree` takes one. Symlinks are refused at the loader rather than
/// modelled here.
#[derive(Debug, Clone)]
pub enum FileTreeNode {
    File {
        contents: Vec<u8>,
    },
    Directory {
        files: HashMap<String, FileTreeNode>,
    },
}

impl FileTreeNode {
    /// The node at `path`, the empty path being the receiver. A `..`, `.`, or
    /// absolute-root component resolves to `None` rather than being dropped:
    /// dropping it makes `get_file("a/../b")` navigate to `a/b`, an asymmetry
    /// with [`insert`](Self::insert) that would mask a caller assuming this
    /// normalizes.
    pub fn get_node<P: AsRef<Path>>(&self, path: P) -> Option<&FileTreeNode> {
        let path = path.as_ref();

        if path == Path::new("") {
            return Some(self);
        }

        let mut components: Vec<&str> = Vec::new();
        for c in path.components() {
            match c {
                std::path::Component::Normal(s) => match s.to_str() {
                    Some(s) => components.push(s),
                    None => return None,
                },
                _ => return None,
            }
        }

        if components.is_empty() {
            return Some(self);
        }

        let mut current_node = self;
        for component in components {
            match current_node {
                FileTreeNode::Directory { files } => {
                    current_node = files.get(component)?;
                }
                FileTreeNode::File { .. } => {
                    return None; // Can't traverse into a file
                }
            }
        }

        Some(current_node)
    }

    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> Option<&[u8]> {
        match self.get_node(path)? {
            FileTreeNode::File { contents } => Some(contents.as_slice()),
            FileTreeNode::Directory { .. } => None,
        }
    }

    /// List the subdirectories of a directory (non-recursive), as paths joined
    /// onto `dir_path` and so relative to the receiver.
    pub fn list_directories<P: AsRef<Path>>(&self, dir_path: P) -> Vec<PathBuf> {
        let dir_path = dir_path.as_ref();
        match self.get_node(dir_path) {
            Some(FileTreeNode::Directory { files }) => files
                .iter()
                .filter(|(_, node)| matches!(node, FileTreeNode::Directory { .. }))
                .map(|(name, _)| {
                    if dir_path == Path::new("") {
                        PathBuf::from(name)
                    } else {
                        dir_path.join(name)
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Get all files matching a pattern (supports glob-style wildcards).
    /// An invalid pattern matches nothing.
    pub fn find_files<P: AsRef<Path>>(&self, pattern: P) -> Vec<PathBuf> {
        let Ok(glob_pattern) = glob::Pattern::new(&pattern.as_ref().to_string_lossy()) else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        // Paths only: the visitor lends the contents, so no bundle bytes are
        // copied to answer a name query.
        self.for_each_file(&mut |path, _| {
            if glob_pattern.matches(path) {
                matches.push(PathBuf::from(path));
            }
        });
        matches.sort();
        matches
    }

    /// Insert `node` at `path`, creating parent directories. A `..`, `.`, or
    /// absolute-root component is an error rather than a silent no-op.
    pub fn insert<P: AsRef<Path>>(
        &mut self,
        path: P,
        node: FileTreeNode,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let path = path.as_ref();

        let mut components: Vec<String> = Vec::new();
        for c in path.components() {
            match c {
                std::path::Component::Normal(s) => {
                    components.push(
                        s.to_str()
                            .ok_or("Path component is not valid UTF-8")?
                            .to_string(),
                    );
                }
                std::path::Component::ParentDir => {
                    return Err("Path traversal ('..') is not allowed".into());
                }
                std::path::Component::CurDir => {
                    return Err("Current-directory ('.') components are not allowed".into());
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err("Absolute paths are not allowed; use a relative path".into());
                }
            }
        }

        if components.is_empty() {
            return Err("Cannot insert at root path".into());
        }

        let mut current_node = self;
        for component in &components[..components.len() - 1] {
            match current_node {
                FileTreeNode::Directory { files } => {
                    current_node =
                        files
                            .entry(component.clone())
                            .or_insert_with(|| FileTreeNode::Directory {
                                files: HashMap::new(),
                            });
                }
                FileTreeNode::File { .. } => {
                    return Err("Cannot traverse into a file".into());
                }
            }
        }

        let filename = &components[components.len() - 1];
        match current_node {
            FileTreeNode::Directory { files } => {
                files.insert(filename.clone(), node);
                Ok(())
            }
            FileTreeNode::File { .. } => Err("Cannot insert into a file".into()),
        }
    }

    /// Flatten the tree into `(path, contents)` pairs: the inverse of building
    /// a tree by `insert`-ing each path. Paths are `"/"`-joined and relative
    /// (no leading slash), exactly the key shape the WASM `Quill.fromTree`
    /// boundary consumes, so `from_tree(flatten(t))` round-trips every file.
    /// Output is sorted by path for deterministic ordering (the construction
    /// side stores children in a `HashMap`, which has no inherent order).
    ///
    /// Only files are emitted, so an empty directory yields no entry and the
    /// round trip preserves file contents, not exact directory structure.
    pub fn flatten(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        self.for_each_file(&mut |path, contents| out.push((path.to_string(), contents.to_vec())));
        out.sort_by(|(a, _), (b, _)| a.cmp(b));
        out
    }

    /// Visit every file in the tree with its `/`-joined path, depth-first in
    /// `HashMap` order (so unordered: callers that need a stable sequence sort
    /// the result). The one walk: [`flatten`](Self::flatten) copies out of it,
    /// [`find_files`](Self::find_files) only reads the paths, and neither pays
    /// for the other's work.
    fn for_each_file(&self, visit: &mut impl FnMut(&str, &[u8])) {
        self.walk_files(String::new(), visit);
    }

    fn walk_files(&self, prefix: String, visit: &mut impl FnMut(&str, &[u8])) {
        match self {
            FileTreeNode::File { contents } => {
                // A File only reaches here with a non-empty prefix: the root is
                // always a Directory, so every file is named by its parent.
                if !prefix.is_empty() {
                    visit(&prefix, contents);
                }
            }
            FileTreeNode::Directory { files } => {
                for (name, node) in files {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", prefix, name)
                    };
                    node.walk_files(path, visit);
                }
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileTreeNode {
        let mut root = FileTreeNode::Directory {
            files: std::collections::HashMap::new(),
        };
        root.insert(
            "a/b.txt",
            FileTreeNode::File {
                contents: b"hi".to_vec(),
            },
        )
        .unwrap();
        root
    }

    #[test]
    fn get_node_rejects_traversal_components() {
        let t = sample();
        assert!(t.get_file("a/b.txt").is_some());
        assert!(t.get_node("a/../b.txt").is_none());
        assert!(t.get_node("./a/b.txt").is_none());
        assert!(t.get_node("/a/b.txt").is_none());
    }
}
