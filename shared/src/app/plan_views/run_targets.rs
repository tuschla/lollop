//! Numeric execution targets for run prescriptions: every planned run
//! reads as something a runner can follow: a pace range from the athlete's
//! current race fitness (running-007 VDOT bands, running-041 freshness), a
//! heart-rate range when HRmax is known (measured, else the Tanaka estimate),
//! and an effort cue (the session's RPE band; the talk test when nothing is
//! measured, RUN-TALKTEST-001). The plan prescribes the BAND; the numbers are
//! resolved here at render time from the anchors, so no prescription invents
//! a pace without a race to derive it from (HARD RULE 1).

use std::fmt::Write as _;

use crate::app::format::{capitalize_first, fmt_mss, fmt_secs_min};
use crate::app::model::{Model, profile_for_day};
use crate::app::plan_views::plan_item::{HrAnchor, PlanItem, TargetAnchor};
use crate::app::view_types::{EvidenceTagView, RunTargetView};
use crate::evidence::claims;
use crate::profile::{DistanceUnit, Profile};
use crate::schema::{
    Prescription, Recommended, RunIntensity, RunSessionType, Session, SessionType, ThreeZone,
    VdotBand, WorkoutType,
};
use crate::{load, running};

/// The training band a run prescription is executed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum TargetBand {
    Recovery,
    Easy,
    LongRun,
    Marathon,
    Threshold,
    Interval,
    Repetition,
    Strides,
    Hills,
}

impl TargetBand {
    /// How the band feels, in plain words, for the human pace/intensity line
    /// ("8:29–8:53 /mi (comfortably hard)").
    fn plain(self) -> &'static str {
        match self {
            TargetBand::Recovery => "very easy",
            TargetBand::Easy | TargetBand::LongRun => "easy",
            TargetBand::Marathon => "marathon pace",
            TargetBand::Threshold => "comfortably hard",
            TargetBand::Interval => "hard",
            TargetBand::Repetition => "fast",
            TargetBand::Strides => "quick and relaxed",
            TargetBand::Hills => "hard uphill",
        }
    }

    /// The Daniels VDOT band whose pace this band runs at (running-007).
    /// Recovery is MP-relative (running-014) and hills are effort-only.
    fn vdot_band(self) -> Option<VdotBand> {
        match self {
            TargetBand::Easy | TargetBand::LongRun => Some(VdotBand::Easy),
            TargetBand::Marathon => Some(VdotBand::Marathon),
            TargetBand::Threshold => Some(VdotBand::Threshold),
            TargetBand::Interval => Some(VdotBand::Interval),
            TargetBand::Repetition | TargetBand::Strides => Some(VdotBand::Repetition),
            TargetBand::Recovery | TargetBand::Hills => None,
        }
    }

    /// The KB workout row carrying this band's RPE (running-014…022).
    fn workout(self) -> RunSessionType {
        match self {
            TargetBand::Recovery => RunSessionType::Recovery,
            TargetBand::Easy => RunSessionType::Easy,
            TargetBand::LongRun => RunSessionType::LongRun,
            TargetBand::Marathon => RunSessionType::RacePace,
            TargetBand::Threshold => RunSessionType::Tempo,
            TargetBand::Interval => RunSessionType::Interval,
            TargetBand::Repetition => RunSessionType::Repetition,
            TargetBand::Strides => RunSessionType::Strides,
            TargetBand::Hills => RunSessionType::Hills,
        }
    }

    /// The run type a run in this band is logged as, for a session whose own
    /// type names none ([`RunSessionType::workout_type`]).
    fn workout_type(self) -> WorkoutType {
        match self {
            TargetBand::Recovery => WorkoutType::Recovery,
            TargetBand::Easy => WorkoutType::Easy,
            TargetBand::LongRun => WorkoutType::LongRun,
            TargetBand::Threshold => WorkoutType::Tempo,
            TargetBand::Interval | TargetBand::Repetition | TargetBand::Hills => {
                WorkoutType::Interval
            }
            TargetBand::Marathon | TargetBand::Strides => WorkoutType::Steady,
        }
    }

    /// Easy-family bands get the talk-test cue when nothing is measured.
    fn is_easy(self) -> bool {
        matches!(
            self,
            TargetBand::Recovery | TargetBand::Easy | TargetBand::LongRun
        )
    }
}

