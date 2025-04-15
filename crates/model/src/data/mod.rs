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

//! Data types for the trading domain model.

pub mod bar;
pub mod bet;
pub mod close;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod greeks;
pub mod order;
pub mod prices;
pub mod quote;
pub mod status;
pub mod trade;

#[cfg(feature = "stubs")]
pub mod stubs;

use std::{
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    str::FromStr,
};

use close::InstrumentClose;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use serde_json::to_string;

// Re-exports
#[rustfmt::skip]  // Keep these grouped
pub use bar::{Bar, BarSpecification, BarType};
pub use delta::OrderBookDelta;
pub use deltas::{OrderBookDeltas, OrderBookDeltas_API};
pub use depth::{DEPTH10_LEN, OrderBookDepth10};
pub use greeks::{
    BlackScholesGreeksResult, GreeksData, PortfolioGreeks, YieldCurveData, black_scholes_greeks,
    imply_vol_and_greeks,
};
pub use order::{BookOrder, NULL_ORDER};
pub use prices::{IndexPriceUpdate, MarkPriceUpdate};
pub use quote::QuoteTick;
pub use status::InstrumentStatus;
pub use trade::TradeTick;

use crate::identifiers::{InstrumentId, Venue};

/// A built-in Nautilus data type.
///
/// Not recommended for storing large amounts of data, as the largest variant is significantly
/// larger (10x) than the smallest.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Data {
    Delta(OrderBookDelta),
    Deltas(OrderBookDeltas_API),
    Depth10(Box<OrderBookDepth10>), // This variant is significantly larger
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
    MarkPriceUpdate(MarkPriceUpdate), // TODO: Rename to MarkPrice once Cython gone
    IndexPriceUpdate(IndexPriceUpdate), // TODO: Rename to IndexPrice once Cython gone
    InstrumentClose(InstrumentClose),
}

macro_rules! impl_try_from_data {
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
    };
}

impl TryFrom<Data> for OrderBookDepth10 {
    type Error = ();

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        match value {
            Data::Depth10(x) => Ok(*x),
            _ => Err(()),
        }
    }
}

impl_try_from_data!(Quote, QuoteTick);
impl_try_from_data!(Delta, OrderBookDelta);
impl_try_from_data!(Deltas, OrderBookDeltas_API);
impl_try_from_data!(Trade, TradeTick);
impl_try_from_data!(Bar, Bar);
impl_try_from_data!(MarkPriceUpdate, MarkPriceUpdate);
impl_try_from_data!(IndexPriceUpdate, IndexPriceUpdate);
impl_try_from_data!(InstrumentClose, InstrumentClose);

pub fn to_variant<T: TryFrom<Data>>(data: Vec<Data>) -> Vec<T> {
    data.into_iter()
        .filter_map(|d| T::try_from(d).ok())
        .collect()
}

impl Data {
    /// Returns the instrument ID for the data.
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Delta(delta) => delta.instrument_id,
            Self::Deltas(deltas) => deltas.instrument_id,
            Self::Depth10(depth) => depth.instrument_id,
            Self::Quote(quote) => quote.instrument_id,
            Self::Trade(trade) => trade.instrument_id,
            Self::Bar(bar) => bar.bar_type.instrument_id(),
            Self::MarkPriceUpdate(mark_price) => mark_price.instrument_id,
            Self::IndexPriceUpdate(index_price) => index_price.instrument_id,
            Self::InstrumentClose(close) => close.instrument_id,
        }
    }

    /// Returns whether the data is a type of order book data.
    pub fn is_order_book_data(&self) -> bool {
        matches!(self, Self::Delta(_) | Self::Deltas(_) | Self::Depth10(_))
    }
}

pub trait GetTsInit {
    fn ts_init(&self) -> UnixNanos;
}

impl GetTsInit for Data {
    fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Delta(d) => d.ts_init,
            Self::Deltas(d) => d.ts_init,
            Self::Depth10(d) => d.ts_init,
            Self::Quote(q) => q.ts_init,
            Self::Trade(t) => t.ts_init,
            Self::Bar(b) => b.ts_init,
            Self::MarkPriceUpdate(p) => p.ts_init,
            Self::IndexPriceUpdate(p) => p.ts_init,
            Self::InstrumentClose(c) => c.ts_init,
        }
    }
}

pub fn is_monotonically_increasing_by_init<T: GetTsInit>(data: &[T]) -> bool {
    data.windows(2)
        .all(|window| window[0].ts_init() <= window[1].ts_init())
}

impl From<OrderBookDelta> for Data {
    fn from(value: OrderBookDelta) -> Self {
        Self::Delta(value)
    }
}

impl From<OrderBookDeltas_API> for Data {
    fn from(value: OrderBookDeltas_API) -> Self {
        Self::Deltas(value)
    }
}

