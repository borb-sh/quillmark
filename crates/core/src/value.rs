//! Value type for unified representation of YAML/JSON values.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::ops::Deref;

/// Unified value type for JSON-shaped data.
#[derive(Clone, PartialEq)]
pub struct QuillValue {
    json: JsonValue,
}

/// One step of a path into a value tree: an object key or an array index. The
/// canonical path-segment type for the whole crate.
///
/// Serializes **untagged** (a key as a JSON string, an index as a JSON number),
/// so a path crosses the binding wire as a plain JS array like
/// `["addr", "street"]` or `["recipients", 0, "name"]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// `true` when a value nests deeper than `max_depth` container levels: the
/// content crate's guard, re-exported so this crate's boundaries and the content
/// model reject the identical shape.
///
/// Every path that stores a value into a `Document` bounds nesting at
/// [`MAX_JSON_DEPTH`](quillmark_content::MAX_JSON_DEPTH), so the recursive consumers
/// (emit, plate-JSON serialization, DTO conversion) are bounded by
/// construction. The Python binding's `py_to_json_at` charges levels the same
/// way.
pub use quillmark_content::json_depth_exceeds;

/// Depth-bound an owned `$ext` / `$seed` map against
/// [`MAX_JSON_DEPTH`](quillmark_content::MAX_JSON_DEPTH), returning it
/// unchanged when within bounds. On overflow, `on_too_deep` builds the caller's
/// boundary error from the limit, so each write surface keeps its own error
/// type while the wrap-check-rebuild lives here once.
pub(crate) fn depth_check_meta_map<E>(
    map: serde_json::Map<String, serde_json::Value>,
    on_too_deep: impl FnOnce(usize) -> E,
) -> Result<serde_json::Map<String, serde_json::Value>, E> {
    let max = quillmark_content::MAX_JSON_DEPTH;
    let as_value = serde_json::Value::Object(map);
    if json_depth_exceeds(&as_value, max) {
        return Err(on_too_deep(max));
    }
    let serde_json::Value::Object(map) = as_value else {
        unreachable!("constructed as Object above")
    };
    Ok(map)
}

/// The one YAML reading every Quillmark parse shares: only YAML 1.2 Core's
/// `true`, `True`, `TRUE`, `false`, `False` and `FALSE` are booleans, and any
/// other casing (`tRue`) and YAML 1.1's words (`y`, `no`, `On`, `OFF`) stay
/// strings. Comments reach no value, so none is buffered, and a run
/// of them meets no count limit. With aliases expanded, scalar and tag bytes
/// stay under twice the input's bytes plus [`EXPANSION_SLACK`], and nodes under
/// its bytes plus [`EXPANSION_SLACK`], so a block's value is linear in its
/// source.
pub(crate) fn parse_yaml<'de, T: Deserialize<'de>>(
    yaml: &'de str,
) -> Result<T, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(
        yaml,
        serde_saphyr::options! {
            strict_booleans: true,
            emit_comments: false,
            budget: serde_saphyr::budget! {
                max_total_scalar_bytes: yaml.len().saturating_mul(2).saturating_add(EXPANSION_SLACK),
                max_nodes: yaml.len().saturating_add(EXPANSION_SLACK),
            },
        },
    )
}

const EXPANSION_SLACK: usize = 4 * 1024;

/// The number the saphyr parser reads a plain scalar of text `s` as, YAML 1.1
/// spellings (`0b11`, `1_000`) included.
pub(crate) fn yaml_number(s: &str) -> Option<serde_json::Number> {
    if s.is_empty() || s.contains(|c: char| c.is_whitespace() || c == '#') {
        return None;
    }
    match parse_yaml::<serde_json::Value>(s) {
        Ok(serde_json::Value::Number(n)) => Some(n),
        _ => None,
    }
}

/// The number YAML 1.2.2's core schema resolves a plain scalar of text `s` to:
/// a decimal integer with an optional sign (`0012` is 12), an unsigned `0o`
/// octal or `0x` hexadecimal, or a float (`1.50`, `.5`, `1.`, `1e3`). Anything
/// else is no number, `0b11`, `1_000`, `0X1F` and `-0x1F` among them, and so
/// are the non-finite floats, which JSON cannot hold.
pub(crate) fn core_number(s: &str) -> Option<serde_json::Number> {
    let digits = |t: &str, radix: u32| !t.is_empty() && t.chars().all(|c| c.is_digit(radix));
    for (prefix, radix) in [("0o", 8), ("0x", 16)] {
        if let Some(t) = s.strip_prefix(prefix) {
            return digits(t, radix)
                .then(|| u64::from_str_radix(t, radix).ok())
                .flatten()
                .map(Into::into);
        }
    }
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits(unsigned, 10) {
        let negative = s.starts_with('-');
        let int = unsigned.trim_start_matches('0');
        let int = if int.is_empty() { "0" } else { int };
        if negative {
            if let Ok(i) = format!("-{int}").parse::<i64>() {
                return Some(i.into());
            }
        } else if let Ok(u) = int.parse::<u64>() {
            return Some(u.into());
        }
    } else {
        let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
            Some((m, e)) => (m, Some(e)),
            None => (unsigned, None),
        };
        let mantissa_ok = match mantissa.split_once('.') {
            Some((int, frac)) => {
                (int.is_empty() && digits(frac, 10))
                    || (digits(int, 10) && (frac.is_empty() || digits(frac, 10)))
            }
            None => digits(mantissa, 10),
        };
        let exponent_ok =
            exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e), 10));
        if !(mantissa_ok && exponent_ok) {
            return None;
        }
    }
    s.parse::<f64>().ok().and_then(serde_json::Number::from_f64)
}

