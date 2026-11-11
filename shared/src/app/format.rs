//! Display formatting helpers: weight units, local-day/week bucketing,
//! weekday names, session/goal titles.

use crate::day::local_day;
use crate::plates::LB_PER_KG;
use crate::profile::DistanceUnit;
use crate::running::{self, GoalDistance};
use crate::schema::Goal;

/// Render an inclusive integer range, collapsing to a single value when the
/// bounds coincide (e.g. `(3, 3) -> "3"`, `(6, 9) -> "6–9"`). Keeps guidance
/// copy from printing degenerate `3–3` spans.
pub(super) fn fmt_u8_range((lo, hi): (u8, u8)) -> String {
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}–{hi}")
    }
}

/// Render a percent band as whole numbers (no `%` sign), collapsing when both
/// print the same (`(50.0, 50.0) -> "50"`, `(40.0, 60.0) -> "40–60"`).
pub(super) fn fmt_pct_range(lo: f64, hi: f64) -> String {
    let (lo, hi) = (format!("{lo:.0}"), format!("{hi:.0}"));
    if lo == hi { lo } else { format!("{lo}–{hi}") }
}

/// [`fmt_pct_range`] of a fraction band (`(0.4, 0.6) -> "40–60"`).
pub(super) fn fmt_frac_pct_range(lo: f64, hi: f64) -> String {
    fmt_pct_range(lo * 100.0, hi * 100.0)
}

/// Monday-indexed weekday of an epoch-day (epoch day 0 = 1970-01-01 = Thursday).
/// `rem_euclid` handles pre-epoch (negative) days too.
///
/// Reduce BEFORE adding so a wire `epoch_day` near `i64::MAX` (a corrupt
/// `SetToday`) can't debug-overflow on `+ 3`; the panic would cross the FFI
/// firewall as an error object and brick every later `view()`.
pub(super) fn mon0_weekday(epoch_day: i64) -> i64 {
    (epoch_day.rem_euclid(7) + 3).rem_euclid(7)
}

/// English weekday name for a local epoch day.
pub(super) fn weekday_name(epoch_day: i64) -> &'static str {
    const NAMES: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    NAMES[mon0_weekday(epoch_day) as usize]
}

/// The Monday of local day `day`'s week (saturating, like [`mon0_weekday`]).
pub(super) fn monday_of(day: i64) -> i64 {
    day.saturating_sub(mon0_weekday(day))
}

/// Monday-anchored LOCAL week index for a UTC `observed_at`: the user's real
/// Monday–Sunday week, the one the plan strip is dated against
/// ([`local_day`] + [`mon0_weekday`]).
pub(super) fn local_week(observed_at: i64, offset: i64) -> i64 {
    let d = local_day(observed_at, offset);
    (d - mon0_weekday(d)).div_euclid(7)
}

/// The Monday epoch-day of local week `w`, the inverse of [`local_week`].
pub(super) fn week_monday(w: i64) -> i64 {
    w.saturating_mul(7).saturating_add(4)
}

/// Format a kg figure: rounded ONCE to 0.1 kg, then printed as an integer when
/// that is whole, else with one decimal (e.g. `92.5`; 99.97 → `100`).
pub(super) fn fmt_kg(kg: f64) -> String {
    fmt_rounded(round_tenths(kg))
}

/// Format a pound figure: rounded ONCE to the nearest 0.5 lb (US plates and
/// dumbbells move in 2.5/5 lb, so a tenth of a pound is never a real load),
/// then printed whole or with `.5` (264.55 → `264.5`, 289.97 → `290`).
pub(super) fn fmt_lb(lb: f64) -> String {
    fmt_rounded((lb * 2.0).round() / 2.0)
}

/// An already-rounded figure: integer when whole, else one decimal (`8`,
/// `8.5`).
pub(super) fn fmt_rounded(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{:.0}", v.round())
    } else {
        format!("{v:.1}")
    }
}

/// A weight figure (no unit) in the profile's display unit, rounded once to
/// that unit's display precision ([`fmt_lb`] / [`fmt_kg`]).
pub(super) fn fmt_weight_num(kg: f64, lb: bool) -> String {
    if lb {
        fmt_lb(kg * LB_PER_KG)
    } else {
        fmt_kg(kg)
    }
}

/// The weight-unit suffix a profile renders in: "lb" when `lb`, else "kg".
pub(super) fn weight_suffix(lb: bool) -> &'static str {
    if lb { "lb" } else { "kg" }
}

/// A canonical kg figure (an absolute load OR a load increment — the conversion
/// is linear so both are correct) expressed in the profile's display unit:
/// pounds when `lb`, otherwise the kg value unchanged. Precision is left to the
/// call site.
pub(super) fn weight_num(kg: f64, lb: bool) -> f64 {
    if lb { kg * LB_PER_KG } else { kg }
}

/// Format a weight figure in the profile's unit, rounded once to that unit's
/// display precision (lb: whole or .5; kg: whole or one decimal) plus a spaced
/// unit suffix — e.g. "92.5 kg" / "264.5 lb". Sites that fix a different
/// precision (`{:.0}`, `+{:.1}`, or a glued suffix) use `weight_num` +
/// `weight_suffix` directly to hold that precision.
pub(super) fn fmt_weight(kg: f64, lb: bool) -> String {
    format!("{} {}", fmt_weight_num(kg, lb), weight_suffix(lb))
}

/// The smallest kg figure a weight ratio is taken over (a prior e1RM best):
/// below it the base quotes as "0 kg" and is no baseline, the weight twin of
/// [`MIN_RATIO_BASE_KM`].
pub(super) const MIN_RATIO_BASE_KG: f64 = 0.05;

