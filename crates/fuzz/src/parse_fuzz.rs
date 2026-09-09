//! `Document::parse` over a card-yaml payload of generated width: every
//! `fieldN: valueN` line reaches the parsed payload.

use proptest::prelude::*;
use quillmark_core::Document;

proptest! {
    /// Shaped payloads of growing width: every generated field survives parse.
    #[test]
    fn fuzz_decompose_large_payload(size in 1usize..100) {
        let fields: Vec<String> = (0..size)
            .map(|i| format!("field{}: value{}", i, i))
            .collect();
        let payload = fields.join("\n");
        let markdown =
            format!("~~~card-yaml\n$quill: test_quill\n$kind: main\n{}\n~~~\n\nContent", payload);

        let doc = Document::parse(&markdown)
            .expect("a well-formed payload parses")
            .document;
        prop_assert_eq!(doc.main().payload().len(), size);
    }
}
