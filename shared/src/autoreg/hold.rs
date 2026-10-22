//! Graded return after a pain / illness hold (ready-upd-003/004).

use super::pain::{PainGate, pain_gate};
use super::rules::{illness_downgrade, illness_stop};
use crate::evidence::{self, claims};
use crate::schema::{Adjustment, ReadinessInput, ReadinessSignal, Recommended};

/// Which hold just ended, deciding the first session back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnKind {
    /// Fever or below-neck illness: a 10–20 min return check at 60–70 % of
    /// usual effort (ILLNESS-RTT-001).
    AfterFever,
    /// Head-cold illness: one easy session (RETURN-HOLD-001).
    AfterIllness,
    /// A pain stop: one easy session (RETURN-HOLD-001).
    AfterPain,
}

/// One Pain/Illness report with the moment it stopped being in force: `None`
/// while active, `Some(t)` once retracted by a dated "I'm better" / "pain-free"
/// removal at unix second `t`.
#[derive(Debug, Clone, Copy)]
pub struct TimedInput<'a> {
    pub input: &'a ReadinessInput,
    pub retracted_at: Option<i64>,
}

/// How severe an illness report in force is; ordered so the worse level wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
enum IllnessLevel {
    #[default]
    None,
    HeadCold,
    FeverOrBelowNeck,
}

/// The hold in force over a set of reports: the illness level and whether a
/// pain stop (whole-day or scoped sharp/joint/uncharacterized) applies.
#[derive(Debug, Clone, Copy, Default)]
struct HoldState {
    illness: IllnessLevel,
    pain: bool,
}

impl HoldState {
    fn of(active: &[ReadinessInput]) -> Self {
        let illness = if illness_stop(active) {
            IllnessLevel::FeverOrBelowNeck
        } else if illness_downgrade(active) {
            IllnessLevel::HeadCold
        } else {
            IllnessLevel::None
        };
        let pain = matches!(pain_gate(active), PainGate::Block(_) | PainGate::Scoped(_));
        HoldState { illness, pain }
    }

    fn on(self) -> bool {
        self.illness > IllnessLevel::None || self.pain
    }

    /// The episode's worst state so far.
    fn worst(self, other: HoldState) -> HoldState {
        HoldState {
            illness: self.illness.max(other.illness),
            pain: self.pain || other.pain,
        }
    }

    /// The episode's worst state decides the return (fever > head cold > pain).
    fn return_kind(self) -> ReturnKind {
        match self.illness {
            IllnessLevel::FeverOrBelowNeck => ReturnKind::AfterFever,
            IllnessLevel::HeadCold => ReturnKind::AfterIllness,
            IllnessLevel::None => ReturnKind::AfterPain,
        }
    }
}

/// The most recent end of a pain/illness hold, when no hold is in force now:
/// `(kind, unix second it ended)`. A hold ends when the user retracts the
/// report as resolved ("I'm better" / "I'm pain-free", a dated removal) or a
/// newer report supersedes it (an "all clear" check-in answer or pain-clear).
/// An undated "logged by mistake" removal deletes the report outright and is
/// never a recovery. The episode's worst state decides the kind (fever >
/// head cold > pain).
pub fn last_hold_resolution(timeline: &[TimedInput]) -> Option<(ReturnKind, i64)> {
    let relevant: Vec<&TimedInput> = timeline
        .iter()
        .filter(|t| {
            matches!(
                t.input.signal,
                ReadinessSignal::Pain | ReadinessSignal::Illness
            )
        })
        .collect();
    let mut times: Vec<i64> = relevant
        .iter()
        .flat_map(|t| std::iter::once(t.input.observed_at).chain(t.retracted_at))
        .collect();
    times.sort_unstable();
    times.dedup();
    let mut episode: Option<HoldState> = None;
    let mut last: Option<(ReturnKind, i64)> = None;
    let mut on_now = false;
    for &t in &times {
        let active: Vec<ReadinessInput> = relevant
            .iter()
            .filter(|x| x.input.observed_at <= t && x.retracted_at.is_none_or(|r| r > t))
            .map(|x| x.input.clone())
            .collect();
        let state = HoldState::of(&active);
        let on = state.on();
        if on {
            episode = Some(episode.unwrap_or_default().worst(state));
        } else if let Some(worst) = episode.take() {
            last = Some((worst.return_kind(), t));
        }
        on_now = on;
    }
    if on_now { None } else { last }
}

/// The first-session-back adjustment for a resolved hold (ready-upd-003/004):
/// after fever / below-neck illness the session becomes the IOC return check
/// (ILLNESS-RTT-001); after a head cold or a pain stop it runs easy
/// (RETURN-HOLD-001: lifts ≥3 reps in reserve, runs easy).
pub fn return_adjustment(kind: ReturnKind) -> Recommended<Adjustment> {
    match kind {
        ReturnKind::AfterFever => {
            evidence::graded(Adjustment::ReturnCheck, claims::ILLNESS_RTT_001)
        }
        ReturnKind::AfterIllness | ReturnKind::AfterPain => {
            evidence::graded(Adjustment::DowngradeSession, claims::RETURN_HOLD_001)
        }
    }
}
