//! Evidence presentation: `Recommended<T>` → `AdjustmentView`, the grade
//! definitions, claim statements, the "why" sheet, and grade notes.

use super::*;

/// Evidence-grade rank for headline priority: the `EvidenceGrade` declaration
/// order (Strong > Moderate > Weak > ExpertOpinion > MarketingMyth); a row
/// with no grade sorts last.
pub(super) fn grade_rank(grade: Option<EvidenceGrade>) -> i32 {
    grade.map_or(-1, |g| g as i32)
}

/// Flatten one evidence-wrapped adjustment into a shell-facing row: the call,
/// the exercise an item-scoped call names, its safety-ladder tier (for the
/// headline ranking) and, for a progression hold, a why? basis that states the
/// rule actually applied before the claim behind it.
pub(super) fn to_view(r: &Recommended<Adjustment>) -> AdjustmentView {
    let mut v = to_view_with(describe_rec(r), r);
    if let Adjustment::ForItem {
        exercise,
        adjustment,
    } = &r.value
    {
        v.exercise = Some(exercise.clone());
        // autoreg-013: a later set that failed ends the exercise in the loop.
        v.suggests_end_exercise = matches!(**adjustment, Adjustment::DowngradeSession)
            && r.claim_id() == claims::AUTOREG_RIR_001;
    }
    v.tier = adjustment_tier(r);
    if let Some(applied) = applied_rule(r) {
        v.why.basis = format!("{applied} {}", v.why.basis);
    }
    v
}

/// The plan change a hold-type call made today, in the words the plan shows,
/// so the why? panel opens on the rule that was applied.
fn applied_rule(r: &Recommended<Adjustment>) -> Option<&'static str> {
    match (&r.value, r.claim_id()) {
        (
            Adjustment::CapTopEnd {
                trigger: ReadinessSignal::Soreness,
            },
            _,
        ) => Some(
            "Applied today: no lift goes heavier than your best for the same reps, every lift stops 1 rep earlier, and runs stay as planned.",
        ),
        (Adjustment::CapTopEnd { .. }, _) | (Adjustment::DowngradeSession, claims::HRV_001) => {
            Some(
                "Applied today: no lift goes heavier than your best for the same reps, every lift stops 1 rep earlier, and runs stay easy (zones 1–2).",
            )
        }
        (Adjustment::DowngradeSession, claims::AUTOREG_VL_001) => Some(
            "Applied today: no lift goes heavier than your best for the same reps and every lift stops 1 rep earlier; runs stay as planned.",
        ),
        (Adjustment::DowngradeSession, claims::RUN_DECOUPLE_001) => {
            Some("Applied today: runs stay easy (zones 1–2); lifts as planned.")
        }
        _ => None,
    }
}

/// The "first set decides the rest" sentence a progression hold's call ends
/// on; dropped once a work set is logged today.
pub(super) const FIRST_SET_CLAUSE: &str =
    " Log your first work set: if it feels as expected, the rest of the session stays as planned.";

/// "1 rep" / "2 reps".
pub(super) fn reps_word(n: u8) -> String {
    if n == 1 {
        "1 rep".into()
    } else {
        format!("{n} reps")
    }
}

