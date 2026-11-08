//! Write a fresh `/AcroForm` (and an `/Info` `/Producer` stamp) onto a base PDF
//! via one incremental-update append.
//!
//! The fields are real and carry `/NeedAppearances`, so a viewer that
//! synthesizes appearances owns how a value looks and refits it as the user
//! types. Each widget also carries the baked `/AP` [`appearance`] draws, so a
//! consumer that synthesizes nothing shows the value rather than an empty box.
//!
//! The background owns the visual chrome, so a widget is a transparent input over
//! it: no borders, no fills, black text.

use std::collections::HashMap;

use pdf_writer::types::{
    AnnotationFlags, AnnotationType, FieldFlags, FieldType as PwFieldType, Quadding, SigFlags,
};
use pdf_writer::writers::{Annotation, Field, Form};
use pdf_writer::{Chunk, Finish, Name, Rect, Ref, Str, TextStr};

use quillmark_core::region::RenderedRegion;

use crate::appearance::{self, Appearance, Normal};
use crate::error::PdfError;
use crate::reader::{
    ObjectIndex, UpdatedObject, array_elements, as_dict, is_name, parse_indirect_ref,
    set_dict_value,
};
use crate::update::PdfUpdate;
use crate::writer::{alloc_id, append_refs_to_array_key, dict_object, to_ref, type1_font_object};
use crate::{FieldSpec, FieldType, FormFont, TextAlign};

const CODE_PARSE: &str = "pdf::stamp_parse";
const CODE_EXISTING_ACROFORM: &str = "pdf::existing_acroform";
const CODE_CONFLICTING_FIELD: &str = "pdf::conflicting_field";
const CODE_BAD_ON_STATE: &str = "pdf::bad_on_state";

/// The fixed checkbox on-state export name. A checkbox [`FieldSpec`] carries
/// this as its `value` when checked, `None` when not.
pub const CHECKBOX_ON_STATE: &str = "Yes";

/// The off-state name every checkbox and radio button shares (ISO 32000-1
/// §12.7.4.2.3).
const OFF_STATE: &[u8] = b"Off";

/// The standard-14 face a check mark is set in, never embedded.
pub const CHECK_FONT: &[u8] = b"ZapfDingbats";

/// The `/DR` `/Font` key [`CHECK_FONT`] is registered under: the name a
/// checkbox `/DA` selects it by, and a drawn content stream too.
pub const CHECK_FONT_RESOURCE: &str = "ZaDb";

/// The [`CHECK_FONT`] glyph a checked box shows, 0x34, the filled check mark:
/// the widget's `/MK /CA` caption, and what a drawn raster shows in its place.
pub(crate) const CHECK_GLYPH: &[u8] = b"4";

/// House-style default appearance: Helvetica, `0 Tf` (auto-size), black fill.
/// `Helv` is registered in the form `/DR` `/Font`.
///
/// The form-level `/DA` fallback only; a widget carries its own, built by
/// [`field_da`].
const DEFAULT_APPEARANCE: &[u8] = b"/Helv 0 Tf 0 g";

/// A `/DA` selecting `resource` at `size` over the house black fill. `f32`'s
/// `Display` drops the trailing `.0`, so a whole-point size writes `12` and
/// `0.0` writes the `0 Tf` that defers to the viewer's auto-size.
fn da(resource: &str, size: f32) -> Vec<u8> {
    format!("/{resource} {size} Tf 0 g").into_bytes()
}

/// A variable-text widget's `/DA`: its own face and size. An absent size — or
/// one not positive and finite, `font_size` being public and `NaN`/`inf` being
/// tokens no PDF number grammar admits — defers to the viewer's auto-size.
fn field_da(spec: &FieldSpec) -> Vec<u8> {
    let size = spec
        .font_size
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(0.0);
    da(spec.font.resource_name(), size)
}

