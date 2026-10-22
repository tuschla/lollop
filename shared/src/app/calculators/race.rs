use super::*;

/// The Cooper formula's distance intercept, m (LOAD-COOPER-001:
/// `VO2max ≈ (d − 504.9) / 44.73`); at or below it the estimate is ≤ 0.
const COOPER_FLOOR_M: f64 = 504.9;

/// Cooper 12-min-test VO2max estimate (File 07 formulas; LOAD-COOPER-001).
/// A distance at/below the formula floor yields an explanatory row, never a
/// negative estimate.
pub(in crate::app) fn build_cooper(distance_m_12min: f64) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let g = graded((), claims::LOAD_COOPER_001);
    if !distance_m_12min.is_finite() || distance_m_12min <= COOPER_FLOOR_M {
        push_guidance(
            &mut rows,
            "Cooper test",
            format!(
                "12-minute distance too short to estimate VO2max: the formula floor is ~{COOPER_FLOOR_M:.0} m"
            ),
            &g,
        );
        return rows;
    }
    let vo2 = load::cooper_vo2max(distance_m_12min);
    push_guidance(
        &mut rows,
        "Cooper test",
        format!(
            "Cooper 12-min test: {distance_m_12min:.0} m → estimated VO2max {vo2:.1} ml/kg/min"
        ),
        &g,
    );
    rows
}

/// Critical-Speed protocol fit (running-009; RUN-CS-PROTOCOL-001): CS + D′
/// when the 2–5-effort protocol validates, otherwise the specific violation
/// explained. The advisory ideal-pairing note rides along when both fit and
/// the effort set misses the 3–8 min + 12–30 min windows.
pub(in crate::app) fn build_critical_speed(efforts: &[CsEffortIn]) -> Vec<GuidanceView> {
    let mut rows = Vec::new();
    let cs_efforts: Vec<load::CsEffort> = efforts
        .iter()
        .map(|e| load::CsEffort {
            distance_m: e.distance_m,
            time_sec: e.time_sec,
        })
        .collect();
    let fit = load::critical_speed_checked(&cs_efforts);
    match fit.value {
        Ok(f) => {
            let pace = if f.cs_m_per_s > 0.0 {
                format!("{}/km", fmt_mss(1000.0 / f.cs_m_per_s))
            } else {
                "-".to_string()
            };
            push_guidance(
                &mut rows,
                "Critical speed",
                format!(
                    "Critical Speed {:.2} m/s ({pace}) · D′ {:.0} m from {} maximal efforts",
                    f.cs_m_per_s,
                    f.d_prime_m,
                    cs_efforts.len()
                ),
                &fit,
            );
            if !load::cs_pairing_ideal(&cs_efforts) {
                push_guidance(
                    &mut rows,
                    "Critical speed",
                    "Protocol note: the ideal pairing is one 3–8 min and one 12–30 min effort"
                        .to_string(),
                    &fit,
                );
            }
        }
        Err(v) => {
            let why = match v {
                load::CsProtocolViolation::TooFewEfforts => "needs at least 2 maximal efforts",
                load::CsProtocolViolation::TooManyEfforts => "uses at most 5 maximal efforts",
                load::CsProtocolViolation::DurationOutOfRange => {
                    "each effort must last 2–30 minutes"
                }
                load::CsProtocolViolation::DegenerateDurations => {
                    "efforts must differ in duration to fit a slope"
                }
                load::CsProtocolViolation::NegativeDPrime => {
                    "fitted D′ is negative. A trial was likely not maximal; re-test"
                }
                load::CsProtocolViolation::NonPositiveSlope => {
                    "These efforts imply speed decreasing with duration — re-check the distances and times."
                }
            };
            push_guidance(
                &mut rows,
                "Critical speed",
                format!("No Critical-Speed estimate: the protocol {why}"),
                &fit,
            );
        }
    }
    rows
}

/// Format a duration in seconds as a race clock ([`fmt_clock`]). Non-positive
/// input renders as `-` (no false prediction).
pub(in crate::app) fn fmt_race_clock(sec: f64) -> String {
    if sec <= 0.0 {
        return "-".to_string();
    }
    fmt_clock(sec)
}

/// Marathon distance, m: the only goal the running-040/008 long-run derate
/// applies to.
const MARATHON_M: f64 = 42_195.0;
/// How close a goal distance must be to [`MARATHON_M`] to count as the
/// marathon, m.
const MARATHON_MATCH_TOLERANCE_M: f64 = 10.0;

