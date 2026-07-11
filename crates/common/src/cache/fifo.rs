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

use std::{collections::VecDeque, fmt::Debug, hash::Hash};

use ahash::{AHashMap, AHashSet};

/// A bounded cache that maintains a set of IDs with O(1) lookups.
///
/// Uses a `VecDeque` for FIFO ordering and an `AHashSet` for fast membership checks.
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
    order: VecDeque<T>,
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
            order: VecDeque::with_capacity(N),
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

        if self.order.len() == N
            && let Some(evicted) = self.order.pop_back()
        {
            self.index.remove(&evicted);
        }

        self.order.push_front(id.clone());
        self.index.insert(id);
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
/// Uses a `VecDeque` for FIFO ordering and an `AHashMap` for fast key-value access.
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
    order: VecDeque<K>,
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
            order: VecDeque::with_capacity(N),
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

        if self.order.len() == N
            && let Some(evicted) = self.order.pop_back()
        {
            self.index.remove(&evicted);
        }

        self.order.push_front(key.clone());
        self.index.insert(key, value);
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

    use ahash::AHashMap;
    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    enum SetOperation {
        Add(u8),
        Remove(u8),
    }

    fn set_operation_strategy() -> impl Strategy<Value = SetOperation> {
        prop_oneof![
            (0..50u8).prop_map(SetOperation::Add),
            (0..50u8).prop_map(SetOperation::Remove),
        ]
    }

    fn set_operations_strategy() -> impl Strategy<Value = Vec<SetOperation>> {
        proptest::collection::vec(set_operation_strategy(), 0..100)
    }

    #[derive(Clone, Debug)]
    enum MapOperation {
        Insert(u8, u8),
        Remove(u8),
    }

    fn map_operation_strategy() -> impl Strategy<Value = MapOperation> {
        prop_oneof![
            (0..50u8, any::<u8>()).prop_map(|(key, value)| MapOperation::Insert(key, value)),
            (0..50u8).prop_map(MapOperation::Remove),
        ]
    }

    fn map_operations_strategy() -> impl Strategy<Value = Vec<MapOperation>> {
        proptest::collection::vec(map_operation_strategy(), 0..100)
    }

    proptest! {
        #[rstest]
        fn prop_set_operations_match_reference(operations in set_operations_strategy()) {
            let mut cache: FifoCache<u8, 8> = FifoCache::new();
            let mut expected_order = Vec::new();

            for operation in operations {
                match operation {
                    SetOperation::Add(id) => {
                        cache.add(id);
                        if !expected_order.contains(&id) {
                            if expected_order.len() == cache.capacity() {
                                expected_order.pop();
                            }
                            expected_order.insert(0, id);
                        }
                    }
                    SetOperation::Remove(id) => {
                        cache.remove(&id);
                        expected_order.retain(|expected| *expected != id);
                    }
                }

                prop_assert_eq!(cache.len(), expected_order.len());
                prop_assert_eq!(cache.is_empty(), expected_order.is_empty());
                for id in 0..50u8 {
                    prop_assert_eq!(cache.contains(&id), expected_order.contains(&id));
                }
            }
        }

        #[rstest]
        fn prop_map_operations_match_reference(operations in map_operations_strategy()) {
            let mut cache: FifoCacheMap<u8, u8, 4> = FifoCacheMap::new();
            let mut expected_order = Vec::new();
            let mut expected_values = AHashMap::new();

            for operation in operations {
                match operation {
                    MapOperation::Insert(key, value) => {
                        cache.insert(key, value);
                        if expected_values.contains_key(&key) {
                            expected_values.insert(key, value);
                        } else {
                            if expected_order.len() == cache.capacity() {
                                let evicted = expected_order.pop().unwrap();
                                expected_values.remove(&evicted);
                            }
                            expected_order.insert(0, key);
                            expected_values.insert(key, value);
                        }
                    }
                    MapOperation::Remove(key) => {
                        cache.remove(&key);
                        if expected_values.remove(&key).is_some() {
                            expected_order.retain(|expected| *expected != key);
                        }
                    }
                }

                prop_assert_eq!(cache.len(), expected_values.len());
                prop_assert_eq!(cache.is_empty(), expected_values.is_empty());
                for key in 0..50u8 {
                    prop_assert_eq!(cache.get(&key).copied(), expected_values.get(&key).copied());
                }
            }
        }
    }
}
