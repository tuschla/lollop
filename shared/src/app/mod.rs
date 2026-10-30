//! The side-effect-free core (crux `App`). `update` folds each event into the
//! model (sanitizing every wire value on the way in); `view()` runs the pure
//! autoregulation layer (`crate::autoreg`), the plan synthesis and every
//! evidence-cited builder over it. No IO, no clock, no randomness.

use crux_core::{
    App, Command,
    macros::effect,
    render::{RenderOperation, render},
};
use serde::{Deserialize, Serialize};

use crate::evidence::{ClaimId, claims, graded};
use crate::feedback::FeedbackCategory;
use crate::hybrid::ConcurrentGoal;
use crate::individualization::{Environment, ProgressionCadence};
use crate::running::{GoalDistance, GpsPoint};
use crate::schema::{
    Adjustment, CheckinInput, EvidenceGrade, Goal, HealthScreen, LiftIntensity, LiftSessionType,
    MesoPhase, Mesocycle, Prescription, Program, ReadinessInput, ReadinessSignal, Recommended,
    RunIntensity, RunSessionType, RunVolume, SafetyTier, Session, SessionType, ThreeZone, VdotBand,
    WorkoutType,
};
use crate::strength::LiftGoal;
use crate::{DAY_SEC, is_undated, local_day};
use crate::{autoreg, feedback, hybrid, hypertrophy, individualization, load, running, strength};

// State, wire types and builders live in submodules, glob-imported here.
mod anchors;
mod calculators;
mod evidence_views;
mod format;
mod guidance;
mod ingest;
mod lift_audit;
mod memo;
mod model;
mod plan_views;
mod readiness;
mod reviews;
mod run_metrics;
mod run_targets;
mod run_views;
mod view_ctx;
mod view_types;
mod youth_activity;

use anchors::*;
use calculators::*;
use evidence_views::*;
use format::*;
use guidance::*;
use ingest::*;
use lift_audit::*;
use memo::*;
pub use model::*;
use plan_views::*;
use readiness::*;
use reviews::*;
use run_metrics::*;
use run_views::*;
use view_ctx::*;
pub use view_types::*;
use youth_activity::*;

// `log.rs` normalizes replayed entry ids through this.
pub(crate) use ingest::sanitize_entry_id;
// `log.rs` compaction asks which store a legacy `SubmitReview` line replaces.
pub(crate) use reviews::{submit_review_sets_session, submit_review_sets_weekly};

