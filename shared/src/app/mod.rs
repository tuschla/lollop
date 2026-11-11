//! The side-effect-free core (crux `App`). `update` folds each event into the
//! model (sanitizing every wire value on the way in); `view()` runs the pure
//! autoregulation layer (`crate::autoreg`), the plan synthesis and every
//! evidence-cited builder over it. No IO, no clock, no randomness.

use crux_core::macros::effect;
use crux_core::render::{RenderOperation, render};
use crux_core::{App, Command};

use crate::app::calculators::{
    build_hypertrophy_plan, build_protein_targets, build_srpe_load, build_training_load,
    build_weekly_report, calculator_hold, hr_zones_for, to_race_view,
};
use crate::app::evidence_views::{
    CallCtx, call_rows, grade_definitions, hrv_single_morning, lift_work_logged,
};
use crate::app::guidance::{
    PROFILE_SECTION, build_calisthenics_guidance, build_guidance_measured, build_reference,
    stated_or_measured_weekly_km,
};
use crate::app::lift_audit::{build_lift_audit, lift_views};
use crate::app::model::profile_for_day;
use crate::app::plan_views::{
    ItemCue, PlanRow, PlanViews, apply_first_set_autoreg, build_calisthenics_ladders,
    build_consistency, build_plan_views, finished_status, render_basis,
};
use crate::app::readiness::{
    HeadlineInputs, REDS_SCREEN_WINDOW_DAYS, RankedRow, ReadinessPicture, blocks_training,
    build_autoreg_source, build_headline, build_provisional, build_signal_groups, hold_reason,
    hold_resolutions, readiness_picture, suppression_active, to_baseline_status_view,
};
use crate::app::review_deloads::{review_deloads, weekly_reds_signal};
use crate::app::reviews::{
    DerivedExec, FeedbackCtx, ReviewCtx, annotate_last_time, build_feedback, build_session_done,
    build_trend, derive_session_exec, detect_mastery, feedback_track_split, latest_run_spike,
    link_win_advice, merged_review, next_time_add_superseded, remaining_sets_call_spent,
    review_adjustment_expired, review_age_days, review_views,
};
use crate::app::run_metrics::longest_logged_run_km;
use crate::app::run_views::{annotate_run_views, build_run_progress, run_views};
use crate::app::view_ctx::ViewCtx;
use crate::app::view_types::{
    CalisLadderView, HoldReason, HoldResolutionView, HrZoneBandView, HypertrophyFiguresView,
    RunProgressView, SessionDoneView, SessionStatus, SrpeLoadView, TodaySurfaceView,
};
use crate::app::youth_activity::{activity_views, build_youth_activity};
use crate::day::{is_undated, local_day};
use crate::event::Event;
use crate::profile::{PlanProfile, PlanWithheld};
use crate::schema::{
    Adjustment, AdjustmentCall, AdjustmentKind, CheckinInput, ReadinessInput, ReadinessSignal,
    SafetyTier,
};
use crate::{autoreg, individualization};

mod anchors;
mod calculators;
mod calis_anchors;
mod evidence_views;
mod format;
mod guidance;
mod ingest;
mod lift_audit;
mod logged_days;
mod model;
mod plan_views;
mod profile_age;
mod readiness;
mod review_deloads;
mod reviews;
mod run_metrics;
mod run_targets;
mod run_views;
mod set_index;
mod text_parts;
mod today_surface;
mod view_ctx;
mod view_types;
mod youth_activity;

pub use model::{HrZoneInputView, HypertrophyInputView, Model, ProteinInputView};
pub use view_types::{
    ActivityResultView, AdjustmentView, BaselineStatusView, E1rmRangeView, FeedbackView,
    GradeDefView, GuidanceView, HrMaxView, IntervalVerdictView, LiftResultView, PrescriptionView,
    ProgramSummaryView, ProteinFigureView, RacePredictionView, ReadinessSignalView, RunResultView,
    RunSplitView, SessionPlanView, SignalGroupView, SplitVerdictView, TodayHeadlineView,
    TrainingLoadView, ViewModel, WhyView, YouthActivityView,
};

// The bridge (`crate::ffi`) answers its GPX export, track-figures and
// draft-facts ops with these; like the bridge, they are compiled only where it
// exists (Android, or the `ffi` feature).
#[cfg(any(ffi_surface, test))]
pub(crate) use ingest::preview_track_figures;
#[cfg(any(ffi_surface, test))]
pub(crate) use profile_age::draft_facts;
#[cfg(any(ffi_surface, test))]
pub(crate) use run_views::{export_run_gpx, run_track_segments};

