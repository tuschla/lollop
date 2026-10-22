//! Flattening a `Prescription::Run` arm and its tracker structure.

use super::*;

/// Render a prescribed run distance: whole km as "8 km", a sub-km step as one
/// decimal ("6.5 km", "4.3 km"), matching the plan's 0.5 km / 0.1 km
/// quantisation. The plan floors to its step, so the one-decimal value is the
/// prescription itself and never reads above the spike gate.
pub(super) fn fmt_run_km(k: f32) -> String {
    let tenths = (f64::from(k) * 10.0).round() / 10.0;
    if (tenths - tenths.round()).abs() < 1e-9 {
        format!("{tenths:.0} km")
    } else {
        format!("{tenths:.1} km")
    }
}

/// Render a C25K run/walk interval set (running-025): `rounds` repeats of a run
/// then a walk, each shown as minutes once it reaches 2 min, e.g.
/// "8 × (60 s run / 90 s walk)".
fn run_walk_label(run_sec: u16, walk_sec: u16, rounds: u8) -> String {
    let secs = |s: u16| -> String {
        match (s / 60, s % 60) {
            (m, 0) if m >= 2 => format!("{m} min"),
            (m, 30) if m >= 2 => format!("{m}.5 min"),
            _ => format!("{s} s"),
        }
    };
    format!(
        "{rounds} × ({} run / {} walk)",
        secs(run_sec),
        secs(walk_sec)
    )
}

/// The numeric structure of a run prescription for the tracker: reps +
/// recovery, warm-up/cool-down, strides and run/walk bouts, straight from the
/// plan's `RunPrescription`. `None` for a plain continuous run.
fn run_structure_view(r: &crate::schema::RunPrescription) -> Option<RunStructureView> {
    let (reps, rep_m, rep_sec) = match r.repeats {
        Some((n, RunVolume::DistanceKm(k))) => (n as u32, (k as f64 * 1000.0).round() as u32, 0),
        Some((n, RunVolume::DurationMin(m))) => (n as u32, 0, m as u32 * 60),
        Some((n, RunVolume::RunWalk { run_sec, .. })) => (n as u32, 0, run_sec as u32),
        None => (0, 0, 0),
    };
    let run_walk: Vec<RunWalkStepView> = if r.segments.is_empty() {
        match r.volume {
            RunVolume::RunWalk {
                run_sec,
                walk_sec,
                rounds,
            } => (0..rounds)
                .map(|_| RunWalkStepView {
                    run_sec: run_sec as u32,
                    walk_sec: walk_sec as u32,
                })
                .collect(),
            _ => Vec::new(),
        }
    } else {
        r.segments
            .iter()
            .map(|s| RunWalkStepView {
                run_sec: s.run_sec as u32,
                walk_sec: s.walk_sec as u32,
            })
            .collect()
    };
    let v = RunStructureView {
        reps,
        rep_m,
        rep_sec,
        recovery_sec: match r.recovery {
            Some(crate::schema::RepRecovery::Seconds(s)) => u32::from(s),
            _ => 0,
        },
        recovery_m: match r.recovery {
            Some(crate::schema::RepRecovery::Metres(m)) => u32::from(m),
            _ => 0,
        },
        recovery_equal_to_rep: r.recovery == Some(crate::schema::RepRecovery::EqualToRep),
        warmup_min: r.warmup_min.unwrap_or(0) as u32,
        cooldown_min: r.cooldown_min.unwrap_or(0) as u32,
        strides_reps: r.strides.map_or(0, |s| s.reps as u32),
        strides_sec: r.strides.map_or(0, |s| s.rep_sec as u32),
        strides_recovery_sec: r.strides.map_or(0, |s| s.recovery_sec as u32),
        run_walk,
    };
    (v != RunStructureView::default()).then_some(v)
}

