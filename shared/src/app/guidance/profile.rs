use crate::app::evidence_views::describe;
use crate::app::format::{fmt_u8_range, fmt_whole_km_or_mi};
use crate::app::guidance::hybrid::build_hybrid_guidance;
use crate::app::guidance::rows::{
    WeeklyBasis, push_guidance, push_hold_row, training_age_label, weekly_basis_text,
};
use crate::app::guidance::running::build_running_guidance;
use crate::app::guidance::strength::build_strength_guidance;
use crate::app::view_types::GuidanceView;
use crate::evidence::claims;
use crate::hybrid;
use crate::individualization::{self, Environment};
use crate::memo::{MemoId, fingerprint, memoized};
use crate::profile::{PlanProfile, Profile};
use crate::schema::{AdjustmentCall, AdjustmentKind, Recommended, ScreenGate};

/// The section of the profile-context rows (the training age), which the view
/// sends apart from the rest of the guidance (`ViewModel::profile_guidance`).
pub(in crate::app) const PROFILE_SECTION: &str = "Profile";

/// The weekly running volume the guidance rows should reason about, and how it
/// was determined: the LARGER of the stated profile figure and the MEASURED
/// recent volume from logged runs, so a runner who set up at 30 km/wk but now
/// logs 70 still trips the volume-gated safety guards. `None` measured (no run
/// history) → the stated figure. Unlike the plan's layoff-aware
/// `plan::run::effective_weekly_km`, the stated figure always stays a floor
/// here: the safety guards must err towards the larger volume.
pub(in crate::app) fn stated_or_measured_weekly_km(
    p: &Profile,
    recent_weekly_km: Option<f64>,
) -> (f64, WeeklyBasis) {
    let stated = p.running_km_per_week;
    match recent_weekly_km {
        Some(m) if m > stated => (m, WeeklyBasis::Logged),
        _ => (stated, WeeklyBasis::Profile),
    }
}

/// `weekly_km` and `weekly_basis` come from [`stated_or_measured_weekly_km`],
/// so the km-gated safety/hybrid guards read the athlete's real recent
/// mileage rather than a stale stated figure; `hold` is today's
/// [`calculator_hold`].
pub(in crate::app) fn build_guidance_measured(
    p: &Profile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    hold: Option<&AdjustmentCall>,
) -> Vec<GuidanceView> {
    let key = (
        fingerprint(&(p, weekly_km, hold.map(|h| &h.value))),
        weekly_basis,
        hold.map(|h| (h.claim_id(), h.kind())),
    );
    memoized(MemoId::Guidance, &key, || {
        build_guidance_measured_uncached(p, weekly_km, weekly_basis, hold)
    })
}

fn build_guidance_measured_uncached(
    p: &Profile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    hold: Option<&AdjustmentCall>,
) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    build_safety_guidance(p, weekly_km, weekly_basis, &mut rows);
    // A hold from outside the profile (a readiness or weekly-check-in
    // deferral, a RED-S signal) says so in the Safety block; a profile gate
    // already leads it with its own row.
    if let Some(hold) = hold.filter(|_| !p.health.any_gate()) {
        push_hold_row(&mut rows, "Safety", hold);
    }

    let pp = PlanProfile::new(p);
    let age_r = pp.training_age_rec();
    let age = age_r.value;
    let before = rows.len();
    push_guidance(
        &mut rows,
        PROFILE_SECTION,
        format!("Training age: {}", training_age_label(age)),
        age_r,
    );
    if let Some(row) = rows.get_mut(before) {
        row.training_age = training_age_label(age).to_string();
    }

    // onboard-050: gates run BEFORE any prescription. A gated profile, or a
    // day held by a readiness / weekly-check-in deferral or RED-S (the same
    // hold Today and the calculators read; safety-040/043 "before
    // prescribing", autoreg-046), keeps only the safety block, its
    // training-age row and the environment notes: no %1RM, 1RM-testing,
    // volume-ramp, running-cap or hybrid rows (safety-011/045/049).
    if p.health.any_gate() || hold.is_some() {
        build_environment_guidance(p, &mut rows);
        return rows;
    }

    if pp.lifts() {
        build_strength_guidance(&pp, &mut rows);
    }

    // indiv-006: novices get the largest return per added set, advanced
    // lifters the smallest.
    let vs = individualization::volume_sensitivity(age);
    push_guidance(
        &mut rows,
        "Your level",
        match vs.value {
            individualization::VolumeSensitivity::High => {
                "Added sets pay off most for you at this stage"
            }
            individualization::VolumeSensitivity::Moderate => {
                "Added sets still pay off for you, though less than for a beginner"
            }
            individualization::VolumeSensitivity::Low => {
                "Added sets pay off least at your level, so add them sparingly"
            }
        }
        .to_string(),
        &vs,
    );

    if pp.runs() {
        build_running_guidance(&pp, &mut rows);
    }

    build_environment_guidance(p, &mut rows);
    build_reentry_guidance(p, &mut rows);

    if pp.lifts() && pp.runs() {
        build_hybrid_guidance(&pp, weekly_km, weekly_basis, &mut rows);
    }

    rows
}

