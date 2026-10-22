//! Run progress over time and the post-memo run-card annotation.

use super::*;

/// Standard best-effort distances: (search metres, wire metres). Labels come
/// from `race_distance_title` / `race_distance_label`.
const BEST_EFFORT_DISTANCES: [(f64, u32); 4] = [
    (1000.0, 1000),
    (5000.0, 5000),
    (10_000.0, 10_000),
    (21_097.5, 21_097),
];

/// A hand-logged run counts as a whole-distance effort when its distance is
/// within this band of the standard distance (a 5.0–5.15 km run is a 5K); the
/// time is scaled to the exact distance at the run's own average pace.
const WHOLE_RUN_MATCH: (f64, f64) = (0.995, 1.03);

/// One run's best effort per standard distance: `(seconds, from_split)`.
type RunBests = [Option<(f64, bool)>; BEST_EFFORT_DISTANCES.len()];

thread_local! {
    /// Per-run best efforts keyed by the run's content fingerprint: a GPS
    /// window scan is O(points) per distance, and a run is immutable once
    /// written.
    pub(super) static BEST_EFFORT_CACHE: std::cell::RefCell<Memo<u64, RunBests>> =
        std::cell::RefCell::new(Memo::new(RUN_MEMO_CAP));
}

/// The MOVING legs of a QC'd track as `(metres, seconds)`, segment-aware: the
/// pause-bridge leg between recording segments is never formed and stopped
/// legs (<0.5 m/s, File 07 auto-pause) are skipped, matching moving time.
fn moving_legs(track: &[GpsPoint], segment_starts: &[u32]) -> Vec<(f64, f64)> {
    let mut legs = Vec::with_capacity(track.len());
    for seg in running::segments(track, segment_starts) {
        for w in seg.windows(2) {
            if let Some((dt, d)) = running::leg(w[0], w[1]) {
                legs.push((d, dt));
            }
        }
    }
    legs
}

/// Fastest time (seconds) to cover `target_m` over contiguous legs. The window
/// start is interpolated inside its first leg (even pace within one leg), so
/// the effort covers exactly `target_m`. `None` when the legs are shorter.
pub(in crate::app) fn fastest_window_sec(legs: &[(f64, f64)], target_m: f64) -> Option<f64> {
    let n = legs.len();
    let mut cd = Vec::with_capacity(n + 1);
    let mut ct = Vec::with_capacity(n + 1);
    cd.push(0.0_f64);
    ct.push(0.0_f64);
    for (d, t) in legs {
        cd.push(cd.last().copied().unwrap_or(0.0) + d);
        ct.push(ct.last().copied().unwrap_or(0.0) + t);
    }
    if cd[n] < target_m {
        return None;
    }
    let mut best: Option<f64> = None;
    let mut i = 0usize;
    for j in 1..=n {
        while i + 1 < j && cd[j] - cd[i + 1] >= target_m {
            i += 1;
        }
        let covered = cd[j] - cd[i];
        if covered < target_m {
            continue;
        }
        // The excess lies inside leg i (i → i+1); trim its share of time.
        let (leg_d, leg_t) = legs[i];
        let excess = covered - target_m;
        let trim = if leg_d > 0.0 {
            excess / leg_d * leg_t
        } else {
            0.0
        };
        let t = ct[j] - ct[i] - trim;
        if t > 0.0 && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    }
    best
}

/// One run's best effort per standard distance, memoized by content key.
/// `qc` is the run's QC'd track (`None` for a hand-entered run).
fn run_best_efforts(r: &LoggedRun, key: u64, qc: Option<&QcTrack>) -> RunBests {
    memoized(&BEST_EFFORT_CACHE, key, || {
        let mut out: RunBests = [None; BEST_EFFORT_DISTANCES.len()];
        match qc {
            None => {
                let (km, min) = (r.distance_km, r.duration_min);
                if km > 0.0 && min > 0.0 {
                    for (slot, (m, _)) in out.iter_mut().zip(BEST_EFFORT_DISTANCES) {
                        let d = km * 1000.0;
                        if d >= m * WHOLE_RUN_MATCH.0 && d <= m * WHOLE_RUN_MATCH.1 {
                            *slot = Some((min * 60.0 * m / d, false));
                        }
                    }
                }
            }
            Some(q) => {
                let legs = moving_legs(&q.track, &q.seg_starts);
                let total_m: f64 = legs.iter().map(|(d, _)| d).sum();
                for (slot, (m, _)) in out.iter_mut().zip(BEST_EFFORT_DISTANCES) {
                    *slot =
                        fastest_window_sec(&legs, m).map(|t| (t, total_m > m * WHOLE_RUN_MATCH.1));
                }
            }
        }
        out
    })
}

/// Weeks in the progress series (a half-marathon block is 12–20 weeks).
const PROGRESS_WEEKS: i64 = 16;

/// Two best-effort times within this many seconds are a tie (float noise from
/// the window interpolation), so the earlier run keeps the record.
const BEST_EFFORT_TIE_SEC: f64 = 1e-9;

