use super::*;
use crate::running::{self, PACE_CORRECTION_ALTITUDE_M, PACE_CORRECTION_HEAT_C};

/// Running rows for a profile that runs.
pub(super) fn build_running_guidance(
    p: &Profile,
    age: individualization::TrainingAge,
    age_years: f64,
    rows: &mut Vec<GuidanceView>,
) {
    push_goal_week_row(p, rows);
    push_intensity_rows(rows);
    push_runner_strength_row(p, rows);
    push_volume_progression_rows(p, age, age_years, rows);
    push_session_rx_rows(p, rows);
    push_recovery_and_taper_rows(p, rows);
    push_pace_correction_row(p, rows);
}

fn push_goal_week_row(p: &Profile, rows: &mut Vec<GuidanceView>) {
    let gp = running::goal_week_plan(p.goal_distance, p.advanced);
    // C25K carries no long-run share (running-025 defines no long run for
    // C25K), so drop the "long run …%" fragment when the share is zero rather
    // than printing a misleading "long run 0–0%". The goal distance renders as
    // a human label, not the raw Debug enum name.
    let (q_lo, q_hi) = gp.value.quality_per_week;
    let hard = if q_hi == 0 {
        "none".to_string()
    } else {
        fmt_u8_range(q_lo, q_hi)
    };
    let head = format!(
        "{}: {} runs a week, {hard} of them hard",
        goal_distance_label(p.goal_distance),
        fmt_u8_range(gp.value.sessions_per_week.0, gp.value.sessions_per_week.1),
    );
    let summary = if gp.value.long_run_share.1 > 0.0 {
        format!(
            "{head}, long run {}% of the week's distance",
            fmt_frac_pct_range(gp.value.long_run_share.0, gp.value.long_run_share.1)
        )
    } else {
        head
    };
    push_guidance(rows, "Running", summary, &gp);
}

fn push_intensity_rows(rows: &mut Vec<GuidanceView>) {
    // Hard-run caps, the guardrail behind the "of them hard" count above. Only
    // relevant to someone who actually runs, so a pure lifter is not shown
    // running caps.
    let ql = running::quality_limits();
    push_guidance(
        rows,
        "Running",
        format!(
            "Hard runs: at most {} a week, at least {} h apart, never on back-to-back days",
            ql.value.max_per_week, ql.value.min_spacing_hours
        ),
        &ql,
    );

    // Default to the base-phase distribution (pyramidal), the sound starting
    // point before a mesocycle phase is chosen.
    let id = running::distribution_for_phase(MesoPhase::Base);
    push_guidance(
        rows,
        "Running",
        format!(
            "Early in a plan: {}% of running time easy, {}% moderate, {}% hard",
            id.value.easy_pct, id.value.moderate_pct, id.value.hard_pct
        ),
        &id,
    );
}

fn push_runner_strength_row(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // A run-only athlete is offered supplemental strength work
    // (RUN-STRENGTH-001; heavy ≥80% or jump work is what helps running economy,
    // HYB-RE-DOSE-001). The ≤3-run-day hybrid default is NOT shown here: it only
    // applies when lifting is a co-primary goal (see the Hybrid rows).
    if p.weekly_sets == 0 {
        if p.runner_strength {
            let re = graded((), claims::HYB_RE_DOSE_001);
            push_guidance(
                rows,
                "Running",
                "Your plan includes 2 short strength sessions a week: squats and hip hinges, heavy (80% of your max or more) for a few sets, kept away from your hard and long runs.".to_string(),
                &re,
            );
        } else {
            let rs = graded((), claims::RUN_STRENGTH_001);
            push_guidance(
                rows,
                "Running",
                "Add 2 short strength sessions a week: heavy leg work (squats, hip hinges) makes running more economical and can improve race times. Turn on runner strength in your profile and the plan places them away from your hard runs.".to_string(),
                &rs,
            );
        }
    }
}

