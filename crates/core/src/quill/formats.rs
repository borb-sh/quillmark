use std::sync::LazyLock;

use time::format_description::{self, FormatDescriptionV3};
use time::{Date, PrimitiveDateTime};

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

/// The `type: date` keyword standing for the day of the render. It stores as
/// written and never reaches a plate: the render projection replaces it with
/// the clock's UTC date.
pub const TODAY: &str = "today";

/// A calendar date read off the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CalendarDate {
    year: i32,
    month: u8,
    day: u8,
}

impl CalendarDate {
    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// The clock's UTC date, shifted by `offset_seconds`. Under WASM the clock
    /// is JavaScript's `Date`, which `time` cannot read there.
    pub fn from_clock(offset_seconds: i64) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = time::OffsetDateTime::now_utc() + time::Duration::seconds(offset_seconds);
            let date = now.date();
            Self {
                year: date.year(),
                month: u8::from(date.month()),
                day: date.day(),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let millis = js_sys::Date::now() + offset_seconds as f64 * 1_000.0;
            let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(millis));
            Self {
                year: date.get_utc_full_year() as i32,
                // `get_utc_month` is 0-based.
                month: (date.get_utc_month() as u8).saturating_add(1),
                day: date.get_utc_date() as u8,
            }
        }
    }
}

impl std::fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// True when `s` is a valid `type: date` value: a strict calendar date with
/// no time component (see [`parse_date`]), or [`TODAY`].
pub(crate) fn is_valid_date(s: &str) -> bool {
    s == TODAY || parse_date(s).is_some()
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