// `ffi.rs` drives these across the JNI boundary as `crate::app::export_run_gpx`
// / `crate::app::preview_track_figures`; like the ffi glue they feed, they are
// compiled only where the exports exist.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(crate) use run_views::{export_run_gpx, preview_track_figures};

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
        // Every wire float is clamped by `sanitize_f64` before it is stored; see
        // its doc.
        match event {
            Event::SubmitReadiness(mut input) => {
                input.value = sanitize_f64(input.value);
                input.effort_min = input.effort_min.map(sanitize_f64);
                model.inputs.push(input);
            }
            Event::ClearReadiness => {
                model.inputs.clear();
                model.retracted_inputs.clear();
            }
            Event::RemoveReadiness {
                signal,
                observed_at,
            } => {
                let pos = if signal == ReadinessSignal::Pain {
                    autoreg::worst_active_pain(&model.inputs)
                        .and_then(|w| model.inputs.iter().position(|i| std::ptr::eq(i, w)))
                        .or_else(|| model.inputs.iter().rposition(|i| i.signal == signal))
                } else {
                    model.inputs.iter().rposition(|i| i.signal == signal)
                };
                if let Some(pos) = pos {
                    let input = model.inputs.remove(pos);
                    if !is_undated(observed_at) {
                        model.retracted_inputs.push(RetractedInput {
                            input,
                            retracted_at: observed_at,
                        });
                    }
                }
            }
            Event::ResolveHold {
                signal,
                observed_at,
            } => {
                if !is_undated(observed_at) {
                    let (resolved, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut model.inputs)
                        .into_iter()
                        .partition(|i| i.signal == signal);
                    model.inputs = kept;
                    model
                        .retracted_inputs
                        .extend(resolved.into_iter().map(|input| RetractedInput {
                            input,
                            retracted_at: observed_at,
                        }));
                }
            }
            Event::SubmitCheckin(mut input) => {
                for v in [&mut input.resting_hr_bpm, &mut input.hrv_rmssd_ms]
                    .into_iter()
                    .flatten()
                {
                    *v = sanitize_f64(*v);
                }
                model.checkins.push(input);
            }
            Event::ClearCheckins => {
                model.checkins.clear();
                model.hrv_baseline_from = None;
            }
            Event::RestartHrvBaseline { observed_at } => {
                // Latest restart wins (never moves the cutoff backwards).
                if !is_undated(observed_at) {
                    model.hrv_baseline_from = Some(
                        model
                            .hrv_baseline_from
                            .map_or(observed_at, |t| t.max(observed_at)),
                    );
                }
            }
            Event::LogSet {
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
                entry_id,
                hold_sec,
            } => {
                let fields = SetFields {
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    observed_at,
                    entry_id,
                    hold_sec,
                };
                if let Some(set) = fields.into_logged() {
                    model.sets.push(set);
                }
            }
            Event::ClearSets => {
                model.sets.clear();
                // The ended markers belong to the SETS family: clearing the
                // sets clears the "ended early" state they qualified.
                model.ended_exercises.clear();
                // Rung placements steer the ladders those sets build, so they
                // share the SETS family lifecycle.
                model.calis_placements.clear();
            }
            Event::SetExerciseEnded {
                exercise,
                epoch_day,
                ended,
                observed_at,
            } => {
                model.ended_exercises.retain(|m| {
                    !(m.epoch_day == epoch_day
                        && crate::plan::same_movement(&m.exercise, &exercise))
                });
                model.ended_exercises.push(EndedMarker {
                    exercise,
                    epoch_day,
                    ended,
                    observed_at,
                });
            }
            Event::SetCalisthenicsRung {
                ladder,
                rung,
                observed_at,
            } => {
                // An out-of-range rung is a no-op (like other rejected inputs).
                if crate::calisthenics::tier(ladder, rung).is_some() {
                    model.calis_placements.push(CalisPlacement {
                        ladder,
                        rung,
                        observed_at,
                    });
                }
            }
            Event::LogRun {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id,
                workout_type,
                session_rpe,
                avg_hr_bpm,
                utc_offset_sec,
            } => {
                let run = logged_run(RunFields {
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    longest_recent_km,
                    observed_at,
                    entry_id,
                    workout_type,
                    session_rpe,
                    avg_hr_bpm,
                    utc_offset_sec,
                });
                store_run(model, run, None);
            }
            Event::LogRunTrack {
                points,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id,
                workout_type,
                session_rpe,
                segment_starts,
                avg_hr_bpm,
                utc_offset_sec,
            } => {
                // Distance and duration derive from the track at `view()`.
                let mut run = logged_run(RunFields {
                    distance_km: 0.0,
                    duration_min: 0.0,
                    hr_pct_max,
                    longest_recent_km,
                    observed_at,
                    entry_id,
                    workout_type,
                    session_rpe,
                    avg_hr_bpm,
                    utc_offset_sec,
                });
                run.track = sanitize_track(points);
                run.track_segment_starts = segment_starts;
                store_run(model, run, None);
            }
            Event::ClearRuns => model.runs.clear(),
            Event::LogActivity {
                kind,
                minutes,
                vigorous,
                strengthening,
                observed_at,
                entry_id,
            } => {
                if activity_passes_gate(minutes) {
                    model.activities.push(LoggedActivity {
                        kind,
                        minutes,
                        vigorous,
                        strengthening,
                        observed_at,
                        entry_id: sanitize_entry_id(entry_id),
                    });
                }
            }
            Event::ClearActivities => model.activities.clear(),
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_entry(&model.sets, entry_id, observed_at_fallback) {
                    model.sets.remove(pos);
                }
            }
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_entry(&model.runs, entry_id, observed_at_fallback) {
                    let gone = model.runs.remove(pos);
                    refresh_spike_baselines(&mut model.runs, &[gone.observed_at]);
                }
            }
            Event::DeleteEntry {
                kind: EntryKind::Activity,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_entry(&model.activities, entry_id, observed_at_fallback) {
                    model.activities.remove(pos);
                }
            }
            Event::AmendSet {
                entry_id,
                exercise,
                weight_kg,
                reps,
                rpe,
                observed_at,
                observed_at_fallback,
                hold_sec,
            } => {
                // A STRICT update: the replacement is pushed ONLY when a row
                // matching the OLD identity is found, so an amend whose target
                // was already deleted is a no-op and a Log→Delete→Amend sequence
                // cannot resurrect the deleted row. Compaction Rule 3 mirrors
                // this: a surviving amend always keeps its base log line, so a
                // compacted edit still replays. A set the gate rejects leaves the
                // existing row untouched.
                let match_at = amend_match_at(observed_at_fallback, observed_at);
                let fields = SetFields {
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    observed_at,
                    entry_id,
                    hold_sec,
                };
                if let Some(set) = fields.into_logged()
                    && let Some(pos) = find_entry(&model.sets, set.entry_id, match_at)
                {
                    model.sets.remove(pos);
                    model.sets.push(set);
                }
            }
            Event::AmendRun {
                entry_id,
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                observed_at_fallback,
                workout_type,
                session_rpe,
                avg_hr_bpm,
            } => {
                // A STRICT update matched on the OLD identity, like `AmendSet`.
                // The replacement is a fresh manual entry; the spike baselines
                // around both its old and its new date are re-derived.
                let match_at = amend_match_at(observed_at_fallback, observed_at);
                if let Some(pos) = find_entry(&model.runs, entry_id, match_at) {
                    let old = model.runs.remove(pos);
                    let run = logged_run(RunFields {
                        distance_km,
                        duration_min,
                        hr_pct_max,
                        longest_recent_km,
                        observed_at,
                        entry_id,
                        workout_type,
                        session_rpe,
                        avg_hr_bpm,
                        utc_offset_sec: None,
                    });
                    store_run(model, run, Some(&old));
                }
            }
            Event::TrimRunTrack {
                entry_id,
                observed_at_fallback,
                keep_from_sec,
                keep_until_sec,
            } => {
                // An inverted range, a no-match or a slice that is not a real
                // trim (`trim_track`) leaves the run untouched.
                if keep_from_sec <= keep_until_sec
                    && let Some(pos) = find_entry(&model.runs, entry_id, observed_at_fallback)
                    && let Some((track, starts)) =
                        trim_track(&model.runs[pos], keep_from_sec, keep_until_sec)
                {
                    let run = &mut model.runs[pos];
                    run.track = track;
                    run.track_segment_starts = starts;
                    let at = run.observed_at;
                    refresh_spike_baselines(&mut model.runs, &[at]);
                }
            }
            Event::AmendRunMeta {
                entry_id,
                observed_at_fallback,
                session_rpe,
                workout_type,
            } => {
                if let Some(pos) = find_entry(&model.runs, entry_id, observed_at_fallback) {
                    let run = &mut model.runs[pos];
                    run.session_rpe = sanitize_session_rpe(session_rpe);
                    run.workout_type = workout_type;
                }
            }
            Event::SetProfile(profile) => {
                apply_profile(model, profile);
                refresh_run_hr(model);
            }
            Event::ClearProfile => {
                model.profile_history.clear();
                // Every run falls back to the empty history, including the
                // ones a legacy undated edit pinned (as a compacted log, which
                // drops that edit with the history, replays them).
                for run in &mut model.runs {
                    run.hr_pinned = false;
                }
                refresh_run_hr(model);
            }
            Event::SubmitReview(mut review) => {
                sanitize_review(&mut review);
                // A payload's weekly/clinical part goes to the
                // weekly check-in, its execution part to the session review,
                // so a routine session review never wipes a referral, the
                // compulsive-training flag or an in-force deload.
                if submit_review_sets_weekly(&review) {
                    store_weekly(model, &review);
                }
                if submit_review_sets_session(&review) {
                    model.review = Some(session_part(&review));
                }
            }
            Event::ClearReview => model.review = None,
            Event::SubmitWeeklyCheckin(mut review) => {
                sanitize_review(&mut review);
                store_weekly(model, &review);
            }
            Event::ClearWeeklyCheckin => model.weekly_checkin = None,
            Event::PredictRace {
                recent_distance_m,
                recent_time_sec,
                goal_distance_m,
                weekly_km,
                weeks_since_race,
            } => {
                model.race_query = Some(RaceQuery {
                    recent_distance_m: sanitize_f64(recent_distance_m),
                    recent_time_sec: sanitize_f64(recent_time_sec),
                    goal_distance_m: sanitize_f64(goal_distance_m),
                    weekly_km: sanitize_f64(weekly_km),
                    weeks_since_race,
                });
            }
            Event::ClearRacePrediction => model.race_query = None,
            Event::PlanHypertrophyMeso {
                muscle,
                weeks,
                not_growing,
                recovering_easily,
            } => {
                model.hypertrophy_plan_query = Some(HypertrophyInputView {
                    muscle,
                    weeks,
                    not_growing,
                    recovering_easily,
                });
            }
            Event::ClearHypertrophyPlan => model.hypertrophy_plan_query = None,
            Event::ComputeProtein {
                bodyweight_kg,
                masters,
                deficit,
            } => {
                model.protein_query = Some(ProteinInputView {
                    bodyweight_kg: sanitize_f64(bodyweight_kg),
                    masters,
                    deficit,
                });
            }
            Event::ClearProtein => model.protein_query = None,
            Event::ComputeHrZones {
                age_years,
                resting_hr_bpm,
            } => {
                model.hr_zone_query = Some(HrZoneInputView {
                    age_years: sanitize_f64(age_years),
                    resting_hr_bpm: resting_hr_bpm.map(sanitize_f64),
                });
            }
            Event::ClearHrZones => model.hr_zone_query = None,
            Event::GeneratePlan { start_epoch_day } => {
                // Re-firing GeneratePlan (the shell sends it every launch) keeps
                // the original start; only ClearPlan re-anchors.
                let anchor = model
                    .plan_request
                    .as_ref()
                    .map_or(start_epoch_day, |p| p.start_epoch_day);
                model.plan_request = Some(PlanRequest {
                    start_epoch_day: anchor,
                });
            }
            Event::ClearPlan => {
                if let Some(req) = model.plan_request.take() {
                    let start = req.start_epoch_day;
                    let end = model.today_epoch_day.unwrap_or(start);
                    model.past_plans.push((start, end));
                }
                model.session_moves.clear();
            }
            // Stored as asked; `view()` validates each move against the week's
            // spacing rules (deterministic, replay-safe: validity never depends
            // on when the event was replayed).
            Event::MoveSession {
                from_epoch_day,
                to_epoch_day,
            } => model.session_moves.push(SessionMove {
                from_epoch_day,
                to_epoch_day,
            }),
            Event::SetToday {
                epoch_day,
                utc_offset_sec,
            } => {
                model.today_epoch_day = Some(epoch_day);
                // A legacy run row (no logged offset) reads the current
                // offset; a stamped one keeps its own, so only those move.
                let offset_changed = model.today_utc_offset_sec != utc_offset_sec;
                model.today_utc_offset_sec = utc_offset_sec;
                ingest::refresh_birthday_ages(model);
                if offset_changed {
                    refresh_run_hr(model);
                }
            }
        }
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
        let readiness_inputs = &picture.inputs;
        let rule_ctx = picture.rule_ctx();
        let review = review_pipeline(model, picture.adjustments(model, &cx.days));
        let safety = safety_state(
            model,
            readiness_inputs,
            autoreg::resolve_safety_in(readiness_inputs, &rule_ctx),
            &review,
        );
        let v = Verdicts {
            readiness_inputs,
            rule_ctx,
            review,
            safety,
        };
        let rule_ctx = &v.rule_ctx;
        let recommended = &v.review.recommended;
        let review_recs = &v.review.review_recs;
        let train_blocked = v.safety.hold.is_some();
        let reds_present = v.safety.reds_present;

        let (derived_hrv_streak, derived_wellness_streak) = derived_streaks(derived);
        let review_adjustments =
            merged_review(model.weekly_checkin.as_ref(), model.review.as_ref())
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

        let mut adjustments: Vec<AdjustmentView> = recommended.iter().map(to_view).collect();
        // Several lift-holding signals apply once, so they read as one call.
        let merged_away = merge_hold_rows(recommended, &mut adjustments);
        let PlanSection {
            next_session,
            week_plan,
            next_week_plan,
            program,
            planned_run_km,
        } = plan_section(&cx, &v, &mut adjustments, &merged_away, weight_lb);

        let FeedbackSection { feedback, spike } = feedback_section(&cx, &v, &week_plan, weight_lb);
        let session_done = build_session_done(&week_plan, model.today_epoch_day);
        // feedback-039/041/042: no trend praise or causal story while a concern
        // card leads or training is blocked.
        let trend_suppressed =
            train_blocked || feedback.as_ref().is_some_and(|f| f.suppresses_praise);

        // One hold for Today, the plan calculators and the prescriptive
        // guidance: today's training hold, else a RED-S signal.
        let calc_hold = calculator_hold(v.safety.hold.as_ref(), reds_present);
        let CalcSection {
            protein_targets,
            protein_figures,
            hr_zones,
            hr_max,
            hr_zone_bands,
            hypertrophy_plan,
            hypertrophy_figures,
            hypertrophy_ramp,
        } = calc_section(model, calc_hold.as_ref(), reds_present, weight_lb);
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

        let (runs, run_progress) = history_section(&cx, planned_run_km);

        // The readiness strip surfaces recency-expired RHR/wellness readings as an
        // "out of date" row, so it is fed the stale inputs too; the rules
        // above ran only on the fresh `readiness_inputs`.
        let mut display_inputs = readiness_inputs.clone();
        display_inputs.extend(derived.stale.iter().cloned());

        ViewModel {
            safety_tier: v.safety.safety_tier,
            hold_reason: hold_reason(v.safety.hold.as_ref(), model.profile().map(|p| &p.health)),
            checked_in_today: model.today_epoch_day.is_some_and(|t| {
                model.checkins.iter().any(|c| {
                    !is_undated(c.observed_at)
                        && local_day(c.observed_at, model.today_utc_offset_sec) == t
                })
            }),
            adjustments,
            review_adjustments,
            weekly_checkin_present: model.weekly_checkin.is_some(),
            input_count: model.inputs.len(),
            lifts: lift_views(&model.sets, cx.sets_fp, weight_lb),
            runs,
            guidance: model
                .profile()
                .map(|p| {
                    // Feed the km-gated safety/hybrid guards the athlete's real
                    // recent volume, not just the stated setup figure.
                    let recent_weekly = model
                        .today_epoch_day
                        .and_then(|today| cx.run_anchors_on(today).recent_weekly_km);
                    let (weekly_km, basis) = effective_weekly_km(p, recent_weekly);
                    build_guidance_measured(p, weekly_km, basis, calc_hold.as_ref())
                })
                .unwrap_or_default(),
            feedback,
            reference: build_reference(reds_present),
            profile: model.profile().cloned(),
            race_prediction: model
                .race_query
                .as_ref()
                .map(|q| to_race_view(q, longest_logged_run_km(&cx.runs))),
            hypertrophy_plan,
            protein_targets,
            hr_zones,
            training_load: build_training_load(model, &cx.runs),
            srpe_load: run_progress.as_ref().and_then(build_srpe_load),
            weekly_report: build_weekly_report(&cx, &week_plan),
            run_progress,
            lift_audit: {
                let mut a = build_lift_audit(&cx);
                a.extend(build_calisthenics_guidance(&cx));
                a
            },
            hr_zone_input: model.hr_zone_query.clone(),
            protein_input: model.protein_query.clone(),
            hypertrophy_input: model.hypertrophy_plan_query.clone(),
            hypertrophy_ramp,
            trend: build_trend(
                model,
                &cx.set_index,
                model.weekly_checkin.as_ref(),
                derived_wellness_streak,
                spike,
                trend_suppressed,
                weight_lb,
            ),
            provisional: build_provisional(model, &cx.runs),
            autoreg_source: build_autoreg_source(
                readiness_inputs,
                derived_hrv_streak.is_some(),
                derived
                    .collecting
                    .iter()
                    .any(|b| b.kind == autoreg::BaselineKind::Suspended),
                model.today_utc_offset_sec,
            ),
            readiness_summary: build_readiness_summary(&display_inputs, rule_ctx, &derived.stale),
            today_headline,
            signal_groups: build_signal_groups(),
            latest_checkin: model.checkins.iter().max_by_key(|c| c.observed_at).cloned(),
            baseline_status: derived
                .collecting
                .iter()
                .map(to_baseline_status_view)
                .collect(),
            grade_definitions: grade_definitions(),
            // ADH-SELFMON-001: planned vs done, built off the week strip before
            // it moves into the view.
            consistency: build_consistency(model, &cx.days, &week_plan, train_blocked),
            next_session,
            week_plan,
            next_week_plan,
            program,
            hr_max,
            protein_figures,
            hr_zone_bands,
            hypertrophy_figures,
            today_epoch_day: model.today_epoch_day,
            session_done,
            calisthenics_ladders: build_calisthenics_ladders(&cx),
            hold_resolutions: hold_resolutions(
                readiness_inputs,
                model.profile().is_some_and(|p| p.health.acute_illness),
            ),
            illness_check_prompt: autoreg::illness_active(readiness_inputs)
                || autoreg::rhr_neck_check_due(readiness_inputs, rule_ctx),
            plan_withheld: model
                .today_epoch_day
                .and_then(|d| profile_for_day(model, d))
                .as_deref()
                .or(model.profile())
                .and_then(crate::plan::plan_withheld),
            activities: activity_views(&model.activities),
            youth_activity: build_youth_activity(&cx),
        }
    }
}

