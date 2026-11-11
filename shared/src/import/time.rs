//! The `<time>` / `<Time>` stamp of an imported GPX or TCX fix.

use super::kt_trim;

/// ISO-8601 stamp → unix seconds; 0 when missing or unparseable (a route-only
/// GPX). Accepts `Z`, `+02:00`, `+02` and `+0200` offsets. GPX and TCX require
/// UTC, so a zone-naive stamp (`YYYY-MM-DDThh:mm:ss`, written by some
/// exporters) is read as UTC.
///
/// Reads exactly what the JVM's java.time parsers the shell once used accepted,
/// tried in turn (pinned against them by `fixtures/import/times.json`):
/// `Instant.parse` (seconds required; `24:00:00` is the next midnight,
/// `23:59:60` reads as `:59`), `OffsetDateTime.parse` (strict; `Z`, `+HH`,
/// `+HH:MM`, `+HH:MM:ss`), `LocalDateTime.parse` (strict; no offset, read as
/// UTC), and a colon-less offset (`Z` or `+HHMM`) resolved smartly (a day past
/// the month's end is its last day, `24:00` the next midnight).
/// Case-insensitive `T`/`Z`; fractions of up to nine digits are dropped.
pub(crate) fn parse_import_time_sec(raw: &str) -> i64 {
    let s = kt_trim(raw);
    if s.is_empty() {
        return 0;
    }
    let Some(t) = read_iso_stamp(s) else {
        return 0;
    };
    t.as_instant()
        .or_else(|| t.as_offset_date_time())
        .or_else(|| t.as_local_date_time())
        .or_else(|| t.as_colonless_offset())
        .unwrap_or(0)
}

/// How an [`IsoStamp`] names its offset.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IsoOffset {
    None,
    Z,
    Hours,
    HoursMinutes,
    HoursMinutesSeconds,
    Colonless,
}

/// The fields of one ISO-8601 date-time as written, before any range check.
struct IsoStamp {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    /// `None` when the stamp has no seconds.
    second: Option<i64>,
    /// No fraction, or one of only zeros.
    whole_second: bool,
    offset: IsoOffset,
    offset_sec: i64,
}

/// java.time's year range (`Year.MIN_VALUE..=Year.MAX_VALUE`).
const ISO_YEARS: std::ops::RangeInclusive<i64> = -999_999_999..=999_999_999;

/// `Instant.MIN` / `Instant.MAX` in epoch seconds.
const INSTANT_MIN_SEC: i64 = -31_557_014_167_219_200;
const INSTANT_MAX_SEC: i64 = 31_556_889_864_403_199;

impl IsoStamp {
    /// Unix seconds as `Instant.parse` reads the stamp (ISO_INSTANT).
    fn as_instant(&self) -> Option<i64> {
        let second = self.second?;
        if matches!(
            self.offset,
            IsoOffset::None | IsoOffset::Hours | IsoOffset::Colonless
        ) {
            return None;
        }
        let (mut h, mut sec, mut days) = (self.hour, second, 0);
        if h == 24 && self.minute == 0 && sec == 0 && self.whole_second {
            h = 0;
            days = 1;
        } else if h == 23 && self.minute == 59 && sec == 60 {
            sec = 59;
        }
        if !(1..=12).contains(&self.month)
            || !(1..=month_length(self.year, self.month)).contains(&self.day)
            || !(0..=23).contains(&h)
            || !(0..=59).contains(&self.minute)
            || !(0..=59).contains(&sec)
        {
            return None;
        }
        let epoch = (iso_epoch_day(self.year, self.month, self.day) + days) * 86_400
            + h * 3600
            + self.minute * 60
            + sec
            - self.offset_sec;
        (INSTANT_MIN_SEC..=INSTANT_MAX_SEC)
            .contains(&epoch)
            .then_some(epoch)
    }

    /// Unix seconds as `OffsetDateTime.parse` reads the stamp
    /// (ISO_OFFSET_DATE_TIME, strict).
    fn as_offset_date_time(&self) -> Option<i64> {
        if matches!(self.offset, IsoOffset::None | IsoOffset::Colonless) {
            return None;
        }
        Some(self.strict_local_sec()? - self.offset_sec)
    }

    /// Unix seconds as `LocalDateTime.parse` reads the stamp (strict), taken as UTC.
    fn as_local_date_time(&self) -> Option<i64> {
        if self.offset == IsoOffset::None {
            self.strict_local_sec()
        } else {
            None
        }
    }

    /// Unix seconds as an ISO local date-time with a `+HHMM` / `Z` offset reads
    /// the stamp, resolved smartly: a day past the month's end is its last
    /// day, `24:00` the next midnight.
    fn as_colonless_offset(&self) -> Option<i64> {
        if !matches!(self.offset, IsoOffset::Z | IsoOffset::Colonless) {
            return None;
        }
        if !ISO_YEARS.contains(&self.year)
            || !(1..=12).contains(&self.month)
            || !(1..=31).contains(&self.day)
            || !(0..=59).contains(&self.minute)
        {
            return None;
        }
        let sec = self.second.unwrap_or(0);
        let (mut h, mut days) = (self.hour, 0);
        if h == 24 && self.minute == 0 && sec == 0 && self.whole_second {
            h = 0;
            days = 1;
        } else if !(0..=23).contains(&h) || !(0..=59).contains(&sec) {
            return None;
        }
        let dom = self.day.min(month_length(self.year, self.month));
        if days == 1 && self.year == *ISO_YEARS.end() && self.month == 12 && dom == 31 {
            return None;
        }
        Some(
            (iso_epoch_day(self.year, self.month, dom) + days) * 86_400
                + h * 3600
                + self.minute * 60
                + sec
                - self.offset_sec,
        )
    }

