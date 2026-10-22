//! The feedback card: the latest run's spike and split, the single resolved
//! message (safety gate first) and its coaching copy.

use super::*;
use crate::feedback;

/// Days a win (a new best, a completed week, a session or run done as planned,
/// good pacing) stays on the card: today and yesterday, so a lift day never
/// leads with a run from earlier in the week.
const WIN_FRESH_DAYS: i64 = 1;

/// The most recent run's single-session spike, for the safety gate and the
/// spike copy: fraction over the 30-day longest, its distance, that baseline,
/// and its local day (`None` day = undated).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::app) struct RunSpike {
    pub(in crate::app) frac: f64,
    pub(in crate::app) km: f64,
    pub(in crate::app) baseline_km: f64,
    pub(in crate::app) day: Option<i64>,
}

/// The most recent logged run's spike over the athlete's recent-longest
/// baseline (`frac` 0.15 = 15 % over). `None` when there is no baseline yet: a
/// first run has nothing to be a spike *over*, so the safety gate must not
/// defer on it (the run view already says why it looks unbounded). The
/// feedback resolver reads it whenever the latest run is less than a week old,
/// with or without a session review: a spike warning needs no form. A review's
/// own stated fraction still wins over it.
///
/// "Most recent" is log TIME (ties → most recently logged), so a backdated run
/// never poses as the latest. A FUTURE-dated import (a mis-timestamped 2030
/// GPX) is excluded, so a phantom run neither poses as "today's run" nor
/// evaluates against an empty trailing baseline; `None` (no clock) applies no
/// guard. The baseline is DERIVED from the current history (the longest OTHER
/// run in the trailing spike window ending at this run), NOT the per-row
/// `longest_recent_km` baked at ingest, so deleting the run that seeded it
/// re-arms the gate on the next view.
pub(in crate::app) fn latest_run_spike(model: &Model, facts: &RunFacts) -> Option<RunSpike> {
    let cutoff = end_of_local_today_sec(model);
    let (idx, r) = model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| cutoff.is_none_or(|c| r.observed_at <= c))
        .max_by_key(|(i, r)| (r.observed_at, *i))?;
    let baseline = spike_baseline_km(&model.runs, &facts.km, r.observed_at, Some(idx));
    if baseline <= 0.0 {
        return None;
    }
    let km = facts.km[idx];
    let frac = km / baseline - 1.0;
    Some(RunSpike {
        frac,
        km,
        baseline_km: km / (1.0 + frac),
        day: (!is_undated(r.observed_at))
            .then(|| local_day(r.observed_at, model.today_utc_offset_sec)),
    })
}

/// Inputs to [`build_feedback`].
pub(in crate::app) struct FeedbackCtx<'a> {
    pub(in crate::app) cx: &'a ViewCtx<'a>,
    /// The weekly check-in in force (safety signals; never expired).
    pub(in crate::app) weekly: Option<&'a SessionReview>,
    /// The current manual session review.
    pub(in crate::app) session: Option<&'a SessionReview>,
    pub(in crate::app) session_age: Option<i64>,
    /// The latest completed plan day, judged from the logs.
    pub(in crate::app) derived: Option<&'a DerivedExec>,
    /// Today's plan session: a past day's lift note expires once that lift's
    /// next session is today.
    pub(in crate::app) today_session: Option<&'a SessionPlanView>,
    pub(in crate::app) mastery: Option<&'a Mastery>,
    /// GPS-derived second-half slowdown of the latest tracked even-effort run.
    pub(in crate::app) track_split: Option<TrackSplit>,
    pub(in crate::app) spike: Option<RunSpike>,
    /// The overtraining recovery week is folded into the plan right now.
    pub(in crate::app) recovery_week_active: bool,
    /// A protective adjustment (load/volume cut, held progression, pain or
    /// illness call, deload) is in force or training is blocked: no praise or
    /// load-increase cue may compete with it (feedback-042, autoreg-044).
    pub(in crate::app) protective: bool,
    pub(in crate::app) advanced_user: bool,
    pub(in crate::app) female_user: bool,
    pub(in crate::app) weight_lb: bool,
}

/// The latest tracked run's split, with its local day (`None` = undated).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::app) struct TrackSplit {
    pub(in crate::app) pct: f64,
    pub(in crate::app) day: Option<i64>,
}

