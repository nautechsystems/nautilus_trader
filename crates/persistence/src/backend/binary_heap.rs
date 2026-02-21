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

//! A priority queue implemented with a binary heap.
//!
//! Vendored from the `binary-heap-plus` crate which depends on the unmaintained
//! `compare` crate. Distributed here under MIT (see `licenses/` directory).
//! Original source: <https://github.com/sekineh/binary-heap-plus-rs>

#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    fmt,
    mem::{ManuallyDrop, swap},
    ops::{Deref, DerefMut},
    ptr,
};

use super::compare::Compare;

/// A priority queue implemented with a binary heap.
///
/// This will be a max-heap by default, but the ordering is determined by the
/// comparator `C`.
pub struct BinaryHeap<T, C> {
    data: Vec<T>,
    cmp: C,
}

impl<T, C: Compare<T>> BinaryHeap<T, C> {
    /// Creates a `BinaryHeap` from a `Vec` and comparator.
    pub fn from_vec_cmp(vec: Vec<T>, cmp: C) -> Self {
        let mut heap = Self { data: vec, cmp };
        if !heap.data.is_empty() {
            heap.rebuild();
        }
        heap
    }

    /// Returns a mutable reference to the greatest item in the binary heap, or
    /// `None` if it is empty.
    pub fn peek_mut(&mut self) -> Option<PeekMut<'_, T, C>> {
        if self.is_empty() {
            None
        } else {
            Some(PeekMut {
                heap: self,
                sift: false,
            })
        }
    }

    /// Removes the greatest item from the binary heap and returns it, or `None`
    /// if it is empty.
    pub fn pop(&mut self) -> Option<T> {
        self.data.pop().map(|mut item| {
            if !self.is_empty() {
                swap(&mut item, &mut self.data[0]);
                // SAFETY: !self.is_empty() means that self.len() > 0
                unsafe { self.sift_down_to_bottom(0) };
            }
            item
        })
    }

    /// Pushes an item onto the binary heap.
    pub fn push(&mut self, item: T) {
        let old_len = self.len();
        self.data.push(item);
        // SAFETY: Since we pushed a new item it means that
        //  old_len = self.len() - 1 < self.len()
        unsafe { self.sift_up(0, old_len) };
    }

    /// Sifts an element up towards the root until heap property is restored.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`.
    unsafe fn sift_up(&mut self, start: usize, pos: usize) -> usize {
        // SAFETY: The caller guarantees that pos < self.len()
        let mut hole = unsafe { Hole::new(&mut self.data, pos) };

        while hole.pos() > start {
            let parent = (hole.pos() - 1) / 2;

            // SAFETY: hole.pos() > start >= 0, which means hole.pos() > 0
            //  and so hole.pos() - 1 can't underflow.
            if self
                .cmp
                .compares_le(hole.element(), unsafe { hole.get(parent) })
            {
                break;
            }

            // SAFETY: Same as above
            unsafe { hole.move_to(parent) };
        }

        hole.pos()
    }

    /// Sifts an element down within the range `[pos, end)`.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < end <= self.len()`.
    unsafe fn sift_down_range(&mut self, pos: usize, end: usize) {
        // SAFETY: The caller guarantees that pos < end <= self.len().
        let mut hole = unsafe { Hole::new(&mut self.data, pos) };
        let mut child = 2 * hole.pos() + 1;

        while child <= end.saturating_sub(2) {
            // SAFETY: child < end - 1 < self.len() and
            //  child + 1 < end <= self.len(), so they're valid indexes.
            child += unsafe { self.cmp.compares_le(hole.get(child), hole.get(child + 1)) } as usize;

            // SAFETY: child is now either the old child or the old child+1
            if self
                .cmp
                .compares_ge(hole.element(), unsafe { hole.get(child) })
            {
                return;
            }

            // SAFETY: same as above.
            unsafe { hole.move_to(child) };
            child = 2 * hole.pos() + 1;
        }

        // SAFETY: && short circuit
        if child == end - 1
            && self
                .cmp
                .compares_lt(hole.element(), unsafe { hole.get(child) })
        {
            unsafe { hole.move_to(child) };
        }
    }

    /// Sifts an element down until heap property is restored.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`.
    unsafe fn sift_down(&mut self, pos: usize) {
        let len = self.len();
        // SAFETY: pos < len is guaranteed by the caller
        unsafe { self.sift_down_range(pos, len) };
    }

    /// Take an element at `pos` and move it all the way down the heap,
    /// then sift it up to its position.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`.
    unsafe fn sift_down_to_bottom(&mut self, mut pos: usize) {
        let end = self.len();
        let start = pos;

        // SAFETY: The caller guarantees that pos < self.len().
        let mut hole = unsafe { Hole::new(&mut self.data, pos) };
        let mut child = 2 * hole.pos() + 1;

        while child <= end.saturating_sub(2) {
            // SAFETY: child < end - 1 < self.len() and
            //  child + 1 < end <= self.len(), so they're valid indexes.
            child += unsafe { self.cmp.compares_le(hole.get(child), hole.get(child + 1)) } as usize;

            // SAFETY: Same as above
            unsafe { hole.move_to(child) };
            child = 2 * hole.pos() + 1;
        }

        if child == end - 1 {
            // SAFETY: child == end - 1 < self.len(), so it's a valid index
            unsafe { hole.move_to(child) };
        }
        pos = hole.pos();
        drop(hole);

        // SAFETY: pos is the position in the hole
        unsafe { self.sift_up(start, pos) };
    }

    /// Rebuilds the heap from an unordered vector.
    fn rebuild(&mut self) {
        let mut n = self.len() / 2;
        while n > 0 {
            n -= 1;
            // SAFETY: n starts from self.len() / 2 and goes down to 0.
            unsafe { self.sift_down(n) };
        }
    }
}

