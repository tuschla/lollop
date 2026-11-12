//! Evidence presentation: `Recommended<T>` → `AdjustmentView`, the grade
//! definitions, claim statements, the "why" sheet, and grade notes.

use crate::app::format::fmt_reps;
use crate::app::plan_views::PlanItem;
use crate::app::view_types::{AdjustmentView, EvidenceTagView, GradeDefView, WhyView};
use crate::autoreg;
use crate::evidence::{ClaimId, EvidenceGrade, claims};
use crate::schema::{
    Adjustment, AdjustmentCall, AdjustmentKind, ReadinessInput, ReadinessSignal, Recommended,
};

/// The facts a call's wording depends on beyond the call itself. The default
/// renders a call on any day other than today.
#[derive(Clone, Copy, Default)]
pub(in crate::app) struct CallCtx<'a> {
    /// The HRV hold rests on one low morning ([`hrv_single_morning`]).
    pub(in crate::app) hrv_single_morning: bool,
    /// The wellness baseline is still collecting: only the absolute floor
    /// fires a check-in hold.
    pub(in crate::app) wellness_collecting: bool,
    /// A lift's work set is logged in today's session: the first-set clause
    /// is spent.
    pub(in crate::app) first_set_logged: bool,
    /// The session the call applies to (a tendon modify call names the lifts
    /// it eases in it).
    pub(in crate::app) items: &'a [PlanItem],
}

pub(in crate::app) fn lift_work_logged(items: &[PlanItem]) -> bool {
    items.iter().any(|it| it.is_lift() && it.has_work())
}

/// Flatten one evidence-wrapped adjustment into a shell-facing row: the call,
/// the exercise an item-scoped call names, its safety-ladder tier (for the
/// headline ranking) and, for a progression hold, a why? basis that states the
/// rule actually applied before the claim behind it.
pub(super) fn to_view(r: &AdjustmentCall) -> AdjustmentView {
    to_view_in(r, &CallCtx::default())
}

/// [`to_view`] worded against `ctx`.
pub(in crate::app) fn to_view_in(r: &AdjustmentCall, ctx: &CallCtx) -> AdjustmentView {
    call_view(describe_rec(r, ctx), r)
}

