//! Typst backend for Quillmark: markdown + card-YAML data → PDF, SVG, PNG.
//!
//! Richtext fields cross the seam as canonical content JSON and are lowered to
//! Typst markup by `emit` at codegen time; no code re-parses markdown at
//! render time. Plates read fields through the `@local/quillmark-helper`
//! virtual package. Form-field widgets become AcroForm widgets on PDF output
//! only; SVG and PNG render an invisible placeholder.

mod compile;
/// Content → Typst-markup lowering plus its per-segment source map.
mod emit;
mod error_mapping;
mod families;
mod glyphs;
mod library;
mod tex;

mod helper;
mod overlay;
mod world;
pub mod workspace;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use library::FontLibrary;
use quillmark_core::{
    backend::{Backend, FontFace},
    error::{Diagnostic, RenderError, RenderResult, Severity},
    quill::{
        build_transform_schema, CalendarDate, Quill, QuillConfig,
        QUILLMARK_ROSTER_KEY,
    },
    region::{ContentHit, RenderedRegion},
    session::{ChangeSet, LiveSession, SessionHandle},
    types::{OutputFormat, RenderOptions},
};
use typst::syntax::VirtualPath;

/// Typst backend implementation for Quillmark, holding the host's font
/// library every session it opens reads.
#[derive(Debug, Default)]
pub struct TypstBackend {
    library: Arc<FontLibrary>,
}

impl TypstBackend {
    pub fn new(library: Arc<FontLibrary>) -> Self {
        Self { library }
    }
}

const SUPPORTED_FORMATS: &[OutputFormat] =
    &[OutputFormat::Pdf, OutputFormat::Svg, OutputFormat::Png];

/// Persisting the world keeps fonts, packages, and assets parsed once per
/// session rather than once per compile: the substrate for incremental
/// recompiles.
struct TypstSession {
    world: world::QuillWorld,
    /// Built once at `open`: the schema never changes for a session's lifetime,
    /// and codegen plus date validation read only these tables.
    schema_meta: SchemaMeta,
    /// `quill.description`, the PDF title when neither the render nor the
    /// plate names one.
    fallback_title: String,
    /// Swapped whole, and only once [`recompile`] has succeeded, so on `Err`
    /// every read keeps serving the last-good compile.
    live: Compiled,
}

/// One compile plus everything derived from it. Derived here rather than per
/// query: a point hit or a region scan is a read of these tables.
struct Compiled {
    document: typst_layout::PagedDocument,
    /// This compile's form fields in the spine's coordinates: what a PDF render
    /// stamps.
    field_specs: Vec<quillmark_pdf::FieldSpec>,
    widget_regions: Vec<RenderedRegion>,
    /// The span scan's classification table: every generated ink window in the
    /// helper, in byte order.
    windows: Vec<overlay::FieldWindow>,
    /// Span resolution goes through this snapshot, not the world: a failed
    /// `update` leaves the *next* injection's text in the world while every read
    /// keeps serving this compile.
    helper_source: typst::syntax::Source,
    /// Diffed against the next compile's to produce `ChangeSet::dirty_pages`.
    page_hashes: Vec<u128>,
    /// What [`session_warnings`] built for this compile. The compile half swaps
    /// on each committed `update`; the load half rides along unchanged.
    warnings: Vec<Diagnostic>,
    /// [`overlay::unclosed_claims`] for this document, suppressed by every
    /// region and point query. Computed with the compile rather than per query:
    /// `field_at` cannot derive it from the page prefix it walks.
    unclosed_claims: Vec<usize>,
}

/// Inject the data, compile, and derive every table a query reads. Nothing a
/// session serves is touched on the way, so a caller commits the result in one
/// move or keeps what it had.
fn recompile(
    world: &mut world::QuillWorld,
    data: &serde_json::Value,
    config: &QuillConfig,
    schema_meta: &SchemaMeta,
) -> Result<Compiled, RenderError> {
    let windows = world
        .inject_helper_package(data, schema_meta)
        .map_err(|e| RenderError::coded(e.code(), e.to_string()))?;

    let (document, compile_warnings) = compile::compile_document(world)?;
    let helper_source = helper_source(world)?;
    let placements = overlay::extract(&document)?;
    let field_specs = overlay::build_field_specs(&document, &placements)?;
    let widget_regions = quillmark_pdf::regions_of(&field_specs);

    let unclosed = overlay::unclosed_claims(&document);
    let hashes = page_hashes(&document);
    let warnings = session_warnings(
        world,
        compile_warnings,
        &unclosed,
        tex::unsupported(config, data),
        families::unknown(config, data, world.families()),
        glyphs::missing(&document),
    );
    Ok(Compiled {
        document,
        field_specs,
        widget_regions,
        windows,
        helper_source,
        page_hashes: hashes,
        warnings,
        unclosed_claims: unclosed.into_iter().map(|(claim, _)| claim).collect(),
    })
}

