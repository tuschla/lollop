use super::*;

/// All dated (`observed_at > 0`) runs with their realised distance, moving
/// duration, and avg-HR zone, in log order. QC-filtered for GPS tracks.
fn dated_runs(model: &Model, facts: &RunFacts) -> Vec<DatedRun> {
    model
        .runs
        .iter()
        .zip(facts.km.iter().zip(&facts.minutes))
        .filter(|(r, _)| r.observed_at > 0)
        .map(|(r, (&km, &minutes))| DatedRun {
            observed_at: r.observed_at,
            km,
            minutes,
            zone: (r.hr_pct_max > 0.0).then(|| running::classify_three_zone(r.hr_pct_max)),
        })
        .collect()
}

/// Representative training-age years for the running progression caps
/// (running-031 splits only novice `<1 yr` from everyone else). Same mapping
/// `build_guidance` uses; without a profile the engine keeps its beginner-safe
/// posture and treats the athlete as a novice.
fn training_age_years(profile: Option<&Profile>) -> f64 {
    match profile.map(|p| individualization::training_age_from_cadence(p.progression_cadence).value)
    {
        Some(individualization::TrainingAge::Novice) | None => 0.5,
        Some(_) => 2.0,
    }
}

/// Whether a planned day is a quality run (threshold, interval, repetition,
/// race-pace or hill work), the sessions running-023's spacing rule governs.
fn is_quality_session_type(session_type: SessionType) -> bool {
    matches!(session_type, SessionType::Run(k) if crate::plan::is_quality_run(k))
}

/// Whether a logged run counts as a HARD session for the running-023 spacing
/// check and the File 10 CAP-3 heavy-leg spacing:
/// - its workout tag or average %HRmax makes it threshold/interval work
///   (`running::classify_hard_run`; heart rate is optional);
/// - else, unless the athlete tagged it easy/steady/long, it sits on a planned
///   quality day (running-012: the session-goal count is valid for plan
///   design), or its average pace is at/faster than the slow end of the
///   athlete's threshold band (running-007 VDOT table), so phone-only runs
///   without heart rate are judged too. `(km, min)` is the run's realised
///   distance and moving time.
fn is_hard_run(
    r: &LoggedRun,
    (km, min): (f64, f64),
    offset: i64,
    planned_quality_days: &[i64],
    threshold_slow_pace_sec_per_km: Option<f64>,
) -> bool {
    if running::classify_hard_run(r.hr_pct_max, r.workout_type.as_ref())
        != running::HardRunKind::None
    {
        return true;
    }
    if matches!(
        r.workout_type,
        Some(
            WorkoutType::Steady | WorkoutType::Easy | WorkoutType::Recovery | WorkoutType::LongRun
        )
    ) {
        return false;
    }
    if planned_quality_days.contains(&local_day(r.observed_at, offset)) {
        return true;
    }
    threshold_slow_pace_sec_per_km
        .is_some_and(|slow| km > 0.0 && min > 0.0 && min * 60.0 / km <= slow)
}

/// Heavy leg work for the hybrid-012 / CAP-3 spacing check (HYB-CAP-001): a
/// squat- or hinge-pattern set at or above this %1RM, the floor of the
/// max-strength band (STR-HEAVYFREQ-001; File 02 strength-001 80–100 %).
const HEAVY_LEG_PCT_1RM: f64 = 80.0;

/// Sum of the realised km of `runs` falling in local week `w`, in log order.
fn week_km(runs: &[DatedRun], offset: i64, w: i64) -> f64 {
    runs.iter()
        .filter(|r| local_week(r.observed_at, offset) == w)
        .map(|r| r.km)
        .sum()
}

/// The most recent logged week's runs and the figures the weekly-report rows
/// share.
struct WeekRuns {
    /// Every dated run at or before the end of local today, in log order. A
    /// future-dated (mis-imported) run never counts toward any week.
    runs: Vec<DatedRun>,
    /// End of local today (`end_of_local_today_sec`); later runs are future-dated.
    cutoff: Option<i64>,
    offset: i64,
    /// The most recent NON-future logged week.
    cur_week: i64,
    weekly_km: f64,
    prev_km: f64,
    baseline2_km: f64,
    long_run_km: f64,
    /// `observed_at` of this week's HARD runs ([`is_hard_run`]), log order.
    hard_runs: Vec<i64>,
    recovery_this_week: bool,
    prev_was_recovery: bool,
}

