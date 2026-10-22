//! Display formatting and wire sanitation helpers: weight units, load grid
//! rounding, local-day/week bucketing, session/goal titles, and ingest clamps.

use super::*;

/// Render an inclusive integer range, collapsing to a single value when the
/// bounds coincide (e.g. `(3, 3) -> "3"`, `(6, 9) -> "6–9"`). Keeps guidance
/// copy from printing degenerate `3–3` spans.
pub(super) fn fmt_u8_range(lo: u8, hi: u8) -> String {
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}–{hi}")
    }
}

/// Render a fraction band as whole percent numbers (no `%` sign), collapsing
/// when both print the same (`(0.5, 0.5) -> "50"`, `(0.4, 0.6) -> "40–60"`).
pub(super) fn fmt_frac_pct_range(lo: f64, hi: f64) -> String {
    let (lo, hi) = (format!("{:.0}", lo * 100.0), format!("{:.0}", hi * 100.0));
    if lo == hi { lo } else { format!("{lo}–{hi}") }
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

/// Monday-anchored LOCAL week index for a UTC `observed_at`. Composes
/// [`local_day`] + [`mon0_weekday`] so the weekly running report judges the
/// running-031 ramp cap and the running-028 two-week flag over the user's real
/// Monday–Sunday training week - the same week the plan strip is dated against.
/// A plain `observed_at.div_euclid(604_800)` bucket would put the boundary at
/// Thursday 00:00 UTC, splitting one Mon–Sun week across two report windows
/// (and, west of UTC, attributing a late-night run to the wrong week entirely).
/// `d - mon0_weekday(d)` is the week's Monday epoch-day, constant for every day
/// in the week, so dividing by 7 yields a stable per-week index.
pub(super) fn local_week(observed_at: i64, offset: i64) -> i64 {
    let d = local_day(observed_at, offset);
    (d - mon0_weekday(d)).div_euclid(7)
}

/// Clamp a wire float to a finite, bounded domain on ingest. `serde_json`
/// already refuses NaN/Inf *tokens*, but a huge FINITE magnitude (e.g. 1e300)
/// can multiply/accumulate to `inf` in `view()`; which then serializes to
/// `null` and breaks a non-null Kotlin decode (a persistent view-brick). NaN
/// maps to 0.0; the ±1e12 bound keeps any downstream product well inside f64
/// range while comfortably covering every real kg / km / min / bpm value.
pub(super) fn sanitize_f64(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(-1.0e12, 1.0e12)
    }
}

/// Plausible calendar window for a wire epoch day on the profile (race / meet
/// / layoff / race-result dates): 1920-01-02 through 2120-01-02. A corrupt
/// value outside it would push week arithmetic toward overflow or make a
/// race "decades away"; it is dropped (`None` = unstated) instead.
pub(super) const PROFILE_EPOCH_DAY_RANGE: std::ops::RangeInclusive<i64> = -18_262..=54_787;

/// Clean the planning fields of a wire `Profile` on `SetProfile` ingest, so a
/// corrupt or hand-edited event can neither poison plan math nor surface an
/// impossible value. Anything implausible becomes "unstated" (`None`), which
/// every consumer already handles as the conservative default:
/// - `lift_days_per_week`: 2..=5, else `None`;
/// - `training_weekdays`: only the seven weekday bits; an empty mask → `None`;
/// - race / meet / layoff-return / birth dates: inside [`PROFILE_EPOCH_DAY_RANGE`];
/// - `recent_race`: distance 0.4–100 km, positive time no faster than
///   2:00/km, an in-range date (an out-of-range date alone is dropped);
/// - `stated_longest_recent_km`: finite, 0–200 km;
/// - `specialization_muscle`: must resolve to a tracked muscle and is stored
///   as its canonical key.
pub(super) fn sanitize_planning_fields(p: &mut Profile) {
    p.lift_days_per_week = p.lift_days_per_week.filter(|d| (2..=5).contains(d));
    p.training_weekdays = p.training_weekdays.map(|m| m & 0x7f).filter(|m| *m != 0);
    let date_ok = |d: &i64| PROFILE_EPOCH_DAY_RANGE.contains(d);
    p.goal_race_epoch_day = p.goal_race_epoch_day.filter(date_ok);
    p.meet_epoch_day = p.meet_epoch_day.filter(date_ok);
    p.layoff_returned_epoch_day = p.layoff_returned_epoch_day.filter(date_ok);
    p.birth_epoch_day = p.birth_epoch_day.filter(date_ok);
    p.recent_race = p.recent_race.take().and_then(|mut r| {
        let plausible = r.distance_km.is_finite()
            && (0.4..=100.0).contains(&r.distance_km)
            && r.time_sec > 0
            && r.time_sec as f64 / r.distance_km >= 120.0;
        r.epoch_day = r.epoch_day.filter(date_ok);
        plausible.then_some(r)
    });
    p.stated_longest_recent_km = p
        .stated_longest_recent_km
        .filter(|km| km.is_finite() && (0.0..=200.0).contains(km));
    p.specialization_muscle = p
        .specialization_muscle
        .take()
        .and_then(|m| hypertrophy::resolve_muscle(&m).map(|lm| lm.muscle.to_string()));
}

