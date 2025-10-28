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

use nautilus_core::{UUID4, UnixNanos};
pub use nautilus_execution::models::latency::LatencyModel;
use nautilus_model::{
    data::{delta::OrderBookDelta, deltas::OrderBookDeltas, order::BookOrder},
    enums::{AccountType, BookAction, OrderSide, PositionSide, RecordFlag},
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    reports::PositionStatusReport,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use ustr::Ustr;

use crate::{
    http::models::{HyperliquidL2Book, HyperliquidLevel},
    websocket::messages::{WsBookData, WsLevelData},
};

/// Configuration for price/size precision.
#[derive(Debug, Clone)]
pub struct HyperliquidInstrumentInfo {
    pub instrument_id: InstrumentId,
    pub price_decimals: u8,
    pub size_decimals: u8,
    /// Minimum tick size for price (optional)
    pub tick_size: Option<Decimal>,
    /// Minimum step size for quantity (optional)
    pub step_size: Option<Decimal>,
    /// Minimum notional value for orders (optional)
    pub min_notional: Option<Decimal>,
}

impl HyperliquidInstrumentInfo {
    /// Create config with specific precision
    pub fn new(instrument_id: InstrumentId, price_decimals: u8, size_decimals: u8) -> Self {
        Self {
            instrument_id,
            price_decimals,
            size_decimals,
            tick_size: None,
            step_size: None,
            min_notional: None,
        }
    }

    /// Create config with full metadata
    pub fn with_metadata(
        instrument_id: InstrumentId,
        price_decimals: u8,
        size_decimals: u8,
        tick_size: Decimal,
        step_size: Decimal,
        min_notional: Decimal,
    ) -> Self {
        Self {
            instrument_id,
            price_decimals,
            size_decimals,
            tick_size: Some(tick_size),
            step_size: Some(step_size),
            min_notional: Some(min_notional),
        }
    }

    /// Create with basic precision config and calculated tick/step sizes
    pub fn with_precision(
        instrument_id: InstrumentId,
        price_decimals: u8,
        size_decimals: u8,
    ) -> Self {
        let tick_size = Decimal::new(1, price_decimals as u32);
        let step_size = Decimal::new(1, size_decimals as u32);
        Self {
            instrument_id,
            price_decimals,
            size_decimals,
            tick_size: Some(tick_size),
            step_size: Some(step_size),
            min_notional: None,
        }
    }

    /// Default configuration for most crypto assets
    pub fn default_crypto(instrument_id: InstrumentId) -> Self {
        Self::with_precision(instrument_id, 2, 5) // 0.01 price precision, 0.00001 size precision
    }
}

/// Simple instrument cache for parsing messages and responses
#[derive(Debug, Default)]
pub struct HyperliquidInstrumentCache {
    instruments_by_symbol: HashMap<Ustr, HyperliquidInstrumentInfo>,
}

impl HyperliquidInstrumentCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            instruments_by_symbol: HashMap::new(),
        }
    }

    /// Add or update an instrument in the cache
    pub fn insert(&mut self, symbol: &str, info: HyperliquidInstrumentInfo) {
        self.instruments_by_symbol.insert(Ustr::from(symbol), info);
    }

    /// Get instrument metadata for a symbol
    pub fn get(&self, symbol: &str) -> Option<&HyperliquidInstrumentInfo> {
        self.instruments_by_symbol.get(&Ustr::from(symbol))
    }

    /// Get all cached instruments
    pub fn get_all(&self) -> Vec<&HyperliquidInstrumentInfo> {
        self.instruments_by_symbol.values().collect()
    }

    /// Check if symbol exists in cache
    pub fn contains(&self, symbol: &str) -> bool {
        self.instruments_by_symbol.contains_key(&Ustr::from(symbol))
    }

    /// Get the number of cached instruments
    pub fn len(&self) -> usize {
        self.instruments_by_symbol.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.instruments_by_symbol.is_empty()
    }

    /// Clear all cached instruments
    pub fn clear(&mut self) {
        self.instruments_by_symbol.clear();
    }
}

/// Key for identifying unique trades/tickers
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HyperliquidTradeKey {
    /// Preferred: exchange-provided unique identifier
    Id(String),
    /// Fallback: exchange sequence number
    Seq(u64),
}

