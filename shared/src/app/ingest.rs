//! Wire → model ingest for the logging and profile events `update` applies.
//! Every wire float is sanitized and every entry id normalized here, once, so
//! a logged row and its amended replacement are built by the same code.

use super::*;

/// A lift set as `LogSet` and `AmendSet` carry it.
pub(super) struct SetFields {
    pub(super) exercise: String,
    pub(super) weight_kg: f64,
    pub(super) reps: u32,
    pub(super) rpe: f64,
    pub(super) observed_at: i64,
    pub(super) entry_id: u64,
    pub(super) hold_sec: Option<u16>,
}

impl SetFields {
    /// The row to store, or `None` when the set is rejected (a no-op, like
    /// other rejected inputs): a negative load is invalid, and a zero-rep set
    /// with no hold carries no e1RM (the e1RM formulas require reps >= 1). A
    /// `NaN` load passes the gate and is sanitized to 0. A set logged under an
    /// earlier catalog label is stored under the current name
    /// (`hypertrophy::renamed`), so every view shows one name for the lift.
    pub(super) fn into_logged(self) -> Option<LoggedSet> {
        crate::log::set_passes_gate(self.weight_kg, self.reps, self.hold_sec).then(|| LoggedSet {
            exercise: match crate::hypertrophy::renamed(&self.exercise) {
                Some(name) => name.to_owned(),
                None => self.exercise,
            },
            weight_kg: sanitize_f64(self.weight_kg),
            reps: sanitize_reps(self.reps),
            rpe: sanitize_f64(self.rpe),
            observed_at: self.observed_at,
            entry_id: sanitize_entry_id(self.entry_id),
            hold_sec: self.hold_sec,
        })
    }
}

/// A run as `LogRun` and `AmendRun` carry it. A GPS run (`LogRunTrack`) has
/// no manual distance or duration; its figures derive from the track.
pub(super) struct RunFields {
    pub(super) distance_km: f64,
    pub(super) duration_min: f64,
    pub(super) hr_pct_max: f64,
    pub(super) longest_recent_km: f64,
    pub(super) observed_at: i64,
    pub(super) entry_id: u64,
    pub(super) workout_type: Option<WorkoutType>,
    pub(super) session_rpe: Option<u8>,
    pub(super) avg_hr_bpm: Option<f64>,
}

/// The run row to store, with no track. `runs` is the history the new row
/// joins (for an amend, without the row it replaces): the spike baseline is
/// floored to the longest run in its trailing 30-day window, so a manual
/// entry gets the same RUN-SPIKE-001 gate a GPS-tracked one does; an explicit
/// caller value (paired 30-day history) still wins when larger.
pub(super) fn logged_run(runs: &[LoggedRun], profile: Option<&Profile>, f: RunFields) -> LoggedRun {
    let prior_longest = ingest_spike_baseline_km(runs, f.observed_at);
    LoggedRun {
        // A negative distance would subtract from `recent_weekly_km`
        // (nulling the measured plan anchor): clamp it out at ingest.
        distance_km: sanitize_f64(f.distance_km).max(0.0),
        duration_min: sanitize_f64(f.duration_min).max(0.0),
        hr_pct_max: sanitize_f64(run_targets::logged_hr_pct_max(
            f.hr_pct_max,
            f.avg_hr_bpm,
            profile,
        )),
        longest_recent_km: sanitize_f64(f.longest_recent_km).max(prior_longest),
        track: Vec::new(),
        track_segment_starts: Vec::new(),
        observed_at: f.observed_at,
        entry_id: sanitize_entry_id(f.entry_id),
        workout_type: f.workout_type,
        session_rpe: f.session_rpe.map(|r| r.min(10)),
        avg_hr_bpm: run_targets::plausible_avg_hr_bpm(f.avg_hr_bpm),
    }
}