/// The running-progress view (self-monitoring, ADH-SELFMON-001): the last
/// [`PROGRESS_WEEKS`] Monday-start local weeks of volume (with the separate
/// session-RPE load series, LOAD-SRPE-001), the longest run and best efforts.
/// Future-dated rows are excluded. `None` until a dated run with a distance
/// exists.
pub(in crate::app) fn build_run_progress(
    model: &Model,
    facts: &RunFacts,
) -> Option<RunProgressView> {
    let offset = model.today_utc_offset_sec;
    let dated: Vec<(usize, &LoggedRun, i64)> = model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.observed_at > 0)
        .map(|(i, r)| (i, r, local_day(r.observed_at, offset)))
        .collect();
    let today = model
        .today_epoch_day
        .or_else(|| dated.iter().map(|(_, _, d)| *d).max())?;
    let live: Vec<(usize, &LoggedRun, i64)> =
        dated.into_iter().filter(|(_, _, d)| *d <= today).collect();
    if !live.iter().any(|(i, ..)| facts.km[*i] > 0.0) {
        return None;
    }
    let cur_monday = today - mon0_weekday(today);
    let first_monday = cur_monday - (PROGRESS_WEEKS - 1) * 7;
    let mut weekly: Vec<RunWeekView> = (0..PROGRESS_WEEKS)
        .map(|w| RunWeekView {
            week_start_epoch_day: first_monday + w * 7,
            ..Default::default()
        })
        .collect();
    let mut longest = (0.0_f64, 0_i64);
    let mut bests = [None::<(f64, bool, u64, i64)>; BEST_EFFORT_DISTANCES.len()];
    for (i, r, day) in &live {
        let (km, min) = (facts.km[*i], facts.minutes[*i]);
        if km > longest.0 {
            longest = (km, r.observed_at);
        }
        let monday = day - mon0_weekday(*day);
        if monday >= first_monday {
            let w = &mut weekly[((monday - first_monday) / 7) as usize];
            w.km += km;
            w.longest_km = w.longest_km.max(km);
            w.runs += 1;
            w.moving_min += min;
            if let Some(rpe) = r.session_rpe
                && min > 0.0
            {
                w.srpe_load_au += load::srpe_load_au(min, rpe);
                w.rated_runs += 1;
            }
        }
        let efforts = run_best_efforts(r, facts.keys[*i], facts.qc[*i].as_deref());
        for (slot, effort) in bests.iter_mut().zip(efforts) {
            if let Some((t, split)) = effort {
                // Strictly faster wins; a tie keeps the EARLIER run (the record
                // was set first), independent of storage order.
                let better = match slot {
                    None => true,
                    Some((bt, _, _, at)) => {
                        t < *bt - BEST_EFFORT_TIE_SEC
                            || ((t - *bt).abs() <= BEST_EFFORT_TIE_SEC && r.observed_at < *at)
                    }
                };
                if better {
                    *slot = Some((t, split, r.entry_id, r.observed_at));
                }
            }
        }
    }
    let round1 = |x: f64| (x * 10.0).round() / 10.0;
    for w in &mut weekly {
        w.km = round1(w.km);
        w.longest_km = round1(w.longest_km);
        w.moving_min = round1(w.moving_min);
        w.srpe_load_au = round1(w.srpe_load_au);
    }
    let best_efforts = bests
        .iter()
        .zip(BEST_EFFORT_DISTANCES)
        .filter_map(|(b, (m, wire_m))| {
            b.map(|(t, from_split, entry_id, observed_at)| BestEffortView {
                distance_m: wire_m,
                label: race_distance_title(m / 1000.0),
                time_sec: whole_seconds(t).min(u64::from(u32::MAX)) as u32,
                time_label: fmt_clock(t),
                pace_label: format!("{} /km", fmt_mss(t / (m / 1000.0))),
                entry_id,
                observed_at,
                from_split,
            })
        })
        .collect();
    let g = graded((), claims::ADH_SELFMON_001);
    Some(RunProgressView {
        weekly,
        planned_weekly_km: model
            .profile
            .as_ref()
            .map(|p| p.running_km_per_week.max(0.0))
            .unwrap_or(0.0),
        longest_run_km: round1(longest.0),
        longest_run_observed_at: longest.1,
        best_efforts,
        citation: g.reference().to_owned(),
        grade: Some(g.grade()),
        confidence: g.confidence(),
    })
}

/// Fill the run-card fields that depend on data beyond the run itself, after
/// the per-run memo: the best-effort records this run holds.
pub(in crate::app) fn annotate_run_views(
    runs: &mut [RunResultView],
    progress: Option<&RunProgressView>,
) {
    let Some(p) = progress else {
        return;
    };
    for v in runs.iter_mut() {
        v.best_effort_labels = p
            .best_efforts
            .iter()
            .filter(|b| b.entry_id == v.entry_id && b.observed_at == v.observed_at)
            .map(|b| {
                format!(
                    "Fastest {}",
                    race_distance_label(f64::from(b.distance_m) / 1000.0)
                )
            })
            .collect();
    }
}