/// Flatten a `Prescription::Run` arm into its `PrescriptionView`.
pub(super) fn flatten_run_prescription(
    r: &crate::schema::RunPrescription,
    rx: &Recommended<Prescription>,
) -> PrescriptionView {
    let vol = match r.volume {
        RunVolume::DurationMin(m) => format!("{m} min"),
        RunVolume::DistanceKm(k) => fmt_run_km(k),
        RunVolume::RunWalk {
            run_sec,
            walk_sec,
            rounds,
        } => run_walk_label(run_sec, walk_sec, rounds),
    };
    // An interval/repetition run carries per-rep structure; surface
    // it so the card reads "4 × 4 min · Interval pace", not a misleading
    // "16 min · Interval pace" that looks like one continuous VO2max run.
    let rep_volume = r.repeats.map(|(_, rv)| match rv {
        RunVolume::DurationMin(m) => format!("{m} min"),
        RunVolume::DistanceKm(k) => format!("{:.0} m", (k as f64) * 1000.0),
        RunVolume::RunWalk {
            run_sec,
            walk_sec,
            rounds,
        } => run_walk_label(run_sec, walk_sec, rounds),
    });
    let rep_count = r.repeats.map_or(0, |(n, _)| n);
    // A long run re-pointed to RUN-SPIKE-001 / RUN-START-001 (plan/run.rs) is
    // set by the 30-day-longest-run ceiling rather than the weekly share. State
    // the basis honestly (HARD RULE 2) instead of the generic claim statement,
    // no invented numbers, only what the KB entries state.
    let spike_pct = crate::running::SPIKE_FRAC * 100.0;
    let run_basis = match rx.claim_id() {
        claims::RUN_SPIKE_001 => Some(format!(
            "Distance stays within {spike_pct:.0}% of your longest run in the last \
             {LONGEST_RUN_WINDOW_DAYS} days, so no single run jumps past what you have \
             already trained. It can be a bigger share of your week than usual until your \
             weekly distance catches up."
        )),
        claims::RUN_START_001 => Some(format!(
            "The distance stays within {spike_pct:.0}% of the longest run you told us you \
             did in the last month. Once a logged run goes further, or that answer is more \
             than {LONGEST_RUN_WINDOW_DAYS} days old, the plan follows your logged runs."
        )),
        claims::RUN_LONGRUN_TARGET_001 => Some(format!(
            "The long run has reached the most your goal distance needs, or about {:.0} \
             minutes at most. Extra distance goes into the rest of your week.",
            crate::running::LONG_RUN_MAX_MIN
        )),
        claims::RUN_TAPER_001 => Some(
            "Taper week before your race: total distance drops while your hard sessions \
             keep their effort, so you arrive fresh."
                .to_string(),
        ),
        // The anchoring run is now older than the 30-day window, so its
        // credit is tapering (DETRAIN-001) rather than dropping off a cliff.
        claims::DETRAIN_001 => Some(
            "Distance stays near your recent long run, then shrinks gradually as that run \
             gets older. Endurance holds for a couple of weeks and then fades slowly, so the \
             plan steps down slowly too. Log a new long run to reset it."
                .to_string(),
        ),
        _ => None,
    };
    let (safety_reason, safety_reason_mi) = {
        let longest = match rx.claim_id() {
            claims::RUN_SPIKE_001 => Some("your longest recent run"),
            claims::RUN_START_001 => Some("the longest recent run you told us about"),
            _ => None,
        };
        match (longest, r.volume) {
            (None, _) => (String::new(), String::new()),
            (Some(l), RunVolume::DistanceKm(k)) if r.repeats.is_none() => {
                let capped = |d: String| format!("Capped at {d}: at most {spike_pct:.0}% over {l}");
                (capped(fmt_run_km(k)), capped(fmt_mi_of_km(f64::from(k))))
            }
            (Some(l), _) => {
                let kept = format!("Kept short and easy: at most {spike_pct:.0}% over {l}");
                (kept.clone(), kept)
            }
        }
    };
    let label = match r.intensity {
        RunIntensity::HrPercentMax { lo, hi, hrr_hi } => {
            let band = match (lo, hi) {
                (Some(lo), Some(hi)) => format!("{lo:.0}–{hi:.0}% HRmax"),
                (None, Some(hi)) => format!("≤ {hi:.0}% HRmax"),
                (Some(lo), None) => format!("≥ {lo:.0}% HRmax"),
                (None, None) => "% HRmax".into(),
            };
            match hrr_hi {
                Some(hrr) => format!("{band} · ≤ {hrr:.0}% of heart-rate reserve"),
                None => band,
            }
        }
        RunIntensity::Vdot(b) => format!("{} pace", vdot_band_name(b)),
        RunIntensity::ThreeZone(ThreeZone::Z1) => "Zone 1 (easy)".into(),
        RunIntensity::ThreeZone(ThreeZone::Z2) => "Zone 2 (steady)".into(),
        RunIntensity::ThreeZone(ThreeZone::Z3) => "Zone 3 (hard)".into(),
        RunIntensity::PaceSecPerKm(s) => format!("{}/km", fmt_mss(f64::from(s))),
        RunIntensity::PowerPercentCp(p) => format!("{p:.0}% of critical power"),
    };
    // Rep-structured runs lead with the honest "N × <rep> · <pace>";
    // continuous runs keep the whole-session "<vol> · <pace>".
    let run_volume = r.repeats.is_none().then(|| RunAmount::of(r.volume));
    let head = match &rep_volume {
        Some(rv) => format!("{rep_count} × {rv}"),
        None => vol,
    };
    let summary = format!("{head} · {label}");
    PrescriptionView {
        summary,
        exercise: String::new(),
        sets: 0,
        reps_low: 0,
        reps_high: 0,
        load_kg: None,
        intensity_label: label,
        // A run target (pace/zone/HRmax) is never RIR/load-adjustable.
        intensity: PlanIntensity::Prose,
        rest_sec: 0,
        rep_count,
        rep_volume: rep_volume.unwrap_or_default(),
        timed_hold: false,
        target_rpe: None,
        hold_low_sec: 0,
        hold_high_sec: 0,
        added_kg: None,
        anchored_on: String::new(),
        anchored_on_last: false,
        anchored_on_single_set: false,
        adjusted_note: String::new(),
        return_check: false,
        safety_reason,
        safety_reason_mi,
        evidence: EvidenceTagView::of(rx),
        why: why_from(run_basis, rx),
        done: false,
        sets_done: 0,
        logged_sets: Vec::new(),
        ended_early: false,
        run_logged_km: 0.0,
        run_logged_min: 0.0,
        run_target: None,
        calis: None,
        last_set: None,
        suggested_load_kg: None,
        plates_per_side: Vec::new(),
        effort_cue: String::new(),
        run_structure: run_structure_view(r),
        face_name: String::new(),
        face_detail: String::new(),
        face_lead: head.clone(),
        run_head: head,
        run_head_km: run_volume.and_then(RunAmount::shown_km),
        status: ItemStatus::Open,
        run_volume,
        rep_amount: r.repeats.map(|(_, rv)| RunAmount::of(rv)),
        run_lighter: false,
        rpe_target: false,
        hold_load_cap_kg: None,
        pick_load: false,
        basis_load_kg: None,
        load_held: false,
    }
}

/// A Daniels training band's name as the run card shows it ("Threshold pace").
pub(in crate::app) fn vdot_band_name(b: VdotBand) -> &'static str {
    match b {
        VdotBand::Easy => "Easy",
        VdotBand::Marathon => "Marathon",
        VdotBand::Threshold => "Threshold",
        VdotBand::Interval => "Interval",
        VdotBand::Repetition => "Repetition",
    }
}