/// The split of the latest tracked run, unless it was tagged as intervals: the
/// pacing cue is for an even-effort run (feedback-016/017), and a rep workout's
/// halves differ by design.
pub(in crate::app) fn feedback_track_split(model: &Model, facts: &RunFacts) -> Option<TrackSplit> {
    let i = latest_tracked_run(model)?;
    let r = &model.runs[i];
    if r.workout_type == Some(WorkoutType::Interval) {
        return None;
    }
    Some(TrackSplit {
        pct: track_split_pct(model, facts, i)?,
        day: (!is_undated(r.observed_at))
            .then(|| local_day(r.observed_at, model.today_utc_offset_sec)),
    })
}

/// Resolve the single feedback message (safety gate first), flattened.
///
/// Sources, merged: the weekly check-in's safety signals (bone pain,
/// compulsive training, RED-S screen, sustained overtraining signs), the
/// latest run's distance spike, the manual session review (an override for its
/// day), the latest completed plan day judged from the logs (no form needed),
/// and a mastery experience (a new best or a completed week). `None` when none
/// of them has anything to say.
pub(in crate::app) fn build_feedback(c: FeedbackCtx) -> Option<FeedbackView> {
    let model = c.cx.model;
    let offset = model.today_utc_offset_sec;
    let today = model.today_epoch_day;
    let fresh = |day: Option<i64>| match (today, day) {
        (Some(t), Some(d)) => t.saturating_sub(d) <= FEEDBACK_FRESH_DAYS,
        _ => true,
    };

    let session = c
        .session
        .filter(|_| c.session_age.is_none_or(|a| a < FEEDBACK_FRESH_DAYS));
    let session_day = session
        .filter(|s| !is_undated(s.observed_at))
        .map(|s| local_day(s.observed_at, offset));
    let derived = c.derived.filter(|d| fresh(Some(d.epoch_day)));
    let spike = c.spike.filter(|s| fresh(s.day));

    let w = c.weekly;
    let safety = feedback::SafetySignals {
        bone_pain_red_flag: w.is_some_and(|w| w.bone_pain_red_flag),
        compulsive_flag: w.is_some_and(|w| w.compulsive_flag),
        reds_signal_reported: w.is_some_and(|w| w.reds_signal_reported),
        overtraining_signal_count: w.map_or(0, overtraining_count),
        overtraining_weeks: w.and_then(|w| w.overtraining_signal_weeks),
        single_session_spike_frac: session
            .and_then(|s| s.single_session_spike_frac)
            .or(spike.map(|s| s.frac)),
    };

    let (inp, exec_day) = execution_input(&c, session, session_day, derived);
    let mut exec = session_execution(&inp);
    let win_fresh = |day: Option<i64>| match (today, day) {
        (Some(t), Some(d)) => t.saturating_sub(d) <= WIN_FRESH_DAYS,
        _ => true,
    };
    if exec.as_ref().is_some_and(|e| {
        matches!(
            e.rec.value,
            FeedbackCategory::PositiveMastery | FeedbackCategory::PositiveExecution
        )
    }) && !win_fresh(exec_day)
    {
        exec = None;
    }

    let mastery = c.mastery.filter(|m| win_fresh(Some(m.day)));
    if let Some(m) = mastery {
        exec = mastery_lead(exec, exec_day, m);
    }

    let concern = feedback::safety_gate(safety);
    // A present session review keeps its card even once aged: its expired
    // execution note resolves to the neutral line.
    if concern.is_none() && exec.is_none() && c.session.is_none() {
        return None;
    }
    let trigger = exec.as_ref().map(|e| e.trigger);
    let lift = exec.as_ref().and_then(|e| e.lift.clone());
    let resolved = feedback::resolve_feedback(safety, exec.map(|e| e.rec));
    // autoreg-044 / feedback-042: while today's plan is eased or held, praise
    // and a load-increase cue would contradict it; the card says to keep to the
    // plan instead.
    let held = c.protective
        && matches!(
            resolved.value,
            FeedbackCategory::PositiveMastery | FeedbackCategory::ProgressionNudge
        );
    // A note that states the absence of anything to act on (nothing to adjust,
    // or the plan already eased) is not a graded claim: no evidence tag.
    let ungraded = held || resolved.claim_id() == claims::FEEDBACK_001;
    let category = if held {
        FeedbackCategory::InformationalNeutral
    } else {
        resolved.value
    };

    let verbosity = feedback::verbosity_for_experience(c.advanced_user).value;
    let double_progression = model
        .profile
        .as_ref()
        .is_some_and(crate::plan::uses_double_progression);
    let lighter_next = lift
        .as_ref()
        .and_then(|l| l.detail.as_ref())
        .is_some_and(|o| {
            c.cx.lift_anchors_now()
                .load_frac_for(&o.exercise)
                .is_some_and(|d| matches!(d.cause, crate::plan::DerateCause::E1rmDrop))
        });
    let mut ctx = MessageCtx {
        cat: resolved.value,
        trigger: if resolved.value.suppresses_competing_praise() {
            None
        } else {
            trigger
        },
        lift: lift.as_ref(),
        drift_pct: inp.decoupling.as_ref().map(|d| d.drift_pct),
        split_pct: inp.split_pct,
        easy_frac: inp.easy_frac,
        spike: spike.filter(|_| session.and_then(|s| s.single_session_spike_frac).is_none()),
        manual_spike: session.and_then(|s| s.single_session_spike_frac),
        run: inp.run.as_ref(),
        mastery: mastery.map(|m| &m.kind),
        derived_lift_count: derived.map_or(0, |d| d.lifts.len()),
        signs: w.and_then(|w| w.overtraining_signs),
        signal_count: w.map_or(0, overtraining_count),
        recovery_week_active: c.recovery_week_active,
        lighter_next,
        double_progression,
        plain: verbosity.minimize_jargon,
        weight_lb: c.weight_lb,
        distance_mi: false,
        today: model.today_epoch_day,
    };
    let (mut message, mut message_mi) = if held {
        (
            "Session logged. Today's plan is eased, so keep to it: no extra weight for now."
                .to_string(),
            String::new(),
        )
    } else {
        let km = feedback_message(&ctx);
        ctx.distance_mi = true;
        let mi = feedback_message(&ctx);
        let mi = if mi == km { String::new() } else { mi };
        (km, mi)
    };

    // feedback-035: a female user routed to the bone-stress referral gets the
    // gentle menstrual/nutrition clinician prompt appended (safety-critical,
    // defers to the professional, never a diagnosis).
    let bsi_referral = resolved.value == FeedbackCategory::ConcernInjury;
    if feedback::bsi_menstrual_nutrition_prompt(c.female_user, bsi_referral).value {
        message.push(' ');
        message.push_str(feedback::BSI_FEMALE_PROMPT);
        if !message_mi.is_empty() {
            message_mi.push(' ');
            message_mi.push_str(feedback::BSI_FEMALE_PROMPT);
        }
    }

    Some(feedback_view(
        category, message, message_mi, verbosity, &resolved, ungraded,
    ))
}

