//! The under-18 weekly activity card (YOUTH-PA-001, WHO 2020 ages 5-17):
//! this local Mon–Sun week's logged runs, lift sessions and other activities up
//! to today (a run counts on the day it started) against the WHO reference.
//! Tracking and information only; the youth plan hold is untouched. Counting
//! rules: `extracted/08` youth-pa-001.

use super::*;

/// Longest lift session span counted toward a day's minutes.
const LIFT_SPAN_CAP_MIN: f64 = 180.0;

/// A run's 0-10 session rating at or above this makes its day a vigorous day
/// (WHO: vigorous is usually 7-8 on a 0-10 scale).
const VIGOROUS_RATING: u8 = 7;

/// Every logged activity, chronological ([`chrono_cmp`]), the `lifts` / `runs`
/// order.
pub(super) fn activity_views(acts: &[LoggedActivity]) -> Vec<ActivityResultView> {
    let mut ordered: Vec<&LoggedActivity> = acts.iter().collect();
    ordered.sort_by(|a, b| chrono_cmp(a, b));
    ordered.into_iter().map(to_activity_view).collect()
}

/// Log order for activities: `observed_at`, then `entry_id`.
fn chrono_cmp(a: &LoggedActivity, b: &LoggedActivity) -> std::cmp::Ordering {
    a.observed_at
        .cmp(&b.observed_at)
        .then(a.entry_id.cmp(&b.entry_id))
}

fn to_activity_view(a: &LoggedActivity) -> ActivityResultView {
    ActivityResultView {
        entry_id: a.entry_id,
        observed_at: a.observed_at,
        kind: a.kind,
        minutes: a.minutes,
        vigorous: a.vigorous,
        strengthening: a.strengthening,
    }
}

/// The card, when today's profile is under 18 and the shell has sent today.
pub(super) fn build_youth_activity(cx: &ViewCtx) -> Option<YouthActivityView> {
    let model = cx.model;
    let today = model.today_epoch_day?;
    if !profile_for_day(model, today).is_some_and(|p| p.health.youth) {
        return None;
    }
    let offset = model.today_utc_offset_sec;
    // Saturating: a corrupt log's SetToday near i64::MIN/MAX must not overflow.
    let monday = today.saturating_sub(mon0_weekday(today));
    let end_of_today = end_of_local_today_sec(model)?;
    // The local day of a dated entry inside this week up to today, as an index 0..7.
    let week_day = |observed_at: i64| {
        if is_undated(observed_at) || observed_at > end_of_today {
            return None;
        }
        let d = local_day(observed_at, offset).saturating_sub(monday);
        (0..7).contains(&d).then_some(d as usize)
    };
    let mut vigorous = [false; 7];
    let mut strengthening = [false; 7];

    let mut run_minutes = 0.0;
    // Chronological (`RunFacts::chrono`): the minute total is a float sum. A run
    // counts on the day it started; `started` is 0 (undated) for an undated run.
    for &i in &cx.runs.chrono {
        let (r, min) = (&model.runs[i], cx.runs.minutes[i]);
        let Some(d) = week_day(cx.runs.started[i]) else {
            continue;
        };
        if min.is_finite() && min > 0.0 {
            run_minutes += min;
        }
        strengthening[d] = true;
        if r.session_rpe.is_some_and(|x| x >= VIGOROUS_RATING) {
            vigorous[d] = true;
        }
    }

    let mut first_last: [Option<(i64, i64)>; 7] = [None; 7];
    let mut set_count = [0u32; 7];
    for s in &model.sets {
        let Some(d) = week_day(s.observed_at) else {
            continue;
        };
        strengthening[d] = true;
        set_count[d] += 1;
        first_last[d] = Some(
            first_last[d].map_or((s.observed_at, s.observed_at), |(a, b)| {
                (a.min(s.observed_at), b.max(s.observed_at))
            }),
        );
    }
    let lift_minutes: f64 = first_last
        .iter()
        .zip(set_count)
        .filter(|&(_, n)| n >= 2)
        .filter_map(|(span, _)| *span)
        .map(|(first, last)| ((last - first) as f64 / 60.0).min(LIFT_SPAN_CAP_MIN))
        .sum();

    let mut week_entries: Vec<&LoggedActivity> = Vec::new();
    for a in &model.activities {
        let Some(d) = week_day(a.observed_at) else {
            continue;
        };
        vigorous[d] |= a.vigorous;
        strengthening[d] |= a.strengthening;
        week_entries.push(a);
    }
    week_entries.sort_by(|a, b| chrono_cmp(a, b));
    // Summed chronologically, never in storage order (an Undo re-pushes a row).
    let activity_minutes: f64 = week_entries.iter().map(|a| a.minutes).sum();

    let whole = |m: f64| m.round() as u32;
    let (run_minutes, lift_minutes, activity_minutes) = (
        whole(run_minutes),
        whole(lift_minutes),
        whole(activity_minutes),
    );
    let days = |flags: [bool; 7]| flags.iter().filter(|&&f| f).count() as u8;
    let targets = individualization::youth_activity_targets();
    Some(YouthActivityView {
        week_monday_epoch_day: monday,
        // Saturating: a corrupt log's run of >= ~4.3e9 minutes saturates
        // `run_minutes` at u32::MAX; the total must not wrap below it.
        minutes: run_minutes
            .saturating_add(lift_minutes)
            .saturating_add(activity_minutes),
        run_minutes,
        lift_minutes,
        activity_minutes,
        daily_target_minutes: targets.value.daily_minutes,
        weekly_target_minutes: targets.value.weekly_minutes,
        vigorous_days: days(vigorous),
        strengthening_days: days(strengthening),
        target_days: targets.value.days_per_week,
        week_entries: week_entries.into_iter().map(to_activity_view).collect(),
        evidence: EvidenceTagView::of(&targets),
        why: why_from(None, &targets),
    })
}
