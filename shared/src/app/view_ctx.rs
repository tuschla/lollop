//! Data every `view()` derives once and threads to the builders that read it,
//! so no builder re-walks the history for a fact another already computed.

use std::cell::OnceCell;
use std::rc::Rc;

use crate::app::anchors::{build_run_anchors, lift_anchors_now};
use crate::app::calis_anchors::build_calisthenics_anchors;
use crate::app::logged_days::LoggedDays;
use crate::app::model::Model;
use crate::app::run_metrics::RunFacts;
use crate::app::set_index::{SetIndex, set_index};

pub(super) struct ViewCtx<'m> {
    pub(super) model: &'m Model,
    /// [`crate::memo::fingerprint`] of `model.sets`.
    pub(super) sets_fp: u64,
    /// Name-derived facts of `model.sets`.
    pub(super) set_index: Rc<SetIndex>,
    /// Realised distance/time and QC'd track per run, aligned to `model.runs`.
    pub(super) runs: RunFacts,
    /// Session-day attribution of every set and run.
    pub(super) days: LoggedDays,
    /// The derived facts of the current profile (none without a profile).
    pub(super) plan_profile: Option<crate::profile::PlanProfile<'m>>,
    lift_anchors_now: OnceCell<crate::plan::LiftAnchors>,
    calis_anchors_now: OnceCell<Vec<crate::calisthenics::progression::CalisAnchor>>,
    run_anchors_today: OnceCell<crate::plan::RunAnchors>,
}

impl<'m> ViewCtx<'m> {
    pub(super) fn new(model: &'m Model) -> Self {
        let sets_fp = crate::memo::fingerprint(&model.sets);
        let runs = RunFacts::new(&model.runs);
        let days = LoggedDays::new(model, &runs);
        Self {
            model,
            sets_fp,
            set_index: set_index(&model.sets, sets_fp),
            runs,
            days,
            plan_profile: model.profile().map(crate::profile::PlanProfile::new),
            lift_anchors_now: OnceCell::new(),
            calis_anchors_now: OnceCell::new(),
            run_anchors_today: OnceCell::new(),
        }
    }

    /// The e1RM anchors per lift as of today ([`lift_anchors_now`]).
    pub(super) fn lift_anchors_now(&self) -> &crate::plan::LiftAnchors {
        self.lift_anchors_now
            .get_or_init(|| lift_anchors_now(self.model, &self.set_index, &self.days))
    }

    /// The calisthenics ladder anchors over the whole history as of today, for
    /// the profile's lift goal (none without a profile).
    pub(super) fn calis_anchors_now(&self) -> &[crate::calisthenics::progression::CalisAnchor] {
        self.calis_anchors_now.get_or_init(|| {
            let m = self.model;
            let Some(p) = m.profile() else {
                return Vec::new();
            };
            build_calisthenics_anchors(
                self.set_index
                    .with_facts(&m.sets)
                    .map(|(s, facts)| (s, facts.calis_tier)),
                &m.calis_placements,
                &m.inputs,
                p.lift_goal,
                m.today_epoch_day,
                m.today_utc_offset_sec,
            )
        })
    }

    /// [`build_run_anchors`] as of local day `day`; today's is built once.
    pub(super) fn run_anchors_on(&self, day: i64) -> crate::plan::RunAnchors {
        let m = self.model;
        let build = || build_run_anchors(&self.runs, day, m.today_utc_offset_sec);
        if m.today_epoch_day == Some(day) {
            *self.run_anchors_today.get_or_init(build)
        } else {
            build()
        }
    }
}
