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

//! Data types for the trading domain model.

pub mod bar;
pub mod bet;
pub mod black_scholes;
pub mod close;
pub mod custom;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod forward;
pub mod funding;
pub mod greeks;
pub mod option_chain;
pub mod order;
pub mod prices;
pub mod quote;
pub mod registry;
pub mod status;
pub mod trade;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

use std::{
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{Params, UnixNanos};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, to_string};

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
pub use delta::OrderBookDelta;
pub use deltas::{OrderBookDeltas, OrderBookDeltas_API};
pub use depth::{DEPTH10_LEN, OrderBookDepth10};
pub use forward::ForwardPrice;
pub use funding::FundingRateUpdate;
pub use greeks::{
    BlackScholesGreeksResult, GreeksData, HasGreeks, OptionGreekValues, PortfolioGreeks,
    YieldCurveData, black_scholes_greeks, imply_vol_and_greeks, refine_vol_and_greeks,
};
pub use option_chain::{OptionChainSlice, OptionGreeks, OptionStrikeData, StrikeRange};
pub use order::{BookOrder, NULL_ORDER};
pub use prices::{IndexPriceUpdate, MarkPriceUpdate};
pub use quote::QuoteTick;
pub use registry::{
    ArrowDecoder, ArrowEncoder, decode_custom_from_arrow, deserialize_custom_from_json,
    encode_custom_to_arrow, ensure_arrow_registered, ensure_json_deserializer_registered,
    get_arrow_schema, register_arrow, register_json_deserializer,
};
#[cfg(feature = "python")]
pub use registry::{
    PyExtractor, ensure_py_extractor_registered, ensure_rust_extractor_factory_registered,
    ensure_rust_extractor_registered, get_rust_extractor, register_py_extractor,
    register_rust_extractor, register_rust_extractor_factory, try_extract_from_py,
};
pub use status::InstrumentStatus;
pub use trade::TradeTick;

use crate::identifiers::{InstrumentId, Venue};
/// A built-in Nautilus data type.
///
/// Not recommended for storing large amounts of data, as the largest variant is significantly
/// larger (10x) than the smallest.
#[derive(Debug)]
pub enum Data {
    Delta(OrderBookDelta),
    Deltas(OrderBookDeltas_API),
    Depth10(Box<OrderBookDepth10>), // This variant is significantly larger
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
    MarkPriceUpdate(MarkPriceUpdate), // TODO: Rename to MarkPrice once Cython gone
    IndexPriceUpdate(IndexPriceUpdate), // TODO: Rename to IndexPrice once Cython gone
    InstrumentStatus(InstrumentStatus),
    InstrumentClose(InstrumentClose),
    Custom(CustomData),
}

/// A C-compatible representation of [`Data`] for FFI.
///
/// This enum matches the standard variants of [`Data`] but excludes the `Custom`
/// variant which is not FFI-safe.
#[cfg(feature = "ffi")]
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum DataFFI {
    Delta(OrderBookDelta),
    Deltas(OrderBookDeltas_API),
    Depth10(Box<OrderBookDepth10>),
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
    MarkPriceUpdate(MarkPriceUpdate),
    IndexPriceUpdate(IndexPriceUpdate),
    InstrumentClose(InstrumentClose),
}

#[cfg(feature = "ffi")]
impl TryFrom<Data> for DataFFI {
    type Error = anyhow::Error;

    fn try_from(value: Data) -> Result<Self, Self::Error> {
        match value {
            Data::Delta(x) => Ok(Self::Delta(x)),
            Data::Deltas(x) => Ok(Self::Deltas(x)),
            Data::Depth10(x) => Ok(Self::Depth10(x)),
            Data::Quote(x) => Ok(Self::Quote(x)),
            Data::Trade(x) => Ok(Self::Trade(x)),
            Data::Bar(x) => Ok(Self::Bar(x)),
            Data::MarkPriceUpdate(x) => Ok(Self::MarkPriceUpdate(x)),
            Data::IndexPriceUpdate(x) => Ok(Self::IndexPriceUpdate(x)),
            Data::InstrumentStatus(_) => {
                anyhow::bail!("Cannot convert Data::InstrumentStatus to DataFFI")
            }
            Data::InstrumentClose(x) => Ok(Self::InstrumentClose(x)),
            Data::Custom(_) => anyhow::bail!("Cannot convert Data::Custom to DataFFI"),
        }
    }
}