impl WeekRuns {
    /// `None` until a dated, non-future run exists.
    fn new(cx: &ViewCtx, week_plan: &[SessionPlanView]) -> Option<Self> {
        let model = cx.model;
        let offset = model.today_utc_offset_sec;
        // Anchor "this week" on the most recent NON-future logged week, so a
        // mis-imported 2030 run can't hijack the weekly guidance until it's deleted.
        let cutoff = end_of_local_today_sec(model);
        let runs: Vec<DatedRun> = dated_runs(model, &cx.runs)
            .into_iter()
            .filter(|r| cutoff.is_none_or(|c| r.observed_at <= c))
            .collect();
        let cur_week = runs
            .iter()
            .map(|r| local_week(r.observed_at, offset))
            .max()?;
        let weekly_km = week_km(&runs, offset, cur_week);
        let prev_km = week_km(&runs, offset, cur_week - 1);
        let baseline2_km = week_km(&runs, offset, cur_week - 2);
        let long_run_km = runs
            .iter()
            .filter(|r| local_week(r.observed_at, offset) == cur_week)
            .map(|r| r.km)
            .fold(0.0_f64, f64::max);

        // This week's HARD runs: shared by the running-023 quality spacing
        // check and the CAP-3 heavy-leg spacing.
        let planned_quality_days: Vec<i64> = week_plan
            .iter()
            .filter(|s| is_quality_session_type(s.session_type))
            .map(|s| s.epoch_day)
            .collect();
        let pace_day = model
            .today_epoch_day
            .unwrap_or_else(|| cur_week.saturating_mul(7).saturating_add(10));
        let threshold_slow_pace = model
            .profile
            .as_ref()
            .and_then(|p| crate::app::run_targets::pace_anchor_vdot(p, pace_day))
            .and_then(|v| load::vdot_band_pace_sec_per_km(v, crate::schema::VdotBand::Threshold))
            .map(|(_, slow)| slow);
        let hard_runs: Vec<i64> = model
            .runs
            .iter()
            .zip(cx.runs.km.iter().zip(&cx.runs.minutes))
            .filter(|(r, _)| {
                r.observed_at > 0
                    && cutoff.is_none_or(|c| r.observed_at <= c)
                    && local_week(r.observed_at, offset) == cur_week
            })
            .filter(|&(r, (&km, &min))| {
                is_hard_run(
                    r,
                    (km, min),
                    offset,
                    &planned_quality_days,
                    threshold_slow_pace,
                )
            })
            .map(|(r, _)| r.observed_at)
            .collect();

        // Plan-awareness: a recovery week is a PLANNED cut, so the athlete's own logged
        // reduction must not be flagged, and the week AFTER a recovery week must ratio
        // against the last LOAD week, not the deloaded one. Mondays fall on epoch days
        // ≡ 4 (mod 7) — epoch day 0 is a Thursday — so local-week `w`'s Monday is
        // `w * 7 + 4`. False without a profile/plan.
        let is_recovery = |w: i64| -> bool {
            model.profile.as_ref().is_some_and(|profile| {
                crate::app::plan_views::plan_week_index(model, &cx.days, w * 7 + 4)
                    .is_some_and(|wi| crate::plan::planned_recovery_week(profile, wi, w * 7 + 4))
            })
        };
        let recovery_this_week = is_recovery(cur_week);
        let prev_was_recovery = is_recovery(cur_week - 1);

        Some(Self {
            runs,
            cutoff,
            offset,
            cur_week,
            weekly_km,
            prev_km,
            baseline2_km,
            long_run_km,
            hard_runs,
            recovery_this_week,
            prev_was_recovery,
        })
    }

    /// This week's runs, log order.
    fn current(&self) -> impl Iterator<Item = &DatedRun> {
        self.runs
            .iter()
            .filter(move |r| local_week(r.observed_at, self.offset) == self.cur_week)
    }

