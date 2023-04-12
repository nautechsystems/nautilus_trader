use std::{task::Poll, vec::IntoIter};

use binary_heap_plus::BinaryHeap;
use compare::Compare;
use futures::{ready, FutureExt, Stream, StreamExt};
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
        // poll next batch from stream and get next item from the batch
        // and add the new element to the heap. No new element is added
        // to the heap if the stream is empty. Keep polling the stream
        // for a batch that is non-empty.
        let next_batch = stream.next().await;
        if let Some(mut batch) = next_batch {
            batch.next().map(|next_item| PeekElementBatchStream {
                item: next_item,
                batch,
                stream,
            })
        } else {
            // stream is empty, no new batch
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
    S: Stream<Item = IntoIter<I>> + Unpin,
    C: Compare<PeekElementBatchStream<S, I>>,
{
    pub fn new(cmp: C) -> Self {
        Self {
            heap: BinaryHeap::from_vec_cmp(Vec::new(), cmp),
        }
    }

    pub async fn push_stream(&mut self, s: S) {
        if let Some(heap_elem) = PeekElementBatchStream::new_from_stream(s).await {
            self.heap.push(heap_elem)
        }
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
            // next element from batch
            if let Some(next_item) = batch.next() {
                this.heap.push(PeekElementBatchStream {
                    item: next_item,
                    batch,
                    stream,
                })
            }
            // batch is empty create new heap element from stream
            else if let Some(heap_elem) =
                ready!(Box::pin(PeekElementBatchStream::new_from_stream(stream)).poll_unpin(cx))
            {
                this.heap.push(heap_elem);
            }
            Poll::Ready(Some(item))
        } else {
            // heap is empty
            Poll::Ready(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream::iter;

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
            // it is a max heap so the ordering must be reversed
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
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 7, 8, 9])
    }

    #[tokio::test]
    async fn test2() {
        let stream_a = iter(vec![vec![1, 2, 6].into_iter(), vec![7, 8, 9].into_iter()]);
        let stream_b = iter(vec![vec![3, 4, 5, 6].into_iter()]);
        let mut kmerge: KMerge<_, i32, _> = KMerge::new(OrdComparator);
        kmerge.push_stream(stream_a).await;
        kmerge.push_stream(stream_b).await;

        let values: Vec<i32> = kmerge.collect().await;
        assert_eq!(values, vec![1, 2, 3, 4, 5, 6, 6, 7, 8, 9])
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
        )
    }
}
