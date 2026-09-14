//! USV → UTF-8 byte conversion: [`Content`](crate::model::Content) positions count
//! Unicode scalar values, Rust slicing needs byte offsets.

/// USV index → UTF-8 byte offset into `text`. Saturates to `text.len()` for an
/// index at or past the end, so it is safe to use as a slice bound.
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASTRAL: &str = "a😀b";

    #[test]
    fn char_to_byte_astral() {
        assert_eq!(char_to_byte(ASTRAL, 0), 0);
        assert_eq!(char_to_byte(ASTRAL, 1), 1);
        assert_eq!(char_to_byte(ASTRAL, 2), 5);
        assert_eq!(char_to_byte(ASTRAL, 3), 6);
        assert_eq!(char_to_byte(ASTRAL, 99), 6);
    }
}