#[effect(typegen)]
#[derive(Debug)]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Default)]
pub struct Engine;

impl App for Engine {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Self::Event, model: &mut Self::Model) -> Command<Effect, Event> {
        ingest::apply(model, event);
        render()
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        // Everything several builders read (run facts, session-day attribution,
        // today's anchors) is derived once here.
        let cx = ViewCtx::new(model);
        // Display unit for every weight string the shell renders verbatim (the
        // numeric view fields stay kg regardless; see `Profile::weight_lb`).
        let weight_lb = model.profile().is_some_and(|p| p.weight_lb);

        // Today's readiness: the retained check-ins normalized into the
        // synthetic signals (WellnessZ / HrvLnRmssd / RestingHr) the autoreg
        // rules consume, merged with the manual/advanced inputs, through the
        // same pipeline every past rendered day uses. The profile's lifting goal
        // selects goal-dependent thresholds (velocity-loss termination,
        // autoreg-010); no profile → conservative defaults.
        let picture = readiness_picture(model, &cx.days, model.profile(), model.today_epoch_day);
        let derived = &picture.derived;
        let v = Verdicts::new(model, &cx, &picture);
        let recommended = &v.review.recommended;
        let review_recs = &v.review.review_recs;
        let train_blocked = v.safety.hold.is_some();
        let reds_present = v.safety.reds_present;

        let (derived_hrv_streak, derived_wellness_streak) = derived_streaks(derived);
        let review_adjustments = merged_review(model.weekly_checkin(), model.review.as_ref())
            .as_ref()
            .map(|r| {
                review_views(
                    r,
                    ReviewCtx {
                        recs: review_recs,
                        readiness_recs: recommended,
                        train_blocked,
                        review_age: v.review.review_age,
                        profile: model.profile(),
                        derived_hrv_streak,
                        derived_wellness_streak,
                    },
                )
            })
            .unwrap_or_default();

        let PlanSection {
            next_session,
            week_plan,
            next_week_plan,
            program,
            planned_run_km,
            adjustments,
            derived_exec,
            session_done,
        } = plan_section(&cx, &v, weight_lb);

        let FeedbackSection {
            feedback,
            win_lift,
            trend,
        } = feedback_section(
            &cx,
            &v,
            &week_plan,
            derived_exec.as_ref(),
            derived_wellness_streak,
            weight_lb,
        );
        // One hold for Today, the plan calculators and the prescriptive
        // guidance: today's training hold, else a RED-S signal.
        let calc_hold = calculator_hold(v.safety.hold.as_ref(), reds_present);
        let CalcSection {
            protein_targets,
            protein_figures,
            protein_input,
            protein_masters_offered,
            hr_zones,
            hr_max,
            hr_zone_bands,
            hr_zone_input,
            hypertrophy_plan,
            hypertrophy_figures,
            hypertrophy_ramp,
            hypertrophy_input,
            race_prediction,
        } = calc_section(&cx, calc_hold.as_ref(), reds_present, weight_lb);
        let today_headline = build_headline(&HeadlineInputs {
            train_blocked,
            gates: &v.safety.gates,
            recommended,
            review_recs,
            adjustments: &adjustments,
            review_adjustments: &review_adjustments,
            feedback: feedback.as_ref(),
            prescription: next_session.as_ref(),
        });
        let HistorySection {
            lifts,
            runs,
            run_progress,
            training_load,
            srpe_load,
            provisional,
            activities,
            youth_activity,
        } = history_section(&cx, planned_run_km, weight_lb);
        let GuidanceSection {
            guidance,
            profile_guidance,
            reference,
            weekly_report,
            lift_audit,
            calisthenics_ladders,
        } = guidance_section(&cx, calc_hold.as_ref(), reds_present, &week_plan);
        let ReadinessSection {
            checked_in_today,
            weekly_checkin_present,
            input_count,
            readiness_summary,
            autoreg_source,
            signal_groups,
            latest_checkin,
            baseline_status,
        } = readiness_section(model, &v, derived_hrv_streak.is_some());
        let SafetySection {
            safety_tier,
            hold_reason,
            hold_resolutions,
            illness_check_prompt,
            plan_withheld,
        } = safety_section(model, &v);
        // ADH-SELFMON-001: planned vs done, built off the week strip before it
        // moves into the view.
        let consistency = build_consistency(model, &cx.days, &week_plan, train_blocked);

