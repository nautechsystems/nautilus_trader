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

//! A bounded FIFO deque that automatically evicts the oldest entry when at capacity.

use std::collections::VecDeque;

use nautilus_core::correctness::{CorrectnessResultExt, FAILED, check_positive_usize};

/// A bounded deque that maintains at most `capacity` elements.
///
/// Uses a `VecDeque` internally with runtime-configured capacity.
/// When capacity is exceeded on `push_front`, the oldest entry (back) is automatically evicted.
#[derive(Debug, Clone)]
pub(super) struct BoundedVecDeque<T> {
    inner: VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedVecDeque<T> {
    /// Creates a new [`BoundedVecDeque`] with the given maximum capacity.
    #[must_use]
    pub(super) fn new(capacity: usize) -> Self {
        check_positive_usize(capacity, stringify!(capacity)).expect_display(FAILED);

        Self {
            inner: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Pushes an item to the front. If at capacity, the oldest item (back) is evicted.
    pub(super) fn push_front(&mut self, item: T) {
        if self.inner.len() >= self.capacity {
            self.inner.pop_back();
        }
        self.inner.push_front(item);
    }

    /// Returns the number of elements.
    #[must_use]
    pub(super) fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns a reference to the front (newest) element.
    #[must_use]
    pub(super) fn front(&self) -> Option<&T> {
        self.inner.front()
    }

    /// Returns a reference to the element at the given index.
    #[must_use]
    pub(super) fn get(&self, index: usize) -> Option<&T> {
        self.inner.get(index)
    }

    /// Returns an iterator over the elements.
    pub(super) fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.inner.iter()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_deque_is_empty_with_correct_capacity() {
        let deque: BoundedVecDeque<i32> = BoundedVecDeque::new(100);

        assert_eq!(deque.len(), 0);
        assert_eq!(deque.front(), None);
        assert_eq!(deque.get(0), None);
    }

    #[rstest]
    #[should_panic(expected = "invalid usize for 'capacity' not positive")]
    fn test_new_rejects_zero_capacity() {
        let _deque: BoundedVecDeque<i32> = BoundedVecDeque::new(0);
    }

    #[rstest]
    fn test_push_front_maintains_newest_first_ordering() {
        let mut deque = BoundedVecDeque::new(5);
        for i in 1..=5 {
            deque.push_front(i * 10);
        }

        assert_eq!(deque.len(), 5);
        assert_eq!(deque.front(), Some(&50)); // newest
        assert_eq!(deque.get(0), Some(&50));
        assert_eq!(deque.get(1), Some(&40));
        assert_eq!(deque.get(2), Some(&30));
        assert_eq!(deque.get(3), Some(&20));
        assert_eq!(deque.get(4), Some(&10)); // oldest
        assert_eq!(deque.get(5), None); // out of bounds

        let items: Vec<_> = deque.iter().copied().collect();
        assert_eq!(items, vec![50, 40, 30, 20, 10]);
    }

    #[rstest]
    fn test_eviction_drops_oldest_and_preserves_order() {
        let mut deque = BoundedVecDeque::new(3);

        // Fill to capacity: front=[30, 20, 10]=back
        deque.push_front(10);
        deque.push_front(20);
        deque.push_front(30);
        assert_eq!(deque.len(), 3);

        // Push 40: evicts 10 (oldest) -> front=[40, 30, 20]=back
        deque.push_front(40);
        assert_eq!(deque.len(), 3);
        assert_eq!(deque.front(), Some(&40));
        let items: Vec<_> = deque.iter().copied().collect();
        assert_eq!(items, vec![40, 30, 20]);

        // Push 50: evicts 20 -> front=[50, 40, 30]=back
        deque.push_front(50);
        assert_eq!(deque.len(), 3);
        let items: Vec<_> = deque.iter().copied().collect();
        assert_eq!(items, vec![50, 40, 30]);

        // Push 60: evicts 30 -> front=[60, 50, 40]=back
        deque.push_front(60);
        assert_eq!(deque.len(), 3);
        let items: Vec<_> = deque.iter().copied().collect();
        assert_eq!(items, vec![60, 50, 40]);
    }

    #[rstest]
    fn test_capacity_one_always_holds_latest() {
        let mut deque = BoundedVecDeque::new(1);

        for i in 1..=100 {
            deque.push_front(i);
            assert_eq!(deque.len(), 1);
            assert_eq!(deque.front(), Some(&i));
        }
    }

    #[rstest]
    fn test_len_never_exceeds_capacity_under_sustained_load() {
        let capacity = 50;
        let mut deque = BoundedVecDeque::new(capacity);

        for i in 0..10_000 {
            deque.push_front(i);
            assert!(
                deque.len() <= capacity,
                "len {} exceeded capacity {} after {} pushes",
                deque.len(),
                capacity,
                i + 1,
            );
        }

        assert_eq!(deque.len(), capacity);
        assert_eq!(deque.front(), Some(&9_999));
        assert_eq!(deque.iter().last(), Some(&(10_000 - capacity as i32)));
    }

    proptest! {
        #[rstest]
        fn prop_len_never_exceeds_capacity(
            capacity in 1usize..64,
            items in proptest::collection::vec(any::<u16>(), 0..256),
        ) {
            let mut deque = BoundedVecDeque::new(capacity);

            for item in items {
                deque.push_front(item);
                prop_assert!(deque.len() <= capacity);
            }
        }

        #[rstest]
        fn prop_retains_latest_items_in_newest_first_order(
            capacity in 1usize..64,
            items in proptest::collection::vec(any::<u16>(), 0..256),
        ) {
            let mut deque = BoundedVecDeque::new(capacity);
            for item in &items {
                deque.push_front(*item);
            }

            let actual: Vec<_> = deque.iter().copied().collect();
            let expected: Vec<_> = items.iter().rev().take(capacity).copied().collect();

            prop_assert_eq!(actual, expected);
        }
    }
}
