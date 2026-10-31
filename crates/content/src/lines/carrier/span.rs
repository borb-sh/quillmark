//! `qm-span`: an inline pair whose attributes spell its run's attribute marks,
//! a font family the one it carries.

use super::{Attrs, Element};
use crate::lines::model::is_font_family_name;
use std::collections::BTreeMap;

/// The family `attrs` spell, and every attribute name they hold that spells
/// nothing: one other than `family`, a family [`is_font_family_name`] refuses,
/// and each the grammar refused.
pub(crate) fn read(attrs: &Attrs) -> (Option<String>, Vec<String>) {
    let mut family = None;
    let mut refused = attrs.refused.clone();
    for (name, value) in &attrs.values {
        match name.as_str() {
            "family" if is_font_family_name(value) => family = Some(value.clone()),
            _ => refused.push(name.clone()),
        }
    }
    (family, refused)
}

/// The pair spelling a run set in `family`.
pub(crate) fn pair(family: &str) -> Element {
    let attrs = BTreeMap::from([("family".to_string(), family.to_string())]);
    Element::new(super::SPAN, attrs).expect("`family` is in the attribute grammar")
}
