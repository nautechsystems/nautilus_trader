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

//! Data types and shared representations for the trading domain model.
//!
//! [`Data`] provides an owned, heterogeneous representation of built-in data, while [`DataRef`]
//! provides borrowed access to the same variants. [`DataBatch`] preserves concrete element types
//! for homogeneous storage and exposes individual items through the borrowed representation.

pub mod bar;
pub mod batch;
pub mod bet;
pub mod black_scholes;
pub mod close;
pub mod custom;
pub mod data_type;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod funding;
pub mod greeks;
pub mod option_chain;
pub mod order;
pub mod prices;
pub mod quote;
pub mod registry;
pub mod status;
pub mod trade;

/// Arrow schema-map name for compact string enum columns.
pub const ARROW_ENUM_DICTIONARY: &str = "Dictionary(Int8, Utf8)";
/// Arrow schema-map name for UTC nanosecond instants.
pub const ARROW_TIMESTAMP_NANOSECOND: &str = "Timestamp(Nanosecond, Some(\"UTC\"))";

#[cfg(any(test, feature = "test-support"))]
pub mod stubs;

use std::{
    fmt::{Debug, Display},
    ops::{Deref, Range},
    str::FromStr,
    sync::Arc,
};

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

#[cfg(feature = "defi")]
use crate::defi::DefiData;
// Re-exports
#[rustfmt::skip]  // Keep these grouped
pub use bar::{Bar, BarSpecification, BarType};
pub use black_scholes::Greeks;
pub use close::InstrumentClose;
#[cfg(feature = "python")]
pub use custom::PythonCustomDataWrapper;
pub use custom::{
    CustomData, CustomDataTrait, ensure_custom_data_json_registered, register_custom_data_json,
};
#[cfg(feature = "python")]
pub use custom::{
    get_python_data_class, reconstruct_python_custom_data, register_python_data_class,
};
pub use data_type::DataType;
pub use delta::OrderBookDelta;
pub use deltas::OrderBookDeltas;
pub use depth::{DEPTH_INLINE_LEN, DEPTH10_LEN, OrderBookDepth, OrderBookDepth10};
pub use funding::FundingRateUpdate;
pub use greeks::{
    BlackScholesGreeksResult, GreeksData, HasGreeks, OptionGreekValues, PortfolioGreeks,
    YieldCurveData, black_scholes_greeks, imply_vol_and_greeks, refine_vol_and_greeks,
};
pub use option_chain::{OptionChainSlice, OptionGreeks, OptionStrikeData, StrikeRange};
pub use order::{BookOrder, NULL_ORDER};
pub use prices::{IndexPriceUpdate, MarkPriceUpdate};
pub use quote::QuoteTick;
#[cfg(feature = "arrow")]
pub use registry::{
    ArrowDecoder, ArrowEncoder, decode_custom_from_arrow, encode_custom_to_arrow,
    ensure_arrow_registered, get_arrow_schema, register_arrow, validate_custom_arrow_schema,
};
#[cfg(feature = "python")]
pub use registry::{
    PyExtractor, ensure_py_extractor_registered, ensure_rust_extractor_factory_registered,
    ensure_rust_extractor_registered, get_rust_extractor, register_py_extractor,
    register_rust_extractor, register_rust_extractor_factory, try_extract_from_py,
};
pub use registry::{
    deserialize_custom_from_json, ensure_json_deserializer_registered, register_json_deserializer,
};
pub use status::InstrumentStatus;
pub use trade::TradeTick;

/// Canonical custom data type name for calculated option Greeks.
pub const GREEKS_DATA_TYPE_NAME: &str = "GreeksData";

use crate::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

/// Invokes a macro with every built-in typed data family.
///
/// Each tuple contains the canonical family name, concrete type, `Data` variant,
/// `DataBatch` variant, and catalog path prefix. The first four fields use the same name whenever
/// the family has one concrete representation. Custom and DeFi data have no single built-in
/// concrete type; `Deltas` and `Data` are aggregate compatibility batches. Consumers handle
/// those variants explicitly.
#[macro_export]
macro_rules! for_each_data_type {
    ($macro:ident) => {
        $macro! {
            (Instrument, InstrumentAny, Instrument, Instrument, "instruments"),
            (QuoteTick, QuoteTick, Quote, Quote, "quotes"),
            (TradeTick, TradeTick, Trade, Trade, "trades"),
            (Bar, Bar, Bar, Bar, "bars"),
            (OrderBookDelta, OrderBookDelta, BookDelta, BookDelta, "order_book_deltas"),
            (OrderBookDepth, OrderBookDepth, BookDepth, BookDepth, "order_book_depths"),
            (MarkPriceUpdate, MarkPriceUpdate, MarkPrice, MarkPrice, "mark_prices"),
            (IndexPriceUpdate, IndexPriceUpdate, IndexPrice, IndexPrice, "index_prices"),
            (FundingRateUpdate, FundingRateUpdate, FundingRate, FundingRate, "funding_rates"),
            (InstrumentStatus, InstrumentStatus, InstrumentStatus, InstrumentStatus, "instrument_status"),
            (OptionGreeks, OptionGreeks, OptionGreeks, OptionGreeks, "option_greeks"),
            (InstrumentClose, InstrumentClose, InstrumentClose, InstrumentClose, "instrument_closes"),
        }
    };
    ($macro:ident, $($args:tt)*) => {
        $macro! {
            ($($args)*);
            (Instrument, InstrumentAny, Instrument, Instrument, "instruments"),
            (QuoteTick, QuoteTick, Quote, Quote, "quotes"),
            (TradeTick, TradeTick, Trade, Trade, "trades"),
            (Bar, Bar, Bar, Bar, "bars"),
            (OrderBookDelta, OrderBookDelta, BookDelta, BookDelta, "order_book_deltas"),
            (OrderBookDepth, OrderBookDepth, BookDepth, BookDepth, "order_book_depths"),
            (MarkPriceUpdate, MarkPriceUpdate, MarkPrice, MarkPrice, "mark_prices"),
            (IndexPriceUpdate, IndexPriceUpdate, IndexPrice, IndexPrice, "index_prices"),
            (FundingRateUpdate, FundingRateUpdate, FundingRate, FundingRate, "funding_rates"),
            (InstrumentStatus, InstrumentStatus, InstrumentStatus, InstrumentStatus, "instrument_status"),
            (OptionGreeks, OptionGreeks, OptionGreeks, OptionGreeks, "option_greeks"),
            (InstrumentClose, InstrumentClose, InstrumentClose, InstrumentClose, "instrument_closes"),
        }
    };
}

