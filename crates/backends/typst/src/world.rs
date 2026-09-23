use std::collections::HashMap;
use std::path::Path;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{
    package::{PackageSpec, PackageVersion},
    FileId, RootedPath, Source, VirtualPath, VirtualRoot,
};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World};

use crate::helper;
use quillmark_core::{error::{Diagnostic, Severity}, quill::Quill};

/// One `(plate address, count)` per content field holding image islands, which
/// this backend draws nothing for.
pub(crate) type DeclinedImages = Vec<(String, usize)>;

/// One shape for assets and package files alike, so a consumer routing on the
/// code need not know which.
fn skipped_path(path: &Path, err: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        format!(
            "Skipping '{}': its path is not usable by Typst ({err})",
            path.display()
        ),
    )
    .with_code("typst::path_skipped".to_string())
    .with_hint("Rename it to a plain relative path.".to_string())
}

/// The keys under `typst:` in `Quill.yaml` this backend reads. Core stores the
/// section verbatim, so every other key would otherwise vanish unremarked.
const CONFIG_KEYS: &[&str] = &["plate_file"];

fn unread_config_keys(source: &Quill, warnings: &mut Vec<Diagnostic>) {
    let mut unread: Vec<&str> = source
        .config()
        .backend_config
        .keys()
        .map(String::as_str)
        .filter(|key| !CONFIG_KEYS.contains(key))
        .collect();
    unread.sort_unstable();
    for key in unread {
        let hint = if key == "packages" {
            "Quillmark never downloads a package: vendor each one under `packages/<dir>/` \
             with its `typst.toml`, and delete `typst.packages`."
                .to_string()
        } else {
            format!("Valid keys under 'typst' are: {}.", CONFIG_KEYS.join(", "))
        };
        warnings.push(
            Diagnostic::new(
                Severity::Warning,
                format!("Ignoring 'typst.{key}' in Quill.yaml: the Typst backend does not read it"),
            )
            .with_code("typst::unknown_config_key".to_string())
            .with_hint(hint),
        );
    }
}

/// Typst 0.15 routes file ids through [`RootedPath`]: project-local files use
/// [`VirtualRoot::Project`], package files use [`VirtualRoot::Package`].
fn file_id(spec: Option<PackageSpec>, vpath: VirtualPath) -> FileId {
    let root = spec.map_or(VirtualRoot::Project, VirtualRoot::Package);
    FileId::new(RootedPath::new(root, vpath))
}

static FALLBACK_REGULAR: &[u8] = include_bytes!("fonts/Figtree-Regular.ttf");
static FALLBACK_BOLD: &[u8] = include_bytes!("fonts/Figtree-Bold.ttf");
static FALLBACK_ITALIC: &[u8] = include_bytes!("fonts/Figtree-Italic.ttf");

/// Typst `World` implementation for quill-based compilation. Packages load from
/// `{quill}/packages/` and assets from `{quill}/assets/`.
pub(crate) struct QuillWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
    sources: HashMap<FileId, Source>,
    binaries: HashMap<FileId, Bytes>,
    /// Non-fatal defects from loading the quill's assets and packages. Without
    /// them a skipped package degrades the compile unattributably, surfacing as
    /// an unresolved `#import` naming the plate. Static for the session.
    load_warnings: Vec<Diagnostic>,
}