/// The run type a run item preselects: its session's own type when that names
/// one, else the band the item runs in (`None` without either).
fn item_workout_type(
    kind: Option<RunSessionType>,
    band: Option<TargetBand>,
) -> Option<WorkoutType> {
    kind.and_then(RunSessionType::workout_type)
        .or(band.map(TargetBand::workout_type))
}

/// Which band a run prescription runs in. The session kind settles the
/// effort-only/stride cases; otherwise the prescribed intensity does: a VDOT
/// band directly, a %HRmax band through the running-007 table (E ≤79, M 80–85,
/// T 88–92, I ≥97 %HRmax), a ceiling-only band (no floor) being Recovery
/// (running-014 states only ceilings).
pub(in crate::app) fn band_for(
    kind: Option<RunSessionType>,
    intensity: &RunIntensity,
) -> Option<TargetBand> {
    if kind == Some(RunSessionType::Strides) {
        return Some(TargetBand::Strides);
    }
    if kind == Some(RunSessionType::Hills) {
        return Some(TargetBand::Hills);
    }
    let long = kind == Some(RunSessionType::LongRun);
    let easy = |b: TargetBand| if long { TargetBand::LongRun } else { b };
    match *intensity {
        RunIntensity::Vdot(VdotBand::Easy) | RunIntensity::ThreeZone(ThreeZone::Z1) => {
            Some(easy(TargetBand::Easy))
        }
        RunIntensity::Vdot(VdotBand::Marathon) => Some(TargetBand::Marathon),
        RunIntensity::Vdot(VdotBand::Threshold) => Some(TargetBand::Threshold),
        RunIntensity::Vdot(VdotBand::Interval) => Some(TargetBand::Interval),
        RunIntensity::Vdot(VdotBand::Repetition) => Some(TargetBand::Repetition),
        RunIntensity::HrPercentMax {
            lo: None,
            hi: Some(_),
            ..
        } => Some(easy(TargetBand::Recovery)),
        RunIntensity::HrPercentMax { lo, hi, .. } => {
            let (lo, hi) = (lo.map(f64::from), hi.map(f64::from));
            let mid = match (lo, hi) {
                (Some(l), Some(h)) => (l + h) / 2.0,
                (Some(l), None) => l,
                (None, Some(h)) => h,
                (None, None) => return None,
            };
            Some(if mid < 80.0 {
                easy(TargetBand::Easy)
            } else if mid < 86.0 {
                TargetBand::Marathon
            } else if mid < 95.0 {
                TargetBand::Threshold
            } else {
                TargetBand::Interval
            })
        }
        RunIntensity::ThreeZone(_) => None,
    }
}

/// Recovery pace floor: running-014 "≥20 % slower than marathon pace". Read
/// conservatively so BOTH interpretations hold (pace ≥1.20× MP pace and speed
/// ≤0.80× MP speed): the floor is MP pace × 1.25. MP pace is the athlete's
/// Daniels marathon prediction ÷ the marathon distance in km.
fn recovery_pace_floor(vdot: f64) -> Option<f64> {
    let mp_sec = load::daniels_predict(vdot, running::MARATHON_M);
    (mp_sec > 0.0).then(|| mp_sec / (running::MARATHON_M / running::KM_M) * 1.25)
}

/// Pace (fast, slow) seconds per km for a band; `fast == 0` marks a
/// "slower than" floor (Recovery), `slow == 0` an "at least this fast" bound
/// (Repetition / strides). The long run is E→E+, 10–20 % slower than marathon
/// pace (running-016); easy is 15–25 % slower (running-015, via the E band).
fn band_pace(band: TargetBand, vdot: f64) -> Option<(f64, f64)> {
    match band {
        TargetBand::Recovery => recovery_pace_floor(vdot).map(|p| (0.0, p)),
        TargetBand::LongRun => running::mp_relative_pace_sec_per_km(vdot, RunSessionType::LongRun),
        TargetBand::Repetition | TargetBand::Strides => {
            running::vdot_band_pace_sec_per_km(vdot, VdotBand::Repetition).map(|(f, _)| (f, 0.0))
        }
        b => b
            .vdot_band()
            .and_then(|vb| running::vdot_band_pace_sec_per_km(vdot, vb)),
    }
}

