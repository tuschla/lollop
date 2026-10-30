//! Event-log compaction. The core keeps no durable state: a shell persists the
//! raw [`Event`] stream and replays it, so the log grows without bound.
//! Compaction drops the lines whose effect on the replayed model is provably
//! nil and keeps the survivors in order, so the shorter log replays to the same
//! model. It rests on `update` storing raw inputs only: a figure derived from
//! other rows (a run's typed-bpm %HRmax, its card spike baseline) is re-derived
//! whenever its inputs change, so dropping a line those inputs no longer hold
//! cannot move it. Each rule is documented on its function:
//!
//! - Rule 0 (`rule0_readiness_removes`): an undated `RemoveReadiness` cancels its submit.
//! - Rule 3 (`rule3_entry_edits`): a `DeleteEntry` cancels its entry; an amend
//!   supersedes the prior amend but keeps the base log line.
//! - Rule 1 (`rule1_clears`): a `Clear<F>` supersedes its family.
//! - Rule 2: last write wins for the calculator singletons and per exact
//!   `SetExerciseEnded` key; profiles keep exactly the history `update` keeps;
//!   reviews are a history; `SetToday` keeps every UTC-offset change.
//! - Rule 2b (`rule2b_plan_keep_first`): the plan request is keep-first.
//!
//! Check-ins and readiness are never windowed: a later `SetToday` can move
//! today back to any reading's day, and a safety hold lives until it is
//! cleared (HARD RULE 3).
//!
//! The Android shell compacts its raw log through [`compact_lines`] over JNI
//! (`ffi.rs`), one [`LineFacts`] per line, so a multi-year GPS log is never
//! held decoded.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::app::Event;
use crate::schema::ReadinessSignal;

/// A compaction family: the events one clear resets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Readiness,
    Sets,
    Runs,
    Profile,
    Review,
    RacePrediction,
    HypertrophyPlan,
    Protein,
    HrZones,
    Checkins,
    Plan,
    Today,
    Activities,
}

/// Families whose members are last-write-wins: each calculator assigns its
/// `model` field outright.
const SINGLETONS: [Family; 4] = [
    Family::RacePrediction,
    Family::HypertrophyPlan,
    Family::Protein,
    Family::HrZones,
];

/// Wire tags of events the core no longer has (the deleted Cooper, critical
/// speed and APRE calculators). Replay rejects them, so compaction drops them.
const RETIRED: [&str; 6] = [
    "ComputeCooper",
    "ClearCooper",
    "ComputeCriticalSpeed",
    "ClearCriticalSpeed",
    "ComputeApre",
    "ClearApre",
];

/// A set/run/activity row's `(family, entry_id, observed_at)` identity.
type Identity = (Family, u64, i64);

