use super::*;

/// Calisthenics-only guidance rows (same `GuidanceView` shape as the lift audit),
/// surfaced only for a `Calisthenics` profile: the muscle-up count gate (when
/// muscle-up rungs are logged but the gate is unmet), a hint when no skill track
/// is active yet, the equipment swaps in force, and the safety cues for the
/// active rungs. Plain language: no internal gate codes. Empty for a
/// `Weights` profile.
pub(in crate::app) fn build_calisthenics_guidance(cx: &ViewCtx) -> Vec<GuidanceView> {
    use crate::calisthenics as cal;
    use crate::schema::LadderId;
    let model = cx.model;
    let mut rows = Vec::new();
    let Some(profile) = model
        .profile
        .as_ref()
        .filter(|p| p.resistance_modality == ResistanceModality::Calisthenics)
    else {
        return rows;
    };
    let anchors = cx.calis_anchors_now();
    let kit = cal::Kit::from_profile(profile);

    // Muscle-up rungs logged but the pull-up/dip count gate is unmet: engine
    // default, contested per the claim's own contested ref.
    if let Some(mu) = anchors.iter().find(|a| a.ladder == LadderId::MuscleUp)
        && !mu.active
    {
        push_guidance(
            &mut rows,
            "Calisthenics",
            format!(
                "Muscle-up work starts at {n} strict pull-ups and {n} dips in a set \
                 (a common coaching threshold; some athletes start earlier).",
                n = cal::MUSCLE_UP_GATE_REPS
            ),
            &graded((), claims::CAL_GATE_NUMBERS_001),
        );
    }

    // Dormant skill tracks: no skill anchor is active → invite the user to start
    // one, naming only the tracks their equipment allows.
    let any_skill_active = anchors
        .iter()
        .any(|a| a.active && cal::ladder(a.ladder).skill);
    if !any_skill_active {
        let starts: Vec<&str> = [
            (LadderId::Handstand, "wall walks (handstand)"),
            (LadderId::FrontLever, "tuck front lever"),
            (LadderId::Planche, "planche lean"),
            (LadderId::BackLever, "German hang (back lever)"),
            (LadderId::MuscleUp, "explosive pull-ups (muscle-up)"),
        ]
        .into_iter()
        .filter(|(id, _)| cal::ladder_available(*id, kit))
        .map(|(_, label)| label)
        .collect();
        push_guidance(
            &mut rows,
            "Calisthenics",
            format!(
                "Skill tracks are optional. To start one, pick it as a starting step or log a first set: {}.",
                starts.join(", ")
            ),
            &graded((), claims::CAL_PROG_STATIC_001),
        );
    }

    // Equipment swaps in force (CAL-EQUIP-001): say what replaces what, once.
    if !kit.hang() {
        push_guidance(
            &mut rows,
            "Calisthenics",
            "No pull-up bar or rings on your profile: pulling is done as table or \
             inverted rows, which also take over the pull-up sets."
                .to_string(),
            &cal::equipment_substitution(),
        );
    }
    if !kit.dip_support() {
        push_guidance(
            &mut rows,
            "Calisthenics",
            "No dip bars or rings on your profile: pike and handstand push-ups replace \
             every dip."
                .to_string(),
            &cal::equipment_substitution(),
        );
    }

    // Safety cues for the athlete's active tiers, surfaced as their own
    // evidence-cited rows so each claim's grade travels (HARD RULE 2). Wrist
    // extension gates hand-balancing; straight-arm lever tiers carry a tendon
    // rate-limit and a breath-holding hazard.
    let wrist_tier = anchors
        .iter()
        .filter(|a| a.active)
        .any(|a| cal::tier(a.ladder, a.tier).is_some_and(|t| t.wrist_gate));
    let straight_arm_tier = anchors
        .iter()
        .filter(|a| a.active)
        .any(|a| cal::tier(a.ladder, a.tier).is_some_and(|t| t.straight_arm));
    if wrist_tier {
        push_guidance(
            &mut rows,
            "Calisthenics",
            "Handstand and pike work loads your wrists. Warm up first (wrist circles, \
             rocking on all fours), and only progress while bending the wrist back \
             under load is pain-free."
                .to_string(),
            &graded((), claims::CAL_INJURY_WRIST_001),
        );
    }
    if straight_arm_tier {
        push_guidance(
            &mut rows,
            "Calisthenics",
            "Straight-arm holds load the elbow tendons, which adapt slowly: stay at \
             least 4 weeks per lever stage and stop if the front of the elbow hurts."
                .to_string(),
            &graded((), claims::CAL_TENDON_001),
        );
        push_guidance(
            &mut rows,
            "Calisthenics",
            "Keep breathing through every hold; never hold your breath.".to_string(),
            &graded((), claims::CAL_BP_VALSALVA_001),
        );
    }
    rows
}