/// The execution input and the day it judges. The manual review leads for its
/// own day (or when it is newer than the derived day), with the derived
/// lift/run filling what it did not state; an older manual review yields to
/// the derived day.
fn execution_input(
    c: &FeedbackCtx,
    session: Option<&SessionReview>,
    session_day: Option<i64>,
    derived: Option<&DerivedExec>,
) -> (ExecInput, Option<i64>) {
    // The pacing cue reads only a split measured on the session being judged.
    let split_on = |day: Option<i64>| c.track_split.filter(|t| t.day == day).map(|t| t.pct);
    let manual_leads = session.is_some()
        && match (derived, session_day) {
            (None, _) => true,
            (Some(_), None) => true,
            (Some(d), Some(sd)) => sd >= d.epoch_day,
        };
    let same_day = matches!((derived, session_day), (Some(d), Some(sd)) if sd == d.epoch_day);
    // A lift whose next session is today has its new prescription on the
    // hero: the older day's note about it is spent.
    let superseded = |d: &DerivedExec, o: &LiftOutcome| {
        c.today_session.is_some_and(|t| {
            t.epoch_day > d.epoch_day
                && t.items
                    .iter()
                    .any(|it| !it.is_run() && crate::plan::same_movement(&it.exercise, &o.exercise))
        })
    };
    let derived_lift = || {
        derived.and_then(|d| {
            d.lifts
                .iter()
                .filter(|o| !superseded(d, o))
                .map(eval_of)
                .max_by_key(lift_severity)
        })
    };
    let mut inp = ExecInput::default();
    let mut exec_day = None;
    if let Some(s) = session.filter(|_| manual_leads) {
        inp.lift = s
            .lift
            .as_ref()
            .map(|l| LiftEval {
                reps_met: l.reps_met,
                rir_actual: Some(l.rir_actual),
                rir_target: Some(l.rir_target),
                detail: None,
            })
            .or_else(|| if same_day { derived_lift() } else { None });
        inp.interval = s.interval.clone();
        inp.decoupling = s.decoupling.clone();
        inp.easy_frac = s.easy_frac_time_above_vt1;
        inp.split_pct = s.positive_split_pct.or(split_on(session_day));
        inp.bad_day = s.bad_day;
        inp.run = if same_day {
            derived.and_then(|d| d.run.clone())
        } else {
            None
        };
        exec_day = session_day.or(derived.map(|d| d.epoch_day));
    } else if let Some(d) = derived {
        inp.lift = derived_lift();
        inp.run = d.run.clone();
        if d.run.is_some() {
            inp.split_pct = split_on(Some(d.epoch_day));
        }
        exec_day = Some(d.epoch_day);
    }
    (inp, exec_day)
}