/// The row a `DeleteEntry` / `AmendSet` / `AmendRun` targets: its family and
/// the `(id, observed_at)` match key `update`'s `find_entry` uses.
#[derive(Clone, Copy, Debug)]
struct EditTarget {
    fam: Family,
    id: u64,
    time: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EntryLine {
    /// `LogSet`, `LogRun`, `LogRunTrack`, `LogActivity`: a base row.
    Log,
    /// `AmendSet`, `AmendRun`: replaces the row, so it supersedes a prior amend.
    Amend,
    /// `TrimRunTrack`, `AmendRunMeta`: an in-place edit. Never superseded, but
    /// a delete's chain-walk passes through it to the base log.
    Trim,
}

/// What a rule reads off one event.
#[derive(Clone, Debug)]
enum Rule {
    Plain,
    Submit(ReadinessSignal),
    /// `RemoveReadiness`; `dated` = `!is_undated(observed_at)`.
    Remove {
        signal: ReadinessSignal,
        dated: bool,
    },
    ResolveHold {
        signal: ReadinessSignal,
        dated: bool,
    },
    Entry {
        kind: EntryLine,
        /// The raw (clamped) `entry_id`, gate or not.
        id: u64,
        /// `None` when the line fails `update`'s gate: it never becomes a row.
        identity: Option<Identity>,
        /// The row an amend replaces.
        target: Option<EditTarget>,
    },
    Delete(EditTarget),
    Profile {
        key: (i64, i64),
        undated: bool,
    },
    Review {
        sets_session: bool,
        sets_weekly: bool,
    },
    WeeklyCheckin,
    ClearReview,
    ClearWeeklyCheckin,
    GeneratePlan,
    ClearPlan,
    /// `SetExerciseEnded` by `(names::exercise_key(exercise), epoch_day)`.
    Ended(String, i64),
    Today {
        utc_offset_sec: i64,
    },
}

static PLAIN: Rule = Rule::Plain;

#[derive(Clone, Debug)]
enum Line {
    /// Not JSON: a tail torn by a kill mid-append.
    Torn,
    /// JSON but no current [`Event`] (a newer build's event, or a shape the
    /// core rejects on replay): kept untouched.
    Opaque {
        entry_id: u64,
    },
    Retired,
    Event {
        family: Family,
        clear: bool,
        rule: Rule,
    },
}

/// What compaction reads off one log line, taken once so the line's decoded
/// event (a GPS track's fixes) can be dropped right after.
#[derive(Clone, Debug)]
pub struct LineFacts(Line);

/// The shape of a JSON line that is no current [`Event`]: a bare unit tag or a
/// single-key object, read for its tag and `entry_id` only.
#[derive(Deserialize)]
#[serde(untagged)]
enum WireShape {
    Unit(String),
    Tagged(HashMap<String, WirePayload>),
}

#[derive(Deserialize)]
struct WirePayload {
    #[serde(default)]
    entry_id: serde_json::Value,
}

impl LineFacts {
    /// The facts of one raw log line (an event's serde wire JSON).
    #[must_use]
    pub fn parse(line: &str) -> Self {
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            return Self::of(&event);
        }
        if serde_json::from_str::<serde::de::IgnoredAny>(line).is_err() {
            return Self(Line::Torn);
        }
        let retired = |tag: &str| RETIRED.contains(&tag);
        Self(match serde_json::from_str::<WireShape>(line) {
            Ok(WireShape::Unit(tag)) if retired(&tag) => Line::Retired,
            Ok(WireShape::Tagged(map)) if map.len() == 1 => match map.into_iter().next() {
                Some((tag, _)) if retired(&tag) => Line::Retired,
                Some((_, payload)) => Line::Opaque {
                    entry_id: payload
                        .entry_id
                        .as_u64()
                        .map_or(0, crate::app::sanitize_entry_id),
                },
                None => Line::Opaque { entry_id: 0 },
            },
            _ => Line::Opaque { entry_id: 0 },
        })
    }