/// Whether `s` is a decimal YAML reads as a number whose own text is `s`:
/// `12`, `-3`, `1.5`, `12.0`. Each digit is significant, at most 15 of them,
/// so the shortest text of the number read is `s` again.
pub(crate) fn is_plain_number(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match digits.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (digits, None),
    };
    let int_ok = int == "0"
        || (int.starts_with(|c: char| ('1'..='9').contains(&c))
            && int.bytes().all(|b| b.is_ascii_digit()));
    let negative_zero =
        digits.len() < s.len() && int == "0" && frac.is_none_or(|f| f.bytes().all(|b| b == b'0'));
    if !int_ok || negative_zero {
        return false;
    }
    match frac {
        None => int.len() <= 18,
        Some(frac) => {
            let leading = frac.bytes().take_while(|&b| b == b'0').count();
            !frac.is_empty()
                && frac.bytes().all(|b| b.is_ascii_digit())
                && (frac == "0" || !frac.ends_with('0'))
                && int.len() + frac.len() <= 15
                && (int != "0" || leading < 4)
        }
    }
}

/// Deserialize `T` from YAML as `Quill.yaml` is read: under the parser's own
/// budget, so an over-deep document errors rather than overflowing its stack,
/// and with only `true` and `false` as booleans. A value that parses but does
/// not fit `T`, a missing field or a mistyped one, carries a position as a
/// syntax error does.
pub fn from_yaml_str<T: serde::de::DeserializeOwned>(
    yaml: &str,
) -> Result<T, crate::error::YamlError> {
    parse_yaml(yaml).map_err(|e| crate::error::YamlError::from_de(e, yaml))
}

impl QuillValue {
    /// Parse a YAML string as [`from_yaml_str`] reads it: `yes`, `n` and `off`
    /// are strings.
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, crate::error::YamlError> {
        from_yaml_str(yaml_str).map(Self::from_json)
    }

    /// The value's data.
    pub fn as_json(&self) -> &serde_json::Value {
        &self.json
    }

    pub(crate) fn as_json_mut(&mut self) -> &mut serde_json::Value {
        &mut self.json
    }

    /// Convert into the underlying JSON value.
    pub fn into_json(self) -> serde_json::Value {
        self.json
    }

    /// Create a QuillValue from a JSON value.
    pub fn from_json(json_val: serde_json::Value) -> Self {
        QuillValue { json: json_val }
    }
}

/// Scalar conversions mirror [`serde_json::Value`]'s, so a non-finite `f64`
/// maps to null. These back the `impl Into<QuillValue>` mutator parameters.
macro_rules! impl_from_scalar {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for QuillValue {
            fn from(v: $ty) -> Self {
                QuillValue::from_json(serde_json::Value::from(v))
            }
        }
    )*};
}
impl_from_scalar!(&str, String, bool, i32, i64, u32, u64, f64);

impl From<serde_json::Value> for QuillValue {
    fn from(v: serde_json::Value) -> Self {
        QuillValue::from_json(v)
    }
}

impl Deref for QuillValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        self.as_json()
    }
}

impl std::fmt::Debug for QuillValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "QuillValue({:?})", self.json)
    }
}

impl Serialize for QuillValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QuillValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = serde_json::Value::deserialize(deserializer)?;
        Ok(QuillValue::from_json(json))
    }
}