/// A figure rounded ONCE to one decimal, half away from zero. For distances it
/// is the one rounding behind both units' lines, so a km line and the miles
/// line of the same distance describe the same distance (`{:.1}` on the raw
/// value would round an exact .x5 tie half to EVEN: "5.2 km" beside the
/// "3.3 mi" of 5.3 km).
pub(super) fn round_tenths(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The km distance a line shows at one decimal ([`round_tenths`]), restated
/// in miles at one decimal with a trailing ".0" dropped ("7.5 mi", "5 mi").
pub(super) fn fmt_mi_of_km(km: f64) -> String {
    let mi = round_tenths(km) * 1000.0 / crate::running::MILE_M;
    format!("{} mi", fmt_rounded(round_tenths(mi)))
}

/// The smallest km figure a distance ratio is taken over (a run's 30-day
/// longest for the spike copy and gate, a week-over-week base). Below it the
/// base quotes as "0.0 km" and is no baseline: a corrupt log's subnormal
/// distance (5e-324 km survives `sanitize_f64`) would otherwise divide to
/// "+inf%".
pub(super) const MIN_RATIO_BASE_KM: f64 = 0.05;

/// Whether `km` is a distance ratio base at all (`>= MIN_RATIO_BASE_KM`). The
/// ONE baseline rule of the run card's spike chip (`spike_has_baseline`, its
/// note), the feedback spike gate (`latest_run_spike`) and the weekly report's
/// ramp rows, so no two verdicts on one screen disagree: a 40 m accidental
/// tracker start is no earlier run or week for any of them, exactly like none.
pub(super) fn is_ratio_base_km(km: f64) -> bool {
    km >= MIN_RATIO_BASE_KM
}

/// A km figure as a line shows it in `unit`: "12.3 km" ([`round_tenths`]), or
/// [`fmt_mi_of_km`].
pub(super) fn fmt_km_or_mi(km: f64, unit: DistanceUnit) -> String {
    match unit {
        DistanceUnit::Km => format!("{:.1} km", round_tenths(km)),
        DistanceUnit::Mi => fmt_mi_of_km(km),
    }
}

/// A km figure rounded to a whole number in `unit` ("64 km", "40 mi"): for
/// weekly volumes and thresholds in prose.
pub(super) fn fmt_whole_km_or_mi(km: f64, unit: DistanceUnit) -> String {
    match unit {
        DistanceUnit::Km => format!("{km:.0} km"),
        DistanceUnit::Mi => format!("{:.0} mi", km * 1000.0 / crate::running::MILE_M),
    }
}

/// Human goal label for the program summary.
pub(super) fn human_goal(goal: &Goal) -> String {
    match goal {
        Goal::Strength => "Strength".into(),
        Goal::Hypertrophy => "Hypertrophy".into(),
        Goal::Power => "Power".into(),
        Goal::RunningRace { distance_km } => {
            format!("{} race", race_distance_title(f64::from(*distance_km)))
        }
        Goal::GeneralEndurance => "Endurance".into(),
        Goal::Hybrid => "Hybrid".into(),
    }
}

/// The single rounding rule for every displayed duration: round to the nearest
/// whole second (half away from zero). Non-finite or negative input is 0, so no
/// caller can render "NaN" or a negative clock. The result is `u64`, so a
/// sanitized wire value (|x| ≤ 1e12) never saturates. Rounding happens BEFORE
/// the value is split into h/m/s, so 59.6 s reads "1:00", never "0:60".
pub(super) fn whole_seconds(sec: f64) -> u64 {
    if sec.is_finite() && sec > 0.0 {
        sec.round() as u64
    } else {
        0
    }
}

/// `m:ss` for a pace (seconds per km/mile) or a sub-hour duration; minutes
/// are not wrapped into hours. Rounds per [`whole_seconds`].
pub(super) fn fmt_mss(sec: f64) -> String {
    let s = whole_seconds(sec);
    format!("{}:{:02}", s / 60, s % 60)
}

/// A clock duration: `h:mm:ss` from one hour up, else `m:ss`. Rounds per
/// [`whole_seconds`], so the hour switch is judged on the rounded value.
pub(super) fn fmt_clock(sec: f64) -> String {
    let s = whole_seconds(sec);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Plain mid-sentence name for a race distance in km: the race's name at a
/// standard race distance ([`GoalDistance::named_at_km`]), "1K" at one
/// kilometre (the same tolerance), otherwise "N km" with one decimal only when
/// the distance is not whole.
pub(super) fn race_distance_label(km: f64) -> String {
    if let Some(name) = GoalDistance::named_at_km(km).and_then(GoalDistance::race_name) {
        name.to_string()
    } else if (km - 1.0).abs() < running::NAME_MATCH_TOLERANCE_M / 1000.0 {
        "1K".to_string()
    } else if (km - km.round()).abs() < 0.05 {
        format!("{:.0} km", km.round())
    } else {
        format!("{km:.1} km")
    }
}

/// [`race_distance_label`] for the start of a line or a standalone label
/// ("Half marathon", "Marathon", "10K").
pub(super) fn race_distance_title(km: f64) -> String {
    capitalize_first(&race_distance_label(km))
}

/// `s` with its first character upper-cased ("easy" → "Easy").
pub(super) fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Seconds as whole or half minutes ("2 min", "1.5 min") once they reach
/// `min_whole` minutes, else as seconds ("45 s", "90 s" below 2 min).
pub(super) fn fmt_secs_min(s: u32, min_whole: u32) -> String {
    match (s / 60, s % 60) {
        (m, 0) if m >= min_whole => format!("{m} min"),
        (m, 30) if m >= min_whole => format!("{m}.5 min"),
        _ => format!("{s} s"),
    }
}