/// One order, built in one place, so an `update` that swaps only what it
/// recompiled keeps the quill's load warnings ahead of its own.
fn session_warnings(
    world: &world::QuillWorld,
    compile: Vec<Diagnostic>,
    unclosed: &[(usize, String)],
    unsupported_tex: Vec<Diagnostic>,
    unknown_families: Vec<Diagnostic>,
    missing_glyphs: Option<Diagnostic>,
) -> Vec<Diagnostic> {
    let mut all = world.load_warnings().to_vec();
    all.extend(compile);
    all.extend(unclosed.iter().map(|(_, field)| {
        Diagnostic::new(
            Severity::Warning,
            format!("`region(\"{field}\", ..)` never closed; its claim was dropped"),
        )
        .with_code("typst::unclosed_region".to_string())
        .with_hint(
            "both markers must reach the frame together, so emit the call's \
             return value whole rather than in parts"
                .to_string(),
        )
    }));
    all.extend(unsupported_tex);
    all.extend(unknown_families);
    all.extend(missing_glyphs);
    all
}

/// Per-page fingerprints of *visible* content, diffed across compiles for
/// `ChangeSet::dirty_pages`. Introspection `Tag` items and group parent
/// locations are skipped: they carry element hashes spanning content on *other*
/// pages, so hashing them dirties page 0 on an end-of-document edit.
///
/// Pixels, not spans: every hashed item drops its source-location `Span`. Only
/// render-affecting data may enter the hash, or a helper-layout shift that moves
/// no ink (a reordered `data` literal regenerating `lib.typ`) reports every
/// content page dirty.
pub(crate) fn page_hashes(document: &typst_layout::PagedDocument) -> Vec<u128> {
    use std::hash::{Hash, Hasher};
    use typst::layout::FrameItem;

    fn hash_text<H: Hasher>(text: &typst::text::TextItem, state: &mut H) {
        text.font.hash(state);
        text.size.hash(state);
        text.fill.hash(state);
        text.stroke.hash(state);
        text.lang.hash(state);
        text.region.hash(state);
        text.text.hash(state);
        for g in &text.glyphs {
            g.id.hash(state);
            g.x_advance.hash(state);
            g.x_offset.hash(state);
            g.y_advance.hash(state);
            g.y_offset.hash(state);
            g.range.hash(state);
            // g.span deliberately omitted: source location, not pixels.
        }
    }

    fn walk<H: Hasher>(frame: &typst::layout::Frame, state: &mut H) {
        frame.size().hash(state);
        for (pos, item) in frame.items() {
            match item {
                FrameItem::Tag(_) => {}
                FrameItem::Group(g) => {
                    pos.hash(state);
                    g.transform.hash(state);
                    g.clip.hash(state);
                    walk(&g.frame, state);
                }
                FrameItem::Text(text) => {
                    pos.hash(state);
                    hash_text(text, state);
                }
                // Shape/Image carry a trailing `Span` their derived `Hash` would
                // fold in; destructure to hash the visible parts and drop it, same
                // reason as glyph spans above.
                FrameItem::Shape(shape, _span) => {
                    pos.hash(state);
                    shape.hash(state);
                }
                FrameItem::Image(image, size, _span) => {
                    pos.hash(state);
                    image.hash(state);
                    size.hash(state);
                }
                FrameItem::Link(dest, size) => {
                    pos.hash(state);
                    dest.hash(state);
                    size.hash(state);
                }
            }
        }
    }

    struct VisiblePage<'a>(&'a typst_layout::Page);
    impl Hash for VisiblePage<'_> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.0.fill.hash(state);
            self.0.numbering.hash(state);
            self.0.number.hash(state);
            walk(&self.0.frame, state);
        }
    }

    document
        .pages()
        .iter()
        .map(|page| typst::utils::hash128(&VisiblePage(page)))
        .collect()
}

impl SessionHandle for TypstSession {
    fn render(&self, opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let format = opts.output_format.unwrap_or(OutputFormat::Pdf);

        compile::render_document_pages(
            &self.live.document,
            opts.pages.as_deref(),
            format,
            opts.ppi_or_default(),
            &self.live.field_specs,
            opts.title.as_deref(),
            &self.fallback_title,
        )
    }

    fn page_count(&self) -> usize {
        self.live.document.pages().len()
    }