    /// The stamp as strictly valid local date-time fields, in seconds since
    /// 1970-01-01T00:00.
    fn strict_local_sec(&self) -> Option<i64> {
        let sec = self.second.unwrap_or(0);
        if !ISO_YEARS.contains(&self.year)
            || !(1..=12).contains(&self.month)
            || !(1..=month_length(self.year, self.month)).contains(&self.day)
            || !(0..=23).contains(&self.hour)
            || !(0..=59).contains(&self.minute)
            || !(0..=59).contains(&sec)
        {
            return None;
        }
        Some(
            iso_epoch_day(self.year, self.month, self.day) * 86_400
                + self.hour * 3600
                + self.minute * 60
                + sec,
        )
    }
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_length(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the proleptic Gregorian `year`-`month`-`day`.
fn iso_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The fields of `s` when it has java.time's ISO date-time shape, else `None`:
/// a year of exactly four digits, or a sign and up to ten (`+` needs five or
/// more; `-0000` is refused), `-MM-dd`, `T` (any case), `HH:mm`, optional `:ss`
/// with an optional fraction of up to nine digits (a bare `.` included), then
/// nothing, `Z` (any case), `±HH`, `±HH:MM`, `±HH:MM:ss` or `±HHMM`. An offset
/// field past its range (hours past 23, minutes or seconds past 59) or beyond
/// ±18:00 refuses the stamp.
fn read_iso_stamp(s: &str) -> Option<IsoStamp> {
    let mut r = Cursor {
        s: s.as_bytes(),
        i: 0,
    };
    let negative_year = r.literal(b'-', false);
    let positive_year = !negative_year && r.literal(b'+', false);
    let year_start = r.i;
    while r.i < r.s.len() && r.i - year_start < 10 && r.s[r.i].is_ascii_digit() {
        r.i += 1;
    }
    let year_digits = r.i - year_start;
    if year_digits < 4
        || (positive_year && year_digits == 4)
        || (!positive_year && !negative_year && year_digits > 4)
    {
        return None;
    }
    let year_value = r.s[year_start..r.i]
        .iter()
        .fold(0i64, |v, d| v * 10 + i64::from(d - b'0'));
    if negative_year && year_value == 0 {
        return None;
    }
    let year = if negative_year {
        -year_value
    } else {
        year_value
    };
    if !r.literal(b'-', false) {
        return None;
    }
    let month = r.digits(2)?;
    if !r.literal(b'-', false) {
        return None;
    }
    let day = r.digits(2)?;
    if !r.literal(b'T', true) {
        return None;
    }
    let hour = r.digits(2)?;
    if !r.literal(b':', false) {
        return None;
    }
    let minute = r.digits(2)?;
    let mut second = None;
    let mut whole_second = true;
    if r.literal(b':', false) {
        second = Some(r.digits(2)?);
        if r.literal(b'.', false) {
            let fraction_start = r.i;
            while r.i < r.s.len() && r.i - fraction_start < 9 && r.s[r.i].is_ascii_digit() {
                r.i += 1;
            }
            whole_second = r.s[fraction_start..r.i].iter().all(|&d| d == b'0');
        }
    }
    let mut offset = IsoOffset::None;
    let mut offset_sec = 0;
    if r.literal(b'Z', true) {
        offset = IsoOffset::Z;
    } else if r.i < r.s.len() && (r.s[r.i] == b'+' || r.s[r.i] == b'-') {
        let sign = if r.s[r.i] == b'-' { -1 } else { 1 };
        r.i += 1;
        let h = r.digits(2).filter(|&h| h <= 23)?;
        let mut m = 0;
        let mut sec = 0;
        offset = if r.literal(b':', false) {
            m = r.digits(2).filter(|&m| m <= 59)?;
            if r.literal(b':', false) {
                sec = r.digits(2).filter(|&s| s <= 59)?;
                IsoOffset::HoursMinutesSeconds
            } else {
                IsoOffset::HoursMinutes
            }
        } else if r.i < r.s.len() {
            m = r.digits(2).filter(|&m| m <= 59)?;
            IsoOffset::Colonless
        } else {
            IsoOffset::Hours
        };
        offset_sec = sign * (h * 3600 + m * 60 + sec);
        if !(-18 * 3600..=18 * 3600).contains(&offset_sec) {
            return None;
        }
    }
    if r.i != r.s.len() {
        return None;
    }
    Some(IsoStamp {
        year,
        month,
        day,
        hour,
        minute,
        second,
        whole_second,
        offset,
        offset_sec,
    })
}

/// A read position in a stamp's bytes. Only ASCII is ever matched, so a
/// multi-byte character simply fails the match it sits in.
struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    /// `n` ASCII digits as a number, advancing past them.
    fn digits(&mut self, n: usize) -> Option<i64> {
        let digits = self.s.get(self.i..self.i + n)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.i += n;
        Some(digits.iter().fold(0, |v, d| v * 10 + i64::from(d - b'0')))
    }

    /// Whether the next byte is `c` (ASCII case-insensitive when
    /// `ignore_case`), advancing past it if so.
    fn literal(&mut self, c: u8, ignore_case: bool) -> bool {
        let hit = self
            .s
            .get(self.i)
            .is_some_and(|&b| b == c || (ignore_case && b.eq_ignore_ascii_case(&c)));
        if hit {
            self.i += 1;
        }
        hit
    }
}
