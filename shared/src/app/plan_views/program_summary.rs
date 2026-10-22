//! The program summary card and its per-muscle weekly volume.

use super::*;
use std::fmt::Write as _;

/// The program summary card, graded with the plan's representative claim.
/// `(week, weeks_total)` is the current program week (advances with
/// `SetToday`), 1-based, of its block or race countdown; `week_index` is the
/// block-cycle week index the plan was synthesized for (it locates this
/// week's per-muscle volume targets).
pub(super) fn build_program_summary(
    profile: &Profile,
    meso: &Mesocycle,
    program: &Program,
    (week, weeks_total): (u8, u8),
    week_index: u32,
) -> ProgramSummaryView {
    let g = graded((), crate::plan::summary_claim(profile));
    let goal_h = human_goal(&program.goal);
    let (muscle_volume, muscle_volume_note) = muscle_volume_rows(profile, meso, week_index);
    ProgramSummaryView {
        name: program.name.clone(),
        goal: goal_h.clone(),
        phase: meso.phase,
        week,
        weeks_total,
        evidence: EvidenceTagView::of(&g),
        why: why_from(
            Some(format!(
                "{} built from your profile and logged training.",
                week_kind(&program.goal, &goal_h)
            )),
            &g,
        ),
        muscle_volume,
        muscle_volume_note,
    }
}

/// "A strength week", "A muscle-building week", "A marathon race week": the
/// goal as it reads mid-sentence.
fn week_kind(goal: &crate::schema::Goal, goal_h: &str) -> String {
    match goal {
        crate::schema::Goal::Hypertrophy => "A muscle-building week".to_string(),
        crate::schema::Goal::GeneralEndurance => "An endurance week".to_string(),
        _ => {
            let mut c = goal_h.chars();
            let lower: String = c
                .next()
                .map(|f| f.to_lowercase().chain(c).collect())
                .unwrap_or_default();
            format!("A {lower} week")
        }
    }
}

/// Weekly sets per muscle the hypertrophy week delivers vs its target, counted
/// fractionally exactly as the plan sized them
/// (`hypertrophy::delivered_weekly_sets`), plus one plain note. Empty for any
/// other goal.
fn muscle_volume_rows(
    profile: &Profile,
    meso: &Mesocycle,
    week_index: u32,
) -> (Vec<MuscleVolumeView>, String) {
    if !crate::plan::uses_double_progression(profile) || profile.weekly_sets == 0 {
        return (Vec::new(), String::new());
    }
    let ta = crate::individualization::training_age_from_cadence(profile.progression_cadence).value;
    let crate::plan::BlockPosition {
        week_in_block,
        block_weeks,
        recovery,
    } = crate::plan::lift_block_position(profile, week_index);
    let lift_days = meso
        .sessions
        .iter()
        .filter(|s| matches!(s.session_type, SessionType::Lift(_)))
        .count();
    let targets =
        crate::plan::hypertrophy_week_targets(profile, ta, week_in_block, block_weeks, lift_days);
    let items: Vec<(&str, u8)> = meso
        .sessions
        .iter()
        .flat_map(|s| s.prescriptions.iter())
        .filter_map(|rx| match &rx.value {
            Prescription::Lift(l) => Some((l.exercise.as_str(), l.sets)),
            _ => None,
        })
        .collect();
    let delivered = crate::hypertrophy::delivered_weekly_sets(items.iter().copied()).value;
    // A running-led hybrid lifts at a maintenance dose (HYB-MAINT-001 /
    // RUN-STRENGTH-001) instead of the volume targets: show what is delivered,
    // never a target the week is not trying to meet.
    let maintenance = meso
        .sessions
        .iter()
        .flat_map(|s| s.prescriptions.iter())
        .filter(|rx| matches!(rx.value, Prescription::Lift(_)))
        .any(|rx| {
            matches!(
                rx.claim_id(),
                claims::HYB_MAINT_001 | claims::RUN_STRENGTH_001
            )
        });
    let no_targets = recovery || maintenance;
    let indirect = |muscle: &str| -> f32 {
        items
            .iter()
            .flat_map(|(name, sets)| {
                crate::hypertrophy::muscle_credits(name)
                    .into_iter()
                    .filter(|(m, w)| *m == muscle && *w < 1.0)
                    .map(move |(_, w)| w * f32::from(*sets))
            })
            .sum()
    };
    let fmt_sets = |x: f32| -> String {
        if (x - x.round()).abs() < 0.05 {
            format!("{:.0}", x.round())
        } else {
            format!("{x:.1}")
        }
    };
    let cap = |m: &str| -> String {
        let mut c = m.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    };
    let mut rows = Vec::new();
    let mut uncovered: Vec<&str> = Vec::new();
    for (muscle, got) in delivered {
        let t = targets.iter().find(|t| t.muscle == muscle);
        let target = t.map_or(0, |t| t.target);
        let priority = t.is_some_and(|t| t.role == crate::hypertrophy::VolumeRole::Priority);
        if (target == 0 || no_targets) && got < 0.05 {
            continue;
        }
        if target > 0 && got < 0.5 && !no_targets {
            uncovered.push(muscle);
        }
        let ind = indirect(muscle);
        let mut label = format!("{}: {} sets", cap(muscle), fmt_sets(got));
        if ind >= 0.05 && ind < got - 0.05 {
            let _ = write!(label, " ({} from compound lifts)", fmt_sets(ind));
        }
        // A recovery or maintenance week sits below the targets on purpose, so
        // its rows show only what is delivered.
        if !no_targets {
            if target > 0 {
                let _ = write!(label, " · target {target}");
                if priority {
                    label.push_str(" (specialization priority)");
                }
            } else {
                label.push_str(" · covered by compound lifts");
            }
        }
        rows.push(MuscleVolumeView {
            muscle: cap(muscle),
            target_sets: if no_targets { 0 } else { target },
            delivered_sets: (got * 10.0).round() / 10.0,
            label,
        });
    }
    let mut notes: Vec<String> = Vec::new();
    if recovery {
        notes.push("Lighter week: about half the usual sets, so the counts sit below your targets on purpose.".into());
    } else if maintenance {
        notes.push(
            "Running leads your plan, so lifting is kept to what holds the muscle you have.".into(),
        );
    }
    if let Some(raw) = profile
        .specialization_muscle
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        match (crate::hypertrophy::resolve_muscle(raw), ta) {
            (Some(lm), crate::individualization::TrainingAge::Advanced) => notes.push(format!(
                "{} gets extra sets this block, and the other muscles get enough to keep what you have.",
                cap(lm.muscle)
            )),
            (Some(_), _) => notes.push(
                "Focusing on one muscle is for advanced lifters who have stopped growing on their usual number of sets, so every muscle keeps its usual weekly target for now."
                    .into(),
            ),
            (None, _) => notes.push(format!(
                "\"{}\" isn't a muscle the plan tracks, so every muscle keeps your weekly target.",
                raw.trim()
            )),
        }
    }
    if !uncovered.is_empty() {
        notes.push(format!(
            "No exercise in your plan trains {} directly yet. Log one you like and it joins the plan.",
            uncovered.join(", ")
        ));
    }
    (rows, notes.join(" "))
}