impl From<OrderBookDepth10> for Data {
    fn from(value: OrderBookDepth10) -> Self {
        Self::Depth10(Box::new(value))
    }
}

impl From<QuoteTick> for Data {
    fn from(value: QuoteTick) -> Self {
        Self::Quote(value)
    }
}

impl From<TradeTick> for Data {
    fn from(value: TradeTick) -> Self {
        Self::Trade(value)
    }
}

impl From<Bar> for Data {
    fn from(value: Bar) -> Self {
        Self::Bar(value)
    }
}

impl From<MarkPriceUpdate> for Data {
    fn from(value: MarkPriceUpdate) -> Self {
        Self::MarkPriceUpdate(value)
    }
}

impl From<IndexPriceUpdate> for Data {
    fn from(value: IndexPriceUpdate) -> Self {
        Self::IndexPriceUpdate(value)
    }
}

impl From<InstrumentClose> for Data {
    fn from(value: InstrumentClose) -> Self {
        Self::InstrumentClose(value)
    }
}

// TODO: https://blog.rust-lang.org/2024/03/30/i128-layout-update.html
// i128 and u128 is now FFI compatible. However, since the clippy lint
// hasn't been removed yet. We'll suppress with #[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
#[unsafe(no_mangle)]
#[cfg_attr(feature = "high-precision", allow(improper_ctypes_definitions))]
pub extern "C" fn data_clone(data: &Data) -> Data {
    // Dummy function for cbindgen to export types
    data.clone()
}

/// Represents a data type including metadata.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct DataType {
    type_name: String,
    metadata: Option<IndexMap<String, String>>,
    topic: String,
    hash: u64,
}

impl DataType {
    /// Creates a new [`DataType`] instance.
    pub fn new(type_name: &str, metadata: Option<IndexMap<String, String>>) -> Self {
        // Precompute topic
        let topic = if let Some(ref meta) = metadata {
            let meta_str = meta
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(".");
            format!("{}.{}", type_name, meta_str)
        } else {
            type_name.to_string()
        };

        // Precompute hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        topic.hash(&mut hasher);

        Self {
            type_name: type_name.to_owned(),
            metadata,
            topic,
            hash: hasher.finish(),
        }
    }

    /// Returns the type name for the data type.
    pub fn type_name(&self) -> &str {
        self.type_name.as_str()
    }

    /// Returns the metadata for the data type.
    pub fn metadata(&self) -> Option<&IndexMap<String, String>> {
        self.metadata.as_ref()
    }

    /// Returns a string representation of the metadata.
    pub fn metadata_str(&self) -> String {
        self.metadata
            .as_ref()
            .map(|metadata| to_string(metadata).unwrap_or_default())
            .unwrap_or_else(|| "null".to_string())
    }

    /// Returns the messaging topic for the data type.
    pub fn topic(&self) -> &str {
        self.topic.as_str()
    }

    /// Returns an [`Option<InstrumentId>`] from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If there is no metadata.
    /// - If the `instrument_id` value contained in the metadata is invalid.
    pub fn instrument_id(&self) -> Option<InstrumentId> {
        let metadata = self.metadata.as_ref().expect("metadata was `None`");
        let instrument_id = metadata.get("instrument_id")?;
        Some(
            InstrumentId::from_str(instrument_id)
                .expect("Invalid `InstrumentId` for 'instrument_id'"),
        )
    }

    /// Returns an [`Option<Venue>`] from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If there is no metadata.
    /// - If the `venue` value contained in the metadata is invalid.
    pub fn venue(&self) -> Option<Venue> {
        let metadata = self.metadata.as_ref().expect("metadata was `None`");
        let venue_str = metadata.get("venue")?;
        Some(Venue::from(venue_str.as_str()))
    }

    /// Returns an [`Option<UnixNanos>`] start from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If there is no metadata.
    /// - If the `start` value contained in the metadata is invalid.
    pub fn start(&self) -> Option<UnixNanos> {
        let metadata = self.metadata.as_ref()?;
        let start_str = metadata.get("start")?;
        Some(UnixNanos::from_str(start_str).expect("Invalid `UnixNanos` for 'start'"))
    }

    /// Returns an [`Option<UnixNanos>`] end from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If there is no metadata.
    /// - If the `end` value contained in the metadata is invalid.
    pub fn end(&self) -> Option<UnixNanos> {
        let metadata = self.metadata.as_ref()?;
        let end_str = metadata.get("end")?;
        Some(UnixNanos::from_str(end_str).expect("Invalid `UnixNanos` for 'end'"))
    }