    /// The week-over-week ramp baseline and its label suffix. After a recovery
    /// week, ratio against the last LOAD week (two weeks back), so the planned
    /// rebound off the deload is not scored as an over-cap jump. Shared by the
    /// running-031 row and the hybrid-021 row, so the two ramp checks always
    /// judge the same baseline and can never contradict.
    fn ramp_base(&self) -> (f64, &'static str) {
        if self.prev_was_recovery && self.baseline2_km > 0.0 {
            (
                self.baseline2_km,
                " vs your last full week; last week was the planned lighter week",
            )
        } else {
            (self.prev_km, "")
        }
    }
}

/// A km figure as a number in the line's unit, one decimal ("12.3"), the
/// miles form dropping a trailing ".0" like [`fmt_mi_of_km`].
fn dist_num(km: f64, mi: bool) -> String {
    let s = fmt_km_or_mi(km, mi);
    s.strip_suffix(if mi { " mi" } else { " km" })
        .map_or_else(|| s.clone(), str::to_string)
}

/// "20.0 → 26.0 km" / "12.4 → 16.2 mi": a week-to-week distance change.
fn dist_change(from_km: f64, to_km: f64, mi: bool) -> String {
    format!("{} → {}", dist_num(from_km, mi), fmt_km_or_mi(to_km, mi))
}

/// A weekly-distance threshold in whole units ("64 km", "40 mi").
fn dist_whole(km: f64, mi: bool) -> String {
    if mi {
        format!("{:.0} mi", km * 1000.0 / running::MILE_M)
    } else {
        format!("{km:.0} km")
    }
}

/// The weekly running-volume system (File 04): aggregates the most recent
/// logged (epoch-)week of dated runs and runs the progression/cap/distribution
/// validators over it. Zone-dependent shares are counted by the avg-HR
/// time-in-zone convention, running-012's stated default for distribution
/// *reporting*, and say so. `week_plan` (the current week's planned days, may
/// be empty) lets the hard-session checks count a run on a planned quality
/// day. Empty until a dated run exists.
pub(in crate::app) fn build_weekly_report(
    cx: &ViewCtx,
    week_plan: &[SessionPlanView],
) -> Vec<GuidanceView> {
    let model = cx.model;
    let mut rows = Vec::new();
    let Some(week) = WeekRuns::new(cx, week_plan) else {
        return rows;
    };
    push_increase_row(&mut rows, model, &week);
    push_two_week_ramp_row(&mut rows, &week);
    if week.weekly_km > 0.0 {
        push_counting_method_row(&mut rows);
        // Run-day denominator: the plan's capped run-day count (goal-table
        // ceiling + hybrid concurrency cap) via `run_slot_count`, so the
        // ≤2×-daily-average guardrail here and the plan's long-run sizing average
        // over the same run days.
        let n_run_days = model
            .profile
            .as_ref()
            .filter(|p| p.running_days_per_week > 0)
            .map(crate::plan::run_slot_count)
            .unwrap_or(7);
        let daily_avg_km = crate::plan::daily_avg_run_km(week.weekly_km, n_run_days);
        push_volume_cap_row(&mut rows, cx, &week, daily_avg_km);
        push_daily_avg_row(&mut rows, &week, daily_avg_km);
        push_long_run_share_row(&mut rows, &week);
    }
    push_easy_share_row(&mut rows, &week);
    push_quality_row(&mut rows, &week);
    push_hybrid_ramp_row(&mut rows, model, &week);
    push_heavy_leg_spacing_row(&mut rows, cx, &week);
    push_bump_hold_row(&mut rows, model);
    rows
}

/// Week-over-week increase vs the training-age cap (running-031). Only
/// meaningful with a prior week to ratio against; skipped when THIS week is the
/// planned recovery week (a cut is expected, not a violation).
fn push_increase_row(rows: &mut Vec<GuidanceView>, model: &Model, week: &WeekRuns) {
    if week.recovery_this_week {
        return;
    }
    let weekly_km = week.weekly_km;
    let (base_km, base_label) = week.ramp_base();
    if base_km > 0.0 {
        let ok = running::weekly_increase_ok(
            base_km,
            weekly_km,
            training_age_years(model.profile.as_ref()),
        );
        let pct = (weekly_km - base_km) / base_km * 100.0;
        let text = |mi: bool| {
            format!(
                "Week-over-week {} ({pct:+.0}%): {}{base_label}",
                dist_change(base_km, weekly_km, mi),
                if ok.value {
                    "within the increase cap"
                } else {
                    "over the increase cap; hold or trim next week"
                }
            )
        };
        push_guidance_mi(rows, "Weekly volume", text(false), text(true), &ok);
    }
}

