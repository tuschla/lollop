//! Pain grading (File 08 Table 4.1; safety-038/039; autoreg-043): the one
//! classifier the safety gate and the readiness row share, the worst-active
//! report selection, and the body-region scoping of a sharp/joint stop.

use crate::evidence::{self, claims};
use crate::schema::{
    Adjustment, PainDetail, PainKind, PainTrend, ReadinessInput, ReadinessSignal, Recommended,
};

/// The graded pain verdict (File 08 Table 4.1; safety-038/039).
pub(super) enum PainGate {
    /// Stop-or-defer level: dominates every other output (structural pattern,
    /// uncharacterized pain, or a persistence escalation to DEFER).
    Block(Recommended<Adjustment>),
    /// Localized structural pain at a body part the engine can map to the
    /// movements that load it (safety-038 "STOP that exercise"): only those
    /// plan items stop; the rest of the day goes ahead with no load added.
    Scoped(Recommended<Adjustment>),
    /// Non-blocking response: tolerable tendon pain → modify & monitor
    /// ("avoid complete rest"); reactive tendon pain → reduce/downgrade.
    Adjust(Recommended<Adjustment>),
    /// DOMS / normal training discomfort → continue (Table 4.1 row 1).
    Continue,
    /// No pain reported.
    None,
}

/// The body region a reported pain location maps to, for scoping a
/// sharp/joint stop to the movements that load it (safety-038
/// "STOP that exercise", autoreg-043 "stop or substitute the movement").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PainRegion {
    /// Hip, thigh, knee, shin, calf, ankle, Achilles, foot.
    LowerLimb,
    /// Neck, shoulder, arm, elbow, forearm, wrist, hand, fingers.
    UpperLimb,
    /// Lower / upper back, spine.
    Back,
}

/// Map a free-text pain location onto a [`PainRegion`], conservatively: only
/// clear body-part words map; anything else (chest, head, stomach, groin,
/// unknown) returns `None` and keeps the whole-day stop. Chest pain in
/// particular must never be scoped (a possible cardiac red flag).
pub fn pain_region(location: &str) -> Option<PainRegion> {
    let l = location.to_ascii_lowercase();
    let has = |words: &[&str]| {
        l.split(|c: char| !c.is_ascii_alphabetic()).any(|t| {
            words
                .iter()
                .any(|w| t == *w || t.strip_suffix('s') == Some(*w))
        })
    };
    if has(&[
        "chest", "head", "stomach", "abdomen", "belly", "groin", "rib", "jaw",
    ]) {
        return None;
    }
    let lower = has(&[
        "hip",
        "thigh",
        "knee",
        "kneecap",
        "patella",
        "shin",
        "calf",
        "calve",
        "ankle",
        "achilles",
        "foot",
        "feet",
        "heel",
        "toe",
        "hamstring",
        "quad",
        "quadricep",
        "glute",
        "leg",
    ]);
    let upper = has(&[
        "neck",
        "shoulder",
        "arm",
        "elbow",
        "forearm",
        "wrist",
        "hand",
        "finger",
        "thumb",
        "bicep",
        "tricep",
        "trap",
        "collarbone",
    ]);
    // "Back" alone is the spine; "back of the knee" is the knee.
    let back =
        !lower && !upper && has(&["back", "spine", "lumbar", "spinal", "sacrum", "tailbone"]);
    // A location naming more than one region (e.g. "knee and shoulder") is not
    // one movement family: keep the whole-day stop.
    match (lower, upper, back) {
        (true, false, false) => Some(PainRegion::LowerLimb),
        (false, true, false) => Some(PainRegion::UpperLimb),
        (false, false, true) => Some(PainRegion::Back),
        _ => None,
    }
}