impl QuillWorld {
    pub(crate) fn new(
        source: &Quill,
        main: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut sources = HashMap::new();
        let mut binaries = HashMap::new();
        let mut load_warnings = Vec::new();
        unread_config_keys(source, &mut load_warnings);

        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        let font_data_list = Self::load_fonts_from_quill(source)?;
        for font_data in font_data_list {
            let font_bytes = Bytes::new(font_data);
            for font in Font::iter(font_bytes) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        // Fall back to the embedded Figtree faces when the quill ships no fonts.
        if fonts.is_empty() {
            for data in [FALLBACK_REGULAR, FALLBACK_BOLD, FALLBACK_ITALIC] {
                let font_bytes = Bytes::new(data.to_vec());
                for font in Font::iter(font_bytes) {
                    book.push(font.info().clone());
                    fonts.push(font);
                }
            }
        }

        Self::load_assets_from_quill(source, &mut binaries, &mut load_warnings)?;

        // Quillmark never downloads packages: every package a quill imports is
        // vendored under `packages/` in the quill tree.
        Self::load_packages_from_quill(source, &mut sources, &mut binaries, &mut load_warnings)?;

        // Constant for the session: re-injecting `lib.typ` leaves it untouched.
        binaries.insert(
            Self::helper_fid("typst.toml"),
            Bytes::new(helper::generate_typst_toml().into_bytes()),
        );

        let main_id = file_id(
            None,
            VirtualPath::new("main.typ").expect("\"main.typ\" is a valid virtual path"),
        );
        let source = Source::new(main_id, main.to_string());

        Ok(Self {
            library: LazyHash::new(<Library as typst::LibraryExt>::default()),
            book: LazyHash::new(book),
            fonts,
            source,
            sources,
            binaries,
            load_warnings,
        })
    }

    pub(crate) fn load_warnings(&self) -> &[Diagnostic] {
        &self.load_warnings
    }

    /// Test-only: boxing collapses codegen's own error, so `open` runs
    /// [`new`](Self::new) and
    /// [`inject_helper_package`](Self::inject_helper_package) itself to keep a
    /// bad date's diagnostic code.
    #[cfg(test)]
    pub fn new_with_data(
        source: &Quill,
        main: &str,
        data: &serde_json::Value,
        meta: &crate::SchemaMeta,
    ) -> Result<(Self, Vec<crate::overlay::FieldWindow>), Box<dyn std::error::Error + Send + Sync>>
    {
        let mut world = Self::new(source, main)?;

        let (windows, _declined) = world.inject_helper_package(data, meta)?;

        Ok((world, windows))
    }

    /// A [`FileId`] for `rel` inside the virtual `@local/quillmark-helper`
    /// package (e.g. `lib.typ`).
    pub(crate) fn helper_fid(rel: &str) -> FileId {
        let spec = PackageSpec {
            namespace: helper::HELPER_NAMESPACE.into(),
            name: helper::HELPER_NAME.into(),
            version: helper::HELPER_VERSION
                .parse()
                .expect("Invalid helper version"),
        };
        file_id(
            Some(spec),
            VirtualPath::new(rel).expect("valid helper vpath"),
        )
    }

    /// Replace-or-insert a source. An existing source is edited via
    /// [`Source::replace`]: a prefix/suffix diff reparses only the changed
    /// span, preserving spans on the untouched regions so `comemo` constraints
    /// keep matching across an edit. A new source is inserted whole.
    pub(crate) fn set_source(&mut self, id: FileId, text: &str) {
        match self.sources.get_mut(&id) {
            Some(s) => {
                s.replace(text);
            }
            None => {
                self.sources.insert(id, Source::new(id, text.to_string()));
            }
        }
    }

    /// Inject the quillmark-helper package generated from the transformed
    /// document data plus the schema meta (see `helper::generate_lib_typ`).
    /// `set_source` on the helper `lib.typ` makes a repeat injection (a session
    /// edit) an incremental reparse rather than a fresh parse. The helper's
    /// `typst.toml` is constant and set once at construction. Returns each
    /// generated content block's byte window, paired with the helper file's id
    /// (the span scan's classification table), beside this injection's
    /// [`DeclinedImages`].
    pub(crate) fn inject_helper_package(
        &mut self,
        data: &serde_json::Value,
        meta: &crate::SchemaMeta,
    ) -> Result<(Vec<crate::overlay::FieldWindow>, DeclinedImages), crate::emit::EmitError> {
        let file = Self::helper_fid("lib.typ");
        let (src, windows) = helper::generate_lib_typ(data, meta)?;
        self.set_source(file, &src);
        let declined = windows
            .iter()
            .filter(|w| w.declined_images > 0)
            .map(|w| (w.path.clone(), w.declined_images))
            .collect();
        let windows = windows
            .into_iter()
            .map(|w| crate::overlay::FieldWindow {
                path: w.path,
                file,
                range: w.block,
                segments: w.segments,
            })
            .collect();
        Ok((windows, declined))
    }

    fn load_fonts_from_quill(
        source: &Quill,
    ) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let mut font_data = Vec::new();

        // Asset fonts first: `QuillWorld` gives them priority over package
        // fonts of the same family, and `Vec` order is that priority.
        for glob in ["assets/fonts/*", "packages/**"] {
            for font_path in source.files().find_files(glob) {
                let Some(ext) = font_path.extension() else {
                    continue;
                };
                if !matches!(ext.to_string_lossy().to_lowercase().as_str(), "ttf" | "otf") {
                    continue;
                }
                if let Some(contents) = source.files().get_file(&font_path) {
                    font_data.push(contents.to_vec());
                }
            }
        }

        Ok(font_data)
    }