/// The face a widget's value is set in, as `(resource name, base font)`, or
/// `None` for a signature — the one type carrying no value. A checkbox's is
/// Quillmark's check font, so `font` names nothing on either.
fn value_font(spec: &FieldSpec) -> Option<(&'static str, &'static [u8])> {
    match spec.field_type {
        FieldType::Text { .. } | FieldType::Choice { .. } => {
            Some((spec.font.resource_name(), spec.font.base_font()))
        }
        FieldType::Checkbox | FieldType::Radio { .. } => Some((CHECK_FONT_RESOURCE, CHECK_FONT)),
        FieldType::Signature => None,
    }
}

/// Every face this stamp registers in `/DR` `/Font`: each widget's own, plus the
/// Helvetica of the form-level [`DEFAULT_APPEARANCE`]. A `/DA` and the `/AP`
/// stream drawing the same value name their face from here, so an appearance
/// always finds it.
fn fonts_used(fields: &[FieldSpec]) -> Vec<(&'static str, &'static [u8])> {
    let mut fonts: Vec<(&'static str, &'static [u8])> =
        fields.iter().filter_map(value_font).collect();
    fonts.push((
        FormFont::Helvetica.resource_name(),
        FormFont::Helvetica.base_font(),
    ));
    fonts.sort_unstable_by_key(|&(resource, _)| resource);
    fonts.dedup();
    fonts
}

/// The predefined encoding a registered face is written with: a text face takes
/// WinAnsi, the byte encoding [`appearance`] transcodes to, and the symbol check
/// font keeps its own built-in one.
fn font_encoding(resource: &str) -> Option<&'static [u8]> {
    (resource != CHECK_FONT_RESOURCE).then_some(b"WinAnsiEncoding")
}

/// The `/DR` `/Font` object `spec`'s value is set in, from the faces
/// [`fonts_used`] collected over the same `spec`s.
fn registered_font(
    fonts: &[(&'static str, &'static [u8])],
    font_ids: &[u32],
    spec: &FieldSpec,
) -> Option<u32> {
    let (resource, _) = value_font(spec)?;
    let at = fonts.iter().position(|&(name, _)| name == resource)?;
    font_ids.get(at).copied()
}

/// Options for [`stamp`](crate::stamp()).
#[derive(Debug, Clone)]
pub struct StampOptions {
    /// The `/Info` `/Producer` this stamp writes over whatever the base carries.
    pub producer: String,
    /// What a checkbox's `/AP` `/N` holds.
    pub states: AppearanceStates,
}

impl Default for StampOptions {
    fn default() -> Self {
        Self {
            producer: format!("Quillmark {}", env!("CARGO_PKG_VERSION")),
            states: AppearanceStates::default(),
        }
    }
}

/// What a box's `/AP` `/N` holds, a checkbox's or a radio button's. Its `/AS`
/// names the current state either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppearanceStates {
    /// A dictionary of both states, the box's on-state and `Off`, so a
    /// filler toggles the box by renaming `/AS` (ISO 32000-1 §12.7.4.2.3).
    #[default]
    All,
    /// The one stream `/AS` selects, for a rasterizer that reads `/N` only as a
    /// stream (hayro 0.7). A filler has no other state to switch to, so this
    /// is a preview's document, never a deliverable.
    Selected,
}

/// What a stamped widget's `/AP` `/N` names: one stream, or a box's two
/// states.
enum NormalRef {
    Stream(u32),
    States { on: u32, off: u32 },
}