/// A pace range in `unit` from its per-km bounds (seconds): "4:50–5:05 /km",
/// "slower than 8:11 /mi" (`fast == 0`), "4:30 /km or faster" (`slow == 0`).
fn pace_label(fast: f64, slow: f64, unit: DistanceUnit) -> String {
    let (suffix, per_km_factor) = match unit {
        DistanceUnit::Km => ("km", 1.0),
        DistanceUnit::Mi => ("mi", running::MILE_M / 1000.0),
    };
    let (f, s) = (fast * per_km_factor, slow * per_km_factor);
    if fast <= 0.0 {
        format!("slower than {} /{suffix}", fmt_mss(s))
    } else if slow <= 0.0 {
        format!("{} /{suffix} or faster", fmt_mss(f))
    } else {
        format!("{}–{} /{suffix}", fmt_mss(f), fmt_mss(s))
    }
}

/// The target's pace in km for the item's own line (`summary`,
/// `intensity_label`, which state km in every unit), from the whole seconds
/// the target carries; `None` without a pace.
fn km_pace_label(t: &RunTargetView) -> Option<String> {
    (!t.pace_label.is_empty()).then(|| {
        pace_label(
            f64::from(t.pace_fast_sec_per_km),
            f64::from(t.pace_slow_sec_per_km),
            DistanceUnit::Km,
        )
    })
}

/// %HRmax band (percent) for a band, from the prescription's own %HRmax band
/// when it carries one, else the running-007 table for HR-valid bands (E/M/T).
/// Interval / repetition work is pace-governed: no bpm (running-002).
fn band_hr_pct(
    band: TargetBand,
    intensity: &RunIntensity,
) -> Option<(Option<f64>, Option<f64>, Option<f64>)> {
    if let RunIntensity::HrPercentMax { lo, hi, hrr_hi } = *intensity {
        return Some((lo.map(f64::from), hi.map(f64::from), hrr_hi.map(f64::from)));
    }
    let vb = band.vdot_band()?;
    if !running::vdot_band_uses_hr(vb) {
        return None;
    }
    let (lo, hi) = running::vdot_band_hr_pct(vb);
    Some((Some(lo), Some(hi), None))
}

fn hr_anchor(profile: Option<&Profile>) -> HrAnchor {
    let Some(p) = profile else {
        return HrAnchor::default();
    };
    let (max_bpm, measured) = p.hr_max_anchor().unwrap_or((0.0, false));
    HrAnchor {
        max_bpm,
        measured,
        rest_bpm: p.resting_hr_anchor().unwrap_or(0.0),
    }
}

