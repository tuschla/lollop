//! The plan surface of one view: the week strip, the next session and the
//! program summary, built from cached per-day plan snapshots.

use super::*;
use std::rc::Rc;

/// One `view()`'s plan snapshots: the picture through today (what the future
/// days, the next session and the summary read) plus each past/today day's own
/// morning snapshot, tagged with its epoch-day. `gated` lists the past days
/// whose own profile raised a health gate (no plan existed that morning).
struct PlanSnapshots {
    current: (crate::plan::Anchors, Program),
    past: Vec<(i64, crate::plan::Anchors, Program)>,
    gated: Vec<i64>,
}

/// Everything [`PlanSnapshots`] is a pure function of: the logged-set and
/// logged-run fingerprints, the profile HISTORY (each past/today day is
/// prescribed with the profile effective THAT day), the active and retracted
/// readiness inputs (they gate calisthenics progression, date-filtered per
/// day), the shell's today/offset, the block position, the plan's start day,
/// the off day and the calisthenics placements. The key covers everything the
/// snapshots read, so an unrelated event reuses them. A view builds it
/// borrowing the model; only a miss stores an owned copy.
#[derive(PartialEq)]
struct PlanSnapshotKey<'m> {
    sets: u64,
    runs: u64,
    profile_history: std::borrow::Cow<'m, [Profile]>,
    inputs: std::borrow::Cow<'m, [ReadinessInput]>,
    retracted: std::borrow::Cow<'m, [RetractedInput]>,
    today: i64,
    offset: i64,
    week_index: u32,
    start: i64,
    off_day: Option<i64>,
    calis_placements: std::borrow::Cow<'m, [CalisPlacement]>,
}

impl PlanSnapshotKey<'_> {
    fn into_owned(self) -> PlanSnapshotKey<'static> {
        use std::borrow::Cow::Owned;
        PlanSnapshotKey {
            sets: self.sets,
            runs: self.runs,
            profile_history: Owned(self.profile_history.into_owned()),
            inputs: Owned(self.inputs.into_owned()),
            retracted: Owned(self.retracted.into_owned()),
            today: self.today,
            offset: self.offset,
            week_index: self.week_index,
            start: self.start,
            off_day: self.off_day,
            calis_placements: Owned(self.calis_placements.into_owned()),
        }
    }
}

/// The last view's snapshots, plus the past-day readiness of its week. The
/// readiness additionally reads the check-ins, so it carries its own key
/// (their fingerprint and the HRV-baseline restart) on top of the snapshot key.
struct PlanCacheValue {
    snapshots: Rc<PlanSnapshots>,
    readiness: ((u64, Option<i64>), Rc<PastReadiness>),
}

thread_local! {
    /// Synthesizing the week costs one `plan::synthesize` pass per past/today
    /// day plus one for today, and judging each past day reruns the readiness
    /// pipeline over the whole history; both only change when the history,
    /// profile, readiness or date changes. Output is identical to the uncached
    /// rebuild (the adjustment folding still runs every view).
    static PLAN_SNAPSHOT_CACHE: std::cell::RefCell<LastMemo<PlanSnapshotKey<'static>, PlanCacheValue>> =
        const { std::cell::RefCell::new(LastMemo::new()) };
}

thread_local! {
    /// Test-only counter of `plan::synthesize` passes, thread-local so a memo
    /// test measures a race-free delta around two `view()` calls. Not compiled
    /// into shipping builds.
    #[cfg(test)]
    pub(in crate::app) static PLAN_SYNTH_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The per-view frame every part of the plan surface reads.
struct PlanFrame<'a> {
    cx: &'a ViewCtx<'a>,
    model: &'a Model,
    /// The current profile; a day with no profile history entry falls back to it.
    profile: &'a Profile,
    /// The profile effective TODAY: it governs the current picture (future days
    /// + the summary card); each past day uses the profile effective THAT day.
    today_profile: &'a Profile,
    start: i64,
    today: i64,
    offset: i64,
    weight_lb: bool,
    week_monday: i64,
    /// Completed plan weeks before this one: the block position the
    /// synthesizer periodizes on (a load week or the recovery week).
    week_index: u32,
}

