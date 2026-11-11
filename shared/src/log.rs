//! Event-log compaction. The core keeps no durable state: a shell persists the
//! raw [`Event`] stream and replays it, so the log grows without bound.
//! Compaction drops the lines whose effect on the replayed model is provably
//! nil and keeps the survivors in order, so the shorter log replays to the same
//! model. It rests on `update` storing raw inputs only: a figure derived from
//! other rows (a run's typed-bpm %HRmax, its card spike baseline) is re-derived
//! whenever its inputs change, so dropping a line those inputs no longer hold
//! cannot move it. Every decision about one event (its family, the row it
//! stores or names, the readiness input it removes, the plan step it takes,
//! its end-marker key) is read off the same `crate::event` functions `update`
//! applies. Each rule is documented on its function:
//!
//! - Rule 0 (`rule0_readiness_removes`): an undated `RemoveReadiness` cancels its submit.
//! - Rule 3 (`rule3_entry_edits`): a `DeleteEntry` cancels its entry; an amend
//!   supersedes the prior amend but keeps the base log line.
//! - Rule 1 (`rule1_clears`): a `Clear<F>` supersedes its family.
//! - Rule 2: last write wins for the calculator singletons and per end-marker
//!   key; profiles keep exactly the history `update` keeps; reviews are a
//!   history; `SetToday` keeps every UTC-offset change.
//! - Rule 2b (`rule2b_plan_spans`): the plan request is keep-first.
//!
//! Check-ins and readiness are never windowed: a later `SetToday` can move
//! today back to any reading's day, and a safety hold lives until it is
//! cleared (HARD RULE 3).
//!
//! Both shells compact their raw log through [`compact_lines`] over the
//! bridge's `CompactLog` op (`ffi/bridge.rs`), one [`LineFacts`] per line, so
//! a multi-year GPS log is never held decoded.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::event::entry::{EntryEvent, EntryOp, EntryRef};
use crate::event::rules::{
    EndMarkerLineKey, Family, PlanStep, end_marker_line_key, readiness_removal,
};
use crate::event::{EntryKind, Event, RETIRED_TAGS};
use crate::sanitize::Sanitize;
use crate::schema::{ReadinessInput, ReadinessSignal};

/// Families whose members are last-write-wins: each calculator assigns its
/// `model` field outright.
const SINGLETONS: [Family; 4] = [
    Family::RacePrediction,
    Family::HypertrophyPlan,
    Family::Protein,
    Family::HrZones,
];

