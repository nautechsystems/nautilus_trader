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

//! Bounded FIFO caches for tracking IDs and key-value pairs with O(1) lookups.

use std::{fmt::Debug, hash::Hash};

use ahash::{AHashMap, AHashSet};
use arraydeque::ArrayDeque;

/// A bounded cache that maintains a set of IDs with O(1) lookups.
///
/// Uses an `ArrayDeque` for FIFO ordering and an `AHashSet` for fast membership checks.
/// When capacity is exceeded, the oldest entry is automatically evicted.
///
/// # Examples
///
/// ```
/// use nautilus_common::cache::fifo::FifoCache;
///
/// let mut cache: FifoCache<u32, 3> = FifoCache::new();
/// cache.add(1);
/// cache.add(2);
/// cache.add(3);
/// assert!(cache.contains(&1));
///
/// // Adding beyond capacity evicts the oldest
/// cache.add(4);
/// assert!(!cache.contains(&1));
/// assert!(cache.contains(&4));
/// ```
///
/// Zero capacity is a compile-time error:
///
/// ```compile_fail
/// use nautilus_common::cache::fifo::FifoCache;
///
/// // This fails to compile: capacity must be > 0
/// let cache: FifoCache<u32, 0> = FifoCache::new();
/// ```
///
/// Default also enforces non-zero capacity:
///
/// ```compile_fail
/// use nautilus_common::cache::fifo::FifoCache;
///
/// // This also fails to compile
/// let cache: FifoCache<u32, 0> = FifoCache::default();
/// ```
#[derive(Debug)]
pub struct FifoCache<T, const N: usize>
where
    T: Clone + Debug + Eq + Hash,
{
    order: ArrayDeque<T, N>,
    index: AHashSet<T>,
}

impl<T, const N: usize> FifoCache<T, N>
where
    T: Clone + Debug + Eq + Hash,
{
    /// Creates a new empty [`FifoCache`] with capacity `N`.
    ///
    /// # Panics
    ///
    /// Compile-time panic if `N == 0`.
    #[must_use]
    pub fn new() -> Self {
        const { assert!(N > 0, "FifoCache capacity must be greater than zero") };

        Self {
            order: ArrayDeque::new(),
            index: AHashSet::with_capacity(N),
        }
    }

    /// Returns the capacity of the cache.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns the number of IDs in the cache.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns whether the cache contains the given ID (O(1) lookup).
    #[must_use]
    pub fn contains(&self, id: &T) -> bool {
        self.index.contains(id)
    }

    /// Adds an ID to the cache.
    ///
    /// If the ID already exists, this is a no-op.
    /// If the cache is at capacity, the oldest entry is evicted.
    pub fn add(&mut self, id: T) {
        if self.index.contains(&id) {
            return;
        }

        if self.order.is_full()
            && let Some(evicted) = self.order.pop_back()
        {
            self.index.remove(&evicted);
        }

        if self.order.push_front(id.clone()).is_ok() {
            self.index.insert(id);
        }
    }

    /// Removes an ID from the cache.
    pub fn remove(&mut self, id: &T) {
        if self.index.remove(id) {
            self.order.retain(|x| x != id);
        }
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.order.clear();
        self.index.clear();
    }
}

impl<T, const N: usize> Default for FifoCache<T, N>
where
    T: Clone + Debug + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

/// A bounded cache that maintains key-value pairs with O(1) lookups.
///
/// Uses an `ArrayDeque` for FIFO ordering and an `AHashMap` for fast key-value access.
/// When capacity is exceeded, the oldest entry is automatically evicted.
///
/// # Examples
///
/// ```
/// use nautilus_common::cache::fifo::FifoCacheMap;
///
/// let mut cache: FifoCacheMap<u32, String, 3> = FifoCacheMap::new();
/// cache.insert(1, "one".to_string());
/// cache.insert(2, "two".to_string());
/// cache.insert(3, "three".to_string());
/// assert_eq!(cache.get(&1), Some(&"one".to_string()));
///
/// // Adding beyond capacity evicts the oldest
/// cache.insert(4, "four".to_string());
/// assert_eq!(cache.get(&1), None);
/// assert_eq!(cache.get(&4), Some(&"four".to_string()));
/// ```
///
/// Zero capacity is a compile-time error:
///
/// ```compile_fail
/// use nautilus_common::cache::fifo::FifoCacheMap;
///
/// // This fails to compile: capacity must be > 0
/// let cache: FifoCacheMap<u32, String, 0> = FifoCacheMap::new();
/// ```
#[derive(Debug)]
pub struct FifoCacheMap<K, V, const N: usize>
where
    K: Clone + Debug + Eq + Hash,
{
    order: ArrayDeque<K, N>,
    index: AHashMap<K, V>,
}