/// A new best or a completed week leads unless a correction or an off-day
/// note for the same or a newer session is the more useful takeaway.
fn mastery_lead(exec: Option<Exec>, exec_day: Option<i64>, m: &Mastery) -> Option<Exec> {
    // An undated manual review is "now": a mastery note never overrides it.
    let mastery_newer = match (exec.is_some(), exec_day) {
        (false, _) => true,
        (true, Some(d)) => m.day > d,
        (true, None) => false,
    };
    let exec_yields = exec.as_ref().is_none_or(|e| {
        matches!(
            e.rec.value,
            FeedbackCategory::PositiveMastery | FeedbackCategory::PositiveExecution
        )
    });
    if mastery_newer || exec_yields {
        return Some(Exec {
            rec: graded(
                FeedbackCategory::PositiveMastery,
                claims::FB_MASTERY_ANCHOR_001,
            ),
            trigger: Trigger::Mastery,
            lift: None,
        });
    }
    exec
}

/// The card for the resolved category and its message; a note that is not a
/// graded claim carries no evidence tag.
fn feedback_view(
    category: FeedbackCategory,
    message: String,
    message_mi: String,
    verbosity: feedback::FeedbackVerbosity,
    resolved: &Recommended<FeedbackCategory>,
    ungraded: bool,
) -> FeedbackView {
    // feedback-023/024 verbosity + feedback-005 mastery anchoring.
    let anchor = feedback::mastery_anchor_required(category).value;

    let view = FeedbackView {
        category,
        category_label: feedback_category_label(category).to_string(),
        message,
        message_mi,
        suppresses_praise: category.suppresses_competing_praise(),
        max_takeaways: verbosity.max_takeaways,
        max_metrics: verbosity.max_metrics,
        rationale_mandatory: verbosity.rationale_mandatory,
        minimize_jargon: verbosity.minimize_jargon,
        anchor_mastery: anchor,
        evidence: EvidenceTagView::default(),
        why: WhyView::default(),
    };
    if ungraded {
        return view;
    }
    FeedbackView {
        evidence: EvidenceTagView::of(resolved),
        why: why_from(None, resolved),
        ..view
    }
}

/// Human overline for the feedback card face (File 05 category naming). Exported
/// from core so a new [`FeedbackCategory`] variant can never silently lose its
/// overline in the shell.
pub(in crate::app) fn feedback_category_label(cat: FeedbackCategory) -> &'static str {
    match cat {
        FeedbackCategory::ConcernInjury => "Injury concern",
        FeedbackCategory::ConcernRecovery => "Recovery concern",
        FeedbackCategory::ConcernBehavior => "Training pattern",
        FeedbackCategory::ConcernEnergy => "Energy & health check",
        FeedbackCategory::DangerousProgression => "Progression warning",
        FeedbackCategory::IntensityDiscipline => "Intensity",
        FeedbackCategory::PositiveExecution => "Pacing",
        FeedbackCategory::InformationalNeutral => "Note",
        FeedbackCategory::CorrectiveProcess => "Adjustment",
        FeedbackCategory::PositiveMastery => "Recent win",
        FeedbackCategory::ContextualBadDay => "Off day",
        FeedbackCategory::ProgressionNudge => "Progression",
    }
}

