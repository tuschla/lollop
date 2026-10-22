use super::*;

/// CTL/ATL/TSB bookkeeping over the logged run history (File 07
/// impulse-response; LOAD-PMC-001). Daily load is a Lucia 3-zone TRIMP: each
/// dated, HR-carrying run's moving minutes weighted 1/2/3 by the 3-zone
/// classification of its average %HRmax (File 07 "Lucia TRIMP"). Lifts and
/// HR-less runs are counted as skipped HERE: a rated HR-less run carries its
/// load in the separate session-RPE series ([`build_srpe_load`],
/// LOAD-SRPE-001), which is not interchangeable with HR load and is never
/// summed into this chain. Time enters only via each event's `observed_at`
/// (no clock in the core).
pub(in crate::app) fn build_training_load(
    model: &Model,
    facts: &RunFacts,
) -> Option<TrainingLoadView> {
    use std::collections::BTreeMap;

    let mut daily: BTreeMap<i64, f64> = BTreeMap::new();
    let mut counted = 0u32;
    let mut skipped = 0u32;
    // A future-dated run (mis-imported 2030 GPX) would set the chain's `last`
    // day into the future, sliding the whole MAX_LOAD_DAYS window off the real
    // history so CTL/ATL/TSB reflect only that phantom run. Drop it here.
    let cutoff = end_of_local_today_sec(model);
    for (r, &minutes) in model.runs.iter().zip(&facts.minutes) {
        if r.observed_at <= 0
            || r.hr_pct_max <= 0.0
            || minutes <= 0.0
            || cutoff.is_some_and(|c| r.observed_at > c)
        {
            skipped += 1;
            continue;
        }
        let zone_minutes = match running::classify_three_zone(r.hr_pct_max) {
            crate::schema::ThreeZone::Z1 => [minutes, 0.0, 0.0],
            crate::schema::ThreeZone::Z2 => [0.0, minutes, 0.0],
            crate::schema::ThreeZone::Z3 => [0.0, 0.0, minutes],
        };
        *daily
            .entry(local_day(r.observed_at, model.today_utc_offset_sec))
            .or_insert(0.0) += load::lucia_trimp(zone_minutes);
        counted += 1;
    }
    // Logged lift sets never contribute load: no KB HR-free load formula (above).
    // Count each distinct lifting DAY as one skipped session, not one per set — a
    // "session" is a day of training, so a 20-set day is one skipped session.
    let lift_days: std::collections::BTreeSet<i64> = model
        .sets
        .iter()
        .map(|s| local_day(s.observed_at, model.today_utc_offset_sec))
        .collect();
    skipped += lift_days.len() as u32;

    let (&first, _) = daily.first_key_value()?;
    let (&last, _) = daily.last_key_value()?;
    // Bound the daily chain to the most recent MAX_LOAD_DAYS (see the constant):
    // the EWMAs have long converged by then, and this caps the loop so a corrupt
    // or mis-unit'd `observed_at` can never spin `view()` into an unbounded loop.
    let start = first.max(last - (MAX_LOAD_DAYS - 1));
    let (mut ctl, mut atl) = (0.0_f64, 0.0_f64);
    for day in start..=last {
        let l = daily.get(&day).copied().unwrap_or(0.0);
        ctl = load::ctl(ctl, l);
        atl = load::atl(atl, l);
    }
    let span_days = last - start + 1;
    let tsb = load::tsb(ctl, atl);
    let round1 = |x: f64| (x * 10.0).round() / 10.0;
    let g = graded((), claims::LOAD_PMC_001);
    Some(TrainingLoadView {
        ctl: round1(ctl),
        atl: round1(atl),
        tsb: round1(tsb),
        days: span_days as u32,
        sessions_counted: counted,
        sessions_skipped: skipped,
        method: "Heart-rate training load (Lucía 3-zone TRIMP, run-average %HRmax), smoothed into a 42-day fitness average (CTL) and a 7-day fatigue average (ATL)".to_string(),
        summary: format!(
            "Fitness (CTL) {:.1} · Fatigue (ATL) {:.1} · Form (TSB) {:+.1} over {} days. A running tally of training, not a prediction of performance",
            round1(ctl),
            round1(atl),
            round1(tsb),
            span_days
        ),
        evidence: EvidenceTagView::of(&g),
    })
}

/// The session-RPE load series (Foster: moving minutes × whole-session CR-10;
/// LOAD-SRPE-001) for this week vs last, from the run-progress weekly series.
/// Its own series, never mixed into the heart-rate TRIMP. `None` until a run
/// in the 16-week window is rated.
pub(in crate::app) fn build_srpe_load(p: &RunProgressView) -> Option<SrpeLoadView> {
    if !p.weekly.iter().any(|w| w.rated_runs > 0) {
        return None;
    }
    let cur = p.weekly.last()?;
    let prev_au = p
        .weekly
        .len()
        .checked_sub(2)
        .and_then(|i| p.weekly.get(i))
        .map_or(0.0, |w| w.srpe_load_au);
    let unrated = cur.runs.saturating_sub(cur.rated_runs);
    let g = graded((), claims::LOAD_SRPE_001);
    Some(SrpeLoadView {
        week_au: cur.srpe_load_au,
        prev_week_au: prev_au,
        rated_runs: cur.rated_runs,
        unrated_runs: unrated,
        method: "Effort load: minutes of running × your 0–10 rating of the whole run, given about 30 minutes afterwards. Kept separate from heart-rate load.".to_string(),
        summary: if cur.rated_runs == 0 {
            format!(
                "No rated runs this week{} · last week {prev_au:.0}",
                if unrated > 0 {
                    format!(" ({unrated} not rated yet)")
                } else {
                    String::new()
                }
            )
        } else {
            format!(
                "Effort load {:.0} this week ({} rated run{}{}) · last week {prev_au:.0}",
                cur.srpe_load_au,
                cur.rated_runs,
                if cur.rated_runs == 1 { "" } else { "s" },
                if unrated > 0 {
                    format!(", {unrated} not rated yet")
                } else {
                    String::new()
                }
            )
        },
        grade: Some(g.grade()),
        citation: g.reference().to_owned(),
        confidence: g.confidence(),
    })
}

/// Flatten the autoreg per-signal states into wire rows, resolving each row's
/// judging claim to its registry evidence (rows with no judging rule keep an
/// empty tag, they state facts, not recommendations).
pub(in crate::app) fn build_readiness_summary(
    inputs: &[ReadinessInput],
    ctx: &autoreg::RuleContext,
    // Recency-expired readings routed to `stale`, forwarded so the derived
    // RHR/wellness rows drop to an "out of date" state.
    stale: &[ReadinessInput],
) -> Vec<ReadinessSignalView> {
    autoreg::signal_states_in(inputs, ctx, stale)
        .into_iter()
        .map(|s| {
            let evidence = s
                .claim
                .map(|id| EvidenceTagView::of(&graded((), id)))
                .unwrap_or_default();
            ReadinessSignalView {
                signal: s.signal,
                group: autoreg::signal_group(s.signal),
                value: s.value,
                streak: s.streak,
                state: s.state,
                detail: s.detail,
                evidence,
                from_today: s.from_today,
                stale: s.stale,
            }
        })
        .collect()
}
