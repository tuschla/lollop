use super::*;
use crate::hybrid;

/// Where the weekly running volume the guidance reasons about came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum WeeklyBasis {
    /// Measured from the athlete's logged runs.
    Logged,
    /// The stated profile figure.
    Profile,
}

/// The weekly running volume the guidance rows should reason about, and how it
/// was determined: the LARGER of the stated profile figure and the MEASURED
/// recent volume from logged runs, so a runner who set up at 30 km/wk but now
/// logs 70 still trips the volume-gated safety guards. `None` measured (no run
/// history) → the stated figure.
pub(in crate::app) fn effective_weekly_km(
    p: &Profile,
    recent_weekly_km: Option<f64>,
) -> (f64, WeeklyBasis) {
    let stated = p.running_km_per_week;
    match recent_weekly_km {
        Some(m) if m > stated => (m, WeeklyBasis::Logged),
        _ => (stated, WeeklyBasis::Profile),
    }
}

/// The weekly volume with its source, in km or miles: "70 km a week from your
/// logged runs" / "19 mi a week from your profile".
pub(super) fn weekly_basis_text(weekly_km: f64, basis: WeeklyBasis, mi: bool) -> String {
    let source = match basis {
        WeeklyBasis::Logged => "from your logged runs",
        WeeklyBasis::Profile => "from your profile",
    };
    format!("{} a week {source}", fmt_whole_km_or_mi(weekly_km, mi))
}

/// Run the programming engine over the profile, producing evidence-cited rows.
/// Uses the STATED weekly volume (no logged-run history in scope) and no
/// training hold; `view()` calls [`build_guidance_measured`] with the measured
/// volume and today's [`calculator_hold`].
#[cfg(test)]
pub(in crate::app) fn build_guidance(p: &Profile) -> Vec<GuidanceView> {
    let (weekly_km, basis) = effective_weekly_km(p, None);
    build_guidance_measured(p, weekly_km, basis, None)
}

/// Memo key of [`build_guidance_measured`]: `(profile, weekly_km bits, basis,
/// hold)`.
type GuidanceKey = (Profile, u64, WeeklyBasis, Option<Recommended<Adjustment>>);

thread_local! {
    /// `build_guidance_measured_uncached` is a pure function of
    /// `(profile, weekly_km bits, weekly_basis, hold)`, and rebuilding the
    /// whole evidence-cited guidance list (every strength/running/hybrid rule)
    /// on every event is wasted work. The inputs change only when the profile,
    /// measured mileage or today's hold changes. Output is identical to the
    /// uncached rebuild.
    static GUIDANCE_CACHE: std::cell::RefCell<LastMemo<GuidanceKey, Vec<GuidanceView>>> =
        const { std::cell::RefCell::new(LastMemo::new()) };
}

/// Memoized wrapper over [`build_guidance_measured_uncached`], keyed on
/// `(profile, weekly_km bits, basis, hold)`.
pub(in crate::app) fn build_guidance_measured(
    p: &Profile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    hold: Option<&Recommended<Adjustment>>,
) -> Vec<GuidanceView> {
    let key_km = weekly_km.to_bits();
    let hit = GUIDANCE_CACHE.with(|c| {
        c.borrow()
            .get_if(|(cp, ck, cb, ch)| {
                cp == p && *ck == key_km && *cb == weekly_basis && ch.as_ref() == hold
            })
            .cloned()
    });
    if let Some(rows) = hit {
        return rows;
    }
    let rows = build_guidance_measured_uncached(p, weekly_km, weekly_basis, hold);
    GUIDANCE_CACHE.with(|c| {
        c.borrow_mut().set(
            (p.clone(), key_km, weekly_basis, hold.cloned()),
            rows.clone(),
        );
    });
    rows
}

/// The uncached guidance builder behind [`build_guidance_measured`]. As
/// [`build_guidance`] but with the caller's effective weekly volume + its
/// source, so the km-gated safety/hybrid guards read the athlete's real recent
/// mileage rather than a stale stated figure, and today's [`calculator_hold`].
fn build_guidance_measured_uncached(
    p: &Profile,
    weekly_km: f64,
    weekly_basis: WeeklyBasis,
    hold: Option<&Recommended<Adjustment>>,
) -> Vec<GuidanceView> {
    let mut rows = Vec::new();

    build_safety_guidance(p, weekly_km, weekly_basis, &mut rows);
    // A hold from outside the profile (a readiness or weekly-check-in
    // deferral, a RED-S signal) says so in the Safety block; a profile gate
    // already leads it with its own row.
    if let Some(hold) = hold.filter(|_| !p.health.any_gate()) {
        push_hold_row(&mut rows, "Safety", hold);
    }

    let lifting = p.weekly_sets > 0;
    let running = p.running_days_per_week > 0;

    let age_r = individualization::training_age_from_cadence(p.progression_cadence);
    let age = age_r.value;
    let before = rows.len();
    push_guidance(
        &mut rows,
        "Profile",
        format!("Training age: {}", training_age_label(age)),
        &age_r,
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

    if lifting {
        build_strength_guidance(p, age, &mut rows);
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

    // Training-age years for the running weekly volume-increase cap and the
    // hybrid lower-body interference check.
    let age_years = crate::plan::training_age_years(age);

    if running {
        build_running_guidance(p, age, age_years, &mut rows);
    }

    build_environment_guidance(p, &mut rows);
    build_reentry_guidance(p, &mut rows);

    if lifting && running {
        build_hybrid_guidance(p, weekly_km, weekly_basis, &mut rows);
    }

    rows
}

/// Leading Safety block: onboarding gates, pregnancy avoid-list, and the
/// high-mileage bone-stress / energy-availability guards (all `p`-gated).
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
        let lead = match gate.claim_id() {
            claims::ILLNESS_NECK_001 => "You marked yourself ill. ",
            claims::SAFE_CVD_001 => "One of your health answers needs a doctor's OK first. ",
            claims::SAFE_PEDS_001 => {
                "You're under 18, so lollop won't build a plan: strength training at your age needs a coach who can watch your technique. You can still log and track. "
            }
            claims::SAFE_PREG_001 => {
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
        let warn = graded((), claims::SAFE_PREG_WARN_001);
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
        let bsi_text = |mi: bool| {
            format!(
                "Bone stress: you run {}, above ~{}. Watch for pain in one spot on a bone, and eat enough for your training",
                weekly_basis_text(weekly_km, weekly_basis, mi),
                fmt_whole_km_or_mi(hybrid::BSI_SURVEILLANCE_KM_PER_WEEK, mi)
            )
        };
        push_guidance_mi(rows, "Safety", bsi_text(false), bsi_text(true), &bsi);
        // hybrid-024 energy-availability guard: high-volume endurance is one
        // of its higher-risk cohorts.
        let ea = graded((), claims::HYB_EA_001);
        push_guidance(
            rows,
            "Safety",
            "High running volume makes under-eating more likely (RED-S). Eat enough to match your training".to_string(),
            &ea,
        );
    }
}

/// Environment modifier row, when the profile declares one.
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

/// Layoff re-entry ramp + post-layoff MEV reduction, when a layoff is declared.
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
        let mev = graded((), claims::HYP_LAYOFF_001);
        push_guidance(
            rows,
            "Return to training",
            "After a break, fewer sets regrow muscle: restart below your old set counts"
                .to_string(),
            &mev,
        );
    }
}
