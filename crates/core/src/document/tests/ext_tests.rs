use serde_json::json;

use crate::document::tests::parse;
use crate::document::Document;

#[test]
fn ext_rides_any_card_through_markdown_and_storage() {
    let doc = parse(
        "\
~~~card-yaml
$quill: q@1.0
$kind: main
$ext:
  presentation:
    title: \"Greeting Card\"
  collapsed: false
title: Hi
~~~

Body.

~~~card-yaml
$kind: indorsement
$ext:
  rename: \"Cmdr's response\"
from: X
~~~
",
    );
    let ext = doc.main().ext().expect("$ext present");
    assert_eq!(ext["presentation"]["title"], json!("Greeting Card"));
    assert_eq!(ext["collapsed"], json!(false));
    assert_eq!(
        doc.cards()[0].ext().expect("composable card $ext present")["rename"],
        json!("Cmdr's response")
    );

    assert_eq!(doc, parse(&doc.to_markdown()));

    let json = serde_json::to_string(&doc).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    assert!(json.contains("\"type\":\"ext\""), "{json}");
}

/// An empty mapping at any depth of `$ext` or `$seed` keeps its key and reads
/// back as a mapping, not as null.
#[test]
fn empty_meta_mappings_keep_their_braces() {
    for block in [
        "$ext: {}\n",
        "$seed: {}\n",
        "$ext:\n  editor: {}\n",
        "$ext:\n  editor:\n    tips: {}\n",
        "$seed:\n  indorsement: {}\n",
    ] {
        let src = format!("~~~card-yaml\n$quill: q@1.0\n$kind: main\n{block}~~~\n");
        let doc = parse(&src);
        let emitted = doc.to_markdown();
        assert!(emitted.contains(block), "{block:?} lost:\n{emitted}");
        assert_eq!(doc, parse(&emitted), "{block:?}");
    }
}