        let mut vm = ViewModel {
            safety_tier,
            hold_reason,
            checked_in_today,
            adjustments: adjustments.into_iter().map(|r| r.view).collect(),
            review_adjustments: review_adjustments.into_iter().map(|r| r.view).collect(),
            weekly_checkin_present,
            input_count,
            lifts,
            runs,
            guidance,
            profile_guidance,
            feedback,
            reference,
            profile: model.profile().cloned(),
            race_prediction,
            hypertrophy_plan,
            protein_targets,
            hr_zones,
            training_load,
            srpe_load,
            weekly_report,
            run_progress,
            lift_audit,
            hr_zone_input,
            protein_input,
            protein_masters_offered,
            hypertrophy_input,
            hypertrophy_ramp,
            trend,
            provisional,
            autoreg_source,
            readiness_summary,
            today_headline,
            signal_groups,
            latest_checkin,
            baseline_status,
            grade_definitions: grade_definitions(),
            consistency,
            next_session,
            week_plan,
            next_week_plan,
            program,
            plan_removed: model.plan_removed,
            hr_max,
            hr_max_known: model
                .profile()
                .is_some_and(|p| run_targets::hr_max_anchor(p).is_some()),
            protein_figures,
            hr_zone_bands,
            hypertrophy_figures,
            today_epoch_day: model.today_epoch_day,
            session_done,
            calisthenics_ladders,
            hold_resolutions,
            illness_check_prompt,
            plan_withheld,
            activities,
            youth_activity,
            active_pain: v.picture.active_pain(),
            today_surface: TodaySurfaceView::default(),
        };
        today_surface::mark_today(&mut vm, model);
        link_win_advice(win_lift.as_deref(), &mut vm.adjustments);
        link_win_advice(win_lift.as_deref(), &mut vm.review_adjustments);
        text_parts::mark_view(&mut vm);
        vm
    }
}

/// What today's readiness rules, the reviews and the onboarding screen
/// decided: the gates every view section after them reads.
struct Verdicts<'a> {
    /// Today's fresh readiness inputs (recency-expired readings excluded).
    readiness_inputs: &'a [ReadinessInput],
    picture: &'a ReadinessPicture,
    review: ReviewPipeline,
    safety: SafetyState,
}

impl<'a> Verdicts<'a> {
    fn new(model: &Model, cx: &ViewCtx, picture: &'a ReadinessPicture) -> Self {
        let review = review_pipeline(model, picture.adjustments(model, &cx.days));
        let safety = safety_state(model, &picture.inputs, picture.safety_tier(), &review);
        Verdicts {
            readiness_inputs: &picture.inputs,
            picture,
            review,
            safety,
        }
    }
}

/// The review-carried adjustments, with today's readiness recommendations
/// reconciled against them.
struct ReviewPipeline {
    /// Age in days of the session review (`None`: none, undated, or no clock).
    review_age: Option<i64>,
    /// The weekly check-in's week-level deloads and deferrals still in force.
    review_recs: Vec<AdjustmentCall>,
    /// Today's readiness recommendations, minus any load increase a
    /// protective review rec overrides.
    recommended: Vec<AdjustmentCall>,
}

/// The weekly check-in carries the week-level deloads and the clinical
/// screens; the session review only one session's execution. Each ages on its
/// own clock.
fn review_pipeline(model: &Model, recommended: Vec<AdjustmentCall>) -> ReviewPipeline {
    let offset = model.today_utc_offset_sec;
    let weekly_age = model
        .weekly_checkin()
        .and_then(|r| review_age_days(r, model.today_epoch_day, offset));
    let review_age = model
        .review
        .as_ref()
        .and_then(|r| review_age_days(r, model.today_epoch_day, offset));
    let review_recs: Vec<AdjustmentCall> = model
        .weekly_checkin()
        .map(review_deloads)
        .unwrap_or_default()
        .into_iter()
        .filter(|rec| !review_adjustment_expired(&rec.value, weekly_age))
        .collect();

    // Same-pipeline reconciliation, mirroring `autoreg::adjustments_in` but
    // across pipelines: when the review already recommends a protective
    // (load-reducing / rest / defer) action, a readiness-derived load INCREASE
    // must not stand, or it would outrank an active deload in the headline.
    // Reconciled here, at the level where the variant is available.
    let recommended = if suppression_active(&review_recs, false) {
        recommended
            .into_iter()
            .filter(|r| !matches!(r.value.base(), Adjustment::IncreaseLoadPct(_)))
            .collect()
    } else {
        recommended
    };
    ReviewPipeline {
        review_age,
        review_recs,
        recommended,
    }
}

