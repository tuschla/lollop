//! Event-log compaction. Shells persist the raw [`Event`] stream and rebuild
//! state by replaying it (the core keeps no durable state), so the log grows
//! without bound, a GPS-tracked run alone is thousands of points on one line.
//!
//! [`compact_event_log`] drops lines whose effect on the replayed model is
//! provably nil, returning a shorter but **replay-equivalent** stream (same
//! relative order of survivors). Four rules, all grounded in [`crate::app`]'s
//! `update`, where the model stores only raw inputs and no event's `update`
//! reads another family's state:
//!
//! 0. **A `RemoveReadiness` cancels its submit.** It drops exactly the latest
//!    prior `SubmitReadiness` with the same signal, so the pair is inert;
//!    an unmatched remove replays as a no-op and is dropped alone.
//! 3. **A `DeleteEntry` cancels its entry; an `AmendSet`/`AmendRun` supersedes a
//!    prior edit but keeps the entry's base log.** Amend is a STRICT update in
//!    `update` (B8): it replaces a matching row and is a NO-OP on a miss, so an
//!    amend whose target was already deleted can no longer resurrect it. To stay
//!    replay-equivalent, a surviving amend must keep its base log line (it drops
//!    only a *superseded prior amend*), so a Log→Amend→Amend chain collapses to
//!    `[log, last amend]`. A Delete removes the whole entry, its newest matching
//!    line and, when that is an amend, the base chain down to and including the
//!    log, plus itself. An unmatched delete OR amend replays as a no-op and is
//!    dropped alone. Runs before Rule 1.
//! 1. **A `Clear<F>` supersedes its family.** It empties family `F`'s vec, so
//!    every `F` event (and the clear) at or before the *last* `Clear<F>` leaves
//!    no residue, `F` events after it replay against an already-empty vec.
//!    (A run's `longest_recent_km` reads prior runs, but only within the run
//!    family and only forward of a clear, so this stays exact.)
//! 2. **Last write wins for singletons.** `SetProfile` / `SubmitReview` assign
//!    `model.profile` / `model.review` outright, and nothing else reads them at
//!    update time, so only the last surviving one matters.
//! 4. **Check-ins (family 12) age out of a trailing window, PER CHANNEL.**
//!    `derive_readiness` reads at most the newest [`BASELINE_WINDOW_DAYS`](crate::autoreg)
//!    (30) of check-ins, but windows EACH channel (wellness / HRV / resting-HR)
//!    on THAT channel's OWN newest reading, not the log's global newest (a sparse
//!    channel last logged long ago is still read). So the cutoff anchors on the
//!    MIN over channels present of that channel's own newest reading: a
//!    `SubmitCheckin` more than [`RETAIN_CHECKIN_DAYS`] (45) days before that
//!    anchor is out of window for EVERY channel it carries, so dropping it is
//!    provably inert, replay-equivalent and deterministic (the reference is log
//!    data, never a clock). Family 0 readiness is emphatically NOT windowed: a
//!    Pain/Illness hold lives until explicitly cleared (HARD RULE 3).
//!
//! This is the authoritative implementation, and the replay-equivalence tests
//! below pin its correctness. Because it is *not* exposed over the JSON FFI, the
//! Kotlin Android shell reimplements the same two rules over raw wire lines in
//! `android/.../EventLog.kt::compact` (called from `Core.kt`). The two must stay
//! in lockstep: any change to the families here (or a new `Event` variant) must
//! be mirrored there.

use crate::app::Event;

/// Family id + `is_clear` for one event. The `match` is exhaustive (no `_`) so a
/// new [`Event`] variant fails to compile until it is placed in a family, a
/// mis-filed variant would silently corrupt compaction.
fn classify(event: &Event) -> (u8, bool) {
    match event {
        Event::SubmitReadiness(_) => (0, false),
        Event::RemoveReadiness { .. } => (0, false),
        Event::ClearReadiness => (0, true),
        Event::LogSet { .. } => (1, false),
        Event::AmendSet { .. } => (1, false),
        Event::ClearSets => (1, true),
        Event::LogRun { .. } | Event::LogRunTrack { .. } => (2, false),
        Event::AmendRun { .. } => (2, false),
        Event::ClearRuns => (2, true),
        // A DeleteEntry joins the family it targets (set→1, run→2). Not a clear:
        // Rule 3 pairs it with its matched log line, and Rule 1 sweeps it away if
        // it sits at/before a later Clear<F> like any other family member.
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Set,
            ..
        } => (1, false),
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Run,
            ..
        } => (2, false),
        Event::SetProfile(_) => (3, false),
        Event::ClearProfile => (3, true),
        Event::SubmitReview(_) => (4, false),
        Event::ClearReview => (4, true),
        Event::PredictRace { .. } => (5, false),
        Event::ClearRacePrediction => (5, true),
        Event::PlanHypertrophyMeso { .. } => (6, false),
        Event::ClearHypertrophyPlan => (6, true),
        Event::ComputeProtein { .. } => (7, false),
        Event::ClearProtein => (7, true),
        Event::ComputeHrZones { .. } => (8, false),
        Event::ClearHrZones => (8, true),
        Event::ComputeCooper { .. } => (9, false),
        Event::ClearCooper => (9, true),
        Event::ComputeCriticalSpeed { .. } => (10, false),
        Event::ClearCriticalSpeed => (10, true),
        Event::ComputeApre { .. } => (11, false),
        Event::ClearApre => (11, true),
        // Morning check-ins (Phase 2 / B1): a RETAINED multi-day history the
        // core normalizes into z-scores/deltas. Deliberately NOT day-scoped like
        // readiness family 0: the whole point is a rolling baseline, so its
        // only reducer is a wholesale `ClearCheckins` (Rule 1). No per-entry undo
        // family-0 analog: check-ins carry no red-flag hold to mis-tap.
        Event::SubmitCheckin(_) => (12, false),
        Event::ClearCheckins => (12, true),
        // Coach-as-planner (Phase 6 / B3). Family 13: the accepted plan request
        // is a last-write-wins singleton (like the profile) with a ClearPlan
        // reset. Family 14: SetToday is a last-write-wins singleton (the shell's
        // clock, sent on every foreground) with no clear: compaction keeps
        // exactly one line so it never bloats the log.
        Event::GeneratePlan { .. } => (13, false),
        Event::ClearPlan => (13, true),
        Event::SetToday { .. } => (14, false),
    }
}

