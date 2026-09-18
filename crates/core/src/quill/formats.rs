use std::sync::LazyLock;

use time::format_description::{self, FormatDescriptionV3};
use time::{Date, PrimitiveDateTime};

use super::types::DatePrecision;

static DATE_FMT: LazyLock<FormatDescriptionV3<'static>> = LazyLock::new(|| {
    format_description::parse_borrowed::<3>("[year]-[month]-[day]").expect("valid format")
});

// Strict offset-less wall-clock datetime forms: the `type: datetime` grammar.
// T separator required, seconds optional (zero-filled); no offset, no space
// separator, no fractional seconds, no bare date. Most-specific first so a
// with-seconds string does not partially match the minute-only variant. The
// `time` parser consumes the whole input, so a trailing offset / fraction /
// stray character fails rather than silently truncating.
static DATETIME_FMTS: LazyLock<[FormatDescriptionV3<'static>; 2]> = LazyLock::new(|| {
    [
        format_description::parse_borrowed::<3>("[year]-[month]-[day]T[hour]:[minute]:[second]")
            .expect("valid format"),
        format_description::parse_borrowed::<3>("[year]-[month]-[day]T[hour]:[minute]")
            .expect("valid format"),
    ]
});

/// True when `s` is a valid `type: date` value: a strict calendar date with
/// no time component. See [`parse_date`].
pub(crate) fn is_valid_date(s: &str) -> bool {
    parse_date(s).is_some()
}

/// True when `s` is a valid `date` value at `precision`. See
/// [`parse_date_at`].
pub(crate) fn is_valid_date_at(s: &str, precision: DatePrecision) -> bool {
    parse_date_at(s, precision).is_some()
}

/// Parse a calendar date at `precision` — `YYYY`, `YYYY-MM`, or the full
/// `YYYY-MM-DD` — to `(year, month, day)` with the components the precision
/// does not carry left `None`. The grammar is **exact**, not a prefix: a
/// `precision: month` field rejects `2024-08-15` as it rejects `2024`, so the
/// stored value carries the precision it was declared at and nothing has to
/// guess which components are real.
///
/// [`parse_date`] is the `day` case, and carries the leading-sign and
/// calendar-validity rules the narrower grammars inherit.
pub fn parse_date_at(s: &str, precision: DatePrecision) -> Option<(i32, Option<u8>, Option<u8>)> {
    match precision {
        DatePrecision::Day => parse_date(s).map(|(y, m, d)| (y, Some(m), Some(d))),
        // Anchored at the floor of the range the prefix names, so the `time`
        // parser's calendar validity (a 13th month, a 31st of February) still
        // does the work.
        DatePrecision::Month => {
            let (year, month) = s.split_once('-')?;
            (year.len() == 4 && month.len() == 2)
                .then(|| parse_date(&format!("{s}-01")))
                .flatten()
                .map(|(y, m, _)| (y, Some(m), None))
        }
        DatePrecision::Year => (s.len() == 4)
            .then(|| parse_date(&format!("{s}-01-01")))
            .flatten()
            .map(|(y, _, _)| (y, None, None)),
    }
}

/// True when `s` is a valid `type: datetime` value: a strict offset-less
/// wall-clock datetime. See [`parse_datetime`].
pub(crate) fn is_valid_datetime(s: &str) -> bool {
    parse_datetime(s).is_some()
}

/// Parse a strict calendar date `YYYY-MM-DD` to `(year, month, day)`: the
/// `type: date` grammar. Any time component (a `T`/space separator and beyond)
/// is rejected: a date field holds a date, full stop, and a time-bearing string
/// is a `type: datetime`. The `time` parser enforces zero-padding and calendar
/// validity (`2026-02-30` is rejected). `None` for any other string.
///
/// `time`'s `[year]` component defaults to `sign:automatic`, accepting a
/// leading `+`/`-`; rejected here up front so the grammar stays strict about
/// the sign too, instead of lowering a BCE year into the Typst backend.
pub fn parse_date(s: &str) -> Option<(i32, u8, u8)> {
    if s.starts_with(['+', '-']) {
        return None;
    }
    Date::parse(s, &*DATE_FMT)
        .ok()
        .map(|d| (d.year(), u8::from(d.month()), d.day()))
}

