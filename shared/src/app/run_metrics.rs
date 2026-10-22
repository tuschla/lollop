//! Run-derived metrics shared by the reducer and the view builders: the
//! per-view [`RunFacts`] (realised distance/time per run), the 30-day spike
//! baseline, the dated-run flattening type and the QC'd-track record.

use super::*;

/// Widest CTL/ATL accumulation window, in days (~2 years). The CTL (τ=42 d) and
/// ATL (τ=7 d) EWMAs settle within a few time-constants, so 730 days is far past
/// convergence for both while bounding the daily chain. Without this cap the
/// `for day in first..=last` loop runs one iteration per epoch-day between the
/// earliest and latest counted run: a single corrupt or mis-unit'd `observed_at`
/// (a millisecond stamp ~1.7e12 beside a seconds one ⇒ ~19.7M days, or a line
/// near `i64::MAX`) would spin `view()` into a multi-million-iteration hang
/// (HIGH bug). `view()` runs on every event and every launch replay, so one bad
/// persisted run could wedge the app permanently.
pub(super) const MAX_LOAD_DAYS: i64 = 730;

/// Longest logged run, km, the running-040 marathon-optimism input. `None`
/// when no run with a measurable distance exists.
pub(super) fn longest_logged_run_km(facts: &RunFacts) -> Option<f64> {
    let m = facts.km.iter().copied().fold(0.0_f64, f64::max);
    (m > 0.0).then_some(m)
}

/// One dated run flattened for the weekly-volume system and the load chain.
pub(super) struct DatedRun {
    pub(super) observed_at: i64,
    pub(super) km: f64,
    pub(super) minutes: f64,
    /// 3-zone classification of the average %HRmax; `None` without HR.
    pub(super) zone: Option<crate::schema::ThreeZone>,
}

/// Per-view facts about `model.runs`, index-aligned with it: each run's content
/// key, its memoized QC'd track (GPS runs only) and its realised distance and
/// moving time (QC'd track when present, else the hand-entered scalars). Built
/// once per `view()` so no consumer re-hashes or re-walks a track.
pub(super) struct RunFacts {
    pub(super) keys: Vec<u64>,
    pub(super) qc: Vec<Option<std::rc::Rc<QcTrack>>>,
    pub(super) km: Vec<f64>,
    pub(super) minutes: Vec<f64>,
}

impl RunFacts {
    pub(super) fn new(runs: &[LoggedRun]) -> Self {
        let keys: Vec<u64> = runs.iter().map(run_content_key).collect();
        let qc: Vec<Option<std::rc::Rc<QcTrack>>> = runs
            .iter()
            .zip(&keys)
            .map(|(r, &k)| (!r.track.is_empty()).then(|| run_qc_keyed(r, k)))
            .collect();
        let km = runs
            .iter()
            .zip(&qc)
            .map(|(r, q)| q.as_ref().map_or(r.distance_km, |q| q.distance_km))
            .collect();
        let minutes = runs
            .iter()
            .zip(&qc)
            .map(|(r, q)| q.as_ref().map_or(r.duration_min, |q| q.moving_min))
            .collect();
        Self {
            keys,
            qc,
            km,
            minutes,
        }
    }
}

/// RUN-SPIKE-001: the spike baseline is the longest run in the TRAILING
/// 30-day window ending at `at`, NOT all-time. An old long run before a layoff
/// must not permanently suppress a genuine load spike after the layoff. Runs
/// with `observed_at` in `[at − 30 d, at]` count (a future-dated row is excluded
/// so a back-dated amend can't seed the baseline of an earlier run).
pub(super) const SPIKE_WINDOW_SEC: i64 = 30 * DAY_SEC;

/// The longest run (km, from `km` aligned with `runs`) in the spike window
/// ending at `at`, skipping index `exclude` (the run being judged).
pub(super) fn spike_baseline_km(
    runs: &[LoggedRun],
    km: &[f64],
    at: i64,
    exclude: Option<usize>,
) -> f64 {
    runs.iter()
        .zip(km)
        .enumerate()
        .filter(|&(i, (r, _))| {
            Some(i) != exclude
                && r.observed_at <= at
                && at.saturating_sub(r.observed_at) <= SPIKE_WINDOW_SEC
        })
        .map(|(_, (_, &k))| k)
        .fold(0.0_f64, f64::max)
}

/// [`spike_baseline_km`] for the reducer, which holds no [`RunFacts`]: only the
/// runs inside the window are measured, each through the QC memo.
pub(super) fn ingest_spike_baseline_km(runs: &[LoggedRun], at: i64) -> f64 {
    runs.iter()
        .filter(|r| r.observed_at <= at && at.saturating_sub(r.observed_at) <= SPIKE_WINDOW_SEC)
        .map(|r| {
            if r.track.is_empty() {
                r.distance_km
            } else {
                run_qc_keyed(r, run_content_key(r)).distance_km
            }
        })
        .fold(0.0_f64, f64::max)
}

/// A run's QC-gated track plus the two scalars every downstream consumer derives
/// from it, memoized per run by [`run_qc_keyed`].
pub(super) struct QcTrack {
    pub(super) track: Vec<GpsPoint>,
    pub(super) dropped: u32,
    pub(super) seg_starts: Vec<u32>,
    /// Distance over the QC'd, segment-aware track (km).
    pub(super) distance_km: f64,
    /// Moving time over the QC'd, segment-aware track (minutes).
    pub(super) moving_min: f64,
}