    fn update(
        &mut self,
        config: &QuillConfig,
        json_data: &serde_json::Value,
    ) -> Result<ChangeSet, RenderError> {
        let compiled = recompile(&mut self.world, json_data, config, &self.schema_meta)?;

        let page_count = compiled.page_hashes.len();
        let dirty_pages = (0..page_count)
            .filter(|&i| self.live.page_hashes.get(i) != Some(&compiled.page_hashes[i]))
            .collect();

        self.live = compiled;
        Ok(ChangeSet::new(page_count, dirty_pages))
    }

    fn warnings(&self) -> &[Diagnostic] {
        &self.live.warnings
    }

    /// Typst points: 1 pt = 1/72 inch.
    fn page_size_pt(&self, page: usize) -> Option<(f32, f32)> {
        let frame = &self.live.document.pages().get(page)?.frame;
        let size = frame.size();
        Some((size.x.to_pt() as f32, size.y.to_pt() as f32))
    }

    fn render_rgba(
        &self,
        page: usize,
        scale: f32,
    ) -> Result<Option<(u32, u32, Vec<u8>)>, RenderError> {
        let Some(p) = self.live.document.pages().get(page) else {
            return Ok(None);
        };
        let size = p.frame.size();
        quillmark_core::backend::check_raster(scale, size.x.to_pt() as f32, size.y.to_pt() as f32)?;
        let pixmap = typst_render::render(p, &compile::render_options(scale));
        let width = pixmap.width();
        let height = pixmap.height();
        let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for px in pixmap.pixels() {
            let c = px.demultiply();
            rgba.push(c.red());
            rgba.push(c.green());
            rgba.push(c.blue());
            rgba.push(c.alpha());
        }
        Ok(Some((width, height, rgba)))
    }

    fn regions(&self) -> Vec<quillmark_core::region::RenderedRegion> {
        let mut regions = self.live.widget_regions.clone();
        regions.extend(self.scan().regions());
        regions
    }

    /// The nearest of the widget and content lanes, a widget taking a tie: a
    /// widget is a deliberate click target drawing no spanned ink of its own, so
    /// ink beneath one must not swallow a click that lands on it. Among
    /// overlapping widgets the later-painted wins, matching `Scan::field_at`.
    fn field_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<String> {
        let widget = self.widget_at(page, x, y, tol);
        // Nothing outranks a widget the point is inside of, and the content lane
        // walks every frame up to `page` to answer.
        if widget.as_ref().is_some_and(|&(gap, _)| gap == 0.0) {
            return widget.map(|(_, field)| field);
        }
        // Widget first: `min_by` keeps the first of equal gaps.
        [widget, self.scan().field_at(page, x, y, tol)]
            .into_iter()
            .flatten()
            .min_by(|(a, _), (b, _)| a.total_cmp(b))
            .map(|(_, field)| field)
    }

    /// The fine-grained twin of [`field_at`](Self::field_at). Widgets draw no
    /// spanned content ink, so unlike `field_at` they are not consulted.
    fn position_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<ContentHit> {
        self.scan().position_at(page, x, y, tol)
    }

    fn locate(&self, field: &str, pos: usize) -> Option<RenderedRegion> {
        self.scan().locate(field, pos)
    }
}

impl TypstSession {
    /// The nearest widget within `tol` and its gap, later-painted on a tie.
    fn widget_at(&self, page: usize, x: f32, y: f32, tol: f32) -> Option<(f32, String)> {
        quillmark_core::region::nearest_region(&self.live.widget_regions, page, x, y, tol)
            .map(|(d, r)| (d, r.field.clone()))
    }

    /// The live compile's tables, as the one context every content query takes.
    fn scan(&self) -> overlay::Scan<'_> {
        overlay::Scan {
            doc: &self.live.document,
            helper: &self.live.helper_source,
            windows: &self.live.windows,
            unclosed: &self.live.unclosed_claims,
        }
    }
}

fn quill_world(source: &Quill, plate: &Plate, library: &FontLibrary) -> Result<world::QuillWorld, RenderError> {
    world::QuillWorld::new(source, plate, library).map_err(|e| {
        RenderError::coded(
            "typst::world_creation",
            format!("Failed to create Typst compilation environment: {e}"),
        )
    })
}