/// Manages precision configuration and converts Hyperliquid data to standard Nautilus formats
#[derive(Debug)]
pub struct HyperliquidDataConverter {
    /// Configuration by instrument symbol
    configs: HashMap<Ustr, HyperliquidInstrumentInfo>,
}

impl Default for HyperliquidDataConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperliquidDataConverter {
    /// Create a new converter
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Create a latency model for order processing simulation
    ///
    /// This uses the execution crate's LatencyModel for simulating order processing latencies.
    /// For real-time latency monitoring, use standard `tracing` macros.
    pub fn create_latency_model(
        &self,
        base_latency_ns: u64,
        insert_latency_ns: u64,
        update_latency_ns: u64,
        delete_latency_ns: u64,
    ) -> LatencyModel {
        LatencyModel::new(
            UnixNanos::from(base_latency_ns),
            UnixNanos::from(insert_latency_ns),
            UnixNanos::from(update_latency_ns),
            UnixNanos::from(delete_latency_ns),
        )
    }

    /// Create a default latency model for Hyperliquid (typical network latencies)
    pub fn create_default_latency_model(&self) -> LatencyModel {
        // Typical latencies for crypto exchanges (in nanoseconds)
        self.create_latency_model(
            50_000_000, // 50ms base latency
            10_000_000, // 10ms insert latency
            5_000_000,  // 5ms update latency
            5_000_000,  // 5ms delete latency
        )
    }

    /// Normalize an order's price and quantity for Hyperliquid
    ///
    /// This is a convenience method that uses the instrument configuration
    /// to apply proper normalization and validation.
    pub fn normalize_order_for_symbol(
        &mut self,
        symbol: &str,
        price: Decimal,
        qty: Decimal,
    ) -> Result<(Decimal, Decimal), String> {
        let config = self.get_config(&Ustr::from(symbol));

        // Use default values if instrument metadata is not available
        let tick_size = config.tick_size.unwrap_or_else(|| Decimal::new(1, 2)); // 0.01
        let step_size = config.step_size.unwrap_or_else(|| {
            // Calculate step size from decimals if not provided
            match config.size_decimals {
                0 => Decimal::ONE,
                1 => Decimal::new(1, 1), // 0.1
                2 => Decimal::new(1, 2), // 0.01
                3 => Decimal::new(1, 3), // 0.001
                4 => Decimal::new(1, 4), // 0.0001
                5 => Decimal::new(1, 5), // 0.00001
                _ => Decimal::new(1, 6), // 0.000001
            }
        });
        let min_notional = config.min_notional.unwrap_or_else(|| Decimal::from(10)); // $10 minimum

        crate::common::parse::normalize_order(
            price,
            qty,
            tick_size,
            step_size,
            min_notional,
            config.price_decimals,
            config.size_decimals,
        )
    }

    /// Configure precision for an instrument
    pub fn configure_instrument(&mut self, symbol: &str, config: HyperliquidInstrumentInfo) {
        self.configs.insert(Ustr::from(symbol), config);
    }

    /// Get configuration for an instrument, using default if not configured
    fn get_config(&self, symbol: &Ustr) -> HyperliquidInstrumentInfo {
        self.configs.get(symbol).cloned().unwrap_or_else(|| {
            // Create default config with a placeholder instrument_id based on symbol
            let instrument_id = InstrumentId::from(format!("{}.HYPER", symbol).as_str());
            HyperliquidInstrumentInfo::default_crypto(instrument_id)
        })
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
        Self::OrderBookDeltasError(err.to_string())
    }
}

