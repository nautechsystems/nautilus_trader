// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Abstraction layer over common hash-based containers.

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Display},
    hash::Hash,
    sync::Arc,
};

use ahash::{AHashMap, AHashSet};
use arc_swap::ArcSwap;
use ustr::Ustr;

/// A lock-free concurrent map optimized for read-heavy access patterns.
///
/// Reads are a single atomic pointer load with no contention between readers.
/// Writes clone the inner map, mutate the clone, and atomically swap it in.
///
/// Not safe for concurrent writers using `load`/`store`: the last `store` wins
/// and earlier updates are silently lost. Use [`rcu`](Self::rcu) when multiple
/// writers may race, or restrict writes to a single task.
///
/// Wrap in `Arc` for shared ownership across threads.
pub struct AtomicMap<K, V>(ArcSwap<AHashMap<K, V>>);

impl<K, V> AtomicMap<K, V> {
    /// Creates a new empty atomic map.
    #[must_use]
    pub fn new() -> Self {
        Self(ArcSwap::new(Arc::new(AHashMap::new())))
    }

    /// Returns a snapshot guard for direct access to the inner map.
    ///
    /// The guard dereferences to `AHashMap<K, V>`. Use for operations that
    /// need a reference into the map (e.g., `load().get(&key)`).
    #[inline]
    pub fn load(&self) -> arc_swap::Guard<Arc<AHashMap<K, V>>> {
        self.0.load()
    }

    /// Atomically replaces the inner map.
    pub fn store(&self, map: AHashMap<K, V>) {
        self.0.store(Arc::new(map));
    }
}

impl<K, V> AtomicMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Atomically applies `f` to a clone of the inner map.
    ///
    /// Retries if another writer swapped the map between the clone and the
    /// compare-and-swap, so `f` may run more than once.
    pub fn rcu<F>(&self, mut f: F)
    where
        F: FnMut(&mut AHashMap<K, V>),
    {
        self.0.rcu(|m| {
            let mut m = (**m).clone();
            f(&mut m);
            m
        });
    }

    /// Returns `true` if the map contains the given key.
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        self.0.load().contains_key(key)
    }

    /// Returns a clone of the value for the given key, if present.
    #[inline]
    pub fn get_cloned(&self, key: &K) -> Option<V> {
        self.0.load().get(key).cloned()
    }

    /// Inserts a key-value pair (clone-and-swap).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "by-value matches HashMap::insert; clone needed because rcu may retry"
    )]
    pub fn insert(&self, key: K, value: V) {
        self.rcu(|m| {
            m.insert(key.clone(), value.clone());
        });
    }

    /// Removes a key (clone-and-swap).
    pub fn remove(&self, key: &K) {
        self.rcu(|m| {
            m.remove(key);
        });
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.load().len()
    }

    /// Returns `true` if the map is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.load().is_empty()
    }
}

impl<K, V> Default for AtomicMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Debug + Eq + Hash, V: Debug> Debug for AtomicMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.0.load().iter()).finish()
    }
}

impl<K: Eq + Hash, V> From<AHashMap<K, V>> for AtomicMap<K, V> {
    fn from(map: AHashMap<K, V>) -> Self {
        Self(ArcSwap::new(Arc::new(map)))
    }
}

/// A lock-free concurrent set optimized for read-heavy access patterns.
///
/// Reads are a single atomic pointer load with no contention between readers.
/// Writes clone the inner set, mutate the clone, and atomically swap it in.
///
/// Not safe for concurrent writers using `load`/`store`: the last `store` wins
/// and earlier updates are silently lost. Use [`rcu`](Self::rcu) when multiple
/// writers may race, or restrict writes to a single task.
///
/// Wrap in `Arc` for shared ownership across threads.
pub struct AtomicSet<K>(ArcSwap<AHashSet<K>>);

impl<K> AtomicSet<K> {
    /// Creates a new empty atomic set.
    #[must_use]
    pub fn new() -> Self {
        Self(ArcSwap::new(Arc::new(AHashSet::new())))
    }

    /// Returns a snapshot guard for direct access to the inner set.
    ///
    /// The guard dereferences to `AHashSet<K>`. Use for operations that
    /// need iteration or reference access.
    #[inline]
    pub fn load(&self) -> arc_swap::Guard<Arc<AHashSet<K>>> {
        self.0.load()
    }

    /// Atomically replaces the inner set.
    pub fn store(&self, set: AHashSet<K>) {
        self.0.store(Arc::new(set));
    }
}

