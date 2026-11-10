//! The review payload `SubmitReview` and `SubmitWeeklyCheckin` carry, and how
//! one payload splits into its per-session part and its weekly part, so a
//! routine session review never overwrites the weekly check-in.

use serde::{Deserialize, Serialize};
use shared_derive::ReviewParts;

use crate::feedback::SafetyScreen;
use crate::sanitize::Sanitize;

/// Lifting-set execution context for post-session feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Sanitize)]
pub struct LiftExec {
    pub reps_met: bool,
    pub rir_actual: u8,
    pub rir_target: u8,
}

/// Aerobic-decoupling context for a run's feedback.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Sanitize)]
pub struct Decouple {
    pub drift_pct: f64,
    /// Only cool, steady, sub-threshold efforts get a decoupling verdict.
    pub cool_steady_context: bool,
}

/// Interval/threshold-session execution context (feedback-015): did every rep
/// hit its target pace, and did the session cost at or below the target RPE.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Sanitize)]
pub struct IntervalExec {
    pub target_paces_met: bool,
    pub rpe_at_or_below_target: bool,
}

/// One review payload, used by two events with separate lifecycles:
/// `SubmitReview` (one session's execution: lift/run context, off day) and
/// `SubmitWeeklyCheckin` (the week-level counts and the clinical screens).
/// Every field is marked with the store it belongs to (`#[review(session)]`
/// or `#[review(weekly)]`), and the core splits every payload by that
/// marking ([`SessionReview::session_part`] / [`SessionReview::weekly_part`]),
/// so a routine session review can never overwrite the weekly check-in's
/// referral or deloads.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, Sanitize, ReviewParts)]
pub struct SessionReview {
    /// The weekly safety screens; flattened, so their wire fields sit at the
    /// top level, first.
    #[serde(flatten)]
    #[review(weekly)]
    pub safety: SafetyScreen,
    /// Single-session distance over the prior-30-day longest, as a fraction.
    #[review(session)]
    pub single_session_spike_frac: Option<f64>,
    #[review(session)]
    pub lift: Option<LiftExec>,
    /// Interval/threshold execution for the feedback-015 mastery arm.
    #[review(session)]
    pub interval: Option<IntervalExec>,
    #[review(session)]
    pub decoupling: Option<Decouple>,
    /// Fraction of an easy run spent above VT1/Zone-2 ceiling.
    #[review(session)]
    pub easy_frac_time_above_vt1: Option<f64>,
    /// Second-half slowdown percent on an even-effort run.
    #[review(session)]
    pub positive_split_pct: Option<f64>,
    /// Count of sessions this week where the planned RPE was only met at loads
    /// ≥7% below plan, a fatigue-accumulation deload trigger (autoreg-023).
    #[review(weekly)]
    pub rpe_load_gap_sessions: Option<u8>,
    /// Reference-load bar-velocity drop across the week, m/s (autoreg-026).
    #[review(weekly)]
    pub weekly_velocity_drop_m_s: Option<f64>,
    /// Failed key (top-set / quality) sessions this week (autoreg-036).
    #[review(weekly)]
    pub failed_key_sessions: Option<u8>,
    /// The at/above-MRV sign cluster is present, joint aches, performance
    /// stall, sleep disruption, motivation drop (autoreg-025; the KB defines
    /// the cluster qualitatively, no numeric count).
    #[serde(default)]
    #[review(weekly)]
    pub mrv_sign_cluster: bool,
    /// Weeks of unexplained performance decline (safety-042 / autoreg-042
    /// NFOR triggers; both need ≥2).
    #[review(weekly)]
    pub decline_weeks: Option<u8>,
    /// Wellness domains currently suppressed alongside the decline -
    /// fatigue/mood/sleep/etc. (autoreg-042 needs ≥2).
    #[review(weekly)]
    pub suppressed_wellness_domains: Option<u8>,
    /// The decline has persisted DESPITE a deload (safety-042 escalation).
    #[serde(default)]
    #[review(weekly)]
    pub despite_deload: bool,
    /// Pace at target HR improved ≥ the smallest-worthwhile change, and for how
    /// many weeks it has held (autoreg-032; ≥2 → re-test threshold pace).
    #[review(weekly)]
    pub pace_at_hr_improved_weeks: Option<u8>,
    /// Count of hypertrophy overreaching triggers present this week -
    /// performance decrement, RIR drift to 0, persistent joint/tendon aches,
    /// disrupted sleep, elevated RHR, mood drop (hypertrophy-035; ≥2 → deload).
    #[review(weekly)]
    pub hypertrophy_deload_triggers: Option<u8>,
    /// Persistent joint aches this week (hypertrophy-009 over-MRV sign).
    #[serde(default)]
    #[review(weekly)]
    pub joint_ache: bool,
    /// Performance regressing this week (hypertrophy-009 over-MRV sign;
    /// feedback-029 trend input).
    #[serde(default)]
    #[review(weekly)]
    pub performance_down: bool,
    /// Recovery currently compromised, deficit, poor sleep, or high stress
    /// (hypertrophy-010/045 → scale weekly sets to 70–80%; also the
    /// feedback-029 low-recovery trend input and the autoreg-011 wellness gate).
    #[serde(default)]
    #[review(weekly)]
    pub low_recovery: bool,
    /// Set-to-set rep drop as a fraction (e.g. 0.15 = reps fell 15% between
    /// sets), hypertrophy-039: >10% → lengthen rest.
    #[review(session)]
    pub rep_drop_frac: Option<f64>,
    /// Today's reference-load mean concentric velocity minus baseline, m/s
    /// (autoreg-008/009 VBT daily readiness; ±0.06 m/s reliability band).
    #[review(session)]
    pub mcv_delta_m_s: Option<f64>,
    /// First work set hit its target reps (autoreg-011/012 set-volume gate).
    #[review(session)]
    pub first_set_reps_met: Option<bool>,
    /// First work set RPE minus target RPE (autoreg-011/012).
    #[review(session)]
    pub first_set_rpe_delta: Option<f64>,
    /// The same lift needed set cuts in both of the last two sessions
    /// (autoreg-014 → hold weekly volume, no add).
    #[serde(default)]
    #[review(weekly)]
    pub cut_last_two_sessions: bool,
    /// Session RPE crept +1 across the week at unchanged loads (autoreg-024
    /// first condition).
    #[serde(default)]
    #[review(weekly)]
    pub rpe_creep_plus_one: bool,
    /// Days the wellness composite z-score has been ≤ −1 (autoreg-024 second
    /// condition; ≥3 with the creep → deload).
    #[review(weekly)]
    pub wellness_z_low_days: Option<u8>,
    /// Interval reps that landed at RPE ≥ target+1 or above the HR cap
    /// (autoreg-031; ≥2 → cut remaining-rep pace ~2–4%).
    #[review(session)]
    pub interval_reps_over_target: Option<u8>,
    /// Whether the prescribed easy pace was holdable under the HR cap
    /// (autoreg-033; `Some(false)` → slow the easy pace).
    #[review(session)]
    pub can_hold_easy_pace_under_hr_cap: Option<bool>,
    /// HRV readings flagged unreliable among the last three (autoreg-050;
    /// ≥2 → suspend HRV gating, fall back to subjective + performance).
    #[review(session)]
    pub hrv_unreliable_last_three: Option<u8>,
    /// Consecutive days lnRMSSD has sat below the SWC band (autoreg-034;
    /// ≥3 → insert a recovery day / easy block).
    #[review(weekly)]
    pub hrv_suppressed_days: Option<u8>,
    /// Days subjective wellness has been suppressed (autoreg-035).
    #[review(weekly)]
    pub wellness_suppressed_days: Option<u8>,
    /// Morning resting HR is trending up (autoreg-035; with ≥2 suppressed
    /// wellness days → 1–3 easy days or cross-training).
    #[serde(default)]
    #[review(weekly)]
    pub rhr_rising: bool,
    /// Single-day lnRMSSD z-score vs baseline (autoreg-028 second trigger;
    /// < −1 with a ≥2-day downtrend → downgrade hard→easy).
    #[review(weekly)]
    pub hrv_single_day_z: Option<f64>,
    /// Days the HRV has been trending down (autoreg-028 second trigger).
    #[review(weekly)]
    pub hrv_downtrend_days: Option<u8>,
    /// Concurrent-training interference symptoms present (hybrid-018/CAP-6;
    /// with running not mandatory → substitute cycling/rowing for part of
    /// the aerobic volume).
    #[serde(default)]
    #[review(weekly)]
    pub interference_symptoms: bool,
    /// Consecutive failed sessions on one novice linear-progression lift
    /// (Starting Strength stall governance; ≥3 with adequate recovery →
    /// deload that lift ~10% and re-ramp).
    #[review(weekly)]
    pub stall_failed_sessions: Option<u8>,
    /// Recovery (sleep/food/stress) was adequate across the stall, required
    /// before the stall triggers a deload rather than a recovery fix.
    #[serde(default)]
    #[review(weekly)]
    pub stall_adequate_recovery: bool,
    /// The same lift stalled again after a deload + re-ramp → transition that
    /// lift to intermediate (weekly) progression.
    #[serde(default)]
    #[review(weekly)]
    pub stalled_again_after_reramp: bool,
    /// Whether the reviewed session was planned-hard (feedback-026 tone:
    /// planned-hard → praise effort; planned-easy → celebrate restraint).
    /// `None` = unstated, no tone modifier.
    #[review(session)]
    pub planned_hard: Option<bool>,
    /// Rolling multi-week trend direction of the athlete's key metric:
    /// `"up"` / `"flat"` / `"down"` (feedback-027/028/029; any other value
    /// reads as flat). `None` = no trend submitted.
    #[review(weekly)]
    pub trend_direction: Option<crate::feedback::TrendDirection>,
    /// Weeks the rolling trend has been flat (feedback-028; ≥4 → plateau
    /// reframing).
    #[review(weekly)]
    pub weeks_flat: Option<u8>,
    /// Target missed on a genuine high-RPE off day.
    #[review(session)]
    pub bad_day: bool,
    /// When the review was submitted, unix seconds; shell-supplied (the core
    /// holds no clock), 0 = undated. Consumed by review expiry: week-scoped
    /// deloads age out after `7 × weeks` days and a session-execution note after
    /// a week (safety rows never expire); an undated review never expires.
    #[serde(default)]
    #[review(stamp)]
    pub observed_at: i64,
}

impl SessionReview {
    fn blank(observed_at: i64) -> Self {
        Self {
            observed_at,
            ..Self::default()
        }
    }

    /// The per-session part (weekly fields at their defaults).
    #[must_use]
    pub(crate) fn session_part(&self) -> Self {
        let mut s = Self::blank(self.observed_at);
        s.take_session_fields(self);
        s
    }

    /// The weekly part (session fields at their defaults).
    #[must_use]
    pub(crate) fn weekly_part(&self) -> Self {
        let mut w = self.clone();
        w.take_session_fields(&Self::default());
        w
    }

    /// Whether a `SubmitReview` of this payload replaces the session review:
    /// it carries execution context, or nothing weekly (a bare review is a
    /// session review).
    pub(crate) fn sets_session(&self) -> bool {
        self.session_part() != Self::blank(self.observed_at) || !self.sets_weekly()
    }

    /// Whether a `SubmitReview` of this payload replaces the weekly check-in:
    /// it carries any weekly/clinical field (the legacy weekly sheet sends its
    /// week counts even when they are 0, so an all-"no" weekly answer still
    /// lands here).
    pub(crate) fn sets_weekly(&self) -> bool {
        self.weekly_part() != Self::blank(self.observed_at)
    }
}
