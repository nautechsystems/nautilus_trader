// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{sync::Arc, vec::IntoIter};

use binary_heap_plus::{BinaryHeap, PeekMut};
use compare::Compare;
use futures::{Stream, StreamExt};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};

pub struct EagerStream<T> {
    rx: Receiver<T>,
    task: JoinHandle<()>,
    runtime: Arc<Runtime>,
}

impl<T> EagerStream<T> {
    pub fn from_stream_with_runtime<S>(stream: S, runtime: Arc<Runtime>) -> Self
    where
        S: Stream<Item = T> + Send + 'static,
        T: Send + 'static,
    {
        let _guard = runtime.enter();
        let (tx, rx) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            stream
                .for_each(|item| async {
                    let _ = tx.send(item).await;
                })
                .await;
        });

        Self { rx, task, runtime }
    }
}

impl<T> Iterator for EagerStream<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.runtime.block_on(self.rx.recv())
    }
}

impl<T> Drop for EagerStream<T> {
    fn drop(&mut self) {
        self.rx.close();
        self.task.abort();
    }
}

// TODO: Investigate implementing Iterator for ElementBatchIter
// to reduce next element duplication. May be difficult to make it peekable.
pub struct ElementBatchIter<I, T>
where
    I: Iterator<Item = IntoIter<T>>,
{
    pub item: T,
    batch: I::Item,
    iter: I,
}

impl<I, T> ElementBatchIter<I, T>
where
    I: Iterator<Item = IntoIter<T>>,
{
    fn new_from_iter(mut iter: I) -> Option<Self> {
        loop {
            match iter.next() {
                Some(mut batch) => match batch.next() {
                    Some(item) => {
                        break Some(Self { item, batch, iter });
                    }
                    None => continue,
                },
                None => break None,
            }
        }
    }
}

pub struct KMerge<I, T, C>
where
    I: Iterator<Item = IntoIter<T>>,
{
    heap: BinaryHeap<ElementBatchIter<I, T>, C>,
}

impl<I, T, C> KMerge<I, T, C>
where
    I: Iterator<Item = IntoIter<T>>,
    C: Compare<ElementBatchIter<I, T>>,
{
    /// Creates a new [`KMerge`] instance.
    pub fn new(cmp: C) -> Self {
        Self {
            heap: BinaryHeap::from_vec_cmp(Vec::new(), cmp),
        }
    }

    pub fn push_iter(&mut self, s: I) {
        if let Some(heap_elem) = ElementBatchIter::new_from_iter(s) {
            self.heap.push(heap_elem);
        }
    }

    pub fn clear(&mut self) {
        self.heap.clear();
    }
}

