//! Filesystem loading for quills, kept here so fs access stays out of the
//! fs-agnostic core.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use quillmark_core::{
    error::RenderError,
    quill::{BUNDLE_EXTENSION, FileTreeNode, Quill, QuillIgnore},
};

/// Load a quill from a filesystem directory, skipping what [`QuillIgnore`]
/// excludes, or from a `.quill` bundle file, read whole as
/// [`Quill::from_bundle`] reads it.
///
/// A pure config load: the declared backend is resolved later, at render time.
/// For an in-memory tree, call [`Quill::from_tree`]. Advisory diagnostics ride
/// the quill, readable from [`Quill::warnings`].
pub fn quill_from_path<P: AsRef<Path>>(path: P) -> Result<Quill, RenderError> {
    let path = path.as_ref();
    let load_failed = |e: &dyn std::fmt::Display| {
        RenderError::coded("quill::load_failed", format!("Failed to load quill: {e}"))
    };
    if is_bundle_file(path) {
        let bytes = std::fs::read(path).map_err(|e| load_failed(&e))?;
        return Quill::from_bundle(&bytes).map_err(RenderError::new);
    }
    let tree = tree_from_path(path).map_err(|e| load_failed(&e))?;
    Quill::from_tree(tree).map_err(RenderError::new)
}

fn is_bundle_file(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e == BUNDLE_EXTENSION)
}

/// Walk a filesystem path into an in-memory [`FileTreeNode`], for a caller that
/// wants to edit the tree before [`Quill::from_tree`] reads it. Skips what
/// [`QuillIgnore`] excludes.
pub fn tree_from_path<P: AsRef<Path>>(
    path: P,
) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    let path = path.as_ref();
    // The root is the one directory whose absence is the caller's mistake
    // rather than a walk detail: unchecked, a typo'd path walks to an empty
    // tree and fails as `Quill.yaml not found in file tree`.
    if !path.is_dir() {
        if path.is_file() {
            return Err(format!(
                "Quill path is a file, not a directory: {}. A quill is the directory \
                 holding Quill.yaml, or a .quill bundle of it.",
                path.display()
            )
            .into());
        }
        return Err(format!("Quill directory not found: {}", path.display()).into());
    }

    load_dir(path, path)
}

/// Bounds one oversize file from an untrusted directory; neither file count nor
/// aggregate bytes is capped.
const MAX_QUILL_FILE_SIZE: u64 = 50 * 1024 * 1024;

fn load_dir(
    current_dir: &Path,
    base_dir: &Path,
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

        if QuillIgnore.is_ignored(&relative_path) {
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
            let subdir_tree = load_dir(&path, base_dir)?;
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

        let tree = tree_from_path(root).unwrap();
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

        let tree = tree_from_path(root).unwrap();
        assert!(tree.get_file(".gitignore").is_none());
        assert!(tree.get_file("target/debug/build.log").is_none());
        assert_eq!(tree.get_file("nested/plate.typ"), Some(&b"keep"[..]));
    }

    #[test]
    fn a_quill_file_loads_the_quill_its_directory_does() {
        let from_dir = quill_from_path(quillmark_fixtures::quills_path("usaf_memo")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("usaf_memo.quill");
        std::fs::write(&file, from_dir.to_bundle().unwrap()).unwrap();

        assert_eq!(
            quill_from_path(&file).unwrap().to_tree(),
            from_dir.to_tree()
        );

        let zip = dir.path().join("usaf_memo.zip");
        std::fs::rename(&file, &zip).unwrap();
        let refused = quill_from_path(&zip).unwrap_err();
        assert_eq!(
            refused.diagnostics()[0].code.as_deref(),
            Some("quill::load_failed")
        );
    }
}
