// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Shared batch representations for model data.
//!
//! [`BatchView`] shares a [`Vec<T>`] allocation and exposes a contiguous range without copying its
//! elements when cloned or sliced. Each view retains the full allocation until its final reference
//! is dropped.
//!
//! [`DataBatch`] preserves concrete storage for supported data families and exposes individual
//! items as [`DataRef`] values for heterogeneous dispatch.

use std::{
    ops::{Deref, Range},
    sync::Arc,
};

use super::{
    Bar, DataRef, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
    MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick,
    TradeTick,
};
#[cfg(feature = "defi")]
use crate::defi::DefiData;

/// Range view over a shared, immutable [`Vec<T>`].
///
/// Cloning or slicing a view is cheap, but retains the full backing allocation until all views are
/// dropped.
#[derive(Debug)]
#[expect(
    clippy::rc_buffer,
    reason = "Batch views preserve Vec ownership so consumers can recover the allocation"
)]
pub struct BatchView<T> {
    data: Arc<Vec<T>>,
    range: Range<usize>,
}

impl<T> Clone for BatchView<T> {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            range: self.range.clone(),
        }
    }
}

impl<T> BatchView<T> {
    /// Creates a new [`BatchView`] instance.
    ///
    /// `range` indexes the backing allocation.
    ///
    /// # Panics
    ///
    /// Panics if `range.start > range.end` or `range.end > data.len()`.
    #[must_use]
    pub fn new(data: Arc<Vec<T>>, range: Range<usize>) -> Self {
        assert!(
            range.start <= range.end,
            "batch view range start exceeds end"
        );
        assert!(
            range.end <= data.len(),
            "batch view range exceeds data length"
        );
        Self { data, range }
    }

    /// Creates a view over all items in `data`.
    #[must_use]
    pub fn full(data: Arc<Vec<T>>) -> Self {
        let len = data.len();
        Self {
            data,
            range: 0..len,
        }
    }

    /// Returns the entire shared backing allocation, including items outside this view's range.
    #[must_use]
    pub fn arc(&self) -> &Arc<Vec<T>> {
        &self.data
    }

    /// Returns the range covered by this view in the backing allocation.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns a sub-view over `[start, end)`, relative to this view's start.
    ///
    /// # Panics
    ///
    /// Panics if `start > end` or `end` exceeds this view's length.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        assert!(start <= end, "batch slice range start exceeds end");
        assert!(end <= self.len(), "batch slice range exceeds view length");
        Self {
            data: Arc::clone(&self.data),
            range: (self.range.start + start)..(self.range.start + end),
        }
    }
}

impl<T> From<Vec<T>> for BatchView<T> {
    fn from(data: Vec<T>) -> Self {
        Self::full(Arc::new(data))
    }
}

impl<T> From<Arc<Vec<T>>> for BatchView<T> {
    fn from(data: Arc<Vec<T>>) -> Self {
        Self::full(data)
    }
}

impl<T> AsRef<[T]> for BatchView<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T> Deref for BatchView<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.data[self.range.clone()]
    }
}

/// Shared batch for a statically modeled [`crate::data::Data`] family.
///
/// [`crate::data::CustomData`] is excluded because one collection may contain multiple logical
/// [`crate::data::DataType`] values.
#[derive(Clone, Debug)]
pub enum DataBatch {
    BookDelta(BatchView<OrderBookDelta>),
    BookDeltas(BatchView<OrderBookDeltas>),
    BookDepth10(BatchView<OrderBookDepth10>),
    Quote(BatchView<QuoteTick>),
    Trade(BatchView<TradeTick>),
    Bar(BatchView<Bar>),
    MarkPrice(BatchView<MarkPriceUpdate>),
    IndexPrice(BatchView<IndexPriceUpdate>),
    FundingRate(BatchView<FundingRateUpdate>),
    OptionGreeks(BatchView<OptionGreeks>),
    InstrumentStatus(BatchView<InstrumentStatus>),
    InstrumentClose(BatchView<InstrumentClose>),
    #[cfg(feature = "defi")]
    Defi(BatchView<DefiData>),
}

