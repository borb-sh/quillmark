//! The `.quill` bundle: one quill's file tree as one zip archive, specified in
//! `prose/references/quill-bundle.md`.

use crate::error::{Diagnostic, Severity};

use super::{FileTreeNode, Quill};

/// The extension a bundle's file name ends in, without its dot.
pub const BUNDLE_EXTENSION: &str = "quill";
/// The media type a bundle is served as.
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.quillmark.quill+zip";
/// The most entries a bundle's central directory may list.
pub const MAX_BUNDLE_ENTRIES: usize = 2048;
/// The most bytes one file in a bundle may unpack to.
pub const MAX_BUNDLE_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// The most bytes a bundle's files may unpack to together.
pub const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const END_SIG: u32 = 0x0605_4b50;
const LOCAL_LEN: usize = 30;
const CENTRAL_LEN: usize = 46;
const END_LEN: usize = 22;

const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const FLAG_ENCRYPTED: u16 = 1;
const FLAG_STRONG_ENCRYPTION: u16 = 1 << 6;
const FLAG_UTF8: u16 = 1 << 11;
const ZIP64_U16: u16 = u16::MAX;
const ZIP64_U32: u32 = u32::MAX;
const MADE_BY_UNIX: u16 = 3;
const UNIX_TYPE_MASK: u32 = 0o170000;
const UNIX_SYMLINK: u32 = 0o120000;

const VERSION: u16 = 20;
const DOS_DATE_1980_01_01: u16 = (1 << 5) | 1;
const DEFLATE_LEVEL: u8 = 6;

fn refusal(code: &str, message: String) -> Diagnostic {
    Diagnostic::new(Severity::Error, message).with_code(code.to_string())
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    refusal(
        "quill::bundle_invalid",
        format!("Not a readable .quill bundle: {}", message.into()),
    )
}

fn unsupported(message: impl Into<String>) -> Diagnostic {
    refusal(
        "quill::bundle_unsupported",
        format!("The .quill bundle uses a zip feature the format leaves out: {}", message.into()),
    )
}

fn bad_path(path: &str, why: &str) -> Diagnostic {
    refusal(
        "quill::bundle_path",
        format!("The .quill bundle entry {path:?} {why}"),
    )
}

fn too_large(message: String) -> Diagnostic {
    refusal("quill::bundle_too_large", message)
}

/// The budget, spent one entry at a time off the sizes it declares.
#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: u64,
}

impl Budget {
    fn spend(&mut self, path: &str, size: u64) -> Result<(), Diagnostic> {
        if size > MAX_BUNDLE_FILE_BYTES {
            return Err(too_large(format!(
                "The .quill bundle file {path:?} unpacks to {size} bytes, over the \
                 {MAX_BUNDLE_FILE_BYTES} one file may"
            )));
        }
        self.bytes += size;
        if self.bytes > MAX_BUNDLE_BYTES {
            return Err(too_large(format!(
                "The .quill bundle unpacks to over {MAX_BUNDLE_BYTES} bytes, the most one may"
            )));
        }
        Ok(())
    }

    fn count(&mut self, entries: usize) -> Result<(), Diagnostic> {
        self.entries += entries;
        if self.entries > MAX_BUNDLE_ENTRIES {
            return Err(too_large(format!(
                "The .quill bundle lists {} entries, over the {MAX_BUNDLE_ENTRIES} one may",
                self.entries
            )));
        }
        Ok(())
    }
}