/// What a rule reads off one event.
#[derive(Clone, Debug)]
enum Rule {
    Plain,
    Submit(ReadinessInput),
    /// `RemoveReadiness`; `dated` = `!is_undated(observed_at)`.
    Remove {
        signal: ReadinessSignal,
        dated: bool,
    },
    ResolveHold {
        signal: ReadinessSignal,
        dated: bool,
    },
    Entry(EntryEvent),
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
    /// `SetExerciseEnded` by its [`end_marker_line_key`].
    Ended(EndMarkerLineKey),
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
            return Self::of(event);
        }
        if serde_json::from_str::<serde::de::IgnoredAny>(line).is_err() {
            return Self(Line::Torn);
        }
        let retired = |tag: &str| RETIRED_TAGS.contains(&tag);
        Self(match serde_json::from_str::<WireShape>(line) {
            Ok(WireShape::Unit(tag)) if retired(&tag) => Line::Retired,
            Ok(WireShape::Tagged(map)) if map.len() == 1 => match map.into_iter().next() {
                Some((tag, _)) if retired(&tag) => Line::Retired,
                Some((_, payload)) => Line::Opaque {
                    entry_id: payload.entry_id.as_u64().map_or(0, Sanitize::sanitized),
                },
                None => Line::Opaque { entry_id: 0 },
            },
            _ => Line::Opaque { entry_id: 0 },
        })
    }

    /// The facts of one event, read off it as `update` applies it
    /// ([`Event::normalized`]).
    fn of(event: Event) -> Self {
        let event = event.normalized();
        let (family, clear) = event.family();
        let rule = if let Some(entry) = event.entry_event() {
            Rule::Entry(entry)
        } else {
            match event {
                Event::SubmitReadiness(input) => Rule::Submit(input),
                Event::RemoveReadiness {
                    signal,
                    observed_at,
                } => Rule::Remove {
                    signal,
                    dated: !crate::day::is_undated(observed_at),
                },
                Event::ResolveHold {
                    signal,
                    observed_at,
                } => Rule::ResolveHold {
                    signal,
                    dated: !crate::day::is_undated(observed_at),
                },
                Event::SetExerciseEnded {
                    exercise,
                    epoch_day,
                    ..
                } => Rule::Ended(end_marker_line_key(&exercise, epoch_day)),
                Event::SetProfile(p) => Rule::Profile {
                    key: p.history_key(),
                    undated: p.effective_from_epoch_day.is_none(),
                },
                Event::SubmitReview(r) => Rule::Review {
                    sets_session: r.sets_session(),
                    sets_weekly: r.sets_weekly(),
                },
                Event::SubmitWeeklyCheckin(_) => Rule::WeeklyCheckin,
                Event::ClearReview => Rule::ClearReview,
                Event::ClearWeeklyCheckin => Rule::ClearWeeklyCheckin,
                Event::GeneratePlan { .. } => Rule::GeneratePlan,
                Event::ClearPlan => Rule::ClearPlan,
                Event::SetToday { utc_offset_sec, .. } => Rule::Today { utc_offset_sec },
                _ => Rule::Plain,
            }
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

    fn entry(&self) -> Option<&EntryEvent> {
        match self.rule() {
            Rule::Entry(e) => Some(e),
            _ => None,
        }
    }

    /// Whether this line stores the row an edit of `kind` naming `target`
    /// finds (as `update`'s `find_entry`).
    fn stores(&self, kind: EntryKind, target: EntryRef) -> bool {
        self.entry()
            .is_some_and(|e| e.stored && e.kind == kind && e.row == target)
    }

    /// The row id the line carries (0 when none), kept or not.
    fn entry_id(&self) -> u64 {
        match &self.0 {
            Line::Opaque { entry_id } => *entry_id,
            _ => self.entry().map_or(0, |e| e.row.id),
        }
    }
}

/// [`compact_lines`]' verdict on a raw log, by line index.
#[derive(Debug, Default, PartialEq, Eq, Serialize, crate::wire::Wire)]
pub struct Compaction {
    /// Lines compaction drops, ascending.
    pub dropped: Vec<usize>,
    /// Lines that are not JSON (a tail torn by a kill mid-append), ascending.
    pub torn: Vec<usize>,
    /// The largest row id any intact line carries, kept or not (0 when
    /// none; a row logged without an id counts with the id it is given), so a
    /// shell never reuses one.
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
    let lines: Vec<LineFacts> = events.iter().cloned().map(LineFacts::of).collect();
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

/// Rule 0: replays the active readiness inputs as `update` keeps them
/// (submits in order, a `ClearReadiness` empties them) and pairs each removal
/// with the input [`readiness_removal`] takes. An UNDATED RemoveReadiness
/// deletes that input, so the pair is inert. A DATED remove retracts it
/// instead (it stays for the past days it shaped): both lines survive and the
/// input leaves the active set, so a later remove pairs with another. A dated
/// ResolveHold retracts EVERY active input of its signal the same way. An
/// unmatched remove, or an undated or unmatched ResolveHold, replays as a
/// no-op and is dropped alone. Rule 1 then sweeps every readiness line before
/// the last `ClearReadiness`, which wipes active and retracted inputs alike.
fn rule0_readiness_removes(lines: &[LineFacts], remove: &mut [bool]) {
    let mut active_lines: Vec<usize> = Vec::new();
    let mut active: Vec<ReadinessInput> = Vec::new();
    for (j, line) in lines.iter().enumerate() {
        match line.rule() {
            Rule::Submit(input) => {
                active_lines.push(j);
                active.push(input.clone());
            }
            Rule::ResolveHold { signal, dated } => {
                let any = active.iter().any(|i| i.signal == *signal);
                if *dated {
                    (active_lines, active) = std::mem::take(&mut active_lines)
                        .into_iter()
                        .zip(std::mem::take(&mut active))
                        .filter(|(_, i)| i.signal != *signal)
                        .unzip();
                }
                if !any || !dated {
                    remove[j] = true;
                }
            }
            Rule::Remove { signal, dated } => match readiness_removal(&active, *signal) {
                Some(k) => {
                    active.remove(k);
                    let i = active_lines.remove(k);
                    if !dated {
                        remove[i] = true;
                        remove[j] = true;
                    }
                }
                None => remove[j] = true,
            },
            _ if line.family() == Some((Family::Readiness, true)) => {
                active.clear();
                active_lines.clear();
            }
            _ => {}
        }
    }
}

/// Rule 3: a Delete cancels its entry; an Amend supersedes a prior amend but
/// keeps the entry's base log. Each DeleteEntry / AmendSet / AmendRun pairs
/// with the newest surviving prior line that stores the row it names
/// ([`LineFacts::stores`], the predicate `update`'s `find_entry` uses).
/// `update`'s amend replaces on a match and is a no-op on a miss, so:
/// - an amend drops a superseded prior AMEND but keeps the base log, which the
///   amend needs to replace on replay (`[log, amend, amend]` → `[log, amend]`);
/// - a delete drops its match and, through amends and trims, the base chain
///   down to and including the log, plus itself ([`delete_chain`]);
/// - an unmatched delete or amend, or an AmendSet that fails the set gate, is
///   dropped alone.
///
/// Runs before Rule 1, which sweeps any line matched across a clear anyway.
/// An amend an old build wrote for a row without an id that also moved its
/// date stores the row under a new id ([`crate::event::entry::row_id`] of
/// the new date): a later amend never supersedes that link, and a delete
/// walks the chain through each link's own target.
fn rule3_entry_edits(lines: &[LineFacts], remove: &mut [bool]) {
    for j in 0..lines.len() {
        let Some(e) = lines[j].entry() else {
            continue;
        };
        let target = match (e.op, e.target) {
            (EntryOp::Amend, _) if !e.stored => {
                remove[j] = true;
                continue;
            }
            (EntryOp::Amend | EntryOp::Delete, Some(t)) => t,
            _ => continue,
        };
        let Some(i) = newest_match(lines, remove, j, e.kind, target) else {
            remove[j] = true;
            continue;
        };
        if e.op == EntryOp::Delete {
            for k in delete_chain(lines, remove, i, e.kind) {
                remove[k] = true;
            }
            remove[j] = true;
        } else if lines[i]
            .entry()
            .is_some_and(|m| m.op == EntryOp::Amend && m.target == Some(m.row))
        {
            remove[i] = true;
        }
    }
}

/// The newest surviving line before `before` storing the row `target` names.
fn newest_match(
    lines: &[LineFacts],
    remove: &[bool],
    before: usize,
    kind: EntryKind,
    target: EntryRef,
) -> Option<usize> {
    (0..before)
        .rev()
        .find(|&i| !remove[i] && lines[i].stores(kind, target))
}

/// The lines a delete cancels: its match `newest` and, while that is an amend
/// or a trim, further prior matches of the row that line names down to and
/// including the base log line. A bare log stops at itself, as `update`
/// removes only the newest row.
fn delete_chain(
    lines: &[LineFacts],
    remove: &[bool],
    newest: usize,
    kind: EntryKind,
) -> Vec<usize> {
    let mut removal = vec![newest];
    let mut last = newest;
    while let Some(target) = lines[last]
        .entry()
        .filter(|e| e.op != EntryOp::Log)
        .and_then(|e| e.target)
    {
        match newest_match(lines, remove, last, kind, target) {
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
/// one has a history key `<=` its own ([`crate::profile::Profile::history_key`]):
/// `update`'s `profile_history` drops every entry at or after a new edit's
/// key, so the survivors are exactly the history replay keeps (ascending dates
/// and later same-day edits all survive; an undated edit replaces everything).
/// An undated edit also pins the %HRmax of every run logged before it, so once
/// a run line survives before it, no line kills an earlier one across it. The
/// last line is never dead, so the current profile is always kept. Runs after
/// Rules 1 and 3.
fn rule2_profile_history(lines: &[LineFacts], remove: &mut [bool]) {
    let first_run = (0..lines.len()).find(|&j| {
        !remove[j]
            && lines[j].entry().is_some_and(|e| {
                e.stored
                    && e.kind == EntryKind::Run
                    && matches!(e.op, EntryOp::Log | EntryOp::Amend)
            })
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

/// Rule 2 (`SetExerciseEnded`): last write wins per [`end_marker_line_key`],
/// under which a later line always replaces the earlier one in `update`.
fn rule2_exercise_ended(lines: &[LineFacts], remove: &mut [bool]) {
    let mut last_ended: HashMap<&EndMarkerLineKey, usize> = HashMap::new();
    for (j, line) in lines.iter().enumerate() {
        if remove[j] {
            continue;
        }
        if let Rule::Ended(key) = line.rule()
            && let Some(prev) = last_ended.insert(key, j)
        {
            remove[prev] = true;
        }
    }
}

/// Rule 2b (plan): each line takes the [`PlanStep`] `update` takes. A
/// `GeneratePlan` that starts a plan survives and one that keeps the original
/// start is inert (the shell fires one every launch); a `ClearPlan` that
/// closes a plan survives with the SetToday before it (together they replay
/// the plan's span, `Model::past_plans`) and one with no plan is inert. The
/// last ClearPlan survives too when no GeneratePlan follows it: replayed as a
/// no-op, it is the durable record that the user removed their plan, which
/// the shell's boot reads. Rule 1 has already swept the MoveSession lines a
/// ClearPlan empties.
fn rule2b_plan_spans(lines: &[LineFacts], remove: &mut [bool]) {
    let (mut open, mut today_line, mut last_plan_line) = (false, None, None);
    for (j, line) in lines.iter().enumerate() {
        let generate = match line.rule() {
            Rule::Today { .. } => {
                today_line = Some(j);
                continue;
            }
            Rule::GeneratePlan => true,
            Rule::ClearPlan => false,
            _ => continue,
        };
        last_plan_line = Some(j);
        match PlanStep::of(generate, open) {
            PlanStep::Start => {
                remove[j] = false;
                open = true;
            }
            PlanStep::KeepOriginal | PlanStep::NoPlan => remove[j] = true,
            PlanStep::Close => {
                remove[j] = false;
                if let Some(t) = today_line {
                    remove[t] = false;
                }
                open = false;
            }
        }
    }
    if let Some(j) = last_plan_line.filter(|&j| matches!(lines[j].rule(), Rule::ClearPlan)) {
        remove[j] = false;
    }
}

#[cfg(test)]
mod tests;