/// Resolve the numeric target for one band. `vdot` provides the paces; `hr`
/// the bpm; the effort cue is always present. The pace label reads in `unit`.
pub(in crate::app) fn resolve_target(
    band: TargetBand,
    intensity: &RunIntensity,
    vdot: Option<f64>,
    hr: HrAnchor,
    unit: DistanceUnit,
) -> RunTargetView {
    let mut t = RunTargetView::default();
    let vdot = vdot.filter(|v| v.is_finite() && *v > 0.0);
    let mut evidence: Option<Recommended<()>> = None;

    // Pace (running-007 VDOT bands off the CURRENT race fitness).
    if let Some(v) = vdot
        && let Some((fast, slow)) = band_pace(band, v)
    {
        t.pace_fast_sec_per_km = fast.round() as u32;
        t.pace_slow_sec_per_km = slow.round() as u32;
        t.pace_label = pace_label(fast, slow, unit);
        evidence = Some(Recommended::new((), claims::RUN_VDOT_001));
    }

    // Heart rate: %HRmax → bpm, the Karvonen %HRR ceiling when resting HR is
    // known (the stricter ceiling governs).
    if hr.max_bpm > 0.0
        && let Some((lo, hi, hrr_hi)) = band_hr_pct(band, intensity)
    {
        let to_bpm = |pct: f64| (pct / 100.0 * hr.max_bpm).round();
        let lo_bpm = lo.map(to_bpm);
        let mut hi_bpm = hi.map(to_bpm);
        if let Some(hrr) = hrr_hi
            && hr.rest_bpm > 0.0
        {
            let k = load::karvonen_target_hr(hr.rest_bpm, hr.max_bpm, hrr / 100.0).round();
            hi_bpm = Some(hi_bpm.map_or(k, |h| h.min(k)));
        }
        t.hr_label = match (lo_bpm, hi_bpm) {
            (Some(l), Some(h)) if h > l => format!("{l:.0}–{h:.0} bpm"),
            (_, Some(h)) => format!("≤ {h:.0} bpm"),
            (Some(l), None) => format!("≥ {l:.0} bpm"),
            (None, None) => String::new(),
        };
        if !t.hr_label.is_empty() && evidence.is_none() {
            evidence = Some(Recommended::new((), claims::RUN_HRMAX_001));
        }
    }

    // Effort: the session's KB RPE band, always (the R row states none); the
    // talk test on easy runs when neither a pace nor a measured HRmax anchors
    // them (RUN-TALKTEST-001).
    let rpe = running::workout_rx(band.workout()).value.rpe;
    let mut detail = match rpe {
        Some((lo, hi)) if lo == hi => format!("{lo} of 10"),
        Some((lo, hi)) => format!("{lo}–{hi} of 10"),
        None => String::new(),
    };
    let unanchored = t.pace_label.is_empty() && !hr.measured;
    if band.is_easy() && unanchored {
        detail = if detail.is_empty() {
            "you can talk in full sentences".to_string()
        } else {
            format!("{detail}, easy enough to talk in full sentences")
        };
        if evidence.is_none() {
            evidence = Some(Recommended::new((), claims::RUN_TALKTEST_001));
        }
    }
    t.effort_cue = if rpe.is_some() {
        format!("effort {detail}")
    } else {
        detail.clone()
    };
    t.effort_detail = detail;
    if let Some(g) = evidence {
        t.evidence = EvidenceTagView::of(&g);
    }
    t
}

/// The intensity line a run item shows: the pace (with how the band feels)
/// when known, otherwise the original band label with its bpm, then the
/// effort cue.
fn target_label(base: &str, t: &RunTargetView, feel: &str) -> String {
    let mut label = if let Some(pace) = km_pace_label(t) {
        let mut label = format!("{pace} ({feel})");
        if !t.hr_label.is_empty() {
            let _ = write!(label, " · {}", t.hr_label);
        }
        label
    } else if !t.hr_label.is_empty() {
        format!("{base} ({})", t.hr_label)
    } else {
        base.to_string()
    };
    if !t.effort_cue.is_empty() {
        let _ = write!(label, " · {}", t.effort_cue);
    }
    label
}

/// A VDOT label like "Threshold pace" names a pace the athlete has no number
/// for; without an anchor it reads as how the band feels ("Comfortably
/// hard"), and the effort cue that follows says how hard in numbers.
fn base_label(original: &str, band: TargetBand, intensity: &RunIntensity) -> String {
    match intensity {
        RunIntensity::Vdot(_) => capitalize_first(band.plain()),
        _ => original.to_string(),
    }
}

