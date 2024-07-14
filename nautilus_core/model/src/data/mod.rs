// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod order;
pub mod quote;
#[cfg(feature = "stubs")]
pub mod stubs;
pub mod trade;

use std::{
    hash::{Hash, Hasher},
    str::FromStr,
};

use indexmap::IndexMap;
use nautilus_core::nanos::UnixNanos;

use self::{
    bar::Bar, delta::OrderBookDelta, deltas::OrderBookDeltas_API, depth::OrderBookDepth10,
    quote::QuoteTick, trade::TradeTick,
};
use crate::{
    enums::BookType,
    identifiers::{InstrumentId, Venue},
};

/// A built-in Nautilus data type.
///
/// Not recommended for storing large amounts of data, as the largest variant is significantly
/// larger (10x) than the smallest.
#[repr(C)]
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Data {
    Delta(OrderBookDelta),
    Deltas(OrderBookDeltas_API),
    Depth10(OrderBookDepth10), // This variant is significantly larger
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
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
        Self::Depth10(value)
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

#[no_mangle]
pub extern "C" fn data_clone(data: &Data) -> Data {
    data.clone()
}

/// Represents a data type including metadata.
#[derive(Clone)]
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

    /// Returns the messaging topic for the data type.
    pub fn topic(&self) -> &str {
        self.topic.as_str()
    }

    pub fn parse_instrument_id_from_metadata(&self) -> anyhow::Result<Option<InstrumentId>> {
        let instrument_id_str = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("metadata was None"))?
            .get("instrument_id");

        if let Some(instrument_id_str) = instrument_id_str {
            Ok(Some(InstrumentId::from_str(instrument_id_str)?))
        } else {
            Ok(None)
        }
    }

    pub fn parse_venue_from_metadata(&self) -> anyhow::Result<Option<Venue>> {
        let venue_str = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("metadata was None"))?
            .get("venue");

        if let Some(venue_str) = venue_str {
            Ok(Some(Venue::from_str(venue_str).unwrap())) //  TODO: Propagate parsing error
        } else {
            Ok(None)
        }
    }

    pub fn parse_book_type_from_metadata(&self) -> anyhow::Result<BookType> {
        let venue_str = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("metadata was None"))?
            .get("book_type")
            .ok_or_else(|| anyhow::anyhow!("'venue' not found in metadata"))?;
        Ok(BookType::from_str(venue_str)?)
    }

    pub fn parse_depth_from_metadata(&self) -> anyhow::Result<usize> {
        let depth_str = self
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("metadata was None"))?
            .get("depth")
            .ok_or_else(|| anyhow::anyhow!("'depth' not found in metadata"))?;
        Ok(depth_str.parse::<usize>()?)
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

impl std::fmt::Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.topic)
    }
}

impl std::fmt::Debug for DataType {
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

    #[test]
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

    #[test]
    fn test_data_type_debug() {
        let metadata = Some(
            [("key1".to_string(), "value1".to_string())]
                .iter()
                .cloned()
                .collect(),
        );
        let data_type = DataType::new("ExampleType", metadata.clone());

        assert_eq!(
            format!("{:?}", data_type),
            format!("DataType(type_name=ExampleType, metadata={:?})", metadata)
        );
    }
}