/// One row per call worded against `ctx`, each with the call it renders.
/// Several lift-holding readiness calls firing together apply once (never
/// stacked), so they read as one row naming every signal: the first hold's
/// row, the others merged into it.
pub(in crate::app) fn call_rows<'r>(
    calls: &'r [AdjustmentCall],
    ctx: &CallCtx,
) -> Vec<(&'r AdjustmentCall, AdjustmentView)> {
    let holds: Vec<(&AdjustmentCall, (&'static str, bool, bool))> = calls
        .iter()
        .filter_map(|r| lift_hold_reason(r, ctx).map(|h| (r, h)))
        .collect();
    let merged = holds.len() > 1;
    calls
        .iter()
        .filter_map(|r| match holds.first() {
            Some((first, _)) if merged && std::ptr::eq(*first, r) => {
                let reasons: Vec<_> = holds.iter().map(|(_, h)| *h).collect();
                Some((r, call_view(hold_summary(&reasons, ctx), r)))
            }
            _ if merged && holds.iter().any(|(h, _)| std::ptr::eq(*h, r)) => None,
            _ => Some((r, to_view_in(r, ctx))),
        })
        .collect()
}

fn call_view(summary: String, r: &AdjustmentCall) -> AdjustmentView {
    let mut v = to_view_with(summary, r);
    if let Adjustment::ForItem { exercise, .. } = &r.value {
        v.exercise = Some(exercise.clone());
        // autoreg-013: a later set that failed ends the exercise in the loop.
        v.suggests_end_exercise = r.kind() == AdjustmentKind::FailedSet;
    }
    if let Some(applied) = applied_rule(r) {
        v.why.basis = format!("{applied} {}", v.why.basis);
    }
    v
}

/// The plan change a hold-type call made today, in the words the plan shows,
/// so the why? panel opens on the rule that was applied.
fn applied_rule(r: &AdjustmentCall) -> Option<&'static str> {
    match r.kind() {
        AdjustmentKind::Soreness => Some(
            "Applied today: no lift goes heavier than your best for the same reps, every lift stops 1 rep earlier, and runs stay as planned.",
        ),
        AdjustmentKind::HrvHold
        | AdjustmentKind::WellnessHold
        | AdjustmentKind::RhrHold
        | AdjustmentKind::HrvLower => Some(
            "Applied today: no lift goes heavier than your best for the same reps, every lift stops 1 rep earlier, and runs stay easy (zones 1–2).",
        ),
        AdjustmentKind::VelocityLoss => Some(
            "Applied today: no lift goes heavier than your best for the same reps and every lift stops 1 rep earlier; runs stay as planned.",
        ),
        AdjustmentKind::Decoupling => {
            Some("Applied today: runs stay easy (zones 1–2); lifts as planned.")
        }
        _ => None,
    }
}

/// The "first set decides the rest" sentence a progression hold's call ends
/// on; dropped once a work set is logged today.
const FIRST_SET_CLAUSE: &str =
    " Log your first work set: if it feels as expected, the rest of the session stays as planned.";

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
pub(super) const RETURN_CHECK_STOP: &str = "Stop and see a doctor if you get chest pain, breathlessness, dizziness, a racing or pounding heartbeat, unusual tiredness, muscle or joint pain, or the work feels harder than usual.";

/// Whether the HRV hold rests on one morning alone: the latest HRV input is
/// below the SWC band and its suppressed-day streak is 1.
pub(super) fn hrv_single_morning(inputs: &[ReadinessInput]) -> bool {
    inputs
        .iter()
        .filter(|i| i.signal == ReadinessSignal::HrvLnRmssd)
        .max_by_key(|i| i.observed_at)
        .is_some_and(|i| autoreg::hrv_below_swc(i.value) && i.streak == 1)
}

/// What a lift-holding readiness call says about the lifts.
const HOLD_LIFTS: &str = "lifts add no weight and stop 1 rep earlier";

/// A lift-holding readiness call (no load added, one more rep in reserve):
/// the signal that fired as a sentence opener, whether runs go easy, and
/// whether the first-set clause follows. A check-in signal stays in force for
/// its recency window after the day it was given, so no opener says "today".
fn lift_hold_reason(r: &AdjustmentCall, ctx: &CallCtx) -> Option<(&'static str, bool, bool)> {
    Some(match r.kind() {
        // The hold fires on the 7-day mean; when only the latest reading is
        // suppressed (the HRV channel's streak is 1), one low morning pulled
        // the mean down. That morning may be an earlier one, so the reason
        // names no day.
        AdjustmentKind::HrvHold if ctx.hrv_single_morning => (
            "HRV was below your normal range on your latest reading",
            true,
            true,
        ),
        AdjustmentKind::HrvHold => ("HRV has been below your normal range this week", true, true),
        AdjustmentKind::RhrHold => ("Resting HR up 2 mornings in a row", true, true),
        AdjustmentKind::Soreness => ("You reported being very sore", false, true),
        // Before the wellness baseline exists only the absolute floor fires
        // the hold, so it names the reading, not a normal not yet known. The
        // reading may be yesterday's (still in force), so it names no day.
        AdjustmentKind::WellnessHold if ctx.wellness_collecting => {
            ("Your latest check-in is very low", true, true)
        }
        AdjustmentKind::WellnessHold => ("Your check-in is well below your normal", true, true),
        AdjustmentKind::HrvLower => (
            "HRV has dropped below your normal range and is trending down",
            true,
            false,
        ),
        _ => return None,
    })
}

/// The one call for one or more lift-holding readiness signals firing
/// together: they apply once (never stacked), so they read as one reason.
fn hold_summary(reasons: &[(&'static str, bool, bool)], ctx: &CallCtx) -> String {
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
    let first_set = if !ctx.first_set_logged && reasons.iter().any(|(.., f)| *f) {
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

/// The one-line call for a readiness/safety adjustment, naming WHICH signal
/// fired and what changes today, in plain language. Falls back
/// to [`describe`] for calls that need no signal-specific wording.
fn describe_rec(r: &AdjustmentCall, ctx: &CallCtx) -> String {
    if let Some(h) = lift_hold_reason(r, ctx) {
        return hold_summary(&[h], ctx);
    }
    match (&r.value, r.kind()) {
        (_, AdjustmentKind::WellnessLower) => {
            "Check-ins well below your normal for several days: today is one step easier (about 5% less weight, 1 rep earlier on every set, easy runs).".into()
        }
        (_, AdjustmentKind::VelocityLoss) => {
            "Bar speed dropped past your goal's limit within a set: stop 1 rep earlier and add no weight. Runs as planned.".into()
        }
        (_, AdjustmentKind::Decoupling) => {
            "Heart rate drifted up 10% or more on a steady run: easy runs only; lifts as planned.".into()
        }
        (_, AdjustmentKind::RhrSpike) => {
            "Your latest resting HR is well above your normal: an easier session today. If you feel ill, answer the illness question in your check-in.".into()
        }
        (Adjustment::DowngradeSession, AdjustmentKind::HeadCold) => {
            "Until you've recovered: no long or hard sessions and no heavy weights.".into()
        }
        (Adjustment::ReduceVolumePct(_), AdjustmentKind::HeadCold) => {
            "Head cold: short and easy only, or rest. Runs: 10–20 min easy. Lifts: half the sets, kept light.".into()
        }
        (_, AdjustmentKind::IllnessStop) => {
            "Flu or fever: no training until you've recovered. Your first session back is a short, easy test.".into()
        }
        (_, AdjustmentKind::PainStop) => {
            "Pain reported: training is paused. Rest the painful area and check it before you train again.".into()
        }
        (_, AdjustmentKind::ReturnHold) => {
            "First session back is easy (3+ reps left on every set, easy runs), then the plan resumes.".into()
        }
        (_, AdjustmentKind::TendonReduce) => {
            "Tendon pain is flaring: an easy session today with lighter weights.".into()
        }
        (Adjustment::ReturnCheck, _) => format!("{}. {RETURN_CHECK_STOP}", return_check_headline()),
        (_, AdjustmentKind::RhrRest) => {
            "Resting HR has been well above your normal 2 mornings in a row (or with illness symptoms): take a rest day and check whether you're getting ill.".into()
        }
        (Adjustment::StopItems { location }, _) => format!(
            "Pain ({location}): skip the exercises that load it today. The rest of the session goes ahead without adding weight."
        ),
        (Adjustment::ModifyAndMonitor { location }, AdjustmentKind::TendonModify(region)) => {
            let call = match region {
                Some(_) => format!(
                    "Tendon pain ({location}): keep the exercises that load it light and pain-free, and train the rest without adding weight. Avoid complete rest."
                ),
                None => describe(&r.value),
            };
            match eased_lifts(ctx.items, region) {
                Some(list) => format!("{call} Today: {list}."),
                None => call,
            }
        }
        (
            Adjustment::ForItem {
                exercise,
                adjustment,
            },
            kind,
        ) => match (adjustment.as_ref(), kind) {
            (Adjustment::ReduceLoadPct(p), AdjustmentKind::FirstSet) => format!(
                "{exercise}: your first set was harder than planned, so the remaining sets drop {p:.0}%."
            ),
            (Adjustment::IncreaseLoadPct(p), AdjustmentKind::FirstSet) => format!(
                "{exercise}: your first set felt easier than planned. You can add about {p:.0}% for the remaining sets."
            ),
            (_, AdjustmentKind::FailedSet) => format!(
                "{exercise}: your last set went to failure before the planned sets were done. End the exercise here, or drop the weight so the remaining sets end with reps left."
            ),
            (Adjustment::ReduceLoadPct(p), _) => format!(
                "{exercise}: today's top set is below your recent best, so the remaining sets drop {p:.0}%."
            ),
            (Adjustment::CapRpe(d), _) => format!(
                "{exercise}: stop {} earlier than planned on the remaining sets.",
                fmt_reps(i64::from(*d))
            ),
            (Adjustment::IncreaseLoadPct(p), _) => format!(
                "{exercise}: today's top set beat your recent best. You can add about {p:.0}% next time."
            ),
            (inner, _) => format!("{exercise}: {}", describe(inner)),
        },
        _ => describe(&r.value),
    }
}

/// The lifts in `items` a tendon modify call eases ("Back squat and
/// Deadlift"): those that load `region`, every lift when it maps to none.
fn eased_lifts(items: &[PlanItem], region: Option<crate::schema::PainRegion>) -> Option<String> {
    let names: Vec<&str> = items
        .iter()
        .filter(|it| it.is_lift())
        .filter(|it| region.is_none_or(|reg| !autoreg::item_spared_by_pain(reg, &it.exercise)))
        .map(|it| it.exercise.as_str())
        .collect();
    match names.as_slice() {
        [] => None,
        [one] => Some((*one).to_string()),
        [init @ .., last] => Some(format!("{} and {last}", init.join(", "))),
    }
}

/// Flatten any evidence-wrapped value into an adjustment row with an explicit
/// summary, for graded verdicts that are not `Adjustment`s (e.g. the
/// autoreg-032 threshold re-test cue).
pub(super) fn to_view_with<T, K>(summary: String, r: &Recommended<T, K>) -> AdjustmentView {
    AdjustmentView {
        summary,
        evidence: EvidenceTagView::of(r),
        // The basis is the claim statement; the improves/grade lines carry the
        // KB-grounded explanation.
        why: why_from(None, r),
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
        let (_, definition) = grade_words(g);
        GradeDefView {
            grade: g,
            definition: definition.to_string(),
            confidence: g.default_confidence(),
        }
    })
    .collect()
}

/// Plain-word label and definition of each grade (the File 09 meanings without
/// the research vocabulary): the legend shows the definition, the why-sheet
/// grade note both. The shells label the grade chip from their own strings.
fn grade_words(g: EvidenceGrade) -> (&'static str, &'static str) {
    match g {
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
    }
}

/// Compose the "why?" triad from a `Recommended<T>`, optionally overriding the
/// `basis` line with a datum-rich, call-site-specific sentence (e.g. the
/// HR-zones card restating the user's age through the Tanaka formula). When no
/// override is given, the basis is the backing claim's own registered
/// statement, always KB-grounded (HARD RULE 1).
pub(super) fn why_from<T, K>(basis_override: Option<String>, r: &Recommended<T, K>) -> WhyView {
    WhyView {
        basis: basis_override.unwrap_or_else(|| r.evidence().statement.to_owned()),
        grade_note: grade_note_str(r.grade(), r.contested()),
        improves: improves_for(r.claim_id()).map(str::to_owned),
    }
}

/// One-sentence gloss of what an evidence grade means for a claim, with the
/// contested question (`contested_ref`) appended when the claim is under
/// active debate.
pub(super) fn grade_note_str(
    grade: EvidenceGrade,
    contested_ref: Option<crate::evidence::ContestedId>,
) -> String {
    let (label, definition) = grade_words(grade);
    let first = char::from(definition.as_bytes()[0].to_ascii_lowercase());
    let rest = &definition[1..];
    let base = match grade {
        EvidenceGrade::MarketingMyth => return String::new(),
        EvidenceGrade::ExpertOpinion => format!("{label}: {first}{rest}"),
        _ => format!("{label} evidence: {first}{rest}"),
    };
    let Some(cq) = contested_ref.map(crate::evidence::ContestedId::question) else {
        return base;
    };
    // Honour the deck's "here's both sides" promise: name the open question
    // AND say what the app does meanwhile, rather than dangling a bare topic
    // label.
    let mut s = format!(
        "{base} Contested: experts disagree on {}. The plan's default: {}.",
        cq.question, cq.engine_default
    );
    // When the KB supplies an attributable opposing reference, show one
    // contradicting side. Left off entirely when the CQ has no KB-sourced
    // opposing cite (`other_side` is None) - never a fabricated "other view".
    if let Some(other) = cq.other_side {
        s.push_str(" Another view: ");
        s.push_str(other);
    }
    s
}

/// The engagement-loop line for the "why?" disclosure: which input the rule
/// lacks or would sharpen. Keyed by claim, this describes the ENGINE's own
/// data needs, never training advice, so it carries no evidence tag and
/// invents no claim (HARD RULE 1-safe by construction; every string is reviewed
/// against that bar). `None` when nothing would genuinely improve the estimate.
pub(super) fn improves_for(claim: ClaimId) -> Option<&'static str> {
    Some(match claim {
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
        _ => return None,
    })
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
            format!("You can add about {p:.0}% to the weight next session.")
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
                fmt_reps(i64::from(*d))
            )
        }
        Adjustment::DowngradeSession => "An easier session today".into(),
        // The region-scoped wording is the call's ([`describe_rec`]): only the
        // rule knows the region.
        Adjustment::ModifyAndMonitor { location } if location.is_empty() => {
            "Tendon pain: keep every exercise light and pain-free today. Avoid complete rest."
                .into()
        }
        Adjustment::ModifyAndMonitor { location } => format!(
            "Tendon pain ({location}): keep every exercise light and pain-free today. Avoid complete rest."
        ),
        Adjustment::RestDay => "Take a full rest day".into(),
        Adjustment::Stop => "Stop: do not train".into(),
        Adjustment::Defer { reason } => reason.clone(),
        Adjustment::ForItem {
            exercise,
            adjustment,
        } => format!("{exercise}: {}", describe(adjustment)),
        Adjustment::CapTopEnd { .. } => {
            "No weight added today: every lift stops 1 rep earlier and runs stay easy".into()
        }
        Adjustment::ReduceVolumePct(p) => format!("Cut today's volume about {p:.0}%"),
        Adjustment::StopItems { location } => {
            format!("Skip the exercises that load the painful area ({location})")
        }
        Adjustment::ReturnCheck => return_check_headline(),
    }
}