fn push_volume_progression_rows(
    p: &Profile,
    age: individualization::TrainingAge,
    age_years: f64,
    rows: &mut Vec<GuidanceView>,
) {
    // Shared predicate with the program synthesizer (RUN-DELOAD-001: masters age
    // ≥ 65 OR novice training age), so the displayed cadence matches the one the
    // program actually builds.
    let conservative = crate::plan::conservative_deload(p.age_years, age);
    let dc = running::deload_cadence(conservative);
    push_guidance(
        rows,
        "Running",
        format!(
            "Lighter weeks: after every {} weeks of building, {} lighter week{} with {}% less running",
            dc.value.load_weeks,
            dc.value.recovery_weeks,
            if dc.value.recovery_weeks == 1 {
                ""
            } else {
                "s"
            },
            fmt_frac_pct_range(dc.value.reduction_frac.0, dc.value.reduction_frac.1)
        ),
        &dc,
    );

    let wc = running::weekly_increase_cap_frac(age_years);
    push_guidance(
        rows,
        "Running",
        format!(
            "Weekly distance grows by at most {:.0}% from one week to the next",
            wc.value * 100.0
        ),
        &wc,
    );
}

/// Session-type prescriptions (running-014…020) for an athlete who runs:
/// the easy-run band, long runs, cruise-interval and VO2max-interval
/// structure, hill sprints and strides. Each row is built from the Rx struct
/// so no number is retyped.
fn push_session_rx_rows(p: &Profile, rows: &mut Vec<GuidanceView>) {
    let easy = running::workout_rx(running::RunWorkout::EasyGeneralAerobic);
    let mut parts: Vec<String> = Vec::new();
    if let (Some(lo), Some(hi)) = easy.value.pct_hr_max {
        parts.push(format!("{:.0}–{:.0} %HRmax", lo * 100.0, hi * 100.0));
    }
    if let Some((lo, hi)) = easy.value.rpe {
        parts.push(format!("RPE {lo}–{hi}"));
    }
    if let (Some(lo), Some(hi)) = easy.value.duration_min {
        parts.push(format!("{lo}–{hi} min"));
    }
    push_guidance(
        rows,
        "Running",
        format!("Easy runs: {}", parts.join(", ")),
        &easy,
    );

    // running-016/018 session Rx from the schema session types.
    let long = running::run_workout_rx(crate::schema::RunSessionType::LongRun);
    let mut long_parts: Vec<String> = Vec::new();
    if let (Some(lo), Some(hi)) = long.value.pct_hr_max {
        long_parts.push(format!("{:.0}–{:.0} %HRmax", lo * 100.0, hi * 100.0));
    }
    if let (Some(lo), Some(hi)) = long.value.pct_slower_than_mp {
        long_parts.push(format!("{lo:.0}–{hi:.0}% slower than marathon pace"));
    }
    if let Some((lo, hi)) = long.value.rpe {
        long_parts.push(format!("RPE {lo}–{hi}"));
    }
    if let (None, Some(hi)) = long.value.duration_min {
        long_parts.push(format!("up to {hi} min"));
    }
    push_guidance(
        rows,
        "Running",
        format!("Long runs: {}", long_parts.join(", ")),
        &long,
    );

    let cruise = running::cruise_interval_rx();
    push_guidance(
        rows,
        "Running",
        format!(
            "Cruise intervals (threshold pace): {}–{} min repeats, about {:.0} min rest, at most {:.0}% of the week's distance",
            cruise.value.rep_duration_min.0,
            cruise.value.rep_duration_min.1,
            cruise.value.rest_approx_min,
            cruise.value.weekly_cap_frac * 100.0
        ),
        &cruise,
    );

    let vo2 = running::vo2max_interval_rx();
    push_guidance(
        rows,
        "Running",
        format!(
            "Hard intervals (interval pace): {}–{} min repeats ({}–{} m), recover as long as each repeat took, at most {:.0}% of the week's distance",
            vo2.value.rep_duration_min.0,
            vo2.value.rep_duration_min.1,
            vo2.value.rep_distance_m.0,
            vo2.value.rep_distance_m.1,
            vo2.value.weekly_cap_frac * 100.0
        ),
        &vo2,
    );

    // running-021 hill sprints, strength-flavoured running work, most
    // relevant to a power-goal athlete who runs.
    if matches!(p.lift_goal, LiftGoal::Power) {
        let hills = running::hill_sprint_rx();
        push_guidance(
            rows,
            "Running",
            format!(
                "Hill sprints: {}–{} × {}–{} s at {:.0}–{:.0}% effort on a {:.0}–{:.0}% grade, full (~{} s) recovery, on easy days",
                hills.value.reps.0,
                hills.value.reps.1,
                hills.value.rep_sec.0,
                hills.value.rep_sec.1,
                hills.value.effort_pct.0,
                hills.value.effort_pct.1,
                hills.value.grade_pct.0,
                hills.value.grade_pct.1,
                hills.value.recovery_approx_sec
            ),
            &hills,
        );
    }

    let strides = running::strides_rx();
    push_guidance(
        rows,
        "Running",
        format!(
            "Strides: {}–{} × {}–{} s controlled-fast (RPE {}–{}), {}–{} s recovery, {}–{}×/wk",
            strides.value.reps.0,
            strides.value.reps.1,
            strides.value.rep_sec.0,
            strides.value.rep_sec.1,
            strides.value.rpe.0,
            strides.value.rpe.1,
            strides.value.recovery_sec.0,
            strides.value.recovery_sec.1,
            strides.value.per_week.0,
            strides.value.per_week.1
        ),
        &strides,
    );
}

