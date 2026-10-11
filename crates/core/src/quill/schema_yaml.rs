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
  version: "1.0"
  backend: typst
  description: Full
main:
  ui:
    groups: [meta]
  fields:
    status:
      type: enum
      values: [draft, final]
      default: draft
      ui:
        group: meta
    page_count:
      type: integer
card_kinds:
  indorsement:
    fields:
      signature_block:
        type: string
"#;

    #[test]
    fn schema_includes_ui() {
        let config = cfg(FULL);
        let yaml = config.schema_yaml().unwrap();
        assert!(yaml.contains("values:") && yaml.contains("type: integer"));
        assert!(yaml.contains("card_kinds:") && yaml.contains("indorsement:"));
        assert!(yaml.contains("ui:") && yaml.contains("group: meta"));
    }

    #[test]
    fn omits_card_kinds_when_absent() {
        let yaml = cfg(r#"
quill: { name: solo, version: "1.0", backend: typst, description: x }
main:
  fields:
    title: { type: string }
"#)
        .schema_yaml()
        .unwrap();
        assert!(yaml.contains("main:") && !yaml.contains("card_kinds:"));
    }

    #[test]
    fn json_yaml_parity() {
        let config = cfg(FULL);
        let parse = |yaml: &str| serde_saphyr::from_str::<serde_json::Value>(yaml).unwrap();
        assert_eq!(config.schema(), parse(&config.schema_yaml().unwrap()));
    }
}