impl<'a> PlanFrame<'a> {
    fn flatten_ctx(&self) -> FlattenCtx<'a> {
        FlattenCtx {
            today: self.today,
            start: self.start,
            model: self.model,
            weight_lb: self.weight_lb,
            session_days: &self.cx.days.sets,
            run_days: &self.cx.days.runs,
            run_facts: &self.cx.runs,
        }
    }
}

/// The adjustments the week is judged and folded with.
struct WeekRecs {
    /// Today's readiness recs first, so they win the single displayed
    /// `adjustment` slot on the hero; then the non-blocking review recs.
    today: Vec<Recommended<Adjustment>>,
    /// An active illness report governs EVERY upcoming day until the user
    /// reports recovery, not only today's session (autoreg-045/046): a head
    /// cold folds into each future day like today; flu or fever blanks them.
    illness: Vec<Recommended<Adjustment>>,
    /// Flu or fever: every upcoming day is held.
    illness_stop: bool,
    past: Rc<PastReadiness>,
    windows: Vec<ReviewWindow>,
}

impl WeekRecs {
    /// The adjustments in force on a judged day (an active illness for a
    /// future day).
    fn on(&self, day: i64, today: i64) -> std::borrow::Cow<'_, [Recommended<Adjustment>]> {
        if day > today {
            std::borrow::Cow::Borrowed(&self.illness)
        } else {
            completion_recs_for_day(day, today, &self.past, &self.today, &self.windows)
        }
    }

    /// Fold an active head cold into a future, non-rest day's row.
    fn fold_illness_into(&self, row: &mut SessionPlanView, weight_lb: bool) {
        if !self.illness_stop && !self.illness.is_empty() && !row.items.is_empty() {
            annotate_adjustments(row, &self.illness, weight_lb);
        }
    }
}

/// Build the whole plan surface (next session + week strip + summary), rendered
/// STRICTLY downstream of the safety gates (HARD RULE 3): a `train_blocked` hold
/// blanks the NEXT SESSION (status "blocked", no load numbers) and mirrors
/// that blanked row back into the week strip; the other week days keep their
/// planned items, except under an active illness report, which governs every
/// upcoming day until the user reports recovery (flu or fever blanks them, a
/// head cold makes each one easy with short easy runs). Otherwise the active
/// readiness + non-blocking review adjustments are folded into today's items,
/// and the first session after a resolved hold is the easy return session even
/// when it falls after today. `None`/empty when no plan is set or the profile
/// raises an onboarding gate.
/// Each calendar day is prescribed from the history logged strictly BEFORE it,
/// so logging a day's sets can never move that day's target; a future day sees
/// everything through today.
pub(in crate::app) fn build_plan_views(
    cx: &ViewCtx,
    train_blocked: bool,
    recommended: &[Recommended<Adjustment>],
    review_recs: &[Recommended<Adjustment>],
) -> (
    Option<SessionPlanView>,
    Vec<SessionPlanView>,
    Option<ProgramSummaryView>,
) {
    let model = cx.model;
    let (Some(profile), Some(req)) = (model.profile.as_ref(), model.plan_request) else {
        return (None, Vec::new(), None);
    };
    let start = req.start_epoch_day;
    let today = model.today_epoch_day.unwrap_or(start);
    let offset = model.today_utc_offset_sec;
    // Saturating so a corrupt wire `today`/`start` near i64::MIN/MAX can't
    // debug-overflow (a panic would brick every later view via the FFI firewall).
    let week_monday = today.saturating_sub(mon0_weekday(today));
    let f = PlanFrame {
        cx,
        model,
        profile,
        today_profile: profile_for_day(model, today).unwrap_or(profile),
        start,
        today,
        offset,
        weight_lb: profile.weight_lb,
        week_monday,
        week_index: plan_week_index(model, &cx.days, week_monday).unwrap_or(0),
    };
    let Some((snapshots, past)) = plan_snapshots(&f) else {
        return (None, Vec::new(), None);
    };
    let mut today_recs: Vec<Recommended<Adjustment>> = recommended.to_vec();
    today_recs.extend(review_recs.iter().cloned());
    let illness = crate::autoreg::illness_plan_recs(&manual_inputs_on(model, today, today, offset));
    let recs = WeekRecs {
        today: today_recs,
        illness_stop: illness.iter().any(|r| r.value == Adjustment::Stop),
        illness,
        past,
        windows: review_windows(model, today, offset),
    };
    let mut week = week_rows(&f, &snapshots, &recs);
    let next_session = resolve_next_session(&f, &snapshots, &recs, train_blocked, &mut week);

    // Flu or fever, or any deferral to a professional (a medical red flag, a
    // persistent structural or tendon pain, an overtraining referral): every
    // upcoming day is held until the user reports recovery or the referral is
    // lifted; no planned distance or load is shown for any of them
    // (safety-040/043, HARD RULE 3). A one-day pain stop or resting-HR rest day
    // keeps the rest of the week.
    let defer_hold = recommended
        .iter()
        .chain(review_recs)
        .any(|r| matches!(r.value, Adjustment::Defer { .. }));
    if recs.illness_stop || defer_hold {
        for row in week
            .iter_mut()
            .filter(|s| s.epoch_day > today && !s.items.is_empty())
        {
            blank_row(row);
        }
    }
    // A past day whose own profile was still gated had no plan that morning.
    for row in week
        .iter_mut()
        .filter(|s| snapshots.gated.contains(&s.epoch_day) && !s.items.is_empty())
    {
        blank_row(row);
    }

    let cur_program = &snapshots.current.1;
    let meso = &cur_program.mesocycle;
    let program_view = Some(build_program_summary(
        f.today_profile,
        meso,
        cur_program,
        program_week(&f, meso),
        f.week_index,
    ));
    (next_session, week, program_view)
}

