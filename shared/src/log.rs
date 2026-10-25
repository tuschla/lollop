//! Event-log compaction. Shells persist the raw [`Event`] stream and rebuild
//! state by replaying it (the core keeps no durable state), so the log grows
//! without bound, a GPS-tracked run alone is thousands of points on one line.
//!
//! [`compact_event_log`] drops lines whose effect on the replayed model is
//! provably nil, returning a shorter but **replay-equivalent** stream (same
//! relative order of survivors). Five rules (0, 1, 2, 2b, 3), all grounded in
//! [`crate::app`]'s `update`, where the model stores only raw inputs and no
//! event's `update` reads another family's state, except for figures that are a
//! pure function of the surviving model and are re-derived whenever their inputs
//! change (a run's typed-bpm %HRmax from the profile in force at its instant, its
//! card spike baseline from the runs around it), so dropping a line those
//! inputs no longer hold cannot change them:
//!
//! 0. **An undated `RemoveReadiness` cancels its submit.** It deletes exactly
//!    the latest prior active `SubmitReadiness` with the same signal, so the
//!    pair is inert; an unmatched remove replays as a no-op and is dropped
//!    alone. A DATED remove (`observed_at > 0`) retracts rather than deletes:
//!    the input stays in the model for the past days it shaped, so the remove
//!    and its submit both survive, and the submit it retracted is no longer a
//!    candidate for a later remove. A dated `ResolveHold` retracts EVERY active
//!    submit of its signal the same way (all survive, all consumed); an undated
//!    or unmatched one replays as a no-op and is dropped alone.
//! 3. **A `DeleteEntry` cancels its entry; an `AmendSet`/`AmendRun` supersedes a
//!    prior edit but keeps the entry's base log.** Amend is a STRICT update in
//!    `update`: it replaces a matching row and is a NO-OP on a miss, so an
//!    amend whose target was already deleted can no longer resurrect it. To stay
//!    replay-equivalent, a surviving amend must keep its base log line (it drops
//!    only a *superseded prior amend*), so a Log→Amend→Amend chain collapses to
//!    `[log, last amend]`. A Delete removes the whole entry, its newest matching
//!    line and, when that is an amend, the base chain down to and including the
//!    log, plus itself. An unmatched delete OR amend replays as a no-op and is
//!    dropped alone. Runs before Rule 1.
//!    A `TrimRunTrack` slices a GPS run's track IN PLACE; all trim lines survive
//!    compaction (replay is sequential slicing), the base `LogRunTrack` stays
//!    alive, and a `DeleteEntry`/`ClearRuns` cancels the base and its trims.
//!    An `AmendRunMeta` (rating/tag edit in place) is handled exactly like a trim.
//!    A logged activity (`LogActivity`, family 15) has no amend: a
//!    `DeleteEntry { kind: Activity }` cancels its newest matching log line
//!    pairwise, and a `LogActivity` that fails `update`'s minutes gate has no
//!    identity (never a delete target).
//! 1. **A `Clear<F>` supersedes its family.** It empties family `F`'s vec, so
//!    every `F` event (and the clear) at or before the *last* `Clear<F>` leaves
//!    no residue, `F` events after it replay against an already-empty vec.
//!    The review family is the exception, see Rule 2. One clear line is kept:
//!    the last `ClearPlan` when no `GeneratePlan` follows it (a no-op on
//!    replay, but the durable record that the user removed their plan).
//! 2. **Last write wins for singletons; profile history is eff-keyed; reviews
//!    are a history.** The calculators assign a `model` field outright, so only
//!    the last surviving one matters. Every `SubmitReview` and
//!    `SubmitWeeklyCheckin` is kept: a weekly check-in (or the weekly part of a
//!    legacy review) is appended to `weekly_history`, which dates past days'
//!    deload and downgrade windows. `ClearReview` empties only the session
//!    review and `ClearWeeklyCheckin` only the weekly check-in in force, so
//!    neither sweeps history; a clear is dropped only when a later line
//!    replaces the same store anyway (a later clear of it, or a later payload
//!    that sets it). Reviews are rare (one per session or week), so keeping
//!    every line costs the log next to nothing. `SetProfile` is special: `update` keeps a
//!    `profile_history` keyed by (effective day, edit instant) and a SetProfile
//!    drops every entry at or after its own key, so a SetProfile line is dead
//!    only when a LATER one has a key `<=` it (`None` = −∞); ascending profiles
//!    all survive, a `None`-dated edit replaces the whole history, and one that
//!    pins logged runs' %HRmax (a run line survives before it) is a barrier no
//!    later line kills an earlier one across. `SetExerciseEnded` (family 1)
//!    additionally last-write-wins by exact `(exercise, epoch_day)` key.
//!
//!    2b. **The plan request is keep-FIRST.** `update`'s `GeneratePlan` arm
//!    keeps the original plan anchor and ignores every later request, so only
//!    the first surviving `GeneratePlan` matters.
//!
//! **Check-ins (family 12) are never windowed.** The view reads a check-in
//! whenever it is its channel's newest reading on or before the day being
//! judged, and a later `SetToday` can move today back to any reading's day, so
//! no check-in outside a `ClearCheckins` (Rule 1) is provably inert for the
//! events a launch-time compaction is followed by. (Check-ins cost about one
//! line a day, a small fraction of the never-compacted set log.) Family 0
//! readiness is not windowed either: a Pain/Illness hold lives until
//! explicitly cleared (HARD RULE 3).
//!
//! This is the authoritative implementation, and the replay-equivalence tests
//! (`log/tests.rs`) pin its correctness. Because it is *not* exposed over the
//! JSON FFI, the Kotlin Android shell reimplements the same rules over raw wire
//! lines in `android/.../EventLog.kt::compact` (called from `Core.kt`). The two
//! must stay in lockstep: any change to the families here (or a new `Event`
//! variant) must be mirrored there.