impl Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrice { value } => write!(f, "Invalid price: {}", value),
            Self::InvalidSize { value } => write!(f, "Invalid size: {}", value),
            Self::OrderBookDeltasError(msg) => {
                write!(f, "OrderBookDeltas error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ConversionError {}

////////////////////////////////////////////////////////////////////////////////
// Position and Account State Management
////////////////////////////////////////////////////////////////////////////////

/// Raw position data from Hyperliquid API for parsing position status reports.
///
/// This struct is used only for parsing API responses and converting to Nautilus
/// PositionStatusReport events. The actual position tracking is handled by the
/// Nautilus platform, not the adapter.
///
/// See Hyperliquid API documentation:
/// - [User State Info](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals#retrieve-users-perpetuals-account-summary)
#[derive(Clone, Debug)]
pub struct HyperliquidPositionData {
    pub asset: String,
    pub position: Decimal, // signed: positive = long, negative = short
    pub entry_px: Option<Decimal>,
    pub unrealized_pnl: Decimal,
    pub cumulative_funding: Option<Decimal>,
    pub position_value: Decimal,
}

impl HyperliquidPositionData {
    /// Check if position is flat (no quantity)
    pub fn is_flat(&self) -> bool {
        self.position.is_zero()
    }

    /// Check if position is long
    pub fn is_long(&self) -> bool {
        self.position > Decimal::ZERO
    }

    /// Check if position is short
    pub fn is_short(&self) -> bool {
        self.position < Decimal::ZERO
    }
}

/// Balance information from Hyperliquid API.
///
/// Represents account balance for a specific asset (currency) as returned by Hyperliquid.
/// Used for converting to Nautilus AccountBalance and AccountState events.
///
/// See Hyperliquid API documentation:
/// - [Perpetuals Account Summary](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals#retrieve-users-perpetuals-account-summary)
#[derive(Clone, Debug)]
pub struct HyperliquidBalance {
    pub asset: String,
    pub total: Decimal,
    pub available: Decimal,
    pub sequence: u64,
    pub ts_event: UnixNanos,
}

impl HyperliquidBalance {
    pub fn new(
        asset: String,
        total: Decimal,
        available: Decimal,
        sequence: u64,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            asset,
            total,
            available,
            sequence,
            ts_event,
        }
    }

    /// Calculate locked (reserved) balance
    pub fn locked(&self) -> Decimal {
        (self.total - self.available).max(Decimal::ZERO)
    }
}

/// Simplified account state for Hyperliquid adapter.
///
/// This tracks only the essential state needed for generating Nautilus AccountState events.
/// Position tracking is handled by the Nautilus platform, not the adapter.
///
/// See Hyperliquid API documentation:
/// - [User State Info](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals#retrieve-users-perpetuals-account-summary)
#[derive(Default, Debug)]
pub struct HyperliquidAccountState {
    pub balances: HashMap<String, HyperliquidBalance>,
    pub last_sequence: u64,
}

impl HyperliquidAccountState {
    pub fn new() -> Self {
        Default::default()
    }

    /// Get balance for an asset, returns zero balance if not found
    pub fn get_balance(&self, asset: &str) -> HyperliquidBalance {
        self.balances.get(asset).cloned().unwrap_or_else(|| {
            HyperliquidBalance::new(
                asset.to_string(),
                Decimal::ZERO,
                Decimal::ZERO,
                0,
                UnixNanos::default(),
            )
        })
    }

    /// Calculate total account value from balances only.
    /// Note: This doesn't include unrealized PnL from positions as those are
    /// tracked by the Nautilus platform, not the adapter.
    pub fn account_value(&self) -> Decimal {
        self.balances.values().map(|balance| balance.total).sum()
    }

    /// Convert HyperliquidAccountState to Nautilus AccountState event.
    ///
    /// This creates a standard Nautilus AccountState from the Hyperliquid-specific account state,
    /// converting balances and handling the margin account type since Hyperliquid supports leverage.
    ///
    /// # Arguments
    ///
    /// * `account_id` - The account identifier for this state
    /// * `ts_event` - When this state was observed/received
    /// * `ts_init` - When this state object was created
    ///
    /// # Returns
    ///
    /// A Nautilus AccountState event that can be processed by the platform
    pub fn to_account_state(
        &self,
        account_id: AccountId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<AccountState> {
        // Convert HyperliquidBalance to AccountBalance
        let balances: Vec<AccountBalance> = self
            .balances
            .values()
            .map(|balance| {
                // Create currency - Hyperliquid primarily uses USD/USDC
                let currency = Currency::from(balance.asset.as_str());

                // Convert Decimal to f64 and create Money with proper currency
                let total = Money::new(balance.total.to_f64().unwrap_or(0.0), currency);
                let free = Money::new(balance.available.to_f64().unwrap_or(0.0), currency);
                let locked = total - free; // locked = total - available

                AccountBalance::new(total, locked, free)
            })
            .collect();

        // For now, we don't map individual position margins since Hyperliquid uses cross-margin
        // The risk management happens at the exchange level
        let margins = Vec::new();

        // Hyperliquid is a margin exchange (supports leverage)
        let account_type = AccountType::Margin;

        // This state comes from the exchange
        let is_reported = true;

        // Generate event ID
        let event_id = UUID4::new();

        Ok(AccountState::new(
            account_id,
            account_type,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event,
            ts_init,
            None, // base_currency: None for multi-currency support
        ))
    }
}

/// Account balance update events from Hyperliquid exchange.
///
/// This enum represents balance update events that can be received from Hyperliquid
/// via WebSocket streams or HTTP responses. Position tracking is handled by the
/// Nautilus platform, so this only processes balance changes.
///
/// See Hyperliquid documentation:
/// - [WebSocket API](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket)
/// - [User State Updates](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket#user-data)
#[derive(Debug, Clone)]
pub enum HyperliquidAccountEvent {
    /// Complete snapshot of balances
    BalanceSnapshot {
        balances: Vec<HyperliquidBalance>,
        sequence: u64,
    },
    /// Delta update for a single balance
    BalanceDelta { balance: HyperliquidBalance },
}

impl HyperliquidAccountState {
    /// Apply a balance event to update the account state
    pub fn apply(&mut self, event: HyperliquidAccountEvent) {
        match event {
            HyperliquidAccountEvent::BalanceSnapshot { balances, sequence } => {
                self.balances.clear();

                for balance in balances {
                    self.balances.insert(balance.asset.clone(), balance);
                }

                self.last_sequence = sequence;
            }
            HyperliquidAccountEvent::BalanceDelta { balance } => {
                let sequence = balance.sequence;
                let entry = self
                    .balances
                    .entry(balance.asset.clone())
                    .or_insert_with(|| balance.clone());

                // Only update if sequence is newer
                if sequence > entry.sequence {
                    *entry = balance;
                    self.last_sequence = self.last_sequence.max(sequence);
                }
            }
        }
    }
}

/// Parse Hyperliquid position data into a Nautilus PositionStatusReport.
///
/// This function converts raw position data from Hyperliquid API responses into
/// the standardized Nautilus PositionStatusReport format. The actual position
/// tracking and management is handled by the Nautilus platform.
///
/// See Hyperliquid API documentation:
/// - [User State Info](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals#retrieve-users-perpetuals-account-summary)
/// - [Position Data Format](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint/perpetuals#retrieve-users-perpetuals-account-summary)
pub fn parse_position_status_report(
    position_data: &HyperliquidPositionData,
    account_id: AccountId,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    // Determine position side
    let position_side = if position_data.is_flat() {
        PositionSide::Flat
    } else if position_data.is_long() {
        PositionSide::Long
    } else {
        PositionSide::Short
    };

    // Convert position size to Quantity
    let quantity = Quantity::new(position_data.position.abs().to_f64().unwrap_or(0.0), 0);

    // Use current timestamp as last update time
    let ts_last = ts_init;

    // Convert entry price to Decimal if available
    let avg_px_open = position_data.entry_px;

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side.as_specified(),
        quantity,
        ts_last,
        ts_init,
        None, // report_id: auto-generated
        None, // venue_position_id: Hyperliquid doesn't use position IDs
        avg_px_open,
    ))
}

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[allow(dead_code)]
mod tests {
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
        let instrument_id = test_instrument_id();
        let config = HyperliquidInstrumentInfo::new(instrument_id, 2, 5);