/// One `typst::helper_import` error per import of the helper a project source
/// spells any way but `#import "@local/quillmark-helper:<version>" as <name>`.
/// The module form is what keeps the helper's names out of a plate's own scope:
/// the one name it binds is the one the plate chose.
pub(crate) fn check_helper_imports(world: &world::QuillWorld) -> Result<(), RenderError> {
    use typst::syntax::ast;
    use typst::syntax::{DiagSpan, SyntaxNode};

    fn walk(node: &SyntaxNode, world: &world::QuillWorld, out: &mut Vec<Diagnostic>) {
        if let Some(import) = node.cast::<ast::ModuleImport>() {
            let ast::Expr::Str(spec) = import.source() else {
                return;
            };
            let spec = spec.get();
            let helper = format!("@{}/{}:", helper::HELPER_NAMESPACE, helper::HELPER_NAME);
            let current = format!("{helper}{}", helper::HELPER_VERSION);
            let module_form = import.imports().is_none() && import.new_name().is_some();
            if spec.starts_with(&helper) && (spec != current || !module_form) {
                let mut diag = Diagnostic::new(
                    Severity::Error,
                    format!("the helper is imported as a module: `#import \"{current}\" as qm`"),
                )
                .with_code("typst::helper_import".to_string())
                .with_hint(
                    "read every helper member through the module name: `qm.data`, \
                     `qm.ink(..)`, `qm.region(..)`"
                        .to_string(),
                );
                diag.location =
                    error_mapping::resolve_span_to_location(DiagSpan::from(node.span()), world);
                out.push(diag);
            }
            return;
        }
        for child in node.children() {
            walk(child, world, out);
        }
    }

    let mut diags = Vec::new();
    for source in world.project_sources() {
        walk(source.root(), world, &mut diags);
    }
    if diags.is_empty() {
        Ok(())
    } else {
        Err(RenderError::new(diags))
    }
}

/// Snapshotted right after a successful compile: the text the served document's
/// spans resolve against.
fn helper_source(world: &world::QuillWorld) -> Result<typst::syntax::Source, RenderError> {
    use typst::World as _;
    world
        .source(world::QuillWorld::helper_fid("lib.typ"))
        .map_err(|e| {
            RenderError::coded(
                "typst::helper_source",
                format!("helper lib.typ unreadable: {e}"),
            )
        })
}

impl Backend for TypstBackend {
    fn id(&self) -> &'static str {
        "typst"
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        SUPPORTED_FORMATS
    }

    fn open(
        &self,
        source: &Quill,
        json_data: &serde_json::Value,
        today: CalendarDate,
    ) -> Result<LiveSession, RenderError> {
        let plate = read_plate(source)?;

        let config = source.config();
        let schema_meta = SchemaMeta::from_config(config);
        // Built in two steps rather than through `new_with_data` so codegen's own
        // diagnostic code survives: boxing it into the world-creation error would
        // relabel a bad date `typst::world_creation`.
        let mut world = quill_world(source, &plate, &self.library)?;
        check_helper_imports(&world)?;
        world.set_today(today);
        // No session survives to serve the load warnings, and a package they
        // name as skipped otherwise fails only as an unresolved import.
        let live = recompile(&mut world, json_data, config, &schema_meta).map_err(|e| {
            let mut diags = e.into_diagnostics();
            diags.extend(world.load_warnings().iter().cloned());
            RenderError::new(diags)
        })?;
        let session = TypstSession {
            world,
            schema_meta,
            fallback_title: config.description.clone(),
            live,
        };
        Ok(LiveSession::new(
            Box::new(session),
            config.clone(),
            today,
            json_data,
        ))
    }

    /// The library's faces that set running text, as `world::sets_text` reads
    /// them. A quill's own faces and the fallback set its plate, never a mark.
    fn font_faces(&self, _source: &Quill) -> Vec<FontFace> {
        self.library
            .faces_with_files()
            .filter(|(font, _)| world::sets_text(font.info(), font))
            .map(|(font, file)| {
                let info = font.info();
                FontFace {
                    family: info.family.clone(),
                    weight: info.variant.weight.to_number(),
                    style: css_style(info.variant.style),
                    index: font.index(),
                    data: file.clone(),
                }
            })
            .collect()
    }
}

/// A face's CSS style keyword.
fn css_style(style: typst::text::FontStyle) -> &'static str {
    match style {
        typst::text::FontStyle::Normal => "normal",
        typst::text::FontStyle::Italic => "italic",
        typst::text::FontStyle::Oblique => "oblique",
    }
}

/// The plate the backend reads when the quill's `typst:` section names none.
const PLATE_FILE: &str = "plate.typ";

pub(crate) struct Plate {
    /// The plate's path from the quill root, less its `.` steps and empty
    /// steps.
    pub(crate) file: String,
    /// Where the world loads the plate.
    pub(crate) path: VirtualPath,
    pub(crate) text: String,
}

/// The plate's path from the quill root as a render reads it, or the refusal a
/// render's `open` raises for it: `typst.plate_file` where the quill declares
/// it, else `plate.typ`.
pub fn plate_file(source: &Quill) -> Result<String, RenderError> {
    Ok(read_plate(source)?.file)
}