impl QuillValue {
    /// Get a field from an object by key.
    pub fn get(&self, key: &str) -> Option<QuillValue> {
        let child = self.json.as_object()?.get(key)?;
        Some(QuillValue::from_json(child.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_expand_linearly_in_the_source() {
        let refused = |yaml: &str| {
            let err = QuillValue::from_yaml_str(yaml).expect_err("expansion refused");
            assert!(err.to_string().contains("budget"), "{err}");
        };
        let anchor = "x".repeat(10_000);
        assert!(QuillValue::from_yaml_str(&format!("z: &a {anchor}\nzz: [*a]\n")).is_ok());
        refused(&format!(
            "z: &a {anchor}\nzz: [{}]\n",
            vec!["*a"; 90].join(",")
        ));

        let tens = |alias: &str| vec![alias; 10].join(",");
        refused(&format!(
            "a: &a [{}]\nb: &b [{}]\nc: &c [{}]\nd: &d [{}]\ne: [{}]\n",
            tens("[]"),
            tens("*a"),
            tens("*b"),
            tens("*c"),
            tens("*d"),
        ));
    }

    #[test]
    fn yaml_error_locates_and_sanitizes() {
        let err = QuillValue::from_yaml_str("a: 1\nb: [unclosed\n")
            .expect_err("malformed YAML must not parse");
        let diag = err.to_diagnostic("quill::yaml_parse_error", "Quill.yaml");
        let loc = diag.location.expect("a located error carries a Location");
        assert_eq!(loc.file, "Quill.yaml");
        assert!(
            loc.line >= 2 && loc.column >= 1,
            "1-indexed position at or past the unclosed sequence: {loc:?}"
        );
        assert_eq!(diag.code.as_deref(), Some("quill::yaml_parse_error"));
    }

    /// `YamlError` promises the parser is invisible, text included.
    #[test]
    fn yaml_error_strips_the_engine_api_names() {
        let err =
            QuillValue::from_yaml_str("a: 1\na: 2\n").expect_err("a duplicate key must not parse");
        assert!(
            !err.message().contains("DuplicateKeyPolicy") && !err.message().contains("Options"),
            "parser API names reached the message: {}",
            err.message()
        );
        assert!(err.message().contains("duplicate"), "{}", err.message());
    }

    #[test]
    fn json_round_trips_declaration_order_included() {
        // Identity, object key order (serde_json `preserve_order`) included.
        let original = serde_json::json!({
            "z": 1,
            "a": [true, "x", 3.5, null],
            "nested": { "k": 42 }
        });
        let qv = QuillValue::from_json(original.clone());
        assert_eq!(qv.as_json(), &original);
        assert_eq!(qv.into_json(), original);
    }

    #[test]
    fn depth_check_counts_empty_containers() {
        use serde_json::json;

        // A container occupies a level even when empty.
        assert!(!json_depth_exceeds(&json!([]), 1));
        assert!(!json_depth_exceeds(&json!({}), 1));
        assert!(json_depth_exceeds(&json!([[]]), 1));
        assert!(json_depth_exceeds(&json!({ "a": {} }), 1));

        // `[[[…[]…]]]`, built iteratively so the test stays stack-safe.
        let deep_empty = |levels: usize| {
            let mut v = serde_json::Value::Array(Vec::new());
            for _ in 1..levels {
                v = serde_json::Value::Array(vec![v]);
            }
            v
        };
        assert!(!json_depth_exceeds(&deep_empty(100), 100));
        assert!(json_depth_exceeds(&deep_empty(101), 100));
    }

    #[test]
    fn depth_check_counts_container_levels_not_the_scalar_leaf() {
        // The Python binding's `py_to_json_at` pins the same boundary.
        let scalar_terminated = |levels: usize| {
            let mut v = serde_json::json!(1);
            for _ in 0..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&scalar_terminated(100), 100));
        assert!(json_depth_exceeds(&scalar_terminated(101), 100));

        // The deepest container, not its contents, occupies the last level.
        let container_terminated = |levels: usize| {
            let mut v = serde_json::json!([1, 2, 3]);
            for _ in 1..levels {
                v = serde_json::json!({ "a": v });
            }
            v
        };
        assert!(!json_depth_exceeds(&container_terminated(100), 100));
        assert!(json_depth_exceeds(&container_terminated(101), 100));
    }

    proptest::proptest! {
        /// A number written plainly is the text of the number YAML reads.
        #[test]
        fn a_plain_number_is_its_own_text(s in "-?(0|[1-9][0-9]{0,19})(\\.[0-9]{1,17})?") {
            if is_plain_number(&s) {
                proptest::prop_assert_eq!(core_number(&s).map(|n| n.to_string()), Some(s));
            }
        }
    }

    #[test]
    fn the_core_schema_resolves_its_own_number_forms_and_no_others() {
        for (text, number) in [
            ("0012", serde_json::json!(12)),
            ("+5", serde_json::json!(5)),
            ("-0", serde_json::json!(0)),
            ("0o17", serde_json::json!(15)),
            ("0x1F", serde_json::json!(31)),
            ("1.50", serde_json::json!(1.5)),
            (".5", serde_json::json!(0.5)),
            ("1.", serde_json::json!(1.0)),
            ("1E3", serde_json::json!(1000.0)),
            ("-1.5e-3", serde_json::json!(-0.0015)),
            ("99999999999999999999", serde_json::json!(1e20)),
        ] {
            assert_eq!(
                core_number(text).map(serde_json::Value::from),
                Some(number),
                "{text}"
            );
        }
        for text in [
            "0b11", "1_000", "0x1_F", "0X1F", "0O17", "-0x1F", "+0o7", "0x", "0o8", "1e", ".e3",
            ".", "1e400", ".inf", ".nan", "1:30", "", "1.5.",
        ] {
            assert_eq!(core_number(text), None, "{text}");
        }
    }
}