    /// The match is exhaustive (no `_`), so a new [`Event`] variant fails to
    /// compile until it is placed in a family.
    fn of(event: &Event) -> Self {
        use Family as F;
        let id = crate::app::sanitize_entry_id;
        // An amend targets its OLD row: for a legacy (`id == 0`) row whose date
        // changed, that is `observed_at_fallback`, as in `find_entry`.
        let amend_time = |observed_at: i64, fallback: i64| {
            if fallback != 0 { fallback } else { observed_at }
        };
        let entry = |kind, valid: bool, fam, entry_id: u64, time, target_time: Option<i64>| {
            let rule = Rule::Entry {
                kind,
                id: id(entry_id),
                identity: valid.then_some((fam, id(entry_id), time)),
                target: target_time.map(|time| EditTarget {
                    fam,
                    id: id(entry_id),
                    time,
                }),
            };
            (fam, false, rule)
        };
        let (family, clear, rule) = match event {
            Event::SubmitReadiness(input) => (F::Readiness, false, Rule::Submit(input.signal)),
            Event::RemoveReadiness {
                signal,
                observed_at,
            } => (
                F::Readiness,
                false,
                Rule::Remove {
                    signal: *signal,
                    dated: !crate::is_undated(*observed_at),
                },
            ),
            Event::ResolveHold {
                signal,
                observed_at,
            } => (
                F::Readiness,
                false,
                Rule::ResolveHold {
                    signal: *signal,
                    dated: !crate::is_undated(*observed_at),
                },
            ),
            Event::LogSet {
                entry_id,
                observed_at,
                weight_kg,
                reps,
                hold_sec,
                ..
            } => entry(
                EntryLine::Log,
                set_passes_gate(*weight_kg, *reps, *hold_sec),
                F::Sets,
                *entry_id,
                *observed_at,
                None,
            ),
            Event::AmendSet {
                entry_id,
                observed_at,
                observed_at_fallback,
                weight_kg,
                reps,
                hold_sec,
                ..
            } => entry(
                EntryLine::Amend,
                set_passes_gate(*weight_kg, *reps, *hold_sec),
                F::Sets,
                *entry_id,
                *observed_at,
                Some(amend_time(*observed_at, *observed_at_fallback)),
            ),
            Event::SetExerciseEnded {
                exercise,
                epoch_day,
                ..
            } => (
                F::Sets,
                false,
                Rule::Ended(crate::names::exercise_key(exercise), *epoch_day),
            ),
            // Every rung placement is history (a past day's rung depends on
            // the placements before it): only ClearSets sweeps one.
            Event::SetCalisthenicsRung { .. } => (F::Sets, false, Rule::Plain),
            Event::LogRun {
                entry_id,
                observed_at,
                ..
            }
            | Event::LogRunTrack {
                entry_id,
                observed_at,
                ..
            } => entry(EntryLine::Log, true, F::Runs, *entry_id, *observed_at, None),
            Event::AmendRun {
                entry_id,
                observed_at,
                observed_at_fallback,
                ..
            } => entry(
                EntryLine::Amend,
                true,
                F::Runs,
                *entry_id,
                *observed_at,
                Some(amend_time(*observed_at, *observed_at_fallback)),
            ),
            // A trim or meta edit names its run by `observed_at_fallback`.
            Event::TrimRunTrack {
                entry_id,
                observed_at_fallback,
                ..
            }
            | Event::AmendRunMeta {
                entry_id,
                observed_at_fallback,
                ..
            } => entry(
                EntryLine::Trim,
                true,
                F::Runs,
                *entry_id,
                *observed_at_fallback,
                None,
            ),
            Event::LogActivity {
                entry_id,
                observed_at,
                minutes,
                ..
            } => entry(
                EntryLine::Log,
                crate::app::activity_passes_gate(*minutes),
                F::Activities,
                *entry_id,
                *observed_at,
                None,
            ),
            // A delete joins the family it targets, so a later clear sweeps it.
            Event::DeleteEntry {
                kind,
                entry_id,
                observed_at_fallback,
            } => {
                let fam = match kind {
                    crate::app::EntryKind::Set => F::Sets,
                    crate::app::EntryKind::Run => F::Runs,
                    crate::app::EntryKind::Activity => F::Activities,
                };
                let target = EditTarget {
                    fam,
                    id: id(*entry_id),
                    time: *observed_at_fallback,
                };
                (fam, false, Rule::Delete(target))
            }
            Event::SetProfile(p) => (
                F::Profile,
                false,
                Rule::Profile {
                    key: profile_key(p),
                    undated: p.effective_from_epoch_day.is_none(),
                },
            ),
            Event::SubmitReview(r) => (
                F::Review,
                false,
                Rule::Review {
                    sets_session: crate::app::submit_review_sets_session(r),
                    sets_weekly: crate::app::submit_review_sets_weekly(r),
                },
            ),
            Event::SubmitWeeklyCheckin(_) => (F::Review, false, Rule::WeeklyCheckin),
            Event::ClearReview => (F::Review, true, Rule::ClearReview),
            Event::ClearWeeklyCheckin => (F::Review, true, Rule::ClearWeeklyCheckin),
            Event::GeneratePlan { .. } => (F::Plan, false, Rule::GeneratePlan),
            Event::ClearPlan => (F::Plan, true, Rule::ClearPlan),
            Event::SetToday { utc_offset_sec, .. } => (
                F::Today,
                false,
                Rule::Today {
                    utc_offset_sec: *utc_offset_sec,
                },
            ),
            Event::PredictRace { .. } => (F::RacePrediction, false, Rule::Plain),
            Event::PlanHypertrophyMeso { .. } => (F::HypertrophyPlan, false, Rule::Plain),
            Event::ComputeProtein { .. } => (F::Protein, false, Rule::Plain),
            Event::ComputeHrZones { .. } => (F::HrZones, false, Rule::Plain),
            Event::SubmitCheckin(_) | Event::RestartHrvBaseline { .. } => {
                (F::Checkins, false, Rule::Plain)
            }
            // Every move is kept (the week replays them in order); ClearPlan
            // sweeps them with the plan.
            Event::MoveSession { .. } => (F::Plan, false, Rule::Plain),
            Event::ClearReadiness => (F::Readiness, true, Rule::Plain),
            Event::ClearSets => (F::Sets, true, Rule::Plain),
            Event::ClearRuns => (F::Runs, true, Rule::Plain),
            Event::ClearActivities => (F::Activities, true, Rule::Plain),
            Event::ClearProfile => (F::Profile, true, Rule::Plain),
            Event::ClearRacePrediction => (F::RacePrediction, true, Rule::Plain),
            Event::ClearHypertrophyPlan => (F::HypertrophyPlan, true, Rule::Plain),
            Event::ClearProtein => (F::Protein, true, Rule::Plain),
            Event::ClearHrZones => (F::HrZones, true, Rule::Plain),
            Event::ClearCheckins => (F::Checkins, true, Rule::Plain),
        };
        Self(Line::Event {
            family,
            clear,
            rule,
        })
    }