/// Holds an entry name to the format's path rules: relative, `/`-separated,
/// and every segment a name a file can carry on every platform.
fn check_path(path: &str) -> Result<(), Diagnostic> {
    if path.is_empty() {
        return Err(bad_path(path, "has an empty name"));
    }
    if path.starts_with('/') {
        return Err(bad_path(path, "is absolute; entry names are relative to the quill root"));
    }
    if let Some(c) = path.chars().find(|c| matches!(c, '\\' | ':' | '\0')) {
        return Err(bad_path(
            path,
            &format!("holds {c:?}; entry names separate segments with '/' alone"),
        ));
    }
    for segment in path.split('/') {
        match segment {
            "" => return Err(bad_path(path, "has an empty segment")),
            "." | ".." => {
                return Err(bad_path(path, &format!("has a {segment:?} segment")));
            }
            _ => {}
        }
    }
    Ok(())
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The end-of-central-directory record: the last offset holding its
/// signature whose comment runs exactly to the end of the bytes.
fn find_end(bytes: &[u8]) -> Option<usize> {
    let last = bytes.len().checked_sub(END_LEN)?;
    let first = last.saturating_sub(usize::from(u16::MAX));
    (first..=last).rev().find(|&at| {
        u32_at(bytes, at) == Some(END_SIG)
            && u16_at(bytes, at + 20).map(|len| at + END_LEN + usize::from(len)) == Some(bytes.len())
    })
}

struct Entry<'a> {
    path: &'a str,
    method: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    offset: usize,
}

/// Every entry the central directory lists, each held to the format and the
/// budget before any byte is inflated: the file entries, and the paths of the
/// directory entries.
fn central_directory(bytes: &[u8]) -> Result<(Vec<Entry<'_>>, Vec<&str>), Diagnostic> {
    let end = find_end(bytes).ok_or_else(|| invalid("no zip end-of-central-directory record"))?;
    let field16 = |at: usize| u16_at(bytes, end + at).ok_or_else(|| invalid("truncated end record"));
    let field32 = |at: usize| u32_at(bytes, end + at).ok_or_else(|| invalid("truncated end record"));
    let (disk, directory_disk) = (field16(4)?, field16(6)?);
    let (on_disk, total) = (field16(8)?, field16(10)?);
    let (directory_len, directory_at) = (field32(12)?, field32(16)?);

    if [disk, directory_disk, on_disk, total].contains(&ZIP64_U16)
        || [directory_len, directory_at].contains(&ZIP64_U32)
    {
        return Err(unsupported("ZIP64"));
    }
    if disk != 0 || directory_disk != 0 || on_disk != total {
        return Err(unsupported("an archive split across disks"));
    }

    let mut budget = Budget::default();
    budget.count(usize::from(total))?;

    let directory_at = directory_at as usize;
    let directory_end = directory_at
        .checked_add(directory_len as usize)
        .filter(|&e| e <= end)
        .ok_or_else(|| invalid("the central directory runs past the end record"))?;
    let directory = &bytes[..directory_end];

    let mut entries = Vec::new();
    let mut directories = Vec::new();
    let mut at = directory_at;
    for _ in 0..total {
        let field16 = |o: usize| u16_at(directory, at + o).ok_or_else(|| invalid("truncated central directory"));
        let field32 = |o: usize| u32_at(directory, at + o).ok_or_else(|| invalid("truncated central directory"));
        if field32(0)? != CENTRAL_SIG {
            return Err(invalid("a central directory entry has no signature"));
        }
        let made_by = field16(4)?;
        let flags = field16(8)?;
        let method = field16(10)?;
        let crc = field32(16)?;
        let compressed = field32(20)?;
        let size = field32(24)?;
        let name_len = usize::from(field16(28)?);
        let extra_len = usize::from(field16(30)?);
        let comment_len = usize::from(field16(32)?);
        let external = field32(38)?;
        let offset = field32(42)?;
        let name_at = at + CENTRAL_LEN;
        let name = directory
            .get(name_at..name_at + name_len)
            .ok_or_else(|| invalid("truncated central directory"))?;
        at = name_at + name_len + extra_len + comment_len;

        let name = std::str::from_utf8(name).map_err(|_| {
            bad_path(&String::from_utf8_lossy(name), "is not UTF-8")
        })?;
        if flags & (FLAG_ENCRYPTED | FLAG_STRONG_ENCRYPTION) != 0 {
            return Err(unsupported(format!("{name:?} is encrypted")));
        }
        if [compressed, size, offset].contains(&ZIP64_U32) {
            return Err(unsupported("ZIP64"));
        }
        if method != STORED && method != DEFLATED {
            return Err(unsupported(format!(
                "{name:?} is compressed with method {method}; a bundle stores or deflates"
            )));
        }
        if made_by >> 8 == MADE_BY_UNIX && (external >> 16) & UNIX_TYPE_MASK == UNIX_SYMLINK {
            return Err(unsupported(format!("{name:?} is a symbolic link")));
        }

        let (path, is_dir) = match name.strip_suffix('/') {
            Some(dir) => (dir, true),
            None => (name, false),
        };
        check_path(path)?;
        if is_dir {
            if size != 0 {
                return Err(invalid(format!("directory entry {name:?} holds bytes")));
            }
            directories.push(path);
            continue;
        }
        if method == STORED && compressed != size {
            return Err(invalid(format!("stored entry {path:?} declares two sizes")));
        }
        budget.spend(path, u64::from(size))?;
        entries.push(Entry {
            path,
            method,
            crc,
            compressed: compressed as usize,
            size: size as usize,
            offset: offset as usize,
        });
    }
    Ok((entries, directories))
}