/// Compaction family of a [`crate::app::EntryKind`] (Set→1, Run→2), used to
/// place a [`Event::DeleteEntry`] with its target family in Rule 3.
fn entry_kind_family(kind: crate::app::EntryKind) -> u8 {
    match kind {
        crate::app::EntryKind::Set => 1,
        crate::app::EntryKind::Run => 2,
    }
}

/// A logged-history line's `(family, entry_id, observed_at)` identity, or `None`
/// if the event is not a set/run log-or-amend line. Rule 3 matches a delete/amend
/// against these, the same fields `update`'s `find_set`/`find_run` compare.
fn entry_identity(event: &Event) -> Option<(u8, u64, i64)> {
    match event {
        Event::LogSet {
            entry_id,
            observed_at,
            ..
        }
        | Event::AmendSet {
            entry_id,
            observed_at,
            ..
        } => Some((1, *entry_id, *observed_at)),
        Event::LogRun {
            entry_id,
            observed_at,
            ..
        }
        | Event::LogRunTrack {
            entry_id,
            observed_at,
            ..
        }
        | Event::AmendRun {
            entry_id,
            observed_at,
            ..
        } => Some((2, *entry_id, *observed_at)),
        _ => None,
    }
}

/// Whether any run-family log/amend line sits strictly between indices `i` and
/// `j`. A run bakes its spike baseline from the runs present when logged, so a
/// run at `i` that a later run (in this window) baked into its baseline cannot be
/// pair-dropped, see Rule 3.
fn run_line_between(events: &[Event], i: usize, j: usize) -> bool {
    events[i + 1..j]
        .iter()
        .any(|e| matches!(entry_identity(e), Some((2, _, _))))
}

/// Whether `event` is an `AmendSet`/`AmendRun` (an *edit* of an existing entry),
/// as opposed to an original `LogSet`/`LogRun`/`LogRunTrack` (a base row). Rule 3
/// keeps a base log alive for a surviving amend but telescopes away a superseded
/// prior amend, and a delete walks the amend chain back to the base log.
fn is_amend_line(event: &Event) -> bool {
    matches!(event, Event::AmendSet { .. } | Event::AmendRun { .. })
}

/// Whether `event` is the log/amend line a delete/amend targeting
/// `(fam, id, time)` matches: same family, and same id (nonzero) or same
/// `observed_at` for a legacy (`id == 0`) row. Mirrors `find_set`/`find_run`.
fn entry_line_matches(event: &Event, fam: u8, id: u64, time: i64) -> bool {
    match entry_identity(event) {
        Some((f, cid, ctime)) if f == fam => {
            if id != 0 {
                cid == id
            } else {
                cid == 0 && ctime == time
            }
        }
        _ => false,
    }
}

/// Number of distinct families [`classify`] partitions events into.
const FAMILIES: u8 = 15;
/// Singleton families whose members are last-write-wins (profile, review,
/// race prediction, hypertrophy plan, protein target, HR-zone table, Cooper
/// test, critical-speed fit, APRE adjustment, plan request, today's epoch-day).
const SINGLETONS: [u8; 11] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14];

/// Rule 4 retention window: a `SubmitCheckin` more than this many days before the
/// newest surviving check-in's `observed_at` is dropped. Chosen safely larger
/// than autoreg's `BASELINE_WINDOW_DAYS` (30) so every dropped line is already
/// outside the rolling-baseline window `derive_readiness` reads, lockstep with
/// `EventLog.kt::RETAIN_CHECKIN_DAYS`.
const RETAIN_CHECKIN_DAYS: i64 = 45;
/// Seconds per day, for the Rule 4 retention window (matches autoreg's day size).
const CHECKIN_DAY_SEC: i64 = 86_400;

