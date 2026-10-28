use super::QuillConfig;

impl QuillConfig {
    /// YAML encoding of [`QuillConfig::schema`].
    pub fn schema_yaml(&self) -> Result<String, crate::error::YamlError> {
        serde_saphyr::to_string(&self.schema()).map_err(crate::error::YamlError::from_ser)
    }
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid quill yaml")
    }

    const FULL: &str = r#"
quill:
  name: full
  version: "1.0.0"
  backend: typst
  description: Full
main:
  fields:
    page_count:
      type: integer
    meta:
      type: object
      title: Meta
      properties:
        status:
          type: enum
          values: [draft, final]
          default: draft
card_kinds:
  indorsement:
    ui:
      groups:
        closing: { title: Closing }
    fields:
      signature_block:
        type: text
        ui: { group: closing }
"#;

    #[test]
    fn schema_keys_emit_in_declaration_order() {
        let schema = cfg(r#"
quill: { name: order, version: "1.0.0", backend: typst, description: x }
main:
  fields:
    zeta:
      type: enum
      values: [b, a]
      ui: { compact: true }
      default: b
      example: a
      description: d
      title: T
    alpha:
      type: object
      properties:
        yankee: { type: text }
        bravo: { type: integer }
    mike:
      type: richtext
      multiline: false
      ui: { compact: true }
card_kinds:
  zulu:
    fields:
      x: { type: text }
  alpha:
    fields:
      y: { type: text }
"#)
        .schema();
        let keys = |v: &serde_json::Value| -> Vec<String> {
            v.as_object().unwrap().keys().cloned().collect()
        };
        let fields = &schema["main"]["fields"];
        assert_eq!(keys(fields), ["zeta", "alpha", "mike"]);
        assert_eq!(keys(&fields["alpha"]["properties"]), ["yankee", "bravo"]);
        assert_eq!(keys(&schema["card_kinds"]), ["zulu", "alpha"]);
        assert_eq!(
            keys(&fields["zeta"]),
            ["type", "title", "description", "default", "example", "ui", "values"]
        );
        assert_eq!(keys(&fields["mike"]), ["type", "ui", "multiline"]);
    }

    #[test]
    fn json_yaml_parity() {
        let config = cfg(FULL);
        let parse = |yaml: &str| serde_saphyr::from_str::<serde_json::Value>(yaml).unwrap();
        assert_eq!(config.schema(), parse(&config.schema_yaml().unwrap()));
    }
}