/// Stamp `fields` onto `base` as a fresh AcroForm via one incremental update,
/// and `/Info` `/Producer` with it. Geometry is not produced here; it is a
/// session-level query (see [`regions_of`]).
///
/// Specs sharing a name stamp as one field with a widget for each
/// ([`FieldSpec`]); ones that disagree on what the field holds are refused as
/// `pdf::conflicting_field`, and a radio button whose on-state is empty or
/// `Off` as `pdf::bad_on_state`.
///
/// `base` must satisfy the reader's input contract (traditional-xref,
/// unencrypted, inline-annots, bounded-tree) and carry no form of its own: no
/// catalog `/AcroForm`, and no widget annotation in any page's `/Annots`. Each
/// `rect` must already be final, finite, bottom-left PDF-point geometry.
pub fn stamp(
    base: Vec<u8>,
    fields: &[FieldSpec],
    opts: &StampOptions,
) -> Result<Vec<u8>, PdfError> {
    for spec in fields {
        spec.assert_finite_rect()?;
    }
    let groups = group_by_name(fields);
    for group in &groups {
        check_group(fields, group)?;
    }

    let pdf = base;
    let idx = ObjectIndex::new(&pdf);
    let mut up = PdfUpdate::begin(&idx, &opts.producer)?;

    if !fields.is_empty() {
        // Before any allocation: a second `/AcroForm` on the catalog is a dict
        // the spec does not define, and the base's own widgets stay live in the
        // page `/Annots` this update preserves, whatever its catalog names.
        let catalog = idx.dict(up.catalog_id, CODE_PARSE, "catalog")?;
        if idx.value(catalog, "AcroForm").is_some() {
            return Err(PdfError::new(
                CODE_EXISTING_ACROFORM,
                "base PDF already carries an /AcroForm; strip its form before stamping",
            ));
        }

        let pages = up.resolve_pages(&idx, fields)?;
        for (at, page) in pages.iter().enumerate() {
            let what = format!("page node {}", page.id);
            if holds_a_widget(&idx, idx.dict(page.id, CODE_PARSE, &what)?)? {
                return Err(PdfError::new(
                    CODE_EXISTING_ACROFORM,
                    format!(
                        "page {} of the base PDF holds a widget annotation in its /Annots; \
                         strip the base's widgets from every page's /Annots before stamping",
                        at + 1
                    ),
                ));
            }
        }
        let page_count = pages.len();

        let fonts = fonts_used(fields);
        let font_ids: Vec<u32> = fonts
            .iter()
            .map(|_| alloc_id(&mut up.next_id))
            .collect::<Result<_, _>>()?;
        let widget_ids: Vec<u32> = fields
            .iter()
            .map(|_| alloc_id(&mut up.next_id))
            .collect::<Result<_, _>>()?;
        // A field of one widget is that widget, merged; a field of several is
        // a dictionary of its own whose kids they are.
        let mut parent_of: Vec<Option<u32>> = vec![None; fields.len()];
        let mut field_ids: Vec<u32> = Vec::with_capacity(groups.len());
        for group in &groups {
            match group.as_slice() {
                [only] => field_ids.push(widget_ids[*only]),
                kids => {
                    let id = alloc_id(&mut up.next_id)?;
                    for &kid in kids {
                        parent_of[kid] = Some(id);
                    }
                    field_ids.push(id);
                }
            }
        }
        let acroform_id = alloc_id(&mut up.next_id)?;

        // Grouped by page for the page-side `/Annots`.
        let mut widgets_by_page: Vec<Vec<u32>> = vec![Vec::new(); page_count];
        for ((spec, &wid), &parent) in fields.iter().zip(&widget_ids).zip(&parent_of) {
            widgets_by_page[spec.page].push(wid);
            let page_ref = to_ref(pages[spec.page].id)?;
            let normal = match appearance::of(spec).zip(registered_font(&fonts, &font_ids, spec)) {
                None => None,
                Some((Normal::Value(ap), font)) => {
                    Some(NormalRef::Stream(bake(&mut up, &ap, font)?))
                }
                Some((Normal::States { on, off }, font)) => Some(match opts.states {
                    AppearanceStates::All => NormalRef::States {
                        on: bake(&mut up, &on, font)?,
                        off: bake(&mut up, &off, font)?,
                    },
                    AppearanceStates::Selected => {
                        let selected = if spec.is_checked() { &on } else { &off };
                        NormalRef::Stream(bake(&mut up, selected, font)?)
                    }
                }),
            };
            let bytes = match parent {
                None => write_merged_widget(spec, to_ref(wid)?, page_ref, normal)?,
                Some(parent) => {
                    write_kid_widget(spec, to_ref(wid)?, to_ref(parent)?, page_ref, normal)?
                }
            };
            up.objects.push(UpdatedObject { id: wid, bytes });
        }

        for (group, &fid) in groups.iter().zip(&field_ids) {
            if group.len() < 2 {
                continue;
            }
            let kids: Vec<&FieldSpec> = group.iter().map(|&i| &fields[i]).collect();
            let kid_refs: Vec<Ref> = group
                .iter()
                .map(|&i| to_ref(widget_ids[i]))
                .collect::<Result<_, _>>()?;
            up.objects.push(UpdatedObject {
                id: fid,
                bytes: write_parent_field(&kids, to_ref(fid)?, &kid_refs)?,
            });
        }

        for (&(resource, base_font), &fid) in fonts.iter().zip(&font_ids) {
            up.objects
                .push(type1_font_object(fid, base_font, font_encoding(resource))?);
        }

        let has_signature = fields
            .iter()
            .any(|f| matches!(f.field_type, FieldType::Signature));
        let field_refs: Vec<Ref> = field_ids
            .iter()
            .map(|&id| to_ref(id))
            .collect::<Result<_, _>>()?;
        let mut achunk = Chunk::new();
        {
            let mut form: Form<'_> = achunk.indirect(to_ref(acroform_id)?).start::<Form>();
            form.fields(field_refs);
            if has_signature {
                form.sig_flags(SigFlags::SIGNATURES_EXIST);
            }
            form.pair(Name(b"NeedAppearances"), true);
            form.default_appearance(Str(DEFAULT_APPEARANCE));
            // /DR << /Font << /Helv <font> .. >> >>
            {
                let mut dr = form.insert(Name(b"DR")).dict();
                let mut font_dict = dr.insert(Name(b"Font")).dict();
                for (&(resource, _), &fid) in fonts.iter().zip(&font_ids) {
                    font_dict.pair(Name(resource.as_bytes()), to_ref(fid)?);
                }
            }
            form.finish();
        }
        up.objects.push(UpdatedObject {
            id: acroform_id,
            bytes: achunk.as_bytes().to_vec(),
        });

        // A widget is fillable only if reachable both ways: the catalog's
        // `/AcroForm /Fields` (added here) and the page's `/Annots` (below).
        let cat_dict = idx.dict(up.catalog_id, CODE_PARSE, "catalog")?;
        let cat_inner = set_dict_value(
            cat_dict,
            "AcroForm",
            format!("{acroform_id} 0 R").as_bytes(),
        );
        up.objects.push(dict_object(up.catalog_id, &cat_inner));

        for (page_idx, widget_refs) in widgets_by_page.iter().enumerate() {
            if widget_refs.is_empty() {
                continue;
            }
            let page_obj_id = pages[page_idx].id;
            let what = format!("page node {page_obj_id}");
            let pg_dict = idx.dict(page_obj_id, CODE_PARSE, &what)?;
            up.objects.push(dict_object(
                page_obj_id,
                &rewrite_page_with_annots(&idx, pg_dict, widget_refs)?,
            ));
        }
    }

    Ok(up.finish(pdf))
}