/// Whether training is blocked today, and how far up the safety ladder.
struct SafetyState {
    /// Stage-0 onboarding gates (File 08 onboard-050): profile health-screen
    /// deferrals. Rendered as Safety guidance rows (build_guidance) AND
    /// reflected in `safety_tier` and the hold so no shell can miss them.
    gates: Vec<AdjustmentCall>,
    /// Today's training hold: the first blocking call (Stop / RestDay /
    /// Defer) of the profile gates, then readiness, then the weekly check-in.
    /// The single source for Today's block, [`hold_reason`], the plan
    /// calculators and %1RM guidance ([`calculator_hold`]).
    hold: Option<AdjustmentCall>,
    safety_tier: Option<SafetyTier>,
    /// RED-S flagged on any channel as of today; feeds the safety-022 deficit
    /// refusal and the calculator hold.
    reds_present: bool,
}

/// `readiness_inputs` are today's date-scoped readiness inputs (removals in
/// force today applied): the same inputs the readiness rules ran on.
fn safety_state(
    model: &Model,
    readiness_inputs: &[ReadinessInput],
    readiness_tier: Option<SafetyTier>,
    review: &ReviewPipeline,
) -> SafetyState {
    let gates = model
        .profile()
        .map(|p| individualization::onboarding_gates(&p.health))
        .unwrap_or_default();
    let hold = gates
        .iter()
        .chain(&review.recommended)
        .chain(&review.review_recs)
        .find(|r| blocks_training(r))
        .cloned();

    // Highest safety tier: the readiness ladder, raised to MedicalReferral when
    // an onboarding gate or a review-carried NFOR/OTS deferral fires: the KB
    // defers those to a professional (File 08 §5; HARD RULE 3).
    let deferred = !gates.is_empty()
        || review
            .review_recs
            .iter()
            .any(|r| matches!(r.value, Adjustment::Defer { .. }));
    let safety_tier = if deferred {
        Some(SafetyTier::MedicalReferral)
    } else {
        readiness_tier
    };

    // RED-S from any channel as of today: the readiness signal (read by the
    // same rule, over the same date-scoped inputs, as the readiness hold; a
    // future-dated all-clear or retraction is not in force yet), the
    // onboarding screen, or a weekly RED-S signal (screen or compulsive
    // exercise, REDS-SCREEN-001 / safety-049) in force or within the screen
    // window.
    let reds_present = autoreg::reds_defer(readiness_inputs)
        || model.profile().is_some_and(|p| p.health.reds_signal)
        || model.weekly_checkin().is_some_and(weekly_reds_signal)
        || model.weekly_history.iter().any(|r| {
            weekly_reds_signal(r)
                && model.today_epoch_day.is_none_or(|today| {
                    is_undated(r.observed_at)
                        || today
                            .saturating_sub(local_day(r.observed_at, model.today_utc_offset_sec))
                            < REDS_SCREEN_WINDOW_DAYS
                })
        });

    SafetyState {
        gates,
        hold,
        safety_tier,
        reds_present,
    }
}

/// The core-derived HRV and wellness streaks, `(hrv, wellness)`.
///
/// autoreg-034 is core-derived: the HRV rolling channel counts consecutive
/// suppressed days itself, so it is preferred over any shell-supplied count.
/// The streak counts DAILY dips (one gap tolerated); the recovery-day row
/// fires only while the 7-day mean itself sits below the SWC band, the signal
/// the rule is about, so the row never contradicts an "HRV normal" strip.
fn derived_streaks(derived: &autoreg::DerivedReadiness) -> (Option<u8>, Option<u8>) {
    let hrv = derived
        .inputs
        .iter()
        .find(|i| i.signal == ReadinessSignal::HrvLnRmssd)
        .map(|i| {
            if autoreg::hrv_below_swc(i.value) {
                i.streak
            } else {
                0
            }
        });
    let wellness = derived
        .inputs
        .iter()
        .find(|i| i.signal == ReadinessSignal::WellnessZ)
        .map(|i| i.streak);
    (hrv, wellness)
}

