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

use std::{collections::HashMap, str::FromStr};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{delta::OrderBookDelta, deltas::OrderBookDeltas, order::BookOrder},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    http::models::{HyperliquidL2Book, HyperliquidLevel},
    websocket::messages::{WsBookData, WsLevelData},
};

/// Configuration for price/size precision when converting Hyperliquid data
#[derive(Debug, Clone)]
pub struct BookConfig {
    /// Price precision (number of decimal places)
    pub price_decimals: u8,
    /// Size precision (number of decimal places)
    pub size_decimals: u8,
}

impl BookConfig {
    /// Create config with specific precision
    pub fn new(price_decimals: u8, size_decimals: u8) -> Self {
        Self {
            price_decimals,
            size_decimals,
        }
    }

    /// Default configuration for most crypto assets
    pub fn default_crypto() -> Self {
        Self::new(2, 5) // 0.01 price precision, 0.00001 size precision
    }
}

/// Manages precision configuration and converts Hyperliquid data to standard Nautilus formats
#[derive(Debug, Default)]
pub struct HyperliquidDataConverter {
    /// Configuration by instrument symbol
    configs: HashMap<String, BookConfig>,
}

impl HyperliquidDataConverter {
    /// Create a new converter
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure precision for an instrument
    pub fn configure_instrument(&mut self, symbol: &str, config: BookConfig) {
        self.configs.insert(symbol.to_string(), config);
    }

    /// Get configuration for an instrument, using default if not configured
    fn get_config(&self, symbol: &str) -> BookConfig {
        self.configs
            .get(symbol)
            .cloned()
            .unwrap_or_else(BookConfig::default_crypto)
    }

    /// Convert Hyperliquid HTTP L2Book snapshot to OrderBookDeltas
    pub fn convert_http_snapshot(
        &self,
        data: &HyperliquidL2Book,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> Result<OrderBookDeltas, ConversionError> {
        let config = self.get_config(&data.coin);
        let mut deltas = Vec::new();

        // Add a clear delta first to reset the book
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            0,                                      // sequence starts at 0 for snapshots
            UnixNanos::from(data.time * 1_000_000), // Convert millis to nanos
            ts_init,
        ));

        let mut order_id = 1u64; // Sequential order IDs for snapshot

        // Convert bid levels
        for level in &data.levels[0] {
            let (price, size) = parse_level(level, &config)?;
            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_LAST as u8, // Mark as last for snapshot
                    order_id,
                    UnixNanos::from(data.time * 1_000_000),
                    ts_init,
                ));
                order_id += 1;
            }
        }

        // Convert ask levels
        for level in &data.levels[1] {
            let (price, size) = parse_level(level, &config)?;
            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_LAST as u8, // Mark as last for snapshot
                    order_id,
                    UnixNanos::from(data.time * 1_000_000),
                    ts_init,
                ));
                order_id += 1;
            }
        }

        Ok(OrderBookDeltas::new(instrument_id, deltas))
    }

    /// Convert Hyperliquid WebSocket book data to OrderBookDeltas
    pub fn convert_ws_snapshot(
        &self,
        data: &WsBookData,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> Result<OrderBookDeltas, ConversionError> {
        let config = self.get_config(&data.coin);
        let mut deltas = Vec::new();

        // Add a clear delta first to reset the book
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            0,                                      // sequence starts at 0 for snapshots
            UnixNanos::from(data.time * 1_000_000), // Convert millis to nanos
            ts_init,
        ));

        let mut order_id = 1u64; // Sequential order IDs for snapshot

        // Convert bid levels
        for level in &data.levels[0] {
            let (price, size) = parse_ws_level(level, &config)?;
            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_LAST as u8,
                    order_id,
                    UnixNanos::from(data.time * 1_000_000),
                    ts_init,
                ));
                order_id += 1;
            }
        }

        // Convert ask levels
        for level in &data.levels[1] {
            let (price, size) = parse_ws_level(level, &config)?;
            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    order,
                    RecordFlag::F_LAST as u8,
                    order_id,
                    UnixNanos::from(data.time * 1_000_000),
                    ts_init,
                ));
                order_id += 1;
            }
        }

        Ok(OrderBookDeltas::new(instrument_id, deltas))
    }

    /// Convert price/size changes to OrderBookDeltas
    /// This would be used for incremental WebSocket updates if Hyperliquid provided them
    #[allow(clippy::too_many_arguments)]
    pub fn convert_delta_update(
        &self,
        symbol: &str,
        instrument_id: InstrumentId,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        bid_updates: &[(String, String)], // (price, size) pairs
        ask_updates: &[(String, String)], // (price, size) pairs
        bid_removals: &[String],          // prices to remove
        ask_removals: &[String],          // prices to remove
    ) -> Result<OrderBookDeltas, ConversionError> {
        let config = self.get_config(symbol);
        let mut deltas = Vec::new();
        let mut order_id = sequence * 1000; // Ensure unique order IDs

        // Process bid removals
        for price_str in bid_removals {
            let price = parse_price(price_str, &config)?;
            let order = BookOrder::new(OrderSide::Buy, price, Quantity::from("0"), order_id);
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Delete,
                order,
                0, // flags
                sequence,
                ts_event,
                ts_init,
            ));
            order_id += 1;
        }

        // Process ask removals
        for price_str in ask_removals {
            let price = parse_price(price_str, &config)?;
            let order = BookOrder::new(OrderSide::Sell, price, Quantity::from("0"), order_id);
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Delete,
                order,
                0, // flags
                sequence,
                ts_event,
                ts_init,
            ));
            order_id += 1;
        }

        // Process bid updates/additions
        for (price_str, size_str) in bid_updates {
            let price = parse_price(price_str, &config)?;
            let size = parse_size(size_str, &config)?;

            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update, // Could be Add or Update - we use Update as safer default
                    order,
                    0, // flags
                    sequence,
                    ts_event,
                    ts_init,
                ));
            } else {
                // Size 0 means removal
                let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    order,
                    0, // flags
                    sequence,
                    ts_event,
                    ts_init,
                ));
            }
            order_id += 1;
        }

        // Process ask updates/additions
        for (price_str, size_str) in ask_updates {
            let price = parse_price(price_str, &config)?;
            let size = parse_size(size_str, &config)?;

            if size.as_f64() > 0.0 {
                let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update, // Could be Add or Update - we use Update as safer default
                    order,
                    0, // flags
                    sequence,
                    ts_event,
                    ts_init,
                ));
            } else {
                // Size 0 means removal
                let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
                deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    order,
                    0, // flags
                    sequence,
                    ts_event,
                    ts_init,
                ));
            }
            order_id += 1;
        }

        Ok(OrderBookDeltas::new(instrument_id, deltas))
    }
}

