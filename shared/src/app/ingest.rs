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
    /// The device's UTC offset when the run was logged (wire `utc_offset_sec`;
    /// `None` on a legacy line and on an amend, which keeps its row's).
    pub(super) utc_offset_sec: Option<i64>,
}

/// A sanitized wire float clamped to `>= +0.0`. `f64::max` may keep a `-0.0`
/// input as `-0.0` (the sign of a zero result is unspecified across targets),
/// so the `+ 0.0` turns it into `+0.0` before it reaches the view.
fn non_negative(v: f64) -> f64 {
    sanitize_f64(v).max(0.0) + 0.0
}

/// The run row as the wire states it, with no track. Its two figures that read
/// other state, the typed-bpm %HRmax and the card spike baseline, start at the
/// wire values; [`store_run`] derives them.
pub(super) fn logged_run(f: RunFields) -> LoggedRun {
    let hr_pct_max = sanitize_f64(f.hr_pct_max);
    let longest_recent_km = non_negative(f.longest_recent_km);
    LoggedRun {
        // A negative distance would subtract from `recent_weekly_km`
        // (nulling the measured plan anchor): clamp it out at ingest.
        distance_km: non_negative(f.distance_km),
        duration_min: non_negative(f.duration_min),
        hr_pct_max,
        wire_hr_pct_max: hr_pct_max,
        longest_recent_km,
        wire_longest_recent_km: longest_recent_km,
        track: Vec::new(),
        logged_utc_offset_sec: f.utc_offset_sec,
        hr_pinned: false,
        track_segment_starts: Vec::new(),
        observed_at: f.observed_at,
        entry_id: sanitize_entry_id(f.entry_id),
        workout_type: f.workout_type,
        session_rpe: f.session_rpe.map(|r| r.min(10)),
        avg_hr_bpm: run_targets::plausible_avg_hr_bpm(f.avg_hr_bpm),
    }
}

/// Store a new run row (or an amend's replacement of `replaced`, already
/// removed, which keeps the original row's logged UTC offset) with its %HRmax
/// derived, and re-derive the spike baselines its arrival and the old row's
/// departure can move.
pub(super) fn store_run(model: &mut Model, mut run: LoggedRun, replaced: Option<&LoggedRun>) {
    if let Some(old) = replaced {
        run.logged_utc_offset_sec = old.logged_utc_offset_sec;
    }
    run.hr_pct_max = derived_hr_pct_max(model, &run);
    let at = run.observed_at;
    model.runs.push(run);
    match replaced {
        Some(old) => refresh_spike_baselines(&mut model.runs, &[old.observed_at, at]),
        None => refresh_spike_baselines(&mut model.runs, &[at]),
    }
}

/// A run's stored %HRmax: a plausible typed bpm converted against the HRmax
/// of the profile in force at the run's instant on the clock it was logged
/// under (`profile_at` with `logged_utc_offset_sec`, else the current offset
/// for a legacy row: a birthday's age is that local day's, and an edit made
/// later that day is not yet in force), else the wire figure. A pure function
/// of the row and the profile history, never of log order or of where the
/// viewer is now: a full replay, a compacted one (superseded profiles and
/// days dropped) and the shell's delete and Undo (which re-sends the row's
/// offset) all store the same figure, and a later time-zone change moves none.
pub(super) fn derived_hr_pct_max(model: &Model, run: &LoggedRun) -> f64 {
    if run.avg_hr_bpm.is_none() {
        return run.wire_hr_pct_max;
    }
    let offset = run
        .logged_utc_offset_sec
        .unwrap_or(model.today_utc_offset_sec);
    let profile = profile_at(model, run.observed_at, offset);
    sanitize_f64(run_targets::logged_hr_pct_max(
        run.wire_hr_pct_max,
        run.avg_hr_bpm,
        profile.as_deref(),
    ))
}

/// Re-derive every typed-bpm run's %HRmax ([`derived_hr_pct_max`]) after the
/// profile history or the clock's UTC offset (which a legacy row without a
/// stamped offset reads) changed. A row pinned by a legacy undated edit keeps
/// its figure.
pub(super) fn refresh_run_hr(model: &mut Model) {
    let derived: Vec<(usize, f64)> = model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.avg_hr_bpm.is_some() && !r.hr_pinned)
        .map(|(i, r)| (i, derived_hr_pct_max(model, r)))
        .collect();
    for (i, pct) in derived {
        model.runs[i].hr_pct_max = pct;
    }
}