/// A deload's cuts in plain words ("50% fewer sets, 10% less weight"), leaving
/// out a zero cut; `None` when both are zero.
pub(super) fn deload_cut_clauses(volume_pct: f32, load_pct: f32) -> Option<String> {
    let mut parts = Vec::new();
    if volume_pct.round() > 0.0 {
        parts.push(format!("{volume_pct:.0}% fewer sets"));
    }
    if load_pct.round() > 0.0 {
        parts.push(format!("{load_pct:.0}% less weight"));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// The short easy dose after an illness, in minutes and as a share of usual
/// effort: the post-illness return check (ILLNESS-RTT-001) and the head-cold
/// run cap (ILLNESS-COLD-RUN-001) share the IOC 2022 exercise-challenge dose.
pub(super) const RETURN_DOSE_MIN: (u16, u16) = (10, 20);
pub(super) const RETURN_DOSE_EFFORT_PCT: (u8, u8) = (60, 70);

/// The return check's one-line dose, shared by the call and its plan item.
pub(super) fn return_check_headline() -> String {
    let (min_lo, min_hi) = RETURN_DOSE_MIN;
    let (pct_lo, pct_hi) = RETURN_DOSE_EFFORT_PCT;
    format!(
        "Return check: {min_lo}–{min_hi} minutes easy at {pct_lo}–{pct_hi}% of your usual effort"
    )
}

/// When to stop a return check and see a doctor (ILLNESS-RTT-001), the one
/// wording the call and its plan item share.
pub(super) const RETURN_CHECK_STOP: &str = "Stop and see a doctor if you get chest pain, breathlessness, dizziness, a racing or pounding heartbeat, unusual tiredness, or muscle or joint pain. The same goes if the work feels harder than usual.";

/// The HRV hold's reason: it fires on the 7-day mean.
const HRV_WEEK: &str = "HRV has been below your normal range this week";
/// The same hold when only the latest reading is suppressed (the HRV
/// channel's streak is 1): one low morning pulled the 7-day mean down. That
/// morning may be an earlier one, so the reason names no day.
const HRV_MORNING: &str = "HRV was below your normal range on your latest reading";

/// Whether the HRV hold rests on one morning alone: the latest HRV input is
/// below the SWC band and its suppressed-day streak is 1.
pub(super) fn hrv_single_morning(inputs: &[ReadinessInput]) -> bool {
    inputs
        .iter()
        .filter(|i| i.signal == ReadinessSignal::HrvLnRmssd)
        .max_by_key(|i| i.observed_at)
        .is_some_and(|i| autoreg::hrv_below_swc(i.value) && i.streak == 1)
}

/// Name the latest reading rather than "this week" on an HRV hold row when
/// the trend rests on one low morning ([`hrv_single_morning`]).
pub(super) fn hrv_morning_wording(v: &mut AdjustmentView) {
    if v.summary.contains(HRV_WEEK) {
        v.summary = v.summary.replace(HRV_WEEK, HRV_MORNING);
    }
}

/// The check-in hold's reason against a known personal normal.
const CHECKIN_BELOW_NORMAL: &str = "check-in is well below your normal";
/// The same hold before the wellness baseline exists: only the absolute floor
/// fires it, so it names the reading, not a normal not yet known. The reading
/// may be yesterday's (still in force), so it names no day.
const CHECKIN_VERY_LOW: &str = "latest check-in is very low";

/// Say "very low" rather than "below your normal" on a check-in hold row
/// while the wellness baseline is still collecting.
pub(super) fn checkin_floor_wording(v: &mut AdjustmentView) {
    if v.summary.contains(CHECKIN_BELOW_NORMAL) {
        v.summary = v.summary.replace(CHECKIN_BELOW_NORMAL, CHECKIN_VERY_LOW);
    }
}

/// What a lift-holding readiness call says about the lifts.
const HOLD_LIFTS: &str = "lifts add no weight and stop 1 rep earlier";

/// A lift-holding readiness call (no load added, one more rep in reserve):
/// the signal that fired as a sentence opener, whether runs go easy, and
/// whether the first-set clause follows. A check-in signal stays in force for
/// its recency window after the day it was given, so no opener says "today".
fn hold_reason(r: &Recommended<Adjustment>) -> Option<(&'static str, bool, bool)> {
    match (&r.value, r.claim_id()) {
        (Adjustment::CapTopEnd { trigger }, _) => Some(match trigger {
            ReadinessSignal::HrvLnRmssd => (HRV_WEEK, true, true),
            ReadinessSignal::RestingHr => ("Resting HR up 2 mornings running", true, true),
            ReadinessSignal::Soreness => ("You reported being very sore", false, true),
            _ => ("Your check-in is well below your normal", true, true),
        }),
        (Adjustment::DowngradeSession, claims::HRV_001) => Some((
            "HRV has dropped below your normal range and is trending down",
            true,
            false,
        )),
        _ => None,
    }
}

/// The one call for one or more lift-holding readiness signals firing
/// together: they apply once (never stacked), so they read as one reason.
fn hold_summary(reasons: &[(&'static str, bool, bool)]) -> String {
    let mut who = String::new();
    for (k, (reason, ..)) in reasons.iter().enumerate() {
        if k > 0 {
            who.push_str(if k + 1 == reasons.len() {
                " and "
            } else {
                ", "
            });
            let mut c = reason.chars();
            if let Some(f) = c.next() {
                let acronym = c.clone().next().is_some_and(char::is_uppercase);
                if acronym {
                    who.push(f);
                } else {
                    who.extend(f.to_lowercase());
                }
                who.push_str(c.as_str());
            }
        } else {
            who.push_str(reason);
        }
    }
    let runs_easy = reasons.iter().any(|(_, easy, _)| *easy);
    let first_set = if reasons.iter().any(|(.., f)| *f) {
        FIRST_SET_CLAUSE
    } else {
        ""
    };
    if runs_easy {
        format!("{who}: runs go easy; {HOLD_LIFTS}.{first_set}")
    } else {
        format!("{who}: {HOLD_LIFTS}; runs as planned.{first_set}")
    }
}

/// Merge the rows of several lift-holding readiness calls (`rows[k]` is
/// `to_view(&recs[k])`) into the first one's, naming every signal. Returns the
/// rows to drop (the merged-away ones).
pub(super) fn merge_hold_rows(
    recs: &[Recommended<Adjustment>],
    rows: &mut [AdjustmentView],
) -> Vec<bool> {
    let mut drop = vec![false; recs.len()];
    let holds: Vec<(usize, (&'static str, bool, bool))> = recs
        .iter()
        .enumerate()
        .filter_map(|(k, r)| hold_reason(r).map(|h| (k, h)))
        .collect();
    if let [(first, _), rest @ ..] = holds.as_slice()
        && !rest.is_empty()
    {
        let reasons: Vec<_> = holds.iter().map(|(_, h)| *h).collect();
        if let Some(row) = rows.get_mut(*first) {
            row.summary = hold_summary(&reasons);
        }
        for (k, _) in rest {
            drop[*k] = true;
        }
    }
    drop
}

/// The one-line call for a readiness/safety adjustment, naming WHICH signal
/// fired and what changes today, in plain language. Falls back
/// to [`describe`] for calls that need no signal-specific wording.
pub(super) fn describe_rec(r: &Recommended<Adjustment>) -> String {
    if let Some(h) = hold_reason(r) {
        return hold_summary(&[h]);
    }
    let claim = r.claim_id();
    match (&r.value, claim) {
        (Adjustment::DowngradeSession, claims::WELLNESS_001) => {
            "Your check-ins have been well below your normal for several days: today is one step easier, with about 5% less weight, stopping 1 rep earlier on every set, and easy runs.".into()
        }
        (Adjustment::DowngradeSession, claims::AUTOREG_VL_001) => {
            "Bar speed dropped past your goal's limit within a set: stop 1 rep earlier and add no weight. Runs as planned.".into()
        }
        (Adjustment::DowngradeSession, claims::RUN_DECOUPLE_001) => {
            "Your heart rate drifted up 10% or more on a steady run: keep runs easy and hold off on harder running; lifts as planned.".into()
        }
        (Adjustment::DowngradeSession, claims::AUTOREG_RHR_NOISE_001) => {
            "Your latest resting HR is well above your normal: an easier session today. Feeling ill? Answer the illness question in your check-in.".into()
        }
        (Adjustment::DowngradeSession, claims::ILLNESS_NECK_001) => {
            "Until you've recovered: no long or hard sessions and no heavy weights.".into()
        }
        (Adjustment::ReduceVolumePct(_), claims::ILLNESS_NECK_001) => {
            "Head cold: short and easy only, or rest. Runs: 10–20 min easy. Lifts: half the sets, kept light.".into()
        }
        (Adjustment::Stop, claims::ILLNESS_NECK_001) => {
            "Flu or fever: no training until you've recovered. Your first session back is a short, easy test.".into()
        }
        (Adjustment::DowngradeSession, claims::RETURN_HOLD_001) => {
            "Welcome back: today's session is easy (3+ reps left on every set, easy runs), then the plan resumes.".into()
        }
        (Adjustment::DowngradeSession, claims::SAFE_TENDON_001) => {
            "Tendon pain is flaring: an easy session today with lighter weights.".into()
        }
        (Adjustment::ReturnCheck, _) => format!("{}. {RETURN_CHECK_STOP}", return_check_headline()),
        (Adjustment::RestDay, claims::AUTOREG_RHR_NOISE_001) => {
            "Resting HR has been well above your normal 2 mornings running (or with illness symptoms): take a rest day and check whether you're getting ill.".into()
        }
        (Adjustment::StopItems { location }, _) => format!(
            "Pain ({location}): skip the exercises that load it today. The rest of the session goes ahead without adding weight."
        ),
        (
            Adjustment::ForItem {
                exercise,
                adjustment,
            },
            _,
        ) => match (adjustment.as_ref(), claim) {
            (Adjustment::ReduceLoadPct(p), claims::AUTOREG_RIR_001) => format!(
                "{exercise}: your first set was harder than planned, so the remaining sets drop {p:.0}%."
            ),
            (Adjustment::IncreaseLoadPct(p), claims::AUTOREG_RIR_001) => format!(
                "{exercise}: your first set felt easier than planned. You can add about {p:.0}% for the remaining sets."
            ),
            (Adjustment::DowngradeSession, claims::AUTOREG_RIR_001) => format!(
                "{exercise}: your last set went to failure before the planned sets were done. End the exercise here, or drop the weight so the remaining sets end with reps left."
            ),
            (Adjustment::ReduceLoadPct(p), _) => format!(
                "{exercise}: today's top set is below your recent best, so the remaining sets drop {p:.0}%."
            ),
            (Adjustment::CapRpe(d), _) => format!(
                "{exercise}: stop {} earlier than planned on the remaining sets.",
                reps_word(*d)
            ),
            (Adjustment::IncreaseLoadPct(p), _) => format!(
                "{exercise}: today's top set beat your recent best. You can add about {p:.0}% next time."
            ),
            (inner, _) => format!("{exercise}: {}", describe(inner)),
        },
        _ => describe(&r.value),
    }
}

/// Flatten any evidence-wrapped value into an adjustment row with an explicit
/// summary, for graded verdicts that are not `Adjustment`s (e.g. the
/// autoreg-032 threshold re-test cue).
pub(super) fn to_view_with<T>(summary: String, r: &Recommended<T>) -> AdjustmentView {
    AdjustmentView {
        summary,
        evidence: r.into(),
        // The basis is the claim statement; the improves/grade lines carry the
        // KB-grounded explanation.
        why: why_from(None, r),
        // Real graded adjustments default to non-neutral; the two "no change"
        // review verdicts (VBT hold, hold planned sets) flip this on the row.
        neutral: false,
        ..AdjustmentView::default()
    }
}

/// KB definition of what each evidence grade means (File 09), plus the default
/// confidence it maps to, the data behind the "How evidence grading works"
/// legend sheet. `MarketingMyth` is included so the legend can name the
/// hard-blocked bottom of the scale, but it is never emitted on a card.
pub(super) fn grade_definitions() -> std::borrow::Cow<'static, [GradeDefView]> {
    // The File 09 grade legend is a fixed five-row table, memoize (see
    // `build_reference`).
    static GRADE_DEFINITIONS: std::sync::LazyLock<Vec<GradeDefView>> =
        std::sync::LazyLock::new(grade_definitions_impl);
    std::borrow::Cow::Borrowed(&GRADE_DEFINITIONS)
}

fn grade_definitions_impl() -> Vec<GradeDefView> {
    [
        EvidenceGrade::Strong,
        EvidenceGrade::Moderate,
        EvidenceGrade::Weak,
        EvidenceGrade::ExpertOpinion,
        EvidenceGrade::MarketingMyth,
    ]
    .into_iter()
    .map(|g| {
        // Plain words for the legend (File 09 grade meanings without the
        // research vocabulary).
        let (label, definition) = match g {
            EvidenceGrade::Strong => ("Strong", "Many good studies agree."),
            EvidenceGrade::Moderate => (
                "Moderate",
                "Some good studies support it, but it isn't settled yet.",
            ),
            EvidenceGrade::Weak => (
                "Weak",
                "Based on how the body works or on observation, not direct trials.",
            ),
            EvidenceGrade::ExpertOpinion => (
                "Expert opinion",
                "What experienced coaches do; not yet tested in trials.",
            ),
            EvidenceGrade::MarketingMyth => {
                ("Myth", "Studies contradict it, so the app never uses it.")
            }
        };
        GradeDefView {
            grade: g,
            label: label.to_string(),
            definition: definition.to_string(),
            confidence: g.default_confidence(),
        }
    })
    .collect()
}

/// Compose the "why?" triad from a `Recommended<T>`, optionally overriding the
/// `basis` line with a datum-rich, call-site-specific sentence (e.g. the
/// HR-zones card restating the user's age through the Tanaka formula). When no
/// override is given, the basis is the backing claim's own registered
/// statement, always KB-grounded (HARD RULE 1).
pub(super) fn why_from<T>(basis_override: Option<String>, r: &Recommended<T>) -> WhyView {
    WhyView {
        basis: basis_override.unwrap_or_else(|| r.evidence().statement.to_owned()),
        grade_note: grade_note_str(r.grade(), r.contested()),
        improves: improves_for(r.claim_id()).to_owned(),
    }
}

/// One-sentence gloss of what an evidence grade means for a claim, with the
/// contested question (`contested_ref`, e.g. `"CQ-01"`) appended when the
/// claim is under active debate.
pub(super) fn grade_note_str(grade: EvidenceGrade, contested_ref: Option<&str>) -> String {
    let base = match grade {
        EvidenceGrade::Strong => "Strong evidence: many good studies agree.",
        EvidenceGrade::Moderate => {
            "Moderate evidence: some good studies support it, but it isn't settled yet."
        }
        EvidenceGrade::Weak => {
            "Weak evidence: based on how the body works or on observation, not direct trials."
        }
        EvidenceGrade::ExpertOpinion => {
            "Expert opinion: what experienced coaches do; not yet tested in trials."
        }
        EvidenceGrade::MarketingMyth => return String::new(),
    };
    let Some(cq) = contested_ref.and_then(crate::evidence::contested_question) else {
        return base.to_string();
    };
    // Honour the deck's "here's both sides" promise: name the open question
    // AND say what the app does meanwhile, rather than dangling a bare topic
    // label.
    let mut s = format!(
        "{base} Experts still disagree on {}, so this is contested. What the app does for now: {}.",
        cq.question, cq.engine_default
    );
    // When the KB supplies an attributable opposing reference, show one
    // contradicting side. Left off entirely when the CQ has no KB-sourced
    // opposing cite (`other_side` is None) - never a fabricated "other view".
    if let Some(other) = cq.other_side {
        s.push_str(" One view on the other side: ");
        s.push_str(other);
    }
    s
}

/// The engagement-loop line for the "why?" disclosure: which input the rule
/// lacks or would sharpen. Keyed by claim, this describes the ENGINE's own
/// data needs, never training advice, so it carries no evidence tag and
/// invents no claim (HARD RULE 1-safe by construction; every string is reviewed
/// against that bar). `"-"` when nothing would genuinely improve the estimate.
pub(super) fn improves_for(claim: ClaimId) -> &'static str {
    match claim {
        claims::RUN_HRMAX_001 => {
            "Log a measured max HR from an all-out effort to replace the age-based estimate."
        }
        claims::RUN_VDOT_001 => {
            "Add a recent race time so these zones anchor to your own VDOT instead of a default."
        }
        claims::HRV_001 | claims::AUTOREG_HRV_SAT_001 => {
            "Keep logging morning check-ins. More readings sharpen your HRV normal."
        }
        claims::WELLNESS_001 | claims::AUTOREG_WELLNESS_RHR_001 => {
            "A couple more weeks of morning check-ins sharpen what counts as normal for you."
        }
        claims::AUTOREG_RHR_DOWN_001 | claims::AUTOREG_RHR_NOISE_001 => {
            "Log your resting HR a few more mornings to sharpen your normal."
        }
        claims::AUTOREG_HRV_LIFT_001 | claims::AUTOREG_RIR_001 => {
            "Rate how hard your first work set felt; that decides the rest of the session."
        }
        claims::RETURN_HOLD_001 | claims::ILLNESS_RTT_001 => {
            "Tomorrow's check-in decides whether you're ready for the full plan again."
        }
        claims::RUN_DECOUPLE_001 => {
            "Log more long runs with heart rate to track how your aerobic base is trending."
        }
        claims::STR_PRILEPIN_001 => {
            "Log your working sets so the app can check each session's reps against the target."
        }
        claims::AUTOREG_E1RM_GATE_001 => {
            "Log a few more sessions at this intensity to confirm the trend."
        }
        claims::YOUTH_PA_001 => {
            "Log your sports, PE lessons and other activities so the week's total is complete."
        }
        _ => "-",
    }
}

/// Human-readable one-liner for an adjustment.
pub(super) fn describe(a: &Adjustment) -> String {
    match a {
        Adjustment::ReduceLoadPct(p) => {
            format!("Drop the weight {p:.0}% for the remaining sets")
        }
        // Never folded into the rendered session (the safe choice, an increase is
        // opt-in, unlike a safety cut), so the copy tells the user to apply it.
        // It fires on an easy effort rating or an estimated-max gain.
        Adjustment::IncreaseLoadPct(p) => {
            format!("You're ahead of plan: you can add about {p:.0}% to the weight next session.")
        }
        Adjustment::Deload {
            volume_reduction_pct,
            load_reduction_pct,
            weeks,
        } => {
            let head = if *weeks <= 1 {
                "Lighter week".to_string()
            } else {
                format!("Lighter week for {weeks} weeks")
            };
            match deload_cut_clauses(*volume_reduction_pct, *load_reduction_pct) {
                Some(cuts) => format!("{head}: {cuts}"),
                None => head,
            }
        }
        Adjustment::CapRpe(d) => {
            format!(
                "Stop {} earlier than planned on every set today",
                reps_word(*d)
            )
        }
        Adjustment::DowngradeSession => "An easier session today".into(),
        Adjustment::ModifyAndMonitor { location } => {
            if crate::autoreg::pain_region(location).is_some() {
                format!(
                    "Tendon pain ({location}): keep the exercises that load it light and pain-free, and train the rest without adding weight. Avoid complete rest."
                )
            } else if location.is_empty() {
                "Tendon pain: keep every exercise light and pain-free today. Avoid complete rest."
                    .into()
            } else {
                format!(
                    "Tendon pain ({location}): keep every exercise light and pain-free today. Avoid complete rest."
                )
            }
        }
        Adjustment::RestDay => "Take a full rest day".into(),
        Adjustment::Stop => "Stop: do not train".into(),
        Adjustment::Defer { reason } => reason.clone(),
        Adjustment::ForItem {
            exercise,
            adjustment,
        } => format!("{exercise}: {}", describe(adjustment)),
        Adjustment::CapTopEnd { .. } => {
            "Hold progression today: planned weights, stop 1 rep earlier; easy runs".into()
        }
        Adjustment::ReduceVolumePct(p) => format!("Cut today's volume about {p:.0}%"),
        Adjustment::StopItems { location } => {
            format!("Skip the exercises that load the painful area ({location})")
        }
        Adjustment::ReturnCheck => return_check_headline(),
    }
}