/// Two-week ramp flag (running-028: >30% over two weeks ≈ 1.6× injury odds).
/// After a recovery week the two-week baseline is the load week before the
/// deload (three weeks back), so the planned rebound is not double-counted.
/// Skipped when THIS week is the planned recovery week.
fn push_two_week_ramp_row(rows: &mut Vec<GuidanceView>, week: &WeekRuns) {
    if week.recovery_this_week {
        return;
    }
    let weekly_km = week.weekly_km;
    let ramp_baseline = if week.prev_was_recovery {
        week_km(&week.runs, week.offset, week.cur_week - 3)
    } else {
        week.baseline2_km
    };
    if ramp_baseline > 0.0 {
        let flag = running::two_week_increase_flag(ramp_baseline, weekly_km);
        if flag.value {
            let text = |mi: bool| {
                format!(
                    "Distance up more than 30% in two weeks ({}). That raises injury risk; build more slowly",
                    dist_change(ramp_baseline, weekly_km, mi)
                )
            };
            push_guidance_mi(rows, "Weekly volume", text(false), text(true), &flag);
        }
    }
}

/// Zone-counting convention note (running-012): time-in-zone is the
/// reporting default; the T/I shares below are avg-HR-classified.
fn push_counting_method_row(rows: &mut Vec<GuidanceView>) {
    let method = running::default_counting_method(false);
    push_guidance(
        rows,
        "Weekly volume",
        "The shares below count time in each heart-rate zone, from each run's average heart rate"
            .to_string(),
        &method,
    );
}

/// Daniels weekly-share caps (running-016/018/019): the long-run share plus
/// the T/I shares, each run classified by its workout-type tag or (untagged)
/// its average %HRmax. A correctly executed tempo counts as threshold, never
/// interval, volume — one average %HRmax per run cannot be split into zones,
/// so the tag governs when present. No rep (R) distance is derivable from
/// logged data, so R is counted as 0.
fn push_volume_cap_row(
    rows: &mut Vec<GuidanceView>,
    cx: &ViewCtx,
    week: &WeekRuns,
    daily_avg_km: f64,
) {
    let model = cx.model;
    let (weekly_km, long_run_km) = (week.weekly_km, week.long_run_km);
    let cutoff = week.cutoff;
    let hard_km = |kind: running::HardRunKind| -> f64 {
        model
            .runs
            .iter()
            .zip(&cx.runs.km)
            .filter(|(r, _)| {
                r.observed_at > 0
                    && cutoff.is_none_or(|c| r.observed_at <= c)
                    && local_week(r.observed_at, week.offset) == week.cur_week
            })
            .filter(|(r, _)| {
                running::classify_hard_run(r.hr_pct_max, r.workout_type.as_ref()) == kind
            })
            .map(|(_, &km)| km)
            .sum()
    };
    let threshold_km = hard_km(running::HardRunKind::Threshold);
    let interval_km = hard_km(running::HardRunKind::Interval);

    let caps = running::check_volume_caps(long_run_km, threshold_km, interval_km, 0.0, weekly_km);
    // The ≤25% single-run share is Daniels' cap for runners at/above ~64
    // km/wk; below that the KB's operative bound is ≤2× the average daily
    // distance (running-016). A long run inside the 2× bound under 64 km/wk
    // is informational, not a violation.
    if let Some(running::CapViolation::LongRun) = caps.value
        && weekly_km < running::LONG_RUN_SHARE_SPLIT_KM
        && long_run_km <= 2.0 * daily_avg_km
    {
        let pct = long_run_km / weekly_km * 100.0;
        let cap = 2.0 * daily_avg_km;
        let text = |mi: bool| {
            format!(
                "Long run {} is {pct:.0}% of the week. The 25% limit applies from {} a week; under that, the limit is twice your average daily distance ({}).",
                fmt_km_or_mi(long_run_km, mi),
                dist_whole(running::LONG_RUN_SHARE_SPLIT_KM, mi),
                fmt_km_or_mi(cap, mi)
            )
        };
        push_guidance_mi(
            rows,
            "Weekly volume",
            text(false),
            text(true),
            &running::long_run_within_daily_avg(long_run_km, daily_avg_km),
        );
    } else {
        let text = |mi: bool| match caps.value {
            None => format!(
                "Long run and hard running within weekly limits ({}: long run {}, threshold {}, intervals {})",
                fmt_km_or_mi(weekly_km, mi),
                dist_num(long_run_km, mi),
                dist_num(threshold_km, mi),
                dist_num(interval_km, mi)
            ),
            Some(running::CapViolation::LongRun) => format!(
                "Long run {} is {:.0}% of the week: over the ≤25% single-run cap",
                fmt_km_or_mi(long_run_km, mi),
                long_run_km / weekly_km * 100.0
            ),
            Some(running::CapViolation::Threshold) => format!(
                "Threshold volume {} exceeds the ≤10% weekly cap",
                fmt_km_or_mi(threshold_km, mi)
            ),
            Some(running::CapViolation::Interval) => format!(
                "Interval volume {} exceeds the ≤8% weekly cap",
                fmt_km_or_mi(interval_km, mi)
            ),
            Some(running::CapViolation::Repetition) => {
                "Repetition volume exceeds the ≤5% weekly cap".to_string()
            }
        };
        let mi_text = if matches!(caps.value, Some(running::CapViolation::Repetition)) {
            String::new()
        } else {
            text(true)
        };
        push_guidance_mi(rows, "Weekly volume", text(false), mi_text, &caps);
    }
}

