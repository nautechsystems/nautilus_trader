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

//! Data models for Delta Exchange HTTP API responses.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        DeltaExchangeAssetStatus, DeltaExchangeOrderState, DeltaExchangeOrderType,
        DeltaExchangeProductType, DeltaExchangeSide, DeltaExchangeTimeInForce,
        DeltaExchangeTradingState,
    },
    parse::{parse_decimal_or_zero, parse_empty_string_as_none, parse_optional_decimal},
};

/// Represents a Delta Exchange asset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeAsset {
    /// Asset ID.
    pub id: u64,
    /// Asset symbol (e.g., "BTC", "ETH").
    pub symbol: Ustr,
    /// Asset name.
    pub name: String,
    /// Asset status.
    pub status: DeltaExchangeAssetStatus,
    /// Precision for the asset.
    pub precision: u8,
    /// Deposit status.
    pub deposit_status: String,
    /// Withdrawal status.
    pub withdrawal_status: String,
    /// Base withdrawal fee.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub base_withdrawal_fee: Decimal,
    /// Minimum withdrawal amount.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub min_withdrawal_amount: Decimal,
}

/// Represents a Delta Exchange product (instrument).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeProduct {
    /// Product ID.
    pub id: u64,
    /// Trading symbol.
    pub symbol: Ustr,
    /// Product description.
    pub description: String,
    /// Product type.
    pub product_type: DeltaExchangeProductType,
    /// Underlying asset symbol.
    pub underlying_asset: Option<Ustr>,
    /// Quoting asset symbol.
    pub quoting_asset: Option<Ustr>,
    /// Settlement asset symbol.
    pub settlement_asset: Option<Ustr>,
    /// Contract value.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub contract_value: Decimal,
    /// Contract unit currency.
    pub contract_unit_currency: String,
    /// Tick size for price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub tick_size: Decimal,
    /// Minimum order size.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub min_size: Decimal,
    /// Maximum order size.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub max_size: Option<Decimal>,
    /// Trading state.
    pub state: DeltaExchangeTradingState,
    /// Whether the product is tradeable.
    pub tradeable: bool,
    /// Launch date.
    pub launch_date: Option<DateTime<Utc>>,
    /// Settlement time (for options/futures).
    pub settlement_time: Option<DateTime<Utc>>,
    /// Strike price (for options).
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub strike_price: Option<Decimal>,
    /// Initial margin.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub initial_margin: Decimal,
    /// Maintenance margin.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub maintenance_margin: Decimal,
    /// Maker fee rate.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub maker_commission_rate: Decimal,
    /// Taker fee rate.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub taker_commission_rate: Decimal,
    /// Liquidation penalty rate.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub liquidation_penalty_rate: Decimal,
}

/// Represents a Delta Exchange order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeOrder {
    /// Order ID.
    pub id: u64,
    /// User ID.
    pub user_id: u64,
    /// Product ID.
    pub product_id: u64,
    /// Product symbol.
    pub product_symbol: Ustr,
    /// Order size.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    /// Unfilled size.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub unfilled_size: Decimal,
    /// Order side.
    pub side: DeltaExchangeSide,
    /// Order type.
    pub order_type: DeltaExchangeOrderType,
    /// Limit price (for limit orders).
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub limit_price: Option<Decimal>,
    /// Stop price (for stop orders).
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub stop_price: Option<Decimal>,
    /// Paid price (average fill price).
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub paid_price: Option<Decimal>,
    /// Order state.
    pub state: DeltaExchangeOrderState,
    /// Time in force.
    pub time_in_force: DeltaExchangeTimeInForce,
    /// Post only flag.
    pub post_only: bool,
    /// Reduce only flag.
    pub reduce_only: bool,
    /// Client order ID.
    #[serde(deserialize_with = "parse_empty_string_as_none")]
    pub client_order_id: Option<String>,
    /// Order creation time.
    pub created_at: DateTime<Utc>,
    /// Order update time.
    pub updated_at: DateTime<Utc>,
}

