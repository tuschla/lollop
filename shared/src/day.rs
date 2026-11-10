//! Day bucketing for wire `observed_at` stamps: the one calendar convention
//! every module shares.

/// Seconds per day: the one deterministic day bucket every module shares.
pub(crate) const DAY_SEC: i64 = 86_400;

/// LOCAL epoch day for a UTC `observed_at` shifted by `offset` seconds. The one
/// day-bucketing convention the app views share, so a set logged 23:30 local
/// lands on the same calendar day everywhere. Saturating, so a corrupt wire
/// stamp cannot overflow.
pub(crate) fn local_day(observed_at: i64, offset: i64) -> i64 {
    observed_at.saturating_add(offset).div_euclid(DAY_SEC)
}

/// Whether a wire `observed_at` stamp carries no date: legacy lines decode it
/// as 0, and a non-positive stamp names no real day. The one predicate every
/// dated-row filter shares, so a stamp is never dated for one check and
/// undated for another.
pub(crate) fn is_undated(observed_at: i64) -> bool {
    observed_at <= 0
}

/// Convert a count of days since the unix epoch (1970-01-01) to a civil
/// `(year, month, day)`. Howard Hinnant, "chrono-Compatible Low-Level Date
/// Algorithms" (public domain).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
