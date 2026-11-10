//! Run-derived metrics shared by the reducer and the view builders: the
//! per-view [`RunFacts`] (realised distance/time per run), the 30-day spike
//! baseline, the run content key and the memoized QC'd-track record.

use crate::app::memo::{Memo, RUN_MEMO_CAP, memoized};
use crate::app::model::LoggedRun;
use crate::day::DAY_SEC;
use crate::gps::{self, GpsPoint};
use crate::running;

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

/// Whether a run at `from` lies in the RUN-SPIKE-001 baseline window ending at
/// `at`: the trailing [`running::SPIKE_WINDOW_DAYS`], inclusive of `at`
/// itself. A future-dated row is excluded so a back-dated amend can't seed the
/// baseline of an earlier run.
pub(super) fn in_spike_window(from: i64, at: i64) -> bool {
    from <= at && at.saturating_sub(from) <= running::SPIKE_WINDOW_DAYS * DAY_SEC
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

/// A content fingerprint of a logged run, every field the derived view depends
/// on. Cheap integer hashing (bit patterns for the floats) versus the trig +
/// string allocation the cached derivation avoids. Collision probability is
/// ~n²/2⁶⁴, negligible for a personal history; a collision would serve another
/// run's QC'd distance, which feeds the spike gate and the run anchors.
pub(in crate::app) fn run_content_key(r: &LoggedRun) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    r.entry_id.hash(&mut h);
    r.observed_at.hash(&mut h);
    r.distance_km.to_bits().hash(&mut h);
    r.duration_min.to_bits().hash(&mut h);
    r.hr_pct_max.to_bits().hash(&mut h);
    r.longest_recent_km.to_bits().hash(&mut h);
    r.workout_type.hash(&mut h);
    r.session_rpe.hash(&mut h);
    r.avg_hr_bpm.map(f64::to_bits).hash(&mut h);
    r.logged_utc_offset_sec.hash(&mut h);
    r.track_segment_starts.hash(&mut h);
    for p in &r.track {
        p.lat.to_bits().hash(&mut h);
        p.lon.to_bits().hash(&mut h);
        p.observed_at.hash(&mut h);
        p.accuracy_m.to_bits().hash(&mut h);
    }
    h.finish()
}

thread_local! {
    /// The File 07 GPS QC pass ([`gps::qc_track`], an O(points) haversine scan) plus
    /// the distance and moving-time derivations, keyed by the run's content
    /// fingerprint like [`RUN_VIEW_CACHE`].
    pub(in crate::app) static QC_TRACK_CACHE: std::cell::RefCell<Memo<u64, std::rc::Rc<QcTrack>>> =
        std::cell::RefCell::new(Memo::new(RUN_MEMO_CAP));
}

/// The QC'd track + derived distance/moving-time for one logged run whose
/// content key the caller already holds, memoized.
pub(in crate::app) fn run_qc_keyed(r: &LoggedRun, key: u64) -> std::rc::Rc<QcTrack> {
    memoized(&QC_TRACK_CACHE, key, || {
        std::rc::Rc::new(qc_figures(&r.track, &r.track_segment_starts))
    })
}

/// The one derivation of a track's of-record figures: [`gps::qc_track`], then
/// distance and moving time over the same legs.
pub(super) fn qc_figures(points: &[GpsPoint], segment_starts: &[u32]) -> QcTrack {
    let qc = gps::qc_track(points, segment_starts);
    let distance_km = gps::track_distance_km(&qc.track, &qc.seg_starts);
    let moving_min = gps::moving_duration_min(&qc);
    QcTrack {
        track: qc.track,
        seg_starts: qc.seg_starts,
        distance_km,
        moving_min,
    }
}

/// [`RunResultView::started_at`]: a GPS run's first QC'd fix (its first raw
/// fix when fewer than two survived QC, the card's unmeasurable case), a
/// hand-entered run's stamp minus its duration. Never after `observed_at` (a
/// corrupt fix time cannot date a run's start past its end); 0 stays 0
/// (undated). The run's day everywhere a view groups runs by day for the shell
/// (the card's title and date, the progress weeks), so they agree.
pub(in crate::app) fn run_started_at(r: &LoggedRun, qc: Option<&QcTrack>) -> i64 {
    if r.observed_at == 0 {
        return 0;
    }
    let qc_track = qc.map_or(&[][..], |q| &q.track[..]);
    let qc_track = if qc_track.len() >= 2 { qc_track } else { &[] };
    let start = match qc_track.first().or(r.track.first()) {
        Some(p) => p.observed_at,
        None => {
            let secs = r.duration_min * 60.0;
            // A non-finite or negative duration (corrupt log) has no length;
            // `as` saturates a huge one.
            let secs = if secs.is_finite() && secs > 0.0 {
                secs.round() as i64
            } else {
                0
            };
            r.observed_at.saturating_sub(secs)
        }
    };
    start.min(r.observed_at)
}