    /// Project root only: an asset is the plate's to reach, and nothing
    /// generated names one.
    fn load_assets_from_quill(
        source: &Quill,
        binaries: &mut HashMap<FileId, Bytes>,
        warnings: &mut Vec<Diagnostic>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let asset_paths = source.files().find_files("assets/*");

        for asset_path in asset_paths {
            if let Some(contents) = source.files().get_file(&asset_path) {
                let virtual_path = match VirtualPath::new(asset_path.to_string_lossy().as_ref()) {
                    Ok(vpath) => vpath,
                    Err(e) => {
                        warnings.push(skipped_path(&asset_path, e));
                        continue;
                    }
                };
                let id = file_id(None, virtual_path);
                binaries.insert(id, Bytes::new(contents.to_vec()));
            }
        }

        Ok(())
    }

    fn load_packages_from_quill(
        source: &Quill,
        sources: &mut HashMap<FileId, Source>,
        binaries: &mut HashMap<FileId, Bytes>,
        warnings: &mut Vec<Diagnostic>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let package_dirs = source.files().list_directories("packages");

        for package_dir in package_dirs {
            let package_name = package_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let toml_path = package_dir.join("typst.toml");
            if let Some(toml_contents) = source.files().get_file(&toml_path) {
                let toml_content = String::from_utf8_lossy(toml_contents);
                match parse_package_toml(&toml_content) {
                    Ok(package_info) => {
                        let Ok(version) = package_info.version.parse::<PackageVersion>() else {
                            warnings.push(
                                Diagnostic::new(
                                    Severity::Warning,
                                    format!(
                                        "Skipping package '{package_name}': its typst.toml \
                                         declares version '{}', which is not `major.minor.patch`",
                                        package_info.version
                                    ),
                                )
                                .with_code("typst::package_manifest".to_string()),
                            );
                            continue;
                        };
                        let spec = PackageSpec {
                            namespace: package_info.namespace.clone().into(),
                            name: package_info.name.clone().into(),
                            version,
                        };

                        Self::load_package_files_from_quill(
                            source,
                            &package_dir,
                            sources,
                            binaries,
                            spec,
                            &package_info.entrypoint,
                            warnings,
                        )?;
                    }
                    Err(e) => {
                        // The package is skipped entirely, so the plate's
                        // `#import` for it fails later with an unresolved-file
                        // error naming the plate, not this manifest.
                        warnings.push(
                            Diagnostic::new(
                                Severity::Warning,
                                format!(
                                    "Skipping package '{package_name}': its typst.toml did not \
                                     parse ({e})"
                                ),
                            )
                            .with_code("typst::package_manifest".to_string()),
                        );
                    }
                }
            } else {
                warnings.push(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "Skipping package '{package_name}': it has no typst.toml. Add one \
                             declaring `[package]` with a `name`, `version` and `entrypoint`; \
                             `namespace` defaults to `local`."
                        ),
                    )
                    .with_code("typst::package_manifest".to_string()),
                );
            }
        }

        Ok(())
    }

    fn load_package_files_from_quill(
        source: &Quill,
        package_dir: &Path,
        sources: &mut HashMap<FileId, Source>,
        binaries: &mut HashMap<FileId, Bytes>,
        package_spec: PackageSpec,
        entrypoint: &str,
        warnings: &mut Vec<Diagnostic>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let package_pattern = format!("{}/*", package_dir.to_string_lossy());
        let package_files = source.files().find_files(&package_pattern);

        for file_path in package_files {
            if let Some(contents) = source.files().get_file(&file_path) {
                let relative_path = file_path.strip_prefix(package_dir).map_err(|_| {
                    format!("Failed to get relative path for {}", file_path.display())
                })?;

                let virtual_path = match VirtualPath::new(relative_path.to_string_lossy().as_ref())
                {
                    Ok(vpath) => vpath,
                    Err(e) => {
                        warnings.push(skipped_path(&file_path, e));
                        continue;
                    }
                };
                let id = file_id(Some(package_spec.clone()), virtual_path);

                if let Some(ext) = file_path.extension() {
                    if ext == "typ" {
                        let source_content = String::from_utf8_lossy(contents);
                        let source = Source::new(id, source_content.to_string());
                        sources.insert(id, source);
                    } else {
                        binaries.insert(id, Bytes::new(contents.to_vec()));
                    }
                } else {
                    binaries.insert(id, Bytes::new(contents.to_vec()));
                }
            }
        }

        let entrypoint_path = match VirtualPath::new(entrypoint) {
            Ok(vpath) => vpath,
            Err(e) => {
                warnings.push(skipped_path(Path::new(entrypoint), e));
                return Ok(());
            }
        };
        let entrypoint_file_id = file_id(Some(package_spec.clone()), entrypoint_path);

        if !sources.contains_key(&entrypoint_file_id) {
            warnings.push(
                Diagnostic::new(
                    Severity::Warning,
                    format!(
                        "Package '{}' declares entrypoint '{entrypoint}', which it does not ship",
                        package_spec.name
                    ),
                )
                .with_code("typst::package_entrypoint_missing".to_string()),
            );
        }

        Ok(())
    }
}

