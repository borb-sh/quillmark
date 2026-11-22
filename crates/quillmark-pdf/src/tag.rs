//! Tagging stamped widgets into a tagged base's structure tree: one `Form`
//! element per widget, linked by an `OBJR`, appended under the tree's top
//! element, with each widget's `/StructParent` registered in `/ParentTree`.

use crate::error::PdfError;
use crate::reader::{
    ObjectIndex, UpdatedObject, array_elements, parse_indirect_ref, set_dict_value,
};
use crate::writer::{alloc_id, dict_object};

const CODE_TAG: &str = "pdf::struct_tree";

/// What [`tag_widgets`] did with a base's structure tree.
pub(crate) enum Tagging {
    /// The base carries no `/StructTreeRoot`, and stays untagged.
    BaseUntagged,
    /// Each widget's `/StructParent` key, by position.
    Tagged { struct_parents: Vec<i64> },
    /// The base is tagged in a shape this writer leaves as written: a
    /// `/ParentTree` of `/Kids`, or a top element whose `/K` array is an
    /// object of its own. The widgets stay out of the tree.
    Left,
}

fn err(msg: impl Into<String>) -> PdfError {
    PdfError::new(CODE_TAG, msg)
}

/// `widgets` is `(widget id, page id, alternate text)` per widget, in stamp
/// order.
pub(crate) fn tag_widgets(
    idx: &ObjectIndex,
    catalog: &[u8],
    widgets: &[(u32, u32, Option<&str>)],
    next_id: &mut u32,
    objects: &mut Vec<UpdatedObject>,
) -> Result<Tagging, PdfError> {
    let Some(root_value) = idx.value(catalog, "StructTreeRoot") else {
        return Ok(Tagging::BaseUntagged);
    };
    let (root_id, _) = parse_indirect_ref(root_value)
        .ok_or_else(|| err("/StructTreeRoot is not an indirect reference"))?;
    idx.assert_overwrite_gen_zero(root_id, "/StructTreeRoot")?;
    let root = idx.dict(root_id, CODE_TAG, "/StructTreeRoot")?;

    // Forms join the root's one element, or the root itself when it holds
    // none or several.
    let top_id = match idx.value(root, "K") {
        None => None,
        Some(v) => match idx.resolve(v) {
            Some(r) if r.trim_ascii().starts_with(b"[") => {
                let mut kids = array_elements(r.trim_ascii());
                match (kids.next(), kids.next()) {
                    (Some(only), None) => Some(only),
                    _ => None,
                }
            }
            _ => Some(v),
        },
    }
    .map(|top_ref| {
        let (top_id, _) = parse_indirect_ref(top_ref)
            .ok_or_else(|| err("the top structure element is not an indirect reference"))?;
        idx.assert_overwrite_gen_zero(top_id, "top structure element")?;
        Ok::<_, PdfError>(top_id)
    })
    .transpose()?;
    let (parent_id, parent) = match top_id {
        Some(id) => (id, idx.dict(id, CODE_TAG, "top structure element")?),
        None => (root_id, root),
    };
    if !takes_kids(idx, parent) {
        return Ok(Tagging::Left);
    }

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
                return Ok(Tagging::Left);
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
            "/Type /StructElem /S /Form /P {parent_id} 0 R /Pg {page_id} 0 R \
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

    let parent = with_kids(idx, parent, &form_ids);
    let mut new_root = if top_id.is_some() {
        objects.push(dict_object(parent_id, &parent));
        root.to_vec()
    } else {
        parent
    };

    let tree = format!("<< /Nums [{}] >>", String::from_utf8_lossy(&new_nums));
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

    Ok(Tagging::Tagged { struct_parents })
}

/// Whether `/K` can take more kids in place: absent, an inline array, or one
/// kid. An array held in an object of its own is left as written.
fn takes_kids(idx: &ObjectIndex, elem: &[u8]) -> bool {
    match idx.value(elem, "K") {
        None => true,
        Some(v) if v.trim_ascii().starts_with(b"[") => true,
        Some(v) => !idx
            .resolve(v)
            .is_some_and(|r| r.trim_ascii().starts_with(b"[")),
    }
}

/// `elem` with `refs` appended to its `/K`, a lone kid becoming the array's
/// first.
fn with_kids(idx: &ObjectIndex, elem: &[u8], refs: &[u32]) -> Vec<u8> {
    let mut kids = vec![b'['];
    if let Some(existing) = idx.value(elem, "K") {
        let t = existing.trim_ascii();
        let inner = match t.strip_prefix(b"[") {
            Some(rest) => {
                rest[..rest.iter().rposition(|&b| b == b']').unwrap_or(rest.len())].trim_ascii()
            }
            None => t,
        };
        if !inner.is_empty() {
            kids.extend_from_slice(inner);
            // A `%` comment runs to the end of its line, so only a newline
            // keeps the refs out of one.
            kids.push(if inner.contains(&b'%') { b'\n' } else { b' ' });
        }
    }
    let refs = refs
        .iter()
        .map(|r| format!("{r} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    kids.extend_from_slice(refs.as_bytes());
    kids.push(b']');
    set_dict_value(elem, "K", &kids)
}
