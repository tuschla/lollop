//! Wire → model ingest: the wire sanitizers and the row builders for the
//! logging and profile events `update` applies, so a logged row and its
//! amended replacement are built by the same code.

use crate::app::model::{
    LoggedRun, LoggedSet, Model, SessionReview, history_key, profile_at, sanitize_entry_id,
    set_passes_gate,
};
use crate::app::profile_age::{PROFILE_EPOCH_DAY_RANGE, derive_birthday_age};
use crate::app::reviews::weekly_part;
use crate::app::run_metrics::{in_spike_window, spike_km};
use crate::app::run_targets;
use crate::gps::GpsPoint;
use crate::hypertrophy;
use crate::profile::Profile;
use crate::schema::WorkoutType;

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
    p.training_weekdays = p.chosen_weekdays();
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

/// Clamp every wire float on a submitted review, including the nested
/// execution contexts. A review is replayed from the event log, so an
/// out-of-domain magnitude that reached disk once would otherwise poison
/// `view()` on every launch.
pub(super) fn sanitize_review(r: &mut SessionReview) {
    for v in [
        &mut r.safety.overtraining_signal_weeks,
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

/// Clamp a wire whole-session CR-10 rating to the 0–10 scale.
pub(super) fn sanitize_session_rpe(rpe: Option<u8>) -> Option<u8> {
    rpe.map(|r| r.min(10))
}

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
    /// (`exercise::renamed`), so every view shows one name for the lift.
    pub(super) fn into_logged(self) -> Option<LoggedSet> {
        set_passes_gate(self.weight_kg, self.reps, self.hold_sec).then(|| LoggedSet {
            exercise: match crate::exercise::renamed(&self.exercise) {
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
        session_rpe: sanitize_session_rpe(f.session_rpe),
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
/// of the profile in force at the run's instant (`profile_at`, on the clock
/// the row was logged under), else the wire figure. A pure function of the
/// row and the profile history, never of log order.
pub(super) fn derived_hr_pct_max(model: &Model, run: &LoggedRun) -> f64 {
    if run.avg_hr_bpm.is_none() {
        return run.wire_hr_pct_max;
    }
    let offset = run
        .logged_utc_offset_sec
        .unwrap_or(model.today_utc_offset_sec);
    let profile = profile_at(model, run.observed_at, offset);
    run_targets::logged_hr_pct_max(run.wire_hr_pct_max, run.avg_hr_bpm, profile.as_deref())
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
/// so the raw segment-start indices still align (the `gps` track fns ignore
/// any that are 0, out-of-range, or duplicated).
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

/// The distance (km) and moving time (min) a GPS track will be SAVED with,
/// before it is logged: the tracker's Finish review shows these, so the
/// figures the runner confirms are the ones History then lists. Same ingest
/// sanitizing and `run_metrics::qc_figures` as a logged `LogRunTrack`, and the same
/// "fewer than two usable fixes measures nothing" rule as its card.
#[cfg(any(target_os = "android", feature = "ffi", test))]
pub(crate) fn preview_track_figures(points: Vec<GpsPoint>, segment_starts: &[u32]) -> (f64, f64) {
    let q = crate::app::run_metrics::qc_figures(&sanitize_track(points), segment_starts);
    if q.track.len() < 2 {
        return (0.0, 0.0);
    }
    (q.distance_km, q.moving_min)
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

/// Store `review`'s weekly part as the weekly check-in in force: the newest
/// entry of the weekly history.
pub(super) fn store_weekly(model: &mut Model, review: &SessionReview) {
    model.weekly_history.push(weekly_part(review));
    model.weekly_checkin_cleared = false;
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
    if profile.age_years.is_some_and(Profile::youth_age) {
        profile.health.youth = true;
    }
    // Profile history: an edit supersedes every entry at or after its own
    // [`history_key`] (effective day, then the instant it was made; an
    // undated edit, −∞, the whole history), so a
    // later goal/weekly-set change never re-prescribes an already-shown past
    // day, and an evening edit leaves the morning's in force for the morning.
    model.profile_history.retain(|p| history_key(p) < key);
    model.profile_history.push(profile);
}

/// `SetToday`: re-derive every birthday-carrying profile's age (and with it
/// the under-18 gate) for the new day, so the age never goes stale and the
/// gate lifts on the 18th birthday without a profile edit. The stored values
/// are TODAY's; a past day reads its own day's age through
/// `profile_age::aged_on` (`profile_for_day`), so the birthday never rewrites
/// the days it held.
pub(super) fn refresh_birthday_ages(model: &mut Model) {
    let Some(today) = model.today_epoch_day else {
        return;
    };
    for p in &mut model.profile_history {
        derive_birthday_age(p, today);
    }
}
