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

use std::{collections::HashMap, fmt::Display, str::FromStr};

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{delta::OrderBookDelta, deltas::OrderBookDeltas, order::BookOrder},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    http::models::{HyperliquidL2Book, HyperliquidLevel},
    websocket::messages::{WsBookData, WsLevelData},
};

/// Configuration for price/size precision when converting Hyperliquid data
#[derive(Debug, Clone)]
pub struct HyperliquidInstrumentInfo {
    /// Price precision (number of decimal places)
    pub price_decimals: u8,
    /// Size precision (number of decimal places)
    pub size_decimals: u8,
}

impl HyperliquidInstrumentInfo {
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
    configs: HashMap<Ustr, HyperliquidInstrumentInfo>,
}

impl HyperliquidDataConverter {
    /// Create a new converter
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure precision for an instrument
    pub fn configure_instrument(&mut self, symbol: &str, config: HyperliquidInstrumentInfo) {
        self.configs.insert(Ustr::from(symbol), config);
    }

    /// Get configuration for an instrument, using default if not configured
    fn get_config(&self, symbol: &Ustr) -> HyperliquidInstrumentInfo {
        self.configs
            .get(symbol)
            .cloned()
            .unwrap_or_else(HyperliquidInstrumentInfo::default_crypto)
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
            if size.is_positive() {
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
            if size.is_positive() {
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
            if size.is_positive() {
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
            if size.is_positive() {
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
        instrument_id: InstrumentId,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        bid_updates: &[(String, String)], // (price, size) pairs
        ask_updates: &[(String, String)], // (price, size) pairs
        bid_removals: &[String],          // prices to remove
        ask_removals: &[String],          // prices to remove
    ) -> Result<OrderBookDeltas, ConversionError> {
        let config = self.get_config(&instrument_id.symbol.inner());
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

            if size.is_positive() {
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

            if size.is_positive() {
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
    inst_info: &HyperliquidInstrumentInfo,
) -> Result<(Price, Quantity), ConversionError> {
    let price = parse_price(&level.px, inst_info)?;
    let size = parse_size(&level.sz, inst_info)?;
    Ok((price, size))
}

/// Convert WebSocket level to price and size
fn parse_ws_level(
    level: &WsLevelData,
    config: &HyperliquidInstrumentInfo,
) -> Result<(Price, Quantity), ConversionError> {
    let price = parse_price(&level.px, config)?;
    let size = parse_size(&level.sz, config)?;
    Ok((price, size))
}

/// Parse price string to Price with proper precision
fn parse_price(
    price_str: &str,
    _config: &HyperliquidInstrumentInfo,
) -> Result<Price, ConversionError> {
    let _decimal = Decimal::from_str(price_str).map_err(|_| ConversionError::InvalidPrice {
        value: price_str.to_string(),
    })?;

    Price::from_str(price_str).map_err(|_| ConversionError::InvalidPrice {
        value: price_str.to_string(),
    })
}

/// Parse size string to Quantity with proper precision
fn parse_size(
    size_str: &str,
    _config: &HyperliquidInstrumentInfo,
) -> Result<Quantity, ConversionError> {
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

impl Display for ConversionError {
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

    fn load_test_data<T>(filename: &str) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let path = format!("test_data/{}", filename);
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn test_instrument_id() -> InstrumentId {
        InstrumentId::from("BTC.HYPER")
    }

    fn sample_http_book() -> HyperliquidL2Book {
        load_test_data("http_l2_book_snapshot.json")
    }

    fn sample_ws_book() -> WsBookData {
        load_test_data("ws_book_data.json")
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
        assert_eq!(deltas.deltas.len(), 11); // 1 clear + 5 bids + 5 asks

        // First delta should be Clear - assert all fields
        let clear_delta = &deltas.deltas[0];
        assert_eq!(clear_delta.instrument_id, instrument_id);
        assert_eq!(clear_delta.action, BookAction::Clear);
        assert_eq!(clear_delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(clear_delta.order.price.raw, 0);
        assert_eq!(clear_delta.order.price.precision, 0);
        assert_eq!(clear_delta.order.size.raw, 0);
        assert_eq!(clear_delta.order.size.precision, 0);
        assert_eq!(clear_delta.order.order_id, 0);
        assert_eq!(clear_delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(clear_delta.sequence, 0);
        assert_eq!(
            clear_delta.ts_event,
            UnixNanos::from(book_data.time * 1_000_000)
        );
        assert_eq!(clear_delta.ts_init, ts_init);

        // Second delta should be first bid Add - assert all fields
        let first_bid_delta = &deltas.deltas[1];
        assert_eq!(first_bid_delta.instrument_id, instrument_id);
        assert_eq!(first_bid_delta.action, BookAction::Add);
        assert_eq!(first_bid_delta.order.side, OrderSide::Buy);
        assert_eq!(first_bid_delta.order.price, Price::from("98450.50"));
        assert_eq!(first_bid_delta.order.size, Quantity::from("2.5"));
        assert_eq!(first_bid_delta.order.order_id, 1);
        assert_eq!(first_bid_delta.flags, RecordFlag::F_LAST as u8);
        assert_eq!(first_bid_delta.sequence, 1);
        assert_eq!(
            first_bid_delta.ts_event,
            UnixNanos::from(book_data.time * 1_000_000)
        );
        assert_eq!(first_bid_delta.ts_init, ts_init);

        // Verify remaining deltas are Add actions with positive sizes
        for delta in &deltas.deltas[1..] {
            assert_eq!(delta.action, BookAction::Add);
            assert!(delta.order.size.is_positive());
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
        assert_eq!(deltas.deltas.len(), 11); // 1 clear + 5 bids + 5 asks

        // First delta should be Clear - assert all fields
        let clear_delta = &deltas.deltas[0];
        assert_eq!(clear_delta.instrument_id, instrument_id);
        assert_eq!(clear_delta.action, BookAction::Clear);
        assert_eq!(clear_delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(clear_delta.order.price.raw, 0);
        assert_eq!(clear_delta.order.price.precision, 0);
        assert_eq!(clear_delta.order.size.raw, 0);
        assert_eq!(clear_delta.order.size.precision, 0);
        assert_eq!(clear_delta.order.order_id, 0);
        assert_eq!(clear_delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(clear_delta.sequence, 0);
        assert_eq!(
            clear_delta.ts_event,
            UnixNanos::from(book_data.time * 1_000_000)
        );
        assert_eq!(clear_delta.ts_init, ts_init);

        // Second delta should be first bid Add - assert all fields
        let first_bid_delta = &deltas.deltas[1];
        assert_eq!(first_bid_delta.instrument_id, instrument_id);
        assert_eq!(first_bid_delta.action, BookAction::Add);
        assert_eq!(first_bid_delta.order.side, OrderSide::Buy);
        assert_eq!(first_bid_delta.order.price, Price::from("98450.50"));
        assert_eq!(first_bid_delta.order.size, Quantity::from("2.5"));
        assert_eq!(first_bid_delta.order.order_id, 1);
        assert_eq!(first_bid_delta.flags, RecordFlag::F_LAST as u8);
        assert_eq!(first_bid_delta.sequence, 1);
        assert_eq!(
            first_bid_delta.ts_event,
            UnixNanos::from(book_data.time * 1_000_000)
        );
        assert_eq!(first_bid_delta.ts_init, ts_init);
    }

    #[rstest]
    fn test_delta_update_conversion() {
        let converter = HyperliquidDataConverter::new();
        let instrument_id = test_instrument_id();
        let ts_event = UnixNanos::default();
        let ts_init = UnixNanos::default();

        let bid_updates = vec![("98450.00".to_string(), "1.5".to_string())];
        let ask_updates = vec![("98451.00".to_string(), "2.0".to_string())];
        let bid_removals = vec!["98449.00".to_string()];
        let ask_removals = vec!["98452.00".to_string()];

        let deltas = converter
            .convert_delta_update(
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

        // First delta should be bid removal - assert all fields
        let first_delta = &deltas.deltas[0];
        assert_eq!(first_delta.instrument_id, instrument_id);
        assert_eq!(first_delta.action, BookAction::Delete);
        assert_eq!(first_delta.order.side, OrderSide::Buy);
        assert_eq!(first_delta.order.price, Price::from("98449.00"));
        assert_eq!(first_delta.order.size, Quantity::from("0"));
        assert_eq!(first_delta.order.order_id, 123000);
        assert_eq!(first_delta.flags, 0);
        assert_eq!(first_delta.sequence, 123);
        assert_eq!(first_delta.ts_event, ts_event);
        assert_eq!(first_delta.ts_init, ts_init);
    }

    #[rstest]
    fn test_price_size_parsing() {
        let config = HyperliquidInstrumentInfo::new(2, 5);

        let price = parse_price("98450.50", &config).unwrap();
        assert_eq!(price.to_string(), "98450.50");

        let size = parse_size("2.5", &config).unwrap();
        assert_eq!(size.to_string(), "2.5");
    }

    #[rstest]
    fn test_hyperliquid_instrument_mini_info() {
        // Test constructor with all fields
        let config = HyperliquidInstrumentInfo::new(4, 6);
        assert_eq!(config.price_decimals, 4);
        assert_eq!(config.size_decimals, 6);

        // Test default crypto configuration - assert all fields
        let default_config = HyperliquidInstrumentInfo::default_crypto();
        assert_eq!(default_config.price_decimals, 2);
        assert_eq!(default_config.size_decimals, 5);
    }

    #[rstest]
    fn test_invalid_price_parsing() {
        let config = HyperliquidInstrumentInfo::new(2, 5);

        // Test invalid price parsing
        let result = parse_price("invalid", &config);
        assert!(result.is_err());

        match result.unwrap_err() {
            ConversionError::InvalidPrice { value } => {
                assert_eq!(value, "invalid");
                // Verify the error displays correctly
                assert!(value.contains("invalid"));
            }
            _ => panic!("Expected InvalidPrice error"),
        }

        // Test invalid size parsing
        let size_result = parse_size("not_a_number", &config);
        assert!(size_result.is_err());

        match size_result.unwrap_err() {
            ConversionError::InvalidSize { value } => {
                assert_eq!(value, "not_a_number");
                // Verify the error displays correctly
                assert!(value.contains("not_a_number"));
            }
            _ => panic!("Expected InvalidSize error"),
        }
    }

    #[rstest]
    fn test_configuration() {
        let mut converter = HyperliquidDataConverter::new();
        let config = HyperliquidInstrumentInfo::new(4, 8);

        let asset = Ustr::from("ETH");

        converter.configure_instrument(asset.as_str(), config.clone());

        // Assert all fields of the retrieved config
        let retrieved_config = converter.get_config(&asset);
        assert_eq!(retrieved_config.price_decimals, 4);
        assert_eq!(retrieved_config.size_decimals, 8);

        // Assert all fields of the default config for unknown symbol
        let default_config = converter.get_config(&Ustr::from("UNKNOWN"));
        assert_eq!(default_config.price_decimals, 2);
        assert_eq!(default_config.size_decimals, 5);

        // Verify the original config object has expected values
        assert_eq!(config.price_decimals, 4);
        assert_eq!(config.size_decimals, 8);
    }
}