/// What today's readiness rules, the reviews and the onboarding screen
/// decided: the gates every view section after them reads.
struct Verdicts<'a> {
    /// Today's fresh readiness inputs (recency-expired readings excluded).
    readiness_inputs: &'a [ReadinessInput],
    rule_ctx: autoreg::RuleContext<'a>,
    review: ReviewPipeline,
    safety: SafetyState,
}

/// The review-carried adjustments, with today's readiness recommendations
/// reconciled against them.
struct ReviewPipeline {
    /// Age in days of the session review (`None`: none, undated, or no clock).
    review_age: Option<i64>,
    /// The weekly check-in's week-level deloads and deferrals still in force.
    review_recs: Vec<Recommended<Adjustment>>,
    /// Today's readiness recommendations, minus any load increase a
    /// protective review rec overrides.
    recommended: Vec<Recommended<Adjustment>>,
}

/// The weekly check-in carries the week-level deloads and the clinical
/// screens; the session review only one session's execution. Each ages on its
/// own clock.
fn review_pipeline(model: &Model, recommended: Vec<Recommended<Adjustment>>) -> ReviewPipeline {
    let offset = model.today_utc_offset_sec;
    let weekly_age = model
        .weekly_checkin
        .as_ref()
        .and_then(|r| review_age_days(r, model.today_epoch_day, offset));
    let review_age = model
        .review
        .as_ref()
        .and_then(|r| review_age_days(r, model.today_epoch_day, offset));
    let review_recs: Vec<Recommended<Adjustment>> = model
        .weekly_checkin
        .as_ref()
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
    gates: Vec<Recommended<Adjustment>>,
    /// Today's training hold: the first blocking call (Stop / RestDay /
    /// Defer) of the profile gates, then readiness, then the weekly check-in.
    /// The single source for Today's block, [`hold_reason`], the plan
    /// calculators and %1RM guidance ([`calculator_hold`]).
    hold: Option<Recommended<Adjustment>>,
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
        || model
            .weekly_checkin
            .as_ref()
            .is_some_and(weekly_reds_signal)
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

/// The dated week with today's session as the coach prescribes it, and next
/// week's days.
struct PlanSection {
    next_session: Option<SessionPlanView>,
    week_plan: Vec<SessionPlanView>,
    next_week_plan: Vec<SessionPlanView>,
    program: Option<ProgramSummaryView>,
    /// The run km this plan week prescribes (0 without a plan).
    planned_run_km: f64,
}

/// Coach-as-planner: synthesize + date the week, then apply readiness/safety
/// INSIDE the rendered session (strictly downstream of the gates; HARD RULE
/// 3). The prescription becomes the top non-safety headline rung. Settles the
/// readiness `adjustments` rows against what today's session already says;
/// `merged_away` marks the rows [`merge_hold_rows`] folded into another.
fn plan_section(
    cx: &ViewCtx,
    v: &Verdicts,
    adjustments: &mut Vec<AdjustmentView>,
    merged_away: &[bool],
    weight_lb: bool,
) -> PlanSection {
    let model = cx.model;
    let today = model.today_epoch_day;
    let recommended = &v.review.recommended;
    let review_recs = &v.review.review_recs;
    let train_blocked = v.safety.hold.is_some();
    let (mut next_session, mut week_plan, mut next_week_plan, program, planned_run_km) =
        build_plan_views(cx, train_blocked, recommended, review_recs);
    let wordings = [
        (
            hrv_single_morning(v.readiness_inputs),
            hrv_morning_wording as fn(&mut AdjustmentView),
        ),
        (v.rule_ctx.wellness_collecting, checkin_floor_wording),
    ];
    for (_, wording) in wordings.into_iter().filter(|(applies, _)| *applies) {
        adjustments.iter_mut().for_each(wording);
        for s in next_session.iter_mut().chain(week_plan.iter_mut()) {
            if Some(s.epoch_day) == today
                && let Some(a) = s.adjustment.as_mut()
            {
                wording(a);
            }
        }
    }
    let (fresh, fresh_rows) = if train_blocked {
        Default::default()
    } else {
        apply_first_set_cues(
            v,
            today,
            &mut next_session,
            &mut week_plan,
            adjustments,
            weight_lb,
        )
    };
    // Every fold, hold and cue has moved its numbers: the why? basis restates
    // the planned prescription against today's, so the session panel reads
    // the face.
    for it in next_session
        .iter_mut()
        .chain(week_plan.iter_mut())
        .chain(next_week_plan.iter_mut())
        .flat_map(|s| s.items.iter_mut())
    {
        restate_basis(it, weight_lb);
    }
    // Say what changed on each lift since last time, and why.
    if let Some(ns) = next_session.as_mut() {
        let now_anchors = cx.lift_anchors_now();
        annotate_last_time(ns, model, &cx.set_index, now_anchors, weight_lb);
        if let Some(row) = week_plan
            .iter_mut()
            .chain(next_week_plan.iter_mut())
            .find(|s| s.epoch_day == ns.epoch_day)
        {
            annotate_last_time(row, model, &cx.set_index, now_anchors, weight_lb);
        }
    }
    // Today's session: the hero while it is today's, else today's row (a
    // finished session's hero has moved on to the next training day).
    let today_session = next_session
        .iter()
        .chain(&week_plan)
        .find(|s| today == Some(s.epoch_day));
    // A tendon modify call names the lifts it eases in today's session.
    if let Some(ts) = today_session {
        for (row, r) in adjustments.iter_mut().zip(recommended) {
            name_modified_exercises(&mut row.summary, r, &ts.items);
        }
    }
    // One cue per lift: today's in-session add replaces the "next time" add
    // for the same lift; a "remaining sets" call on an exercise already
    // finished today has nothing left to change.
    let mut drop_row = recommended.iter().zip(merged_away).map(|(r, merged)| {
        *merged
            || next_time_add_superseded(r, &fresh)
            || today_session.is_some_and(|ts| remaining_sets_call_spent(r, ts))
    });
    adjustments.retain(|_| !drop_row.next().unwrap_or(false));
    adjustments.extend(fresh_rows);
    // The "log your first work set" clause is spent once one is logged, on
    // the readiness rows and on the session's own hold line.
    if today_session.is_some_and(|ts| ts.items.iter().any(|it| !it.is_run() && it.has_work())) {
        let session_rows = next_session
            .iter_mut()
            .chain(week_plan.iter_mut())
            .filter(|s| Some(s.epoch_day) == today)
            .filter_map(|s| s.adjustment.as_mut());
        for row in adjustments.iter_mut().chain(session_rows) {
            if let Some(s) = row.summary.strip_suffix(FIRST_SET_CLAUSE) {
                row.summary = s.to_string();
            }
        }
    }
    PlanSection {
        next_session,
        week_plan,
        next_week_plan,
        program,
        planned_run_km,
    }
}

/// Today's logged first work set of each lift, against that item's target,
/// scopes a load cut (or an opt-in add) to THAT lift's remaining sets; a
/// hard first set on a held day becomes the one-level downgrade by
/// composition. Returns the new calls (not among the readiness
/// recommendations) and their rows.
fn apply_first_set_cues(
    v: &Verdicts,
    today: Option<i64>,
    next_session: &mut Option<SessionPlanView>,
    week_plan: &mut [SessionPlanView],
    adjustments: &mut [AdjustmentView],
    weight_lb: bool,
) -> (Vec<Recommended<Adjustment>>, Vec<AdjustmentView>) {
    let recommended = &v.review.recommended;
    let review_recs = &v.review.review_recs;
    let no_increase = autoreg::increase_hold(v.readiness_inputs, &v.rule_ctx)
        || suppression_active(recommended, false)
        || suppression_active(review_recs, false);
    // Today's row honours the in-session cue loads too (a lift finished at
    // the "remaining sets drop 5%" load is done, not lighter or open), so the
    // strip and the Session Done card read what the hero reads.
    if let Some(row) = week_plan.iter_mut().find(|s| {
        today == Some(s.epoch_day)
            && matches!(
                s.status,
                SessionStatus::Planned | SessionStatus::Next | SessionStatus::DoneLighter
            )
    }) {
        let (status, adjustment) = (row.status, row.adjustment.take());
        apply_first_set_autoreg(row, recommended, review_recs, no_increase, weight_lb);
        row.adjustment = adjustment;
        row.status = finished_status(&row.items).unwrap_or(status);
    }
    let Some(ns) = next_session
        .as_mut()
        .filter(|ns| today == Some(ns.epoch_day))
    else {
        return Default::default();
    };
    let cues = apply_first_set_autoreg(ns, recommended, review_recs, no_increase, weight_lb);
    if cues.is_empty() {
        return Default::default();
    }
    // A matched readiness rec keeps its row (rows 0..recommended.len() are
    // `recommended`, in order), now carrying the cue's load.
    for c in cues.iter().filter(|c| !c.fresh) {
        if let Some(k) = recommended.iter().position(|r| *r == c.rec) {
            adjustments[k].suggested_load_kg = c.load_kg;
        }
    }
    if let Some(row) = week_plan.iter_mut().find(|s| s.epoch_day == ns.epoch_day) {
        *row = ns.clone();
    }
    cues.into_iter()
        .filter(|c| c.fresh)
        .map(|c| {
            let mut row = to_view(&c.rec);
            row.suggested_load_kg = c.load_kg;
            (c.rec, row)
        })
        .unzip()
}

/// The calculators' prose rows and the structured figures beside them, so a
/// shell reads numbers instead of scraping the summary strings.
struct CalcSection {
    protein_targets: Vec<GuidanceView>,
    protein_figures: Vec<ProteinFigureView>,
    hr_zones: Vec<GuidanceView>,
    hr_max: Option<HrMaxView>,
    hr_zone_bands: Vec<HrZoneBandView>,
    hypertrophy_plan: Vec<GuidanceView>,
    hypertrophy_figures: Option<HypertrophyFiguresView>,
    hypertrophy_ramp: Vec<u8>,
}

fn calc_section(
    model: &Model,
    calc_hold: Option<&Recommended<Adjustment>>,
    reds_present: bool,
    weight_lb: bool,
) -> CalcSection {
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
        .map(|q| build_hypertrophy_plan(q, model.profile(), calc_hold))
        .unwrap_or_default();
    CalcSection {
        protein_targets,
        protein_figures,
        hr_zones,
        hr_max,
        hr_zone_bands,
        hypertrophy_plan,
        hypertrophy_figures,
        hypertrophy_ramp,
    }
}

/// The coaching takeaway and the latest run's spike it (and the trend) read.
struct FeedbackSection {
    feedback: Option<FeedbackView>,
    spike: Option<RunSpike>,
}

/// One coaching takeaway from the weekly check-in's safety signals, the
/// latest run's spike, the session review, the latest completed plan day
/// judged from the logs (no form needed) and any new best.
fn feedback_section(
    cx: &ViewCtx,
    v: &Verdicts,
    week_plan: &[SessionPlanView],
    weight_lb: bool,
) -> FeedbackSection {
    let model = cx.model;
    // Feedback personalization inputs (feedback-023/024/035).
    let advanced_user = model.profile().is_some_and(|p| {
        individualization::training_age_from_cadence(p.progression_cadence).value
            == individualization::TrainingAge::Advanced
    });
    let female_user = model.profile().is_some_and(|p| p.female.unwrap_or(false));
    let derived_exec = model
        .today_epoch_day
        .and_then(|t| derive_session_exec(week_plan, t));
    let mastery = detect_mastery(model, &cx.set_index, &cx.runs, week_plan);
    let spike = latest_run_spike(model, &cx.runs);
    let review_recs = &v.review.review_recs;
    let feedback = build_feedback(FeedbackCtx {
        cx,
        weekly: model.weekly_checkin.as_ref(),
        session: model.review.as_ref(),
        session_age: v.review.review_age,
        derived: derived_exec.as_ref(),
        today_session: model
            .today_epoch_day
            .and_then(|t| week_plan.iter().find(|s| s.epoch_day == t)),
        mastery: mastery.as_ref(),
        track_split: feedback_track_split(model, &cx.runs),
        spike,
        recovery_week_active: review_recs
            .iter()
            .any(|r| r.claim_id() == RECOVERY_WEEK_CLAIM),
        protective: suppression_active(&v.review.recommended, v.safety.hold.is_some())
            || suppression_active(review_recs, false),
        advanced_user,
        female_user,
        weight_lb,
    });
    FeedbackSection { feedback, spike }
}

/// The run history views in chronological (observed_at) order, stable for
/// ties and undated (0) legacy entries, so a backdated log slots into its true
/// position and the shells' oldest→newest rendering follows log TIME, not
/// submission order; plus the run progress layered on them, its target marker
/// at `planned_run_km`. Every run consumer reads `cx.runs`, so each track is
/// QC'd at most once per view.
fn history_section(
    cx: &ViewCtx,
    planned_run_km: f64,
) -> (Vec<RunResultView>, Option<RunProgressView>) {
    let model = cx.model;
    let mut runs = run_views(&model.runs, &cx.runs);
    // Run progress and the cross-run best-effort records are layered on the
    // memoized per-run views.
    let run_progress = build_run_progress(model, &cx.runs, planned_run_km);
    annotate_run_views(&mut runs, run_progress.as_ref());
    // Keep the on-demand GPX registry in step with the current run history
    // so `exportGpx(entry_id, observed_at)` can build any inspected run's GPX
    // without that ~0.5 MB string ever riding the ViewModel.
    #[cfg(any(target_os = "android", feature = "jni-host", test))]
    refresh_gpx_registry(&model.runs, &cx.runs);
    // Bound the per-run memos to the live history: a deleted run's entries
    // stop lingering past this view().
    prune_run_memos(&cx.runs.keys);
    (runs, run_progress)
}

#[cfg(test)]
pub(crate) mod tests;