use crate::app::Event;

/// A compaction family: the events one clear resets. Discriminants are stable
/// and mirror the shell's `EventLog.kt::compact` family numbering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Family {
    Readiness = 0,
    Sets = 1,
    Runs = 2,
    Profile = 3,
    Review = 4,
    RacePrediction = 5,
    HypertrophyPlan = 6,
    Protein = 7,
    HrZones = 8,
    Cooper = 9,
    CriticalSpeed = 10,
    Apre = 11,
    Checkins = 12,
    Plan = 13,
    Today = 14,
    Activities = 15,
}

impl Family {
    /// Every family, in discriminant order.
    const ALL: [Family; 16] = [
        Family::Readiness,
        Family::Sets,
        Family::Runs,
        Family::Profile,
        Family::Review,
        Family::RacePrediction,
        Family::HypertrophyPlan,
        Family::Protein,
        Family::HrZones,
        Family::Cooper,
        Family::CriticalSpeed,
        Family::Apre,
        Family::Checkins,
        Family::Plan,
        Family::Today,
        Family::Activities,
    ];
}

/// Family + `is_clear` for one event. The `match` is exhaustive (no `_`) so a
/// new [`Event`] variant fails to compile until it is placed in a family, a
/// mis-filed variant would silently corrupt compaction.
fn classify(event: &Event) -> (Family, bool) {
    use Family as F;
    match event {
        Event::SubmitReadiness(_) => (F::Readiness, false),
        Event::RemoveReadiness { .. } => (F::Readiness, false),
        Event::ResolveHold { .. } => (F::Readiness, false),
        Event::ClearReadiness => (F::Readiness, true),
        Event::LogSet { .. } => (F::Sets, false),
        Event::AmendSet { .. } => (F::Sets, false),
        Event::ClearSets => (F::Sets, true),
        // An "end exercise" marker belongs to the SETS family, so a later
        // ClearSets drops earlier markers (Rule 1). It ALSO last-write-wins by exact
        // key (exercise.trim().lowercase(), epoch_day) via a dedicated pass below.
        Event::SetExerciseEnded { .. } => (F::Sets, false),
        // A calisthenics rung placement steers the ladders the sets build: SETS
        // family, so a later ClearSets drops it (Rule 1). Every placement is
        // history (a past day's rung depends on the placements before it), so
        // all of them otherwise survive.
        Event::SetCalisthenicsRung { .. } => (F::Sets, false),
        Event::LogRun { .. } | Event::LogRunTrack { .. } => (F::Runs, false),
        Event::AmendRun { .. } => (F::Runs, false),
        // A trim edits a GPS run's track in place; family 2, ClearRuns clears it.
        Event::TrimRunTrack { .. } => (F::Runs, false),
        // A meta edit (CR-10 rating + workout tag) edits a run in place like a
        // trim; family 2, ClearRuns clears it.
        Event::AmendRunMeta { .. } => (F::Runs, false),
        Event::ClearRuns => (F::Runs, true),
        // Other activities (the under-18 WHO card): a plain log/delete/clear
        // family with no amend and no cross-entry baked state.
        Event::LogActivity { .. } => (F::Activities, false),
        Event::ClearActivities => (F::Activities, true),
        // A DeleteEntry joins the family it targets (set→1, run→2, activity→15).
        // Not a clear:
        // Rule 3 pairs it with its matched log line, and Rule 1 sweeps it away if
        // it sits at/before a later Clear<F> like any other family member.
        Event::DeleteEntry { kind, .. } => (entry_kind_family(*kind), false),
        Event::SetProfile(_) => (F::Profile, false),
        Event::ClearProfile => (F::Profile, true),
        Event::SubmitReview(_) => (F::Review, false),
        Event::ClearReview => (F::Review, true),
        // The weekly check-in shares the review family: its history is never
        // swept (Rule 1 exemption) and the Rule 2 review pass handles its clear.
        Event::SubmitWeeklyCheckin(_) => (F::Review, false),
        Event::ClearWeeklyCheckin => (F::Review, true),
        Event::PredictRace { .. } => (F::RacePrediction, false),
        Event::ClearRacePrediction => (F::RacePrediction, true),
        Event::PlanHypertrophyMeso { .. } => (F::HypertrophyPlan, false),
        Event::ClearHypertrophyPlan => (F::HypertrophyPlan, true),
        Event::ComputeProtein { .. } => (F::Protein, false),
        Event::ClearProtein => (F::Protein, true),
        Event::ComputeHrZones { .. } => (F::HrZones, false),
        Event::ClearHrZones => (F::HrZones, true),
        Event::ComputeCooper { .. } => (F::Cooper, false),
        Event::ClearCooper => (F::Cooper, true),
        Event::ComputeCriticalSpeed { .. } => (F::CriticalSpeed, false),
        Event::ClearCriticalSpeed => (F::CriticalSpeed, true),
        Event::ComputeApre { .. } => (F::Apre, false),
        Event::ClearApre => (F::Apre, true),
        // Morning check-ins: a RETAINED multi-day history the
        // core normalizes into z-scores/deltas. Deliberately NOT day-scoped like
        // readiness family 0; the whole point is a rolling baseline, so its
        // only reducer is a wholesale `ClearCheckins` (Rule 1). No per-entry undo
        // family-0 analog: check-ins carry no red-flag hold to mis-tap.
        Event::SubmitCheckin(_) => (F::Checkins, false),
        // Restarting the HRV baseline belongs to the check-in family: a later
        // ClearCheckins resets it in `update`, so Rule 1 may sweep it.
        Event::RestartHrvBaseline { .. } => (F::Checkins, false),
        Event::ClearCheckins => (F::Checkins, true),
        // Coach-as-planner. The plan family: the accepted plan request is
        // keep-FIRST (Rule 2b: `update` preserves the original plan anchor) with
        // a ClearPlan reset. The today family: SetToday is a last-write-wins
        // singleton (the shell's clock, sent on every foreground) with no clear;
        // compaction keeps exactly one line so it never bloats the log.
        Event::GeneratePlan { .. } => (F::Plan, false),
        Event::ClearPlan => (F::Plan, true),
        // A session move belongs to the plan family: `ClearPlan` clears the
        // moves with the plan (Rule 1). Every move is kept (the week's
        // arrangement replays them in order), so Rule 2b's keep-first pass
        // applies to `GeneratePlan` lines only.
        Event::MoveSession { .. } => (F::Plan, false),
        Event::SetToday { .. } => (F::Today, false),
    }
}