#[cfg(feature = "ffi")]
impl From<DataFFI> for Data {
    fn from(value: DataFFI) -> Self {
        match value {
            DataFFI::Delta(x) => Self::Delta(x),
            DataFFI::Deltas(x) => Self::Deltas(x),
            DataFFI::Depth10(x) => Self::Depth10(x),
            DataFFI::Quote(x) => Self::Quote(x),
            DataFFI::Trade(x) => Self::Trade(x),
            DataFFI::Bar(x) => Self::Bar(x),
            DataFFI::MarkPriceUpdate(x) => Self::MarkPriceUpdate(x),
            DataFFI::IndexPriceUpdate(x) => Self::IndexPriceUpdate(x),
            DataFFI::InstrumentClose(x) => Self::InstrumentClose(x),
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
            .ok_or_else(|| D::Error::custom("Missing 'type' field in Data"))?
            .to_string();

        match type_name.as_str() {
            "OrderBookDelta" => Ok(Self::Delta(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "OrderBookDeltas" => Ok(Self::Deltas(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "OrderBookDepth10" => Ok(Self::Depth10(
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
            "MarkPriceUpdate" => Ok(Self::MarkPriceUpdate(
                serde_json::from_value(value).map_err(D::Error::custom)?,
            )),
            "IndexPriceUpdate" => Ok(Self::IndexPriceUpdate(
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
                    deserialize_custom_from_json(&type_name, &value).map_err(D::Error::custom)?
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
            Self::Delta(x) => Self::Delta(*x),
            Self::Deltas(x) => Self::Deltas(x.clone()),
            Self::Depth10(x) => Self::Depth10(x.clone()),
            Self::Quote(x) => Self::Quote(*x),
            Self::Trade(x) => Self::Trade(*x),
            Self::Bar(x) => Self::Bar(*x),
            Self::MarkPriceUpdate(x) => Self::MarkPriceUpdate(*x),
            Self::IndexPriceUpdate(x) => Self::IndexPriceUpdate(*x),
            Self::InstrumentStatus(x) => Self::InstrumentStatus(*x),
            Self::InstrumentClose(x) => Self::InstrumentClose(*x),
            Self::Custom(x) => Self::Custom(x.clone()),
        }
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Delta(a), Self::Delta(b)) => a == b,
            (Self::Deltas(a), Self::Deltas(b)) => a == b,
            (Self::Depth10(a), Self::Depth10(b)) => a == b,
            (Self::Quote(a), Self::Quote(b)) => a == b,
            (Self::Trade(a), Self::Trade(b)) => a == b,
            (Self::Bar(a), Self::Bar(b)) => a == b,
            (Self::MarkPriceUpdate(a), Self::MarkPriceUpdate(b)) => a == b,
            (Self::IndexPriceUpdate(a), Self::IndexPriceUpdate(b)) => a == b,
            (Self::InstrumentStatus(a), Self::InstrumentStatus(b)) => a == b,
            (Self::InstrumentClose(a), Self::InstrumentClose(b)) => a == b,
            (Self::Custom(a), Self::Custom(b)) => a == b,
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
            Self::Delta(x) => x.serialize(serializer),
            Self::Deltas(x) => x.serialize(serializer),
            Self::Depth10(x) => x.serialize(serializer),
            Self::Quote(x) => x.serialize(serializer),
            Self::Trade(x) => x.serialize(serializer),
            Self::Bar(x) => x.serialize(serializer),
            Self::MarkPriceUpdate(x) => x.serialize(serializer),
            Self::IndexPriceUpdate(x) => x.serialize(serializer),
            Self::InstrumentStatus(x) => x.serialize(serializer),
            Self::InstrumentClose(x) => x.serialize(serializer),
            Self::Custom(x) => x.serialize(serializer),
        }
    }
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
impl_try_from_data!(InstrumentStatus, InstrumentStatus);
impl_try_from_data!(InstrumentClose, InstrumentClose);

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

impl Data {
    /// Returns the instrument ID for the data.
    #[must_use]
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
        }
    }

    /// Returns whether the data is a type of order book data.
    #[must_use]
    pub fn is_order_book_data(&self) -> bool {
        matches!(self, Self::Delta(_) | Self::Deltas(_) | Self::Depth10(_))
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

/// Trait for data types that have a catalog path prefix.
pub trait CatalogPathPrefix {
    /// Returns the path prefix (directory name) for this data type.
    fn path_prefix() -> &'static str;
}

/// Macro for implementing [`CatalogPathPrefix`] for data types.
///
/// This macro provides a convenient way to implement the trait for multiple types
/// with their corresponding path prefixes.
///
/// # Parameters
///
/// - `$type`: The data type to implement the trait for.
/// - `$path`: The path prefix string for that type.
#[macro_export]
macro_rules! impl_catalog_path_prefix {
    ($type:ty, $path:expr) => {
        impl $crate::data::CatalogPathPrefix for $type {
            fn path_prefix() -> &'static str {
                $path
            }
        }
    };
}

// Standard implementations for financial data types
impl_catalog_path_prefix!(QuoteTick, "quotes");
impl_catalog_path_prefix!(TradeTick, "trades");
impl_catalog_path_prefix!(OrderBookDelta, "order_book_deltas");
impl_catalog_path_prefix!(OrderBookDepth10, "order_book_depths");
impl_catalog_path_prefix!(Bar, "bars");
impl_catalog_path_prefix!(IndexPriceUpdate, "index_prices");
impl_catalog_path_prefix!(MarkPriceUpdate, "mark_prices");
impl_catalog_path_prefix!(FundingRateUpdate, "funding_rate_update");
impl_catalog_path_prefix!(InstrumentStatus, "instrument_status");
impl_catalog_path_prefix!(InstrumentClose, "instrument_closes");

use crate::instruments::InstrumentAny;
impl_catalog_path_prefix!(InstrumentAny, "instruments");

impl HasTsInit for Data {
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
            Self::InstrumentStatus(s) => s.ts_init,
            Self::InstrumentClose(c) => c.ts_init,
            Self::Custom(c) => c.data.ts_init(),
        }
    }
}

/// Checks if the data slice is monotonically increasing by initialization timestamp.
///
/// Returns `true` if each element's `ts_init` is less than or equal to the next element's `ts_init`.
pub fn is_monotonically_increasing_by_init<T: HasTsInit>(data: &[T]) -> bool {
    data.array_windows()
        .all(|[a, b]| a.ts_init() <= b.ts_init())
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

impl From<InstrumentStatus> for Data {
    fn from(value: InstrumentStatus) -> Self {
        Self::InstrumentStatus(value)
    }
}

impl From<InstrumentClose> for Data {
    fn from(value: InstrumentClose) -> Self {
        Self::InstrumentClose(value)
    }
}

/// Builds a string-only view of a JSON value for use in topic (key=value).
fn value_to_topic_string(v: &JsonValue) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }

    if let Some(n) = v.as_u64() {
        return n.to_string();
    }

    if let Some(n) = v.as_i64() {
        return n.to_string();
    }

    if let Some(b) = v.as_bool() {
        return b.to_string();
    }

    if let Some(f) = v.as_f64() {
        return f.to_string();
    }

    if v.is_null() {
        return "null".to_string();
    }
    serde_json::to_string(v).unwrap_or_default()
}

/// Builds the topic suffix from Params (string-only view: key=value joined by ".").
fn params_to_topic_suffix(params: &Params) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}={}", value_to_topic_string(v)))
        .collect::<Vec<_>>()
        .join(".")
}

/// Represents a data type including metadata.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct DataType {
    type_name: String,
    metadata: Option<Params>,
    topic: String,
    hash: u64,
    identifier: Option<String>,
}

impl DataType {
    /// Creates a new [`DataType`] instance.
    #[must_use]
    pub fn new(type_name: &str, metadata: Option<Params>, identifier: Option<String>) -> Self {
        // Precompute topic from type_name + metadata (string-only view for backward compatibility)
        let topic = if let Some(ref meta) = metadata {
            if meta.is_empty() {
                type_name.to_string()
            } else {
                format!("{type_name}.{}", params_to_topic_suffix(meta))
            }
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
            identifier,
        }
    }

