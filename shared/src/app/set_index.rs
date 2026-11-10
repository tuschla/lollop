//! What the logged sets' exercise names say about them, classified once per
//! distinct name and memoized across views on the set history's fingerprint.

use std::collections::HashMap;
use std::rc::Rc;

use crate::app::memo::LastMemo;
use crate::app::model::LoggedSet;

/// What a set's exercise NAME says about it, resolved once per distinct name.
#[derive(Clone, Copy, Debug)]
pub(super) struct NameFacts {
    /// Interned [`crate::exercise::movement_key`]: two names are
    /// [`crate::exercise::same_movement`] iff their ids are equal.
    pub(super) movement: u32,
    /// [`crate::exercise::is_isolation_lift`].
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
                    .entry(crate::exercise::movement_key(&s.exercise))
                    .or_insert(next);
                if movement == next {
                    idx.by_movement.push(Vec::new());
                }
                idx.names.push(NameFacts {
                    movement,
                    isolation: crate::exercise::is_isolation_lift(&s.exercise),
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
    /// movement as `name` ([`crate::exercise::same_movement`]); empty when none is
    /// logged.
    pub(super) fn same_movement_sets(&self, name: &str) -> &[usize] {
        self.movement_ids
            .get(&crate::exercise::movement_key(name))
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
pub(super) fn set_index(sets: &[LoggedSet], sets_fp: u64) -> Rc<SetIndex> {
    let hit = SET_INDEX_CACHE.with(|c| c.borrow().get_if(|k| *k == sets_fp).cloned());
    hit.unwrap_or_else(|| {
        #[cfg(test)]
        SET_INDEX_BUILDS.with(|c| c.set(c.get() + 1));
        let idx = Rc::new(SetIndex::new(sets));
        SET_INDEX_CACHE.with(|c| c.borrow_mut().set(sets_fp, idx.clone()));
        idx
    })
}