/// Represents a Delta Exchange position.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangePosition {
    /// User ID.
    pub user_id: u64,
    /// Product ID.
    pub product_id: u64,
    /// Product symbol.
    pub product_symbol: Ustr,
    /// Position size (positive for long, negative for short).
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    /// Entry price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub entry_price: Option<Decimal>,
    /// Mark price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// Unrealized PnL.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub unrealized_pnl: Decimal,
    /// Realized PnL.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub realized_pnl: Decimal,
    /// Margin.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub margin: Decimal,
    /// Maintenance margin.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub maintenance_margin: Decimal,
    /// Liquidation price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub liquidation_price: Option<Decimal>,
    /// Position creation time.
    pub created_at: DateTime<Utc>,
    /// Position update time.
    pub updated_at: DateTime<Utc>,
}

/// Represents a Delta Exchange wallet balance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeBalance {
    /// Asset ID.
    pub asset_id: u64,
    /// Asset symbol.
    pub asset_symbol: Ustr,
    /// Available balance.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub available_balance: Decimal,
    /// Balance on orders.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub order_margin: Decimal,
    /// Position margin.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub position_margin: Decimal,
    /// Commission.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub commission: Decimal,
    /// Withdrawal pending.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub withdrawal_pending: Decimal,
    /// Deposit pending.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub deposit_pending: Decimal,
    /// Total balance.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub balance: Decimal,
}

/// Represents a Delta Exchange fill/trade.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeFill {
    /// Fill ID.
    pub id: u64,
    /// User ID.
    pub user_id: u64,
    /// Order ID.
    pub order_id: u64,
    /// Product ID.
    pub product_id: u64,
    /// Product symbol.
    pub product_symbol: Ustr,
    /// Fill size.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    /// Fill price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    /// Order side.
    pub side: DeltaExchangeSide,
    /// Commission paid.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub commission: Decimal,
    /// Realized PnL from this fill.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub realized_pnl: Decimal,
    /// Fill timestamp.
    pub created_at: DateTime<Utc>,
    /// Role (maker/taker).
    pub role: String,
    /// Client order ID.
    #[serde(deserialize_with = "parse_empty_string_as_none")]
    pub client_order_id: Option<String>,
}

/// Represents a Delta Exchange ticker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeTicker {
    /// Product symbol.
    pub symbol: Ustr,
    /// Last traded price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub price: Option<Decimal>,
    /// 24h price change.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub change_24h: Option<Decimal>,
    /// 24h high price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub high_24h: Option<Decimal>,
    /// 24h low price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub low_24h: Option<Decimal>,
    /// 24h volume.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub volume_24h: Option<Decimal>,
    /// Best bid price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub bid: Option<Decimal>,
    /// Best ask price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub ask: Option<Decimal>,
    /// Mark price.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub mark_price: Option<Decimal>,
    /// Open interest.
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub open_interest: Option<Decimal>,
    /// Timestamp.
    pub timestamp: u64,
}

/// Represents a Delta Exchange candle/OHLCV data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeCandle {
    /// Candle timestamp.
    pub time: u64,
    /// Open price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub open: Decimal,
    /// High price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub high: Decimal,
    /// Low price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub low: Decimal,
    /// Close price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub close: Decimal,
    /// Volume.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub volume: Decimal,
}

/// Represents a Delta Exchange public trade.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeTrade {
    /// Trade ID.
    pub id: u64,
    /// Product symbol.
    pub symbol: Ustr,
    /// Trade price.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    /// Trade size.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    /// Trade side (buyer side).
    pub buyer_role: String,
    /// Trade timestamp.
    pub timestamp: u64,
}

/// Represents order book level.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeOrderBookLevel {
    /// Price level.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    /// Size at this level.
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
}

/// Represents a Delta Exchange order book.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaExchangeOrderBook {
    /// Product symbol.
    pub symbol: Ustr,
    /// Buy levels (bids).
    pub buy: Vec<DeltaExchangeOrderBookLevel>,
    /// Sell levels (asks).
    pub sell: Vec<DeltaExchangeOrderBookLevel>,
    /// Last update ID.
    pub last_sequence_no: u64,
    /// Last update timestamp.
    pub last_updated_at: u64,
}