fn build_safety_guidance(
    p: &Profile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    rows: &mut Vec<GuidanceView>,
) {
    // Stage-0 onboarding gates lead every guidance list (File 08 onboard-050:
    // screen BEFORE any prescription; safety-000: never overridden by goals).
    // Each fired gate renders as a Safety row with its deferral reason. The
    // reason is written to sit under Today's hold banner, so where the banner
    // carries the "why" the standalone row restates it first.
    for gate in individualization::onboarding_gates(&p.health) {
        let lead = match gate.kind() {
            AdjustmentKind::Screen(ScreenGate::AcuteIllness) => "You marked yourself ill. ",
            AdjustmentKind::Screen(ScreenGate::Parq) => {
                "One of your health answers needs a doctor's OK first. "
            }
            AdjustmentKind::Screen(ScreenGate::Youth) => {
                "You're under 18, so lollop won't build a plan: strength training at your age needs a coach who can watch your technique. You can still log and track. "
            }
            AdjustmentKind::Screen(ScreenGate::Pregnancy) => {
                "You're pregnant, so lollop won't plan or increase your training. "
            }
            _ => "",
        };
        push_guidance(
            rows,
            "Safety",
            format!("{lead}{}", describe(&gate.value)),
            &gate,
        );
    }
    // Pregnancy avoid-list (safety-047) travels with the safety-045 deferral,
    // then every warning sign, so the user knows when to stop (safety-046).
    if p.health.pregnant {
        let pre = individualization::pregnancy_precautions();
        push_guidance(
            rows,
            "Safety",
            format!(
                "Pregnancy: avoid lying flat on your back for long, overheating, sports with contact or fall risk, scuba diving, altitude above {:.0} m, and holding your breath while lifting",
                pre.value.avoid_altitude_above_m
            ),
            &pre,
        );
        let warn = Recommended::new((), claims::SAFE_PREG_WARN_001);
        push_guidance(
            rows,
            "Safety",
            "Pregnancy: stop and contact your doctor or midwife if you notice vaginal bleeding, fluid leaking, regular painful contractions, breathlessness before exercise, dizziness or feeling faint, headache, chest pain, calf pain or swelling, muscle weakness affecting your balance, or the baby moving less".to_string(),
            &warn,
        );
    }

    // Safety-critical: high weekly mileage raises bone-stress-injury surveillance
    // (File 10 hybrid-023). Only surfaced once the profile's mileage crosses the
    // threshold, so a low-volume runner is not warned needlessly. Lives in the
    // leading Safety block so the section stays contiguous with the gates.
    let bsi = hybrid::bsi_surveillance_flag(weekly_km);
    if bsi.value {
        let unit = p.display_distance_unit();
        let bsi_text = format!(
            "Bone stress: you run {}, above ~{}. Watch for pain in one spot on a bone, and eat enough for your training",
            weekly_basis_text(weekly_km, weekly_basis, unit),
            fmt_whole_km_or_mi(hybrid::BSI_SURVEILLANCE_KM_PER_WEEK, unit)
        );
        push_guidance(rows, "Safety", bsi_text, &bsi);
        // hybrid-024 energy-availability guard: high-volume endurance is one
        // of its higher-risk cohorts.
        let ea = Recommended::new((), claims::HYB_EA_001);
        push_guidance(
            rows,
            "Safety",
            "High running volume makes under-eating more likely (RED-S). Eat enough to match your training".to_string(),
            &ea,
        );
    }
}

fn build_environment_guidance(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // File 08 indiv-025 / safety-024 environment modifiers, when declared.
    if let Some(env) = p.environment {
        let m = individualization::environment_modifier(env);
        let text = match env {
            Environment::Heat => {
                // The KB day range is optional; without it the clause is
                // dropped rather than printing an invented figure (HARD RULE 1).
                let acclimatize = m
                    .value
                    .acclimatization_days
                    .map(|(lo, hi)| format!(". Build up heat exposure over ~{lo}–{hi} days"))
                    .unwrap_or_default();
                format!(
                    "Heat: ease off the effort and drink enough{acclimatize}. Stop at once on signs of heat illness (confusion, dizziness, or you stop sweating)"
                )
            }
            Environment::Altitude => {
                "Altitude (above ~2,500 m): go easier until your body adjusts".to_string()
            }
            Environment::Cold => "Cold: extend the warm-up".to_string(),
            Environment::Neutral => return,
        };
        push_guidance(rows, "Environment", text, &m);
    }
}

fn build_reentry_guidance(p: &Profile, rows: &mut Vec<GuidanceView>) {
    // REENTRY-001 layoff re-entry ramp + the post-layoff MEV reduction.
    if let Some(weeks_off) = p.weeks_off
        && weeks_off > 0.0
    {
        let re = individualization::resistance_reentry(weeks_off);
        // The >8 wk bracket carries NO KB load fraction (`load_frac == None`),
        // so render a fresh-start message with no invented percentage (KB Table
        // 3.4b language). The 1-8 wk numeric brackets keep the derate sentence.
        let weeks = |n: u8| if n == 1 { "week" } else { "weeks" };
        let off = match weeks_off.round() {
            n if n < 1.0 => "under a week".to_string(),
            n => format!("{n:.0} {}", weeks(n as u8)),
        };
        let (lo, hi) = re.value.ramp_weeks;
        let summary = match re.value.load_frac {
            Some(_) if hi == 0 => format!("After {off} off: carry on at your old weights"),
            Some(frac) => format!(
                "After {off} off: restart at ~{:.0}% of your old weights and build back over {} {}",
                frac * 100.0,
                fmt_u8_range((lo, hi)),
                weeks(hi)
            ),
            None => format!(
                "After {off} off: treat it as a fresh start. Rebuild your technique and build back over 4–6+ weeks, progressing like a beginner."
            ),
        };
        push_guidance(rows, "Return to training", summary, &re);
        let mev = Recommended::new((), claims::HYP_LAYOFF_001);
        push_guidance(
            rows,
            "Return to training",
            "After a break, fewer sets regrow muscle: restart below your old set counts"
                .to_string(),
            &mev,
        );
    }
}