/// A built-in Nautilus data type.
///
/// Not recommended for storing large amounts of data, as the largest variant is significantly
/// larger (~10x) than the smallest.
#[derive(Debug)]
pub enum Data {
    Instrument(Box<InstrumentAny>),
    BookDelta(OrderBookDelta),
    BookDeltas(Box<OrderBookDeltas>),
    BookDepth(Box<OrderBookDepth>), // This variant is significantly larger
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    FundingRate(FundingRateUpdate),
    OptionGreeks(OptionGreeks),
    InstrumentStatus(InstrumentStatus),
    InstrumentClose(InstrumentClose),
    Custom(CustomData),
    #[cfg(feature = "defi")]
    Defi(Box<DefiData>), // This variant is significantly larger
}

/// Borrowed data item used by typed replay paths.
#[derive(Clone, Copy, Debug)]
pub enum DataRef<'a> {
    Instrument(&'a InstrumentAny),
    BookDelta(&'a OrderBookDelta),
    BookDeltas(&'a OrderBookDeltas),
    BookDepth(&'a OrderBookDepth),
    Quote(&'a QuoteTick),
    Trade(&'a TradeTick),
    Bar(&'a Bar),
    MarkPrice(&'a MarkPriceUpdate),
    IndexPrice(&'a IndexPriceUpdate),
    FundingRate(&'a FundingRateUpdate),
    OptionGreeks(&'a OptionGreeks),
    InstrumentStatus(&'a InstrumentStatus),
    InstrumentClose(&'a InstrumentClose),
    Custom(&'a CustomData),
    #[cfg(feature = "defi")]
    Defi(&'a DefiData),
}

impl<'a> From<&'a Data> for DataRef<'a> {
    fn from(data: &'a Data) -> Self {
        match data {
            Data::Instrument(instrument) => Self::Instrument(instrument),
            Data::BookDelta(delta) => Self::BookDelta(delta),
            Data::BookDeltas(deltas) => Self::BookDeltas(deltas),
            Data::BookDepth(depth) => Self::BookDepth(depth),
            Data::Quote(quote) => Self::Quote(quote),
            Data::Trade(trade) => Self::Trade(trade),
            Data::Bar(bar) => Self::Bar(bar),
            Data::MarkPrice(mark_price) => Self::MarkPrice(mark_price),
            Data::IndexPrice(index_price) => Self::IndexPrice(index_price),
            Data::FundingRate(funding_rate) => Self::FundingRate(funding_rate),
            Data::OptionGreeks(greeks) => Self::OptionGreeks(greeks),
            Data::InstrumentStatus(status) => Self::InstrumentStatus(status),
            Data::InstrumentClose(close) => Self::InstrumentClose(close),
            Data::Custom(custom) => Self::Custom(custom),
            #[cfg(feature = "defi")]
            Data::Defi(defi) => Self::Defi(defi),
        }
    }
}

impl DataRef<'_> {
    /// Returns the instrument ID for the data.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Instrument(instrument) => instrument.id(),
            Self::BookDelta(delta) => delta.instrument_id,
            Self::BookDeltas(deltas) => deltas.instrument_id,
            Self::BookDepth(depth) => depth.instrument_id,
            Self::Quote(quote) => quote.instrument_id,
            Self::Trade(trade) => trade.instrument_id,
            Self::Bar(bar) => bar.bar_type.instrument_id(),
            Self::MarkPrice(mark_price) => mark_price.instrument_id,
            Self::IndexPrice(index_price) => index_price.instrument_id,
            Self::FundingRate(funding_rate) => funding_rate.instrument_id,
            Self::OptionGreeks(greeks) => greeks.instrument_id,
            Self::InstrumentStatus(status) => status.instrument_id,
            Self::InstrumentClose(close) => close.instrument_id,
            Self::Custom(custom) => custom
                .data_type
                .identifier()
                .and_then(|s| InstrumentId::from_str(s).ok())
                .or_else(|| {
                    custom
                        .data_type
                        .metadata()
                        .and_then(|m| m.get_str("instrument_id"))
                        .and_then(|s| InstrumentId::from_str(s).ok())
                })
                .unwrap_or_else(|| InstrumentId::from("NULL.NULL")),
            #[cfg(feature = "defi")]
            Self::Defi(defi) => defi.instrument_id(),
        }
    }

    /// Returns whether the data is a type of order book data.
    #[must_use]
    pub fn is_order_book_data(&self) -> bool {
        matches!(
            self,
            Self::BookDelta(_) | Self::BookDeltas(_) | Self::BookDepth(_)
        )
    }

    /// Materializes this borrowed item as an owned [`Data`] enum for compatibility.
    #[must_use]
    pub fn to_owned_data(&self) -> Data {
        match self {
            Self::Instrument(instrument) => Data::Instrument(Box::new((*instrument).clone())),
            Self::BookDelta(delta) => Data::BookDelta(**delta),
            Self::BookDeltas(deltas) => Data::BookDeltas(Box::new((**deltas).clone())),
            Self::BookDepth(depth) => Data::BookDepth(Box::new((*depth).clone())),
            Self::Quote(quote) => Data::Quote(**quote),
            Self::Trade(trade) => Data::Trade(**trade),
            Self::Bar(bar) => Data::Bar(**bar),
            Self::MarkPrice(mark_price) => Data::MarkPrice(**mark_price),
            Self::IndexPrice(index_price) => Data::IndexPrice(**index_price),
            Self::FundingRate(funding_rate) => Data::FundingRate(**funding_rate),
            Self::OptionGreeks(greeks) => Data::OptionGreeks(**greeks),
            Self::InstrumentStatus(status) => Data::InstrumentStatus(**status),
            Self::InstrumentClose(close) => Data::InstrumentClose(**close),
            Self::Custom(custom) => Data::Custom((**custom).clone()),
            #[cfg(feature = "defi")]
            Self::Defi(defi) => Data::Defi(Box::new((**defi).clone())),
        }
    }
}

impl HasTsInit for DataRef<'_> {
    fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Instrument(instrument) => Instrument::ts_init(*instrument),
            Self::BookDelta(delta) => delta.ts_init,
            Self::BookDeltas(deltas) => deltas.ts_init,
            Self::BookDepth(depth) => depth.ts_init,
            Self::Quote(quote) => quote.ts_init,
            Self::Trade(trade) => trade.ts_init,
            Self::Bar(bar) => bar.ts_init,
            Self::MarkPrice(mark_price) => mark_price.ts_init,
            Self::IndexPrice(index_price) => index_price.ts_init,
            Self::FundingRate(funding_rate) => funding_rate.ts_init,
            Self::OptionGreeks(greeks) => greeks.ts_init,
            Self::InstrumentStatus(status) => status.ts_init,
            Self::InstrumentClose(close) => close.ts_init,
            Self::Custom(custom) => custom.data.ts_init(),
            #[cfg(feature = "defi")]
            Self::Defi(defi) => defi.ts_init(),
        }
    }
}

/// Range view over a shared typed data batch.
#[derive(Clone, Debug)]
#[expect(
    clippy::rc_buffer,
    reason = "Backtest batch views share full Vec batches by design"
)]
pub struct BatchView<T> {
    data: Arc<Vec<T>>,
    range: Range<usize>,
}

impl<T> BatchView<T> {
    /// Creates a new [`BatchView`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `range` is invalid or exceeds `data.len()`.
    #[must_use]
    pub fn new(data: Arc<Vec<T>>, range: Range<usize>) -> Self {
        assert!(range.start <= range.end, "invalid batch view range");
        assert!(
            range.end <= data.len(),
            "batch view range exceeds data length"
        );
        Self { data, range }
    }

    #[must_use]
    pub fn full(data: Arc<Vec<T>>) -> Self {
        let len = data.len();
        Self {
            data,
            range: 0..len,
        }
    }

    #[must_use]
    pub fn arc(&self) -> &Arc<Vec<T>> {
        &self.data
    }

    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Returns a sub-view of this batch view.
    ///
    /// # Panics
    ///
    /// Panics if `[start, end)` is invalid or exceeds this view length.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        assert!(start <= end, "invalid batch slice range");
        assert!(end <= self.len(), "batch slice range exceeds view length");
        Self {
            data: self.data.clone(),
            range: (self.range.start + start)..(self.range.start + end),
        }
    }
    /// Returns this view for modification, cloning shared storage when needed.
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

/// Shared typed batch used by replay and catalog fan-out paths.
#[derive(Clone, Debug)]
pub enum DataBatch {
    Instrument(BatchView<InstrumentAny>),
    BookDelta(BatchView<OrderBookDelta>),
    BookDeltas(BatchView<OrderBookDeltas>),
    BookDepth(BatchView<OrderBookDepth>),
    Quote(BatchView<QuoteTick>),
    Trade(BatchView<TradeTick>),
    Bar(BatchView<Bar>),
    MarkPrice(BatchView<MarkPriceUpdate>),
    IndexPrice(BatchView<IndexPriceUpdate>),
    FundingRate(BatchView<FundingRateUpdate>),
    OptionGreeks(BatchView<OptionGreeks>),
    InstrumentStatus(BatchView<InstrumentStatus>),
    InstrumentClose(BatchView<InstrumentClose>),
    Custom(BatchView<CustomData>),
    #[cfg(feature = "defi")]
    Defi(BatchView<DefiData>),
}

macro_rules! data_batch_from_data_vec {
    (
        ($data_type:ident, $input:ident);
        $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
    ) => {
        match $data_type {
            $(
                NautilusDataType::$variant => Ok(Self::$batch(
                    to_variant_for_batch::<$type>($data_type, $input)?.into(),
                )),
            )+
            NautilusDataType::OrderBook => {
                anyhow::bail!("order book snapshots cannot be represented as a data batch")
            }
            NautilusDataType::Custom { .. } => {
                let expected_len = $input.len();
                let custom = $input
                    .into_iter()
                    .filter_map(|item| match item {
                        Data::Custom(custom) => Some(custom),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                anyhow::ensure!(
                    custom.len() == expected_len,
                    "catalog query for {} returned rows with another data type",
                    $data_type,
                );
                Ok(Self::Custom(custom.into()))
            }
            #[cfg(feature = "defi")]
            NautilusDataType::Defi => {
                let expected_len = $input.len();
                let defi = $input
                    .into_iter()
                    .filter_map(|item| match item {
                        Data::Defi(defi) => Some(*defi),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                anyhow::ensure!(
                    defi.len() == expected_len,
                    "catalog query for {} returned rows with another data type",
                    $data_type,
                );
                Ok(Self::Defi(defi.into()))
            }
        }
    };
}

impl DataBatch {
    /// Converts owned compatibility rows into a typed batch for `data_type`.
    ///
    /// This is a transition path for backends that still decode through [`Data`] but should not
    /// expose legacy batches to typed replay.
    ///
    /// # Errors
    ///
    /// Returns an error if `data_type` is not replayable as a typed batch.
    pub fn from_data_vec_for_type(
        data_type: &NautilusDataType,
        data: Vec<Data>,
    ) -> anyhow::Result<Self> {
        if matches!(data_type, NautilusDataType::OrderBookDelta)
            && matches!(data.first(), Some(Data::BookDeltas(_)))
        {
            return Ok(Self::BookDeltas(
                to_variant_for_batch::<OrderBookDeltas>(data_type, data)?.into(),
            ));
        }

        crate::for_each_data_type!(data_batch_from_data_vec, data_type, data)
    }

    /// Groups mixed compatibility rows into typed batches.
    ///
    /// [`Data::BookDeltas`] values retain their event-batch boundary in a separate typed batch.
    ///
    /// # Errors
    ///
    /// Returns an error if a row cannot be represented by a typed batch.
    pub fn from_data_vec_grouped(data: &[Data]) -> anyhow::Result<Vec<Self>> {
        let mut groups = Vec::<(NautilusDataType, Vec<Data>)>::new();

        for item in data.iter().cloned() {
            let data_type = NautilusDataType::from_data(&item);
            let is_deltas = matches!(item, Data::BookDeltas(_));

            if let Some((_, group)) = groups.iter_mut().find(|(group_type, group)| {
                group_type == &data_type
                    && group
                        .first()
                        .is_some_and(|item| matches!(item, Data::BookDeltas(_)) == is_deltas)
            }) {
                group.push(item);
            } else {
                groups.push((data_type, vec![item]));
            }
        }

        groups
            .into_iter()
            .map(|(data_type, data)| Self::from_data_vec_for_type(&data_type, data))
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Instrument(data) => data.len(),
            Self::BookDelta(data) => data.len(),
            Self::BookDeltas(data) => data.len(),
            Self::BookDepth(data) => data.len(),
            Self::Quote(data) => data.len(),
            Self::Trade(data) => data.len(),
            Self::Bar(data) => data.len(),
            Self::MarkPrice(data) => data.len(),
            Self::IndexPrice(data) => data.len(),
            Self::FundingRate(data) => data.len(),
            Self::OptionGreeks(data) => data.len(),
            Self::InstrumentStatus(data) => data.len(),
            Self::InstrumentClose(data) => data.len(),
            Self::Custom(data) => data.len(),
            #[cfg(feature = "defi")]
            Self::Defi(data) => data.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the storage-family name of this batch's data type.
    #[must_use]
    pub fn data_type_name(&self) -> &'static str {
        match self {
            Self::Instrument(_) => "instruments",
            Self::BookDelta(_) => "order_book_deltas",
            Self::BookDeltas(_) => "order_book_deltas_batches",
            Self::BookDepth(_) => "order_book_depths",
            Self::Quote(_) => "quotes",
            Self::Trade(_) => "trades",
            Self::Bar(_) => "bars",
            Self::MarkPrice(_) => "mark_prices",
            Self::IndexPrice(_) => "index_prices",
            Self::FundingRate(_) => "funding_rates",
            Self::OptionGreeks(_) => "option_greeks",
            Self::InstrumentStatus(_) => "instrument_status",
            Self::InstrumentClose(_) => "instrument_closes",
            Self::Custom(_) => "custom",
            #[cfg(feature = "defi")]
            Self::Defi(_) => "defi",
        }
    }

    #[must_use]
    pub fn is_monotonically_increasing_by_init(&self) -> bool {
        (0..self.len())
            .filter_map(|index| self.get(index))
            .map(|data| data.ts_init())
            .is_sorted()
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<DataRef<'_>> {
        match self {
            Self::Instrument(data) => data.get(index).map(DataRef::Instrument),
            Self::BookDelta(data) => data.get(index).map(DataRef::BookDelta),
            Self::BookDeltas(data) => data.get(index).map(DataRef::BookDeltas),
            Self::BookDepth(data) => data.get(index).map(DataRef::BookDepth),
            Self::Quote(data) => data.get(index).map(DataRef::Quote),
            Self::Trade(data) => data.get(index).map(DataRef::Trade),
            Self::Bar(data) => data.get(index).map(DataRef::Bar),
            Self::MarkPrice(data) => data.get(index).map(DataRef::MarkPrice),
            Self::IndexPrice(data) => data.get(index).map(DataRef::IndexPrice),
            Self::FundingRate(data) => data.get(index).map(DataRef::FundingRate),
            Self::OptionGreeks(data) => data.get(index).map(DataRef::OptionGreeks),
            Self::InstrumentStatus(data) => data.get(index).map(DataRef::InstrumentStatus),
            Self::InstrumentClose(data) => data.get(index).map(DataRef::InstrumentClose),
            Self::Custom(data) => data.get(index).map(DataRef::Custom),
            #[cfg(feature = "defi")]
            Self::Defi(data) => data.get(index).map(DataRef::Defi),
        }
    }

    #[must_use]
    pub fn aligned_chunk(&self, start: usize, chunk_size: Option<usize>) -> Option<(Self, usize)> {
        let len = self.len();
        if start >= len {
            return None;
        }

        let mut end = match chunk_size {
            Some(size) => len.min(start + size.max(1)),
            None => len,
        };

        if end < len
            && let Some(boundary_ts) = self.get(end - 1).map(|data| data.ts_init())
        {
            while end < len
                && self
                    .get(end)
                    .is_some_and(|data| data.ts_init() == boundary_ts)
            {
                end += 1;
            }
        }

        Some((self.slice(start, end), end))
    }

    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        if start == 0 && end == self.len() {
            return self.clone();
        }

        match self {
            Self::Instrument(data) => Self::Instrument(data.slice(start, end)),
            Self::BookDelta(data) => Self::BookDelta(data.slice(start, end)),
            Self::BookDeltas(data) => Self::BookDeltas(data.slice(start, end)),
            Self::BookDepth(data) => Self::BookDepth(data.slice(start, end)),
            Self::Quote(data) => Self::Quote(data.slice(start, end)),
            Self::Trade(data) => Self::Trade(data.slice(start, end)),
            Self::Bar(data) => Self::Bar(data.slice(start, end)),
            Self::MarkPrice(data) => Self::MarkPrice(data.slice(start, end)),
            Self::IndexPrice(data) => Self::IndexPrice(data.slice(start, end)),
            Self::FundingRate(data) => Self::FundingRate(data.slice(start, end)),
            Self::OptionGreeks(data) => Self::OptionGreeks(data.slice(start, end)),
            Self::InstrumentStatus(data) => Self::InstrumentStatus(data.slice(start, end)),
            Self::InstrumentClose(data) => Self::InstrumentClose(data.slice(start, end)),
            Self::Custom(data) => Self::Custom(data.slice(start, end)),
            #[cfg(feature = "defi")]
            Self::Defi(data) => Self::Defi(data.slice(start, end)),
        }
    }

    #[must_use]
    pub fn to_data_vec_for_compat(&self) -> Vec<Data> {
        (0..self.len())
            .filter_map(|index| self.get(index).map(|item| item.to_owned_data()))
            .collect()
    }
}

/// Typed values that wrap into their [`DataBatch`] variant.
pub trait IntoDataBatch: Sized {
    /// Wraps owned typed values in their [`DataBatch`] variant.
    #[must_use]
    fn into_batch(data: Vec<Self>) -> DataBatch;
}

impl<T: IntoDataBatch> From<Vec<T>> for DataBatch {
    fn from(data: Vec<T>) -> Self {
        T::into_batch(data)
    }
}

macro_rules! impl_into_data_batch {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        $(
            impl IntoDataBatch for $type {
                fn into_batch(data: Vec<Self>) -> DataBatch {
                    DataBatch::$batch(data.into())
                }
            }
        )+
    };
}

for_each_data_type!(impl_into_data_batch);

impl IntoDataBatch for OrderBookDeltas {
    fn into_batch(data: Vec<Self>) -> DataBatch {
        DataBatch::BookDeltas(data.into())
    }
}

impl IntoDataBatch for CustomData {
    fn into_batch(data: Vec<Self>) -> DataBatch {
        DataBatch::Custom(data.into())
    }
}

#[cfg(feature = "defi")]
impl IntoDataBatch for DefiData {
    fn into_batch(data: Vec<Self>) -> DataBatch {
        DataBatch::Defi(data.into())
    }
}

/// Typed values that unwrap from their [`DataBatch`] variant.
pub trait FromDataBatch: Sized {
    /// Unwraps a typed batch into owned values.
    ///
    /// # Errors
    ///
    /// Returns an error if `batch` holds another variant.
    fn from_batch(batch: DataBatch) -> anyhow::Result<Vec<Self>>;
}

macro_rules! impl_from_data_batch {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        $(
            impl FromDataBatch for $type {
                fn from_batch(batch: DataBatch) -> anyhow::Result<Vec<Self>> {
                    match batch {
                        DataBatch::$batch(data) => Ok(data.as_ref().to_vec()),
                        _ => anyhow::bail!(
                            "expected a {} batch, found another data batch variant",
                            stringify!($batch),
                        ),
                    }
                }
            }
        )+
    };
}

for_each_data_type!(impl_from_data_batch);

impl FromDataBatch for CustomData {
    fn from_batch(batch: DataBatch) -> anyhow::Result<Vec<Self>> {
        match batch {
            DataBatch::Custom(data) => Ok(data.as_ref().to_vec()),
            _ => anyhow::bail!("expected a Custom batch, found another data batch variant"),
        }
    }
}

#[cfg(feature = "defi")]
impl FromDataBatch for DefiData {
    fn from_batch(batch: DataBatch) -> anyhow::Result<Vec<Self>> {
        match batch {
            DataBatch::Defi(data) => Ok(data.as_ref().to_vec()),
            _ => anyhow::bail!("expected a Defi batch, found another data batch variant"),
        }
    }
}

/// Data family selector used by request and catalog APIs.
///
/// This is a type-level descriptor, not a decoded data value. [`Data::BookDeltas`] maps to
/// [`NautilusDataType::OrderBookDelta`] because both share the same storage and request family.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum NautilusDataType {
    Instrument,
    OrderBook,
    QuoteTick,
    TradeTick,
    Bar,
    OrderBookDelta,
    OrderBookDepth,
    MarkPriceUpdate,
    IndexPriceUpdate,
    FundingRateUpdate,
    InstrumentStatus,
    OptionGreeks,
    InstrumentClose,
    /// User-defined data type identified by `type_name`.
    Custom {
        type_name: String,
    },
    #[cfg(feature = "defi")]
    Defi,
}

impl Display for NautilusDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instrument => f.write_str("Instrument"),
            Self::OrderBook => f.write_str("OrderBook"),
            Self::QuoteTick => f.write_str("QuoteTick"),
            Self::TradeTick => f.write_str("TradeTick"),
            Self::Bar => f.write_str("Bar"),
            Self::OrderBookDelta => f.write_str("OrderBookDelta"),
            Self::OrderBookDepth => f.write_str("OrderBookDepth"),
            Self::MarkPriceUpdate => f.write_str("MarkPriceUpdate"),
            Self::IndexPriceUpdate => f.write_str("IndexPriceUpdate"),
            Self::FundingRateUpdate => f.write_str("FundingRateUpdate"),
            Self::InstrumentStatus => f.write_str("InstrumentStatus"),
            Self::OptionGreeks => f.write_str("OptionGreeks"),
            Self::InstrumentClose => f.write_str("InstrumentClose"),
            #[cfg(feature = "defi")]
            Self::Defi => f.write_str("Defi"),
            Self::Custom { type_name } => write!(f, "Custom:{type_name}"),
        }
    }
}

impl FromStr for NautilusDataType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "Instrument" | "instruments" | "instrument" => Ok(Self::Instrument),
            "OrderBook" | "order_book" => Ok(Self::OrderBook),
            "QuoteTick" | "quotes" | "quote" | "quote_tick" => Ok(Self::QuoteTick),
            "TradeTick" | "trades" | "trade" | "trade_tick" => Ok(Self::TradeTick),
            "Bar" | "bars" | "bar" => Ok(Self::Bar),
            "OrderBookDelta" | "OrderBookDeltas" | "order_book_deltas" | "order_book_delta" => {
                Ok(Self::OrderBookDelta)
            }
            // One-release compatibility spellings for the former fixed-depth type
            "OrderBookDepth10" | "OrderBookDepth" | "order_book_depths" | "order_book_depth10" => {
                Ok(Self::OrderBookDepth)
            }
            "MarkPriceUpdate" | "mark_price_updates" | "mark_prices" | "mark_price_update" => {
                Ok(Self::MarkPriceUpdate)
            }
            "IndexPriceUpdate" | "index_price_updates" | "index_prices" | "index_price_update" => {
                Ok(Self::IndexPriceUpdate)
            }
            "FundingRateUpdate" | "funding_rate_update" | "funding_rates" => {
                Ok(Self::FundingRateUpdate)
            }
            "InstrumentStatus" | "instrument_status" => Ok(Self::InstrumentStatus),
            "OptionGreeks" | "option_greeks" => Ok(Self::OptionGreeks),
            "InstrumentClose" | "instrument_closes" | "instrument_close" => {
                Ok(Self::InstrumentClose)
            }
            #[cfg(feature = "defi")]
            "Defi" => Ok(Self::Defi),
            custom if custom.starts_with("Custom:") => Ok(Self::Custom {
                type_name: custom.trim_start_matches("Custom:").to_string(),
            }),
            _ => anyhow::bail!("Invalid `NautilusDataType`: '{s}'"),
        }
    }
}