/// running-016 alternative long-run bound: ≤2× the average daily distance,
/// averaged over RUN days (not calendar days) so the report and the plan
/// agree via `daily_avg_run_km`. Only shown when violated: the share row
/// already states the primary band.
fn push_daily_avg_row(rows: &mut Vec<GuidanceView>, week: &WeekRuns, daily_avg_km: f64) {
    let long_run_km = week.long_run_km;
    let daily_avg = running::long_run_within_daily_avg(long_run_km, daily_avg_km);
    if !daily_avg.value {
        let text = |mi: bool| {
            format!(
                "Long run {} exceeds 2× your average daily distance ({}): outsized relative to the week",
                fmt_km_or_mi(long_run_km, mi),
                fmt_km_or_mi(daily_avg_km, mi)
            )
        };
        push_guidance_mi(rows, "Weekly volume", text(false), text(true), &daily_avg);
    }
}

/// Long-run share vs the running-016 default band.
fn push_long_run_share_row(rows: &mut Vec<GuidanceView>, week: &WeekRuns) {
    let long_run_km = week.long_run_km;
    let share = running::long_run_share_default();
    let text = |mi: bool| {
        format!(
            "Long run {} is {:.0}% of the week (usual share {:.0}–{:.0}%)",
            fmt_km_or_mi(long_run_km, mi),
            long_run_km / week.weekly_km * 100.0,
            share.value.0 * 100.0,
            share.value.1 * 100.0
        )
    };
    push_guidance_mi(rows, "Weekly volume", text(false), text(true), &share);
}

/// Easy-share floor (running-011): ~80% of running time easy, counted over
/// the HR-carrying runs.
fn push_easy_share_row(rows: &mut Vec<GuidanceView>, week: &WeekRuns) {
    let hr_minutes: f64 = week
        .current()
        .filter(|r| r.zone.is_some())
        .map(|r| r.minutes)
        .sum();
    if hr_minutes > 0.0 {
        let easy_minutes: f64 = week
            .current()
            .filter(|r| r.zone == Some(crate::schema::ThreeZone::Z1))
            .map(|r| r.minutes)
            .sum();
        let frac = easy_minutes / hr_minutes;
        let floor = running::easy_share_floor_ok(frac);
        push_guidance(
            rows,
            "Intensity",
            format!(
                "Easy (Z1) share {:.0}% of run time: {}",
                frac * 100.0,
                if floor.value {
                    "meets the 80% easy floor"
                } else {
                    "below the ~80% easy floor; make easy days easier"
                }
            ),
            &floor,
        );
    }
}

