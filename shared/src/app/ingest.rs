//! Wire → model ingest: `update`'s fold of one event into the model
//! ([`apply`]) and the row builders it uses, so a logged row and its amended
//! replacement are built by the same code; and back, the event that re-logs
//! a stored row ([`relog_event`], the shell's Undo).

use crate::app::format::round_tenths;
use crate::app::model::{
    EndedMarker, LoggedActivity, LoggedRun, LoggedSet, Model, PlanRequest, RaceQuery,
    RetractedInput, SessionMove, profile_at,
};
use crate::app::profile_age::{PROFILE_EPOCH_DAY_RANGE, derive_age_gate, derive_birthday_age};
use crate::app::run_metrics::{in_spike_window, spike_km};
use crate::app::{HrZoneInputView, HypertrophyInputView, ProteinInputView};
use crate::day::is_undated;
use crate::event::entry::{EntryRef, LoggedEntry, find_entry};
use crate::event::rules::{PlanStep, end_marker_key, readiness_removal};
use crate::event::{EntryKind, Event, RunMeta, SessionReview};
use crate::hypertrophy;
use crate::profile::Profile;
#[cfg(any(ffi_surface, test))]
use crate::{gps::GpsPoint, sanitize::Sanitize};

/// `update`: fold one event into the model. The event is normalized first
/// ([`Event::normalized`]: legacy shapes upgraded, every value sanitized), and
/// whether a logging event stores a row, the id it stores it under and which
/// row an edit names are read off [`Event::entry_event`], the facts
/// compaction reads.
pub(super) fn apply(model: &mut Model, event: Event) {
    let event = event.normalized();
    let entry = event.entry_event();
    let stores_row = entry.is_some_and(|e| e.stored);
    let row_id = entry.map_or(0, |e| e.row.id);
    let target = entry.and_then(|e| e.target);
    let (family, _) = event.family();
    match event {
        Event::SubmitReadiness(input) => model.inputs.push(input),
        Event::RemoveReadiness {
            signal,
            observed_at,
        } => {
            if let Some(pos) = readiness_removal(&model.inputs, signal) {
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
        Event::SubmitCheckin(input) => model.checkins.push(input),
        Event::RestartHrvBaseline { observed_at } => {
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
            entry_id: _,
            hold_sec,
        } => {
            if stores_row {
                model.sets.push(logged_set(LoggedSet {
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    observed_at,
                    entry_id: row_id,
                    hold_sec,
                }));
            }
        }
        Event::SetExerciseEnded {
            exercise,
            epoch_day,
            ended,
            observed_at,
        } => {
            let key = end_marker_key(&exercise, epoch_day);
            model
                .ended_exercises
                .retain(|m| end_marker_key(&m.exercise, m.epoch_day) != key);
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
                model
                    .calis_placements
                    .push(crate::calisthenics::progression::CalisPlacement {
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
            entry_id: _,
            meta,
        } => {
            let run = logged_run(RunFields {
                distance_km,
                duration_min,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id: row_id,
                meta,
            });
            store_run(model, run, None);
        }
        Event::LogRunTrack {
            points,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            entry_id: _,
            segment_starts,
            meta,
        } => {
            let mut run = logged_run(RunFields {
                distance_km: 0.0,
                duration_min: 0.0,
                hr_pct_max,
                longest_recent_km,
                observed_at,
                entry_id: row_id,
                meta,
            });
            run.track = points;
            run.track_segment_starts = segment_starts;
            store_run(model, run, None);
        }
        Event::LogActivity {
            kind,
            minutes,
            vigorous,
            strengthening,
            observed_at,
            entry_id: _,
        } => {
            if stores_row {
                model.activities.push(LoggedActivity {
                    kind,
                    minutes,
                    vigorous,
                    strengthening,
                    observed_at,
                    entry_id: row_id,
                });
            }
        }
        Event::DeleteEntry { kind, .. } => match kind {
            EntryKind::Set => {
                if let Some(pos) = located(&model.sets, target) {
                    model.sets.remove(pos);
                }
            }
            EntryKind::Run => {
                if let Some(pos) = located(&model.runs, target) {
                    let gone = model.runs.remove(pos);
                    refresh_spike_baselines(&mut model.runs, &[gone.observed_at]);
                }
            }
            EntryKind::Activity => {
                if let Some(pos) = located(&model.activities, target) {
                    model.activities.remove(pos);
                }
            }
        },
        Event::AmendSet {
            entry_id: _,
            exercise,
            weight_kg,
            reps,
            rpe,
            observed_at,
            hold_sec,
            ..
        } => {
            // A STRICT update: the replacement is pushed ONLY when the row the
            // amend names is found, so an amend whose target was already
            // deleted is a no-op and a Log→Delete→Amend sequence cannot
            // resurrect the deleted row (compaction keeps an amend's base log
            // line for this). A set that fails the gate leaves the row as is.
            if stores_row && let Some(pos) = located(&model.sets, target) {
                model.sets.remove(pos);
                model.sets.push(logged_set(LoggedSet {
                    exercise,
                    weight_kg,
                    reps,
                    rpe,
                    observed_at,
                    entry_id: row_id,
                    hold_sec,
                }));
            }
        }
        Event::AmendRun {
            entry_id: _,
            distance_km,
            duration_min,
            hr_pct_max,
            longest_recent_km,
            observed_at,
            meta,
            ..
        } => {
            // A STRICT update like `AmendSet`. The replacement is a fresh manual
            // entry; the spike baselines around both its old and its new date
            // are re-derived.
            if let Some(pos) = located(&model.runs, target) {
                let old = model.runs.remove(pos);
                let run = logged_run(RunFields {
                    distance_km,
                    duration_min,
                    hr_pct_max,
                    longest_recent_km,
                    observed_at,
                    entry_id: row_id,
                    meta,
                });
                store_run(model, run, Some(&old));
            }
        }
        Event::TrimRunTrack {
            keep_from_sec,
            keep_until_sec,
            ..
        } => {
            // An inverted range, a no-match or a slice that is not a real
            // trim (`gps::trim_track`) leaves the run untouched.
            if keep_from_sec <= keep_until_sec
                && let Some(pos) = located(&model.runs, target)
                && let Some((track, starts)) = crate::gps::trim_track(
                    &model.runs[pos].track,
                    &model.runs[pos].track_segment_starts,
                    keep_from_sec,
                    keep_until_sec,
                )
            {
                let run = &mut model.runs[pos];
                run.track = track;
                run.track_segment_starts = starts;
                let at = run.observed_at;
                refresh_spike_baselines(&mut model.runs, &[at]);
            }
        }
        Event::AmendRunMeta {
            session_rpe,
            workout_type,
            ..
        } => {
            if let Some(pos) = located(&model.runs, target) {
                let run = &mut model.runs[pos];
                run.meta.session_rpe = session_rpe;
                run.meta.workout_type = workout_type;
            }
        }
        Event::SetProfile(profile) => {
            apply_profile(model, profile);
            refresh_run_hr(model);
        }
        Event::ClearProfile => {
            model.clear(family);
            refresh_run_hr(model);
        }
        Event::ClearReadiness
        | Event::ClearSets
        | Event::ClearRuns
        | Event::ClearActivities
        | Event::ClearCheckins
        | Event::ClearRacePrediction
        | Event::ClearHypertrophyPlan
        | Event::ClearProtein
        | Event::ClearHrZones
        | Event::ClearPlan => model.clear(family),
        Event::SubmitReview(review) => {
            // A legacy payload's weekly/clinical part goes to the weekly
            // check-in, its execution part to the session review, so a routine
            // session review never wipes a referral, the compulsive-training
            // flag or an in-force deload.
            if review.sets_weekly() {
                store_weekly(model, &review);
            }
            if review.sets_session() {
                let session = review.session_part();
                model.review_history.push(session.clone());
                model.review = Some(session);
            }
        }
        Event::ClearReview => model.review = None,
        Event::SubmitWeeklyCheckin(review) => store_weekly(model, &review),
        Event::ClearWeeklyCheckin => model.clear_weekly_checkin(),
        Event::PredictRace {
            recent_distance_m,
            recent_time_sec,
            goal_distance_m,
            weekly_km,
            weeks_since_race,
        } => {
            model.race_query = Some(RaceQuery {
                recent_distance_m,
                recent_time_sec,
                goal_distance_m,
                weekly_km,
                weeks_since_race,
            });
        }
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
        Event::ComputeProtein {
            bodyweight_kg,
            masters,
            deficit,
        } => {
            model.protein_query = Some(ProteinInputView {
                bodyweight_kg,
                masters,
                deficit,
            });
        }
        Event::ComputeHrZones {
            age_years,
            resting_hr_bpm,
        } => {
            model.hr_zone_query = Some(HrZoneInputView {
                age_years,
                resting_hr_bpm,
            });
        }
        Event::GeneratePlan { start_epoch_day } => {
            model.plan_removed = false;
            if PlanStep::of(true, model.plan_request.is_some()) == PlanStep::Start {
                model.plan_request = Some(PlanRequest { start_epoch_day });
            }
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
            refresh_birthday_ages(model);
            if offset_changed {
                refresh_run_hr(model);
            }
        }
    }
}

/// The row of `rows` an edit's `target` names.
fn located<T: LoggedEntry>(rows: &[T], target: Option<EntryRef>) -> Option<usize> {
    target.and_then(|t| find_entry(rows, t))
}

/// Clean the planning fields of a wire `Profile` on `SetProfile` ingest, so a
/// corrupt or hand-edited event can neither poison plan math nor surface an
/// impossible value. Anything implausible becomes "unstated" (`None`), which
/// every consumer already handles as the conservative default:
/// - `lift_days_per_week`: 2..=5, else `None`;
/// - `training_weekdays`: only the seven weekday bits; an empty mask → `None`;
/// - race / meet / layoff-return / birth dates: inside [`PROFILE_EPOCH_DAY_RANGE`];
/// - `recent_race`: distance 0.4–100 km, positive time no faster than
///   2:00/km, an in-range date (an out-of-range date alone is dropped);
/// - `stated_longest_recent_km`: 0–200 km;
/// - `specialization_muscle`: must resolve to a tracked muscle and is stored
///   as its canonical key.
pub(super) fn sanitize_planning_fields(p: &mut Profile) {
    p.lift_days_per_week = p.lift_days_per_week.filter(|d| (2..=5).contains(d));
    p.training_weekdays = p.chosen_weekdays();
    let date_ok = |d: &i64| PROFILE_EPOCH_DAY_RANGE.contains(d);
    p.goal_race_epoch_day = p.goal_race_epoch_day.filter(date_ok);
    p.meet_epoch_day = p.meet_epoch_day.filter(date_ok);
    p.layoff_returned_epoch_day = p.layoff_returned_epoch_day.filter(date_ok);
    p.birth_epoch_day = p.birth_epoch_day.filter(date_ok);
    p.recent_race = p.recent_race.take().and_then(|mut r| {
        let plausible = (0.4..=100.0).contains(&r.distance_km)
            && r.time_sec > 0
            && r.time_sec as f64 / r.distance_km >= 120.0;
        r.epoch_day = r.epoch_day.filter(date_ok);
        plausible.then_some(r)
    });
    p.stated_longest_recent_km = p
        .stated_longest_recent_km
        .filter(|km| (0.0..=200.0).contains(km));
    p.specialization_muscle = p
        .specialization_muscle
        .take()
        .and_then(|m| hypertrophy::resolve_muscle(&m).map(|lm| lm.muscle.to_string()));
}

/// The stored set. A set logged under an earlier catalog label is stored
/// under the current name (`exercise::renamed`), so every view shows one name
/// for the lift.
fn logged_set(mut set: LoggedSet) -> LoggedSet {
    if let Some(name) = crate::exercise::renamed(&set.exercise) {
        set.exercise = name.to_owned();
    }
    set
}

/// A run as `LogRun` and `AmendRun` carry it. A GPS run (`LogRunTrack`) has
/// no manual distance or duration; its figures derive from the track.
struct RunFields {
    distance_km: f64,
    duration_min: f64,
    hr_pct_max: f64,
    longest_recent_km: f64,
    observed_at: i64,
    entry_id: u64,
    meta: RunMeta,
}

/// The run row as the wire states it, with no track. Its two figures that read
/// other state, the typed-bpm %HRmax and the card spike baseline, start at the
/// wire values; [`store_run`] derives them.
fn logged_run(f: RunFields) -> LoggedRun {
    LoggedRun {
        distance_km: f.distance_km,
        duration_min: f.duration_min,
        hr_pct_max: f.hr_pct_max,
        wire_hr_pct_max: f.hr_pct_max,
        longest_recent_km: f.longest_recent_km,
        wire_longest_recent_km: f.longest_recent_km,
        track: Vec::new(),
        hr_pinned: false,
        track_segment_starts: Vec::new(),
        observed_at: f.observed_at,
        entry_id: f.entry_id,
        meta: RunMeta {
            avg_hr_bpm: crate::running::plausible_avg_hr_bpm(f.meta.avg_hr_bpm),
            ..f.meta
        },
    }
}

/// Store a new run row (or an amend's replacement of `replaced`, already
/// removed, which keeps the original row's logged UTC offset) with its %HRmax
/// derived, and re-derive the spike baselines its arrival and the old row's
/// departure can move.
fn store_run(model: &mut Model, mut run: LoggedRun, replaced: Option<&LoggedRun>) {
    if let Some(old) = replaced {
        run.meta.utc_offset_sec = old.meta.utc_offset_sec;
    }
    run.hr_pct_max = derived_hr_pct_max(model, &run);
    let at = run.observed_at;
    model.runs.push(run);
    match replaced {
        Some(old) => refresh_spike_baselines(&mut model.runs, &[old.observed_at, at]),
        None => refresh_spike_baselines(&mut model.runs, &[at]),
    }
}

/// A run's stored %HRmax: a plausible typed bpm converted against the HRmax
/// ([`Profile::hr_max_anchor`]: measured, else the Tanaka age estimate) of the
/// profile in force at the run's instant (`profile_at`, on the clock the row
/// was logged under), rounded to 0.1 %; else the wire figure. A pure function
/// of the row and the profile history, never of log order.
pub(super) fn derived_hr_pct_max(model: &Model, run: &LoggedRun) -> f64 {
    let Some(bpm) = run.meta.avg_hr_bpm else {
        return run.wire_hr_pct_max;
    };
    let offset = run
        .meta
        .utc_offset_sec
        .unwrap_or(model.today_utc_offset_sec);
    match profile_at(model, run.observed_at, offset).and_then(|p| p.hr_max_anchor()) {
        Some((max, _)) => round_tenths(bpm / max * 100.0),
        None => run.wire_hr_pct_max,
    }
}

/// Re-derive every typed-bpm run's %HRmax ([`derived_hr_pct_max`]) after the
/// profile history or the clock's UTC offset (which a legacy row without a
/// stamped offset reads) changed. A row pinned by a legacy undated edit keeps
/// its figure.
fn refresh_run_hr(model: &mut Model) {
    let derived: Vec<(usize, f64)> = model
        .runs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.meta.avg_hr_bpm.is_some() && !r.hr_pinned)
        .map(|(i, r)| (i, derived_hr_pct_max(model, r)))
        .collect();
    for (i, pct) in derived {
        model.runs[i].hr_pct_max = pct;
    }
}

/// Re-derive the card spike baseline of every run a change at the stamps in
/// `changed` can move. A run's baseline is the longest OTHER run in its
/// trailing spike window ([`in_spike_window`]), floored by the caller-supplied
/// paired-history value, so a manual entry gets the same RUN-SPIKE-001 chip a
/// GPS-tracked one does; only runs dated in `[t, t + window]` of a changed
/// stamp `t` can move. It is a pure function of the surviving rows, so a
/// replay, a compacted replay and a delete + Undo all show the same chip.
fn refresh_spike_baselines(runs: &mut [LoggedRun], changed: &[i64]) {
    let affected: Vec<usize> = (0..runs.len())
        .filter(|&i| {
            changed
                .iter()
                .any(|&t| in_spike_window(t, runs[i].observed_at))
        })
        .collect();
    if affected.is_empty() {
        return;
    }
    // Each run in a window is measured once (a GPS run through the QC memo).
    let mut km: Vec<Option<f64>> = vec![None; runs.len()];
    let mut baselines = Vec::with_capacity(affected.len());
    for &i in &affected {
        let at = runs[i].observed_at;
        let mut longest = 0.0_f64;
        for (j, r) in runs.iter().enumerate() {
            if j != i && in_spike_window(r.observed_at, at) {
                longest = longest.max(*km[j].get_or_insert_with(|| spike_km(r)));
            }
        }
        baselines.push((i, runs[i].wire_longest_recent_km.max(longest)));
    }
    for (i, baseline) in baselines {
        runs[i].longest_recent_km = baseline;
    }
}

/// The distance (km) and moving time (min) a GPS track will be SAVED with,
/// before it is logged: the tracker's Finish review shows these, so the
/// figures the runner confirms are the ones History then lists. Same ingest
/// sanitizing and `run_metrics::qc_figures` as a logged `LogRunTrack`, and the same
/// "fewer than two usable fixes measures nothing" rule as its card.
#[cfg(any(ffi_surface, test))]
pub(crate) fn preview_track_figures(points: Vec<GpsPoint>, segment_starts: &[u32]) -> (f64, f64) {
    let q = crate::app::run_metrics::qc_figures(&points.sanitized(), segment_starts);
    if q.track.len() < 2 {
        return (0.0, 0.0);
    }
    (q.distance_km, q.moving_min)
}

/// The event that puts the stored row `(entry_id, observed_at)` back as it is
/// now, the identity delete and amend name it by (the newest such row). For a
/// delete's Undo (`amended_at` `None`) the logging event, a GPS run with its
/// recorded track; for an edit's Undo the amend that restores the row the
/// edit saved under `amended_at`. A run carries its wire `hr_pct_max` and
/// its [`RunMeta`], so `update` derives a typed bpm's percentage again; the
/// legacy `longest_recent_km` is never written, so the shell's copy carries
/// none. `None` for an unknown row, and for an edit's Undo of a GPS run or an
/// activity, which no amend restores.
#[cfg(any(ffi_surface, test))]
pub(crate) fn relog_event(
    model: &Model,
    kind: EntryKind,
    entry_id: u64,
    observed_at: i64,
    amended_at: Option<i64>,
) -> Option<Event> {
    let named = |id: u64, at: i64| id == entry_id && at == observed_at;
    match kind {
        EntryKind::Set => {
            let s = model
                .sets
                .iter()
                .rev()
                .find(|s| named(s.entry_id, s.observed_at))?;
            Some(match amended_at {
                None => Event::LogSet {
                    exercise: s.exercise.clone(),
                    weight_kg: s.weight_kg,
                    reps: s.reps,
                    rpe: s.rpe,
                    observed_at,
                    entry_id,
                    hold_sec: s.hold_sec,
                },
                Some(at) => Event::AmendSet {
                    entry_id,
                    exercise: s.exercise.clone(),
                    weight_kg: s.weight_kg,
                    reps: s.reps,
                    rpe: s.rpe,
                    observed_at,
                    observed_at_fallback: at,
                    hold_sec: s.hold_sec,
                },
            })
        }
        EntryKind::Run => {
            let r = model
                .runs
                .iter()
                .rev()
                .find(|r| named(r.entry_id, r.observed_at))?;
            match (amended_at, r.track.is_empty()) {
                (None, true) => Some(Event::LogRun {
                    distance_km: r.distance_km,
                    duration_min: r.duration_min,
                    hr_pct_max: r.wire_hr_pct_max,
                    longest_recent_km: 0.0,
                    observed_at,
                    entry_id,
                    meta: r.meta,
                }),
                (None, false) => Some(Event::LogRunTrack {
                    points: r.track.clone(),
                    hr_pct_max: r.wire_hr_pct_max,
                    longest_recent_km: 0.0,
                    observed_at,
                    entry_id,
                    segment_starts: r.track_segment_starts.clone(),
                    meta: r.meta,
                }),
                // The amend keeps its row's logged offset.
                (Some(at), true) => Some(Event::AmendRun {
                    entry_id,
                    distance_km: r.distance_km,
                    duration_min: r.duration_min,
                    hr_pct_max: r.wire_hr_pct_max,
                    longest_recent_km: 0.0,
                    observed_at,
                    meta: RunMeta {
                        utc_offset_sec: None,
                        ..r.meta
                    },
                    observed_at_fallback: at,
                }),
                (Some(_), false) => None,
            }
        }
        EntryKind::Activity => {
            let a = model
                .activities
                .iter()
                .rev()
                .find(|a| named(a.entry_id, a.observed_at))?;
            amended_at.is_none().then_some(Event::LogActivity {
                kind: a.kind,
                minutes: a.minutes,
                vigorous: a.vigorous,
                strengthening: a.strengthening,
                observed_at,
                entry_id,
            })
        }
    }
}

/// Store `review`'s weekly part as the weekly check-in in force: the newest
/// entry of the weekly history.
fn store_weekly(model: &mut Model, review: &SessionReview) {
    model.weekly_history.push(review.weekly_part());
    model.weekly_checkin_cleared = false;
}

/// `SetProfile`: derive what the core owns and record the profile in the
/// profile history.
fn apply_profile(model: &mut Model, mut profile: Profile) {
    // A legacy undated edit applies to logged runs from its position in the
    // log: every run already logged keeps the %HRmax it has now (it replaces
    // the whole history below, so re-deriving would re-convert all of them).
    if profile.effective_from_epoch_day.is_none() {
        for run in &mut model.runs {
            run.hr_pinned = true;
        }
    }
    let key = profile.history_key();
    // running-041: an undated race takes the date of the same race on the
    // previous history entry, else this edit's effective day. Read before
    // the planning fields are cleaned (a dropped out-of-range date stays
    // undated); an undated edit leaves the race undated.
    if let Some(race) = profile.recent_race.as_mut()
        && race.epoch_day.is_none()
    {
        let previous = model
            .profile_history
            .iter()
            .rev()
            .find(|p| p.history_key() < key)
            .and_then(|p| p.recent_race.as_ref())
            .filter(|p| p.distance_km == race.distance_km && p.time_sec == race.time_sec);
        race.epoch_day = previous
            .and_then(|p| p.epoch_day)
            .or(profile.effective_from_epoch_day);
    }
    sanitize_planning_fields(&mut profile);
    if !profile.running_km_per_week_set {
        profile.running_km_per_week = 0.0;
    }
    derive_age_gate(&mut profile, model.today_epoch_day);
    // Profile history: an edit supersedes every entry at or after its own
    // [`Profile::history_key`] (effective day, then the instant it was made;
    // an undated edit, −∞, the whole history), so a later goal/weekly-set
    // change never re-prescribes an already-shown past day, and an evening
    // edit leaves the morning's in force for the morning.
    model.profile_history.retain(|p| p.history_key() < key);
    model.profile_history.push(profile);
}

/// `SetToday`: re-derive every birthday-carrying profile's age (and with it
/// the under-18 gate) for the new day, so the age never goes stale and the
/// gate lifts on the 18th birthday without a profile edit. The stored values
/// are TODAY's; a past day reads its own day's age through
/// `profile_age::aged_on` (`profile_for_day`), so the birthday never rewrites
/// the days it held.
fn refresh_birthday_ages(model: &mut Model) {
    let Some(today) = model.today_epoch_day else {
        return;
    };
    for p in &mut model.profile_history {
        derive_birthday_age(p, today);
    }
}