/// Convert HTTP level to price and size
fn parse_level(
    level: &HyperliquidLevel,
    config: &BookConfig,
) -> Result<(Price, Quantity), ConversionError> {
    let price = parse_price(&level.px, config)?;
    let size = parse_size(&level.sz, config)?;
    Ok((price, size))
}

/// Convert WebSocket level to price and size
fn parse_ws_level(
    level: &WsLevelData,
    config: &BookConfig,
) -> Result<(Price, Quantity), ConversionError> {
    let price = parse_price(&level.px, config)?;
    let size = parse_size(&level.sz, config)?;
    Ok((price, size))
}

/// Parse price string to Price with proper precision
fn parse_price(price_str: &str, _config: &BookConfig) -> Result<Price, ConversionError> {
    let _decimal = Decimal::from_str(price_str).map_err(|_| ConversionError::InvalidPrice {
        value: price_str.to_string(),
    })?;

    Price::from_str(price_str).map_err(|_| ConversionError::InvalidPrice {
        value: price_str.to_string(),
    })
}

/// Parse size string to Quantity with proper precision
fn parse_size(size_str: &str, _config: &BookConfig) -> Result<Quantity, ConversionError> {
    let _decimal = Decimal::from_str(size_str).map_err(|_| ConversionError::InvalidSize {
        value: size_str.to_string(),
    })?;

    Quantity::from_str(size_str).map_err(|_| ConversionError::InvalidSize {
        value: size_str.to_string(),
    })
}

/// Error conditions from Hyperliquid data conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    /// Invalid price string format.
    InvalidPrice { value: String },
    /// Invalid size string format.
    InvalidSize { value: String },
    /// Error creating OrderBookDeltas
    OrderBookDeltasError(String),
}

