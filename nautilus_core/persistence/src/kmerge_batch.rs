// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::vec::IntoIter;

use binary_heap_plus::{BinaryHeap, PeekMut};
use compare::Compare;
use futures::{Stream, StreamExt};
use pyo3_asyncio::tokio::get_runtime;
use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};

pub struct EagerStream<T> {
    rx: Receiver<T>,
    task: JoinHandle<()>,
}

impl<T> EagerStream<T> {
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            stream
                .for_each(|item| async {
                    let _ = tx.send(item).await;
                })
                .await;
        });

        EagerStream { rx, task }
    }
}

impl<T> Iterator for EagerStream<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let rt = get_runtime();
        match self.task.is_finished() {
            false => rt.block_on(self.rx.recv()),
            true => None,
        }
    }
}

impl<T> Drop for EagerStream<T> {
    fn drop(&mut self) {
        self.task.abort();
        self.rx.close();
    }
}

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
        let next_batch = iter.next();
        if let Some(mut batch) = next_batch {
            batch.next().map(|item| Self { item, batch, iter })
        } else {
            None
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
                    // swap current heap element with new element
                    // return the old element
                    Some(mut item) => {
                        std::mem::swap(&mut item, &mut heap_elem.item);
                        Some(item)
                    }
                    // Otherwise get the next batch and the element from it
                    // Unless the underlying iterator is exhausted
                    None => loop {
                        match heap_elem.iter.next() {
                            Some(mut batch) => match batch.next() {
                                Some(mut item) => {
                                    std::mem::swap(&mut item, &mut heap_elem.item);
                                    heap_elem.batch = batch;
                                    break Some(item);
                                }
                                // get next batch from iterator
                                None => continue,
                            },
                            // iterator has no more batches return current element
                            // and pop the heap element
                            None => {
                                let ElementBatchIter {
                                    item,
                                    batch: _,
                                    iter: _,
                                } = PeekMut::pop(heap_elem);
                                break Some(item);
                            }
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

    #[test]
    fn test1() {
        let iter_a = vec![vec![1, 2, 3].into_iter(), vec![7, 8, 9].into_iter()].into_iter();
        let iter_b = vec![vec![4, 5, 6].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test2() {
        let iter_a = vec![vec![1, 2, 6].into_iter(), vec![7, 8, 9].into_iter()].into_iter();
        let iter_b = vec![vec![3, 4, 5, 6].into_iter()].into_iter();
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_iter(iter_a);
        kmerge.push_iter(iter_b);

        let values: Vec<i32> = kmerge.collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 6, 7, 8, 9]);
    }

    #[test]
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
}