    /// Creates a [`DataType`] from persisted parts (`type_name`, topic, metadata).
    /// Hash is recomputed from topic. Use when restoring from legacy `data_type` column.
    /// Identifier is set to None.
    #[must_use]
    pub fn from_parts(type_name: &str, topic: &str, metadata: Option<Params>) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        topic.hash(&mut hasher);
        Self {
            type_name: type_name.to_owned(),
            metadata,
            topic: topic.to_owned(),
            hash: hasher.finish(),
            identifier: None,
        }
    }

    /// Serializes to JSON for persistence (`type_name`, metadata, identifier; no topic, no hash).
    ///
    /// # Errors
    ///
    /// Returns a JSON serialization error if the data cannot be serialized.
    pub fn to_persistence_json(&self) -> Result<String, serde_json::Error> {
        let mut map = serde_json::Map::new();
        map.insert(
            "type_name".to_string(),
            serde_json::Value::String(self.type_name.clone()),
        );
        map.insert(
            "metadata".to_string(),
            self.metadata.as_ref().map_or(serde_json::Value::Null, |m| {
                serde_json::to_value(m).unwrap_or(serde_json::Value::Null)
            }),
        );

        if let Some(ref id) = self.identifier {
            map.insert(
                "identifier".to_string(),
                serde_json::Value::String(id.clone()),
            );
        }
        serde_json::to_string(&serde_json::Value::Object(map))
    }

