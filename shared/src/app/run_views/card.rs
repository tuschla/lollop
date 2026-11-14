use crate::app::format::{fmt_mss, is_ratio_base_km, round_tenths, whole_seconds};
use crate::app::model::LoggedRun;
use crate::app::run_metrics::{QcTrack, RunFacts, run_started_at};
use crate::app::view_types::{
    EvidenceTagView, IntervalVerdictView, PaceVariability, RunResultView, RunSplitView,
    SplitVerdict, SplitVerdictView, zone_label,
};
use crate::feedback::FeedbackCategory;
use crate::gps::GpsPoint;
use crate::memo::{MemoId, fingerprint, memoized};
use crate::profile::DistanceUnit;
use crate::schema::ThreeZone;
use crate::{feedback, gps, load, running};

/// Build the shell-facing per-unit split views for a QC-gated track (unit metres
/// = [`running::KM_M`] or [`running::MILE_M`]). `distance_km` carries the
/// cumulative distance in km for both unit systems.
pub(in crate::app) fn run_split_views(
    track: &[GpsPoint],
    unit_m: f64,
    segment_starts: &[u32],
) -> Vec<RunSplitView> {
    gps::track_splits(track, unit_m, segment_starts)
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

/// The ordered runs list for the ViewModel, memoized ([`MemoId::RunViews`]) on
/// the run keys and `unit` so an event that touches no run skips the sort and
/// per-run lookups, in [`RunFacts::chrono`] order: chronological, tie-broken by
/// `entry_id` (shell epoch-MILLIS log id)
/// so same-second runs keep a deterministic order across an `AmendRun`
/// remove+push (the amended run carries its original `entry_id` to the Vec
/// tail). Same-second rows sharing an id (an old build's id-less pair) keep
/// insertion order; a backdated run slots into its true position.
pub(in crate::app) fn run_views(
    runs: &[LoggedRun],
    facts: &RunFacts,
    unit: DistanceUnit,
) -> Vec<RunResultView> {
    memoized(MemoId::RunViews, &(fingerprint(&facts.keys), unit), || {
        facts
            .chrono
            .iter()
            .map(|&i| run_card(&runs[i], facts.keys[i], facts.qc[i].as_deref(), unit).view)
            .collect()
    })
}

/// A run's card with its splits in km, plus its per-mile splits and the
/// measured split behind its pacing verdict: what [`MemoId::RunCard`] holds,
/// so a unit change re-uses the derivation.
#[derive(Clone)]
pub(in crate::app) struct RunCard {
    pub(in crate::app) view: RunResultView,
    splits_mi: Vec<RunSplitView>,
    /// Second-half pace slowdown percent of a GPS track (a positive split;
    /// positive = slowed in the back half). `None` for a hand-entered run or a
    /// track too short/degenerate to split. Feeds `split` and the pacing
    /// feedback.
    pub(in crate::app) split_pct: Option<f64>,
}

/// One run's card with its splits in `unit`, memoized on its content key
/// ([`MemoId::RunCard`]): the derivation (QC'd splits per km and per mile,
/// variability index, copy) runs once per distinct run rather than on every
/// `view()`; an amend changes the key and misses. `qc` is the run's QC'd track
/// (`None` for a hand-entered run).
pub(in crate::app) fn run_card(
    r: &LoggedRun,
    key: u64,
    qc: Option<&QcTrack>,
    unit: DistanceUnit,
) -> RunCard {
    let mut card = memoized(MemoId::RunCard, &key, || to_run_view_uncached(r, qc));
    if unit == DistanceUnit::Mi {
        card.view.splits = std::mem::take(&mut card.splits_mi);
    }
    card
}

/// The uncached run-view derivation over the run's QC'd track (`None` for a
/// hand-entered run). The run CARD's spike chip judges against the per-row
/// `longest_recent_km`, which the reducer re-derives from the surviving runs
/// (floored by a tracker's caller-supplied paired history, see
/// `ingest::refresh_spike_baselines`). The SAFETY GATE that can defer training
/// derives its baseline at view() time from `model.runs` (see
/// [`latest_run_spike`]).
fn to_run_view_uncached(r: &LoggedRun, qc: Option<&QcTrack>) -> RunCard {
    // The feedback gate's baseline rule (`is_ratio_base_km`): a base under it
    // is no earlier run, so card and gate always give one spike verdict.
    let has_baseline = is_ratio_base_km(r.longest_recent_km);
    let recent_longest_km = if has_baseline {
        r.longest_recent_km
    } else {
        0.0
    };
    // A negative figure is no reading, like the 0 sentinel; the core clamps
    // regardless of the shell's import clamp.
    let hr_pct_max = r.hr_pct_max.clamp(0.0, crate::running::HR_PCT_MAX_CEILING);
    let gps = qc.is_some();
    // File 07 QC gates ran BEFORE any derivation (accuracy, implausible speed,
    // jitter, non-advancing time).
    let (track, seg_starts): (&[GpsPoint], &[u32]) = match qc {
        Some(q) => (&q.track, &q.seg_starts),
        None => (&[], &[]),
    };

    // A GPS run whose fixes all fail the QC gates has no measurable route:
    // distance/duration collapse to 0, which would otherwise render as a
    // "0.0km @ -" entry *and* trip the spike gate against a phantom baseline.
    // Report the signal problem instead of a zero-distance run.
    if gps && track.len() < 2 {
        return RunCard {
            view: unmeasurable_gps_view(r, hr_pct_max),
            splits_mi: Vec::new(),
            split_pct: None,
        };
    }

    // GPS duration is MOVING time (the `gps` auto-pause gate excludes stopped
    // intervals), so a paused run's pace reflects running, not standing.
    let (distance_km, duration_min) = match qc {
        Some(q) => (q.distance_km, q.moving_min),
        None => (r.distance_km, r.duration_min),
    };

    let shown_zone = displayed_zone(hr_pct_max, r.meta.session_rpe);
    let spike = running::single_session_spike_flag(distance_km, recent_longest_km);

    let pace = run_pace_label(distance_km, duration_min);

    let split_pct = if gps {
        gps::track_positive_split_pct(track, seg_starts)
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
    // is still kept).
    let full_km_splits = splits_km.iter().filter(|s| !s.partial).count();
    let split = split_pct
        .filter(|_| full_km_splits >= MIN_FULL_SPLITS_FOR_PACING_VERDICT)
        .map(split_verdict_view);

    // Interval-vs-steady differentiation (RUN-INTERVAL-VI-001): the variability
    // index (normalized speed ÷ average speed) rates a reps-plus-recovery run
    // above a steady run of the *same average pace*. GPS-only: a hand-entered
    // run has no per-point speed series to compute it from.
    let variability_index = if gps {
        gps::track_variability_index(track, seg_starts)
    } else {
        None
    };

    let split_note = split_note_suffix(split.as_ref().and(split_pct));
    let spike_note = spike_note_text(spike.value, has_baseline);
    let spike_seg = if spike_note.is_empty() {
        String::new()
    } else {
        format!(": {spike_note}")
    };

    let view = RunResultView {
        zone: shown_zone,
        distance_km,
        duration_min,
        hr_pct_max,
        hr_label: hr_card_label(r.meta.avg_hr_bpm, hr_pct_max, shown_zone),
        avg_hr_bpm: r.meta.avg_hr_bpm,
        spike_flag: spike.value,
        spike_note: spike_note.to_string(),
        split,
        interval: variability_index.map(interval_verdict_view),
        workout_type: r.meta.workout_type,
        summary: format!(
            "{}{:.1}km @ {} ({}){}{}",
            if gps { "GPS " } else { "" },
            round_tenths(distance_km),
            pace,
            zone_label(shown_zone),
            spike_seg,
            split_note,
        ),
        evidence: EvidenceTagView::of(&spike),
        splits: splits_km,
        // The ~0.5 MB GPX document never rides the list payload. A GPS run
        // reaching here has ≥2 usable fixes (the degenerate case returned
        // above), so it HAS an exportable route; the shell fetches the document
        // on demand through the bridge's `ExportGpx` op ([`export_run_gpx`])
        // only for the one run it inspects.
        has_gpx: track.len() >= 2,
        observed_at: r.observed_at,
        entry_id: r.entry_id,
        spike_has_baseline: has_baseline,
        session_rpe: r.meta.session_rpe,
        // Session-RPE load (Foster; LOAD-SRPE-001): moving minutes × CR-10.
        // Its own figure, never mixed into the heart-rate load.
        srpe_load_au: r
            .meta
            .session_rpe
            .filter(|_| duration_min > 0.0)
            .map(|rpe| round_tenths(load::srpe_load_au(duration_min, rpe))),
        // Best-effort records depend on OTHER data (the rest of the history),
        // so they are filled after the per-run memo by `annotate_run_views`.
        best_effort_labels: Vec::new(),
        started_at: run_started_at(r, qc),
    };
    RunCard {
        view,
        splits_mi,
        split_pct,
    }
}

/// The card for a GPS run whose fixes all fail the QC gates: no distance, pace,
/// zone or spike verdict, only the signal problem and the echoed inputs.
fn unmeasurable_gps_view(r: &LoggedRun, hr_pct_max: f64) -> RunResultView {
    RunResultView {
        zone: None,
        distance_km: 0.0,
        duration_min: 0.0,
        hr_pct_max,
        spike_flag: false,
        spike_note: String::new(),
        split: None,
        interval: None,
        workout_type: r.meta.workout_type,
        summary: "GPS signal too poor to measure this run".to_string(),
        // No measurable distance → the spike gate never ran, so no claim was
        // used and the evidence tag stays empty.
        evidence: EvidenceTagView::default(),
        has_gpx: false,
        observed_at: r.observed_at,
        entry_id: r.entry_id,
        splits: Vec::new(),
        spike_has_baseline: is_ratio_base_km(r.longest_recent_km),
        hr_label: hr_card_label(r.meta.avg_hr_bpm, hr_pct_max, None),
        avg_hr_bpm: r.meta.avg_hr_bpm,
        session_rpe: r.meta.session_rpe,
        srpe_load_au: None,
        best_effort_labels: Vec::new(),
        started_at: run_started_at(r, None),
    }
}

fn displayed_zone(hr_pct_max: f64, session_rpe: Option<u8>) -> Option<ThreeZone> {
    // Zone is a heart-rate classification (File 04). With no HR sample the
    // zone is unknown (`None`), never a default Z1.
    let has_hr = hr_pct_max > 0.0;
    let zone = running::classify_three_zone(hr_pct_max);
    // running-018 (RUN-TEMPO-001): tempo work is rated RPE 6–7. A session the
    // athlete rated at or above that floor was not an easy effort, so a Z1 label
    // from the average HR alone (a low HRmax estimate, a strap dropout) would
    // contradict their own rating: the zone is left off instead.
    let tempo_rpe_floor = running::workout_rx(crate::schema::RunSessionType::Tempo)
        .value
        .rpe
        .map(|(lo, _)| lo);
    let effort_contradicts_z1 = matches!(zone, crate::schema::ThreeZone::Z1)
        && matches!((session_rpe, tempo_rpe_floor), (Some(rpe), Some(floor)) if rpe >= floor);
    (has_hr && !effort_contradicts_z1).then_some(zone)
}

fn run_pace_label(distance_km: f64, duration_min: f64) -> String {
    if distance_km > 0.0 && duration_min > 0.0 {
        format!("{}/km", fmt_mss((duration_min * 60.0) / distance_km))
    } else {
        "-".to_string()
    }
}

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

fn spike_note_text(spike: bool, has_baseline: bool) -> &'static str {
    // The spike gate errs safe with no history (see single_session_spike), so a
    // first run trips it; the note says why.
    if spike {
        if has_baseline {
            "Longer than your recent longest run"
        } else {
            "No earlier run to compare this distance with yet"
        }
    } else {
        ""
    }
}

const MIN_FULL_SPLITS_FOR_PACING_VERDICT: usize = 2;

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
        // feedback-017: even or negative split.
        _ if split_pct < -feedback::POSITIVE_SPLIT_FLAG_PCT => (
            SplitVerdict::Negative,
            format!("NEG SPLIT {:.0}%", split_pct.abs()),
            format!("Second half {:.0}% faster than the first.", split_pct.abs()),
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
/// VI is the flat-ground GOVSS/NGP simplification, deliberately Weak-graded.
pub(in crate::app) fn interval_verdict_view(vi: f64) -> IntervalVerdictView {
    let r = running::interval_verdict(vi);
    // The interval/steady cutoff is an engine heuristic with no cited threshold.
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
        evidence: EvidenceTagView::of(&r),
    }
}
