//! GPS track processing: the File 07 ingest QC limits ("Sanity / QC"), fix
//! acceptance, recording segments, the QC pass, moving legs, the track
//! metrics derived from them (distance, splits, positive split, variability
//! index), trimming, decimation and the GPX writer. The one definition of
//! every GPS limit and of the geometry; the run import, the run views and the
//! shells' live recorder (through the generated `CoreConstants`) use it.
//!
//! Pure geodesy, not a coached claim, so nothing here carries an evidence tag
//! (the graded interval verdict on the variability index is
//! `running::interval_verdict`). Wall-clock time enters only as fix stamps.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use crate::sanitize::{positive_zero, sum0};

/// Mean Earth radius (metres) for the haversine great-circle distance.
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Fixes with worse horizontal accuracy than this (metres) are noise; they are
/// dropped before deriving distance *and* duration so both are computed from the
/// same usable slice (otherwise a noisy first/last fix skews derived pace).
pub const MAX_GPS_ACCURACY_M: f32 = 30.0;

/// A GPS ground speed above this (m/s) is impossible for a runner and is treated
/// as jitter (File 07 "reject samples implying >12 m/s"). One value for the
/// QC gate, the moving-leg cap and the shell live-jitter guard.
pub const MAX_PLAUSIBLE_SPEED_MPS: f64 = 12.0;

/// The smallest move that counts as one (File 07, Apple pattern): a fix closer
/// than this to the previous one is jitter.
const GPS_MIN_MOVE_M: f64 = 2.5;

/// Ground speed (m/s) below which a leg counts as stopped (File 07 auto-pause
/// rule): it adds no moving time or distance.
pub const MIN_MOVING_SPEED_MPS: f64 = 0.5;

/// A gap longer than this (seconds) between consecutive accepted fixes opens a
/// new recording segment: the straight-line bridge across it (a pause and
/// relocation, a tunnel, a signal dropout) is no run distance. A normal 1–4 s
/// fix cadence never trips it.
pub const MAX_FIX_GAP_SEC: i64 = 30;

/// The most GPS fixes a saved run keeps in one `LogRunTrack` (the heaviest line
/// of the append-only log): about 90 min at 1 Hz, so ordinary runs keep full
/// detail and only very long ones are thinned ([`decimation_keep`]).
pub const TRACK_DECIMATION_CAP: u32 = 5400;

/// The keep threshold of a run: a track under this distance (km) or under
/// [`MIN_RUN_SEC`] is as likely an accidental recording as a run.
pub const MIN_RUN_KM: f64 = 0.5;

/// The keep threshold's time (seconds); see [`MIN_RUN_KM`].
pub const MIN_RUN_SEC: i64 = 180;

/// GPS speed sanity gate (File 07): true when the speed is plausible for a
/// runner (≤ [`MAX_PLAUSIBLE_SPEED_MPS`]).
fn gps_speed_plausible(speed_m_s: f64) -> bool {
    speed_m_s <= MAX_PLAUSIBLE_SPEED_MPS
}

/// GPS point-acceptance gate (Apple pattern, File 07): accept a new fix only if
/// it moved ≥ [`GPS_MIN_MOVE_M`]. File 07's vertical-rate arm (≤ 5 m/s) never
/// applies: fixes carry no altitude.
fn gps_point_accept(distance_moved_m: f64) -> bool {
    distance_moved_m >= GPS_MIN_MOVE_M
}

/// Stop detection for pace/load exclusion: below [`MIN_MOVING_SPEED_MPS`]
/// counts as stopped (File 07 auto-pause rule).
fn is_stopped(speed_m_s: f64) -> bool {
    speed_m_s < MIN_MOVING_SPEED_MPS
}

fn accurate(p: &GpsPoint) -> bool {
    p.accuracy_m <= MAX_GPS_ACCURACY_M
}

/// One GPS fix from a run track. Deterministic core input: wall-clock time
/// enters only as `observed_at` (unix seconds), never a live clock. Lat/lon in
/// decimal degrees; `accuracy_m` is the shell provider's reported horizontal
/// error, used to drop noisy fixes before summing distance.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Serialize,
    Deserialize,
    crate::sanitize::Sanitize,
    crate::wire::Wire,
)]
pub struct GpsPoint {
    pub lat: f64,
    pub lon: f64,
    pub observed_at: i64,
    pub accuracy_m: f32,
}

/// Great-circle distance between two fixes, in metres (haversine).
pub fn haversine_m(a: GpsPoint, b: GpsPoint) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let dlat = (b.lat - a.lat).to_radians();
    let dlon = (b.lon - a.lon).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    // Rounding can push `h` a hair above 1.0 for near-antipodal points, and
    // `asin` of a root above 1 is NaN.
    2.0 * EARTH_RADIUS_M * h.min(1.0).sqrt().asin()
}