fn push_recovery_and_taper_rows(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // running-033 depth: the KB names no mileage threshold separating the
    // bands, so the general 20–40 % band is stated (never a guess).
    let rw = running::recovery_week_rx(running::MileageBand::Unspecified);
    push_guidance(
        rows,
        "Running",
        format!(
            "Lighter week: cut distance {}%, ease the effort, and drop one hard run",
            fmt_frac_pct_range(
                rw.value.volume_reduction_frac.0.unwrap_or(0.0),
                rw.value.volume_reduction_frac.1
            )
        ),
        &rw,
    );

    // running-038 distance-specific taper for race goals.
    if let Some(taper) = running::distance_taper(p.goal_distance) {
        let t = taper.value;
        push_guidance(
            rows,
            "Running",
            format!(
                "Race taper ({}): {} days, cut volume {}%{}. Hold intensity and frequency",
                goal_distance_label(p.goal_distance),
                fmt_u8_range(t.days.0, t.days.1),
                fmt_frac_pct_range(t.volume_cut_frac.0, t.volume_cut_frac.1),
                if t.keep_mp_touches_and_short_tempo {
                    ", keep a few short marathon-pace and tempo efforts"
                } else if t.keep_sharp_sessions.is_some() {
                    ", keep 1–2 short sharp sessions"
                } else {
                    ""
                }
            ),
            &taper,
        );
    }

    // running-032 single-variable rule, stated educationally (the call's
    // both-true arguments only inherit the citation, same pattern as the
    // two-for-two reference row).
    let onevar = running::single_variable_progression_ok(true, true);
    push_guidance(
        rows,
        "Running",
        "In any week, raise either your distance or your effort, never both".to_string(),
        &onevar,
    );
}

fn push_pace_correction_row(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // running-041 environment pace-correction triggers, when the profile
    // states conditions. Only trigger flags are evidence-stated, no
    // correction magnitudes exist in the KB, so none are shown.
    if p.env_temp_c.is_some() || p.env_altitude_m.is_some() {
        let pc = running::pace_correction_triggers(
            p.env_temp_c.unwrap_or(0.0),
            p.env_altitude_m.unwrap_or(0.0),
        );
        if pc.value.heat || pc.value.altitude {
            let what = match (pc.value.heat, pc.value.altitude) {
                (true, true) => format!(
                    "heat (above {PACE_CORRECTION_HEAT_C} °C) and altitude (above {PACE_CORRECTION_ALTITUDE_M} m)"
                ),
                (true, false) => format!("heat (above {PACE_CORRECTION_HEAT_C} °C)"),
                _ => format!("altitude (above {PACE_CORRECTION_ALTITUDE_M} m)"),
            };
            push_guidance(
                rows,
                "Running",
                format!(
                    "In {what}, expect slower paces at the same effort. Go by heart rate or effort, not pace"
                ),
                &pc,
            );
        }
    }
}