/// The plan surface and today's readiness rows, settled against each other.
struct PlanSection {
    next_session: Option<SessionPlanView>,
    week_plan: Vec<SessionPlanView>,
    next_week_plan: Vec<SessionPlanView>,
    program: Option<ProgramSummaryView>,
    planned_run_km: f64,
    adjustments: Vec<RankedRow>,
    /// The latest completed plan day this week, judged from the logs.
    derived_exec: Option<DerivedExec>,
    session_done: Option<SessionDoneView>,
}

/// Coach-as-planner: synthesize + date the week, then apply readiness/safety
/// INSIDE the rendered session (strictly downstream of the gates; HARD RULE
/// 3). The prescription becomes the top non-safety headline rung. The
/// readiness rows are rendered last, against what today's session already
/// says. The hero is its strip row, so `next_session` is that row as rendered.
fn plan_section(cx: &ViewCtx, v: &Verdicts, weight_lb: bool) -> PlanSection {
    let today = cx.model.today_epoch_day;
    let (plan, adjustments) = working_plan_section(cx, v, weight_lb);
    let derived_exec = today.and_then(|t| derive_session_exec(&plan.week_plan, t));
    let session_done = build_session_done(&plan.week_plan, today);
    let next_session = plan
        .hero_day
        .and_then(|d| plan.row(d))
        .cloned()
        .map(PlanRow::into_view);
    PlanSection {
        next_session,
        week_plan: plan.week_plan.into_iter().map(PlanRow::into_view).collect(),
        next_week_plan: plan
            .next_week_plan
            .into_iter()
            .map(PlanRow::into_view)
            .collect(),
        program: plan.program,
        planned_run_km: plan.planned_run_km,
        adjustments,
        derived_exec,
        session_done,
    }
}

/// The plan as [`plan_section`] works it before rendering, with today's
/// readiness rows.
fn working_plan_section(
    cx: &ViewCtx,
    v: &Verdicts,
    weight_lb: bool,
) -> (PlanViews, Vec<RankedRow>) {
    let model = cx.model;
    let today = model.today_epoch_day;
    let recommended = &v.review.recommended;
    let review_recs = &v.review.review_recs;
    let wording = CallCtx {
        hrv_single_morning: hrv_single_morning(v.readiness_inputs),
        wellness_collecting: v.picture.wellness_collecting(),
        ..CallCtx::default()
    };
    let mut plan = build_plan_views(
        cx,
        v.safety.hold.is_some(),
        recommended,
        review_recs,
        &wording,
    );
    let cues = match today.and_then(|t| plan.row_mut(t)) {
        Some(row) => apply_first_set_cues(v, row, weight_lb),
        None => Vec::new(),
    };
    // Every fold, hold and cue has moved its numbers: the why? basis restates
    // the planned prescription against today's, so the session panel reads
    // the face.
    for it in plan
        .week_plan
        .iter_mut()
        .chain(plan.next_week_plan.iter_mut())
        .flat_map(|s| s.items.iter_mut())
    {
        render_basis(it, weight_lb);
    }
    // Say what changed on each lift since last time, and why.
    if let Some(hero) = plan.hero_day.and_then(|d| plan.row_mut(d)) {
        annotate_last_time(hero, model, &cx.set_index, cx.lift_anchors_now(), weight_lb);
    }
    let today_session = today.and_then(|t| plan.row(t));
    let ctx = CallCtx {
        first_set_logged: today_session.is_some_and(|ts| lift_work_logged(&ts.items)),
        items: today_session.map_or(&[], |ts| &ts.items),
        ..wording
    };
    let fresh: Vec<AdjustmentCall> = cues
        .iter()
        .filter(|c| c.fresh())
        .map(|c| c.rec.clone())
        .collect();
    // One cue per lift: today's in-session add replaces the "next time" add
    // for the same lift; a "remaining sets" call on an exercise already
    // finished today has nothing left to change. A cue's load rides on the
    // row of the call it applies.
    let adjustments: Vec<RankedRow> = call_rows(recommended, &ctx)
        .into_iter()
        .filter(|(r, _)| {
            !next_time_add_superseded(r, &fresh)
                && !today_session.is_some_and(|ts| remaining_sets_call_spent(r, ts))
        })
        .chain(call_rows(&fresh, &ctx))
        .map(|(r, mut view)| {
            if let Some(c) = cues.iter().rfind(|c| c.rec == *r) {
                view.suggested_load_kg = c.load_kg;
            }
            RankedRow {
                view,
                tier: r.tier(),
                neutral: false,
            }
        })
        .collect();
    (plan, adjustments)
}