/// One entry's bytes, inflated to exactly the size it declares and checked
/// against the CRC-32 it declares.
fn contents(bytes: &[u8], entry: &Entry<'_>) -> Result<Vec<u8>, Diagnostic> {
    let path = entry.path;
    let truncated = || invalid(format!("{path:?} runs past the end of the archive"));
    let local = bytes.get(entry.offset..).ok_or_else(truncated)?;
    if u32_at(local, 0).ok_or_else(truncated)? != LOCAL_SIG {
        return Err(invalid(format!("{path:?} has no local header")));
    }
    let name_len = usize::from(u16_at(local, 26).ok_or_else(truncated)?);
    let extra_len = usize::from(u16_at(local, 28).ok_or_else(truncated)?);
    let data_at = LOCAL_LEN + name_len + extra_len;
    let data = data_at
        .checked_add(entry.compressed)
        .and_then(|data_end| local.get(data_at..data_end))
        .ok_or_else(truncated)?;

    let out = match entry.method {
        STORED => data.to_vec(),
        _ => miniz_oxide::inflate::decompress_to_vec_with_limit(data, entry.size)
            .map_err(|e| invalid(format!("{path:?} does not inflate: {e}")))?,
    };
    if out.len() != entry.size {
        return Err(invalid(format!(
            "{path:?} unpacks to {} bytes and declares {}",
            out.len(),
            entry.size
        )));
    }
    if crc32fast::hash(&out) != entry.crc {
        return Err(invalid(format!("{path:?} fails its CRC-32")));
    }
    Ok(out)
}

/// The file tree a bundle holds.
pub(crate) fn read_bundle(bytes: &[u8]) -> Result<FileTreeNode, Diagnostic> {
    let mut root = FileTreeNode::Directory {
        files: Default::default(),
    };
    let (files, directories) = central_directory(bytes)?;
    for entry in files {
        if root.get_node(entry.path).is_some() {
            return Err(bad_path(entry.path, "is listed twice, or as a file and a directory"));
        }
        let node = FileTreeNode::File {
            contents: contents(bytes, &entry)?,
        };
        root.insert(entry.path, node)
            .map_err(|_| bad_path(entry.path, "sits under a path the bundle lists as a file"))?;
    }
    for path in directories {
        let mut ancestry = path.match_indices('/').map(|(at, _)| at).chain([path.len()]);
        if ancestry.any(|end| root.get_file(&path[..end]).is_some()) {
            return Err(bad_path(path, "is listed as a directory, at or under a path the bundle lists as a file"));
        }
    }
    Ok(root)
}