impl<K, V, const N: usize> FifoCacheMap<K, V, N>
where
    K: Clone + Debug + Eq + Hash,
{
    /// Creates a new empty [`FifoCacheMap`] with capacity `N`.
    ///
    /// # Panics
    ///
    /// Compile-time panic if `N == 0`.
    #[must_use]
    pub fn new() -> Self {
        const { assert!(N > 0, "FifoCacheMap capacity must be greater than zero") };

        Self {
            order: ArrayDeque::new(),
            index: AHashMap::with_capacity(N),
        }
    }

    /// Returns the capacity of the cache.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns the number of entries in the cache.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns whether the cache contains the given key (O(1) lookup).
    #[must_use]
    pub fn contains_key(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    /// Returns a reference to the value for the given key (O(1) lookup).
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.index.get(key)
    }

    /// Returns a mutable reference to the value for the given key (O(1) lookup).
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.index.get_mut(key)
    }

    /// Inserts a key-value pair into the cache.
    ///
    /// If the key already exists, the value is updated (no eviction occurs).
    /// If the cache is at capacity and the key is new, the oldest entry is evicted.
    pub fn insert(&mut self, key: K, value: V) {
        if self.index.contains_key(&key) {
            self.index.insert(key, value);
            return;
        }

        if self.order.is_full()
            && let Some(evicted) = self.order.pop_back()
        {
            self.index.remove(&evicted);
        }

        if self.order.push_front(key.clone()).is_ok() {
            self.index.insert(key, value);
        }
    }

    /// Removes a key from the cache, returning the value if present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(value) = self.index.remove(key) {
            self.order.retain(|x| x != key);
            Some(value)
        } else {
            None
        }
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.order.clear();
        self.index.clear();
    }
}

