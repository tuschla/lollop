use super::*;

/// File 07 GPS quality gates over a fix track, applied BEFORE any distance /
/// duration / split is derived. Returns the surviving fixes, the dropped
/// count (accuracy-gated fixes included), the remapped segment starts and,
/// per survivor, the last jitter-dropped fix the runner was still standing at
/// before it (see [`moving_duration_min`]). Gates, in order per surviving
/// pair: a non-advancing timestamp (speed undefined), an implied speed over
/// 12 m/s (`load::gps_speed_plausible`, impossible for a runner), and a move
/// under 2.5 m (`load::gps_point_accept`, the Apple jitter/auto-pause pattern;
/// the vertical-rate arm passes 0.0 because fixes carry no altitude).
pub(in crate::app) fn qc_track(points: &[GpsPoint], segment_starts: &[u32]) -> QcFixes {
    let mut dropped = 0u32;
    let mut out: Vec<GpsPoint> = Vec::with_capacity(points.len());
    let mut still: Vec<Option<GpsPoint>> = Vec::with_capacity(points.len());
    // The latest <2.5 m-dropped fix still below the auto-pause floor measured
    // from the last survivor: the runner was standing there, so the leg into
    // the next survivor only starts moving from it.
    let mut standing: Option<GpsPoint> = None;
    // Segment-start indices REMAPPED to positions in the QC'd output, a boundary
    // fix may itself be dropped, so the first survivor at/after it opens the new
    // segment. Passed on to the segment-aware `running::*_seg` fns and the GPX
    // export so a pause + relocation is neither summed nor drawn across.
    let mut out_starts: Vec<u32> = Vec::new();
    // A boundary seen in the raw stream but not yet committed (its fix and any
    // until the next survivor may be dropped). While pending, the inter-fix QC
    // gates are SUPPRESSED: the leg across a pause bridge is a legitimate
    // relocation teleport, not a jitter fix, and gating it would wrongly drop the
    // first fix of the new segment.
    let mut pending_boundary = false;
    for (idx, p) in points.iter().enumerate() {
        if segment_starts.contains(&(idx as u32)) {
            pending_boundary = true;
        }
        // Accuracy gate, same as `usable_track`.
        if p.accuracy_m > running::MAX_GPS_ACCURACY_M {
            dropped += 1;
            continue;
        }
        if !pending_boundary && let Some(last) = out.last() {
            let dt = p.observed_at.saturating_sub(last.observed_at);
            if dt <= 0 {
                dropped += 1;
                continue;
            }
            let dist_m = running::haversine_m(*last, *p);
            let speed = dist_m / dt as f64;
            if !load::gps_speed_plausible(speed) {
                dropped += 1;
                continue;
            }
            if !load::gps_point_accept(dist_m, 0.0) {
                if running::leg(*last, *p).is_none() {
                    standing = Some(*p);
                }
                dropped += 1;
                continue;
            }
        }
        // First survivor of a new segment (index 0 is never a boundary → the
        // `!out.is_empty()` guard mirrors `running::segments` ignoring index 0).
        if pending_boundary && !out.is_empty() {
            out_starts.push(out.len() as u32);
        }
        pending_boundary = false;
        out.push(*p);
        still.push(standing.take());
    }
    QcFixes {
        track: out,
        dropped,
        seg_starts: out_starts,
        still,
    }
}

/// [`qc_track`]'s output.
pub(in crate::app) struct QcFixes {
    pub(in crate::app) track: Vec<GpsPoint>,
    pub(in crate::app) dropped: u32,
    pub(in crate::app) seg_starts: Vec<u32>,
    /// Per survivor: where the runner last stood still before it, if a
    /// standing spell was merged into the leg entering it.
    pub(in crate::app) still: Vec<Option<GpsPoint>>,
}

/// Moving time over a QC'd track, minutes: interval seconds where the implied
/// speed clears the File 07 stop gate (`load::is_stopped`, <0.5 m/s counts as
/// stopped, the auto-pause rule), so standing time never dilutes pace. A leg
/// that swallowed a standing spell (its fixes dropped as <2.5 m jitter) only
/// moves from the last fix the runner stood at, so the stop is not counted
/// as running time.
pub(in crate::app) fn moving_duration_min(qc: &QcFixes) -> f64 {
    let mut sec = 0.0;
    // Sum moving legs WITHIN each segment: the pause-bridge leg between segments
    // is never formed, so a paused relocation adds no moving time. Empty
    // `segment_starts` → one segment → the whole-track sum, unchanged.
    let mut offset = 0;
    for seg in running::segments(&qc.track, &qc.seg_starts) {
        for (i, w) in seg.windows(2).enumerate() {
            let from = qc.still[offset + i + 1].unwrap_or(w[0]);
            if let Some((dt, _)) = running::leg(from, w[1]) {
                sec += dt;
            }
        }
        offset += seg.len();
    }
    sec / 60.0
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
    // Small enum-or-None: hash a stable discriminant code, never a `format!`
    // allocation (this runs per run per view across several readers).
    let wt_code: u8 = match r.workout_type {
        None => 0,
        Some(crate::schema::WorkoutType::Steady) => 1,
        Some(crate::schema::WorkoutType::Interval) => 2,
        Some(crate::schema::WorkoutType::Tempo) => 3,
        Some(crate::schema::WorkoutType::LongRun) => 4,
        Some(crate::schema::WorkoutType::Recovery) => 5,
        Some(crate::schema::WorkoutType::Easy) => 6,
    };
    wt_code.hash(&mut h);
    r.session_rpe.hash(&mut h);
    r.avg_hr_bpm.map(f64::to_bits).hash(&mut h);
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
    /// The File 07 GPS QC pass (`qc_track`, an O(points) haversine scan) plus
    /// the distance and moving-time derivations, keyed by the run's content
    /// fingerprint like [`RUN_VIEW_CACHE`].
    pub(in crate::app) static QC_TRACK_CACHE: std::cell::RefCell<Memo<u64, std::rc::Rc<QcTrack>>> =
        std::cell::RefCell::new(Memo::new(RUN_MEMO_CAP));
}

/// The QC'd track + derived distance/moving-time for one logged run whose
/// content key the caller already holds, memoized.
pub(in crate::app) fn run_qc_keyed(r: &LoggedRun, key: u64) -> std::rc::Rc<QcTrack> {
    memoized(&QC_TRACK_CACHE, key, || {
        let qc = qc_track(&r.track, &r.track_segment_starts);
        let distance_km =
            running::track_distance_km_seg(&qc.track, running::MAX_GPS_ACCURACY_M, &qc.seg_starts);
        let moving_min = moving_duration_min(&qc);
        std::rc::Rc::new(QcTrack {
            track: qc.track,
            dropped: qc.dropped,
            seg_starts: qc.seg_starts,
            distance_km,
            moving_min,
        })
    })
}
