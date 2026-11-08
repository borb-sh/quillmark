//! Tagging stamped widgets into a tagged base's structure tree: one `Form`
//! element per widget, linked by an `OBJR`, appended under the tree's top
//! element, with each widget's `/StructParent` registered in `/ParentTree`.

use crate::error::PdfError;
use crate::reader::{
    ObjectIndex, UpdatedObject, array_elements, parse_indirect_ref, set_dict_value,
};
use crate::writer::{alloc_id, append_refs_to_array_key, dict_object};

const CODE_TAG: &str = "pdf::struct_tree";

pub(crate) struct Tagged {
    /// The `/StructParent` key each widget carries, by position.
    pub struct_parents: Vec<i64>,
}

fn err(msg: impl Into<String>) -> PdfError {
    PdfError::new(CODE_TAG, msg)
}

/// `widgets` is `(widget id, page id, alternate text)` per widget, in stamp
/// order. `None` when the base carries no `/StructTreeRoot`: an untagged base
/// stays untagged.
pub(crate) fn tag_widgets(
    idx: &ObjectIndex,
    catalog: &[u8],
    widgets: &[(u32, u32, Option<&str>)],
    next_id: &mut u32,
    objects: &mut Vec<UpdatedObject>,
) -> Result<Option<Tagged>, PdfError> {
    let Some(root_value) = idx.value(catalog, "StructTreeRoot") else {
        return Ok(None);
    };
    let (root_id, _) = parse_indirect_ref(root_value)
        .ok_or_else(|| err("/StructTreeRoot is not an indirect reference"))?;
    idx.assert_overwrite_gen_zero(root_id, "/StructTreeRoot")?;
    let root = idx.dict(root_id, CODE_TAG, "/StructTreeRoot")?;

    let top_value = idx
        .value(root, "K")
        .ok_or_else(|| err("/StructTreeRoot has no /K"))?;
    let top_ref = match idx.resolve(top_value) {
        Some(v) if v.trim_ascii().starts_with(b"[") => {
            let mut kids = array_elements(v);
            let first = kids
                .next()
                .ok_or_else(|| err("/StructTreeRoot /K is empty"))?;
            if kids.next().is_some() {
                return Err(err("/StructTreeRoot /K holds more than one element"));
            }
            first
        }
        _ => top_value,
    };
    let (top_id, _) = parse_indirect_ref(top_ref)
        .ok_or_else(|| err("the top structure element is not an indirect reference"))?;
    idx.assert_overwrite_gen_zero(top_id, "top structure element")?;
    let top = idx.dict(top_id, CODE_TAG, "top structure element")?;

    let (nums, inline_tree, tree_id) = match idx.value(root, "ParentTree") {
        None => (Vec::new(), true, None),
        Some(v) => {
            let (dict, inline, id) = match parse_indirect_ref(v) {
                Some((id, _)) => (idx.dict(id, CODE_TAG, "/ParentTree")?, false, Some(id)),
                None => {
                    let t = v.trim_ascii();
                    let inner = t
                        .strip_prefix(b"<<")
                        .and_then(|t| t.strip_suffix(b">>"))
                        .ok_or_else(|| err("/ParentTree is neither a dict nor a reference"))?;
                    (inner, true, None)
                }
            };
            if idx.value(dict, "Kids").is_some() {
                return Err(err("/ParentTree with /Kids is not supported"));
            }
            let nums = idx
                .value(dict, "Nums")
                .and_then(|n| idx.resolve(n))
                .map(|n| array_elements(n).map(<[u8]>::to_vec).collect())
                .unwrap_or_default();
            (nums, inline, id)
        }
    };
    let max_key = nums
        .iter()
        .step_by(2)
        .filter_map(|k| {
            std::str::from_utf8(k.trim_ascii())
                .ok()?
                .parse::<i64>()
                .ok()
        })
        .max();
    let declared_next = idx.value(root, "ParentTreeNextKey").and_then(|v| {
        std::str::from_utf8(v.trim_ascii())
            .ok()?
            .parse::<i64>()
            .ok()
    });
    let first_key = declared_next.unwrap_or(0).max(max_key.map_or(0, |k| k + 1));

    let mut form_ids = Vec::with_capacity(widgets.len());
    let mut struct_parents = Vec::with_capacity(widgets.len());
    let mut new_nums: Vec<u8> = nums.join(&b' ');
    for (at, &(wid, page_id, alt)) in widgets.iter().enumerate() {
        let form_id = alloc_id(next_id)?;
        let key = first_key + at as i64;
        let mut elem = format!(
            "/Type /StructElem /S /Form /P {top_id} 0 R /Pg {page_id} 0 R \
             /K << /Type /OBJR /Obj {wid} 0 R /Pg {page_id} 0 R >>"
        )
        .into_bytes();
        if let Some(alt) = alt {
            elem.extend_from_slice(b" /Alt ");
            elem.extend_from_slice(&crate::writer::pdf_text_string(alt));
        }
        objects.push(dict_object(form_id, &elem));
        if !new_nums.is_empty() {
            new_nums.push(b' ');
        }
        new_nums.extend_from_slice(format!("{key} {form_id} 0 R").as_bytes());
        form_ids.push(form_id);
        struct_parents.push(key);
    }

    objects.push(dict_object(
        top_id,
        &append_refs_to_array_key(idx, top, "K", &form_ids, CODE_TAG, |_| {
            err("the top structure element's /K is not an inline array")
        })?,
    ));

    let tree = format!("<< /Nums [{}] >>", String::from_utf8_lossy(&new_nums));
    let mut new_root = root.to_vec();
    match (inline_tree, tree_id) {
        (false, Some(id)) => objects.push(dict_object(id, &tree.as_bytes()[2..tree.len() - 2])),
        _ => new_root = set_dict_value(&new_root, "ParentTree", tree.as_bytes()),
    }
    let next_key = first_key + widgets.len() as i64;
    new_root = set_dict_value(
        &new_root,
        "ParentTreeNextKey",
        next_key.to_string().as_bytes(),
    );
    objects.push(dict_object(root_id, &new_root));

    Ok(Some(Tagged { struct_parents }))
}
