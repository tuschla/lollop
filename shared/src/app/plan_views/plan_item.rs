//! The plan day and item the view pipeline works on: the day's fields and
//! each item's wire view plus the numbers the folds, holds and cues read and
//! write. Rendered once, at the end of the plan section
//! ([`PlanRow::into_view`]).

use std::ops::{Deref, DerefMut};

use crate::app::view_types::{
    AdjustmentView, PlanIntensity, PrescriptionView, SessionPlanView, SessionStatus,
};
use crate::profile::DistanceUnit;
use crate::schema::{RunSessionType, SessionType};

/// One plan item: its wire view (read through `Deref`) and the working state
/// behind it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlanItem {
    pub(crate) view: PrescriptionView,
    /// Structured form of `intensity_label`: readiness adjustments (CapRpe,
    /// load reductions) change THIS and the label is re-rendered from it, so no
    /// adjustment parses the display string. `Prose` is the free-text label.
    pub(crate) intensity: PlanIntensity,
    /// For an interval/repetition run, the rep count. `0` for a continuous
    /// run or a lift: the `summary` then carries the whole-session volume. When
    /// nonzero the `summary` already reads as "`N × <rep_volume> · <pace>`".
    pub(crate) rep_count: u8,
    /// For a run item: the shown whole-session volume as a number, so a
    /// readiness cut or an easy downgrade rebuilds `summary` without parsing it.
    pub(crate) run_volume: Option<RunAmount>,
    /// For an interval/repetition run: the per-rep volume behind `rep_volume`.
    pub(crate) rep_amount: Option<RunAmount>,
    /// For a run item: the judged day's runs cover at least half its planned
    /// volume, so a run under the done threshold still reads done lighter.
    pub(crate) run_lighter: bool,
    /// The target is an RPE (`intensity_label` "RPE 8", `target_rpe` its
    /// value), so the face phrases it as reps left.
    pub(crate) rpe_target: bool,
    /// The heaviest weight already lifted on this movement for at least
    /// `reps_low` reps in the recent anchor window, before this day: the load a
    /// held day (ready-upd-001, no load added) may not exceed.
    pub(crate) hold_load_cap_kg: Option<f64>,
    /// `anchored_on` is a last-time line naming one set: the face already
    /// shows the last top set.
    pub(crate) anchored_on_single_set: bool,
    /// The day took this lift's load away (an easy / pain-free day): the face
    /// says to pick a weight for the reps-left target.
    pub(crate) pick_load: bool,
    /// The prescription `why.basis` states as the plan derived it, before any
    /// fold, hold or in-session cue moved the face.
    pub(crate) basis_rx: Option<BasisRx>,
    /// A held day (ready-upd-001) clamped `load_kg` to `hold_load_cap_kg`.
    pub(crate) load_held: bool,
    /// The figures `run_target` was resolved from, so a downgrade re-derives
    /// the easy band from them.
    pub(crate) target_anchor: Option<TargetAnchor>,
}

impl Deref for PlanItem {
    type Target = PrescriptionView;

    fn deref(&self) -> &PrescriptionView {
        &self.view
    }
}

impl DerefMut for PlanItem {
    fn deref_mut(&mut self) -> &mut PrescriptionView {
        &mut self.view
    }
}

impl From<PrescriptionView> for PlanItem {
    fn from(view: PrescriptionView) -> Self {
        PlanItem {
            view,
            ..PlanItem::default()
        }
    }
}

/// The figures a run item's target was resolved from
/// (`run_targets::apply_run_targets`): the session's run type, the fitness
/// score behind the paces, the heart-rate figures (zero when HRmax is unknown)
/// and the unit the pace label reads in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct TargetAnchor {
    pub(in crate::app) kind: Option<RunSessionType>,
    pub(in crate::app) vdot: Option<f64>,
    pub(in crate::app) hr: HrAnchor,
    pub(in crate::app) unit: DistanceUnit,
}

impl TargetAnchor {
    pub(in crate::app) fn new(vdot: Option<f64>, hr: HrAnchor, unit: DistanceUnit) -> Self {
        TargetAnchor {
            kind: None,
            vdot: vdot.filter(|v| v.is_finite() && *v > 0.0),
            hr: if hr.max_bpm > 0.0 {
                hr
            } else {
                HrAnchor::default()
            },
            unit,
        }
    }
}

/// The heart-rate figures behind a run target.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct HrAnchor {
    pub(in crate::app) max_bpm: f64,
    pub(in crate::app) measured: bool,
    pub(in crate::app) rest_bpm: f64,
}

/// One plan day as the pipeline works it: the day's [`SessionPlanView`]
/// fields and its working items. `run_item` and `adjustment_on_item` are not
/// here: `today_surface::mark_today` fills them on the rendered view.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlanRow {
    pub(crate) epoch_day: i64,
    pub(crate) title: String,
    pub(crate) session_type: SessionType,
    pub(crate) status: SessionStatus,
    pub(crate) items: Vec<PlanItem>,
    pub(crate) adjustment: Option<AdjustmentView>,
    pub(crate) move_targets: Vec<i64>,
    pub(crate) moved_from_epoch_day: Option<i64>,
    pub(crate) move_note: Option<String>,
    pub(crate) move_blocked_reason: Option<String>,
    pub(crate) credited_from_epoch_day: Option<i64>,
}

impl PlanRow {
    /// The day as the shells read it.
    pub(crate) fn into_view(self) -> SessionPlanView {
        SessionPlanView {
            epoch_day: self.epoch_day,
            title: self.title,
            session_type: self.session_type,
            status: self.status,
            items: self.items.into_iter().map(|it| it.view).collect(),
            adjustment: self.adjustment,
            move_targets: self.move_targets,
            moved_from_epoch_day: self.moved_from_epoch_day,
            move_note: self.move_note,
            move_blocked_reason: self.move_blocked_reason,
            credited_from_epoch_day: self.credited_from_epoch_day,
            run_item: None,
            adjustment_on_item: false,
        }
    }
}

/// A lift's `why.basis` as the prescription arm wrote it, in pieces, so it is
/// rendered once against the face the day ends up showing
/// (`plan_views::render_basis`): the load, the `"3×5"` sets×reps lead and the
/// reps left each as planned, and the pieces that state them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BasisRx {
    pub(crate) load_kg: Option<f64>,
    pub(crate) lead: String,
    pub(crate) rir: Option<i64>,
    pub(crate) pieces: Vec<BasisPiece>,
}

/// One piece of a lift's basis sentence.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BasisPiece {
    Text(String),
    /// A sets×reps lead as written (`"3×5"`).
    Lead(String),
    /// `"with ~2 reps left"` ([`with_reps_left`](crate::app::plan_views)).
    RepsLeft(i64),
    /// `"at least two left"`.
    AtLeast(i64),
    /// `"could lift about N times"` for `reps` reps with `rir` in reserve.
    CouldLift {
        reps: i64,
        rir: i64,
    },
    /// The working load, kg.
    Load(f64),
}

/// A run item's volume as a number (the label lives in `summary` /
/// `rep_volume`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RunAmount {
    Minutes(f64),
    /// Kilometres as the plan quantised them, shown by `fmt_run_km` ("8 km",
    /// "6.5 km").
    PlanKm(f32),
    /// A minutes band ("10–20 min"), never scaled.
    MinutesRange(u16, u16),
    /// Kilometres after a readiness change, shown to one decimal ("4.0 km").
    Km(f64),
    /// Run/walk bouts: no single number to scale.
    RunWalk,
}