        let price = parse_price("98450.50", &config).unwrap();
        assert_eq!(price.to_string(), "98450.50");

        let size = parse_size("2.5", &config).unwrap();
        assert_eq!(size.to_string(), "2.5");
    }

    #[rstest]
    fn test_hyperliquid_instrument_mini_info() {
        let instrument_id = test_instrument_id();

        // Test constructor with all fields
        let config = HyperliquidInstrumentInfo::new(instrument_id, 4, 6);
        assert_eq!(config.instrument_id, instrument_id);
        assert_eq!(config.price_decimals, 4);
        assert_eq!(config.size_decimals, 6);

        // Test default crypto configuration - assert all fields
        let default_config = HyperliquidInstrumentInfo::default_crypto(instrument_id);
        assert_eq!(default_config.instrument_id, instrument_id);
        assert_eq!(default_config.price_decimals, 2);
        assert_eq!(default_config.size_decimals, 5);
    }

    #[rstest]
    fn test_invalid_price_parsing() {
        let instrument_id = test_instrument_id();
        let config = HyperliquidInstrumentInfo::new(instrument_id, 2, 5);

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
        let eth_id = InstrumentId::from("ETH.HYPER");
        let config = HyperliquidInstrumentInfo::new(eth_id, 4, 8);

        let asset = Ustr::from("ETH");

        converter.configure_instrument(asset.as_str(), config.clone());

        // Assert all fields of the retrieved config
        let retrieved_config = converter.get_config(&asset);
        assert_eq!(retrieved_config.instrument_id, eth_id);
        assert_eq!(retrieved_config.price_decimals, 4);
        assert_eq!(retrieved_config.size_decimals, 8);

        // Assert all fields of the default config for unknown symbol
        let default_config = converter.get_config(&Ustr::from("UNKNOWN"));
        assert_eq!(
            default_config.instrument_id,
            InstrumentId::from("UNKNOWN.HYPER")
        );
        assert_eq!(default_config.price_decimals, 2);
        assert_eq!(default_config.size_decimals, 5);

        // Verify the original config object has expected values
        assert_eq!(config.instrument_id, eth_id);
        assert_eq!(config.price_decimals, 4);
        assert_eq!(config.size_decimals, 8);
    }

    #[rstest]
    fn test_instrument_info_creation() {
        let instrument_id = InstrumentId::from("BTC.HYPER");
        let info = HyperliquidInstrumentInfo::with_metadata(
            instrument_id,
            2,
            5,
            Decimal::from_f64_retain(0.01).unwrap(),
            Decimal::from_f64_retain(0.00001).unwrap(),
            Decimal::from_f64_retain(10.0).unwrap(),
        );

        assert_eq!(info.instrument_id, instrument_id);
        assert_eq!(info.price_decimals, 2);
        assert_eq!(info.size_decimals, 5);
        assert_eq!(
            info.tick_size,
            Some(Decimal::from_f64_retain(0.01).unwrap())
        );
        assert_eq!(
            info.step_size,
            Some(Decimal::from_f64_retain(0.00001).unwrap())
        );
        assert_eq!(
            info.min_notional,
            Some(Decimal::from_f64_retain(10.0).unwrap())
        );
    }

    #[rstest]
    fn test_instrument_info_with_precision() {
        let instrument_id = test_instrument_id();
        let info = HyperliquidInstrumentInfo::with_precision(instrument_id, 3, 4);
        assert_eq!(info.instrument_id, instrument_id);
        assert_eq!(info.price_decimals, 3);
        assert_eq!(info.size_decimals, 4);
        assert_eq!(info.tick_size, Some(Decimal::new(1, 3))); // 0.001
        assert_eq!(info.step_size, Some(Decimal::new(1, 4))); // 0.0001
    }

    #[tokio::test]
    async fn test_instrument_cache_basic_operations() {
        let btc_info = HyperliquidInstrumentInfo::with_metadata(
            InstrumentId::from("BTC.HYPER"),
            2,
            5,
            Decimal::from_f64_retain(0.01).unwrap(),
            Decimal::from_f64_retain(0.00001).unwrap(),
            Decimal::from_f64_retain(10.0).unwrap(),
        );

        let eth_info = HyperliquidInstrumentInfo::with_metadata(
            InstrumentId::from("ETH.HYPER"),
            2,
            4,
            Decimal::from_f64_retain(0.01).unwrap(),
            Decimal::from_f64_retain(0.0001).unwrap(),
            Decimal::from_f64_retain(10.0).unwrap(),
        );

        let mut cache = HyperliquidInstrumentCache::new();

        // Insert instruments manually
        cache.insert("BTC", btc_info.clone());
        cache.insert("ETH", eth_info.clone());

        // Get BTC instrument
        let retrieved_btc = cache.get("BTC").unwrap();
        assert_eq!(retrieved_btc.instrument_id, btc_info.instrument_id);
        assert_eq!(retrieved_btc.size_decimals, 5);

        // Get ETH instrument
        let retrieved_eth = cache.get("ETH").unwrap();
        assert_eq!(retrieved_eth.instrument_id, eth_info.instrument_id);
        assert_eq!(retrieved_eth.size_decimals, 4);

        // Test cache methods
        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());

        // Test contains
        assert!(cache.contains("BTC"));
        assert!(cache.contains("ETH"));
        assert!(!cache.contains("UNKNOWN"));

        // Test get_all
        let all_instruments = cache.get_all();
        assert_eq!(all_instruments.len(), 2);
    }

    #[rstest]
    fn test_instrument_cache_empty() {
        let cache = HyperliquidInstrumentCache::new();
        let result = cache.get("UNKNOWN");
        assert!(result.is_none());
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[rstest]
    fn test_latency_model_creation() {
        let converter = HyperliquidDataConverter::new();

        // Test custom latency model
        let latency_model = converter.create_latency_model(
            100_000_000, // 100ms base
            20_000_000,  // 20ms insert
            10_000_000,  // 10ms update
            10_000_000,  // 10ms delete
        );

        assert_eq!(latency_model.base_latency_nanos.as_u64(), 100_000_000);
        assert_eq!(latency_model.insert_latency_nanos.as_u64(), 20_000_000);
        assert_eq!(latency_model.update_latency_nanos.as_u64(), 10_000_000);
        assert_eq!(latency_model.delete_latency_nanos.as_u64(), 10_000_000);

        // Test default latency model
        let default_model = converter.create_default_latency_model();
        assert_eq!(default_model.base_latency_nanos.as_u64(), 50_000_000);
        assert_eq!(default_model.insert_latency_nanos.as_u64(), 10_000_000);
        assert_eq!(default_model.update_latency_nanos.as_u64(), 5_000_000);
        assert_eq!(default_model.delete_latency_nanos.as_u64(), 5_000_000);

        // Test that Display trait works
        let display_str = format!("{}", default_model);
        assert_eq!(display_str, "LatencyModel()");
    }

    #[rstest]
    fn test_normalize_order_for_symbol() {
        use rust_decimal_macros::dec;

        let mut converter = HyperliquidDataConverter::new();

        // Configure BTC with specific instrument info
        let btc_info = HyperliquidInstrumentInfo::with_metadata(
            InstrumentId::from("BTC.HYPER"),
            2,
            5,
            dec!(0.01),    // tick_size
            dec!(0.00001), // step_size
            dec!(10.0),    // min_notional
        );
        converter.configure_instrument("BTC", btc_info);

        // Test successful normalization
        let result = converter.normalize_order_for_symbol(
            "BTC",
            dec!(50123.456789), // price
            dec!(0.123456789),  // qty
        );

        assert!(result.is_ok());
        let (price, qty) = result.unwrap();
        assert_eq!(price, dec!(50123.45)); // rounded down to tick size
        assert_eq!(qty, dec!(0.12345)); // rounded down to step size

        // Test with symbol not configured (should use defaults)
        let result_eth = converter.normalize_order_for_symbol("ETH", dec!(3000.123), dec!(1.23456));
        assert!(result_eth.is_ok());

        // Test minimum notional failure
        let result_fail = converter.normalize_order_for_symbol(
            "BTC",
            dec!(1.0),   // low price
            dec!(0.001), // small qty
        );
        assert!(result_fail.is_err());
        assert!(result_fail.unwrap_err().contains("Notional value"));
    }

    #[rstest]
    fn test_hyperliquid_balance_creation_and_properties() {
        use rust_decimal_macros::dec;

        let asset = "USD".to_string();
        let total = dec!(1000.0);
        let available = dec!(750.0);
        let sequence = 42;
        let ts_event = UnixNanos::default();

        let balance = HyperliquidBalance::new(asset.clone(), total, available, sequence, ts_event);

        assert_eq!(balance.asset, asset);
        assert_eq!(balance.total, total);
        assert_eq!(balance.available, available);
        assert_eq!(balance.sequence, sequence);
        assert_eq!(balance.ts_event, ts_event);
        assert_eq!(balance.locked(), dec!(250.0)); // 1000 - 750

        // Test balance with all available
        let full_balance = HyperliquidBalance::new(
            "ETH".to_string(),
            dec!(100.0),
            dec!(100.0),
            1,
            UnixNanos::default(),
        );
        assert_eq!(full_balance.locked(), dec!(0.0));

        // Test edge case where available > total (should return 0 locked)
        let weird_balance = HyperliquidBalance::new(
            "WEIRD".to_string(),
            dec!(50.0),
            dec!(60.0),
            1,
            UnixNanos::default(),
        );
        assert_eq!(weird_balance.locked(), dec!(0.0));
    }

    #[rstest]
    fn test_hyperliquid_account_state_creation() {
        let state = HyperliquidAccountState::new();
        assert!(state.balances.is_empty());
        assert_eq!(state.last_sequence, 0);

        let default_state = HyperliquidAccountState::default();
        assert!(default_state.balances.is_empty());
        assert_eq!(default_state.last_sequence, 0);
    }

    #[rstest]
    fn test_hyperliquid_account_state_getters() {
        use rust_decimal_macros::dec;

        let mut state = HyperliquidAccountState::new();

        // Test get_balance for non-existent asset (should return zero balance)
        let balance = state.get_balance("USD");
        assert_eq!(balance.asset, "USD");
        assert_eq!(balance.total, dec!(0.0));
        assert_eq!(balance.available, dec!(0.0));

        // Add actual balance
        let real_balance = HyperliquidBalance::new(
            "USD".to_string(),
            dec!(1000.0),
            dec!(750.0),
            1,
            UnixNanos::default(),
        );
        state.balances.insert("USD".to_string(), real_balance);

        // Test retrieving real data
        let retrieved_balance = state.get_balance("USD");
        assert_eq!(retrieved_balance.total, dec!(1000.0));
    }

    #[rstest]
    fn test_hyperliquid_account_state_account_value() {
        use rust_decimal_macros::dec;

        let mut state = HyperliquidAccountState::new();

        // Add USD balance
        state.balances.insert(
            "USD".to_string(),
            HyperliquidBalance::new(
                "USD".to_string(),
                dec!(10000.0),
                dec!(5000.0),
                1,
                UnixNanos::default(),
            ),
        );

        let total_value = state.account_value();
        assert_eq!(total_value, dec!(10000.0));

        // Test with no balance
        state.balances.clear();
        let no_balance_value = state.account_value();
        assert_eq!(no_balance_value, dec!(0.0));
    }

    #[rstest]
    fn test_hyperliquid_account_event_balance_snapshot() {
        use rust_decimal_macros::dec;

        let mut state = HyperliquidAccountState::new();

        let balance = HyperliquidBalance::new(
            "USD".to_string(),
            dec!(1000.0),
            dec!(750.0),
            10,
            UnixNanos::default(),
        );

        let snapshot_event = HyperliquidAccountEvent::BalanceSnapshot {
            balances: vec![balance],
            sequence: 10,
        };

        state.apply(snapshot_event);

        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.last_sequence, 10);
        assert_eq!(state.get_balance("USD").total, dec!(1000.0));
    }

    #[rstest]
    fn test_hyperliquid_account_event_balance_delta() {
        use rust_decimal_macros::dec;

        let mut state = HyperliquidAccountState::new();

        // Add initial balance
        let initial_balance = HyperliquidBalance::new(
            "USD".to_string(),
            dec!(1000.0),
            dec!(750.0),
            5,
            UnixNanos::default(),
        );
        state.balances.insert("USD".to_string(), initial_balance);
        state.last_sequence = 5;

        // Apply balance delta with newer sequence
        let updated_balance = HyperliquidBalance::new(
            "USD".to_string(),
            dec!(1200.0),
            dec!(900.0),
            10,
            UnixNanos::default(),
        );

        let delta_event = HyperliquidAccountEvent::BalanceDelta {
            balance: updated_balance,
        };

        state.apply(delta_event);

        let balance = state.get_balance("USD");
        assert_eq!(balance.total, dec!(1200.0));
        assert_eq!(balance.available, dec!(900.0));
        assert_eq!(balance.sequence, 10);
        assert_eq!(state.last_sequence, 10);

        // Try to apply older sequence (should be ignored)
        let old_balance = HyperliquidBalance::new(
            "USD".to_string(),
            dec!(800.0),
            dec!(600.0),
            8,
            UnixNanos::default(),
        );

        let old_delta_event = HyperliquidAccountEvent::BalanceDelta {
            balance: old_balance,
        };

        state.apply(old_delta_event);

        // Balance should remain unchanged
        let balance = state.get_balance("USD");
        assert_eq!(balance.total, dec!(1200.0)); // Still the newer value
        assert_eq!(balance.sequence, 10); // Still the newer sequence
        assert_eq!(state.last_sequence, 10); // Global sequence unchanged
    }
}