/// Whether the page dict `page` holds a `/Subtype /Widget` annotation in its
/// `/Annots`: the array, each element and its `/Subtype` read through any
/// reference, as a viewer reads them. An annotation dictionary the reader
/// cannot read is `Err`, since it may be a widget.
fn holds_a_widget<'a>(idx: &ObjectIndex<'a>, page: &'a [u8]) -> Result<bool, PdfError> {
    let Some(annots) = idx
        .value(page, "Annots")
        .and_then(|annots| idx.resolve(annots))
    else {
        return Ok(false);
    };
    for annot in array_elements(annots).filter_map(|annot| idx.resolve(annot)) {
        let Some(annot) = as_dict(annot, CODE_PARSE, "annotation")? else {
            continue;
        };
        if idx
            .value(annot, "Subtype")
            .and_then(|subtype| idx.resolve(subtype))
            .is_some_and(|subtype| is_name(subtype, b"/Widget"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Write `ap` as a fresh object selecting the registered face `font_id`, and
/// return its id.
fn bake(up: &mut PdfUpdate, ap: &Appearance, font_id: u32) -> Result<u32, PdfError> {
    let id = alloc_id(&mut up.next_id)?;
    up.objects.push(ap.object(id, font_id)?);
    Ok(id)
}

/// One [`RenderedRegion`] per field carrying a schema address, keyed on that
/// address. A widget with no schema field is a backend-only artifact and emits
/// nothing. Shared by `stamp` and the no-stamp render paths, so region geometry
/// always matches the widget.
pub fn regions_of(fields: &[FieldSpec]) -> Vec<RenderedRegion> {
    fields
        .iter()
        .filter_map(|f| Some(RenderedRegion::new(f.schema_field.clone()?, f.page, f.rect)))
        .collect()
}

/// Left is the PDF's own `/Q` default, so it is left unwritten: a field that
/// never touches this dial stamps exactly as it did before the dial existed.
fn write_quadding(field: &mut Field<'_>, align: TextAlign) {
    let q = match align {
        TextAlign::Left => return,
        TextAlign::Center => Quadding::Center,
        TextAlign::Right => Quadding::Right,
    };
    field.vartext_quadding(q);
}

/// `fields`' indices grouped by name, each group in spec order and the groups
/// in the order of their first spec.
fn group_by_name(fields: &[FieldSpec]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (i, spec) in fields.iter().enumerate() {
        match at.get(spec.name.as_str()) {
            Some(&g) => groups[g].push(i),
            None => {
                at.insert(&spec.name, groups.len());
                groups.push(vec![i]);
            }
        }
    }
    groups
}

/// `Err` unless the widgets of one field agree on what the field holds, and
/// each radio button's on-state is a name a filler can tell from `Off`.
fn check_group(fields: &[FieldSpec], group: &[usize]) -> Result<(), PdfError> {
    let first = &fields[group[0]];
    for spec in group.iter().map(|&i| &fields[i]) {
        if let FieldType::Radio { on } = &spec.field_type
            && (on.is_empty() || on.as_bytes() == OFF_STATE)
        {
            return Err(PdfError::new(
                CODE_BAD_ON_STATE,
                format!(
                    "radio button `{}` names its on-state {on:?}; it must be neither empty nor \
                     `Off`, the name of every button's off-state",
                    spec.name
                ),
            ));
        }
        let same_kind = match (&first.field_type, &spec.field_type) {
            (FieldType::Radio { .. }, FieldType::Radio { .. }) => true,
            (a, b) => a == b,
        };
        let agrees = same_kind
            && spec.value == first.value
            && spec.font == first.font
            && spec.font_size.map(f32::to_bits) == first.font_size.map(f32::to_bits)
            && spec.align == first.align
            && spec.max_len == first.max_len;
        if !agrees {
            return Err(PdfError::new(
                CODE_CONFLICTING_FIELD,
                format!(
                    "the widgets named `{}` are one field but disagree on its type, value, \
                     face, size, justification or character cap",
                    spec.name
                ),
            ));
        }
    }
    Ok(())
}

/// Serialize a field of one widget as a merged field+widget indirect object,
/// `normal` naming its `/AP` `/N` Form XObjects when the widget draws any.
fn write_merged_widget(
    spec: &FieldSpec,
    wid: Ref,
    page_ref: Ref,
    normal: Option<NormalRef>,
) -> Result<Vec<u8>, PdfError> {
    let mut chunk = Chunk::new();
    {
        let mut field = chunk.form_field(wid);
        write_field_entries(&mut field, &[spec]);
        if let Some(tt) = spec.tooltip.as_deref() {
            field.alternate_name(TextStr(tt));
        }
        // `into_annotation` writes `/Type /Annot` + `/Subtype /Widget` exactly
        // once; do not call `.subtype()` again or it duplicates the key.
        let mut ann = field.into_annotation();
        write_widget_entries(&mut ann, spec, page_ref, normal)?;
        ann.finish();
    }
    Ok(chunk.as_bytes().to_vec())
}

/// Serialize one widget of a field of several as a widget annotation that is
/// a kid of `parent`. Its tooltip rides the widget, where a radio button's
/// names its own value.
fn write_kid_widget(
    spec: &FieldSpec,
    wid: Ref,
    parent: Ref,
    page_ref: Ref,
    normal: Option<NormalRef>,
) -> Result<Vec<u8>, PdfError> {
    let mut chunk = Chunk::new();
    {
        let mut ann = chunk.annotation(wid);
        ann.subtype(AnnotationType::Widget).parent(parent);
        if let Some(tt) = spec.tooltip.as_deref() {
            ann.pair(Name(b"TU"), TextStr(tt));
        }
        write_widget_entries(&mut ann, spec, page_ref, normal)?;
        ann.finish();
    }
    Ok(chunk.as_bytes().to_vec())
}

/// Serialize the field dictionary holding `kids`, which [`check_group`] has
/// held to one type and value.
fn write_parent_field(kids: &[&FieldSpec], id: Ref, kid_refs: &[Ref]) -> Result<Vec<u8>, PdfError> {
    let mut chunk = Chunk::new();
    {
        let mut field = chunk.form_field(id);
        write_field_entries(&mut field, kids);
        field.children(kid_refs.iter().copied());
        field.finish();
    }
    Ok(chunk.as_bytes().to_vec())
}

/// The entries of the field `widgets` are the widgets of, read off the first:
/// the others agree on them ([`check_group`]). A radio field holds a value
/// only when one of its buttons is named for it, and lets a filler clear it
/// only when it is a lone button.
fn write_field_entries(field: &mut Field<'_>, widgets: &[&FieldSpec]) {
    let spec = widgets[0];
    field.partial_name(TextStr(&spec.name));
    let text_da = field_da(spec);
    match &spec.field_type {
        FieldType::Text { multiline } => {
            field.field_type(PwFieldType::Text);
            field.vartext_default_appearance(Str(&text_da));
            write_quadding(field, spec.align);
            if *multiline {
                field.field_flags(FieldFlags::MULTILINE);
            }
            if let Some(max) = spec.max_len {
                field.text_max_len(i32::try_from(max).unwrap_or(i32::MAX));
            }
            if let Some(v) = spec.value.as_deref() {
                field.text_value(TextStr(v));
            }
        }
        FieldType::Checkbox => {
            field.field_type(PwFieldType::Button);
            // The viewer synthesizes the `/MK /CA` caption in the `/DA`
            // face: under the form-level Helvetica the glyph is a digit.
            field.vartext_default_appearance(Str(&da(CHECK_FONT_RESOURCE, 0.0)));
            field.pair(Name(b"V"), box_state(spec));
        }
        FieldType::Radio { .. } => {
            field.field_type(PwFieldType::Button);
            field.vartext_default_appearance(Str(&da(CHECK_FONT_RESOURCE, 0.0)));
            let mut flags = FieldFlags::RADIO;
            if widgets.len() > 1 {
                flags |= FieldFlags::NO_TOGGLE_TO_OFF;
            }
            let ons: Vec<&str> = widgets.iter().filter_map(|w| w.on_state()).collect();
            if (1..ons.len()).any(|i| ons[..i].contains(&ons[i])) {
                flags |= FieldFlags::RADIOS_IN_UNISON;
            }
            field.field_flags(flags);
            let held = widgets.iter().find(|w| w.is_checked());
            field.pair(Name(b"V"), held.map_or(Name(OFF_STATE), |w| box_state(w)));
        }
        FieldType::Choice { options } => {
            field.field_type(PwFieldType::Choice);
            field.field_flags(FieldFlags::COMBO);
            field.vartext_default_appearance(Str(&text_da));
            write_quadding(field, spec.align);
            {
                let mut opts = field.choice_options();
                for o in options {
                    opts.option(TextStr(o));
                }
            }
            if let Some(v) = spec.value.as_deref() {
                field.choice_value(Some(TextStr(v)));
            }
        }
        FieldType::Signature => {
            field.field_type(PwFieldType::Signature);
        }
    }
}

/// The entries of `spec`'s widget annotation: where it sits, and for a box,
/// the state it shows and the caption a viewer redraws it with.
fn write_widget_entries(
    ann: &mut Annotation<'_>,
    spec: &FieldSpec,
    page_ref: Ref,
    normal: Option<NormalRef>,
) -> Result<(), PdfError> {
    ann.rect(Rect::new(
        spec.rect[0],
        spec.rect[1],
        spec.rect[2],
        spec.rect[3],
    ))
    .page(page_ref)
    .flags(AnnotationFlags::PRINT);
    if spec.on_state().is_some() {
        ann.appearance_state(box_state(spec));
        ann.insert(Name(b"MK"))
            .dict()
            .pair(Name(b"CA"), Str(CHECK_GLYPH));
    }
    match normal {
        None => {}
        Some(NormalRef::Stream(id)) => ann.appearance().normal().stream(to_ref(id)?),
        Some(NormalRef::States { on, off }) => {
            let on_state = spec.on_state().unwrap_or(CHECKBOX_ON_STATE);
            ann.appearance()
                .normal()
                .streams()
                .pair(Name(on_state.as_bytes()), to_ref(on)?)
                .pair(Name(OFF_STATE), to_ref(off)?);
        }
    }
    Ok(())
}

/// The state name a box's `/AS` carries, and a field's `/V` when it is that
/// box's: its on-state when checked, else `Off`.
fn box_state(spec: &FieldSpec) -> Name<'_> {
    match spec.on_state() {
        Some(on) if spec.is_checked() => Name(on.as_bytes()),
        _ => Name(OFF_STATE),
    }
}

/// Three cases for the existing `/Annots`: absent or `null`, directly or by
/// reference (write a fresh array); inline array (splice widget refs before
/// `]`); any other indirect reference (hard error, the input contract requires
/// inline annots).
fn rewrite_page_with_annots(
    idx: &ObjectIndex,
    pg_dict: &[u8],
    widget_refs: &[u32],
) -> Result<Vec<u8>, PdfError> {
    append_refs_to_array_key(
        idx,
        pg_dict,
        "Annots",
        widget_refs,
        CODE_PARSE,
        non_array_annots,
    )
}

fn non_array_annots(existing: &[u8]) -> PdfError {
    if parse_indirect_ref(existing).is_some() {
        PdfError::new(
            "pdf::indirect_annots",
            "/Annots is an indirect reference; only inline arrays are supported",
        )
    } else {
        PdfError::new(CODE_PARSE, "/Annots is neither array nor indirect ref")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_widget_reads_through_a_referenced_subtype() {
        let idx = ObjectIndex::new(b"%PDF\n7 0 obj /Widget endobj\n8 0 obj /Link endobj\n");
        for (page, widget) in [
            (&b"/Annots [<< /Subtype /Widget >>]"[..], true),
            (b"/Annots [<< /Subtype 7 0 R >>]", true),
            (b"/Annots [<< /Subtype 8 0 R >>]", false),
            (b"/Annots [<< /Subtype /Wid#67et >>]", true),
        ] {
            assert_eq!(
                holds_a_widget(&idx, page).unwrap(),
                widget,
                "{}",
                String::from_utf8_lossy(page)
            );
        }
    }

    #[test]
    fn an_annotation_the_reader_cannot_read_is_refused() {
        let idx = ObjectIndex::new(b"%PDF\n7 0 obj << /Subtype /Widget /Subtype /Link >> endobj\n");
        for page in [
            &b"/Annots [<< /Subtype /Link /Subtype /Widget >>]"[..],
            b"/Annots [<< /Subtype /Link (stray) >>]",
            b"/Annots [7 0 R]",
        ] {
            let e = holds_a_widget(&idx, page).expect_err(&String::from_utf8_lossy(page));
            assert_eq!(e.code, CODE_PARSE);
        }
    }
}