/// Compaction family of a [`crate::app::EntryKind`], used to place a
/// [`Event::DeleteEntry`] with its target family in Rule 3.
fn entry_kind_family(kind: crate::app::EntryKind) -> Family {
    match kind {
        crate::app::EntryKind::Set => Family::Sets,
        crate::app::EntryKind::Run => Family::Runs,
        crate::app::EntryKind::Activity => Family::Activities,
    }
}

/// `update`'s set-validity gate for `LogSet`/`AmendSet`: a non-negative (or NaN,
/// sanitized to 0 on store) weight and either reps or a hold time. The single
/// definition read by `update` and by compaction: compaction is replay-equivalent
/// only while both apply the same gate.
pub(crate) fn set_passes_gate(weight_kg: f64, reps: u32, hold_sec: Option<u16>) -> bool {
    (weight_kg >= 0.0 || weight_kg.is_nan()) && (reps > 0 || hold_sec.is_some())
}

/// Whether a `LogSet`/`AmendSet` passes [`set_passes_gate`] and a
/// `LogActivity` passes [`crate::app::activity_passes_gate`]. `update` drops an
/// invalid line as a NO-OP, so compaction must treat it the same: an invalid
/// `LogSet`/`LogActivity` carries no entry identity (it is never a delete/amend
/// target), and an invalid `AmendSet` supersedes nothing (dropped alone, like an
/// unmatched amend). Other events have no such gate (runs clamp rather than
/// reject) and are always valid here.
fn line_passes_gate(event: &Event) -> bool {
    match event {
        Event::LogSet {
            weight_kg,
            reps,
            hold_sec,
            ..
        }
        | Event::AmendSet {
            weight_kg,
            reps,
            hold_sec,
            ..
        } => set_passes_gate(*weight_kg, *reps, *hold_sec),
        Event::LogActivity { minutes, .. } => crate::app::activity_passes_gate(*minutes),
        _ => true,
    }
}

