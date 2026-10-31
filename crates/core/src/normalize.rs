//! Post-parse normalization of a [`Document`](crate::document::Document):
//! payload field names to Unicode NFC, so composed `"café"` and decomposed
//! `"cafe\u{0301}"` compare equal. Field *values* pass through verbatim, and
//! bodies are already normalized at import.

use crate::document::Card;
use unicode_normalization::UnicodeNormalization;

/// Normalize a field name to Unicode NFC, so visually identical keys compare
/// equal.
pub fn normalize_field_name(name: &str) -> String {
    name.nfc().collect()
}

/// Normalize every card's payload field names to Unicode NFC. Values and
/// bodies carry through unchanged. Idempotent.
pub fn normalize_document(doc: crate::document::Document) -> crate::document::Document {
    use crate::document::Document;

    let main = normalize_card(doc.main());
    let normalized_cards: Vec<Card> = doc.cards().iter().map(normalize_card).collect();

    Document::from_main_and_cards(main, normalized_cards)
}

fn normalize_card(card: &Card) -> Card {
    use crate::document::PayloadItem;
    let mut payload = card.payload().clone();
    let renames: Vec<(String, String)> = payload
        .items()
        .iter()
        .filter_map(|item| match item {
            PayloadItem::Field { key, .. } => {
                let normalized = normalize_field_name(key);
                (normalized != *key).then(|| (key.clone(), normalized))
            }
            _ => None,
        })
        .collect();
    for (from, to) in renames {
        payload.rename_field(&from, to);
    }
    Card::from_parts(payload, card.body().clone())
}

#[cfg(test)]
mod tests {
    use crate::document::{Card, Document, Payload};
    use crate::value::QuillValue;

    #[test]
    fn a_non_nfc_key_renames_to_nfc_in_place() {
        let card = |kind: &str| {
            let mut map = indexmap::IndexMap::new();
            map.insert("a".to_string(), QuillValue::from_json(serde_json::json!(1)));
            map.insert(
                "cafe\u{301}".to_string(),
                QuillValue::from_json(serde_json::json!("hot")),
            );
            map.insert("z".to_string(), QuillValue::from_json(serde_json::json!(2)));
            let mut payload = Payload::from_index_map(map);
            payload.set_kind(kind);
            if kind == "main" {
                payload.set_quill("q@1.0".parse().unwrap());
            }
            Card::from_parts(payload, quillmark_content::model::Normalized::empty())
        };
        let doc = Document::from_main_and_cards(card("main"), vec![card("note")]);
        let normalized = super::normalize_document(doc);
        for card in std::iter::once(normalized.main()).chain(normalized.cards()) {
            let keys: Vec<&str> = card.payload().keys().map(String::as_str).collect();
            assert_eq!(keys, ["a", "caf\u{e9}", "z"]);
            assert_eq!(
                card.payload().get("caf\u{e9}").unwrap().as_str(),
                Some("hot")
            );
        }
    }
}