/// The week's plan snapshots and past-day readiness, from the cache when the
/// key matches. `None` when today's own profile raises a health gate (the
/// whole plan is withheld).
fn plan_snapshots(f: &PlanFrame) -> Option<(Rc<PlanSnapshots>, Rc<PastReadiness>)> {
    let PlanFrame {
        cx,
        model,
        profile,
        today_profile,
        start,
        today,
        offset,
        week_monday,
        week_index,
        ..
    } = *f;
    // The view's one session-day attribution pass, shared by the completion
    // counts (in `flatten_session`) and the per-day snapshot cutoff below.
    let session_days = &cx.days.sets;

    // Anchors + program synthesized from the history attributed to session days
    // `<= through`, with the profile effective on the day being prescribed.
    // Every day is prescribed with `through = day - 1`, so the sets logged ON a
    // day can never move that day's own target. Undated legacy rows
    // (`observed_at <= 0`) have no session day and always count, exactly as
    // `build_plan_anchors` already treats them for decay. Readiness inputs are
    // date-filtered to the same cutoff, so a pain report logged AFTER finishing a
    // rung cannot rewrite that day's rung, and an input removed after the
    // prescribed day `through + 1` still counts for it, so a later removal cannot
    // rewrite it either. `through` is also the
    // decay/window reference, so an anchor ages against the day being prescribed.
    let run_week_km = build_run_week_km(&model.runs, &cx.runs, week_monday, offset);
    let plan_through =
        |through: i64, day_profile: &Profile| -> Option<(crate::plan::Anchors, Program)> {
            let dated_set = |i: usize| session_days[i].is_none_or(|d| d <= through);
            let mut anchors = build_plan_anchors(
                model
                    .sets
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| dated_set(*i))
                    .map(|(_, s)| s),
                AnchorCtx::for_profile(Some(day_profile), model, offset),
                through,
                offset,
            );
            let (longest_recent, recent_weekly, detrained) = cx.run_anchors_on(through);
            anchors.longest_recent_run_km = longest_recent;
            anchors.recent_weekly_km = recent_weekly;
            anchors.longest_run_detrained = detrained;
            anchors.stated_longest_in_window = stated_longest_since(model, day_profile)
                .is_some_and(|d| through.saturating_sub(d) <= LONGEST_RUN_WINDOW_DAYS);
            // Race-date periodization dates the week; the cross-cycle volume
            // carry reads whole calendar weeks before it (RUN-VOLUME-001).
            anchors.week_start_epoch_day = Some(week_monday);
            anchors.plan_start_epoch_day = Some(start);
            anchors.run_week_km = run_week_km.clone();
            // Pace anchor: a fresh recent race (≤8 wk) on the day being
            // prescribed (`through + 1`).
            anchors.run_vdot =
                crate::app::run_targets::pace_anchor_vdot(day_profile, through.saturating_add(1));
            let day_inputs = manual_inputs_on(model, through, through.saturating_add(1), offset);
            anchors.calisthenics = build_calisthenics_anchors(
                model
                    .sets
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| dated_set(*i))
                    .map(|(_, s)| s),
                &calis_placements_through(model, through),
                &day_inputs,
                day_profile.lift_goal,
                Some(through),
                offset,
            );
            #[cfg(test)]
            PLAN_SYNTH_BUILDS.with(|c| c.set(c.get() + 1));
            let program = crate::plan::synthesize(day_profile, &anchors, week_index)?;
            Some((anchors, program))
        };

    // The snapshots are a pure function of the key below, so a view driven by an
    // unrelated event (a calculator query, a readiness echo) reuses them instead
    // of re-synthesizing the week.
    use std::borrow::Cow::Borrowed;
    let key = PlanSnapshotKey {
        sets: cx.sets_fp,
        runs: runs_fingerprint(&cx.runs.keys),
        profile_history: Borrowed(&model.profile_history),
        inputs: Borrowed(&model.inputs),
        retracted: Borrowed(&model.retracted_inputs),
        today,
        offset,
        week_index,
        start,
        off_day: crate::app::anchors::off_day(model, offset),
        calis_placements: Borrowed(&model.calis_placements),
    };
    let readiness_key = (
        checkins_fingerprint(&model.checkins),
        model.hrv_baseline_from,
    );
    let cached = PLAN_SNAPSHOT_CACHE.with(|c| {
        c.borrow().get_if(|k| *k == key).map(|v| {
            let (rk, past) = &v.readiness;
            (
                v.snapshots.clone(),
                (*rk == readiness_key).then(|| past.clone()),
            )
        })
    });
    let past_readiness = || {
        Rc::new(PastReadiness::build(
            model,
            &cx.days,
            profile,
            week_monday,
            today,
        ))
    };
    Some(match cached {
        Some((snapshots, Some(past))) => (snapshots, past),
        Some((snapshots, None)) => {
            let past = past_readiness();
            PLAN_SNAPSHOT_CACHE.with(|c| {
                if let Some(v) = c.borrow_mut().value_mut() {
                    v.readiness = (readiness_key, past.clone());
                }
            });
            (snapshots, past)
        }
        None => {
            // Everything through today: the future days, the wrapped next session
            // and the program summary all read this current picture. A health
            // gate on TODAY's profile withholds the whole plan.
            let current = plan_through(today, today_profile)?;
            let mut past = Vec::with_capacity(7);
            let mut gated = Vec::new();
            for d in 0..7u16 {
                let epoch_day = week_monday.saturating_add(d as i64);
                if epoch_day > today {
                    continue;
                }
                // A past/today day keeps the prescription it had on its own
                // morning: prescribed from the history logged strictly before it,
                // with the profile effective that day. A day whose own profile was
                // still gated (cleared mid-week) had no plan: it renders blocked,
                // never wiping the rest of the week.
                let day_profile = profile_for_day(model, epoch_day).unwrap_or(profile);
                match plan_through(epoch_day.saturating_sub(1), day_profile) {
                    Some((a, p)) => past.push((epoch_day, a, p)),
                    None => gated.push(epoch_day),
                }
            }
            let snapshots = Rc::new(PlanSnapshots {
                current,
                past,
                gated,
            });
            let readiness = past_readiness();
            PLAN_SNAPSHOT_CACHE.with(|c| {
                c.borrow_mut().set(
                    key.into_owned(),
                    PlanCacheValue {
                        snapshots: snapshots.clone(),
                        readiness: (readiness_key, readiness.clone()),
                    },
                );
            });
            (snapshots, readiness)
        }
    })
}