/// Structure text around the main set: between-rep recovery, warm-up /
/// cool-down and a strides add-on, from the plan's numeric
/// [`RunStructureView`] (zero = absent). The volume stays the summary's first
/// `·` segment (the downgrade path reads it), so every structure note goes
/// after the target.
pub(super) fn structure_suffix(it: &PlanItem) -> String {
    let Some(s) = it.run_structure.as_ref() else {
        return String::new();
    };
    let mut out = String::new();
    if it.rep_count > 0 {
        if s.recovery_sec > 0 {
            let _ = write!(
                out,
                " · {} easy jog between reps",
                fmt_secs_min(s.recovery_sec, 1)
            );
        } else if s.recovery_m > 0 {
            let _ = write!(out, " · {} m easy jog between reps", s.recovery_m);
        } else if s.recovery_equal_to_rep {
            out.push_str(" · jog easy between reps for as long as each rep took");
        }
    }
    let _ = match (s.warmup_min, s.cooldown_min) {
        (0, 0) => Ok(()),
        (w, 0) => write!(out, " · warm up {w} min easy first"),
        (0, c) => write!(out, " · cool down {c} min easy after"),
        (w, c) => write!(
            out,
            " · warm up {w} min easy first, cool down {c} min easy after"
        ),
    };
    if s.strides_reps > 0 && s.strides_sec > 0 {
        let _ = write!(
            out,
            " · then {} × {} s relaxed strides",
            s.strides_reps, s.strides_sec
        );
        if s.strides_recovery_sec > 0 {
            let _ = write!(
                out,
                ", {} easy between",
                fmt_secs_min(s.strides_recovery_sec, 1)
            );
        }
    }
    out
}

/// Rewrite one run item's intensity line + summary from a resolved target.
fn apply_target(it: &mut PlanItem, t: RunTargetView, base: &str, feel: &str) {
    let new_label = target_label(base, &t, feel);
    let head = if it.run_head.is_empty() {
        String::new()
    } else {
        format!("{} · ", it.run_head)
    };
    let suffix = structure_suffix(it);
    it.summary = format!("{head}{new_label}{suffix}");
    it.intensity_label = new_label;
    it.run_target = Some(t);
}

/// Fill every run item of one flattened session with its session's run type
/// and its numeric target (`items` are aligned with `s.prescriptions`; lifts
/// are untouched).
pub(super) fn apply_run_targets(
    items: &mut [PlanItem],
    s: &Session,
    anchors: &crate::plan::Anchors,
    model: &Model,
    epoch_day: i64,
) {
    let kind = match s.session_type {
        SessionType::Run(k) => Some(k),
        _ => None,
    };
    let day_profile = profile_for_day(model, epoch_day);
    let hr = hr_anchor(day_profile.as_deref().or(model.profile()));
    for (rx, it) in s.prescriptions.iter().zip(items.iter_mut()) {
        let Prescription::Run(r) = &rx.value else {
            continue;
        };
        let band = band_for(kind, &r.intensity);
        it.workout_type = item_workout_type(kind, band);
        let Some(band) = band else {
            continue;
        };
        let t = resolve_target(
            band,
            &r.intensity,
            anchors.run_vdot,
            hr,
            model.distance_unit(),
        );
        let base = base_label(&it.intensity_label, band, &r.intensity);
        apply_target(it, t, &base, band.plain());
        it.target_anchor = Some(TargetAnchor {
            kind,
            ..TargetAnchor::new(anchors.run_vdot, hr, model.distance_unit())
        });
    }
}

/// Re-resolve a run item's target for the easy band after a readiness /
/// safety downgrade, so its numbers match the easy session it now is. Called by
/// the downgrade path right after it sets the easy `intensity_label` (and
/// before it rebuilds `summary` from it): appends the easy pace / bpm / effort
/// to that label. A session whose own type names no run type now reads as an
/// easy run. No-op for an item without a run target.
pub(super) fn downgrade_run_target_to_easy(it: &mut PlanItem) {
    if it.run_target.take().is_none() {
        return;
    }
    let a = it.target_anchor.unwrap_or_default();
    it.workout_type = item_workout_type(a.kind, Some(TargetBand::Easy));
    let easy = RunIntensity::Vdot(VdotBand::Easy);
    let t = resolve_target(TargetBand::Easy, &easy, a.vdot, a.hr, a.unit);
    let pace = km_pace_label(&t).unwrap_or_default();
    for part in [&pace, &t.hr_label, &t.effort_cue] {
        if !part.is_empty() {
            it.intensity_label.push_str(" · ");
            it.intensity_label.push_str(part);
        }
    }
    it.run_target = Some(t);
}
