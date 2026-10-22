use super::*;

/// Build the shell-facing per-unit split views for a QC-gated track (unit metres
/// = [`running::KM_M`] or [`running::MILE_M`]). `distance_km` carries the
/// cumulative distance in km for both unit systems.
pub(in crate::app) fn run_split_views(
    track: &[GpsPoint],
    unit_m: f64,
    segment_starts: &[u32],
) -> Vec<RunSplitView> {
    running::track_splits_seg(track, running::MAX_GPS_ACCURACY_M, unit_m, segment_starts)
        .into_iter()
        .map(|s| {
            // Round to whole seconds ONCE (`whole_seconds`, the display rule) so
            // the `m:ss` string and the numeric `seconds_per_unit` (for bar
            // lengths) derive from the same value and can never disagree.
            let seconds_per_unit =
                whole_seconds(s.pace_sec_per_unit).min(u64::from(u32::MAX)) as u32;
            RunSplitView {
                index: s.index,
                pace: fmt_mss(f64::from(seconds_per_unit)),
                seconds_per_unit,
                distance_km: s.cumulative_m / 1000.0,
                partial: s.partial,
            }
        })
        .collect()
}

thread_local! {
    /// The finished per-run card keyed by the run's content fingerprint: the
    /// derivation (QC'd splits per km and per mile, variability index, copy) is
    /// a pure function of the run, which is immutable once written; an amend
    /// changes the key and misses.
    pub(in crate::app) static RUN_VIEW_CACHE: std::cell::RefCell<Memo<u64, RunResultView>> =
        std::cell::RefCell::new(Memo::new(RUN_MEMO_CAP));
}

