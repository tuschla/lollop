use super::*;

/// File 07 GPS quality gates over a fix track, applied BEFORE any distance /
/// duration / split is derived. Returns the surviving fixes, the dropped
/// count (accuracy-gated fixes included) and the remapped segment starts.
/// Gates, in order per surviving pair: a non-advancing timestamp (speed
/// undefined), an implied speed over 12 m/s (`load::gps_speed_plausible`,
/// impossible for a runner), and a move under 2.5 m (`load::gps_point_accept`,
/// the Apple jitter/auto-pause pattern; the vertical-rate arm passes 0.0
/// because fixes carry no altitude). Stops are judged over a [`StopWindow`],
/// never leg by leg: a stop keeps one fix, so the leg into it is no move and
/// the stride out of it ([`stride_out_start`]) is a normal moving leg.
///
/// The pass ([`qc_pass`]) repeats on its own output until every gate accepts
/// the track unchanged: windows are cut from the raw stream, so a single pass
/// can leave a survivor that a window over the survivors would drop. Each pass
/// keeps an ordered subset of its input, so this ends, and the QC'd track is a
/// fixed point: QC of an exported (QC'd) route (Undo, re-import) gives back the
/// same fixes and figures.
pub(in crate::app) fn qc_track(points: &[GpsPoint], segment_starts: &[u32]) -> QcFixes {
    let mut q = qc_pass(points, segment_starts);
    loop {
        let next = qc_pass(&q.track, &q.seg_starts);
        if next.track.len() == q.track.len() {
            // Same fixes; the boundaries as this pass remapped them.
            return QcFixes {
                dropped: (points.len() - next.track.len()) as u32,
                ..next
            };
        }
        q = next;
    }
}

/// One pass of [`qc_track`]'s gates.
fn qc_pass(points: &[GpsPoint], segment_starts: &[u32]) -> QcFixes {
    let is_boundary = |i: usize| segment_starts.contains(&(i as u32));
    let mut out: Vec<GpsPoint> = Vec::with_capacity(points.len());
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
        // Accuracy gate, same as `usable_track`.
        if p.accuracy_m > running::MAX_GPS_ACCURACY_M {
            i += 1;
            continue;
        }
        let last = match out.last() {
            Some(&s) if !pending_boundary => s,
            // First survivor of the track or of a new segment (index 0 is never
            // a boundary → the `!out.is_empty()` guard mirrors `running::segments`
            // ignoring index 0).
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
            let w = stop_window(points, segment_starts, i, last);
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
                                q.accuracy_m <= running::MAX_GPS_ACCURACY_M
                                    && q.observed_at > last.observed_at
                                    && running::leg(last, q).is_none()
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
        let dist_m = running::haversine_m(last, p);
        if !load::gps_speed_plausible(dist_m / dt as f64) || !load::gps_point_accept(dist_m, 0.0) {
            continue;
        }
        out.push(p);
    }
    QcFixes {
        dropped: (points.len() - out.len()) as u32,
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
    /// Raw index of the window's last STANDING fix: under 2.5 m from `s`
    /// (`load::gps_point_accept`) and below the File 07 auto-pause floor
    /// measured from `s` (`running::leg` is `None`).
    last_standing: Option<usize>,
}

fn stop_window(
    points: &[GpsPoint],
    segment_starts: &[u32],
    from: usize,
    s: GpsPoint,
) -> StopWindow {
    let mut last_standing = None;
    for (j, q) in points.iter().enumerate().skip(from) {
        if j > from && segment_starts.contains(&(j as u32)) {
            return StopWindow {
                end: j,
                leaving: None,
                last_standing,
            };
        }
        if q.accuracy_m > running::MAX_GPS_ACCURACY_M || q.observed_at <= s.observed_at {
            continue;
        }
        let d = running::haversine_m(s, *q);
        if d > stand_radius_m(s, *q) {
            return StopWindow {
                end: j,
                leaving: Some(d),
                last_standing,
            };
        }
        if !load::gps_point_accept(d, 0.0) && running::leg(s, *q).is_none() {
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
/// its own accuracy of the true point), never under the File 07 2.5 m jitter
/// gate.
fn stand_radius_m(s: GpsPoint, q: GpsPoint) -> f64 {
    (f64::from(s.accuracy_m) + f64::from(q.accuracy_m)).max(load::GPS_MIN_MOVE_M)
}

/// Raw index where the stride out of a stop at `s` begins: the longest run of
/// usable fixes after the window's last standing fix `standing` (and before
/// `end`) that ends the window moving steadily AWAY from `s`, each one past the
/// 2.5 m jitter gate and farther from `s` than the one before. `leaving` is the
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
        if q.accuracy_m > running::MAX_GPS_ACCURACY_M || q.observed_at <= s.observed_at {
            continue;
        }
        let d = running::haversine_m(s, q);
        // Negated so a NaN distance ends the stride out instead of joining it.
        if !(d >= load::GPS_MIN_MOVE_M && d < next_d) {
            break;
        }
        next_d = d;
        start = j;
    }
    start
}

/// [`qc_track`]'s output.
pub(in crate::app) struct QcFixes {
    pub(in crate::app) track: Vec<GpsPoint>,
    pub(in crate::app) dropped: u32,
    pub(in crate::app) seg_starts: Vec<u32>,
}

/// Moving time over a QC'd track, minutes: interval seconds where the implied
/// speed clears the File 07 stop gate (`load::is_stopped`, <0.5 m/s counts as
/// stopped, the auto-pause rule), so standing time never dilutes pace. The
/// same `running::leg` gate picks the legs `running::track_distance_km`
/// sums, so distance and moving time always describe the same legs; a
/// standing spell ends at the standing fix [`qc_track`] keeps, so only the
/// stride out of it moves.
pub(in crate::app) fn moving_duration_min(qc: &QcFixes) -> f64 {
    let mut sec = 0.0;
    // Sum moving legs WITHIN each segment: the pause-bridge leg between segments
    // is never formed, so a paused relocation adds no moving time. Empty
    // `segment_starts` → one segment → the whole-track sum, unchanged.
    for seg in running::segments(&qc.track, &qc.seg_starts) {
        for w in seg.windows(2) {
            if let Some((dt, _)) = running::leg(w[0], w[1]) {
                sec += dt;
            }
        }
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
        std::rc::Rc::new(qc_figures(&r.track, &r.track_segment_starts))
    })
}

/// The one derivation of a track's of-record figures: [`qc_track`], then
/// distance and moving time over the same legs.
fn qc_figures(points: &[GpsPoint], segment_starts: &[u32]) -> QcTrack {
    let qc = qc_track(points, segment_starts);
    let distance_km = running::track_distance_km(&qc.track, &qc.seg_starts);
    let moving_min = moving_duration_min(&qc);
    QcTrack {
        track: qc.track,
        dropped: qc.dropped,
        seg_starts: qc.seg_starts,
        distance_km,
        moving_min,
    }
}

/// The distance (km) and moving time (min) a GPS track will be SAVED with,
/// before it is logged: the tracker's Finish review shows these, so the
/// figures the runner confirms are the ones History then lists. Same ingest
/// sanitizing and [`qc_figures`] as a logged `LogRunTrack`, and the same
/// "fewer than two usable fixes measures nothing" rule as its card.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(crate) fn preview_track_figures(points: Vec<GpsPoint>, segment_starts: &[u32]) -> (f64, f64) {
    let q = qc_figures(&crate::app::ingest::sanitize_track(points), segment_starts);
    if q.track.len() < 2 {
        return (0.0, 0.0);
    }
    (q.distance_km, q.moving_min)
}