/// Catalog record selector used to query Arrow-backed persisted records.
#[derive(
    Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, strum::Display, strum::EnumIter,
)]
pub enum NautilusRecordType {
    AccountState,
    OrderInitialized,
    OrderDenied,
    OrderEmulated,
    OrderSubmitted,
    OrderAccepted,
    OrderRejected,
    OrderPendingCancel,
    OrderCanceled,
    OrderCancelRejected,
    OrderExpired,
    OrderTriggered,
    OrderPendingUpdate,
    OrderReleased,
    OrderModifyRejected,
    OrderUpdated,
    OrderFilled,
    OrderFillVoided,
    PositionOpened,
    PositionChanged,
    PositionClosed,
    PositionAdjusted,
    OrderSnapshot,
    PositionSnapshot,
    OrderStatusReport,
    FillReport,
    PositionStatusReport,
    ExecutionMassStatus,
    #[cfg(feature = "defi")]
    Defi,
}

impl FromStr for NautilusRecordType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "AccountState" | "account_state" => Ok(Self::AccountState),
            "OrderInitialized" | "order_initialized" => Ok(Self::OrderInitialized),
            "OrderDenied" | "order_denied" => Ok(Self::OrderDenied),
            "OrderEmulated" | "order_emulated" => Ok(Self::OrderEmulated),
            "OrderSubmitted" | "order_submitted" => Ok(Self::OrderSubmitted),
            "OrderAccepted" | "order_accepted" => Ok(Self::OrderAccepted),
            "OrderRejected" | "order_rejected" => Ok(Self::OrderRejected),
            "OrderPendingCancel" | "order_pending_cancel" => Ok(Self::OrderPendingCancel),
            "OrderCanceled" | "order_canceled" => Ok(Self::OrderCanceled),
            "OrderCancelRejected" | "order_cancel_rejected" => Ok(Self::OrderCancelRejected),
            "OrderExpired" | "order_expired" => Ok(Self::OrderExpired),
            "OrderTriggered" | "order_triggered" => Ok(Self::OrderTriggered),
            "OrderPendingUpdate" | "order_pending_update" => Ok(Self::OrderPendingUpdate),
            "OrderReleased" | "order_released" => Ok(Self::OrderReleased),
            "OrderModifyRejected" | "order_modify_rejected" => Ok(Self::OrderModifyRejected),
            "OrderUpdated" | "order_updated" => Ok(Self::OrderUpdated),
            "OrderFilled" | "order_filled" => Ok(Self::OrderFilled),
            "OrderFillVoided" | "order_fill_voided" => Ok(Self::OrderFillVoided),
            "PositionOpened" | "position_opened" => Ok(Self::PositionOpened),
            "PositionChanged" | "position_changed" => Ok(Self::PositionChanged),
            "PositionClosed" | "position_closed" => Ok(Self::PositionClosed),
            "PositionAdjusted" | "position_adjusted" => Ok(Self::PositionAdjusted),
            "OrderSnapshot" | "order_snapshot" => Ok(Self::OrderSnapshot),
            "PositionSnapshot" | "position_snapshot" => Ok(Self::PositionSnapshot),
            "OrderStatusReport" | "order_status_report" => Ok(Self::OrderStatusReport),
            "FillReport" | "fill_report" => Ok(Self::FillReport),
            "PositionStatusReport" | "position_status_report" => Ok(Self::PositionStatusReport),
            "ExecutionMassStatus" | "execution_mass_status" => Ok(Self::ExecutionMassStatus),
            "custom" | "CustomData" => {
                anyhow::bail!("custom data queries require NautilusDataType::Custom")
            }
            #[cfg(feature = "defi")]
            "Defi" | "defi" => Ok(Self::Defi),
            _ => anyhow::bail!("Invalid `NautilusRecordType`: '{s}'"),
        }
    }
}