thread_local! {
    /// Test-only counter of UNCACHED run-view builds, thread-local so a laziness
    /// test measures a race-free delta around two `view()` calls (see the
    /// run-view laziness test). Not compiled into shipping builds.
    #[cfg(test)]
    pub(in crate::app) static RUN_VIEW_BUILDS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Cheap content fingerprint of the logged runs: length plus each run's
/// [`run_content_key`] (which already covers every field the derived view uses).
pub(in crate::app) fn runs_fingerprint(keys: &[u64]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    keys.len().hash(&mut h);
    for k in keys {
        k.hash(&mut h);
    }
    h.finish()
}

thread_local! {
    /// The finished ordered runs list keyed by [`runs_fingerprint`], so an
    /// event that touches no run skips the sort and per-run lookups.
    static RUNS_VIEWS_CACHE: std::cell::RefCell<Memo<u64, Vec<RunResultView>>> =
        std::cell::RefCell::new(Memo::new(VIEW_LIST_CACHE_CAP));
}

/// The ordered runs list for the ViewModel, memoized. Chronological order,
/// tie-broken by `entry_id` (shell epoch-MILLIS log id) so same-second runs keep
/// a deterministic order across an `AmendRun` remove+push (the amended run
/// carries its original `entry_id` to the Vec tail). Legacy rows
/// (`entry_id == 0`) keep the stable sort's insertion order; a backdated run
/// slots into its true position.
pub(in crate::app) fn run_views(runs: &[LoggedRun], facts: &RunFacts) -> Vec<RunResultView> {
    memoized(&RUNS_VIEWS_CACHE, runs_fingerprint(&facts.keys), || {
        let mut ordered: Vec<(&LoggedRun, u64, Option<&QcTrack>)> = runs
            .iter()
            .zip(&facts.keys)
            .zip(&facts.qc)
            .map(|((r, &k), q)| (r, k, q.as_deref()))
            .collect();
        ordered.sort_by(|(a, ..), (b, ..)| {
            a.observed_at
                .cmp(&b.observed_at)
                .then(a.entry_id.cmp(&b.entry_id))
        });
        ordered
            .into_iter()
            .map(|(r, k, q)| to_run_view(r, k, q))
            .collect()
    })
}

/// One run's card, memoized on its content key: the derivation runs once per
/// distinct run rather than on every `view()`. `qc` is the run's QC'd track
/// (`None` for a hand-entered run).
pub(in crate::app) fn to_run_view(r: &LoggedRun, key: u64, qc: Option<&QcTrack>) -> RunResultView {
    memoized(&RUN_VIEW_CACHE, key, || to_run_view_uncached(r, qc))
}

/// Physiological ceiling for a run's average %HRmax echo/classification. A stale
/// or low `measured_hr_max` paired with a hard effort can push the imported
/// average above 100 % (e.g. 104.7 %), which is physiologically impossible.
/// `Gpx.kt` already clamps at import, but the core is the single trust boundary
/// and cannot assume it: a pre-clamp persisted run replays through here, a
/// non-Android or future shell may skip the import clamp, and a hand-edited log
/// line can carry any value. Clamp here so no >100 % figure is ever echoed to a
/// shell or fed to `classify_three_zone`. (The Lucia zone bucket is >=88 % = Z3
/// either way, so this cannot alter a classification; it only stops an absurd
/// percentage from surfacing.)
const MAX_HR_PCT_MAX: f64 = 100.0;

/// The uncached run-view derivation over the run's QC'd track (`None` for a
/// hand-entered run). The run CARD's spike chip judges against the per-row
/// `longest_recent_km` baked at ingest (which honors a tracker's
/// caller-supplied paired history). The SAFETY GATE that can defer training
/// derives its baseline fresh at view() time from `model.runs` instead (see
/// [`latest_run_spike`]).
fn to_run_view_uncached(r: &LoggedRun, qc: Option<&QcTrack>) -> RunResultView {
    #[cfg(test)]
    RUN_VIEW_BUILDS.with(|c| c.set(c.get() + 1));
    let recent_longest_km = r.longest_recent_km;
    // Clamp the stored %HRmax to the physiological ceiling before it is echoed
    // or classified (see MAX_HR_PCT_MAX): the core never trusts the import-side
    // clamp alone.
    let hr_pct_max = r.hr_pct_max.min(MAX_HR_PCT_MAX);
    // A GPS track derives its own distance/duration; a manual run uses scalars.
    let gps = qc.is_some();
    // File 07 QC gates ran BEFORE any derivation (accuracy, implausible speed,
    // jitter, non-advancing time); the dropped count is surfaced to the shell.
    let (track, qc_dropped, seg_starts): (&[GpsPoint], u32, &[u32]) = match qc {
        Some(q) => (&q.track, q.dropped, &q.seg_starts),
        None => (&[], 0, &[]),
    };

    // A GPS run whose fixes all fail the QC gates has no measurable route:
    // distance/duration collapse to 0, which would otherwise render as a
    // "0.0km @ -" entry *and* trip the spike gate against a phantom baseline.
    // Report the signal problem honestly instead of fabricating a null run.
    if gps && track.len() < 2 {
        return unmeasurable_gps_view(r, hr_pct_max, qc_dropped);
    }

    // GPS duration is MOVING time (File 07 auto-pause: <0.5 m/s intervals are
    // excluded), so a paused run's pace reflects running, not standing.
    let (distance_km, duration_min) = match qc {
        Some(q) => (q.distance_km, q.moving_min),
        None => (r.distance_km, r.duration_min),
    };

    let shown_zone = displayed_zone(hr_pct_max, r.session_rpe);
    let spike = running::single_session_spike_flag(distance_km, recent_longest_km);

    let pace = run_pace_label(distance_km, duration_min);

    let split_pct = if gps {
        running::track_positive_split_pct_seg(track, running::MAX_GPS_ACCURACY_M, seg_starts)
    } else {
        None
    };

    // Per-km and per-mile splits from the SAME QC-gated track as distance/pace
    // (GPS runs only; a hand-entered run has no track to walk). The core owns the
    // `m:ss` pace formatting so a shell renders each split verbatim.
    let (splits_km, splits_mi) = if gps {
        (
            run_split_views(track, running::KM_M, seg_starts),
            run_split_views(track, running::MILE_M, seg_starts),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    // A pacing verdict judges the run's halves against each other; with fewer
    // than two full km splits there is too little run to praise or fault the
    // pacing, so no verdict or pacing note is shown (the measured `split_pct`
    // still is).
    let full_km_splits = splits_km.iter().filter(|s| !s.partial).count();
    let split = split_pct
        .filter(|_| full_km_splits >= MIN_FULL_SPLITS_FOR_PACING_VERDICT)
        .map(split_verdict_view);

    // Interval-vs-steady differentiation (RUN-INTERVAL-VI-001): the variability
    // index (normalized speed ÷ average speed) rates a reps-plus-recovery run
    // above a steady run of the *same average pace*. GPS-only, a hand-entered
    // run has no per-point speed series to compute it from.
    let variability_index = if gps {
        running::track_variability_index_seg(track, running::MAX_GPS_ACCURACY_M, seg_starts)
    } else {
        None
    };

    let split_note = split_note_suffix(split.as_ref().and(split_pct));
    let spike_note = spike_note_text(spike.value, recent_longest_km);
    let spike_seg = if spike_note.is_empty() {
        String::new()
    } else {
        format!(": {spike_note}")
    };

    RunResultView {
        zone: shown_zone,
        pace: pace.clone(),
        distance_km,
        duration_min,
        hr_pct_max,
        hr_label: hr_card_label(r.avg_hr_bpm, hr_pct_max, shown_zone),
        avg_hr_bpm: r.avg_hr_bpm,
        spike_flag: spike.value,
        spike_note: spike_note.to_string(),
        split_pct,
        split,
        interval: variability_index.map(interval_verdict_view),
        workout_type: r.workout_type,
        summary: format!(
            "{}{:.1}km @ {} ({}){}{}",
            if gps { "GPS " } else { "" },
            distance_km,
            pace,
            zone_label(shown_zone),
            spike_seg,
            split_note,
        ),
        citation: spike.reference().to_owned(),
        grade: Some(spike.grade()),
        confidence: spike.confidence(),
        safety_critical: spike.safety_critical(),
        contested: spike.contested().is_some(),
        splits_km,
        splits_mi,
        qc_dropped,
        // The ~0.5 MB GPX document never rides the list payload. A GPS run
        // reaching here has ≥2 usable fixes (the degenerate case returned
        // above), so it HAS an exportable route; the shell fetches the document
        // on demand via the `exportGpx` FFI query (see [`export_run_gpx`]) only
        // for the one run it inspects.
        has_gpx: track.len() >= 2,
        observed_at: r.observed_at,
        entry_id: r.entry_id,
        spike_has_baseline: recent_longest_km > 0.0,
        session_rpe: r.session_rpe,
        // Session-RPE load (Foster; LOAD-SRPE-001): moving minutes × CR-10.
        // Its own figure, never mixed into the heart-rate load.
        srpe_load_au: r
            .session_rpe
            .filter(|_| duration_min > 0.0)
            .map(|rpe| (load::srpe_load_au(duration_min, rpe) * 10.0).round() / 10.0),
        // Best-effort records depend on OTHER data (the rest of the history),
        // so they are filled after the per-run memo by `annotate_run_views`.
        best_effort_labels: Vec::new(),
    }
}

/// The card for a GPS run whose fixes all fail the QC gates: no distance, pace,
/// zone or spike verdict, only the signal problem and the echoed inputs.
fn unmeasurable_gps_view(r: &LoggedRun, hr_pct_max: f64, qc_dropped: u32) -> RunResultView {
    RunResultView {
        zone: None,
        pace: "-".to_string(),
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max,
        spike_flag: false,
        spike_note: String::new(),
        split_pct: None,
        split: None,
        interval: None,
        workout_type: r.workout_type,
        summary: "GPS signal too poor to measure this run".to_string(),
        // No measurable distance → the spike gate never ran; carry an
        // empty evidence tag rather than citing a claim that was not used.
        citation: String::new(),
        grade: None,
        confidence: 0.0,
        safety_critical: false,
        contested: false,
        qc_dropped,
        has_gpx: false,
        observed_at: r.observed_at,
        entry_id: r.entry_id,
        splits_km: Vec::new(),
        splits_mi: Vec::new(),
        spike_has_baseline: r.longest_recent_km > 0.0,
        hr_label: hr_card_label(r.avg_hr_bpm, hr_pct_max, None),
        avg_hr_bpm: r.avg_hr_bpm,
        session_rpe: r.session_rpe,
        // No measurable time → no session-RPE load.
        srpe_load_au: None,
        best_effort_labels: Vec::new(),
    }
}

/// The zone the card shows for the run's average %HRmax, if any.
fn displayed_zone(hr_pct_max: f64, session_rpe: Option<u8>) -> Option<ThreeZone> {
    // Zone is a heart-rate classification (File 04). With no HR sample we do not
    // fabricate one, report it as unknown rather than defaulting to Z1.
    let has_hr = hr_pct_max > 0.0;
    let zone = running::classify_three_zone(hr_pct_max);
    // running-018 (RUN-TEMPO-001): tempo work is rated RPE 6–7. A session the
    // athlete rated at or above that floor was not an easy effort, so a Z1 label
    // from the average HR alone (a low HRmax estimate, a strap dropout) would
    // contradict their own rating: the zone is left off instead.
    let tempo_rpe_floor = running::run_workout_rx(crate::schema::RunSessionType::Tempo)
        .value
        .rpe
        .map(|(lo, _)| lo);
    let effort_contradicts_z1 = matches!(zone, crate::schema::ThreeZone::Z1)
        && matches!((session_rpe, tempo_rpe_floor), (Some(rpe), Some(floor)) if rpe >= floor);
    (has_hr && !effort_contradicts_z1).then_some(zone)
}

/// The card's average pace, `m:ss/km`; "-" when the run has no distance or time.
fn run_pace_label(distance_km: f64, duration_min: f64) -> String {
    if distance_km > 0.0 && duration_min > 0.0 {
        format!("{}/km", fmt_mss((duration_min * 60.0) / distance_km))
    } else {
        "-".to_string()
    }
}

/// The summary's pacing suffix after the zone; empty for an even split.
fn split_note_suffix(split_pct: Option<f64>) -> String {
    // A positive split is a second-half slowdown; a negative value is a faster
    // second half. Thresholds are strict (>3 %, <-3 %) to match
    // `feedback::positive_split_discipline`, so this note and the coaching cue it
    // pairs with never disagree at exactly 3 %.
    match split_pct {
        Some(p) if p > feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · second half {p:.0}% slower")
        }
        Some(p) if p < -feedback::POSITIVE_SPLIT_FLAG_PCT => {
            format!(" · second half {:.0}% faster", p.abs())
        }
        _ => String::new(),
    }
}

/// Why the card carries a SPIKE chip; empty when the spike gate did not trip.
fn spike_note_text(spike: bool, recent_longest_km: f64) -> &'static str {
    // The spike gate errs safe with no history (see single_session_spike), so a
    // user's first run trips it. Say *why* rather than claiming a ">10%" jump over
    // a baseline that does not exist yet, the SPIKE flag itself is unchanged, only
    // the wording is honest about the cause. Held as a field so a shell can render
    // it beside the SPIKE chip without re-parsing `summary`.
    if spike {
        if recent_longest_km > 0.0 {
            "Longer than your recent longest run"
        } else {
            "No earlier run to compare this distance with yet"
        }
    } else {
        ""
    }
}

/// Fewest full km splits a GPS run needs before it gets a pacing verdict.
const MIN_FULL_SPLITS_FOR_PACING_VERDICT: usize = 2;

/// The run card's heart-rate line: the bpm the athlete entered when known,
/// else the whole-percent %HRmax, followed by the zone when one is shown.
/// Empty when the run carries no heart-rate figure at all.
fn hr_card_label(avg_hr_bpm: Option<f64>, hr_pct_max: f64, zone: Option<ThreeZone>) -> String {
    let figure = match avg_hr_bpm {
        Some(bpm) => format!("{bpm:.0} bpm"),
        None if hr_pct_max > 0.0 => format!("HR {hr_pct_max:.0}%"),
        None => return String::new(),
    };
    if zone.is_some() {
        format!("{figure} · {}", zone_label(zone))
    } else {
        figure
    }
}

/// Build the core-owned pacing-verdict chip for a measured split
/// (feedback-016/017; FB-PACING-001). The verdict/message routing reuses
/// `feedback::positive_split_discipline`, so the ~3% line
/// (`feedback::POSITIVE_SPLIT_FLAG_PCT`) is decided in exactly one place: a
/// shell renders `verdict`/`label`/`message` verbatim, never re-deriving the
/// threshold.
pub(in crate::app) fn split_verdict_view(split_pct: f64) -> SplitVerdictView {
    let f = feedback::positive_split_discipline(split_pct);
    let (verdict, label, message) = match f.value {
        // feedback-016: >3% second-half slowdown on an even-effort run → the
        // easier-start cue toward an even-to-negative split.
        FeedbackCategory::IntensityDiscipline => (
            SplitVerdict::Fade,
            format!("FADE +{split_pct:.0}%"),
            format!(
                "Second half {split_pct:.0}% slower than the first. Start easier so the second half is as fast or faster."
            ),
        ),
        // feedback-017: even or negative split → pacing-discipline praise.
        _ if split_pct < -feedback::POSITIVE_SPLIT_FLAG_PCT => (
            SplitVerdict::Negative,
            format!("NEG SPLIT {:.0}%", split_pct.abs()),
            format!(
                "Second half {:.0}% faster than the first. Well paced.",
                split_pct.abs()
            ),
        ),
        _ => (
            SplitVerdict::Even,
            "EVEN SPLIT".to_string(),
            "Both halves at about the same pace.".to_string(),
        ),
    };
    SplitVerdictView {
        verdict,
        label,
        message,
        evidence: EvidenceTagView::of(&f),
    }
}

/// Build the interval-vs-steady chip for a GPS run's variability index
/// (RUN-INTERVAL-VI-001). The `interval`/`steady` cutoff lives in exactly one
/// place, `running::interval_verdict` / `running::INTERVAL_VI_THRESHOLD`, so a
/// shell renders `kind`/`label`/`message` verbatim and never re-derives it. The
/// copy is honest about the grade: this is the flat-ground GOVSS/NGP
/// simplification, deliberately Weak-graded.
pub(in crate::app) fn interval_verdict_view(vi: f64) -> IntervalVerdictView {
    let r = running::interval_verdict(vi);
    // Copy MEASURES, it does not "rate": the variability index is a descriptive
    // label only, no downstream load/adjustment consumes it yet, so claiming the
    // run is "rated above its average" would overstate. The interval/steady cutoff
    // is an engine heuristic (there is no cited threshold), stated as such.
    let (kind, label, message) = if r.value {
        (
            PaceVariability::Interval,
            format!("INTERVAL · VI {vi:.2}"),
            "Your pace rose and fell a lot, like hard efforts with easy recovery between. \
             A steady run's variability index is about 1.0."
                .to_string(),
        )
    } else {
        (
            PaceVariability::Steady,
            format!("STEADY · VI {vi:.2}"),
            "Evenly paced: its average pace reflects the whole run.".to_string(),
        )
    };
    IntervalVerdictView {
        kind,
        label,
        message,
        variability_index: vi,
        evidence: EvidenceTagView::of(&r),
    }
}