/// `tree` as a bundle: file entries only, in path order, each deflated where
/// that shrinks it and stored where it does not, every timestamp 1980-01-01.
pub(crate) fn write_bundle(tree: &FileTreeNode) -> Result<Vec<u8>, Diagnostic> {
    let files = tree.flatten();
    let mut budget = Budget::default();
    budget.count(files.len())?;

    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (path, bytes) in &files {
        check_path(path)?;
        budget.spend(path, bytes.len() as u64)?;
        let crc = crc32fast::hash(bytes);
        let deflated = miniz_oxide::deflate::compress_to_vec(bytes, DEFLATE_LEVEL);
        let (method, data) = if deflated.len() < bytes.len() {
            (DEFLATED, deflated.as_slice())
        } else {
            (STORED, bytes.as_slice())
        };
        let offset = out.len() as u32;
        let name = path.as_bytes();
        let shared = |into: &mut Vec<u8>| {
            for half in [VERSION, FLAG_UTF8, method, 0, DOS_DATE_1980_01_01] {
                into.extend_from_slice(&half.to_le_bytes());
            }
            for word in [crc, data.len() as u32, bytes.len() as u32] {
                into.extend_from_slice(&word.to_le_bytes());
            }
            into.extend_from_slice(&(name.len() as u16).to_le_bytes());
            into.extend_from_slice(&0u16.to_le_bytes());
        };

        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        shared(&mut out);
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        directory.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        directory.extend_from_slice(&VERSION.to_le_bytes());
        shared(&mut directory);
        directory.extend_from_slice(&[0; 10]);
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name);
    }

    let directory_at = out.len() as u32;
    let count = files.len() as u16;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&END_SIG.to_le_bytes());
    for half in [0, 0, count, count] {
        out.extend_from_slice(&u16::to_le_bytes(half));
    }
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// The one directory a tree holds when that directory, and not the root,
/// holds `Quill.yaml`: a folder zipped rather than its contents.
fn wrapping_directory(tree: &FileTreeNode) -> Option<&str> {
    let FileTreeNode::Directory { files } = tree else {
        return None;
    };
    let mut only = files.iter();
    match (only.next(), only.next()) {
        (Some((name, node @ FileTreeNode::Directory { .. })), None)
            if node.get_file("Quill.yaml").is_some() =>
        {
            Some(name)
        }
        _ => None,
    }
}

impl Quill {
    /// Read a `.quill` bundle: the zip archive holding one quill's file tree,
    /// `Quill.yaml` at its root. The tree then loads as
    /// [`from_tree`](Self::from_tree) loads it.
    ///
    /// # Errors
    ///
    /// A bundle outside the format is one `quill::bundle_*` diagnostic, and a
    /// tree that does not load is what `from_tree` returns. A bundle that
    /// wraps the tree in one folder fails as `quill::missing_file`, its hint
    /// naming the folder.
    pub fn from_bundle(bytes: &[u8]) -> Result<Self, Vec<Diagnostic>> {
        let tree = read_bundle(bytes).map_err(|d| vec![d])?;
        let hint = wrapping_directory(&tree).map(|dir| {
            format!(
                "The archive holds the quill inside the folder {dir:?}; a bundle holds \
                 the folder's contents, with Quill.yaml at its root"
            )
        });
        Quill::from_tree(tree).map_err(|mut diags| {
            if let Some(hint) = hint {
                for d in &mut diags {
                    if d.code.as_deref() == Some("quill::missing_file") {
                        d.hint = Some(hint.clone());
                    }
                }
            }
            diags
        })
    }

    /// This quill's file tree as a `.quill` bundle. The same tree writes the
    /// same bytes.
    ///
    /// # Errors
    ///
    /// A tree no reader would take back, over the budget or holding a path
    /// the format refuses, is one `quill::bundle_*` diagnostic.
    pub fn to_bundle(&self) -> Result<Vec<u8>, Vec<Diagnostic>> {
        write_bundle(&self.files).map_err(|d| vec![d])
    }
}

#[cfg(test)]
mod tests;
