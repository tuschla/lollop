use super::*;

/// How close a goal distance must be to [`running::MARATHON_M`] to count as the
/// marathon, m.
const MARATHON_MATCH_TOLERANCE_M: f64 = 10.0;
/// Longest plausible race distance, m (~500 km events with headroom).
/// Sanitized floats are finite; bounding distances makes a 1e12 m input read
/// as no prediction.
const MAX_RACE_DISTANCE_M: f64 = 1_000_000.0;

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

    // running-040/008: derate the displayed band for an under-mileaged
    // marathoner (logged history only); the note below states it.
    let is_marathon = (q.goal_distance_m - running::MARATHON_M).abs() < MARATHON_MATCH_TOLERANCE_M;
    let marathon_derate = if is_marathon {
        longest_logged_km.and_then(|longest| {
            let opt = running::marathon_prediction_optimistic(longest);
            opt.value.then(|| {
                (low_sec, high_sec) = running::marathon_derated_band(low_sec, high_sec, vdot).value;
                (longest, opt)
            })
        })
    } else {
        None
    };

    let goal_label = race_distance_title(q.goal_distance_m / 1000.0);
    let plausible = (1.0..=MAX_RACE_DISTANCE_M).contains(&q.goal_distance_m)
        && (1.0..=MAX_RACE_DISTANCE_M).contains(&q.recent_distance_m);
    let degenerate = !plausible || !low_sec.is_finite() || !high_sec.is_finite() || low_sec <= 0.0;
    // A derate can open a span even where the two methods agreed.
    let single = low_sec == high_sec;
    let predicted = if degenerate {
        "-".to_string()
    } else if single {
        fmt_clock(low_sec)
    } else {
        format!("{}–{}", fmt_clock(low_sec), fmt_clock(high_sec))
    };
    let summary = if degenerate {
        format!("{goal_label}: enter a recent race to get a prediction")
    } else if agreed {
        format!("{goal_label} ≈ {predicted} (two prediction methods agree)")
    } else {
        format!("{goal_label} ≈ {predicted} (range from two prediction methods)")
    };

    // Caveats on the prediction; a degenerate view has none to qualify.
    // running-041: the input race must be recent (≤6 wk, 7–8 marginal, >8 stale).
    let mut notes = Vec::new();
    if !degenerate {
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
        if let Some((longest, opt)) = &marathon_derate {
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