impl<T, C> BinaryHeap<T, C> {
    /// Returns the length of the binary heap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the binary heap is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops all items from the binary heap.
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

impl<T: fmt::Debug, C> fmt::Debug for BinaryHeap<T, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.data.iter()).finish()
    }
}

impl<T: Clone, C: Clone> Clone for BinaryHeap<T, C> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            cmp: self.cmp.clone(),
        }
    }
}

/// Structure wrapping a mutable reference to the greatest item on a
/// `BinaryHeap`.
pub struct PeekMut<'a, T: 'a, C: 'a + Compare<T>> {
    heap: &'a mut BinaryHeap<T, C>,
    sift: bool,
}

impl<T: fmt::Debug, C: Compare<T>> fmt::Debug for PeekMut<'_, T, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PeekMut").field(&self.heap.data[0]).finish()
    }
}

impl<T, C: Compare<T>> Drop for PeekMut<'_, T, C> {
    fn drop(&mut self) {
        if self.sift {
            // SAFETY: PeekMut is only instantiated for non-empty heaps.
            unsafe { self.heap.sift_down(0) };
        }
    }
}

impl<T, C: Compare<T>> Deref for PeekMut<'_, T, C> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFETY: PeekMut is only instantiated for non-empty heaps
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T, C: Compare<T>> DerefMut for PeekMut<'_, T, C> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());
        self.sift = true;
        // SAFETY: PeekMut is only instantiated for non-empty heaps
        unsafe { self.heap.data.get_unchecked_mut(0) }
    }
}

impl<'a, T, C: Compare<T>> PeekMut<'a, T, C> {
    /// Removes the peeked value from the heap and returns it.
    pub fn pop(mut this: Self) -> T {
        let value = this.heap.pop().unwrap();
        this.sift = false;
        value
    }
}

/// Hole represents a hole in a slice i.e., an index without valid value
/// (because it was moved from or duplicated).
struct Hole<'a, T: 'a> {
    data: &'a mut [T],
    elt: ManuallyDrop<T>,
    pos: usize,
}

impl<'a, T> Hole<'a, T> {
    /// Create a new `Hole` at index `pos`.
    ///
    /// # Safety
    ///
    /// `pos` must be within the data slice.
    #[inline]
    unsafe fn new(data: &'a mut [T], pos: usize) -> Self {
        debug_assert!(pos < data.len());
        // SAFETY: pos should be inside the slice
        let elt = unsafe { ptr::read(data.get_unchecked(pos)) };
        Hole {
            data,
            elt: ManuallyDrop::new(elt),
            pos,
        }
    }