/// One MOVING leg between consecutive fixes as `(dt_seconds, distance_m)`.
/// `None` when the timestamps don't advance or the ground speed is below the
/// auto-pause floor ([`is_stopped`]). Saturating: a corrupt stamp pair
/// (i64::MIN → i64::MAX) cannot overflow; the dt or stop gate then discards
/// the leg. Distance is raw haversine; callers that need
/// jitter rejection cap it at [`MAX_PLAUSIBLE_SPEED_MPS`].
fn leg(a: GpsPoint, b: GpsPoint) -> Option<(f64, f64)> {
    let dt = b.observed_at.saturating_sub(a.observed_at) as f64;
    if dt <= 0.0 {
        return None;
    }
    let d = haversine_m(a, b);
    (!is_stopped(d / dt)).then_some((dt, d))
}

/// The fixes within [`MAX_GPS_ACCURACY_M`], in original order: the accuracy
/// gate of [`moving_legs`] and [`usable_segments`].
fn usable_track(points: &[GpsPoint]) -> Vec<GpsPoint> {
    points.iter().copied().filter(accurate).collect()
}

/// Split `points` into recording SEGMENTS at `segment_starts`, the indices that
/// BEGIN a new segment (the first fix captured after a pause/resume or a long
/// gap in fixes). The leg from `points[i-1]` to `points[i]` at such an index is a
/// PAUSE BRIDGE the runner may have relocated across, so no track metric may sum
/// distance or time across it. An EMPTY `segment_starts` (a re-anchored log or
/// a hand-logged run) yields the whole track as a single segment. Out-of-range,
/// zero, and duplicate indices are ignored, so a malformed boundary list can
/// only under-split, never panic.
pub(crate) fn segments<'a>(points: &'a [GpsPoint], segment_starts: &[u32]) -> Vec<&'a [GpsPoint]> {
    let bounds = segment_bounds(segment_starts, points.len());
    if bounds.is_empty() {
        return vec![points];
    }
    let mut out = Vec::with_capacity(bounds.len() + 1);
    let mut prev = 0usize;
    for b in bounds {
        out.push(&points[prev..b]);
        prev = b;
    }
    out.push(&points[prev..]);
    out
}

/// The indices of a `len`-fix track that begin a new segment, sorted and
/// deduplicated: `segment_starts` without index 0 (the track's first fix
/// begins the first segment anyway) and without out-of-range entries. The one
/// boundary rule [`segments`], [`qc_track`] and [`trim_track`] share.
fn segment_bounds(segment_starts: &[u32], len: usize) -> Vec<usize> {
    let mut bounds: Vec<usize> = segment_starts
        .iter()
        .map(|&i| i as usize)
        .filter(|&i| i > 0 && i < len)
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    bounds
}

/// The usable fixes of every segment, flattened in order, each paired with a flag
/// that is `true` for the first usable fix of its segment: the leg entering it
/// is a pause bridge and contributes neither distance nor time.
fn usable_segments(points: &[GpsPoint], segment_starts: &[u32]) -> (Vec<GpsPoint>, Vec<bool>) {
    let mut pts = Vec::with_capacity(points.len());
    let mut is_start = Vec::with_capacity(points.len());
    for seg in segments(points, segment_starts) {
        let mut first = true;
        for p in usable_track(seg) {
            pts.push(p);
            is_start.push(first);
            first = false;
        }
    }
    (pts, is_start)
}

/// Total track distance in km: the sum of `moving_legs`, so inaccurate fixes,
/// stopped legs and pause bridges add nothing and jitter is capped. The
/// displayed pace, variability index and splits share these gates.
pub fn track_distance_km(points: &[GpsPoint], segment_starts: &[u32]) -> f64 {
    sum0(moving_legs(points, segment_starts).iter().map(|(_, d)| *d)) / 1000.0
}

/// Cumulative moving distance in metres to each usable fix (`out[0] == 0.0`):
/// the leg gates and cap of [`moving_legs`], nothing for a pause-bridge leg, so
/// the split walk sums to [`track_distance_km`].
fn cumulative_moving_m(usable: &[GpsPoint], is_start: &[bool]) -> Vec<f64> {
    let mut cumulative = Vec::with_capacity(usable.len());
    let mut running_total = 0.0;
    cumulative.push(0.0);
    for i in 1..usable.len() {
        let leg_m = if is_start[i] {
            0.0
        } else {
            leg(usable[i - 1], usable[i]).map_or(0.0, |(dt, d)| d.min(MAX_PLAUSIBLE_SPEED_MPS * dt))
        };
        running_total += leg_m;
        cumulative.push(running_total);
    }
    cumulative
}