/// The plan rows as `view()` works them, before rendering: the working
/// fields behind the rendered rows.
#[cfg(test)]
fn working_plan(model: &Model) -> PlanViews {
    let cx = ViewCtx::new(model);
    let picture = readiness_picture(model, &cx.days, model.profile(), model.today_epoch_day);
    let v = Verdicts::new(model, &cx, &picture);
    let weight_lb = model.profile().is_some_and(|p| p.weight_lb);
    working_plan_section(&cx, &v, weight_lb).0
}

/// Today's logged first work set of each lift, against that item's target,
/// scopes a load cut (or an opt-in add) to THAT lift's remaining sets; a
/// hard first set on a held day becomes the one-level downgrade by
/// composition. Today's session is re-judged at the cue loads (a lift
/// finished at the "remaining sets drop 5%" load is done, not lighter or
/// open); a session already done at full has nothing to re-judge. Returns the
/// cues.
fn apply_first_set_cues(v: &Verdicts, today: &mut PlanRow, weight_lb: bool) -> Vec<ItemCue> {
    // A training hold blanks today's session (HARD RULE 3): no cue to give.
    if v.safety.hold.is_some() || today.status == SessionStatus::Done {
        return Vec::new();
    }
    let recommended = &v.review.recommended;
    let review_recs = &v.review.review_recs;
    let no_increase = v.picture.increase_hold()
        || suppression_active(recommended, false)
        || suppression_active(review_recs, false);
    let cues = apply_first_set_autoreg(today, recommended, review_recs, no_increase, weight_lb);
    today.status = finished_status(&today.items).unwrap_or(today.status);
    cues
}

/// The calculators' prose rows, the structured figures beside them (so a
/// shell reads numbers instead of scraping the summary strings) and the
/// queries they answer.
struct CalcSection {
    protein_targets: Vec<GuidanceView>,
    protein_figures: Vec<ProteinFigureView>,
    protein_input: Option<ProteinInputView>,
    protein_masters_offered: bool,
    hr_zones: Vec<GuidanceView>,
    hr_max: Option<HrMaxView>,
    hr_zone_bands: Vec<HrZoneBandView>,
    hr_zone_input: Option<HrZoneInputView>,
    hypertrophy_plan: Vec<GuidanceView>,
    hypertrophy_figures: Option<HypertrophyFiguresView>,
    hypertrophy_ramp: Vec<u8>,
    hypertrophy_input: Option<HypertrophyInputView>,
    race_prediction: Option<RacePredictionView>,
}

fn calc_section(
    cx: &ViewCtx,
    calc_hold: Option<&AdjustmentCall>,
    reds_present: bool,
    weight_lb: bool,
) -> CalcSection {
    let model = cx.model;
    let (protein_targets, protein_figures) = model
        .protein_query
        .as_ref()
        .map(|q| build_protein_targets(q, reds_present, weight_lb))
        .unwrap_or_default();
    let (hr_zones, hr_max, hr_zone_bands) = model
        .hr_zone_query
        .as_ref()
        .map(|q| hr_zones_for(q, model))
        .unwrap_or_default();
    let (hypertrophy_plan, hypertrophy_figures, hypertrophy_ramp) = model
        .hypertrophy_plan_query
        .as_ref()
        .map(|q| {
            let pp = model.profile().map(PlanProfile::new);
            build_hypertrophy_plan(q, pp.as_ref(), calc_hold)
        })
        .unwrap_or_default();
    CalcSection {
        protein_targets,
        protein_figures,
        protein_input: model.protein_query.clone(),
        protein_masters_offered: model
            .profile()
            .and_then(|p| p.age_years)
            .is_none_or(|a| individualization::masters_age(Some(a))),
        hr_zones,
        hr_max,
        hr_zone_bands,
        hr_zone_input: model.hr_zone_query.clone(),
        hypertrophy_plan,
        hypertrophy_figures,
        hypertrophy_ramp,
        hypertrophy_input: model.hypertrophy_plan_query.clone(),
        race_prediction: model
            .race_query
            .as_ref()
            .map(|q| to_race_view(q, longest_logged_run_km(&cx.runs), model.distance_unit())),
    }
}

/// The coaching takeaway, the lift its win names and the trend line beside it.
struct FeedbackSection {
    feedback: Option<FeedbackView>,
    win_lift: Option<String>,
    trend: Option<AdjustmentView>,
}