impl World for QuillWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else if let Some(source) = self.sources.get(&id) {
            Ok(source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().get_without_slash().into()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.binaries.get(&id) {
            Ok(bytes.clone())
        } else {
            Err(FileError::NotFound(id.vpath().get_without_slash().into()))
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use time::{Duration as TimeDuration, OffsetDateTime};

            let now = OffsetDateTime::now_utc();
            let adjusted = if let Some(offset) = offset {
                now + TimeDuration::seconds(offset.seconds() as i64)
            } else {
                now
            };

            let date = adjusted.date();
            Datetime::from_ymd(date.year(), date.month() as u8, date.day())
        }

        #[cfg(target_arch = "wasm32")]
        {
            // js-sys returns components in UTC.
            use js_sys::Date;
            use wasm_bindgen::JsValue;

            let d = Date::new_0();
            let year = d.get_utc_full_year() as i32;
            // `get_utc_month` is 0-based.
            let month = (d.get_utc_month() as u8).saturating_add(1);
            let day = d.get_utc_date() as u8;

            if let Some(offset) = offset {
                let millis = d.get_time() + offset.seconds() * 1_000.0;
                let d2 = Date::new(&JsValue::from_f64(millis));
                let year = d2.get_utc_full_year() as i32;
                let month = (d2.get_utc_month() as u8).saturating_add(1);
                let day = d2.get_utc_date() as u8;
                return Datetime::from_ymd(year, month, day);
            }

            Datetime::from_ymd(year, month, day)
        }
    }
}

#[derive(Debug, Clone)]
struct PackageInfo {
    namespace: String,
    name: String,
    version: String,
    entrypoint: String,
}