/// Copy context for [`feedback_message`].
pub(in crate::app) struct MessageCtx<'a> {
    pub(in crate::app) cat: FeedbackCategory,
    pub(in crate::app) trigger: Option<Trigger>,
    pub(in crate::app) lift: Option<&'a LiftEval>,
    pub(in crate::app) drift_pct: Option<f64>,
    pub(in crate::app) split_pct: Option<f64>,
    pub(in crate::app) easy_frac: Option<f64>,
    /// Log-derived spike with its distances (copy states a safe next distance).
    pub(in crate::app) spike: Option<RunSpike>,
    /// A spike fraction stated on a manual review (no distances known).
    pub(in crate::app) manual_spike: Option<f64>,
    pub(in crate::app) run: Option<&'a RunOutcome>,
    pub(in crate::app) mastery: Option<&'a MasteryKind>,
    pub(in crate::app) derived_lift_count: usize,
    pub(in crate::app) signs: Option<OvertrainingSigns>,
    pub(in crate::app) signal_count: u8,
    pub(in crate::app) recovery_week_active: bool,
    /// The plan set this lift's next session ~5 % lighter (AUTOREG-E1RM-GATE-001).
    pub(in crate::app) lighter_next: bool,
    pub(in crate::app) double_progression: bool,
    /// Beginner copy: plain words, no RIR/e1RM terms (feedback-023).
    pub(in crate::app) plain: bool,
    pub(in crate::app) weight_lb: bool,
    /// Distances in miles (the `message_mi` twin).
    pub(in crate::app) distance_mi: bool,
    /// Today's local epoch-day: a lift from today is "today", not its weekday.
    pub(in crate::app) today: Option<i64>,
}

/// What a short lift did against what was planned, e.g. " (5 reps at 154.5 lb
/// instead of 3 at 165 lb)"; empty when nothing is known, or when the top set
/// matched the plan as shown and only the set count fell short.
fn short_detail(o: &LiftOutcome, w: &dyn Fn(f64) -> String) -> String {
    let did = o
        .load_kg
        .filter(|_| o.top_reps > 0)
        .map(|kg| format!("{} reps at {}", o.top_reps, w(kg)));
    let planned = (o.reps_target > 0).then(|| match o.planned_kg {
        Some(kg) => format!("{} at {}", o.reps_target, w(kg)),
        None => format!("{} reps", o.reps_target),
    });
    let differs = u32::from(o.reps_target) != o.top_reps
        || matches!((o.load_kg, o.planned_kg), (Some(a), Some(b)) if w(a) != w(b));
    match (did, planned) {
        (Some(d), Some(p)) if differs => format!(" ({d} instead of {p})"),
        (Some(_), Some(_)) => String::new(),
        (Some(d), None) => format!(" ({d})"),
        (None, Some(p)) => format!(" (target {p})"),
        (None, None) => String::new(),
    }
}

/// "2 reps left": the one effort wording every face uses, for beginners and
/// advanced lifters alike (plain words carry no RIR jargon, feedback-023).
pub(super) fn reserve(n: u8) -> String {
    match n {
        0 => "no reps left".to_string(),
        1 => "1 rep left".to_string(),
        n => format!("{n} reps left"),
    }
}

/// "estimated max" (plain) or "e1RM" (advanced).
fn max_term(plain: bool) -> &'static str {
    if plain { "estimated max" } else { "e1RM" }
}

fn progression_cue(double_progression: bool) -> &'static str {
    if double_progression {
        "Next session adds one rep per set, or adds weight once every set reaches the top of the rep range."
    } else {
        "Next session's weight goes up as planned."
    }
}

/// Coaching copy for the resolved category (File 05 §6.2 templates with their
/// numbers; autonomy-supportive, process-framed, never guilt-inducing; concern
/// copy routes to a professional and never labels the person).
pub(in crate::app) fn feedback_message(m: &MessageCtx) -> String {
    match m.cat {
        FeedbackCategory::ConcernInjury => {
            "Stop training, especially running and jumping, and see a doctor or physio urgently. Bone pain that worsens with effort, or shows up at rest or at night, can be a stress injury.".to_string()
        }
        FeedbackCategory::ConcernRecovery => recovery_concern_message(m),
        FeedbackCategory::ConcernBehavior => {
            "You mentioned feeling driven to train even when tired, sore or unwell. Rest days are part of the plan. A doctor or sports psychologist can help you talk it through.".to_string()
        }
        FeedbackCategory::ConcernEnergy => {
            "You reported a sign that your body may be short on energy (missed or irregular periods, lower sex drive or fewer morning erections). See a doctor or sports dietitian soon. They can check whether you eat enough for your training.".to_string()
        }
        FeedbackCategory::DangerousProgression => spike_message(m),
        FeedbackCategory::IntensityDiscipline => intensity_message(m),
        FeedbackCategory::PositiveExecution => pacing_message(m),
        FeedbackCategory::InformationalNeutral => neutral_message(m),
        FeedbackCategory::CorrectiveProcess => corrective_message(m),
        FeedbackCategory::PositiveMastery => positive_mastery_message(m),
        FeedbackCategory::ContextualBadDay => {
            "Marked as an off day. Your next session stays as planned.".to_string()
        }
        FeedbackCategory::ProgressionNudge => nudge_message(m),
    }
}

