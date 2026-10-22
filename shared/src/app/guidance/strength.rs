use super::*;
use crate::strength;

/// Strength + Hypertrophy rows for a lifting profile.
pub(super) fn build_strength_guidance(
    p: &Profile,
    age: individualization::TrainingAge,
    rows: &mut Vec<GuidanceView>,
) {
    push_lift_week_layout(p, rows);
    let lr = push_loading_rows(p, age, rows);
    push_one_rm_rows(age, rows);
    push_periodization_rows(age, rows);
    push_load_progression_row(p, age, rows);
    push_prilepin_row(&lr, rows);
    push_power_row(p, age, rows);
    push_hypertrophy_rows(p, age, rows);
}

/// The lifting week the plan actually lays out (Table 1.6 templates,
/// LIFT-DAYS-001), with the athlete's weekday choice (ADH-DAYS-001).
fn push_lift_week_layout(p: &Profile, rows: &mut Vec<GuidanceView>) {
    let n = crate::plan::week_shape(p).lift_days;
    let layout = match n {
        0 | 1 => None,
        2 => Some("2 full-body days built on the big compound lifts"),
        3 => Some("3 full-body days"),
        4 => Some("2 upper-body and 2 lower-body days"),
        _ => Some("2 upper-body, 2 lower-body and 1 full-body day"),
    };
    if let Some(layout) = layout {
        let ld = graded((), claims::LIFT_DAYS_001);
        let days_note = if p.training_weekdays.is_some_and(|m| m & 0x7f != 0) {
            " on the days you picked"
        } else {
            ""
        };
        push_guidance(
            rows,
            "Strength",
            format!(
                "Lifting {n} days a week{days_note}: {layout}. Weekly sets per muscle stay the same however many days you lift, and days that train the same muscles are about 48 hours apart."
            ),
            &ld,
        );
    }
}

/// Training-age defaults, the goal's loading band and the velocity-loss set
/// cutoff. Returns the loading band, which also anchors the Prilepin row.
fn push_loading_rows(
    p: &Profile,
    age: individualization::TrainingAge,
    rows: &mut Vec<GuidanceView>,
) -> Recommended<strength::LoadingRx> {
    let sd = individualization::strength_defaults(age);
    push_guidance(
        rows,
        "Strength",
        format!(
            "Starting point: {}% 1RM, each muscle {}× a week, {} sets per muscle",
            sd.value.intensity_pct_1rm, sd.value.freq_per_muscle, sd.value.sets_per_muscle
        ),
        &sd,
    );

    let lr = strength::loading_rx(p.lift_goal);
    // The Power band carries no KB-sanctioned numeric RIR (its `rir` is `None`),
    // so state the KB's qualitative power instruction instead of a fabricated
    // number; every other goal states its RIR band.
    let rir_clause = match lr.value.rir {
        None => "never to failure; stop before bar speed drops".to_string(),
        Some((lo, hi)) => format!("~{} reps left", fmt_u8_range(lo, hi)),
    };
    // A single "0–95% 1RM" band tells nobody what to load. The Power band is an
    // ENVELOPE across exercise types (strength-002/030; STR-POWER-SELECT-001),
    // so state it per exercise type in plain words.
    let loading = if p.lift_goal == LiftGoal::Power {
        format!(
            "Power loading: jumps at bodyweight up to about 30% 1RM, loaded squats 30–70%, cleans and pulls 70–95% 1RM. {} fast reps, {} sets, never to failure",
            fmt_u8_range(lr.value.reps.0, lr.value.reps.1),
            fmt_u8_range(lr.value.sets.0, lr.value.sets.1)
        )
    } else {
        format!(
            "{} loading: {}% 1RM, {} reps, {} sets, {}",
            lift_goal_label(p.lift_goal),
            fmt_u8_range(lr.value.pct_1rm.0, lr.value.pct_1rm.1),
            fmt_u8_range(lr.value.reps.0, lr.value.reps.1),
            fmt_u8_range(lr.value.sets.0, lr.value.sets.1),
            rir_clause
        )
    };
    push_guidance(rows, "Strength", loading, &lr);

    // Nobody perceives a ~N% bar-speed loss without a velocity device, so the
    // cue is the visible slowdown; the percentage is for velocity trackers.
    let vlt = strength::vl_termination_threshold(p.lift_goal);
    push_guidance(
        rows,
        "Strength",
        format!(
            "Set cutoff for {}: end the set when a rep visibly slows (about {:.0}% bar-speed loss if you track bar speed)",
            lift_goal_label(p.lift_goal),
            vlt.value * 100.0
        ),
        &vlt,
    );
    lr
}