/// Quality-plan governance (running-023): count this week's HARD runs, their
/// minimum spacing, and back-to-back hard days. A run is hard when its workout
/// tag or average heart rate says so (`classify_hard_run`), when it sits on a
/// planned quality day (running-012 allows the session-goal count for plan
/// design), or when its average pace is at/faster than the athlete's threshold
/// pace — so runs without a heart-rate strap count too.
fn push_quality_row(rows: &mut Vec<GuidanceView>, week: &WeekRuns) {
    let offset = week.offset;
    let mut quality: Vec<i64> = week.hard_runs.clone();
    quality.sort_unstable();
    if !quality.is_empty() {
        let min_gap_hours = quality
            .windows(2)
            .map(|w| (w[1] - w[0]) / 3600)
            .min()
            .unwrap_or(i64::from(u8::MAX))
            .clamp(0, i64::from(u8::MAX)) as u8;
        let consecutive_hard = quality
            .windows(2)
            .any(|w| (local_day(w[1], offset) - local_day(w[0], offset)) <= 1);
        let ok = running::quality_plan_ok(
            quality.len().min(255) as u8,
            min_gap_hours,
            consecutive_hard,
        );
        push_guidance(
            rows,
            "Intensity",
            format!(
                "{} hard session{} this week: {}",
                quality.len(),
                if quality.len() == 1 { "" } else { "s" },
                if ok.value {
                    "within the hard-run limits (at most 3 a week, at least 48 h apart)"
                } else {
                    "outside the hard-run limits (at most 3 a week, at least 48 h apart, no back-to-back hard days)"
                }
            ),
            &ok,
        );
    }
}

/// Hybrid combined-load ramp guard (hybrid-021): a concurrent athlete keeps
/// weekly running growth ≤~10% to bound stacked mechanical+systemic load.
/// Same baseline and same recovery-week skip as the running-031 row.
fn push_hybrid_ramp_row(rows: &mut Vec<GuidanceView>, model: &Model, week: &WeekRuns) {
    let weekly_km = week.weekly_km;
    let (ramp_base_km, ramp_base_label) = week.ramp_base();
    if let Some(p) = model.profile.as_ref()
        && p.weekly_sets > 0
        && !week.recovery_this_week
        && ramp_base_km > 0.0
    {
        let ok = hybrid::combined_load_progression_ok(ramp_base_km, weekly_km);
        if ok.value == Some(false) {
            let text = |mi: bool| {
                format!(
                    "Running distance up more than 10% ({}{ramp_base_label}) while you also lift: keep the weekly increase to about 10% or less",
                    dist_change(ramp_base_km, weekly_km, mi)
                )
            };
            push_guidance_mi(rows, "Hybrid", text(false), text(true), &ok);
        }
    }
}

/// `observed_at` of this week's runs the heavy-leg spacing guards, sorted and
/// deduplicated: the HARD runs plus the long run.
///
/// A "long run" is the week's LONGEST run AND ≥25% of a full-week basis: the
/// planned weekly km, else the trailing-7-day total. For a PURE LIFTER (lifts
/// programmed, running not configured) an incidental jog is not a programmed
/// long run, so it counts only when real running history certifies it —
/// otherwise the 25% test is self-referential (25% of the jog's own distance)
/// and would flag a recovery jog near leg day. A configured runner (or an
/// unknown profile) keeps the week's-longest rule.
fn spaced_run_times(model: &Model, week: &WeekRuns) -> Vec<i64> {
    let long_run_km = week.long_run_km;
    let planned_km = model
        .profile
        .as_ref()
        .map(|p| p.running_km_per_week.max(0.0))
        .unwrap_or(0.0);
    let pure_lifter = model.profile.as_ref().is_some_and(|p| {
        p.weekly_sets > 0 && p.running_days_per_week == 0 && p.running_km_per_week <= 0.0
    });
    let mut spaced_runs: Vec<i64> = week
        .current()
        .filter(|r| r.km > 0.0 && r.km >= long_run_km - 1e-9)
        .filter(|r| {
            let trailing: f64 = week
                .runs
                .iter()
                .filter(|o| {
                    o.observed_at > r.observed_at - 7 * DAY_SEC && o.observed_at <= r.observed_at
                })
                .map(|o| o.km)
                .sum();
            let external_ok = !pure_lifter || planned_km > 0.0 || trailing - r.km > 1e-9;
            external_ok && r.km >= running::LONG_RUN_SHARE_MIN * planned_km.max(trailing)
        })
        .map(|r| r.observed_at)
        .collect();
    spaced_runs.extend(week.hard_runs.iter().copied());
    spaced_runs.sort_unstable();
    spaced_runs.dedup();
    spaced_runs
}