/// Whether a plan item can go ahead while the given region hurts. `exercise`
/// is the item's lift name; an empty name is a run. Conservative allow-lists:
/// an item is kept only when its name matches a movement that cannot load the
/// painful region (no bar on the back, nothing held in the hands, no overhead
/// or rack position, no impact); everything else, including any unrecognized
/// lift, stops. Stopping one extra exercise costs little, loading an injured
/// part does not (safety-038 "STOP that exercise"; autoreg-043; HARD RULE 3).
/// Runs load the legs and, through impact, the spine: they stay only for arm,
/// shoulder or neck pain.
pub fn item_spared_by_pain(region: PainRegion, exercise: &str) -> bool {
    let n = exercise.trim().to_ascii_lowercase();
    if n.is_empty() {
        return region == PainRegion::UpperLimb;
    }
    let has = |kws: &[&str]| kws.iter().any(|k| n.contains(*k));
    match region {
        // Leg work with nothing in the hands and no pad or bar on the shoulders.
        PainRegion::UpperLimb => {
            let calf_raise = has(&["calf raise"])
                && (has(&["seated"]) || !has(&["standing", "smith", "machine"]));
            (calf_raise
                || has(&[
                    "leg press",
                    "leg extension",
                    "leg curl",
                    "hamstring curl",
                    "bodyweight squat",
                    "air squat",
                    "pistol",
                ]))
                && !has(&[
                    "barbell",
                    "dumbbell",
                    "kettlebell",
                    "weighted",
                    "goblet",
                    "loaded",
                    "overhead",
                    "front",
                    "hack",
                    "nordic",
                ])
        }
        // Seated, supported or hanging upper-body work with the legs unloaded
        // (a seated cable row only braces the feet on its platform).
        PainRegion::LowerLimb => {
            (has(&[
                "cable row",
                // The legacy catalog label of the same Cable row.
                "row (barbell/cable/machine)",
                "seated row",
                "machine row",
                "bench",
                "lat pulldown",
                "pulldown",
                "pull-up",
                "pullup",
                "pull up",
                "chin-up",
                "chinup",
                "chin up",
                "chest-supported row",
                "chest supported row",
                "machine chest press",
                "chest press machine",
                "curl",
                "tricep",
            ]) || (has(&["seated"]) && has(&["press"])))
                && !has(&[
                    "leg",
                    "squat",
                    "lunge",
                    "split",
                    "step",
                    "jump",
                    "box",
                    "hop",
                    "bound",
                    "calf",
                    "hip",
                    "glute",
                    "deadlift",
                    "rdl",
                    "good morning",
                    "clean",
                    "snatch",
                    "jerk",
                    "thruster",
                    "burpee",
                    "sprint",
                    "sled",
                    "carry",
                    "swing",
                    "standing",
                    "bent",
                    "nordic",
                    "hamstring",
                ])
        }
        // Supported, non-axial, non-impact machine work only.
        PainRegion::Back => {
            has(&[
                "lat pulldown",
                "pulldown",
                "chest-supported row",
                "chest supported row",
                "machine chest press",
                "chest press machine",
                "leg extension",
                "leg curl",
                "hamstring curl",
            ]) && !has(&["standing", "barbell", "good morning", "nordic"])
        }
    }
}

/// Top of the tolerable tendon-pain band, /10 (safety-039, SAFE-TENDON-001):
/// the band is ≤3–5/10 AND stable; the operational threshold is the stated ≤5.
const TENDON_TOLERABLE_MAX: u8 = 5;

/// Tendon reactive band (safety-039): above [`TENDON_TOLERABLE_MAX`],
/// worsening after, or rising week-to-week. An unrated report is not known to
/// sit inside the tolerable band, so it is graded reactive (HARD RULE 3).
pub(super) fn tendon_reactive(d: &PainDetail) -> bool {
    d.severity.is_none_or(|s| s > TENDON_TOLERABLE_MAX)
        || matches!(d.trend, PainTrend::Rising | PainTrend::WorseningAfter24h)
}