fn parse_package_toml(
    content: &str,
) -> Result<PackageInfo, Box<dyn std::error::Error + Send + Sync>> {
    let value: toml::Value = toml::from_str(content)?;

    let package_section = value
        .get("package")
        .ok_or("Missing [package] section in typst.toml")?;

    // Typst parses the manifest again on `#import` and refuses one missing any
    // of these, so a package loaded without them is unimportable.
    let required = |key: &str| {
        package_section
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("`{key}` is required under [package]"))
    };

    Ok(PackageInfo {
        namespace: package_section
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("local")
            .to_string(),
        name: required("name")?,
        version: required("version")?,
        entrypoint: required("entrypoint")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_defaults_only_its_namespace() {
        let full = "[package]\nname = \"p\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n";
        assert_eq!(parse_package_toml(full).unwrap().namespace, "local");

        for missing in ["name", "version", "entrypoint"] {
            let partial: String = full
                .lines()
                .filter(|line| !line.starts_with(missing))
                .map(|line| format!("{line}\n"))
                .collect();
            assert!(
                parse_package_toml(&partial).is_err(),
                "a manifest without `{missing}` loads"
            );
        }
    }

    #[test]
    fn a_vendored_package_imports_its_dependency_under_the_declared_namespace() {
        let quill = quill_with(&[
            (
                "packages/outer/typst.toml",
                "[package]\nnamespace = \"preview\"\nname = \"outer\"\nversion = \"1.2.0\"\n\
                 entrypoint = \"lib.typ\"\n",
            ),
            (
                "packages/outer/lib.typ",
                "#import \"@preview/inner:0.1.0\": word\n#let greet = [hi #word]\n",
            ),
            (
                "packages/inner/typst.toml",
                "[package]\nnamespace = \"preview\"\nname = \"inner\"\nversion = \"0.1.0\"\n\
                 entrypoint = \"lib.typ\"\n",
            ),
            ("packages/inner/lib.typ", "#let word = \"there\"\n"),
        ]);
        let world =
            QuillWorld::new(&quill, "#import \"@preview/outer:1.2.0\": greet\n#greet\n")
                .expect("world");
        crate::compile::compile_document(&world).expect("the vendored chain resolves");
    }

    /// `extra` files are inserted under their `/`-joined tree paths.
    fn quill_with(extra: &[(&str, &str)]) -> quillmark_core::quill::Quill {
        quill_with_typst_section("  plate_file: plate.typ\n", extra)
    }

    /// `typst` is the indented body of the `typst:` section.
    fn quill_with_typst_section(typst: &str, extra: &[(&str, &str)]) -> quillmark_core::quill::Quill {
        use quillmark_core::quill::{FileTreeNode, Quill};
        let mut root = FileTreeNode::Directory {
            files: HashMap::new(),
        };
        root.insert(
            "Quill.yaml",
            FileTreeNode::File {
                contents: format!(
                    "quill:\n  name: warn\n  version: 0.1.0\n  backend: typst\n  \
                     description: load-warning probe\ntypst:\n{typst}\
                     main:\n  fields:\n    title:\n      type: string\n      \
                     description: title\n"
                )
                .into_bytes(),
            },
        )
        .expect("insert Quill.yaml");
        root.insert(
            "plate.typ",
            FileTreeNode::File {
                contents: b"#set page(width: 100pt, height: 100pt)\n".to_vec(),
            },
        )
        .expect("insert plate");
        for (path, contents) in extra {
            root.insert(
                path,
                FileTreeNode::File {
                    contents: contents.as_bytes().to_vec(),
                },
            )
            .expect("insert extra file");
        }
        Quill::from_tree(root).expect("load quill")
    }

    #[test]
    fn unparseable_package_manifest_warns_instead_of_vanishing() {
        let quill = quill_with(&[
            ("packages/brokenpkg/typst.toml", "this is not [ valid toml"),
            ("packages/brokenpkg/lib.typ", "#let x = 1\n"),
        ]);
        let world = QuillWorld::new(&quill, "// probe").expect("world builds anyway");

        let codes: Vec<&str> = world
            .load_warnings()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(
            codes.contains(&"typst::package_manifest"),
            "expected a package-manifest warning, got {codes:?}"
        );
        let warning = &world.load_warnings()[0];
        assert_eq!(warning.severity, quillmark_core::error::Severity::Warning);
        assert!(
            warning.message.contains("brokenpkg"),
            "warning must name the package: {}",
            warning.message
        );
    }

    #[test]
    fn a_package_without_a_manifest_is_skipped_with_a_warning() {
        let quill = quill_with(&[("packages/bare/lib.typ", "#let x = 1\n")]);
        let world = QuillWorld::new(&quill, "// probe").expect("world builds anyway");

        let warning = world
            .load_warnings()
            .iter()
            .find(|d| d.code.as_deref() == Some("typst::package_manifest"))
            .expect("a manifest-less package warns");
        assert!(
            warning.message.contains("bare") && warning.message.contains("typst.toml"),
            "the warning names the package and the file it wants: {}",
            warning.message
        );
    }

    #[test]
    fn a_well_formed_quill_loads_without_warnings() {
        let quill = quill_with(&[
            (
                "packages/goodpkg/typst.toml",
                "[package]\nname = \"goodpkg\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
            ),
            ("packages/goodpkg/lib.typ", "#let x = 1\n"),
        ]);
        let world = QuillWorld::new(&quill, "// probe").expect("world");
        assert!(
            world.load_warnings().is_empty(),
            "clean quill warned: {:?}",
            world.load_warnings()
        );
    }

    /// A manifest that parses but declares a non-semver version, and one whose
    /// entrypoint is not a usable Typst path: both degrade like every other
    /// unusable quill file rather than failing the session.
    #[test]
    fn unusable_package_version_or_entrypoint_warns_instead_of_failing_the_world() {
        let quill = quill_with(&[
            (
                "packages/oldver/typst.toml",
                "[package]\nname = \"oldver\"\nversion = \"1.0\"\nentrypoint = \"lib.typ\"\n",
            ),
            ("packages/oldver/lib.typ", "#let x = 1\n"),
        ]);
        let world = QuillWorld::new(&quill, "// probe").expect("world builds anyway");
        let codes: Vec<&str> = world
            .load_warnings()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(
            codes.contains(&"typst::package_manifest"),
            "expected a package-manifest warning, got {codes:?}"
        );

        let quill = quill_with(&[
            (
                "packages/escapee/typst.toml",
                "[package]\nname = \"escapee\"\nversion = \"0.1.0\"\nentrypoint = \"../lib.typ\"\n",
            ),
            ("packages/escapee/lib.typ", "#let x = 1\n"),
        ]);
        let world = QuillWorld::new(&quill, "// probe").expect("world builds anyway");
        let codes: Vec<&str> = world
            .load_warnings()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(
            codes.contains(&"typst::path_skipped"),
            "expected a skipped-path warning, got {codes:?}"
        );
    }

    #[test]
    fn missing_package_entrypoint_warns() {
        let quill = quill_with(&[(
            "packages/hollow/typst.toml",
            "[package]\nname = \"hollow\"\nversion = \"0.1.0\"\nentrypoint = \"lib.typ\"\n",
        )]);
        let world = QuillWorld::new(&quill, "// probe").expect("world");
        let codes: Vec<&str> = world
            .load_warnings()
            .iter()
            .filter_map(|d| d.code.as_deref())
            .collect();
        assert!(
            codes.contains(&"typst::package_entrypoint_missing"),
            "expected an entrypoint warning, got {codes:?}"
        );
    }

    #[test]
    fn a_typst_key_the_backend_does_not_read_warns() {
        let quill = quill_with_typst_section(
            "  plate_file: plate.typ\n  packages:\n    - \"@preview/bubble:0.2.2\"\n",
            &[],
        );
        let world = QuillWorld::new(&quill, "// probe").expect("world");
        let flagged: Vec<&str> = world
            .load_warnings()
            .iter()
            .filter(|d| d.code.as_deref() == Some("typst::unknown_config_key"))
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(flagged.len(), 1, "only `packages` is unread: {flagged:?}");
        assert!(flagged[0].contains("typst.packages"), "{}", flagged[0]);
    }

}