/// Parse a strict offset-less wall-clock datetime `YYYY-MM-DDThh:mm[:ss]` to
/// `(year, month, day, hour, minute, second)`, seconds zero-filled when absent:
/// the `type: datetime` grammar. Rejects timezone offsets (`Z`, `±HH:MM`),
/// the space separator, fractional seconds, and a bare date. The engine keeps
/// wall-clock semantics end to end and does no zone math, so an offset is an
/// error at the seam, never a silently dropped component; the whose-wall-clock
/// decision is forced to the consumer boundary where the context lives. `None`
/// for any other string.
///
/// Same leading-sign rejection as [`parse_date`]: a signed year is rejected
/// up front rather than accepted and lowered as BCE.
pub fn parse_datetime(s: &str) -> Option<(i32, u8, u8, u8, u8, u8)> {
    if s.starts_with(['+', '-']) {
        return None;
    }
    for fmt in DATETIME_FMTS.iter() {
        if let Ok(dt) = PrimitiveDateTime::parse(s, fmt) {
            return Some((
                dt.year(),
                u8::from(dt.month()),
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_at_takes_exactly_its_precision() {
        assert_eq!(parse_date_at("2024", DatePrecision::Year), Some((2024, None, None)));
        assert_eq!(
            parse_date_at("2024-08", DatePrecision::Month),
            Some((2024, Some(8), None))
        );
        assert_eq!(
            parse_date_at("2024-08-15", DatePrecision::Day),
            Some((2024, Some(8), Some(15)))
        );

        // Exact, not a prefix: a value carries the precision it was declared at.
        for (s, p) in [
            ("2024-08", DatePrecision::Year),
            ("2024-08-15", DatePrecision::Month),
            ("2024", DatePrecision::Month),
            ("2024-08", DatePrecision::Day),
            ("2024-8", DatePrecision::Month),
            ("2024-13", DatePrecision::Month),
            ("+2024", DatePrecision::Year),
            ("24", DatePrecision::Year),
            ("", DatePrecision::Year),
        ] {
            assert_eq!(parse_date_at(s, p), None, "expected rejected: {s} at {p}");
        }
    }


    #[test]
    fn parse_date_accepts_bare_calendar_dates() {
        assert_eq!(parse_date("2026-06-01"), Some((2026, 6, 1)));
        assert_eq!(parse_date("2000-12-31"), Some((2000, 12, 31)));
    }

    #[test]
    fn parse_date_rejects_time_components_and_malformed() {
        for s in [
            "",
            "2026",
            "2026-06",
            "2026-6-1",              // not zero-padded
            "2026-13-01",            // month out of range
            "2026-02-30",            // Feb 30
            "2026-06-01T12:00",      // time component → this is a datetime
            "2026-06-01T12:00:00",   // time component
            "2026-06-01 12:00",      // space-separated time component
            "2026-06-01Z",           // stray offset marker
            "2026-06-01T12:00:00Z",  // offset instant
            "not-a-date",
            "-2026-01-01",           // signed year (BCE): leading sign rejected
            "+2026-01-01",           // signed year: leading sign rejected
        ] {
            assert_eq!(parse_date(s), None, "expected rejected date: {s}");
        }
    }


    #[test]
    fn parse_datetime_accepts_offsetless_wall_clock() {
        // Seconds present.
        assert_eq!(
            parse_datetime("2026-06-01T14:30:15"),
            Some((2026, 6, 1, 14, 30, 15))
        );
        // Seconds omitted → zero-filled (the one human concession).
        assert_eq!(
            parse_datetime("2026-06-01T14:30"),
            Some((2026, 6, 1, 14, 30, 0))
        );
        assert_eq!(
            parse_datetime("2026-06-01T00:00:00"),
            Some((2026, 6, 1, 0, 0, 0))
        );
    }

    #[test]
    fn parse_datetime_rejects_offsets_space_fraction_and_bare_date() {
        for s in [
            "",
            "2026-06-01",               // bare date → this is a `type: date`
            "2026-06-01T14:30:00Z",     // UTC offset, rejected, never treated as local
            "2026-06-01T14:30:00+00:00", // explicit zero offset, no special case
            "2026-06-01T14:30:00+05:30", // offset
            "2026-06-01T14:30:00-05:00", // offset
            "2026-06-01 14:30:00",      // YAML space separator, not RFC 3339
            "2026-06-01T14:30:00.5",    // fractional seconds
            "2026-06-01T14:30:00.123",  // fractional seconds
            "2026-06-01T14:30:00 ",     // trailing space
            "2026-06-01T14",            // no minute
            "2026-06-01T14:3",          // single-digit minute
            "2026-13-01T14:30:00",      // month out of range
            "2026-02-30T14:30:00",      // Feb 30
            "-2026-06-01T12:00",        // signed year (BCE), leading sign rejected
            "+2026-06-01T12:00",        // signed year, leading sign rejected
        ] {
            assert_eq!(parse_datetime(s), None, "expected rejected datetime: {s}");
        }
    }
}