/// The 1RM-test gate and, for a novice, the load-jump caps between attempts.
fn push_one_rm_rows(age: individualization::TrainingAge, rows: &mut Vec<GuidanceView>) {
    // strength-040 1RM-test gate, stated educationally (the all-clear call
    // only inherits the safety-critical citation; the conditions are the row).
    let novice = matches!(age, individualization::TrainingAge::Novice);
    let test_gate = strength::one_rm_test_allowed(strength::OneRmTestContext {
        technically_proficient: true,
        adequately_recovered: true,
        warmed_up: true,
        is_novice: novice,
        supervised: true,
        spinal_loading: true,
        bracing_competent: true,
    });
    push_guidance(
        rows,
        "Strength",
        format!(
            "Test a true 1RM only when your technique is solid and you're rested and warmed up{}. Lifts that load the spine, like squats and deadlifts, also need a solid brace",
            if novice {
                ", and as a beginner only with a coach watching (the estimated 1RM is preferred)"
            } else {
                ""
            }
        ),
        &test_gate,
    );

    // strength-040 novice progression-jump caps between attempts.
    if novice {
        let upper = strength::novice_load_jump_cap_frac(true);
        let lower = strength::novice_load_jump_cap_frac(false);
        push_guidance(
            rows,
            "Strength",
            format!(
                "1RM test jumps for a beginner: upper body +{:.1}–{:.0}%, lower body +{:.0}–{:.0}% per attempt",
                upper.value.0 * 100.0,
                upper.value.1 * 100.0,
                lower.value.0 * 100.0,
                lower.value.1 * 100.0
            ),
            &upper,
        );
    }
}

