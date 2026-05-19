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

//! Wire-format types for the Kraken WebSocket v2 `level3` channel.

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A JSON decimal number that preserves the original wire-format string for checksum computation.
#[derive(Debug, Clone)]
pub struct RawDecimal {
    /// Parsed numeric value.
    pub value: f64,
    /// Exact JSON representation (e.g. `"79754.0"` or `"0.00040000"`).
    pub raw: String,
}

impl Default for RawDecimal {
    fn default() -> Self {
        Self {
            value: 0.0,
            raw: "0".to_string(),
        }
    }
}

impl<'de> Deserialize<'de> for RawDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        let s = raw.get();
        let raw_decimal = if s.starts_with('"') {
            serde_json::from_str::<String>(s).map_err(serde::de::Error::custom)?
        } else {
            s.to_string()
        };
        let value: f64 = raw_decimal.parse().map_err(serde::de::Error::custom)?;
        Ok(Self {
            value,
            raw: raw_decimal,
        })
    }
}

/// Individual order within a `level3` snapshot.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenL3Order {
    /// Venue-assigned order identifier.
    pub order_id: String,
    /// Limit price of the order; preserves the exact JSON string for checksum computation.
    pub limit_price: RawDecimal,
    /// Remaining quantity of the order; preserves the exact JSON string for checksum computation.
    pub order_qty: RawDecimal,
    /// Timestamp when the order was placed or last updated.
    pub timestamp: DateTime<Utc>,
}

/// Full order book snapshot from the `level3` channel.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenL3Snapshot {
    /// Trading pair symbol (e.g. `"BTC/USD"`).
    pub symbol: String,
    /// All resting bid orders at snapshot time.
    pub bids: Vec<KrakenL3Order>,
    /// All resting ask orders at snapshot time.
    pub asks: Vec<KrakenL3Order>,
    /// CRC32 checksum of the top-10 aggregated price levels.
    pub checksum: u32,
    /// Timestamp of the snapshot.
    pub timestamp: DateTime<Utc>,
}

/// Event type for an individual order in a `level3` update.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KrakenL3EventType {
    /// New order added to the book.
    Add,
    /// Existing order quantity or price changed.
    Modify,
    /// Order removed from the book.
    Delete,
}

/// Per-order event within a `level3` incremental update.
///
/// `order_qty` is absent for delete events; defaults to zero value with empty raw string.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenL3OrderEvent {
    /// Type of order event.
    pub event: KrakenL3EventType,
    /// Venue-assigned order identifier.
    pub order_id: String,
    /// Limit price for this event; preserves the exact JSON string for checksum computation.
    pub limit_price: RawDecimal,
    /// Order quantity; absent for delete events — defaults to zero.
    #[serde(default)]
    pub order_qty: RawDecimal,
    /// Timestamp of the event.
    pub timestamp: DateTime<Utc>,
}

/// Incremental update data from the `level3` channel.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenL3UpdateData {
    /// Trading pair symbol (e.g. `"BTC/USD"`).
    pub symbol: String,
    /// Bid-side order events in this update.
    pub bids: Vec<KrakenL3OrderEvent>,
    /// Ask-side order events in this update.
    pub asks: Vec<KrakenL3OrderEvent>,
    /// CRC32 checksum of the top-10 aggregated price levels after applying this update.
    pub checksum: u32,
    /// Timestamp of the update.
    pub timestamp: DateTime<Utc>,
}

/// Typed output from the L3 WebSocket handler.
#[derive(Debug, Clone)]
pub(crate) enum KrakenL3WsMessage {
    /// Full order book snapshot.
    Snapshot(KrakenL3Snapshot),
    /// Incremental order book update.
    Update(KrakenL3UpdateData),
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_raw_decimal_deserializes_json_string_without_quotes_in_raw() {
        let decimal: RawDecimal = serde_json::from_str(r#""0.01000000""#).unwrap();

        assert_eq!(decimal.value, 0.01);
        assert_eq!(decimal.raw, "0.01000000");
    }

    #[rstest]
    fn test_raw_decimal_preserves_numeric_wire_precision() {
        let decimal: RawDecimal = serde_json::from_str("42000.50000").unwrap();

        assert_eq!(decimal.value, 42000.5);
        assert_eq!(decimal.raw, "42000.50000");
    }
}
