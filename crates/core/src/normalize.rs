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

    #[test]
    fn test_normalize_document_idempotent() {
        use crate::document::Document;

        let doc =
            Document::parse("~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\n<<content>>")
                .unwrap()
                .document;
        let normalized_once = super::normalize_document(doc);
        let normalized_twice = super::normalize_document(normalized_once.clone());

        assert_eq!(
            normalized_once.main().body_markdown(),
            normalized_twice.main().body_markdown()
        );
    }

    #[test]
    fn test_normalize_document_yaml_field_bidi_preserved() {
        use crate::document::Document;

        let doc = Document::parse(
            "~~~card-yaml\n$quill: test\n$kind: main\ntitle: a\u{202D}b\n~~~\n",
        )
        .unwrap()
        .document;
        let normalized = super::normalize_document(doc);
        assert_eq!(
            normalized
                .main()
                .payload()
                .get("title")
                .unwrap()
                .as_str()
                .unwrap(),
            "a\u{202D}b"
        );
    }

}
