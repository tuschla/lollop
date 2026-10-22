use super::*;
use crate::{hybrid, running, strength};

/// The refusal both the reference library and the protein calculator show in
/// place of a weight-loss protein figure when the RED-S screen flag is set
/// (SAFE-REDS-001).
pub(in crate::app) const DEFICIT_REFUSAL: &str = "Protein for eating less is not prescribed: your answers show signs of under-eating for your training (RED-S). Train lighter and see a doctor or registered dietitian before eating less.";

/// Profile-independent evidence-cited reference defaults, surfaced always so a
/// shell can show coaching rationale without a full profile set.
pub(in crate::app) fn build_reference(
    reds_present: bool,
) -> std::borrow::Cow<'static, [GuidanceView]> {
    // The reference rows are profile-independent apart from the RED-S deficit
    // gate, and identical every call in the common no-RED-S case, yet
    // each rebuild re-runs the evidence-registry lookups behind ~30 `push_guidance`
    // rows on every `view()`. Borrow the cached common variant; a
    // RED-S profile is a rare safety state, so recompute its (equally
    // deterministic) gated variant on demand rather than caching a second copy.
    // Deterministic (no clock/rand), a lazy constant, not state.
    if reds_present {
        return std::borrow::Cow::Owned(build_reference_impl(true));
    }
    static CACHE: std::sync::LazyLock<Vec<GuidanceView>> =
        std::sync::LazyLock::new(|| build_reference_impl(false));
    std::borrow::Cow::Borrowed(&CACHE)
}

fn build_reference_impl(reds_present: bool) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    push_hybrid_reference(&mut rows);
    push_strength_reference(&mut rows);
    push_hypertrophy_reference(&mut rows);
    push_nutrition_reference(reds_present, &mut rows);
    push_running_reference(&mut rows);
    push_coaching_reference(&mut rows);
    rows
}

/// Concurrent-training scheduling and dosing rows, plus the two Safety rows
/// the hybrid claims carry.
fn push_hybrid_reference(rows: &mut Vec<GuidanceView>) {
    let sp = hybrid::session_spacing();
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "Two sessions on one day: ideally {} h apart, at least {} h",
            fmt_u8_range(sp.value.ideal_min_hours, sp.value.ideal_max_hours),
            sp.value.fallback_min_hours
        ),
        &sp,
    );

    let ml = hybrid::maintenance_lift_sessions();
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "Keep {} lift sessions a week in a running block",
            fmt_u8_range(ml.value.0, ml.value.1)
        ),
        &ml,
    );

    let efc = hybrid::endurance_frequency_cap();
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "When lifting matters as much as running: run at most {} days a week",
            efc.value
        ),
        &efc,
    );

    let md = hybrid::maintenance_dose_fraction();
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "To keep a fitness gain without building it: about {:.0}% of the training that built it (about 2 short sessions a week)",
            md.value * 100.0
        ),
        &md,
    );

    // hybrid-011: place the priority quality when freshest.
    let sched = hybrid::priority_quality_when_freshest(true, true);
    push_guidance(
        rows,
        "Hybrid",
        "Do your most important session when you're freshest: early in the week or right after a rest day".to_string(),
        &sched,
    );

    // hybrid-019 / CAP-8: double-day fueling.
    let cho = hybrid::double_day_cho_refuel(true);
    push_guidance(
        rows,
        "Hybrid",
        "Two sessions in one day: eat plenty of carbs between the run and the lift. Running low on carbs makes the run interfere more with your lifting".to_string(),
        &cho,
    );

    // hybrid-020: phase interference policy (general vs specific/event).
    let phase = hybrid::phase_interference_policy(hybrid::HybridPhase::SpecificEvent);
    push_guidance(
        rows,
        "Hybrid",
        "Most of the year, keep strength and endurance work apart. Combine them only near a race that needs both, and accept some interference (hybrid races: 2–3 strength and 3–4 endurance sessions a week)".to_string(),
        &phase,
    );

    // hybrid-016 / CAP-2: the peak strength/power block running override. A
    // reference card only: the generated plan does not build a peak lifting
    // block, so it is not shown as profile guidance.
    let pk = hybrid::peak_phase_run_cap();
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "In a peak strength or power block: {} easy runs a week, no hard intervals, and long runs at least {} hours from heavy leg days",
            fmt_u8_range(
                pk.value.max_easy_runs_per_week.0,
                pk.value.max_easy_runs_per_week.1
            ),
            pk.value.long_run_min_gap_hours
        ),
        &pk,
    );

    // hybrid-025: tendon-stiffness evidence gap → conservative dual progression.
    let dual = hybrid::conservative_dual_progression(true, true);
    push_guidance(
        rows,
        "Safety",
        "Never ramp up high running volume and heavy lifting fast in the same week. The combined strain of both on tendons has not been studied, so build one and hold the other".to_string(),
        &dual,
    );

    // hybrid-024: energy-availability guard (higher-risk cohorts named).
    let ea = hybrid::energy_availability_guard(true, true, true);
    push_guidance(
        rows,
        "Safety",
        "Eat enough for your training: under-eating (RED-S) is more likely if you run a lot, are lean, or are female".to_string(),
        &ea,
    );
}