/// The wire fixes with every float sanitized. Length and order are preserved,
/// so the raw segment-start indices still align (the core's `segments()`
/// ignores any that are 0, out-of-range, or duplicated).
pub(super) fn sanitize_track(points: Vec<GpsPoint>) -> Vec<GpsPoint> {
    points
        .into_iter()
        .map(|p| GpsPoint {
            lat: sanitize_f64(p.lat),
            lon: sanitize_f64(p.lon),
            observed_at: p.observed_at,
            accuracy_m: if p.accuracy_m.is_finite() {
                p.accuracy_m.clamp(-1.0e12, 1.0e12)
            } else {
                0.0
            },
        })
        .collect()
}

/// The timestamp an amend locates its target row by: the OLD
/// `observed_at_fallback` of a re-dated legacy row, else the new `observed_at`
/// (for logs that predate the fallback field). Matching the OLD row is what
/// keeps a re-dated legacy row from being duplicated.
pub(super) fn amend_match_at(observed_at_fallback: i64, observed_at: i64) -> i64 {
    if observed_at_fallback != 0 {
        observed_at_fallback
    } else {
        observed_at
    }
}

/// `run`'s track cut to the fixes whose per-point `observed_at` lies in
/// `[keep_from_sec, keep_until_sec]`, with the segment starts remapped, or
/// `None` unless that is a REAL trim: at least 2 surviving fixes and at least
/// one dropped.
///
/// Boundaries follow `qc_track`'s pending-boundary rule: a boundary whose fix
/// is cut shifts to the first survivor of its segment; a fully-cut segment
/// drops its boundary.
pub(super) fn trim_track(
    run: &LoggedRun,
    keep_from_sec: i64,
    keep_until_sec: i64,
) -> Option<(Vec<GpsPoint>, Vec<u32>)> {
    let old_len = run.track.len();
    let mut new_track: Vec<GpsPoint> = Vec::with_capacity(old_len);
    let mut new_starts: Vec<u32> = Vec::new();
    let mut pending_boundary = false;
    let starts: std::collections::HashSet<u32> = run.track_segment_starts.iter().copied().collect();
    for (idx, p) in run.track.iter().enumerate() {
        if starts.contains(&(idx as u32)) {
            pending_boundary = true;
        }
        if p.observed_at < keep_from_sec || p.observed_at > keep_until_sec {
            continue;
        }
        if pending_boundary && !new_track.is_empty() {
            new_starts.push(new_track.len() as u32);
        }
        pending_boundary = false;
        new_track.push(*p);
    }
    (new_track.len() >= 2 && new_track.len() < old_len).then_some((new_track, new_starts))
}

