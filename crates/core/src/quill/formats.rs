/// Returns `true` when `s` matches the YAML 1.1 timestamp grammar.
///
/// Accepted forms (YAML 1.1 §10.3.1), with seconds optional:
/// - Bare date:        `YYYY-M?M-D?D`
/// - Local datetime:   `YYYY-M?M-D?D[Tt ]h?h:MM[:SS[.s+]]`
/// - UTC:              `…Z`
/// - With offset:      `…[+-]h?h[:MM]`
/// - Space/tab may replace `T` as separator and may precede the timezone.
pub(crate) fn is_valid_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    let mut i = 0;

    // YYYY
    if n < 4 || !b[..4].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    i += 4;

    if i >= n || b[i] != b'-' {
        return false;
    }
    i += 1;

    // M or MM
    let start = i;
    while i < n && b[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=2).contains(&(i - start)) {
        return false;
    }

    if i >= n || b[i] != b'-' {
        return false;
    }
    i += 1;

    // D or DD
    let start = i;
    while i < n && b[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=2).contains(&(i - start)) {
        return false;
    }

    if i == n {
        return true; // bare date
    }

    // Time separator: T/t or one or more spaces/tabs
    if b[i] == b'T' || b[i] == b't' {
        i += 1;
    } else if b[i] == b' ' || b[i] == b'\t' {
        while i < n && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
    } else {
        return false;
    }

    // H or HH
    let start = i;
    while i < n && b[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=2).contains(&(i - start)) {
        return false;
    }

    // :MM (exactly 2 digits)
    if i >= n || b[i] != b':' {
        return false;
    }
    i += 1;
    if i + 2 > n || !b[i..i + 2].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    i += 2;

    if i == n {
        return true; // HH:MM without seconds
    }

    // Optional :SS[.s+]
    if b[i] == b':' {
        i += 1;
        if i + 2 > n || !b[i..i + 2].iter().all(|c| c.is_ascii_digit()) {
            return false;
        }
        i += 2;
        if i < n && b[i] == b'.' {
            i += 1;
            if i >= n || !b[i].is_ascii_digit() {
                return false;
            }
            while i < n && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }

    if i == n {
        return true; // local datetime
    }

    // Optional timezone: optional whitespace then Z or ±H?H[:MM]
    while i < n && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    if i >= n {
        return false; // trailing whitespace only
    }

    if b[i] == b'Z' || b[i] == b'z' {
        return i + 1 == n;
    }

    if b[i] == b'+' || b[i] == b'-' {
        i += 1;
        let start = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if !(1..=2).contains(&(i - start)) {
            return false;
        }
        if i == n {
            return true;
        }
        if b[i] == b':' {
            i += 1;
        }
        if i + 2 > n || !b[i..i + 2].iter().all(|c| c.is_ascii_digit()) {
            return false;
        }
        i += 2;
        return i == n;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_datetimes() {
        for s in [
            "2026-06-01",
            "2026-6-1",
            "2026-06-01T12:00:00",
            "2026-06-01T12:00",
            "2026-06-01T12:00:00Z",
            "2026-06-01T12:00:00z",
            "2026-06-01T12:00:00+05:30",
            "2026-06-01T12:00:00-05:00",
            "2026-06-01T12:00:00+5",
            "2026-06-01T12:00:00.5",
            "2026-06-01T12:00:00.123",
            "2026-06-01 12:00:00",
            "2026-06-01 12:00:00 Z",
            "2026-06-01 12:00:00 +05:30",
            "2026-06-01\t12:00:00",
        ] {
            assert!(is_valid_datetime(s), "expected valid: {s}");
        }
    }

    #[test]
    fn invalid_datetimes() {
        for s in [
            "",
            "2026",
            "2026-06",
            "not-a-date",
            "13-04-2026",
            "2026-06-01T",
            "2026-06-01T12",
            "2026-06-01T12:",
            "2026-06-01T12:0",
            "2026-06-01T12:00:00 ",
            "2026-06-01T12:00:00+",
            "2026-06-01T12:00:00X",
        ] {
            assert!(!is_valid_datetime(s), "expected invalid: {s}");
        }
    }
}