/// Clamp a wire rep count to a sane maximum on ingest. A corrupt or malicious
/// event could carry `u32::MAX` reps, which overflows an e1RM product to `inf`
/// and bricks the view; no real set exceeds a few thousand reps.
pub(super) fn sanitize_reps(r: u32) -> u32 {
    r.min(10_000)
}

/// Clamp a wire entry id into the range a Kotlin `Long` (`i64`) can decode: a
/// `u64` above `i64::MAX` round-trips out of range and breaks the shell. Every
/// store and match key in `update` and every compaction identity in `log.rs`
/// goes through this, so replay and compaction agree on which rows an id names.
pub(crate) fn sanitize_entry_id(id: u64) -> u64 {
    id.min(i64::MAX as u64)
}

/// Clamp every wire float on a submitted review, including the nested
/// execution contexts. A review is replayed from the event log, so an
/// out-of-domain magnitude that reached disk once would otherwise poison
/// `view()` on every launch.
pub(super) fn sanitize_review(r: &mut SessionReview) {
    for v in [
        &mut r.overtraining_signal_weeks,
        &mut r.single_session_spike_frac,
        &mut r.easy_frac_time_above_vt1,
        &mut r.positive_split_pct,
        &mut r.weekly_velocity_drop_m_s,
        &mut r.rep_drop_frac,
        &mut r.mcv_delta_m_s,
        &mut r.first_set_rpe_delta,
        &mut r.hrv_single_day_z,
    ]
    .into_iter()
    .flatten()
    {
        *v = sanitize_f64(*v);
    }
    if let Some(d) = r.decoupling.as_mut() {
        d.drift_pct = sanitize_f64(d.drift_pct);
    }
}

/// Round a working load DOWN onto the profile's display-unit plate grid, then
/// return it in kg (the wire unit). kg users get the 2.5 kg grid; lb users the
/// 5 lb grid (2.5 lb below 50 lb, where micro-plates are standard). Rounding
/// DOWN — never nearest — guarantees the rendered load never exceeds the
/// prescribed %1RM, and rounding on the DISPLAY grid means a lb user never sees
/// a fractional-lb load no US plate combination can make (the wire stays kg).
/// The grade still travels with the underlying %1RM claim.
pub(crate) fn round_load_grid(kg: f64, weight_lb: bool) -> f64 {
    if weight_lb {
        let lb = kg * LB_PER_KG;
        let step = if lb < 50.0 { 2.5 } else { 5.0 };
        (lb / step).floor() * step / LB_PER_KG
    } else {
        (kg / 2.5).floor() * 2.5
    }
}

/// Format a kg figure: rounded ONCE to 0.1 kg, then printed as an integer when
/// that is whole, else with one decimal (e.g. `92.5`; 99.97 → `100`).
pub(super) fn fmt_kg(kg: f64) -> String {
    fmt_rounded((kg * 10.0).round() / 10.0)
}

/// Format a pound figure: rounded ONCE to the nearest 0.5 lb (US plates and
/// dumbbells move in 2.5/5 lb, so a tenth of a pound is never a real load),
/// then printed whole or with `.5` (264.55 → `264.5`, 289.97 → `290`).
pub(super) fn fmt_lb(lb: f64) -> String {
    fmt_rounded((lb * 2.0).round() / 2.0)
}

/// An already-rounded figure: integer when whole, else one decimal.
fn fmt_rounded(v: f64) -> String {
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

/// The km distance a line shows at one decimal, restated in miles at one
/// decimal with a trailing ".0" dropped ("7.5 mi", "5 mi"): the `_mi` twin of
/// a km line for a shell that shows miles.
pub(super) fn fmt_mi_of_km(km: f64) -> String {
    let shown_km = (km * 10.0).round() / 10.0;
    let mi = shown_km * 1000.0 / crate::running::MILE_M;
    // One decimal, a half rounded up (never to even).
    let tenths = mi * 10.0;
    let one = if tenths.fract() == 0.5 {
        format!("{:.1}", (tenths + 0.5) / 10.0)
    } else {
        format!("{mi:.1}")
    };
    format!("{} mi", one.strip_suffix(".0").unwrap_or(&one))
}

/// A km figure as a line shows it ("12.3 km"), or its miles twin.
pub(super) fn fmt_km_or_mi(km: f64, mi: bool) -> String {
    if mi {
        fmt_mi_of_km(km)
    } else {
        format!("{km:.1} km")
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
    let label = race_distance_label(km);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => label,
    }
}