/// `SetProfile`: sanitize the profile, derive what the core owns, and record
/// it in the profile history.
pub(super) fn apply_profile(model: &mut Model, mut profile: Profile) {
    // Sanitize every wire float BEFORE any branch reads it, so a poisoned
    // `NaN`/`1e300` can't render "inf km" or slip past a gate.
    // `running_km_per_week` also floors at 0 (a negative weekly volume is
    // nonsense and would break the plan's volume math).
    profile.running_km_per_week = sanitize_f64(profile.running_km_per_week).max(0.0);
    for v in [
        &mut profile.env_temp_c,
        &mut profile.env_altitude_m,
        &mut profile.weeks_off,
        &mut profile.bodyweight_kg,
        &mut profile.age_years,
        &mut profile.resting_hr_bpm,
        &mut profile.measured_hr_max,
    ]
    .into_iter()
    .flatten()
    {
        *v = sanitize_f64(*v);
    }
    // running-041: a race only anchors paces while it is recent, so a race the
    // shell sent undated is dated when it is first stated (before sanitizing,
    // so a dropped out-of-range date stays undated). A re-sent unchanged race
    // keeps the date it already has, so re-saving the profile never makes an
    // old race look fresh.
    if let Some(race) = profile.recent_race.as_mut()
        && race.epoch_day.is_none()
    {
        let previous = model
            .profile
            .as_ref()
            .and_then(|p| p.recent_race.as_ref())
            .filter(|p| p.distance_km == race.distance_km && p.time_sec == race.time_sec);
        race.epoch_day = match previous {
            Some(p) => p.epoch_day,
            None => profile.effective_from_epoch_day.or(model.today_epoch_day),
        };
    }
    sanitize_planning_fields(&mut profile);
    if !profile.running_km_per_week_set {
        profile.running_km_per_week = 0.0;
    }
    if let Some(today) = model.today_epoch_day {
        derive_birthday_age(&mut profile, today);
    }
    // HARD RULE 3: auto-derive the pediatric gate from age alone so it fires
    // even when the health screen is skipped. The KB gives no numeric cutoff
    // for "child/adolescent" (safety-011); 18 y is used. Never CLEARS a flag
    // the shell set explicitly. Age is optional: a profile with NO age is
    // treated as an adult and is not gated (owner decision 2026-10-17, recorded
    // in planning/design/user-decisions.md; KB onboard-050 note); it simply has
    // no age-derived values (Tanaka HRmax, masters 65+). Runs AFTER the
    // sanitize above, so a `NaN` age becomes 0 → gate fires (conservative),
    // not a silently-skipped `NaN < 18.0` (false).
    if profile.age_years.is_some_and(|a| a < YOUTH_AGE_YEARS) {
        profile.health.youth = true;
    }
    // Profile history: an edit effective on day E supersedes every entry
    // already effective on or after E (a None-dated edit, −∞, replaces the
    // whole history), so a later goal/weekly-set change never re-prescribes an
    // already-shown past day. `model.profile` stays the latest applied.
    let e = eff_key(profile.effective_from_epoch_day);
    model
        .profile_history
        .retain(|p| eff_key(p.effective_from_epoch_day) < e);
    model.profile_history.push(profile.clone());
    model.profile = Some(profile);
}

/// Age below which the pediatric gate is set (safety-011 names no numeric
/// cutoff for "child/adolescent"; 18 y is the core's choice).
const YOUTH_AGE_YEARS: f64 = 18.0;

/// `SetToday`: re-derive every birthday-carrying profile's age (and with it
/// the under-18 gate) for the new day, so the age never goes stale and the
/// gate lifts on the 18th birthday without a profile edit.
pub(super) fn refresh_birthday_ages(model: &mut Model) {
    let Some(today) = model.today_epoch_day else {
        return;
    };
    for p in model
        .profile
        .iter_mut()
        .chain(model.profile_history.iter_mut())
    {
        derive_birthday_age(p, today);
    }
}

/// With a birthday, `age_years` is the whole calendar years lived by `today`
/// and the youth flag is exactly "under 18 today": the birthday is the
/// authoritative age source, so the core owns both. A future birthday derives
/// age 0 (gate on, the conservative reading).
fn derive_birthday_age(p: &mut Profile, today: i64) {
    if let Some(birth) = p.birth_epoch_day {
        let age = whole_years_between(birth, today);
        p.age_years = Some(age as f64);
        p.health.youth = (age as f64) < YOUTH_AGE_YEARS;
    }
}

/// Completed calendar years from `birth` to `today` (both local epoch days).
/// A 29 February birthday completes its year on 1 March in common years.
/// `today` is clamped to the profile date window so a corrupt clock can't
/// overflow the calendar math.
pub(super) fn whole_years_between(birth: i64, today: i64) -> i64 {
    let range = super::format::PROFILE_EPOCH_DAY_RANGE;
    let today = today.clamp(*range.start(), *range.end());
    if today <= birth {
        return 0;
    }
    let (by, bm, bd) = crate::running::civil_from_days(birth);
    let (ty, tm, td) = crate::running::civil_from_days(today);
    ty - by - i64::from((tm, td) < (bm, bd))
}