/// Recovery-concern copy naming the signs the athlete ticked.
fn recovery_concern_message(m: &MessageCtx) -> String {
    let signs = match m.signs {
        Some(s) if s.count() > 0 => {
            let mut names = Vec::new();
            if s.worse_performance {
                names.push("worse performance");
            }
            if s.low_mood {
                names.push("low mood");
            }
            if s.higher_resting_hr {
                names.push("a higher resting heart rate");
            }
            if s.worse_sleep {
                names.push("worse sleep");
            }
            names.join(", ")
        }
        _ => format!("{} signs you noticed", m.signal_count),
    };
    if m.recovery_week_active {
        format!(
            "A few signs suggest fatigue is building faster than you recover ({signs}). This week's plan is lighter. If you don't feel better within 1–2 weeks, see a doctor."
        )
    } else {
        format!(
            "A few signs suggest fatigue is building faster than you recover ({signs}). If you haven't felt better since your weekly check-in, see a doctor. Update your weekly check-in when things change."
        )
    }
}

/// Spike copy: a log-derived spike states a safe next distance; a fraction
/// stated on a manual review has no distances to quote.
fn spike_message(m: &MessageCtx) -> String {
    match (m.spike, m.manual_spike) {
        (Some(s), _) => format!(
            "This run was {}, {:.0}% longer than your longest in the last 30 days ({}). Big single-run jumps are linked to more injuries, so keep your next long run at or below {}.",
            fmt_km_or_mi(s.km, m.distance_mi),
            s.frac * 100.0,
            fmt_km_or_mi(s.baseline_km, m.distance_mi),
            fmt_km_or_mi(s.baseline_km * (1.0 + running::SPIKE_FRAC), m.distance_mi)
        ),
        (None, Some(f)) => format!(
            "That run was {:.0}% longer than your longest in the last 30 days. Big single-run jumps are linked to more injuries, so keep your next long run within {:.0}% of that longest run.",
            f * 100.0,
            running::SPIKE_FRAC * 100.0
        ),
        (None, None) => format!(
            "That run was much longer than your recent longest. Keep your next long run within {:.0}% of your longest in the last 30 days.",
            running::SPIKE_FRAC * 100.0
        ),
    }
}

/// Intensity-discipline copy: a faded second half, else time above easy
/// effort.
fn intensity_message(m: &MessageCtx) -> String {
    match m.trigger {
        Some(Trigger::PositiveSplit) => format!(
            "Your second half was {:.0}% slower than your first. Start about {} easier next time to hold an even pace.",
            m.split_pct.unwrap_or(0.0),
            if m.distance_mi { "8 s/mi" } else { "5 s/km" }
        ),
        _ => format!(
            "About {:.0}% of this easy run was above easy effort. Next time slow down until you can talk in full sentences.",
            m.easy_frac.unwrap_or(0.0) * 100.0
        ),
    }
}

/// Pacing praise: aerobic durability, else the split.
fn pacing_message(m: &MessageCtx) -> String {
    match m.trigger {
        Some(Trigger::Durability) => format!(
            "Your heart rate held steady for the pace the whole run (heart-rate drift {:.1}%). Your endurance is solid at this pace.",
            m.drift_pct.unwrap_or(0.0)
        ),
        _ => match m.split_pct {
            Some(p) if p < -1.0 => format!(
                "Your second half was {:.0}% faster than your first. Well paced.",
                p.abs()
            ),
            _ => "You ran both halves at about the same pace.".to_string(),
        },
    }
}

/// The neutral note: mild drift, a lift that felt harder than planned, or
/// nothing to adjust.
fn neutral_message(m: &MessageCtx) -> String {
    let detail = m.lift.and_then(|l| l.detail.as_ref());
    let lift_name = detail.map(|o| o.exercise.as_str());
    match (m.trigger, m.lift) {
        (Some(Trigger::MildDrift), _) => format!(
            "Heart-rate drift was {:.1}%: your heart rate crept up late in the run. Common while you build endurance.",
            m.drift_pct.unwrap_or(0.0)
        ),
        (Some(Trigger::HarderThanPlanned), Some(l)) => {
            let (a, t) = (l.rir_actual.unwrap_or(0), l.rir_target.unwrap_or(0));
            let who = lift_name.unwrap_or("That");
            format!(
                "{who} felt harder than planned ({}, target {}). Keep the same weight next time and aim for {}.",
                reserve(a),
                reserve(t),
                reserve(t)
            )
        }
        _ => "Session logged. Nothing to adjust from it.".to_string(),
    }
}