    /// Deserializes from JSON produced by `to_persistence_json`.
    /// Accepts legacy JSON with `topic` (ignored); topic is rebuilt from `type_name` + metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid JSON or missing required fields.
    pub fn from_persistence_json(s: &str) -> Result<Self, anyhow::Error> {
        let value: serde_json::Value =
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Invalid data_type JSON: {e}"))?;
        let obj = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("data_type must be a JSON object"))?;
        let type_name = obj
            .get("type_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("data_type must have type_name"))?
            .to_string();
        let metadata = obj.get("metadata").and_then(|m| {
            if m.is_null() {
                None
            } else {
                let p: Params = serde_json::from_value(m.clone()).ok()?;
                if p.is_empty() { None } else { Some(p) }
            }
        });
        let identifier = obj
            .get("identifier")
            .and_then(|v| v.as_str())
            .map(String::from);
        Ok(Self::new(&type_name, metadata, identifier))
    }

    /// Returns the type name for the data type.
    #[must_use]
    pub fn type_name(&self) -> &str {
        self.type_name.as_str()
    }

    /// Returns the metadata for the data type.
    #[must_use]
    pub fn metadata(&self) -> Option<&Params> {
        self.metadata.as_ref()
    }

    /// Returns a string representation of the metadata.
    #[must_use]
    pub fn metadata_str(&self) -> String {
        self.metadata.as_ref().map_or_else(
            || "null".to_string(),
            |metadata| to_string(metadata).unwrap_or_default(),
        )
    }

    /// Returns metadata as a string-only map (e.g. for Arrow schema metadata).
    #[must_use]
    pub fn metadata_string_map(&self) -> Option<std::collections::HashMap<String, String>> {
        self.metadata.as_ref().map(|p| {
            p.iter()
                .map(|(k, v)| (k.clone(), value_to_topic_string(v)))
                .collect()
        })
    }

