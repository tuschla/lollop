//! One bounded memo type for the per-thread view caches.
//!
//! Every cached value is a pure function of its key (content fingerprints of
//! immutable history, never a clock), so a hit and a miss yield the same
//! output and the cache is unobservable in the `ViewModel`. Growth is bounded
//! by clearing the whole map at the cap: access-order eviction would make the
//! cache's contents depend on call history.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::thread::LocalKey;

pub(super) struct Memo<K, V> {
    map: HashMap<K, V>,
    cap: usize,
}

impl<K: Eq + Hash, V: Clone> Memo<K, V> {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            cap,
        }
    }

    pub(super) fn get(&self, key: &K) -> Option<V> {
        self.map.get(key).cloned()
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        if self.map.len() >= self.cap {
            self.map.clear();
        }
        self.map.insert(key, value);
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.map.retain(|k, _| keep(k));
    }

    #[cfg(test)]
    pub(super) fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }
}

/// A single-entry memo for keys that are compared rather than hashed:
/// consecutive views share their inputs, so the last result is the only one
/// worth keeping.
pub(super) struct LastMemo<K, V> {
    slot: Option<(K, V)>,
}

impl<K, V> LastMemo<K, V> {
    pub(super) const fn new() -> Self {
        Self { slot: None }
    }

    /// The stored value when `hit` accepts the stored key.
    pub(super) fn get_if(&self, hit: impl FnOnce(&K) -> bool) -> Option<&V> {
        self.slot.as_ref().filter(|(k, _)| hit(k)).map(|(_, v)| v)
    }

    /// The stored value, to refresh a part of it that has its own key.
    pub(super) fn value_mut(&mut self) -> Option<&mut V> {
        self.slot.as_mut().map(|(_, v)| v)
    }

    pub(super) fn set(&mut self, key: K, value: V) {
        self.slot = Some((key, value));
    }
}

/// Serve `key` from the thread-local `memo`, computing it with `build` on a
/// miss. The borrow is released while `build` runs, so a builder may consult
/// other memos (or this one) without a re-entrant borrow panic.
pub(super) fn memoized<K: Eq + Hash, V: Clone>(
    memo: &'static LocalKey<RefCell<Memo<K, V>>>,
    key: K,
    build: impl FnOnce() -> V,
) -> V {
    if let Some(hit) = memo.with(|m| m.borrow().get(&key)) {
        return hit;
    }
    let value = build();
    memo.with(|m| m.borrow_mut().insert(key, value.clone()));
    value
}
