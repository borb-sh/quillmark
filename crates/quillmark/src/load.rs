//! Filesystem loading for quills, kept here so fs access stays out of the
//! fs-agnostic core.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use quillmark_core::{FileTreeNode, Quill, QuillIgnore, RenderError};

/// Load a quill from a filesystem directory, skipping what [`QuillIgnore`]
/// excludes.
///
/// A pure config load: the declared backend is resolved later, at render time.
/// For an in-memory tree, call [`Quill::from_tree`]. Advisory diagnostics ride
/// the quill, readable from [`Quill::warnings`].
pub fn quill_from_path<P: AsRef<Path>>(path: P) -> Result<Quill, RenderError> {
    let tree = load_tree_from_path(path.as_ref()).map_err(|e| {
        RenderError::coded("quill::load_failed", format!("Failed to load quill: {e}"))
    })?;
    Quill::from_tree(tree).map_err(RenderError::new)
}

/// Walk a filesystem path into an in-memory [`FileTreeNode`], for a caller that
/// wants to edit the tree before [`Quill::from_tree`] reads it. Skips what
/// [`QuillIgnore`] excludes (`.git/`, `target/`, …).
pub fn tree_from_path<P: AsRef<Path>>(
    path: P,
) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    load_tree_from_path(path.as_ref())
}

fn load_tree_from_path(path: &Path) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    // The root is the one directory whose absence is the caller's mistake
    // rather than a walk detail: unchecked, a typo'd path walks to an empty
    // tree and fails as `Quill.yaml not found in file tree`.
    if !path.is_dir() {
        return Err(format!("Quill directory not found: {}", path.display()).into());
    }

    load_dir(path, path, &QuillIgnore)
}

/// Bounds one oversize file from an untrusted bundle; neither file count nor
/// aggregate bytes is capped.
const MAX_QUILL_FILE_SIZE: u64 = 50 * 1024 * 1024;

fn load_dir(
    current_dir: &Path,
    base_dir: &Path,
    ignore: &QuillIgnore,
) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    use std::fs;

    let mut files = HashMap::new();
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative_path: PathBuf = path
            .strip_prefix(base_dir)
            .map_err(|e| format!("Failed to get relative path: {}", e))?
            .to_path_buf();

        if ignore.is_ignored(&relative_path) {
            continue;
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid filename: {}", path.display()))?
            .to_string();

        // symlink_metadata, so a crafted bundle cannot point a symlink at a
        // sensitive host file and pull it into the tree the backend reads.
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("Failed to stat '{}': {}", path.display(), e))?;
        let file_type = meta.file_type();

        if file_type.is_symlink() {
            continue;
        } else if file_type.is_file() {
            if meta.len() > MAX_QUILL_FILE_SIZE {
                return Err(format!(
                    "File '{}' exceeds the {} MiB quill file-size limit",
                    path.display(),
                    MAX_QUILL_FILE_SIZE / (1024 * 1024)
                )
                .into());
            }
            let contents = fs::read(&path)
                .map_err(|e| format!("Failed to read file '{}': {}", path.display(), e))?;
            files.insert(filename, FileTreeNode::File { contents });
        } else if file_type.is_dir() {
            let subdir_tree = load_dir(&path, base_dir, ignore)?;
            files.insert(filename, subdir_tree);
        }
    }

    Ok(FileTreeNode::Directory { files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn load_dir_skips_symlinks() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut f = std::fs::File::create(root.join("real.txt")).unwrap();
        f.write_all(b"ok").unwrap();

        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, b"TOPSECRET").unwrap();
        std::os::unix::fs::symlink(&secret, root.join("leak.txt")).unwrap();

        let tree = load_tree_from_path(root).unwrap();
        assert!(tree.get_file("leak.txt").is_none());
        assert_eq!(tree.get_file("real.txt"), Some(&b"ok"[..]));
    }

    #[test]
    fn load_dir_skips_what_the_ignore_set_excludes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join(".gitignore"), b"drop").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/build.log"), b"drop").unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/plate.typ"), b"keep").unwrap();

        let tree = load_tree_from_path(root).unwrap();
        assert!(tree.get_file(".gitignore").is_none());
        assert!(tree.get_file("target/debug/build.log").is_none());
        assert_eq!(tree.get_file("nested/plate.typ"), Some(&b"keep"[..]));
    }
}