    /// Returns the precomputed hash for this data type.
    #[must_use]
    pub fn precomputed_hash(&self) -> u64 {
        self.hash
    }

    /// Returns the messaging topic for the data type.
    #[must_use]
    pub fn topic(&self) -> &str {
        self.topic.as_str()
    }

    /// Returns the optional catalog path identifier (can contain subdirs, e.g. `"venue//symbol"`).
    #[must_use]
    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }

    /// Returns an [`Option<InstrumentId>`] parsed from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - There is no metadata.
    /// - The `instrument_id` value contained in the metadata is invalid.
    #[must_use]
    pub fn instrument_id(&self) -> Option<InstrumentId> {
        let metadata = self.metadata.as_ref().expect("metadata was `None`");
        let instrument_id = metadata.get_str("instrument_id")?;
        Some(
            InstrumentId::from_str(instrument_id)
                .expect("Invalid `InstrumentId` for 'instrument_id'"),
        )
    }

    /// Returns an [`Option<Venue>`] parsed from the metadata.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - There is no metadata.
    /// - The `venue` value contained in the metadata is invalid.
    #[must_use]
    pub fn venue(&self) -> Option<Venue> {
        let metadata = self.metadata.as_ref().expect("metadata was `None`");
        let venue_str = metadata.get_str("venue")?;
        Some(Venue::from(venue_str))
    }

    /// Returns an [`Option<UnixNanos>`] parsed from the metadata `start` field.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - There is no metadata.
    /// - The `start` value contained in the metadata is invalid.
    #[must_use]
    pub fn start(&self) -> Option<UnixNanos> {
        let metadata = self.metadata.as_ref()?;
        let start_str = metadata.get_str("start")?;
        Some(UnixNanos::from_str(start_str).expect("Invalid `UnixNanos` for 'start'"))
    }

    /// Returns an [`Option<UnixNanos>`] parsed from the metadata `end` field.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - There is no metadata.
    /// - The `end` value contained in the metadata is invalid.
    #[must_use]
    pub fn end(&self) -> Option<UnixNanos> {
        let metadata = self.metadata.as_ref()?;
        let end_str = metadata.get_str("end")?;
        Some(UnixNanos::from_str(end_str).expect("Invalid `UnixNanos` for 'end'"))
    }

    /// Returns an [`Option<usize>`] parsed from the metadata `limit` field.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - There is no metadata.
    /// - The `limit` value contained in the metadata is invalid.
    #[must_use]
    pub fn limit(&self) -> Option<usize> {
        let metadata = self.metadata.as_ref()?;
        metadata.get_usize("limit").or_else(|| {
            metadata
                .get_str("limit")
                .map(|s| s.parse::<usize>().expect("Invalid `usize` for 'limit'"))
        })
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
            "DataType(type_name={}, metadata={:?}, identifier={:?})",
            self.type_name, self.metadata, self.identifier
        )
    }
}

#[cfg(test)]
mod tests {
    use std::hash::DefaultHasher;

    use rstest::*;
    use serde_json::json;

    use super::*;

    fn params_from_json(value: serde_json::Value) -> Params {
        serde_json::from_value(value).expect("valid Params JSON")
    }

    #[rstest]
    fn test_data_type_creation_with_metadata() {
        let metadata = Some(params_from_json(
            json!({"key1": "value1", "key2": "value2"}),
        ));
        let data_type = DataType::new("ExampleType", metadata.clone(), None);

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType.key1=value1.key2=value2");
        assert_eq!(data_type.metadata(), metadata.as_ref());
    }

    #[rstest]
    fn test_data_type_creation_without_metadata() {
        let data_type = DataType::new("ExampleType", None, None);

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType");
        assert_eq!(data_type.metadata(), None);
    }

    #[rstest]
    fn test_data_type_equality() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key1": "value1"})));