/// The release of the linked Typst compiler, which a plate reads as
/// `sys.version`.
pub fn typst_version() -> &'static str {
    typst::utils::version().raw()
}

/// The plate is a Typst-only notion: `plate.typ` at the quill root, found by
/// name, or the file `typst.plate_file` names. Core reads no backend asset at
/// load, so a quill holding neither fails here, at `open`.
///
/// The plate loads at its own path, so one Typst's path grammar refuses fails
/// here: any stand-in name may belong to a file the quill holds.
fn read_plate(source: &Quill) -> Result<Plate, RenderError> {
    let no_path = |shown: String| {
        RenderError::coded_hint(
            "typst::plate_path_invalid",
            format!("`typst.plate_file` is {shown}, which names no path"),
            format!(
                "Write `typst.plate_file` as a path from the quill root, such as \
                 `tpl/main.typ`, or delete it to read `{PLATE_FILE}`."
            ),
        )
    };
    let declared = match source.config().backend_config.get("plate_file") {
        None => None,
        Some(value) => Some(value.as_str().ok_or_else(|| no_path(value.as_json().to_string()))?),
    };
    let named = declared.unwrap_or(PLATE_FILE);
    let steps: Vec<&str> = named
        .split('/')
        .filter(|step| !step.is_empty() && *step != ".")
        .collect();
    if steps.is_empty() {
        return Err(no_path(format!("'{named}'")));
    }
    let plate_file = steps.join("/");

    let misspelled = named.starts_with('/') || steps.contains(&"..");
    let bytes = (!misspelled)
        .then(|| source.files().get_file(&plate_file))
        .flatten()
        .ok_or_else(|| match declared {
            None => RenderError::coded_hint(
                "typst::plate_missing",
                format!("the quill holds no `{PLATE_FILE}` at its root"),
                format!(
                    "Write the plate as `{PLATE_FILE}` beside `Quill.yaml`, or name its path \
                     with `typst.plate_file`."
                ),
            ),
            Some(declared) => {
                let message = format!("plate file '{declared}' not found in the quill's file tree");
                if misspelled {
                    let hint = plate_spelling_hint(declared, source);
                    RenderError::coded_hint("typst::plate_missing", message, hint)
                } else {
                    RenderError::coded("typst::plate_missing", message)
                }
            }
        })?;

    let path = VirtualPath::new(&plate_file).map_err(|e| {
        RenderError::coded_hint(
            "typst::plate_path_invalid",
            format!("plate file '{plate_file}' is not a path Typst can load ({e})"),
            "Separate `typst.plate_file` with `/`, and rename the file if its own name holds a `\\`.",
        )
    })?;

    let text = String::from_utf8(bytes.to_vec()).map_err(|e| {
        RenderError::coded(
            "typst::invalid_utf8",
            format!("plate file '{plate_file}' is not valid UTF-8: {e}"),
        )
    })?;
    Ok(Plate {
        file: plate_file,
        path,
        text,
    })
}

/// The fix for a `plate_file` holding a leading `/` or a `..` step: its
/// spelling from the quill root where the quill holds a file there, else the
/// rule.
fn plate_spelling_hint(plate_file: &str, source: &Quill) -> String {
    let rule = "`typst.plate_file` names a file inside the quill by its path from the quill \
                root, with no leading `/` and no `..`";
    let spelling = VirtualPath::new(plate_file)
        .ok()
        .map(|path| path.get_without_slash().to_string())
        .filter(|path| source.files().get_file(path).is_some());
    match spelling {
        Some(path) => format!("Write `{path}`: {rule}."),
        None => format!("{rule}."),
    }
}

/// The steps a schema address may take out of one node, and nothing else: the
/// schema pruned to its address grammar.
///
/// A node offers a property step (`props`), an index step (`item`), or neither.
/// One `object` shape spans a typed dictionary and a variant container alike
/// (`prose/canon/PLATE_DATA.md`), while a richtext field, an `object` declaring
/// no `properties`, offers no step at all. An array always offers its index
/// step, whatever its element.
///
/// A `matrix` node also carries its `roster`, and on an open matrix an `added`
/// step: any key spelled as a member id that `props` does not name. Like an
/// index, the step is not checked against the data, since the grammar asks
/// about the schema alone.
#[derive(Debug, Default, Clone)]
pub(crate) struct AddressNode {
    props: BTreeMap<String, AddressNode>,
    item: Option<Box<AddressNode>>,
    added: Option<Box<AddressNode>>,
    roster: Option<serde_json::Value>,
}

