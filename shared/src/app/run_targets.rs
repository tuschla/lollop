//! Numeric execution targets for run prescriptions: every planned run
//! reads as something a runner can follow — a pace range from the athlete's
//! current race fitness (running-007 VDOT bands, running-041 freshness), a
//! heart-rate range when HRmax is known (measured, else the Tanaka estimate),
//! and an effort cue (the session's RPE band; the talk test when nothing is
//! measured, RUN-TALKTEST-001). The plan prescribes the BAND; the numbers are
//! resolved here at render time from the anchors, so no prescription invents
//! a pace without a race to derive it from (HARD RULE 1).

use super::*;
use std::fmt::Write as _;

/// Oldest recent race (weeks) still trusted as a pace anchor: running-041's
/// "input recent race ≤6–8 wk" outer edge (`running::race_input_freshness`
/// Stale beyond it).
const RACE_ANCHOR_MAX_WEEKS: i64 = 8;

/// The pace-anchor VDOT the plan sizes and renders against on a day
/// (`plan::Anchors::run_vdot`): the profile's recent race, when it is fresh
/// enough to train off (≤8 weeks, running-041 / RUN-RETEST-001). An undated
/// race has no provable freshness, and a race run before a layoff the athlete
/// is returning from (`weeks_off`) measures fitness they no longer have, so
/// neither anchors paces; nor does a future-dated or stale one. `None` without
/// a usable race.
pub(super) fn pace_anchor_vdot(profile: &Profile, today_epoch_day: i64) -> Option<f64> {
    let race = profile.recent_race.as_ref()?;
    if !race.distance_km.is_finite() || race.distance_km <= 0.0 || race.time_sec == 0 {
        return None;
    }
    let day = race.epoch_day?;
    let age = today_epoch_day.checked_sub(day)?;
    if age < 0 {
        return None;
    }
    if age / 7 > RACE_ANCHOR_MAX_WEEKS {
        return None;
    }
    if profile.weeks_off.is_some_and(|w| w > 0.0)
        && let Some(returned) = profile
            .layoff_returned_epoch_day
            .or(profile.effective_from_epoch_day)
        && day < returned
    {
        return None;
    }
    let vdot = load::vdot(race.distance_km * 1000.0, f64::from(race.time_sec));
    (vdot.is_finite() && vdot > 0.0).then_some(vdot)
}

/// HRmax for target bpm: a plausible measured max
/// ([`running::plausible_measured_hr_max`], the same gate the HR-zone calculator
/// applies), else the Tanaka age estimate for an age in
/// [`running::tanaka_age_in_range`] (RUN-HRMAX-001). Returns `(bpm, measured)`.
pub(super) fn hr_max_anchor(profile: &Profile) -> Option<(f64, bool)> {
    if let Some(m) = running::plausible_measured_hr_max(profile.measured_hr_max) {
        return Some((m, true));
    }
    profile
        .age_years
        .filter(|a| running::tanaka_age_in_range(*a))
        .map(|a| (running::hr_max_tanaka(a), false))
}

/// A logged average heart rate in bpm when it is plausible (30–250), else
/// `None`. The one gate for both the stored bpm and its %HRmax conversion.
pub(super) fn plausible_avg_hr_bpm(avg_hr_bpm: Option<f64>) -> Option<f64> {
    avg_hr_bpm.filter(|b| b.is_finite() && (30.0..=250.0).contains(b))
}

/// The `hr_pct_max` a logged run stores. A plausible average heart rate
/// in bpm ([`plausible_avg_hr_bpm`]) is converted against [`hr_max_anchor`]
/// (measured HRmax, else the Tanaka age estimate), rounded to 0.1 %; without a
/// bpm figure or an HRmax to divide by, the sent `hr_pct_max` stands unchanged.
pub(super) fn logged_hr_pct_max(
    hr_pct_max: f64,
    avg_hr_bpm: Option<f64>,
    profile: Option<&Profile>,
) -> f64 {
    let bpm = plausible_avg_hr_bpm(avg_hr_bpm);
    match (bpm, profile.and_then(hr_max_anchor)) {
        (Some(b), Some((max, _))) => (b / max * 1000.0).round() / 10.0,
        _ => hr_pct_max,
    }
}

