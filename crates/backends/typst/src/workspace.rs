//! The files Typst's own tooling needs to compile a plate outside Quillmark.

use std::path::PathBuf;

use quillmark_core::{
    error::RenderError,
    quill::{build_transform_schema, Quill},
};

use typst::syntax::package::PackageSpec;

use crate::helper::{self, HELPER_NAME, HELPER_NAMESPACE, HELPER_VERSION};
use crate::{world, SchemaMeta};

/// The workspace directory `--package-path` names.
pub const PACKAGES_DIR: &str = "packages";
/// The workspace directory `--font-path` names.
pub const FONTS_DIR: &str = "fonts";
/// The generated helper package's directory, under [`PACKAGES_DIR`]: present in
/// every workspace.
pub const HELPER_DIR: &str = "local/quillmark-helper";

/// A directory from which `typst compile` or `typst watch`, run with
/// `--root <quill>`, `--package-path <dir>/packages`,
/// `--font-path <dir>/fonts`, `--ignore-system-fonts` and
/// `--ignore-embedded-fonts`, compiles the plate as Quillmark does, against one
/// document's data.
#[non_exhaustive]
pub struct Workspace {
    /// Paths relative to the workspace directory, every component a plain name.
    pub files: Vec<(PathBuf, Vec<u8>)>,
    /// `typst.plate_file`, relative to the quill root.
    pub plate_file: String,
}

/// The workspace for `source` rendering `json_data`, the plate JSON
/// [`Quill::compile_checked`] returns.
///
/// The generated helper package lands under `packages/` beside every vendored
/// package, at the `{namespace}/{name}/{version}` path Typst's package
/// resolution reads. `fonts/` holds the faces the backend loads: the quill's
/// own, or the embedded fallback when it ships none. A vendored package whose
/// manifest names no valid package spec, or names the helper's, is skipped.
pub fn workspace(source: &Quill, json_data: &serde_json::Value) -> Result<Workspace, RenderError> {
    if source.backend_id() != "typst" {
        return Err(RenderError::coded(
            "typst::wrong_backend",
            format!(
                "quill '{}' renders through the '{}' backend, not Typst",
                source.name(),
                source.backend_id()
            ),
        ));
    }
    let Some(plate_file) = crate::read_plate(source)?.file else {
        return Err(RenderError::coded(
            "typst::plate_missing",
            "the quill declares no `typst.plate_file`".to_string(),
        ));
    };

    let meta = SchemaMeta::from_schema_json(build_transform_schema(source.config()).as_json());
    let (lib_typ, _) = helper::generate_lib_typ(json_data, &meta)
        .map_err(|e| RenderError::coded(e.code(), e.to_string()))?;
    let helper_dir = PathBuf::from(PACKAGES_DIR)
        .join(HELPER_DIR)
        .join(HELPER_VERSION);
    let mut files = vec![
        (helper_dir.join("lib.typ"), lib_typ.into_bytes()),
        (
            helper_dir.join("typst.toml"),
            helper::generate_typst_toml().into_bytes(),
        ),
    ];

    let helper_spec = format!("@{HELPER_NAMESPACE}/{HELPER_NAME}:{HELPER_VERSION}");
    for package_dir in source.files().list_directories("packages") {
        let Some(spec) = source
            .files()
            .get_file(package_dir.join("typst.toml"))
            .and_then(|toml| world::parse_package_toml(&String::from_utf8_lossy(toml)).ok())
            .and_then(|info| {
                format!("@{}/{}:{}", info.namespace, info.name, info.version)
                    .parse::<PackageSpec>()
                    .ok()
            })
            .filter(|spec| spec.to_string() != helper_spec)
        else {
            continue;
        };
        let target = PathBuf::from(PACKAGES_DIR)
            .join(spec.namespace.as_str())
            .join(spec.name.as_str())
            .join(spec.version.to_string());
        let pattern = format!("{}/*", package_dir.to_string_lossy());
        for path in source.files().find_files(&pattern) {
            let (Ok(relative), Some(contents)) = (
                path.strip_prefix(&package_dir),
                source.files().get_file(&path),
            ) else {
                continue;
            };
            files.push((target.join(relative), contents.to_vec()));
        }
    }

    let fonts = world::quill_fonts(source);
    if fonts.is_empty() {
        for (name, contents) in world::FALLBACK_FONTS {
            files.push((PathBuf::from(FONTS_DIR).join(name), contents.to_vec()));
        }
    } else {
        for (path, contents) in fonts {
            files.push((PathBuf::from(FONTS_DIR).join(path), contents.to_vec()));
        }
    }

    files.retain(|(path, _)| {
        path.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    });
    Ok(Workspace { files, plate_file })
}