impl<K> AtomicSet<K>
where
    K: Eq + Hash + Clone,
{
    /// Atomically applies `f` to a clone of the inner set.
    ///
    /// Retries if another writer swapped the set between the clone and the
    /// compare-and-swap, so `f` may run more than once.
    pub fn rcu<F>(&self, mut f: F)
    where
        F: FnMut(&mut AHashSet<K>),
    {
        self.0.rcu(|s| {
            let mut s = (**s).clone();
            f(&mut s);
            s
        });
    }

    /// Returns `true` if the set contains the given key.
    #[inline]
    pub fn contains(&self, key: &K) -> bool {
        self.0.load().contains(key)
    }

    /// Inserts a key (clone-and-swap).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "by-value matches HashSet::insert; clone needed because rcu may retry"
    )]
    pub fn insert(&self, key: K) {
        self.rcu(|s| {
            s.insert(key.clone());
        });
    }

    /// Removes a key (clone-and-swap).
    pub fn remove(&self, key: &K) {
        self.rcu(|s| {
            s.remove(key);
        });
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.load().len()
    }

    /// Returns `true` if the set is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.load().is_empty()
    }
}

impl<K> Default for AtomicSet<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Debug + Eq + Hash> Debug for AtomicSet<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.load().iter()).finish()
    }
}

impl<K: Eq + Hash> From<AHashSet<K>> for AtomicSet<K> {
    fn from(set: AHashSet<K>) -> Self {
        Self(ArcSwap::new(Arc::new(set)))
    }
}

/// Represents a generic set-like container with members.
pub trait SetLike {
    /// The type of items stored in the set.
    type Item: Hash + Eq + Display + Clone;

    /// Returns `true` if the set contains the specified item.
    fn contains(&self, item: &Self::Item) -> bool;
    /// Returns `true` if the set is empty.
    fn is_empty(&self) -> bool;
}

impl<T, S> SetLike for HashSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        Self::contains(self, v)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }
}

impl<T, S> SetLike for indexmap::IndexSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        Self::contains(self, v)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Self::is_empty(self)
    }
}