/// Corrective copy: reps that cost more than planned, high heart-rate drift,
/// or a lift that came up short.
fn corrective_message(m: &MessageCtx) -> String {
    let w = |kg: f64| fmt_weight(kg, m.weight_lb);
    let detail = m.lift.and_then(|l| l.detail.as_ref());
    let lift_name = detail.map(|o| o.exercise.as_str());
    match m.trigger {
        Some(Trigger::RepsMetAtRir0) => {
            let t = m.lift.and_then(|l| l.rir_target).unwrap_or(2);
            let on = lift_name.map(|n| format!(" on {n}")).unwrap_or_default();
            format!(
                "You hit the reps{on} with {} (target {}). Keep the same weight next time, or go slightly lighter.",
                reserve(0),
                reserve(t)
            )
        }
        Some(Trigger::DecouplingHigh) => format!(
            "Heart-rate drift hit {:.0}%: the pace was too hard for this distance, or heat or low fluids pushed it up. Start about {} slower next time.",
            m.drift_pct.unwrap_or(0.0),
            if m.distance_mi { "30–50 s/mi" } else { "20–30 s/km" }
        ),
        _ => match detail {
            Some(o) => {
                let next = if m.lighter_next {
                    format!(" Your next {} session is set about 5% lighter.", o.exercise)
                } else {
                    " Keep the same weight next time and aim for every set.".to_string()
                };
                let on = o
                    .day
                    .map(|d| {
                        if Some(d) == m.today {
                            " today".to_string()
                        } else {
                            format!(" on {}", crate::app::plan_views::weekday_name(d))
                        }
                    })
                    .unwrap_or_default();
                format!(
                    "Came up short on {}{on}: {} of {} sets{}.{next} If it was an off day, mark it in the session review.",
                    o.exercise,
                    o.sets_done,
                    o.sets_planned,
                    short_detail(o, &w)
                )
            }
            None => "Came up short on the target reps. If it was an off day, mark it in the session review.".to_string(),
        },
    }
}

/// Praise with its numbers: a mastery experience, a completed session or run,
/// interval mastery, or a lift right on target.
fn positive_mastery_message(m: &MessageCtx) -> String {
    let w = |kg: f64| fmt_weight(kg, m.weight_lb);
    let detail = m.lift.and_then(|l| l.detail.as_ref());
    match (m.trigger, m.mastery) {
        (Some(Trigger::Mastery), Some(k)) => mastery_message(k, m),
        (Some(Trigger::LiftCompleted), _) => match detail {
            Some(o) => {
                let load = o
                    .load_kg
                    .map(|kg| format!(" at {}", w(kg)))
                    .unwrap_or_default();
                let others = if m.derived_lift_count > 1 {
                    format!(", and all {} exercises are done", m.derived_lift_count)
                } else {
                    String::new()
                };
                format!(
                    "All {} sets of {}{load} hit the target reps{others}.",
                    o.sets_planned, o.exercise
                )
            }
            None => "Every set hit the target reps.".to_string(),
        },
        (Some(Trigger::RunCompleted), _) => match m.run {
            Some(r) => run_done_message(r, m.distance_mi),
            None => "Run done, as planned.".to_string(),
        },
        (Some(Trigger::IntervalMastery), _) => {
            "You hit every rep at target pace without going over the planned effort.".to_string()
        }
        _ => {
            let a = m.lift.and_then(|l| l.rir_actual);
            let res = a
                .map(|a| format!(" with {}", reserve(a)))
                .unwrap_or_default();
            match detail {
                Some(o) => {
                    let load = o
                        .load_kg
                        .map(|kg| format!(" at {}", w(kg)))
                        .unwrap_or_default();
                    format!(
                        "All {} sets of {}{load}{res}, right on target. {}",
                        o.sets_planned,
                        o.exercise,
                        progression_cue(m.double_progression)
                    )
                }
                None => format!(
                    "All sets done{res}, right on target. {}",
                    progression_cue(m.double_progression)
                ),
            }
        }
    }
}

