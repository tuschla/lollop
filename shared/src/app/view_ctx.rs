//! Data every `view()` derives once and threads to the builders that read it,
//! so no builder re-walks the history for a fact another already computed.

use super::*;
use std::cell::OnceCell;

/// `(longest_recent_run_km, recent_weekly_km, longest_run_detrained)`, see
/// [`build_run_anchors`].
pub(super) type RunAnchors = (Option<f64>, Option<f64>, bool);

pub(super) struct ViewCtx<'m> {
    pub(super) model: &'m Model,
    /// [`sets_fingerprint`] of `model.sets`.
    pub(super) sets_fp: u64,
    /// Realised distance/time and QC'd track per run, aligned to `model.runs`.
    pub(super) runs: RunFacts,
    /// Session-day attribution of every set and run.
    pub(super) days: LoggedDays,
    anchors_now: OnceCell<crate::plan::Anchors>,
    calis_anchors_now: OnceCell<Vec<crate::plan::CalisAnchor>>,
    run_anchors_today: OnceCell<RunAnchors>,
}

impl<'m> ViewCtx<'m> {
    pub(super) fn new(model: &'m Model) -> Self {
        Self {
            model,
            sets_fp: sets_fingerprint(&model.sets),
            runs: RunFacts::new(&model.runs),
            days: LoggedDays::new(model),
            anchors_now: OnceCell::new(),
            calis_anchors_now: OnceCell::new(),
            run_anchors_today: OnceCell::new(),
        }
    }

    /// The e1RM anchors per lift as of today ([`lift_anchors_now`]).
    pub(super) fn lift_anchors_now(&self) -> &crate::plan::Anchors {
        self.anchors_now
            .get_or_init(|| lift_anchors_now(self.model))
    }

    /// The calisthenics ladder anchors over the whole history as of today, for
    /// the profile's lift goal (none without a profile).
    pub(super) fn calis_anchors_now(&self) -> &[crate::plan::CalisAnchor] {
        self.calis_anchors_now.get_or_init(|| {
            let m = self.model;
            let Some(p) = m.profile.as_ref() else {
                return Vec::new();
            };
            build_calisthenics_anchors(
                m.sets.iter(),
                &m.calis_placements,
                &m.inputs,
                p.lift_goal,
                m.today_epoch_day,
                m.today_utc_offset_sec,
            )
        })
    }

    /// [`build_run_anchors`] as of local day `day`; today's is built once.
    pub(super) fn run_anchors_on(&self, day: i64) -> RunAnchors {
        let m = self.model;
        let build = || build_run_anchors(&m.runs, &self.runs, day, m.today_utc_offset_sec);
        if m.today_epoch_day == Some(day) {
            *self.run_anchors_today.get_or_init(build)
        } else {
            build()
        }
    }
}