    /// Returns an [`Option<usize>`] limit from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If there is no metadata.
    /// - If the `limit` value contained in the metadata is invalid.
    pub fn limit(&self) -> Option<usize> {
        let metadata = self.metadata.as_ref()?;
        let depth_str = metadata.get("limit")?;
        Some(
            depth_str
                .parse::<usize>()
                .expect("Invalid `usize` for 'limit'"),
        )
    }
}

impl PartialEq for DataType {
    fn eq(&self, other: &Self) -> bool {
        self.topic == other.topic
    }
}

impl Eq for DataType {}

impl PartialOrd for DataType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DataType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.topic.cmp(&other.topic)
    }
}

impl Hash for DataType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.topic)
    }
}

impl Debug for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DataType(type_name={}, metadata={:?})",
            self.type_name, self.metadata
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::hash::DefaultHasher;

    use rstest::*;

    use super::*;

    #[rstest]
    fn test_data_type_creation_with_metadata() {
        let metadata = Some(
            [
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
            ]
            .iter()
            .cloned()
            .collect(),
        );
        let data_type = DataType::new("ExampleType", metadata.clone());

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType.key1=value1.key2=value2");
        assert_eq!(data_type.metadata(), metadata.as_ref());
    }

    #[rstest]
    fn test_data_type_creation_without_metadata() {
        let data_type = DataType::new("ExampleType", None);

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType");
        assert_eq!(data_type.metadata(), None);
    }

    #[rstest]
    fn test_data_type_equality() {
        let metadata1 = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let metadata2 = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );

        let data_type1 = DataType::new("ExampleType", metadata1);
        let data_type2 = DataType::new("ExampleType", metadata2);

        assert_eq!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_inequality() {
        let metadata1 = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let metadata2 = Some(
            [("key2".to_string(), "value2".to_string())]
                .iter()
                .cloned()
                .collect(),
        );

        let data_type1 = DataType::new("ExampleType", metadata1);
        let data_type2 = DataType::new("ExampleType", metadata2);

        assert_ne!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_ordering() {
        let metadata1 = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let metadata2 = Some(
            [("key2".to_string(), "value2".to_string())]
                .iter()
                .cloned()
                .collect(),
        );

        let data_type1 = DataType::new("ExampleTypeA", metadata1);
        let data_type2 = DataType::new("ExampleTypeB", metadata2);

        assert!(data_type1 < data_type2);
    }

    #[rstest]
    fn test_data_type_hash() {
        let metadata = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );

        let data_type1 = DataType::new("ExampleType", metadata.clone());
        let data_type2 = DataType::new("ExampleType", metadata.clone());

        let mut hasher1 = DefaultHasher::new();
        data_type1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        data_type2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(hash1, hash2);
    }

    #[rstest]
    fn test_data_type_display() {
        let metadata = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new("ExampleType", metadata);

        assert_eq!(format!("{}", data_type), "ExampleType.key1=value1");
    }

    #[rstest]
    fn test_data_type_debug() {
        let metadata = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new("ExampleType", metadata.clone());

        assert_eq!(
            format!("{data_type:?}"),
            format!("DataType(type_name=ExampleType, metadata={metadata:?})")
        );
    }

    #[rstest]
    fn test_parse_instrument_id_from_metadata() {
        let instrument_id_str = "MSFT.XNAS";
        let metadata = Some(
            [("instrument_id".to_string(), instrument_id_str.to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new("InstrumentAny", metadata);

        assert_eq!(
            data_type.instrument_id().unwrap(),
            InstrumentId::from_str(instrument_id_str).unwrap()
        );
    }

    #[rstest]
    fn test_parse_venue_from_metadata() {
        let venue_str = "BINANCE";
        let metadata = Some(
            [("venue".to_string(), venue_str.to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new(stringify!(InstrumentAny), metadata);

        assert_eq!(data_type.venue().unwrap(), Venue::new(venue_str));
    }

    #[rstest]
    fn test_parse_start_from_metadata() {
        let start_ns = 1600054595844758000;
        let metadata = Some(
            [("start".to_string(), start_ns.to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new(stringify!(TradeTick), metadata);

        assert_eq!(data_type.start().unwrap(), UnixNanos::from(start_ns),);
    }

    #[rstest]
    fn test_parse_end_from_metadata() {
        let end_ns = 1720954595844758000;
        let metadata = Some(
            [("end".to_string(), end_ns.to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new(stringify!(TradeTick), metadata);

        assert_eq!(data_type.end().unwrap(), UnixNanos::from(end_ns),);
    }

    #[rstest]
    fn test_parse_limit_from_metadata() {
        let limit = 1000;
        let metadata = Some(
            [("limit".to_string(), limit.to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new(stringify!(TradeTick), metadata);

        assert_eq!(data_type.limit().unwrap(), limit);
    }
}
