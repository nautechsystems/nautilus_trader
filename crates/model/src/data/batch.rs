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

    /// Returns the items in this view's range for in-place modification.
    ///
    /// Clones the backing allocation first when other views share it, so an unshared batch is
    /// modified without copying.
    pub fn make_mut(&mut self) -> &mut [T]
    where
        T: Clone,
    {
        &mut Arc::make_mut(&mut self.data)[self.range.clone()]
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

macro_rules! impl_data_batch_from_vec {
    ($variant:ident, $type:ty) => {
        impl From<Vec<$type>> for DataBatch {
            fn from(data: Vec<$type>) -> Self {
                Self::$variant(BatchView::from(data))
            }
        }
    };
}

impl_data_batch_from_vec!(BookDelta, OrderBookDelta);
impl_data_batch_from_vec!(BookDeltas, OrderBookDeltas);
impl_data_batch_from_vec!(BookDepth10, OrderBookDepth10);
impl_data_batch_from_vec!(Quote, QuoteTick);
impl_data_batch_from_vec!(Trade, TradeTick);
impl_data_batch_from_vec!(Bar, Bar);
impl_data_batch_from_vec!(MarkPrice, MarkPriceUpdate);
impl_data_batch_from_vec!(IndexPrice, IndexPriceUpdate);
impl_data_batch_from_vec!(FundingRate, FundingRateUpdate);
impl_data_batch_from_vec!(OptionGreeks, OptionGreeks);
impl_data_batch_from_vec!(InstrumentStatus, InstrumentStatus);
impl_data_batch_from_vec!(InstrumentClose, InstrumentClose);
#[cfg(feature = "defi")]
impl_data_batch_from_vec!(Defi, DefiData);

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        data::stubs::{
            stub_bar, stub_delta, stub_deltas, stub_depth10, stub_instrument_close,
            stub_instrument_status, stub_trade_ethusdt_buy,
        },
        identifiers::InstrumentId,
        types::Price,
    };
    #[cfg(feature = "defi")]
    use crate::{
        defi::{
            data::block::BlockPosition,
            pool_analysis::snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
        },
        identifiers::{Symbol, Venue},
    };

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

    #[rstest]
    fn test_batch_view_make_mut_reuses_unshared_backing() {
        let mut view = BatchView::from(vec![3, 1, 2]);
        let backing = Arc::as_ptr(view.arc());

        view.make_mut().sort_unstable();

        assert!(std::ptr::eq(Arc::as_ptr(view.arc()), backing));
        assert_eq!(view.as_ref(), &[1, 2, 3]);
    }

    #[rstest]
    fn test_batch_view_make_mut_clones_shared_backing_within_range() {
        let source = BatchView::new(Arc::new(vec![9, 3, 1, 2]), 1..4);
        let mut view = source.clone();

        view.make_mut().sort_unstable();

        assert!(!Arc::ptr_eq(source.arc(), view.arc()));
        assert_eq!(source.as_ref(), &[3, 1, 2]);
        assert_eq!(view.as_ref(), &[1, 2, 3]);
        assert_eq!(view.arc().as_slice(), &[9, 1, 2, 3]);
    }

    #[rstest]
    fn test_data_batch_from_vec_uses_matching_variant() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mark_price = MarkPriceUpdate::new(
            instrument_id,
            Price::from("100.10"),
            UnixNanos::from(7),
            UnixNanos::from(8),
        );
        let index_price = IndexPriceUpdate::new(
            instrument_id,
            Price::from("100.20"),
            UnixNanos::from(9),
            UnixNanos::from(10),
        );
        let funding_rate = FundingRateUpdate::new(
            instrument_id,
            "0.0001".parse().unwrap(),
            Some(480),
            Some(UnixNanos::from(12)),
            UnixNanos::from(11),
            UnixNanos::from(12),
        );
        let greeks = OptionGreeks {
            instrument_id,
            ts_event: UnixNanos::from(13),
            ts_init: UnixNanos::from(14),
            ..OptionGreeks::default()
        };

        let batches = [
            DataBatch::from(vec![stub_delta()]),
            DataBatch::from(vec![stub_deltas()]),
            DataBatch::from(vec![stub_depth10()]),
            DataBatch::from(vec![QuoteTick::default()]),
            DataBatch::from(vec![stub_trade_ethusdt_buy()]),
            DataBatch::from(vec![stub_bar()]),
            DataBatch::from(vec![mark_price]),
            DataBatch::from(vec![index_price]),
            DataBatch::from(vec![funding_rate]),
            DataBatch::from(vec![greeks]),
            DataBatch::from(vec![stub_instrument_status()]),
            DataBatch::from(vec![stub_instrument_close()]),
        ];

        assert!(matches!(batches[0], DataBatch::BookDelta(ref data) if data.len() == 1));
        assert!(matches!(batches[1], DataBatch::BookDeltas(ref data) if data.len() == 1));
        assert!(matches!(batches[2], DataBatch::BookDepth10(ref data) if data.len() == 1));
        assert!(matches!(batches[3], DataBatch::Quote(ref data) if data.len() == 1));
        assert!(matches!(batches[4], DataBatch::Trade(ref data) if data.len() == 1));
        assert!(matches!(batches[5], DataBatch::Bar(ref data) if data.len() == 1));
        assert!(matches!(batches[6], DataBatch::MarkPrice(ref data) if data.len() == 1));
        assert!(matches!(batches[7], DataBatch::IndexPrice(ref data) if data.len() == 1));
        assert!(matches!(batches[8], DataBatch::FundingRate(ref data) if data.len() == 1));
        assert!(matches!(batches[9], DataBatch::OptionGreeks(ref data) if data.len() == 1));
        assert!(matches!(batches[10], DataBatch::InstrumentStatus(ref data) if data.len() == 1));
        assert!(matches!(batches[11], DataBatch::InstrumentClose(ref data) if data.len() == 1));
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_data_batch_from_defi_vec_uses_defi_variant() {
        let instrument_id = InstrumentId::new(Symbol::from("ETH/USDC"), Venue::from("UNISWAPV3"));
        let snapshot = PoolSnapshot::new(
            instrument_id,
            PoolState::default(),
            Vec::new(),
            Vec::new(),
            PoolAnalytics::default(),
            BlockPosition::new(12, "0xabc".to_string(), 3, 4),
            UnixNanos::from(5),
            UnixNanos::from(6),
        );

        let batch = DataBatch::from(vec![DefiData::PoolSnapshot(snapshot)]);

        assert!(matches!(batch, DataBatch::Defi(ref data) if data.len() == 1));
    }
}