/// Combine a Daniels VDOT projection and a Riegel projection into a graded
/// finish-time prediction (running-039). Agreement within ~2% collapses to a
/// single time; otherwise a low–high range is shown so neither method's false
/// precision stands alone. Degenerate input yields an empty (`-`) prediction.
pub(in crate::app) fn to_race_view(
    q: &RaceQuery,
    longest_logged_km: Option<f64>,
) -> RacePredictionView {
    let exponent = load::riegel_exponent(q.weekly_km);
    let riegel_sec = load::riegel_predict(
        q.recent_time_sec,
        q.recent_distance_m,
        q.goal_distance_m,
        exponent,
    );
    let vdot = load::vdot(q.recent_distance_m, q.recent_time_sec);
    let daniels_sec = load::daniels_predict(vdot, q.goal_distance_m);
    let eq = running::race_equivalency(riegel_sec, daniels_sec);
    let (agreed, mut low_sec, mut high_sec) = match eq.value {
        running::Equivalency::Agreed(mid) => (true, mid, mid),
        running::Equivalency::Range(lo, hi) => (false, lo, hi),
    };

    // running-040/008 (flag AND derate): a marathon prediction from a runner
    // without long-run support runs optimistic, so shift the DISPLAYED band
    // SLOWER by the cited VDOT-point derate, not merely a caveat. Gated on the
    // logged run history (no history → no claim → band untouched); computed
    // once here so the evidence-bearing caveat note below reuses it. ONLY the
    // marathon goal is affected; every other distance and an
    // adequately-mileaged marathon keep their band.
    let marathon_derate = if (q.goal_distance_m - MARATHON_M).abs() < MARATHON_MATCH_TOLERANCE_M {
        longest_logged_km.and_then(|longest| {
            let opt = running::marathon_prediction_optimistic(longest);
            opt.value.then(|| {
                (
                    longest,
                    opt,
                    running::marathon_derated_band(low_sec, high_sec, vdot),
                )
            })
        })
    } else {
        None
    };
    if let Some((_, _, band)) = &marathon_derate {
        low_sec = band.value.0;
        high_sec = band.value.1;
    }

    let goal_label = race_distance_title(q.goal_distance_m / 1000.0);
    // Degenerate when the band is empty/non-physical OR the distances are not
    // plausibly raceable. The wire floats are sanitized (finite, clamped to
    // ±1e12 — see `sanitize_f64`), so an absurd input like 1e300 yields a
    // *finite* multi-century band rather than Inf; bounding both distances to
    // the longest raceable events (~500 km, with headroom) sends it down the
    // same "-" path as a non-finite band. `is_finite` keeps the guard
    // self-contained.
    const MAX_RACE_DISTANCE_M: f64 = 1_000_000.0;
    let plausible = (1.0..=MAX_RACE_DISTANCE_M).contains(&q.goal_distance_m)
        && (1.0..=MAX_RACE_DISTANCE_M).contains(&q.recent_distance_m);
    let degenerate = !plausible || !low_sec.is_finite() || !high_sec.is_finite() || low_sec <= 0.0;
    // Display shape follows the ACTUAL band: a derate can open a span even where
    // the two methods agreed. Without a derate `low_sec == high_sec` iff the
    // methods agreed (`Equivalency::Agreed` sets the bounds equal, `Range` sets
    // them strictly apart).
    let single = low_sec == high_sec;
    let predicted = if degenerate {
        "-".to_string()
    } else if single {
        fmt_race_clock(low_sec)
    } else {
        format!("{}–{}", fmt_race_clock(low_sec), fmt_race_clock(high_sec))
    };
    let summary = if degenerate {
        format!("{goal_label}: enter a recent race to get a prediction")
    } else if agreed {
        format!("{goal_label} ≈ {predicted} (two prediction methods agree)")
    } else {
        format!("{goal_label} ≈ {predicted} (range from two prediction methods)")
    };

    // Graded caveats riding on the prediction. running-041: the input race
    // must be recent (≤6 wk strict, 7–8 marginal, >8 stale → re-test).
    let mut notes = Vec::new();
    if let Some(weeks) = q.weeks_since_race {
        let fresh = running::race_input_freshness(weeks);
        match fresh.value {
            running::RaceInputFreshness::Fresh => {}
            running::RaceInputFreshness::Marginal => push_guidance(
                &mut notes,
                "Prediction",
                format!(
                    "Your race was {weeks} weeks ago, close to the 6–8 weeks after which predictions go stale"
                ),
                &fresh,
            ),
            running::RaceInputFreshness::Stale => push_guidance(
                &mut notes,
                "Prediction",
                format!(
                    "Your race was {weeks} weeks ago, more than 8. Race or time-trial again before trusting this prediction"
                ),
                &fresh,
            ),
        }
    }
    // running-040: marathon predictions run optimistic without long-run
    // support. Judged from the logged run history; no history → no claim. The
    // displayed band was already derated above (flag AND derate); this keeps
    // the flag + the ~2–3 VDOT-point magnitude, and both the note (`opt`) and
    // the derated band carry RUN-VDOT-001 evidence (HARD RULE 2).
    if let Some((longest, opt, _band)) = &marathon_derate {
        // running-008: the matching correction, derate the projection by
        // ~2–3 VDOT points for an under-mileaged marathoner.
        let derate = running::vdot_derate_points(GoalDistance::Marathon, true);
        let note = |mi: bool| {
            format!(
                "Your longest logged run is {}. Without longer runs, marathon predictions come out too fast, so this one is slowed by about {:.0}–{:.0} VDOT points",
                fmt_km_or_mi(*longest, mi),
                derate.value.0,
                derate.value.1
            )
        };
        push_guidance_mi(&mut notes, "Prediction", note(false), note(true), opt);
    }

    RacePredictionView {
        goal_label,
        predicted,
        agreed,
        low_sec,
        high_sec,
        summary,
        evidence: EvidenceTagView::of(&eq),
        notes,
        recent_distance_m: q.recent_distance_m,
        recent_time_sec: q.recent_time_sec,
        goal_distance_m: q.goal_distance_m,
        weekly_km: q.weekly_km,
        weeks_since_race: q.weeks_since_race,
    }
}