/// The week strip: each weekday's session under this week's arrangement,
/// judged against its own day's logs and adjustments. A past/today day keeps
/// its own morning's snapshot; a future day shares the current one and carries
/// an active illness.
fn week_rows(f: &PlanFrame, snapshots: &PlanSnapshots, recs: &WeekRecs) -> Vec<SessionPlanView> {
    let PlanFrame {
        cx,
        model,
        start,
        today,
        weight_lb,
        week_monday,
        ..
    } = *f;
    let session_days = &cx.days.sets;
    let run_days = &cx.days.runs;
    let ctx = f.flatten_ctx();
    let (cur_anchors, cur_program) = (&snapshots.current.0, &snapshots.current.1);
    // The athlete's session moves for this week (ADH-RESCHED-001): `source[d]`
    // is the plan day whose session shows on weekday `d`.
    let cur_sessions = &cur_program.mesocycle.sessions;
    let idle = idle_days(&cx.days, week_monday, today, start);
    let (source, move_notes) = week_arrangement(model, week_monday, cur_sessions, idle);
    let loads = arranged_loads(&source, cur_sessions);
    // Each weekday's session. `synthesize` fills all 7 days (Rest included), so
    // the lookup always hits.
    let plan_days: Vec<Option<(&crate::plan::Anchors, &Session)>> = (0..7usize)
        .map(|d| {
            let epoch_day = week_monday.saturating_add(d as i64);
            let (a, p) = match snapshots.past.iter().find(|(day, ..)| *day == epoch_day) {
                Some((_, a, p)) => (a, p),
                None => (cur_anchors, cur_program),
            };
            p.mesocycle
                .sessions
                .iter()
                .find(|s| s.day == source[d])
                .map(|s| (a, s))
        })
        .collect();
    // Which weekdays in the plan window, through today, logged each discipline,
    // and which of those logs are spare: the day's own session is another
    // discipline (or rest), so the work can only belong to a swapped or shifted
    // session.
    let logged: [[bool; 2]; 7] = std::array::from_fn(|d| {
        let day = week_monday.saturating_add(d as i64);
        let window = day >= start && day <= today;
        [
            window && session_days.contains(&Some(day)),
            window && run_days.contains(&Some(day)),
        ]
    });
    let spare: [[bool; 2]; 7] = std::array::from_fn(|d| {
        let own = plan_days[d].and_then(|(_, s)| discipline(s.session_type));
        [Discipline::Sets, Discipline::Run].map(|k| logged[d][k as usize] && own != Some(k))
    });
    let mut claimed = [[false; 2]; 7];
    let earliest_move = today.max(start);
    let mut week: Vec<SessionPlanView> = Vec::with_capacity(7);
    for d in 0..7usize {
        let epoch_day = week_monday.saturating_add(d as i64);
        let Some((a, s)) = plan_days[d] else {
            continue;
        };
        let day_recs = recs.on(epoch_day, today);
        let judge = DayJudge {
            day: epoch_day,
            fold: fold_completion_adjustments(&day_recs),
            recs: &day_recs,
        };
        let mut row = flatten_session(s, epoch_day, a, &judge, &ctx);
        // A swapped or shifted session: a past day with none of its own
        // discipline logged is credited with the nearest spare same-discipline
        // session of the week (at most two days away), judged on that day.
        if row.status == SessionStatus::Missed
            && let Some(k) = discipline(s.session_type)
            && !logged[d][k as usize]
        {
            let mut candidates: Vec<usize> = (0..7usize)
                .filter(|&t| {
                    t != d
                        && t.abs_diff(d) <= CREDIT_WINDOW_DAYS
                        && spare[t][k as usize]
                        && !claimed[t][k as usize]
                })
                .collect();
            candidates.sort_by_key(|&t| (t.abs_diff(d), t));
            for t in candidates {
                let from_day = week_monday.saturating_add(t as i64);
                let from_recs = completion_recs_for_day(
                    from_day,
                    today,
                    &recs.past,
                    &recs.today,
                    &recs.windows,
                );
                let from_judge = DayJudge {
                    day: from_day,
                    fold: fold_completion_adjustments(&from_recs),
                    recs: &from_recs,
                };
                let credited = flatten_session(s, epoch_day, a, &from_judge, &ctx);
                if credited.status.is_done() {
                    row = credited;
                    row.credited_from_epoch_day = Some(from_day);
                    claimed[t][k as usize] = true;
                    break;
                }
            }
        }
        if epoch_day > today {
            recs.fold_illness_into(&mut row, weight_lb);
        }
        let src = source[d];
        if usize::from(src) != d && !matches!(s.session_type, SessionType::Rest) {
            row.moved_from_epoch_day = Some(week_monday.saturating_add(src as i64));
        }
        row.move_note = move_notes[d].clone();
        if row.status == SessionStatus::BeforePlan {
            // The plan did not exist yet: nothing to show, log or move.
            row.items.clear();
            row.adjustment = None;
        } else if movable(row.status) {
            fill_move_options(&mut row, loads, idle, d, week_monday, earliest_move);
        }
        week.push(row);
    }
    week
}

