//! Display formatting helpers: weight units, load grid rounding,
//! local-day/week bucketing, session/goal titles.

use super::*;

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

/// Round a working load DOWN onto the profile's display-unit plate grid, then
/// return it in kg (the wire unit). kg users get the 2.5 kg grid; lb users the
/// 5 lb grid (2.5 lb below 50 lb, where micro-plates are standard). Rounding
/// DOWN — never nearest — guarantees the rendered load never exceeds the
/// prescribed %1RM, and rounding on the DISPLAY grid means a lb user never sees
/// a fractional-lb load no US plate combination can make (the wire stays kg).
/// The grade still travels with the underlying %1RM claim.
///
/// A load already ON the grid stays put: a lb entry is stored as
/// `lb / LB_PER_KG` and an e1RM × %1RM product carries float dust, so
/// `lb / step` can land at 42.99999… and a bare floor would drop a whole plate.
/// [`LOAD_GRID_EPS`] absorbs that dust; this is the ONE place it lives, so
/// every caller passes its raw load (an f32-sourced one via [`f32_load_ceil`]).
pub(crate) fn round_load_grid(kg: f64, weight_lb: bool) -> f64 {
    let floor_steps = |x: f64, step: f64| (x / step + LOAD_GRID_EPS).floor() * step;
    if weight_lb {
        let lb = kg * LB_PER_KG;
        let step = if lb < 50.0 { 2.5 } else { 5.0 };
        floor_steps(lb, step) / LB_PER_KG
    } else {
        floor_steps(kg, 2.5)
    }
}

/// Float-dust tolerance of [`round_load_grid`], in grid steps: far above the
/// ~1e-13-step error of an f64 kg↔lb round trip or an e1RM × %1RM × derate
/// product, far below any load difference (2.5e-10 kg / 5e-10 lb overshoot).
const LOAD_GRID_EPS: f64 = 1e-10;

/// A kg load that came off an f32 wire field (`LiftPrescription::load_kg`),
/// raised by its own f32 precision (half an f32 ulp, at most 2^-24 relative;
/// apply it before any factor) before it is put on the grid: the f32 cast can
/// leave an on-grid lb load ~2e-6 plate steps under its grid point, beyond
/// [`LOAD_GRID_EPS`], and the f32 cannot tell the two apart anyway.
pub(crate) fn f32_load_ceil(kg: f64) -> f64 {
    kg + kg.abs() * f64::from(f32::EPSILON) / 2.0
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

/// Pounds per kilogram. NUMERIC wire/view fields stay in kg (see the
/// `weight_kg`/`e1rm_kg`/`load_kg` field docs and `Profile::weight_lb`); ONLY
/// human-facing display strings convert, and only through the helpers below so
/// the factor and the "kg"/"lb" suffix can never drift per call site.
pub(super) const LB_PER_KG: f64 = 2.2046226218;

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
/// is the one rounding behind both the km line and its miles twin, so the pair
/// always describe the same distance (`{:.1}` on the raw value would round an
/// exact .x5 tie half to EVEN: "5.2 km" beside the "3.3 mi" of 5.3 km).
pub(super) fn round_tenths(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// The km distance a line shows at one decimal ([`round_tenths`]), restated
/// in miles at one decimal with a trailing ".0" dropped ("7.5 mi", "5 mi"): the
/// `_mi` twin of a km line for a shell that shows miles.
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

/// A km figure as a line shows it ("12.3 km", [`round_tenths`]), or its
/// miles twin.
pub(super) fn fmt_km_or_mi(km: f64, mi: bool) -> String {
    if mi {
        fmt_mi_of_km(km)
    } else {
        format!("{:.1} km", round_tenths(km))
    }
}

/// A km figure rounded to a whole number with its unit ("64 km"), or restated
/// in whole miles ("40 mi"): for weekly volumes and thresholds in prose.
pub(super) fn fmt_whole_km_or_mi(km: f64, mi: bool) -> String {
    if mi {
        format!("{:.0} mi", km * 1000.0 / crate::running::MILE_M)
    } else {
        format!("{km:.0} km")
    }
}

/// Human title for a session type ("Heavy day", "Long run", "Rest").
pub(super) fn session_title(st: SessionType) -> String {
    match st {
        SessionType::Lift(LiftSessionType::MaxEffort) => "Heavy day",
        SessionType::Lift(LiftSessionType::DynamicEffort) => "Power day",
        SessionType::Lift(LiftSessionType::Repetition) => "Volume day",
        SessionType::Lift(LiftSessionType::Accessory) => "Accessory",
        SessionType::Lift(LiftSessionType::Calisthenics) => "Calisthenics",
        SessionType::Lift(LiftSessionType::FullBody) => "Strength day",
        SessionType::Run(RunSessionType::LongRun) => "Long run",
        SessionType::Run(RunSessionType::Tempo) => "Tempo run",
        SessionType::Run(RunSessionType::Interval) => "Intervals",
        SessionType::Run(RunSessionType::Recovery) => "Recovery run",
        SessionType::Run(RunSessionType::RacePace) => "Race-pace run",
        SessionType::Run(RunSessionType::Repetition) => "Repetitions",
        SessionType::Run(RunSessionType::Strides) => "Strides",
        SessionType::Run(RunSessionType::Hills) => "Hill repeats",
        SessionType::Run(RunSessionType::Easy) => "Easy run",
        SessionType::Rest => "Rest",
    }
    .to_string()
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

/// Plain mid-sentence name for a race distance in km: "marathon",
/// "half marathon", "10K", "5K", "1K" at their standard distances (within
/// 50 m, so the common 42.2 / 21.1 roundings match), otherwise "N km" with one
/// decimal only when the distance is not whole.
pub(super) fn race_distance_label(km: f64) -> String {
    let near = |x: f64| (km - x).abs() < 0.05;
    if near(42.195) {
        "marathon".to_string()
    } else if near(21.0975) {
        "half marathon".to_string()
    } else if near(10.0) {
        "10K".to_string()
    } else if near(5.0) {
        "5K".to_string()
    } else if near(1.0) {
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