impl DataBatch {
    /// Returns the number of items in this batch.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::BookDelta(data) => data.len(),
            Self::BookDeltas(data) => data.len(),
            Self::BookDepth10(data) => data.len(),
            Self::Quote(data) => data.len(),
            Self::Trade(data) => data.len(),
            Self::Bar(data) => data.len(),
            Self::MarkPrice(data) => data.len(),
            Self::IndexPrice(data) => data.len(),
            Self::FundingRate(data) => data.len(),
            Self::OptionGreeks(data) => data.len(),
            Self::InstrumentStatus(data) => data.len(),
            Self::InstrumentClose(data) => data.len(),
            #[cfg(feature = "defi")]
            Self::Defi(data) => data.len(),
        }
    }

    /// Returns whether this batch contains no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a borrowed view of the item at `index`, or `None` if `index` is outside this batch.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<DataRef<'_>> {
        match self {
            Self::BookDelta(data) => data.get(index).map(DataRef::BookDelta),
            Self::BookDeltas(data) => data.get(index).map(DataRef::BookDeltas),
            Self::BookDepth10(data) => data.get(index).map(DataRef::BookDepth10),
            Self::Quote(data) => data.get(index).map(DataRef::Quote),
            Self::Trade(data) => data.get(index).map(DataRef::Trade),
            Self::Bar(data) => data.get(index).map(DataRef::Bar),
            Self::MarkPrice(data) => data.get(index).map(DataRef::MarkPrice),
            Self::IndexPrice(data) => data.get(index).map(DataRef::IndexPrice),
            Self::FundingRate(data) => data.get(index).map(DataRef::FundingRate),
            Self::OptionGreeks(data) => data.get(index).map(DataRef::OptionGreeks),
            Self::InstrumentStatus(data) => data.get(index).map(DataRef::InstrumentStatus),
            Self::InstrumentClose(data) => data.get(index).map(DataRef::InstrumentClose),
            #[cfg(feature = "defi")]
            Self::Defi(data) => data.get(index).map(DataRef::Defi),
        }
    }

    /// Returns a shared sub-batch over `[start, end)`, relative to this batch's start.
    ///
    /// # Panics
    ///
    /// Panics if `start > end` or `end` exceeds this batch's length.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        match self {
            Self::BookDelta(data) => Self::BookDelta(data.slice(start, end)),
            Self::BookDeltas(data) => Self::BookDeltas(data.slice(start, end)),
            Self::BookDepth10(data) => Self::BookDepth10(data.slice(start, end)),
            Self::Quote(data) => Self::Quote(data.slice(start, end)),
            Self::Trade(data) => Self::Trade(data.slice(start, end)),
            Self::Bar(data) => Self::Bar(data.slice(start, end)),
            Self::MarkPrice(data) => Self::MarkPrice(data.slice(start, end)),
            Self::IndexPrice(data) => Self::IndexPrice(data.slice(start, end)),
            Self::FundingRate(data) => Self::FundingRate(data.slice(start, end)),
            Self::OptionGreeks(data) => Self::OptionGreeks(data.slice(start, end)),
            Self::InstrumentStatus(data) => Self::InstrumentStatus(data.slice(start, end)),
            Self::InstrumentClose(data) => Self::InstrumentClose(data.slice(start, end)),
            #[cfg(feature = "defi")]
            Self::Defi(data) => Self::Defi(data.slice(start, end)),
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;

    struct NonClone(i32);

    #[rstest]
    fn test_batch_view_clone_does_not_require_clone_elements() {
        let view = BatchView::from(vec![NonClone(10)]);

        let cloned = view.clone();

        assert!(Arc::ptr_eq(view.arc(), cloned.arc()));
        assert_eq!(cloned[0].0, 10);
    }

    #[rstest]
    fn test_batch_view_slice_shares_backing_after_source_drop() {
        let view = BatchView::new(Arc::new(vec![10, 20, 30, 40]), 1..4);
        let slice = view.slice(1, 3);

        assert_eq!(Arc::strong_count(view.arc()), 2);
        assert_eq!(slice.as_ref(), &[30, 40]);
        assert_eq!(slice.range(), 2..4);

        drop(view);

        assert_eq!(Arc::strong_count(slice.arc()), 1);
        assert_eq!(slice.as_ref(), &[30, 40]);
    }

    #[rstest]
    #[case(2, 1)]
    #[case(0, 3)]
    #[should_panic(expected = "batch view")]
    fn test_batch_view_new_rejects_invalid_range(#[case] start: usize, #[case] end: usize) {
        let _ = BatchView::new(Arc::new(vec![10, 20]), start..end);
    }

    #[rstest]
    #[case(2, 1)]
    #[case(0, 3)]
    #[should_panic(expected = "batch slice")]
    fn test_batch_view_slice_rejects_invalid_range(#[case] start: usize, #[case] end: usize) {
        let _ = BatchView::from(vec![10, 20]).slice(start, end);
    }

    #[rstest]
    fn test_data_batch_slice_preserves_variant_and_order() {
        let first = QuoteTick {
            ts_init: UnixNanos::from(7),
            ..QuoteTick::default()
        };
        let second = QuoteTick {
            ts_init: UnixNanos::from(3),
            ..QuoteTick::default()
        };
        let third = QuoteTick {
            ts_init: UnixNanos::from(11),
            ..QuoteTick::default()
        };
        let batch = DataBatch::Quote(vec![first, second, third].into());

        let slice = batch.slice(1, 3);

        assert_eq!(slice.len(), 2);
        assert!(!slice.is_empty());
        assert!(matches!(
            slice.get(0),
            Some(DataRef::Quote(quote)) if quote.ts_init == UnixNanos::from(3)
        ));
        assert!(matches!(
            slice.get(1),
            Some(DataRef::Quote(quote)) if quote.ts_init == UnixNanos::from(11)
        ));
    }

    #[rstest]
    #[should_panic(expected = "batch slice range exceeds view length")]
    fn test_data_batch_slice_rejects_out_of_bounds_range() {
        let _ = DataBatch::Quote(vec![QuoteTick::default()].into()).slice(0, 2);
    }
}