/// The reported body-part location, trimmed, if present and non-empty. Never
/// fabricated, `None`/blank stays `None` (display-only context, HARD RULE 1).
fn pain_location(d: &PainDetail) -> Option<&str> {
    d.location
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Human display string for a characterized pain report, the safety-banner
/// sub-line context, e.g. `"Left knee · sharp/joint · 6/10"` or, with no
/// location reported, `"tendon-like · 3/10"`. Location is included only when the
/// user supplied one. Empty for a bare (detail-less) report; the caller renders
/// that generic case itself.
pub(super) fn pain_context(d: &PainDetail) -> String {
    let kind = match d.kind {
        PainKind::SharpJoint => "sharp/joint",
        PainKind::TendonLoadRelated => "tendon-like",
        PainKind::Doms => "muscle soreness",
        PainKind::Other => "not sure",
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(loc) = pain_location(d) {
        parts.push(loc.to_string());
    }
    parts.push(kind.to_string());
    if let Some(s) = d.severity {
        parts.push(format!("{s}/10"));
    }
    if matches!(d.trend, PainTrend::Rising | PainTrend::WorseningAfter24h) {
        parts.push("worsening".to_string());
    }
    parts.join(" · ")
}

/// Append the reported body-part location to a Defer reason so the headline can
/// name it, e.g. `"… see a doctor or physiotherapist (Left knee)."`. The
/// base message is preserved verbatim when no location was reported.
fn with_location(reason: String, d: &PainDetail) -> String {
    match pain_location(d) {
        Some(loc) => {
            let base = reason.strip_suffix('.').unwrap_or(&reason);
            format!("{base} ({loc}).")
        }
        None => reason,
    }
}

/// The File 08 Table 4.1 classification of ONE active pain report. The single
/// decision tree both the gate ([`grade_pain`]) and the readiness label read,
/// so the strip can never describe a different response than the rule applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PainVerdict {
    /// Bare report with no detail: conservative hard stop (autoreg-043).
    BareStop,
    /// Sharp/joint-line pain persisting: defer (safety-038).
    StructuralDefer,
    /// Sharp/joint-line pain at a mappable body part: stop the movements that
    /// load it (safety-038 "STOP that exercise").
    StructuralScoped,
    /// Sharp/joint-line pain with no mappable location: whole-day stop.
    StructuralStop,
    /// Reactive tendon pain persisting despite reduced load: defer.
    TendonDefer,
    /// Reactive tendon pain: reduce load (an easier session).
    TendonReduce,
    /// Tolerable, stable tendon pain: modify and monitor.
    TendonModify,
    /// DOMS / normal training discomfort: continue.
    Doms,
    /// Uncharacterized pain persisting: defer.
    OtherDefer,
    /// Uncharacterized pain: hard stop.
    OtherStop,
}

/// Classify one pain report (`value > 0`) against Table 4.1.
pub(super) fn classify_pain(input: &ReadinessInput) -> PainVerdict {
    let Some(detail) = &input.pain else {
        return PainVerdict::BareStop;
    };
    match detail.kind {
        PainKind::SharpJoint if detail.persists => PainVerdict::StructuralDefer,
        PainKind::SharpJoint if pain_location(detail).is_some_and(|l| pain_region(l).is_some()) => {
            PainVerdict::StructuralScoped
        }
        PainKind::SharpJoint => PainVerdict::StructuralStop,
        PainKind::TendonLoadRelated if tendon_reactive(detail) && detail.persists => {
            PainVerdict::TendonDefer
        }
        PainKind::TendonLoadRelated if tendon_reactive(detail) => PainVerdict::TendonReduce,
        PainKind::TendonLoadRelated => PainVerdict::TendonModify,
        PainKind::Doms => PainVerdict::Doms,
        PainKind::Other if detail.persists => PainVerdict::OtherDefer,
        PainKind::Other => PainVerdict::OtherStop,
    }
}

/// Grade ONE reported pain input against File 08 Table 4.1.
///
/// A bare `Pain` report (`value > 0`, no detail) is a conservative hard stop
/// (autoreg-043 / File 06 §6C: never ramp load into pain), as is
/// `PainKind::Other`.
///
/// Deliberate deferral: Table 4.1's "MODIFY if DOMS severe" clause carries no
/// numeric severity threshold in the KB (the ≤3–5/10 band is tendon-specific),
/// so severe DOMS is NOT auto-modified here (HARD RULE 1, no invented
/// thresholds); the wellness soreness item (autoreg-030, ≥6/7 → downgrade)
/// covers high soreness via `ReadinessSignal::Soreness` instead.
fn grade_pain(input: &ReadinessInput) -> PainGate {
    let defer = |reason: &str, claim: evidence::ClaimId| {
        let reason = match &input.pain {
            Some(d) => with_location(reason.into(), d),
            None => reason.into(),
        };
        PainGate::Block(evidence::graded(Adjustment::Defer { reason }, claim))
    };
    match classify_pain(input) {
        PainVerdict::BareStop | PainVerdict::OtherStop => {
            PainGate::Block(evidence::graded(Adjustment::Stop, claims::SAFE_PAIN_001))
        }
        PainVerdict::StructuralDefer => defer(
            "Sharp or joint pain that isn't going away. Stop that exercise and see a doctor or physiotherapist.",
            claims::SAFE_PAIN_STRUCT_001,
        ),
        PainVerdict::StructuralScoped => PainGate::Scoped(evidence::graded(
            Adjustment::StopItems {
                location: input
                    .pain
                    .as_ref()
                    .and_then(pain_location)
                    .unwrap_or_default()
                    .to_string(),
            },
            claims::SAFE_PAIN_STRUCT_001,
        )),
        PainVerdict::StructuralStop => PainGate::Block(evidence::graded(
            Adjustment::Stop,
            claims::SAFE_PAIN_STRUCT_001,
        )),
        PainVerdict::TendonDefer => defer(
            "Tendon pain that keeps flaring, even after you've eased off. See a doctor or physiotherapist.",
            claims::SAFE_TENDON_001,
        ),
        // REDUCE load & compressive positions: an easier session, not a stop
        // (the KB states no reduction percentage).
        PainVerdict::TendonReduce => PainGate::Adjust(evidence::graded(
            Adjustment::DowngradeSession,
            claims::SAFE_TENDON_001,
        )),
        // Tolerable band: modify/continue with monitoring; avoid complete rest.
        PainVerdict::TendonModify => PainGate::Adjust(evidence::graded(
            Adjustment::ModifyAndMonitor {
                location: input
                    .pain
                    .as_ref()
                    .and_then(pain_location)
                    .unwrap_or_default()
                    .to_string(),
            },
            claims::SAFE_TENDON_001,
        )),
        PainVerdict::Doms => PainGate::Continue,
        PainVerdict::OtherDefer => defer(
            "Pain that isn't going away. Stop and have a doctor or physiotherapist check it.",
            claims::SAFE_PAIN_001,
        ),
    }
}

/// Severity rank of a graded verdict for choosing the WORST active pain report.
/// Block (stop/defer) dominates Adjust (modify/monitor), which dominates
/// Continue (DOMS); None is the absence of any active report.
fn pain_gate_rank(gate: &PainGate) -> u8 {
    match gate {
        PainGate::Block(_) => 4,
        PainGate::Scoped(_) => 3,
        PainGate::Adjust(_) => 2,
        PainGate::Continue => 1,
        PainGate::None => 0,
    }
}

/// The single Pain report that decides the verdict: the WORST currently-active
/// report, not merely the newest.
///
/// A pain-clear (a `Pain` reading with `value <= 0`) supersedes every earlier
/// report; only reports logged AFTER the most recent clear are active. Among
/// those, the highest-severity grade wins (Block > Scoped > Adjust > Continue)
/// so a later, milder pain at a different site can never displace an unresolved
/// red-flag stop: latest-wins would let a tolerable ache, or a DOMS report,
/// silently drop a sharp-joint hold. Ties within a grade keep latest-wins (the
/// most recent report of that grade). `None` when no active report remains.
///
/// One selection point for the gate, the readiness row/banner, and the undo, so
/// they can never name different body parts (the row shows THIS report's detail,
/// and `RemoveReadiness(Pain)` removes THIS report to lift the hold).
pub fn worst_active_pain(inputs: &[ReadinessInput]) -> Option<&ReadinessInput> {
    let mut worst: Option<&ReadinessInput> = None;
    let mut worst_rank = 0u8;
    for input in active_pain_reports(inputs) {
        let rank = pain_gate_rank(&grade_pain(input));
        let wins = match worst {
            None => true,
            Some(w) => {
                rank > worst_rank || (rank == worst_rank && input.observed_at >= w.observed_at)
            }
        };
        if wins {
            worst = Some(input);
            worst_rank = rank;
        }
    }
    worst
}

/// Pain reports still active: positive `Pain` readings logged after the most
/// recent pain-clear (a `Pain` reading with `value <= 0`), in input order.
fn active_pain_reports(inputs: &[ReadinessInput]) -> impl Iterator<Item = &ReadinessInput> {
    let cleared_at = inputs
        .iter()
        .filter(|i| i.signal == ReadinessSignal::Pain && i.value <= 0.0)
        .map(|i| i.observed_at)
        .max();
    inputs
        .iter()
        .filter(|i| i.signal == ReadinessSignal::Pain && i.value > 0.0)
        .filter(move |i| cleared_at.is_none_or(|c| i.observed_at > c))
}

/// Resolve the pain verdict over the [`worst_active_pain`] report.
pub(super) fn pain_gate(inputs: &[ReadinessInput]) -> PainGate {
    worst_active_pain(inputs).map_or(PainGate::None, grade_pain)
}

/// Whether the active Pain reports gate *difficulty progression* — i.e. any
/// graded pain verdict other than "continue". Shares `pain_gate`'s semantics
/// exactly (worst active Pain report since the last pain-clear, kind-graded per
/// File 08 Table 4.1): a lone `Doms` report is normal training discomfort
/// (Table 4.1 row 1, "continue") and does NOT gate progression, and a pain-clear
/// reading (`value 0`) re-opens it. The calisthenics anchor builder uses this to
/// freeze `advance_ready` without duplicating the Table 4.1 mapping.
pub fn pain_blocks_progression(inputs: &[ReadinessInput]) -> bool {
    matches!(
        pain_gate(inputs),
        PainGate::Block(_) | PainGate::Scoped(_) | PainGate::Adjust(_)
    )
}

/// Whether an active pain report stops training (the whole day, or the
/// movements that load the painful part): the hold an "I'm pain-free now"
/// action resolves into a graded return (ready-upd-004).
pub fn pain_hold_active(inputs: &[ReadinessInput]) -> bool {
    matches!(pain_gate(inputs), PainGate::Block(_) | PainGate::Scoped(_))
}

/// Every active pain response that does not block the whole day, deduplicated:
/// each active scoped sharp/joint stop (two sore joints both stop their own
/// movements; the worst-report selection alone would drop one), plus the
/// strongest tendon modify/reduce response. Only consulted when no active report
/// blocks (a Block dominates and returns alone).
pub(super) fn active_pain_responses(inputs: &[ReadinessInput]) -> Vec<Recommended<Adjustment>> {
    let mut active: Vec<&ReadinessInput> = active_pain_reports(inputs).collect();
    active.sort_by_key(|i| i.observed_at);
    let mut out: Vec<Recommended<Adjustment>> = Vec::new();
    let mut adjust: Option<Recommended<Adjustment>> = None;
    for i in active.iter().rev() {
        match grade_pain(i) {
            PainGate::Scoped(r) => {
                if !out.iter().any(|o| o.value == r.value) {
                    out.push(r);
                }
            }
            PainGate::Adjust(r) => {
                // Reactive (downgrade) outranks tolerable (modify); newest wins a tie.
                let stronger = |a: &Adjustment| matches!(a, Adjustment::DowngradeSession);
                if adjust
                    .as_ref()
                    .is_none_or(|cur| stronger(&r.value) && !stronger(&cur.value))
                {
                    adjust = Some(r);
                }
            }
            _ => {}
        }
    }
    out.reverse();
    out.extend(adjust);
    out
}