/// Peaking, progression and autoregulation rules for strength work.
fn push_strength_reference(rows: &mut Vec<GuidanceView>) {
    let tr = strength::taper_rx();
    push_guidance(
        rows,
        "Strength",
        format!(
            "Peak taper: cut volume {}% over {} days, hold intensity",
            fmt_frac_pct_range(
                tr.value.volume_reduction_frac.0,
                tr.value.volume_reduction_frac.1
            ),
            fmt_u8_range(tr.value.duration_days.0, tr.value.duration_days.1)
        ),
        &tr,
    );

    let dl = strength::deadlift_peak_days_out();
    push_guidance(
        rows,
        "Strength",
        format!(
            "Last near-max deadlift {} days out from a meet",
            fmt_u8_range(dl.value.0, dl.value.1)
        ),
        &dl,
    );

    let pap = strength::pap_rest_window_min();
    push_guidance(
        rows,
        "Strength",
        format!(
            "PAP: rest {} min between the heavy set and the jump or sprint",
            fmt_u8_range(pap.value.0, pap.value.1)
        ),
        &pap,
    );

    // Grouped with the Strength rows above (not the Hypertrophy block below) so
    // the section runs stay contiguous, reference cards are labelled per-section,
    // and a stray out-of-order Strength row reads as an orphan.
    let mf = individualization::maintenance_frequency_per_week();
    push_guidance(
        rows,
        "Strength",
        format!(
            "Maintenance: train each lift {}× a week to hold strength",
            mf.value
        ),
        &mf,
    );

    let od = strength::olympic_derivative_rx();
    push_guidance(
        rows,
        "Strength",
        format!(
            "Olympic pulling derivatives: {} sets × {} reps at {}% 1RM, {}–{} min rest",
            fmt_u8_range(od.value.sets.0, od.value.sets.1),
            fmt_u8_range(od.value.reps.0, od.value.reps.1),
            fmt_u8_range(od.value.pct_1rm.0, od.value.pct_1rm.1),
            od.value.rest_sec.0 / 60,
            od.value.rest_sec.1 / 60
        ),
        &od,
    );

    let bo = strength::rpe_anchored_back_off();
    push_guidance(
        rows,
        "Strength",
        format!(
            "RPE-anchored: top set at RPE {}, then back-off sets {:.0}–{:.0}% lighter",
            bo.value.top_set_rpe,
            bo.value.drop_frac.0 * 100.0,
            bo.value.drop_frac.1 * 100.0
        ),
        &bo,
    );

    // Call with the rule's own trigger thresholds purely to inherit the graded
    // citation; the prose states the rule itself (educational, not a live verdict).
    let two = strength::two_for_two_met(2, 2);
    push_guidance(
        rows,
        "Strength",
        "Two-for-two rule: add weight once you beat the top of the rep target by 2 or more reps on the last set for 2 sessions in a row".to_string(),
        &two,
    );

    let stall = strength::stall_triggers_deload(2, true);
    push_guidance(
        rows,
        "Strength",
        "Stall: if e1RM stays flat for 2 weeks or more while you're recovering well, take a lighter week or change how your training is structured".to_string(),
        &stall,
    );

    let dbl = individualization::double_progression_add_load(true);
    push_guidance(
        rows,
        "Strength",
        "Double progression: once you reach the top of the rep range on every set, add weight and drop back to the bottom of the range".to_string(),
        &dbl,
    );
}

/// Deload, minimum-exposure and joint-pain rules for hypertrophy work.
fn push_hypertrophy_reference(rows: &mut Vec<GuidanceView>) {
    let hd = hypertrophy::deload_rx();
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "Lighter week: about {:.0}% of your sets, ~{} reps left, {:.0}–{:.0}% of your usual weights",
            hd.value.sets_fraction * 100.0,
            fmt_u8_range(hd.value.rir.0, hd.value.rir.1),
            hd.value.load_frac_of_working.0 * 100.0,
            hd.value.load_frac_of_working.1 * 100.0
        ),
        &hd,
    );

    let mex = individualization::min_muscle_exposures_per_week();
    push_guidance(
        rows,
        "Hypertrophy",
        format!("Train each muscle at least {}× a week", mex.value),
        &mex,
    );

    let jp = hypertrophy::joint_pain_rep_shift();
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "Joint pain at heavy weights: shift that muscle to {} reps at {}% 1RM (lighter weights still build muscle)",
            fmt_u8_range(jp.value.reps.0, jp.value.reps.1),
            fmt_u8_range(jp.value.pct_1rm.0, jp.value.pct_1rm.1)
        ),
        &jp,
    );
}