/// hybrid-012 / CAP-3: keep heavy leg work ≥24 h from a HARD or LONG run, both
/// directions (residual fatigue lasts 24–48 h), including a same-day
/// lift-then-run pair: the lift-first order (hybrid-005) is the right order
/// WHEN the two must share a day, not an exemption from the spacing. A
/// same-day pair ≥6 h apart meets the same-day separation floor (hybrid-007,
/// CONC-SEP-001) and gets a softer note; under 6 h, or any cross-day pair
/// under 24 h, is the CAP-3 warning. Heavy leg work = a squat- or
/// hinge-pattern set at or above [`HEAVY_LEG_PCT_1RM`] %1RM. Hard runs are the week's
/// quality runs (tag, heart rate, plan slot or pace); the long run is the
/// week's LONGEST run that also clears 25% of a full-week volume basis
/// ([`spaced_run_times`]) — a short Z1 recovery jog is neither hard nor long.
fn push_heavy_leg_spacing_row(rows: &mut Vec<GuidanceView>, cx: &ViewCtx, week: &WeekRuns) {
    let model = cx.model;
    let offset = week.offset;
    // Heavy is judged against the lift's demonstrated e1RM anchor when one
    // exists (actual load ≥ 0.8× anchor), else the rep/RPE estimate; a
    // bodyweight set with no RPE has no knowable intensity and is not heavy.
    let anchors = cx.lift_anchors_now();
    let spaced_runs = spaced_run_times(model, week);
    // Closest heavy-leg pairing per kind: (gap hours, same local day).
    let mut warn_h: Option<f64> = None;
    let mut same_day_ok_h: Option<f64> = None;
    for &run_at in &spaced_runs {
        let run_day = local_day(run_at, offset);
        for s in model.sets.iter().filter(|s| s.observed_at > 0) {
            let set_day = local_day(s.observed_at, offset);
            if (set_day - run_day).abs() > 1 {
                continue;
            }
            let heavy_leg = matches!(
                crate::plan::movement_pattern(&s.exercise),
                Some(crate::plan::MovementPattern::Squat | crate::plan::MovementPattern::Hinge)
            ) && set_intensity_pct(s, anchors.e1rm_for(&s.exercise))
                .is_some_and(|p| p >= HEAVY_LEG_PCT_1RM);
            if !heavy_leg {
                continue;
            }
            let h = (s.observed_at - run_at).abs() as f64 / 3600.0;
            if hybrid::heavy_leg_run_gap_ok(h).value {
                continue;
            }
            let same_day_floor = f64::from(hybrid::session_spacing().value.ideal_min_hours);
            if set_day == run_day && h >= same_day_floor {
                same_day_ok_h = Some(same_day_ok_h.map_or(h, |m| m.min(h)));
            } else {
                warn_h = Some(warn_h.map_or(h, |m| m.min(h)));
            }
        }
    }
    if let Some(h) = warn_h {
        let gap_ok = hybrid::heavy_leg_run_gap_ok(h);
        push_guidance(
            rows,
            "Hybrid",
            format!(
                "Heavy leg work and a hard or long run only {h:.0} h apart: keep ≥{:.0} h between them (residual fatigue lasts 24–48 h)",
                hybrid::HEAVY_LEG_RUN_GAP_H
            ),
            &gap_ok,
        );
    } else if let Some(h) = same_day_ok_h {
        let spacing = hybrid::session_spacing();
        push_guidance(
            rows,
            "Hybrid",
            format!(
                "Heavy legs and a hard or long run {h:.0} h apart on the same day: that gap works (at least {} h), but {:.0} h or more is better",
                spacing.value.ideal_min_hours,
                hybrid::HEAVY_LEG_RUN_GAP_H
            ),
            &spacing,
        );
    }
}

/// Novice volume-bump hold (running-031): hold 2–3 weeks between bumps.
fn push_bump_hold_row(rows: &mut Vec<GuidanceView>, model: &Model) {
    let hold = running::novice_volume_bump_hold_weeks(training_age_years(model.profile.as_ref()));
    if let Some((lo, hi)) = hold.value {
        push_guidance(
            rows,
            "Weekly volume",
            format!(
                "After raising your weekly distance, hold it {} weeks before raising it again",
                fmt_u8_range(lo, hi)
            ),
            &hold,
        );
    }
}
