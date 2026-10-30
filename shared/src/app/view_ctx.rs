//! Data every `view()` derives once and threads to the builders that read it,
//! so no builder re-walks the history for a fact another already computed.

use super::*;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Run-history anchors for plan synthesis ([`build_run_anchors`]); each field
/// feeds the [`crate::plan::Anchors`] field of the same name.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct RunAnchors {
    /// Longest run of the trailing [`LONGEST_RUN_WINDOW_DAYS`] (the
    /// RUN-SPIKE-001 window); an older run counts at its detraining credit.
    /// `None` when no run carries credit.
    pub(super) longest_recent_run_km: Option<f64>,
    /// The trailing-28-day km ÷ the 1–4 weeks that window spans since its
    /// first run ([`recent_weekly_km`]). `None` with no run in the window.
    pub(super) recent_weekly_km: Option<f64>,
    /// The longest run is older than the full-credit window, so the long run
    /// cites DETRAIN-001.
    pub(super) longest_run_detrained: bool,
}

/// What a set's exercise NAME says about it, resolved once per distinct name.
#[derive(Clone, Copy, Debug)]
pub(super) struct NameFacts {
    /// Interned [`crate::plan::movement_key`]: two names are
    /// [`crate::plan::same_movement`] iff their ids are equal.
    pub(super) movement: u32,
    /// [`crate::plan::is_isolation_lift`].
    pub(super) isolation: bool,
    /// [`crate::calisthenics::tier_by_name`]: the ladder rung the name logs.
    pub(super) calis_tier: Option<(crate::schema::LadderId, u8)>,
}

/// Name-derived facts of `model.sets`. A history repeats a handful of
/// exercise names thousands of times, so each distinct name is classified once
/// (not once per set per builder), and each movement keeps the positions of
/// its sets, so a per-movement lookup reads only that movement's sets.
pub(super) struct SetIndex {
    /// Per set, aligned to `model.sets`: its row in `names`.
    name_of: Vec<u32>,
    names: Vec<NameFacts>,
    /// Interned movement key → movement id.
    movement_ids: HashMap<Vec<String>, u32>,
    /// Per movement id: the positions of its sets in `model.sets`, ascending.
    by_movement: Vec<Vec<usize>>,
}

impl SetIndex {
    pub(super) fn new(sets: &[LoggedSet]) -> Self {
        let mut rows: HashMap<&str, u32> = HashMap::new();
        let mut idx = SetIndex {
            name_of: Vec::with_capacity(sets.len()),
            names: Vec::new(),
            movement_ids: HashMap::new(),
            by_movement: Vec::new(),
        };
        for (i, s) in sets.iter().enumerate() {
            let row = *rows.entry(s.exercise.as_str()).or_insert_with(|| {
                let next = idx.movement_ids.len() as u32;
                let movement = *idx
                    .movement_ids
                    .entry(crate::plan::movement_key(&s.exercise))
                    .or_insert(next);
                if movement == next {
                    idx.by_movement.push(Vec::new());
                }
                idx.names.push(NameFacts {
                    movement,
                    isolation: crate::plan::is_isolation_lift(&s.exercise),
                    calis_tier: crate::calisthenics::tier_by_name(&s.exercise),
                });
                (idx.names.len() - 1) as u32
            });
            idx.name_of.push(row);
            idx.by_movement[idx.names[row as usize].movement as usize].push(i);
        }
        idx
    }

    /// The facts of set `i`'s name.
    pub(super) fn facts(&self, i: usize) -> NameFacts {
        self.names[self.name_of[i] as usize]
    }

    /// Each of `sets` (the slice this index was built from) with its facts.
    pub(super) fn with_facts<'s>(
        &'s self,
        sets: &'s [LoggedSet],
    ) -> impl Iterator<Item = (&'s LoggedSet, NameFacts)> + 's {
        sets.iter().enumerate().map(|(i, s)| (s, self.facts(i)))
    }

    /// The positions in `model.sets` (ascending) of every set that is the same
    /// movement as `name` ([`crate::plan::same_movement`]); empty when none is
    /// logged.
    pub(super) fn same_movement_sets(&self, name: &str) -> &[usize] {
        self.movement_ids
            .get(&crate::plan::movement_key(name))
            .map_or(&[], |&m| &self.by_movement[m as usize])
    }

    /// The positions in `model.sets` (ascending) of movement `movement`'s sets.
    pub(super) fn movement_sets(&self, movement: u32) -> &[usize] {
        &self.by_movement[movement as usize]
    }
}

thread_local! {
    /// The [`SetIndex`] of the last set log seen, keyed by its
    /// [`sets_fingerprint`]: consecutive views share the set history, so only
    /// a log/amend/delete re-classifies it.
    static SET_INDEX_CACHE: std::cell::RefCell<LastMemo<u64, Rc<SetIndex>>> =
        const { std::cell::RefCell::new(LastMemo::new()) };
    /// Test-only: [`SetIndex`] builds by [`set_index`] (cache misses).
    #[cfg(test)]
    pub(super) static SET_INDEX_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The memoized [`SetIndex`] of `sets`, whose fingerprint is `sets_fp`.
fn set_index(sets: &[LoggedSet], sets_fp: u64) -> Rc<SetIndex> {
    let hit = SET_INDEX_CACHE.with(|c| c.borrow().get_if(|k| *k == sets_fp).cloned());
    hit.unwrap_or_else(|| {
        #[cfg(test)]
        SET_INDEX_BUILDS.with(|c| c.set(c.get() + 1));
        let idx = Rc::new(SetIndex::new(sets));
        SET_INDEX_CACHE.with(|c| c.borrow_mut().set(sets_fp, idx.clone()));
        idx
    })
}

pub(super) struct ViewCtx<'m> {
    pub(super) model: &'m Model,
    /// [`sets_fingerprint`] of `model.sets`.
    pub(super) sets_fp: u64,
    /// Name-derived facts of `model.sets`.
    pub(super) set_index: Rc<SetIndex>,
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
        let sets_fp = sets_fingerprint(&model.sets);
        let runs = RunFacts::new(&model.runs);
        let days = LoggedDays::new(model, &runs);
        Self {
            model,
            sets_fp,
            set_index: set_index(&model.sets, sets_fp),
            runs,
            days,
            anchors_now: OnceCell::new(),
            calis_anchors_now: OnceCell::new(),
            run_anchors_today: OnceCell::new(),
        }
    }

    /// The e1RM anchors per lift as of today ([`lift_anchors_now`]).
    pub(super) fn lift_anchors_now(&self) -> &crate::plan::Anchors {
        self.anchors_now
            .get_or_init(|| lift_anchors_now(self.model, &self.set_index))
    }

    /// The calisthenics ladder anchors over the whole history as of today, for
    /// the profile's lift goal (none without a profile).
    pub(super) fn calis_anchors_now(&self) -> &[crate::plan::CalisAnchor] {
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
    pub(super) fn run_anchors_on(&self, day: i64) -> RunAnchors {
        let m = self.model;
        let build = || build_run_anchors(&self.runs, day, m.today_utc_offset_sec);
        if m.today_epoch_day == Some(day) {
            *self.run_anchors_today.get_or_init(build)
        } else {
            build()
        }
    }
}
