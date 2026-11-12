//! Flattening a `Prescription::Run` arm and its tracker structure.

use crate::app::evidence_views::why_from;
use crate::app::format::{fmt_mi_of_km, fmt_rounded, fmt_secs_min, round_tenths};
use crate::app::plan_views::plan_item::{PlanItem, RunAmount};
use crate::app::view_types::{
    EvidenceTagView, ItemKind, PlanIntensity, PrescriptionView, RunStructureView, RunWalkStepView,
};
use crate::profile::DistanceUnit;
use crate::schema::{RunIntensity, RunVolume, Rx, RxBasis, ThreeZone, VdotBand};

/// Render a prescribed run distance: whole km as "8 km", a sub-km step as one
/// decimal ("6.5 km", "4.3 km"), matching the plan's 0.5 km / 0.1 km
/// quantisation. The plan floors to its step, so the one-decimal value is the
/// prescription itself and never reads above the spike gate.
pub(super) fn fmt_run_km(k: f32) -> String {
    format!("{} km", fmt_rounded(round_tenths(f64::from(k))))
}

/// Render a C25K run/walk interval set (running-025): `rounds` repeats of a run
/// then a walk, each shown as minutes once it reaches 2 min, e.g.
/// "8 × (60 s run / 90 s walk)".
fn run_walk_label(run_sec: u16, walk_sec: u16, rounds: u8) -> String {
    format!(
        "{rounds} × ({} run / {} walk)",
        fmt_secs_min(run_sec.into(), 2),
        fmt_secs_min(walk_sec.into(), 2)
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

/// A run volume as the card writes it: "30 min", "8 km" (a rep's distance in
/// metres, "800 m", when `metres`), or a run/walk bout set.
fn volume_label(v: RunVolume, metres: bool) -> String {
    match v {
        RunVolume::DurationMin(m) => format!("{m} min"),
        RunVolume::DistanceKm(k) if metres => format!("{:.0} m", f64::from(k) * 1000.0),
        RunVolume::DistanceKm(k) => fmt_run_km(k),
        RunVolume::RunWalk {
            run_sec,
            walk_sec,
            rounds,
        } => run_walk_label(run_sec, walk_sec, rounds),
    }
}

/// Flatten a `Prescription::Run` arm into its [`PlanItem`], the safety
/// reason in `unit`.
pub(super) fn flatten_run_prescription(
    r: &crate::schema::RunPrescription,
    rx: &Rx,
    unit: DistanceUnit,
) -> PlanItem {
    let vol = volume_label(r.volume, false);
    // An interval/repetition run carries per-rep structure; surface
    // it so the card reads "4 × 4 min · Interval pace", not a misleading
    // "16 min · Interval pace" that looks like one continuous VO2max run.
    let rep_volume = r.repeats.map(|(_, rv)| volume_label(rv, true));
    let rep_count = r.repeats.map_or(0, |(n, _)| n);
    let safety_reason = run_safety_reason(r, rx, unit);
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
    };
    // Rep-structured runs lead with the honest "N × <rep> · <pace>";
    // continuous runs keep the whole-session "<vol> · <pace>".
    let run_volume = r.repeats.is_none().then(|| RunAmount::of(r.volume));
    let head = match &rep_volume {
        Some(rv) => format!("{rep_count} × {rv}"),
        None => vol,
    };
    let summary = format!("{head} · {label}");
    PlanItem {
        view: PrescriptionView {
            kind: ItemKind::Run,
            summary,
            intensity_label: label,
            rep_volume: rep_volume.unwrap_or_default(),
            safety_reason,
            evidence: EvidenceTagView::of(rx),
            why: why_from(run_basis(rx), rx),
            run_structure: run_structure_view(r),
            face_lead: head.clone(),
            run_head: head,
            run_head_km: run_volume.and_then(RunAmount::shown_km),
            ..PrescriptionView::default()
        },
        // A run target (pace/zone/HRmax) is never RIR/load-adjustable.
        intensity: PlanIntensity::Prose,
        rep_count,
        run_volume,
        rep_amount: r.repeats.map(|(_, rv)| RunAmount::of(rv)),
        ..PlanItem::default()
    }
}

/// The why? basis of a run whose numbers were set by a rule other than its
/// workout's own. A long run held by the 30-day-longest-run ceiling rather
/// than the weekly share states that rule honestly (HARD RULE 2) instead of
/// the generic claim statement, no invented numbers, only what the KB entries
/// state.
fn run_basis(rx: &Rx) -> Option<String> {
    let spike_pct = crate::running::SPIKE_FRAC * 100.0;
    let window_days = crate::running::SPIKE_WINDOW_DAYS;
    match rx.kind() {
        RxBasis::SpikeCeiling => Some(format!(
            "Distance stays within {spike_pct:.0}% of your longest run in the last \
             {window_days} days. It can be a bigger share of your week than usual \
             until your weekly distance catches up."
        )),
        RxBasis::StatedCeiling => Some(format!(
            "The distance stays within {spike_pct:.0}% of the longest run you entered for \
             the last month. Once a logged run goes further, or that answer is more than \
             {window_days} days old, the plan follows your logged runs."
        )),
        RxBasis::LongRunTarget => Some(format!(
            "The long run has reached the most your goal distance needs, or about {:.0} \
             minutes at most. Extra distance goes into the rest of your week.",
            crate::running::LONG_RUN_MAX_MIN
        )),
        RxBasis::Taper => Some(
            "Taper week before your race: total distance drops while your hard sessions \
             keep their effort, so you arrive fresh."
                .to_string(),
        ),
        // The anchoring run is now older than the 30-day window, so its
        // credit is tapering (DETRAIN-001) rather than dropping off a cliff.
        RxBasis::Detrained => Some(
            "Distance stays near your recent long run, then steps down as that run gets older, \
             since endurance fades slowly after a couple of weeks. Log a new long run to reset it."
                .to_string(),
        ),
        RxBasis::AverageRun => Some(
            "With only a few run days, 25–30% of the week would be shorter than your other \
             runs, so the long run is set to this week's average run. It never goes past \
             twice your average daily run."
                .to_string(),
        ),
        RxBasis::Workout
        | RxBasis::StrengthDeload
        | RxBasis::HypertrophyDeload
        | RxBasis::Maintenance => None,
    }
}

/// A spike-capped run's safety line, its distance in `unit`: "Capped at 8 km:
/// at most N% over your longest recent run" for a continuous distance, else
/// "Kept short and easy: …". Empty for any other basis.
fn run_safety_reason(r: &crate::schema::RunPrescription, rx: &Rx, unit: DistanceUnit) -> String {
    let spike_pct = crate::running::SPIKE_FRAC * 100.0;
    let longest = match rx.kind() {
        RxBasis::SpikeCeiling => Some("your longest recent run"),
        RxBasis::StatedCeiling => Some("the longest recent run you entered"),
        _ => None,
    };
    match (longest, r.volume) {
        (None, _) => String::new(),
        (Some(l), RunVolume::DistanceKm(k)) if r.repeats.is_none() => {
            let d = match unit {
                DistanceUnit::Km => fmt_run_km(k),
                DistanceUnit::Mi => fmt_mi_of_km(f64::from(k)),
            };
            format!("Capped at {d}: at most {spike_pct:.0}% over {l}")
        }
        (Some(l), _) => format!("Kept short and easy: at most {spike_pct:.0}% over {l}"),
    }
}

/// A Daniels training band's name ("Threshold").
pub(in crate::app) fn vdot_band_name(b: VdotBand) -> &'static str {
    match b {
        VdotBand::Easy => "Easy",
        VdotBand::Marathon => "Marathon",
        VdotBand::Threshold => "Threshold",
        VdotBand::Interval => "Interval",
        VdotBand::Repetition => "Repetition",
    }
}
