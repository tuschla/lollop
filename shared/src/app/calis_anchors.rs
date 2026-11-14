//! Calisthenics ladder anchors: the logged sets, rung placements and pain
//! reports the domain replay ([`crate::calisthenics::progression`]) reads.

use crate::app::model::{LoggedSet, Model};
use crate::calisthenics::progression::{self, CalisAnchor, CalisPlacement, RungSet};
use crate::day::{is_undated, local_day};
use crate::schema::ReadinessInput;
use crate::strength::LiftGoal;

impl RungSet for LoggedSet {
    fn weight_kg(&self) -> f64 {
        self.weight_kg
    }
    fn reps(&self) -> u32 {
        self.reps
    }
    fn rpe(&self) -> f64 {
        self.rpe
    }
    fn hold_sec(&self) -> Option<u16> {
        self.hold_sec
    }
    fn observed_at(&self) -> i64 {
        self.observed_at
    }
    fn chrono_cmp(&self, other: &Self) -> std::cmp::Ordering {
        LoggedSet::chrono_cmp(self, other)
    }
}

/// Per-ladder calisthenics tier state ([`progression::build_anchors`]) from
/// `sets` (each with its name's rung, resolved once per distinct name by the
/// caller), the `placements` in force and the day's readiness `inputs`: an
/// active gating pain report (File 08 Table 4.1, `pain_blocks_progression`)
/// freezes progression on every ladder; DOMS never does.
pub(in crate::app) fn build_calisthenics_anchors<'a>(
    sets: impl Iterator<Item = (&'a LoggedSet, Option<(crate::schema::LadderId, u8)>)>,
    placements: &[CalisPlacement],
    inputs: &[ReadinessInput],
    goal: LiftGoal,
    through_day: Option<i64>,
    utc_offset_sec: i64,
) -> Vec<CalisAnchor> {
    progression::build_anchors(
        sets,
        placements,
        crate::autoreg::pain_blocks_progression(inputs),
        goal,
        through_day,
        utc_offset_sec,
    )
}

/// The placements in force for a prescription built from history through local
/// day `through`: every placement dated up to the NEXT day (the day being
/// prescribed; an Easier / Harder choice applies to that same day's item),
/// plus undated ones.
pub(in crate::app) fn calis_placements_through(model: &Model, through: i64) -> Vec<CalisPlacement> {
    let offset = model.today_utc_offset_sec;
    model
        .calis_placements
        .iter()
        .filter(|p| {
            is_undated(p.observed_at)
                || local_day(p.observed_at, offset) <= through.saturating_add(1)
        })
        .copied()
        .collect()
}
