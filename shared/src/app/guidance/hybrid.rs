use crate::app::format::fmt_frac_pct_range;
use crate::app::guidance::rows::{WeeklyBasis, push_guidance, weekly_basis_text};
use crate::app::view_types::GuidanceView;
use crate::evidence::claims;
use crate::hybrid;
use crate::profile::{HybridLead, PlanProfile};
use crate::schema::Recommended;

/// Hybrid (concurrent lift + run) rows, gated on the athlete doing both. Every
/// row is read off the athlete's OWN plan (scheduled run days, the hardest
/// scheduled run, which side leads), never a self-reported intensity, and is a
/// full sentence (no raw true/false).
pub(super) fn build_hybrid_guidance(
    pp: &PlanProfile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    rows: &mut Vec<GuidanceView>,
) {
    let p = pp.profile();
    let lead = pp.hybrid_lead();
    let shape = pp.shape();
    let so = hybrid::same_session_order(p.concurrent_goal);
    // Full-sentence copy keyed on the ordering (the goal is implied by the
    // sentence), instead of leaking the raw Debug enum names.
    let so_text = match so.value {
        hybrid::SessionOrder::LiftFirst => {
            "Combined days: lift first, run after (strength and muscle goals)."
        }
        hybrid::SessionOrder::RunFirst => {
            "Combined days: running first is fine (endurance priority)."
        }
        hybrid::SessionOrder::ForbidSameSession => {
            "Power goal: keep lifting and running in separate sessions."
        }
    };
    push_guidance(rows, "Hybrid", so_text.to_string(), &so);

    let maint = Recommended::new((), claims::HYB_MAINT_002);
    let (lead_text, building, holding) = match lead {
        Some(HybridLead::Running) => (
            "Running leads, so lifting drops to a maintenance dose: about a third of the usual sets, still heavy (80% of your max or more), none added through the block.",
            "running",
            "lifting",
        ),
        _ => (
            "Lifting leads, so weekly running distance and effort hold steady instead of growing.",
            "lifting",
            "running",
        ),
    };
    push_guidance(rows, "Hybrid", lead_text.to_string(), &maint);
    // hybrid-025: tendon-stiffness evidence gap → progress one side at a time.
    let dual = Recommended::new((), claims::HYB_TENDON_001);
    push_guidance(
        rows,
        "Hybrid",
        format!(
            "One side progresses at a time: {building} builds while {holding} holds, because the combined strain of both on tendons has not been studied."
        ),
        &dual,
    );

    // The ≤3-run-day default applies only while lifting leads (HYB-RUNCAP-001).
    if lead == Some(HybridLead::Lifting) {
        let ef = hybrid::endurance_frequency_ok(p.running_days_per_week);
        let text = if ef.value {
            format!(
                "Running {} days a week alongside lifting: within the 3-day limit while lifting leads.",
                p.running_days_per_week
            )
        } else {
            format!(
                "You picked {} run days. While lifting leads, your plan schedules {} and shortens the week's distance to match, so each run keeps its usual length. Set \"Which leads?\" to Running to keep every run day.",
                p.running_days_per_week, shape.run_days
            )
        };
        push_guidance(rows, "Hybrid", text, &ef);
    }

    if let Some(llc) = hybrid::lower_lift_cap(shape.run_days, weekly_km) {
        let mid = (llc.value.volume_reduction_frac.0 + llc.value.volume_reduction_frac.1) / 2.0;
        let llc_text = format!(
            "With your running ({}), leg lifting is kept to {} days a week and leg sets are trimmed by about {:.0}% (guideline {}%).",
            weekly_basis_text(weekly_km, weekly_basis, p.distance_unit),
            llc.value.max_lower_sessions,
            mid * 100.0,
            fmt_frac_pct_range(
                llc.value.volume_reduction_frac.0,
                llc.value.volume_reduction_frac.1
            )
        );
        push_guidance(rows, "Hybrid", llc_text, &llc);
    }

    // Interference is judged from the plan's own run days and hardest
    // scheduled run (HYB-THRESH-001), not a self-reported %VO2max.
    if let Some((days, peak)) = crate::plan::plan_run_intensity(pp, weekly_km) {
        let ie = hybrid::interference_expected(days, peak);
        let hardest = if peak >= 95.0 {
            "hard intervals"
        } else if peak > 80.0 {
            "threshold runs"
        } else {
            "easy and steady runs"
        };
        let text = if ie.value {
            format!(
                "With your running load ({days} run days a week, hardest sessions: {hardest}), expect slightly smaller lifting gains."
            )
        } else {
            format!(
                "Your running load ({days} run days a week, hardest sessions: {hardest}) is low enough that lifting gains shouldn't be blunted."
            )
        };
        push_guidance(rows, "Hybrid", text, &ie);
        // hybrid-004: name the strongest lever when interference is expected.
        if ie.value {
            let im = Recommended::new((), claims::HYB_DURATION_001);
            push_guidance(
                rows,
                "Hybrid",
                "Run length matters most, then how often you run: shorter runs protect lifting gains better than fewer run days.".to_string(),
                &im,
            );
        }
    }

    // Sex is the supported moderator (HYB-SEX-001); training status
    // is not, so no one is told they are immune.
    let li = hybrid::expect_lower_strength_interference(p.female);
    let text = match li.value {
        hybrid::LowerStrengthInterference::PossibleSmall => {
            "Running alongside lifting can slightly blunt leg-strength gains (seen in men). For upper-body strength, muscle size and aerobic gains, no difference between men and women has been found."
        }
        hybrid::LowerStrengthInterference::NotShownInWomen => {
            "In women, running alongside lifting has not been shown to blunt leg-strength gains (few studies so far). For upper-body strength, muscle size and aerobic gains, no difference between men and women has been found."
        }
    };
    push_guidance(rows, "Hybrid", text.to_string(), &li);
}