/// One coaching takeaway from the weekly check-in's safety signals, the
/// latest run's spike, the session review, the latest completed plan day
/// judged from the logs (no form needed) and any new best; and the trend
/// line, which reads the same spike.
fn feedback_section(
    cx: &ViewCtx,
    v: &Verdicts,
    week_plan: &[SessionPlanView],
    derived_exec: Option<&DerivedExec>,
    derived_wellness_streak: Option<u8>,
    weight_lb: bool,
) -> FeedbackSection {
    let model = cx.model;
    // Feedback personalization inputs (feedback-023/024/035).
    let advanced_user = cx
        .plan_profile
        .as_ref()
        .is_some_and(|pp| pp.training_age() == individualization::TrainingAge::Advanced);
    let female_user = model.profile().is_some_and(|p| p.female.unwrap_or(false));
    let mastery = detect_mastery(model, &cx.set_index, &cx.runs, week_plan);
    let spike = latest_run_spike(model, &cx.runs);
    let review_recs = &v.review.review_recs;
    let feedback = build_feedback(FeedbackCtx {
        cx,
        weekly: model.weekly_checkin(),
        session: model.review.as_ref(),
        session_age: v.review.review_age,
        derived: derived_exec,
        today_session: model
            .today_epoch_day
            .and_then(|t| week_plan.iter().find(|s| s.epoch_day == t)),
        mastery: mastery.as_ref(),
        track_split: feedback_track_split(model, &cx.runs),
        spike,
        recovery_week_active: review_recs.iter().any(|r| {
            r.kind() == AdjustmentKind::WeeklyDeload(crate::schema::DeloadTrigger::RecoveryWeek)
        }),
        protective: suppression_active(&v.review.recommended, v.safety.hold.is_some())
            || suppression_active(review_recs, false),
        advanced_user,
        female_user,
        weight_lb,
    });
    // feedback-039/041/042: no trend praise or causal story while a concern
    // card leads or training is blocked.
    let trend_suppressed = v.safety.hold.is_some()
        || feedback
            .as_ref()
            .is_some_and(|f| f.view.category.suppresses_competing_praise());
    let trend = build_trend(
        model,
        &cx.set_index,
        model.weekly_checkin(),
        derived_wellness_streak,
        spike,
        trend_suppressed,
        weight_lb,
    );
    let (feedback, win_lift) = feedback.map_or((None, None), |f| (Some(f.view), f.win_lift));
    FeedbackSection {
        feedback,
        win_lift,
        trend,
    }
}

/// The logged history: lift and run results, run progress and the load and
/// activity views built on them.
struct HistorySection {
    lifts: Vec<LiftResultView>,
    runs: Vec<RunResultView>,
    run_progress: Option<RunProgressView>,
    training_load: Option<TrainingLoadView>,
    srpe_load: Option<SrpeLoadView>,
    provisional: Option<AdjustmentView>,
    activities: Vec<ActivityResultView>,
    youth_activity: Option<YouthActivityView>,
}

/// The run history views in chronological (observed_at) order, stable for
/// ties and undated (0) legacy entries, so a backdated log slots into its true
/// position and the shells' oldest→newest rendering follows log TIME, not
/// submission order; plus the run progress layered on them, its target marker
/// at `planned_run_km`. Every run consumer reads `cx.runs`, so each track is
/// QC'd at most once per view.
fn history_section(cx: &ViewCtx, planned_run_km: f64, weight_lb: bool) -> HistorySection {
    let model = cx.model;
    let mut runs = run_views(&model.runs, &cx.runs, model.distance_unit());
    // Run progress and the cross-run best-effort records are layered on the
    // memoized per-run views.
    let progress = build_run_progress(model, &cx.runs, planned_run_km);
    annotate_run_views(&mut runs, progress.as_ref().map(|p| &p.view));
    crate::memo::retain_runs(&cx.runs.keys);
    HistorySection {
        lifts: lift_views(&model.sets, cx.sets_fp, weight_lb),
        runs,
        training_load: build_training_load(model, &cx.runs),
        srpe_load: progress.as_ref().and_then(build_srpe_load),
        run_progress: progress.map(|p| p.view),
        provisional: build_provisional(model, &cx.runs),
        activities: activity_views(&model.activities),
        youth_activity: build_youth_activity(cx),
    }
}

