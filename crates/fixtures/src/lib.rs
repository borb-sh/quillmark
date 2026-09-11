use std::path::{Path, PathBuf};

/// Path to `resources/<name>` in this crate.
pub fn resource_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("resources").join(name)
}

/// Path to a quill fixture in `resources/quills/`.
///
/// If the quill directory contains versioned subdirectories (e.g. `0.1.0/`),
/// the latest version directory is returned automatically.
pub fn quills_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let quill_dir = Path::new(manifest_dir)
        .join("resources")
        .join("quills")
        .join(name);

    if quill_dir.join("Quill.yaml").exists() {
        return quill_dir;
    }

    if let Ok(entries) = std::fs::read_dir(&quill_dir) {
        let mut versions: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        versions.sort_by(|a, b| {
            let parse =
                |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
            parse(a).cmp(&parse(b))
        });
        if let Some(latest) = versions.last() {
            return quill_dir.join(latest);
        }
    }

    quill_dir
}

/// Names of every quill fixture in `resources/quills/`, sorted. A directory
/// counts when [`quills_path`] resolves it to a bundle carrying `Quill.yaml`,
/// so a versioned layout lists under its bare name and a stray directory is
/// skipped.
///
/// Panics when nothing resolves: a sweep over an empty list proves nothing.
pub fn quill_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(resource_path("quills"))
        .expect("fixtures carry a quills directory")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| quills_path(name).join("Quill.yaml").is_file())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no quill fixtures found");
    names
}

/// Path to this crate's `output/` directory, where examples write their artifacts.
pub fn example_output_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("output")
}

/// Write `content` to [`example_output_dir`]`/<name>`, creating the directory if absent.
pub fn write_example_output(name: &str, content: &[u8]) -> Result<(), std::io::Error> {
    use std::fs;

    let output_dir = example_output_dir();
    fs::create_dir_all(&output_dir)?;

    let output_path = output_dir.join(name);
    fs::write(output_path, content)?;

    Ok(())
}
