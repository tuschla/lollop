use crate::app::format::{fmt_clock, fmt_km_or_mi, race_distance_title};
use crate::app::guidance::push_guidance;
use crate::app::model::RaceQuery;
use crate::app::view_types::{EvidenceTagView, RacePredictionView};
use crate::load;
use crate::profile::DistanceUnit;
use crate::running::{self, GoalDistance};
use crate::schema::Recommended;

/// Longest plausible race distance, m (~500 km events with headroom).
/// Sanitized floats are finite; bounding distances makes a 1e12 m input read
/// as no prediction.
const MAX_RACE_DISTANCE_M: f64 = 1_000_000.0;

/// The finish-time band a prediction is rendered from.
pub(in crate::app) struct RaceBand {
    /// The two methods agreed (a single time); false = a range.
    pub(in crate::app) agreed: bool,
    /// Range bounds in seconds (equal when `agreed`, until a marathon derate
    /// opens them).
    pub(in crate::app) low_sec: f64,
    pub(in crate::app) high_sec: f64,
    equivalency: Recommended<running::Equivalency>,
    /// The longest logged run that derated a marathon band, and the rule.
    marathon_derate: Option<(f64, Recommended<bool>)>,
}

/// Combine a Daniels VDOT projection and a Riegel projection (running-039);
/// an under-mileaged marathoner's band is derated (running-040/008, logged
/// history only).
pub(in crate::app) fn race_band(q: &RaceQuery, longest_logged_km: Option<f64>) -> RaceBand {
    let exponent = load::riegel_exponent(q.weekly_km);
    let riegel_sec = load::riegel_predict(
        q.recent_time_sec,
        q.recent_distance_m,
        q.goal_distance_m,
        exponent,
    );
    let vdot = load::vdot(q.recent_distance_m, q.recent_time_sec);
    let daniels_sec = load::daniels_predict(vdot, q.goal_distance_m);
    let equivalency = running::race_equivalency(riegel_sec, daniels_sec);
    let (agreed, mut low_sec, mut high_sec) = match equivalency.value {
        running::Equivalency::Agreed(mid) => (true, mid, mid),
        running::Equivalency::Range(lo, hi) => (false, lo, hi),
    };
    let is_marathon = GoalDistance::Marathon.is_race_distance(q.goal_distance_m);
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
    RaceBand {
        agreed,
        low_sec,
        high_sec,
        equivalency,
        marathon_derate,
    }
}

/// The graded finish-time prediction of [`race_band`]. Agreement within ~2%
/// collapses to a single time; otherwise a low–high range is shown so neither
/// method's false precision stands alone. Degenerate input yields no
/// prediction (`predicted` is `None`). Distances in the notes read in `unit`.
pub(in crate::app) fn to_race_view(
    q: &RaceQuery,
    longest_logged_km: Option<f64>,
    unit: DistanceUnit,
) -> RacePredictionView {
    let RaceBand {
        agreed,
        low_sec,
        high_sec,
        equivalency,
        marathon_derate,
    } = race_band(q, longest_logged_km);
    let goal_label = race_distance_title(q.goal_distance_m / 1000.0);
    let plausible = (1.0..=MAX_RACE_DISTANCE_M).contains(&q.goal_distance_m)
        && (1.0..=MAX_RACE_DISTANCE_M).contains(&q.recent_distance_m);
    let degenerate = !plausible || !low_sec.is_finite() || !high_sec.is_finite() || low_sec <= 0.0;
    // A derate can open a span even where the two methods agreed.
    let single = low_sec == high_sec;
    let predicted = if degenerate {
        None
    } else if single {
        Some(fmt_clock(low_sec))
    } else {
        Some(format!("{}–{}", fmt_clock(low_sec), fmt_clock(high_sec)))
    };
    let summary = match &predicted {
        None => format!("{goal_label}: enter a recent race to get a prediction"),
        Some(p) if agreed => format!("{goal_label} ≈ {p} (two prediction methods agree)"),
        Some(p) => format!("{goal_label} ≈ {p} (range from two prediction methods)"),
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
            let note = format!(
                "Your longest logged run is {}. Without longer runs, marathon predictions come out too fast, so this one is slowed by about {:.0}–{:.0} VDOT points",
                fmt_km_or_mi(*longest, unit),
                derate.value.0,
                derate.value.1
            );
            push_guidance(&mut notes, "Prediction", note, opt);
        }
    }

    RacePredictionView {
        goal_label,
        predicted,
        summary,
        glossary: Vec::new(),
        evidence: EvidenceTagView::of(&equivalency),
        notes,
        recent_distance_m: q.recent_distance_m,
        recent_time_sec: q.recent_time_sec,
        goal_distance_m: q.goal_distance_m,
        weekly_km: q.weekly_km,
        weeks_since_race: q.weeks_since_race,
    }
}
