//! The core's caches: one registry of per-thread memos, and the content
//! fingerprint that keys them.
//!
//! Every cached value is a pure function of its key (a content fingerprint of
//! the inputs, never a clock), so a hit and a miss yield the same output and no
//! cache is observable in the `ViewModel`. Each memo is bounded by its
//! [`MemoId`] cap: an insert into a full memo clears it first. Access-order
//! eviction would make the contents depend on call history; clearing keeps
//! them a function of the keys inserted since the last clear. The memos are
//! per thread: the bridge drives the core from one thread, and each test
//! thread keeps its own.

use std::any::{Any, TypeId};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use serde::ser::{self, Serialize};

/// Every memo the core keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoId {
    /// The finished lift list, per set-history fingerprint and display unit.
    LiftViews,
    /// The finished ordered run list, per fingerprint of the run keys.
    RunViews,
    /// One run's card, per run key.
    RunCard,
    /// One GPS run's QC'd track with its distance and moving time, per run key.
    RunQcTrack,
    /// One run's best effort per standard distance, per run key.
    RunBestEfforts,
    /// The name index of the set history, per set-history fingerprint.
    SetIndex,
    /// One view's plan snapshots (this week and next), per plan-input
    /// fingerprint.
    PlanSnapshots,
    /// The past-day readiness of the current plan week, per plan-input and
    /// check-in fingerprint.
    PastReadiness,
    /// The profile's guidance rows, per profile, weekly volume and hold.
    Guidance,
    /// What an exercise name says (movement key, isolation, movement
    /// pattern), per distinct name.
    ExerciseNames,
    /// The calisthenics rung an exercise name logs, per distinct name.
    CalisthenicsNames,
}

const MEMO_COUNT: usize = 11;

impl MemoId {
    pub(crate) const ALL: [MemoId; MEMO_COUNT] = [
        MemoId::LiftViews,
        MemoId::RunViews,
        MemoId::RunCard,
        MemoId::RunQcTrack,
        MemoId::RunBestEfforts,
        MemoId::SetIndex,
        MemoId::PlanSnapshots,
        MemoId::PastReadiness,
        MemoId::Guidance,
        MemoId::ExerciseNames,
        MemoId::CalisthenicsNames,
    ];

    /// The most entries the memo holds; an insert past it clears the memo.
    const fn cap(self) -> usize {
        match self {
            // Every view trims these to its live runs (`retain_runs`), so the
            // cap only bounds what accrues between views: a replayed event log
            // that logs and amends runs without viewing.
            MemoId::RunCard | MemoId::RunQcTrack | MemoId::RunBestEfforts => 4096,
            // Within a session only the current history and the one an edit
            // just superseded are live, and the lift list needs its kg AND lb
            // entries for an unchanged set log warm at once (the key carries
            // the unit). Each entry is a whole-history list, so the cap bounds
            // the worst-case memory directly.
            MemoId::LiftViews | MemoId::RunViews => 8,
            // Consecutive views share their inputs: the last result is the
            // only one worth keeping.
            MemoId::SetIndex | MemoId::PlanSnapshots | MemoId::PastReadiness | MemoId::Guidance => {
                1
            }
            // A history repeats a handful of names; plan synthesis adds the
            // catalog's.
            MemoId::ExerciseNames | MemoId::CalisthenicsNames => 1024,
        }
    }

    /// Keyed by a run's content fingerprint ([`fingerprint`] of the
    /// `LoggedRun`) and trimmed to the live runs by [`retain_runs`].
    pub(crate) const fn per_run(self) -> bool {
        matches!(
            self,
            MemoId::RunCard | MemoId::RunQcTrack | MemoId::RunBestEfforts
        )
    }
}

/// Lookups and builds (misses) of one memo on this thread, for laziness tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoStats {
    pub(crate) lookups: u64,
    pub(crate) builds: u64,
}