/// The progression nudge: the lift felt easy, so progress it.
fn nudge_message(m: &MessageCtx) -> String {
    let detail = m.lift.and_then(|l| l.detail.as_ref());
    let lift_name = detail.map(|o| o.exercise.as_str());
    let feel = match m.lift.and_then(|l| l.rir_actual.zip(l.rir_target)) {
        Some((a, t)) => {
            let who = lift_name
                .map(|n| format!("{n} "))
                .unwrap_or_else(|| "That ".to_string());
            format!("{who}felt easy ({}, target {}).", reserve(a), reserve(t))
        }
        None => "That was well under your target effort.".to_string(),
    };
    let cue = if m.double_progression {
        progression_cue(true)
    } else {
        "Add a little weight next session."
    };
    format!("{feel} {cue}")
}

/// Mastery copy with its numbers (feedback-005 + KB 05 §6.2 POSITIVE_MASTERY).
fn mastery_message(k: &MasteryKind, m: &MessageCtx) -> String {
    let w = |kg: f64| fmt_weight(kg, m.weight_lb);
    match k {
        MasteryKind::E1rmPr {
            exercise,
            e1rm,
            prior,
            weight,
            reps,
            rpe,
        } => {
            let gain = e1rm - prior;
            if m.plain {
                format!(
                    "New best on {exercise}: {} × {reps}. Your {} is up {} on your best of the last 90 days.",
                    w(*weight),
                    max_term(true),
                    w(gain)
                )
            } else {
                let left = if *rpe > 0.0 {
                    let rir = crate::strength::rpe_to_rir(*rpe).round() as u8;
                    format!(" with {}", reserve(rir))
                } else {
                    String::new()
                };
                format!(
                    "New best: {exercise} {} {} (+{}, +{:.0}% on your 90-day best) from {} × {reps}{left}.",
                    max_term(false),
                    w(*e1rm),
                    w(gain),
                    (e1rm / prior - 1.0) * 100.0,
                    w(*weight)
                )
            }
        }
        MasteryKind::RepPr {
            exercise,
            weight,
            reps,
            prior_reps,
        } => format!(
            "Rep best on {exercise}: {reps} reps at {}, {} more than your best at that weight in the last 90 days.",
            w(*weight),
            reps - prior_reps
        ),
        MasteryKind::LongestRun { km, prior_km } => format!(
            "Longest run in 90 days: {} (previous longest {}), built up gradually.",
            fmt_km_or_mi(*km, m.distance_mi),
            fmt_km_or_mi(*prior_km, m.distance_mi)
        ),
        MasteryKind::WeekComplete { sessions } => {
            format!("All {sessions} planned sessions this week done.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short_msg(load_kg: f64, day: i64) -> String {
        let lift = LiftEval {
            reps_met: false,
            rir_actual: None,
            rir_target: None,
            detail: Some(LiftOutcome {
                exercise: "Barbell back squat".into(),
                load_kg: Some(load_kg),
                top_reps: 5,
                sets_done: 3,
                sets_planned: 6,
                reps_target: 5,
                reps_met: false,
                rir_actual: None,
                rir_target: None,
                planned_kg: Some(124.74),
                day: Some(day),
            }),
        };
        feedback_message(&MessageCtx {
            cat: FeedbackCategory::CorrectiveProcess,
            trigger: None,
            lift: Some(&lift),
            drift_pct: None,
            split_pct: None,
            easy_frac: None,
            spike: None,
            manual_spike: None,
            run: None,
            mastery: None,
            derived_lift_count: 1,
            signs: None,
            signal_count: 0,
            recovery_week_active: false,
            lighter_next: false,
            double_progression: false,
            plain: true,
            weight_lb: true,
            distance_mi: false,
            today: Some(100),
        })
    }

    #[test]
    fn came_up_short_says_today_and_compares_the_loads_as_shown() {
        // final-today F6: 124.7 kg and 124.74 kg both render "275 lb", so the
        // only shortfall is the set count: no "(5 reps at 275 lb instead of 5
        // at 275 lb)", and a lift from today says "today".
        let m = short_msg(124.7, 100);
        assert!(
            m.starts_with("Came up short on Barbell back squat today: 3 of 6 sets."),
            "{m}"
        );
        assert!(!m.contains("instead of"), "{m}");
        // Another day names its weekday; a load that renders differently is
        // still spelled out.
        let m = short_msg(120.0, 99);
        assert!(m.contains("Barbell back squat on "), "{m}");
        assert!(
            m.contains("(5 reps at 264.5 lb instead of 5 at 275 lb)"),
            "{m}"
        );
    }
}