        let data_type1 = DataType::new("ExampleType", metadata1, None);
        let data_type2 = DataType::new("ExampleType", metadata2, None);

        assert_eq!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_inequality() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key2": "value2"})));

        let data_type1 = DataType::new("ExampleType", metadata1, None);
        let data_type2 = DataType::new("ExampleType", metadata2, None);

        assert_ne!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_ordering() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key2": "value2"})));

        let data_type1 = DataType::new("ExampleTypeA", metadata1, None);
        let data_type2 = DataType::new("ExampleTypeB", metadata2, None);

        assert!(data_type1 < data_type2);
    }

    #[rstest]
    fn test_data_type_hash() {
        let metadata = Some(params_from_json(json!({"key1": "value1"})));

        let data_type1 = DataType::new("ExampleType", metadata.clone(), None);
        let data_type2 = DataType::new("ExampleType", metadata, None);

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
        let metadata = Some(params_from_json(json!({"key1": "value1"})));
        let data_type = DataType::new("ExampleType", metadata, None);

        assert_eq!(format!("{data_type}"), "ExampleType.key1=value1");
    }

    #[rstest]
    fn test_data_type_debug() {
        let metadata = Some(params_from_json(json!({"key1": "value1"})));
        let data_type = DataType::new("ExampleType", metadata.clone(), None);

        assert_eq!(
            format!("{data_type:?}"),
            format!("DataType(type_name=ExampleType, metadata={metadata:?}, identifier=None)")
        );
    }

    #[rstest]
    fn test_parse_instrument_id_from_metadata() {
        let instrument_id_str = "MSFT.XNAS";
        let metadata = Some(params_from_json(
            json!({"instrument_id": instrument_id_str}),
        ));
        let data_type = DataType::new("InstrumentAny", metadata, None);

        assert_eq!(
            data_type.instrument_id().unwrap(),
            InstrumentId::from_str(instrument_id_str).unwrap()
        );
    }

    #[rstest]
    fn test_parse_venue_from_metadata() {
        let venue_str = "BINANCE";
        let metadata = Some(params_from_json(json!({"venue": venue_str})));
        let data_type = DataType::new(stringify!(InstrumentAny), metadata, None);

        assert_eq!(data_type.venue().unwrap(), Venue::new(venue_str));
    }

    #[rstest]
    fn test_parse_start_from_metadata() {
        let start_ns = 1_600_054_595_844_758_000;
        let metadata = Some(params_from_json(json!({"start": start_ns.to_string()})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(data_type.start().unwrap(), UnixNanos::from(start_ns),);
    }

    #[rstest]
    fn test_parse_end_from_metadata() {
        let end_ns = 1_720_954_595_844_758_000;
        let metadata = Some(params_from_json(json!({"end": end_ns.to_string()})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(data_type.end().unwrap(), UnixNanos::from(end_ns),);
    }

    #[rstest]
    fn test_parse_limit_from_metadata() {
        let limit = 1000;
        let metadata = Some(params_from_json(json!({"limit": limit})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(data_type.limit().unwrap(), limit);
    }

    #[rstest]
    fn test_data_type_persistence_json_with_identifier() {
        let data_type = DataType::new("MyCustomType", None, Some("venue//symbol".to_string()));
        let json = data_type.to_persistence_json().unwrap();
        assert!(!json.contains("topic"));
        assert!(json.contains("\"identifier\":\"venue//symbol\""));
        let restored = DataType::from_persistence_json(&json).unwrap();
        assert_eq!(restored.type_name(), "MyCustomType");
        assert_eq!(restored.identifier(), Some("venue//symbol"));
        assert_eq!(restored.topic(), "MyCustomType");
    }

    #[rstest]
    fn test_data_type_identifier_getter() {
        let data_type = DataType::new("T", None, Some("id".to_string()));
        assert_eq!(data_type.identifier(), Some("id"));
        let data_type_no_id = DataType::new("T", None, None);
        assert_eq!(data_type_no_id.identifier(), None);
    }
}