struct Memo<K, V> {
    map: HashMap<K, V>,
    cap: usize,
    #[cfg(test)]
    stats: MemoStats,
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    fn new(id: MemoId) -> Self {
        Self {
            map: HashMap::new(),
            cap: id.cap(),
            #[cfg(test)]
            stats: MemoStats::default(),
        }
    }

    fn get<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        #[cfg(test)]
        {
            self.stats.lookups += 1;
        }
        self.map.get(key).cloned()
    }

    fn insert(&mut self, key: K, value: V) {
        #[cfg(test)]
        {
            self.stats.builds += 1;
        }
        if self.map.len() >= self.cap {
            self.map.clear();
        }
        self.map.insert(key, value);
    }
}

/// A registered memo with its key and value types erased.
trait Slot: Any {
    /// Drop every entry whose run key is not in `live`.
    fn retain_runs(&mut self, live: &HashSet<u64>);
    #[cfg(test)]
    fn stats(&self) -> MemoStats;
    #[cfg(test)]
    fn holds_run(&self, key: u64) -> bool;
}

impl<K: Any + Eq + Hash, V: Any> Slot for Memo<K, V> {
    fn retain_runs(&mut self, live: &HashSet<u64>) {
        self.map.retain(|k, _| {
            (k as &dyn Any)
                .downcast_ref::<u64>()
                .is_some_and(|k| live.contains(k))
        });
    }

    #[cfg(test)]
    fn stats(&self) -> MemoStats {
        self.stats
    }

    #[cfg(test)]
    fn holds_run(&self, key: u64) -> bool {
        self.map
            .keys()
            .any(|k| (k as &dyn Any).downcast_ref() == Some(&key))
    }
}

thread_local! {
    static MEMOS: RefCell<[Option<Box<dyn Slot>>; MEMO_COUNT]> =
        const { RefCell::new([const { None }; MEMO_COUNT]) };
}

/// Run `f` on memo `id`, created on first use. Each `MemoId` is served with
/// one key and value type; a per-run memo is keyed by `u64`.
fn with_memo<K, V, R>(id: MemoId, f: impl FnOnce(&mut Memo<K, V>) -> R) -> R
where
    K: Any + Eq + Hash,
    V: Any + Clone,
{
    MEMOS.with(|memos| {
        let mut memos = memos.borrow_mut();
        let slot = memos[id as usize].get_or_insert_with(|| {
            assert!(
                !id.per_run() || TypeId::of::<K>() == TypeId::of::<u64>(),
                "{id:?} is keyed by a run's u64 fingerprint"
            );
            Box::new(Memo::<K, V>::new(id))
        });
        let slot: &mut dyn Any = &mut **slot;
        match slot.downcast_mut::<Memo<K, V>>() {
            Some(memo) => f(memo),
            None => panic!("{id:?} is served with one key and value type"),
        }
    })
}

/// Serve `key` from memo `id`, computing it with `build` on a miss. The
/// registry is not borrowed while `build` runs, so a builder may consult other
/// memos (or this one).
pub(crate) fn memoized<K, V, Q>(id: MemoId, key: &Q, build: impl FnOnce() -> V) -> V
where
    K: Any + Eq + Hash + Borrow<Q>,
    Q: Eq + Hash + ToOwned<Owned = K> + ?Sized,
    V: Any + Clone,
{
    if let Some(hit) = with_memo(id, |m: &mut Memo<K, V>| m.get(key)) {
        return hit;
    }
    let value = build();
    with_memo(id, |m: &mut Memo<K, V>| {
        m.insert(key.to_owned(), value.clone())
    });
    value
}

/// Trim every per-run memo to the runs whose content keys are in `live`, so a
/// deleted or amended-away run's entries do not outlive the view that no
/// longer shows it. Output is identical whether or not it runs.
pub(crate) fn retain_runs(live: &[u64]) {
    let live: HashSet<u64> = live.iter().copied().collect();
    MEMOS.with(|memos| {
        let mut memos = memos.borrow_mut();
        for id in MemoId::ALL.into_iter().filter(|id| id.per_run()) {
            if let Some(slot) = memos[id as usize].as_mut() {
                slot.retain_runs(&live);
            }
        }
    });
}