impl<K, V, const N: usize> Default for FifoCacheMap<K, V, N>
where
    K: Clone + Debug + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_add_and_contains() {
        let mut cache: FifoCache<u32, 4> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.add(3);

        assert!(cache.contains(&1));
        assert!(cache.contains(&2));
        assert!(cache.contains(&3));
        assert!(!cache.contains(&4));
        assert_eq!(cache.len(), 3);
    }

    #[rstest]
    fn test_eviction_at_capacity() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.add(3);
        assert_eq!(cache.len(), 3);

        // Adding a 4th should evict the oldest (1)
        cache.add(4);
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains(&1));
        assert!(cache.contains(&2));
        assert!(cache.contains(&3));
        assert!(cache.contains(&4));
    }

    #[rstest]
    fn test_duplicate_add_is_noop() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.add(1); // duplicate

        assert_eq!(cache.len(), 2);
        assert!(cache.contains(&1));
        assert!(cache.contains(&2));
    }

    #[rstest]
    fn test_remove() {
        let mut cache: FifoCache<u32, 4> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.add(3);

        cache.remove(&2);
        assert_eq!(cache.len(), 2);
        assert!(cache.contains(&1));
        assert!(!cache.contains(&2));
        assert!(cache.contains(&3));
    }

    #[rstest]
    fn test_remove_nonexistent_is_noop() {
        let mut cache: FifoCache<u32, 4> = FifoCache::new();
        cache.add(1);
        cache.remove(&99);
        assert_eq!(cache.len(), 1);
    }

    #[rstest]
    fn test_capacity() {
        let cache: FifoCache<u32, 10> = FifoCache::new();
        assert_eq!(cache.capacity(), 10);
    }

    #[rstest]
    fn test_is_empty() {
        let mut cache: FifoCache<u32, 4> = FifoCache::new();
        assert!(cache.is_empty());
        cache.add(1);
        assert!(!cache.is_empty());
    }

    #[rstest]
    fn test_capacity_one_evicts_immediately() {
        let mut cache: FifoCache<u32, 1> = FifoCache::new();
        cache.add(1);
        assert!(cache.contains(&1));
        assert_eq!(cache.len(), 1);

        cache.add(2);
        assert!(!cache.contains(&1));
        assert!(cache.contains(&2));
        assert_eq!(cache.len(), 1);
    }

    #[rstest]
    fn test_sequential_eviction_order() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();

        // Fill: [3, 2, 1] (front to back)
        cache.add(1);
        cache.add(2);
        cache.add(3);

        // Add 4: evicts 1 -> [4, 3, 2]
        cache.add(4);
        assert!(!cache.contains(&1));
        assert!(cache.contains(&2));

        // Add 5: evicts 2 -> [5, 4, 3]
        cache.add(5);
        assert!(!cache.contains(&2));
        assert!(cache.contains(&3));

        // Add 6: evicts 3 -> [6, 5, 4]
        cache.add(6);
        assert!(!cache.contains(&3));
        assert!(cache.contains(&4));
        assert!(cache.contains(&5));
        assert!(cache.contains(&6));
    }

    #[rstest]
    fn test_remove_then_readd() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.remove(&1);
        assert!(!cache.contains(&1));
        assert_eq!(cache.len(), 1);

        cache.add(1);
        assert!(cache.contains(&1));
        assert_eq!(cache.len(), 2);
    }

    #[rstest]
    fn test_remove_frees_slot_for_new_element() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();

        cache.add(1);
        cache.add(2);
        cache.add(3);
        cache.remove(&2);
        assert_eq!(cache.len(), 2);

        // Add new element - should not evict anyone
        cache.add(4);
        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&1));
        assert!(cache.contains(&3));
        assert!(cache.contains(&4));
    }

    #[rstest]
    fn test_duplicate_add_does_not_refresh_position() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();

        // Add 1, 2, 3 (1 is oldest)
        cache.add(1);
        cache.add(2);
        cache.add(3);

        // Re-add 1 (should be no-op, 1 stays oldest)
        cache.add(1);

        // Add 4: should evict 1 (still oldest), not 2
        cache.add(4);
        assert!(!cache.contains(&1));
        assert!(cache.contains(&2));
        assert!(cache.contains(&3));
        assert!(cache.contains(&4));
    }

    #[rstest]
    fn test_interleaved_add_remove() {
        let mut cache: FifoCache<u32, 4> = FifoCache::new();

        cache.add(1);
        cache.add(2);
        cache.remove(&1);
        cache.add(3);
        cache.add(4);
        cache.remove(&3);
        cache.add(5);

        assert!(!cache.contains(&1));
        assert!(cache.contains(&2));
        assert!(!cache.contains(&3));
        assert!(cache.contains(&4));
        assert!(cache.contains(&5));
        assert_eq!(cache.len(), 3);
    }

    #[rstest]
    fn test_remove_all_elements() {
        let mut cache: FifoCache<u32, 3> = FifoCache::new();
        cache.add(1);
        cache.add(2);
        cache.add(3);

        cache.remove(&1);
        cache.remove(&2);
        cache.remove(&3);

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[rstest]
    fn test_string_type() {
        let mut cache: FifoCache<String, 2> = FifoCache::new();
        cache.add("hello".to_string());
        cache.add("world".to_string());

        assert!(cache.contains(&"hello".to_string()));
        assert!(cache.contains(&"world".to_string()));

        cache.add("foo".to_string());
        assert!(!cache.contains(&"hello".to_string()));
    }

    #[rstest]
    fn test_map_insert_and_get() {
        let mut cache: FifoCacheMap<u32, String, 4> = FifoCacheMap::new();
        cache.insert(1, "one".to_string());
        cache.insert(2, "two".to_string());
        cache.insert(3, "three".to_string());

        assert_eq!(cache.get(&1), Some(&"one".to_string()));
        assert_eq!(cache.get(&2), Some(&"two".to_string()));
        assert_eq!(cache.get(&3), Some(&"three".to_string()));
        assert_eq!(cache.get(&4), None);
        assert_eq!(cache.len(), 3);
    }

    #[rstest]
    fn test_map_eviction_at_capacity() {
        let mut cache: FifoCacheMap<u32, &str, 3> = FifoCacheMap::new();
        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");
        assert_eq!(cache.len(), 3);

        // Adding a 4th should evict the oldest (1)
        cache.insert(4, "four");
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"two"));
        assert_eq!(cache.get(&3), Some(&"three"));
        assert_eq!(cache.get(&4), Some(&"four"));
    }

    #[rstest]
    fn test_map_update_existing_key() {
        let mut cache: FifoCacheMap<u32, &str, 3> = FifoCacheMap::new();
        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        // Update existing key - should not evict
        cache.insert(1, "ONE");
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&1), Some(&"ONE"));
        assert_eq!(cache.get(&2), Some(&"two"));
        assert_eq!(cache.get(&3), Some(&"three"));
    }

    #[rstest]
    fn test_map_remove() {
        let mut cache: FifoCacheMap<u32, &str, 4> = FifoCacheMap::new();
        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        let removed = cache.remove(&2);
        assert_eq!(removed, Some("two"));
        assert_eq!(cache.len(), 2);
        assert!(cache.contains_key(&1));
        assert!(!cache.contains_key(&2));
        assert!(cache.contains_key(&3));
    }

    #[rstest]
    fn test_map_remove_nonexistent() {
        let mut cache: FifoCacheMap<u32, &str, 4> = FifoCacheMap::new();
        cache.insert(1, "one");
        let removed = cache.remove(&99);
        assert_eq!(removed, None);
        assert_eq!(cache.len(), 1);
    }

    #[rstest]
    fn test_map_get_mut() {
        let mut cache: FifoCacheMap<u32, String, 4> = FifoCacheMap::new();
        cache.insert(1, "one".to_string());

        if let Some(value) = cache.get_mut(&1) {
            value.push_str("_modified");
        }

        assert_eq!(cache.get(&1), Some(&"one_modified".to_string()));
    }

    #[rstest]
    fn test_map_capacity() {
        let cache: FifoCacheMap<u32, &str, 10> = FifoCacheMap::new();
        assert_eq!(cache.capacity(), 10);
    }

    #[rstest]
    fn test_map_is_empty() {
        let mut cache: FifoCacheMap<u32, &str, 4> = FifoCacheMap::new();
        assert!(cache.is_empty());
        cache.insert(1, "one");
        assert!(!cache.is_empty());
    }

    #[rstest]
    fn test_map_capacity_one() {
        let mut cache: FifoCacheMap<u32, &str, 1> = FifoCacheMap::new();
        cache.insert(1, "one");
        assert_eq!(cache.get(&1), Some(&"one"));

        cache.insert(2, "two");
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&"two"));
        assert_eq!(cache.len(), 1);
    }

    #[rstest]
    fn test_map_sequential_eviction() {
        let mut cache: FifoCacheMap<u32, u32, 3> = FifoCacheMap::new();

        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.insert(3, 30);

        // Add 4: evicts 1
        cache.insert(4, 40);
        assert!(!cache.contains_key(&1));
        assert!(cache.contains_key(&2));

        // Add 5: evicts 2
        cache.insert(5, 50);
        assert!(!cache.contains_key(&2));
        assert!(cache.contains_key(&3));
    }

    #[rstest]
    fn test_map_update_does_not_change_eviction_order() {
        let mut cache: FifoCacheMap<u32, &str, 3> = FifoCacheMap::new();

        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        // Update key 1 - should NOT move it to front
        cache.insert(1, "ONE");

        // Add new key - should still evict 1 (oldest by insertion order)
        cache.insert(4, "four");
        assert!(!cache.contains_key(&1));
        assert!(cache.contains_key(&2));
        assert!(cache.contains_key(&3));
        assert!(cache.contains_key(&4));
    }

    #[rstest]
    fn test_map_remove_frees_slot() {
        let mut cache: FifoCacheMap<u32, &str, 3> = FifoCacheMap::new();

        cache.insert(1, "one");
        cache.insert(2, "two");
        cache.insert(3, "three");

        cache.remove(&2);
        assert_eq!(cache.len(), 2);

        // Add new element - should not evict anyone
        cache.insert(4, "four");
        assert_eq!(cache.len(), 3);
        assert!(cache.contains_key(&1));
        assert!(cache.contains_key(&3));
        assert!(cache.contains_key(&4));
    }

    use proptest::prelude::*;

    /// Operations that can be performed on a FifoCache
    #[derive(Clone, Debug)]
    enum Op {
        Add(u8),
        Remove(u8),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![(0..50u8).prop_map(Op::Add), (0..50u8).prop_map(Op::Remove),]
    }

    fn ops_strategy() -> impl Strategy<Value = Vec<Op>> {
        proptest::collection::vec(op_strategy(), 0..100)
    }

    /// Apply operations and return final cache state
    fn apply_ops<const N: usize>(ops: &[Op]) -> FifoCache<u8, N> {
        let mut cache = FifoCache::<u8, N>::new();
        for op in ops {
            match op {
                Op::Add(id) => cache.add(*id),
                Op::Remove(id) => cache.remove(id),
            }
        }
        cache
    }

    proptest! {
        /// Invariant: len() never exceeds capacity
        #[rstest]
        fn prop_len_never_exceeds_capacity(ops in ops_strategy()) {
            let cache = apply_ops::<8>(&ops);
            prop_assert!(cache.len() <= cache.capacity());
        }

        /// Invariant: is_empty() iff len() == 0
        #[rstest]
        fn prop_is_empty_consistent_with_len(ops in ops_strategy()) {
            let cache = apply_ops::<8>(&ops);
            if cache.is_empty() {
                prop_assert_eq!(cache.len(), 0);
            } else {
                prop_assert!(!cache.is_empty());
            }
        }

        /// Invariant: Adding a duplicate does not change len
        #[rstest]
        fn prop_add_duplicate_is_idempotent(
            ops in ops_strategy(),
            id in 0..50u8
        ) {
            let mut cache = apply_ops::<8>(&ops);
            cache.add(id);
            let len_after_first = cache.len();
            let contained_after_first = cache.contains(&id);

            cache.add(id);
            prop_assert_eq!(cache.len(), len_after_first);
            prop_assert_eq!(cache.contains(&id), contained_after_first);
        }

        /// Invariant: After remove(x), contains(x) is false
        #[rstest]
        fn prop_remove_ensures_not_contained(
            ops in ops_strategy(),
            id in 0..50u8
        ) {
            let mut cache = apply_ops::<8>(&ops);
            cache.remove(&id);
            prop_assert!(!cache.contains(&id));
        }

        /// Invariant: After add(x), contains(x) is true (unless immediately evicted)
        #[rstest]
        fn prop_add_ensures_contained_if_capacity(id in 0..50u8) {
            let mut cache: FifoCache<u8, 8> = FifoCache::new();
            cache.add(id);
            prop_assert!(cache.contains(&id));
        }

        /// Invariant: FIFO eviction order - oldest element evicted first
        #[rstest]
        fn prop_fifo_eviction_order(extra in 0..20u8) {
            let mut cache: FifoCache<u8, 4> = FifoCache::new();

            // Fill cache with 0, 1, 2, 3
            for i in 0..4u8 {
                cache.add(i);
            }
            prop_assert_eq!(cache.len(), 4);

            // Add more elements, should evict in FIFO order
            for i in 0..extra {
                let new_id = 100 + i;
                cache.add(new_id);

                // The element that should have been evicted
                let evicted = i;
                if evicted < 4 {
                    prop_assert!(!cache.contains(&evicted),
                        "Element {} should have been evicted", evicted);
                }
            }
        }

        /// Invariant: Remove on empty cache is safe no-op
        #[rstest]
        fn prop_remove_on_empty_is_noop(id in 0..50u8) {
            let mut cache: FifoCache<u8, 8> = FifoCache::new();
            cache.remove(&id);
            prop_assert!(cache.is_empty());
            prop_assert_eq!(cache.len(), 0);
        }

        /// Invariant: len() decreases by 1 when removing existing element
        #[rstest]
        fn prop_remove_decreases_len(
            ops in ops_strategy(),
            id in 0..50u8
        ) {
            let mut cache = apply_ops::<8>(&ops);
            cache.add(id); // Ensure it exists
            let len_before = cache.len();

            cache.remove(&id);

            if cache.contains(&id) {
                prop_assert!(false, "Element still contained after remove");
            }
            prop_assert!(cache.len() < len_before || len_before == 0);
        }

        /// Invariant: At capacity, adding new element keeps len same
        #[rstest]
        fn prop_add_at_capacity_maintains_len(new_id in 50..100u8) {
            let mut cache: FifoCache<u8, 4> = FifoCache::new();

            // Fill to capacity with distinct values
            for i in 0..4u8 {
                cache.add(i);
            }
            prop_assert_eq!(cache.len(), 4);

            // Add new element (guaranteed not in cache)
            cache.add(new_id);
            prop_assert_eq!(cache.len(), 4);
        }

        /// Invariant: All added elements are contained until evicted or removed
        #[rstest]
        fn prop_recent_adds_are_contained(recent in proptest::collection::vec(0..50u8, 1..5)) {
            let mut cache: FifoCache<u8, 8> = FifoCache::new();

            for &id in &recent {
                cache.add(id);
            }

            // Deduplicate to get expected unique count
            let mut unique: Vec<u8> = recent;
            unique.sort_unstable();
            unique.dedup();
            let expected_len = unique.len().min(8);

            prop_assert_eq!(cache.len(), expected_len);

            // All unique recent adds should be contained (capacity is 8, we add at most 5)
            for id in unique {
                prop_assert!(cache.contains(&id), "Recently added {} not contained", id);
            }
        }

        /// Invariant: len() never exceeds capacity for map
        #[rstest]
        fn prop_map_len_never_exceeds_capacity(
            keys in proptest::collection::vec(0..50u8, 0..100)
        ) {
            let mut cache: FifoCacheMap<u8, u8, 8> = FifoCacheMap::new();
            for key in keys {
                cache.insert(key, key);
            }
            prop_assert!(cache.len() <= cache.capacity());
        }

        /// Invariant: is_empty() iff len() == 0 for map
        #[rstest]
        fn prop_map_is_empty_consistent_with_len(
            keys in proptest::collection::vec(0..50u8, 0..20)
        ) {
            let mut cache: FifoCacheMap<u8, u8, 8> = FifoCacheMap::new();
            for key in keys {
                cache.insert(key, key);
            }
            if cache.is_empty() {
                prop_assert_eq!(cache.len(), 0);
            } else {
                prop_assert!(!cache.is_empty());
            }
        }

        /// Invariant: Updating existing key does not change len
        #[rstest]
        fn prop_map_update_is_idempotent_for_len(
            keys in proptest::collection::vec(0..50u8, 1..10),
            key in 0..50u8
        ) {
            let mut cache: FifoCacheMap<u8, u8, 8> = FifoCacheMap::new();
            for k in keys {
                cache.insert(k, k);
            }
            cache.insert(key, 100);
            let len_after_first = cache.len();

            cache.insert(key, 200);
            prop_assert_eq!(cache.len(), len_after_first);
        }

        /// Invariant: After remove(k), get(k) is None
        #[rstest]
        fn prop_map_remove_ensures_not_contained(
            keys in proptest::collection::vec(0..50u8, 0..20),
            key in 0..50u8
        ) {
            let mut cache: FifoCacheMap<u8, u8, 8> = FifoCacheMap::new();
            for k in keys {
                cache.insert(k, k);
            }
            cache.remove(&key);
            prop_assert!(cache.get(&key).is_none());
        }

        /// Invariant: After insert(k, v), get(k) returns Some(&v)
        #[rstest]
        fn prop_map_insert_ensures_get(key in 0..50u8, value in 0..100u8) {
            let mut cache: FifoCacheMap<u8, u8, 8> = FifoCacheMap::new();
            cache.insert(key, value);
            prop_assert_eq!(cache.get(&key), Some(&value));
        }

        /// Invariant: At capacity, inserting new key keeps len same
        #[rstest]
        fn prop_map_insert_at_capacity_maintains_len(new_key in 50..100u8) {
            let mut cache: FifoCacheMap<u8, u8, 4> = FifoCacheMap::new();

            for i in 0..4u8 {
                cache.insert(i, i * 10);
            }
            prop_assert_eq!(cache.len(), 4);

            cache.insert(new_key, 99);
            prop_assert_eq!(cache.len(), 4);
        }

        /// Invariant: FIFO eviction for map
        #[rstest]
        fn prop_map_fifo_eviction(extra in 0..20u8) {
            let mut cache: FifoCacheMap<u8, u8, 4> = FifoCacheMap::new();

            for i in 0..4u8 {
                cache.insert(i, i * 10);
            }

            for i in 0..extra {
                let new_key = 100 + i;
                cache.insert(new_key, new_key);

                let evicted = i;
                if evicted < 4 {
                    prop_assert!(cache.get(&evicted).is_none(),
                        "Key {} should have been evicted", evicted);
                }
            }
        }
    }
}