/// Second-half-vs-first-half pace slowdown across a track, as a percent (a
/// *positive split*: positive = the runner slowed in the back half). The track
/// is split at its halfway moving distance and each half's moving pace is
/// compared. Descriptive, not a recommendation. `None` for fewer than three
/// usable fixes or a zero-distance/zero-duration half.
pub fn track_positive_split_pct(points: &[GpsPoint], segment_starts: &[u32]) -> Option<f64> {
    let (usable, is_start) = usable_segments(points, segment_starts);
    if usable.len() < 3 {
        return None;
    }
    // Cumulative MOVING distance to each fix (a pause-bridge or below-floor leg
    // adds nothing), so the halfway split and each half sit on the same distance
    // the headline uses (see `cumulative_moving_m`).
    let cumulative = cumulative_moving_m(&usable, &is_start);
    let total = *cumulative.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return None;
    }
    let half = total / 2.0;
    let split = (1..usable.len() - 1).find(|&i| cumulative[i] >= half)?;

    let dist1 = cumulative[split];
    let dist2 = total - dist1;
    // Each half's time is moving seconds only (stopped and pause-bridge legs
    // add none), the base of the displayed pace, splits and VI.
    let moving_leg = |i: usize| -> f64 {
        if is_start[i] {
            return 0.0;
        }
        leg(usable[i - 1], usable[i]).map_or(0.0, |(dt, _)| dt)
    };
    let dur1: f64 = (1..=split).map(moving_leg).sum();
    let dur2: f64 = (split + 1..usable.len()).map(moving_leg).sum();
    if dist1 <= 0.0 || dist2 <= 0.0 || dur1 <= 0.0 || dur2 <= 0.0 {
        return None;
    }
    let pace1 = dur1 / dist1;
    let pace2 = dur2 / dist2;
    Some(positive_zero(
        ((pace2 - pace1) / pace1 * 100.0 * 10.0).round() / 10.0,
    ))
}

/// One completed (or final partial) split of a run track. Descriptive: a
/// measurement of the run, not a recommendation, so it carries no evidence tag
/// (same standing as [`track_positive_split_pct`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunSplit {
    /// 1-based split index (1 = first km/mile).
    pub index: u32,
    /// Pace over this split, seconds per unit distance (per km, or per mile),
    /// timed in MOVING seconds (stopped legs excluded, see [`track_splits`])
    /// so it lines up with the run's moving-time headline pace.
    /// For the final partial split the pace is normalized to a full unit
    /// (`duration / covered_distance × unit`) so it stays comparable to the full
    /// splits rather than looking artificially fast over a short remainder.
    pub pace_sec_per_unit: f64,
    /// Cumulative track distance at the END of this split, metres.
    pub cumulative_m: f64,
    /// Distance actually covered by this split, metres. Equals the unit distance
    /// for a full split; smaller for the final partial split.
    pub distance_m: f64,
    /// True only for a final split shorter than a full unit (`distance_m < unit`).
    pub partial: bool,
}

/// Hard cap on emitted splits (see [`track_splits`]), bounding per-`view()`
/// work against a corrupt track. 10 k km/mile splits is far past any real run.
const MAX_SPLITS: usize = 10_000;

/// Per-unit run splits from a GPS track: one [`RunSplit`] per completed
/// `unit_m` (a kilometre or a mile), plus a final partial split when the track
/// ends mid-unit. Boundaries fall at exact unit distances (apportioned within
/// the crossing leg at its constant speed). Splits are timed and measured over
/// moving legs only (see `moving_legs`), so they sum to [`track_distance_km`]
/// and a stop does not slow a split. Empty for a sub-metre unit or a track with
/// no moving distance.
pub fn track_splits(points: &[GpsPoint], unit_m: f64, segment_starts: &[u32]) -> Vec<RunSplit> {
    // Reject a sub-metre unit. The boundary loop below emits one split per
    // `unit_m` of distance, so a tiny unit (a caller bug / corrupt input) would
    // spin for millions of iterations. Every real caller passes km/mile.
    if unit_m < 1.0 {
        return Vec::new();
    }
    let (usable, is_start) = usable_segments(points, segment_starts);
    if usable.len() < 2 {
        return Vec::new();
    }
    let cumulative = cumulative_moving_m(&usable, &is_start);
    let total = *cumulative.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return Vec::new();
    }

    let mut splits = Vec::new();
    let mut index = 1u32;
    let mut boundary = unit_m;
    let mut moving_s = 0.0; // the split's pace clock (moving legs only)
    for i in 1..usable.len() {
        let leg_d = cumulative[i] - cumulative[i - 1];
        let dt = (usable[i]
            .observed_at
            .saturating_sub(usable[i - 1].observed_at) as f64)
            .max(0.0);
        // Same pause gate as `leg`, read off the leg's gated cumulative distance
        // (0 for a paused or pause-bridge leg).
        let moving = dt > 0.0 && !is_stopped(leg_d / dt);
        // Distance already consumed within this leg (start of unconsumed part).
        let mut pos = cumulative[i - 1];
        // Emit every unit boundary that falls inside this leg; the leg's time
        // is apportioned to each side at the leg's constant speed.
        while boundary <= cumulative[i] + 1e-6 {
            let sub = if leg_d > 0.0 {
                dt * ((boundary - pos) / leg_d)
            } else {
                0.0
            };
            if moving {
                moving_s += sub;
            }
            let dur = moving_s;
            splits.push(RunSplit {
                index,
                // Full split: distance == unit, so pace IS the (moving) time.
                pace_sec_per_unit: dur,
                cumulative_m: boundary,
                distance_m: unit_m,
                partial: false,
            });
            // Even at a legitimate unit, a corrupt track (a mis-unit'd
            // coordinate inflating cumulative distance to thousands of km) could
            // emit millions of splits in `view()`. Bound the list: a genuine
            // run never approaches this many.
            if splits.len() >= MAX_SPLITS {
                return splits;
            }
            pos = boundary;
            moving_s = 0.0;
            index += 1;
            boundary = unit_m * index as f64;
        }
        // Remainder of the leg past the last boundary feeds the next split.
        // Only a moving leg advances the split clock; a paused / zero-distance
        // leg (leg_d == 0) contributes neither time nor distance.
        if moving {
            moving_s += dt * ((cumulative[i] - pos) / leg_d);
        }
    }
    let remaining = total - unit_m * (index - 1) as f64;
    if remaining > 1e-6 {
        let dur = moving_s;
        splits.push(RunSplit {
            index,
            // Normalize the short remainder to a full unit so it is comparable.
            pace_sec_per_unit: dur / remaining * unit_m,
            cumulative_m: total,
            distance_m: remaining,
            partial: true,
        });
    }
    splits
}

