//! Card identity: the `$id` grammar, and the mint that gives every composable
//! card one. The policy is `prose/canon/DOCUMENT_STORAGE.md` § "The id
//! handles".

use std::collections::HashSet;

use sha2::{Digest, Sha256};

use super::Card;
use crate::error::{diag_args, Diagnostic, Severity};
use crate::path::DocPath;

/// The diagnostic code a parse's re-minted card id rides.
pub const CARD_ID_REMINTED: &str = "parse::card_id_reminted";

/// The warning a parse owes a card whose spelled `$id` it replaced: one
/// outside the grammar, or one an earlier card holds. The caller anchors it
/// at the card.
pub(crate) fn reminted_warning(spelled: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        format!("card id `{spelled}` was replaced with a new one: it is not a card id, or an earlier card holds it"),
    )
    .with_code(CARD_ID_REMINTED.to_string())
    .with_hint(
        "A card id is 1 to 64 of [A-Za-z0-9_-], opening with a letter or digit, and unique in the document. \
         Leave `$id` out of a new card and one is minted."
            .to_string(),
    )
    .with_args(diag_args! { "id" => spelled })
}

/// The longest `$id` a card carries.
pub const MAX_CARD_ID_LEN: usize = 64;

/// Whether `id` is a card id: 1 to [`MAX_CARD_ID_LEN`] ASCII letters, digits,
/// `_` and `-`, opening with a letter or digit.
pub fn is_card_id(id: &str) -> bool {
    id.len() <= MAX_CARD_ID_LEN
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The base32 letters a minted id spells, without `l`, `o`, `0` or `1`.
const ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// The `n`th token of the mint sequence: `c` and five letters of the hash of
/// `n`. A function of `n` alone, never of a clock or a counter outside the
/// document, and shaped so it reads as no ordinal.
fn token(n: usize) -> String {
    let digest = Sha256::digest(format!("quillmark/card-id\n{n}").as_bytes());
    let bits = u64::from_be_bytes(digest[..8].try_into().expect("a SHA-256 digest is 32 bytes"));
    let mut out = String::from("c");
    for k in 0..5 {
        out.push(char::from(ALPHABET[((bits >> (59 - 5 * k)) & 31) as usize]));
    }
    out
}

/// The mint over one document: each call takes the next token of the
/// sequence that `taken` does not hold, and holds it.
pub(crate) struct Mint {
    taken: HashSet<String>,
    next: usize,
}

impl Mint {
    pub(crate) fn new(taken: HashSet<String>) -> Self {
        Mint { taken, next: 0 }
    }

    pub(crate) fn mint(&mut self) -> String {
        loop {
            let id = token(self.next);
            self.next += 1;
            if self.taken.insert(id.clone()) {
                return id;
            }
        }
    }
}

/// A card the settle gave a new id where it spelled one: its index and the id
/// it spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reminted {
    pub index: usize,
    pub spelled: String,
}

/// Give every card in `cards` a card id no other holds: one it spells that is
/// valid and no earlier card holds stays, and every other card mints, past
/// the ids the cards keep and past `reserved`. Returns each card that spelled
/// an id it did not keep.
pub(crate) fn settle(cards: &mut [Card], reserved: &HashSet<String>) -> Vec<Reminted> {
    let mut held: HashSet<String> = HashSet::new();
    let mut needs = Vec::new();
    let mut reminted = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        match card.id() {
            Some(id) if is_card_id(id) && held.insert(id.to_string()) => {}
            Some(id) => {
                reminted.push(Reminted {
                    index,
                    spelled: id.to_string(),
                });
                needs.push(index);
            }
            None => needs.push(index),
        }
    }
    if needs.is_empty() {
        return reminted;
    }
    held.extend(reserved.iter().cloned());
    let mut mint = Mint::new(held);
    for index in needs {
        cards[index].payload_mut().set_id(mint.mint());
    }
    reminted
}

/// [`settle`], with a [`reminted_warning`] onto `warnings` at each card that
/// spelled an id it did not keep.
pub(crate) fn settle_reporting(
    cards: &mut [Card],
    reserved: &HashSet<String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for reminted in settle(cards, reserved) {
        let at = DocPath::card(cards[reminted.index].kind(), reminted.index);
        warnings.push(reminted_warning(&reminted.spelled).with_path(at.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_id_is_a_card_id_and_the_sequence_does_not_repeat_early() {
        let tokens: HashSet<String> = (0..1000).map(token).collect();
        assert_eq!(tokens.len(), 1000);
        assert!(tokens.iter().all(|t| is_card_id(t) && t.len() == 6));
    }

    #[test]
    fn the_grammar() {
        for ok in ["c3kq9f", "intro", "0d8e2b9c-6f6a-4e8b-9a8d-2a6f1d1c3b4e", "a_b"] {
            assert!(is_card_id(ok), "{ok}");
        }
        for bad in ["", "-a", "_a", "a b", "a.b", "é", &"a".repeat(65)] {
            assert!(!is_card_id(bad), "{bad}");
        }
    }

    #[test]
    fn the_mint_skips_what_is_taken() {
        let mut mint = Mint::new([token(0), token(2)].into_iter().collect());
        assert_eq!((mint.mint(), mint.mint()), (token(1), token(3)));
    }
}
