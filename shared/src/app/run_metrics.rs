//! Run-derived metrics shared by the reducer and the view builders: the
//! per-view [`RunFacts`] (realised distance/time per run), the 30-day spike
//! baseline and the QC'd-track record.

use super::*;

/// Widest CTL/ATL window (~2 years): both EWMAs have converged, and the cap
/// bounds the daily loop when a corrupt stamp spreads the runs over millions of
/// days.
pub(super) const MAX_LOAD_DAYS: i64 = 730;

/// Longest logged run, km, the running-040 marathon-optimism input. `None`
/// when no run with a measurable distance exists.
pub(super) fn longest_logged_run_km(facts: &RunFacts) -> Option<f64> {
    let m = facts.km.iter().copied().fold(0.0_f64, f64::max);
    (m > 0.0).then_some(m)
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
    /// When each run started ([`run_started_at`]: its first QC'd fix, else
    /// its stamp minus its duration; 0 when undated). A run belongs to the
    /// local day and week it STARTED in every view and coaching bucket (the
    /// progress chart, the weekly report, the run anchors, the training-load
    /// chain, the feedback and mastery day), the day its card names.
    pub(super) started: Vec<i64>,
    /// Run indices in chronological order (`observed_at`, then `entry_id`,
    /// the `runs` order). Float sums walk this order, never storage order:
    /// an Undo or amend moves a row to the tail, and a rounded total must not
    /// change with it.
    pub(super) chrono: Vec<usize>,
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
        let started = runs
            .iter()
            .zip(&qc)
            .map(|(r, q)| run_started_at(r, q.as_deref()))
            .collect();
        let mut chrono: Vec<usize> = (0..runs.len()).collect();
        chrono.sort_by_key(|&i| (runs[i].observed_at, runs[i].entry_id));
        Self {
            keys,
            qc,
            km,
            minutes,
            started,
            chrono,
        }
    }
}

/// RUN-SPIKE-001: the spike baseline is the longest run in the TRAILING
/// 30-day window ending at `at`, NOT all-time. An old long run before a layoff
/// must not permanently suppress a genuine load spike after the layoff. Runs
/// with `observed_at` in `[at − 30 d, at]` count (a future-dated row is excluded
/// so a back-dated amend can't seed the baseline of an earlier run).
pub(super) const SPIKE_WINDOW_SEC: i64 = 30 * DAY_SEC;

/// Whether a run at `from` lies in the spike window ending at `at` (the
/// trailing 30 days, inclusive of `at` itself).
pub(super) fn in_spike_window(from: i64, at: i64) -> bool {
    from <= at && at.saturating_sub(from) <= SPIKE_WINDOW_SEC
}

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
        .filter(|&(i, (r, _))| Some(i) != exclude && in_spike_window(r.observed_at, at))
        .map(|(_, (_, &k))| k)
        .fold(0.0_f64, f64::max)
}

/// One run's distance as [`spike_baseline_km`] measures it, for the reducer,
/// which holds no [`RunFacts`]: a GPS run through the QC memo.
pub(super) fn spike_km(r: &LoggedRun) -> f64 {
    if r.track.is_empty() {
        r.distance_km
    } else {
        run_qc_keyed(r, run_content_key(r)).distance_km
    }
}

/// A run's QC-gated track plus the two scalars every downstream consumer derives
/// from it, memoized per run by [`run_qc_keyed`].
pub(super) struct QcTrack {
    pub(super) track: Vec<GpsPoint>,
    pub(super) seg_starts: Vec<u32>,
    /// Distance over the QC'd, segment-aware track (km).
    pub(super) distance_km: f64,
    /// Moving time over the QC'd, segment-aware track (minutes).
    pub(super) moving_min: f64,
}