impl<I, T, C> Iterator for KMerge<I, T, C>
where
    I: Iterator<Item = IntoIter<T>>,
    C: Compare<ElementBatchIter<I, T>>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.heap.peek_mut() {
            Some(mut heap_elem) => {
                // Get next element from batch
                match heap_elem.batch.next() {
                    // Swap current heap element with new element
                    // return the old element
                    Some(mut item) => {
                        std::mem::swap(&mut item, &mut heap_elem.item);
                        Some(item)
                    }
                    // Otherwise get the next batch and the element from it
                    // Unless the underlying iterator is exhausted
                    None => loop {
                        if let Some(mut batch) = heap_elem.iter.next() {
                            match batch.next() {
                                Some(mut item) => {
                                    heap_elem.batch = batch;
                                    std::mem::swap(&mut item, &mut heap_elem.item);
                                    break Some(item);
                                }
                                // Get next batch from iterator
                                None => continue,
                            }
                        } else {
                            let ElementBatchIter {
                                item,
                                batch: _,
                                iter: _,
                            } = PeekMut::pop(heap_elem);
                            break Some(item);
                        }
                    },
                }
            }
            None => None,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    struct OrdComparator;
    impl<S> Compare<ElementBatchIter<S, i32>> for OrdComparator
    where
        S: Iterator<Item = IntoIter<i32>>,
    {
        fn compare(
            &self,
            l: &ElementBatchIter<S, i32>,
            r: &ElementBatchIter<S, i32>,
        ) -> std::cmp::Ordering {
            // Max heap ordering must be reversed
            l.item.cmp(&r.item).reverse()
        }
    }

    impl<S> Compare<ElementBatchIter<S, u64>> for OrdComparator
    where
        S: Iterator<Item = IntoIter<u64>>,
    {
        fn compare(
            &self,
            l: &ElementBatchIter<S, u64>,
            r: &ElementBatchIter<S, u64>,
        ) -> std::cmp::Ordering {
            // Max heap ordering must be reversed
            l.item.cmp(&r.item).reverse()
        }
    }

    #[rstest]
    fn test1() {
        let iter_a = vec![vec![1, 2, 3].into_iter(), vec![7, 8, 9].into_iter()].into_iter();
        let iter_b = vec![vec![4, 5, 6].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[rstest]
    fn test2() {
        let iter_a = vec![vec![1, 2, 6].into_iter(), vec![7, 8, 9].into_iter()].into_iter();
        let iter_b = vec![vec![3, 4, 5, 6].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 6, 7, 8, 9]);
    }

    #[rstest]
    fn test3() {
        let iter_a = vec![vec![1, 4, 7].into_iter(), vec![24, 35, 56].into_iter()].into_iter();
        let iter_b = vec![vec![2, 4, 8].into_iter()].into_iter();
        let iter_c = vec![vec![3, 5, 9].into_iter(), vec![12, 12, 90].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);
        kmerge.push_iter(iter_c);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(
            values,
            vec![1, 2, 3, 4, 4, 5, 7, 8, 9, 12, 12, 24, 35, 56, 90]
        );
    }

    #[rstest]
    fn test5() {
        let iter_a = vec![
            vec![1, 3, 5].into_iter(),
            vec![].into_iter(),
            vec![7, 9, 11].into_iter(),
        ]
        .into_iter();
        let iter_b = vec![vec![2, 4, 6].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 9, 11]);
    }

    #[derive(Debug, Clone)]
    struct SortedNestedVec(Vec<Vec<u64>>);

    /// Strategy to generate nested vectors where each inner vector is sorted.
    fn sorted_nested_vec_strategy() -> impl Strategy<Value = SortedNestedVec> {
        // Generate a vector of u64 values, then split into sorted chunks
        prop::collection::vec(any::<u64>(), 0..=100).prop_flat_map(|mut flat_vec| {
            flat_vec.sort_unstable();

            // Generate chunk sizes that will split the sorted vector
            let total_len = flat_vec.len();
            if total_len == 0 {
                return Just(SortedNestedVec(vec![vec![]])).boxed();
            }

            // Generate random chunk boundaries
            prop::collection::vec(0..=total_len, 0..=10)
                .prop_map(move |mut boundaries| {
                    boundaries.push(0);
                    boundaries.push(total_len);
                    boundaries.sort_unstable();
                    boundaries.dedup();

                    let mut nested_vec = Vec::new();
                    for window in boundaries.windows(2) {
                        let start = window[0];
                        let end = window[1];
                        nested_vec.push(flat_vec[start..end].to_vec());
                    }

                    SortedNestedVec(nested_vec)
                })
                .boxed()
        })
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Property-based testing
    ////////////////////////////////////////////////////////////////////////////////

    proptest! {
        /// Property: K-way merge should produce the same result as sorting all data together
        #[rstest]
        fn prop_kmerge_equivalent_to_sort(
            all_data in prop::collection::vec(sorted_nested_vec_strategy(), 0..=10)
        ) {
            let mut kmerge: KMerge<_, u64, _> = KMerge::new(OrdComparator);

            let copy_data = all_data.clone();
            for stream in copy_data {
                let input = stream.0.into_iter().map(std::iter::IntoIterator::into_iter);
                kmerge.push_iter(input);
            }
            let merged_data: Vec<u64> = kmerge.collect();

            let mut sorted_data: Vec<u64> = all_data
                .into_iter()
                .flat_map(|stream| stream.0.into_iter().flatten())
                .collect();
            sorted_data.sort_unstable();

            prop_assert_eq!(merged_data.len(), sorted_data.len(), "Lengths should be equal");
            prop_assert_eq!(merged_data, sorted_data, "Merged data should equal sorted data");
        }

        /// Property: K-way merge should preserve sortedness when inputs are sorted
        #[rstest]
        fn prop_kmerge_preserves_sort_order(
            all_data in prop::collection::vec(sorted_nested_vec_strategy(), 1..=5)
        ) {
            let mut kmerge: KMerge<_, u64, _> = KMerge::new(OrdComparator);

            for stream in all_data {
                let input = stream.0.into_iter().map(std::iter::IntoIterator::into_iter);
                kmerge.push_iter(input);
            }
            let merged_data: Vec<u64> = kmerge.collect();

            // Check that the merged data is sorted
            for window in merged_data.windows(2) {
                prop_assert!(window[0] <= window[1], "Merged data should be sorted");
            }
        }

        /// Property: Empty iterators should not affect the merge result
        #[rstest]
        fn prop_kmerge_handles_empty_iterators(
            data in sorted_nested_vec_strategy(),
            empty_count in 0usize..=5
        ) {
            let mut kmerge_with_empty: KMerge<_, u64, _> = KMerge::new(OrdComparator);
            let mut kmerge_without_empty: KMerge<_, u64, _> = KMerge::new(OrdComparator);

            // Add the actual data to both merges
            let input_with_empty = data.0.clone().into_iter().map(std::iter::IntoIterator::into_iter);
            let input_without_empty = data.0.into_iter().map(std::iter::IntoIterator::into_iter);

            kmerge_with_empty.push_iter(input_with_empty);
            kmerge_without_empty.push_iter(input_without_empty);

            // Add empty iterators to the first merge
            for _ in 0..empty_count {
                let empty_vec: Vec<Vec<u64>> = vec![];
                let empty_input = empty_vec.into_iter().map(std::iter::IntoIterator::into_iter);
                kmerge_with_empty.push_iter(empty_input);
            }

            let result_with_empty: Vec<u64> = kmerge_with_empty.collect();
            let result_without_empty: Vec<u64> = kmerge_without_empty.collect();

            prop_assert_eq!(result_with_empty, result_without_empty, "Empty iterators should not affect result");
        }
    }
}
