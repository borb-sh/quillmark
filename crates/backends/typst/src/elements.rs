//! The elements a compile drew with no renderer, read off the
//! `<__qm_element__>` markers the helper's `_qm-element` stamps where it falls
//! through to the body.

use std::collections::BTreeSet;

use quillmark_core::{
    error::{Diagnostic, Severity},
    path::DocPath,
    quill::{element_runs, QuillConfig},
    Content,
};
use typst::foundations::{Label, Selector, Value};
use typst::introspection::Introspector;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

/// The label and metadata `kind` of the fallthrough's marker.
const MARKER: &str = "__qm_element__";

const CODE: &str = "typst::unregistered_element";

/// One `typst::unregistered_element` per content field of the plate JSON
/// `data` and element name the compile drew with no renderer, counting that
/// name's runs in the field. The registry is the plate's final state, so a
/// name that falls through once falls through wherever it stands.
pub(crate) fn unregistered(
    document: &PagedDocument,
    config: &QuillConfig,
    data: &serde_json::Value,
) -> Vec<Diagnostic> {
    let Some(label) = Label::new(PicoStr::intern(MARKER)) else {
        return Vec::new();
    };
    let mut fallen = BTreeSet::new();
    let mut rendered = BTreeSet::from(["keep".to_string()]);
    for marker in document.introspector().query(&Selector::Label(label)).iter() {
        let Ok(Value::Dict(dict)) = marker.get_by_name("value") else {
            continue;
        };
        // A plate attaching the reserved label to its own metadata is ignored.
        if !matches!(dict.get("kind"), Ok(Value::Str(kind)) if kind.as_str() == MARKER) {
            continue;
        }
        let Ok(Value::Str(name)) = dict.get("name") else {
            continue;
        };
        fallen.insert(name.to_string());
        if let Ok(Value::Array(registered)) = dict.get("registered") {
            rendered.extend(registered.iter().filter_map(|v| match v {
                Value::Str(s) => Some(s.to_string()),
                _ => None,
            }));
        }
    }
    if fallen.is_empty() {
        return Vec::new();
    }
    let hint = format!("this quill renders {}", spelled(&rendered));
    let mut diags = Vec::new();
    config.each_plate_content(data, &mut |at: &DocPath, content: &Content| {
        for name in &fallen {
            let count = element_runs(content, name);
            if count > 0 {
                diags.push(warning(name, count, at, &hint));
            }
        }
    });
    diags
}

fn warning(name: &str, count: usize, at: &DocPath, hint: &str) -> Diagnostic {
    let message = if count == 1 {
        format!("`qm-{name}` has no renderer in this quill, so it draws only what it wraps")
    } else {
        format!(
            "`qm-{name}` has no renderer in this quill, so its {count} runs in this field \
             draw only what they wrap"
        )
    };
    Diagnostic::new(Severity::Warning, message)
        .with_code(CODE.to_string())
        .with_path(at.to_string())
        .with_hint(hint.to_string())
}

/// The names as `qm-` tags in an English list.
fn spelled(names: &BTreeSet<String>) -> String {
    let tags: Vec<String> = names.iter().map(|n| format!("`qm-{n}`")).collect();
    match tags.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => tags.concat(),
    }
}