impl From<anyhow::Error> for ConversionError {
    fn from(err: anyhow::Error) -> Self {
        ConversionError::OrderBookDeltasError(err.to_string())
    }
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversionError::InvalidPrice { value } => write!(f, "Invalid price: {}", value),
            ConversionError::InvalidSize { value } => write!(f, "Invalid size: {}", value),
            ConversionError::OrderBookDeltasError(msg) => {
                write!(f, "OrderBookDeltas error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ConversionError {}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;

    use super::*;

    fn test_instrument_id() -> InstrumentId {
        InstrumentId::from("BTC.HYPER")
    }

    fn sample_http_book() -> HyperliquidL2Book {
        HyperliquidL2Book {
            coin: "BTC".to_string(),
            levels: vec![
                vec![
                    HyperliquidLevel {
                        px: "50000.00".to_string(),
                        sz: "1.5".to_string(),
                    },
                    HyperliquidLevel {
                        px: "49999.50".to_string(),
                        sz: "2.0".to_string(),
                    },
                ],
                vec![
                    HyperliquidLevel {
                        px: "50001.00".to_string(),
                        sz: "1.0".to_string(),
                    },
                    HyperliquidLevel {
                        px: "50002.50".to_string(),
                        sz: "3.0".to_string(),
                    },
                ],
            ],
            time: 1234567890,
        }
    }

    fn sample_ws_book() -> WsBookData {
        WsBookData {
            coin: "BTC".to_string(),
            levels: [
                vec![
                    WsLevelData {
                        px: "50000.00".to_string(),
                        sz: "1.5".to_string(),
                        n: 1,
                    },
                    WsLevelData {
                        px: "49999.50".to_string(),
                        sz: "2.0".to_string(),
                        n: 2,
                    },
                ],
                vec![
                    WsLevelData {
                        px: "50001.00".to_string(),
                        sz: "1.0".to_string(),
                        n: 1,
                    },
                    WsLevelData {
                        px: "50002.50".to_string(),
                        sz: "3.0".to_string(),
                        n: 1,
                    },
                ],
            ],
            time: 1234567890,
        }
    }

    #[rstest]
    fn test_http_snapshot_conversion() {
        let converter = HyperliquidDataConverter::new();
        let book_data = sample_http_book();
        let instrument_id = test_instrument_id();
        let ts_init = UnixNanos::default();

        let deltas = converter
            .convert_http_snapshot(&book_data, instrument_id, ts_init)
            .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 5); // 1 clear + 2 bids + 2 asks

        // First delta should be Clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Next deltas should be Add actions
        for delta in &deltas.deltas[1..] {
            assert_eq!(delta.action, BookAction::Add);
            assert!(delta.order.size.as_f64() > 0.0);
        }
    }

    #[rstest]
    fn test_ws_snapshot_conversion() {
        let converter = HyperliquidDataConverter::new();
        let book_data = sample_ws_book();
        let instrument_id = test_instrument_id();
        let ts_init = UnixNanos::default();

        let deltas = converter
            .convert_ws_snapshot(&book_data, instrument_id, ts_init)
            .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 5); // 1 clear + 2 bids + 2 asks

        // First delta should be Clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    }

    #[rstest]
    fn test_delta_update_conversion() {
        let converter = HyperliquidDataConverter::new();
        let instrument_id = test_instrument_id();
        let ts_event = UnixNanos::default();
        let ts_init = UnixNanos::default();

        let bid_updates = vec![("50000.00".to_string(), "1.5".to_string())];
        let ask_updates = vec![("50001.00".to_string(), "2.0".to_string())];
        let bid_removals = vec!["49999.00".to_string()];
        let ask_removals = vec!["50002.00".to_string()];

        let deltas = converter
            .convert_delta_update(
                "BTC",
                instrument_id,
                123,
                ts_event,
                ts_init,
                &bid_updates,
                &ask_updates,
                &bid_removals,
                &ask_removals,
            )
            .unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        assert_eq!(deltas.deltas.len(), 4); // 2 removals + 2 updates
        assert_eq!(deltas.sequence, 123);
    }

    #[rstest]
    fn test_price_size_parsing() {
        let config = BookConfig::new(2, 5);

        let price = parse_price("50000.12", &config).unwrap();
        assert_eq!(price.to_string(), "50000.12");

        let size = parse_size("1.23456", &config).unwrap();
        assert_eq!(size.to_string(), "1.23456");
    }

    #[rstest]
    fn test_invalid_price_parsing() {
        let config = BookConfig::new(2, 5);

        let result = parse_price("invalid", &config);
        assert!(result.is_err());

        match result.unwrap_err() {
            ConversionError::InvalidPrice { value } => assert_eq!(value, "invalid"),
            _ => panic!("Expected InvalidPrice error"),
        }
    }

    #[rstest]
    fn test_configuration() {
        let mut converter = HyperliquidDataConverter::new();
        let config = BookConfig::new(4, 8);

        converter.configure_instrument("ETH", config.clone());

        let retrieved_config = converter.get_config("ETH");
        assert_eq!(retrieved_config.price_decimals, 4);
        assert_eq!(retrieved_config.size_decimals, 8);

        // Unknown symbol should return default
        let default_config = converter.get_config("UNKNOWN");
        assert_eq!(default_config.price_decimals, 2);
        assert_eq!(default_config.size_decimals, 5);
    }
}