/// The prescriptive guidance rows: the profile's guidance, the reference
/// card, the weekly running report, the lift audit and the calisthenics
/// ladders.
struct GuidanceSection {
    guidance: Vec<GuidanceView>,
    profile_guidance: Vec<GuidanceView>,
    reference: std::borrow::Cow<'static, [GuidanceView]>,
    weekly_report: Vec<GuidanceView>,
    lift_audit: Vec<GuidanceView>,
    calisthenics_ladders: Vec<CalisLadderView>,
}

fn guidance_section(
    cx: &ViewCtx,
    calc_hold: Option<&AdjustmentCall>,
    reds_present: bool,
    week_plan: &[SessionPlanView],
) -> GuidanceSection {
    let model = cx.model;
    let all = model
        .profile()
        .map(|p| {
            // Feed the km-gated safety/hybrid guards the athlete's real
            // recent volume, not just the stated setup figure.
            let recent_weekly = model
                .today_epoch_day
                .and_then(|today| cx.run_anchors_on(today).recent_weekly_km);
            let (weekly_km, basis) = stated_or_measured_weekly_km(p, recent_weekly);
            build_guidance_measured(p, weekly_km, basis, calc_hold)
        })
        .unwrap_or_default();
    let (profile_guidance, guidance) = all
        .into_iter()
        .partition(|g: &GuidanceView| g.section == PROFILE_SECTION);
    let mut lift_audit = build_lift_audit(cx);
    lift_audit.extend(build_calisthenics_guidance(cx));
    GuidanceSection {
        guidance,
        profile_guidance,
        reference: build_reference(reds_present),
        weekly_report: build_weekly_report(cx, week_plan),
        lift_audit,
        calisthenics_ladders: build_calisthenics_ladders(cx),
    }
}

/// Today's readiness picture: the check-in state, the signal rows, where the
/// autoregulation reads from, and the baselines still collecting.
struct ReadinessSection {
    checked_in_today: bool,
    weekly_checkin_present: bool,
    input_count: usize,
    readiness_summary: Vec<ReadinessSignalView>,
    autoreg_source: Option<AdjustmentView>,
    signal_groups: std::borrow::Cow<'static, [SignalGroupView]>,
    latest_checkin: Option<CheckinInput>,
    baseline_status: Vec<BaselineStatusView>,
}

fn readiness_section(model: &Model, v: &Verdicts, hrv_streak_derived: bool) -> ReadinessSection {
    let derived = &v.picture.derived;
    ReadinessSection {
        checked_in_today: model.today_epoch_day.is_some_and(|t| {
            model.checkins.iter().any(|c| {
                !is_undated(c.observed_at)
                    && local_day(c.observed_at, model.today_utc_offset_sec) == t
            })
        }),
        weekly_checkin_present: model.weekly_checkin().is_some(),
        input_count: model.inputs.len(),
        readiness_summary: v.picture.signal_rows(),
        autoreg_source: build_autoreg_source(
            v.readiness_inputs,
            hrv_streak_derived,
            derived
                .collecting
                .iter()
                .any(|b| b.kind == autoreg::BaselineKind::Suspended),
            model.today_utc_offset_sec,
        ),
        signal_groups: build_signal_groups(),
        latest_checkin: model.checkins.iter().max_by_key(|c| c.observed_at).cloned(),
        baseline_status: derived
            .collecting
            .iter()
            .map(to_baseline_status_view)
            .collect(),
    }
}

/// The safety banner: the highest tier, why training is held and how the
/// hold resolves, the illness check prompt, and why no plan is shown.
struct SafetySection {
    safety_tier: Option<SafetyTier>,
    hold_reason: Option<HoldReason>,
    hold_resolutions: Vec<HoldResolutionView>,
    illness_check_prompt: bool,
    plan_withheld: Option<PlanWithheld>,
}

fn safety_section(model: &Model, v: &Verdicts) -> SafetySection {
    let readiness_inputs = v.readiness_inputs;
    SafetySection {
        safety_tier: v.safety.safety_tier,
        hold_reason: hold_reason(v.safety.hold.as_ref(), model.profile().map(|p| &p.health)),
        hold_resolutions: hold_resolutions(
            readiness_inputs,
            model.profile().is_some_and(|p| p.health.acute_illness),
        ),
        illness_check_prompt: autoreg::illness_active(readiness_inputs)
            || autoreg::rhr_neck_check_due(readiness_inputs, &v.picture.rule_ctx()),
        plan_withheld: model
            .today_epoch_day
            .and_then(|d| profile_for_day(model, d))
            .as_deref()
            .or(model.profile())
            .and_then(|p| PlanProfile::new(p).withheld()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