impl<T, S> SetLike for ahash::AHashSet<T, S>
where
    T: Eq + Hash + Display + Clone,
    S: std::hash::BuildHasher,
{
    type Item = T;

    #[inline]
    fn contains(&self, v: &T) -> bool {
        self.get(v).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Represents a generic map-like container with key-value pairs.
pub trait MapLike {
    /// The type of keys stored in the map.
    type Key: Hash + Eq + Display + Clone;
    /// The type of values stored in the map.
    type Value: Debug;

    /// Returns `true` if the map contains the specified key.
    fn contains_key(&self, key: &Self::Key) -> bool;
    /// Returns `true` if the map is empty.
    fn is_empty(&self) -> bool;
}

impl<K, V, S> MapLike for HashMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.contains_key(k)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V, S> MapLike for indexmap::IndexMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.get(k).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

impl<K, V, S> MapLike for ahash::AHashMap<K, V, S>
where
    K: Eq + Hash + Display + Clone,
    V: Debug,
    S: std::hash::BuildHasher,
{
    type Key = K;
    type Value = V;

    #[inline]
    fn contains_key(&self, k: &K) -> bool {
        self.get(k).is_some()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Convert any iterator of string-like items into a `Vec<Ustr>`.
#[must_use]
pub fn into_ustr_vec<I, T>(iter: I) -> Vec<Ustr>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let iter = iter.into_iter();
    let (lower, _) = iter.size_hint();
    let mut result = Vec::with_capacity(lower);

    for item in iter {
        result.push(Ustr::from(item.as_ref()));
    }

    result
}

#[cfg(test)]
#[expect(
    clippy::unnecessary_to_owned,
    reason = "Required for trait bound satisfaction"
)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        sync::{Arc, Barrier},
    };

    use ahash::{AHashMap, AHashSet};
    use indexmap::{IndexMap, IndexSet};
    use rstest::*;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_atomic_set_new_is_empty() {
        let set: AtomicSet<String> = AtomicSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[rstest]
    fn test_atomic_set_default_is_empty() {
        let set: AtomicSet<u64> = AtomicSet::default();
        assert!(set.is_empty());
    }

    #[rstest]
    fn test_atomic_set_insert_and_contains() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(2);

        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(!set.contains(&3));
        assert_eq!(set.len(), 2);
    }

    #[rstest]
    fn test_atomic_set_insert_duplicate() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(1);

        assert_eq!(set.len(), 1);
        assert!(set.contains(&1));
    }

    #[rstest]
    fn test_atomic_set_remove() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(2);
        set.remove(&1);

        assert!(!set.contains(&1));
        assert!(set.contains(&2));
        assert_eq!(set.len(), 1);
    }

    #[rstest]
    fn test_atomic_set_remove_nonexistent() {
        let set: AtomicSet<i32> = AtomicSet::new();
        set.insert(1);
        set.remove(&999);

        assert_eq!(set.len(), 1);
        assert!(set.contains(&1));
    }

    #[rstest]
    fn test_atomic_set_store_replaces_contents() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(2);

        let mut replacement = AHashSet::new();
        replacement.insert(10);
        replacement.insert(20);
        set.store(replacement);

        assert!(!set.contains(&1));
        assert!(!set.contains(&2));
        assert!(set.contains(&10));
        assert!(set.contains(&20));
        assert_eq!(set.len(), 2);
    }

    #[rstest]
    fn test_atomic_set_store_empty_clears() {
        let set = AtomicSet::new();
        set.insert(1);
        set.store(AHashSet::new());

        assert!(set.is_empty());
    }

    #[rstest]
    fn test_atomic_set_rcu_batch_insert() {
        let set = AtomicSet::new();
        set.rcu(|s| {
            s.insert(1);
            s.insert(2);
            s.insert(3);
        });

        assert_eq!(set.len(), 3);
        assert!(set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
    }

    #[rstest]
    fn test_atomic_set_rcu_mixed_operations() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(2);

        set.rcu(|s| {
            s.remove(&1);
            s.insert(3);
        });

        assert!(!set.contains(&1));
        assert!(set.contains(&2));
        assert!(set.contains(&3));
    }

    #[rstest]
    fn test_atomic_set_load_returns_snapshot() {
        let set = AtomicSet::new();
        set.insert(1);

        let snapshot = set.load();
        assert!(snapshot.contains(&1));
        assert_eq!(snapshot.len(), 1);
    }

    #[rstest]
    fn test_atomic_set_load_snapshot_not_affected_by_later_writes() {
        let set = AtomicSet::new();
        set.insert(1);

        let snapshot = set.load();
        set.insert(2);

        assert!(!snapshot.contains(&2));
        assert!(set.contains(&2));
    }

    #[rstest]
    fn test_atomic_set_from_ahashset() {
        let mut source = AHashSet::new();
        source.insert("a".to_string());
        source.insert("b".to_string());

        let set = AtomicSet::from(source);

        assert_eq!(set.len(), 2);
        assert!(set.contains(&"a".to_string()));
        assert!(set.contains(&"b".to_string()));
    }

    #[rstest]
    fn test_atomic_set_debug() {
        let set = AtomicSet::new();
        set.insert(42);

        let debug_str = format!("{set:?}");
        assert!(debug_str.contains("42"));
    }

    #[rstest]
    fn test_atomic_set_debug_empty() {
        let set: AtomicSet<i32> = AtomicSet::new();
        let debug_str = format!("{set:?}");
        assert_eq!(debug_str, "{}");
    }

    #[rstest]
    fn test_atomic_set_load_iteration() {
        let set = AtomicSet::new();
        set.insert(1);
        set.insert(2);
        set.insert(3);

        let guard = set.load();
        let mut values: Vec<_> = guard.iter().copied().collect();
        values.sort_unstable();

        assert_eq!(values, vec![1, 2, 3]);
    }

    #[rstest]
    fn test_atomic_set_concurrent_reads() {
        let set = Arc::new(AtomicSet::new());
        for i in 0..100 {
            set.insert(i);
        }

        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let set = Arc::clone(&set);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for i in 0..100 {
                        assert!(set.contains(&i));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[rstest]
    fn test_atomic_set_concurrent_rcu_writes() {
        let set = Arc::new(AtomicSet::new());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4u32)
            .map(|t| {
                let set = Arc::clone(&set);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for i in 0..25 {
                        set.insert(t * 25 + i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(set.len(), 100);
        for i in 0..100 {
            assert!(set.contains(&i), "missing {i}");
        }
    }

    #[rstest]
    fn test_atomic_set_concurrent_read_write() {
        let set = Arc::new(AtomicSet::new());
        for i in 0u32..100 {
            set.insert(i);
        }

        let barrier = Arc::new(Barrier::new(5));

        let writer = {
            let set = Arc::clone(&set);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();

                for i in 100u32..200 {
                    set.insert(i);
                }
            })
        };

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let set = Arc::clone(&set);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for _ in 0..1000 {
                        let snapshot = set.load();
                        let len = snapshot.len();
                        assert!(
                            (100..=200).contains(&len),
                            "snapshot len {len} outside expected range"
                        );

                        for i in 0u32..100 {
                            assert!(snapshot.contains(&i), "original key {i} missing");
                        }
                    }
                })
            })
            .collect();

        writer.join().unwrap();
        for r in readers {
            r.join().unwrap();
        }

        assert_eq!(set.len(), 200);
    }

    #[rstest]
    fn test_atomic_set_snapshot_consistency_under_store() {
        let set = Arc::new(AtomicSet::new());
        let barrier = Arc::new(Barrier::new(5));

        let writer = {
            let set = Arc::clone(&set);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();

                for batch in 0u32..50 {
                    let start = batch * 10;
                    let new_set: AHashSet<u32> = (start..start + 10).collect();
                    set.store(new_set);
                }
            })
        };

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let set = Arc::clone(&set);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for _ in 0..5000 {
                        let snapshot = set.load();
                        let items: Vec<u32> = snapshot.iter().copied().collect();
                        if items.is_empty() {
                            continue;
                        }
                        assert_eq!(
                            items.len(),
                            10,
                            "partial snapshot: got {} items: {items:?}",
                            items.len()
                        );
                        let min = *items.iter().min().unwrap();
                        let max = *items.iter().max().unwrap();
                        assert_eq!(
                            max - min,
                            9,
                            "snapshot not from single batch: min={min} max={max}"
                        );
                    }
                })
            })
            .collect();

        writer.join().unwrap();
        for r in readers {
            r.join().unwrap();
        }
    }

    #[rstest]
    fn test_atomic_map_new_is_empty() {
        let map: AtomicMap<String, i32> = AtomicMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[rstest]
    fn test_atomic_map_default_is_empty() {
        let map: AtomicMap<u32, u32> = AtomicMap::default();
        assert!(map.is_empty());
    }

    #[rstest]
    fn test_atomic_map_insert_and_get_cloned() {
        let map = AtomicMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);

        assert_eq!(map.get_cloned(&"a".to_string()), Some(1));
        assert_eq!(map.get_cloned(&"b".to_string()), Some(2));
        assert_eq!(map.get_cloned(&"c".to_string()), None);
        assert_eq!(map.len(), 2);
    }

    #[rstest]
    fn test_atomic_map_insert_overwrites() {
        let map = AtomicMap::new();
        map.insert("key".to_string(), 1);
        map.insert("key".to_string(), 2);

        assert_eq!(map.get_cloned(&"key".to_string()), Some(2));
        assert_eq!(map.len(), 1);
    }

    #[rstest]
    fn test_atomic_map_contains_key() {
        let map = AtomicMap::new();
        map.insert("present".to_string(), 42);

        assert!(map.contains_key(&"present".to_string()));
        assert!(!map.contains_key(&"absent".to_string()));
    }

    #[rstest]
    fn test_atomic_map_remove() {
        let map = AtomicMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);
        map.remove(&"a".to_string());

        assert!(!map.contains_key(&"a".to_string()));
        assert!(map.contains_key(&"b".to_string()));
        assert_eq!(map.len(), 1);
    }

    #[rstest]
    fn test_atomic_map_remove_nonexistent() {
        let map = AtomicMap::new();
        map.insert("a".to_string(), 1);
        map.remove(&"z".to_string());

        assert_eq!(map.len(), 1);
    }

    #[rstest]
    fn test_atomic_map_store_replaces_contents() {
        let map = AtomicMap::new();
        map.insert("old".to_string(), 1);

        let mut replacement = AHashMap::new();
        replacement.insert("new".to_string(), 99);
        map.store(replacement);

        assert!(!map.contains_key(&"old".to_string()));
        assert_eq!(map.get_cloned(&"new".to_string()), Some(99));
    }

    #[rstest]
    fn test_atomic_map_store_empty_clears() {
        let map = AtomicMap::new();
        map.insert("key".to_string(), 1);
        map.store(AHashMap::new());

        assert!(map.is_empty());
    }

    #[rstest]
    fn test_atomic_map_rcu_batch_insert() {
        let map = AtomicMap::new();
        let entries: Vec<(String, i32)> = (0..5).map(|i| (format!("k{i}"), i)).collect();

        map.rcu(|m| {
            for (k, v) in &entries {
                m.insert(k.clone(), *v);
            }
        });

        assert_eq!(map.len(), 5);
        for i in 0..5 {
            assert_eq!(map.get_cloned(&format!("k{i}")), Some(i));
        }
    }

    #[rstest]
    fn test_atomic_map_rcu_mixed_operations() {
        let map = AtomicMap::new();
        map.insert("a".to_string(), 1);
        map.insert("b".to_string(), 2);

        map.rcu(|m| {
            m.remove(&"a".to_string());
            m.insert("c".to_string(), 3);
            if let Some(v) = m.get_mut(&"b".to_string()) {
                *v = 20;
            }
        });

        assert_eq!(map.get_cloned(&"a".to_string()), None);
        assert_eq!(map.get_cloned(&"b".to_string()), Some(20));
        assert_eq!(map.get_cloned(&"c".to_string()), Some(3));
    }

    #[rstest]
    fn test_atomic_map_load_returns_snapshot() {
        let map = AtomicMap::new();
        map.insert("key".to_string(), 42);

        let snapshot = map.load();
        assert_eq!(snapshot.get(&"key".to_string()), Some(&42));
    }

    #[rstest]
    fn test_atomic_map_load_snapshot_not_affected_by_later_writes() {
        let map = AtomicMap::new();
        map.insert("a".to_string(), 1);

        let snapshot = map.load();
        map.insert("b".to_string(), 2);

        assert!(snapshot.get(&"b".to_string()).is_none());
        assert_eq!(map.get_cloned(&"b".to_string()), Some(2));
    }

    #[rstest]
    fn test_atomic_map_from_ahashmap() {
        let mut source = AHashMap::new();
        source.insert(1, "one".to_string());
        source.insert(2, "two".to_string());

        let map = AtomicMap::from(source);

        assert_eq!(map.len(), 2);
        assert_eq!(map.get_cloned(&1), Some("one".to_string()));
    }

    #[rstest]
    fn test_atomic_map_debug() {
        let map = AtomicMap::new();
        map.insert("key".to_string(), 42);

        let debug_str = format!("{map:?}");
        assert!(debug_str.contains("key"));
        assert!(debug_str.contains("42"));
    }

    #[rstest]
    fn test_atomic_map_debug_empty() {
        let map: AtomicMap<String, i32> = AtomicMap::new();
        let debug_str = format!("{map:?}");
        assert_eq!(debug_str, "{}");
    }

    #[rstest]
    fn test_atomic_map_load_iteration() {
        let map = AtomicMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);

        let guard = map.load();
        let mut pairs: Vec<_> = guard.iter().map(|(k, v)| (*k, *v)).collect();
        pairs.sort_unstable();

        assert_eq!(pairs, vec![(1, 10), (2, 20), (3, 30)]);
    }

    #[rstest]
    fn test_atomic_map_concurrent_reads() {
        let map = Arc::new(AtomicMap::new());
        for i in 0u32..100 {
            map.insert(i, i * 10);
        }

        let barrier = Arc::new(Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let map = Arc::clone(&map);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for i in 0u32..100 {
                        assert_eq!(map.get_cloned(&i), Some(i * 10));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[rstest]
    fn test_atomic_map_concurrent_rcu_writes() {
        let map = Arc::new(AtomicMap::new());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4u32)
            .map(|t| {
                let map = Arc::clone(&map);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for i in 0..25 {
                        let key = t * 25 + i;
                        map.insert(key, key * 10);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(map.len(), 100);
        for i in 0u32..100 {
            assert_eq!(map.get_cloned(&i), Some(i * 10), "wrong value for {i}");
        }
    }

    #[rstest]
    fn test_atomic_map_concurrent_read_write() {
        let map = Arc::new(AtomicMap::new());
        for i in 0u32..100 {
            map.insert(i, i);
        }

        let barrier = Arc::new(Barrier::new(5));

        let writer = {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();

                for i in 100u32..200 {
                    map.insert(i, i);
                }
            })
        };

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let map = Arc::clone(&map);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for _ in 0..1000 {
                        let snapshot = map.load();
                        let len = snapshot.len();
                        assert!(
                            (100..=200).contains(&len),
                            "snapshot len {len} outside expected range"
                        );

                        for i in 0u32..100 {
                            assert_eq!(
                                snapshot.get(&i).copied(),
                                Some(i),
                                "original key {i} missing or wrong"
                            );
                        }
                    }
                })
            })
            .collect();

        writer.join().unwrap();
        for r in readers {
            r.join().unwrap();
        }

        assert_eq!(map.len(), 200);
    }

    #[rstest]
    fn test_atomic_map_snapshot_consistency_under_store() {
        let map = Arc::new(AtomicMap::new());
        let barrier = Arc::new(Barrier::new(5));

        let writer = {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();

                for batch in 0u32..50 {
                    let start = batch * 10;
                    let new_map: AHashMap<u32, u32> =
                        (start..start + 10).map(|i| (i, batch)).collect();
                    map.store(new_map);
                }
            })
        };

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let map = Arc::clone(&map);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();

                    for _ in 0..5000 {
                        let snapshot = map.load();
                        if snapshot.is_empty() {
                            continue;
                        }
                        let values: AHashSet<u32> = snapshot.values().copied().collect();
                        assert_eq!(
                            values.len(),
                            1,
                            "snapshot has mixed batch values: {values:?}"
                        );
                        assert_eq!(snapshot.len(), 10, "partial snapshot");
                    }
                })
            })
            .collect();

        writer.join().unwrap();
        for r in readers {
            r.join().unwrap();
        }
    }

    mod proptests {
        use proptest::prelude::*;
        use rstest::rstest;

        use super::*;

        #[derive(Debug, Clone)]
        enum SetOp {
            Insert(u16),
            Remove(u16),
            Contains(u16),
            Len,
            IsEmpty,
        }

        fn set_op_strategy() -> impl Strategy<Value = SetOp> {
            prop_oneof![
                3 => any::<u16>().prop_map(SetOp::Insert),
                3 => any::<u16>().prop_map(SetOp::Remove),
                3 => any::<u16>().prop_map(SetOp::Contains),
                1 => Just(SetOp::Len),
                1 => Just(SetOp::IsEmpty),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig {
                failure_persistence: Some(Box::new(
                    proptest::test_runner::FileFailurePersistence::WithSource("atomic_set")
                )),
                cases: 500,
                ..ProptestConfig::default()
            })]

            /// AtomicSet matches AHashSet behavior for any sequence of ops.
            #[rstest]
            fn atomic_set_matches_ahashset(ops in proptest::collection::vec(set_op_strategy(), 0..200)) {
                let atomic = AtomicSet::new();
                let mut reference = AHashSet::new();

                for op in &ops {
                    match op {
                        SetOp::Insert(k) => {
                            atomic.insert(*k);
                            reference.insert(*k);
                        }
                        SetOp::Remove(k) => {
                            atomic.remove(k);
                            reference.remove(k);
                        }
                        SetOp::Contains(k) => {
                            prop_assert_eq!(
                                atomic.contains(k),
                                reference.contains(k),
                                "contains mismatch for key {}", k
                            );
                        }
                        SetOp::Len => {
                            prop_assert_eq!(atomic.len(), reference.len());
                        }
                        SetOp::IsEmpty => {
                            prop_assert_eq!(atomic.is_empty(), reference.is_empty());
                        }
                    }
                }

                prop_assert_eq!(atomic.len(), reference.len());
                prop_assert_eq!(atomic.is_empty(), reference.is_empty());

                for k in &reference {
                    prop_assert!(atomic.contains(k), "atomic missing key {}", k);
                }
            }

            /// store() followed by reads yields exactly the stored contents.
            #[rstest]
            fn atomic_set_store_snapshot(items in proptest::collection::vec(any::<u16>(), 0..100)) {
                let set = AtomicSet::new();
                set.insert(9999);

                let expected: AHashSet<u16> = items.iter().copied().collect();
                set.store(expected.clone());

                prop_assert_eq!(set.len(), expected.len());
                prop_assert!(!set.contains(&9999) || expected.contains(&9999));

                for k in &expected {
                    prop_assert!(set.contains(k));
                }
            }

            /// rcu batch mutation matches sequential application.
            #[rstest]
            fn atomic_set_rcu_batch(
                initial in proptest::collection::vec(any::<u16>(), 0..50),
                to_add in proptest::collection::vec(any::<u16>(), 0..50),
                to_remove in proptest::collection::vec(any::<u16>(), 0..20),
            ) {
                let set = AtomicSet::new();
                let mut reference = AHashSet::new();

                for k in &initial {
                    set.insert(*k);
                    reference.insert(*k);
                }

                let to_add_clone = to_add.clone();
                let to_remove_clone = to_remove.clone();
                set.rcu(|s| {
                    for k in &to_add_clone {
                        s.insert(*k);
                    }

                    for k in &to_remove_clone {
                        s.remove(k);
                    }
                });

                for k in &to_add {
                    reference.insert(*k);
                }

                for k in &to_remove {
                    reference.remove(k);
                }

                prop_assert_eq!(set.len(), reference.len());
                for k in &reference {
                    prop_assert!(set.contains(k));
                }
            }

            /// load() returns a frozen snapshot unaffected by subsequent writes.
            #[rstest]
            fn atomic_set_snapshot_isolation(
                initial in proptest::collection::vec(any::<u16>(), 1..50),
                extra in proptest::collection::vec(any::<u16>(), 1..50),
            ) {
                let set = AtomicSet::new();
                for k in &initial {
                    set.insert(*k);
                }

                let snapshot = set.load();
                let snapshot_contents: AHashSet<u16> = snapshot.iter().copied().collect();

                for k in &extra {
                    set.insert(*k);
                }

                let snapshot_after: AHashSet<u16> = snapshot.iter().copied().collect();
                prop_assert_eq!(snapshot_contents, snapshot_after, "snapshot mutated after write");
            }

            /// From<AHashSet> roundtrip: every element in the source is present.
            #[rstest]
            fn atomic_set_from_roundtrip(items in proptest::collection::vec(any::<u16>(), 0..100)) {
                let expected: AHashSet<u16> = items.iter().copied().collect();
                let set = AtomicSet::from(expected.clone());

                prop_assert_eq!(set.len(), expected.len());
                for k in &expected {
                    prop_assert!(set.contains(k));
                }
            }
        }

        #[derive(Debug, Clone)]
        enum MapOp {
            Insert(u16, u32),
            Remove(u16),
            GetCloned(u16),
            ContainsKey(u16),
            Len,
            IsEmpty,
            LoadGet(u16),
        }

        fn map_op_strategy() -> impl Strategy<Value = MapOp> {
            prop_oneof![
                3 => (any::<u16>(), any::<u32>()).prop_map(|(k, v)| MapOp::Insert(k, v)),
                3 => any::<u16>().prop_map(MapOp::Remove),
                3 => any::<u16>().prop_map(MapOp::GetCloned),
                3 => any::<u16>().prop_map(MapOp::ContainsKey),
                1 => Just(MapOp::Len),
                1 => Just(MapOp::IsEmpty),
                3 => any::<u16>().prop_map(MapOp::LoadGet),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig {
                failure_persistence: Some(Box::new(
                    proptest::test_runner::FileFailurePersistence::WithSource("atomic_map")
                )),
                cases: 500,
                ..ProptestConfig::default()
            })]

            /// AtomicMap matches AHashMap behavior for any sequence of ops.
            #[rstest]
            fn atomic_map_matches_ahashmap(ops in proptest::collection::vec(map_op_strategy(), 0..200)) {
                let atomic = AtomicMap::new();
                let mut reference = AHashMap::new();

                for op in &ops {
                    match op {
                        MapOp::Insert(k, v) => {
                            atomic.insert(*k, *v);
                            reference.insert(*k, *v);
                        }
                        MapOp::Remove(k) => {
                            atomic.remove(k);
                            reference.remove(k);
                        }
                        MapOp::GetCloned(k) => {
                            prop_assert_eq!(
                                atomic.get_cloned(k),
                                reference.get(k).copied(),
                                "get_cloned mismatch for key {}", k
                            );
                        }
                        MapOp::ContainsKey(k) => {
                            prop_assert_eq!(
                                atomic.contains_key(k),
                                reference.contains_key(k),
                                "contains_key mismatch for key {}", k
                            );
                        }
                        MapOp::Len => {
                            prop_assert_eq!(atomic.len(), reference.len());
                        }
                        MapOp::IsEmpty => {
                            prop_assert_eq!(atomic.is_empty(), reference.is_empty());
                        }
                        MapOp::LoadGet(k) => {
                            let snapshot = atomic.load();
                            let via_load = snapshot.get(k).copied();
                            let via_method = atomic.get_cloned(k);
                            prop_assert_eq!(
                                via_load,
                                reference.get(k).copied(),
                                "load().get() mismatch for key {}", k
                            );
                            prop_assert_eq!(
                                via_method,
                                reference.get(k).copied(),
                                "get_cloned mismatch for key {}", k
                            );
                        }
                    }
                }

                prop_assert_eq!(atomic.len(), reference.len());
                prop_assert_eq!(atomic.is_empty(), reference.is_empty());

                for (k, v) in &reference {
                    prop_assert_eq!(
                        atomic.get_cloned(k),
                        Some(*v),
                        "value mismatch for key {}", k
                    );
                }
            }

            /// store() followed by reads yields exactly the stored contents.
            #[rstest]
            fn atomic_map_store_snapshot(
                items in proptest::collection::vec((any::<u16>(), any::<u32>()), 0..100),
            ) {
                let map = AtomicMap::new();
                map.insert(9999, 0);

                let expected: AHashMap<u16, u32> = items.into_iter().collect();
                map.store(expected.clone());

                prop_assert_eq!(map.len(), expected.len());
                prop_assert!(!map.contains_key(&9999) || expected.contains_key(&9999));

                for (k, v) in &expected {
                    prop_assert_eq!(map.get_cloned(k), Some(*v));
                }
            }

            /// rcu batch mutation matches sequential application.
            #[rstest]
            fn atomic_map_rcu_batch(
                initial in proptest::collection::vec((any::<u16>(), any::<u32>()), 0..50),
                to_add in proptest::collection::vec((any::<u16>(), any::<u32>()), 0..50),
                to_remove in proptest::collection::vec(any::<u16>(), 0..20),
            ) {
                let map = AtomicMap::new();
                let mut reference = AHashMap::new();

                for (k, v) in &initial {
                    map.insert(*k, *v);
                    reference.insert(*k, *v);
                }

                let to_add_clone = to_add.clone();
                let to_remove_clone = to_remove.clone();
                map.rcu(|m| {
                    for (k, v) in &to_add_clone {
                        m.insert(*k, *v);
                    }

                    for k in &to_remove_clone {
                        m.remove(k);
                    }
                });

                for (k, v) in &to_add {
                    reference.insert(*k, *v);
                }

                for k in &to_remove {
                    reference.remove(k);
                }

                prop_assert_eq!(map.len(), reference.len());
                for (k, v) in &reference {
                    prop_assert_eq!(map.get_cloned(k), Some(*v));
                }
            }

            /// load() returns a frozen snapshot unaffected by subsequent writes.
            #[rstest]
            fn atomic_map_snapshot_isolation(
                initial in proptest::collection::vec((any::<u16>(), any::<u32>()), 1..50),
                extra in proptest::collection::vec((any::<u16>(), any::<u32>()), 1..50),
            ) {
                let map = AtomicMap::new();
                for (k, v) in &initial {
                    map.insert(*k, *v);
                }

                let snapshot = map.load();
                let snapshot_contents: AHashMap<u16, u32> =
                    snapshot.iter().map(|(k, v)| (*k, *v)).collect();

                for (k, v) in &extra {
                    map.insert(*k, *v);
                }

                let snapshot_after: AHashMap<u16, u32> =
                    snapshot.iter().map(|(k, v)| (*k, *v)).collect();
                prop_assert_eq!(snapshot_contents, snapshot_after, "snapshot mutated after write");
            }

            /// From<AHashMap> roundtrip: every entry in the source is present.
            #[rstest]
            fn atomic_map_from_roundtrip(
                items in proptest::collection::vec((any::<u16>(), any::<u32>()), 0..100),
            ) {
                let expected: AHashMap<u16, u32> = items.into_iter().collect();
                let map = AtomicMap::from(expected.clone());

                prop_assert_eq!(map.len(), expected.len());
                for (k, v) in &expected {
                    prop_assert_eq!(map.get_cloned(k), Some(*v));
                }
            }
        }
    }

    #[rstest]
    fn test_hashset_setlike() {
        let mut set: HashSet<String> = HashSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: HashSet<String> = HashSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_indexset_setlike() {
        let mut set: IndexSet<String> = IndexSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: IndexSet<String> = IndexSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_into_ustr_vec_from_strings() {
        let items = vec!["foo".to_string(), "bar".to_string()];
        let ustrs = super::into_ustr_vec(items);

        assert_eq!(ustrs.len(), 2);
        assert_eq!(ustrs[0], Ustr::from("foo"));
        assert_eq!(ustrs[1], Ustr::from("bar"));
    }

    #[rstest]
    fn test_into_ustr_vec_from_str_slices() {
        let items = ["alpha", "beta", "gamma"];
        let ustrs = super::into_ustr_vec(items);

        assert_eq!(ustrs.len(), 3);
        assert_eq!(ustrs[2], Ustr::from("gamma"));
    }

    #[rstest]
    fn test_ahashset_setlike() {
        let mut set: AHashSet<String> = AHashSet::new();
        set.insert("test".to_string());
        set.insert("value".to_string());

        assert!(set.contains(&"test".to_string()));
        assert!(!set.contains(&"missing".to_string()));
        assert!(!set.is_empty());

        let empty_set: AHashSet<String> = AHashSet::new();
        assert!(empty_set.is_empty());
    }

    #[rstest]
    fn test_hashmap_maplike() {
        let mut map: HashMap<String, i32> = HashMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: HashMap<String, i32> = HashMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_indexmap_maplike() {
        let mut map: IndexMap<String, i32> = IndexMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: IndexMap<String, i32> = IndexMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_ahashmap_maplike() {
        let mut map: AHashMap<String, i32> = AHashMap::new();
        map.insert("key1".to_string(), 42);
        map.insert("key2".to_string(), 100);

        assert!(map.contains_key(&"key1".to_string()));
        assert!(!map.contains_key(&"missing".to_string()));
        assert!(!map.is_empty());

        let empty_map: AHashMap<String, i32> = AHashMap::new();
        assert!(empty_map.is_empty());
    }

    #[rstest]
    fn test_trait_object_setlike() {
        let mut hashset: HashSet<String> = HashSet::new();
        hashset.insert("test".to_string());

        let mut indexset: IndexSet<String> = IndexSet::new();
        indexset.insert("test".to_string());

        let sets: Vec<&dyn SetLike<Item = String>> = vec![&hashset, &indexset];

        for set in sets {
            assert!(set.contains(&"test".to_string()));
            assert!(!set.is_empty());
        }
    }

    #[rstest]
    fn test_trait_object_maplike() {
        let mut hashmap: HashMap<String, i32> = HashMap::new();
        hashmap.insert("key".to_string(), 42);

        let mut indexmap: IndexMap<String, i32> = IndexMap::new();
        indexmap.insert("key".to_string(), 42);

        let maps: Vec<&dyn MapLike<Key = String, Value = i32>> = vec![&hashmap, &indexmap];

        for map in maps {
            assert!(map.contains_key(&"key".to_string()));
            assert!(!map.is_empty());
        }
    }
}