/// A plausible resting HR for the Karvonen %HRR ceiling (RUN-KARVONEN-001).
fn hr_rest_anchor(profile: &Profile) -> Option<f64> {
    profile
        .resting_hr_bpm
        .filter(|r| r.is_finite() && (25.0..=120.0).contains(r))
}

/// The training band a run prescription is executed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TargetBand {
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
    fn word(self) -> &'static str {
        match self {
            TargetBand::Recovery => "recovery",
            TargetBand::Easy | TargetBand::LongRun => "easy",
            TargetBand::Marathon => "marathon pace",
            TargetBand::Threshold => "threshold",
            TargetBand::Interval => "interval",
            TargetBand::Repetition => "repetition",
            TargetBand::Strides => "strides",
            TargetBand::Hills => "hill effort",
        }
    }

    /// How the band feels, in plain words, for the human pace/intensity line
    /// ("8:29–8:53 /mi (comfortably hard)"). `word` stays the wire key the
    /// shell matches on; this is display only.
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
    fn workout(self) -> running::RunWorkout {
        use crate::running::RunWorkout::{EasyGeneralAerobic, Session};
        match self {
            TargetBand::Recovery => Session(RunSessionType::Recovery),
            TargetBand::Easy => EasyGeneralAerobic,
            TargetBand::LongRun => Session(RunSessionType::LongRun),
            TargetBand::Marathon => Session(RunSessionType::RacePace),
            TargetBand::Threshold => Session(RunSessionType::Tempo),
            TargetBand::Interval => Session(RunSessionType::Interval),
            TargetBand::Repetition => Session(RunSessionType::Repetition),
            TargetBand::Strides => Session(RunSessionType::Strides),
            TargetBand::Hills => Session(RunSessionType::Hills),
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

/// Which band a run prescription runs in. The session kind settles the
/// effort-only/stride cases; otherwise the prescribed intensity does: a VDOT
/// band directly, a %HRmax band through the running-007 table (E ≤79, M 80–85,
/// T 88–92, I ≥97 %HRmax), a ceiling-only band (no floor) being Recovery
/// (running-014 states only ceilings).
pub(super) fn band_for(
    kind: Option<RunSessionType>,
    claim: ClaimId,
    intensity: &RunIntensity,
) -> Option<TargetBand> {
    if claim == claims::RUN_STRIDES_001 || kind == Some(RunSessionType::Strides) {
        return Some(TargetBand::Strides);
    }
    if kind == Some(RunSessionType::Hills) {
        return Some(TargetBand::Hills);
    }
    let long = kind == Some(RunSessionType::LongRun);
    let easy = |b: TargetBand| if long { TargetBand::LongRun } else { b };
    match *intensity {
        RunIntensity::Vdot(VdotBand::Easy) => Some(easy(TargetBand::Easy)),
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
        RunIntensity::ThreeZone(ThreeZone::Z1) => Some(easy(TargetBand::Easy)),
        RunIntensity::ThreeZone(_)
        | RunIntensity::PaceSecPerKm(_)
        | RunIntensity::PowerPercentCp(_) => None,
    }
}

/// Recovery pace floor: running-014 "≥20 % slower than marathon pace". Read
/// conservatively so BOTH interpretations hold (pace ≥1.20× MP pace and speed
/// ≤0.80× MP speed): the floor is MP pace × 1.25. MP pace is the athlete's
/// Daniels marathon prediction ÷ 42.195 km.
fn recovery_pace_floor(vdot: f64) -> Option<f64> {
    let mp_sec = load::daniels_predict(vdot, 42_195.0);
    (mp_sec > 0.0).then(|| mp_sec / 42.195 * 1.25)
}

/// Pace (fast, slow) seconds per km for a band; `fast == 0` marks a
/// "slower than" floor (Recovery), `slow == 0` an "at least this fast" bound
/// (Repetition / strides). The long run is E→E+, 10–20 % slower than marathon
/// pace (running-016); easy is 15–25 % slower (running-015, via the E band).
fn band_pace(band: TargetBand, vdot: f64) -> Option<(f64, f64)> {
    match band {
        TargetBand::Recovery => recovery_pace_floor(vdot).map(|p| (0.0, p)),
        TargetBand::LongRun => running::mp_relative_pace_sec_per_km(
            vdot,
            running::RunWorkout::Session(RunSessionType::LongRun),
        ),
        TargetBand::Repetition | TargetBand::Strides => {
            load::vdot_band_pace_sec_per_km(vdot, VdotBand::Repetition).map(|(f, _)| (f, 0.0))
        }
        b => b
            .vdot_band()
            .and_then(|vb| load::vdot_band_pace_sec_per_km(vdot, vb)),
    }
}

fn pace_label(fast: f64, slow: f64, unit: &str, per_km_factor: f64) -> String {
    let (f, s) = (fast * per_km_factor, slow * per_km_factor);
    if fast <= 0.0 {
        format!("slower than {} /{unit}", fmt_mss(s))
    } else if slow <= 0.0 {
        format!("{} /{unit} or faster", fmt_mss(f))
    } else {
        format!("{}–{} /{unit}", fmt_mss(f), fmt_mss(s))
    }
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

/// The heart-rate figures behind a target.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct HrAnchor {
    pub(super) max_bpm: f64,
    pub(super) measured: bool,
    pub(super) rest_bpm: f64,
}

pub(super) fn hr_anchor(profile: Option<&Profile>) -> HrAnchor {
    let Some(p) = profile else {
        return HrAnchor::default();
    };
    let (max_bpm, measured) = hr_max_anchor(p).unwrap_or((0.0, false));
    HrAnchor {
        max_bpm,
        measured,
        rest_bpm: hr_rest_anchor(p).unwrap_or(0.0),
    }
}

/// Resolve the numeric target for one band. `vdot` provides the paces; `hr`
/// the bpm; the effort cue is always present.
pub(super) fn resolve_target(
    band: TargetBand,
    intensity: &RunIntensity,
    vdot: Option<f64>,
    hr: HrAnchor,
) -> RunTargetView {
    let mut t = RunTargetView {
        band: band.word().to_string(),
        ..Default::default()
    };
    let mut evidence: Option<Recommended<()>> = None;

    // Pace (running-007 VDOT bands off the CURRENT race fitness).
    if let Some(v) = vdot.filter(|v| v.is_finite() && *v > 0.0)
        && let Some((fast, slow)) = band_pace(band, v)
    {
        t.vdot = (v * 10.0).round() / 10.0;
        t.pace_fast_sec_per_km = fast.round() as u32;
        t.pace_slow_sec_per_km = slow.round() as u32;
        t.pace_label_km = pace_label(fast, slow, "km", 1.0);
        t.pace_label_mi = pace_label(fast, slow, "mi", running::MILE_M / 1000.0);
        evidence = Some(graded((), claims::RUN_VDOT_001));
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
        t.hr_low_bpm = lo_bpm.unwrap_or(0.0) as u32;
        t.hr_high_bpm = hi_bpm.unwrap_or(0.0) as u32;
        t.hr_label = match (lo_bpm, hi_bpm) {
            (Some(l), Some(h)) if h > l => format!("{l:.0}–{h:.0} bpm"),
            (_, Some(h)) => format!("≤ {h:.0} bpm"),
            (Some(l), None) => format!("≥ {l:.0} bpm"),
            (None, None) => String::new(),
        };
        if !t.hr_label.is_empty() {
            t.hr_max_bpm = hr.max_bpm;
            t.hr_max_measured = hr.measured;
            t.hr_rest_bpm = hr.rest_bpm;
            if evidence.is_none() {
                evidence = Some(graded((), claims::RUN_HRMAX_001));
            }
        }
    }

    // Effort: the session's KB RPE band, always (the R row states none); the
    // talk test on easy runs when neither a pace nor a measured HRmax anchors
    // them (RUN-TALKTEST-001).
    let rpe = running::workout_rx(band.workout()).value.rpe;
    if let Some((lo, hi)) = rpe {
        t.rpe_low = lo;
        t.rpe_high = hi;
    }
    let mut detail = match rpe {
        Some((lo, hi)) if lo == hi => format!("{lo} of 10"),
        Some((lo, hi)) => format!("{lo}–{hi} of 10"),
        None => String::new(),
    };
    let unanchored = t.pace_label_km.is_empty() && !hr.measured;
    if band.is_easy() && unanchored {
        detail = if detail.is_empty() {
            "you can talk in full sentences".to_string()
        } else {
            format!("{detail}, easy enough to talk in full sentences")
        };
        if evidence.is_none() {
            evidence = Some(graded((), claims::RUN_TALKTEST_001));
        }
    }
    t.effort_cue = if rpe.is_some() {
        format!("effort {detail}")
    } else {
        detail.clone()
    };
    t.effort_detail = detail;
    if let Some(g) = evidence {
        t.citation = g.reference().to_owned();
        t.grade = Some(g.grade());
        t.confidence = g.confidence();
    }
    t
}

/// The intensity line a run item shows: the pace (with how the band feels)
/// when known, otherwise the original band label with its bpm, then the
/// effort cue.
fn target_label(base: &str, t: &RunTargetView, feel: &str) -> String {
    let mut label = if !t.pace_label_km.is_empty() {
        let mut label = format!("{} ({feel})", t.pace_label_km);
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
        RunIntensity::Vdot(_) => {
            let w = band.plain();
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
        _ => original.to_string(),
    }
}

/// Seconds as "90 s" / "2 min" / "2.5 min".
fn fmt_secs(s: u32) -> String {
    match (s / 60, s % 60) {
        (m, 0) if m >= 1 => format!("{m} min"),
        (m, 30) if m >= 1 => format!("{m}.5 min"),
        _ => format!("{s} s"),
    }
}

/// Structure text around the main set: between-rep recovery, warm-up /
/// cool-down and a strides add-on, from the plan's numeric
/// [`RunStructureView`] (zero = absent). The volume stays the summary's first
/// `·` segment (the downgrade path reads it), so every structure note goes
/// after the target.
fn structure_suffix(it: &PrescriptionView) -> String {
    let Some(s) = it.run_structure.as_ref() else {
        return String::new();
    };
    let mut out = String::new();
    if it.rep_count > 0 {
        if s.recovery_sec > 0 {
            let _ = write!(out, " · {} easy jog between reps", fmt_secs(s.recovery_sec));
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
            let _ = write!(out, ", {} easy between", fmt_secs(s.strides_recovery_sec));
        }
    }
    out
}

/// Rewrite one run item's intensity line + summary from a resolved target.
fn apply_target(it: &mut PrescriptionView, t: RunTargetView, base: &str, feel: &str) {
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

/// Fill every run item of one flattened session with its numeric target
/// (`items` are aligned with `s.prescriptions`; lifts are untouched).
pub(super) fn apply_run_targets(
    items: &mut [PrescriptionView],
    s: &Session,
    anchors: &crate::plan::Anchors,
    model: &Model,
    epoch_day: i64,
) {
    let kind = match s.session_type {
        SessionType::Run(k) => Some(k),
        _ => None,
    };
    let profile = profile_for_day(model, epoch_day).or(model.profile.as_ref());
    let hr = hr_anchor(profile);
    for (rx, it) in s.prescriptions.iter().zip(items.iter_mut()) {
        let Prescription::Run(r) = &rx.value else {
            continue;
        };
        let Some(band) = band_for(kind, rx.claim_id(), &r.intensity) else {
            continue;
        };
        let t = resolve_target(band, &r.intensity, anchors.run_vdot, hr);
        let base = base_label(&it.intensity_label, band, &r.intensity);
        apply_target(it, t, &base, band.plain());
    }
}

/// Re-resolve a run item's target for the easy band after a readiness /
/// safety downgrade, so its numbers match the easy session it now is. Called by
/// the downgrade path right after it sets the easy `intensity_label` (and
/// before it rebuilds `summary` from it): appends the easy pace / bpm / effort
/// to that label. No-op for an item without a run target.
pub(super) fn downgrade_run_target_to_easy(it: &mut PrescriptionView) {
    let Some(old) = it.run_target.take() else {
        return;
    };
    let hr = HrAnchor {
        max_bpm: old.hr_max_bpm,
        measured: old.hr_max_measured,
        rest_bpm: old.hr_rest_bpm,
    };
    let vdot = (old.vdot > 0.0).then_some(old.vdot);
    let easy = RunIntensity::Vdot(VdotBand::Easy);
    let t = resolve_target(TargetBand::Easy, &easy, vdot, hr);
    for part in [&t.pace_label_km, &t.hr_label, &t.effort_cue] {
        if !part.is_empty() {
            it.intensity_label.push_str(" · ");
            it.intensity_label.push_str(part);
        }
    }
    it.run_target = Some(t);
}