impl NautilusDataType {
    /// Returns the discriminant tag for a [`Data`] value.
    #[must_use]
    pub fn from_data(data: &Data) -> Self {
        match data {
            Data::Instrument(_) => Self::Instrument,
            Data::Quote(_) => Self::QuoteTick,
            Data::Trade(_) => Self::TradeTick,
            Data::Bar(_) => Self::Bar,
            Data::BookDelta(_) | Data::BookDeltas(_) => Self::OrderBookDelta,
            Data::BookDepth(_) => Self::OrderBookDepth,
            Data::MarkPrice(_) => Self::MarkPriceUpdate,
            Data::IndexPrice(_) => Self::IndexPriceUpdate,
            Data::FundingRate(_) => Self::FundingRateUpdate,
            Data::InstrumentStatus(_) => Self::InstrumentStatus,
            Data::OptionGreeks(_) => Self::OptionGreeks,
            Data::InstrumentClose(_) => Self::InstrumentClose,
            Data::Custom(c) => Self::Custom {
                type_name: c.data_type.type_name().to_string(),
            },
            #[cfg(feature = "defi")]
            Data::Defi(_) => Self::Defi,
        }
    }
}

impl<'de> Deserialize<'de> for Data {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        let type_name = value
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| D::Error::custom("Missing 'type' field in Data"))?;

        match type_name {
            "Instrument" => Ok(Self::Instrument(Box::new(
                serde_json::from_value(
                    value
                        .get("data")
                        .cloned()
                        .ok_or_else(|| D::Error::custom("Missing 'data' field for Instrument"))?,
                )
                .map_err(D::Error::custom)?,
            ))),
            "OrderBookDelta" => Ok(Self::BookDelta(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "OrderBookDeltas" => Ok(Self::BookDeltas(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            // One-release compatibility spelling for serialized v1 payloads
            "OrderBookDepth10" | "OrderBookDepth" => Ok(Self::BookDepth(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "QuoteTick" => Ok(Self::Quote(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "TradeTick" => Ok(Self::Trade(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "Bar" => Ok(Self::Bar(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "MarkPriceUpdate" => Ok(Self::MarkPrice(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "IndexPriceUpdate" => Ok(Self::IndexPrice(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "FundingRateUpdate" => Ok(Self::FundingRate(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "OptionGreeks" => Ok(Self::OptionGreeks(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "InstrumentStatus" => Ok(Self::InstrumentStatus(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "InstrumentClose" => Ok(Self::InstrumentClose(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            _ => {
                if let Some(data) =
                    deserialize_custom_from_json(type_name, &value).map_err(D::Error::custom)?
                {
                    Ok(data)
                } else {
                    Err(D::Error::custom(format!("Unknown Data type: {type_name}")))
                }
            }
        }
    }
}

impl Clone for Data {
    fn clone(&self) -> Self {
        match self {
            Self::Instrument(x) => Self::Instrument(x.clone()),
            Self::BookDelta(x) => Self::BookDelta(*x),
            Self::BookDeltas(x) => Self::BookDeltas(x.clone()),
            Self::BookDepth(x) => Self::BookDepth(x.clone()),
            Self::Quote(x) => Self::Quote(*x),
            Self::Trade(x) => Self::Trade(*x),
            Self::Bar(x) => Self::Bar(*x),
            Self::MarkPrice(x) => Self::MarkPrice(*x),
            Self::IndexPrice(x) => Self::IndexPrice(*x),
            Self::FundingRate(x) => Self::FundingRate(*x),
            Self::OptionGreeks(x) => Self::OptionGreeks(*x),
            Self::InstrumentStatus(x) => Self::InstrumentStatus(*x),
            Self::InstrumentClose(x) => Self::InstrumentClose(*x),
            Self::Custom(x) => Self::Custom(x.clone()),
            #[cfg(feature = "defi")]
            Self::Defi(x) => Self::Defi(x.clone()),
        }
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Instrument(a), Self::Instrument(b)) => a == b,
            (Self::BookDelta(a), Self::BookDelta(b)) => a == b,
            (Self::BookDeltas(a), Self::BookDeltas(b)) => a == b,
            (Self::BookDepth(a), Self::BookDepth(b)) => a == b,
            (Self::Quote(a), Self::Quote(b)) => a == b,
            (Self::Trade(a), Self::Trade(b)) => a == b,
            (Self::Bar(a), Self::Bar(b)) => a == b,
            (Self::MarkPrice(a), Self::MarkPrice(b)) => a == b,
            (Self::IndexPrice(a), Self::IndexPrice(b)) => a == b,
            (Self::FundingRate(a), Self::FundingRate(b)) => a == b,
            (Self::OptionGreeks(a), Self::OptionGreeks(b)) => a == b,
            (Self::InstrumentStatus(a), Self::InstrumentStatus(b)) => a == b,
            (Self::InstrumentClose(a), Self::InstrumentClose(b)) => a == b,
            (Self::Custom(a), Self::Custom(b)) => a == b,
            #[cfg(feature = "defi")]
            (Self::Defi(a), Self::Defi(b)) => a == b,
            _ => false,
        }
    }
}

impl Serialize for Data {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Instrument(instrument) => serde_json::json!({
                "type": "Instrument",
                "data": instrument,
            })
            .serialize(serializer),
            Self::BookDelta(x) => x.serialize(serializer),
            Self::BookDeltas(x) => x.serialize(serializer),
            Self::BookDepth(x) => x.serialize(serializer),
            Self::Quote(x) => x.serialize(serializer),
            Self::Trade(x) => x.serialize(serializer),
            Self::Bar(x) => x.serialize(serializer),
            Self::MarkPrice(x) => x.serialize(serializer),
            Self::IndexPrice(x) => x.serialize(serializer),
            Self::FundingRate(x) => x.serialize(serializer),
            Self::OptionGreeks(x) => x.serialize(serializer),
            Self::InstrumentStatus(x) => x.serialize(serializer),
            Self::InstrumentClose(x) => x.serialize(serializer),
            Self::Custom(x) => x.serialize(serializer),
            #[cfg(feature = "defi")]
            Self::Defi(_) => Err(serde::ser::Error::custom(
                "Data::Defi serialization is not supported",
            )),
        }
    }
}

macro_rules! impl_data_conversions {
    ($variant:ident, $type:ty) => {
        impl TryFrom<Data> for $type {
            type Error = ();

            fn try_from(value: Data) -> Result<Self, Self::Error> {
                match value {
                    Data::$variant(x) => Ok(x),
                    _ => Err(()),
                }
            }
        }

        impl From<$type> for Data {
            fn from(value: $type) -> Self {
                Self::$variant(value)
            }
        }
    };
}

impl TryFrom<Data> for OrderBookDepth {
    type Error = ();

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        match value {
            Data::BookDepth(x) => Ok(*x),
            _ => Err(()),
        }
    }
}

impl TryFrom<Data> for InstrumentAny {
    type Error = ();

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        match value {
            Data::Instrument(instrument) => Ok(*instrument),
            _ => Err(()),
        }
    }
}

impl TryFrom<Data> for OrderBookDeltas {
    type Error = ();

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        match value {
            Data::BookDeltas(deltas) => Ok(*deltas),
            _ => Err(()),
        }
    }
}

impl_data_conversions!(Quote, QuoteTick);
impl_data_conversions!(BookDelta, OrderBookDelta);
impl_data_conversions!(Trade, TradeTick);
impl_data_conversions!(Bar, Bar);
impl_data_conversions!(MarkPrice, MarkPriceUpdate);
impl_data_conversions!(IndexPrice, IndexPriceUpdate);
impl_data_conversions!(FundingRate, FundingRateUpdate);
impl_data_conversions!(OptionGreeks, OptionGreeks);
impl_data_conversions!(InstrumentStatus, InstrumentStatus);
impl_data_conversions!(InstrumentClose, InstrumentClose);

/// Converts a vector of `Data` items to a specific variant type.
///
/// Filters and converts the data vector, keeping only items that can be
/// successfully converted to the target type `T`.
#[must_use]
pub fn to_variant<T: TryFrom<Data>>(data: Vec<Data>) -> Vec<T> {
    data.into_iter()
        .filter_map(|d| T::try_from(d).ok())
        .collect()
}

fn to_variant_for_batch<T: TryFrom<Data>>(
    data_type: &NautilusDataType,
    data: Vec<Data>,
) -> anyhow::Result<Vec<T>> {
    let expected_len = data.len();
    let converted = to_variant(data);
    anyhow::ensure!(
        converted.len() == expected_len,
        "catalog query for {data_type} returned rows with another data type",
    );
    Ok(converted)
}

impl Data {
    /// Returns the instrument ID for the data.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        DataRef::from(self).instrument_id()
    }

    /// Returns whether the data is a type of order book data.
    #[must_use]
    pub fn is_order_book_data(&self) -> bool {
        DataRef::from(self).is_order_book_data()
    }
}

impl From<InstrumentAny> for Data {
    fn from(value: InstrumentAny) -> Self {
        Self::Instrument(Box::new(value))
    }
}

/// Marker trait for types that carry a creation timestamp.
///
/// `ts_init` is the moment (UNIX nanoseconds) when this value was first generated or
/// ingested by Nautilus. It can be used for sequencing, latency measurements,
/// or monitoring data-pipeline delays.
pub trait HasTsInit {
    /// Returns the UNIX timestamp (nanoseconds) when the instance was created.
    fn ts_init(&self) -> UnixNanos;
}

impl HasTsInit for Data {
    fn ts_init(&self) -> UnixNanos {
        DataRef::from(self).ts_init()
    }
}

/// Checks if the data slice is monotonically increasing by initialization timestamp.
///
/// Returns `true` if each element's `ts_init` is less than or equal to the next element's `ts_init`.
pub fn is_monotonically_increasing_by_init<T: HasTsInit>(data: &[T]) -> bool {
    data.array_windows()
        .all(|[a, b]| a.ts_init() <= b.ts_init())
}

impl From<OrderBookDeltas> for Data {
    fn from(value: OrderBookDeltas) -> Self {
        Self::BookDeltas(Box::new(value))
    }
}

impl From<OrderBookDepth> for Data {
    fn from(value: OrderBookDepth) -> Self {
        Self::BookDepth(Box::new(value))
    }
}

#[cfg(feature = "defi")]
impl From<DefiData> for Data {
    fn from(value: DefiData) -> Self {
        Self::Defi(Box::new(value))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::*;

    use super::*;
    use crate::instruments::stubs::crypto_perpetual_ethusdt;

    #[rstest]
    fn test_depth_family_matches_both_macro_forms() {
        macro_rules! family_rows {
            (($marker:ident); $($rows:tt)*) => { family_rows!($($rows)*) };
            ($(($family:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
                vec![$((stringify!($family), stringify!($type), stringify!($data), stringify!($batch), $prefix)),+]
            };
        }
        let rows = for_each_data_type!(family_rows);
        let rows_with_args = for_each_data_type!(family_rows, context);
        let depth = rows
            .iter()
            .find(|row| row.4 == "order_book_depths")
            .copied()
            .unwrap();
        assert_eq!(rows, rows_with_args);
        assert_eq!(
            depth,
            (
                "OrderBookDepth",
                "OrderBookDepth",
                "BookDepth",
                "BookDepth",
                "order_book_depths"
            )
        );
    }

    #[rstest]
    fn data_instrument_json_roundtrips() {
        let data = Data::from(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));

        let encoded = serde_json::to_string(&data).unwrap();
        let decoded = serde_json::from_str::<Data>(&encoded).unwrap();

        assert_eq!(decoded, data);
    }

    #[rstest]
    #[case(NautilusDataType::Instrument, "Instrument")]
    #[case(NautilusDataType::OrderBook, "OrderBook")]
    #[case(NautilusDataType::QuoteTick, "QuoteTick")]
    #[case(NautilusDataType::OrderBookDelta, "OrderBookDelta")]
    #[case(
        NautilusDataType::Custom {
            type_name: "Example".to_string()
        },
        "Custom:Example"
    )]
    fn nautilus_data_type_display_from_str_roundtrips(
        #[case] nautilus_data_type: NautilusDataType,
        #[case] expected: &str,
    ) {
        assert_eq!(nautilus_data_type.to_string(), expected);
        assert_eq!(
            expected.parse::<NautilusDataType>().unwrap(),
            nautilus_data_type
        );
    }

    #[rstest]
    #[case("instruments", NautilusDataType::Instrument)]
    #[case("order_book", NautilusDataType::OrderBook)]
    #[case("quotes", NautilusDataType::QuoteTick)]
    #[case("order_book_deltas", NautilusDataType::OrderBookDelta)]
    #[case("instrument_closes", NautilusDataType::InstrumentClose)]
    // Plural class names, so callers can pass a Nautilus type's own name.
    #[case("OrderBookDeltas", NautilusDataType::OrderBookDelta)]
    #[case("OrderBookDepth", NautilusDataType::OrderBookDepth)]
    fn nautilus_data_type_storage_names_parse(
        #[case] value: &str,
        #[case] expected: NautilusDataType,
    ) {
        assert_eq!(value.parse::<NautilusDataType>().unwrap(), expected);
    }

    #[rstest]
    #[case(NautilusRecordType::AccountState, "AccountState")]
    #[case(NautilusRecordType::OrderFilled, "OrderFilled")]
    #[case(NautilusRecordType::OrderFillVoided, "OrderFillVoided")]
    #[case(NautilusRecordType::ExecutionMassStatus, "ExecutionMassStatus")]
    fn nautilus_record_type_display_from_str_roundtrips(
        #[case] record_type: NautilusRecordType,
        #[case] expected: &str,
    ) {
        assert_eq!(record_type.to_string(), expected);
        assert_eq!(expected.parse::<NautilusRecordType>().unwrap(), record_type);
    }

    #[rstest]
    #[case("account_state", NautilusRecordType::AccountState)]
    #[case("order_filled", NautilusRecordType::OrderFilled)]
    #[case("order_fill_voided", NautilusRecordType::OrderFillVoided)]
    #[case("position_snapshot", NautilusRecordType::PositionSnapshot)]
    fn nautilus_record_type_storage_names_parse(
        #[case] value: &str,
        #[case] expected: NautilusRecordType,
    ) {
        assert_eq!(value.parse::<NautilusRecordType>().unwrap(), expected);
    }

    #[rstest]
    fn data_batches_group_mixed_rows_in_first_seen_order() {
        let data = vec![
            Data::Quote(QuoteTick::default()),
            Data::Trade(TradeTick::default()),
            Data::Quote(QuoteTick::default()),
        ];

        let batches = DataBatch::from_data_vec_grouped(&data).unwrap();

        assert_eq!(batches.len(), 2);
        assert!(matches!(&batches[0], DataBatch::Quote(rows) if rows.len() == 2));
        assert!(matches!(&batches[1], DataBatch::Trade(rows) if rows.len() == 1));
    }

    #[rstest]
    fn data_batches_preserve_order_book_delta_event_batches() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let delta = OrderBookDelta::clear(instrument_id, 1, UnixNanos::from(2), UnixNanos::from(3));
        let data = vec![Data::BookDeltas(Box::new(OrderBookDeltas::new(
            instrument_id,
            vec![delta],
        )))];

        let batches = DataBatch::from_data_vec_grouped(&data).unwrap();

        assert_eq!(batches.len(), 1);
        assert!(
            matches!(&batches[0], DataBatch::BookDeltas(rows) if rows.len() == 1 && rows[0].deltas == vec![delta])
        );
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
}