/// The hero: the earliest unfinished session from today on, else next week's
/// first training day; then folded with the adjustments in force for it (or
/// blanked under a hold) and mirrored back into the week strip.
fn resolve_next_session(
    f: &PlanFrame,
    snapshots: &PlanSnapshots,
    recs: &WeekRecs,
    train_blocked: bool,
    week: &mut [SessionPlanView],
) -> Option<SessionPlanView> {
    let PlanFrame {
        cx,
        model,
        start,
        today,
        offset,
        weight_lb,
        week_monday,
        ..
    } = *f;
    let (cur_anchors, cur_program) = (&snapshots.current.0, &snapshots.current.1);
    let mut next_session = match week
        .iter()
        .find(|s| s.epoch_day >= today && !s.items.is_empty() && !s.status.is_done())
    {
        Some(s) => {
            let mut n = s.clone();
            // A future day already carrying an illness adjustment stays
            // "adjusted", like today's adjusted session.
            if n.adjustment.is_none() {
                n.status = SessionStatus::Next;
            }
            Some(n)
        }
        // Nothing left this week: wrap to next week's first training day. It is
        // flattened from the CURRENT program, never cloned off a past week row —
        // that row carries ITS OWN morning's snapshot (and its logged-done
        // flags), neither of which belongs on a session dated a week out.
        None => cur_program
            .mesocycle
            .sessions
            .iter()
            .find(|s| !s.prescriptions.is_empty())
            .map(|s| {
                let next_monday = week_monday.saturating_add(7);
                let epoch_day = next_monday.saturating_add(s.day as i64);
                // Dated a week out, so nothing is logged/attributed against it:
                // `flatten_session` sees no matching sets, so nothing is done.
                let judge = DayJudge {
                    day: epoch_day,
                    fold: fold_completion_adjustments(&recs.illness),
                    recs: &recs.illness,
                };
                let mut n = flatten_session(s, epoch_day, cur_anchors, &judge, &f.flatten_ctx());
                n.status = SessionStatus::Next;
                recs.fold_illness_into(&mut n, weight_lb);
                // Its move options come from next week's own arrangement, when
                // the session still sits on its planned day there.
                let cur_sessions = &cur_program.mesocycle.sessions;
                let next_idle = idle_days(&cx.days, next_monday, today, start);
                let (next_source, _) =
                    week_arrangement(model, next_monday, cur_sessions, next_idle);
                let from = usize::from(s.day);
                if next_source.get(from) == Some(&s.day) {
                    let next_loads = arranged_loads(&next_source, cur_sessions);
                    fill_move_options(
                        &mut n,
                        next_loads,
                        next_idle,
                        from,
                        next_monday,
                        today.max(start),
                    );
                }
                n
            }),
    };

    if let Some(ns) = next_session.as_mut() {
        // The hero keeps EVERY item: done items stay with `done == true` so
        // the shell collapses them itself; logging one exercise never vanishes the
        // rest, and the whole session advances only once every item is done.
        if train_blocked || recs.illness_stop {
            // HARD RULE 3: never program through a hold. Blank the session.
            blank_row(ns);
        } else if ns.epoch_day <= today {
            // Fold BOTH the readiness adjustments AND the non-blocking
            // review-channel deloads/downgrades (rpe-load-gap / velocity /
            // failed-session / MRV deloads, HRV downgrade) into the rendered
            // session, so the NextSessionCard loads match a headline that says
            // "deload". Blocking review recs (Stop/RestDay/Defer) fall through
            // `annotate_adjustments`'s catch-all untouched (a blocking one would
            // have set `train_blocked` and blanked the session above). The row
            // is today's, already carrying `today_recs`' fold from
            // `flatten_session`, so only the per-adjustment rewrite runs here.
            annotate_adjustments(ns, &recs.today, weight_lb);
            refresh_item_statuses(&mut ns.items, &recs.today);
        } else if let Some(r) = return_step_on(model, ns.epoch_day, offset) {
            // ready-upd-003/004: the first session after a resolved hold is the
            // easy one (or the post-illness return check) even when it falls
            // after today (the hold was resolved on a rest day).
            annotate_adjustments(ns, std::slice::from_ref(&r), weight_lb);
            refresh_item_statuses(&mut ns.items, std::slice::from_ref(&r));
        }
        // Today's transient readiness/review adjustments fold only into a
        // session dated today; a future session carries an active illness hold
        // (folded above) or the return step. A BLOCKING/safety hold blanks the
        // session above regardless of date (HARD RULE 3).
        // Mirror the resolved next-session row back into the week strip so its
        // deload-folded sets/done match the hero.
        if let Some(row) = week.iter_mut().find(|s| s.epoch_day == ns.epoch_day) {
            *row = ns.clone();
        }
    }
    next_session
}