/// A logged-history line's `(family, entry_id, observed_at)` identity, or `None`
/// if the event is not a set/run/activity log-or-amend line. Rule 3 matches a
/// delete/amend against these, the same fields `update`'s `find_set`/`find_run`/
/// `find_activity` compare, with the same id clamp `update` applies on store and
/// match.
fn entry_identity(event: &Event) -> Option<(Family, u64, i64)> {
    // An invalid set or activity (fails `update`'s gate) never becomes a row, so
    // it has no identity: a later delete must skip it, exactly as `find_*` would.
    if !line_passes_gate(event) {
        return None;
    }
    let id = crate::app::sanitize_entry_id;
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
        } => Some((Family::Sets, id(*entry_id), *observed_at)),
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
        } => Some((Family::Runs, id(*entry_id), *observed_at)),
        // A trim or meta edit targets its run by (id, observed_at_fallback); it
        // has no observed_at of its own, so identity keys on the fallback (= the
        // run's observed_at, as the shell sends it), which lets a delete's
        // chain-walk find and cancel it.
        Event::TrimRunTrack {
            entry_id,
            observed_at_fallback,
            ..
        }
        | Event::AmendRunMeta {
            entry_id,
            observed_at_fallback,
            ..
        } => Some((Family::Runs, id(*entry_id), *observed_at_fallback)),
        Event::LogActivity {
            entry_id,
            observed_at,
            ..
        } => Some((Family::Activities, id(*entry_id), *observed_at)),
        _ => None,
    }
}

/// Whether `event` is an `AmendSet`/`AmendRun` (an *edit* of an existing entry),
/// as opposed to an original `LogSet`/`LogRun`/`LogRunTrack` (a base row). Rule 3
/// keeps a base log alive for a surviving amend but telescopes away a superseded
/// prior amend, and a delete walks the amend chain back to the base log.
fn is_amend_line(event: &Event) -> bool {
    matches!(event, Event::AmendSet { .. } | Event::AmendRun { .. })
}

/// Whether `event` is a `TrimRunTrack` (a head/tail slice of an existing GPS
/// run's track) or an `AmendRunMeta` (an in-place rating/tag edit). Both are
/// amend-LIKE for one purpose only — a delete's chain-walk treats them as
/// non-base lines so the walk reaches and cancels the base log line — but they
/// NEVER telescope: every such line survives compaction (replay = sequential
/// in-place edits), so they are deliberately excluded from [`is_amend_line`]
/// and the newest-amend-wins collapse.
fn is_trim_line(event: &Event) -> bool {
    matches!(
        event,
        Event::TrimRunTrack { .. } | Event::AmendRunMeta { .. }
    )
}

