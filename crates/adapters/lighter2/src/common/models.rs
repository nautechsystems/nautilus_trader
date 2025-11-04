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

//! Common data models for Lighter API responses.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::enums::{LighterInstrumentType, LighterOrderSide, LighterOrderStatus, LighterOrderType};

/// Lighter market/instrument information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterMarket {
    /// Market ID.
    pub id: u64,
    /// Market symbol (e.g., "BTC-USD").
    pub symbol: String,
    /// Base currency.
    pub base_currency: String,
    /// Quote currency.
    pub quote_currency: String,
    /// Instrument type.
    pub instrument_type: LighterInstrumentType,
    /// Minimum order size.
    pub min_order_size: Decimal,
    /// Maximum order size.
    pub max_order_size: Decimal,
    /// Tick size (price increment).
    pub tick_size: Decimal,
    /// Step size (quantity increment).
    pub step_size: Decimal,
    /// Price precision.
    pub price_precision: u8,
    /// Size precision.
    pub size_precision: u8,
    /// Whether the market is active.
    pub is_active: bool,
}

/// Lighter account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterAccount {
    /// Account ID.
    pub id: u64,
    /// L1 address.
    pub address: String,
    /// Account balances.
    pub balances: Vec<LighterBalance>,
}

/// Account balance for a currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterBalance {
    /// Currency symbol.
    pub currency: String,
    /// Available balance.
    pub available: Decimal,
    /// Total balance.
    pub total: Decimal,
    /// Locked balance (in orders).
    pub locked: Decimal,
}

/// Lighter order information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterOrder {
    /// Order ID.
    pub id: String,
    /// Client order ID.
    pub client_order_id: Option<String>,
    /// Market ID.
    pub market_id: u64,
    /// Order side.
    pub side: LighterOrderSide,
    /// Order type.
    pub order_type: LighterOrderType,
    /// Order price.
    pub price: Decimal,
    /// Order quantity.
    pub quantity: Decimal,
    /// Filled quantity.
    pub filled_quantity: Decimal,
    /// Remaining quantity.
    pub remaining_quantity: Decimal,
    /// Order status.
    pub status: LighterOrderStatus,
    /// Creation timestamp (Unix nanoseconds).
    pub created_at: i64,
    /// Update timestamp (Unix nanoseconds).
    pub updated_at: i64,
}

/// Lighter trade information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterTrade {
    /// Trade ID.
    pub id: String,
    /// Market ID.
    pub market_id: u64,
    /// Trade price.
    pub price: Decimal,
    /// Trade quantity.
    pub quantity: Decimal,
    /// Trade side (from taker's perspective).
    pub side: LighterOrderSide,
    /// Trade timestamp (Unix nanoseconds).
    pub timestamp: i64,
}

/// Lighter order book snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterOrderBook {
    /// Market ID.
    pub market_id: u64,
    /// Bid levels (price, quantity).
    pub bids: Vec<(Decimal, Decimal)>,
    /// Ask levels (price, quantity).
    pub asks: Vec<(Decimal, Decimal)>,
    /// Timestamp (Unix nanoseconds).
    pub timestamp: i64,
}

/// Lighter position information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LighterPosition {
    /// Market ID.
    pub market_id: u64,
    /// Position side.
    pub side: LighterOrderSide,
    /// Position size.
    pub size: Decimal,
    /// Entry price.
    pub entry_price: Decimal,
    /// Unrealized PnL.
    pub unrealized_pnl: Decimal,
    /// Realized PnL.
    pub realized_pnl: Decimal,
}