impl AddressNode {
    pub(crate) fn from_schema(node: &serde_json::Value) -> Self {
        let props = node
            .get("properties")
            .and_then(|v| v.as_object())
            .map(|props| {
                props
                    .iter()
                    .map(|(name, child)| (name.clone(), Self::from_schema(child)))
                    .collect()
            })
            .unwrap_or_default();
        let item = (helper::node_type(node) == Some("array")).then(|| {
            Box::new(
                node.get("items")
                    .map(Self::from_schema)
                    .unwrap_or_default(),
            )
        });
        let added = node
            .get("additionalProperties")
            .filter(|v| v.is_object())
            .map(|child| Box::new(Self::from_schema(child)));
        let roster = node.get(QUILLMARK_ROSTER_KEY).cloned();
        Self {
            props,
            item,
            added,
            roster,
        }
    }

    /// The generated `_qm-meta` shape: absent keys are absent steps, so a leaf
    /// serializes as the empty dict.
    fn to_json(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        if !self.props.is_empty() {
            out.insert(
                "props".to_string(),
                serde_json::Value::Object(
                    self.props
                        .iter()
                        .map(|(name, child)| (name.clone(), child.to_json()))
                        .collect(),
                ),
            );
        }
        if let Some(item) = &self.item {
            out.insert("item".to_string(), item.to_json());
        }
        if let Some(added) = &self.added {
            out.insert("added".to_string(), added.to_json());
        }
        if let Some(roster) = &self.roster {
            out.insert("roster".to_string(), roster.clone());
        }
        serde_json::Value::Object(out)
    }
}

/// The transform schema plus the address tree derived from it, kept apart
/// because they answer different questions. Lowering reads the schema node
/// (`helper::lowering`); the tree answers which *addresses* a plate may
/// write, which is the same walk with everything but the steps pruned away.
pub(crate) struct SchemaMeta {
    /// The walk's cursor source: the same recursive projection
    /// `build_transform_schema` produced, kept whole rather than flattened.
    schema: serde_json::Value,
    /// The address trees [`address_json`](Self::address_json) serializes into
    /// [`meta_literal`](Self::meta_literal).
    root: AddressNode,
    cards: BTreeMap<String, AddressNode>,
    /// Serialized once: the schema is fixed for a session's lifetime, and every
    /// update splices this same literal into the generated `lib.typ`.
    meta_literal: String,
}

impl Default for SchemaMeta {
    fn default() -> Self {
        Self::from_schema_json(&serde_json::Value::Null)
    }
}

impl SchemaMeta {
    /// A schema with no top-level `properties` yields an empty tree and a walk
    /// that lowers every value literally: `build_transform_schema` always emits
    /// `properties`, so that only arises for hand-built schemas in tests.
    pub(crate) fn from_schema_json(schema_json: &serde_json::Value) -> Self {
        let mut cards = BTreeMap::new();
        if let Some(defs) = schema_json.get("$defs").and_then(|v| v.as_object()) {
            for (def_name, def_schema) in defs {
                let Some(kind) = def_name.strip_suffix("_card") else {
                    continue;
                };
                cards.insert(kind.to_string(), AddressNode::from_schema(def_schema));
            }
        }

        let mut meta = Self {
            root: AddressNode::from_schema(schema_json),
            cards,
            schema: schema_json.clone(),
            meta_literal: String::new(),
        };
        meta.meta_literal = helper::lit(&meta.address_json());
        meta
    }

    /// The meta a session over `config` lowers by: its transform schema.
    pub(crate) fn from_config(config: &QuillConfig) -> Self {
        Self::from_schema_json(build_transform_schema(config, quillmark_core::quill::SchemaCodec::Content).as_json())
    }

    /// The address tables the helper's `_qm-node` walks.
    fn address_json(&self) -> serde_json::Value {
        serde_json::json!({
            "fields": self.root.to_json(),
            "cards": serde_json::Value::Object(
                self.cards
                    .iter()
                    .map(|(kind, node)| (kind.clone(), node.to_json()))
                    .collect(),
            ),
        })
    }

    /// [`address_json`](Self::address_json) as the Typst literal codegen
    /// splices into `_qm-meta`.
    pub(crate) fn meta_literal(&self) -> &str {
        &self.meta_literal
    }

    /// The schema node declaring a top-level field.
    pub(crate) fn field_node(&self, name: &str) -> Option<&serde_json::Value> {
        self.schema.get("properties")?.get(name)
    }

    pub(crate) fn card_props(
        &self,
        kind: &str,
    ) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.schema
            .get("$defs")?
            .get(format!("{kind}_card"))?
            .get("properties")?
            .as_object()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillmark_core::value::QuillValue;
    use serde_json::json;
    use std::collections::HashMap;