/// Re-derive the card spike baseline of every run a change at the stamps in
/// `changed` can move. A run's baseline is the longest OTHER run in its
/// trailing spike window ([`in_spike_window`]), floored by the caller-supplied
/// paired-history value, so a manual entry gets the same RUN-SPIKE-001 chip a
/// GPS-tracked one does; only runs dated in `[t, t + window]` of a changed
/// stamp `t` can move. It is a pure function of the surviving rows, so a
/// replay, a compacted replay and a delete + Undo all show the same chip.
pub(super) fn refresh_spike_baselines(runs: &mut [LoggedRun], changed: &[i64]) {
    let affected: Vec<usize> = (0..runs.len())
        .filter(|&i| {
            changed
                .iter()
                .any(|&t| in_spike_window(t, runs[i].observed_at))
        })
        .collect();
    if affected.is_empty() {
        return;
    }
    // Each run in a window is measured once (a GPS run through the QC memo).
    let mut km: Vec<Option<f64>> = vec![None; runs.len()];
    let mut baselines = Vec::with_capacity(affected.len());
    for &i in &affected {
        let at = runs[i].observed_at;
        let mut longest = 0.0_f64;
        for (j, r) in runs.iter().enumerate() {
            if j != i && in_spike_window(r.observed_at, at) {
                longest = longest.max(*km[j].get_or_insert_with(|| spike_km(r)));
            }
        }
        baselines.push((i, runs[i].wire_longest_recent_km.max(longest)));
    }
    for (i, baseline) in baselines {
        runs[i].longest_recent_km = baseline;
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
            // NaN reads as "no accuracy reported"; ±inf clamps like any huge
            // value, so an infinitely bad fix fails the accuracy gate.
            accuracy_m: if p.accuracy_m.is_nan() {
                0.0
            } else {
                p.accuracy_m.clamp(-1.0e12, 1.0e12)
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

/// Store `review`'s weekly part as the weekly check-in in force and append it
/// to the weekly history.
pub(super) fn store_weekly(model: &mut Model, review: &SessionReview) {
    let weekly = weekly_part(review);
    model.weekly_history.push(weekly.clone());
    model.weekly_checkin = Some(weekly);
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
    profile.running_km_per_week = non_negative(profile.running_km_per_week);
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
    // A legacy undated edit applies to logged runs from its position in the
    // log: every run already logged keeps the %HRmax it has now (it replaces
    // the whole history below, so re-deriving would re-convert all of them).
    if profile.effective_from_epoch_day.is_none() {
        for run in &mut model.runs {
            run.hr_pinned = true;
        }
    }
    let key = history_key(&profile);
    // running-041: an undated race takes the date of the same race on the
    // previous history entry, else this edit's effective day. Read before
    // sanitizing (a dropped out-of-range date stays undated); an undated edit
    // leaves the race undated.
    if let Some(race) = profile.recent_race.as_mut()
        && race.epoch_day.is_none()
    {
        let previous = model
            .profile_history
            .iter()
            .rev()
            .find(|p| history_key(p) < key)
            .and_then(|p| p.recent_race.as_ref())
            .filter(|p| p.distance_km == race.distance_km && p.time_sec == race.time_sec);
        race.epoch_day = previous
            .and_then(|p| p.epoch_day)
            .or(profile.effective_from_epoch_day);
    }
    sanitize_planning_fields(&mut profile);
    if !profile.running_km_per_week_set {
        profile.running_km_per_week = 0.0;
    }
    if let Some(today) = model.today_epoch_day {
        derive_birthday_age(&mut profile, today);
    }
    // HARD RULE 3: derive the pediatric gate from age (18 y; safety-011 gives no
    // cutoff). No age = adult, not gated (KB onboard-050). Runs after
    // sanitizing, so a NaN age (0) gates.
    if profile.age_years.is_some_and(|a| a < YOUTH_AGE_YEARS) {
        profile.health.youth = true;
    }
    // Profile history: an edit supersedes every entry at or after its own
    // [`history_key`] (effective day, then the instant it was made; an
    // undated edit, −∞, the whole history), so a
    // later goal/weekly-set change never re-prescribes an already-shown past
    // day, and an evening edit leaves the morning's in force for the morning.
    // `model.profile` stays the latest applied.
    model.profile_history.retain(|p| history_key(p) < key);
    model.profile_history.push(profile.clone());
    model.profile = Some(profile);
}

/// Age below which the pediatric gate is set (safety-011 names no numeric
/// cutoff for "child/adolescent"; 18 y is the core's choice).
const YOUTH_AGE_YEARS: f64 = 18.0;

/// `SetToday`: re-derive every birthday-carrying profile's age (and with it
/// the under-18 gate) for the new day, so the age never goes stale and the
/// gate lifts on the 18th birthday without a profile edit. The stored values
/// are TODAY's; a past day reads its own day's age through [`aged_on`]
/// (`profile_for_day`), so the birthday never rewrites the days it held.
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

/// `p` as of local day `day`: a birthday-carrying profile's age and under-18
/// gate are those of `day`, not of today (SAFE-PEDS-001: a day the gate held
/// stays held after the 18th birthday). Borrowed unless the day's derived
/// values differ from the stored ones (only across a birthday).
pub(super) fn aged_on(p: &Profile, day: i64) -> std::borrow::Cow<'_, Profile> {
    let Some(birth) = p.birth_epoch_day else {
        return std::borrow::Cow::Borrowed(p);
    };
    let age = whole_years_between(birth, day) as f64;
    if p.age_years == Some(age) && p.health.youth == (age < YOUTH_AGE_YEARS) {
        return std::borrow::Cow::Borrowed(p);
    }
    let mut q = p.clone();
    derive_birthday_age(&mut q, day);
    std::borrow::Cow::Owned(q)
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
    let (by, bm, bd) = crate::civil_from_days(birth);
    let (ty, tm, td) = crate::civil_from_days(today);
    ty - by - i64::from((tm, td) < (bm, bd))
}