/// Hold a row: blocked, with no items, adjustment or move options.
fn blank_row(row: &mut SessionPlanView) {
    row.status = SessionStatus::Blocked;
    row.items.clear();
    row.adjustment = None;
    row.move_targets.clear();
    row.move_blocked_reason = None;
}

/// The current program week as `(week, weeks_total)`, 1-based. The program
/// week ADVANCES with completed weeks (ADH-PROGRESS-001), cycling
/// 1..=weeks_total as the block repeats; a recovery week is the last position.
/// `meso.weeks` is the cycle length the synthesizer periodized on. A dated race
/// plan instead counts calendar weeks from the plan's first week to the race
/// week (RUN-PERIOD-001: its cycles are aligned backward from the race, so a
/// 4-week block position would not match the weeks).
fn program_week(f: &PlanFrame, meso: &Mesocycle) -> (u8, u8) {
    let PlanFrame {
        today_profile,
        start,
        week_monday,
        week_index,
        ..
    } = *f;
    let race_weeks_out = today_profile
        .goal_race_epoch_day
        .filter(|race| {
            *race >= week_monday && crate::running::taper_weeks(today_profile.goal_distance) > 0
        })
        .map(|race| (race - week_monday) / 7);
    match race_weeks_out {
        Some(out) => {
            let start_monday = start.saturating_sub(mon0_weekday(start));
            let week = (week_monday.saturating_sub(start_monday) / 7).max(0) + 1;
            let clamp = |w: i64| w.clamp(1, i64::from(u8::MAX)) as u8;
            (clamp(week), clamp(week.saturating_add(out)))
        }
        None => {
            let cycle_len = (meso.weeks as i64).max(1);
            ((week_index as i64 % cycle_len) as u8 + 1, meso.weeks)
        }
    }
}