    /// The shape the seam carries for a richtext field.
    fn content(markdown: &str) -> serde_json::Value {
        let rt = quillmark_content::import::from_markdown(markdown).expect("import").content;
        quillmark_content::serial::to_canonical_value(&rt)
    }

    /// The two quills differ only by an extra unused schema field, which
    /// lengthens the generated `_qm-meta` literal ahead of the content blocks
    /// and so shifts every glyph's span without moving a pixel.
    #[test]
    fn page_hashes_ignore_span_shift_when_ink_is_identical() {
        use quillmark_core::quill::FileTreeNode;

        const PLATE: &str = r#"#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#qm.data.prose
"#;
        let quill_with = |extra_field: bool| {
            let mut yaml = String::from(
                "quill:\n  name: shift\n  version: 0.1.0\n  backend: typst\n  description: span shift probe\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    prose:\n      type: richtext\n      multiline: true\n      description: body\n",
            );
            if extra_field {
                yaml.push_str(
                    "    zz_unused:\n      type: text\n      description: never placed\n",
                );
            }
            let mut files = HashMap::new();
            files.insert(
                "Quill.yaml".to_string(),
                FileTreeNode::File {
                    contents: yaml.into_bytes(),
                },
            );
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File {
                    contents: PLATE.as_bytes().to_vec(),
                },
            );
            Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
        };

        let json =
            serde_json::json!({ "prose": content("A **markdown** body with real ink to lay out.") });
        let hashes_of = |quill: &Quill| {
            let plate_content = read_plate(quill).expect("plate");
            let transform_schema = build_transform_schema(quill.config(), quillmark_core::quill::SchemaCodec::Content);
            let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
            let data = &json;
            let (world, _windows) =
                world::QuillWorld::new_with_data(quill, &plate_content, data, &schema_meta)
                    .expect("world");
            let (document, _warnings) = compile::compile_document(&world).expect("compile");
            page_hashes(&document)
        };

