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

use std::{task::Poll, vec::IntoIter};

use binary_heap_plus::BinaryHeap;
use compare::Compare;
use futures::{future::join_all, ready, FutureExt, Stream, StreamExt};
use pin_project_lite::pin_project;

pub struct PeekElementBatchStream<S, I>
where
    S: Stream<Item = IntoIter<I>>,
{
    pub item: I,
    batch: S::Item,
    stream: S,
}

impl<S, I> PeekElementBatchStream<S, I>
where
    S: Stream<Item = IntoIter<I>> + Unpin,
{
    async fn new_from_stream(mut stream: S) -> Option<Self> {
        // Poll next batch from stream and get next item from the batch
        // and add the new element to the heap. No new element is added
        // to the heap if the stream is empty. Keep polling the stream
        // for a batch that is non-empty.
        let next_batch = stream.next().await;
        if let Some(mut batch) = next_batch {
            batch.next().map(|next_item| Self {
                item: next_item,
                batch,
                stream,
            })
        } else {
            // Stream is empty, no new batch
            None
        }
    }
}

pin_project! {
    pub struct KMerge<S, I, C>
    where
        S: Stream<Item = IntoIter<I>>,
    {
        heap: BinaryHeap<PeekElementBatchStream<S, I>, C>,
    }
}

impl<S, I, C> KMerge<S, I, C>
where
    S: Stream<Item = IntoIter<I>> + Unpin + Send + 'static,
    C: Compare<PeekElementBatchStream<S, I>>,
    I: Send + 'static,
{
    pub fn new(cmp: C) -> Self {
        Self {
            heap: BinaryHeap::from_vec_cmp(Vec::new(), cmp),
        }
    }

    #[cfg(test)]
    async fn push_stream(&mut self, s: S) {
        if let Some(heap_elem) = PeekElementBatchStream::new_from_stream(s).await {
            self.heap.push(heap_elem);
        }
    }

    /// Push elements on to the heap
    ///
    /// Takes a Iterator of Streams. It concurrently converts all the streams
    /// to heap elements and then pushes them onto the heap.
    pub async fn push_iter_stream<L>(&mut self, l: L)
    where
        L: Iterator<Item = S>,
    {
        let tasks = l.map(|batch| {
            tokio::spawn(async move { PeekElementBatchStream::new_from_stream(batch).await })
        });

        join_all(tasks)
            .await
            .into_iter()
            .for_each(|heap_elem| match heap_elem {
                Ok(Some(heap_elem)) => self.heap.push(heap_elem),
                Ok(None) => (),
                Err(e) => panic!("Failed to create heap element because of error: {e}"),
            });
    }
}

impl<S, I, C> Stream for KMerge<S, I, C>
where
    S: Stream<Item = IntoIter<I>> + Unpin,
    C: Compare<PeekElementBatchStream<S, I>>,
{
    type Item = I;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        if let Some(PeekElementBatchStream {
            item,
            mut batch,
            stream,
        }) = this.heap.pop()
        {
            // Next element from batch
            if let Some(next_item) = batch.next() {
                this.heap.push(PeekElementBatchStream {
                    item: next_item,
                    batch,
                    stream,
                });
            }
            // Batch is empty create new heap element from stream
            else if let Some(heap_elem) =
                ready!(Box::pin(PeekElementBatchStream::new_from_stream(stream)).poll_unpin(cx))
            {
                this.heap.push(heap_elem);
            }
            Poll::Ready(Some(item))
        } else {
            // Heap is empty
            Poll::Ready(None)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use futures::stream::iter;

    use super::*;

    struct OrdComparator;
    impl<S> Compare<PeekElementBatchStream<S, i32>> for OrdComparator
    where
        S: Stream<Item = IntoIter<i32>>,
    {
        fn compare(
            &self,
            l: &PeekElementBatchStream<S, i32>,
            r: &PeekElementBatchStream<S, i32>,
        ) -> std::cmp::Ordering {
            // Max heap ordering must be reversed
            l.item.cmp(&r.item).reverse()
        }
    }

    #[tokio::test]
    async fn test1() {
        let stream_a = iter(vec![vec![1, 2, 3].into_iter(), vec![7, 8, 9].into_iter()]);
        let stream_b = iter(vec![vec![4, 5, 6].into_iter()]);
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_stream(stream_a).await;
        kmerge.push_stream(stream_b).await;

        let values: Vec<i32> = kmerge.collect().await;
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[tokio::test]
    async fn test2() {
        let stream_a = iter(vec![vec![1, 2, 6].into_iter(), vec![7, 8, 9].into_iter()]);
        let stream_b = iter(vec![vec![3, 4, 5, 6].into_iter()]);
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_stream(stream_a).await;
        kmerge.push_stream(stream_b).await;

        let values: Vec<i32> = kmerge.collect().await;
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 6, 7, 8, 9]);
    }

    #[tokio::test]
    async fn test3() {
        let stream_a = iter(vec![
            vec![1, 4, 7].into_iter(),
            vec![24, 35, 56].into_iter(),
        ]);
        let stream_b = iter(vec![vec![2, 4, 8].into_iter()]);
        let stream_c = iter(vec![
            vec![3, 5, 9].into_iter(),
            vec![12, 12, 90].into_iter(),
        ]);
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_stream(stream_a).await;
        kmerge.push_stream(stream_b).await;
        kmerge.push_stream(stream_c).await;

        let values: Vec<i32> = kmerge.collect().await;
        assert_eq!(
            values,
            vec![1, 2, 3, 4, 4, 5, 7, 8, 9, 12, 12, 24, 35, 56, 90]
        );
    }
}