/// Protein targets; the deficit row is gated on RED-S.
fn push_nutrition_reference(reds_present: bool, rows: &mut Vec<GuidanceView>) {
    let mp = individualization::masters_protein_target();
    push_guidance(
        rows,
        "Nutrition",
        format!(
            "Age 65+ protein: {:.1}–{:.1} g/kg/day (older muscle needs more)",
            mp.value.g_per_kg.0, mp.value.g_per_kg.1
        ),
        &mp,
    );

    // Deficit-protein education row, gated on RED-S exactly like the live
    // calculator refusal (`build_protein_targets`): a RED-S / low-energy-
    // availability profile must never see deficit framing (File 08
    // safety-022/safety-049 → SAFE-REDS-001). The refusal copy carries the same
    // graded citation the number would, so the disclosure stays evidence-cited.
    let dp = individualization::deficit_protein_target(reds_present);
    match &dp.value {
        Some(t) => push_guidance(
            rows,
            "Nutrition",
            format!(
                "Protein while eating less to lose weight (keeps muscle): {:.1}–{:.1} g/kg/day",
                t.g_per_kg.0, t.g_per_kg.1
            ),
            &dp,
        ),
        None => push_guidance(rows, "Nutrition", DEFICIT_REFUSAL.to_string(), &dp),
    }

    let mpm = individualization::masters_protein_per_meal();
    push_guidance(
        rows,
        "Nutrition",
        format!(
            "Age 65+ per-meal protein: ~{:.1} g/kg (older muscle needs more)",
            mpm.value
        ),
        &mpm,
    );
}

/// Couch-to-5K structure and the intensity-distribution models.
fn push_running_reference(rows: &mut Vec<GuidanceView>) {
    let c25k = running::c25k_plan();
    push_guidance(
        rows,
        "Running",
        format!(
            "Couch to 5K: {} run/walk sessions a week over {}–{} weeks, a rest day between; repeat a hard week whenever you need to",
            c25k.value.runs_per_week, c25k.value.weeks.0, c25k.value.weeks.1
        ),
        &c25k,
    );

    let pyr = running::intensity_distribution(running::DistributionModel::Pyramidal);
    push_guidance(
        rows,
        "Running",
        format!(
            "Pyramidal split (base / newer runners): {}% easy, {}% moderate, {}% hard",
            pyr.value.easy_pct, pyr.value.moderate_pct, pyr.value.hard_pct
        ),
        &pyr,
    );

    let pol = running::intensity_distribution(running::DistributionModel::Polarized);
    push_guidance(
        rows,
        "Running",
        format!(
            "Polarized split (trained / peak phase): {}% easy, {}% moderate, {}% hard",
            pol.value.easy_pct, pol.value.moderate_pct, pol.value.hard_pct
        ),
        &pol,
    );
}

/// Scaling order, substitutions and the feedback framing.
fn push_coaching_reference(rows: &mut Vec<GuidanceView>) {
    let join_levers = |levers: &[individualization::ScaleLever]| -> String {
        levers
            .iter()
            .map(|l| match l {
                individualization::ScaleLever::AccessoryVolume => "extra exercises",
                individualization::ScaleLever::SetsTowardMev => {
                    "sets per muscle (toward the fewest that still build muscle)"
                }
                individualization::ScaleLever::Frequency => "sessions per week",
                individualization::ScaleLever::SecondaryQuality => "your second goal",
                individualization::ScaleLever::IntensityAndMainCompounds => "heavy main lifts",
            })
            .collect::<Vec<_>>()
            .join(", then ")
    };

    let sd = individualization::scale_down_order();
    push_guidance(
        rows,
        "Scaling",
        format!(
            "To scale back, cut in this order: {}",
            join_levers(&sd.value)
        ),
        &sd,
    );

    let su = individualization::scale_up_order();
    push_guidance(
        rows,
        "Scaling",
        format!("To scale up, add in this order: {}", join_levers(&su.value)),
        &su,
    );

    let sub = individualization::substitution_rule();
    let mut sub_parts = Vec::new();
    if sub.value.match_movement_pattern {
        sub_parts.push("match the movement pattern");
    }
    if sub.value.compensate_with_reps_near_failure {
        sub_parts.push("add reps near failure to make up for the lighter weight");
    }
    push_guidance(
        rows,
        "Substitutions",
        format!("Home / minimal equipment: {}", sub_parts.join(", then ")),
        &sub,
    );

    let framing = feedback::default_goal_framing();
    let framing_text = match framing.value {
        feedback::GoalFraming::Process => {
            "Next steps name things you can control: pacing, effort and technique"
        }
        feedback::GoalFraming::Outcome => {
            "Outcome/result framing (only where an individual goal-efficacy signal supports it)"
        }
    };
    push_guidance(rows, "Coaching", framing_text.to_string(), &framing);
}