        assert_eq!(
            hashes_of(&quill_with(false)),
            hashes_of(&quill_with(true)),
            "identical ink must fingerprint identically across a whole-file span shift"
        );
    }

    /// Asks Typst's introspector how many blocks the compile really produced,
    /// so a future Typst that changes line-anchoring fails loud here.
    #[test]
    fn line_anchored_paragraph_text_stays_literal() {
        use quillmark_core::quill::FileTreeNode;
        use typst::foundations::{NativeElement, Selector};
        use typst::introspection::Introspector;
        use typst::model::{EnumElem, HeadingElem, ListElem, TermsElem};

        const PLATE: &str = r#"#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 400pt, margin: 20pt)
#set text(size: 11pt)
#qm.data.prose
"#;
        let quill = || {
            let yaml = "quill:\n  name: anchor\n  version: 0.1.0\n  backend: typst\n  description: line-anchor guard\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    prose:\n      type: richtext\n      multiline: true\n      description: body\n";
            let mut files = HashMap::new();
            files.insert(
                "Quill.yaml".to_string(),
                FileTreeNode::File { contents: yaml.as_bytes().to_vec() },
            );
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File { contents: PLATE.as_bytes().to_vec() },
            );
            Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
        };
        let counts = |rt: &quillmark_content::model::Normalized| {
            let json =
                serde_json::json!({ "prose": quillmark_content::serial::to_canonical_value(rt) });
            let q = quill();
            let plate_content = read_plate(&q).expect("plate");
            let transform_schema = build_transform_schema(q.config(), quillmark_core::quill::SchemaCodec::Content);
            let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
            let data = &json;
            let (world, _w) =
                world::QuillWorld::new_with_data(&q, &plate_content, data, &schema_meta)
                    .expect("world");
            let (document, _warn) = compile::compile_document(&world).expect("compile");
            let intro = document.introspector();
            let count = |e| intro.query(&Selector::Elem(e, None)).len();
            [
                count(<HeadingElem as NativeElement>::ELEM),
                count(<ListElem as NativeElement>::ELEM),
                count(<EnumElem as NativeElement>::ELEM),
                count(<TermsElem as NativeElement>::ELEM),
            ]
        };
        // `Para` lines directly: markdown import would parse `- `/`+ `/`N. ` as
        // real lists, which is not the bug.
        //
        // The second half is the same five markers with nothing after them, which
        // Typst reads off the line's end: the bare `/` is a term list missing its
        // colon, so an unescaped one fails the compile rather than the count.
        use quillmark_content::model::{Container, Line, LineKind, Content};
        let para = |_: usize| Line::new(LineKind::Para);
        let rt = Content::new(
            "= Heading\n- bullet\n+ numbered\n1. dotted\n/ term: desc\n=\n-\n+\n1.\n/"
                .to_string(),
            (0..10).map(para).collect(),
        );
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()), "content invariants");
        assert_eq!(counts(&rt), [0, 0, 0, 0], "paragraph text stays literal");

        // A list item's body head is a line start of Typst's own, the marker
        // sitting where indentation would: the same five, one item each, and the
        // one bullet list they make is the only block the compile may produce.
        let item = |ordinal: u64| {
            Line::new(LineKind::Para).with_containers(vec![Container::ListItem {
                ordered: false,
                start: 1,
                ordinal,
                checked: None,
                instance: 0,
            }])
        };
        let rt = Content::new(
            "=\n-\n+\n1.\n/".to_string(),
            (0..5).map(item).collect(),
        );
        let rt = rt.into_normalized();
        assert_eq!(rt.validate(), Ok(()), "content invariants");
        assert_eq!(counts(&rt), [0, 1, 0, 0], "item text stays literal");
    }

    #[test]
    fn inline_field_in_par_emits_no_parbreak_warning() {
        use quillmark_core::quill::FileTreeNode;

        const PLATE: &str = r#"#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: 200pt, margin: 20pt)
#set text(size: 11pt)
#par(qm.data.subject)
"#;
        let quill = |inline: bool| {
            let multiline = !inline;
            let yaml = format!(
                "quill:\n  name: parbreak\n  version: 0.1.0\n  backend: typst\n  description: parbreak probe\ntypst:\n  plate_file: plate.typ\nmain:\n  fields:\n    subject:\n      type: richtext\n      multiline: {multiline}\n      description: subject\n",
            );
            let mut files = HashMap::new();
            files.insert(
                "Quill.yaml".to_string(),
                FileTreeNode::File { contents: yaml.into_bytes() },
            );
            files.insert(
                "plate.typ".to_string(),
                FileTreeNode::File { contents: PLATE.as_bytes().to_vec() },
            );
            Quill::from_tree(FileTreeNode::Directory { files }).expect("quill")
        };
        let warnings_for = |inline: bool| {
            let q = quill(inline);
            let json = serde_json::json!({ "subject": content("A subject line") });
            let plate_content = read_plate(&q).expect("plate");
            let transform_schema = build_transform_schema(q.config(), quillmark_core::quill::SchemaCodec::Content);
            let schema_meta = SchemaMeta::from_schema_json(transform_schema.as_json());
            let data = &json;
            let (world, _w) =
                world::QuillWorld::new_with_data(&q, &plate_content, data, &schema_meta)
                    .expect("world");
            let (_doc, warnings) = compile::compile_document(&world).expect("compile");
            warnings
        };
        let has_parbreak =
            |ws: &[Diagnostic]| ws.iter().any(|d| d.message.contains("parbreak"));

        // Negative control: the block lowering warns, so the probe has teeth.
        assert!(
            has_parbreak(&warnings_for(false)),
            "block richtext in par() should emit the parbreak warning"
        );
        assert!(
            !has_parbreak(&warnings_for(true)),
            "inline richtext in par() must emit no parbreak warning"
        );
    }

    #[test]
    fn schema_meta_serves_the_declaring_node_at_every_position() {
        let schema = QuillValue::from_json(json!({
            "type": "object",
            "properties": {
                "issued": { "type": "string", "format": "date" },
                "contact": { "type": "object", "properties": {
                    "reply_by": { "type": "string", "format": "date" }
                }}
            },
            "$defs": {
                "indorsement_card": {
                    "type": "object",
                    "properties": { "signed_on": { "type": "string", "format": "date" } }
                }
            }
        }));
        let meta = SchemaMeta::from_schema_json(schema.as_json());

        let format_of = |node: Option<&serde_json::Value>| {
            node.and_then(|n| n.get("format"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        assert_eq!(format_of(meta.field_node("issued")).as_deref(), Some("date"));
        // A nested declaration is reachable, which is what the lowering walk
        // reads and what a table of top-level names cannot express.
        let contact = meta.field_node("contact").expect("contact declared");
        assert_eq!(
            format_of(contact.get("properties").and_then(|p| p.get("reply_by"))).as_deref(),
            Some("date"),
        );
        assert_eq!(
            format_of(meta.card_props("indorsement").and_then(|p| p.get("signed_on"))).as_deref(),
            Some("date"),
        );
        assert!(meta.field_node("nonexistent").is_none());
        assert!(meta.card_props("nonexistent").is_none());
    }
}