/// Rolling-average window (seconds) applied to the speed series before the
/// 4th-power weighting (GOVSS / Normalized Graded Pace; Skiba 120 s,
/// GoldenCheetah 30 s). At 1 Hz the per-second speed is dominated by GPS noise,
/// which the 4th power amplifies; 30 s averages it away and keeps 400 m reps.
const INTERVAL_WINDOW_SEC: f64 = 30.0;

/// The MOVING legs of a track as `(dt_seconds, distance_m)`, in order: the one
/// rule behind every moving time and distance. Fixes worse than
/// [`MAX_GPS_ACCURACY_M`] are dropped, legs are formed within each
/// [`segments`] segment (a pause bridge is never a leg), a leg that [`leg`]
/// rejects (timestamps not advancing, speed below the auto-pause floor) is
/// dropped, and distance is capped at [`MAX_PLAUSIBLE_SPEED_MPS`] × dt (GPS
/// jitter).
pub(crate) fn moving_legs(points: &[GpsPoint], segment_starts: &[u32]) -> Vec<(f64, f64)> {
    segments(points, segment_starts)
        .into_iter()
        .flat_map(|seg| {
            usable_track(seg)
                .windows(2)
                .filter_map(|w| {
                    leg(w[0], w[1]).map(|(dt, d)| (dt, d.min(MAX_PLAUSIBLE_SPEED_MPS * dt)))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resample moving legs into fixed [`INTERVAL_WINDOW_SEC`] bins along the moving
/// timeline (the pause gaps are already removed), returning each bin's
/// `(mean_speed, duration)`. A leg is split across bin boundaries at its own
/// constant speed, so the bins are a proper rolling average of speed over the
/// window, the smoothing that makes the 4th-power step respond to real pace
/// structure instead of per-second GPS noise. The final short bin keeps its true
/// (sub-window) duration so it is weighted correctly.
fn windowed_bin_speeds(legs: &[(f64, f64)], window_s: f64) -> Vec<(f64, f64)> {
    let mut bins = Vec::new();
    // A non-positive window would make `take` zero and spin forever; refuse it.
    if window_s <= 0.0 {
        return bins;
    }
    let mut bin_time = 0.0;
    let mut bin_dist = 0.0;
    for &(dt, dist) in legs {
        let speed = if dt > 0.0 { dist / dt } else { 0.0 };
        let mut rem = dt;
        while rem > 0.0 {
            let take = rem.min(window_s - bin_time);
            bin_time += take;
            bin_dist += speed * take;
            rem -= take;
            if bin_time >= window_s - 1e-9 {
                bins.push((bin_dist / bin_time, bin_time));
                bin_time = 0.0;
                bin_dist = 0.0;
                // A corrupt `dt` (e.g. a mis-unit'd timestamp yielding a
                // multi-week "leg") would otherwise drive this resampling loop
                // for millions of iterations inside `view()`. A real run never
                // approaches this many windows; past it the series is corrupt
                // and the truncated (still deterministic) VI is good enough.
                if bins.len() >= MAX_WINDOW_BINS {
                    return bins;
                }
            }
        }
    }
    if bin_time > 0.0 {
        bins.push((bin_dist / bin_time, bin_time));
    }
    bins
}

/// Hard cap on smoothing bins produced by [`windowed_bin_speeds`], bounding the
/// per-`view()` work against a corrupt GPS timeline (see the early return there).
/// ~50 k × 30 s ≈ 17 days of moving time, unreachable for any genuine run.
const MAX_WINDOW_BINS: usize = 50_000;

/// Normalized speed (m/s): the duration-weighted 4th-power mean of the
/// [`INTERVAL_WINDOW_SEC`]-averaged speed of `legs` (GOVSS / Normalized Graded
/// Pace on flat ground, so speed, not power). The convex 4th power scores a
/// 6×800 m session above its own average while a steady run stays at it.
/// `None` with no moving time.
fn normalized_speed(legs: &[(f64, f64)]) -> Option<f64> {
    let bins = windowed_bin_speeds(legs, INTERVAL_WINDOW_SEC);
    let (mut p4_sum, mut weight_sum) = (0.0, 0.0);
    for &(speed, dur) in &bins {
        p4_sum += dur * speed.powi(4);
        weight_sum += dur;
    }
    if weight_sum <= 0.0 {
        return None;
    }
    Some((p4_sum / weight_sum).powf(0.25))
}

/// Variability Index = normalized speed ÷ average moving speed, both over
/// `moving_legs`. About 1.0 for a steady run, rising as the run becomes
/// interval-like, so it tells a 6×800 m session from a steady run of the same
/// average pace. Descriptive, not a load input. `None` with no moving legs.
pub fn track_variability_index(points: &[GpsPoint], segment_starts: &[u32]) -> Option<f64> {
    let legs = moving_legs(points, segment_starts);
    let ns = normalized_speed(&legs)?;
    let dur: f64 = legs.iter().map(|&(dt, _)| dt).sum();
    let dist: f64 = legs.iter().map(|&(_, d)| d).sum();
    if dur <= 0.0 {
        return None;
    }
    let avg = dist / dur;
    if avg <= 0.0 {
        return None;
    }
    Some(((ns / avg) * 100.0).round() / 100.0)
}

/// File 07 GPS quality gates over a fix track, applied BEFORE any distance /
/// duration / split is derived. Returns the surviving fixes and the remapped
/// segment starts.
///
/// Gates, in order per surviving pair: a non-advancing timestamp (speed
/// undefined), an implied speed over [`MAX_PLAUSIBLE_SPEED_MPS`]
/// ([`gps_speed_plausible`], impossible for a runner), and a move under
/// [`GPS_MIN_MOVE_M`] ([`gps_point_accept`], the Apple jitter/auto-pause
/// pattern).
/// Stops are judged over a [`StopWindow`], never leg by leg: a stop keeps one
/// fix, so the leg into it is no move and the stride out of it
/// ([`stride_out_start`]) is a normal moving leg.
///
/// The pass ([`qc_pass`]) repeats on its own output until every gate accepts
/// the track unchanged: windows are cut from the raw stream, so a single pass
/// can leave a survivor that a window over the survivors would drop. Each pass
/// keeps an ordered subset of its input, so this ends, and the QC'd track is a
/// fixed point: QC of an exported (QC'd) route (Undo, re-import) gives back the
/// same fixes and figures.
pub(crate) fn qc_track(points: &[GpsPoint], segment_starts: &[u32]) -> QcFixes {
    let mut q = qc_pass(points, segment_starts);
    loop {
        let next = qc_pass(&q.track, &q.seg_starts);
        if next.track.len() == q.track.len() {
            // Same fixes; the boundaries as this pass remapped them.
            return next;
        }
        q = next;
    }
}

/// One pass of [`qc_track`]'s gates.
fn qc_pass(points: &[GpsPoint], segment_starts: &[u32]) -> QcFixes {
    let bounds = segment_bounds(segment_starts, points.len());
    let is_boundary = |i: usize| bounds.binary_search(&i).is_ok();
    let mut out: Vec<GpsPoint> = Vec::with_capacity(points.len());
    // Segment-start indices REMAPPED to positions in the QC'd output. A boundary
    // fix may itself be dropped, so the first survivor at/after it opens the new
    // segment. Passed on to the segment-aware track metrics and the GPX export
    // so a pause + relocation is neither summed nor drawn across.
    let mut out_starts: Vec<u32> = Vec::new();
    // A boundary seen in the raw stream but not yet committed (its fix and any
    // until the next survivor may be dropped). While pending, the inter-fix QC
    // gates are SUPPRESSED: the leg across a pause bridge is a legitimate
    // relocation teleport, not a jitter fix, and gating it would wrongly drop the
    // first fix of the new segment.
    let mut pending_boundary = false;
    // The window last scanned found no standing fix for the anchor at this
    // output length, up to this raw index: a later start inside it holds a
    // subset, so it needs no rescan.
    let mut clear_until: Option<(usize, usize)> = None;
    // Raw fixes before this index (a resolved stop's stride out) only pass the
    // plain gates: a stop is resolved once, so its stride out is never re-read
    // as standing around the kept fix.
    let mut plain_until = 0usize;
    let mut i = 0;
    while i < points.len() {
        let p = points[i];
        if is_boundary(i) {
            pending_boundary = true;
        }
        if !accurate(&p) {
            i += 1;
            continue;
        }
        let last = match out.last() {
            Some(&s) if !pending_boundary => s,
            // First survivor of the track or of a new segment. The track's
            // first survivor opens no segment: output index 0 begins the first
            // one anyway ([`segment_bounds`]).
            _ => {
                if pending_boundary && !out.is_empty() {
                    out_starts.push(out.len() as u32);
                }
                pending_boundary = false;
                out.push(p);
                i += 1;
                continue;
            }
        };
        if i >= plain_until
            && !clear_until.is_some_and(|(anchor, end)| anchor == out.len() && i < end)
        {
            let w = stop_window(points, &bounds, i, last);
            match w.last_standing {
                Some(standing) => {
                    // The runner stood at `last`: keep the window's last fix
                    // before the stride out that is still below the floor from
                    // `last` (at the latest the last standing fix), drop the
                    // rest, resume at the stride out. The stop then ends where
                    // the runner set off, even when the fixes settled off the
                    // spot after the last one under 2.5 m. A stop the track or
                    // segment ends in keeps nothing: no stride leaves it, and a
                    // stray fix would only add a point to the route (or make a
                    // single-fix run look measurable).
                    let start = stride_out_start(points, standing, w.end, last, w.leaving);
                    if w.leaving.is_some() || start < w.end {
                        let keep = (standing..start)
                            .rev()
                            .find(|&j| {
                                let q = points[j];
                                accurate(&q)
                                    && q.observed_at > last.observed_at
                                    && leg(last, q).is_none()
                            })
                            .unwrap_or(standing);
                        out.push(points[keep]);
                    }
                    i = start;
                    plain_until = w.end;
                    clear_until = None;
                    continue;
                }
                None => clear_until = Some((out.len(), w.end)),
            }
        }
        i += 1;
        let dt = p.observed_at.saturating_sub(last.observed_at);
        if dt <= 0 {
            continue;
        }
        let dist_m = haversine_m(last, p);
        if !gps_speed_plausible(dist_m / dt as f64) || !gps_point_accept(dist_m) {
            continue;
        }
        out.push(p);
    }
    QcFixes {
        track: out,
        seg_starts: out_starts,
    }
}

/// The look-ahead window of [`qc_track`] for anchor `s` (the last survivor)
/// from raw index `from`: the usable fixes after `s` (accuracy-gated, stamped
/// after `s`) up to the first one that LEAVES `s`, i.e. lies farther from it
/// than [`stand_radius_m`]. A segment boundary or the track's end also closes
/// it. An outlier that ends a window early is gated out after it, and the
/// next window from the kept fix carries the stop on.
struct StopWindow {
    /// Raw index one past the window: the leaving fix, a boundary or the end.
    end: usize,
    /// Distance from `s` of the leaving fix; `None` at a boundary or the end.
    leaving: Option<f64>,
    /// Raw index of the window's last STANDING fix: under [`GPS_MIN_MOVE_M`]
    /// from `s` ([`gps_point_accept`]) and below the auto-pause floor measured
    /// from `s` ([`leg`] is `None`).
    last_standing: Option<usize>,
}

fn stop_window(points: &[GpsPoint], bounds: &[usize], from: usize, s: GpsPoint) -> StopWindow {
    let mut last_standing = None;
    for (j, q) in points.iter().enumerate().skip(from) {
        if j > from && bounds.binary_search(&j).is_ok() {
            return StopWindow {
                end: j,
                leaving: None,
                last_standing,
            };
        }
        if !accurate(q) || q.observed_at <= s.observed_at {
            continue;
        }
        let d = haversine_m(s, *q);
        if d > stand_radius_m(s, *q) {
            return StopWindow {
                end: j,
                leaving: Some(d),
                last_standing,
            };
        }
        if !gps_point_accept(d) && leg(s, *q).is_none() {
            last_standing = Some(j);
        }
    }
    StopWindow {
        end: points.len(),
        leaving: None,
        last_standing,
    }
}

/// How far from `s` a fix may lie and still be the same standing position:
/// the sum of both fixes' reported horizontal accuracy (each fix lies within
/// its own accuracy of the true point), never under the [`GPS_MIN_MOVE_M`]
/// jitter gate.
fn stand_radius_m(s: GpsPoint, q: GpsPoint) -> f64 {
    (f64::from(s.accuracy_m) + f64::from(q.accuracy_m)).max(GPS_MIN_MOVE_M)
}

/// Raw index where the stride out of a stop at `s` begins: the longest run of
/// usable fixes after the window's last standing fix `standing` (and before
/// `end`) that ends the window moving steadily AWAY from `s`, each one past the
/// [`GPS_MIN_MOVE_M`] jitter gate and farther from `s` than the one before. `leaving` is the
/// leaving fix's distance (`None` at a boundary or the end of the track).
/// Jitter or drift around the stop rarely forms such a run; a runner setting
/// off always does. With no stride out, `end`.
fn stride_out_start(
    points: &[GpsPoint],
    standing: usize,
    end: usize,
    s: GpsPoint,
    leaving: Option<f64>,
) -> usize {
    let mut start = end;
    let mut next_d = leaving.unwrap_or(f64::INFINITY);
    for j in (standing + 1..end).rev() {
        let q = points[j];
        if !accurate(&q) || q.observed_at <= s.observed_at {
            continue;
        }
        let d = haversine_m(s, q);
        // Negated so a NaN distance ends the stride out instead of joining it.
        if !(d >= GPS_MIN_MOVE_M && d < next_d) {
            break;
        }
        next_d = d;
        start = j;
    }
    start
}

/// [`qc_track`]'s output.
pub(crate) struct QcFixes {
    pub(crate) track: Vec<GpsPoint>,
    pub(crate) seg_starts: Vec<u32>,
}

/// Moving time over a QC'd track, minutes: interval seconds where the implied
/// speed clears the stop gate ([`is_stopped`]), so standing time never dilutes
/// pace. The same [`leg`] gate picks the legs [`track_distance_km`] sums, so distance and moving time always describe the same legs; a
/// standing spell ends at the standing fix [`qc_track`] keeps, so only the
/// stride out of it moves.
pub(crate) fn moving_duration_min(qc: &QcFixes) -> f64 {
    let mut sec = 0.0;
    // Sum moving legs WITHIN each segment: the pause-bridge leg between segments
    // is never formed, so a paused relocation adds no moving time. Empty
    // `segment_starts` → one segment → the whole-track sum, unchanged.
    for seg in segments(&qc.track, &qc.seg_starts) {
        for w in seg.windows(2) {
            if let Some((dt, _)) = leg(w[0], w[1]) {
                sec += dt;
            }
        }
    }
    sec / 60.0
}

/// The fixes of `points` whose `observed_at` lies in
/// `[keep_from_sec, keep_until_sec]`, with `segment_starts` remapped, or `None`
/// unless that is a REAL trim: at least 2 surviving fixes and at least one
/// dropped.
///
/// Boundaries follow [`qc_track`]'s pending-boundary rule: a boundary whose fix
/// is cut shifts to the first survivor of its segment; a fully-cut segment
/// drops its boundary.
pub(crate) fn trim_track(
    points: &[GpsPoint],
    segment_starts: &[u32],
    keep_from_sec: i64,
    keep_until_sec: i64,
) -> Option<(Vec<GpsPoint>, Vec<u32>)> {
    let bounds = segment_bounds(segment_starts, points.len());
    let mut new_track: Vec<GpsPoint> = Vec::with_capacity(points.len());
    let mut new_starts: Vec<u32> = Vec::new();
    let mut pending_boundary = false;
    for (idx, p) in points.iter().enumerate() {
        if bounds.binary_search(&idx).is_ok() {
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
    (new_track.len() >= 2 && new_track.len() < points.len()).then_some((new_track, new_starts))
}

/// The indices (ascending) kept when thinning an `n`-fix track to about
/// `max_points`: a uniform stride (ceiling division, so the strided count stays
/// within the cap) plus the last fix and every segment start in `starts`. At or
/// below the cap, every index. The save rule of a live run (the shell's
/// recorder) and of an imported one ([`crate::import`]).
pub(crate) fn decimation_keep(n: usize, max_points: usize, starts: &[usize]) -> Vec<usize> {
    if n <= max_points || max_points < 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    let stride = n.div_ceil(max_points);
    for k in (0..n).step_by(stride) {
        keep[k] = true;
    }
    keep[n - 1] = true;
    for &s in starts {
        if (1..n).contains(&s) {
            keep[s] = true;
        }
    }
    (0..n).filter(|&k| keep[k]).collect()
}

/// The positions in `kept` (original indices, ascending) of the segment
/// `starts` (sorted): the starts of the thinned track.
pub(crate) fn remap_starts(kept: &[usize], starts: &[usize]) -> Vec<u32> {
    kept.iter()
        .enumerate()
        .filter(|(_, k)| starts.binary_search(k).is_ok())
        .map(|(pos, _)| pos as u32)
        .collect()
}

/// A track of `n` fixes thinned to about `max_points` for a save or a crash
/// recovery (the shell's recorder): the indices [`decimation_keep`] keeps and
/// the segment `starts` (any order; one outside `1..n` opens nothing) as
/// positions among them ([`remap_starts`]).
pub fn thin_track(n: usize, max_points: usize, starts: &[u32]) -> (Vec<usize>, Vec<u32>) {
    let mut starts: Vec<usize> = starts
        .iter()
        .map(|&s| s as usize)
        .filter(|s| (1..n).contains(s))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let kept = decimation_keep(n, max_points, &starts);
    let remapped = remap_starts(&kept, &starts);
    (kept, remapped)
}

/// Nominal GNSS range error (m) a GPX `<hdop>` multiplies into a horizontal
/// accuracy: the export writes `accuracy_m / 5` and the importer reads
/// `hdop × 5` ([`hdop_accuracy_m`]), so a fix's accuracy round-trips.
const GPX_HDOP_UERE_M: f64 = 5.0;

/// The `<hdop>` a fix's accuracy is exported as. Divided in f64 and written as
/// the shortest f64 decimal: the importer's `hdop × 5` then lands within one
/// f64 ulp of the f32 accuracy, which rounds back to it exactly, so a route
/// rebuilt from the file (a re-import) keeps every `accuracy_m` bit for bit.
pub(crate) fn gpx_hdop(accuracy_m: f32) -> f64 {
    f64::from(accuracy_m) / GPX_HDOP_UERE_M
}

/// A GPX `<hdop>` as an estimated accuracy in metres ([`GPX_HDOP_UERE_M`]): a
/// coarse estimate that only feeds the QC gate. `None` when the dilution is
/// not a finite positive number.
pub(crate) fn hdop_accuracy_m(hdop: f64) -> Option<f64> {
    (hdop.is_finite() && hdop > 0.0).then_some(hdop * GPX_HDOP_UERE_M)
}

/// XML namespace of lollop's own GPX extension elements. The importer
/// (`import::gpx`) matches the elements by local name.
const GPX_LOLLOP_NS: &str = "urn:lollop:gpx:1";

/// Serialise a fix track to a GPX 1.1 document (the format Strava, Garmin
/// Connect, Komoot, etc. import), one `<trkseg>` per recording segment at the
/// true coordinates, so a paused relocation shows as a gap. `observed_at` is
/// rendered as RFC 3339 UTC in-core, never from a live clock. Elevation is
/// omitted: the shell records no altitude.
///
/// `logged_at` is the run's logged-at stamp (its identity with the entry id).
/// It is written as `<trk><extensions><lollop:logged_at>` because the track's
/// last fix is not that stamp: QC drops a standing-still finish and a trim cuts
/// the end while the run keeps its stamp. Re-importing the file then stamps the
/// run as the one it came from, so the importer recognises it as already logged.
pub fn export_gpx(
    points: &[GpsPoint],
    track_name: &str,
    segment_starts: &[u32],
    logged_at: Option<i64>,
) -> String {
    let mut s = String::with_capacity(256 + points.len() * 96);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(
        "<gpx version=\"1.1\" creator=\"lollop\" \
         xmlns=\"http://www.topografix.com/GPX/1/1\"",
    );
    if logged_at.is_some() {
        let _ = write!(s, " xmlns:lollop=\"{GPX_LOLLOP_NS}\"");
    }
    s.push_str(">\n");
    s.push_str("  <trk>\n    <name>");
    xml_escape_into(&mut s, track_name);
    s.push_str("</name>\n");
    if let Some(at) = logged_at {
        // GPX 1.1 `trkType`: `<extensions>` comes after `<name>`, before `<trkseg>`.
        s.push_str("    <extensions><lollop:logged_at>");
        s.push_str(&unix_to_rfc3339_utc(at));
        s.push_str("</lollop:logged_at></extensions>\n");
    }
    for seg in segments(points, segment_starts) {
        if seg.is_empty() {
            continue;
        }
        s.push_str("    <trkseg>\n");
        for p in seg {
            s.push_str("      <trkpt lat=\"");
            // Shortest round-trip decimal (`{}`, never exponent notation), so a
            // re-imported route is bit-identical.
            let _ = write!(s, "{}", p.lat);
            s.push_str("\" lon=\"");
            let _ = write!(s, "{}", p.lon);
            s.push_str("\"><time>");
            s.push_str(&unix_to_rfc3339_utc(p.observed_at));
            s.push_str("</time>");
            if p.accuracy_m.is_finite() && p.accuracy_m > 0.0 {
                let _ = write!(s, "<hdop>{}</hdop>", gpx_hdop(p.accuracy_m));
            }
            s.push_str("</trkpt>\n");
        }
        s.push_str("    </trkseg>\n");
    }
    s.push_str("  </trk>\n</gpx>\n");
    s
}

/// Append `text` to `out`, escaping the five XML predefined entities so a track
/// name can never break the document.
fn xml_escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (UTC). A year outside
/// 0000–9999 takes the ISO 8601 expanded form java.time's `Instant` writes and
/// the importer (`import::time::parse_import_time_sec`) reads: an explicit sign
/// and at least four digits (`+10000-01-01T00:00:00Z`, `-0001-…`), never a bare
/// five-digit year it would read as no timestamp.
pub(crate) fn unix_to_rfc3339_utc(secs: i64) -> String {
    let days = secs.div_euclid(crate::day::DAY_SEC);
    let tod = secs.rem_euclid(crate::day::DAY_SEC);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = crate::day::civil_from_days(days);
    let sign = match y {
        10_000.. => "+",
        ..0 => "-",
        _ => "",
    };
    let y = y.unsigned_abs();
    format!("{sign}{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests;