/// Whether `event` is the log/amend line a delete/amend targeting
/// `(fam, id, time)` matches: same family, and same id (nonzero) or same
/// `observed_at` for a legacy (`id == 0`) row. Mirrors `find_set`/`find_run`.
fn entry_line_matches(event: &Event, fam: Family, id: u64, time: i64) -> bool {
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

/// Singleton families whose members are last-write-wins. The profile family is
/// NOT here: it is history-keyed (a dedicated Rule 2 pass keeps the
/// key-subsequence that survives `update`'s `profile_history` reducer, not just
/// the last line). The plan family is NOT here either: `update`'s `GeneratePlan`
/// arm is first-write-wins (it preserves the original plan anchor), so it is
/// compacted keep-FIRST by Rule 2b, not last-write-wins. The review family
/// (every `SubmitReview` / `SubmitWeeklyCheckin` survives, since weekly
/// check-ins feed `weekly_history`) is neither swept by a clear (Rule 1) nor a
/// singleton; a dedicated Rule 2 pass drops only the provably inert clears.
const SINGLETONS: [Family; 8] = [
    Family::RacePrediction,
    Family::HypertrophyPlan,
    Family::Protein,
    Family::HrZones,
    Family::Cooper,
    Family::CriticalSpeed,
    Family::Apre,
    Family::Today,
];

/// Drop provably-inert events from a persisted log, preserving replay order.
#[must_use]
pub fn compact_event_log(events: Vec<Event>) -> Vec<Event> {
    let kinds: Vec<(Family, bool)> = events.iter().map(classify).collect();
    let mut remove = vec![false; events.len()];
    rule0_readiness_removes(&events, &mut remove);
    rule3_entry_edits(&events, &mut remove);
    rule1_clears(&kinds, &mut remove);
    rule1_keep_final_plan_clear(&events, &mut remove);
    rule2_singletons(&kinds, &mut remove);
    rule2_review_clears(&events, &mut remove);
    rule2_profile_history(&events, &mut remove);
    rule2_exercise_ended(&events, &mut remove);
    rule2b_plan_keep_first(&events, &kinds, &mut remove);
    events
        .into_iter()
        .zip(remove)
        .filter_map(|(e, removed)| (!removed).then_some(e))
        .collect()
}

/// Rule 0: an UNDATED RemoveReadiness deletes the latest active prior
/// SubmitReadiness carrying the same signal, so it cancels that line,
/// replay-equivalent because `update` drops exactly that input (rposition).
/// A DATED remove moves that input to `retracted_inputs` instead, so both
/// lines survive; its submit is marked consumed (no longer active), so a
/// later remove pairs with the next older one, as `update` does. An
/// unmatched remove of either kind replays as a no-op and is dropped alone.
/// Runs before Rule 1; the outcomes agree in every interleaving with
/// ClearReadiness because it wipes the active and the retracted inputs alike.
fn rule0_readiness_removes(events: &[Event], remove: &mut [bool]) {
    let mut consumed = vec![false; events.len()];
    for j in 0..events.len() {
        // ResolveHold: a dated one retracts EVERY active prior submit of its
        // signal (all lines survive, all consumed); undated or unmatched → no-op.
        if let Event::ResolveHold {
            signal,
            observed_at,
        } = &events[j]
        {
            let mut any = false;
            for i in 0..j {
                if !remove[i]
                    && !consumed[i]
                    && matches!(&events[i], Event::SubmitReadiness(input) if input.signal == *signal)
                {
                    any = true;
                    if *observed_at > 0 {
                        consumed[i] = true;
                    }
                }
            }
            if !any || *observed_at <= 0 {
                remove[j] = true;
            }
            continue;
        }
        let Event::RemoveReadiness {
            signal,
            observed_at,
        } = &events[j]
        else {
            continue;
        };
        let matched = (0..j).rev().find(|&i| {
            !remove[i]
                && !consumed[i]
                && matches!(&events[i], Event::SubmitReadiness(input) if input.signal == *signal)
        });
        match matched {
            Some(i) if *observed_at > 0 => consumed[i] = true,
            Some(i) => {
                remove[i] = true;
                remove[j] = true;
            }
            None => remove[j] = true,
        }
    }
}

/// The entry a `DeleteEntry` / `AmendSet` / `AmendRun` targets: its family and
/// the `(id, observed_at)` match key `update`'s `find_set`/`find_run` use.
struct EditTarget {
    fam: Family,
    id: u64,
    time: i64,
    is_delete: bool,
}

/// The [`EditTarget`] of a delete or amend line; `None` for every other event.
fn edit_target(event: &Event) -> Option<EditTarget> {
    // An amend targets its OLD row: for a legacy (`id == 0`) row whose date
    // CHANGED, that is `observed_at_fallback` (the old timestamp), not the
    // amend's new `observed_at`. Mirrors `update`'s `find_set`/`find_run` match
    // key so replay and compaction agree.
    let amend_time = |observed_at: i64, fallback: i64| {
        if fallback != 0 { fallback } else { observed_at }
    };
    let id = crate::app::sanitize_entry_id;
    Some(match event {
        Event::DeleteEntry {
            kind,
            entry_id,
            observed_at_fallback,
        } => EditTarget {
            fam: entry_kind_family(*kind),
            id: id(*entry_id),
            time: *observed_at_fallback,
            is_delete: true,
        },
        Event::AmendSet {
            entry_id,
            observed_at,
            observed_at_fallback,
            ..
        } => EditTarget {
            fam: Family::Sets,
            id: id(*entry_id),
            time: amend_time(*observed_at, *observed_at_fallback),
            is_delete: false,
        },
        Event::AmendRun {
            entry_id,
            observed_at,
            observed_at_fallback,
            ..
        } => EditTarget {
            fam: Family::Runs,
            id: id(*entry_id),
            time: amend_time(*observed_at, *observed_at_fallback),
            is_delete: false,
        },
        _ => return None,
    })
}

/// Rule 3: a Delete cancels its entry; an Amend supersedes a prior edit but
/// keeps the entry's base log. For each DeleteEntry / AmendSet / AmendRun, find
/// the newest not-yet-removed PRIOR log-or-amend line in the same family whose
/// identity matches (id if nonzero, else `observed_at` for a legacy row), the
/// same predicate `update` uses (`find_set`/`find_run`, newest-match).
///
/// `update`'s amend is a STRICT update (replace-on-match, NO-OP on miss), so a
/// lone amend with no base row replays to nothing. Two consequences, both
/// replay-equivalent:
/// - A matched Amend telescopes away a *superseded prior amend* but KEEPS the
///   base log line, so the strict amend has a row to replace on replay. A
///   Log→Amend→Amend chain collapses to `[log, last amend]`.
/// - A Delete removes the WHOLE entry: its newest matching line and, when that
///   is an amend, the base chain back through and INCLUDING the log, the one
///   row `update`'s delete removes, plus itself.
///
/// An unmatched delete OR amend replays as a no-op and is dropped alone. Runs
/// before Rule 1: a Clear then sweeps away anything still marked in-family
/// at/before it, absorbing any match made across a clear (that line was cleared
/// anyway).
///
/// Legacy-row invariant: the chain-walk depends on legacy (`entry_id == 0`)
/// rows NEVER being re-dated, the walk keys a whole set/run chain by ONE
/// `(fam, id, observed_at)` match key, which only stays valid while a legacy
/// row's `observed_at` is immutable. The shell enforces this (LogEntry.kt pins
/// `observed_at == observed_at_fallback` for id-0 rows, blocking re-dating). If
/// that pin is ever lifted, a re-dated legacy `Log→Amend→Amend` telescopes away
/// the intermediate re-date link (reverting to the original value on replay) and
/// a re-dated `Log→Amend→Delete` can resurrect the pre-edit row, see the
/// `rule3_walk_assumes_legacy_rows_are_never_redated` regression guard.
fn rule3_entry_edits(events: &[Event], remove: &mut [bool]) {
    for j in 0..events.len() {
        // An invalid `AmendSet` (fails `update`'s set gate) replays as a NO-OP,
        // exactly like an unmatched amend: it supersedes no prior edit. Drop it
        // alone before the telescope search so it cannot collapse a valid prior
        // amend into a silent revert on the next launch.
        if matches!(&events[j], Event::AmendSet { .. }) && !line_passes_gate(&events[j]) {
            remove[j] = true;
            continue;
        }
        let Some(target) = edit_target(&events[j]) else {
            continue;
        };
        let EditTarget { fam, id, time, .. } = target;
        let Some(i) = (0..j)
            .rev()
            .find(|&i| !remove[i] && entry_line_matches(&events[i], fam, id, time))
        else {
            // Unmatched delete OR amend: replays as a no-op under `update`'s strict
            // find-then-act, so drop it alone.
            remove[j] = true;
            continue;
        };
        // No entry carries baked cross-entry state (a set's e1RM chain and a
        // run's card spike baseline are derived from the surviving rows), so
        // removing one is exact.
        if target.is_delete {
            for k in delete_chain(events, remove, i, &target) {
                remove[k] = true;
            }
            remove[j] = true;
        } else if is_amend_line(&events[i]) {
            // Amend: telescope away a superseded prior AMEND, but keep the base
            // LOG line (the strict amend needs a row to replace on replay). The
            // amend itself always survives: it carries the entry's current values.
            remove[i] = true;
        }
    }
}

/// The lines a delete cancels: the newest match `newest`, and, when that is an
/// amend OR a trim, its base chain (further prior matches back through and
/// INCLUDING the first log line, so a run's base `LogRunTrack` and all its trim
/// lines go together). A bare (shared-id) log stops at itself, mirroring
/// `update`'s newest-only row removal. Indices are in descending order.
fn delete_chain(
    events: &[Event],
    remove: &[bool],
    newest: usize,
    target: &EditTarget,
) -> Vec<usize> {
    let mut removal = vec![newest];
    let mut last = newest;
    // Stop at the base log line (not an amend or a trim).
    while is_amend_line(&events[last]) || is_trim_line(&events[last]) {
        match (0..last).rev().find(|&p| {
            !remove[p] && entry_line_matches(&events[p], target.fam, target.id, target.time)
        }) {
            Some(p) => {
                removal.push(p);
                last = p;
            }
            None => break,
        }
    }
    removal
}

/// Rule 1: everything in family F at or before its last clear. The review
/// family is exempt: its clears leave `weekly_history` intact.
fn rule1_clears(kinds: &[(Family, bool)], remove: &mut [bool]) {
    for fam in Family::ALL.into_iter().filter(|&f| f != Family::Review) {
        if let Some(last_clear) = kinds.iter().rposition(|&(f, c)| f == fam && c) {
            for (j, &(f, _)) in kinds.iter().enumerate().take(last_clear + 1) {
                if f == fam {
                    remove[j] = true;
                }
            }
        }
    }
}

/// Rule 1's one kept clear: the last `ClearPlan` survives when no
/// `GeneratePlan` follows it, because it is then the log's final plan state.
/// Replaying it is a no-op on the plan family Rule 1 already emptied, but the
/// line is the durable record that the user removed their plan: the shell's
/// boot reads it so a removed plan is not auto-generated again when its own
/// prefs marker was lost (lockstep with `EventLog.kt`).
fn rule1_keep_final_plan_clear(events: &[Event], remove: &mut [bool]) {
    if let Some(last) = events.iter().rposition(|e| matches!(e, Event::ClearPlan))
        && !events[last + 1..]
            .iter()
            .any(|e| matches!(e, Event::GeneratePlan { .. }))
    {
        remove[last] = false;
    }
}

/// Rule 2: for each [`SINGLETONS`] family, keep only the last surviving member.
fn rule2_singletons(kinds: &[(Family, bool)], remove: &mut [bool]) {
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
}

/// Rule 2 (reviews): every SubmitReview / SubmitWeeklyCheckin survives. A
/// ClearReview only empties the session review, so it is inert when a later
/// ClearReview or a later SubmitReview that sets the session review follows;
/// a ClearWeeklyCheckin only empties the weekly check-in in force, so it is
/// inert when a later ClearWeeklyCheckin, SubmitWeeklyCheckin or weekly-
/// carrying SubmitReview follows. Drop exactly those.
fn rule2_review_clears(events: &[Event], remove: &mut [bool]) {
    for (j, e) in events.iter().enumerate() {
        let later = &events[j + 1..];
        let inert = match e {
            Event::ClearReview => later.iter().any(|l| match l {
                Event::ClearReview => true,
                Event::SubmitReview(r) => crate::app::submit_review_sets_session(r),
                _ => false,
            }),
            Event::ClearWeeklyCheckin => later.iter().any(|l| match l {
                Event::ClearWeeklyCheckin | Event::SubmitWeeklyCheckin(_) => true,
                Event::SubmitReview(r) => crate::app::submit_review_sets_weekly(r),
                _ => false,
            }),
            _ => false,
        };
        if inert {
            remove[j] = true;
        }
    }
}

/// Rule 2 (profile history): a SetProfile line is dead iff a LATER surviving
/// SetProfile has a history key <= its own ([`profile_key`]), with no pinning
/// edit in between. Mirrors `update`'s `profile_history` reducer (a SetProfile
/// supersedes every history entry at or after its key), so the survivors are
/// exactly the entries that remain in `profile_history` after replay — NOT a
/// plain keep-last singleton: a run of ascending-dated profiles all survive,
/// same-day edits made later in the day all survive, and a None-dated edit
/// (-inf) supersedes the whole history. A None-dated edit also pins the
/// %HRmax every run logged before it has at that moment, which reads the
/// history then: once a run line survives before it, it is a barrier no line
/// at or after it may kill an earlier one across. The last line is never dead
/// (nothing follows it), so `model.profile` (= the last applied) is always
/// kept. Runs after Rules 1 and 3, which already swept the pre-ClearProfile
/// history and the deleted runs.
fn rule2_profile_history(events: &[Event], remove: &mut [bool]) {
    let first_run = events.iter().enumerate().position(|(j, e)| {
        !remove[j]
            && matches!(
                e,
                Event::LogRun { .. } | Event::LogRunTrack { .. } | Event::AmendRun { .. }
            )
    });
    let profiles: Vec<(usize, &crate::app::Profile)> = events
        .iter()
        .enumerate()
        .filter(|&(j, _)| !remove[j])
        .filter_map(|(j, e)| match e {
            Event::SetProfile(p) => Some((j, p)),
            _ => None,
        })
        .collect();
    for (a, &(j, p)) in profiles.iter().enumerate() {
        let key_j = profile_key(p);
        for &(k, q) in &profiles[a + 1..] {
            let pins = q.effective_from_epoch_day.is_none() && first_run.is_some_and(|r| r < k);
            if pins {
                break;
            }
            if profile_key(q) <= key_j {
                remove[j] = true;
                break;
            }
        }
    }
}

/// A SetProfile's `profile_history` key, as `update` orders it
/// (`app::history_key`): (effective day, then `edited_at`), `None` = -inf, and
/// an undated edit is (-inf, -inf).
fn profile_key(p: &crate::app::Profile) -> (i64, i64) {
    match p.effective_from_epoch_day {
        Some(day) => (day, p.edited_at.unwrap_or(i64::MIN)),
        None => (i64::MIN, i64::MIN),
    }
}

/// Rule 2 (`SetExerciseEnded`): last-write-wins per EXACT key
/// (exercise.trim().lowercase(), epoch_day). `update` stores end-markers keyed
/// by same_movement-normalized exercise + epoch_day, so an earlier marker
/// sharing a later one's EXACT key is definitely overwritten on replay (the
/// exact-key subset is always safe: a survivor `update` would overwrite is
/// inert, and lines differing only in equipment words are conservatively
/// kept). Family 1, so Rule 1 already dropped markers at/before the last
/// ClearSets; only survivors count.
fn rule2_exercise_ended(events: &[Event], remove: &mut [bool]) {
    let mut last_ended: std::collections::HashMap<(String, i64), usize> =
        std::collections::HashMap::new();
    for (j, e) in events.iter().enumerate() {
        if remove[j] {
            continue;
        }
        if let Event::SetExerciseEnded {
            exercise,
            epoch_day,
            ..
        } = e
        {
            let key = (exercise.trim().to_lowercase(), *epoch_day);
            if let Some(prev) = last_ended.insert(key, j) {
                remove[prev] = true;
            }
        }
    }
}

/// Rule 2b: the plan family (plan request) is keep-FIRST, not last-write-wins.
/// `update`'s `Event::GeneratePlan` arm is first-write-wins: it preserves the
/// ORIGINAL plan anchor and, once `plan_request` is `Some`, treats every later
/// GeneratePlan as a no-op. The shell auto-fires GeneratePlan every launch, so
/// a last-write-wins compaction would re-date the plan and irreversibly lose
/// the anchor. Rule 1 already removed everything at or before the last
/// ClearPlan, so the first surviving request is the anchor-defining line; keep
/// it and drop the rest. Keep-first is replay-equivalent to `update`.
fn rule2b_plan_keep_first(events: &[Event], kinds: &[(Family, bool)], remove: &mut [bool]) {
    let plan_survivors: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|&(j, &(f, c))| {
            f == Family::Plan && !c && !remove[j] && matches!(events[j], Event::GeneratePlan { .. })
        })
        .map(|(j, _)| j)
        .collect();
    for &j in plan_survivors.iter().skip(1) {
        remove[j] = true;
    }
}

#[cfg(test)]
mod tests;