/// Drop provably-inert events from a persisted log, preserving replay order.
pub fn compact_event_log(events: Vec<Event>) -> Vec<Event> {
    let kinds: Vec<(u8, bool)> = events.iter().map(classify).collect();
    let mut remove = vec![false; events.len()];

    // Rule 0: a RemoveReadiness cancels the latest not-yet-cancelled prior
    // SubmitReadiness carrying the same signal, replay-equivalent because
    // `update` drops exactly that input (rposition). An unmatched remove
    // replays as a no-op, so it is dropped either way. Runs before Rule 1;
    // the outcomes agree in every interleaving with ClearReadiness because
    // both reductions preserve the replayed `inputs` vec.
    for j in 0..events.len() {
        let Event::RemoveReadiness { signal } = &events[j] else {
            continue;
        };
        let matched = (0..j).rev().find(|&i| {
            !remove[i]
                && matches!(&events[i], Event::SubmitReadiness(input) if input.signal == *signal)
        });
        if let Some(i) = matched {
            remove[i] = true;
        }
        remove[j] = true;
    }

    // Rule 3: a Delete cancels its entry; an Amend supersedes a prior edit but
    // keeps the entry's base log. For each DeleteEntry / AmendSet / AmendRun, find
    // the newest not-yet-removed PRIOR log-or-amend line in the same family whose
    // identity matches (id if nonzero, else `observed_at` for a legacy row), the
    // same predicate `update` uses (`find_set`/`find_run`, newest-match).
    //
    // B8 full prevention: `update`'s amend is a STRICT update (replace-on-match,
    // NO-OP on miss), so a lone amend with no base row replays to nothing. Two
    // consequences, both replay-equivalent:
    //   * A matched Amend telescopes away a *superseded prior amend* but KEEPS the
    //     base log line, so the strict amend has a row to replace on replay. A
    //     Log→Amend→Amend chain collapses to `[log, last amend]`.
    //   * A Delete removes the WHOLE entry: its newest matching line and, when that
    //     is an amend, the base chain back through and INCLUDING the log, the one
    //     row `update`'s delete removes, plus itself.
    // An unmatched delete OR amend replays as a no-op and is dropped alone. Runs
    // before Rule 1: a Clear then sweeps away anything still marked in-family
    // at/before it, absorbing any match made across a clear (that line was cleared
    // anyway).
    //
    // F2 INVARIANT: Rule 3's chain-walk correctness depends on legacy (`entry_id
    // == 0`) rows NEVER being re-dated, the walk keys a whole set/run chain by
    // ONE `(fam, id, observed_at)` match key, which only stays valid while a legacy
    // row's `observed_at` is immutable. The shell enforces this (LogEntry.kt pins
    // `observed_at == observed_at_fallback` for id-0 rows, blocking re-dating). If
    // that pin is ever lifted, a re-dated legacy `Log→Amend→Amend` telescopes away
    // the intermediate re-date link (reverting to the original value on replay) and
    // a re-dated `Log→Amend→Delete` can resurrect the pre-edit row, see the
    // `rule3_walk_assumes_legacy_rows_are_never_redated` regression guard below.
    for j in 0..events.len() {
        let (fam, id, time, is_delete) = match &events[j] {
            Event::DeleteEntry {
                kind,
                entry_id,
                observed_at_fallback,
            } => (entry_kind_family(*kind), *entry_id, *observed_at_fallback, true),
            // B8: an amend targets its OLD row, for a legacy (`id == 0`) row whose
            // date CHANGED, that is `observed_at_fallback` (the old timestamp),
            // not the amend's new `observed_at`. Mirrors `update`'s `find_set`/
            // `find_run` match key so replay and compaction agree.
            Event::AmendSet {
                entry_id,
                observed_at,
                observed_at_fallback,
                ..
            } => (
                1u8,
                *entry_id,
                if *observed_at_fallback != 0 {
                    *observed_at_fallback
                } else {
                    *observed_at
                },
                false,
            ),
            Event::AmendRun {
                entry_id,
                observed_at,
                observed_at_fallback,
                ..
            } => (
                2u8,
                *entry_id,
                if *observed_at_fallback != 0 {
                    *observed_at_fallback
                } else {
                    *observed_at
                },
                false,
            ),
            _ => continue,
        };
        let Some(i) = (0..j)
            .rev()
            .find(|&i| !remove[i] && entry_line_matches(&events[i], fam, id, time))
        else {
            // Unmatched delete OR amend: replays as a no-op under `update`'s strict
            // find-then-act (B8), so drop it alone.
            remove[j] = true;
            continue;
        };
        // A set carries NO baked cross-entry state (its e1RM chain is derived in
        // `view` from the surviving rows), so removing a set line is always exact.
        // A RUN bakes `longest_recent_km` from the runs present when it was logged,
        // so removing a run line could change the baked spike baseline of any run
        // logged while it was still present: i.e. any run line between the EARLIEST
        // removed line and this delete/amend. In that case DON'T remove: keeping the
        // lines is trivially replay-equivalent (only removals can break replay).
        if is_delete {
            // Collect the entry this delete cancels: the newest match, and, when
            // that is an amend: its base chain (further prior matches back through
            // and INCLUDING the first log line). A bare (shared-id) log stops at
            // itself, mirroring `update`'s newest-only row removal.
            let mut removal = vec![i];
            loop {
                let last = *removal.last().unwrap();
                if !is_amend_line(&events[last]) {
                    break; // reached the base log line
                }
                match (0..last)
                    .rev()
                    .find(|&p| !remove[p] && entry_line_matches(&events[p], fam, id, time))
                {
                    Some(p) => removal.push(p),
                    None => break,
                }
            }
            let earliest = *removal.iter().min().unwrap();
            if fam == 1 || !run_line_between(&events, earliest, j) {
                for k in removal {
                    remove[k] = true;
                }
                remove[j] = true;
            }
            // else: baseline-unsafe → keep the entry and the delete (both survive).
        } else {
            // Amend: telescope away a superseded prior AMEND, but keep the base LOG
            // line (the strict amend needs a row to replace on replay). The amend
            // itself always survives: it carries the entry's current values.
            if is_amend_line(&events[i]) && (fam == 1 || !run_line_between(&events, i, j)) {
                remove[i] = true;
            }
        }
    }

    // Rule 1: everything in family F at or before its last clear.
    for fam in 0..FAMILIES {
        if let Some(last_clear) = kinds.iter().rposition(|&(f, c)| f == fam && c) {
            for (j, &(f, _)) in kinds.iter().enumerate().take(last_clear + 1) {
                if f == fam {
                    remove[j] = true;
                }
            }
        }
    }

    // Rule 2: for each singleton family, keep only the last surviving member.
    for fam in SINGLETONS {
        let survivors: Vec<usize> = kinds
            .iter()
            .enumerate()
            .filter(|&(j, &(f, c))| f == fam && !c && !remove[j])
            .map(|(j, _)| j)
            .collect();
        for &j in survivors.iter().rev().skip(1) {
            remove[j] = true;
        }
    }

    // Rule 4: age check-ins (family 12) out of a trailing window, PER CHANNEL.
    // `derive_readiness` windows EACH channel (wellness / HRV / resting-HR) on
    // THAT channel's own newest reading (`per_day_series`, autoreg.rs: it takes
    // the max day *among readings that carry the channel's value* and keeps
    // `newest - day <= BASELINE_WINDOW_DAYS` (30)): NOT the log's global newest
    // check-in. A channel last logged long ago (e.g. sparse resting-HR/HRV) is
    // therefore STILL read on replay, so anchoring the cutoff on the global
    // newest could delete lines a sparse channel still needs, silently vanishing
    // its readiness row on the next launch (safety-adjacent, RestingHr/HRV feed
    // the SafetyTier ladder). Conservative per-channel-safe cutoff: anchor on the
    // MIN over channels PRESENT of that channel's own newest reading. Any check-in
    // older than `anchor - RETAIN_CHECKIN_DAYS` is out of window for EVERY channel
    // it carries (each channel's newest >= the min), so dropping it is provably
    // replay-equivalent. Reference is surviving-check-in log data, never a clock.
    // Family 0 readiness is never touched here: HARD RULE 3.
    let (mut wellness_newest, mut hrv_newest, mut rhr_newest): (
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ) = (None, None, None);
    for (j, e) in events.iter().enumerate() {
        if remove[j] {
            continue;
        }
        let Event::SubmitCheckin(c) = e else { continue };
        // `per_day_series` skips `observed_at <= 0` (can't be day-bucketed), so
        // such readings anchor no channel's window.
        if c.observed_at <= 0 {
            continue;
        }
        if c.sleep_quality.is_some() || c.soreness.is_some() || c.mood.is_some() {
            wellness_newest = Some(wellness_newest.map_or(c.observed_at, |n| n.max(c.observed_at)));
        }
        if c.hrv_rmssd_ms.is_some_and(|v| v > 0.0) {
            hrv_newest = Some(hrv_newest.map_or(c.observed_at, |n| n.max(c.observed_at)));
        }
        if c.resting_hr_bpm.is_some_and(|v| v > 0.0) {
            rhr_newest = Some(rhr_newest.map_or(c.observed_at, |n| n.max(c.observed_at)));
        }
    }
    let anchor = [wellness_newest, hrv_newest, rhr_newest]
        .into_iter()
        .flatten()
        .min();
    if let Some(anchor) = anchor {
        let cutoff = anchor - RETAIN_CHECKIN_DAYS * CHECKIN_DAY_SEC;
        for (j, e) in events.iter().enumerate() {
            if matches!(e, Event::SubmitCheckin(c) if c.observed_at < cutoff) {
                remove[j] = true;
            }
        }
    }

    events
        .into_iter()
        .enumerate()
        .filter(|(j, _)| !remove[*j])
        .map(|(_, e)| e)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Engine, Model, Profile, SessionReview};
    use crate::hybrid::ConcurrentGoal;
    use crate::individualization::ProgressionCadence;
    use crate::running::GoalDistance;
    use crate::schema::ReadinessInput;
    use crate::schema::ReadinessSignal;
    use crate::strength::LiftGoal;
    use crux_core::App;

    const DAY: i64 = 86_400;

    fn set(exercise: &str) -> Event {
        Event::LogSet {
            exercise: exercise.into(),
            weight_kg: 100.0,
            reps: 5,
            rpe: 8.0,
            observed_at: 0,
            entry_id: 0,
        }
    }

    fn run(distance_km: f64) -> Event {
        Event::LogRun {
            distance_km,
            duration_min: distance_km * 5.0,
            hr_pct_max: 75.0,
            longest_recent_km: 0.0,
            observed_at: 0,
            entry_id: 0,
            workout_type: None,
        }
    }

    fn readiness(value: f64) -> Event {
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::WellnessZ,
            value,
            observed_at: 0,
            streak: 0,
            pain: None,
            effort_min: None,
        })
    }

    fn profile(weekly_sets: u8) -> Event {
        Event::SetProfile(Profile {
            progression_cadence: ProgressionCadence::WeekToWeek,
            lift_goal: LiftGoal::MaxStrength,
            goal_distance: GoalDistance::TenK,
            concurrent_goal: ConcurrentGoal::Strength,
            weekly_sets,
            running_days_per_week: 4,
            running_km_per_week: 45.0,
            advanced: false,
            endurance_intensity_pct_vo2max: 75.0,
            female: false,
            high_load_block: false,
            health: Default::default(),
            environment: None,
            env_temp_c: None,
            env_altitude_m: None,
            weeks_off: None,
            bodyweight_kg: None,
            age_years: None,
            resting_hr_bpm: None,
            measured_hr_max: None,
        })
    }

    fn review(overtraining: u8) -> Event {
        Event::SubmitReview(SessionReview {
            overtraining_signal_count: overtraining,
            ..SessionReview::default()
        })
    }

    /// Rebuild the view a shell would render from a raw event stream.
    fn replay(events: &[Event]) -> crate::app::ViewModel {
        let app = Engine;
        let mut model = Model::default();
        for ev in events {
            let _ = app.update(ev.clone(), &mut model);
        }
        app.view(&model)
    }

    /// The core invariant: compaction must not change what the shell renders.
    fn assert_replay_equivalent(events: Vec<Event>) {
        let compacted = compact_event_log(events.clone());
        assert!(
            compacted.len() <= events.len(),
            "compaction must never grow the log"
        );
        assert_eq!(
            replay(&events),
            replay(&compacted),
            "compacted log replayed to a different view"
        );
    }

    #[test]
    fn clear_drops_its_family_prefix_but_keeps_later_entries() {
        let out = compact_event_log(vec![set("a"), set("b"), Event::ClearSets, set("c")]);
        assert_eq!(out, vec![set("c")], "only the post-clear set survives");
    }

    #[test]
    fn clear_with_no_later_entry_collapses_to_empty() {
        let out = compact_event_log(vec![run(5.0), run(6.0), Event::ClearRuns]);
        assert!(out.is_empty(), "a trailing clear leaves nothing to replay");
    }

    #[test]
    fn clear_only_touches_its_own_family() {
        let out = compact_event_log(vec![set("a"), run(5.0), Event::ClearSets]);
        assert_eq!(out, vec![run(5.0)], "the run survives the ClearSets");
    }

    #[test]
    fn only_the_last_profile_survives() {
        let out = compact_event_log(vec![profile(10), set("a"), profile(20)]);
        assert_eq!(
            out,
            vec![set("a"), profile(20)],
            "earlier profile is dropped"
        );
    }

    #[test]
    fn only_the_last_review_survives() {
        let out = compact_event_log(vec![review(1), review(2), review(3)]);
        assert_eq!(out, vec![review(3)]);
    }

    #[test]
    fn empty_log_stays_empty() {
        assert!(compact_event_log(vec![]).is_empty());
    }

    #[test]
    fn replay_equivalence_across_interleaved_families() {
        assert_replay_equivalent(vec![
            profile(10),
            readiness(-1.0),
            set("a"),
            run(5.0),
            Event::ClearReadiness,
            readiness(-2.0),
            set("b"),
            Event::ClearSets,
            set("c"),
            run(6.0),
            Event::ClearRuns,
            run(7.0),
            review(1),
            profile(20),
            review(2),
        ]);
    }

    #[test]
    fn replay_equivalence_when_a_clear_wipes_a_safety_hold() {
        // A Pain flag blocks training; ClearReadiness must lift it identically
        // whether or not the pre-clear inputs were compacted away.
        assert_replay_equivalent(vec![
            Event::SubmitReadiness(ReadinessInput {
                signal: ReadinessSignal::Pain,
                value: 1.0,
                observed_at: 0,
                streak: 0,
                pain: None,
                effort_min: None,
            }),
            Event::ClearReadiness,
            readiness(-1.5),
        ]);
    }

    fn pain() -> Event {
        Event::SubmitReadiness(ReadinessInput {
            signal: ReadinessSignal::Pain,
            value: 1.0,
            observed_at: 0,
            streak: 0,
            pain: None,
            effort_min: None,
        })
    }

    fn remove(signal: ReadinessSignal) -> Event {
        Event::RemoveReadiness { signal }
    }

    #[test]
    fn remove_readiness_cancels_its_submit_pairwise() {
        // The mis-tap undo: pain + its removal are inert; the wellness input
        // logged in between must survive untouched.
        let out = compact_event_log(vec![pain(), readiness(-1.5), remove(ReadinessSignal::Pain)]);
        assert_eq!(out, vec![readiness(-1.5)]);
        assert_replay_equivalent(vec![pain(), readiness(-1.5), remove(ReadinessSignal::Pain)]);
    }

    #[test]
    fn remove_readiness_cancels_only_the_latest_matching_submit() {
        // Two pain reports, one undo: the earlier report must still replay
        // (and still hard-stop training).
        let events = vec![pain(), pain(), remove(ReadinessSignal::Pain)];
        let out = compact_event_log(events.clone());
        assert_eq!(out, vec![pain()]);
        assert_replay_equivalent(events);
        assert!(replay(&[pain()]).train_blocked);
    }

    #[test]
    fn unmatched_remove_readiness_is_dropped_alone() {
        // A remove with no prior matching submit replays as a no-op: dropping
        // it (and nothing else) is replay-equivalent, also across a clear.
        let across_clear = vec![pain(), Event::ClearReadiness, remove(ReadinessSignal::Pain)];
        assert!(compact_event_log(across_clear.clone()).is_empty());
        assert_replay_equivalent(across_clear);

        let before_submit = vec![remove(ReadinessSignal::Pain), pain()];
        assert_eq!(compact_event_log(before_submit.clone()), vec![pain()]);
        assert_replay_equivalent(before_submit);
    }

    #[test]
    fn replay_equivalence_preserves_run_spike_baseline() {
        // A run's longest_recent_km is derived from prior runs at update time;
        // compaction across a ClearRuns must not perturb the surviving runs'
        // spike baseline.
        assert_replay_equivalent(vec![
            run(20.0),
            run(5.0),
            Event::ClearRuns,
            run(8.0),
            run(30.0),
        ]);
    }

    fn predict(goal_distance_m: f64) -> Event {
        Event::PredictRace {
            recent_distance_m: 5000.0,
            recent_time_sec: 1200.0,
            goal_distance_m,
            weekly_km: 40.0,
            weeks_since_race: None,
        }
    }

    fn plan(muscle: &str) -> Event {
        Event::PlanHypertrophyMeso {
            muscle: muscle.into(),
            weeks: 4,
            not_growing: false,
            recovering_easily: false,
        }
    }

    fn protein(bodyweight_kg: f64) -> Event {
        Event::ComputeProtein {
            bodyweight_kg,
            masters: false,
            deficit: true,
        }
    }

    fn hr_zones(age_years: f64) -> Event {
        Event::ComputeHrZones {
            age_years,
            resting_hr_bpm: None,
            weeks_since_recalc: None,
            weeks_since_pace_test: None,
        }
    }

    #[test]
    fn only_the_last_race_prediction_survives() {
        // PredictRace is a last-write-wins singleton (family 5); an earlier query
        // is provably inert once a later one replaces model.race_query.
        let out = compact_event_log(vec![predict(10_000.0), plan("chest"), predict(21_097.5)]);
        assert_eq!(out, vec![plan("chest"), predict(21_097.5)]);
    }

    #[test]
    fn only_the_last_protein_target_survives() {
        // ComputeProtein is a last-write-wins singleton (family 7); an earlier
        // query is provably inert once a later one replaces model.protein_query.
        let out = compact_event_log(vec![protein(70.0), plan("chest"), protein(90.0)]);
        assert_eq!(out, vec![plan("chest"), protein(90.0)]);
    }

    #[test]
    fn clear_wipes_the_protein_family() {
        // A trailing ClearProtein (family 7) supersedes every ComputeProtein
        // before it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![protein(70.0), protein(90.0), Event::ClearProtein]);
        assert!(out.is_empty());
    }

    #[test]
    fn clear_wipes_the_hypertrophy_plan_family() {
        // A trailing ClearHypertrophyPlan (family 6) supersedes every plan before
        // it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![
            plan("chest"),
            plan("back"),
            Event::ClearHypertrophyPlan,
        ]);
        assert!(out.is_empty());
    }

    #[test]
    fn only_the_last_hr_zone_query_survives() {
        // ComputeHrZones is a last-write-wins singleton (family 8); an earlier
        // query is provably inert once a later one replaces model.hr_zone_query.
        let out = compact_event_log(vec![hr_zones(30.0), plan("chest"), hr_zones(40.0)]);
        assert_eq!(out, vec![plan("chest"), hr_zones(40.0)]);
    }

    #[test]
    fn clear_wipes_the_hr_zone_family() {
        // A trailing ClearHrZones (family 8) supersedes every ComputeHrZones
        // before it, leaving nothing to replay in that family.
        let out = compact_event_log(vec![hr_zones(30.0), hr_zones(40.0), Event::ClearHrZones]);
        assert!(out.is_empty());
    }

    fn checkin(observed_at: i64) -> Event {
        Event::SubmitCheckin(crate::schema::CheckinInput {
            observed_at,
            sleep_quality: Some(3),
            soreness: Some(3),
            mood: Some(3),
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        })
    }

    /// A resting-HR-ONLY check-in (no wellness items): exercises the RestingHr
    /// channel in isolation for the per-channel Rule 4 window (F1).
    fn checkin_rhr(observed_at: i64, rhr: f64) -> Event {
        Event::SubmitCheckin(crate::schema::CheckinInput {
            observed_at,
            sleep_quality: None,
            soreness: None,
            mood: None,
            resting_hr_bpm: Some(rhr),
            hrv_rmssd_ms: None,
        })
    }

    /// A sleep-ONLY check-in (no resting-HR/HRV): the wellness channel in
    /// isolation for the per-channel Rule 4 window (F1).
    fn checkin_sleep(observed_at: i64, sleep: u8) -> Event {
        Event::SubmitCheckin(crate::schema::CheckinInput {
            observed_at,
            sleep_quality: Some(sleep),
            soreness: None,
            mood: None,
            resting_hr_bpm: None,
            hrv_rmssd_ms: None,
        })
    }

    #[test]
    fn checkins_are_retained_history_only_a_clear_wipes() {
        // Family 12 is NOT day-scoped: multiple check-ins across days all survive
        // (they ARE the rolling baseline). Only a ClearCheckins supersedes them.
        let out = compact_event_log(vec![checkin(0), checkin(DAY), checkin(2 * DAY)]);
        assert_eq!(out.len(), 3, "every check-in is retained history");

        let cleared = compact_event_log(vec![checkin(0), checkin(DAY), Event::ClearCheckins]);
        assert!(cleared.is_empty(), "a trailing ClearCheckins wipes the family");

        let after = compact_event_log(vec![checkin(0), Event::ClearCheckins, checkin(DAY)]);
        assert_eq!(after, vec![checkin(DAY)], "a check-in after the clear survives");
    }

    #[test]
    fn clear_checkins_only_touches_its_own_family() {
        // A ClearCheckins must not disturb readiness/sets/runs.
        let out = compact_event_log(vec![
            readiness(-1.0),
            set("a"),
            checkin(0),
            Event::ClearCheckins,
        ]);
        assert_eq!(out, vec![readiness(-1.0), set("a")]);
    }

    #[test]
    fn rule4_drops_checkins_older_than_the_retention_window() {
        // Newest check-in is day 60; the window is 45 days, so the cutoff is
        // day 15. Day 0 (60 days old) is dropped; day 20 (40 days old, inside
        // 45) survives; day 60 survives.
        let out = compact_event_log(vec![checkin(0), checkin(20 * DAY), checkin(60 * DAY)]);
        assert_eq!(out, vec![checkin(20 * DAY), checkin(60 * DAY)]);
    }

    #[test]
    fn rule4_keeps_the_checkin_exactly_at_the_window_edge() {
        // observed_at == cutoff (newest − 45 days) is NOT strictly older, so the
        // boundary check-in is retained: the window is inclusive at its edge.
        let out = compact_event_log(vec![checkin(0), checkin(45 * DAY)]);
        assert_eq!(
            out,
            vec![checkin(0), checkin(45 * DAY)],
            "the check-in exactly 45 days back is retained"
        );
    }

    #[test]
    fn rule4_is_a_noop_without_checkins() {
        // No family-12 line → no reference point → nothing dropped by Rule 4.
        let out = compact_event_log(vec![run(5.0), readiness(-1.0)]);
        assert_eq!(out, vec![run(5.0), readiness(-1.0)]);
    }

    #[test]
    fn rule4_anchors_on_the_newest_surviving_checkin_not_a_cleared_one() {
        // A ClearCheckins wipes the pre-clear history (Rule 1); Rule 4 then
        // anchors on the post-clear check-in, so a lone recent check-in after a
        // clear is never mistaken for "stale" against a wiped older reference.
        let out = compact_event_log(vec![
            checkin(0),
            checkin(DAY),
            Event::ClearCheckins,
            checkin(100 * DAY),
        ]);
        assert_eq!(out, vec![checkin(100 * DAY)]);
    }

    #[test]
    fn replay_equivalence_drops_stale_checkins() {
        // A full baseline week on days 0..=6, then a fresh burst on days 60..=66.
        // The old week is >45 days before the newest check-in, so Rule 4 drops it
        // and since derive_readiness only reads the trailing 30 days, the
        // replayed view is byte-identical before and after compaction.
        let mut events = Vec::new();
        for d in 0..7 {
            events.push(checkin(d * DAY));
        }
        for d in 60..67 {
            events.push(checkin(d * DAY));
        }
        let compacted = compact_event_log(events.clone());
        assert!(
            compacted.len() < events.len(),
            "the stale week must be dropped"
        );
        assert_replay_equivalent(events);
    }

    #[test]
    fn rule4_per_channel_window_keeps_a_sparse_channel_alive() {
        // F1 replay-equivalence guard. `derive_readiness` windows EACH channel on
        // ITS OWN newest reading (per_day_series), not the log's global newest.
        // Eight resting-HR-ONLY check-ins on days 0..=7 build a full RHR baseline
        // (>= MIN_BASELINE_CHECKINS), then sleep-ONLY check-ins on days 60..=63
        // carry NO resting-HR. Anchoring Rule 4 on the GLOBAL newest (day 63)
        // would drop the whole RHR week (>45 days back) and silently vanish the
        // RestingHr readiness row on the next launch. The per-channel anchor
        // (min channel newest = day 7 for RHR) keeps them, so replay is identical.
        let mut events = Vec::new();
        for d in 0..8 {
            events.push(checkin_rhr(d * DAY, 50.0 + d as f64));
        }
        for d in 60..64 {
            events.push(checkin_sleep(d * DAY, 3));
        }
        let out = compact_event_log(events.clone());
        let rhr_kept = out
            .iter()
            .filter(|e| matches!(e, Event::SubmitCheckin(c) if c.resting_hr_bpm.is_some()))
            .count();
        assert_eq!(
            rhr_kept, 8,
            "all 8 resting-HR check-ins must survive - their channel still reads them"
        );
        // The full stream really does derive a RestingHr input (else the guard is
        // vacuous); compaction must not change the rendered view.
        assert_replay_equivalent(events);
    }

    #[test]
    fn replay_equivalence_with_checkins_interleaved() {
        assert_replay_equivalent(vec![
            checkin(0),
            readiness(-1.0),
            checkin(DAY),
            set("a"),
            Event::ClearCheckins,
            checkin(2 * DAY),
            run(5.0),
        ]);
    }

    fn generate_plan(start: i64) -> Event {
        Event::GeneratePlan { start_epoch_day: start }
    }

    fn set_today(day: i64) -> Event {
        Event::SetToday {
            epoch_day: day,
            utc_offset_sec: 0,
        }
    }

    #[test]
    fn only_the_last_plan_request_survives() {
        // GeneratePlan is a last-write-wins singleton (family 13); an earlier
        // request is provably inert once a later one replaces model.plan_request.
        let out = compact_event_log(vec![generate_plan(1), plan("chest"), generate_plan(8)]);
        assert_eq!(out, vec![plan("chest"), generate_plan(8)]);
    }

    #[test]
    fn clear_wipes_the_plan_family() {
        let out = compact_event_log(vec![generate_plan(1), generate_plan(8), Event::ClearPlan]);
        assert!(out.is_empty(), "a trailing ClearPlan wipes the family");
    }

    #[test]
    fn only_the_last_set_today_survives() {
        // SetToday is a last-write-wins singleton (family 14) sent on every
        // foreground: compaction keeps exactly one line, so the log never bloats.
        let out = compact_event_log(vec![set_today(1), set_today(2), set_today(3)]);
        assert_eq!(out, vec![set_today(3)]);
    }

    #[test]
    fn replay_equivalence_across_plan_and_today_families() {
        assert_replay_equivalent(vec![
            profile(12),
            set_today(1),
            generate_plan(1),
            set_today(2),
            generate_plan(2),
            Event::ClearPlan,
            generate_plan(3),
            set_today(3),
        ]);
    }

    #[test]
    fn replay_equivalence_across_new_singleton_families() {
        // Race prediction (5) and hypertrophy plan (6) interleaved with their
        // clears must replay identically before and after compaction.
        assert_replay_equivalent(vec![
            predict(10_000.0),
            plan("chest"),
            predict(21_097.5),
            Event::ClearRacePrediction,
            plan("back"),
            predict(42_195.0),
        ]);
    }

    // ── Rule 3: DeleteEntry / AmendSet / AmendRun (Phase 4 / M4) ──────────────

    fn set_id(exercise: &str, id: u64, observed_at: i64) -> Event {
        Event::LogSet {
            exercise: exercise.into(),
            weight_kg: 100.0,
            reps: 5,
            rpe: 8.0,
            observed_at,
            entry_id: id,
        }
    }

    fn run_id(distance_km: f64, id: u64, observed_at: i64) -> Event {
        Event::LogRun {
            distance_km,
            duration_min: distance_km * 5.0,
            hr_pct_max: 75.0,
            longest_recent_km: 0.0,
            observed_at,
            entry_id: id,
            workout_type: None,
        }
    }

    fn del_set(id: u64) -> Event {
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Set,
            entry_id: id,
            observed_at_fallback: 0,
        }
    }

    fn del_run(id: u64) -> Event {
        Event::DeleteEntry {
            kind: crate::app::EntryKind::Run,
            entry_id: id,
            observed_at_fallback: 0,
        }
    }

    fn amend_set(id: u64, weight_kg: f64) -> Event {
        Event::AmendSet {
            entry_id: id,
            exercise: "Bench".into(),
            weight_kg,
            reps: 5,
            rpe: 8.0,
            observed_at: 0,
            observed_at_fallback: 0,
        }
    }

    #[test]
    fn delete_cancels_its_logged_set_pairwise() {
        // A set logged then deleted is inert; an unrelated run survives.
        let events = vec![set_id("Bench", 7, 0), run(5.0), del_set(7)];
        let out = compact_event_log(events.clone());
        assert_eq!(out, vec![run(5.0)], "the set+delete pair is dropped");
        assert_replay_equivalent(events);
    }

    #[test]
    fn delete_targets_the_newest_matching_row() {
        // Two sets share an id; the delete removes the newest: the earlier
        // survives, exactly as `update` (find_set = rposition) would.
        let events = vec![set_id("A", 3, 0), set_id("B", 3, DAY), del_set(3)];
        let out = compact_event_log(events.clone());
        assert_eq!(out, vec![set_id("A", 3, 0)]);
        assert_replay_equivalent(events);
    }

    #[test]
    fn amend_supersedes_the_prior_set_but_survives() {
        // B8: a strict amend needs its base row on replay, so Log→Amend keeps BOTH
        // the base log AND the amend (replay-equivalent to editing the set in
        // place). The amend keeps the entry's id for later edits.
        let events = vec![set_id("Bench", 5, 0), amend_set(5, 120.0)];
        let out = compact_event_log(events.clone());
        assert_eq!(
            out,
            vec![set_id("Bench", 5, 0), amend_set(5, 120.0)],
            "the base log survives so the strict amend can replace it on replay"
        );
        assert_replay_equivalent(events);
    }

    #[test]
    fn amend_chain_collapses_to_log_plus_last_amend() {
        // B8: a Log→Amend→Amend chain (no delete) collapses to `[log, last amend]`
        // the superseded middle amend is dropped, the base log is retained so the
        // strict final amend has a row to replace on replay.
        let events = vec![set_id("Bench", 9, 0), amend_set(9, 110.0), amend_set(9, 130.0)];
        let out = compact_event_log(events.clone());
        assert_eq!(
            out,
            vec![set_id("Bench", 9, 0), amend_set(9, 130.0)],
            "the middle amend is superseded; the base log and last amend remain"
        );
        assert_replay_equivalent(events);
    }

    #[test]
    fn amend_after_delete_never_resurrects_the_entry() {
        // B8 core prevention: Log→Delete→Amend, where the amend targets the row the
        // delete already removed. The delete cancels the entry (log + itself); the
        // amend then matches nothing and is dropped: the entry STAYS deleted, both
        // in a live `update` replay and after compaction. Before B8, the amend
        // replayed as a push and RESURRECTED the deleted row.
        let events = vec![set_id("Bench", 5, 0), del_set(5), amend_set(5, 120.0)];
        // Live replay: the amend is a no-op on the missing row → nothing survives.
        assert!(replay(&events).lifts.is_empty(), "the deleted set is not resurrected");
        // Compaction: the whole sequence is inert.
        assert!(
            compact_event_log(events.clone()).is_empty(),
            "delete cancels the entry; the orphan amend is dropped"
        );
        assert_replay_equivalent(events);

        // Same for a run, and with an unrelated survivor in between.
        let run_events = vec![
            run_id(5.0, 2, 0),
            run_id(8.0, 3, DAY),
            del_run(3),
            Event::AmendRun {
                entry_id: 3,
                distance_km: 9.0,
                duration_min: 45.0,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: DAY,
                observed_at_fallback: 0,
                workout_type: None,
            },
        ];
        assert_eq!(
            compact_event_log(run_events.clone()),
            vec![run_id(5.0, 2, 0)],
            "only the unrelated run survives; the deleted run is not resurrected"
        );
        assert_replay_equivalent(run_events);
    }

    #[test]
    fn amend_chain_collapses_to_the_last_amend() {
        // Log→Amend→Amend→Delete is fully inert.
        let events = vec![
            set_id("Bench", 9, 0),
            amend_set(9, 110.0),
            amend_set(9, 130.0),
            del_set(9),
        ];
        let out = compact_event_log(events.clone());
        assert!(out.is_empty(), "the whole edit chain cancels away");
        assert_replay_equivalent(events);
    }

    #[test]
    fn unmatched_delete_is_dropped_alone() {
        // A delete with no matching prior line replays as a no-op → dropped, and
        // nothing else is touched.
        let before = vec![del_set(42), set_id("Bench", 42, 0)];
        assert_eq!(compact_event_log(before.clone()), vec![set_id("Bench", 42, 0)]);
        assert_replay_equivalent(before);

        let orphan = vec![run(5.0), del_set(1)];
        assert_eq!(compact_event_log(orphan.clone()), vec![run(5.0)]);
        assert_replay_equivalent(orphan);
    }

    #[test]
    fn delete_across_a_clear_stays_replay_equivalent() {
        // The set is cleared before the delete ever runs; the delete is a no-op.
        // Rule 3 may match the pre-clear line, but Rule 1 sweeps it regardless.
        let events = vec![set_id("Bench", 4, 0), Event::ClearSets, del_set(4)];
        assert!(compact_event_log(events.clone()).is_empty());
        assert_replay_equivalent(events);

        // A re-logged post-clear set with the same id is the real delete target.
        let relog = vec![
            set_id("Bench", 4, 0),
            Event::ClearSets,
            set_id("Bench", 4, DAY),
            del_set(4),
        ];
        assert!(compact_event_log(relog.clone()).is_empty());
        assert_replay_equivalent(relog);
    }

    #[test]
    fn legacy_row_without_id_is_matched_on_observed_at() {
        // A pre-Phase-4 set (entry_id 0) is deleted by its observed_at fallback.
        let events = vec![
            set_id("Old", 0, 5 * DAY),
            Event::DeleteEntry {
                kind: crate::app::EntryKind::Set,
                entry_id: 0,
                observed_at_fallback: 5 * DAY,
            },
        ];
        assert!(compact_event_log(events.clone()).is_empty());
        assert_replay_equivalent(events);
    }

    #[test]
    fn run_delete_and_amend_replay_equivalent() {
        let deleted = vec![run_id(5.0, 2, 0), run_id(8.0, 3, DAY), del_run(3)];
        assert_eq!(compact_event_log(deleted.clone()), vec![run_id(5.0, 2, 0)]);
        assert_replay_equivalent(deleted);

        let amended = vec![
            run_id(5.0, 6, 0),
            Event::AmendRun {
                entry_id: 6,
                distance_km: 6.0,
                duration_min: 30.0,
                hr_pct_max: 0.0,
                longest_recent_km: 0.0,
                observed_at: 0,
                observed_at_fallback: 0,
                workout_type: None,
            },
        ];
        // B8: the base run log is KEPT (the strict amend replaces it on replay);
        // the amend survives too, so `[log, amend]` both remain.
        assert_eq!(compact_event_log(amended.clone()).len(), 2);
        assert_replay_equivalent(amended);
    }

    // A legacy (pre-Phase-4, entry_id 0) set log + amend, matched on observed_at.
    fn legacy_set(observed_at: i64, weight_kg: f64) -> Event {
        Event::LogSet {
            exercise: "Bench".into(),
            weight_kg,
            reps: 5,
            rpe: 8.0,
            observed_at,
            entry_id: 0,
        }
    }

    fn legacy_amend(observed_at: i64, observed_at_fallback: i64, weight_kg: f64) -> Event {
        Event::AmendSet {
            entry_id: 0,
            exercise: "Bench".into(),
            weight_kg,
            reps: 5,
            rpe: 8.0,
            observed_at,
            observed_at_fallback,
        }
    }

    #[test]
    fn rule3_walk_assumes_legacy_rows_are_never_redated() {
        // F2 REGRESSION GUARD (latent, shell-blocked). Rule 3's chain-walk keys a
        // whole legacy (entry_id 0) set/run chain by ONE (fam, id, observed_at)
        // match key: correct ONLY while a legacy row's date is immutable, which
        // the shell enforces (LogEntry.kt pins observed_at == observed_at_fallback
        // for id-0 rows, blocking re-dating). This guard pins that invariant: if a
        // future change lets legacy rows be re-dated, the `assert_ne!`s below start
        // failing and force revisiting Rule 3 and the shell pin TOGETHER.

        // PINNED (supported): every link shares one date (observed_at ==
        // observed_at_fallback == DAY) → the legacy chain collapses
        // replay-equivalently, exactly like an id-bearing one.
        assert_replay_equivalent(vec![
            legacy_set(DAY, 100.0),
            legacy_amend(DAY, DAY, 110.0),
            legacy_amend(DAY, DAY, 130.0),
        ]);

        // RE-DATED Log→Amend→Amend (unsupported): each amend moves the row's date,
        // so the single-key walk telescopes the middle re-date link away and the
        // final amend then matches nothing on replay → reverts to the ORIGINAL
        // value after compaction. The full and compacted views MUST differ today.
        let redated_chain = vec![
            legacy_set(DAY, 100.0),
            legacy_amend(2 * DAY, DAY, 110.0),     // re-date DAY → 2*DAY
            legacy_amend(3 * DAY, 2 * DAY, 130.0), // re-date 2*DAY → 3*DAY
        ];
        assert_ne!(
            replay(&redated_chain),
            replay(&compact_event_log(redated_chain.clone())),
            "legacy re-dating became replay-safe - update Rule 3 AND remove the \
             LogEntry.kt pin together (see the F2 invariant comment)"
        );

        // RE-DATED Log→Amend→Delete (unsupported): the delete's fallback matches
        // the re-dated amend but its base-chain walk (same key) can't reach the
        // differently-dated base log, so the base log survives compaction and the
        // deleted row is RESURRECTED. Full and compacted views MUST differ today.
        let redated_delete = vec![
            legacy_set(DAY, 100.0),
            legacy_amend(2 * DAY, DAY, 110.0), // re-date DAY → 2*DAY
            Event::DeleteEntry {
                kind: crate::app::EntryKind::Set,
                entry_id: 0,
                observed_at_fallback: 2 * DAY, // delete the re-dated (current) row
            },
        ];
        assert_ne!(
            replay(&redated_delete),
            replay(&compact_event_log(redated_delete.clone())),
            "legacy re-dating became replay-safe - update Rule 3 AND remove the \
             LogEntry.kt pin together (see the F2 invariant comment)"
        );
    }

    #[test]
    fn delete_and_amend_interleaved_with_everything_replay_equivalent() {
        assert_replay_equivalent(vec![
            profile(10),
            set_id("Bench", 1, 0),
            run_id(5.0, 2, 0),
            amend_set(1, 120.0),
            set_id("Squat", 3, DAY),
            del_set(3),
            run_id(8.0, 4, DAY),
            del_run(2),
            Event::ClearSets,
            set_id("Deadlift", 5, 2 * DAY),
            amend_set(5, 200.0),
        ]);
    }
}
