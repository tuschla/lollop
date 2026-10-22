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
use crate::feedback::{FeedbackCategory, TrendDirection};
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
use crate::{DAY_SEC, local_day};
use crate::{autoreg, feedback, hybrid, hypertrophy, individualization, load, running, strength};

// The reducer (`impl App for Engine`) lives here; state, wire types, and
// every view/plan/calculator builder live in sibling submodules. They are
// re-exported into this module's namespace so `update`/`view`, the sibling
// submodules (`use super::*`), and the `#[cfg(test)]` tests all reach them under
// their original `crate::app::*` paths.
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
pub(crate) use format::sanitize_entry_id;
// `log.rs` compaction asks which store a legacy `SubmitReview` line replaces.
pub(crate) use reviews::{submit_review_sets_session, submit_review_sets_weekly};

// `ffi.rs` drives this across the JNI boundary as `crate::app::export_run_gpx`;
// like the ffi glue it feeds, it is compiled only where the exports exist.
#[cfg(any(target_os = "android", feature = "jni-host", test))]
pub(crate) use run_views::export_run_gpx;

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
        match event {
            Event::SubmitReadiness(mut input) => {
                // Sanitize the wire floats BEFORE storing: a huge FINITE value
                // accumulates to `inf` in the per-day variance math, which
                // serializes as JSON `null` and breaks a non-null Kotlin decode.
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
                // For Pain, remove the report that DRIVES the hold (the worst
                // active one the gate and readiness row resolve to), so a single
                // "remove the pain report" tap actually lifts the training hold;
                // every other signal removes its newest reading.
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
                // "I'm better" / "I'm pain-free now": retract EVERY active report
                // of the signal as resolved (dated), keeping it for the past days
                // it shaped. An undated resolution is meaningless → no-op.
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
                if observed_at > 0 {
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
                // Last-write-wins per (same_movement-normalized exercise, day):
                // drop any prior marker for the same movement on this day, then
                // record the new state (`ended: false` reopens). Equipment-word
                // variants of the same movement collapse to one marker.
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
            } => {
                let run = logged_run(
                    &model.runs,
                    model.profile.as_ref(),
                    RunFields {
                        distance_km,
                        duration_min,
                        hr_pct_max,
                        longest_recent_km,
                        observed_at,
                        entry_id,
                        workout_type,
                        session_rpe,
                        avg_hr_bpm,
                    },
                );
                model.runs.push(run);
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
            } => {
                // Distance and duration derive from the track at `view()`; the
                // spike baseline is floored like a manual run's, so the gate
                // works without the shell fabricating a recent-longest figure.
                let mut run = logged_run(
                    &model.runs,
                    model.profile.as_ref(),
                    RunFields {
                        distance_km: 0.0,
                        duration_min: 0.0,
                        hr_pct_max,
                        longest_recent_km,
                        observed_at,
                        entry_id,
                        workout_type,
                        session_rpe,
                        avg_hr_bpm,
                    },
                );
                run.track = sanitize_track(points);
                run.track_segment_starts = segment_starts;
                model.runs.push(run);
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
            // Delete/amend of a logged set or run. Both target the
            // NEWEST matching row (id first, else `observed_at` for a legacy row)
            // mirroring the compaction Rule 3 matcher exactly. Amend = remove +
            // push carrying the SAME id, so `view()`'s derivation stays untouched.
            Event::DeleteEntry {
                kind: EntryKind::Set,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_set(
                    &model.sets,
                    sanitize_entry_id(entry_id),
                    observed_at_fallback,
                ) {
                    model.sets.remove(pos);
                }
            }
            Event::DeleteEntry {
                kind: EntryKind::Run,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_run(
                    &model.runs,
                    sanitize_entry_id(entry_id),
                    observed_at_fallback,
                ) {
                    model.runs.remove(pos);
                }
            }
            Event::DeleteEntry {
                kind: EntryKind::Activity,
                entry_id,
                observed_at_fallback,
            } => {
                if let Some(pos) = find_activity(
                    &model.activities,
                    sanitize_entry_id(entry_id),
                    observed_at_fallback,
                ) {
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
                    && let Some(pos) = find_set(&model.sets, set.entry_id, match_at)
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
                // The replacement is a fresh manual entry, so its spike baseline
                // is floored over the history without the replaced row.
                let match_at = amend_match_at(observed_at_fallback, observed_at);
                if let Some(pos) = find_run(&model.runs, sanitize_entry_id(entry_id), match_at) {
                    model.runs.remove(pos);
                    let run = logged_run(
                        &model.runs,
                        model.profile.as_ref(),
                        RunFields {
                            distance_km,
                            duration_min,
                            hr_pct_max,
                            longest_recent_km,
                            observed_at,
                            entry_id,
                            workout_type,
                            session_rpe,
                            avg_hr_bpm,
                        },
                    );
                    model.runs.push(run);
                }
            }
            Event::TrimRunTrack {
                entry_id,
                observed_at_fallback,
                keep_from_sec,
                keep_until_sec,
            } => {
                // The run is mutated IN PLACE with its identity (entry_id,
                // observed_at) unchanged, so replay order and other runs' baked
                // baselines stay stable; every derived figure re-derives from the
                // sliced track at view(). An inverted range, a no-match, or a
                // slice that is not a real trim leaves the run untouched (the
                // shell clamps the slider so an invalid trim is never dispatched;
                // a persisted no-op line replays as a harmless no-op).
                if keep_from_sec <= keep_until_sec
                    && let Some(pos) = find_run(
                        &model.runs,
                        sanitize_entry_id(entry_id),
                        observed_at_fallback,
                    )
                    && let Some((track, starts)) =
                        trim_track(&model.runs[pos], keep_from_sec, keep_until_sec)
                {
                    let run = &mut model.runs[pos];
                    run.track = track;
                    run.track_segment_starts = starts;
                }
            }
            Event::AmendRunMeta {
                entry_id,
                observed_at_fallback,
                session_rpe,
                workout_type,
            } => {
                // In-place meta edit (rating + tag) of any run, GPS included: the
                // run's identity, distance, time and track are untouched, so its
                // position in replay order and every other run's baked spike
                // baseline stay stable (mirrors TrimRunTrack's in-place edit).
                if let Some(pos) = find_run(
                    &model.runs,
                    sanitize_entry_id(entry_id),
                    observed_at_fallback,
                ) {
                    let run = &mut model.runs[pos];
                    run.session_rpe = session_rpe.map(|r| r.min(10));
                    run.workout_type = workout_type;
                }
            }
            Event::SetProfile(profile) => apply_profile(model, profile),
            Event::ClearProfile => {
                model.profile = None;
                model.profile_history.clear();
            }
            Event::SubmitReview(mut review) => {
                sanitize_review(&mut review);
                // A payload's weekly/clinical part goes to the
                // weekly check-in, its execution part to the session review,
                // so a routine session review never wipes a referral, the
                // compulsive-training flag or an in-force deload.
                if submit_review_sets_weekly(&review) {
                    let weekly = weekly_part(&review);
                    model.weekly_history.push(weekly.clone());
                    model.weekly_checkin = Some(weekly);
                }
                if submit_review_sets_session(&review) {
                    model.review = Some(session_part(&review));
                }
            }
            Event::ClearReview => model.review = None,
            Event::SubmitWeeklyCheckin(mut review) => {
                sanitize_review(&mut review);
                let weekly = weekly_part(&review);
                model.weekly_history.push(weekly.clone());
                model.weekly_checkin = Some(weekly);
            }
            Event::ClearWeeklyCheckin => model.weekly_checkin = None,
            Event::PredictRace {
                mut recent_distance_m,
                mut recent_time_sec,
                mut goal_distance_m,
                mut weekly_km,
                weeks_since_race,
            } => {
                // Sanitize every wire float BEFORE storing the query so a huge
                // FINITE input (e.g. 1e300) can't overflow a downstream product
                // (VDOT `v²`, Riegel power) to `inf` → JSON `null` → a non-null
                // Kotlin decode brick. Mirrors `SetProfile`.
                recent_distance_m = sanitize_f64(recent_distance_m);
                recent_time_sec = sanitize_f64(recent_time_sec);
                goal_distance_m = sanitize_f64(goal_distance_m);
                weekly_km = sanitize_f64(weekly_km);
                model.race_query = Some(RaceQuery {
                    recent_distance_m,
                    recent_time_sec,
                    goal_distance_m,
                    weekly_km,
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
                model.hypertrophy_plan_query = Some(HypertrophyPlanQuery {
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
                // Sanitize the wire float before storing (see `SetProfile`): an
                // `inf`-scale bodyweight would overflow the g/day product to
                // `null` and brick the non-null Kotlin decode.
                model.protein_query = Some(ProteinQuery {
                    bodyweight_kg: sanitize_f64(bodyweight_kg),
                    masters,
                    deficit,
                });
            }
            Event::ClearProtein => model.protein_query = None,
            Event::ComputeHrZones {
                age_years,
                resting_hr_bpm,
                weeks_since_recalc,
                weeks_since_pace_test,
            } => {
                // Sanitize wire floats before storing (see `SetProfile`).
                model.hr_zone_query = Some(HrZoneQuery {
                    age_years: sanitize_f64(age_years),
                    resting_hr_bpm: resting_hr_bpm.map(sanitize_f64),
                    weeks_since_recalc,
                    weeks_since_pace_test,
                });
            }
            Event::ClearHrZones => model.hr_zone_query = None,
            Event::ComputeCooper { distance_m_12min } => {
                // Sanitize the wire float before storing (see `SetProfile`).
                model.cooper_query = Some(sanitize_f64(distance_m_12min));
            }
            Event::ClearCooper => model.cooper_query = None,
            Event::ComputeCriticalSpeed { mut efforts } => {
                // Sanitize every effort's wire floats before storing (see
                // `SetProfile`): an `inf` distance/time would overflow the
                // CS/D' regression to `null` and brick the Kotlin decode.
                for e in &mut efforts {
                    e.distance_m = sanitize_f64(e.distance_m);
                    e.time_sec = sanitize_f64(e.time_sec);
                }
                model.cs_query = Some(efforts);
            }
            Event::ClearCriticalSpeed => model.cs_query = None,
            Event::ComputeApre {
                scheme,
                reps,
                current_load_kg,
            } => {
                // Sanitize the wire float before storing (see `SetProfile`).
                model.apre_query = Some(ApreQuery {
                    scheme,
                    reps,
                    current_load_kg: sanitize_f64(current_load_kg),
                });
            }
            Event::ClearApre => model.apre_query = None,
            Event::GeneratePlan { start_epoch_day } => {
                // Preserve an existing anchor: re-firing GeneratePlan (e.g. the
                // shell auto-generates on every launch so a set-up user always has
                // a plan) must NOT re-date the plan to today. Re-anchoring would
                // pin the week strip at "week 1" forever (weeks_elapsed = today −
                // start) and flip already-logged done/missed days (epoch_day <
                // start) back to a neutral "planned" on every relaunch. Keep the
                // ORIGINAL start; only ClearPlan → GeneratePlan makes a new anchor.
                let anchor = model
                    .plan_request
                    .as_ref()
                    .map_or(start_epoch_day, |p| p.start_epoch_day);
                model.plan_request = Some(PlanRequest {
                    start_epoch_day: anchor,
                });
            }
            Event::ClearPlan => {
                model.plan_request = None;
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
                model.today_utc_offset_sec = utc_offset_sec;
                ingest::refresh_birthday_ages(model);
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
        let weight_lb = model.profile.as_ref().is_some_and(|p| p.weight_lb);

        // Today's readiness: the retained check-ins normalized into the
        // synthetic signals (WellnessZ / HrvLnRmssd / RestingHr) the autoreg
        // rules consume, merged with the manual/advanced inputs, through the
        // same pipeline every past rendered day uses. The profile's lifting goal
        // selects goal-dependent thresholds (velocity-loss termination,
        // autoreg-010); no profile → conservative defaults.
        let picture = readiness_picture(
            model,
            &cx.days,
            model.profile.as_ref(),
            model.today_epoch_day,
        );
        let derived = &picture.derived;
        let readiness_inputs = &picture.inputs;
        let rule_ctx = picture.rule_ctx();
        let review = review_pipeline(model, picture.adjustments(model));
        let safety = safety_state(
            model,
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
        let train_blocked = v.safety.train_blocked;
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
                            profile: model.profile.as_ref(),
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
            program,
        } = plan_section(&cx, &v, &mut adjustments, &merged_away, weight_lb);

        let FeedbackSection { feedback, spike } = feedback_section(&cx, &v, &week_plan, weight_lb);
        let session_done = build_session_done(&week_plan, model.today_epoch_day);
        // feedback-039/041/042: no trend praise or causal story while a concern
        // card leads or training is blocked.
        let trend_suppressed =
            train_blocked || feedback.as_ref().is_some_and(|f| f.suppresses_praise);

        // The protein + HRmax calculators return their prose rows AND a
        // structured figure, so a shell consumes numbers instead of scraping the
        // summary strings. Computed here so both halves land in the literal.
        let (protein_targets, protein_figures) = match model.protein_query.as_ref() {
            Some(q) => build_protein_targets(q, reds_present, weight_lb),
            None => (Vec::new(), Vec::new()),
        };
        let calc_hold = calculator_hold(model.profile.as_ref(), reds_present);
        let (hr_zones, hr_max, hr_zone_bands) = match model.hr_zone_query.as_ref() {
            Some(q) => build_hr_zones(q, model.profile.as_ref().and_then(|p| p.measured_hr_max)),
            None => (Vec::new(), None, Vec::new()),
        };
        let (hypertrophy_plan, hypertrophy_figures) = model
            .hypertrophy_plan_query
            .as_ref()
            .map(|q| build_hypertrophy_plan(q, model.profile.as_ref(), calc_hold.as_ref()))
            .unwrap_or_default();
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

        let (runs, run_progress) = history_section(&cx);

        // The readiness strip surfaces recency-expired RHR/wellness readings as an
        // "out of date" row, so it is fed the stale inputs too; the rules
        // above ran only on the fresh `readiness_inputs`.
        let mut display_inputs = readiness_inputs.clone();
        display_inputs.extend(derived.stale.iter().cloned());

        ViewModel {
            safety_tier: v.safety.safety_tier,
            train_blocked,
            hold_reason: hold_reason(
                train_blocked,
                model.profile.as_ref().map(|p| &p.health),
                recommended,
                review_recs,
            ),
            checked_in_today: model.today_epoch_day.is_some_and(|t| {
                model.checkins.iter().any(|c| {
                    !is_undated(c.observed_at)
                        && local_day(c.observed_at, model.today_utc_offset_sec) == t
                })
            }),
            adjustments,
            review_adjustments,
            review_present: model.review.is_some(),
            weekly_checkin_present: model.weekly_checkin.is_some(),
            input_count: model.inputs.len(),
            lifts: lift_views(&model.sets, cx.sets_fp, weight_lb),
            runs,
            guidance: model
                .profile
                .as_ref()
                .map(|p| {
                    // Feed the km-gated safety/hybrid guards the athlete's real
                    // recent volume, not just the stated setup figure.
                    let recent_weekly = model
                        .today_epoch_day
                        .and_then(|today| cx.run_anchors_on(today).1);
                    let (weekly_km, basis) = effective_weekly_km(p, recent_weekly);
                    build_guidance_measured(p, weekly_km, basis)
                })
                .unwrap_or_default(),
            feedback,
            reference: build_reference(reds_present),
            profile: model.profile.clone(),
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
            cooper: model.cooper_query.map(build_cooper).unwrap_or_default(),
            critical_speed: model
                .cs_query
                .as_deref()
                .map(build_critical_speed)
                .unwrap_or_default(),
            apre: model
                .apre_query
                .as_ref()
                .map(|q| build_apre(q, weight_lb, calc_hold.as_ref()))
                .unwrap_or_default(),
            hr_zone_input: model.hr_zone_query.as_ref().map(|q| HrZoneInputView {
                age_years: q.age_years,
                resting_hr_bpm: q.resting_hr_bpm,
                weeks_since_recalc: q.weeks_since_recalc,
                weeks_since_pace_test: q.weeks_since_pace_test,
            }),
            protein_input: model.protein_query.as_ref().map(|q| ProteinInputView {
                bodyweight_kg: q.bodyweight_kg,
                masters: q.masters,
                deficit: q.deficit,
            }),
            hypertrophy_input: model.hypertrophy_plan_query.as_ref().map(|q| {
                HypertrophyInputView {
                    muscle: q.muscle.clone(),
                    weeks: q.weeks,
                    not_growing: q.not_growing,
                    recovering_easily: q.recovering_easily,
                }
            }),
            hypertrophy_ramp: model
                .hypertrophy_plan_query
                .as_ref()
                .map(|q| hypertrophy_ramp(q, model.profile.as_ref(), calc_hold.as_ref()))
                .unwrap_or_default(),
            cooper_input: model.cooper_query,
            critical_speed_input: model.cs_query.clone().unwrap_or_default(),
            apre_input: model.apre_query.as_ref().map(|q| ApreInputView {
                scheme: q.scheme,
                reps: q.reps,
                current_load_kg: q.current_load_kg,
            }),
            trend: build_trend(
                model,
                model.weekly_checkin.as_ref(),
                derived_wellness_streak,
                spike,
                trend_suppressed,
                weight_lb,
            ),
            provisional: build_provisional(model),
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
            checkin_today: model
                .checkins
                .iter()
                .max_by_key(|c| c.observed_at)
                .map(to_checkin_echo),
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
                model
                    .profile
                    .as_ref()
                    .is_some_and(|p| p.health.acute_illness),
            ),
            illness_check_prompt: autoreg::illness_active(readiness_inputs)
                || autoreg::rhr_neck_check_due(readiness_inputs, rule_ctx),
            plan_withheld: model
                .today_epoch_day
                .and_then(|d| profile_for_day(model, d))
                .or(model.profile.as_ref())
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
    /// reflected in `safety_tier`/`train_blocked` so no shell can miss them.
    gates: Vec<Recommended<Adjustment>>,
    train_blocked: bool,
    safety_tier: Option<SafetyTier>,
    /// RED-S flagged on any channel; feeds the safety-022 deficit refusal.
    reds_present: bool,
}

fn safety_state(
    model: &Model,
    readiness_tier: Option<SafetyTier>,
    review: &ReviewPipeline,
) -> SafetyState {
    let gates = model
        .profile
        .as_ref()
        .map(|p| individualization::onboarding_gates(&p.health))
        .unwrap_or_default();
    let blocks = |r: &Recommended<Adjustment>| {
        matches!(
            r.value,
            Adjustment::Stop | Adjustment::RestDay | Adjustment::Defer { .. }
        )
    };
    let train_blocked = review.recommended.iter().any(blocks)
        || gates.iter().any(blocks)
        || review.review_recs.iter().any(blocks);

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

    // RED-S from any channel: the readiness signal, the onboarding screen, or
    // a positive recurring weekly screen (REDS-SCREEN-001) in force or within
    // the screen window.
    let reds_present = model
        .inputs
        .iter()
        .filter(|i| i.signal == ReadinessSignal::RedS)
        .max_by_key(|i| i.observed_at)
        .is_some_and(|i| i.value > 0.0)
        || model.profile.as_ref().is_some_and(|p| p.health.reds_signal)
        || model
            .weekly_checkin
            .as_ref()
            .is_some_and(|r| r.reds_signal_reported)
        || model.weekly_history.iter().any(|r| {
            r.reds_signal_reported
                && model.today_epoch_day.is_none_or(|today| {
                    r.observed_at <= 0
                        || today
                            .saturating_sub(local_day(r.observed_at, model.today_utc_offset_sec))
                            < REDS_SCREEN_WINDOW_DAYS
                })
        });

    SafetyState {
        gates,
        train_blocked,
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

/// The dated week with today's session as the coach prescribes it.
struct PlanSection {
    next_session: Option<SessionPlanView>,
    week_plan: Vec<SessionPlanView>,
    program: Option<ProgramSummaryView>,
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
    let recommended = &v.review.recommended;
    let review_recs = &v.review.review_recs;
    let train_blocked = v.safety.train_blocked;
    let (mut next_session, mut week_plan, program) =
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
        let today = model.today_epoch_day;
        for s in next_session.iter_mut().chain(week_plan.iter_mut()) {
            if Some(s.epoch_day) == today
                && let Some(a) = s.adjustment.as_mut()
            {
                wording(a);
            }
        }
    }
    // Today's logged first work set of each lift, against that item's target,
    // scopes a load cut (or an opt-in add) to THAT lift's remaining sets; a
    // hard first set on a held day becomes the one-level downgrade by
    // composition.
    let mut fresh_rows: Vec<AdjustmentView> = Vec::new();
    let mut fresh: Vec<Recommended<Adjustment>> = Vec::new();
    let today_ns = next_session
        .as_mut()
        .filter(|ns| model.today_epoch_day == Some(ns.epoch_day));
    let hold = autoreg::increase_hold(v.readiness_inputs, &v.rule_ctx)
        || suppression_active(recommended, false)
        || suppression_active(review_recs, false);
    // Today's row honours the in-session cue loads too (a lift finished at
    // the "remaining sets drop 5%" load is done, not lighter or open), so the
    // strip and the Session Done card read what the hero reads.
    if !train_blocked
        && let Some(row) = week_plan.iter_mut().find(|s| {
            model.today_epoch_day == Some(s.epoch_day)
                && matches!(
                    s.status,
                    SessionStatus::Planned | SessionStatus::Next | SessionStatus::DoneLighter
                )
        })
    {
        let (status, adjustment) = (row.status, row.adjustment.take());
        apply_first_set_autoreg(row, recommended, review_recs, hold, weight_lb);
        row.adjustment = adjustment;
        row.status = rejudged_today_status(&row.items, status);
    }
    if !train_blocked && let Some(ns) = today_ns {
        let cues = apply_first_set_autoreg(ns, recommended, review_recs, hold, weight_lb);
        if !cues.is_empty() {
            // A matched readiness rec keeps its row (rows 0..recommended.len()
            // are `recommended`, in order), now carrying the cue's load.
            for c in cues.iter().filter(|c| !c.fresh) {
                if let Some(k) = recommended.iter().position(|r| *r == c.rec) {
                    adjustments[k].suggested_load_kg = c.load_kg;
                }
            }
            fresh = cues
                .iter()
                .filter(|c| c.fresh)
                .map(|c| c.rec.clone())
                .collect();
            fresh_rows = cues
                .iter()
                .filter(|c| c.fresh)
                .map(|c| {
                    let mut row = to_view(&c.rec);
                    row.suggested_load_kg = c.load_kg;
                    row
                })
                .collect();
            if let Some(row) = week_plan.iter_mut().find(|s| s.epoch_day == ns.epoch_day) {
                *row = ns.clone();
            }
        }
    }
    // Every fold, hold and cue has moved its loads: the why? basis restates
    // the planned load against today's, so the session panel reads the face.
    for it in next_session
        .iter_mut()
        .chain(week_plan.iter_mut())
        .flat_map(|s| s.items.iter_mut())
    {
        restate_basis_load(it, weight_lb);
    }
    // Say what changed on each lift since last time, and why.
    if let Some(ns) = next_session.as_mut() {
        let now_anchors = cx.lift_anchors_now();
        annotate_last_time(ns, model, now_anchors, weight_lb);
        if let Some(row) = week_plan.iter_mut().find(|s| s.epoch_day == ns.epoch_day) {
            annotate_last_time(row, model, now_anchors, weight_lb);
        }
    }
    let today_session = next_session
        .as_ref()
        .filter(|ns| model.today_epoch_day == Some(ns.epoch_day));
    // A tendon modify call names the lifts it eases in today's session.
    if let Some(items) = today_session.map(|ns| ns.items.as_slice()).or_else(|| {
        week_plan
            .iter()
            .find(|s| model.today_epoch_day == Some(s.epoch_day))
            .map(|s| s.items.as_slice())
    }) {
        for (row, r) in adjustments.iter_mut().zip(recommended) {
            name_modified_exercises(&mut row.summary, r, items);
        }
    }
    // One cue per lift: today's in-session add replaces the "next time" add
    // for the same lift; a "remaining sets" call on an exercise already
    // finished today has nothing left to change.
    let mut drop_row = recommended.iter().zip(merged_away).map(|(r, merged)| {
        *merged
            || next_time_add_superseded(r, &fresh)
            || today_session.is_some_and(|ns| remaining_sets_call_spent(r, ns))
    });
    adjustments.retain(|_| !drop_row.next().unwrap_or(false));
    adjustments.extend(fresh_rows);
    // The "log your first work set" clause is spent once one is logged, on
    // the readiness rows and on the session's own hold line.
    if today_session.is_some_and(|ns| {
        ns.items
            .iter()
            .any(|it| !it.is_run() && (it.sets_done > 0 || !it.logged_sets.is_empty()))
    }) {
        let today = model.today_epoch_day;
        let session_rows = next_session
            .iter_mut()
            .chain(week_plan.iter_mut().filter(|s| Some(s.epoch_day) == today))
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
        program,
    }
}

/// Today's session status after its items were re-judged at the in-session
/// cue loads: every item done → done; every item finished with some work →
/// done lighter; otherwise unchanged.
fn rejudged_today_status(items: &[PrescriptionView], before: SessionStatus) -> SessionStatus {
    use ItemStatus::*;
    if items.is_empty() {
        return before;
    }
    if items.iter().all(|it| it.status == Done) {
        return SessionStatus::Done;
    }
    let finished = items.iter().all(|it| {
        matches!(
            it.status,
            Done | DoneLighter | DoneShort | Skipped | SkippedPain | StoppedPain
        )
    });
    let worked = items
        .iter()
        .any(|it| matches!(it.status, Done | DoneLighter | DoneShort | StoppedPain));
    if finished && worked {
        SessionStatus::DoneLighter
    } else {
        before
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
    let advanced_user = model.profile.as_ref().is_some_and(|p| {
        individualization::training_age_from_cadence(p.progression_cadence).value
            == individualization::TrainingAge::Advanced
    });
    let female_user = model
        .profile
        .as_ref()
        .is_some_and(|p| p.female.unwrap_or(false));
    let derived_exec = model
        .today_epoch_day
        .and_then(|t| derive_session_exec(week_plan, t));
    let mastery = detect_mastery(model, &cx.runs, week_plan);
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
        protective: suppression_active(&v.review.recommended, v.safety.train_blocked)
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
/// submission order; plus the run progress layered on them. Every run consumer
/// reads `cx.runs`, so each track is QC'd at most once per view.
fn history_section(cx: &ViewCtx) -> (Vec<RunResultView>, Option<RunProgressView>) {
    let model = cx.model;
    let mut runs = run_views(&model.runs, &cx.runs);
    // Run progress and the cross-run best-effort records are layered on the
    // memoized per-run views.
    let run_progress = build_run_progress(model, &cx.runs);
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
