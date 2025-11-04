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

//! Gate.io data models and structures.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Spot currency pair information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioSpotCurrencyPair {
    /// Currency pair ID
    pub id: String,
    /// Base currency
    pub base: String,
    /// Quote currency
    pub quote: String,
    /// Trading fee rate
    pub fee: String,
    /// Minimum base currency amount
    pub min_base_amount: String,
    /// Minimum quote currency amount
    pub min_quote_amount: String,
    /// Amount precision
    pub amount_precision: u8,
    /// Price precision
    pub precision: u8,
    /// Trading status (untradable, buyable, sellable, tradable)
    pub trade_status: String,
    /// Sell start time (Unix timestamp)
    #[serde(default)]
    pub sell_start: i64,
    /// Buy start time (Unix timestamp)
    #[serde(default)]
    pub buy_start: i64,
}

/// Futures contract information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioFuturesContract {
    /// Contract name
    pub name: String,
    /// Settlement currency
    #[serde(rename = "type")]
    pub contract_type: String,
    /// Contract size
    pub quanto_multiplier: String,
    /// Leverage multiplier (deprecated, use leverage_min and leverage_max)
    #[serde(default)]
    pub leverage_min: String,
    /// Maximum leverage
    #[serde(default)]
    pub leverage_max: String,
    /// Maintenance rate
    pub maintenance_rate: String,
    /// Mark price
    #[serde(default)]
    pub mark_price: String,
    /// Index price
    #[serde(default)]
    pub index_price: String,
    /// Last trade price
    #[serde(default)]
    pub last_price: String,
    /// Maker fee rate
    pub maker_fee_rate: String,
    /// Taker fee rate
    pub taker_fee_rate: String,
    /// Order price rounding
    pub order_price_round: String,
    /// Mark price rounding
    pub mark_price_round: String,
    /// Funding rate
    #[serde(default)]
    pub funding_rate: String,
    /// Order size minimum
    pub order_size_min: i64,
    /// Order size maximum
    pub order_size_max: i64,
    /// Order price deviation rate
    #[serde(default)]
    pub order_price_deviate: String,
    /// Funding interval (seconds)
    pub funding_interval: i64,
    /// In delisting state
    pub in_delisting: bool,
}

/// Account balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioBalance {
    /// Currency name
    pub currency: String,
    /// Available balance
    pub available: String,
    /// Locked balance
    pub locked: String,
}

/// Spot account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioSpotAccount {
    /// Account balances
    #[serde(default)]
    pub balances: Vec<GateioBalance>,
}

/// Futures account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioFuturesAccount {
    /// Total account balance
    pub total: String,
    /// Unrealized profit and loss
    pub unrealised_pnl: String,
    /// Position margin
    pub position_margin: String,
    /// Order margin
    pub order_margin: String,
    /// Available balance
    pub available: String,
    /// Point balance
    #[serde(default)]
    pub point: String,
    /// Currency
    pub currency: String,
    /// In dual mode
    #[serde(default)]
    pub in_dual_mode: bool,
    /// Enable credit
    #[serde(default)]
    pub enable_credit: bool,
    /// Position leverage
    #[serde(default)]
    pub position_leverage: String,
    /// Margin mode (cross or isolated)
    #[serde(default)]
    pub margin_mode: i32,
}

/// Order information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioOrder {
    /// Order ID
    pub id: String,
    /// User-defined order ID
    #[serde(default)]
    pub text: String,
    /// Currency pair or contract
    #[serde(default)]
    pub currency_pair: String,
    #[serde(default)]
    pub contract: String,
    /// Order status
    pub status: String,
    /// Order side (buy or sell)
    pub side: String,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: String,
    /// Order amount
    pub amount: String,
    /// Order price
    pub price: String,
    /// Time in force
    pub time_in_force: String,
    /// Filled amount
    #[serde(default)]
    pub filled_amount: String,
    /// Filled total
    #[serde(default)]
    pub filled_total: String,
    /// Average fill price
    #[serde(default)]
    pub avg_deal_price: String,
    /// Fee paid
    #[serde(default)]
    pub fee: String,
    /// Fee currency
    #[serde(default)]
    pub fee_currency: String,
    /// Create time (Unix timestamp in seconds)
    #[serde(default)]
    pub create_time: String,
    /// Update time (Unix timestamp in seconds)
    #[serde(default)]
    pub update_time: String,
}

/// Trade information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioTrade {
    /// Trade ID
    pub id: String,
    /// Create time (Unix timestamp)
    pub create_time: String,
    /// Currency pair or contract
    #[serde(default)]
    pub currency_pair: String,
    #[serde(default)]
    pub contract: String,
    /// Trade side (buy or sell)
    pub side: String,
    /// Trade role (taker or maker)
    #[serde(default)]
    pub role: String,
    /// Trade amount
    pub amount: String,
    /// Trade price
    pub price: String,
    /// Order ID
    #[serde(default)]
    pub order_id: String,
    /// Fee paid
    #[serde(default)]
    pub fee: String,
    /// Fee currency
    #[serde(default)]
    pub fee_currency: String,
    /// Point fee
    #[serde(default)]
    pub point_fee: String,
}

/// Order book level (price and quantity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioOrderBookLevel {
    /// Price
    #[serde(rename = "0")]
    pub price: String,
    /// Quantity
    #[serde(rename = "1")]
    pub quantity: String,
}

/// Order book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioOrderBook {
    /// Order book ID
    #[serde(default)]
    pub id: i64,
    /// Currency pair or contract
    #[serde(default)]
    pub currency_pair: String,
    #[serde(default)]
    pub contract: String,
    /// Current update timestamp
    #[serde(default)]
    pub current: i64,
    /// Update timestamp
    #[serde(default)]
    pub update: i64,
    /// Ask levels
    pub asks: Vec<GateioOrderBookLevel>,
    /// Bid levels
    pub bids: Vec<GateioOrderBookLevel>,
}

/// Position information (futures)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateioPosition {
    /// User ID
    #[serde(default)]
    pub user: i64,
    /// Contract name
    pub contract: String,
    /// Position size
    pub size: i64,
    /// Leverage
    #[serde(default)]
    pub leverage: String,
    /// Risk limit
    #[serde(default)]
    pub risk_limit: String,
    /// Leverage max
    #[serde(default)]
    pub leverage_max: String,
    /// Maintenance rate
    #[serde(default)]
    pub maintenance_rate: String,
    /// Position value
    #[serde(default)]
    pub value: String,
    /// Position margin
    #[serde(default)]
    pub margin: String,
    /// Entry price
    #[serde(default)]
    pub entry_price: String,
    /// Liquidation price
    #[serde(default)]
    pub liq_price: String,
    /// Mark price
    #[serde(default)]
    pub mark_price: String,
    /// Unrealized P&L
    #[serde(default)]
    pub unrealised_pnl: String,
    /// Realised P&L
    #[serde(default)]
    pub realised_pnl: String,
    /// Position mode (single, dual_long, dual_short)
    #[serde(default)]
    pub mode: String,
}