/// The periodization model and its phase-by-phase prescription.
fn push_periodization_rows(age: individualization::TrainingAge, rows: &mut Vec<GuidanceView>) {
    let pm = strength::periodization_model(age);
    // Human label, not the raw Debug enum name (e.g. "Dup").
    let pm_label = match pm.value {
        strength::PeriodizationModel::Linear => "linear",
        strength::PeriodizationModel::Dup => {
            "daily undulating (heavy, medium and light days each week)"
        }
        strength::PeriodizationModel::Block => "block",
        strength::PeriodizationModel::Conjugate => "conjugate",
    };
    push_guidance(
        rows,
        "Strength",
        format!("Periodization model: {pm_label}"),
        &pm,
    );

    // Surface the actual phase-by-phase prescription for the athlete's model, so
    // the periodization label above becomes an actionable mesocycle plan. Only
    // Linear and Block have phase tables in the source; DUP does not, so it is
    // left as the label only.
    let fmt_phase = |rx: &strength::PhaseRx| -> String {
        let mut parts = Vec::new();
        if let Some((lo, hi)) = rx.pct_1rm {
            parts.push(format!("{}% 1RM", fmt_u8_range(lo, hi)));
        }
        if let (Some((slo, shi)), Some((rlo, rhi))) = (rx.sets, rx.reps) {
            parts.push(format!(
                "{}×{}",
                fmt_u8_range(slo, shi),
                fmt_u8_range(rlo, rhi)
            ));
        } else if let Some((rlo, rhi)) = rx.reps {
            parts.push(format!("{} reps", fmt_u8_range(rlo, rhi)));
        }
        if parts.is_empty() {
            parts.push("keep the weights the same (taper)".to_string());
        }
        let (wlo, whi) = rx.weeks;
        let weeks = if wlo == whi { "week" } else { "weeks" };
        format!("{} · {weeks} {}", parts.join(", "), fmt_u8_range(wlo, whi))
    };
    match pm.value {
        strength::PeriodizationModel::Linear => {
            use strength::LinearPhase::*;
            for (name, phase) in [
                ("Base", Base),
                ("Build", Build),
                ("Peak", Peak),
                ("Taper", Taper),
            ] {
                let ph = strength::linear_phase_rx(phase);
                push_guidance(
                    rows,
                    "Strength",
                    format!("Linear {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Block => {
            use strength::BlockPhase::*;
            for (name, phase) in [
                ("Accumulation", Accumulation),
                ("Transmutation (strength)", Transmutation),
                ("Realization (peak)", Realization),
            ] {
                let ph = strength::block_phase_rx(phase);
                push_guidance(
                    rows,
                    "Strength",
                    format!("Block {name}: {}", fmt_phase(&ph.value)),
                    &ph,
                );
            }
        }
        strength::PeriodizationModel::Dup | strength::PeriodizationModel::Conjugate => {}
    }
}

/// How load progresses: a fixed per-session jump for a novice, a weekly
/// percentage after that.
fn push_load_progression_row(
    p: &Profile,
    age: individualization::TrainingAge,
    rows: &mut Vec<GuidanceView>,
) {
    // Double-progression load jump, only actionable for a novice on linear
    // progression; intermediate/advanced lifters no longer add fixed load every
    // session, so showing it to them would misdescribe their programming.
    if matches!(age, individualization::TrainingAge::Novice) {
        let inc = individualization::novice_load_increment();
        let unit = weight_suffix(p.weight_lb);
        let upper = weight_num(inc.value.upper_kg, p.weight_lb);
        let lower = weight_num(inc.value.lower_kg, p.weight_lb);
        push_guidance(
            rows,
            "Strength",
            format!(
                "Adding weight as a beginner: +{upper:.1} {unit} on upper-body lifts, +{lower:.1} {unit} on lower-body lifts per session (double progression)"
            ),
            &inc,
        );
    } else {
        // Past the novice phase the fixed per-session jump no longer applies;
        // progression moves to a per-week percentage of load. Surface both body
        // regions since the bands differ (upper is more conservative).
        let upper = strength::weekly_pct_increment(true);
        let lower = strength::weekly_pct_increment(false);
        // Render each bound faithfully: some bands carry a half-percent (e.g.
        // 2.5%), so a whole-number format would understate the cited value. Drop
        // the decimal only when the bound is whole.
        let pct = |frac: f64| -> String {
            let v = frac * 100.0;
            if v.fract().abs() < 1e-9 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        };
        push_guidance(
            rows,
            "Strength",
            format!(
                "Adding weight: +{}–{}% on upper-body lifts, +{}–{}% on lower-body lifts after each week you hit your reps",
                pct(upper.value.0),
                pct(upper.value.1),
                pct(lower.value.0),
                pct(lower.value.1)
            ),
            &upper,
        );
    }
}

fn push_prilepin_row(lr: &Recommended<strength::LoadingRx>, rows: &mut Vec<GuidanceView>) {
    // Prilepin volume ceiling at the midpoint of the goal's loading intensity -
    // the reps/set + optimal-total governor that keeps quality high per zone.
    // Kept adjacent to the other Strength rows (before the Power block below) so
    // the section stays contiguous: the shell suppresses a repeated section
    // header only for consecutive same-section rows.
    let pct_mid = f64::from(lr.value.pct_1rm.0 + lr.value.pct_1rm.1) / 2.0;
    if let Some(pr) = strength::prilepin_for(pct_mid) {
        let pr = graded(*pr, claims::STR_PRILEPIN_001);
        push_guidance(
            rows,
            "Strength",
            format!(
                "Prilepin chart at ~{:.0}% 1RM: {} reps a set, {} reps in total is optimal (range {}–{})",
                pct_mid,
                fmt_u8_range(pr.value.reps_per_set.0, pr.value.reps_per_set.1),
                pr.value.optimal_total,
                pr.value.total_range.0,
                pr.value.total_range.1
            ),
            &pr,
        );
    }
}

fn push_power_row(p: &Profile, age: individualization::TrainingAge, rows: &mut Vec<GuidanceView>) {
    // Plyometric foot-contact ceiling, only relevant when the goal is Power.
    // Placed after all Strength rows so the "Power" section forms its own
    // contiguous block rather than splitting Strength in two.
    if p.lift_goal == LiftGoal::Power {
        let plyo = strength::plyo_foot_contact_cap(age);
        push_guidance(
            rows,
            "Power",
            format!(
                "Jumps: at most {}–{} foot contacts a session. Add more jumps or harder jumps, never both at once",
                plyo.value.0, plyo.value.1
            ),
            &plyo,
        );
    }
}

/// Weekly-set frequency, session split, rep/load bands, the growth-target cap
/// and MEV.
fn push_hypertrophy_rows(
    p: &Profile,
    age: individualization::TrainingAge,
    rows: &mut Vec<GuidanceView>,
) {
    let fr = hypertrophy::frequency_for_weekly_sets(p.weekly_sets);
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "{} weekly sets → {}×/wk, {} sets/session",
            p.weekly_sets,
            fmt_u8_range(fr.value.freq.0, fr.value.freq.1),
            fmt_u8_range(fr.value.per_session.0, fr.value.per_session.1)
        ),
        &fr,
    );

    let split = hypertrophy::needs_session_split(p.weekly_sets);
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "{} weekly sets per muscle: {}",
            p.weekly_sets,
            if split.value {
                "spread them over 2 or more sessions a week so each set stays good"
            } else {
                "one session a week is enough"
            }
        ),
        &split,
    );

    // Rep/load prescription by exercise class. The frequency/volume rows above
    // answer "how many sets"; this answers "how many reps and what load", the
    // per-set target. All three classes share one evidence source, so surface
    // them together.
    let heavy = hypertrophy::rep_load(hypertrophy::ExerciseClass::HeavyCompound);
    let moderate = hypertrophy::rep_load(hypertrophy::ExerciseClass::ModerateCompound);
    let iso = hypertrophy::rep_load(hypertrophy::ExerciseClass::Isolation);
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "Reps and weight: big lifts {} reps at {}% 1RM, other compound lifts {} at {}%, single-joint lifts {} at {}%",
            fmt_u8_range(heavy.value.reps.0, heavy.value.reps.1),
            fmt_u8_range(heavy.value.pct_1rm.0, heavy.value.pct_1rm.1),
            fmt_u8_range(moderate.value.reps.0, moderate.value.reps.1),
            fmt_u8_range(moderate.value.pct_1rm.0, moderate.value.pct_1rm.1),
            fmt_u8_range(iso.value.reps.0, iso.value.reps.1),
            fmt_u8_range(iso.value.pct_1rm.0, iso.value.pct_1rm.1)
        ),
        &heavy,
    );

    let cap = hypertrophy::cap_weekly_growth_target(p.weekly_sets);
    push_guidance(
        rows,
        "Hypertrophy",
        format!("Weekly sets for muscle growth top out at {}", cap.value),
        &cap,
    );

    let mev = hypertrophy::mev_sets_by_training_age(age);
    push_guidance(
        rows,
        "Hypertrophy",
        format!(
            "Fewest sets that still build muscle ({}): {} per muscle a week",
            training_age_label(age),
            fmt_u8_range(mev.value.0, mev.value.1)
        ),
        &mev,
    );
}