    #[inline]
    fn pos(&self) -> usize {
        self.pos
    }

    /// Returns a reference to the element removed.
    #[inline]
    fn element(&self) -> &T {
        &self.elt
    }

    /// Returns a reference to the element at `index`.
    ///
    /// # Safety
    ///
    /// `index` must be within the data slice and not equal to pos.
    #[inline]
    unsafe fn get(&self, index: usize) -> &T {
        debug_assert!(index != self.pos);
        debug_assert!(index < self.data.len());
        unsafe { self.data.get_unchecked(index) }
    }

    /// Move hole to new location.
    ///
    /// # Safety
    ///
    /// `index` must be within the data slice and not equal to pos.
    #[inline]
    unsafe fn move_to(&mut self, index: usize) {
        debug_assert!(index != self.pos);
        debug_assert!(index < self.data.len());
        unsafe {
            let ptr = self.data.as_mut_ptr();
            let index_ptr: *const _ = ptr.add(index);
            let hole_ptr = ptr.add(self.pos);
            ptr::copy_nonoverlapping(index_ptr, hole_ptr, 1);
        }
        self.pos = index;
    }
}

impl<T> Drop for Hole<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Fill the hole again
        unsafe {
            let pos = self.pos;
            ptr::copy_nonoverlapping(
                ptr::from_ref(&*self.elt),
                self.data.get_unchecked_mut(pos),
                1,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use rstest::rstest;

    use super::*;

    struct MaxComparator;

    impl Compare<i32> for MaxComparator {
        fn compare(&self, a: &i32, b: &i32) -> Ordering {
            a.cmp(b)
        }
    }

    struct MinComparator;

    impl Compare<i32> for MinComparator {
        fn compare(&self, a: &i32, b: &i32) -> Ordering {
            b.cmp(a)
        }
    }

    #[rstest]
    fn test_max_heap() {
        let mut heap = BinaryHeap::from_vec_cmp(vec![], MaxComparator);
        heap.push(3);
        heap.push(1);
        heap.push(5);

        assert_eq!(heap.pop(), Some(5));
        assert_eq!(heap.pop(), Some(3));
        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), None);
    }

    #[rstest]
    fn test_min_heap() {
        let mut heap = BinaryHeap::from_vec_cmp(vec![], MinComparator);
        heap.push(3);
        heap.push(1);
        heap.push(5);

        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), Some(3));
        assert_eq!(heap.pop(), Some(5));
        assert_eq!(heap.pop(), None);
    }

    #[rstest]
    fn test_peek_mut() {
        let mut heap = BinaryHeap::from_vec_cmp(vec![1, 5, 2], MaxComparator);

        if let Some(mut val) = heap.peek_mut() {
            *val = 0;
        }

        assert_eq!(heap.pop(), Some(2));
    }

    #[rstest]
    fn test_peek_mut_pop() {
        let mut heap = BinaryHeap::from_vec_cmp(vec![1, 5, 2], MaxComparator);

        if let Some(val) = heap.peek_mut() {
            let popped = PeekMut::pop(val);
            assert_eq!(popped, 5);
        }

        assert_eq!(heap.pop(), Some(2));
        assert_eq!(heap.pop(), Some(1));
    }

    #[rstest]
    fn test_clear() {
        let mut heap = BinaryHeap::from_vec_cmp(vec![1, 2, 3], MaxComparator);
        assert!(!heap.is_empty());

        heap.clear();
        assert!(heap.is_empty());
        assert_eq!(heap.len(), 0);
    }

    #[rstest]
    fn test_from_vec() {
        let heap = BinaryHeap::from_vec_cmp(vec![3, 1, 4, 1, 5, 9, 2, 6], MaxComparator);
        let mut sorted = Vec::new();
        let mut heap = heap;
        while let Some(v) = heap.pop() {
            sorted.push(v);
        }
        assert_eq!(sorted, vec![9, 6, 5, 4, 3, 2, 1, 1]);
    }
}