    fn family(&self) -> Option<(Family, bool)> {
        match self.0 {
            Line::Event { family, clear, .. } => Some((family, clear)),
            _ => None,
        }
    }

    fn rule(&self) -> &Rule {
        match &self.0 {
            Line::Event { rule, .. } => rule,
            _ => &PLAIN,
        }
    }

    fn entry_kind(&self) -> Option<EntryLine> {
        match self.rule() {
            Rule::Entry { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// Whether this is the row an edit of `t` names: same family, and the
    /// same id, or the same `observed_at` for a legacy (`id == 0`) row.
    /// Mirrors `find_entry`.
    fn matches(&self, t: &EditTarget) -> bool {
        match self.rule() {
            Rule::Entry {
                identity: Some((fam, id, time)),
                ..
            } if *fam == t.fam => {
                if t.id != 0 {
                    *id == t.id
                } else {
                    *id == 0 && *time == t.time
                }
            }
            _ => false,
        }
    }

    /// The `entry_id` the line carries (0 when none), kept or not.
    fn entry_id(&self) -> u64 {
        match (&self.0, self.rule()) {
            (Line::Opaque { entry_id }, _) => *entry_id,
            (_, Rule::Entry { id, .. }) => *id,
            (_, Rule::Delete(t)) => t.id,
            _ => 0,
        }
    }
}

/// `update`'s set-validity gate for `LogSet`/`AmendSet`: a non-negative (or NaN,
/// sanitized to 0 on store) weight and either reps or a hold time. Shared with
/// `update`, so compaction gives an invalid line no identity exactly when
/// `update` drops it.
pub(crate) fn set_passes_gate(weight_kg: f64, reps: u32, hold_sec: Option<u16>) -> bool {
    (weight_kg >= 0.0 || weight_kg.is_nan()) && (reps > 0 || hold_sec.is_some())
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

/// [`compact_lines`]' verdict on a raw log, by line index.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Compaction {
    /// Lines compaction drops, ascending.
    pub dropped: Vec<usize>,
    /// Lines that are not JSON (a tail torn by a kill mid-append), ascending.
    pub torn: Vec<usize>,
    /// The largest `entry_id` any intact line carries, kept or not (0 when
    /// none), so a shell never reuses one.
    pub max_entry_id: u64,
    /// The last surviving plan line is a `ClearPlan`: the user removed their plan.
    pub plan_removed: bool,
}

/// Compact a raw log given as one [`LineFacts`] per line.
#[must_use]
pub fn compact_lines(lines: &[LineFacts]) -> Compaction {
    let remove = removals(lines);
    let mut out = Compaction::default();
    let mut last_plan = None;
    for (j, line) in lines.iter().enumerate() {
        match line.0 {
            Line::Torn => {
                out.torn.push(j);
                continue;
            }
            Line::Retired => out.dropped.push(j),
            _ if remove[j] => out.dropped.push(j),
            _ => match line.rule() {
                Rule::GeneratePlan => last_plan = Some(false),
                Rule::ClearPlan => last_plan = Some(true),
                _ => {}
            },
        }
        out.max_entry_id = out.max_entry_id.max(line.entry_id());
    }
    out.plan_removed = last_plan == Some(true);
    out
}

/// Drop provably-inert events from a log, preserving replay order.
#[must_use]
pub fn compact_event_log(events: Vec<Event>) -> Vec<Event> {
    let lines: Vec<LineFacts> = events.iter().map(LineFacts::of).collect();
    let remove = removals(&lines);
    events
        .into_iter()
        .zip(remove)
        .filter_map(|(e, removed)| (!removed).then_some(e))
        .collect()
}

/// Every rule over `lines`: true = the line is inert.
fn removals(lines: &[LineFacts]) -> Vec<bool> {
    let mut remove = vec![false; lines.len()];
    rule0_readiness_removes(lines, &mut remove);
    rule3_entry_edits(lines, &mut remove);
    rule1_clears(lines, &mut remove);
    rule2_singletons(lines, &mut remove);
    rule2_today(lines, &mut remove);
    rule2_review_clears(lines, &mut remove);
    rule2_profile_history(lines, &mut remove);
    rule2_exercise_ended(lines, &mut remove);
    rule2b_plan_spans(lines, &mut remove);
    remove
}

/// Rule 0: an UNDATED RemoveReadiness deletes the latest active prior
/// SubmitReadiness of its signal (`update` drops exactly that input), so the
/// pair is inert. A DATED remove retracts that input instead (it stays for
/// the past days it shaped): both lines survive and the submit is consumed,
/// so a later remove pairs with the next older one. A dated ResolveHold
/// retracts EVERY active submit of its signal the same way. An unmatched
/// remove, or an undated or unmatched ResolveHold, replays as a no-op and is
/// dropped alone. ClearReadiness (Rule 1) wipes active and retracted inputs
/// alike, so the outcome holds in every interleaving with it.
fn rule0_readiness_removes(lines: &[LineFacts], remove: &mut [bool]) {
    let mut consumed = vec![false; lines.len()];
    let submit_of =
        |line: &LineFacts, signal| matches!(line.rule(), Rule::Submit(s) if *s == signal);
    for j in 0..lines.len() {
        match *lines[j].rule() {
            Rule::ResolveHold { signal, dated } => {
                let mut any = false;
                for i in 0..j {
                    if !remove[i] && !consumed[i] && submit_of(&lines[i], signal) {
                        any = true;
                        consumed[i] |= dated;
                    }
                }
                if !any || !dated {
                    remove[j] = true;
                }
            }
            Rule::Remove { signal, dated } => {
                let matched = (0..j)
                    .rev()
                    .find(|&i| !remove[i] && !consumed[i] && submit_of(&lines[i], signal));
                match matched {
                    Some(i) if dated => consumed[i] = true,
                    Some(i) => {
                        remove[i] = true;
                        remove[j] = true;
                    }
                    None => remove[j] = true,
                }
            }
            _ => {}
        }
    }
}

/// Rule 3: a Delete cancels its entry; an Amend supersedes a prior amend but
/// keeps the entry's base log. Each DeleteEntry / AmendSet / AmendRun pairs
/// with the newest surviving prior row line that [`LineFacts::matches`] it,
/// the predicate `update`'s `find_*` use. `update`'s amend replaces on a match
/// and is a no-op on a miss, so:
/// - an amend drops a superseded prior AMEND but keeps the base log, which the
///   amend needs to replace on replay (`[log, amend, amend]` → `[log, amend]`);
/// - a delete drops its match and, through amends and trims, the base chain
///   down to and including the log, plus itself ([`delete_chain`]);
/// - an unmatched delete or amend, or an AmendSet that fails the set gate, is
///   dropped alone.
///
/// Runs before Rule 1, which sweeps any line matched across a clear anyway.
/// The chain-walk keys a legacy (`entry_id == 0`) chain by one `observed_at`,
/// which holds only while the shell never re-dates a legacy row (it pins
/// `observed_at == observed_at_fallback` for id-0 rows; see
/// `rule3_walk_assumes_legacy_rows_are_never_redated`).
fn rule3_entry_edits(lines: &[LineFacts], remove: &mut [bool]) {
    for j in 0..lines.len() {
        let (target, is_delete) = match lines[j].rule() {
            Rule::Delete(t) => (*t, true),
            Rule::Entry {
                kind: EntryLine::Amend,
                identity: None,
                ..
            } => {
                remove[j] = true;
                continue;
            }
            Rule::Entry {
                kind: EntryLine::Amend,
                target: Some(t),
                ..
            } => (*t, false),
            _ => continue,
        };
        let Some(i) = newest_match(lines, remove, j, &target) else {
            remove[j] = true;
            continue;
        };
        if is_delete {
            for k in delete_chain(lines, remove, i, &target) {
                remove[k] = true;
            }
            remove[j] = true;
        } else if lines[i].entry_kind() == Some(EntryLine::Amend) {
            remove[i] = true;
        }
    }
}

/// The newest surviving line before `before` that `t` names.
fn newest_match(
    lines: &[LineFacts],
    remove: &[bool],
    before: usize,
    t: &EditTarget,
) -> Option<usize> {
    (0..before)
        .rev()
        .find(|&i| !remove[i] && lines[i].matches(t))
}

/// The lines a delete cancels: its match `newest` and, while that is an amend
/// or a trim, further prior matches down to and including the base log line.
/// A bare log stops at itself, as `update` removes only the newest row.
fn delete_chain(
    lines: &[LineFacts],
    remove: &[bool],
    newest: usize,
    target: &EditTarget,
) -> Vec<usize> {
    let mut removal = vec![newest];
    let mut last = newest;
    while lines[last].entry_kind() != Some(EntryLine::Log) {
        match newest_match(lines, remove, last, target) {
            Some(p) => {
                removal.push(p);
                last = p;
            }
            None => break,
        }
    }
    removal
}

/// Rule 1: a `Clear<F>` empties family F, so every F line at or before the
/// last clear of F is inert. The review family is exempt: its clears leave
/// the weekly history intact (Rule 2 handles them).
fn rule1_clears(lines: &[LineFacts], remove: &mut [bool]) {
    let mut cleared: Vec<Family> = Vec::new();
    for (j, line) in lines.iter().enumerate().rev() {
        let Some((fam, clear)) = line.family() else {
            continue;
        };
        if fam == Family::Review {
            continue;
        }
        if clear && !cleared.contains(&fam) {
            cleared.push(fam);
        }
        remove[j] |= cleared.contains(&fam);
    }
}

/// Rule 2: for each [`SINGLETONS`] family, keep only the last surviving member.
fn rule2_singletons(lines: &[LineFacts], remove: &mut [bool]) {
    let mut seen: Vec<Family> = Vec::new();
    for (j, line) in lines.iter().enumerate().rev() {
        let Some((fam, false)) = line.family() else {
            continue;
        };
        if remove[j] || !SINGLETONS.contains(&fam) {
            continue;
        }
        if seen.contains(&fam) {
            remove[j] = true;
        } else {
            seen.push(fam);
        }
    }
}

/// Rule 2 (`SetToday`): the shell's clock. The day bakes nothing but the day
/// a ClearPlan removes a plan on (Rule 2b keeps that SetToday; `update`
/// re-derives birthday ages on every SetToday), and the UTC offset does: a
/// legacy run row without its own offset converts its typed bpm under the
/// offset in force, and a later undated SetProfile pins that figure. So a
/// SetToday survives when it changes the offset (from the model's default 0),
/// and the last one survives as today.
fn rule2_today(lines: &[LineFacts], remove: &mut [bool]) {
    let mut offset = 0;
    let mut last = None;
    for (j, line) in lines.iter().enumerate() {
        if let Rule::Today { utc_offset_sec } = *line.rule() {
            remove[j] = utc_offset_sec == offset;
            offset = utc_offset_sec;
            last = Some(j);
        }
    }
    if let Some(j) = last {
        remove[j] = false;
    }
}

/// Rule 2 (reviews): every SubmitReview / SubmitWeeklyCheckin survives (the
/// weekly check-ins date past days' deload and downgrade windows). A
/// ClearReview empties only the session review, so it is inert when a later
/// ClearReview or session-setting SubmitReview follows; a ClearWeeklyCheckin
/// empties only the weekly check-in, so it is inert when a later
/// ClearWeeklyCheckin, SubmitWeeklyCheckin or weekly-carrying SubmitReview
/// follows.
fn rule2_review_clears(lines: &[LineFacts], remove: &mut [bool]) {
    let (mut session_later, mut weekly_later) = (false, false);
    for (j, line) in lines.iter().enumerate().rev() {
        match *line.rule() {
            Rule::ClearReview => {
                remove[j] |= session_later;
                session_later = true;
            }
            Rule::ClearWeeklyCheckin => {
                remove[j] |= weekly_later;
                weekly_later = true;
            }
            Rule::Review {
                sets_session,
                sets_weekly,
            } => {
                session_later |= sets_session;
                weekly_later |= sets_weekly;
            }
            Rule::WeeklyCheckin => weekly_later = true,
            _ => {}
        }
    }
}

/// Rule 2 (profile history): a SetProfile line is dead iff a LATER surviving
/// one has a history key `<=` its own ([`profile_key`]): `update`'s
/// `profile_history` drops every entry at or after a new edit's key, so the
/// survivors are exactly the history replay keeps (ascending dates and later
/// same-day edits all survive; an undated edit replaces everything). An
/// undated edit also pins the %HRmax of every run logged before it, so once a
/// run line survives before it, no line kills an earlier one across it. The
/// last line is never dead, so the current profile is always kept. Runs after
/// Rules 1 and 3.
fn rule2_profile_history(lines: &[LineFacts], remove: &mut [bool]) {
    let first_run = (0..lines.len()).find(|&j| {
        !remove[j]
            && matches!(
                lines[j].rule(),
                Rule::Entry {
                    kind: EntryLine::Log | EntryLine::Amend,
                    identity: Some((Family::Runs, ..)),
                    ..
                }
            )
    });
    let profiles: Vec<(usize, (i64, i64), bool)> = lines
        .iter()
        .enumerate()
        .filter(|&(j, _)| !remove[j])
        .filter_map(|(j, l)| match *l.rule() {
            Rule::Profile { key, undated } => Some((j, key, undated)),
            _ => None,
        })
        .collect();
    for (a, &(j, key_j, _)) in profiles.iter().enumerate() {
        for &(k, key_k, undated) in &profiles[a + 1..] {
            if undated && first_run.is_some_and(|r| r < k) {
                break;
            }
            if key_k <= key_j {
                remove[j] = true;
                break;
            }
        }
    }
}

/// Rule 2 (`SetExerciseEnded`): last write wins per EXACT key
/// `(names::exercise_key(exercise), epoch_day)`. `update` keys end-markers by a
/// looser movement-normalized name (`plan::same_movement`, whose tokens compare
/// with the same fold), so the exact-key subset is always safe (lines differing
/// only in equipment words are conservatively kept).
fn rule2_exercise_ended(lines: &[LineFacts], remove: &mut [bool]) {
    let mut last_ended: HashMap<(&str, i64), usize> = HashMap::new();
    for (j, line) in lines.iter().enumerate() {
        if remove[j] {
            continue;
        }
        if let Rule::Ended(exercise, day) = line.rule()
            && let Some(prev) = last_ended.insert((exercise.as_str(), *day), j)
        {
            remove[prev] = true;
        }
    }
}

/// Rule 2b (plan): `update`'s GeneratePlan arm keeps the ORIGINAL start and
/// ignores every later request while a plan exists (the shell fires one every
/// launch), and a ClearPlan without a plan is a no-op. So each plan's first
/// GeneratePlan survives, and so does the ClearPlan that removed it with the
/// SetToday before it: together they replay the plan's span
/// (`Model::past_plans`). The last ClearPlan survives too when no
/// GeneratePlan follows it: replayed as a no-op, it is the durable record
/// that the user removed their plan, which the shell's boot reads. Rule 1
/// has already swept the MoveSession lines a ClearPlan empties.
fn rule2b_plan_spans(lines: &[LineFacts], remove: &mut [bool]) {
    let (mut open, mut today_line, mut last_plan_line) = (false, None, None);
    for (j, line) in lines.iter().enumerate() {
        match line.rule() {
            Rule::Today { .. } => today_line = Some(j),
            Rule::GeneratePlan => {
                remove[j] = open;
                open = true;
                last_plan_line = Some(j);
            }
            Rule::ClearPlan => {
                remove[j] = !open;
                if let Some(t) = today_line.filter(|_| open) {
                    remove[t] = false;
                }
                open = false;
                last_plan_line = Some(j);
            }
            _ => {}
        }
    }
    if let Some(j) = last_plan_line.filter(|&j| matches!(lines[j].rule(), Rule::ClearPlan)) {
        remove[j] = false;
    }
}

#[cfg(test)]
mod tests;