/// This thread's lookups and builds of memo `id` so far.
#[cfg(test)]
pub(crate) fn stats(id: MemoId) -> MemoStats {
    MEMOS
        .with(|memos| memos.borrow()[id as usize].as_ref().map(|s| s.stats()))
        .unwrap_or_default()
}

/// Whether per-run memo `id` holds an entry for run key `key` on this thread.
#[cfg(test)]
pub(crate) fn holds_run(id: MemoId, key: u64) -> bool {
    MEMOS.with(|memos| {
        memos.borrow()[id as usize]
            .as_ref()
            .is_some_and(|s| s.holds_run(key))
    })
}

/// A 64-bit content hash of `value`, derived from its `Serialize` impl: every
/// serialized field takes part, so a field added to a type is in its
/// fingerprint with no edit here. Floats hash their bit patterns; a skipped
/// field hashes a marker. Map entries hash in iteration order, so a key type
/// holding a `HashMap` would miss more often, never serve a wrong value.
/// Collision probability is ~n²/2⁶⁴ over n distinct keys, negligible for a
/// personal history; a collision would serve another input's cached value.
pub(crate) fn fingerprint<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut fp = Fingerprinter(DefaultHasher::new());
    match value.serialize(&mut fp) {
        Ok(()) | Err(std::fmt::Error) => fp.0.finish(),
    }
}

/// A serde `Serializer` that feeds a value's content into a hasher.
struct Fingerprinter(DefaultHasher);

type FpResult = Result<(), std::fmt::Error>;

impl ser::Serializer for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_i128(self, v: i128) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_u128(self, v: u128) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> FpResult {
        v.to_bits().hash(&mut self.0);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> FpResult {
        v.to_bits().hash(&mut self.0);
        Ok(())
    }
    fn serialize_char(self, v: char) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_str(self, v: &str) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> FpResult {
        v.hash(&mut self.0);
        Ok(())
    }
    fn serialize_none(self) -> FpResult {
        self.0.write_u8(0);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> FpResult {
        self.0.write_u8(1);
        value.serialize(self)
    }
    fn serialize_unit(self) -> FpResult {
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> FpResult {
        Ok(())
    }
    fn serialize_unit_variant(self, _name: &'static str, index: u32, _v: &'static str) -> FpResult {
        index.hash(&mut self.0);
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> FpResult {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        value: &T,
    ) -> FpResult {
        index.hash(&mut self.0);
        value.serialize(self)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self, std::fmt::Error> {
        len.hash(&mut self.0);
        Ok(self)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, std::fmt::Error> {
        index.hash(&mut self.0);
        Ok(self)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self, std::fmt::Error> {
        len.hash(&mut self.0);
        Ok(self)
    }
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, std::fmt::Error> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, std::fmt::Error> {
        index.hash(&mut self.0);
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> FpResult {
        value.serialize(&mut **self)
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> FpResult {
        value.serialize(&mut **self)
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> FpResult {
        value.serialize(&mut **self)
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> FpResult {
        value.serialize(&mut **self)
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeMap for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> FpResult {
        key.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> FpResult {
        value.serialize(&mut **self)
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> FpResult {
        value.serialize(&mut **self)
    }
    fn skip_field(&mut self, _key: &'static str) -> FpResult {
        self.0.write_u8(0xfe);
        Ok(())
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Fingerprinter {
    type Ok = ();
    type Error = std::fmt::Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> FpResult {
        value.serialize(&mut **self)
    }
    fn skip_field(&mut self, _key: &'static str) -> FpResult {
        self.0.write_u8(0xfe);
        Ok(())
    }
    fn end(self) -> FpResult {
        Ok(())
    }
}
